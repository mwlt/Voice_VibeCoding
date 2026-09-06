# T1 BLE L0 — AC Search 切断（无自签驱动 / 不改 BIOS）

## 做法

Windows 收到的 **AC Search (`0x0221`)** 来自 T1 BLE 的 **Consumer Control** HID 集合
（`UP:000C_U:0001`，设备管理器里一般是 “HID-compliant consumer control device”）。

本 L0 **只禁用该集合**（`pnputil /disable-device`）：

- 系统不再生成 Browser Search / APPCOMMAND_BROWSER_SEARCH
- **键盘 / 鼠标 / 系统控制 / Vendor** 集合保持启用
- 语音仍走 ATVV → VB-CABLE，不依赖 Consumer HID
- **Secure Boot 可保持开启**；只需一次 UAC，不改 BIOS、不开 testsigning

副作用：该遥控器上走 Consumer Page 的媒体键（音量等，若有）会一并失效。
其它电脑键盘不受影响（白名单仅 `01620A/0407`）。

## 命令

```powershell
cd src-tauri\assets\t1_hid_filter
.\install-t1-hid-filter.ps1 -Mode Status
.\install-t1-hid-filter.ps1 -Mode Install    # UAC
.\install-t1-hid-filter.ps1 -Mode Uninstall  # 恢复 Consumer 集合
```

应用内：「自动修复 Search 剥离」调用同一脚本。

## 已放弃

自研 `t1blehidf.sys` + 测试签名：在 Secure Boot 下无法让普通用户启用，**不作为产品方案**。
若将来需要「禁用集合但不丢音量键」的精细剥离，需 **微软 Attestation 签名** 的正式过滤驱动。
