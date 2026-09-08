//! T1 BLE 按键路径：Raw Input 匹配 BLE 设备 → 吞原生 → WinUHid 注入映射
//!
//! 与 USB `runtime.rs` 独立；语音优先由 ATVV（`ble_voice`）处理，HID 语音事件在 ATVV
//! 已订阅时跳过，避免双发。

use crate::bridges::t1::consumer_raw_input::{
    device_matches, ConsumerRawEvent, ConsumerRawInput,
};
use crate::bridges::t1::inject::{tap_single_vk, tap_vks};
use crate::bridges::t1::key_diag;
use crate::bridges::t1::mapping::{
    build_event_to_button, default_event_aliases, resolve_button,
};
use crate::bridges::t1::native_suppress;
use crate::bridges::shared::hid_injector;
use crate::bridges::shared::host_hooks as special_keys;
use crate::config::manager::{ConfigManager, DeviceConfig, KeyAction};
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::LazyLock;
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

/// BLE 设备路径匹配 token（Windows DeviceInterface / Raw Input 名称）
pub const BLE_DEVICE_MATCH: &[&str] = &[
    "dev_vid&01620a_pid&0407",
    "01620a",
    // 部分 HOGP 路径写成 VID_1620A（无前导 0）
    "1620a",
    "pid&0407",
    "pid_0407",
    "t1-remote",
    "t1_remote",
];

static KEYS_RUNNING: AtomicBool = AtomicBool::new(false);
static RAW: Mutex<Option<ConsumerRawInput>> = Mutex::new(None);

#[derive(Clone)]
struct BleLlGateCtx {
    app: AppHandle,
}

static BLE_LL_GATE_CTX: LazyLock<Mutex<Option<BleLlGateCtx>>> =
    LazyLock::new(|| Mutex::new(None));

static LAST_INJECT: LazyLock<Mutex<HashMap<String, Instant>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 本会话已确认的 T1 设备指纹（Raw Input 路径归一化）。
/// 方向/OK 等常用键**只**在 token 命中或指纹命中时注入；不确定则回放原生（保实体键盘）。
static REMEMBERED_T1_AFFINITY: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

const DEDUPE_MS: u64 = 80;

/// 路径是否像 T1 BLE HID（单测缝合点）
pub fn looks_like_t1_ble_device(device_name: &str) -> bool {
    let tokens: Vec<String> = BLE_DEVICE_MATCH.iter().map(|s| (*s).to_string()).collect();
    device_matches(device_name, &tokens)
}

/// 从 Raw 设备路径提取亲和指纹（同遥控的 keyboard/consumer 接口常共享尾段实例 ID）。
fn device_affinity_key(device_name: &str) -> Option<String> {
    let low = device_name.trim().to_ascii_lowercase();
    if low.is_empty() {
        return None;
    }
    // 优先：路径末段里连续 ≥8 位十六进制（常见 BLE 地址 / 实例 ID）
    if let Some(seg) = low.rsplit(['#', '\\']).find(|s| !s.is_empty()) {
        let hex: String = seg.chars().filter(|c| c.is_ascii_hexdigit()).collect();
        if hex.len() >= 8 {
            return Some(hex);
        }
    }
    // 回退：整路径（已确认 T1 时仍可区分实体键盘）
    Some(low)
}

fn remember_t1_device(device_name: &str) {
    let Some(key) = device_affinity_key(device_name) else {
        return;
    };
    if REMEMBERED_T1_AFFINITY.lock().insert(key.clone()) {
        log::info!("T1 BLE remember device affinity={key}");
    }
}

fn is_remembered_t1_device(device_name: &str) -> bool {
    let Some(key) = device_affinity_key(device_name) else {
        return false;
    };
    REMEMBERED_T1_AFFINITY.lock().contains(&key)
}

fn clear_remembered_t1_devices() {
    REMEMBERED_T1_AFFINITY.lock().clear();
}

fn has_remembered_t1() -> bool {
    !REMEMBERED_T1_AFFINITY.lock().is_empty()
}

/// BLE 已连接地址写入亲和指纹（HOGP 键盘路径常带同一蓝牙地址尾段）。
pub fn remember_ble_address(address: &str) {
    let compact: String = address
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect::<String>()
        .to_ascii_lowercase();
    if compact.len() < 8 {
        return;
    }
    if REMEMBERED_T1_AFFINITY.lock().insert(compact.clone()) {
        log::info!("T1 BLE remember address affinity={compact}");
    }
}

