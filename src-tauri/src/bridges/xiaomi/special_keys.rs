//! 对齐 Python `XiaomiSpecialKeyHook`：抑制遥控器原生气
//!
//! 仅在「刚收到同键 HID direct / ATVV 信号」时吞掉 Windows 翻译的原 VK。

use crate::bridges::xiaomi::key_mapping::{
    direct_signal_recent, disarm_voice_native_suppress, voice_native_suppress_active, EXTRA_INFO,
};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};
use std::time::Duration;

static RUNNING: AtomicBool = AtomicBool::new(false);
static HOOK_THREAD_ID: AtomicU32 = AtomicU32::new(0);
static HID_TAP_READY: AtomicBool = AtomicBool::new(false);
static HOOK_ENABLED: AtomicBool = AtomicBool::new(true);

#[cfg(target_os = "windows")]
static HOOK_PTR: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

/// HID Tap 已验证 IO（可捕获返回/音量）后置 true
pub fn set_hid_tap_ready(ready: bool) {
    HID_TAP_READY.store(ready, Ordering::Release);
    log::info!("XIAOMI SPECIAL KEY hid_tap_ready={ready}");
}

pub fn hid_tap_ready() -> bool {
    HID_TAP_READY.load(Ordering::Acquire)
}

pub fn set_hook_enabled(enabled: bool) {
    HOOK_ENABLED.store(enabled, Ordering::Release);
}

pub fn start_special_key_hook() {
    if !HOOK_ENABLED.load(Ordering::Acquire) {
        log::info!("XIAOMI SPECIAL KEY hook disabled by config");
        return;
    }
    if RUNNING.swap(true, Ordering::AcqRel) {
        return;
    }
    std::thread::Builder::new()
        .name("xiaomi-special-keys".into())
        .spawn(|| {
            #[cfg(target_os = "windows")]
            hook_loop();
            RUNNING.store(false, Ordering::Release);
            HOOK_THREAD_ID.store(0, Ordering::Release);
        })
        .ok();
    log::info!("XIAOMI SPECIAL KEY hook starting");
}

pub fn stop_special_key_hook() {
    HID_TAP_READY.store(false, Ordering::Release);
    if !RUNNING.swap(false, Ordering::AcqRel) {
        return;
    }
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};
        let tid = HOOK_THREAD_ID.load(Ordering::Acquire);
        if tid != 0 {
            unsafe {
                let _ = PostThreadMessageW(tid, WM_QUIT, None, None);
            }
        }
    }
    log::info!("XIAOMI SPECIAL KEY hook stop requested");
}

#[cfg(target_os = "windows")]
fn load_hook() -> windows::Win32::UI::WindowsAndMessaging::HHOOK {
    use windows::Win32::UI::WindowsAndMessaging::HHOOK;
    HHOOK(HOOK_PTR.load(Ordering::Acquire))
}

#[cfg(target_os = "windows")]
fn store_hook(h: windows::Win32::UI::WindowsAndMessaging::HHOOK) {
    HOOK_PTR.store(h.0, Ordering::Release);
}

