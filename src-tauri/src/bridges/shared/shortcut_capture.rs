//! 快捷键捕获
//!
//! 架构（修「任何键都录不进 / 录完后残留吞键」）：
//! - WH_KEYBOARD_LL **只负责吞键**（`SWALLOW_ACTIVE` 时 return 1），不做和弦判定
//! - GetAsyncKeyState **轮询负责识别**单键/组合键（物理键状态与是否吞键无关）
//! - 钩子消息循环写法对齐本仓库已验证的 `special_keys.rs`
//!
//! 完成规则：
//! - 主键按下：提交「修饰键 + 主键」
//! - 纯修饰键 ≥2：任一抬起即提交（扛 Win KEYUP 丢失）
//! - 纯修饰键 =1：抬起后提交

use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, LazyLock, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

const VK_SHIFT: u32 = 0x10;
const VK_CONTROL: u32 = 0x11;
const VK_MENU: u32 = 0x12;
const VK_LWIN: u32 = 0x5B;
const VK_RWIN: u32 = 0x5C;
const VK_LSHIFT: u32 = 0xA0;
const VK_RSHIFT: u32 = 0xA1;
const VK_LCONTROL: u32 = 0xA2;
const VK_RCONTROL: u32 = 0xA3;
const VK_LMENU: u32 = 0xA4;
const VK_RMENU: u32 = 0xA5;

const ARM_GRACE_MS: u64 = 200;

fn scan_vks() -> Vec<u32> {
    let mut vks = vec![
        VK_LCONTROL, VK_RCONTROL, VK_CONTROL, VK_LSHIFT, VK_RSHIFT, VK_SHIFT, VK_LMENU,
        VK_RMENU, VK_MENU, VK_LWIN, VK_RWIN, 0x08, 0x09, 0x0D, 0x1B, 0x20, 0x21, 0x22, 0x23,
        0x24, 0x25, 0x26, 0x27, 0x28, 0x2D, 0x2E, 0x5D, 0xAD, 0xAE, 0xAF,
    ];
    vks.extend(0x30u32..=0x39);
    vks.extend(0x41u32..=0x5A);
    vks.extend(0x70u32..=0x7B);
    vks.extend([0xBA, 0xBB, 0xBC, 0xBD, 0xBE, 0xBF, 0xC0, 0xDB, 0xDC, 0xDD, 0xDE]);
    vks.extend(0x60u32..=0x69);
    vks.extend([0x6A, 0x6B, 0x6D, 0x6E, 0x6F]);
    vks
}

fn is_modifier(vk: u32) -> bool {
    matches!(
        vk,
        VK_SHIFT
            | VK_CONTROL
            | VK_MENU
            | VK_LWIN
            | VK_RWIN
            | VK_LSHIFT
            | VK_RSHIFT
            | VK_LCONTROL
            | VK_RCONTROL
            | VK_LMENU
            | VK_RMENU
    )
}

fn normalize_chord(keys: &[u32]) -> Vec<u32> {
    let set: HashSet<u32> = keys.iter().copied().collect();
    let mut out = Vec::new();

    let ctrl = if set.contains(&VK_LCONTROL) {
        Some(VK_LCONTROL)
    } else if set.contains(&VK_RCONTROL) {
        Some(VK_RCONTROL)
    } else if set.contains(&VK_CONTROL) {
        Some(VK_LCONTROL)
    } else {
        None
    };
    let shift = if set.contains(&VK_LSHIFT) {
        Some(VK_LSHIFT)
    } else if set.contains(&VK_RSHIFT) {
        Some(VK_RSHIFT)
    } else if set.contains(&VK_SHIFT) {
        Some(VK_LSHIFT)
    } else {
        None
    };
    let alt = if set.contains(&VK_LMENU) {
        Some(VK_LMENU)
    } else if set.contains(&VK_RMENU) {
        Some(VK_RMENU)
    } else if set.contains(&VK_MENU) {
        Some(VK_LMENU)
    } else {
        None
    };
    let win = if set.contains(&VK_LWIN) {
        Some(VK_LWIN)
    } else if set.contains(&VK_RWIN) {
        Some(VK_RWIN)
    } else {
        None
    };

    for m in [ctrl, shift, alt, win].into_iter().flatten() {
        out.push(m);
    }
    let mut mains: Vec<u32> = set
        .into_iter()
        .filter(|vk| !is_modifier(*vk))
        .collect();
    mains.sort();
    out.extend(mains);
    out
}

