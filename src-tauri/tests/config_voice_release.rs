//! Integration tests for DeviceConfig voice_release_behavior serde.
//! Prefer this harness when `cargo test --lib` fails to start (STATUS_ENTRYPOINT_NOT_FOUND).

use remote_bridge_hub_lib::config::manager::{DeviceConfig, VoiceReleaseBehavior};

#[test]
fn voice_release_behavior_defaults_to_none_when_missing_from_json() {
    let user_json = r#"{
        "button_aliases": {},
        "button_bindings": {},
        "voice_hotkey": ["rightalt"],
        "trigger_mode": "Hold",
        "bluetooth_address": null
    }"#;
    let config: DeviceConfig = serde_json::from_str(user_json).unwrap();
    assert_eq!(config.voice_release_behavior, VoiceReleaseBehavior::None);
}

#[test]
fn voice_release_behavior_roundtrips_tap_same_chord() {
    let mut config = DeviceConfig::new();
    config.voice_release_behavior = VoiceReleaseBehavior::TapSameChord;
    let json = serde_json::to_string(&config).unwrap();
    assert!(json.contains("TapSameChord"));
    let decoded: DeviceConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(
        decoded.voice_release_behavior,
        VoiceReleaseBehavior::TapSameChord
    );
}
