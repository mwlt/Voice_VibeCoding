//! 语音和弦注入路由决策（纯函数，便于测试）。
//!
//! 豆包/千问等会过滤 SendInput；语音唤醒**优先** WinUHid。
//! WinUHid 不可用时互斥降级 SendInput（1.3.15）；同一次按键禁止双发。

/// 未使用的虚拟键（AutoHotkey / prevent-alt-win-menu 惯例 `vkE8`）。
pub const ALT_MENU_SUPPRESS_DUMMY_VK: u16 = 0xE8;

/// `KBDLLHOOKSTRUCT.flags`：`LLKHF_INJECTED` + `LLKHF_LOWER_IL_INJECTED`
pub const LLKHF_INJECTED: u32 = 0x10;
pub const LLKHF_LOWER_IL_INJECTED: u32 = 0x02;
pub const LLKHF_INJECTED_MASK: u32 = LLKHF_INJECTED | LLKHF_LOWER_IL_INJECTED;

/// 语音唤醒实际使用的注入后端（互斥）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceInjectBackend {
    /// WinUHid 可用：虚拟硬件键
    VirtualHid,
    /// WinUHid 不可用：SendInput 降级（微信或可用；豆包/千问常失败）
    SendInputFallback,
}

/// 兼容旧名：无可用性参数时仍表示「偏好虚拟 HID」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceChordInjectRoute {
    RequireVirtualHid,
    SendInputFallback,
}

/// 按 WinUHid 是否可用选择唯一后端（同一次 DOWN/UP 必须同一后端）。
pub fn voice_inject_backend(_vks: &[u16], winuhid_available: bool) -> VoiceInjectBackend {
    if winuhid_available {
        VoiceInjectBackend::VirtualHid
    } else {
        VoiceInjectBackend::SendInputFallback
    }
}

/// 兼容旧 API：无可用性参数时仍表示「偏好虚拟 HID」。
pub fn voice_chord_inject_route(vks: &[u16]) -> VoiceChordInjectRoute {
    let _ = vks;
    VoiceChordInjectRoute::RequireVirtualHid
}

/// 是否允许语音路径走虚拟 HID（由运行时 `hid_injector::is_available()` 再把关）。
pub fn voice_chord_allows_virtual_hid(vks: &[u16]) -> bool {
    let _ = vks;
    true
}

pub fn is_alt_modifier(vk: u16) -> bool {
    matches!(vk, 0x12 | 0xA4 | 0xA5) // VK_MENU, VK_LMENU, VK_RMENU
}

pub fn is_win_modifier(vk: u16) -> bool {
    matches!(vk, 0x5B | 0x5C) // VK_LWIN, VK_RWIN
}

pub fn has_alt_modifier(vks: &[u16]) -> bool {
    vks.iter().copied().any(is_alt_modifier)
}

pub fn has_win_modifier(vks: &[u16]) -> bool {
    vks.iter().copied().any(is_win_modifier)
}

/// 是否应在 Alt 和弦 **KEYUP 完成之后** 发送 dummy，以取消窗口菜单栏。
pub fn should_suppress_alt_menu_after_keyup(vks: &[u16], key_up: bool) -> bool {
    key_up && has_alt_modifier(vks)
}

/// Alt 菜单栏 **或** Win 开始菜单：KEYUP 之后插 `vkE8` dummy。
/// 千问 Win+Alt：松开时若 Win 被系统当成「点按」会弹出开始菜单，识别结果进搜索框。
pub fn should_suppress_shell_menu_after_keyup(vks: &[u16], key_up: bool) -> bool {
    key_up && (has_alt_modifier(vks) || has_win_modifier(vks))
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

/// 语音注入前规范化 VK（generic Ctrl/Alt → 左 Ctrl/左 Alt 等）。
pub fn normalize_voice_chord_vks(vks: &[u16]) -> Vec<u16> {
    crate::bridges::shared::shortcut_capture::normalize_chord_vks(vks)
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
    fn backend_virtual_when_available() {
        assert_eq!(
            voice_inject_backend(&[0xA5], true),
            VoiceInjectBackend::VirtualHid
        );
        assert_eq!(
            voice_inject_backend(&[0xA2, 0x5B], true),
            VoiceInjectBackend::VirtualHid
        );
    }

    #[test]
    fn backend_sendinput_when_unavailable() {
        assert_eq!(
            voice_inject_backend(&[0xA5], false),
            VoiceInjectBackend::SendInputFallback
        );
        assert_eq!(
            voice_inject_backend(&[0xA2, 0x5B], false),
            VoiceInjectBackend::SendInputFallback
        );
    }

    #[test]
    fn legacy_route_still_prefers_virtual_hid() {
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
            sanitize_own_inject_flags(0x584D_4952, 0x584D_4952, injected) & LLKHF_INJECTED_MASK,
            0
        );
        assert_eq!(
            sanitize_own_inject_flags(1, 0x584D_4952, injected) & LLKHF_INJECTED,
            LLKHF_INJECTED
        );
    }

    #[test]
    fn win_alt_needs_shell_menu_dummy_after_keyup() {
        assert!(should_suppress_shell_menu_after_keyup(&[0x5B, 0xA4], true));
        assert!(!should_suppress_shell_menu_after_keyup(&[0x5B, 0xA4], false));
        assert!(should_suppress_shell_menu_after_keyup(&[0xA2, 0x5B], true));
        assert!(should_suppress_shell_menu_after_keyup(&[0xA5], true));
    }

    #[test]
    fn normalize_voice_chord_maps_generic_modifiers_to_left() {
        assert_eq!(normalize_voice_chord_vks(&[0x12, 0x5B]), vec![0xA4, 0x5B]);
        assert_eq!(normalize_voice_chord_vks(&[0x11, 0x5B]), vec![0xA2, 0x5B]);
    }
}
