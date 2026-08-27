//! 对齐 Python `handle_direct_hid_report` / `VoiceShortcut` / `_perform_button_action`
//!
//! 遥控器按键 → 读取 xiaomi.json 的 button_bindings / voice_hotkey → SendInput 注入

use crate::bridges::xiaomi::connect;
use crate::bridges::xiaomi::key_log::{button_label, emit_key_phase};
use crate::bridges::xiaomi::tv_gate;
use crate::bridges::xiaomi::voice_chord_state::VoiceChordState;
use crate::bridges::xiaomi::voice_chord_sanitizer::{
    recover_chord_modifiers, recover_foreign_modifiers,
};
use crate::bridges::xiaomi::voice_inject::scan_code_for_vk;
use crate::config::manager::{ConfigManager, DeviceConfig, KeyAction};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

/// 与 Python `EXTRA_INFO = 0x584D4952` ('XMIR') 一致，供 LL hook 放行虚拟键
pub const EXTRA_INFO: usize = 0x584D_4952;

static VOICE_CHORD: Mutex<VoiceChordState> = Mutex::new(VoiceChordState::empty());
static DIRECT_MARKS: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);
static REPEAT_GEN: Mutex<Option<HashMap<String, u64>>> = Mutex::new(None);
static ACTION_SEQ: AtomicU64 = AtomicU64::new(1);

/// 语音键在 Windows 上常被译成 F5；记事本 F5=插入日期。
/// 短窗 `direct_signal_recent` 盖不住 typematic，故额外 sticky 抑制直到 F5 抬起或截止。
static VOICE_NATIVE_SUPPRESS: AtomicBool = AtomicBool::new(false);
static VOICE_NATIVE_DEADLINE: Mutex<Option<Instant>> = Mutex::new(None);
/// 对齐 Python `voice_f5_down_suppressed`：一次语音按压周期内吞掉 F5 连发/typematic
static VOICE_F5_DOWN_SUPPRESSED: AtomicBool = AtomicBool::new(false);
static INPUT_SESSION_ACTIVE: AtomicBool = AtomicBool::new(false);
static FIRMWARE_VOICE_HELD: AtomicBool = AtomicBool::new(false);
static VOICE_HOOK_APP: Mutex<Option<AppHandle>> = Mutex::new(None);

/// 输入会话（含仅电量）运行中：供 F5 固件泄漏抑制
pub fn set_input_session_active(active: bool) {
    INPUT_SESSION_ACTIVE.store(active, Ordering::Release);
    if active {
        // 新连接会话：允许再发一次 ATVV 失败 F5 提示
        reset_atvv_f5_toast_throttle();
    }
}

pub fn input_session_active() -> bool {
    INPUT_SESSION_ACTIVE.load(Ordering::Acquire)
}

/// 供 F5 固件回退路径发 UI 事件（ATVV 未订阅时语音键仍走 Windows F5）
pub fn bind_voice_hook_app(app: AppHandle) {
    *VOICE_HOOK_APP.lock() = Some(app);
}

/// ATVV 不可用时，由 special_keys 在吞掉固件 F5 后调用，补齐按键映射区的按下/抬起提示
pub fn on_firmware_voice_key(pressed: bool) {
    if connect::atvv_subscribed() {
        return;
    }
    let Some(app) = VOICE_HOOK_APP.lock().clone() else {
        return;
    };
    if pressed {
        if FIRMWARE_VOICE_HELD.swap(true, Ordering::SeqCst) {
            return;
        }
        mark_direct_signal("voice");
        mark_direct_signal("mic");
        emit_key_phase(&app, "mic", button_label("mic"), true);
        handle_voice(&app, true);
        log::debug!("XIAOMI VOICE UI down (firmware F5 fallback)");
    } else {
        if !FIRMWARE_VOICE_HELD.swap(false, Ordering::SeqCst) {
            return;
        }
        mark_direct_signal("voice");
        mark_direct_signal("mic");
        emit_key_phase(&app, "mic", button_label("mic"), false);
        handle_voice(&app, false);
        log::debug!("XIAOMI VOICE UI up (firmware F5 fallback)");
    }
}

