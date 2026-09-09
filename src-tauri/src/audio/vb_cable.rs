//! 小米语音环境：检测 VB-CABLE，并用内嵌官方驱动包修复
//!
//! **安装（未就绪 / 强制重装）**：解压内嵌 `VBCABLE_Driver_Pack45.zip`，启动官方
//! `VBCABLE_Setup_x64.exe`（有界面）。解压优先到安装目录旁 `vb-cable-official-stage`，
//! 无写权限（如 Program Files）时回退 `%LOCALAPPDATA%\Voice VibeCoding\…`。
//! 不再使用 SetupAPI 静默注册根设备（易双实例、Failed Start）。
//!
//! **已就绪时的「自动修复」**：仅 EnsureMic（设默认麦为 CABLE Output），不重装驱动。
//!
//! **探测策略（长期最优）**：
//! - 优先读 MMDevices **注册表**（与 configure 脚本一致），避免 cpal/WASAPI 枚举打爆 audiodg
//! - 仅当注册表 **读失败** 时才 cpal 兜底一次；「没有 CABLE」不算失败
//! - 启动实探一次；**已就绪则停探**；**未就绪**按间隔重试（默认 60s）
//! - 「检测/修复」走 `voice_env_status_fresh` / `invalidate` 强制重探
//! - `REMOTE_BRIDGE_CABLE_PROBE_TTL_MS`：未就绪重试间隔；`0` = 未就绪也不自动重试

use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::copy;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub const DRIVER_ZIP_NAME: &str = "VBCABLE_Driver_Pack45.zip";
pub const DRIVER_ZIP_SHA256: &str =
    "b950e39f01af1d04ea623c8f6d8eb9b6ea5c477c637295fabf20631c85116bfb";
pub const CONFIGURE_SCRIPT_NAME: &str = "configure-xiaomi-audio.ps1";
pub const DOWNLOAD_PAGE_URL: &str = "https://vb-audio.com/Cable/";
pub const DOWNLOAD_ZIP_URL: &str =
    "https://download.vb-audio.com/Download_CABLE/VBCABLE_Driver_Pack45.zip";

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceEnvStatus {
    pub ready: bool,
    pub cable_input: bool,
    pub cable_output: bool,
    pub embedded_available: bool,
    pub embedded_zip_path: Option<String>,
    pub download_page_url: String,
    pub download_zip_url: String,
    pub message: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceEnvActionResult {
    pub ok: bool,
    pub ready: bool,
    pub needs_choice: bool,
    pub needs_reboot: bool,
    pub message: String,
    pub report_path: Option<String>,
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let file = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let mut reader = std::io::BufReader::new(file);
    let mut hasher = Sha256::new();
    copy(&mut reader, &mut hasher).map_err(|e| format!("hash {}: {e}", path.display()))?;
    Ok(format!("{:x}", hasher.finalize()))
}

fn asset_candidates(file_name: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(p) = std::env::var("REMOTE_BRIDGE_XIAOMI_VB_CABLE_ZIP") {
        if file_name == DRIVER_ZIP_NAME {
            out.push(PathBuf::from(p));
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            out.push(dir.join("assets").join("xiaomi").join(file_name));
            out.push(
                dir.join("resources")
                    .join("assets")
                    .join("xiaomi")
                    .join(file_name),
            );
            out.push(
                dir.join("_up_")
                    .join("resources")
                    .join("assets")
                    .join("xiaomi")
                    .join(file_name),
            );
            if let Some(parent) = dir.parent() {
                out.push(
                    parent
                        .join("resources")
                        .join("assets")
                        .join("xiaomi")
                        .join(file_name),
                );
            }
        }
    }
    if let Some(manifest) = option_env!("CARGO_MANIFEST_DIR") {
        out.push(
            PathBuf::from(manifest)
                .join("assets")
                .join("xiaomi")
                .join(file_name),
        );
    }
    out
}

pub fn find_driver_zip() -> Option<PathBuf> {
    for path in asset_candidates(DRIVER_ZIP_NAME) {
        if !path.is_file() {
            continue;
        }
        match sha256_file(&path) {
            Ok(hash) if hash.eq_ignore_ascii_case(DRIVER_ZIP_SHA256) => return Some(path),
            Ok(hash) => log::warn!(
                "VB-CABLE zip hash mismatch path={} got={hash}",
                path.display()
            ),
            Err(e) => log::warn!("VB-CABLE zip unreadable: {e}"),
        }
    }
    None
}

pub fn find_configure_script() -> Option<PathBuf> {
    asset_candidates(CONFIGURE_SCRIPT_NAME)
        .into_iter()
        .find(|p| p.is_file())
}

/// 官方安装器文件名（64 位优先 x64）
pub fn official_setup_exe_name() -> &'static str {
    if cfg!(target_pointer_width = "64") {
        "VBCABLE_Setup_x64.exe"
    } else {
        "VBCABLE_Setup.exe"
    }
}

