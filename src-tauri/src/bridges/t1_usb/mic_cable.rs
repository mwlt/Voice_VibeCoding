//! T1 USB：Mic Device → audio_router(UDP 48k) → CABLE Input
//!
//! 与小米 / T1 BLE 一样，输入法听 **CABLE Output**。
//! 推流必须按实时节拍发送：router 缓冲只有 ~60ms，打爆会被丢光 → 输入法无声。

use parking_lot::Mutex;
use std::collections::VecDeque;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::audio::pcm_router::DEFAULT_PCM_PORT;

const MIC_LABEL: &str = "Mic Device";
const TARGET_HZ: u32 = 48_000;
const PACKET_SAMPLES: usize = 960; // ~20ms @48k
const TOGGLE_MAX_HOLD: Duration = Duration::from_secs(90);

static STOP: AtomicBool = AtomicBool::new(true);
static RUNNING: AtomicBool = AtomicBool::new(false);
static FIRST_PACKET: AtomicBool = AtomicBool::new(false);
static THREAD: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);
static TOGGLE_DEADLINE: Mutex<Option<Instant>> = Mutex::new(None);
static LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);

fn pcm_port() -> u16 {
    std::env::var("REMOTE_BRIDGE_PCM_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_PCM_PORT)
}

fn peer_addr() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], pcm_port()))
}

pub fn is_running() -> bool {
    RUNNING.load(Ordering::SeqCst)
}

/// Hold / Toggle：开 Mic→CABLE，并把默认麦设为 CABLE Output。
/// 1.5s 内无 UDP 包则 Err，调用方应回退 EnsureUsbMic。
pub fn start_for_voice(toggle_mode: bool) -> Result<(), String> {
    let _ = crate::audio::pcm_router::ensure_audio_router_process();
    crate::audio::vb_cable::ensure_cable_mic_for_ime_before_voice();

    if toggle_mode {
        *TOGGLE_DEADLINE.lock() = Some(Instant::now() + TOGGLE_MAX_HOLD);
    } else {
        *TOGGLE_DEADLINE.lock() = None;
    }

    if RUNNING.load(Ordering::SeqCst) && FIRST_PACKET.load(Ordering::SeqCst) {
        log::info!("T1 USB mic→CABLE already running (extend session)");
        return Ok(());
    }

    stop_join_only();
    *LAST_ERROR.lock() = None;
    FIRST_PACKET.store(false, Ordering::SeqCst);
    STOP.store(false, Ordering::SeqCst);
    RUNNING.store(true, Ordering::SeqCst);

    let handle = thread::Builder::new()
        .name("t1-usb-mic-cable".into())
        .spawn(run_loopback_thread)
        .map_err(|e| format!("spawn mic→CABLE: {e}"))?;
    *THREAD.lock() = Some(handle);

    let wait_deadline = Instant::now() + Duration::from_millis(1500);
    while Instant::now() < wait_deadline {
        if FIRST_PACKET.load(Ordering::SeqCst) {
            return Ok(());
        }
        if !RUNNING.load(Ordering::SeqCst) {
            return Err(LAST_ERROR
                .lock()
                .clone()
                .unwrap_or_else(|| "mic→CABLE thread exited early".into()));
        }
        thread::sleep(Duration::from_millis(20));
    }
    if FIRST_PACKET.load(Ordering::SeqCst) {
        return Ok(());
    }
    stop();
    Err("mic→CABLE no UDP packets within 1.5s".into())
}

pub fn stop() {
    *TOGGLE_DEADLINE.lock() = None;
    if !RUNNING.load(Ordering::SeqCst) && THREAD.lock().is_none() {
        FIRST_PACKET.store(false, Ordering::SeqCst);
        return;
    }
    STOP.store(true, Ordering::SeqCst);
    stop_join_only();
    RUNNING.store(false, Ordering::SeqCst);
    FIRST_PACKET.store(false, Ordering::SeqCst);
    log::info!("T1 USB mic→CABLE stopped");
}

