//! The object painters (Render.cpp): each vector object is drawn on its own layer with full colors; the shadow (color,
//! blur and offset in the chosen direction) comes from that layer's alpha, and both are composited with the object's
//! opacity, so overlapping parts (line + end, body + callout tip) do not get darker. Every GDI+ call is the one the C++
//! made, in the same order and with the same float arithmetic, so the pixels come out the same.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use windows::Win32::Graphics::GdiPlus::*;

use crate::doc::*;
use crate::fx;
use crate::gdip::{Bitmap, Brush, Font, FontFamily, Graphics, Matrix, Path, Pen, StringFormat, pt, rect};
use crate::image::{Image, Rect, argb, blend_over, ca, cb, cg, cr};
use crate::stamps;

const NEAR: StringAlignment = StringAlignmentNear;

/// Gc: the color with its alpha scaled by the opacity (%).
pub fn gc(c: u32, opacity: i32) -> u32 {
    let a = ((c >> 24) & 255) as i32 * opacity.clamp(0, 100) / 100;
    (a as u32) << 24 | (c & 0x00FFFFFF)
}

fn sub(a: PointF, b: PointF) -> PointF {
    pt(a.X - b.X, a.Y - b.Y)
}
fn add(a: PointF, b: PointF) -> PointF {
    pt(a.X + b.X, a.Y + b.Y)
}
fn mul(a: PointF, k: f32) -> PointF {
    pt(a.X * k, a.Y * k)
}
fn len(a: PointF) -> f32 {
    (a.X * a.X + a.Y * a.Y).sqrt()
}
fn norm(a: PointF) -> PointF {
    let l = len(a);
    if l > 1e-4 { pt(a.X / l, a.Y / l) } else { pt(1.0, 0.0) }
}
fn center(r: RectF) -> PointF {
    pt(r.X + r.Width / 2.0, r.Y + r.Height / 2.0)
}
/// FromColorref(Lighter(ToColorref(c)), a): each channel 2/5 of the way to white.
fn lighter(c: u32, a: i32) -> u32 {
    let l = |v: i32| v + (255 - v) * 2 / 5;
    argb(a, l(cr(c)), l(cg(c)), l(cb(c)))
}
fn font_style(s: &Style) -> i32 {
    (if s.bold { 1 } else { 0 }) | (if s.italic { 2 } else { 0 }) | (if s.underline { 4 } else { 0 })
}
fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn add_round_rect(gp: &Path, r: RectF, rad: f32) {
    let mut rad = rad;
    if r.Width / 2.0 < rad {
        rad = r.Width / 2.0;
    }
    if r.Height / 2.0 < rad {
        rad = r.Height / 2.0;
    }
    if rad < 1.0 {
        gp.rectangle(r);
        return;
    }
    let d = rad * 2.0;
    gp.arc(r.X, r.Y, d, d, 180.0, 90.0);
    gp.arc(right(&r) - d, r.Y, d, d, 270.0, 90.0);
    gp.arc(right(&r) - d, bottom(&r) - d, d, d, 0.0, 90.0);
    gp.arc(r.X, bottom(&r) - d, d, d, 90.0, 90.0);
    gp.close_figure();
}

pub fn apply_dash(p: &Pen, dash: i32) {
    match dash {
        DASH_DASH => p.dash_pattern(&[3.0, 2.0]),
        DASH_DOT => {
            p.dash_pattern(&[0.25, 1.9]);
            p.set_dash_cap(DashCapRound);
        }
        DASH_DASH_DOT => {
            p.dash_pattern(&[3.0, 1.6, 0.25, 1.6]);
            p.set_dash_cap(DashCapRound);
        }
        DASH_DASH_DOT_DOT => {
            p.dash_pattern(&[3.0, 1.5, 0.25, 1.5, 0.25, 1.5]);
            p.set_dash_cap(DashCapRound);
        }
        DASH_LONG => p.dash_pattern(&[6.0, 2.5]),
        _ => {}
    }
}

// ---------------------------------------------------------------- shapes
pub fn shape_path(gp: &Path, kind: i32, r: RectF) {
    let (l, t, w, h) = (r.X, r.Y, r.Width, r.Height);
    let (rr, b, cx, cy) = (l + w, t + h, l + w / 2.0, t + h / 2.0);
    match kind {
        SHAPE_ROUND_RECT => add_round_rect(gp, r, w.min(h) * 0.18),
        SHAPE_ELLIPSE => gp.ellipse_r(r),
        SHAPE_TRIANGLE => gp.polygon(&[pt(cx, t), pt(rr, b), pt(l, b)]),
        SHAPE_DIAMOND => gp.polygon(&[pt(cx, t), pt(rr, cy), pt(cx, b), pt(l, cy)]),
        SHAPE_PARALLELOGRAM => {
            let k = w * 0.22;
            gp.polygon(&[pt(l + k, t), pt(rr, t), pt(rr - k, b), pt(l, b)]);
        }
        SHAPE_HEXAGON => {
            let k = w * 0.25;
            gp.polygon(&[pt(l + k, t), pt(rr - k, t), pt(rr, cy), pt(rr - k, b), pt(l + k, b), pt(l, cy)]);
        }
        SHAPE_STAR => {
            let p: Vec<PointF> = (0..10)
                .map(|i| {
                    let a = -3.14159265f64 / 2.0 + i as f64 * 3.14159265 / 5.0;
                    let k = if i % 2 == 1 { 0.40f32 } else { 1.0 };
                    pt(cx + a.cos() as f32 * w / 2.0 * k, cy + a.sin() as f32 * h / 2.0 * k + h * 0.05)
                })
                .collect();
            gp.polygon(&p);
        }
        _ => gp.rectangle(r),
    }
}

// ---------------------------------------------------------------- gradients
fn grad_color_at(g: &Gradient, t: f32) -> u32 {
    let st = &g.stops;
    if st.is_empty() {
        return 0;
    }
    if t <= st[0].pos {
        return st[0].color;
    }
    if t >= st[st.len() - 1].pos {
        return st[st.len() - 1].color;
    }
    for i in 1..st.len() {
        if t > st[i].pos {
            continue;
        }
        let span = 1e-4f32.max(st[i].pos - st[i - 1].pos);
        let k = (t - st[i - 1].pos) / span;
        let (a, b) = (st[i - 1].color, st[i].color);
        let mix = |sh: u32| {
            let (ac, bc) = (((a >> sh) & 255) as i32, ((b >> sh) & 255) as i32);
            ((ac as f32 + (bc - ac) as f32 * k).round() as i64 as u32) << sh
        };
        return mix(24) | mix(16) | mix(8) | mix(0);
    }
    st[st.len() - 1].color
}

fn grad_arrays(g: &Gradient, opacity: i32, reverse: bool) -> (Vec<u32>, Vec<f32>) {
    let mut st = g.stops.clone();
    if st[0].pos > 0.0 {
        st.insert(0, GradStop { pos: 0.0, color: st[0].color });
    }
    if st[st.len() - 1].pos < 1.0 {
        let c = st[st.len() - 1].color;
        st.push(GradStop { pos: 1.0, color: c });
    }
    if reverse {
        st.reverse();
        for x in st.iter_mut() {
            x.pos = 1.0 - x.pos;
        }
    }
    let cols: Vec<u32> = st.iter().map(|x| gc(x.color, opacity)).collect();
    let mut pos: Vec<f32> = st.iter().map(|x| x.pos.clamp(0.0, 1.0)).collect();
    pos[0] = 0.0;
    let n = pos.len();
    pos[n - 1] = 1.0;
    (cols, pos)
}

/// The brush of a fill: solid color, or the linear (at an angle) / radial gradient spread over `r`.
pub fn fill_brush(color: u32, g: &Gradient, r: RectF, opacity: i32) -> Brush {
    if !g.active() || r.Width < 1.0 || r.Height < 1.0 {
        return Brush::solid(gc(color, opacity));
    }
    if g.kind == 2 {
        // radial: from the center (stop 0) to the corners (stop 1)
        let (cols, pos) = grad_arrays(g, opacity, true);
        let mut e = r;
        inflate(&mut e, r.Width * 0.2072, r.Height * 0.2072);
        let gp = Path::new();
        gp.ellipse_r(e);
        let b = Brush::path_gradient(&gp);
        b.path_center(center(r));
        b.path_blend(&cols, &pos);
        return b;
    }
    let (cols, pos) = grad_arrays(g, opacity, false);
    let mut e = r;
    inflate(&mut e, 1.0, 1.0);
    let b = Brush::linear_angle(e, cols[0], cols[cols.len() - 1], g.angle, true);
    b.line_blend(&cols, &pos);
    b.line_wrap(WrapModeTileFlipXY);
    b
}

