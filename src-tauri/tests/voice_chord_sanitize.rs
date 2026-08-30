//! Integration tests for voice chord release sanitizer (R1).

use remote_bridge_hub_lib::bridges::xiaomi::voice_chord_sanitizer::{
    foreign_modifiers_for_chord, modifiers_still_down, sanitizer_targets,
};
use remote_bridge_hub_lib::bridges::xiaomi::voice_inject::normalize_voice_chord_vks;

#[test]
fn sanitizer_targets_ctrl_win_chord() {
    let chord = normalize_voice_chord_vks(&[0x11, 0x5B]); // generic Ctrl + Win → LCtrl + LWin
    assert_eq!(chord, vec![0xA2, 0x5B]);
    assert_eq!(sanitizer_targets(&chord), vec![0xA2, 0x5B]);
}

#[test]
fn sanitizer_targets_right_alt_only() {
    assert_eq!(sanitizer_targets(&[0xA5]), vec![0xA5]);
}

#[test]
fn foreign_modifiers_excludes_chord_members() {
    let chord = vec![0xA2, 0x5B];
    let foreign = foreign_modifiers_for_chord(&chord);
    assert!(!foreign.contains(&0xA2));
    assert!(!foreign.contains(&0x5B));
    assert!(foreign.contains(&0xA4)); // left Alt still foreign
}

#[test]
fn modifiers_still_down_filters_by_callback() {
    let chord = vec![0xA2, 0x5B];
    let stuck = modifiers_still_down(&chord, |vk| vk == 0x5B);
    assert_eq!(stuck, vec![0x5B]);
}

#[test]
fn win_in_voice_chord_is_always_recovered_even_if_async_state_reads_up() {
    use remote_bridge_hub_lib::bridges::xiaomi::voice_chord_sanitizer::modifiers_to_recover;
    // HID 全零后 GetAsyncKeyState 常已读成抬起，但 Explorer 仍咬着 Win → 必须无条件补 KEYUP
    assert_eq!(
        modifiers_to_recover(&[0xA2, 0x5B], |_| false),
        vec![0x5B],
        "Ctrl+Win: force LWin KEYUP even when async state says up"
    );
    assert_eq!(
        modifiers_to_recover(&[0x5B, 0xA4], |_| false),
        vec![0x5B],
        "Win+Alt: force LWin KEYUP even when async state says up"
    );
    assert!(
        modifiers_to_recover(&[0xA5], |_| false).is_empty(),
        "Right Alt alone: no Win to force"
    );
    assert_eq!(
        modifiers_to_recover(&[0xA5], |vk| vk == 0xA5),
        vec![0xA5],
        "non-Win modifiers still recover only when still down"
    );
}
