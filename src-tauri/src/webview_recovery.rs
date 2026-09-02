//! WebView2 恢复：reload → recreate → restart 三级自救。

use crate::webview_guard::{self, HealthAction};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

const MAIN_LABEL: &str = "main";

/// 启动时是否进入托盘（仅「启动后最小化到托盘」全局设置）
static BOOT_TO_TRAY: AtomicBool = AtomicBool::new(false);
/// 当前会话用户是否处于托盘态（含关窗进托盘）；用于 WebView 恢复时保持托盘
static SESSION_IN_TRAY: AtomicBool = AtomicBool::new(false);

pub fn set_boot_to_tray(v: bool) {
    BOOT_TO_TRAY.store(v, Ordering::SeqCst);
}

pub fn boot_to_tray() -> bool {
    BOOT_TO_TRAY.load(Ordering::SeqCst)
}

#[cfg(test)]
pub fn set_session_in_tray(v: bool) {
    SESSION_IN_TRAY.store(v, Ordering::SeqCst);
}

pub fn session_in_tray() -> bool {
    SESSION_IN_TRAY.load(Ordering::SeqCst)
}

/// 是否应在 WebView reload/recreate 后回到托盘（启动策略或用户已进托盘）
fn prefer_stay_in_tray() -> bool {
    boot_to_tray() || session_in_tray()
}

/// 恢复 WebView2 渲染可见性（Windows hide/visible:false 后必须先 SetIsVisible）
pub fn reveal_webview(window: &WebviewWindow) {
    #[cfg(target_os = "windows")]
    {
        let w = window.clone();
        let _ = w.with_webview(move |webview| unsafe {
            let _ = webview.controller().SetIsVisible(true);
        });
    }
}

/// 最小化到托盘：show + minimize + 不占任务栏。
/// **禁止 hide()**：长期 hide 会让 Windows 回收 WebView2，导致白屏/黑屏。
pub fn minimize_main_to_tray(window: &WebviewWindow) {
    reveal_webview(window);
    let _ = window.show();
    let _ = window.minimize();
    let _ = window.set_skip_taskbar(true);
    SESSION_IN_TRAY.store(true, Ordering::SeqCst);
    log::info!("WINDOW: minimized to tray (skip_taskbar, no hide)");
}

/// 若主窗口已存在则立即进托盘，返回是否成功。
pub fn try_minimize_main_to_tray(app: &AppHandle) -> bool {
    if let Some(win) = app.get_webview_window(MAIN_LABEL) {
        minimize_main_to_tray(&win);
        true
    } else {
        false
    }
}

const SECOND_INSTANCE_TRAY_RETRY_MS: u64 = 200;
const SECOND_INSTANCE_TRAY_RETRY_MAX: u32 = 15;

/// 单实例二次启动：按「启动后最小化到托盘」保持托盘或弹出窗口。
pub fn apply_second_instance_policy(app: &AppHandle, start_minimized: bool) {
    if start_minimized {
        if try_minimize_main_to_tray(app) {
            log::info!("single-instance: start_minimized_to_tray=true, keep in tray");
        } else {
            log::warn!("single-instance: start_minimized_to_tray=true but main window not ready, retrying");
            schedule_minimize_main_when_ready(app.clone());
        }
    } else {
        restore_main_window(app);
        log::info!("single-instance: start_minimized_to_tray=false, restore window");
    }
}

/// 窗口尚未创建时，短轮询直至 minimize（二次启动早于 setup 完成）。
fn schedule_minimize_main_when_ready(app: AppHandle) {
    std::thread::Builder::new()
        .name("second-instance-tray".into())
        .spawn(move || {
            for _ in 0..SECOND_INSTANCE_TRAY_RETRY_MAX {
                std::thread::sleep(std::time::Duration::from_millis(
                    SECOND_INSTANCE_TRAY_RETRY_MS,
                ));
                if !crate::config::manager::ConfigManager::read_start_minimized_to_tray(&app) {
                    log::info!("single-instance tray retry: setting now false, abort");
                    return;
                }
                if try_minimize_main_to_tray(&app) {
                    log::info!("single-instance: deferred minimize to tray succeeded");
                    return;
                }
            }
            log::warn!("single-instance: timed out waiting for main window to minimize to tray");
        })
        .ok();
}

