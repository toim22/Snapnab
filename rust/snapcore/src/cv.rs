//! In-house computer vision (Cv.cpp; the TSCV.* equivalents, ds_arch §11 / ds_pseudocode §9): the magic wand, masks,
//! inpainting (Dingo7 and diffusion), the Smart Move tolerance and the phase-correlation translation detector of
//! scrolling captures. The C++ arithmetic is kept (float where it was float; complex numbers multiplied and measured as
//! MSVC's std::complex does: plain products, hypot), so the results are the same.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};

use crate::gif::msvc_sort;
use crate::image::{Image, Rect, argb, ca, cb, cg, cr};

/// w × h, 0 or 255
pub type Mask = Vec<u8>;

// ---------------------------------------------------------------- magic wand

/// WandSelector::Select: region growing ordered by cost; `global` takes every pixel of a similar color.
pub fn wand_select(img: &Image, seed: (i32, i32), tolerance: f32, global: bool) -> Mask {
    let mut m = vec![0u8; img.px.len()];
    let (sx, sy) = seed;
    if img.is_empty() || sx < 0 || sy < 0 || sx >= img.wi() || sy >= img.hi() {
        return m;
    }
    let tolerance = if 0.0 < tolerance && tolerance <= 100.0 { tolerance } else { tolerance.clamp(0.01, 100.0) };
    let s = img.at(sx, sy);
    let limit = (tolerance * 255.0 / 100.0).round() as i32;
    let cost = |p: u32| {
        (cr(p) - cr(s)).abs().max((cg(p) - cg(s)).abs()).max((cb(p) - cb(s)).abs()).max((ca(p) - ca(s)).abs())
    };
    if global {
        for (i, &p) in img.px.iter().enumerate() {
            if cost(p) <= limit {
                m[i] = 255;
            }
        }
        return m;
    }
    let w = img.wi();
    let mut pq = BinaryHeap::new();
    let mut seen = vec![0u8; m.len()];
    let si = sy * w + sx;
    pq.push(Reverse((0, si)));
    seen[si as usize] = 1;
    while let Some(Reverse((_, i))) = pq.pop() {
        m[i as usize] = 255;
        let (x, y) = (i % w, i / w);
        for (nx, ny) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
            if nx < 0 || ny < 0 || nx >= w || ny >= img.hi() {
                continue;
            }
            let j = ny * w + nx;
            if seen[j as usize] != 0 {
                continue;
            }
            seen[j as usize] = 1;
            let cj = cost(img.px[j as usize]);
            if cj <= limit {
                pq.push(Reverse((cj, j)));
            }
        }
    }
    m
}

/// DoWand: clamps to [0.01, 100]; 100 takes the whole layer. Empty outside the image.
pub fn do_wand(img: &Image, pt: (i32, i32), tol: f32, global: bool) -> Mask {
    if pt.0 < 0 || pt.1 < 0 || pt.0 >= img.wi() || pt.1 >= img.hi() {
        return Vec::new();
    }
    let mut tol = tol;
    if (0.01..=100.0).contains(&tol) {
        if tol == 100.0 {
            return vec![255; img.px.len()];
        }
    } else {
        tol = if tol < 0.01 { 0.01 } else { 100.0 };
    }
    wand_select(img, pt, tol, global)
}

// ---------------------------------------------------------------- masks

pub fn rect_mask(w: i32, h: i32, r: Rect) -> Mask {
    let mut m = vec![0u8; (w * h) as usize];
    for y in r.top.max(0)..r.bottom.min(h) {
        for x in r.left.max(0)..r.right.min(w) {
            m[(y * w + x) as usize] = 255;
        }
    }
    m
}

