//! 轻量更新检查：读取公开 update/latest.json（Gitee raw 优先，失败再 GitHub raw）

use crate::config::manager::ConfigManager;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

const GITEE_RAW: &str =
    "https://gitee.com/mwlt/remote-voice-vibe-coding/raw/main/update/latest.json";
const GITHUB_RAW: &str =
    "https://raw.githubusercontent.com/mwlt/Voice_VibeCoding/main/update/latest.json";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LatestManifest {
    version: String,
    #[serde(default)]
    notes: String,
    gitee_page: String,
    github_page: String,
    gitee_setup_url: String,
    github_setup_url: String,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckResult {
    pub checked: bool,
    /// semver 上确有新版本（可下载；设置里「检查更新」据此弹窗）
    pub has_newer_version: bool,
    /// 用户已忽略该版本的自动提醒
    pub prompt_suppressed: bool,
    /// 兼容字段：与 has_newer_version 相同
    pub update_available: bool,
    /// 兼容字段：与 prompt_suppressed 相同（仅有新版本时才有意义）
    pub ignored: bool,
    pub current_version: String,
    pub latest_version: String,
    pub notes: String,
    pub gitee_page: String,
    pub github_page: String,
    /// 检测成功侧的安装包/发行页直链（「直接下载」用）
    pub setup_url: String,
    pub source: String,
    pub error: Option<String>,
}

static LAST_RESULT: Mutex<Option<UpdateCheckResult>> = Mutex::new(None);
static DOWNLOADING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgressPayload {
    pub downloaded: u64,
    pub total: Option<u64>,
    pub percent: Option<f32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadCompletePayload {
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadErrorPayload {
    pub message: String,
}

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub fn last_result() -> UpdateCheckResult {
    LAST_RESULT.lock().clone().unwrap_or_else(|| UpdateCheckResult {
        checked: false,
        current_version: current_version().into(),
        ..Default::default()
    })
}

fn normalize_version(raw: &str) -> String {
    raw.trim().trim_start_matches(['v', 'V']).trim().to_string()
}

/// 返回 Some(true) 若 remote > local
fn is_newer(remote: &str, local: &str) -> bool {
    let r = parse_semver(remote);
    let l = parse_semver(local);
    r > l
}

fn parse_semver(s: &str) -> (u64, u64, u64) {
    let s = normalize_version(s);
    let mut parts = s.split('.').filter_map(|p| {
        let digits: String = p.chars().take_while(|c| c.is_ascii_digit()).collect();
        digits.parse::<u64>().ok()
    });
    (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    )
}

/// 纯函数：根据当前版本、远端版本、已忽略版本，判定是否有新版本及是否应抑制被动提醒。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpdateEvaluation {
    pub has_newer_version: bool,
    pub prompt_suppressed: bool,
}

pub fn evaluate_update(
    current: &str,
    latest: &str,
    ignored_version: Option<&str>,
) -> UpdateEvaluation {
    let latest_norm = normalize_version(latest);
    let newer = is_newer(&latest_norm, current);
    let prompt_suppressed = newer
        && ignored_version
            .map(|v| normalize_version(v) == latest_norm)
            .unwrap_or(false);
    UpdateEvaluation {
        has_newer_version: newer,
        prompt_suppressed,
    }
}

/// 是否应向用户发出被动更新提醒（启动检测、顶栏角标、自动弹窗）。
pub fn should_emit_passive_prompt(result: &UpdateCheckResult) -> bool {
    result.has_newer_version && !result.prompt_suppressed
}

fn fetch_manifest(url: &str) -> Result<LatestManifest, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(8))
        .timeout_read(Duration::from_secs(12))
        .build();
    let resp = agent
        .get(url)
        .set("User-Agent", "VoiceVibeCoding-UpdateCheck/1.0")
        .call()
        .map_err(|e| format!("请求失败: {e}"))?;
    if !(200..300).contains(&resp.status()) {
        return Err(format!("HTTP {}", resp.status()));
    }
    resp.into_json::<LatestManifest>()
        .map_err(|e| format!("解析 latest.json 失败: {e}"))
}