#[cfg(target_os = "windows")]
fn hook_loop() {
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage,
        UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL,
    };

    unsafe extern "system" fn proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        let hook = load_hook();
        if code >= 0 {
            let info = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            let flags = info.flags.0;
            if info.dwExtraInfo == EXTRA_INFO || (flags & 0x10) != 0 {
                return CallNextHookEx(hook, code, wparam, lparam);
            }

            let vk = info.vkCode;
            let scan = info.scanCode;
            let msg = wparam.0 as u32;
            let down = msg == 0x0100 || msg == 0x0104;
            let up = msg == 0x0101 || msg == 0x0105;
            let tap_ready = HID_TAP_READY.load(Ordering::Acquire);

            // 对齐 Python：音量仅在 Tap 就绪后抑制；其它键在 recent 信号时抑制
            let suppress = match vk {
                0xAF if tap_ready
                    && direct_signal_recent("volume_up", Duration::from_millis(200)) =>
                {
                    Some("volume_up")
                }
                0xAE if tap_ready
                    && direct_signal_recent("volume_down", Duration::from_millis(200)) =>
                {
                    Some("volume_down")
                }
                0xAD if tap_ready
                    && (direct_signal_recent("volume_mute", Duration::from_millis(200))
                        || direct_signal_recent("mute", Duration::from_millis(200))) =>
                {
                    Some("volume_mute")
                }
                0xA6 if direct_signal_recent("back", Duration::from_millis(250)) => Some("back"),
                0x24 | 0xAC
                    if direct_signal_recent("home", Duration::from_millis(250)) =>
                {
                    Some("home")
                }
                0x5D if direct_signal_recent("menu", Duration::from_millis(250)) => Some("menu"),
                0x0D if direct_signal_recent("ok", Duration::from_millis(200)) => Some("ok"),
                0x25 if direct_signal_recent("left", Duration::from_millis(200))
                    || direct_signal_recent("dpad_left", Duration::from_millis(200)) =>
                {
                    Some("left")
                }
                0x27 if direct_signal_recent("right", Duration::from_millis(200))
                    || direct_signal_recent("dpad_right", Duration::from_millis(200)) =>
                {
                    Some("right")
                }
                0x26 if direct_signal_recent("up", Duration::from_millis(200))
                    || direct_signal_recent("dpad_up", Duration::from_millis(200)) =>
                {
                    Some("up")
                }
                0x28 if direct_signal_recent("down", Duration::from_millis(200))
                    || direct_signal_recent("dpad_down", Duration::from_millis(200)) =>
                {
                    Some("down")
                }
                // TV: OEM_3 + scan 0x29
                0xC0 if scan == 0x29 && direct_signal_recent("tv", Duration::from_millis(250)) => {
                    Some("tv")
                }
                // Power: Sleep / 0xFF / scan 0x5E
                0x5F | 0xFF if direct_signal_recent("power", Duration::from_millis(250)) => {
                    Some("power")
                }
                _ if scan == 0x5E && direct_signal_recent("power", Duration::from_millis(250)) => {
                    Some("power")
                }
                0x74
                    if voice_native_suppress_active()
                        || direct_signal_recent("voice", Duration::from_millis(120))
                        || direct_signal_recent("mic", Duration::from_millis(120)) =>
                {
                    // sticky：F5 抬起后解除，避免误伤用户真 F5；截止见 VOICE_F5_SUPPRESS_DEADLINE_MS
                    if up {
                        disarm_voice_native_suppress();
                    }
                    Some("voice")
                }
                _ => None,
            };

            if let Some(name) = suppress {
                if down || up {
                    log::info!("XIAOMI SPECIAL KEY {name} original_suppressed vk=0x{vk:02X}");
                    return LRESULT(1);
                }
            }
        }
        CallNextHookEx(hook, code, wparam, lparam)
    }

    unsafe {
        HOOK_THREAD_ID.store(GetCurrentThreadId(), Ordering::Release);
        let hook = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(proc), None, 0) {
            Ok(h) => h,
            Err(e) => {
                log::error!("SetWindowsHookExW failed: {e}");
                return;
            }
        };
        store_hook(hook);
        log::info!(
            "XIAOMI SPECIAL KEYS READY mapping=configurable \
             repeat=back,volume,direction suppress_original=device-correlated"
        );
        let mut msg = MSG::default();
        while RUNNING.load(Ordering::Acquire) {
            let ret = GetMessageW(&mut msg, None, 0, 0);
            if ret.0 == -1 || ret.0 == 0 {
                break;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        // 先清空再 Unhook，避免卸载窗口期回调读到悬空句柄语义
        let hook = load_hook();
        store_hook(HHOOK(std::ptr::null_mut()));
        if !hook.is_invalid() {
            let _ = UnhookWindowsHookEx(hook);
        }
    }
}
