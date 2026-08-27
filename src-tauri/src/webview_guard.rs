//! WebView2 渲染健康守卫：前端心跳 + 后端判定，检测到渲染进程死亡后自动 reload / recreate。
//!
//! 背景：应用长时间运行（或窗口长期隐藏）后，WebView2 渲染进程可能被系统回收/崩溃，
//! 此时窗口仍存在但内容白屏/黑屏，所有 WebView2 API 调用返回 ERROR_INVALID_STATE(0x8007139F)。
//! reload 无法复活已死的渲染进程；连续 reload 失败后应销毁并重建 WebviewWindow。
//!
//! # 示例（doctest）
//!
//! ```
//! use std::time::{Duration, Instant};
//! use remote_bridge_hub_lib::webview_guard::{
//!     WebviewHealth, HealthAction, STALE_AFTER, FAIL_THRESHOLD, RELOAD_FAIL_THRESHOLD,
//! };
//!
//! // 心跳正常：不触发干预
//! let mut h = WebviewHealth::new();
//! h.on_pong();
//! assert_eq!(h.check(Instant::now(), true), HealthAction::None);
//!
//! // 心跳停止：连续 FAIL_THRESHOLD 次检查后触发 reload
//! let start = Instant::now();
//! let t = |s: u64| start + Duration::from_secs(s);
//! for i in 1..FAIL_THRESHOLD {
//!     assert_eq!(h.check(t(STALE_AFTER.as_secs() + i as u64), true), HealthAction::None);
//! }
//! assert_eq!(
//!     h.check(t(STALE_AFTER.as_secs() + FAIL_THRESHOLD as u64), true),
//!     HealthAction::Reload
//! );
//!
//! // reload 连续失败后升级 recreate（需先满足 stale 判定）
//! h.note_reload_failed();
//! h.note_reload_failed();
//! let base = t(STALE_AFTER.as_secs() + 1);
//! for i in 0..(FAIL_THRESHOLD - 1) {
//!     assert_eq!(h.check(base + Duration::from_secs(i as u64), true), HealthAction::None);
//! }
//! assert_eq!(
//!     h.check(base + Duration::from_secs(FAIL_THRESHOLD as u64), true),
//!     HealthAction::Recreate
//! );
//! ```

use std::sync::LazyLock;
use std::time::{Duration, Instant};

/// 心跳过期阈值：超过该时长未收到前端心跳视为疑似死亡
pub const STALE_AFTER: Duration = Duration::from_secs(15);
/// 连续判定失败多少次后触发 reload / recreate
pub const FAIL_THRESHOLD: u32 = 3;
/// 两次 reload 之间的最小间隔（冷却期，防止循环重载）
pub const RELOAD_COOLDOWN: Duration = Duration::from_secs(30);
/// 两次 recreate 之间的最小间隔
pub const RECREATE_COOLDOWN: Duration = Duration::from_secs(60);
/// 启动后从未收到 pong 的宽限期（窗口可见时）
pub const FIRST_PONG_GRACE: Duration = Duration::from_secs(45);
/// reload 连续失败多少次后升级 recreate
pub const RELOAD_FAIL_THRESHOLD: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthAction {
    /// 无需干预
    None,
    /// WebView2 疑似死亡，应重载页面
    Reload,
    /// reload 无效，应销毁并重建 WebviewWindow
    Recreate,
}

/// 健康状态机（纯逻辑，可注入时钟测试）
pub struct WebviewHealth {
    started_at: Instant,
    last_ok: Option<Instant>,
    ever_pong: bool,
    fail_streak: u32,
    reload_fail_streak: u32,
    last_reload: Option<Instant>,
    last_recreate: Option<Instant>,
}

impl WebviewHealth {
    pub fn new() -> Self {
        let now = Instant::now();
        WebviewHealth {
            started_at: now,
            last_ok: Some(now),
            ever_pong: false,
            fail_streak: 0,
            reload_fail_streak: 0,
            last_reload: None,
            last_recreate: None,
        }
    }

    /// 前端心跳成功（页面 JS 仍在运行）
    pub fn on_pong(&mut self) {
        let now = Instant::now();
        self.last_ok = Some(now);
        self.ever_pong = true;
        self.fail_streak = 0;
        self.reload_fail_streak = 0;
    }

    /// reload 调用失败（ERROR_INVALID_STATE 等）
    pub fn note_reload_failed(&mut self) {
        self.reload_fail_streak = self.reload_fail_streak.saturating_add(1);
        self.fail_streak = 0;
    }

    /// recreate 完成后重置宽限期
    pub fn on_recreated(&mut self) {
        let now = Instant::now();
        self.started_at = now;
        self.last_ok = Some(now);
        self.ever_pong = false;
        self.fail_streak = 0;
        self.reload_fail_streak = 0;
        self.last_recreate = Some(now);
    }

    fn is_alive(&self, now: Instant, window_visible: bool) -> bool {
        if self
            .last_ok
            .map(|t| now.duration_since(t) < STALE_AFTER)
            .unwrap_or(false)
        {
            return true;
        }
        // 启动宽限期：窗口可见但 JS 尚未就绪，不判死
        if !self.ever_pong
            && window_visible
            && now.duration_since(self.started_at) < FIRST_PONG_GRACE
        {
            return true;
        }
        false
    }

