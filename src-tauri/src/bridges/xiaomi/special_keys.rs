//! 对齐 Python `XiaomiSpecialKeyHook`：抑制遥控器原生气
//!
//! 仅在「刚收到同键 HID direct / ATVV 信号」时吞掉 Windows 翻译的原 VK。

use crate::bridges::xiaomi::key_mapping::{
    direct_signal_recent, on_uncorrelated_f5_down, should_suppress_voice_f5, EXTRA_INFO,
};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};
use std::time::Duration;

/// 钩子线程：重新 SetWindowsHookEx，把自己挂到链头（最后安装 = 最先调用）。
#[cfg(target_os = "windows")]
const WM_BUMP_HOOK_FRONT: u32 = 0x8000 + 71; // WM_APP + 71

static RUNNING: AtomicBool = AtomicBool::new(false);
static HOOK_THREAD_ID: AtomicU32 = AtomicU32::new(0);
static HID_TAP_READY: AtomicBool = AtomicBool::new(false);
static HOOK_ENABLED: AtomicBool = AtomicBool::new(true);

/// 遥控器正在注入 Alt 开头的组合键（如 Alt+Space, Alt+S），
/// 由 key_mapping 在 key_chord 注入前设置、注入后清除。
/// 钩子检测到此标志时，对带 EXTRA_INFO 的 Alt/Space 等系统键也进行特殊处理：
/// 吞掉原始的 WM_SYSKEYDOWN（防止系统菜单），改用 WM_KEYDOWN 放行。
static ALT_CHORD_ACTIVE: AtomicBool = AtomicBool::new(false);

/// 通知钩子：即将注入 Alt 组合键（key_mapping 在 SendInput 之前调用）
pub fn arm_alt_chord() {
    ALT_CHORD_ACTIVE.store(true, Ordering::Release);
}

/// 通知钩子：Alt 组合键注入完毕（key_mapping 在 SendInput 之后调用）
pub fn disarm_alt_chord() {
    ALT_CHORD_ACTIVE.store(false, Ordering::Release);
}

fn alt_chord_active() -> bool {
    ALT_CHORD_ACTIVE.load(Ordering::Acquire)
}

#[cfg(target_os = "windows")]
static HOOK_PTR: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

/// HID Tap 已验证 IO（可捕获返回/音量）后置 true
pub fn set_hid_tap_ready(ready: bool) {
    HID_TAP_READY.store(ready, Ordering::Release);
    log::info!("XIAOMI SPECIAL KEY hid_tap_ready={ready}");
}

pub fn hid_tap_ready() -> bool {
    HID_TAP_READY.load(Ordering::Acquire)
}

pub fn set_hook_enabled(enabled: bool) {
    HOOK_ENABLED.store(enabled, Ordering::Release);
}

/// 语音注入前调用：把本进程 LL 钩子顶到链头，便于清 INJECTED 后输入法仍能看到事件。
pub fn bump_hook_to_front() {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::PostThreadMessageW;
        // 语音唤起依赖钩子清 INJECTED；即使配置关掉抑制钩子也临时拉起
        HOOK_ENABLED.store(true, Ordering::Release);
        let tid = HOOK_THREAD_ID.load(Ordering::Acquire);
        if tid == 0 {
            start_special_key_hook();
            return;
        }
        unsafe {
            let _ = PostThreadMessageW(tid, WM_BUMP_HOOK_FRONT, WPARAM(0), LPARAM(0));
        }
    }
}

/// 诊断/录入：钩子线程是否在跑
pub fn is_hook_running() -> bool {
    RUNNING.load(Ordering::Acquire)
}

