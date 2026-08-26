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

Write-Host "PASS: all winuhid install script checks"
exit 0
