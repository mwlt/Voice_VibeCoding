//! T1 原生键吞掉 / 映射放行 — 公共缝（不启 RegisterHotKey）。
//!
//! 缝：
//! - `native_suppress::{set_gate, should_suppress_native, allow_pass_vks}`
//! - `mapping` 事件 → 必须吞的原生侧效应 VK
//! - 映射目标（右 Alt / Ctrl / Win / Space）在闸门开启时仍可放行

use std::sync::{Mutex, MutexGuard, OnceLock};

use remote_bridge_hub_lib::bridges::t1::ble_runtime::should_autostart_t1_ble;
use remote_bridge_hub_lib::bridges::t1::inject::{
    mapped_tap_needs_shell_dummy, mapped_voice_is_tap, names_to_vks, SHELL_MENU_DUMMY_VK,
};
use remote_bridge_hub_lib::bridges::t1::mapping::native_side_effect_vk_for_event;
use remote_bridge_hub_lib::bridges::t1::ble_adpcm::encode_16k_to_cable_le;
use remote_bridge_hub_lib::bridges::t1::native_suppress::{
    allow_pass_vks, apply_native_press_policy, apply_native_release_policy,
    foreground_hint_is_search_or_start, hold_suppress_native_vk, is_t1_native_side_effect_vk,
    is_windows_search_or_start_class, is_windows_search_or_start_process,
    release_suppress_native_vk, set_gate, shell_should_swallow_appcommand,
    should_keep_swallow_gate, should_suppress_native, should_tap_escape_for_shell,
    should_watchdog_escape_now, APPCOMMAND_BROWSER_SEARCH, FG_WATCHDOG_ESC_GAP_MS,
};

fn gate_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

#[test]
fn t1_gate_on_swallows_browser_search() {
    let _g = gate_lock();
    set_gate(true);
    assert!(
        should_suppress_native(0xAA, false, true),
        "T1 开闸后 Browser Search 必须吞掉"
    );
    assert!(
        !should_suppress_native(0xAA, true, true),
        "本进程注入仍可放行"
    );
    set_gate(false);
    assert!(!should_suppress_native(0xAA, false, true));
}

/// T1 原生侧效应（搜索/主页/菜单/音量/浏览器后退）开闸后一律吞，不能等 120ms arm。
#[test]
fn t1_gate_swallows_all_native_side_effect_vks() {
    let _g = gate_lock();
    set_gate(true);
    for vk in [0xAA, 0xAC, 0x5D, 0xAD, 0xAE, 0xAF, 0xA6] {
        assert!(
            should_suppress_native(vk, false, true),
            "native vk 0x{vk:02X} must be swallowed while T1 gate is on"
        );
    }
    set_gate(false);
}

#[test]
fn t1_native_events_have_swallow_vk() {
    assert_eq!(native_side_effect_vk_for_event("hid:02-21-02"), Some(0xAA));
    assert_eq!(native_side_effect_vk_for_event("kbd:VK_AA"), Some(0xAA));
    assert_eq!(native_side_effect_vk_for_event("hid:02-23-02"), Some(0xAC));
    assert_eq!(native_side_effect_vk_for_event("kbd:VK_5D"), Some(0x5D));
    assert_eq!(native_side_effect_vk_for_event("hid:02-E9-00"), Some(0xAF));
    assert_eq!(native_side_effect_vk_for_event("hid:02-EA-00"), Some(0xAE));
    assert_eq!(native_side_effect_vk_for_event("hid:02-E2-00"), Some(0xAD));
    assert_eq!(native_side_effect_vk_for_event("hid:02-24-02"), Some(0xA6));
}

#[test]
fn mapped_inject_vks_are_not_swallowed() {
    let _g = gate_lock();
    set_gate(true);
    let mapped = names_to_vks(&["rightalt".into(), "leftctrl".into(), "leftwin".into(), "space".into()]);
    assert_eq!(mapped, vec![0xA5, 0xA2, 0x5B, 0x20]);
    for &vk in &mapped {
        assert!(
            !is_t1_native_side_effect_vk(vk),
            "mapped vk 0x{vk:02X} must not be a T1 native side-effect"
        );
        assert!(
            !should_suppress_native(vk, false, true),
            "mapped vk 0x{vk:02X} must pass the gate"
        );
    }
    allow_pass_vks(&mapped);
    assert!(should_suppress_native(0xAA, false, true));
    set_gate(false);
}

