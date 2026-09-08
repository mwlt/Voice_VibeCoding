//! T1 按键诊断日志：把「真实触发」写全，方便对照映射问题。
//!
//! 统一 USB / BLE：event_id、按下/抬起、映射按钮、目标 VK、HID report、设备短名、风险提示。

use crate::bridges::t1::consumer_raw_input::ConsumerRawEvent;
use tauri::{AppHandle, Emitter};

/// 设备路径缩略，避免整段 GUID 刷屏
pub fn short_device(device_name: &str) -> String {
    let s = device_name.trim();
    if s.is_empty() {
        return "?".into();
    }
    let low = s.to_ascii_lowercase();
    if let Some(i) = low.find("vid_") {
        let tail = &s[i..];
        return if tail.len() > 48 {
            format!("{}…", &tail[..48])
        } else {
            tail.to_string()
        };
    }
    if let Some(i) = low.find("dev_vid") {
        let tail = &s[i..];
        return if tail.len() > 56 {
            format!("{}…", &tail[..56])
        } else {
            tail.to_string()
        };
    }
    if s.len() > 56 {
        format!("{}…", &s[..56])
    } else {
        s.to_string()
    }
}

pub fn format_vks_label(vks: &[u16]) -> String {
    if vks.is_empty() {
        return "（无）".into();
    }
    vks.iter()
        .map(|vk| format!("0x{vk:02X}"))
        .collect::<Vec<_>>()
        .join("+")
}

/// 原生事件风险 / 语义提示（开始菜单、搜索、常见 Consumer）
pub fn classify_event_note(event_id: &str) -> Option<&'static str> {
    let key = event_id.trim().to_ascii_uppercase();
    if key.contains("02-21-02") {
        return Some("AC Search 0x0221→Windows搜索");
    }
    if key.contains("02-CF-00") || key.contains("00-CF") {
        return Some("Voice Command 0xCF");
    }
    if key.contains("02-E9-00") {
        return Some("Volume+");
    }
    if key.contains("02-EA-00") {
        return Some("Volume-");
    }
    if key.contains("02-E2-00") {
        return Some("Mute");
    }
    if key.contains("02-30-00") || key.contains("HID:02-30") {
        return Some("Consumer Power（电源）");
    }
    if key.contains("HID:01-82") || key == "HID:82" || key.starts_with("HID:82-") {
        return Some("System Sleep（可能电源）");
    }
    if key.contains("HID:01-81") || key == "HID:81" || key.starts_with("HID:81-") {
        return Some("System Power Down");
    }
    if key.starts_with("MOUSE:") {
        return Some("鼠标键/空鼠探测");
    }
    if key.contains("02-23-02") {
        return Some("AC Home");
    }
    if key.contains("02-24-02") {
        return Some("AC Back");
    }
    if let Some(rest) = key.strip_prefix("KBD:VK_") {
        let hex = rest.split(|c| c == ':' || c == '-').next().unwrap_or("");
        let vk = u16::from_str_radix(hex, 16).ok()?;
        return match vk {
            0x0D => Some("Enter/OK"),
            0x25 => Some("Left"),
            0x26 => Some("Up"),
            0x27 => Some("Right"),
            0x28 => Some("Down"),
            0x5B => Some("左Win→开始菜单"),
            0x5C => Some("右Win→开始菜单"),
            0x5D => Some("Apps菜单"),
            0x86 => Some("F23/Copilot"),
            0xA6 => Some("Browser Back"),
            0xAA => Some("Browser Search"),
            0xAC => Some("Browser Home"),
            0xAD => Some("Volume Mute VK"),
            0xAE => Some("Volume- VK"),
            0xAF => Some("Volume+ VK"),
            0x5E => Some("OEM/电源相关"),
            0x5F => Some("Sleep（可能电源）"),
            0xFF => Some("OEM FF（可能电源）"),
            _ => None,
        };
    }
    None
}

