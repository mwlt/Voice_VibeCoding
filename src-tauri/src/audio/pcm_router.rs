//! 对齐 Python `audio_router.py` PCM 侧：独立进程监听 UDP，写入 VB-CABLE
//!
//! 协议：PING→PONG / CLEAR / END / 原始 int16 LE PCM（48k mono）
//! Windows：子进程挂到 Job Object（KILL_ON_JOB_CLOSE），父进程崩溃时子进程一并退出。

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub const DEFAULT_PCM_PORT: u16 = 31680;

fn pcm_port() -> u16 {
    std::env::var("REMOTE_BRIDGE_PCM_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_PCM_PORT)
}

fn find_cable(host: &cpal::Host) -> Result<cpal::Device, String> {
    let devices = host
        .output_devices()
        .map_err(|e| format!("枚举输出失败: {e}"))?;
    for d in devices {
        let Ok(name) = d.name() else { continue };
        if name.to_ascii_lowercase().contains("cable input") {
            return Ok(d);
        }
    }
    Err("未找到 CABLE Input (VB-Audio Virtual Cable)".into())
}

/// CLI：`xiaomi-audio-router [--pcm-port N]`
pub fn run_audio_router_cli(args: &[String]) -> i32 {
    crate::logging::init_from_env();
    let mut port = pcm_port();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--pcm-port" {
            if let Some(v) = args.get(i + 1).and_then(|s| s.parse().ok()) {
                port = v;
                i += 2;
                continue;
            }
        }
        i += 1;
    }
    match run_router(port) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("xiaomi-audio-router failed: {e}");
            20
        }
    }
}

