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
| S4 | `inject_voice_chord(keys, key_up) → bool` | 有 WinUHid 时优先 press/release；失败回落 SendInput | `key_mapping.rs`（运行时接线；DLL 有无由环境决定） |
| S5 | `should_tap_same_chord_after_up(behavior)` | `None` 不追加；`TapSameChord` 追加 | `voice_release.rs` + integration test |

非本阶段缝：自动识别前台输入法、UI 快照、真实 WinUHid 硬件 DLL。

## 步骤清单

| 步骤 | 内容 | 状态 | 验证 |
| --- | --- | --- | --- |
| 0 | 本计划 + 缝约定 | ✅ 完成 | 文档已写入 |
| 1 | TS：`imePreset` + Vitest | ✅ 完成 | `npm test`：5 passed |
| 2 | Rust/TS：`voice_release_behavior` 配置字段 | ✅ 完成 | `npm run test:rust` 中 config 2 passed |
| 3 | `VoiceChordState` + Hold 注入优先 WinUHid | ✅ 完成 | voice_chord 4 passed；`inject_voice_chord` 已接线 |
| 4 | 「输入法设置」多卡片 UI + 说明 | ✅ 完成 | `npm run build`（vue-tsc + vite）通过；6 张预设卡 |
| 5 | 接线：`handle_voice` UP 后按 behavior tap | ✅ 完成 | `maybe_tap_after_voice_up` + S5 单测 |
| 6 | README / 本计划更新 | ✅ 完成 | README 能力表 + 预设表；本文件全绿 |

## 首批 Preset（已落地）

| id | 名称 | 快捷键 | 触发 | release |
| --- | --- | --- | --- | --- |
| `wechat-legacy-hold` | 微信 · 旧版按住 | Ctrl+Win | Hold | None |
| `wechat-hold` | 微信 · 新版按住 | Ctrl+Shift+D | Hold | None |
| `wechat-toggle` | 微信 · 开关语音 | Ctrl+Win | Hold | TapSameChord |
| `doubao-hold` | 豆包 · 长按 | Right Alt | Hold | None |
| `doubao-hands-free` | 豆包 · 免按 | Right Alt+Space | Toggle | None |
| `qianwen-hold` | 千问 · 按住 | Right Alt | Hold | None |

## 验证命令（复跑）

```bash
npm test
npm run test:rust
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
```

说明：`cargo test --lib` 在本开发机曾出现 `STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139)`，故 Rust 测例放在 `src-tauri/tests/` 集成测试。

## 变更日志（实施过程）

- Step 0：写入计划与 TDD 缝约定。
- Step 1：`src/utils/imePreset.ts` + Vitest；`DeviceConfig.voice_release_behavior`（TS）。
- Step 2：Rust `VoiceReleaseBehavior`；`cargo test --test config_voice_release`。
- Step 3：`voice_chord_state.rs`、`inject_voice_chord`（WinUHid → SendInput）；`handle_voice` 改用状态机。
- Step 5（同批接线）：`voice_release.rs` + `maybe_tap_after_voice_up`。
- Step 4：`XiaomiSettings.vue` 多预设卡片；`npm run build` 通过。
- Step 6：README 能力表 / 预设表；顺手消除 `SetIsVisible` unused Result 告警。
- 最终复跑：`npm test` 5、`test:rust` 6、`build`、`cargo check` 均通过（2026-08-26）。
- 诊断（diagnosing-bugs）：用户反馈右 Alt **与** Ctrl+Win 均无法唤醒 → 根因是语音路径对非 Alt 仍优先 WinUHid（输入法听不到虚拟 HID）。已对齐 Nexus：`inject_voice_chord` / `voice_tap_vks` **永不** WinUHid，一律 SendInput+EXTRA_INFO；`tap_vks` 仅保留给普通按键映射。反馈环：`cargo test --test ime_voice_wake_route`。
