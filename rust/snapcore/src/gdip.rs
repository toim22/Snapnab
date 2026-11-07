//! GDI+ through its flat API, for the drawing the C++ did with the Gdiplus classes: each wrapper makes the same flat call
//! the class did (Bitmap(w, h, stride, PixelFormat32bppARGB, scan0) → GdipCreateBitmapFromScan0, Graphics(image) →
//! GdipGetImageGraphicsContext…), so the pixels come out the same. Statuses are ignored, as the C++ did.

use std::marker::PhantomData;
use std::sync::OnceLock;

use windows::Win32::Graphics::GdiPlus::*;
use windows::core::PCWSTR;

use crate::image::Image;

/// PixelFormat32bppARGB (10 | 32 bpp | alpha | GDI | canonical).
const PIXEL_FORMAT_32BPP_ARGB: i32 = 10 | (32 << 8) | 0x40000 | 0x20000 | 0x200000;

/// GDI+ for the process: the programs start it themselves; this covers the Rust tests (never shut down).
pub fn startup() {
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        let input = GdiplusStartupInput { GdiplusVersion: 1, ..Default::default() };
        let mut token = 0usize;
        unsafe { GdiplusStartup(&mut token, &input, std::ptr::null_mut()) };
    });
}

/// A GDI+ bitmap over the pixels of an Image (Image::WrapGdip: no copy, drawn into in place).
pub struct Bitmap<'a> {
    raw: *mut GpBitmap,
    _px: PhantomData<&'a mut [u32]>,
}

impl Bitmap<'static> {
    /// Bitmap(w, h, PixelFormat32bppARGB): its own memory (the 1×1 bitmaps the C++ measured text on).
    pub fn blank(w: i32, h: i32) -> Bitmap<'static> {
        startup();
        let mut raw = std::ptr::null_mut();
        unsafe { GdipCreateBitmapFromScan0(w, h, 0, PIXEL_FORMAT_32BPP_ARGB, None, &mut raw) };
        Bitmap { raw, _px: PhantomData }
    }
}

impl<'a> Bitmap<'a> {
    pub fn wrap(img: &'a mut Image) -> Bitmap<'a> {
        Self::over(img.w, img.h, img.px.as_mut_ptr())
    }

    /// A source bitmap: GDI+ only reads it (the C++ cast the const away).
    pub fn wrap_read(img: &'a Image) -> Bitmap<'a> {
        Self::over(img.w, img.h, img.px.as_ptr() as *mut u32)
    }

    /// A bitmap over pixels the caller keeps alive (and may also change directly between flushes).
    ///
    /// # Safety
    /// `px` must point to `w * h` pixels that outlive the bitmap.
    pub unsafe fn over_raw(w: u32, h: u32, px: *mut u32) -> Bitmap<'a> {
        Self::over(w, h, px)
    }

    fn over(w: u32, h: u32, px: *mut u32) -> Bitmap<'a> {
        startup();
        let mut raw = std::ptr::null_mut();
        unsafe {
            GdipCreateBitmapFromScan0(w as i32, h as i32, w as i32 * 4, PIXEL_FORMAT_32BPP_ARGB, Some(px as *const u8), &mut raw)
        };
        Bitmap { raw, _px: PhantomData }
    }

    fn image(&self) -> *mut GpImage {
        self.raw as *mut GpImage
    }
}

impl Drop for Bitmap<'_> {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe { GdipDisposeImage(self.raw as *mut GpImage) };
        }
    }
}

pub struct Graphics<'b> {
    raw: *mut GpGraphics,
    owned: bool,
    _target: PhantomData<&'b ()>,
}