/// A gradient painted (blended) on the pixels of the mask, spread over `b` (Fill tool, canvas color).
pub fn fill_gradient_mask(img: &mut Image, mask: &[u8], b: Rect, g: &Gradient, opacity: i32) {
    if !g.active() || b.right <= b.left || b.bottom <= b.top {
        return;
    }
    let (w, h) = ((b.right - b.left) as f32, (b.bottom - b.top) as f32);
    let (cx, cy) = (b.left as f32 + w / 2.0, b.top as f32 + h / 2.0);
    let a = g.angle as f64 * 3.14159265358979 / 180.0;
    let (ux, uy) = (a.cos() as f32, a.sin() as f32);
    let half = (ux.abs() * w + uy.abs() * h) / 2.0;
    let rad = (w * w + h * h).sqrt() / 2.0;
    for y in b.top.max(0)..b.bottom.min(img.hi()) {
        for x in b.left.max(0)..b.right.min(img.wi()) {
            let m = if mask.is_empty() { 255 } else { mask[y as usize * img.w as usize + x as usize] as i32 };
            if m == 0 {
                continue;
            }
            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            let t = if g.kind == 2 {
                (dx * dx + dy * dy).sqrt() / 1.0f32.max(rad)
            } else {
                (dx * ux + dy * uy) / 1.0f32.max(2.0 * half) + 0.5
            };
            let c = grad_color_at(g, t.clamp(0.0, 1.0));
            let al = ca(c) * opacity.clamp(0, 100) / 100 * m / 255;
            if al != 0 {
                let p = img.at_mut(x, y);
                *p = blend_over(*p, (c & 0x00FFFFFF) | (al as u32) << 24);
            }
        }
    }
}

/// The outline of a highlighter / spotlight / hide area: one of the shapes, or the clicked polygon / freehand outline.
pub fn area_path(gp: &Path, o: &DrawObj) {
    if o.is_point_shape() {
        if o.style.area_shape == AREA_FREEHAND && o.pts.len() >= 3 {
            gp.closed_curve(&o.pts, 0.3);
        } else {
            gp.polygon(&o.pts);
        }
        return;
    }
    // a polygon / freehand area without its points yet is drawn as a rectangle
    let k = if is_point_area(o.style.area_shape) { SHAPE_RECT } else { o.style.area_shape.clamp(0, SHAPE_COUNT - 1) };
    shape_path(gp, k, o.rect());
}

// ---------------------------------------------------------------- callouts
// 24 shapes, in gallery order: speech, clouds, box with arrow, arrows, rectangles, note, explosion, thought, whisper.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Body {
    RoundSpeech,
    Rect,
    RoundRect,
    Ellipse,
    Cloud,
    Note,
    Burst,
    Tag,
    Block,
    Notch,
    Fat,
    Double,
    Chevron,
    ArrowBox,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tail {
    None,
    Wedge,
    Bubbles,
    Leader,
    Direction,
}
struct CalloutDef(Body, Tail, bool);

const CALLOUTS: [CalloutDef; 24] = [
    CalloutDef(Body::RoundSpeech, Tail::Wedge, false), CalloutDef(Body::Rect, Tail::Wedge, false),
    CalloutDef(Body::Cloud, Tail::Wedge, false), CalloutDef(Body::Cloud, Tail::Bubbles, false),
    CalloutDef(Body::Rect, Tail::Leader, false), CalloutDef(Body::Tag, Tail::Direction, false),
    CalloutDef(Body::Block, Tail::Direction, false), CalloutDef(Body::Notch, Tail::Direction, false),
    CalloutDef(Body::Fat, Tail::Direction, false), CalloutDef(Body::Double, Tail::Direction, false),
    CalloutDef(Body::Chevron, Tail::Direction, false), CalloutDef(Body::ArrowBox, Tail::Direction, false),
    CalloutDef(Body::Rect, Tail::Wedge, false), CalloutDef(Body::RoundRect, Tail::Wedge, false),
    CalloutDef(Body::Ellipse, Tail::Wedge, false), CalloutDef(Body::RoundRect, Tail::Leader, false),
    CalloutDef(Body::Rect, Tail::None, false), CalloutDef(Body::RoundRect, Tail::None, false),
    CalloutDef(Body::Ellipse, Tail::None, false), CalloutDef(Body::Cloud, Tail::None, false),
    CalloutDef(Body::Note, Tail::None, false), CalloutDef(Body::Burst, Tail::None, false),
    CalloutDef(Body::Ellipse, Tail::Bubbles, false), CalloutDef(Body::Ellipse, Tail::Wedge, true),
];

fn def(shape: i32) -> &'static CalloutDef {
    &CALLOUTS[shape.clamp(0, CALLOUT_SHAPES - 1) as usize]
}
fn is_arrow_body(b: Body) -> bool {
    b >= Body::Tag
}

/// Arrow direction from the draggable tip: 0 right, 1 down, 2 left, 3 up.
fn arrow_dir(o: &DrawObj) -> i32 {
    let d = sub(o.tail, center(o.rect()));
    if d.X.abs() < 1.0 && d.Y.abs() < 1.0 {
        return 0;
    }
    if d.X.abs() >= d.Y.abs() {
        return if d.X >= 0.0 { 0 } else { 2 };
    }
    if d.Y >= 0.0 { 1 } else { 3 }
}

fn arrow_body_path(gp: &Path, body: Body, r: RectF, dir: i32) {
    let vert = dir == 1 || dir == 3;
    let (w, h) = if vert { (r.Height, r.Width) } else { (r.Width, r.Height) };
    let h2 = (h * 0.5).min(w * 0.35);
    let p: Vec<PointF> = match body {
        Body::Tag => vec![pt(0.0, 0.0), pt(w - h2, 0.0), pt(w, h / 2.0), pt(w - h2, h), pt(0.0, h)],
        Body::Notch => vec![pt(0.0, 0.0), pt(w - h2, 0.0), pt(w, h / 2.0), pt(w - h2, h), pt(0.0, h), pt(h2 * 0.6, h / 2.0)],
        Body::Chevron => vec![pt(0.0, 0.0), pt(w - h2, 0.0), pt(w, h / 2.0), pt(w - h2, h), pt(0.0, h), pt(h2, h / 2.0)],
        Body::Block => {
            let hd = (w * 0.4).min(h * 0.9);
            vec![pt(0.0, h * 0.25), pt(w - hd, h * 0.25), pt(w - hd, 0.0), pt(w, h / 2.0), pt(w - hd, h), pt(w - hd, h * 0.75),
                 pt(0.0, h * 0.75)]
        }
        Body::Fat => {
            let hd = (w * 0.5).min(h);
            vec![pt(0.0, h * 0.15), pt(w - hd, h * 0.15), pt(w - hd, 0.0), pt(w, h / 2.0), pt(w - hd, h), pt(w - hd, h * 0.85),
                 pt(0.0, h * 0.85)]
        }
        Body::Double => {
            let hd = (w * 0.28).min(h * 0.8);
            vec![pt(0.0, h / 2.0), pt(hd, 0.0), pt(hd, h * 0.25), pt(w - hd, h * 0.25), pt(w - hd, 0.0), pt(w, h / 2.0), pt(w - hd, h),
                 pt(w - hd, h * 0.75), pt(hd, h * 0.75), pt(hd, h)]
        }
        Body::ArrowBox => {
            let k = (w * 0.2).min(h * 0.4);
            vec![pt(0.0, 0.0), pt(w - k, 0.0), pt(w - k, h * 0.3), pt(w, h / 2.0), pt(w - k, h * 0.7), pt(w - k, h), pt(0.0, h)]
        }
        _ => Vec::new(),
    };
    let local = Path::new();
    local.polygon(&p);
    let m = Matrix::new();
    match dir {
        1 => {
            m.translate(r.X + r.Width, r.Y);
            m.rotate(90.0);
        }
        2 => {
            m.translate(r.X + r.Width, r.Y + r.Height);
            m.rotate(180.0);
        }
        3 => {
            m.translate(r.X, r.Y + r.Height);
            m.rotate(-90.0);
        }
        _ => m.translate(r.X, r.Y),
    }
    local.transform(&m);
    gp.add_path(&local, false);
}

fn cloud_path(gp: &Path, r: RectF) {
    let c = center(r);
    let n = 9;
    for i in 0..n {
        let a = (i * 2) as f64 * 3.14159265 / n as f64 + 0.3;
        let ex = c.X + a.cos() as f32 * r.Width * 0.34;
        let ey = c.Y + a.sin() as f32 * r.Height * 0.32;
        gp.ellipse(ex - r.Width * 0.18, ey - r.Height * 0.21, r.Width * 0.36, r.Height * 0.42);
    }
    gp.ellipse(r.X + r.Width * 0.14, r.Y + r.Height * 0.14, r.Width * 0.72, r.Height * 0.72);
}

/// The body of a callout (without the tip).
pub fn callout_body_path(gp: &Path, shape: i32, r: RectF) {
    let d = def(shape);
    match d.0 {
        Body::RoundSpeech => add_round_rect(gp, r, r.Width.min(r.Height) * 0.45),
        Body::Rect => gp.rectangle(r),
        Body::RoundRect => add_round_rect(gp, r, r.Width.min(r.Height) * 0.2),
        Body::Ellipse => gp.ellipse_r(r),
        Body::Cloud => cloud_path(gp, r),
        Body::Note => {
            let f = r.Width.min(r.Height) * 0.22;
            gp.polygon(&[pt(r.X + f, r.Y), pt(right(&r), r.Y), pt(right(&r), bottom(&r)), pt(r.X, bottom(&r)), pt(r.X, r.Y + f)]);
        }
        Body::Burst => {
            let c = center(r);
            let n = 14;
            let p: Vec<PointF> = (0..n * 2)
                .map(|i| {
                    let a = -3.14159265f64 / 2.0 + i as f64 * 3.14159265 / n as f64;
                    let k = if i % 2 == 1 { 0.62f32 + 0.08 * ((i / 2) % 3) as f32 } else { 1.0 };
                    pt(c.X + a.cos() as f32 * r.Width / 2.0 * k, c.Y + a.sin() as f32 * r.Height / 2.0 * k)
                })
                .collect();
            gp.polygon(&p);
        }
        b => arrow_body_path(gp, b, r, 0),
    }
}

