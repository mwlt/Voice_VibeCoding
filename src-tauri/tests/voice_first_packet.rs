//! Integration tests for voice first-packet latency plan (L1/L2).

use remote_bridge_hub_lib::bridges::xiaomi::voice_pcm::{ping_deadline_secs, ping_retry_interval_ms};
use remote_bridge_hub_lib::bridges::xiaomi::voice_press::{
    voice_remote_press_steps, VoicePressStep,
};

#[test]
fn press_plan_puts_shortcut_before_pcm_clear() {
    let steps = voice_remote_press_steps();
    let down = steps
        .iter()
        .position(|&s| s == VoicePressStep::ShortcutDown)
        .expect("ShortcutDown");
    let clear = steps
        .iter()
        .position(|&s| s == VoicePressStep::PcmClear)
        .expect("PcmClear");
    assert!(down < clear, "IME shortcut must fire before VB-CABLE CLEAR");
}

/// W1 — wake latency: IME inject must not wait on PCM UDP ensure.
#[test]
fn press_plan_puts_shortcut_before_pcm_ensure() {
    let steps = voice_remote_press_steps();
    let down = steps
        .iter()
        .position(|&s| s == VoicePressStep::ShortcutDown)
        .expect("ShortcutDown");
    let ensure = steps
        .iter()
        .position(|&s| s == VoicePressStep::EnsurePcmReady)
        .expect("EnsurePcmReady");
    assert!(
        down < ensure,
        "IME ShortcutDown must precede EnsurePcmReady (wake must not block on PCM)"
    );
}

/// W1 — on_voice_remote_press body must call on_remote_button before ensure_pcm.
#[test]
fn on_voice_remote_press_injects_before_pcm_ensure() {
    let src = include_str!("../src/bridges/xiaomi/input_session.rs");
    let fn_start = src
        .find("fn on_voice_remote_press")
        .expect("on_voice_remote_press");
    let body = &src[fn_start..];
    let end = body.find("\nfn ").unwrap_or(body.len().min(2500));
    let body = &body[..end];
    let inject = body
        .find("on_remote_button")
        .expect("on_remote_button in press path");
    let pcm = body
        .find("ensure_pcm_ready_on_press")
        .expect("ensure_pcm_ready_on_press");
    assert!(
        inject < pcm,
        "on_remote_button (ShortcutDown) must run before ensure_pcm_ready_on_press"
    );
}

#[test]
fn ping_retry_interval_is_aggressive_for_cold_start() {
    assert!(
        ping_retry_interval_ms() <= 20,
        "PING retry should be <=20ms, got {}",
        ping_retry_interval_ms()
    );
}

#[test]
fn ping_deadline_keeps_bounded_wait() {
    assert_eq!(ping_deadline_secs(), 4);
}
