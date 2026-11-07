//! The editor's objects (Document.h / Document.cpp): the drawing objects and their style, with the geometry the
//! painters and the editor ask for (rectangle, bounds, effective shape, step labels, arrowhead length).

pub use windows::Win32::Graphics::GdiPlus::{PointF, RectF};

use crate::gdip::{pt, rect};
use crate::render;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ObjType {
    Arrow,
    Line,
    #[default]
    Rect,
    Ellipse,
    RoundRect,
    Pen,
    Highlighter,
    Text,
    Callout,
    Step,
    Stamp,
    Blur,
    Pixelate,
    Spotlight,
    Magnify,
    Picture,
    Shape,
    Group,
    /// Clickable area found by UI Automation when capturing (URL in `text`); not drawn, kept in PDF exports.
    Link,
}

impl ObjType {
    pub const ALL: [ObjType; 19] = [
        ObjType::Arrow, ObjType::Line, ObjType::Rect, ObjType::Ellipse, ObjType::RoundRect, ObjType::Pen, ObjType::Highlighter,
        ObjType::Text, ObjType::Callout, ObjType::Step, ObjType::Stamp, ObjType::Blur, ObjType::Pixelate, ObjType::Spotlight,
        ObjType::Magnify, ObjType::Picture, ObjType::Shape, ObjType::Group, ObjType::Link,
    ];
    pub fn from_i32(v: i32) -> ObjType {
        Self::ALL.get(v.max(0) as usize).copied().unwrap_or_default()
    }
}

// line and arrow ends (Cap); 0 and 1 are the old "no arrow" / "arrow"
pub const CAP_FLAT: i32 = 0;
pub const CAP_ARROW: i32 = 1;
pub const CAP_ARROW_WIDE: i32 = 2;
pub const CAP_ARROW_OPEN: i32 = 3;
pub const CAP_ROUND: i32 = 4;
pub const CAP_DOT: i32 = 5;
pub const CAP_SQUARE: i32 = 6;
pub const CAP_DIAMOND: i32 = 7;
pub const CAP_COUNT: i32 = 8;
// dash patterns (Dash)
pub const DASH_SOLID: i32 = 0;
pub const DASH_DASH: i32 = 1;
pub const DASH_DOT: i32 = 2;
pub const DASH_DASH_DOT: i32 = 3;
pub const DASH_DASH_DOT_DOT: i32 = 4;
pub const DASH_LONG: i32 = 5;
pub const DASH_COUNT: i32 = 6;
// shapes of the Shape tool (ShapeKind)
pub const SHAPE_RECT: i32 = 0;
pub const SHAPE_ROUND_RECT: i32 = 1;
pub const SHAPE_ELLIPSE: i32 = 2;
pub const SHAPE_TRIANGLE: i32 = 3;
pub const SHAPE_DIAMOND: i32 = 4;
pub const SHAPE_PARALLELOGRAM: i32 = 5;
pub const SHAPE_HEXAGON: i32 = 6;
pub const SHAPE_STAR: i32 = 7;
pub const SHAPE_COUNT: i32 = 8;
pub const CALLOUT_SHAPES: i32 = 24;
// step numbering (StepType)
pub const STEP_NUMBERS: i32 = 0;
pub const STEP_UPPER: i32 = 1;
pub const STEP_LOWER: i32 = 2;
pub const STEP_ROMAN: i32 = 3;
pub const STEP_ROMAN_LOWER: i32 = 4;
pub const STEP_TYPE_COUNT: i32 = 5;
// area shapes of the highlighter, spotlight and hide tools: the ShapeKind outlines, a clicked polygon and a freehand line
pub const AREA_POLYGON: i32 = SHAPE_COUNT;
pub const AREA_FREEHAND: i32 = SHAPE_COUNT + 1;
pub const AREA_SHAPE_COUNT: i32 = SHAPE_COUNT + 2;
pub const MAGNIFY_KIND_COUNT: i32 = 6;