/// Body + tip (wedge or bubbles) in one path (filled "winding").
fn callout_full_path(gp: &Path, o: &DrawObj) {
    let d = def(o.style.callout_shape);
    let r = o.rect();
    if is_arrow_body(d.0) {
        arrow_body_path(gp, d.0, r, arrow_dir(o));
        return;
    }
    callout_body_path(gp, o.style.callout_shape, r);
    let c = center(r);
    if d.1 == Tail::Bubbles {
        for i in 1..=3 {
            let t = i as f32 / 4.0;
            let s = 3.0f32.max(r.Width.min(r.Height) * 0.09 * (1.25 - t));
            let q = pt(c.X + (o.tail.X - c.X) * (0.55 + t * 0.45), c.Y + (o.tail.Y - c.Y) * (0.55 + t * 0.45));
            gp.ellipse(q.X - s, q.Y - s, s * 2.0, s * 2.0);
        }
        return;
    }
    if d.1 != Tail::Wedge || contains(&r, o.tail) {
        return;
    }
    let dv = sub(o.tail, c);
    let l = len(dv);
    if l < 1.0 {
        return;
    }
    let n = pt(-dv.Y / l, dv.X / l);
    let base = 6.0f32.max(r.Width.min(r.Height) * if d.0 == Body::Rect { 0.12 } else { 0.17 });
    let mut tri = [add(c, mul(n, base)), o.tail, sub(c, mul(n, base))];
    let cross = (tri[1].X - tri[0].X) * (tri[2].Y - tri[0].Y) - (tri[1].Y - tri[0].Y) * (tri[2].X - tri[0].X);
    if cross < 0.0 {
        tri.swap(0, 2);
    }
    let t = Path::new();
    t.polygon(&tri);
    gp.add_path(&t, false);
}

/// The text area inside a callout.
pub fn callout_text_rect(o: &DrawObj) -> RectF {
    let d = def(o.style.callout_shape);
    let mut r = o.rect();
    let m = r.Width.min(r.Height);
    let mut inset = o.style.padding + o.style.width.min(8.0) / 2.0;
    match d.0 {
        Body::Ellipse | Body::Cloud => {
            inset += m * 0.15;
            let dx = -r.Width * 0.06;
            inflate(&mut r, dx, 0.0);
        }
        Body::Burst => {
            inset += m * 0.22;
            let dx = -r.Width * 0.08;
            inflate(&mut r, dx, 0.0);
        }
        Body::RoundSpeech => inflate(&mut r, -m * 0.12, 0.0),
        Body::Note => {
            r.Y += m * 0.12;
            r.Height -= m * 0.12;
        }
        _ => {}
    }
    if is_arrow_body(d.0) {
        let dir = arrow_dir(o);
        let both = d.0 == Body::Double;
        let hw = (if dir == 0 || dir == 2 { r.Width } else { r.Height }) * 0.22;
        if dir == 0 || both {
            if dir == 0 || dir == 2 {
                r.Width -= hw;
            } else {
                r.Height -= hw;
            }
        }
        if dir == 2 || both {
            if dir == 0 || dir == 2 {
                r.X += hw;
                r.Width -= hw;
            } else {
                r.Y += hw;
                r.Height -= hw;
            }
        }
        if !both && dir == 1 {
            r.Height -= hw;
        }
        if !both && dir == 3 {
            r.Y += hw;
            r.Height -= hw;
        }
    }
    inflate(&mut r, -inset, -inset);
    if r.Width < 4.0 {
        r.X -= (4.0 - r.Width) / 2.0;
        r.Width = 4.0;
    }
    if r.Height < 4.0 {
        r.Y -= (4.0 - r.Height) / 2.0;
        r.Height = 4.0;
    }
    r
}

/// The point on the rectangle border in the direction of `to` (the line of the box with an arrow).
fn rect_edge(r: RectF, to: PointF) -> PointF {
    let c = center(r);
    let d = sub(to, c);
    if d.X.abs() < 1e-3 && d.Y.abs() < 1e-3 {
        return c;
    }
    let sx = if d.X != 0.0 { (r.Width / 2.0) / d.X.abs() } else { 1e9 };
    let sy = if d.Y != 0.0 { (r.Height / 2.0) / d.Y.abs() } else { 1e9 };
    add(c, mul(d, sx.min(sy).min(1.0)))
}

// ---------------------------------------------------------------- lines and ends
fn cap_inset(cap: i32, l: f32, w: f32) -> f32 {
    match cap {
        CAP_ARROW => l * 0.8,
        CAP_ARROW_WIDE => l * 0.55,
        CAP_ARROW_OPEN => w * 0.4,
        _ => 0.0,
    }
}

fn draw_cap(g: &Graphics, cap: i32, tip: PointF, dir: PointF, w: f32, l: f32, col: u32) {
    let n = pt(-dir.Y, dir.X);
    let br = Brush::solid(col);
    match cap {
        CAP_ARROW => {
            let half = l * 0.55;
            let base = sub(tip, mul(dir, l));
            g.fill_polygon(&br, &[tip, add(base, mul(n, half)), sub(base, mul(n, half))]);
        }
        CAP_ARROW_WIDE => {
            let half = l * 0.62;
            let base = sub(tip, mul(dir, l));
            let notch = sub(tip, mul(dir, l * 0.6));
            g.fill_polygon(&br, &[tip, add(base, mul(n, half)), notch, sub(base, mul(n, half))]);
        }
        CAP_ARROW_OPEN => {
            let half = l * 0.5;
            let base = sub(tip, mul(dir, l * 0.85));
            let p = Pen::new(col, 1.0f32.max(w * 0.9));
            p.start_cap(LineCapRound);
            p.end_cap(LineCapRound);
            p.line_join(LineJoinRound);
            g.draw_lines(&p, &[add(base, mul(n, half)), tip, sub(base, mul(n, half))]);
        }
        CAP_DOT => {
            let r = (w * 0.9).max(l * 0.36);
            g.fill_ellipse(&br, tip.X - r, tip.Y - r, r * 2.0, r * 2.0);
        }
        CAP_SQUARE => {
            let s = (w * 1.6).max(l * 0.6) / 2.0;
            g.fill_polygon(&br, &[
                add(add(tip, mul(dir, s)), mul(n, s)),
                sub(add(tip, mul(dir, s)), mul(n, s)),
                sub(sub(tip, mul(dir, s)), mul(n, s)),
                add(sub(tip, mul(dir, s)), mul(n, s)),
            ]);
        }
        CAP_DIAMOND => {
            let l2 = (w * 2.0).max(l * 0.75) / 2.0;
            g.fill_polygon(&br, &[add(tip, mul(dir, l2)), add(tip, mul(n, l2 * 0.7)), sub(tip, mul(dir, l2)), sub(tip, mul(n, l2 * 0.7))]);
        }
        _ => {}
    }
}

// ---------------------------------------------------------------- hand-drawn style (C35)
// deterministic noise from the object's seed: the same object always looks the same (until "randomize")
fn hash01(mut x: u32) -> f32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846ca68b);
    x ^= x >> 16;
    (x & 0xFFFFFF) as f32 / 16777215.0
}
fn noise1(seed: u32, t: f32) -> f32 {
    let i = t.floor() as i32;
    let f = t - i as f32;
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash01(seed.wrapping_mul(7919).wrapping_add((i as u32).wrapping_mul(104729)));
    let b = hash01(seed.wrapping_mul(7919).wrapping_add((i.wrapping_add(1) as u32).wrapping_mul(104729)));
    (a + (b - a) * u) * 2.0 - 1.0
}

/// The path drawn as two slightly different wobbly strokes, like a marker on paper; closed outlines overshoot a little.
fn sketch_stroke(g: &Graphics, src: &Path, pen_in: &Pen, seed: i32) {
    let gp = Path::new();
    gp.add_path(src, false);
    gp.flatten(0.4);
    let (pts, types) = gp.data();
    if pts.len() < 2 {
        return;
    }
    let w = pen_in.width();
    let amp = (w * 0.45 + 1.0).clamp(1.2, 5.0);
    let c = pen_in.color();
    let (mut fig, mut start) = (0u32, 0usize);
    for i in 0..pts.len() {
        let end_fig = i == pts.len() - 1 || (types[i + 1] & 7) == 0;
        if !end_fig {
            continue;
        }
        let closed = types[i] & 0x80 != 0;
        let mut inp: Vec<PointF> = pts[start..=i].to_vec();
        start = i + 1;
        fig += 1;
        if inp.len() < 2 {
            continue;
        }
        if closed {
            inp.push(inp[0]);
        }
        let mut total = 0.0f32;
        for k in 1..inp.len() {
            total += len(sub(inp[k], inp[k - 1]));
        }
        if total < 1.0 {
            continue;
        }
        for pass in 0..2u32 {
            let sd = (seed as u32).wrapping_mul(131).wrapping_add(fig.wrapping_mul(17)).wrapping_add(pass * 977);
            let extra = if closed { (total * 0.08).min(24.0) } else { 0.0 };
            let step = 3.0f32.max(10.0f32.min(total / 12.0));
            let mut out = Vec::new();
            let mut seg = 1usize;
            let mut seg_start = 0.0f32;
            let mut t = 0.0f32;
            while t <= total + extra + 0.01 {
                let tt = if t > total { t - total } else { t };
                if t > total {
                    seg = 1;
                    seg_start = 0.0;
                }
                while seg < inp.len() - 1 && seg_start + len(sub(inp[seg], inp[seg - 1])) < tt {
                    seg_start += len(sub(inp[seg], inp[seg - 1]));
                    seg += 1;
                }
                let (a, b) = (inp[seg - 1], inp[seg]);
                let sl = 0.001f32.max(len(sub(b, a)));
                let u = ((tt - seg_start) / sl).clamp(0.0, 1.0);
                let p = add(a, mul(sub(b, a), u));
                let nrm = pt(-(b.Y - a.Y) / sl, (b.X - a.X) / sl);
                let off = noise1(sd, t / 38.0) * amp + noise1(sd.wrapping_add(5), t / 9.0) * amp * 0.25;
                out.push(add(p, mul(nrm, off)));
                t += step;
            }
            if out.len() < 2 {
                continue;
            }
            let alpha = if pass == 1 { (ca(c) as f32 * 0.75) as u8 } else { ca(c) as u8 };
            let p = Pen::new((alpha as u32) << 24 | (c & 0x00FFFFFF), if pass == 1 { 0.8f32.max(w * 0.7) } else { w });
            p.line_join(LineJoinRound);
            p.start_cap(LineCapRound);
            p.end_cap(LineCapRound);
            g.draw_curve(&p, &out, 0.4);
        }
    }
}

