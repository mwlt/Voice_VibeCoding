//! T1 蓝牙桥接运行时（与 USB `runtime.rs` 完全独立）

use crate::bridges::t1::ble_connect::{self, T1BleConnection};
use crate::bridges::t1::ble_pcm;
use crate::bridges::t1::ble_session;
use crate::bridges::t1::ble_voice;
use crate::config::manager::{ConfigManager, KeyAction};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Default)]
pub struct T1BleRuntime {
    pub stop: AtomicBool,
    pub running: AtomicBool,
}

impl T1BleRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    pub fn clear_stop(&self) {
        self.stop.store(false, Ordering::SeqCst);
    }

    pub fn should_stop(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }
}

fn emit(app: &AppHandle, phase: &str, message: &str, extra: serde_json::Value) {
    let mut obj = serde_json::json!({
        "phase": phase,
        "message": message,
    });
    if let Some(map) = obj.as_object_mut() {
        if let Some(extra_map) = extra.as_object() {
            for (k, v) in extra_map {
                map.insert(k.clone(), v.clone());
            }
        }
    }
    let _ = app.emit("t1-ble", obj);
}

/// 已保存过 T1 蓝牙地址则开机自动连，否则闸门不会挂上，原生搜索会漏。
pub fn should_autostart_t1_ble(bluetooth_address: Option<&str>) -> bool {
    bluetooth_address
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
}

