# 更新忽略策略重构计划

> 目标：「不再提醒此版本」仅抑制被动提醒；设置内「检查更新」始终可打开弹窗并下载。

## 语义

| 字段 | 含义 |
|------|------|
| `hasNewerVersion` / `updateAvailable` | semver 上确有新版本（可下载） |
| `promptSuppressed` | 用户已忽略**该版本**的自动提醒 |
| 被动提醒 | `updateAvailable && !promptSuppressed` |

## 行为矩阵

| 场景 | 顶栏角标 | 自动弹窗 | 设置 → 检查更新 |
|------|----------|----------|-----------------|
| 有新版本，未忽略 | ✅ | ✅（本会话未关闭） | ✅ 弹窗 |
| 有新版本，已忽略 | ❌ | ❌ | ✅ 弹窗（含提示） |
| 无新版本 | ❌ | ❌ | 提示已是最新 |

## 步骤与完成情况

- [x] **Step 1** 后端纯函数 `evaluate_update` + 单元测试（`cargo test --lib app_update::tests` 编译通过；本机运行时 DLL 问题未执行）
- [x] **Step 2** `UpdateCheckResult` 新字段 + `build_result` / `ignore_version` / `emit_if_available`
- [x] **Step 3** IPC `check_app_update(force?)` 
- [x] **Step 4** 前端 `appUpdateLogic.ts` + store + Vitest（6 tests passed）
- [x] **Step 5** UI：顶栏、设置检查更新、弹窗文案「不再提醒此版本」
- [x] **Step 6** `npm test` + `npm run build` + README 更新

## 涉及文件

- `src-tauri/src/app_update.rs` — 评估逻辑、结果字段、被动 emit
- `src-tauri/src/ipc/update_cmds.rs` — `force` 参数
- `src/stores/appUpdateLogic.ts` — 前端纯函数
- `src/stores/appUpdate.ts` — Pinia store
- `src/views/GlobalSettings.vue` — 主动检查
- `src/components/SideNav.vue` — 被动角标
- `src/components/AppUpdateModal.vue` — 文案与已忽略提示
