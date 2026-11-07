//! Editor tools (EditorTools.cpp): combine captures (C09), automatic redaction (C11), the difference image (C17), the
//! date and time stamp (C26), background removal (C27), splitting long images (C34) and the contrast checker (C36).
//! Only the logic is here; the forms that ask for the options are in the app.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::OnceLock;

use regex::Regex;
use snapcore::color::contrast_ratio;
use snapcore::gdip::scale_image;
use snapcore::image::{Image, Rect, argb, ca, cb, cg, cr, from_colorref};
use snapcore::ocr::Line;

use crate::tr;

fn reg_dw(name: &str, def: u32) -> u32 {
    snapcore::reg::get_dword(name.as_ref(), def, Some("Editor".as_ref()))
}

fn set_reg_dw(name: &str, v: u32) {
    snapcore::reg::set_dword(name.as_ref(), v, Some("Editor".as_ref()));
}

// ================================================================ C09 combine
#[derive(Clone, Debug, PartialEq)]
pub struct CombineOptions {
    pub layout: i32, // 0 side by side, 1 stacked, 2 grid
    pub columns: i32,
    pub gap: i32,
    pub margin: i32,
    pub align: i32, // 0 top/left, 1 center, 2 bottom/right
    pub same_size: bool,
    pub transparent: bool,
    pub background: u32, // COLORREF
}

impl Default for CombineOptions {
    fn default() -> Self {
        CombineOptions { layout: 0, columns: 2, gap: 16, margin: 0, align: 1, same_size: false, transparent: false, background: 0x00FF_FFFF }
    }
}

impl CombineOptions {
    pub fn load() -> Self {
        CombineOptions {
            layout: (reg_dw("CombineLayout", 0) as i32).clamp(0, 2),
            columns: (reg_dw("CombineColumns", 2) as i32).clamp(1, 20),
            gap: (reg_dw("CombineGap", 16) as i32).clamp(0, 500),
            margin: (reg_dw("CombineMargin", 0) as i32).clamp(0, 500),
            align: (reg_dw("CombineAlign", 1) as i32).clamp(0, 2),
            same_size: reg_dw("CombineSameSize", 0) != 0,
            transparent: reg_dw("CombineTransparent", 0) != 0,
            background: reg_dw("CombineBackground", 0x00FF_FFFF),
        }
    }

    pub fn save(&self) {
        set_reg_dw("CombineLayout", self.layout as u32);
        set_reg_dw("CombineColumns", self.columns as u32);
        set_reg_dw("CombineGap", self.gap as u32);
        set_reg_dw("CombineMargin", self.margin as u32);
        set_reg_dw("CombineAlign", self.align as u32);
        set_reg_dw("CombineSameSize", self.same_size as u32);
        set_reg_dw("CombineTransparent", self.transparent as u32);
        set_reg_dw("CombineBackground", self.background);
    }
}

