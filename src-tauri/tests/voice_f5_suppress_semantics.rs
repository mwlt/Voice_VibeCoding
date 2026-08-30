//! Step 1 — F5 sticky 抑制语义（docs/VOICE_F5_LONGTERM_PLAN.md）
//!
//! 关键契约：
//! - `end_voice_period` **不得**清 sticky（ATVV 松开早于 F5 KEYUP）
//! - 已 sticky 时，armed 3s 截止到期仍续期吞键（长按 typematic）
//! - 长时间无 F5 事件 → 自动解粘（防永久吞真键盘 F5）
//! - `disarm` 才是明确停手，一并清 sticky
//! - orphan KEYUP 放行；无会话不吞物理 F5
//!
//! 运行: cargo test --test voice_f5_suppress_semantics -- --nocapture

use remote_bridge_hub_lib::bridges::xiaomi::key_mapping::{
    arm_voice_native_suppress, begin_voice_period, disarm_voice_native_suppress, end_voice_period,
    set_input_session_active, should_suppress_voice_f5, voice_f5_down_suppressed,
    voice_f5_expire_suppress_deadline_for_test, voice_f5_reset_for_test,
    voice_f5_set_last_event_age_for_test, voice_native_suppress_active, VOICE_F5_STICKY_MAX_IDLE_MS,
};
use std::time::Duration;

fn reset() {
    voice_f5_reset_for_test();
}

#[test]
fn end_voice_period_must_not_unstick_held_f5() {
    reset();
    set_input_session_active(true);
    assert!(should_suppress_voice_f5(true, false, false));
    // KEYUP 永远放行并清 sticky（防链头漏 DOWN 后粘死）
    assert!(!should_suppress_voice_f5(false, true, false));
    assert!(!voice_f5_down_suppressed());
    end_voice_period("atvv_early_up");
    // ATVV 已松，固件仍可能发 typematic DOWN：须继续吞 DOWN
    assert!(
        should_suppress_voice_f5(true, false, false),
        "end_voice_period must NOT clear sticky while F5 still held (DOWN path)"
    );
    assert!(!should_suppress_voice_f5(false, true, false));
    set_input_session_active(false);
    disarm_voice_native_suppress();
}

#[test]
fn long_press_past_deadline_still_suppresses() {
    reset();
    set_input_session_active(false);
    arm_voice_native_suppress();
    assert!(should_suppress_voice_f5(true, false, false));
    voice_f5_expire_suppress_deadline_for_test();
    // 截止已过，但 sticky 仍按住 → 必须续期并继续吞
    assert!(
        should_suppress_voice_f5(true, false, false),
        "sticky hold must renew past 3s armed deadline"
    );
    assert!(
        voice_native_suppress_active(),
        "armed flag must renew while F5 sticky is held"
    );
    disarm_voice_native_suppress();
}

#[test]
fn typematic_repeats_stay_suppressed_while_held() {
    reset();
    begin_voice_period();
    assert!(should_suppress_voice_f5(true, false, false));
    for _ in 0..30 {
        assert!(should_suppress_voice_f5(true, false, false));
    }
    assert!(
        !should_suppress_voice_f5(false, true, false),
        "KEYUP always passes"
    );
    assert!(!voice_f5_down_suppressed());
    end_voice_period("test");
    disarm_voice_native_suppress();
}

#[test]
fn sticky_unsticks_when_f5_stream_stops() {
    reset();
    // 仅语音周期：end_period 保留 sticky；不要 set_input_session_active(false)（会 disarm）
    begin_voice_period();
    assert!(should_suppress_voice_f5(true, false, false));
    end_voice_period("remote_up");
    assert!(voice_f5_down_suppressed());
    voice_f5_expire_suppress_deadline_for_test();
    voice_f5_set_last_event_age_for_test(VOICE_F5_STICKY_MAX_IDLE_MS + 50);
    assert!(
        !should_suppress_voice_f5(true, false, false),
        "after idle > {VOICE_F5_STICKY_MAX_IDLE_MS}ms sticky must auto-release so physical F5 works"
    );
    assert!(!voice_f5_down_suppressed());
    disarm_voice_native_suppress();
}

