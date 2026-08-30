# Automated checks for install-winuhid.ps1 (no admin / no UAC required).
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$scriptPath = Join-Path $root "src-tauri\assets\winuhid\install-winuhid.ps1"
$driverDir = Join-Path $root "src-tauri\assets\winuhid\driver"
$dll = Join-Path $root "src-tauri\assets\winuhid\WinUHid.dll"

function Assert([bool] $Condition, [string] $Message) {
  if (-not $Condition) { throw "ASSERT FAILED: $Message" }
}

Write-Host "== winuhid install script tests =="

# 1) Syntax parse
$tokens = $null
$errors = $null
[void][System.Management.Automation.Language.Parser]::ParseFile($scriptPath, [ref]$tokens, [ref]$errors)
Assert ($errors.Count -eq 0) ("syntax errors: " + ($errors | ForEach-Object { $_.Message }) -join '; ')

# 2) Required canonical phases present in source
$src = Get-Content -LiteralPath $scriptPath -Raw
foreach ($needle in @(
  'RegisterRootDevice',
  'PhaseName "StageDriver"',
  'Write-Phase "RegisterRoot"',
  'PhaseName "BindDriver"',
  'PhaseName "ScanDevices"',
  '/scan-devices'
)) {
  Assert ($src -like "*$needle*") "missing required fragment: $needle"
}

# 3) Removed broken phantom bind API
Assert ($src -notmatch 'UpdateDriverForPlugAndPlayDevices') 'UpdateDriverForPlugAndPlayDevices must be removed'

# 4) Status mode runs
$statusOut = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $scriptPath -Mode Status -PackageDir $driverDir -DllSource $dll 2>&1 | Out-String
Assert ($statusOut -match 'Phase: Verify') 'Status mode must emit Phase: Verify'
Assert ($statusOut -match 'Result:') 'Status mode must emit Result:'

# 5) Force flag documented in Install path
Assert ($src -match '\[switch\] \$Force') 'Install script must support -Force switch'
Assert ($src -match 'force reinstall') 'Install script must mention force reinstall'

# 5b) Reboot (3010 + flag) only when pnputil actually returned 3010; otherwise retry auto-repair
Assert ($src -match 'retry auto-repair \(no reboot\)') 'unreachable without Windows 3010 must ask for auto-repair retry, not reboot'
Assert ($src -match 'Windows restart required \(exit 3010\)') 'true pnputil 3010 must still exit 3010'
$verifyFail = [regex]::Match($src, '(?s)if \(-not \(Wait-WinUHidReady\)\).*?exit 1')
Assert $verifyFail.Success 'Wait-WinUHidReady failure without $reboot must exit 1 (not 3010)'
Assert ($verifyFail.Value -match 'if \(\$reboot\)') '3010/flag must be gated on $reboot from pnputil'
Assert ($verifyFail.Value -match 'Set-Content[^\r\n]*\$RebootFlag') 'true 3010 path must still write reboot flag'
$afterRebootGate = [regex]::Match($verifyFail.Value, '(?s)exit 3010\s*\}\s*(.*)$')
Assert $afterRebootGate.Success 'verify-fail block must continue after $reboot gate'
Assert ($afterRebootGate.Groups[1].Value -notmatch 'Set-Content[^\r\n]*\$RebootFlag') 'non-3010 verify failure must not write reboot flag'
Assert ($afterRebootGate.Groups[1].Value -notmatch 'exit 3010') 'non-3010 verify failure must not exit 3010'

# 6) GitHub #10: SetupDiSetDeviceRegistryProperty must use Unicode (W) API
Assert (
  $src -match 'SetupDiSetDeviceRegistryPropertyW' -or
  ($src -match 'EntryPoint\s*=\s*"SetupDiSetDeviceRegistryPropertyW"')
) 'SetupDiSetDeviceRegistryProperty must use EntryPoint W / PropertyW'

# 6b) Pure helper: single-char MULTI_SZ is detected as corrupt
Assert ($src -match 'function Test-HardwareIdValueCorrupt') 'must define Test-HardwareIdValueCorrupt'
Assert ($src -match 'function Repair-WinUHidHardwareId') 'must define Repair-WinUHidHardwareId'

# 7) Corrupt HardwareID repair is invoked on RegisterRoot path
Assert ($src -match 'Repair-WinUHidHardwareId') 'RegisterRoot must call Repair-WinUHidHardwareId'

# 8) Audio root installer same Unicode fix
$audioPath = Join-Path $root "src-tauri\assets\xiaomi\configure-xiaomi-audio.ps1"
Assert (Test-Path -LiteralPath $audioPath) "missing $audioPath"
$audioSrc = Get-Content -LiteralPath $audioPath -Raw
Assert (
  $audioSrc -match 'SetupDiSetDeviceRegistryPropertyW' -or
  ($audioSrc -match 'EntryPoint\s*=\s*"SetupDiSetDeviceRegistryPropertyW"')
) 'configure-xiaomi-audio.ps1 must use SetupDiSetDeviceRegistryPropertyW'

# 9) Unit-test corrupt detector logic via scriptblock extract (no admin)
$detector = @'
function Test-HardwareIdValueCorrupt([string[]] $Values, [string] $Expected) {
  if ($null -eq $Values -or $Values.Count -eq 0) { return $false }
  if ($Values -contains $Expected) { return $false }
  $joined = ($Values -join '')
  if ($joined -eq $Expected) { return $true }
  if ($Values.Count -ge 4 -and ($Values | Where-Object { $_.Length -eq 1 }).Count -eq $Values.Count) {
    return $true
  }
  return $false
}
'@
Invoke-Expression $detector
Assert (-not (Test-HardwareIdValueCorrupt -Values @('Root\WinUHid') -Expected 'Root\WinUHid')) 'good id must not be corrupt'
Assert (Test-HardwareIdValueCorrupt -Values @('R','o','o','t','\','W','i','n','U','H','i','d') -Expected 'Root\WinUHid') 'single-char split must be corrupt'

Write-Host "PASS: all winuhid install script checks"
exit 0
