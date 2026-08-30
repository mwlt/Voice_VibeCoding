//! 按键实际输出诊断：固件监视 VK 集合（漏 F5 / 双触发箭头等）。
//!
//!   cargo test --manifest-path src-tauri/Cargo.toml --test key_output_watch -- --nocapture

use remote_bridge_hub_lib::bridges::xiaomi::config::vk_code_to_name;
use remote_bridge_hub_lib::bridges::xiaomi::key_log::is_firmware_watch_vk;

#[test]
fn watches_f5_and_dpad() {
    assert!(is_firmware_watch_vk(0x74), "F5");
    assert!(is_firmware_watch_vk(0x26), "Up");
    assert!(is_firmware_watch_vk(0x0D), "Enter");
    assert!(!is_firmware_watch_vk(0x41), "letter A is not firmware leak watch");
}

#[test]
fn watches_vk_noname_0xfc() {
    assert!(is_firmware_watch_vk(0xFC));
    assert!(vk_code_to_name(0xFC).contains("0xFC") || vk_code_to_name(0xFC).contains("NONAME"));
}
