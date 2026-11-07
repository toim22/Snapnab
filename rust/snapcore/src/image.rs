//! In-memory image (BGRA 32 bpp, straight alpha), as `Image` in Image.h: 0xAARRGGBB per pixel, rows from top to bottom,
//! with the helpers of Image.cpp (Crop, Blit, FillRect, BlendOver) and of Image.h (ARGB, CA…, FromColorref).

/// Metadata carried with an image (C29): what an imported photo had (EXIF) and what Snapnab writes when saving.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Meta {
    pub date_taken: String, // "YYYY:MM:DD HH:MM:SS" (EXIF form)
    pub make: String,
    pub model: String,
    pub software: String,
    pub gps: bool,
    pub lat: f64, // degrees, south negative
    pub lon: f64, // degrees, west negative
}

impl Meta {
    pub fn is_empty(&self) -> bool {
        self.date_taken.is_empty() && self.make.is_empty() && self.model.is_empty() && self.software.is_empty() && !self.gps
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Image {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u32>,
    pub dpi: f32,           // resolution written to the file (0 = the encoder's default)
    pub meta: Option<Meta>, // metadata of the source photo
}

impl Image {
    pub fn new(w: u32, h: u32, fill: u32) -> Self {
        Image { w, h, px: vec![fill; w as usize * h as usize], dpi: 0.0, meta: None }
    }

    pub fn is_empty(&self) -> bool {
        self.w == 0 || self.h == 0
    }

    pub fn has_transparency(&self) -> bool {
        self.px.iter().any(|&p| p >> 24 != 255)
    }

    /// The image composed over an opaque color (JPEG and BMP have no alpha).
    pub fn flattened(&self, background: u32) -> Image {
        Image { px: self.px.iter().map(|&s| blend_over(background, s)).collect(), meta: self.meta.clone(), ..*self }
    }

    /// The pixels as bytes (B, G, R, A per pixel).
    pub fn bytes(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.px.as_ptr() as *const u8, self.px.len() * 4) }
    }

    pub fn bytes_mut(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.px.as_mut_ptr() as *mut u8, self.px.len() * 4) }
    }
}

/// RECT: left, top, right, bottom (right and bottom outside).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
        Rect { left, top, right, bottom }
    }
    pub fn width(&self) -> i32 {
        self.right - self.left
    }
    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }
    /// IsRectEmpty
    pub fn is_empty(&self) -> bool {
        self.right <= self.left || self.bottom <= self.top
    }
    /// IntersectRect: the common part, or an empty (all zero) rectangle
    pub fn intersect(&self, o: &Rect) -> Rect {
        let r = Rect::new(self.left.max(o.left), self.top.max(o.top), self.right.min(o.right), self.bottom.min(o.bottom));
        if r.is_empty() { Rect::default() } else { r }
    }
    /// UnionRect (an empty side is ignored)
    pub fn union(&self, o: &Rect) -> Rect {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        Rect::new(self.left.min(o.left), self.top.min(o.top), self.right.max(o.right), self.bottom.max(o.bottom))
    }
    /// OffsetRect
    pub fn offset(&self, dx: i32, dy: i32) -> Rect {
        Rect::new(self.left + dx, self.top + dy, self.right + dx, self.bottom + dy)
    }
    /// PtInRect (right and bottom outside)
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
}

pub fn argb(a: i32, r: i32, g: i32, b: i32) -> u32 {
    (a as u32) << 24 | (r as u32) << 16 | (g as u32) << 8 | b as u32
}
pub fn ca(p: u32) -> i32 {
    (p >> 24) as i32 & 255
}
pub fn cr(p: u32) -> i32 {
    (p >> 16) as i32 & 255
}
pub fn cg(p: u32) -> i32 {
    (p >> 8) as i32 & 255
}
pub fn cb(p: u32) -> i32 {
    p as i32 & 255
}
/// FromColorref: a COLORREF (0x00BBGGRR) as an opaque pixel.
pub fn from_colorref(c: u32) -> u32 {
    argb(255, (c & 255) as i32, (c >> 8 & 255) as i32, (c >> 16 & 255) as i32)
}

