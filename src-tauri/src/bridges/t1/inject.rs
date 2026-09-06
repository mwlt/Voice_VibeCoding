//! 最小 SendInput 封装（T1 用，避免依赖 xiaomi 模块）

use std::thread;
use std::time::Duration;

pub fn name_to_vk(name: &str) -> Option<u16> {
    let n = name.trim().to_ascii_lowercase().replace(' ', "");
    match n.as_str() {
        "backspace" => Some(0x08),
        "tab" => Some(0x09),
        "enter" | "return" => Some(0x0D),
        "shift" => Some(0x10),
        "ctrl" | "control" => Some(0x11),
        "alt" => Some(0x12),
        "esc" | "escape" => Some(0x1B),
        "space" => Some(0x20),
        "left" => Some(0x25),
        "up" => Some(0x26),
        "right" => Some(0x27),
        "down" => Some(0x28),
        "home" => Some(0x24),
        "win" | "leftwin" | "lwin" => Some(0x5B),
        "rightwin" | "rwin" => Some(0x5C),
        "apps" | "menu" | "contextmenu" => Some(0x5D),
        "leftshift" => Some(0xA0),
        "rightshift" => Some(0xA1),
        "leftctrl" => Some(0xA2),
        "rightctrl" => Some(0xA3),
        "leftalt" => Some(0xA4),
        "rightalt" | "ralt" | "rmenu" => Some(0xA5),
        "volume_mute" | "volumemute" | "mute" => Some(0xAD),
        "volume_down" | "volumedown" => Some(0xAE),
        "volume_up" | "volumeup" => Some(0xAF),
        "k" => Some(0x4B),
        other if other.len() == 1 => {
            let c = other.chars().next()?.to_ascii_uppercase();
            if c.is_ascii_alphanumeric() {
                Some(c as u16)
            } else {
                None
            }
        }
        _ => None,
    }
}

pub fn names_to_vks(names: &[String]) -> Vec<u16> {
    names.iter().filter_map(|n| name_to_vk(n)).collect()
}

/// AutoHotkey / prevent-alt-win-menu 惯例：无键帽 vkE8。
/// Win/Alt 点按抬起后补一下，避免开始菜单 / 窗口菜单栏，且不向目标应用发 Esc。
pub const SHELL_MENU_DUMMY_VK: u16 = 0xE8;

/// 全部可能粘住的修饰键（含左右 + 通用）。注入后必须清干净，否则整机卡死。
pub const ALL_MODIFIER_VKS: &[u16] = &[
    0xA0, 0xA1, // Shift
    0xA2, 0xA3, // Ctrl — 用户反馈漏 Ctrl 且按住
    0xA4, 0xA5, // Alt — 漏 Alt
    0x5B, 0x5C, // Win
    0x10, 0x11, 0x12, // generic Shift/Ctrl/Alt
];

pub fn chord_has_modifier(vks: &[u16]) -> bool {
    vks.iter()
        .any(|&vk| crate::bridges::xiaomi::voice_chord_sanitizer::is_modifier_vk(vk))
}

pub fn mapped_tap_needs_shell_dummy(vks: &[u16]) -> bool {
    vks.iter()
        .any(|&vk| matches!(vk, 0x12 | 0xA4 | 0xA5 | 0x5B | 0x5C))
}

/// 含 Alt / Win 的映射必须整组点按，不能闩锁成「第一次按下、第二次抬起」。
/// 含右 Alt+Space（用户语音映射）：Hold 模式下也不得闩住 Alt。
pub fn mapped_voice_is_tap(vks: &[u16]) -> bool {
    vks.iter()
        .any(|&vk| matches!(vk, 0x12 | 0xA4 | 0xA5 | 0x5B | 0x5C))
}

/// 映射点按成功后：仅非语音路径；含 Win/Alt 时补 dummy。
/// 调用前应已 [`panic_clear_all_modifiers`]。
pub fn after_mapped_tap(vks: &[u16]) {
    if !mapped_tap_needs_shell_dummy(vks) {
        return;
    }
    crate::bridges::t1::native_suppress::allow_pass_vks(&[SHELL_MENU_DUMMY_VK]);
    if tap_vks(&[SHELL_MENU_DUMMY_VK], 1) {
        log::info!("T1 mapped tap shell-menu dummy vk=0x{SHELL_MENU_DUMMY_VK:02X}");
    }
    // dummy 后再清一次，避免 0xE8 过程中又粘住修饰键
    panic_clear_all_modifiers("after_shell_dummy");
}

