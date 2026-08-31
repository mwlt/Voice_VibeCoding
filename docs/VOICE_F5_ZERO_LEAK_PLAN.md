# F5 零泄漏交付（TDD）

> 目标：消除**本进程自造的 Unhook 空窗**；源头清 `0x003E`；会话 LL 吞作兜底。  
> 标记规则：仅据自动测试实测勾选。

## 根因

1. 并行 BLE HID F5（非 WinUHid）。  
2. **`bump` 先 Unhook 再挂** → 空窗 F5 直达（日志里我们事后 `suppressed`，但微信可能已先看见）。  
3. Gadget 须加载含 `0x003E` 的新脚本。

## 步骤

| 步骤 | 内容 | 状态 | 验证 |
|------|------|------|------|
| 0 | 本计划 | ✅ | 本文 |
| 1 | 重叠 bump（先 Set 再 Unhook）+ `HOOK_PROC_DEPTH` | ✅ | `bump_installs_*` + `ll_proc_has_nesting_guard_*` |
| 2 | Gadget 戳 `v1.5.9-f5-zero` + `cleared_f5` 事件 | ✅ | `gadget_clears_keyboard_f5_usage` |
| 3 | `handle_voice`：先 period 再 bump(40ms) | ✅ | `voice_press_bumps_hook_like_pr8` |
| 4 | 回归 | ✅ | `voice_f5_home_like` **19 ok** + `ime_voice_wake_route` **4** + `voice_chord_and_release` **8**（2026-08-30） |

## 实机（须你）

1. **完全退出**旧进程再开（强制重载 WUDFHost / 新 gadget）。  
2. 日志应偶发 `HID TAP cleared_f5`（若该路报告含 F5）。  
3. 微信设置框：语音键不应再稳定出现裸 F5。  

## 诚实上限

无法撤回「已排在我们前面的钩子」见过的键；重叠 bump 消除的是**我们自己的空窗**。若仍漏，下一步才是 HidHide 藏键盘集合。
