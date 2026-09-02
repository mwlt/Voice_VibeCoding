# 开机自启与启动进托盘

> 最后更新：2026-09-02  
> 关联代码：`bridges/xiaomi/autostart.rs`、`lib.rs`、`webview_recovery.rs`

---

## 用户设置（全局设置）

| 开关 | 作用 |
|------|------|
| **开机自启** | 登录时自动运行；写入 `HKCU\...\Run\RemoteBridgeHub`，命令行带 `--minimized`（仅用于单实例去重，**不**单独决定是否进托盘） |
| **启动后最小化到托盘** | **唯一**控制启动是否进托盘：手动打开 / 开机自启 / 再次点图标，均受此开关约束 |
| **最小化到托盘** | 点关闭按钮时进托盘（非开机行为） |

推荐组合：三个都开 → 登录静默在托盘，手动打开也静默，关窗不退出。

---

## 进托盘机制

**仅**当 `start_minimized_to_tray == true`（读 `settings.json`）时 `boot_to_tray = true`。

命令行 `--minimized`（自启 Run 项携带）**不再**触发进托盘，只用于单实例：重复的自启进程静默忽略，避免误 restore。

| 启动后最小化到托盘 | 手动/自启冷启动 | 已在跑时再点图标 |
|------------------|----------------|-----------------|
| 关 | 显示窗口 | 显示窗口 |
| 开 | 进托盘 | 保持/回到托盘 |

执行：`show()` → `minimize()` → `set_skip_taskbar(true)`。**禁止 `hide()`**（易导致 WebView2 白屏，见 [WEBVIEW_RECOVERY.md](./WEBVIEW_RECOVERY.md)）。

前端 `App.vue` 就绪后调用 `reveal_main_on_frontend_ready`；若 `boot_to_tray` 则再次 `minimize_main_to_tray`。后端另有 800ms 兜底线程。

---

## 自启只保留一处入口（v1.6.6+）

**旧版问题**：同时写 Run 注册表 + `Startup\RemoteBridgeHub.lnk`，登录时 Windows 常各拉起一次。第二次进程命中单实例后 **无条件 `restore_main_window`**，把已进托盘的窗口又亮出来。

**现行策略**：

| 项 | 做法 |
|----|------|
| 写入 | 仅 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\RemoteBridgeHub` |
| 清理 | 启用/禁用自启时删除遗留 `Startup\RemoteBridgeHub.lnk` |
| 迁移 | 每次应用启动 `reconcile_autostart_entries(settings.autostart)`：Run+lnk 并存 → 删 lnk；仅 lnk → 迁移到 Run；**settings 已关自启但注册表仍在 → 删 Run+lnk** |

---

## 单实例与 `--minimized`

```text
第二次启动
  ├─ 命令行含 --minimized → 静默忽略（重复自启，保持托盘）
  ├─ 不含 --minimized + start_minimized_to_tray=true → 保持/回到托盘
  └─ 不含 --minimized + start_minimized_to_tray=false → restore_main_window
```

托盘左键 / 菜单「打开状态」仍可主动弹出窗口（`restore_main_window`）。

实现：`lib.rs` 中 `should_ignore_second_instance(&args)`。

---

## WebView 恢复与托盘

`recreate_main_window` / `try_reload_main` 在 **`prefer_stay_in_tray`**（`boot_to_tray` 或 `session_in_tray`）为 true 时，恢复后再次 `minimize_main_to_tray`，避免守卫重建窗口时弹出任务栏。

- **`boot_to_tray`**：本次启动是否应进托盘（仅 `start_minimized_to_tray`）；用于启动 reveal 与 800ms 兜底。
- **`session_in_tray`**：用户已进入托盘（含关窗进托盘）；WebView reload/recreate 后保持托盘。

用户通过托盘左键、菜单「打开状态」调用 `restore_main_window` 后，会清除上述两标志，此后 WebView 恢复/刷新界面不会再自动缩回托盘（除非用户再次关窗进托盘）。

800ms 启动兜底线程也会在 `boot_to_tray == false` 时跳过 minimize，避免用户在启动后 800ms 内打开窗口又被压回托盘。

---

## 日志关键词

| 关键词 | 含义 |
|--------|------|
| `START: start_minimized_to_tray=true` | 本次启动策略为进托盘 |
| `WINDOW: minimized to tray` | 已 minimize + skip_taskbar |
| `autostart: removed duplicate Startup shortcut` | 去重清理旧快捷方式 |
| `autostart: migrated legacy Startup shortcut` | 旧仅-lnk 用户迁移到 Run |
| `single-instance: duplicate --minimized launch ignored` | 重复自启被忽略 |

---

## 真机验证清单

- [ ] **关**「启动后最小化到托盘」：双击图标 → 显示窗口
- [ ] **关** + 开机自启：登录 → 显示窗口
- [ ] **开**「启动后最小化到托盘」：双击图标 → 仅托盘，无任务栏
- [ ] **开** + 开机自启：登录 → 仅托盘
- [ ] **开** + 托盘已在跑时再点桌面图标 → 保持托盘（不弹窗）
- [ ] **关** + 托盘已在跑时再点桌面图标 → 弹出窗口
- [ ] 点托盘左键 → 无论开关均弹出窗口
- [ ] `%APPDATA%\...\Startup\RemoteBridgeHub.lnk` 不存在（或启动后被删）
- [ ] Run 注册表项含 `--minimized`（单实例去重用）
- [ ] 关窗（最小化到托盘开）→ 托盘在，任务栏无按钮

---

## 改动文件

| 文件 | 职责 |
|------|------|
| `src-tauri/src/bridges/xiaomi/autostart.rs` | Run 自启、遗留 lnk 清理与迁移 |
| `src-tauri/src/config/manager.rs` | 读 `start_minimized_to_tray`（含磁盘回退） |
| `src-tauri/src/lib.rs` | 单实例、`reconcile`、冷启动入口 |
| `src-tauri/src/webview_recovery.rs` | 进托盘/还原/单实例/冷启动/WebView 恢复 |
| `src-tauri/src/ipc/commands.rs` | 保存全局设置（含自启注册表同步） |
| `src/App.vue` | 前端就绪后 `reveal_main_on_frontend_ready` |
