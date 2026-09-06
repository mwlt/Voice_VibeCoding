# T1 BLE L0 — disable Consumer Control HID collection only (no custom .sys).
# AC Search (0x0221) lives on UP:000C / U:0001. Keyboard/mouse/vendor collections stay on.
# Works with Secure Boot; no testsigning / BIOS changes.
[CmdletBinding()]
param(
  [ValidateSet("Install", "InstallElevated", "Status", "Uninstall")]
  [string] $Mode = "Status",
  [string] $PackageDir = "",
  [switch] $Force
)

$ErrorActionPreference = "Stop"
$ScriptRoot = if ($PSScriptRoot) { $PSScriptRoot } else { Split-Path -Parent $MyInvocation.MyCommand.Path }

$AllowVid = @("01620a", "1620a", "vid_1620", "vid_620a")
$AllowPid = @("0407", "pid&0407", "pid_0407")
$DenyAny = @(
  "vid_1915", "pid_1025", "vid_2717", "winuhid", "root\winuhid", "vid_046d", "vid_05ac"
)
# Consumer Control TLC — HID_DEVICE_SYSTEM_CONSUMER / UP:000C_U:0001
$ConsumerMarkers = @(
  "hid_device_system_consumer",
  "up:000c_u:0001",
  "up:000c_u:0001",
  "hid-compliant consumer control"
)

function Write-Phase([string] $Name, [string] $Detail) {
  Write-Output ("Phase: {0} | {1}" -f $Name, $Detail)
}