pub fn ellipse_mask(w: i32, h: i32, r: Rect) -> Mask {
    let mut m = vec![0u8; (w * h) as usize];
    let (cx, cy) = ((r.left + r.right) as f64 / 2.0, (r.top + r.bottom) as f64 / 2.0);
    let rx = 0.5f64.max((r.right - r.left) as f64 / 2.0);
    let ry = 0.5f64.max((r.bottom - r.top) as f64 / 2.0);
    for y in r.top.max(0)..r.bottom.min(h) {
        for x in r.left.max(0)..r.right.min(w) {
            let (dx, dy) = ((x as f64 + 0.5 - cx) / rx, (y as f64 + 0.5 - cy) / ry);
            if dx * dx + dy * dy <= 1.0 {
                m[(y * w + x) as usize] = 255;
            }
        }
    }
    m
}

pub fn polygon_mask(w: i32, h: i32, pts: &[(i32, i32)]) -> Mask {
    let mut m = vec![0u8; (w * h) as usize];
    if pts.len() < 3 {
        return m;
    }
    let mut xs: Vec<f64> = Vec::new();
    for y in 0..h {
        let sy = y as f64 + 0.5;
        xs.clear();
        let mut j = pts.len() - 1;
        for i in 0..pts.len() {
            let (y0, y1) = (pts[j].1 as f64, pts[i].1 as f64);
            if (y0 <= sy && y1 > sy) || (y1 <= sy && y0 > sy) {
                xs.push(pts[j].0 as f64 + (sy - y0) / (y1 - y0) * (pts[i].0 - pts[j].0) as f64);
            }
            j = i;
        }
        xs.sort_by(|a, b| a.total_cmp(b));
        let mut k = 0;
        while k + 1 < xs.len() {
            let x0 = 0.max((xs[k] - 0.5).ceil() as i32);
            let x1 = (w - 1).min((xs[k + 1] - 0.5).floor() as i32);
            for x in x0..=x1 {
                m[(y * w + x) as usize] = 255;
            }
            k += 2;
        }
    }
    m
}

/// The rectangle around the set pixels, or all zero.
pub fn mask_bounds(m: &[u8], w: i32, h: i32) -> Rect {
    let mut r = Rect::new(w, h, 0, 0);
    for y in 0..h {
        for x in 0..w {
            if m[(y * w + x) as usize] != 0 {
                r.left = r.left.min(x);
                r.right = r.right.max(x + 1);
                r.top = r.top.min(y);
                r.bottom = r.bottom.max(y + 1);
            }
        }
    }
    if r.right <= r.left { Rect::default() } else { r }
}

pub fn mask_empty(m: &[u8]) -> bool {
    m.iter().all(|&v| v == 0)
}

/// Separable dilation by a k × k rectangle (getStructuringElement(MORPH_RECT)).
pub fn dilate(m: &[u8], w: i32, h: i32, k: i32) -> Mask {
    let r = 1.max(k / 2);
    let (mut t, mut o) = (vec![0u8; m.len()], vec![0u8; m.len()]);
    let at = |y: i32, x: i32| (y * w + x) as usize;
    for y in 0..h {
        let mut last = -1000000;
        for x in 0..w {
            if m[at(y, x)] != 0 {
                last = x;
            } else if x - last <= r {
                t[at(y, x)] = 255;
            }
        }
        last = 1000000;
        for x in (0..w).rev() {
            if m[at(y, x)] != 0 {
                last = x;
                t[at(y, x)] = 255;
            } else if last - x <= r {
                t[at(y, x)] = 255;
            }
        }
    }
    for x in 0..w {
        let mut last = -1000000;
        for y in 0..h {
            if t[at(y, x)] != 0 {
                last = y;
                o[at(y, x)] = 255;
            } else if y - last <= r {
                o[at(y, x)] = 255;
            }
        }
        last = 1000000;
        for y in (0..h).rev() {
            if t[at(y, x)] != 0 {
                last = y;
            } else if last - y <= r {
                o[at(y, x)] = 255;
            }
        }
    }
    o
}

// ---------------------------------------------------------------- inpainting

