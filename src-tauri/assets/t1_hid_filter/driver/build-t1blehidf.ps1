# Build t1blehidf.sys via WDK NuGet (no classic WDK MSI required).
param(
  [ValidateSet("Release", "Debug")]
  [string] $Configuration = "Release"
)

$ErrorActionPreference = "Stop"
$DriverRoot = $PSScriptRoot
$Nuget = Join-Path $DriverRoot "nuget.exe"
$PackagesConfig = Join-Path $DriverRoot "packages.config"
$Proj = Join-Path $DriverRoot "src\t1blehidf.vcxproj"
$OutSys = Join-Path $DriverRoot "out\x64\$Configuration\t1blehidf.sys"
$DestSys = Join-Path $DriverRoot "t1blehidf.sys"

if (-not (Test-Path $Nuget)) {
  Invoke-WebRequest -Uri "https://dist.nuget.org/win-x86-commandline/latest/nuget.exe" -OutFile $Nuget -UseBasicParsing
}

Write-Host "Restoring WDK NuGet packages..."
& $Nuget restore $PackagesConfig -PackagesDirectory (Join-Path $DriverRoot "packages") -NonInteractive
if ($LASTEXITCODE -ne 0) { throw "nuget restore failed: $LASTEXITCODE" }

$vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
# Prefer 64-bit MSBuild so WDK tool path selection uses x64 stampinf/etc from NuGet.
$msbCandidates = @()
if (Test-Path $vswhere) {
  $msbCandidates += & $vswhere -latest -products * -requires Microsoft.Component.MSBuild -find "MSBuild\**\Bin\amd64\MSBuild.exe"
  $msbCandidates += & $vswhere -latest -products * -requires Microsoft.Component.MSBuild -find "MSBuild\**\Bin\MSBuild.exe"
}
$msbCandidates += @(
  (Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\18\BuildTools\MSBuild\Current\Bin\amd64\MSBuild.exe"),
  (Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\2022\BuildTools\MSBuild\Current\Bin\amd64\MSBuild.exe"),
  (Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\18\BuildTools\MSBuild\Current\Bin\MSBuild.exe"),
  (Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\2022\BuildTools\MSBuild\Current\Bin\MSBuild.exe")
)
$msb = $msbCandidates | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
if (-not $msb) { throw "MSBuild not found" }
Write-Host "MSBuild: $msb"
$wdkBinX64 = Join-Path $DriverRoot "packages\Microsoft.Windows.WDK.x64.10.0.26100.6584\c\bin\10.0.26100.0\x64"
if (Test-Path $wdkBinX64) {
  $env:PATH = "$wdkBinX64;$env:PATH"
}

Push-Location $DriverRoot
try {
  # SignMode=Off: WDK test-sign step fails on modern signtool without /fd; we sign below.
  & $msb $Proj /t:Rebuild /p:Configuration=$Configuration /p:Platform=x64 /p:SignMode=Off /p:EnableTestSign=false /m /v:minimal
  if ($LASTEXITCODE -ne 0) { throw "msbuild failed: $LASTEXITCODE" }
} finally {
  Pop-Location
}

if (-not (Test-Path $OutSys)) {
  # fallback search
  $found = Get-ChildItem (Join-Path $DriverRoot "out") -Recurse -Filter "t1blehidf.sys" -ErrorAction SilentlyContinue |
    Select-Object -First 1 -ExpandProperty FullName
  if (-not $found) { throw "t1blehidf.sys not produced" }
  $OutSys = $found
}

Copy-Item -LiteralPath $OutSys -Destination $DestSys -Force

# Optional test-sign (needed to load the filter unless Secure Boot + production WHQL).
$signTool = @(
  (Join-Path ${env:ProgramFiles(x86)} "Windows Kits\10\bin\10.0.26100.0\x64\signtool.exe"),
  (Join-Path ${env:ProgramFiles(x86)} "Windows Kits\10\bin\10.0.28000.0\x64\signtool.exe")
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if ($signTool) {
  $certStore = "Cert:\CurrentUser\My"
  $cert = Get-ChildItem $certStore -CodeSigningCert -ErrorAction SilentlyContinue |
    Where-Object { $_.Subject -like "*T1BleHidFilter Test*" -and $_.NotAfter -gt (Get-Date) } |
    Select-Object -First 1
  if (-not $cert) {
    $cert = New-SelfSignedCertificate `
      -Type CodeSigningCert `
      -Subject "CN=T1BleHidFilter Test" `
      -CertStoreLocation $certStore `
      -KeyExportPolicy Exportable `
      -HashAlgorithm SHA256 `
      -NotAfter (Get-Date).AddYears(5)
    Write-Host "Created test code-signing cert: $($cert.Thumbprint)"
  }
  & $signTool sign /fd SHA256 /sha1 $cert.Thumbprint /v $DestSys
  if ($LASTEXITCODE -ne 0) {
    Write-Warning "signtool failed ($LASTEXITCODE); $DestSys left unsigned"
  } else {
    Write-Host "Test-signed: $DestSys"
  }
} else {
  Write-Warning "signtool.exe not found; $DestSys left unsigned"
}

Write-Host "OK: $DestSys"
Get-Item $DestSys | Format-List FullName, Length, LastWriteTime
