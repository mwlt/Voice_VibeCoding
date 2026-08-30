//! 语音 F5 简化模型契约（docs/VOICE_F5_SIMPLE_PLAN.md）。
//!
//! 路径：gadget 清 0x003E(F5) → 会话 LL 吞 → 注入前 bump → WinUHid 映射。
//! 禁止：neutralize / SendInput 清 F5 / LL 内 sleep。
//!
//! 运行: cargo test --test voice_f5_home_like -- --nocapture

use remote_bridge_hub_lib::bridges::xiaomi::key_mapping::{
    arm_voice_native_suppress, begin_voice_period, disarm_voice_native_suppress, end_voice_period,
    set_input_session_active, should_suppress_voice_f5, voice_f5_down_suppressed,
};

#[test]
fn session_alone_must_not_block_physical_keyboard_f5() {
    // 回归：会话在线时若吞全体 F5，真键盘 F5（记事本插日期等）会完全失效。
    end_voice_period("test");
    disarm_voice_native_suppress();
    set_input_session_active(true);
    assert!(
        !should_suppress_voice_f5(true, false, false),
        "session alone must not swallow physical keyboard F5"
    );
    assert!(
        !should_suppress_voice_f5(true, false, true),
        "session + tap_ready must not swallow physical keyboard F5"
    );
    assert!(!voice_f5_down_suppressed());
    set_input_session_active(false);
    disarm_voice_native_suppress();
}

#[test]
fn tap_ready_session_allows_physical_f5_outside_voice() {
    end_voice_period("test");
    disarm_voice_native_suppress();
    set_input_session_active(true);
    assert!(!should_suppress_voice_f5(true, false, true));
    assert!(!should_suppress_voice_f5(false, true, true));
    assert!(!voice_f5_down_suppressed());
    set_input_session_active(false);
    disarm_voice_native_suppress();
}

#[test]
fn without_session_physical_f5_passes() {
    end_voice_period("test");
    disarm_voice_native_suppress();
    set_input_session_active(false);
    assert!(!should_suppress_voice_f5(true, false, true));
    assert!(!should_suppress_voice_f5(true, false, false));
}

#[test]
fn armed_still_suppresses_without_session() {
    end_voice_period("test");
    disarm_voice_native_suppress();
    set_input_session_active(false);
    arm_voice_native_suppress();
    assert!(should_suppress_voice_f5(true, false, false));
    assert!(!should_suppress_voice_f5(false, true, false));
    disarm_voice_native_suppress();
}

#[test]
fn voice_period_swallows_without_session() {
    end_voice_period("test");
    disarm_voice_native_suppress();
    set_input_session_active(false);
    begin_voice_period();
    assert!(should_suppress_voice_f5(true, false, false));
    assert!(!should_suppress_voice_f5(false, true, false));
    end_voice_period("test");
    // period 结束保留 sticky；须 disarm 才彻底放开物理 F5 DOWN
    disarm_voice_native_suppress();
    assert!(!should_suppress_voice_f5(true, false, false));
}

#[test]
fn f5_keyup_passes_if_keydown_never_suppressed() {
    // bump 空窗漏过 KEYDOWN 时 sticky 仍为 false；必须放行 KEYUP 才能解开粘键
    end_voice_period("test");
    disarm_voice_native_suppress();
    set_input_session_active(true);
    assert!(
        !should_suppress_voice_f5(false, true, false),
        "F5 KEYUP must pass when no KEYDOWN was swallowed (unstick leaked F5)"
    );
    set_input_session_active(false);
}

#[test]
fn suppress_path_must_not_sleep_and_keys_off_session() {
    let src = include_str!("../src/bridges/xiaomi/key_mapping.rs");
    let body = src
        .split("pub fn should_suppress_voice_f5")
        .nth(1)
        .and_then(|s| s.split("pub fn ").next())
        .expect("fn body");
    assert!(!body.contains("wait_for_direct_signal"));
    assert!(!body.contains("thread::sleep"));
    assert!(
        !body.contains("input_session_active"),
        "must not key off whole input session (blocks physical keyboard F5)"
    );
    assert!(
        body.contains("return false"),
        "F5 KEYUP path must return false (always pass UP to unstick)"
    );
    assert!(
        body.contains("VOICE_F5_DOWN_SUPPRESSED"),
        "F5 KEYUP path must clear sticky"
    );
}

