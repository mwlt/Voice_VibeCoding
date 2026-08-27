[CmdletBinding()]
param(
  [ValidateSet("Install", "InstallElevated", "Status")]
  [string] $Mode = "Install",
  [string] $PackageDir = "",
  [string] $DllSource = "",
  [switch] $Force
)

$ErrorActionPreference = "Stop"
$ScriptRoot = if ($PSScriptRoot) { $PSScriptRoot } else { Split-Path -Parent $MyInvocation.MyCommand.Path }

function Resolve-InstallPaths {
  param([string] $PackageDirIn, [string] $DllSourceIn)
  $pkg = $PackageDirIn
  if ([string]::IsNullOrWhiteSpace($pkg)) {
    $pkg = Join-Path $ScriptRoot "driver"
  } elseif (-not [System.IO.Path]::IsPathRooted($pkg)) {
    $pkg = Join-Path $ScriptRoot $pkg
  }
  if (-not (Test-Path -LiteralPath $pkg)) {
    throw "driver folder not found: $pkg`nRun from this folder in Admin PowerShell: .\install-winuhid.ps1"
  }
  $pkg = (Resolve-Path -LiteralPath $pkg).Path

  $dll = $DllSourceIn
  if ([string]::IsNullOrWhiteSpace($dll)) {
    $dll = Join-Path $ScriptRoot "WinUHid.dll"
  } elseif (-not [System.IO.Path]::IsPathRooted($dll)) {
    $dll = Join-Path $ScriptRoot $dll
  }
  if (-not (Test-Path -LiteralPath $dll)) {
    throw "WinUHid.dll not found: $dll"
  }
  return @{ PackageDir = $pkg; DllSource = $dll }
}

$resolved = Resolve-InstallPaths -PackageDirIn $PackageDir -DllSourceIn $DllSource
$PackageDir = $resolved.PackageDir
$DllSource = $resolved.DllSource

$StateRoot = Join-Path $env:LOCALAPPDATA "com.remote-bridge-hub.app\winuhid"
$RebootFlag = Join-Path $StateRoot "reboot-required.flag"
$HardwareId = "Root\WinUHid"
$DeviceDescription = "WinUHid Virtual HID Enumerator"

function Write-Phase([string] $Name, [string] $Detail) {
  Write-Output ("Phase: {0} | {1}" -f $Name, $Detail)
}

function Test-WinUHidDevice {
  try {
    $fs = [System.IO.File]::Open('\\.\WinUHid', 'Open', 'ReadWrite', 'ReadWrite')
    $fs.Close()
    return $true
  } catch {
    return $false
  }
}

