# Manual install launcher (ASCII-only for cmd/PowerShell 5.1 encoding safety)
$ErrorActionPreference = "Continue"
$root = $PSScriptRoot
Write-Host ""
Write-Host "Voice VibeCoding - WinUHid install"
Write-Host "UAC prompt will appear - click Yes."
Write-Host "Do not close this window during install."
Write-Host ""

& (Join-Path $root "install-winuhid.ps1") -Mode Install
$rc = $LASTEXITCODE

Write-Host ""
if ($rc -eq 0) {
  Write-Host "OK: WinUHid ready. Check virtual keyboard status in the app." -ForegroundColor Green
} elseif ($rc -eq 3010) {
  Write-Host "REBOOT REQUIRED: Restart Windows, then run Run-Status.cmd" -ForegroundColor Yellow
} else {
  Write-Host "FAILED: exit code $rc. Copy all Phase: lines above for support." -ForegroundColor Red
}
Write-Host ""
exit $rc
