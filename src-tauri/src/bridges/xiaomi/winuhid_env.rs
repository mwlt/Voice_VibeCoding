//! WinUHid 虚拟键盘环境：检测、部署 DLL、提权安装内嵌驱动包。
//!
//! 豆包/千问等输入法会过滤 SendInput；语音唤醒需要 WinUHid.dll + UMDF 驱动（`\\.\WinUHid`）。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

pub const DOWNLOAD_PAGE_URL: &str =
    "https://gitee.com/mwlt/remote-voice-vibe-coding/releases";

pub fn download_zip_url() -> String {
    let ver = env!("CARGO_PKG_VERSION");
    format!(
        "https://gitee.com/mwlt/remote-voice-vibe-coding/releases/download/v{ver}/WinUHid_Manual_{ver}.zip"
    )
}

pub fn download_zip_filename() -> String {
    format!("WinUHid_Manual_{}.zip", env!("CARGO_PKG_VERSION"))
}

pub const MANUAL_FOLDER_NAME: &str = "Voice VibeCoding WinUHid 手动安装";

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WinUHidEnvStatus {
    pub ready: bool,
    pub dll_found: bool,
    pub dll_path: Option<String>,
    pub driver_ready: bool,
    pub embedded_driver_available: bool,
    pub package_dir: Option<String>,
    pub download_page_url: String,
    pub download_zip_url: String,
    pub message: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WinUHidActionResult {
    pub ok: bool,
    pub ready: bool,
    pub needs_choice: bool,
    pub needs_reboot: bool,
    pub message: String,
    pub export_path: Option<String>,
}

fn desktop_dir() -> Result<PathBuf, String> {
    if let Ok(profile) = std::env::var("USERPROFILE") {
        let desktop = PathBuf::from(profile).join("Desktop");
        if desktop.is_dir() {
            return Ok(desktop);
        }
    }
    std::env::var("USERPROFILE")
        .map(|p| PathBuf::from(p).join("Desktop"))
        .map_err(|_| "无法定位桌面目录".to_string())
}

fn copy_tree(src: &Path, dst: &Path) -> Result<(), String> {
    if !src.is_dir() {
        return Err(format!("源目录不存在: {}", src.display()));
    }
    fs::create_dir_all(dst).map_err(|e| format!("创建目录失败: {e}"))?;
    for entry in fs::read_dir(src).map_err(|e| format!("读取目录失败: {e}"))? {
        let entry = entry.map_err(|e| format!("读取目录项失败: {e}"))?;
        let name = entry.file_name();
        let from = entry.path();
        let to = dst.join(&name);
        if from.is_dir() {
            copy_tree(&from, &to)?;
        } else {
            fs::copy(&from, &to).map_err(|e| format!("复制 {} 失败: {e}", from.display()))?;
        }
    }
    Ok(())
}

pub fn export_manual_package() -> Result<WinUHidActionResult, String> {
    let root = find_asset_root().ok_or_else(|| "内嵌 WinUHid 资源不可用".to_string())?;
    let dest = desktop_dir()?.join(MANUAL_FOLDER_NAME);
    if dest.exists() {
        fs::remove_dir_all(&dest).map_err(|e| format!("清理旧安装包失败: {e}"))?;
    }
    copy_tree(&root, &dest)?;
    open_folder(&dest)?;
    Ok(WinUHidActionResult {
        ok: true,
        ready: false,
        needs_choice: false,
        needs_reboot: false,
        message: format!(
            "已导出 WinUHid 安装包到桌面「{}」。请阅读文件夹内「安装说明.txt」，双击 Run-Install.cmd 安装，完成后回到应用确认「虚拟键盘」状态。",
            MANUAL_FOLDER_NAME
        ),
        export_path: Some(dest.display().to_string()),
    })
}

pub fn open_folder(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(path)
            .spawn()
            .map_err(|e| format!("打开文件夹失败: {e}"))?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
        return Err("仅 Windows 支持".into());
    }
    Ok(())
}

pub fn open_download_page() -> Result<WinUHidActionResult, String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", DOWNLOAD_PAGE_URL])
            .spawn()
            .map_err(|e| format!("打开下载页失败: {e}"))?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        return Err("仅 Windows 支持".into());
    }
    Ok(WinUHidActionResult {
        ok: true,
        ready: false,
        needs_choice: false,
        needs_reboot: false,
        message: "已打开 Release 页。下载 WinUHid_Manual 压缩包，解压后阅读「安装说明.txt」安装。".into(),
        export_path: None,
    })
}

