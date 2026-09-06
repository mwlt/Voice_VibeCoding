//! T1 BLE 语音快捷键注入（独立于 USB runtime 闩锁，避免互相干扰）

use crate::bridges::t1::inject::{names_to_vks, press_vks, release_vks};
use crate::bridges::xiaomi::hid_injector;
use crate::config::manager::{ConfigManager, DeviceConfig, KeyAction, TriggerMode};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

static VOICE_HELD: AtomicBool = AtomicBool::new(false);
static HELD_VKS: Mutex<Vec<u16>> = Mutex::new(Vec::new());
static LAST_PRESS: Mutex<Option<Instant>> = Mutex::new(None);
/// HID AC Search 与 ATVV START_SEARCH 常跨线程前后脚；关搜索同步路径可达 ~1s。
/// 去重窗必须盖住「慢 dismiss 后第二路才进锁」的双发，否则 Ctrl+Win 会点两下把微信开了又关。
pub const VOICE_DUP_EVENT_MS: u64 = 900;
static VOICE_INJECT: Mutex<()> = Mutex::new(());

pub fn is_voice_held() -> bool {
    VOICE_HELD.load(Ordering::SeqCst)
}

pub fn is_same_voice_press(elapsed: Duration) -> bool {
    elapsed < Duration::from_millis(VOICE_DUP_EVENT_MS)
}

/// 是否仍在「同一次物理按」去重窗内（只读，不刷新时间戳）。
pub fn is_within_voice_dup_window() -> bool {
    let g = LAST_PRESS.lock();
    match *g {
        Some(prev) => is_same_voice_press(Instant::now().duration_since(prev)),
        None => false,
    }
}

fn same_physical_press() -> bool {
    let mut g = LAST_PRESS.lock();
    let now = Instant::now();
    if let Some(prev) = *g {
        if is_same_voice_press(now.duration_since(prev)) {
            return true;
        }
    }
    *g = Some(now);
    false
}

/// 注入结束后刷新去重起点（允许紧接着再点）
fn mark_press_handled() {
    *LAST_PRESS.lock() = Some(Instant::now());
}

/// 与 USB `runtime::resolve_voice_vks` 同规则，但独立实现不共用函数
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

fn load_t1_config(app: &AppHandle) -> Option<DeviceConfig> {
    app.try_state::<ConfigManager>()
        .and_then(|m| m.get_device_config("t1").ok())
}

fn emit(app: &AppHandle, message: &str) {
    let _ = app.emit(
        "t1-ble",
        serde_json::json!({ "phase": "voice", "message": message }),
    );
}

pub fn on_remote_press(app: &AppHandle) {
    // 先读配置（快）；去重必须在任何慢路径（同步关搜索）之前，否则 HID/START_SEARCH
    // 会各注入一次 Ctrl+Win，微信 Toggle 变成「开了又关」。
    let Some(cfg) = load_t1_config(app) else {
        emit(app, "BLE 语音：无法读取配置");
        return;
    };
    if cfg.voice_shortcut_enabled == false {
        emit(app, "BLE 语音：快捷键已关闭");
        return;
    }
    let vks = resolve_voice_vks(&cfg);
    if vks.is_empty() {
        emit(app, "BLE 语音：未配置快捷键");
        return;
    }

    let _inject = VOICE_INJECT.lock();
    if same_physical_press() {
        crate::bridges::t1::native_suppress::arm_voice_browser_search();
        crate::bridges::t1::native_suppress::dismiss_windows_search_async(false);
        log::info!("T1 BLE voice same physical press (skip duplicate inject)");
        emit(app, "BLE 语音：同一次按下（忽略重复 START_SEARCH/AUDIO_START）");
        return;
    }

    // 去重通过后再吞 0xAA；异步关搜索，勿在注入前同步 dismiss（会拉长双发窗口）
    crate::bridges::t1::native_suppress::arm_voice_browser_search();
    crate::bridges::t1::native_suppress::dismiss_windows_search_async(false);

    let hold = voice_should_latch(&vks, cfg.trigger_mode.clone());

    if hold {
        // 与 USB 一致：第一次按下闩锁，第二次按下才松开。AUDIO_STOP 不能松键。
        if VOICE_HELD.load(Ordering::SeqCst) {
            end_hold_latch(app, "BLE 语音结束（第二次按）");
            crate::bridges::t1::native_suppress::release_voice_browser_search();
            // 键已松开：允许 Esc 收开始菜单/搜索
            crate::bridges::t1::native_suppress::dismiss_windows_search_async(true);
            mark_press_handled();
            return;
        }
        // 先按下映射键，再处理音频 / 关搜索（避免 BR Search 抢在组合键之前）
        // 禁止 press() 分步回退（会先露出 Win/Ctrl）。
        crate::bridges::t1::inject::panic_clear_all_modifiers("voice_latch_pre");
        crate::bridges::t1::native_suppress::allow_pass_vks(&vks);
        VOICE_HELD.store(true, Ordering::SeqCst);
        *HELD_VKS.lock() = vks.clone();
        let ok = if hid_injector::is_available() {
            match hid_injector::press_single(&vks) {
                Ok(()) => true,
                Err(e) => {
                    log::warn!("T1 BLE voice latch press_single failed: {e}");
                    false
                }
            }
        } else {
            press_vks(&vks)
        };
        if !ok {
            VOICE_HELD.store(false, Ordering::SeqCst);
            HELD_VKS.lock().clear();
            crate::bridges::t1::inject::panic_clear_all_modifiers("voice_latch_press_fail");
            crate::bridges::t1::native_suppress::release_voice_browser_search();
        }
        // 映射后：键若仍按住（闩锁）只关窗不 Esc；点按路径在下面单独 true
        crate::bridges::t1::native_suppress::arm_voice_browser_search();
        crate::bridges::t1::native_suppress::dismiss_windows_search_async(false);
        mark_press_handled();
        let _ = crate::audio::pcm_router::ensure_audio_router_process();
        crate::bridges::t1::ble_pcm::warmup_async();
        emit(
            app,
            if ok {
                "BLE 语音开始（闩锁，再按一次结束）"
            } else {
                "BLE 语音开始失败"
            },
        );
    } else {
        if VOICE_HELD.load(Ordering::SeqCst) {
            end_hold_latch(app, "BLE 语音：清除残留闩锁");
        }
        let hold_ms = if vks.iter().any(|&vk| matches!(vk, 0x5B | 0x5C)) {
            120
        } else if vks.iter().any(|&vk| matches!(vk, 0x12 | 0xA4 | 0xA5)) {
            100
        } else {
            70
        };
        // 先完整点按映射键，再关搜索 / 开音频（Win+Alt 必须原子报告，勿分步先发 Win）
        crate::bridges::t1::native_suppress::allow_pass_vks(&vks);
        let ok = crate::bridges::t1::inject::voice_chord_tap(&vks, hold_ms);
        if ok {
            log::info!(
                "T1 BLE voice mapped tap ok vks={}",
                vks.iter()
                    .map(|v| format!("0x{v:02X}"))
                    .collect::<Vec<_>>()
                    .join("+")
            );
        } else {
            log::warn!("T1 BLE voice mapped tap FAILED");
        }
        crate::bridges::t1::native_suppress::release_voice_browser_search();
        crate::bridges::t1::native_suppress::arm_voice_browser_search();
        // Win 和弦：勿立刻 Esc（易在 Alt 菜单态下乱打）；只关 SearchHost
        let allow_esc = !vks.iter().any(|&vk| matches!(vk, 0x5B | 0x5C));
        crate::bridges::t1::native_suppress::dismiss_windows_search_async(allow_esc);
        mark_press_handled();
        let _ = crate::audio::pcm_router::ensure_audio_router_process();
        crate::bridges::t1::ble_pcm::warmup_async();
        emit(
            app,
            if ok {
                "BLE 语音点按快捷键"
            } else {
                "BLE 语音点按失败"
            },
        );
    }
}

