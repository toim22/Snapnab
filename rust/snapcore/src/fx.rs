//! Effects and filters (Effects.cpp; IImageFilters / CEffectSettings* / CPaintOpRaster, comp_4 + ds_arch §17.B):
//! geometry, color, blur and pixelate, borders, shadows, edges, perspective, page curl, spotlight, emboss, sharpen,
//! watermark, caption and the capture preset effects. A port that keeps the C++ arithmetic (int, float or double as it
//! was), its random sequence (mt19937 read as MSVC's uniform_real_distribution) and its GDI+ calls, so every effect
//! gives the same pixels.

use windows::Win32::Graphics::GdiPlus::*;
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::System::WindowsProgramming::{GetComputerNameW, GetUserNameW};
use windows::core::PWSTR;

use crate::beautify::{self, BeautifyOptions};
use crate::gdip::{self, Bitmap, Brush, Font, Graphics, Pen, StringFormat, pt, rect};
use crate::image::{Image, Rect, argb, ca, cb, cg, cr, from_colorref};

pub const PI_14: f64 = 3.14159265358979;

// ---------------------------------------------------------------- geometry

pub fn rotate90(s: &Image, cw: bool) -> Image {
    let mut d = Image::sized(s.hi(), s.wi(), 0);
    for y in 0..s.hi() {
        for x in 0..s.wi() {
            let v = s.at(x, y);
            if cw {
                *d.at_mut(s.hi() - 1 - y, x) = v;
            } else {
                *d.at_mut(y, s.wi() - 1 - x) = v;
            }
        }
    }
    d
}

pub fn rotate180(s: &Image) -> Image {
    let mut d = Image::sized(s.wi(), s.hi(), 0);
    let n = s.px.len();
    for i in 0..n {
        d.px[n - 1 - i] = s.px[i];
    }
    d
}

pub fn flip_h(s: &Image) -> Image {
    let mut d = Image::sized(s.wi(), s.hi(), 0);
    for y in 0..s.hi() {
        for x in 0..s.wi() {
            *d.at_mut(s.wi() - 1 - x, y) = s.at(x, y);
        }
    }
    d
}

pub fn flip_v(s: &Image) -> Image {
    let mut d = Image::sized(s.wi(), s.hi(), 0);
    let w = s.w as usize;
    for y in 0..s.h as usize {
        let t = s.h as usize - 1 - y;
        d.px[t * w..(t + 1) * w].copy_from_slice(&s.px[y * w..(y + 1) * w]);
    }
    d
}

pub fn rotate_angle(s: &Image, deg: f64, bg: u32) -> Image {
    let rad = deg * PI_14 / 180.0;
    let (c, sn) = (rad.cos().abs(), rad.sin().abs());
    let nw = (s.w as f64 * c + s.h as f64 * sn).ceil() as i32;
    let nh = (s.w as f64 * sn + s.h as f64 * c).ceil() as i32;
    let mut d = Image::sized(nw, nh, bg);
    {
        let sb = Bitmap::wrap_read(s);
        let db = Bitmap::wrap(&mut d);
        let g = Graphics::new(&db);
        g.interpolation(InterpolationModeHighQualityBicubic);
        g.smoothing(SmoothingModeAntiAlias);
        g.translate(nw as f32 / 2.0, nh as f32 / 2.0);
        g.rotate(deg as f32);
        g.draw_image(&sb, -s.wi() as f32 / 2.0, -s.hi() as f32 / 2.0, s.w as f32, s.h as f32);
    }
    d
}

pub fn canvas_resize(s: &Image, nw: i32, nh: i32, anchor: i32, bg: u32) -> Image {
    let mut d = Image::sized(1.max(nw), 1.max(nh), bg);
    let (ax, ay) = (anchor % 3, anchor / 3);
    let ox = if ax == 0 { 0 } else if ax == 1 { (nw - s.wi()) / 2 } else { nw - s.wi() };
    let oy = if ay == 0 { 0 } else if ay == 1 { (nh - s.hi()) / 2 } else { nh - s.hi() };
    d.blit(s, ox, oy, false);
    d
}

fn near(a: u32, b: u32, tol: i32) -> bool {
    (cr(a) - cr(b)).abs() <= tol && (cg(a) - cg(b)).abs() <= tol && (cb(a) - cb(b)).abs() <= tol && (ca(a) - ca(b)).abs() <= tol
}

pub fn auto_trim(s: &Image, tol: i32) -> Image {
    if s.is_empty() {
        return s.clone();
    }
    let rf = s.at(0, 0);
    let row_uniform = |y: i32| (0..s.wi()).all(|x| near(s.at(x, y), rf, tol));
    let col_uniform = |x: i32, y0: i32, y1: i32| (y0..y1).all(|y| near(s.at(x, y), rf, tol));
    let (mut top, mut bottom, mut left, mut right) = (0, s.hi(), 0, s.wi());
    while top < bottom - 1 && row_uniform(top) {
        top += 1;
    }
    while bottom - 1 > top && row_uniform(bottom - 1) {
        bottom -= 1;
    }
    while left < right - 1 && col_uniform(left, top, bottom) {
        left += 1;
    }
    while right - 1 > left && col_uniform(right - 1, top, bottom) {
        right -= 1;
    }
    s.crop(Rect::new(left, top, right, bottom))
}