#[test]
fn session_end_disarms_sticky() {
    reset();
    set_input_session_active(true);
    assert!(should_suppress_voice_f5(true, false, false));
    assert!(voice_f5_down_suppressed());
    set_input_session_active(false);
    assert!(
        !voice_f5_down_suppressed(),
        "session end must disarm sticky so physical F5 works immediately after disconnect"
    );
    assert!(!should_suppress_voice_f5(true, false, false));
}

#[test]
fn disarm_clears_sticky_completely() {
    reset();
    begin_voice_period();
    assert!(should_suppress_voice_f5(true, false, false));
    end_voice_period("test");
    assert!(voice_f5_down_suppressed(), "period end keeps sticky");
    disarm_voice_native_suppress();
    assert!(!voice_f5_down_suppressed());
    assert!(!should_suppress_voice_f5(true, false, false));
}

#[test]
fn f5_keyup_always_passes_even_after_suppressed_down() {
    // 回归：网页键盘测试见 Ctrl+Win+F5 且 F5 粘死 → 因吞了 KEYUP。
    // DOWN 可能已漏到链前钩子/系统；UP 必须放行才能解粘，否则微信热键变成 Ctrl+Win+F5。
    reset();
    set_input_session_active(true);
    assert!(should_suppress_voice_f5(true, false, false));
    assert!(voice_f5_down_suppressed());
    assert!(!should_suppress_voice_f5(false, true, false));
    assert!(!voice_f5_down_suppressed());
    set_input_session_active(false);
    disarm_voice_native_suppress();
}

#[test]
fn no_session_no_arm_does_not_swallow_physical_f5() {
    reset();
    assert!(!should_suppress_voice_f5(true, false, true));
    assert!(!should_suppress_voice_f5(true, false, false));
}

#[test]
fn sticky_idle_constant_is_sane() {
    assert!(
        VOICE_F5_STICKY_MAX_IDLE_MS >= 2_000,
        "too short → typematic gaps unstick early"
    );
    assert!(
        VOICE_F5_STICKY_MAX_IDLE_MS <= 15_000,
        "too long → physical F5 blocked after voice"
    );
}

#[test]
fn suppress_path_must_not_sleep() {
    let src = include_str!("../src/bridges/xiaomi/key_mapping.rs");
    let body = src
        .split("pub fn should_suppress_voice_f5")
        .nth(1)
        .and_then(|s| s.split("pub fn ").next())
        .expect("fn body");
    assert!(!body.contains("thread::sleep"));
    assert!(!body.contains("wait_for_direct_signal"));
}

#[test]
fn end_voice_period_source_must_not_clear_sticky() {
    let src = include_str!("../src/bridges/xiaomi/key_mapping.rs");
    let body = src
        .split("pub fn end_voice_period")
        .nth(1)
        .and_then(|s| s.split("pub fn ").next())
        .expect("end_voice_period body");
    assert!(
        !body.contains("VOICE_F5_DOWN_SUPPRESSED.store(false"),
        "end_voice_period must not clear VOICE_F5_DOWN_SUPPRESSED"
    );
    let disarm = src
        .split("pub fn disarm_voice_native_suppress")
        .nth(1)
        .and_then(|s| s.split("pub fn ").next())
        .expect("disarm body");
    assert!(
        disarm.contains("VOICE_F5_DOWN_SUPPRESSED.store(false"),
        "disarm must clear sticky"
    );
}

#[test]
fn sleep_helper_not_needed_for_idle_check() {
    // 纯函数边界：age 刚好等于上限仍有效；超过才失效
    use remote_bridge_hub_lib::bridges::xiaomi::key_mapping::voice_f5_sticky_valid_for_test;
    use std::time::Instant;
    let now = Instant::now();
    let at_limit = now - Duration::from_millis(VOICE_F5_STICKY_MAX_IDLE_MS);
    assert!(voice_f5_sticky_valid_for_test(now, Some(at_limit)));
    let over = now - Duration::from_millis(VOICE_F5_STICKY_MAX_IDLE_MS + 1);
    assert!(!voice_f5_sticky_valid_for_test(now, Some(over)));
    assert!(!voice_f5_sticky_valid_for_test(now, None));
}
