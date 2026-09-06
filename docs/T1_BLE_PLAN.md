# T1 蓝牙桥接计划与落地清单

> 与 USB 路径完全独立；USB 行为以 [T1_BRIDGE_PLAN.md](./T1_BRIDGE_PLAN.md) 为准。  
> 工作方式：TDD（红→绿）垂直切片；每步执行 → 测试 → 全量检查 → 修正 → 更新本清单。

## 当前方案：长时收音（已落地）

| 项 | 说明 |
|----|------|
| 协议 | Google ATVV（`AB5E0001-5A21-4F05-BC7D-AF01F617B664`） |
| 设备 | 已配对 `T1-Remote`；硬件 token `dev_vid&01620a_pid&0407` |
| 保活 | 语音会话中约每 **8s** 发 `MIC_EXTEND`（`0x0E 0x00`） |
| 效果 | **不受 USB Mic Device ~15s 静音限制**，可长时间收音并送 PCM |
| 音频 | ADPCM → PCM → UDP `audio_router` → **VB-CABLE / CABLE Output** |
| 快捷键 | 独立 `ble_voice` 闩锁；优先 WinUHid |
| 按键 | `ble_keys`：BLE Raw Input 匹配 + `native_suppress` 吞键 + WinUHid 映射 |
| 与 USB | `start_bridge("t1")` 不动；`start_t1_ble_bridge` 独立 IPC；suppress 与 USB 共存（停一侧不关另一侧闸门） |

## TDD 缝合点（本轮约定）

| 缝合点 | 测什么 | 位置 | 验证 |
|--------|--------|------|------|
| A. BLE 候选选择 | 地址 / 硬件 token / 名称匹配 | `ble_connect` unit | ⚠️ 本机 `cargo test --lib` 仍 `STATUS_ENTRYPOINT_NOT_FOUND`；代码已写测例 |
| B. BLE 设备路径匹配 | Raw Input 路径含 `01620a`/`0407`/`t1-remote` | `ble_keys` unit | 同上（编译通过） |
| C. 语音电平快照 | session / receiving / cable_active | `ble_voice_meter` unit | 同上 |
| D. 主机状态拼装 | all_ok / ATVV warn / idle | `ble_host` unit | 同上 |
| E. 前端接线 | 蓝牙按钮、电平、主机状态 | `t1SettingsWiring.test.ts` | ✅ 17 tests passed（含 3 条 wiring） |

## 分步清单（按实际落地勾选）

| 步骤 | 内容 | 状态 | 验证 |
|------|------|------|------|
| 0 | 本计划 + 长时收音方案入库 | ✅ | 本文档 |
| 1 | 文档：MIC_EXTEND 方案写入 T1_BRIDGE_PLAN | ✅ | 见该文档蓝牙节 |
| 2 | 主机状态 IPC + UI 信息区 | ✅ | `get_t1_ble_host_status`；T1 页 host-status；`cargo check` 绿；vitest 绿 |
| 3 | 独立 `ble_voice_meter` + 波形/虚拟声卡音量 UI | ✅ | IPC `get_t1_ble_voice_meter` + 事件 `t1-ble-voice-meter`；UI 音频信号/CableVolRuler |
| 4 | BLE 连接态按键：Raw Input + WinUHid 映射注入 | ✅ 代码落地 | `ble_keys`；蓝牙连上后 `start`；**待实机确认** BLE HID 路径 token 是否命中 |
| 5 | BLE 连接态吞键盘（与 USB 共存） | ✅ 代码落地 | `native_suppress`；LL 回调兼 `ble_keys::on_ll_gate_keydown`；USB stop 时若 BLE 在跑不关闸门 |
| 6 | 全量检查 / 修漏 / 文档如实标记 | ✅ | `cargo check --lib` 无告警；vitest T1 相关 17 通过；lib 测例因环境入口点无法跑 |

状态说明：✅ 已落地并验证 · ⬜ 未做 · ⚠️ 部分落地（文中注明缺口）

## 模块结构（已存在）

```
bridges/t1/
  ble_runtime.rs      # 启停 worker + keys 启停
  ble_connect.rs      # 发现/打开
  ble_session.rs      # ATVV + MIC_EXTEND + meter/atvv 标志
  ble_adpcm.rs        # ADPCM
  ble_pcm.rs          # UDP PCM + meter 上报
  ble_voice.rs        # 语音快捷键
  ble_voice_meter.rs  # 电平（独立，不调 xiaomi::voice_meter）
  ble_keys.rs         # BLE HID/键盘 → 映射注入
  ble_host.rs         # 主机状态快照
```

## 实机待确认（未勾选为完成）

- [ ] BLE 仅连接时方向/菜单等键 Raw Input 设备名是否含 `01620a` / `t1-remote`（若不命中需按实机路径补 token）
- [ ] 长按时波形与虚拟声卡音量条是否随说话跳动
- [ ] 蓝牙连接下 WinUHid 映射与吞键是否与 USB 行为一致、实体键盘是否仍可用

## 非目标（本轮不做）

- 改 USB `runtime.rs` 语音闩锁语义
- 把 T1 ATVV 接到 `bridges::xiaomi::input_session`
- 电池电量（T1 BLE 未验证 GATT 电池前不假报）
- 在 T1 页复刻小米「修复 VB-CABLE / WinUHid」全套向导（可沿用小米页修复；主机状态会提示）
