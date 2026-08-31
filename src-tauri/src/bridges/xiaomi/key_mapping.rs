//! 对齐 Python `handle_direct_hid_report` / `VoiceShortcut` / `_perform_button_action`
//!
//! 遥控器按键 → 读取 xiaomi.json 的 button_bindings / voice_hotkey → SendInput 注入

use crate::bridges::xiaomi::voice_f5_trace::{self, Guards as F5Guards};
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
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

/// 与 Python `EXTRA_INFO = 0x584D4952` ('XMIR') 一致，供 LL hook 放行虚拟键
pub const EXTRA_INFO: usize = 0x584D_4952;

/// 语音注入前 bump settle 上限（ms）。
/// 微信「F5+映射」场景下长等待收益小；仍请求置顶，但不再空等 40ms。
pub const VOICE_BUMP_SETTLE_MS: u64 = 8;

pub fn voice_bump_settle_ms() -> u64 {
    VOICE_BUMP_SETTLE_MS
}

static VOICE_CHORD: Mutex<VoiceChordState> = Mutex::new(VoiceChordState::empty());
static DIRECT_MARKS: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);
static REPEAT_GEN: Mutex<Option<HashMap<String, u64>>> = Mutex::new(None);
static ACTION_SEQ: AtomicU64 = AtomicU64::new(1);

/// 语音键在 Windows 上常被译成 F5；记事本 F5=插入日期。
/// 短窗 `direct_signal_recent` 盖不住 typematic，故额外 sticky 抑制直到 F5 抬起或截止。
static VOICE_NATIVE_SUPPRESS: AtomicBool = AtomicBool::new(false);
static VOICE_NATIVE_DEADLINE: Mutex<Option<Instant>> = Mutex::new(None);
/// 本轮已吞过固件 F5 DOWN（sticky）。UP 到达或空闲超时才解粘；
/// **`end_voice_period` 不得清除**（ATVV 松开通常早于 F5 KEYUP）。
static VOICE_F5_DOWN_SUPPRESSED: AtomicBool = AtomicBool::new(false);
/// 本按压周期内曾有 F5 DOWN **放行进 OS**（leak）。此后 UP 必须放行解粘，
/// 即使 late mic 已补 sticky（实机 14:18：keyup_suppress → F5 永久按下）。
static F5_DOWN_REACHED_OS: AtomicBool = AtomicBool::new(false);
/// 最近一次参与抑制判定的 F5 事件时刻（空闲解粘基准）。
static VOICE_F5_LAST_EVENT: Mutex<Option<Instant>> = Mutex::new(None);
/// sticky 在无 F5 事件超过此时长后自动解粘，避免永久吞真键盘 F5。
pub const VOICE_F5_STICKY_MAX_IDLE_MS: u64 = 10_000;
/// 非阻塞关联窗：mic/voice 标记与 F5 的最大间隔。
pub const VOICE_F5_CORRELATE_MS: u64 = 120;
/// Python `_should_suppress_voice_f5`：F5 先到时在钩子里 wait mic≈80ms。
pub const VOICE_F5_CORRELATE_WAIT_MS: u64 = 80;
/// ATVV/remote 松开后仍可能收到固件 F5 typematic；tail 内继续吞，避免 11:49:13 类泄漏。
pub const VOICE_F5_POST_TAIL_MS: u64 = 3_000;
/// F5 放行时刻：若随后 mic 在关联窗内到达，补 sticky 堵住 typematic。
static LAST_PASSTHROUGH_F5_DOWN: Mutex<Option<Instant>> = Mutex::new(None);
/// `end_voice_period` 后至此时刻：无 period/arm 时仍吞遥控 F5 typematic。
static VOICE_F5_POST_TAIL_UNTIL: Mutex<Option<Instant>> = Mutex::new(None);
/// ATVV 语音按压周期（0x08/0x04 → 松开）：周期内无条件吞全部 F5 down/up。
static VOICE_PERIOD_ACTIVE: AtomicBool = AtomicBool::new(false);
static INPUT_SESSION_ACTIVE: AtomicBool = AtomicBool::new(false);
static FIRMWARE_VOICE_HELD: AtomicBool = AtomicBool::new(false);
static VOICE_HOOK_APP: Mutex<Option<AppHandle>> = Mutex::new(None);
/// 语音唤醒后端锁定：0=无 1=WinUHid 2=SendInput。DOWN 时写入，UP 时沿用，禁止中途换路双发。
static VOICE_INJECT_BACKEND_HELD: AtomicU8 = AtomicU8::new(0);
static VOICE_SENDINPUT_DEGRADED_TOAST_LAST: Mutex<Option<Instant>> = Mutex::new(None);
const VOICE_BACKEND_NONE: u8 = 0;
const VOICE_BACKEND_HID: u8 = 1;
const VOICE_BACKEND_SENDINPUT: u8 = 2;

