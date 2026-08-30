# 语音键 F5 简化方案（实施计划）

> 目标：去掉中和/多状态机复杂度，收成「源头清 F5 + 会话吞 + 注入前 bump + WinUHid 映射」。  
> 方法：TDD，每步 = 红→绿→检查→修正→更新本文完成标记（**仅据实测勾选**）。

## 根因更正（2026-08-30 diagnosing-bugs）

USB HID Keyboard：**F1=`0x003A`，F5=`0x003E`**（VK_F5=`0x74` → usage = `0x74-0x70+0x3A` = `0x3E`）。  
此前 gadget **误清 `0x003A`(F1)**，固件语音键的 **F5 从未在源头被抹掉**，只能靠 LL 钩子事后抢，所以在线键盘测试仍见 F5、微信链头仍脏、偶发粘键。  
同业（macOS remote-mic-app）将 RC003 语音识别为 usage **`0x3E`**，Typeless 模式把 F5 映到 usage 0。

另：若 bump 空窗漏过 KEYDOWN 仍吞 KEYUP → F5 粘键；UP 仅在曾吞过 DOWN 时才吞。

## 测试缝（Seams）

| # | 缝 | 断言什么 |
|---|-----|----------|
| S1 | `xiaomi_hid_gadget.js` | HID usage **`0x003E`(F5)** 被清零；**不得**再把 `0x003A` 当 F5；版本戳触发宿主重启 |
| S2 | `should_suppress_voice_f5` | 会话/周期/armed 时吞 down；**orphan UP 放行**；无会话不吞物理 F5；LL 内无 sleep |
| S3 | `handle_voice` | 注入前 `bump_hook`（对齐 PR #8） |
| S4 | `inject_voice_chord` / `special_keys` | **无** neutralize / SendInput 清 F5 |
| S5 | `inject_voice_chord` + `handle_voice` | 仅 WinUHid；拒绝映射为 F5 |
| S6 | `key_probe` | 独立 `key-probe.log`；可分析 leak / stuck / ctrl-without-win |

## 简化后唯一路径

```
gadget 清 0x003E(F5) → 主（F5 不进 Windows）
会话中 LL 吞 F5      → 记事本兜底（orphan UP 放行防粘键）
注入前 bump 钩子     → 微信链头兜底（PR #8）
WinUHid 发映射键     → IME 能认（无 INJECTED）
```

**已删除**：`neutralize_f5` / `press_report_state`、async/sync 中和、SendInput F5 KEYUP、LL 内逐次 F5 INFO 刷屏。

## 步骤完成情况

| 步骤 | 内容 | 状态 | 验证命令 / 证据 |
|------|------|------|-----------------|
| 0 | 本计划 + 缝约定 | ✅ 已落地 | 本文存在 |
| 1 | Gadget 清 **`0x003E`** + 戳 `v1.5.8-voice-f5-usage` | ✅ 代码更正 | 旧戳误清 F1；须实机确认 WUDFHost 重启后无 F5 |
| 2 | 简化 LL 会话/周期吞 F5 + orphan UP 放行 | ✅ 代码测 | `f5_keyup_passes_if_keydown_never_suppressed` |
| 3 | 注入前 bump（PR#8） | ✅ 代码测 | `voice_press_bumps_hook_like_pr8` |
| 4 | 移除 neutralize | ✅ | — |
| 5 | WinUHid-only + 拒 F5 映射 | ✅ | — |
| 6 | 键盘探测 UI + `key-probe.log` + HITL | ✅ 代码 | 设置页「键盘探测」；`scripts/hitl-key-probe-voice.ps1` |

说明：本机 `cargo test --lib` 可能 `0xc0000139`；以 `src-tauri/tests/` 为准。

## 实机（需用户）

- [ ] **完全退出旧进程**后启动；日志有 HID Tap script changed / host restart（`v1.5.8-voice-f5-usage`）
- [ ] 「键盘探测」：语音键 3 次 → **F5泄漏=否**、**粘键嫌疑=否**、见映射键
- [ ] 在线键盘测试：无 F5；Ctrl+Win 成对
- [ ] 微信快捷键框 / 按住说话 / 记事本不插日期

## 用户态做不到的上限

- LL `return 1` **不能**抹掉已排在更前的钩子（如微信）已看到的 F5 → 必须源头清 + bump 抢链头  
- ATVV 与 BLE HID 是**并行**两路：吞 ATVV 侧映射不等于固件 F5 消失  

## 相关

- **后续强化（无 helper）：** `docs/VOICE_F5_LONGTERM_PLAN.md`（sticky / voice_dispatch / bump generation / 嵌套仍吞 F5 / UAC 短退避）
- `docs/VOICE_HOLD_PR8.md`
- `scripts/hitl-key-probe-voice.ps1`
