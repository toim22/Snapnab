//! Panoramic scrolling (Panoramic.cpp, PanoramaStitcher): successive frames of the same region, scrolled by the user
//! in any direction, fitted onto the previous one by the offset of their rows (or columns) and stitched into a single
//! long image.

use crate::image::{Image, Rect};

pub const MAX_SIZE: i32 = 32000;

/// The signature of each row (or column); single-color lines get 0 and do not count in the comparison.
fn signatures(img: &Image, rows: bool) -> Vec<u64> {
    let (n, len) = if rows { (img.hi(), img.wi()) } else { (img.wi(), img.hi()) };
    let w = img.w as usize;
    (0..n as usize)
        .map(|i| {
            let px = |k: usize| if rows { img.px[i * w + k] } else { img.px[k * w + i] } & 0xFF_FFFF;
            let first = px(0);
            let (mut h, mut uniform) = (1469598103934665603u64, true);
            for k in 0..len as usize {
                let p = px(k);
                uniform &= p == first;
                h = (h ^ p as u64).wrapping_mul(1099511628211);
            }
            if uniform { 0 } else { h | 1 }
        })
        .collect()
}

/// The offset d such that line y of `b` = line y + d of `a` (d > 0: the content moved up or left); None when nothing
/// moved or no offset fits.
fn offset(a: &Image, b: &Image, vertical: bool) -> Option<i32> {
    let (sa, sb) = (signatures(a, vertical), signatures(b, vertical));
    let n = sa.len() as i32;
    let min_overlap = 12.max(n / 6);
    let score = |dd: i32| {
        let (mut matched, mut used) = (0, 0);
        for y in 0.max(-dd)..n.min(n - dd) {
            let (x, z) = (sb[y as usize], sa[(y + dd) as usize]);
            if x == 0 || z == 0 {
                continue;
            }
            used += 1;
            if x == z {
                matched += 1;
            }
        }
        (if used > 0 { matched as f64 / used as f64 } else { 0.0 }, used)
    };
    let (s0, used0) = score(0);
    if s0 > 0.97 && used0 >= 4 {
        return None; // nothing moved
    }
    let (mut best, mut best_d, mut best_used) = (0.0f64, 0i32, 0);
    for dd in -(n - min_overlap)..=(n - min_overlap) {
        if dd == 0 {
            continue;
        }
        let (s, used) = score(dd);
        // on a tie, the smallest offset (scrolls between two frames are short)
        if used >= 8 && (s > best + 1e-9 || ((s - best).abs() < 1e-9 && dd.abs() < best_d.abs())) {
            (best, best_d, best_used) = (s, dd, used);
        }
    }
    (best >= 0.85 && best_used >= 8).then_some(best_d)
}

#[derive(Default)]
pub struct Stitcher {
    canvas: Image,
    prev: Image,
    /// 0 = still idle, 1 = vertical, 2 = horizontal
    axis: i32,
    /// position of the last frame in the result
    pos: (i32, i32),
}

impl Stitcher {
    pub fn result(&self) -> &Image {
        &self.canvas
    }

    pub fn axis(&self) -> i32 {
        self.axis
    }

    /// True when the frame added content.
    pub fn add(&mut self, f: &Image) -> bool {
        if f.is_empty() {
            return false;
        }
        if self.canvas.is_empty() {
            (self.canvas, self.prev, self.pos) = (f.clone(), f.clone(), (0, 0));
            return true;
        }
        if f.w != self.prev.w || f.h != self.prev.h {
            return false;
        }
        let mut found = None;
        if self.axis != 2 {
            found = offset(&self.prev, f, true).map(|d| (d, true));
        }
        if found.is_none() && self.axis != 1 {
            found = offset(&self.prev, f, false).map(|d| (d, false));
        }
        let Some((d, vertical)) = found else { return false };
        if self.axis == 0 {
            self.axis = if vertical { 1 } else { 2 };
        }
        let (mut nx, mut ny) = (self.pos.0 + if vertical { 0 } else { d }, self.pos.1 + if vertical { d } else { 0 });
        // grows the result to fit the frame at its new position
        let (cw, ch) = (self.canvas.wi(), self.canvas.hi());
        let (left, top) = (nx.min(0), ny.min(0));
        let (right, bottom) = (cw.max(nx + f.wi()), ch.max(ny + f.hi()));
        if right - left > MAX_SIZE || bottom - top > MAX_SIZE {
            return false;
        }
        if left < 0 || top < 0 || right > cw || bottom > ch {
            let mut big = Image::sized(right - left, bottom - top, 0xFFFF_FFFF);
            big.blit(&self.canvas, -left, -top, false);
            self.canvas = big;
            nx -= left;
            ny -= top;
        }
        // only the strip that just appeared (fixed content, such as headers, is not repeated)
        let mut strip = Rect::new(0, 0, f.wi(), f.hi());
        if vertical {
            if d > 0 { strip.top = f.hi() - d } else { strip.bottom = -d }
        } else if d > 0 {
            strip.left = f.wi() - d
        } else {
            strip.right = -d
        }
        self.canvas.blit(&f.crop(strip), nx + strip.left, ny + strip.top, false);
        self.pos = (nx, ny);
        self.prev = f.clone();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A page of distinct rows; the frame shows rows [top, top + h).
    fn page(w: i32, h: i32) -> Image {
        let mut p = Image::sized(w, h, 0xFFFF_FFFF);
        for y in 0..h {
            for x in 0..w {
                *p.at_mut(x, y) = 0xFF00_0000 | ((y as u32).wrapping_mul(2654435761) ^ (x as u32 * 40503)) & 0xFF_FFFF;
            }
        }
        p
    }

    #[test]
    fn stitches_a_vertical_scroll_down_and_back() {
        let p = page(60, 400);
        let mut s = Stitcher::default();
        for top in [0, 30, 75, 120, 200, 260, 200] {
            s.add(&p.crop(Rect::new(0, top, 60, top + 140)));
        }
        assert_eq!(s.axis(), 1);
        assert_eq!((s.result().wi(), s.result().hi()), (60, 400));
        assert_eq!(s.result().px, p.px);
        // a frame that did not move adds nothing
        assert!(!s.add(&p.crop(Rect::new(0, 200, 60, 340))));
    }

    #[test]
    fn stitches_a_horizontal_scroll() {
        let p = page(300, 50);
        // the page's columns are distinct too: transposing the roles
        let mut s = Stitcher::default();
        for left in [0, 40, 100, 160] {
            s.add(&p.crop(Rect::new(left, 0, left + 120, 50)));
        }
        assert_eq!(s.axis(), 2);
        assert_eq!((s.result().wi(), s.result().hi()), (280, 50));
        assert_eq!(s.result().crop(Rect::new(0, 0, 280, 50)).px, p.crop(Rect::new(0, 0, 280, 50)).px);
    }
}
