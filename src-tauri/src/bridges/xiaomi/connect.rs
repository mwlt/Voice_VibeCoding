//! 小米遥控器 2 Pro 连接 — 对齐 Python `atvv_live_bridge.py` 的发现/连接逻辑
//!
//! Python 不做 BLE 广播扫描，而是：
//! 1. 用 ATVV GATT UUID 的 AQS 选择器枚举 **已配对** 的 Windows 设备接口
//! 2. 按 VID/PID token / 设备名筛选小米 2 Pro
//! 3. `BluetoothLEDevice::FromBluetoothAddressAsync` 打开设备并校验 ATVV 服务

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Android TV Voice-over-BLE 服务 UUID（与 Python `atvv_record.VOICE_SERVICE_UUID` 一致）
pub const VOICE_SERVICE_UUID: &str = "ab5e0001-5a21-4f05-bc7d-af01f617b664";
pub const VOICE_TX_UUID: &str = "ab5e0002-5a21-4f05-bc7d-af01f617b664";
pub const VOICE_AUDIO_UUID: &str = "ab5e0003-5a21-4f05-bc7d-af01f617b664";
pub const VOICE_CONTROL_UUID: &str = "ab5e0004-5a21-4f05-bc7d-af01f617b664";

/// 小米遥控器 2 Pro 硬件 token（Windows 接口 ID 中的 VID/PID）
pub const XIAOMI_2_PRO_HARDWARE_TOKEN: &str = "dev_vid&012717_pid&32b8";

const XIAOMI_2_PRO_NAMES: &[&str] = &["mi rc", "xiaomi bluetooth remote 2 pro"];

/// 运行时停止标志（点击「断开」时置位）
#[derive(Default)]
pub struct XiaomiRuntime {
    pub stop: AtomicBool,
    pub running: AtomicBool,
}

impl XiaomiRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    pub fn clear_stop(&self) {
        self.stop.store(false, Ordering::SeqCst);
    }

    pub fn should_stop(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }
}

#[derive(Debug, Clone)]
pub struct XiaomiCandidate {
    pub name: String,
    pub address: String,
    pub address_u64: u64,
    pub device_token: String,
    pub hardware_match: bool,
    /// DeviceInformation.Id —— GATT 服务接口路径，优先用于 FromIdAsync
    pub interface_id: String,
}

#[derive(Debug, Clone)]
pub struct XiaomiConnection {
    pub name: String,
    pub address: String,
    pub address_u64: u64,
    /// ATVV GATT 服务接口 Id（用于 FromId 订阅语音键）
    pub atvv_interface_id: String,
}

/// 规范化蓝牙地址为 `AA:BB:CC:DD:EE:FF`
pub fn normalize_bluetooth_address(value: &str) -> Result<String, String> {
    let compact: String = value
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect::<String>()
        .to_uppercase();
    if compact.len() != 12 {
        return Err(format!("蓝牙地址格式无效：{value}"));
    }
    Ok(compact
        .as_bytes()
        .chunks(2)
        .map(|c| std::str::from_utf8(c).unwrap_or("00"))
        .collect::<Vec<_>>()
        .join(":"))
}

pub fn device_token_from_address(value: &str) -> Result<String, String> {
    Ok(normalize_bluetooth_address(value)?.replace(':', "").to_lowercase())
}

pub fn address_to_u64(address: &str) -> Result<u64, String> {
    let compact: String = address.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    u64::from_str_radix(&compact, 16).map_err(|e| format!("无效蓝牙地址 {address}: {e}"))
}

pub fn format_address(addr: u64) -> String {
    format!(
        "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
        (addr >> 40) & 0xFF,
        (addr >> 32) & 0xFF,
        (addr >> 24) & 0xFF,
        (addr >> 16) & 0xFF,
        (addr >> 8) & 0xFF,
        addr & 0xFF,
    )
}

