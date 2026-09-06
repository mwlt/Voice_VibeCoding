# Live probe: WM_APPCOMMAND / APPCOMMAND_BROWSER_SEARCH via tray/fg/broadcast.
# This path often bypasses WH_SHELL (tray handles WM_APPCOMMAND itself).
# For hard-block proof use scripts/probe_t1_wh_shell_hardblock.ps1 (DefWindowProc path).
$ErrorActionPreference = "Stop"

Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class T1AppCmd {
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindow(string lpClassName, string lpWindowName);
  [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)]
  public static extern int GetClassName(IntPtr hWnd, StringBuilder lpClassName, int nMaxCount);
  const uint WM_APPCOMMAND = 0x0319;
  // Win32: APPCOMMAND_BROWSER_SEARCH = 5 (Favorites = 6). Do not use 6.
  const int APPCOMMAND_BROWSER_SEARCH = 5;
  public static void SendSearch() {
    IntPtr lp = (IntPtr)(APPCOMMAND_BROWSER_SEARCH << 16);
    var tray = FindWindow("Shell_TrayWnd", null);
    var fg = GetForegroundWindow();
    if (tray != IntPtr.Zero) {
      SendMessage(tray, WM_APPCOMMAND, tray, lp);
    }
    if (fg != IntPtr.Zero) {
      SendMessage(fg, WM_APPCOMMAND, fg, lp);
    }
    IntPtr broadcast = (IntPtr)0xFFFF;
    PostMessage(broadcast, WM_APPCOMMAND, fg, lp);
  }
  public static string Fg() {
    var hwnd = GetForegroundWindow();
    if (hwnd == IntPtr.Zero) return "none";
    uint pid;
    GetWindowThreadProcessId(hwnd, out pid);
    var cls = new StringBuilder(256);
    GetClassName(hwnd, cls, cls.Capacity);
    try {
      return System.Diagnostics.Process.GetProcessById((int)pid).ProcessName + " class=" + cls;
    } catch {
      return "pid=" + pid + " class=" + cls;
    }
  }
}
"@

function Test-FgIsShell {
  $fg = [T1AppCmd]::Fg()
  $hit = $fg -match "SearchHost|SearchApp|SearchUI|StartMenuExperienceHost|ShellExperienceHost"
  [pscustomobject]@{ Fg = $fg; IsShell = $hit }
}

$before = Test-FgIsShell
Write-Host "BEFORE $($before.Fg)"
[T1AppCmd]::SendSearch()
Start-Sleep -Milliseconds 350
$mid = Test-FgIsShell
Write-Host "AFTER350 $($mid.Fg)"
Start-Sleep -Milliseconds 900
$after = Test-FgIsShell
Write-Host "AFTER1250 $($after.Fg)"

if ($after.IsShell) {
  Write-Host "FAIL APPCOMMAND left Search/Start in foreground"
  exit 1
}
if ($mid.IsShell) {
  Write-Host "WARN Search flashed then dismissed"
  exit 0
}
Write-Host "PASS APPCOMMAND did not steal foreground"
exit 0