/// LL 钩子是否已 SetWindowsHookEx 成功（比 RUNNING 更准）
pub fn is_hook_armed() -> bool {
    #[cfg(target_os = "windows")]
    {
        !HOOK_PTR.load(Ordering::Acquire).is_null()
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

/// 录入开始时确保常驻 LL 钩子在跑（即使配置曾关掉抑制钩子）
pub fn ensure_hook_for_capture() {
    HOOK_ENABLED.store(true, Ordering::Release);
    start_special_key_hook();
}

pub fn start_special_key_hook() {
    if !HOOK_ENABLED.load(Ordering::Acquire) {
        log::info!("XIAOMI SPECIAL KEY hook disabled by config");
        return;
    }
    if RUNNING.swap(true, Ordering::AcqRel) {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("xiaomi-special-keys".into())
        .spawn(|| {
            #[cfg(target_os = "windows")]
            hook_loop();
            RUNNING.store(false, Ordering::Release);
            HOOK_THREAD_ID.store(0, Ordering::Release);
        });
    // ponytail: 线程启动失败要复位 RUNNING，否则钩子永久卡死且静默不重试
    if spawned.is_err() {
        RUNNING.store(false, Ordering::Release);
        log::error!("XIAOMI SPECIAL KEY hook thread spawn failed");
        return;
    }
    log::info!("XIAOMI SPECIAL KEY hook starting");
}

pub fn stop_special_key_hook() {
    HID_TAP_READY.store(false, Ordering::Release);
    if !RUNNING.swap(false, Ordering::AcqRel) {
        return;
    }
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};
        let tid = HOOK_THREAD_ID.load(Ordering::Acquire);
        if tid != 0 {
            unsafe {
                let _ = PostThreadMessageW(tid, WM_QUIT, None, None);
            }
        }
    }
    log::info!("XIAOMI SPECIAL KEY hook stop requested");
}

#[cfg(target_os = "windows")]
fn load_hook() -> windows::Win32::UI::WindowsAndMessaging::HHOOK {
    use windows::Win32::UI::WindowsAndMessaging::HHOOK;
    HHOOK(HOOK_PTR.load(Ordering::Acquire))
}

#[cfg(target_os = "windows")]
fn store_hook(h: windows::Win32::UI::WindowsAndMessaging::HHOOK) {
    HOOK_PTR.store(h.0, Ordering::Release);
}

/// 判断 vk 是否属于 Alt 按下时会被 Windows 系统拦截的组合键成员。
/// 包括 Alt 本身 + Space / F4 / Tab / Esc / 字母键等可能被全局热键占用的键。
#[cfg(target_os = "windows")]
fn is_alt_system_key(vk: u32) -> bool {
    // Alt 修饰键本身
    matches!(
        vk,
        0x12 | 0xA4 | 0xA5 | // VK_MENU, VK_LMENU, VK_RMENU
        0x20 | // VK_SPACE → Alt+Space 系统菜单
        0x73 | // VK_F4   → Alt+F4 关闭窗口
        0x09 | // VK_TAB  → Alt+Tab 任务切换
        0x1B    // VK_ESCAPE → Alt+Esc 切换窗口
    ) || (0x41u32..=0x5A).contains(&vk) // A-Z 可能被注册为全局热键
       || (0x30u32..=0x39).contains(&vk) // 0-9 可能被注册为全局热键
}

#[cfg(target_os = "windows")]
fn hook_loop() {
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage,
        UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL,
    };

    unsafe extern "system" fn proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        let hook = load_hook();
        if code >= 0 {
            let info = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            let flags = info.flags.0;
            let vk = info.vkCode;
            let msg = wparam.0 as u32;
            let injected = info.dwExtraInfo == EXTRA_INFO || (flags & 0x10) != 0;

            // 快捷键录入：最优先吞掉全部物理键（含 WM_SYSKEY* / Alt+Space / Win 热键）
            // 必须在 CallNextHookEx 之前；第二套短生命周期钩子不可靠（易被超时静默卸掉）
            if crate::bridges::shared::shortcut_capture::try_swallow_capture_key(vk, msg, injected)
            {
                return LRESULT(1);
            }

            // Alt 和弦注入中：即使是注入键（带 EXTRA_INFO），
            // 也不能直接放行 Alt/Space 等系统键 —— 否则会触发系统菜单。
            // 这里走抑制路径，让调用方（key_mapping）通过 WM_KEYDOWN 路径
            // 单独投递按键，避免 WM_SYSKEYDOWN。
            if alt_chord_active()
                && injected
                && is_alt_system_key(vk)
            {
                log::info!("XIAOMI SPECIAL KEY alt_chord suppressed vk=0x{vk:02X}");
                return LRESULT(1);
            }

            if injected {
                return CallNextHookEx(hook, code, wparam, lparam);
            }

            let scan = info.scanCode;
            let down = msg == 0x0100 || msg == 0x0104;
            let up = msg == 0x0101 || msg == 0x0105;
            let tap_ready = HID_TAP_READY.load(Ordering::Acquire);

            // 对齐 Python：音量仅在 Tap 就绪后抑制；其它键在 recent 信号时抑制
            // v1.5.x 修双发：Tap 接管时无条件吞原生音量（消除 LL 先于 BLE 信号的时序窗口）
            let suppress = match vk {
                0xAF if should_suppress_volume_native(
                    0xAF,
                    tap_ready,
                    direct_signal_recent("volume_up", Duration::from_millis(200)),
                ) =>
                {
                    Some("volume_up")
                }
                0xAE if should_suppress_volume_native(
                    0xAE,
                    tap_ready,
                    direct_signal_recent("volume_down", Duration::from_millis(200)),
                ) =>
                {
                    Some("volume_down")
                }
                0xAD if should_suppress_volume_native(
                    0xAD,
                    tap_ready,
                    direct_signal_recent("volume_mute", Duration::from_millis(200))
                        || direct_signal_recent("mute", Duration::from_millis(200)),
                ) =>
                {
                    Some("volume_mute")
                }
                0xA6 if direct_signal_recent("back", Duration::from_millis(250)) => Some("back"),
                0x24 | 0xAC
                    if should_suppress_native_menu_home(
                        vk as u16,
                        tap_ready,
                        direct_signal_recent("home", Duration::from_millis(250)),
                    ) =>
                {
                    Some("home")
                }
                0x5D if should_suppress_native_menu_home(
                    vk as u16,
                    tap_ready,
                    direct_signal_recent("menu", Duration::from_millis(250)),
                ) =>
                {
                    Some("menu")
                }
                0x0D if direct_signal_recent("ok", Duration::from_millis(200)) => Some("ok"),
                0x25 if direct_signal_recent("left", Duration::from_millis(200))
                    || direct_signal_recent("dpad_left", Duration::from_millis(200)) =>
                {
                    Some("left")
                }
                0x27 if direct_signal_recent("right", Duration::from_millis(200))
                    || direct_signal_recent("dpad_right", Duration::from_millis(200)) =>
                {
                    Some("right")
                }
                0x26 if direct_signal_recent("up", Duration::from_millis(200))
                    || direct_signal_recent("dpad_up", Duration::from_millis(200)) =>
                {
                    Some("up")
                }
                0x28 if direct_signal_recent("down", Duration::from_millis(200))
                    || direct_signal_recent("dpad_down", Duration::from_millis(200)) =>
                {
                    Some("down")
                }
                // TV: OEM_3 + scan 0x29
                0xC0 if scan == 0x29 && direct_signal_recent("tv", Duration::from_millis(250)) => {
                    Some("tv")
                }
                // Power: Sleep / 0xFF / scan 0x5E
                0x5F | 0xFF if direct_signal_recent("power", Duration::from_millis(250)) => {
                    Some("power")
                }
                _ if scan == 0x5E && direct_signal_recent("power", Duration::from_millis(250)) => {
                    Some("power")
                }
                0x74 if !injected && (down || up) => {
                    if should_suppress_voice_f5(down, up) {
                        crate::bridges::xiaomi::key_mapping::on_firmware_voice_key(down);
                        Some("voice_f5")
                    } else {
                        if down {
                            on_uncorrelated_f5_down();
                        }
                        None
                    }
                }
                _ => None,
            };

            if let Some(name) = suppress {
                if down || up {
                    log::info!("XIAOMI SPECIAL KEY {name} original_suppressed vk=0x{vk:02X}");
                    return LRESULT(1);
                }
            }
        }
        CallNextHookEx(hook, code, wparam, lparam)
    }

    unsafe {
        HOOK_THREAD_ID.store(GetCurrentThreadId(), Ordering::Release);
        let hook = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(proc), None, 0) {
            Ok(h) => h,
            Err(e) => {
                log::error!("SetWindowsHookExW failed: {e}");
                return;
            }
        };
        store_hook(hook);
        log::info!(
            "XIAOMI SPECIAL KEYS READY mapping=configurable \
             repeat=back,volume,direction suppress_original=device-correlated"
        );
        let mut msg = MSG::default();
        while RUNNING.load(Ordering::Acquire) {
            let ret = GetMessageW(&mut msg, None, 0, 0);
            if ret.0 == -1 || ret.0 == 0 {
                break;
            }
            if msg.message == WM_BUMP_HOOK_FRONT {
                let old = load_hook();
                if !old.is_invalid() {
                    let _ = UnhookWindowsHookEx(old);
                }
                match SetWindowsHookExW(WH_KEYBOARD_LL, Some(proc), None, 0) {
                    Ok(h) => {
                        store_hook(h);
                        log::debug!("XIAOMI SPECIAL KEY hook bumped to chain head");
                    }
                    Err(e) => {
                        store_hook(HHOOK(std::ptr::null_mut()));
                        log::error!("XIAOMI SPECIAL KEY bump SetWindowsHookExW failed: {e}");
                    }
                }
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        // 先清空再 Unhook，避免卸载窗口期回调读到悬空句柄语义
        let hook = load_hook();
        store_hook(HHOOK(std::ptr::null_mut()));
        if !hook.is_invalid() {
            let _ = UnhookWindowsHookEx(hook);
        }
    }
}

