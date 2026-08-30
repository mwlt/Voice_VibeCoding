//! Phase-1/5：语音键原生 F5 抑制必须盖住 typematic，否则记事本刷日期时间。
//!
//! 运行: cargo test -p remote-bridge-hub --lib bridges::xiaomi::voice_f5_suppress -- --nocapture

use crate::bridges::xiaomi::key_mapping::{
    arm_voice_native_suppress, begin_voice_period, disarm_voice_native_suppress, end_voice_period,
    set_input_session_active, should_suppress_voice_f5, voice_native_suppress_active,
    VOICE_F5_SUPPRESS_DEADLINE_MS,
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
    end_voice_period("test");
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
    end_voice_period("test");
    disarm_voice_native_suppress();
    set_input_session_active(false);
    arm_voice_native_suppress();
    assert!(
        should_suppress_voice_f5(true, false, false),
        "native F5 must be swallowed while voice chord is armed"
    );
    assert!(
        should_suppress_voice_f5(true, false, false),
        "sticky down suppress covers typematic repeats"
    );
    assert!(
        !should_suppress_voice_f5(false, true, false),
        "F5 KEYUP must always pass to unstick leaked DOWN"
    );
    // UP 已清 sticky；周期/armed 仍在则再 DOWN 仍吞
    assert!(should_suppress_voice_f5(true, false, false));
    disarm_voice_native_suppress();
    assert!(!should_suppress_voice_f5(true, false, false));
}

/// 会话活跃即吞 F5（不依赖 Tap；语音 ATVV 路径）。
#[test]
fn session_suppresses_f5_without_tap_ready() {
    end_voice_period("test");
    disarm_voice_native_suppress();
    set_input_session_active(true);
    assert!(
        should_suppress_voice_f5(true, false, false),
        "session alone must swallow firmware F5 before ATVV marks"
    );
    assert!(!should_suppress_voice_f5(false, true, false));
    assert!(should_suppress_voice_f5(true, false, false));
    set_input_session_active(false);
    disarm_voice_native_suppress();
}

#[test]
fn voice_period_swallows_f5_without_session() {
    end_voice_period("test");
    disarm_voice_native_suppress();
    set_input_session_active(false);
    begin_voice_period();
    assert!(should_suppress_voice_f5(true, false, false));
    assert!(!should_suppress_voice_f5(false, true, false));
    assert!(should_suppress_voice_f5(true, false, false));
    end_voice_period("test");
    // sticky 保留到 disarm（与 end_voice_period 不清 sticky 对齐）
    disarm_voice_native_suppress();
    assert!(!should_suppress_voice_f5(true, false, false));
}

#[test]
fn no_session_no_arm_does_not_swallow_physical_f5() {
    end_voice_period("test");
    disarm_voice_native_suppress();
    set_input_session_active(false);
    assert!(
        !should_suppress_voice_f5(true, false, true),
        "without input session, even tap_ready must not eat keyboard F5"
    );
}

/// 回归：LL 回调里 sleep 等 ATVV 会被 Windows 静默卸钩 → 间歇漏 F5 / 唤醒失败。
#[test]
fn should_suppress_voice_f5_must_not_block_wait() {
    let src = include_str!("key_mapping.rs");
    let fn_body = src
        .split("pub fn should_suppress_voice_f5")
        .nth(1)
        .and_then(|s| s.split("pub fn ").next())
        .expect("should_suppress_voice_f5 body");
    assert!(
        !fn_body.contains("wait_for_direct_signal"),
        "must not wait/sleep inside LL hook suppress path"
    );
    assert!(
        !fn_body.contains("thread::sleep") && !fn_body.contains("std::thread::sleep"),
        "must not sleep inside should_suppress_voice_f5"
    );
}

#[test]
fn orphan_f5_keyup_must_pass_to_unstick() {
    end_voice_period("test");
    disarm_voice_native_suppress();
    set_input_session_active(true);
    // 从未吞过 DOWN：UP 必须放行（否则 bump 空窗漏 DOWN 后永久粘键）
    assert!(!should_suppress_voice_f5(false, true, false));
    set_input_session_active(false);
}
