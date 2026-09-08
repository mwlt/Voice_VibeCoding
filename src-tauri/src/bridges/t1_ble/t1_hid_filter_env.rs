//! T1 蓝牙专属 L0：保证 Consumer Control 集合可用（恢复误禁用），Search 靠 L1 吞键。
//!
//! `pnputil` **无法**只禁 AC Search 单个 Usage；旧方案整集禁用会导致 Home/删除/音量全死。
//! 现行：自动修复 = Enable 白名单内 T1 Consumer；AC Search 由 LL/HotKey/WH_SHELL（L1）处理。

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

static STATUS_CACHE: Mutex<Option<(Instant, T1HidFilterEnvStatus)>> = Mutex::new(None);
static L0_READY: AtomicBool = AtomicBool::new(false);
static REFRESH_IN_FLIGHT: AtomicBool = AtomicBool::new(false);
/// 本进程是否已对「未禁用」跑过自动修复（避免反复弹 UAC）
static AUTO_REPAIR_ATTEMPTED: AtomicBool = AtomicBool::new(false);
static WATCHDOG_STARTED: AtomicBool = AtomicBool::new(false);
static LAST_AUTO_REPAIR_AT: Mutex<Option<Instant>> = Mutex::new(None);
/// 串行化 Status/Install，避免启动流水线与 BLE 连接同时弹两次 UAC
static REPAIR_LOCK: Mutex<()> = Mutex::new(());
/// 主机状态轮询很勤；L0 探测走 PowerShell 很慢，禁止在 IPC 热路径同步执行。
const STATUS_TTL: Duration = Duration::from_secs(60);
/// 重连后 Consumer 可能重新启用：冷却期内不重复弹 UAC
const AUTO_REPAIR_COOLDOWN: Duration = Duration::from_secs(90);
const WATCHDOG_INTERVAL: Duration = Duration::from_secs(45);

pub fn invalidate_status_cache() {
    *STATUS_CACHE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    L0_READY.store(false, Ordering::SeqCst);
}

/// 主机状态栏用：只读原子缓存，绝不跑 PowerShell。
pub fn l0_ready_fast() -> bool {
    kick_status_refresh_if_stale();
    L0_READY.load(Ordering::Relaxed)
}

/// 返回最近一次探测结果；若无缓存则立即返回占位并不阻塞，后台刷新。
pub fn env_status_nonblocking() -> T1HidFilterEnvStatus {
    kick_status_refresh_if_stale();
    if let Some((_at, ref st)) = *STATUS_CACHE.lock().unwrap_or_else(|e| e.into_inner()) {
        return st.clone();
    }
    placeholder_status("检测中…")
}

fn placeholder_status(message: &str) -> T1HidFilterEnvStatus {
    let package = find_driver_package_dir();
    T1HidFilterEnvStatus {
        ready: L0_READY.load(Ordering::Relaxed),
        service_present: false,
        binary_present: true, // 不再依赖自签 .sys
        package_available: find_install_script().is_some(),
        matched_device_count: 0,
        bound_device_count: 0,
        package_dir: package.map(|p| p.display().to_string()),
        message: message.into(),
        result_code: "PENDING".into(),
    }
}

fn cache_is_fresh() -> bool {
    match *STATUS_CACHE.lock().unwrap_or_else(|e| e.into_inner()) {
        Some((at, _)) => at.elapsed() < STATUS_TTL,
        None => false,
    }
}

/// 是否应对 L0 跑自动修复（恢复被禁用的 T1 Consumer）。
/// - 已就绪：否
/// - 冷却中：否
/// - 仅 `NOT_BOUND`（找到设备但仍有禁用实例）才修
pub fn should_auto_repair_l0(ready: bool, result_code: &str, blocked_by_attempt_or_cooldown: bool) -> bool {
    if ready || blocked_by_attempt_or_cooldown {
        return false;
    }
    result_code.eq_ignore_ascii_case("NOT_BOUND")
}

fn auto_repair_blocked() -> bool {
    if AUTO_REPAIR_ATTEMPTED.load(Ordering::SeqCst) {
        // 进程内首次尝试后仍允许冷却到期再试（BLE 重连可能冒出新实例）
        if let Some(at) = *LAST_AUTO_REPAIR_AT.lock().unwrap_or_else(|e| e.into_inner()) {
            if at.elapsed() < AUTO_REPAIR_COOLDOWN {
                return true;
            }
            return false;
        }
        return true;
    }
    false
}

