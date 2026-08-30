//! Step 3 — bump-to-front 真落地（docs/VOICE_F5_LONGTERM_PLAN.md）
//!
//! 契约：
//! - 请求 generation 单调递增
//! - 在钩子线程上 wait = 自死锁，立即返回
//! - 仅当钩子线程 mark_handled 才算 Settled；超时为 TimedOut
//! - special_keys 的 settle 必须走 hook_bump，禁止盲目 sleep 当成功
//!
//! 运行: cargo test --test hook_bump_to_front -- --nocapture

use remote_bridge_hub_lib::bridges::xiaomi::hook_bump::{
    mark_handled, next_generation, reset_for_test, wait_for, BumpOutcome,
};
use std::thread;
use std::time::Duration;

#[test]
fn request_generation_is_monotonic() {
    reset_for_test();
    let a = next_generation();
    let b = next_generation();
    let c = next_generation();
    assert!(a < b && b < c);
}

#[test]
fn waiting_on_the_hook_thread_is_detected_as_self_deadlock() {
    reset_for_test();
    let gen = next_generation();
    let outcome = wait_for(gen, /*current*/ 42, /*hook*/ 42, 50);
    assert_eq!(outcome, BumpOutcome::SelfDeadlock);
}

#[test]
fn settled_when_hook_thread_marks_handled() {
    reset_for_test();
    let gen = next_generation();
    let g = gen;
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(10));
        mark_handled();
    });
    let outcome = wait_for(g, 1, 2, 200);
    assert_eq!(outcome, BumpOutcome::Settled);
}

#[test]
fn times_out_when_never_handled() {
    reset_for_test();
    let gen = next_generation();
    let outcome = wait_for(gen, 1, 2, 40);
    assert_eq!(outcome, BumpOutcome::TimedOut);
}

#[test]
fn stale_mark_does_not_settle_newer_generation() {
    reset_for_test();
    let old = next_generation();
    mark_handled(); // 标记旧代
    let newer = next_generation();
    assert!(newer > old);
    let outcome = wait_for(newer, 1, 2, 30);
    assert_eq!(outcome, BumpOutcome::TimedOut);
}

#[test]
fn no_hook_thread_returns_dedicated_outcome() {
    reset_for_test();
    let gen = next_generation();
    assert_eq!(wait_for(gen, 1, 0, 20), BumpOutcome::NoHookThread);
}

#[test]
fn special_keys_settle_uses_hook_bump() {
    let src = include_str!("../src/bridges/xiaomi/special_keys.rs");
    let settle = src
        .split("pub fn bump_hook_to_front_and_settle")
        .nth(1)
        .and_then(|s| s.split("pub fn is_hook_running").next())
        .expect("settle fn");
    assert!(
        settle.contains("hook_bump::wait_for") || settle.contains("wait_for("),
        "settle must wait on hook_bump generation, not blind sleep success"
    );
    assert!(
        settle.contains("SelfDeadlock") || settle.contains("GetCurrentThreadId"),
        "settle must detect hook-thread self-deadlock"
    );
}

#[test]
fn bump_handler_marks_handled() {
    let src = include_str!("../src/bridges/xiaomi/special_keys.rs");
    let body = src
        .split("msg.message == WM_BUMP_HOOK_FRONT")
        .nth(1)
        .and_then(|s| s.split("TranslateMessage").next())
        .expect("bump handler");
    assert!(
        body.contains("mark_handled") || body.contains("hook_bump::mark_handled"),
        "WM_BUMP handler must mark_handled so waiters can settle"
    );
}

#[test]
fn bump_still_overlaps_install() {
    let src = include_str!("../src/bridges/xiaomi/special_keys.rs");
    let body = src
        .split("msg.message == WM_BUMP_HOOK_FRONT")
        .nth(1)
        .and_then(|s| s.split("TranslateMessage").next())
        .expect("bump handler");
    let set_pos = body.find("SetWindowsHookExW").expect("Set");
    let unhook_pos = body.find("UnhookWindowsHookEx").expect("Unhook");
    assert!(set_pos < unhook_pos);
}

#[test]
fn hook_bump_module_is_pure_logic() {
    let src = include_str!("../src/bridges/xiaomi/hook_bump.rs");
    for forbid in ["windows::", "tauri::", "user32"] {
        assert!(!src.contains(forbid), "hook_bump must stay free of {forbid}");
    }
}

#[test]
fn handle_voice_still_bumps_before_inject() {
    let src = include_str!("../src/bridges/xiaomi/key_mapping.rs");
    let handle = src
        .split("fn handle_voice")
        .nth(1)
        .and_then(|s| s.split("\nfn ").next())
        .expect("handle_voice");
    assert!(handle.contains("bump_hook_to_front"));
}
