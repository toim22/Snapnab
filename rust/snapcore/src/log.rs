//! Internal log (CSnagitLogger, comp_10): %LOCALAPPDATA%\Snapnab\Logs\<program>.log, UTF-8 with a BOM, one line per
//! message as "2026-11-03 02:52:05.705 [40284] text" (local time, thread id). Above 2 MB at startup the file moves to
//! <program>.log.old. Every line also goes to OutputDebugString. The e2e suite reads these lines (tests/e2e logs.py).

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use windows::Win32::System::Diagnostics::Debug::OutputDebugStringW;
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::core::PCWSTR;

use crate::wide::to_wide;

const ROTATE_AT: u64 = 2 * 1024 * 1024;

static FILE: Mutex<Option<PathBuf>> = Mutex::new(None);
static VERSION: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// The program version `init` was given ("" before), for the HTTP user agent.
pub fn app_version() -> &'static str {
    VERSION.get().map_or("", |v| v.as_str())
}

/// Opens the log of `component` ("Snapnab", "SnapnabEditor") and writes the start line.
pub fn init(component: &str, version: &str) {
    let path = crate::paths::logs_dir().join(format!("{component}.log"));
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > ROTATE_AT) {
        let mut old = path.clone().into_os_string();
        old.push(".old");
        let _ = std::fs::rename(&path, old);
    }
    *FILE.lock().unwrap_or_else(|e| e.into_inner()) = Some(path);
    let _ = VERSION.set(version.to_owned());
    write(&format!("==== {component} {version} started (pid {}) ====", std::process::id()));
}

pub fn write(msg: &str) {
    let t = unsafe { GetLocalTime() };
    let line = format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03} [{}] {msg}\r\n", t.wYear, t.wMonth, t.wDay, t.wHour,
                       t.wMinute, t.wSecond, t.wMilliseconds, unsafe { GetCurrentThreadId() });
    let w = to_wide(&line);
    unsafe { OutputDebugStringW(PCWSTR(w.as_ptr())) };
    let file = FILE.lock().unwrap_or_else(|e| e.into_inner());
    let Some(path) = file.as_ref() else { return };
    // opened per line, shared, so the other program and Notepad can read it at any time
    let Ok(mut f) = OpenOptions::new().append(true).create(true).open(path) else { return };
    if f.metadata().is_ok_and(|m| m.len() == 0) {
        let _ = f.write_all(b"\xEF\xBB\xBF");                     // UTF-8 BOM (Notepad)
    }
    let _ = f.write_all(line.as_bytes());
}
