# T1(蓝牙) / T1(USB) 产品拆分

> 长期契约：双产品入口、双配置、一份进程级吞键；T1 不直接依赖 `bridges::xiaomi`。

## 产品面

| 侧栏 | 路由 | BridgeType | 配置文件 |
|------|------|------------|----------|
| T1(蓝牙) | `/t1-ble` | `t1_ble` | `%APPDATA%\RemoteBridgeHub\t1_ble.json` |
| T1(USB) | `/t1-usb` | `t1_usb` | `%APPDATA%\RemoteBridgeHub\t1_usb.json` |

- 侧栏顺序：小米 → **T1(蓝牙)** → **T1(USB)** → …
- 旧路由 `/t1` → 重定向到 `/t1-ble`
- 旧 `t1.json`：首次启动若新文件缺失则 **复制到两份**；之后两边独立修改，不再双向同步。`t1.json` 保留作只读备份（废弃）。

## 运行时

| | T1(蓝牙) | T1(USB) |
|--|----------|---------|
| 模块 | `bridges/t1_ble` | `bridges/t1_usb` |
| 设备 | HOGP `01620A/0407` + ATVV | 接收器 `1915/1025` + Mic Device |
| 启停 | `start_t1_ble_bridge` | `start_bridge("t1_usb")` |
| 事件 | `t1-ble` | `t1-key` |
| L0 Consumer 修复 | 仅蓝牙 | 无 |

**允许同时连接。**

## 必要共享（`bridges/shared` + 吞键）

| 组件 | 说明 |
|------|------|
| `native_suppress` | 单进程一份 `WH_KEYBOARD_LL` / HotKey；用 **原因位** 启停（`UsbMap` / `BleMap` / `BleVoiceSearch` / …），业务只 `arm_reason` / `disarm_reason`，不写对端是否存活 |
| WinUHid / 注入 | `shared::hid_injector` 等；小米经 shared 使用，T1 **禁止** `use bridges::xiaomi` |
| `voice_gain` | `shared::voice_gain`；各读各 config 的 `gain_db` |
| Raw Input 观察壳 | 可共用；设备匹配与 mapping 表分传输 |

## 开发指引

- 改蓝牙映射 / 语音 / 增益 → `t1_ble.json` + `t1_ble` 设置页  
- 改 USB 映射 / Mic → `t1_usb.json` + `t1_usb` 设置页  
- 改吞 Browser Search / LL → `native_suppress`（shared 或 t1 公共层），勿在 usb/ble 各装一套钩子  

## 验证清单

- [ ] 侧栏：小米 → T1(蓝牙) → T1(USB)
- [ ] 旧 `/t1` 打开后落到 `/t1-ble`
- [ ] 仅 USB 连接：映射读写 `t1_usb.json`
- [ ] 仅 BLE 连接：映射读写 `t1_ble.json`；两边改映射互不影响
- [ ] USB+BLE 同时开；停一侧后另一侧吞键仍有效
- [ ] 旧 `t1.json` 首次启动复制到两份
- [ ] 托盘：T1(蓝牙)/T1(USB) 分项连接
