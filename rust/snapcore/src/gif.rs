//! Animated GIF writer (V2AGNative::GifCreator / GIFMaker::Writer, ds_pseudocode §11.5): 4 bits per pixel (16 colors,
//! the original's default) or 8, Floyd–Steinberg dithering, delta frames (only the changed rectangle, the rest
//! transparent), infinite loop. A port of Gif.cpp, writing the same bytes: the median cut palette sorts with
//! `msvc_sort`, the MSVC std::sort, whose order of equal elements decides the palette.
//!
//! Frames come at the GIF's size: the caller scales them (GDI+ stays in C++ for now).

use std::fs::File;
use std::io::Write;
use std::os::windows::fs::OpenOptionsExt;
use std::path::Path;

use crate::image::{Image, blend_over};

fn cr(c: u32) -> i32 {
    (c >> 16 & 255) as i32
}
fn cg(c: u32) -> i32 {
    (c >> 8 & 255) as i32
}
fn cb(c: u32) -> i32 {
    (c & 255) as i32
}
fn argb(a: i32, r: i32, g: i32, b: i32) -> u32 {
    (a as u32) << 24 | (r as u32) << 16 | (g as u32) << 8 | b as u32
}

// ---------------------------------------------------------------- std::sort of the MSVC STL (<algorithm>)

const ISORT_MAX: usize = 32;

fn insertion_sort<T: Copy>(v: &mut [T], lt: &impl Fn(&T, &T) -> bool) {
    for mid in 1..v.len() {
        let val = v[mid];
        if lt(&val, &v[0]) {
            v.copy_within(0..mid, 1);
            v[0] = val;
        } else {
            let mut hole = mid;
            while lt(&val, &v[hole - 1]) {
                v[hole] = v[hole - 1];
                hole -= 1;
            }
            v[hole] = val;
        }
    }
}

fn med3<T: Copy>(v: &mut [T], first: usize, mid: usize, last: usize, lt: &impl Fn(&T, &T) -> bool) {
    if lt(&v[mid], &v[first]) {
        v.swap(mid, first);
    }
    if lt(&v[last], &v[mid]) {
        v.swap(last, mid);
        if lt(&v[mid], &v[first]) {
            v.swap(mid, first);
        }
    }
}

fn guess_median<T: Copy>(v: &mut [T], first: usize, mid: usize, last: usize, lt: &impl Fn(&T, &T) -> bool) {
    let count = last - first;
    if count > 40 {
        // Tukey's ninther
        let step = (count + 1) >> 3;
        let two_step = step << 1;
        med3(v, first, first + step, first + two_step, lt);
        med3(v, mid - step, mid, mid + step, lt);
        med3(v, last - two_step, last - step, last, lt);
        med3(v, first + step, mid, last - step, lt);
    } else {
        med3(v, first, mid, last, lt);
    }
}

/// Three-way partition around a guessed median: [first, pfirst) < pivot == [pfirst, plast) < [plast, last).
fn partition<T: Copy>(v: &mut [T], first: usize, last: usize, lt: &impl Fn(&T, &T) -> bool) -> (usize, usize) {
    let mid = first + ((last - first) >> 1);
    guess_median(v, first, mid, last - 1, lt);
    let mut pfirst = mid;
    let mut plast = pfirst + 1;
    while first < pfirst && !lt(&v[pfirst - 1], &v[pfirst]) && !lt(&v[pfirst], &v[pfirst - 1]) {
        pfirst -= 1;
    }
    while plast < last && !lt(&v[plast], &v[pfirst]) && !lt(&v[pfirst], &v[plast]) {
        plast += 1;
    }
    let mut gfirst = plast;
    let mut glast = pfirst;
    loop {
        while gfirst < last {
            if lt(&v[pfirst], &v[gfirst]) {
            } else if lt(&v[gfirst], &v[pfirst]) {
                break;
            } else if plast != gfirst {
                v.swap(plast, gfirst);
                plast += 1;
            } else {
                plast += 1;
            }
            gfirst += 1;
        }
        while first < glast {
            let prev = glast - 1;
            if lt(&v[prev], &v[pfirst]) {
            } else if lt(&v[pfirst], &v[prev]) {
                break;
            } else {
                pfirst -= 1;
                if pfirst != prev {
                    v.swap(pfirst, prev);
                }
            }
            glast -= 1;
        }
        if glast == first && gfirst == last {
            return (pfirst, plast);
        }
        if glast == first {
            // no room at the bottom: rotate the pivot upward
            if plast != gfirst {
                v.swap(pfirst, plast);
            }
            plast += 1;
            v.swap(pfirst, gfirst);
            pfirst += 1;
            gfirst += 1;
        } else if gfirst == last {
            // no room at the top: rotate the pivot downward
            glast -= 1;
            pfirst -= 1;
            if glast != pfirst {
                v.swap(glast, pfirst);
            }
            plast -= 1;
            v.swap(pfirst, plast);
        } else {
            glast -= 1;
            v.swap(gfirst, glast);
            gfirst += 1;
        }
    }
}

