//! VB-CABLE：WASAPI/cpal 不响；MME `waveOut` 写入 CABLE Input 环回正常。
//!
//! 不用 `windows` crate 的 `WAVEHDR`：它是 `repr(C, packed(1))`，对本机编译器
//! 访问 `dwFlags` / 传引用有未定义行为风险，表现为 write 成功但环回静音。
//!
//! 播放用 **多缓冲排队**（不等第一块播完再提交下一块），欠载时补静音，避免
//! 麦信号出现「缝」——豆包流式出字对连续时钟很敏感。

#![cfg(target_os = "windows")]

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

const SRC_RATE: u32 = 48_000;
const MMSYSERR_NOERROR: u32 = 0;
const WAVE_FORMAT_PCM: u16 = 1;
const CALLBACK_NULL: u32 = 0;
const WHDR_DONE: u32 = 0x0000_0001;
const WHDR_PREPARED: u32 = 0x0000_0002;
const MAXPNAMELEN: usize = 32;
/// 同时排队的 waveOut 缓冲数。
/// 2 槽≈20ms 预缓冲，接近普通麦；会话内欠载再补静音防缝。
const QUEUE_SLOTS: usize = 2;
/// 仅「语音会话进行中」欠载时补静音；会话外绝不灌。
const UNDERRUN_PAD_MS: u64 = 120;
/// 半包等待上限（会话中有数据就尽快送，少等）。
const PARTIAL_WAIT_MS: u64 = 3;
/// 目标块时长（毫秒）。声卡本来就是按小块拉流；这不是「切成大段再识别」。
const CHUNK_MS: u32 = 10;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct WaveFormatEx {
    w_format_tag: u16,
    n_channels: u16,
    n_samples_per_sec: u32,
    n_avg_bytes_per_sec: u32,
    n_block_align: u16,
    w_bits_per_sample: u16,
    cb_size: u16,
}

/// 与 MSVC / ctypes 一致的自然对齐 WAVEHDR（x64 上 48 字节）。
#[repr(C)]
#[derive(Clone, Copy)]
struct WaveHdr {
    lp_data: *mut u8,
    dw_buffer_length: u32,
    dw_bytes_recorded: u32,
    dw_user: usize,
    dw_flags: u32,
    dw_loops: u32,
    lp_next: *mut WaveHdr,
    reserved: usize,
}

impl Default for WaveHdr {
    fn default() -> Self {
        Self {
            lp_data: std::ptr::null_mut(),
            dw_buffer_length: 0,
            dw_bytes_recorded: 0,
            dw_user: 0,
            dw_flags: 0,
            dw_loops: 0,
            lp_next: std::ptr::null_mut(),
            reserved: 0,
        }
    }
}

#[repr(C)]
struct WaveOutCapsW {
    w_mid: u16,
    w_pid: u16,
    v_driver_version: u32,
    sz_pname: [u16; MAXPNAMELEN],
    dw_formats: u32,
    w_channels: u16,
    w_reserved1: u16,
    dw_support: u32,
}

#[link(name = "winmm")]
extern "system" {
    fn waveOutGetNumDevs() -> u32;
    fn waveOutGetDevCapsW(u_device_id: usize, pwoc: *mut WaveOutCapsW, cbwoc: u32) -> u32;
    fn waveOutOpen(
        phwo: *mut usize,
        u_device_id: u32,
        pwfx: *const WaveFormatEx,
        dw_callback: usize,
        dw_instance: usize,
        fdw_open: u32,
    ) -> u32;
    fn waveOutClose(hwo: usize) -> u32;
    fn waveOutPrepareHeader(hwo: usize, pwh: *mut WaveHdr, cbwh: u32) -> u32;
    fn waveOutUnprepareHeader(hwo: usize, pwh: *mut WaveHdr, cbwh: u32) -> u32;
    fn waveOutWrite(hwo: usize, pwh: *mut WaveHdr, cbwh: u32) -> u32;
    fn waveOutReset(hwo: usize) -> u32;
    fn waveOutSetVolume(hwo: usize, dw_volume: u32) -> u32;
}