pub fn scale(s: &Image, percent: i32) -> Image {
    if percent == 100 || percent <= 0 {
        return s.clone();
    }
    gdip::scale_image(s, 1.max(s.wi() * percent / 100), 1.max(s.hi() * percent / 100), true)
}

// ---------------------------------------------------------------- color

pub fn grayscale(img: &mut Image) {
    for p in img.px.iter_mut() {
        let l = (cr(*p) * 299 + cg(*p) * 587 + cb(*p) * 114) / 1000;
        *p = argb(ca(*p), l, l, l);
    }
}

pub fn invert(img: &mut Image) {
    for p in img.px.iter_mut() {
        *p = (*p & 0xFF000000) | (!*p & 0x00FFFFFF);
    }
}

pub fn sepia(img: &mut Image) {
    for p in img.px.iter_mut() {
        let (r, g, b) = (cr(*p), cg(*p), cb(*p));
        let nr = 255.min((r * 393 + g * 769 + b * 189) / 1000);
        let ng = 255.min((r * 349 + g * 686 + b * 168) / 1000);
        let nb = 255.min((r * 272 + g * 534 + b * 131) / 1000);
        *p = argb(ca(*p), nr, ng, nb);
    }
}

pub fn monochrome(img: &mut Image, threshold: i32) {
    for p in img.px.iter_mut() {
        let l = (cr(*p) * 299 + cg(*p) * 587 + cb(*p) * 114) / 1000;
        let v = if l >= threshold { 255 } else { 0 };
        *p = argb(ca(*p), v, v, v);
    }
}

/// std::lround: halves away from zero.
fn lround(v: f64) -> i64 {
    v.round() as i64
}

fn clamp255(v: i64) -> i32 {
    v.clamp(0, 255) as i32
}

pub fn brightness_contrast(img: &mut Image, b: i32, c: i32) {
    let mut cf = (100.0 + c as f64) / 100.0;
    cf *= cf;
    let mut lut = [0i32; 256];
    for (i, l) in lut.iter_mut().enumerate() {
        let mut v = i as f64 + b as f64 * 2.55;
        v = ((v / 255.0 - 0.5) * cf + 0.5) * 255.0;
        *l = clamp255(lround(v));
    }
    for p in img.px.iter_mut() {
        *p = argb(ca(*p), lut[cr(*p) as usize], lut[cg(*p) as usize], lut[cb(*p) as usize]);
    }
}

fn rgb_to_hsl(r: i32, g: i32, b: i32) -> (f64, f64, f64) {
    let (rf, gf, bf) = (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
    let mx = rf.max(gf).max(bf);
    let mn = rf.min(gf).min(bf);
    let l = (mx + mn) / 2.0;
    let (mut h, mut s) = (0.0, 0.0);
    if mx != mn {
        let d = mx - mn;
        s = if l > 0.5 { d / (2.0 - mx - mn) } else { d / (mx + mn) };
        h = if mx == rf {
            (gf - bf) / d + if gf < bf { 6.0 } else { 0.0 }
        } else if mx == gf {
            (bf - rf) / d + 2.0
        } else {
            (rf - gf) / d + 4.0
        };
        h /= 6.0;
    }
    (h, s, l)
}

fn hue2rgb(p: f64, q: f64, mut t: f64) -> f64 {
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        return p + (q - p) * 6.0 * t;
    }
    if t < 0.5 {
        return q;
    }
    if t < 2.0 / 3.0 {
        return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
    }
    p
}

pub fn hue_saturation(img: &mut Image, hue_shift: i32, sat: i32) {
    for p in img.px.iter_mut() {
        let (mut h, mut s, l) = rgb_to_hsl(cr(*p), cg(*p), cb(*p));
        h = (h + hue_shift as f64 / 360.0 + 1.0) % 1.0;
        s = (s * (1.0 + sat as f64 / 100.0)).clamp(0.0, 1.0);
        let (r, g, b);
        if s == 0.0 {
            (r, g, b) = (l, l, l);
        } else {
            let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
            let pp = 2.0 * l - q;
            r = hue2rgb(pp, q, h + 1.0 / 3.0);
            g = hue2rgb(pp, q, h);
            b = hue2rgb(pp, q, h - 1.0 / 3.0);
        }
        *p = argb(ca(*p), (r * 255.0 + 0.5) as i32, (g * 255.0 + 0.5) as i32, (b * 255.0 + 0.5) as i32);
    }
}

pub fn gamma(img: &mut Image, gamma: f64) {
    let mut lut = [0i32; 256];
    for (i, l) in lut.iter_mut().enumerate() {
        *l = clamp255(lround((i as f64 / 255.0).powf(1.0 / gamma) * 255.0));
    }
    for p in img.px.iter_mut() {
        *p = argb(ca(*p), lut[cr(*p) as usize], lut[cg(*p) as usize], lut[cb(*p) as usize]);
    }
}

pub fn replace_color(img: &mut Image, from: u32, to: u32, tol: i32) {
    for p in img.px.iter_mut() {
        if near(*p | 0xFF000000, from | 0xFF000000, tol) {
            *p = to;
        }
    }
}

