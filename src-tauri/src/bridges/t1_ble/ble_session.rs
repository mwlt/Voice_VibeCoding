//! T1 BLE ATVV 会话（独立实现，不调用 `bridges::xiaomi::input_session`）
//!
//! - 订阅 Control / Audio
//! - START_SEARCH(0x08) → MIC_OPEN
//! - AUDIO_START(0x04) / AUDIO_STOP(0x00) → 快捷键 + PCM
//! - 会话中每 8s 发 MIC_EXTEND，突破 USB 麦约 15s 限制

use crate::bridges::t1::ble_adpcm::{postprocess, AdpcmDecoder};
use crate::bridges::t1::ble_connect::{
    ATVV_AUDIO_UUID, ATVV_CONTROL_UUID, ATVV_SERVICE_UUID, ATVV_TX_UUID,
};
use crate::bridges::t1::ble_pcm;
use crate::bridges::t1::ble_runtime::T1BleRuntime;
use crate::bridges::t1::ble_voice;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

const GET_CAPS_V10: [u8; 6] = [0x0A, 0x01, 0x00, 0x00, 0x03, 0x03];
const MIC_EXTEND_INTERVAL: Duration = Duration::from_secs(8);
/// 麦已开但长时间无 ADPCM：强制收口，否则 MIC_EXTEND 空转、CABLE 只播静音。
const MIC_STALE_NO_AUDIO: Duration = Duration::from_secs(12);

/// AUDIO_START 是否视为新会话（仅新会话 CLEAR；推流中 CLEAR 会掏空 CABLE）。
/// 映射点按不在此决策——由 START_SEARCH/AUDIO_START 调 on_remote_press，450ms 去重。
pub fn audio_start_should_clear(already_streaming: bool, awaiting_audio_start: bool) -> bool {
    awaiting_audio_start || !already_streaming
}

/// START_SEARCH → 开/关麦：只看主机开麦意图。
/// - 未开麦 → Open（关麦后短抑制窗内除外，挡固件回声）
/// - 已开麦 + 同一次按连发 → SkipLive
/// - 已开麦 + 新一次按 → CloseEnd
pub fn start_search_mic_action(
    host_mic_wanted: bool,
    within_dup_window: bool,
    reopen_suppressed: bool,
) -> StartSearchMicAction {
    if host_mic_wanted {
        if within_dup_window {
            return StartSearchMicAction::SkipLive;
        }
        return StartSearchMicAction::CloseEnd;
    }
    if reopen_suppressed {
        return StartSearchMicAction::SkipAfterClose;
    }
    StartSearchMicAction::Open
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartSearchMicAction {
    Open,
    SkipLive,
    /// 用户再按结束：MIC_CLOSE + 停 PCM。
    CloseEnd,
    /// 刚 CloseEnd，忽略紧随的 START_SEARCH，避免立刻又 MIC_OPEN。
    SkipAfterClose,
}

const BATTERY_SERVICE: u128 = 0x0000180f_0000_1000_8000_00805f9b34fb;
const BATTERY_LEVEL: u128 = 0x00002a19_0000_1000_8000_00805f9b34fb;

struct AtvvVoiceState {
    decoder: AdpcmDecoder,
    streaming: bool,
    pending: Vec<u8>,
    frame_size: usize,
    pending_sync: Option<(i32, i32)>,
    frames: u64,
    remote_pressed: bool,
    last_extend: Instant,
    /// 麦会话开始时间（无 last_audio 时用它判定卡死）。
    session_started: Option<Instant>,
    /// 最近一次解码到 PCM 的时间；长时间无帧则强制关麦，避免 CABLE 空转。
    last_audio: Option<Instant>,
    /// MIC_OPEN 后、正式 AUDIO_START 前：抢跑帧不得把 fresh 吃掉（对齐小米 arm）。
    awaiting_audio_start: bool,
    /// 主机是否希望开麦。CloseEnd/AUDIO_STOP 后为 false。
    /// 本机日志：MIC_CLOSE 后遥控器仍回 AUDIO_START，若不挡住会把会话重新打开。
    host_mic_wanted: bool,
}

/// 阻塞运行直到 `runtime` 请求停止
pub fn run_atvv_session(
    app: AppHandle,
    address_u64: u64,
    atvv_interface_id: String,
    runtime: Arc<T1BleRuntime>,
) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        windows_run(app, address_u64, atvv_interface_id, runtime)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app, address_u64, atvv_interface_id, runtime);
        Err("仅支持 Windows".into())
    }
}

