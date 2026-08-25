//! 端口 / 桥接进程冲突检测与白名单结束
//!
//! 触发：PCM/HID Tap bind 失败（含 WinError 10048）、ATVV 订阅失败。
//! 仅允许结束已知遥控桥接进程，不杀当前进程与未知程序。

use parking_lot::Mutex;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

use crate::audio::pcm_router::DEFAULT_PCM_PORT;
use crate::bridges::xiaomi::hid_tap_runtime::HID_TAP_PORT;

static APP: Mutex<Option<AppHandle>> = Mutex::new(None);
static LAST_EMIT: Mutex<Option<Instant>> = Mutex::new(None);
static EMIT_GAP: Duration = Duration::from_secs(8);

/// 允许提示并结束的进程名（小写比较）
const WHITELIST: &[&str] = &[
    "xiaomiremotebridge.exe",
    "remote-bridge-hub.exe",
    "xiaomi_main.exe",
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictProcess {
    pub pid: u32,
    pub name: String,
    /// 占用原因：port:31680 / port:30684 / bridge_process
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictSnapshot {
    pub trigger: String,
    pub detail: String,
    pub processes: Vec<ConflictProcess>,
    pub pcm_port: u16,
    pub hid_tap_port: u16,
}

pub fn bind_app(app: AppHandle) {
    *APP.lock() = Some(app);
}

fn pcm_port() -> u16 {
    std::env::var("REMOTE_BRIDGE_PCM_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_PCM_PORT)
}

fn hid_tap_port() -> u16 {
    std::env::var("REMOTE_BRIDGE_XIAOMI_HID_TAP_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(HID_TAP_PORT)
}

fn is_whitelisted(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    WHITELIST.iter().any(|w| lower == *w || lower.ends_with(w))
}

fn self_pid() -> u32 {
    std::process::id()
}

fn is_our_pid(pid: u32) -> bool {
    if pid == 0 || pid == self_pid() {
        return true;
    }
    crate::audio::pcm_router::audio_router_child_pid() == Some(pid)
}

/// 扫描白名单冲突进程（端口占用 + 仅进程名）
pub fn scan_conflicts(include_idle_bridges: bool) -> Vec<ConflictProcess> {
    let mut by_pid: HashMap<u32, ConflictProcess> = HashMap::new();

    let pcm = pcm_port();
    let tap = hid_tap_port();

    for (port, proto) in [(pcm, "UDP"), (tap, "TCP")] {
        for pid in pids_holding_port(port, proto) {
            if is_our_pid(pid) {
                continue;
            }
            let name = process_name(pid).unwrap_or_else(|| format!("pid:{pid}"));
            if !is_whitelisted(&name) {
                continue;
            }
            let entry = by_pid.entry(pid).or_insert_with(|| ConflictProcess {
                pid,
                name: name.clone(),
                reasons: Vec::new(),
            });
            let reason = format!("port:{port} ({proto})");
            if !entry.reasons.iter().any(|r| r == &reason) {
                entry.reasons.push(reason);
            }
        }
    }

    if include_idle_bridges {
        for (pid, name) in list_whitelist_processes() {
            if is_our_pid(pid) {
                continue;
            }
            let entry = by_pid.entry(pid).or_insert_with(|| ConflictProcess {
                pid,
                name: name.clone(),
                reasons: Vec::new(),
            });
            if entry.reasons.is_empty() {
                entry.reasons.push("bridge_process".into());
            }
        }
    }

    let mut list: Vec<_> = by_pid.into_values().collect();
    list.sort_by_key(|p| p.pid);
    list
}

pub fn current_snapshot(trigger: &str, detail: &str, include_idle_bridges: bool) -> ConflictSnapshot {
    ConflictSnapshot {
        trigger: trigger.to_string(),
        detail: detail.to_string(),
        processes: scan_conflicts(include_idle_bridges),
        pcm_port: pcm_port(),
        hid_tap_port: hid_tap_port(),
    }
}

/// 有冲突时向 UI 发事件（节流）
pub fn emit_if_conflicts(trigger: &str, detail: &str, include_idle_bridges: bool) {
    let snap = current_snapshot(trigger, detail, include_idle_bridges);
    if snap.processes.is_empty() {
        return;
    }
    {
        let mut last = LAST_EMIT.lock();
        if let Some(t) = *last {
            if t.elapsed() < EMIT_GAP {
                return;
            }
        }
        *last = Some(Instant::now());
    }
    emit_snapshot(&snap);
}

/// 用户主动修复：立即弹出冲突框（不节流）
pub fn emit_conflicts_now(trigger: &str, detail: &str, include_idle_bridges: bool) -> ConflictSnapshot {
    let snap = current_snapshot(trigger, detail, include_idle_bridges);
    if !snap.processes.is_empty() {
        *LAST_EMIT.lock() = Some(Instant::now());
        emit_snapshot(&snap);
    }
    snap
}

fn emit_snapshot(snap: &ConflictSnapshot) {
    let Some(app) = APP.lock().clone() else {
        log::warn!(
            "conflict_guard: no app handle; trigger={} procs={}",
            snap.trigger,
            snap.processes.len()
        );
        return;
    };
    log::warn!(
        "XIAOMI CONFLICT trigger={} procs={} detail={}",
        snap.trigger,
        snap.processes.len(),
        snap.detail
    );
    let _ = app.emit("xiaomi-conflict", snap);
}

/// 结束白名单 PID；返回成功结束的 pid 列表
pub fn kill_whitelisted(pids: &[u32]) -> Result<Vec<u32>, String> {
    let mut killed = Vec::new();
    for &pid in pids {
        if is_our_pid(pid) {
            continue;
        }
        let name = process_name(pid).unwrap_or_default();
        if !is_whitelisted(&name) {
            return Err(format!("拒绝结束非白名单进程 {name} (pid={pid})"));
        }
        terminate_pid(pid)?;
        killed.push(pid);
        log::info!("XIAOMI CONFLICT killed pid={pid} name={name}");
    }
    Ok(killed)
}

/// 清理冲突后自动重试：语音路由 + 标记需用户重连时由前端调 restart
pub fn retry_after_clear() -> Result<String, String> {
    crate::audio::pcm_router::stop_audio_router_process();
    std::thread::sleep(Duration::from_millis(300));
    crate::audio::pcm_router::spawn_audio_router_process()?;
    // 给子进程一点时间 bind
    for _ in 0..20 {
        if crate::audio::pcm_router::audio_router_ready() {
            crate::bridges::xiaomi::voice_pcm::warmup_async();
            return Ok("语音路由已重新启动".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if crate::audio::pcm_router::audio_router_process_alive() {
        crate::bridges::xiaomi::voice_pcm::warmup_async();
        Ok("已尝试重启语音路由（仍在启动中）".into())
    } else {
        Err("重启语音路由失败：端口可能仍被占用".into())
    }
}

fn pids_holding_port(port: u16, proto: &str) -> HashSet<u32> {
    let mut out = HashSet::new();
    #[cfg(target_os = "windows")]
    {
        let proto_arg = if proto.eq_ignore_ascii_case("UDP") {
            "udp"
        } else {
            "tcp"
        };
        let Ok(output) = ({
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            // ponytail: 隐藏控制台，避免冲突扫描闪黑框
            std::process::Command::new("netstat")
                .creation_flags(CREATE_NO_WINDOW)
                .args(["-ano", "-p", proto_arg])
                .output()
        }) else {
            return out;
        };
        let text = String::from_utf8_lossy(&output.stdout);
        let port_s = port.to_string();
        for line in text.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 4 {
                continue;
            }
            // TCP: Proto Local Foreign State PID | UDP: Proto Local Foreign PID
            let local = parts[1];
            let local_port = local.rsplit_once(':').map(|(_, p)| p);
            if local_port != Some(port_s.as_str()) {
                continue;
            }
            let pid_str = parts.last().copied().unwrap_or("");
            if let Ok(pid) = pid_str.parse::<u32>() {
                out.insert(pid);
            }
        }
    }
    let _ = (port, proto);
    out
}

fn list_whitelist_processes() -> Vec<(u32, String)> {
    let mut out = Vec::new();
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        };

        unsafe {
            let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
                return out;
            };
            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            if Process32FirstW(snap, &mut entry).is_ok() {
                loop {
                    let name = String::from_utf16_lossy(
                        &entry.szExeFile
                            [..entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(0)],
                    );
                    if is_whitelisted(&name) {
                        out.push((entry.th32ProcessID, name));
                    }
                    if Process32NextW(snap, &mut entry).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snap);
        }
    }
    out
}

fn process_name(pid: u32) -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        };
        unsafe {
            let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
                return None;
            };
            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            let mut found = None;
            if Process32FirstW(snap, &mut entry).is_ok() {
                loop {
                    if entry.th32ProcessID == pid {
                        found = Some(String::from_utf16_lossy(
                            &entry.szExeFile
                                [..entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(0)],
                        ));
                        break;
                    }
                    if Process32NextW(snap, &mut entry).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snap);
            found
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = pid;
        None
    }
}

fn terminate_pid(pid: u32) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{
            OpenProcess, TerminateProcess, PROCESS_TERMINATE,
        };
        unsafe {
            let handle = OpenProcess(PROCESS_TERMINATE, false, pid)
                .map_err(|e| format!("OpenProcess({pid}): {e}"))?;
            let r = TerminateProcess(handle, 1);
            let _ = CloseHandle(handle);
            r.map_err(|e| format!("TerminateProcess({pid}): {e}"))?;
        }
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = pid;
        Err("仅 Windows 支持结束进程".into())
    }
}

