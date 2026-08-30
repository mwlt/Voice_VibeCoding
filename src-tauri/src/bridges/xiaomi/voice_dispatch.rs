//! 语音键任务派发：**无 GUI / 无 Win32 依赖**的队列层
//!
//! `WH_KEYBOARD_LL` 回调只投递，重活由工作线程执行，避免回调超时被系统静默卸钩。

use parking_lot::Mutex;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

pub type VoiceSink = Arc<dyn Fn(bool) + Send + Sync>;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum VoiceTask {
    Down,
    Up,
}

impl VoiceTask {
    fn bit(self) -> u8 {
        match self {
            VoiceTask::Down => 1,
            VoiceTask::Up => 2,
        }
    }
    fn as_down(self) -> bool {
        matches!(self, VoiceTask::Down)
    }
    fn name(self) -> &'static str {
        match self {
            VoiceTask::Down => "down",
            VoiceTask::Up => "up",
        }
    }
}

static PENDING: AtomicU8 = AtomicU8::new(0);
static QUEUED: AtomicUsize = AtomicUsize::new(0);
static TX: Mutex<Option<Sender<VoiceTask>>> = Mutex::new(None);
static SINK: Mutex<Option<VoiceSink>> = Mutex::new(None);
const ORDER_LOG_MAX: usize = 256;
static ORDER: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

pub fn set_sink(sink: impl Fn(bool) + Send + Sync + 'static) {
    *SINK.lock() = Some(Arc::new(sink));
}

pub fn clear_sink() {
    *SINK.lock() = None;
}

/// LL 钩子回调专用：瞬时返回。`false` = 同相位已在队列/处理中被合并。
pub fn submit_firmware_voice_key(down: bool) -> bool {
    let task = if down {
        VoiceTask::Down
    } else {
        VoiceTask::Up
    };
    let bit = task.bit();
    if PENDING.fetch_or(bit, Ordering::AcqRel) & bit != 0 {
        return false;
    }
    let Some(tx) = sender() else {
        PENDING.fetch_and(!bit, Ordering::AcqRel);
        return false;
    };
    if tx.send(task).is_err() {
        PENDING.fetch_and(!bit, Ordering::AcqRel);
        return false;
    }
    QUEUED.fetch_add(1, Ordering::AcqRel);
    true
}

pub fn queued_depth() -> usize {
    QUEUED.load(Ordering::Acquire)
}

pub fn take_order_for_test() -> Vec<&'static str> {
    std::mem::take(&mut *ORDER.lock())
}

pub fn drain_order_for_test() -> Vec<&'static str> {
    take_order_for_test()
}

pub fn reset_for_test() {
    clear_sink();
    PENDING.store(0, Ordering::Release);
    QUEUED.store(0, Ordering::Release);
    ORDER.lock().clear();
    // 保留 worker 线程与 channel，避免测例间反复 spawn
}

fn sender() -> Option<Sender<VoiceTask>> {
    let mut g = TX.lock();
    if let Some(tx) = g.as_ref() {
        return Some(tx.clone());
    }
    let (tx, rx) = channel::<VoiceTask>();
    let spawned = std::thread::Builder::new()
        .name("xiaomi-voice-worker".into())
        .spawn(move || worker_loop(rx));
    if spawned.is_err() {
        log::error!("XIAOMI VOICE worker thread spawn failed");
        return None;
    }
    *g = Some(tx.clone());
    Some(tx)
}

fn worker_loop(rx: Receiver<VoiceTask>) {
    log::info!("XIAOMI VOICE worker ready");
    while let Ok(task) = rx.recv() {
        dispatch(task);
        PENDING.fetch_and(!task.bit(), Ordering::AcqRel);
        let _ = QUEUED.fetch_update(Ordering::AcqRel, Ordering::Acquire, |v| {
            Some(v.saturating_sub(1))
        });
    }
    log::warn!("XIAOMI VOICE worker exited");
}

fn dispatch(task: VoiceTask) {
    {
        let mut order = ORDER.lock();
        if order.len() >= ORDER_LOG_MAX {
            order.remove(0);
        }
        order.push(task.name());
    }
    let sink = SINK.lock().clone();
    if let Some(sink) = sink {
        sink(task.as_down());
    }
}