/// 与 special_keys F5 抑制策略对齐（测试/文档）
pub const VOICE_F5_SUPPRESS_DEADLINE_MS: u64 = 3_000;

pub fn arm_voice_native_suppress() {
    VOICE_NATIVE_SUPPRESS.store(true, Ordering::Release);
    *VOICE_NATIVE_DEADLINE.lock() =
        Some(Instant::now() + Duration::from_millis(VOICE_F5_SUPPRESS_DEADLINE_MS));
}

pub fn disarm_voice_native_suppress() {
    VOICE_NATIVE_SUPPRESS.store(false, Ordering::Release);
    *VOICE_NATIVE_DEADLINE.lock() = None;
}

pub fn voice_native_suppress_active() -> bool {
    if !VOICE_NATIVE_SUPPRESS.load(Ordering::Acquire) {
        return false;
    }
    let mut g = VOICE_NATIVE_DEADLINE.lock();
    match *g {
        Some(deadline) if Instant::now() <= deadline => true,
        _ => {
            VOICE_NATIVE_SUPPRESS.store(false, Ordering::Release);
            *g = None;
            false
        }
    }
}

fn marks() -> parking_lot::MutexGuard<'static, Option<HashMap<String, Instant>>> {
    let mut g = DIRECT_MARKS.lock();
    if g.is_none() {
        *g = Some(HashMap::new());
    }
    g
}

fn repeats() -> parking_lot::MutexGuard<'static, Option<HashMap<String, u64>>> {
    let mut g = REPEAT_GEN.lock();
    if g.is_none() {
        *g = Some(HashMap::new());
    }
    g
}

/// HID DIRECT 刚触发某键：供 special hook 抑制 Windows 原键
pub fn mark_direct_signal(name: &str) {
    marks().as_mut().unwrap().insert(name.to_string(), Instant::now());
    // 别名同步标记，便于 LL hook 用 Python 键名匹配
    for alt in binding_aliases(name) {
        if *alt != name {
            marks()
                .as_mut()
                .unwrap()
                .insert((*alt).to_string(), Instant::now());
        }
    }
    // 语音键原生多为 F5：提前 sticky 抑制，避免 120ms 后 typematic 漏进记事本
    if name == "mic" || name == "voice" || binding_aliases(name).iter().any(|a| *a == "mic") {
        arm_voice_native_suppress();
    }
}

pub fn direct_signal_recent(name: &str, window: Duration) -> bool {
    let g = marks();
    let Some(m) = g.as_ref() else {
        return false;
    };
    if m.get(name).map(|t| t.elapsed() <= window).unwrap_or(false) {
        return true;
    }
    for alt in binding_aliases(name) {
        if m.get(*alt).map(|t| t.elapsed() <= window).unwrap_or(false) {
            return true;
        }
    }
    false
}

/// 对齐 Python `_wait_for_direct_signal`：F5 可能比 ATVV 0x04 先到
fn wait_for_direct_signal(name: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if direct_signal_recent(name, Duration::from_millis(400)) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    direct_signal_recent(name, Duration::from_millis(400))
}

/// 对齐 Python `_should_suppress_voice_f5`：关联固件 F5 与语音键，避免记事本刷日期时间
pub fn should_suppress_voice_f5(down: bool, up: bool) -> bool {
    if !down && !up {
        return false;
    }
    if up {
        return VOICE_F5_DOWN_SUPPRESSED.swap(false, Ordering::AcqRel);
    }
    if VOICE_F5_DOWN_SUPPRESSED.load(Ordering::Acquire) {
        return true;
    }
    // 明确注入语音和弦期间：无条件吞 F5（遥控器原生 F5 会混入 Ctrl+Win，微信「按住说话」不识别）
    if voice_native_suppress_active() {
        VOICE_F5_DOWN_SUPPRESSED.store(true, Ordering::Release);
        return true;
    }
    if !input_session_active() {
        return false;
    }
    if direct_signal_recent("voice", Duration::from_millis(300))
        || direct_signal_recent("mic", Duration::from_millis(300))
    {
        VOICE_F5_DOWN_SUPPRESSED.store(true, Ordering::Release);
        return true;
    }
    if wait_for_direct_signal("mic", Duration::from_millis(80)) {
        VOICE_F5_DOWN_SUPPRESSED.store(true, Ordering::Release);
        arm_voice_native_suppress();
        return true;
    }
    // Policy B：关联不上则放行（键盘 F5 可用）。ATVV 挂掉时由 toast 提示，不再会话级全吞。
    false
}

