//! Network helpers for the optional upload (C21) and the AI assistant (C22), as Net.cpp had them: HTTPS through WinHTTP,
//! SHA-256/HMAC and random bytes (BCrypt), a small JSON reader/writer and secrets in the Windows Credential Manager.
//! Nothing here runs unless the user turns on a feature that needs the internet.

use std::collections::BTreeMap;
use std::ffi::c_void;

use windows::Win32::Foundation::GetLastError;
use windows::Win32::Networking::WinHttp::*;
use windows::Win32::Security::Credentials::{
    CRED_FLAGS, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredFree, CredReadW, CredWriteW,
};
use windows::Win32::Security::Cryptography::{
    BCRYPT_ALG_HANDLE, BCRYPT_ALG_HANDLE_HMAC_FLAG, BCRYPT_HASH_HANDLE, BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS,
    BCRYPT_SHA256_ALGORITHM, BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptCloseAlgorithmProvider, BCryptCreateHash,
    BCryptDestroyHash, BCryptFinishHash, BCryptGenRandom, BCryptHashData, BCryptOpenAlgorithmProvider,
};
use windows::core::{PCWSTR, PWSTR};

use crate::wide::to_wide;

// ---------------------------------------------------------------- crypto

fn hash(data: &[u8], key: Option<&[u8]>) -> Vec<u8> {
    unsafe {
        let mut alg = BCRYPT_ALG_HANDLE::default();
        let flags = if key.is_some() { BCRYPT_ALG_HANDLE_HMAC_FLAG } else { BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS(0) };
        if BCryptOpenAlgorithmProvider(&mut alg, BCRYPT_SHA256_ALGORITHM, PCWSTR::null(), flags).is_err() {
            return Vec::new();
        }
        let mut out = vec![0u8; 32];
        let mut h = BCRYPT_HASH_HANDLE::default();
        if BCryptCreateHash(alg, &mut h, None, key, 0).is_ok() {
            let _ = BCryptHashData(h, data, 0);
            let _ = BCryptFinishHash(h, &mut out, 0);
            let _ = BCryptDestroyHash(h);
        } else {
            out.clear();
        }
        let _ = BCryptCloseAlgorithmProvider(alg, 0);
        out
    }
}

pub fn sha256(data: &[u8]) -> Vec<u8> {
    hash(data, None)
}

pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> Vec<u8> {
    hash(msg, Some(key))
}

/// Lowercase hexadecimal.
pub fn hex(d: &[u8]) -> String {
    const X: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(d.len() * 2);
    for &b in d {
        s.push(X[(b >> 4) as usize] as char);
        s.push(X[(b & 15) as usize] as char);
    }
    s
}

/// Base64 with padding (RFC 4648 §4), as Base64Encode.
pub fn base64(d: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(d.len().div_ceil(3) * 4);
    for c in d.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        s.push(T[(n >> 18) as usize & 63] as char);
        s.push(T[(n >> 12) as usize & 63] as char);
        s.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        s.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    s
}

/// RFC 4648 §5 without padding (PKCE).
pub fn base64_url(d: &[u8]) -> String {
    base64(d).trim_end_matches('=').replace('+', "-").replace('/', "_")
}

pub fn random_bytes(n: usize) -> Vec<u8> {
    let mut v = vec![0u8; n];
    unsafe {
        let _ = BCryptGenRandom(None, &mut v, BCRYPT_USE_SYSTEM_PREFERRED_RNG);
    }
    v
}

/// RFC 3986: the unreserved characters are kept, everything else as %XX (and '/' too unless `keep_slash`).
pub fn url_encode(s: &[u8], keep_slash: bool) -> String {
    const X: &[u8; 16] = b"0123456789ABCDEF";
    let mut o = String::with_capacity(s.len());
    for &c in s {
        if c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b'~') || (keep_slash && c == b'/') {
            o.push(c as char);
        } else {
            o.push('%');
            o.push(X[(c >> 4) as usize] as char);
            o.push(X[(c & 15) as usize] as char);
        }
    }
    o
}

