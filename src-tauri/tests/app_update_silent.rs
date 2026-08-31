//! 静默升级参数与批处理编排（不启动真实安装器）

use remote_bridge_hub_lib::app_update::{
    build_silent_upgrade_batch, silent_install_args, silent_uninstall_keep_data_args,
};
use std::path::Path;

#[test]
fn silent_install_args_are_s_and_r() {
    assert_eq!(silent_install_args(), ["/S", "/R"]);
}

#[test]
fn silent_uninstall_keep_data_args_are_s_and_update() {
    assert_eq!(silent_uninstall_keep_data_args(), ["/S", "/UPDATE"]);
}

#[test]
fn upgrade_batch_waits_then_uninstalls_then_installs() {
    let batch = build_silent_upgrade_batch(
        4242,
        Some(Path::new(r"C:\Program Files\Voice VibeCoding\uninstall.exe")),
        Path::new(r"C:\Users\me\updates\VoiceVibeCoding_1.6.2_x64-setup.exe"),
    );
    assert!(batch.contains("WAIT_PID=4242"));
    assert!(batch.contains(r#""C:\Program Files\Voice VibeCoding\uninstall.exe" /S /UPDATE"#));
    assert!(batch.contains(r#""C:\Users\me\updates\VoiceVibeCoding_1.6.2_x64-setup.exe" /S /R"#));
    let un_pos = batch.find("/S /UPDATE").expect("uninstall");
    let in_pos = batch.find("/S /R").expect("install");
    assert!(un_pos < in_pos);
}

#[test]
fn upgrade_batch_skips_uninstall_when_absent() {
    let batch = build_silent_upgrade_batch(7, None, Path::new(r"D:\setup.exe"));
    assert!(!batch.to_ascii_lowercase().contains("uninstall.exe"));
    assert!(batch.contains(r#""D:\setup.exe" /S /R"#));
}

#[test]
fn parse_uninstall_string_quoted_and_plain() {
    use remote_bridge_hub_lib::app_update::parse_uninstall_exe_path;
    use std::path::PathBuf;
    assert_eq!(
        parse_uninstall_exe_path(r#""C:\Program Files\Voice VibeCoding\uninstall.exe""#),
        Some(PathBuf::from(r"C:\Program Files\Voice VibeCoding\uninstall.exe"))
    );
    assert_eq!(
        parse_uninstall_exe_path(r"C:\App\uninstall.exe /S"),
        Some(PathBuf::from(r"C:\App\uninstall.exe"))
    );
    assert_eq!(parse_uninstall_exe_path(""), None);
}