struct WaveOutCable {
    handle: usize,
    channels: u16,
    rate: u32,
}

impl WaveOutCable {
    fn open() -> Result<Self, String> {
        unsafe {
            let n = waveOutGetNumDevs();
            let mut chosen: Option<(u32, String)> = None;
            for i in 0..n {
                let mut caps = std::mem::zeroed::<WaveOutCapsW>();
                if waveOutGetDevCapsW(
                    i as usize,
                    &mut caps,
                    std::mem::size_of::<WaveOutCapsW>() as u32,
                ) != MMSYSERR_NOERROR
                {
                    continue;
                }
                let name = String::from_utf16_lossy(
                    &caps
                        .sz_pname
                        .iter()
                        .copied()
                        .take_while(|&c| c != 0)
                        .collect::<Vec<_>>(),
                );
                let lower = name.to_ascii_lowercase();
                if lower.contains("cable input") && !lower.contains("16ch") {
                    chosen = Some((i, name));
                    break;
                }
            }
            let Some((dev_id, name)) = chosen else {
                return Err("waveOut: 未找到 CABLE Input".into());
            };

            // 源 PCM 为 48k：优先 48k 避免重采样漂移；16ch 返回 mmr=11。
            let attempts: [(u16, u32); 2] = [(2, 48_000), (2, 44_100)];
            let mut last_err = String::from("unknown");
            for &(channels, rate) in &attempts {
                let mut handle: usize = 0;
                let fmt = WaveFormatEx {
                    w_format_tag: WAVE_FORMAT_PCM,
                    n_channels: channels,
                    n_samples_per_sec: rate,
                    n_avg_bytes_per_sec: rate * channels as u32 * 2,
                    n_block_align: channels * 2,
                    w_bits_per_sample: 16,
                    cb_size: 0,
                };
                let rc = waveOutOpen(
                    &mut handle,
                    dev_id,
                    &fmt,
                    0,
                    0,
                    CALLBACK_NULL,
                );
                if rc == MMSYSERR_NOERROR {
                    let _ = waveOutSetVolume(handle, 0xFFFF_FFFF);
                    match crate::audio::session_volume::unmute_current_process_sessions() {
                        Ok(s) => log::info!("AUDIO ROUTER session volume: {s}"),
                        Err(e) => log::warn!("AUDIO ROUTER session unmute failed: {e}"),
                    }
                    log::info!(
                        "AUDIO ROUTER WINMM device={name} id={dev_id} rate={rate} ch={channels} hdr_size={} slots={QUEUE_SLOTS}",
                        std::mem::size_of::<WaveHdr>()
                    );
                    eprintln!(
                        "AUDIO ROUTER WINMM device={name} id={dev_id} rate={rate} ch={channels}"
                    );
                    return Ok(Self {
                        handle,
                        channels,
                        rate,
                    });
                }
                last_err = format!("mmr={rc}");
            }
            Err(format!("waveOutOpen ({name}): {last_err}"))
        }
    }
}

impl Drop for WaveOutCable {
    fn drop(&mut self) {
        unsafe {
            let _ = waveOutReset(self.handle);
            let _ = waveOutClose(self.handle);
        }
    }
}

struct QueueSlot {
    bytes: Vec<u8>,
    hdr: WaveHdr,
    in_flight: bool,
}

impl QueueSlot {
    fn new() -> Self {
        Self {
            bytes: Vec::new(),
            hdr: WaveHdr::default(),
            in_flight: false,
        }
    }

    fn is_free(&self) -> bool {
        if !self.in_flight {
            return true;
        }
        unsafe {
            let flags = std::ptr::addr_of!(self.hdr.dw_flags).read_volatile();
            flags & WHDR_DONE != 0
        }
    }

    fn reclaim(&mut self, handle: usize) {
        if !self.in_flight {
            return;
        }
        let hdr_size = std::mem::size_of::<WaveHdr>() as u32;
        unsafe {
            let flags = std::ptr::addr_of!(self.hdr.dw_flags).read_volatile();
            if flags & WHDR_PREPARED != 0 || flags & WHDR_DONE != 0 {
                let _ = waveOutUnprepareHeader(handle, &mut self.hdr, hdr_size);
            }
        }
        self.in_flight = false;
        self.hdr = WaveHdr::default();
        self.bytes.clear();
    }
}

