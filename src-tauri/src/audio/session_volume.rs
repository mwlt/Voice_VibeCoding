//! 解除本进程在音量混合器中的会话静音（remote-bridge-hub.exe 曾被系统记住为静音）。

#![cfg(target_os = "windows")]

use windows::core::Interface;
use windows::Win32::Foundation::BOOL;
use windows::Win32::Media::Audio::{
    eRender, IAudioSessionControl2, IAudioSessionManager2, ISimpleAudioVolume,
    IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
};

/// 对本进程在所有活动渲染设备上的音频会话：取消静音并拉满音量。
pub fn unmute_current_process_sessions() -> Result<String, String> {
    unsafe {
        let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
        let should_uninit = hr.is_ok();
        let result = unmute_inner();
        if should_uninit {
            CoUninitialize();
        }
        result
    }
}

unsafe fn unmute_inner() -> Result<String, String> {
    let pid = std::process::id();
    let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
        .map_err(|e| format!("MMDeviceEnumerator: {e}"))?;
    let collection = enumerator
        .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
        .map_err(|e| format!("EnumAudioEndpoints: {e}"))?;
    let n = collection.GetCount().map_err(|e| format!("GetCount: {e}"))?;
    let mut fixed = 0u32;
    let mut seen = 0u32;
    for i in 0..n {
        let device = match collection.Item(i) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let mgr: IAudioSessionManager2 = match device.Activate(CLSCTX_ALL, None) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let sessions = match mgr.GetSessionEnumerator() {
            Ok(s) => s,
            Err(_) => continue,
        };
        let count = sessions.GetCount().unwrap_or(0);
        for s in 0..count {
            let ctl = match sessions.GetSession(s) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let Ok(ctl2) = ctl.cast::<IAudioSessionControl2>() else {
                continue;
            };
            let Ok(sess_pid) = ctl2.GetProcessId() else {
                continue;
            };
            if sess_pid != pid {
                continue;
            }
            seen += 1;
            let Ok(vol) = ctl.cast::<ISimpleAudioVolume>() else {
                continue;
            };
            let muted = vol.GetMute().unwrap_or(BOOL(0)).as_bool();
            let level = vol.GetMasterVolume().unwrap_or(0.0);
            let _ = vol.SetMute(false, std::ptr::null());
            let _ = vol.SetMasterVolume(1.0, std::ptr::null());
            let muted2 = vol.GetMute().unwrap_or(BOOL(1)).as_bool();
            let level2 = vol.GetMasterVolume().unwrap_or(0.0);
            log::info!(
                "AUDIO SESSION unmute pid={pid} was_mute={muted} was_vol={level:.2} -> mute={muted2} vol={level2:.2}"
            );
            fixed += 1;
        }
    }
    Ok(format!("sessions_seen={seen} unmuted={fixed}"))
}
