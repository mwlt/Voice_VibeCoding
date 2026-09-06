//! Standalone 式原生麦克风校验 + 会话保活（对齐 Python `native_audio.py`）
//!
//! T1 的「Mic Device」是 USB 复合设备上的音频接口。冷启动可 >5s；
//! Windows USB 选择暂停 / 设备节能也可能在约十余秒后让采集变静音。
//! USB 桥接运行期间用 cpal 占住输入流（丢弃采样），把冷开麦挪到启动阶段，
//! 并防止端点被挂起；语音结束时不关保活。

use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

#[cfg(target_os = "windows")]
use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_SET_VALUE};
#[cfg(target_os = "windows")]
use winreg::RegKey;

const MMDEVICES_CAPTURE: &str =
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Capture";
const PKEY_DEVICE_FRIENDLY_NAME: &str = "{a45c254e-df1c-4efd-8020-67d146a850e0},2";
const PKEY_ENDPOINT_FRIENDLY_NAME: &str = "{b3f8fa53-0004-438e-9003-51a46e139bfc},6";

static MIC_KEEPALIVE_STOP: AtomicBool = AtomicBool::new(true);
static MIC_KEEPALIVE_THREAD: LazyLock<Mutex<Option<JoinHandle<()>>>> =
    LazyLock::new(|| Mutex::new(None));

/// 返回当前活动的捕获端点友好名列表
pub fn active_capture_endpoint_names() -> Vec<String> {
    #[cfg(target_os = "windows")]
    {
        let mut names = Vec::new();
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        let Ok(root) = hklm.open_subkey(MMDEVICES_CAPTURE) else {
            return names;
        };
        for endpoint_id in root.enum_keys().flatten() {
            let Ok(endpoint) = root.open_subkey(&endpoint_id) else {
                continue;
            };
            let state: u32 = endpoint.get_value("DeviceState").unwrap_or(0);
            if state != 1 {
                continue;
            }
            let Ok(props) = endpoint.open_subkey("Properties") else {
                continue;
            };
            let mut parts = Vec::new();
            for key in [PKEY_DEVICE_FRIENDLY_NAME, PKEY_ENDPOINT_FRIENDLY_NAME] {
                if let Ok(v) = props.get_value::<String, _>(key) {
                    let t = v.trim();
                    if !t.is_empty() && !parts.iter().any(|p: &String| p == t) {
                        parts.push(t.to_string());
                    }
                }
            }
            let label = parts.join(" ");
            if !label.is_empty() {
                names.push(label);
            }
        }
        names
    }
    #[cfg(not(target_os = "windows"))]
    {
        Vec::new()
    }
}

pub fn find_active_capture_endpoint(device: &str) -> Option<String> {
    let needle = device.trim().to_ascii_lowercase();
    if needle.is_empty() {
        return None;
    }
    let names = active_capture_endpoint_names();
    let exact: Vec<_> = names
        .iter()
        .filter(|n| n.to_ascii_lowercase() == needle)
        .cloned()
        .collect();
    let partial: Vec<_> = names
        .iter()
        .filter(|n| n.to_ascii_lowercase().contains(&needle))
        .cloned()
        .collect();
    exact.into_iter().next().or_else(|| partial.into_iter().next())
}

pub fn ensure_mic_device(label: &str) -> Result<String, String> {
    find_active_capture_endpoint(label)
        .ok_or_else(|| format!("Windows 未找到设备自带麦克风：{label}"))
}

/// 尽量关掉 T1（VID_1915）USB 选择暂停 / 增强电源管理，减轻约 20s 麦静音。
/// 无管理员权限时可能失败，仅打日志。
pub fn disable_t1_usb_selective_suspend() {
    #[cfg(target_os = "windows")]
    {
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        let Ok(usb) = hklm.open_subkey_with_flags(
            r"SYSTEM\CurrentControlSet\Enum\USB",
            KEY_READ,
        ) else {
            return;
        };
        let mut touched = 0u32;
        for vid in usb.enum_keys().flatten() {
            if !vid.to_ascii_uppercase().contains("VID_1915") {
                continue;
            }
            let Ok(vid_key) = usb.open_subkey_with_flags(&vid, KEY_READ) else {
                continue;
            };
            for inst in vid_key.enum_keys().flatten() {
                let Ok(inst_key) = vid_key.open_subkey_with_flags(&inst, KEY_READ) else {
                    continue;
                };
                let Ok(params) =
                    inst_key.open_subkey_with_flags("Device Parameters", KEY_SET_VALUE)
                else {
                    continue;
                };
                let _ = params.set_value("SelectiveSuspendEnabled", &0u32);
                let _ = params.set_value("EnhancedPowerManagementEnabled", &0u32);
                touched += 1;
            }
        }
        if touched > 0 {
            log::info!("T1 USB power mgmt cleared on {touched} instance(s) (VID_1915)");
        }
    }
}

