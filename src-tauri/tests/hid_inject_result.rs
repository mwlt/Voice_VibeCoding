//! 注入结果契约（docs/VOICE_F5_LONGTERM_PLAN.md Step 5）

use remote_bridge_hub_lib::bridges::xiaomi::hid_inject_result::{
    begin_watch, label, peek, reset_for_test, resolve, take, InjectResult,
};

#[test]
fn exit_code_zero_means_success() {
    reset_for_test();
    begin_watch(42);
    resolve(42, InjectResult::Ok);
    assert_eq!(take(), Some((42, InjectResult::Ok)));
}

#[test]
fn declined_is_distinct_from_err() {
    reset_for_test();
    begin_watch(7);
    resolve(7, InjectResult::Declined);
    assert_eq!(label(&InjectResult::Declined), "UAC被拒");
    assert_eq!(peek(7), Some(InjectResult::Declined));
}

#[test]
fn stale_pid_result_is_discarded() {
    reset_for_test();
    begin_watch(10);
    resolve(99, InjectResult::Ok);
    assert!(take().is_none());
}

#[test]
fn only_first_result_wins() {
    reset_for_test();
    begin_watch(1);
    resolve(1, InjectResult::Ok);
    resolve(1, InjectResult::Err(3));
    assert_eq!(take(), Some((1, InjectResult::Ok)));
}

#[test]
fn labels_are_stable() {
    assert_eq!(label(&InjectResult::Ok), "注入成功");
    assert_eq!(label(&InjectResult::Err(1)), "注入失败");
}

#[test]
fn inject_result_module_is_pure() {
    let src = include_str!("../src/bridges/xiaomi/hid_inject_result.rs");
    for forbid in ["windows::", "tauri::", "user32"] {
        assert!(!src.contains(forbid));
    }
}
