//! 快捷键捕获 — 用 GetAsyncKeyState 轮询（不依赖 WH_KEYBOARD_LL）
//!
//! 低级键盘钩子在 WebView2/Tauri 进程里经常装不上或被超时卸掉，导致「按什么都不录入」。
//! 这里改为物理键状态轮询，与是否有前台焦点无关，稳定可录入。
//! 结束时强制补发修饰键 KEYUP，减轻 Win 粘滞。

use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;
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

/// 参与扫描的 VK（修饰键 + 常用键）
fn scan_vks() -> Vec<u32> {
    let mut vks = vec![
        VK_LCONTROL, VK_RCONTROL, VK_CONTROL, VK_LSHIFT, VK_RSHIFT, VK_SHIFT, VK_LMENU,
        VK_RMENU, VK_MENU, VK_LWIN, VK_RWIN, 0x08, 0x09, 0x0D, 0x1B, 0x20, 0x21, 0x22, 0x23,
        0x24, 0x25, 0x26, 0x27, 0x28, 0x2D, 0x2E, 0x5D, 0xAD, 0xAE, 0xAF,
    ];
    // 0-9 A-Z
    vks.extend(0x30u32..=0x39);
    vks.extend(0x41u32..=0x5A);
    // F1-F12
    vks.extend(0x70u32..=0x7B);
    // OEM
    vks.extend([0xBA, 0xBB, 0xBC, 0xBD, 0xBE, 0xBF, 0xC0, 0xDB, 0xDC, 0xDD, 0xDE]);
    // Numpad
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

fn modifier_order(vk: u32) -> u32 {
    match vk {
        VK_CONTROL | VK_LCONTROL => 10,
        VK_RCONTROL => 11,
        VK_SHIFT | VK_LSHIFT => 20,
        VK_RSHIFT => 21,
        VK_MENU | VK_LMENU => 30,
        VK_RMENU => 31,
        VK_LWIN => 40,
        VK_RWIN => 41,
        _ => 99,
    }
}

/// 规范化修饰键：左右优先，去掉通用 VK_CONTROL/SHIFT/MENU 重复
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
        VK_SHIFT | VK_LSHIFT => "Shift".into(),
        VK_RSHIFT => "右 Shift".into(),
        VK_CONTROL | VK_LCONTROL => "Ctrl".into(),
        VK_RCONTROL => "右 Ctrl".into(),
        VK_MENU | VK_LMENU => "Alt".into(),
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
        log::info!("Shortcut captured via poll: {}", labels.join("+"));
        let payload = ShortcutCapturedPayload {
            keys,
            labels: labels.clone(),
        };
        *self.pending.lock().unwrap() = Some(payload.clone());
        self.capturing.store(false, Ordering::SeqCst);
        self.stop.store(true, Ordering::SeqCst);
        if let Some(app) = self.app.lock().unwrap().as_ref() {
            let _ = app.emit("shortcut-captured", payload);
        }
    }

    fn take_pending(&self) -> Option<ShortcutCapturedPayload> {
        self.pending.lock().unwrap().take()
    }
}

pub struct ShortcutCaptureSession {
    runtime: Arc<CaptureRuntime>,
    join: Mutex<Option<JoinHandle<()>>>,
}

impl ShortcutCaptureSession {
    pub fn new() -> Self {
        Self {
            runtime: Arc::new(CaptureRuntime::new()),
            join: Mutex::new(None),
        }
    }

    pub fn cancel(&self) -> Result<(), String> {
        self.runtime.stop.store(true, Ordering::SeqCst);
        self.runtime.capturing.store(false, Ordering::SeqCst);
        if let Some(h) = self.join.lock().unwrap().take() {
            let _ = h.join();
        }
        // 注意：不清理 pending，方便前端在 stop 前后仍能 poll 到结果
        self.runtime.progress.lock().unwrap().clear();
        force_release_modifiers();
        Ok(())
    }