static ATVV_F5_TOAST_LAST: Mutex<Option<Instant>> = Mutex::new(None);
const ATVV_F5_TOAST_GAP: Duration = Duration::from_secs(60);

fn reset_atvv_f5_toast_throttle() {
    *ATVV_F5_TOAST_LAST.lock() = None;
}

/// N1：会话中且 ATVV 未订阅时，未关联的 F5（多为遥控语音键固件泄漏）→ 系统通知
pub fn on_uncorrelated_f5_down() {
    if !input_session_active() || connect::atvv_subscribed() {
        return;
    }
    {
        let mut last = ATVV_F5_TOAST_LAST.lock();
        if let Some(t) = *last {
            if t.elapsed() < ATVV_F5_TOAST_GAP {
                return;
            }
        }
        *last = Some(Instant::now());
    }
    let Some(app) = VOICE_HOOK_APP.lock().clone() else {
        return;
    };
    use tauri_plugin_notification::NotificationExt;
    log::info!("XIAOMI VOICE F5 toast (atvv down; not suppressed)");
    if let Err(e) = app
        .notification()
        .builder()
        .title("遥控器 ATVV 未连接")
        .body(
            "语音键可能触发系统 F5（如记事本插入日期）。请打开本软件，在小米设置中点击「修复 ATVV 连接」。",
        )
        .show()
    {
        log::warn!("ATVV F5 notification failed: {e}");
    }
}

/// Python / 旧版 UI 键名互认
fn binding_aliases(id: &str) -> &'static [&'static str] {
    match id {
        "up" | "dpad_up" => &["up", "dpad_up"],
        "down" | "dpad_down" => &["down", "dpad_down"],
        "left" | "dpad_left" => &["left", "dpad_left"],
        "right" | "dpad_right" => &["right", "dpad_right"],
        "mic" | "voice" => &["mic", "voice"],
        "volume_mute" | "mute" => &["volume_mute", "mute"],
        _ => &[],
    }
}

fn lookup_action<'a>(config: &'a DeviceConfig, button_id: &str) -> Option<&'a KeyAction> {
    if let Some(a) = config.button_bindings.get(button_id) {
        return Some(a);
    }
    for alt in binding_aliases(button_id) {
        if let Some(a) = config.button_bindings.get(*alt) {
            return Some(a);
        }
    }
    None
}

fn load_xiaomi_config(app: &AppHandle) -> Option<DeviceConfig> {
    let mgr = app.try_state::<ConfigManager>()?;
    mgr.get_device_config("xiaomi").ok()
}

/// 按下遥控器物理键后的统一处理
pub fn on_remote_button(app: &AppHandle, button_id: &str, pressed: bool) {
    if button_id == "voice" || button_id == "mic" {
        mark_direct_signal("voice");
        mark_direct_signal("mic");
        handle_voice(app, pressed);
        return;
    }

    if button_id == "tv" && pressed && !tv_gate::is_ready() {
        log::info!("XIAOMI MAPPING tv blocked_by_gate");
        return;
    }

    if !pressed {
        mark_direct_signal(button_id);
        cancel_repeat(button_id);
        for alt in binding_aliases(button_id) {
            cancel_repeat(alt);
        }
        return;
    }

    let Some(config) = load_xiaomi_config(app) else {
        log::warn!("XIAOMI MAPPING no config manager");
        return;
    };

    let triggered = perform_button_action(&config, button_id);
    log::debug!("XIAOMI MAPPING key={button_id} mapped={triggered} pressed=true");

    if triggered {
        mark_direct_signal(button_id);
        match button_id {
            "back" => start_hold_repeat(
                app.clone(),
                button_id.to_string(),
                Duration::from_millis(280),
                Duration::from_millis(40),
            ),
            "volume_up" | "volume_down" => start_hold_repeat(
                app.clone(),
                button_id.to_string(),
                Duration::from_millis(400),
                Duration::from_millis(120),
            ),
            "up" | "down" | "left" | "right" | "dpad_up" | "dpad_down" | "dpad_left"
            | "dpad_right" => start_hold_repeat(
                app.clone(),
                button_id.to_string(),
                Duration::from_millis(280),
                Duration::from_millis(40),
            ),
            _ => {}
        }
    }
}