fn mark_auto_repair_attempted() {
    AUTO_REPAIR_ATTEMPTED.store(true, Ordering::SeqCst);
    *LAST_AUTO_REPAIR_AT.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
}

/// 同步探测；若为 NOT_BOUND 则跑一次 Install（可能弹 UAC）。
pub fn ensure_ready_once(reason: &str) -> T1HidFilterActionResult {
    let _guard = REPAIR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    invalidate_status_cache();
    let status = env_status();
    L0_READY.store(status.ready, Ordering::SeqCst);
    *STATUS_CACHE.lock().unwrap_or_else(|e| e.into_inner()) =
        Some((Instant::now(), status.clone()));
    if status.ready {
        crate::bridges::t1::ble_keys::resync_gates_after_l0();
        return T1HidFilterActionResult {
            ok: true,
            ready: true,
            needs_reboot: false,
            message: status.message,
            result_code: "READY".into(),
        };
    }
    if !should_auto_repair_l0(status.ready, &status.result_code, auto_repair_blocked()) {
        log::info!(
            "T1 L0 auto-repair skip reason={reason} code={} ready={} blocked={}",
            status.result_code,
            status.ready,
            auto_repair_blocked()
        );
        return T1HidFilterActionResult {
            ok: false,
            ready: false,
            needs_reboot: false,
            message: status.message,
            result_code: status.result_code,
        };
    }
    mark_auto_repair_attempted();
    log::info!("T1 L0 auto-repair begin reason={reason}");
    // 已持 REPAIR_LOCK：直接 Install，避免 repair() 再抢锁死锁
    match repair_unlocked() {
        Ok(r) => {
            log::info!(
                "T1 L0 auto-repair done reason={reason} ready={} code={}",
                r.ready,
                r.result_code
            );
            r
        }
        Err(e) => {
            log::warn!("T1 L0 auto-repair failed reason={reason}: {e}");
            T1HidFilterActionResult {
                ok: false,
                ready: false,
                needs_reboot: false,
                message: e,
                result_code: "SCRIPT_ERROR".into(),
            }
        }
    }
}

/// 后台：BLE 连接 / 启动流水线调用，不阻塞 UI。
pub fn kick_auto_repair_if_needed(reason: &'static str) {
    let _ = thread::Builder::new()
        .name("t1-l0-auto-repair".into())
        .spawn(move || {
            let _ = ensure_ready_once(reason);
        });
}

/// 常驻看门狗：Consumer 被系统重新枚举启用后自动再禁（冷却防刷 UAC）。
pub fn spawn_auto_repair_watchdog() {
    if WATCHDOG_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = thread::Builder::new()
        .name("t1-l0-watchdog".into())
        .spawn(|| {
            log::info!(
                "T1 L0 watchdog start interval={}s cooldown={}s",
                WATCHDOG_INTERVAL.as_secs(),
                AUTO_REPAIR_COOLDOWN.as_secs()
            );
            loop {
                thread::sleep(WATCHDOG_INTERVAL);
                // 已就绪则只做轻量过期刷新；未就绪则走 ensure
                if L0_READY.load(Ordering::Relaxed) && cache_is_fresh() {
                    continue;
                }
                invalidate_status_cache();
                let st = env_status();
                L0_READY.store(st.ready, Ordering::SeqCst);
                *STATUS_CACHE.lock().unwrap_or_else(|e| e.into_inner()) =
                    Some((Instant::now(), st.clone()));
                if should_auto_repair_l0(st.ready, &st.result_code, auto_repair_blocked()) {
                    mark_auto_repair_attempted();
                    log::warn!(
                        "T1 L0 watchdog: Consumer 仍禁用，自动恢复启用 matched={} bound={}",
                        st.matched_device_count,
                        st.bound_device_count
                    );
                    let _guard = REPAIR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
                    let _ = repair_unlocked();
                }
            }
        });
}

/// 过期时在后台线程跑 Status；可重入、单飞。
pub fn kick_status_refresh_if_stale() {
    if cache_is_fresh() {
        return;
    }
    if REFRESH_IN_FLIGHT
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }
    let _ = thread::Builder::new()
        .name("t1-l0-status".into())
        .spawn(|| {
            let st = env_status();
            L0_READY.store(st.ready, Ordering::SeqCst);
            *STATUS_CACHE.lock().unwrap_or_else(|e| e.into_inner()) =
                Some((Instant::now(), st));
            REFRESH_IN_FLIGHT.store(false, Ordering::SeqCst);
        });
}

