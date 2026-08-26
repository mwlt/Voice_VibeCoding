//! 开机自启：写入当前用户 Startup 快捷方式 + Run 注册表（对齐安装器/源码文档意图）

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
fn shortcut_path() -> Result<PathBuf, String> {
    Ok(startup_dir()?.join("RemoteBridgeHub.lnk"))
}

/// 启用/禁用开机自启（`--minimized`）
pub fn set_autostart_enabled(enable: bool) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        set_run_key(enable)?;
        set_startup_shortcut(enable)?;
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
        run_key_exists() || shortcut_path().map(|p| p.is_file()).unwrap_or(false)
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
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
            // 删除；不存在也算成功
            let _ = RegDeleteValueW(key, name);
            ERROR_SUCCESS
        };
        let _ = RegCloseKey(key);
        if result != ERROR_SUCCESS && enable {
            return Err(format!("RegSetValueExW failed {result:?}"));
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

#[cfg(target_os = "windows")]
fn set_startup_shortcut(enable: bool) -> Result<(), String> {
    let link = shortcut_path()?;
    if !enable {
        let _ = std::fs::remove_file(&link);
        return Ok(());
    }
    let dir = startup_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    // 用 PowerShell 创建 .lnk（无需额外 COM 绑定）
    let ps = format!(
        "$ws = New-Object -ComObject WScript.Shell; \
         $s = $ws.CreateShortcut('{}'); \
         $s.TargetPath = '{}'; \
         $s.Arguments = '--minimized'; \
         $s.WorkingDirectory = '{}'; \
         $s.Save()",
        link.display().to_string().replace('\'', "''"),
        exe.display().to_string().replace('\'', "''"),
        exe.parent()
            .unwrap_or(std::path::Path::new("."))
            .display()
            .to_string()
            .replace('\'', "''"),
    );
    let out = {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        // ponytail: 隐藏控制台，避免创建自启快捷方式时闪黑框
        std::process::Command::new("powershell")
            .creation_flags(CREATE_NO_WINDOW)
            .args(["-NoProfile", "-Command", &ps])
            .output()
    }
    .map_err(|e| format!("powershell: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "create shortcut failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}