fn emit_msg(app: &AppHandle, message: &str) {
    let _ = app.emit(
        "t1-ble",
        serde_json::json!({ "phase": "session", "message": message }),
    );
}

/// 从 t1.json 应用增益。启动时 lib 会把小米 `gain_db`（常为 0）写入全局，T1 必须覆盖。
fn apply_t1_voice_gain(app: &AppHandle) {
    use tauri::Manager;
    let gain = app
        .try_state::<crate::config::manager::ConfigManager>()
        .and_then(|m| m.get_device_config("t1_ble").ok())
        .map(|c| c.gain_db)
        .unwrap_or(crate::bridges::shared::voice_gain::GAIN_DB_DEFAULT);
    crate::bridges::shared::voice_gain::set_gain_db(gain);
}

#[cfg(target_os = "windows")]
fn windows_run(
    app: AppHandle,
    address_u64: u64,
    atvv_interface_id: String,
    runtime: Arc<T1BleRuntime>,
) -> Result<(), String> {
    use windows::core::{GUID, HSTRING};
    use windows::Devices::Bluetooth::GenericAttributeProfile::{
        GattCharacteristic, GattClientCharacteristicConfigurationDescriptorValue,
        GattCommunicationStatus, GattDeviceService, GattSharingMode,
    };
    use windows::Devices::Bluetooth::{
        BluetoothCacheMode, BluetoothConnectionStatus, BluetoothLEDevice,
    };
    use windows::Foundation::TypedEventHandler;
    use windows::Storage::Streams::DataReader;

    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_MULTITHREADED,
        );
    }

    let _ = crate::audio::pcm_router::ensure_audio_router_process();
    ble_pcm::warmup_async();

    let device = BluetoothLEDevice::FromBluetoothAddressAsync(address_u64)
        .map_err(|e| format!("打开 T1 BLE: {e}"))?
        .get()
        .map_err(|e| format!("打开 T1 BLE get: {e}"))?;
    match device.ConnectionStatus() {
        Ok(status) if status == BluetoothConnectionStatus::Disconnected => {
            return Err("T1-Remote 蓝牙已断开".into());
        }
        Ok(_) => {}
        Err(e) => return Err(format!("读取连接状态失败: {e}")),
    }

    let runtime_conn = Arc::clone(&runtime);
    let _conn_token = device.ConnectionStatusChanged(&TypedEventHandler::new(
        move |sender: &Option<BluetoothLEDevice>, _args| {
            if let Some(dev) = sender {
                if let Ok(status) = dev.ConnectionStatus() {
                    if status == BluetoothConnectionStatus::Disconnected {
                        log::warn!("T1 BLE disconnected");
                        runtime_conn.request_stop();
                    }
                }
            }
            Ok(())
        },
    ));

    let atvv_service_guid = GUID::from_u128(uuid_u128(ATVV_SERVICE_UUID));
    let tx_guid = GUID::from_u128(uuid_u128(ATVV_TX_UUID));
    let audio_guid = GUID::from_u128(uuid_u128(ATVV_AUDIO_UUID));
    let control_guid = GUID::from_u128(uuid_u128(ATVV_CONTROL_UUID));

    let atvv = if !atvv_interface_id.is_empty() {
        let id = HSTRING::from(atvv_interface_id.as_str());
        match GattDeviceService::FromIdAsync(&id).and_then(|op| op.get()) {
            Ok(svc) => {
                let _ = svc
                    .OpenAsync(GattSharingMode::SharedReadOnly)
                    .and_then(|op| op.get());
                Some(svc)
            }
            Err(e) => {
                log::warn!("T1 BLE FromId ATVV failed: {e}");
                None
            }
        }
    } else {
        None
    };

    let atvv = match atvv {
        Some(s) => s,
        None => {
            let result = device
                .GetGattServicesForUuidWithCacheModeAsync(
                    atvv_service_guid,
                    BluetoothCacheMode::Uncached,
                )
                .map_err(|e| format!("GetGattServices ATVV: {e}"))?
                .get()
                .map_err(|e| format!("GetGattServices ATVV get: {e}"))?;
            if result.Status().ok() != Some(GattCommunicationStatus::Success) {
                return Err("未找到 T1 ATVV 服务".into());
            }
            let services = result.Services().map_err(|e| e.to_string())?;
            if services.Size().unwrap_or(0) == 0 {
                return Err("T1 ATVV 服务列表为空".into());
            }
            let svc = services.GetAt(0).map_err(|e| e.to_string())?;
            let _ = svc
                .OpenAsync(GattSharingMode::SharedReadOnly)
                .and_then(|op| op.get());
            svc
        }
    };

    let _ = atvv
        .OpenAsync(GattSharingMode::SharedReadOnly)
        .and_then(|op| op.get())
        .or_else(|_| {
            atvv.OpenAsync(GattSharingMode::SharedReadAndWrite)
                .and_then(|op| op.get())
        });

    let chars_result = atvv
        .GetCharacteristicsWithCacheModeAsync(BluetoothCacheMode::Uncached)
        .map_err(|e| e.to_string())?
        .get()
        .map_err(|e| e.to_string())?;
    let chars_result = if chars_result.Status().ok() == Some(GattCommunicationStatus::Success) {
        chars_result
    } else {
        atvv.GetCharacteristicsWithCacheModeAsync(BluetoothCacheMode::Cached)
            .map_err(|e| e.to_string())?
            .get()
            .map_err(|e| e.to_string())?
    };
    if chars_result.Status().ok() != Some(GattCommunicationStatus::Success) {
        return Err("读取 T1 ATVV 特征失败".into());
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
        } else if uuid == control_guid {
            control = Some(ch);
        }
    }
    let Some(control) = control else {
        return Err("T1 ATVV 缺少 Control 特征".into());
    };
    let Some(tx) = tx else {
        return Err("T1 ATVV 缺少 TX 特征".into());
    };

    let voice_state = Arc::new(Mutex::new(AtvvVoiceState {
        decoder: AdpcmDecoder::new_ima(),
        streaming: false,
        pending: Vec::new(),
        frame_size: 120,
        pending_sync: None,
        frames: 0,
        remote_pressed: false,
        last_extend: Instant::now(),
        session_started: None,
        last_audio: None,
        awaiting_audio_start: false,
        host_mic_wanted: false,
    }));

    // T1 用自己的 gain_db；勿用启动时写入的小米全局值（xiaomi.json 常为 0 → CABLE 近静音）
    apply_t1_voice_gain(&app);
    // 仅会话建立时设一次默认麦；按键时再跑 EnsureMic 会打坏 WASAPI→CABLE 流（环回变静音）
    crate::audio::vb_cable::ensure_cable_mic_for_voice_async();

    crate::bridges::t1::ble_host::set_atvv_ok(false);
    crate::bridges::t1::ble_voice_meter::reset();
    ble_voice::set_mic_session_wanted(false);

    let mut tokens: Vec<(GattCharacteristic, windows::Foundation::EventRegistrationToken)> =
        Vec::new();

    // Control notify
    {
        let app2 = app.clone();
        let voice_ctrl = Arc::clone(&voice_state);
        let tx_for_mic = tx.clone();
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
                            handle_control(&app2, &voice_ctrl, &tx_for_mic, &data);
                        }
                    }
                }
                Ok(())
            },
        );
        let token = control
            .ValueChanged(&handler)
            .map_err(|e| format!("Control ValueChanged: {e}"))?;
        let cccd = control
            .WriteClientCharacteristicConfigurationDescriptorAsync(
                GattClientCharacteristicConfigurationDescriptorValue::Notify,
            )
            .and_then(|op| op.get());
        if cccd.ok() != Some(GattCommunicationStatus::Success) {
            let _ = control.RemoveValueChanged(token);
            return Err("T1 ATVV Control CCCD 失败".into());
        }
        tokens.push((control.clone(), token));
        log::info!("T1 BLE subscribed ATVV control");
        crate::bridges::t1::ble_host::set_atvv_ok(true);
    }

    // Audio notify
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
                            handle_audio(&voice_audio, &data);
                        }
                    }
                }
                Ok(())
            },
        );
        if let Ok(audio_token) = audio_ch.ValueChanged(&audio_handler) {
            let ok = audio_ch
                .WriteClientCharacteristicConfigurationDescriptorAsync(
                    GattClientCharacteristicConfigurationDescriptorValue::Notify,
                )
                .and_then(|op| op.get())
                .map(|s| s == GattCommunicationStatus::Success)
                .unwrap_or(false);
            if ok {
                tokens.push((audio_ch.clone(), audio_token));
                log::info!("T1 BLE subscribed ATVV audio");
                emit_msg(&app, "ATVV 音频已订阅 → CABLE（请在豆包选 CABLE Output）");
            } else {
                let _ = audio_ch.RemoveValueChanged(audio_token);
                log::warn!("T1 BLE audio CCCD failed");
            }
        }
    }

    // GET_CAPS
    write_tx(&tx, &GET_CAPS_V10, "GET_CAPS");
    emit_msg(&app, "T1 BLE ATVV 已就绪，按语音键说话");
    crate::bridges::t1::ble_host::set_atvv_ok(true);

    let mut battery_ch = setup_battery_monitor(&app, &device, &mut tokens);
    let mut last_battery: Option<u8> = None;
    let mut since_batt = Instant::now();
    if let Some(ch) = battery_ch.as_ref() {
        if let Some(level) = read_battery_level(ch) {
            publish_battery(&app, level, &mut last_battery, true);
        }
    }

    // Keepalive + wait loop
    while !runtime.should_stop() {
        // 仅「长时间无 PCM」收口；不对点按做空闲关麦（对齐小米：麦由 AUDIO_STOP / 用户再按驱动）
        let stale_mic = voice_state.lock().ok().and_then(|s| {
            if !s.remote_pressed {
                return None;
            }
            let since = s
                .last_audio
                .or(s.session_started)
                .map(|t| t.elapsed())
                .unwrap_or(Duration::ZERO);
            if since >= MIC_STALE_NO_AUDIO
                && s.last_audio
                    .map(|t| t.elapsed() >= MIC_STALE_NO_AUDIO)
                    .unwrap_or(true)
            {
                Some(since)
            } else {
                None
            }
        });
        if let Some(since) = stale_mic {
            log::warn!(
                "T1 BLE mic stale no-audio for {}ms — force MIC_CLOSE + END",
                since.as_millis()
            );
            #[cfg(target_os = "windows")]
            write_tx(&tx, &[0x0C, 0x00, 0x00], "MIC_CLOSE");
            if let Ok(mut s) = voice_state.lock() {
                s.remote_pressed = false;
                s.streaming = false;
                s.pending.clear();
                s.session_started = None;
                s.last_audio = None;
                s.awaiting_audio_start = false;
                s.host_mic_wanted = false;
                s.decoder.reset();
            }
            ble_voice::set_mic_session_wanted(false);
            ble_voice::arm_reopen_suppress();
            ble_voice::on_remote_release(&app);
            crate::bridges::t1::ble_voice_meter::set_session(false);
        }
        if voice_state
            .lock()
            .map(|s| {
                s.host_mic_wanted
                    && s.remote_pressed
                    && s.last_extend.elapsed() >= MIC_EXTEND_INTERVAL
            })
            .unwrap_or(false)
        {
            write_tx(&tx, &[0x0E, 0x00], "MIC_EXTEND");
            if let Ok(mut s) = voice_state.lock() {
                s.last_extend = Instant::now();
            }
        }
        if let Some(ch) = battery_ch.as_ref() {
            let due = since_batt.elapsed() >= Duration::from_secs(45)
                || (last_battery.is_none() && since_batt.elapsed() >= Duration::from_secs(3));
            if due {
                since_batt = Instant::now();
                if let Some(level) = read_battery_level(ch) {
                    publish_battery(&app, level, &mut last_battery, false);
                }
            }
        } else if since_batt.elapsed() >= Duration::from_secs(8) {
            since_batt = Instant::now();
            battery_ch = setup_battery_monitor(&app, &device, &mut tokens);
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    ble_voice::force_release(&app);
    ble_pcm::end_session();
    ble_pcm::shutdown();
    crate::bridges::t1::ble_host::set_atvv_ok(false);
    crate::bridges::t1::ble_voice_meter::reset();
    for (ch, token) in tokens {
        let _ = ch.RemoveValueChanged(token);
    }
    Ok(())
}

fn uuid_u128(s: &str) -> u128 {
    let hex: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    u128::from_str_radix(&hex, 16).unwrap_or(0)
}

#[cfg(target_os = "windows")]
fn write_tx(
    tx: &windows::Devices::Bluetooth::GenericAttributeProfile::GattCharacteristic,
    bytes: &[u8],
    label: &str,
) {
    use windows::Devices::Bluetooth::GenericAttributeProfile::GattWriteOption;
    use windows::Storage::Streams::DataWriter;
    if let Ok(writer) = DataWriter::new() {
        if writer.WriteBytes(bytes).is_ok() {
            if let Ok(buf) = writer.DetachBuffer() {
                match tx
                    .WriteValueWithOptionAsync(&buf, GattWriteOption::WriteWithoutResponse)
                    .and_then(|op| op.get())
                {
                    Ok(_) => log::info!("T1 BLE {label} sent"),
                    Err(e) => log::warn!("T1 BLE {label} failed: {e}"),
                }
            }
        }
    }
}

fn handle_control(
    app: &AppHandle,
    state: &Arc<Mutex<AtvvVoiceState>>,
    tx: &windows::Devices::Bluetooth::GenericAttributeProfile::GattCharacteristic,
    payload: &[u8],
) {
    if payload.is_empty() {
        return;
    }
    let raw = payload
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join("-");
    match payload[0] {
        0x08 => {
            let within_dup = ble_voice::is_within_voice_dup_window();
            let reopen_suppressed = ble_voice::is_reopen_suppressed();
            let host_mic_wanted = state
                .lock()
                .map(|s| s.host_mic_wanted)
                .unwrap_or(false);
            let action =
                start_search_mic_action(host_mic_wanted, within_dup, reopen_suppressed);
            #[cfg(target_os = "windows")]
            match action {
                StartSearchMicAction::SkipLive => {
                    log::info!("T1 BLE START_SEARCH skipped (mic open, same press)");
                }
                StartSearchMicAction::SkipAfterClose => {
                    log::info!("T1 BLE START_SEARCH skipped MIC_OPEN (reopen suppressed after close)");
                }
                StartSearchMicAction::CloseEnd => {
                    log::info!("T1 BLE START_SEARCH → MIC_CLOSE (user end / toggle off)");
                    write_tx(tx, &[0x0C, 0x00, 0x00], "MIC_CLOSE");
                    if let Ok(mut st) = state.lock() {
                        st.remote_pressed = false;
                        st.streaming = false;
                        st.pending.clear();
                        st.session_started = None;
                        st.last_audio = None;
                        st.frames = 0;
                        st.awaiting_audio_start = false;
                        st.host_mic_wanted = false;
                        st.decoder.reset();
                    }
                    ble_voice::set_mic_session_wanted(false);
                    ble_pcm::end_session();
                    crate::bridges::t1::ble_voice_meter::set_session(false);
                }
                StartSearchMicAction::Open => {
                    write_tx(tx, &[0x0C, 0x00, 0x01], "MIC_OPEN");
                    if let Ok(mut st) = state.lock() {
                        st.remote_pressed = true;
                        st.session_started = Some(Instant::now());
                        st.last_extend = Instant::now();
                        st.awaiting_audio_start = true;
                        st.host_mic_wanted = true;
                        st.decoder.reset();
                    }
                    ble_voice::set_mic_session_wanted(true);
                }
            }
            apply_t1_voice_gain(app);
            // 尽早吞 0xAA + 关搜索，再开语音（与 HID 共用 on_ac_search_hid_seen）
            crate::bridges::t1::native_suppress::on_ac_search_hid_seen();
            // 仅真实开/关手势注入；Skip* 是同一次按的回声。
            if !matches!(
                action,
                StartSearchMicAction::SkipLive | StartSearchMicAction::SkipAfterClose
            ) {
                ble_voice::on_remote_press(app);
            }
            // CloseEnd：抑制窗从 inject 之后起算，挡固件回声，且 inject 侧也会认这个窗。
            if matches!(action, StartSearchMicAction::CloseEnd) {
                ble_voice::arm_reopen_suppress();
            }
            if !matches!(
                action,
                StartSearchMicAction::CloseEnd | StartSearchMicAction::SkipAfterClose
            ) {
                crate::bridges::t1::ble_voice_meter::set_session(true);
            }
            emit_msg(
                app,
                &format!("ATVV START_SEARCH(0x08) raw={raw} action={action:?} → voice"),
            );
            log::info!("T1 BLE START_SEARCH → {action:?} + voice press raw={raw}");
        }
        0x04 => {
            // 本机：MIC_CLOSE 后遥控器仍可能回 AUDIO_START；主机已关麦则忽略，否则会话永不结束。
            let ignored = state
                .lock()
                .map(|s| !s.host_mic_wanted)
                .unwrap_or(true);
            if ignored {
                log::info!("T1 BLE AUDIO_START ignored (host mic closed) raw={raw}");
                emit_msg(app, &format!("ATVV AUDIO_START ignored (host closed) raw={raw}"));
                return;
            }
            // 对齐小米 arm_atvv_voice_session：AUDIO_START 时复位解码/CLEAR；
            // MIC_OPEN 后抢跑帧不得把 fresh 吃掉（awaiting_audio_start）。
            let fresh = if let Ok(mut st) = state.lock() {
                let fresh =
                    audio_start_should_clear(st.streaming, st.awaiting_audio_start);
                st.remote_pressed = true;
                st.streaming = true;
                st.awaiting_audio_start = false;
                st.last_extend = Instant::now();
                if fresh {
                    st.pending.clear();
                    st.frames = 0;
                    st.last_audio = None;
                    st.session_started = Some(Instant::now());
                    st.decoder.reset();
                }
                fresh
            } else {
                true
            };
            if fresh {
                ble_pcm::clear();
            }
            apply_t1_voice_gain(app);
            crate::bridges::t1::ble_voice_meter::set_session(true);
            emit_msg(app, &format!("ATVV AUDIO_START(0x04) raw={raw} fresh={fresh}"));
            log::info!("T1 BLE AUDIO_START raw={raw} fresh={fresh}");
        }
        0x00 => {
            if let Ok(mut st) = state.lock() {
                st.remote_pressed = false;
                st.streaming = false;
                st.pending.clear();
                st.session_started = None;
                st.last_audio = None;
                st.awaiting_audio_start = false;
                st.host_mic_wanted = false;
            }
            ble_voice::set_mic_session_wanted(false);
            // 遥控器报停后主机也发 MIC_CLOSE，避免只停 PCM 而麦口仍开、EXTEND 空转。
            #[cfg(target_os = "windows")]
            write_tx(tx, &[0x0C, 0x00, 0x00], "MIC_CLOSE");
            ble_voice::on_remote_release(app);
            ble_voice::arm_reopen_suppress();
            crate::bridges::t1::ble_voice_meter::set_session(false);
            emit_msg(app, &format!("ATVV AUDIO_STOP(0x00) raw={raw}"));
            log::info!("T1 BLE AUDIO_STOP raw={raw}");
        }
        0x0A if payload.len() >= 7 => {
            let predictor = i16::from_be_bytes([payload[4], payload[5]]) as i32;
            let step_index = payload[6] as i32;
            if let Ok(mut st) = state.lock() {
                st.pending.clear();
                st.pending_sync = Some((predictor, step_index));
            }
            emit_msg(
                app,
                &format!("ATVV AUDIO_SYNC(0x0A) predictor={predictor} step={step_index} raw={raw}"),
            );
            log::info!("T1 BLE AUDIO_SYNC predictor={predictor} step={step_index}");
        }
        0x0B if payload.len() >= 7 => {
            let frame_size = u16::from_be_bytes([payload[5], payload[6]]) as usize;
            if let Ok(mut st) = state.lock() {
                if frame_size > 0 {
                    st.frame_size = frame_size;
                }
            }
            emit_msg(app, &format!("ATVV CAPS(0x0B) frame_size={frame_size} raw={raw}"));
            log::info!("T1 BLE CAPS frame_size={frame_size}");
        }
        0x0B => {
            emit_msg(app, &format!("ATVV CAPS(0x0B) raw={raw}"));
            log::info!("T1 BLE CAPS received");
        }
        other => {
            emit_msg(app, &format!("ATVV opcode=0x{other:02X} raw={raw}"));
            log::info!("T1 BLE opcode=0x{other:02X} raw={raw}");
        }
    }
}

#[cfg(target_os = "windows")]
fn publish_battery(app: &AppHandle, level: u8, last: &mut Option<u8>, force_log: bool) {
    use crate::bridges::{BridgeState, BridgeType};
    use tauri::Manager;

    let changed = last.map(|v| v != level).unwrap_or(true);
    *last = Some(level);
    if let Some(state) = app.try_state::<BridgeState>() {
        state.update_battery_level(BridgeType::T1Ble, level);
    }
    if force_log || changed {
        let _ = app.emit(
            "t1-ble",
            serde_json::json!({
                "phase": "battery",
                "level": level,
                "message": format!("电量 {level}%"),
            }),
        );
        log::info!("T1 BLE BATTERY level={level}%");
    }
}

#[cfg(target_os = "windows")]
fn setup_battery_monitor(
    app: &AppHandle,
    device: &windows::Devices::Bluetooth::BluetoothLEDevice,
    tokens: &mut Vec<(
        windows::Devices::Bluetooth::GenericAttributeProfile::GattCharacteristic,
        windows::Foundation::EventRegistrationToken,
    )>,
) -> Option<windows::Devices::Bluetooth::GenericAttributeProfile::GattCharacteristic> {
    use windows::core::GUID;
    use windows::Devices::Bluetooth::BluetoothCacheMode;
    use windows::Devices::Bluetooth::GenericAttributeProfile::{
        GattCharacteristic, GattClientCharacteristicConfigurationDescriptorValue,
        GattCommunicationStatus, GattOpenStatus, GattSharingMode,
    };
    use windows::Foundation::TypedEventHandler;
    use windows::Storage::Streams::DataReader;
    use tauri::Manager;

    let battery_guid = GUID::from_u128(BATTERY_SERVICE);
    let result = device
        .GetGattServicesForUuidWithCacheModeAsync(battery_guid, BluetoothCacheMode::Uncached)
        .and_then(|op| op.get())
        .ok()?;
    if result.Status().ok() != Some(GattCommunicationStatus::Success) {
        return None;
    }
    let services = result.Services().ok()?;
    if services.Size().unwrap_or(0) == 0 {
        return None;
    }
    let service = services.GetAt(0).ok()?;
    match service.OpenAsync(GattSharingMode::SharedReadOnly) {
        Ok(op) => match op.get() {
            Ok(status)
                if status == GattOpenStatus::Success
                    || status == GattOpenStatus::AlreadyOpened => {}
            Ok(status) => log::warn!("T1 BLE BATTERY OpenAsync status={status:?}"),
            Err(e) => log::warn!("T1 BLE BATTERY OpenAsync: {e}"),
        },
        Err(e) => log::warn!("T1 BLE BATTERY OpenAsync unavailable: {e}"),
    }

    let level_guid = GUID::from_u128(BATTERY_LEVEL);
    let chars_result = service
        .GetCharacteristicsForUuidWithCacheModeAsync(level_guid, BluetoothCacheMode::Uncached)
        .ok()?
        .get()
        .ok()?;
    if chars_result.Status().ok() != Some(GattCommunicationStatus::Success) {
        return None;
    }
    let chars = chars_result.Characteristics().ok()?;
    if chars.Size().unwrap_or(0) == 0 {
        return None;
    }
    let ch = chars.GetAt(0).ok()?;

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
                                    state.update_battery_level(
                                        crate::bridges::BridgeType::T1Ble,
                                        level,
                                    );
                                }
                                let _ = app2.emit(
                                    "t1-ble",
                                    serde_json::json!({
                                        "phase": "battery",
                                        "level": level,
                                        "message": format!("电量 {level}%"),
                                    }),
                                );
                                log::info!("T1 BLE BATTERY notify level={level}%");
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
            log::info!("T1 BLE BATTERY notify subscribed");
        } else {
            let _ = ch.RemoveValueChanged(token);
            log::info!("T1 BLE BATTERY notify unsupported; will poll");
        }
    }
    Some(ch)
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

