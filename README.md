# Voice VibeCoding

本项目：   rust语言 windows版（基于python版本重构） 作者 ：mwlt

*gitee:* 

[https://gitee.com/mwlt/remote-voice-vibe-coding](https://gitee.com/mwlt/remote-voice-vibe-coding)

github:

[https://github.com/mwlt/Voice_VibeCoding](https://github.com/mwlt/Voice_VibeCoding)



python windows版，作者：[xxb26553663-star](https://github.com/xxb26553663-star)

  
[https://github.com/xxb26553663-star/remote-bridge-hub](https://github.com/xxb26553663-star/remote-bridge-hub)

apple macos版 ，作者 [nijez](https://github.com/nijez)


[https://github.com/nijez/open-voice-bridge](https://github.com/nijez/open-voice-bridge)



**v1.5.6** · Windows 桌面应用

把小米遥控器 2 Pro（及预留的 T1 / 汉王 V60）接到电脑：按键可映射成键盘快捷键，语音可送到输入法听写。

本仓库是 **Rust + Tauri 2 + Vue 3** 实现，不是 Python 版 Remote Bridge Hub。二者功能相近，但运行时、安装包与配置目录均独立。

---

## 它能做什么



### 小米遥控器 2 Pro（主力）

相对 Python 版（Xiaomi Remote Bridge）在体验与可靠性上的改进与增量如下。基础能力（蓝牙连接、按键映射、ATVV 语音、HID Tap、VB-CABLE 等）两端对齐，此处不重复罗列。


| 能力 / 改进点 | 说明 | 相对 Python |
| --- | --- | --- |
| 遥控器示意 | 按小米遥控器 2 Pro 实物重绘：丝印图标、银壳凹槽、键面凹凸层次与扁平选中态 | 增强 |
| 音量键防双格 | HID Tap 接管后无条件吞原生音量事件，消除 LL 钩子先于 BLE 信号的时序窗（固件原生 + 注入叠成两格） | 优化 |
| WebView 白屏/黑屏恢复 | 心跳守卫 + reload；reload 失败自动 recreate WebView；关闭到托盘改 minimize；托盘「刷新界面」「重启软件」 | 增强 |
| 启动黑框消除 | 子进程（icacls / netstat / powershell）加 CREATE_NO_WINDOW；HID Tap HostPid 日志降为 debug | 优化 |
| 启动后最小化到托盘 | 全局设置可选：启动不显示主窗口，点托盘打开 | 新增 |
| 一键修复 ATVV | 「修复 ATVV 连接」：有占用先清进程，再停 HID Tap、软重启并等待语音通道恢复；文案区分有无占用 | 新增 |
| ATVV 状态红字提示 | 桥接已运行但语音通道未订阅时，在「音频信号」旁显示「ATVV 未连接」 | 新增 |
| ATVV 失败系统通知 | 语音通道未就绪时按语音键，右下角通知引导去点「修复 ATVV 连接」（限流，避免刷屏） | 新增 |
| 按键录入扩展 | 对齐常见 108 键显示名；录入会话旁听 Consumer 媒体键（音量±/静音）；常驻「设置为：」按钮兜底计算器等；默认语音触发为「按住」 | 增强 |
| F5 抑制策略 | ATVV 正常时只吞与语音键关联的 F5，物理键盘 F5 可用；失败时放行并提示，避免「连着遥控就不能按 F5」 | 优化 |
| ATVV / HID Tap 时序 | 订阅语音通道前暂停 HID Tap，降低 AccessDenied；订阅成功后再启 Tap | 优化 |
| 语音路由占用策略 | 默认 `hold_device`：启动握 CABLE 设备，仅说话时 play；空闲不常驻写静音（见下文「VB-CABLE 占用方式」） | 优化 |
| 虚拟声卡状态探测 | 优先读系统 MMDevices 注册表判断 CABLE 是否就绪；已就绪后停探，避免设置页轮询经 WASAPI 枚举导致 `audiodg` 句柄异常上涨 | 优化 |
| 虚拟声卡修复体验 | 「虚拟声卡检测与修复」隐藏 PowerShell 黑框与系统 OK 弹窗；结果进状态日志；脚本 UTF-8 BOM，避免中文系统解析失败；仅需重启 Windows 时弹醒目提示 | 优化 |
| HID 注入闪窗 | 提权注入 WUDFHost 时隐藏控制台闪窗（UAC 仍保留） | 优化 |
| 音频信号波形 | 设置页实时显示 BLE 解码电平 / 波形，便于判断语音是否真正进机 | 增强 |
| 虚拟声卡检测与安装 | 应用内检测 VB-CABLE，支持内嵌驱动或官网安装指引，结果写回主机状态 | 增强 |
| 语音键按住说话（微信等） | WinUHid 单报告注入 Ctrl+Win；吞遥控器泄漏 F5；松手统一 disarm；输入法设置说明与参考图 | 修复 |
| 输入法设置引导 | 「输入法设置」分 Tab：微信 / 豆包 / 千问 / 常见问题；一键预设、设置参考图、口语化步骤 | 增强 |
| 语音键快速设置 | 键位映射页一键设置常用语音组合（微信、千问 Win+Alt 等） | 新增 |
| 修复虚拟键盘 | 「修复虚拟键盘」修复 WinUHid 虚拟键盘；支持导出/应用内下载驱动包（进度条、自选保存路径）；Release 附带 WinUHid_Manual 手动安装包 | 增强 |
| 配置加载容错 | 按键映射区依赖配置加载；损坏 xiaomi.json 自动备份并恢复默认；失败时显示错误与重试 | 修复 |
| 主机状态栏布局 | 四列同排显示虚拟声卡/键盘/路由/桥接；虚拟声卡电平条可收缩，状态文字不挤出边框 | 优化 |
| 应用内更新 | 顶栏被动提醒；「不再提醒此版本」仅关闭自动弹窗/角标；设置 → 检查更新仍可打开弹窗并下载 | 增强 |
| 语音键注入稳态 | Hold 路径用 VoiceChordState 防粘键；WinUHid 分步 press/release；UP 后 sanitizer 全零报告 + SendInput 仅清键 | 优化 |
| 语音首包延迟 | 按下先快捷键 DOWN 再 VB-CABLE CLEAR；PCM 按下同步 ensure；PING 重试 15ms | 优化 |
| 单实例 | 再次启动只激活已有窗口，降低双开抢端口 | 增强 |
| 应用内日志 | 界面直接查看 / 复制 / 打开日志，不必只翻 `%APPDATA%` 文件 | 增强 |
| 统一桌面壳 | Rust + Tauri 2 + Vue 单安装包、托盘与设置页一体，免 Python 运行时 | 增强 |
| 项目与致谢 | 设置页标明本版与 Python / macOS 相关仓库来源 | 新增 |


### 其它

- **T1 / V60**：界面与配置页已预留，我没有对应设备无法测试，需要使用的请自行二次开发
- **托盘**：可最小化到托盘；支持开机自启

![界面图](./image/1.png "系统界面预览")

![界面图2](./image/2.png "系统界面预览 2")

---



## 下载安装包

正式安装包在两边的 Release 页（当前 **v1.5.6**）：

- [Gitee Releases](https://gitee.com/mwlt/remote-voice-vibe-coding/releases/tag/v1.5.6)（国内优先）
- [GitHub Releases](https://github.com/mwlt/Voice_VibeCoding/releases/tag/v1.5.6)

常用文件：

- `Voice VibeCoding_1.5.6_x64-setup.exe`（NSIS）
- `Voice VibeCoding_1.5.6_x64_zh-CN.msi`
- `WinUHid_Manual_1.5.6.zip`（WinUHid 虚拟键盘手动安装包，也可在应用内「修复虚拟键盘 → 下载驱动包」下载）

安装时若提示无法覆盖 `remote-bridge-hub.exe`，请先退出本软件（含托盘）再重试。

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



## VB-CABLE 占用方式（语音路由生命周期）

语音路由子进程把 PCM 写到 **CABLE Input**。占用虚拟声卡的时机不同，会影响首按延迟，以及个别机器上系统音频隔离进程 `audiodg.exe` 是否异常涨句柄。本机对照测试了三种策略（空闲涨速均≈0 后按体验选型）：

| 方式 | 环境变量值 | 行为（人话） | 空闲占用 | 首按 |
| --- | --- | --- | --- | --- |
| ① 一直播放 | `always_play` | 启动就建流并一直 play（没说话时灌静音） | 最高 | 最快 |
| ② 握设备（**默认**） | `hold_device` | 启动先握住 CABLE 设备；**说话才 play**；说完停播但设备仍握着 | 中 | 接近① |
| ③ 全推迟 | `deferred` | 空闲只听 UDP；按下说话才开设备+流；说完全部释放 | 最低 | 略冷启动 |

**为何默认用 ② `hold_device`：**

- 三档在「设置页不再狂扫声卡」之后，空闲句柄涨速都能压住；差别主要在体验与占用。
- 比 ①：空闲不常驻「播放静音」，少占活跃音频通路。
- 比 ③：设备已握好，按语音键只需开始播放，首字更跟手。
- 按键映射 / HID 与此无关；设置页波形、「输送中」仍看是否真有 PCM，不依赖占用策略。

开发排障可覆盖默认：

```powershell
$env:REMOTE_BRIDGE_AUDIO_LIFECYCLE = "hold_device"   # always_play | hold_device | deferred
```

更细的对照步骤见 `scripts/ab_audio_lifecycle.md`。

**虚拟声卡「已安装」状态灯：** 优先读注册表 MMDevices（与安装脚本一致）；已就绪后停止自动重探；未就绪约每 60s 可再试一次；点「虚拟声卡检测与修复」强制重探。勿与波形闪烁混淆——闪烁表示正在送语音，不是探测间隔。

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

---



## 编译安装包

```powershell
npm run tauri:build
```

常见产物：


| 类型       | 路径                                                                            |
| -------- | ----------------------------------------------------------------------------- |
| 可执行文件    | `src-tauri/target/release/remote-bridge-hub.exe`                              |
| MSI      | `src-tauri/target/release/bundle/msi/Voice VibeCoding_1.5.6_x64_zh-CN.msi`  |
| NSIS 安装包 | `src-tauri/target/release/bundle/nsis/Voice VibeCoding_1.5.6_x64-setup.exe` |


发新版时请同步更新仓库根目录 `update/latest.json`（提高 `version`，填写 Gitee/GitHub 页面与安装包直链）。应用会优先读 Gitee raw，失败再读 GitHub raw。有新版本时在顶栏显示「新版本」与「查看更新内容」；弹窗内可选「不再提醒此版本」（仅抑制自动提醒，不影响设置页「检查更新」）。详见 [docs/UPDATE_IGNORE_PLAN.md](docs/UPDATE_IGNORE_PLAN.md)。


---



## 仓库结构

```
├── src/                     # Vue 前端（页面、组件、状态）
├── src-tauri/
│   ├── src/                 # Rust：桥接、音频、配置、IPC
│   ├── assets/xiaomi/       # VB-CABLE、Frida Gadget、configure 脚本等
│   ├── icons/
│   └── tauri.conf.json
├── scripts/                 # 排障脚本（如 audiodg 句柄对照、生命周期 A/B）
├── update/latest.json       # 轻量更新检查清单
├── package.json
└── README.md
```
---



## 配置与端口

- **配置 / 日志**：写入本机应用数据目录（不在仓库里），可在界面打开日志  
- **PCM 语音路由**：默认 UDP `127.0.0.1:31680`（`REMOTE_BRIDGE_PCM_PORT`）；生命周期默认 `hold_device`（`REMOTE_BRIDGE_AUDIO_LIFECYCLE`）  
- **虚拟声卡探测**：注册表优先；未就绪重试间隔可用 `REMOTE_BRIDGE_CABLE_PROBE_TTL_MS`（毫秒，默认 60000；`0` 表示未就绪也不自动重试）  
- **HID Tap**：默认 TCP `127.0.0.1:30684`（`REMOTE_BRIDGE_XIAOMI_HID_TAP_PORT`）  
- 若同时运行旧版 Python 桥接或其它实例，可能抢端口或 BLE，应用会提示冲突

输入法侧请将麦克风选为 **CABLE Output (VB-Audio Virtual Cable)**。语音键映射须与输入法内快捷键一致：**按住遥控语音键 = 按住该组合，松手 = 释放**（见应用内「输入法设置」）。

### 输入法预设（一键应用）

首页「输入法设置」提供常用预设。**本软件映射须与输入法内对应快捷键一致**：

| 预设 | 本软件快捷键 | 输入法侧须一致 |
| --- | --- | --- |
| 微信 · 按住说话 | 左 Ctrl + 左 Win | 微信「按住说话」（可改组合，两边相同即可） |
| 豆包 · 长按 | 右 Alt | 豆包「长按模式」 |
| 千问 · 右 Alt | 右 Alt | 千问按住语音 |
| 千问 · Win+Alt | 左 Win + 左 Alt | 千问按住语音（需 WinUHid） |
| 千问 · Ctrl+Win | 左 Ctrl + 左 Win | 千问按住语音（需 WinUHid） |

键位映射页「语音键快速设置」可一键应用上表常用组合。其它输入法：在按键映射里设成与输入法相同的组合即可。详见应用内「常见问题」Tab、`docs/IME_PROFILE_PLAN.md` 与 `docs/VOICE_HOLD_PR8.md`。

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
