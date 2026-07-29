# Audio lifecycle A/B（audiodg 句柄）

约定（grilling 拍板）：

- 硬门槛：空闲 `|rate| < 5/s`
- 平局优先：`hold_device` > `always_play` > `deferred`
- **本机结论（2026-07-29）：三档均过线 → 默认锁死 `hold_device`**

| 档 | 冷启动空闲 | 说完再空闲 |
|----|------------|------------|
| always_play | 0/s | n/a |
| hold_device | 0/s | 0/s |
| deferred | 0/s | ~-0.1/s |

## 环境变量

| 变量 | 作用 |
|------|------|
| `REMOTE_BRIDGE_AUDIO_LIFECYCLE` | `always_play` \| `hold_device`（默认） \| `deferred` |
| `REMOTE_BRIDGE_CABLE_PROBE_TTL_MS` | **未就绪**重试间隔（默认 60000）；`0`=未就绪也不自动重试；已就绪始终停探 |

## 回归步骤

```powershell
$env:REMOTE_BRIDGE_CABLE_PROBE_TTL_MS = "0"
$env:REMOTE_BRIDGE_AUDIO_LIFECYCLE = "hold_device"   # 或 always_play / deferred
npm run tauri:dev
# 日志确认 lifecycle=...
# 空闲跑 scripts\compare_audiodg_handles.ps1
# hold_device/deferred 再：说话 → 松开 → 等≥2s → 再跑一轮
```