/// **硬清全部修饰键**：WinUHid 全零 + 逐个 SendInput KEYUP。
/// 任何 T1 注入（语音/映射）结束后都应调用；漏 Alt/Ctrl 按住会导致系统无法使用。
pub fn panic_clear_all_modifiers(reason: &str) {
    let _ = crate::bridges::xiaomi::hid_injector::release_all();
    crate::bridges::t1::native_suppress::allow_pass_vks(ALL_MODIFIER_VKS);
    // 逐个 KEYUP 比整组更可靠（粘住的可能只有其中一个）
    for &vk in ALL_MODIFIER_VKS {
        let _ = release_vks(&[vk]);
    }
    let _ = crate::bridges::xiaomi::hid_injector::release_all();
    log::info!("T1 panic_clear_all_modifiers reason={reason}");
}

/// 语音点按收尾：立刻全清 + 短延迟再清一轮（壳层有时晚一拍进入菜单态）。
pub fn sanitize_after_voice_chord_tap(vks: &[u16]) {
    let _ = vks;
    panic_clear_all_modifiers("voice_tap_sync");
    thread::spawn(|| {
        thread::sleep(Duration::from_millis(80));
        panic_clear_all_modifiers("voice_tap_delayed_80ms");
        thread::sleep(Duration::from_millis(220));
        panic_clear_all_modifiers("voice_tap_delayed_300ms");
    });
}

/// 语音点按前：先清残留，避免上一次漏的 Ctrl/Alt 带进本次和弦。
pub fn sanitize_before_voice_chord_tap(vks: &[u16]) {
    let _ = vks;
    panic_clear_all_modifiers("voice_tap_pre");
}

/// 安全点按任意映射（含修饰键）：多修饰键/Win **禁止**分步 press（会先露出 Win/Ctrl）。
pub fn safe_mapped_tap(vks: &[u16], hold_ms: u64) -> bool {
    if vks.is_empty() {
        return false;
    }
    // 音量/浏览器等不在 WinUHid boot keyboard 内 → 直接 SendInput
    if crate::bridges::t1::native_suppress::vks_need_sendinput(vks) {
        crate::bridges::t1::native_suppress::allow_pass_vks(vks);
        let ok = tap_vks(vks, hold_ms);
        let _ = crate::bridges::xiaomi::hid_injector::release_all();
        return ok;
    }
    let needs_atomic = chord_has_modifier(vks);
    crate::bridges::t1::native_suppress::allow_pass_vks(vks);
    let ok = if crate::bridges::xiaomi::hid_injector::is_available() {
        if needs_atomic {
            log::info!(
                "T1 mapped tap atomic vks={}",
                vks.iter()
                    .map(|v| format!("0x{v:02X}"))
                    .collect::<Vec<_>>()
                    .join("+")
            );
            crate::bridges::xiaomi::hid_injector::tap_vks_atomic(vks, hold_ms)
        } else {
            crate::bridges::xiaomi::hid_injector::tap_vks(vks, hold_ms)
        }
    } else {
        log::warn!("T1 mapped tap: WinUHid unavailable, SendInput fallback");
        tap_vks(vks, hold_ms)
    };
    if needs_atomic {
        panic_clear_all_modifiers("mapped_tap_mods");
    } else {
        let _ = crate::bridges::xiaomi::hid_injector::release_all();
    }
    ok
}

/// 语音点按：始终原子 HID + 全修饰键收尾。
pub fn voice_chord_tap(vks: &[u16], hold_ms: u64) -> bool {
    if vks.is_empty() {
        return false;
    }
    sanitize_before_voice_chord_tap(vks);
    crate::bridges::t1::native_suppress::allow_pass_vks(vks);
    let ok = if crate::bridges::xiaomi::hid_injector::is_available() {
        log::info!(
            "T1 voice chord tap atomic vks={}",
            vks.iter()
                .map(|v| format!("0x{v:02X}"))
                .collect::<Vec<_>>()
                .join("+")
        );
        crate::bridges::xiaomi::hid_injector::tap_vks_atomic(vks, hold_ms)
    } else {
        log::warn!("T1 voice chord: WinUHid unavailable, SendInput fallback");
        tap_vks(vks, hold_ms)
    };
    sanitize_after_voice_chord_tap(vks);
    ok
}

