//! T1 运行时 — Consumer HID → 映射注入 + 语音（Hold 按住 / Toggle 点按）

use crate::bridges::t1::consumer_raw_input::{device_matches, ConsumerRawEvent, ConsumerRawInput};
use crate::bridges::t1::inject::{names_to_vks, press_vks, release_vks, tap_single_vk, tap_vks};
use crate::bridges::t1::key_diag;
use crate::bridges::t1::mapping::{
    build_event_to_button, default_event_aliases, resolve_button, VoiceSessionState,
};
use crate::bridges::t1::native_mic;
use crate::bridges::t1::native_suppress;
use crate::bridges::shared::hid_injector;
use crate::bridges::{BridgeState, BridgeStatus, BridgeType};
use crate::config::manager::{ConfigManager, DeviceConfig, KeyAction, TriggerMode};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

const DEVICE_MATCH: &[&str] = &["VID_1915&PID_1025"];
const MIC_LABEL: &str = "Mic Device";

/// HID 与随后冒出的 kbd:VK_A6/AF… 去重，避免双注入
static LAST_INJECT: LazyLock<Mutex<Option<(String, Instant)>>> =
    LazyLock::new(|| Mutex::new(None));

/// LL 闸门兜底注入所需上下文（菜单键等 Raw Input 按下常丢失）
#[derive(Clone)]
struct LlGateCtx {
    app: AppHandle,
    voice_state: Arc<Mutex<VoiceSessionState>>,
}

static LL_GATE_CTX: LazyLock<Mutex<Option<LlGateCtx>>> = LazyLock::new(|| Mutex::new(None));


/// Hold 闩锁期间保持的语音快捷键（其它按键 release_all 后需恢复，否则豆包会停录）
static VOICE_LATCH_VKS: LazyLock<Mutex<Vec<u16>>> = LazyLock::new(|| Mutex::new(Vec::new()));
/// 闩锁开始时间（对齐 Python `audio_max_hold_ms=300000`）
static VOICE_LATCH_STARTED: LazyLock<Mutex<Option<Instant>>> =
    LazyLock::new(|| Mutex::new(None));
static VOICE_WATCHDOG_RUNNING: AtomicBool = AtomicBool::new(false);

/// 与 Python T1 `audio_max_hold_ms` 一致：最长 5 分钟后强制收口
const VOICE_MAX_HOLD: Duration = Duration::from_millis(300_000);
/// WinUHid / 系统有时会静默丢掉「一直按下的修饰键」→ 约十余秒后豆包停录；周期性重按保活
const VOICE_HEARTBEAT: Duration = Duration::from_millis(1_500);

pub fn voice_latch_active() -> bool {
    !VOICE_LATCH_VKS.lock().is_empty()
}

fn vk_async_down(vk: u16) -> bool {
    #[cfg(windows)]
    {
        use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
        unsafe { (GetAsyncKeyState(vk as i32) as u16) & 0x8000 != 0 }
    }
    #[cfg(not(windows))]
    {
        let _ = vk;
        true
    }
}

fn ensure_voice_watchdog(app: AppHandle) {
    if VOICE_WATCHDOG_RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    thread::spawn(move || {
        log::info!(
            "T1 voice watchdog start heartbeat={}ms max_hold={}ms",
            VOICE_HEARTBEAT.as_millis(),
            VOICE_MAX_HOLD.as_millis()
        );
        while VOICE_WATCHDOG_RUNNING.load(Ordering::SeqCst) {
            thread::sleep(VOICE_HEARTBEAT);
            let held = VOICE_LATCH_VKS.lock().clone();
            if held.is_empty() {
                continue;
            }
            if let Some(started) = *VOICE_LATCH_STARTED.lock() {
                if started.elapsed() >= VOICE_MAX_HOLD {
                    log::info!("T1 voice max hold reached (300000ms) — force end");
                    force_end_voice_latch("audio_max_hold_ms=300000");
                    let _ = app.emit(
                        "t1-key",
                        serde_json::json!({
                            "id": "voice",
                            "message": "语音已达 5 分钟上限，已自动结束（对齐 Python audio_max_hold_ms）",
                        }),
                    );
                    continue;
                }
            }
            let dropped: Vec<u16> = held
                .iter()
                .copied()
                .filter(|vk| !vk_async_down(*vk))
                .collect();
            if dropped.is_empty() {
                continue;
            }
            log::warn!(
                "T1 voice latch OS-dropped vks={dropped:?} — re-pressing full chord {held:?}"
            );
            native_suppress::allow_pass_vks(&held);
            // 只允许 press_single；禁止 press() 分步（会先露出 Win/Ctrl）
            match hid_injector::press_single(&held) {
                Ok(()) => {
                    let _ = app.emit(
                        "t1-key",
                        serde_json::json!({
                            "id": "voice",
                            "message": format!(
                                "语音快捷键曾被系统松开，已恢复按下 {:?}",
                                held
                            ),
                        }),
                    );
                }
                Err(e) => {
                    log::warn!("T1 voice heartbeat press_single failed: {e} — not using stagger press");
                }
            }
        }
        log::info!("T1 voice watchdog stopped");
    });
}

