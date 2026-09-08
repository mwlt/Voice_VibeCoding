//! T1 原生键闸门
//!
//! **硬约束：不得导致实体键盘任何常用键失效。**
//!
//! LL 钩子无法区分「遥控固件」与「实体键盘」。因此：
//! - 空格 / 退格 / 方向 / Enter / 字母等 **永不** TTL-arm；
//! - **同键映射**（如 OK→Enter、左→左）：不 hold、不注入，原生透传一次（避免 LL 先到 + 注入双发）；
//! - 仅对遥控侧效应冷门键（Apps / Browser_*）常驻闸门；
//! - 音量±：仅当映射**不是**同键/未绑定 时才进闸门（未绑定须让系统音量生效）；
//! - 静音：HOGP 原生静音不可靠 → 桥接开启时**始终**闸门 + SendInput 注入（同键也注入 0xAD）；
//! - 方向/OK：仅当映射到**其它**键时 LL 常驻闸门（否则 LL 先于 Raw 会漏出原生方向）。
//!
//! **0xAA / barsearch 完整吞掉：**
//! - 闸门优先于 `allow_pass`（映射注入放行窗口不得开洞）；
//! - `RegisterHotKey(VK_BROWSER_SEARCH)` 吃 APPCOMMAND；
//! - HID `02-21-02` / LL swallow / hotkey / 映射前后都 `dismiss SearchHost`；
//! - WinEvent 关 Search **仅在** 遥控触发的短 arm 窗口内生效（勿常驻杀开始菜单）；
//! - 绝不 `on_foreign_keyboard` 回放 0xAA。
//! - `on_foreign_keyboard` 只回放 Apps/Browser Home/Back/(可选)VK_HOME；
//!   **禁止**回放方向/OK/音量/静音（防 foreign↔LL 无限连击）。

use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::LazyLock;
use std::thread;
use std::time::{Duration, Instant};

static ENABLED: AtomicBool = AtomicBool::new(false);

/// 音量±闸门：仅「重映射走」时开启（见 [`refresh_media_gates_from_bindings`]）。
static GATE_VOL_UP: AtomicBool = AtomicBool::new(false);
static GATE_VOL_DOWN: AtomicBool = AtomicBool::new(false);
/// 静音：桥接期始终开（见 refresh；HOGP 原生静音常无效）。
static GATE_MUTE: AtomicBool = AtomicBool::new(false);
/// 方向/OK：仅重映射到其它键时开（见 [`refresh_dpad_remap_gates`]）。
static GATE_LEFT: AtomicBool = AtomicBool::new(false);
static GATE_RIGHT: AtomicBool = AtomicBool::new(false);
static GATE_UP: AtomicBool = AtomicBool::new(false);
static GATE_DOWN: AtomicBool = AtomicBool::new(false);
static GATE_OK: AtomicBool = AtomicBool::new(false);
/// VK_HOME(0x24)：主页绑到非 Home 时开（BLE 有时发 0x24 而非 Browser Home 0xAC）。
static GATE_VK_HOME: AtomicBool = AtomicBool::new(false);

static ALLOW_PASS: LazyLock<Mutex<HashMap<u16, Instant>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static ARMED: LazyLock<Mutex<HashMap<u16, Instant>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static RECENT_CLAIM: LazyLock<Mutex<HashMap<u16, Instant>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// T1 遥控当前按住、且已被重映射走的原生 VK（如右方向→右Alt+空格时的 0x27）。
/// 仅在遥控按住期间吞掉该 VK，实体键盘同键在遥控未按时仍可用。
static T1_HELD_NATIVE: LazyLock<Mutex<HashSet<u16>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

struct BrowserHomeGuard {
    until: Instant,
    snapshot: HashSet<isize>,
    closed: bool,
}

static BROWSER_HOME_GUARD: LazyLock<Mutex<Option<BrowserHomeGuard>>> =
    LazyLock::new(|| Mutex::new(None));

/// RegisterHotKey 线程（吃掉系统级 Browser Search，堵住 APPCOMMAND 漏网）
static BRSEARCH_HOTKEY_STOP: AtomicBool = AtomicBool::new(false);
static BRSEARCH_HOTKEY_RUNNING: AtomicBool = AtomicBool::new(false);
static BRSEARCH_HOTKEY_TID: LazyLock<Mutex<Option<u32>>> = LazyLock::new(|| Mutex::new(None));
static FG_WATCHDOG_RUNNING: AtomicBool = AtomicBool::new(false);
static FG_WATCHDOG_UNTIL: LazyLock<Mutex<Option<Instant>>> = LazyLock::new(|| Mutex::new(None));
/// `RegisterWindowMessage("SHELLHOOK")` 消息号；0 = 未注册。
#[cfg(target_os = "windows")]
static SHELLHOOK_MSG: AtomicU32 = AtomicU32::new(0);
/// L1：壳层 APPCOMMAND_BROWSER_SEARCH 已吞次数（可观测）。
static SHELL_APPCOMMAND_SWALLOW_COUNT: AtomicU64 = AtomicU64::new(0);
/// L2：Raw/HID 见到 AC Search 次数。
static HID_AC_SEARCH_SEEN_COUNT: AtomicU64 = AtomicU64::new(0);
/// L3：FG 看门狗 Esc 次数。
static SEARCH_FG_ESCAPE_COUNT: AtomicU64 = AtomicU64::new(0);

const ARM_TTL: Duration = Duration::from_millis(120);
const ALLOW_TTL: Duration = Duration::from_millis(250);
const CLAIM_TTL: Duration = Duration::from_millis(200);
const BROWSER_HOME_GUARD_TTL: Duration = Duration::from_millis(900);
const HOTKEY_ID_BROWSER_SEARCH: i32 = 0x54_AA;
const HOTKEY_ID_BROWSER_HOME: i32 = 0x54_AC;
const HOTKEY_ID_BROWSER_BACK: i32 = 0x54_A6;
const HOTKEY_ID_APPS_MENU: i32 = 0x54_5D;

/// Win32 `APPCOMMAND_BROWSER_SEARCH`（开搜索）。注意：Favorites 才是 6。
pub const APPCOMMAND_BROWSER_SEARCH: i16 = 5;
/// `ShellProc` / `RegisterShellHookWindow` 的 `HSHELL_APPCOMMAND`。
pub const HSHELL_APPCOMMAND: usize = 12;

/// L1 策略缝：闸门开启时是否吞掉该 APPCOMMAND（仅 Browser Search）。
pub fn shell_should_swallow_appcommand(cmd: i16, gate_on: bool) -> bool {
    gate_on && cmd == APPCOMMAND_BROWSER_SEARCH
}

/// 从 `WM_APPCOMMAND` / `HSHELL_APPCOMMAND` 的 lParam 取出 app command（对齐 GET_APPCOMMAND_LPARAM）。
pub fn appcommand_from_lparam(lparam: isize) -> i16 {
    ((lparam >> 16) as u16 & 0x0FFF) as i16
}

/// 壳层/ShellHook 见到 APPCOMMAND：闸门开且为 Browser Search 则立刻 arm + L3 mop-up。
pub fn on_shell_appcommand_seen(cmd: i16) {
    if !shell_should_swallow_appcommand(cmd, is_enabled()) {
        return;
    }
    SHELL_APPCOMMAND_SWALLOW_COUNT.fetch_add(1, Ordering::Relaxed);
    log::info!("T1 shell_appcommand_swallow cmd={cmd}");
    arm_voice_browser_search();
    start_foreground_search_watchdog();
    dismiss_windows_search_async(false);
}

pub fn shell_appcommand_swallow_count() -> u64 {
    SHELL_APPCOMMAND_SWALLOW_COUNT.load(Ordering::Relaxed)
}

pub fn hid_ac_search_seen_count() -> u64 {
    HID_AC_SEARCH_SEEN_COUNT.load(Ordering::Relaxed)
}

pub fn search_fg_escape_count() -> u64 {
    SEARCH_FG_ESCAPE_COUNT.load(Ordering::Relaxed)
}

/// 与 `t1_shell_hook.dll` 共享：闸门开=1。名称必须与 DLL 一致（无尾 NUL）。
#[cfg(target_os = "windows")]
pub const SHELL_GATE_MAP_NAME: &str = "Local\\T1BrSearchShellGate";

#[cfg(target_os = "windows")]
const SHELL_GATE_MAP_NAME_Z: &str = "Local\\T1BrSearchShellGate\0";

#[cfg(target_os = "windows")]
static SHELL_GATE_MAP: LazyLock<Mutex<Option<isize>>> = LazyLock::new(|| Mutex::new(None));

fn publish_shell_gate(on: bool) {
    #[cfg(not(target_os = "windows"))]
    {
        let _ = on;
    }
    #[cfg(target_os = "windows")]
    {
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
        use windows::Win32::System::Memory::{
            CreateFileMappingW, MapViewOfFile, UnmapViewOfFile, FILE_MAP_ALL_ACCESS,
            PAGE_READWRITE, MEMORY_MAPPED_VIEW_ADDRESS,
        };

        let mut slot = SHELL_GATE_MAP.lock();
        if slot.is_none() {
            let name: Vec<u16> = SHELL_GATE_MAP_NAME_Z.encode_utf16().collect();
            let h = unsafe {
                CreateFileMappingW(
                    INVALID_HANDLE_VALUE,
                    None,
                    PAGE_READWRITE,
                    0,
                    4,
                    PCWSTR(name.as_ptr()),
                )
            };
            match h {
                Ok(handle) if !handle.is_invalid() => {
                    *slot = Some(handle.0 as isize);
                    log::info!("T1 shell gate mapping created");
                }
                Ok(_) | Err(_) => {
                    log::warn!("T1 shell gate CreateFileMapping failed");
                    return;
                }
            }
        }
        let Some(raw) = *slot else { return };
        let handle = HANDLE(raw as *mut std::ffi::c_void);
        let view = unsafe { MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, 4) };
        if view.Value.is_null() {
            log::warn!("T1 shell gate MapViewOfFile failed");
            return;
        }
        unsafe {
            *(view.Value as *mut u8) = if on { 1 } else { 0 };
            let _ = UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS { Value: view.Value });
        }
    }
}

/// Search/Start 飞出层所在进程（不含 explorer，整进程不可关）。
pub fn is_windows_search_or_start_process(path_or_name: &str) -> bool {
    let p = path_or_name.to_ascii_lowercase();
    let name = p.rsplit(['\\', '/']).next().unwrap_or(p.as_str());
    matches!(
        name,
        "searchhost.exe"
            | "searchapp.exe"
            | "searchui.exe"
            | "startmenuexperiencehost.exe"
            | "shellexperiencehost.exe"
    )
}

/// 仅认明确的开始/搜索粗类名。`Windows.UI.Core.CoreWindow` 太宽（设置/计算器），不认。
pub fn is_windows_search_or_start_class(class: &str) -> bool {
    let c = class.trim();
    c.eq_ignore_ascii_case("ImmersiveLauncher") || c.eq_ignore_ascii_case("SearchPane")
}

