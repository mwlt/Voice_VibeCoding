// Windows GUI：始终无控制台窗口（含子进程 CLI 入口）。调试日志写文件；需要控制台时设 RUST_LOG 并自行 AllocConsole。
#![windows_subsystem = "windows"]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("xiaomi-hid-injector") => {
            let code =
                remote_bridge_hub_lib::bridges::xiaomi::hid_tap_injector::run_injector_cli(&args);
            std::process::exit(code);
        }
        Some("xiaomi-audio-router") => {
            let code = remote_bridge_hub_lib::audio::pcm_router::run_audio_router_cli(&args);
            std::process::exit(code);
        }
        _ => remote_bridge_hub_lib::run(),
    }
}