/// 启动 T1 蓝牙连接（不触碰 USB runtime）
pub fn start_t1_ble_bridge(app: AppHandle, runtime: Arc<T1BleRuntime>) -> Result<(), String> {
    if runtime.running.load(Ordering::SeqCst) {
        return Err("T1 蓝牙已在连接中".into());
    }
    runtime.clear_stop();
    runtime.running.store(true, Ordering::SeqCst);
    // 发现/重连窗口也要吞原生键，不能等 GATT 连上才挂闸门。
    crate::bridges::t1::native_suppress::arm_reason(
        crate::bridges::t1::native_suppress::SwallowReason::BleBridge,
    );
    if let Some(cfg) = app
        .try_state::<ConfigManager>()
        .and_then(|m| m.get_device_config("t1_ble").ok())
    {
        let vol_plus = match cfg.button_bindings.get("vol_plus") {
            Some(KeyAction::SingleKey(vk)) => vec![*vk],
            Some(KeyAction::ComboKey(v)) => v.clone(),
            _ => Vec::new(),
        };
        let vol_minus = match cfg.button_bindings.get("vol_minus") {
            Some(KeyAction::SingleKey(vk)) => vec![*vk],
            Some(KeyAction::ComboKey(v)) => v.clone(),
            _ => Vec::new(),
        };
        let mute = match cfg.button_bindings.get("mute") {
            Some(KeyAction::SingleKey(vk)) => vec![*vk],
            Some(KeyAction::ComboKey(v)) => v.clone(),
            _ => Vec::new(),
        };
        crate::bridges::t1::native_suppress::refresh_media_gates_from_bindings(
            Some(vol_plus.as_slice()).filter(|v| !v.is_empty()),
            Some(vol_minus.as_slice()).filter(|v| !v.is_empty()),
            Some(mute.as_slice()).filter(|v| !v.is_empty()),
        );
        let up = match cfg.button_bindings.get("up") {
            Some(KeyAction::SingleKey(vk)) => vec![*vk],
            Some(KeyAction::ComboKey(v)) => v.clone(),
            _ => Vec::new(),
        };
        let down = match cfg.button_bindings.get("down") {
            Some(KeyAction::SingleKey(vk)) => vec![*vk],
            Some(KeyAction::ComboKey(v)) => v.clone(),
            _ => Vec::new(),
        };
        let left = match cfg.button_bindings.get("left") {
            Some(KeyAction::SingleKey(vk)) => vec![*vk],
            Some(KeyAction::ComboKey(v)) => v.clone(),
            _ => Vec::new(),
        };
        let right = match cfg.button_bindings.get("right") {
            Some(KeyAction::SingleKey(vk)) => vec![*vk],
            Some(KeyAction::ComboKey(v)) => v.clone(),
            _ => Vec::new(),
        };
        let ok = match cfg.button_bindings.get("ok") {
            Some(KeyAction::SingleKey(vk)) => vec![*vk],
            Some(KeyAction::ComboKey(v)) => v.clone(),
            _ => Vec::new(),
        };
        crate::bridges::t1::native_suppress::refresh_dpad_remap_gates(
            Some(up.as_slice()).filter(|v| !v.is_empty()),
            Some(down.as_slice()).filter(|v| !v.is_empty()),
            Some(left.as_slice()).filter(|v| !v.is_empty()),
            Some(right.as_slice()).filter(|v| !v.is_empty()),
            Some(ok.as_slice()).filter(|v| !v.is_empty()),
        );
        let home = match cfg.button_bindings.get("home") {
            Some(KeyAction::SingleKey(vk)) => vec![*vk],
            Some(KeyAction::ComboKey(v)) => v.clone(),
            _ => Vec::new(),
        };
        crate::bridges::t1::native_suppress::refresh_home_vk24_gate(
            Some(home.as_slice()).filter(|v| !v.is_empty()),
        );
    }

    let configured = app
        .try_state::<ConfigManager>()
        .and_then(|m| m.get_device_config("t1_ble").ok())
        .and_then(|c| c.bluetooth_address.clone());

    let app2 = app.clone();
    let runtime2 = Arc::clone(&runtime);
    thread::Builder::new()
        .name("t1-ble-worker".into())
        .spawn(move || {
            // 连接前预热 PCM 路由，避免首句语音才等 PONG
            let _ = crate::audio::pcm_router::ensure_audio_router_process();
            ble_pcm::warmup_async();
            emit(
                &app2,
                "connecting",
                "正在查找已配对的 T1-Remote…",
                serde_json::json!({ "status": "connecting" }),
            );
            loop {
                if runtime2.should_stop() {
                    break;
                }
                match ble_connect::discover_and_connect(configured.as_deref()) {
                    Ok(conn) => {
                        emit(
                            &app2,
                            "connected",
                            &format!("已连接 {} ({})", conn.name, conn.address),
                            serde_json::json!({
                                "status": "connected",
                                "name": conn.name,
                                "address": conn.address,
                            }),
                        );
                        if let Some(state) = app2.try_state::<crate::bridges::BridgeState>() {
                            state.update_device_info(
                                crate::bridges::BridgeType::T1Ble,
                                Some(conn.name.clone()),
                                Some(conn.address.clone()),
                                None,
                            );
                        }
                        log::info!(
                            "T1 BLE connected name={} address={}",
                            conn.name,
                            conn.address
                        );
                        crate::bridges::t1::ble_keys::remember_ble_address(&conn.address);
                        // HOGP 重连后 Consumer 集合可能重新启用 → 自动再禁 AC Search
                        crate::bridges::t1::t1_hid_filter_env::kick_auto_repair_if_needed(
                            "ble_connect",
                        );
                        if let Some(mgr) = app2.try_state::<ConfigManager>() {
                            if let Ok(mut cfg) = mgr.get_device_config("t1_ble") {
                                cfg.bluetooth_address = Some(conn.address.clone());
                                let _ = mgr.save_device_config("t1_ble", &cfg);
                            }
                        }
                        if let Err(e) = crate::bridges::t1::ble_keys::start(&app2) {
                            log::warn!("T1 BLE keys start: {e}");
                            emit(
                                &app2,
                                "error",
                                &format!("按键监听启动失败: {e}"),
                                serde_json::json!({ "status": "connected" }),
                            );
                        }
                        if let Err(e) = run_session(&app2, &conn, Arc::clone(&runtime2)) {
                            log::warn!("T1 BLE session ended: {e}");
                            emit(
                                &app2,
                                "error",
                                &format!("会话结束: {e}"),
                                serde_json::json!({ "status": "error" }),
                            );
                        }
                        crate::bridges::t1::ble_keys::stop(&app2);
                    }
                    Err(e) => {
                        log::warn!("T1 BLE discover failed: {e}");
                        emit(
                            &app2,
                            "error",
                            &e,
                            serde_json::json!({ "status": "error" }),
                        );
                        if runtime2.should_stop() {
                            break;
                        }
                        thread::sleep(std::time::Duration::from_secs(3));
                        continue;
                    }
                }
                if runtime2.should_stop() {
                    break;
                }
                emit(
                    &app2,
                    "reconnecting",
                    "蓝牙断开，3 秒后重试…",
                    serde_json::json!({ "status": "connecting" }),
                );
                thread::sleep(std::time::Duration::from_secs(3));
            }
            ble_voice::force_release(&app2);
            ble_pcm::shutdown();
            crate::bridges::t1::ble_keys::stop(&app2);
            crate::bridges::t1::ble_host::set_atvv_ok(false);
            crate::bridges::t1::ble_voice_meter::reset();
            runtime2.running.store(false, Ordering::SeqCst);
            crate::bridges::t1::native_suppress::disarm_reason(
                crate::bridges::t1::native_suppress::SwallowReason::BleBridge,
            );
            if let Some(state) = app2.try_state::<crate::bridges::BridgeState>() {
                state.update_status(
                    crate::bridges::BridgeType::T1Ble,
                    crate::bridges::BridgeStatus::Disconnected,
                );
            }
            emit(
                &app2,
                "disconnected",
                "T1 蓝牙已断开",
                serde_json::json!({ "status": "disconnected" }),
            );
            log::info!("T1 BLE worker exited");
        })
        .map_err(|e| {
            runtime.running.store(false, Ordering::SeqCst);
            crate::bridges::t1::native_suppress::disarm_reason(
                crate::bridges::t1::native_suppress::SwallowReason::BleBridge,
            );
            format!("启动 T1 BLE worker 失败: {e}")
        })?;

    Ok(())
}

fn run_session(
    app: &AppHandle,
    conn: &T1BleConnection,
    runtime: Arc<T1BleRuntime>,
) -> Result<(), String> {
    ble_session::run_atvv_session(
        app.clone(),
        conn.address_u64,
        conn.atvv_interface_id.clone(),
        runtime,
    )
}

pub fn stop_t1_ble_bridge(app: &AppHandle, runtime: &T1BleRuntime) {
    runtime.request_stop();
    ble_voice::force_release(app);
    ble_pcm::end_session();
    crate::bridges::t1::ble_keys::stop(app);
    // worker 线程会自行清 running / shutdown pcm
    emit(
        app,
        "disconnecting",
        "正在断开 T1 蓝牙…",
        serde_json::json!({ "status": "disconnecting" }),
    );
}

pub fn is_running(runtime: &T1BleRuntime) -> bool {
    runtime.running.load(Ordering::SeqCst)
}