/// The images in one picture; `placed` gets where each one went.
pub fn combine_images(input: &[Image], o: &CombineOptions, mut placed: Option<&mut Vec<Rect>>) -> Image {
    let mut imgs: Vec<Image> = input.iter().filter(|i| !i.is_empty()).cloned().collect();
    if let Some(p) = placed.as_deref_mut() {
        p.clear();
    }
    if imgs.is_empty() {
        return Image::default();
    }
    let n = imgs.len() as i32;
    let cols = match o.layout {
        0 => n,
        1 => 1,
        _ => o.columns.clamp(1, n),
    };
    let rows = (n + cols - 1) / cols;
    if o.same_size {
        let max_w = imgs.iter().map(|i| i.wi()).max().unwrap_or(0);
        let max_h = imgs.iter().map(|i| i.hi()).max().unwrap_or(0);
        for i in imgs.iter_mut() {
            if o.layout == 0 && i.hi() != max_h {
                let w = ((i.wi() as f64 * max_h as f64 / i.hi() as f64).round() as i32).max(1);
                *i = scale_image(i, w, max_h, true);
            } else if o.layout != 0 && i.wi() != max_w {
                let h = ((i.hi() as f64 * max_w as f64 / i.wi() as f64).round() as i32).max(1);
                *i = scale_image(i, max_w, h, true);
            }
        }
    }
    let mut col_w = vec![0; cols as usize];
    let mut row_h = vec![0; rows as usize];
    for (i, im) in imgs.iter().enumerate() {
        let (c, r) = (i % cols as usize, i / cols as usize);
        col_w[c] = col_w[c].max(im.wi());
        row_h[r] = row_h[r].max(im.hi());
    }
    let w = 2 * o.margin + o.gap * (cols - 1) + col_w.iter().sum::<i32>();
    let h = 2 * o.margin + o.gap * (rows - 1) + row_h.iter().sum::<i32>();
    if w as i64 * h as i64 > 400_000_000 || w <= 0 || h <= 0 {
        return Image::default();
    }
    let mut out = Image::sized(w, h, if o.transparent { 0 } else { from_colorref(o.background) });
    let mut y = o.margin;
    for r in 0..rows {
        let mut x = o.margin;
        for c in 0..cols {
            let i = r * cols + c;
            if i >= n {
                break;
            }
            let im = &imgs[i as usize];
            let (fx, fy) = (col_w[c as usize] - im.wi(), row_h[r as usize] - im.hi());
            let pick = |f: i32| match o.align {
                0 => 0,
                1 => f / 2,
                _ => f,
            };
            let ox = if o.layout != 0 { pick(fx) } else { 0 };
            let oy = if o.layout != 1 { pick(fy) } else { 0 };
            out.blit(im, x + ox, y + oy, true);
            if let Some(p) = placed.as_deref_mut() {
                p.push(Rect::new(x + ox, y + oy, x + ox + im.wi(), y + oy + im.hi()));
            }
            x += col_w[c as usize] + o.gap;
        }
        y += row_h[r as usize] + o.gap;
    }
    out
}

// ================================================================ C11 automatic redaction
#[derive(Clone, Debug, PartialEq)]
pub struct RedactItem {
    pub kind: String,
    pub text: String,
    pub rect: Rect,
    pub on: bool,
}

fn luhn(digits: &str) -> bool {
    let mut sum = 0;
    for (i, c) in digits.bytes().rev().enumerate() {
        let mut d = (c - b'0') as i32;
        if i % 2 == 1 {
            d *= 2;
            if d > 9 {
                d -= 9;
            }
        }
        sum += d;
    }
    digits.len() >= 13 && sum % 10 == 0
}

fn digits_of(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_digit()).collect()
}

fn iban_valid(s: &str) -> bool {
    let t: String = s.chars().filter(|&c| c != ' ').map(|c| c.to_ascii_uppercase()).collect();
    if t.len() < 15 || t.len() > 34 || !t.is_ascii() {
        return false;
    }
    let t = format!("{}{}", &t[4..], &t[..4]);
    let mut m = 0i64;
    for c in t.chars() {
        let v = match c {
            '0'..='9' => c as i64 - '0' as i64,
            'A'..='Z' => c as i64 - 'A' as i64 + 10,
            _ => return false,
        };
        m = if v >= 10 { (m * 100 + v) % 97 } else { (m * 10 + v) % 97 };
    }
    m == 1
}

fn cpf_valid(s: &str) -> bool {
    let d: Vec<i32> = digits_of(s).bytes().map(|c| (c - b'0') as i32).collect();
    if d.len() != 11 || d.iter().all(|&x| x == d[0]) {
        return false;
    }
    for k in 9..=10usize {
        let sum: i32 = (0..k).map(|i| d[i] * (k as i32 + 1 - i as i32)).sum();
        if sum * 10 % 11 % 10 != d[k] {
            return false;
        }
    }
    true
}

struct Pattern {
    kind: &'static str,
    re: Regex,
    whole: Regex,
    check: Option<fn(&str) -> bool>,
}