/// 兼容旧调用：仍可能阻塞；优先用 `env_status_nonblocking` / `l0_ready_fast`。
pub fn env_status_cached() -> T1HidFilterEnvStatus {
    if let Some((at, ref st)) = *STATUS_CACHE.lock().unwrap_or_else(|e| e.into_inner()) {
        if at.elapsed() < STATUS_TTL {
            return st.clone();
        }
    }
    kick_status_refresh_if_stale();
    env_status_nonblocking()
}

/// 与 `ble_keys::BLE_DEVICE_MATCH` / 安装脚本 Allow 列表一致（文档与单测用）。
pub const T1_BLE_L0_ALLOW_TOKENS: &[&str] = &[
    "dev_vid&01620a_pid&0407",
    "vid_1620a&pid_0407",
    "vid_1620&pid_0407",
    "vid_01620a&pid_0407",
    "t1-remote",
    "t1_remote",
];

/// 硬拒绝：USB 接收器、小米、WinUHid、常见外设（脚本侧也会拦）。
pub const T1_BLE_L0_DENY_TOKENS: &[&str] = &[
    "vid_1915",
    "pid_1025",
    "vid_2717",
    "winuhid",
    "root\\winuhid",
];

pub fn hardware_id_is_t1_ble_only(hardware_blob: &str) -> bool {
    let b = hardware_blob.to_ascii_lowercase();
    if T1_BLE_L0_DENY_TOKENS
        .iter()
        .any(|d| b.contains(&d.to_ascii_lowercase()))
    {
        return false;
    }
    if T1_BLE_L0_ALLOW_TOKENS
        .iter()
        .any(|a| b.contains(&a.to_ascii_lowercase()))
    {
        return true;
    }
    let vid = b.contains("01620a")
        || b.contains("vid_1620a")
        || b.contains("vid_1620&")
        || b.contains("vid_01620a");
    let pid = b.contains("pid_0407") || b.contains("pid&0407");
    vid && pid
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct T1HidFilterEnvStatus {
    pub ready: bool,
    pub service_present: bool,
    pub binary_present: bool,
    pub package_available: bool,
    pub matched_device_count: u32,
    pub bound_device_count: u32,
    pub package_dir: Option<String>,
    pub message: String,
    pub result_code: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct T1HidFilterActionResult {
    pub ok: bool,
    pub ready: bool,
    pub needs_reboot: bool,
    pub message: String,
    pub result_code: String,
}

fn asset_candidates(relative: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            out.push(dir.join("assets").join("t1_hid_filter").join(relative));
            out.push(
                dir.join("resources")
                    .join("assets")
                    .join("t1_hid_filter")
                    .join(relative),
            );
            out.push(dir.join("resources").join("t1_hid_filter").join(relative));
            if let Some(parent) = dir.parent() {
                out.push(
                    parent
                        .join("resources")
                        .join("assets")
                        .join("t1_hid_filter")
                        .join(relative),
                );
            }
        }
    }
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    out.push(
        manifest
            .join("assets")
            .join("t1_hid_filter")
            .join(relative),
    );
    if let Ok(cwd) = std::env::current_dir() {
        out.push(
            cwd.join("src-tauri")
                .join("assets")
                .join("t1_hid_filter")
                .join(relative),
        );
        out.push(cwd.join("assets").join("t1_hid_filter").join(relative));
    }
    out
}

pub fn find_install_script() -> Option<PathBuf> {
    asset_candidates("install-t1-hid-filter.ps1")
        .into_iter()
        .find(|p| p.is_file())
}

pub fn find_asset_root() -> Option<PathBuf> {
    find_install_script().and_then(|s| s.parent().map(|p| p.to_path_buf()))
}

pub fn find_driver_package_dir() -> Option<PathBuf> {
    find_asset_root().map(|r| r.join("driver")).filter(|p| p.is_dir())
}

fn script_result_line(text: &str) -> Option<&str> {
    text.lines()
        .find_map(|l| l.trim().strip_prefix("Result: ").map(str::trim))
}