fn cancel_repeat(button_id: &str) {
    let mut map = repeats();
    let gen = map
        .as_mut()
        .unwrap()
        .entry(button_id.to_string())
        .or_insert(0);
    *gen = gen.wrapping_add(1);
}

fn start_hold_repeat(app: AppHandle, button_id: String, delay: Duration, interval: Duration) {
    let gen = {
        let mut map = repeats();
        let e = map.as_mut().unwrap().entry(button_id.clone()).or_insert(0);
        *e = e.wrapping_add(1);
        *e
    };
    std::thread::Builder::new()
        .name(format!("xiaomi-repeat-{button_id}"))
        .spawn(move || {
            std::thread::sleep(delay);
            loop {
                {
                    let map = repeats();
                    if map.as_ref().and_then(|m| m.get(&button_id)).copied() != Some(gen) {
                        break;
                    }
                }
                if button_id == "tv" && !tv_gate::is_ready() {
                    break;
                }
                if let Some(config) = load_xiaomi_config(&app) {
                    let _ = perform_button_action(&config, &button_id);
                }
                std::thread::sleep(interval);
            }
        })
        .ok();
}

fn perform_button_action(config: &DeviceConfig, button_id: &str) -> bool {
    let Some(action) = lookup_action(config, button_id) else {
        return false;
    };
    match action {
        KeyAction::None => false,
        KeyAction::SingleKey(vk) => {
            tap_vks(&[*vk], 20);
            true
        }
        KeyAction::ComboKey(vks) if !vks.is_empty() => {
            tap_vks(vks, 70);
            true
        }
        KeyAction::ComboKey(_) => false,
        KeyAction::TextInput(text) => {
            tap_unicode_text(text);
            true
        }
        KeyAction::LaunchApp(path) => {
            let _ = std::process::Command::new(path).spawn();
            true
        }
    }
}

fn handle_voice(app: &AppHandle, pressed: bool) {
    let Some(config) = load_xiaomi_config(app) else {
        return;
    };
    if !config.voice_shortcut_enabled {
        log::info!("XIAOMI VOICE shortcut disabled");
        return;
    }
    let vks = resolve_voice_hotkey(&config);
    if vks.is_empty() {
        log::warn!("XIAOMI VOICE shortcut empty");
        return;
    }
    // 纯 hold：按下 → 映射键 DOWN，抬起 → UP（单击=热键按一次，按住=热键持续按住）
    if pressed {
        // 语音唤起前把本进程 LL 钩子顶到链头（装到微信钩子之前），
        // 才能替微信吞掉遥控器原生 F5，避免 Ctrl+Win+F5 使微信「按住说话」不识别
        crate::bridges::xiaomi::special_keys::bump_hook_to_front();
        // 注入语音和弦期间吞掉遥控器原生 F5
        arm_voice_native_suppress();
        let pressed_ok = {
            let mut state = VOICE_CHORD.lock();
            state.press_with(&vks, inject_voice_chord)
        };
        if pressed_ok {
            log::info!("XIAOMI VOICE SHORTCUT DOWN vks={vks:?}");
        } else {
            log::warn!("XIAOMI VOICE SHORTCUT DOWN failed vks={vks:?}");
        }
    } else {
        force_release_voice_shortcut("remote_up");
    }
}

