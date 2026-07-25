//! 小米按键采集 — 对齐 Python 生产路径
//!
//! - HidOverGatt Frida Gadget tap → 返回键 0xF1、音量 0x80/0x81（Windows HID 独占时必需）
//! - ATVV Control → 语音键
//! - 低级键盘钩 → 抑制已由 Tap 映射的原生气，避免双触发
//!
//! 故意不做：hidapi 打开设备、默认 GATT HID 订阅（会抢占 Microsoft HID，导致
//! Windows 原生音量失效且 Tap 未就绪时三键全死）。

use crate::bridges::xiaomi::connect::XiaomiRuntime;
use crate::bridges::xiaomi::hid_report_tap::{ensure_started, stop_and_join};
use crate::bridges::xiaomi::input_session::run_input_session;
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct XiaomiKeyEvent {
    pub button_id: String,
    pub label: String,
    /// "down" | "up"
    #[serde(default = "default_key_phase")]
    pub phase: String,
}

#[allow(dead_code)] // referenced by serde default = "default_key_phase"
fn default_key_phase() -> String {
    "down".into()
}

#[derive(Clone, Serialize)]
pub struct XiaomiKeyMessage {
    pub message: String,
}

/// 按键去抖门闩：同一 button_id 在窗口内只发一次 UI 事件
pub struct KeyEmitGate {
    last: Mutex<HashMap<String, Instant>>,
    window: Duration,
}

impl KeyEmitGate {
    pub fn new(window_ms: u64) -> Self {
        Self {
            last: Mutex::new(HashMap::new()),
            window: Duration::from_millis(window_ms),
        }
    }

    pub fn try_emit(&self, button_id: &str) -> bool {
        let now = Instant::now();
        let mut guard = self.last.lock();
        if let Some(prev) = guard.get(button_id) {
            if now.duration_since(*prev) < self.window {
                return false;
            }
        }
        guard.insert(button_id.to_string(), now);
        true
    }
}

pub fn emit_key(app: &AppHandle, button_id: &str, label: &str) {
    emit_key_phase(app, button_id, label, true);
}

pub fn emit_key_phase(app: &AppHandle, button_id: &str, label: &str, pressed: bool) {
    let _ = app.emit(
        "xiaomi-key",
        XiaomiKeyEvent {
            button_id: button_id.to_string(),
            label: label.to_string(),
            phase: if pressed { "down".into() } else { "up".into() },
        },
    );
}

/// 对齐 Python：检测后立刻执行 button_bindings 映射
pub fn emit_key_and_map(app: &AppHandle, button_id: &str, label: &str, pressed: bool) {
    emit_key_phase(app, button_id, label, pressed);
    crate::bridges::xiaomi::key_mapping::on_remote_button(app, button_id, pressed);
}

pub fn emit_message(app: &AppHandle, message: &str) {
    let _ = app.emit(
        "xiaomi-key",
        XiaomiKeyMessage {
            message: message.to_string(),
        },
    );
}

pub fn button_label(id: &str) -> &'static str {
    match id {
        "power" => "电源",
        "volume_up" => "音量+",
        "volume_down" => "音量-",
        "up" | "dpad_up" => "上",
        "down" | "dpad_down" => "下",
        "left" | "dpad_left" => "左",
        "right" | "dpad_right" => "右",
        "ok" => "确定",
        "back" => "返回",
        "home" => "主页",
        "menu" => "菜单",
        "voice" | "mic" => "语音",
        "mute" | "volume_mute" => "静音",
        "tv" => "TV",
        _ => "未知",
    }
}

