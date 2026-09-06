//! T1 WH_SHELL hook DLL — injected by SetWindowsHookEx.
//!
//! When the named gate mapping byte0 is non-zero and the shell appcommand is
//! APPCOMMAND_BROWSER_SEARCH (5), return 1 without CallNextHookEx so Explorer
//! does not open SearchHost.
//!
//! Shared mapping `Local\T1BrSearchShellGate` (4 bytes):
//!   [0] gate on/off
//!   [1] HSHELL_APPCOMMAND enter count (saturating)
//!   [2] swallow count (saturating)
//!   [3] reserved

#![allow(non_snake_case)]

use std::ffi::c_void;
use std::ptr;

type HHOOK = *mut c_void;
type LRESULT = isize;
type WPARAM = usize;
type LPARAM = isize;
type BOOL = i32;
type DWORD = u32;
type HANDLE = *mut c_void;

const HSHELL_APPCOMMAND: i32 = 12;
const APPCOMMAND_BROWSER_SEARCH: i16 = 5;
const FILE_MAP_ALL_ACCESS: DWORD = 0x000F_001F;
const MAP_BYTES: usize = 4;

/// Must match native_suppress gate mapping name (UTF-16).
const GATE_MAP_NAME: &[u16] = &[
    b'L' as u16, b'o' as u16, b'c' as u16, b'a' as u16, b'l' as u16, b'\\' as u16,
    b'T' as u16, b'1' as u16, b'B' as u16, b'r' as u16, b'S' as u16, b'e' as u16,
    b'a' as u16, b'r' as u16, b'c' as u16, b'h' as u16, b'S' as u16, b'h' as u16,
    b'e' as u16, b'l' as u16, b'l' as u16, b'G' as u16, b'a' as u16, b't' as u16,
    b'e' as u16, 0,
];

#[link(name = "user32")]
extern "system" {
    fn CallNextHookEx(hhk: HHOOK, nCode: i32, wParam: WPARAM, lParam: LPARAM) -> LRESULT;
}

#[link(name = "kernel32")]
extern "system" {
    fn OpenFileMappingW(dwDesiredAccess: DWORD, bInheritHandle: BOOL, lpName: *const u16) -> HANDLE;
    fn MapViewOfFile(
        hFileMappingObject: HANDLE,
        dwDesiredAccess: DWORD,
        dwFileOffsetHigh: DWORD,
        dwFileOffsetLow: DWORD,
        dwNumberOfBytesToMap: usize,
    ) -> *mut c_void;
    fn UnmapViewOfFile(lpBaseAddress: *const c_void) -> BOOL;
    fn CloseHandle(hObject: HANDLE) -> BOOL;
}

struct GateView {
    handle: HANDLE,
    view: *mut u8,
}

impl GateView {
    unsafe fn open() -> Option<Self> {
        let h = OpenFileMappingW(FILE_MAP_ALL_ACCESS, 0, GATE_MAP_NAME.as_ptr());
        if h.is_null() {
            return None;
        }
        let view = MapViewOfFile(h, FILE_MAP_ALL_ACCESS, 0, 0, MAP_BYTES) as *mut u8;
        if view.is_null() {
            CloseHandle(h);
            return None;
        }
        Some(Self { handle: h, view })
    }

    unsafe fn gate_on(&self) -> bool {
        *self.view != 0
    }

    unsafe fn bump(offset: usize) {
        if let Some(g) = Self::open() {
            let p = g.view.add(offset);
            let v = *p;
            if v < 255 {
                *p = v + 1;
            }
            // GateView drop closes
            drop(g);
        }
    }
}

impl Drop for GateView {
    fn drop(&mut self) {
        unsafe {
            UnmapViewOfFile(self.view as *const c_void);
            CloseHandle(self.handle);
        }
    }
}

fn gate_is_on() -> bool {
    unsafe {
        GateView::open()
            .map(|g| g.gate_on())
            .unwrap_or(false)
    }
}

fn appcommand_from_lparam(lparam: LPARAM) -> i16 {
    ((lparam >> 16) as u16 & 0x0FFF) as i16
}

/// Exported shell procedure for SetWindowsHookEx(WH_SHELL, …).
#[no_mangle]
pub unsafe extern "system" fn T1ShellProc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    if n_code < 0 {
        return CallNextHookEx(ptr::null_mut(), n_code, w_param, l_param);
    }
    if n_code == HSHELL_APPCOMMAND {
        GateView::bump(1);
        let cmd = appcommand_from_lparam(l_param);
        if cmd == APPCOMMAND_BROWSER_SEARCH && gate_is_on() {
            GateView::bump(2);
            // Documented: handle HSHELL_APPCOMMAND → nonzero, no CallNextHookEx.
            return 1;
        }
    }
    CallNextHookEx(ptr::null_mut(), n_code, w_param, l_param)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_cmd_is_five() {
        assert_eq!(APPCOMMAND_BROWSER_SEARCH, 5);
        assert_eq!(appcommand_from_lparam(5 << 16), 5);
        assert_eq!(HSHELL_APPCOMMAND, 12);
    }
}