/// 音量键原生事件是否应被吞掉（避免与 SendInput 注入叠成系统双格）。
///
/// - `tap_ready`：HID Tap 已接管（应用负责注入一次）→ **无条件吞**原生音量事件，
///   消除 LL 钩子先于 BLE/HID-Tap 信号到达时 `direct_signal_recent` 尚未标记的时序窗口；
/// - `recent_signal`：200ms 窗口内遥控器刚按下过该音量键 → 兜底吞（Tap 未就绪时）。
///
/// 注意：非音量键（方向/OK/返回等）不受此判定影响。
///
/// ```
/// use remote_bridge_hub_lib::bridges::xiaomi::special_keys::should_suppress_volume_native;
///
/// // HID Tap 接管：无条件吞（消除 LL 先于 BLE 信号的时序窗口 → 防双格）
/// assert!(should_suppress_volume_native(0xAF, true, false));
/// assert!(should_suppress_volume_native(0xAE, true, false));
/// assert!(should_suppress_volume_native(0xAD, true, false));
/// // Tap 未就绪但有近期信号：兜底吞
/// assert!(should_suppress_volume_native(0xAF, false, true));
/// // 两者皆无：透传（物理键盘音量键必须可用）
/// assert!(!should_suppress_volume_native(0xAF, false, false));
/// // 非音量键不受影响
/// assert!(!should_suppress_volume_native(0x26, true, true));
/// assert!(!should_suppress_volume_native(0x0D, true, false));
/// ```
pub fn should_suppress_volume_native(vk: u16, tap_ready: bool, recent_signal: bool) -> bool {
    let is_volume = matches!(vk, 0xAF | 0xAE | 0xAD); // VK_VOLUME_UP / VK_VOLUME_DOWN / VK_VOLUME_MUTE
    is_volume && (tap_ready || recent_signal)
}

