//! 小米输入会话 — 对齐 Python `XiaomiGattHidSession` + ATVV Control
//!
//! - 返回键：HID usage `0xF1`（Windows kbdhid 丢弃）→ GATT HID Report
//! - 音量±：HID usage `0x80`/`0x81`（GATT）或由上层 VK 并行兜底
//! - 语音键：ATVV Control opcode `0x08`/`0x04`/`0x00`

use crate::bridges::xiaomi::ble_bridge::XiaomiButton;
use crate::bridges::xiaomi::connect::XiaomiRuntime;
use crate::bridges::xiaomi::key_log::{
    button_label, emit_key_and_map, emit_key_phase, emit_message, KeyEmitGate,
};
use crate::bridges::xiaomi::key_mapping;
use crate::config::manager::{ConfigManager, TriggerMode};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

const HID_SERVICE: u128 = 0x00001812_0000_1000_8000_00805f9b34fb;
const HID_REPORT: u128 = 0x00002a4d_0000_1000_8000_00805f9b34fb;
const HID_REPORT_REFERENCE: u128 = 0x00002908_0000_1000_8000_00805f9b34fb;
const HID_CONTROL_POINT: u128 = 0x00002a4c_0000_1000_8000_00805f9b34fb;
const HID_PROTOCOL_MODE: u128 = 0x00002a4e_0000_1000_8000_00805f9b34fb;

const ATVV_SERVICE: u128 = 0xab5e0001_5a21_4f05_bc7d_af01f617b664;
const ATVV_TX: u128 = 0xab5e0002_5a21_4f05_bc7d_af01f617b664;
const ATVV_AUDIO: u128 = 0xab5e0003_5a21_4f05_bc7d_af01f617b664;
const ATVV_CONTROL: u128 = 0xab5e0004_5a21_4f05_bc7d_af01f617b664;

/// 标准 BLE Battery Service / Battery Level
const BATTERY_SERVICE: u128 = 0x0000180f_0000_1000_8000_00805f9b34fb;
const BATTERY_LEVEL: u128 = 0x00002a19_0000_1000_8000_00805f9b34fb;

const GET_CAPS_V10: [u8; 6] = [0x0A, 0x01, 0x00, 0x00, 0x03, 0x03];

/// 解析 RC003 HID 报告（对齐 Python `handle_direct_hid_report` / `decode_rc003_ioctl_output`）
pub fn parse_hid_usages(payload: &[u8]) -> HashSet<u16> {
    let mut usages = HashSet::new();
    let data: &[u8] = if payload.len() == 9 && payload.starts_with(&[0x01, 0x00, 0x00]) {
        // HidOverGatt IOCTL：3 字节前缀 + 6 字节 usages
        &payload[3..]
    } else if payload.len() == 7 && payload[0] == 1 {
        // 带 report id=1 前缀
        &payload[1..]
    } else if payload.len() >= 6 && payload.len() % 2 == 0 {
        payload
    } else if payload.len() > 6 && (payload.len() - 1) % 2 == 0 && payload[0] <= 0x0F {
        // 其它小 report id 前缀
        &payload[1..]
    } else {
        payload
    };

    if data.is_empty() || data.len() % 2 != 0 {
        return usages;
    }
    for chunk in data.chunks_exact(2) {
        let usage = u16::from_le_bytes([chunk[0], chunk[1]]);
        if usage != 0 {
            usages.insert(usage);
        }
    }
    usages
}

/// 启动 GATT HID + ATVV（阻塞直到 stop）。任一通道成功即可。
pub fn run_input_session(
    app: AppHandle,
    address_u64: u64,
    atvv_interface_id: String,
    runtime: Arc<XiaomiRuntime>,
    gate: Arc<KeyEmitGate>,
) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        windows_run_input_session(app, address_u64, atvv_interface_id, runtime, gate)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app, address_u64, atvv_interface_id, runtime, gate);
        Err("仅支持 Windows".into())
    }
}