fn official_setup_fallback_name() -> &'static str {
    if cfg!(target_pointer_width = "64") {
        "VBCABLE_Setup.exe"
    } else {
        "VBCABLE_Setup_x64.exe"
    }
}

const STAGE_DIR_NAME: &str = "vb-cable-official-stage";

fn local_app_data_stage_root() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("Voice VibeCoding").join(STAGE_DIR_NAME)
}

/// 探测目录是否可写（不留下永久文件）。
fn path_parent_writable(parent: &Path) -> bool {
    if std::fs::create_dir_all(parent).is_err() {
        return false;
    }
    let probe = parent.join(format!(".vb-cable-wprobe-{}", std::process::id()));
    match std::fs::write(&probe, b"ok") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// 优先解压到安装目录旁；`Program Files` 等无写权限时回退 LocalAppData。
fn stage_root() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            // 安装根目录：…\Voice VibeCoding\vb-cable-official-stage
            if path_parent_writable(dir) {
                return dir.join(STAGE_DIR_NAME);
            }
            // 资源目录旁：…\resources\vb-cable-official-stage（与内嵌 zip 同层更近）
            let resources = dir.join("resources");
            if path_parent_writable(&resources) {
                return resources.join(STAGE_DIR_NAME);
            }
            if let Some(parent) = dir.parent() {
                if path_parent_writable(parent) {
                    return parent.join(STAGE_DIR_NAME);
                }
                let parent_resources = parent.join("resources");
                if path_parent_writable(&parent_resources) {
                    return parent_resources.join(STAGE_DIR_NAME);
                }
            }
        }
    }
    local_app_data_stage_root()
}

fn find_file_named(root: &Path, name: &str) -> Option<PathBuf> {
    let direct = root.join(name);
    if direct.is_file() {
        return Some(direct);
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for ent in rd.flatten() {
            let p = ent.path();
            if p.is_dir() {
                stack.push(p);
            } else if p
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.eq_ignore_ascii_case(name))
            {
                return Some(p);
            }
        }
    }
    None
}

/// 解压内嵌 zip 到本地 staging，返回官方 Setup.exe 路径
pub fn stage_official_setup(zip: &Path) -> Result<PathBuf, String> {
    let hash = sha256_file(zip)?;
    if !hash.eq_ignore_ascii_case(DRIVER_ZIP_SHA256) {
        return Err(format!(
            "内嵌驱动包校验失败（hash={hash}），请改用官网包或重装本软件"
        ));
    }
    let stage = stage_root();
    log::info!(
        "XIAOMI VOICE ENV: staging official setup under {}",
        stage.display()
    );
    let marker = stage.join(".zip.sha256");
    let preferred = official_setup_exe_name();
    let reuse = marker
        .is_file()
        .then(|| std::fs::read_to_string(&marker).ok())
        .flatten()
        .is_some_and(|h| h.trim().eq_ignore_ascii_case(&hash))
        && find_file_named(&stage, preferred).is_some();
    if !reuse {
        if stage.exists() {
            let _ = std::fs::remove_dir_all(&stage);
        }
        std::fs::create_dir_all(&stage)
            .map_err(|e| format!("创建解压目录失败: {e}"))?;
        // Windows 自带 tar 可解 zip，避免再引 zip crate / 静默 SetupAPI
        let status = Command::new("tar")
            .args([
                "-xf",
                &zip.display().to_string(),
                "-C",
                &stage.display().to_string(),
            ])
            .status()
            .map_err(|e| format!("解压驱动包失败（tar）: {e}"))?;
        if !status.success() {
            // 兜底：PowerShell Expand-Archive
            let ps = format!(
                "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
                zip.display().to_string().replace('\'', "''"),
                stage.display().to_string().replace('\'', "''")
            );
            let st = Command::new("powershell.exe")
                .args(["-NoProfile", "-Command", &ps])
                .status()
                .map_err(|e| format!("解压驱动包失败（Expand-Archive）: {e}"))?;
            if !st.success() {
                return Err("解压 VB-CABLE 驱动包失败".into());
            }
        }
        std::fs::write(&marker, hash).map_err(|e| format!("写解压标记失败: {e}"))?;
    }
    find_file_named(&stage, preferred)
        .or_else(|| find_file_named(&stage, official_setup_fallback_name()))
        .ok_or_else(|| {
            format!(
                "解压后未找到官方安装程序（期望 {}）",
                preferred
            )
        })
}