/// 强制结束语音闩锁并松开 WinUHid（含实体键盘误触时的逃生）。
pub fn force_end_voice_latch(reason: &str) {
    let had = {
        let mut g = VOICE_LATCH_VKS.lock();
        let had = !g.is_empty();
        g.clear();
        had
    };
    *VOICE_LATCH_STARTED.lock() = None;
    if let Some(ctx) = LL_GATE_CTX.lock().as_ref() {
        *ctx.voice_state.lock() = VoiceSessionState::Idle;
    }
    crate::bridges::shared::host_hooks::set_virtual_hid_chord_held(None);
    crate::bridges::t1_usb::mic_cable::stop();
    crate::bridges::t1::inject::panic_clear_all_modifiers(&format!("force_end_latch:{reason}"));
    native_suppress::release_voice_browser_search();
    if had {
        log::info!("T1 voice latch force-ended ({reason})");
    }
}

/// 实体键盘非修饰键：结束闩锁，避免「一直按着 Alt」污染真实键盘。
pub fn maybe_end_voice_latch_on_physical_key(vk: u16) {
    if !voice_latch_active() || vk == 0 {
        return;
    }
    if matches!(vk, 0x12 | 0xA4 | 0xA5) {
        return;
    }
    if native_suppress::is_temporarily_allowed(vk) {
        return;
    }
    let is_modifier = matches!(
        vk,
        0x10 | 0x11 | 0x12 | 0x14 | 0x5B | 0x5C | 0xA0..=0xA5
    );
    if is_modifier {
        return;
    }
    force_end_voice_latch(&format!("physical vk=0x{vk:02X}"));
}


fn should_skip_duplicate_inject(button_id: &str) -> bool {
    let mut guard = LAST_INJECT.lock();
    let now = Instant::now();
    if let Some((id, at)) = guard.as_ref() {
        if id == button_id && now.duration_since(*at) < Duration::from_millis(120) {
            return true;
        }
    }
    *guard = Some((button_id.to_string(), now));
    false
}

