//! T1 Consumer HID Raw Input — 对齐 Python `raw_input_bridge` 的 listen_consumer 路径
//!
//! 注册：
//! - Consumer Control `0x0C/0x01`
//! - System Control `0x01/0x80`（电源/睡眠，常不走 Consumer）
//! - 可选 Keyboard `0x01/0x06`、Mouse `0x01/0x02`（探测空鼠键）
//!
//! 解析 HID report；附带设备路径供 VID/PID 过滤。
//!
//! **局限（吞搜索）：** Consumer TLC 为 Shared，`RIDEV_INPUTSINK` 仅并行观察，
//! **不能**阻止系统 consumer/shell 处理 AC Search。`RIDEV_NOLEGACY` 只适用于
//! 鼠标/键盘 TLC，且是全局的——禁止用来「关掉」搜索。本模块对 `02-21-02`
//! 只做最早 arm + 踢 L3 mop-up（`on_ac_search_hid_seen`）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::sync::LazyLock;

/// 上一笔非零 Consumer HID（device, event_id），全零报告时合成 pressed=false
static LAST_CONSUMER_HID: LazyLock<Mutex<Option<(String, String)>>> =
    LazyLock::new(|| Mutex::new(None));

#[cfg(target_os = "windows")]
#[allow(non_snake_case, non_camel_case_types, dead_code)]
mod win32 {
    use std::ffi::c_void;
    pub type HWND = *mut c_void;
    pub type HMODULE = *mut c_void;
    pub type HRAWINPUT = *mut c_void;
    pub type ATOM = u16;
    pub type LRESULT = isize;
    pub type WPARAM = usize;
    pub type LPARAM = isize;
    pub type LPVOID = *mut c_void;
    pub type DWORD = u32;
    pub type UINT = u32;
    pub type LONG_PTR = isize;
    pub type ULONG_PTR = usize;

    pub const WM_INPUT: u32 = 0x00FF;
    pub const WM_QUIT: u32 = 0x0012;
    pub const HWND_MESSAGE: isize = -3;
    pub const GWLP_USERDATA: i32 = -21;
    pub const RIDEV_INPUTSINK: DWORD = 0x100;
    pub const RID_INPUT: DWORD = 0x10000003;
    pub const RIDI_DEVICENAME: UINT = 0x20000007;
    pub const RIM_TYPEMOUSE: DWORD = 0;
    pub const RIM_TYPEKEYBOARD: DWORD = 1;
    pub const RIM_TYPEHID: DWORD = 2;
    pub const RI_KEY_BREAK: u16 = 1;
    pub const RI_MOUSE_LEFT_BUTTON_DOWN: u16 = 0x0001;
    pub const RI_MOUSE_LEFT_BUTTON_UP: u16 = 0x0002;
    pub const RI_MOUSE_RIGHT_BUTTON_DOWN: u16 = 0x0004;
    pub const RI_MOUSE_RIGHT_BUTTON_UP: u16 = 0x0008;
    pub const RI_MOUSE_MIDDLE_BUTTON_DOWN: u16 = 0x0010;
    pub const RI_MOUSE_MIDDLE_BUTTON_UP: u16 = 0x0020;

    #[repr(C)]
    pub struct RAWINPUTDEVICE {
        pub usUsagePage: u16,
        pub usUsage: u16,
        pub dwFlags: DWORD,
        pub hwndTarget: HWND,
    }

    #[repr(C)]
    pub struct RAWINPUTHEADER {
        pub dwType: DWORD,
        pub dwSize: DWORD,
        pub hDevice: *mut c_void,
        pub wParam: usize,
    }

    #[derive(Copy, Clone)]
    #[repr(C)]
    pub struct RAWKEYBOARD {
        pub MakeCode: u16,
        pub Flags: u16,
        pub Reserved: u16,
        pub VKey: u16,
        pub Message: UINT,
        pub ExtraInformation: ULONG_PTR,
    }

    #[repr(C)]
    pub struct MSG {
        pub hwnd: HWND,
        pub message: UINT,
        pub wParam: WPARAM,
        pub lParam: LPARAM,
        pub time: DWORD,
        pub pt: POINT,
    }

    #[repr(C)]
    pub struct POINT {
        pub x: i32,
        pub y: i32,
    }

