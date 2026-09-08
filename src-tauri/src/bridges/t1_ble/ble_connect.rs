//! T1 BLE 发现与连接（独立于 USB / 小米桥接）
//!
//! 参考小米 `connect.rs` 的 AQS 枚举方式，但只匹配：
//! - 设备名含 `T1-Remote` / `T1 Remote`
//! - 或 Windows 接口 ID 含 `dev_vid&01620a_pid&0407`（本机实测 T1 BLE）

use std::collections::HashMap;

/// Google ATVV Voice Service（与规范一致）
pub const ATVV_SERVICE_UUID: &str = "ab5e0001-5a21-4f05-bc7d-af01f617b664";
pub const ATVV_TX_UUID: &str = "ab5e0002-5a21-4f05-bc7d-af01f617b664";
pub const ATVV_AUDIO_UUID: &str = "ab5e0003-5a21-4f05-bc7d-af01f617b664";
pub const ATVV_CONTROL_UUID: &str = "ab5e0004-5a21-4f05-bc7d-af01f617b664";

/// 本机 T1-Remote BLE 硬件 token（Windows DeviceInformation.Id）
pub const T1_BLE_HARDWARE_TOKEN: &str = "dev_vid&01620a_pid&0407";

const T1_BLE_NAMES: &[&str] = &["t1-remote", "t1 remote", "t1_remote"];

#[derive(Debug, Clone)]
pub struct T1BleCandidate {
    pub name: String,
    pub address: String,
    pub address_u64: u64,
    pub device_token: String,
    pub hardware_match: bool,
    pub interface_id: String,
}

#[derive(Debug, Clone)]
pub struct T1BleConnection {
    pub name: String,
    pub address: String,
    pub address_u64: u64,
    pub atvv_interface_id: String,
}

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