pub fn is_point_area(area_shape: i32) -> bool {
    area_shape == AREA_POLYGON || area_shape == AREA_FREEHAND
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradStop {
    pub pos: f32,
    pub color: u32,
}

/// Gradient fill: linear at an angle or radial, two to four stops sorted by position.
#[derive(Clone, Debug, PartialEq)]
pub struct Gradient {
    pub kind: i32, // 0 none (solid color), 1 linear, 2 radial
    pub angle: f32,
    pub stops: Vec<GradStop>,
}

impl Default for Gradient {
    fn default() -> Self {
        Gradient { kind: 0, angle: 90.0, stops: Vec::new() }
    }
}

impl Gradient {
    pub fn active(&self) -> bool {
        self.kind > 0 && self.stops.len() >= 2
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    pub stroke: u32,
    pub fill: u32,
    pub text_color: u32,
    pub width: f32,
    pub opacity: i32,
    pub shadow: bool,
    pub shadow_dir: i32,
    pub shadow_color: u32,
    pub shadow_opacity: i32,
    pub shadow_blur: i32,
    pub shadow_dist: i32,
    pub font: String,
    pub font_size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub align_h: i32,
    pub align_v: i32,
    pub padding: f32,
    pub arrow_start: i32,
    pub arrow_end: i32,
    pub start_size: f32,
    pub end_size: f32,
    pub dash: i32,
    pub bezier: bool,
    pub callout_shape: i32,
    pub shaded: bool,
    pub tail_color: u32,
    pub tail_width: f32,
    pub shape_kind: i32,
    pub step_shape: i32,
    pub step_type: i32,
    pub magnify_shape: i32,
    pub magnify_scale: i32,
    pub magnify_link: bool,
    pub hide_kind: i32,
    pub antialias: bool,
    pub amount: i32,
    pub line_spacing: f32,
    pub step_outline: u32,
    pub step_outline_width: f32,
    pub area_shape: i32,
    pub grad: Gradient,
    pub bg_blur: bool,
    pub bg_blur_amount: i32,
    pub loupe: bool,
    pub loupe_handle: bool,
    pub text_only: bool,
    pub snap_text: bool,
    pub sketch: bool,
    pub sketch_seed: i32,
    pub vertical: bool,
    pub text_on_path: bool,
}

impl Default for Style {
    fn default() -> Self {
        Style {
            stroke: 0xFFE5322D,
            fill: 0,
            text_color: 0xFF000000,
            width: 4.0,
            opacity: 100,
            shadow: true,
            shadow_dir: 8,
            shadow_color: 0xFF000000,
            shadow_opacity: 45,
            shadow_blur: 6,
            shadow_dist: 5,
            font: "Segoe UI".into(),
            font_size: 28.0,
            bold: true,
            italic: false,
            underline: false,
            align_h: 1,
            align_v: 1,
            padding: 5.0,
            arrow_start: CAP_FLAT,
            arrow_end: CAP_ARROW,
            start_size: 3.0,
            end_size: 3.0,
            dash: DASH_SOLID,
            bezier: false,
            callout_shape: 13,
            shaded: false,
            tail_color: 0xFFE5322D,
            tail_width: 6.0,
            shape_kind: SHAPE_RECT,
            step_shape: 0,
            step_type: STEP_NUMBERS,
            magnify_shape: 0,
            magnify_scale: 200,
            magnify_link: false,
            hide_kind: 0,
            antialias: true,
            amount: 10,
            line_spacing: 1.0,
            step_outline: 0xFFFFFFFF,
            step_outline_width: 0.0,
            area_shape: SHAPE_RECT,
            grad: Gradient::default(),
            bg_blur: false,
            bg_blur_amount: 8,
            loupe: false,
            loupe_handle: true,
            text_only: false,
            snap_text: false,
            sketch: false,
            sketch_seed: 1,
            vertical: false,
            text_on_path: false,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct DrawObj {
    pub kind: ObjType,
    pub p1: PointF,
    pub p2: PointF,
    pub pts: Vec<PointF>,
    pub tail: PointF,
    pub ctrl: PointF,
    /// UTF-16, as the Windows text calls take it (one unit at a time along a path).
    pub text: Vec<u16>,
    pub step_value: i32,
    pub style: Style,
    pub picture: Option<crate::image::Image>,
    pub children: Vec<DrawObj>,
    pub name: String,
    pub hidden: bool,
    pub locked: bool,
    pub floating: bool,
    pub text_boxes: Vec<RectF>,
    pub pressure: Vec<f32>,
}

// ---------------------------------------------------------------- RectF and PointF helpers (Gdiplus::RectF methods)
pub fn right(r: &RectF) -> f32 {
    r.X + r.Width
}
pub fn bottom(r: &RectF) -> f32 {
    r.Y + r.Height
}
pub fn inflate(r: &mut RectF, dx: f32, dy: f32) {
    r.X -= dx;
    r.Y -= dy;
    r.Width += 2.0 * dx;
    r.Height += 2.0 * dy;
}
pub fn contains(r: &RectF, p: PointF) -> bool {
    p.X >= r.X && p.X < r.X + r.Width && p.Y >= r.Y && p.Y < r.Y + r.Height
}
/// RectF::Union
pub fn union(a: &RectF, b: &RectF) -> RectF {
    let r = right(a).max(right(b));
    let bt = bottom(a).max(bottom(b));
    let l = a.X.min(b.X);
    let t = a.Y.min(b.Y);
    rect(l, t, r - l, bt - t)
}

fn grow(r: &mut RectF, p: PointF) {
    let x0 = r.X.min(p.X);
    let y0 = r.Y.min(p.Y);
    let x1 = right(r).max(p.X);
    let y1 = bottom(r).max(p.Y);
    *r = rect(x0, y0, x1 - x0, y1 - y0);
}

impl DrawObj {
    pub fn new(kind: ObjType) -> DrawObj {
        DrawObj { kind, step_value: 1, ..Default::default() }
    }

    /// The normalized rectangle (p1..p2).
    pub fn rect(&self) -> RectF {
        rect(self.p1.X.min(self.p2.X), self.p1.Y.min(self.p2.Y), (self.p2.X - self.p1.X).abs(), (self.p2.Y - self.p1.Y).abs())
    }
    pub fn is_line(&self) -> bool {
        matches!(self.kind, ObjType::Arrow | ObjType::Line)
    }
    pub fn is_path(&self) -> bool {
        self.kind == ObjType::Pen || (self.kind == ObjType::Highlighter && !self.pts.is_empty() && !is_point_area(self.style.area_shape))
    }
    pub fn is_area_tool(&self) -> bool {
        matches!(self.kind, ObjType::Highlighter | ObjType::Spotlight | ObjType::Blur | ObjType::Pixelate)
    }
    /// Highlighter, spotlight or hide whose outline is the clicked polygon / freehand line in `pts`.
    pub fn is_point_shape(&self) -> bool {
        self.is_area_tool() && is_point_area(self.style.area_shape) && self.pts.len() >= 2
    }
    pub fn has_text(&self) -> bool {
        matches!(self.kind, ObjType::Text | ObjType::Callout)
    }
    /// Arrow with a label at its tail; a line gets one when its text follows the line (C37).
    pub fn has_label(&self) -> bool {
        self.kind == ObjType::Arrow || (self.kind == ObjType::Line && self.style.text_on_path)
    }
    pub fn is_shape(&self) -> bool {
        matches!(self.kind, ObjType::Shape | ObjType::Rect | ObjType::Ellipse | ObjType::RoundRect)
    }
    /// The effective shape (ShapeKind), also for the old Rect / Ellipse / RoundRect objects.
    pub fn shape_kind(&self) -> i32 {
        match self.kind {
            ObjType::Ellipse => SHAPE_ELLIPSE,
            ObjType::RoundRect => SHAPE_ROUND_RECT,
            ObjType::Rect => SHAPE_RECT,
            _ => self.style.shape_kind.clamp(0, SHAPE_COUNT - 1),
        }
    }
    /// Callout shapes with a draggable tip.
    pub fn callout_has_tail(&self) -> bool {
        !(16..=21).contains(&self.style.callout_shape)
    }

    /// The area drawn: stroke, ends, callout tip, label and shadow included.
    pub fn bounds(&self) -> RectF {
        if self.kind == ObjType::Group {
            if self.children.is_empty() {
                return self.rect();
            }
            let mut u = self.children[0].bounds();
            for c in &self.children {
                u = union(&u, &c.bounds());
            }
            return u;
        }
        let s = &self.style;
        let mut r = if (self.is_path() || self.is_point_shape()) && !self.pts.is_empty() {
            let (mut x0, mut y0) = (self.pts[0].X, self.pts[0].Y);
            let (mut x1, mut y1) = (x0, y0);
            for p in &self.pts {
                x0 = x0.min(p.X);
                y0 = y0.min(p.Y);
                x1 = x1.max(p.X);
                y1 = y1.max(p.Y);
            }
            rect(x0, y0, x1 - x0, y1 - y0)
        } else {
            self.rect()
        };
        let mut pad = s.width / 2.0;
        match self.kind {
            ObjType::Step => {
                let d = s.font_size * 1.5;
                r = rect(self.p1.X - d / 2.0, self.p1.Y - d / 2.0, d, d);
                if s.step_shape == 1 {
                    r.Width += d * 0.45;
                }
                pad = if s.step_outline_width > 0.0 { s.step_outline_width / 2.0 + 1.0 } else { 0.0 };
            }
            ObjType::Callout => {
                if self.callout_has_tail() {
                    grow(&mut r, self.tail);
                }
                pad = s.width.max(s.tail_width) / 2.0 + cap_length(s.tail_width, s.end_size);
            }
            ObjType::Arrow | ObjType::Line => {
                if s.bezier {
                    grow(&mut r, self.ctrl);
                }
                pad = s.width / 2.0 + cap_length(s.width, s.start_size.max(s.end_size));
                if self.has_label() && !self.text.is_empty() && s.text_on_path {
                    let m = s.font_size * 1.6 + s.width;
                    inflate(&mut r, m, m);
                } else if self.has_label() && !self.text.is_empty() {
                    let mut lb = render::arrow_label_rect(self);
                    inflate(&mut lb, s.width + 2.0 - pad, s.width + 2.0 - pad);
                    r = union(&r, &lb);
                }
            }
            ObjType::Magnify => {
                if s.magnify_link {
                    let k = 1.1f32.max(s.magnify_scale as f32 / 100.0);
                    let sw = r.Width / k / 2.0;
                    let sh = r.Height / k / 2.0;
                    grow(&mut r, pt(self.tail.X - sw, self.tail.Y - sh));
                    grow(&mut r, pt(self.tail.X + sw, self.tail.Y + sh));
                }
                if s.loupe {
                    let ex = render::magnifier_extra(self);
                    r = union(&r, &ex);
                    let rc = self.rect();
                    let rim = 4.0f32.max(rc.Width.min(rc.Height) * 0.055 + s.width * 0.5);
                    pad = pad.max(rim);
                }
            }
            ObjType::Text => pad = s.width,
            ObjType::Highlighter => {
                if self.pts.is_empty() || self.is_point_shape() {
                    pad = 0.0;
                }
            }
            ObjType::Stamp | ObjType::Picture | ObjType::Link => pad = 0.0,
            _ => {}
        }
        if s.shadow
            && !matches!(self.kind, ObjType::Highlighter | ObjType::Blur | ObjType::Pixelate | ObjType::Spotlight | ObjType::Link)
        {
            pad += (s.shadow_blur * 2 + s.shadow_dist + 2) as f32;
        }
        inflate(&mut r, pad, pad);
        r
    }

    /// Group: p1..p2 = the area its children cover.
    pub fn update_group_rect(&mut self) {
        if self.kind != ObjType::Group || self.children.is_empty() {
            return;
        }
        let mut u = self.children[0].bounds();
        for c in &self.children {
            u = union(&u, &c.bounds());
        }
        self.p1 = pt(u.X, u.Y);
        self.p2 = pt(right(&u), bottom(&u));
    }
}

// ---------------------------------------------------------------- hit-testing, moving and scaling (Document.cpp)
fn dist_seg(p: PointF, a: PointF, b: PointF) -> f32 {
    let dx = b.X - a.X;
    let dy = b.Y - a.Y;
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 { (((p.X - a.X) * dx + (p.Y - a.Y) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
    let qx = a.X + t * dx - p.X;
    let qy = a.Y + t * dy - p.Y;
    (qx * qx + qy * qy).sqrt()
}

/// Quadratic curve a-c-b, sampled.
fn dist_quad(p: PointF, a: PointF, c: PointF, b: PointF) -> f32 {
    let mut best = 1e9f32;
    let mut prev = a;
    for i in 1..=24 {
        let t = i as f32 / 24.0;
        let u = 1.0 - t;
        let q = pt(u * u * a.X + 2.0 * u * t * c.X + t * t * b.X, u * u * a.Y + 2.0 * u * t * c.Y + t * t * b.Y);
        best = best.min(dist_seg(p, prev, q));
        prev = q;
    }
    best
}

fn area_hit(o: &DrawObj, p: PointF, tol: f32) -> bool {
    let gp = crate::gdip::Path::new();
    render::area_path(&gp, o);
    let pen = crate::gdip::Pen::new(0xFF000000, (tol * 2.0).max(2.0));
    gp.is_visible(p) || gp.is_outline_visible(p, &pen)
}

fn inflated_contains(mut r: RectF, d: f32, p: PointF) -> bool {
    inflate(&mut r, d, d);
    contains(&r, p)
}

impl DrawObj {
    /// Whether the point is on the object, `tol` pixels around it counting.
    pub fn hit_test(&self, p: PointF, tol: f32) -> bool {
        let t = tol + self.style.width / 2.0;
        match self.kind {
            ObjType::Group => self.children.iter().any(|c| c.hit_test(p, tol)),
            ObjType::Arrow | ObjType::Line => {
                if self.has_label() && !self.text.is_empty() && inflated_contains(render::arrow_label_rect(self), tol, p) {
                    return true;
                }
                let d = if self.style.bezier { dist_quad(p, self.p1, self.ctrl, self.p2) } else { dist_seg(p, self.p1, self.p2) };
                d <= t + 2.0
            }
            ObjType::Pen | ObjType::Highlighter => {
                if self.kind == ObjType::Highlighter && (self.pts.is_empty() || self.is_point_shape()) {
                    if self.style.area_shape == SHAPE_RECT && !self.is_point_shape() {
                        return inflated_contains(self.rect(), tol, p);
                    }
                    return area_hit(self, p, tol);
                }
                if self.pts.windows(2).any(|w| dist_seg(p, w[0], w[1]) <= t) {
                    return true;
                }
                self.pts.len() == 1 && dist_seg(p, self.pts[0], self.pts[0]) <= t
            }
            ObjType::Rect | ObjType::RoundRect | ObjType::Ellipse | ObjType::Shape => {
                let gp = crate::gdip::Path::new();
                render::shape_path(&gp, self.shape_kind(), self.rect());
                if (self.style.fill >> 24) != 0 && gp.is_visible(p) {
                    return true;
                }
                let pen = crate::gdip::Pen::new(0xFF000000, (t * 2.0).max(2.0));
                gp.is_outline_visible(p, &pen)
            }
            ObjType::Callout => {
                if inflated_contains(self.rect(), tol, p) {
                    return true;
                }
                if !self.callout_has_tail() {
                    return false;
                }
                let c = self.rect();
                dist_seg(p, self.tail, pt(c.X + c.Width / 2.0, c.Y + c.Height / 2.0)) <= t.max(self.style.tail_width / 2.0 + tol) + 6.0
            }
            ObjType::Magnify => {
                let mut r = self.rect();
                inflate(&mut r, tol, tol);
                if contains(&r, p) {
                    return true;
                }
                self.style.magnify_link
                    && dist_seg(p, self.tail, self.tail) <= 12f32.max(r.Width / 1.1f32.max(self.style.magnify_scale as f32 / 100.0) / 2.0)
            }
            ObjType::Blur | ObjType::Pixelate | ObjType::Spotlight => {
                if self.style.area_shape == SHAPE_RECT && !self.is_point_shape() {
                    return inflated_contains(self.rect(), tol, p);
                }
                area_hit(self, p, tol)
            }
            _ => inflated_contains(self.bounds(), tol, p),
        }
    }

    /// Moves every point of the object (and of its children).
    pub fn move_by(&mut self, dx: f32, dy: f32) {
        self.map_points(&|p: PointF| pt(p.X + dx, p.Y + dy));
    }

    /// Maps the object from one rectangle to another (resizing a group or the selection).
    pub fn scale(&mut self, from: RectF, to: RectF) {
        self.map_points(&|p: PointF| {
            let u = if from.Width > 0.01 { (p.X - from.X) / from.Width } else { 0.0 };
            let v = if from.Height > 0.01 { (p.Y - from.Y) / from.Height } else { 0.0 };
            pt(to.X + u * to.Width, to.Y + v * to.Height)
        });
    }

    /// Every point of the object through `f` (rotating or resizing the image); the text boxes stay upright.
    pub fn map_points(&mut self, f: &dyn Fn(PointF) -> PointF) {
        self.p1 = f(self.p1);
        self.p2 = f(self.p2);
        self.tail = f(self.tail);
        self.ctrl = f(self.ctrl);
        for p in &mut self.pts {
            *p = f(*p);
        }
        for r in &mut self.text_boxes {
            let a = f(pt(r.X, r.Y));
            let b = f(pt(right(r), bottom(r)));
            *r = rect(a.X.min(b.X), a.Y.min(b.Y), (b.X - a.X).abs(), (b.Y - a.Y).abs());
        }
        for c in &mut self.children {
            c.map_points(f);
        }
    }
}

/// The label of a step: number, letters (A…Z, AA…) or Roman numerals.
pub fn step_label(v: i32, kind: i32) -> String {
    let v = v.max(1);
    let letters = |mut n: i32, a: u8| {
        let mut s = Vec::new();
        while n > 0 {
            n -= 1;
            s.insert(0, a + (n % 26) as u8);
            n /= 26;
        }
        String::from_utf8(s).unwrap_or_default()
    };
    let roman = |mut n: i32, lower: bool| {
        const VAL: [i32; 13] = [1000, 900, 500, 400, 100, 90, 50, 40, 10, 9, 5, 4, 1];
        const SYM: [&str; 13] = ["M", "CM", "D", "CD", "C", "XC", "L", "XL", "X", "IX", "V", "IV", "I"];
        let mut s = String::new();
        for i in 0..13 {
            while n >= VAL[i] {
                s += SYM[i];
                n -= VAL[i];
            }
        }
        if lower { s.to_lowercase() } else { s }
    };
    match kind {
        STEP_UPPER => letters(v, b'A'),
        STEP_LOWER => letters(v, b'a'),
        STEP_ROMAN => roman(v.min(3999), false),
        STEP_ROMAN_LOWER => roman(v.min(3999), true),
        _ => v.to_string(),
    }
}

/// Arrowhead length for the thickness and size.
pub fn cap_length(w: f32, size: f32) -> f32 {
    8.0f32.max(w * (1.0 + size.max(1.0) * 0.6))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_and_bounds() {
        assert_eq!(step_label(28, STEP_UPPER), "AB");
        assert_eq!(step_label(703, STEP_LOWER), "aaa");
        assert_eq!(step_label(1994, STEP_ROMAN), "MCMXCIV");
        assert_eq!(step_label(0, STEP_NUMBERS), "1");
        let mut o = DrawObj::new(ObjType::Shape);
        o.p1 = pt(50.0, 40.0);
        o.p2 = pt(10.0, 20.0);
        o.style.shadow = false;
        let b = o.bounds();
        assert_eq!((b.X, b.Y, b.Width, b.Height), (8.0, 18.0, 44.0, 24.0));
    }
}