/// 真看到 Search/Start 或开始菜单可见时才 Esc，避免误关刚唤醒的映射应用。
pub fn should_tap_escape_for_shell(found_search_or_start: bool, launcher_visible: bool) -> bool {
    found_search_or_start || launcher_visible
}

/// 吞键闸门原因位：任一侧需要则开闸，全部清除才关。业务只 arm/disarm，不问对端是否存活。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum SwallowReason {
    UsbBridge = 1 << 0,
    BleBridge = 1 << 1,
}

static REASON_BITS: AtomicU32 = AtomicU32::new(0);

fn sync_gate_from_reasons() {
    let on = REASON_BITS.load(Ordering::SeqCst) != 0;
    let was = ENABLED.load(Ordering::SeqCst);
    if on == was {
        return;
    }
    set_enabled(on);
}

pub fn arm_reason(reason: SwallowReason) {
    REASON_BITS.fetch_or(reason as u32, Ordering::SeqCst);
    sync_gate_from_reasons();
}

pub fn disarm_reason(reason: SwallowReason) {
    REASON_BITS.fetch_and(!(reason as u32), Ordering::SeqCst);
    sync_gate_from_reasons();
}

pub fn reasons_active() -> u32 {
    REASON_BITS.load(Ordering::SeqCst)
}

/// 兼容旧调用：改为「是否仍有任一原因位」。
pub fn should_keep_swallow_gate(
    usb_alive: bool,
    ble_runtime_running: bool,
    ble_stopping: bool,
) -> bool {
    let _ = (usb_alive, ble_runtime_running, ble_stopping);
    REASON_BITS.load(Ordering::SeqCst) != 0
}

/// `foreground_process_hint` 形如 `SearchHost.exe pid=1234`。
pub fn foreground_hint_is_search_or_start(hint: &str) -> bool {
    let name = hint.split_whitespace().next().unwrap_or(hint);
    is_windows_search_or_start_process(name)
}

/// 看门狗：前台是 Search/Start 才 Esc；两次 Esc 至少隔 80ms，避免 Search 关掉后 Esc 打进微信。
pub const FG_WATCHDOG_ESC_GAP_MS: u64 = 80;

pub fn should_watchdog_escape_now(fg_is_shell: bool, since_last_esc: Option<Duration>) -> bool {
    if !fg_is_shell {
        return false;
    }
    match since_last_esc {
        None => true,
        Some(d) => d >= Duration::from_millis(FG_WATCHDOG_ESC_GAP_MS),
    }
}

/// 只开/关吞键闸门，不启 RegisterHotKey（测试缝；运行时由 `set_enabled` 一并开系统热键）。
pub fn set_gate(on: bool) {
    ENABLED.store(on, Ordering::SeqCst);
    publish_shell_gate(on);
    if !on {
        ALLOW_PASS.lock().clear();
        ARMED.lock().clear();
        RECENT_CLAIM.lock().clear();
        T1_HELD_NATIVE.lock().clear();
        *BROWSER_HOME_GUARD.lock() = None;
        crate::bridges::t1::inject::panic_clear_all_modifiers("swallow_gate_off");
    }
}

pub fn set_enabled(on: bool) {
    if !on {
        REASON_BITS.store(0, Ordering::SeqCst);
    }
    set_gate(on);
    if !on {
        stop_browser_search_hotkey();
    } else {
        start_browser_search_hotkey();
    }
}

pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::SeqCst)
}

/// 实体键盘上常见的键：绝对禁止 suppress（即使被误 arm）。
pub fn is_ubiquitous_keyboard_vk(vk: u16) -> bool {
    matches!(
        vk,
        // 编辑 / 导航
        0x08 | 0x09 | 0x0D | 0x14 | 0x1B | 0x20 | 0x21 | 0x22 | 0x23 | 0x24
            | 0x25 | 0x26 | 0x27 | 0x28 | 0x2C | 0x2D | 0x2E
            // 主键盘数字字母
            | 0x30..=0x39
            | 0x41..=0x5A
            // Win（实体键盘常用）
            | 0x5B | 0x5C
            // 小键盘
            | 0x60..=0x6F
            // F1–F24
            | 0x70..=0x87
            // 左右修饰键
            | 0xA0..=0xA5
            // OEM 标点
            | 0xBA..=0xC0
            | 0xDB..=0xDF
            | 0xE2
    )
}

/// 允许短时武装 / 闸门的冷门侧效应键（实体键盘极少单独依赖）。
pub fn is_armable_suppress_vk(vk: u16) -> bool {
    if is_ubiquitous_keyboard_vk(vk) {
        return false;
    }
    matches!(
        vk,
        0x5D // Apps
            | 0xA6
            | 0xA7
            | 0xAA // Browser Search（BLE 语音 AC Search → 0xAA）
            | 0xAC // Browser Home（非 VK_HOME）
            | 0xAD
            | 0xAE
            | 0xAF
    )
}

/// 常驻闸门：Apps / Browser_* 始终开；静音桥接期始终开；音量±/方向/OK/VK_HOME 仅重映射时开。
pub fn is_gate_vk(vk: u16) -> bool {
    match vk {
        0xAF => GATE_VOL_UP.load(Ordering::SeqCst),
        0xAE => GATE_VOL_DOWN.load(Ordering::SeqCst),
        0xAD => GATE_MUTE.load(Ordering::SeqCst),
        0x25 => GATE_LEFT.load(Ordering::SeqCst),
        0x26 => GATE_UP.load(Ordering::SeqCst),
        0x27 => GATE_RIGHT.load(Ordering::SeqCst),
        0x28 => GATE_DOWN.load(Ordering::SeqCst),
        0x0D => GATE_OK.load(Ordering::SeqCst),
        0x24 => GATE_VK_HOME.load(Ordering::SeqCst),
        0x5D | 0xA6 | 0xA7 | 0xAA | 0xAC => true,
        _ => false,
    }
}

/// 桥接开启时可能吞掉的 T1 原生侧效应 VK（不含空格/方向等常用键）。
pub fn is_t1_native_side_effect_vk(vk: u16) -> bool {
    is_armable_suppress_vk(vk)
}

/// 未绑定或同键（vol+→音量+）→ 不开闸，让系统 Consumer/VK 生效。
fn media_gate_needed(native_vk: u16, binding_vks: Option<&[u16]>) -> bool {
    match binding_vks {
        None | Some([]) => false,
        Some([vk]) if *vk == native_vk => false,
        Some(_) => true,
    }
}

/// 配置加载/保存后刷新音量/静音闸门。
/// 音量±：仅重映射时开闸；静音：有绑定则开闸（含同键 0xAD，HOGP 原生不可靠）。
pub fn refresh_media_gates_from_bindings(
    vol_plus: Option<&[u16]>,
    vol_minus: Option<&[u16]>,
    mute: Option<&[u16]>,
) {
    GATE_VOL_UP.store(media_gate_needed(0xAF, vol_plus), Ordering::SeqCst);
    GATE_VOL_DOWN.store(media_gate_needed(0xAE, vol_minus), Ordering::SeqCst);
    GATE_MUTE.store(
        match mute {
            None | Some([]) => false,
            Some(_) => true,
        },
        Ordering::SeqCst,
    );
    log::info!(
        "T1 media gates vol+={} vol-={} mute={}",
        GATE_VOL_UP.load(Ordering::SeqCst),
        GATE_VOL_DOWN.load(Ordering::SeqCst),
        GATE_MUTE.load(Ordering::SeqCst)
    );
}

/// 方向/OK：仅重映射到其它键时开闸（同键/未绑定透传）。
pub fn refresh_dpad_remap_gates(
    up: Option<&[u16]>,
    down: Option<&[u16]>,
    left: Option<&[u16]>,
    right: Option<&[u16]>,
    ok: Option<&[u16]>,
) {
    GATE_UP.store(media_gate_needed(0x26, up), Ordering::SeqCst);
    GATE_DOWN.store(media_gate_needed(0x28, down), Ordering::SeqCst);
    GATE_LEFT.store(media_gate_needed(0x25, left), Ordering::SeqCst);
    GATE_RIGHT.store(media_gate_needed(0x27, right), Ordering::SeqCst);
    GATE_OK.store(media_gate_needed(0x0D, ok), Ordering::SeqCst);
    log::info!(
        "T1 dpad remap gates up={} down={} left={} right={} ok={}",
        GATE_UP.load(Ordering::SeqCst),
        GATE_DOWN.load(Ordering::SeqCst),
        GATE_LEFT.load(Ordering::SeqCst),
        GATE_RIGHT.load(Ordering::SeqCst),
        GATE_OK.load(Ordering::SeqCst)
    );
}

/// 主页映射不是 VK_HOME(0x24) 时，LL 吞 0x24 并补注入（BLE 有时发 Home 而非 Browser Home）。
pub fn refresh_home_vk24_gate(home: Option<&[u16]>) {
    GATE_VK_HOME.store(media_gate_needed(0x24, home), Ordering::SeqCst);
    log::info!(
        "T1 home VK_HOME(0x24) gate={}",
        GATE_VK_HOME.load(Ordering::SeqCst)
    );
}

/// 原生 VK 与映射目标完全相同（单键）→ 同键映射，应透传、勿 hold/注入。
pub fn is_identity_binding(native_vk: Option<u16>, target_vks: &[u16]) -> bool {
    matches!((native_vk, target_vks), (Some(nv), [t]) if nv == *t)
}

/// 按钮侧效应 VK 与映射目标相同（如 vol_plus→0xAF）→ 透传系统行为。
pub fn is_side_effect_identity(button_id: &str, target_vks: &[u16]) -> bool {
    let side = side_effect_vks_for_button(button_id);
    matches!((side, target_vks), ([s], [t]) if *s == *t)
}

/// 同键或侧效应同键：不吞、不注入。静音永不透传（需注入）。
pub fn is_passthrough_binding(
    button_id: &str,
    native_vk: Option<u16>,
    target_vks: &[u16],
) -> bool {
    if button_id == "mute" {
        return false;
    }
    if target_vks.is_empty() {
        return true;
    }
    is_identity_binding(native_vk, target_vks) || is_side_effect_identity(button_id, target_vks)
}

/// WinUHid boot keyboard 不支持的媒体/浏览器键 → 必须 SendInput。
pub fn vks_need_sendinput(vks: &[u16]) -> bool {
    vks.iter().any(|&vk| {
        matches!(
            vk,
            0xAD | 0xAE | 0xAF | 0xA6 | 0xA7 | 0xAA | 0xAC | 0x5D
        )
    })
}

pub fn side_effect_vks_for_button(button_id: &str) -> &'static [u16] {
    match button_id {
        "vol_plus" => &[0xAF],
        "vol_minus" => &[0xAE],
        "mute" => &[0xAD],
        // 删除：只拦浏览器后退，绝不拦 Backspace（0x08）
        "delete" => &[0xA6],
        // 主页：只拦 Browser Home，绝不拦 VK_HOME(0x24) / Space
        "home" => &[0xAC],
        "menu" => &[0x5D],
        // 语音：拦 Browser Search（USB Voice Command 通常不走 0xAA；BLE AC Search 会）
        "voice" => &[0xAA],
        // 方向/OK 等：不武装原生箭头/Enter（否则实体键盘方向键失效）
        _ => &[],
    }
}

