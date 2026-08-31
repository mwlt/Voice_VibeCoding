//! HTTP 文件下载（带进度事件），供应用更新、WinUHid / VB-CABLE 驱动包等复用。
//! 支持协作式取消：短读超时轮询 cancel 标志，丢弃半成品文件。

use serde::Serialize;
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

pub const CANCELLED_MSG: &str = "已取消下载";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDownloadProgress {
    pub downloaded: u64,
    pub total: Option<u64>,
    pub percent: Option<f32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDownloadComplete {
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDownloadError {
    pub message: String,
}

static WINUHID_DOWNLOADING: AtomicBool = AtomicBool::new(false);
static WINUHID_CANCEL: AtomicBool = AtomicBool::new(false);
static VBCABLE_DOWNLOADING: AtomicBool = AtomicBool::new(false);
static VBCABLE_CANCEL: AtomicBool = AtomicBool::new(false);

fn download_agent() -> ureq::Agent {
    // 短读超时，便于轮询 cancel；连接仍给足时间
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(30))
        .timeout_read(Duration::from_secs(3))
        .build()
}

fn emit_progress(app: &AppHandle, event: &str, downloaded: u64, total: Option<u64>) {
    let percent = total
        .filter(|t| *t > 0)
        .map(|t| ((downloaded as f64 / t as f64) * 100.0).min(100.0) as f32);
    let _ = app.emit(
        event,
        FileDownloadProgress {
            downloaded,
            total,
            percent,
        },
    );
}

fn is_timeout(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        ErrorKind::TimedOut | ErrorKind::WouldBlock | ErrorKind::Interrupted
    )
}

pub fn download_http_to_file(
    app: &AppHandle,
    url: &str,
    dest: &Path,
    progress_event: &str,
    cancel: &AtomicBool,
) -> Result<(), String> {
    if cancel.load(Ordering::SeqCst) {
        return Err(CANCELLED_MSG.into());
    }

    let agent = download_agent();
    let resp = agent
        .get(url)
        .set("User-Agent", "VoiceVibeCoding-Download/1.0")
        .call()
        .map_err(|e| {
            if cancel.load(Ordering::SeqCst) {
                CANCELLED_MSG.into()
            } else {
                format!("下载请求失败: {e}")
            }
        })?;
    if cancel.load(Ordering::SeqCst) {
        return Err(CANCELLED_MSG.into());
    }
    if !(200..300).contains(&resp.status()) {
        return Err(format!("下载失败: HTTP {}", resp.status()));
    }

    let total = resp
        .header("Content-Length")
        .and_then(|s| s.parse::<u64>().ok());

    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("无法创建目录 {}: {e}", parent.display()))?;
        }
    }

    let mut reader = resp.into_reader();
    let mut file = std::fs::File::create(dest)
        .map_err(|e| format!("无法写入 {}: {e}", dest.display()))?;

    let mut downloaded = 0u64;
    let mut buf = [0u8; 64 * 1024];
    let mut last_emit = Instant::now();

    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(CANCELLED_MSG.into());
        }
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if is_timeout(&e) => {
                if cancel.load(Ordering::SeqCst) {
                    return Err(CANCELLED_MSG.into());
                }
                continue;
            }
            Err(e) => {
                if cancel.load(Ordering::SeqCst) {
                    return Err(CANCELLED_MSG.into());
                }
                return Err(format!("读取下载数据失败: {e}"));
            }
        };
        file.write_all(&buf[..n])
            .map_err(|e| format!("写入文件失败: {e}"))?;
        downloaded += n as u64;

        if last_emit.elapsed() >= Duration::from_millis(120) {
            emit_progress(app, progress_event, downloaded, total);
            last_emit = Instant::now();
        }
    }

    if cancel.load(Ordering::SeqCst) {
        return Err(CANCELLED_MSG.into());
    }
    emit_progress(app, progress_event, downloaded, total);
    Ok(())
}

fn spawn_tracked_zip_download(
    app: AppHandle,
    url: String,
    dest: PathBuf,
    busy: &'static AtomicBool,
    cancel: &'static AtomicBool,
    thread_name: &str,
    progress_event: &'static str,
    complete_event: &'static str,
    error_event: &'static str,
    log_label: &str,
    busy_msg: &str,
) -> Result<(), String> {
    if url.trim().is_empty() {
        return Err("下载地址为空".into());
    }
    if dest.as_os_str().is_empty() {
        return Err("保存路径为空".into());
    }
    if busy
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err(busy_msg.into());
    }
    cancel.store(false, Ordering::SeqCst);

    let app_bg = app.clone();
    let label = log_label.to_string();
    std::thread::Builder::new()
        .name(thread_name.into())
        .spawn(move || {
            let result = download_http_to_file(&app_bg, &url, &dest, progress_event, cancel);
            let cancelled = matches!(result.as_ref(), Err(e) if e == CANCELLED_MSG)
                || cancel.load(Ordering::SeqCst);
            busy.store(false, Ordering::SeqCst);
            cancel.store(false, Ordering::SeqCst);

            match result {
                Ok(()) => {
                    log::info!("{label} zip downloaded: {}", dest.display());
                    let _ = app_bg.emit(
                        complete_event,
                        FileDownloadComplete {
                            path: dest.display().to_string(),
                        },
                    );
                }
                Err(e) => {
                    let _ = std::fs::remove_file(&dest);
                    if cancelled || e == CANCELLED_MSG {
                        log::info!("{label} zip download cancelled");
                        let _ = app_bg.emit(
                            error_event,
                            FileDownloadError {
                                message: CANCELLED_MSG.into(),
                            },
                        );
                    } else {
                        log::warn!("{label} zip download failed: {e}");
                        let _ = app_bg.emit(error_event, FileDownloadError { message: e });
                    }
                }
            }
        })
        .map_err(|e| {
            busy.store(false, Ordering::SeqCst);
            cancel.store(false, Ordering::SeqCst);
            format!("启动下载线程失败: {e}")
        })?;

    Ok(())
}

pub fn request_cancel_winuhid_zip_download() {
    if WINUHID_DOWNLOADING.load(Ordering::SeqCst) {
        WINUHID_CANCEL.store(true, Ordering::SeqCst);
    }
}

pub fn request_cancel_vbcable_zip_download() {
    if VBCABLE_DOWNLOADING.load(Ordering::SeqCst) {
        VBCABLE_CANCEL.store(true, Ordering::SeqCst);
    }
}

pub fn spawn_winuhid_zip_download(app: AppHandle, url: String, dest: PathBuf) -> Result<(), String> {
    spawn_tracked_zip_download(
        app,
        url,
        dest,
        &WINUHID_DOWNLOADING,
        &WINUHID_CANCEL,
        "winuhid-zip-download",
        "winuhid-download-progress",
        "winuhid-download-complete",
        "winuhid-download-error",
        "WinUHid",
        "已有 WinUHid 下载任务进行中",
    )
}

pub fn spawn_vbcable_zip_download(app: AppHandle, url: String, dest: PathBuf) -> Result<(), String> {
    spawn_tracked_zip_download(
        app,
        url,
        dest,
        &VBCABLE_DOWNLOADING,
        &VBCABLE_CANCEL,
        "vbcable-zip-download",
        "vbcable-download-progress",
        "vbcable-download-complete",
        "vbcable-download-error",
        "VB-CABLE",
        "已有 VB-CABLE 下载任务进行中",
    )
}
