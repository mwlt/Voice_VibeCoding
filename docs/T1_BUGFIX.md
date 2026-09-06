# T1 桥接缺陷修复记录

> 设备：Google T1 Remote（`VID_1915&PID_1025`）  
> 代码：`src-tauri/src/bridges/t1/`  
> 更新日期：2026-09-05

---

## 总原则

1. **实体键盘优先**：不得因 T1 闸门导致空格 / 退格 / 方向 / 字母等常用键失效。  
2. **重映射必须吞原生**：遥控方向等若映射到其它和弦，必须在按住期间吞掉原生 VK，否则会与 WinUHid 合并成错误和弦并粘键。  
3. **抬起必须清理**：按键抬起时解除吞键，并 `WinUHid release_all`，防止右 Alt 等粘住。

---

## Bug 列表与修复

### B1. 右方向 →「右 Alt + 空格」实际变成「右 Alt + 右」并粘键

| 项 | 说明 |
|----|------|
| **现象** | 映射为右 Alt+空格，系统收到右 Alt+右方向，且常一直按下 |
| **根因** | ① 为保实体键盘，曾禁止吞方向键原生 VK；T1 右方向仍经 LL 送达。② WinUHid 注入右 Alt+空格时，与遥控未抬起的「右」在系统层合并 → 表现为 Alt+右。③ 非语音键忽略 Raw Input 抬起，未清吞键 / 未 `release_all`，修饰键易粘住。 |
| **修复** | `T1_HELD_NATIVE`：按钮**有绑定**时（含同键映射）按住期间吞原生，只走注入，避免双发；未绑定透传原生。注入前补一次原生 keyup 中和 LL 先漏；`inject_mapped_keys` / 抬起 / `stop` 均 `release_all`。LL 闸门补映射仅限 Apps/Browser_*，勿对方向 hold 补注入（否则实体同键变映射）。 |
| **代码** | `native_suppress.rs`（hold_suppress）、`runtime.rs` / `ble_keys.rs`（keydown/keyup）、`special_keys.rs` |

**同类风险（已一并覆盖）**：左/上/下/OK 等 `kbd:VK_*` 只要有绑定就按住吞键；未绑定不吞。

---

### B2. 映射目标键导致实体键盘同键失效（空格 / 退格 / 方向等）

| 项 | 说明 |
|----|------|
| **现象** | 主页→空格后实体空格失效；退格未映射也失效；方向键失效 |
| **根因** | 常驻闸门 / `arm` 误含 Backspace、VK_HOME、方向等；LL 无法区分实体与遥控 |
| **修复** | `is_ubiquitous_keyboard_vk` 硬放行；仅允许武装 Apps/Browser_*/音量；删除侧效应不再含 `0x08`；方向原生 VK 不再 TTL arm（改用 B1 按住吞键） |
| **代码** | `native_suppress.rs` |

---

### B3. 语音键右 Alt 单击/长按不发送（对齐 Python T1）

| 项 | 说明 |
|----|------|
| **现象** | 语音映射右 Alt 后，单击/长按都像没注入 |
| **Python 参考** | `t1/app.py`：`T1 only exposes a ~120 ms HID pulse even during a long physical press. Keep the target hotkey held between the first and second voice-button press.`；`audio_trigger_mode=toggle_hotkey` + `second_press_stops`；**忽略** `hid:02-00-00` 抬起。`raw_input_bridge`：**不吞**遥控原生键（additive）。 |
| **根因（Rust）** | 把 HID 全零抬起当成「松手」立刻 `voice_release`，脉冲结束就松开右 Alt；物理长按也不会延长 HID。 |
| **修复** | Hold=闩锁：忽略 HID 抬起；第 1 次脉冲 `press` 并保持，第 2 次脉冲 `release`。Toggle=每次脉冲 `tap`。 |
| **代码** | `runtime.rs` `handle_voice` |

---

### B3b. Python T1 吞键策略说明

Python T1 **默认不吞键盘**（`listen_keyboard: false`，文档写明 additive）。Xiaomi 才用 LL 钩子按 recent 吞固件键。Rust T1 因开启 `listen_keyboard` 做方向映射，另见 B1/B2 的按住吞键与常用键硬放行。

