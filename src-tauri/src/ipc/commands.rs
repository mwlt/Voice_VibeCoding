//! Tauri IPC 命令 — 前端调用后端的所有接口

use crate::bridges::xiaomi::connect::{self, XiaomiRuntime};
use crate::bridges::{BridgeState, BridgeStatus, BridgeType, DeviceInfo};
use crate::config::manager::{ConfigManager, DeviceConfig, GlobalSettings, KeyAction};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

// ============================================================
// 前端请求/响应类型
// ============================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct AudioDevice {
    pub name: String,
    pub id: String,
    pub is_input: bool,
    pub is_default: bool,
}

// ============================================================
// 命令实现
// ============================================================

/// 获取指定设备的连接状态
#[tauri::command]
pub async fn get_device_status(
    bridge_type: String,
    state: State<'_, BridgeState>,
) -> Result<DeviceInfo, String> {
    let bt = parse_bridge_type(&bridge_type)?;
    Ok(state.get_info(bt))
}

/// 启动桥接连接
#[tauri::command]
pub async fn start_bridge(
    bridge_type: String,
    app: AppHandle,
    state: State<'_, BridgeState>,
    config_manager: State<'_, ConfigManager>,
) -> Result<(), String> {
    let bt = parse_bridge_type(&bridge_type)?;
    state.update_status(bt, BridgeStatus::Connecting);

    match bt {
        BridgeType::Xiaomi => start_xiaomi_bridge(app, &state, &config_manager).await,
        BridgeType::T1 | BridgeType::Hanvon => {
            // 其他设备后续接入；避免假成功
            let msg = format!("{bt} 连接逻辑尚未接入");
            state.update_status(bt, BridgeStatus::Error(msg.clone()));
            Err(msg)
        }
    }
}

async fn start_xiaomi_bridge(
    app: AppHandle,
    state: &BridgeState,
    config_manager: &ConfigManager,
) -> Result<(), String> {
    let runtime = app.state::<Arc<XiaomiRuntime>>();
    if runtime.running.load(std::sync::atomic::Ordering::SeqCst) {
        return Err("小米桥接已在运行".into());
    }
    runtime.clear_stop();
    runtime
        .running
        .store(true, std::sync::atomic::Ordering::SeqCst);

    let config = config_manager.get_device_config("xiaomi")?;
    let retry = std::time::Duration::from_secs_f32(config.retry_delay.max(0.5));
    let configured = config.bluetooth_address.clone();

    let runtime = Arc::clone(&runtime);
    let app_handle = app.clone();

    std::thread::Builder::new()
        .name("xiaomi-worker".into())
        .spawn(move || {
            xiaomi_reconnect_loop(app_handle, runtime, configured, retry);
        })
        .map_err(|e| format!("启动小米 worker 失败: {e}"))?;

    let _ = state; // 状态由 worker 更新
    Ok(())
}

/// 供 lib 自动连接复用的公开入口
pub fn xiaomi_reconnect_loop_public(
    app: AppHandle,
    runtime: Arc<XiaomiRuntime>,
    configured: Option<String>,
    retry: std::time::Duration,
) {
    xiaomi_reconnect_loop(app, runtime, configured, retry);
}

