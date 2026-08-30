//! 把 `on_firmware_voice_key` 接到 `voice_dispatch` 工作线程。

use std::sync::atomic::{AtomicBool, Ordering};

static INSTALLED: AtomicBool = AtomicBool::new(false);

/// 幂等：钩子启动时调用一次即可。
pub fn install() {
    if INSTALLED.swap(true, Ordering::AcqRel) {
        return;
    }
    crate::bridges::xiaomi::voice_dispatch::set_sink(|down| {
        crate::bridges::xiaomi::key_mapping::on_firmware_voice_key(down);
    });
    log::info!("XIAOMI VOICE worker sink installed (on_firmware_voice_key)");
}