fn run_router(port: u16) -> Result<(), String> {
    let host = cpal::default_host();
    let device = find_cable(&host)?;
    let supported = device
        .default_output_config()
        .map_err(|e| format!("输出配置: {e}"))?;
    let sample_format = supported.sample_format();
    let stream_config = low_latency_stream_config(&supported);
    eprintln!(
        "AUDIO ROUTER device={} port={} format={:?} buffer={:?}",
        device.name().unwrap_or_default(),
        port,
        sample_format,
        stream_config.buffer_size
    );

    let buffer = Arc::new(Mutex::new(VecDeque::<i16>::new()));
    let running = Arc::new(AtomicBool::new(true));
    let buffer_cb = Arc::clone(&buffer);
    let running_cb = Arc::clone(&running);
    let channels = stream_config.channels as usize;

    let stream = match sample_format {
        cpal::SampleFormat::I16 => build_i16_stream(
            &device,
            &stream_config,
            &supported,
            buffer_cb,
            running_cb,
            channels,
        )?,
        cpal::SampleFormat::F32 => build_f32_stream(
            &device,
            &stream_config,
            &supported,
            buffer_cb,
            running_cb,
            channels,
        )?,
        other => return Err(format!("unsupported format {other:?}")),
    };
    stream.play().map_err(|e| e.to_string())?;

    let sock = UdpSocket::bind(format!("127.0.0.1:{port}"))
        .map_err(|e| format!("bind pcm {port}: {e}"))?;
    sock.set_read_timeout(Some(Duration::from_millis(500)))
        .map_err(|e| e.to_string())?;
    eprintln!("AUDIO ROUTER READY pcm=127.0.0.1:{port}");

    // 最多缓存约 60ms@48k，避免堆积导致听写「越说越慢」
    const MAX_BUFFER_SAMPLES: usize = 2_880;

    let mut buf = [0u8; 65536];
    loop {
        match sock.recv_from(&mut buf) {
            Ok((n, peer)) => {
                let data = &buf[..n];
                if data == b"PING" {
                    let _ = sock.send_to(b"PONG", peer);
                } else if data == b"CLEAR" || data == b"END" {
                    buffer.lock().clear();
                } else if data == b"STOP" || data.is_empty() {
                    // ignore
                } else if n % 2 == 0 {
                    let mut samples = Vec::with_capacity(n / 2);
                    for chunk in data.chunks_exact(2) {
                        samples.push(i16::from_le_bytes([chunk[0], chunk[1]]));
                    }
                    let mut b = buffer.lock();
                    b.extend(samples);
                    while b.len() > MAX_BUFFER_SAMPLES {
                        b.pop_front();
                    }
                }
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => {
                eprintln!("AUDIO ROUTER recv: {e}");
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        // 兜底：无 Job 时仍靠 parent-pid 自检
        if let Ok(pid_s) = std::env::var("REMOTE_BRIDGE_PARENT_PID") {
            if let Ok(pid) = pid_s.parse::<u32>() {
                if !parent_alive(pid) {
                    break;
                }
            }
        }
    }
    running.store(false, Ordering::Release);
    Ok(())
}

/// 对齐 Python `latency="low"`：约 10ms 固定缓冲，失败则回退默认
fn low_latency_stream_config(supported: &cpal::SupportedStreamConfig) -> cpal::StreamConfig {
    use cpal::{BufferSize, SupportedBufferSize};
    let mut cfg = supported.config();
    let rate = cfg.sample_rate.0.max(1);
    let target = ((rate as f32) * 0.01).round() as u32; // ~10ms
    let frames = match supported.buffer_size() {
        SupportedBufferSize::Range { min, max } => target.clamp(*min, *max).max(*min),
        SupportedBufferSize::Unknown => target.clamp(256, 1024),
    };
    cfg.buffer_size = BufferSize::Fixed(frames);
    cfg
}

fn build_i16_stream(
    device: &cpal::Device,
    preferred: &cpal::StreamConfig,
    supported: &cpal::SupportedStreamConfig,
    buffer_cb: Arc<Mutex<VecDeque<i16>>>,
    running_cb: Arc<AtomicBool>,
    channels: usize,
) -> Result<cpal::Stream, String> {
    let make = |cfg: &cpal::StreamConfig,
                buffer_cb: Arc<Mutex<VecDeque<i16>>>,
                running_cb: Arc<AtomicBool>| {
        device.build_output_stream(
            cfg,
            move |data: &mut [i16], _| {
                if !running_cb.load(Ordering::Acquire) {
                    data.fill(0);
                    return;
                }
                let mut buf = buffer_cb.lock();
                for frame in data.chunks_mut(channels.max(1)) {
                    let s = buf.pop_front().unwrap_or(0);
                    for o in frame.iter_mut() {
                        *o = s;
                    }
                }
            },
            |e| eprintln!("audio stream err: {e}"),
            None,
        )
    };
    match make(preferred, Arc::clone(&buffer_cb), Arc::clone(&running_cb)) {
        Ok(s) => Ok(s),
        Err(e) => {
            eprintln!("AUDIO ROUTER low-latency buffer rejected ({e}); fallback default");
            let mut fallback = supported.config();
            fallback.buffer_size = cpal::BufferSize::Default;
            make(&fallback, buffer_cb, running_cb).map_err(|e| e.to_string())
        }
    }
}

fn build_f32_stream(
    device: &cpal::Device,
    preferred: &cpal::StreamConfig,
    supported: &cpal::SupportedStreamConfig,
    buffer_cb: Arc<Mutex<VecDeque<i16>>>,
    running_cb: Arc<AtomicBool>,
    channels: usize,
) -> Result<cpal::Stream, String> {
    let make = |cfg: &cpal::StreamConfig,
                buffer_cb: Arc<Mutex<VecDeque<i16>>>,
                running_cb: Arc<AtomicBool>| {
        device.build_output_stream(
            cfg,
            move |data: &mut [f32], _| {
                if !running_cb.load(Ordering::Acquire) {
                    data.fill(0.0);
                    return;
                }
                let mut buf = buffer_cb.lock();
                for frame in data.chunks_mut(channels.max(1)) {
                    let s = buf.pop_front().unwrap_or(0);
                    let f = s as f32 / 32768.0;
                    for o in frame.iter_mut() {
                        *o = f;
                    }
                }
            },
            |e| eprintln!("audio stream err: {e}"),
            None,
        )
    };
    match make(preferred, Arc::clone(&buffer_cb), Arc::clone(&running_cb)) {
        Ok(s) => Ok(s),
        Err(e) => {
            eprintln!("AUDIO ROUTER low-latency buffer rejected ({e}); fallback default");
            let mut fallback = supported.config();
            fallback.buffer_size = cpal::BufferSize::Default;
            make(&fallback, buffer_cb, running_cb).map_err(|e| e.to_string())
        }
    }
}

#[cfg(target_os = "windows")]
fn parent_alive(pid: u32) -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    unsafe {
        match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(h) => {
                let _ = CloseHandle(h);
                true
            }
            Err(_) => false,
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn parent_alive(_pid: u32) -> bool {
    true
}

struct AudioChild {
    child: std::process::Child,
    #[cfg(target_os = "windows")]
    job: Option<windows::Win32::Foundation::HANDLE>,
}

// Job HANDLE 仅由本模块串行持有
unsafe impl Send for AudioChild {}

impl Drop for AudioChild {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        #[cfg(target_os = "windows")]
        {
            if let Some(job) = self.job.take() {
                unsafe {
                    let _ = windows::Win32::Foundation::CloseHandle(job);
                }
            }
        }
    }
}

static AUDIO_CHILD: Mutex<Option<AudioChild>> = Mutex::new(None);

#[cfg(target_os = "windows")]
fn create_kill_on_close_job() -> Result<windows::Win32::Foundation::HANDLE, String> {
    use std::ffi::c_void;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::JobObjects::{
        CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    unsafe {
        let job = CreateJobObjectW(None, PCWSTR::null())
            .map_err(|e| format!("CreateJobObjectW: {e}"))?;
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if let Err(e) = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const c_void,
            std::mem::size_of_val(&info) as u32,
        ) {
            let _ = CloseHandle(job);
            return Err(format!("SetInformationJobObject: {e}"));
        }
        Ok(job)
    }
}

/// 主进程：拉起 audio router 子进程（对齐 Python XiaomiWorkers audio 角色）
pub fn spawn_audio_router_process() -> Result<(), String> {
    stop_audio_router_process();
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let port = pcm_port();
    let parent = std::process::id().to_string();
    let mut cmd = std::process::Command::new(&exe);
    cmd.arg("xiaomi-audio-router")
        .arg("--pcm-port")
        .arg(port.to_string())
        .env("REMOTE_BRIDGE_PCM_PORT", port.to_string())
        .env("REMOTE_BRIDGE_PARENT_PID", &parent)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if let Ok(log_path) = std::env::var("REMOTE_BRIDGE_LOG_PATH") {
        cmd.env("REMOTE_BRIDGE_LOG_PATH", log_path);
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // 隐藏控制台：无窗口 + 分离，避免启动时闪黑框
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        cmd.creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS);
    }
    let child = cmd.spawn().map_err(|e| format!("spawn audio router: {e}"))?;

    #[cfg(target_os = "windows")]
    let job = {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::{CloseHandle, HANDLE};
        use windows::Win32::System::JobObjects::AssignProcessToJobObject;

        match create_kill_on_close_job() {
            Ok(job) => {
                let proc = HANDLE(child.as_raw_handle());
                if let Err(e) = unsafe { AssignProcessToJobObject(job, proc) } {
                    log::warn!("AssignProcessToJobObject failed: {e}; fallback parent-pid watch");
                    let _ = unsafe { CloseHandle(job) };
                    None
                } else {
                    log::info!("AUDIO ROUTER assigned to JobObject (KILL_ON_JOB_CLOSE)");
                    Some(job)
                }
            }
            Err(e) => {
                log::warn!("CreateJobObject failed: {e}; fallback parent-pid watch");
                None
            }
        }
    };

    log::info!(
        "XIAOMI AUDIO ROUTER spawned pid={} pcm_port={}",
        child.id(),
        port
    );
    *AUDIO_CHILD.lock() = Some(AudioChild {
        child,
        #[cfg(target_os = "windows")]
        job,
    });
    Ok(())
}

pub fn stop_audio_router_process() {
    // Drop 会 kill + CloseHandle(job)
    let _ = AUDIO_CHILD.lock().take();
}

/// 进程是否仍在跑（poll）
pub fn audio_router_process_alive() -> bool {
    let mut guard = AUDIO_CHILD.lock();
    match guard.as_mut() {
        Some(ac) => match ac.child.try_wait() {
            Ok(None) => true,
            Ok(Some(_)) => {
                *guard = None;
                false
            }
            Err(_) => false,
        },
        None => false,
    }
}

/// UDP PING/PONG：语音路由就绪（对齐 Python audio status）
pub fn audio_router_ready() -> bool {
    let port = pcm_port();
    let Ok(sock) = UdpSocket::bind("127.0.0.1:0") else {
        return false;
    };
    let _ = sock.set_read_timeout(Some(Duration::from_millis(300)));
    if sock.send_to(b"PING", ("127.0.0.1", port)).is_err() {
        return false;
    }
    let mut buf = [0u8; 32];
    match sock.recv_from(&mut buf) {
        Ok((n, _)) => &buf[..n] == b"PONG",
        Err(_) => false,
    }
}