fn push_heap_by_index<T: Copy>(v: &mut [T], mut hole: usize, top: usize, val: T, lt: &impl Fn(&T, &T) -> bool) {
    while top < hole {
        let idx = (hole - 1) >> 1;
        if !lt(&v[idx], &val) {
            break;
        }
        v[hole] = v[idx];
        hole = idx;
    }
    v[hole] = val;
}

fn pop_heap_hole_by_index<T: Copy>(v: &mut [T], mut hole: usize, bottom: usize, val: T, lt: &impl Fn(&T, &T) -> bool) {
    let top = hole;
    let mut idx = hole;
    let max_non_leaf = (bottom - 1) >> 1;
    while idx < max_non_leaf {
        idx = 2 * idx + 2;
        if lt(&v[idx], &v[idx - 1]) {
            idx -= 1;
        }
        v[hole] = v[idx];
        hole = idx;
    }
    if idx == max_non_leaf && bottom % 2 == 0 {
        v[hole] = v[bottom - 1];
        hole = bottom - 1;
    }
    push_heap_by_index(v, hole, top, val, lt);
}

/// std::make_heap then std::sort_heap (the introsort's fallback).
pub fn heap_sort<T: Copy>(v: &mut [T], lt: &impl Fn(&T, &T) -> bool) {
    let bottom = v.len();
    let mut hole = bottom >> 1;
    while hole > 0 {
        hole -= 1;
        let val = v[hole];
        pop_heap_hole_by_index(v, hole, bottom, val, lt);
    }
    let mut last = v.len();
    while last >= 2 {
        last -= 1;
        let val = v[last];
        v[last] = v[0];
        pop_heap_hole_by_index(&mut v[..last], 0, last, val, lt);
    }
}

fn sort_range<T: Copy>(v: &mut [T], mut first: usize, mut last: usize, mut ideal: isize, lt: &impl Fn(&T, &T) -> bool) {
    loop {
        if last - first <= ISORT_MAX {
            insertion_sort(&mut v[first..last], lt);
            return;
        }
        if ideal <= 0 {
            heap_sort(&mut v[first..last], lt);
            return;
        }
        let (mf, ms) = partition(v, first, last, lt);
        ideal = (ideal >> 1) + (ideal >> 2); // 1.5 log2(N) divisions
        if mf - first < last - ms {
            sort_range(v, first, mf, ideal, lt);
            first = ms;
        } else {
            sort_range(v, ms, last, ideal, lt);
            last = mf;
        }
    }
}

/// std::sort as the MSVC STL does it (introsort: insertion sort up to 32 elements, a three-way partition around
/// Tukey's ninther, heap sort past 1.5 log2(N) divisions), so equal elements end in the same order.
pub fn msvc_sort<T: Copy>(v: &mut [T], lt: impl Fn(&T, &T) -> bool) {
    let n = v.len();
    sort_range(v, 0, n, n as isize, &lt);
}

// ---------------------------------------------------------------- palette and compression

/// Median cut palette of at most `max_colors` (reorders `cols`).
fn median_cut(cols: &mut [u32], max_colors: usize) -> Vec<u32> {
    let mut boxes: Vec<(usize, usize)> = vec![(0, cols.len())];
    let range_of = |cols: &[u32], (lo, hi): (usize, usize)| {
        let (mut mn, mut mx) = ([255; 3], [0; 3]);
        for &c in &cols[lo..hi] {
            for (k, v) in [cr(c), cg(c), cb(c)].into_iter().enumerate() {
                mn[k] = mn[k].min(v);
                mx[k] = mx[k].max(v);
            }
        }
        let (mut best, mut ch) = (0, 0);
        for k in 0..3 {
            if mx[k] - mn[k] > best {
                best = mx[k] - mn[k];
                ch = k;
            }
        }
        (best, ch)
    };
    while boxes.len() < max_colors {
        let (mut bi, mut best_r, mut best_ch) = (None, 0, 0);
        for (i, &b) in boxes.iter().enumerate() {
            if b.1 - b.0 < 2 {
                continue;
            }
            let (r, ch) = range_of(cols, b);
            if r > best_r {
                best_r = r;
                bi = Some(i);
                best_ch = ch;
            }
        }
        let Some(bi) = bi else { break };
        let (lo, hi) = boxes[bi];
        let shift = 16 - best_ch * 8;
        msvc_sort(&mut cols[lo..hi], |a, c| (a >> shift) & 255 < (c >> shift) & 255);
        let mid = (lo + hi) / 2;
        boxes[bi] = (lo, mid);
        boxes.push((mid, hi));
    }
    let mut pal = Vec::new();
    for &(lo, hi) in &boxes {
        let n = (hi - lo) as i64;
        if n <= 0 {
            continue;
        }
        let (mut r, mut g, mut b) = (0i64, 0i64, 0i64);
        for &c in &cols[lo..hi] {
            r += cr(c) as i64;
            g += cg(c) as i64;
            b += cb(c) as i64;
        }
        pal.push(argb(255, (r / n) as i32, (g / n) as i32, (b / n) as i32));
    }
    if pal.is_empty() {
        pal.push(0xFF000000);
    }
    pal
}