    #[repr(C)]
    pub struct WNDCLASSEXW {
        pub cbSize: UINT,
        pub style: UINT,
        pub lpfnWndProc: Option<unsafe extern "system" fn(HWND, UINT, WPARAM, LPARAM) -> LRESULT>,
        pub cbClsExtra: i32,
        pub cbWndExtra: i32,
        pub hInstance: *mut c_void,
        pub hIcon: *mut c_void,
        pub hCursor: *mut c_void,
        pub hbrBackground: *mut c_void,
        pub lpszMenuName: *const u16,
        pub lpszClassName: *const u16,
        pub hIconSm: *mut c_void,
    }

    extern "system" {
        pub fn GetModuleHandleW(lpModuleName: *const u16) -> HMODULE;
        pub fn RegisterClassExW(lpWndClass: *const WNDCLASSEXW) -> ATOM;
        pub fn CreateWindowExW(
            dwExStyle: DWORD,
            lpClassName: *const u16,
            lpWindowName: *const u16,
            dwStyle: DWORD,
            x: i32,
            y: i32,
            nWidth: i32,
            nHeight: i32,
            hWndParent: HWND,
            hMenu: *mut c_void,
            hInstance: *mut c_void,
            lpParam: LPVOID,
        ) -> HWND;
        pub fn DestroyWindow(hWnd: HWND) -> i32;
        pub fn DefWindowProcW(hWnd: HWND, Msg: UINT, wParam: WPARAM, lParam: LPARAM) -> LRESULT;
        pub fn RegisterRawInputDevices(
            pRawInputDevices: *const RAWINPUTDEVICE,
            uiNumDevices: UINT,
            cbSize: UINT,
        ) -> i32;
        pub fn GetRawInputData(
            hRawInput: HRAWINPUT,
            uiCommand: UINT,
            pData: LPVOID,
            pcbSize: *mut UINT,
            cbSizeHeader: UINT,
        ) -> UINT;
        pub fn GetRawInputDeviceInfoW(
            hDevice: *mut c_void,
            uiCommand: UINT,
            pData: LPVOID,
            pcbSize: *mut UINT,
        ) -> UINT;
        pub fn GetMessageW(
            lpMsg: *mut MSG,
            hWnd: HWND,
            wMsgFilterMin: UINT,
            wMsgFilterMax: UINT,
        ) -> i32;
        pub fn TranslateMessage(lpMsg: *const MSG) -> i32;
        pub fn DispatchMessageW(lpMsg: *const MSG) -> LRESULT;
        pub fn PostThreadMessageW(idThread: DWORD, Msg: UINT, wParam: WPARAM, lParam: LPARAM)
            -> i32;
        pub fn GetCurrentThreadId() -> DWORD;
        pub fn SetWindowLongPtrW(hWnd: HWND, nIndex: i32, dwNewLong: LONG_PTR) -> LONG_PTR;
        pub fn GetWindowLongPtrW(hWnd: HWND, nIndex: i32) -> LONG_PTR;
    }
}

/// 对齐 Python event：hid report 或键盘
#[derive(Debug, Clone)]
pub struct ConsumerRawEvent {
    /// `hid:02-CF-00` 或 `kbd:VK_0D`
    pub event_id: String,
    pub device_name: String,
    pub pressed: bool,
    pub hid_report_hex: Option<String>,
}

/// 将 report 字节格式化为 Python 风格 `02-CF-00`
pub fn format_hid_report_hex(raw: &[u8]) -> String {
    raw.iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join("-")
}

/// 设备路径是否匹配任一 token（大小写不敏感子串）
pub fn device_matches(device_name: &str, tokens: &[String]) -> bool {
    if tokens.is_empty() {
        return true;
    }
    let low = device_name.to_ascii_lowercase();
    if tokens.iter().any(|t| low.contains(&t.to_ascii_lowercase())) {
        return true;
    }
    // 兼容路径里写成 VID_1915&PID_1025 / vid_1915&pid_1025 / 1915_1025
    tokens.iter().any(|t| {
        let t = t.to_ascii_lowercase().replace('&', "_");
        let compact = low.replace('&', "_");
        compact.contains(&t) || {
            let digits: String = t.chars().filter(|c| c.is_ascii_hexdigit() || *c == '_').collect();
            !digits.is_empty() && compact.contains(&digits)
        }
    })
}

/// 是否像 T1 USB 接收器或 BLE HOGP（用于 Raw HID 落盘 / 抬起合成 / 诊断）
pub fn looks_like_t1_device(device_name: &str) -> bool {
    let low = device_name.to_ascii_lowercase();
    let usb = low.contains("vid_1915") && low.contains("pid_1025");
    let ble = ((low.contains("01620a") || low.contains("1620a"))
        && (low.contains("0407") || low.contains("pid&0407") || low.contains("pid_0407")))
        || low.contains("dev_vid&01620a")
        || low.contains("t1-remote")
        || low.contains("t1_remote")
        || (low.contains("bthledevice")
            && (low.contains("01620a") || low.contains("1620a") || low.contains("0407")));
    usb || ble
}

