//! 小米 BLE 桥接 — WinRT BLE API 连接小米遥控器 2 Pro (RC003 / MI RC)
//!
//! GATT 服务 (与 Python atvv_record.py 一致):
//! - ATVV 语音: ab5e0001-5a21-4f05-bc7d-af01f617b664
//! - HID 报告:  00001812-0000-1000-8000-00805f9b34fb
//!
//! 连接入口见 `connect.rs`：枚举已配对 GATT 接口，而非广告扫描。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::mpsc;

/// 小米 BLE 设备信息
#[derive(Debug, Clone)]
pub struct XiaomiBleInfo {
    pub device_id: String,
    pub device_name: String,
    pub bluetooth_address: u64,
    pub rssi: i16,
}

/// 按键事件类型（从 BLE HID 报告解析）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XiaomiButton {
    Power,
    VolumeUp,
    VolumeDown,
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
    Ok,
    Back,
    Home,
    Menu,
    Voice,
    Mute,
    Tv,
    Unknown(u16),
}

impl XiaomiButton {
    /// 对齐 Python `USAGE_NAMES` / `DIRECT_USAGES`（含 Windows 会丢掉的 0xF1 返回键）
    pub fn from_hid_usage(usage_id: u16) -> Self {
        match usage_id {
            0x0028 => Self::Ok,
            0x0035 => Self::Tv, // TV / 输入切换
            0x004A => Self::Home,
            0x004F => Self::DpadRight,
            0x0050 => Self::DpadLeft,
            0x0051 => Self::DpadDown,
            0x0052 => Self::DpadUp,
            0x0065 => Self::Menu,
            0x0066 => Self::Power,
            0x007F => Self::Mute,
            0x0080 => Self::VolumeUp,
            0x0081 => Self::VolumeDown,
            0x00F1 => Self::Back, // KEY_BACK — Windows kbdhid 会丢弃
            _ => Self::Unknown(usage_id),
        }
    }

    /// button_id：与 Python `button_bindings` 键名对齐（up/left/…/tv/mic）
    /// 同时兼容旧版 dpad_* / voice 别名（见 key_mapping::resolve_button_id）
    pub fn to_button_id(&self) -> &str {
        match self {
            Self::Power => "power",
            Self::VolumeUp => "volume_up",
            Self::VolumeDown => "volume_down",
            Self::DpadUp => "up",
            Self::DpadDown => "down",
            Self::DpadLeft => "left",
            Self::DpadRight => "right",
            Self::Ok => "ok",
            Self::Back => "back",
            Self::Home => "home",
            Self::Menu => "menu",
            Self::Voice => "mic",
            Self::Mute => "volume_mute",
            Self::Tv => "tv",
            Self::Unknown(_) => "unknown",
        }
    }
}

/// BLE 事件
#[derive(Debug, Clone)]
pub enum BleEvent {
    /// 扫描发现设备
    DeviceFound(XiaomiBleInfo),
    /// 已连接到设备
    Connected(XiaomiBleInfo),
    /// 连接断开
    Disconnected,
    /// 按键按下
    ButtonPressed(XiaomiButton),
    /// 按键释放
    ButtonReleased(XiaomiButton),
    /// 音频数据（ADPCM 编码）
    AudioData(Vec<u8>),
    /// 错误
    Error(String),
}

/// 扫描过滤器
#[derive(Debug, Clone)]
pub struct ScanFilter {
    /// 蓝牙地址过滤（None = 不过滤）
    pub bluetooth_address: Option<u64>,
    /// 设备名称包含过滤
    pub name_contains: Option<String>,
    /// 最小 RSSI
    pub min_rssi: Option<i16>,
}

impl Default for ScanFilter {
    fn default() -> Self {
        Self {
            bluetooth_address: None,
            name_contains: Some("MI RC".into()),
            min_rssi: Some(-80),
        }
    }
}

/// BLE 桥接状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BleState {
    Idle,
    Scanning,
    Connecting,
    Connected,
    Error,
}

/// BLE 桥接管理器
pub struct XiaomiBleBridge {
    running: Arc<AtomicBool>,
    state: Arc<Mutex<BleState>>,
    event_tx: Option<mpsc::UnboundedSender<BleEvent>>,
    filter: ScanFilter,
}

impl XiaomiBleBridge {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            state: Arc::new(Mutex::new(BleState::Idle)),
            event_tx: None,
            filter: ScanFilter::default(),
        }
    }

    /// 设置扫描过滤条件
    pub fn with_filter(mut self, filter: ScanFilter) -> Self {
        self.filter = filter;
        self
    }

    /// 开始扫描并返回事件接收器
    pub async fn start_scan(&mut self) -> Result<mpsc::UnboundedReceiver<BleEvent>, String> {
        if self.running.load(Ordering::SeqCst) {
            return Err("BLE 桥接已在运行".into());
        }

        let (tx, rx) = mpsc::unbounded_channel();
        self.event_tx = Some(tx.clone());

        self.running.store(true, Ordering::SeqCst);
        *self.state.lock() = BleState::Scanning;

        let running = Arc::clone(&self.running);
        let state = Arc::clone(&self.state);
        let filter = self.filter.clone();

        // 后台 BLE 扫描（std::thread: WinRT 对象不支持 Send，需要 STA COM）
        std::thread::spawn(move || {
            #[cfg(target_os = "windows")]
            unsafe {
                let _ = windows::Win32::System::Com::CoInitializeEx(
                    None,
                    windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
                ).ok();
            }

            let rt = tokio::runtime::Runtime::new()
                .expect("failed to create tokio runtime for BLE scan thread");
            rt.block_on(async {
                log::info!("BLE scanning started with filter: {:?}", filter);

                #[cfg(target_os = "windows")]
                ble_scan_windows(running, state, tx, filter).await;

                #[cfg(not(target_os = "windows"))]
                {
                    log::warn!("BLE only supported on Windows");
                    let _ = (running, state, filter);
                    let _ = tx.send(BleEvent::Error("BLE not supported on this platform".into()));
                }
            });
        });

        Ok(rx)
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        *self.state.lock() = BleState::Idle;
        self.event_tx = None;
        log::info!("BLE bridge stopped");
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn state(&self) -> BleState {
        *self.state.lock()
    }
}

