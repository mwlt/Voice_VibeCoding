//! Voice chord release hygiene — pure planning + Windows recovery helpers.
//!
//! SendInput is **never** used for IME wake (filtered by 豆包/千问). Recovery KEYUP only.

use std::sync::atomic::{AtomicU64, Ordering};

static RECOVER_COUNT: AtomicU64 = AtomicU64::new(0);

/// 标准修饰键 VK（与 `hid_injector::modifier_bit` 对齐）
pub const MODIFIER_VKS: [u16; 8] = [0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0x5B, 0x5C];

pub fn is_modifier_vk(vk: u16) -> bool {
    matches!(vk, 0x10 | 0x11 | 0x12) || MODIFIER_VKS.contains(&vk)
}

/// UP 后需要检查/清掉的修饰键：和弦内全部修饰键（**含 Win**，不再 skip）。
pub fn sanitizer_targets(chord: &[u16]) -> Vec<u16> {
    let mut out = Vec::new();
    for &vk in chord {
        if is_modifier_vk(vk) && !out.contains(&vk) {
            out.push(vk);
        }
    }
    out
}

/// DOWN 前清理：不在当前和弦内、却仍按下的修饰键。
pub fn foreign_modifiers_for_chord(chord: &[u16]) -> Vec<u16> {
    MODIFIER_VKS
        .iter()
        .copied()
        .filter(|vk| !chord_contains_modifier(chord, *vk))
        .collect()
}

fn chord_contains_modifier(chord: &[u16], vk: u16) -> bool {
    chord.iter().any(|&c| {
        c == vk || (c == 0x12 && matches!(vk, 0xA4 | 0xA5)) || (c == 0x11 && matches!(vk, 0xA2 | 0xA3))
    })
}

/// 纯函数：哪些和弦修饰键仍报告为按下（测试缝）。
pub fn modifiers_still_down(chord: &[u16], is_down: impl Fn(u16) -> bool) -> Vec<u16> {
    sanitizer_targets(chord)
        .into_iter()
        .filter(|&vk| is_down(vk))
        .collect()
}

pub fn is_win_modifier(vk: u16) -> bool {
    matches!(vk, 0x5B | 0x5C)
}

/// UP 后应补 KEYUP 的修饰键。
///
/// **实验（2026-08-31）**：不再对 Win 无条件补 SendInput KEYUP。
/// 原先 HID 松开后再补一次会导致键盘测试软件见双 SYSKeyUp；
/// 用户实测是否影响微信语音唤醒。仅当 `GetAsyncKeyState` 仍报按下时才补。
pub fn modifiers_to_recover(chord: &[u16], is_down: impl Fn(u16) -> bool) -> Vec<u16> {
    sanitizer_targets(chord)
        .into_iter()
        .filter(|&vk| is_down(vk))
        .collect()
}

pub fn recover_count() -> u64 {
    RECOVER_COUNT.load(Ordering::Relaxed)
}

fn bump_recover(n: u32) {
    if n > 0 {
        RECOVER_COUNT.fetch_add(n as u64, Ordering::Relaxed);
    }
}

/// WinUHid UP 后：仅当修饰键仍 down 时补 SendInput KEYUP（含 Win，不再强制）。
#[cfg(target_os = "windows")]
pub fn recover_chord_modifiers(chord: &[u16], send_keyup: impl Fn(&[u16]) -> bool) -> u32 {
    let stuck = modifiers_to_recover(chord, |vk| {
        use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
        (unsafe { GetAsyncKeyState(vk as i32) } as u16) & 0x8000 != 0
    });
    let mut cleared = 0u32;
    for vk in stuck {
        if send_keyup(&[vk]) {
            cleared += 1;
            log::info!("XIAOMI VOICE sanitizer cleared stuck vk=0x{vk:02X}");
        }
    }
    bump_recover(cleared);
    cleared
}

#[cfg(not(target_os = "windows"))]
pub fn recover_chord_modifiers(_chord: &[u16], _send_keyup: impl Fn(&[u16]) -> bool) -> u32 {
    0
}

/// DOWN 前：清和弦外的残留修饰键。
#[cfg(target_os = "windows")]
pub fn recover_foreign_modifiers(chord: &[u16], send_keyup: impl Fn(&[u16]) -> bool) -> u32 {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    let mut cleared = 0u32;
    for vk in foreign_modifiers_for_chord(chord) {
        let down = (unsafe { GetAsyncKeyState(vk as i32) } as u16) & 0x8000 != 0;
        if down && send_keyup(&[vk]) {
            cleared += 1;
            log::info!("XIAOMI VOICE sanitizer cleared foreign vk=0x{vk:02X}");
        }
    }
    bump_recover(cleared);
    cleared
}

#[cfg(not(target_os = "windows"))]
pub fn recover_foreign_modifiers(_chord: &[u16], _send_keyup: impl Fn(&[u16]) -> bool) -> u32 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreign_excludes_ctrl_win() {
        let f = foreign_modifiers_for_chord(&[0xA2, 0x5B]);
        assert!(!f.contains(&0xA2));
        assert!(!f.contains(&0x5B));
    }

    #[test]
    fn win_not_force_recovered_when_async_reads_up() {
        // 实验：取消 Win 无条件补 KEYUP；async 已抬起则不补
        assert!(modifiers_to_recover(&[0xA2, 0x5B], |_| false).is_empty());
        assert!(modifiers_to_recover(&[0x5B, 0xA4], |_| false).is_empty());
        assert_eq!(
            modifiers_to_recover(&[0xA2, 0x5B], |vk| vk == 0x5B),
            vec![0x5B],
            "still recover Win when async reports down"
        );
        assert!(modifiers_to_recover(&[0xA5], |_| false).is_empty());
    }
}
