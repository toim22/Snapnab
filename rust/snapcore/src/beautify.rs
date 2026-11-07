//! Beautify a capture (C05, Beautify.cpp): background (solid color, gradient, desktop wallpaper, image file or
//! transparent), padding, rounded corners, shadow, alignment, aspect-ratio presets and "auto balance" of the margins.
//! The presets and the dialog stay with the interface.

use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;

use windows::Win32::System::Environment::ExpandEnvironmentStringsW;
use windows::Win32::UI::WindowsAndMessaging::{SPI_GETDESKWALLPAPER, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW};
use windows::core::w;

use crate::codec;
use crate::fx::{self, PI_14};
use crate::gdip;
use crate::image::{Image, Rect, argb, ca, cb, cg, cr, from_colorref};
use crate::library::file_exists;

/// BeautifyOptions of Beautify.h; colors are COLORREFs.
#[derive(Clone, Debug)]
pub struct BeautifyOptions {
    pub background: i32, // 0 solid color, 1 gradient, 2 desktop wallpaper, 3 image file, 4 transparent
    pub color1: u32,
    pub color2: u32,
    pub angle: i32,
    pub image_path: Vec<u16>,
    pub padding: i32,
    pub radius: i32,
    pub shadow: i32,
    pub shadow_size: i32,
    pub align: i32,
    pub aspect: i32,
    pub auto_balance: bool,
}

impl Default for BeautifyOptions {
    /// The defaults of Beautify.h.
    fn default() -> Self {
        BeautifyOptions {
            background: 1,
            color1: crate::color::rgb(102, 126, 234),
            color2: crate::color::rgb(118, 75, 162),
            angle: 135,
            image_path: Vec::new(),
            padding: 64,
            radius: 12,
            shadow: 45,
            shadow_size: 24,
            align: 4,
            aspect: 0,
            auto_balance: true,
        }
    }
}

/// The desktop wallpaper file (SPI_GETDESKWALLPAPER, or the copy Windows keeps), or empty.
pub fn wallpaper_path() -> Vec<u16> {
    const MAX_PATH: usize = 260;
    let mut b = vec![0u16; MAX_PATH * 2];
    let ok = unsafe {
        SystemParametersInfoW(SPI_GETDESKWALLPAPER, (MAX_PATH * 2) as u32, Some(b.as_mut_ptr() as _),
                              SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0))
    }
    .is_ok();
    let len = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    if ok && len > 0 && file_exists(&b[..len]) {
        return b[..len].to_vec();
    }
    let mut ad = vec![0u16; MAX_PATH];
    let n = unsafe { ExpandEnvironmentStringsW(w!("%APPDATA%\\Microsoft\\Windows\\Themes\\TranscodedWallpaper"), Some(&mut ad)) };
    let len = ad.iter().position(|&c| c == 0).unwrap_or(0);
    if n > 0 && len > 0 && file_exists(&ad[..len]) { ad[..len].to_vec() } else { Vec::new() }
}

/// Background image scaled to cover the canvas, centered.
fn cover_fit(canvas: &mut Image, bg: &Image) {
    if bg.is_empty() {
        return;
    }
    let k = (canvas.w as f64 / bg.w as f64).max(canvas.h as f64 / bg.h as f64);
    let w = 1.max((bg.w as f64 * k).ceil() as i32);
    let h = 1.max((bg.h as f64 * k).ceil() as i32);
    let s = gdip::scale_image(bg, w, h, true);
    let (cw, ch) = (canvas.wi(), canvas.hi());
    canvas.blit(&s, (cw - w) / 2, (ch - h) / 2, false);
}

/// Distance-based alpha of a rounded rectangle (antialiased edge).
fn round_alpha(x: i32, y: i32, w: i32, h: i32, r: i32) -> i32 {
    if r <= 0 {
        return 255;
    }
    let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
    let rf = r as f32;
    let (mut dx, mut dy) = (0f32, 0f32);
    if cx < rf {
        dx = rf - cx;
    } else if cx > (w - r) as f32 {
        dx = cx - (w - r) as f32;
    }
    if cy < rf {
        dy = rf - cy;
    } else if cy > (h - r) as f32 {
        dy = cy - (h - r) as f32;
    }
    if dx <= 0.0 || dy <= 0.0 {
        return 255;
    }
    let d = (dx * dx + dy * dy).sqrt();
    ((rf - d + 0.5) * 255.0).clamp(0.0, 255.0) as i32
}