fn submit_slot(
    handle: usize,
    slot: &mut QueueSlot,
    interleaved: &[i16],
) -> Result<(), String> {
    slot.reclaim(handle);
    slot.bytes.clear();
    slot.bytes.reserve(interleaved.len() * 2);
    for s in interleaved {
        slot.bytes.extend_from_slice(&s.to_le_bytes());
    }
    let hdr_size = std::mem::size_of::<WaveHdr>() as u32;
    unsafe {
        slot.hdr = WaveHdr::default();
        slot.hdr.lp_data = slot.bytes.as_mut_ptr();
        slot.hdr.dw_buffer_length = slot.bytes.len() as u32;
        let rc = waveOutPrepareHeader(handle, &mut slot.hdr, hdr_size);
        if rc != MMSYSERR_NOERROR {
            return Err(format!("waveOutPrepareHeader mmr={rc}"));
        }
        let rc = waveOutWrite(handle, &mut slot.hdr, hdr_size);
        if rc != MMSYSERR_NOERROR {
            let _ = waveOutUnprepareHeader(handle, &mut slot.hdr, hdr_size);
            slot.hdr = WaveHdr::default();
            return Err(format!("waveOutWrite mmr={rc}"));
        }
        slot.in_flight = true;
    }
    Ok(())
}

fn resample_and_expand(samples_48k: &[i16], out_rate: u32, out_ch: u16) -> Vec<i16> {
    let out_len = if out_rate == SRC_RATE {
        samples_48k.len()
    } else {
        ((samples_48k.len() as u64 * out_rate as u64) / SRC_RATE as u64).max(1) as usize
    };
    let mut mono = Vec::with_capacity(out_len);
    if out_rate == SRC_RATE {
        mono.extend_from_slice(samples_48k);
    } else {
        for i in 0..out_len {
            let src = i as f64 * (SRC_RATE as f64) / (out_rate as f64);
            let i0 = src.floor() as usize;
            let i1 = (i0 + 1).min(samples_48k.len().saturating_sub(1));
            let frac = src - i0 as f64;
            let a = samples_48k.get(i0).copied().unwrap_or(0) as f64;
            let b = samples_48k.get(i1).copied().unwrap_or(0) as f64;
            mono.push((a + (b - a) * frac).round() as i16);
        }
    }
    let ch = out_ch.max(1) as usize;
    let mut out = vec![0i16; mono.len() * ch];
    for (i, &s) in mono.iter().enumerate() {
        for c in 0..ch {
            out[i * ch + c] = s;
        }
    }
    out
}

