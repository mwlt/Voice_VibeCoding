//! 非阻塞 F5 关联吞（docs/VOICE_F5_CORRELATE_PLAN.md）
//!
//! 运行: cargo test --test voice_f5_correlate -- --nocapture --test-threads=1

use remote_bridge_hub_lib::bridges::xiaomi::key_mapping::{
    begin_voice_period, disarm_voice_native_suppress, end_voice_period, mark_direct_signal,
    note_passthrough_f5_down, set_input_session_active, should_suppress_voice_f5,
    voice_f5_down_suppressed, voice_f5_reset_for_test, VOICE_F5_CORRELATE_MS,
};
use std::thread;
use std::time::Duration;

fn reset() {
    voice_f5_reset_for_test();
    end_voice_period("test");
    disarm_voice_native_suppress();
    set_input_session_active(false);
}

#[test]
fn mic_mark_then_f5_down_is_suppressed_without_voice_period() {
    reset();
    // 未 begin_voice_period：仅靠 mic 关联/arm
    mark_direct_signal("mic");
    assert!(
        should_suppress_voice_f5(true, false, false),
        "after mic mark, F5 DOWN must suppress (correlate/arm, no sleep)"
    );
    assert!(voice_f5_down_suppressed());
    assert!(
        should_suppress_voice_f5(false, true, false),
        "paired KEYUP must suppress when sticky (Python parity)"
    );
    disarm_voice_native_suppress();
}

#[test]
fn no_mic_no_period_physical_f5_passes() {
    reset();
    assert!(
        !should_suppress_voice_f5(true, false, false),
        "physical F5 must pass without mic/period/arm"
    );
    assert!(!voice_f5_down_suppressed());
}

#[test]
fn f5_passthrough_then_late_mic_sets_sticky_for_typematic() {
    reset();
    // F5 先到且未关联 → 放行，但记下 passthrough 时刻
    assert!(!should_suppress_voice_f5(true, false, false));
    note_passthrough_f5_down();
    assert!(!voice_f5_down_suppressed());
    // mic 在关联窗内到达 → 补 sticky（堵后续 typematic；首帧可能已漏）
    mark_direct_signal("mic");
    assert!(
        voice_f5_down_suppressed(),
        "late mic within correlate window must arm sticky after passthrough F5"
    );
    assert!(
        should_suppress_voice_f5(true, false, false),
        "typematic F5 DOWN after late correlate must be swallowed"
    );
    disarm_voice_native_suppress();
}

#[test]
fn late_mic_outside_correlate_window_does_not_sticky_from_stale_passthrough() {
    reset();
    note_passthrough_f5_down();
    thread::sleep(Duration::from_millis(VOICE_F5_CORRELATE_MS + 40));
    mark_direct_signal("mic");
    // arm 仍会打开，但不应因「过期 passthrough」单独置 sticky
    // 若仅 arm：第一次 DOWN 才置 sticky；此处断言 mark 本身未因 stale passthrough 置 sticky
    assert!(
        !voice_f5_down_suppressed(),
        "stale passthrough must not create sticky on late mic"
    );
    // arm 仍有效：随后 DOWN 应吞（与既有 arm 语义一致）
    assert!(should_suppress_voice_f5(true, false, false));
    disarm_voice_native_suppress();
}

#[test]
fn suppress_path_uses_bounded_mic_wait_helper() {
    let src = include_str!("../src/bridges/xiaomi/key_mapping.rs");
    assert!(
        src.contains("wait_for_mic_correlate"),
        "Python-parity bounded mic wait must exist"
    );
    assert!(
        src.contains("VOICE_F5_CORRELATE_WAIT_MS"),
        "correlate wait constant must be documented"
    );
    let body = src
        .split("pub fn should_suppress_voice_f5")
        .nth(1)
        .and_then(|s| s.split("pub fn ").next())
        .expect("should_suppress_voice_f5 body");
    assert!(
        body.contains("wait_for_mic_correlate"),
        "should_suppress_voice_f5 must call bounded wait when session online"
    );
}

#[test]
fn f5_down_waits_for_late_mic_mark_within_python_window() {
    reset();
    set_input_session_active(true);
    let handle = std::thread::spawn(|| {
        std::thread::sleep(Duration::from_millis(25));
        mark_direct_signal("mic");
    });
    assert!(
        should_suppress_voice_f5(true, false, false),
        "F5-first with mic arriving within 80ms must suppress (Python parity)"
    );
    handle.join().unwrap();
    disarm_voice_native_suppress();
    set_input_session_active(false);
}

#[test]
fn post_voice_tail_suppresses_typematic_after_period_end_and_keyup() {
    reset();
    set_input_session_active(true);
    begin_voice_period();
    assert!(should_suppress_voice_f5(true, false, false));
    end_voice_period("remote_up");
    assert!(
        should_suppress_voice_f5(false, true, false),
        "paired KEYUP must suppress while sticky after period end (Python parity)"
    );
    assert!(
        !voice_f5_down_suppressed(),
        "KEYUP clears sticky; tail catches subsequent DOWN"
    );
    assert!(
        should_suppress_voice_f5(true, false, false),
        "typematic F5 within post-tail must suppress"
    );
    disarm_voice_native_suppress();
    set_input_session_active(false);
}