/// Consumer / 冷门侧效应：仅删除·Home（音量/静音/方向永久透传）。
fn is_force_handle_side_effect(button: Option<&str>) -> bool {
    matches!(button, Some("home" | "delete"))
}

fn is_face_remap_button(button: Option<&str>) -> bool {
    let _ = button;
    false
}

fn is_native_passthrough_button(button_id: &str) -> bool {
    matches!(
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
    )
}

/// 侧效应 soft-handle：空路径 / 已像 T1 才允许；带其它 VID 的当实体媒体键（不记指纹）。
fn may_soft_handle_side_effect(device_name: &str) -> bool {
    let low = device_name.trim().to_ascii_lowercase();
    if low.is_empty() || looks_like_t1_ble_device(device_name) {
        return true;
    }
    // 有 VID/PID 却不像 T1 → 实体键盘媒体键，绝不当遥控
    if low.contains("vid_") || low.contains("vid&") {
        return false;
    }
    // 无 VID 残缺路径：允许处理，但不在此写入指纹（避免污染）
    true
}

/// 明确的实体键盘（有其它 VID，或 ACPI/i8042 笔记本键盘）。这些必须回放原生方向/回车。
fn is_clearly_foreign_keyboard(device_name: &str) -> bool {
    let low = device_name.trim().to_ascii_lowercase();
    if low.is_empty() {
        return false;
    }
    if looks_like_t1_ble_device(device_name) || is_remembered_t1_device(device_name) {
        return false;
    }
    if low.contains("acpi") || low.contains("pnp0303") || low.contains("i8042") {
        return true;
    }
    if low.contains("vid_") || low.contains("vid&") {
        return !(low.contains("1620a")
            || low.contains("01620a")
            || low.contains("1915")
            || low.contains("pid_1025")
            || low.contains("pid&0407")
            || low.contains("pid_0407"));
    }
    false
}

/// 方向/OK：正向识别为 T1，或（已连接 T1 且本键已 LL 暂挂且设备不是明确实体键盘）。
fn may_handle_face_key(device_name: &str, event_id: &str) -> bool {
    if looks_like_t1_ble_device(device_name) || is_remembered_t1_device(device_name) {
        return true;
    }
    // 路径任意位置含已登记的蓝牙地址
    let low = device_name.to_ascii_lowercase();
    if REMEMBERED_T1_AFFINITY
        .lock()
        .iter()
        .any(|k| k.len() >= 8 && low.contains(k))
    {
        return true;
    }
    // 已连上 T1：非明确实体键盘的暂挂方向/OK 按遥控注入（避免记事本只收到原生回放）
    if has_remembered_t1()
        && !is_clearly_foreign_keyboard(device_name)
        && parse_kbd_vk(event_id).is_some_and(|vk| native_suppress::source_decision_pending(vk))
    {
        return true;
    }
    false
}

fn parse_kbd_vk(event_id: &str) -> Option<u16> {
    let rest = event_id.trim().strip_prefix("kbd:VK_")?;
    let hex = rest.split(|c| c == ':' || c == '-').next()?;
    u16::from_str_radix(hex, 16).ok()
}

