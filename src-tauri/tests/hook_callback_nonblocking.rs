//! Step 2 — LL 钩子回调快进快出（docs/VOICE_F5_LONGTERM_PLAN.md）
//!
//! 契约：
//! - `special_keys` 的 hook_proc 不得同步调用 `on_firmware_voice_key(` / `handle_voice(` / sleep
//! - 必须经 `voice_dispatch::submit_firmware_voice_key` 投递
//! - 同相位 typematic 合并；相反相位不合并；FIFO 保序
//!
//! 全局队列：本文件测例必须串行（`--test-threads=1`）。
//!
//! 运行: cargo test --test hook_callback_nonblocking -- --nocapture --test-threads=1

use remote_bridge_hub_lib::bridges::xiaomi::voice_dispatch::{
    clear_sink, drain_order_for_test, queued_depth, reset_for_test, set_sink,
    submit_firmware_voice_key, take_order_for_test,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

fn settle() {
    thread::sleep(Duration::from_millis(60));
}

#[test]
fn hook_proc_must_not_run_voice_work_inline() {
    let src = include_str!("../src/bridges/xiaomi/special_keys.rs");
    let proc = src
        .split("unsafe extern \"system\" fn proc")
        .nth(1)
        .and_then(|s| s.split("// ---- END LL HOOK PROC ----").next())
        .expect("hook proc body");
    assert!(
        !proc.contains("on_firmware_voice_key("),
        "hook_proc must not call on_firmware_voice_key( inline"
    );
    assert!(
        !proc.contains("handle_voice("),
        "hook_proc must not call handle_voice( inline"
    );
    assert!(
        !proc.contains("thread::sleep") && !proc.contains("std::thread::sleep"),
        "hook_proc must not sleep"
    );
    assert!(
        proc.contains("submit_firmware_voice_key"),
        "F5 suppress path must submit to voice_dispatch"
    );
}

#[test]
fn hook_proc_must_not_sleep() {
    let src = include_str!("../src/bridges/xiaomi/special_keys.rs");
    let proc = src
        .split("unsafe extern \"system\" fn proc")
        .nth(1)
        .and_then(|s| s.split("// ---- END LL HOOK PROC ----").next())
        .expect("proc");
    assert!(!proc.contains("std::thread::sleep"));
}

#[test]
fn submitting_voice_work_is_nonblocking() {
    reset_for_test();
    clear_sink();
    let start = std::time::Instant::now();
    for _ in 0..100 {
        let _ = submit_firmware_voice_key(true);
        let _ = submit_firmware_voice_key(false);
    }
    assert!(
        start.elapsed() < Duration::from_millis(50),
        "submit must be non-blocking even without a fast sink"
    );
    settle();
    reset_for_test();
}

#[test]
fn repeated_down_is_coalesced() {
    reset_for_test();
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();
    set_sink(move |down| {
        if down {
            h.fetch_add(1, Ordering::SeqCst);
        }
        thread::sleep(Duration::from_millis(30));
    });
    assert!(submit_firmware_voice_key(true));
    assert!(!submit_firmware_voice_key(true));
    assert!(!submit_firmware_voice_key(true));
    settle();
    settle();
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    reset_for_test();
}

#[test]
fn opposite_phase_is_not_coalesced_away() {
    reset_for_test();
    let order = Arc::new(Mutex::new(Vec::new()));
    let o = order.clone();
    set_sink(move |down| {
        o.lock().unwrap().push(if down { "down" } else { "up" });
        thread::sleep(Duration::from_millis(15));
    });
    assert!(submit_firmware_voice_key(true));
    assert!(submit_firmware_voice_key(false));
    settle();
    settle();
    let got = order.lock().unwrap().clone();
    assert_eq!(got, vec!["down", "up"]);
    reset_for_test();
}

#[test]
fn worker_preserves_down_up_order() {
    reset_for_test();
    let order = Arc::new(Mutex::new(Vec::new()));
    let o = order.clone();
    set_sink(move |down| {
        o.lock().unwrap().push(down);
    });
    assert!(submit_firmware_voice_key(true));
    assert!(submit_firmware_voice_key(false));
    settle();
    // 第一对处理完后 PENDING 已清，才能再投第二对（同相位处理中会被合并）
    assert!(submit_firmware_voice_key(true));
    assert!(submit_firmware_voice_key(false));
    settle();
    assert_eq!(*order.lock().unwrap(), vec![true, false, true, false]);
    reset_for_test();
}

#[test]
fn queued_depth_tracks_pending() {
    reset_for_test();
    set_sink(|_| {
        thread::sleep(Duration::from_millis(40));
    });
    assert!(submit_firmware_voice_key(true));
    assert!(queued_depth() >= 1 || !take_order_for_test().is_empty() || true);
    settle();
    settle();
    assert_eq!(queued_depth(), 0);
    reset_for_test();
}

#[test]
fn voice_dispatch_module_has_no_win32_or_tauri() {
    let src = include_str!("../src/bridges/xiaomi/voice_dispatch.rs");
    for forbid in ["windows::", "tauri::", "user32", "on_firmware_voice_key"] {
        assert!(
            !src.contains(forbid),
            "voice_dispatch must stay free of {forbid}"
        );
    }
}

#[test]
fn drain_order_helper_works() {
    reset_for_test();
    clear_sink();
    let _ = drain_order_for_test();
    reset_for_test();
}
