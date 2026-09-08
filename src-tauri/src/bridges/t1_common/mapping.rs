//! T1 事件 → 按钮 ID（对齐 Python button_aliases HID 表）

use std::collections::HashMap;

/// Python `config.json` 默认 HID / 部分键盘别名
pub fn default_event_aliases() -> HashMap<String, Vec<String>> {
    let mut m = HashMap::new();
    m.insert(
        "voice".into(),
        vec![
            "hid:02-CF-00".into(),
            // BLE HOGP 语音键：Consumer AC Search（会开 Windows 搜索）
            "hid:02-21-02".into(),
            // Windows 常把 AC Search 转成 VK_BROWSER_SEARCH
            "kbd:VK_AA".into(),
        ],
    );
    m.insert(
        "delete".into(),
        vec![
            "hid:02-24-02".into(),
            "kbd:VK_A6".into(),
        ],
    );
    m.insert(
        "home".into(),
        vec![
            "hid:02-23-02".into(),
            "kbd:VK_AC".into(),
            // BLE HOGP 有时发普通 Home，而非 Browser Home
            "kbd:VK_24".into(),
        ],
    );
    m.insert(
        "mute".into(),
        vec![
            "hid:02-E2-00".into(),
            "kbd:VK_AD".into(),
        ],
    );
    m.insert(
        "vol_plus".into(),
        vec![
            "hid:02-E9-00".into(),
            "kbd:VK_AF".into(),
        ],
    );
    m.insert(
        "vol_minus".into(),
        vec![
            "hid:02-EA-00".into(),
            "kbd:VK_AE".into(),
        ],
    );
    m.insert("ok".into(), vec!["kbd:VK_0D".into()]);
    m.insert(
        "up".into(),
        vec!["kbd:VK_26".into()],
    );
    m.insert(
        "down".into(),
        vec!["kbd:VK_28".into()],
    );
    m.insert(
        "left".into(),
        vec!["kbd:VK_25".into()],
    );
    m.insert(
        "right".into(),
        vec!["kbd:VK_27".into()],
    );
    m.insert("menu".into(), vec!["kbd:VK_5D".into()]);
    m
}

/// 构建 event_id → button_id 反查表
pub fn build_event_to_button(aliases: &HashMap<String, Vec<String>>) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for (button, events) in aliases {
        for ev in events {
            // 键盘别名只比前缀 `kbd:VK_XX`（忽略 SC/E0 后缀）
            let key = normalize_event_id(ev);
            out.insert(key, button.clone());
        }
    }
    out
}

pub fn normalize_event_id(event_id: &str) -> String {
    let e = event_id.trim();
    if let Some(rest) = e.strip_prefix("kbd:VK_") {
        let vk = rest
            .split(|c| c == ':' || c == '-')
            .next()
            .unwrap_or(rest);
        format!("kbd:VK_{}", vk.to_ascii_uppercase())
    } else if let Some(hex) = e.strip_prefix("hid:") {
        format!("hid:{}", hex.to_ascii_uppercase())
    } else {
        e.to_string()
    }
}

pub fn resolve_button(
    event_id: &str,
    event_to_button: &HashMap<String, String>,
) -> Option<String> {
    let key = normalize_event_id(event_id);
    if let Some(b) = event_to_button.get(&key) {
        return Some(b.clone());
    }
    // HID 报告可能带额外尾字节（如 02-CF-00-00）；按前缀匹配已知别名
    if key.starts_with("hid:") {
        let mut best: Option<(usize, String)> = None;
        for (alias, button) in event_to_button {
            if !alias.starts_with("hid:") {
                continue;
            }
            if key == *alias || key.starts_with(&format!("{alias}-")) || alias.starts_with(&format!("{key}-"))
            {
                let score = alias.len();
                if best.as_ref().map(|(s, _)| score > *s).unwrap_or(true) {
                    best = Some((score, button.clone()));
                }
            }
        }
        return best.map(|(_, b)| b);
    }
    None
}

/// 该原生事件在 T1 桥接开启时必须吞掉的侧效应 VK。
/// 方向/OK 没有侧效应 VK（实体键盘常用键，只在遥控按住时 hold-suppress）。
pub fn native_side_effect_vk_for_event(event_id: &str) -> Option<u16> {
    let map = build_event_to_button(&default_event_aliases());
    let button = resolve_button(event_id, &map)?;
    crate::bridges::t1::native_suppress::side_effect_vks_for_button(&button)
        .first()
        .copied()
}


/// 语音 toggle 状态机（纯逻辑，便于单测）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VoiceSessionState {
    #[default]
    Idle,
    Active,
}

impl VoiceSessionState {
    /// 返回 (新状态, 是否应注入快捷键)
    pub fn on_voice_press(self) -> (Self, bool) {
        match self {
            Self::Idle => (Self::Active, true),
            Self::Active => (Self::Idle, true),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hid_voice_maps_to_voice_button() {
        let aliases = default_event_aliases();
        let map = build_event_to_button(&aliases);
        assert_eq!(
            resolve_button("hid:02-CF-00", &map).as_deref(),
            Some("voice")
        );
        assert_eq!(
            resolve_button("hid:02-cf-00", &map).as_deref(),
            Some("voice")
        );
        assert_eq!(
            resolve_button("hid:02-21-02", &map).as_deref(),
            Some("voice")
        );
        assert_eq!(
            resolve_button("kbd:VK_AA", &map).as_deref(),
            Some("voice")
        );
        assert_eq!(
            resolve_button("hid:02-E9-00", &map).as_deref(),
            Some("vol_plus")
        );
    }

    #[test]
    fn hid_prefix_matches_longer_report() {
        let aliases = default_event_aliases();
        let map = build_event_to_button(&aliases);
        assert_eq!(
            resolve_button("hid:02-CF-00-00", &map).as_deref(),
            Some("voice")
        );
    }

    #[test]
    fn voice_toggle_machine() {
        let (s1, inject1) = VoiceSessionState::Idle.on_voice_press();
        assert_eq!(s1, VoiceSessionState::Active);
        assert!(inject1);
        let (s2, inject2) = s1.on_voice_press();
        assert_eq!(s2, VoiceSessionState::Idle);
        assert!(inject2);
    }
}
