//! HID Tap 健康 / UAC 短退避 —— 纯逻辑
//!
//! 故意把上限压在 60s：长退避会让「源头清除」长期缺席，漏 F5 比狂弹 UAC 更糟。

use std::time::{Duration, Instant};

/// UAC 被拒后的基础退避（短）。
pub const UAC_BACKOFF_BASE: Duration = Duration::from_secs(8);
/// 退避上限（≤60s）。
pub const UAC_BACKOFF_MAX: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TapPhase {
    Idle,
    Injecting,
    Attached,
    Declined,
    Failed,
}

static PHASE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
static DECLINED_STREAK: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn phase_code(p: TapPhase) -> u8 {
    match p {
        TapPhase::Idle => 0,
        TapPhase::Injecting => 1,
        TapPhase::Attached => 2,
        TapPhase::Declined => 3,
        TapPhase::Failed => 4,
    }
}

fn phase_from(c: u8) -> TapPhase {
    match c {
        1 => TapPhase::Injecting,
        2 => TapPhase::Attached,
        3 => TapPhase::Declined,
        4 => TapPhase::Failed,
        _ => TapPhase::Idle,
    }
}

pub fn set_phase(p: TapPhase) {
    PHASE.store(phase_code(p), std::sync::atomic::Ordering::Release);
    if p == TapPhase::Declined {
        DECLINED_STREAK.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    }
    if p == TapPhase::Attached {
        DECLINED_STREAK.store(0, std::sync::atomic::Ordering::Release);
    }
}

pub fn phase() -> TapPhase {
    phase_from(PHASE.load(std::sync::atomic::Ordering::Acquire))
}

pub fn declined_streak() -> u32 {
    DECLINED_STREAK.load(std::sync::atomic::Ordering::Acquire)
}

pub fn reset_declined_streak() {
    DECLINED_STREAK.store(0, std::sync::atomic::Ordering::Release);
}

/// `streak` = 已连续被拒次数（0 = 第一次被拒）。
pub fn uac_backoff(streak: u32) -> Duration {
    let secs = UAC_BACKOFF_BASE
        .as_secs()
        .saturating_mul(1u64 << streak.min(3))
        .min(UAC_BACKOFF_MAX.as_secs());
    Duration::from_secs(secs)
}

pub fn should_retry_now(last_attempt: Option<Instant>, now: Instant, backoff: Duration) -> bool {
    match last_attempt {
        None => true,
        Some(t) => now.duration_since(t) >= backoff,
    }
}

pub fn reset_for_test() {
    PHASE.store(0, std::sync::atomic::Ordering::Release);
    DECLINED_STREAK.store(0, std::sync::atomic::Ordering::Release);
}
