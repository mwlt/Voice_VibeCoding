# Live probe: WH_SHELL DLL hard-block via DefWindowProc → HSHELL_APPCOMMAND path.
# Direct SendMessage to Shell_TrayWnd does NOT exercise WH_SHELL (tray handles it).
$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
$candidates = @(
  (Join-Path $root "src-tauri\target\debug\t1_shell_hook.dll"),
  (Join-Path $root "src-tauri\target\release\t1_shell_hook.dll"),
  (Join-Path $root "src-tauri\target-test-swallow\debug\t1_shell_hook.dll")
)
$dllPath = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $dllPath) {
  Write-Host "FAIL t1_shell_hook.dll missing — cargo build -p t1_shell_hook"
  exit 2
}
Write-Host "DLL $dllPath"

Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class T1WhShellProbe {
  const int WH_SHELL = 10;
  const uint WM_APPCOMMAND = 0x0319;
  const uint WM_DESTROY = 0x0002;
  const int APPCOMMAND_BROWSER_SEARCH = 5;
  const uint PAGE_READWRITE = 0x04;
  const uint FILE_MAP_ALL_ACCESS = 0xF001F;
  const int CW_USEDEFAULT = unchecked((int)0x80000000);
  static readonly IntPtr INVALID_HANDLE_VALUE = new IntPtr(-1);

  [DllImport("kernel32", CharSet = CharSet.Unicode, SetLastError = true)]
  public static extern IntPtr LoadLibrary(string path);
  [DllImport("kernel32", CharSet = CharSet.Ansi, SetLastError = true)]
  public static extern IntPtr GetProcAddress(IntPtr h, string name);
  [DllImport("kernel32", SetLastError = true)]
  public static extern bool FreeLibrary(IntPtr h);
  [DllImport("kernel32", CharSet = CharSet.Unicode, SetLastError = true)]
  public static extern IntPtr CreateFileMapping(IntPtr hFile, IntPtr attr, uint prot, uint hi, uint lo, string name);
  [DllImport("kernel32", SetLastError = true)]
  public static extern IntPtr MapViewOfFile(IntPtr h, uint access, uint hi, uint lo, UIntPtr bytes);
  [DllImport("kernel32", SetLastError = true)]
  public static extern bool UnmapViewOfFile(IntPtr p);
  [DllImport("kernel32", SetLastError = true)]
  public static extern bool CloseHandle(IntPtr h);
  [DllImport("user32", SetLastError = true)]
  public static extern IntPtr SetWindowsHookEx(int id, IntPtr fn, IntPtr mod, uint tid);
  [DllImport("user32", SetLastError = true)]
  public static extern bool UnhookWindowsHookEx(IntPtr h);
  [DllImport("user32")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32")] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32")] public static extern IntPtr DefWindowProc(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32", CharSet = CharSet.Unicode)]
  public static extern int GetClassName(IntPtr h, StringBuilder b, int n);
  [DllImport("user32", CharSet = CharSet.Unicode, SetLastError = true)]
  public static extern ushort RegisterClassEx(ref WNDCLASSEX wc);
  [DllImport("user32", CharSet = CharSet.Unicode, SetLastError = true)]
  public static extern IntPtr CreateWindowEx(uint ex, string cls, string name, uint style,
    int x, int y, int w, int h, IntPtr parent, IntPtr menu, IntPtr inst, IntPtr param);
  [DllImport("user32")] public static extern bool DestroyWindow(IntPtr h);
  [DllImport("user32")] public static extern bool PeekMessage(out MSG msg, IntPtr h, uint min, uint max, uint remove);
  [DllImport("user32")] public static extern bool TranslateMessage(ref MSG msg);
  [DllImport("user32")] public static extern IntPtr DispatchMessage(ref MSG msg);
  [DllImport("kernel32")] public static extern IntPtr GetModuleHandle(string name);

  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
  public struct WNDCLASSEX {
    public uint cbSize; public uint style; public IntPtr lpfnWndProc;
    public int cbClsExtra; public int cbWndExtra; public IntPtr hInstance;
    public IntPtr hIcon; public IntPtr hCursor; public IntPtr hbrBackground;
    public string lpszMenuName; public string lpszClassName; public IntPtr hIconSm;
  }
  [StructLayout(LayoutKind.Sequential)]
  public struct MSG {
    public IntPtr hwnd; public uint message; public IntPtr wParam; public IntPtr lParam;
    public uint time; public int pt_x; public int pt_y;
  }

  public delegate IntPtr WndProc(IntPtr h, uint m, IntPtr w, IntPtr l);
  static WndProc KeepAlive; // prevent GC of delegate

  public static IntPtr GateMap;
  public static IntPtr Dll;
  public static IntPtr Hook;
  public static IntPtr ProbeHwnd;

  static IntPtr ProbeWndProc(IntPtr h, uint m, IntPtr w, IntPtr l) {
    // Do NOT handle WM_APPCOMMAND — DefWindowProc must notify shell hooks.
    if (m == WM_DESTROY) return IntPtr.Zero;
    return DefWindowProc(h, m, w, l);
  }

  public static void Arm(string dllPath, byte gateOn) {
    GateMap = CreateFileMapping(INVALID_HANDLE_VALUE, IntPtr.Zero, PAGE_READWRITE, 0, 4, "Local\\T1BrSearchShellGate");
    if (GateMap == IntPtr.Zero) throw new Exception("CreateFileMapping failed " + Marshal.GetLastWin32Error());
    var view = MapViewOfFile(GateMap, FILE_MAP_ALL_ACCESS, 0, 0, new UIntPtr(4));
    if (view == IntPtr.Zero) throw new Exception("MapViewOfFile failed");
    Marshal.WriteByte(view, 0, gateOn);
    Marshal.WriteByte(view, 1, 0); // hook enter (DLL may not write yet)
    Marshal.WriteByte(view, 2, 0); // swallow
    Marshal.WriteByte(view, 3, 0);
    UnmapViewOfFile(view);

    Dll = LoadLibrary(dllPath);
    if (Dll == IntPtr.Zero) throw new Exception("LoadLibrary failed " + Marshal.GetLastWin32Error());
    var proc = GetProcAddress(Dll, "T1ShellProc");
    if (proc == IntPtr.Zero) throw new Exception("GetProcAddress T1ShellProc failed");
    Hook = SetWindowsHookEx(WH_SHELL, proc, Dll, 0);
    if (Hook == IntPtr.Zero) throw new Exception("SetWindowsHookEx failed " + Marshal.GetLastWin32Error());

    KeepAlive = ProbeWndProc;
    var inst = GetModuleHandle(null);
    var wc = new WNDCLASSEX();
    wc.cbSize = (uint)Marshal.SizeOf(typeof(WNDCLASSEX));
    wc.lpfnWndProc = Marshal.GetFunctionPointerForDelegate(KeepAlive);
    wc.hInstance = inst;
    wc.lpszClassName = "T1WhShellProbeCls";
    RegisterClassEx(ref wc);
    ProbeHwnd = CreateWindowEx(0, "T1WhShellProbeCls", "T1WhShellProbe", 0,
      CW_USEDEFAULT, CW_USEDEFAULT, 0, 0, IntPtr.Zero, IntPtr.Zero, inst, IntPtr.Zero);
    if (ProbeHwnd == IntPtr.Zero) throw new Exception("CreateWindowEx failed " + Marshal.GetLastWin32Error());
  }

  public static byte[] ReadGate() {
    var view = MapViewOfFile(GateMap, FILE_MAP_ALL_ACCESS, 0, 0, new UIntPtr(4));
    var buf = new byte[4];
    Marshal.Copy(view, buf, 0, 4);
    UnmapViewOfFile(view);
    return buf;
  }

  public static void Disarm() {
    if (ProbeHwnd != IntPtr.Zero) { DestroyWindow(ProbeHwnd); ProbeHwnd = IntPtr.Zero; }
    if (Hook != IntPtr.Zero) { UnhookWindowsHookEx(Hook); Hook = IntPtr.Zero; }
    if (Dll != IntPtr.Zero) { FreeLibrary(Dll); Dll = IntPtr.Zero; }
    if (GateMap != IntPtr.Zero) { CloseHandle(GateMap); GateMap = IntPtr.Zero; }
  }

  public static void Pump(int ms) {
    var end = Environment.TickCount + ms;
    MSG msg;
    while (Environment.TickCount < end) {
      while (PeekMessage(out msg, IntPtr.Zero, 0, 0, 1)) {
        TranslateMessage(ref msg);
        DispatchMessage(ref msg);
      }
      System.Threading.Thread.Sleep(10);
    }
  }

  public static void SendSearchViaDefWindowProc() {
    IntPtr lp = (IntPtr)(APPCOMMAND_BROWSER_SEARCH << 16);
    // Unhandled WM_APPCOMMAND → DefWindowProc → HSHELL_APPCOMMAND
    SendMessage(ProbeHwnd, WM_APPCOMMAND, ProbeHwnd, lp);
  }

  public static string Fg() {
    var hwnd = GetForegroundWindow();
    if (hwnd == IntPtr.Zero) return "none";
    uint pid; GetWindowThreadProcessId(hwnd, out pid);
    var cls = new StringBuilder(256);
    GetClassName(hwnd, cls, cls.Capacity);
    try { return System.Diagnostics.Process.GetProcessById((int)pid).ProcessName + " class=" + cls; }
    catch { return "pid=" + pid + " class=" + cls; }
  }

  public static bool IsShell(string fg) {
    return System.Text.RegularExpressions.Regex.IsMatch(fg,
      "SearchHost|SearchApp|SearchUI|StartMenuExperienceHost|ShellExperienceHost");
  }
}
"@