fn xiaomi_candidate_from_interface(name: &str, interface_id: &str) -> Option<XiaomiCandidate> {
    let folded_id = interface_id.to_lowercase();
    let folded_name = name.trim().to_lowercase();
    let hardware_match = folded_id.contains(XIAOMI_2_PRO_HARDWARE_TOKEN);
    let name_match = XIAOMI_2_PRO_NAMES.iter().any(|n| folded_name == *n);
    if !hardware_match && !name_match {
        return None;
    }

    // 接口 ID 中提取 12 位十六进制 MAC：..._a1b2c3d4e5f6#... 或 -a1b2c3d4e5f6\
    let re = regex_lite_find_mac(&folded_id)?;
    let token = re.to_lowercase();
    let address = normalize_bluetooth_address(&token).ok()?;
    let address_u64 = address_to_u64(&address).ok()?;
    Some(XiaomiCandidate {
        name: {
            let n = name.trim();
            if n.is_empty() {
                "MI RC".into()
            } else {
                n.to_string()
            }
        },
        address,
        address_u64,
        device_token: token,
        hardware_match,
        interface_id: interface_id.to_string(),
    })
}

/// 对齐 Python: `[_-]([0-9a-f]{12})(?:[#\\]|$)`
fn regex_lite_find_mac(folded_id: &str) -> Option<String> {
    let bytes = folded_id.as_bytes();
    let mut i = 0;
    while i + 13 <= bytes.len() {
        let c0 = bytes[i];
        if c0 == b'_' || c0 == b'-' {
            let slice = &folded_id[i + 1..i + 13];
            if slice.chars().all(|c| c.is_ascii_hexdigit()) {
                let next = bytes.get(i + 13).copied();
                let ok = match next {
                    None => true,
                    Some(b'#' | b'\\') => true,
                    _ => false,
                };
                if ok {
                    return Some(slice.to_string());
                }
            }
        }
        i += 1;
    }
    None
}

pub fn choose_xiaomi_2_pro_candidate(
    candidates: &[XiaomiCandidate],
    configured_address: Option<&str>,
) -> Option<XiaomiCandidate> {
    let configured_token = configured_address
        .and_then(|a| device_token_from_address(a).ok())
        .unwrap_or_default();

    if !configured_token.is_empty() {
        if let Some(c) = candidates
            .iter()
            .find(|c| c.device_token == configured_token)
        {
            return Some(c.clone());
        }
    }
    if candidates.len() == 1 {
        return Some(candidates[0].clone());
    }
    let hardware: Vec<_> = candidates
        .iter()
        .filter(|c| c.hardware_match)
        .cloned()
        .collect();
    if hardware.len() == 1 {
        return Some(hardware[0].clone());
    }
    None
}

/// 发现 + 连接（阻塞，应在专用线程中调用）
pub fn discover_and_connect(
    configured_address: Option<&str>,
) -> Result<XiaomiConnection, String> {
    #[cfg(target_os = "windows")]
    {
        return windows_discover_and_connect(configured_address);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = configured_address;
        Err("小米遥控器连接仅支持 Windows".into())
    }
}

/// 保持连接监控，直到 stop 置位或设备断开
pub fn monitor_connection(
    conn: &XiaomiConnection,
    stop: Arc<XiaomiRuntime>,
    app: Option<tauri::AppHandle>,
) -> Result<(), String> {
    if let Some(app) = app.clone() {
        crate::bridges::xiaomi::key_log::start_key_logger(
            app,
            Arc::clone(&stop),
            conn.address_u64,
            conn.atvv_interface_id.clone(),
        );
    }
    #[cfg(target_os = "windows")]
    {
        return windows_monitor_connection(conn.address_u64, stop);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (conn, stop, app);
        Err("小米遥控器连接仅支持 Windows".into())
    }
}

