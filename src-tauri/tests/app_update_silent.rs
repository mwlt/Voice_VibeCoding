//! 静默升级参数与脚本编排（不启动真实安装器）

use remote_bridge_hub_lib::app_update::{
    build_silent_upgrade_ps1, silent_install_args, silent_uninstall_keep_data_args,
    silent_upgrade_powershell_args,
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
fn upgrade_ps1_waits_then_uninstalls_then_installs_hidden() {
    let script = build_silent_upgrade_ps1(
        4242,
        Some(Path::new(r"C:\Program Files\Voice VibeCoding\uninstall.exe")),
        Path::new(r"C:\Users\me\updates\VoiceVibeCoding_1.6.3_x64-setup.exe"),
    );
    assert!(script.contains("4242"), "must wait for old pid: {script}");
    assert!(
        script.contains("Wait-Process"),
        "must Wait-Process (no cmd timeout): {script}"
    );
    assert!(
        !script.to_ascii_lowercase().contains("timeout"),
        "timeout breaks under CREATE_NO_WINDOW: {script}"
    );
    assert!(
        !script.to_ascii_lowercase().contains("start /wait"),
        "start /wait flashes console: {script}"
    );
    assert!(
        script.contains("/S") && script.contains("/UPDATE"),
        "uninstall keep-data: {script}"
    );
    assert!(
        script.contains("/S") && script.contains("/R"),
        "silent install+run: {script}"
    );
    assert!(
        script.contains("WindowStyle") && script.contains("Hidden"),
        "child processes must be Hidden: {script}"
    );
    let un_pos = script.find("/UPDATE").expect("uninstall");
    let in_pos = script.find("/R").expect("install");
    assert!(un_pos < in_pos, "uninstall before install");
}

#[test]
fn upgrade_ps1_shows_progress_window_not_messagebox() {
    let script = build_silent_upgrade_ps1(1, None, Path::new(r"D:\setup.exe"));
    assert!(
        script.contains("正在升级") || script.contains("ProgressBar"),
        "progress UI required: {script}"
    );
    assert!(
        !script.contains("MessageBox") && !script.contains("MsgBox"),
        "no blocking dialogs: {script}"
    );
}

#[test]
fn upgrade_ps1_skips_uninstall_when_absent() {
    let script = build_silent_upgrade_ps1(7, None, Path::new(r"D:\setup.exe"));
    assert!(!script.to_ascii_lowercase().contains("uninstall.exe"));
    assert!(script.contains(r"D:\setup.exe") || script.contains("D:\\setup.exe"));
}

#[test]
fn powershell_launch_args_are_hidden_bypass_file() {
    let args = silent_upgrade_powershell_args(Path::new(
        r"C:\Users\me\AppData\Local\Temp\voice_upgrade.ps1",
    ));
    let joined = args.join(" ");
    assert!(joined.contains("-NoProfile"), "{joined}");
    assert!(joined.contains("-WindowStyle"), "{joined}");
    assert!(joined.contains("Hidden"), "{joined}");
    assert!(joined.contains("-ExecutionPolicy"), "{joined}");
    assert!(joined.contains("Bypass"), "{joined}");
    assert!(joined.contains("-File"), "{joined}");
    assert!(
        joined.contains("voice_upgrade.ps1"),
        "{joined}"
    );
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