function Initialize-RootDeviceInstaller {
  if ("WinUHidRootInstaller" -as [type]) { return }
  Add-Type -Language CSharp -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;
public static class WinUHidRootInstaller {
  const uint DICD_GENERATE_ID=0x1, SPDRP_HARDWAREID=0x1, DIF_REGISTERDEVICE=0x19;
  static readonly IntPtr INVALID_HANDLE_VALUE=new IntPtr(-1);
  [StructLayout(LayoutKind.Sequential)] struct SP_DEVINFO_DATA { public uint cbSize; public Guid ClassGuid; public uint DevInst; public IntPtr Reserved; }
  [DllImport("setupapi.dll",SetLastError=true)] static extern IntPtr SetupDiCreateDeviceInfoList(ref Guid ClassGuid,IntPtr hwndParent);
  [DllImport("setupapi.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool SetupDiCreateDeviceInfo(IntPtr set,string name,ref Guid guid,string desc,IntPtr hwnd,uint flags,ref SP_DEVINFO_DATA data);
  [DllImport("setupapi.dll",SetLastError=true)] static extern bool SetupDiSetDeviceRegistryProperty(IntPtr set,ref SP_DEVINFO_DATA data,uint property,byte[] buffer,uint size);
  [DllImport("setupapi.dll",SetLastError=true)] static extern bool SetupDiCallClassInstaller(uint installFunction,IntPtr set,ref SP_DEVINFO_DATA data);
  [DllImport("setupapi.dll",SetLastError=true)] static extern bool SetupDiDestroyDeviceInfoList(IntPtr set);
  static void Check(bool ok){if(!ok)throw new Win32Exception(Marshal.GetLastWin32Error());}
  public static void RegisterRootDevice(string hardwareId,string description){
    Guid systemClass=new Guid("4d36e97d-e325-11ce-bfc1-08002be10318");
    IntPtr set=SetupDiCreateDeviceInfoList(ref systemClass,IntPtr.Zero);
    if(set==INVALID_HANDLE_VALUE)throw new Win32Exception(Marshal.GetLastWin32Error());
    try {
      SP_DEVINFO_DATA data=new SP_DEVINFO_DATA(); data.cbSize=(uint)Marshal.SizeOf(typeof(SP_DEVINFO_DATA));
      Check(SetupDiCreateDeviceInfo(set,description,ref systemClass,description,IntPtr.Zero,DICD_GENERATE_ID,ref data));
      byte[] ids=Encoding.Unicode.GetBytes(hardwareId+"\0\0");
      Check(SetupDiSetDeviceRegistryProperty(set,ref data,SPDRP_HARDWAREID,ids,(uint)ids.Length));
      Check(SetupDiCallClassInstaller(DIF_REGISTERDEVICE,set,ref data));
    } finally { SetupDiDestroyDeviceInfoList(set); }
  }
}
'@
}

function Test-RootDeviceNodeListed([string] $PnputilPath) {
  try {
    $text = (& $PnputilPath /enum-devices /instanceid $HardwareId 2>&1 | Out-String)
    return ($text -match [regex]::Escape($HardwareId))
  } catch {
    return $false
  }
}

function Invoke-PnputilPhase {
  param(
    [Parameter(Mandatory = $true)][string] $PnputilPath,
    [Parameter(Mandatory = $true)][string] $PhaseName,
    [Parameter(Mandatory = $true)][string[]] $Arguments,
    [int[]] $AllowedExitCodes = @(0)
  )
  $argLine = ($Arguments -join ' ')
  Write-Phase $PhaseName "running pnputil $argLine"
  & $PnputilPath @Arguments
  $code = $LASTEXITCODE
  if ($AllowedExitCodes -contains $code) {
    Write-Phase $PhaseName "exit=$code OK"
    return $code
  }
  throw "pnputil $argLine failed with exit code $code"
}

function Register-RootDeviceNode([string] $InfPath, [string] $PnputilPath) {
  if (Test-RootDeviceNodeListed $PnputilPath) {
    Write-Phase "RegisterRoot" "node already listed ($HardwareId)"
    return
  }

  $devcon = $null
  foreach ($pattern in @(
    "C:\Program Files (x86)\Windows Kits\10\Tools\*\x64\devcon.exe",
    "C:\Program Files (x86)\Windows Kits\10\Tools\*\*\x64\devcon.exe"
  )) {
    $found = Get-Item $pattern -ErrorAction SilentlyContinue | Sort-Object FullName -Descending | Select-Object -First 1
    if ($found) { $devcon = $found.FullName; break }
  }

  if ($devcon) {
    Write-Phase "RegisterRoot" "trying devcon fast-path"
    & $devcon install $InfPath $HardwareId
    $devconCode = $LASTEXITCODE
    if ($devconCode -eq 0) {
      Write-Phase "RegisterRoot" "devcon exit=0 OK"
      return
    }
    Write-Phase "RegisterRoot" "devcon exit=$devconCode; falling back to SetupAPI DIF_REGISTERDEVICE"
  } else {
    Write-Phase "RegisterRoot" "devcon not found; using SetupAPI DIF_REGISTERDEVICE"
  }

  Initialize-RootDeviceInstaller
  try {
    [WinUHidRootInstaller]::RegisterRootDevice($HardwareId, $DeviceDescription)
    Write-Phase "RegisterRoot" "SetupAPI DIF_REGISTERDEVICE OK"
  } catch {
    if (Test-RootDeviceNodeListed $PnputilPath) {
      Write-Phase "RegisterRoot" "SetupAPI reported error but node now listed; continuing ($($_.Exception.Message))"
      return
    }
    throw
  }
}

function Bind-AndPresentRootDevice([string] $InfPath, [string] $PnputilPath) {
  $reboot = $false
  $installCode = Invoke-PnputilPhase -PnputilPath $PnputilPath -PhaseName "BindDriver" -Arguments @('/add-driver', $InfPath, '/install') -AllowedExitCodes @(0, 259, 3010)
  if ($installCode -eq 3010) { $reboot = $true }

  Invoke-PnputilPhase -PnputilPath $PnputilPath -PhaseName "ScanDevices" -Arguments @('/scan-devices') -AllowedExitCodes @(0)
  return $reboot
}

function Wait-WinUHidReady {
  param([int] $Attempts = 12, [int] $DelayMs = 500)
  for ($i = 0; $i -lt $Attempts; $i++) {
    if (Test-WinUHidDevice) { return $true }
    Start-Sleep -Milliseconds $DelayMs
  }
  return $false
}

function Install-PublisherCert([string] $CerPath) {
  if (-not (Test-Path -LiteralPath $CerPath)) { return }
  $cert = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2($CerPath)
  foreach ($storeName in @('Root', 'TrustedPublisher')) {
    $store = New-Object System.Security.Cryptography.X509Certificates.X509Store($storeName, 'LocalMachine')
    $store.Open('ReadWrite')
    try {
      $exists = $store.Certificates | Where-Object { $_.Thumbprint -eq $cert.Thumbprint }
      if (-not $exists) { $store.Add($cert) }
    } finally { $store.Close() }
  }
}

function Deploy-UserDll {
  if ([string]::IsNullOrWhiteSpace($DllSource) -or -not (Test-Path -LiteralPath $DllSource)) { return }
  $targets = @()
  if ($env:REMOTE_BRIDGE_WINUHID_DLL_DIR) { $targets += $env:REMOTE_BRIDGE_WINUHID_DLL_DIR }
  $targets += (Join-Path $env:LOCALAPPDATA "com.remote-bridge-hub.app\winuhid")
  foreach ($dir in $targets) {
    $null = New-Item -ItemType Directory -Force -Path $dir
    Copy-Item -LiteralPath $DllSource -Destination (Join-Path $dir "WinUHid.dll") -Force
  }
}

function Invoke-ElevatedInstall {
  $args = '-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File "{0}" -Mode InstallElevated -PackageDir "{1}" -DllSource "{2}"' -f $PSCommandPath, $PackageDir, $DllSource
  $process = Start-Process -FilePath "powershell.exe" -ArgumentList $args -Verb RunAs -WindowStyle Hidden -PassThru -Wait
  if ($null -eq $process) { throw "UAC cancelled or elevated WinUHid install did not start" }
  if ($process.ExitCode -notin @(0, 3010)) { throw "WinUHid driver install failed with code $($process.ExitCode)" }
  return $process.ExitCode
}

$result = "OK"
try {
  $null = New-Item -ItemType Directory -Force -Path $StateRoot
  switch ($Mode) {
    "Status" {
      if (Test-WinUHidDevice) {
        Write-Phase "Verify" "device reachable"
        $result = "OK"
      } else {
        Write-Phase "Verify" "device not accessible"
        $result = "WARNING: WinUHid device not accessible"
      }
    }
    "InstallElevated" {
      if (-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw "Administrator rights are required"
      }
      $inf = Join-Path $PackageDir "WinUHidDriver.inf"
      $dll = Join-Path $PackageDir "WinUHidDriver.dll"
      $cat = Join-Path $PackageDir "WinUHidDriver.cat"
      foreach ($f in @($inf, $dll, $cat)) {
        if (-not (Test-Path -LiteralPath $f)) { throw "Missing driver package file: $f" }
      }
      $cer = Join-Path (Split-Path -Parent $PackageDir) "WinUHidPublisher.cer"
      if (-not (Test-Path -LiteralPath $cer)) {
        $cer = Join-Path $PackageDir "WinUHidPublisher.cer"
      }

      Write-Phase "Prepare" "install publisher cert + deploy dll"
      Install-PublisherCert $cer
      Deploy-UserDll

      $pnputil = Join-Path $env:SystemRoot "System32\pnputil.exe"
      if (-not (Test-Path -LiteralPath $pnputil)) {
        throw "pnputil.exe not found"
      }

      # Phase 1: stage driver package into driver store (no device bind yet)
      Invoke-PnputilPhase -PnputilPath $pnputil -PhaseName "StageDriver" -Arguments @('/add-driver', $inf) -AllowedExitCodes @(0, 259, 3010) | Out-Null

      # Phase 2: ensure Root\WinUHid phantom root node exists (DIF_REGISTERDEVICE only)
      Register-RootDeviceNode -InfPath $inf -PnputilPath $pnputil

      # Phase 3: bind driver to node + force PnP rescan (device becomes present/started)
      $reboot = Bind-AndPresentRootDevice -InfPath $inf -PnputilPath $pnputil

      # Phase 4: verify \\.\WinUHid
      Write-Phase "Verify" "waiting for device"
      if (-not (Wait-WinUHidReady)) {
        if ($reboot) {
          Set-Content -LiteralPath $RebootFlag -Value "reboot required" -Encoding ASCII
          Write-Phase "Verify" "not reachable; reboot flag set (exit 3010)"
          exit 3010
        }
        Set-Content -LiteralPath $RebootFlag -Value "reboot required" -Encoding ASCII
        Write-Phase "Verify" "not reachable after bind+scan; reboot may be required (exit 3010)"
        exit 3010
      }

      Remove-Item -LiteralPath $RebootFlag -Force -ErrorAction SilentlyContinue
      Write-Phase "Verify" "device reachable (exit 0)"
      exit 0
    }
    "Install" {
      Deploy-UserDll
      if ((Test-WinUHidDevice) -and (-not $Force)) {
        Write-Phase "Verify" "already reachable (use -Force to rerun full install)"
        $result = "OK"
        break
      }
      if ($Force -and (Test-WinUHidDevice)) {
        Write-Phase "Install" "force reinstall requested"
      }
      $code = Invoke-ElevatedInstall
      Start-Sleep -Seconds 1
      if (Test-WinUHidDevice) {
        Remove-Item -LiteralPath $RebootFlag -Force -ErrorAction SilentlyContinue
        Write-Phase "Verify" "reachable after elevated install"
        $result = "OK"
      } elseif ($code -eq 3010 -or (Test-Path -LiteralPath $RebootFlag)) {
        Write-Phase "Verify" "restart required"
        $result = "Driver installed; Windows restart required"
      } else {
        Write-Phase "Verify" "not reachable after elevated install"
        $result = "WARNING: WinUHid driver installed but device not accessible yet"
      }
    }
  }
} catch {
  $msg = $_.Exception.Message
  Write-Phase "Error" $msg
  $result = "WARNING: $msg"
  if ($Mode -eq "InstallElevated") { exit 1 }
}

Write-Output "Result: $result"
if ($result -like "WARNING:*") { exit 1 }
if ($result -like "*restart required*") { exit 3010 }
exit 0
