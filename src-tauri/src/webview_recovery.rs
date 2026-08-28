//! WebView2 恢复：reload → recreate → restart 三级自救。

use crate::webview_guard::{self, HealthAction};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

const MAIN_LABEL: &str = "main";

/// 启动时是否进入托盘（设置「启动后最小化到托盘」或 `--minimized`）
static BOOT_TO_TRAY: AtomicBool = AtomicBool::new(false);

pub fn set_boot_to_tray(v: bool) {
    BOOT_TO_TRAY.store(v, Ordering::SeqCst);
}

pub fn boot_to_tray() -> bool {
    BOOT_TO_TRAY.load(Ordering::SeqCst)
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
    log::info!("WINDOW: minimized to tray (skip_taskbar, no hide)");
}

/// 还原主窗口到前台（托盘左键 / 二次启动 / 菜单「打开状态」）
pub fn restore_main_window(app: &AppHandle) {
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
    if let Some(window) = app.get_webview_window(MAIN_LABEL) {
        if boot_to_tray() {
            minimize_main_to_tray(&window);
        } else {
            let _ = window.set_skip_taskbar(false);
            reveal_webview(&window);
            let _ = window.show();
        }
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
    restore_main_window(app);
    window.reload().map_err(|e| format!("WebView2 error: {e}"))
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
    reveal_webview(&window);
    let _ = window.show();
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
}
