//! 对齐 Python `XiaomiTvActionGate`：连接后短暂屏蔽 TV 键，防误触

use parking_lot::Mutex;
use std::time::{Duration, Instant};

struct GateState {
    ready_at: Option<Instant>,
    connecting: bool,
}

static GATE: Mutex<Option<GateState>> = Mutex::new(None);

fn with_gate<R>(f: impl FnOnce(&mut GateState) -> R) -> R {
    let mut g = GATE.lock();
    if g.is_none() {
        *g = Some(GateState {
            ready_at: None,
            connecting: false,
        });
    }
    f(g.as_mut().unwrap())
}

/// 开始连接 / 重连
pub fn mark_connecting() {
    with_gate(|s| {
        s.connecting = true;
        s.ready_at = None;
    });
    log::info!("XIAOMI TV GATE connecting");
}

/// ATVV/会话就绪后开启倒计时（默认 2s，对齐 Python tv_action_ready_delay）
pub fn mark_ready(delay: Duration) {
    with_gate(|s| {
        s.connecting = false;
        s.ready_at = Some(Instant::now() + delay);
    });
    log::info!("XIAOMI TV GATE ready_in={delay:?}");
}

pub fn reset() {
    with_gate(|s| {
        s.connecting = false;
        s.ready_at = None;
    });
}

/// TV 动作是否允许执行
pub fn is_ready() -> bool {
    with_gate(|s| {
        if s.connecting {
            return false;
        }
        match s.ready_at {
            None => true,
            Some(at) => Instant::now() >= at,
        }
    })
}