/// 语音键按下时立刻武装 0xAA（ATVV / HID 两条路径都要调）。
/// 闸门已常驻吞 0xAA，不再 hold——避免永不释放的按住吞。
pub fn arm_voice_browser_search() {
    if !is_enabled() {
        return;
    }
    arm_for_button("voice", Some(0xAA), &[]);
    log::info!("T1 arm voice native suppress vk=0xAA (Browser Search)");
}

/// 语音会话结束：闸门仍在，无需额外动作。
pub fn release_voice_browser_search() {}

fn prune_map(map: &mut HashMap<u16, Instant>, now: Instant) {
    map.retain(|_, until| *until > now);
}

pub fn allow_pass_vks(vks: &[u16]) {
    if !is_enabled() || vks.is_empty() {
        return;
    }
    let until = Instant::now() + ALLOW_TTL;
    let mut g = ALLOW_PASS.lock();
    for &vk in vks {
        // 闸门键（含 0xAA）永不可 allow_pass，否则 barsearch 会漏进系统
        if vk != 0 && !is_gate_vk(vk) {
            g.insert(vk, until);
        }
    }
}

pub fn arm_native_vk(vk: u16) {
    if vk == 0 || !is_enabled() || !is_armable_suppress_vk(vk) {
        return;
    }
    ARMED.lock().insert(vk, Instant::now() + ARM_TTL);
}

pub fn arm_for_button(button_id: &str, event_native_vk: Option<u16>, keep_vks: &[u16]) {
    let keep: HashSet<u16> = keep_vks.iter().copied().collect();
    if let Some(vk) = event_native_vk {
        // 映射目标 + 常用键：永不 arm/claim
        if !keep.contains(&vk) && is_armable_suppress_vk(vk) {
            arm_native_vk(vk);
            claim_vk(vk);
        }
    }
    for &vk in side_effect_vks_for_button(button_id) {
        if !keep.contains(&vk) && is_armable_suppress_vk(vk) {
            arm_native_vk(vk);
            claim_vk(vk);
        }
    }
    if button_id == "home" {
        arm_browser_home_guard();
    }
}

fn claim_vk(vk: u16) {
    if !is_armable_suppress_vk(vk) {
        return;
    }
    RECENT_CLAIM
        .lock()
        .insert(vk, Instant::now() + CLAIM_TTL);
}

fn recently_claimed(vk: u16, now: Instant) -> bool {
    let mut g = RECENT_CLAIM.lock();
    prune_map(&mut g, now);
    g.get(&vk).is_some_and(|u| *u > now)
}

fn is_allow_pass(vk: u16, now: Instant) -> bool {
    let mut g = ALLOW_PASS.lock();
    prune_map(&mut g, now);
    g.get(&vk).is_some_and(|u| *u > now)
}

fn is_armed(vk: u16, now: Instant) -> bool {
    let mut g = ARMED.lock();
    prune_map(&mut g, now);
    g.get(&vk).is_some_and(|u| *u > now)
}

/// 绝对不能按住吞的编辑键（即使误判 native=空格，也不能废掉实体键盘）。
/// 修饰键也禁止 hold-suppress：否则实体 Ctrl/Alt 会被吞且易与注入粘键叠加。
fn is_hold_suppress_forbidden(vk: u16) -> bool {
    matches!(
        vk,
        0x08 | 0x09 | 0x1B | 0x20 // Backspace / Tab / Esc / Space
            | 0x10 | 0x11 | 0x12 // generic modifiers
            | 0xA0..=0xA5 // L/R Shift/Ctrl/Alt
            | 0x5B | 0x5C // Win
    ) || (0x30..=0x39).contains(&vk)
        || (0x41..=0x5A).contains(&vk)
}

/// 遥控按下且该原生键已被映射走：开始按住吞键（方向/Enter 等）。
pub fn hold_suppress_native_vk(vk: u16) {
    if vk == 0 || !is_enabled() {
        return;
    }
    if is_hold_suppress_forbidden(vk) {
        log::warn!("T1 refuse hold-suppress edit key vk=0x{vk:02X} (protect physical keyboard)");
        return;
    }
    T1_HELD_NATIVE.lock().insert(vk);
    log::debug!("T1 hold-suppress native vk=0x{vk:02X}");
}

/// 遥控抬起：停止吞该原生键
pub fn release_suppress_native_vk(vk: u16) {
    if vk == 0 {
        return;
    }
    T1_HELD_NATIVE.lock().remove(&vk);
    log::debug!("T1 hold-suppress release vk=0x{vk:02X}");
}

pub fn clear_hold_suppress() {
    T1_HELD_NATIVE.lock().clear();
}

/// 注入目标是否与当前 hold-suppress 重叠（WinUHid 无 EXTRA_INFO，会被 LL 再吞）。
pub fn held_intersects(vks: &[u16]) -> bool {
    let held = T1_HELD_NATIVE.lock();
    vks.iter().any(|vk| held.contains(vk))
}

/// 注入放行窗口内：实体键检测不要误判为我们的注入。
pub fn is_temporarily_allowed(vk: u16) -> bool {
    is_allow_pass(vk, Instant::now())
}

/// 有绑定且**非同键**时按住吞原生，只走注入，避免原生+映射双发。
/// 未绑定 / 同键映射：不吞，让原生透传一次。
/// 空格等编辑键仍禁止 hold（保护实体键盘）。
pub fn should_hold_suppress_native(native_vk: u16, target_vks: &[u16]) -> bool {
    if native_vk == 0 || target_vks.is_empty() || is_hold_suppress_forbidden(native_vk) {
        return false;
    }
    // 同键（OK→Enter、左→左）：透传，禁止 hold+注入双发
    if is_identity_binding(Some(native_vk), target_vks) {
        return false;
    }
    true
}

/// USB/BLE 共用：按下时吞重映射原生键并武装侧效应闸门。
pub fn apply_native_press_policy(button_id: &str, native_vk: Option<u16>, target_vks: &[u16]) {
    if let Some(nv) = native_vk {
        if should_hold_suppress_native(nv, target_vks) {
            hold_suppress_native_vk(nv);
        }
    }
    arm_for_button(button_id, native_vk, target_vks);
}

/// USB/BLE 共用：抬起时停止 hold-suppress。
pub fn apply_native_release_policy(native_vk: Option<u16>) {
    if let Some(nv) = native_vk {
        release_suppress_native_vk(nv);
    }
}

/// LL 钩子。
///
/// `our_inject`：**仅**本进程 WinUHid/SendInput（EXTRA_INFO）。  
/// 不要把 `LLKHF_INJECTED` 当成 our_inject——HOGP/HID 栈常给 0xAA 打上 INJECTED，
/// 若因此放行，Browser Search 会漏进系统。
pub fn should_suppress_native(vk: u16, our_inject: bool, _down: bool) -> bool {
    if our_inject || !is_enabled() || vk == 0 {
        return false;
    }
    // 闸门键优先：原生侧效应 VK 无视 allow_pass
    if is_gate_vk(vk) {
        return true;
    }
    // 空格/退格/字母等：硬放行，优先于 HELD（防止 Home→Space 映射误 hold 废掉实体空格）
    if is_hold_suppress_forbidden(vk) {
        return false;
    }
    // 遥控正按住且已重映射的原生键：优先于 allow_pass（注入窗口不得漏出原生方向）
    if T1_HELD_NATIVE.lock().contains(&vk) {
        return true;
    }
    let now = Instant::now();
    if is_allow_pass(vk, now) {
        return false;
    }
    // 硬保证：其余常用键永不吞
    if is_ubiquitous_keyboard_vk(vk) {
        return false;
    }
    is_armable_suppress_vk(vk) && is_armed(vk, now)
}

/// HID 见 AC Search（02-21-02）：立刻吞 0xAA + 关 SearchHost（APPCOMMAND 可能绕过 LL）。
pub fn on_ac_search_hid_seen() {
    if !is_enabled() {
        return;
    }
    HID_AC_SEARCH_SEEN_COUNT.fetch_add(1, Ordering::Relaxed);
    log::info!("T1 hid_seen AC Search (02-21-02 / voice arm)");
    arm_voice_browser_search();
    start_foreground_search_watchdog();
    dismiss_windows_search_async(false);
}

/// LL 已吞 0xAA：再关一轮 SearchHost（热启动/APPCOMMAND 漏网兜底）。
/// 不得在 LL 回调里 EnumWindows / SendInput，只拉起看门狗线程。
pub fn on_browser_search_ll_swallowed() {
    if !is_enabled() {
        return;
    }
    arm_voice_browser_search();
    start_foreground_search_watchdog();
    dismiss_windows_search_async(false);
}

/// HID 见 AC Home（02-23-02）时武装 Browser Home + Chromium 新窗守卫。
/// 映射注入由调用方走 [`crate::bridges::t1::ble_keys::on_ll_gate_keydown`] / USB 同名函数。
pub fn on_ac_home_hid_seen() {
    if !is_enabled() {
        return;
    }
    arm_for_button("home", Some(0xAC), &[]);
    log::info!("T1 arm home native suppress vk=0xAC (Browser Home) from HID");
}

/// HID 见 AC Back（02-24-02）→ 武装删除侧效应（Browser Back）。
pub fn on_ac_back_hid_seen() {
    if !is_enabled() {
        return;
    }
    arm_for_button("delete", Some(0xA6), &[]);
    log::info!("T1 arm delete native suppress vk=0xA6 (Browser Back) from HID");
}

/// HID 见 Consumer Mute（02-E2）→ 武装静音闸门侧效应。
pub fn on_mute_hid_seen() {
    if !is_enabled() {
        return;
    }
    arm_for_button("mute", Some(0xAD), &[]);
    log::info!("T1 arm mute native suppress vk=0xAD from HID");
}

/// HID 见 Consumer Volume Up（02-E9）→ 武装音量+闸门侧效应。
pub fn on_vol_up_hid_seen() {
    if !is_enabled() {
        return;
    }
    arm_for_button("vol_plus", Some(0xAF), &[]);
    log::info!("T1 arm vol+ native suppress vk=0xAF from HID");
}

/// HID 见 Consumer Volume Down（02-EA）→ 武装音量-闸门侧效应。
pub fn on_vol_down_hid_seen() {
    if !is_enabled() {
        return;
    }
    arm_for_button("vol_minus", Some(0xAE), &[]);
    log::info!("T1 arm vol- native suppress vk=0xAE from HID");
}

/// Consumer HID 落盘后：武装 + LL 闸门同路径补映射（设备名偶发不匹配时仍能注入）。
pub fn inject_after_consumer_hid(button_hint_vk: u16) {
    if !is_enabled() {
        return;
    }
    match button_hint_vk {
        0xAC | 0x24 => on_ac_home_hid_seen(),
        0xA6 => on_ac_back_hid_seen(),
        0xAD => on_mute_hid_seen(),
        0xAF => on_vol_up_hid_seen(),
        0xAE => on_vol_down_hid_seen(),
        _ => {}
    }
    crate::bridges::t1::runtime::on_ll_gate_keydown(button_hint_vk);
    crate::bridges::t1::ble_keys::on_ll_gate_keydown(button_hint_vk);
}