/// Dingo7Inpainter: paints the selection with the dominant color of the ring around it. Returns that color and its
/// share of the ring, or None when there is no ring.
pub fn dingo7_inpaint(img: &mut Image, mask: &[u8], kernel: i32) -> Option<(u32, f64)> {
    let grown = dilate(mask, img.wi(), img.hi(), kernel);
    let mut hist: BTreeMap<u32, i32> = BTreeMap::new();
    let mut total = 0;
    for i in 0..grown.len() {
        if grown[i] != 0 && mask[i] == 0 {
            *hist.entry(img.px[i]).or_insert(0) += 1;
            total += 1;
        }
    }
    if hist.is_empty() {
        return None;
    }
    let (mut dom, mut best, mut best_bright) = (0u32, -1, -1);
    for (&c, &n) in &hist {
        let bright = ca(c) + cr(c) + cg(c) + cb(c); // tie-break by the sum of the 4 bytes
        if n > best || (n == best && bright > best_bright) {
            best = n;
            dom = c;
            best_bright = bright;
        }
    }
    for i in 0..mask.len() {
        if mask[i] != 0 {
            img.px[i] = dom;
        }
    }
    Some((dom, if total != 0 { best as f64 / total as f64 } else { 0.0 }))
}

/// Fills from the border inward with the average of the known neighbors, then smooths (Jacobi) inside the mask.
pub fn diffusion_inpaint(img: &mut Image, mask: &[u8], iterations: i32) {
    let (w, h) = (img.wi(), img.hi());
    let b = mask_bounds(mask, w, h);
    if b.right <= b.left {
        return;
    }
    let n = (w * h) as usize;
    let mut c: [Vec<f32>; 4] = [vec![0.0; n], vec![0.0; n], vec![0.0; n], vec![0.0; n]];
    let mut known = vec![1u8; n];
    for i in 0..mask.len() {
        if mask[i] != 0 {
            known[i] = 0;
            continue;
        }
        let p = img.px[i];
        c[0][i] = cr(p) as f32;
        c[1][i] = cg(p) as f32;
        c[2][i] = cb(p) as f32;
        c[3][i] = ca(p) as f32;
    }
    let mut frontier: Vec<i32> = Vec::new();
    for y in b.top..b.bottom {
        for x in b.left..b.right {
            let i = y * w + x;
            if known[i as usize] == 0 {
                frontier.push(i);
            }
        }
    }
    let mut next: Vec<i32> = Vec::new();
    while !frontier.is_empty() {
        next.clear();
        let mut upd: Vec<(i32, [f32; 4])> = Vec::new();
        for &i in &frontier {
            let (x, y) = (i % w, i / w);
            let mut s = [0f32; 4];
            let mut cnt = 0i32;
            let nb = [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1), (x - 1, y - 1), (x + 1, y - 1), (x - 1, y + 1), (x + 1, y + 1)];
            for (nx, ny) in nb {
                if nx < 0 || ny < 0 || nx >= w || ny >= h {
                    continue;
                }
                let j = (ny * w + nx) as usize;
                if known[j] == 0 {
                    continue;
                }
                for q in 0..4 {
                    s[q] += c[q][j];
                }
                cnt += 1;
            }
            if cnt != 0 {
                let nf = cnt as f32;
                upd.push((i, [s[0] / nf, s[1] / nf, s[2] / nf, s[3] / nf]));
            } else {
                next.push(i);
            }
        }
        if upd.is_empty() {
            break;
        }
        for (i, v) in &upd {
            for q in 0..4 {
                c[q][*i as usize] = v[q];
            }
            known[*i as usize] = 1;
        }
        std::mem::swap(&mut frontier, &mut next);
    }
    let iterations = if iterations <= 0 { ((b.right - b.left).max(b.bottom - b.top) / 2).clamp(20, 150) } else { iterations };
    let wu = w as usize;
    for _ in 0..iterations {
        for y in 1.max(b.top)..(h - 1).min(b.bottom) {
            for x in 1.max(b.left)..(w - 1).min(b.right) {
                let i = (y * w + x) as usize;
                if mask[i] == 0 {
                    continue;
                }
                for q in 0..4 {
                    c[q][i] = (c[q][i - 1] + c[q][i + 1] + c[q][i - wu] + c[q][i + wu]) * 0.25;
                }
            }
        }
    }
    for i in 0..mask.len() {
        if mask[i] != 0 {
            img.px[i] = argb((c[3][i] + 0.5) as i32, (c[0][i] + 0.5) as i32, (c[1][i] + 0.5) as i32, (c[2][i] + 0.5) as i32);
        }
    }
}

