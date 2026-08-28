# WebView2 白屏/黑屏恢复方案

> 最后更新：2026-08-27  
> 关联代码：`webview_guard.rs`、`webview_recovery.rs`、`lib.rs`、`ipc/tray.rs`

---

## 症状

- 界面全白或全黑，但**语音、按键映射、桥接功能仍正常**
- 托盘「刷新界面」无效
- 任务管理器中有 `remote-bridge-hub.exe`，但**可能没有**属于本应用的 `msedgewebview2.exe`
- 日志大量 `HRESULT(0x8007139F)`（`ERROR_INVALID_STATE`）

## 根因

Rust 主进程与音频子进程存活，WebView2 **渲染进程已死亡**进入僵尸态。  
`window.reload()` 只能重载 URL，**无法复活已死的 msedgewebview2 进程**。

长期 `window.hide()` 关闭到托盘时，Windows 可能回收隐藏窗口的 WebView2 渲染资源，加速此问题。

---

## 日志位置（Windows）

```
%APPDATA%\com.remote-bridge-hub.app\logs\app.log
```

示例：`C:\Users\<用户名>\AppData\Roaming\com.remote-bridge-hub.app\logs\app.log`

应用内：**设置 → 打开日志目录**（IPC `open_logs_folder`）。

关键日志关键词：

| 关键词 | 含义 |
|--------|------|
| `WEBVIEW GUARD: reloading` | 守卫检测到渲染异常，尝试 reload |
| `failed to reload: WebView2 error` | reload 失败（僵尸态） |
| `WEBVIEW GUARD: recreating` | 升级 destroy + 重建窗口 |
| `WEBVIEW RECOVERY: main window recreated` | recreate 成功 |
| `TRAY: restarting application` | 用户触发「重启软件」 |

---

## 三级恢复 ladder

| 级别 | 手段 | 触发条件 |
|------|------|----------|
| L0 预防 | 关闭 / 启动进托盘 → `minimize()` + `set_skip_taskbar(true)`，**禁止 `hide()`** | 用户点关闭且「最小化到托盘」开启；或「启动后最小化到托盘」/`--minimized` |
| L1 轻量 | `window.reload()` | 心跳超时 / 启动宽限期无 pong |
| L2 重建 | destroy + `WebviewWindowBuilder` 重建 | reload 连续失败 ≥2 次 |
| L3 重启 | `app.restart()` 整进程 relaunch | 托盘「重启软件」；L2 仍失败时用户手动 |

---

## 守卫参数

| 常量 | 值 | 说明 |
|------|-----|------|
| `STALE_AFTER` | 15s | 无心跳视为疑似死亡 |
| `FAIL_THRESHOLD` | 3 | 连续 3 次检查后才干预 |
| `RELOAD_COOLDOWN` | 30s | reload 冷却 |
| `RELOAD_FAIL_THRESHOLD` | 2 | reload 失败 2 次升级 recreate |
| `RECREATE_COOLDOWN` | 60s | recreate 冷却 |
| `FIRST_PONG_GRACE` | 45s | 启动后窗口可见但 JS 未就绪的宽限期 |

前端：`App.vue` 每 5s 调用 `webview_ping`；`onMounted` 后 `getCurrentWindow().show()`（配合 `visible:false` 防白屏闪烁）。

---

## 托盘菜单

1. **刷新界面（白屏自救）** — reload，失败则立即 recreate  
2. **重启软件** — 清理桥接/HID/音频后 relaunch（L3）

---

## 真机验证清单

- [ ] 关闭到任务栏，长时间后再打开：界面正常
- [ ] 模拟黑屏：守卫日志出现 recreate 且界面恢复
- [ ] 托盘「刷新界面」：reload 失败时能 recreate 恢复
- [ ] 托盘「重启软件」：进程重启且功能正常
- [ ] 开机自启：无白屏，页面就绪后窗口出现
- [ ] 黑屏时后端（语音/按键）仍可用，恢复后 UI 与后端状态一致

---

## 改动文件

| 文件 | 职责 |
|------|------|
| `src-tauri/src/webview_guard.rs` | 心跳状态机、Grace、Reload/Recreate 判定 |
| `src-tauri/src/webview_recovery.rs` | reload / recreate / restart 执行 |
| `src-tauri/src/lib.rs` | 守卫线程、关闭 minimize、自启错峰 |
| `src-tauri/src/ipc/tray.rs` | 托盘刷新 + 重启软件 |
| `src/App.vue` | 心跳 + 就绪后 show |
| `src-tauri/tauri.conf.json` | 主窗口 `visible: false` |