/// 对齐 Python `atvv_live_bridge.run`：断线后按 retry_delay 自动重连
fn xiaomi_reconnect_loop(
    app: AppHandle,
    runtime: Arc<XiaomiRuntime>,
    mut configured: Option<String>,
    retry: std::time::Duration,
) {
    while !runtime.should_stop() {
        if let Some(state) = app.try_state::<BridgeState>() {
            state.update_status(BridgeType::Xiaomi, BridgeStatus::Connecting);
        }

        let addr = configured.clone();
        let connect_result = connect::discover_and_connect(addr.as_deref());
        let connection = match connect_result {
            Ok(c) => c,
            Err(e) => {
                log::warn!("Xiaomi connect failed: {e}; retry in {retry:?}");
                if let Some(state) = app.try_state::<BridgeState>() {
                    if !runtime.should_stop() {
                        state.update_status(
                            BridgeType::Xiaomi,
                            BridgeStatus::Error(format!("{e}（将自动重试）")),
                        );
                    }
                }
                if wait_interruptible(&runtime, retry) {
                    break;
                }
                continue;
            }
        };

        configured = Some(connection.address.clone());
        if let Some(mgr) = app.try_state::<ConfigManager>() {
            if let Ok(mut cfg) = mgr.get_device_config("xiaomi") {
                cfg.bluetooth_address = Some(connection.address.clone());
                let _ = mgr.save_device_config("xiaomi", &cfg);
            }
        }
        if let Some(state) = app.try_state::<BridgeState>() {
            state.update_device_info(
                BridgeType::Xiaomi,
                Some(connection.name.clone()),
                Some(connection.address.clone()),
                None,
            );
            state.update_status(BridgeType::Xiaomi, BridgeStatus::Connected);
        }
        log::info!(
            "Xiaomi bridge ready name={} address={}",
            connection.name,
            connection.address
        );

        let result =
            connect::monitor_connection(&connection, Arc::clone(&runtime), Some(app.clone()));
        crate::bridges::xiaomi::special_keys::stop_special_key_hook();
        crate::bridges::xiaomi::voice_pcm::stop();

        if runtime.should_stop() {
            if let Some(state) = app.try_state::<BridgeState>() {
                state.update_status(BridgeType::Xiaomi, BridgeStatus::Disconnected);
            }
            break;
        }

        match result {
            Ok(()) => log::info!("Xiaomi monitor ended; reconnecting..."),
            Err(e) => log::warn!("Xiaomi disconnected: {e}; reconnecting..."),
        }
        if let Some(state) = app.try_state::<BridgeState>() {
            state.update_status(
                BridgeType::Xiaomi,
                BridgeStatus::Connecting,
            );
        }
        if wait_interruptible(&runtime, retry) {
            break;
        }
    }
    runtime
        .running
        .store(false, std::sync::atomic::Ordering::SeqCst);
}

fn wait_interruptible(runtime: &XiaomiRuntime, total: std::time::Duration) -> bool {
    let slice = std::time::Duration::from_millis(200);
    let mut left = total;
    while left > std::time::Duration::ZERO {
        if runtime.should_stop() {
            return true;
        }
        let step = left.min(slice);
        std::thread::sleep(step);
        left = left.saturating_sub(step);
    }
    runtime.should_stop()
}

/// 停止桥接连接
#[tauri::command]
pub async fn stop_bridge(
    bridge_type: String,
    app: AppHandle,
    state: State<'_, BridgeState>,
) -> Result<(), String> {
    let bt = parse_bridge_type(&bridge_type)?;
    if bt == BridgeType::Xiaomi {
        if let Some(runtime) = app.try_state::<Arc<XiaomiRuntime>>() {
            runtime.request_stop();
        }
    }
    state.update_status(bt, BridgeStatus::Disconnected);
    Ok(())
}

/// 获取设备配置
#[tauri::command]
pub async fn get_config(
    bridge_type: String,
    config_manager: State<'_, ConfigManager>,
) -> Result<DeviceConfig, String> {
    let device = bridge_type_to_device(&bridge_type)?;
    config_manager.get_device_config(device)
}

/// 保存设备配置
#[tauri::command]
pub async fn save_config(
    bridge_type: String,
    config: DeviceConfig,
    config_manager: State<'_, ConfigManager>,
) -> Result<(), String> {
    let device = bridge_type_to_device(&bridge_type)?;
    config_manager.save_device_config(device, &config)
}

/// 获取按键映射
#[tauri::command]
pub async fn get_key_mappings(
    bridge_type: String,
    config_manager: State<'_, ConfigManager>,
) -> Result<HashMap<String, KeyAction>, String> {
    let device = bridge_type_to_device(&bridge_type)?;
    let config = config_manager.get_device_config(device)?;
    Ok(config.button_bindings)
}

/// 更新单个按键映射
#[tauri::command]
pub async fn update_key_mapping(
    bridge_type: String,
    button_id: String,
    action: KeyAction,
    config_manager: State<'_, ConfigManager>,
) -> Result<(), String> {
    let device = bridge_type_to_device(&bridge_type)?;
    let mut config = config_manager.get_device_config(device)?;
    config.button_bindings.insert(button_id, action);
    config_manager.save_device_config(device, &config)
}

