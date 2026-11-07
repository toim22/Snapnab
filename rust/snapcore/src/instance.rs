//! One instance per program (Util.cpp): a named mutex marks the running one, and a second start hands its command
//! line to the first one's window (WM_COPYDATA 0x8010 with a GUID header) and ends. Also the launch of the sibling
//! program and the command-line splitting both programs share.

use std::ffi::c_void;

use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HLOCAL, HWND, LPARAM, LocalFree, WPARAM,
};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::Environment::GetCommandLineW;
use windows::Win32::System::Threading::{
    BELOW_NORMAL_PRIORITY_CLASS, CreateMutexW, CreateProcessW, OpenProcess, PROCESS_CREATION_FLAGS,
    PROCESS_INFORMATION, PROCESS_SYNCHRONIZE, ReleaseMutex, STARTUPINFOW, WaitForSingleObject,
};
use windows::Win32::UI::Shell::CommandLineToArgvW;
use windows::Win32::UI::WindowsAndMessaging::{
    ASFW_ANY, AllowSetForegroundWindow, FindWindowW, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_COPYDATA,
};
use windows::core::{PCWSTR, PWSTR};

use crate::wide::to_wide;
use crate::{log, paths};

/// WM_COPYDATA dwData of a forwarded command line.
pub const COPYDATA_CMDLINE: usize = 0x8010;
/// {61E85D7D-94D3-4123-8410-FA58D2D09DF2}, as it lies in memory: the start of the WM_COPYDATA block.
const COPYDATA_GUID: [u8; 16] = [0x7d, 0x5d, 0xe8, 0x61, 0xd3, 0x94, 0x23, 0x41, 0x84, 0x10, 0xfa, 0x58, 0xd2, 0xd0, 0x9d, 0xf2];
const HEADER: usize = 20; // GUID + DWORD byte count
const MAX_TEXT_BYTES: usize = 10000;

/// The process's command line as Windows gave it.
pub fn command_line() -> String {
    unsafe { GetCommandLineW().to_string() }.unwrap_or_default()
}

/// The arguments of a command line, without the program (CommandLineToArgvW's rules for quotes).
pub fn split_command_line(cmd: &str) -> Vec<String> {
    let w = to_wide(cmd);
    let mut argc = 0;
    let argv = unsafe { CommandLineToArgvW(PCWSTR(w.as_ptr()), &mut argc) };
    if argv.is_null() {
        return Vec::new();
    }
    let args = (1..argc as usize).map(|i| unsafe { (*argv.add(i)).to_string() }.unwrap_or_default()).collect();
    unsafe {
        let _ = LocalFree(Some(HLOCAL(argv as *mut c_void)));
    }
    args
}

/// The block WM_COPYDATA carries: the GUID, the byte count and the text with its NUL (at most 4999 characters).
pub fn copy_data_block(cmd: &str) -> Vec<u8> {
    let mut text: Vec<u16> = cmd.encode_utf16().take(4999).collect();
    text.push(0);
    let mut b = Vec::with_capacity(HEADER + text.len() * 2);
    b.extend_from_slice(&COPYDATA_GUID);
    b.extend_from_slice(&((text.len() * 2) as u32).to_le_bytes());
    b.extend(text.iter().flat_map(|c| c.to_le_bytes()));
    b
}

/// The command line in a WM_COPYDATA block, or None if the block is not one.
pub fn parse_copy_data_block(dw_data: usize, data: &[u8]) -> Option<String> {
    if dw_data != COPYDATA_CMDLINE || data.len() < HEADER || data[..16] != COPYDATA_GUID {
        return None;
    }
    let cb = u32::from_le_bytes(data[16..20].try_into().unwrap()) as usize;
    if cb > data.len() - HEADER || cb > MAX_TEXT_BYTES {
        return None;
    }
    let w: Vec<u16> = data[HEADER..HEADER + cb].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]]))
                                               .take_while(|&c| c != 0).collect();
    Some(String::from_utf16_lossy(&w))
}

/// ParseCopyData: the command line of a WM_COPYDATA message's lParam.
///
/// # Safety
/// `cds` must be the lParam of a WM_COPYDATA message being handled.
pub unsafe fn parse_copy_data(cds: *const COPYDATASTRUCT) -> Option<String> {
    let c = unsafe { cds.as_ref()? };
    if c.lpData.is_null() {
        return None;
    }
    let data = unsafe { std::slice::from_raw_parts(c.lpData as *const u8, c.cbData as usize) };
    parse_copy_data_block(c.dwData, data)
}

/// Hands a command line to a running instance's window (waits up to 10 s, not at all for a hung window).
pub fn send_command_line(target: HWND, cmd: &str) -> bool {
    let block = copy_data_block(cmd);
    let cds = COPYDATASTRUCT { dwData: COPYDATA_CMDLINE, cbData: block.len() as u32, lpData: block.as_ptr() as *mut c_void };
    let mut res = 0usize;
    unsafe {
        SendMessageTimeoutW(target, WM_COPYDATA, WPARAM(0), LPARAM(&cds as *const _ as isize), SMTO_ABORTIFHUNG, 10000,
                            Some(&mut res))
        .0 != 0
    }
}

