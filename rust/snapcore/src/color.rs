//! Color values as text for the screen color picker, the crosshair and pinned images (C10, Color.cpp): HEX, RGB, HSL
//! and code formats, WCAG luminance and contrast, and the recently picked colors (RecentColors in the registry, shared
//! by both programs). Colors are COLORREFs (0x00BBGGRR).

use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};

use crate::reg;

fn r(c: u32) -> i32 {
    (c & 255) as i32
}
fn g(c: u32) -> i32 {
    (c >> 8 & 255) as i32
}
fn b(c: u32) -> i32 {
    (c >> 16 & 255) as i32
}

pub fn rgb(r: u32, g: u32, b: u32) -> u32 {
    (r & 255) | (g & 255) << 8 | (b & 255) << 16
}

/// The formats, in the order of Color.h's Fmt.
pub const FORMATS: [&str; 8] = [
    "HEX  #RRGGBB",
    "RGB  r, g, b",
    "HSL  hsl(h, s%, l%)",
    "HEX  RRGGBB",
    "CSS  rgb(r, g, b)",
    "C/C++  0xRRGGBB",
    "C#  Color.FromArgb(r, g, b)",
    "Delphi  $00BBGGRR",
];

/// h 0..360, s and l 0..1.
pub fn rgb_to_hsl(c: u32) -> (f64, f64, f64) {
    let (r, g, b) = (r(c) as f64 / 255.0, g(c) as f64 / 255.0, b(c) as f64 / 255.0);
    let mx = r.max(g).max(b);
    let mn = r.min(g).min(b);
    let d = mx - mn;
    let l = (mx + mn) / 2.0;
    if d < 1e-9 {
        return (0.0, 0.0, l);
    }
    let s = if l > 0.5 { d / (2.0 - mx - mn) } else { d / (mx + mn) };
    let h = if mx == r {
        ((g - b) / d + 6.0) % 6.0
    } else if mx == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h * 60.0, s, l)
}

pub fn to_text(c: u32, fmt: i32) -> String {
    let (r, g, b) = (r(c), g(c), b(c));
    match fmt {
        1 => format!("{r}, {g}, {b}"),
        2 => {
            let (h, s, l) = rgb_to_hsl(c);
            format!("hsl({}, {}%, {}%)", (h.round() as i32) % 360, (s * 100.0).round() as i32, (l * 100.0).round() as i32)
        }
        3 => format!("{r:02X}{g:02X}{b:02X}"),
        4 => format!("rgb({r}, {g}, {b})"),
        5 => format!("0x{r:02X}{g:02X}{b:02X}"),
        6 => format!("Color.FromArgb({r}, {g}, {b})"),
        7 => format!("$00{b:02X}{g:02X}{r:02X}"),
        _ => color_to_hex(c),
    }
}

pub fn color_to_hex(c: u32) -> String {
    format!("#{:02X}{:02X}{:02X}", r(c), g(c), b(c))
}

/// "#RRGGBB" (what wcstoul reads after the '#'); `def` when it is shorter than 7 or has no '#'.
pub fn hex_to_color(s: &[u16], def: u32) -> u32 {
    if s.len() < 7 || s[0] != '#' as u16 {
        return def;
    }
    let v = wcstoul16(&s[1..]);
    rgb(v >> 16, v >> 8, v)
}

/// wcstoul(s, nullptr, 16): spaces, a sign, an optional 0x, the hex digits; ULONG_MAX when it overflows.
fn wcstoul16(s: &[u16]) -> u32 {
    let ch = |i: usize| s.get(i).copied().map(|c| char::from_u32(c as u32).unwrap_or('\0')).unwrap_or('\0');
    let mut i = 0;
    while matches!(ch(i), ' ' | '\t' | '\n' | '\r' | '\x0B' | '\x0C') {
        i += 1;
    }
    let neg = ch(i) == '-';
    if matches!(ch(i), '+' | '-') {
        i += 1;
    }
    if ch(i) == '0' && matches!(ch(i + 1), 'x' | 'X') && ch(i + 2).is_ascii_hexdigit() {
        i += 2;
    }
    let mut v: u64 = 0;
    let mut over = false;
    while let Some(d) = ch(i).to_digit(16) {
        v = v * 16 + d as u64;
        if v > u32::MAX as u64 {
            over = true;
            v = u32::MAX as u64;
        }
        i += 1;
    }
    if over {
        return u32::MAX;
    }
    if neg { (v as u32).wrapping_neg() } else { v as u32 }
}

/// WCAG relative luminance (sRGB).
pub fn luminance(c: u32) -> f64 {
    let lin = |v: i32| {
        let x = v as f64 / 255.0;
        if x <= 0.04045 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * lin(r(c)) + 0.7152 * lin(g(c)) + 0.0722 * lin(b(c))
}

/// WCAG 2 contrast ratio, 1..21.
pub fn contrast_ratio(a: u32, b: u32) -> f64 {
    let (mut la, mut lb) = (luminance(a), luminance(b));
    if la < lb {
        std::mem::swap(&mut la, &mut lb);
    }
    (la + 0.05) / (lb + 0.05)
}

/// ColorPickerFormat (HEX by default).
pub fn default_format() -> i32 {
    reg::get_dword(&OsString::from("ColorPickerFormat"), 0, None).clamp(0, FORMATS.len() as u32 - 1) as i32
}

pub fn set_default_format(f: i32) {
    reg::set_dword(&OsString::from("ColorPickerFormat"), f as u32, None);
}

/// The recently picked colors, most recent first (up to 16).
pub fn recent() -> Vec<u32> {
    let s: Vec<u16> = reg::get_string(&OsString::from("RecentColors"), None).map(|v| v.encode_wide().collect()).unwrap_or_default();
    s.split(|&c| c == ';' as u16).filter(|t| !t.is_empty()).map(|t| hex_to_color(t, 0)).collect()
}

pub fn add_recent(c: u32) {
    let mut v = recent();
    v.retain(|&x| x != c);
    v.insert(0, c);
    v.truncate(16);
    let s: String = v.iter().map(|&x| color_to_hex(x) + ";").collect();
    reg::set_string(&OsString::from("RecentColors"), &OsString::from_wide(&s.encode_utf16().collect::<Vec<_>>()), None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        let c = rgb(255, 128, 0);
        let want = ["#FF8000", "255, 128, 0", "hsl(30, 100%, 50%)", "FF8000", "rgb(255, 128, 0)", "0xFF8000",
                    "Color.FromArgb(255, 128, 0)", "$000080FF"];
        for (i, w) in want.iter().enumerate() {
            assert_eq!(to_text(c, i as i32), *w);
        }
        let w: Vec<u16> = "#12AB3F".encode_utf16().collect();
        assert_eq!(hex_to_color(&w, 7), rgb(0x12, 0xAB, 0x3F));
        assert_eq!(hex_to_color(&w[..6], 7), 7);
        assert!((contrast_ratio(0, 0xFFFFFF) - 21.0).abs() < 1e-9);
    }
}
