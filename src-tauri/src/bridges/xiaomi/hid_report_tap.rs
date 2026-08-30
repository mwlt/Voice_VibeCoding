//! 对齐 Python `hid_report_tap.py`：TCP hub + 解码 HidOverGatt IOCTL 报告
//!
//! 生产路径：监听 30684 → UAC 注入 Frida Gadget 到 RC003 的 WUDFHost →
//! 钩 NtDeviceIoControlFile(0x80018483) → 转发返回/音量等 usage。
//!
//! 生命周期：进程级单例。桥接重启不拆 hub；仅应用退出或配置禁用时 `stop_and_join`。

use crate::bridges::xiaomi::ble_bridge::XiaomiButton;
use crate::bridges::xiaomi::hid_tap_injector::launch_elevated_injector;
use crate::bridges::xiaomi::hid_tap_runtime::{
    find_rc003_hidogatt_host_pid, gadget_archive_available, hid_tap_port,
};
use crate::bridges::xiaomi::key_log::{button_label, emit_key_and_map, emit_message, KeyEmitGate};
use crate::bridges::xiaomi::special_keys;
use parking_lot::Mutex;
use serde::Deserialize;
use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

const FORWARDED: &[u16] = &[
    0x00F1, 0x0028, 0x0035, 0x004A, 0x004F, 0x0050, 0x0051, 0x0052, 0x0065, 0x0066, 0x007F,
    0x0080, 0x0081,
];

/// 附着/首包 IO 后短冷静期：只同步按键状态，不注入（避免连接抖动误触）
const INPUT_GRACE: Duration = Duration::from_millis(800);
static INPUT_GRACE_UNTIL: Mutex<Option<Instant>> = Mutex::new(None);

fn arm_input_grace() {
    *INPUT_GRACE_UNTIL.lock() = Some(Instant::now() + INPUT_GRACE);
    tap_log(&format!(
        "XIAOMI HID TAP input grace {}ms",
        INPUT_GRACE.as_millis()
    ));
}

fn in_input_grace() -> bool {
    let mut g = INPUT_GRACE_UNTIL.lock();
    match *g {
        Some(until) if Instant::now() < until => true,
        Some(_) => {
            *g = None;
            false
        }
        None => false,
    }
}

struct HidTapController {
    stop: Arc<AtomicBool>,
    gate: Arc<Mutex<Arc<KeyEmitGate>>>,
    join: Option<JoinHandle<()>>,
}

static CONTROLLER: OnceLock<Mutex<Option<HidTapController>>> = OnceLock::new();

fn controller_slot() -> &'static Mutex<Option<HidTapController>> {
    CONTROLLER.get_or_init(|| Mutex::new(None))
}

#[derive(Deserialize)]
struct HubMessage {
    kind: String,
    #[serde(default)]
    raw: String,
    #[serde(default)]
    message: String,
}

fn tap_log(msg: &str) {
    log::info!("{msg}");
    let path = std::env::temp_dir().join("remote-bridge-hid-tap.log");
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{}", msg);
    }
}

pub fn decode_rc003_ioctl_output(data: &[u8]) -> Option<&[u8]> {
    if data.len() == 9 && data.starts_with(&[0x01, 0x00, 0x00]) {
        Some(&data[3..9])
    } else {
        None
    }
}

fn payload_usages(payload: &[u8]) -> HashSet<u16> {
    let mut set = HashSet::new();
    if payload.len() != 6 {
        return set;
    }
    for chunk in payload.chunks_exact(2) {
        let usage = u16::from_le_bytes([chunk[0], chunk[1]]);
        if usage != 0 {
            set.insert(usage);
        }
    }
    set
}

fn sleep_interruptible(stop: &AtomicBool, total: Duration) {
    let step = Duration::from_millis(50);
    let deadline = Instant::now() + total;
    while Instant::now() < deadline {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        let remain = deadline.saturating_duration_since(Instant::now());
        std::thread::sleep(remain.min(step));
    }
}

