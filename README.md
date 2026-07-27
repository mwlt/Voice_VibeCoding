# Voice VibeCoding





本项目：   rust语言 windows版（基于python版本重构） 作者 

*gitee:* **[](https://gitee.com/mwlt)**

[RemoteVoiceVibeCoding: Windows 桌面端遥控器桥接工具。把小米遥控器 2 Pro / T1 / 汉王 V60 接到电脑，用于按键映射与语音听写通路。](https://gitee.com/mwlt/remote-voice-vibe-coding)

github:





python windows版，作者：[xxb26553663-star](https://github.com/xxb26553663-star)  
[xxb26553663-star/remote-bridge-hub: Windows voice input and remote-control bridge for Xiaomi Remote, T1 and Hanvon V60](https://github.com/xxb26553663-star/remote-bridge-hub)

apple macos版 ，作者 [nijez](https://github.com/nijez)

[nijez/open-voice-bridge: 小米蓝牙遥控器 2 Pro / RC003 的原生 macOS 按键与 ATVV 语音桥接](https://github.com/nijez/open-voice-bridge)













**v1.3.6** · Windows 桌面应用

把小米遥控器 2 Pro（及预留的 T1 / 汉王 V60）接到电脑：按键可映射成键盘快捷键，语音可送到输入法听写。

本仓库是 **Rust + Tauri 2 + Vue 3** 实现，不是 Python 版 Remote Bridge Hub。二者功能相近，但运行时、安装包与配置目录均独立。

---

## 它能做什么

### 小米遥控器 2 Pro（主力）


| 能力      | 说明                                             |
| ------- | ---------------------------------------------- |
| 蓝牙连接    | 自动发现已配对设备，断线可重连                                |
| 按键映射    | 方向、确认、返回、音量等映射为自定义快捷键                          |
| 语音键     | 映射为「按住说话」或「点按开关」类输入法热键                         |
| 音频信号    | 显示遥控器 BLE 解码后的电平/波形                            |
| 虚拟声卡    | 检测并修复 VB-CABLE，让听写软件听到遥控器麦克风                   |
| 特殊键     | HID Tap 接管返回/音量等系统不易收到的键；抑制语音键泄漏的 F5（避免记事本插日期） |
| 冲突处理    | 端口或其它桥接进程占用时提示，可结束白名单进程并重试                     |
| ATVV 修复 | 一键软重启并重新订阅语音通道                                 |


### 其它

- **T1 / V60**：界面与配置页已预留，我没有对应设备无法测试，需要使用的请自行二次开发
- **托盘**：可最小化到托盘；支持开机自启
- **单实例**：再次打开会激活已有窗口，避免重复占端口
- **日志**：应用内查看 / 打开日志文件

---



## 架构（怎么串起来的）

用一句话理解：

> **遥控器** →（蓝牙 BLE / HID）→ **本应用（Rust）** →（键盘注入 + 虚拟声卡）→ **输入法 / 其它软件**

```
┌─────────────────────────────────────────────────────────────┐
│  界面（Vue 3）                                               │
│  设置、波形、映射、修复按钮、冲突弹窗                           │
└───────────────────────────┬─────────────────────────────────┘
                            │ Tauri IPC / 事件
┌───────────────────────────▼─────────────────────────────────┐
│  后端（Rust）                                                 │
│  · 连接与 ATVV 语音订阅（控制键 + 音频 GATT）                   │
│  · 按键映射 / 低级键盘钩子（吞 F5 等）                          │
│  · HID Tap：注入 WUDFHost，转发返回/音量等                      │
│  · 语音路由子进程：UDP PCM → VB-CABLE                          │
│  · 冲突扫描与白名单结束进程                                     │
└───────┬─────────────────┬─────────────────┬─────────────────┘
        │                 │                 │
   小米遥控器 BLE     HID / WUDFHost     VB-CABLE 虚拟声卡
```

**语音听写路径（小米）**

1. 按住遥控语音键 → ATVV 上报并传来压缩音频
2. 本应用解码后经本机 UDP 送给语音路由
3. 语音路由写入 VB-CABLE
4. 输入法把「麦克风」选成 `CABLE Output` 即可听写

若 ATVV 未连上：波形可能不动，语音键还可能变成系统 F5；此时用「修复 ATVV 连接」。

---



## 环境要求


| 项目       | 要求                                                                                                       |
| -------- | -------------------------------------------------------------------------------------------------------- |
| 系统       | Windows 10 / 11（64 位）                                                                                    |
| Node.js  | 18+（建议 LTS）                                                                                              |
| Rust     | `rustup` 安装的 stable 工具链                                                                                  |
| C++ 构建   | [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)（含「使用 C++ 的桌面开发」） |
| WebView2 | Win10/11 通常已自带                                                                                           |


小米语音另需：

- 遥控器已在系统蓝牙设置中配对  
- **VB-CABLE**（可在应用内「虚拟声卡检测与修复」安装/修复）  
- 首次启用返回/音量专用通道时，可能弹出 **UAC**（管理员注入）

---



## 从源码运行（开发）

```powershell
npm install
npm run tauri:dev
```

只跑前端（无桥接）：

```powershell
npm run dev
```

开发包窗口标题带 `[开发]`，顶栏版本为 `v1.3.6-dev`。

---



## 编译安装包

```powershell
npm run tauri:build
```

常见产物：


| 类型       | 路径                                                                          |
| -------- | --------------------------------------------------------------------------- |
| 可执行文件    | `src-tauri/target/release/remote-bridge-hub.exe`                            |
| MSI      | `src-tauri/target/release/bundle/msi/Voice VibeCoding_1.3.6_x64_zh-CN.msi`  |
| NSIS 安装包 | `src-tauri/target/release/bundle/nsis/Voice VibeCoding_1.3.6_x64-setup.exe` |


---



## 仓库结构

```
├── src/                     # Vue 前端（页面、组件、状态）
├── src-tauri/
│   ├── src/                 # Rust：桥接、音频、配置、IPC
│   ├── assets/xiaomi/       # VB-CABLE、Frida Gadget 等资源
│   ├── icons/
│   └── tauri.conf.json
├── package.json
└── README.md
```

---



## 配置与端口

- **配置 / 日志**：写入本机应用数据目录（不在仓库里），可在界面打开日志  
- **PCM 语音路由**：默认 UDP `127.0.0.1:31680`（环境变量 `REMOTE_BRIDGE_PCM_PORT`）  
- **HID Tap**：默认 TCP `127.0.0.1:30684`（环境变量 `REMOTE_BRIDGE_XIAOMI_HID_TAP_PORT`）  
- 若同时运行旧版 Python 桥接或其它实例，可能抢端口或 BLE，应用会提示冲突

输入法侧请将麦克风选为 **CABLE Output (VB-Audio Virtual Cable)**，且快捷键与本应用「语音键映射」一致（见应用内「输入法设置」）。

---



## 第三方组件

小米相关打包资源可能包含：

- **VB-Audio VB-CABLE**（虚拟声卡，遵循其 Donationware 说明）  
- **Frida Gadget**（用于读取 RC003 部分 HID 报告，非破解组件）

具体文件在 `src-tauri/assets/xiaomi/`。使用与再分发时请遵守各自许可。

---



## 与 Python 版的关系

同属「遥控器桥接」思路；本仓库为 **Voice VibeCoding / remote-bridge-hub 的 Rust·Tauri 重写**。

- 不要混用两套进程同时抢同一遥控器或相同端口  
- 配置目录、安装包名称均不同

---



## 参与贡献

欢迎提 Issue / PR：修 bug、补设备、改进文案与无障碍。  
大改前请先说明动机与影响范围。

---



## 许可证

仓库若未附带 `LICENSE` 文件，默认保留所有权利。  
开源分发前请补充许可证并核对第三方组件条款。