impl<'b> Graphics<'b> {
    pub fn new(target: &'b Bitmap<'_>) -> Graphics<'b> {
        let mut raw = std::ptr::null_mut();
        unsafe { GdipGetImageGraphicsContext(target.image(), &mut raw) };
        Graphics { raw, owned: true, _target: PhantomData }
    }

    /// Graphics(hdc): drawing on a device context (a DIB section selected into a memory DC).
    pub fn from_hdc(hdc: isize) -> Graphics<'static> {
        startup();
        let mut raw = std::ptr::null_mut();
        unsafe { GdipCreateFromHDC(windows::Win32::Graphics::Gdi::HDC(hdc as _), &mut raw) };
        Graphics { raw, owned: true, _target: PhantomData }
    }

    /// Clear(color)
    pub fn clear(&self, argb: u32) {
        unsafe { GdipGraphicsClear(self.raw, argb) };
    }

    /// The Graphics a caller made (C++ `Graphics::nativeGraphics`): used, never deleted.
    ///
    /// # Safety
    /// `raw` must be a live GpGraphics for the life of the result.
    pub unsafe fn borrow(raw: *mut core::ffi::c_void) -> Graphics<'static> {
        Graphics { raw: raw as *mut GpGraphics, owned: false, _target: PhantomData }
    }

    pub fn interpolation(&self, m: InterpolationMode) {
        unsafe { GdipSetInterpolationMode(self.raw, m) };
    }
    pub fn smoothing(&self, m: SmoothingMode) {
        unsafe { GdipSetSmoothingMode(self.raw, m) };
    }
    pub fn pixel_offset(&self, m: PixelOffsetMode) {
        unsafe { GdipSetPixelOffsetMode(self.raw, m) };
    }
    pub fn compositing(&self, m: CompositingMode) {
        unsafe { GdipSetCompositingMode(self.raw, m) };
    }
    pub fn text_hint(&self, h: TextRenderingHint) {
        unsafe { GdipSetTextRenderingHint(self.raw, h) };
    }
    pub fn translate(&self, dx: f32, dy: f32) {
        unsafe { GdipTranslateWorldTransform(self.raw, dx, dy, MatrixOrderPrepend) };
    }
    pub fn rotate(&self, degrees: f32) {
        unsafe { GdipRotateWorldTransform(self.raw, degrees, MatrixOrderPrepend) };
    }
    /// DrawImage(image, x, y, w, h)
    pub fn draw_image(&self, img: &Bitmap, x: f32, y: f32, w: f32, h: f32) {
        unsafe { GdipDrawImageRect(self.raw, img.image(), x, y, w, h) };
    }
    /// DrawImage(image, Rect dst, srcx, srcy, srcw, srch, UnitPixel, attributes)
    pub fn draw_image_rect(&self, img: &Bitmap, dst: [i32; 4], src: [i32; 4], attr: &ImageAttributes) {
        unsafe {
            GdipDrawImageRectRectI(self.raw, img.image(), dst[0], dst[1], dst[2], dst[3], src[0], src[1], src[2], src[3],
                                   UnitPixel, attr.0, 0, std::ptr::null_mut())
        };
    }
    pub fn fill_polygon(&self, brush: &Brush, pts: &[PointF]) {
        unsafe { GdipFillPolygon(self.raw, brush.0, pts.as_ptr(), pts.len() as i32, FillModeAlternate) };
    }
    pub fn draw_polygon(&self, pen: &Pen, pts: &[PointF]) {
        unsafe { GdipDrawPolygon(self.raw, pen.0, pts.as_ptr(), pts.len() as i32) };
    }
    pub fn draw_line(&self, pen: &Pen, x1: f32, y1: f32, x2: f32, y2: f32) {
        unsafe { GdipDrawLine(self.raw, pen.0, x1, y1, x2, y2) };
    }
    /// DrawString(text, -1, font, rect, format, brush); `text` without the terminating zero.
    pub fn draw_string(&self, text: &[u16], font: &Font, rect: RectF, format: &StringFormat, brush: &Brush) {
        let t: Vec<u16> = text.iter().copied().chain(std::iter::once(0)).collect();
        unsafe { GdipDrawString(self.raw, PCWSTR(t.as_ptr()), -1, font.font, &rect, format.0, brush.0) };
    }
}

impl Drop for Graphics<'_> {
    fn drop(&mut self) {
        if self.owned && !self.raw.is_null() {
            unsafe { GdipDeleteGraphics(self.raw) };
        }
    }
}

