# T1 实机桥接（连接 + 语音）

对齐 Python `before-v0.6.42` 的 Raw Input / Standalone 路径。

> **产品拆分**：侧栏 **T1(蓝牙)** / **T1(USB)** 分离；配置 `t1_ble.json` / `t1_usb.json`。  
> 契约见 [T1_USB_BLE_SPLIT.md](./T1_USB_BLE_SPLIT.md)。本文件偏 USB；蓝牙见 [T1_BLE_PLAN.md](./T1_BLE_PLAN.md)。

## 状态

| 能力 | 状态 | 说明 |
|------|------|------|
| `start_bridge("t1_usb")`（别名 `"t1"`） | 已接入 | USB 接收器路径 |
| Consumer HID `0x0C/0x01` | 已接入 | [`consumer_raw_input.rs`](../src-tauri/src/bridges/t1/consumer_raw_input.rs) |
| VID/PID `1915:1025` 过滤 | 已接入 | 设备路径子串匹配 |
| HID → 按键映射 → 注入 | 已接入 | **优先 WinUHid**，失败降级 SendInput；注入在后台线程，不堵 WM_INPUT |
| 键盘重映射吞原生键 | 已接入 | LL 钩子短暂抑制遥控原生 VK，避免双发 |
| 语音注入 | 已接入 | **优先 WinUHid**（豆包/千问过滤 SendInput）；Hold 闩锁 / Toggle 点按 |
| 语音 Toggle / Hold + Mic Device | 已接入 | **USB** Standalone；不混 VB-CABLE |
| `start_t1_ble_bridge` / 蓝牙连接 | 已接入 | **独立** ATVV 路径；与 USB runtime 无共用；PCM → VB-CABLE |
| UDP 30682 控制面 | 未做 | 一体式 Tauri 不需要 |
| Hub AudioRouter 混音（USB） | 不做 | USB 仍用 Mic Device |

## 用法

### USB（原路径，不变）

1. 插入 T1 USB 接收器（VID_1915&PID_1025）
2. T1 设置页点「连接设备」（USB）
3. 按遥控器 Consumer HID 键应触发已配置映射
4. 语音键：需 Windows 捕获端点名含 `Mic Device`；成功后按 `trigger_mode` 注入 `voice_hotkey`
   - **豆包长按**：映射 `rightalt`，`trigger_mode=Hold`，豆包侧快捷键=右 Alt + 长按模式；麦克风选 **Mic Device**（不是 CABLE）
   - **豆包免按**：映射 `rightalt+space`，`trigger_mode=Toggle`
   - 若日志出现 SendInput 降级：到小米页点「修复虚拟键盘」（WinUHid），否则豆包常无反应
5. 光标需在可输入框内；改映射后无需重连（运行时每次读最新配置）

### 蓝牙（独立路径）

1. 系统蓝牙设置中配对 **T1-Remote**（本机示例 `12:AC:2C:46:C4:AB`，硬件 token `dev_vid&01620a_pid&0407`）
2. T1 设置页点 **「蓝牙连接」**（不走 `start_bridge("t1")`）
3. 语音走 Google ATVV（`AB5E0001-…`），会话中约每 8s `MIC_EXTEND`（可突破 USB 麦约 15s 静音）
4. PCM → 本机 `audio_router` / **CABLE Output**；豆包麦克风请选 **CABLE Output**
5. USB 与蓝牙可分别启停，互不影响

## 模块

### USB

- `bridges/t1/runtime.rs` — 启停与事件分发
- `bridges/t1/consumer_raw_input.rs` — Raw Input + HID 解析
- `bridges/t1/mapping.rs` — event_id → button
- `bridges/t1/inject.rs` — SendInput（普通键 + 语音降级）
- `bridges/t1/native_mic.rs` — 注册表枚举捕获端点
- 语音优先复用 `bridges/xiaomi/hid_injector.rs`（WinUHid）

### 蓝牙（独立文件，不 import 小米 ATVV 模块）