/// A straight or curved line (quadratic through `ctrl`) with its ends and dash pattern.
#[allow(clippy::too_many_arguments)]
fn stroke_line(g: &Graphics, a: PointF, b: PointF, curve: bool, ctrl: PointF, col: u32, w: f32, cap_a: i32, cap_b: i32, size_a: f32,
               size_b: f32, dash: i32, sketch_seed: i32) {
    let w = 0.5f32.max(w);
    let da = norm(if curve { sub(a, ctrl) } else { sub(a, b) });
    let db = norm(if curve { sub(b, ctrl) } else { sub(b, a) });
    let total = if curve { len(sub(a, ctrl)) + len(sub(b, ctrl)) } else { len(sub(b, a)) };
    if total < 0.5 {
        return;
    }
    let (mut la, mut lb) = (cap_length(w, size_a), cap_length(w, size_b));
    let (mut ia, mut ib) = (cap_inset(cap_a, la, w), cap_inset(cap_b, lb, w));
    if ia + ib > total * 0.9 {
        let k = total * 0.9 / 0.1f32.max(ia + ib);
        ia *= k;
        ib *= k;
        la *= 1.0f32.min(k * 1.1);
        lb *= 1.0f32.min(k * 1.1);
    }
    let a2 = sub(a, mul(da, ia));
    let b2 = sub(b, mul(db, ib));
    let p = Pen::new(col, w);
    apply_dash(&p, dash);
    p.line_join(LineJoinRound);
    p.start_cap(if cap_a == CAP_ROUND || cap_a == CAP_ARROW_OPEN { LineCapRound } else { LineCapFlat });
    p.end_cap(if cap_b == CAP_ROUND || cap_b == CAP_ARROW_OPEN { LineCapRound } else { LineCapFlat });
    if dash != DASH_SOLID {
        p.set_dash_cap(if cap_a == CAP_ROUND { DashCapRound } else { p.dash_cap() });
    }
    let c1 = add(a2, mul(sub(ctrl, a2), 2.0f32 / 3.0));
    let c2 = add(b2, mul(sub(ctrl, b2), 2.0f32 / 3.0));
    if sketch_seed != 0 {
        // hand-drawn shaft (C35); the heads stay crisp
        let sp = Path::new();
        if curve {
            sp.bezier(a2, c1, c2, b2);
        } else {
            sp.line(a2, b2);
        }
        sketch_stroke(g, &sp, &p, sketch_seed);
    } else if curve {
        g.draw_bezier(&p, a2, c1, c2, b2);
    } else {
        g.line(&p, a2, b2);
    }
    draw_cap(g, cap_a, a, da, w, la, col);
    draw_cap(g, cap_b, b, db, w, lb, col);
}

/// The sample of a line end for the menus.
pub fn draw_cap_glyph(g: &Graphics, cap: i32, r: RectF, c: u32, at_start: bool) {
    let y = r.Y + r.Height / 2.0;
    let w = 2.0f32.max(r.Height * 0.16);
    let a = pt(r.X + r.Width * 0.12, y);
    let b = pt(right(&r) - r.Width * 0.12, y);
    if at_start {
        stroke_line(g, a, b, false, a, c, w, cap, CAP_FLAT, 2.4, 2.4, DASH_SOLID, 0);
    } else {
        stroke_line(g, a, b, false, a, c, w, CAP_FLAT, cap, 2.4, 2.4, DASH_SOLID, 0);
    }
}

// ---------------------------------------------------------------- text
/// Measures on a 1×1 bitmap, as the C++ did.
fn with_measure<R>(f: impl FnOnce(&Graphics) -> R) -> R {
    let bmp = Bitmap::blank(1, 1);
    let g = Graphics::new(&bmp);
    f(&g)
}

fn find(v: &[u16], c: u16, from: usize) -> Option<usize> {
    v.get(from..)?.iter().position(|&x| x == c).map(|i| i + from)
}

/// The text wrapped to the width (as AddString does: by words, breaking words longer than the line).
fn wrap_lines(text: &[u16], font: &Font, sf: &StringFormat, width: f32) -> Vec<Vec<u16>> {
    with_measure(|mg| wrap_lines_on(mg, text, font, sf, width))
}

fn wrap_lines_on(mg: &Graphics, text: &[u16], font: &Font, sf: &StringFormat, width: f32) -> Vec<Vec<u16>> {
    let fits = |t: &[u16]| mg.measure_at(t, font, pt(0.0, 0.0), sf).Width <= width + 0.5;
    let mut lines = Vec::new();
    let mut i = 0usize;
    while i <= text.len() {
        let nl = find(text, b'\n' as u16, i);
        let para = &text[i..nl.unwrap_or(text.len())];
        let mut cur: Vec<u16> = Vec::new();
        let mut k = 0usize;
        while k < para.len() {
            let sp = find(para, b' ' as u16, k);
            let mut word: Vec<u16> = para[k..sp.map_or(para.len(), |s| s + 1)].to_vec();
            k = sp.map_or(para.len(), |s| s + 1);
            let mut cand = cur.clone();
            cand.extend_from_slice(&word);
            if fits(&cand) || cur.is_empty() && fits(&word) {
                cur = cand;
                continue;
            }
            if !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
            }
            while !word.is_empty() && !fits(&word) {
                // a word wider than the line: broken by characters
                let mut n = 1;
                while n < word.len() && fits(&word[..n + 1]) {
                    n += 1;
                }
                lines.push(word[..n].to_vec());
                word.drain(..n);
            }
            cur = word;
        }
        lines.push(cur);
        match nl {
            None => break,
            Some(n) => i = n + 1,
        }
    }
    lines
}

/// The hand-drawn style (C35): a handwriting font that comes with Windows.
fn sketch_font() -> &'static str {
    static FACE: OnceLock<&'static str> = OnceLock::new();
    FACE.get_or_init(|| if FontFamily::new("Ink Free").available() { "Ink Free" } else { "Segoe Print" })
}

fn family_for(s: &Style) -> FontFamily {
    let ff = FontFamily::new(if s.sketch { sketch_font() } else { &s.font });
    if ff.available() { ff } else { FontFamily::new("Segoe UI") }
}

fn text_path(gp: &Path, o: &DrawObj, r: RectF) {
    let s = &o.style;
    let fam = family_for(s);
    let st = font_style(s);
    let sf = StringFormat::new();
    const AL: [StringAlignment; 3] = [StringAlignmentNear, StringAlignmentCenter, StringAlignmentFar];
    sf.align(AL[s.align_h.clamp(0, 2) as usize]);
    sf.line_align(AL[s.align_v.clamp(0, 2) as usize]);
    sf.trimming(StringTrimmingWord);
    let em = 4.0f32.max(s.font_size);
    let spacing = s.line_spacing.clamp(0.5, 4.0);
    if s.vertical {
        // vertical text (C37): top to bottom, columns right to left
        sf.set_flags(StringFormatFlagsDirectionVertical.0 | StringFormatFlagsDirectionRightToLeft.0);
        gp.string(&o.text, -1, &fam, st, em, r, &sf);
        return;
    }
    if (spacing - 1.0).abs() < 0.01 {
        gp.string(&o.text, -1, &fam, st, em, r, &sf);
        return;
    }
    // line spacing: the lines are wrapped here and placed `spacing` line heights apart
    let lf = sf.copy();
    lf.set_flags(lf.flags() | StringFormatFlagsNoWrap.0);
    lf.line_align(NEAR);
    let font = Font::of(&fam, em, st);
    let lines = wrap_lines(&o.text, &font, &lf, r.Width);
    let line_h = em * fam.line_spacing(st) as f32 / 1u16.max(fam.em_height(st)) as f32;
    let step = line_h * spacing;
    let total = line_h + step * (lines.len() - 1) as f32;
    let mut y = if s.align_v == 1 {
        r.Y + (r.Height - total) / 2.0
    } else if s.align_v == 2 {
        bottom(&r) - total
    } else {
        r.Y
    };
    for line in &lines {
        if !line.is_empty() {
            gp.string(line, -1, &fam, st, em, rect(r.X, y, r.Width, line_h * 1.2), &lf);
        }
        y += step;
    }
}

