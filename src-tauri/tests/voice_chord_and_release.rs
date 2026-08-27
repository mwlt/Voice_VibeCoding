//! Integration tests for VoiceChordState (S3) and voice inject route helpers (S4/S5).

use remote_bridge_hub_lib::bridges::xiaomi::voice_chord_state::VoiceChordState;
use remote_bridge_hub_lib::bridges::xiaomi::voice_inject::{
    has_alt_modifier, scan_code_for_vk, should_suppress_alt_menu_after_keyup,
    ALT_MENU_SUPPRESS_DUMMY_VK,
};

#[test]
fn releases_original_keys_and_is_idempotent() {
    let keys = vec![0xA2u16, 0x5B];
    let mut state = VoiceChordState::default();
    assert!(state.press_with(&keys, |_keys, _up| true));
    assert_eq!(state.release_with(|_keys, _up| true), Some((keys, true)));
    assert!(state.release_with(|_keys, _up| true).is_none());
}

#[test]
fn partial_down_is_compensated_with_keyup() {
    let mut state = VoiceChordState::default();
    let mut phases = Vec::new();
    assert!(!state.press_with(&[0xA2, 0x5B], |_keys, up| {
        phases.push(up);
        false
    }));
    assert_eq!(phases, vec![false, true]);
    assert!(!state.is_held());
}

#[test]
fn release_retries_once_when_first_keyup_fails() {
    let mut state = VoiceChordState::default();
    assert!(state.press_with(&[0xA2, 0x5B], |_keys, _up| true));
    let mut attempts = 0;
    assert_eq!(
        state.release_with(|_keys, up| {
            assert!(up);
            attempts += 1;
            attempts == 2
        }),
        Some((vec![0xA2, 0x5B], true))
    );
    assert_eq!(attempts, 2);
}

#[test]
fn press_while_held_releases_previous_first() {
    let mut state = VoiceChordState::default();
    let mut log: Vec<(Vec<u16>, bool)> = Vec::new();
    assert!(state.press_with(&[0xA2, 0x5B], |keys, up| {
        log.push((keys.to_vec(), up));
        true
    }));
    assert!(state.press_with(&[0xA2, 0x5B], |keys, up| {
        log.push((keys.to_vec(), up));
        true
    }));
    assert_eq!(log.len(), 4);
    assert_eq!(log[0], (vec![0xA2, 0x5B], false));
    assert_eq!(log[1], (vec![0xA2, 0x5B], true));
    assert_eq!(log[2], (vec![0xA2, 0x5B], true));
    assert_eq!(log[3], (vec![0xA2, 0x5B], false));
}

#[test]
fn right_alt_menu_suppress_runs_after_keyup_not_before() {
    // 豆包/千问：右 Alt 按住说话；须在 Alt UP 之后再插 dummy，UP 前插会打断唤起
    assert!(should_suppress_alt_menu_after_keyup(&[0xA5], true));
    assert!(!should_suppress_alt_menu_after_keyup(&[0xA5], false));
    assert!(should_suppress_alt_menu_after_keyup(&[0xA5, 0x20], true));
    assert!(!should_suppress_alt_menu_after_keyup(&[0xA2, 0x5B], true));
    assert_eq!(ALT_MENU_SUPPRESS_DUMMY_VK, 0xE8);
}

#[test]
fn alt_voice_chords_are_detected_for_sendinput_route() {
    assert!(has_alt_modifier(&[0xA5]));
    assert!(has_alt_modifier(&[0xA5, 0x20]));
    assert!(has_alt_modifier(&[0xA4]));
    assert!(!has_alt_modifier(&[0xA2, 0x5B]));
}

#[test]
fn right_alt_uses_scan_0x38_even_when_mapvirtualkey_returns_zero() {
    assert_eq!(scan_code_for_vk(0xA5, 0), 0x38);
    assert_eq!(scan_code_for_vk(0xA4, 0), 0x38);
    assert_eq!(scan_code_for_vk(0x41, 0x1E), 0x1E);
}