fn patterns() -> &'static [Pattern] {
    static P: OnceLock<Vec<Pattern>> = OnceLock::new();
    P.get_or_init(|| {
        let list: [(&'static str, &str, Option<fn(&str) -> bool>); 10] = [
            (
                "Link com token",
                r"(?i)https?://\S+?[?&#](?:token|key|apikey|api_key|access_token|auth|sig|signature|code|secret|password|pwd|session)=[^\s&]+\S*",
                None,
            ),
            (
                "Chave de API",
                r"(?:sk-[A-Za-z0-9_-]{16,}|AKIA[0-9A-Z]{16}|gh[pousr]_[A-Za-z0-9]{30,}|xox[baprs]-[A-Za-z0-9-]{10,}|AIza[0-9A-Za-z_-]{35}|eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{5,})",
                None,
            ),
            ("E-mail", r"[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}", None),
            ("IBAN", r"\b[A-Z]{2}\d{2}(?: ?[A-Z0-9]{4}){2,7}(?: ?[A-Z0-9]{1,4})?\b", Some(iban_valid)),
            ("CNPJ", r"\b\d{2}\.\d{3}\.\d{3}/\d{4}-\d{2}\b", None),
            ("CPF", r"\b\d{3}\.\d{3}\.\d{3}-\d{2}\b", Some(cpf_valid)),
            ("Cartão", r"\b(?:\d[ -]?){12,18}\d\b", Some(|s| luhn(&digits_of(s)))),
            ("Documento (SSN)", r"\b\d{3}-\d{2}-\d{4}\b", None),
            ("Endereço IP", r"\b(?:(?:25[0-5]|2[0-4]\d|1?\d?\d)\.){3}(?:25[0-5]|2[0-4]\d|1?\d?\d)\b", None),
            (
                "Telefone",
                r"(?:\+\d{1,3}[ .-]?)?(?:\(\d{1,4}\)[ .-]?)?\d{2,5}[ .-]?\d{3,5}(?:[ .-]?\d{2,5})?",
                Some(|s| (8..=15).contains(&digits_of(s).len())),
            ),
        ];
        list.into_iter()
            .map(|(kind, re, check)| {
                // ASCII digits, as std::wregex
                let re = re.replace(r"\d", "[0-9]");
                Pattern { kind, re: Regex::new(&re).unwrap(), whole: Regex::new(&format!("^(?:{re})$")).unwrap(), check }
            })
            .collect()
    })
}

/// The kind of sensitive data a whole piece of text is ("" = none), untranslated.
pub fn sensitive_kind(text: &str) -> &'static str {
    let t = text.trim();
    patterns()
        .iter()
        .find(|p| p.whole.is_match(t) && p.check.is_none_or(|c| c(t)))
        .map_or("", |p| p.kind)
}

/// The sensitive pieces of the recognized text, each with the union of its words' boxes.
pub fn find_sensitive(lines: &[Line]) -> Vec<RedactItem> {
    let mut out = Vec::new();
    for line in lines {
        let mut t = String::new();
        let mut ranges = Vec::new();
        for w in &line.words {
            if !t.is_empty() {
                t.push(' ');
            }
            let s = String::from_utf16_lossy(&w.text);
            ranges.push((t.len(), t.len() + s.len()));
            t.push_str(&s);
        }
        let mut used = vec![false; t.len()];
        for p in patterns() {
            for m in p.re.find_iter(&t) {
                let (a, mut b) = (m.start(), m.end());
                let mut s = m.as_str();
                while let Some(c) = s.chars().last()
                    && ".,;:)]}>\"'".contains(c)
                {
                    s = &s[..s.len() - c.len_utf8()];
                    b -= c.len_utf8();
                }
                if s.is_empty() || p.check.is_some_and(|c| !c(s)) || used[a..b].iter().any(|&u| u) {
                    continue;
                }
                used[a..b].iter_mut().for_each(|u| *u = true);
                let mut u: Option<Rect> = None;
                for (k, &(ra, rb)) in ranges.iter().enumerate() {
                    if rb <= a || ra >= b {
                        continue;
                    }
                    let r = line.words[k].rect;
                    u = Some(u.map_or(r, |x| {
                        Rect::new(x.left.min(r.left), x.top.min(r.top), x.right.max(r.right), x.bottom.max(r.bottom))
                    }));
                }
                let Some(u) = u else { continue };
                out.push(RedactItem {
                    kind: tr(p.kind),
                    text: s.to_string(),
                    rect: Rect::new(u.left - 3, u.top - 3, u.right + 3, u.bottom + 3),
                    on: true,
                });
            }
        }
    }
    out
}

/// Hides an area: 0 blur, 1 pixelate, 2 black box.
pub fn apply_redaction(img: &mut Image, r: Rect, style: i32) {
    let r = r.intersect(&Rect::new(0, 0, img.wi(), img.hi()));
    if r.is_empty() {
        return;
    }
    let h = r.height();
    match style {
        0 => {
            snapcore::fx::box_blur(img, r, (h / 3).max(4));
            snapcore::fx::box_blur(img, r, (h / 3).max(4));
        }
        1 => snapcore::fx::pixelate(img, r, (h / 3).max(6)),
        _ => img.fill_rect(r, 0xFF00_0000),
    }
}