/// 菜单键侧效应：闸门常驻即可。
pub fn arm_menu_apps_key() {
    if !is_enabled() {
        return;
    }
    arm_for_button("menu", Some(0x5D), &[]);
    log::info!("T1 arm menu native suppress vk=0x5D (Apps)");
}

pub fn release_menu_apps_key() {}

pub fn release_browser_home_key() {}

/// 占位：LL 钩子每轮调用的过期清理(保留以兼容 special_keys 调用)。
pub fn sweep_expired_pending() {}

/// LL / RegisterHotKey 吞掉闸门键后补映射（菜单/主页/删除/静音/音量/重映射方向·OK）。
/// USB runtime 与 BLE keys 共用去重表，避免同一次按键在 LL/HotKey/HID 与 USB/BLE
/// 双重路径下重复注入（菜单键一次按下会同时命中共用 RegisterHotKey + LL + 双 runtime）。
static GATE_INJECT_DEDUPE: LazyLock<Mutex<HashMap<String, Instant>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

const GATE_INJECT_DEDUPE_MS: u64 = 120;

/// 闸门键补映射去重（跨 USB/BLE/LL/HotKey 共用）。true = 应跳过本次注入。
pub fn should_skip_gate_inject(button_id: &str) -> bool {
    let mut g = GATE_INJECT_DEDUPE.lock();
    let now = Instant::now();
    g.retain(|_, at| now.duration_since(*at) < Duration::from_millis(GATE_INJECT_DEDUPE_MS));
    if g.contains_key(button_id) {
        return true;
    }
    g.insert(button_id.to_string(), now);
    false
}


/// 非 T1 键盘：回放被闸门误吞的侧效应键（Apps / Browser Home/Back / 可选 VK_HOME）。
///
/// **禁止**回放方向/OK/音量/静音等重映射闸门键：
/// - `allow_pass` 对闸门 VK 无效，注入后 LL 仍吞；
/// - Raw 再把注入当 foreign → `press`→`foreign` 死循环（实体/遥控左方向无限连发）。
/// **0xAA 绝不回放**——HOGP 常带 LLKHF_INJECTED，误判 foreign 再注入等于主动开搜索。
pub fn on_foreign_keyboard(vk: u16, pressed: bool) -> bool {
    if !is_enabled() || !is_gate_vk(vk) {
        return false;
    }
    if vk == 0xAA {
        log::debug!("T1 gate refuse foreign replay of BrowserSearch 0xAA");
        return false;
    }
    // 仅冷门侧效应；方向/OK/音量/静音禁止回放（防连击）
    if !matches!(vk, 0x5D | 0xAC | 0xA6 | 0x24) {
        log::debug!("T1 gate refuse foreign replay of remap-gate vk=0x{vk:02X}");
        return false;
    }
    let now = Instant::now();
    if recently_claimed(vk, now) {
        return false;
    }
    // 先 claim，防止 Raw 把我们的 SendInput 再当 foreign 打回来
    RECENT_CLAIM
        .lock()
        .insert(vk, Instant::now() + CLAIM_TTL);
    log::info!("T1 gate foreign replay vk=0x{vk:02X} pressed={pressed}");
    allow_pass_vks(&[vk]);
    let ok = if pressed {
        crate::bridges::t1::inject::press_vks(&[vk])
    } else {
        crate::bridges::t1::inject::release_vks(&[vk])
    };
    if !ok {
        log::warn!("T1 gate foreign replay failed vk=0x{vk:02X}");
    }
    ok
}

/// 是否允许对某闸门 VK 做 foreign 回放（单测缝）。
pub fn foreign_replay_allowed_for_gate_vk(vk: u16) -> bool {
    matches!(vk, 0x5D | 0xAC | 0xA6 | 0x24) && vk != 0xAA
}

pub fn arm_browser_home_guard() {
    if !is_enabled() {
        return;
    }
    let snapshot = snapshot_chromium_toplevel_hwnds();
    *BROWSER_HOME_GUARD.lock() = Some(BrowserHomeGuard {
        until: Instant::now() + BROWSER_HOME_GUARD_TTL,
        snapshot,
        closed: false,
    });
    log::info!("T1 browser-home guard armed (snapshot+single-close)");
    thread::spawn(|| {
        let start = Instant::now();
        for at_ms in [160_u64, 350, 600] {
            let elapsed = start.elapsed().as_millis() as u64;
            if elapsed < at_ms {
                thread::sleep(Duration::from_millis(at_ms - elapsed));
            }
            if try_close_one_new_chromium_window() {
                break;
            }
            if !browser_home_guard_pending() {
                break;
            }
        }
    });
}

pub fn maybe_arm_home_from_hid_hex(hex: &str) {
    if !is_enabled() {
        return;
    }
    let h = hex.trim().to_ascii_uppercase();
    if h.starts_with("02-23-02") {
        arm_for_button("home", None, &[]);
    }
}

/// BLE 语音键 HOGP 会发 Consumer AC Search（0x0221）。
/// LL 能吞 0xAA，但 **APPCOMMAND / 开始菜单** 不走键盘钩；关窗枚举又常为 0。
/// 因此：强关 Search/Start 相关进程顶层窗 +（键已松开时）补一次 Esc。
///
/// **硬约束：** WinEvent 常驻钩不得在未 arm 时关 Search/Start，否则用户点任务栏/
/// 开始菜单/搜索框会被立刻关掉（见 `search_dismiss_armed`）。
/// LL 吞掉的重映射方向/OK 等「来源暂未判明」的按键（vk, 吞键时刻）。
/// LL 拿不到设备来源；Raw Input 随后裁决：T1 设备补映射，真实键盘回放原生键。
static PENDING_SOURCE_GATE: LazyLock<Mutex<HashMap<u16, Instant>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

const PENDING_SOURCE_TTL: Duration = Duration::from_millis(150);
static SOURCE_WATCHDOG_RUNNING: AtomicBool = AtomicBool::new(false);

/// 该闸门键是否必须等 Raw 判明来源后才能补映射（方向/OK 等实体键盘常用键）。
/// Apps/Browser_* 等冷门侧效应键不受影响，LL 可直接补映射。
pub fn gate_needs_source_decision(vk: u16) -> bool {
    matches!(vk, 0x25 | 0x26 | 0x27 | 0x28 | 0x0D | 0x24)
}

/// 单一后台守护：Raw 超过窗口仍未裁决来源时，兜底回放原生键。
/// 这样即使 Raw Input 未运行/迟到达，实体键盘方向键/回车也不至于永久丢失。
fn ensure_source_watchdog() {
    if SOURCE_WATCHDOG_RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    thread::spawn(|| {
        log::info!(
            "T1 source-gate watchdog start ttl={}ms",
            PENDING_SOURCE_TTL.as_millis()
        );
        while SOURCE_WATCHDOG_RUNNING.load(Ordering::SeqCst) {
            thread::sleep(PENDING_SOURCE_TTL / 2);
            let now = Instant::now();
            let expired: Vec<u16> = {
                let mut g = PENDING_SOURCE_GATE.lock();
                let out: Vec<u16> = g
                    .iter()
                    .filter(|(_, until)| **until <= now)
                    .map(|(vk, _)| *vk)
                    .collect();
                for vk in &out {
                    g.remove(vk);
                }
                out
            };
            for vk in expired {
                // 兜底回放原生键（带 EXTRA_INFO，LL 会放行）。Raw 随后若确认是
                // T1 仍会注入映射；实体键盘则只需这一次原生回放。
                allow_pass_vks(&[vk]);
                let ok = crate::bridges::t1::inject::tap_single_vk(vk, 50);
                log::warn!("T1 source-gate watchdog replay vk=0x{vk:02X} ok={ok}");
            }
        }
        log::info!("T1 source-gate watchdog stopped");
    });
}

/// LL 吞键时记录来源待判定的重映射方向/OK/Home 键。
pub fn defer_gate_source(vk: u16, down: bool) {
    if !down || !gate_needs_source_decision(vk) {
        return;
    }
    PENDING_SOURCE_GATE
        .lock()
        .insert(vk, Instant::now() + PENDING_SOURCE_TTL);
    ensure_source_watchdog();
}

/// 该键的暂挂窗口是否仍有效（Raw 未在窗口内裁决策略时，允许把原键透传下去）。
pub fn source_decision_pending(vk: u16) -> bool {
    let mut g = PENDING_SOURCE_GATE.lock();
    let now = Instant::now();
    g.retain(|_, until| *until > now);
    g.get(&vk).is_some_and(|until| *until > now)
}

/// Raw 已判明来源（无论 T1 还是真实键盘）：清掉暂挂，避免错过窗口后的重复回放。
pub fn consume_source_pending(vk: u16) -> bool {
    PENDING_SOURCE_GATE.lock().remove(&vk).is_some()
}

/// 真实键盘按下的重映射方向/OK：用带 EXTRA_INFO 的 SendInput 回放原生键，
/// 不再补发遥控映射键。EXTRA_INFO 会让 LL 视为 our_inject 放行，不会死循环。
pub fn replay_foreign_gate_vk(vk: u16) -> bool {
    if !gate_needs_source_decision(vk) || !is_enabled() {
        return false;
    }
    let _ = consume_source_pending(vk);
    allow_pass_vks(&[vk]);
    let ok = crate::bridges::t1::inject::tap_single_vk(vk, 50);
    log::info!(
        "T1 foreign gate replay vk=0x{vk:02X} ok={ok} (EXTRA_INFO, LL passthrough)"
    );
    ok
}

static LAST_SEARCH_DISMISS: LazyLock<Mutex<Option<Instant>>> =
    LazyLock::new(|| Mutex::new(None));

/// 仅在遥控触发关 Search 的短窗口内为 true；用户手动开开始菜单/搜索时为 false。
fn search_dismiss_armed() -> bool {
    if let Some(until) = *FG_WATCHDOG_UNTIL.lock() {
        if Instant::now() < until {
            return true;
        }
    }
    if let Some(prev) = *LAST_SEARCH_DISMISS.lock() {
        // 与 async dismiss budget（~1.2s）+ 余量对齐；勿做成常开。
        if prev.elapsed() < Duration::from_millis(2000) {
            return true;
        }
    }
    false
}

/// 语音 HID 当下同步关一层；前台已是 Search/Start 时立刻 Esc（UWP 常不理 WM_CLOSE）。
pub fn dismiss_windows_search_now() {
    {
        let mut g = LAST_SEARCH_DISMISS.lock();
        *g = Some(Instant::now());
    }
    start_foreground_search_watchdog();
    let n = close_search_host_windows();
    if n > 0 {
        log::info!("T1 search dismiss sync closed={n}");
    }
    let fg = foreground_process_hint();
    if foreground_hint_is_search_or_start(&fg) {
        let escaped = tap_escape_once();
        let n2 = close_search_host_windows();
        log::info!("T1 search dismiss sync Esc fg={fg} escaped={escaped} closed2={n2}");
    }
}