/// AUDIO_STOP / 物理抬起：只收 PCM，不松闩锁快捷键（对齐 USB 忽略脉冲抬起）
pub fn on_remote_release(app: &AppHandle) {
    crate::bridges::t1::ble_pcm::end_session();
    let _ = app;
}

fn end_hold_latch(app: &AppHandle, ok_msg: &str) {
    if !VOICE_HELD.swap(false, Ordering::SeqCst) {
        return;
    }
    let vks = HELD_VKS.lock().clone();
    HELD_VKS.lock().clear();
    crate::bridges::t1::native_suppress::release_voice_browser_search();
    if vks.is_empty() {
        return;
    }
    let ok = if hid_injector::is_available() {
        let _ = hid_injector::release(&vks);
        let _ = hid_injector::release_all();
        true
    } else {
        release_vks(&vks)
    };
    crate::bridges::t1::inject::panic_clear_all_modifiers("voice_latch_end");
    emit(
        app,
        if ok {
            ok_msg
        } else {
            "BLE 语音结束失败"
        },
    );
}


pub fn force_release(app: &AppHandle) {
    crate::bridges::t1::ble_pcm::end_session();
    end_hold_latch(app, "BLE 语音结束（断开）");
    crate::bridges::t1::native_suppress::release_voice_browser_search();
    crate::bridges::t1::inject::panic_clear_all_modifiers("voice_force_release");
}

fn voice_should_latch(vks: &[u16], mode: TriggerMode) -> bool {
    !crate::bridges::t1::inject::mapped_voice_is_tap(vks) && mode == TriggerMode::Hold
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn win_combo_always_taps() {
        // 左 Ctrl+左 Win 等：每次点击完整 down+up，不闩锁
        assert!(!voice_should_latch(&[0xA2, 0x5B], TriggerMode::Toggle));
        assert!(!voice_should_latch(&[0xA2, 0x5B], TriggerMode::Hold));
        assert!(!voice_should_latch(&[0x5B, 0xA4], TriggerMode::Hold));
        assert!(!voice_should_latch(&[0x5B], TriggerMode::Toggle));
    }

    #[test]
    fn lone_alt_always_taps() {
        // 无论 Toggle/Hold，单键 Alt 都是一次 down+up
        assert!(!voice_should_latch(&[0xA5], TriggerMode::Toggle));
        assert!(!voice_should_latch(&[0xA5], TriggerMode::Hold));
        assert!(!voice_should_latch(&[0xA4], TriggerMode::Hold));
        assert!(!voice_should_latch(&[0x12], TriggerMode::Hold));
    }

    #[test]
    fn ralt_space_always_taps() {
        // 右 Alt+Space 是用户语音映射：Hold 也不得闩住 Alt（否则系统菜单粘死）
        assert!(!voice_should_latch(&[0xA5, 0x20], TriggerMode::Toggle));
        assert!(!voice_should_latch(&[0xA5, 0x20], TriggerMode::Hold));
    }

    #[test]
    fn dup_window_only_covers_double_fire_not_rapid_clicks() {
        // 同步关搜索可达 ~1s；去重须盖住双路，但仍短于正常连点
        assert!(VOICE_DUP_EVENT_MS < 1200);
        assert!(VOICE_DUP_EVENT_MS > 400);
    }
}
