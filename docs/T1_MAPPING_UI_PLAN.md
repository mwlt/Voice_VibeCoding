# T1 遥控器重绘与独立映射台

按小米 KeyMappingStage 模式，为 T1 独立实现遥控器重绘 + 按键映射台。  
**零复用**小米组件：`KeyMappingStage` / `RemoteHotspot` / `RemoteKeyIcon` / `KeyBindingEditor`。

## 进度（按实测标记，禁止虚假勾选）

| Step | 内容 | 状态 | 验证 |
|------|------|------|------|
| 1 | `t1Keys` 纯逻辑缝 + 文档骨架 | 完成 | `npm test`：`t1Keys.test.ts` 11 passed |
| 2 | `T1RemoteHotspot` 机身重绘 | 完成 | 12 面键齐全、无 mic/back/volume/tv/vol；`vue-tsc --noEmit` 通过；参考图 `src/assets/remotes/t1-remote-ref.png` |
| 3 | `T1KeyIcon` + `T1KeyMappingStage` | 完成 | independence 测试通过；无小米组件 import；`vue-tsc` 通过 |
| 4 | 接入 `T1Settings` + Rust aliases 契约 | 完成 | `T1Settings` → `T1KeyMappingStage`；`npm test` 33 passed；`cargo check --lib` 通过；aliases 走 `bridges::t1::config` |
| 5 | 全仓收尾检查 + 参考图入库 | 完成 | 全局搜无误用；README 已补 T1 入口；见下方缺口 |

## 键位契约（14）

面键（示意热区）：`power` `up` `down` `left` `right` `ok` `delete` `voice` `mute` `mouse` `home` `menu`  
仅侧栏：`vol_plus` `vol_minus`（实物正面无音量键）

## 文件

- `src/utils/t1Keys.ts` / `t1Keys.test.ts`
- `src/utils/t1MappingIndependence.test.ts`
- `src/utils/t1SettingsWiring.test.ts`
- `src/components/t1/T1RemoteHotspot.vue`
- `src/components/t1/T1KeyIcon.vue`
- `src/components/t1/T1KeyMappingStage.vue`
- `src/assets/remotes/t1-remote-ref.png`
- `src/views/T1Settings.vue`（改用 T1 映射台）
- `src-tauri/src/bridges/t1/config.rs`（aliases 单一来源）
- `src-tauri/src/config/manager.rs`（`t1_button_aliases` 委托 t1::config；契约测试已扩展）

## 实测记录（2026-09-05）

- `npm test`：6 files / 33 tests passed
- `npx vue-tsc --noEmit`：exit 0
- `cargo check --manifest-path src-tauri/Cargo.toml --lib`：Finished ok
- Grep：`T1Settings` / `src/components/t1` 无小米映射组件 import；`t1Keys` 无 `mic` 双写

## 缺口 / 备注

- **`cargo test --lib`**：本机仍报 `STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139)`（既有 Windows DLL 环境问题）。`cargo check --lib` 可确认编译。
- 顺带修复：`src-tauri/src/lib.rs` 测试内 `use config::manager` → `use crate::config::manager`。
- `tsconfig.json` 排除 `src/**/*.test.ts`。
- **实机桥接**已另见 [T1_BRIDGE_PLAN.md](./T1_BRIDGE_PLAN.md)（连接 + 语音）。
- `DeviceStatus` 仍与其它页共用。
