//! SnapEx64.dll, "Snapnab Extended Window Capture" (≙ SnagEx64.dll, ds_pseudocode §6 / ds_arch §10.1).
//!
//! `Snap5000(msgEnter, msgLeave, threadId)` installs a WH_CALLWNDPROC hook on the target window's thread; when the
//! window receives `msgEnter` it is subclassed and answers WM_GETMINMAXINFO with 19500×19500, so it can grow beyond
//! the screen; `msgLeave` gives it its window procedure back. `Snap5001()` removes the hook.
#![allow(non_snake_case)]

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering::SeqCst};

use windows::core::BOOL;
use windows::Win32::Foundation::{HINSTANCE, HMODULE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::DisableThreadLibraryCalls;
use windows::Win32::System::SystemServices::{DLL_PROCESS_ATTACH, DLL_PROCESS_DETACH};
use windows::Win32::UI::WindowsAndMessaging::{
    CWPSTRUCT, CallNextHookEx, CallWindowProcA, CallWindowProcW, GWLP_WNDPROC, HC_ACTION, HHOOK, IsWindow, IsWindowUnicode,
    MINMAXINFO, SetWindowLongPtrA, SetWindowLongPtrW, SetWindowsHookExW, UnhookWindowsHookEx, WH_CALLWNDPROC,
    WM_GETMINMAXINFO, WNDPROC,
};

const MAX: i32 = 19500; // 0x4c2c

// shared by every process the DLL is mapped into
#[unsafe(link_section = ".shared")]
static MSG_ENTER: AtomicU32 = AtomicU32::new(0);
#[unsafe(link_section = ".shared")]
static MSG_LEAVE: AtomicU32 = AtomicU32::new(0);

// the state of each process
static SELF: AtomicIsize = AtomicIsize::new(0);
static HOOK: AtomicIsize = AtomicIsize::new(0);
static TARGET: AtomicIsize = AtomicIsize::new(0);
static OLD_PROC: AtomicIsize = AtomicIsize::new(0);
static SUBCLASSED: AtomicBool = AtomicBool::new(false);

fn old_proc() -> WNDPROC {
    let p = OLD_PROC.load(SeqCst);
    // SAFETY: OLD_PROC holds the window procedure SetWindowLongPtr returned (non-zero when SUBCLASSED)
    (p != 0).then(|| unsafe { std::mem::transmute::<isize, unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT>(p) })
}

unsafe fn set_proc(h: HWND, p: isize) -> isize {
    unsafe { if IsWindowUnicode(h).as_bool() { SetWindowLongPtrW(h, GWLP_WNDPROC, p) } else { SetWindowLongPtrA(h, GWLP_WNDPROC, p) } }
}

unsafe extern "system" fn subclass_proc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if m == WM_GETMINMAXINFO {
        let mmi = unsafe { &mut *(lp.0 as *mut MINMAXINFO) };
        mmi.ptMaxTrackSize.x = MAX;
        mmi.ptMaxTrackSize.y = MAX;
        mmi.ptMaxSize.x = MAX;
        mmi.ptMaxSize.y = MAX;
        return LRESULT(1); // does not call the original procedure
    }
    unsafe {
        if IsWindowUnicode(h).as_bool() { CallWindowProcW(old_proc(), h, m, wp, lp) } else { CallWindowProcA(old_proc(), h, m, wp, lp) }
    }
}

unsafe extern "system" fn hook_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let c = unsafe { &*(lp.0 as *const CWPSTRUCT) };
        let (enter, leave) = (MSG_ENTER.load(SeqCst), MSG_LEAVE.load(SeqCst));
        if enter != 0 && c.message == enter && !SUBCLASSED.load(SeqCst) {
            let old = unsafe { set_proc(c.hwnd, subclass_proc as *const () as isize) };
            if old != 0 {
                OLD_PROC.store(old, SeqCst);
                TARGET.store(c.hwnd.0 as isize, SeqCst);
                SUBCLASSED.store(true, SeqCst);
            }
        }
        if leave != 0 && c.message == leave && SUBCLASSED.load(SeqCst) && c.hwnd.0 as isize == TARGET.load(SeqCst) {
            unsafe { set_proc(c.hwnd, OLD_PROC.load(SeqCst)) };
            SUBCLASSED.store(false, SeqCst);
            TARGET.store(0, SeqCst);
        }
    }
    unsafe { CallNextHookEx(None, code, wp, lp) }
}

/// threadId = 0: every thread of the desktop (like the original); Snapnab passes the target window's thread.
#[unsafe(no_mangle)]
pub extern "system" fn Snap5000(msg_enter: u32, msg_leave: u32, thread_id: u32) -> BOOL {
    MSG_ENTER.store(msg_enter, SeqCst);
    MSG_LEAVE.store(msg_leave, SeqCst);
    let me = HINSTANCE(SELF.load(SeqCst) as _);
    match unsafe { SetWindowsHookExW(WH_CALLWNDPROC, Some(hook_proc), Some(me), thread_id) } {
        Ok(h) => {
            HOOK.store(h.0 as isize, SeqCst);
            BOOL(1)
        }
        Err(_) => BOOL(0),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Snap5001() {
    let h = HOOK.swap(0, SeqCst);
    if h != 0 {
        unsafe {
            let _ = UnhookWindowsHookEx(HHOOK(h as _));
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn DllMain(h: HMODULE, reason: u32, _: *mut core::ffi::c_void) -> BOOL {
    if reason == DLL_PROCESS_ATTACH {
        SELF.store(h.0 as isize, SeqCst);
        unsafe {
            let _ = DisableThreadLibraryCalls(h);
        }
    }
    if reason == DLL_PROCESS_DETACH && SUBCLASSED.load(SeqCst) {
        // never leaves the window with a dangling procedure
        let t = HWND(TARGET.load(SeqCst) as _);
        if unsafe { IsWindow(Some(t)) }.as_bool() {
            unsafe { set_proc(t, OLD_PROC.load(SeqCst)) };
        }
    }
    BOOL(1)
}