#[test]
fn remapped_dpad_swallowed_only_while_held() {
    let _g = gate_lock();
    set_gate(true);
    assert!(!should_suppress_native(0x27, false, true));
    hold_suppress_native_vk(0x27);
    assert!(should_suppress_native(0x27, false, true));
    assert!(!should_suppress_native(0x20, false, true));
    release_suppress_native_vk(0x27);
    assert!(!should_suppress_native(0x27, false, true));
    set_gate(false);
}

/// 映射注入的 allow_pass 不得把正在 hold-suppress 的原生方向键放进系统。
#[test]
fn hold_suppress_beats_allow_pass() {
    let _g = gate_lock();
    set_gate(true);
    hold_suppress_native_vk(0x27);
    allow_pass_vks(&[0x27]);
    assert!(
        should_suppress_native(0x27, false, true),
        "remapped native Right must stay swallowed during inject allow_pass"
    );
    release_suppress_native_vk(0x27);
    set_gate(false);
}

#[test]
fn press_policy_holds_remapped_dpad_until_release() {
    let _g = gate_lock();
    set_gate(true);
    apply_native_press_policy("right", Some(0x27), &[0xA5, 0x20]);
    assert!(should_suppress_native(0x27, false, true));
    apply_native_release_policy(Some(0x27));
    assert!(!should_suppress_native(0x27, false, true));
    set_gate(false);
}

#[test]
fn voice_pcm_16k_encodes_to_48k_le_for_cable() {
    let (bytes, last) = encode_16k_to_cable_le(&[1000, -1000], None);
    assert_eq!(bytes.len(), 2 * 3 * 2);
    assert_eq!(last, Some(-1000));
    let s0 = i16::from_le_bytes([bytes[2], bytes[3]]);
    assert_eq!(s0, 1000);
}

/// 映射含 Win/Alt 的点按会让系统当成「点了 Win/Alt」→ 开始菜单/窗口菜单。
/// 公共缝：抬键后必须补无键帽 dummy；dummy 本身不得被闸门吞掉。
#[test]
fn mapped_win_alt_tap_needs_shell_dummy_not_swallowed() {
    let _g = gate_lock();
    assert!(mapped_tap_needs_shell_dummy(&[0xA2, 0x5B]));
    assert!(mapped_tap_needs_shell_dummy(&[0xA5]));
    assert!(mapped_tap_needs_shell_dummy(&[0xA5, 0x20]));
    assert!(mapped_tap_needs_shell_dummy(&[0x5B]));
    assert!(!mapped_tap_needs_shell_dummy(&[0xA2]));
    assert!(!mapped_tap_needs_shell_dummy(&[0x20]));
    assert_eq!(SHELL_MENU_DUMMY_VK, 0xE8);
    set_gate(true);
    assert!(!is_t1_native_side_effect_vk(SHELL_MENU_DUMMY_VK));
    assert!(
        !should_suppress_native(SHELL_MENU_DUMMY_VK, false, true),
        "shell dummy must reach the system after mapped tap"
    );
    set_gate(false);
}

/// APPCOMMAND 打开的是 SearchHost/Start 飞出层，不是 0xAA 键盘消息。
/// 只认这些进程/粗类名；explorer 整进程不可关。Esc 只在真看到壳层时点。
#[test]
fn search_start_shell_is_identified_without_killing_explorer() {
    assert!(is_windows_search_or_start_process(
        r"C:\Windows\SystemApps\MicrosoftWindows.Client.CBS_cw5n1h2txyewy\SearchHost.exe"
    ));
    assert!(is_windows_search_or_start_process("StartMenuExperienceHost.exe"));
    assert!(is_windows_search_or_start_process("ShellExperienceHost.exe"));
    assert!(!is_windows_search_or_start_process(r"C:\Windows\explorer.exe"));
    assert!(!is_windows_search_or_start_process("chrome.exe"));
    assert!(is_windows_search_or_start_class("ImmersiveLauncher"));
    assert!(is_windows_search_or_start_class("SearchPane"));
    assert!(!is_windows_search_or_start_class("Chrome_WidgetWin_1"));
    assert!(!is_windows_search_or_start_class("Windows.UI.Core.CoreWindow"));
    assert!(should_tap_escape_for_shell(true, false));
    assert!(should_tap_escape_for_shell(false, true));
    assert!(!should_tap_escape_for_shell(false, false));
    assert!(foreground_hint_is_search_or_start("SearchHost.exe pid=4242"));
    assert!(foreground_hint_is_search_or_start("StartMenuExperienceHost.exe pid=1"));
    assert!(!foreground_hint_is_search_or_start("WeChat.exe pid=99"));
    assert!(!foreground_hint_is_search_or_start("remote-bridge-hub.exe pid=1"));
}

