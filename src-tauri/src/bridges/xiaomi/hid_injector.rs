//! 对齐 Python `virtual_hid_keyboard.py`：动态加载 WinUHid.dll，优先虚拟 HID，失败回退 SendInput

use parking_lot::Mutex;
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const KEYBOARD_REPORT_DESCRIPTOR: &[u8] = &[
    0x05, 0x01, 0x09, 0x06, 0xA1, 0x01, 0x05, 0x07, 0x19, 0xE0, 0x29, 0xE7, 0x15, 0x00, 0x25,
    0x01, 0x75, 0x01, 0x95, 0x08, 0x81, 0x02, 0x95, 0x01, 0x75, 0x08, 0x81, 0x01, 0x95, 0x05,
    0x75, 0x01, 0x05, 0x08, 0x19, 0x01, 0x29, 0x05, 0x91, 0x02, 0x95, 0x01, 0x75, 0x03, 0x91,
    0x01, 0x95, 0x06, 0x75, 0x08, 0x15, 0x00, 0x25, 0x73, 0x05, 0x07, 0x19, 0x00, 0x29, 0x73,
    0x81, 0x00, 0xC0,
];

const CONSUMER_REPORT_DESCRIPTOR: &[u8] = &[
    0x05, 0x0C, 0x09, 0x01, 0xA1, 0x01, 0x15, 0x00, 0x26, 0xFF, 0x03, 0x19, 0x00, 0x2A, 0xFF,
    0x03, 0x75, 0x10, 0x95, 0x01, 0x81, 0x00, 0xC0,
];

#[repr(C, packed)]
struct WinUHidDeviceConfig {
    supported_events: i32,
    vendor_id: u16,
    product_id: u16,
    version_number: u16,
    report_descriptor_length: u16,
    report_descriptor: *const u8,
    container_id: [u8; 16],
    instance_id: *const u16,
    hardware_ids: *const u16,
    read_report_period_us: u32,
}

type FnGetVersion = unsafe extern "system" fn() -> u32;
type FnCreateDevice = unsafe extern "system" fn(*const WinUHidDeviceConfig) -> *mut c_void;
type FnStartDevice =
    unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void) -> i32;
type FnSubmitReport = unsafe extern "system" fn(*mut c_void, *const u8, u32) -> i32;
type FnDestroyDevice = unsafe extern "system" fn(*mut c_void);

struct Api {
    _module: windows::Win32::Foundation::HMODULE,
    create: FnCreateDevice,
    start: FnStartDevice,
    submit: FnSubmitReport,
    destroy: FnDestroyDevice,
}

struct Devices {
    api: Api,
    keyboard: *mut c_void,
    consumer: *mut c_void,
}

#[cfg(target_os = "windows")]
impl Drop for Devices {
    fn drop(&mut self) {
        unsafe {
            if !self.keyboard.is_null() {
                (self.api.destroy)(self.keyboard);
                self.keyboard = std::ptr::null_mut();
            }
            if !self.consumer.is_null() {
                (self.api.destroy)(self.consumer);
                self.consumer = std::ptr::null_mut();
            }
            let _ = windows::Win32::Foundation::FreeLibrary(self.api._module);
        }
    }
}

// 句柄仅在本模块内通过 Mutex 使用
unsafe impl Send for Devices {}

static INIT_TRIED: AtomicBool = AtomicBool::new(false);
static DEVICES: Mutex<Option<Devices>> = Mutex::new(None);

fn dll_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(p) = std::env::var("REMOTE_BRIDGE_WINUHID_DLL") {
        let t = p.trim();
        if !t.is_empty() {
            out.push(PathBuf::from(t));
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            out.push(dir.join("WinUHid.dll"));
            out.push(dir.join("assets").join("winuhid").join("WinUHid.dll"));
            out.push(
                dir.join("resources")
                    .join("assets")
                    .join("winuhid")
                    .join("WinUHid.dll"),
            );
        }
    }
    if let Some(manifest) = option_env!("CARGO_MANIFEST_DIR") {
        out.push(
            PathBuf::from(manifest)
                .join("assets")
                .join("winuhid")
                .join("WinUHid.dll"),
        );
    }
    out
}