/// 开始快捷键捕获（物理键会被临时吞掉，松手/按完后自动完成）
#[tauri::command]
pub async fn capture_shortcut_start(
    app: AppHandle,
    session: State<'_, crate::bridges::shared::shortcut_capture::ShortcutCaptureSession>,
) -> Result<(), String> {
    session.start(app)?;
    log::info!("Shortcut capture started");
    Ok(())
}

/// 取消快捷键捕获
#[tauri::command]
pub async fn capture_shortcut_stop(
    session: State<'_, crate::bridges::shared::shortcut_capture::ShortcutCaptureSession>,
) -> Result<Vec<u32>, String> {
    session.cancel()?;
    log::info!("Shortcut capture cancelled");
    Ok(vec![])
}

/// 轮询取出录制结果（事件可能丢失时的兜底；取出后清空）
#[tauri::command]
pub async fn capture_shortcut_poll(
    session: State<'_, crate::bridges::shared::shortcut_capture::ShortcutCaptureSession>,
) -> Result<Option<crate::bridges::shared::shortcut_capture::ShortcutCapturedPayload>, String> {
    Ok(session.take_result())
}

/// 获取音频设备列表
#[tauri::command]
pub async fn get_audio_devices() -> Result<Vec<AudioDevice>, String> {
    Ok(vec![])
}

/// 获取桥接日志
#[tauri::command]
pub async fn get_bridge_logs(bridge_type: String) -> Result<Vec<String>, String> {
    let _ = parse_bridge_type(&bridge_type)?;
    Ok(vec![])
}

/// 设置开机自启
#[tauri::command]
pub async fn set_autostart(
    enable: bool,
    config_manager: State<'_, ConfigManager>,
) -> Result<(), String> {
    crate::bridges::xiaomi::autostart::set_autostart_enabled(enable)?;
    let mut settings = config_manager.get_global_settings()?;
    settings.autostart = enable;
    config_manager.save_global_settings(&settings)
}

/// 获取开机自启状态
#[tauri::command]
pub async fn get_autostart(
    config_manager: State<'_, ConfigManager>,
) -> Result<bool, String> {
    let settings = config_manager.get_global_settings()?;
    Ok(settings.autostart || crate::bridges::xiaomi::autostart::is_autostart_enabled())
}

/// 获取全局设置
#[tauri::command]
pub async fn get_global_settings(
    config_manager: State<'_, ConfigManager>,
) -> Result<GlobalSettings, String> {
    config_manager.get_global_settings()
}

/// 保存全局设置
#[tauri::command]
pub async fn save_global_settings(
    settings: GlobalSettings,
    config_manager: State<'_, ConfigManager>,
) -> Result<(), String> {
    crate::bridges::xiaomi::autostart::set_autostart_enabled(settings.autostart)?;
    config_manager.save_global_settings(&settings)
}

// ============================================================
// 对齐 Python xiaomi_main 主窗口：状态 / 重启 / 日志 / 退出
// ============================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct XiaomiHostStatusItem {
    pub id: String,
    pub label: String,
    pub state_label: String,
    /// ok | warn | error
    pub tone: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct XiaomiHostStatus {
    pub bridge_alive: bool,
    pub audio_alive: bool,
    pub cable_ready: bool,
    pub status_text: String,
    pub detail: String,
    /// ok | warn | error
    pub tone: String,
    pub items: Vec<XiaomiHostStatusItem>,
}

/// 对齐 Python `_poll`：按键桥接 + 语音路由 + VB-CABLE
#[tauri::command]
pub async fn get_xiaomi_host_status(app: AppHandle) -> Result<XiaomiHostStatus, String> {
    Ok(xiaomi_host_status_now(&app))
}

/// BLE / 虚拟声卡输送电平快照（页面初次打开兜底）
#[tauri::command]
pub async fn get_xiaomi_voice_meter(
) -> Result<crate::bridges::xiaomi::voice_meter::VoiceMeterSnapshot, String> {
    Ok(crate::bridges::xiaomi::voice_meter::current_snapshot())
}

