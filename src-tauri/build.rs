use std::env;
use std::path::PathBuf;

fn main() {
    tauri_build::build();
    ensure_shell_hook_dll_notice();
}

/// WH_SHELL 硬拦依赖旁路 `t1_shell_hook.dll`。不在 build.rs 里再调 cargo（会死锁），
/// 只提示/确认产物是否已在 target/{profile}/。
fn ensure_shell_hook_dll_notice() {
    println!("cargo:rerun-if-changed=t1_shell_hook/src/lib.rs");
    println!("cargo:rerun-if-changed=t1_shell_hook/Cargo.toml");

    if env::var("CARGO_CFG_TARGET_OS").ok().as_deref() != Some("windows") {
        return;
    }

    let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".into());
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let mut candidates = vec![manifest.join("target").join(&profile).join("t1_shell_hook.dll")];
    if let Ok(td) = env::var("CARGO_TARGET_DIR") {
        candidates.push(PathBuf::from(td).join(&profile).join("t1_shell_hook.dll"));
    }
    if candidates.iter().any(|p| p.is_file()) {
        if let Some(p) = candidates.into_iter().find(|p| p.is_file()) {
            println!("cargo:warning=t1_shell_hook.dll ready at {}", p.display());
        }
    } else {
        println!(
            "cargo:warning=t1_shell_hook.dll missing — run: cargo build -p t1_shell_hook (WH_SHELL hard-block needs it)"
        );
    }
}
