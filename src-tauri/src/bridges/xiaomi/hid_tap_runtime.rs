//! 对齐 Python `hid_tap_runtime.py`：校验/解压 Frida Gadget 到 ProgramData

use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{copy, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const GADGET_VERSION: &str = "17.15.3";
pub const GADGET_ARCHIVE_NAME: &str = "frida-gadget-17.15.3-windows-x86_64.dll.xz";
pub const GADGET_ARCHIVE_SHA256: &str =
    "b566d70189b6d551ad8f4e0bea24de08a3d4c0f559bb35b2bdb67d45182240c2";
pub const GADGET_DLL_NAME: &str = "RemoteBridgeHidTap.dll";
pub const GADGET_DLL_SHA256: &str =
    "6fca4007b2284c765a6c15c967a741f536b5865bf83867326a54029a3b752748";
pub const GADGET_CONFIG_NAME: &str = "RemoteBridgeHidTap.config";
pub const GADGET_SCRIPT_NAME: &str = "xiaomi_hid_gadget.js";

pub const HID_TAP_PORT: u16 = 30684;

pub const BTHLE_ENUM_KEY: &str = r"SYSTEM\CurrentControlSet\Enum\BTHLEDevice";
pub const HID_SERVICE_PREFIX: &str = "{00001812-0000-1000-8000-00805f9b34fb}";
/// 对齐 Python：含 rev 后缀；匹配时用 contains，兼容缺省 rev 的旧写法
pub const RC003_HARDWARE_TOKEN: &str = "dev_vid&012717_pid&32b8_rev&00a4";
pub const RC003_HARDWARE_TOKEN_SHORT: &str = "dev_vid&012717_pid&32b8";
pub const WUDF_DIAGNOSTIC_SUFFIX: &str = r"Device Parameters\WUDFDiagnosticInfo";

const GADGET_SCRIPT: &str = include_str!("xiaomi_hid_gadget.js");

pub fn hid_tap_port() -> u16 {
    std::env::var("REMOTE_BRIDGE_XIAOMI_HID_TAP_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(HID_TAP_PORT)
}

pub fn sha256_file(path: &Path) -> Result<String, String> {
    let file = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    copy(&mut reader, &mut hasher).map_err(|e| format!("hash {}: {e}", path.display()))?;
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn gadget_archive_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(p) = std::env::var("REMOTE_BRIDGE_XIAOMI_GADGET_ARCHIVE") {
        out.push(PathBuf::from(p));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            out.push(dir.join("assets").join("xiaomi").join(GADGET_ARCHIVE_NAME));
            out.push(
                dir.join("resources")
                    .join("assets")
                    .join("xiaomi")
                    .join(GADGET_ARCHIVE_NAME),
            );
            // Tauri 2 Windows 资源布局
            out.push(
                dir.join("_up_")
                    .join("resources")
                    .join("assets")
                    .join("xiaomi")
                    .join(GADGET_ARCHIVE_NAME),
            );
            if let Some(parent) = dir.parent() {
                out.push(
                    parent
                        .join("resources")
                        .join("assets")
                        .join("xiaomi")
                        .join(GADGET_ARCHIVE_NAME),
                );
            }
        }
    }
    if let Some(manifest) = option_env!("CARGO_MANIFEST_DIR") {
        out.push(
            PathBuf::from(manifest)
                .join("assets")
                .join("xiaomi")
                .join(GADGET_ARCHIVE_NAME),
        );
    }
    out
}

pub fn find_gadget_archive() -> Option<PathBuf> {
    for path in gadget_archive_candidates() {
        if path.is_file() {
            match sha256_file(&path) {
                Ok(hash) if hash.eq_ignore_ascii_case(GADGET_ARCHIVE_SHA256) => return Some(path),
                Ok(hash) => {
                    log::warn!(
                        "Gadget archive hash mismatch path={} got={hash}",
                        path.display()
                    );
                }
                Err(e) => log::warn!("Gadget archive unreadable: {e}"),
            }
        }
    }
    None
}

pub fn gadget_archive_available() -> bool {
    find_gadget_archive().is_some()
}

pub fn secure_runtime_directory() -> PathBuf {
    let program_data = std::env::var("PROGRAMDATA").unwrap_or_else(|_| r"C:\ProgramData".into());
    let runtime_id =
        std::env::var("REMOTE_BRIDGE_XIAOMI_RUNTIME_ID").unwrap_or_else(|_| "RemoteBridgeHub".into());
    PathBuf::from(program_data)
        .join(runtime_id)
        .join("hid-tap")
        .join(format!("{GADGET_VERSION}-x64-{}", &GADGET_DLL_SHA256[..12]))
}

fn gadget_config_text() -> String {
    format!(
        "{{\n  \"interaction\": {{\n    \"type\": \"script\",\n    \"path\": \"{GADGET_SCRIPT_NAME}\",\n    \"parameters\": {{\n      \"host\": \"127.0.0.1\",\n      \"port\": {}\n    }},\n    \"on_change\": \"ignore\"\n  }},\n  \"runtime\": \"qjs\",\n  \"teardown\": \"minimal\"\n}}\n",
        hid_tap_port()
    )
}

fn write_verified_text(path: &Path, content: &str) -> Result<(), String> {
    let encoded = content.as_bytes();
    if path.is_file() {
        if let Ok(existing) = fs::read(path) {
            if existing == encoded {
                return Ok(());
            }
        }
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    let tmp = path.with_extension(format!(
        "{}.tmp.{}",
        path.extension().and_then(|s| s.to_str()).unwrap_or("tmp"),
        std::process::id()
    ));
    fs::write(&tmp, encoded).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("replace {}: {e}", path.display()))?;
    Ok(())
}

fn lock_runtime_acl(path: &Path) -> Result<(), String> {
    fn apply(target: &Path, directory: bool) -> Result<(), String> {
        let suffix = if directory { "(OI)(CI)" } else { "" };
        let output = {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            // ponytail: 隐藏控制台，避免启动时闪黑框
            Command::new("icacls.exe")
                .creation_flags(CREATE_NO_WINDOW)
                .arg(target)
                .args([
                    "/inheritance:r",
                    "/grant:r",
                    &format!("*S-1-5-18:{suffix}F"),
                    &format!("*S-1-5-32-544:{suffix}F"),
                    &format!("*S-1-5-32-545:{suffix}RX"),
                    "/C",
                    "/Q",
                ])
                .output()
                .map_err(|e| format!("icacls spawn failed: {e}"))?
        };
        if !output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Err(format!("failed to secure Gadget runtime ACL: {}", stdout.trim()));
        }
        Ok(())
    }

    apply(path, true)?;
    if path.is_dir() {
        for entry in walkdir_shallow(path)? {
            apply(&entry, entry.is_dir())?;
        }
    }
    Ok(())
}