#[test]
fn f5_handled_before_injected_early_out_in_special_keys() {
    let src = include_str!("../src/bridges/xiaomi/special_keys.rs");
    let f5_pos = src
        .find("vk == 0x74 && !our_inject")
        .expect("F5 must use !our_inject before injected early-out");
    let injected_early = src
        .find("if injected {")
        .expect("injected early-out");
    assert!(
        f5_pos < injected_early,
        "F5 suppress must run before `if injected` CallNextHookEx early return"
    );
}

#[test]
fn voice_press_bumps_hook_like_pr8() {
    let src = include_str!("../src/bridges/xiaomi/key_mapping.rs");
    let handle = src
        .split("fn handle_voice")
        .nth(1)
        .and_then(|s| s.split("\nfn ").next())
        .expect("handle_voice");
    assert!(
        handle.contains("bump_hook_to_front"),
        "handle_voice must bump_hook before inject (PR #8: ahead of WeChat)"
    );
    let begin = handle
        .find("begin_voice_period")
        .expect("begin_voice_period");
    let bump = handle.find("bump_hook_to_front").expect("bump");
    assert!(
        begin < bump,
        "arm F5 suppress before bump so hook is armed when remounted"
    );
}

#[test]
fn bump_installs_new_hook_before_unhooking_old() {
    // 先 Unhook 再挂 = F5 空窗。必须先 SetWindowsHookEx 再 UnhookWindowsHookEx。
    let src = include_str!("../src/bridges/xiaomi/special_keys.rs");
    let body = src
        .split("msg.message == WM_BUMP_HOOK_FRONT")
        .nth(1)
        .and_then(|s| s.split("TranslateMessage").next())
        .expect("bump handler body");
    assert!(
        body.contains("overlap") || body.contains("先挂新钩"),
        "bump handler must document overlap install"
    );
    let set_pos = body
        .find("SetWindowsHookExW")
        .expect("bump must SetWindowsHookExW");
    let unhook_pos = body
        .find("UnhookWindowsHookEx")
        .expect("bump must eventually Unhook old hook");
    assert!(
        set_pos < unhook_pos,
        "SetWindowsHookEx must come before UnhookWindowsHookEx (no F5 leak window)"
    );
}

#[test]
fn ll_proc_has_nesting_guard_for_overlap_bump() {
    let src = include_str!("../src/bridges/xiaomi/special_keys.rs");
    assert!(
        src.contains("HOOK_PROC_DEPTH") || src.contains("proc_depth"),
        "overlap bump needs nesting depth guard so old+new hook don't double-handle"
    );
}

#[test]
fn mic_open_must_not_bump_hook() {
    let src = include_str!("../src/bridges/xiaomi/input_session.rs");
    let mic = src
        .split("0x08 =>")
        .nth(1)
        .and_then(|s| s.split("0x04 =>").next())
        .expect("0x08 MIC_OPEN arm");
    assert!(!mic.contains("bump_hook"), "MIC_OPEN must NOT bump_hook");
    assert!(
        mic.contains("begin_voice_period"),
        "MIC_OPEN must begin_voice_period"
    );
}

/// Step4 契约：简化模型禁止 neutralize（会把 F5 再喂给链头微信）。
#[test]
fn voice_path_must_not_neutralize_f5() {
    let mapping = include_str!("../src/bridges/xiaomi/key_mapping.rs");
    let inject = mapping
        .split("fn inject_voice_chord")
        .nth(1)
        .and_then(|s| s.split("\nfn ").next())
        .expect("inject_voice_chord");
    assert!(
        !inject.contains("neutralize"),
        "inject_voice_chord must not neutralize F5"
    );
    assert!(
        !inject.contains("clear_stuck_firmware_f5"),
        "SendInput F5 KEYUP is ignored by WeChat"
    );
    let special = include_str!("../src/bridges/xiaomi/special_keys.rs");
    let f5_block = special
        .split("vk == 0x74 && !our_inject")
        .nth(1)
        .and_then(|s| s.split("if injected {").next())
        .expect("F5 block");
    assert!(
        !f5_block.contains("neutralize"),
        "special_keys F5 swallow must not call neutralize"
    );
}