/// 当前 F5 guard 快照（供全链路 TRACE 与冲突检测）
pub fn voice_f5_guards_snapshot() -> F5Guards {
    F5Guards {
        session: input_session_active(),
        period: voice_period_active(),
        armed: voice_native_suppress_active(),
        correlate: voice_mic_correlate_active(),
        tail: post_voice_f5_tail_active(),
        sticky: voice_f5_down_suppressed(),
    }
}

/// 输入会话（含仅电量）运行中：供 F5 固件泄漏抑制
pub fn set_input_session_active(active: bool) {
    INPUT_SESSION_ACTIVE.store(active, Ordering::Release);
    if active {
        reset_atvv_f5_toast_throttle();
        voice_f5_trace::state(
            "session_on",
            "input_session_active=true",
            voice_f5_guards_snapshot(),
        );
    } else {
        end_voice_period("session_end");
        disarm_voice_native_suppress();
        voice_f5_trace::state(
            "session_off",
            "input_session_active=false disarm+period_end",
            voice_f5_guards_snapshot(),
        );
    }
}

pub fn input_session_active() -> bool {
    INPUT_SESSION_ACTIVE.load(Ordering::Acquire)
}

/// ATVV 0x08/0x04：进入语音按压周期（先于/并行于固件 F5）。
pub fn begin_voice_period() {
    VOICE_PERIOD_ACTIVE.store(true, Ordering::Release);
    *VOICE_F5_POST_TAIL_UNTIL.lock() = None;
    *LAST_PASSTHROUGH_F5_DOWN.lock() = None;
    arm_voice_native_suppress();
    crate::bridges::xiaomi::key_log::reset_f5_suppress_log_flag();
    voice_f5_trace::state("period_begin", "voice_period+arm", voice_f5_guards_snapshot());
}

/// 遥控松开 / 会话结束：结束周期。
///
/// **不清** `VOICE_F5_DOWN_SUPPRESSED`：ATVV 松开（0x00）通常早于固件 F5 KEYUP，
/// 此处清标志会让后续 typematic 漏到系统（记事本插日期）。
/// sticky 由 [`should_suppress_voice_f5`] 管理（UP 到达或长时间无 F5 事件时解粘）。
pub fn end_voice_period(reason: &str) {
    let was = VOICE_PERIOD_ACTIVE.swap(false, Ordering::AcqRel);
    VOICE_INJECT_BACKEND_HELD.store(VOICE_BACKEND_NONE, Ordering::Release);
    *LAST_PASSTHROUGH_F5_DOWN.lock() = None;
    *VOICE_F5_POST_TAIL_UNTIL.lock() =
        Some(Instant::now() + Duration::from_millis(VOICE_F5_POST_TAIL_MS));
    if was {
        voice_f5_trace::state(
            "period_end",
            &format!("reason={reason} post_tail_ms={VOICE_F5_POST_TAIL_MS}"),
            voice_f5_guards_snapshot(),
        );
    }
}

pub fn voice_period_active() -> bool {
    VOICE_PERIOD_ACTIVE.load(Ordering::Acquire)
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
        voice_f5_trace::event(
            "firmware_fallback",
            "down",
            "atvv_unsub",
            "firmware F5→handle_voice (no ATVV)",
            voice_f5_guards_snapshot(),
            None,
        );
        mark_direct_signal("voice");
        mark_direct_signal("mic");
        emit_key_phase(&app, "mic", button_label("mic"), true);
        handle_voice(&app, true);
        log::debug!("XIAOMI VOICE UI down (firmware F5 fallback)");
    } else {
        if !FIRMWARE_VOICE_HELD.swap(false, Ordering::SeqCst) {
            return;
        }
        voice_f5_trace::event(
            "firmware_fallback",
            "up",
            "atvv_unsub",
            "firmware F5 up→handle_voice",
            voice_f5_guards_snapshot(),
            None,
        );
        mark_direct_signal("voice");
        mark_direct_signal("mic");
        emit_key_phase(&app, "mic", button_label("mic"), false);
        handle_voice(&app, false);
        log::debug!("XIAOMI VOICE UI up (firmware F5 fallback)");
    }
}

/// 与 special_keys F5 抑制策略对齐（测试/文档）
pub const VOICE_F5_SUPPRESS_DEADLINE_MS: u64 = 3_000;

/// 方向/OK 固件在 Windows 上的原生 VK（未清 usage 时 BLE HID 的翻译结果）。
pub fn firmware_vk_for_dpad_ok(button_id: &str) -> Option<u16> {
    match button_id {
        "up" | "dpad_up" => Some(0x26),
        "down" | "dpad_down" => Some(0x28),
        "left" | "dpad_left" => Some(0x25),
        "right" | "dpad_right" => Some(0x27),
        "ok" => Some(0x0D),
        _ => None,
    }
}

/// 映射是否等于固件原生 VK（身份映射：应透传原生，禁止再注入）。
pub fn is_dpad_ok_identity_mapping(button_id: &str, action: &KeyAction) -> bool {
    match (firmware_vk_for_dpad_ok(button_id), action) {
        (Some(fw), KeyAction::SingleKey(vk)) => fw == *vk,
        _ => false,
    }
}

