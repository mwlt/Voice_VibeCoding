//! 键盘探测：记录 LL 钩子见到的真实按键（类似在线键盘测试），写入独立日志。
//! 供诊断「F5 是否泄漏 / 是否粘键 / 映射键是否真的发出」。
//!
//! 日志：`%APPDATA%/…/logs/key-probe.log`
//! LL 回调内只投递到通道，由后台线程写盘（避免拖死 WH_KEYBOARD_LL）。

use parking_lot::Mutex;
use serde::Serialize;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

static PROBE_ACTIVE: AtomicBool = AtomicBool::new(false);
static TX: Mutex<Option<SyncSender<ProbeLine>>> = Mutex::new(None);
static APP: Mutex<Option<AppHandle>> = Mutex::new(None);
static LOG_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeLine {
    pub ts_ms: u64,
    pub vk: u16,
    pub label: String,
    pub phase: String,
    pub injected: bool,
    pub our_inject: bool,
    pub decision: String,
    pub session: bool,
    pub voice_period: bool,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn default_log_path() -> PathBuf {
    if let Some(p) = crate::logging::log_path() {
        if let Some(dir) = p.parent() {
            return dir.join("key-probe.log");
        }
    }
    std::env::temp_dir().join("voice-vibecoding-key-probe.log")
}

pub fn is_active() -> bool {
    PROBE_ACTIVE.load(Ordering::Acquire)
}

pub fn log_path() -> PathBuf {
    LOG_PATH
        .lock()
        .clone()
        .unwrap_or_else(default_log_path)
}

/// 启动探测：确保 LL 钩子在跑，清空/追加日志头，启动写盘线程。
pub fn start(app: AppHandle) -> Result<String, String> {
    *APP.lock() = Some(app);
    let path = default_log_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    {
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| format!("open key-probe.log: {e}"))?;
        writeln!(
            f,
            "===== KEY PROBE START ts_ms={} =====",
            now_ms()
        )
        .map_err(|e| e.to_string())?;
        writeln!(
            f,
            "# format: ts_ms\tvk\tlabel\tphase\tinjected\tour_inject\tdecision\tsession\tvoice_period"
        )
        .ok();
    }
    *LOG_PATH.lock() = Some(path.clone());

    let mut slot = TX.lock();
    if slot.is_none() {
        let (tx, rx) = mpsc::sync_channel::<ProbeLine>(512);
        *slot = Some(tx);
        std::thread::Builder::new()
            .name("xiaomi-key-probe-writer".into())
            .spawn(move || {
                while let Ok(line) = rx.recv() {
                    let path = log_path();
                    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
                        let _ = writeln!(
                            f,
                            "{}\t0x{:02X}\t{}\t{}\tinj={}\tour={}\t{}\tsess={}\tperiod={}",
                            line.ts_ms,
                            line.vk,
                            line.label,
                            line.phase,
                            line.injected as u8,
                            line.our_inject as u8,
                            line.decision,
                            line.session as u8,
                            line.voice_period as u8
                        );
                    }
                    if let Some(app) = APP.lock().clone() {
                        let _ = app.emit("xiaomi-key-probe", &line);
                    }
                }
            })
            .map_err(|e| format!("spawn probe writer: {e}"))?;
    }
    drop(slot);

    crate::bridges::xiaomi::special_keys::ensure_hook_for_capture();
    PROBE_ACTIVE.store(true, Ordering::Release);
    log::info!("XIAOMI KEY PROBE started path={}", path.display());
    Ok(path.to_string_lossy().into_owned())
}

pub fn stop() -> Result<(), String> {
    PROBE_ACTIVE.store(false, Ordering::Release);
    let path = log_path();
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "===== KEY PROBE STOP ts_ms={} =====", now_ms());
    }
    log::info!("XIAOMI KEY PROBE stopped");
    Ok(())
}

pub fn clear_log() -> Result<String, String> {
    let path = log_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, "").map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