#[cfg(target_os = "windows")]
fn windows_run_input_session(
    app: AppHandle,
    address_u64: u64,
    atvv_interface_id: String,
    runtime: Arc<XiaomiRuntime>,
    gate: Arc<KeyEmitGate>,
) -> Result<(), String> {
    use windows::core::GUID;
    use windows::Devices::Bluetooth::GenericAttributeProfile::{
        GattCharacteristic, GattCommunicationStatus, GattDeviceService,
    };
    use windows::Devices::Bluetooth::{BluetoothCacheMode, BluetoothLEDevice};
    use crate::bridges::xiaomi::tv_gate;
    use crate::bridges::xiaomi::voice_pcm;
    use crate::config::manager::ConfigManager;
    use tauri::Manager;

    tv_gate::mark_connecting();

    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_MULTITHREADED,
        );
    }

    let cfg = app
        .try_state::<ConfigManager>()
        .and_then(|m| m.get_device_config("xiaomi").ok());
    let gain_db = cfg.as_ref().map(|c| c.gain_db).unwrap_or(10.0);
    let tv_delay = cfg
        .as_ref()
        .map(|c| c.tv_action_ready_delay)
        .unwrap_or(2.0);

    let device = BluetoothLEDevice::FromBluetoothAddressAsync(address_u64)
        .map_err(|e| format!("input session open: {e}"))?
        .get()
        .map_err(|e| format!("input session get: {e}"))?;

    let services = device
        .GetGattServicesWithCacheModeAsync(BluetoothCacheMode::Uncached)
        .map_err(|e| e.to_string())?
        .get()
        .map_err(|e| e.to_string())?;
    if services.Status().ok() != Some(GattCommunicationStatus::Success) {
        return Err("GATT 服务发现失败".into());
    }
    let services = services.Services().map_err(|e| e.to_string())?;
    let count = services.Size().map_err(|e| e.to_string())?;

    let hid_guid = GUID::from_u128(HID_SERVICE);
    let atvv_guid = GUID::from_u128(ATVV_SERVICE);
    let battery_guid = GUID::from_u128(BATTERY_SERVICE);
    let report_guid = GUID::from_u128(HID_REPORT);
    let report_ref_guid = GUID::from_u128(HID_REPORT_REFERENCE);
    let protocol_guid = GUID::from_u128(HID_PROTOCOL_MODE);
    let control_point_guid = GUID::from_u128(HID_CONTROL_POINT);

    let mut hid_service: Option<GattDeviceService> = None;
    let mut atvv_service: Option<GattDeviceService> = None;
    let mut battery_service: Option<GattDeviceService> = None;
    for i in 0..count {
        let svc = services.GetAt(i).map_err(|e| e.to_string())?;
        let uuid = svc.Uuid().map_err(|e| e.to_string())?;
        if uuid == hid_guid {
            hid_service = Some(svc);
        } else if uuid == atvv_guid {
            atvv_service = Some(svc);
        } else if uuid == battery_guid {
            battery_service = Some(svc);
        }
    }

    let active_usages: Arc<Mutex<HashSet<u16>>> = Arc::new(Mutex::new(HashSet::new()));
    let mut tokens: Vec<(
        GattCharacteristic,
        windows::Foundation::EventRegistrationToken,
    )> = Vec::new();
    let mut hid_ok = false;
    let mut atvv_ok = false;

    // 默认跳过 GATT HID：Windows Microsoft HID 独占时 Open/CCCD 会抢占设备，
    // 导致原生音量失效且又收不到报告。生产路径用 HID Tap（对齐 Python 注释）。
    // 仅当显式设置 REMOTE_BRIDGE_XIAOMI_FORCE_GATT_HID=1 时尝试（Windows HID 关闭时的 fallback）。
    let force_gatt_hid = std::env::var("REMOTE_BRIDGE_XIAOMI_FORCE_GATT_HID")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    if force_gatt_hid {
        if let Some(hid) = hid_service.as_ref() {
            try_subscribe_gatt_hid(
                &app,
                hid,
                &gate,
                &active_usages,
                &mut tokens,
                &mut hid_ok,
                protocol_guid,
                control_point_guid,
                report_guid,
                report_ref_guid,
            );
        } else {
            log::warn!("FORCE_GATT_HID set but HID service not found");
        }
    } else {
        log::info!(
            "Skip GATT HID open (use HID Tap for back/volume); set \
             REMOTE_BRIDGE_XIAOMI_FORCE_GATT_HID=1 only if Windows HID is disabled"
        );
        emit_message(
            &app,
            "跳过 GATT HID（避免抢占 Windows 音量；返回/音量走 HID Tap）",
        );
    }

    // ---- ATVV Control：语音键（优先用发现阶段的服务接口 FromId）----
    if !atvv_interface_id.is_empty() {
        match subscribe_atvv_from_interface(&app, &atvv_interface_id, &gate, &mut tokens, gain_db) {
            Ok(true) => {
                atvv_ok = true;
                emit_message(&app, "ATVV 语音键/音频已订阅（FromId）");
            }
            Ok(false) => log::warn!("ATVV FromId subscribe returned empty"),
            Err(e) => {
                log::warn!("ATVV FromId path failed: {e}");
                emit_message(&app, &format!("ATVV FromId 失败，回退地址打开: {e}"));
            }
        }
    }

    if !atvv_ok {
        if let Some(atvv) = atvv_service.as_ref() {
            match subscribe_atvv_service(&app, atvv, &gate, &mut tokens, gain_db) {
                Ok(true) => atvv_ok = true,
                Ok(false) => {}
                Err(e) => log::warn!("ATVV address-path failed: {e}"),
            }
        } else {
            log::warn!("ATVV service not found");
        }
    }

    // ---- Battery Level（0x180F / 0x2A19）----
    // 与 ATVV 解耦：语音通道失败时仍应能显示电量
    let mut battery_ch: Option<GattCharacteristic> = None;
    let mut last_battery: Option<u8> = None;
    if let Some(batt) = battery_service.as_ref() {
        match setup_battery_monitor(&app, batt, &mut tokens) {
            Ok(ch) => {
                if let Some(level) = read_battery_level(&ch) {
                    publish_battery(&app, level, &mut last_battery, true);
                }
                battery_ch = Some(ch);
            }
            Err(e) => {
                log::warn!("XIAOMI BATTERY setup failed: {e}");
                emit_message(&app, &format!("电量读取失败: {e}"));
            }
        }
    } else {
        log::info!("XIAOMI BATTERY service 0x180F not found on device");
    }

    if !atvv_ok {
        if battery_ch.is_none() {
            tv_gate::reset();
            return Err(
                "无法订阅 ATVV 通知（语音键依赖 ATVV；返回/音量依赖 HID Tap）".into(),
            );
        }
        log::warn!("ATVV subscribe failed; continuing for battery monitor");
        emit_message(
            &app,
            "ATVV 语音通道不可用；电量仍会刷新（请重连或稍后再试语音）",
        );
    }

    let mode = match (hid_ok, atvv_ok) {
        (true, true) => "GATT HID+ATVV",
        (true, false) => "GATT HID",
        (false, true) => "ATVV（语音+音频）",
        _ if battery_ch.is_some() => "Battery",
        _ => "GATT",
    };
    emit_message(&app, &format!("输入会话已启动 ({mode})"));
    log::info!(
        "Input session running mode={mode} atvv={atvv_ok} battery={} subscriptions={}",
        battery_ch.is_some(),
        tokens.len()
    );
    if atvv_ok {
        tv_gate::mark_ready(Duration::from_secs_f32(tv_delay.max(0.0)));
        // 同步预热一次；失败则后台继续重试
        if let Err(e) = voice_pcm::ensure_started() {
            log::warn!("VB-CABLE PCM not ready yet: {e}");
            emit_message(
                &app,
                &format!("语音音频：VB-CABLE 未就绪（{e}）；快捷键仍可用"),
            );
            voice_pcm::warmup_async();
        }
    }

    let mut since_batt = Instant::now();
    let mut since_pcm_warm = Instant::now();
    while !runtime.should_stop() {
        std::thread::sleep(Duration::from_millis(200));
        // 会话中保持 PCM 通路预热（路由重启后自动恢复）
        if atvv_ok
            && !voice_pcm::is_ready()
            && since_pcm_warm.elapsed() >= Duration::from_secs(2)
        {
            since_pcm_warm = Instant::now();
            voice_pcm::warmup_async();
        }
        if let Some(ch) = battery_ch.as_ref() {
            // 首次已读；之后每 45s 轮询，并在启动后 3s 再读一次（提高 UI 首次可见性）
            let due = since_batt.elapsed() >= Duration::from_secs(45)
                || (last_battery.is_none() && since_batt.elapsed() >= Duration::from_secs(3));
            if due {
                since_batt = Instant::now();
                if let Some(level) = read_battery_level(ch) {
                    publish_battery(&app, level, &mut last_battery, false);
                }
            }
        }
    }

    voice_pcm::stop();
    tv_gate::reset();
    for (ch, token) in tokens {
        let _ = ch.RemoveValueChanged(token);
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn publish_battery(app: &AppHandle, level: u8, last: &mut Option<u8>, force_log: bool) {
    use crate::bridges::{BridgeState, BridgeType};
    use tauri::Manager;

    let changed = last.map(|v| v != level).unwrap_or(true);
    *last = Some(level);
    if let Some(state) = app.try_state::<BridgeState>() {
        state.update_device_info(BridgeType::Xiaomi, None, None, Some(level));
    }
    if force_log || changed {
        emit_message(app, &format!("电量 {level}%"));
        log::info!("XIAOMI BATTERY level={level}%");
    }
}

#[cfg(target_os = "windows")]
fn setup_battery_monitor(
    app: &AppHandle,
    service: &windows::Devices::Bluetooth::GenericAttributeProfile::GattDeviceService,
    tokens: &mut Vec<(
        windows::Devices::Bluetooth::GenericAttributeProfile::GattCharacteristic,
        windows::Foundation::EventRegistrationToken,
    )>,
) -> Result<
    windows::Devices::Bluetooth::GenericAttributeProfile::GattCharacteristic,
    String,
> {
    use windows::core::GUID;
    use windows::Devices::Bluetooth::BluetoothCacheMode;
    use windows::Devices::Bluetooth::GenericAttributeProfile::{
        GattCharacteristic, GattClientCharacteristicConfigurationDescriptorValue,
        GattCommunicationStatus, GattOpenStatus, GattSharingMode,
    };
    use windows::Foundation::TypedEventHandler;
    use windows::Storage::Streams::DataReader;
    use tauri::Manager;

    match service.OpenAsync(GattSharingMode::SharedReadOnly) {
        Ok(op) => match op.get() {
            Ok(status)
                if status == GattOpenStatus::Success
                    || status == GattOpenStatus::AlreadyOpened => {}
            Ok(status) => log::warn!("XIAOMI BATTERY OpenAsync status={status:?}"),
            Err(e) => log::warn!("XIAOMI BATTERY OpenAsync: {e}"),
        },
        Err(e) => log::warn!("XIAOMI BATTERY OpenAsync unavailable: {e}"),
    }

    let level_guid = GUID::from_u128(BATTERY_LEVEL);
    let result = service
        .GetCharacteristicsForUuidWithCacheModeAsync(level_guid, BluetoothCacheMode::Uncached)
        .map_err(|e| format!("Battery GetCharacteristics: {e}"))?
        .get()
        .map_err(|e| format!("Battery GetCharacteristics get: {e}"))?;
    if result.Status().ok() != Some(GattCommunicationStatus::Success) {
        return Err(format!("Battery characteristics status={:?}", result.Status()));
    }
    let chars = result
        .Characteristics()
        .map_err(|e| format!("Battery Characteristics: {e}"))?;
    if chars.Size().unwrap_or(0) == 0 {
        return Err("Battery Level characteristic missing".into());
    }
    let ch = chars
        .GetAt(0)
        .map_err(|e| format!("Battery GetAt: {e}"))?;

    // 通知：电量变化时刷新 UI（可选，失败仍可轮询读）
    let app2 = app.clone();
    let handler = TypedEventHandler::new(
        move |_sender: &Option<GattCharacteristic>,
              args: &Option<
            windows::Devices::Bluetooth::GenericAttributeProfile::GattValueChangedEventArgs,
        >| {
            if let Some(args) = args {
                if let Ok(buf) = args.CharacteristicValue() {
                    if let Ok(reader) = DataReader::FromBuffer(&buf) {
                        let len = reader.UnconsumedBufferLength().unwrap_or(0);
                        if len > 0 {
                            let mut data = [0u8; 1];
                            if reader.ReadBytes(&mut data).is_ok() {
                                let level = data[0].min(100);
                                if let Some(state) = app2.try_state::<crate::bridges::BridgeState>()
                                {
                                    state.update_device_info(
                                        crate::bridges::BridgeType::Xiaomi,
                                        None,
                                        None,
                                        Some(level),
                                    );
                                }
                                emit_message(&app2, &format!("电量 {level}%"));
                                log::info!("XIAOMI BATTERY notify level={level}%");
                            }
                        }
                    }
                }
            }
            Ok(())
        },
    );
    if let Ok(token) = ch.ValueChanged(&handler) {
        let cccd_ok = ch
            .WriteClientCharacteristicConfigurationDescriptorAsync(
                GattClientCharacteristicConfigurationDescriptorValue::Notify,
            )
            .and_then(|op| op.get())
            .map(|s| s == GattCommunicationStatus::Success)
            .unwrap_or(false);
        if cccd_ok {
            tokens.push((ch.clone(), token));
            log::info!("XIAOMI BATTERY notify subscribed");
        } else {
            let _ = ch.RemoveValueChanged(token);
            log::info!("XIAOMI BATTERY notify unsupported; will poll");
        }
    }

    Ok(ch)
}

#[cfg(target_os = "windows")]
fn read_battery_level(
    ch: &windows::Devices::Bluetooth::GenericAttributeProfile::GattCharacteristic,
) -> Option<u8> {
    use windows::Devices::Bluetooth::BluetoothCacheMode;
    use windows::Devices::Bluetooth::GenericAttributeProfile::GattCommunicationStatus;
    use windows::Storage::Streams::DataReader;

    let result = ch
        .ReadValueWithCacheModeAsync(BluetoothCacheMode::Uncached)
        .ok()?
        .get()
        .ok()?;
    if result.Status().ok() != Some(GattCommunicationStatus::Success) {
        return None;
    }
    let buf = result.Value().ok()?;
    let reader = DataReader::FromBuffer(&buf).ok()?;
    let len = reader.UnconsumedBufferLength().unwrap_or(0);
    if len == 0 {
        return None;
    }
    let mut data = [0u8; 1];
    reader.ReadBytes(&mut data).ok()?;
    Some(data[0].min(100))
}

/// Windows HID 关闭时的可选 GATT HID fallback（默认不调用）
#[cfg(target_os = "windows")]
fn try_subscribe_gatt_hid(
    app: &AppHandle,
    hid: &windows::Devices::Bluetooth::GenericAttributeProfile::GattDeviceService,
    gate: &Arc<KeyEmitGate>,
    active_usages: &Arc<Mutex<HashSet<u16>>>,
    tokens: &mut Vec<(
        windows::Devices::Bluetooth::GenericAttributeProfile::GattCharacteristic,
        windows::Foundation::EventRegistrationToken,
    )>,
    hid_ok: &mut bool,
    protocol_guid: windows::core::GUID,
    control_point_guid: windows::core::GUID,
    report_guid: windows::core::GUID,
    report_ref_guid: windows::core::GUID,
) {
    use windows::Devices::Bluetooth::GenericAttributeProfile::{
        GattCharacteristic, GattCharacteristicProperties,
        GattClientCharacteristicConfigurationDescriptorValue, GattCommunicationStatus,
        GattSharingMode,
    };
    use windows::Devices::Bluetooth::BluetoothCacheMode;
    use windows::Foundation::TypedEventHandler;
    use windows::Storage::Streams::DataReader;

    // 对齐 Python：只用 SharedReadOnly；绝不 SharedReadAndWrite（会抢占）
    if let Err(e) = hid
        .OpenAsync(GattSharingMode::SharedReadOnly)
        .and_then(|op| op.get())
    {
        log::warn!("HID DIRECT open SharedReadOnly failed: {e}");
        emit_message(app, "GATT HID 无法 SharedReadOnly（Windows HID 可能独占）");
        return;
    }

    match hid.GetCharacteristicsWithCacheModeAsync(BluetoothCacheMode::Uncached) {
        Ok(op) => match op.get() {
            Ok(chars_result)
                if chars_result.Status().ok() == Some(GattCommunicationStatus::Success) =>
            {
                if let Ok(chars) = chars_result.Characteristics() {
                    let n = chars.Size().unwrap_or(0);
                    for i in 0..n {
                        let Ok(ch) = chars.GetAt(i) else { continue };
                        let Ok(uuid) = ch.Uuid() else { continue };
                        let props = ch
                            .CharacteristicProperties()
                            .unwrap_or(GattCharacteristicProperties(0));

                        if uuid == protocol_guid
                            && (props.contains(GattCharacteristicProperties::Write)
                                || props.contains(
                                    GattCharacteristicProperties::WriteWithoutResponse,
                                ))
                        {
                            write_gatt_byte(&ch, 1, "protocol_report_mode");
                            continue;
                        }
                        if uuid == control_point_guid
                            && (props.contains(GattCharacteristicProperties::Write)
                                || props.contains(
                                    GattCharacteristicProperties::WriteWithoutResponse,
                                ))
                        {
                            write_gatt_byte(&ch, 1, "exit_suspend");
                            continue;
                        }

                        if uuid != report_guid {
                            continue;
                        }
                        let can_notify = props.contains(GattCharacteristicProperties::Notify)
                            || props.contains(GattCharacteristicProperties::Indicate);
                        if !can_notify {
                            continue;
                        }

                        let (report_id, report_type) =
                            read_report_reference(&ch, report_ref_guid);
                        if report_type != 0 && report_type != 1 {
                            continue;
                        }

                        let app2 = app.clone();
                        let usages2 = Arc::clone(active_usages);
                        let gate2 = Arc::clone(gate);
                        let handler = TypedEventHandler::new(
                            move |_sender: &Option<GattCharacteristic>,
                                  args: &Option<
                                windows::Devices::Bluetooth::GenericAttributeProfile::GattValueChangedEventArgs,
                            >| {
                                if let Some(args) = args {
                                    if let Ok(buf) = args.CharacteristicValue() {
                                        if let Ok(reader) = DataReader::FromBuffer(&buf) {
                                            let len = reader
                                                .UnconsumedBufferLength()
                                                .unwrap_or(0)
                                                as usize;
                                            let mut data = vec![0u8; len];
                                            let _ = reader.ReadBytes(&mut data);
                                            handle_hid_payload(
                                                &app2, &usages2, &gate2, &data,
                                            );
                                        }
                                    }
                                }
                                Ok(())
                            },
                        );

                        let cccd = if props.contains(GattCharacteristicProperties::Notify) {
                            GattClientCharacteristicConfigurationDescriptorValue::Notify
                        } else {
                            GattClientCharacteristicConfigurationDescriptorValue::Indicate
                        };

                        if let Ok(token) = ch.ValueChanged(&handler) {
                            match ch
                                .WriteClientCharacteristicConfigurationDescriptorAsync(cccd)
                                .and_then(|op| op.get())
                            {
                                Ok(status) if status == GattCommunicationStatus::Success => {
                                    tokens.push((ch.clone(), token));
                                    *hid_ok = true;
                                    log::info!(
                                        "Subscribed HID report id={report_id} type={report_type}"
                                    );
                                }
                                Ok(status) => {
                                    let _ = ch.RemoveValueChanged(token);
                                    log::warn!("HID CCCD write failed status={status:?}");
                                }
                                Err(e) => {
                                    let _ = ch.RemoveValueChanged(token);
                                    log::warn!("HID CCCD write error: {e}");
                                }
                            }
                        }
                    }
                }
                if !*hid_ok {
                    log::warn!("HID DIRECT unavailable no_input_reports");
                    let _ = hid.Close();
                }
            }
            Ok(_) => {
                log::warn!(
                    "HID DIRECT unavailable characteristics_status; windows_hid_active=true"
                );
                let _ = hid.Close();
            }
            Err(e) => log::warn!("HID GetCharacteristics failed: {e}"),
        },
        Err(e) => log::warn!("HID GetCharacteristicsAsync failed: {e}"),
    }
}

#[cfg(target_os = "windows")]
fn subscribe_atvv_from_interface(
    app: &AppHandle,
    interface_id: &str,
    gate: &Arc<KeyEmitGate>,
    tokens: &mut Vec<(
        windows::Devices::Bluetooth::GenericAttributeProfile::GattCharacteristic,
        windows::Foundation::EventRegistrationToken,
    )>,
    gain_db: f32,
) -> Result<bool, String> {
    use windows::core::HSTRING;
    use windows::Devices::Bluetooth::GenericAttributeProfile::{
        GattDeviceService, GattSharingMode,
    };

    let id = HSTRING::from(interface_id);
    let service = GattDeviceService::FromIdAsync(&id)
        .map_err(|e| format!("ATVV FromIdAsync: {e}"))?
        .get()
        .map_err(|e| format!("ATVV FromId get: {e}"))?;

    let _ = service
        .OpenAsync(GattSharingMode::SharedReadOnly)
        .and_then(|op| op.get())
        .or_else(|_| {
            service
                .OpenAsync(GattSharingMode::SharedReadAndWrite)
                .and_then(|op| op.get())
        });

    subscribe_atvv_service(app, &service, gate, tokens, gain_db)
}

/// ATVV 语音会话共享状态
struct AtvvVoiceState {
    decoder: crate::bridges::xiaomi::adpcm_decoder::AdpcmDecoder,
    streaming: bool,
    pending: Vec<u8>,
    frame_size: usize,
    pending_sync: Option<(i32, i32)>,
    last_mic_off: Option<Instant>,
    gain_db: f32,
    frames: u64,
    /// 遥控语音键当前是否按下
    remote_pressed: bool,
    /// 按下时刻（点击模式区分短按/长按）
    press_at: Option<Instant>,
    /// 点击模式：已超过阈值并已对映射键 DOWN
    hold_chord_armed: bool,
    /// 取消过期的「长按判定」定时器
    press_gen: u64,
}

fn voice_trigger_is_toggle(app: &AppHandle) -> bool {
    app.try_state::<ConfigManager>()
        .and_then(|m| m.get_device_config("xiaomi").ok())
        .map(|c| matches!(c.trigger_mode, TriggerMode::Toggle))
        .unwrap_or(true)
}

#[cfg(target_os = "windows")]
fn atvv_write_tx(
    tx: &windows::Devices::Bluetooth::GenericAttributeProfile::GattCharacteristic,
    bytes: &[u8],
    label: &str,
) {
    use windows::Devices::Bluetooth::GenericAttributeProfile::GattWriteOption;
    use windows::Storage::Streams::DataWriter;
    if let Ok(writer) = DataWriter::new() {
        if writer.WriteBytes(bytes).is_ok() {
            if let Ok(buf) = writer.DetachBuffer() {
                let _ = tx.WriteValueWithOptionAsync(&buf, GattWriteOption::WriteWithoutResponse);
                log::info!("ATVV {label} sent");
            }
        }
    }
}

/// 短于此时长抬起 → 视为「点击」；达到此时长仍按住 → 视为「按住」
const CLICK_HOLD_THRESHOLD_MS: u64 = 200;

fn notify_voice_phase(app: &AppHandle, gate: &KeyEmitGate, pressed: bool) {
    if pressed {
        let _ = gate.try_emit("mic");
    }
    emit_key_phase(app, "mic", button_label("mic"), pressed);
}

fn reset_pcm_session(state: &Arc<Mutex<AtvvVoiceState>>, clear_frames: bool) {
    use crate::bridges::xiaomi::voice_pcm;
    if let Ok(mut st) = state.lock() {
        st.streaming = true;
        st.pending.clear();
        st.decoder.reset_with(0, 0);
        st.pending_sync = None;
        st.last_mic_off = None;
        if clear_frames {
            st.frames = 0;
        }
    }
    voice_pcm::clear();
}

/// 遥控语音键按下：传声 + 按模式注入快捷键
fn on_voice_remote_press(app: &AppHandle, gate: &KeyEmitGate, state: &Arc<Mutex<AtvvVoiceState>>) {
    let toggle = voice_trigger_is_toggle(app);
    let gen = {
        let Ok(mut st) = state.lock() else {
            return;
        };
        if st.remote_pressed {
            return;
        }
        st.remote_pressed = true;
        st.press_at = Some(Instant::now());
        st.hold_chord_armed = false;
        st.press_gen = st.press_gen.wrapping_add(1);
        st.press_gen
    };

    reset_pcm_session(state, true);
    notify_voice_phase(app, gate, true);
    if !crate::bridges::xiaomi::voice_pcm::is_ready() {
        crate::bridges::xiaomi::voice_pcm::warmup_async();
    }
    crate::bridges::xiaomi::voice_meter::set_session(true);

    if toggle {
        // 点击模式：短按抬起再 TAP；长按阈值到再 DOWN
        let app2 = app.clone();
        let state2 = Arc::clone(state);
        std::thread::Builder::new()
            .name("xiaomi-voice-click-hold".into())
            .spawn(move || {
                std::thread::sleep(Duration::from_millis(CLICK_HOLD_THRESHOLD_MS));
                let Ok(mut st) = state2.lock() else {
                    return;
                };
                if st.press_gen != gen || !st.remote_pressed || st.hold_chord_armed {
                    return;
                }
                st.hold_chord_armed = true;
                drop(st);
                key_mapping::voice_shortcut_ensure_down(&app2);
                log::info!("XIAOMI ATVV click-mode → HOLD chord (threshold reached)");
            })
            .ok();
        log::info!("XIAOMI ATVV AUDIO_START click-mode (await click vs hold)");
    } else {
        key_mapping::on_remote_button(app, "mic", true);
        log::info!("XIAOMI ATVV AUDIO_START hold-mode → shortcut DOWN");
    }
}

/// 遥控语音键抬起：结束传声 + 短按 TAP / 长按 UP
fn on_voice_remote_release(app: &AppHandle, gate: &KeyEmitGate, state: &Arc<Mutex<AtvvVoiceState>>) {
    use crate::bridges::xiaomi::voice_pcm;
    let toggle = voice_trigger_is_toggle(app);
    let (was_pressed, hold_armed, press_ms) = {
        let Ok(mut st) = state.lock() else {
            return;
        };
        if !st.remote_pressed {
            return;
        }
        let ms = st
            .press_at
            .map(|t| t.elapsed().as_millis() as u64)
            .unwrap_or(0);
        let hold_armed = st.hold_chord_armed;
        st.remote_pressed = false;
        st.press_at = None;
        st.hold_chord_armed = false;
        st.press_gen = st.press_gen.wrapping_add(1); // 作废阈值定时器
        st.streaming = false;
        st.last_mic_off = Some(Instant::now());
        st.pending.clear();
        (true, hold_armed, ms)
    };
    if !was_pressed {
        return;
    }

    std::thread::sleep(Duration::from_millis(40));
    voice_pcm::end_session();

    notify_voice_phase(app, gate, false);

    if toggle {
        if hold_armed || press_ms >= CLICK_HOLD_THRESHOLD_MS {
            key_mapping::on_remote_button(app, "mic", false);
            log::info!("XIAOMI ATVV AUDIO_STOP click-mode HOLD release ms={press_ms}");
        } else {
            key_mapping::voice_shortcut_tap(app);
            log::info!("XIAOMI ATVV AUDIO_STOP click-mode CLICK tap ms={press_ms}");
        }
    } else {
        key_mapping::on_remote_button(app, "mic", false);
        log::info!("XIAOMI ATVV AUDIO_STOP hold-mode → shortcut UP");
    }

    crate::bridges::xiaomi::key_mapping::disarm_voice_native_suppress();
    crate::bridges::xiaomi::voice_meter::set_session(false);
}

#[cfg(target_os = "windows")]
fn subscribe_atvv_service(
    app: &AppHandle,
    atvv: &windows::Devices::Bluetooth::GenericAttributeProfile::GattDeviceService,
    gate: &Arc<KeyEmitGate>,
    tokens: &mut Vec<(
        windows::Devices::Bluetooth::GenericAttributeProfile::GattCharacteristic,
        windows::Foundation::EventRegistrationToken,
    )>,
    gain_db: f32,
) -> Result<bool, String> {
    use windows::core::GUID;
    use windows::Devices::Bluetooth::GenericAttributeProfile::{
        GattCharacteristic, GattClientCharacteristicConfigurationDescriptorValue,
        GattCommunicationStatus, GattSharingMode, GattWriteOption,
    };
    use windows::Devices::Bluetooth::BluetoothCacheMode;
    use windows::Foundation::TypedEventHandler;
    use windows::Storage::Streams::{DataReader, DataWriter};

    let _ = atvv
        .OpenAsync(GattSharingMode::SharedReadAndWrite)
        .and_then(|op| op.get())
        .or_else(|_| {
            atvv.OpenAsync(GattSharingMode::SharedReadOnly)
                .and_then(|op| op.get())
        })
        .or_else(|_| {
            atvv.OpenAsync(GattSharingMode::Exclusive)
                .and_then(|op| op.get())
        });

    let tx_guid = GUID::from_u128(ATVV_TX);
    let audio_guid = GUID::from_u128(ATVV_AUDIO);
    let atvv_control_guid = GUID::from_u128(ATVV_CONTROL);

    let chars_result = atvv
        .GetCharacteristicsWithCacheModeAsync(BluetoothCacheMode::Uncached)
        .map_err(|e| e.to_string())?
        .get()
        .map_err(|e| e.to_string())?;
    if chars_result.Status().ok() != Some(GattCommunicationStatus::Success) {
        return Err("ATVV GetCharacteristics status failed".into());
    }
    let chars = chars_result.Characteristics().map_err(|e| e.to_string())?;
    let n = chars.Size().unwrap_or(0);
    let mut tx: Option<GattCharacteristic> = None;
    let mut audio: Option<GattCharacteristic> = None;
    let mut control: Option<GattCharacteristic> = None;
    for i in 0..n {
        let Ok(ch) = chars.GetAt(i) else { continue };
        let Ok(uuid) = ch.Uuid() else { continue };
        if uuid == tx_guid {
            tx = Some(ch);
        } else if uuid == audio_guid {
            audio = Some(ch);
        } else if uuid == atvv_control_guid {
            control = Some(ch);
        }
    }

    let Some(control) = control else {
        return Ok(false);
    };

    let voice_state = Arc::new(Mutex::new(AtvvVoiceState {
        decoder: crate::bridges::xiaomi::adpcm_decoder::AdpcmDecoder::new_ima(),
        streaming: false,
        pending: Vec::new(),
        frame_size: 120,
        pending_sync: None,
        last_mic_off: None,
        gain_db,
        frames: 0,
        remote_pressed: false,
        press_at: None,
        hold_chord_armed: false,
        press_gen: 0,
    }));

    let app2 = app.clone();
    let gate2 = Arc::clone(gate);
    let tx_for_mic = tx.clone();
    let voice_ctrl = Arc::clone(&voice_state);
    let handler = TypedEventHandler::new(
        move |_sender: &Option<GattCharacteristic>,
              args: &Option<
            windows::Devices::Bluetooth::GenericAttributeProfile::GattValueChangedEventArgs,
        >| {
            if let Some(args) = args {
                if let Ok(buf) = args.CharacteristicValue() {
                    if let Ok(reader) = DataReader::FromBuffer(&buf) {
                        let len = reader.UnconsumedBufferLength().unwrap_or(0) as usize;
                        let mut data = vec![0u8; len];
                        let _ = reader.ReadBytes(&mut data);
                        handle_atvv_control(
                            &app2,
                            &gate2,
                            &voice_ctrl,
                            tx_for_mic.as_ref(),
                            &data,
                        );
                    }
                }
            }
            Ok(())
        },
    );

    let token = control
        .ValueChanged(&handler)
        .map_err(|e| format!("ATVV ValueChanged: {e}"))?;
    let cccd_ok = control
        .WriteClientCharacteristicConfigurationDescriptorAsync(
            GattClientCharacteristicConfigurationDescriptorValue::Notify,
        )
        .and_then(|op| op.get())
        .map(|s| s == GattCommunicationStatus::Success)
        .unwrap_or(false);
    if !cccd_ok {
        let _ = control.RemoveValueChanged(token);
        return Err("ATVV CCCD notify failed".into());
    }
    tokens.push((control.clone(), token));
    log::info!("Subscribed ATVV control characteristic");

    // 订阅 AUDIO 特征 → ADPCM → VB-CABLE
    if let Some(audio_ch) = audio {
        let voice_audio = Arc::clone(&voice_state);
        let audio_handler = TypedEventHandler::new(
            move |_sender: &Option<GattCharacteristic>,
                  args: &Option<
                windows::Devices::Bluetooth::GenericAttributeProfile::GattValueChangedEventArgs,
            >| {
                if let Some(args) = args {
                    if let Ok(buf) = args.CharacteristicValue() {
                        if let Ok(reader) = DataReader::FromBuffer(&buf) {
                            let len = reader.UnconsumedBufferLength().unwrap_or(0) as usize;
                            let mut data = vec![0u8; len];
                            let _ = reader.ReadBytes(&mut data);
                            handle_atvv_audio(&voice_audio, &data);
                        }
                    }
                }
                Ok(())
            },
        );
        if let Ok(audio_token) = audio_ch.ValueChanged(&audio_handler) {
            let audio_cccd = audio_ch
                .WriteClientCharacteristicConfigurationDescriptorAsync(
                    GattClientCharacteristicConfigurationDescriptorValue::Notify,
                )
                .and_then(|op| op.get())
                .map(|s| s == GattCommunicationStatus::Success)
                .unwrap_or(false);
            if audio_cccd {
                tokens.push((audio_ch.clone(), audio_token));
                log::info!("Subscribed ATVV audio characteristic");
                emit_message(app, "ATVV 麦克风音频已订阅 → VB-CABLE");
            } else {
                let _ = audio_ch.RemoveValueChanged(audio_token);
                log::warn!("ATVV audio CCCD failed");
            }
        }
    } else {
        log::warn!("ATVV audio characteristic not found");
    }

    if let Some(tx) = tx {
        if let Ok(writer) = DataWriter::new() {
            if writer.WriteBytes(&GET_CAPS_V10).is_ok() {
                if let Ok(buf) = writer.DetachBuffer() {
                    let _ = tx
                        .WriteValueWithOptionAsync(&buf, GattWriteOption::WriteWithoutResponse)
                        .and_then(|op| op.get());
                    log::info!("ATVV GET_CAPS sent");
                }
            }
        }
    }
    Ok(true)
}

fn handle_atvv_audio(state: &Arc<Mutex<AtvvVoiceState>>, payload: &[u8]) {
    use crate::bridges::xiaomi::adpcm_decoder::postprocess;
    use crate::bridges::xiaomi::voice_pcm;

    let Ok(mut st) = state.lock() else {
        return;
    };
    if !st.streaming {
        // 按键已按下但 streaming 尚未置位时，音频首帧可直接入流
        if st.remote_pressed {
            st.streaming = true;
            st.pending.clear();
        } else if let Some(t) = st.last_mic_off {
            if t.elapsed() < Duration::from_millis(300) {
                return;
            }
            st.streaming = true;
            st.pending.clear();
            voice_pcm::clear();
            log::info!("XIAOMI ATVV MIC ON session=implicit_audio_race");
        } else {
            st.streaming = true;
            st.pending.clear();
            voice_pcm::clear();
            log::info!("XIAOMI ATVV MIC ON session=implicit_audio_race");
        }
    }
    st.pending.extend_from_slice(payload);
    while st.pending.len() >= st.frame_size {
        let frame_size = st.frame_size;
        let frame: Vec<u8> = st.pending.drain(..frame_size).collect();
        if let Some((pred, idx)) = st.pending_sync.take() {
            st.decoder.reset_with(pred, idx);
        }
        let samples = st.decoder.decode_bytes(&frame);
        let samples = postprocess(&samples, st.gain_db);
        voice_pcm::push_16k(&samples);
        st.frames += 1;
        if st.frames == 1 || st.frames == 10 || st.frames % 200 == 0 {
            let (sent, drop) = voice_pcm::stats();
            log::debug!(
                "XIAOMI ATVV AUDIO frames={} sent={} drop={}",
                st.frames,
                sent,
                drop
            );
        }
    }
}

fn handle_atvv_control(
    app: &AppHandle,
    gate: &KeyEmitGate,
    state: &Arc<Mutex<AtvvVoiceState>>,
    tx: Option<&windows::Devices::Bluetooth::GenericAttributeProfile::GattCharacteristic>,
    payload: &[u8],
) {
    if payload.is_empty() {
        return;
    }
    match payload[0] {
        0x08 => {
            key_mapping::mark_direct_signal("voice");
            key_mapping::mark_direct_signal("mic");
            if let Some(tx) = tx {
                atvv_write_tx(tx, &[0x0C, 0x00], "MIC_OPEN");
            }
            log::info!("XIAOMI ATVV MIC_OPEN request opcode=0x08");
        }
        0x04 => {
            key_mapping::mark_direct_signal("voice");
            key_mapping::mark_direct_signal("mic");
            on_voice_remote_press(app, gate, state);
        }
        0x00 => {
            on_voice_remote_release(app, gate, state);
        }
        0x0A if payload.len() >= 7 => {
            let predictor = i16::from_be_bytes([payload[4], payload[5]]) as i32;
            let step_index = payload[6] as i32;
            if let Ok(mut st) = state.lock() {
                st.pending.clear();
                st.pending_sync = Some((predictor, step_index));
            }
            log::info!("XIAOMI ATVV AUDIO_SYNC predictor={predictor} step={step_index}");
        }
        0x0B if payload.len() >= 7 => {
            let frame_size = u16::from_be_bytes([payload[5], payload[6]]) as usize;
            if let Ok(mut st) = state.lock() {
                if frame_size > 0 {
                    st.frame_size = frame_size;
                }
            }
            log::info!("XIAOMI ATVV CAPS received frame_size={frame_size}");
        }
        0x0B => log::info!("XIAOMI ATVV CAPS received"),
        other => log::debug!("XIAOMI ATVV opcode=0x{other:02X}"),
    }
}

#[cfg(target_os = "windows")]
fn write_gatt_byte(
    ch: &windows::Devices::Bluetooth::GenericAttributeProfile::GattCharacteristic,
    value: u8,
    label: &str,
) {
    use windows::Devices::Bluetooth::GenericAttributeProfile::GattWriteOption;
    use windows::Storage::Streams::DataWriter;

    if let Ok(writer) = DataWriter::new() {
        if writer.WriteBytes(&[value]).is_ok() {
            if let Ok(buf) = writer.DetachBuffer() {
                match ch
                    .WriteValueWithOptionAsync(&buf, GattWriteOption::WriteWithoutResponse)
                    .and_then(|op| op.get())
                {
                    Ok(_) => log::info!("HID write {label}={value}"),
                    Err(e) => log::warn!("HID write {label} failed: {e}"),
                }
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn read_report_reference(
    ch: &windows::Devices::Bluetooth::GenericAttributeProfile::GattCharacteristic,
    report_ref_guid: windows::core::GUID,
) -> (u8, u8) {
    use windows::Devices::Bluetooth::BluetoothCacheMode;
    use windows::Devices::Bluetooth::GenericAttributeProfile::GattCommunicationStatus;
    use windows::Storage::Streams::DataReader;

    let Ok(op) = ch.GetDescriptorsWithCacheModeAsync(BluetoothCacheMode::Uncached) else {
        return (0, 0);
    };
    let Ok(result) = op.get() else {
        return (0, 0);
    };
    if result.Status().ok() != Some(GattCommunicationStatus::Success) {
        return (0, 0);
    }
    let Ok(descriptors) = result.Descriptors() else {
        return (0, 0);
    };
    let n = descriptors.Size().unwrap_or(0);
    for i in 0..n {
        let Ok(desc) = descriptors.GetAt(i) else { continue };
        let Ok(uuid) = desc.Uuid() else { continue };
        if uuid != report_ref_guid {
            continue;
        }
        let Ok(read_op) = desc.ReadValueWithCacheModeAsync(BluetoothCacheMode::Uncached) else {
            continue;
        };
        let Ok(value_result) = read_op.get() else { continue };
        if value_result.Status().ok() != Some(GattCommunicationStatus::Success) {
            continue;
        }
        let Ok(buf) = value_result.Value() else { continue };
        let Ok(reader) = DataReader::FromBuffer(&buf) else { continue };
        let len = reader.UnconsumedBufferLength().unwrap_or(0) as usize;
        let mut data = vec![0u8; len];
        let _ = reader.ReadBytes(&mut data);
        if data.len() >= 2 {
            return (data[0], data[1]);
        }
    }
    (0, 0)
}

fn handle_hid_payload(
    app: &AppHandle,
    active: &Arc<Mutex<HashSet<u16>>>,
    gate: &KeyEmitGate,
    payload: &[u8],
) {
    let usages = parse_hid_usages(payload);
    let Ok(mut guard) = active.lock() else {
        return;
    };
    let pressed: Vec<u16> = usages.difference(&guard).copied().collect();
    let released: Vec<u16> = guard.difference(&usages).copied().collect();
    *guard = usages;
    drop(guard);

    for usage in pressed {
        let btn = match usage {
            0x00E9 => XiaomiButton::VolumeUp,
            0x00EA => XiaomiButton::VolumeDown,
            0x00E2 => XiaomiButton::Mute,
            other => XiaomiButton::from_hid_usage(other),
        };
        let id = btn.to_button_id();
        if id == "unknown" {
            log::debug!("HID usage 0x{usage:04X} ignored");
            continue;
        }
        if gate.try_emit(id) {
            emit_key_and_map(app, id, button_label(id), true);
        } else {
            // 短窗重复边沿：不偷偷注入
            log::debug!("XIAOMI HID gated drop key={id} usage=0x{usage:04X}");
        }
        log::info!("XIAOMI HID key={id} usage=0x{usage:04X}");
    }
    for usage in released {
        let btn = XiaomiButton::from_hid_usage(usage);
        let id = btn.to_button_id();
        if id != "unknown" {
            emit_key_and_map(app, id, button_label(id), false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_hid_usages;

    #[test]
    fn parse_six_byte_usages() {
        // back=0xF1, vol+=0x80
        let data = [0xF1u8, 0x00, 0x80, 0x00, 0x00, 0x00];
        let u = parse_hid_usages(&data);
        assert!(u.contains(&0x00F1));
        assert!(u.contains(&0x0080));
    }

    #[test]
    fn parse_report_id_prefix() {
        let data = [0x01u8, 0xF1, 0x00, 0x81, 0x00, 0x00, 0x00];
        let u = parse_hid_usages(&data);
        assert!(u.contains(&0x00F1));
        assert!(u.contains(&0x0081));
    }

    #[test]
    fn parse_hidogatt_prefix() {
        let data = [0x01u8, 0x00, 0x00, 0xF1, 0x00, 0x00, 0x00, 0x00, 0x00];
        let u = parse_hid_usages(&data);
        assert!(u.contains(&0x00F1));
    }
}