/// The height of the text wrapped to the width.
pub fn text_block_height(o: &DrawObj, width: f32) -> f32 {
    if o.text.is_empty() {
        return o.style.font_size * 1.35;
    }
    let mut t = o.clone();
    t.style.align_v = 0;
    if t.style.vertical {
        t.style.align_h = 0; // vertical text: the "horizontal" alignment runs down the column
    }
    let gp = Path::new();
    text_path(&gp, &t, rect(0.0, 0.0, 4.0f32.max(width), 100000.0));
    let b = gp.bounds();
    let mut extra = 0.0f32; // empty last lines (Enter at the end) take up height too
    if (o.style.line_spacing - 1.0).abs() >= 0.01 {
        for &c in o.text.iter().rev() {
            if c != b'\n' as u16 {
                break;
            }
            extra += o.style.font_size * 1.2 * o.style.line_spacing;
        }
    }
    (o.style.font_size * 1.35).max(bottom(&b) + extra + o.style.font_size * 0.3)
}

/// The label of an arrow (Skitch style): a text block beyond the tail, on the side opposite to the arrow head.
pub fn arrow_label_rect(o: &DrawObj) -> RectF {
    let s = &o.style;
    let ff = FontFamily::new(&s.font);
    let fam = if ff.available() { ff } else { FontFamily::new("Segoe UI") };
    let font = Font::of(&fam, 4.0f32.max(s.font_size), font_style(s));
    let sf = StringFormat::new();
    sf.align(StringAlignmentCenter);
    let mut t = if o.text.is_empty() { utf16("Ag") } else { o.text.clone() };
    if t.last() == Some(&(b'\n' as u16)) {
        t.extend(utf16("Ag"));
    }
    let (w, h) = with_measure(|mg| mg.measure_in(&t, &font, 160.0f32.max(s.font_size * 14.0), 100000.0, &sf));
    let (tw, th) = (w + 2.0, h + 2.0);
    let from = if s.bezier { o.ctrl } else { o.p2 };
    let dir = norm(sub(o.p1, from));
    let gap = s.width * 1.5
        + cap_length(s.width, s.start_size) * if s.arrow_start != CAP_FLAT && s.arrow_start != CAP_ROUND { 0.5 } else { 0.0 }
        + 4.0;
    let a = add(o.p1, mul(dir, gap));
    rect(a.X + dir.X * tw / 2.0 - tw / 2.0, a.Y + dir.Y * th / 2.0 - th / 2.0, tw, th)
}

/// Text that follows a line or curve (C37): each character along the path, turned to its direction, read from left to
/// right, centered, just above the line.
fn text_along_line(g: &Graphics, o: &DrawObj, fill: u32, outline: u32, outline_w: f32) {
    let s = &o.style;
    let mut pl: Vec<PointF> = Vec::new();
    if s.bezier {
        let c1 = add(o.p1, mul(sub(o.ctrl, o.p1), 2.0f32 / 3.0));
        let c2 = add(o.p2, mul(sub(o.ctrl, o.p2), 2.0f32 / 3.0));
        for i in 0..=64 {
            let t = i as f32 / 64.0;
            let u = 1.0 - t;
            pl.push(pt(u * u * u * o.p1.X + 3.0 * u * u * t * c1.X + 3.0 * u * t * t * c2.X + t * t * t * o.p2.X,
                       u * u * u * o.p1.Y + 3.0 * u * u * t * c1.Y + 3.0 * u * t * t * c2.Y + t * t * t * o.p2.Y));
        }
    } else {
        pl = vec![o.p1, o.p2];
    }
    if pl[0].X > pl[pl.len() - 1].X {
        pl.reverse();
    }
    let mut acc = vec![0.0f32; pl.len()];
    for i in 1..pl.len() {
        acc[i] = acc[i - 1] + len(sub(pl[i], pl[i - 1]));
    }
    let total_len = acc[acc.len() - 1];
    if total_len < 1.0 {
        return;
    }
    let text: Vec<u16> = o.text.iter().map(|&c| if c == b'\n' as u16 || c == b'\r' as u16 { b' ' as u16 } else { c }).collect();
    let fam = family_for(s);
    let st = font_style(s);
    let em = 4.0f32.max(s.font_size);
    let font = Font::of(&fam, em, st);
    let sf = StringFormat::typographic();
    sf.set_flags(sf.flags() | StringFormatFlagsMeasureTrailingSpaces.0);
    let mut adv = vec![0.0f32; text.len()];
    let mut total = 0.0f32;
    with_measure(|mg| {
        for i in 0..text.len() {
            adv[i] = mg.measure_at(&text[i..i + 1], &font, pt(0.0, 0.0), &sf).Width;
            total += adv[i];
        }
    });
    let ascent = em * fam.cell_ascent(st) as f32 / 1u16.max(fam.em_height(st)) as f32;
    let lift = s.width / 2.0 + em * 0.15; // the baseline just above the line
    let at = |d: f32| {
        let d = d.clamp(0.0, total_len);
        let mut k = 1;
        while k < acc.len() - 1 && acc[k] < d {
            k += 1;
        }
        let sl = 0.001f32.max(acc[k] - acc[k - 1]);
        let u = (d - acc[k - 1]) / sl;
        (add(pl[k - 1], mul(sub(pl[k], pl[k - 1]), u)), (pl[k].Y - pl[k - 1].Y).atan2(pl[k].X - pl[k - 1].X))
    };
    let all = Path::new();
    let mut pos = 0.0f32.max((total_len - total) / 2.0);
    for i in 0..text.len() {
        let mid = pos + adv[i] / 2.0;
        pos += adv[i];
        if text[i] == b' ' as u16 {
            continue;
        }
        let (p, ang) = at(mid);
        let cp = Path::new();
        cp.string(&text[i..i + 1], 1, &fam, st, em, rect(-adv[i] / 2.0, -ascent - lift, 0.0, 0.0), &sf);
        let mx = Matrix::new();
        mx.translate(p.X, p.Y);
        mx.rotate(ang * 180.0 / 3.14159265);
        cp.transform(&mx);
        all.add_path(&cp, false);
    }
    if outline_w > 0.0 && (outline >> 24) != 0 {
        let pn = Pen::new(outline, outline_w * 0.8 + 1.0);
        pn.line_join(LineJoinRound);
        g.draw_path(&pn, &all);
    }
    g.fill_path(&Brush::solid(fill), &all);
}

fn draw_text_obj(g: &Graphics, o: &DrawObj, r: RectF, fill: u32, outline: u32, outline_w: f32) {
    if o.text.is_empty() {
        return;
    }
    let gp = Path::new();
    text_path(&gp, o, r);
    if outline_w > 0.0 && (outline >> 24) != 0 {
        let p = Pen::new(outline, outline_w * 0.8 + 1.0);
        p.line_join(LineJoinRound);
        g.draw_path(&p, &gp);
    }
    g.fill_path(&Brush::solid(fill), &gp);
}

// ---------------------------------------------------------------- stamps
fn stamp_cache() -> &'static Mutex<HashMap<(String, i32), Image>> {
    static CACHE: OnceLock<Mutex<HashMap<(String, i32), Image>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The image of a stamp (emoji, "file:path", "label:RRGGBB:Text" or "cursor:kind") at a size bucket, cached.
fn stamp_bitmap(id: &str, px: i32) -> Image {
    let bucket = ((px + 31) / 32 * 32).clamp(32, 1024);
    let key = (id.to_string(), bucket);
    let mut cache = stamp_cache().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(img) = cache.get(&key) {
        return img.clone();
    }
    if cache.len() > 160 {
        cache.clear();
    }
    let img = if let Some(path) = id.strip_prefix("file:") {
        crate::codec::load(std::path::Path::new(path)).unwrap_or_default()
    } else if id.starts_with("label:") {
        stamps::label(id, bucket)
    } else if id.starts_with("cursor:") {
        stamps::cursor(id, bucket)
    } else {
        stamps::emoji(id, bucket)
    };
    cache.insert(key, img.clone());
    img
}

/// Width / height of a stamp's image.
pub fn stamp_aspect(id: &str) -> f32 {
    let img = stamp_bitmap(id, 96);
    if img.is_empty() || img.h == 0 { 1.0 } else { img.w as f32 / img.h as f32 }
}

pub fn stamp_image(id: &str, px: i32) -> Image {
    stamp_bitmap(id, px)
}

// ---------------------------------------------------------------- magnifier
fn mag_shape_kind(m: i32) -> i32 {
    const K: [i32; 6] = [SHAPE_ELLIPSE, SHAPE_RECT, SHAPE_ROUND_RECT, SHAPE_HEXAGON, SHAPE_DIAMOND, SHAPE_STAR];
    K[m.clamp(0, MAGNIFY_KIND_COUNT - 1) as usize]
}

/// The loupe handle: from the lens border at 45° (bottom right) outward.
fn loupe_handle(o: &DrawObj) -> (PointF, PointF, f32) {
    let r = o.rect();
    let rad = r.Width.min(r.Height) / 2.0;
    let c = center(r);
    let d = pt(0.7071, 0.7071);
    let edge = if mag_shape_kind(o.style.magnify_shape) == SHAPE_ELLIPSE {
        r.Width.min(r.Height) / 2.0 * 0.98
    } else {
        r.Width.min(r.Height) / 2.0 * 1.3
    };
    let a = add(c, mul(d, edge));
    let b = add(a, mul(d, rad * 0.85));
    (a, b, 6.0f32.max(rad * 0.24))
}

