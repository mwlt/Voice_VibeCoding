//! 启动环境自动修复流水线：决策纯函数 + 串行编排
//!
//! 顺序：虚拟声卡 → 虚拟键盘 → 等语音路由 → 等桥接落定 → ATVV（条件性一次）

use std::sync::atomic::{AtomicBool, Ordering};

/// 流水线步骤（固定顺序的公共契约）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineStep {
    Cable,
    WinUHid,
    WaitAudio,
    WaitBridge,
    Atvv,
}

/// 固定启动修复顺序（桥接本身不单独「修复」，只等待落定）
pub fn pipeline_steps() -> &'static [PipelineStep] {
    &[
        PipelineStep::Cable,
        PipelineStep::WinUHid,
        PipelineStep::WaitAudio,
        PipelineStep::WaitBridge,
        PipelineStep::Atvv,
    ]
}

/// 是否应对 VB-CABLE 跑一次启动自动修复。
/// - 已就绪：否
/// - 本进程已尝试过：否
/// - 已知待重启（装过但未 reboot）：否（避免反复弹 UAC）
pub fn should_auto_repair_cable(ready: bool, attempted: bool, reboot_pending: bool) -> bool {
    !ready && !attempted && !reboot_pending
}

/// 是否应对 ATVV 跑一次自动修复（= 重启桥接再等订阅）。
/// - 桥接未起来：否（先等自动连接）
/// - 已有 ATVV：否
/// - 本进程已尝试过：否
/// - 尚未等到落定窗口：否
pub fn should_auto_repair_atvv(
    bridge_alive: bool,
    atvv_ok: bool,
    attempted: bool,
    settle_ok: bool,
) -> bool {
    bridge_alive && !atvv_ok && !attempted && settle_ok
}

/// 进程级：声卡自动修是否已尝试
static CABLE_ATTEMPTED: AtomicBool = AtomicBool::new(false);
/// 进程级：ATVV 自动修是否已尝试
static ATVV_ATTEMPTED: AtomicBool = AtomicBool::new(false);
/// 流水线是否已启动（防重复 spawn）
static PIPELINE_STARTED: AtomicBool = AtomicBool::new(false);

pub fn cable_auto_repair_attempted() -> bool {
    CABLE_ATTEMPTED.load(Ordering::SeqCst)
}

pub fn mark_cable_auto_repair_attempted() {
    CABLE_ATTEMPTED.store(true, Ordering::SeqCst);
}

pub fn atvv_auto_repair_attempted() -> bool {
    ATVV_ATTEMPTED.load(Ordering::SeqCst)
}

pub fn mark_atvv_auto_repair_attempted() {
    ATVV_ATTEMPTED.store(true, Ordering::SeqCst);
}

/// 测试用重置
#[cfg(test)]
pub fn reset_attempt_flags_for_test() {
    CABLE_ATTEMPTED.store(false, Ordering::SeqCst);
    ATVV_ATTEMPTED.store(false, Ordering::SeqCst);
    PIPELINE_STARTED.store(false, Ordering::SeqCst);
}

pub fn try_mark_pipeline_started() -> bool {
    PIPELINE_STARTED
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
}

/// 单步执行结果汇总
#[derive(Debug, Default, Clone)]
pub struct PipelineReport {
    pub completed: usize,
    pub errors: Vec<String>,
}

type StepFn = Box<dyn FnMut() -> Result<(), String> + Send>;

/// 可注入步骤的串行执行器（同一时刻只跑一步）
pub struct PipelineRunner {
    steps: Vec<(PipelineStep, StepFn)>,
}

impl PipelineRunner {
    pub fn new() -> Self {
        Self { steps: Vec::new() }
    }

    pub fn push<F>(&mut self, step: PipelineStep, f: F)
    where
        F: FnMut() -> Result<(), String> + Send + 'static,
    {
        self.steps.push((step, Box::new(f)));
    }

    pub fn run(mut self) -> PipelineReport {
        let mut report = PipelineReport::default();
        for (step, f) in self.steps.iter_mut() {
            log::info!("startup-env pipeline step={step:?} begin");
            match f() {
                Ok(()) => {
                    report.completed += 1;
                    log::info!("startup-env pipeline step={step:?} ok");
                }
                Err(e) => {
                    report.completed += 1;
                    log::warn!("startup-env pipeline step={step:?} err={e}");
                    report.errors.push(format!("{step:?}: {e}"));
                }
            }
        }
        report
    }
}

impl Default for PipelineRunner {
    fn default() -> Self {
        Self::new()
    }
}

