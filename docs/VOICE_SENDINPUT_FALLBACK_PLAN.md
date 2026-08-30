# 语音注入：WinUHid 优先 + SendInput 降级（TDD）

> 目标：虚拟键盘可用时**始终** WinUHid；检测失效时**互斥**降级 SendInput（1.3.15 行为），并明确提示降级。  
> 方法：每步 = 红 → 绿 → 检查/修正 → 更新本文（**仅据实测勾选**）。

## 原则

```
按下/抬起同一次会话只用一条后端（互斥，禁止双发）
  ├─ WinUHid available → VirtualHid（press_single / release_single）
  └─ unavailable       → SendInputFallback（整段 DOWN+UP；豆包/千问可能无效）
```

- **禁止**：同一次 DOWN 同时 WinUHid + SendInput  
- **禁止**：DOWN 用 A、UP 用 B（以后端在 DOWN 时锁定为准）  
- SendInput 仍可用于：清键 sanitizer、shell-menu `0xE8` 哑键（与唤醒路径分离）

## 测试缝

| # | Seam | 断言 |
|---|------|------|
| F1 | `voice_inject_backend(available)` | true→VirtualHid；false→SendInputFallback |
| F2 | `inject_voice_chord` 源码/行为 | available 分支含 press_single；unavailable 分支含 SendInput 且**无**双发；有降级日志 |
| F3 | DOWN 锁定后端 | UP 与 DOWN 同后端（`VOICE_INJECT_BACKEND_HELD`） |
| F4 | 文档与旧测 | 旧「必须 BLOCKED」测改为「降级」 |

## 步骤

| 步骤 | 内容 | 状态 | 验证 |
|------|------|------|------|
| 0 | 本计划 + 缝 | ✅ | 本文存在 |
| 1 | 纯函数路由 F1 | ✅ | `ime_voice_wake_route` → **4 ok**（2026-08-30） |
| 2 | 接线 `inject_voice_chord` + 后端锁定 F2/F3 | ✅ | `voice_f5_home_like` → **17 ok**；`voice_chord_and_release` → **8 ok** |
| 3 | 降级日志 + 节流通知 + period 清锁定 | ✅ | `DEGRADED SendInput`；`notify_voice_sendinput_degraded`；`end_voice_period` 清 backend |
| 4 | 更新冲突文档/README；回归 | ✅ | README + `VOICE_CHORD_RELEASE_PLAN`；同日三测 **4+8+17 ok** |

## 落地摘要

- `voice_inject.rs`：`VoiceInjectBackend` + `voice_inject_backend(vks, available)`
- `key_mapping.rs`：`inject_voice_chord` 按后端互斥；DOWN 锁定；降级 warn + 通知
- 实机：WinUHid 正常应仍见 `inject via WinUHid`；拔驱动/检测失败应见 `DEGRADED SendInput`

## 相关

- `docs/VOICE_CHORD_RELEASE_PLAN.md`
- `voice_inject.rs` / `key_mapping.rs` / `ime_voice_wake_route.rs`
