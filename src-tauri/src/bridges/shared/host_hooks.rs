//! T1/共享主机钩子：虚拟 HID 和弦标记、录入钩（不依赖 xiaomi 模块路径）

use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

static VIRTUAL_HID_CHORD: Mutex<Option<Vec<u16>>> = Mutex::new(None);
static CAPTURE_HOOK_ARMED: AtomicBool = AtomicBool::new(false);

pub fn set_virtual_hid_chord_held(vks: Option<&[u16]>) {
    *VIRTUAL_HID_CHORD.lock() = vks.map(|v| v.to_vec());
}

pub fn virtual_hid_chord_held() -> Option<Vec<u16>> {
    VIRTUAL_HID_CHORD.lock().clone()
}

/// T1 启动按键桥时武装录入相关钩；实现放在 shared，避免 T1→xiaomi。
pub fn ensure_hook_for_capture() {
    if CAPTURE_HOOK_ARMED.swap(true, Ordering::AcqRel) {
        return;
    }
    // 快捷键录入走 shared::shortcut_capture；此处仅标记已请求。
    log::info!("shared ensure_hook_for_capture armed (T1)");
}
