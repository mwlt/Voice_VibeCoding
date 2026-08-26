//! Diagnosis feedback loop: IME voice wake — injected keys must not look "injected"
//! to downstream hooks after our special_keys sanitizer runs.
//!
//! User symptom: physical Right Alt / Ctrl+Win wakes 豆包/千问; remote-mapped SendInput does not.
//! Root pattern: LLKHF_INJECTED filtering. This loop asserts our hook clears the flag for EXTRA_INFO.
//!
//! Run:
//!   cargo run --manifest-path src-tauri/Cargo.toml --example diag_ime_inject_flags
//! Exit 0 = GREEN (downstream saw cleaned flags). Exit 1 = RED.

use remote_bridge_hub_lib::bridges::xiaomi::key_mapping::EXTRA_INFO;
use remote_bridge_hub_lib::bridges::xiaomi::voice_inject::{
    sanitize_own_inject_flags, LLKHF_INJECTED, LLKHF_INJECTED_MASK,
};
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;
use std::time::Duration;

fn main() {
    // Pure-function seam (always agent-runnable)
    let dirty = LLKHF_INJECTED | 0x01;
    let cleaned = sanitize_own_inject_flags(EXTRA_INFO, EXTRA_INFO, dirty);
    if cleaned & LLKHF_INJECTED_MASK != 0 {
        eprintln!("RED: sanitize_own_inject_flags left INJECTED bits");
        std::process::exit(1);
    }
    println!("ok: sanitize_own_inject_flags strips INJECTED (unit)");

    #[cfg(not(windows))]
    {
        println!("SKIP: live hook probe (non-Windows)");
        return;
    }

    #[cfg(windows)]
    {
        use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
            KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, VIRTUAL_KEY,
        };
        use windows::Win32::UI::WindowsAndMessaging::{
            CallNextHookEx, DispatchMessageW, PeekMessageW, SetWindowsHookExW, TranslateMessage,
            UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, MSG, PM_REMOVE, WH_KEYBOARD_LL, WM_QUIT,
        };

        // Start app sanitizer hook first, then bump it to front after we install the probe.
        remote_bridge_hub_lib::bridges::xiaomi::special_keys::set_hook_enabled(true);
        remote_bridge_hub_lib::bridges::xiaomi::special_keys::start_special_key_hook();
        thread::sleep(Duration::from_millis(80));

        static SEEN_FLAGS: AtomicU32 = AtomicU32::new(0xFFFF_FFFF);
        static SEEN_VK: AtomicU32 = AtomicU32::new(0);

        unsafe extern "system" fn probe(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
            if code >= 0 {
                let info = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
                if info.dwExtraInfo == EXTRA_INFO && info.vkCode == 0xA5 {
                    SEEN_FLAGS.store(info.flags.0, Ordering::Release);
                    SEEN_VK.store(info.vkCode, Ordering::Release);
                }
            }
            CallNextHookEx(HHOOK(std::ptr::null_mut()), code, wparam, lparam)
        }

        // Install probe FIRST (older), then bump our sanitizer to chain head (newer = first called).
        let probe_hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(probe), None, 0) };
        let Ok(probe_hook) = probe_hook else {
            eprintln!("RED: failed to install probe hook");
            std::process::exit(1);
        };

        remote_bridge_hub_lib::bridges::xiaomi::special_keys::bump_hook_to_front();
        thread::sleep(Duration::from_millis(40));

        // Inject Right Alt via SendInput + EXTRA_INFO (same as voice fallback path)
        let vk = 0xA5u16;
        let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as u16;
        let scan = if scan == 0 { 0x38 } else { scan };
        let down = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: scan,
                    dwFlags: KEYEVENTF_EXTENDEDKEY,
                    time: 0,
                    dwExtraInfo: EXTRA_INFO,
                },
            },
        };
        let up = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: scan,
                    dwFlags: KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: EXTRA_INFO,
                },
            },
        };
        unsafe {
            let _ = SendInput(&[down, up], std::mem::size_of::<INPUT>() as i32);
        }

        // Pump messages briefly so hooks run
        let deadline = std::time::Instant::now() + Duration::from_millis(300);
        let mut msg = MSG::default();
        while std::time::Instant::now() < deadline {
            unsafe {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    if msg.message == WM_QUIT {
                        break;
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            if SEEN_VK.load(Ordering::Acquire) == 0xA5 {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }

        unsafe {
            let _ = UnhookWindowsHookEx(probe_hook);
        }
        remote_bridge_hub_lib::bridges::xiaomi::special_keys::stop_special_key_hook();

        let seen_vk = SEEN_VK.load(Ordering::Acquire);
        let seen_flags = SEEN_FLAGS.load(Ordering::Acquire);
        if seen_vk != 0xA5 {
            eprintln!(
                "RED: probe never saw EXTRA_INFO Right Alt (vk={seen_vk:#x}). Hook chain order may differ."
            );
            std::process::exit(1);
        }
        if seen_flags & LLKHF_INJECTED != 0 {
            eprintln!(
                "RED: probe still saw LLKHF_INJECTED (flags={seen_flags:#x}) — IME would filter this"
            );
            std::process::exit(1);
        }
        println!(
            "GREEN: probe saw Right Alt EXTRA_INFO with INJECTED cleared (flags={seen_flags:#x})"
        );
    }
}
