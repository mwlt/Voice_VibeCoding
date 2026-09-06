//! T1 BLE-only L0 HID filter seams（不碰 USB / 小米 / WinUHid）

use remote_bridge_hub_lib::bridges::t1::t1_hid_filter_env::{
    hardware_id_is_t1_ble_only, T1_BLE_L0_ALLOW_TOKENS, T1_BLE_L0_DENY_TOKENS,
};

#[test]
fn allowlist_matches_ble_keys_tokens() {
    assert!(T1_BLE_L0_ALLOW_TOKENS
        .iter()
        .any(|t| *t == "dev_vid&01620a_pid&0407"));
    assert!(T1_BLE_L0_DENY_TOKENS.iter().any(|t| *t == "vid_1915"));
    assert!(T1_BLE_L0_DENY_TOKENS.iter().any(|t| *t == "vid_2717"));
}

#[test]
fn ble_hwid_allowed_usb_xiaomi_denied() {
    assert!(hardware_id_is_t1_ble_only(
        r"\\?\HID#VID_1620A&PID_0407#dev_vid&01620a_pid&0407"
    ));
    assert!(!hardware_id_is_t1_ble_only(r"\\?\HID#VID_1915&PID_1025&MI_01"));
    assert!(!hardware_id_is_t1_ble_only(r"HID\VID_2717&PID_32B8"));
    assert!(!hardware_id_is_t1_ble_only(r"Root\WinUHid"));
}

#[test]
fn package_and_script_present() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/t1_hid_filter");
    assert!(root.join("install-t1-hid-filter.ps1").is_file());
    assert!(root.join("driver/t1blehidf.inf").is_file());
    assert!(root.join("driver/src/t1blehidf.c").is_file());
}