pub struct Brush(*mut GpBrush);

impl Brush {
    /// SolidBrush(Color)
    pub fn solid(argb: u32) -> Brush {
        startup();
        let mut b = std::ptr::null_mut();
        unsafe { GdipCreateSolidFill(argb, &mut b) };
        Brush(b as *mut GpBrush)
    }
    /// LinearGradientBrush(point1, point2, color1, color2): WrapModeTile.
    pub fn linear(p1: PointF, p2: PointF, c1: u32, c2: u32) -> Brush {
        startup();
        let mut b = std::ptr::null_mut();
        unsafe { GdipCreateLineBrush(&p1, &p2, c1, c2, WrapModeTile, &mut b) };
        Brush(b as *mut GpBrush)
    }
}

impl Drop for Brush {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { GdipDeleteBrush(self.0) };
        }
    }
}

pub struct Pen(*mut GpPen, bool);

impl Pen {
    /// Pen(Color, width): UnitWorld.
    pub fn new(argb: u32, width: f32) -> Pen {
        startup();
        let mut p = std::ptr::null_mut();
        unsafe { GdipCreatePen1(argb, width, UnitWorld, &mut p) };
        Pen(p, true)
    }
    pub fn dash(&self, style: DashStyle) {
        unsafe { GdipSetPenDashStyle(self.0, style) };
    }
}

impl Drop for Pen {
    fn drop(&mut self) {
        if self.1 && !self.0.is_null() {
            unsafe { GdipDeletePen(self.0) };
        }
    }
}

/// FontFamily(name) and Font(&family, size, style, UnitPixel).
pub struct Font {
    family: *mut GpFontFamily,
    font: *mut GpFont,
}

impl Font {
    pub fn new(family: &str, px: f32, style: FontStyle) -> Font {
        startup();
        let name: Vec<u16> = family.encode_utf16().chain(std::iter::once(0)).collect();
        let (mut fam, mut font) = (std::ptr::null_mut(), std::ptr::null_mut());
        unsafe {
            GdipCreateFontFamilyFromName(PCWSTR(name.as_ptr()), std::ptr::null_mut(), &mut fam);
            GdipCreateFont(fam, px, style.0, UnitPixel, &mut font);
        }
        Font { family: fam, font }
    }
}

impl Drop for Font {
    fn drop(&mut self) {
        unsafe {
            if !self.font.is_null() {
                GdipDeleteFont(self.font);
            }
            if !self.family.is_null() {
                GdipDeleteFontFamily(self.family);
            }
        }
    }
}

/// StringFormat() (no flags, LANG_NEUTRAL).
pub struct StringFormat(*mut GpStringFormat);

impl StringFormat {
    pub fn new() -> StringFormat {
        startup();
        let mut f = std::ptr::null_mut();
        unsafe { GdipCreateStringFormat(0, 0, &mut f) };
        StringFormat(f)
    }
    pub fn align(&self, a: StringAlignment) {
        unsafe { GdipSetStringFormatAlign(self.0, a) };
    }
    pub fn line_align(&self, a: StringAlignment) {
        unsafe { GdipSetStringFormatLineAlign(self.0, a) };
    }
    pub fn trimming(&self, t: StringTrimming) {
        unsafe { GdipSetStringFormatTrimming(self.0, t) };
    }
}

impl Drop for StringFormat {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { GdipDeleteStringFormat(self.0) };
        }
    }
}

pub struct ImageAttributes(*mut GpImageAttributes);

impl ImageAttributes {
    pub fn new() -> ImageAttributes {
        startup();
        let mut a = std::ptr::null_mut();
        unsafe { GdipCreateImageAttributes(&mut a) };
        ImageAttributes(a)
    }
    /// SetWrapMode(wrap): the border color is Color() (opaque black), no clamp.
    pub fn wrap_mode(&self, w: WrapMode) {
        unsafe { GdipSetImageAttributesWrapMode(self.0, w, 0xFF000000, false) };
    }
}

