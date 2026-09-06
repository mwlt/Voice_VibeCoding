# One-shot: trust test cert + enable testsigning (if allowed) + install t1blehidf.
param(
  [switch] $RunElevated
)

$ErrorActionPreference = "Stop"
$Root = if ($PSScriptRoot) { $PSScriptRoot } else { Split-Path -Parent $MyInvocation.MyCommand.Path }
$DriverDir = Join-Path $Root "driver"
$InstallPs1 = Join-Path $Root "install-t1-hid-filter.ps1"
$Log = Join-Path $env:TEMP "t1-l0-setup.log"
$Cer = Join-Path $env:TEMP "t1blehidf-test.cer"
$ThumbHint = "T1BleHidFilter Test"

function Log([string]$m) {
  $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m
  Add-Content -LiteralPath $Log -Value $line -ErrorAction SilentlyContinue
  Write-Host $line
}

function Test-IsAdmin {
  $id = [Security.Principal.WindowsIdentity]::GetCurrent()
  $p = New-Object Security.Principal.WindowsPrincipal($id)
  return $p.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Export-TestCert {
  $cert = Get-ChildItem Cert:\CurrentUser\My -CodeSigningCert -ErrorAction SilentlyContinue |
    Where-Object { $_.Subject -like "*$ThumbHint*" -and $_.NotAfter -gt (Get-Date) } |
    Select-Object -First 1
  if (-not $cert) {
    throw "Test signing cert '$ThumbHint' not found in CurrentUser\My. Rebuild driver first."
  }
  Export-Certificate -Cert $cert -FilePath $Cer -Force | Out-Null
  Log "Exported cert $($cert.Thumbprint) -> $Cer"
  return $cert.Thumbprint
}

function Import-TrustedCert([string]$CerPath) {
  if (-not (Test-Path -LiteralPath $CerPath)) { throw "cer missing: $CerPath" }
  $imported = Import-Certificate -FilePath $CerPath -CertStoreLocation Cert:\LocalMachine\Root
  Log "Trusted Root: $($imported.Thumbprint)"
  $imported2 = Import-Certificate -FilePath $CerPath -CertStoreLocation Cert:\LocalMachine\TrustedPublisher
  Log "Trusted Publisher: $($imported2.Thumbprint)"
}

function Get-SecureBootOn {
  try { return [bool](Confirm-SecureBootUEFI) } catch { return $false }
}

function Enable-TestSigning {
  $cur = (bcdedit /enum "{current}" 2>$null | Out-String)
  if ($cur -match "testsigning\s+Yes") {
    Log "testsigning already ON"
    return @{ Ok = $true; NeedReboot = $false; BlockedBySecureBoot = $false }
  }
  $out = & bcdedit.exe /set testsigning on 2>&1 | Out-String
  if ($LASTEXITCODE -eq 0) {
    Log "testsigning enabled (reboot required)"
    return @{ Ok = $true; NeedReboot = $true; BlockedBySecureBoot = $false }
  }
  $sb = Get-SecureBootOn
  Log "bcdedit testsigning FAILED: $out"
  if ($sb -or $out -match "Secure Boot") {
    Log "Blocked by Secure Boot — disable Secure Boot in UEFI/BIOS, then re-run this script"
    return @{ Ok = $false; NeedReboot = $false; BlockedBySecureBoot = $true }
  }
  return @{ Ok = $false; NeedReboot = $false; BlockedBySecureBoot = $false }
}

# --- main ---
"" | Set-Content -LiteralPath $Log
Log "L0 setup start"

if (-not $RunElevated) {
  $null = Export-TestCert
  if (-not (Test-IsAdmin)) {
    Log "Elevating via UAC..."
    $p = Start-Process -FilePath "powershell.exe" -Verb RunAs -ArgumentList @(
      "-NoProfile", "-ExecutionPolicy", "Bypass",
      "-File", $PSCommandPath,
      "-RunElevated"
    ) -Wait -PassThru
    Get-Content -LiteralPath $Log -ErrorAction SilentlyContinue
    exit $p.ExitCode
  }
}

if (-not (Test-IsAdmin)) {
  Log "NEED_ADMIN"
  exit 5
}

if (-not (Test-Path -LiteralPath $Cer)) {
  try { $null = Export-TestCert } catch {
    Log "WARN: $($_.Exception.Message)"
  }
}
if (Test-Path -LiteralPath $Cer) {
  Import-TrustedCert $Cer
}

$ts = Enable-TestSigning

if (-not (Test-Path -LiteralPath (Join-Path $DriverDir "t1blehidf.sys"))) {
  Log "NEED_BINARY: missing $DriverDir\t1blehidf.sys"
  Write-Output "Result: NEED_BINARY"
  exit 2
}

$FastInstall = Join-Path $Root "install-t1-hid-filter-fast.ps1"
Log "Installing filter (fast path, no Get-PnpDevice)..."
& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $FastInstall -PackageDir $DriverDir
$rc = $LASTEXITCODE
Log "Install exit=$rc"

if ($ts.BlockedBySecureBoot) {
  Log "Result: NEED_DISABLE_SECURE_BOOT"
  Write-Output "Result: NEED_DISABLE_SECURE_BOOT"
  # Install may have succeeded; kernel still won't load until Secure Boot off + testsigning on + reboot.
  exit 6
}

if ($ts.NeedReboot) {
  Log "Result: NEED_REBOOT"
  Write-Output "Result: NEED_REBOOT"
  exit 0
}

if ($rc -eq 0) {
  Log "Result: READY"
  Write-Output "Result: READY"
  exit 0
}

Log "Result: INSTALL_FAILED_$rc"
Write-Output "Result: INSTALL_FAILED_$rc"
exit $rc
