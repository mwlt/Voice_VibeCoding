//! 语音电平 / 波形：BLE 解码 PCM + UDP 输送活动（供 UI 轻量指示，不采声卡）

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

pub const WAVE_BINS: usize = 28;
const EMIT_MIN_INTERVAL: Duration = Duration::from_millis(50);
const RECEIVING_HOLD: Duration = Duration::from_millis(280);
const CABLE_HOLD: Duration = Duration::from_millis(320);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BleMeterState {
    Idle,
    Session,
    Receiving,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceMeterSnapshot {
    /// idle | session | receiving
    pub ble_state: BleMeterState,
    pub ble_level: f32,
    pub waveform: Vec<f32>,
    /// 虚拟声卡侧：近期是否有 UDP PCM 送出
    pub cable_active: bool,
    pub cable_level: f32,
    /// ATVV 控制/音频 GATT 是否已订阅
    pub atvv_ok: bool,
}

struct MeterInner {
    app: Option<AppHandle>,
    session: bool,
    last_pcm: Option<Instant>,
    last_udp: Option<Instant>,
    ble_level: f32,
    cable_level: f32,
    waveform: [f32; WAVE_BINS],
    last_emit: Option<Instant>,
    last_payload: Option<VoiceMeterSnapshot>,
}

impl MeterInner {
    fn snapshot(&self, now: Instant) -> VoiceMeterSnapshot {
        let receiving = self
            .last_pcm
            .map(|t| now.duration_since(t) < RECEIVING_HOLD)
            .unwrap_or(false);
        let ble_state = if receiving {
            BleMeterState::Receiving
        } else if self.session {
            BleMeterState::Session
        } else {
            BleMeterState::Idle
        };
        let cable_active = self
            .last_udp
            .map(|t| now.duration_since(t) < CABLE_HOLD)
            .unwrap_or(false);
        let ble_level = if matches!(ble_state, BleMeterState::Receiving) {
            self.ble_level
        } else {
            0.0
        };
        let waveform = if matches!(ble_state, BleMeterState::Receiving) {
            self.waveform.to_vec()
        } else {
            vec![0.0; WAVE_BINS]
        };
        let cable_level = if cable_active { self.cable_level } else { 0.0 };
        VoiceMeterSnapshot {
            ble_state,
            ble_level,
            waveform,
            cable_active,
            cable_level,
            atvv_ok: crate::bridges::xiaomi::connect::atvv_subscribed(),
        }
    }
}

static METER: Mutex<MeterInner> = Mutex::new(MeterInner {
    app: None,
    session: false,
    last_pcm: None,
    last_udp: None,
    ble_level: 0.0,
    cable_level: 0.0,
    waveform: [0.0; WAVE_BINS],
    last_emit: None,
    last_payload: None,
});

static TICKER_STARTED: AtomicBool = AtomicBool::new(false);

/// 在 app setup 时绑定，便于 PCM 线程发事件
pub fn bind_app(app: AppHandle) {
    if let Ok(mut g) = METER.lock() {
        g.app = Some(app);
    }
    start_ticker_once();
}

fn start_ticker_once() {
    if TICKER_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::Builder::new()
        .name("xiaomi-voice-meter".into())
        .spawn(|| loop {
            std::thread::sleep(Duration::from_millis(100));
            emit_if_needed(false);
        })
        .ok();
}

pub fn set_session(active: bool) {
    if let Ok(mut g) = METER.lock() {
        g.session = active;
        if !active {
            g.last_pcm = None;
            g.ble_level = 0.0;
            g.waveform = [0.0; WAVE_BINS];
            // 会话结束也不立刻清 UDP 痕迹，交给 hold 自然掉
        }
    }
    emit_if_needed(true);
}

/// ATVV 订阅状态变化时立刻推 UI（红字「ATVV 未连接」）
pub fn force_emit_atvv_change() {
    emit_if_needed(true);
}

/// BLE 解码 PCM 到达；`udp_ok` 表示本帧已成功 UDP 送出
pub fn on_pcm(samples: &[i16], udp_ok: bool) {
    if samples.is_empty() {
        return;
    }
    let now = Instant::now();
    let (level, bins) = analyze(samples);
    if let Ok(mut g) = METER.lock() {
        // 有 PCM 即视为会话中（含隐式开流）
        g.session = true;
        g.last_pcm = Some(now);
        g.ble_level = level;
        g.waveform = bins;
        if udp_ok {
            g.last_udp = Some(now);
            g.cable_level = level;
        }
    }
    emit_if_needed(false);
}

fn analyze(samples: &[i16]) -> (f32, [f32; WAVE_BINS]) {
    let mut peak = 0i32;
    let mut sum_sq: f64 = 0.0;
    for &s in samples {
        let a = (s as i32).abs();
        if a > peak {
            peak = a;
        }
        let f = s as f64 / 32768.0;
        sum_sq += f * f;
    }
    let rms = ((sum_sq / samples.len().max(1) as f64).sqrt()) as f32;
    let peak_n = (peak as f32 / 32768.0).min(1.0);
    let level = (rms * 1.6 + peak_n * 0.45).min(1.0);

    let mut bins = [0.0f32; WAVE_BINS];
    let chunk = (samples.len() / WAVE_BINS).max(1);
    for (i, bin) in bins.iter_mut().enumerate() {
        let start = i * chunk;
        if start >= samples.len() {
            break;
        }
        let end = (start + chunk).min(samples.len());
        let mut local = 0i32;
        for &s in &samples[start..end] {
            local = local.max((s as i32).abs());
        }
        *bin = (local as f32 / 32768.0).min(1.0);
    }
    (level, bins)
}

fn emit_if_needed(force: bool) {
    let now = Instant::now();
    let (app, snap, should) = {
        let Ok(mut g) = METER.lock() else {
            return;
        };
        let snap = g.snapshot(now);
        let changed = g
            .last_payload
            .as_ref()
            .map(|p| {
                p.ble_state != snap.ble_state
                    || p.cable_active != snap.cable_active
                    || p.atvv_ok != snap.atvv_ok
                    || (p.ble_level - snap.ble_level).abs() > 0.04
                    || (p.cable_level - snap.cable_level).abs() > 0.04
            })
            .unwrap_or(true);
        let interval_ok = g
            .last_emit
            .map(|t| now.duration_since(t) >= EMIT_MIN_INTERVAL)
            .unwrap_or(true);
        let live = matches!(snap.ble_state, BleMeterState::Receiving) || snap.cable_active;
        let should = force || (interval_ok && (changed || live));
        if !should {
            return;
        }
        g.last_emit = Some(now);
        g.last_payload = Some(snap.clone());
        (g.app.clone(), snap, should)
    };
    if !should {
        return;
    }
    if let Some(app) = app {
        let _ = app.emit("xiaomi-voice-meter", snap);
    }
}

/// 供轮询兜底（页面刚打开时）
pub fn current_snapshot() -> VoiceMeterSnapshot {
    let now = Instant::now();
    METER
        .lock()
        .map(|g| g.snapshot(now))
        .unwrap_or(VoiceMeterSnapshot {
            ble_state: BleMeterState::Idle,
            ble_level: 0.0,
            waveform: vec![0.0; WAVE_BINS],
            cable_active: false,
            cable_level: 0.0,
            atvv_ok: crate::bridges::xiaomi::connect::atvv_subscribed(),
        })
}