    pub fn start(&self, app: AppHandle) -> Result<(), String> {
        self.cancel()?;

        *self.runtime.app.lock().unwrap() = Some(app);
        *self.runtime.pending.lock().unwrap() = None;
        self.runtime.progress.lock().unwrap().clear();
        self.runtime.stop.store(false, Ordering::SeqCst);
        self.runtime.capturing.store(true, Ordering::SeqCst);

        let runtime = Arc::clone(&self.runtime);
        let handle = thread::Builder::new()
            .name("shortcut-capture-poll".into())
            .spawn(move || capture_poll_thread(runtime))
            .map_err(|e| format!("启动录制线程失败: {e}"))?;
        *self.join.lock().unwrap() = Some(handle);
        log::info!("Shortcut capture started (GetAsyncKeyState poll)");
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

    // 等待所有键抬起，避免点击「录入」时鼠标/焦点键被算进去
    let warmup_deadline = std::time::Instant::now() + Duration::from_millis(800);
    while !runtime.stop.load(Ordering::SeqCst) && std::time::Instant::now() < warmup_deadline {
        let any_down = vks.iter().any(|&vk| key_down(vk));
        if !any_down {
            break;
        }
        thread::sleep(Duration::from_millis(15));
    }

    if runtime.stop.load(Ordering::SeqCst) {
        return;
    }

    // 关键：必须用当前真实键位初始化 prev。
    // 若一律 false，预热超时后仍按下/粘滞的键（尤其遥控器 HID 的 Home=0x24）
    // 会在首轮被当成「新按下」，导致录入结果全变成 VK_0x24。
    let mut prev: HashMap<u32, bool> = vks.iter().map(|&vk| (vk, key_down(vk))).collect();

    // 再稳定一小段，只同步状态、不触发录入，避开点击按钮后的毛刺边沿
    let settle_deadline = std::time::Instant::now() + Duration::from_millis(150);
    while !runtime.stop.load(Ordering::SeqCst) && std::time::Instant::now() < settle_deadline {
        for &vk in &vks {
            prev.insert(vk, key_down(vk));
        }
        thread::sleep(Duration::from_millis(10));
    }

    if runtime.stop.load(Ordering::SeqCst) {
        return;
    }

    let mut active_mods: HashSet<u32> = HashSet::new();
    let mut mod_history: HashSet<u32> = HashSet::new();
    // 稳定期结束时已按下的修饰键，记入当前组合（用户可先按住 Ctrl 再点录入）
    for &vk in &vks {
        if is_modifier(vk) && prev.get(&vk).copied().unwrap_or(false) {
            active_mods.insert(vk);
            mod_history.insert(vk);
        }
    }
    let mut finished = false;
    let mut last_progress = String::new();

    log::info!("Shortcut capture polling armed");

    while !runtime.stop.load(Ordering::SeqCst) && !finished {
        let mut newly_down_mains: Vec<u32> = Vec::new();
        let mut newly_up_mod = false;

        for &vk in &vks {
            let down = key_down(vk);
            let was = prev.get(&vk).copied().unwrap_or(false);

            if is_modifier(vk) {
                if down && !was {
                    active_mods.insert(vk);
                    mod_history.insert(vk);
                } else if !down && was {
                    active_mods.remove(&vk);
                    newly_up_mod = true;
                }
            } else if down && !was && !finished {
                newly_down_mains.push(vk);
            }
            prev.insert(vk, down);
        }

        let mut pressed_mods: Vec<u32> = active_mods.iter().copied().collect();
        pressed_mods.sort_by_key(|vk| modifier_order(*vk));
        let labels: Vec<String> = pressed_mods.iter().copied().map(vk_to_label).collect();
        let progress_key = labels.join("+");
        if progress_key != last_progress {
            last_progress = progress_key;
            runtime.publish_progress(labels);
        }

        // 同一轮出现多个主键边沿 → 多为状态未同步毛刺，忽略本帧
        if newly_down_mains.len() == 1 {
            let main_vk = newly_down_mains[0];
            let mut keys: Vec<u32> = active_mods.iter().copied().collect();
            keys.push(main_vk);
            runtime.publish_result(keys);
            finished = true;
            break;
        } else if newly_down_mains.len() > 1 {
            log::warn!(
                "Shortcut capture ignored ambiguous edges: {:?}",
                newly_down_mains
            );
        }

        if newly_up_mod && active_mods.is_empty() && !mod_history.is_empty() {
            let keys: Vec<u32> = mod_history.iter().copied().collect();
            runtime.publish_result(keys);
            finished = true;
            break;
        }

        thread::sleep(Duration::from_millis(10));
    }

    // 稍等再抬修饰键，避免和用户松手打架
    thread::sleep(Duration::from_millis(30));
    force_release_modifiers();
    runtime.capturing.store(false, Ordering::SeqCst);
    log::info!("Shortcut capture poll thread exit finished={finished}");
}

fn force_release_modifiers() {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
        };

        let vks = [
            VK_LWIN, VK_RWIN, VK_LCONTROL, VK_RCONTROL, VK_LMENU, VK_RMENU, VK_LSHIFT,
            VK_RSHIFT, VK_CONTROL, VK_MENU, VK_SHIFT,
        ];
        let mut inputs: Vec<INPUT> = vks
            .iter()
            .map(|&vk| INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(vk as u16),
                        wScan: 0,
                        dwFlags: KEYEVENTF_KEYUP,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            })
            .collect();
        if !inputs.is_empty() {
            unsafe {
                let _ = SendInput(&mut inputs, std::mem::size_of::<INPUT>() as i32);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_chord() {
        let keys = normalize_chord(&[VK_CONTROL, VK_LCONTROL, 0x41]);
        assert_eq!(keys, vec![VK_LCONTROL, 0x41]);
    }

    #[test]
    fn test_labels() {
        assert_eq!(vk_to_label(0x41), "A");
        assert_eq!(vk_to_label(VK_LWIN), "左 Win");
    }
}