/// The lens rectangle plus the loupe handle.
pub fn magnifier_extra(o: &DrawObj) -> RectF {
    let r = o.rect();
    if !o.style.loupe || !o.style.loupe_handle {
        return r;
    }
    let (a, b, w) = loupe_handle(o);
    let hb = rect(a.X.min(b.X) - w, a.Y.min(b.Y) - w, (b.X - a.X).abs() + 2.0 * w, (b.Y - a.Y).abs() + 2.0 * w);
    union(&r, &hb)
}

// ---------------------------------------------------------------- the body of each object (layer)
fn draw_body(g: &Graphics, canvas: &Image, o: &DrawObj, hide_text: bool) {
    let s = &o.style;
    match o.kind {
        ObjType::Arrow | ObjType::Line => {
            stroke_line(g, o.p1, o.p2, s.bezier, o.ctrl, s.stroke, s.width, s.arrow_start, s.arrow_end, s.start_size, s.end_size,
                        s.dash, if s.sketch { 1.max(s.sketch_seed) } else { 0 });
            if o.has_label() && !o.text.is_empty() && !hide_text && s.text_on_path {
                text_along_line(g, o, s.stroke | 0xFF000000, 0xFFFFFFFF, 2.0f32.max(s.font_size * 0.14));
            } else if o.has_label() && !o.text.is_empty() && !hide_text {
                // the label at the tail, in the arrow color with a white outline
                let mut t = DrawObj::new(ObjType::Rect);
                t.text = o.text.clone();
                t.style = s.clone();
                t.style.align_h = 1;
                t.style.align_v = 1;
                t.style.line_spacing = 1.0;
                draw_text_obj(g, &t, arrow_label_rect(o), s.stroke | 0xFF000000, 0xFFFFFFFF, 2.0f32.max(s.font_size * 0.14));
            }
        }
        ObjType::Rect | ObjType::RoundRect | ObjType::Ellipse | ObjType::Shape => {
            let gp = Path::new();
            shape_path(&gp, o.shape_kind(), o.rect());
            if (s.fill >> 24) != 0 || s.grad.active() {
                g.fill_path(&fill_brush(s.fill, &s.grad, o.rect(), 100), &gp);
            }
            if s.width > 0.0 && (s.stroke >> 24) != 0 {
                let p = Pen::new(s.stroke, s.width);
                p.line_join(LineJoinMiter);
                apply_dash(&p, s.dash);
                if s.sketch {
                    sketch_stroke(g, &gp, &p, 1.max(s.sketch_seed)); // hand-drawn outline (C35)
                } else {
                    g.draw_path(&p, &gp);
                }
            }
        }
        ObjType::Pen => {
            if o.pts.is_empty() {
                return;
            }
            let p = Pen::new(s.stroke, 0.5f32.max(s.width));
            apply_dash(&p, s.dash);
            p.line_join(LineJoinRound);
            p.start_cap(LineCapRound);
            p.end_cap(LineCapRound);
            if o.pts.len() == 1 {
                g.fill_ellipse(&Brush::solid(s.stroke), o.pts[0].X - s.width / 2.0, o.pts[0].Y - s.width / 2.0, s.width, s.width);
            } else if o.pressure.len() == o.pts.len() {
                // pen pressure (C43): each segment with its own width
                for i in 1..o.pts.len() {
                    let k = 0.25 + 0.75 * (o.pressure[i - 1] + o.pressure[i]) / 2.0;
                    let sp = Pen::new(s.stroke, 0.5f32.max(s.width * k));
                    sp.start_cap(LineCapRound);
                    sp.end_cap(LineCapRound);
                    g.line(&sp, o.pts[i - 1], o.pts[i]);
                }
            } else {
                g.draw_lines(&p, &o.pts);
            }
        }
        ObjType::Highlighter => {
            // the highlighter: an area in the chosen color; the opacity comes from the layer composition
            if s.text_only {
                let b = Brush::solid(s.stroke);
                for r in &o.text_boxes {
                    g.fill_rect(&b, *r);
                }
                return;
            }
            if o.pts.is_empty() && s.area_shape == SHAPE_RECT {
                g.fill_rect(&Brush::solid(s.stroke), o.rect());
                return;
            }
            if o.pts.is_empty() || o.is_point_shape() {
                let gp = Path::new();
                area_path(&gp, o);
                g.fill_path(&Brush::solid(s.stroke), &gp);
                return;
            }
            let p = Pen::new(s.stroke, 1.0f32.max(s.width)); // old freehand highlighters
            p.line_join(LineJoinRound);
            p.start_cap(LineCapSquare);
            p.end_cap(LineCapSquare);
            g.draw_lines(&p, &o.pts);
        }
        ObjType::Text => {
            let mut r = o.rect();
            if (s.fill >> 24) != 0 {
                // old texts with a background box
                let gp = Path::new();
                add_round_rect(&gp, r, 6.0);
                g.fill_path(&Brush::solid(s.fill), &gp);
            }
            inflate(&mut r, -s.padding, -s.padding);
            if !hide_text {
                draw_text_obj(g, o, r, s.text_color, s.stroke, s.width);
            }
        }
        ObjType::Callout => {
            let d = def(s.callout_shape);
            let gp = Path::new();
            gp.fill_mode(FillModeWinding);
            callout_full_path(&gp, o);
            if d.1 == Tail::Leader && !contains(&o.rect(), o.tail) {
                // box with an arrow: the line leaves from the border
                stroke_line(g, rect_edge(o.rect(), o.tail), o.tail, false, o.tail, s.tail_color, s.tail_width, CAP_FLAT, s.arrow_end,
                            s.start_size, s.end_size, s.dash, 0);
            }
            if (s.fill >> 24) != 0 {
                let bb = gp.bounds();
                if s.shaded && bb.Height > 1.0 {
                    let lb = Brush::linear(pt(0.0, bb.Y), pt(0.0, bottom(&bb)), lighter(s.fill, ca(s.fill)), s.fill);
                    g.fill_path(&lb, &gp);
                } else {
                    g.fill_path(&Brush::solid(s.fill), &gp);
                }
            }
            if s.width > 0.0 && (s.stroke >> 24) != 0 {
                let outline = Path::new();
                outline.fill_mode(FillModeWinding);
                callout_full_path(&outline, o);
                outline.outline();
                let p = Pen::new(s.stroke, s.width);
                p.line_join(LineJoinRound);
                if d.2 {
                    apply_dash(&p, DASH_DASH);
                }
                g.draw_path(&p, &outline);
                if d.0 == Body::Note {
                    // the note's fold
                    let r = o.rect();
                    let f = r.Width.min(r.Height) * 0.22;
                    g.draw_lines(&p, &[pt(r.X + f, r.Y), pt(r.X + f, r.Y + f), pt(r.X, r.Y + f)]);
                }
            }
            if !hide_text {
                draw_text_obj(g, o, callout_text_rect(o), s.text_color, 0, 0.0);
            }
        }
        ObjType::Step => {
            let d = s.font_size * 1.5;
            let r = rect(o.p1.X - d / 2.0, o.p1.Y - d / 2.0, d, d);
            let gp = Path::new();
            gp.fill_mode(FillModeWinding);
            match s.step_shape {
                1 => {
                    // teardrop pointing right
                    gp.ellipse_r(r);
                    gp.polygon(&[pt(r.X + d * 0.5, r.Y + d * 0.06), pt(right(&r) + d * 0.42, r.Y + d / 2.0),
                                 pt(r.X + d * 0.5, bottom(&r) - d * 0.06)]);
                }
                2 => gp.rectangle(r),
                3 => add_round_rect(&gp, r, d * 0.22),
                _ => gp.ellipse_r(r),
            }
            g.fill_path(&Brush::solid(s.stroke), &gp);
            if s.step_outline_width > 0.0 && (s.step_outline >> 24) != 0 {
                // an outline for contrast on any background
                let outline = Path::new();
                outline.fill_mode(FillModeWinding);
                outline.add_path(&gp, false);
                outline.outline();
                let p = Pen::new(s.step_outline, s.step_outline_width);
                p.line_join(LineJoinRound);
                g.draw_path(&p, &outline);
            }
            let label = utf16(&step_label(o.step_value, s.step_type));
            let mut t = DrawObj::new(ObjType::Rect);
            t.style.font = "Segoe UI".into();
            t.style.bold = true;
            t.style.align_h = 1;
            t.style.align_v = 1;
            t.style.font_size = d * if label.len() <= 1 { 0.56 } else if label.len() == 2 { 0.46 } else { 0.34 };
            t.text = label;
            draw_text_obj(g, &t, rect(r.X, r.Y + d * 0.02, d, d), s.text_color, 0, 0.0);
        }
        ObjType::Stamp => {
            let r = o.rect();
            if r.Width < 2.0 || r.Height < 2.0 {
                return;
            }
            let img = stamp_bitmap(&String::from_utf16_lossy(&o.text), r.Width.max(r.Height) as i32);
            if img.is_empty() {
                return;
            }
            let k = (r.Width / img.w as f32).min(r.Height / img.h as f32);
            let dst = rect(r.X + (r.Width - img.w as f32 * k) / 2.0, r.Y + (r.Height - img.h as f32 * k) / 2.0, img.w as f32 * k,
                           img.h as f32 * k);
            let bmp = Bitmap::wrap_read(&img);
            g.interpolation(InterpolationModeHighQualityBicubic);
            g.draw_image_f(&bmp, dst);
        }
        ObjType::Magnify => draw_magnifier(g, canvas, o),
        ObjType::Picture => {
            let Some(pic) = &o.picture else { return };
            if pic.is_empty() {
                return;
            }
            let r = o.rect();
            let pb = Bitmap::wrap_read(pic);
            g.draw_image_src(&pb, r, rect(0.0, 0.0, pic.w as f32, pic.h as f32));
            if s.width > 0.0 && (s.stroke >> 24) != 0 {
                g.draw_rect(&Pen::new(s.stroke, s.width), r);
            }
        }
        _ => {}
    }
}

