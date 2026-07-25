//! 对齐 Python `hid_tap_injector.py`：UAC 提升 + LoadLibraryW 注入 Gadget

use crate::bridges::xiaomi::hid_tap_runtime::{
    find_gadget_archive, find_rc003_hidogatt_host_pid, prepare_secure_runtime, sha256_file,
    GADGET_DLL_SHA256,
};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

fn injector_log(msg: &str) {
    log::info!("{msg}");
    let path = std::env::temp_dir().join("remote-bridge-hid-injector.log");
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{}", msg);
    }
}

/// 主进程：已提权则进程内注入，否则 ShellExecute runas
pub fn launch_elevated_injector(pid: u32) -> Result<bool, String> {
    #[cfg(target_os = "windows")]
    {
        windows_launch_or_inject(pid)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = pid;
        Err("仅支持 Windows".into())
    }
}

/// CLI 入口：`xiaomi-hid-injector --pid <n>`
pub fn run_injector_cli(args: &[String]) -> i32 {
    #[cfg(target_os = "windows")]
    {
        match windows_injector_main(args) {
            Ok(()) => {
                injector_log("xiaomi-hid-injector OK");
                0
            }
            Err(e) => {
                injector_log(&format!("xiaomi-hid-injector failed: {e}"));
                eprintln!("xiaomi-hid-injector failed: {e}");
                1
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = args;
        eprintln!("xiaomi-hid-injector: Windows only");
        1
    }
}

#[cfg(target_os = "windows")]
fn parse_pid(args: &[String]) -> Result<u32, String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--pid" {
            let value = iter
                .next()
                .ok_or_else(|| "missing --pid value".to_string())?;
            return value
                .parse()
                .map_err(|_| format!("invalid --pid: {value}"));
        }
    }
    Err("required --pid".into())
}

#[cfg(target_os = "windows")]
fn perform_injection(pid: u32) -> Result<(), String> {
    let expected = find_rc003_hidogatt_host_pid();
    if expected != Some(pid) {
        return Err(format!(
            "RC003 host changed before injection: expected={expected:?} requested={pid}"
        ));
    }
    let name = process_image_name(pid)?;
    if !name.eq_ignore_ascii_case("wudfhost.exe") {
        return Err(format!("refusing non-WUDFHost target: {name}"));
    }
    if find_gadget_archive().is_none() {
        return Err(
            "verified Gadget archive missing (src-tauri/assets/xiaomi/frida-gadget-*.dll.xz)"
                .into(),
        );
    }
    let dll_path = prepare_secure_runtime()?;
    let dll_hash = sha256_file(&dll_path)?;
    if !dll_hash.eq_ignore_ascii_case(GADGET_DLL_SHA256) {
        return Err(format!("verified Gadget changed before injection: {dll_hash}"));
    }
    enable_debug_privilege()?;
    inject_library(pid, &dll_path)?;
    injector_log(&format!(
        "injected pid={pid} dll={}",
        dll_path.display()
    ));
    Ok(())
}

#[cfg(target_os = "windows")]
fn windows_injector_main(args: &[String]) -> Result<(), String> {
    use windows::Win32::UI::Shell::IsUserAnAdmin;

    let pid = parse_pid(args)?;
    injector_log(&format!("injector start pid={pid}"));
    if !unsafe { IsUserAnAdmin().as_bool() } {
        return Err("RC003 injector requires elevation".into());
    }
    perform_injection(pid)
}

#[cfg(target_os = "windows")]
fn windows_launch_or_inject(pid: u32) -> Result<bool, String> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::UI::Shell::{IsUserAnAdmin, ShellExecuteW};

    if find_gadget_archive().is_none() {
        return Err("Gadget 资源缺失，无法注入".into());
    }

    // 已管理员：直接进程内注入，避免二次 UAC
    if unsafe { IsUserAnAdmin().as_bool() } {
        injector_log(&format!("in-process inject pid={pid}"));
        perform_injection(pid)?;
        return Ok(true);
    }

    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let cwd = exe
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    let params = format!("xiaomi-hid-injector --pid {pid}");
    let exe_h = HSTRING::from(exe.as_os_str());
    let params_h = HSTRING::from(params.as_str());
    let cwd_h = HSTRING::from(cwd.as_os_str());
    let verb = HSTRING::from("runas");

    injector_log(&format!(
        "ShellExecute runas exe={} params={}",
        exe.display(),
        params
    ));

    // SW_SHOWNORMAL：确保 UAC 提示可见（SW_HIDE 在部分系统上会吞掉提权对话框）
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(verb.as_ptr()),
            PCWSTR(exe_h.as_ptr()),
            PCWSTR(params_h.as_ptr()),
            PCWSTR(cwd_h.as_ptr()),
            windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
        )
    };
    let code = result.0 as isize;
    if code <= 32 {
        return Err(format!("ShellExecuteW failed code={code}"));
    }
    Ok(true)
}

#[cfg(target_os = "windows")]
fn process_image_name(pid: u32) -> Result<String, String> {
    // WUDFHost 对 OpenProcess 常返回 ACCESS_DENIED；用 Toolhelp 取映像名（对齐 HostPid 查找）
    if let Some(name) = crate::bridges::xiaomi::hid_tap_runtime::process_name_toolhelp(pid) {
        return Ok(name);
    }
    Err(format!("process {pid} not found via Toolhelp"))
}

