//! Step 4 — 嵌套仍吞 F5（docs/VOICE_F5_LONGTERM_PLAN.md）
//!
//! 重叠 bump 时新旧钩子同 proc，`HOOK_PROC_DEPTH > 0`。
//! 旧实现一刀切 CallNextHookEx → F5 抑制真空。
//! 现契约：注入键（EXTRA_INFO）放行；F5 仍走 should_suppress；其余转发。
//!
//! 运行: cargo test --test hook_nesting_guard -- --nocapture

#[test]
fn nesting_branch_still_suppresses_f5() {
    let src = include_str!("../src/bridges/xiaomi/special_keys.rs");
    let proc = src
        .split("unsafe extern \"system\" fn proc")
        .nth(1)
        .and_then(|s| s.split("// ---- END LL HOOK PROC ----").next())
        .expect("proc");
    // 嵌套分支必须在 depth>0 时仍能处理 F5
    assert!(
        proc.contains("if depth > 0"),
        "must have nesting depth branch"
    );
    let nest = proc
        .split("if depth > 0")
        .nth(1)
        .and_then(|s| s.split("// ---- 非嵌套主路径 ----").next())
        .expect("nesting body");
    assert!(
        nest.contains("0x74") && nest.contains("should_suppress_voice_f5"),
        "nesting branch must still suppress F5 via should_suppress_voice_f5"
    );
    assert!(
        !nest.trim_start().starts_with("{\n            return CallNextHookEx"),
        "nesting must not blindly forward all keys"
    );
}

#[test]
fn nesting_branch_passes_injected_keys() {
    let src = include_str!("../src/bridges/xiaomi/special_keys.rs");
    let nest = nesting_body(src);
    assert!(
        nest.contains("our_inject") && nest.contains("CallNextHookEx"),
        "injected EXTRA_INFO keys must CallNextHookEx in nesting branch"
    );
}

#[test]
fn nesting_branch_forwards_other_keys_untouched() {
    let nest = nesting_body(include_str!("../src/bridges/xiaomi/special_keys.rs"));
    // 非注入、非 F5：最终 CallNextHookEx
    assert!(
        nest.matches("CallNextHookEx").count() >= 2,
        "nesting should forward non-F5 non-inject keys"
    );
}

#[test]
fn capture_swallow_precedes_nesting_guard() {
    let src = include_str!("../src/bridges/xiaomi/special_keys.rs");
    let proc = src
        .split("unsafe extern \"system\" fn proc")
        .nth(1)
        .and_then(|s| s.split("// ---- END LL HOOK PROC ----").next())
        .expect("proc");
    let capture = proc
        .find("try_swallow_capture_key")
        .expect("capture swallow");
    let depth = proc.find("if depth > 0").expect("depth guard");
    assert!(
        capture < depth,
        "capture swallow must run before nesting early-out"
    );
}

#[test]
fn depth_counter_still_guarded() {
    let src = include_str!("../src/bridges/xiaomi/special_keys.rs");
    assert!(src.contains("HOOK_PROC_DEPTH"));
    assert!(
        src.contains("HookProcDepthGuard") || src.contains("fetch_sub"),
        "depth must be decremented on proc exit"
    );
}

fn nesting_body(src: &str) -> &str {
    src.split("unsafe extern \"system\" fn proc")
        .nth(1)
        .and_then(|s| s.split("// ---- END LL HOOK PROC ----").next())
        .and_then(|proc| {
            proc.split("if depth > 0")
                .nth(1)
                .and_then(|s| s.split("// ---- 非嵌套主路径 ----").next())
        })
        .expect("nesting body")
}
