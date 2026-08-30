//! Voice wake inject backend: WinUHid when available, else SendInput fallback.
//!
//! Run:
//!   cargo test --manifest-path src-tauri/Cargo.toml --test ime_voice_wake_route -- --nocapture

use remote_bridge_hub_lib::bridges::xiaomi::voice_inject::{
    sanitize_own_inject_flags, voice_inject_backend, VoiceInjectBackend, LLKHF_INJECTED,
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
fn when_winuhid_available_prefer_virtual_hid() {
    for (label, chord) in ime_voice_chords() {
        assert_eq!(
            voice_inject_backend(&chord, true),
            VoiceInjectBackend::VirtualHid,
            "{label}: WinUHid up → virtual HID only"
        );
    }
}

#[test]
fn when_winuhid_unavailable_fallback_sendinput() {
    for (label, chord) in ime_voice_chords() {
        assert_eq!(
            voice_inject_backend(&chord, false),
            VoiceInjectBackend::SendInputFallback,
            "{label}: WinUHid down → SendInput degraded (not blocked)"
        );
    }
}

#[test]
fn backends_are_mutually_exclusive() {
    assert_ne!(
        VoiceInjectBackend::VirtualHid,
        VoiceInjectBackend::SendInputFallback
    );
    // Same press must never pick both — availability is a single bool.
    assert_eq!(voice_inject_backend(&[], true), VoiceInjectBackend::VirtualHid);
    assert_eq!(
        voice_inject_backend(&[], false),
        VoiceInjectBackend::SendInputFallback
    );
}

#[test]
fn sanitize_helper_still_strips_own_extra_for_optional_diag() {
    let own = 0x584D_4952usize;
    let flags = LLKHF_INJECTED | 0x01;
    let cleaned = sanitize_own_inject_flags(own, own, flags);
    assert_eq!(cleaned & LLKHF_INJECTED_MASK, 0);
    assert_eq!(cleaned & 0x01, 0x01);
}
