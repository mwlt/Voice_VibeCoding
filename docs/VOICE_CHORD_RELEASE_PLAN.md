# 语音和弦释放卫生 — 长期最优方案（TDD 实施）

> 目标：语音键 **WinUHid 唤醒**可靠；长按传声正常；**不再残留 Win 修饰键**（物理 `D` 变 `Win+D`）。  
> 方法：TDD 垂直切片 — 每步 **红 → 绿 → 测试 → 检查修正 → 更新本文档**。  
> 标记规则：**仅在该步测试与检查通过后**标为完成；禁止预标。

## 架构原则

```
语音键按下/抬起
    ↓
WinUHid press / release（唯一唤醒通道 — 豆包/千问会过滤 SendInput）
    ↓
VoiceChordState（连点时先 recover 再 press；以 release 卫生为准）
    ↓
Release Sanitizer（UP 后必发全零 HID 报告 + 必要时 SendInput 仅 KEYUP）
    ↓
SendInput 仅作「清键器」，不作唤醒兜底
```

| 场景 | SendInput | 说明 |
| --- | --- | --- |
| 语音唤醒 DOWN | ❌ | IME 过滤，已验证 |
| WinUHid UP 后 sanitizer | ✅ 仅 KEYUP | 清 Win/Ctrl 残留 |
| WinUHid 不可用 | ❌ 静默回落 | 阻断 +「修复虚拟键盘」 |
| 普通按键映射 | ✅ fallback | 与语音路径分离 |

## TDD 测试缝

| # | Seam | 测什么 | 位置 |
| --- | --- | --- | --- |
| R1 | `voice_chord_sanitizer::sanitizer_targets` | 和弦修饰键列表（含 Ctrl+Win） | `voice_chord_sanitizer.rs` + integration test |
| R2 | `hid_injector::release_all` / `release` | UP 后必达全零报告 | `hid_injector.rs` |
| R3 | `VoiceChordState::press_with` | 已 held 时先 release 再 press | `voice_chord_state.rs` + test |
| R4 | `inject_voice_chord` | UP 路径调用 sanitizer；DOWN 不 SendInput 唤醒 | `key_mapping.rs` + `ime_voice_wake_route` |
| R5 | `on_voice_remote_release` | 先 UP 快捷键，再 sleep/PCM 收尾 | `input_session.rs`（逻辑审查 + 现有 rust 测） |

## 步骤清单

| 步骤 | 内容 | 状态 | 验证 |
| --- | --- | --- | --- |
| 0 | 本文档 + 缝约定 | ✅ 完成 | 文档已写入 |
| 1 | `voice_chord_sanitizer` 纯函数 + `hid_injector::release_all` | ✅ 完成 | `voice_chord_sanitize` 4 passed |
| 2 | `inject_voice_chord` 接线 sanitizer；`VoiceChordState` 连点 recover | ✅ 完成 | `voice_chord_and_release` 8 passed |
| 3 | `input_session` UP 先于 40ms sleep | ✅ 完成 | 代码审查 + 全量 rust 测通过 |
| 4 | recover 计数/日志可观测 | ✅ 完成 | `recover_count()` + sanitizer 日志 |
| 5 | 全量回归 + README 能力表 | ✅ 完成 | `npm test` 5 + `test:rust` 16 + `cargo check` |

## 验证命令

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test voice_chord_sanitize
cargo test --manifest-path src-tauri/Cargo.toml --test voice_chord_and_release
cargo test --manifest-path src-tauri/Cargo.toml --test ime_voice_wake_route
npm run test:rust
npm test
cargo check --manifest-path src-tauri/Cargo.toml
```

## 变更日志

- Step 0：写入 `docs/VOICE_CHORD_RELEASE_PLAN.md` 长期架构与 TDD 缝。
- Step 1：新增 `voice_chord_sanitizer.rs`；`hid_injector::release` UP 后必发全零报告；`release_all()`。
- Step 2：`inject_voice_chord` 用 sanitizer（DOWN 清 foreign / UP 清 chord 含 Win）；连点 `press_with` 先 UP 再 DOWN；UP 失败且 sanitizer 清键成功仍返回 ok。
- Step 3：`on_voice_remote_release` 先 `shortcut UP`，再 sleep/PCM 收尾。
- Step 4：`recover_count()` 原子计数 + 逐键 `sanitizer cleared` 日志。
- Step 5：2026-08-27 复跑 — `npm test` 5 passed、`test:rust` 16 passed、`cargo check` ok。

## PR #8 追加（2026-08-27）

- 语音 DOWN 改用 **`press_single`**（Ctrl+Win 等修饰键单 HID 报告），见 `docs/VOICE_HOLD_PR8.md`。
- UP 路径不变：sanitizer + `disarm_voice_native_suppress` 与 `force_release_voice_shortcut` 同路径。
- 集成测与 `voice_f5_suppress` / `voice_single_report` 单测覆盖 F5 抑制与 modifier 字节。