- `bridges/t1/ble_runtime.rs` — `start_t1_ble_bridge` / `stop_t1_ble_bridge`
- `bridges/t1/ble_connect.rs` — 发现已配对 T1-Remote + ATVV
- `bridges/t1/ble_session.rs` — ATVV 订阅 / MIC_OPEN / MIC_EXTEND
- `bridges/t1/ble_adpcm.rs` — ADPCM 自包含副本
- `bridges/t1/ble_pcm.rs` — UDP → audio_router
- `bridges/t1/ble_voice.rs` — 语音快捷键注入（独立闩锁）
- IPC：`start_t1_ble_bridge` / `stop_t1_ble_bridge` / `t1_ble_running`；事件 `t1-ble`

## 缺口 / 备注

- **`cargo test --lib`**：本机仍报 `STATUS_ENTRYPOINT_NOT_FOUND`（既有环境问题）。
- **录入修复（2026-09-05）**：T1 Raw Input 在快捷键录入期间改为空操作，避免拖垮 LL 吞键钩子；前端增加 keydown/keyup 和弦兜底。
- **豆包语音（2026-09-05）**：T1 原先只 SendInput 点按，豆包会过滤且与「长按右 Alt」语义不符；已改为 WinUHid 优先 + Hold 闩锁。
- **媒体键映射（2026-09-05）**：音量/静音/删除/主页等 Consumer HID 注入成功后，系统仍会冒出 VK_VOLUME_/BROWSER_*；现于 Raw Input 回调内同步武装 LL 抑制，并 HID+KBD 去重。
- **LL-first 闸门（2026-09-05）**：菜单/主页等键 LL 早于 Raw Input，事后吞键无效；T1 运行期对 Apps/Browser/Volume 等 VK 在 LL 先吞，T1 认领后只注入映射，实体键盘同 VK 可回放。不需要单独单击/双击模式。
- **语音时长（2026-09-05）**：对齐 Python `audio_max_hold_ms=300000`；Hold 闩锁增加 1.5s 心跳重按，避免右 Alt 被系统静默松开后约 20s 停录。
- **USB 麦约 15s 静音**：固件/USB 音频限制；BLE 路径用 MIC_EXTEND 规避。
- **蓝牙（2026-09-06）**：独立 BLE 按钮与模块已接入；主机状态 / 语音电平 / 按键+吞键代码已落地。清单与实机待确认项见 [T1_BLE_PLAN.md](./T1_BLE_PLAN.md)。

## Rust T1 vs Python T1 差异

| 项 | Python T1 | Rust T1（当前） |
|----|-----------|-----------------|
| 语音 HID 脉冲 ~120ms | 有；忽略抬起；二次按下结束 | 同左（Hold 闩锁 / Toggle 点按） |
| 默认语音快捷键 | `leftshift+k` + **toggle_hotkey 点按** | 用户可配；单 Alt 强制 **Hold 按住**（豆包长按） |
| 输入法收到的键 | 快速点按（非长按） | Hold=持续按住；Toggle=点按 |
| `audio_max_hold_ms` | **300000（5 分钟）强制收口** | **已对齐 5 分钟** |
| 静音自动停 | 参数有，但默认 `second_press`，UI 写「停顿不断开」 | 无静音看门狗（靠二次按 / 5 分钟 / 实体键） |
| USB 音频路径 | 校验 `Mic Device`，IME 自开端点；不混 VB-CABLE | 同左 |
| BLE 音频路径 | **无** | ATVV → VB-CABLE（`start_t1_ble_bridge`） |
| USB 麦慢醒 | `open_timeout=8s`，注释称可 >5s | 仅注册表校验端点存在，无重试等待 |
| `listen_keyboard` | **false**（additive，不吞原生键） | **true 路径**：LL 闸门 + 重映射吞键 |
| `handle_key_up` | **false**（忽略抬起） | 语音忽略抬起；其它键抬起清吞键/`release_all` |
| HID 去重 | `dedupe_hid_reports` | 有 button 级去重 + HID/KBD 去重 |
| 控制面 UDP 30682 | 有（stop_voice / 状态） | 无（Tauri 一体） |
| 「立即结束语音」 | UI 有 | 再按语音键 / 停桥接 / Esc·实体键结束闩锁 |
| 电源/鼠标键 | 默认空映射 | 同配置模型，视 `t1.json` |
| 删除键冷却 | 120ms | 视映射，无强制 120ms cooldown |
| 虚拟键盘 | SendInput 为主（旧） | **WinUHid 优先**（豆包过滤 SendInput） |
| Alt 粘键防护 | 点按模式不易粘 | Hold 会粘；心跳保活 + 实体键/二次按解除 |