//! 系统托盘 — 左键还原窗口，右键菜单

use std::sync::atomic::{AtomicU8, Ordering};

use tauri::{
    image::Image,
    menu::{Menu, MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder},
    tray::{MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager,
};

/// 托盘运行态：0=初始化(黄) 1=就绪(绿) 2=异常(红) 255=未应用
static TRAY_ICON_STATE: AtomicU8 = AtomicU8::new(255);

/// 托盘图标语义（窗口/桌面主图标不随此切换）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayIconKind {
    /// 设备/桥接尚未起来
    Init,
    /// 语音可用（运行正常）
    Ready,
    /// 已尝试运行但未完全就绪 / 出错
    Error,
}

impl TrayIconKind {
    fn code(self) -> u8 {
        match self {
            Self::Init => 0,
            Self::Ready => 1,
            Self::Error => 2,
        }
    }
}

fn load_tray_png(bytes: &'static [u8]) -> Result<Image<'static>, tauri::Error> {
    Image::from_bytes(bytes)
}

fn load_tray_icon(kind: TrayIconKind) -> Result<Image<'static>, tauri::Error> {
    match kind {
        TrayIconKind::Ready => load_tray_png(include_bytes!("../../icons/tray-icon.png"))
            .or_else(|_| load_tray_png(include_bytes!("../../icons/tray-icon-32.png"))),
        TrayIconKind::Init => load_tray_png(include_bytes!("../../icons/tray-icon-init.png"))
            .or_else(|_| load_tray_png(include_bytes!("../../icons/tray-icon-init-32.png"))),
        TrayIconKind::Error => load_tray_png(include_bytes!("../../icons/tray-icon-error.png"))
            .or_else(|_| load_tray_png(include_bytes!("../../icons/tray-icon-error-32.png"))),
    }
}

fn load_window_icon(_app: &AppHandle) -> Result<Image<'static>, tauri::Error> {
    // 左上角 / 任务栏：固定用软件主图标（docs/icon-3/主.png → icon.ico / icon.png）
    Image::from_bytes(include_bytes!("../../icons/icon.ico"))
        .or_else(|_| Image::from_bytes(include_bytes!("../../icons/icon.png")))
        .or_else(|_| Image::from_bytes(include_bytes!("../../icons/32x32.png")))
}

fn apply_window_icon(app: &AppHandle) {
    let Ok(icon) = load_window_icon(app) else {
        log::warn!("window icon load failed");
        return;
    };
    if let Some(win) = app.get_webview_window("main") {
        if let Err(e) = win.set_icon(icon) {
            log::warn!("set window icon failed: {e}");
        } else {
            log::info!("window/taskbar icon applied (main)");
        }
    }
}

fn tray_tooltip(kind: TrayIconKind) -> &'static str {
    match kind {
        TrayIconKind::Init => "Voice VibeCoding · 设备初始化中…",
        TrayIconKind::Ready => "Voice VibeCoding · 语音已就绪",
        TrayIconKind::Error => "Voice VibeCoding · 运行异常，请检查主机状态",
    }
}

/// 仅更新托盘图标；窗口/任务栏保持主图标。
pub fn sync_runtime_icons(app: &AppHandle, kind: TrayIconKind) {
    let next = kind.code();
    if TRAY_ICON_STATE.swap(next, Ordering::SeqCst) == next {
        return;
    }

    let Ok(icon) = load_tray_icon(kind) else {
        log::warn!("tray icon load failed kind={kind:?}");
        return;
    };

    if let Some(tray) = app.tray_by_id("main") {
        if let Err(e) = tray.set_icon(Some(icon)) {
            log::warn!("tray set_icon failed: {e}");
        }
        let _ = tray.set_tooltip(Some(tray_tooltip(kind)));
    }
    log::info!("tray icon -> {kind:?}");
}

/// 兼容旧调用：true=就绪绿，false=初始化黄
pub fn sync_runtime_icons_ready(app: &AppHandle, voice_ready: bool) {
    sync_runtime_icons(
        app,
        if voice_ready {
            TrayIconKind::Ready
        } else {
            TrayIconKind::Init
        },
    );
}

fn quit_app(app: &AppHandle) {
    // 先停桥接 / HID Tap / 钩子 / 音频子进程，避免托盘退出后 remote-bridge-hub.exe 残留
    if let Some(runtime) =
        app.try_state::<std::sync::Arc<crate::bridges::xiaomi::connect::XiaomiRuntime>>()
    {
        runtime.request_stop();
    }
    crate::bridges::xiaomi::hid_report_tap::stop_and_join();
    crate::bridges::xiaomi::special_keys::stop_special_key_hook();
    crate::audio::pcm_router::stop_audio_router_process();
    // 给后台线程一点时间退出 accept/GetMessage
    std::thread::sleep(std::time::Duration::from_millis(150));
    app.exit(0);
}

/// 供 IPC `quit_application` 调用
pub fn quit_app_public(app: &AppHandle) {
    quit_app(app);
}