impl Drop for ImageAttributes {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { GdipDisposeImageAttributes(self.0) };
        }
    }
}

pub fn pt(x: f32, y: f32) -> PointF {
    PointF { X: x, Y: y }
}

pub fn rect(x: f32, y: f32, w: f32, h: f32) -> RectF {
    RectF { X: x, Y: y, Width: w, Height: h }
}

/// ScaleImage: GDI+ high quality bicubic (or nearest neighbor), half pixel offset, source copy, tile flip edges.
pub fn scale_image(src: &Image, nw: i32, nh: i32, high_quality: bool) -> Image {
    if src.is_empty() || nw <= 0 || nh <= 0 {
        return Image::default();
    }
    let mut dst = Image::new(nw as u32, nh as u32, 0);
    {
        let sb = Bitmap::wrap_read(src);
        let db = Bitmap::wrap(&mut dst);
        let g = Graphics::new(&db);
        g.interpolation(if high_quality { InterpolationModeHighQualityBicubic } else { InterpolationModeNearestNeighbor });
        g.pixel_offset(PixelOffsetModeHalf);
        g.compositing(CompositingModeSourceCopy);
        let ia = ImageAttributes::new();
        ia.wrap_mode(WrapModeTileFlipXY);
        g.draw_image_rect(&sb, [0, 0, nw, nh], [0, 0, src.w as i32, src.h as i32], &ia);
    }
    dst
}

/// MakeThumbnail: fitted inside max_w × max_h, never enlarged.
pub fn make_thumbnail(img: &Image, max_w: i32, max_h: i32) -> Image {
    if img.is_empty() {
        return Image::default();
    }
    let s = 1.0f64.min((max_w as f64 / img.w as f64).min(max_h as f64 / img.h as f64));
    scale_image(img, 1.max((img.w as f64 * s) as i32), 1.max((img.h as f64 * s) as i32), true)
}

// ---------------------------------------------------------------- the editor painters (Render.cpp)
// Each method is the flat call the Gdiplus class made, with the class's default arguments.

fn wz(text: &[u16]) -> Vec<u16> {
    text.iter().copied().chain(std::iter::once(0)).collect()
}

