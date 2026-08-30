//! 小米按键采集 — 对齐 Python 生产路径
//!
//! - HidOverGatt Frida Gadget tap → 返回键 0xF1、音量 0x80/0x81（Windows HID 独占时必需）
//! - ATVV Control → 语音键
//! - 低级键盘钩 → 抑制已由 Tap 映射的原生气，避免双触发
//!
//! 故意不做：hidapi 打开设备、默认 GATT HID 订阅（会抢占 Microsoft HID，导致
//! Windows 原生音量失效且 Tap 未就绪时三键全死）。

use crate::bridges::xiaomi::connect::XiaomiRuntime;
use crate::bridges::xiaomi::input_session::run_input_session;
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct XiaomiKeyEvent {
    pub button_id: String,
    pub label: String,
    /// "down" | "up"
    #[serde(default = "default_key_phase")]
    pub phase: String,
}

#[allow(dead_code)] // referenced by serde default = "default_key_phase"
fn default_key_phase() -> String {
    "down".into()
}

#[derive(Clone, Serialize)]
pub struct XiaomiKeyMessage {
    pub message: String,
}

/// 实际落到系统的按键输出（映射注入 / 原生漏键），供 UI 对比「配置映射 vs 真实发送」。
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct XiaomiKeyOutputEvent {
    /// "down" | "up"
    pub phase: String,
    /// 显示名，如 F5 / M / LCtrl
    pub label: String,
    pub vk: u16,
    /// "mapped" = 本程序注入；"extra" = 监视窗内放行的原生键（漏键/双触发，UI 标红）
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
}

static OUTPUT_APP: Mutex<Option<AppHandle>> = Mutex::new(None);
static OUTPUT_WATCH_UNTIL: Mutex<Option<Instant>> = Mutex::new(None);
/// WinUHid 注入的 VK（无 EXTRA_INFO），避免 LL 再报成 extra
static VIRTUAL_HID_RECENT: Mutex<Vec<(u16, Instant)>> = Mutex::new(Vec::new());
static LAST_REMOTE_LABEL: Mutex<Option<String>> = Mutex::new(None);

const OUTPUT_WATCH_MS: u64 = 900;
/// 短窗：松键后仍忽略同源 VK 的 LL 回声
const VIRTUAL_HID_SKIP_MS: u64 = 400;
/// 语音长按期间：WinUHid 和弦 VK 绝不能标成「漏键」（否则刷屏拖死 LL 钩子，微信也收不全 Ctrl+Win）
static VIRTUAL_HID_HELD: Mutex<Vec<u16>> = Mutex::new(Vec::new());
static F5_SUPPRESS_LOGGED: AtomicBool = AtomicBool::new(false);

pub fn bind_key_output_app(app: AppHandle) {
    *OUTPUT_APP.lock() = Some(app);
}

fn output_app() -> Option<AppHandle> {
    OUTPUT_APP.lock().clone()
}

/// 遥控键活动：开启短监视窗，LL 放行的固件相关 VK 记为 extra
pub fn arm_output_watch(remote_label: &str) {
    *OUTPUT_WATCH_UNTIL.lock() = Some(Instant::now() + Duration::from_millis(OUTPUT_WATCH_MS));
    *LAST_REMOTE_LABEL.lock() = Some(remote_label.to_string());
}

fn output_watch_active() -> bool {
    matches!(
        *OUTPUT_WATCH_UNTIL.lock(),
        Some(until) if Instant::now() <= until
    )
}

/// WinUHid / 虚拟 HID 注入前调用，防止同源 VK 被 LL 误报为漏键
pub fn note_virtual_hid_inject(vks: &[u16]) {
    let now = Instant::now();
    let mut g = VIRTUAL_HID_RECENT.lock();
    g.retain(|(_, t)| now.duration_since(*t) < Duration::from_millis(VIRTUAL_HID_SKIP_MS));
    for &vk in vks {
        g.push((vk, now));
    }
}

/// 语音长按：整段 hold 期间这些 VK 都是「我们注入的」，不是漏键
pub fn set_virtual_hid_chord_held(vks: Option<&[u16]>) {
    let mut g = VIRTUAL_HID_HELD.lock();
    match vks {
        Some(v) => {
            g.clear();
            g.extend_from_slice(v);
        }
        None => g.clear(),
    }
}

