pub mod bridges;
pub mod config;
pub mod ipc;
pub mod audio;
pub mod logging;
pub mod app_update;
pub mod webview_guard;

use tauri::{Manager, RunEvent};

/// 退出前统一清理：停桥接 + HID Tap + 卸键盘钩子，避免进程残留
fn cleanup_on_exit(app: &tauri::AppHandle) {
    if let Some(runtime) =
        app.try_state::<std::sync::Arc<bridges::xiaomi::connect::XiaomiRuntime>>()
    {
        runtime.request_stop();
    }
    bridges::xiaomi::hid_report_tap::stop_and_join();
    bridges::xiaomi::special_keys::stop_special_key_hook();
}

/// 自启参数解析：`--minimized`（注册表 Run 键与 Startup 快捷方式均带此参数）。
/// 语义：自启时最小化到任务栏（保留渲染，避免 hide 造成 WebView2 白屏），
/// 用户点任务栏/托盘即可恢复。
///
/// ```
/// use remote_bridge_hub_lib::should_start_minimized;
///
/// assert!(should_start_minimized(&["app.exe".into(), "--minimized".into()]));
/// assert!(!should_start_minimized(&["app.exe".into()]));
/// assert!(!should_start_minimized(&["app.exe".into(), "xiaomi-hid-injector".into()]));
/// assert!(should_start_minimized(&["app.exe".into(), " --minimized ".into()]));
/// ```
pub fn should_start_minimized(args: &[String]) -> bool {
    args.iter().any(|a| a.trim() == "--minimized")
}