/// APPCOMMAND 晚于 LL 才抢前台：看门狗只 Esc Search/Start，且两次 Esc 隔 80ms。
#[test]
fn fg_watchdog_escapes_search_not_wechat_and_debounces() {
    use std::time::Duration;
    assert!(should_watchdog_escape_now(true, None));
    assert!(!should_watchdog_escape_now(false, None));
    assert!(!should_watchdog_escape_now(
        false,
        Some(Duration::from_millis(200))
    ));
    assert!(!should_watchdog_escape_now(
        true,
        Some(Duration::from_millis(FG_WATCHDOG_ESC_GAP_MS - 1))
    ));
    assert!(should_watchdog_escape_now(
        true,
        Some(Duration::from_millis(FG_WATCHDOG_ESC_GAP_MS))
    ));
}

/// GATT 闪断会停按键监听，但不能卸闸门；用户点断开才卸。
#[test]
fn swallow_gate_stays_up_while_ble_reconnects() {
    assert!(should_keep_swallow_gate(false, true, false));
    assert!(!should_keep_swallow_gate(false, true, true));
    assert!(should_keep_swallow_gate(true, false, true));
    assert!(!should_keep_swallow_gate(false, false, false));
}

/// 用户映射 Ctrl+Win / 右 Alt(+Space)：每次按必须是完整点按，不能闩锁。
#[test]
fn mapped_ctrl_win_and_alt_are_taps_not_latch() {
    assert!(mapped_voice_is_tap(&[0xA2, 0x5B]));
    assert!(mapped_voice_is_tap(&[0xA5]));
    assert!(mapped_voice_is_tap(&[0xA5, 0x20]));
    assert!(!mapped_voice_is_tap(&[0xA2]));
    assert!(mapped_tap_needs_shell_dummy(&[0xA2, 0x5B]));
    assert!(mapped_tap_needs_shell_dummy(&[0xA5, 0x20]));
}

/// 回归：麦会话开着不得永久禁注入；只靠同一次物理按下的去重窗。
#[test]
fn voice_reinject_allowed_after_dup_window_even_if_session_open() {
    use std::time::Duration;
    use remote_bridge_hub_lib::bridges::t1::ble_voice::is_same_voice_press;
    use remote_bridge_hub_lib::bridges::t1::ble_session::audio_start_should_clear;
    // 推流中禁止 CLEAR，但不得据此跳过映射点按（历史 sticky skip reinject）。
    assert!(!audio_start_should_clear(true));
    assert!(is_same_voice_press(Duration::from_millis(100)));
    assert!(!is_same_voice_press(Duration::from_millis(1000)));
}

/// HID AC Search 与 ATVV START_SEARCH 会前后脚到达；关搜索/点按可达数百 ms，
/// 去重窗必须盖住「第二路进锁前被同步 dismiss 拖慢」的双发。
#[test]
fn voice_hid_and_start_search_are_one_press() {
    use std::time::Duration;
    use remote_bridge_hub_lib::bridges::t1::ble_voice::is_same_voice_press;
    assert!(is_same_voice_press(Duration::from_millis(0)));
    assert!(is_same_voice_press(Duration::from_millis(120)));
    assert!(is_same_voice_press(Duration::from_millis(800)));
    assert!(!is_same_voice_press(Duration::from_millis(1000)));
}

/// 闸门只在 T1 桥接启动后挂上。已保存蓝牙地址必须自动连，否则重建后原生键全漏。
#[test]
fn saved_t1_ble_address_autostarts() {
    assert!(should_autostart_t1_ble(Some("12:AC:2C:46:C4:AB")));
    assert!(!should_autostart_t1_ble(None));
    assert!(!should_autostart_t1_ble(Some("")));
    assert!(!should_autostart_t1_ble(Some("   ")));
}

/// 麦会话已开时再点语音不得 CLEAR，否则 router 清空缓冲、流一直播 0，输入法听不到。
#[test]
fn voice_session_must_not_clear_pcm_midstream() {
    use remote_bridge_hub_lib::bridges::t1::ble_voice_meter::should_clear_pcm_on_voice_edge;
    assert!(should_clear_pcm_on_voice_edge(false));
    assert!(!should_clear_pcm_on_voice_edge(true));
}

