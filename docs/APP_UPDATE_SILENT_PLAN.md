# 应用内静默升级（卸旧装新 · 保留用户数据）

> 目标：点「下载并更新」后尽量零交互 → 关旧进程 → 卸旧程序（保留 AppData）→ 静默装新版 → 桌面图标 → 自动打开新版。  
> 例外：若安装目录需管理员，Windows 仍可能弹出 UAC。  
> 进度：仅显示一个无按钮的升级进度窗（非 MessageBox / 非 cmd 黑框）。

## 用户确认的完整流程

1. 点击「下载并更新」并完成下载  
2. 关闭正在运行的旧版  
3. **卸载旧版程序**（`/S /UPDATE`：不删用户数据）  
4. **静默安装新版**（`/S`）  
5. **默认建桌面图标**（Tauri NSIS：Silent 下自动 `CreateOrUpdateDesktopShortcut`）  
6. **自动打开新版**（`/R`；若未拉起则回退启动 `Program Files\...\remote-bridge-hub.exe`）

## 依据（Tauri NSIS `installer.nsi`）

| 参数 | 作用 |
|------|------|
| `/S` | 静默安装/卸载 |
| `/R` | 静默/被动安装成功后启动主程序（必须带，否则不自动打开） |
| `/UPDATE` | 更新模式：卸载时**不删** `$APPDATA\$BUNDLEID` |

本仓库 `productName`：`Voice VibeCoding`；`identifier`：`com.remote-bridge-hub.app`（用户数据目录）。

## TDD 测试缝（Seams）

1. `silent_install_args()` → `/S` + `/R`  
2. `silent_uninstall_keep_data_args()` → `/S` + `/UPDATE`  
3. `build_silent_upgrade_ps1(...)` → `Wait-Process` → 卸旧 → 装新；**无** `timeout` / `start /wait`；含 Hidden 与进度窗  
4. `silent_upgrade_powershell_args(...)` → `-WindowStyle Hidden` + `-File`  
5. `parse_uninstall_exe_path(...)` → 解析注册表 UninstallString  
6. 下载完成路径调用 `launch_silent_upgrade` + `app.exit(0)`（行为由编排保证；真机安装包冒烟另测）

自动化：`cargo test --test app_update_silent`

## 实施步骤与完成标记

| Step | 内容 | 状态 |
|------|------|------|
| 0 | 本文档与缝约定 | ✅ |
| 1 | 静默参数纯函数 + 批处理编排 + `app_update_silent` 测试 | ✅ 初版 2026-08-31 |
| 2 | 注册表查找 uninstall.exe；下载完成后调度并 `app.exit(0)` | ✅ |
| 3 | UI 文案改为静默升级说明 | ✅ |
| 4 | NSIS `displayLanguageSelector: false` | ✅ |
| 5 | 真机冒烟（点下载并安装 → UAC 后自动进新版） | ⬜ 需用含本改动的安装包验证 |
| 6 | **修复黑框 / 升级失败**：改 PowerShell Hidden + 进度窗（替代 cmd/timeout/start） | ✅ `cargo test --test app_update_silent` 7 passed（2026-09-01） |

## 运行时编排（落地实现 · v1.6.4+）

1. 下载 setup.exe 到配置目录 `updates/`  
2. `find_installed_uninstall_exe()`（HKCU/HKLM Uninstall，DisplayName=`Voice VibeCoding`）  
3. 写 `%TEMP%\voice_vibecoding_silent_upgrade_<pid>.ps1`（UTF-8 BOM）  
4. `powershell.exe -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File …` + `CREATE_NO_WINDOW`（**不用** DETACHED_PROCESS / cmd 黑框）  
5. 脚本：`Wait-Process` → 清残留 `remote-bridge-hub` → 进度窗 → `Start-Process -WindowStyle Hidden` 卸旧 `/S /UPDATE` → 装新 `/S /R` → 必要时回退启动 exe  
6. 主进程 `app.exit(0)`  
7. 日志：`%TEMP%\voice_vibecoding_silent_upgrade.log`

## 注意

- `update/latest.json` 指向正式 `*_x64-setup.exe`。  
- **v1.6.2 / v1.6.3** 内置的是旧 cmd 编排（易闪黑框、升级可能失败）；需**手动装一次 1.6.4+**，之后应用内升级才走本修复。  
- 勿对卸载去掉 `/UPDATE`，否则可能清掉 AppData。
