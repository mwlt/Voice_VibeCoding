pub mod bridges;
pub mod config;
pub mod ipc;
pub mod audio;
pub mod logging;

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
    audio::pcm_router::stop_audio_router_process();
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

            // 开发包窗口标题加标记，便于和正式安装包区分
            if let Some(window) = app.get_webview_window("main") {
                #[cfg(debug_assertions)]
                {
                    let _ = window.set_title("Voice VibeCoding [开发]");
                }

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
            }

            // 独立 audio_router 子进程（对齐 Python --role audio）
            std::env::set_var("REMOTE_BRIDGE_PCM_PORT", "31680");
            if let Err(e) = audio::pcm_router::spawn_audio_router_process() {
                log::warn!("audio router spawn failed: {e}");
            } else {
                // 路由起来后立刻预热 UDP，避免首句语音才 PING
                bridges::xiaomi::voice_pcm::warmup_async();
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