pub fn open_download_zip() -> Result<WinUHidActionResult, String> {
    // 保留给旧调用方；新流程走 download_xiaomi_winuhid_zip（应用内下载 + 进度条）
    Ok(WinUHidActionResult {
        ok: true,
        ready: false,
        needs_choice: false,
        needs_reboot: false,
        message: "请在弹窗中使用「下载驱动包手动安装」，可选择保存位置并查看下载进度。".into(),
        export_path: None,
    })
}

pub fn repair_with_source(source: &str, force: bool) -> Result<WinUHidActionResult, String> {
    match source.to_ascii_lowercase().as_str() {
        "embedded" | "embedded_force" => repair_embedded(force || source.eq_ignore_ascii_case("embedded_force")),
        "export" => export_manual_package(),
        "download_page" => open_download_page(),
        "download_zip" => open_download_zip(),
        other => Err(format!(
            "未知来源: {other}，可选 embedded / embedded_force / export / download_page / download_zip"
        )),
    }
}

fn asset_candidates(relative: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            out.push(dir.join("assets").join("winuhid").join(relative));
            out.push(
                dir.join("resources")
                    .join("assets")
                    .join("winuhid")
                    .join(relative),
            );
            out.push(
                dir.join("_up_")
                    .join("resources")
                    .join("assets")
                    .join("winuhid")
                    .join(relative),
            );
            if let Some(parent) = dir.parent() {
                out.push(
                    parent
                        .join("resources")
                        .join("assets")
                        .join("winuhid")
                        .join(relative),
                );
            }
            // 旁路部署：exe 同目录
            if relative == "WinUHid.dll" {
                out.push(dir.join("WinUHid.dll"));
            }
        }
    }
    if let Some(manifest) = option_env!("CARGO_MANIFEST_DIR") {
        out.push(
            PathBuf::from(manifest)
                .join("assets")
                .join("winuhid")
                .join(relative),
        );
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        if relative == "WinUHid.dll" {
            out.push(
                PathBuf::from(local)
                    .join("com.remote-bridge-hub.app")
                    .join("winuhid")
                    .join("WinUHid.dll"),
            );
        }
    }
    out
}

pub fn find_dll() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("REMOTE_BRIDGE_WINUHID_DLL") {
        let t = p.trim();
        if !t.is_empty() {
            let pb = PathBuf::from(t);
            if pb.is_file() {
                return Some(pb);
            }
        }
    }
    asset_candidates("WinUHid.dll")
        .into_iter()
        .find(|p| p.is_file())
}

pub fn find_driver_package_dir() -> Option<PathBuf> {
    for cand in asset_candidates("WinUHid.dll") {
        let Some(winuhid) = cand.parent() else {
            continue;
        };
        let base = winuhid.join("driver");
        let inf = base.join("WinUHidDriver.inf");
        let dll = base.join("WinUHidDriver.dll");
        let cat = base.join("WinUHidDriver.cat");
        if inf.is_file() && dll.is_file() && cat.is_file() {
            return Some(base);
        }
    }
    None
}

pub fn find_install_script() -> Option<PathBuf> {
    asset_candidates("install-winuhid.ps1")
        .into_iter()
        .find(|p| p.is_file())
}

pub fn find_asset_root() -> Option<PathBuf> {
    find_install_script().and_then(|s| s.parent().map(|p| p.to_path_buf()))
}

/// 把内嵌 WinUHid.dll 拷到 exe 旁与 LocalAppData，便于 LoadLibrary。
pub fn deploy_dll_beside_exe() -> Result<Option<PathBuf>, String> {
    let src = find_dll().ok_or_else(|| "内嵌 WinUHid.dll 不可用".to_string())?;
    let mut deployed = None;
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let dst = dir.join("WinUHid.dll");
            if should_copy(&src, &dst) {
                fs::copy(&src, &dst).map_err(|e| format!("copy WinUHid.dll beside exe: {e}"))?;
                log::info!("WinUHid.dll deployed beside exe: {}", dst.display());
            }
            deployed = Some(dst);
        }
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let dir = PathBuf::from(local)
            .join("com.remote-bridge-hub.app")
            .join("winuhid");
        fs::create_dir_all(&dir).map_err(|e| format!("create winuhid dir: {e}"))?;
        let dst = dir.join("WinUHid.dll");
        if should_copy(&src, &dst) {
            fs::copy(&src, &dst).map_err(|e| format!("copy WinUHid.dll to LocalAppData: {e}"))?;
        }
        if deployed.is_none() {
            deployed = Some(dst);
        }
    }
    Ok(deployed)
}

