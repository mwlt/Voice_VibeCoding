//! 注入结果回传 —— 纯逻辑
//!
//! 把「子进程启动了」和「注入成功/失败/拒 UAC」分开。

use parking_lot::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InjectResult {
    Ok,
    Err(i32),
    Declined,
}

static TARGET_PID: AtomicU32 = AtomicU32::new(0);
static RESULT: Mutex<Option<(u32, InjectResult)>> = Mutex::new(None);

pub fn begin_watch(pid: u32) {
    TARGET_PID.store(pid, Ordering::Release);
    *RESULT.lock() = None;
}

pub fn resolve(pid: u32, result: InjectResult) {
    let expected = TARGET_PID.load(Ordering::Acquire);
    if expected != 0 && pid != expected {
        // 旧 pid 结果丢弃
        return;
    }
    let mut g = RESULT.lock();
    if g.is_some() {
        return; // only first wins
    }
    *g = Some((pid, result));
}

pub fn take() -> Option<(u32, InjectResult)> {
    RESULT.lock().take()
}

pub fn peek(pid: u32) -> Option<InjectResult> {
    RESULT.lock().as_ref().and_then(|(p, r)| {
        if *p == pid {
            Some(r.clone())
        } else {
            None
        }
    })
}

pub fn label(r: &InjectResult) -> &'static str {
    match r {
        InjectResult::Ok => "注入成功",
        InjectResult::Declined => "UAC被拒",
        InjectResult::Err(_) => "注入失败",
    }
}

pub fn reset_for_test() {
    TARGET_PID.store(0, Ordering::Release);
    *RESULT.lock() = None;
}