#[cfg(target_os = "windows")]
fn windows_discover_and_connect(
    configured_address: Option<&str>,
) -> Result<XiaomiConnection, String> {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_MULTITHREADED,
        );
    }

    let candidates = windows_discover_candidates()?;
    log::info!("Xiaomi discovery found {} candidate(s)", candidates.len());
    for c in &candidates {
        log::info!(
            "  candidate name={} address={} hw={}",
            c.name,
            c.address,
            c.hardware_match
        );
    }

    let candidate = if let Some(addr) = configured_address.filter(|s| !s.trim().is_empty()) {
        // 配置了地址：优先精确匹配；没有候选时仍尝试直接连接
        choose_xiaomi_2_pro_candidate(&candidates, Some(addr)).or_else(|| {
            let address = normalize_bluetooth_address(addr).ok()?;
            let address_u64 = address_to_u64(&address).ok()?;
            Some(XiaomiCandidate {
                name: "MI RC".into(),
                address: address.clone(),
                address_u64,
                device_token: device_token_from_address(&address).unwrap_or_default(),
                hardware_match: false,
                interface_id: String::new(),
            })
        })
    } else {
        choose_xiaomi_2_pro_candidate(&candidates, None)
    };

    let Some(candidate) = candidate else {
        if candidates.len() > 1 {
            return Err(format!(
                "发现 {} 个小米候选设备，请仅保留当前 2 Pro 配对后重试，或在设置中填写蓝牙地址",
                candidates.len()
            ));
        }
        return Err("未找到已配对的小米遥控器 2 Pro。请先在 Windows 蓝牙设置中配对「MI RC」".into());
    };

    windows_open_and_verify(&candidate)
}

#[cfg(target_os = "windows")]
fn map_winrt_null(context: &str, err: windows::core::Error) -> String {
    // WinRT 异步成功但返回 null 时，windows-rs 常报 0x00000000「操作成功完成」
    let code = err.code().0 as u32;
    if code == 0 {
        format!(
            "{context}：设备对象为空。请确认遥控器已开机、已在 Windows 配对，并靠近电脑后重试"
        )
    } else {
        format!("{context}: {err}")
    }
}

#[cfg(target_os = "windows")]
fn windows_discover_candidates() -> Result<Vec<XiaomiCandidate>, String> {
    use windows::core::GUID;
    use windows::Devices::Bluetooth::GenericAttributeProfile::GattDeviceService;
    use windows::Devices::Enumeration::DeviceInformation;

    let uuid = GUID::from_u128(0xab5e0001_5a21_4f05_bc7d_af01f617b664);
    let selector = GattDeviceService::GetDeviceSelectorFromUuid(uuid)
        .map_err(|e| format!("GetDeviceSelectorFromUuid 失败: {e}"))?;

    let collection = DeviceInformation::FindAllAsyncAqsFilter(&selector)
        .map_err(|e| format!("FindAllAsyncAqsFilter 失败: {e}"))?
        .get()
        .map_err(|e| format!("枚举 GATT 接口失败: {e}"))?;

    let size = collection.Size().map_err(|e| format!("Size 失败: {e}"))?;
    let mut by_token: HashMap<String, XiaomiCandidate> = HashMap::new();

    for i in 0..size {
        let info = collection
            .GetAt(i)
            .map_err(|e| format!("GetAt({i}) 失败: {e}"))?;
        let name = info
            .Name()
            .map(|n| n.to_string())
            .unwrap_or_default();
        let id = info.Id().map(|n| n.to_string()).unwrap_or_default();
        if let Some(candidate) = xiaomi_candidate_from_interface(&name, &id) {
            let replace = match by_token.get(&candidate.device_token) {
                None => true,
                Some(existing) => candidate.hardware_match && !existing.hardware_match,
            };
            if replace {
                by_token.insert(candidate.device_token.clone(), candidate);
            }
        }
    }

    let mut list: Vec<_> = by_token.into_values().collect();
    list.sort_by_key(|c| (!c.hardware_match, c.device_token.clone()));
    Ok(list)
}

#[cfg(target_os = "windows")]
fn windows_open_and_verify(candidate: &XiaomiCandidate) -> Result<XiaomiConnection, String> {
    log::info!(
        "CONNECTING Xiaomi remote={} address={} interface={}",
        candidate.name,
        candidate.address,
        candidate.interface_id
    );

    // 1) 优先用枚举到的 GATT 接口 ID 打开（避免 FromBluetoothAddressAsync 返回空对象）
    if !candidate.interface_id.is_empty() {
        match windows_open_via_gatt_interface(candidate) {
            Ok(conn) => return Ok(conn),
            Err(e) => {
                log::warn!("GATT FromId 路径失败，回退地址打开: {e}");
            }
        }
    }

    // 2) 回退：按蓝牙地址打开
    windows_open_via_address(candidate)
}

