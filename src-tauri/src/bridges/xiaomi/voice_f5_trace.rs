//! F5 全链路追踪 — 统一前缀 `XIAOMI F5 TRACE`，便于 grep 排漏、看各层生效顺序与冲突。
//!
//! 层（layer）约定：
//! - `gadget_tap` — Frida/HID Tap 清 0x003E（最上游）
//! - `ll_hook` / `ll_hook_bump` — WH_KEYBOARD_LL 吞/放
//! - `correlate` — 关联窗 / wait mic / sticky / post-tail 状态机
//! - `dispatch` — 语音 worker 队列
//! - `key_output` — 最终落到系统的 mapped / suppressed / extra
//! - `firmware_fallback` — ATVV 不可用时的 F5→handle_voice 回退
//! - `state` — period / session / disarm 等生命周期

use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Default)]
pub struct Guards {
    pub session: bool,
    pub period: bool,
    pub armed: bool,
    pub correlate: bool,
    pub tail: bool,
    pub sticky: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct HookCtx {
    pub tap_ready: bool,
    pub injected: bool,
    pub our_inject: bool,
    pub hook_depth: u32,
}

fn next_seq() -> u64 {
    SEQ.fetch_add(1, Ordering::Relaxed) + 1
}

fn guards_str(g: Guards) -> String {
    format!(
        "sess={} period={} armed={} corr={} tail={} sticky={}",
        u8::from(g.session),
        u8::from(g.period),
        u8::from(g.armed),
        u8::from(g.correlate),
        u8::from(g.tail),
        u8::from(g.sticky),
    )
}

fn hook_str(h: Option<HookCtx>) -> String {
    match h {
        Some(x) => format!(
            " tap={} inj={} our={} depth={}",
            u8::from(x.tap_ready),
            u8::from(x.injected),
            u8::from(x.our_inject),
            x.hook_depth,
        ),
        None => String::new(),
    }
}

fn warn_conflict(seq: u64, layer: &str, phase: &str, action: &str, g: Guards, detail: &str) {
    let leak = matches!(action, "passthrough" | "leak_extra" | "decide_passthrough");
    if !leak {
        return;
    }
    if g.period || g.tail || g.correlate || g.sticky || g.armed {
        log::warn!(
            "XIAOMI F5 TRACE CONFLICT seq={seq} layer={layer} phase={phase} action={action} \
             leak_while_guard_active {} detail={detail}",
            guards_str(g),
        );
    }
    if layer == "gadget_tap" && action == "cleared" && g.sticky {
        log::warn!(
            "XIAOMI F5 TRACE CONFLICT seq={seq} gadget_cleared_but_sticky_set detail={detail}",
        );
    }
}

/// 主追踪行（INFO，不被 logging 噪音表过滤）
pub fn event(
    layer: &'static str,
    phase: &'static str,
    action: &'static str,
    detail: &str,
    guards: Guards,
    hook: Option<HookCtx>,
) {
    let seq = next_seq();
    log::info!(
        "XIAOMI F5 TRACE seq={seq} layer={layer} phase={phase} action={action} \
         {}{} detail={detail}",
        guards_str(guards),
        hook_str(hook),
    );
    warn_conflict(seq, layer, phase, action, guards, detail);
}

/// 生命周期 / 状态变更（无 hook 上下文）
pub fn state(action: &'static str, detail: &str, guards: Guards) {
    event("state", "-", action, detail, guards, None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guards_format_includes_all_flags() {
        let s = guards_str(Guards {
            session: true,
            period: true,
            armed: false,
            correlate: true,
            tail: false,
            sticky: true,
        });
        assert!(s.contains("sess=1"));
        assert!(s.contains("corr=1"));
        assert!(s.contains("sticky=1"));
    }
}