/// 占住 Mic Device 输入流（USB 桥接全程 / 语音闩锁）。
/// cpal::Stream 非 Send，放在专用线程里持有。已在跑则直接返回，避免反复冷开麦。
pub fn start_mic_keepalive(label: &str) -> Result<(), String> {
    {
        let guard = MIC_KEEPALIVE_THREAD.lock();
        if let Some(handle) = guard.as_ref() {
            if !handle.is_finished() && !MIC_KEEPALIVE_STOP.load(Ordering::SeqCst) {
                return Ok(());
            }
        }
    }
    stop_mic_keepalive();
    let label = label.to_string();
    MIC_KEEPALIVE_STOP.store(false, Ordering::SeqCst);
    let handle = thread::Builder::new()
        .name("t1-mic-keepalive".into())
        .spawn(move || {
            use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

            let host = cpal::default_host();
            let needle = label.trim().to_ascii_lowercase();
            let device = match host.input_devices() {
                Ok(mut devs) => devs.find(|d| {
                    d.name()
                        .ok()
                        .map(|n| n.to_ascii_lowercase().contains(&needle))
                        .unwrap_or(false)
                }),
                Err(e) => {
                    log::warn!("T1 mic keepalive enum failed: {e}");
                    return;
                }
            };
            let Some(device) = device else {
                log::warn!("T1 mic keepalive: cpal 未找到含 [{label}] 的输入设备");
                return;
            };
            let name = device.name().unwrap_or_else(|_| label.clone());
            let config = match device.default_input_config() {
                Ok(c) => c,
                Err(e) => {
                    log::warn!("T1 mic keepalive config failed: {e}");
                    return;
                }
            };
            let running = Arc::new(AtomicBool::new(true));
            let running_cb = Arc::clone(&running);
            let stream = match config.sample_format() {
                cpal::SampleFormat::I16 => device.build_input_stream(
                    &config.config(),
                    move |data: &[i16], _| {
                        if running_cb.load(Ordering::Relaxed) {
                            let _ = data.len();
                        }
                    },
                    |e| log::warn!("T1 mic keepalive stream error: {e}"),
                    None,
                ),
                cpal::SampleFormat::F32 => device.build_input_stream(
                    &config.config(),
                    move |data: &[f32], _| {
                        if running_cb.load(Ordering::Relaxed) {
                            let _ = data.len();
                        }
                    },
                    |e| log::warn!("T1 mic keepalive stream error: {e}"),
                    None,
                ),
                other => {
                    log::warn!("T1 mic keepalive unsupported format: {other:?}");
                    return;
                }
            };
            let stream = match stream {
                Ok(s) => s,
                Err(e) => {
                    log::warn!("T1 mic keepalive build failed: {e}");
                    return;
                }
            };
            if let Err(e) = stream.play() {
                log::warn!("T1 mic keepalive play failed: {e}");
                return;
            }
            log::info!(
                "T1 AUDIO KEEPALIVE ON device={name} format={:?}",
                config.sample_format()
            );
            while !MIC_KEEPALIVE_STOP.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(200));
            }
            running.store(false, Ordering::SeqCst);
            drop(stream);
            log::info!("T1 AUDIO KEEPALIVE OFF");
        })
        .map_err(|e| format!("spawn mic keepalive: {e}"))?;
    *MIC_KEEPALIVE_THREAD.lock() = Some(handle);
    Ok(())
}

pub fn stop_mic_keepalive() {
    MIC_KEEPALIVE_STOP.store(true, Ordering::SeqCst);
    if let Some(handle) = MIC_KEEPALIVE_THREAD.lock().take() {
        let _ = handle.join();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_needle_returns_none() {
        assert!(find_active_capture_endpoint("").is_none());
        assert!(find_active_capture_endpoint("   ").is_none());
    }
}