/// 无黑框启动 GUI 安装器并等待退出，返回进程退出码。
#[cfg(target_os = "windows")]
fn launch_gui_exe_and_wait(exe: &Path) -> Result<Option<i32>, String> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::{CloseHandle, GetLastError, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, WaitForSingleObject, INFINITE,
    };
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let cwd = exe
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| stage_root());
    let file_h = HSTRING::from(exe.as_os_str());
    let cwd_h = HSTRING::from(cwd.as_os_str());

    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpFile: PCWSTR(file_h.as_ptr()),
        lpDirectory: PCWSTR(cwd_h.as_ptr()),
        nShow: SW_SHOWNORMAL.0 as i32,
        ..Default::default()
    };

    let ok = unsafe { ShellExecuteExW(&mut info).is_ok() };
    if !ok {
        let code = unsafe { GetLastError() }.0;
        return Err(format!("启动官方安装程序失败 GetLastError={code}"));
    }
    if info.hProcess.is_invalid() {
        return Ok(Some(0));
    }
    unsafe {
        let wait = WaitForSingleObject(info.hProcess, INFINITE);
        if wait != WAIT_OBJECT_0 {
            let _ = CloseHandle(info.hProcess);
            return Err("等待官方安装程序结束失败".into());
        }
        let mut code: u32 = 0;
        let _ = GetExitCodeProcess(info.hProcess, &mut code);
        let _ = CloseHandle(info.hProcess);
        Ok(Some(code as i32))
    }
}

#[cfg(not(target_os = "windows"))]
fn launch_gui_exe_and_wait(_exe: &Path) -> Result<Option<i32>, String> {
    Err("仅 Windows 支持".into())
}