/// APPCOMMAND 可在 HID/LL 之后才把 Search 抢到前台。短轮询前台，只对 Search/Start Esc。
pub fn start_foreground_search_watchdog() {
    let until = Instant::now() + Duration::from_millis(1500);
    {
        let mut g = FG_WATCHDOG_UNTIL.lock();
        *g = Some(g.map(|t| t.max(until)).unwrap_or(until));
    }
    if FG_WATCHDOG_RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    thread::spawn(|| {
        let mut last_esc: Option<Instant> = None;
        loop {
            let until = *FG_WATCHDOG_UNTIL.lock();
            let Some(until) = until else {
                break;
            };
            if Instant::now() >= until {
                break;
            }
            let fg = foreground_process_hint();
            let shell = foreground_hint_is_search_or_start(&fg);
            let since = last_esc.map(|t| t.elapsed());
            if should_watchdog_escape_now(shell, since) {
                let escaped = tap_escape_once();
                let n = close_search_host_windows();
                last_esc = Some(Instant::now());
                log::info!("T1 FG watchdog Esc fg={fg} escaped={escaped} closed={n}");
            } else if shell {
                let _ = close_search_host_windows();
            }
            thread::sleep(Duration::from_millis(8));
        }
        *FG_WATCHDOG_UNTIL.lock() = None;
        FG_WATCHDOG_RUNNING.store(false, Ordering::SeqCst);
    });
}

pub fn dismiss_windows_search_async(allow_escape: bool) {
    {
        let mut g = LAST_SEARCH_DISMISS.lock();
        if let Some(prev) = *g {
            if prev.elapsed() < Duration::from_millis(80) {
                return;
            }
        }
        *g = Some(Instant::now());
    }
    // 让 WinEvent 在遥控关 Search 窗口内才动手，而不是常驻杀 SearchHost。
    start_foreground_search_watchdog();
    thread::spawn(move || {
        let budget_ms = 1200_u64;
        let start = Instant::now();
        let mut closed_total = 0usize;
        let fg0 = foreground_process_hint();
        log::info!(
            "T1 search dismiss begin allow_escape={allow_escape} fg={fg0}"
        );
        closed_total += close_search_host_windows();
        let mut escaped = false;
        if allow_escape {
            thread::sleep(Duration::from_millis(35));
            closed_total += close_search_host_windows();
            let launcher = is_start_launcher_visible();
            if should_tap_escape_for_shell(closed_total > 0, launcher) {
                if tap_escape_once() {
                    escaped = true;
                    log::info!("T1 search dismiss Esc tapped (shell visible)");
                }
                closed_total += close_search_host_windows();
            }
        }
        while start.elapsed() < Duration::from_millis(budget_ms) {
            thread::sleep(Duration::from_millis(25));
            closed_total += close_search_host_windows();
            if allow_escape && !escaped {
                let launcher = is_start_launcher_visible();
                if should_tap_escape_for_shell(closed_total > 0, launcher) {
                    if tap_escape_once() {
                        escaped = true;
                        log::info!("T1 search dismiss Esc tapped (late shell)");
                    }
                }
            }
        }
        log::info!(
            "T1 search dismiss done closed={closed_total} budget={budget_ms}ms escape={allow_escape} escaped={escaped} fg_now={}",
            foreground_process_hint()
        );
    });
}

/// 结束闩锁后仍可用的短阻塞收口。
pub fn dismiss_windows_search_blocking(budget_ms: u64) {
    let start = Instant::now();
    let mut closed_total = 0usize;
    loop {
        closed_total += close_search_host_windows();
        if start.elapsed() >= Duration::from_millis(budget_ms.max(40)) {
            break;
        }
        thread::sleep(Duration::from_millis(25));
    }
    log::info!("T1 search dismiss blocked closed={closed_total} budget={budget_ms}ms");
}

fn close_search_host_windows() -> usize {
    #[cfg(not(target_os = "windows"))]
    {
        0
    }
    #[cfg(target_os = "windows")]
    {
        snapshot_search_host_hwnds()
            .into_iter()
            .map(|hwnd| {
                log::info!("T1 closing Windows Search/Start hwnd={hwnd:#x}");
                close_hwnd(hwnd);
            })
            .count()
    }
}

fn foreground_process_hint() -> String {
    #[cfg(not(target_os = "windows"))]
    {
        return "-".into();
    }
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION,
        };
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId,
        };
        let fg = unsafe { GetForegroundWindow() };
        if fg.0.is_null() {
            return "none".into();
        }
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(fg, Some(&mut pid)) };
        if pid == 0 {
            return format!("hwnd={:#x}", fg.0 as isize);
        }
        let Ok(proc) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) })
        else {
            return format!("pid={pid}");
        };
        let mut buf = [0u16; 260];
        let mut len = buf.len() as u32;
        let ok = unsafe {
            QueryFullProcessImageNameW(
                proc,
                PROCESS_NAME_WIN32,
                windows::core::PWSTR(buf.as_mut_ptr()),
                &mut len,
            )
        };
        if ok.is_err() || len == 0 {
            return format!("pid={pid}");
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        let name = path
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or(path.as_str())
            .to_string();
        format!("{name} pid={pid}")
    }
}

/// 键已松开后点一下 Esc，收掉开始菜单 / 搜索叠层（比关窗可靠）。
fn is_start_launcher_visible() -> bool {
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
        };
        use windows::Win32::UI::Shell::{AppVisibility, IAppVisibility};
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let av: IAppVisibility = match unsafe { CoCreateInstance(&AppVisibility, None, CLSCTX_ALL) }
        {
            Ok(v) => v,
            Err(_) => return false,
        };
        match unsafe { av.IsLauncherVisible() } {
            Ok(v) => v.as_bool(),
            Err(_) => false,
        }
    }
}

fn hwnd_is_search_or_start_shell(hwnd_val: isize) -> bool {
    #[cfg(not(target_os = "windows"))]
    {
        let _ = hwnd_val;
        false
    }
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION,
        };
        use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetWindowThreadProcessId};
        if hwnd_val == 0 {
            return false;
        }
        let hwnd = HWND(hwnd_val as *mut _);
        let mut class = [0u16; 256];
        let n = unsafe { GetClassNameW(hwnd, &mut class) };
        if n > 0 {
            let class_name = String::from_utf16_lossy(&class[..n as usize]);
            if is_windows_search_or_start_class(&class_name) {
                return true;
            }
        }
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        if pid == 0 {
            return false;
        }
        let Ok(proc) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) })
        else {
            return false;
        };
        let mut buf = [0u16; 260];
        let mut len = buf.len() as u32;
        let ok = unsafe {
            QueryFullProcessImageNameW(
                proc,
                PROCESS_NAME_WIN32,
                windows::core::PWSTR(buf.as_mut_ptr()),
                &mut len,
            )
        };
        if ok.is_err() || len == 0 {
            return false;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        is_windows_search_or_start_process(&path)
    }
}

fn tap_escape_once() -> bool {
    #[cfg(not(target_os = "windows"))]
    {
        return false;
    }
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
        };
        let extra = crate::bridges::shared::input_vk::EXTRA_INFO;
        let mk = |up: bool| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0x1B),
                    wScan: 0x01,
                    dwFlags: if up {
                        KEYEVENTF_KEYUP
                    } else {
                        Default::default()
                    },
                    time: 0,
                    dwExtraInfo: extra,
                },
            },
        };
        let inputs = [mk(false), mk(true)];
        let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
        let ok = sent as usize == inputs.len();
        if ok {
            SEARCH_FG_ESCAPE_COUNT.fetch_add(1, Ordering::Relaxed);
            log::info!("T1 search_fg_escape");
        }
        ok
    }
}

fn snapshot_search_host_hwnds() -> HashSet<isize> {
    #[cfg(not(target_os = "windows"))]
    {
        HashSet::new()
    }
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
        use windows::Win32::System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION,
        };
        use windows::Win32::UI::WindowsAndMessaging::{
            EnumWindows, GetClassNameW, GetForegroundWindow, GetWindowThreadProcessId,
        };

        fn is_search_process(pid: u32) -> bool {
            let Ok(proc) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) })
            else {
                return false;
            };
            let mut buf = [0u16; 260];
            let mut len = buf.len() as u32;
            let ok = unsafe {
                QueryFullProcessImageNameW(
                    proc,
                    PROCESS_NAME_WIN32,
                    windows::core::PWSTR(buf.as_mut_ptr()),
                    &mut len,
                )
            };
            if ok.is_err() || len == 0 {
                return false;
            }
            let path = String::from_utf16_lossy(&buf[..len as usize]);
            is_windows_search_or_start_process(&path)
        }

        fn window_class_name(hwnd: HWND) -> String {
            let mut class = [0u16; 256];
            let n = unsafe { GetClassNameW(hwnd, &mut class) };
            if n <= 0 {
                return String::new();
            }
            String::from_utf16_lossy(&class[..n as usize])
        }

        unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
            let set = &mut *(lparam.0 as *mut HashSet<isize>);
            if hwnd.0.is_null() {
                return BOOL(1);
            }
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
            if pid != 0 && is_search_process(pid) {
                set.insert(hwnd.0 as isize);
                return BOOL(1);
            }
            if is_windows_search_or_start_class(&window_class_name(hwnd)) {
                set.insert(hwnd.0 as isize);
            }
            BOOL(1)
        }

        let mut set = HashSet::new();
        let fg = unsafe { GetForegroundWindow() };
        if !fg.0.is_null() {
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(fg, Some(&mut pid)) };
            if pid != 0 && is_search_process(pid) {
                set.insert(fg.0 as isize);
            }
        }
        let _ = unsafe {
            EnumWindows(
                Some(enum_proc),
                LPARAM(&mut set as *mut HashSet<isize> as isize),
            )
        };
        set
    }
}

fn browser_home_guard_pending() -> bool {
    let now = Instant::now();
    let g = BROWSER_HOME_GUARD.lock();
    match g.as_ref() {
        Some(s) if s.until > now && !s.closed => true,
        _ => false,
    }
}

fn try_close_one_new_chromium_window() -> bool {
    #[cfg(not(target_os = "windows"))]
    {
        return false;
    }
    #[cfg(target_os = "windows")]
    {
        let mut g = BROWSER_HOME_GUARD.lock();
        let Some(state) = g.as_mut() else {
            return false;
        };
        let now = Instant::now();
        if state.closed || state.until <= now {
            return state.closed;
        }
        let snapshot = state.snapshot.clone();
        drop(g);

        let newcomers = snapshot_chromium_toplevel_hwnds()
            .into_iter()
            .filter(|h| !snapshot.contains(h))
            .collect::<Vec<_>>();
        let Some(&hwnd_val) = newcomers.first() else {
            return false;
        };

        let mut g = BROWSER_HOME_GUARD.lock();
        let Some(state) = g.as_mut() else {
            return false;
        };
        if state.closed || state.until <= Instant::now() {
            return state.closed;
        }
        state.closed = true;
        drop(g);

        log::info!("T1 browser-home guard closing one new hwnd={hwnd_val:#x}");
        close_hwnd(hwnd_val);
        true
    }
}

