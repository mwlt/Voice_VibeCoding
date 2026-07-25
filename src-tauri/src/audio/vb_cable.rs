//! 小米语音环境：检测 VB-CABLE，并用内嵌驱动包 / 官网下载修复
//!
//! 安装逻辑复用 Python `configure-xiaomi-audio.ps1`（校验签名、提权安装、设默认麦）。

use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::copy;
use std::path::{Path, PathBuf};
use std::process::Command;

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

#[cfg(target_os = "windows")]
fn probe_cable_endpoints() -> (bool, bool) {
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

#[cfg(not(target_os = "windows"))]
fn probe_cable_endpoints() -> (bool, bool) {
    (false, false)
}

pub fn voice_env_status() -> VoiceEnvStatus {
    let (cable_input, cable_output) = probe_cable_endpoints();
    let ready = cable_input && cable_output;
    let zip = find_driver_zip();
    let embedded_available = zip.is_some() && find_configure_script().is_some();
    let message = if ready {
        "VB-CABLE 已就绪。可点「虚拟声卡检测与修复」将默认麦克风设为 CABLE Output。".into()
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

fn desktop_report_path() -> PathBuf {
    let desktop = std::env::var("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("Desktop");
    desktop.join("XiaomiRemoteBridge-audio-check.txt")
}

fn app_path_for_script() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn run_configure_script(mode: &str, zip: &Path) -> Result<VoiceEnvActionResult, String> {
    let script = find_configure_script().ok_or_else(|| "未找到 configure-xiaomi-audio.ps1".to_string())?;
    let app_path = app_path_for_script();
    log::info!(
        "XIAOMI VOICE ENV: run script mode={mode} zip={} app={}",
        zip.display(),
        app_path.display()
    );

    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            &script.display().to_string(),
            "-Mode",
            mode,
            "-AppPath",
            &app_path.display().to_string(),
            "-DriverZipPath",
            &zip.display().to_string(),
        ])
        .output()
        .map_err(|e| format!("启动语音环境脚本失败: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stdout.is_empty() {
        log::info!("XIAOMI VOICE ENV stdout: {stdout}");
    }
    if !stderr.is_empty() {
        log::warn!("XIAOMI VOICE ENV stderr: {stderr}");
    }

    // 脚本多数情况 exit 0，结果写在桌面报告里
    let report = desktop_report_path();
    let report_text = std::fs::read_to_string(&report).unwrap_or_default();
    let needs_reboot = report_text.to_ascii_lowercase().contains("restart required")
        || report_text.contains("需要重启")
        || output.status.code() == Some(3010);
    let warning = report_text
        .lines()
        .find(|l| l.starts_with("Result: WARNING"))
        .map(|l| l.trim_start_matches("Result: ").to_string());

    // 稍等端点出现
    for _ in 0..15 {
        let (i, o) = probe_cable_endpoints();
        if i && o {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    let (cable_input, cable_output) = probe_cable_endpoints();
    let ready = cable_input && cable_output;

    let message = if let Some(w) = warning {
        w
    } else if needs_reboot {
        "驱动已安装，但可能需要重启 Windows 后端点才会出现。重启后再点一次「虚拟声卡检测与修复」。"
            .into()
    } else if ready {
        "语音环境已就绪：VB-CABLE 可用，默认麦克风已尝试设为 CABLE Output。".into()
    } else if !output.status.success() {
        format!(
            "脚本执行失败 (code={:?})。{}",
            output.status.code(),
            if stderr.is_empty() { stdout } else { stderr }
        )
    } else {
        "脚本已执行，但尚未检测到 CABLE Input/Output。若刚装驱动请重启后再试，或改用官网最新包。"
            .into()
    };

    Ok(VoiceEnvActionResult {
        ok: ready || needs_reboot,
        ready,
        needs_choice: false,
        needs_reboot,
        message,
        report_path: if report.is_file() {
            Some(report.display().to_string())
        } else {
            None
        },
    })
}

/// 检测；若已就绪则直接 Repair（设默认麦）；若未就绪则返回 needs_choice
pub fn check_or_prompt() -> VoiceEnvActionResult {
    let status = voice_env_status();
    if status.ready {
        match find_driver_zip() {
            Some(zip) => match run_configure_script("Repair", &zip) {
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
            },
            None => VoiceEnvActionResult {
                ok: true,
                ready: true,
                needs_choice: false,
                needs_reboot: false,
                message: "VB-CABLE 已就绪（无内嵌包，跳过默认麦克风修复脚本）。".into(),
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
    let zip = find_driver_zip().ok_or_else(|| "内嵌 VB-CABLE 驱动包不可用".to_string())?;
    run_configure_script("Repair", &zip)
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
        message: "已打开 VB-Audio 官网。安装完成后请再点「虚拟声卡检测与修复」。".into(),
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
        message: "已开始下载官方驱动包。安装完成后请再点「虚拟声卡检测与修复」。".into(),
        report_path: None,
    })
}
