# T1 BLE 桥接 PRD — 正常 V1

> **里程碑**：commit `正常V1`（`feat/t1-ble-voice-bridge`，2026-09-07）  
> **定位**：当前**正确、可验收**的产品行为基线。后续改动不得破坏本节「验收标准」；缺陷见 [T1_BUGFIX.md](./T1_BUGFIX.md)。  
> **范围**：T1 **蓝牙**产品（ATVV 语音 + HOGP 按键映射；配置 `t1_ble.json`）。  
> USB 为独立产品入口，见 [T1_BRIDGE_PLAN.md](./T1_BRIDGE_PLAN.md) 与 [T1_USB_BLE_SPLIT.md](./T1_USB_BLE_SPLIT.md)。  
> 吞键：进程级一份 LL/HotKey，原因位启停，允许与 USB 同时开。

---

## 1. 产品目标

用户用 T1 遥控器蓝牙连接本应用后：

1. **语音键**：一点开麦并注入映射快捷键，再一点关麦并再注入一次（Toggle 语义与麦状态一致，不拧反）。  
2. **其它按键**：主页 / 删除 / 静音 / 方向 / 菜单 / 音量等按 UI 映射生效；未映射或同键映射则透传系统行为。  
3. **实体键盘**：桥接开启时，常用键（空格、退格、字母、未重映射的方向等）仍可用。  
4. **不抢桌面**：语音时吞掉 Browser Search（`0xAA` / AC Search），不因语音弹出系统搜索。

---

## 2. 语音（BLE / ATVV）— 必须遵守的架构

### 2.1 单一职责（本版核心结论）

| 通道 | 职责 | 禁止 |
|------|------|------|
| **HID AC Search** | **L0** 禁用 T1 BLE Consumer Control（Secure Boot 友好，可自动修复）+ **L1** 早注入映射和弦 + 静默武装 `0xAA` | 关麦抑制 / 去重窗内禁止再注 |
| **ATVV `START_SEARCH`** | **只决策开/关麦**；仅当 HID 未注入时兜底 `on_remote_press` | Skip 回声上不注入、不反复开麦 |
| **LL `VK_AA`** | 静默吞；dismiss 在和弦注入之后 | 不注入 |

**时序（微信，修订）**：仅「延后 dismiss」实机无效——微信还要求和弦**足够早**。因此 HID 先注 Ctrl+Win（等），ATVV 随后开/关麦；去重保证只注一次。豆包/千问对时序不敏感，同样走此路径。

**L0（现行）**：`pnputil` 禁用 T1 BLE 的 Consumer Control HID（不装自签 `.sys`、不关 Secure Boot）。启动流水线 / BLE 连接 / 看门狗可自动修复；设置页可手动修。

**拧反防护**：`host_mic_wanted` + `REOPEN_SUPPRESS` + `VOICE_DUP`；抑制期内 HID 也不得注入。

### 2.2 开麦状态机（唯一判据）

以主机意图 **`host_mic_wanted`**（与 `ble_voice` 侧 `MIC_SESSION_WANTED` 同步）为准：

| 当前状态 | 用户一次有效按（新物理按） | 动作 |
|----------|---------------------------|------|
| **未开麦** | 点语音键 | `MIC_OPEN` + 注入映射快捷键（Toggle 点按 / Hold 闩锁按配置） |
| **已开麦** | 再点语音键 | `MIC_CLOSE` + 再注入一次（Toggle）或结束闩锁（Hold） |
| 同一次按的固件/双发回声 | — | `SkipLive` 或 `SkipAfterClose`，**不注入、不反复开麦** |
| 关麦后短抑制窗内 | 晚到的 `START_SEARCH` | `SkipAfterClose`；**禁止注入** |

伪代码（与实现一致）：

```text
if host_mic_wanted:
  if within_dup_window → SkipLive
  else → CloseEnd
else:
  if reopen_suppressed → SkipAfterClose
  else → Open
```

### 2.3 时间窗（正常 V1 数值）

| 常量 | 值 | 用途 |
|------|-----|------|
| `VOICE_DUP_EVENT_MS` | **350** | 同一次物理按的双 `START_SEARCH` 去重；须短于正常连点间隔 |
| `REOPEN_SUPPRESS_MS` | **400** | CloseEnd / AUDIO_STOP 后挡住固件回声；inject 侧同样认此窗 |

不得随意加长到 ≥900ms：会把「一点开一点关」的连点当成同一次按，重新引入拧反或要点多次。

### 2.4 音频与快捷键