// ---------------------------------------------------------------- HTTP

#[derive(Clone, Debug, Default)]
pub struct Response {
    /// 0 = no answer (network error)
    pub status: i32,
    pub body: Vec<u8>,
    /// lowercase names
    pub headers: BTreeMap<String, String>,
    /// the WinHTTP failure
    pub error: String,
    pub url: String,
}

impl Response {
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

fn trim_header(s: &str) -> &str {
    s.trim_matches([' ', '\t', '\r', '\n', '"'])
}

struct Handle(*mut c_void);
impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

fn last_error() -> u32 {
    unsafe { GetLastError().0 }
}

/// One request; `headers` as "Name: value" lines, `body` sent as is.
pub fn request(method: &str, url: &str, headers: &[String], body: &[u8], timeout_ms: i32) -> Response {
    let mut r = Response { url: url.to_owned(), ..Default::default() };
    let wurl: Vec<u16> = url.encode_utf16().collect();
    let mut uc = URL_COMPONENTS {
        dwStructSize: size_of::<URL_COMPONENTS>() as u32,
        dwHostNameLength: u32::MAX,
        dwUrlPathLength: u32::MAX,
        dwExtraInfoLength: u32::MAX,
        ..Default::default()
    };
    if unsafe { WinHttpCrackUrl(&wurl, 0, &mut uc) }.is_err() {
        r.error = "Invalid URL".into();
        return r;
    }
    let part = |p: PWSTR, n: u32| -> Vec<u16> {
        if p.is_null() { Vec::new() } else { unsafe { std::slice::from_raw_parts(p.0, n as usize) }.to_vec() }
    };
    let host: Vec<u16> = part(uc.lpszHostName, uc.dwHostNameLength).into_iter().chain([0]).collect();
    let target: Vec<u16> = part(uc.lpszUrlPath, uc.dwUrlPathLength)
        .into_iter()
        .chain(part(uc.lpszExtraInfo, uc.dwExtraInfoLength))
        .chain([0])
        .collect();
    let agent = to_wide(format!("Snapnab/{}", crate::log::app_version()));
    let s = Handle(unsafe {
        WinHttpOpen(PCWSTR(agent.as_ptr()), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0)
    });
    if s.0.is_null() {
        r.error = "WinHttpOpen failed".into();
        return r;
    }
    unsafe {
        let _ = WinHttpSetTimeouts(s.0, timeout_ms, timeout_ms, timeout_ms, timeout_ms);
    }
    let c = Handle(unsafe { WinHttpConnect(s.0, PCWSTR(host.as_ptr()), uc.nPort, 0) });
    let verb = to_wide(method);
    let q = Handle(if c.0.is_null() {
        std::ptr::null_mut()
    } else {
        let flags = if uc.nScheme == WINHTTP_INTERNET_SCHEME_HTTPS { WINHTTP_FLAG_SECURE } else { WINHTTP_OPEN_REQUEST_FLAGS(0) };
        unsafe {
            WinHttpOpenRequest(c.0, PCWSTR(verb.as_ptr()), PCWSTR(target.as_ptr()), PCWSTR::null(), PCWSTR::null(),
                               std::ptr::null(), flags)
        }
    });
    if q.0.is_null() {
        r.error = format!("Connection failed ({})", last_error());
        return r;
    }
    let hs: Vec<u16> = headers.iter().flat_map(|h| h.encode_utf16().chain("\r\n".encode_utf16())).collect();
    let sent = unsafe {
        WinHttpSendRequest(
            q.0,
            if hs.is_empty() { None } else { Some(&hs) },
            if body.is_empty() { None } else { Some(body.as_ptr() as *const c_void) },
            body.len() as u32,
            body.len() as u32,
            0,
        )
    };
    if sent.is_err() || unsafe { WinHttpReceiveResponse(q.0, std::ptr::null_mut()) }.is_err() {
        r.error = format!("Request failed ({})", last_error());
        return r;
    }
    unsafe {
        let mut code = 0u32;
        let mut sz = 4u32;
        let _ = WinHttpQueryHeaders(q.0, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, PCWSTR::null(),
                                    Some(&mut code as *mut u32 as *mut c_void), &mut sz, std::ptr::null_mut());
        r.status = code as i32;
        let mut hl = 0u32;
        let _ = WinHttpQueryHeaders(q.0, WINHTTP_QUERY_RAW_HEADERS_CRLF, PCWSTR::null(), None, &mut hl, std::ptr::null_mut());
        if hl > 0 {
            let mut raw = vec![0u16; hl as usize / 2 + 1];
            if WinHttpQueryHeaders(q.0, WINHTTP_QUERY_RAW_HEADERS_CRLF, PCWSTR::null(), Some(raw.as_mut_ptr() as *mut c_void),
                                   &mut hl, std::ptr::null_mut())
                .is_ok()
            {
                raw.truncate(hl as usize / 2);
                for line in String::from_utf16_lossy(&raw).split("\r\n") {
                    if let Some((k, v)) = line.split_once(':') {
                        r.headers.insert(trim_header(k).to_lowercase(), trim_header(v).to_owned());
                    }
                }
            }
        }
        loop {
            let mut avail = 0u32;
            if WinHttpQueryDataAvailable(q.0, &mut avail).is_err() || avail == 0 {
                break;
            }
            let mut buf = vec![0u8; avail as usize];
            let mut rd = 0u32;
            if WinHttpReadData(q.0, buf.as_mut_ptr() as *mut c_void, avail, &mut rd).is_err() || rd == 0 {
                break;
            }
            r.body.extend_from_slice(&buf[..rd as usize]);
        }
    }
    r
}

/// A multipart/form-data body: text fields and one file part.
pub struct Multipart {
    pub boundary: String,
    pub data: Vec<u8>,
}

impl Default for Multipart {
    fn default() -> Self {
        Self::new()
    }
}

impl Multipart {
    pub fn new() -> Self {
        Multipart { boundary: format!("----SnapnabBoundary{}", hex(&random_bytes(12))), data: Vec::new() }
    }