pub fn vk_to_label(vk: u32) -> String {
    match vk {
        VK_SHIFT | VK_LSHIFT => "左 Shift".into(),
        VK_RSHIFT => "右 Shift".into(),
        VK_CONTROL | VK_LCONTROL => "左 Ctrl".into(),
        VK_RCONTROL => "右 Ctrl".into(),
        VK_MENU | VK_LMENU => "左 Alt".into(),
        VK_RMENU => "右 Alt".into(),
        VK_LWIN => "左 Win".into(),
        VK_RWIN => "右 Win".into(),
        0x08 => "Backspace".into(),
        0x09 => "Tab".into(),
        0x0D => "Enter".into(),
        0x1B => "Esc".into(),
        0x20 => "Space".into(),
        0x21 => "PageUp".into(),
        0x22 => "PageDown".into(),
        0x23 => "End".into(),
        0x24 => "Home".into(),
        0x25 => "←".into(),
        0x26 => "↑".into(),
        0x27 => "→".into(),
        0x28 => "↓".into(),
        0x2E => "Delete".into(),
        0xAD => "Mute".into(),
        0xAE => "Vol-".into(),
        0xAF => "Vol+".into(),
        0x70..=0x7B => format!("F{}", vk - 0x6F),
        0x30..=0x39 => format!("{}", vk - 0x30),
        0x41..=0x5A => ((vk as u8) as char).to_string(),
        _ => format!("VK_0x{vk:02X}"),
    }
}

fn should_commit_mods_only(hist_norm: &[u32], active_norm: &[u32], newly_up_mod: bool) -> bool {
    if !newly_up_mod || hist_norm.is_empty() {
        return false;
    }
    if hist_norm.iter().any(|vk| !is_modifier(*vk)) {
        return false;
    }
    hist_norm.len() >= 2 || active_norm.is_empty()
}

struct CaptureEngine {
    prev: HashMap<u32, bool>,
    active_mods: HashSet<u32>,
    chord_history: HashSet<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CaptureStep {
    Progress(Vec<u32>),
    Captured(Vec<u32>),
}

impl CaptureEngine {
    fn new(initial_down: HashMap<u32, bool>) -> Self {
        let mut active_mods = HashSet::new();
        let mut chord_history = HashSet::new();
        for (&vk, &down) in &initial_down {
            if down && is_modifier(vk) {
                active_mods.insert(vk);
                chord_history.insert(vk);
            }
        }
        Self {
            prev: initial_down,
            active_mods,
            chord_history,
        }
    }

    fn progress_mods(&self) -> Vec<u32> {
        normalize_chord(&self.active_mods.iter().copied().collect::<Vec<_>>())
    }