pub fn spawn_winmm_writer(
    buffer: Arc<Mutex<VecDeque<i16>>>,
    running: Arc<AtomicBool>,
    // CLEAR/PCM → true，END → false：仅语音会话期间往 CABLE 送声。
    session_live: Arc<AtomicBool>,
) -> Result<(), String> {
    debug_assert_eq!(std::mem::size_of::<WaveHdr>(), 48);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("winmm-cable-out".into())
        .spawn(move || match WaveOutCable::open() {
            Ok(out) => {
                let _ = tx.send(Ok(()));
                let mut unmuted_after_write = false;
                let chunk_mono = ((SRC_RATE * CHUNK_MS) as usize / 1000).max(240);
                let min_submit = (chunk_mono / 2).max(120);
                let mut slots: Vec<QueueSlot> =
                    (0..QUEUE_SLOTS).map(|_| QueueSlot::new()).collect();
                let mut writes: u64 = 0;
                let mut last_pcm_at: Option<Instant> = None;
                let mut underrun_pads: u64 = 0;
                let mut was_live = false;

                while running.load(Ordering::Acquire) {
                    let live = session_live.load(Ordering::Acquire);

                    // 语音结束：立刻停表、丢掉已排队块，等同普通麦松手。
                    if was_live && !live {
                        unsafe {
                            let _ = waveOutReset(out.handle);
                        }
                        for slot in &mut slots {
                            slot.reclaim(out.handle);
                        }
                        last_pcm_at = None;
                        log::info!("AUDIO ROUTER WINMM session end — reset output");
                    }
                    was_live = live;

                    for slot in &mut slots {
                        if slot.in_flight && slot.is_free() {
                            slot.reclaim(out.handle);
                        }
                    }

                    if !live {
                        // 会话外：清空残留，不写、不补静音。
                        {
                            let mut g = buffer.lock();
                            if !g.is_empty() {
                                g.clear();
                            }
                        }
                        std::thread::sleep(Duration::from_millis(5));
                        continue;
                    }

                    let free_idx = slots.iter().position(|s| s.is_free());
                    let Some(idx) = free_idx else {
                        std::thread::sleep(Duration::from_millis(1));
                        continue;
                    };

                    let mono_48k = {
                        let queued = buffer.lock().len();
                        if queued >= chunk_mono {
                            let mut g = buffer.lock();
                            last_pcm_at = Some(Instant::now());
                            g.drain(..chunk_mono).collect::<Vec<_>>()
                        } else if queued >= min_submit {
                            // 会话中够半块就送，降低首字延迟。
                            let mut g = buffer.lock();
                            last_pcm_at = Some(Instant::now());
                            let n = g.len().min(chunk_mono);
                            g.drain(..n).collect::<Vec<_>>()
                        } else if queued > 0 {
                            let wait_deadline =
                                Instant::now() + Duration::from_millis(PARTIAL_WAIT_MS);
                            while buffer.lock().len() < min_submit
                                && Instant::now() < wait_deadline
                                && running.load(Ordering::Acquire)
                                && session_live.load(Ordering::Acquire)
                            {
                                std::thread::sleep(Duration::from_millis(1));
                            }
                            let mut g = buffer.lock();
                            if g.is_empty() {
                                Vec::new()
                            } else {
                                last_pcm_at = Some(Instant::now());
                                let n = g.len().min(chunk_mono);
                                g.drain(..n).collect::<Vec<_>>()
                            }
                        } else {
                            Vec::new()
                        }
                    };

                    let mono_48k = if !mono_48k.is_empty() {
                        mono_48k
                    } else if last_pcm_at
                        .map(|t| t.elapsed() < Duration::from_millis(UNDERRUN_PAD_MS))
                        .unwrap_or(false)
                    {
                        underrun_pads += 1;
                        if underrun_pads == 1 || underrun_pads % 50 == 0 {
                            log::debug!("AUDIO ROUTER WINMM underrun pad x{underrun_pads}");
                        }
                        vec![0i16; chunk_mono]
                    } else {
                        std::thread::sleep(Duration::from_millis(2));
                        continue;
                    };

                    let interleaved = resample_and_expand(&mono_48k, out.rate, out.channels);
                    match submit_slot(out.handle, &mut slots[idx], &interleaved) {
                        Ok(()) => {
                            writes += 1;
                            if !unmuted_after_write {
                                unmuted_after_write = true;
                                match crate::audio::session_volume::unmute_current_process_sessions()
                                {
                                    Ok(s) => {
                                        log::info!("AUDIO ROUTER session volume(after write): {s}")
                                    }
                                    Err(e) => {
                                        log::warn!("AUDIO ROUTER session unmute failed: {e}")
                                    }
                                }
                            }
                            if writes == 1 || writes % 100 == 0 {
                                log::info!(
                                    "AUDIO ROUTER WINMM writes={writes} samples={} queued={}",
                                    mono_48k.len(),
                                    slots.iter().filter(|s| s.in_flight).count()
                                );
                            }
                        }
                        Err(e) => {
                            log::warn!("WINMM write: {e}");
                            std::thread::sleep(Duration::from_millis(10));
                        }
                    }
                }

                unsafe {
                    let _ = waveOutReset(out.handle);
                }
                for slot in &mut slots {
                    slot.reclaim(out.handle);
                }
            }
            Err(e) => {
                let _ = tx.send(Err(e));
            }
        })
        .map_err(|e| e.to_string())?;
    rx.recv_timeout(Duration::from_secs(5))
        .map_err(|_| "WINMM open timeout".to_string())?
}