/// AUDIO_START：仅新会话 CLEAR；映射点按由 450ms 去重，不绑会话生命周期。
#[test]
fn audio_start_should_clear_only_when_fresh() {
    use remote_bridge_hub_lib::bridges::t1::ble_session::audio_start_should_clear;
    assert!(audio_start_should_clear(false));
    assert!(!audio_start_should_clear(true));
}

/// L1 缝：闸门开时吞 APPCOMMAND_BROWSER_SEARCH(=5)；其它 cmd / 闸门关则放行。
#[test]
fn shell_appcommand_browser_search_swallowed_only_when_gate_on() {
    assert_eq!(APPCOMMAND_BROWSER_SEARCH, 5);
    assert!(shell_should_swallow_appcommand(APPCOMMAND_BROWSER_SEARCH, true));
    assert!(!shell_should_swallow_appcommand(APPCOMMAND_BROWSER_SEARCH, false));
    // 6 = APPCOMMAND_BROWSER_FAVORITES，不得误吞
    assert!(!shell_should_swallow_appcommand(6, true));
    assert!(!shell_should_swallow_appcommand(1, true));
}

/// 与 t1_shell_hook.dll 契约：共享映射名必须一致。
#[cfg(windows)]
#[test]
fn shell_gate_map_name_matches_dll_contract() {
    assert_eq!(
        remote_bridge_hub_lib::bridges::t1::native_suppress::SHELL_GATE_MAP_NAME,
        "Local\\T1BrSearchShellGate"
    );
}

/// 产物存在时：LoadLibrary + GetProcAddress(T1ShellProc) + SetWindowsHookEx(WH_SHELL) 必须成功。
#[cfg(windows)]
#[test]
fn wh_shell_dll_can_arm_hook() {
    use std::path::PathBuf;

    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut candidates = vec![
        manifest.join("target").join("debug").join("t1_shell_hook.dll"),
        manifest.join("target").join("release").join("t1_shell_hook.dll"),
    ];
    if let Ok(td) = std::env::var("CARGO_TARGET_DIR") {
        let td = PathBuf::from(td);
        candidates.push(td.join("debug").join("t1_shell_hook.dll"));
        candidates.push(td.join("release").join("t1_shell_hook.dll"));
    }
    let Some(dll_path) = candidates.into_iter().find(|p| p.is_file()) else {
        panic!(
            "t1_shell_hook.dll missing — build with: cargo build -p t1_shell_hook --manifest-path src-tauri/Cargo.toml"
        );
    };

    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryW(lp: *const u16) -> *mut std::ffi::c_void;
        fn GetProcAddress(m: *mut std::ffi::c_void, n: *const i8) -> *mut std::ffi::c_void;
        fn FreeLibrary(m: *mut std::ffi::c_void) -> i32;
        fn GetLastError() -> u32;
    }
    #[link(name = "user32")]
    extern "system" {
        fn SetWindowsHookExW(
            id: i32,
            f: Option<unsafe extern "system" fn(i32, usize, isize) -> isize>,
            m: *mut std::ffi::c_void,
            tid: u32,
        ) -> *mut std::ffi::c_void;
        fn UnhookWindowsHookEx(h: *mut std::ffi::c_void) -> i32;
    }

    let wide: Vec<u16> = dll_path
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let dll = unsafe { LoadLibraryW(wide.as_ptr()) };
    assert!(
        !dll.is_null(),
        "LoadLibraryW({}) failed err={}",
        dll_path.display(),
        unsafe { GetLastError() }
    );
    let sym = unsafe { GetProcAddress(dll, b"T1ShellProc\0".as_ptr() as *const i8) };
    assert!(!sym.is_null(), "GetProcAddress(T1ShellProc) failed");
    type HookFn = unsafe extern "system" fn(i32, usize, isize) -> isize;
    let hook = unsafe {
        SetWindowsHookExW(
            10, // WH_SHELL
            Some(std::mem::transmute::< *mut std::ffi::c_void, HookFn>(sym)),
            dll,
            0,
        )
    };
    assert!(
        !hook.is_null(),
        "SetWindowsHookEx(WH_SHELL) via DLL failed err={}",
        unsafe { GetLastError() }
    );
    unsafe {
        UnhookWindowsHookEx(hook);
        FreeLibrary(dll);
    }
}