fn handle_audio(state: &Arc<Mutex<AtvvVoiceState>>, payload: &[u8]) {
    let Ok(mut st) = state.lock() else {
        return;
    };
    // 主机已关麦：丢掉残留 ADPCM，绝不能再 auto 把 streaming 置回 true。
    if !st.host_mic_wanted {
        st.pending.clear();
        st.streaming = false;
        return;
    }
    if !st.streaming {
        st.streaming = true;
        st.pending.clear();
        // CLEAR 只由 AUDIO_START fresh 负责；这里再 CLEAR 会掏空正在播的缓冲。
    }
    st.pending.extend_from_slice(payload);
    while st.pending.len() >= st.frame_size {
        let frame_size = st.frame_size;
        let frame: Vec<u8> = st.pending.drain(..frame_size).collect();
        if let Some((pred, idx)) = st.pending_sync.take() {
            st.decoder.reset_with(pred, idx);
        }
        let samples = st.decoder.decode_bytes(&frame);
        let gain_db = crate::bridges::shared::voice_gain::gain_db();
        let samples = postprocess(&samples, gain_db);
        ble_pcm::push_16k(&samples);
        st.frames += 1;
        st.last_audio = Some(Instant::now());
        if st.frames == 1 || st.frames == 10 || st.frames % 200 == 0 {
            let mut sum = 0.0f64;
            for &s in &samples {
                let v = s as f64;
                sum += v * v;
            }
            let rms = ((sum / samples.len().max(1) as f64).sqrt() / 32768.0).clamp(0.0, 1.0);
            if st.frames == 1 {
                log::info!(
                    "T1 BLE AUDIO first frame → CABLE samples={} gain={gain_db:.1}dB rms={rms:.4}",
                    samples.len()
                );
            } else {
                log::info!(
                    "T1 BLE AUDIO frames={} gain={gain_db:.1}dB rms={rms:.4}",
                    st.frames
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_search_opens_when_mic_closed() {
        assert_eq!(
            start_search_mic_action(false, false, false),
            StartSearchMicAction::Open
        );
        // 去重窗占用但未开麦：仍应开（HID 不再抢注入后，这是「开始」）
        assert_eq!(
            start_search_mic_action(false, true, false),
            StartSearchMicAction::Open
        );
    }

    #[test]
    fn after_close_suppresses_immediate_reopen() {
        assert_eq!(
            start_search_mic_action(false, false, true),
            StartSearchMicAction::SkipAfterClose
        );
        assert_eq!(
            start_search_mic_action(false, true, true),
            StartSearchMicAction::SkipAfterClose
        );
    }

    #[test]
    fn mic_open_same_press_skips_close() {
        assert_eq!(
            start_search_mic_action(true, true, false),
            StartSearchMicAction::SkipLive
        );
    }

    #[test]
    fn mic_open_new_press_closes() {
        assert_eq!(
            start_search_mic_action(true, false, false),
            StartSearchMicAction::CloseEnd
        );
        // 开麦意图优先于关麦抑制
        assert_eq!(
            start_search_mic_action(true, false, true),
            StartSearchMicAction::CloseEnd
        );
    }

    #[test]
    fn audio_start_clears_when_awaiting_even_if_streaming() {
        assert!(audio_start_should_clear(true, true));
        assert!(!audio_start_should_clear(true, false));
        assert!(audio_start_should_clear(false, false));
    }
}