fn stop_join_only() {
    STOP.store(true, Ordering::SeqCst);
    if let Some(h) = THREAD.lock().take() {
        let _ = h.join();
    }
    RUNNING.store(false, Ordering::SeqCst);
}

fn fail(msg: impl Into<String>) {
    let msg = msg.into();
    log::warn!("T1 USB mic→CABLE: {msg}");
    *LAST_ERROR.lock() = Some(msg);
    RUNNING.store(false, Ordering::SeqCst);
}

fn run_loopback_thread() {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    FIRST_PACKET.store(false, Ordering::SeqCst);
    let peer = peer_addr();
    let sock = match UdpSocket::bind("127.0.0.1:0") {
        Ok(s) => s,
        Err(e) => {
            fail(format!("UDP bind failed: {e}"));
            return;
        }
    };
    let _ = sock.set_read_timeout(Some(Duration::from_millis(80)));

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut ready = false;
    while Instant::now() < deadline && !STOP.load(Ordering::SeqCst) {
        let _ = sock.send_to(b"PING", peer);
        let mut buf = [0u8; 32];
        if let Ok((n, _)) = sock.recv_from(&mut buf) {
            if &buf[..n] == b"PONG" {
                ready = true;
                break;
            }
        }
        thread::sleep(Duration::from_millis(20));
    }
    if !ready {
        fail(format!("audio_router not ready at {peer}"));
        return;
    }
    let _ = sock.send_to(b"CLEAR", peer);

    let host = cpal::default_host();
    let needle = MIC_LABEL.to_ascii_lowercase();
    let device = match host.input_devices() {
        Ok(mut devs) => devs.find(|d| {
            d.name()
                .ok()
                .map(|n| n.to_ascii_lowercase().contains(&needle))
                .unwrap_or(false)
        }),
        Err(e) => {
            fail(format!("enum failed: {e}"));
            return;
        }
    };
    let Some(device) = device else {
        fail(format!("no capture device containing [{MIC_LABEL}]"));
        return;
    };
    let name = device.name().unwrap_or_else(|_| MIC_LABEL.into());

    let config = match pick_input_config(&device) {
        Ok(c) => c,
        Err(e) => {
            fail(e);
            return;
        }
    };
    let in_hz = config.sample_rate().0.max(1);
    let channels = config.channels().max(1) as usize;
    let sample_format = config.sample_format();
    log::info!(
        "T1 USB mic→CABLE open device={name} hz={in_hz} ch={channels} fmt={sample_format:?}"
    );

    let ring: Arc<Mutex<VecDeque<i16>>> =
        Arc::new(Mutex::new(VecDeque::with_capacity(in_hz as usize * 2)));
    let stop_flag = Arc::new(AtomicBool::new(false));
    let err_fn = |e| log::warn!("T1 USB mic→CABLE stream error: {e}");

    let stream = match sample_format {
        cpal::SampleFormat::I16 => {
            let ring_cb = Arc::clone(&ring);
            let stop_cb = Arc::clone(&stop_flag);
            let ch = channels;
            device.build_input_stream(
                &config.config(),
                move |data: &[i16], _| {
                    if stop_cb.load(Ordering::Relaxed) {
                        return;
                    }
                    let mut g = ring_cb.lock();
                    if ch <= 1 {
                        g.extend(data.iter().copied());
                    } else {
                        for frame in data.chunks(ch) {
                            let sum: i32 = frame.iter().map(|&s| s as i32).sum();
                            g.push_back((sum / ch as i32) as i16);
                        }
                    }
                    let cap = in_hz as usize * 2;
                    while g.len() > cap {
                        g.pop_front();
                    }
                },
                err_fn,
                None,
            )
        }
        cpal::SampleFormat::F32 => {
            let ring_cb = Arc::clone(&ring);
            let stop_cb = Arc::clone(&stop_flag);
            let ch = channels;
            device.build_input_stream(
                &config.config(),
                move |data: &[f32], _| {
                    if stop_cb.load(Ordering::Relaxed) {
                        return;
                    }
                    let mut g = ring_cb.lock();
                    for frame in data.chunks(ch.max(1)) {
                        let s = if frame.len() == 1 {
                            frame[0]
                        } else {
                            frame.iter().sum::<f32>() / frame.len() as f32
                        };
                        g.push_back((s.clamp(-1.0, 1.0) * 32767.0) as i16);
                    }
                    let cap = in_hz as usize * 2;
                    while g.len() > cap {
                        g.pop_front();
                    }
                },
                err_fn,
                None,
            )
        }
        other => {
            fail(format!("unsupported format: {other:?}"));
            return;
        }
    };

    let stream = match stream {
        Ok(s) => s,
        Err(e) => {
            fail(format!("build failed: {e}"));
            return;
        }
    };
    if let Err(e) = stream.play() {
        fail(format!("play failed: {e}"));
        return;
    }

    let mut in_phase = 0.0f64;
    let phase_step = in_hz as f64 / TARGET_HZ as f64;
    let mut packets = 0u64;
    let mut next_tick = Instant::now();
    let tick = Duration::from_secs_f64(PACKET_SAMPLES as f64 / TARGET_HZ as f64);

    while !STOP.load(Ordering::SeqCst) {
        if let Some(until) = *TOGGLE_DEADLINE.lock() {
            if Instant::now() >= until {
                log::info!("T1 USB mic→CABLE toggle max hold reached");
                break;
            }
        }

        let mut out = vec![0i16; PACKET_SAMPLES];
        {
            let mut g = ring.lock();
            for s in out.iter_mut() {
                if g.is_empty() {
                    *s = 0;
                    continue;
                }
                let idx = (in_phase.floor() as usize).min(g.len() - 1);
                *s = g[idx];
                in_phase += phase_step;
                while in_phase >= 1.0 && !g.is_empty() {
                    g.pop_front();
                    in_phase -= 1.0;
                }
            }
        }

        let mut bytes = Vec::with_capacity(PACKET_SAMPLES * 2);
        for s in &out {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        if sock.send_to(&bytes, peer).is_ok() {
            packets = packets.wrapping_add(1);
            FIRST_PACKET.store(true, Ordering::SeqCst);
            if packets == 1 || packets % 50 == 0 {
                let sum: f64 = out.iter().map(|&s| (s as f64) * (s as f64)).sum();
                let rms = (sum / out.len() as f64).sqrt() / 32768.0;
                log::info!("T1 USB mic→CABLE packets={packets} rms={rms:.4}");
            }
        }

        next_tick += tick;
        let now = Instant::now();
        if next_tick > now {
            thread::sleep(next_tick - now);
        } else {
            next_tick = now;
        }
    }

    stop_flag.store(true, Ordering::SeqCst);
    drop(stream);
    let _ = sock.send_to(b"END", peer);
    RUNNING.store(false, Ordering::SeqCst);
    log::info!("T1 USB mic→CABLE thread exit packets={packets}");
}

fn pick_input_config(device: &cpal::Device) -> Result<cpal::SupportedStreamConfig, String> {
    use cpal::traits::DeviceTrait;
    if let Ok(configs) = device.supported_input_configs() {
        let mut f32_48k = None;
        for range in configs {
            if range.min_sample_rate().0 <= TARGET_HZ && range.max_sample_rate().0 >= TARGET_HZ {
                let c = range.with_sample_rate(cpal::SampleRate(TARGET_HZ));
                match c.sample_format() {
                    cpal::SampleFormat::I16 => return Ok(c),
                    cpal::SampleFormat::F32 => f32_48k = Some(c),
                    _ => {}
                }
            }
        }
        if let Some(c) = f32_48k {
            return Ok(c);
        }
    }
    device
        .default_input_config()
        .map_err(|e| format!("default input config: {e}"))
}