pub fn xiaomi_host_status_now(app: &AppHandle) -> XiaomiHostStatus {
    let bridge_alive = app
        .try_state::<Arc<XiaomiRuntime>>()
        .map(|r| r.running.load(std::sync::atomic::Ordering::SeqCst))
        .unwrap_or(false);
    let audio_alive = crate::audio::pcm_router::audio_router_ready()
        || crate::audio::pcm_router::audio_router_process_alive();
    let cable_ready = crate::audio::vb_cable::voice_env_status().ready;

    let items = vec![
        XiaomiHostStatusItem {
            id: "cable".into(),
            label: "虚拟声卡".into(),
            state_label: if cable_ready {
                "已安装".into()
            } else {
                "未检测到".into()
            },
            tone: if cable_ready {
                "ok".into()
            } else {
                "error".into()
            },
        },
        XiaomiHostStatusItem {
            id: "audio".into(),
            label: "语音路由".into(),
            state_label: if audio_alive {
                "运行中".into()
            } else {
                "已停止".into()
            },
            tone: if audio_alive {
                "ok".into()
            } else {
                "error".into()
            },
        },
        XiaomiHostStatusItem {
            id: "bridge".into(),
            label: "按键桥接".into(),
            state_label: if bridge_alive {
                "监听中".into()
            } else {
                "未启动".into()
            },
            tone: if bridge_alive {
                "ok".into()
            } else {
                "error".into()
            },
        },
    ];

    let (status_text, detail, tone) = if bridge_alive && audio_alive && cable_ready {
        (
            "运行正常".into(),
            String::new(),
            "ok".into(),
        )
    } else if !cable_ready {
        (
            "语音环境未就绪".into(),
            "未检测到 VB-CABLE。可点「虚拟声卡检测与修复」安装或修复。".into(),
            "warn".into(),
        )
    } else if bridge_alive && !audio_alive {
        (
            "语音路由未就绪".into(),
            "可点「重启桥接」或「虚拟声卡检测与修复」后重试。".into(),
            "warn".into(),
        )
    } else if !bridge_alive {
        (
            "桥接未运行".into(),
            "可点「重启桥接」或打开日志检查。".into(),
            "error".into(),
        )
    } else {
        (
            "部分服务异常".into(),
            "可点「重启桥接」或「虚拟声卡检测与修复」。".into(),
            "warn".into(),
        )
    };

    XiaomiHostStatus {
        bridge_alive,
        audio_alive,
        cable_ready,
        status_text,
        detail,
        tone,
        items,
    }
}

/// 对齐 Python `workers.restart_bridge`
#[tauri::command]
pub async fn restart_xiaomi_bridge(
    app: AppHandle,
    state: State<'_, BridgeState>,
    config_manager: State<'_, ConfigManager>,
) -> Result<(), String> {
    restart_xiaomi_bridge_inner(&app, &state, &config_manager)
}