    pub fn field(&mut self, name: &[u8], value: &[u8]) {
        self.data.extend_from_slice(format!("--{}\r\nContent-Disposition: form-data; name=\"", self.boundary).as_bytes());
        self.data.extend_from_slice(name);
        self.data.extend_from_slice(b"\"\r\n\r\n");
        self.data.extend_from_slice(value);
        self.data.extend_from_slice(b"\r\n");
    }

    pub fn file(&mut self, name: &[u8], file_name: &[u8], content_type: &str, bytes: &[u8]) {
        self.data.extend_from_slice(format!("--{}\r\nContent-Disposition: form-data; name=\"", self.boundary).as_bytes());
        self.data.extend_from_slice(name);
        self.data.extend_from_slice(b"\"; filename=\"");
        self.data.extend_from_slice(file_name);
        self.data.extend_from_slice(format!("\"\r\nContent-Type: {content_type}\r\n\r\n").as_bytes());
        self.data.extend_from_slice(bytes);
        self.data.extend_from_slice(b"\r\n");
    }

    /// The closed body.
    pub fn finish(&self) -> Vec<u8> {
        let mut d = self.data.clone();
        d.extend_from_slice(format!("--{}--\r\n", self.boundary).as_bytes());
        d
    }

    pub fn content_type(&self) -> String {
        format!("Content-Type: multipart/form-data; boundary={}", self.boundary)
    }
}

// ---------------------------------------------------------------- JSON

/// Enough JSON for API answers and requests. Objects keep their order and duplicate keys, as the C++ did.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Json {
    #[default]
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r')
}

