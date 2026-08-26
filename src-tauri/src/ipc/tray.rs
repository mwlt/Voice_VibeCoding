//! 系统托盘 — 左键还原窗口，右键菜单

use tauri::{
    image::Image,
    menu::{Menu, MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder},
    tray::{MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager,
};

fn restore_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        // WebView2 可能处于隐藏状态（启动时 SetIsVisible(false)），先恢复渲染再显示窗口
        #[cfg(target_os = "windows")]
        {
            let w = window.clone();
            let _ = w.with_webview(move |webview| unsafe {
                webview.controller().SetIsVisible(true);
            });
        }
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
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
        "restore" | "show" => restore_main_window(app),
        "refresh_ui" => {
            // 白屏自救：强制重载主窗口（WebView2 渲染进程死亡时前端按钮已不可用，必须走后端）
            restore_main_window(app);
            if let Some(window) = app.get_webview_window("main") {
                log::info!("TRAY: manual refresh UI requested");
                let _ = window.reload();
            }
        }
        "xiaomi_settings" => {
            restore_main_window(app);
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

    // 优先用专用托盘图；失败再退回窗口默认图标
    let icon = Image::from_bytes(include_bytes!("../../icons/tray-icon.png"))
        .or_else(|_| Image::from_bytes(include_bytes!("../../icons/32x32.png")))
        .or_else(|_| {
            app.default_window_icon()
                .cloned()
                .ok_or_else(|| tauri::Error::FailedToReceiveMessage)
        })?;

    let tray = TrayIconBuilder::with_id("main")
        .icon(icon)
        .icon_as_template(false)
        .menu(&menu)
        .tooltip("Voice VibeCoding")
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
                restore_main_window(tray.app_handle());
            }
        })
        .build(app)?;

    log::info!("System tray icon created");
    Ok(tray)
}
