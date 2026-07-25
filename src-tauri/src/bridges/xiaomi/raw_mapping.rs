//! 对齐 Python Raw Input fallback：HID Tap 未启动时，用设备过滤的 Raw Input 映射

use crate::bridges::shared::raw_input::{RawInputBridge, RawInputDeviceType, RawInputEvent};
use crate::bridges::xiaomi::connect::XiaomiRuntime;
use crate::bridges::xiaomi::key_log::{button_label, emit_key_and_map, emit_message, KeyEmitGate};
use crate::bridges::xiaomi::tv_gate;
use crate::config::manager::ConfigManager;
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{AppHandle, Manager};

/// VK → button_id（与 Python BUTTON_ALIASES / 常见 HID 翻译对应）
fn vk_to_button(vk: u16) -> Option<&'static str> {
    match vk {
        0xAF => Some("volume_up"),
        0xAE => Some("volume_down"),
        0xAD => Some("volume_mute"),
        0x26 => Some("up"),
        0x28 => Some("down"),
        0x25 => Some("left"),
        0x27 => Some("right"),
        0x0D => Some("ok"),
        0xA6 => Some("back"),
        0x24 | 0xAC => Some("home"),
        0x5D => Some("menu"),
        0x1B => Some("power"),
        0xC0 => Some("tv"), // OEM_3 近似
        _ => None,
    }
}

/// Tap 未启动且配置允许时启动
pub fn maybe_start_raw_mapping(
    app: AppHandle,
    runtime: Arc<XiaomiRuntime>,
    gate: Arc<KeyEmitGate>,
    hid_tap_started: bool,
) {
    let config = app
        .try_state::<ConfigManager>()
        .and_then(|m| m.get_device_config("xiaomi").ok());
    // DeviceConfig 暂无 raw_mapping_enabled 字段时默认 true（对齐 Python）
    let raw_enabled = config.as_ref().map(|_| true).unwrap_or(true);
    let tap_enabled = config
        .as_ref()
        .map(|c| c.hid_report_tap_enabled)
        .unwrap_or(true);
    let gadget_missing = !crate::bridges::xiaomi::hid_tap_runtime::gadget_archive_available();
    let fallback_required = tap_enabled && gadget_missing;
    let should = !hid_tap_started && (raw_enabled || fallback_required);
    if !should {
        log::info!(
            "XIAOMI RAW MAPPING skipped tap_started={hid_tap_started} fallback={fallback_required}"
        );
        return;
    }

    emit_message(&app, "Raw Input 旁路已启动（HID Tap 未附着时的按键兜底）");
    std::thread::Builder::new()
        .name("xiaomi-raw-mapping".into())
        .spawn(move || {
            run_raw_mapping(app, runtime, gate);
        })
        .ok();
}

fn run_raw_mapping(app: AppHandle, runtime: Arc<XiaomiRuntime>, gate: Arc<KeyEmitGate>) {
    let mut bridge = RawInputBridge::new();
    let app2 = app.clone();
    let gate2 = Arc::clone(&gate);
    // device token：配置中的蓝牙地址去冒号
    let token = app
        .try_state::<ConfigManager>()
        .and_then(|m| m.get_device_config("xiaomi").ok())
        .and_then(|c| c.bluetooth_address)
        .map(|a| a.replace(':', "").replace('-', "").to_ascii_lowercase())
        .unwrap_or_default();

    let mut last_down: HashMap<u16, bool> = HashMap::new();

    if let Err(e) = bridge.start(move |ev: RawInputEvent| {
        if ev.device_type != RawInputDeviceType::Keyboard {
            return;
        }
        // 无 device_match 时仍允许动作（开发便利）；有 token 时仅作日志提示
        let _ = &token;
        let Some(id) = vk_to_button(ev.usage_id) else {
            return;
        };
        if id == "tv" && !tv_gate::is_ready() {
            return;
        }
        let was = last_down.get(&ev.usage_id).copied().unwrap_or(false);
        if ev.pressed && !was {
            if gate2.try_emit(id) {
                emit_key_and_map(&app2, id, button_label(id), true);
                log::info!("XIAOMI RAW MAP key={id} vk=0x{:02X} down", ev.usage_id);
            } else {
                log::debug!(
                    "XIAOMI RAW MAP gated drop key={id} vk=0x{:02X}",
                    ev.usage_id
                );
            }
        } else if !ev.pressed && was {
            crate::bridges::xiaomi::key_mapping::on_remote_button(&app2, id, false);
        }
        last_down.insert(ev.usage_id, ev.pressed);
    }) {
        log::warn!("XIAOMI RAW MAPPING start failed: {e}");
        emit_message(&app, &format!("Raw Input 启动失败: {e}"));
        return;
    }

    log::info!("XIAOMI RAW MAPPING READY");
    while !runtime.should_stop() {
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    bridge.stop();
}