/// 启动后检测语音路由是否因端口冲突起不来
pub fn check_audio_router_after_spawn(app: &AppHandle) {
    bind_app(app.clone());
    let app2 = app.clone();
    std::thread::Builder::new()
        .name("xiaomi-conflict-audio".into())
        .spawn(move || {
            std::thread::sleep(Duration::from_secs(2));
            if crate::audio::pcm_router::audio_router_ready() {
                return;
            }
            // 子进程可能因 10048 立刻退出
            if !crate::audio::pcm_router::audio_router_process_alive()
                || !crate::audio::pcm_router::audio_router_ready()
            {
                // 试探：本机能否独占绑定 PCM 端口
                let port = pcm_port();
                let bind_fail = std::net::UdpSocket::bind(("127.0.0.1", port)).is_err();
                if bind_fail
                    || !crate::audio::pcm_router::audio_router_ready()
                {
                    emit_if_conflicts(
                        "pcm_port",
                        &format!(
                            "语音路由端口 {port} 可能被占用（WinError 10048）或路由未就绪"
                        ),
                        true,
                    );
                    let _ = app2;
                }
            }
        })
        .ok();
}

static DEBOUNCE_ATVV: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();

pub fn notify_atvv_failed(detail: &str) {
    let slot = DEBOUNCE_ATVV.get_or_init(|| Mutex::new(None));
    {
        let mut g = slot.lock();
        if let Some(t) = *g {
            if t.elapsed() < EMIT_GAP {
                return;
            }
        }
        *g = Some(Instant::now());
    }
    emit_if_conflicts("atvv", detail, true);
}

pub fn notify_hid_tap_bind_failed(port: u16, err: &str) {
    emit_if_conflicts(
        "hid_tap_port",
        &format!("HID Tap 端口 {port} 绑定失败: {err}"),
        true,
    );
}