fn consumer_usage(vk: u16) -> Option<u16> {
    match vk {
        0xAD => Some(0x00E2),
        0xAE => Some(0x00EA),
        0xAF => Some(0x00E9),
        0xB0 => Some(0x00B6),
        0xB1 => Some(0x00B5),
        0xB2 => Some(0x00B7),
        0xB3 => Some(0x00CD),
        0xB7 => Some(0x0192), // VK_LAUNCH_APP2 → AL Calculator
        _ => None,
    }
}

fn modifier_bit(vk: u16) -> Option<u8> {
    match vk {
        0x11 | 0xA2 => Some(0x01),
        0x10 | 0xA0 => Some(0x02),
        0x12 | 0xA4 => Some(0x04),
        0x5B => Some(0x08),
        0xA3 => Some(0x10),
        0xA1 => Some(0x20),
        0xA5 => Some(0x40),
        0x5C => Some(0x80),
        _ => None,
    }
}

fn vk_usage(vk: u16) -> Option<u8> {
    if modifier_bit(vk).is_some() {
        return None;
    }
    Some(match vk {
        0x08 => 0x2A,
        0x09 => 0x2B,
        0x0D => 0x28,
        0x1B => 0x29,
        0x20 => 0x2C,
        0x21 => 0x4B,
        0x22 => 0x4E,
        0x23 => 0x4D,
        0x24 => 0x4A,
        0x25 => 0x50,
        0x26 => 0x52,
        0x27 => 0x4F,
        0x28 => 0x51,
        0x2D => 0x49,
        0x2E => 0x4C,
        0x5D => 0x65,
        0x70..=0x7B => (vk - 0x70 + 0x3A) as u8,
        0x41..=0x5A => (vk - 0x41 + 0x04) as u8,
        0x31..=0x39 => (vk - 0x31 + 0x1E) as u8,
        0x30 => 0x27,
        _ => return None,
    })
}

/// 分步按下时的修饰键顺序：Ctrl → Shift → Win → Alt（Win 先于 Alt，避免先按 Alt 激活菜单栏）。
fn modifier_stagger_rank(vk: u16) -> u8 {
    match vk {
        0x11 | 0xA2 | 0xA3 => 0,
        0x10 | 0xA0 | 0xA1 => 1,
        0x5B | 0x5C => 2,
        0x12 | 0xA4 | 0xA5 => 3,
        _ => 4,
    }
}

fn split_modifiers_and_keys(vks: &[u16]) -> (Vec<u16>, Vec<u16>) {
    let mut mods = Vec::new();
    let mut keys = Vec::new();
    for &vk in vks {
        if modifier_bit(vk).is_some() {
            mods.push(vk);
        } else {
            keys.push(vk);
        }
    }
    mods.sort_by_key(|vk| modifier_stagger_rank(*vk));
    (mods, keys)
}

fn build_keyboard_report(vks: &[u16]) -> Result<[u8; 8], String> {
    let mut modifier = 0u8;
    let mut usages = Vec::new();
    for &vk in vks {
        if let Some(bit) = modifier_bit(vk) {
            modifier |= bit;
            continue;
        }
        let Some(usage) = vk_usage(vk) else {
            return Err(format!("WinUHid keyboard unsupported VK 0x{vk:02X}"));
        };
        if !usages.contains(&usage) {
            usages.push(usage);
        }
    }
    if usages.len() > 6 {
        return Err("WinUHid keyboard supports at most six non-modifier keys".into());
    }
    let mut report = [0u8; 8];
    report[0] = modifier;
    for (i, u) in usages.into_iter().enumerate() {
        report[2 + i] = u;
    }
    Ok(report)
}

/// 修复安装后允许重新探测 DLL/驱动
pub fn reset_and_retry() {
    {
        let mut g = DEVICES.lock();
        *g = None;
    }
    INIT_TRIED.store(false, Ordering::SeqCst);
    let _ = ensure_init();
}