#[cfg(target_os = "windows")]
fn windows_open_via_gatt_interface(
    candidate: &XiaomiCandidate,
) -> Result<XiaomiConnection, String> {
    use windows::core::HSTRING;
    use windows::Devices::Bluetooth::BluetoothLEDevice;
    use windows::Devices::Bluetooth::GenericAttributeProfile::{
        GattDeviceService, GattOpenStatus, GattSharingMode,
    };

    let id = HSTRING::from(candidate.interface_id.as_str());
    let service = GattDeviceService::FromIdAsync(&id)
        .map_err(|e| format!("GattDeviceService::FromIdAsync 失败: {e}"))?
        .get()
        .map_err(|e| map_winrt_null("打开 ATVV GATT 服务失败", e))?;

    // 新版 WinRT 需要显式 Open
    match service.OpenAsync(GattSharingMode::SharedReadOnly) {
        Ok(op) => match op.get() {
            Ok(status) if status == GattOpenStatus::Success || status == GattOpenStatus::AlreadyOpened => {}
            Ok(status) => {
                return Err(format!("打开 ATVV 服务状态异常: {status:?}"));
            }
            Err(e) => {
                // 部分系统 OpenAsync 不可用/失败时仍可继续读 DeviceId
                log::warn!("GattDeviceService::OpenAsync: {e}");
            }
        },
        Err(e) => log::warn!("GattDeviceService::OpenAsync 不可用: {e}"),
    }

    let device_id = service
        .DeviceId()
        .map_err(|e| format!("读取 GATT DeviceId 失败: {e}"))?;

    let device = BluetoothLEDevice::FromIdAsync(&device_id)
        .map_err(|e| format!("BluetoothLEDevice::FromIdAsync 失败: {e}"))?
        .get()
        .map_err(|e| map_winrt_null("通过 DeviceId 打开 BLE 设备失败", e))?;

    let name = device
        .Name()
        .map(|n| n.to_string())
        .unwrap_or_else(|_| candidate.name.clone());
    let addr = device
        .BluetoothAddress()
        .unwrap_or(candidate.address_u64);

    log::info!("CONNECTED via GATT interface remote={name} address={}", format_address(addr));

    // 已通过 ATVV 服务接口打开，视为 ATVV 可用
    Ok(XiaomiConnection {
        name: if name.is_empty() {
            candidate.name.clone()
        } else {
            name
        },
        address: format_address(addr),
        address_u64: addr,
        atvv_interface_id: candidate.interface_id.clone(),
    })
}

#[cfg(target_os = "windows")]
fn windows_open_via_address(candidate: &XiaomiCandidate) -> Result<XiaomiConnection, String> {
    use windows::Devices::Bluetooth::{BluetoothCacheMode, BluetoothLEDevice};
    use windows::Devices::Bluetooth::GenericAttributeProfile::GattCommunicationStatus;

    let device = BluetoothLEDevice::FromBluetoothAddressAsync(candidate.address_u64)
        .map_err(|e| format!("FromBluetoothAddressAsync 失败: {e}"))?
        .get()
        .map_err(|e| map_winrt_null("打开 BLE 设备失败", e))?;

    let name = device
        .Name()
        .map(|n| n.to_string())
        .unwrap_or_else(|_| candidate.name.clone());
    if name.is_empty() && candidate.name.is_empty() {
        return Err(format!(
            "未找到已配对 BLE 设备: {}（请确认 Windows 已配对且设备开机）",
            candidate.address
        ));
    }

    log::info!("CONNECTED remote={name}; discovering ATVV");

    let services_result = device
        .GetGattServicesWithCacheModeAsync(BluetoothCacheMode::Uncached)
        .map_err(|e| format!("GetGattServices 失败: {e}"))?
        .get()
        .map_err(|e| format!("GATT 服务发现失败: {e}"))?;

    let status = services_result
        .Status()
        .map_err(|e| format!("读取 GATT status 失败: {e}"))?;
    if status != GattCommunicationStatus::Success {
        return Err(format!("GATT 服务发现状态异常: {status:?}"));
    }

    let services = services_result
        .Services()
        .map_err(|e| format!("读取 Services 失败: {e}"))?;
    let count = services.Size().map_err(|e| format!("Services.Size 失败: {e}"))?;

    let target_guid = windows::core::GUID::from_u128(0xab5e0001_5a21_4f05_bc7d_af01f617b664);
    let mut found_atvv = false;
    for i in 0..count {
        let svc = services.GetAt(i).map_err(|e| format!("Service.GetAt 失败: {e}"))?;
        let uuid = svc.Uuid().map_err(|e| format!("Service.Uuid 失败: {e}"))?;
        if uuid == target_guid {
            found_atvv = true;
            break;
        }
    }

    if !found_atvv {
        return Err(
            "已打开蓝牙设备，但未找到 ATVV 语音服务。请确认是小米遥控器 2 Pro (MI RC)".into(),
        );
    }

    log::info!("ATVV DISCOVERED remote={name}");

    Ok(XiaomiConnection {
        name: if name.is_empty() {
            candidate.name.clone()
        } else {
            name
        },
        address: candidate.address.clone(),
        address_u64: candidate.address_u64,
        atvv_interface_id: candidate.interface_id.clone(),
    })
}