fn force_release_voice_shortcut(reason: &str) -> bool {
    let mut state = VOICE_CHORD.lock();
    let Some((keys, released)) = state.release_with(inject_voice_chord) else {
        return false;
    };
    if released {
        log::info!("XIAOMI VOICE SHORTCUT UP reason={reason} vks={keys:?}");
    } else {
        log::error!("XIAOMI VOICE SHORTCUT UP failed reason={reason} vks={keys:?}");
    }
    released
}

/// ATVV opcode 路径调用（对齐 VoiceShortcut.press/release/tap）
pub fn voice_from_atvv(app: &AppHandle, opcode: u8) {
    match opcode {
        0x04 => on_remote_button(app, "mic", true),
        0x00 => on_remote_button(app, "mic", false),
        _ => {}
    }
}

fn resolve_voice_hotkey(config: &DeviceConfig) -> Vec<u16> {
    // 对齐 Python voice_hotkey_from_configs：界面上的 mic 按键映射优先于 voice_hotkey 字段
    if let Some(action) = config.button_bindings.get("mic") {
        if let Some(vks) = action_to_vks(action) {
            return vks;
        }
    }
    if let Some(action) = config.button_bindings.get("voice") {
        if let Some(vks) = action_to_vks(action) {
            return vks;
        }
    }
    if let Some(keys) = &config.voice_hotkey {
        let mut out = Vec::new();
        for k in keys {
            if let Some(vk) = name_to_vk(k) {
                out.push(vk);
            }
        }
        if !out.is_empty() {
            return out;
        }
    }
    vec![0xA5] // 默认右 Alt
}

fn action_to_vks(action: &KeyAction) -> Option<Vec<u16>> {
    match action {
        KeyAction::SingleKey(vk) => Some(vec![*vk]),
        KeyAction::ComboKey(vks) if !vks.is_empty() => Some(vks.clone()),
        _ => None,
    }
}

fn vks_to_hotkey_names(vks: &[u16]) -> Vec<String> {
    vks.iter()
        .map(|&vk| match vk {
            0xA2 => "leftctrl".into(),
            0xA3 => "rightctrl".into(),
            0x11 => "ctrl".into(),
            0xA0 => "leftshift".into(),
            0xA1 => "rightshift".into(),
            0x10 => "shift".into(),
            0xA4 => "leftalt".into(),
            0xA5 => "rightalt".into(),
            0x12 => "alt".into(),
            0x5B => "leftwin".into(),
            0x5C => "rightwin".into(),
            0x20 => "space".into(),
            0x0D => "enter".into(),
            0x08 => "backspace".into(),
            0x1B => "esc".into(),
            other if (0x41..=0x5A).contains(&other) => {
                ((other as u8) as char).to_ascii_lowercase().to_string()
            }
            other if (0x30..=0x39).contains(&other) => {
                char::from(b'0' + (other - 0x30) as u8).to_string()
            }
            other if (0x70..=0x7B).contains(&other) => format!("f{}", other - 0x6F),
            other => format!("vk_{other:02x}"),
        })
        .collect()
}

/// 保存前：mic 映射同步到 voice_hotkey / voice 别名（对齐 Python 保存逻辑）
pub fn sync_voice_from_mic_binding(config: &mut DeviceConfig) {
    let mic = config
        .button_bindings
        .get("mic")
        .cloned()
        .or_else(|| config.button_bindings.get("voice").cloned());
    let Some(action) = mic else {
        return;
    };
    let Some(vks) = action_to_vks(&action) else {
        return;
    };
    config.voice_hotkey = Some(vks_to_hotkey_names(&vks));
    config.button_bindings.insert("mic".into(), action.clone());
    config.button_bindings.insert("voice".into(), action);
}

