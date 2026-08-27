# 语音首包延迟优化 — TDD 实施计划

> 目标：按住语音键 → **更快**唤起输入法 + **更早**向 VB-CABLE 送首帧 PCM。  
> 方法：TDD 垂直切片；每步测试通过后再标 ✅。  
> 不可压缩：BLE 传输、ADPCM 攒满 `frame_size`（固件/协议下限）。

## 当前瓶颈（按优先级）

| # | 瓶颈 | 优化方向 |
| --- | --- | --- |
| B1 | 按下时 PCM 未就绪，仅 `warmup_async` | 按下路径 **同步** `ensure_started`，失败再 async |
| B2 | PING 重试间隔 50ms | 改为 **15ms** |
| B3 | 先 `CLEAR` 再快捷键 DOWN | **先 DOWN 再 CLEAR**（IME 先开，声卡后开流） |
| B4 | `deferred` 生命周期冷启动 | 文档强调默认 `hold_device`（不改默认） |

## TDD 测试缝

| # | Seam | 测什么 | 位置 |
| --- | --- | --- | --- |
| L1 | `voice_press::shortcut_before_pcm_clear` | 编排顺序：快捷键在 CLEAR 之前 | `voice_press.rs` + test |
| L2 | `voice_pcm::ping_retry_interval_ms` | PING 重试间隔 ≤20ms | `voice_pcm.rs` + test |
| L3 | `voice_pcm::ensure_pcm_ready_on_press` | 未就绪时调用同步 ensure（行为由集成/log 验证） | `voice_pcm.rs` |

## 步骤清单

| 步骤 | 内容 | 状态 | 验证 |
| --- | --- | --- | --- |
| 0 | 本文档 | ⬜ 待做 | — |
| 1 | L1 `voice_press` 顺序 + 测试 | ⬜ 待做 | `voice_first_packet` test |
| 2 | `on_voice_remote_press` 按 L1 重排 | ⬜ 待做 | 同上 + 逻辑审查 |
| 3 | L2/L3 `voice_pcm` PING 间隔 + 按下同步 ensure | ⬜ 待做 | `voice_first_packet` + `test:rust` |
| 4 | 全量回归 + README | ⬜ 待做 | `npm test` + `test:rust` + `cargo check` |

## 验证命令

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test voice_first_packet
npm run test:rust
cargo check --manifest-path src-tauri/Cargo.toml
```

## 变更日志

- （实施过程中追加）