try {
  [T1WhShellProbe]::Arm($dllPath, 1)
  Write-Host "HOOK armed gate=1 hwnd=$([T1WhShellProbe]::ProbeHwnd)"
  $before = [T1WhShellProbe]::Fg()
  Write-Host "BEFORE $before"
  [T1WhShellProbe]::SendSearchViaDefWindowProc()
  [T1WhShellProbe]::Pump(400)
  $mid = [T1WhShellProbe]::Fg()
  $g1 = [T1WhShellProbe]::ReadGate()
  Write-Host ("AFTER400 {0} gate=[{1},{2},{3},{4}]" -f $mid, $g1[0], $g1[1], $g1[2], $g1[3])
  [T1WhShellProbe]::Pump(900)
  $after = [T1WhShellProbe]::Fg()
  $g2 = [T1WhShellProbe]::ReadGate()
  Write-Host ("AFTER1300 {0} gate=[{1},{2},{3},{4}]" -f $after, $g2[0], $g2[1], $g2[2], $g2[3])

  if ([T1WhShellProbe]::IsShell($after)) {
    Write-Host "FAIL APPCOMMAND left Search/Start in foreground"
    exit 1
  }
  if ([T1WhShellProbe]::IsShell($mid)) {
    Write-Host "WARN Search flashed then dismissed (hook may not hard-block)"
    exit 0
  }
  Write-Host "PASS WH_SHELL hard-block: APPCOMMAND did not steal foreground"
  exit 0
}
finally {
  [T1WhShellProbe]::Disarm()
}