    fn step(&mut self, down: &HashMap<u32, bool>) -> CaptureStep {
        let mut newly_down_mains: Vec<u32> = Vec::new();
        let mut newly_up_mod = false;

        for (&vk, &is_down) in down {
            let was = self.prev.get(&vk).copied().unwrap_or(false);
            if is_down && !was {
                self.chord_history.insert(vk);
                if is_modifier(vk) {
                    self.active_mods.insert(vk);
                } else {
                    newly_down_mains.push(vk);
                }
            } else if !is_down && was && is_modifier(vk) {
                self.active_mods.remove(&vk);
                newly_up_mod = true;
            }
            self.prev.insert(vk, is_down);
        }

        if newly_down_mains.len() == 1 {
            let mut keys: Vec<u32> = self.active_mods.iter().copied().collect();
            keys.push(newly_down_mains[0]);
            return CaptureStep::Captured(normalize_chord(&keys));
        }
        if newly_down_mains.len() > 1 {
            return CaptureStep::Progress(self.progress_mods());
        }

        let hist = normalize_chord(&self.chord_history.iter().copied().collect::<Vec<_>>());
        let active = self.progress_mods();
        if should_commit_mods_only(&hist, &active, newly_up_mod) {
            return CaptureStep::Captured(hist);
        }
        CaptureStep::Progress(active)
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutCapturedPayload {
    pub keys: Vec<u32>,
    pub labels: Vec<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutCaptureProgress {
    pub labels: Vec<String>,
}

struct CaptureRuntime {
    stop: AtomicBool,
    capturing: AtomicBool,
    pending: Mutex<Option<ShortcutCapturedPayload>>,
    progress: Mutex<Vec<String>>,
    app: Mutex<Option<AppHandle>>,
}

impl CaptureRuntime {
    fn new() -> Self {
        Self {
            stop: AtomicBool::new(true),
            capturing: AtomicBool::new(false),
            pending: Mutex::new(None),
            progress: Mutex::new(Vec::new()),
            app: Mutex::new(None),
        }
    }

    fn publish_progress(&self, labels: Vec<String>) {
        *self.progress.lock().unwrap() = labels.clone();
        if let Some(app) = self.app.lock().unwrap().as_ref() {
            let _ = app.emit(
                "shortcut-capture-progress",
                ShortcutCaptureProgress { labels },
            );
        }
    }

    fn publish_result(&self, keys: Vec<u32>) {
        let keys = normalize_chord(&keys);
        if keys.is_empty() {
            return;
        }
        let labels: Vec<String> = keys.iter().copied().map(vk_to_label).collect();
        log::info!("Shortcut captured: {}", labels.join("+"));
        let payload = ShortcutCapturedPayload {
            keys,
            labels: labels.clone(),
        };
        *self.pending.lock().unwrap() = Some(payload.clone());
        self.capturing.store(false, Ordering::SeqCst);
        // 对齐 Python：提交后钩子继续吞键，直到 blocked_vks 全部 KEYUP 再退出。
        mark_capture_submitted();
        if let Some(app) = self.app.lock().unwrap().as_ref() {
            let _ = app.emit("shortcut-captured", payload);
        }
    }

    fn take_pending(&self) -> Option<ShortcutCapturedPayload> {
        self.pending.lock().unwrap().take()
    }
}

// ---------------------------------------------------------------------------
// 吞键钩子：全局原子开关，回调极简
// ---------------------------------------------------------------------------

static SWALLOW_ACTIVE: AtomicBool = AtomicBool::new(false);
static CAPTURE_SUBMITTED: AtomicBool = AtomicBool::new(false);
static HOOK_THREAD_ID: AtomicU32 = AtomicU32::new(0);
static HOOK_HANDLE: AtomicUsize = AtomicUsize::new(0);
static SWALLOW_HIT_LOGGED: AtomicBool = AtomicBool::new(false);
static BLOCKED_VKS: LazyLock<Mutex<HashSet<u32>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

fn reset_hook_session() {
    CAPTURE_SUBMITTED.store(false, Ordering::SeqCst);
    if let Ok(mut blocked) = BLOCKED_VKS.lock() {
        blocked.clear();
    }
}

fn mark_capture_submitted() {
    CAPTURE_SUBMITTED.store(true, Ordering::SeqCst);
    maybe_finish_hook_after_drain();
}

fn maybe_finish_hook_after_drain() {
    if !CAPTURE_SUBMITTED.load(Ordering::SeqCst) {
        return;
    }
    let empty = BLOCKED_VKS
        .lock()
        .map(|g| g.is_empty())
        .unwrap_or(false);
    if empty {
        set_swallow_active(false);
        request_hook_thread_quit();
        log::info!("Shortcut capture hook drain complete");
    }
}

fn set_swallow_active(active: bool) {
    SWALLOW_ACTIVE.store(active, Ordering::SeqCst);
}

fn request_hook_thread_quit() {
    let tid = HOOK_THREAD_ID.load(Ordering::SeqCst);
    if tid == 0 {
        return;
    }
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};
        // 即使在钩子线程内也必须投递，否则 GetMessage 永不返回、钩子残留
        let _ = unsafe {
            PostThreadMessageW(
                tid,
                WM_QUIT,
                windows::Win32::Foundation::WPARAM(0),
                windows::Win32::Foundation::LPARAM(0),
            )
        };
    }
}

fn emergency_unhook() {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::WindowsAndMessaging::{UnhookWindowsHookEx, HHOOK};
        let raw = HOOK_HANDLE.swap(0, Ordering::SeqCst);
        if raw != 0 {
            let _ = unsafe { UnhookWindowsHookEx(HHOOK(raw as *mut _)) };
            log::warn!("Shortcut capture emergency unhook raw={raw:#x}");
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        HOOK_HANDLE.store(0, Ordering::SeqCst);
    }
    HOOK_THREAD_ID.store(0, Ordering::SeqCst);
    set_swallow_active(false);
}

pub struct ShortcutCaptureSession {
    runtime: Arc<CaptureRuntime>,
    poll_join: Mutex<Option<JoinHandle<()>>>,
    hook_join: Mutex<Option<JoinHandle<()>>>,
}

impl ShortcutCaptureSession {
    pub fn new() -> Self {
        Self {
            runtime: Arc::new(CaptureRuntime::new()),
            poll_join: Mutex::new(None),
            hook_join: Mutex::new(None),
        }
    }