/// strtoul(…, 16) of a short field: white space, a sign, "0x", then hex digits.
fn strtoul16(s: &[u8]) -> u32 {
    let mut i = 0;
    while i < s.len() && is_space(s[i]) {
        i += 1;
    }
    let neg = i < s.len() && s[i] == b'-';
    if i < s.len() && (s[i] == b'-' || s[i] == b'+') {
        i += 1;
    }
    if i + 1 < s.len() && s[i] == b'0' && (s[i + 1] | 0x20) == b'x' && s.get(i + 2).is_some_and(|c| c.is_ascii_hexdigit()) {
        i += 2;
    }
    let mut v = 0u32;
    while i < s.len() && s[i].is_ascii_hexdigit() {
        v = v.wrapping_mul(16).wrapping_add((s[i] as char).to_digit(16).unwrap());
        i += 1;
    }
    if neg { v.wrapping_neg() } else { v }
}

/// strtod of the number at the start of `s`: its value and length (None when there is none).
fn strtod(s: &[u8]) -> Option<(f64, usize)> {
    let mut i = 0;
    let neg = s.first() == Some(&b'-');
    if matches!(s.first(), Some(b'-' | b'+')) {
        i = 1;
    }
    let sign = if neg { -1.0 } else { 1.0 };
    let rest = &s[i..];
    let starts = |w: &str| rest.len() >= w.len() && rest[..w.len()].eq_ignore_ascii_case(w.as_bytes());
    if starts("infinity") {
        return Some((sign * f64::INFINITY, i + 8));
    }
    if starts("inf") {
        return Some((sign * f64::INFINITY, i + 3));
    }
    if starts("nan") {
        return Some((if neg { -f64::NAN } else { f64::NAN }, i + 3));
    }
    // hexadecimal: 0x digits [. digits] [p exponent]
    if rest.len() > 2 && rest[0] == b'0' && (rest[1] | 0x20) == b'x' {
        let mut j = 2;
        let mut v = 0.0f64;
        let mut digits = 0;
        let mut scale = 0i32;
        while j < rest.len() && rest[j].is_ascii_hexdigit() {
            v = v * 16.0 + (rest[j] as char).to_digit(16).unwrap() as f64;
            j += 1;
            digits += 1;
        }
        if j < rest.len() && rest[j] == b'.' {
            j += 1;
            while j < rest.len() && rest[j].is_ascii_hexdigit() {
                v = v * 16.0 + (rest[j] as char).to_digit(16).unwrap() as f64;
                scale -= 4;
                j += 1;
                digits += 1;
            }
        }
        if digits > 0 {
            if j < rest.len() && (rest[j] | 0x20) == b'p' {
                let mut k = j + 1;
                let eneg = rest.get(k) == Some(&b'-');
                if matches!(rest.get(k), Some(b'-' | b'+')) {
                    k += 1;
                }
                let start = k;
                let mut e = 0i32;
                while k < rest.len() && rest[k].is_ascii_digit() {
                    e = e.saturating_mul(10).saturating_add((rest[k] - b'0') as i32);
                    k += 1;
                }
                if k > start {
                    scale = scale.saturating_add(if eneg { -e } else { e });
                    j = k;
                }
            }
            return Some((sign * v * 2f64.powi(scale), i + j));
        }
        return Some((sign * 0.0, i + 1)); // "0x" alone: the 0
    }
    let mut j = 0;
    let mut digits = 0;
    while j < rest.len() && rest[j].is_ascii_digit() {
        j += 1;
        digits += 1;
    }
    if j < rest.len() && rest[j] == b'.' {
        j += 1;
        while j < rest.len() && rest[j].is_ascii_digit() {
            j += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return None;
    }
    if j < rest.len() && (rest[j] | 0x20) == b'e' {
        let mut k = j + 1;
        if matches!(rest.get(k), Some(b'-' | b'+')) {
            k += 1;
        }
        let start = k;
        while k < rest.len() && rest[k].is_ascii_digit() {
            k += 1;
        }
        if k > start {
            j = k;
        }
    }
    let text = std::str::from_utf8(&s[..i + j]).ok()?;
    text.parse::<f64>().ok().map(|v| (v, i + j))
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && is_space(self.s[self.i]) {
            self.i += 1;
        }
    }

    fn lit(&mut self, t: &str) -> bool {
        if self.s[self.i..].starts_with(t.as_bytes()) {
            self.i += t.len();
            true
        } else {
            false
        }
    }

    fn str(&mut self) -> Option<String> {
        let s = self.s;
        if self.i >= s.len() || s[self.i] != b'"' {
            return None;
        }
        self.i += 1;
        let mut out: Vec<u8> = Vec::new();
        while self.i < s.len() && s[self.i] != b'"' {
            let c = s[self.i];
            self.i += 1;
            if c != b'\\' {
                out.push(c);
                continue;
            }
            if self.i >= s.len() {
                return None;
            }
            let e = s[self.i];
            self.i += 1;
            match e {
                b'n' => out.push(b'\n'),
                b't' => out.push(b'\t'),
                b'r' => out.push(b'\r'),
                b'b' => out.push(8),
                b'f' => out.push(12),
                b'u' => {
                    if self.i + 4 > s.len() {
                        return None;
                    }
                    let mut cp = strtoul16(&s[self.i..self.i + 4]);
                    self.i += 4;
                    if (0xD800..0xDC00).contains(&cp) && self.i + 6 <= s.len() && s[self.i] == b'\\' && s[self.i + 1] == b'u' {
                        let lo = strtoul16(&s[self.i + 2..self.i + 6]);
                        self.i += 6;
                        cp = 0x10000u32.wrapping_add((cp - 0xD800) << 10).wrapping_add(lo.wrapping_sub(0xDC00));
                    }
                    if cp < 0x80 {
                        out.push(cp as u8);
                    } else if cp < 0x800 {
                        out.extend([(0xC0 | (cp >> 6)) as u8, (0x80 | (cp & 63)) as u8]);
                    } else if cp < 0x10000 {
                        out.extend([(0xE0 | (cp >> 12)) as u8, (0x80 | ((cp >> 6) & 63)) as u8, (0x80 | (cp & 63)) as u8]);
                    } else {
                        out.extend([(0xF0 | (cp >> 18)) as u8, (0x80 | ((cp >> 12) & 63)) as u8, (0x80 | ((cp >> 6) & 63)) as u8,
                                    (0x80 | (cp & 63)) as u8]);
                    }
                }
                _ => out.push(e),
            }
        }
        if self.i >= s.len() {
            return None;
        }
        self.i += 1;
        Some(match String::from_utf8(out) {
            Ok(t) => t,
            Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
        })
    }

    fn val(&mut self, depth: i32) -> Option<Json> {
        if depth > 200 {
            return None;
        }
        self.ws();
        let s = self.s;
        let c = *s.get(self.i)?;
        if c == b'{' {
            self.i += 1;
            let mut obj = Vec::new();
            self.ws();
            if s.get(self.i) == Some(&b'}') {
                self.i += 1;
                return Some(Json::Object(obj));
            }
            loop {
                self.ws();
                let k = self.str()?;
                self.ws();
                if s.get(self.i) != Some(&b':') {
                    return None;
                }
                self.i += 1;
                let m = self.val(depth + 1)?;
                obj.push((k, m));
                self.ws();
                match s.get(self.i) {
                    Some(b',') => self.i += 1,
                    Some(b'}') => {
                        self.i += 1;
                        return Some(Json::Object(obj));
                    }
                    _ => return None,
                }
            }
        }
        if c == b'[' {
            self.i += 1;
            let mut arr = Vec::new();
            self.ws();
            if s.get(self.i) == Some(&b']') {
                self.i += 1;
                return Some(Json::Array(arr));
            }
            loop {
                arr.push(self.val(depth + 1)?);
                self.ws();
                match s.get(self.i) {
                    Some(b',') => self.i += 1,
                    Some(b']') => {
                        self.i += 1;
                        return Some(Json::Array(arr));
                    }
                    _ => return None,
                }
            }
        }
        if c == b'"' {
            return self.str().map(Json::String);
        }
        if self.lit("true") {
            return Some(Json::Bool(true));
        }
        if self.lit("false") {
            return Some(Json::Bool(false));
        }
        if self.lit("null") {
            return Some(Json::Null);
        }
        let (v, n) = strtod(&s[self.i..])?;
        self.i += n;
        Some(Json::Number(v))
    }
}