fn parse_status_phase(stdout: &str) -> (bool, bool, bool, u32, u32) {
    // Phase: Status | ready=.. svc=.. bin=.. matched=.. bound=..
    for line in stdout.lines() {
        let Some(rest) = line.trim().strip_prefix("Phase: Status | ") else {
            continue;
        };
        let mut ready = false;
        let mut svc = false;
        let mut bin = false;
        let mut matched = 0u32;
        let mut bound = 0u32;
        for part in rest.split_whitespace() {
            if let Some(v) = part.strip_prefix("ready=") {
                ready = v.eq_ignore_ascii_case("true");
            } else if let Some(v) = part.strip_prefix("svc=") {
                svc = v.eq_ignore_ascii_case("true");
            } else if let Some(v) = part.strip_prefix("bin=") {
                bin = v.eq_ignore_ascii_case("true");
            } else if let Some(v) = part.strip_prefix("matched=") {
                matched = v.parse().unwrap_or(0);
            } else if let Some(v) = part.strip_prefix("bound=") {
                bound = v.parse().unwrap_or(0);
            }
        }
        return (ready, svc, bin, matched, bound);
    }
    (false, false, false, 0, 0)
}

fn run_script(mode: &str) -> Result<(i32, String, String), String> {
    let script = find_install_script().ok_or_else(|| "未找到 install-t1-hid-filter.ps1".to_string())?;
    let mut cmd = Command::new("powershell.exe");
    let mut args = vec![
        "-NoProfile".into(),
        "-WindowStyle".into(),
        "Hidden".into(),
        "-ExecutionPolicy".into(),
        "Bypass".into(),
        "-File".into(),
        script.display().to_string(),
        "-Mode".into(),
        mode.to_string(),
    ];
    if let Some(package) = find_driver_package_dir() {
        args.push("-PackageDir".into());
        args.push(package.display().to_string());
    }
    cmd.args(&args);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let output = cmd
        .output()
        .map_err(|e| format!("启动 L0 安装脚本失败: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stdout.is_empty() {
        log::info!("T1 L0 filter {mode} stdout:\n{stdout}");
    }
    if !stderr.is_empty() {
        log::warn!("T1 L0 filter {mode} stderr:\n{stderr}");
    }
    Ok((output.status.code().unwrap_or(-1), stdout, stderr))
}

pub fn env_status() -> T1HidFilterEnvStatus {
    let package = find_driver_package_dir();
    let package_available = find_install_script().is_some();

    let st = if !package_available {
        T1HidFilterEnvStatus {
            ready: false,
            service_present: false,
            binary_present: true,
            package_available: false,
            matched_device_count: 0,
            bound_device_count: 0,
            package_dir: None,
            message: "T1 BLE Search 剥离脚本缺失。".into(),
            result_code: "NO_PACKAGE".into(),
        }
    } else {
        #[cfg(not(target_os = "windows"))]
        {
            T1HidFilterEnvStatus {
                ready: false,
                service_present: false,
                binary_present: true,
                package_available: true,
                matched_device_count: 0,
                bound_device_count: 0,
                package_dir: package.map(|p| p.display().to_string()),
                message: "L0 Search 剥离仅支持 Windows。".into(),
                result_code: "UNSUPPORTED_OS".into(),
            }
        }
        #[cfg(target_os = "windows")]
        {
            match run_script("Status") {
                Ok((_code, stdout, _stderr)) => {
                    let result = script_result_line(&stdout)
                        .unwrap_or("UNKNOWN")
                        .to_string();
                    let (ready, _svc, _bin, matched, bound) = parse_status_phase(&stdout);
                    let message = match result.as_str() {
                        "READY" => {
                            "T1 Consumer 已启用（Home/删除/音量可用）。AC Search 由应用内 L1 吞键，非整集禁用。".into()
                        }
                        "NO_T1_BLE_DEVICE" => {
                            "未检测到 T1 蓝牙 Consumer HID（01620A/0407）。请先系统配对并连接 T1-Remote。".into()
                        }
                        "NOT_BOUND" => {
                            "T1 Consumer 仍有禁用实例（旧整集剥离残留）。点「自动修复」重新启用（需 UAC）。".into()
                        }
                        other => format!("L0 状态：{other}"),
                    };
                    T1HidFilterEnvStatus {
                        ready,
                        service_present: false,
                        binary_present: true,
                        package_available: true,
                        matched_device_count: matched,
                        bound_device_count: bound,
                        package_dir: package.map(|p| p.display().to_string()),
                        message,
                        result_code: result,
                    }
                }
                Err(e) => T1HidFilterEnvStatus {
                    ready: false,
                    service_present: false,
                    binary_present: true,
                    package_available: true,
                    matched_device_count: 0,
                    bound_device_count: 0,
                    package_dir: package.map(|p| p.display().to_string()),
                    message: e,
                    result_code: "SCRIPT_ERROR".into(),
                },
            }
        }
    };
    L0_READY.store(st.ready, Ordering::SeqCst);
    st
}

pub fn repair() -> Result<T1HidFilterActionResult, String> {
    let _guard = REPAIR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    repair_unlocked()
}

fn repair_unlocked() -> Result<T1HidFilterActionResult, String> {
    invalidate_status_cache();
    let status = env_status();
    L0_READY.store(status.ready, Ordering::SeqCst);
    *STATUS_CACHE.lock().unwrap_or_else(|e| e.into_inner()) =
        Some((Instant::now(), status.clone()));
    if status.ready {
        return Ok(T1HidFilterActionResult {
            ok: true,
            ready: true,
            needs_reboot: false,
            message: status.message,
            result_code: "READY".into(),
        });
    }

    let (_code, stdout, _stderr) = run_script("Install")?;
    invalidate_status_cache();
    let result = script_result_line(&stdout)
        .unwrap_or("UNKNOWN")
        .to_string();
    let after = env_status();
    L0_READY.store(after.ready || result == "READY", Ordering::SeqCst);
    *STATUS_CACHE.lock().unwrap_or_else(|e| e.into_inner()) =
        Some((Instant::now(), after.clone()));
    let ready = after.ready || result == "READY";
    if ready {
        crate::bridges::t1::ble_keys::resync_gates_after_l0();
    }
    let message = if ready {
        "已恢复 T1 Consumer Control：Home/删除/音量可映射。Windows 搜索靠应用内 L1 防护（无法用 pnputil 只剥 AC Search 一键）。".into()
    } else {
        match result.as_str() {
            "NO_T1_BLE_DEVICE" => after.message,
            "REFUSED_NON_T1_BLE" => {
                "安全拒绝：匹配结果含非 T1 蓝牙设备，未改动任何设备。".into()
            }
            "NEED_ADMIN" => "需要管理员权限（UAC）。请允许提权后重试。".into(),
            other => format!("{}（{other}）", after.message),
        }
    };
    Ok(T1HidFilterActionResult {
        ok: ready,
        ready,
        needs_reboot: false,
        message,
        result_code: result,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allow_t1_ble_tokens_only() {
        assert!(hardware_id_is_t1_ble_only(
            r"HID\VID_1620A&PID_0407\dev_vid&01620a_pid&0407"
        ));
        assert!(hardware_id_is_t1_ble_only(
            "HID\\{00001812-0000-1000-8000-00805f9b34fb}_Dev_VID&01620A_PID&0407"
        ));
        assert!(hardware_id_is_t1_ble_only("T1-Remote BLE"));
    }

    #[test]
    fn deny_usb_xiaomi_winuhid() {
        assert!(!hardware_id_is_t1_ble_only(r"HID\VID_1915&PID_1025&MI_01"));
        assert!(!hardware_id_is_t1_ble_only(r"HID\VID_2717&PID_32B8"));
        assert!(!hardware_id_is_t1_ble_only(r"Root\WinUHid"));
        assert!(!hardware_id_is_t1_ble_only(r"HID\VID_046D&PID_C52B"));
    }

    #[test]
    fn package_layout_exists_in_repo() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/t1_hid_filter");
        assert!(root.join("install-t1-hid-filter.ps1").is_file());
    }

    #[test]
    fn auto_repair_only_when_not_bound() {
        assert!(!should_auto_repair_l0(true, "READY", false));
        assert!(!should_auto_repair_l0(false, "NOT_BOUND", true));
        assert!(!should_auto_repair_l0(false, "NO_T1_BLE_DEVICE", false));
        assert!(!should_auto_repair_l0(false, "NEED_ADMIN", false));
        assert!(should_auto_repair_l0(false, "NOT_BOUND", false));
        assert!(should_auto_repair_l0(false, "not_bound", false));
    }
}