fn was_virtual_hid_recent(vk: u16) -> bool {
    if VIRTUAL_HID_HELD.lock().iter().any(|v| *v == vk) {
        return true;
    }
    let now = Instant::now();
    let mut g = VIRTUAL_HID_RECENT.lock();
    g.retain(|(_, t)| now.duration_since(*t) < Duration::from_millis(VIRTUAL_HID_SKIP_MS));
    g.iter().any(|(v, _)| *v == vk)
}

/// 已吞掉的固件键（如 F5）——每轮按压只报一次，且勿在 LL 回调里同步 emit
pub fn emit_suppressed_output(vk: u16) {
    if F5_SUPPRESS_LOGGED.swap(true, Ordering::AcqRel) {
        return;
    }
    // 离开 LL 线程再 emit/log，避免 typematic 拖死钩子被 Windows 静默卸载
    std::thread::spawn(move || {
        let Some(app) = output_app() else {
            return;
        };
        let remote = LAST_REMOTE_LABEL.lock().clone();
        let remote_s = remote.as_deref().unwrap_or("语音");
        let label = crate::bridges::xiaomi::config::vk_code_to_name(vk);
        log::info!(
            "XIAOMI KEY OUTPUT role=suppressed remote={remote_s} vk=0x{vk:02X} label={label}"
        );
        let _ = app.emit(
            "xiaomi-key-output",
            XiaomiKeyOutputEvent {
                phase: "down".into(),
                label,
                vk,
                role: "suppressed".into(),
                remote: remote.or_else(|| Some(button_label("mic").to_string())),
            },
        );
    });
}

pub fn reset_f5_suppress_log_flag() {
    F5_SUPPRESS_LOGGED.store(false, Ordering::Release);
}

/// 本程序映射注入成功后上报（可多次 → UI 显示 ×N）
pub fn emit_mapped_outputs(vks: &[u16], phase_down: bool) {
    if vks.is_empty() {
        return;
    }
    let Some(app) = output_app() else {
        return;
    };
    let remote = LAST_REMOTE_LABEL.lock().clone();
    let phase = if phase_down { "down" } else { "up" };
    let remote_s = remote.as_deref().unwrap_or("-");
    for &vk in vks {
        let label = crate::bridges::xiaomi::config::vk_code_to_name(vk);
        log::info!(
            "XIAOMI KEY OUTPUT role=mapped remote={remote_s} vk=0x{vk:02X} label={label}"
        );
        let _ = app.emit(
            "xiaomi-key-output",
            XiaomiKeyOutputEvent {
                phase: phase.into(),
                label,
                vk,
                role: "mapped".into(),
                remote: remote.clone(),
            },
        );
    }
}

/// LL 钩子：放行的原生键 → extra（漏 F5 / 双触发 / OEM 等）
/// - 一般键：需遥控监视窗（点遥控后 ~900ms）
/// - **F5**：输入会话中即使监视窗未开也记（F5 常比 ATVV/UI 事件更早，否则记事本插了日期但日志空白）
pub fn report_native_passthrough(vk: u16, phase_down: bool) {
    if !phase_down || vk == 0 {
        return;
    }
    if was_virtual_hid_recent(vk) {
        return;
    }
    let session_f5 = vk == 0x74
        && crate::bridges::xiaomi::key_mapping::input_session_active();
    if !output_watch_active() && !session_f5 {
        return;
    }
    let Some(app) = output_app() else {
        return;
    };
    if session_f5 && LAST_REMOTE_LABEL.lock().is_none() {
        *LAST_REMOTE_LABEL.lock() = Some(button_label("mic").to_string());
    }
    let remote = LAST_REMOTE_LABEL.lock().clone();
    let remote_s = remote.as_deref().unwrap_or("-");
    let label = crate::bridges::xiaomi::config::vk_code_to_name(vk);
    log::info!(
        "XIAOMI KEY OUTPUT role=extra remote={remote_s} vk=0x{vk:02X} label={label}"
    );
    let _ = app.emit(
        "xiaomi-key-output",
        XiaomiKeyOutputEvent {
            phase: "down".into(),
            label,
            vk,
            role: "extra".into(),
            remote,
        },
    );
}

