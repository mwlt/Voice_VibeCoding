//! Step 5 — 短 UAC 退避 + 注入结果（docs/VOICE_F5_LONGTERM_PLAN.md）
//!
//! **不做 helper。** 退避上限 ≤60s，避免 Tap 长期缺席导致漏 F5。
//!
//! 运行: cargo test --test hid_tap_health --test hid_inject_result -- --nocapture

use remote_bridge_hub_lib::bridges::xiaomi::hid_tap_health::{
    should_retry_now, uac_backoff, UAC_BACKOFF_BASE, UAC_BACKOFF_MAX,
};
use std::time::{Duration, Instant};

#[test]
fn backoff_base_and_max_are_short() {
    assert!(
        UAC_BACKOFF_BASE >= Duration::from_secs(5) && UAC_BACKOFF_BASE <= Duration::from_secs(15),
        "base should be 5–15s, got {:?}",
        UAC_BACKOFF_BASE
    );
    assert!(
        UAC_BACKOFF_MAX <= Duration::from_secs(60),
        "max must be ≤60s so Tap recovers; got {:?}",
        UAC_BACKOFF_MAX
    );
    assert!(UAC_BACKOFF_MAX >= UAC_BACKOFF_BASE);
}

#[test]
fn backoff_is_monotonic_and_capped() {
    let mut prev = Duration::ZERO;
    for streak in 0..8 {
        let b = uac_backoff(streak);
        assert!(b >= prev || b == UAC_BACKOFF_MAX);
        assert!(b <= UAC_BACKOFF_MAX);
        prev = b;
    }
    assert_eq!(uac_backoff(100), UAC_BACKOFF_MAX);
}

#[test]
fn should_retry_respects_backoff_window() {
    let now = Instant::now();
    assert!(should_retry_now(None, now, Duration::from_secs(10)));
    assert!(!should_retry_now(
        Some(now),
        now + Duration::from_secs(1),
        Duration::from_secs(10)
    ));
    assert!(should_retry_now(
        Some(now),
        now + Duration::from_secs(10),
        Duration::from_secs(10)
    ));
}

#[test]
fn report_tap_uses_short_uac_backoff_on_decline() {
    let src = include_str!("../src/bridges/xiaomi/hid_report_tap.rs");
    assert!(
        src.contains("hid_tap_health::uac_backoff") && src.contains("InjectResult::Declined"),
        "UAC decline path must use short hid_tap_health backoff + inject result"
    );
    assert!(
        !src.contains("from_secs(300)") && !src.contains("Duration::from_secs(240)"),
        "must not use the old 300s-class long backoff"
    );
}

#[test]
fn injector_maps_uac_cancel_to_ok_false() {
    let src = include_str!("../src/bridges/xiaomi/hid_tap_injector.rs");
    assert!(
        src.contains("ShellExecuteExW") && src.contains("ERROR_CANCELLED"),
        "must use ShellExecuteEx so UAC cancel is detectable"
    );
    assert!(
        src.contains("Ok(false)"),
        "UAC decline must return Ok(false) for short-backoff path"
    );
    assert!(
        !src.contains("ShellExecuteW("),
        "old ShellExecuteW cannot distinguish UAC decline → dead Ok(false) path"
    );
}