    fn next_intervention(&self, now: Instant) -> Option<HealthAction> {
        if self.reload_fail_streak >= RELOAD_FAIL_THRESHOLD {
            let cooled = self
                .last_recreate
                .map(|t| now.duration_since(t) >= RECREATE_COOLDOWN)
                .unwrap_or(true);
            return cooled.then_some(HealthAction::Recreate);
        }
        let cooled = self
            .last_reload
            .map(|t| now.duration_since(t) >= RELOAD_COOLDOWN)
            .unwrap_or(true);
        cooled.then_some(HealthAction::Reload)
    }

    /// 定时检查。`window_visible` 供宽限期判定。
    pub fn check(&mut self, now: Instant, window_visible: bool) -> HealthAction {
        if self.is_alive(now, window_visible) {
            self.fail_streak = 0;
            return HealthAction::None;
        }
        self.fail_streak += 1;
        if self.fail_streak < FAIL_THRESHOLD {
            return HealthAction::None;
        }
        match self.next_intervention(now) {
            Some(HealthAction::Recreate) => {
                self.last_recreate = Some(now);
                self.fail_streak = 0;
                HealthAction::Recreate
            }
            Some(HealthAction::Reload) => {
                self.last_reload = Some(now);
                self.fail_streak = 0;
                HealthAction::Reload
            }
            _ => HealthAction::None,
        }
    }
}

static HEALTH: LazyLock<parking_lot::Mutex<WebviewHealth>> =
    LazyLock::new(|| parking_lot::Mutex::new(WebviewHealth::new()));

/// 前端心跳（IPC `webview_ping` 调用）
pub fn ping() {
    HEALTH.lock().on_pong();
}

/// 守卫线程定时检查
pub fn check(now: Instant, window_visible: bool) -> HealthAction {
    HEALTH.lock().check(now, window_visible)
}

/// reload 失败后由恢复层回调
pub fn note_reload_failed() {
    HEALTH.lock().note_reload_failed();
}

/// recreate 成功后重置状态
pub fn on_recreated() {
    HEALTH.lock().on_recreated();
}

/// reload 连续失败是否应升级 recreate
pub fn needs_recreate() -> bool {
    HEALTH.lock().reload_fail_streak >= RELOAD_FAIL_THRESHOLD
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(secs: u64) -> Instant {
        Instant::now() + Duration::from_secs(secs)
    }

    #[test]
    fn healthy_heartbeat_no_action() {
        let mut h = WebviewHealth::new();
        h.on_pong();
        assert_eq!(h.check(Instant::now(), true), HealthAction::None);
        assert_eq!(
            h.check(Instant::now() + Duration::from_secs(5), true),
            HealthAction::None
        );
    }

    #[test]
    fn no_heartbeat_triggers_reload_after_threshold() {
        let mut h = WebviewHealth::new();
        let start = t(0);
        h.on_pong();
        assert_eq!(
            h.check(start + STALE_AFTER + Duration::from_secs(1), true),
            HealthAction::None
        );
        assert_eq!(
            h.check(start + STALE_AFTER + Duration::from_secs(2), true),
            HealthAction::None
        );
        assert_eq!(
            h.check(start + STALE_AFTER + Duration::from_secs(3), true),
            HealthAction::Reload
        );
    }

    #[test]
    fn reload_has_cooldown() {
        let mut h = WebviewHealth::new();
        let start = t(0);
        h.on_pong();
        let mut now = start + STALE_AFTER + Duration::from_secs(3);
        assert_eq!(h.check(now, true), HealthAction::Reload);
        now += Duration::from_secs(10);
        assert_eq!(h.check(now, true), HealthAction::None);
        now += RELOAD_COOLDOWN + Duration::from_secs(1);
        assert_eq!(h.check(now, true), HealthAction::Reload);
    }

    #[test]
    fn reload_failures_upgrade_to_recreate() {
        let mut h = WebviewHealth::new();
        h.on_pong();
        h.note_reload_failed();
        h.note_reload_failed();
        let now = t(STALE_AFTER.as_secs() + FAIL_THRESHOLD as u64 + 1);
        assert_eq!(h.check(now, true), HealthAction::Recreate);
    }

    #[test]
    fn never_pong_within_grace_no_action() {
        let mut h = WebviewHealth::new();
        let start = h.started_at;
        assert_eq!(
            h.check(start + Duration::from_secs(30), true),
            HealthAction::None
        );
    }

    #[test]
    fn never_pong_after_grace_triggers_reload() {
        let mut h = WebviewHealth::new();
        let start = h.started_at;
        let mut now = start + FIRST_PONG_GRACE + Duration::from_secs(1);
        for _ in 0..(FAIL_THRESHOLD - 1) {
            assert_eq!(h.check(now, true), HealthAction::None);
            now += Duration::from_secs(1);
        }
        assert_eq!(h.check(now, true), HealthAction::Reload);
    }

    #[test]
    fn heartbeat_recovers_after_failures() {
        let mut h = WebviewHealth::new();
        let start = t(0);
        h.on_pong();
        let now = start + STALE_AFTER + Duration::from_secs(1);
        assert_eq!(h.check(now, true), HealthAction::None);
        h.on_pong();
        assert_eq!(h.check(now + Duration::from_secs(2), true), HealthAction::None);
    }
}