/// Dingo7 when the neighborhood is nearly uniform; diffusion otherwise.
pub fn smart_fill(img: &mut Image, mask: &[u8]) {
    let mut probe = img.clone();
    if let Some((_, share)) = dingo7_inpaint(&mut probe, mask, 7) {
        if share >= 0.6 {
            *img = probe;
            return;
        }
    }
    diffusion_inpaint(img, mask, 0);
}

// ---------------------------------------------------------------- Smart Move

pub fn smart_move_auto_tolerance(m: &Image) -> f32 {
    if m.is_empty() {
        return 10.0;
    }
    let (mut sum, mut sum2) = (0f64, 0f64);
    let n = m.px.len() as f64;
    for &p in &m.px {
        let l = (cr(p) + cg(p) + cb(p)) as f64 / 3.0;
        sum += l;
        sum2 += l * l;
    }
    let mean = (sum / n) as f32;
    let sigma = (0f64.max(sum2 / n - (sum / n) * (sum / n))).sqrt() as f32;
    ((sigma * 14.0 / 255.0) + 1.0) * (((255.0 - mean) * 3.0 / 255.0) + 2.0)
}

pub fn smart_move_tolerance_scaling(t: f32, in_min: f32, in_max: f32, out_min: f32, out_max: f32) -> f32 {
    if in_min != in_max {
        return out_min + (t - in_min) * (out_max - out_min) / (in_max - in_min);
    }
    in_min
}

// ---------------------------------------------------------------- phase correlation

#[derive(Clone, Copy, Default)]
struct Cpx {
    re: f64,
    im: f64,
}

impl Cpx {
    /// MSVC complex *=: (a·c − b·d, a·d + b·c)
    fn mul(self, o: Cpx) -> Cpx {
        Cpx { re: self.re * o.re - self.im * o.im, im: self.re * o.im + self.im * o.re }
    }
    fn add(self, o: Cpx) -> Cpx {
        Cpx { re: self.re + o.re, im: self.im + o.im }
    }
    fn sub(self, o: Cpx) -> Cpx {
        Cpx { re: self.re - o.re, im: self.im - o.im }
    }
    fn div(self, v: f64) -> Cpx {
        Cpx { re: self.re / v, im: self.im / v }
    }
}

fn fft1d(a: &mut [Cpx], base: usize, n: usize, inv: bool, stride: usize) {
    let at = |i: usize| base + i * stride;
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            a.swap(at(i), at(j));
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = 2.0 * 3.14159265358979323846 / len as f64 * if inv { 1.0 } else { -1.0 };
        let wl = Cpx { re: ang.cos(), im: ang.sin() };
        let mut i = 0;
        while i < n {
            let mut w = Cpx { re: 1.0, im: 0.0 };
            for k in 0..len / 2 {
                let u = a[at(i + k)];
                let v = a[at(i + k + len / 2)].mul(w);
                a[at(i + k)] = u.add(v);
                a[at(i + k + len / 2)] = u.sub(v);
                w = w.mul(wl);
            }
            i += len;
        }
        len <<= 1;
    }
    if inv {
        for i in 0..n {
            let p = at(i);
            a[p] = a[p].div(n as f64);
        }
    }
}

