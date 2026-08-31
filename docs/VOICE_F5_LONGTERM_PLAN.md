# 语音键漏 F5 —— 长期方案（无 helper）

> 目标：源头清除为主、LL 钩子可靠兜底、可量化验收。  
> **不做** elevated helper / 整应用提权。  
> 方法：TDD（红 → 绿 → 检查修正 → 更新本文）。  
> **标记规则：只据自动测试实测结果勾选，禁止预估、禁止虚标。**

## 根因（保留）

1. 唯一稳的抑制层是 HID 报告清 usage `0x003E`（Tap/gadget）。
2. `WH_KEYBOARD_LL` 拿不到设备身份；回调超时会被 Windows **静默卸钩**。
3. ATVV 松开常早于固件 F5 KEYUP；若此时清 sticky，后续 typematic 漏进记事本。
4. bump「盲目 sleep」与嵌套一刀切转发，会制造抑制真空窗。

## 测试缝（Seams）

| # | 缝 | 断言什么 |
|---|-----|----------|
| S1 | `should_suppress_voice_f5` / sticky 生命周期 | 吞 **DOWN**/typematic（语音周期/armed/**mic·voice 关联窗**，**不含**整段 input session）；**配对 KEYUP**：sticky 时吞 UP（Python parity），无 sticky 的孤儿 UP 放行并清 sticky；`end_voice_period` 不清 sticky；会话结束 `disarm`；空闲解粘 |
| S2 | `special_keys` hook_proc + `voice_dispatch` | 回调内无 sleep/IO/`on_firmware_voice_key(` 同步重活；投递非阻塞 |
| S3 | `bump_hook_to_front_and_settle` / `hook_bump` | generation 落地；钩子线程自死锁检测；重叠先挂后卸 |
| S4 | `HOOK_PROC_DEPTH` 嵌套分支 | 注入键可放行；**F5 仍抑制**（含 probe/arm_output）；其余转发 |
| S5 | Tap UAC / 注入结果 | `ShellExecuteEx` 拒 UAC → `Ok(false)`；短退避 base=8s、max=60s；成功/失败/拒 UAC 可区分 |

**明确不做的缝：** helper 管道、计划任务、主进程自提权。

## 步骤

| # | 步骤 | 状态 | 测试目标 | 实测 |
|---|------|------|----------|------|
| 0 | 本计划 + 缝约定 | ✅ 完成 | 本文存在 | 文档已写入 |
| 1 | sticky 抑制语义修正 | ✅ 完成 | `tests/voice_f5_suppress_semantics.rs` + `voice_f5_home_like` | **12 + 19 passed / 0 failed**（2026-08-31 复查） |
| 2 | 钩子回调快进快出（voice_dispatch） | ✅ 完成 | `tests/hook_callback_nonblocking.rs`（`--test-threads=1`） | **9 passed / 0 failed** |
| 3 | bump 真落地（generation） | ✅ 完成 | `tests/hook_bump_to_front.rs`（`--test-threads=1`） | **11 passed / 0 failed** |
| 4 | 嵌套仍吞 F5 | ✅ 完成 | `tests/hook_nesting_guard.rs` + `voice_f5_home_like` | **5 + 19 passed / 0 failed** |
| 5 | 短 UAC 退避 + 注入结果可观测 | ✅ 完成 | `tests/hid_tap_health.rs` + `tests/hid_inject_result.rs` | **5 + 6 passed / 0 failed**；`ShellExecuteEx`+`ERROR_CANCELLED`→`Ok(false)`；base=8s、max=60s；**无 helper** |
| 6 | （可选）按次 F5 追踪 verdict | ⏭ 暂缓 | `tests/voice_guard_trace.rs` | **未实施**（未虚标） |
| 7 | 非阻塞 mic 关联吞 F5 | ✅ 完成 | `tests/voice_f5_correlate.rs` + 回归 | **5 + 13 + 19 = 37 passed / 0 failed**（2026-08-31）；见 `docs/VOICE_F5_CORRELATE_PLAN.md` |

## 落地模块（与仓库一致）

| 模块 | 作用 |
|------|------|
| `key_mapping.rs` | sticky / `end_voice_period` / `disarm` / 会话结束清 sticky / **mic 关联窗 + 迟到 sticky** |
| `voice_dispatch.rs` + `voice_worker.rs` | LL 回调只投递；工作线程跑 `on_firmware_voice_key` |
| `hook_bump.rs` | bump generation 等待落地 |
| `special_keys.rs` | 嵌套仍吞 F5；WM_BUMP `mark_handled`；重叠先挂后卸 |
| `hid_tap_health.rs` | UAC 短退避状态机 |
| `hid_inject_result.rs` | 注入结果槽（成功/失败/拒 UAC） |
| `hid_tap_injector.rs` | `ShellExecuteExW` 区分拒 UAC |
| `hid_report_tap.rs` | 拒 UAC 走短退避；附着时标 Attached |

**前端：** 本轮无 UI 徽章/设置项变更（追踪 Step 6 暂缓）；用户可见文案仅为 UAC 拒绝后的状态日志（「N 秒后重试」）。

## 实机验收（须人工，不计入步骤完成）

- [ ] 记事本长按语音键 ≥5s 不插日期  
- [ ] 短按/连按无裸 F5  
- [ ] 拒 UAC 后约 **8s** 可再试（连续拒绝指数退避，上限 **60s**），不狂弹  
- [ ] 真键盘 F5：遥控器**已连接**时也可用；语音按住期间可短暂被吞；断开会话后立即可用
- [ ] 语音松手后若固件 F5 仍按住，记事本仍不插日期（sticky 保留到 KEYUP / 10s 空闲）

## 变更日志

- 2026-08-30：建立本计划；完成 Step 0–5（无 helper）。Step 6 追踪暂缓。
- 2026-08-31 全量复查修正：
  - **Bug：** `ShellExecuteW` 拒 UAC 从未返回 `Ok(false)` → `ShellExecuteExW` + cancel
  - **Bug：** 会话结束不清 sticky → 断连后吞真键盘 F5 → 会话结束 `disarm`
  - 嵌套 F5 路径补齐 probe 副作用
- 2026-08-31 实机回归（粘键）：
  - **Bug：** sticky 吞 KEYUP；链头漏过的 KEYDOWN 无法配对 → F5 永久按下 → Ctrl+Win+F5，微信无法唤醒
  - **修：** `should_suppress_voice_f5` 对 **KEYUP 永远放行**并清 sticky；只吞 DOWN/typematic
- 2026-08-31 实机回归（真键盘 F5）：
  - **Bug：** `in_guard` 含 `input_session_active` → 遥控器已连接时真键盘 F5 全部被吞
  - **修：** 仅语音周期 / armed 时吞 DOWN；会话在线不再挡物理 F5（遥控器 F5 靠 gadget `0x003E` + 语音窗口）
- 2026-08-31 非阻塞关联吞（Step 7）：
  - mic/voice 标记关联窗 120ms；F5 先漏后 mic 迟到则补 sticky 堵 typematic
  - **不做** LL 内 sleep 等待（异于 Python）；测试见 `VOICE_F5_CORRELATE_PLAN.md`（37 passed）
- 2026-08-31 Python 对齐（见 `docs/VOICE_F5_PYTHON_PARITY_PLAN.md`）：
  - **修正**历史「KEYUP 永远放行」：改为 sticky 时配对吞 UP（Python `_should_suppress_voice_f5`）；孤儿 UP 仍放行
  - `0x04`/`0x00` 去掉双 begin/end period；单一入口 `handle_voice`
  - 回归 **67 passed / 0 failed**（voice_f5_* + hook_*）
- 2026-08-31 防 F5 粘键（日志 14:18 `leak_extra` + `keyup_suppress`）：
  - 有过 passthrough DOWN 的周期，UP 强制放行（`F5_DOWN_REACHED_OS`）
  - **43 passed / 0 failed**（suppress+correlate+home_like）

## 相关

- `docs/VOICE_F5_SIMPLE_PLAN.md`（gadget `0x003E` + 简化路径；后续强化见本文）  
- `docs/VOICE_HOLD_PR8.md`  
- `docs/VOICE_F5_ZERO_LEAK_PLAN.md`（历史重叠 bump 记录）  
