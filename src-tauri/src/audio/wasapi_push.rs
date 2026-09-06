//! VB-CABLE：cpal 回调 WASAPI 本机不响；推模式 GetBuffer 写入环回正常。

#![cfg(target_os = "windows")]

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use windows::core::GUID;
use windows::Win32::Media::Audio::{
    eConsole, eRender, IAudioClient, IAudioRenderClient, IMMDevice, IMMDeviceEnumerator,
    MMDeviceEnumerator, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
    AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, DEVICE_STATE_ACTIVE, WAVEFORMATEX,
    WAVEFORMATEXTENSIBLE,
};
use windows::Win32::Media::KernelStreaming::WAVE_FORMAT_EXTENSIBLE;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
    COINIT_MULTITHREADED,
};

const RATE: u32 = 48_000;
const CHANNELS: u16 = 2;
const BITS: u16 = 32;
const KSDATAFORMAT_SUBTYPE_IEEE_FLOAT: GUID =
    GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);

pub struct WasapiPushOutput {
    client: IAudioClient,
    render: IAudioRenderClient,
    buffer_frames: u32,
    should_uninit: bool,
}

impl WasapiPushOutput {
    pub fn open_cable_input() -> Result<Self, String> {
        unsafe {
            let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
            let should_uninit = hr.is_ok();

            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .map_err(|e| format!("MMDeviceEnumerator: {e}"))?;
            let (device, name) = find_cable_render(&enumerator)?;
            let client: IAudioClient = device
                .Activate::<IAudioClient>(CLSCTX_ALL, None)
                .map_err(|e| format!("Activate IAudioClient: {e}"))?;

            let mut format = WAVEFORMATEXTENSIBLE {
                Format: WAVEFORMATEX {
                    wFormatTag: WAVE_FORMAT_EXTENSIBLE as u16,
                    nChannels: CHANNELS,
                    nSamplesPerSec: RATE,
                    nAvgBytesPerSec: RATE * CHANNELS as u32 * (BITS as u32 / 8),
                    nBlockAlign: CHANNELS * (BITS / 8),
                    wBitsPerSample: BITS,
                    cbSize: 22,
                },
                Samples: Default::default(),
                dwChannelMask: 0x3,
                SubFormat: KSDATAFORMAT_SUBTYPE_IEEE_FLOAT,
            };
            format.Samples.wValidBitsPerSample = BITS;

            let hns = 200_000i64;
            client
                .Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
                    hns,
                    0,
                    &format.Format,
                    None,
                )
                .map_err(|e| format!("IAudioClient::Initialize ({name}): {e}"))?;

            let buffer_frames = client
                .GetBufferSize()
                .map_err(|e| format!("GetBufferSize: {e}"))?;
            let render: IAudioRenderClient = client
                .GetService()
                .map_err(|e| format!("GetService IAudioRenderClient: {e}"))?;
            client
                .Start()
                .map_err(|e| format!("IAudioClient::Start: {e}"))?;

            log::info!(
                "AUDIO ROUTER WASAPI PUSH device={name} rate={RATE} ch={CHANNELS} buffer_frames={buffer_frames}"
            );
            eprintln!(
                "AUDIO ROUTER WASAPI PUSH device={name} rate={RATE} ch={CHANNELS} buffer_frames={buffer_frames}"
            );

            Ok(Self {
                client,
                render,
                buffer_frames,
                should_uninit,
            })
        }
    }

    pub fn write_mono_i16(&self, samples: &[i16]) -> Result<(), String> {
        if samples.is_empty() {
            return Ok(());
        }
        unsafe {
            let mut offset = 0usize;
            let mut spins = 0u32;
            while offset < samples.len() {
                let padding = self
                    .client
                    .GetCurrentPadding()
                    .map_err(|e| format!("GetCurrentPadding: {e}"))?;
                let avail = self.buffer_frames.saturating_sub(padding);
                if avail == 0 {
                    spins += 1;
                    if spins > 500 {
                        return Err("WASAPI buffer full timeout".into());
                    }
                    std::thread::sleep(Duration::from_millis(2));
                    continue;
                }
                spins = 0;
                let n = (samples.len() - offset).min(avail as usize);
                let ptr = self
                    .render
                    .GetBuffer(n as u32)
                    .map_err(|e| format!("GetBuffer: {e}"))?;
                let out = std::slice::from_raw_parts_mut(ptr as *mut f32, n * CHANNELS as usize);
                for i in 0..n {
                    let f = samples[offset + i] as f32 / 32768.0;
                    out[i * 2] = f;
                    out[i * 2 + 1] = f;
                }
                self.render
                    .ReleaseBuffer(n as u32, 0)
                    .map_err(|e| format!("ReleaseBuffer: {e}"))?;
                offset += n;
            }
        }
        Ok(())
    }
}