/// LL / RegisterHotKey 吞掉闸门键后补映射（菜单/主页/删除/静音/音量/重映射方向·OK）。
pub fn on_ll_gate_keydown(vk: u16) {
    if !native_suppress::is_gate_vk(vk) {
        return;
    }
    // 进入补映射路径即代表来源已由调用方裁决，清理来源暂挂，避免 Raw 再回放。
    let _ = native_suppress::consume_source_pending(vk);
    let Some(ctx) = LL_GATE_CTX.lock().clone() else {
        return;
    };
    let event_id = format!("kbd:VK_{vk:02X}");
    let event_to_button = build_event_to_button(&default_event_aliases());
    let button_id = resolve_button(&event_id, &event_to_button);
    let config = ctx
        .app
        .try_state::<ConfigManager>()
        .and_then(|mgr| mgr.get_device_config("t1_usb").ok());
    let target_vks = match (&button_id, &config) {
        (Some(bid), Some(cfg)) => binding_vks(cfg, bid),
        _ => Vec::new(),
    };
    let msg = key_diag::format_line(
        &event_id,
        true,
        button_id.as_deref(),
        &target_vks,
        None,
        "LL-gate",
        Some("闸门吞键后补发映射"),
    );
    key_diag::emit_usb(
        &ctx.app,
        "ll",
        button_id.as_deref(),
        None,
        Some(&event_id),
        Some(true),
        &target_vks,
        &msg,
    );
    let Some(button_id) = button_id else {
        return;
    };
    if button_id == "voice" {
        return;
    }
    if button_id == "home" {
        native_suppress::on_ac_home_hid_seen();
    }
    if button_id == "menu" {
        native_suppress::arm_menu_apps_key();
    }
    native_suppress::arm_for_button(&button_id, Some(vk), &target_vks);
    if native_suppress::is_passthrough_binding(&button_id, Some(vk), &target_vks) {
        key_diag::emit_usb(
            &ctx.app,
            "ll",
            Some(&button_id),
            None,
            Some(&event_id),
            Some(true),
            &target_vks,
            &format!("LL 同键/未绑定透传 {button_id}"),
        );
        return;
    }
    if should_skip_duplicate_inject(&button_id)
        || native_suppress::should_skip_gate_inject(&button_id)
    {
        key_diag::emit_usb(
            &ctx.app,
            "ll",
            Some(&button_id),
            None,
            Some(&event_id),
            Some(true),
            &target_vks,
            &format!("LL 去重跳过 {button_id}"),
        );
        return;
    }
    let Some(config) = config else {
        key_diag::emit_usb(
            &ctx.app,
            "ll",
            Some(&button_id),
            None,
            Some(&event_id),
            Some(true),
            &[],
            "LL 闸门：t1 配置不可用",
        );
        return;
    };
    let voice_state = Arc::clone(&ctx.voice_state);
    let app_emit = ctx.app.clone();
    let button = button_id.to_string();
    thread::spawn(move || {
        handle_button(&button, &config, &voice_state, &app_emit);
    });
}

pub struct T1Runtime {
    pub stop: AtomicBool,
    pub running: AtomicBool,
    inner: Mutex<Option<RuntimeInner>>,
}

struct RuntimeInner {
    raw: ConsumerRawInput,
}

impl Default for T1Runtime {
    fn default() -> Self {
        Self::new()
    }
}