#[cfg(target_os = "windows")]
fn close_hwnd(hwnd_val: isize) {
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        PostMessageW, ShowWindow, SW_HIDE, WM_CLOSE,
    };
    let hwnd = HWND(hwnd_val as *mut _);
    const WM_SYSCOMMAND: u32 = 0x0112;
    const SC_CLOSE: usize = 0xF060;
    // 先 Hide 再 Close：UWP SearchHost 常忽略 WM_CLOSE，补 SYSCOMMAND。
    let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
    let _ = unsafe { PostMessageW(hwnd, WM_SYSCOMMAND, WPARAM(SC_CLOSE), LPARAM(0)) };
    let _ = unsafe { PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)) };
}

fn snapshot_chromium_toplevel_hwnds() -> HashSet<isize> {
    #[cfg(not(target_os = "windows"))]
    {
        HashSet::new()
    }
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{
            EnumWindows, GetClassNameW, GetWindow, GW_OWNER,
        };

        unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
            let set = &mut *(lparam.0 as *mut HashSet<isize>);
            if hwnd.0.is_null() {
                return BOOL(1);
            }
            let owner = unsafe { GetWindow(hwnd, GW_OWNER).unwrap_or(HWND(std::ptr::null_mut())) };
            if !owner.0.is_null() {
                return BOOL(1);
            }
            let mut class = [0u16; 256];
            let n = unsafe { GetClassNameW(hwnd, &mut class) };
            if n <= 0 {
                return BOOL(1);
            }
            let class_name = String::from_utf16_lossy(&class[..n as usize]);
            if class_name.contains("Chrome_WidgetWin_1") {
                set.insert(hwnd.0 as isize);
            }
            BOOL(1)
        }

        let mut set = HashSet::new();
        let _ = unsafe {
            EnumWindows(
                Some(enum_proc),
                LPARAM(&mut set as *mut HashSet<isize> as isize),
            )
        };
        set
    }
}

fn start_browser_search_hotkey() {
    if BRSEARCH_HOTKEY_RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    BRSEARCH_HOTKEY_STOP.store(false, Ordering::SeqCst);
    thread::spawn(|| {
        #[cfg(target_os = "windows")]
        {
            browser_search_hotkey_thread();
        }
        BRSEARCH_HOTKEY_RUNNING.store(false, Ordering::SeqCst);
        *BRSEARCH_HOTKEY_TID.lock() = None;
    });
}

fn stop_browser_search_hotkey() {
    BRSEARCH_HOTKEY_STOP.store(true, Ordering::SeqCst);
    #[cfg(target_os = "windows")]
    {
        if let Some(tid) = *BRSEARCH_HOTKEY_TID.lock() {
            unsafe {
                win_hotkey::PostThreadMessageW(tid, win_hotkey::WM_QUIT, 0, 0);
            }
        }
    }
}

#[cfg(target_os = "windows")]
#[allow(non_snake_case, non_camel_case_types, dead_code)]
mod win_hotkey {
    use std::ffi::c_void;
    pub type HWND = *mut c_void;
    pub type HMODULE = *mut c_void;
    pub type LRESULT = isize;
    pub type WPARAM = usize;
    pub type LPARAM = isize;
    pub type UINT = u32;
    pub type DWORD = u32;
    pub type ATOM = u16;
    pub type LONG_PTR = isize;

    pub const WM_QUIT: UINT = 0x0012;
    pub const WM_HOTKEY: UINT = 0x0312;
    pub const WM_DESTROY: UINT = 0x0002;
    pub const HWND_MESSAGE: isize = -3;
    pub const WH_SHELL: i32 = 10;
    pub const HSHELL_APPCOMMAND: i32 = 12;

    #[repr(C)]
    pub struct POINT {
        pub x: i32,
        pub y: i32,
    }
    #[repr(C)]
    pub struct MSG {
        pub hwnd: HWND,
        pub message: UINT,
        pub wParam: WPARAM,
        pub lParam: LPARAM,
        pub time: DWORD,
        pub pt: POINT,
    }
    #[repr(C)]
    pub struct WNDCLASSEXW {
        pub cbSize: UINT,
        pub style: UINT,
        pub lpfnWndProc: Option<unsafe extern "system" fn(HWND, UINT, WPARAM, LPARAM) -> LRESULT>,
        pub cbClsExtra: i32,
        pub cbWndExtra: i32,
        pub hInstance: HMODULE,
        pub hIcon: *mut c_void,
        pub hCursor: *mut c_void,
        pub hbrBackground: *mut c_void,
        pub lpszMenuName: *const u16,
        pub lpszClassName: *const u16,
        pub hIconSm: *mut c_void,
    }

    extern "system" {
        pub fn GetModuleHandleW(lpModuleName: *const u16) -> HMODULE;
        pub fn RegisterClassExW(lpWndClass: *const WNDCLASSEXW) -> ATOM;
        pub fn CreateWindowExW(
            dwExStyle: DWORD,
            lpClassName: *const u16,
            lpWindowName: *const u16,
            dwStyle: DWORD,
            x: i32,
            y: i32,
            nWidth: i32,
            nHeight: i32,
            hWndParent: HWND,
            hMenu: *mut c_void,
            hInstance: HMODULE,
            lpParam: *mut c_void,
        ) -> HWND;
        pub fn DestroyWindow(hWnd: HWND) -> i32;
        pub fn DefWindowProcW(hWnd: HWND, Msg: UINT, wParam: WPARAM, lParam: LPARAM) -> LRESULT;
        pub fn GetMessageW(
            lpMsg: *mut MSG,
            hWnd: HWND,
            wMsgFilterMin: UINT,
            wMsgFilterMax: UINT,
        ) -> i32;
        pub fn TranslateMessage(lpMsg: *const MSG) -> i32;
        pub fn DispatchMessageW(lpMsg: *const MSG) -> LRESULT;
        pub fn RegisterHotKey(hWnd: HWND, id: i32, fsModifiers: UINT, vk: UINT) -> i32;
        pub fn UnregisterHotKey(hWnd: HWND, id: i32) -> i32;
        pub fn PostThreadMessageW(idThread: DWORD, Msg: UINT, wParam: WPARAM, lParam: LPARAM)
            -> i32;
        pub fn GetCurrentThreadId() -> DWORD;
        pub fn SetWinEventHook(
            eventMin: DWORD,
            eventMax: DWORD,
            hmodWinEventProc: HMODULE,
            pfnWinEventProc: Option<
                unsafe extern "system" fn(
                    *mut c_void,
                    DWORD,
                    HWND,
                    i32,
                    i32,
                    DWORD,
                    DWORD,
                ),
            >,
            idProcess: DWORD,
            idThread: DWORD,
            dwFlags: DWORD,
        ) -> *mut c_void;
        pub fn UnhookWinEvent(hWinEventHook: *mut c_void) -> i32;
        pub fn RegisterWindowMessageW(lpString: *const u16) -> UINT;
        pub fn RegisterShellHookWindow(hwnd: HWND) -> i32;
        pub fn DeregisterShellHookWindow(hwnd: HWND) -> i32;
        pub fn SetWindowsHookExW(
            idHook: i32,
            lpfn: Option<unsafe extern "system" fn(i32, WPARAM, LPARAM) -> LRESULT>,
            hmod: HMODULE,
            dwThreadId: DWORD,
        ) -> *mut c_void;
        pub fn UnhookWindowsHookEx(hhk: *mut c_void) -> i32;
        pub fn CallNextHookEx(
            hhk: *mut c_void,
            nCode: i32,
            wParam: WPARAM,
            lParam: LPARAM,
        ) -> LRESULT;
        pub fn LoadLibraryW(lpLibFileName: *const u16) -> HMODULE;
        pub fn GetProcAddress(hModule: HMODULE, lpProcName: *const i8) -> *mut c_void;
        pub fn FreeLibrary(hLibModule: HMODULE) -> i32;
        pub fn GetLastError() -> DWORD;
    }

    pub const EVENT_SYSTEM_FOREGROUND: DWORD = 0x0003;
    pub const WINEVENT_OUTOFCONTEXT: DWORD = 0x0000;
    pub const WINEVENT_SKIPOWNPROCESS: DWORD = 0x0002;
    pub const OBJID_WINDOW: i32 = 0;
}

