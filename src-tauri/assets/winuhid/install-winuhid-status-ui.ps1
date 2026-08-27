# Status check launcher (ASCII-only for encoding safety)
$ErrorActionPreference = "Continue"
$root = $PSScriptRoot
Write-Host ""
Write-Host "Checking WinUHid virtual keyboard..."
Write-Host ""

& (Join-Path $root "install-winuhid.ps1") -Mode Status
$rc = $LASTEXITCODE

Write-Host ""
if ($rc -eq 0) {
  Write-Host "OK: WinUHid device is reachable." -ForegroundColor Green
} else {
  Write-Host "NOT READY: Run Run-Install.cmd again or check Phase output above." -ForegroundColor Yellow
}
Write-Host ""
exit $rc