/// 固件/OEM 常见泄漏 VK（测试与文档用；实际上报已改为监视窗内全量放行键）
pub fn is_firmware_watch_vk(vk: u16) -> bool {
    matches!(
        vk,
        0x74 | // F5
        0x25 | 0x26 | 0x27 | 0x28 | 0x0D | // arrows + Enter
        0xAD | 0xAE | 0xAF | // mute / vol
        0x24 | 0xAC | 0x5D | // Home / browser home / Apps
        0xA6 | // Browser Back
        0x08 | // Backspace
        0xFC // VK_NONAME（OEM 保留，遥控器偶发泄漏）
    ) || (0xE9..=0xFE).contains(&vk) // OEM / reserved
}

/// 按键去抖门闩：同一 button_id 在窗口内只发一次 UI 事件
pub struct KeyEmitGate {
    last: Mutex<HashMap<String, Instant>>,
    window: Duration,
}

impl KeyEmitGate {
    pub fn new(window_ms: u64) -> Self {
        Self {
            last: Mutex::new(HashMap::new()),
            window: Duration::from_millis(window_ms),
        }
    }

    pub fn try_emit(&self, button_id: &str) -> bool {
        let now = Instant::now();
        let mut guard = self.last.lock();
        if let Some(prev) = guard.get(button_id) {
            if now.duration_since(*prev) < self.window {
                return false;
            }
        }
        guard.insert(button_id.to_string(), now);
        true
    }
}

pub fn emit_key(app: &AppHandle, button_id: &str, label: &str) {
    emit_key_phase(app, button_id, label, true);
}

pub fn emit_key_phase(app: &AppHandle, button_id: &str, label: &str, pressed: bool) {
    arm_output_watch(label);
    let _ = app.emit(
        "xiaomi-key",
        XiaomiKeyEvent {
            button_id: button_id.to_string(),
            label: label.to_string(),
            phase: if pressed { "down".into() } else { "up".into() },
        },
    );
}

/// 对齐 Python：检测后立刻执行 button_bindings 映射
pub fn emit_key_and_map(app: &AppHandle, button_id: &str, label: &str, pressed: bool) {
    emit_key_phase(app, button_id, label, pressed);
    crate::bridges::xiaomi::key_mapping::on_remote_button(app, button_id, pressed);
}

pub fn emit_message(app: &AppHandle, message: &str) {
    let _ = app.emit(
        "xiaomi-key",
        XiaomiKeyMessage {
            message: message.to_string(),
        },
    );
}

pub fn button_label(id: &str) -> &'static str {
    match id {
        "power" => "电源",
        "volume_up" => "音量+",
        "volume_down" => "音量-",
        "up" | "dpad_up" => "上",
        "down" | "dpad_down" => "下",
        "left" | "dpad_left" => "左",
        "right" | "dpad_right" => "右",
        "ok" => "确定",
        "back" => "返回",
        "home" => "主页",
        "menu" => "菜单",
        "voice" | "mic" => "语音",
        "mute" | "volume_mute" => "静音",
        "tv" => "TV",
        _ => "未知",
    }
}

