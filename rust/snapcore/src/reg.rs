//! Settings in HKCU\Software\Snapnab\1 (RegistryConfig, ds_arch K8): the same values RegGetDword / RegSetDword /
//! RegGetString / RegSetString in Common.h read and write. `sub` is a subkey below the root ("Editor", "Recent"…).

use std::ffi::{OsStr, OsString, c_void};

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_DWORD, REG_SZ, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW, RegSetKeyValueW,
};
use windows::core::PCWSTR;

use crate::wide::{to_os, to_wide};

pub const ROOT: &str = r"Software\Snapnab\1";

fn key(sub: Option<&OsStr>) -> Vec<u16> {
    let mut p = OsString::from(ROOT);
    if let Some(s) = sub.filter(|s| !s.is_empty()) {
        p.push("\\");
        p.push(s);
    }
    to_wide(p)
}

pub fn get_dword(name: &OsStr, def: u32, sub: Option<&OsStr>) -> u32 {
    let (k, n) = (key(sub), to_wide(name));
    let (mut v, mut cb) = (0u32, 4u32);
    let r = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()), PCWSTR(n.as_ptr()), RRF_RT_REG_DWORD, None,
                     Some(&mut v as *mut u32 as *mut c_void), Some(&mut cb))
    };
    if r == ERROR_SUCCESS { v } else { def }
}

pub fn set_dword(name: &OsStr, v: u32, sub: Option<&OsStr>) {
    let (k, n) = (key(sub), to_wide(name));
    unsafe {
        let _ = RegSetKeyValueW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()), PCWSTR(n.as_ptr()), REG_DWORD.0,
                                Some(&v as *const u32 as *const c_void), 4);
    }
}

/// The string value, or None if it is missing or not a REG_SZ.
pub fn get_string(name: &OsStr, sub: Option<&OsStr>) -> Option<OsString> {
    let (k, n) = (key(sub), to_wide(name));
    let mut cb = 0u32;
    let r = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()), PCWSTR(n.as_ptr()), RRF_RT_REG_SZ, None, None, Some(&mut cb))
    };
    if r != ERROR_SUCCESS {
        return None;
    }
    let mut buf = vec![0u16; cb as usize / 2 + 1];
    let mut cb = (buf.len() * 2) as u32;
    let r = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()), PCWSTR(n.as_ptr()), RRF_RT_REG_SZ, None,
                     Some(buf.as_mut_ptr() as *mut c_void), Some(&mut cb))
    };
    if r != ERROR_SUCCESS {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(to_os(&buf[..len]))
}

pub fn set_string(name: &OsStr, v: &OsStr, sub: Option<&OsStr>) {
    let (k, n, w) = (key(sub), to_wide(name), to_wide(v));
    unsafe {
        let _ = RegSetKeyValueW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()), PCWSTR(n.as_ptr()), REG_SZ.0,
                                Some(w.as_ptr() as *const c_void), (w.len() * 2) as u32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::System::Registry::RegDeleteTreeW;

    #[test]
    fn round_trip() {
        let sub = OsString::from(format!("snapcore-test-{}", std::process::id()));
        let s = Some(sub.as_os_str());
        assert_eq!(get_dword(OsStr::new("Missing"), 7, s), 7);
        set_dword(OsStr::new("Number"), 0xDEAD_BEEF, s);
        assert_eq!(get_dword(OsStr::new("Number"), 0, s), 0xDEAD_BEEF);
        assert_eq!(get_string(OsStr::new("Number"), s), None);           // a DWORD is not read as a string
        let text = OsString::from_wide(&[0x0043, 0x00E9, 0xD83D, 0xDE00, 0xD800]);   // "Cé😀" + a lone surrogate
        set_string(OsStr::new("Text"), &text, s);
        assert_eq!(get_string(OsStr::new("Text"), s), Some(text));
        set_string(OsStr::new("Empty"), OsStr::new(""), s);
        assert_eq!(get_string(OsStr::new("Empty"), s), Some(OsString::new()));
        let k = key(s);
        unsafe {
            let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()));
        }
    }
}
