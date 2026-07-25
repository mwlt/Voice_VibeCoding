pub mod xiaomi;
pub mod t1;
pub mod hanvon;
pub mod shared;

use parking_lot::RwLock;

/// Represents the type of bridge device
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BridgeType {
    Xiaomi,
    T1,
    Hanvon,
}

impl std::fmt::Display for BridgeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BridgeType::Xiaomi => write!(f, "小米遥控器"),
            BridgeType::T1 => write!(f, "T1 遥控器"),
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
    pub t1: RwLock<DeviceInfo>,
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
            t1: RwLock::new(DeviceInfo {
                bridge_type: BridgeType::T1,
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

    pub fn update_status(&self, bridge_type: BridgeType, status: BridgeStatus) {
        let info = match bridge_type {
            BridgeType::Xiaomi => &self.xiaomi,
            BridgeType::T1 => &self.t1,
            BridgeType::Hanvon => &self.hanvon,
        };
        let mut guard = info.write();
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
        let info = match bridge_type {
            BridgeType::Xiaomi => &self.xiaomi,
            BridgeType::T1 => &self.t1,
            BridgeType::Hanvon => &self.hanvon,
        };
        let mut guard = info.write();
        guard.status = BridgeStatus::Connected;
        if let Some(n) = name { guard.device_name = Some(n); }
        if let Some(a) = address { guard.device_address = Some(a); }
        if let Some(b) = battery { guard.battery_level = Some(b); }
    }

    pub fn get_info(&self, bridge_type: BridgeType) -> DeviceInfo {
        match bridge_type {
            BridgeType::Xiaomi => self.xiaomi.read().clone(),
            BridgeType::T1 => self.t1.read().clone(),
            BridgeType::Hanvon => self.hanvon.read().clone(),
        }
    }
}