fn build_result(
    manifest: LatestManifest,
    source: &str,
    ignored_version: Option<&str>,
) -> UpdateCheckResult {
    let current = current_version().to_string();
    let latest = normalize_version(&manifest.version);
    let eval = evaluate_update(&current, &latest, ignored_version);
    let setup_url = if source == "gitee" {
        manifest.gitee_setup_url.clone()
    } else {
        manifest.github_setup_url.clone()
    };
    UpdateCheckResult {
        checked: true,
        has_newer_version: eval.has_newer_version,
        prompt_suppressed: eval.prompt_suppressed,
        update_available: eval.has_newer_version,
        ignored: eval.prompt_suppressed,
        current_version: current,
        latest_version: latest,
        notes: manifest.notes,
        gitee_page: manifest.gitee_page,
        github_page: manifest.github_page,
        setup_url,
        source: source.into(),
        error: None,
    }
}

/// Gitee raw 优先，失败再 GitHub raw
pub fn check_for_update(config: &ConfigManager) -> UpdateCheckResult {
    let ignored = config
        .get_global_settings()
        .ok()
        .and_then(|s| s.ignored_update_version);

    let mut errors = Vec::new();
    match fetch_manifest(GITEE_RAW) {
        Ok(m) => {
            let r = build_result(m, "gitee", ignored.as_deref());
            *LAST_RESULT.lock() = Some(r.clone());
            log::info!(
                "UPDATE check via gitee: current={} latest={} newer={} suppressed={}",
                r.current_version,
                r.latest_version,
                r.has_newer_version,
                r.prompt_suppressed
            );
            return r;
        }
        Err(e) => {
            log::warn!("UPDATE check gitee failed: {e}");
            errors.push(format!("Gitee: {e}"));
        }
    }

    match fetch_manifest(GITHUB_RAW) {
        Ok(m) => {
            let r = build_result(m, "github", ignored.as_deref());
            *LAST_RESULT.lock() = Some(r.clone());
            log::info!(
                "UPDATE check via github: current={} latest={} newer={} suppressed={}",
                r.current_version,
                r.latest_version,
                r.has_newer_version,
                r.prompt_suppressed
            );
            return r;
        }
        Err(e) => {
            log::warn!("UPDATE check github failed: {e}");
            errors.push(format!("GitHub: {e}"));
        }
    }

    let r = UpdateCheckResult {
        checked: true,
        current_version: current_version().into(),
        error: Some(errors.join("；")),
        ..Default::default()
    };
    *LAST_RESULT.lock() = Some(r.clone());
    r
}

pub fn ignore_version(config: &ConfigManager, version: &str) -> Result<UpdateCheckResult, String> {
    let mut settings = config.get_global_settings()?;
    settings.ignored_update_version = Some(normalize_version(version));
    config.save_global_settings(&settings)?;
    if let Some(mut last) = LAST_RESULT.lock().clone() {
        let latest = normalize_version(&last.latest_version);
        if latest == normalize_version(version) {
            last.prompt_suppressed = true;
            last.ignored = true;
            *LAST_RESULT.lock() = Some(last.clone());
            return Ok(last);
        }
    }
    Ok(check_for_update(config))
}

pub fn emit_if_available(app: &AppHandle, result: &UpdateCheckResult) {
    if should_emit_passive_prompt(result) {
        let _ = app.emit("app-update-available", result);
    }
}

pub fn spawn_startup_check(app: AppHandle) {
    std::thread::Builder::new()
        .name("app-update-check".into())
        .spawn(move || {
            // 推迟网络检测，避免与 BLE/WinUHid/语音路由冷启动抢资源；
            // 前端还会再延迟自动弹窗（APP_UPDATE_AUTO_OPEN_DELAY_MS）。
            std::thread::sleep(Duration::from_secs(10));
            let Some(config) = app.try_state::<ConfigManager>() else {
                return;
            };
            let result = check_for_update(config.inner());
            emit_if_available(&app, &result);
        })
        .ok();
}

fn download_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(30))
        .timeout_read(Duration::from_secs(3600))
        .build()
}

fn setup_filename(version: &str) -> String {
    format!("Voice VibeCoding_{}_x64-setup.exe", normalize_version(version))
}

fn emit_download_progress(app: &AppHandle, downloaded: u64, total: Option<u64>) {
    let percent = total
        .filter(|t| *t > 0)
        .map(|t| ((downloaded as f64 / t as f64) * 100.0).min(100.0) as f32);
    let _ = app.emit(
        "app-update-download-progress",
        DownloadProgressPayload {
            downloaded,
            total,
            percent,
        },
    );
}

