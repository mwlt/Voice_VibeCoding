pub mod xiaomi;
pub mod t1_common;
pub mod t1_ble;
pub mod t1_usb;
pub mod hanvon;
pub mod shared;

/// 过渡：旧 `bridges::t1::foo` 仍可用
pub mod t1 {
    pub use crate::bridges::t1_ble::{
        ble_adpcm, ble_connect, ble_host, ble_keys, ble_pcm, ble_runtime, ble_session, ble_voice,
        ble_voice_meter, t1_hid_filter_env,
    };
    pub use crate::bridges::t1_common::{
        config, consumer_raw_input, inject, key_diag, mapping, native_suppress,
    };
    pub use crate::bridges::t1_usb::{bridge, native_mic, runtime};
}

use parking_lot::RwLock;

/// Represents the type of bridge device
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeType {
    Xiaomi,
    T1Ble,
    T1Usb,
    Hanvon,
}

impl BridgeType {
    pub fn as_config_key(self) -> &'static str {
        match self {
            BridgeType::Xiaomi => "xiaomi",
            BridgeType::T1Ble => "t1_ble",
            BridgeType::T1Usb => "t1_usb",
            BridgeType::Hanvon => "hanvon",
        }
    }
}

impl std::fmt::Display for BridgeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BridgeType::Xiaomi => write!(f, "小米遥控器"),
            BridgeType::T1Ble => write!(f, "T1(蓝牙)"),
            BridgeType::T1Usb => write!(f, "T1(USB)"),
            BridgeType::Hanvon => write!(f, "汉王 V60 语音笔"),
        }
    }
}

/// Status of a bridge connection
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeStatus {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}

impl serde::Serialize for BridgeStatus {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            BridgeStatus::Disconnected => serializer.serialize_str("Disconnected"),
            BridgeStatus::Connecting => serializer.serialize_str("Connecting"),
            BridgeStatus::Connected => serializer.serialize_str("Connected"),
            BridgeStatus::Error(msg) => serializer.serialize_str(&format!("Error|{}", msg)),
        }
    }
}

impl<'de> serde::Deserialize<'de> for BridgeStatus {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Ok(match s.as_str() {
            "Disconnected" => BridgeStatus::Disconnected,
            "Connecting" => BridgeStatus::Connecting,
            "Connected" => BridgeStatus::Connected,
            _ if s.starts_with("Error|") => BridgeStatus::Error(s[6..].to_string()),
            _ => {
                log::warn!("Unknown BridgeStatus value: {s}, defaulting to Disconnected");
                BridgeStatus::Disconnected
            }
        })
    }
}

impl std::fmt::Display for BridgeStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BridgeStatus::Disconnected => write!(f, "未连接"),
            BridgeStatus::Connecting => write!(f, "连接中..."),
            BridgeStatus::Connected => write!(f, "已连接"),
            BridgeStatus::Error(e) => write!(f, "错误: {}", e),
        }
    }
}

/// Device information returned to the frontend
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DeviceInfo {
    pub bridge_type: BridgeType,
    pub status: BridgeStatus,
    pub device_name: Option<String>,
    pub device_address: Option<String>,
    pub battery_level: Option<u8>,
}

/// Global bridge state shared across the application
pub struct BridgeState {
    pub xiaomi: RwLock<DeviceInfo>,
    pub t1_ble: RwLock<DeviceInfo>,
    pub t1_usb: RwLock<DeviceInfo>,
    pub hanvon: RwLock<DeviceInfo>,
}

impl BridgeState {
    pub fn new() -> Self {
        Self {
            xiaomi: RwLock::new(DeviceInfo {
                bridge_type: BridgeType::Xiaomi,
                status: BridgeStatus::Disconnected,
                device_name: None,
                device_address: None,
                battery_level: None,
            }),
            t1_ble: RwLock::new(DeviceInfo {
                bridge_type: BridgeType::T1Ble,
                status: BridgeStatus::Disconnected,
                device_name: None,
                device_address: None,
                battery_level: None,
            }),
            t1_usb: RwLock::new(DeviceInfo {
                bridge_type: BridgeType::T1Usb,
                status: BridgeStatus::Disconnected,
                device_name: None,
                device_address: None,
                battery_level: None,
            }),
            hanvon: RwLock::new(DeviceInfo {
                bridge_type: BridgeType::Hanvon,
                status: BridgeStatus::Disconnected,
                device_name: None,
                device_address: None,
                battery_level: None,
            }),
        }
    }

    fn slot(&self, bridge_type: BridgeType) -> &RwLock<DeviceInfo> {
        match bridge_type {
            BridgeType::Xiaomi => &self.xiaomi,
            BridgeType::T1Ble => &self.t1_ble,
            BridgeType::T1Usb => &self.t1_usb,
            BridgeType::Hanvon => &self.hanvon,
        }
    }

    pub fn update_status(&self, bridge_type: BridgeType, status: BridgeStatus) {
        let mut guard = self.slot(bridge_type).write();
        let is_disconnected = status == BridgeStatus::Disconnected;
        guard.status = status;
        if is_disconnected {
            guard.device_name = None;
            guard.device_address = None;
            guard.battery_level = None;
        }
    }

    /// Update full device info (name, address, battery) after successful connection.
    /// Also sets the status to Connected.
    pub fn update_device_info(
        &self,
        bridge_type: BridgeType,
        name: Option<String>,
        address: Option<String>,
        battery: Option<u8>,
    ) {
        let mut guard = self.slot(bridge_type).write();
        guard.status = BridgeStatus::Connected;
        if let Some(n) = name {
            guard.device_name = Some(n);
        }
        if let Some(a) = address {
            guard.device_address = Some(a);
        }
        if let Some(b) = battery {
            guard.battery_level = Some(b);
        }
    }

    /// 只写电量，不改连接状态（T1 USB / BLE 互相独立）。
    pub fn update_battery_level(&self, bridge_type: BridgeType, battery: u8) {
        self.slot(bridge_type).write().battery_level = Some(battery);
    }

    pub fn get_info(&self, bridge_type: BridgeType) -> DeviceInfo {
        self.slot(bridge_type).read().clone()
    }
}