/// 电源/系统控制探测：Consumer Power `0x30`、System Power/Sleep/Wake `0x81/82/83`
pub fn looks_power_probe_hex(hex: &str) -> bool {
    let u = hex.to_ascii_uppercase();
    if u.contains("02-30") || u.starts_with("30-") || u == "30" {
        return true;
    }
    // 短报告里出现系统控制 usage（避免在长报告里误伤普通字节）
    let parts: Vec<&str> = u.split('-').filter(|p| !p.is_empty()).collect();
    if parts.len() <= 4 {
        parts.iter().any(|b| matches!(*b, "30" | "81" | "82" | "83"))
    } else {
        false
    }
}

/// 电源相关键盘 VK：Sleep / OEM FF（小米电源残留）/ 少见 0x5E
pub fn looks_power_probe_vk(vk: u16) -> bool {
    matches!(vk, 0x5F | 0xFF | 0x5E)
}

type EventCallback = Arc<Mutex<dyn FnMut(ConsumerRawEvent) + Send + 'static>>;

/// 进程内单例 Raw Input：USB/BLE 共用一个 `RegisterRawInputDevices`。
/// Windows 对同一 usage 后注册覆盖前注册；BLE 重连反复 start/stop 会弄死仍在跑的 USB 收键。
struct SharedRawHub {
    listeners: std::collections::HashMap<u64, EventCallback>,
    next_id: u64,
    running: Arc<AtomicBool>,
    thread_handle: Option<thread::JoinHandle<()>>,
    thread_id: Arc<Mutex<Option<u32>>>,
    listen_keyboard: bool,
}

impl SharedRawHub {
    fn new() -> Self {
        Self {
            listeners: std::collections::HashMap::new(),
            next_id: 1,
            running: Arc::new(AtomicBool::new(false)),
            thread_handle: None,
            thread_id: Arc::new(Mutex::new(None)),
            listen_keyboard: false,
        }
    }
}

static SHARED_RAW_HUB: LazyLock<Mutex<SharedRawHub>> =
    LazyLock::new(|| Mutex::new(SharedRawHub::new()));

fn hub_register<F>(listen_keyboard: bool, callback: F) -> Result<u64, String>
where
    F: FnMut(ConsumerRawEvent) + Send + 'static,
{
    let mut hub = SHARED_RAW_HUB
        .lock()
        .map_err(|_| "SharedRawHub lock poisoned".to_string())?;
    let id = hub.next_id;
    hub.next_id = hub.next_id.wrapping_add(1).max(1);
    hub.listeners
        .insert(id, Arc::new(Mutex::new(callback)) as EventCallback);
    hub.listen_keyboard |= listen_keyboard;

    if hub.thread_handle.is_none() {
        hub.running.store(true, Ordering::SeqCst);
        let running = Arc::clone(&hub.running);
        let thread_id = Arc::clone(&hub.thread_id);
        let listen_kb = hub.listen_keyboard;
        let fanout: EventCallback = Arc::new(Mutex::new(move |ev: ConsumerRawEvent| {
            let cbs: Vec<EventCallback> = SHARED_RAW_HUB
                .lock()
                .map(|h| h.listeners.values().cloned().collect())
                .unwrap_or_default();
            for cb in cbs {
                if let Ok(mut f) = cb.lock() {
                    f(ev.clone());
                }
            }
        }));
        hub.thread_handle = Some(thread::spawn(move || {
            #[cfg(target_os = "windows")]
            {
                if let Ok(mut g) = thread_id.lock() {
                    *g = Some(unsafe { win32::GetCurrentThreadId() });
                }
                consumer_raw_thread(running, fanout, listen_kb);
                if let Ok(mut g) = thread_id.lock() {
                    *g = None;
                }
            }
            #[cfg(not(target_os = "windows"))]
            {
                log::warn!("ConsumerRawInput only on Windows");
                let _ = (running, fanout, listen_kb);
            }
        }));
        log::info!(
            "ConsumerRawInput shared hub started listen_keyboard={listen_kb} listeners=1"
        );
    } else {
        log::info!(
            "ConsumerRawInput shared hub add listener id={id} total={}",
            hub.listeners.len()
        );
    }
    Ok(id)
}

