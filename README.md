# Voice VibeCoding

Windows 桌面端遥控器桥接工具（**v1.3.3**）。把小米遥控器 2 Pro / T1 / 汉王 V60 接到电脑，用于按键映射与语音听写通路。

技术栈：**Rust + Tauri 2 + Vue 3 + TypeScript**。

## 功能概览

- **小米遥控器 2 Pro**：蓝牙 BLE 连接、按键映射、语音键快捷键（点击/按住）、麦克风增益、VB-CABLE 语音路由、HID Tap（返回/音量等特殊键）
- **T1 / V60**：独立设置页（按设备能力启用）
- **系统能力**：托盘、开机自启（可配）、运行日志、虚拟声卡检测与修复

## 环境要求

| 项目 | 说明 |
|------|------|
| 系统 | Windows 10/11 x64 |
| Node.js | 18+（推荐 LTS） |
| Rust | stable（`rustup`） |
| 构建工具 | [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)（含 C++ 桌面开发） |
| WebView2 | 一般 Windows 10/11 已自带 |

小米语音听写还需要：

- 遥控器已与 Windows 蓝牙配对
- 可选安装 **VB-CABLE**（应用内可检测/修复；资源见 `src-tauri/assets/xiaomi/`）
- 首次接管返回/音量键时可能弹出 **UAC**（HID Tap / Frida Gadget）

## 快速开始（开发）

```bash
# 安装前端依赖
npm install

# 开发模式（热更新）
npm run tauri:dev
```

仅前端：

```bash
npm run dev
```

## 编译发布

```bash
npm run tauri:build
```

产物位置：

- 可执行文件：`src-tauri/target/release/remote-bridge-hub.exe`
- 安装包：
  - `src-tauri/target/release/bundle/msi/Voice VibeCoding_1.3.3_x64_zh-CN.msi`
  - `src-tauri/target/release/bundle/nsis/Voice VibeCoding_1.3.3_x64-setup.exe`

## 目录结构

```
├── src/                  # Vue 前端
│   ├── components/
│   ├── views/            # 小米 / T1 / V60 / 全局设置
│   ├── stores/
│   └── assets/
├── src-tauri/            # Rust / Tauri 后端
│   ├── src/              # 桥接、音频、配置、IPC
│   ├── assets/xiaomi/    # VB-CABLE、Frida Gadget、音频配置脚本
│   ├── icons/
│   └── tauri.conf.json
├── package.json
└── README.md
```

## 配置说明

- 设备配置由应用写入本机配置目录（非仓库内）
- 小米 HID Tap 默认端口：`127.0.0.1:30684`（可用环境变量 `REMOTE_BRIDGE_XIAOMI_HID_TAP_PORT` 覆盖）
- 请勿同时打开多个本软件实例，以免端口占用

## 推送到 GitHub / Gitee（首次）

本仓库已本地初始化。创建空仓库后执行：

```bash
# GitHub 示例
git remote add origin https://github.com/<你的用户名>/<仓库名>.git
git push -u origin main

# Gitee 示例
git remote add origin https://gitee.com/<你的用户名>/<仓库名>.git
git push -u origin main
```

> `src-tauri/assets/xiaomi/frida-gadget-*.dll.xz` 约 7MB，GitHub / Gitee 均可正常推送。若用 Git LFS，可按需自行配置。

## 许可证

未单独声明许可证前，保留所有权利。如需开源分发，请补充 `LICENSE` 后再发布。