const STARTUP_TRAY_FALLBACK_MS: u64 = 800;
const STARTUP_SHOW_FALLBACK_MS: u64 = 400;

/// 冷启动时应用「启动后最小化到托盘」策略（设置 boot 标志 + 兜底线程）。
pub fn apply_startup_window_policy(_app: &AppHandle, start_to_tray: bool, window: &WebviewWindow) {
    set_boot_to_tray(start_to_tray);
    log::info!("START: start_minimized_to_tray={start_to_tray}");

    if start_to_tray {
        let win = window.clone();
        std::thread::Builder::new()
            .name("start-to-tray".into())
            .spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(STARTUP_TRAY_FALLBACK_MS));
                if boot_to_tray() {
                    minimize_main_to_tray(&win);
                }
            })
            .ok();
    } else {
        let win = window.clone();
        std::thread::Builder::new()
            .name("show-main-window".into())
            .spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(STARTUP_SHOW_FALLBACK_MS));
                if boot_to_tray() {
                    return;
                }
                reveal_webview(&win);
                let _ = win.set_skip_taskbar(false);
                let _ = win.show();
            })
            .ok();
    }
}

/// 还原主窗口到前台（托盘左键 / 菜单「打开状态」）
pub fn restore_main_window(app: &AppHandle) {
    // 用户主动打开窗口后，不再因 WebView 恢复/兜底线程重新缩回托盘
    set_boot_to_tray(false);
    SESSION_IN_TRAY.store(false, Ordering::SeqCst);
    if let Some(window) = app.get_webview_window(MAIN_LABEL) {
        let _ = window.set_skip_taskbar(false);
        reveal_webview(&window);
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// 前端就绪后显示窗口；若启动策略为托盘则直接 minimize_to_tray。
pub fn reveal_main_on_frontend_ready(app: &AppHandle) {
    let Some(window) = app.get_webview_window(MAIN_LABEL) else {
        log::warn!("reveal_main_on_frontend_ready: main window not found");
        return;
    };
    if boot_to_tray() {
        minimize_main_to_tray(&window);
    } else {
        let _ = window.set_skip_taskbar(false);
        reveal_webview(&window);
        let _ = window.show();
    }
}

/// 关闭到托盘：minimize + skip_taskbar（不用 hide）
pub fn attach_main_window_close_handler(app: &AppHandle, window: &WebviewWindow) {
    let app_handle = app.clone();
    let window_ = window.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            let minimize = app_handle
                .try_state::<crate::config::manager::ConfigManager>()
                .and_then(|m| m.get_global_settings().ok())
                .map(|s| s.minimize_to_tray)
                .unwrap_or(true);
            if minimize {
                api.prevent_close();
                minimize_main_to_tray(&window_);
            } else {
                // 关窗即退出（托盘仍存活时须主动 exit）
                api.prevent_close();
                crate::ipc::tray::quit_app_public(&app_handle);
            }
        }
    });
}

/// 尝试 reload 主窗口
pub fn try_reload_main(app: &AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window(MAIN_LABEL)
        .ok_or_else(|| "main window not found".to_string())?;
    let stay_in_tray = prefer_stay_in_tray();
    if stay_in_tray {
        reveal_webview(&window);
    } else {
        restore_main_window(app);
    }
    window.reload().map_err(|e| format!("WebView2 error: {e}"))?;
    if stay_in_tray {
        minimize_main_to_tray(&window);
    }
    Ok(())
}

fn build_main_window(app: &AppHandle) -> Result<WebviewWindow, String> {
    WebviewWindowBuilder::new(app, MAIN_LABEL, WebviewUrl::App("index.html".into()))
        .title("Voice VibeCoding")
        .inner_size(1080.0, 920.0)
        .min_inner_size(880.0, 720.0)
        .resizable(true)
        .center()
        .decorations(true)
        .visible(false)
        .build()
        .map_err(|e| e.to_string())
}