impl Drop for WasapiPushOutput {
    fn drop(&mut self) {
        unsafe {
            let _ = self.client.Stop();
            if self.should_uninit {
                CoUninitialize();
            }
        }
    }
}

fn find_cable_render(enumerator: &IMMDeviceEnumerator) -> Result<(IMMDevice, String), String> {
    unsafe {
        if let Ok(dev) = enumerator.GetDefaultAudioEndpoint(eRender, eConsole) {
            if let Some(name) = endpoint_label(&dev) {
                if name_is_cable_input(&name) {
                    return Ok((dev, name));
                }
            }
        }
        let collection = enumerator
            .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
            .map_err(|e| format!("EnumAudioEndpoints: {e}"))?;
        let count = collection.GetCount().unwrap_or(0);
        for i in 0..count {
            let Ok(dev) = collection.Item(i) else { continue };
            let Some(name) = endpoint_label(&dev) else { continue };
            if name_is_cable_input(&name) {
                return Ok((dev, name));
            }
        }
        Err("未找到 CABLE Input (WASAPI)".into())
    }
}

fn name_is_cable_input(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("cable input") && !n.contains("16ch")
}

/// 用设备 ID 走 MMDevices 注册表读友好名（与 vb_cable / configure 脚本一致）。
fn endpoint_label(dev: &IMMDevice) -> Option<String> {
    unsafe {
        let id_ptr = dev.GetId().ok()?;
        if id_ptr.is_null() {
            return None;
        }
        let id = id_ptr.to_string().ok()?;
        CoTaskMemFree(Some(id_ptr.as_ptr() as *const _));
        // id: {0.0.0.00000000}.{guid}
        let guid = id.rsplit('.').next()?.trim_matches(|c| c == '{' || c == '}');
        let path = format!(
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render\{{{guid}}}"
        );
        use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ};
        use winreg::RegKey;
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        let endpoint = hklm.open_subkey_with_flags(&path, KEY_READ).ok()?;
        let props = endpoint.open_subkey_with_flags("Properties", KEY_READ).ok()?;
        const PKEY_DEVICE: &str = "{a45c254e-df1c-4efd-8020-67d146a850e0},2";
        const PKEY_ENDPOINT: &str = "{b3f8fa53-0004-438e-9003-51a46e139bfc},6";
        let mut parts = Vec::new();
        for key in [PKEY_DEVICE, PKEY_ENDPOINT] {
            if let Ok(v) = props.get_value::<String, _>(key) {
                let t = v.trim();
                if !t.is_empty() && !parts.iter().any(|p: &String| p.eq_ignore_ascii_case(t)) {
                    parts.push(t.to_string());
                }
            }
        }
        let label = parts.join(" ");
        if label.is_empty() {
            None
        } else {
            Some(label)
        }
    }
}

pub fn spawn_wasapi_writer(
    buffer: Arc<Mutex<VecDeque<i16>>>,
    running: Arc<AtomicBool>,
) -> Result<(), String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("wasapi-cable-push".into())
        .spawn(move || {
            match WasapiPushOutput::open_cable_input() {
                Ok(out) => {
                    let _ = tx.send(Ok(()));
                    while running.load(Ordering::Acquire) {
                        let chunk: Vec<i16> = {
                            let mut g = buffer.lock();
                            if g.is_empty() {
                                Vec::new()
                            } else {
                                let n = g.len().min(480 * 4);
                                g.drain(..n).collect()
                            }
                        };
                        if chunk.is_empty() {
                            std::thread::sleep(Duration::from_millis(5));
                            continue;
                        }
                        if let Err(e) = out.write_mono_i16(&chunk) {
                            log::warn!("WASAPI push write: {e}");
                            std::thread::sleep(Duration::from_millis(10));
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(e));
                }
            }
        })
        .map_err(|e| e.to_string())?;
    rx.recv_timeout(Duration::from_secs(5))
        .map_err(|_| "WASAPI push open timeout".to_string())?
}