/// 连接成功后启动按键通道（对齐 Python atvv_live_bridge 启动顺序）
pub fn start_key_logger(
    app: AppHandle,
    runtime: Arc<XiaomiRuntime>,
    address_u64: u64,
    atvv_interface_id: String,
) {
    #[cfg(target_os = "windows")]
    {
        use crate::bridges::xiaomi::connect::reset_atvv_subscribed;
        use crate::bridges::xiaomi::hid_report_tap::{ensure_started, stop_and_join};
        use crate::config::manager::ConfigManager;
        use tauri::Manager;

        // 60ms 去抖：防固件 down-up-down 抖动，同时允许真实快速连按（人最快约 100ms/次）
        let gate = Arc::new(KeyEmitGate::new(60));
        let (tap_enabled, hook_enabled) = app
            .try_state::<ConfigManager>()
            .and_then(|m| m.get_device_config("xiaomi").ok())
            .map(|c| {
                crate::bridges::xiaomi::key_mapping::refresh_dpad_ok_custom_suppress_mask(&c);
                (c.hid_report_tap_enabled, c.special_key_hook_enabled)
            })
            .unwrap_or((true, true));

        crate::bridges::xiaomi::special_keys::set_hook_enabled(hook_enabled);
        crate::bridges::xiaomi::key_mapping::bind_voice_hook_app(app.clone());
        bind_key_output_app(app.clone());
        // 语音键固件 F5 直发，需 LL 钩子常驻才能吞 F5（gadget 清除 0x3E 未生效时的唯一兜底）。
        // 因此连接成功后**始终**拉起钩子——覆盖 special_key_hook_enabled=false 的情形，
        // 否则固件 F5 逐字泄漏到系统，与注入的映射键叠加成 F5+Ctrl+Win。
        crate::bridges::xiaomi::special_keys::ensure_hook_for_capture();

        // 对齐 v1.3.3：先 HID Tap，再 ATVV 输入会话（连接阶段已 FromId 打开 ATVV）
        reset_atvv_subscribed();

        let mut tap_started = false;
        if tap_enabled {
            let app2 = app.clone();
            let gate2 = Arc::clone(&gate);
            tap_started = ensure_started(app2, gate2);
            if !tap_started {
                emit_message(
                    &app,
                    "HID Tap 未启动：返回/音量键不可用（请确认 Frida Gadget 资源）",
                );
            }
        } else {
            stop_and_join();
            emit_message(&app, "HID Tap 已按配置禁用");
        }

        crate::bridges::xiaomi::raw_mapping::maybe_start_raw_mapping(
            app.clone(),
            Arc::clone(&runtime),
            Arc::clone(&gate),
            tap_started,
        );

        {
            let app2 = app.clone();
            let runtime2 = Arc::clone(&runtime);
            let gate2 = Arc::clone(&gate);
            let iface = atvv_interface_id.clone();
            std::thread::Builder::new()
                .name("xiaomi-gatt-input".into())
                .spawn(move || {
                    let result =
                        run_input_session(app2.clone(), address_u64, iface, runtime2.clone(), gate2);
                    runtime2.running.store(false, std::sync::atomic::Ordering::SeqCst);
                    match result {
                        Ok(()) => {}
                        Err(e) => {
                            log::warn!("ATVV input session unavailable: {e}");
                            emit_message(&app2, &format!("ATVV 语音通道不可用: {e}"));
                        }
                    }
                })
                .ok();
        }

        {
            let app2 = app.clone();
            let runtime2 = Arc::clone(&runtime);
            let gate2 = Arc::clone(&gate);
            std::thread::Builder::new()
                .name("xiaomi-vk-poll".into())
                .spawn(move || {
                    windows_vk_poll_logger(app2, runtime2, gate2);
                })
                .ok();
        }

        emit_message(
            &app,
            "按键监听已启动（HID-Tap 返回/音量 + ATVV 语音/音频）",
        );
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app, runtime, address_u64, atvv_interface_id);
    }
}

/// VK 轮询：仅作 UI/诊断兜底，不执行映射（避免与系统原生气 + HID 映射双触发）
#[cfg(target_os = "windows")]
fn windows_vk_poll_logger(app: AppHandle, runtime: Arc<XiaomiRuntime>, gate: Arc<KeyEmitGate>) {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

    let keys: &[(i32, &str)] = &[
        (0xAF, "volume_up"),
        (0xAE, "volume_down"),
        (0xAD, "volume_mute"),
        (0x26, "up"),
        (0x28, "down"),
        (0x25, "left"),
        (0x27, "right"),
        (0x0D, "ok"),
        (0x24, "home"),
    ];

    let mut prev: HashMap<i32, bool> = HashMap::new();
    while !runtime.should_stop() {
        for &(vk, id) in keys {
            let down = unsafe { GetAsyncKeyState(vk) as u16 } & 0x8000 != 0;
            let was = prev.get(&vk).copied().unwrap_or(false);
            if down && !was && gate.try_emit(id) {
                emit_key(&app, id, button_label(id));
                log::info!("XIAOMI VK observe key={id} vk=0x{vk:02X} (no map)");
            }
            prev.insert(vk, down);
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}