fn should_copy(src: &Path, dst: &Path) -> bool {
    if !dst.is_file() {
        return true;
    }
    let Ok(sm) = fs::metadata(src) else {
        return false;
    };
    let Ok(dm) = fs::metadata(dst) else {
        return true;
    };
    sm.len() != dm.len()
}

#[cfg(target_os = "windows")]
fn probe_driver_device() -> bool {
    // Test-Path 对 \\.\WinUHid 不可靠；直接尝试打开设备句柄。
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(r"\\.\WinUHid")
        .map(|f| {
            drop(f);
            true
        })
        .unwrap_or(false)
}

#[cfg(not(target_os = "windows"))]
fn probe_driver_device() -> bool {
    false
}

pub fn env_status() -> WinUHidEnvStatus {
    let dll = find_dll();
    let dll_found = dll.is_some();
    let package = find_driver_package_dir();
    let embedded = package.is_some() && find_install_script().is_some();
    // hid_injector 成功打开设备 = 真正可用
    let injector_ok = crate::bridges::xiaomi::hid_injector::is_available();
    let driver_ready = injector_ok || probe_driver_device();
    let ready = injector_ok;
    let message = if ready {
        "虚拟键盘（WinUHid）已就绪，语音键可按硬件方式注入。".into()
    } else if dll_found && !driver_ready && embedded {
        "已找到 WinUHid.dll，但驱动未就绪。请点「修复虚拟键盘」选择自动修复或导出安装包。".into()
    } else if !dll_found && embedded {
        "未找到 WinUHid.dll。请点「修复虚拟键盘」自动部署或导出安装包。".into()
    } else if dll_found && driver_ready && !injector_ok {
        "驱动设备可访问，但注入器未打开。可再点一次「修复虚拟键盘」或重启桥接。".into()
    } else {
        "虚拟键盘环境不可用：豆包/千问等输入法会忽略普通模拟按键，语音唤醒需要 WinUHid。".into()
    };
    WinUHidEnvStatus {
        ready,
        dll_found,
        dll_path: dll.map(|p| p.display().to_string()),
        driver_ready,
        embedded_driver_available: embedded,
        package_dir: package.map(|p| p.display().to_string()),
        download_page_url: DOWNLOAD_PAGE_URL.into(),
        download_zip_url: download_zip_url(),
        message,
    }
}

fn script_result_line(text: &str) -> Option<&str> {
    text.lines()
        .find_map(|l| l.trim().strip_prefix("Result: ").map(str::trim))
}

fn script_last_phase(text: &str) -> Option<String> {
    text.lines()
        .filter_map(|l| {
            let t = l.trim();
            t.strip_prefix("Phase: ").map(str::trim)
        })
        .map(str::to_string)
        .last()
}

fn script_error_phase(text: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let t = l.trim();
        if t.starts_with("Phase: Error |") {
            Some(t.trim_start_matches("Phase: Error |").trim().to_string())
        } else {
            None
        }
    })
}

fn format_repair_failure(output: &std::process::Output, result_raw: &str) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if let Some(err) = script_error_phase(&stdout) {
        return format!("虚拟键盘修复失败：{err}");
    }
    if let Some(phase) = script_last_phase(&stdout) {
        return format!(
            "虚拟键盘修复未完成 (code={:?})。最后阶段：{phase}",
            output.status.code()
        );
    }
    let detail = if !stderr.is_empty() {
        stderr.trim().to_string()
    } else if !result_raw.is_empty() {
        result_raw.to_string()
    } else {
        stdout.trim().to_string()
    };
    format!(
        "虚拟键盘修复未完成 (code={:?})。{detail}",
        output.status.code()
    )
}

/// 启动时尽力部署 DLL 并尝试打开注入器（不弹 UAC）。
pub fn ensure_runtime_quiet() {
    match deploy_dll_beside_exe() {
        Ok(Some(p)) => {
            if let Ok(s) = p.into_os_string().into_string() {
                std::env::set_var("REMOTE_BRIDGE_WINUHID_DLL", s);
            }
        }
        Ok(None) => {}
        Err(e) => log::warn!("WinUHid DLL deploy skipped: {e}"),
    }
    crate::bridges::xiaomi::hid_injector::reset_and_retry();
    if crate::bridges::xiaomi::hid_injector::is_available() {
        log::info!("WinUHid runtime ready");
    } else {
        log::warn!(
            "WinUHid not ready at startup — voice IME wake needs「修复虚拟键盘」: {}",
            env_status().message
        );
    }
}

