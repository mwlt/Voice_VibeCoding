# Fast elevated install without Get-PnpDevice hang.
# Copies sys, creates service, binds LowerFilters via CIM/registry.
param(
  [string] $PackageDir = ""
)

$ErrorActionPreference = "Stop"
$Root = if ($PSScriptRoot) { $PSScriptRoot } else { Split-Path -Parent $MyInvocation.MyCommand.Path }
if ([string]::IsNullOrWhiteSpace($PackageDir)) { $PackageDir = Join-Path $Root "driver" }
$ServiceName = "t1blehidf"
$SysName = "t1blehidf.sys"
$Log = Join-Path $env:TEMP "t1-l0-fast-install.log"
$Allow = @("01620a", "1620a", "vid_1620")
$PidTok = @("0407", "pid&0407", "pid_0407")
$Deny = @("vid_1915", "pid_1025", "vid_2717", "winuhid", "vid_046d", "vid_05ac")

function Log($m) {
  $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m
  Add-Content $Log $line
  Write-Host $line
}

function Test-IsAdmin {
  $p = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
  return $p.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

"" | Set-Content $Log
Log "fast-install start pkg=$PackageDir"

if (-not (Test-IsAdmin)) {
  $p = Start-Process powershell.exe -Verb RunAs -ArgumentList @(
    "-NoProfile","-ExecutionPolicy","Bypass","-File",$PSCommandPath,"-PackageDir",$PackageDir
  ) -Wait -PassThru
  Get-Content $Log -EA SilentlyContinue
  exit $p.ExitCode
}

$sys = Join-Path $PackageDir $SysName
if (-not (Test-Path $sys)) { Log "NEED_BINARY"; Write-Output "Result: NEED_BINARY"; exit 2 }

$dest = Join-Path $env:WINDIR "System32\drivers\$SysName"
Copy-Item -LiteralPath $sys -Destination $dest -Force
Log "Copied $dest"

$svc = Get-Service -Name $ServiceName -EA SilentlyContinue
if (-not $svc) {
  & sc.exe create $ServiceName type= kernel start= demand binPath= "\SystemRoot\System32\drivers\$SysName" DisplayName= "T1 BLE HID Filter (t1blehidf)" | Out-Null
  Log "Created service $ServiceName"
} else {
  & sc.exe config $ServiceName binPath= "\SystemRoot\System32\drivers\$SysName" | Out-Null
  Log "Configured service $ServiceName"
}

# Fast match via registry Enum\HID (avoid Get-PnpDevice hang)
$matched = @()
$hidRoot = "HKLM:\SYSTEM\CurrentControlSet\Enum\HID"
if (Test-Path $hidRoot) {
  foreach ($devNode in @(Get-ChildItem $hidRoot -EA SilentlyContinue)) {
    foreach ($instNode in @(Get-ChildItem $devNode.PSPath -EA SilentlyContinue)) {
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
      # Also match on the device node key (often contains VID/PID)
      $blob = (($ids + @($devNode.PSChildName, $inst)) -join "|").ToLowerInvariant()
      $denied = $false
      foreach ($d in $Deny) { if ($blob.Contains($d)) { $denied = $true; break } }
      if ($denied) { continue }
      $vidOk = $false
      foreach ($a in $Allow) { if ($blob.Contains($a)) { $vidOk = $true; break } }
      $pidOk = $false
      foreach ($a in $PidTok) { if ($blob.Contains($a)) { $pidOk = $true; break } }
      if ($vidOk -and $pidOk) {
        $matched += $inst
      }
    }
  }
}

Log "Matched HID instances: $($matched.Count)"
foreach ($m in $matched) { Log "  $m" }

if ($matched.Count -eq 0) {
  Log "NO_T1_BLE_DEVICE (service installed anyway)"
  Write-Output "Result: NO_T1_BLE_DEVICE"
  exit 3
}

foreach ($inst in $matched) {
  $path = "HKLM:\SYSTEM\CurrentControlSet\Enum\$inst"
  if (-not (Test-Path $path)) { Log "skip missing $inst"; continue }
  $cur = @()
  $lf = (Get-ItemProperty $path -Name LowerFilters -EA SilentlyContinue).LowerFilters
  if ($lf) { $cur = @($lf) }
  if ($cur -contains $ServiceName) {
    Log "already bound $inst"
    continue
  }
  $next = $cur + @($ServiceName)
  New-ItemProperty -LiteralPath $path -Name LowerFilters -PropertyType MultiString -Value $next -Force | Out-Null
  Log "bound LowerFilters -> $inst"
}

# Try start service (may fail until testsigning/reboot)
$startOut = & sc.exe start $ServiceName 2>&1 | Out-String
Log "sc start: $startOut"

Write-Output "Result: BOUND_$($matched.Count)"
Log "Result: BOUND_$($matched.Count)"
exit 0