fn name_to_vk(name: &str) -> Option<u16> {
    let n = name.trim().to_ascii_lowercase().replace(' ', "");
    match n.as_str() {
        "backspace" => Some(0x08),
        "tab" => Some(0x09),
        "enter" | "return" => Some(0x0D),
        "shift" => Some(0x10),
        "ctrl" | "control" => Some(0x11),
        "alt" => Some(0x12),
        "esc" | "escape" => Some(0x1B),
        "space" => Some(0x20),
        "left" => Some(0x25),
        "up" => Some(0x26),
        "right" => Some(0x27),
        "down" => Some(0x28),
        "home" => Some(0x24),
        "f10" => Some(0x79),
        "d" => Some(0x44),
        "win" | "leftwin" | "lwin" => Some(0x5B),
        "rightwin" | "rwin" => Some(0x5C),
        "leftshift" => Some(0xA0),
        "rightshift" => Some(0xA1),
        "leftctrl" => Some(0xA2),
        "rightctrl" => Some(0xA3),
        "leftalt" => Some(0xA4),
        "rightalt" | "ralt" | "rmenu" => Some(0xA5),
        "volume_mute" | "volumemute" => Some(0xAD),
        "volume_down" | "volumedown" => Some(0xAE),
        "volume_up" | "volumeup" => Some(0xAF),
        other if other.len() == 1 => {
            let c = other.chars().next()?.to_ascii_uppercase();
            if c.is_ascii_alphanumeric() {
                Some(c as u16)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn is_extended(vk: u16) -> bool {
    matches!(
        vk,
        0x21 | 0x22 | 0x23 | 0x24 | 0x25 | 0x26 | 0x27 | 0x28 | 0x2C | 0x2D | 0x2E | 0x5B
            | 0x5C | 0x5D | 0xA3 | 0xA5 | 0xAD | 0xAE | 0xAF | 0xB0 | 0xB1 | 0xB2 | 0xB3
            | 0xB7
    )
}

fn is_alt_modifier(vk: u16) -> bool {
    matches!(vk, 0x12 | 0xA4 | 0xA5) // VK_MENU, VK_LMENU, VK_RMENU
}

fn has_alt_modifier(vks: &[u16]) -> bool {
    vks.iter().any(|&vk| is_alt_modifier(vk))
}

pub fn tap_vks(vks: &[u16], hold_ms: u64) {
    // 音量/静音：优先走 SendInput 的 VK_VOLUME_*（系统音量最稳）
    // 计算器等其它键：先试 WinUHid（含 consumer），再回落 SendInput
    let is_volume = vks.len() == 1 && matches!(vks[0], 0xAD | 0xAE | 0xAF);
    if !is_volume {
        if crate::bridges::xiaomi::hid_injector::tap_vks(vks, hold_ms) {
            let _ = ACTION_SEQ.fetch_add(1, Ordering::Relaxed);
            return;
        }
    }

    // Alt 组合键（如 Alt+Space, Alt+S）：使用 SendMessage(WM_KEYDOWN) 注入，
    // 避免 SendInput 触发 WM_SYSKEYDOWN → 系统菜单/全局热键
    if has_alt_modifier(vks) {
        inject_alt_chord_via_message(vks, hold_ms);
        let _ = ACTION_SEQ.fetch_add(1, Ordering::Relaxed);
        return;
    }

    key_chord(vks, false);
    std::thread::sleep(Duration::from_millis(hold_ms.max(1)));
    key_chord(vks, true);
    let _ = ACTION_SEQ.fetch_add(1, Ordering::Relaxed);
    log::debug!("XIAOMI MAPPING inject SendInput vks={vks:?} hold_ms={hold_ms} volume={is_volume}");
}

/// 通过 SendMessage(WM_KEYDOWN/WM_KEYUP) 注入 Alt 组合键。
///
/// 与 SendInput 不同，SendMessage 投递的是 WM_KEYDOWN（非 WM_SYSKEYDOWN），
/// Windows 不会将其解释为系统键，因此 Alt+Space 不会弹出系统菜单、
/// Alt+S 不会触发全局热键。
#[cfg(target_os = "windows")]
fn inject_alt_chord_via_message(vks: &[u16], hold_ms: u64) {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, SendMessageTimeoutW, SMTO_NORMAL, WM_KEYDOWN, WM_KEYUP,
    };
    use windows::Win32::Foundation::HWND;

    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd == HWND(std::ptr::null_mut()) {
        // 无前台窗口，回退 SendInput
        log::warn!("XIAOMI MAPPING alt_chord: no foreground window, fallback SendInput");
        crate::bridges::xiaomi::special_keys::arm_alt_chord();
        key_chord(vks, false);
        std::thread::sleep(Duration::from_millis(hold_ms.max(1)));
        key_chord(vks, true);
        crate::bridges::xiaomi::special_keys::disarm_alt_chord();
        return;
    }

    // 武装特殊键钩子：若回调仍触发则抑制（双保险）
    crate::bridges::xiaomi::special_keys::arm_alt_chord();

    // 按下：正序发送 WM_KEYDOWN
    for &vk in vks {
        let lparam = make_key_lparam(vk, false);
        unsafe {
            let _ = SendMessageTimeoutW(
                hwnd,
                WM_KEYDOWN,
                windows::Win32::Foundation::WPARAM(vk as usize),
                windows::Win32::Foundation::LPARAM(lparam as isize),
                SMTO_NORMAL,
                500,
                None,
            );
        }
    }

    std::thread::sleep(Duration::from_millis(hold_ms.max(1)));

    // 释放：逆序发送 WM_KEYUP
    for &vk in vks.iter().rev() {
        let lparam = make_key_lparam(vk, true);
        unsafe {
            let _ = SendMessageTimeoutW(
                hwnd,
                WM_KEYUP,
                windows::Win32::Foundation::WPARAM(vk as usize),
                windows::Win32::Foundation::LPARAM(lparam as isize),
                SMTO_NORMAL,
                500,
                None,
            );
        }
    }

    crate::bridges::xiaomi::special_keys::disarm_alt_chord();
    log::debug!(
        "XIAOMI MAPPING inject alt_chord via SendMessage vks={vks:?} hold_ms={hold_ms}"
    );
}

/// 构造 WM_KEYDOWN/WM_KEYUP 的 lParam
#[cfg(target_os = "windows")]
fn make_key_lparam(vk: u16, key_up: bool) -> u32 {
    use windows::Win32::UI::Input::KeyboardAndMouse::{MapVirtualKeyW, MAPVK_VK_TO_VSC};

    let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as u32;
    let mut lparam: u32 = (scan & 0xFF) << 16;

    // bit 24: extended key flag
    if is_extended(vk) {
        lparam |= 1 << 24;
    }

    if key_up {
        // bit 30: previous key state (was down)
        // bit 31: transition state (being released)
        lparam |= (1 << 30) | (1 << 31);
    }

    // repeat count = 1 (bits 0-15 保持 1)
    lparam |= 1;

    lparam
}

#[cfg(not(target_os = "windows"))]
fn inject_alt_chord_via_message(vks: &[u16], hold_ms: u64) {
    // 非 Windows 回退
    key_chord(vks, false);
    std::thread::sleep(Duration::from_millis(hold_ms.max(1)));
    key_chord(vks, true);
}

fn tap_unicode_text(text: &str) {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
            KEYEVENTF_UNICODE, VIRTUAL_KEY,
        };
        for ch in text.encode_utf16() {
            let inputs = [
                INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT {
                            wVk: VIRTUAL_KEY(0),
                            wScan: ch,
                            dwFlags: KEYEVENTF_UNICODE,
                            time: 0,
                            dwExtraInfo: EXTRA_INFO,
                        },
                    },
                },
                INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT {
                            wVk: VIRTUAL_KEY(0),
                            wScan: ch,
                            dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                            time: 0,
                            dwExtraInfo: EXTRA_INFO,
                        },
                    },
                },
            ];
            unsafe {
                let _ = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = text;
    }
}

