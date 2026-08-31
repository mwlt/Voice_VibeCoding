//! W2 — voice wake latency: bump settle must not add ~40ms before inject.
//!
//! 运行: cargo test --test voice_wake_latency -- --nocapture

use remote_bridge_hub_lib::bridges::xiaomi::key_mapping::voice_bump_settle_ms;

#[test]
fn voice_bump_settle_is_at_most_10ms() {
    assert!(
        voice_bump_settle_ms() <= 10,
        "wake path bump settle must be <=10ms, got {}",
        voice_bump_settle_ms()
    );
}

#[test]
fn handle_voice_uses_voice_bump_settle_ms_not_hardcoded_40() {
    let src = include_str!("../src/bridges/xiaomi/key_mapping.rs");
    let handle = src
        .split("fn handle_voice")
        .nth(1)
        .and_then(|s| s.split("\nfn ").next())
        .expect("handle_voice");
    assert!(
        !handle.contains("bump_hook_to_front_and_settle(40)"),
        "handle_voice must not hardcode 40ms settle (wake latency)"
    );
    assert!(
        handle.contains("voice_bump_settle_ms()")
            || handle.contains("VOICE_BUMP_SETTLE_MS"),
        "handle_voice must use VOICE_BUMP_SETTLE_MS / voice_bump_settle_ms()"
    );
}
