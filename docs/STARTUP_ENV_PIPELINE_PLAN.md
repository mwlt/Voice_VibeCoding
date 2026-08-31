# 启动环境自动修复流水线（Startup Env Pipeline）

> 目标：软件启动后按固定顺序、互斥地自动修复未就绪组件，减少用户手动点「修复」。  
> 原则：能自动修就修一次；需要重启则只提示；不并行提权/重启桥接。

## 用户确认的顺序

1. **虚拟声卡（VB-CABLE）**：未就绪 → 内嵌 Repair **一次**；`needs_reboot` 则同开机内不再反复安装；**OS 重启后**若仍未就绪再试一次  
2. **虚拟键盘（WinUHid）**：沿用 `ensure_runtime_quiet`（含重启后一次自动修）  
3. **语音路由**：等待 audio router 就绪（已有 spawn，此处只等）  
4. **桥接**：沿用现有启动自动连接；流水线 **等待落定**，不额外 restart  
5. **ATVV**：桥接已在跑且仍无 ATVV → 跑现有 `run_atvv_repair_pipeline` **一次**

## TDD 测试缝（Seams）

| Seam | 行为 | 自动化 |
|------|------|--------|
| `should_auto_repair_cable(ready, attempted, reboot_pending)` | 是否应对声卡跑一次自动修 | `cargo test --test startup_env_pipeline` ✅ |
| `cable_reboot_blocks_auto_repair(flag, age, uptime)` | 同开机阻断 / 重启后放行 | 同上 ✅ |
| `should_auto_repair_atvv(bridge_alive, atvv_ok, attempted, settle_ok)` | 是否应对 ATVV 跑一次修复 | 同上 ✅ |
| `pipeline_steps()` | 固定步骤顺序 | 同上 ✅ |
| `PipelineRunner`（可注入步骤） | 串行、不重叠执行；一步失败继续后续 | 同上 ✅ |
| 启动接线 | `lib.rs` 只起一条 pipeline；无并行 `winuhid-ensure` | 审查 + `cargo check` ✅ |

真机 UAC / 驱动安装冒烟另测（不强制 CI）。

**最近验证**：`cargo test --test startup_env_pipeline` → **7 passed**；`cargo check -p remote-bridge-hub` → ok（2026-08-31）。

## 实施步骤与完成标记

| Step | 内容 | 状态 | 落地证据 |
|------|------|------|----------|
| 0 | 本文档与缝约定 | ✅ | `docs/STARTUP_ENV_PIPELINE_PLAN.md` |
| 1 | 纯函数决策 + 顺序测试 | ✅ | `should_*` / `pipeline_steps` 测试绿 |
| 2 | 串行 PipelineRunner | ✅ | `runner_executes_steps_serially_*` 绿 |
| 3 | VB-CABLE 启动一次性自动修 | ✅ | `ensure_cable_once` + reboot flag + 重启后放行；相关单测绿。**真机 UAC 未在 CI 跑** |
| 4 | WinUHid 纳入流水线（取消并行 spawn） | ✅ | `run_startup_env_pipeline` 调 `ensure_runtime_quiet`；`lib.rs` 仅 `spawn_startup_env_pipeline`，无 `winuhid-ensure` 线程 |
| 5 | 等待语音路由 + 桥接落定 | ✅ | `wait_audio_router` / `wait_bridge_settle` 编入流水线；时序依赖真机，无独立 mock 测试 |
| 6 | ATVV 一次性自动修 | ✅ | `step_atvv_once` → `run_atvv_repair_pipeline`；决策单测绿。**真机桥接冒烟未在 CI 跑** |
| 7 | 接入 `lib.rs` + 文档收尾 | ✅ | `pub mod startup_env`；setup 中 `startup_env::spawn_startup_env_pipeline`；本文档按实测标记 |

## 代码入口

- 模块：`src-tauri/src/startup_env.rs`
- 启动：`src-tauri/src/lib.rs` → `startup_env::spawn_startup_env_pipeline`
- 测试：`src-tauri/tests/startup_env_pipeline.rs`

## 注意

- ATVV 修复 = 重启桥接；不要再叠「桥接自动修复」。  
- 声卡/键盘提权可能弹 UAC；流水线串行避免双 UAC 抢焦点。  
- 标记完成必须以测试通过 + 代码接线落地为准，禁止空标。