fn draw_magnifier(g: &Graphics, canvas: &Image, o: &DrawObj) {
    let s = &o.style;
    let r = o.rect();
    if r.Width < 4.0 || r.Height < 4.0 {
        return;
    }
    let k = (s.magnify_scale as f32 / 100.0).clamp(1.1, 10.0);
    let src = if s.magnify_link { o.tail } else { center(r) };
    let (sw, sh) = (r.Width / k, r.Height / k);
    let sr = rect(src.X - sw / 2.0, src.Y - sh / 2.0, sw, sh);
    let shape = mag_shape_kind(s.magnify_shape);
    let pen = Pen::new(s.stroke, 1.0f32.max(s.width));
    if s.magnify_link {
        // the magnified area marked and connected to the lens
        let thin = Pen::new(s.stroke, 1.0f32.max(s.width * 0.6));
        let sp = Path::new();
        shape_path(&sp, shape, sr);
        let (a, b) = (center(sr), center(r));
        let dv = norm(sub(b, a));
        let ra = sr.Width.min(sr.Height) / 2.0;
        let rb = r.Width.min(r.Height) / 2.0;
        if len(sub(b, a)) > ra + rb {
            g.line(&thin, add(a, mul(dv, ra)), sub(b, mul(dv, rb)));
        }
        g.draw_path(&thin, &sp);
    }
    let piece = canvas.crop(Rect::new(sr.X.floor() as i32, sr.Y.floor() as i32, right(&sr).ceil() as i32, bottom(&sr).ceil() as i32));
    let lens = Path::new();
    shape_path(&lens, shape, r);
    g.fill_path(&Brush::solid(0xFFFFFFFF), &lens);
    if !piece.is_empty() {
        let pb = Bitmap::wrap_read(&piece);
        let gs = g.save();
        g.clip_path(&lens);
        g.interpolation(InterpolationModeHighQualityBicubic);
        let ox = 0.0f32.max(sr.X.floor()) - sr.X;
        let oy = 0.0f32.max(sr.Y.floor()) - sr.Y;
        g.draw_image_f(&pb, rect(r.X + ox * k, r.Y + oy * k, piece.w as f32 * k, piece.h as f32 * k));
        g.restore(gs);
    }
    if s.loupe {
        // a real lens: handle, thick shaded rim, inner shadow and reflection
        let rad = r.Width.min(r.Height) / 2.0;
        let rim = 4.0f32.max(rad * 0.11 + s.width * 0.5);
        let light = lighter(lighter(s.stroke, 255), 255);
        let dark = argb(255, cr(s.stroke) * 3 / 5, cg(s.stroke) * 3 / 5, cb(s.stroke) * 3 / 5);
        if s.loupe_handle {
            let (a, b, w) = loupe_handle(o);
            let n = pt(-0.7071, 0.7071);
            let hb = Brush::linear(add(a, mul(n, w / 2.0)), sub(a, mul(n, w / 2.0)), dark, light);
            let hp = Pen::with_brush(&hb, w);
            hp.start_cap(LineCapFlat);
            hp.end_cap(LineCapRound);
            g.line(&hp, a, b);
            let collar = Pen::new(dark, w * 1.25);
            collar.start_cap(LineCapFlat);
            collar.end_cap(LineCapFlat);
            g.line(&collar, a, add(a, mul(pt(0.7071, 0.7071), w * 0.9)));
        }
        let gs = g.save(); // inner shadow at the edge of the glass
        g.clip_path(&lens);
        g.draw_path(&Pen::new(argb(60, 0, 0, 0), rim * 1.6), &lens);
        let glare = Path::new(); // reflection on the upper left
        glare.ellipse(r.X + r.Width * 0.14, r.Y + r.Height * 0.08, r.Width * 0.52, r.Height * 0.34);
        let gb = Brush::path_gradient(&glare);
        gb.path_blend(&[argb(0, 255, 255, 255), argb(95, 255, 255, 255)], &[0.0, 1.0]);
        gb.path_center(pt(r.X + r.Width * 0.36, r.Y + r.Height * 0.2));
        g.fill_path(&gb, &glare);
        g.restore(gs);
        let rb = Brush::linear(pt(r.X, r.Y), pt(right(&r), bottom(&r)), light, dark);
        let rp = Pen::with_brush(&rb, rim);
        rp.line_join(LineJoinRound);
        g.draw_path(&rp, &lens);
        let edge = Pen::new(argb(150, 0, 0, 0), 1.0);
        let outer = Path::new();
        shape_path(&outer, shape, rect(r.X - rim / 2.0, r.Y - rim / 2.0, r.Width + rim, r.Height + rim));
        g.draw_path(&edge, &outer);
        let inner_edge = Path::new();
        shape_path(&inner_edge, shape, rect(r.X + rim / 2.0, r.Y + rim / 2.0, r.Width - rim, r.Height - rim));
        g.draw_path(&edge, &inner_edge);
    } else if s.width > 0.0 {
        g.draw_path(&pen, &lens);
    }
}

/// Blur of the alpha only (three box passes ≈ Gaussian), for the shadow.
fn blur_alpha(a: &mut [i32], w: i32, h: i32, radius: i32) {
    if radius < 1 {
        return;
    }
    let (wu, n) = (w as usize, a.len());
    let mut tmp = vec![0i32; n];
    let r = 1.max(radius / 2);
    for _ in 0..3 {
        for y in 0..h as usize {
            let (src, dst) = (&a[y * wu..(y + 1) * wu], &mut tmp[y * wu..(y + 1) * wu]);
            let mut sum = 0;
            for x in -r..=r {
                sum += src[x.clamp(0, w - 1) as usize];
            }
            for x in 0..w {
                dst[x as usize] = sum / (2 * r + 1);
                sum += src[(x + r + 1).min(w - 1) as usize] - src[(x - r).max(0) as usize];
            }
        }
        for x in 0..wu {
            let mut sum = 0;
            for y in -r..=r {
                sum += tmp[y.clamp(0, h - 1) as usize * wu + x];
            }
            for y in 0..h {
                a[y as usize * wu + x] = sum / (2 * r + 1);
                sum += tmp[(y + r + 1).min(h - 1) as usize * wu + x] - tmp[(y - r).max(0) as usize * wu + x];
            }
        }
    }
}

fn composite(dst: &mut Image, src: &Image, ox: i32, oy: i32, opacity: i32) {
    let opacity = opacity.clamp(0, 100);
    for y in 0..src.hi() {
        let dy = oy + y;
        if dy < 0 || dy >= dst.hi() {
            continue;
        }
        for x in 0..src.wi() {
            let dx = ox + x;
            if dx < 0 || dx >= dst.wi() {
                continue;
            }
            let s = src.at(x, y);
            let a = ca(s) * opacity / 100;
            if a == 0 {
                continue;
            }
            let d = dst.at_mut(dx, dy);
            *d = blend_over(*d, (s & 0x00FFFFFF) | (a as u32) << 24);
        }
    }
}

fn clip_rect(rf: RectF, canvas: &Image) -> Rect {
    Rect::new(0.max(rf.X.floor() as i32), 0.max(rf.Y.floor() as i32), canvas.wi().min(right(&rf).ceil() as i32),
              canvas.hi().min(bottom(&rf).ceil() as i32))
}

/// Old freehand highlighters (documents from earlier versions): the color is multiplied by the image, as before.
fn legacy_highlight(g: &Graphics, canvas: &mut Image, o: &DrawObj) {
    g.flush();
    let b = clip_rect(o.bounds(), canvas);
    if b.right <= b.left || b.bottom <= b.top {
        return;
    }
    let mut mask = Image::sized(b.right - b.left, b.bottom - b.top, 0);
    {
        let mb = Bitmap::wrap(&mut mask);
        let mg = Graphics::new(&mb);
        mg.smoothing(SmoothingModeAntiAlias);
        mg.translate(-b.left as f32, -b.top as f32);
        let w = o.style.width;
        let p = Pen::new(0xFFFFFFFF, 1.0f32.max(w));
        p.line_join(LineJoinRound);
        p.start_cap(LineCapSquare);
        p.end_cap(LineCapSquare);
        if o.pts.len() == 1 {
            mg.fill_rect(&Brush::solid(0xFFFFFFFF), rect(o.pts[0].X - w / 2.0, o.pts[0].Y - w / 2.0, w, w));
        } else {
            mg.draw_lines(&p, &o.pts);
        }
    }
    let s = o.style.stroke;
    let (hr, hg, hb) = (cr(s), cg(s), cb(s));
    let op = o.style.opacity.clamp(0, 100);
    for y in 0..mask.hi() {
        for x in 0..mask.wi() {
            let a = ca(mask.at(x, y)) * op / 100;
            if a == 0 {
                continue;
            }
            let p = canvas.at_mut(b.left + x, b.top + y);
            let pa = ca(*p);
            let r = (cr(*p) * pa + 255 * (255 - pa)) / 255;
            let gg = (cg(*p) * pa + 255 * (255 - pa)) / 255;
            let bb = (cb(*p) * pa + 255 * (255 - pa)) / 255;
            *p = argb(pa.max(a), r + (r * hr / 255 - r) * a / 255, gg + (gg * hg / 255 - gg) * a / 255, bb + (bb * hb / 255 - bb) * a / 255);
        }
    }
}