### B4. 菜单键弹出右键菜单、映射不生效

| 项 | 说明 |
|----|------|
| **现象** | 菜单键出系统上下文菜单，映射和弦不执行 |
| **根因** | LL 吞 Apps 后超时「回放」`0x5D`，等于自己弹菜单；Raw Input 按下常缺失 |
| **修复** | 取消超时回放；仅实体键盘 foreign 回放；LL 吞到 Apps 时 `on_ll_gate_keydown` 注入 |
| **代码** | `native_suppress.rs`、`runtime.rs`、`special_keys.rs` |

---

### B5. 主页键开 Edge / 曾无限开窗

| 项 | 说明 |
|----|------|
| **现象** | Home→空格仍开 Edge；曾出现无限 Edge 窗口 |
| **根因** | AC Home 走 APPCOMMAND；WinEvent 在 CREATE 时关空标题 Chromium 窗 → 重建死循环 |
| **修复** | 去掉实时 WinEvent 关窗；改为快照差集最多关 1 次新建顶层窗 |
| **代码** | `native_suppress.rs` |

---

### B6. 语音录入右 Alt 录成左 Alt

| 项 | 说明 |
|----|------|
| **现象** | 录入状态点键盘右 Alt，结果为左 Alt |
| **根因** | `VK_MENU`/`VK_LMENU` + `LLKHF_EXTENDED` 未规范为 `VK_RMENU` |
| **修复** | `canonicalize_side_vk`；`normalize_chord` 优先右 Alt |
| **代码** | `shortcut_capture.rs`、`special_keys.rs` |

---

### B7. 语音唤醒后过一会不收音

| 项 | 说明 |
|----|------|
| **现象** | 点语音键唤醒输入法后，过一段时间不再收音/发送；Windows 录音机同样约 15–16s 后静音 |
| **实测（本机）** | `scripts/probe_t1_mic_rms.py`：`Microphone (Mic Device)` 有声至约 **16.2s**，之后 RMS≈0 且持续静音；仅重开 WASAPI 流**不能**恢复（`probe_t1_mic_reopen.py`） |
| **结论** | **不是桥接 5 分钟逻辑，也不是豆包专有问题**。是 T1 USB「Mic Device」本身在约 15–16s 后停止送出有效音频（固件/USB 音频节能类行为）。 |
| **是否有 T1 时长限制** | Python：`audio_max_hold_ms=300000` 只是软件强制收口。硬件麦有效窗约 **15–16s**，与软件上限无关。 |
| **本桥接侧仍做的** | Hold 闩锁保活；Mic keepalive；尽量清 VID_1915 电源管理（需管理员才写得进注册表）。无法单独「软件续命」过硬件静音窗。 |
| **用户可试** | 管理员运行 `scripts/disable_t1_usb_power_mgmt.ps1` → 拔插接收器；电源计划 USB 选择暂停已禁用时仍可能 16s 静音。长录音需换麦或分段再说。 |

---

## 回归检查清单

- [ ] 语音 → 右 Alt Hold：第一次日志「语音开始（闩锁）」，右 Alt 保持；HID 抬起被忽略；再按一次「语音结束」  
- [ ] 语音 → 右 Alt+空格 Toggle：每次脉冲点按一次  
- [ ] 右 → 右 Alt+空格：只出 Alt+空格，不粘键；实体右方向仍可用（遥控未按住时）  
- [ ] 左/上/下/OK 重映射：无「原生方向 + 映射」粘组合  
- [ ] 主页 → 空格：实体空格可用  
- [ ] 实体退格 / 方向 / 字母：T1 连接后仍可用  
- [ ] 菜单：映射生效，无系统右键菜单（或极少闪一下）  
- [ ] 停止 T1 桥接后无粘键  

---

## 已知取舍

- **身份映射**（如右→右）：不吞原生，可能与注入叠成双击；可改为跳过注入（未做）。  
- **Apps 常驻闸门**：实体「菜单键」极少；若有，靠 Raw Input foreign 回放。  
- **主页 Edge**：APPCOMMAND 无法在无 DLL 下可靠拦截；靠最多关一次新建窗，可能偶发漏关或误关新建标签。