/// 销毁并重建主窗口（reload 无法复活僵尸 WebView2 时的唯一手段）
pub fn recreate_main_window(app: &AppHandle) -> Result<(), String> {
    if let Some(old) = app.get_webview_window(MAIN_LABEL) {
        log::warn!("WEBVIEW RECOVERY: destroying zombie main window");
        old.destroy()
            .map_err(|e| format!("destroy main window failed: {e}"))?;
        // 给 WebView2 进程一点时间退出
        std::thread::sleep(std::time::Duration::from_millis(300));
    }

    let window = build_main_window(app)?;
    attach_main_window_close_handler(app, &window);
    if prefer_stay_in_tray() {
        minimize_main_to_tray(&window);
    } else {
        reveal_webview(&window);
        let _ = window.show();
    }
    webview_guard::on_recreated();
    log::info!("WEBVIEW RECOVERY: main window recreated");
    Ok(())
}

/// reload → recreate 两级恢复
pub fn apply_health_action(app: &AppHandle, action: HealthAction) {
    match action {
        HealthAction::None => {}
        HealthAction::Reload => {
            log::warn!("WEBVIEW GUARD: reloading main window (rendering suspected dead)");
            match try_reload_main(app) {
                Ok(()) => log::info!("WEBVIEW GUARD: reload succeeded"),
                Err(e) => {
                    log::error!("WEBVIEW GUARD: failed to reload: {e}");
                    webview_guard::note_reload_failed();
                    if webview_guard::needs_recreate() {
                        if let Err(e2) = recreate_main_window(app) {
                            log::error!("WEBVIEW GUARD: recreate after reload failed: {e2}");
                        }
                    }
                }
            }
        }
        HealthAction::Recreate => {
            log::warn!("WEBVIEW GUARD: recreating main window (reload ineffective)");
            if let Err(e) = recreate_main_window(app) {
                log::error!("WEBVIEW GUARD: recreate failed: {e}");
            }
        }
    }
}

/// 托盘「刷新界面」：先 reload，失败则 recreate
pub fn manual_refresh_ui(app: &AppHandle) {
    log::info!("TRAY: manual refresh UI requested");
    restore_main_window(app);
    match try_reload_main(app) {
        Ok(()) => log::info!("TRAY: refresh reload succeeded"),
        Err(e) => {
            log::warn!("TRAY: refresh reload failed ({e}), trying recreate");
            webview_guard::note_reload_failed();
            if let Err(e2) = recreate_main_window(app) {
                log::error!("TRAY: refresh recreate failed: {e2}");
            }
        }
    }
}

/// 托盘「重启软件」：清理桥接/HID/音频后 relaunch
pub fn restart_application(app: &AppHandle) {
    log::info!("TRAY: restarting application");
    if let Some(runtime) =
        app.try_state::<std::sync::Arc<crate::bridges::xiaomi::connect::XiaomiRuntime>>()
    {
        runtime.request_stop();
    }
    crate::bridges::xiaomi::hid_report_tap::stop_and_join();
    crate::bridges::xiaomi::special_keys::stop_special_key_hook();
    crate::audio::pcm_router::stop_audio_router_process();
    std::thread::sleep(std::time::Duration::from_millis(150));
    app.restart();
}

#[cfg(test)]
mod tests {
    #[test]
    fn main_label_constant() {
        assert_eq!(super::MAIN_LABEL, "main");
    }

    #[test]
    fn boot_to_tray_flag() {
        super::set_boot_to_tray(true);
        assert!(super::boot_to_tray());
        super::set_boot_to_tray(false);
        assert!(!super::boot_to_tray());
    }

    #[test]
    fn session_in_tray_survives_boot_clear() {
        super::set_boot_to_tray(false);
        super::set_session_in_tray(true);
        assert!(super::session_in_tray());
        super::set_session_in_tray(false);
        assert!(!super::session_in_tray());
    }

    #[test]
    fn prefer_stay_in_tray_combines_boot_and_session() {
        super::set_boot_to_tray(true);
        super::set_session_in_tray(false);
        assert!(super::boot_to_tray());
        super::set_boot_to_tray(false);
        super::set_session_in_tray(true);
        assert!(super::session_in_tray());
    }
}