fn key_chord(vks: &[u16], key_up: bool) {
    // 非语音映射：可走 WinUHid；失败再 SendInput(extra=0，避免截图 Esc 被丢弃)
    #[cfg(target_os = "windows")]
    {
        if !key_up {
            if crate::bridges::xiaomi::hid_injector::press(vks).is_ok() {
                return;
            }
        } else if crate::bridges::xiaomi::hid_injector::release(vks).is_ok() {
            return;
        }
        let _ = key_chord_send_input_with_extra(vks, key_up, 0);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (vks, key_up);
    }
}

/// 注入前松开不在和弦内、却仍按下的修饰键，避免粘键污染组合。
#[cfg(target_os = "windows")]
fn voice_sanitize_keyup(vks: &[u16]) -> bool {
    key_chord_send_input_with_extra(vks, true, EXTRA_INFO)
}

/// 注入前松开不在和弦内、却仍按下的修饰键（SendInput 仅 KEYUP，非唤醒）。
#[cfg(not(target_os = "windows"))]
fn voice_sanitize_keyup(_vks: &[u16]) -> bool {
    false
}

/// 语音和弦注入 — **必须** WinUHid（虚拟硬件 HID）。
/// SendInput 会被豆包/千问等过滤，不能作为语音唤醒修复方案；仅 UP 后 sanitizer 清键。
fn inject_voice_chord(vks: &[u16], key_up: bool) -> bool {
    if vks.is_empty() {
        return false;
    }
    #[cfg(target_os = "windows")]
    {
        let vks = crate::bridges::xiaomi::voice_inject::normalize_voice_chord_vks(vks);
        if !key_up {
            recover_foreign_modifiers(&vks, voice_sanitize_keyup);
        }

        if crate::bridges::xiaomi::hid_injector::is_available() {
            let hid_ok = if !key_up {
                crate::bridges::xiaomi::hid_injector::press_single(&vks).is_ok()
            } else {
                match crate::bridges::xiaomi::hid_injector::release_single(&vks) {
                    Ok(()) => true,
                    Err(e) => {
                        log::error!("XIAOMI VOICE WinUHid release failed: {e}");
                        let _ = crate::bridges::xiaomi::hid_injector::release_all();
                        false
                    }
                }
            };
            if key_up {
                let cleared = recover_chord_modifiers(&vks, voice_sanitize_keyup);
                if !hid_ok && cleared > 0 {
                    log::warn!(
                        "XIAOMI VOICE release recovered via sanitizer cleared={cleared} vks={vks:?}"
                    );
                    return true;
                }
            }
            if hid_ok {
                log::info!("XIAOMI VOICE inject via WinUHid key_up={key_up} vks={vks:?}");
                return true;
            }
            if !key_up {
                log::error!("XIAOMI VOICE WinUHid inject failed key_up={key_up} vks={vks:?}");
            }
            return false;
        }
        if !key_up {
            log::error!(
                "XIAOMI VOICE inject BLOCKED: WinUHid unavailable — 请点「修复虚拟键盘」安装驱动；SendInput 无法稳定唤醒豆包/千问 vks={vks:?}"
            );
        }
        false
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (vks, key_up);
        false
    }
}

