# HITL: 验证豆包/千问语音热键唤起
# 用法：完全退出旧进程 → 重新编译/启动本应用 → 按遥控语音键 → 在下方打 Y/N

$ErrorActionPreference = "Stop"
Write-Host "=== IME voice wake HITL ==="
Write-Host "前置: 完全退出托盘里的旧 Voice VibeCoding，再启动当前构建。"
Write-Host "配置: 千问=右 Alt 或 Ctrl+Win；豆包=右 Alt（按住）。"
Write-Host ""

function Ask-YesNo([string]$prompt) {
  while ($true) {
    $a = Read-Host $prompt
    if ($a -match '^[Yy]') { return $true }
    if ($a -match '^[Nn]') { return $false }
    Write-Host "请输入 Y 或 N"
  }
}

$phys = Ask-YesNo "1) 实体键盘热键能否唤起输入法? (Y/N)"
$map = Ask-YesNo "2) 遥控器语音键映射后能否唤起? (Y/N)"

if ($phys -and $map) {
  Write-Host "GREEN: 映射与物理一致，问题已解。"
  exit 0
}
if ($phys -and -not $map) {
  Write-Host "RED: 仍是「物理有效 / 注入无效」。请把日志里含 'XIAOMI VOICE inject via' 的行贴回。"
  exit 1
}
if (-not $phys) {
  Write-Host "SKIP: 先确认输入法热键本身配置正确。"
  exit 2
}