/// 是否应对该方向/OK 做应用侧注入。
/// 方向/OK 一律注入（gadget 清固件 usage）；身份与自定义都走 SendInput，避免双发/漏发。
pub fn should_inject_dpad_ok_mapping(button_id: &str, _action: &KeyAction) -> bool {
    firmware_vk_for_dpad_ok(button_id).is_some()
}

/// KeyEmitGate 是否允许挡住该键的映射路径。
/// 方向/OK 必须为 false：否则快速连点会被 60ms 去抖吞成「点三次只跳一格」。
pub fn should_gate_block_dpad_ok_mapping(button_id: &str) -> bool {
    firmware_vk_for_dpad_ok(button_id).is_none()
}

/// 自定义映射的方向/OK 固件 VK 位图（与 Home 同策略：Tap 就绪即吞该固件 VK）。
/// bit0=Left bit1=Up bit2=Right bit3=Down bit4=Enter
static DPAD_OK_CUSTOM_SUPPRESS_MASK: AtomicU32 = AtomicU32::new(0);

fn dpad_ok_vk_bit(vk: u16) -> Option<u32> {
    match vk {
        0x25 => Some(1 << 0),
        0x26 => Some(1 << 1),
        0x27 => Some(1 << 2),
        0x28 => Some(1 << 3),
        0x0D => Some(1 << 4),
        _ => None,
    }
}

pub fn set_dpad_ok_custom_suppress_vks(vks: &[u16]) {
    let mut mask = 0u32;
    for &vk in vks {
        if let Some(bit) = dpad_ok_vk_bit(vk) {
            mask |= bit;
        }
    }
    DPAD_OK_CUSTOM_SUPPRESS_MASK.store(mask, Ordering::Release);
}

pub fn dpad_ok_custom_suppress_contains(vk: u16) -> bool {
    let Some(bit) = dpad_ok_vk_bit(vk) else {
        return false;
    };
    DPAD_OK_CUSTOM_SUPPRESS_MASK.load(Ordering::Acquire) & bit != 0
}

/// 非身份映射的方向/OK：写入吞键表（Up→M 时吞 VK_UP，与 Home→Space 吞 VK_HOME 同理）。
pub fn refresh_dpad_ok_custom_suppress_mask(config: &DeviceConfig) {
    let mut vks = Vec::new();
    for id in [
        "up",
        "down",
        "left",
        "right",
        "ok",
        "dpad_up",
        "dpad_down",
        "dpad_left",
        "dpad_right",
    ] {
        let Some(action) = lookup_action(config, id) else {
            continue;
        };
        // 身份映射不进表 → 真实键盘该键仍可用；自定义才吞固件 VK
        if firmware_vk_for_dpad_ok(id).is_some() && !is_dpad_ok_identity_mapping(id, action) {
            if let Some(fw) = firmware_vk_for_dpad_ok(id) {
                if !vks.contains(&fw) {
                    vks.push(fw);
                }
            }
        }
    }
    set_dpad_ok_custom_suppress_vks(&vks);
    if !vks.is_empty() {
        log::debug!("XIAOMI DPAD custom suppress vks={vks:?}");
    }
}

/// 兼容旧 sticky API（改为 no-op / 读自定义表）。
pub const DPAD_OK_FIRMWARE_SUPPRESS_MS: u64 = 0;
pub fn arm_dpad_ok_firmware_suppress(_button_id: &str) {}
pub fn clear_dpad_ok_firmware_suppress() {
    // 不清除自定义表；仅测服用
}
pub fn dpad_ok_firmware_suppress_active(vk: u16) -> bool {
    dpad_ok_custom_suppress_contains(vk)
}

pub fn arm_voice_native_suppress() {
    VOICE_NATIVE_SUPPRESS.store(true, Ordering::Release);
    *VOICE_NATIVE_DEADLINE.lock() =
        Some(Instant::now() + Duration::from_millis(VOICE_F5_SUPPRESS_DEADLINE_MS));
    voice_f5_trace::state("arm", "voice_native_suppress deadline armed", voice_f5_guards_snapshot());
}

/// 显式解除语音 F5 抑制：**一并清掉 sticky 与事件计时**。
///
/// 与 [`end_voice_period`] 不同：period end 保留 sticky（等 F5 KEYUP）；
/// disarm 是明确停手信号，sticky 残留会把 F5 永久吞掉。
pub fn disarm_voice_native_suppress() {
    VOICE_NATIVE_SUPPRESS.store(false, Ordering::Release);
    *VOICE_NATIVE_DEADLINE.lock() = None;
    VOICE_F5_DOWN_SUPPRESSED.store(false, Ordering::Release);
    F5_DOWN_REACHED_OS.store(false, Ordering::Release);
    *VOICE_F5_LAST_EVENT.lock() = None;
    *LAST_PASSTHROUGH_F5_DOWN.lock() = None;
    *VOICE_F5_POST_TAIL_UNTIL.lock() = None;
    voice_f5_trace::state("disarm", "clear sticky+tail+passthrough", voice_f5_guards_snapshot());
}