#[cfg(target_os = "windows")]
fn browser_search_hotkey_thread() {
    use std::mem;
    use std::ptr;
    use win_hotkey::*;
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};

    // OUTOFCONTEXT WinEvent 走 COM 封送，STA + 消息循环才能收到前台切换。
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    *BRSEARCH_HOTKEY_TID.lock() = Some(unsafe { GetCurrentThreadId() });

    let class_name: Vec<u16> = "T1BrSearchHotkeyClass\0".encode_utf16().collect();
    let hinstance = unsafe { GetModuleHandleW(ptr::null()) };
    let mut wc: WNDCLASSEXW = unsafe { mem::zeroed() };
    wc.cbSize = mem::size_of::<WNDCLASSEXW>() as u32;
    wc.lpfnWndProc = Some(brsearch_wndproc);
    wc.hInstance = hinstance;
    wc.lpszClassName = class_name.as_ptr();
    let _ = unsafe { RegisterClassExW(&wc) };

    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            class_name.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE as HWND,
            ptr::null_mut(),
            hinstance,
            ptr::null_mut(),
        )
    };
    if hwnd.is_null() {
        log::warn!("T1 BrowserSearch RegisterHotKey window failed");
        return;
    }

    // MOD_NOREPEAT = 0x4000 — Search / Home / Back(删除) / Apps
    let hotkeys: &[(i32, u32, &str)] = &[
        (HOTKEY_ID_BROWSER_SEARCH, 0xAA, "VK_BROWSER_SEARCH"),
        (HOTKEY_ID_BROWSER_HOME, 0xAC, "VK_BROWSER_HOME"),
        (HOTKEY_ID_BROWSER_BACK, 0xA6, "VK_BROWSER_BACK"),
        (HOTKEY_ID_APPS_MENU, 0x5D, "VK_APPS"),
    ];
    for &(id, vk, name) in hotkeys {
        let ok = unsafe { RegisterHotKey(hwnd, id, 0x4000, vk) };
        if ok == 0 {
            log::warn!("T1 RegisterHotKey({name}/0x{vk:02X}) failed — 依赖 LL+关窗兜底");
        } else {
            log::info!("T1 RegisterHotKey({name}/0x{vk:02X}) armed");
        }
    }

    let hook_flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;
    // 只钩前台：OBJECT_SHOW 会淹没消息循环，关 Search 反而变慢。
    let hook_fg = unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            ptr::null_mut(),
            Some(search_start_win_event),
            0,
            0,
            hook_flags,
        )
    };
    if hook_fg.is_null() {
        log::warn!("T1 Search/Start WinEvent hook failed");
    } else {
        log::info!("T1 Search/Start WinEvent hook armed");
    }

    // L1：ShellHook 最早感知 HSHELL_APPCOMMAND；WH_SHELL 若无 DLL 常失败。
    let shellhook_name: Vec<u16> = "SHELLHOOK\0".encode_utf16().collect();
    let shellhook_msg = unsafe { RegisterWindowMessageW(shellhook_name.as_ptr()) };
    if shellhook_msg != 0 {
        SHELLHOOK_MSG.store(shellhook_msg, Ordering::SeqCst);
        if unsafe { RegisterShellHookWindow(hwnd) } != 0 {
            log::info!("T1 ShellHook window registered msg=0x{shellhook_msg:04X}");
        } else {
            log::warn!("T1 RegisterShellHookWindow failed");
        }
    } else {
        log::warn!("T1 RegisterWindowMessage(SHELLHOOK) failed");
    }

    // L1 硬拦：注入 t1_shell_hook.dll 的 WH_SHELL（无 DLL 则退回 exe 内联尝试）。
    publish_shell_gate(is_enabled());
    let (shell_hook, shell_dll) = install_wh_shell_hook(hinstance);
    if shell_hook.is_null() {
        log::warn!("T1 WH_SHELL hook failed — rely on ShellHook+HotKey+LL+L3");
    } else if shell_dll.is_null() {
        log::info!("T1 WH_SHELL hook armed (in-process fallback)");
    } else {
        log::info!("T1 WH_SHELL hook armed via t1_shell_hook.dll");
    }

    let mut msg: MSG = unsafe { mem::zeroed() };
    loop {
        if BRSEARCH_HOTKEY_STOP.load(Ordering::SeqCst) {
            break;
        }
        let ret = unsafe { GetMessageW(&mut msg, ptr::null_mut(), 0, 0) };
        if ret <= 0 {
            break;
        }
        if msg.message == WM_HOTKEY {
            let id = msg.wParam as i32;
            match id {
                HOTKEY_ID_BROWSER_SEARCH => {
                    log::info!("T1 BrowserSearch hotkey consumed");
                    crate::bridges::t1::ble_keys::on_browser_search_hotkey();
                }
                HOTKEY_ID_BROWSER_HOME => {
                    log::info!("T1 BrowserHome hotkey consumed — arm + map inject");
                    on_ac_home_hid_seen();
                    crate::bridges::t1::runtime::on_ll_gate_keydown(0xAC);
                    crate::bridges::t1::ble_keys::on_ll_gate_keydown(0xAC);
                }
                HOTKEY_ID_BROWSER_BACK => {
                    log::info!("T1 BrowserBack hotkey consumed — map inject delete");
                    crate::bridges::t1::runtime::on_ll_gate_keydown(0xA6);
                    crate::bridges::t1::ble_keys::on_ll_gate_keydown(0xA6);
                }
                HOTKEY_ID_APPS_MENU => {
                    log::info!("T1 Apps/Menu hotkey consumed — arm + map inject");
                    arm_menu_apps_key();
                    crate::bridges::t1::runtime::on_ll_gate_keydown(0x5D);
                    crate::bridges::t1::ble_keys::on_ll_gate_keydown(0x5D);
                }
                _ => {}
            }
            continue;
        }
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    unsafe {
        if !shell_hook.is_null() {
            UnhookWindowsHookEx(shell_hook);
        }
        if !shell_dll.is_null() {
            FreeLibrary(shell_dll);
        }
        let _ = DeregisterShellHookWindow(hwnd);
        SHELLHOOK_MSG.store(0, Ordering::SeqCst);
        if !hook_fg.is_null() {
            UnhookWinEvent(hook_fg);
        }
        UnregisterHotKey(hwnd, HOTKEY_ID_BROWSER_SEARCH);
        UnregisterHotKey(hwnd, HOTKEY_ID_BROWSER_HOME);
        UnregisterHotKey(hwnd, HOTKEY_ID_BROWSER_BACK);
        UnregisterHotKey(hwnd, HOTKEY_ID_APPS_MENU);
        DestroyWindow(hwnd);
        CoUninitialize();
    }
    log::info!("T1 side-effect RegisterHotKey stopped");

    fn shell_hook_dll_candidates() -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                out.push(dir.join("t1_shell_hook.dll"));
            }
        }
        let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        out.push(manifest.join("target").join("debug").join("t1_shell_hook.dll"));
        out.push(manifest.join("target").join("release").join("t1_shell_hook.dll"));
        // Workspace target when CARGO_TARGET_DIR overridden
        if let Ok(td) = std::env::var("CARGO_TARGET_DIR") {
            let td = std::path::PathBuf::from(td);
            out.push(td.join("debug").join("t1_shell_hook.dll"));
            out.push(td.join("release").join("t1_shell_hook.dll"));
        }
        out
    }

    fn install_wh_shell_hook(hinstance: HMODULE) -> (*mut c_void, HMODULE) {
        for path in shell_hook_dll_candidates() {
            if !path.is_file() {
                continue;
            }
            let wide: Vec<u16> = path
                .to_string_lossy()
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let dll = unsafe { LoadLibraryW(wide.as_ptr()) };
            if dll.is_null() {
                log::warn!(
                    "T1 LoadLibraryW({}) failed err={}",
                    path.display(),
                    unsafe { GetLastError() }
                );
                continue;
            }
            let proc_name = b"T1ShellProc\0";
            let sym = unsafe { GetProcAddress(dll, proc_name.as_ptr() as *const i8) };
            if sym.is_null() {
                log::warn!("T1 GetProcAddress(T1ShellProc) failed in {}", path.display());
                unsafe { FreeLibrary(dll) };
                continue;
            }
            let hook = unsafe {
                SetWindowsHookExW(
                    WH_SHELL,
                    Some(std::mem::transmute::<
                        *mut c_void,
                        unsafe extern "system" fn(i32, WPARAM, LPARAM) -> LRESULT,
                    >(sym)),
                    dll,
                    0,
                )
            };
            if hook.is_null() {
                log::warn!(
                    "T1 SetWindowsHookEx WH_SHELL via DLL failed err={}",
                    unsafe { GetLastError() }
                );
                unsafe { FreeLibrary(dll) };
                continue;
            }
            log::info!("T1 loaded shell hook DLL {}", path.display());
            return (hook, dll);
        }
        // Fallback: in-process (usually fails for global WH_SHELL)
        let hook = unsafe {
            SetWindowsHookExW(WH_SHELL, Some(shell_appcommand_proc), hinstance, 0)
        };
        (hook, ptr::null_mut())
    }

    unsafe extern "system" fn shell_appcommand_proc(
        n_code: i32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if n_code < 0 {
            return CallNextHookEx(ptr::null_mut(), n_code, wparam, lparam);
        }
        if n_code == win_hotkey::HSHELL_APPCOMMAND {
            let cmd = appcommand_from_lparam(lparam);
            if shell_should_swallow_appcommand(cmd, is_enabled()) {
                on_shell_appcommand_seen(cmd);
                return 1;
            }
        }
        CallNextHookEx(ptr::null_mut(), n_code, wparam, lparam)
    }

    unsafe extern "system" fn search_start_win_event(
        _hook: *mut std::ffi::c_void,
        event: UINT,
        hwnd: HWND,
        id_object: i32,
        _id_child: i32,
        _thread: DWORD,
        _time: DWORD,
    ) {
        // 未闸门 / 未在遥控关 Search 窗口内：绝不碰用户任务栏/开始菜单/搜索。
        if !is_enabled() || !search_dismiss_armed() {
            return;
        }
        if !hwnd.is_null()
            && (id_object == OBJID_WINDOW || event == EVENT_SYSTEM_FOREGROUND)
            && hwnd_is_search_or_start_shell(hwnd as isize)
        {
            log::info!("T1 WinEvent closing Search/Start hwnd={:#x}", hwnd as isize);
            close_hwnd(hwnd as isize);
        }
        if event == EVENT_SYSTEM_FOREGROUND {
            let n = close_search_host_windows();
            let fg = foreground_process_hint();
            let mut escaped = false;
            if foreground_hint_is_search_or_start(&fg) {
                escaped = tap_escape_once();
            }
            if n > 0 || escaped {
                log::info!("T1 WinEvent FG sweep closed={n} escaped={escaped} fg={fg}");
            }
        }
    }

    unsafe extern "system" fn brsearch_wndproc(
        hwnd: HWND,
        msg: UINT,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        let shellhook = SHELLHOOK_MSG.load(Ordering::SeqCst);
        if shellhook != 0 && msg == shellhook {
            if wparam == win_hotkey::HSHELL_APPCOMMAND as WPARAM {
                let cmd = appcommand_from_lparam(lparam);
                on_shell_appcommand_seen(cmd);
            }
            return 0;
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_appcommand_lparam_extracts_cmd() {
        // HIWORD(lParam) & 0x0FFF == cmd（对齐 GET_APPCOMMAND_LPARAM）
        assert_eq!(appcommand_from_lparam((5isize) << 16), 5);
        assert_eq!(appcommand_from_lparam((6isize) << 16), 6);
        assert!(shell_should_swallow_appcommand(5, true));
        assert!(!shell_should_swallow_appcommand(5, false));
    }

    #[test]
    fn search_dismiss_not_armed_by_default() {
        *LAST_SEARCH_DISMISS.lock() = None;
        *FG_WATCHDOG_UNTIL.lock() = None;
        assert!(
            !search_dismiss_armed(),
            "WinEvent must not close user Start/Search without remote arm"
        );
    }

    #[test]
    fn ubiquitous_keys_never_suppressed() {
        set_enabled(true);
        // 即使误 arm，常用键也不得吞
        for vk in [0x08u16, 0x20, 0x24, 0x25, 0x26, 0x27, 0x28, 0x0D, 0x41, 0x1B] {
            arm_native_vk(vk); // 应被拒绝
            assert!(
                !should_suppress_native(vk, false, true),
                "vk=0x{vk:02X} must pass"
            );
        }
        set_enabled(false);
    }

    #[test]
    fn mapping_space_does_not_arm_space() {
        set_enabled(true);
        arm_for_button("home", None, &[0x20]);
        assert!(!should_suppress_native(0x20, false, true));
        assert!(!should_suppress_native(0x08, false, true));
        assert!(!should_suppress_native(0x24, false, true));
        set_enabled(false);
    }

    #[test]
    fn dpad_native_vk_not_armed() {
        set_enabled(true);
        // 左键映射到 G：不得武装 VK_LEFT，否则实体左失效
        arm_for_button("left", Some(0x25), &[0x47]);
        assert!(!should_suppress_native(0x25, false, true));
        assert!(!should_suppress_native(0x47, false, true));
        set_enabled(false);
    }

    #[test]
    fn delete_does_not_arm_backspace() {
        set_enabled(true);
        arm_for_button("delete", Some(0xA6), &[]);
        assert!(!should_suppress_native(0x08, false, true));
        set_enabled(false);
    }

    #[test]
    fn apps_gate_still_works() {
        set_enabled(true);
        assert!(is_gate_vk(0x5D));
        assert!(should_suppress_native(0x5D, false, true));
        // 未刷新重映射前，音量闸门默认关（未绑定透传）；静音 refresh 后始终开
        assert!(!is_gate_vk(0xAF));
        refresh_media_gates_from_bindings(Some(&[0x20]), Some(&[0x20]), Some(&[0x20]));
        assert!(is_gate_vk(0xAF));
        assert!(should_suppress_native(0xAF, false, true));
        assert!(is_gate_vk(0xAD), "mute always gated");
        refresh_media_gates_from_bindings(Some(&[0xAF]), Some(&[0xAE]), Some(&[0xAD]));
        assert!(!is_gate_vk(0xAF), "identity vol+ must not gate");
        assert!(is_gate_vk(0xAD), "mute stays gated even identity");
        set_enabled(false);
    }

    #[test]
    fn dpad_remap_gates_only_when_non_identity() {
        set_enabled(true);
        refresh_dpad_remap_gates(None, None, Some(&[0x46]), Some(&[0x27]), Some(&[0x0D]));
        assert!(is_gate_vk(0x25), "left→F must gate");
        assert!(!is_gate_vk(0x27), "right identity must not gate");
        assert!(!is_gate_vk(0x0D), "ok identity must not gate");
        assert!(!is_gate_vk(0x26), "unbound up must not gate");
        refresh_dpad_remap_gates(None, None, None, None, None);
        assert!(!is_gate_vk(0x25));
        set_enabled(false);
    }

    #[test]
    fn foreign_replay_refuses_remap_gate_vks() {
        set_enabled(true);
        refresh_dpad_remap_gates(
            Some(&[0x57]),
            Some(&[0x53]),
            Some(&[0x41]),
            Some(&[0x44]),
            Some(&[0x20]),
        );
        refresh_media_gates_from_bindings(Some(&[0x20]), Some(&[0x20]), Some(&[0x20]));
        assert!(is_gate_vk(0x25));
        assert!(is_gate_vk(0x0D));
        assert!(is_gate_vk(0xAF));
        assert!(is_gate_vk(0xAD));
        assert!(
            !foreign_replay_allowed_for_gate_vk(0x25),
            "left remap must not foreign-replay"
        );
        assert!(!foreign_replay_allowed_for_gate_vk(0x0D));
        assert!(!foreign_replay_allowed_for_gate_vk(0xAF));
        assert!(!foreign_replay_allowed_for_gate_vk(0xAD));
        assert!(!foreign_replay_allowed_for_gate_vk(0xAA));
        assert!(foreign_replay_allowed_for_gate_vk(0x5D));
        assert!(foreign_replay_allowed_for_gate_vk(0xAC));
        assert!(foreign_replay_allowed_for_gate_vk(0xA6));
        // 调用路径：方向键不得进入注入（返回 false）
        assert!(!on_foreign_keyboard(0x25, true));
        assert!(!on_foreign_keyboard(0x0D, true));
        assert!(!on_foreign_keyboard(0xAF, true));
        refresh_dpad_remap_gates(None, None, None, None, None);
        refresh_media_gates_from_bindings(Some(&[0xAF]), Some(&[0xAE]), Some(&[0xAD]));
        set_enabled(false);
    }

    #[test]
    fn browser_search_is_gate_when_t1_enabled() {
        set_enabled(true);
        assert!(is_armable_suppress_vk(0xAA));
        assert!(is_gate_vk(0xAA));
        assert!(should_suppress_native(0xAA, false, true));
        assert!(!should_suppress_native(0xAA, true, true)); // 仅豁免我们自己的注入
        // HID 栈 LLKHF_INJECTED 在调用侧已不当作 our_inject；闸门仍应吞
        assert!(should_suppress_native(0xAA, false, true));
        set_enabled(false);
    }

    /// 回归：映射键 allow_pass / 误 allow 0xAA 时，Browser Search 仍必须被吞。
    /// （此前 allow_pass 优先于 gate，会把 barsearch 放进系统。）
    #[test]
    fn browser_search_never_opened_by_allow_pass() {
        set_enabled(true);
        allow_pass_vks(&[0xAA]);
        assert!(
            should_suppress_native(0xAA, false, true),
            "0xAA must stay swallowed even after allow_pass_vks([0xAA])"
        );
        // 典型语音映射放行窗口也不得波及 0xAA
        allow_pass_vks(&[0xA5, 0xA2, 0x5B, 0x12]);
        assert!(should_suppress_native(0xAA, false, true));
        assert!(!should_suppress_native(0xA5, false, true)); // 映射目标仍可放行
        set_enabled(false);
    }

    #[test]
    fn browser_search_stays_swallowed_across_voice_arm_release_inject() {
        set_enabled(true);
        arm_voice_browser_search();
        assert!(should_suppress_native(0xAA, false, true));
        // 模拟注入前后：放行映射键 + release 语音闸门
        allow_pass_vks(&[0xA5]);
        release_voice_browser_search();
        assert!(should_suppress_native(0xAA, false, true));
        allow_pass_vks(&[0xA5]);
        assert!(should_suppress_native(0xAA, false, true));
        set_enabled(false);
    }

    #[test]
    fn voice_side_effect_arms_browser_search() {
        set_enabled(true);
        assert_eq!(side_effect_vks_for_button("voice"), &[0xAA]);
        arm_voice_browser_search();
        assert!(should_suppress_native(0xAA, false, true));
        release_voice_browser_search();
        // 闸门仍在
        assert!(should_suppress_native(0xAA, false, true));
        set_enabled(false);
    }

    #[test]
    fn space_never_hold_suppressed_even_if_requested() {
        set_enabled(true);
        assert!(!should_hold_suppress_native(0x20, &[0xAC]));
        hold_suppress_native_vk(0x20); // 必须拒绝
        assert!(!should_suppress_native(0x20, false, true));
        // Home→Space：实体空格始终可用
        arm_for_button("home", Some(0xAC), &[0x20]);
        assert!(!should_suppress_native(0x20, false, true));
        assert!(should_suppress_native(0xAC, false, true)); // 仍吞 Browser Home
        set_enabled(false);
    }

    #[test]
    fn remapped_right_held_suppresses_only_while_held() {
        set_enabled(true);
        assert!(!should_suppress_native(0x27, false, true));
        hold_suppress_native_vk(0x27);
        assert!(should_suppress_native(0x27, false, true));
        // 实体其它键仍放行
        assert!(!should_suppress_native(0x25, false, true));
        assert!(!should_suppress_native(0x20, false, true));
        release_suppress_native_vk(0x27);
        assert!(!should_suppress_native(0x27, false, true));
        set_enabled(false);
    }

    #[test]
    fn held_intersects_detects_overlap() {
        set_enabled(true);
        clear_hold_suppress();
        assert!(!held_intersects(&[0x28]));
        hold_suppress_native_vk(0x28);
        assert!(held_intersects(&[0x28]));
        assert!(held_intersects(&[0x41, 0x28]));
        assert!(!held_intersects(&[0x41]));
        release_suppress_native_vk(0x28);
        set_enabled(false);
    }

    #[test]
    fn should_hold_suppress_when_bound_including_identity() {
        // 重映射：吞原生
        assert!(should_hold_suppress_native(0x27, &[0xA5, 0x20]));
        // 同键映射：不吞（透传，避免原生+注入双发）
        assert!(!should_hold_suppress_native(0x27, &[0x27]));
        assert!(!should_hold_suppress_native(0x28, &[0x28]));
        assert!(!should_hold_suppress_native(0x0D, &[0x0D]));
        // 未绑定：不吞，原生透传
        assert!(!should_hold_suppress_native(0x27, &[]));
        assert!(!should_hold_suppress_native(0, &[0x20]));
    }

    #[test]
    fn passthrough_binding_detects_identity_and_side_effect() {
        assert!(is_passthrough_binding("ok", Some(0x0D), &[0x0D]));
        assert!(is_passthrough_binding("vol_plus", None, &[0xAF]));
        assert!(!is_passthrough_binding("home", Some(0xAC), &[0x20]));
        assert!(!is_passthrough_binding("mute", Some(0xAD), &[0xAD]));
        assert!(!is_passthrough_binding("mute", Some(0xAD), &[0x52]));
        assert!(is_passthrough_binding("up", Some(0x26), &[]));
        assert!(vks_need_sendinput(&[0xAF]));
        assert!(!vks_need_sendinput(&[0x0D]));
    }

    #[test]
    fn gate_covers_apps_search_and_home() {
        assert!(is_gate_vk(0x5D));
        assert!(is_gate_vk(0xAA));
        assert!(is_gate_vk(0xAC));
        assert!(!is_gate_vk(0x0D));
        assert!(!is_gate_vk(0x08));
        assert!(!is_gate_vk(0x24));
        assert!(!is_gate_vk(0x20));
        refresh_home_vk24_gate(Some(&[0x20]));
        assert!(is_gate_vk(0x24), "home→Space must gate VK_HOME");
        refresh_home_vk24_gate(Some(&[0x24]));
        assert!(!is_gate_vk(0x24), "home→Home identity must not gate");
    }

    #[test]
    fn browser_home_is_gate_when_t1_enabled() {
        set_enabled(true);
        assert!(is_armable_suppress_vk(0xAC));
        assert!(is_gate_vk(0xAC));
        assert!(should_suppress_native(0xAC, false, true));
        set_enabled(false);
    }

    #[test]
    fn dpad_remap_gate_needs_source_decision() {
        assert!(gate_needs_source_decision(0x25));
        assert!(gate_needs_source_decision(0x26));
        assert!(gate_needs_source_decision(0x27));
        assert!(gate_needs_source_decision(0x28));
        assert!(gate_needs_source_decision(0x0D));
        assert!(gate_needs_source_decision(0x24));
        assert!(!gate_needs_source_decision(0x5D), "Apps 键 LL 可直接补映射");
        assert!(!gate_needs_source_decision(0xAC));
        assert!(!gate_needs_source_decision(0xAF));
    }

    #[test]
    fn deferred_gate_source_is_consumable_and_expires() {
        defer_gate_source(0x26, true);
        assert!(source_decision_pending(0x26));
        assert!(consume_source_pending(0x26));
        assert!(!source_decision_pending(0x26));

        // 非 down 不暂挂
        defer_gate_source(0x25, false);
        assert!(!source_decision_pending(0x25));

        // 冷门侧效应键不暂挂
        defer_gate_source(0x5D, true);
        assert!(!source_decision_pending(0x5D));
    }

    #[test]
    fn replay_foreign_gate_vk_clears_pending_only_for_dpad() {
        defer_gate_source(0x27, true);
        assert!(replay_foreign_gate_vk(0x27));
        assert!(!source_decision_pending(0x27));
        // 冷门键不应回放
        defer_gate_source(0x5D, true);
        assert!(!replay_foreign_gate_vk(0x5D));
    }

    #[test]
    fn gate_inject_dedupe_is_shared_across_paths() {
        // 模拟 menu 键同时命中了 LL、RegisterHotKey、USB runtime、BLE runtime：
        // 第一次会允许，紧随其后的重复调用必须被去重（一次物理按键只注入一次）。
        assert!(!should_skip_gate_inject("menu"));
        assert!(should_skip_gate_inject("menu"));
        assert!(should_skip_gate_inject("menu"));
        // 不同按钮互不影响
        assert!(!should_skip_gate_inject("home"));
        assert!(should_skip_gate_inject("home"));
    }
}