/// menu/home 原生事件是否应被吞掉（与音量键 `should_suppress_volume_native` 同策略）。
///
/// 背景：HID Tap 是旁路抄送，LL 钩子可能先于 hub 的 HID 报文到达 →
/// `direct_signal_recent("menu"/"home")` 尚未标记，原生气漏出：
/// - menu 固件 usage 0x65 翻译成 VK_APPS(0x5D)：慢速点击时先弹右键菜单，
///   再叠上注入的 Win(0x5B)，之后 Win 弹起被系统吃掉 → 菜单关不掉；
/// - home 映射为 Space(0x20) 时，固件原生 VK_HOME(0x24)/0xAC 漏出会先跳行首。
///
/// - `tap_ready`：Tap 已接管（应用负责注入）→ 无条件吞原生 menu/home；
/// - `recent_signal`：250ms 内遥控器刚按下过该键 → 兜底吞；
/// - 两者皆无：透传（物理键盘的 Home / Menu 键必须可用）。
///
/// ```
/// use remote_bridge_hub_lib::bridges::xiaomi::special_keys::should_suppress_native_menu_home;
///
/// // Tap 接管：无条件吞（消除 LL 先于 HID-Tap 信号的时序窗口）
/// assert!(should_suppress_native_menu_home(0x5D, true, false));
/// assert!(should_suppress_native_menu_home(0x24, true, false));
/// assert!(should_suppress_native_menu_home(0xAC, true, true));
/// // Tap 未就绪但有近期信号：兜底吞
/// assert!(should_suppress_native_menu_home(0x5D, false, true));
/// assert!(should_suppress_native_menu_home(0x24, false, true));
/// // 两者皆无：透传
/// assert!(!should_suppress_native_menu_home(0x5D, false, false));
/// assert!(!should_suppress_native_menu_home(0x24, false, false));
/// // 非 menu/home 键不受影响
/// assert!(!should_suppress_native_menu_home(0x20, true, true));
/// assert!(!should_suppress_native_menu_home(0xA6, true, true));
/// ```
pub fn should_suppress_native_menu_home(vk: u16, tap_ready: bool, recent_signal: bool) -> bool {
    let is_menu_or_home = matches!(vk, 0x5D | 0x24 | 0xAC); // VK_APPS / VK_HOME / 0xAC
    is_menu_or_home && (tap_ready || recent_signal)
}