fn cable_reboot_flag_path() -> std::path::PathBuf {
    std::env::temp_dir().join("voice_vibecoding_cable_reboot_pending.flag")
}

pub fn cable_reboot_pending() -> bool {
    cable_reboot_flag_path().exists()
}

pub fn set_cable_reboot_pending(pending: bool) {
    let p = cable_reboot_flag_path();
    if pending {
        let _ = std::fs::write(&p, b"1");
    } else {
        let _ = std::fs::remove_file(&p);
    }
}

fn cable_reboot_flag_age() -> Option<std::time::Duration> {
    let meta = std::fs::metadata(cable_reboot_flag_path()).ok()?;
    let modified = meta.modified().ok()?;
    modified.elapsed().ok()
}

#[cfg(target_os = "windows")]
fn os_uptime() -> Option<std::time::Duration> {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetTickCount64() -> u64;
    }
    Some(std::time::Duration::from_millis(unsafe { GetTickCount64() }))
}

#[cfg(not(target_os = "windows"))]
fn os_uptime() -> Option<std::time::Duration> {
    None
}

/// 重启标记是否仍应阻断自动修。
/// 同一次开机内（flag 年龄 ≤ 系统 uptime）继续阻断，避免未重启就反复弹 UAC；
/// 真正重启后（flag 年龄 > uptime）解除阻断，允许再试一次。
pub fn cable_reboot_blocks_auto_repair(
    flag_present: bool,
    flag_age: Option<std::time::Duration>,
    uptime: Option<std::time::Duration>,
) -> bool {
    if !flag_present {
        return false;
    }
    match (flag_age, uptime) {
        (Some(age), Some(up)) if age > up => false,
        _ => true,
    }
}

/// 启动时对 VB-CABLE 最多自动修复一次（可能弹 UAC）。
pub fn ensure_cable_once() -> Result<(), String> {
    let status = crate::audio::vb_cable::voice_env_status_fresh();
    if status.ready {
        set_cable_reboot_pending(false);
        log::info!("startup-env cable already ready");
        return Ok(());
    }
    let reboot_blocks = cable_reboot_blocks_auto_repair(
        cable_reboot_pending(),
        cable_reboot_flag_age(),
        os_uptime(),
    );
    // 重启后 flag 仍在但已不应阻断：清掉，允许本进程再试一次
    if cable_reboot_pending() && !reboot_blocks {
        set_cable_reboot_pending(false);
        log::info!("startup-env cable reboot flag cleared after OS reboot");
    }
    if !should_auto_repair_cable(
        status.ready,
        cable_auto_repair_attempted(),
        reboot_blocks,
    ) {
        log::info!(
            "startup-env cable skip auto-repair ready={} attempted={} reboot_blocks={}",
            status.ready,
            cable_auto_repair_attempted(),
            reboot_blocks
        );
        return Ok(());
    }
    if !status.embedded_available {
        mark_cable_auto_repair_attempted();
        return Err("VB-CABLE 未就绪且内嵌驱动包不可用".into());
    }
    mark_cable_auto_repair_attempted();
    log::info!("startup-env cable auto-repair begin");
    match crate::audio::vb_cable::install_embedded() {
        Ok(r) => {
            if r.ready {
                set_cable_reboot_pending(false);
                log::info!("startup-env cable auto-repair ready");
                Ok(())
            } else if r.needs_reboot {
                set_cable_reboot_pending(true);
                log::warn!("startup-env cable auto-repair needs reboot: {}", r.message);
                Ok(()) // 不算流水线失败，避免阻断后续
            } else {
                Err(r.message)
            }
        }
        Err(e) => Err(e),
    }
}

fn wait_until(timeout: std::time::Duration, mut pred: impl FnMut() -> bool) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if pred() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(400));
    }
    pred()
}

/// 等待语音路由子进程就绪（已在别处 spawn）。
pub fn wait_audio_router(timeout: std::time::Duration) -> Result<(), String> {
    if wait_until(timeout, || {
        crate::audio::pcm_router::audio_router_ready()
            || crate::audio::pcm_router::audio_router_process_alive()
    }) {
        log::info!("startup-env audio router ready");
        Ok(())
    } else {
        Err("等待语音路由超时".into())
    }
}

