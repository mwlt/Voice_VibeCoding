# WinUHid（虚拟键盘）内嵌资源

语音键唤醒豆包/千问等输入法需要 **WinUHid.dll + UMDF 驱动**（硬件级 HID）。
SendInput 会被过滤，不能作为修复方案。

## 目录

- `WinUHid.dll` — 用户态 SDK（已内嵌）
- `WinUHidPublisher.cer` — 驱动包签名证书（安装时写入 TrustedPublisher）
- `install-winuhid.ps1` — 提权安装脚本（应用「修复虚拟键盘」调用）
- `driver/`
  - `WinUHidDriver.dll`
  - `WinUHidDriver.inf`（已 stampinf）
  - `WinUHidDriver.cat`

## 应用行为

1. 启动时自动把 DLL 部署到 exe 旁 / LocalAppData
2. 尝试打开 `\\.\WinUHid`；失败则状态显示「虚拟键盘未就绪」
3. 「修复虚拟键盘」→ UAC → 按 **Stage → Register → Bind → Scan → Verify** 安装（见下）
4. 语音和弦注入 **仅** 走 WinUHid；不可用时写错误日志并提示修复
5. UP 后 **Release Sanitizer**：必发全零 HID 报告；若 Win/Ctrl 仍 down 则 SendInput 仅 KEYUP（不作唤醒兜底）

## 安装流程（v1.5.1+ canonical）

`install-winuhid.ps1` 的 `InstallElevated` 顺序：

1. **Prepare** — 安装发布者证书、部署 `WinUHid.dll`
2. **StageDriver** — `pnputil /add-driver WinUHidDriver.inf`（驱动进存储）
3. **RegisterRoot** — 创建 `Root\WinUHid` 节点（SetupAPI `DIF_REGISTERDEVICE`；可选 devcon 加速）
4. **BindDriver** — `pnputil /add-driver … /install`（绑定驱动到节点）
5. **ScanDevices** — `pnputil /scan-devices`（phantom 节点 present 并启动）
6. **Verify** — 轮询 `\\.\WinUHid`；仍不可达则 exit 3010（需重启）

脚本输出 `Phase: …` 行，应用日志可看到失败阶段。详见 [`docs/WINUHID_INSTALL_FIX.md`](../../../docs/WINUHID_INSTALL_FIX.md)。

## 测试

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-winuhid-install.ps1
```

## 重新编译驱动

需要 WDK + VS Build Tools。见仓库 `_third_party/wdk/` 与 `scripts/build-winuhid.ps1`（若有）。
源码：https://github.com/cgutman/WinUHid