fn binding_vks(config: &DeviceConfig, button_id: &str) -> Vec<u16> {
    if is_native_passthrough_button(button_id) {
        return Vec::new();
    }
    match config.button_bindings.get(button_id) {
        Some(KeyAction::SingleKey(vk)) => vec![*vk],
        Some(KeyAction::ComboKey(vks)) => vks.clone(),
        _ => Vec::new(),
    }
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

/// L0 修复后刷新闸门（Consumer 已恢复启用时按配置重算媒体/方向闸门）。
pub fn resync_gates_after_l0() {
    if let Some(ctx) = BLE_LL_GATE_CTX.lock().clone() {
        if let Some(cfg) = ctx
            .app
            .try_state::<ConfigManager>()
            .and_then(|m| m.get_device_config("t1_ble").ok())
        {
            sync_media_gates(&cfg);
        }
    }
}

fn format_vks_label(vks: &[u16]) -> String {
    key_diag::format_vks_label(vks)
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

fn should_skip_duplicate(button_id: &str) -> bool {
    let mut map = LAST_INJECT.lock();
    let now = Instant::now();
    if let Some(t) = map.get(button_id) {
        if now.duration_since(*t) < Duration::from_millis(DEDUPE_MS) {
            return true;
        }
    }
    map.insert(button_id.to_string(), now);
    false
}

fn inject_mapped_keys(vks: &[u16], hold_ms: u64) -> bool {
    // 与 hold-suppress 重叠、或媒体/浏览器 VK（WinUHid 不支持）→ SendInput+EXTRA_INFO
    let force_sendinput =
        native_suppress::held_intersects(vks) || native_suppress::vks_need_sendinput(vks);
    let ok = if force_sendinput {
        log::debug!("T1 BLE map: SendInput+EXTRA_INFO vks={vks:?}");
        native_suppress::allow_pass_vks(vks);
        let tapped = if vks.len() == 1 {
            tap_single_vk(vks[0], hold_ms)
        } else {
            tap_vks(vks, hold_ms)
        };
        if crate::bridges::t1::inject::chord_has_modifier(vks) {
            crate::bridges::t1::inject::panic_clear_all_modifiers("ble_map_sendinput_mods");
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

fn handle_button(button_id: &str, config: &DeviceConfig, app: &AppHandle) {
    if is_native_passthrough_button(button_id) {
        return;
    }
    let Some(action) = config.button_bindings.get(button_id) else {
        key_diag::emit_ble(
            app,
            "inject",
            Some(button_id),
            None,
            None,
            None,
            &[],
            &format!("BLE 映射无效：{button_id} 未绑定"),
        );
        return;
    };
    let vks: Vec<u16> = match action {
        KeyAction::SingleKey(vk) => vec![*vk],
        KeyAction::ComboKey(vks) => vks.clone(),
        KeyAction::None => {
            key_diag::emit_ble(
                app,
                "inject",
                Some(button_id),
                None,
                None,
                None,
                &[],
                &format!("BLE 映射空：{button_id}"),
            );
            return;
        }
        KeyAction::TextInput(_) | KeyAction::LaunchApp(_) => {
            key_diag::emit_ble(
                app,
                "inject",
                Some(button_id),
                None,
                None,
                None,
                &[],
                &format!("BLE 映射跳过：{button_id} 类型暂不支持"),
            );
            return;
        }
    };
    if vks.is_empty() {
        key_diag::emit_ble(
            app,
            "inject",
            Some(button_id),
            None,
            None,
            None,
            &[],
            &format!("BLE 映射跳过：{button_id} VK 列表空"),
        );
        return;
    }
    let ok = inject_mapped_keys(&vks, hold_ms_for(&vks));
    let label = format_vks_label(&vks);
    let msg = if ok {
        format!("BLE 注入 {button_id} → {label} ✓ winuhid={}", hid_injector::is_available())
    } else {
        format!("BLE 注入 {button_id} → {label} 失败 winuhid={}", hid_injector::is_available())
    };
    key_diag::emit_ble(app, "inject", Some(button_id), None, None, None, &vks, &msg);
}

/// 可能唤起开始菜单 / 搜索 / Copilot 的原生事件（纯函数，供单测）
pub fn classify_start_menu_risk(event_id: &str) -> Option<&'static str> {
    let key = event_id.trim().to_ascii_uppercase();
    if key.contains("02-21-02") {
        return Some("AC Search 0x0221 → 会开 Windows 搜索/开始菜单");
    }
    if key.contains("CF-00") || key.contains("00-CF") {
        return Some("Voice Command 0xCF（Windows 通常不弹菜单）");
    }
    if let Some(vk) = parse_kbd_vk(event_id) {
        return match vk {
            0x5B => Some("左 Win → 会弹开始菜单"),
            0x5C => Some("右 Win → 会弹开始菜单"),
            0x5D => Some("Apps 菜单键"),
            0x86 => Some("F23 → 常见 Copilot 和弦"),
            0xAA => Some("浏览器搜索"),
            0xAC => Some("Browser Home"),
            _ => None,
        };
    }
    None
}

#[cfg(test)]
fn format_native_probe(event_id: &str, pressed: bool, button: Option<&str>) -> String {
    let dir = if pressed { "↓" } else { "↑" };
    let btn = match button {
        Some(b) => format!(" → {b}"),
        None => " → 未映射".into(),
    };
    match classify_start_menu_risk(event_id) {
        Some(risk) => format!("{event_id} {dir}{btn} · {risk}"),
        None => format!("{event_id} {dir}{btn}"),
    }
}

fn emit_native_probe(
    app: &AppHandle,
    ev: &ConsumerRawEvent,
    button: Option<&str>,
    map_vks: &[u16],
    extra: Option<&str>,
) {
    if ev.event_id.eq_ignore_ascii_case("hid:02-00-00") {
        return;
    }
    let message = key_diag::format_from_event(ev, button, map_vks, extra);
    key_diag::emit_ble(app, "native", button, Some(ev), None, None, map_vks, &message);
}

fn dispatch_event(app: &AppHandle, ev: &ConsumerRawEvent, event_to_button: &HashMap<String, String>) {
    if crate::bridges::shared::shortcut_capture::is_swallow_active() {
        return;
    }
    let tokens: Vec<String> = BLE_DEVICE_MATCH.iter().map(|s| (*s).to_string()).collect();
    let button_preview = resolve_button(&ev.event_id, event_to_button);
    if device_matches(&ev.device_name, &tokens) {
        remember_t1_device(&ev.device_name);
    } else {
        // 业界标准：LL 无设备源；方向/OK 必须 Raw 正向识别，不确定则回放原生。
        // 删除/Home/静音/音量：冷门侧效应，可在残缺路径下 soft-handle（不误伤实体方向/回车）。
        let soft_side = KEYS_RUNNING.load(Ordering::SeqCst)
            && ev.pressed
            && is_force_handle_side_effect(button_preview.as_deref())
            && may_soft_handle_side_effect(&ev.device_name);
        let soft_face = KEYS_RUNNING.load(Ordering::SeqCst)
            && ev.pressed
            && is_face_remap_button(button_preview.as_deref())
            && may_handle_face_key(&ev.device_name, &ev.event_id);
        if soft_side && looks_like_t1_ble_device(&ev.device_name) {
            remember_t1_device(&ev.device_name);
        }
        if soft_face {
            remember_t1_device(&ev.device_name);
        }
        if !(soft_side || soft_face) {
            if let Some(vk) = parse_kbd_vk(&ev.event_id) {
                native_suppress::on_foreign_keyboard(vk, ev.pressed);
                // 真实键盘按下重映射方向/OK（已被 LL 吞并暂挂）：直接回放原生键。
                if ev.pressed
                    && native_suppress::source_decision_pending(vk)
                    && !native_suppress::replay_foreign_gate_vk(vk)
                {
                    log::warn!("T1 BLE foreign replay failed for vk=0x{vk:02X}");
                }
            }
            if looks_like_t1_ble_device(&ev.device_name)
                && ev.pressed
                && !ev.event_id.eq_ignore_ascii_case("hid:02-00-00")
            {
                // 像 T1 但本事件未处理：仍记指纹，供同设备后续方向/OK 正向命中
                remember_t1_device(&ev.device_name);
                emit_native_probe(
                    app,
                    ev,
                    None,
                    &[],
                    Some("像 BLE T1 但本事件未作映射处理"),
                );
            }
            return;
        }
        log::info!(
            "T1 BLE soft-match handle button={:?} event={} device={}",
            button_preview,
            ev.event_id,
            if ev.device_name.is_empty() {
                "?"
            } else {
                &ev.device_name
            }
        );
    }

    let button_id = resolve_button(&ev.event_id, event_to_button);
    // T1 BLE 设备命中：消费来源暂挂，按正常映射流程走。
    if let Some(vk) = parse_kbd_vk(&ev.event_id) {
        let _ = crate::bridges::t1::native_suppress::consume_source_pending(vk);
    }
    let config = app
        .try_state::<ConfigManager>()
        .and_then(|m| m.get_device_config("t1_ble").ok());
    let target_vks = match (&button_id, &config) {
        (Some(bid), Some(cfg)) => binding_vks(cfg, bid),
        _ => Vec::new(),
    };

    let atvv_owns_voice = button_id.as_deref() == Some("voice")
        && crate::bridges::t1::ble_host::atvv_ok();
    let probe_extra = if atvv_owns_voice {
        Some("HID 语音：仅吞 Search（开/关麦+注入由 ATVV START_SEARCH）")
    } else if button_id.as_deref() == Some("voice") {
        Some("HID 语音：无 ATVV，本路径注入")
    } else {
        None
    };
    emit_native_probe(app, ev, button_id.as_deref(), &target_vks, probe_extra);

    let Some(button_id) = button_id else {
        return;
    };

    // 语音 HID（含 AC Search）：ATVV 在线时只吞 Search，禁止在此注入——
    // 否则关麦抑制期内 HID 仍会 Toggle 快捷键，造成「麦关了微信却开了」。
    if button_id == "voice" {
        crate::bridges::t1::native_suppress::on_ac_search_hid_seen();
        if !ev.pressed {
            crate::bridges::t1::native_suppress::release_voice_browser_search();
            return;
        }
        if atvv_owns_voice {
            return;
        }
        crate::bridges::t1::ble_voice::on_remote_press(app);
        return;
    }

    if config.is_none() {
        key_diag::emit_ble(
            app,
            "key",
            Some(&button_id),
            Some(ev),
            None,
            None,
            &[],
            &format!("BLE 忽略 {button_id}：配置不可用"),
        );
        return;
    }

    let native_vk = parse_kbd_vk(&ev.event_id);

    if !ev.pressed {
        native_suppress::apply_native_release_policy(native_vk);
        let _ = hid_injector::release_all();
        return;
    }

    if let Some(nv) = native_vk {
        if native_suppress::should_hold_suppress_native(nv, &target_vks) {
            let _ = crate::bridges::t1::inject::release_vks(&[nv]);
            key_diag::emit_ble(
                app,
                "key",
                Some(&button_id),
                Some(ev),
                None,
                None,
                &target_vks,
                &format!("吞原生 vk=0x{nv:02X}"),
            );
        }
    }
    native_suppress::apply_native_press_policy(&button_id, native_vk, &target_vks);

    // 同键 / 未绑定 / 音量同键：只透传，不注入（避免 OK→Enter 双发、音量空注入）
    if native_suppress::is_passthrough_binding(&button_id, native_vk, &target_vks) {
        key_diag::emit_ble(
            app,
            "key",
            Some(&button_id),
            Some(ev),
            None,
            None,
            &target_vks,
            &format!("同键/未绑定透传 {button_id}（不注入）"),
        );
        return;
    }

    if should_skip_duplicate(&button_id) {
        key_diag::emit_ble(
            app,
            "key",
            Some(&button_id),
            Some(ev),
            None,
            None,
            &target_vks,
            &format!("BLE 去重跳过 {button_id}"),
        );
        return;
    }

    let app2 = app.clone();
    let bid = button_id.clone();
    thread::spawn(move || {
        if let Some(cfg) = app2
            .try_state::<ConfigManager>()
            .and_then(|m| m.get_device_config("t1_ble").ok())
        {
            handle_button(&bid, &cfg, &app2);
        }
    });
}

/// RegisterHotKey 吃掉 0xAA 后：与 HID 同路径（arm+看门狗+mop-up）；注入由 ATVV/HID 负责。
pub fn on_browser_search_hotkey() {
    crate::bridges::t1::native_suppress::on_ac_search_hid_seen();
}

/// LL 闸门：侧效应键 + **重映射方向/OK** 在 Raw 丢按下时补映射。
/// 同键/未绑定的方向不进闸门 → 实体键盘同 VK 仍可用。
pub fn on_ll_gate_keydown(vk: u16) {
    if !native_suppress::is_gate_vk(vk) {
        return;
    }
    // 进入补映射路径即代表来源已由调用方裁决，清理来源暂挂，避免 Raw 再回放。
    let _ = native_suppress::consume_source_pending(vk);
    let Some(ctx) = BLE_LL_GATE_CTX.lock().clone() else {
        return;
    };
    if !KEYS_RUNNING.load(Ordering::SeqCst) {
        return;
    }
    let event_id = format!("kbd:VK_{vk:02X}");
    let event_to_button = build_event_to_button(&default_event_aliases());
    let button_id = resolve_button(&event_id, &event_to_button);
    let config = ctx
        .app
        .try_state::<ConfigManager>()
        .and_then(|m| m.get_device_config("t1_ble").ok());
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
        Some("BLE LL 闸门"),
    );
    key_diag::emit_ble(
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
        // 只吞 0xAA，不从 LL 再开语音（避免与 ATVV START_SEARCH 双发）
        crate::bridges::t1::native_suppress::on_browser_search_ll_swallowed();
        return;
    }
    if button_id == "home" {
        crate::bridges::t1::native_suppress::on_ac_home_hid_seen();
    }
    if button_id == "menu" {
        crate::bridges::t1::native_suppress::arm_menu_apps_key();
    }
    let Some(config) = config else {
        return;
    };
    native_suppress::arm_for_button(&button_id, Some(vk), &target_vks);
    if native_suppress::is_passthrough_binding(&button_id, Some(vk), &target_vks) {
        key_diag::emit_ble(
            &ctx.app,
            "ll",
            Some(&button_id),
            None,
            Some(&event_id),
            Some(true),
            &target_vks,
            &format!("BLE LL 同键/未绑定透传 {button_id}"),
        );
        return;
    }
    if should_skip_duplicate(&button_id)
        || native_suppress::should_skip_gate_inject(&button_id)
    {
        key_diag::emit_ble(
            &ctx.app,
            "ll",
            Some(&button_id),
            None,
            Some(&event_id),
            Some(true),
            &target_vks,
            &format!("BLE LL 去重跳过 {button_id}"),
        );
        return;
    }
    handle_button(&button_id, &config, &ctx.app);
}

pub fn start(app: &AppHandle) -> Result<(), String> {
    if KEYS_RUNNING.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    clear_remembered_t1_devices();
    special_keys::ensure_hook_for_capture();
    // 吞键原因位由 ble_runtime 持有；此处不重复 arm。
    if let Some(cfg) = app
        .try_state::<ConfigManager>()
        .and_then(|m| m.get_device_config("t1_ble").ok())
    {
        sync_media_gates(&cfg);
        if let Some(addr) = cfg.bluetooth_address.as_deref() {
            remember_ble_address(addr);
        }
    }
    *BLE_LL_GATE_CTX.lock() = Some(BleLlGateCtx { app: app.clone() });

    let mut raw = ConsumerRawInput::new(true);
    let event_to_button = build_event_to_button(&default_event_aliases());
    let app_cb = app.clone();
    raw.start(move |ev: ConsumerRawEvent| {
        dispatch_event(&app_cb, &ev, &event_to_button);
    })
    .map_err(|e| {
        KEYS_RUNNING.store(false, Ordering::SeqCst);
        *BLE_LL_GATE_CTX.lock() = None;
        e
    })?;
    *RAW.lock() = Some(raw);
    log::info!(
        "T1 BLE key listener started (Raw Input + WinUHid); swallow gate={}",
        native_suppress::is_enabled()
    );
    Ok(())
}

pub fn stop(_app: &AppHandle) {
    KEYS_RUNNING.store(false, Ordering::SeqCst);
    *BLE_LL_GATE_CTX.lock() = None;
    clear_remembered_t1_devices();
    if let Some(mut raw) = RAW.lock().take() {
        raw.stop();
    }
    native_suppress::clear_hold_suppress();
    crate::bridges::t1::inject::panic_clear_all_modifiers("ble_keys_stop");
    log::info!("T1 BLE key listener stopped");
}

pub fn is_running() -> bool {
    KEYS_RUNNING.load(Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ble_path_matches_hardware_token() {
        let path = r"\\?\BTHLEDevice#{00001812-...}#dev_vid&01620a_pid&0407#12ac2c46c4ab";
        assert!(looks_like_t1_ble_device(path));
    }

    #[test]
    fn ble_path_matches_vid_without_leading_zero() {
        let path = r"\\?\HID#VID_1620A&PID_0407&MI_01#7&abc";
        assert!(looks_like_t1_ble_device(path));
    }

    #[test]
    fn usb_vid_pid_not_matched_as_ble() {
        let path = r"\\?\HID#VID_1915&PID_1025&MI_01#7&abc";
        assert!(!looks_like_t1_ble_device(path));
    }

    #[test]
    fn name_t1_remote_matches() {
        assert!(looks_like_t1_ble_device("T1-Remote"));
    }

    #[test]
    fn affinity_key_prefers_trailing_hex_instance() {
        let path = r"\\?\BTHLEDevice#{00001812-...}#dev_vid&01620a_pid&0407#12ac2c46c4ab";
        assert_eq!(
            device_affinity_key(path).as_deref(),
            Some("12ac2c46c4ab")
        );
    }

    #[test]
    fn face_key_requires_positive_t1_identity() {
        clear_remembered_t1_devices();
        let laptop = r"\\?\ACPI#PNP0303#4&1234abcd&0";
        let logitech = r"\\?\HID#VID_046D&PID_C52B&MI_00#7&abc";
        assert!(!may_handle_face_key(laptop, "kbd:VK_26"));
        assert!(!may_handle_face_key(logitech, "kbd:VK_26"));
        assert!(!may_handle_face_key("", "kbd:VK_26"));
        assert!(is_clearly_foreign_keyboard(laptop));
        assert!(is_clearly_foreign_keyboard(logitech));

        let t1 = r"\\?\BTHLEDevice#{00001812}#dev_vid&01620a_pid&0407#12ac2c46c4ab";
        assert!(may_handle_face_key(t1, "kbd:VK_26"));
        remember_t1_device(t1);
        // 同实例、不同接口前缀：指纹命中即可（模拟 keyboard TLC 路径差异）
        let t1_kbd = r"\\?\HID#...#12ac2c46c4ab";
        assert!(may_handle_face_key(t1_kbd, "kbd:VK_26"));
        clear_remembered_t1_devices();
        assert!(!may_handle_face_key(t1_kbd, "kbd:VK_26"));
    }

    #[test]
    fn face_key_uses_ble_address_when_pending() {
        clear_remembered_t1_devices();
        remember_ble_address("12:AC:2C:46:C4:AB");
        native_suppress::set_enabled(true);
        native_suppress::defer_gate_source(0x26, true);
        // 无 VID、非 ACPI：已登记地址 + LL 暂挂 → 按 T1 注入（记事本应收到映射）
        assert!(may_handle_face_key(r"\\?\HID#unknown#iface0", "kbd:VK_26"));
        // 笔记本 ACPI 仍回放原生
        assert!(!may_handle_face_key(r"\\?\ACPI#PNP0303#4&abcd", "kbd:VK_26"));
        let _ = native_suppress::consume_source_pending(0x26);
        native_suppress::set_enabled(false);
        clear_remembered_t1_devices();
    }

    #[test]
    fn side_effect_soft_rejects_foreign_vid() {
        assert!(may_soft_handle_side_effect(""));
        assert!(may_soft_handle_side_effect(
            r"\\?\BTHLEDevice#dev_vid&01620a_pid&0407#12ac"
        ));
        assert!(!may_soft_handle_side_effect(
            r"\\?\HID#VID_046D&PID_C52B&MI_01"
        ));
    }

    #[test]
    fn ble_search_hid_flagged() {
        assert_eq!(
            classify_start_menu_risk("hid:02-21-02"),
            Some("AC Search 0x0221 → 会开 Windows 搜索/开始菜单")
        );
    }

    #[test]
    fn voice_cf_classified_as_consumer_not_win() {
        assert_eq!(
            classify_start_menu_risk("hid:02-CF-00"),
            Some("Voice Command 0xCF（Windows 通常不弹菜单）")
        );
        assert_eq!(
            classify_start_menu_risk("hid:01-CF-00-00"),
            Some("Voice Command 0xCF（Windows 通常不弹菜单）")
        );
    }

    #[test]
    fn win_and_copilot_flagged() {
        assert_eq!(
            classify_start_menu_risk("kbd:VK_5B"),
            Some("左 Win → 会弹开始菜单")
        );
        assert_eq!(
            classify_start_menu_risk("kbd:VK_86"),
            Some("F23 → 常见 Copilot 和弦")
        );
        assert!(classify_start_menu_risk("kbd:VK_0D").is_none());
    }

    #[test]
    fn native_probe_labels_unmapped_win() {
        let s = format_native_probe("kbd:VK_5B", true, None);
        assert!(s.contains("未映射"));
        assert!(s.contains("左 Win"));
    }
}