fn download_setup(app: &AppHandle, url: &str, dest: &Path) -> Result<(), String> {
    let agent = download_agent();
    let resp = agent
        .get(url)
        .set("User-Agent", "VoiceVibeCoding-UpdateDownload/1.0")
        .call()
        .map_err(|e| format!("下载请求失败: {e}"))?;
    if !(200..300).contains(&resp.status()) {
        return Err(format!("下载失败: HTTP {}", resp.status()));
    }

    let total = resp
        .header("Content-Length")
        .and_then(|s| s.parse::<u64>().ok());

    let mut reader = resp.into_reader();
    let mut file = std::fs::File::create(dest)
        .map_err(|e| format!("无法写入 {}: {e}", dest.display()))?;

    let mut downloaded = 0u64;
    let mut buf = [0u8; 64 * 1024];
    let mut last_emit = Instant::now();

    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("读取下载数据失败: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| format!("写入安装包失败: {e}"))?;
        downloaded += n as u64;

        if last_emit.elapsed() >= Duration::from_millis(120) {
            emit_download_progress(app, downloaded, total);
            last_emit = Instant::now();
        }
    }

    emit_download_progress(app, downloaded, total);
    Ok(())
}

/// 从 UninstallString（可能带引号/参数）解析 uninstall.exe 路径。
pub fn parse_uninstall_exe_path(uninstall_string: &str) -> Option<PathBuf> {
    let s = uninstall_string.trim();
    if s.is_empty() {
        return None;
    }
    let path = if s.starts_with('"') {
        let rest = &s[1..];
        let end = rest.find('"')?;
        rest[..end].to_string()
    } else {
        s.split_whitespace().next()?.to_string()
    };
    let p = PathBuf::from(path);
    let name = p.file_name()?.to_string_lossy();
    if name.eq_ignore_ascii_case("uninstall.exe")
        || p.extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
    {
        Some(p)
    } else {
        None
    }
}

const PRODUCT_DISPLAY_NAME: &str = "Voice VibeCoding";

#[cfg(target_os = "windows")]
fn lookup_uninstall_string_in_hive(hive: winreg::HKEY, subkey: &str) -> Option<String> {
    use winreg::enums::KEY_READ;
    use winreg::RegKey;
    let root = RegKey::predef(hive);
    let uninstall = root.open_subkey_with_flags(subkey, KEY_READ).ok()?;
    for name in uninstall.enum_keys().filter_map(|k| k.ok()) {
        let key = uninstall.open_subkey_with_flags(&name, KEY_READ).ok()?;
        let display: String = key.get_value("DisplayName").ok()?;
        if display.trim() != PRODUCT_DISPLAY_NAME {
            continue;
        }
        let uninstall_string: String = key.get_value("UninstallString").ok()?;
        if !uninstall_string.trim().is_empty() {
            return Some(uninstall_string);
        }
    }
    None
}

