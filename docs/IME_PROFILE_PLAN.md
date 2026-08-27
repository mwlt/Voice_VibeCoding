# IME Profile 长期方案 — 实施计划与完成度

> 目标：可扩展输入法配置（Preset）+ 可靠注入 + 多输入法设置说明。  
> 方法：TDD（红 → 绿 → 检查修正 → 更新本文档）逐步落地。  
> 标记规则：**仅在该步测试与检查通过后**标为完成；禁止预标。

## TDD 测试缝（已确认，据此写测）

| # | Seam（公共边界） | 测什么 | 位置 |
| --- | --- | --- | --- |
| S1 | `applyImePresetConfig(config, presetId)` | 一键应用后：快捷键 VK、`voice_hotkey`、`trigger_mode`、`voice_release_behavior`、`voice_shortcut_enabled` | `src/utils/imePreset.ts` + Vitest |
| S2 | `DeviceConfig.voice_release_behavior` 序列化默认 | 旧配置缺字段 → `None`；新字段可读写 | `src-tauri/tests/config_voice_release.rs` |
| S3 | `VoiceChordState::press_with` / `release_with` | 粘键防护、DOWN 失败补偿 KEYUP、release 重试一次 | `voice_chord_state.rs` + integration test |
| S4 | `inject_voice_chord(keys, key_up) → bool` | 有 WinUHid 时 press/release；DOWN 用 `press_single`；UP 后 sanitizer；**不** SendInput 唤醒 | `key_mapping.rs` + `ime_voice_wake_route` |
| S5 | ~~`should_tap_same_chord_after_up`~~ | **已废弃**：PR #8 删除 `voice_release.rs`；后端固定 hold 语义，不再读 `voice_release_behavior` | — |
| S6 | `should_suppress_voice_f5` + `press_single` modifier | 语音 armed 时吞 F5；Ctrl+Win 单报告 `0x09` | `voice_f5_suppress_tests.rs` + `hid_injector` 单测 |

非本阶段缝：自动识别前台输入法、UI 快照、真实 WinUHid 硬件 DLL。

## 步骤清单

| 步骤 | 内容 | 状态 | 验证 |
| --- | --- | --- | --- |
| 0 | 本计划 + 缝约定 | ✅ 完成 | 文档已写入 |
| 1 | TS：`imePreset` + Vitest | ✅ 完成 | `npm test` |
| 2 | Rust/TS：`voice_release_behavior` 配置字段 | ✅ 完成（字段保留，运行时未用） | `config_voice_release` |
| 3 | `VoiceChordState` + Hold 注入优先 WinUHid | ✅ 完成 | voice_chord 集成测 |
| 4 | 「输入法设置」多卡片 UI + 说明 | ✅ 完成 | 微信/豆包/千问 Tab + V2 截图 |
| 5 | ~~UP 后 TapSameChord~~ | ⏭ 跳过 | PR #8 hold-only |
| 6 | README / 本计划更新 | ✅ 完成 | 见 PR #8 文档 |
| 7 | PR #8：单报告注入 + F5 抑制 + UI 隐藏触发模式 | ✅ 完成 | `docs/VOICE_HOLD_PR8.md` |

## 首批 Preset（当前）

| id | 名称 | 快捷键 | 说明 |
| --- | --- | --- | --- |
| `wechat-hold` | 微信 · 按住说话 | Ctrl+Win | 须与微信「按住说话」一致；图 `wechat-ime-hotkeysV2.png` |
| `doubao-hold` | 豆包 · 长按 | Right Alt | 须与豆包「长按模式」一致；图 `doubao.png` |
| ~~`doubao-hands-free`~~ | 豆包 · 免按 | Right Alt+Space | 仅按键映射页快速应用，不在「输入法设置」展示 |
| `qianwen-hold` | 千问 · 右 Alt | Right Alt | 按住说话 |
| `qianwen-win-alt` | 千问 · Win+Alt | Left Win + Left Alt | 需 WinUHid |
| `qianwen-ctrl-win` | 千问 · Ctrl+Win | Left Ctrl + Left Win | 需 WinUHid |

**运行时语义**：不论 preset 里 `triggerMode` 为何，语音键均 **跟随遥控器物理操作**（见 `docs/VOICE_HOLD_PR8.md`）。前端「触发模式」下拉默认隐藏。

## 验证命令（复跑）

```bash
npm test
npm run test:rust
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --lib voice_f5_suppress voice_single_report
```

说明：`cargo test --lib` 在本开发机曾出现 `STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139)`，故 Rust 测例亦放在 `src-tauri/tests/` 集成测试。

## 变更日志（实施过程）

- Step 0–6：初版 IME Profile（2026-08-26）。
- 语音释放卫生（2026-08-27）：`docs/VOICE_CHORD_RELEASE_PLAN.md`。
- PR #8（2026-08-27）：`press_single`、F5 抑制、`disarm` 统一在 `handle_voice(false)`；微信/豆包说明更新；隐藏触发模式 UI；`docs/VOICE_HOLD_PR8.md`。
