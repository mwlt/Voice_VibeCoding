# Live probe: inject VK_BROWSER_SEARCH (0xAA) without EXTRA_INFO.
# Pass = Search/Start stay down (T1 LL/hotkey swallowed it).
$ErrorActionPreference = "Stop"

Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class T1AaProbe {
  [DllImport("user32.dll")]
  public static extern void keybd_event(byte bVk, byte bScan, uint dwFlags, UIntPtr dwExtraInfo);
  [DllImport("user32.dll")]
  public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")]
  public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)]
  public static extern int GetClassName(IntPtr hWnd, StringBuilder lpClassName, int nMaxCount);
  const uint KEYEVENTF_KEYUP = 2;
  public static void TapBrowserSearch() {
    keybd_event(0xAA, 0, 0, UIntPtr.Zero);
    keybd_event(0xAA, 0, KEYEVENTF_KEYUP, UIntPtr.Zero);
  }
  public static string ForegroundHint() {
    var hwnd = GetForegroundWindow();
    if (hwnd == IntPtr.Zero) return "none";
    uint pid;
    GetWindowThreadProcessId(hwnd, out pid);
    var cls = new StringBuilder(256);
    GetClassName(hwnd, cls, cls.Capacity);
    try {
      var p = System.Diagnostics.Process.GetProcessById((int)pid);
      return p.ProcessName + " pid=" + pid + " class=" + cls;
    } catch {
      return "pid=" + pid + " class=" + cls;
    }
  }
}
"@

function Test-SearchStartOpen {
  $names = @("SearchHost", "SearchApp", "SearchUI", "StartMenuExperienceHost", "ShellExperienceHost")
  $procs = Get-Process -Name $names -ErrorAction SilentlyContinue
  $fg = [T1AaProbe]::ForegroundHint()
  $fgHit = $false
  foreach ($n in $names) {
    if ($fg -like "*$n*") { $fgHit = $true }
  }
  [pscustomobject]@{
    Fg = $fg
    FgIsShell = $fgHit
    ShellProcs = @($procs | ForEach-Object { "$($_.ProcessName):$($_.Id)" })
  }
}

$before = Test-SearchStartOpen
Write-Host "BEFORE fg=$($before.Fg) shell=$($before.ShellProcs -join ',')"
[T1AaProbe]::TapBrowserSearch()
Start-Sleep -Milliseconds 900
$after = Test-SearchStartOpen
Write-Host "AFTER  fg=$($after.Fg) shell=$($after.ShellProcs -join ',')"

$log = Join-Path $env:APPDATA "com.remote-bridge-hub.app\logs\app.log"
$swallowed = $false
if (Test-Path $log) {
  $tail = Get-Content $log -Tail 40
  $swallowed = [bool]($tail | Select-String -Pattern "T1 LL swallow BrowserSearch vk=0xAA|T1 BrowserSearch hotkey consumed")
  Write-Host "LOG swallow=$swallowed"
}

if ($after.FgIsShell) {
  Write-Host "FAIL Search/Start took foreground after 0xAA"
  exit 1
}
if (-not $swallowed) {
  Write-Host "FAIL T1 LL hook did not log swallow (gate may be off)"
  exit 2
}
Write-Host "PASS foreground is not Search/Start and LL swallowed 0xAA"
exit 0
