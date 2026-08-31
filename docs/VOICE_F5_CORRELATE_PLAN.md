# 语音 F5 —— 非阻塞关联吞（补充方案）

> 在现有「gadget 清 `0x003E` + 语音周期/armed + **配对 KEYUP 吞（sticky）**」之上，增加 **mic/voice 信号时间戳关联**。  
> LL 内允许 **bounded** wait mic（Python 80ms）；非阻塞关联窗仍为 120ms。  
> 方法：TDD。**标记只据自动测试实测，禁止虚标。**

## 目标

1. `mark_direct_signal(mic|voice)` 之后极短窗内，F5 DOWN 可被吞（显式关联缝，不依赖整段 session）。  
2. **F5 先漏、mic 后到**（≤关联窗）：置 sticky，后续 typematic 不再漏（首帧可能已漏——非阻塞无法撤回）。  
3. 无 mic/周期/armed → 真键盘 F5 放行。  
4. 抑制路径源码无 `sleep` / `wait` / `park`。

## 测试缝

| # | 缝 | 断言 |
|---|-----|------|
| C1 | `should_suppress_voice_f5` + `mark_direct_signal` | mic 标记后短窗内 DOWN 吞；无标记不吞 |
| C2 | `note_passthrough_f5_down` + 随后 `mark_direct_signal(mic)` | 关联窗内置 sticky，再 DOWN 吞 |
| C3 | 关联窗外再 mark | 不因过期 passthrough 误 sticky |
| C4 | `should_suppress_voice_f5` 源码 | 无 sleep/wait_for |

## 步骤

| # | 步骤 | 状态 | 测试 | 实测 |
|---|------|------|------|------|
| 0 | 本计划 | ✅ 完成 | 本文存在 | 文档已写入 |
| 1 | mic 标记后短窗吞 DOWN | ✅ 完成 | `tests/voice_f5_correlate.rs` | **`mic_mark_then_f5_down_is_suppressed_without_voice_period` passed**（2026-08-31） |
| 2 | F5 passthrough 后 mic 迟到 → sticky | ✅ 完成 | 同上 | **`f5_passthrough_then_late_mic_sets_sticky_for_typematic` passed** |
| 3 | 窗外 / 无标记不误伤真键盘 | ✅ 完成 | 同上 | **`no_mic…` + `late_mic_outside…` passed** |
| 4 | LL 路径无阻塞 + 回归 | ✅ 完成 | correlate + suppress_semantics + home_like | **5 + 13 + 19 = 37 passed / 0 failed**（`--test-threads=1`） |

## 落地（与仓库一致）

| 符号 / 模块 | 作用 |
|-------------|------|
| `VOICE_F5_CORRELATE_MS` (=120) | 非阻塞关联窗 |
| `note_passthrough_f5_down` | F5 放行时记时刻（`special_keys` passthrough 路径调用） |
| `apply_late_correlate_from_passthrough_f5` | mic mark 时若窗内有 passthrough → 补 sticky |
| `voice_mic_correlate_active` | `should_suppress_voice_f5` 的 in_guard 第三条件 |
| `mark_direct_signal(mic\|voice)` | 仍 `arm` + 迟到关联 |

**明确不做：** LL 内 sleep 等 mic；SendInput F5 KEYUP 中和。

## 变更日志

- 2026-08-31：建立本计划；TDD 完成 Step 0–4。  
- 2026-08-31：**Python 对齐 + post-tail**  
  - `wait_for_mic_correlate`：会话在线、无 guard 时 F5 DOWN 最多 wait **80ms**（Python parity）。  
  - `VOICE_F5_POST_TAIL_MS`（3s）：`end_voice_period` 后继续吞松手后的 F5 typematic（修 11:49:13 泄漏）。  
  - `end_voice_period` / `disarm` 清 stale `LAST_PASSTHROUGH_F5_DOWN`。
- 2026-08-31：KEYUP 语义改由 `VOICE_F5_PYTHON_PARITY_PLAN.md` 管理（sticky 配对吞）；本文件关联测试仍绿（7 passed）。

## 相关

- `docs/VOICE_F5_LONGTERM_PLAN.md`（主路径 Step 0–5）  
- `docs/VOICE_F5_SIMPLE_PLAN.md`  
- Python 对照：`atvv_live_bridge._should_suppress_voice_f5`（钩子内 wait≈80ms，本方案刻意不采用）
