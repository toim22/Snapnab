//! Pure helpers of the Editor's tools (src/editor/EditorTools.cpp): the shape of a pen stroke held still (draw &
//! hold, C23) and the text under an area (C12: the words an area hides, the lines a highlighter snaps to).

use windows::Win32::Graphics::GdiPlus::{PointF, RectF};
use snapcore::gdip::{pt, rect};
use snapcore::ocr::Line;

pub const REC_NONE: i32 = 0;
pub const REC_LINE: i32 = 1;
pub const REC_ARROW: i32 = 2;
pub const REC_RECT: i32 = 3;
pub const REC_ELLIPSE: i32 = 4;
pub const REC_TRIANGLE: i32 = 5;

fn hypot(x: f32, y: f32) -> f32 {
    x.hypot(y)
}

fn seg_dist(p: PointF, a: PointF, b: PointF) -> f32 {
    let (dx, dy) = (b.X - a.X, b.Y - a.Y);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 { (((p.X - a.X) * dx + (p.Y - a.Y) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
    hypot(p.X - (a.X + t * dx), p.Y - (a.Y + t * dy))
}

/// The shape of a rough pen stroke (REC_*); `a`, `b` are the line's ends (tail and tip for the arrow) or the corners
/// of the box.
pub fn recognize_stroke(pts: &[PointF]) -> (i32, PointF, PointF) {
    let none = (REC_NONE, PointF::default(), PointF::default());
    if pts.len() < 4 {
        return none;
    }
    let (mut x0, mut y0) = (pts[0].X, pts[0].Y);
    let (mut x1, mut y1, mut l) = (x0, y0, 0.0f32);
    for (i, p) in pts.iter().enumerate() {
        x0 = x0.min(p.X);
        y0 = y0.min(p.Y);
        x1 = x1.max(p.X);
        y1 = y1.max(p.Y);
        if i > 0 {
            l += hypot(p.X - pts[i - 1].X, p.Y - pts[i - 1].Y);
        }
    }
    let (w, h) = (x1 - x0, y1 - y0);
    let diag = hypot(w, h);
    if diag < 16.0 || l < 16.0 {
        return none;
    }
    let (s, e) = (pts[0], *pts.last().unwrap());
    let chord = hypot(e.X - s.X, e.Y - s.Y);
    // straight line: every point close to the chord
    let max_dev = pts.iter().map(|p| seg_dist(*p, s, e)).fold(0.0f32, f32::max);
    if chord > 20.0 && max_dev < 4.0f32.max(chord * 0.05) && l < chord * 1.25 {
        return (REC_LINE, s, e);
    }
    // arrow: straight to the tip (the farthest point from the start), then a short hook drawn for the head
    let (mut tip, mut farthest) = (0usize, 0.0f32);
    for (i, p) in pts.iter().enumerate() {
        let d = hypot(p.X - s.X, p.Y - s.Y);
        if d > farthest {
            farthest = d;
            tip = i;
        }
    }
    if farthest > 30.0 && tip > pts.len() / 2 && tip + 2 < pts.len() {
        let dev = pts[..=tip].iter().map(|p| seg_dist(*p, s, pts[tip])).fold(0.0f32, f32::max);
        let tail: f32 = (tip + 1..pts.len()).map(|i| hypot(pts[i].X - pts[i - 1].X, pts[i].Y - pts[i - 1].Y)).sum();
        if dev < 5.0f32.max(farthest * 0.07) && tail > farthest * 0.08 && tail < farthest * 0.9 {
            return (REC_ARROW, s, pts[tip]);
        }
    }
    // closed outlines: the end comes back near the start
    if chord > diag * 0.25 || w < 12.0 || h < 12.0 {
        return none;
    }
    let mut area = 0.0f64; // shoelace on the closed stroke
    for i in 0..pts.len() {
        let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
        area += p.X as f64 * q.Y as f64 - q.X as f64 * p.Y as f64;
    }
    let area = area.abs() / 2.0;
    let (a, b) = (pt(x0, y0), pt(x1, y1));
    // measured in the smallest box around the stroke at any angle (a hand-drawn square is rarely level): the share of
    // the box it fills (rectangle ≈ 1, ellipse ≈ 0.785, triangle ≈ 0.5), the radial error against the ellipse
    // inscribed in that box and how many of the box's corners the stroke reaches
    let mut sorted = pts.to_vec();
    sorted.sort_by(|p, q| p.X.partial_cmp(&q.X).unwrap().then(p.Y.partial_cmp(&q.Y).unwrap()));
    let cross = |o: PointF, p: PointF, q: PointF| (p.X - o.X) * (q.Y - o.Y) - (p.Y - o.Y) * (q.X - o.X);
    let mut hull: Vec<PointF> = Vec::new();
    for _ in 0..2 {
        let base = hull.len();
        for &p in &sorted {
            while hull.len() >= base + 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0 {
                hull.pop();
            }
            hull.push(p);
        }
        hull.pop();
        sorted.reverse();
    }
    let (mut best_area, mut ux, mut uy) = (1e30f32, 1.0f32, 0.0f32);
    for i in 0..hull.len() {
        let (p, q) = (hull[i], hull[(i + 1) % hull.len()]);
        let len = hypot(q.X - p.X, q.Y - p.Y);
        if len < 1e-3 {
            continue;
        }
        let (dx, dy) = ((q.X - p.X) / len, (q.Y - p.Y) / len);
        let (mut u0, mut u1, mut v0, mut v1) = (1e30f32, -1e30f32, 1e30f32, -1e30f32);
        for r in &hull {
            let (u, v) = (r.X * dx + r.Y * dy, -r.X * dy + r.Y * dx);
            u0 = u0.min(u);
            u1 = u1.max(u);
            v0 = v0.min(v);
            v1 = v1.max(v);
        }
        if (u1 - u0) * (v1 - v0) < best_area {
            best_area = (u1 - u0) * (v1 - v0);
            ux = dx;
            uy = dy;
        }
    }
    let (mut u0, mut u1, mut v0, mut v1) = (1e30f32, -1e30f32, 1e30f32, -1e30f32);
    for p in pts {
        let (u, v) = (p.X * ux + p.Y * uy, -p.X * uy + p.Y * ux);
        u0 = u0.min(u);
        u1 = u1.max(u);
        v0 = v0.min(v);
        v1 = v1.max(v);
    }
    let (bw, bh) = ((u1 - u0).max(1.0), (v1 - v0).max(1.0));
    let fill = area / (bw as f64 * bh as f64);
    if fill < 0.62 {
        return if fill > 0.3 { (REC_TRIANGLE, a, b) } else { none };
    }
    let mut err = 0.0f32;
    let mut cn = [1e9f32; 4];
    for p in pts {
        let nu = (p.X * ux + p.Y * uy - u0) / bw;
        let nv = (-p.X * uy + p.Y * ux - v0) / bh;
        err += (hypot(nu * 2.0 - 1.0, nv * 2.0 - 1.0) - 1.0).abs();
        for (k, c) in cn.iter_mut().enumerate() {
            *c = c.min(hypot(nu - (k & 1) as f32, nv - (k >> 1) as f32));
        }
    }
    err /= pts.len() as f32;
    let corners = cn.iter().filter(|d| **d < 0.14).count();
    if corners >= 3 || fill > 0.87 || err >= 0.11 { (REC_RECT, a, b) } else { (REC_ELLIPSE, a, b) }
}

/// RectF::Intersect: the common part, None when it has no area.
pub fn intersect(a: RectF, b: RectF) -> Option<RectF> {
    let (l, t) = (a.X.max(b.X), a.Y.max(b.Y));
    let (r, bt) = ((a.X + a.Width).min(b.X + b.Width), (a.Y + a.Height).min(b.Y + b.Height));
    (r > l && bt > t).then(|| rect(l, t, r - l, bt - t))
}

fn word_rect(r: &snapcore::image::Rect) -> RectF {
    rect(r.left as f32, r.top as f32, (r.right - r.left) as f32, (r.bottom - r.top) as f32)
}

/// Word boxes inside `area` (at least half of each word), cut to the area.
pub fn words_in_area(lines: &[Line], area: RectF) -> Vec<RectF> {
    let mut out = Vec::new();
    for l in lines {
        for w in &l.words {
            let r = word_rect(&w.rect);
            if r.Width <= 0.0 || r.Height <= 0.0 {
                continue;
            }
            let Some(x) = intersect(r, area) else { continue };
            if x.Width * x.Height * 2.0 < r.Width * r.Height {
                continue;
            }
            out.push(x);
        }
    }
    out
}

/// Smart highlighter: one rectangle per text line, covering the words the stroke touched (empty = no text there).
pub fn snap_to_text_lines(lines: &[Line], area: RectF) -> Vec<RectF> {
    let mut out = Vec::new();
    for l in lines {
        let mut u: Option<RectF> = None;
        for w in &l.words {
            let r = word_rect(&w.rect);
            if r.Width <= 0.0 || r.Height <= 0.0 {
                continue;
            }
            let Some(x) = intersect(r, area) else { continue };
            // touched: a good part of the word, or most of the line's height under the stroke
            if x.Width * x.Height * 2.5 < r.Width * r.Height && x.Height < r.Height * 0.5 {
                continue;
            }
            u = Some(match u {
                None => r,
                Some(a) => snapcore::doc::union(&a, &r),
            });
        }
        if let Some(mut u) = u {
            let pad = (u.Height * 0.12).max(1.0);
            snapcore::doc::inflate(&mut u, pad, pad);
            out.push(u);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x: f32, y: f32, s: f32) -> Vec<PointF> {
        let mut v = Vec::new();
        for i in 0..20 {
            v.push(pt(x + s * i as f32 / 20.0, y));
        }
        for i in 0..20 {
            v.push(pt(x + s, y + s * i as f32 / 20.0));
        }
        for i in 0..20 {
            v.push(pt(x + s - s * i as f32 / 20.0, y + s));
        }
        for i in 0..20 {
            v.push(pt(x, y + s - s * i as f32 / 20.0));
        }
        v.push(pt(x + 2.0, y + 3.0));
        v
    }

    #[test]
    fn a_rough_square_is_a_rectangle() {
        assert_eq!(recognize_stroke(&square(10.0, 10.0, 200.0)).0, REC_RECT);
    }

    #[test]
    fn a_straight_stroke_is_a_line_and_a_circle_an_ellipse() {
        let line: Vec<PointF> = (0..30).map(|i| pt(i as f32 * 10.0, 5.0 + (i % 2) as f32)).collect();
        assert_eq!(recognize_stroke(&line).0, REC_LINE);
        let circle: Vec<PointF> = (0..=60)
            .map(|i| {
                let a = i as f32 / 60.0 * std::f32::consts::TAU;
                pt(200.0 + 100.0 * a.cos(), 200.0 + 100.0 * a.sin())
            })
            .collect();
        assert_eq!(recognize_stroke(&circle).0, REC_ELLIPSE);
    }
}