fn walkdir_shallow(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
        for entry in fs::read_dir(dir).map_err(|e| format!("read_dir {}: {e}", dir.display()))? {
            let entry = entry.map_err(|e| format!("dir entry: {e}"))?;
            let path = entry.path();
            out.push(path.clone());
            if path.is_dir() {
                walk(&path, out)?;
            }
        }
        Ok(())
    }
    walk(root, &mut out)?;
    Ok(out)
}

/// 解压并写入 Gadget DLL / config / script，返回 (DLL 路径, 是否检测到脚本变更)。
/// `script_changed=true` 时调用方必须在注入完成后重启 RC003 宿主，
/// 让下一次挂载加载新脚本（on_change=ignore 使 Frida 不会热重载）。
pub fn prepare_secure_runtime() -> Result<(PathBuf, bool), String> {
    let archive = find_gadget_archive().ok_or_else(|| {
        "verified RC003 Gadget asset is missing (assets/xiaomi/frida-gadget-*.dll.xz)".to_string()
    })?;

    let destination = secure_runtime_directory();
    fs::create_dir_all(&destination)
        .map_err(|e| format!("mkdir runtime {}: {e}", destination.display()))?;
    if let Err(e) = lock_runtime_acl(&destination) {
        log::warn!("lock runtime ACL (pre): {e}");
    }

    // 脚本变更检测：若磁盘上的 gadget 脚本与当前内嵌版本不一致，
    // 说明 dev 重编译 / 版本升级更新了脚本。Frida 配置是 on_change=ignore，
    // WUDFHost 不会重载新脚本 —— 只有重启 RC003 宿主（WUDFHost）才会在挂载时
    // 重新读取新脚本。否则脚本清除逻辑（如 menu 0x65、音量 0x80/81）永远不生效。
    //
    // 注意：这里只做「检测 + 写盘」，绝不在此处杀宿主 —— 本函数在注入器里
    // `inject_library(pid)` 之前调用，此刻杀掉 pid 会让随后的注入必然失败。
    // 正确顺序由调用方保证：先注入旧宿主 → 若本次注入前检测到脚本变化，
    // 再重启宿主，让下一次挂载加载新脚本（见 hid_tap_injector::perform_injection）。
    let script_path = destination.join(GADGET_SCRIPT_NAME);
    let script_changed = !script_path.is_file()
        || fs::read_to_string(&script_path)
            .map(|cur| cur.trim() != GADGET_SCRIPT.trim())
            .unwrap_or(true);
    if script_changed {
        log::info!(
            "XIAOMI HID TAP script changed ({} -> {}), host restart scheduled after inject",
            script_path.display(),
            GADGET_SCRIPT.trim().len()
        );
    }

    let dll_path = destination.join(GADGET_DLL_NAME);
    let need_extract = !dll_path.is_file()
        || sha256_file(&dll_path)
            .map(|h| !h.eq_ignore_ascii_case(GADGET_DLL_SHA256))
            .unwrap_or(true);

    if need_extract {
        let temporary = dll_path.with_extension(format!("dll.{}.tmp", std::process::id()));
        {
            let archive_file =
                File::open(&archive).map_err(|e| format!("open archive: {e}"))?;
            let mut decoder = xz2::read::XzDecoder::new(BufReader::new(archive_file));
            let mut out = File::create(&temporary)
                .map_err(|e| format!("create {}: {e}", temporary.display()))?;
            copy(&mut decoder, &mut out).map_err(|e| format!("decompress gadget: {e}"))?;
        }
        let dll_hash = sha256_file(&temporary)?;
        if !dll_hash.eq_ignore_ascii_case(GADGET_DLL_SHA256) {
            let _ = fs::remove_file(&temporary);
            return Err(format!("Gadget DLL hash mismatch: {dll_hash}"));
        }
        fs::rename(&temporary, &dll_path)
            .map_err(|e| format!("install dll {}: {e}", dll_path.display()))?;
    }

    write_verified_text(&destination.join(GADGET_CONFIG_NAME), &gadget_config_text())?;
    write_verified_text(&destination.join(GADGET_SCRIPT_NAME), GADGET_SCRIPT)?;
    if let Err(e) = lock_runtime_acl(&destination) {
        log::warn!("lock runtime ACL (post): {e}");
    }

    Ok((dll_path, script_changed))
}