#[test]
fn voice_handle_refuses_f5_as_mapped_hotkey() {
    let src = include_str!("../src/bridges/xiaomi/key_mapping.rs");
    let handle = src
        .split("fn handle_voice")
        .nth(1)
        .and_then(|s| s.split("\nfn ").next())
        .expect("handle_voice");
    assert!(
        handle.contains("voice_hotkey_is_firmware_f5"),
        "must refuse injecting firmware F5 as the voice shortcut"
    );
    assert!(
        handle.contains("begin_voice_period"),
        "must arm F5 suppress even when shortcut disabled / mapping refused"
    );
}

#[test]
fn gadget_clears_keyboard_f5_usage() {
    let src = include_str!("../src/bridges/xiaomi/xiaomi_hid_gadget.js");
    // USB HID Keyboard: F1=0x3A, F5=0x3E. Clearing 0x3A was a long-standing bug.
    assert!(
        src.contains("usage === 0x003e") || src.contains("usage === 0x003E"),
        "gadget must clear HID usage 0x003E (F5), not 0x003A (F1)"
    );
    assert!(
        !src.contains("usage === 0x003a") && !src.contains("usage === 0x003A"),
        "must not clear usage === 0x003A (F1) as if it were F5"
    );
    assert!(
        src.contains("writeU16(0)"),
        "gadget must zero cleared usages"
    );
    assert!(
        src.contains("v1.5.9-f5-zero"),
        "gadget version stamp must bump so prepare_secure_runtime restarts WUDFHost"
    );
}

#[test]
fn vk_f5_maps_to_hid_usage_0x3e() {
    // Same formula as hid_injector::vk_to_usage: F-keys 0x70..=0x7B → usage = vk-0x70+0x3A
    assert_eq!(0x74u8 - 0x70 + 0x3A, 0x3E, "VK_F5 → HID usage F5");
    assert_eq!(0x70u8 - 0x70 + 0x3A, 0x3A, "VK_F1 → HID usage F1");
}

#[test]
fn inject_voice_prefers_winuhid_with_sendinput_fallback() {
    let src = include_str!("../src/bridges/xiaomi/key_mapping.rs");
    let inject = src
        .split("fn inject_voice_chord")
        .nth(1)
        .and_then(|s| s.split("\nfn ").next())
        .expect("inject_voice_chord");
    assert!(
        inject.contains("press_single") && inject.contains("is_available"),
        "voice inject must prefer WinUHid press_single when available"
    );
    assert!(
        inject.contains("SendInputFallback") || inject.contains("DEGRADED SendInput"),
        "when WinUHid unavailable must degrade to SendInput (not silent block)"
    );
    assert!(
        !inject.contains("inject BLOCKED"),
        "must not hard-block voice when WinUHid missing"
    );
    // Mutual exclusion: SendInput path must not call press_single in same arm —
    // press_single only under VirtualHid match arm (count: press_single appears, SendInputFallback arm uses key_chord_send_input)
    assert!(
        inject.contains("VOICE_INJECT_BACKEND_HELD") || inject.contains("VOICE_BACKEND_"),
        "DOWN must lock backend so UP does not switch mid-hold"
    );
}

#[test]
fn inject_voice_sendinput_arm_excludes_press_single() {
    let src = include_str!("../src/bridges/xiaomi/key_mapping.rs");
    let inject = src
        .split("fn inject_voice_chord")
        .nth(1)
        .and_then(|s| s.split("\nfn ").next())
        .expect("inject_voice_chord");
    let fallback_arm = inject
        .split("VoiceInjectBackend::SendInputFallback => {")
        .nth(1)
        .expect("SendInputFallback match arm with body");
    // Only the arm body until the next top-level match closing — cut at following fn if any
    let arm_body = fallback_arm.split("\nfn ").next().unwrap_or(fallback_arm);
    assert!(
        !arm_body.contains("press_single") && !arm_body.contains("release_single"),
        "SendInput fallback arm must not also call WinUHid press/release"
    );
    assert!(
        arm_body.contains("key_chord_send_input_with_extra"),
        "fallback arm must SendInput"
    );
}

#[test]
fn prepare_secure_runtime_restarts_host_on_script_change() {
    let inj = include_str!("../src/bridges/xiaomi/hid_tap_injector.rs");
    assert!(
        inj.contains("script_changed"),
        "injector must react to script_changed from prepare_secure_runtime"
    );
    let rt = include_str!("../src/bridges/xiaomi/hid_tap_runtime.rs");
    assert!(
        rt.contains("script_changed"),
        "prepare_secure_runtime must detect gadget script changes"
    );
}