fn fft2d(a: &mut [Cpx], w: usize, h: usize, inv: bool) {
    for y in 0..h {
        fft1d(a, y * w, w, inv, 1);
    }
    for x in 0..w {
        fft1d(a, x, h, inv, w);
    }
}

fn next_pow2(v: i32) -> i32 {
    let mut p = 1;
    while p < v {
        p <<= 1;
    }
    p
}

/// PhaseCorrelationTranslationDetector: the offset of `b` relative to `a`, and the relative height of the correlation
/// peak.
pub fn detect_translation(a: &Image, b: &Image) -> ((i32, i32), f64) {
    if a.is_empty() || b.is_empty() || a.w != b.w || a.h != b.h {
        return ((0, 0), 0.0);
    }
    let (aw, ah) = (a.wi(), a.hi());
    let mut f = 1;
    while aw.max(ah) / f > 512 {
        f *= 2;
    }
    let (sw, sh) = (aw / f, ah / f);
    let (wp, hp) = (next_pow2(sw) as usize, next_pow2(sh) as usize);
    let mut fa = vec![Cpx::default(); wp * hp];
    let mut fb = vec![Cpx::default(); wp * hp];
    let gray = |im: &Image, out: &mut [Cpx]| {
        let mut mean = 0f64;
        let mut g = vec![0f64; (sw * sh) as usize];
        for y in 0..sh {
            for x in 0..sw {
                let mut s = 0f64;
                for yy in 0..f {
                    for xx in 0..f {
                        let p = im.at(x * f + xx, y * f + yy);
                        s += 0.114 * cb(p) as f64 + 0.587 * cg(p) as f64 + 0.299 * cr(p) as f64;
                    }
                }
                let i = (y * sw + x) as usize;
                g[i] = s / (f * f) as f64;
                mean += g[i];
            }
        }
        mean /= g.len() as f64;
        for y in 0..sh {
            let wy = 0.5 - 0.5 * (2.0 * 3.14159265 * y as f64 / 1.max(sh - 1) as f64).cos();
            for x in 0..sw {
                let wx = 0.5 - 0.5 * (2.0 * 3.14159265 * x as f64 / 1.max(sw - 1) as f64).cos();
                out[y as usize * wp + x as usize] = Cpx { re: (g[(y * sw + x) as usize] - mean) * wx * wy, im: 0.0 };
            }
        }
    };
    gray(a, &mut fa);
    gray(b, &mut fb);
    fft2d(&mut fa, wp, hp, false);
    fft2d(&mut fb, wp, hp, false);
    for i in 0..fa.len() {
        let c = fa[i].mul(Cpx { re: fb[i].re, im: -fb[i].im }); // mulSpectrums(A, conj(B))
        let mag = c.re.hypot(c.im);
        fa[i] = c.div(if mag > 0.0 { mag } else { 2.220446049250313e-16 });
    }
    fft2d(&mut fa, wp, hp, true);
    // the 6 largest local maxima of the correlation
    let sum: f64 = fa.iter().fold(0.0, |s, c| s + c.re.abs());
    #[derive(Clone, Copy)]
    struct Peak {
        v: f64,
        x: i32,
        y: i32,
    }
    let mut peaks: Vec<Peak> = Vec::new();
    let (wi, hi) = (wp as i32, hp as i32);
    for y in 0..hi {
        for x in 0..wi {
            let v = fa[(y * wi + x) as usize].re;
            let mut local_max = true;
            'k: for k in -1..=1 {
                for j in -1..=1 {
                    if k == 0 && j == 0 {
                        continue;
                    }
                    if fa[(((y + k + hi) % hi) * wi + (x + j + wi) % wi) as usize].re > v {
                        local_max = false;
                        break 'k;
                    }
                }
            }
            if local_max {
                peaks.push(Peak { v, x, y });
            }
        }
    }
    msvc_sort(&mut peaks, |p, q| p.v > q.v);
    peaks.truncate(6);
    let confidence = if sum > 0.0 && !peaks.is_empty() { peaks[0].v / (sum / fa.len() as f64) } else { 0.0 };

    // full-resolution SAD over a central band: b(p + d) = a(p)
    let (x0, x1) = (aw / 4, aw * 3 / 4);
    let sad_at = |dx: i32, dy: i32| -> i64 {
        let (y0, y1) = (0.max(-dy), ah.min(ah - dy));
        if y1 - y0 < ah / 8 || dx.abs() >= aw / 2 {
            return i64::MAX;
        }
        let (mut s, mut n) = (0i64, 0i32);
        let ys = 1.max((y1 - y0) / 400);
        let xs = 1.max((x1 - x0) / 200);
        let mut y = y0;
        while y < y1 {
            let mut x = x0.max(-dx);
            while x < x1.min(aw - dx) {
                let (p, q) = (a.at(x, y), b.at(x + dx, y + dy));
                s += ((cr(p) - cr(q)).abs() + (cg(p) - cg(q)).abs() + (cb(p) - cb(q)).abs()) as i64;
                n += 1;
                x += xs;
            }
            y += ys;
        }
        if n > 50 { s * 1000 / n as i64 } else { i64::MAX }
    };
    let mut cand: Vec<(i32, i32)> = Vec::new();
    for p in &peaks {
        let px = if p.x > wi / 2 { p.x - wi } else { p.x };
        let py = if p.y > hi / 2 { p.y - hi } else { p.y };
        cand.push((-px * f, -py * f));
    }
    // extra candidates from the row profile (vertical scrolling): average luminance per row
    {
        let (mut ra, mut rb) = (vec![0f64; ah as usize], vec![0f64; ah as usize]);
        for y in 0..ah {
            let (mut sa, mut sb, mut n) = (0f64, 0f64, 0i32);
            let mut x = x0;
            while x < x1 {
                let (p, q) = (a.at(x, y), b.at(x, y));
                sa += (cr(p) + cg(p) + cb(p)) as f64;
                sb += (cr(q) + cg(q) + cb(q)) as f64;
                n += 1;
                x += 2;
            }
            ra[y as usize] = sa / 1.max(n) as f64;
            rb[y as usize] = sb / 1.max(n) as f64;
        }
        let mut sc: Vec<(f64, i32)> = Vec::new();
        for d in -(ah * 7 / 8)..=ah * 7 / 8 {
            let (y0, y1) = (0.max(-d), ah.min(ah - d));
            let mut s = 0f64;
            for y in y0..y1 {
                s += (ra[y as usize] - rb[(y + d) as usize]).abs();
            }
            sc.push((s / 1.max(y1 - y0) as f64, d));
        }
        sc.sort_by(|p, q| p.0.total_cmp(&q.0).then(p.1.cmp(&q.1)));
        for item in sc.iter().take(4) {
            cand.push((0, item.1));
        }
    }
    let mut best = i64::MAX;
    let mut best_p = cand.first().copied().unwrap_or((0, 0));
    let l1 = |p: (i32, i32)| p.0.abs() + p.1.abs();
    for &(cx, cy) in &cand {
        let span = if cx != 0 { f } else { 0 };
        for dy in cy - f..=cy + f {
            for dx in cx - span..=cx + span {
                let s = sad_at(dx, dy);
                // tie (periodic content): prefers the smallest offset. The C++ product wraps around when `s` is the
                // "no overlap" maximum, which makes such a candidate a tie; that is kept.
                let tie = best != i64::MAX && (s - best).wrapping_abs().wrapping_mul(100) <= best;
                if s < best && !(tie && l1((dx, dy)) > l1(best_p)) {
                    best = s;
                    best_p = (dx, dy);
                } else if tie && l1((dx, dy)) < l1(best_p) {
                    best = best.min(s);
                    best_p = (dx, dy);
                }
            }
        }
    }
    (best_p, confidence)
}