/// 组装一行人类可读诊断（不含渠道前缀）
pub fn format_line(
    event_id: &str,
    pressed: bool,
    button: Option<&str>,
    map_vks: &[u16],
    report_hex: Option<&str>,
    device_name: &str,
    extra: Option<&str>,
) -> String {
    let dir = if pressed { "↓" } else { "↑" };
    let btn = match button {
        Some(b) => format!("→{b}"),
        None => "→未映射".into(),
    };
    let mut parts = vec![format!("{event_id} {dir} {btn}")];
    if let Some(note) = classify_event_note(event_id) {
        parts.push(note.to_string());
    }
    if !map_vks.is_empty() {
        parts.push(format!("注入{}", format_vks_label(map_vks)));
    } else if button.is_some() && pressed {
        parts.push("注入（无）".into());
    }
    if let Some(hex) = report_hex.filter(|h| !h.is_empty()) {
        parts.push(format!("report={hex}"));
    }
    parts.push(format!("dev={}", short_device(device_name)));
    if let Some(ex) = extra.filter(|s| !s.is_empty()) {
        parts.push(ex.to_string());
    }
    parts.join(" | ")
}

pub fn format_from_event(
    ev: &ConsumerRawEvent,
    button: Option<&str>,
    map_vks: &[u16],
    extra: Option<&str>,
) -> String {
    format_line(
        &ev.event_id,
        ev.pressed,
        button,
        map_vks,
        ev.hid_report_hex.as_deref(),
        &ev.device_name,
        extra,
    )
}

/// USB 桥：`t1-key`，phase=native|key|inject|ll
pub fn emit_usb(
    app: &AppHandle,
    phase: &str,
    button: Option<&str>,
    ev: Option<&ConsumerRawEvent>,
    event_id: Option<&str>,
    pressed: Option<bool>,
    map_vks: &[u16],
    message: &str,
) {
    log::info!("T1 USB [{phase}] {message}");
    let mut json = serde_json::json!({
        "phase": phase,
        "message": message,
    });
    if let Some(b) = button {
        json["id"] = serde_json::json!(b);
    }
    if let Some(ev) = ev {
        json["event"] = serde_json::json!(ev.event_id);
        json["pressed"] = serde_json::json!(ev.pressed);
        json["device"] = serde_json::json!(short_device(&ev.device_name));
        if let Some(hex) = &ev.hid_report_hex {
            json["report"] = serde_json::json!(hex);
        }
    } else {
        if let Some(eid) = event_id {
            json["event"] = serde_json::json!(eid);
        }
        if let Some(p) = pressed {
            json["pressed"] = serde_json::json!(p);
        }
    }
    if !map_vks.is_empty() {
        json["vks"] = serde_json::json!(map_vks);
    }
    let _ = app.emit("t1-key", json);
}

/// BLE 桥：`t1-ble`
pub fn emit_ble(
    app: &AppHandle,
    phase: &str,
    button: Option<&str>,
    ev: Option<&ConsumerRawEvent>,
    event_id: Option<&str>,
    pressed: Option<bool>,
    map_vks: &[u16],
    message: &str,
) {
    log::info!("T1 BLE [{phase}] {message}");
    let mut json = serde_json::json!({
        "phase": phase,
        "message": message,
    });
    if let Some(b) = button {
        json["id"] = serde_json::json!(b);
    }
    if let Some(ev) = ev {
        json["event"] = serde_json::json!(ev.event_id);
        json["pressed"] = serde_json::json!(ev.pressed);
        json["device"] = serde_json::json!(short_device(&ev.device_name));
        if let Some(hex) = &ev.hid_report_hex {
            json["report"] = serde_json::json!(hex);
        }
    } else {
        if let Some(eid) = event_id {
            json["event"] = serde_json::json!(eid);
        }
        if let Some(p) = pressed {
            json["pressed"] = serde_json::json!(p);
        }
    }
    if !map_vks.is_empty() {
        json["vks"] = serde_json::json!(map_vks);
    }
    let _ = app.emit("t1-ble", json);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_includes_unmapped_and_note() {
        let s = format_line("hid:02-21-02", true, None, &[], Some("02-21-02"), "VID_x", None);
        assert!(s.contains("未映射"));
        assert!(s.contains("AC Search"));
        assert!(s.contains("report=02-21-02"));
    }

    #[test]
    fn line_includes_inject_target() {
        let s = format_line(
            "kbd:VK_27",
            true,
            Some("right"),
            &[0xA5, 0x20],
            None,
            "VID_1915&PID_1025&MI_01",
            Some("suppress"),
        );
        assert!(s.contains("→right"));
        assert!(s.contains("注入0xA5+0x20"));
        assert!(s.contains("suppress"));
    }
}
