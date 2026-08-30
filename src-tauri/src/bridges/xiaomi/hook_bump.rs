//! bump-to-front 落地协议 —— **纯逻辑，无系统 API / GUI 依赖**
//!
//! 旧实现：PostThreadMessage 后盲目 sleep，并以 `is_hook_armed()`（句柄非空）当成功。
//! 若在钩子线程上 wait，消息泵被堵住，WM_BUMP 永远处理不了。

use parking_lot::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BumpOutcome {
    Settled,
    TimedOut,
    SelfDeadlock,
    NoHookThread,
}

static NEXT_GEN: AtomicU64 = AtomicU64::new(1);
static HANDLED_GEN: AtomicU64 = AtomicU64::new(0);
static LAST_REQUEST: Mutex<Option<u64>> = Mutex::new(None);

pub fn next_generation() -> u64 {
    let g = NEXT_GEN.fetch_add(1, Ordering::AcqRel);
    *LAST_REQUEST.lock() = Some(g);
    g
}

pub fn mark_handled() {
    if let Some(g) = *LAST_REQUEST.lock() {
        HANDLED_GEN.store(g, Ordering::Release);
    }
}

pub fn wait_for(gen: u64, current_tid: u32, hook_tid: u32, settle_ms: u64) -> BumpOutcome {
    if hook_tid == 0 {
        return BumpOutcome::NoHookThread;
    }
    if current_tid == hook_tid {
        return BumpOutcome::SelfDeadlock;
    }
    let deadline = Instant::now() + Duration::from_millis(settle_ms.max(1));
    while Instant::now() < deadline {
        if HANDLED_GEN.load(Ordering::Acquire) >= gen {
            return BumpOutcome::Settled;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    BumpOutcome::TimedOut
}

pub fn reset_for_test() {
    NEXT_GEN.store(1, Ordering::Release);
    HANDLED_GEN.store(0, Ordering::Release);
    *LAST_REQUEST.lock() = None;
}