    pub fn cancel(&self) -> Result<(), String> {
        self.runtime.stop.store(true, Ordering::SeqCst);
        self.runtime.capturing.store(false, Ordering::SeqCst);

        if CAPTURE_SUBMITTED.load(Ordering::SeqCst) {
            // 已提交：等钩子线程根据 blocked_vks 自然排空后退出（勿 SendInput 强放 Alt）
            let hook_ok = join_with_timeout(self.hook_join.lock().unwrap().take(), 2500);
            if !hook_ok {
                log::warn!("Shortcut capture hook drain timeout, emergency unhook");
                emergency_unhook();
            }
        } else {
            set_swallow_active(false);
            request_hook_thread_quit();
            let hook_ok = join_with_timeout(self.hook_join.lock().unwrap().take(), 800);
            if !hook_ok {
                emergency_unhook();
                request_hook_thread_quit();
            }
        }

        join_with_timeout(self.poll_join.lock().unwrap().take(), 400);
        reset_hook_session();
        self.runtime.progress.lock().unwrap().clear();
        Ok(())
    }

    pub fn start(&self, app: AppHandle) -> Result<(), String> {
        self.cancel()?;

        *self.runtime.app.lock().unwrap() = Some(app);
        *self.runtime.pending.lock().unwrap() = None;
        self.runtime.progress.lock().unwrap().clear();
        self.runtime.stop.store(false, Ordering::SeqCst);
        self.runtime.capturing.store(true, Ordering::SeqCst);
        reset_hook_session();

        // 先装吞键钩子，再开轮询识别
        #[cfg(target_os = "windows")]
        {
            match start_swallow_hook_thread() {
                Ok(h) => {
                    *self.hook_join.lock().unwrap() = Some(h);
                    SWALLOW_HIT_LOGGED.store(false, Ordering::SeqCst);
                    set_swallow_active(true);
                    log::info!("Shortcut capture swallow hook ON");
                }
                Err(e) => {
                    log::warn!(
                        "Shortcut capture swallow hook failed: {e} (poll-only, OS/WebView 仍会响应快捷键)"
                    );
                }
            }
        }

        let runtime = Arc::clone(&self.runtime);
        let poll = thread::Builder::new()
            .name("shortcut-capture-poll".into())
            .spawn(move || capture_poll_thread(runtime))
            .map_err(|e| format!("启动录入轮询失败: {e}"))?;
        *self.poll_join.lock().unwrap() = Some(poll);
        log::info!("Shortcut capture started (poll detect + optional LL swallow)");
        Ok(())
    }

    pub fn take_result(&self) -> Option<ShortcutCapturedPayload> {
        self.runtime.take_pending()
    }

    pub fn is_active(&self) -> bool {
        self.runtime.capturing.load(Ordering::SeqCst)
    }
}

impl Default for ShortcutCaptureSession {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ShortcutCaptureSession {
    fn drop(&mut self) {
        let _ = self.cancel();
    }
}

fn join_with_timeout(handle: Option<JoinHandle<()>>, ms: u64) -> bool {
    let Some(h) = handle else {
        return true;
    };
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = h.join();
        let _ = tx.send(());
    });
    rx.recv_timeout(Duration::from_millis(ms)).is_ok()
}

#[cfg(target_os = "windows")]
fn start_swallow_hook_thread() -> Result<JoinHandle<()>, String> {
    let (tx, rx) = mpsc::channel::<Result<(), String>>();
    let handle = thread::Builder::new()
        .name("shortcut-swallow-hook".into())
        .spawn(move || swallow_hook_thread(tx))
        .map_err(|e| format!("{e}"))?;

    match rx.recv_timeout(Duration::from_millis(800)) {
        Ok(Ok(())) => Ok(handle),
        Ok(Err(e)) => {
            let _ = handle.join();
            Err(e)
        }
        Err(_) => {
            request_hook_thread_quit();
            let _ = handle.join();
            Err("钩子线程启动超时".into())
        }
    }
}

#[cfg(target_os = "windows")]
fn swallow_hook_thread(ready: mpsc::Sender<Result<(), String>>) {
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
        MSG, WH_KEYBOARD_LL,
    };