#[cfg(target_os = "windows")]
fn enable_debug_privilege() -> Result<(), String> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_NOT_ALL_ASSIGNED, HANDLE, LUID};
    use windows::Win32::Security::{
        AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED,
        TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        )
        .map_err(|e| format!("OpenProcessToken: {e}"))?;

        let mut luid = LUID::default();
        let name: Vec<u16> = "SeDebugPrivilege\0".encode_utf16().collect();
        let lookup = LookupPrivilegeValueW(PCWSTR::null(), PCWSTR(name.as_ptr()), &mut luid);
        if lookup.is_err() {
            let _ = CloseHandle(token);
            return Err(format!("LookupPrivilegeValueW: {:?}", lookup.err()));
        }

        let mut privileges = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES {
                Luid: luid,
                Attributes: SE_PRIVILEGE_ENABLED,
            }],
        };

        let adjust = AdjustTokenPrivileges(token, false, Some(&mut privileges), 0, None, None);
        let err = GetLastError();
        let _ = CloseHandle(token);
        if adjust.is_err() {
            return Err(format!("AdjustTokenPrivileges failed: {err:?}"));
        }
        if err == ERROR_NOT_ALL_ASSIGNED {
            return Err("SeDebugPrivilege is not assigned".into());
        }
        Ok(())
    }
}

#[cfg(target_os = "windows")]
fn inject_library(pid: u32, dll_path: &Path) -> Result<(), String> {
    use windows::core::s;
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows::Win32::System::Diagnostics::Debug::WriteProcessMemory;
    use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    use windows::Win32::System::Memory::{
        VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
    };
    use windows::Win32::System::Threading::{
        CreateRemoteThread, GetExitCodeThread, OpenProcess, WaitForSingleObject,
        PROCESS_CREATE_THREAD, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ,
        PROCESS_VM_WRITE,
    };

    let rights = PROCESS_CREATE_THREAD
        | PROCESS_QUERY_INFORMATION
        | PROCESS_VM_OPERATION
        | PROCESS_VM_WRITE
        | PROCESS_VM_READ;

    unsafe {
        let process = OpenProcess(rights, false, pid)
            .map_err(|e| format!("OpenProcess inject: {e}"))?;

        let path_str = dll_path
            .canonicalize()
            .unwrap_or_else(|_| dll_path.to_path_buf());
        // LoadLibraryW 需要 DOS 路径；去掉 \\?\ 前缀
        let path_display = path_str.to_string_lossy();
        let path_for_load = path_display
            .strip_prefix(r"\\?\")
            .unwrap_or(&path_display)
            .to_string();

        let mut encoded: Vec<u8> = path_for_load
            .encode_utf16()
            .flat_map(|c| c.to_le_bytes())
            .collect();
        encoded.extend_from_slice(&[0, 0]);

        let remote = VirtualAllocEx(
            process,
            None,
            encoded.len(),
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        );
        if remote.is_null() {
            let _ = CloseHandle(process);
            return Err("VirtualAllocEx failed".into());
        }

        let write = WriteProcessMemory(
            process,
            remote,
            encoded.as_ptr() as *const _,
            encoded.len(),
            None,
        );
        if write.is_err() {
            let _ = VirtualFreeEx(process, remote, 0, MEM_RELEASE);
            let _ = CloseHandle(process);
            return Err(format!("WriteProcessMemory: {:?}", write.err()));
        }

        let kernel = GetModuleHandleW(windows::core::w!("kernel32.dll"))
            .map_err(|e| format!("GetModuleHandleW: {e}"))?;
        let load_library = GetProcAddress(kernel, s!("LoadLibraryW"))
            .ok_or_else(|| "GetProcAddress(LoadLibraryW) failed".to_string())?;

        let thread = CreateRemoteThread(
            process,
            None,
            0,
            Some(std::mem::transmute(load_library)),
            Some(remote),
            0,
            None,
        );
        let thread = match thread {
            Ok(h) => h,
            Err(e) => {
                let _ = VirtualFreeEx(process, remote, 0, MEM_RELEASE);
                let _ = CloseHandle(process);
                return Err(format!("CreateRemoteThread: {e}"));
            }
        };

        let wait = WaitForSingleObject(thread, 20_000);
        if wait != WAIT_OBJECT_0 {
            let _ = CloseHandle(thread);
            let _ = VirtualFreeEx(process, remote, 0, MEM_RELEASE);
            let _ = CloseHandle(process);
            return Err("remote LoadLibraryW timed out".into());
        }

        let mut exit_code = 0u32;
        let _ = GetExitCodeThread(thread, &mut exit_code);
        let _ = CloseHandle(thread);
        let _ = VirtualFreeEx(process, remote, 0, MEM_RELEASE);
        let _ = CloseHandle(process);

        if exit_code == 0 {
            return Err(format!(
                "remote LoadLibraryW returned NULL (dll={path_for_load})"
            ));
        }
        Ok(())
    }
}
