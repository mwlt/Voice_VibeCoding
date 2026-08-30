# HITL: 键盘探测 — 验证语音键是否仍泄漏 F5 / 粘键 / 仅 Ctrl
# 前置：启动当前构建的应用，遥控器已连接，语音映射为 Ctrl+Win（或你的目标组合）
# 用法：在应用里点「键盘探测」→ 按 3 次语音键 → 再点「停止键盘探测」→ 本脚本读分析结果

$ErrorActionPreference = "Stop"
Write-Host "=== Key probe voice F5 HITL ==="
Write-Host "1) 应用内点「键盘探测」"
Write-Host "2) 按遥控语音键 3 次（含长按一次）"
Write-Host "3) 点「停止键盘探测」，看界面分析结果；或打开「探测日志」"
Write-Host ""

function Ask-YesNo([string]$prompt) {
  while ($true) {
    $a = Read-Host $prompt
    if ($a -match '^[Yy]') { return $true }
    if ($a -match '^[Nn]') { return $false }
    Write-Host "请输入 Y 或 N"
  }
}

$probeOk = Ask-YesNo "界面/分析是否显示「F5泄漏=否」? (Y/N)"
$stuckOk = Ask-YesNo "是否「F5粘键嫌疑=否」且松开后其它键正常? (Y/N)"
$chordOk = Ask-YesNo "在线键盘测试/探测日志是否看到映射键(如 Ctrl+Win)，且不是「只有 F5」? (Y/N)"

if ($probeOk -and $stuckOk -and $chordOk) {
  Write-Host "GREEN: F5 源头清除 + 映射正常。把 key-probe.log 末尾留档即可。"
  exit 0
}

Write-Host "RED: 仍有泄漏/粘键/映射不全。"
Write-Host "请贴回: (1) 界面分析结果 (2) key-probe.log 里含 0x74 / 0xA2 / 0x5B 的行"
Write-Host "并确认已完全退出旧进程后重启（gadget 戳 v1.6.0-f5-zero 需重载 WUDFHost）"
exit 1