fn focus_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[cfg_attr(mobile, mobile_entry_point)]
pub fn run() {
    // single-instance 必须最先注册：二次启动时激活已有窗口并退出新进程
    let mut builder = tauri::Builder::default();
    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            focus_main_window(app);
        }));
    }

    builder
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // Initialize configuration + 单文件日志
            let config_manager = config::manager::ConfigManager::new(app.handle().clone())?;
            let log_path = logging::init(&config_manager.logs_dir());
            std::env::set_var("REMOTE_BRIDGE_LOG_PATH", &log_path);
            app.manage(config_manager);

            log::info!("Voice VibeCoding starting...");
            #[cfg(debug_assertions)]
            log::info!("build_profile=debug (开发包)");
            #[cfg(not(debug_assertions))]
            log::info!("build_profile=release");

            // Initialize bridge state
            let bridge_state = bridges::BridgeState::new();
            app.manage(bridge_state);

            // Xiaomi 连接运行时（停止信号）
            app.manage(std::sync::Arc::new(
                bridges::xiaomi::connect::XiaomiRuntime::new(),
            ));

            // 快捷键录制会话
            app.manage(bridges::shared::shortcut_capture::ShortcutCaptureSession::new());

            // Setup tray menu（必须 manage，否则 TrayIcon Drop 会摘掉托盘）
            let tray = ipc::tray::setup_tray(app.handle())?;
            app.manage(tray);

            // 语音电平/波形 UI 事件
            bridges::xiaomi::voice_meter::bind_app(app.handle().clone());
            bridges::xiaomi::conflict_guard::bind_app(app.handle().clone());

            if let Some(window) = app.get_webview_window("main") {
                // 关闭窗口：minimize_to_tray=true 则隐藏；false 则真正退出
                let app_handle = app.handle().clone();
                let window_ = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        let minimize = app_handle
                            .try_state::<config::manager::ConfigManager>()
                            .and_then(|m| m.get_global_settings().ok())
                            .map(|s| s.minimize_to_tray)
                            .unwrap_or(true);
                        if minimize {
                            api.prevent_close();
                            let _ = window_.hide();
                        }
                        // else: 允许关闭 → 触发 Exit → cleanup_on_exit
                    }
                });

                // 启动后最小化到托盘（用户设置）或 --minimized（自启参数）
                let start_hidden = app
                    .try_state::<config::manager::ConfigManager>()
                    .and_then(|m| m.get_global_settings().ok())
                    .map(|s| s.start_minimized_to_tray)
                    .unwrap_or(false);
                let auto_minimized =
                    should_start_minimized(&std::env::args().collect::<Vec<_>>());

                if start_hidden {
                    // 用户设置：窗口不显示，仅托盘常驻（visible:false 已使窗口隐藏）
                    log::info!("START: start_minimized_to_tray=true, window stays hidden");
                } else if auto_minimized {
                    // 自启参数：最小化到任务栏
                    log::info!("START: --minimized detected, minimizing window to taskbar");
                    let win = window.clone();
                    std::thread::Builder::new()
                        .name("start-minimized".into())
                        .spawn(move || {
                            std::thread::sleep(std::time::Duration::from_millis(600));
                            let _ = win.minimize();
                        })?;
                } else {
                    // 正常启动：等 WebView 加载完成后显示窗口（避免闪烁）
                    let win = window.clone();
                    std::thread::Builder::new()
                        .name("show-main-window".into())
                        .spawn(move || {
                            std::thread::sleep(std::time::Duration::from_millis(1000));
                            let _ = win.show();
                        })?;
                }
            }

            // 独立 audio_router 子进程（对齐 Python --role audio）
            std::env::set_var("REMOTE_BRIDGE_PCM_PORT", "31680");
            if let Err(e) = audio::pcm_router::spawn_audio_router_process() {
                log::warn!("audio router spawn failed: {e}");
                bridges::xiaomi::conflict_guard::emit_if_conflicts(
                    "pcm_port",
                    &format!("语音路由启动失败: {e}"),
                    true,
                );
            } else {
                // 路由起来后立刻预热 UDP，避免首句语音才 PING
                bridges::xiaomi::voice_pcm::warmup_async();
                bridges::xiaomi::conflict_guard::check_audio_router_after_spawn(app.handle());
            }

            // 启动后自动连接 + 断线重连（对齐 Python worker 循环）
            let auto_app = app.handle().clone();
            std::thread::Builder::new()
                .name("xiaomi-auto-connect".into())
                .spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    if let (Some(config_manager), Some(runtime)) = (
                        auto_app.try_state::<config::manager::ConfigManager>(),
                        auto_app
                            .try_state::<std::sync::Arc<bridges::xiaomi::connect::XiaomiRuntime>>(),
                    ) {
                        if runtime.running.load(std::sync::atomic::Ordering::SeqCst) {
                            return;
                        }
                        let cfg = config_manager.get_device_config("xiaomi").ok();
                        let retry = std::time::Duration::from_secs_f32(
                            cfg.as_ref().map(|c| c.retry_delay).unwrap_or(3.0).max(0.5),
                        );
                        let configured = cfg.and_then(|c| c.bluetooth_address);
                        runtime.clear_stop();
                        runtime
                            .running
                            .store(true, std::sync::atomic::Ordering::SeqCst);
                        ipc::commands::xiaomi_reconnect_loop_public(
                            auto_app.clone(),
                            std::sync::Arc::clone(&runtime),
                            configured,
                            retry,
                        );
                    }
                })?;

            // 启动后静默检查更新（有新版才向前端发事件）
            app_update::spawn_startup_check(app.handle().clone());

            // WebView2 健康守卫：前端每 5s 心跳；检测到渲染进程死亡自动 reload（修长时间运行后白屏）
            {
                let guard_app = app.handle().clone();
                std::thread::Builder::new()
                    .name("webview-guard".into())
                    .spawn(move || loop {
                        std::thread::sleep(std::time::Duration::from_secs(5));
                        if webview_guard::check_and_reload(std::time::Instant::now()) {
                            log::warn!("WEBVIEW GUARD: reloading main window (rendering suspected dead)");
                            if let Some(window) = guard_app.get_webview_window("main") {
                                let _ = window.reload();
                            }
                        }
                    })?;
            }

            log::info!("Voice VibeCoding started successfully");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ipc::commands::get_device_status,
            ipc::commands::start_bridge,
            ipc::commands::stop_bridge,
            ipc::commands::get_config,
            ipc::commands::save_config,
            ipc::commands::get_key_mappings,
            ipc::commands::update_key_mapping,
            ipc::commands::capture_shortcut_start,
            ipc::commands::capture_shortcut_stop,
            ipc::commands::capture_shortcut_poll,
            ipc::commands::get_audio_devices,
            ipc::commands::get_bridge_logs,
            ipc::commands::set_autostart,
            ipc::commands::get_autostart,
            ipc::commands::get_global_settings,
            ipc::commands::save_global_settings,
            ipc::commands::get_xiaomi_host_status,
            ipc::commands::get_xiaomi_voice_meter,
            ipc::commands::restart_xiaomi_bridge,
            ipc::commands::check_xiaomi_voice_env,
            ipc::commands::get_xiaomi_voice_env_status,
            ipc::commands::repair_xiaomi_voice_env,
            ipc::commands::open_logs_folder,
            ipc::commands::get_app_log,
            ipc::commands::open_app_log,
            ipc::commands::quit_application,
            ipc::commands::get_xiaomi_conflicts,
            ipc::commands::kill_xiaomi_conflicts,
            ipc::commands::retry_xiaomi_after_conflict_clear,
            ipc::commands::repair_xiaomi_atvv,
            ipc::update_cmds::check_app_update,
            ipc::update_cmds::get_app_update_state,
            ipc::update_cmds::ignore_app_update,
            ipc::commands::webview_ping,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            match event {
                RunEvent::ExitRequested { .. } | RunEvent::Exit => {
                    cleanup_on_exit(app_handle);
                }
                _ => {}
            }
        });
}

#[cfg(test)]
mod tests {
    use super::should_start_minimized;

    #[test]
    fn minimized_flag_detected() {
        assert!(should_start_minimized(&["app.exe".into(), "--minimized".into()]));
        assert!(should_start_minimized(&["--minimized".into()]));
    }

    #[test]
    fn no_flag_when_absent() {
        assert!(!should_start_minimized(&["app.exe".into()]));
        assert!(!should_start_minimized(&[]));
        assert!(!should_start_minimized(&["app.exe".into(), "--other".into()]));
        assert!(!should_start_minimized(&["app.exe".into(), "-minimized".into()]));
        assert!(!should_start_minimized(&["app.exe".into(), "xiaomi-hid-injector".into()]));
    }

    #[test]
    fn whitespace_tolerated() {
        assert!(should_start_minimized(&["app.exe".into(), " --minimized ".into()]));
    }
}