#[cfg(target_os = "windows")]
fn key_chord_send_input_with_extra(vks: &[u16], key_up: bool, extra_info: usize) -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
        KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, VIRTUAL_KEY,
    };

    let iter: Box<dyn Iterator<Item = &u16>> = if key_up {
        Box::new(vks.iter().rev())
    } else {
        Box::new(vks.iter())
    };

    // dwExtraInfo：语音 Alt 对齐 Nexus 使用 EXTRA_INFO；普通映射保持 0
    //（截图 overlay 会丢弃带未知 extraInfo 的 Esc）。
    let mut inputs: Vec<INPUT> = Vec::with_capacity(vks.len());
    for &vk in iter {
        let mapped = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as u16;
        let scan = scan_code_for_vk(vk, mapped);
        let mut flags = if is_extended(vk) {
            KEYEVENTF_EXTENDEDKEY
        } else {
            Default::default()
        };
        if key_up {
            flags |= KEYEVENTF_KEYUP;
        }
        inputs.push(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: scan,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: extra_info,
                },
            },
        });
    }
    if inputs.is_empty() {
        return false;
    }
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    let ok = sent as usize == inputs.len();
    if !ok {
        log::warn!(
            "XIAOMI MAPPING SendInput incomplete sent={sent} expected={} key_up={key_up} vks={vks:?}",
            inputs.len()
        );
    }
    ok
}
