# Pack WinUHid manual install zip for Release upload.
# Output: dist/WinUHid_Manual_<version>.zip

param(
    [string]$Version = "1.6.1"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$src = Join-Path $root "src-tauri\assets\winuhid"
$outDir = Join-Path $root "dist"
$zipName = "WinUHid_Manual_$Version.zip"
$zipPath = Join-Path $outDir $zipName
$staging = Join-Path $env:TEMP "WinUHid_Manual_staging_$Version"

if (-not (Test-Path $src)) { throw "Missing: $src" }
New-Item -ItemType Directory -Force -Path $outDir | Out-Null
if (Test-Path $staging) { Remove-Item -Recurse -Force $staging }
New-Item -ItemType Directory -Force -Path $staging | Out-Null
Copy-Item -Path (Join-Path $src "*") -Destination $staging -Recurse -Force

$requiredAscii = @(
  "Run-Install.cmd",
  "install-winuhid.ps1",
  "WinUHid.dll",
  "driver\WinUHidDriver.inf"
)
foreach ($rel in $requiredAscii) {
  if (-not (Test-Path (Join-Path $staging $rel))) {
    throw "Missing $rel in $src"
  }
}
if (-not (Get-ChildItem -LiteralPath $staging -Filter "*.txt" | Select-Object -First 1)) {
  throw "Missing install readme (*.txt) in $src"
}

if (Test-Path $zipPath) { Remove-Item -Force $zipPath }
Compress-Archive -Path (Join-Path $staging "*") -DestinationPath $zipPath -Force
Remove-Item -Recurse -Force $staging
Write-Host "Created $zipPath"
Get-Item $zipPath | Format-Table Name, Length
