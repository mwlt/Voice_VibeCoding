[CmdletBinding()]
param(
  [ValidateSet("Install", "InstallElevated", "Status")]
  [string] $Mode = "Install",
  [Parameter(Mandatory = $true)]
  [string] $PackageDir,
  [string] $DllSource = ""
)

$ErrorActionPreference = "Stop"
$StateRoot = Join-Path $env:LOCALAPPDATA "com.remote-bridge-hub.app\winuhid"
$RebootFlag = Join-Path $StateRoot "reboot-required.flag"

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
  const uint DICD_GENERATE_ID=0x1, SPDRP_HARDWAREID=0x1, DIF_REGISTERDEVICE=0x19, INSTALLFLAG_FORCE=0x1;
  static readonly IntPtr INVALID_HANDLE_VALUE=new IntPtr(-1);
  [StructLayout(LayoutKind.Sequential)] struct SP_DEVINFO_DATA { public uint cbSize; public Guid ClassGuid; public uint DevInst; public IntPtr Reserved; }
  [DllImport("setupapi.dll",SetLastError=true)] static extern IntPtr SetupDiCreateDeviceInfoList(ref Guid ClassGuid,IntPtr hwndParent);
  [DllImport("setupapi.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool SetupDiCreateDeviceInfo(IntPtr set,string name,ref Guid guid,string desc,IntPtr hwnd,uint flags,ref SP_DEVINFO_DATA data);
  [DllImport("setupapi.dll",SetLastError=true)] static extern bool SetupDiSetDeviceRegistryProperty(IntPtr set,ref SP_DEVINFO_DATA data,uint property,byte[] buffer,uint size);
  [DllImport("setupapi.dll",SetLastError=true)] static extern bool SetupDiCallClassInstaller(uint installFunction,IntPtr set,ref SP_DEVINFO_DATA data);
  [DllImport("setupapi.dll",SetLastError=true)] static extern bool SetupDiDestroyDeviceInfoList(IntPtr set);
  [DllImport("newdev.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool UpdateDriverForPlugAndPlayDevices(IntPtr hwnd,string hardwareId,string fullInfPath,uint flags,out bool reboot);
  static void Check(bool ok){if(!ok)throw new Win32Exception(Marshal.GetLastWin32Error());}
  public static bool Install(string infPath,string hardwareId,string description){
    // System class — matches WinUHidDriver.inf ClassGuid
    Guid systemClass=new Guid("4d36e97d-e325-11ce-bfc1-08002be10318");
    IntPtr set=SetupDiCreateDeviceInfoList(ref systemClass,IntPtr.Zero);
    if(set==INVALID_HANDLE_VALUE)throw new Win32Exception(Marshal.GetLastWin32Error());
    try {
      SP_DEVINFO_DATA data=new SP_DEVINFO_DATA(); data.cbSize=(uint)Marshal.SizeOf(typeof(SP_DEVINFO_DATA));
      Check(SetupDiCreateDeviceInfo(set,description,ref systemClass,description,IntPtr.Zero,DICD_GENERATE_ID,ref data));
      byte[] ids=Encoding.Unicode.GetBytes(hardwareId+"\0\0");
      Check(SetupDiSetDeviceRegistryProperty(set,ref data,SPDRP_HARDWAREID,ids,(uint)ids.Length));
      Check(SetupDiCallClassInstaller(DIF_REGISTERDEVICE,set,ref data));
      bool reboot; Check(UpdateDriverForPlugAndPlayDevices(IntPtr.Zero,hardwareId,System.IO.Path.GetFullPath(infPath),INSTALLFLAG_FORCE,out reboot));
      return reboot;
    } finally { SetupDiDestroyDeviceInfoList(set); }
  }
}
'@
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
      if (Test-WinUHidDevice) { $result = "OK" } else { $result = "WARNING: WinUHid device not accessible" }
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
      Install-PublisherCert $cer
      Deploy-UserDll

      $pnputil = Join-Path $env:SystemRoot "System32\pnputil.exe"
      & $pnputil /add-driver $inf /install
      $pnpu = $LASTEXITCODE
      if ($pnpu -notin @(0, 259, 3010)) {
        Write-Warning "pnputil exited $pnpu (continuing to bind Root\WinUHid)"
      }

      $devcon = $null
      foreach ($pattern in @(
        "C:\Program Files (x86)\Windows Kits\10\Tools\*\x64\devcon.exe",
        "C:\Program Files (x86)\Windows Kits\10\Tools\*\*\x64\devcon.exe"
      )) {
        $found = Get-Item $pattern -ErrorAction SilentlyContinue | Sort-Object FullName -Descending | Select-Object -First 1
        if ($found) { $devcon = $found.FullName; break }
      }

      $reboot = $false
      if ($devcon) {
        & $devcon install $inf "Root\WinUHid"
        if ($LASTEXITCODE -ne 0) {
          Write-Warning "devcon install exit=$LASTEXITCODE; trying SetupAPI fallback"
          Initialize-RootDeviceInstaller
          $reboot = [WinUHidRootInstaller]::Install($inf, "Root\WinUHid", "WinUHid Virtual HID Enumerator")
        }
      } else {
        Initialize-RootDeviceInstaller
        $reboot = [WinUHidRootInstaller]::Install($inf, "Root\WinUHid", "WinUHid Virtual HID Enumerator")
      }

      Start-Sleep -Seconds 2
      if ($reboot -or -not (Test-WinUHidDevice)) {
        # 再等一会儿：WUDFHost 拉起可能稍慢
        Start-Sleep -Seconds 3
      }
      if (-not (Test-WinUHidDevice)) {
        Set-Content -LiteralPath $RebootFlag -Value "reboot required" -Encoding ASCII
        exit 3010
      }
      Remove-Item -LiteralPath $RebootFlag -Force -ErrorAction SilentlyContinue
      exit 0
    }
    "Install" {
      Deploy-UserDll
      if (Test-WinUHidDevice) {
        $result = "OK"
        break
      }
      $code = Invoke-ElevatedInstall
      Start-Sleep -Seconds 2
      if (Test-WinUHidDevice) {
        Remove-Item -LiteralPath $RebootFlag -Force -ErrorAction SilentlyContinue
        $result = "OK"
      } elseif ($code -eq 3010 -or (Test-Path -LiteralPath $RebootFlag)) {
        $result = "Driver installed; Windows restart required"
      } else {
        $result = "WARNING: WinUHid driver installed but device not accessible yet"
      }
    }
  }
} catch {
  $result = "WARNING: $($_.Exception.Message)"
  if ($Mode -eq "InstallElevated") { exit 1 }
}

Write-Output "Result: $result"
if ($result -like "WARNING:*") { exit 1 }
if ($result -like "*restart required*") { exit 3010 }
exit 0