// ================================================================ C17 difference image
/// Pixels that differ more than `tolerance` in red over a faded gray copy of `a`; also the count and the box.
pub fn diff_image(a: &Image, b: &Image, tolerance: i32) -> (Image, usize, Rect) {
    let (w, h) = (a.wi().max(b.wi()), a.hi().max(b.hi()));
    let mut out = Image::sized(w, h, 0);
    let mut changed = 0;
    let mut bx = Rect::new(i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for y in 0..h {
        for x in 0..w {
            let ina = x < a.wi() && y < a.hi();
            let inb = x < b.wi() && y < b.hi();
            let pa = if ina { a.at(x, y) } else { 0 };
            let pb = if inb { b.at(x, y) } else { 0 };
            let diff = ina != inb
                || (cr(pa) - cr(pb)).abs() > tolerance
                || (cg(pa) - cg(pb)).abs() > tolerance
                || (cb(pa) - cb(pb)).abs() > tolerance
                || (ca(pa) - ca(pb)).abs() > tolerance;
            if diff {
                *out.at_mut(x, y) = 0xFFE5_322D;
                changed += 1;
                bx = Rect::new(bx.left.min(x), bx.top.min(y), bx.right.max(x + 1), bx.bottom.max(y + 1));
            } else {
                let l = (cr(pa) * 30 + cg(pa) * 59 + cb(pa) * 11) / 100;
                let v = 255 - (255 - l) / 4;
                *out.at_mut(x, y) = argb(255, v, v, v);
            }
        }
    }
    (out, changed, if changed > 0 { bx } else { Rect::default() })
}

// ================================================================ C26 date and time stamp
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DateTime {
    pub year: i32,
    pub month: i32,
    pub day: i32,
    pub weekday: i32, // 0 = Sunday
    pub hour: i32,
    pub minute: i32,
    pub second: i32,
}

impl DateTime {
    pub fn now() -> DateTime {
        let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
        DateTime {
            year: t.wYear as i32,
            month: t.wMonth as i32,
            day: t.wDay as i32,
            weekday: t.wDayOfWeek as i32,
            hour: t.wHour as i32,
            minute: t.wMinute as i32,
            second: t.wSecond as i32,
        }
    }

    fn system_time(&self) -> windows::Win32::Foundation::SYSTEMTIME {
        windows::Win32::Foundation::SYSTEMTIME {
            wYear: self.year as u16,
            wMonth: self.month as u16,
            wDayOfWeek: self.weekday as u16,
            wDay: self.day as u16,
            wHour: self.hour as u16,
            wMinute: self.minute as u16,
            wSecond: self.second as u16,
            wMilliseconds: 0,
        }
    }
}

pub fn date_stamp_format() -> String {
    snapcore::reg::get_string("DateStampFormat".as_ref(), Some("Editor".as_ref()))
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub fn set_date_stamp_format(f: &str) {
    snapcore::reg::set_string("DateStampFormat".as_ref(), f.as_ref(), Some("Editor".as_ref()));
}

fn ui_locale() -> Vec<u16> {
    let c = snapcore::i18n::current();
    let l = match c {
        "pt" => "pt-BR",
        "zh" => "zh-CN",
        other => other,
    };
    l.encode_utf16().chain(Some(0)).collect()
}

/// A date (picture like "MMMM") or the short date / time in the interface language (user's locale as fallback).
fn locale_format(t: &DateTime, pic: Option<&str>, time: bool) -> String {
    use windows::Win32::Globalization::{DATE_SHORTDATE, ENUM_DATE_FORMATS_FLAGS, GetDateFormatEx, GetTimeFormatEx, TIME_FORMAT_FLAGS};
    use windows::core::PCWSTR;
    let st = t.system_time();
    let loc = ui_locale();
    let pic_w: Option<Vec<u16>> = pic.map(|p| p.encode_utf16().chain(Some(0)).collect());
    let pic_p = pic_w.as_ref().map_or(PCWSTR::null(), |p| PCWSTR(p.as_ptr()));
    let mut buf = [0u16; 128];
    for l in [PCWSTR(loc.as_ptr()), PCWSTR::null()] {
        let n = unsafe {
            if time {
                GetTimeFormatEx(l, TIME_FORMAT_FLAGS(0), Some(&st), pic_p, Some(&mut buf))
            } else {
                let flags = if pic.is_none() { DATE_SHORTDATE } else { ENUM_DATE_FORMATS_FLAGS(0) };
                GetDateFormatEx(l, flags, Some(&st), pic_p, Some(&mut buf), PCWSTR::null())
            }
        };
        if n > 0 {
            return String::from_utf16_lossy(&buf[..n as usize - 1]);
        }
    }
    String::new()
}

/// Tokens: yyyy yy MMMM MMM MM M dddd ddd dd d HH H hh h mm m ss s t; text in '…' is kept; an empty format = the
/// short date and time.
pub fn date_stamp_text(format: &str, t: &DateTime) -> String {
    if format.trim().is_empty() {
        return format!("{} {}", locale_format(t, None, false), locale_format(t, None, true));
    }
    let f: Vec<char> = format.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < f.len() {
        let c = f[i];
        if c == '\'' {
            let e = f[i + 1..].iter().position(|&x| x == '\'').map_or(f.len(), |p| i + 1 + p);
            out.extend(&f[i + 1..e]);
            i = e + 1;
            continue;
        }
        let mut n = 1;
        while i + n < f.len() && f[i + n] == c {
            n += 1;
        }
        let one = |v: i32, two: bool| if two { format!("{v:02}") } else { v.to_string() };
        match c {
            'y' => out += &if n >= 4 { format!("{:04}", t.year) } else { format!("{:02}", t.year % 100) },
            'M' if n >= 3 => out += &locale_format(t, Some(if n >= 4 { "MMMM" } else { "MMM" }), false),
            'M' => out += &one(t.month, n == 2),
            'd' if n >= 3 => out += &locale_format(t, Some(if n >= 4 { "dddd" } else { "ddd" }), false),
            'd' => out += &one(t.day, n == 2),
            'H' => out += &one(t.hour, n >= 2),
            'h' => {
                let h12 = if t.hour % 12 != 0 { t.hour % 12 } else { 12 };
                out += &one(h12, n >= 2)
            }
            'm' => out += &one(t.minute, n >= 2),
            's' => out += &one(t.second, n >= 2),
            't' => out += if t.hour < 12 { "AM" } else { "PM" },
            _ => out.extend(&f[i..i + n]),
        }
        i += n;
    }
    out
}

// ================================================================ C27 background removal
/// The background is what touches the edges and looks like the border colors; the main subject stays.
/// `tolerance` 1..100, `feather` px of soft edge.
pub fn remove_background(src: &Image, tolerance: i32, feather: i32) -> Image {
    if src.is_empty() {
        return src.clone();
    }
    let (w, h) = (src.wi(), src.hi());
    let (wu, n) = (w as usize, (w * h) as usize);
    let t = 6.0 + tolerance.clamp(1, 100) as f32 * 1.4;
    let dist = |a: u32, b: u32| {
        let (dr, dg, db) = ((cr(a) - cr(b)) as f32, (cg(a) - cg(b)) as f32, (cb(a) - cb(b)) as f32);
        (dr * dr + dg * dg + db * db).sqrt()
    };
    // 1. background colors: the dominant colors of the border
    let mut buckets: BTreeMap<u32, (i32, [i64; 4])> = BTreeMap::new();
    let mut add_border = |p: u32| {
        if ca(p) < 16 {
            return;
        }
        let key = ((cr(p) >> 4) << 8 | (cg(p) >> 4) << 4 | (cb(p) >> 4)) as u32;
        let e = buckets.entry(key).or_insert((0, [0; 4]));
        e.0 += 1;
        e.1[0] += cr(p) as i64;
        e.1[1] += cg(p) as i64;
        e.1[2] += cb(p) as i64;
        e.1[3] += 1;
    };
    for x in 0..w {
        add_border(src.at(x, 0));
        add_border(src.at(x, h - 1));
    }
    for y in 0..h {
        add_border(src.at(0, y));
        add_border(src.at(w - 1, y));
    }
    let total: i32 = buckets.values().map(|b| b.0).sum();
    let mut order: Vec<(i32, u32)> = buckets.iter().map(|(&k, v)| (v.0, k)).collect();
    order.sort_by(|a, b| b.cmp(a));
    let mut bg = Vec::new();
    let mut covered = 0;
    for (cnt, k) in order {
        let s = buckets[&k].1;
        bg.push(argb(255, (s[0] / s[3]) as i32, (s[1] / s[3]) as i32, (s[2] / s[3]) as i32));
        covered += cnt;
        if covered >= total * 85 / 100 || bg.len() >= 12 {
            break;
        }
    }
    let near_bg = |p: u32| bg.iter().map(|&b| dist(p, b)).fold(1e9f32, f32::min);
    // 2. flood from the border through background-like pixels (or a smooth gradient of them)
    let mut is_bg = vec![false; n];
    let mut q = VecDeque::new();
    let seed = |x: i32, y: i32, is_bg: &mut Vec<bool>, q: &mut VecDeque<usize>| {
        let i = (y * w + x) as usize;
        if is_bg[i] {
            return;
        }
        let p = src.px[i];
        if ca(p) < 16 || near_bg(p) < t {
            is_bg[i] = true;
            q.push_back(i);
        }
    };
    for x in 0..w {
        seed(x, 0, &mut is_bg, &mut q);
        seed(x, h - 1, &mut is_bg, &mut q);
    }
    for y in 0..h {
        seed(0, y, &mut is_bg, &mut q);
        seed(w - 1, y, &mut is_bg, &mut q);
    }
    let neighbors = |i: usize| {
        let (x, y) = ((i % wu) as i32, (i / wu) as i32);
        [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)]
            .into_iter()
            .filter(move |&(a, b)| a >= 0 && b >= 0 && a < w && b < h)
            .map(move |(a, b)| (b * w + a) as usize)
    };
    while let Some(i) = q.pop_front() {
        for j in neighbors(i) {
            if is_bg[j] {
                continue;
            }
            let p = src.px[j];
            let d = near_bg(p);
            if ca(p) < 16 || d < t || (dist(p, src.px[i]) < t * 0.25 && d < t * 2.2) {
                is_bg[j] = true;
                q.push_back(j);
            }
        }
    }
    // 3. keep the main subject: pieces much smaller than the largest become background
    let mut comp = vec![usize::MAX; n];
    let mut sizes = Vec::new();
    for i in 0..n {
        if is_bg[i] || comp[i] != usize::MAX {
            continue;
        }
        let id = sizes.len();
        let mut cnt = 0;
        comp[i] = id;
        q.push_back(i);
        while let Some(c) = q.pop_front() {
            cnt += 1;
            for j in neighbors(c) {
                if !is_bg[j] && comp[j] == usize::MAX {
                    comp[j] = id;
                    q.push_back(j);
                }
            }
        }
        sizes.push(cnt);
    }
    let Some(&largest) = sizes.iter().max() else { return src.clone() };
    // 4. alpha mask, with a soft edge
    let mut a: Vec<f32> = (0..n).map(|i| if !is_bg[i] && sizes[comp[i]] * 50 >= largest { 1.0 } else { 0.0 }).collect();
    let r = feather.clamp(0, 20);
    if r > 0 {
        let mut tmp = vec![0f32; n];
        for y in 0..h {
            let (mut acc, mut cnt) = (0f32, 0);
            for x in -r..w {
                if x + r < w {
                    acc += a[(y * w + x + r) as usize];
                    cnt += 1;
                }
                if x - r - 1 >= 0 {
                    acc -= a[(y * w + x - r - 1) as usize];
                    cnt -= 1;
                }
                if x >= 0 {
                    tmp[(y * w + x) as usize] = acc / cnt as f32;
                }
            }
        }
        for x in 0..w {
            let (mut acc, mut cnt) = (0f32, 0);
            for y in -r..h {
                if y + r < h {
                    acc += tmp[((y + r) * w + x) as usize];
                    cnt += 1;
                }
                if y - r - 1 >= 0 {
                    acc -= tmp[((y - r - 1) * w + x) as usize];
                    cnt -= 1;
                }
                if y >= 0 {
                    let i = (y * w + x) as usize;
                    let v = acc / cnt as f32;
                    a[i] = (if a[i] > 0.0 { 1.0f32 } else { 0.0 }).min(v) * 0.5 + v * 0.5;
                }
            }
        }
    }
    let mut out = src.clone();
    for (i, p) in out.px.iter_mut().enumerate() {
        let al = (ca(*p) as f32 * a[i].clamp(0.0, 1.0)).round() as u32;
        *p = (*p & 0x00FF_FFFF) | (al << 24);
    }
    out
}

// ================================================================ C34 split long images
/// The parts between the cuts (rows, or columns when `vertical`).
pub fn split_image(img: &Image, cuts: &[i32], vertical: bool) -> Vec<Image> {
    let mut cuts = cuts.to_vec();
    cuts.sort();
    let len = if vertical { img.wi() } else { img.hi() };
    cuts.push(len);
    let mut prev = 0;
    let mut parts = Vec::new();
    for c in cuts {
        let c = c.clamp(0, len);
        if c - prev < 1 {
            continue;
        }
        parts.push(img.crop(if vertical { Rect::new(prev, 0, c, img.hi()) } else { Rect::new(0, prev, img.wi(), c) }));
        prev = c;
    }
    parts
}

/// The split dialog's choices: cut direction (vertical = parts side by side), by size or count, smart breaks.
pub fn split_cuts(img: &Image, vertical: bool, by_count: bool, size: i32, parts: i32, smart: bool) -> Vec<i32> {
    let len = if vertical { img.wi() } else { img.hi() };
    let size = if by_count { (len + parts - 1) / parts.max(1) } else { size }.max(16);
    if smart {
        let t = if vertical { snapcore::fx::rotate90(img, true) } else { img.clone() };
        return if by_count { snapcore::pdf::smart_cuts(&t, parts.max(2)) } else { snapcore::pdf::smart_breaks(&t, size, -1) };
    }
    if by_count {
        (1..parts).map(|i| (len as i64 * i as i64 / parts as i64) as i32).collect()
    } else {
        (1..).map(|k| k * size).take_while(|&p| p < len).collect()
    }
}

// ================================================================ C36 contrast checker
/// Ratio and WCAG AA/AAA verdicts.
pub fn contrast_report(fg: u32, bg: u32) -> String {
    let r = contrast_ratio(fg, bg);
    let v = |need: f64| if r + 1e-9 >= need { tr("passa") } else { tr("falha") };
    snapcore::i18n::fill(
        &tr("Contraste %.2f:1\n\nTexto normal — AA (4,5:1): %s · AAA (7:1): %s\nTexto grande — AA (3:1): %s · AAA (4,5:1): %s\nComponentes e gráficos — AA (3:1): %s"),
        &[&format!("{r:.2}"), &v(4.5), &v(7.0), &v(3.0), &v(4.5), &v(3.0)],
    )
}

/// The two main colors of an area (COLORREF): background = the most common, text = the most common that contrasts.
pub fn dominant_colors(img: &Image) -> Option<(u32, u32)> {
    let mut hist: HashMap<u32, i32> = HashMap::new();
    for &p in &img.px {
        if ca(p) > 128 {
            *hist.entry(((cr(p) >> 4) << 8 | (cg(p) >> 4) << 4 | (cb(p) >> 4)) as u32).or_default() += 1;
        }
    }
    let color = |k: u32| snapcore::color::rgb(((k >> 8) & 15) * 17, ((k >> 4) & 15) * 17, (k & 15) * 17);
    let (&best, _) = hist.iter().max_by_key(|(k, n)| (**n, std::cmp::Reverse(**k)))?;
    let bg = color(best);
    let mut fg = if contrast_ratio(bg, 0) > contrast_ratio(bg, 0x00FF_FFFF) { 0 } else { 0x00FF_FFFF };
    let mut best_score = -1.0;
    let mut keys: Vec<_> = hist.iter().collect();
    keys.sort();
    for (&k, &n) in keys {
        let c = contrast_ratio(bg, color(k));
        if c < 1.6 || (n as usize) * 400 < img.px.len() {
            continue;
        }
        let score = n as f64 * c.ln();
        if score > best_score {
            best_score = score;
            fg = color(k);
        }
    }
    Some((fg, bg))
}

#[cfg(test)]
mod tests {
    use super::*;
    use snapcore::ocr::Word;

    #[test]
    fn sensitive_kinds() {
        assert_eq!(sensitive_kind("ana@example.com"), "E-mail");
        assert_eq!(sensitive_kind("4111 1111 1111 1111"), "Cartão");
        assert_eq!(sensitive_kind("4111 1111 1111 1112"), "");
        assert_eq!(sensitive_kind("111.444.777-35"), "CPF");
        assert_eq!(sensitive_kind("GB82 WEST 1234 5698 7654 32"), "IBAN");
        assert_eq!(sensitive_kind("192.168.0.1"), "Endereço IP");
        assert_eq!(sensitive_kind("https://x.io/a?token=abc123"), "Link com token");
        assert_eq!(sensitive_kind("+55 11 98765-4321"), "Telefone");
        assert_eq!(sensitive_kind("hello"), "");
    }

    #[test]
    fn finds_words_and_boxes() {
        let w = |t: &str, x: i32| Word { text: t.encode_utf16().collect(), rect: Rect::new(x, 10, x + 50, 20) };
        let line = Line { text: vec![], rect: Rect::default(), words: vec![w("mail:", 0), w("ana@example.com.", 60), w("ok", 120)] };
        let items = find_sensitive(&[line]);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].text, "ana@example.com");
        assert_eq!(items[0].rect, Rect::new(57, 7, 113, 23));
    }

    #[test]
    fn combine_side_by_side_and_stacked() {
        let a = Image::sized(10, 20, 0xFFFF_0000);
        let b = Image::sized(30, 10, 0xFF00_FF00);
        let mut placed = Vec::new();
        let o = CombineOptions { gap: 4, margin: 2, ..Default::default() };
        let out = combine_images(&[a.clone(), b.clone()], &o, Some(&mut placed));
        assert_eq!((out.wi(), out.hi()), (2 + 10 + 4 + 30 + 2, 2 + 20 + 2));
        assert_eq!(placed[1], Rect::new(16, 2 + 5, 46, 17));
        let o = CombineOptions { layout: 1, align: 0, gap: 0, ..Default::default() };
        let out = combine_images(&[a, b], &o, None);
        assert_eq!((out.wi(), out.hi()), (30, 30));
        assert_eq!(out.at(0, 25), 0xFF00_FF00);
        assert_eq!(out.at(20, 5), 0xFFFF_FFFF);
    }

    #[test]
    fn diff_marks_changes() {
        let a = Image::sized(4, 4, 0xFFFF_FFFF);
        let mut b = a.clone();
        *b.at_mut(2, 1) = 0xFF00_0000;
        let (d, n, r) = diff_image(&a, &b, 10);
        assert_eq!(n, 1);
        assert_eq!(r, Rect::new(2, 1, 3, 2));
        assert_eq!(d.at(2, 1), 0xFFE5_322D);
    }

    #[test]
    fn date_stamp_tokens() {
        let t = DateTime { year: 2026, month: 3, day: 7, weekday: 6, hour: 15, minute: 4, second: 9 };
        assert_eq!(date_stamp_text("yyyy-MM-dd HH:mm:ss", &t), "2026-03-07 15:04:09");
        assert_eq!(date_stamp_text("d/M/yy h:m tt", &t), "7/3/26 3:4 PM");
        assert_eq!(date_stamp_text("'Hoje:' dd", &t), "Hoje: 07");
    }

    #[test]
    fn background_goes_subject_stays() {
        let mut img = Image::sized(40, 40, 0xFFFF_FFFF);
        img.fill_rect(Rect::new(10, 10, 30, 30), 0xFF20_4080);
        let out = remove_background(&img, 30, 0);
        assert_eq!(ca(out.at(2, 2)), 0);
        assert_eq!(ca(out.at(20, 20)), 255);
    }

    #[test]
    fn split_parts() {
        let img = Image::sized(10, 100, 0xFF00_0000);
        let parts = split_image(&img, &split_cuts(&img, false, true, 0, 4, false), false);
        assert_eq!(parts.iter().map(|p| p.hi()).collect::<Vec<_>>(), vec![25, 25, 25, 25]);
        assert_eq!(split_cuts(&img, false, false, 40, 0, false), vec![40, 80]);
    }

    #[test]
    fn dominant_and_report() {
        let mut img = Image::sized(40, 40, 0xFFFF_FFFF);
        img.fill_rect(Rect::new(0, 0, 40, 8), 0xFF00_0000);
        let (fg, bg) = dominant_colors(&img).unwrap();
        assert_eq!((fg, bg), (0, 0x00FF_FFFF));
        assert!(contrast_report(0, 0x00FF_FFFF).contains("21.00"));
    }
}