/// 点按组合键（按下 → 短暂保持 → 抬起）
pub fn tap_vks(vks: &[u16], hold_ms: u64) -> bool {
    if vks.is_empty() {
        return false;
    }
    if !press_vks(vks) {
        return false;
    }
    thread::sleep(Duration::from_millis(hold_ms.max(1)));
    release_vks(vks)
}

pub fn tap_single_vk(vk: u16, hold_ms: u64) -> bool {
    tap_vks(&[vk], hold_ms)
}

/// SendInput 仅按下（Hold 语义 / 回退用）
pub fn press_vks(vks: &[u16]) -> bool {
    if vks.is_empty() {
        return false;
    }
    send_chord(vks, false)
}

/// SendInput 仅抬起
pub fn release_vks(vks: &[u16]) -> bool {
    if vks.is_empty() {
        return false;
    }
    send_chord(vks, true)
}

#[cfg(target_os = "windows")]
fn is_extended(vk: u16) -> bool {
    matches!(
        vk,
        0x21 | 0x22 | 0x23 | 0x24 | 0x25 | 0x26 | 0x27 | 0x28 | 0x2C | 0x2D | 0x2E | 0x5B
            | 0x5C | 0x5D | 0xA3 | 0xA5 | 0xAD | 0xAE | 0xAF
    )
}

#[cfg(target_os = "windows")]
fn send_chord(vks: &[u16], key_up: bool) -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
        KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, VIRTUAL_KEY,
    };

    let iter: Box<dyn Iterator<Item = &u16>> = if key_up {
        Box::new(vks.iter().rev())
    } else {
        Box::new(vks.iter())
    };

    let mut inputs: Vec<INPUT> = Vec::with_capacity(vks.len());
    for &vk in iter {
        let mapped = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as u16;
        // MapVirtualKey(VK_RMENU) 部分环境返回 0，输入法认不出右 Alt
        let scan = crate::bridges::xiaomi::voice_inject::scan_code_for_vk(vk, mapped);
        let mut flags = if is_extended(vk) {
            KEYEVENTF_EXTENDEDKEY
        } else {
            Default::default()
        };
        if key_up {
            flags |= KEYEVENTF_KEYUP;
        }
        inputs.push(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: scan,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: crate::bridges::xiaomi::key_mapping::EXTRA_INFO,
                },
            },
        });
    }
    if inputs.is_empty() {
        return false;
    }
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    sent as usize == inputs.len()
}

#[cfg(not(target_os = "windows"))]
fn send_chord(_vks: &[u16], _key_up: bool) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_leftshift_k() {
        assert_eq!(name_to_vk("leftshift"), Some(0xA0));
        assert_eq!(name_to_vk("k"), Some(0x4B));
        assert_eq!(
            names_to_vks(&["leftshift".into(), "k".into()]),
            vec![0xA0, 0x4B]
        );
    }

    #[test]
    fn all_modifier_vks_cover_ctrl_alt_win() {
        assert!(ALL_MODIFIER_VKS.contains(&0xA2), "must clear LCtrl");
        assert!(ALL_MODIFIER_VKS.contains(&0xA4), "must clear LAlt");
        assert!(ALL_MODIFIER_VKS.contains(&0x5B), "must clear LWin");
        assert!(chord_has_modifier(&[0x5B, 0xA4]));
        assert!(!chord_has_modifier(&[0x28]));
    }

    #[test]
    fn win_alt_chord_needs_shell_dummy_but_voice_uses_sanitizer() {
        // 普通映射仍可 after_mapped_tap；语音必须走 sanitize_after_voice_chord_tap
        assert!(mapped_tap_needs_shell_dummy(&[0x5B, 0xA4]));
        assert!(mapped_voice_is_tap(&[0x5B, 0xA4]));
    }

    #[test]
    fn voice_win_alt_is_multi_modifier_chord() {
        let mods: Vec<_> = [0x5Bu16, 0xA4]
            .into_iter()
            .filter(|&vk| crate::bridges::xiaomi::voice_chord_sanitizer::is_modifier_vk(vk))
            .collect();
        assert!(mods.len() >= 2, "Win+Alt must use atomic HID tap");
    }
}
