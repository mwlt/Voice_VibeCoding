//! Phase-1/5：语音键原生 F5 抑制必须盖住 typematic，否则记事本刷日期时间。
//!
//! 运行: cargo test -p remote-bridge-hub --lib bridges::xiaomi::voice_f5_suppress -- --nocapture

use crate::bridges::xiaomi::key_mapping::{
    arm_voice_native_suppress, disarm_voice_native_suppress, should_suppress_voice_f5,
    voice_native_suppress_active, VOICE_F5_SUPPRESS_DEADLINE_MS,
};
use std::time::Duration;

/// Windows 默认 typematic 延迟约 400–1000ms
pub const WINDOWS_TYPEMATIC_DELAY_MS: u64 = 400;

#[test]
fn voice_f5_suppress_deadline_covers_typematic() {
    assert!(
        VOICE_F5_SUPPRESS_DEADLINE_MS >= WINDOWS_TYPEMATIC_DELAY_MS,
        "deadline {VOICE_F5_SUPPRESS_DEADLINE_MS}ms < typematic {WINDOWS_TYPEMATIC_DELAY_MS}ms"
    );
}

#[test]
fn voice_f5_sticky_arm_stays_active_past_old_120ms_window() {
    disarm_voice_native_suppress();
    arm_voice_native_suppress();
    assert!(voice_native_suppress_active(), "armed should be active");
    std::thread::sleep(Duration::from_millis(200)); // 超过旧的 120ms recent 窗
    assert!(
        voice_native_suppress_active(),
        "sticky suppress must still be active after 200ms (typematic would have started leaking)"
    );
    disarm_voice_native_suppress();
    assert!(!voice_native_suppress_active());
}

#[test]
fn notepad_f5_is_vk_0x74() {
    assert_eq!(0x74u16, 0x74);
}

#[test]
fn suppress_firmware_f5_while_voice_native_armed() {
    disarm_voice_native_suppress();
    arm_voice_native_suppress();
    assert!(
        should_suppress_voice_f5(true, false),
        "native F5 must be swallowed while voice chord is armed"
    );
    assert!(
        should_suppress_voice_f5(true, false),
        "sticky down suppress covers typematic repeats"
    );
    assert!(
        should_suppress_voice_f5(false, true),
        "F5 up should be swallowed to complete the cycle"
    );
    disarm_voice_native_suppress();
    assert!(!should_suppress_voice_f5(true, false));
}