impl Graphics<'_> {
    pub fn raw(&self) -> *mut GpGraphics {
        self.raw
    }
    pub fn flush(&self) {
        unsafe { GdipFlush(self.raw, FlushIntentionSync) };
    }
    pub fn fill_path(&self, brush: &Brush, path: &Path) {
        unsafe { GdipFillPath(self.raw, brush.0, path.raw) };
    }
    pub fn draw_path(&self, pen: &Pen, path: &Path) {
        unsafe { GdipDrawPath(self.raw, pen.0, path.raw) };
    }
    /// SetClip(path) (CombineModeReplace)
    pub fn clip_path(&self, path: &Path) {
        unsafe { GdipSetClipPath(self.raw, path.raw, CombineModeReplace) };
    }
    pub fn save(&self) -> u32 {
        let mut st = 0u32;
        unsafe { GdipSaveGraphics(self.raw, &mut st) };
        st
    }
    pub fn restore(&self, st: u32) {
        unsafe { GdipRestoreGraphics(self.raw, st) };
    }
    pub fn line(&self, pen: &Pen, a: PointF, b: PointF) {
        unsafe { GdipDrawLine(self.raw, pen.0, a.X, a.Y, b.X, b.Y) };
    }
    pub fn draw_lines(&self, pen: &Pen, pts: &[PointF]) {
        unsafe { GdipDrawLines(self.raw, pen.0, pts.as_ptr(), pts.len() as i32) };
    }
    pub fn draw_bezier(&self, pen: &Pen, a: PointF, b: PointF, c: PointF, d: PointF) {
        unsafe { GdipDrawBezier(self.raw, pen.0, a.X, a.Y, b.X, b.Y, c.X, c.Y, d.X, d.Y) };
    }
    pub fn draw_curve(&self, pen: &Pen, pts: &[PointF], tension: f32) {
        unsafe { GdipDrawCurve2(self.raw, pen.0, pts.as_ptr(), pts.len() as i32, tension) };
    }
    pub fn fill_ellipse(&self, brush: &Brush, x: f32, y: f32, w: f32, h: f32) {
        unsafe { GdipFillEllipse(self.raw, brush.0, x, y, w, h) };
    }
    pub fn draw_ellipse(&self, pen: &Pen, x: f32, y: f32, w: f32, h: f32) {
        unsafe { GdipDrawEllipse(self.raw, pen.0, x, y, w, h) };
    }
    pub fn draw_arc(&self, pen: &Pen, r: RectF, start: f32, sweep: f32) {
        unsafe { GdipDrawArc(self.raw, pen.0, r.X, r.Y, r.Width, r.Height, start, sweep) };
    }
    pub fn fill_rect(&self, brush: &Brush, r: RectF) {
        unsafe { GdipFillRectangle(self.raw, brush.0, r.X, r.Y, r.Width, r.Height) };
    }
    pub fn draw_rect(&self, pen: &Pen, r: RectF) {
        unsafe { GdipDrawRectangle(self.raw, pen.0, r.X, r.Y, r.Width, r.Height) };
    }
    /// DrawImage(image, RectF)
    pub fn draw_image_f(&self, img: &Bitmap, r: RectF) {
        unsafe { GdipDrawImageRect(self.raw, img.image(), r.X, r.Y, r.Width, r.Height) };
    }
    /// DrawImage(image, RectF dst, srcx, srcy, srcw, srch, UnitPixel)
    pub fn draw_image_src(&self, img: &Bitmap, dst: RectF, src: RectF) {
        unsafe {
            GdipDrawImageRectRect(self.raw, img.image(), dst.X, dst.Y, dst.Width, dst.Height, src.X, src.Y, src.Width, src.Height,
                                  UnitPixel, std::ptr::null(), 0, std::ptr::null_mut())
        };
    }
    /// MeasureString(text, length, font, PointF origin, format, &RectF): the box from that origin.
    pub fn measure_at(&self, text: &[u16], font: &Font, origin: PointF, fmt: &StringFormat) -> RectF {
        let t = wz(text);
        let layout = rect(origin.X, origin.Y, 0.0, 0.0);
        let mut b = RectF::default();
        unsafe {
            GdipMeasureString(self.raw, PCWSTR(t.as_ptr()), text.len() as i32, font.font, &layout, fmt.0, &mut b,
                              std::ptr::null_mut(), std::ptr::null_mut())
        };
        b
    }
    /// MeasureString(text, -1, font, SizeF layout, format, &SizeF): width and height inside that size.
    pub fn measure_in(&self, text: &[u16], font: &Font, w: f32, h: f32, fmt: &StringFormat) -> (f32, f32) {
        let t = wz(text);
        let layout = rect(0.0, 0.0, w, h);
        let mut b = RectF::default();
        unsafe {
            GdipMeasureString(self.raw, PCWSTR(t.as_ptr()), -1, font.font, &layout, fmt.0, &mut b, std::ptr::null_mut(),
                              std::ptr::null_mut())
        };
        (b.Width, b.Height)
    }
}