/// 启动官方有界面安装器并等待退出，再探测端点（不经 cmd/powershell，无黑框）。
pub fn run_official_setup_installer(_force_ui: bool) -> Result<VoiceEnvActionResult, String> {
    let zip = find_driver_zip().ok_or_else(|| "内嵌 VB-CABLE 驱动包不可用".to_string())?;
    let setup = stage_official_setup(&zip)?;
    log::info!(
        "XIAOMI VOICE ENV: launching official setup UI (ShellExecute, no console) setup={}",
        setup.display()
    );

    let code = launch_gui_exe_and_wait(&setup)?;
    log::info!("XIAOMI VOICE ENV: official setup exited code={code:?}");

    invalidate_cable_probe_cache();
    for _ in 0..20 {
        let (i, o) = probe_cable_endpoints(true);
        if i && o {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    let (cable_input, cable_output) = probe_cable_endpoints(true);
    let ready = cable_input && cable_output;

    let needs_reboot = !ready || code == Some(3010) || code == Some(1641);

    let message = if ready {
        match run_configure_script_ex("EnsureMic", None, false) {
            Ok(r) => {
                if r.ready {
                    "官方 VB-CABLE 安装完成，默认麦克风已设为 CABLE Output。".into()
                } else {
                    format!(
                        "官方安装完成且已检测到 CABLE，但设置默认麦克风未完成：{}",
                        r.message
                    )
                }
            }
            Err(e) => format!(
                "官方安装完成且已检测到 CABLE，但设置默认麦克风失败：{e}"
            ),
        }
    } else if needs_reboot {
        "官方安装程序已结束。请按提示重启 Windows，再打开本软件点一次「虚拟声卡修复」→「自动修复」。"
            .into()
    } else if code == Some(0) {
        "官方安装程序已退出，但仍未检测到 CABLE Input/Output。若安装器要求重启请先重启；或到设备管理器检查 VB-Audio 是否 Error。"
            .into()
    } else {
        format!(
            "官方安装程序退出码 {:?}。若已取消 UAC/安装，可再试；仍失败请用官网包手动安装。",
            code
        )
    };

    Ok(VoiceEnvActionResult {
        ok: ready || needs_reboot || code == Some(0),
        ready,
        needs_choice: false,
        needs_reboot: needs_reboot && !ready,
        message,
        report_path: None,
    })
}

#[cfg(target_os = "windows")]
fn probe_via_cpal() -> (bool, bool) {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    let mut cable_input = false;
    let mut cable_output = false;
    if let Ok(devices) = host.output_devices() {
        for d in devices {
            if let Ok(name) = d.name() {
                if name.to_ascii_lowercase().contains("cable input") {
                    cable_input = true;
                }
            }
        }
    }
    if let Ok(devices) = host.input_devices() {
        for d in devices {
            if let Ok(name) = d.name() {
                if name.to_ascii_lowercase().contains("cable output") {
                    cable_output = true;
                }
            }
        }
    }
    (cable_input, cable_output)
}

#[cfg(target_os = "windows")]
fn mmdevices_endpoint_label(props: &winreg::RegKey) -> String {
    // 对齐 configure-xiaomi-audio.ps1 / Python native_audio
    const PKEY_DEVICE: &str = "{a45c254e-df1c-4efd-8020-67d146a850e0},2";
    const PKEY_ENDPOINT: &str = "{b3f8fa53-0004-438e-9003-51a46e139bfc},6";
    let mut parts: Vec<String> = Vec::new();
    for key in [PKEY_DEVICE, PKEY_ENDPOINT] {
        if let Ok(v) = props.get_value::<String, _>(key) {
            let t = v.trim();
            if !t.is_empty() && !parts.iter().any(|p| p.eq_ignore_ascii_case(t)) {
                parts.push(t.to_string());
            }
        }
    }
    parts.join(" ")
}

/// 对齐脚本：只认 DeviceState==1（Active）且名称匹配的端点。
#[cfg(target_os = "windows")]
fn registry_flow_has_cable(flow: &str, needle: &str) -> Result<bool, String> {
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ};
    use winreg::RegKey;

    let path = format!(r"SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\{flow}");
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let root = hklm
        .open_subkey_with_flags(&path, KEY_READ)
        .map_err(|e| format!("open {path}: {e}"))?;

    let needle = needle.to_ascii_lowercase();
    for endpoint_id in root.enum_keys().filter_map(|k| k.ok()) {
        let Ok(endpoint) = root.open_subkey_with_flags(&endpoint_id, KEY_READ) else {
            continue;
        };
        let state: u32 = match endpoint.get_value("DeviceState") {
            Ok(v) => v,
            Err(_) => continue,
        };
        // DEVICE_STATE_ACTIVE = 1
        if state != 1 {
            continue;
        }
        let Ok(props) = endpoint.open_subkey_with_flags("Properties", KEY_READ) else {
            continue;
        };
        let label = mmdevices_endpoint_label(&props);
        if label.to_ascii_lowercase().contains(&needle) {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(target_os = "windows")]
fn probe_via_registry() -> Result<(bool, bool), String> {
    let cable_input = registry_flow_has_cable("Render", "cable input")?;
    let cable_output = registry_flow_has_cable("Capture", "cable output")?;
    Ok((cable_input, cable_output))
}

#[cfg(target_os = "windows")]
fn probe_cable_endpoints_uncached() -> (bool, bool) {
    match probe_via_registry() {
        Ok(v) => v,
        Err(e) => {
            log::warn!("VB-CABLE registry probe failed ({e}); cpal fallback once");
            probe_via_cpal()
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn probe_cable_endpoints_uncached() -> (bool, bool) {
    (false, false)
}

/// 未就绪时的自动重试间隔。`REMOTE_BRIDGE_CABLE_PROBE_TTL_MS`：默认 60000；`0` = 不自动重试。
fn not_ready_retry_interval() -> Option<Duration> {
    match std::env::var("REMOTE_BRIDGE_CABLE_PROBE_TTL_MS") {
        Ok(s) => {
            let s = s.trim();
            if s.is_empty() {
                return Some(Duration::from_secs(60));
            }
            match s.parse::<u64>() {
                Ok(0) => None,
                Ok(ms) => Some(Duration::from_millis(ms)),
                Err(_) => Some(Duration::from_secs(60)),
            }
        }
        Err(_) => Some(Duration::from_secs(60)),
    }
}

struct CableProbeCache {
    at: Instant,
    cable_input: bool,
    cable_output: bool,
}

static CABLE_PROBE: Mutex<Option<CableProbeCache>> = Mutex::new(None);

fn probe_cable_endpoints(force: bool) -> (bool, bool) {
    if !force {
        let g = CABLE_PROBE.lock();
        if let Some(c) = g.as_ref() {
            let ready = c.cable_input && c.cable_output;
            if ready {
                // 已就绪：停探，直到 invalidate / fresh
                return (c.cable_input, c.cable_output);
            }
            match not_ready_retry_interval() {
                None => return (c.cable_input, c.cable_output),
                Some(interval) if c.at.elapsed() < interval => {
                    return (c.cable_input, c.cable_output);
                }
                Some(_) => {}
            }
        }
    }
    let (cable_input, cable_output) = probe_cable_endpoints_uncached();
    *CABLE_PROBE.lock() = Some(CableProbeCache {
        at: Instant::now(),
        cable_input,
        cable_output,
    });
    log::debug!("VB-CABLE probe input={cable_input} output={cable_output} force={force}");
    (cable_input, cable_output)
}

/// 安装/修复后立刻失效缓存，下次 status 会重探
pub fn invalidate_cable_probe_cache() {
    *CABLE_PROBE.lock() = None;
}

pub fn voice_env_status() -> VoiceEnvStatus {
    voice_env_status_inner(false)
}

/// 用户主动「检测/修复」时强制重探
pub fn voice_env_status_fresh() -> VoiceEnvStatus {
    voice_env_status_inner(true)
}

fn voice_env_status_inner(force: bool) -> VoiceEnvStatus {
    let (cable_input, cable_output) = probe_cable_endpoints(force);
    let ready = cable_input && cable_output;
    let zip = find_driver_zip();
    let embedded_available = zip.is_some() && find_configure_script().is_some();
    let message = if ready {
        "VB-CABLE 已就绪。可点「虚拟声卡修复」将默认麦克风设为 CABLE Output。".into()
    } else if embedded_available {
        "未检测到 VB-CABLE。可使用内嵌驱动安装，或打开官网下载最新版。".into()
    } else {
        "未检测到 VB-CABLE，且内嵌驱动包不可用。请从官网下载安装。".into()
    };
    VoiceEnvStatus {
        ready,
        cable_input,
        cable_output,
        embedded_available,
        embedded_zip_path: zip.map(|p| p.display().to_string()),
        download_page_url: DOWNLOAD_PAGE_URL.into(),
        download_zip_url: DOWNLOAD_ZIP_URL.into(),
        message,
    }
}

fn app_path_for_script() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn script_result_line(text: &str) -> Option<&str> {
    text.lines()
        .find_map(|l| l.trim().strip_prefix("Result: ").map(str::trim))
}

fn humanize_script_result(mode: &str, raw: &str, ready: bool, needs_reboot: bool) -> String {
    if mode.eq_ignore_ascii_case("EnsureUsbMic") {
        if raw.eq_ignore_ascii_case("OK") || raw.is_empty() {
            return "USB 语音：默认麦克风已设为 Mic Device。".into();
        }
        if let Some(rest) = raw.strip_prefix("WARNING:") {
            return format!("USB 默认麦克风切换未完成：{}", rest.trim());
        }
        return format!("USB 默认麦克风处理结束：{raw}");
    }
    let lower = raw.to_ascii_lowercase();
    if lower.contains("restart required") || raw.contains("需要重启") {
        return "驱动已安装，必须重启 Windows 后虚拟声卡才会生效。重启后再点一次「虚拟声卡修复」。"
            .into();
    }
    if let Some(rest) = raw.strip_prefix("WARNING:") {
        let detail = rest.trim();
        let detail_cn = if detail.is_empty() {
            "原因未知，请查看应用日志。".into()
        } else if detail.to_ascii_lowercase().contains("uac")
            || detail.to_ascii_lowercase().contains("cancelled")
            || detail.contains("did not start")
        {
            "未获得管理员授权（UAC），安装已取消。".into()
        } else if detail.to_ascii_lowercase().contains("hash mismatch") {
            "内嵌驱动包校验失败，请改用官网驱动或重装本软件。".into()
        } else if detail.to_ascii_lowercase().contains("not available")
            || detail.to_ascii_lowercase().contains("not ready")
        {
            "仍未检测到 CABLE Output，请重启电脑后再试。".into()
        } else {
            detail.to_string()
        };
        return format!("虚拟声卡修复未完成：{detail_cn}");
    }
    if raw.eq_ignore_ascii_case("OK") || raw.is_empty() {
        if ready {
            return "语音环境已就绪：VB-CABLE 可用，默认麦克风已设为 CABLE Output。".into();
        }
        if needs_reboot {
            return "驱动已安装，必须重启 Windows 后虚拟声卡才会生效。重启后再点一次「虚拟声卡修复」。"
                .into();
        }
        return "脚本已执行，但尚未检测到 CABLE Input/Output。若刚装驱动请重启后再试。"
            .into();
    }
    if ready {
        format!("语音环境已就绪（{raw}）。")
    } else {
        format!("虚拟声卡处理结束：{raw}")
    }
}

fn run_configure_script_ex(
    mode: &str,
    zip: Option<&Path>,
    force: bool,
) -> Result<VoiceEnvActionResult, String> {
    let script = find_configure_script().ok_or_else(|| "未找到 configure-xiaomi-audio.ps1".to_string())?;
    let app_path = app_path_for_script();
    log::info!(
        "XIAOMI VOICE ENV: run script mode={mode} force={force} zip={} app={}",
        zip.map(|p| p.display().to_string())
            .unwrap_or_else(|| "(none)".into()),
        app_path.display()
    );

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
        "-AppPath".into(),
        app_path.display().to_string(),
    ];
    if let Some(zip) = zip {
        args.push("-DriverZipPath".into());
        args.push(zip.display().to_string());
    }
    if force {
        args.push("-Force".into());
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
        .map_err(|e| format!("启动语音环境脚本失败: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stdout.is_empty() {
        log::info!("XIAOMI VOICE ENV stdout:\n{stdout}");
    }
    if !stderr.is_empty() {
        log::warn!("XIAOMI VOICE ENV stderr:\n{stderr}");
    }

    let result_raw = script_result_line(&stdout).unwrap_or("").to_string();
    let needs_reboot = result_raw.to_ascii_lowercase().contains("restart required")
        || result_raw.contains("需要重启")
        || stdout.to_ascii_lowercase().contains("restart required")
        || output.status.code() == Some(3010);

    let usb_mic_mode = mode.eq_ignore_ascii_case("EnsureUsbMic");
    let (ready, message) = if usb_mic_mode {
        // EnsureUsbMic 只切默认录音端点到 Mic Device，与 VB-CABLE 无关
        let ok = output.status.success()
            && (result_raw.eq_ignore_ascii_case("OK")
                || result_raw.is_empty()
                || !result_raw.to_ascii_uppercase().starts_with("WARNING"));
        let message = if !output.status.success() {
            let detail = if !stderr.is_empty() {
                stderr
            } else if !result_raw.is_empty() {
                result_raw.clone()
            } else {
                stdout
            };
            format!(
                "USB Mic Device 脚本执行失败 (code={:?})。{}",
                output.status.code(),
                detail
            )
        } else {
            humanize_script_result(mode, &result_raw, ok, needs_reboot)
        };
        (ok, message)
    } else {
        // 稍等端点出现（强制重探，安装后缓存必须失效）
        invalidate_cable_probe_cache();
        for _ in 0..15 {
            let (i, o) = probe_cable_endpoints(true);
            if i && o {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        let (cable_input, cable_output) = probe_cable_endpoints(true);
        let ready = cable_input && cable_output;

        let message = if !output.status.success() && !needs_reboot {
            let detail = if !stderr.is_empty() {
                stderr
            } else if !result_raw.is_empty() {
                result_raw.clone()
            } else {
                stdout
            };
            format!(
                "虚拟声卡脚本执行失败 (code={:?})。{}",
                output.status.code(),
                detail
            )
        } else {
            humanize_script_result(mode, &result_raw, ready, needs_reboot)
        };
        (ready, message)
    };

    log::info!(
        "XIAOMI VOICE ENV done ready={ready} needs_reboot={needs_reboot} result={result_raw} msg={message}"
    );

    Ok(VoiceEnvActionResult {
        ok: ready || needs_reboot,
        ready,
        needs_choice: false,
        needs_reboot,
        message,
        report_path: None,
    })
}

/// 语音键按下：异步把默认麦设为 CABLE Output，并取消静音、拉满 CABLE 两端音量。
pub fn ensure_cable_mic_for_voice_async() {
    static BUSY: AtomicBool = AtomicBool::new(false);
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);

    {
        let mut last = LAST.lock();
        if let Some(t) = *last {
            if t.elapsed() < Duration::from_secs(8) {
                return;
            }
        }
        *last = Some(Instant::now());
    }
    if BUSY
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }
    std::thread::spawn(|| {
        let result = ensure_cable_mic_for_voice();
        BUSY.store(false, Ordering::SeqCst);
        match result {
            Ok(r) => {
                log::info!("XIAOMI VOICE EnsureMic: {}", r.message);
                // 不在此处 respawn router：会打断正在推流的 CABLE 输出
            }
            Err(e) => log::warn!("XIAOMI VOICE EnsureMic failed: {e}"),
        }
    });
}

/// 当前系统默认捕获端点是否已是 CABLE Output（小米 ATVV 环回路）。
pub fn default_capture_is_cable_output() -> bool {
    #[cfg(target_os = "windows")]
    {
        use cpal::traits::{DeviceTrait, HostTrait};
        let host = cpal::default_host();
        let Some(dev) = host.default_input_device() else {
            return false;
        };
        let Ok(name) = dev.name() else {
            return false;
        };
        let n = name.to_ascii_lowercase();
        n.contains("cable output") || (n.contains("cable") && n.contains("output"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

/// 小米语音唤醒前：若默认麦不是 CABLE（常见于 T1 USB 抢成 Mic Device），同步切回。
/// 已是 CABLE 则跳过，避免无谓 EnsureMic 打断 WASAPI→CABLE 推流。
pub fn ensure_cable_mic_for_ime_before_voice() {
    if default_capture_is_cable_output() {
        log::info!("XIAOMI default capture already CABLE Output — skip EnsureMic");
        return;
    }
    log::info!(
        "XIAOMI default capture is not CABLE Output — EnsureMic sync before voice shortcut"
    );
    match ensure_cable_mic_for_voice() {
        Ok(r) => log::info!("XIAOMI EnsureMic sync: {}", r.message),
        Err(e) => log::warn!("XIAOMI EnsureMic sync failed: {e}"),
    }
}

/// 同步 EnsureMic（测试 / 手动修复）
pub fn ensure_cable_mic_for_voice() -> Result<VoiceEnvActionResult, String> {
    run_configure_script_ex("EnsureMic", None, false)
}

/// T1 USB：异步把默认麦切到 Mic Device（并取消静音、拉满音量）。
/// 小米/BLE 的 EnsureMic 会把默认麦设成 CABLE；USB 路径若不切回，输入法听 CABLE 必无声。
pub fn ensure_usb_mic_for_voice_async() {
    static BUSY: AtomicBool = AtomicBool::new(false);
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);

    {
        let mut last = LAST.lock();
        if let Some(t) = *last {
            if t.elapsed() < Duration::from_secs(5) {
                return;
            }
        }
        *last = Some(Instant::now());
    }
    if BUSY
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }
    std::thread::spawn(|| {
        let result = ensure_usb_mic_for_voice();
        BUSY.store(false, Ordering::SeqCst);
        match result {
            Ok(r) => log::info!("T1 USB EnsureUsbMic: {}", r.message),
            Err(e) => log::warn!("T1 USB EnsureUsbMic failed: {e}"),
        }
    });
}

/// 同步：默认录音设备 → Mic Device
pub fn ensure_usb_mic_for_voice() -> Result<VoiceEnvActionResult, String> {
    run_configure_script_ex("EnsureUsbMic", None, false)
}

/// 检测；若已就绪则直接 Repair（设默认麦）；若未就绪则返回 needs_choice
pub fn check_or_prompt() -> VoiceEnvActionResult {
    let status = voice_env_status_fresh();
    if status.ready {
        match run_configure_script_ex("EnsureMic", None, false) {
            Ok(mut r) => {
                r.needs_choice = false;
                r
            }
            Err(e) => VoiceEnvActionResult {
                ok: false,
                ready: true,
                needs_choice: false,
                needs_reboot: false,
                message: format!("VB-CABLE 已在，但修复默认麦克风失败: {e}"),
                report_path: None,
            },
        }
    } else {
        VoiceEnvActionResult {
            ok: false,
            ready: false,
            needs_choice: true,
            needs_reboot: false,
            message: status.message,
            report_path: None,
        }
    }
}

pub fn install_embedded() -> Result<VoiceEnvActionResult, String> {
    // 用户明确点「官方安装」：始终弹出 Setup（勿因误检已就绪而跳过）
    run_official_setup_installer(true)
}

/// 强制启动官方安装界面（即使已检测到 CABLE），用于驱动异常 / 排障
pub fn install_embedded_force() -> Result<VoiceEnvActionResult, String> {
    run_official_setup_installer(true)
}

pub fn open_download_page() -> Result<VoiceEnvActionResult, String> {
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", "start", "", DOWNLOAD_PAGE_URL])
            .spawn()
            .map_err(|e| format!("打开下载页失败: {e}"))?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        return Err("仅 Windows 支持".into());
    }
    Ok(VoiceEnvActionResult {
        ok: true,
        ready: false,
        needs_choice: false,
        needs_reboot: false,
        message: "已打开 VB-Audio 官网。安装完成后请再点「虚拟声卡修复」。".into(),
        report_path: None,
    })
}

pub fn open_download_zip() -> Result<VoiceEnvActionResult, String> {
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", "start", "", DOWNLOAD_ZIP_URL])
            .spawn()
            .map_err(|e| format!("打开下载链接失败: {e}"))?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        return Err("仅 Windows 支持".into());
    }
    Ok(VoiceEnvActionResult {
        ok: true,
        ready: false,
        needs_choice: false,
        needs_reboot: false,
        message: "已开始下载官方驱动包。解压后运行 VBCABLE_Setup_x64.exe，装完再点「虚拟声卡修复」。".into(),
        report_path: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_setup_name_is_x64_on_64bit() {
        let name = official_setup_exe_name();
        assert!(
            name == "VBCABLE_Setup_x64.exe" || name == "VBCABLE_Setup.exe",
            "unexpected setup name {name}"
        );
        if cfg!(target_pointer_width = "64") {
            assert_eq!(name, "VBCABLE_Setup_x64.exe");
        }
    }

    #[test]
    fn stage_root_prefers_install_or_falls_back() {
        let p = stage_root();
        let s = p.to_string_lossy().to_ascii_lowercase();
        assert!(
            s.ends_with("vb-cable-official-stage") || s.contains("\\vb-cable-official-stage"),
            "unexpected stage_root={p:?}"
        );
        // 开发目录可写时应落在 exe 旁；Program Files 无权限时才是 LocalAppData
        assert!(
            p.file_name().and_then(|n| n.to_str()) == Some(STAGE_DIR_NAME),
            "stage dir name mismatch: {p:?}"
        );
    }
}