fn build_tray_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    // 对齐 Python xiaomi_main 托盘：打开状态 / 按键与语音设置 / 重启桥接 / 退出
    let restore = MenuItemBuilder::with_id("restore", "打开状态").build(app)?;
    let settings = MenuItemBuilder::with_id("xiaomi_settings", "按键与语音设置").build(app)?;
    let restart = MenuItemBuilder::with_id("restart_bridge", "重启桥接").build(app)?;
    let refresh = MenuItemBuilder::with_id("refresh_ui", "刷新界面（白屏自救）").build(app)?;
    let restart_app = MenuItemBuilder::with_id("restart_app", "重启软件").build(app)?;
    let separator1 = PredefinedMenuItem::separator(app)?;

    let xiaomi_connect = MenuItemBuilder::with_id("xiaomi_connect", "连接小米遥控器").build(app)?;
    let xiaomi_disconnect =
        MenuItemBuilder::with_id("xiaomi_disconnect", "断开小米遥控器").build(app)?;
    let xiaomi_submenu = SubmenuBuilder::new(app, "小米遥控器")
        .item(&xiaomi_connect)
        .item(&xiaomi_disconnect)
        .build()?;

    let t1_connect = MenuItemBuilder::with_id("t1_connect", "连接 T1 遥控器").build(app)?;
    let t1_disconnect = MenuItemBuilder::with_id("t1_disconnect", "断开 T1 遥控器").build(app)?;
    let t1_submenu = SubmenuBuilder::new(app, "T1 遥控器")
        .item(&t1_connect)
        .item(&t1_disconnect)
        .build()?;

    let v60_connect = MenuItemBuilder::with_id("hanvon_connect", "连接 V60 语音笔").build(app)?;
    let v60_disconnect =
        MenuItemBuilder::with_id("hanvon_disconnect", "断开 V60 语音笔").build(app)?;
    let v60_submenu = SubmenuBuilder::new(app, "汉王 V60")
        .item(&v60_connect)
        .item(&v60_disconnect)
        .build()?;

    let separator2 = PredefinedMenuItem::separator(app)?;
    let quit = MenuItemBuilder::with_id("quit", "退出").build(app)?;

    MenuBuilder::new(app)
        .item(&restore)
        .item(&settings)
        .item(&restart)
        .item(&refresh)
        .item(&restart_app)
        .item(&separator1)
        .item(&xiaomi_submenu)
        .item(&t1_submenu)
        .item(&v60_submenu)
        .item(&separator2)
        .item(&quit)
        .build()
}

fn on_menu_event(app: &AppHandle, id: &str) {
    match id {
        "restore" | "show" => crate::webview_recovery::restore_main_window(app),
        "refresh_ui" => crate::webview_recovery::manual_refresh_ui(app),
        "restart_app" => crate::webview_recovery::restart_application(app),
        "xiaomi_settings" => {
            crate::webview_recovery::restore_main_window(app);
            let _ = app.emit("navigate", "/xiaomi");
        }
        "restart_bridge" => {
            let app = app.clone();
            std::thread::spawn(move || {
                let Some(state) = app.try_state::<crate::bridges::BridgeState>() else {
                    return;
                };
                let Some(config_manager) =
                    app.try_state::<crate::config::manager::ConfigManager>()
                else {
                    return;
                };
                if let Err(e) =
                    crate::ipc::commands::restart_xiaomi_bridge_inner(&app, &state, &config_manager)
                {
                    log::warn!("Tray restart bridge failed: {e}");
                }
                // 重启桥接后先显示初始化中
                crate::ipc::tray::sync_runtime_icons(&app, TrayIconKind::Init);
            });
        }
        "quit" => quit_app(app),
        "xiaomi_connect" => {
            log::info!("Tray: connecting Xiaomi");
            let app = app.clone();
            std::thread::spawn(move || {
                let Some(state) = app.try_state::<crate::bridges::BridgeState>() else {
                    return;
                };
                let Some(config_manager) =
                    app.try_state::<crate::config::manager::ConfigManager>()
                else {
                    return;
                };
                crate::ipc::tray::sync_runtime_icons(&app, TrayIconKind::Init);
                if let Err(e) =
                    crate::ipc::commands::restart_xiaomi_bridge_inner(&app, &state, &config_manager)
                {
                    log::warn!("Tray connect (restart) failed: {e}");
                }
            });
        }
        "xiaomi_disconnect" => {
            log::info!("Tray: disconnecting Xiaomi");
            if let Some(runtime) =
                app.try_state::<std::sync::Arc<crate::bridges::xiaomi::connect::XiaomiRuntime>>()
            {
                runtime.request_stop();
            }
            if let Some(state) = app.try_state::<crate::bridges::BridgeState>() {
                state.update_status(
                    crate::bridges::BridgeType::Xiaomi,
                    crate::bridges::BridgeStatus::Disconnected,
                );
            }
            // 断开后回到「初始化中」黄标，避免仍显示已就绪
            sync_runtime_icons(app, TrayIconKind::Init);
        }
        "t1_connect" => log::info!("Tray: connecting T1"),
        "t1_disconnect" => log::info!("Tray: disconnecting T1"),
        "hanvon_connect" => log::info!("Tray: connecting V60"),
        "hanvon_disconnect" => log::info!("Tray: disconnecting V60"),
        _ => {}
    }
}

/// 配置系统托盘图标和菜单；返回的 TrayIcon 必须由调用方 keep-alive（manage）
pub fn setup_tray(app: &AppHandle) -> Result<TrayIcon, Box<dyn std::error::Error>> {
    let menu = build_tray_menu(app)?;

    // 启动：托盘黄标；窗口/任务栏固定主图标
    let icon = load_tray_icon(TrayIconKind::Init)?;
    TRAY_ICON_STATE.store(TrayIconKind::Init.code(), Ordering::SeqCst);
    apply_window_icon(app);

    let tray = TrayIconBuilder::with_id("main")
        .icon(icon)
        .icon_as_template(false)
        .menu(&menu)
        .tooltip(tray_tooltip(TrayIconKind::Init))
        // 左键还原窗口；右键弹出菜单（Windows 默认）
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            on_menu_event(app, event.id().as_ref());
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: tauri::tray::MouseButtonState::Up,
                ..
            } = event
            {
                crate::webview_recovery::restore_main_window(tray.app_handle());
            }
        })
        .build(app)?;

    log::info!("System tray icon created (init/yellow)");
    Ok(tray)
}
