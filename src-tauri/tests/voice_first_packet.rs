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