/// A JSON string's inside: quotes, backslashes and control characters escaped; other bytes kept (as JsonEscape).
pub fn json_escape_bytes(s: &[u8]) -> Vec<u8> {
    let mut o = Vec::with_capacity(s.len());
    for &c in s {
        match c {
            b'"' => o.extend_from_slice(br#"\""#),
            b'\\' => o.extend_from_slice(br"\\"),
            b'\n' => o.extend_from_slice(br"\n"),
            b'\r' => o.extend_from_slice(br"\r"),
            b'\t' => o.extend_from_slice(br"\t"),
            c if c < 0x20 => o.extend_from_slice(format!("\\u{c:04x}").as_bytes()),
            c => o.push(c),
        }
    }
    o
}

pub fn json_escape(s: &str) -> String {
    String::from_utf8(json_escape_bytes(s.as_bytes())).unwrap_or_default()
}

impl Json {
    /// The whole text has to be one value (white space around it is allowed).
    pub fn parse(text: &[u8]) -> Option<Json> {
        let mut p = Parser { s: text, i: 0 };
        let v = p.val(0)?;
        p.ws();
        (p.i == text.len()).then_some(v)
    }

    /// The type number of Net.h (Null, Bool, Number, String, Array, Object).
    pub fn kind(&self) -> i32 {
        match self {
            Json::Null => 0,
            Json::Bool(_) => 1,
            Json::Number(_) => 2,
            Json::String(_) => 3,
            Json::Array(_) => 4,
            Json::Object(_) => 5,
        }
    }

    pub fn dump(&self) -> String {
        match self {
            Json::Bool(b) => if *b { "true" } else { "false" }.to_owned(),
            Json::Number(n) => {
                if (n - n.round()).abs() < 1e-9 && n.abs() < 1e15 {
                    format!("{}", n.round() as i64)
                } else {
                    crate::crt::sprintf("%.17g", &[crate::crt::Arg::F(*n)])
                }
            }
            Json::String(s) => format!("\"{}\"", json_escape(s)),
            Json::Array(a) => format!("[{}]", a.iter().map(Json::dump).collect::<Vec<_>>().join(",")),
            Json::Object(o) => {
                format!("{{{}}}", o.iter().map(|(k, v)| format!("\"{}\":{}", json_escape(k), v.dump())).collect::<Vec<_>>().join(","))
            }
            Json::Null => "null".to_owned(),
        }
    }

    /// An object member (the first with that key).
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(o) => o.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// A path like "choices[0].message.content", "$.data.link" or "data['key']" (a JSONPath-style subset).
    pub fn path(&self, path: &str) -> Option<&Json> {
        let p = if let Some(r) = path.strip_prefix("$.") {
            r
        } else if path == "$" {
            return Some(self);
        } else {
            path
        };
        let b = p.as_bytes();
        let mut cur = self;
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'.' {
                i += 1;
                continue;
            }
            if b[i] == b'[' {
                let e = i + p[i..].find(']')?;
                let idx = &p[i + 1..e];
                cur = if idx.starts_with(['\'', '"']) {
                    let ib = idx.as_bytes();
                    cur.get(&String::from_utf8_lossy(if ib.len() >= 2 { &ib[1..ib.len() - 1] } else { &[] }))?
                } else {
                    let n = crate::snap::atoi(idx);
                    match cur {
                        Json::Array(a) if n >= 0 => a.get(n as usize)?,
                        _ => return None,
                    }
                };
                i = e + 1;
                continue;
            }
            let e = p[i..].find(['.', '[']).map_or(p.len(), |e| i + e);
            cur = cur.get(&p[i..e])?;
            i = e;
        }
        Some(cur)
    }

    /// A string's value, a number or bool as text, "" otherwise.
    pub fn text(&self) -> String {
        match self {
            Json::String(s) => s.clone(),
            Json::Number(_) => self.dump(),
            Json::Bool(b) => if *b { "true" } else { "false" }.to_owned(),
            _ => String::new(),
        }
    }

    // builders
    pub fn str(s: impl Into<String>) -> Json {
        Json::String(s.into())
    }

    pub fn obj() -> Json {
        Json::Object(Vec::new())
    }

    pub fn arr() -> Json {
        Json::Array(Vec::new())
    }

    /// Adds a member (the value becomes an object if it was not one).
    pub fn set(mut self, k: &str, v: Json) -> Json {
        if !matches!(self, Json::Object(_)) {
            self = Json::obj();
        }
        if let Json::Object(o) = &mut self {
            o.push((k.to_owned(), v));
        }
        self
    }

    /// Adds an element (the value becomes an array if it was not one).
    pub fn push(mut self, v: Json) -> Json {
        if !matches!(self, Json::Array(_)) {
            self = Json::arr();
        }
        if let Json::Array(a) = &mut self {
            a.push(v);
        }
        self
    }
}