/// 尝试初始化 WinUHid（幂等）；无 DLL/驱动时保持关闭
pub fn ensure_init() -> bool {
    if DEVICES.lock().is_some() {
        return true;
    }
    if INIT_TRIED.swap(true, Ordering::SeqCst) {
        return DEVICES.lock().is_some();
    }
    #[cfg(target_os = "windows")]
    {
        match windows_init() {
            Ok(dev) => {
                *DEVICES.lock() = Some(dev);
                log::info!("WinUHid ready (keyboard+consumer)");
                true
            }
            Err(e) => {
                log::warn!("WinUHid unavailable: {e}");
                false
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

pub fn is_available() -> bool {
    DEVICES.lock().is_some() || ensure_init()
}

/// 仅读缓存，不触发 LoadLibrary / CreateDevice（供 UI 轮询，避免卡 UI/IPC）
pub fn is_ready_cached() -> bool {
    DEVICES.lock().is_some()
}

/// 对齐 Python `tap`：优先 WinUHid，失败返回 false 由调用方 SendInput
pub fn tap_vks(vks: &[u16], hold_ms: u64) -> bool {
    if vks.is_empty() || !ensure_init() {
        return false;
    }
    let hold = hold_ms.clamp(20, 1000);
    if let Err(e) = press(vks) {
        log::warn!("WinUHid press failed: {e}");
        return false;
    }
    std::thread::sleep(Duration::from_millis(hold));
    if let Err(e) = release(vks) {
        log::warn!("WinUHid release failed: {e}");
        return false;
    }
    true
}

const MOD_STAGGER_MS: u64 = 4;

fn press_keyboard(dev: &Devices, vks: &[u16]) -> Result<(), String> {
    let (mods, keys) = split_modifiers_and_keys(vks);
    if mods.len() <= 1 {
        let report = build_keyboard_report(vks)?;
        return submit(dev, dev.keyboard, &report);
    }
    // 多修饰键：逐步叠加，模拟真实按键时序（千问 Win+Alt / Ctrl+Win 依赖此路径）
    for step in 1..=mods.len() {
        let partial: Vec<u16> = mods
            .iter()
            .take(step)
            .chain(keys.iter())
            .copied()
            .collect();
        let report = build_keyboard_report(&partial)?;
        submit(dev, dev.keyboard, &report)?;
        if step < mods.len() {
            std::thread::sleep(Duration::from_millis(MOD_STAGGER_MS));
        }
    }
    Ok(())
}

fn release_keyboard(dev: &Devices, vks: &[u16]) -> Result<(), String> {
    let (mods, keys) = split_modifiers_and_keys(vks);
    if mods.is_empty() {
        return submit(dev, dev.keyboard, &[0u8; 8]);
    }
    if mods.len() <= 1 {
        return submit(dev, dev.keyboard, &[0u8; 8]);
    }
    // 逆序松开：先放 Alt/Win，最后放 Ctrl，避免 Alt 单键残留触发菜单
    for held in (0..mods.len()).rev() {
        if held == 0 {
            submit(dev, dev.keyboard, &[0u8; 8])?;
        } else {
            let partial: Vec<u16> = mods
                .iter()
                .take(held)
                .chain(keys.iter())
                .copied()
                .collect();
            let report = build_keyboard_report(&partial)?;
            submit(dev, dev.keyboard, &report)?;
        }
        if held > 0 {
            std::thread::sleep(Duration::from_millis(MOD_STAGGER_MS));
        }
    }
    Ok(())
}

pub fn press(vks: &[u16]) -> Result<(), String> {
    ensure_init();
    let guard = DEVICES.lock();
    let Some(dev) = guard.as_ref() else {
        return Err("WinUHid not open".into());
    };
    if let Some(usage_vk) = vks.iter().copied().find(|vk| consumer_usage(*vk).is_some()) {
        if vks.len() != 1 {
            return Err("Consumer Control keys cannot be combined".into());
        }
        let report = consumer_usage(usage_vk).unwrap().to_le_bytes();
        return submit(dev, dev.consumer, &report);
    }
    press_keyboard(dev, vks)
}

pub fn release(vks: &[u16]) -> Result<(), String> {
    ensure_init();
    let guard = DEVICES.lock();
    let Some(dev) = guard.as_ref() else {
        return Err("WinUHid not open".into());
    };
    if vks.iter().any(|vk| consumer_usage(*vk).is_some()) {
        return submit(dev, dev.consumer, &[0, 0]);
    }
    let stagger = release_keyboard(dev, vks);
    // 必达：无论分步 release 是否成功，再发一次全零键盘报告，防止 Win 位残留
    let zero = submit(dev, dev.keyboard, &[0u8; 8]);
    stagger?;
    zero?;
    Ok(())
}

/// 强制全零键盘报告（panic release / sanitizer 用）
pub fn release_all() -> Result<(), String> {
    ensure_init();
    let guard = DEVICES.lock();
    let Some(dev) = guard.as_ref() else {
        return Err("WinUHid not open".into());
    };
    submit(dev, dev.keyboard, &[0u8; 8])
}

/// 单报告同时按下：多修饰键一次到位（微信要求 Ctrl+Win 同时按住才开条；
/// 跳过 press_keyboard 的分步时序）。// ponytail: 千问若需分步时序，改回 press/release
pub fn press_single(vks: &[u16]) -> Result<(), String> {
    ensure_init();
    let guard = DEVICES.lock();
    let dev = guard
        .as_ref()
        .ok_or_else(|| "WinUHid not open".to_string())?;
    let report = build_keyboard_report(vks)?;
    submit(dev, dev.keyboard, &report)
}

/// 单报告全部抬起（全零键盘报告）。
pub fn release_single(_vks: &[u16]) -> Result<(), String> {
    ensure_init();
    let guard = DEVICES.lock();
    let dev = guard
        .as_ref()
        .ok_or_else(|| "WinUHid not open".to_string())?;
    submit(dev, dev.keyboard, &[0u8; 8])
}

fn submit(dev: &Devices, handle: *mut c_void, report: &[u8]) -> Result<(), String> {
    unsafe {
        if (dev.api.submit)(handle, report.as_ptr(), report.len() as u32) == 0 {
            return Err("WinUHidSubmitInputReport failed".into());
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn windows_init() -> Result<Devices, String> {
    use windows::core::{PCSTR, PCWSTR};
    use windows::Win32::Foundation::FreeLibrary;
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

    let path = dll_candidates()
        .into_iter()
        .find(|p| p.is_file())
        .ok_or_else(|| "WinUHid.dll was not found".to_string())?;
    let wide: Vec<u16> = path
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let module = unsafe { LoadLibraryW(PCWSTR(wide.as_ptr())) }
        .map_err(|e| format!("LoadLibraryW WinUHid.dll: {e}"))?;

    unsafe fn proc<T>(module: windows::Win32::Foundation::HMODULE, name: &[u8]) -> Result<T, String> {
        let p = GetProcAddress(module, PCSTR(name.as_ptr()))
            .ok_or_else(|| format!("GetProcAddress missing {}", String::from_utf8_lossy(name)))?;
        Ok(std::mem::transmute_copy(&p))
    }

    let get_ver: FnGetVersion = unsafe { proc(module, b"WinUHidGetDriverInterfaceVersion\0")? };
    let create: FnCreateDevice = unsafe { proc(module, b"WinUHidCreateDevice\0")? };
    let start: FnStartDevice = unsafe { proc(module, b"WinUHidStartDevice\0")? };
    let submit: FnSubmitReport = unsafe { proc(module, b"WinUHidSubmitInputReport\0")? };
    let destroy: FnDestroyDevice = unsafe { proc(module, b"WinUHidDestroyDevice\0")? };

    let version = unsafe { get_ver() };
    if version < 1 {
        unsafe {
            let _ = FreeLibrary(module);
        }
        return Err(format!("WinUHid driver unavailable version={version}"));
    }

    let api = Api {
        _module: module,
        create,
        start,
        submit,
        destroy,
    };

    let keyboard = create_device(&api, KEYBOARD_REPORT_DESCRIPTOR, 1, "XiaomiRemoteBridgeKeyboard")?;
    let consumer = match create_device(&api, CONSUMER_REPORT_DESCRIPTOR, 2, "XiaomiRemoteBridgeConsumer")
    {
        Ok(h) => h,
        Err(e) => {
            unsafe { (api.destroy)(keyboard) };
            unsafe {
                let _ = FreeLibrary(module);
            }
            return Err(e);
        }
    };

    // 空报告复位
    let devices = Devices {
        api,
        keyboard,
        consumer,
    };
    let zero_kb = [0u8; 8];
    let zero_cc = [0u8; 2];
    let _ = unsafe { (devices.api.submit)(devices.keyboard, zero_kb.as_ptr(), 8) };
    let _ = unsafe { (devices.api.submit)(devices.consumer, zero_cc.as_ptr(), 2) };
    Ok(devices)
}

#[cfg(target_os = "windows")]
fn create_device(
    api: &Api,
    descriptor: &[u8],
    product_id: u16,
    instance_id: &str,
) -> Result<*mut c_void, String> {
    let mut wide: Vec<u16> = instance_id.encode_utf16().chain(std::iter::once(0)).collect();
    let config = WinUHidDeviceConfig {
        supported_events: 0,
        vendor_id: 0,
        product_id,
        version_number: 1,
        report_descriptor_length: descriptor.len() as u16,
        report_descriptor: descriptor.as_ptr(),
        container_id: [0u8; 16],
        instance_id: wide.as_mut_ptr(),
        hardware_ids: std::ptr::null(),
        read_report_period_us: 0,
    };
    let device = unsafe { (api.create)(&config) };
    if device.is_null() {
        return Err(format!("WinUHidCreateDevice failed for {instance_id}"));
    }
    if unsafe { (api.start)(device, std::ptr::null_mut(), std::ptr::null_mut()) } == 0 {
        unsafe { (api.destroy)(device) };
        return Err(format!("WinUHidStartDevice failed for {instance_id}"));
    }
    Ok(device)
}

// ---- 兼容旧 API（测试 / 旧调用方）----

#[repr(C)]
pub struct KeyboardReport {
    pub modifier: u8,
    pub reserved: u8,
    pub keys: [u8; 6],
}

impl KeyboardReport {
    pub fn new() -> Self {
        Self {
            modifier: 0,
            reserved: 0,
            keys: [0; 6],
        }
    }
    pub fn to_bytes(&self) -> [u8; 8] {
        let mut buf = [0u8; 8];
        buf[0] = self.modifier;
        buf[2..8].copy_from_slice(&self.keys);
        buf
    }
}

pub struct ConsumerReport {
    pub usage: u16,
}

impl ConsumerReport {
    pub fn volume_up() -> Self {
        Self { usage: 0xE9 }
    }
    pub fn to_bytes(&self) -> [u8; 2] {
        self.usage.to_le_bytes()
    }
}

pub struct WinUHidInjector;

impl WinUHidInjector {
    pub fn new() -> Self {
        Self
    }
    pub fn is_open(&self) -> bool {
        is_available()
    }
    pub fn init(&mut self) -> Result<(), String> {
        if ensure_init() {
            Ok(())
        } else {
            Err("WinUHid unavailable".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_report_escape() {
        let r = build_keyboard_report(&[0x1B]).unwrap();
        assert_eq!(r[2], 0x29);
    }

    #[test]
    fn consumer_volume() {
        assert_eq!(consumer_usage(0xAF), Some(0xE9));
        assert_eq!(consumer_usage(0xB7), Some(0x0192));
    }

    #[test]
    fn keyboard_report_win_alt_and_ctrl_win() {
        let win_alt = build_keyboard_report(&[0x5B, 0xA4]).unwrap();
        assert_eq!(win_alt[0], 0x0C); // Left GUI | Left Alt

        let ctrl_win = build_keyboard_report(&[0xA2, 0x5B]).unwrap();
        assert_eq!(ctrl_win[0], 0x09); // Left Ctrl | Left GUI
    }

    #[test]
    fn modifier_stagger_order_win_before_alt() {
        let (mods, _) = split_modifiers_and_keys(&[0xA4, 0x5B]);
        assert_eq!(mods, vec![0x5B, 0xA4]);

        let (mods, _) = split_modifiers_and_keys(&[0x5B, 0xA2]);
        assert_eq!(mods, vec![0xA2, 0x5B]);
    }
}
