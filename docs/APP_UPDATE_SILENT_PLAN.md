# 应用内静默升级（卸旧装新 · 保留用户数据）

> 目标：点「下载并更新」后尽量零交互 → 关旧进程 → 卸旧程序（保留 AppData）→ 静默装新版 → 桌面图标 → 自动打开新版。  
> 例外：若安装目录需管理员，Windows 仍可能弹出 UAC。

## 用户确认的完整流程

1. 点击「下载并更新」并完成下载  
2. 关闭正在运行的旧版  
3. **卸载旧版程序**（`/S /UPDATE`：不删用户数据）  
4. **静默安装新版**（`/S`）  
5. **默认建桌面图标**（Tauri NSIS：Silent 下自动 `CreateOrUpdateDesktopShortcut`）  
6. **自动打开新版**（`/R`）

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
3. `build_silent_upgrade_batch(...)` → 等 PID → 卸旧 → 装新  
4. `parse_uninstall_exe_path(...)` → 解析注册表 UninstallString  
5. 下载完成路径调用 `launch_silent_upgrade` + `app.exit(0)`（行为由编排保证；真机安装包冒烟另测）

自动化：`cargo test --test app_update_silent`

## 实施步骤与完成标记

| Step | 内容 | 状态 |
|------|------|------|
| 0 | 本文档与缝约定 | ✅ |
| 1 | 静默参数纯函数 + 批处理编排 + `app_update_silent` 测试 | ✅ `cargo test --test app_update_silent` 5 passed（2026-08-31） |
| 2 | 注册表查找 uninstall.exe；下载完成后调度批处理并 `app.exit(0)` | ✅ 代码已落地于 `src-tauri/src/app_update.rs` |
| 3 | UI 文案改为静默升级说明 | ✅ `appUpdate.ts` / `AppUpdateModal.vue` |
| 4 | NSIS `displayLanguageSelector: false`（减少手工安装打扰；静默本身跳过语言页） | ✅ `tauri.conf.json`（需**重新打包**后装机才生效） |
| 5 | 真机冒烟（点下载并安装 → UAC 后自动进新版） | ⬜ 需用含本改动的安装包验证；当前仓库代码已具备编排 |

## 运行时编排（落地实现）

1. 下载 setup.exe 到配置目录 `updates/`  
2. `find_installed_uninstall_exe()`（HKCU/HKLM Uninstall，DisplayName=`Voice VibeCoding`）  
3. 写 `%TEMP%\voice_vibecoding_silent_upgrade_<pid>.cmd`  
4. `cmd /C` 分离启动（无黑框）  
5. 批处理：轮询等 PID 退出 → `uninstall.exe /S /UPDATE`（若存在）→ `setup.exe /S /R`  
6. 主进程 `app.exit(0)`

开发态未安装正式版时：跳过卸载，仅静默装（若用户选择了可写安装路径/有权限）。

## 注意

- **已发布的 v1.6.1 安装包不含本逻辑**；用户需先装上含本改动的版本后，「下载并安装」才会静默。首次升级到该版本仍可能走旧交互。  
- 桌面图标依赖 NSIS Silent 分支，无需额外勾选。  
- 勿对卸载去掉 `/UPDATE`，否则可能清掉 AppData。