fn find_window(class: &str) -> Option<HWND> {
    let c = to_wide(class);
    unsafe { FindWindowW(PCWSTR(c.as_ptr()), PCWSTR::null()) }.ok().filter(|h| !h.0.is_null())
}

/// The single-instance mutex, held while the program runs.
pub struct InstanceMutex(HANDLE);

impl Drop for InstanceMutex {
    fn drop(&mut self) {
        unsafe {
            let _ = ReleaseMutex(self.0);
            let _ = CloseHandle(self.0);
        }
    }
}

/// ForwardToExistingInstance: the mutex when this is the first instance; None when another one runs, after handing
/// it this process's command line (its window may still be starting: up to 5 s).
pub fn claim_or_forward(mutex: &str, class: &str) -> Option<InstanceMutex> {
    let n = to_wide(mutex);
    let m = unsafe { CreateMutexW(None, true, PCWSTR(n.as_ptr())) };
    let exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    match m {
        Ok(h) if !exists => return Some(InstanceMutex(h)),
        Ok(h) => unsafe {
            let _ = CloseHandle(h);
        },
        Err(_) => {}
    }
    let mut w = None;
    for i in 0..25 {
        if i > 0 {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        w = find_window(class);
        if w.is_some() {
            break;
        }
    }
    if let Some(w) = w {
        unsafe {
            let _ = AllowSetForegroundWindow(ASFW_ANY);
        }
        send_command_line(w, &command_line());
    }
    None
}

/// The arguments that make a restarted program wait for this one to end (/restartwait <pid>).
pub fn restart_args() -> String {
    format!("/restartwait {}", std::process::id())
}

/// WaitForPreviousInstance: with /restartwait <pid>, waits (up to 15 s) for that process to end, or for the old
/// window to go away when there is no pid.
pub fn wait_for_previous_instance(class: &str) {
    let args = split_command_line(&command_line());
    let Some(i) = args.iter().position(|a| a.eq_ignore_ascii_case("/restartwait")) else { return };
    let pid = args.get(i + 1).map_or(0, |p| crate::crt::strtoul(p, 10));
    // the window goes away before the process releases the mutex: with the pid, wait for the whole process
    if pid != 0
        && let Ok(p) = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }
    {
        unsafe {
            WaitForSingleObject(p, 15000);
            let _ = CloseHandle(p);
        }
        return;
    }
    for _ in 0..75 {
        if find_window(class).is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

/// Starts the other program from this program's folder ("SnapnabEditor.exe", "Snapnab.exe").
pub fn launch_sibling(exe_name: &str, args: &str, low_priority: bool) -> bool {
    let dir = paths::exe_dir();
    let exe = dir.join(exe_name);
    let (exew, dirw) = (to_wide(&exe), to_wide(&dir));
    let mut cmd = to_wide(format!("\"{}\" {args}", exe.display()));
    let si = STARTUPINFOW { cb: std::mem::size_of::<STARTUPINFOW>() as u32, ..Default::default() };
    let mut pi = PROCESS_INFORMATION::default();
    let flags = if low_priority { BELOW_NORMAL_PRIORITY_CLASS } else { PROCESS_CREATION_FLAGS(0) };
    let ok = unsafe {
        CreateProcessW(PCWSTR(exew.as_ptr()), Some(PWSTR(cmd.as_mut_ptr())), None, None, false, flags, None,
                       PCWSTR(dirw.as_ptr()), &si, &mut pi)
    };
    if let Err(e) = ok {
        log::write(&format!("LaunchSibling failed: {} ({})", exe.display(), e.code().0 as u32 & 0xFFFF));
        return false;
    }
    unsafe {
        let _ = AllowSetForegroundWindow(pi.dwProcessId);
        let _ = CloseHandle(pi.hThread);
        let _ = CloseHandle(pi.hProcess);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_line_blocks() {
        let b = copy_data_block(r#""C:\x\Snapnab.exe" /preset "Região ç""#);
        assert_eq!(parse_copy_data_block(COPYDATA_CMDLINE, &b).as_deref(), Some(r#""C:\x\Snapnab.exe" /preset "Região ç""#));
        assert_eq!(parse_copy_data_block(0x8011, &b), None);
        let mut bad = b.clone();
        bad[0] ^= 1;
        assert_eq!(parse_copy_data_block(COPYDATA_CMDLINE, &bad), None);
        assert_eq!(parse_copy_data_block(COPYDATA_CMDLINE, &b[..b.len() - 2]), None, "count past the end");
        let long = "x".repeat(6000);
        assert_eq!(parse_copy_data_block(COPYDATA_CMDLINE, &copy_data_block(&long)).unwrap().len(), 4999);
        assert_eq!(split_command_line(r#"app.exe /preset "a b" /hide"#), ["/preset", "a b", "/hide"]);
    }
}
