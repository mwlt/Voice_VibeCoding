# Live probe: T1 voice map (Ctrl+Win) tap + vkE8 dummy. Pass = Start/Search stay down.
$ErrorActionPreference = "Stop"

Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class T1TapProbe {
  [DllImport("user32.dll")]
  public static extern void keybd_event(byte bVk, byte bScan, uint dwFlags, UIntPtr dwExtraInfo);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)]
  public static extern int GetClassName(IntPtr hWnd, StringBuilder lpClassName, int nMaxCount);
  const uint KEYEVENTF_KEYUP = 2;
  public static void Tap(byte vk) {
    keybd_event(vk, 0, 0, UIntPtr.Zero);
    keybd_event(vk, 0, KEYEVENTF_KEYUP, UIntPtr.Zero);
  }
  public static void TapCtrlWinDummy() {
    keybd_event(0xA2, 0, 0, UIntPtr.Zero);
    keybd_event(0x5B, 0, 0, UIntPtr.Zero);
    System.Threading.Thread.Sleep(120);
    keybd_event(0x5B, 0, KEYEVENTF_KEYUP, UIntPtr.Zero);
    keybd_event(0xA2, 0, KEYEVENTF_KEYUP, UIntPtr.Zero);
    keybd_event(0xE8, 0, 0, UIntPtr.Zero);
    keybd_event(0xE8, 0, KEYEVENTF_KEYUP, UIntPtr.Zero);
  }
  public static string Fg() {
    var hwnd = GetForegroundWindow();
    if (hwnd == IntPtr.Zero) return "none";
    uint pid; GetWindowThreadProcessId(hwnd, out pid);
    var cls = new StringBuilder(256);
    GetClassName(hwnd, cls, cls.Capacity);
    try { return System.Diagnostics.Process.GetProcessById((int)pid).ProcessName + " class=" + cls; }
    catch { return "pid=" + pid; }
  }
}
"@

function Test-FgIsShell {
  $fg = [T1TapProbe]::Fg()
  [pscustomobject]@{
    Fg = $fg
    IsShell = [bool]($fg -match "SearchHost|SearchApp|SearchUI|StartMenuExperienceHost|ShellExperienceHost")
  }
}

$before = Test-FgIsShell
Write-Host "BEFORE $($before.Fg)"
[T1TapProbe]::TapCtrlWinDummy()
Start-Sleep -Milliseconds 400
$mid = Test-FgIsShell
Write-Host "AFTER400 $($mid.Fg)"
Start-Sleep -Milliseconds 800
$after = Test-FgIsShell
Write-Host "AFTER1200 $($after.Fg)"
if ($after.IsShell) {
  Write-Host "FAIL Ctrl+Win+dummy left Search/Start in foreground"
  exit 1
}
Write-Host "PASS Ctrl+Win tap + dummy did not leave Search/Start"
exit 0