function Test-IsAdmin {
  $id = [Security.Principal.WindowsIdentity]::GetCurrent()
  $p = New-Object Security.Principal.WindowsPrincipal($id)
  return $p.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Invoke-SelfElevate([string] $ElevMode) {
  $arg = @(
    "-NoProfile", "-ExecutionPolicy", "Bypass",
    "-File", "`"$PSCommandPath`"",
    "-Mode", $ElevMode
  )
  if ($Force) { $arg += "-Force" }
  $p = Start-Process -FilePath "powershell.exe" -Verb RunAs -ArgumentList $arg -Wait -PassThru
  exit $p.ExitCode
}

function Test-BlobAllowed([string] $Blob) {
  $b = $Blob.ToLowerInvariant()
  foreach ($d in $DenyAny) {
    if ($b.Contains($d.ToLowerInvariant())) { return $false }
  }
  $vidOk = $false
  foreach ($a in $AllowVid) { if ($b.Contains($a)) { $vidOk = $true; break } }
  $pidOk = $false
  foreach ($a in $AllowPid) { if ($b.Contains($a)) { $pidOk = $true; break } }
  return ($vidOk -and $pidOk)
}

function Test-BlobConsumer([string] $Blob) {
  $b = $Blob.ToLowerInvariant()
  foreach ($m in $ConsumerMarkers) {
    if ($b.Contains($m)) { return $true }
  }
  return $false
}

function Test-DeviceDisabled([string] $RegPath) {
  # CONFIGFLAG_DISABLED = 0x1
  $cf = (Get-ItemProperty -LiteralPath $RegPath -Name ConfigFlags -ErrorAction SilentlyContinue).ConfigFlags
  if ($null -ne $cf -and (($cf -as [int]) -band 1) -ne 0) { return $true }
  return $false
}

function Get-T1ConsumerDevices {
  $out = @()
  $hidRoot = "HKLM:\SYSTEM\CurrentControlSet\Enum\HID"
  if (-not (Test-Path $hidRoot)) { return $out }
  foreach ($devNode in @(Get-ChildItem $hidRoot -ErrorAction SilentlyContinue)) {
    foreach ($instNode in @(Get-ChildItem $devNode.PSPath -ErrorAction SilentlyContinue)) {
      $inst = "HID\$($devNode.PSChildName)\$($instNode.PSChildName)"
      $ids = @()
      try {
        $hw = (Get-ItemProperty $instNode.PSPath -Name HardwareID -EA SilentlyContinue).HardwareID
        if ($hw) { $ids += @($hw) }
      } catch {}
      try {
        $comp = (Get-ItemProperty $instNode.PSPath -Name CompatibleIDs -EA SilentlyContinue).CompatibleIDs
        if ($comp) { $ids += @($comp) }
      } catch {}
      $desc = ""
      try { $desc = [string](Get-ItemProperty $instNode.PSPath -Name DeviceDesc -EA SilentlyContinue).DeviceDesc } catch {}
      $blob = (($ids + @($devNode.PSChildName, $inst, $desc)) -join "|")
      if (-not (Test-BlobAllowed $blob)) { continue }
      if (-not (Test-BlobConsumer $blob)) { continue }
      $disabled = Test-DeviceDisabled $instNode.PSPath
      $out += [pscustomobject]@{
        InstanceId = $inst
        RegPath    = $instNode.PSPath
        Disabled   = $disabled
        Blob       = $blob
      }
    }
  }
  return $out
}

function Disable-Instance([string] $InstanceId) {
  $r = & pnputil.exe /disable-device "$InstanceId" 2>&1 | Out-String
  Write-Phase "Disable" ("{0} => {1}" -f $InstanceId, ($r.Trim() -replace '\s+', ' '))
}

function Enable-Instance([string] $InstanceId) {
  $r = & pnputil.exe /enable-device "$InstanceId" 2>&1 | Out-String
  Write-Phase "Enable" ("{0} => {1}" -f $InstanceId, ($r.Trim() -replace '\s+', ' '))
}

# --- main ---
# PackageDir kept for Rust IPC compatibility; unused for disable path.
if (-not [string]::IsNullOrWhiteSpace($PackageDir)) {
  # no-op
}

if ($Mode -eq "Status") {
  $devs = @(Get-T1ConsumerDevices)
  $disabled = @($devs | Where-Object { $_.Disabled })
  $ready = ($devs.Count -gt 0) -and ($disabled.Count -eq $devs.Count)
  # ready / svc(unused=false) / bin(always true — no .sys) / matched / bound(=disabled)
  Write-Phase "Status" ("ready={0} svc=False bin=True matched={1} bound={2}" -f $ready, $devs.Count, $disabled.Count)
  if ($ready) {
    Write-Output "Result: READY"
    exit 0
  }
  if ($devs.Count -eq 0) {
    Write-Output "Result: NO_T1_BLE_DEVICE"
    exit 3
  }
  Write-Output "Result: NOT_BOUND"
  exit 1
}

if ($Mode -eq "Install") {
  if (-not (Test-IsAdmin)) {
    Write-Phase "Elevate" "UAC"
    Invoke-SelfElevate "InstallElevated"
  }
  $Mode = "InstallElevated"
}

if ($Mode -eq "Uninstall") {
  if (-not (Test-IsAdmin)) {
    Write-Phase "Elevate" "UAC"
    Invoke-SelfElevate "Uninstall"
  }
  $devs = @(Get-T1ConsumerDevices)
  foreach ($d in $devs) {
    if ($d.Disabled) { Enable-Instance $d.InstanceId }
  }
  Write-Output "Result: UNINSTALLED"
  exit 0
}

if ($Mode -eq "InstallElevated") {
  if (-not (Test-IsAdmin)) {
    Write-Output "Result: NEED_ADMIN"
    exit 5
  }
  $devs = @(Get-T1ConsumerDevices)
  Write-Phase "Match" ("consumer={0}" -f $devs.Count)
  if ($devs.Count -eq 0) {
    Write-Output "Result: NO_T1_BLE_DEVICE"
    exit 3
  }
  foreach ($d in $devs) {
    $blob = $d.Blob.ToLowerInvariant()
    foreach ($deny in $DenyAny) {
      if ($blob.Contains($deny.ToLowerInvariant())) {
        Write-Phase "Refuse" "deny $deny on $($d.InstanceId)"
        Write-Output "Result: REFUSED_NON_T1_BLE"
        exit 4
      }
    }
  }
  foreach ($d in $devs) {
    if (-not $d.Disabled) { Disable-Instance $d.InstanceId }
    else { Write-Phase "Disable" "already $($d.InstanceId)" }
  }
  Start-Sleep -Milliseconds 500
  $after = @(Get-T1ConsumerDevices)
  $disabled = @($after | Where-Object { $_.Disabled })
  if (($after.Count -gt 0) -and ($disabled.Count -eq $after.Count)) {
    Write-Output "Result: READY"
    exit 0
  }
  Write-Output "Result: NOT_BOUND"
  exit 1
}

Write-Output "Result: BAD_MODE"
exit 9
