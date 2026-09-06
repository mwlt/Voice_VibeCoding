# Remove unsigned t1blehidf bindings/service (Secure Boot incompatible path).
param([switch]$RunElevated)
$ErrorActionPreference = "Stop"
$ServiceName = "t1blehidf"
$Log = Join-Path $env:TEMP "t1-l0-cleanup.log"

function Log($m) {
  $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m
  Add-Content $Log $line -EA SilentlyContinue
  Write-Host $line
}
function Test-IsAdmin {
  $p = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
  return $p.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

if (-not $RunElevated) {
  "" | Set-Content $Log
  if (-not (Test-IsAdmin)) {
    $p = Start-Process powershell.exe -Verb RunAs -ArgumentList @(
      "-NoProfile","-ExecutionPolicy","Bypass","-File",$PSCommandPath,"-RunElevated"
    ) -Wait -PassThru
    Get-Content $Log -EA SilentlyContinue
    exit $p.ExitCode
  }
}

Log "cleanup start"
& sc.exe stop $ServiceName 2>$null | Out-Null

$hidRoot = "HKLM:\SYSTEM\CurrentControlSet\Enum\HID"
$removed = 0
if (Test-Path $hidRoot) {
  foreach ($devNode in @(Get-ChildItem $hidRoot -EA SilentlyContinue)) {
    $key = $devNode.PSChildName.ToLowerInvariant()
    if ($key -notmatch '01620a|1620a') { continue }
    foreach ($instNode in @(Get-ChildItem $devNode.PSPath -EA SilentlyContinue)) {
      $path = $instNode.PSPath
      $lf = (Get-ItemProperty $path -Name LowerFilters -EA SilentlyContinue).LowerFilters
      if (-not $lf) { continue }
      $next = @($lf | Where-Object { $_ -ne $ServiceName })
      if ($next.Count -eq @($lf).Count) { continue }
      if ($next.Count -eq 0) {
        Remove-ItemProperty -LiteralPath $path -Name LowerFilters -EA SilentlyContinue
      } else {
        Set-ItemProperty -LiteralPath $path -Name LowerFilters -Value $next
      }
      $removed++
      Log "removed LowerFilters from HID\$($devNode.PSChildName)\$($instNode.PSChildName)"
    }
  }
}

& sc.exe delete $ServiceName 2>$null | Out-Null
$sysPath = Join-Path $env:WINDIR "System32\drivers\t1blehidf.sys"
if (Test-Path $sysPath) {
  Remove-Item $sysPath -Force -EA SilentlyContinue
  Log "deleted $sysPath"
}
Log "cleanup done removed=$removed"
Write-Output "Result: CLEANED_$removed"
exit 0
