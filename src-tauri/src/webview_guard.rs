//! WebView2 渲染健康守卫：前端心跳 + 后端判定，检测到渲染进程死亡后自动 reload。
//!
//! 背景：应用长时间运行（或窗口长期隐藏）后，WebView2 渲染进程可能被系统回收/崩溃，
//! 此时窗口仍存在但内容白屏，所有 WebView2 API 调用返回 ERROR_INVALID_STATE(0x8007139F)。
//! 本模块通过"前端定期心跳 + 后端超时判定"检测死亡，并触发 `window.reload()` 自愈。
//!
//! # 示例（doctest）
//!
//! ```
//! use std::time::{Duration, Instant};
//! use remote_bridge_hub_lib::webview_guard::{WebviewHealth, HealthAction, STALE_AFTER, FAIL_THRESHOLD};
//!
//! // 心跳正常：不触发 reload
//! let mut h = WebviewHealth::new();
//! h.on_pong();
//! assert_eq!(h.check(Instant::now()), HealthAction::None);
//!
//! // 心跳停止：连续 FAIL_THRESHOLD 次检查后触发 reload
//! let start = Instant::now();
//! let t = |s: u64| start + Duration::from_secs(s);
//! for i in 1..FAIL_THRESHOLD {
//!     assert_eq!(h.check(t(STALE_AFTER.as_secs() + i as u64)), HealthAction::None);
//! }
//! assert_eq!(
//!     h.check(t(STALE_AFTER.as_secs() + FAIL_THRESHOLD as u64)),
//!     HealthAction::Reload
//! );
//!
//! // reload 冷却期内不会重复触发
//! assert_eq!(h.check(t((STALE_AFTER.as_secs() + 20) as u64)), HealthAction::None);
//!
//! // 心跳恢复后重置计数
//! h.on_pong();
//! assert_eq!(h.check(Instant::now() + Duration::from_secs(5)), HealthAction::None);
//! ```

use std::time::{Duration, Instant};

/// 心跳过期阈值：超过该时长未收到前端心跳视为疑似死亡
pub const STALE_AFTER: Duration = Duration::from_secs(15);
/// 连续判定失败多少次后触发 reload
pub const FAIL_THRESHOLD: u32 = 3;
/// 两次 reload 之间的最小间隔（冷却期，防止循环重载）
pub const RELOAD_COOLDOWN: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthAction {
    /// 无需干预
    None,
    /// WebView2 疑似死亡，应重载页面
    Reload,
}

/// 健康状态机（纯逻辑，可用注入时钟测试）
pub struct WebviewHealth {
    last_ok: Option<Instant>,
    fail_streak: u32,
    last_reload: Option<Instant>,
}

impl WebviewHealth {
    pub const fn new() -> Self {
        WebviewHealth {
            last_ok: None,
            fail_streak: 0,
            last_reload: None,
        }
    }

    /// 前端心跳成功（页面 JS 仍在运行）
    pub fn on_pong(&mut self) {
        self.last_ok = Some(Instant::now());
        self.fail_streak = 0;
    }

    /// 定时检查。`now` 为当前时间（可注入以便测试）。
    /// 返回 `Reload` 表示应重载页面（带冷却期与连续失败阈值）。
    pub fn check(&mut self, now: Instant) -> HealthAction {
        let alive = self
            .last_ok
            .map(|t| now.duration_since(t) < STALE_AFTER)
            .unwrap_or(false);
        if alive {
            self.fail_streak = 0;
            return HealthAction::None;
        }
        self.fail_streak += 1;
        if self.fail_streak >= FAIL_THRESHOLD {
            let cooled = self
                .last_reload
                .map(|t| now.duration_since(t) >= RELOAD_COOLDOWN)
                .unwrap_or(true);
            if cooled {
                self.last_reload = Some(now);
                self.fail_streak = 0;
                return HealthAction::Reload;
            }
        }
        HealthAction::None
    }
}

/// 全局健康状态（进程级单例，供 IPC/守卫线程使用）
static HEALTH: parking_lot::Mutex<WebviewHealth> = parking_lot::Mutex::new(WebviewHealth::new());

/// 前端心跳（IPC `webview_ping` 调用）
pub fn ping() {
    HEALTH.lock().on_pong();
}

/// 守卫线程定时检查；返回 true 表示应 reload 主窗口
pub fn check_and_reload(now: Instant) -> bool {
    HEALTH.lock().check(now) == HealthAction::Reload
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
        // 心跳刚发生：任何检查都不该触发 reload
        assert_eq!(h.check(Instant::now()), HealthAction::None);
        assert_eq!(h.check(Instant::now() + Duration::from_secs(5)), HealthAction::None);
    }

    #[test]
    fn no_heartbeat_triggers_reload_after_threshold() {
        let mut h = WebviewHealth::new();
        let start = t(0);
        h.on_pong();
        // 超过 STALE_AFTER 后连续 FAIL_THRESHOLD 次检查才触发
        assert_eq!(h.check(start + STALE_AFTER + Duration::from_secs(1)), HealthAction::None);
        assert_eq!(h.check(start + STALE_AFTER + Duration::from_secs(2)), HealthAction::None);
        assert_eq!(h.check(start + STALE_AFTER + Duration::from_secs(3)), HealthAction::Reload);
    }

    #[test]
    fn reload_has_cooldown() {
        let mut h = WebviewHealth::new();
        let start = t(0);
        h.on_pong();
        // 触发第一次 reload
        let mut now = start + STALE_AFTER + Duration::from_secs(3);
        assert_eq!(h.check(now), HealthAction::Reload);
        // 冷却期内：即使再失败也不会重复 reload
        now += Duration::from_secs(10);
        assert_eq!(h.check(now), HealthAction::None);
        assert_eq!(h.check(now + Duration::from_secs(1)), HealthAction::None);
        // 冷却期结束后可再次 reload
        now += RELOAD_COOLDOWN + Duration::from_secs(1);
        assert_eq!(h.check(now), HealthAction::Reload);
    }

    #[test]
    fn heartbeat_recovers_after_failures() {
        let mut h = WebviewHealth::new();
        let start = t(0);
        h.on_pong();
        // 2 次失败（未达阈值）
        let mut now = start + STALE_AFTER + Duration::from_secs(1);
        assert_eq!(h.check(now), HealthAction::None);
        assert_eq!(h.check(now + Duration::from_secs(1)), HealthAction::None);
        // 心跳恢复
        h.on_pong();
        assert_eq!(h.check(now + Duration::from_secs(2)), HealthAction::None);
        // 计数器已重置：再超时需重新累计
        now += STALE_AFTER + Duration::from_secs(3);
        assert_eq!(h.check(now), HealthAction::None);
        assert_eq!(h.check(now + Duration::from_secs(1)), HealthAction::None);
        assert_eq!(h.check(now + Duration::from_secs(2)), HealthAction::Reload);
    }
}