/// 查找 RC003 HidOverGatt 对应的 WUDFHost PID（对齐 Python：直接返回注册表 HostPid）
pub fn find_rc003_hidogatt_host_pid() -> Option<u32> {
    #[cfg(target_os = "windows")]
    {
        windows_find_host_pid().filter(|pid| *pid > 0)
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

#[cfg(target_os = "windows")]
pub fn process_name_toolhelp(pid: u32) -> Option<String> {
    windows_process_name_toolhelp(pid)
}

#[cfg(target_os = "windows")]
fn windows_process_name_toolhelp(pid: u32) -> Option<String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut found = None;
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                if entry.th32ProcessID == pid {
                    let len = entry
                        .szExeFile
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(entry.szExeFile.len());
                    found = Some(String::from_utf16_lossy(&entry.szExeFile[..len]));
                    break;
                }
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
        found
    }
}

#[cfg(target_os = "windows")]
fn windows_find_host_pid() -> Option<u32> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY_LOCAL_MACHINE, KEY_READ,
        REG_DWORD, REG_VALUE_TYPE,
    };

    unsafe {
        let mut root = Default::default();
        let key_wide: Vec<u16> = BTHLE_ENUM_KEY
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let open_root = RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(key_wide.as_ptr()),
            0,
            KEY_READ,
            &mut root,
        );
        if open_root != ERROR_SUCCESS {
            log::warn!("XIAOMI HID TAP RegOpenKeyExW BTHLEDevice failed {open_root:?}");
            return None;
        }

        let mut service_index = 0u32;
        let mut matched_services = 0u32;
        loop {
            let mut name_buf = [0u16; 512];
            let mut name_len = name_buf.len() as u32;
            let enum_result = RegEnumKeyExW(
                root,
                service_index,
                windows::core::PWSTR(name_buf.as_mut_ptr()),
                &mut name_len,
                None,
                windows::core::PWSTR::null(),
                None,
                None,
            );
            if enum_result != ERROR_SUCCESS {
                break;
            }
            service_index += 1;
            let service_name = String::from_utf16_lossy(&name_buf[..name_len as usize]);
            let folded = service_name.to_ascii_lowercase();
            if !folded.starts_with(&HID_SERVICE_PREFIX.to_ascii_lowercase()) {
                continue;
            }
            if !(folded.contains(RC003_HARDWARE_TOKEN) || folded.contains(RC003_HARDWARE_TOKEN_SHORT))
            {
                continue;
            }
            matched_services += 1;

            let mut service_key = Default::default();
            let service_wide: Vec<u16> = service_name
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            if RegOpenKeyExW(
                root,
                PCWSTR(service_wide.as_ptr()),
                0,
                KEY_READ,
                &mut service_key,
            ) != ERROR_SUCCESS
            {
                continue;
            }

            let mut instance_index = 0u32;
            loop {
                let mut inst_buf = [0u16; 512];
                let mut inst_len = inst_buf.len() as u32;
                let inst_result = RegEnumKeyExW(
                    service_key,
                    instance_index,
                    windows::core::PWSTR(inst_buf.as_mut_ptr()),
                    &mut inst_len,
                    None,
                    windows::core::PWSTR::null(),
                    None,
                    None,
                );
                if inst_result != ERROR_SUCCESS {
                    break;
                }
                instance_index += 1;
                let instance_name = String::from_utf16_lossy(&inst_buf[..inst_len as usize]);

                let mut instance_key = Default::default();
                let inst_wide: Vec<u16> = instance_name
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect();
                if RegOpenKeyExW(
                    service_key,
                    PCWSTR(inst_wide.as_ptr()),
                    0,
                    KEY_READ,
                    &mut instance_key,
                ) != ERROR_SUCCESS
                {
                    continue;
                }

                let diag_rel: Vec<u16> = WUDF_DIAGNOSTIC_SUFFIX
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect();
                let mut diag_key = Default::default();
                let open_diag = RegOpenKeyExW(
                    instance_key,
                    PCWSTR(diag_rel.as_ptr()),
                    0,
                    KEY_READ,
                    &mut diag_key,
                );
                let _ = RegCloseKey(instance_key);
                if open_diag != ERROR_SUCCESS {
                    continue;
                }

                let value_name: Vec<u16> = "HostPid\0".encode_utf16().collect();
                let mut data = [0u8; 8];
                let mut data_len = data.len() as u32;
                let mut value_type = REG_VALUE_TYPE::default();
                let q = RegQueryValueExW(
                    diag_key,
                    PCWSTR(value_name.as_ptr()),
                    None,
                    Some(&mut value_type),
                    Some(data.as_mut_ptr()),
                    Some(&mut data_len),
                );
                let _ = RegCloseKey(diag_key);
                if q != ERROR_SUCCESS || data_len < 4 {
                    continue;
                }
                // 本机实测 HostPid 可能是 REG_DWORD(4) 或 REG_QWORD(11)；Python winreg 两种都能读成 int
                let ty = value_type.0;
                if ty != REG_DWORD.0 && ty != 4 && ty != 11 {
                    log::warn!(
                        "XIAOMI HID TAP HostPid unexpected type={ty} len={data_len} instance={instance_name}"
                    );
                    continue;
                }
                let pid = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                if pid > 0 {
                    let _ = RegCloseKey(service_key);
                    let _ = RegCloseKey(root);
                    // debug：hub loop 每轮重试都会调用本函数，INFO 会每秒刷屏撑爆日志文件
                    log::debug!(
                        "XIAOMI HID TAP HostPid={pid} type={ty} service={service_name} instance={instance_name}"
                    );
                    return Some(pid);
                }
            }
            let _ = RegCloseKey(service_key);
        }
        let _ = RegCloseKey(root);
        log::warn!(
            "XIAOMI HID TAP HostPid not found services_scanned={service_index} rc003_matched={matched_services}"
        );
    }
    None
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(target_os = "windows")]
    fn find_rc003_host_pid_smoke() {
        let pid = super::find_rc003_hidogatt_host_pid()
            .expect("RC003 HostPid must be found when remote is paired (check BTHLEDevice registry)");
        assert!(pid > 0);
        let name = super::process_name_toolhelp(pid).unwrap_or_default();
        assert!(
            name.eq_ignore_ascii_case("wudfhost.exe"),
            "expected WUDFHost.exe got {name} pid={pid}"
        );
    }
}
