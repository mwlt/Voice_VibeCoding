[CmdletBinding()]
param(
  [ValidateSet("Install", "InstallElevated", "Finish", "Repair", "Restore", "Audit")]
  [string] $Mode = "Install",
  [Parameter(Mandatory = $true)]
  [string] $AppPath,
  [Parameter(Mandatory = $true)]
  [string] $DriverZipPath
)

$ErrorActionPreference = "Stop"
$ExpectedZipSha256 = "b950e39f01af1d04ea623c8f6d8eb9b6ea5c477c637295fabf20631c85116bfb"
$StateRoot = Join-Path $env:LOCALAPPDATA "2655AI\BridgeAudio\XiaomiRemoteBridge"
$DriverRoot = Join-Path $StateRoot "VB-CABLE"
$PreviousMicFile = Join-Path $StateRoot "previous-default-microphone.txt"
$RebootFlag = Join-Path $StateRoot "reboot-required.flag"
$RunOnceKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\RunOnce"
$RunOnceName = "XiaomiRemoteBridgeAudioFinish"
function Get-VBCableEndpoint([string] $Flow, [string] $Prefix, [string] $Pattern) {
  $root = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\$Flow"
  foreach ($key in Get-ChildItem -LiteralPath $root -ErrorAction SilentlyContinue) {
    $state = (Get-ItemProperty -LiteralPath $key.PSPath -Name DeviceState -ErrorAction SilentlyContinue).DeviceState
    if ($null -ne $state -and [int] $state -ne 1) { continue }
    $props = Get-ItemProperty -LiteralPath (Join-Path $key.PSPath "Properties") -ErrorAction SilentlyContinue
    $name = "$($props.'{a45c254e-df1c-4efd-8020-67d146a850e0},2') $($props.'{b3f8fa53-0004-438e-9003-51a46e139bfc},6')".Trim()
    if ($name -match $Pattern) {
      return [pscustomobject]@{ Id = "$Prefix.$($key.PSChildName)"; Name = $name }
    }
  }
  return $null
}

function Get-VBCableCapture { Get-VBCableEndpoint "Capture" "{0.0.1.00000000}" "(?i)(^|\s)CABLE Output(\s|$)" }
function Get-VBCableRender { Get-VBCableEndpoint "Render" "{0.0.0.00000000}" "(?i)(^|\s)CABLE Input(\s|$)" }
function Test-VBCableReady { return [bool](Get-VBCableCapture) -and [bool](Get-VBCableRender) }

