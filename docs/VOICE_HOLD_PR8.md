# 语音键按住说话（PR #8 合并说明）

> 分支：`test/pr-8-voice-hold`（GitHub PR #8）  
> 目标：微信「按住说话」等输入法可靠唤起；吞掉遥控器原生 F5，避免 `Ctrl+Win+F5` 导致微信不识别。

## 行为（当前实现）

| 遥控器操作 | 本软件行为 |
| --- | --- |
| 按下语音键 | WinUHid **单报告**注入映射快捷键 DOWN；`arm_voice_native_suppress()` 吞固件 F5 |
| 松开语音键 | 映射快捷键 UP + sanitizer；`disarm_voice_native_suppress()` |
| 短按（快按快松） | 等效点按一次快捷键（适合豆包「免按」等开关式） |
| 长按 | 快捷键持续按住（适合微信/豆包/千问「按住说话」） |

后端 **`handle_voice` 固定为上述 hold 语义**，不再读取 `trigger_mode` / `voice_release_behavior`。配置字段仍保留以便旧配置兼容与日后恢复 UI。

## 与输入法设置对齐

| 输入法 | 本软件预设 | 输入法内须一致 |
| --- | --- | --- |
| 微信 | 左 Ctrl + 左 Win | 「按住说话」快捷键（可改组合，两边一致即可） |
| 豆包 · 长按 | 右 Alt | 「长按模式」快捷键 |
| 豆包 · 免按 | 右 Alt + 空格 | 免按/开关式快捷键 |
| 千问 | 三选一 | 千问设置中的按住语音快捷键 |

详见应用内「输入法设置」与各 Tab 步骤说明；微信参考图：`wechat-ime-hotkeysV2.png`。

## 技术要点

1. **`press_single` / `release_single`**（`hid_injector.rs`）：多修饰键一次 HID 报告到位（如 Ctrl+Win → modifier `0x09`），避免分步时序导致微信不认。
2. **F5 抑制**（`key_mapping.rs` + `special_keys.rs`）：语音和弦期间吞遥控器泄漏的 F5；松手路径 **`disarm_voice_native_suppress()`** 与 `force_release_voice_shortcut` 同路径调用。
3. **钩子前置**：`bump_hook_to_front()` 在语音 DOWN 前执行，确保 LL 钩子先于微信钩子处理 F5。
4. **非语音键**：仍走分步 `press()` / `release()`。

## 前端

- 「触发模式」工具栏块默认 **隐藏**（`SHOW_VOICE_TRIGGER_MODE = false`，`XiaomiSettings.vue`），代码保留，改常量即可恢复。
- 豆包/微信/千问说明文案见 `src/utils/imePreset.ts`。

## 单元测试

| 位置 | 内容 |
| --- | --- |
| `hid_injector.rs` `#[cfg(test)]` | Ctrl+Win 单报告 modifier = `0x09` |
| `voice_f5_suppress_tests.rs` | `should_suppress_voice_f5` 在 `voice_native_suppress_active` 期间为 true |

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib voice_f5_suppress
cargo test --manifest-path src-tauri/Cargo.toml --lib voice_single_report
npm test
npm run test:rust
cargo check --manifest-path src-tauri/Cargo.toml
```

说明：部分 Windows 开发机 `--lib` 测例可能因 `STATUS_ENTRYPOINT_NOT_FOUND` 无法运行，集成测在 `src-tauri/tests/`。

## 相关文档

- `docs/VOICE_CHORD_RELEASE_PLAN.md` — UP 后 sanitizer、连点 recover
- `docs/IME_PROFILE_PLAN.md` — 预设与 `applyImePresetConfig`
- `bug修复.md` §4.5 — F5 泄漏与 hold 语义