/// armed 窗口是否仍然有效。
///
/// 当 F5 仍处于按住状态（sticky）时，截止到期**续期**而非失效——
/// 语音长按 >3s 是常态，旧实现会让判据在 3s 后消失。
pub fn voice_native_suppress_active() -> bool {
    if !VOICE_NATIVE_SUPPRESS.load(Ordering::Acquire) {
        return false;
    }
    let mut g = VOICE_NATIVE_DEADLINE.lock();
    match *g {
        Some(deadline) if Instant::now() <= deadline => true,
        Some(_) if VOICE_F5_DOWN_SUPPRESSED.load(Ordering::Acquire) => {
            *g = Some(Instant::now() + Duration::from_millis(VOICE_F5_SUPPRESS_DEADLINE_MS));
            true
        }
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
    // 语音键原生多为 F5：提前 arm，避免关联窗后 typematic 漏进记事本
    if name == "mic" || name == "voice" || binding_aliases(name).iter().any(|a| *a == "mic") {
        voice_f5_trace::event(
            "correlate",
            "-",
            "mark_signal",
            &format!("signal={name}"),
            voice_f5_guards_snapshot(),
            None,
        );
        arm_voice_native_suppress();
        apply_late_correlate_from_passthrough_f5();
    }
}

/// 记录「未吞掉的 F5 DOWN」时刻，供迟到 mic 关联补 sticky。
/// 同时标记本周期 DOWN 已进 OS：后续 UP 不得吞，否则 F5 粘键。
pub fn note_passthrough_f5_down() {
    *LAST_PASSTHROUGH_F5_DOWN.lock() = Some(Instant::now());
    F5_DOWN_REACHED_OS.store(true, Ordering::Release);
    voice_f5_trace::event(
        "correlate",
        "down",
        "passthrough_record",
        &format!("await_late_ms={VOICE_F5_CORRELATE_MS} reached_os=1"),
        voice_f5_guards_snapshot(),
        None,
    );
}

fn apply_late_correlate_from_passthrough_f5() {
    let mut g = LAST_PASSTHROUGH_F5_DOWN.lock();
    let Some(passthrough_at) = *g else {
        voice_f5_trace::event(
            "correlate",
            "-",
            "late_miss",
            "reason=no_passthrough_record",
            voice_f5_guards_snapshot(),
            None,
        );
        return;
    };
    let age_ms = passthrough_at.elapsed().as_millis();
    let within = age_ms <= u128::from(VOICE_F5_CORRELATE_MS);
    if within {
        VOICE_F5_DOWN_SUPPRESSED.store(true, Ordering::Release);
        drop(g);
        touch_f5_event();
        voice_f5_trace::event(
            "correlate",
            "down",
            "late_hit",
            &format!("f5_to_mic_ms={age_ms} window_ms={VOICE_F5_CORRELATE_MS}"),
            voice_f5_guards_snapshot(),
            None,
        );
        *LAST_PASSTHROUGH_F5_DOWN.lock() = None;
    } else {
        voice_f5_trace::event(
            "correlate",
            "-",
            "late_miss",
            &format!("reason=outside_window f5_to_mic_ms={age_ms} window_ms={VOICE_F5_CORRELATE_MS}"),
            voice_f5_guards_snapshot(),
            None,
        );
        *g = None;
    }
}

fn log_voice_f5_suppress_down(reason: &str) {
    voice_f5_trace::event(
        "correlate",
        "down",
        "decide_suppress",
        &format!("reason={reason}"),
        voice_f5_guards_snapshot(),
        None,
    );
}

/// mic/voice 标记是否仍在非阻塞关联窗内。
pub fn voice_mic_correlate_active() -> bool {
    let window = Duration::from_millis(VOICE_F5_CORRELATE_MS);
    direct_signal_recent("mic", window) || direct_signal_recent("voice", window)
}

/// 语音松开后 tail 窗：吞迟到的固件 F5 typematic（日志 11:49:13 类泄漏）。
pub fn post_voice_f5_tail_active() -> bool {
    match *VOICE_F5_POST_TAIL_UNTIL.lock() {
        Some(deadline) => Instant::now() <= deadline,
        None => false,
    }
}

/// Python `wait_for_direct_signal("mic", timeout≈0.08)`：仅在 BLE 会话在线时调用。
fn wait_for_mic_correlate(timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    voice_f5_trace::event(
        "correlate",
        "down",
        "wait_mic_start",
        &format!("max_ms={}", timeout.as_millis()),
        voice_f5_guards_snapshot(),
        None,
    );
    while Instant::now() < deadline {
        if voice_mic_correlate_active() {
            voice_f5_trace::event(
                "correlate",
                "down",
                "wait_mic_hit",
                "mic arrived during bounded wait",
                voice_f5_guards_snapshot(),
                None,
            );
            return true;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let hit = voice_mic_correlate_active();
    voice_f5_trace::event(
        "correlate",
        "down",
        if hit { "wait_mic_hit" } else { "wait_mic_miss" },
        &format!("elapsed_ms={}", timeout.as_millis()),
        voice_f5_guards_snapshot(),
        None,
    );
    hit
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

fn touch_f5_event() {
    *VOICE_F5_LAST_EVENT.lock() = Some(Instant::now());
}

fn voice_f5_sticky_valid(now: Instant, last: Option<Instant>) -> bool {
    match last {
        Some(t) => now.duration_since(t) <= Duration::from_millis(VOICE_F5_STICKY_MAX_IDLE_MS),
        None => false,
    }
}

/// 固件语音键常被译成 F5。
///
/// **DOWN**：语音周期 / armed / **mic·voice 关联窗** → 吞，并记 sticky；
///          已 sticky 且未超时 → 一律吞（覆盖 typematic）。
///          **不用** `input_session_active`：会话在线时吞全体 F5 会让真键盘 F5 完全失效。
///          遥控器裸 F5 靠 gadget 清 `0x003E` + 语音窗口 / 关联窗 LL 兜底。
/// **UP**：对齐 Python 配对吞，但加安全阀——
///        若本周期曾有 DOWN 放行进 OS（`F5_DOWN_REACHED_OS`），UP **必须放行**解粘；
///        仅当全部 DOWN 都被本钩吞掉（sticky 且未 leak）时才吞 UP。
///
/// 运行在 `WH_KEYBOARD_LL` 回调里。无 guard 且会话在线时允许 **bounded** wait mic（Python 对齐）。
pub fn should_suppress_voice_f5(down: bool, up: bool, _tap_ready: bool) -> bool {
    if up {
        let leaked = F5_DOWN_REACHED_OS.swap(false, Ordering::AcqRel);
        let was_sticky = VOICE_F5_DOWN_SUPPRESSED.swap(false, Ordering::AcqRel);
        *VOICE_F5_LAST_EVENT.lock() = None;
        // 漏过 DOWN 进 OS 后绝不能吞 UP（实机 F5 永久按下）
        let suppress_up = was_sticky && !leaked;
        voice_f5_trace::event(
            "correlate",
            "up",
            if suppress_up {
                "keyup_suppress"
            } else if leaked {
                "keyup_pass_unstick_leak"
            } else {
                "keyup_pass"
            },
            &format!("cleared_sticky={was_sticky} leaked_down={leaked} python_pair_safe"),
            voice_f5_guards_snapshot(),
            None,
        );
        return suppress_up;
    }
    if !down {
        return false;
    }
    let now = Instant::now();
    let last = *VOICE_F5_LAST_EVENT.lock();
    if VOICE_F5_DOWN_SUPPRESSED.load(Ordering::Acquire) {
        if voice_f5_sticky_valid(now, last) {
            touch_f5_event();
            let _ = voice_native_suppress_active();
            voice_f5_trace::event(
                "correlate",
                "down",
                "decide_suppress",
                "reason=sticky_typematic",
                voice_f5_guards_snapshot(),
                None,
            );
            return true;
        }
        VOICE_F5_DOWN_SUPPRESSED.store(false, Ordering::Release);
        voice_f5_trace::event(
            "correlate",
            "down",
            "sticky_idle_release",
            &format!("idle_ms>{VOICE_F5_STICKY_MAX_IDLE_MS}"),
            voice_f5_guards_snapshot(),
            None,
        );
    }
    let period = voice_period_active();
    let armed = voice_native_suppress_active();
    let correlate = voice_mic_correlate_active();
    let tail = post_voice_f5_tail_active();
    let mut in_guard = period || armed || correlate || tail;
    let mut waited_mic = false;
    if !in_guard && down && input_session_active() {
        waited_mic = wait_for_mic_correlate(Duration::from_millis(VOICE_F5_CORRELATE_WAIT_MS));
        if waited_mic {
            in_guard = true;
        }
    }
    if !in_guard {
        voice_f5_trace::event(
            "correlate",
            "down",
            "decide_passthrough",
            &format!(
                "period={period} armed={armed} corr={correlate} tail={tail} waited_mic={waited_mic}"
            ),
            voice_f5_guards_snapshot(),
            None,
        );
        return false;
    }
    let correlate = voice_mic_correlate_active();
    let reason = if waited_mic && !period && !armed && !tail {
        "f5_wait_mic"
    } else if tail && !period && !armed && !correlate {
        "post_voice_tail"
    } else if correlate && !period && !armed && !tail {
        "mic_before_f5"
    } else if period && correlate {
        "voice_period+mic_correlate"
    } else if period {
        "voice_period"
    } else if armed && correlate {
        "voice_armed+mic_correlate"
    } else if armed {
        "voice_armed"
    } else if tail {
        "post_voice_tail+guard"
    } else {
        "mic_correlate"
    };
    log_voice_f5_suppress_down(reason);
    VOICE_F5_DOWN_SUPPRESSED.store(true, Ordering::Release);
    touch_f5_event();
    true
}

/// 复位全部 F5 抑制相关状态。仅供测试使用。
#[doc(hidden)]
pub fn voice_f5_reset_for_test() {
    VOICE_F5_DOWN_SUPPRESSED.store(false, Ordering::Release);
    F5_DOWN_REACHED_OS.store(false, Ordering::Release);
    VOICE_NATIVE_SUPPRESS.store(false, Ordering::Release);
    *VOICE_NATIVE_DEADLINE.lock() = None;
    *VOICE_F5_LAST_EVENT.lock() = None;
    *LAST_PASSTHROUGH_F5_DOWN.lock() = None;
    *VOICE_F5_POST_TAIL_UNTIL.lock() = None;
    *DIRECT_MARKS.lock() = None;
    VOICE_PERIOD_ACTIVE.store(false, Ordering::Release);
    VOICE_INJECT_BACKEND_HELD.store(VOICE_BACKEND_NONE, Ordering::Release);
    INPUT_SESSION_ACTIVE.store(false, Ordering::Release);
}

/// sticky 是否已置位（诊断/测试）。
pub fn voice_f5_down_suppressed() -> bool {
    VOICE_F5_DOWN_SUPPRESSED.load(Ordering::Acquire)
}

#[doc(hidden)]
pub fn voice_f5_expire_suppress_deadline_for_test() {
    *VOICE_NATIVE_DEADLINE.lock() = Some(Instant::now() - Duration::from_millis(1));
}

#[doc(hidden)]
pub fn voice_f5_set_last_event_age_for_test(age_ms: u64) {
    *VOICE_F5_LAST_EVENT.lock() = Some(Instant::now() - Duration::from_millis(age_ms));
}

#[doc(hidden)]
pub fn voice_f5_expire_post_tail_for_test() {
    *VOICE_F5_POST_TAIL_UNTIL.lock() = Some(Instant::now() - Duration::from_millis(1));
}

#[doc(hidden)]
pub fn voice_f5_sticky_valid_for_test(now: Instant, last: Option<Instant>) -> bool {
    voice_f5_sticky_valid(now, last)
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
    // 录入中禁止映射注入，避免「真实键盘刚被吞录 → 又 SendInput 映射键」干扰录入引擎
    if crate::bridges::shared::shortcut_capture::is_swallow_active() {
        log::debug!("XIAOMI MAPPING skipped during shortcut capture key={button_id}");
        return;
    }
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

    // 方向/OK：一律注入（gadget 清固件 usage）；先 mark 再注入，便于 LL 按 recent 吞残留
    refresh_dpad_ok_custom_suppress_mask(&config);
    mark_direct_signal(button_id);
    let triggered = perform_button_action(&config, button_id);
    log::debug!("XIAOMI MAPPING key={button_id} mapped={triggered} pressed=true");

    if triggered {
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
            // 方向/OK 自定义映射：强制 SendInput+EXTRA_INFO，避免 WinUHid 被当成原生再吞
            if firmware_vk_for_dpad_ok(button_id).is_some() {
                tap_vks_sendinput_extra(&[*vk], 20);
            } else {
                tap_vks(&[*vk], 20);
            }
            crate::bridges::xiaomi::key_log::emit_mapped_outputs(&[*vk], true);
            true
        }
        KeyAction::ComboKey(vks) if !vks.is_empty() => {
            if firmware_vk_for_dpad_ok(button_id).is_some() {
                tap_vks_sendinput_extra(vks, 70);
            } else {
                tap_vks(vks, 70);
            }
            crate::bridges::xiaomi::key_log::emit_mapped_outputs(vks, true);
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

/// 方向/OK 自定义映射专用：带 EXTRA_INFO 的 SendInput，LL 钩子认作注入并放行。
fn tap_vks_sendinput_extra(vks: &[u16], hold_ms: u64) {
    #[cfg(target_os = "windows")]
    {
        let _ = key_chord_send_input_with_extra(vks, false, EXTRA_INFO);
        std::thread::sleep(Duration::from_millis(hold_ms.max(1)));
        let _ = key_chord_send_input_with_extra(vks, true, EXTRA_INFO);
        let _ = ACTION_SEQ.fetch_add(1, Ordering::Relaxed);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (vks, hold_ms);
    }
}

fn handle_voice(app: &AppHandle, pressed: bool) {
    // 无论快捷键是否启用：先进入/结束按压周期并吞固件 F5（禁用快捷键也不能漏 F5）
    if pressed {
        begin_voice_period();
        crate::bridges::xiaomi::special_keys::ensure_hook_for_capture();
    } else {
        force_release_voice_shortcut("remote_up");
        crate::bridges::xiaomi::key_log::set_virtual_hid_chord_held(None);
        end_voice_period("remote_up");
        return;
    }

    let Some(config) = load_xiaomi_config(app) else {
        return;
    };
    if !config.voice_shortcut_enabled {
        log::info!("XIAOMI VOICE shortcut disabled (F5 still suppressed)");
        return;
    }
    let vks = resolve_voice_hotkey(&config);
    if vks.is_empty() {
        log::warn!("XIAOMI VOICE shortcut empty");
        return;
    }
    // 固件原生就是 F5：若映射也绑成 F5，等于「吞掉再原样注入」→ 用户只看到 F5
    if voice_hotkey_is_firmware_f5(&vks) {
        log::error!(
            "XIAOMI VOICE shortcut is F5 (vk=0x74) — refusing inject. \
             Rebind mic to Left Ctrl+Left Win (WeChat) or your IME hotkey; \
             capturing the remote voice key itself records firmware F5."
        );
        return;
    }
    crate::bridges::xiaomi::key_log::arm_output_watch(
        crate::bridges::xiaomi::key_log::button_label("mic"),
    );
    // PR #8 实证：必须把本进程 LL 钩子顶到微信/输入法之前，才能替它们吞掉固件 F5。
    // 只吞对本进程有效；微信若在链头已看到 F5，return 1 无法撤回。
    // 现在 handle_voice 跑在 voice_dispatch 工作线程上，等待 bump 才有意义。
    match crate::bridges::xiaomi::special_keys::bump_hook_to_front_and_settle(voice_bump_settle_ms()) {
        crate::bridges::xiaomi::hook_bump::BumpOutcome::Settled => {}
        crate::bridges::xiaomi::hook_bump::BumpOutcome::TimedOut => {
            log::warn!("XIAOMI VOICE bump settle timed out — hook may not be at chain head");
        }
        crate::bridges::xiaomi::hook_bump::BumpOutcome::SelfDeadlock => {
            log::error!("XIAOMI VOICE bump settle self-deadlock (called from hook thread)");
        }
        crate::bridges::xiaomi::hook_bump::BumpOutcome::NoHookThread => {
            log::debug!("XIAOMI VOICE bump: hook thread not ready yet");
        }
    }
    let pressed_ok = {
        let mut state = VOICE_CHORD.lock();
        state.press_with(&vks, inject_voice_chord)
    };
    if pressed_ok {
        log::info!("XIAOMI VOICE SHORTCUT DOWN vks={vks:?}");
    } else {
        log::warn!("XIAOMI VOICE SHORTCUT DOWN failed vks={vks:?}");
        crate::bridges::xiaomi::key_log::set_virtual_hid_chord_held(None);
    }
}

/// 语音快捷键不可含固件 F5（录入遥控语音键时极易误录成 0x74）。
fn voice_hotkey_is_firmware_f5(vks: &[u16]) -> bool {
    vks.iter().any(|v| *v == 0x74)
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

/// AutoHotkey 同款：Win/Alt 抬起后点一下无键帽 vkE8，避免开始菜单/窗口菜单栏。
/// **语音路径已停用**（微信会收到空键）；保留函数便于菜单弹起时再挂回。
#[allow(dead_code)]
#[cfg(target_os = "windows")]
fn inject_shell_menu_suppress_dummy() {
    use crate::bridges::xiaomi::voice_inject::ALT_MENU_SUPPRESS_DUMMY_VK;
    let vk = [ALT_MENU_SUPPRESS_DUMMY_VK];
    let _ = key_chord_send_input_with_extra(&vk, false, EXTRA_INFO);
    let _ = key_chord_send_input_with_extra(&vk, true, EXTRA_INFO);
    log::info!("XIAOMI VOICE shell-menu suppress dummy vk=0x{ALT_MENU_SUPPRESS_DUMMY_VK:02X}");
}

#[cfg(not(target_os = "windows"))]
fn inject_shell_menu_suppress_dummy() {}

/// 注入前松开不在和弦内、却仍按下的修饰键（SendInput 仅 KEYUP，非唤醒）。
#[cfg(not(target_os = "windows"))]
fn voice_sanitize_keyup(_vks: &[u16]) -> bool {
    false
}

/// 语音和弦注入 — **优先** WinUHid；不可用时互斥降级 SendInput（1.3.15）。
/// DOWN 锁定后端，UP 沿用同一后端，禁止双发。
/// F5 不在此中和：靠 gadget 清 0x003E + 会话 LL 吞 + 注入前 bump（见 VOICE_F5_SIMPLE_PLAN）。
fn inject_voice_chord(vks: &[u16], key_up: bool) -> bool {
    if vks.is_empty() {
        return false;
    }
    #[cfg(target_os = "windows")]
    {
        use crate::bridges::xiaomi::voice_inject::{
            voice_inject_backend, VoiceInjectBackend,
        };
        let vks = crate::bridges::xiaomi::voice_inject::normalize_voice_chord_vks(vks);
        if !key_up {
            recover_foreign_modifiers(&vks, voice_sanitize_keyup);
        }

        let backend = if key_up {
            match VOICE_INJECT_BACKEND_HELD.load(Ordering::Acquire) {
                VOICE_BACKEND_HID => VoiceInjectBackend::VirtualHid,
                VOICE_BACKEND_SENDINPUT => VoiceInjectBackend::SendInputFallback,
                _ => voice_inject_backend(
                    &vks,
                    crate::bridges::xiaomi::hid_injector::is_available(),
                ),
            }
        } else {
            let b = voice_inject_backend(
                &vks,
                crate::bridges::xiaomi::hid_injector::is_available(),
            );
            VOICE_INJECT_BACKEND_HELD.store(
                match b {
                    VoiceInjectBackend::VirtualHid => VOICE_BACKEND_HID,
                    VoiceInjectBackend::SendInputFallback => VOICE_BACKEND_SENDINPUT,
                },
                Ordering::Release,
            );
            b
        };

        match backend {
            VoiceInjectBackend::VirtualHid => {
                crate::bridges::xiaomi::key_log::note_virtual_hid_inject(&vks);
                if !key_up {
                    crate::bridges::xiaomi::key_log::set_virtual_hid_chord_held(Some(&vks));
                }
                let hid_ok = if !key_up {
                    crate::bridges::xiaomi::hid_injector::press_single(&vks).is_ok()
                } else {
                    // 与菜单键→Win 相同：分步松开 + 再发全零，避免 Win 位残留。
                    // 单报告一次清零时 Explorer 常吃掉 LWin UP。
                    match crate::bridges::xiaomi::hid_injector::release(&vks) {
                        Ok(()) => true,
                        Err(e) => {
                            log::error!("XIAOMI VOICE WinUHid release failed: {e}");
                            let _ = crate::bridges::xiaomi::hid_injector::release_all();
                            false
                        }
                    }
                };
                if key_up {
                    crate::bridges::xiaomi::key_log::set_virtual_hid_chord_held(None);
                    VOICE_INJECT_BACKEND_HELD.store(VOICE_BACKEND_NONE, Ordering::Release);
                    // 不再发 vkE8：微信/输入法会收到「空键」，污染热键观测。
                    // （旧逻辑：Win/Alt UP 后 dummy 防开始菜单；若菜单弹起再评估恢复。）
                    let cleared = recover_chord_modifiers(&vks, voice_sanitize_keyup);
                    if !hid_ok && cleared > 0 {
                        log::warn!(
                            "XIAOMI VOICE release recovered via sanitizer cleared={cleared} vks={vks:?}"
                        );
                        return true;
                    }
                }
                if hid_ok {
                    if !key_up {
                        crate::bridges::xiaomi::key_log::emit_mapped_outputs(&vks, true);
                    }
                    log::info!("XIAOMI VOICE inject via WinUHid key_up={key_up} vks={vks:?}");
                    return true;
                }
                if !key_up {
                    // DOWN 失败：解锁，避免后续 UP 误走 HID
                    VOICE_INJECT_BACKEND_HELD.store(VOICE_BACKEND_NONE, Ordering::Release);
                    log::error!("XIAOMI VOICE WinUHid inject failed key_up={key_up} vks={vks:?}");
                }
                false
            }
            VoiceInjectBackend::SendInputFallback => {
                // 互斥：本臂只用 key_chord_send_input_with_extra，不走虚拟键盘 API
                if !key_up {
                    log::warn!(
                        "XIAOMI VOICE inject DEGRADED SendInput (WinUHid unavailable) — 豆包/千问可能无效；请点「修复虚拟键盘」 vks={vks:?}"
                    );
                    crate::bridges::xiaomi::key_log::emit_mapped_outputs(&vks, true);
                    notify_voice_sendinput_degraded();
                }
                let ok = key_chord_send_input_with_extra(&vks, key_up, EXTRA_INFO);
                if key_up {
                    VOICE_INJECT_BACKEND_HELD.store(VOICE_BACKEND_NONE, Ordering::Release);
                    // 同 WinUHid 路径：不发 vkE8，避免微信收到空键
                    let _ = recover_chord_modifiers(&vks, voice_sanitize_keyup);
                }
                if ok {
                    log::info!(
                        "XIAOMI VOICE inject via SendInputFallback key_up={key_up} vks={vks:?}"
                    );
                } else if !key_up {
                    VOICE_INJECT_BACKEND_HELD.store(VOICE_BACKEND_NONE, Ordering::Release);
                    log::error!(
                        "XIAOMI VOICE SendInputFallback failed key_up={key_up} vks={vks:?}"
                    );
                }
                ok
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (vks, key_up);
        false
    }
}

/// 降级通知（节流）：WinUHid 不可用走 SendInput 时提示一次。
fn notify_voice_sendinput_degraded() {
    const GAP: Duration = Duration::from_secs(60);
    {
        let mut last = VOICE_SENDINPUT_DEGRADED_TOAST_LAST.lock();
        if let Some(t) = *last {
            if t.elapsed() < GAP {
                return;
            }
        }
        *last = Some(Instant::now());
    }
    let Some(app) = VOICE_HOOK_APP.lock().clone() else {
        return;
    };
    use tauri_plugin_notification::NotificationExt;
    if let Err(e) = app
        .notification()
        .builder()
        .title("虚拟键盘不可用，已降级")
        .body(
            "语音键暂用 SendInput（类似旧版）。微信或可用；豆包/千问常无效。请点「修复虚拟键盘」。",
        )
        .show()
    {
        log::warn!("voice SendInput degraded notification failed: {e}");
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
