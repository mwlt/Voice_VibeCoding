//! 静默升级引导：供旧版客户端（启动安装包时不带 /S）下载后直接运行。
//! 流程：结束旧进程 → uninstall /S /UPDATE → 下载正式 NSIS → /S /R 安装并启动。

#![cfg_attr(windows, windows_subsystem = "windows")]

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

const PRODUCT: &str = "Voice VibeCoding";
const APP_EXE: &str = "remote-bridge-hub.exe";

const SETUP_URLS: &[&str] = &[
    "https://gitee.com/mwlt/remote-voice-vibe-coding/releases/download/v1.6.2/Voice%20VibeCoding_1.6.2_x64-setup.exe",
    "https://github.com/mwlt/Voice_VibeCoding/releases/download/v1.6.2/Voice.VibeCoding_1.6.2_x64-setup.exe",
];

fn main() {
    // 非阻塞提示，不挡住升级
    std::thread::spawn(|| {
        show_message(
            "正在升级",
            "Voice VibeCoding 正在静默升级，请稍候。\n完成后将自动打开（可能只需确认一次 UAC）。",
        );
    });
    std::thread::sleep(Duration::from_millis(300));

    if let Err(e) = run() {
        show_message(
            "升级失败",
            &format!("{e}\n\n请到发行页手动下载安装包，或关闭本软件后重试。"),
        );
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let _ = Command::new("taskkill")
        .args(["/IM", APP_EXE, "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    std::thread::sleep(Duration::from_secs(1));

    if let Some(un) = find_uninstall_exe() {
        log_line(&format!("uninstall {}", un.display()));
        let status = Command::new(&un)
            .args(["/S", "/UPDATE"])
            .status()
            .map_err(|e| format!("启动卸载失败: {e}"))?;
        log_line(&format!("uninstall exit={status:?}"));
        std::thread::sleep(Duration::from_secs(1));
    } else {
        log_line("uninstall.exe not found, skip");
    }

    let setup = download_setup()?;
    log_line(&format!("setup {}", setup.display()));
    let status = Command::new(&setup)
        .args(["/S", "/R"])
        .status()
        .map_err(|e| format!("启动静默安装失败: {e}"))?;
    if !status.success() {
        return Err(format!("安装程序退出码异常: {status:?}"));
    }
    Ok(())
}

fn download_setup() -> Result<PathBuf, String> {
    let dest = std::env::temp_dir().join("VoiceVibeCoding_1.6.2_x64-setup.exe");
    if dest.exists() {
        let _ = std::fs::remove_file(&dest);
    }

    let mut last_err = String::new();
    for url in SETUP_URLS {
        match download_url(url, &dest) {
            Ok(()) => return Ok(dest),
            Err(e) => {
                last_err = format!("{url}: {e}");
                let _ = std::fs::remove_file(&dest);
            }
        }
    }
    Err(format!("下载安装包失败: {last_err}"))
}

fn download_url(url: &str, dest: &Path) -> Result<(), String> {
    let resp = ureq::get(url)
        .set("User-Agent", "VoiceVibeCoding-SilentUpdater/1.6.2")
        .timeout(Duration::from_secs(600))
        .call()
        .map_err(|e| e.to_string())?;
    if !(200..300).contains(&resp.status()) {
        return Err(format!("HTTP {}", resp.status()));
    }
    let mut reader = resp.into_reader();
    let mut file = File::create(dest).map_err(|e| e.to_string())?;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
    }
    file.flush().map_err(|e| e.to_string())?;
    let meta = std::fs::metadata(dest).map_err(|e| e.to_string())?;
    if meta.len() < 1_000_000 {
        return Err(format!("文件过小 ({} bytes)，可能不是安装包", meta.len()));
    }
    Ok(())
}

fn find_uninstall_exe() -> Option<PathBuf> {
    let queries = [
        format!(r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\{PRODUCT}"),
        format!(r"HKLM\Software\Microsoft\Windows\CurrentVersion\Uninstall\{PRODUCT}"),
        format!(r"HKLM\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\{PRODUCT}"),
    ];
    for key in queries {
        if let Some(p) = reg_uninstall_string(&key) {
            if p.exists() {
                return Some(p);
            }
        }
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let p = PathBuf::from(local).join(PRODUCT).join("uninstall.exe");
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn reg_uninstall_string(key: &str) -> Option<PathBuf> {
    let output = Command::new("reg")
        .args(["query", key, "/v", "UninstallString"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        if let Some(val) = line.split("REG_SZ").nth(1) {
            return parse_quoted_path(val.trim());
        }
    }
    None
}

fn parse_quoted_path(s: &str) -> Option<PathBuf> {
    let s = s.trim();
    let path = if let Some(rest) = s.strip_prefix('"') {
        let end = rest.find('"')?;
        rest[..end].to_string()
    } else {
        s.split_whitespace().next()?.to_string()
    };
    Some(PathBuf::from(path))
}

fn show_message(title: &str, body: &str) {
    let safe_body = body.replace('\'', "''");
    let safe_title = title.replace('\'', "''");
    let ps = format!(
        "Add-Type -AssemblyName PresentationFramework; [System.Windows.MessageBox]::Show('{safe_body}','{safe_title}') | Out-Null"
    );
    let _ = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn log_line(msg: &str) {
    let path = std::env::temp_dir().join("voice_vibecoding_silent_updater.log");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "{msg}");
    }
}