function Initialize-AudioEndpointApi {
  if ("XiaomiAudioEndpoint" -as [type]) { return }
  Add-Type -Language CSharp -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public enum EDataFlow { eRender = 0, eCapture = 1, eAll = 2 }
public enum ERole { eConsole = 0, eMultimedia = 1, eCommunications = 2 }
[ComImport, Guid("BCDE0395-E52F-467C-8E3D-C4579291692E")] internal class MMDeviceEnumeratorComObject {}
[ComImport, Guid("A95664D2-9614-4F35-A746-DE8DB63617E6"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
internal interface IMMDeviceEnumerator {
  int EnumAudioEndpoints(EDataFlow flow, uint mask, out IntPtr devices);
  int GetDefaultAudioEndpoint(EDataFlow flow, ERole role, out IMMDevice device);
  int GetDevice(string id, out IMMDevice device);
  int RegisterEndpointNotificationCallback(IntPtr client);
  int UnregisterEndpointNotificationCallback(IntPtr client);
}
[ComImport, Guid("D666063F-1587-4E43-81F1-B948E807363F"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
internal interface IMMDevice {
  int Activate(ref Guid iid, uint context, IntPtr args, out IntPtr instance);
  int OpenPropertyStore(uint access, out IntPtr properties);
  int GetId([MarshalAs(UnmanagedType.LPWStr)] out string id);
  int GetState(out uint state);
}
[ComImport, Guid("870AF99C-171D-4F9E-AF0D-E63DF40C2BC9"), ClassInterface(ClassInterfaceType.None)] internal class PolicyConfigClient {}
[ComImport, Guid("F8679F50-850A-41CF-9C72-430F290290C8"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
internal interface IPolicyConfig {
  int GetMixFormat(string d, out IntPtr f); int GetDeviceFormat(string d, int x, out IntPtr f);
  int ResetDeviceFormat(string d); int SetDeviceFormat(string d, IntPtr a, IntPtr b);
  int GetProcessingPeriod(string d, int x, out long a, out long b); int SetProcessingPeriod(string d, ref long p);
  int GetShareMode(string d, IntPtr m); int SetShareMode(string d, IntPtr m);
  int GetPropertyValue(string d, IntPtr k, IntPtr v); int SetPropertyValue(string d, IntPtr k, IntPtr v);
  int SetDefaultEndpoint([MarshalAs(UnmanagedType.LPWStr)] string d, ERole role); int SetEndpointVisibility(string d, int v);
}
public static class XiaomiAudioEndpoint {
  public static string GetDefaultCapture() {
    IMMDeviceEnumerator e = (IMMDeviceEnumerator)new MMDeviceEnumeratorComObject(); IMMDevice d = null;
    try { int hr=e.GetDefaultAudioEndpoint(EDataFlow.eCapture,ERole.eMultimedia,out d); if(hr!=0)Marshal.ThrowExceptionForHR(hr); string id; hr=d.GetId(out id); if(hr!=0)Marshal.ThrowExceptionForHR(hr); return id; }
    finally { if(d!=null)Marshal.ReleaseComObject(d); Marshal.ReleaseComObject(e); }
  }
  public static void SetDefaultCapture(string id) {
    IPolicyConfig c=(IPolicyConfig)new PolicyConfigClient(); try { for(int r=0;r<3;r++){int hr=c.SetDefaultEndpoint(id,(ERole)r);if(hr!=0)Marshal.ThrowExceptionForHR(hr);} } finally { Marshal.ReleaseComObject(c); }
  }
}
'@
}

function Initialize-RootDeviceInstaller {
  if ("RootDeviceInstaller" -as [type]) { return }
  Add-Type -Language CSharp -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;
public static class RootDeviceInstaller {
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
    Guid media=new Guid("4d36e96c-e325-11ce-bfc1-08002be10318"); IntPtr set=SetupDiCreateDeviceInfoList(ref media,IntPtr.Zero);
    if(set==INVALID_HANDLE_VALUE)throw new Win32Exception(Marshal.GetLastWin32Error());
    try {
      SP_DEVINFO_DATA data=new SP_DEVINFO_DATA(); data.cbSize=(uint)Marshal.SizeOf(typeof(SP_DEVINFO_DATA));
      Check(SetupDiCreateDeviceInfo(set,description,ref media,description,IntPtr.Zero,DICD_GENERATE_ID,ref data));
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

function Prepare-DriverFiles {
  if (-not (Test-Path -LiteralPath $DriverZipPath)) { throw "VB-CABLE driver package is missing" }
  $hash = (Get-FileHash -LiteralPath $DriverZipPath -Algorithm SHA256).Hash.ToLowerInvariant()
  if ($hash -ne $ExpectedZipSha256) { throw "VB-CABLE package hash mismatch" }
  $safeRoot = [IO.Path]::GetFullPath($StateRoot).TrimEnd('\') + '\'
  $fullDriverRoot = [IO.Path]::GetFullPath($DriverRoot)
  if (-not $fullDriverRoot.StartsWith($safeRoot, [StringComparison]::OrdinalIgnoreCase)) { throw "Unsafe driver staging path" }
  if (Test-Path -LiteralPath $fullDriverRoot) { Remove-Item -LiteralPath $fullDriverRoot -Recurse -Force }
  $null = New-Item -ItemType Directory -Force -Path $DriverRoot
  Expand-Archive -LiteralPath $DriverZipPath -DestinationPath $DriverRoot -Force
  $inf = Join-Path $DriverRoot "vbMmeCable64_win10.inf"
  $cat = Join-Path $DriverRoot "vbaudio_cable64_win10.cat"
  if (-not (Test-Path -LiteralPath $inf) -or -not (Test-Path -LiteralPath $cat)) { throw "Signed VB-CABLE Windows 10 driver files are missing" }
  $signature = Get-AuthenticodeSignature -LiteralPath $cat
  if ($signature.Status -ne "Valid" -or $signature.SignerCertificate.Subject -notmatch "BUREL VINCENT") { throw "VB-CABLE catalog signature is invalid" }
  return $inf
}

function Set-DefaultCableMicrophone {
  $capture = Get-VBCableCapture
  if (-not $capture) { throw "CABLE Output is not available" }
  Initialize-AudioEndpointApi
  $current = [XiaomiAudioEndpoint]::GetDefaultCapture()
  if (-not (Test-Path -LiteralPath $PreviousMicFile) -and $current -ne $capture.Id) {
    Set-Content -LiteralPath $PreviousMicFile -Value $current -Encoding UTF8
  }
  [XiaomiAudioEndpoint]::SetDefaultCapture($capture.Id)
  $root = "HKCU:\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone"
  $desktop = Join-Path $root "NonPackaged"
  $null = New-Item -ItemType Directory -Force -Path $root, $desktop
  Set-ItemProperty -LiteralPath $root -Name Value -Value Allow -Type String
  Set-ItemProperty -LiteralPath $desktop -Name Value -Value Allow -Type String
}

function Set-FinishRunOnce {
  $null = New-Item -Path $RunOnceKey -Force
  $command = 'powershell.exe -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File "{0}" -Mode Finish -AppPath "{1}" -DriverZipPath "{2}"' -f $PSCommandPath,$AppPath,$DriverZipPath
  New-ItemProperty -Path $RunOnceKey -Name $RunOnceName -Value $command -PropertyType String -Force | Out-Null
}

function Invoke-ElevatedInstall {
  $args = '-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File "{0}" -Mode InstallElevated -AppPath "{1}" -DriverZipPath "{2}"' -f $PSCommandPath,$AppPath,$DriverZipPath
  $process = Start-Process -FilePath "powershell.exe" -ArgumentList $args -Verb RunAs -WindowStyle Hidden -PassThru -Wait
  if ($null -eq $process) { throw "UAC cancelled or elevated install did not start" }
  if ($process.ExitCode -notin @(0, 3010)) { throw "Automatic VB-CABLE install failed with code $($process.ExitCode)" }
}

function Wait-VBCable([int] $Seconds) {
  $until = (Get-Date).AddSeconds($Seconds)
  do {
    if (Test-VBCableReady) { return $true }
    Start-Sleep -Milliseconds 1000
  } while ((Get-Date) -lt $until)
  return $false
}

$result = "OK"
try {
  $null = New-Item -ItemType Directory -Force -Path $StateRoot
  switch ($Mode) {
    "InstallElevated" {
      if (-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw "Administrator rights are required" }
      $inf = Prepare-DriverFiles
      Initialize-RootDeviceInstaller
      $reboot = [RootDeviceInstaller]::Install($inf, "VBAudioVACWDM", "VB-Audio Virtual Cable")
      if ($reboot) { Set-Content -LiteralPath $RebootFlag -Value "reboot required" -Encoding ASCII; exit 3010 }
      exit 0
    }
    "Install" {
      if (-not (Test-VBCableReady)) { Invoke-ElevatedInstall }
      if (Wait-VBCable 45) { Set-DefaultCableMicrophone; Remove-Item -LiteralPath $RebootFlag -Force -ErrorAction SilentlyContinue }
      else { Set-Content -LiteralPath $RebootFlag -Value "reboot required" -Encoding ASCII; Set-FinishRunOnce; $result = "Driver installed; Windows restart required" }
    }
    "Finish" {
      if (Wait-VBCable 60) { Set-DefaultCableMicrophone; Remove-Item -LiteralPath $RebootFlag -Force -ErrorAction SilentlyContinue; Remove-ItemProperty -Path $RunOnceKey -Name $RunOnceName -Force -ErrorAction SilentlyContinue }
      else { throw "VB-CABLE endpoints are still unavailable after restart" }
    }
    "Repair" {
      if (-not (Test-VBCableReady)) { Invoke-ElevatedInstall }
      if (Wait-VBCable 45) { Set-DefaultCableMicrophone } else { Set-FinishRunOnce; $result = "Driver installed; Windows restart required" }
    }
    "Restore" {
      if (Test-Path -LiteralPath $PreviousMicFile) { Initialize-AudioEndpointApi; $id=(Get-Content -LiteralPath $PreviousMicFile -Raw -Encoding UTF8).Trim(); if($id){[XiaomiAudioEndpoint]::SetDefaultCapture($id)}; Remove-Item -LiteralPath $PreviousMicFile -Force }
      Remove-ItemProperty -Path $RunOnceKey -Name $RunOnceName -Force -ErrorAction SilentlyContinue
      $result = "Previous microphone restored; VB-CABLE retained"
    }
    "Audit" { if (-not (Test-VBCableReady)) { throw "VB-CABLE is not ready" } }
  }
} catch {
  $result = "WARNING: $($_.Exception.Message)"
}

# Emit result lines on stdout for the host app to parse (no desktop file / no MessageBox).
@(
  "Xiaomi Remote Bridge audio check",
  "Time: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')",
  "Mode: $Mode",
  "Result: $result",
  "VB-CABLE render: $([bool](Get-VBCableRender))",
  "VB-CABLE capture: $([bool](Get-VBCableCapture))",
  "Driver install: automatic signed root-device installation",
  "Input method or speech recognition: not included"
) | ForEach-Object { Write-Output $_ }

exit 0