pub fn restart_xiaomi_bridge_inner(
    app: &AppHandle,
    state: &BridgeState,
    config_manager: &ConfigManager,
) -> Result<(), String> {
    log::info!("XIAOMI host: restart bridge requested");
    append_host_log(config_manager, "bridge restart requested");

    // 仅停 BLE worker；HID Tap 为进程级单例，重启不解绑 30684（避免自占用）
    if let Some(runtime) = app.try_state::<Arc<XiaomiRuntime>>() {
        runtime.request_stop();
        // 等旧 worker 退出
        for _ in 0..50 {
            if !runtime.running.load(std::sync::atomic::Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    // 语音路由挂了则一并拉起（比 Python 更稳，不破坏 restart_bridge 语义）
    if !crate::audio::pcm_router::audio_router_ready() {
        if let Err(e) = crate::audio::pcm_router::spawn_audio_router_process() {
            log::warn!("audio router respawn on restart: {e}");
            append_host_log(config_manager, &format!("audio respawn failed: {e}"));
        } else {
            crate::bridges::xiaomi::voice_pcm::warmup_async();
        }
    }

    let runtime = app
        .try_state::<Arc<XiaomiRuntime>>()
        .ok_or_else(|| "XiaomiRuntime missing".to_string())?;
    if runtime.running.load(std::sync::atomic::Ordering::SeqCst) {
        return Err("旧桥接尚未退出，请稍后再试".into());
    }

    let config = config_manager.get_device_config("xiaomi")?;
    let retry = std::time::Duration::from_secs_f32(config.retry_delay.max(0.5));
    let configured = config.bluetooth_address.clone();

    runtime.clear_stop();
    runtime
        .running
        .store(true, std::sync::atomic::Ordering::SeqCst);
    state.update_status(BridgeType::Xiaomi, BridgeStatus::Connecting);

    let app_handle = app.clone();
    let runtime = Arc::clone(&runtime);
    std::thread::Builder::new()
        .name("xiaomi-worker".into())
        .spawn(move || {
            xiaomi_reconnect_loop(app_handle, runtime, configured, retry);
        })
        .map_err(|e| format!("重启 worker 失败: {e}"))?;

    append_host_log(config_manager, "bridge restart spawned");
    Ok(())
}

/// 检测小米语音环境（VB-CABLE）；已就绪则直接 Repair，否则前端弹出内嵌/下载选择
#[tauri::command]
pub async fn check_xiaomi_voice_env() -> Result<crate::audio::vb_cable::VoiceEnvActionResult, String> {
    Ok(tokio::task::spawn_blocking(crate::audio::vb_cable::check_or_prompt)
        .await
        .map_err(|e| format!("voice env task: {e}"))?)
}

#[tauri::command]
pub async fn get_xiaomi_voice_env_status() -> Result<crate::audio::vb_cable::VoiceEnvStatus, String> {
    Ok(crate::audio::vb_cable::voice_env_status())
}

/// source: "embedded" | "download_page" | "download_zip"
#[tauri::command]
pub async fn repair_xiaomi_voice_env(
    source: String,
) -> Result<crate::audio::vb_cable::VoiceEnvActionResult, String> {
    let source = source.to_ascii_lowercase();
    tokio::task::spawn_blocking(move || match source.as_str() {
        "embedded" => crate::audio::vb_cable::install_embedded(),
        "download_page" => crate::audio::vb_cable::open_download_page(),
        "download_zip" => crate::audio::vb_cable::open_download_zip(),
        other => Err(format!("未知来源: {other}，可选 embedded / download_page / download_zip")),
    })
    .await
    .map_err(|e| format!("voice repair task: {e}"))?
}

/// 对齐 Python `open_logs`：打开日志目录
#[tauri::command]
pub async fn open_logs_folder(config_manager: State<'_, ConfigManager>) -> Result<(), String> {
    let dir = config_manager.logs_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&dir)
            .spawn()
            .map_err(|e| format!("打开日志目录失败: {e}"))?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = dir;
        return Err("仅支持 Windows".into());
    }
    Ok(())
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppLogPayload {
    pub path: String,
    pub content: String,
}

/// 读取运行日志（末尾一段，供界面展示/复制）
#[tauri::command]
pub async fn get_app_log() -> Result<AppLogPayload, String> {
    let path = crate::logging::log_path()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let content = crate::logging::read_log_text(80_000)?;
    Ok(AppLogPayload { path, content })
}

/// 用系统默认程序打开 app.log
#[tauri::command]
pub async fn open_app_log() -> Result<(), String> {
    crate::logging::open_log_in_editor()
}

/// 对齐 Python `exit`：真正退出进程（非托盘隐藏）
#[tauri::command]
pub async fn quit_application(app: AppHandle) -> Result<(), String> {
    crate::ipc::tray::quit_app_public(&app);
    Ok(())
}

fn append_host_log(_config_manager: &ConfigManager, message: &str) {
    crate::logging::append(message);
}

// ============================================================
// 辅助函数
// ============================================================

fn parse_bridge_type(s: &str) -> Result<BridgeType, String> {
    match s.to_lowercase().as_str() {
        "xiaomi" => Ok(BridgeType::Xiaomi),
        "t1" => Ok(BridgeType::T1),
        "hanvon" | "v60" => Ok(BridgeType::Hanvon),
        _ => Err(format!("未知设备类型: {}", s)),
    }
}

fn bridge_type_to_device(s: &str) -> Result<&str, String> {
    match s.to_lowercase().as_str() {
        "xiaomi" => Ok("xiaomi"),
        "t1" => Ok("t1"),
        "hanvon" | "v60" => Ok("hanvon"),
        _ => Err(format!("未知设备类型: {}", s)),
    }
}