/// 等待桥接自动连接落定。返回 (bridge_alive, settle_ok)。
pub fn wait_bridge_settle(
    app: &tauri::AppHandle,
    max_wait: std::time::Duration,
    settle_after_alive: std::time::Duration,
) -> (bool, bool) {
    use tauri::Manager;
    let start = std::time::Instant::now();
    let mut alive_since: Option<std::time::Instant> = None;
    while start.elapsed() < max_wait {
        let alive = app
            .try_state::<std::sync::Arc<crate::bridges::xiaomi::connect::XiaomiRuntime>>()
            .map(|r| r.running.load(Ordering::SeqCst))
            .unwrap_or(false);
        if alive {
            if alive_since.is_none() {
                alive_since = Some(std::time::Instant::now());
            }
            if crate::bridges::xiaomi::connect::atvv_subscribed() {
                return (true, true);
            }
            if alive_since
                .map(|t| t.elapsed() >= settle_after_alive)
                .unwrap_or(false)
            {
                return (true, true);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    let alive = app
        .try_state::<std::sync::Arc<crate::bridges::xiaomi::connect::XiaomiRuntime>>()
        .map(|r| r.running.load(Ordering::SeqCst))
        .unwrap_or(false);
    let settle_ok = alive
        && alive_since
            .map(|t| t.elapsed() >= settle_after_alive)
            .unwrap_or(false);
    (alive, settle_ok || alive) // 超时但仍 alive：允许尝试一次 ATVV 修
}

fn step_atvv_once(app: &tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    let bridge_alive = app
        .try_state::<std::sync::Arc<crate::bridges::xiaomi::connect::XiaomiRuntime>>()
        .map(|r| r.running.load(Ordering::SeqCst))
        .unwrap_or(false);
    let atvv_ok = crate::bridges::xiaomi::connect::atvv_subscribed();
    if !should_auto_repair_atvv(
        bridge_alive,
        atvv_ok,
        atvv_auto_repair_attempted(),
        true, // WaitBridge 已保证落定后再进本步
    ) {
        log::info!(
            "startup-env atvv skip auto-repair bridge_alive={bridge_alive} atvv_ok={atvv_ok} attempted={}",
            atvv_auto_repair_attempted()
        );
        return Ok(());
    }
    mark_atvv_auto_repair_attempted();
    let Some(state) = app.try_state::<crate::bridges::BridgeState>() else {
        return Err("BridgeState 不可用".into());
    };
    let Some(config) = app.try_state::<crate::config::manager::ConfigManager>() else {
        return Err("ConfigManager 不可用".into());
    };
    log::info!("startup-env atvv auto-repair begin");
    let (ok, msg) =
        crate::ipc::commands::run_atvv_repair_pipeline(app, state.inner(), config.inner())?;
    if ok {
        log::info!("startup-env atvv auto-repair ok: {msg}");
        Ok(())
    } else {
        Err(msg)
    }
}

/// 启动串行环境流水线（应在独立线程调用；全进程只跑一次）。
pub fn run_startup_env_pipeline(app: tauri::AppHandle) -> PipelineReport {
    if !try_mark_pipeline_started() {
        log::warn!("startup-env pipeline already started; skip");
        return PipelineReport::default();
    }

    let app_audio = app.clone();
    let app_bridge = app.clone();
    let app_atvv = app.clone();

    let mut runner = PipelineRunner::new();
    runner.push(PipelineStep::Cable, || ensure_cable_once());
    runner.push(PipelineStep::WinUHid, || {
        crate::bridges::xiaomi::winuhid_env::ensure_runtime_quiet();
        Ok(())
    });
    runner.push(PipelineStep::WaitAudio, move || {
        let _ = &app_audio;
        wait_audio_router(std::time::Duration::from_secs(20))
    });
    runner.push(PipelineStep::WaitBridge, move || {
        let (alive, settle) = wait_bridge_settle(
            &app_bridge,
            std::time::Duration::from_secs(45),
            std::time::Duration::from_secs(8),
        );
        log::info!("startup-env bridge settle alive={alive} settle_ok={settle}");
        if !alive {
            return Err("桥接未在时限内启动（将依赖后续重连）".into());
        }
        Ok(())
    });
    runner.push(PipelineStep::Atvv, move || step_atvv_once(&app_atvv));

    let report = runner.run();
    log::info!(
        "startup-env pipeline done completed={} errors={}",
        report.completed,
        report.errors.len()
    );
    report
}

/// 后台启动流水线线程
pub fn spawn_startup_env_pipeline(app: tauri::AppHandle) {
    std::thread::Builder::new()
        .name("startup-env-pipeline".into())
        .spawn(move || {
            // 稍晚于窗口/托盘，避开启动尖峰；仍早于大部分用户交互
            std::thread::sleep(std::time::Duration::from_millis(800));
            let _ = run_startup_env_pipeline(app);
        })
        .ok();
}