fn wait_port_free(port: u16) {
    for _ in 0..50 {
        match TcpListener::bind(("127.0.0.1", port)) {
            Ok(listener) => {
                drop(listener);
                return;
            }
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    tap_log(&format!(
        "XIAOMI HID TAP port {port} still busy after stop wait"
    ));
}

/// 进程级确保 HID Tap hub 在跑；已运行则只刷新 gate，不二次 bind。
pub fn ensure_started(app: AppHandle, gate: Arc<KeyEmitGate>) -> bool {
    if !gadget_archive_available() {
        emit_message(
            &app,
            "HID Tap 不可用：缺少 Frida Gadget（返回/音量键依赖此通道）",
        );
        tap_log("XIAOMI HID TAP unavailable verified_gadget_not_installed");
        return false;
    }

    let mut slot = controller_slot().lock();
    if let Some(ctrl) = slot.as_mut() {
        *ctrl.gate.lock() = gate;
        tap_log("XIAOMI HID TAP ensure_started reuse (singleton)");
        return true;
    }

    special_keys::set_hid_tap_ready(false);

    let stop = Arc::new(AtomicBool::new(false));
    let gate_slot = Arc::new(Mutex::new(gate));
    let stop_run = Arc::clone(&stop);
    let gate_run = Arc::clone(&gate_slot);
    let app2 = app.clone();

    let join = std::thread::Builder::new()
        .name("xiaomi-hid-tap-hub".into())
        .spawn(move || run_hub(app2, gate_run, stop_run))
        .ok();

    if join.is_none() {
        tap_log("XIAOMI HID TAP hub thread spawn failed");
        return false;
    }

    *slot = Some(HidTapController {
        stop,
        gate: gate_slot,
        join,
    });

    emit_message(
        &app,
        "HID Tap 已启动（首次需允许 UAC，以捕获返回/音量键）",
    );
    tap_log("XIAOMI HID TAP hub thread spawned (singleton)");
    true
}

/// 显式停止 hub 并等待端口释放。桥接重启不要调用；应用退出 / 配置禁用时调用。
pub fn stop_and_join() {
    let ctrl = {
        let mut slot = controller_slot().lock();
        slot.take()
    };
    let Some(ctrl) = ctrl else {
        return;
    };

    tap_log("XIAOMI HID TAP stop_and_join requested");
    ctrl.stop.store(true, Ordering::SeqCst);
    special_keys::set_hid_tap_ready(false);

    if let Some(handle) = ctrl.join {
        let _ = handle.join();
    }

    wait_port_free(hid_tap_port());
    tap_log("XIAOMI HID TAP stop_and_join done");
}

pub fn is_running() -> bool {
    controller_slot().lock().is_some()
}

/// 对齐 Python `XiaomiHidReportTap._run`：bind → 注入一次 → accept 超时则续听，不反复清 injection 标记
fn run_hub(app: AppHandle, gate_slot: Arc<Mutex<Arc<KeyEmitGate>>>, stop: Arc<AtomicBool>) {
    let port = hid_tap_port();
    let active = Arc::new(Mutex::new(HashSet::<u16>::new()));
    let mut injection_attempted_pid: Option<u32> = None;
    let mut last_wait_log = Instant::now() - Duration::from_secs(60);
    let mut last_bind_fail_log = Instant::now() - Duration::from_secs(60);
    let retry = Duration::from_secs(2);
    let heartbeat_timeout = Duration::from_secs(20);

    tap_log(&format!("XIAOMI HID TAP hub loop start port={port}"));

    while !stop.load(Ordering::SeqCst) {
        let Some(pid) = find_rc003_hidogatt_host_pid() else {
            if last_wait_log.elapsed() >= Duration::from_secs(15) {
                last_wait_log = Instant::now();
                emit_message(
                    &app,
                    "HID Tap 等待 RC003 WUDFHost（请确认遥控器已配对且开机）",
                );
                tap_log("XIAOMI HID TAP waiting_for_rc003_host (HostPid registry miss — not normal when remote is connected)");
            }
            sleep_interruptible(&stop, retry);
            continue;
        };
        // 找到 HostPid 后只打一次（避免刷屏）
        if injection_attempted_pid.is_none() {
            tap_log(&format!("XIAOMI HID TAP found_host pid={pid}"));
            emit_message(&app, &format!("已找到 RC003 WUDFHost pid={pid}，准备注入…"));
        }

        // 主机 PID 变化时允许重新注入
        if injection_attempted_pid.is_some() && injection_attempted_pid != Some(pid) {
            tap_log(&format!(
                "XIAOMI HID TAP host pid changed old={injection_attempted_pid:?} new={pid}"
            ));
            injection_attempted_pid = None;
            special_keys::set_hid_tap_ready(false);
        }

        let listener = match TcpListener::bind(("127.0.0.1", port)) {
            Ok(l) => {
                let _ = l.set_nonblocking(true);
                l
            }
            Err(e) => {
                tap_log(&format!("XIAOMI HID TAP bind {port} failed: {e}"));
                // 单例下本进程不应再开第二个 hub；此处失败几乎总是其它进程占用
                if last_bind_fail_log.elapsed() >= Duration::from_secs(8) {
                    last_bind_fail_log = Instant::now();
                    emit_message(
                        &app,
                        &format!(
                            "HID Tap 端口 {port} 被其它进程占用（请关闭其它 RemoteBridge 实例）: {e}"
                        ),
                    );
                    crate::bridges::xiaomi::conflict_guard::notify_hid_tap_bind_failed(
                        port,
                        &e.to_string(),
                    );
                }
                sleep_interruptible(&stop, retry);
                continue;
            }
        };

        // 对齐 Python：每个存活 HostPid 只请求一次 UAC 注入；accept 超时不重置
        if injection_attempted_pid.is_none() {
            crate::bridges::xiaomi::hid_inject_result::begin_watch(pid);
            match launch_elevated_injector(pid) {
                Ok(true) => {
                    injection_attempted_pid = Some(pid);
                    crate::bridges::xiaomi::hid_tap_health::set_phase(
                        crate::bridges::xiaomi::hid_tap_health::TapPhase::Injecting,
                    );
                    emit_message(
                        &app,
                        &format!("已请求注入 WUDFHost pid={pid}（若弹出 UAC 请允许）"),
                    );
                    tap_log(&format!("XIAOMI HID TAP injection requested pid={pid}"));
                }
                Ok(false) => {
                    crate::bridges::xiaomi::hid_inject_result::resolve(
                        pid,
                        crate::bridges::xiaomi::hid_inject_result::InjectResult::Declined,
                    );
                    crate::bridges::xiaomi::hid_tap_health::set_phase(
                        crate::bridges::xiaomi::hid_tap_health::TapPhase::Declined,
                    );
                    let streak = crate::bridges::xiaomi::hid_tap_health::declined_streak();
                    let backoff = crate::bridges::xiaomi::hid_tap_health::uac_backoff(
                        streak.saturating_sub(1),
                    );
                    emit_message(
                        &app,
                        &format!(
                            "UAC 注入被拒绝；{} 秒后重试（源头清除暂停，仅钩子兜底）",
                            backoff.as_secs()
                        ),
                    );
                    tap_log(&format!(
                        "XIAOMI HID TAP UAC declined streak={streak} backoff={}s",
                        backoff.as_secs()
                    ));
                    drop(listener);
                    sleep_interruptible(&stop, backoff);
                    continue;
                }
                Err(e) => {
                    crate::bridges::xiaomi::hid_inject_result::resolve(
                        pid,
                        crate::bridges::xiaomi::hid_inject_result::InjectResult::Err(1),
                    );
                    crate::bridges::xiaomi::hid_tap_health::set_phase(
                        crate::bridges::xiaomi::hid_tap_health::TapPhase::Failed,
                    );
                    emit_message(&app, &format!("HID Tap 注入失败: {e}"));
                    tap_log(&format!("XIAOMI HID TAP injection error: {e}"));
                    drop(listener);
                    sleep_interruptible(&stop, retry);
                    continue;
                }
            }
        }

        // 对齐 Python：1s accept 超时后 continue，保留 injection_attempted_pid
        let mut client: Option<TcpStream> = None;
        let wait_deadline = Instant::now() + Duration::from_secs(1);
        while !stop.load(Ordering::SeqCst) && Instant::now() < wait_deadline {
            if find_rc003_hidogatt_host_pid() != Some(pid) {
                injection_attempted_pid = None;
                special_keys::set_hid_tap_ready(false);
                break;
            }
            match listener.accept() {
                Ok((stream, _)) => {
                    let _ = stream.set_read_timeout(Some(Duration::from_millis(1000)));
                    let _ = stream.set_nonblocking(false);
                    client = Some(stream);
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => {
                    tap_log(&format!("XIAOMI HID TAP accept: {e}"));
                    break;
                }
            }
        }

        let Some(mut stream) = client else {
            // 超时：不重置注入标记，下一轮继续 listen（Gadget 可能仍在连）
            drop(listener);
            continue;
        };

        emit_message(
            &app,
            &format!("HID Tap 已附着 pid={pid}，请按返回/音量键验证"),
        );
        tap_log(&format!(
            "XIAOMI HID TAP ATTACHED pid={pid} awaiting_io=true forwarded={}",
            FORWARDED.len()
        ));
        arm_input_grace();

        let mut buffer = Vec::new();
        let mut last_heartbeat = Instant::now();
        let mut io_announced = false;
        let mut tmp = [0u8; 65536];

        while !stop.load(Ordering::SeqCst) {
            if find_rc003_hidogatt_host_pid() != Some(pid) {
                tap_log(&format!("XIAOMI HID TAP HOST CHANGED old_pid={pid}"));
                injection_attempted_pid = None;
                special_keys::set_hid_tap_ready(false);
                break;
            }

            match stream.read(&mut tmp) {
                Ok(0) => {
                    tap_log("XIAOMI HID TAP client disconnected");
                    break;
                }
                Ok(n) => {
                    buffer.extend_from_slice(&tmp[..n]);
                    while let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                        let line = buffer.drain(..=pos).collect::<Vec<u8>>();
                        let line = &line[..line.len().saturating_sub(1)];
                        if let Ok(text) = std::str::from_utf8(line) {
                            if let Ok(msg) = serde_json::from_str::<HubMessage>(text) {
                                match msg.kind.as_str() {
                                    "heartbeat" | "ready" => {
                                        last_heartbeat = Instant::now();
                                        if msg.kind == "ready" && !io_announced {
                                            emit_message(
                                                &app,
                                                "HID Tap Gadget 已就绪（等待按键 IO）",
                                            );
                                        }
                                    }
                                    "cleared_f5" => {
                                        tap_log(&format!(
                                            "XIAOMI HID TAP cleared_f5 raw={}",
                                            msg.raw
                                        ));
                                    }
                                    "gatt_read" => {
                                        if let Some(data) = decode_hex(msg.raw.trim()) {
                                            if !data.is_empty() {
                                                if !io_announced {
                                                    io_announced = true;
                                                    special_keys::set_hid_tap_ready(true);
                                                    crate::bridges::xiaomi::hid_tap_health::set_phase(
                                                        crate::bridges::xiaomi::hid_tap_health::TapPhase::Attached,
                                                    );
                                                    crate::bridges::xiaomi::hid_inject_result::resolve(
                                                        pid,
                                                        crate::bridges::xiaomi::hid_inject_result::InjectResult::Ok,
                                                    );
                                                    // 尽早按绑定表武装「自定义方向/OK」吞键（Up→M 防先上移）
                                                    if let Some(mgr) =
                                                        app.try_state::<crate::config::manager::ConfigManager>()
                                                    {
                                                        if let Ok(cfg) = mgr.get_device_config("xiaomi")
                                                        {
                                                            crate::bridges::xiaomi::key_mapping::refresh_dpad_ok_custom_suppress_mask(
                                                                &cfg,
                                                            );
                                                        }
                                                    }
                                                    arm_input_grace();
                                                    emit_message(
                                                        &app,
                                                        "HID Tap 就绪：返回/音量信号可捕获",
                                                    );
                                                    tap_log(&format!(
                                                        "XIAOMI HID TAP READY pid={pid} io_verified=true"
                                                    ));
                                                }
                                                let gate = Arc::clone(&gate_slot.lock());
                                                handle_ioctl(&app, &gate, &active, &data);
                                            }
                                        }
                                    }
                                    "error" => {
                                        tap_log(&format!(
                                            "XIAOMI HID TAP hook_error={}",
                                            msg.message
                                        ));
                                        emit_message(
                                            &app,
                                            &format!("HID Tap 钩子错误: {}", msg.message),
                                        );
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(e) => {
                    tap_log(&format!("XIAOMI HID TAP recv: {e}"));
                    break;
                }
            }

            if last_heartbeat.elapsed() >= heartbeat_timeout {
                tap_log(&format!("XIAOMI HID TAP UNHEALTHY pid={pid}"));
                emit_message(&app, "HID Tap 心跳超时，重新注入");
                injection_attempted_pid = None;
                special_keys::set_hid_tap_ready(false);
                break;
            }
        }

        release_active(&active);
        drop(listener);
        if !stop.load(Ordering::SeqCst) {
            sleep_interruptible(&stop, Duration::from_millis(400));
        }
    }

    special_keys::set_hid_tap_ready(false);
    tap_log("XIAOMI HID TAP hub loop exit");
}

fn release_active(active: &Mutex<HashSet<u16>>) {
    active.lock().clear();
}

fn handle_ioctl(
    app: &AppHandle,
    gate: &KeyEmitGate,
    active: &Mutex<HashSet<u16>>,
    data: &[u8],
) {
    let Some(payload) = decode_rc003_ioctl_output(data) else {
        return;
    };
    let forwarded: HashSet<u16> = FORWARDED.iter().copied().collect();
    let next: HashSet<u16> = payload_usages(payload)
        .into_iter()
        .filter(|u| forwarded.contains(u))
        .collect();

    let mut guard = active.lock();
    if next == *guard {
        return;
    }
    // 冷静期内只同步状态，避免附着抖动当成真实按键注入
    if in_input_grace() {
        *guard = next;
        return;
    }
    let pressed: Vec<u16> = next.difference(&guard).copied().collect();
    let released: Vec<u16> = guard.difference(&next).copied().collect();
    *guard = next;
    drop(guard);

    for usage in pressed {
        let btn = XiaomiButton::from_hid_usage(usage);
        let id = btn.to_button_id();
        if id == "unknown" {
            continue;
        }
        // 录入会话中不映射注入（与 key_mapping::on_remote_button 一致）
        if crate::bridges::shared::shortcut_capture::is_swallow_active() {
            continue;
        }
        // 方向/OK：禁止 KeyEmitGate 挡映射（连点会被 60ms 去抖吞成只跳一格）
        if crate::bridges::xiaomi::key_mapping::should_gate_block_dpad_ok_mapping(id)
            && !gate.try_emit(id)
        {
            continue;
        }
        emit_key_and_map(app, id, button_label(id), true);
        tap_log(&format!(
            "XIAOMI HID TAP key={id} usage=0x{usage:04X} down raw={}",
            encode_hex(data)
        ));
    }
    for usage in released {
        let btn = XiaomiButton::from_hid_usage(usage);
        let id = btn.to_button_id();
        if id == "unknown" {
            continue;
        }
        emit_key_and_map(app, id, button_label(id), false);
        tap_log(&format!("XIAOMI HID TAP key={id} usage=0x{usage:04X} up"));
    }
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if s.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let hi = from_hex_digit(bytes[i])?;
        let lo = from_hex_digit(bytes[i + 1])?;
        out.push((hi << 4) | lo);
        i += 2;
    }
    Some(out)
}

fn from_hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn encode_hex(data: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(data.len() * 2);
    for &b in data {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0xf) as usize] as char);
    }
    s
}