// ---------------------------------------------------------------- hide

fn clip_to(img: &Image, r: Rect) -> Rect {
    Rect::new(r.left.max(0), r.top.max(0), r.right.min(img.wi()), r.bottom.min(img.hi()))
}

/// Three passes of a box blur on premultiplied channels (single precision, as before).
pub fn box_blur(img: &mut Image, r: Rect, radius: i32) {
    let r = clip_to(img, r);
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    if w <= 1 || h <= 1 || radius < 1 {
        return;
    }
    let (wu, n) = (w as usize, w as usize * h as usize);
    let mut ch: [Vec<f32>; 4] = [vec![0.0; n], vec![0.0; n], vec![0.0; n], vec![0.0; n]];
    for y in 0..h {
        for x in 0..w {
            let p = img.at(r.left + x, r.top + y);
            let a = ca(p) as f32 / 255.0;
            let i = y as usize * wu + x as usize;
            ch[0][i] = cr(p) as f32 * a;
            ch[1][i] = cg(p) as f32 * a;
            ch[2][i] = cb(p) as f32 * a;
            ch[3][i] = ca(p) as f32;
        }
    }
    let mut tmp = vec![0f32; w.max(h) as usize];
    for _pass in 0..3 {
        for a in ch.iter_mut() {
            for y in 0..h as usize {
                let rowp = &mut a[y * wu..(y + 1) * wu];
                let (mut sum, mut cnt) = (0f32, 0i32);
                for x in -radius..=radius {
                    sum += rowp[x.clamp(0, w - 1) as usize];
                    cnt += 1;
                }
                for x in 0..w {
                    tmp[x as usize] = sum / cnt as f32;
                    sum += rowp[(w - 1).min(x + radius + 1) as usize] - rowp[0.max(x - radius) as usize];
                }
                rowp.copy_from_slice(&tmp[..wu]);
            }
            for x in 0..w as usize {
                let (mut sum, mut cnt) = (0f32, 0i32);
                for y in -radius..=radius {
                    sum += a[y.clamp(0, h - 1) as usize * wu + x];
                    cnt += 1;
                }
                for y in 0..h {
                    tmp[y as usize] = sum / cnt as f32;
                    sum += a[(h - 1).min(y + radius + 1) as usize * wu + x] - a[0.max(y - radius) as usize * wu + x];
                }
                for y in 0..h as usize {
                    a[y * wu + x] = tmp[y];
                }
            }
        }
    }
    for y in 0..h {
        for x in 0..w {
            let i = y as usize * wu + x as usize;
            let al = ch[3][i];
            let inv = if al > 0.5 { 255.0f32 / al } else { 0.0 };
            let c = |v: f32| ((v * inv + 0.5) as i32).clamp(0, 255);
            *img.at_mut(r.left + x, r.top + y) = argb((al + 0.5) as i32, c(ch[0][i]), c(ch[1][i]), c(ch[2][i]));
        }
    }
}

pub fn pixelate(img: &mut Image, r: Rect, block: i32) {
    let r = clip_to(img, r);
    let block = 2.max(block);
    let mut by = r.top;
    while by < r.bottom {
        let mut bx = r.left;
        while bx < r.right {
            let (ex, ey) = ((bx + block).min(r.right), (by + block).min(r.bottom));
            let (mut sr, mut sg, mut sb, mut sa, mut n) = (0i64, 0i64, 0i64, 0i64, 0i64);
            for y in by..ey {
                for x in bx..ex {
                    let p = img.at(x, y);
                    sr += cr(p) as i64;
                    sg += cg(p) as i64;
                    sb += cb(p) as i64;
                    sa += ca(p) as i64;
                    n += 1;
                }
            }
            if n != 0 {
                let v = argb((sa / n) as i32, (sr / n) as i32, (sg / n) as i32, (sb / n) as i32);
                for y in by..ey {
                    for x in bx..ex {
                        *img.at_mut(x, y) = v;
                    }
                }
            }
            bx += block;
        }
        by += block;
    }
}

// ---------------------------------------------------------------- border and finish

pub fn border(s: &Image, width: i32, color: u32) -> Image {
    let mut d = Image::sized(s.wi() + 2 * width, s.hi() + 2 * width, color);
    d.blit(s, width, width, false);
    d
}

pub fn shadow(s: &Image, offset: i32, blur: i32, opacity: i32) -> Image {
    let pad = blur * 2;
    let mut d = Image::sized(s.wi() + offset + pad * 2, s.hi() + offset + pad * 2, 0);
    let mut sh = Image::sized(d.wi(), d.hi(), 0);
    for y in 0..s.hi() {
        for x in 0..s.wi() {
            let a = ca(s.at(x, y)) * opacity / 100;
            *sh.at_mut(x + pad + offset, y + pad + offset) = argb(a, 0, 0, 0);
        }
    }
    let (sw, shh) = (sh.wi(), sh.hi());
    box_blur(&mut sh, Rect::new(0, 0, sw, shh), 1.max(blur));
    d.blit(&sh, 0, 0, false);
    d.blit(s, pad, pad, true);
    d
}

/// std::mt19937 (the standard 32-bit Mersenne Twister).
pub struct Mt19937 {
    mt: [u32; 624],
    i: usize,
}

