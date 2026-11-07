//! printf and the number readers of the C runtime, for text that has to come out (or be read) as the C++ did it
//! (FormatA, snprintf, atoi, atof, strtoul, sscanf_s): the same static CRT functions with the options the C++ passed.
//! The C++ startup code turns on standard rounding (an exact tie goes to the even digit: 0.125 → "0.12"), so that is
//! asked for here too.

use std::ffi::{c_char, c_void};

unsafe extern "C" {
    fn __stdio_common_vsprintf(
        options: u64,
        buffer: *mut c_char,
        count: usize,
        format: *const c_char,
        locale: *mut c_void,
        args: *const u64,
    ) -> i32;
    fn __stdio_common_vsscanf(
        options: u64,
        buffer: *const c_char,
        count: usize,
        format: *const c_char,
        locale: *mut c_void,
        args: *const *mut c_void,
    ) -> i32;
    #[link_name = "atoi"]
    fn crt_atoi(s: *const c_char) -> i32;
    #[link_name = "atof"]
    fn crt_atof(s: *const c_char) -> f64;
    #[link_name = "strtoul"]
    fn crt_strtoul(s: *const c_char, end: *mut *mut c_char, base: i32) -> u32;
}

/// _CRT_INTERNAL_SCANF_SECURECRT (sscanf_s).
const SCANF_SECURE: u64 = 1;

fn cstr(s: &str) -> Vec<u8> {
    s.bytes().chain(std::iter::once(0)).collect()
}

/// atoi of the C runtime.
pub fn atoi(s: &str) -> i32 {
    let b = cstr(s);
    unsafe { crt_atoi(b.as_ptr() as _) }
}

/// atof of the C runtime (hexadecimal, inf and nan included).
pub fn atof(s: &str) -> f64 {
    let b = cstr(s);
    unsafe { crt_atof(b.as_ptr() as _) }
}

/// strtoul(s, nullptr, base) of the C runtime (32 bits, as unsigned long on Windows).
pub fn strtoul(s: &str, base: i32) -> u32 {
    let b = cstr(s);
    unsafe { crt_strtoul(b.as_ptr() as _, std::ptr::null_mut(), base) }
}

/// sscanf_s(text, format, &f[0], &f[1]…, &used) where `format` has one %f per float and ends in %n: what sscanf_s
/// returns and the %n count. `text` must end with a 0 byte.
pub fn scan_floats(text: &[u8], format: &str, f: &mut [f32]) -> (i32, i32) {
    debug_assert_eq!(text.last(), Some(&0));
    let fmt = cstr(format);
    let mut used: i32 = 0;
    let mut args: Vec<*mut c_void> = f.iter_mut().map(|v| v as *mut f32 as *mut c_void).collect();
    args.push(&mut used as *mut i32 as *mut c_void);
    let r = unsafe {
        __stdio_common_vsscanf(SCANF_SECURE, text.as_ptr() as _, usize::MAX, fmt.as_ptr() as _, std::ptr::null_mut(), args.as_ptr())
    };
    (r, used)
}

/// _CRT_INTERNAL_PRINTF_STANDARD_SNPRINTF_BEHAVIOR: the length is returned when the buffer is too small (snprintf).
const STANDARD_SNPRINTF: u64 = 2;
/// _CRT_INTERNAL_PRINTF_STANDARD_ROUNDING, which the C++ startup sets for every C++ printf.
const STANDARD_ROUNDING: u64 = 32;

/// One printf argument; on x64 each takes an 8-byte slot of the va_list.
#[derive(Clone, Copy, Debug)]
pub enum Arg {
    I(i64),
    F(f64),
}

/// sprintf with numeric arguments (%d, %zu, %f, %g…).
pub fn sprintf(format: &str, args: &[Arg]) -> String {
    let fmt: Vec<u8> = format.bytes().chain(std::iter::once(0)).collect();
    let slots: Vec<u64> = args
        .iter()
        .map(|a| match *a {
            Arg::I(v) => v as u64,
            Arg::F(v) => v.to_bits(),
        })
        .collect();
    let call = |buf: *mut c_char, len: usize| unsafe {
        __stdio_common_vsprintf(STANDARD_SNPRINTF | STANDARD_ROUNDING, buf, len, fmt.as_ptr() as _, std::ptr::null_mut(), slots.as_ptr())
    };
    let n = call(std::ptr::null_mut(), 0);
    if n <= 0 {
        return String::new();
    }
    let mut buf = vec![0u8; n as usize + 1];
    let n = call(buf.as_mut_ptr() as _, buf.len()).max(0) as usize;
    buf.truncate(n);
    String::from_utf8_lossy(&buf).into_owned()
}

/// "%.{decimals}f" of one value.
pub fn fixed(v: f64, decimals: u32) -> String {
    sprintf(&format!("%.{decimals}f"), &[Arg::F(v)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_like_the_crt() {
        assert_eq!(fixed(0.125, 2), "0.12");
        assert_eq!(fixed(0.375, 2), "0.38");
        assert_eq!(fixed(0.03125, 4), "0.0312");
        assert_eq!(fixed(1.0625, 3), "1.062");
        assert_eq!(fixed(-0.0, 2), "-0.00");
        assert_eq!(fixed(595.28, 2), "595.28");
        assert_eq!(sprintf("%zu 0 obj %d %g", &[Arg::I(12), Arg::I(-3), Arg::F(0.5)]), "12 0 obj -3 0.5");
        assert_eq!(sprintf("%010zu 00000 n ", &[Arg::I(1234)]), "0000001234 00000 n ");
    }

    #[test]
    fn reads_like_the_crt() {
        assert_eq!(atoi(" -12x"), -12);
        assert_eq!(atof("0x1A"), 26.0);
        assert_eq!(atof(" 1e2abc"), 100.0);
        assert_eq!(strtoul("abcdeg", 16), 0xabcde);
        let mut f = [0f32; 2];
        assert_eq!(scan_floats(b"1.5, 2 3,4\0", "%f,%f %n", &mut f), (2, 7));
        assert_eq!(f, [1.5, 2.0]);
        assert_eq!(scan_floats(b"1 ,2\0", "%f,%f %n", &mut f).0, 1);
        assert_eq!(scan_floats(b"\0", "%f,%f %n", &mut f).0, -1);
    }
}