impl Brush {
    pub fn raw(&self) -> *mut GpBrush {
        self.0
    }
    /// Hands the brush over (the C++ wraps and deletes it).
    pub fn into_raw(self) -> *mut GpBrush {
        let b = self.0;
        std::mem::forget(self);
        b
    }
    /// LinearGradientBrush(RectF, color1, color2, angle, isAngleScalable): WrapModeTile.
    pub fn linear_angle(r: RectF, c1: u32, c2: u32, angle: f32, scalable: bool) -> Brush {
        startup();
        let mut b = std::ptr::null_mut();
        unsafe { GdipCreateLineBrushFromRectWithAngle(&r, c1, c2, angle, scalable, WrapModeTile, &mut b) };
        Brush(b as *mut GpBrush)
    }
    /// LinearGradientBrush::SetInterpolationColors
    pub fn line_blend(&self, cols: &[u32], pos: &[f32]) {
        unsafe { GdipSetLinePresetBlend(self.0 as *mut GpLineGradient, cols.as_ptr(), pos.as_ptr(), cols.len() as i32) };
    }
    pub fn line_wrap(&self, w: WrapMode) {
        unsafe { GdipSetLineWrapMode(self.0 as *mut GpLineGradient, w) };
    }
    /// PathGradientBrush(const GraphicsPath*)
    pub fn path_gradient(path: &Path) -> Brush {
        startup();
        let mut b = std::ptr::null_mut();
        unsafe { GdipCreatePathGradientFromPath(path.raw, &mut b) };
        Brush(b as *mut GpBrush)
    }
    /// PathGradientBrush::SetInterpolationColors
    pub fn path_blend(&self, cols: &[u32], pos: &[f32]) {
        unsafe { GdipSetPathGradientPresetBlend(self.0 as *mut GpPathGradient, cols.as_ptr(), pos.as_ptr(), cols.len() as i32) };
    }
    pub fn path_center(&self, c: PointF) {
        unsafe { GdipSetPathGradientCenterPoint(self.0 as *mut GpPathGradient, &c) };
    }
}

impl Pen {
    /// Pen(const Brush*, width): UnitWorld.
    pub fn with_brush(brush: &Brush, width: f32) -> Pen {
        startup();
        let mut p = std::ptr::null_mut();
        unsafe { GdipCreatePen2(brush.0, width, UnitWorld, &mut p) };
        Pen(p, true)
    }
    /// A pen a caller made (C++ `Pen::nativePen`): used, never deleted.
    ///
    /// # Safety
    /// `raw` must be a live GpPen for the life of the result.
    pub unsafe fn borrow(raw: *mut core::ffi::c_void) -> Pen {
        Pen(raw as *mut GpPen, false)
    }
    pub fn line_join(&self, j: LineJoin) {
        unsafe { GdipSetPenLineJoin(self.0, j) };
    }
    pub fn start_cap(&self, c: LineCap) {
        unsafe { GdipSetPenStartCap(self.0, c) };
    }
    pub fn end_cap(&self, c: LineCap) {
        unsafe { GdipSetPenEndCap(self.0, c) };
    }
    pub fn set_dash_cap(&self, c: DashCap) {
        unsafe { GdipSetPenDashCap197819(self.0, c) };
    }
    pub fn dash_cap(&self) -> DashCap {
        let mut c = DashCap(0);
        unsafe { GdipGetPenDashCap197819(self.0, &mut c) };
        c
    }
    pub fn dash_pattern(&self, d: &[f32]) {
        unsafe { GdipSetPenDashArray(self.0, d.as_ptr(), d.len() as i32) };
    }
    pub fn width(&self) -> f32 {
        let mut w = 0.0f32;
        unsafe { GdipGetPenWidth(self.0, &mut w) };
        w
    }
    pub fn color(&self) -> u32 {
        let mut c = 0u32;
        unsafe { GdipGetPenColor(self.0, &mut c) };
        c
    }
}

/// GraphicsPath (FillModeAlternate when made here).
pub struct Path {
    raw: *mut GpPath,
    owned: bool,
}

