# 语音 F5 —— Python 对齐（UP 配对吞 + 去重 period）

> 实机日志（2026-08-31）：切换窗口后要按两次才能唤醒微信；桥接层每次都有 `SHORTCUT DOWN`，但松键时 `phase=up action=passthrough` + `CONFLICT leak_while_guard_active`。  
> 根因候选：Rust **吞 F5 DOWN、放行 F5 UP**（异于 Python）；`0x04` 路径 **双 `begin_voice_period`**。  
> 方法：TDD。**标记只据自动测试实测，禁止虚标。**

## 目标

1. **F5 UP**：仅当此前 DOWN 已被本钩 suppress（sticky）时吞掉 UP 并清 sticky —— 对齐 Python `_should_suppress_voice_f5`。  
2. **孤儿 UP**：从未 suppress 过 DOWN（sticky=false）→ UP 必须放行（解粘漏到 OS 的 KEYDOWN）。  
3. **`0x04` 路径**：`begin_voice_period` 只调用一次（去掉 `input_session` + `handle_voice` 双调）。

## 测试缝

| # | 缝 | 断言 |
|---|-----|------|
| P1 | `should_suppress_voice_f5(up)` | sticky 后 UP → `true` 并清 sticky；无 sticky 的 UP → `false` |
| P2 | `should_suppress_voice_f5` 源码契约 | UP 分支按 sticky 决定，**不再** `always return false` |
| P3 | `0x04` / `handle_voice` / `input_session` | 单次 `begin_voice_period`（源码/契约测试） |

**明确不做：** 重新引入 early inject / focus 追踪；改 Frida 为进程外 attach。

## 步骤

| # | 步骤 | 状态 | 测试 | 实测 |
|---|------|------|------|------|
| 0 | 本计划 + 缝约定 | ✅ 完成 | 本文存在 | 文档已写入 |
| 1 | F5 UP 配对吞（Python parity） | ✅ 完成 | `voice_f5_suppress_semantics` + `home_like` + `correlate` | **14 + 19 + 7 = 40 passed / 0 failed**（`--test-threads=1`，2026-08-31） |
| 2 | 去掉双 `begin_voice_period` / 双 `end` | ✅ 完成 | `voice_f5_home_like` `audio_start_0x04_*` / `audio_stop_0x00_*` | **21 passed / 0 failed**（home_like 全量含新契约，2026-08-31） |
| 3 | 全量 voice_f5 / hook 回归 | ✅ 完成 | correlate + suppress + home_like + hook_* | **7+14+21+9+5+11 = 67 passed / 0 failed**（`--test-threads=1`，2026-08-31） |
| 4 | 漏 DOWN 后 UP 强制放行（防 F5 粘键） | ✅ 完成 | `f5_keyup_must_pass_after_leaked_passthrough_even_if_sticky` | **suppress 15 + correlate 7 + home_like 21 = 43 passed / 0 failed**（2026-08-31） |

## 对照（Python）

```text
if is_up:
    matched = voice_f5_down_suppressed
    voice_f5_down_suppressed = False
    return matched   # sticky→吞 UP；否则放行
```

## 变更日志

- 2026-08-31：建立本计划；Step 0 完成。
- 2026-08-31 Step 1 落地（实测）：
  - `should_suppress_voice_f5`：UP 按 sticky 配对吞（`swap` 清 sticky）；孤儿 UP 仍放行。
  - 测试：`f5_keyup_suppressed_when_sticky_python_parity` / `f5_keyup_passes_when_never_sticky`；旧「KEYUP 永远放行」断言已改。
  - **40 passed / 0 failed**（suppress 14 + home_like 19 + correlate 7）。
- 2026-08-31 Step 2 落地（实测）：
  - `input_session` `0x04` 去掉 `begin_voice_period`；`0x00` 去掉 `end_voice_period`。
  - 单一入口：`handle_voice(true/false)`。
  - **home_like 21 passed / 0 failed**（含 `audio_start_0x04_must_not_double_begin_voice_period`、`audio_stop_0x00_must_not_double_end_voice_period`）。
- 2026-08-31 Step 3 回归（实测）：
  - `voice_f5_correlate` 7 + `voice_f5_suppress_semantics` 14 + `voice_f5_home_like` 21
  - `hook_callback_nonblocking` 9 + `hook_nesting_guard` 5 + `hook_bump_to_front` 11
  - **合计 67 passed / 0 failed**（`--test-threads=1`）。
- 2026-08-31 Step 4 防粘键（实测，日志 14:18）：
  - **Bug：** DOWN `leak_extra` 进 OS 后，late sticky + `keyup_suppress` → F5 永久按下。
  - **修：** `F5_DOWN_REACHED_OS`；有过 passthrough 的周期 UP 强制放行（`keyup_pass_unstick_leak`）。
  - **43 passed / 0 failed**（suppress 15 + correlate 7 + home_like 21）。

## 相关

- `docs/VOICE_F5_LONGTERM_PLAN.md`（历史「KEYUP 永远放行」将被本计划 Step 1 修正）  
- `docs/VOICE_F5_CORRELATE_PLAN.md`  
- Python：`atvv_live_bridge._should_suppress_voice_f5`