/// 连接成功后启动按键通道（对齐 Python atvv_live_bridge 启动顺序）
pub fn start_key_logger(
    app: AppHandle,
    runtime: Arc<XiaomiRuntime>,
    address_u64: u64,
    atvv_interface_id: String,
) {
    #[cfg(target_os = "windows")]
    {
        use crate::config::manager::ConfigManager;
        use tauri::Manager;

        let gate = Arc::new(KeyEmitGate::new(90));
        let (tap_enabled, hook_enabled) = app
            .try_state::<ConfigManager>()
            .and_then(|m| m.get_device_config("xiaomi").ok())
            .map(|c| (c.hid_report_tap_enabled, c.special_key_hook_enabled))
            .unwrap_or((true, true));

        // 0) 特殊键 + HID Tap
        crate::bridges::xiaomi::special_keys::set_hook_enabled(hook_enabled);
        if hook_enabled {
            crate::bridges::xiaomi::special_keys::start_special_key_hook();
        }

        let mut tap_started = false;
        if tap_enabled {
            let app2 = app.clone();
            let gate2 = Arc::clone(&gate);
            tap_started = ensure_started(app2, gate2);
            if !tap_started {
                emit_message(
                    &app,
                    "HID Tap 未启动：返回/音量键不可用（请确认 Frida Gadget 资源）",
                );
            }
        } else {
            // 配置关闭：释放进程级 hub（若曾启动）
            stop_and_join();
            emit_message(&app, "HID Tap 已按配置禁用");
        }

        // Tap 未附着时启用 Raw Input 兜底（对齐 Python should_start_raw_mapping）
        crate::bridges::xiaomi::raw_mapping::maybe_start_raw_mapping(
            app.clone(),
            Arc::clone(&runtime),
            Arc::clone(&gate),
            tap_started,
        );

        {
            let app2 = app.clone();
            let runtime2 = Arc::clone(&runtime);
            let gate2 = Arc::clone(&gate);
            let iface = atvv_interface_id.clone();
            std::thread::Builder::new()
                .name("xiaomi-gatt-input".into())
                .spawn(move || {
                    match run_input_session(app2.clone(), address_u64, iface, runtime2, gate2) {
                        Ok(()) => {}
                        Err(e) => {
                            log::warn!("ATVV input session unavailable: {e}");
                            emit_message(&app2, &format!("ATVV 语音通道不可用: {e}"));
                        }
                    }
                })
                .ok();
        }

        {
            let app2 = app.clone();
            let runtime2 = Arc::clone(&runtime);
            let gate2 = Arc::clone(&gate);
            std::thread::Builder::new()
                .name("xiaomi-vk-poll".into())
                .spawn(move || {
                    windows_vk_poll_logger(app2, runtime2, gate2);
                })
                .ok();
        }

        emit_message(
            &app,
            "按键监听已启动（HID-Tap 返回/音量 + ATVV 语音/音频；对齐 Python）",
        );
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app, runtime, address_u64, atvv_interface_id);
    }
}

/// VK 轮询：仅作 UI/诊断兜底，不执行映射（避免与系统原生气 + HID 映射双触发）
#[cfg(target_os = "windows")]
fn windows_vk_poll_logger(app: AppHandle, runtime: Arc<XiaomiRuntime>, gate: Arc<KeyEmitGate>) {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

    let keys: &[(i32, &str)] = &[
        (0xAF, "volume_up"),
        (0xAE, "volume_down"),
        (0xAD, "volume_mute"),
        (0x26, "up"),
        (0x28, "down"),
        (0x25, "left"),
        (0x27, "right"),
        (0x0D, "ok"),
        (0x24, "home"),
    ];

    let mut prev: HashMap<i32, bool> = HashMap::new();
    while !runtime.should_stop() {
        for &(vk, id) in keys {
            let down = unsafe { GetAsyncKeyState(vk) as u16 } & 0x8000 != 0;
            let was = prev.get(&vk).copied().unwrap_or(false);
            if down && !was && gate.try_emit(id) {
                emit_key(&app, id, button_label(id));
                log::info!("XIAOMI VK observe key={id} vk=0x{vk:02X} (no map)");
            }
            prev.insert(vk, down);
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}
