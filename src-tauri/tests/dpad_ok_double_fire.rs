//! 方向/OK：自定义映射用 Home 同款 tap_ready 吞固件 VK；身份映射不误伤真实键盘。
//!
//!   cargo test --manifest-path src-tauri/Cargo.toml --test dpad_ok_double_fire -- --nocapture

use remote_bridge_hub_lib::bridges::xiaomi::key_mapping::{
    set_dpad_ok_custom_suppress_vks, should_gate_block_dpad_ok_mapping,
};
use remote_bridge_hub_lib::bridges::xiaomi::special_keys::should_suppress_native_dpad_ok;

#[test]
fn up_mapped_to_m_suppresses_firmware_up_when_tap_ready() {
    // 与 Home→Space 相同：Tap 就绪即吞固件原生，消除空闲单点「先 M 后上」
    set_dpad_ok_custom_suppress_vks(&[0x26]);
    assert!(should_suppress_native_dpad_ok(0x26, true, false));
    // 身份左不在表内 → 真实键盘左仍可用
    assert!(!should_suppress_native_dpad_ok(0x25, true, false));
    set_dpad_ok_custom_suppress_vks(&[]);
}

#[test]
fn identity_ok_not_suppressed_on_tap_ready_alone() {
    set_dpad_ok_custom_suppress_vks(&[]);
    assert!(!should_suppress_native_dpad_ok(0x0D, true, false));
    assert!(!should_suppress_native_dpad_ok(0x25, true, false));
}

#[test]
fn recent_still_suppresses() {
    set_dpad_ok_custom_suppress_vks(&[]);
    assert!(should_suppress_native_dpad_ok(0x26, false, true));
}

#[test]
fn dpad_ok_mapping_must_not_be_gate_blocked() {
    assert!(!should_gate_block_dpad_ok_mapping("up"));
    assert!(should_gate_block_dpad_ok_mapping("volume_up"));
}