impl Image {
    /// Image(w, h, fill) of the C++ (sizes as int).
    pub fn sized(w: i32, h: i32, fill: u32) -> Image {
        Image::new(w.max(0) as u32, h.max(0) as u32, fill)
    }
    pub fn wi(&self) -> i32 {
        self.w as i32
    }
    pub fn hi(&self) -> i32 {
        self.h as i32
    }
    pub fn at(&self, x: i32, y: i32) -> u32 {
        self.px[y as usize * self.w as usize + x as usize]
    }
    pub fn at_mut(&mut self, x: i32, y: i32) -> &mut u32 {
        let w = self.w as usize;
        &mut self.px[y as usize * w + x as usize]
    }
    pub fn row(&self, y: i32) -> &[u32] {
        let w = self.w as usize;
        &self.px[y as usize * w..(y as usize + 1) * w]
    }

    /// Crop: the part inside the image, or an empty image.
    pub fn crop(&self, r: Rect) -> Image {
        let (l, t) = (r.left.max(0), r.top.max(0));
        let (rr, b) = (r.right.min(self.wi()), r.bottom.min(self.hi()));
        if rr <= l || b <= t {
            return Image::default();
        }
        let mut out = Image::sized(rr - l, b - t, 0);
        let w = out.w as usize;
        for y in 0..out.hi() {
            let src = &self.row(t + y)[l as usize..l as usize + w];
            out.px[y as usize * w..(y as usize + 1) * w].copy_from_slice(src);
        }
        out
    }

    /// Blit: `src` at (dx, dy), blended over (straight alpha) or copied.
    pub fn blit(&mut self, src: &Image, dx: i32, dy: i32, blend: bool) {
        for y in 0..src.hi() {
            let ty = dy + y;
            if ty < 0 || ty >= self.hi() {
                continue;
            }
            for x in 0..src.wi() {
                let tx = dx + x;
                if tx < 0 || tx >= self.wi() {
                    continue;
                }
                let s = src.at(x, y);
                let d = self.at_mut(tx, ty);
                *d = if blend { blend_over(*d, s) } else { s };
            }
        }
    }

    /// MakeOpaque
    pub fn make_opaque(&mut self) {
        for p in &mut self.px {
            *p |= 0xFF00_0000;
        }
    }

    pub fn fill_rect(&mut self, r: Rect, color: u32) {
        let (l, t) = (r.left.max(0), r.top.max(0));
        let (rr, b) = (r.right.min(self.wi()), r.bottom.min(self.hi()));
        for y in t..b {
            for x in l..rr {
                *self.at_mut(x, y) = color;
            }
        }
    }
}

/// `src` over `dst` with straight alpha, with the integer arithmetic of BlendOver in Image.cpp.
pub fn blend_over(d: u32, s: u32) -> u32 {
    let sa = (s >> 24) as i32;
    if sa == 255 {
        return s;
    }
    if sa == 0 {
        return d;
    }
    let da = (d >> 24) as i32;
    let oa = sa + da * (255 - sa) / 255;
    if oa == 0 {
        return 0;
    }
    let ch = |shift: u32| {
        let (sc, dc) = (((s >> shift) & 255) as i32, ((d >> shift) & 255) as i32);
        ((sc * sa + dc * da * (255 - sa) / 255) / oa) as u32
    };
    (oa as u32) << 24 | ch(16) << 16 | ch(8) << 8 | ch(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blending() {
        assert_eq!(blend_over(0xFFFFFFFF, 0xFF102030), 0xFF102030);
        assert_eq!(blend_over(0xFFFFFFFF, 0x00102030), 0xFFFFFFFF);
        assert_eq!(blend_over(0x00000000, 0x00102030), 0x00000000);
        // half-transparent black over white: (0*128 + 255*255*127/255) / 255 = 127
        assert_eq!(blend_over(0xFFFFFFFF, 0x80000000), 0xFF7F7F7F);
        let img = Image::new(2, 1, 0x80FF0000);
        assert!(img.has_transparency());
        let flat = img.flattened(0xFFFFFFFF);
        assert!(!flat.has_transparency());
        assert_eq!(flat.px[0], blend_over(0xFFFFFFFF, 0x80FF0000));
    }
}
