//! Diagnosis: IME voice wake requires WinUHid (not SendInput).
//!
//! Run:
//!   cargo test --manifest-path src-tauri/Cargo.toml --test ime_voice_wake_route -- --nocapture

use remote_bridge_hub_lib::bridges::xiaomi::voice_inject::{
    sanitize_own_inject_flags, voice_chord_inject_route, VoiceChordInjectRoute, LLKHF_INJECTED,
    LLKHF_INJECTED_MASK,
};

fn ime_voice_chords() -> Vec<(&'static str, Vec<u16>)> {
    vec![
        ("doubao/qianwen Right Alt", vec![0xA5]),
        ("wechat/qianwen Ctrl+Win", vec![0xA2, 0x5B]),
        ("wechat hold Ctrl+Shift+D", vec![0xA2, 0xA0, 0x44]),
        ("doubao hands-free Alt+Space", vec![0xA5, 0x20]),
    ]
}

#[test]
fn ime_voice_chords_require_virtual_hid() {
    for (label, chord) in ime_voice_chords() {
        assert_eq!(
            voice_chord_inject_route(&chord),
            VoiceChordInjectRoute::RequireVirtualHid,
            "{label}: voice wake must use WinUHid, not SendInput"
        );
    }
}

#[test]
fn sanitize_helper_still_strips_own_extra_for_optional_diag() {
    let own = 0x584D_4952usize;
    let flags = LLKHF_INJECTED | 0x01;
    let cleaned = sanitize_own_inject_flags(own, own, flags);
    assert_eq!(cleaned & LLKHF_INJECTED_MASK, 0);
    assert_eq!(cleaned & 0x01, 0x01);
}