fn hub_unregister(id: u64) {
    let join_handle = {
        let Ok(mut hub) = SHARED_RAW_HUB.lock() else {
            return;
        };
        hub.listeners.remove(&id);
        let remaining = hub.listeners.len();
        if remaining > 0 {
            log::info!("ConsumerRawInput shared hub remove id={id} remaining={remaining}");
            return;
        }
        // 最后一个监听者离开：停掉唯一的 Raw Input 线程（join 必须在锁外，避免 fanout 死锁）
        hub.running.store(false, Ordering::SeqCst);
        #[cfg(target_os = "windows")]
        {
            if let Ok(g) = hub.thread_id.lock() {
                if let Some(tid) = *g {
                    unsafe {
                        win32::PostThreadMessageW(tid, win32::WM_QUIT, 0, 0);
                    }
                }
            }
        }
        hub.listen_keyboard = false;
        hub.thread_handle.take()
    };
    if let Some(h) = join_handle {
        let _ = h.join();
        log::info!("ConsumerRawInput shared hub stopped (no listeners)");
    }
}

pub struct ConsumerRawInput {
    running: Arc<AtomicBool>,
    hub_id: Option<u64>,
    listen_keyboard: bool,
}

impl ConsumerRawInput {
    pub fn new(listen_keyboard: bool) -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            hub_id: None,
            listen_keyboard,
        }
    }

    pub fn start<F>(&mut self, callback: F) -> Result<(), String>
    where
        F: FnMut(ConsumerRawEvent) + Send + 'static,
    {
        if self.hub_id.is_some() {
            return Err("ConsumerRawInput 已在运行".into());
        }
        let id = hub_register(self.listen_keyboard, callback)?;
        self.hub_id = Some(id);
        self.running.store(true, Ordering::SeqCst);
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(id) = self.hub_id.take() {
            hub_unregister(id);
        }
        self.running.store(false, Ordering::SeqCst);
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

impl Drop for ConsumerRawInput {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(target_os = "windows")]
fn get_device_name(h_device: *mut std::ffi::c_void) -> String {
    use win32::*;
    use std::ptr;
    if h_device.is_null() {
        return String::new();
    }
    let mut size: UINT = 0;
    unsafe {
        GetRawInputDeviceInfoW(h_device, RIDI_DEVICENAME, ptr::null_mut(), &mut size);
    }
    if size == 0 {
        return String::new();
    }
    let mut buf: Vec<u16> = vec![0u16; size as usize];
    let got = unsafe {
        GetRawInputDeviceInfoW(
            h_device,
            RIDI_DEVICENAME,
            buf.as_mut_ptr() as LPVOID,
            &mut size,
        )
    };
    if got == UINT::MAX || got == 0 {
        return String::new();
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

/// 从 GetRawInputData 缓冲解析 HID report（header 之后为 RAWHID：size, count, bytes）
pub fn parse_hid_report_from_raw_buffer(buf: &[u8], header_size: usize) -> Option<Vec<u8>> {
    if buf.len() < header_size + 8 {
        return None;
    }
    let size_hid =
        u32::from_le_bytes(buf[header_size..header_size + 4].try_into().ok()?) as usize;
    let count =
        u32::from_le_bytes(buf[header_size + 4..header_size + 8].try_into().ok()?) as usize;
    let start = header_size + 8;
    let end = start.saturating_add(size_hid.saturating_mul(count));
    if end > buf.len() || size_hid == 0 || count == 0 {
        return None;
    }
    Some(buf[start..end].to_vec())
}

/// USB HID boot keyboard → 首个非零 keycode 转 VK（Usage Page 0x07）
pub fn boot_keyboard_vk_from_hid(report: &[u8]) -> Option<u16> {
    // 标准 8 字节：mod, reserved, key[6]；偶发带 report id 前缀共 9 字节
    let keys = if report.len() == 8 {
        &report[2..]
    } else if report.len() == 9 && report[0] <= 0x0A {
        &report[3..]
    } else {
        return None;
    };
    let usage = *keys.iter().find(|&&b| b != 0)?;
    hid_keyboard_usage_to_vk(usage)
}

fn hid_keyboard_usage_to_vk(usage: u8) -> Option<u16> {
    Some(match usage {
        0x04..=0x1D => 0x41 + (usage - 0x04) as u16, // A-Z
        0x1E..=0x26 => 0x31 + (usage - 0x1E) as u16, // 1-9
        0x27 => 0x30,                                // 0
        0x28 => 0x0D,                                // Enter
        0x29 => 0x1B,                                // Escape
        0x2A => 0x08,                                // Backspace
        0x2B => 0x09,                                // Tab
        0x2C => 0x20,                                // Space
        0x4F => 0x27,                                // Right
        0x50 => 0x25,                                // Left
        0x51 => 0x28,                                // Down
        0x52 => 0x26,                                // Up
        0x65 => 0x5D,                                // Application (Menu)
        _ => return None,
    })
}

#[cfg(target_os = "windows")]
fn raw_input_header_size() -> usize {
    std::mem::size_of::<win32::RAWINPUTHEADER>()
}

#[cfg(not(target_os = "windows"))]
fn raw_input_header_size() -> usize {
    24 // x64 RAWINPUTHEADER
}

#[cfg(target_os = "windows")]
fn consumer_raw_thread(running: Arc<AtomicBool>, callback: EventCallback, listen_keyboard: bool) {
    use win32::*;
    use std::mem;
    use std::ptr;

    let class_name: Vec<u16> = "T1ConsumerRawInputClass\0".encode_utf16().collect();
    let window_name: Vec<u16> = "T1ConsumerRawInput\0".encode_utf16().collect();
    let hinstance = unsafe { GetModuleHandleW(ptr::null()) };

    let mut wc: WNDCLASSEXW = unsafe { mem::zeroed() };
    wc.cbSize = mem::size_of::<WNDCLASSEXW>() as u32;
    wc.lpfnWndProc = Some(consumer_wndproc);
    wc.hInstance = hinstance;
    wc.lpszClassName = class_name.as_ptr();

    // 允许重复启动：忽略已注册错误
    let _ = unsafe { RegisterClassExW(&wc) };

    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            window_name.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE as HWND,
            ptr::null_mut(),
            hinstance,
            ptr::null_mut(),
        )
    };
    if hwnd.is_null() {
        log::error!("T1 ConsumerRawInput CreateWindowExW failed");
        return;
    }

    let cb_raw = Arc::into_raw(Arc::new(callback));
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, cb_raw as LONG_PTR);
    }

    let mut devices = vec![
        RAWINPUTDEVICE {
            usUsagePage: 0x0C,
            usUsage: 0x01, // Consumer Control
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: hwnd,
        },
        // 电源键常走 System Control，不在 Consumer TLC 上
        RAWINPUTDEVICE {
            usUsagePage: 0x01,
            usUsage: 0x80, // System Control
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: hwnd,
        },
    ];
    if listen_keyboard {
        devices.push(RAWINPUTDEVICE {
            usUsagePage: 0x01,
            usUsage: 0x06, // Keyboard
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: hwnd,
        });
        // 空鼠/鼠标键探测（若键只切机内模式则仍无报告）
        devices.push(RAWINPUTDEVICE {
            usUsagePage: 0x01,
            usUsage: 0x02, // Mouse
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: hwnd,
        });
    }

    if unsafe {
        RegisterRawInputDevices(
            devices.as_ptr(),
            devices.len() as u32,
            mem::size_of::<RAWINPUTDEVICE>() as u32,
        )
    } == 0
    {
        log::error!("T1 RegisterRawInputDevices (consumer+system) failed");
        unsafe {
            let cb_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
            if cb_ptr != 0 {
                let _ = Arc::from_raw(cb_ptr as *const EventCallback);
            }
            DestroyWindow(hwnd);
        }
        return;
    }

    log::info!(
        "T1 Consumer RawInput registered (0x0C/0x01 + SystemControl 0x01/0x80{})",
        if listen_keyboard {
            " + keyboard + mouse"
        } else {
            ""
        }
    );

    let mut msg: MSG = unsafe { mem::zeroed() };
    loop {
        if !running.load(Ordering::SeqCst) {
            break;
        }
        let ret = unsafe { GetMessageW(&mut msg, ptr::null_mut(), 0, 0) };
        if ret <= 0 {
            break;
        }
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    unsafe {
        let cb_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
        if cb_ptr != 0 {
            let _ = Arc::from_raw(cb_ptr as *const EventCallback);
        }
        DestroyWindow(hwnd);
    }
    log::info!("T1 Consumer RawInput message loop exited");

    unsafe extern "system" fn consumer_wndproc(
        hwnd: HWND,
        msg: UINT,
        w_param: WPARAM,
        l_param: LPARAM,
    ) -> LRESULT {
        if msg == WM_INPUT {
            // 录入吞键期间不做任何 Raw Input 处理（含 GetRawInputDeviceInfo），减轻与 LL 钩子争用
            if crate::bridges::shared::shortcut_capture::is_swallow_active() {
                return 0;
            }
            let cb_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
            if cb_ptr == 0 {
                return 0;
            }
            let cb_arc = &*(cb_ptr as *const EventCallback);
            let mut size: UINT = 0;
            let header_size = raw_input_header_size() as UINT;
            if GetRawInputData(
                l_param as HRAWINPUT,
                RID_INPUT,
                ptr::null_mut(),
                &mut size,
                header_size,
            ) != 0
                || size == 0
            {
                return 0;
            }
            let mut buf: Vec<u8> = vec![0u8; size as usize];
            let written = GetRawInputData(
                l_param as HRAWINPUT,
                RID_INPUT,
                buf.as_mut_ptr() as LPVOID,
                &mut size,
                header_size,
            );
            if written != size {
                return 0;
            }
            let header = &*(buf.as_ptr() as *const RAWINPUTHEADER);
            let device_name = get_device_name(header.hDevice);
            let t1ish = looks_like_t1_device(&device_name);

            if header.dwType == RIM_TYPEHID {
                if let Some(report) =
                    parse_hid_report_from_raw_buffer(&buf, header_size as usize)
                {
                    let all_zero = report.iter().all(|&b| b == 0);
                    let hex = format_hid_report_hex(&report);
                    let powerish = looks_power_probe_hex(&hex);
                    // 全零 = Consumer HID 抬起：回放上一笔（语音脉冲抬起由 runtime 忽略，不结束闩锁）
                    if all_zero {
                        if let Some((prev_dev, prev_event)) =
                            LAST_CONSUMER_HID.lock().ok().and_then(|mut g| g.take())
                        {
                            if t1ish || looks_like_t1_device(&prev_dev) || powerish {
                                log::info!(
                                    "T1 raw HID release device={} was={}",
                                    device_name,
                                    prev_event
                                );
                                if let Ok(mut cb) = cb_arc.lock() {
                                    cb(ConsumerRawEvent {
                                        event_id: prev_event,
                                        device_name: device_name.clone(),
                                        pressed: false,
                                        hid_report_hex: Some(hex),
                                    });
                                }
                            }
                        }
                    } else if t1ish
                        || powerish
                        || device_name.is_empty()
                        || crate::bridges::t1::ble_keys::is_running()
                    {
                        // BLE 按键会话中：即使 Raw 设备名偶发不像 T1，也要早处理 Consumer
                        //（尤其 02-21-02 AC Search，否则 APPCOMMAND 会先弹出 Browser Search）
                        if t1ish {
                            log::info!(
                                "T1 raw HID device={} report=hid:{}",
                                device_name,
                                hex
                            );
                        } else {
                            log::info!(
                                "T1 probe HID (power/system/ble-session) device={} report=hid:{}",
                                if device_name.is_empty() {
                                    "?"
                                } else {
                                    &device_name
                                },
                                hex
                            );
                        }
                        crate::bridges::t1::native_suppress::maybe_arm_home_from_hid_hex(&hex);
                        let hex_up = hex.to_ascii_uppercase();
                        if hex_up.starts_with("02-21-02") || hex_up.contains("-21-02") {
                            // AC Search：立刻吞 0xAA + 关搜索（不等 LL / APPCOMMAND）
                            crate::bridges::t1::native_suppress::on_ac_search_hid_seen();
                        }
                        if hex_up.starts_with("02-23-02") || hex_up.contains("-23-02") {
                            crate::bridges::t1::native_suppress::inject_after_consumer_hid(0xAC);
                        }
                        if hex_up.starts_with("02-24-02") || hex_up.contains("-24-02") {
                            crate::bridges::t1::native_suppress::inject_after_consumer_hid(0xA6);
                        }
                        if hex_up.starts_with("02-E2") || hex_up.contains("-E2-") {
                            crate::bridges::t1::native_suppress::inject_after_consumer_hid(0xAD);
                        }
                        // 音量 ±：对齐 Home/删除/静音，HID 落盘即补映射（不依赖设备名 token）
                        if hex_up.starts_with("02-E9") || hex_up.contains("-E9-") {
                            crate::bridges::t1::native_suppress::inject_after_consumer_hid(0xAF);
                        }
                        if hex_up.starts_with("02-EA") || hex_up.contains("-EA-") {
                            crate::bridges::t1::native_suppress::inject_after_consumer_hid(0xAE);
                        }
                        // Boot keyboard HID（8 字节）：部分接收器不以 RIM_TYPEKEYBOARD 上报
                        if let Some(vk) = boot_keyboard_vk_from_hid(&report) {
                            if let Ok(mut cb) = cb_arc.lock() {
                                cb(ConsumerRawEvent {
                                    event_id: format!("kbd:VK_{vk:02X}"),
                                    device_name: device_name.clone(),
                                    pressed: true,
                                    hid_report_hex: Some(hex),
                                });
                            }
                        } else {
                            let event_id = format!("hid:{hex}");
                            if let Ok(mut g) = LAST_CONSUMER_HID.lock() {
                                *g = Some((device_name.clone(), event_id.clone()));
                            }
                            if let Ok(mut cb) = cb_arc.lock() {
                                cb(ConsumerRawEvent {
                                    event_id,
                                    device_name: device_name.clone(),
                                    pressed: true,
                                    hid_report_hex: Some(hex),
                                });
                            }
                        }
                    } else if let Some(vk) = boot_keyboard_vk_from_hid(&report) {
                        if let Ok(mut cb) = cb_arc.lock() {
                            cb(ConsumerRawEvent {
                                event_id: format!("kbd:VK_{vk:02X}"),
                                device_name: device_name.clone(),
                                pressed: true,
                                hid_report_hex: Some(hex),
                            });
                        }
                    } else {
                        log::debug!(
                            "T1 probe HID foreign device={} report=hid:{}",
                            device_name,
                            hex
                        );
                        if let Ok(mut cb) = cb_arc.lock() {
                            // 非 T1 consumer：仍上报按下，供 foreign 路径（若有）
                            cb(ConsumerRawEvent {
                                event_id: format!("hid:{hex}"),
                                device_name: device_name.clone(),
                                pressed: true,
                                hid_report_hex: Some(hex),
                            });
                        }
                    }
                }
            } else if header.dwType == RIM_TYPEKEYBOARD {
                // keyboard payload starts after header；手动读 VKey，避免对齐差异
                let kb_off = header_size as usize;
                if buf.len() >= kb_off + 8 {
                    let make = u16::from_le_bytes([buf[kb_off], buf[kb_off + 1]]);
                    let flags = u16::from_le_bytes([buf[kb_off + 2], buf[kb_off + 3]]);
                    let vkey = u16::from_le_bytes([buf[kb_off + 6], buf[kb_off + 7]]);
                    let pressed = (flags & RI_KEY_BREAK) == 0;
                    let powerish = looks_power_probe_vk(vkey) || make == 0x5E;
                    if t1ish {
                        log::info!(
                            "T1 raw KBD device={} vk=0x{vkey:02X} sc=0x{make:02X} pressed={pressed}",
                            device_name
                        );
                    } else if powerish {
                        log::info!(
                            "T1 probe KBD (power?) device={} vk=0x{vkey:02X} sc=0x{make:02X} pressed={pressed}",
                            if device_name.is_empty() {
                                "?"
                            } else {
                                &device_name
                            }
                        );
                    }
                    // 按下与抬起都回调：实体键盘 foreign 回放需要 keyup；T1 侧 runtime 会忽略抬起
                    if vkey != 0 {
                        if let Ok(mut cb) = cb_arc.lock() {
                            cb(ConsumerRawEvent {
                                event_id: format!("kbd:VK_{vkey:02X}"),
                                device_name,
                                pressed,
                                hid_report_hex: None,
                            });
                        }
                    }
                }
            } else if header.dwType == RIM_TYPEMOUSE {
                // Windows RAWMOUSE：usFlags@0，union.usButtonFlags@2（见 MSDN）
                let m_off = header_size as usize;
                if buf.len() >= m_off + 4 {
                    let btn_flags =
                        u16::from_le_bytes([buf[m_off + 2], buf[m_off + 3]]);
                    let mut events: Vec<(String, bool)> = Vec::new();
                    if (btn_flags & RI_MOUSE_LEFT_BUTTON_DOWN) != 0 {
                        events.push(("mouse:BTN_LEFT".into(), true));
                    }
                    if (btn_flags & RI_MOUSE_LEFT_BUTTON_UP) != 0 {
                        events.push(("mouse:BTN_LEFT".into(), false));
                    }
                    if (btn_flags & RI_MOUSE_RIGHT_BUTTON_DOWN) != 0 {
                        events.push(("mouse:BTN_RIGHT".into(), true));
                    }
                    if (btn_flags & RI_MOUSE_RIGHT_BUTTON_UP) != 0 {
                        events.push(("mouse:BTN_RIGHT".into(), false));
                    }
                    if (btn_flags & RI_MOUSE_MIDDLE_BUTTON_DOWN) != 0 {
                        events.push(("mouse:BTN_MIDDLE".into(), true));
                    }
                    if (btn_flags & RI_MOUSE_MIDDLE_BUTTON_UP) != 0 {
                        events.push(("mouse:BTN_MIDDLE".into(), false));
                    }
                    for (event_id, pressed) in events {
                        if t1ish {
                            log::info!(
                                "T1 raw MOUSE device={} {event_id} pressed={pressed}",
                                device_name
                            );
                        } else if device_name.is_empty() {
                            log::info!(
                                "T1 probe MOUSE device=? {event_id} pressed={pressed}"
                            );
                        } else {
                            // 非 T1 实体鼠标极多，避免刷屏
                            log::debug!(
                                "T1 probe MOUSE foreign device={} {event_id} pressed={pressed}",
                                device_name
                            );
                            continue;
                        }
                        if let Ok(mut cb) = cb_arc.lock() {
                            cb(ConsumerRawEvent {
                                event_id,
                                device_name: device_name.clone(),
                                pressed,
                                hid_report_hex: None,
                            });
                        }
                    }
                }
            }
            return 0;
        }
        DefWindowProcW(hwnd, msg, w_param, l_param)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_hid_matches_python_style() {
        assert_eq!(format_hid_report_hex(&[0x02, 0xCF, 0x00]), "02-CF-00");
        assert_eq!(format_hid_report_hex(&[0x02, 0xE9, 0x00]), "02-E9-00");
    }

    #[test]
    fn device_match_vid_pid_substring() {
        let path = r"\\?\HID#VID_1915&PID_1025&MI_01#7&abc#{" ;
        assert!(device_matches(
            path,
            &["VID_1915&PID_1025".into()]
        ));
        assert!(!device_matches(
            r"\\?\HID#VID_046D&PID_C52B",
            &["VID_1915&PID_1025".into()]
        ));
    }

    #[test]
    fn boot_keyboard_enter_and_arrows() {
        assert_eq!(
            boot_keyboard_vk_from_hid(&[0, 0, 0x28, 0, 0, 0, 0, 0]),
            Some(0x0D)
        );
        assert_eq!(
            boot_keyboard_vk_from_hid(&[0, 0, 0x52, 0, 0, 0, 0, 0]),
            Some(0x26)
        );
        assert_eq!(boot_keyboard_vk_from_hid(&[0x02, 0xCF, 0x00]), None);
    }

    #[test]
    fn power_probe_hex_consumer_and_system() {
        assert!(looks_power_probe_hex("02-30-00"));
        assert!(looks_power_probe_hex("01-82"));
        assert!(looks_power_probe_hex("82"));
        assert!(!looks_power_probe_hex("02-E9-00"));
        assert!(looks_power_probe_vk(0x5F));
        assert!(!looks_power_probe_vk(0x0D));
    }

    #[test]
    fn looks_like_t1_usb_and_ble() {
        assert!(looks_like_t1_device(
            r"\\?\HID#VID_1915&PID_1025&MI_01#7&abc#{"
        ));
        assert!(looks_like_t1_device(
            r"\\?\HID#Dev_VID&01620a_PID&0407#{"
        ));
        assert!(looks_like_t1_device(
            r"\\?\BTHLEDevice#dev_vid&01620a_pid&0407#12ac"
        ));
        assert!(looks_like_t1_device(
            r"\\?\HID#VID_1620A&PID_0407&MI_00#8&xyz"
        ));
        assert!(!looks_like_t1_device(r"\\?\HID#VID_046D&PID_C52B"));
    }

    #[test]
    fn parse_hid_buffer_layout() {
        let header_size = raw_input_header_size();
        let mut buf = vec![0u8; header_size + 8 + 3];
        buf[header_size..header_size + 4].copy_from_slice(&3u32.to_le_bytes());
        buf[header_size + 4..header_size + 8].copy_from_slice(&1u32.to_le_bytes());
        buf[header_size + 8] = 0x02;
        buf[header_size + 9] = 0xCF;
        buf[header_size + 10] = 0x00;
        let report = parse_hid_report_from_raw_buffer(&buf, header_size).expect("report");
        assert_eq!(format_hid_report_hex(&report), "02-CF-00");
    }
}
