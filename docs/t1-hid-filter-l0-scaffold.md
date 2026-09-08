# T1(蓝牙) L0 — Consumer 集合策略（无法单键 pnputil 剥离）

> **仅 T1(蓝牙)**。T1(USB) 不使用本包。见 [T1_USB_BLE_SPLIT.md](./T1_USB_BLE_SPLIT.md)。

## 为什么不能「只剥 AC Search」

Windows `pnputil /disable-device` 只能禁用**整个** HID 集合（Consumer Control `UP:000C`），
**不能**只关掉 Usage `0x0221`（AC Search）。

若整集禁用，同集合上的键会一并消失：

- AC Search（目标）
- AC Home / AC Back（**Home / 删除映射会完全无响应**）
- Volume± / Mute 等 Consumer 媒体键

要「只剥指定 Usage」需要 **按设备 HID 过滤驱动**（读完成里 zero 指定字节），且需微软 Attestation 签名才能在 Secure Boot 下给普通用户用。自签 `.sys` 已放弃。

## 现行做法

| 层 | 作用 |
|----|------|
| **L0（本包）** | 保证 T1 Consumer **启用**；若旧版曾整集禁用 →「自动修复」= `pnputil /enable-device` |
| **L1** | LL + HotKey + `t1_shell_hook` 吞 `0xAA` / APPCOMMAND Search |
| **L2/L3** | Raw 武装 + 搜索 UI mop-up |

Home / 删除 / 音量继续走 Consumer HID → 应用映射，与 L1 吞搜索并行。

## 命令

```powershell
cd src-tauri\assets\t1_hid_filter
.\install-t1-hid-filter.ps1 -Mode Status   # READY = 已找到且全部启用
.\install-t1-hid-filter.ps1 -Mode Install  # Enable 残留禁用实例（UAC）
.\install-t1-hid-filter.ps1 -Mode Uninstall  # 同上 Enable
```

## 自动修复

启动流水线 / BLE 连接 / 看门狗：若 Status=`NOT_BOUND`（仍有禁用实例）→ Enable。
设置页按钮可手动触发。
