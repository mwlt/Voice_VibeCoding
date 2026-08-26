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
3. 「修复虚拟键盘」→ UAC → 安装证书 + pnputil + 创建 `Root\WinUHid`
4. 语音和弦注入 **仅** 走 WinUHid；不可用时写错误日志并提示修复

## 重新编译驱动

需要 WDK + VS Build Tools。见仓库 `_third_party/wdk/` 与 `scripts/build-winuhid.ps1`（若有）。
源码：https://github.com/cgutman/WinUHid
