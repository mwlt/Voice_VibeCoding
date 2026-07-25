//! 单文件运行日志：只记录排障必要内容，供「日志」按钮查看/复制

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
static LOG_LOCK: Mutex<()> = Mutex::new(());

const MAX_BYTES: u64 = 1_500_000; // ~1.5MB 后轮转

/// 高频/调试噪音：不写进用户可见日志
fn is_noise(msg: &str) -> bool {
    const NOISE: &[&str] = &[
        "ATVV AUDIO frames",
        "XIAOMI MAPPING key=",
        "XIAOMI MAPPING inject",
        "XIAOMI HID key=",
        "HID write ",
        "ATVV AUDIO_SYNC",
        "ATVV MIC_OPEN",
        "MIC_OPEN sent",
        "Shortcut capture",
        "Shortcut captured",
        "capture started",
        "capture cancelled",
        "polling armed",
        "poll thread",
        "Main window hidden",
        "BLE scan",
        "BLE scanning",
        "notify level=",
        "notify subscribed",
        "notify unsupported",
        "GET_CAPS",
        "CAPS received",
        "Raw Input",
        "SPECIAL KEY",
        "hotkey monitor",
        "Input device:",
        "Output device:",
        "Input config:",
        "Output config:",
        "Audio mixer",
        "Audio UDP",
        "Input devices:",
        "Output devices:",
    ];
    NOISE.iter().any(|p| msg.contains(p))
}

fn format_line(level: log::Level, msg: &str) -> String {
    let ts = chrono_like_now();
    format!("[{ts}] [{level}] {msg}\n")
}

fn chrono_like_now() -> String {
    chrono::Local::now()
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

fn rotate_if_needed(path: &Path) {
    let Ok(meta) = fs::metadata(path) else {
        return;
    };
    if meta.len() < MAX_BYTES {
        return;
    }
    let bak = path.with_extension("log.1");
    let _ = fs::remove_file(&bak);
    let _ = fs::rename(path, &bak);
}

fn write_line(path: &Path, line: &str) {
    let _guard = LOG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    rotate_if_needed(path);
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(line.as_bytes());
        let _ = f.flush();
    }
}

struct AppFileLogger;

impl log::Log for AppFileLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        // 只收本应用；第三方 crate 的 info 容易刷屏
        let target = record.target();
        let ours = target.starts_with("remote_bridge_hub")
            || target.starts_with("remote_bridge_hub_lib");
        if !ours && record.level() > log::Level::Warn {
            return;
        }
        let msg = record.args().to_string();
        if record.level() == log::Level::Info && is_noise(&msg) {
            return;
        }
        let Some(path) = LOG_PATH.get() else {
            return;
        };
        write_line(path, &format_line(record.level(), &msg));
    }

    fn flush(&self) {}
}

/// 初始化单文件日志；返回日志路径
pub fn init(logs_dir: &Path) -> PathBuf {
    let _ = fs::create_dir_all(logs_dir);
    let path = logs_dir.join("app.log");
    let _ = LOG_PATH.set(path.clone());

    // 控制台：仅在设置了 RUST_LOG 时启用（开发用）
    let console = std::env::var_os("RUST_LOG").is_some();
    if console {
        let env = env_logger::Builder::from_default_env().build();
        let _ = log::set_boxed_logger(Box::new(TeeLogger {
            file: AppFileLogger,
            console: Some(env),
        }));
    } else {
        let _ = log::set_boxed_logger(Box::new(AppFileLogger));
    }
    log::set_max_level(log::LevelFilter::Info);

    write_line(
        &path,
        &format_line(log::Level::Info, "—— 应用启动 ——"),
    );
    path
}

/// 子进程可复用同一日志文件（通过环境变量）
pub fn init_from_env() {
    if let Ok(p) = std::env::var("REMOTE_BRIDGE_LOG_PATH") {
        let path = PathBuf::from(p);
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = LOG_PATH.set(path);
        let _ = log::set_boxed_logger(Box::new(AppFileLogger));
        log::set_max_level(log::LevelFilter::Info);
    }
}

pub fn log_path() -> Option<PathBuf> {
    LOG_PATH.get().cloned()
}

pub fn read_log_text(max_chars: usize) -> Result<String, String> {
    let path = LOG_PATH
        .get()
        .cloned()
        .ok_or_else(|| "日志尚未初始化".to_string())?;
    if !path.exists() {
        return Ok(String::new());
    }
    let data = fs::read(&path).map_err(|e| format!("读取日志失败: {e}"))?;
    let text = String::from_utf8_lossy(&data).into_owned();
    if text.chars().count() <= max_chars {
        return Ok(text);
    }
    // 只取末尾，避免弹窗过大
    let skip = text.chars().count().saturating_sub(max_chars);
    Ok(format!(
        "……（仅显示末尾）\n{}",
        text.chars().skip(skip).collect::<String>()
    ))
}

pub fn open_log_in_editor() -> Result<(), String> {
    let path = LOG_PATH
        .get()
        .cloned()
        .ok_or_else(|| "日志尚未初始化".to_string())?;
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if !path.exists() {
        write_line(&path, &format_line(log::Level::Info, "（日志文件已创建）"));
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &path.to_string_lossy()])
            .spawn()
            .map_err(|e| format!("打开日志失败: {e}"))?;
        return Ok(());
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("仅支持 Windows".into())
    }
}

/// 手动追加一行（兼容旧 append_host_log）
pub fn append(message: &str) {
    let Some(path) = LOG_PATH.get() else {
        return;
    };
    write_line(path, &format_line(log::Level::Info, message));
}

struct TeeLogger {
    file: AppFileLogger,
    console: Option<env_logger::Logger>,
}

impl log::Log for TeeLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        self.file.enabled(metadata)
            || self
                .console
                .as_ref()
                .map(|c| c.enabled(metadata))
                .unwrap_or(false)
    }

    fn log(&self, record: &log::Record) {
        self.file.log(record);
        if let Some(c) = &self.console {
            c.log(record);
        }
    }

    fn flush(&self) {
        self.file.flush();
        if let Some(c) = &self.console {
            c.flush();
        }
    }
}