    let tid = unsafe { GetCurrentThreadId() };
    HOOK_THREAD_ID.store(tid, Ordering::SeqCst);

    // 对齐 Python / hotkey_monitor：LL 钩子需传入本模块句柄，hMod=None 时常见「装上了但不回调」
    let hmod = match unsafe { GetModuleHandleW(None) } {
        Ok(h) => h,
        Err(e) => {
            let msg = format!("GetModuleHandleW failed: {e}");
            log::error!("Shortcut capture {msg}");
            let _ = ready.send(Err(msg));
            HOOK_THREAD_ID.store(0, Ordering::SeqCst);
            return;
        }
    };

    let hook =
        unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(swallow_low_level_proc), hmod, 0) };
    let hook = match hook {
        Ok(h) if !h.is_invalid() => h,
        other => {
            let msg = format!("SetWindowsHookExW failed: {other:?}");
            log::error!("Shortcut capture {msg}");
            let _ = ready.send(Err(msg));
            HOOK_THREAD_ID.store(0, Ordering::SeqCst);
            return;
        }
    };

    HOOK_HANDLE.store(hook.0 as usize, Ordering::SeqCst);
    let _ = ready.send(Ok(()));
    log::info!("Shortcut capture LL swallow armed tid={tid}");

    unsafe {
        let mut msg = MSG::default();
        // 对齐 special_keys：用 .0 判断，勿用 into() 误判
        loop {
            let ret = GetMessageW(&mut msg, None, 0, 0);
            if ret.0 == -1 || ret.0 == 0 {
                break;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        HOOK_HANDLE.store(0, Ordering::SeqCst);
        let _ = UnhookWindowsHookEx(hook);
    }

    HOOK_THREAD_ID.store(0, Ordering::SeqCst);
    set_swallow_active(false);
    log::info!("Shortcut capture LL swallow thread exit");
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn swallow_low_level_proc(
    code: i32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::Foundation::LRESULT;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, HHOOK, KBDLLHOOKSTRUCT, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
    };

    const LLKHF_INJECTED: u32 = 0x10;
    let hook = HHOOK(HOOK_HANDLE.load(Ordering::SeqCst) as *mut _);

    if code < 0 {
        return unsafe { CallNextHookEx(hook, code, wparam, lparam) };
    }

    let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
    let flags = info.flags.0 as u32;
    if (flags & LLKHF_INJECTED) != 0 {
        return unsafe { CallNextHookEx(hook, code, wparam, lparam) };
    }

    if !SWALLOW_ACTIVE.load(Ordering::SeqCst) {
        return unsafe { CallNextHookEx(hook, code, wparam, lparam) };
    }

    let wp = wparam.0 as u32;
    let is_down = wp == WM_KEYDOWN || wp == WM_SYSKEYDOWN;
    let is_up = wp == WM_KEYUP || wp == WM_SYSKEYUP;
    if is_down || is_up {
        let vk = info.vkCode;
        if let Ok(mut blocked) = BLOCKED_VKS.lock() {
            if is_down {
                blocked.insert(vk);
            } else {
                blocked.remove(&vk);
            }
        }
        if is_up {
            maybe_finish_hook_after_drain();
        }
    }

    if !SWALLOW_HIT_LOGGED.swap(true, Ordering::SeqCst) {
        log::info!(
            "Shortcut capture swallow hit vk=0x{:02X} wp=0x{:X} flags=0x{:X}",
            info.vkCode,
            wp,
            flags
        );
    }
    // 非零 = 吞掉（含 WM_SYSKEY* / Alt+Space），对齐 Python 始终 return 1
    LRESULT(1)
}

// ---------------------------------------------------------------------------
// 轮询识别
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
fn key_down(vk: u32) -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    unsafe { GetAsyncKeyState(vk as i32) as u16 & 0x8000 != 0 }
}

#[cfg(not(target_os = "windows"))]
fn key_down(_vk: u32) -> bool {
    false
}