/// The text at `path` of a JSON answer ("" if it is not JSON or the path is missing).
pub fn json_field(body: &[u8], path: &str) -> String {
    Json::parse(body).and_then(|j| j.path(path).map(Json::text)).unwrap_or_default()
}

// ---------------------------------------------------------------- secrets (Credential Manager, "Snapnab/<name>")

pub fn save_secret(name: &str, value: &str) -> bool {
    let target = to_wide(format!("Snapnab/{name}"));
    let user = to_wide("Snapnab");
    let mut blob = value.as_bytes().to_vec();
    let c = CREDENTIALW {
        Flags: CRED_FLAGS(0),
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(target.as_ptr() as *mut u16),
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        UserName: PWSTR(user.as_ptr() as *mut u16),
        ..Default::default()
    };
    unsafe { CredWriteW(&c, 0) }.is_ok()
}

pub fn load_secret(name: &str) -> String {
    let target = to_wide(format!("Snapnab/{name}"));
    let mut c: *mut CREDENTIALW = std::ptr::null_mut();
    unsafe {
        if CredReadW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None, &mut c).is_err() || c.is_null() {
            return String::new();
        }
        let r = &*c;
        let v = if r.CredentialBlob.is_null() {
            String::new()
        } else {
            String::from_utf8_lossy(std::slice::from_raw_parts(r.CredentialBlob, r.CredentialBlobSize as usize)).into_owned()
        };
        CredFree(c as *const c_void);
        v
    }
}

pub fn delete_secret(name: &str) {
    let target = to_wide(format!("Snapnab/{name}"));
    unsafe {
        let _ = CredDeleteW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trips() {
        let j = Json::parse(r#"{"a":[1,2,{"b":"c\u00e9"}],"t":true}"#.as_bytes()).unwrap();
        assert_eq!(j.path("a[2].b").map(Json::text).as_deref(), Some("cé"));
        assert_eq!(j.dump(), r#"{"a":[1,2,{"b":"cé"}],"t":true}"#);
        assert!(Json::parse(b"[1,]").is_none());
        assert_eq!(Json::parse(b" 0x10 ").map(|j| j.dump()).as_deref(), Some("16"));
    }

    #[test]
    fn crypto() {
        assert_eq!(hex(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(base64(b"ab"), "YWI=");
        assert_eq!(base64_url(&[0xfb, 0xff]), "-_8");
        assert_eq!(url_encode(b"a b/c", true), "a%20b/c");
        assert_eq!(random_bytes(16).len(), 16);
    }
}
