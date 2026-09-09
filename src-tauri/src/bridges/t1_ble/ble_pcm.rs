//! T1 BLE PCM → 本机 audio_router（UDP），自包含，不依赖 xiaomi::voice_pcm
//!
//! 豆包请选 **CABLE Output**（BLE ATVV 与 USB Mic→CABLE 环回均走虚拟声卡）。

use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::audio::pcm_router::DEFAULT_PCM_PORT;

struct Client {
    sock: UdpSocket,
    peer: SocketAddr,
    prev: i16,
    have_prev: bool,
    sent: AtomicU64,
    dropped: AtomicU64,
}

static CLIENT: Mutex<Option<Client>> = Mutex::new(None);
static READY: AtomicBool = AtomicBool::new(false);

fn pcm_port() -> u16 {
    std::env::var("REMOTE_BRIDGE_PCM_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_PCM_PORT)
}

fn peer_addr() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], pcm_port()))
}

pub fn ensure_started() -> Result<(), String> {
    if READY.load(Ordering::Acquire) {
        return Ok(());
    }
    {
        let g = CLIENT.lock();
        if g.is_some() {
            READY.store(true, Ordering::Release);
            return Ok(());
        }
    }
    let peer = peer_addr();
    let sock = UdpSocket::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    sock.set_read_timeout(Some(Duration::from_millis(150)))
        .map_err(|e| e.to_string())?;
    // 路由已常驻时 PONG 通常 <50ms；2s 足够等首次 spawn，避免假「冷启动 4～5s」
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut ok = false;
    while Instant::now() < deadline {
        let _ = sock.send_to(b"PING", peer);
        let mut buf = [0u8; 64];
        if let Ok((n, _)) = sock.recv_from(&mut buf) {
            if &buf[..n] == b"PONG" {
                ok = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(15));
    }
    if !ok {
        return Err(format!("T1 BLE audio router not ready at {peer}"));
    }
    *CLIENT.lock() = Some(Client {
        sock,
        peer,
        prev: 0,
        have_prev: false,
        sent: AtomicU64::new(0),
        dropped: AtomicU64::new(0),
    });
    READY.store(true, Ordering::Release);
    log::info!("T1 BLE PCM UDP ready peer={peer}");
    Ok(())
}

pub fn warmup_async() {
    if READY.load(Ordering::Acquire) {
        return;
    }
    std::thread::Builder::new()
        .name("t1-ble-pcm-warmup".into())
        .spawn(|| {
            for attempt in 1..=8 {
                match ensure_started() {
                    Ok(()) => {
                        log::info!("T1 BLE PCM warmup ok attempt={attempt}");
                        return;
                    }
                    Err(e) => {
                        log::debug!("T1 BLE PCM warmup attempt={attempt}: {e}");
                        std::thread::sleep(Duration::from_millis(250));
                    }
                }
            }
            log::warn!("T1 BLE PCM warmup gave up");
        })
        .ok();
}

pub fn clear() {
    if let Some(c) = CLIENT.lock().as_ref() {
        let _ = c.sock.send_to(b"CLEAR", c.peer);
    }
    if let Some(c) = CLIENT.lock().as_mut() {
        c.have_prev = false;
    }
}

pub fn end_session() {
    if let Some(c) = CLIENT.lock().as_ref() {
        let _ = c.sock.send_to(b"END", c.peer);
    }
}

pub fn push_16k(samples: &[i16]) {
    if samples.is_empty() {
        return;
    }
    if !READY.load(Ordering::Acquire) && ensure_started().is_err() {
        return;
    }
    let mut g = CLIENT.lock();
    let Some(c) = g.as_mut() else {
        return;
    };
    let prev = if c.have_prev { Some(c.prev) } else { None };
    let (out, last) = crate::bridges::t1::ble_adpcm::encode_16k_to_cable_le(samples, prev);
    if let Some(s) = last {
        c.prev = s;
        c.have_prev = true;
    }
    match c.sock.send_to(&out, c.peer) {
        Ok(_) => {
            c.sent.fetch_add(1, Ordering::Relaxed);
            let level = {
                let sum: f64 = samples.iter().map(|&s| (s as f64) * (s as f64)).sum();
                let rms = (sum / samples.len().max(1) as f64).sqrt();
                (rms / 32768.0).clamp(0.0, 1.0) as f32
            };
            crate::bridges::t1::ble_voice_meter::on_pcm(samples, true);
            crate::bridges::t1::ble_voice_meter::on_udp_sent(level);
        }
        Err(_) => {
            c.dropped.fetch_add(1, Ordering::Relaxed);
            crate::bridges::t1::ble_voice_meter::on_pcm(samples, false);
        }
    }
}

pub fn shutdown() {
    READY.store(false, Ordering::Release);
    *CLIENT.lock() = None;
}