pub fn repair_embedded(force: bool) -> Result<WinUHidActionResult, String> {
    let _ = deploy_dll_beside_exe()?;
    let package = find_driver_package_dir()
        .ok_or_else(|| "内嵌 WinUHid 驱动包不可用（缺少 driver/ 下的 inf/dll/cat）".to_string())?;
    let script = find_install_script().ok_or_else(|| "未找到 install-winuhid.ps1".to_string())?;
    let dll = find_dll().ok_or_else(|| "未找到 WinUHid.dll".to_string())?;

    log::info!(
        "WinUHid repair: force={force} script={} package={} dll={}",
        script.display(),
        package.display(),
        dll.display()
    );

    let mut cmd = Command::new("powershell.exe");
    let mut args = vec![
        "-NoProfile".to_string(),
        "-WindowStyle".to_string(),
        "Hidden".to_string(),
        "-ExecutionPolicy".to_string(),
        "Bypass".to_string(),
        "-File".to_string(),
        script.display().to_string(),
        "-Mode".to_string(),
        "Install".to_string(),
        "-PackageDir".to_string(),
        package.display().to_string(),
        "-DllSource".to_string(),
        dll.display().to_string(),
    ];
    if force {
        args.push("-Force".to_string());
    }
    cmd.args(args);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let output = cmd
        .output()
        .map_err(|e| format!("启动 WinUHid 安装脚本失败: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stdout.is_empty() {
        log::info!("WinUHid install stdout:\n{stdout}");
    }
    if !stderr.is_empty() {
        log::warn!("WinUHid install stderr:\n{stderr}");
    }

    let result_raw = script_result_line(&stdout).unwrap_or("").to_string();
    let needs_reboot = result_raw.to_ascii_lowercase().contains("restart required")
        || result_raw.contains("需要重启")
        || output.status.code() == Some(3010);

    // 安装后重试打开设备
    crate::bridges::xiaomi::hid_injector::reset_and_retry();
    for _ in 0..20 {
        if crate::bridges::xiaomi::hid_injector::is_available() {
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
        crate::bridges::xiaomi::hid_injector::reset_and_retry();
    }
    let ready = crate::bridges::xiaomi::hid_injector::is_available();

    let message = if ready {
        "虚拟键盘已就绪：WinUHid 驱动可用，语音键将按硬件方式注入。".into()
    } else if needs_reboot {
        "驱动已安装，必须重启 Windows 后虚拟键盘才会生效。重启后再点一次「修复虚拟键盘」。".into()
    } else if !output.status.success() {
        format_repair_failure(&output, &result_raw)
    } else {
        format!(
            "脚本已执行，但 WinUHid 仍不可用。{} 可查看日志或重启后再试。",
            if result_raw.is_empty() {
                String::new()
            } else {
                format!("({result_raw})")
            }
        )
    };

    let needs_choice = !ready && !needs_reboot;

    Ok(WinUHidActionResult {
        ok: ready || needs_reboot,
        ready,
        needs_choice,
        needs_reboot,
        message,
        export_path: None,
    })
}

pub fn check_or_repair() -> WinUHidActionResult {
    let status = env_status();
    if status.ready {
        return WinUHidActionResult {
            ok: true,
            ready: true,
            needs_choice: false,
            needs_reboot: false,
            message: status.message,
            export_path: None,
        };
    }
    match repair_embedded(false) {
        Ok(mut r) => {
            if !r.ready && !r.needs_reboot && !r.ok {
                r.needs_choice = true;
                r.message = format!(
                    "{} 可在「修复虚拟键盘」弹窗中导出安装包或从 Release 下载。",
                    r.message
                );
            }
            r
        }
        Err(e) => WinUHidActionResult {
            ok: false,
            ready: false,
            needs_choice: true,
            needs_reboot: false,
            message: format!("{e} 请点「修复虚拟键盘」导出安装包或选择其他方式。"),
            export_path: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_script_result_line() {
        let text = "Phase: Verify | ok\nResult: OK\n";
        assert_eq!(script_result_line(text), Some("OK"));
    }

    #[test]
    fn parse_last_phase_and_error() {
        let text = "Phase: BindDriver | exit=0 OK\nPhase: Error | pnputil failed\n";
        assert_eq!(
            script_last_phase(text).as_deref(),
            Some("Error | pnputil failed")
        );
        assert_eq!(
            script_error_phase(text).as_deref(),
            Some("pnputil failed")
        );
    }
}