#[cfg(target_os = "windows")]
pub fn find_installed_uninstall_exe() -> Option<PathBuf> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    const UNINSTALL: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";
    const UNINSTALL_WOW: &str =
        r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall";
    for (hive, sub) in [
        (HKEY_CURRENT_USER, UNINSTALL),
        (HKEY_LOCAL_MACHINE, UNINSTALL),
        (HKEY_LOCAL_MACHINE, UNINSTALL_WOW),
    ] {
        if let Some(s) = lookup_uninstall_string_in_hive(hive, sub) {
            if let Some(p) = parse_uninstall_exe_path(&s) {
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }
    None
}

#[cfg(not(target_os = "windows"))]
pub fn find_installed_uninstall_exe() -> Option<PathBuf> {
    None
}

#[cfg(target_os = "windows")]
fn launch_silent_upgrade(setup: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let uninstall = find_installed_uninstall_exe();
    let pid = std::process::id();
    let script = build_silent_upgrade_ps1(pid, uninstall.as_deref(), setup);
    let ps_path = std::env::temp_dir().join(format!(
        "voice_vibecoding_silent_upgrade_{pid}.ps1"
    ));
    // UTF-8 BOM：避免中文进度文案在 powershell -File 下乱码
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(script.as_bytes());
    std::fs::write(&ps_path, bytes).map_err(|e| format!("写入升级脚本失败: {e}"))?;

    let args = silent_upgrade_powershell_args(&ps_path);
    std::process::Command::new("powershell.exe")
        .args(&args)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("启动静默升级失败: {e}"))?;
    log::info!(
        "UPDATE silent upgrade scheduled setup={} uninstall={} ps1={}",
        setup.display(),
        uninstall
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(none)".into()),
        ps_path.display()
    );
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn launch_silent_upgrade(setup: &Path) -> Result<(), String> {
    let _ = setup;
    Err("仅支持 Windows 安装包".into())
}

#[cfg(target_os = "windows")]
fn launch_installer(path: &Path) -> Result<(), String> {
    launch_silent_upgrade(path)
}

#[cfg(not(target_os = "windows"))]
fn launch_installer(path: &Path) -> Result<(), String> {
    launch_silent_upgrade(path)
}

/// Tauri NSIS：静默安装 + 装完自动运行主程序。
pub fn silent_install_args() -> [&'static str; 2] {
    ["/S", "/R"]
}

/// Tauri NSIS：静默卸载且保留用户数据（AppData）。
pub fn silent_uninstall_keep_data_args() -> [&'static str; 2] {
    ["/S", "/UPDATE"]
}

/// 启动升级脚本用的 powershell.exe 参数（无窗口）。
pub fn silent_upgrade_powershell_args(script_path: &Path) -> Vec<String> {
    vec![
        "-NoProfile".into(),
        "-WindowStyle".into(),
        "Hidden".into(),
        "-ExecutionPolicy".into(),
        "Bypass".into(),
        "-File".into(),
        script_path.display().to_string(),
    ]
}

fn ps_single_quote(path: &Path) -> String {
    let s = path.display().to_string().replace('\'', "''");
    format!("'{s}'")
}

/// 生成静默升级 PowerShell：等旧进程退出 → 清残留 →（可选）卸旧 → 装新并 `/R`。
/// 子进程 Hidden；仅显示一个无按钮的进度窗（非 MessageBox）。
pub fn build_silent_upgrade_ps1(
    wait_pid: u32,
    uninstall_exe: Option<&Path>,
    setup_exe: &Path,
) -> String {
    let setup_q = ps_single_quote(setup_exe);
    let uninstall_block = if let Some(un) = uninstall_exe {
        let un_q = ps_single_quote(un);
        format!(
            r#"
$Uninstall = {un_q}
if (Test-Path -LiteralPath $Uninstall) {{
  Set-ProgressText '正在卸载旧版本（保留配置）…'
  [void](Invoke-HiddenProcess -FilePath $Uninstall -ArgumentList @('/S','/UPDATE'))
  Start-Sleep -Milliseconds 800
}}
"#
        )
    } else {
        String::new()
    };

    format!(
        r#"$ErrorActionPreference = 'Continue'
$WaitPid = {wait_pid}
$Setup = {setup_q}
$Log = Join-Path $env:TEMP 'voice_vibecoding_silent_upgrade.log'
function Write-UpgradeLog([string]$Message) {{
  $line = '[{{0}}] {{1}}' -f (Get-Date -Format 'o'), $Message
  Add-Content -LiteralPath $Log -Value $line -Encoding UTF8
}}
Write-UpgradeLog ("begin wait_pid=" + $WaitPid + " setup=" + $Setup)

try {{ Wait-Process -Id $WaitPid -ErrorAction SilentlyContinue }} catch {{ }}
Start-Sleep -Seconds 2
Get-Process -Name 'remote-bridge-hub' -ErrorAction SilentlyContinue | ForEach-Object {{
  Write-UpgradeLog ("stop leftover pid=" + $_.Id)
  Stop-Process -Id $_.Id -Force -ErrorAction SilentlyContinue
}}
Start-Sleep -Seconds 1

Add-Type -AssemblyName System.Windows.Forms | Out-Null
Add-Type -AssemblyName System.Drawing | Out-Null
$form = New-Object System.Windows.Forms.Form
$form.Text = 'Voice VibeCoding'
$form.StartPosition = 'CenterScreen'
$form.FormBorderStyle = 'FixedDialog'
$form.MaximizeBox = $false
$form.MinimizeBox = $false
$form.ControlBox = $false
$form.TopMost = $true
$form.ShowInTaskbar = $true
$form.ClientSize = New-Object System.Drawing.Size(380, 110)
$label = New-Object System.Windows.Forms.Label
$label.AutoSize = $false
$label.SetBounds(16, 16, 348, 28)
$label.Text = '正在升级，请稍候…'
$label.Font = New-Object System.Drawing.Font('Microsoft YaHei UI', 10)
$bar = New-Object System.Windows.Forms.ProgressBar
$bar.Style = 'Marquee'
$bar.MarqueeAnimationSpeed = 30
$bar.SetBounds(16, 56, 348, 22)
$form.Controls.Add($label)
$form.Controls.Add($bar)
$form.Show()
[System.Windows.Forms.Application]::DoEvents()

function Set-ProgressText([string]$Text) {{
  $label.Text = $Text
  [System.Windows.Forms.Application]::DoEvents()
}}

function Invoke-HiddenProcess {{
  param([string]$FilePath, [string[]]$ArgumentList)
  Write-UpgradeLog ("run " + $FilePath + " " + ($ArgumentList -join ' '))
  $p = Start-Process -FilePath $FilePath -ArgumentList $ArgumentList -WindowStyle Hidden -PassThru
  while ($null -ne $p -and -not $p.HasExited) {{
    [System.Windows.Forms.Application]::DoEvents()
    Start-Sleep -Milliseconds 200
  }}
  $code = if ($null -eq $p) {{ -1 }} else {{ $p.ExitCode }}
  Write-UpgradeLog ("exit_code=" + $code)
  return $code
}}
{uninstall_block}
Set-ProgressText '正在安装新版本…'
$installCode = Invoke-HiddenProcess -FilePath $Setup -ArgumentList @('/S','/R')
Start-Sleep -Seconds 1
$exe = Join-Path $env:ProgramFiles 'Voice VibeCoding\remote-bridge-hub.exe'
if (-not (Get-Process -Name 'remote-bridge-hub' -ErrorAction SilentlyContinue)) {{
  if (Test-Path -LiteralPath $exe) {{
    Write-UpgradeLog 'fallback launch after install'
    Start-Process -FilePath $exe
  }}
}}
Set-ProgressText '升级完成'
[System.Windows.Forms.Application]::DoEvents()
Start-Sleep -Milliseconds 600
$form.Close()
Write-UpgradeLog ("done install_code=" + $installCode)
"#
    )
}

/// 兼容旧测试名：现改为生成 PowerShell 脚本内容。
pub fn build_silent_upgrade_batch(
    wait_pid: u32,
    uninstall_exe: Option<&Path>,
    setup_exe: &Path,
) -> String {
    build_silent_upgrade_ps1(wait_pid, uninstall_exe, setup_exe)
}

pub fn spawn_download(
    app: AppHandle,
    config: &ConfigManager,
    url: String,
    version: String,
) -> Result<(), String> {
    if url.trim().is_empty() {
        return Err("安装包地址为空".into());
    }
    if DOWNLOADING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("已有下载任务进行中".into());
    }

    let dir: PathBuf = config.config_dir().join("updates");
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建下载目录失败: {e}"))?;
    let dest = dir.join(setup_filename(&version));
    if dest.exists() {
        let _ = std::fs::remove_file(&dest);
    }

    let app_bg = app.clone();
    std::thread::Builder::new()
        .name("app-update-download".into())
        .spawn(move || {
            let result = download_setup(&app_bg, &url, &dest);
            DOWNLOADING.store(false, Ordering::SeqCst);

            match result {
                Ok(()) => match launch_installer(&dest) {
                    Ok(()) => {
                        log::info!(
                            "UPDATE download complete, silent upgrade scheduled: {}",
                            dest.display()
                        );
                        let _ = app_bg.emit(
                            "app-update-download-complete",
                            DownloadCompletePayload {
                                path: dest.display().to_string(),
                            },
                        );
                        // 批处理会等本进程退出后再卸旧/装新
                        app_bg.exit(0);
                    }
                    Err(e) => {
                        log::warn!("UPDATE installer launch failed: {e}");
                        let _ = app_bg.emit(
                            "app-update-download-error",
                            DownloadErrorPayload { message: e },
                        );
                    }
                },
                Err(e) => {
                    log::warn!("UPDATE download failed: {e}");
                    let _ = std::fs::remove_file(&dest);
                    let _ = app_bg.emit(
                        "app-update-download-error",
                        DownloadErrorPayload { message: e },
                    );
                }
            }
        })
        .map_err(|e| {
            DOWNLOADING.store(false, Ordering::SeqCst);
            format!("启动下载线程失败: {e}")
        })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_version_detect() {
        assert!(is_newer("1.3.7", "1.3.6"));
        assert!(!is_newer("1.3.6", "1.3.6"));
        assert!(!is_newer("1.3.5", "1.3.6"));
        assert!(is_newer("v2.0.0", "1.9.9"));
    }

    #[test]
    fn evaluate_update_newer_not_ignored() {
        let e = evaluate_update("1.5.2", "1.5.3", None);
        assert!(e.has_newer_version);
        assert!(!e.prompt_suppressed);
    }

    #[test]
    fn evaluate_update_newer_ignored_same_version() {
        let e = evaluate_update("1.5.2", "1.5.3", Some("1.5.3"));
        assert!(e.has_newer_version);
        assert!(e.prompt_suppressed);
    }

    #[test]
    fn evaluate_update_newer_ignored_different_version() {
        let e = evaluate_update("1.5.2", "1.5.4", Some("1.5.3"));
        assert!(e.has_newer_version);
        assert!(!e.prompt_suppressed);
    }

    #[test]
    fn evaluate_update_not_newer() {
        let e = evaluate_update("1.5.3", "1.5.3", Some("1.5.3"));
        assert!(!e.has_newer_version);
        assert!(!e.prompt_suppressed);
    }

    #[test]
    fn passive_prompt_emits_only_when_not_suppressed() {
        let active = UpdateCheckResult {
            has_newer_version: true,
            prompt_suppressed: false,
            update_available: true,
            ..Default::default()
        };
        let suppressed = UpdateCheckResult {
            has_newer_version: true,
            prompt_suppressed: true,
            update_available: true,
            ignored: true,
            ..Default::default()
        };
        assert!(should_emit_passive_prompt(&active));
        assert!(!should_emit_passive_prompt(&suppressed));
    }

    #[test]
    fn silent_install_args_are_s_and_r() {
        // /S 静默；/R 装完自动打开（Tauri NSIS .onInstSuccess）
        assert_eq!(silent_install_args(), ["/S", "/R"]);
    }

    #[test]
    fn silent_uninstall_keep_data_args_are_s_and_update() {
        // /S 静默卸；/UPDATE 保留 AppData（不删用户配置）
        assert_eq!(silent_uninstall_keep_data_args(), ["/S", "/UPDATE"]);
    }

    #[test]
    fn upgrade_ps1_waits_then_uninstalls_then_installs() {
        let script = build_silent_upgrade_ps1(
            4242,
            Some(Path::new(r"C:\Program Files\Voice VibeCoding\uninstall.exe")),
            Path::new(r"C:\Users\me\updates\VoiceVibeCoding_1.6.2_x64-setup.exe"),
        );
        assert!(script.contains("4242"), "must wait for old pid");
        assert!(script.contains("Wait-Process"));
        assert!(!script.to_ascii_lowercase().contains("timeout"));
        assert!(!script.to_ascii_lowercase().contains("start /wait"));
        assert!(
            script.contains(r"C:\Program Files\Voice VibeCoding\uninstall.exe"),
            "uninstall path: {script}"
        );
        assert!(script.contains("/UPDATE"));
        assert!(
            script.contains(r"C:\Users\me\updates\VoiceVibeCoding_1.6.2_x64-setup.exe"),
            "setup path: {script}"
        );
        assert!(script.contains("/R"));
        assert!(script.contains("Hidden"));
        let un_pos = script.find("/UPDATE").expect("uninstall args");
        let in_pos = script.find("/R").expect("install args");
        assert!(un_pos < in_pos, "uninstall must run before install");
    }

    #[test]
    fn upgrade_ps1_skips_uninstall_when_absent() {
        let script = build_silent_upgrade_ps1(7, None, Path::new(r"D:\setup.exe"));
        assert!(!script.to_ascii_lowercase().contains("uninstall.exe"));
        assert!(script.contains(r"D:\setup.exe"));
    }

    #[test]
    fn powershell_launch_args_hidden() {
        let args = silent_upgrade_powershell_args(Path::new(r"C:\Temp\u.ps1"));
        assert_eq!(
            args,
            vec![
                "-NoProfile",
                "-WindowStyle",
                "Hidden",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                r"C:\Temp\u.ps1",
            ]
        );
    }
}