impl Mt19937 {
    pub fn new(seed: u32) -> Self {
        let mut mt = [0u32; 624];
        mt[0] = seed;
        for i in 1..624 {
            mt[i] = 1812433253u32.wrapping_mul(mt[i - 1] ^ (mt[i - 1] >> 30)).wrapping_add(i as u32);
        }
        Mt19937 { mt, i: 624 }
    }

    pub fn next_u32(&mut self) -> u32 {
        if self.i >= 624 {
            for k in 0..624 {
                let y = (self.mt[k] & 0x80000000) | (self.mt[(k + 1) % 624] & 0x7FFFFFFF);
                let mut v = self.mt[(k + 397) % 624] ^ (y >> 1);
                if y & 1 != 0 {
                    v ^= 0x9908B0DF;
                }
                self.mt[k] = v;
            }
            self.i = 0;
        }
        let mut y = self.mt[self.i];
        self.i += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9D2C5680;
        y ^= (y << 15) & 0xEFC60000;
        y ^ (y >> 18)
    }

    /// generate_canonical<double, 53> of MSVC: two draws, 53 bits, times 2^-53.
    pub fn canonical(&mut self) -> f64 {
        let mut sx = (self.next_u32() >> 11) as u64;
        sx += (self.next_u32() as u64) << 21;
        sx as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// uniform_real_distribution<double>(a, b)
    pub fn uniform(&mut self, a: f64, b: f64) -> f64 {
        self.canonical() * (b - a) + a
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum Edge {
    Torn = 0,
    Wave,
    Sawtooth,
    SharkTooth,
    Fade,
    Beveled,
}

impl Edge {
    pub fn from_i32(v: i32) -> Edge {
        match v {
            0 => Edge::Torn,
            1 => Edge::Wave,
            2 => Edge::Sawtooth,
            3 => Edge::SharkTooth,
            4 => Edge::Fade,
            _ => Edge::Beveled,
        }
    }
}

pub const TOP: i32 = 1;
pub const RIGHT: i32 = 2;
pub const BOTTOM: i32 = 4;
pub const LEFT: i32 = 8;

fn edge_depth(st: Edge, i: i32, size: i32, rng: &mut Mt19937, walk: &mut f64) -> f64 {
    let sz = size as f64;
    match st {
        Edge::Torn => {
            let u = rng.uniform(-0.35, 0.35);
            *walk = (*walk + u * sz * 0.5).clamp(sz * 0.15, sz);
            *walk
        }
        Edge::Wave => sz * 0.5 * (1.0 + ((i * 2) as f64 * 3.14159265 / (sz * 3.0)).sin()),
        Edge::Sawtooth => {
            let per = sz * 1.6;
            let t = (i as f64 % per) / per;
            sz * if t < 0.5 { t * 2.0 } else { (1.0 - t) * 2.0 }
        }
        Edge::SharkTooth => {
            let per = sz * 1.8;
            let t = (i as f64 % per) / per;
            sz * (1.0 - t)
        }
        _ => 0.0,
    }
}

pub fn edge_effect(s: &Image, style: Edge, size: i32, sides: i32, with_shadow: bool) -> Image {
    let mut d = s.clone();
    let size = size.clamp(2, 2.max(s.wi().min(s.hi()) / 3));
    let mut rng = Mt19937::new(1234);
    let (w, h) = (d.wi(), d.hi());
    if style == Edge::Beveled {
        // 3D bevel: light band at the top/left and dark band at the bottom/right, fading inward
        for y in 0..h {
            for x in 0..w {
                let (mut li, mut dk) = (0f64, 0f64);
                if sides & TOP != 0 && y < size {
                    li = li.max(1.0 - y as f64 / size as f64);
                }
                if sides & LEFT != 0 && x < size {
                    li = li.max(1.0 - x as f64 / size as f64);
                }
                if sides & BOTTOM != 0 && h - 1 - y < size {
                    dk = dk.max(1.0 - (h - 1 - y) as f64 / size as f64);
                }
                if sides & RIGHT != 0 && w - 1 - x < size {
                    dk = dk.max(1.0 - (w - 1 - x) as f64 / size as f64);
                }
                if li <= 0.0 && dk <= 0.0 {
                    continue;
                }
                let p = d.at_mut(x, y);
                let c = |c: i32| clamp255(lround(c as f64 + (255 - c) as f64 * 0.55 * li - c as f64 * 0.5 * dk));
                *p = argb(ca(*p), c(cr(*p)), c(cg(*p)), c(cb(*p)));
            }
        }
    } else if style == Edge::Fade {
        for y in 0..h {
            for x in 0..w {
                let mut f = 1.0f64;
                if sides & LEFT != 0 {
                    f = f.min(x as f64 / size as f64);
                }
                if sides & RIGHT != 0 {
                    f = f.min((w - 1 - x) as f64 / size as f64);
                }
                if sides & TOP != 0 {
                    f = f.min(y as f64 / size as f64);
                }
                if sides & BOTTOM != 0 {
                    f = f.min((h - 1 - y) as f64 / size as f64);
                }
                let f = f.clamp(0.0, 1.0);
                let p = d.at_mut(x, y);
                *p = (*p & 0x00FFFFFF) | (((ca(*p) as f64 * f) as u32) << 24);
            }
        }
    } else {
        let mut apply = |d: &mut Image, len: i32, set: &dyn Fn(&mut Image, i32, f64)| {
            let mut walk = size as f64 * 0.5;
            for i in 0..len {
                let dep = edge_depth(style, i, size, &mut rng, &mut walk);
                set(d, i, dep);
            }
        };
        let clear_h = |d: &mut Image, x: i32, y0: i32, y1: i32| {
            for y in y0.max(0)..y1.min(d.hi()) {
                *d.at_mut(x, y) = 0;
            }
        };
        let clear_v = |d: &mut Image, y: i32, x0: i32, x1: i32| {
            for x in x0.max(0)..x1.min(d.wi()) {
                *d.at_mut(x, y) = 0;
            }
        };
        if sides & TOP != 0 {
            apply(&mut d, w, &|d, i, dep| clear_h(d, i, 0, dep as i32));
        }
        if sides & BOTTOM != 0 {
            apply(&mut d, w, &|d, i, dep| clear_h(d, i, h - dep as i32, h));
        }
        if sides & LEFT != 0 {
            apply(&mut d, h, &|d, i, dep| clear_v(d, i, 0, dep as i32));
        }
        if sides & RIGHT != 0 {
            apply(&mut d, h, &|d, i, dep| clear_v(d, i, w - dep as i32, w));
        }
    }
    if with_shadow { shadow(&d, 5, 5, 45) } else { d }
}

/// Bilinear sample (outside the image = transparent).
fn sample(s: &Image, x: f64, y: f64) -> u32 {
    if x < -0.5 || y < -0.5 || x > s.w as f64 - 0.5 || y > s.h as f64 - 0.5 {
        return 0;
    }
    let (x0, y0) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - x0 as f64, y - y0 as f64);
    let at = |xx: i32, yy: i32| s.at(xx.clamp(0, s.wi() - 1), yy.clamp(0, s.hi() - 1));
    let (p00, p10, p01, p11) = (at(x0, y0), at(x0 + 1, y0), at(x0, y0 + 1), at(x0 + 1, y0 + 1));
    let mix = |sh: u32| {
        let a = ((p00 >> sh) & 255) as f64 * (1.0 - fx) + ((p10 >> sh) & 255) as f64 * fx;
        let b = ((p01 >> sh) & 255) as f64 * (1.0 - fx) + ((p11 >> sh) & 255) as f64 * fx;
        (clamp255(lround(a * (1.0 - fy) + b * fy)) as u32) << sh
    };
    mix(24) | mix(16) | mix(8) | mix(0)
}

pub fn perspective(s: &Image, horizontal: i32, vertical: i32, shear: i32) -> Image {
    let mut d = Image::sized(s.wi(), s.hi(), 0);
    let hz = horizontal.clamp(-100, 100) as f64 / 100.0 * 0.6;
    let vt = vertical.clamp(-100, 100) as f64 / 100.0 * 0.6;
    let sh = shear.clamp(-100, 100) as f64 / 100.0 * 0.5;
    let (cx, cy) = (s.w as f64 / 2.0, s.h as f64 / 2.0);
    for y in 0..d.hi() {
        for x in 0..d.wi() {
            let u = if s.w > 1 { x as f64 / (s.wi() - 1) as f64 } else { 0.0 };
            let v = if s.h > 1 { y as f64 / (s.hi() - 1) as f64 } else { 0.0 };
            let sy = 1.0 - hz.abs() * if hz > 0.0 { u } else { 1.0 - u };
            let sx = 1.0 - vt.abs() * if vt > 0.0 { v } else { 1.0 - v };
            let xs = (x as f64 - cx) / sx + cx - sh * (y as f64 - cy);
            let ys = (y as f64 - cy) / sy + cy;
            *d.at_mut(x, y) = sample(s, xs, ys);
        }
    }
    d
}

pub fn page_curl(s_in: &Image, size: i32, corner: i32) -> Image {
    // always works on the bottom-right corner; the other corners are mirrored before and after
    let fh = corner == 0 || corner == 3;
    let fv = corner == 0 || corner == 1;
    let mut s = s_in.clone();
    if fh {
        s = flip_h(&s);
    }
    if fv {
        s = flip_v(&s);
    }
    let size = size.clamp(8, 8.max(s.wi().min(s.hi()) - 4));
    let (w, h) = (s.wi(), s.hi());
    for y in 0.max(h - size)..h {
        for x in 0.max(w - size)..w {
            if x + y > w + h - size {
                *s.at_mut(x, y) = 0;
            }
        }
    }
    {
        let bmp = Bitmap::wrap(&mut s);
        let g = Graphics::new(&bmp);
        g.smoothing(SmoothingModeAntiAlias);
        let sz = size as f32;
        let a = pt((w - size) as f32, h as f32);
        let b = pt(w as f32, (h - size) as f32);
        let tip = pt((w - size) as f32, (h - size) as f32);
        let shadow_tri = [a, b, pt(tip.X - sz * 0.08, tip.Y - sz * 0.08)];
        let shb = Brush::solid(argb(70, 0, 0, 0));
        g.fill_polygon(&shb, &shadow_tri);
        let flap = [a, b, tip];
        let mid = pt((a.X + b.X) / 2.0, (a.Y + b.Y) / 2.0);
        let lg = Brush::linear(mid, tip, argb(255, 196, 198, 202), argb(255, 250, 250, 251));
        g.fill_polygon(&lg, &flap);
        let edge = Pen::new(argb(120, 120, 122, 128), 1.0);
        g.draw_polygon(&edge, &flap);
    }
    if fv {
        s = flip_v(&s);
    }
    if fh {
        s = flip_h(&s);
    }
    s
}

pub fn spotlight(s: &Image, f: Rect, dim_percent: i32, blur: i32, zoom_percent: i32, with_shadow: bool) -> Image {
    let f = clip_to(s, f);
    if f.right <= f.left || f.bottom <= f.top {
        return s.clone();
    }
    let mut d = s.clone();
    if blur > 0 {
        let (w, h) = (d.wi(), d.hi());
        box_blur(&mut d, Rect::new(0, 0, w, h), blur);
    }
    let dim = dim_percent.clamp(0, 95);
    for p in d.px.iter_mut() {
        *p = argb(ca(*p), cr(*p) * (100 - dim) / 100, cg(*p) * (100 - dim) / 100, cb(*p) * (100 - dim) / 100);
    }
    let mut inner = s.crop(f);
    let (mut x, mut y) = (f.left, f.top);
    if zoom_percent > 100 {
        let (nw, nh) = (inner.wi() * zoom_percent / 100, inner.hi() * zoom_percent / 100);
        inner = gdip::scale_image(&inner, nw, nh, true);
        x = (f.left + f.right) / 2 - nw / 2;
        y = (f.top + f.bottom) / 2 - nh / 2;
    }
    if with_shadow {
        let mut sh = Image::sized(inner.wi() + 24, inner.hi() + 24, 0);
        for yy in 0..inner.hi() {
            for xx in 0..inner.wi() {
                *sh.at_mut(xx + 12, yy + 12) = argb(110, 0, 0, 0);
            }
        }
        let (w, h) = (sh.wi(), sh.hi());
        box_blur(&mut sh, Rect::new(0, 0, w, h), 6);
        d.blit(&sh, x - 12 + 4, y - 12 + 5, true);
    }
    d.blit(&inner, x, y, true);
    d
}

pub fn emboss(img: &mut Image, depth: i32) {
    let s = img.clone();
    let k = depth.clamp(1, 10) as f64 / 2.0;
    let lum = |x: i32, y: i32| {
        let p = s.at(x.clamp(0, s.wi() - 1), y.clamp(0, s.hi() - 1));
        0.299 * cr(p) as f64 + 0.587 * cg(p) as f64 + 0.114 * cb(p) as f64
    };
    for y in 0..img.hi() {
        for x in 0..img.wi() {
            let v = clamp255(lround(128.0 + (lum(x + 1, y + 1) - lum(x - 1, y - 1)) * k));
            *img.at_mut(x, y) = argb(ca(s.at(x, y)), v, v, v);
        }
    }
}

pub fn sharpen(img: &mut Image, amount: i32) {
    let s = img.clone();
    let k = amount.clamp(0, 100) as f64 / 100.0;
    let at = |x: i32, y: i32| s.at(x.clamp(0, s.wi() - 1), y.clamp(0, s.hi() - 1));
    for y in 0..img.hi() {
        for x in 0..img.wi() {
            let (c, l, r, u, dn) = (at(x, y), at(x - 1, y), at(x + 1, y), at(x, y - 1), at(x, y + 1));
            let ch = |sh: u32| {
                let cc = ((c >> sh) & 255) as i32;
                let nb = ((l >> sh) & 255) as i32 + ((r >> sh) & 255) as i32 + ((u >> sh) & 255) as i32 + ((dn >> sh) & 255) as i32;
                clamp255(lround(cc as f64 * (1.0 + 4.0 * k) - nb as f64 * k))
            };
            *img.at_mut(x, y) = argb(ca(c), ch(16), ch(8), ch(0));
        }
    }
}

// ---------------------------------------------------------------- text

fn centered() -> StringFormat {
    let sf = StringFormat::new();
    sf.align(StringAlignmentCenter);
    sf.line_align(StringAlignmentCenter);
    sf
}

pub fn watermark(img: &mut Image, text: &[u16], opacity: i32, tiled: bool, emboss: bool) {
    if text.is_empty() || img.is_empty() {
        return;
    }
    let (w, h) = (img.wi(), img.hi());
    let bmp = Bitmap::wrap(img);
    let g = Graphics::new(&bmp);
    g.text_hint(TextRenderingHintAntiAlias);
    g.smoothing(SmoothingModeAntiAlias);
    let fs = 12.max(w.min(h) / if tiled { 12 } else { 6 }) as f32;
    let font = Font::new("Segoe UI", fs, FontStyleBold);
    let a = (opacity.clamp(0, 100) * 255 / 100) as u8 as i32;
    let br = Brush::solid(argb(a, 255, 255, 255));
    let dark = Brush::solid(argb(a, 0, 0, 0));
    let sf = centered();
    let draw = |x: f32, y: f32| {
        let rc = rect(x - 1000.0, y - fs, 2000.0, fs * 2.0);
        let off = if emboss { 1.5f32 } else { 1.0 };
        g.draw_string(text, &font, rect(rc.X + off, rc.Y + off, rc.Width, rc.Height), &sf, &dark);
        g.draw_string(text, &font, rc, &sf, &br);
    };
    if tiled {
        g.translate(w as f32 / 2.0, h as f32 / 2.0);
        g.rotate(-30.0);
        let step = fs * 6.0;
        let diag = (w as f64).hypot(h as f64) as f32;
        let mut y = -diag;
        while y < diag {
            let mut x = -diag;
            while x < diag {
                draw(x + if (y / (fs * 3.0)) % 2.0f32 != 0.0 { step / 2.0 } else { 0.0 }, y);
                x += step;
            }
            y += fs * 3.0;
        }
    } else {
        draw(w as f32 / 2.0, h as f32 / 2.0);
    }
}

fn computer_name() -> Vec<u16> {
    let mut b = [0u16; 16]; // MAX_COMPUTERNAME_LENGTH + 1
    let mut n = b.len() as u32;
    if unsafe { GetComputerNameW(Some(PWSTR(b.as_mut_ptr())), &mut n) }.is_ok() {
        b[..n as usize].to_vec()
    } else {
        Vec::new()
    }
}

fn user_name() -> Vec<u16> {
    let mut b = [0u16; 257];
    let mut n = b.len() as u32;
    if unsafe { GetUserNameW(Some(PWSTR(b.as_mut_ptr())), &mut n) }.is_ok() && n > 0 {
        b[..n as usize - 1].to_vec()
    } else {
        Vec::new()
    }
}

/// {computer} {user} {date} {time}
pub fn expand_caption_tokens(text: &[u16]) -> Vec<u16> {
    let st = unsafe { GetLocalTime() };
    let w = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
    let mut r = text.to_vec();
    let mut rep = |k: &str, v: Vec<u16>| {
        let k = w(k);
        while let Some(p) = find(&r, &k) {
            r.splice(p..p + k.len(), v.iter().copied());
        }
    };
    rep("{computer}", computer_name());
    rep("{user}", user_name());
    rep("{date}", w(&format!("{:02}/{:02}/{:04}", st.wDay, st.wMonth, st.wYear)));
    rep("{time}", w(&format!("{:02}:{:02}:{:02}", st.wHour, st.wMinute, st.wSecond)));
    r
}

fn find(hay: &[u16], needle: &[u16]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).find(|&i| &hay[i..i + needle.len()] == needle)
}

pub fn caption(s: &Image, text_in: &[u16], top: bool, font_px: i32, fg: u32, bg: u32) -> Image {
    let text = expand_caption_tokens(text_in);
    let bar = font_px * 2;
    let mut d = Image::sized(s.wi(), s.hi() + bar, bg);
    d.blit(s, 0, if top { bar } else { 0 }, false);
    {
        let bmp = Bitmap::wrap(&mut d);
        let g = Graphics::new(&bmp);
        g.text_hint(TextRenderingHintAntiAlias);
        let font = Font::new("Segoe UI", font_px as f32, FontStyleRegular);
        let br = Brush::solid(fg);
        let sf = centered();
        sf.trimming(StringTrimmingEllipsisCharacter);
        let rc = rect(0.0, if top { 0.0 } else { s.h as f32 }, s.w as f32, bar as f32);
        g.draw_string(&text, &font, rc, &sf, &br);
    }
    d
}

// ---------------------------------------------------------------- capture preset effects (CaptureEffects, comp_7 §2.1)

/// CaptureEffects of Effects.h; colors are COLORREFs.
#[derive(Clone, Debug)]
pub struct CaptureEffects {
    pub border: bool,
    pub border_width: i32,
    pub border_color: u32,
    pub filter: bool,
    pub filter_kind: i32,
    pub shadow: bool,
    pub shadow_offset: i32,
    pub shadow_blur: i32,
    pub shadow_opacity: i32,
    pub caption: bool,
    pub caption_text: Vec<u16>,
    pub caption_top: bool,
    pub replace_color: bool,
    pub replace_from: u32,
    pub replace_to: u32,
    pub replace_tolerance: i32,
    pub scale: bool,
    pub scale_percent: i32,
    pub edge: bool,
    pub edge_style: i32,
    pub edge_size: i32,
    pub edge_sides: i32,
    pub trim: bool,
    pub trim_tolerance: i32,
    pub adjust: bool,
    pub brightness: i32,
    pub contrast: i32,
    pub hue: i32,
    pub saturation: i32,
    pub rotate: bool,
    pub rotate_kind: i32,
    pub watermark: bool,
    pub watermark_text: Vec<u16>,
    pub watermark_opacity: i32,
    pub watermark_tiled: bool,
    pub resolution: bool,
    pub dpi: i32,
    pub beautify: bool,
    pub beautify_opt: BeautifyOptions,
}

impl CaptureEffects {
    /// Any effect on (CaptureEffects::Any).
    pub fn any(&self) -> bool {
        self.border || self.filter || self.shadow || self.caption || self.replace_color || self.scale || self.edge
            || self.trim || self.adjust || self.rotate || self.watermark || self.resolution || self.beautify
    }
}

impl Default for CaptureEffects {
    /// The defaults of Effects.h.
    fn default() -> Self {
        let rgb = crate::color::rgb;
        CaptureEffects {
            border: false,
            border_width: 4,
            border_color: rgb(40, 40, 40),
            filter: false,
            filter_kind: 0,
            shadow: false,
            shadow_offset: 6,
            shadow_blur: 6,
            shadow_opacity: 50,
            caption: false,
            caption_text: "{date} {time} — {computer}".encode_utf16().collect(),
            caption_top: false,
            replace_color: false,
            replace_from: rgb(255, 255, 255),
            replace_to: rgb(255, 255, 0),
            replace_tolerance: 10,
            scale: false,
            scale_percent: 100,
            edge: false,
            edge_style: 0,
            edge_size: 14,
            edge_sides: 6,
            trim: false,
            trim_tolerance: 8,
            adjust: false,
            brightness: 0,
            contrast: 0,
            hue: 0,
            saturation: 0,
            rotate: false,
            rotate_kind: 0,
            watermark: false,
            watermark_text: "Snapnab".encode_utf16().collect(),
            watermark_opacity: 35,
            watermark_tiled: true,
            resolution: false,
            dpi: 300,
            beautify: false,
            beautify_opt: BeautifyOptions::default(),
        }
    }
}

/// Geometry first, then color, text and finish (border, edge effects and shadow last), Beautify at the end.
pub fn apply_capture_effects(s: &Image, e: &CaptureEffects) -> Image {
    let mut img = if e.trim { auto_trim(s, e.trim_tolerance) } else { s.clone() };
    if e.rotate {
        img = match e.rotate_kind {
            0 => rotate90(&img, true),
            1 => rotate90(&img, false),
            2 => rotate180(&img),
            3 => flip_h(&img),
            _ => flip_v(&img),
        };
    }
    if e.scale && e.scale_percent != 100 {
        img = scale(&img, e.scale_percent);
    }
    if e.adjust {
        if e.brightness != 0 || e.contrast != 0 {
            brightness_contrast(&mut img, e.brightness, e.contrast);
        }
        if e.hue != 0 || e.saturation != 0 {
            hue_saturation(&mut img, e.hue, e.saturation);
        }
    }
    if e.filter {
        match e.filter_kind {
            0 => grayscale(&mut img),
            1 => sepia(&mut img),
            2 => invert(&mut img),
            _ => monochrome(&mut img, 128),
        }
    }
    if e.replace_color {
        replace_color(&mut img, from_colorref(e.replace_from), from_colorref(e.replace_to), e.replace_tolerance);
    }
    if e.watermark {
        watermark(&mut img, &e.watermark_text, e.watermark_opacity, e.watermark_tiled, false);
    }
    if e.caption {
        img = caption(&img, &expand_caption_tokens(&e.caption_text), e.caption_top, 14, 0xFFFFFFFF, 0xFF202020);
    }
    if e.border {
        img = border(&img, e.border_width, from_colorref(e.border_color));
    }
    if e.edge {
        let sides = if e.edge_sides != 0 { e.edge_sides } else { BOTTOM | RIGHT };
        img = edge_effect(&img, Edge::from_i32(e.edge_style), e.edge_size, sides, false);
    }
    if e.shadow {
        img = shadow(&img, e.shadow_offset, e.shadow_blur, e.shadow_opacity);
    }
    if e.beautify {
        img = beautify::beautify(&img, &e.beautify_opt).0;
    }
    if e.resolution {
        img.dpi = e.dpi as f32;
    }
    img
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mersenne_twister() {
        // the 10000th output of a default-seeded mt19937 is 4123659995 (C++ standard, [rand.predef])
        let mut r = Mt19937::new(5489);
        let mut v = 0;
        for _ in 0..10000 {
            v = r.next_u32();
        }
        assert_eq!(v, 4123659995);
        let u = Mt19937::new(1).uniform(-0.35, 0.35);
        assert!((-0.35..0.35).contains(&u));
    }

    #[test]
    fn geometry_and_color() {
        let mut img = Image::new(3, 2, 0);
        img.px = vec![1, 2, 3, 4, 5, 6];
        assert_eq!(rotate90(&img, true).px, vec![4, 1, 5, 2, 6, 3]);
        assert_eq!(rotate180(&img).px, vec![6, 5, 4, 3, 2, 1]);
        assert_eq!(flip_h(&img).px, vec![3, 2, 1, 6, 5, 4]);
        assert_eq!(flip_v(&img).px, vec![4, 5, 6, 1, 2, 3]);
        let mut c = Image::new(1, 1, 0x80102030);
        invert(&mut c);
        assert_eq!(c.px[0], 0x80EFDFCF);
        let r = rotate_angle(&Image::new(10, 4, 0xFFFF0000), 90.0, 0);
        assert_eq!((r.w, r.h), (5, 11)); // as the C++ did: ceil of a 90° turn with a 14-digit pi is one pixel larger
        let t = caption(&Image::new(40, 10, 0xFFFFFFFF), &"x".encode_utf16().collect::<Vec<_>>(), true, 8, 0xFF000000, 0xFF00FF00);
        assert_eq!((t.w, t.h), (40, 26));
    }
}
