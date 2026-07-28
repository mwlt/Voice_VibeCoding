//! 轻量更新检查：读取公开 update/latest.json（Gitee raw 优先，失败再 GitHub raw）

use crate::config::manager::ConfigManager;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::time::Duration;
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
    pub update_available: bool,
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
    let newer = is_newer(&latest, &current);
    let ignored = ignored_version
        .map(|v| normalize_version(v) == latest)
        .unwrap_or(false);
    let setup_url = if source == "gitee" {
        manifest.gitee_setup_url.clone()
    } else {
        manifest.github_setup_url.clone()
    };
    UpdateCheckResult {
        checked: true,
        update_available: newer && !ignored,
        ignored: newer && ignored,
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
                "UPDATE check via gitee: current={} latest={} available={}",
                r.current_version,
                r.latest_version,
                r.update_available
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
                "UPDATE check via github: current={} latest={} available={}",
                r.current_version,
                r.latest_version,
                r.update_available
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
            last.ignored = true;
            last.update_available = false;
            *LAST_RESULT.lock() = Some(last.clone());
            return Ok(last);
        }
    }
    Ok(check_for_update(config))
}

pub fn emit_if_available(app: &AppHandle, result: &UpdateCheckResult) {
    if result.update_available {
        let _ = app.emit("app-update-available", result);
    }
}

pub fn spawn_startup_check(app: AppHandle) {
    std::thread::Builder::new()
        .name("app-update-check".into())
        .spawn(move || {
            std::thread::sleep(Duration::from_secs(4));
            let Some(config) = app.try_state::<ConfigManager>() else {
                return;
            };
            let result = check_for_update(config.inner());
            emit_if_available(&app, &result);
        })
        .ok();
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
}
