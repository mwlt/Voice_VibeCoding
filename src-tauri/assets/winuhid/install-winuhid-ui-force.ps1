# Force full reinstall even when WinUHid is already reachable (dev / QA)
param([switch] $Force)
$ErrorActionPreference = "Continue"
$root = $PSScriptRoot
Write-Host ""
Write-Host "Voice VibeCoding - WinUHid FORCE reinstall"
Write-Host "Runs full elevated install even if device is already OK."
Write-Host "UAC prompt will appear - click Yes."
Write-Host ""

$params = @{ Mode = "Install"; Force = $true }
& (Join-Path $root "install-winuhid.ps1") @params
$rc = $LASTEXITCODE

Write-Host ""
if ($rc -eq 0) {
  Write-Host "OK: Force reinstall finished." -ForegroundColor Green
} elseif ($rc -eq 3010) {
  Write-Host "REBOOT REQUIRED: Restart Windows, then run Run-Status.cmd" -ForegroundColor Yellow
} else {
  Write-Host "FAILED: exit code $rc. Copy all Phase: lines above for support." -ForegroundColor Red
}
Write-Host ""
exit $rc