#[cfg(target_os = "windows")]
fn windows_monitor_connection(
    address_u64: u64,
    runtime: Arc<XiaomiRuntime>,
) -> Result<(), String> {
    use std::sync::{Arc as StdArc, Condvar, Mutex};
    use windows::Devices::Bluetooth::{BluetoothConnectionStatus, BluetoothLEDevice};
    use windows::Foundation::TypedEventHandler;

    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_MULTITHREADED,
        );
    }

    let device = match BluetoothLEDevice::FromBluetoothAddressAsync(address_u64)
        .map_err(|e| format!("monitor open 失败: {e}"))?
        .get()
    {
        Ok(d) => d,
        Err(e) => {
            return Err(map_winrt_null("监控时打开 BLE 设备失败", e));
        }
    };

    // 立即检查一次，避免已断开还挂事件
    match device.ConnectionStatus() {
        Ok(status) if status == BluetoothConnectionStatus::Disconnected => {
            runtime.running.store(false, Ordering::SeqCst);
            return Err("遥控器已断开连接".into());
        }
        Ok(_) => {}
        Err(e) => {
            runtime.running.store(false, Ordering::SeqCst);
            return Err(format!("读取连接状态失败: {e}"));
        }
    }

    #[derive(Default)]
    struct Flag {
        disconnected: bool,
    }
    let pair = StdArc::new((Mutex::new(Flag::default()), Condvar::new()));
    let pair_cb = StdArc::clone(&pair);
    let token = device
        .ConnectionStatusChanged(&TypedEventHandler::new(
            move |sender: &Option<BluetoothLEDevice>, _args| {
                if let Some(dev) = sender {
                    if let Ok(status) = dev.ConnectionStatus() {
                        if status == BluetoothConnectionStatus::Disconnected {
                            if let Ok(mut g) = pair_cb.0.lock() {
                                g.disconnected = true;
                                pair_cb.1.notify_all();
                            }
                        }
                    }
                }
                Ok(())
            },
        ))
        .map_err(|e| format!("ConnectionStatusChanged: {e}"))?;

    runtime.running.store(true, Ordering::SeqCst);
    let result = loop {
        if runtime.should_stop() {
            break Ok(());
        }
        let (lock, cvar) = &*pair;
        let Ok(guard) = lock.lock() else {
            break Err("连接监控锁失败".into());
        };
        if guard.disconnected {
            log::warn!("Xiaomi remote disconnected (event)");
            break Err("遥控器已断开连接".into());
        }
        let (guard, _) = cvar
            .wait_timeout(guard, Duration::from_millis(200))
            .unwrap_or_else(|e| e.into_inner());
        if guard.disconnected {
            log::warn!("Xiaomi remote disconnected (event)");
            break Err("遥控器已断开连接".into());
        }
        drop(guard);
    };

    let _ = device.RemoveConnectionStatusChanged(token);
    runtime.running.store(false, Ordering::SeqCst);
    result
}