impl T1Runtime {
    pub fn new() -> Self {
        Self {
            stop: AtomicBool::new(false),
            running: AtomicBool::new(false),
            inner: Mutex::new(None),
        }
    }

    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(inner) = self.inner.lock().as_mut() {
            inner.raw.stop();
        }
    }

    pub fn clear_stop(&self) {
        self.stop.store(false, Ordering::SeqCst);
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

/// 由 IPC 调用：启动 T1 Raw Input 监听
pub fn start_t1_bridge(
    app: AppHandle,
    state: &BridgeState,
    config_manager: &ConfigManager,
) -> Result<(), String> {
    let runtime = app
        .try_state::<Arc<T1Runtime>>()
        .ok_or_else(|| "T1Runtime 未注册".to_string())?;
    if runtime.running.load(Ordering::SeqCst) {
        return Err("T1 桥接已在运行".into());
    }
    runtime.clear_stop();
    runtime.running.store(true, Ordering::SeqCst);

    let _config = config_manager.get_device_config("t1_usb")?;

    crate::bridges::shared::host_hooks::ensure_hook_for_capture();
    native_suppress::arm_reason(native_suppress::SwallowReason::UsbBridge);
    sync_media_gates(&_config);

    if hid_injector::is_available() {
        log::info!("T1 WinUHid ready for key inject");
    } else {
        log::warn!("T1 WinUHid unavailable — mapped keys fall back to SendInput");
    }

    // 把 Mic Device 选择暂停关掉即可；不要在「仅连接」时抢系统默认麦。
    // 否则小米（听 CABLE）与 T1 USB（听 Mic Device）同时在线时，默认麦被钉死在 Mic Device，
    // 小米唤醒输入法后听空线，输入法会很快自动结束语音。
    native_mic::disable_t1_usb_selective_suspend();
    match native_mic::start_mic_keepalive(MIC_LABEL) {
        Ok(()) => log::info!("T1 mic session noted at USB bridge start (no capture stream)"),
        Err(e) => log::warn!("T1 mic session note failed: {e}"),
    }

    let mut raw = ConsumerRawInput::new(true);
    let match_tokens: Vec<String> = DEVICE_MATCH.iter().map(|s| (*s).to_string()).collect();
    let event_to_button = build_event_to_button(&default_event_aliases());
    let voice_state = Arc::new(Mutex::new(VoiceSessionState::Idle));
    let saw_device = Arc::new(AtomicBool::new(false));

    *LL_GATE_CTX.lock() = Some(LlGateCtx {
        app: app.clone(),
        voice_state: Arc::clone(&voice_state),
    });

    let voice_state_cb = Arc::clone(&voice_state);
    let saw_device_cb = Arc::clone(&saw_device);
    let app_for_cb = app.clone();
    let match_tokens_cb = match_tokens;
    let runtime_flag = Arc::clone(&runtime);

    raw.start(move |ev: ConsumerRawEvent| {
        if crate::bridges::shared::shortcut_capture::is_swallow_active() {
            return;
        }
        if !device_matches(&ev.device_name, &match_tokens_cb) {
            if let Some(vk) = parse_kbd_vk(&ev.event_id) {
                native_suppress::on_foreign_keyboard(vk, ev.pressed);
                // 真实键盘按下重映射方向/OK（已被 LL 吞并暂挂）：直接回放原生键，
                // 绝不补发遥控映射键。
                if ev.pressed
                    && native_suppress::source_decision_pending(vk)
                    && !native_suppress::replay_foreign_gate_vk(vk)
                {
                    log::warn!("T1 foreign replay failed for vk=0x{vk:02X}");
                }
            }
            return;
        }
        if !saw_device_cb.swap(true, Ordering::SeqCst) {
            log::info!("T1 device matched: {}", ev.device_name);
            if let Some(st) = app_for_cb.try_state::<BridgeState>() {
                st.update_device_info(
                    BridgeType::T1Usb,
                    Some("T1 Google Remote".into()),
                    Some(ev.device_name.clone()),
                    None,
                );
            }
        }

        // T1 设备命中：消费来源暂挂，按正常映射流程走（不额外回放原生键）
        if let Some(vk) = parse_kbd_vk(&ev.event_id) {
            let _ = native_suppress::consume_source_pending(vk);
        }

        let Some(button_id) = resolve_button(&ev.event_id, &event_to_button) else {
            if ev.pressed && ev.event_id != "hid:02-00-00" {
                log::info!("T1 unmapped event {} device={}", ev.event_id, ev.device_name);
            }
            return;
        };

        let native_vk = parse_kbd_vk(&ev.event_id);

        // 非语音：抬起只停 hold-suppress，不注入
        if !ev.pressed && button_id != "voice" {
            native_suppress::apply_native_release_policy(native_vk);
            return;
        }

        log::info!(
            "T1 button={button_id} event={} pressed={}",
            ev.event_id,
            ev.pressed
        );

        let config = app_for_cb
            .try_state::<ConfigManager>()
            .and_then(|mgr| mgr.get_device_config("t1_usb").ok());
        let Some(config) = config else {
            log::warn!("T1 button ignored: t1 config unavailable");
            return;
        };

        let target_vks = binding_vks(&config, &button_id);
        if ev.pressed {
            native_suppress::apply_native_press_policy(&button_id, native_vk, &target_vks);
        }

        let _ = app_for_cb.emit(
            "t1-key",
            serde_json::json!({
                "id": button_id,
                "event": ev.event_id,
                "pressed": ev.pressed,
                "message": format!(
                    "按键 {} ({}) {}",
                    button_id,
                    ev.event_id,
                    if ev.pressed { "↓" } else { "↑" }
                ),
            }),
        );

        // 语音同步处理，避免 press/release 线程乱序导致右 Alt 立刻被抬起
        if button_id == "voice" {
            let voice_state = Arc::clone(&voice_state_cb);
            handle_voice(&config, &voice_state, &app_for_cb, ev.pressed);
            return;
        }

        if native_suppress::is_passthrough_binding(&button_id, native_vk, &target_vks) {
            log::info!("T1 passthrough button={button_id} (identity/unbound, no inject)");
            return;
        }

        if should_skip_duplicate_inject(&button_id) {
            log::info!("T1 dedupe skip inject button={button_id} event={}", ev.event_id);
            let _ = app_for_cb.emit(
                "t1-key",
                serde_json::json!({
                    "id": button_id,
                    "event": ev.event_id,
                    "message": format!("去重跳过 {}", button_id),
                }),
            );
            return;
        }

        let voice_state = Arc::clone(&voice_state_cb);
        let app_emit = app_for_cb.clone();
        thread::spawn(move || {
            handle_button(&button_id, &config, &voice_state, &app_emit);
        });
    })
    .map_err(|e| {
        runtime_flag.running.store(false, Ordering::SeqCst);
        native_suppress::disarm_reason(native_suppress::SwallowReason::UsbBridge);
        e
    })?;

    *runtime.inner.lock() = Some(RuntimeInner { raw });

    state.update_device_info(
        BridgeType::T1Usb,
        Some("T1 Google Remote".into()),
        Some("VID_1915&PID_1025".into()),
        None,
    );
    log::info!("T1 USB bridge started (swallow side-effects + LL inject; shared Raw Input hub)");
    ensure_voice_watchdog(app.clone());
    Ok(())
}

pub fn stop_t1_bridge(app: &AppHandle, state: &BridgeState) {
    VOICE_WATCHDOG_RUNNING.store(false, Ordering::SeqCst);
    force_end_voice_latch("stop_t1_bridge");
    crate::bridges::t1_usb::mic_cable::stop();
    native_mic::stop_mic_keepalive();
    *LL_GATE_CTX.lock() = None;
    let (_ble_running, _ble_stopping) = app
        .try_state::<Arc<crate::bridges::t1::ble_runtime::T1BleRuntime>>()
        .map(|r| {
            (
                r.running.load(std::sync::atomic::Ordering::SeqCst),
                r.should_stop(),
            )
        })
        .unwrap_or((false, false));
    native_suppress::disarm_reason(native_suppress::SwallowReason::UsbBridge);
    if let Some(runtime) = app.try_state::<Arc<T1Runtime>>() {
        runtime.request_stop();
        if let Some(mut inner) = runtime.inner.lock().take() {
            inner.raw.stop();
        }
        runtime.running.store(false, Ordering::SeqCst);
        runtime.clear_stop();
    }
    state.update_status(BridgeType::T1Usb, BridgeStatus::Disconnected);
    log::info!("T1 bridge stopped");
}

fn parse_kbd_vk(event_id: &str) -> Option<u16> {
    let key = event_id.trim();
    let rest = key.strip_prefix("kbd:VK_")?;
    let hex = rest.split(|c| c == ':' || c == '-').next()?;
    u16::from_str_radix(hex, 16).ok()
}

fn binding_vks(config: &DeviceConfig, button_id: &str) -> Vec<u16> {
    if button_id == "voice" {
        return resolve_voice_vks(config);
    }
    let vks = match config.button_bindings.get(button_id) {
        Some(KeyAction::SingleKey(vk)) => vec![*vk],
        Some(KeyAction::ComboKey(vks)) => vks.clone(),
        _ => Vec::new(),
    };
    // 静音未绑定：默认系统静音（HOGP 原生不可靠）
    if button_id == "mute" && vks.is_empty() {
        return vec![0xAD];
    }
    vks
}

fn sync_media_gates(config: &DeviceConfig) {
    let vol_plus = binding_vks(config, "vol_plus");
    let vol_minus = binding_vks(config, "vol_minus");
    let mute = binding_vks(config, "mute");
    native_suppress::refresh_media_gates_from_bindings(
        Some(vol_plus.as_slice()).filter(|v| !v.is_empty()),
        Some(vol_minus.as_slice()).filter(|v| !v.is_empty()),
        Some(mute.as_slice()).filter(|v| !v.is_empty()),
    );
    let up = binding_vks(config, "up");
    let down = binding_vks(config, "down");
    let left = binding_vks(config, "left");
    let right = binding_vks(config, "right");
    let ok = binding_vks(config, "ok");
    native_suppress::refresh_dpad_remap_gates(
        Some(up.as_slice()).filter(|v| !v.is_empty()),
        Some(down.as_slice()).filter(|v| !v.is_empty()),
        Some(left.as_slice()).filter(|v| !v.is_empty()),
        Some(right.as_slice()).filter(|v| !v.is_empty()),
        Some(ok.as_slice()).filter(|v| !v.is_empty()),
    );
    let home = binding_vks(config, "home");
    native_suppress::refresh_home_vk24_gate(Some(home.as_slice()).filter(|v| !v.is_empty()));
}

/// 与小米一致：button_bindings.voice 优先，其次 voice_hotkey
fn resolve_voice_vks(config: &DeviceConfig) -> Vec<u16> {
    if let Some(action) = config.button_bindings.get("voice") {
        match action {
            KeyAction::SingleKey(vk) if *vk != 0 => return vec![*vk],
            KeyAction::ComboKey(vks) if !vks.is_empty() => return vks.clone(),
            _ => {}
        }
    }
    let names = config
        .voice_hotkey
        .as_ref()
        .map(|v| v.as_slice())
        .unwrap_or(&[]);
    let vks = names_to_vks(names);
    if !vks.is_empty() {
        return vks;
    }
    vec![0xA5] // 默认右 Alt
}

fn format_vks_label(vks: &[u16]) -> String {
    key_diag::format_vks_label(vks)
}

fn handle_button(
    button_id: &str,
    config: &DeviceConfig,
    _voice_state: &Mutex<VoiceSessionState>,
    app: &AppHandle,
) {
    if matches!(
        button_id,
        "power"
            | "mouse"
            | "up"
            | "down"
            | "left"
            | "right"
            | "ok"
            | "mute"
            | "vol_plus"
            | "vol_minus"
    ) {
        return;
    }
    let Some(action) = config.button_bindings.get(button_id) else {
        let msg = format!("映射无效：{button_id} 未绑定");
        key_diag::emit_usb(app, "inject", Some(button_id), None, None, None, &[], &msg);
        return;
    };

    let vks: Vec<u16> = match action {
        KeyAction::SingleKey(vk) => vec![*vk],
        KeyAction::ComboKey(vks) => vks.clone(),
        KeyAction::None => {
            let msg = format!("映射无效：{button_id} 为空");
            key_diag::emit_usb(app, "inject", Some(button_id), None, None, None, &[], &msg);
            return;
        }
        KeyAction::TextInput(_) | KeyAction::LaunchApp(_) => {
            let msg = format!("映射跳过：{button_id} 类型暂不支持");
            key_diag::emit_usb(app, "inject", Some(button_id), None, None, None, &[], &msg);
            return;
        }
    };
    if vks.is_empty() {
        key_diag::emit_usb(
            app,
            "inject",
            Some(button_id),
            None,
            None,
            None,
            &[],
            &format!("映射跳过：{button_id} VK 列表空"),
        );
        return;
    }

    let ok = inject_mapped_keys(&vks, hold_ms_for(&vks));
    let label = format_vks_label(&vks);
    let msg = if ok {
        format!("注入 {button_id} → {label} ✓ winuhid={}", hid_injector::is_available())
    } else {
        format!("注入 {button_id} → {label} 失败 winuhid={}", hid_injector::is_available())
    };
    key_diag::emit_usb(app, "inject", Some(button_id), None, None, None, &vks, &msg);
}

fn hold_ms_for(vks: &[u16]) -> u64 {
    let has_mod = vks
        .iter()
        .any(|vk| matches!(vk, 0xA0 | 0xA1 | 0xA2 | 0xA3 | 0xA4 | 0xA5 | 0x5B | 0x5C));
    if has_mod && vks.len() >= 2 {
        120
    } else {
        50
    }
}

fn inject_mapped_keys(vks: &[u16], hold_ms: u64) -> bool {
    let force_sendinput =
        native_suppress::held_intersects(vks) || native_suppress::vks_need_sendinput(vks);
    let ok = if force_sendinput {
        log::debug!("T1 map: SendInput+EXTRA_INFO vks={vks:?}");
        native_suppress::allow_pass_vks(vks);
        let tapped = if vks.len() == 1 {
            tap_single_vk(vks[0], hold_ms)
        } else {
            tap_vks(vks, hold_ms)
        };
        if crate::bridges::t1::inject::chord_has_modifier(vks) {
            crate::bridges::t1::inject::panic_clear_all_modifiers("usb_map_sendinput_mods");
        } else {
            let _ = hid_injector::release_all();
        }
        tapped
    } else {
        crate::bridges::t1::inject::safe_mapped_tap(vks, hold_ms)
    };
    if ok && crate::bridges::t1::inject::mapped_tap_needs_shell_dummy(vks) {
        crate::bridges::t1::inject::after_mapped_tap(vks);
    }
    ok
}

/// T1 语音键语义（对齐 Python `t1/app.py` + `raw_input_bridge`）：
///
/// - 遥控语音键即使物理长按，HID 也只是约 120ms 脉冲（down 后很快全零抬起）。
/// - **必须忽略 HID 抬起**，否则 Hold 会在脉冲结束时立刻松开右 Alt，映射像「没发送」。
/// - Hold（豆包长按右 Alt）：第一次脉冲 → press 并保持；第二次脉冲 → release（second_press 闩锁）。
/// - Toggle（免按和弦）：每次脉冲 → tap 一次。
fn handle_voice(
    config: &DeviceConfig,
    voice_state: &Mutex<VoiceSessionState>,
    app: &AppHandle,
    pressed: bool,
) {
    if config.voice_shortcut_enabled == false {
        log::info!("T1 voice shortcut disabled");
        let _ = app.emit(
            "t1-key",
            serde_json::json!({ "id": "voice", "message": "语音快捷键已关闭" }),
        );
        return;
    }

    // 抬起：忽略全零抬起（T1 脉冲必然带抬起，不能当「松手结束语音」）
    if !pressed {
        log::info!("T1 voice ignore HID release (≈120ms pulse; latch until second press)");
        return;
    }

    let vks = resolve_voice_vks(config);
    if vks.is_empty() {
        log::warn!("T1 voice hotkey resolved empty");
        return;
    }
    let label = format_vks_label(&vks);

    let mut mode = config.trigger_mode.clone();
    if crate::bridges::t1::inject::mapped_voice_is_tap(&vks) {
        mode = TriggerMode::Toggle;
    }

    match mode {
        TriggerMode::Hold => {
            let starting = {
                let mut g = voice_state.lock();
                if *g == VoiceSessionState::Idle {
                    *g = VoiceSessionState::Active;
                    true
                } else {
                    *g = VoiceSessionState::Idle;
                    false
                }
            };

            if starting {
                native_suppress::arm_voice_browser_search();
                native_suppress::dismiss_windows_search_async(false);
                // 优先 Mic→CABLE（与小米同默认麦）；失败则回退 EnsureUsbMic
                let via_cable = match crate::bridges::t1_usb::mic_cable::start_for_voice(false) {
                    Ok(()) => {
                        log::info!("T1 USB voice audio path=CABLE (mic loopback)");
                        true
                    }
                    Err(e) => {
                        log::warn!("T1 USB mic→CABLE failed ({e}); fallback EnsureUsbMic");
                        native_mic::ensure_default_mic_device_for_ime(MIC_LABEL);
                        false
                    }
                };
                match native_mic::ensure_mic_device(MIC_LABEL) {
                    Ok(endpoint) => {
                        log::info!("T1 AUDIO OPEN source={MIC_LABEL} endpoint={endpoint}")
                    }
                    Err(e) => {
                        log::warn!("T1 AUDIO OPEN WARNING (仍注入快捷键): {e}");
                        let _ = app.emit(
                            "t1-key",
                            serde_json::json!({
                                "id": "voice",
                                "message": format!("麦克风告警：{e}（仍尝试注入）"),
                            }),
                        );
                    }
                }
                let ok = voice_press(&vks);
                native_suppress::arm_voice_browser_search();
                native_suppress::dismiss_windows_search_async(false);
                if ok {
                    *VOICE_LATCH_STARTED.lock() = Some(Instant::now());
                    ensure_voice_watchdog(app.clone());
                    let _ = native_mic::start_mic_keepalive(MIC_LABEL);
                }
                let path = if via_cable { "CABLE" } else { "Mic Device" };
                let msg = if ok {
                    format!("语音开始（闩锁）→ {label} ✓ 再按一次结束 · 经 {path}")
                } else {
                    format!("语音开始 → {label} 失败")
                };
                let _ = app.emit("t1-key", serde_json::json!({ "id": "voice", "message": msg }));
                if ok {
                    log::info!(
                        "T1 voice latch DOWN vks={vks:?} path={path} winuhid={}",
                        hid_injector::is_available()
                    );
                } else {
                    log::warn!("T1 voice latch DOWN failed vks={vks:?}");
                    *voice_state.lock() = VoiceSessionState::Idle;
                    crate::bridges::t1_usb::mic_cable::stop();
                }
            } else {
                let ok = voice_release(&vks);
                crate::bridges::t1_usb::mic_cable::stop();
                native_suppress::release_voice_browser_search();
                native_suppress::dismiss_windows_search_async(true);
                let msg = if ok {
                    format!("语音结束（第二次按）→ {label} ✓")
                } else {
                    format!("语音结束 → {label} 失败")
                };
                let _ = app.emit("t1-key", serde_json::json!({ "id": "voice", "message": msg }));
                log::info!(
                    "T1 voice latch UP (second press) vks={vks:?} ok={ok} winuhid={}",
                    hid_injector::is_available()
                );
            }
        }
        TriggerMode::Toggle => {
            native_suppress::arm_voice_browser_search();
            native_suppress::dismiss_windows_search_async(false);
            let via_cable = match crate::bridges::t1_usb::mic_cable::start_for_voice(true) {
                Ok(()) => {
                    log::info!("T1 USB voice audio path=CABLE (mic loopback, Toggle)");
                    true
                }
                Err(e) => {
                    log::warn!("T1 USB mic→CABLE failed ({e}); fallback EnsureUsbMic");
                    native_mic::ensure_default_mic_device_for_ime(MIC_LABEL);
                    false
                }
            };
            let hold_ms = if vks.len() == 1 && matches!(vks[0], 0x12 | 0xA4 | 0xA5) {
                100
            } else if vks.iter().any(|&vk| vk == 0x5B || vk == 0x5C) {
                120
            } else {
                70
            };
            let ok = voice_tap(&vks, hold_ms);
            native_suppress::release_voice_browser_search();
            native_suppress::arm_voice_browser_search();
            let allow_esc = !vks.iter().any(|&vk| matches!(vk, 0x5B | 0x5C));
            native_suppress::dismiss_windows_search_async(allow_esc);
            match native_mic::ensure_mic_device(MIC_LABEL) {
                Ok(endpoint) => log::info!("T1 AUDIO OPEN source={MIC_LABEL} endpoint={endpoint}"),
                Err(e) => {
                    log::warn!("T1 AUDIO OPEN WARNING (仍注入快捷键): {e}");
                }
            }
            let path = if via_cable { "CABLE" } else { "Mic Device" };
            let msg = if ok {
                format!("语音点按 → {label} ✓（经 {path}）")
            } else {
                format!("语音点按 → {label} 失败")
            };
            let _ = app.emit("t1-key", serde_json::json!({ "id": "voice", "message": msg }));
            if ok {
                log::info!(
                    "T1 voice Toggle tap vks={vks:?} winuhid={}",
                    hid_injector::is_available()
                );
            } else {
                log::warn!("T1 voice Toggle tap failed vks={vks:?}");
            }
        }
    }
}

fn voice_tap(vks: &[u16], hold_ms: u64) -> bool {
    crate::bridges::t1::inject::voice_chord_tap(vks, hold_ms)
}

fn voice_press(vks: &[u16]) -> bool {
    crate::bridges::t1::inject::panic_clear_all_modifiers("voice_press_pre");
    native_suppress::allow_pass_vks(vks);
    // 与小米语音一致：单报告 press_single；禁止 press() 分步回退
    if hid_injector::is_available() {
        match hid_injector::press_single(vks) {
            Ok(()) => {
                crate::bridges::shared::host_hooks::set_virtual_hid_chord_held(Some(vks));
                *VOICE_LATCH_VKS.lock() = vks.to_vec();
                return true;
            }
            Err(e) => log::warn!("T1 voice WinUHid press_single failed: {e}"),
        }
    }
    log::warn!("T1 voice: WinUHid press_single 不可用，降级 SendInput（豆包/千问常无效；请修复虚拟键盘）");
    native_suppress::allow_pass_vks(vks);
    let ok = press_vks(vks);
    if ok {
        *VOICE_LATCH_VKS.lock() = vks.to_vec();
    } else {
        crate::bridges::t1::inject::panic_clear_all_modifiers("voice_press_fail");
    }
    ok
}

fn voice_release(vks: &[u16]) -> bool {
    VOICE_LATCH_VKS.lock().clear();
    *VOICE_LATCH_STARTED.lock() = None;
    native_suppress::allow_pass_vks(vks);
    crate::bridges::shared::host_hooks::set_virtual_hid_chord_held(None);
    if hid_injector::is_available() {
        if let Err(e) = hid_injector::release(vks) {
            log::warn!("T1 voice WinUHid release failed: {e}");
        }
    } else {
        let _ = release_vks(vks);
    }
    crate::bridges::t1::inject::panic_clear_all_modifiers("voice_release");
    true
}