/// GIF's LZW: variable code size from min+1 to 12 bits, cleared when the dictionary fills.
struct Lzw<'a> {
    min: u32,
    out: &'a mut Vec<u8>,
    acc: u32,
    nbits: u32,
}

impl Lzw<'_> {
    fn emit(&mut self, code: i32, size: u32) {
        self.acc |= (code as u32) << self.nbits;
        self.nbits += size;
        while self.nbits >= 8 {
            self.out.push((self.acc & 255) as u8);
            self.acc >>= 8;
            self.nbits -= 8;
        }
    }

    fn encode(mut self, idx: &[u8]) {
        let clear = 1i32 << self.min;
        let eoi = clear + 1;
        let mut dict = vec![-1i32; 4096 * 256]; // (prefix, symbol) → code
        let mut next = eoi + 1;
        let mut size = self.min + 1;
        self.emit(clear, size);
        let mut prefix = idx.first().copied().unwrap_or(0) as i32;
        for &k in idx.iter().skip(1) {
            let slot = prefix as usize * 256 + k as usize;
            if dict[slot] >= 0 {
                prefix = dict[slot];
                continue;
            }
            self.emit(prefix, size);
            dict[slot] = next;
            next += 1;
            if next > (1 << size) && size < 12 {
                size += 1;
            }
            if next == 4096 {
                self.emit(clear, size);
                dict.fill(-1);
                next = eoi + 1;
                size = self.min + 1;
            }
            prefix = k as i32;
        }
        if !idx.is_empty() {
            self.emit(prefix, size);
        }
        self.emit(eoi, size);
        if self.nbits > 0 {
            self.out.push(self.acc as u8);
        }
    }
}

// ---------------------------------------------------------------- the writer

pub struct GifWriter {
    file: Option<File>,
    w: i32,
    h: i32,
    bpp: i32,
    delay: i32,
    dither: bool,
    delta: bool,
    failed: bool,
    prev: Vec<u32>, // last frame shown (for delta frames)
    buf: Vec<u8>,
}

impl GifWriter {
    /// Creates the file and writes the header, the infinite loop and the comment; None if it cannot be created.
    #[allow(clippy::too_many_arguments)]
    pub fn open(path: &Path, w: i32, h: i32, fps: f64, bpp: i32, dither: bool, delta: bool, comment: &[u8]) -> Option<Self> {
        // exclusive, as CreateFileW with no sharing
        let Ok(file) = std::fs::OpenOptions::new().write(true).create(true).truncate(true).share_mode(0).open(path) else {
            crate::log::write(&format!("Failed to open {} for writing", path.display()));
            return None;
        };
        let mut g = GifWriter {
            file: Some(file),
            w,
            h,
            bpp: if bpp <= 4 { 4 } else { 8 },
            delay: 2.max((100.0 / fps.max(1.0)).round() as i32),
            dither,
            delta,
            failed: false,
            prev: Vec::new(),
            buf: Vec::new(),
        };
        g.put(b"GIF89a");
        g.put_word(w as u16);
        g.put_word(h as u16);
        g.put(&[0, 0, 0]); // no global palette (each frame has its own)
        g.put(&[0x21, 0xFF, 0x0B, b'N', b'E', b'T', b'S', b'C', b'A', b'P', b'E', b'2', b'.', b'0', 0x03, 0x01, 0x00, 0x00, 0x00]);
        if !comment.is_empty() {
            g.put(&[0x21, 0xFE]);
            for chunk in comment.chunks(255) {
                g.put(&[chunk.len() as u8]);
                g.put(chunk);
            }
            g.put(&[0]);
        }
        Some(g)
    }

