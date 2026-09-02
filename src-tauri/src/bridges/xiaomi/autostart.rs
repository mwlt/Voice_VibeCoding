//! 开机自启：仅写入当前用户 Run 注册表（`exe --minimized`）。
//!
//! `--minimized` 仅用于单实例去重（重复自启静默忽略）；是否进托盘由
//! `start_minimized_to_tray` 全局设置决定。

use std::path::PathBuf;

#[cfg(target_os = "windows")]
fn startup_dir() -> Result<PathBuf, String> {
    let appdata = std::env::var("APPDATA").map_err(|_| "APPDATA missing".to_string())?;
    Ok(PathBuf::from(appdata)
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs")
        .join("Startup"))
}

#[cfg(target_os = "windows")]
fn legacy_shortcut_path() -> Result<PathBuf, String> {
    Ok(startup_dir()?.join("RemoteBridgeHub.lnk"))
}

#[cfg(target_os = "windows")]
fn legacy_shortcut_exists() -> bool {
    legacy_shortcut_path()
        .map(|p| p.is_file())
        .unwrap_or(false)
}

/// 启用/禁用开机自启（Run 键 + `--minimized`；始终清理旧版 Startup 快捷方式）
pub fn set_autostart_enabled(enable: bool) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        set_run_key(enable)?;
        remove_legacy_startup_shortcut();
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = enable;
        Err("仅支持 Windows".into())
    }
}

pub fn is_autostart_enabled() -> bool {
    #[cfg(target_os = "windows")]
    {
        run_key_exists() || legacy_shortcut_exists()
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

/// 应用启动时去重：Run 与 Startup 并存时删快捷方式；仅快捷方式时迁移到 Run。
/// `settings_autostart`: 来自 settings.json；为 `Some(false)` 时清除遗留注册表/lnk（避免 UI 已关但仍自启）。
pub fn reconcile_autostart_entries(settings_autostart: Option<bool>) {
    #[cfg(target_os = "windows")]
    {
        if settings_autostart == Some(false) {
            if run_key_exists() {
                match set_run_key(false) {
                    Ok(()) => log::info!("autostart: removed Run key (settings.autostart=false)"),
                    Err(e) => log::warn!("autostart: remove Run key failed: {e}"),
                }
            }
            remove_legacy_startup_shortcut();
            return;
        }

        let run = run_key_exists();
        let lnk = legacy_shortcut_exists();
        if run && lnk {
            remove_legacy_startup_shortcut();
            log::info!("autostart: removed duplicate Startup shortcut (Run key active)");
        } else if !run && lnk {
            match set_run_key(true) {
                Ok(()) => {
                    remove_legacy_startup_shortcut();
                    log::info!("autostart: migrated legacy Startup shortcut to Run key");
                }
                Err(e) => log::warn!("autostart: migrate Startup shortcut failed: {e}"),
            }
        }
        if run_key_exists() {
            match set_run_key(true) {
                Ok(()) => log::debug!("autostart: Run key path refreshed to current exe"),
                Err(e) => log::warn!("autostart: refresh Run key path failed: {e}"),
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = settings_autostart;
    }
}

#[cfg(target_os = "windows")]
fn remove_legacy_startup_shortcut() {
    if let Ok(link) = legacy_shortcut_path() {
        if link.is_file() {
            match std::fs::remove_file(&link) {
                Ok(()) => log::info!("autostart: removed legacy Startup shortcut {:?}", link),
                Err(e) => log::warn!("autostart: failed to remove legacy shortcut {:?}: {e}", link),
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn set_run_key(enable: bool) -> Result<(), String> {
    use windows::core::w;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY_CURRENT_USER, KEY_WRITE,
        REG_SZ,
    };

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let value = format!("\"{}\" --minimized", exe.display());
    let value_wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    let name = w!("RemoteBridgeHub");

    unsafe {
        let mut key = Default::default();
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            0,
            KEY_WRITE,
            &mut key,
        )
        .ok()
        .map_err(|e| format!("RegOpenKeyExW: {e}"))?;

        let result = if enable {
            let bytes = std::slice::from_raw_parts(
                value_wide.as_ptr() as *const u8,
                value_wide.len() * 2,
            );
            RegSetValueExW(key, name, 0, REG_SZ, Some(bytes))
        } else {
            RegDeleteValueW(key, name)
        };
        let _ = RegCloseKey(key);
        if enable {
            if result != ERROR_SUCCESS {
                return Err(format!("RegSetValueExW failed {result:?}"));
            }
        } else if result.is_err() && run_key_exists() {
            return Err(format!("RegDeleteValueW failed: {:?}", result));
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn run_key_exists() -> bool {
    use windows::core::w;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY_CURRENT_USER, KEY_READ, REG_VALUE_TYPE,
    };
    unsafe {
        let mut key = Default::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            0,
            KEY_READ,
            &mut key,
        )
        .is_err()
        {
            return false;
        }
        let mut data_len = 0u32;
        let mut ty = REG_VALUE_TYPE::default();
        let q = RegQueryValueExW(
            key,
            w!("RemoteBridgeHub"),
            None,
            Some(&mut ty),
            None,
            Some(&mut data_len),
        );
        let _ = RegCloseKey(key);
        q.is_ok()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn reconcile_is_noop_off_windows() {
        super::reconcile_autostart_entries(None);
        super::reconcile_autostart_entries(Some(false));
    }
}