impl Drop for XiaomiBleBridge {
    fn drop(&mut self) { self.stop(); }
}

/// Windows BLE 扫描实现
///
/// 需要 windows-rs 的 WinRT BLE API:
/// ```toml
/// [dependencies.windows]
/// features = [
///   "Devices_Bluetooth_Advertisement",
///   "Devices_Bluetooth",
///   "Devices_Bluetooth_GenericAttributeProfile",
///   "Foundation",
/// ]
/// ```
///
/// 核心 API 调用链路:
/// 1. BluetoothLEAdvertisementWatcher::new() → 创建扫描器
/// 2. watcher.Received(handler) → 注册事件处理器
/// 3. watcher.Start() → 开始扫描
/// 4. args.BluetoothAddress() → 获取设备地址
/// 5. BluetoothLEDevice::FromBluetoothAddressAsync(addr) → 连接设备
/// 6. device.GetGattServicesAsync() → 枚举 GATT 服务
/// 7. service.GetCharacteristicsAsync() → 获取特征
/// 8. characteristic.ValueChanged(handler) → 订阅通知
#[cfg(target_os = "windows")]
async fn ble_scan_windows(
    running: Arc<AtomicBool>,
    state: Arc<Mutex<BleState>>,
    tx: mpsc::UnboundedSender<BleEvent>,
    filter: ScanFilter,
) {
    use windows::Devices::Bluetooth::Advertisement::{
        BluetoothLEAdvertisementWatcher, BluetoothLEScanningMode,
        BluetoothLEAdvertisementFilter,
    };
    use windows::Foundation::TypedEventHandler;

    // Step 1: Create watcher
    let watcher = match BluetoothLEAdvertisementWatcher::new() {
        Ok(w) => w,
        Err(e) => {
            log::error!("Failed to create BLE watcher: {:?}", e);
            *state.lock() = BleState::Error;
            let _ = tx.send(BleEvent::Error(format!("创建BLE扫描器失败: {:?}", e)));
            return;
        }
    };

    // Step 2: Apply scan filter (name/RSSI filtering done in handler)
    if let Ok(ad_filter) = BluetoothLEAdvertisementFilter::new() {
        let _ = watcher.SetAdvertisementFilter(&ad_filter);
    }

    if watcher.SetScanningMode(BluetoothLEScanningMode::Active).is_err() {
        log::warn!("Failed to set Active scanning mode, using default");
    }

    // Step 3: Register event handler — wire tx through Arc
    let tx_arc = Arc::new(tx);
    let tx_handler = Arc::clone(&tx_arc);
    let filter_handler = filter.clone();
    let running_clone = running.clone();

    let handler = TypedEventHandler::new(
        move |_sender: &Option<BluetoothLEAdvertisementWatcher>,
              args: &Option<
                  windows::Devices::Bluetooth::Advertisement::BluetoothLEAdvertisementReceivedEventArgs,
              >| {
            if !running_clone.load(Ordering::SeqCst) {
                return Ok(());
            }
            if let Some(args) = args {
                if let Ok(addr) = args.BluetoothAddress() {
                    let name = args.Advertisement()
                        .and_then(|a| a.LocalName())
                        .map(|n| n.to_string())
                        .unwrap_or_default();
                    let rssi = args.RawSignalStrengthInDBm().unwrap_or(0) as i16;

                    // Apply filter
                    if let Some(ref name_filter) = filter_handler.name_contains {
                        if !name.to_lowercase().contains(&name_filter.to_lowercase()) {
                            return Ok(());
                        }
                    }
                    if let Some(min_rssi) = filter_handler.min_rssi {
                        if rssi < min_rssi { return Ok(()); }
                    }
                    if let Some(target_addr) = filter_handler.bluetooth_address {
                        if addr != target_addr { return Ok(()); }
                    }

                    let _ = tx_handler.send(BleEvent::DeviceFound(XiaomiBleInfo {
                        device_id: format!("{:016X}", addr),
                        device_name: name,
                        bluetooth_address: addr,
                        rssi,
                    }));
                }
            }
            Ok(())
        },
    );

    if watcher.Received(&handler).is_err() {
        log::error!("Failed to register BLE handler");
        *state.lock() = BleState::Error;
        let _ = tx_arc.send(BleEvent::Error("注册BLE事件处理器失败".into()));
        return;
    }

    // Step 4: Start scanning
    if let Err(e) = watcher.Start() {
        log::error!("Failed to start BLE scan: {:?}", e);
        *state.lock() = BleState::Error;
        let _ = tx_arc.send(BleEvent::Error(format!("启动BLE扫描失败: {:?}", e)));
        return;
    }

    log::info!("BLE scan active (WinRT)");
    *state.lock() = BleState::Scanning;

    // Wait for stop signal
    while running.load(Ordering::SeqCst) {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    let _ = watcher.Stop();
    *state.lock() = BleState::Idle;
    log::info!("BLE scan stopped");
}