fn find_mac_in_id(folded_id: &str) -> Option<String> {
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

fn candidate_from_interface(name: &str, interface_id: &str) -> Option<T1BleCandidate> {
    let folded_id = interface_id.to_lowercase();
    let folded_name = name.trim().to_lowercase();
    let hardware_match = folded_id.contains(T1_BLE_HARDWARE_TOKEN);
    let name_match = T1_BLE_NAMES.iter().any(|n| folded_name.contains(n));
    if !hardware_match && !name_match {
        return None;
    }
    let token = find_mac_in_id(&folded_id)?;
    let address = normalize_bluetooth_address(&token).ok()?;
    let address_u64 = address_to_u64(&address).ok()?;
    Some(T1BleCandidate {
        name: {
            let n = name.trim();
            if n.is_empty() {
                "T1-Remote".into()
            } else {
                n.to_string()
            }
        },
        address,
        address_u64,
        device_token: token.to_lowercase(),
        hardware_match,
        interface_id: interface_id.to_string(),
    })
}

pub fn choose_t1_ble_candidate(
    candidates: &[T1BleCandidate],
    configured_address: Option<&str>,
) -> Option<T1BleCandidate> {
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
    candidates
        .iter()
        .find(|c| {
            let n = c.name.to_lowercase();
            n.contains("t1-remote") || n.contains("t1 remote")
        })
        .cloned()
}

/// 发现并打开已配对的 T1 BLE（阻塞）
pub fn discover_and_connect(
    configured_address: Option<&str>,
) -> Result<T1BleConnection, String> {
    #[cfg(target_os = "windows")]
    {
        windows_discover_and_connect(configured_address)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = configured_address;
        Err("T1 蓝牙仅支持 Windows".into())
    }
}

#[cfg(target_os = "windows")]
fn map_winrt_null(context: &str, err: windows::core::Error) -> String {
    let code = err.code().0 as u32;
    if code == 0 {
        format!(
            "{context}：设备对象为空。请确认 T1-Remote 已开机、已在 Windows 蓝牙设置中配对"
        )
    } else {
        format!("{context}: {err}")
    }
}

#[cfg(target_os = "windows")]
fn windows_discover_candidates() -> Result<Vec<T1BleCandidate>, String> {
    use windows::core::GUID;
    use windows::Devices::Bluetooth::GenericAttributeProfile::GattDeviceService;
    use windows::Devices::Enumeration::DeviceInformation;

    let uuid = GUID::from_u128(0xab5e0001_5a21_4f05_bc7d_af01f617b664);
    let selector = GattDeviceService::GetDeviceSelectorFromUuid(uuid)
        .map_err(|e| format!("GetDeviceSelectorFromUuid 失败: {e}"))?;
    let collection = DeviceInformation::FindAllAsyncAqsFilter(&selector)
        .map_err(|e| format!("FindAllAsyncAqsFilter 失败: {e}"))?
        .get()
        .map_err(|e| format!("枚举 ATVV GATT 接口失败: {e}"))?;
    let size = collection.Size().map_err(|e| format!("Size 失败: {e}"))?;
    let mut by_token: HashMap<String, T1BleCandidate> = HashMap::new();
    for i in 0..size {
        let info = collection
            .GetAt(i)
            .map_err(|e| format!("GetAt({i}) 失败: {e}"))?;
        let name = info.Name().map(|n| n.to_string()).unwrap_or_default();
        let id = info.Id().map(|n| n.to_string()).unwrap_or_default();
        if let Some(candidate) = candidate_from_interface(&name, &id) {
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
fn windows_discover_and_connect(
    configured_address: Option<&str>,
) -> Result<T1BleConnection, String> {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_MULTITHREADED,
        );
    }
    let candidates = windows_discover_candidates()?;
    log::info!("T1 BLE discovery found {} candidate(s)", candidates.len());
    for c in &candidates {
        log::info!(
            "  T1 BLE candidate name={} address={} hw={}",
            c.name,
            c.address,
            c.hardware_match
        );
    }
    let candidate = if let Some(addr) = configured_address.filter(|s| !s.trim().is_empty()) {
        choose_t1_ble_candidate(&candidates, Some(addr)).or_else(|| {
            let address = normalize_bluetooth_address(addr).ok()?;
            let address_u64 = address_to_u64(&address).ok()?;
            Some(T1BleCandidate {
                name: "T1-Remote".into(),
                address: address.clone(),
                address_u64,
                device_token: device_token_from_address(&address).unwrap_or_default(),
                hardware_match: false,
                interface_id: String::new(),
            })
        })
    } else {
        choose_t1_ble_candidate(&candidates, None)
    };
    let Some(candidate) = candidate else {
        return Err(
            "未找到已配对的 T1-Remote。请先在 Windows 蓝牙设置中配对「T1-Remote」，并确认带有 ATVV 语音服务"
                .into(),
        );
    };
    windows_open_and_verify(&candidate)
}

#[cfg(target_os = "windows")]
fn windows_open_and_verify(candidate: &T1BleCandidate) -> Result<T1BleConnection, String> {
    log::info!(
        "T1 BLE CONNECTING name={} address={} interface={}",
        candidate.name,
        candidate.address,
        candidate.interface_id
    );
    if !candidate.interface_id.is_empty() {
        match windows_open_via_gatt_interface(candidate) {
            Ok(conn) => return Ok(conn),
            Err(e) => log::warn!("T1 BLE GATT FromId 失败，回退地址打开: {e}"),
        }
    }
    windows_open_via_address(candidate)
}

#[cfg(target_os = "windows")]
fn windows_open_via_gatt_interface(
    candidate: &T1BleCandidate,
) -> Result<T1BleConnection, String> {
    use windows::core::HSTRING;
    use windows::Devices::Bluetooth::BluetoothLEDevice;
    use windows::Devices::Bluetooth::GenericAttributeProfile::{
        GattDeviceService, GattOpenStatus, GattSharingMode,
    };

    let id = HSTRING::from(candidate.interface_id.as_str());
    let service = GattDeviceService::FromIdAsync(&id)
        .map_err(|e| format!("GattDeviceService::FromIdAsync 失败: {e}"))?
        .get()
        .map_err(|e| map_winrt_null("打开 T1 ATVV GATT 服务失败", e))?;
    match service.OpenAsync(GattSharingMode::SharedReadOnly) {
        Ok(op) => match op.get() {
            Ok(status)
                if status == GattOpenStatus::Success || status == GattOpenStatus::AlreadyOpened => {}
            Ok(status) => return Err(format!("打开 ATVV 服务状态异常: {status:?}")),
            Err(e) => log::warn!("T1 BLE OpenAsync: {e}"),
        },
        Err(e) => log::warn!("T1 BLE OpenAsync invoke: {e}"),
    }
    let device_id = service
        .DeviceId()
        .map_err(|e| format!("读取 DeviceId 失败: {e}"))?;
    let device = BluetoothLEDevice::FromIdAsync(&device_id)
        .map_err(|e| format!("BluetoothLEDevice::FromIdAsync 失败: {e}"))?
        .get()
        .map_err(|e| map_winrt_null("从 ATVV 接口打开 BLE 设备失败", e))?;
    let _ = device;
    Ok(T1BleConnection {
        name: candidate.name.clone(),
        address: candidate.address.clone(),
        address_u64: candidate.address_u64,
        atvv_interface_id: candidate.interface_id.clone(),
    })
}

#[cfg(target_os = "windows")]
fn windows_open_via_address(candidate: &T1BleCandidate) -> Result<T1BleConnection, String> {
    use windows::Devices::Bluetooth::BluetoothLEDevice;

    let device = BluetoothLEDevice::FromBluetoothAddressAsync(candidate.address_u64)
        .map_err(|e| format!("FromBluetoothAddressAsync 失败: {e}"))?
        .get()
        .map_err(|e| map_winrt_null("按地址打开 T1 BLE 失败", e))?;
    let name = device
        .Name()
        .map(|n| n.to_string())
        .unwrap_or_else(|_| candidate.name.clone());
    Ok(T1BleConnection {
        name,
        address: candidate.address.clone(),
        address_u64: candidate.address_u64,
        atvv_interface_id: candidate.interface_id.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(name: &str, addr: &str, hw: bool) -> T1BleCandidate {
        T1BleCandidate {
            name: name.into(),
            address: addr.into(),
            address_u64: address_to_u64(addr).unwrap_or(0),
            device_token: device_token_from_address(addr).unwrap_or_default(),
            hardware_match: hw,
            interface_id: format!("#{}#", T1_BLE_HARDWARE_TOKEN),
        }
    }

    #[test]
    fn choose_by_configured_address() {
        let list = vec![
            cand("Other", "11:22:33:44:55:66", false),
            cand("T1-Remote", "12:AC:2C:46:C4:AB", true),
        ];
        let got = choose_t1_ble_candidate(&list, Some("12:AC:2C:46:C4:AB")).unwrap();
        assert_eq!(got.address, "12:AC:2C:46:C4:AB");
    }

    #[test]
    fn choose_single_hardware() {
        let list = vec![cand("T1-Remote", "12:AC:2C:46:C4:AB", true)];
        let got = choose_t1_ble_candidate(&list, None).unwrap();
        assert!(got.hardware_match);
    }

    #[test]
    fn normalize_address_roundtrip() {
        assert_eq!(
            normalize_bluetooth_address("12ac2c46c4ab").unwrap(),
            "12:AC:2C:46:C4:AB"
        );
    }
}
