# WinUHid HardwareID 损坏修复（GitHub #10）— TDD 计划

> 目标：跨 Win10/11/IoT 正确写入 `SPDRP_HARDWAREID`（REG_MULTI_SZ Unicode），并修复已损坏节点。  
> 标记规则：仅据自动化测试实测勾选。

## 根因

`SetupDiSetDeviceRegistryProperty` 未走 **W** → 调用 **A 版**，UTF-16 字节被当成 ANSI MULTI_SZ → HardwareID 拆成单字符 → 驱动无法绑定（Stopped / `0xE0000203`）。

## 测试缝

| # | Seam | 断言 |
|---|------|------|
| H1 | `install-winuhid.ps1` | `EntryPoint=SetupDiSetDeviceRegistryPropertyW` |
| H2 | 同脚本 | `Repair-WinUHidHardwareId` + `Test-HardwareIdValueCorrupt` |
| H3 | `configure-xiaomi-audio.ps1` | 同样 PropertyW |
| H4 | 检测器单测 | 好 id 不标坏；`R,o,o,t…` 标坏 |

## 步骤

| 步骤 | 内容 | 状态 | 验证 |
|------|------|------|------|
| 0 | 本计划 | ✅ | 本文 |
| 1 | PropertyW（install + audio） | ✅ | H1/H3 |
| 2 | 损坏节点修复 + RegisterRoot 调用 | ✅ | H2；装前/装后 `Repair-WinUHidHardwareId` |
| 3 | 自动化测试 + 文档 | ✅ | `scripts/test-winuhid-install.ps1` → **PASS**（2026-08-30） |

## 如何复测

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-winuhid-install.ps1
# 已损坏机器：管理员
.\src-tauri\assets\winuhid\install-winuhid.ps1 -Force
```

## 相关

- Issue: https://github.com/mwlt/Voice_VibeCoding/issues/10  
- `docs/WINUHID_INSTALL_FIX.md`