    pub fn failed(&self) -> bool {
        self.failed
    }

    fn put(&mut self, d: &[u8]) {
        let Some(f) = self.file.as_mut() else { return };
        self.buf.extend_from_slice(d);
        if self.buf.len() > 1 << 20 {
            if f.write_all(&self.buf).is_err() {
                self.failed = true;
            }
            self.buf.clear();
        }
    }

    fn put_word(&mut self, v: u16) {
        self.put(&v.to_le_bytes());
    }

    /// Adds a frame of the GIF's size; `delay_cs` in hundredths of a second (≤ 0: from the frame rate).
    pub fn add_frame(&mut self, frame: &Image, delay_cs: i32) -> bool {
        if self.file.is_none() || self.failed || frame.w as i32 != self.w || frame.h as i32 != self.h {
            return false;
        }
        let (w, h) = (self.w, self.h);
        let px: Vec<u32> = frame.px.iter().map(|&p| blend_over(0xFFFFFFFF, p)).collect(); // no partial alpha in GIF
        let at = |x: i32, y: i32| px[(y * w + x) as usize];

        // changed rectangle (delta frames)
        let (mut left, mut top, mut right, mut bottom) = (0, 0, w, h);
        let use_delta = self.delta && !self.prev.is_empty();
        let mut changed = Vec::new();
        if use_delta {
            changed = vec![0u8; (w * h) as usize];
            let (mut l, mut t, mut r, mut b) = (w, h, 0, 0);
            for y in 0..h {
                for x in 0..w {
                    let i = (y * w + x) as usize;
                    let (a, c) = (px[i], self.prev[i]);
                    if (cr(a) - cr(c)).abs() + (cg(a) - cg(c)).abs() + (cb(a) - cb(c)).abs() > 12 {
                        changed[i] = 1;
                        l = l.min(x);
                        t = t.min(y);
                        r = r.max(x + 1);
                        b = b.max(y + 1);
                    }
                }
            }
            if r <= l {
                (l, t, r, b) = (0, 0, 1, 1); // identical frame: 1 pixel
            }
            (left, top, right, bottom) = (l, t, r, b);
        }
        let (fw, fh) = (right - left, bottom - top);
        let ncolors = 1 << self.bpp;
        let pal_colors = if use_delta { ncolors - 1 } else { ncolors }; // an index kept for transparency

        // colors sampled from the rectangle, and the palette
        let mut sample = Vec::new();
        let step = 1.max(((fw as f64 * fh as f64) / 40000.0).sqrt() as i32);
        let mut y = top;
        while y < bottom {
            let mut x = left;
            while x < right {
                if !use_delta || changed[(y * w + x) as usize] != 0 {
                    sample.push(at(x, y));
                }
                x += step;
            }
            y += step;
        }
        if sample.is_empty() {
            sample.push(at(left, top));
        }
        let pal = median_cut(&mut sample, pal_colors as usize);

        // 15-bit table → nearest index
        let mut lut = vec![0u8; 32768];
        for (i, l) in lut.iter_mut().enumerate() {
            let i = i as i32;
            let (r, g, b) = ((i >> 10 & 31) * 255 / 31, (i >> 5 & 31) * 255 / 31, (i & 31) * 255 / 31);
            let (mut best, mut bd) = (0, i32::MAX);
            for (k, &p) in pal.iter().enumerate() {
                let (dr, dg, db) = (r - cr(p), g - cg(p), b - cb(p));
                let d = dr * dr * 2 + dg * dg * 4 + db * db * 3;
                if d < bd {
                    bd = d;
                    best = k;
                }
            }
            *l = best as u8;
        }
        let nearest = |r: i32, g: i32, b: i32| lut[((r >> 3) << 10 | (g >> 3) << 5 | b >> 3) as usize];

        let trans = (ncolors - 1) as u8;
        let mut idx = vec![0u8; (fw * fh) as usize];
        let (mut err0, mut err1) = (vec![0f32; (fw as usize + 2) * 3], vec![0f32; (fw as usize + 2) * 3]);
        for y in 0..fh {
            err1.fill(0.0);
            for x in 0..fw {
                let gi = ((top + y) * w + left + x) as usize;
                let xi = x as usize;
                if use_delta && changed[gi] == 0 {
                    idx[(y * fw + x) as usize] = trans;
                    continue;
                }
                let p = px[gi];
                let (mut r, mut g, mut b) = (cr(p), cg(p), cb(p));
                if self.dither {
                    r = ((r as f32 + err0[(xi + 1) * 3]) as i32).clamp(0, 255);
                    g = ((g as f32 + err0[(xi + 1) * 3 + 1]) as i32).clamp(0, 255);
                    b = ((b as f32 + err0[(xi + 1) * 3 + 2]) as i32).clamp(0, 255);
                }
                let k = nearest(r, g, b);
                idx[(y * fw + x) as usize] = k;
                if self.dither {
                    // Floyd–Steinberg
                    let pk = pal[k as usize];
                    let e = [(r - cr(pk)) as f32, (g - cg(pk)) as f32, (b - cb(pk)) as f32];
                    for c in 0..3 {
                        err0[(xi + 2) * 3 + c] += e[c] * 7.0 / 16.0;
                        err1[xi * 3 + c] += e[c] * 3.0 / 16.0;
                        err1[(xi + 1) * 3 + c] += e[c] * 5.0 / 16.0;
                        err1[(xi + 2) * 3 + c] += e[c] / 16.0;
                    }
                }
            }
            std::mem::swap(&mut err0, &mut err1);
        }
        // the frame shown after this one (for the next delta)
        if self.prev.is_empty() {
            self.prev = vec![0; (w * h) as usize];
        }
        for y in 0..fh {
            for x in 0..fw {
                let k = idx[(y * fw + x) as usize];
                if use_delta && k == trans {
                    continue;
                }
                self.prev[((top + y) * w + left + x) as usize] = pal[k as usize];
            }
        }

        // Graphic Control Extension: disposal 1 (keep) and the transparency flag
        self.put(&[0x21, 0xF9, 4, (1 << 2) | use_delta as u8]);
        self.put_word(if delay_cs > 0 { delay_cs } else { self.delay } as u16);
        self.put(&[if use_delta { trans } else { 0 }, 0]);
        // Image Descriptor with a local palette
        self.put(&[0x2C]);
        for v in [left, top, fw, fh] {
            self.put_word(v as u16);
        }
        self.put(&[0x80 | (self.bpp - 1) as u8]);
        for k in 0..ncolors as usize {
            let c = pal.get(k).copied().unwrap_or(0);
            self.put(&[cr(c) as u8, cg(c) as u8, cb(c) as u8]);
        }
        let min_code = 2.max(self.bpp);
        self.put(&[min_code as u8]);
        let mut data = Vec::new();
        Lzw { min: min_code as u32, out: &mut data, acc: 0, nbits: 0 }.encode(&idx);
        for chunk in data.chunks(255) {
            self.put(&[chunk.len() as u8]);
            self.put(chunk);
        }
        self.put(&[0]);
        !self.failed
    }

