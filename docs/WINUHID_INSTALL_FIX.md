# WinUHid 安装脚本修复（v1.5.1+）

> 跟踪「修复虚拟键盘」在 phantom 根设备上的跨机器可靠性改进。

## 问题摘要

`Root\WinUHid` 为 phantom 根设备：`DIF_REGISTERDEVICE` 后节点存在但未必 **Started**。
旧脚本在无 devcon 时调用 `UpdateDriverForPlugAndPlayDevices` 会失败（0xE000020B），
且在绑定驱动前缺少 `pnputil /scan-devices`，导致 `\\.\WinUHid` 不可达。

## 落地后的 canonical 流程

| 阶段 | 动作 | 实现位置 |
|------|------|----------|
| Prepare | 安装证书 + 部署 DLL | `Install-PublisherCert` / `Deploy-UserDll` |
| StageDriver | `pnputil /add-driver inf`（仅进驱动存储） | `Invoke-PnputilPhase StageDriver` |
| RegisterRoot | 创建 `Root\WinUHid` 节点（仅 `DIF_REGISTERDEVICE`） | `Register-RootDeviceNode` |
| BindDriver | `pnputil /add-driver inf /install` | `Bind-AndPresentRootDevice` |
| ScanDevices | `pnputil /scan-devices` | `Bind-AndPresentRootDevice` |
| Verify | 轮询 `\\.\WinUHid` | `Wait-WinUHidReady` |

可选加速：若本机有 WDK `devcon.exe`，RegisterRoot 阶段先尝试 `devcon install`；
失败则回退 SetupAPI，**不再**调用 `UpdateDriverForPlugAndPlayDevices`。

## 验证状态（按实际执行标记）

| 检查项 | 状态 | 说明 |
|--------|------|------|
| `install-winuhid.ps1` 重构落地 | ✅ 已完成 | `src-tauri/assets/winuhid/install-winuhid.ps1` |
| 移除 `UpdateDriverForPlugAndPlayDevices` | ✅ 已完成 | 源码与自动化测试已确认 |
| 分阶段日志 `Phase: …` | ✅ 已完成 | 脚本 + `winuhid_env.rs` 解析最后阶段 |
| 自动化测试 `scripts/test-winuhid-install.ps1` | ✅ 已通过 | 语法 / 结构 / Status 模式 |
| Rust `cargo check` | ✅ 已通过 | `winuhid_env.rs` 编译通过 |
| Status 模式（本机） | ✅ 已通过 | `Phase: Verify \| device reachable` / `Result: OK` |
| InstallElevated 全路径（无 devcon 干净 VM） | ⏳ 未在本会话验证 | 需无 WDK devcon 的干净 Win10/11 VM + UAC |
| InstallElevated 全路径（本机提权） | ⏳ 未在本会话验证 | 当前 shell 非管理员，未触发 UAC 实测 |

## 如何复测

```powershell
# 无需管理员
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-winuhid-install.ps1

# 查看设备是否可达
powershell -NoProfile -ExecutionPolicy Bypass -File src-tauri/assets/winuhid/install-winuhid.ps1 `
  -Mode Status -PackageDir src-tauri/assets/winuhid/driver -DllSource src-tauri/assets/winuhid/WinUHid.dll
```

在无 devcon 的 VM 上完整验证：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File src-tauri/assets/winuhid/install-winuhid.ps1 `
  -Mode Install -PackageDir src-tauri/assets/winuhid/driver -DllSource src-tauri/assets/winuhid/WinUHid.dll
```

应用内：小米设置页 → **修复虚拟键盘**（等价于 `-Mode Install`）。

## 相关文件

- 正式脚本：`src-tauri/assets/winuhid/install-winuhid.ps1`
- Rust 调用：`src-tauri/src/bridges/xiaomi/winuhid_env.rs`
- 参考草稿（已 superseded）：`docs/install-winuhid(1).ps1`
