//! 语音和弦注入路由决策（纯函数，便于测试）。
//!
//! 豆包/千问等会过滤 SendInput；语音唤醒路径要求 WinUHid 虚拟硬件键。

/// 未使用的虚拟键（AutoHotkey / prevent-alt-win-menu 惯例 `vkE8`）。
pub const ALT_MENU_SUPPRESS_DUMMY_VK: u16 = 0xE8;

/// `KBDLLHOOKSTRUCT.flags`：`LLKHF_INJECTED` + `LLKHF_LOWER_IL_INJECTED`
pub const LLKHF_INJECTED: u32 = 0x10;
pub const LLKHF_LOWER_IL_INJECTED: u32 = 0x02;
pub const LLKHF_INJECTED_MASK: u32 = LLKHF_INJECTED | LLKHF_LOWER_IL_INJECTED;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceChordInjectRoute {
    /// 必须有 WinUHid.dll+驱动；不可用时阻断并提示修复（不再静默 SendInput）
    RequireVirtualHid,
}

pub fn is_alt_modifier(vk: u16) -> bool {
    matches!(vk, 0x12 | 0xA4 | 0xA5) // VK_MENU, VK_LMENU, VK_RMENU
}

pub fn has_alt_modifier(vks: &[u16]) -> bool {
    vks.iter().copied().any(is_alt_modifier)
}

/// 语音快捷键注入路由：始终要求虚拟 HID。
pub fn voice_chord_inject_route(vks: &[u16]) -> VoiceChordInjectRoute {
    let _ = vks;
    VoiceChordInjectRoute::RequireVirtualHid
}

/// 是否允许语音路径走虚拟 HID（由运行时 `hid_injector::is_available()` 再把关）。
pub fn voice_chord_allows_virtual_hid(vks: &[u16]) -> bool {
    let _ = vks;
    true
}

/// 语音注入前规范化 VK（generic Ctrl/Alt → 左 Ctrl/左 Alt 等）。
pub fn normalize_voice_chord_vks(vks: &[u16]) -> Vec<u16> {
    crate::bridges::shared::shortcut_capture::normalize_chord_vks(vks)
}

/// 是否应在 Alt 和弦 **KEYUP 完成之后** 发送 dummy，以取消窗口菜单栏。
pub fn should_suppress_alt_menu_after_keyup(vks: &[u16], key_up: bool) -> bool {
    key_up && has_alt_modifier(vks)
}

/// 右/左 Alt 的基扫描码均为 0x38；右 Alt 再加 KEYEVENTF_EXTENDEDKEY。
/// `MapVirtualKey(VK_RMENU)` 在部分环境返回 0，会导致输入法认不出右 Alt。
pub fn scan_code_for_vk(vk: u16, mapped: u16) -> u16 {
    match vk {
        0xA4 | 0xA5 | 0x12 => {
            if mapped != 0 {
                mapped
            } else {
                0x38
            }
        }
        0xA2 | 0xA3 | 0x11 => {
            if mapped != 0 {
                mapped
            } else {
                0x1D
            }
        }
        0x5B | 0x5C => {
            if mapped != 0 {
                mapped
            } else {
                0x5B
            }
        }
        _ if mapped != 0 => mapped,
        _ => mapped,
    }
}

/// 纯函数：仅当 `extra_info` 为本应用标记时清除 INJECTED 位（供测试/可选诊断钩子）。
pub fn sanitize_own_inject_flags(extra_info: usize, own_extra: usize, flags: u32) -> u32 {
    if extra_info == own_extra {
        flags & !LLKHF_INJECTED_MASK
    } else {
        flags
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_route_requires_virtual_hid() {
        assert_eq!(
            voice_chord_inject_route(&[0xA5]),
            VoiceChordInjectRoute::RequireVirtualHid
        );
        assert_eq!(
            voice_chord_inject_route(&[0xA2, 0x5B]),
            VoiceChordInjectRoute::RequireVirtualHid
        );
    }

    #[test]
    fn strips_injected_only_for_own_extra() {
        let injected = LLKHF_INJECTED | 0x01;
        assert_eq!(
            sanitize_own_inject_flags(0x584D_4952, 0x584D_4952, injected),
            0x01
        );
    }

    #[test]
    fn normalize_voice_chord_maps_generic_modifiers_to_left() {
        assert_eq!(normalize_voice_chord_vks(&[0x12, 0x5B]), vec![0xA4, 0x5B]);
        assert_eq!(normalize_voice_chord_vks(&[0x11, 0x5B]), vec![0xA2, 0x5B]);
        assert_eq!(normalize_voice_chord_vks(&[0xA5]), vec![0xA5]);
    }
}