#[cfg(test)]
mod tests {
    use super::should_suppress_volume_native;

    #[test]
    fn volume_up_suppressed_when_tap_ready() {
        // HID Tap 接管时：即使 recent 信号尚未标记（LL 钩子先到），也必须吞掉原生音量，
        // 否则固件原生 + SendInput 注入 = 两格。
        assert!(should_suppress_volume_native(0xAF, true, false));
        assert!(should_suppress_volume_native(0xAE, true, false));
        assert!(should_suppress_volume_native(0xAD, true, false));
    }

    #[test]
    fn volume_suppressed_on_recent_signal_without_tap() {
        // Tap 未就绪但 200ms 内有遥控器信号：兜底吞（对齐旧行为）
        assert!(should_suppress_volume_native(0xAF, false, true));
        assert!(should_suppress_volume_native(0xAE, false, true));
        assert!(should_suppress_volume_native(0xAD, false, true));
    }

    #[test]
    fn volume_passthrough_when_neither_ready() {
        // Tap 未接管且无近期信号：透传原生事件（物理键盘音量键必须可用）
        assert!(!should_suppress_volume_native(0xAF, false, false));
        assert!(!should_suppress_volume_native(0xAE, false, false));
        assert!(!should_suppress_volume_native(0xAD, false, false));
    }

    #[test]
    fn non_volume_keys_never_affected() {
        // 方向键 0x26/0x28、OK 0x0D、返回 0xA6 不受音量判定影响
        assert!(!should_suppress_volume_native(0x26, true, true));
        assert!(!should_suppress_volume_native(0x28, true, true));
        assert!(!should_suppress_volume_native(0x0D, true, false));
        assert!(!should_suppress_volume_native(0xA6, true, false));
    }

    #[test]
    fn tap_ready_beats_stale_recent() {
        // tap_ready=true 且 recent=false 时必须吞（时序窗口核心场景）
        assert!(should_suppress_volume_native(0xAF, true, false));
        // 与 recent=true 时结果一致
        assert_eq!(
            should_suppress_volume_native(0xAF, true, false),
            should_suppress_volume_native(0xAF, true, true)
        );
    }

    // ---- menu/home：与音量键同策略（v1.3.13 时序窗口修复）----

    use super::should_suppress_native_menu_home;

    #[test]
    fn menu_home_suppressed_when_tap_ready() {
        // Tap 接管时无条件吞原生 menu(VK_APPS)/home，即使 recent 信号尚未标记
        // （LL 钩子先于 hub HID 报文到达的时序窗口 → 慢速点击漏右键菜单/跳行首）
        assert!(should_suppress_native_menu_home(0x5D, true, false));
        assert!(should_suppress_native_menu_home(0x24, true, false));
        assert!(should_suppress_native_menu_home(0xAC, true, false));
    }

    #[test]
    fn menu_home_suppressed_on_recent_signal_without_tap() {
        // Tap 未就绪但 250ms 内有遥控器信号：兜底吞（对齐旧行为）
        assert!(should_suppress_native_menu_home(0x5D, false, true));
        assert!(should_suppress_native_menu_home(0x24, false, true));
        assert!(should_suppress_native_menu_home(0xAC, false, true));
    }

    #[test]
    fn menu_home_passthrough_when_neither_ready() {
        // 两者皆无：透传原生事件（物理键盘 Home / Menu 键必须可用）
        assert!(!should_suppress_native_menu_home(0x5D, false, false));
        assert!(!should_suppress_native_menu_home(0x24, false, false));
        assert!(!should_suppress_native_menu_home(0xAC, false, false));
    }

    #[test]
    fn non_menu_home_keys_never_affected() {
        // Space 0x20（home 的注入目标）、Back 0xA6、OK 0x0D 不受此判定影响
        assert!(!should_suppress_native_menu_home(0x20, true, true));
        assert!(!should_suppress_native_menu_home(0xA6, true, true));
        assert!(!should_suppress_native_menu_home(0x0D, true, false));
    }
}