    /// Writes the trailer and closes the file; false if anything could not be written.
    pub fn close(mut self) -> bool {
        self.put(&[0x3B]);
        if let Some(mut f) = self.file.take() {
            if !self.buf.is_empty() && f.write_all(&self.buf).is_err() {
                self.failed = true;
            }
        }
        !self.failed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_orders() {
        // pseudo-random keys with many repeats, sizes across the insertion sort, partition and heap sort paths
        let mut seed = 12345u32;
        for n in [0usize, 1, 2, 31, 32, 33, 41, 100, 1000, 20000] {
            let mut v: Vec<u32> = (0..n)
                .map(|_| {
                    seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
                    seed
                })
                .collect();
            msvc_sort(&mut v, |a, b| a >> 28 < b >> 28);
            assert!(v.windows(2).all(|w| w[0] >> 28 <= w[1] >> 28), "{n}");
        }
    }

    #[test]
    fn writes_a_gif() {
        let p = std::env::temp_dir().join(format!("snapcore_gif_{}.gif", std::process::id()));
        let mut g = GifWriter::open(&p, 40, 30, 10.0, 8, true, true, b"Snapnab").unwrap();
        let mut img = Image::new(40, 30, 0xFF2080C0);
        assert!(g.add_frame(&img, -1));
        for i in 0..200 {
            img.px[i] = 0xFFFF0000;
        }
        assert!(g.add_frame(&img, 50));
        assert!(!g.add_frame(&Image::new(10, 10, 0), -1)); // another size
        assert!(g.close());
        let data = std::fs::read(&p).unwrap();
        assert!(data.starts_with(b"GIF89a") && data.ends_with(&[0x3B]));
        let decoded = crate::codec::decode(&data).unwrap();
        assert_eq!((decoded.w, decoded.h), (40, 30));
        let _ = std::fs::remove_file(&p);
    }
}