/// Auto balance: the plain border around the content is made even (the smallest of the four margins on every side).
fn balance_margins(img: &Image) -> Image {
    let bg = img.at(0, 0);
    let differs = |p: u32| {
        (cr(p) - cr(bg)).abs() > 6 || (cg(p) - cg(bg)).abs() > 6 || (cb(p) - cb(bg)).abs() > 6 || (ca(p) - ca(bg)).abs() > 6
    };
    let (mut x0, mut y0, mut x1, mut y1) = (img.wi(), img.hi(), -1, -1);
    for y in 0..img.hi() {
        for x in 0..img.wi() {
            if differs(img.at(x, y)) {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    if x1 < 0 {
        return img.clone();
    }
    let m = x0.min(y0).min(img.wi() - 1 - x1).min(img.hi() - 1 - y1);
    let mut out = Image::sized(x1 - x0 + 1 + 2 * m, y1 - y0 + 1 + 2 * m, bg);
    out.blit(&img.crop(Rect::new(x0, y0, x1 + 1, y1 + 1)), m, m, false);
    out
}

/// The beautified capture and where the capture ended up in it.
pub fn beautify(src_in: &Image, o: &BeautifyOptions) -> (Image, (i32, i32)) {
    if src_in.is_empty() {
        return (src_in.clone(), (0, 0));
    }
    let mut src = if o.auto_balance { balance_margins(src_in) } else { src_in.clone() };
    if src.is_empty() {
        src = src_in.clone();
    }
    // rounded corners of the capture
    let r = o.radius.min(src.wi() / 2).min(src.hi() / 2);
    if r > 0 {
        let (w, h) = (src.wi(), src.hi());
        for y in 0..h {
            for x in 0..w {
                let a = round_alpha(x, y, w, h, r);
                if a < 255 {
                    let p = src.at_mut(x, y);
                    *p = (*p & 0x00FFFFFF) | (((ca(*p) * a / 255) as u32) << 24);
                }
            }
        }
    }
    // canvas: capture + padding, grown to the aspect ratio
    let pad = 0.max(o.padding);
    let (mut cw, mut ch) = (src.wi() + 2 * pad, src.hi() + 2 * pad);
    const RATIOS: [f64; 5] = [0.0, 16.0 / 9.0, 4.0 / 3.0, 1.0, 9.0 / 16.0];
    if o.aspect > 0 && o.aspect < 5 {
        let want = RATIOS[o.aspect as usize];
        if (cw as f64 / ch as f64) < want {
            cw = (ch as f64 * want).ceil() as i32;
        } else {
            ch = (cw as f64 / want).ceil() as i32;
        }
    }
    let mut canvas = Image::sized(cw, ch, 0);
    let (r1, g1, b1) = ((o.color1 & 255) as i32, (o.color1 >> 8 & 255) as i32, (o.color1 >> 16 & 255) as i32);
    let (r2, g2, b2) = ((o.color2 & 255) as i32, (o.color2 >> 8 & 255) as i32, (o.color2 >> 16 & 255) as i32);
    match o.background {
        0 => canvas = Image::sized(cw, ch, from_colorref(o.color1)),
        1 => {
            // linear gradient along the angle
            let th = o.angle as f64 * PI_14 / 180.0;
            let (cs, sn) = (th.cos(), th.sin());
            let half = (cs.abs() * cw as f64 + sn.abs() * ch as f64) / 2.0;
            for y in 0..ch {
                for x in 0..cw {
                    let t = (((x as f64 - cw as f64 / 2.0) * cs + (y as f64 - ch as f64 / 2.0) * sn) / (2.0 * half) + 0.5)
                        .clamp(0.0, 1.0);
                    let mix = |a: i32, b: i32| (a as f64 + (b - a) as f64 * t).round() as i32;
                    *canvas.at_mut(x, y) = argb(255, mix(r1, r2), mix(g1, g2), mix(b1, b2));
                }
            }
        }
        2 | 3 => {
            canvas = Image::sized(cw, ch, from_colorref(o.color1));
            let path = if o.background == 2 { wallpaper_path() } else { o.image_path.clone() };
            if !path.is_empty() {
                if let Ok(bg) = codec::load(&PathBuf::from(std::ffi::OsString::from_wide(&path))) {
                    cover_fit(&mut canvas, &bg);
                }
            }
        }
        _ => {} // transparent
    }
    // where the capture goes (alignment inside the free space)
    let (free_w, free_h) = (cw - src.wi() - 2 * pad, ch - src.hi() - 2 * pad);
    let (col, row) = (o.align % 3, o.align / 3);
    let (x0, y0) = (pad + free_w * col / 2, pad + free_h * row / 2);
    // shadow: the capture's alpha, blurred, a little below
    if o.shadow > 0 && o.shadow_size > 0 {
        let blur = o.shadow_size;
        let off = 1.max(blur / 3);
        let mut sh = Image::sized(cw, ch, 0);
        for y in 0..src.hi() {
            for x in 0..src.wi() {
                let (tx, ty) = (x + x0, y + y0 + off);
                if tx >= 0 && ty >= 0 && tx < cw && ty < ch {
                    *sh.at_mut(tx, ty) = argb(ca(src.at(x, y)) * o.shadow / 100, 0, 0, 0);
                }
            }
        }
        fx::box_blur(&mut sh, Rect::new(0, 0, cw, ch), 1.max(blur / 2));
        canvas.blit(&sh, 0, 0, true);
    }
    canvas.blit(&src, x0, y0, true);
    canvas.dpi = src_in.dpi;
    canvas.meta = src_in.meta.clone();
    (canvas, (x0, y0))
}
