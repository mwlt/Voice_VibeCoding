# Build WinUHid user DLL + UMDF driver and copy into src-tauri/assets/winuhid
# Prerequisites: VS 2022 Build Tools + WDK (Windows Kits 10), PlatformToolset WindowsUserModeDriver10.0
param(
  [string]$Configuration = "Release",
  [string]$Platform = "x64"
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$WinUHid = Join-Path $Root "_third_party\WinUHid-main"
$Assets = Join-Path $Root "src-tauri\assets\winuhid"
$VsTool = Join-Path $WinUHid "scripts\invoke-vs-tool.ps1"

if (-not (Test-Path $WinUHid)) {
  throw "Missing $WinUHid — extract cgutman/WinUHid sources first."
}

$env:WDKContentRoot = "C:\Program Files (x86)\Windows Kits\10\"
$env:WDKBuildFolder = "10.0.28000.0"

& $VsTool msbuild (Join-Path $WinUHid "WinUHid\WinUHid.vcxproj") /p:Configuration=$Configuration /p:Platform=$Platform /m /verbosity:minimal
$dll = Join-Path $WinUHid "WinUHid\x64\$Configuration\WinUHid.dll"
if (-not (Test-Path $dll)) {
  $dll = Get-ChildItem $WinUHid -Recurse -Filter WinUHid.dll | Where-Object { $_.FullName -match $Configuration } | Select-Object -First 1 -ExpandProperty FullName
}
Copy-Item $dll (Join-Path $Assets "WinUHid.dll") -Force

$rsp = Join-Path $Root "_third_party\wdk\build-driver.rsp"
& "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\MSBuild\Current\Bin\MSBuild.exe" "@$rsp"

$driverOut = Join-Path $WinUHid "WinUHid Driver\build\$Configuration\$Platform"
$driverAssets = Join-Path $Assets "driver"
New-Item -ItemType Directory -Force -Path $driverAssets | Out-Null
Copy-Item (Join-Path $driverOut "WinUHidDriver.dll") $driverAssets -Force
$inf = Join-Path $WinUHid "WinUHid Driver\$Platform\$Configuration\WinUHidDriver.inf"
if (Test-Path $inf) { Copy-Item $inf $driverAssets -Force }

Write-Host "Copied WinUHid.dll + driver package into $Assets"
Write-Host "Sign/catalog: run _third_party/wdk/sign-driver-package.ps1 elevated if needed."
