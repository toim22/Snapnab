//! UTF-16 strings, as Windows and the C++ side use them. Lone surrogates survive the trip through `OsString`.

use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};

/// `s` as UTF-16 with a terminating nul.
pub fn to_wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(Some(0)).collect()
}

/// The nul-terminated string at `p`, without the nul; None for a null pointer.
///
/// # Safety
/// `p` is null or points to a nul-terminated UTF-16 string that outlives the returned slice.
pub unsafe fn from_ptr<'a>(p: *const u16) -> Option<&'a [u16]> {
    if p.is_null() {
        return None;
    }
    let mut n = 0;
    while unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    Some(unsafe { std::slice::from_raw_parts(p, n) })
}

pub fn to_os(w: &[u16]) -> OsString {
    OsString::from_wide(w)
}

/// For text that only goes into the log or a comparison: invalid UTF-16 becomes U+FFFD.
pub fn to_string_lossy(w: &[u16]) -> String {
    String::from_utf16_lossy(w)
}
