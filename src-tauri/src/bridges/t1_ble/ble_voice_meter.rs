//! T1 BLE 语音电平（独立副本，不调用 `xiaomi::voice_meter`）

use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
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

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct T1BleVoiceMeterSnapshot {
    pub ble_state: BleMeterState,
    pub ble_level: f32,
    pub waveform: Vec<f32>,
    pub cable_active: bool,
    pub cable_level: f32,
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
}

impl MeterInner {
    fn snapshot(&self, now: Instant, atvv_ok: bool) -> T1BleVoiceMeterSnapshot {
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
        T1BleVoiceMeterSnapshot {
            ble_state,
            ble_level,
            waveform,
            cable_active,
            cable_level,
            atvv_ok,
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
});

static TICKER_STARTED: AtomicBool = AtomicBool::new(false);

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
        .name("t1-ble-voice-meter".into())
        .spawn(|| loop {
            std::thread::sleep(Duration::from_millis(100));
            emit_if_needed(false);
        })
        .ok();
}

fn rms_level(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|&s| (s as f64) * (s as f64)).sum();
    let rms = (sum / samples.len() as f64).sqrt();
    (rms / 32768.0).clamp(0.0, 1.0) as f32
}

fn fill_waveform(wave: &mut [f32; WAVE_BINS], samples: &[i16]) {
    if samples.is_empty() {
        return;
    }
    let chunk = (samples.len() / WAVE_BINS).max(1);
    for i in 0..WAVE_BINS {
        let start = i * chunk;
        if start >= samples.len() {
            wave[i] = 0.0;
            continue;
        }
        let end = (start + chunk).min(samples.len());
        wave[i] = rms_level(&samples[start..end]);
    }
}

pub fn set_session(on: bool) {
    if let Ok(mut g) = METER.lock() {
        g.session = on;
        if !on {
            g.last_pcm = None;
            g.ble_level = 0.0;
            g.waveform = [0.0; WAVE_BINS];
        }
    }
    emit_if_needed(true);
}

/// ATVV 麦会话是否已开（AUDIO_START 后到 AUDIO_STOP）。
/// 会话中再点语音键不得 CLEAR/重注映射，否则 CABLE 会一直播静音。
pub fn is_session_active() -> bool {
    METER.lock().map(|g| g.session).unwrap_or(false)
}

/// 语音点按路径：仅在新会话开始时 CLEAR；会话中途 CLEAR 会清空正在播的 PCM。
pub fn should_clear_pcm_on_voice_edge(session_active: bool) -> bool {
    !session_active
}

pub fn on_pcm(samples: &[i16], udp_ok: bool) {
    let level = rms_level(samples);
    if let Ok(mut g) = METER.lock() {
        let now = Instant::now();
        g.last_pcm = Some(now);
        g.ble_level = level;
        fill_waveform(&mut g.waveform, samples);
        if udp_ok {
            g.last_udp = Some(now);
            g.cable_level = level;
        }
    }
    emit_if_needed(false);
}

pub fn on_udp_sent(level: f32) {
    if let Ok(mut g) = METER.lock() {
        g.last_udp = Some(Instant::now());
        g.cable_level = level.clamp(0.0, 1.0);
    }
}

pub fn reset() {
    if let Ok(mut g) = METER.lock() {
        g.session = false;
        g.last_pcm = None;
        g.last_udp = None;
        g.ble_level = 0.0;
        g.cable_level = 0.0;
        g.waveform = [0.0; WAVE_BINS];
    }
    emit_if_needed(true);
}

pub fn current_snapshot() -> T1BleVoiceMeterSnapshot {
    let atvv = crate::bridges::t1::ble_host::atvv_ok();
    METER
        .lock()
        .map(|g| g.snapshot(Instant::now(), atvv))
        .unwrap_or(T1BleVoiceMeterSnapshot {
            ble_state: BleMeterState::Idle,
            ble_level: 0.0,
            waveform: vec![0.0; WAVE_BINS],
            cable_active: false,
            cable_level: 0.0,
            atvv_ok: atvv,
        })
}

fn emit_if_needed(force: bool) {
    let Ok(mut g) = METER.lock() else {
        return;
    };
    let now = Instant::now();
    if !force {
        if let Some(last) = g.last_emit {
            if now.duration_since(last) < EMIT_MIN_INTERVAL {
                return;
            }
        }
    }
    g.last_emit = Some(now);
    let snap = g.snapshot(now, crate::bridges::t1::ble_host::atvv_ok());
    if let Some(app) = g.app.clone() {
        drop(g);
        let _ = app.emit("t1-ble-voice-meter", snap);
    }
}

/// 纯函数：由标志拼快照（单测）
pub fn build_snapshot_for_test(
    session: bool,
    pcm_recent: bool,
    udp_recent: bool,
    ble_level: f32,
    cable_level: f32,
    atvv_ok: bool,
) -> T1BleVoiceMeterSnapshot {
    let ble_state = if pcm_recent {
        BleMeterState::Receiving
    } else if session {
        BleMeterState::Session
    } else {
        BleMeterState::Idle
    };
    T1BleVoiceMeterSnapshot {
        ble_state,
        ble_level: if pcm_recent { ble_level } else { 0.0 },
        waveform: if pcm_recent {
            vec![ble_level; WAVE_BINS]
        } else {
            vec![0.0; WAVE_BINS]
        },
        cable_active: udp_recent,
        cable_level: if udp_recent { cable_level } else { 0.0 },
        atvv_ok,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_when_no_session() {
        let s = build_snapshot_for_test(false, false, false, 0.5, 0.5, false);
        assert_eq!(s.ble_state, BleMeterState::Idle);
        assert_eq!(s.ble_level, 0.0);
        assert!(!s.cable_active);
    }

    #[test]
    fn session_without_pcm() {
        let s = build_snapshot_for_test(true, false, false, 0.8, 0.0, true);
        assert_eq!(s.ble_state, BleMeterState::Session);
        assert!(s.atvv_ok);
        assert_eq!(s.ble_level, 0.0);
    }

    #[test]
    fn receiving_and_cable() {
        let s = build_snapshot_for_test(true, true, true, 0.4, 0.3, true);
        assert_eq!(s.ble_state, BleMeterState::Receiving);
        assert_eq!(s.ble_level, 0.4);
        assert!(s.cable_active);
        assert_eq!(s.cable_level, 0.3);
    }
}