/// LL 钩子调用：必须极快；decision = suppressed | passthrough | injected_pass
pub fn record(
    vk: u32,
    down: bool,
    up: bool,
    injected: bool,
    our_inject: bool,
    decision: &str,
) {
    if !is_active() || (!down && !up) {
        return;
    }
    let phase = if down {
        "down"
    } else if up {
        "up"
    } else {
        return;
    };
    let line = ProbeLine {
        ts_ms: now_ms(),
        vk: vk as u16,
        label: crate::bridges::xiaomi::config::vk_code_to_name(vk as u16),
        phase: phase.into(),
        injected,
        our_inject,
        decision: decision.into(),
        session: crate::bridges::xiaomi::key_mapping::input_session_active(),
        voice_period: crate::bridges::xiaomi::key_mapping::voice_period_active(),
    };
    if let Some(tx) = TX.lock().as_ref() {
        let _ = tx.try_send(line);
    }
}

/// 分析文本（可单测）；decision 字段见日志格式。
pub fn analyze_text(tail: &str) -> ProbeAnalysis {
    let mut f5_pass_down = 0u32;
    let mut f5_supp_down = 0u32;
    let mut f5_pass_up = 0u32;
    let mut f5_supp_up = 0u32;
    let mut ctrl_down = 0u32;
    let mut win_down = 0u32;
    for line in tail.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 7 {
            continue;
        }
        let vk = parts[1];
        let phase = parts[3];
        let decision = parts[6];
        if vk.eq_ignore_ascii_case("0x74") {
            match (phase, decision) {
                ("down", "passthrough") => f5_pass_down += 1,
                ("down", "suppressed") => f5_supp_down += 1,
                ("up", "passthrough") => f5_pass_up += 1,
                ("up", "suppressed") => f5_supp_up += 1,
                _ => {}
            }
        }
        if phase == "down" && decision != "suppressed" {
            if vk.eq_ignore_ascii_case("0xA2")
                || vk.eq_ignore_ascii_case("0xA3")
                || vk.eq_ignore_ascii_case("0x11")
            {
                ctrl_down += 1;
            }
            if vk.eq_ignore_ascii_case("0x5B") || vk.eq_ignore_ascii_case("0x5C") {
                win_down += 1;
            }
        }
    }
    let stuck_suspect = f5_pass_down > 0 && f5_pass_up == 0 && f5_supp_up > 0;
    ProbeAnalysis {
        path: String::new(),
        f5_passthrough_down: f5_pass_down,
        f5_suppressed_down: f5_supp_down,
        f5_passthrough_up: f5_pass_up,
        f5_suppressed_up: f5_supp_up,
        ctrl_down_seen: ctrl_down,
        win_down_seen: win_down,
        f5_leak: f5_pass_down > 0,
        f5_stuck_suspect: stuck_suspect,
        ctrl_without_win: ctrl_down > 0 && win_down == 0,
    }
}

/// 分析最近日志：是否出现 F5 passthrough、F5 down 无匹配 up（粘键嫌疑）
pub fn analyze_recent(max_bytes: usize) -> Result<ProbeAnalysis, String> {
    let path = log_path();
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let tail = if text.len() > max_bytes {
        &text[text.len() - max_bytes..]
    } else {
        &text
    };
    let mut a = analyze_text(tail);
    a.path = path.to_string_lossy().into_owned();
    Ok(a)
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeAnalysis {
    pub path: String,
    pub f5_passthrough_down: u32,
    pub f5_suppressed_down: u32,
    pub f5_passthrough_up: u32,
    pub f5_suppressed_up: u32,
    pub ctrl_down_seen: u32,
    pub win_down_seen: u32,
    pub f5_leak: bool,
    pub f5_stuck_suspect: bool,
    pub ctrl_without_win: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analysis_detects_stuck_pattern() {
        let sample = "1\t0x74\tF5\tdown\tinj=0\tour=0\tpassthrough\tsess=1\tperiod=0\n\
             2\t0x74\tF5\tup\tinj=0\tour=0\tsuppressed\tsess=1\tperiod=1\n";
        let a = analyze_text(sample);
        assert!(a.f5_leak);
        assert!(a.f5_stuck_suspect);
    }

    #[test]
    fn analysis_ctrl_win_pair() {
        let sample = "1\t0xA2\tLCtrl\tdown\tinj=1\tour=0\tinjected_pass\tsess=1\tperiod=1\n\
             2\t0x5B\tLWin\tdown\tinj=1\tour=0\tinjected_pass\tsess=1\tperiod=1\n";
        let a = analyze_text(sample);
        assert!(!a.ctrl_without_win);
        assert_eq!(a.ctrl_down_seen, 1);
        assert_eq!(a.win_down_seen, 1);
    }
}