- PCM：ATVV ADPCM → `ble_pcm` → VB-CABLE；会话中约每 **8s** `MIC_EXTEND`（**必选保活**）。  
- **实机结论（2026-09-07）**：将 `MIC_EXTEND` 间隔改为 30min 后无法长时间收音 → **禁止**省略或显著拉长 8s 保活。  
- 映射：读 `t1.json` 的 `voice` / `voice_hotkey`；优先 WinUHid。  
- Toggle（如右 Alt+空格、Ctrl+Win）：每次有效开/关各完整点按一次。  
- Hold（如单独右 Alt）：第一次闩锁按下，第二次松开；忽略 HID 脉冲抬起。  
- `AUDIO_START`：仅当 `host_mic_wanted`；关麦后固件仍回 START 则忽略。

---

## 3. 按键映射（BLE HOGP）

### 3.1 通则

1. **重映射才吞原生 + 注入**；**同键 / 未绑定透传**（音量±未绑定须放行系统音量）。  
2. **静音**：桥接期始终闸门 + 注入（空绑默认系统静音 VK），不因「同键透传」丢掉。  
3. **方向 / OK**：仅绑到其它键时进 LL 闸门并补注入，避免「映射 + 原生」双发。  
4. **主页 / 删除**：优先 Consumer HID（如 `02-23` / `02-24`）早注入；别名含浏览器 Home/Back 与必要 VK；**不得**再用壳层吞 Home/Back APPCOMMAND 的方式抢语音（B18 已回滚，正常 V1 不恢复该做法）。  
5. 抬起清理吞键与 `release_all`，防止修饰键粘住。

### 3.2 正常 V1 验收键（实机已确认可用）

| 键 | 期望行为（示例） |
|----|------------------|
| 语音 | 一点开、一点关；音频电平与快捷键同步；不拧反 |
| 静音 | 映射或默认静音生效 |
| 主页 | 映射目标键生效（如空格） |
| 删除 | 映射目标键生效（如退格） |
| 方向 / 菜单 / 音量 | 按绑定：重映射注入，同键/未绑定透传 |

---

## 4. 非目标 / 禁止项（正常 V1）

- HID 与 ATVV **双路注入**语音快捷键。  
- 用过长去重/抑制窗「硬扛」双发（牺牲连点）。  
- 壳层 DLL 吞 Home/Back + pending 轮询（已证实干扰语音）。  
- 把 T1 ATVV 接到小米 `input_session`。  
- 为修主页/删除而破坏语音单一有效注入（去重）。  
- 省略或显著拉长 **8s `MIC_EXTEND`**（实机 30min 间隔无法长时收音）。  
- **自签 `.sys` + 关 Secure Boot / testsigning** 装过滤驱动（对用户不可接受）；现行 L0 为 `pnputil` 禁用 Consumer，无需改 BIOS。

---

## 5. 验收清单（正常 V1）

- [ ] BLE 连接后，语音：**连续多次**「开 → 说 → 关」，无拧反、无需连点多次才关。  
- [ ] 快速连点语音（间隔 > ~0.5s）：仍交替开/关，快捷键与麦一致。  
- [ ] 语音过程中系统搜索不抢焦点（Search 被吞）。  
- [ ] 主页 / 删除 / 静音映射按 UI 生效。  
- [ ] 方向重映射无原生双发；同键方向透传。  
- [ ] 音量未绑定可调系统音量；重映射后目标键生效。  
- [ ] 实体键盘空格/退格/字母在桥接开启时仍可用。  
- [ ] 停桥接后无粘键。

---

## 6. 代码锚点

| 职责 | 文件 |
|------|------|
| 开/关麦决策 | `ble_session.rs` → `start_search_mic_action` |
| 注入 + 抑制窗 + 去重 | `ble_voice.rs` |
| HID 语音只吞不注（ATVV 在线） | `ble_keys.rs` |
| Consumer / 闸门 / 主页删除静音 | `consumer_raw_input.rs`、`native_suppress.rs` |
| 缺陷史 | [T1_BUGFIX.md](./T1_BUGFIX.md) B17–B19 |
| 蓝牙计划 | [T1_BLE_PLAN.md](./T1_BLE_PLAN.md) |

---

## 7. 变更策略

- **正常 V1 为行为基线**：改语音/按键前先对照本文第 2–3 节。  
- 若需加长窗口、恢复 HID 早注入、或壳层吞 Home/Back，必须先更新本 PRD 并附实机验收，禁止静默回退。  
- 版本标记：文档与 commit 标题 **「正常V1」** 对齐；下一基线另起「正常V2」并说明相对本版的差分。