impl Path {
    pub fn new() -> Path {
        startup();
        let mut raw = std::ptr::null_mut();
        unsafe { GdipCreatePath(FillModeAlternate, &mut raw) };
        Path { raw, owned: true }
    }
    /// A path a caller made (C++ `GraphicsPath::nativePath`): added to, never deleted.
    ///
    /// # Safety
    /// `raw` must be a live GpPath for the life of the result.
    pub unsafe fn borrow(raw: *mut core::ffi::c_void) -> Path {
        Path { raw: raw as *mut GpPath, owned: false }
    }
    pub fn raw(&self) -> *mut GpPath {
        self.raw
    }
    pub fn fill_mode(&self, m: FillMode) {
        unsafe { GdipSetPathFillMode(self.raw, m) };
    }
    pub fn arc(&self, x: f32, y: f32, w: f32, h: f32, start: f32, sweep: f32) {
        unsafe { GdipAddPathArc(self.raw, x, y, w, h, start, sweep) };
    }
    pub fn rectangle(&self, r: RectF) {
        unsafe { GdipAddPathRectangle(self.raw, r.X, r.Y, r.Width, r.Height) };
    }
    pub fn ellipse(&self, x: f32, y: f32, w: f32, h: f32) {
        unsafe { GdipAddPathEllipse(self.raw, x, y, w, h) };
    }
    pub fn ellipse_r(&self, r: RectF) {
        self.ellipse(r.X, r.Y, r.Width, r.Height);
    }
    pub fn polygon(&self, pts: &[PointF]) {
        unsafe { GdipAddPathPolygon(self.raw, pts.as_ptr(), pts.len() as i32) };
    }
    pub fn line(&self, a: PointF, b: PointF) {
        unsafe { GdipAddPathLine(self.raw, a.X, a.Y, b.X, b.Y) };
    }
    pub fn bezier(&self, a: PointF, b: PointF, c: PointF, d: PointF) {
        unsafe { GdipAddPathBezier(self.raw, a.X, a.Y, b.X, b.Y, c.X, c.Y, d.X, d.Y) };
    }
    pub fn closed_curve(&self, pts: &[PointF], tension: f32) {
        unsafe { GdipAddPathClosedCurve2(self.raw, pts.as_ptr(), pts.len() as i32, tension) };
    }
    pub fn add_path(&self, other: &Path, connect: bool) {
        unsafe { GdipAddPathPath(self.raw, other.raw, connect) };
    }
    /// AddString(text, length, family, style, em, RectF layout, format); a PointF origin is a 0×0 layout.
    #[allow(clippy::too_many_arguments)]
    pub fn string(&self, text: &[u16], len: i32, family: &FontFamily, style: i32, em: f32, layout: RectF, fmt: &StringFormat) {
        let t = wz(text);
        unsafe { GdipAddPathString(self.raw, PCWSTR(t.as_ptr()), len, family.0, style, em, &layout, fmt.0) };
    }
    pub fn close_figure(&self) {
        unsafe { GdipClosePathFigure(self.raw) };
    }
    pub fn start_figure(&self) {
        unsafe { GdipStartPathFigure(self.raw) };
    }
    pub fn transform(&self, m: &Matrix) {
        unsafe { GdipTransformPath(self.raw, m.0) };
    }
    /// Flatten(nullptr, flatness)
    pub fn flatten(&self, flatness: f32) {
        unsafe { GdipFlattenPath(self.raw, std::ptr::null_mut(), flatness) };
    }
    /// Outline(): FlatnessDefault (1/4)
    pub fn outline(&self) {
        unsafe { GdipWindingModeOutline(self.raw, std::ptr::null_mut(), 0.25) };
    }
    /// GetBounds(&RectF)
    pub fn bounds(&self) -> RectF {
        let mut r = RectF::default();
        unsafe { GdipGetPathWorldBounds(self.raw, &mut r, std::ptr::null(), std::ptr::null()) };
        r
    }
    /// GetPathData: the points and their types.
    pub fn data(&self) -> (Vec<PointF>, Vec<u8>) {
        let mut n = 0i32;
        unsafe { GdipGetPointCount(self.raw, &mut n) };
        if n <= 0 {
            return (Vec::new(), Vec::new());
        }
        let mut pts = vec![PointF::default(); n as usize];
        let mut types = vec![0u8; n as usize];
        unsafe {
            GdipGetPathPoints(self.raw, pts.as_mut_ptr(), n);
            GdipGetPathTypes(self.raw, &mut types);
        }
        (pts, types)
    }
    /// IsVisible(PointF)
    pub fn is_visible(&self, p: PointF) -> bool {
        let mut b = windows::core::BOOL(0);
        unsafe { GdipIsVisiblePathPoint(self.raw, p.X, p.Y, std::ptr::null_mut(), &mut b) };
        b.as_bool()
    }
    /// IsOutlineVisible(PointF, pen)
    pub fn is_outline_visible(&self, p: PointF, pen: &Pen) -> bool {
        let mut b = windows::core::BOOL(0);
        unsafe { GdipIsOutlineVisiblePathPoint(self.raw, p.X, p.Y, pen.0, std::ptr::null_mut(), &mut b) };
        b.as_bool()
    }
}