/// The anti-aliased coverage of a path inside `b` (one byte per pixel).
fn path_mask(gp: &Path, b: Rect) -> Vec<u8> {
    let mut m = Image::sized(b.right - b.left, b.bottom - b.top, 0);
    {
        let mb = Bitmap::wrap(&mut m);
        let mg = Graphics::new(&mb);
        mg.smoothing(SmoothingModeAntiAlias);
        mg.pixel_offset(PixelOffsetModeHalf);
        mg.translate(-b.left as f32, -b.top as f32);
        mg.fill_path(&Brush::solid(0xFFFFFFFF), gp);
        mg.flush();
    }
    m.px.iter().map(|&p| ca(p) as u8).collect()
}

/// An effect (blur / pixelate) applied to the canvas only inside the path, with soft edges.
fn effect_in_path(canvas: &mut Image, gp: &Path, effect: impl Fn(&mut Image, Rect)) {
    let b = clip_rect(gp.bounds(), canvas);
    if b.right <= b.left || b.bottom <= b.top {
        return;
    }
    let mask = path_mask(gp, b);
    let mut piece = canvas.crop(b);
    let pr = Rect::new(0, 0, piece.wi(), piece.hi());
    effect(&mut piece, pr);
    for y in 0..piece.hi() {
        for x in 0..piece.wi() {
            let a = mask[y as usize * piece.w as usize + x as usize] as u32;
            if a == 0 {
                continue;
            }
            let d = canvas.at_mut(b.left + x, b.top + y);
            let sp = piece.at(x, y);
            if a == 255 {
                *d = sp;
                continue;
            }
            let mix = |sh: u32| ((((*d >> sh) & 255) * (255 - a) + ((sp >> sh) & 255) * a) / 255) << sh;
            *d = mix(24) | mix(16) | mix(8) | mix(0);
        }
    }
}

fn dim(p: &mut u32, k: i32) {
    *p = argb(ca(*p), cr(*p) * (100 - k) / 100, cg(*p) * (100 - k) / 100, cb(*p) * (100 - k) / 100);
}

/// Draws one object on the canvas; `g` is a Graphics over the same pixels (anti-aliased, half pixel offset).
pub fn render_object(g: &Graphics, canvas: &mut Image, o: &DrawObj, hide_text: bool) {
    let s = &o.style;
    if o.kind == ObjType::Highlighter && o.is_path() {
        legacy_highlight(g, canvas, o); // polygon / freehand areas are not legacy
        return;
    }
    if o.kind == ObjType::Group {
        for c in &o.children {
            render_object(g, canvas, c, false); // each child with its own shadow
        }
        return;
    }
    if matches!(o.kind, ObjType::Blur | ObjType::Pixelate | ObjType::Spotlight) {
        g.flush();
        let dark = (s.amount * 6).clamp(20, 85);
        if s.text_only {
            // only the words inside the area (C12): their boxes
            let mut boxes = Vec::new();
            for rf in &o.text_boxes {
                let r = Rect::new(0.max(rf.X.floor() as i32 - 1), 0.max(rf.Y.floor() as i32 - 1),
                                  canvas.wi().min(right(rf).ceil() as i32 + 1), canvas.hi().min(bottom(rf).ceil() as i32 + 1));
                if r.right > r.left && r.bottom > r.top {
                    boxes.push(r);
                }
            }
            match o.kind {
                ObjType::Blur => {
                    for r in &boxes {
                        fx::box_blur(canvas, *r, 2.max(((r.bottom - r.top) / 2).min(s.amount)));
                    }
                }
                ObjType::Pixelate => {
                    for r in &boxes {
                        fx::pixelate(canvas, *r, 3.max(((r.bottom - r.top) / 2).min(s.amount * 3 / 2)));
                    }
                }
                _ => {
                    for y in 0..canvas.hi() {
                        for x in 0..canvas.wi() {
                            if boxes.iter().any(|r| x >= r.left && x < r.right && y >= r.top && y < r.bottom) {
                                continue;
                            }
                            dim(canvas.at_mut(x, y), dark);
                        }
                    }
                }
            }
            return;
        }
        if s.area_shape != SHAPE_RECT || o.is_point_shape() {
            // round, polygon and freehand areas (F05, F18)
            let gp = Path::new();
            area_path(&gp, o);
            match o.kind {
                ObjType::Blur => effect_in_path(canvas, &gp, |im, r| fx::box_blur(im, r, 2.max(s.amount))),
                ObjType::Pixelate => effect_in_path(canvas, &gp, |im, r| fx::pixelate(im, r, 3.max(s.amount * 3 / 2))),
                _ => {
                    let b = clip_rect(gp.bounds(), canvas);
                    let mask = if b.right > b.left && b.bottom > b.top { path_mask(&gp, b) } else { Vec::new() };
                    for y in 0..canvas.hi() {
                        for x in 0..canvas.wi() {
                            let a = if x >= b.left && x < b.right && y >= b.top && y < b.bottom {
                                mask[(y - b.top) as usize * (b.right - b.left) as usize + (x - b.left) as usize] as i32
                            } else {
                                0
                            };
                            if a == 255 {
                                continue;
                            }
                            dim(canvas.at_mut(x, y), dark * (255 - a) / 255);
                        }
                    }
                    let p = Pen::new(0xC8FFFFFF, 2.0);
                    p.line_join(LineJoinRound);
                    g.draw_path(&p, &gp);
                }
            }
            return;
        }
        let rf = o.rect();
        let r = Rect::new(rf.X.floor() as i32, rf.Y.floor() as i32, right(&rf).ceil() as i32, bottom(&rf).ceil() as i32);
        match o.kind {
            ObjType::Blur => fx::box_blur(canvas, r, 2.max(s.amount)),
            ObjType::Pixelate => fx::pixelate(canvas, r, 3.max(s.amount * 3 / 2)),
            _ => {
                for y in 0..canvas.hi() {
                    for x in 0..canvas.wi() {
                        if x >= r.left && x < r.right && y >= r.top && y < r.bottom {
                            continue;
                        }
                        dim(canvas.at_mut(x, y), dark);
                    }
                }
                g.draw_rect(&Pen::new(0xC8FFFFFF, 2.0), rf);
            }
        }
        return;
    }
    g.flush();
    if o.kind == ObjType::Callout && s.bg_blur {
        // frosted glass: blurs the composed image behind the callout
        let gp = Path::new();
        gp.fill_mode(FillModeWinding);
        callout_full_path(&gp, o);
        let radius = s.bg_blur_amount.clamp(1, 40);
        effect_in_path(canvas, &gp, |im, r| fx::box_blur(im, r, radius));
    }
    let b = clip_rect(o.bounds(), canvas);
    if b.right <= b.left || b.bottom <= b.top {
        return;
    }
    let mut layer = Image::sized(b.right - b.left, b.bottom - b.top, 0);
    {
        let lb = Bitmap::wrap(&mut layer);
        let lg = Graphics::new(&lb);
        lg.smoothing(SmoothingModeAntiAlias);
        lg.text_hint(TextRenderingHintAntiAlias);
        lg.pixel_offset(PixelOffsetModeHalf);
        lg.interpolation(InterpolationModeHighQualityBicubic);
        lg.translate(-b.left as f32, -b.top as f32);
        draw_body(&lg, canvas, o, hide_text);
        lg.flush();
    }
    if s.shadow && o.kind != ObjType::Highlighter {
        let mut a: Vec<i32> = layer.px.iter().map(|&p| ca(p)).collect();
        blur_alpha(&mut a, layer.wi(), layer.hi(), s.shadow_blur);
        let mut sh = Image::new(layer.w, layer.h, 0);
        let rgb = s.shadow_color & 0x00FFFFFF;
        for (i, &v) in a.iter().enumerate() {
            if v != 0 {
                sh.px[i] = rgb | (255.min(v) as u32) << 24;
            }
        }
        let dir = s.shadow_dir.clamp(0, 8);
        let (dx, dy) = ((dir % 3 - 1) * s.shadow_dist, (dir / 3 - 1) * s.shadow_dist);
        composite(canvas, &sh, b.left + dx, b.top + dy, s.shadow_opacity * s.opacity / 100);
    }
    composite(canvas, &layer, b.left, b.top, s.opacity);
}

/// Document::Render: the objects composed over the base image (hidden objects and links left out).
pub fn render(base: &Image, objs: &[DrawObj], hide_text: Option<usize>) -> Image {
    let mut out = base.clone();
    let px = out.px.as_mut_ptr();
    let (w, h) = (out.w, out.h);
    // the Graphics draws into the same pixels the painters also change directly (flushed before each direct change)
    let bmp = unsafe { Bitmap::over_raw(w, h, px) };
    let g = Graphics::new(&bmp);
    g.smoothing(SmoothingModeAntiAlias);
    g.text_hint(TextRenderingHintAntiAliasGridFit);
    g.interpolation(InterpolationModeHighQualityBicubic);
    g.pixel_offset(PixelOffsetModeHalf);
    for (i, o) in objs.iter().enumerate() {
        if !o.hidden && o.kind != ObjType::Link {
            render_object(&g, &mut out, o, hide_text == Some(i));
        }
    }
    g.flush();
    drop(g);
    drop(bmp);
    out
}

