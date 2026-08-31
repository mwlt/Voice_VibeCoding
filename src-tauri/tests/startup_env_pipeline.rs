//! 启动环境流水线：决策与步骤顺序（不跑真实驱动安装）

use remote_bridge_hub_lib::startup_env::{
    pipeline_steps, should_auto_repair_atvv, should_auto_repair_cable, try_mark_pipeline_started,
    PipelineRunner, PipelineStep,
};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[test]
fn pipeline_order_is_cable_winuhid_audio_bridge_atvv() {
    assert_eq!(
        pipeline_steps(),
        &[
            PipelineStep::Cable,
            PipelineStep::WinUHid,
            PipelineStep::WaitAudio,
            PipelineStep::WaitBridge,
            PipelineStep::Atvv,
        ]
    );
}

#[test]
fn cable_auto_repair_only_when_not_ready_and_not_attempted() {
    assert!(should_auto_repair_cable(false, false, false));
    assert!(!should_auto_repair_cable(true, false, false));
    assert!(!should_auto_repair_cable(false, true, false));
    assert!(!should_auto_repair_cable(false, false, true));
}

#[test]
fn atvv_auto_repair_only_after_bridge_settle_without_atvv() {
    assert!(should_auto_repair_atvv(true, false, false, true));
    assert!(!should_auto_repair_atvv(false, false, false, true));
    assert!(!should_auto_repair_atvv(true, true, false, true));
    assert!(!should_auto_repair_atvv(true, false, true, true));
    assert!(!should_auto_repair_atvv(true, false, false, false));
}

#[test]
fn runner_executes_steps_serially_in_order_and_continues_after_error() {
    let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let active = Arc::new(Mutex::new(0usize));
    let max_active = Arc::new(Mutex::new(0usize));

    let mk = |name: &'static str, fail: bool| {
        let log = Arc::clone(&log);
        let active = Arc::clone(&active);
        let max_active = Arc::clone(&max_active);
        move || {
            {
                let mut a = active.lock().unwrap();
                *a += 1;
                let mut m = max_active.lock().unwrap();
                if *a > *m {
                    *m = *a;
                }
            }
            std::thread::sleep(Duration::from_millis(30));
            log.lock().unwrap().push(name.to_string());
            {
                let mut a = active.lock().unwrap();
                *a -= 1;
            }
            if fail {
                Err(format!("{name} failed"))
            } else {
                Ok(())
            }
        }
    };

    let mut runner = PipelineRunner::new();
    runner.push(PipelineStep::Cable, mk("cable", false));
    runner.push(PipelineStep::WinUHid, mk("winuhid", true)); // error mid-way
    runner.push(PipelineStep::WaitAudio, mk("audio", false));
    let report = runner.run();

    assert_eq!(
        *log.lock().unwrap(),
        vec!["cable", "winuhid", "audio"]
    );
    assert_eq!(*max_active.lock().unwrap(), 1, "must not overlap steps");
    assert_eq!(report.completed, 3);
    assert_eq!(report.errors.len(), 1);
    assert!(report.errors[0].contains("winuhid"));
}

#[test]
fn cable_reboot_pending_flag_roundtrip() {
    use remote_bridge_hub_lib::startup_env::{
        cable_reboot_pending, set_cable_reboot_pending, should_auto_repair_cable,
    };
    set_cable_reboot_pending(false);
    assert!(!cable_reboot_pending());
    assert!(should_auto_repair_cable(false, false, cable_reboot_pending()));
    set_cable_reboot_pending(true);
    assert!(cable_reboot_pending());
    assert!(!should_auto_repair_cable(false, false, cable_reboot_pending()));
    set_cable_reboot_pending(false);
    assert!(!cable_reboot_pending());
}

#[test]
fn cable_reboot_blocks_only_until_os_reboot() {
    use remote_bridge_hub_lib::startup_env::cable_reboot_blocks_auto_repair;
    use std::time::Duration;

    // no flag → never blocks
    assert!(!cable_reboot_blocks_auto_repair(false, None, Some(Duration::from_secs(3600))));

    // same boot: flag younger than uptime → still blocks (avoid repeat UAC)
    assert!(cable_reboot_blocks_auto_repair(
        true,
        Some(Duration::from_secs(60)),
        Some(Duration::from_secs(3600))
    ));

    // after OS reboot: flag older than uptime → allow one more attempt
    assert!(!cable_reboot_blocks_auto_repair(
        true,
        Some(Duration::from_secs(3600)),
        Some(Duration::from_secs(60))
    ));
}

#[test]
fn pipeline_started_flag_is_one_shot() {
    // integration with process flag — first wins
    // Note: other tests in this binary may have set it; we only assert compare-exchange semantics
    // via two calls in isolation when first returns true then second false.
    // If already started by a previous test in-process, skip soft.
    let first = try_mark_pipeline_started();
    let second = try_mark_pipeline_started();
    if first {
        assert!(!second);
    } else {
        assert!(!second);
    }
}