impl Drop for Path {
    fn drop(&mut self) {
        if self.owned && !self.raw.is_null() {
            unsafe { GdipDeletePath(self.raw) };
        }
    }
}

pub struct Matrix(*mut windows::Win32::Graphics::GdiPlus::Matrix);

impl Matrix {
    pub fn new() -> Matrix {
        startup();
        let mut m = std::ptr::null_mut();
        unsafe { GdipCreateMatrix(&mut m) };
        Matrix(m)
    }
    pub fn translate(&self, dx: f32, dy: f32) {
        unsafe { GdipTranslateMatrix(self.0, dx, dy, MatrixOrderPrepend) };
    }
    pub fn rotate(&self, degrees: f32) {
        unsafe { GdipRotateMatrix(self.0, degrees, MatrixOrderPrepend) };
    }
    /// RotateAt(angle, center) (MatrixOrderPrepend: translate, rotate, translate back)
    pub fn rotate_at(&self, degrees: f32, c: PointF) {
        self.translate(c.X, c.Y);
        self.rotate(degrees);
        self.translate(-c.X, -c.Y);
    }
}

impl Drop for Matrix {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { GdipDeleteMatrix(self.0) };
        }
    }
}

/// FontFamily(name): null when Windows does not have it (IsAvailable).
pub struct FontFamily(*mut GpFontFamily);

impl FontFamily {
    pub fn new(name: &str) -> FontFamily {
        startup();
        let n: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        let mut f = std::ptr::null_mut();
        unsafe { GdipCreateFontFamilyFromName(PCWSTR(n.as_ptr()), std::ptr::null_mut(), &mut f) };
        FontFamily(f)
    }
    pub fn available(&self) -> bool {
        !self.0.is_null()
    }
    pub fn line_spacing(&self, style: i32) -> u16 {
        let mut v = 0u16;
        unsafe { GdipGetLineSpacing(self.0, style, &mut v) };
        v
    }
    pub fn em_height(&self, style: i32) -> u16 {
        let mut v = 0u16;
        unsafe { GdipGetEmHeight(self.0, style, &mut v) };
        v
    }
    pub fn cell_ascent(&self, style: i32) -> u16 {
        let mut v = 0u16;
        unsafe { GdipGetCellAscent(self.0, style, &mut v) };
        v
    }
}

impl Drop for FontFamily {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { GdipDeleteFontFamily(self.0) };
        }
    }
}

impl Font {
    /// Font(&family, em, style, UnitPixel)
    pub fn of(family: &FontFamily, em: f32, style: i32) -> Font {
        let mut font = std::ptr::null_mut();
        unsafe { GdipCreateFont(family.0, em, style, UnitPixel, &mut font) };
        Font { family: std::ptr::null_mut(), font }
    }
}

impl StringFormat {
    /// StringFormat(StringFormat::GenericTypographic())
    pub fn typographic() -> StringFormat {
        startup();
        let (mut g, mut f) = (std::ptr::null_mut(), std::ptr::null_mut());
        unsafe {
            GdipStringFormatGetGenericTypographic(&mut g);
            GdipCloneStringFormat(g, &mut f);
        }
        StringFormat(f)
    }
    /// StringFormat(const StringFormat*)
    pub fn copy(&self) -> StringFormat {
        let mut f = std::ptr::null_mut();
        unsafe { GdipCloneStringFormat(self.0, &mut f) };
        StringFormat(f)
    }
    pub fn flags(&self) -> i32 {
        let mut v = 0i32;
        unsafe { GdipGetStringFormatFlags(self.0, &mut v) };
        v
    }
    pub fn set_flags(&self, f: i32) {
        unsafe { GdipSetStringFormatFlags(self.0, f) };
    }
}
