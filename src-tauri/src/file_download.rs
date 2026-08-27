//! HTTP 文件下载（带进度事件），供应用更新、WinUHid 驱动包等复用。

use serde::Serialize;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

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

fn download_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(30))
        .timeout_read(Duration::from_secs(3600))
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

pub fn download_http_to_file(
    app: &AppHandle,
    url: &str,
    dest: &Path,
    progress_event: &str,
) -> Result<(), String> {
    let agent = download_agent();
    let resp = agent
        .get(url)
        .set("User-Agent", "VoiceVibeCoding-Download/1.0")
        .call()
        .map_err(|e| format!("下载请求失败: {e}"))?;
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
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("读取下载数据失败: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| format!("写入文件失败: {e}"))?;
        downloaded += n as u64;

        if last_emit.elapsed() >= Duration::from_millis(120) {
            emit_progress(app, progress_event, downloaded, total);
            last_emit = Instant::now();
        }
    }

    emit_progress(app, progress_event, downloaded, total);
    Ok(())
}

pub fn spawn_winuhid_zip_download(app: AppHandle, url: String, dest: PathBuf) -> Result<(), String> {
    if url.trim().is_empty() {
        return Err("下载地址为空".into());
    }
    if dest.as_os_str().is_empty() {
        return Err("保存路径为空".into());
    }
    if WINUHID_DOWNLOADING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("已有 WinUHid 下载任务进行中".into());
    }

    let app_bg = app.clone();
    std::thread::Builder::new()
        .name("winuhid-zip-download".into())
        .spawn(move || {
            let result = download_http_to_file(
                &app_bg,
                &url,
                &dest,
                "winuhid-download-progress",
            );
            WINUHID_DOWNLOADING.store(false, Ordering::SeqCst);

            match result {
                Ok(()) => {
                    log::info!("WinUHid zip downloaded: {}", dest.display());
                    let _ = app_bg.emit(
                        "winuhid-download-complete",
                        FileDownloadComplete {
                            path: dest.display().to_string(),
                        },
                    );
                }
                Err(e) => {
                    log::warn!("WinUHid zip download failed: {e}");
                    let _ = std::fs::remove_file(&dest);
                    let _ = app_bg.emit(
                        "winuhid-download-error",
                        FileDownloadError { message: e },
                    );
                }
            }
        })
        .map_err(|e| {
            WINUHID_DOWNLOADING.store(false, Ordering::SeqCst);
            format!("启动下载线程失败: {e}")
        })?;

    Ok(())
}