fn capture_poll_thread(runtime: Arc<CaptureRuntime>) {
    let vks = scan_vks();

    // 宽限：同步状态，不触发边沿
    let grace_deadline = Instant::now() + Duration::from_millis(ARM_GRACE_MS);
    let mut prev: HashMap<u32, bool> = vks.iter().map(|&vk| (vk, key_down(vk))).collect();
    while !runtime.stop.load(Ordering::SeqCst) && Instant::now() < grace_deadline {
        for &vk in &vks {
            prev.insert(vk, key_down(vk));
        }
        thread::sleep(Duration::from_millis(10));
    }

    if runtime.stop.load(Ordering::SeqCst) {
        return;
    }

    let mut engine = CaptureEngine::new(prev);
    let mut finished = false;
    let mut last_progress = String::new();
    log::info!("Shortcut capture poll armed");

    while !runtime.stop.load(Ordering::SeqCst) && !finished {
        if !runtime.capturing.load(Ordering::SeqCst) {
            break;
        }
        let frame: HashMap<u32, bool> = vks.iter().map(|&vk| (vk, key_down(vk))).collect();
        match engine.step(&frame) {
            CaptureStep::Captured(keys) => {
                runtime.publish_result(keys);
                finished = true;
            }
            CaptureStep::Progress(mods) => {
                let labels: Vec<String> = mods.iter().copied().map(vk_to_label).collect();
                let key = labels.join("+");
                if key != last_progress {
                    last_progress = key;
                    if !labels.is_empty() {
                        runtime.publish_progress(labels);
                    }
                }
            }
        }
        thread::sleep(Duration::from_millis(10));
    }

    // 清理交给 cancel()/钩子 drain；轮询线程只等待 stop
    while finished && !runtime.stop.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(10));
    }
    log::info!("Shortcut capture poll exit finished={finished}");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(downs: &[(u32, bool)]) -> HashMap<u32, bool> {
        downs.iter().copied().collect()
    }

    fn idle_engine() -> CaptureEngine {
        CaptureEngine::new(HashMap::new())
    }

    #[test]
    fn test_normalize_and_labels() {
        assert_eq!(
            normalize_chord(&[VK_CONTROL, VK_LCONTROL, 0x41]),
            vec![VK_LCONTROL, 0x41]
        );
        assert_eq!(vk_to_label(VK_LCONTROL), "左 Ctrl");
        assert_eq!(vk_to_label(VK_LWIN), "左 Win");
    }

    #[test]
    fn capture_single_key() {
        let mut eng = idle_engine();
        assert_eq!(
            eng.step(&frame(&[(0x41, true)])),
            CaptureStep::Captured(vec![0x41])
        );
    }

    #[test]
    fn capture_ctrl_plus_a() {
        let mut eng = idle_engine();
        eng.step(&frame(&[(VK_LCONTROL, true), (VK_CONTROL, true)]));
        assert_eq!(
            eng.step(&frame(&[(VK_LCONTROL, true), (VK_CONTROL, true), (0x41, true)])),
            CaptureStep::Captured(vec![VK_LCONTROL, 0x41])
        );
    }

    #[test]
    fn capture_ctrl_win_on_first_release() {
        let mut eng = idle_engine();
        eng.step(&frame(&[(VK_LCONTROL, true), (VK_CONTROL, true)]));
        eng.step(&frame(&[
            (VK_LCONTROL, true),
            (VK_CONTROL, true),
            (VK_LWIN, true),
        ]));
        assert_eq!(
            eng.step(&frame(&[
                (VK_LCONTROL, false),
                (VK_CONTROL, false),
                (VK_LWIN, true),
            ])),
            CaptureStep::Captured(vec![VK_LCONTROL, VK_LWIN])
        );
    }

    #[test]
    fn capture_single_ctrl_on_release() {
        let mut eng = idle_engine();
        eng.step(&frame(&[(VK_LCONTROL, true), (VK_CONTROL, true)]));
        assert_eq!(
            eng.step(&frame(&[(VK_LCONTROL, false), (VK_CONTROL, false)])),
            CaptureStep::Captured(vec![VK_LCONTROL])
        );
    }

    #[test]
    fn second_session_engine_is_independent() {
        let mut a = idle_engine();
        a.step(&frame(&[(VK_LCONTROL, true)]));
        assert_eq!(
            a.step(&frame(&[(VK_LCONTROL, false)])),
            CaptureStep::Captured(vec![VK_LCONTROL])
        );
        let mut b = idle_engine();
        assert_eq!(
            b.step(&frame(&[(0x41, true)])),
            CaptureStep::Captured(vec![0x41])
        );
    }
}
