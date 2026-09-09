//! Standalone 式原生麦克风校验（对齐 Python `native_audio.py`）
//!
//! T1 的「Mic Device」是 USB 复合设备上的音频接口。
//! Python **只验证端点存在并记账**，**不**打开 WASAPI/cpal 采集流——
//! 由语音输入法自己打开 Mic Device。
//!
//! 历史实现用 cpal 长期占流「保活」，在单客户/弱共享驱动上会把采样吃光，
//! 表现为：语音键能唤醒输入法，但所有输入法都听不到声音。

use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(target_os = "windows")]
use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_SET_VALUE};
#[cfg(target_os = "windows")]
use winreg::RegKey;

const MMDEVICES_CAPTURE: &str =
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Capture";
const PKEY_DEVICE_FRIENDLY_NAME: &str = "{a45c254e-df1c-4efd-8020-67d146a850e0},2";
const PKEY_ENDPOINT_FRIENDLY_NAME: &str = "{b3f8fa53-0004-438e-9003-51a46e139bfc},6";

/// 会话记账（对齐 Python NativeAudioSessionClient；不占硬件流）
static NATIVE_MIC_SESSION_OPEN: AtomicBool = AtomicBool::new(false);

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

/// 当前系统默认捕获端点是否已是 Mic Device（输入法多数跟默认麦走）。
pub fn default_capture_is_mic_device(label: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        use cpal::traits::{DeviceTrait, HostTrait};
        let needle = label.trim().to_ascii_lowercase();
        if needle.is_empty() {
            return false;
        }
        let host = cpal::default_host();
        let Some(dev) = host.default_input_device() else {
            return false;
        };
        let Ok(name) = dev.name() else {
            return false;
        };
        name.to_ascii_lowercase().contains(&needle)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = label;
        false
    }
}

/// 若默认麦不是 Mic Device，同步切过去（阻塞约 1–2s）；已是则跳过。
pub fn ensure_default_mic_device_for_ime(label: &str) {
    if default_capture_is_mic_device(label) {
        log::info!("T1 USB default capture already [{label}] — skip EnsureUsbMic");
        return;
    }
    log::info!("T1 USB default capture is not [{label}] — EnsureUsbMic now");
    match crate::audio::vb_cable::ensure_usb_mic_for_voice() {
        Ok(r) => log::info!("T1 USB EnsureUsbMic sync: {}", r.message),
        Err(e) => log::warn!("T1 USB EnsureUsbMic sync failed: {e}"),
    }
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

/// 对齐 Python `NativeAudioSessionClient.open`：只确认 Mic Device 在线并记账。
/// **禁止**打开 cpal/WASAPI 流，否则会与语音输入法抢同一捕获端点。
pub fn start_mic_keepalive(label: &str) -> Result<(), String> {
    let endpoint = ensure_mic_device(label)?;
    NATIVE_MIC_SESSION_OPEN.store(true, Ordering::SeqCst);
    log::info!(
        "T1 AUDIO OPEN verified (no capture stream; IME owns Mic Device) endpoint={endpoint}"
    );
    Ok(())
}

pub fn stop_mic_keepalive() {
    if NATIVE_MIC_SESSION_OPEN.swap(false, Ordering::SeqCst) {
        log::info!("T1 AUDIO session closed (native verify-only)");
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
