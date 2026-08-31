# 语音唤醒延迟优化计划

> 目标：缩短「按下语音键 → 输入法收到快捷键」的路径延迟。  
> 方法：TDD。**标记只据自动测试实测，禁止虚标。**

## 背景

旧路径：`0x04` → `ensure_pcm_ready_on_press`（可能同步堵）→ `bump_hook_to_front_and_settle(40)` → WinUHid 注入。  
新路径：`0x04` → ShortcutDown（含 bump settle ≤8ms）→ 再 `ensure_pcm_ready_on_press` → CLEAR。  
微信「F5 + App 映射」时，长 bump 等待收益小；PCM 更不应挡在注入前。

## 测试缝

| # | 缝 | 断言 |
|---|-----|------|
| W1 | `voice_remote_press_steps` + `on_voice_remote_press` 源码顺序 | **ShortcutDown 先于 EnsurePcmReady** |
| W2 | `handle_voice` bump settle | `VOICE_BUMP_SETTLE_MS` ≤10（现 8） |
| W3 | 回归 | `voice_first_packet` + `voice_f5_*` / chord / hook_bump |

## 步骤

| # | 步骤 | 状态 | 测试 | 实测 |
|---|------|------|------|------|
| 0 | 本计划 | ✅ 完成 | 本文存在 | 文档已写入 |
| 1 | 注入先于 PCM ensure | ✅ 完成 | `voice_first_packet` | **5 passed**（含 W1） |
| 2 | 缩短 bump settle | ✅ 完成 | `voice_wake_latency` | **2 passed**（settle=8） |
| 3 | 回归 + 文档勾选 | ✅ 完成 | 见下 | **66 passed**（`--test-threads=1`） |

### Step 3 回归命令与计数（2026-08-31 实测）

```text
cargo test --test hook_bump_to_front --test voice_f5_home_like \
  --test voice_f5_suppress_semantics --test voice_f5_correlate \
  --test voice_chord_sanitize --test voice_first_packet \
  --test voice_wake_latency -- --test-threads=1
```

| 二进制 | 通过 |
|--------|------|
| hook_bump_to_front | 11 |
| voice_chord_sanitize | 5 |
| voice_f5_correlate | 7 |
| voice_f5_home_like | 21 |
| voice_f5_suppress_semantics | 15 |
| voice_first_packet | 5 |
| voice_wake_latency | 2 |
| **合计** | **66** |

说明：同二进制内共享全局原子状态时，多线程并行偶发 flake（与本改动无关）；回归以 `--test-threads=1` 为准。

## 落地变更摘要

1. `voice_press::voice_remote_press_steps`：`ShortcutDown` 移到 `EnsurePcmReady` 之前。  
2. `input_session::on_voice_remote_press`：先 `on_remote_button`，再 `ensure_pcm_ready_on_press`。  
3. `key_mapping::VOICE_BUMP_SETTLE_MS = 8`；`handle_voice` 用 `voice_bump_settle_ms()` 替代硬编码 40。

## 明确不做

- 不改 BLE/固件时序  
- 不删除 bump（仍请求置顶，只是少等）  
- 不恢复 vkE8  
- 未做实机毫秒级计时（仅自动测试契约）；实机延迟需另测

## 变更日志

- 2026-08-31：建立本计划；Step 0–3 按 TDD 落地；回归 66 passed（serial）。
