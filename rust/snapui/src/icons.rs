//! The vector icons (Icons.cpp) drawn with the egui painter: each icon is laid out on a 24×24 grid, as in the C++, and
//! scaled to the rectangle it is drawn in. Only the icons the egui windows use so far are here; the rest move over with
//! the windows that need them.

use egui::{Color32, Mesh, Painter, Pos2, Rect, Shape, Stroke, vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Record,
    Pause,
    Stop,
    Restart,
    Cancel,
    Mic,
    MicOff,
    Speaker,
    SpeakerOff,
    Camera,
    Webcam,
    WebcamOff,
    Sliders,
    Cursor,
    Focus,
    Lock,
    Pin,
}

struct Pen<'a> {
    p: &'a Painter,
    r: Rect,
    k: f32,
    c: Color32,
}

impl Pen<'_> {
    fn at(&self, x: f32, y: f32) -> Pos2 {
        self.r.min + vec2(x, y) * self.k
    }
    fn stroke(&self) -> Stroke {
        Stroke::new(1.8 * self.k, self.c)
    }
    fn faint(&self) -> Color32 {
        Color32::from_rgba_unmultiplied(self.c.r(), self.c.g(), self.c.b(), 70)
    }
    fn line(&self, x1: f32, y1: f32, x2: f32, y2: f32) {
        self.p.line_segment([self.at(x1, y1), self.at(x2, y2)], self.stroke());
    }
    fn fill_rect(&self, x: f32, y: f32, w: f32, h: f32, c: Color32) {
        self.p.rect_filled(Rect::from_min_size(self.at(x, y), vec2(w, h) * self.k), 0.0, c);
    }
    fn rect(&self, x: f32, y: f32, w: f32, h: f32) {
        self.p.rect_stroke(Rect::from_min_size(self.at(x, y), vec2(w, h) * self.k), 0.0, self.stroke(), egui::StrokeKind::Middle);
    }
    /// An ellipse in the bounding box (x, y, w, h).
    fn ellipse(&self, x: f32, y: f32, w: f32, h: f32, fill: Option<Color32>) {
        let c = self.at(x + w / 2.0, y + h / 2.0);
        let rad = vec2(w, h) * self.k / 2.0;
        match fill {
            Some(f) => self.p.add(Shape::ellipse_filled(c, rad, f)),
            None => self.p.add(Shape::ellipse_stroke(c, rad, self.stroke())),
        };
    }
    /// DrawArc: the part of the ellipse (x, y, w, h) from `start` degrees, `sweep` degrees clockwise (y down).
    fn arc(&self, x: f32, y: f32, w: f32, h: f32, start: f32, sweep: f32) {
        let (cx, cy, rx, ry) = (x + w / 2.0, y + h / 2.0, w / 2.0, h / 2.0);
        let n = ((sweep.abs() / 10.0).ceil() as usize).max(4);
        let pts = (0..=n)
            .map(|i| {
                let a = (start + sweep * i as f32 / n as f32).to_radians();
                self.at(cx + rx * a.cos(), cy + ry * a.sin())
            })
            .collect();
        self.p.add(Shape::line(pts, self.stroke()));
    }
    fn polygon(&self, pts: &[(f32, f32)], fill: Option<Color32>, outline: bool) {
        let ps: Vec<Pos2> = pts.iter().map(|&(x, y)| self.at(x, y)).collect();
        if let Some(f) = fill {
            self.p.add(Shape::mesh(triangulate(&ps, f)));
        }
        if outline {
            self.p.add(Shape::closed_line(ps, self.stroke()));
        }
    }
}

/// Ear clipping: the fill of a simple polygon, convex or not (the cursor arrow is not).
fn triangulate(pts: &[Pos2], c: Color32) -> Mesh {
    let mut m = Mesh::default();
    for &p in pts {
        m.colored_vertex(p, c);
    }
    let area: f32 = (0..pts.len()).map(|i| {
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        a.x * b.y - b.x * a.y
    }).sum();
    let mut idx: Vec<u32> = (0..pts.len() as u32).collect();
    if area < 0.0 {
        idx.reverse();
    }
    let cross = |a: Pos2, b: Pos2, c: Pos2| (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
    let mut guard = 0;
    while idx.len() > 3 && guard < 1000 {
        guard += 1;
        let n = idx.len();
        let ear = (0..n).find(|&i| {
            let (a, b, c) = (pts[idx[(i + n - 1) % n] as usize], pts[idx[i] as usize], pts[idx[(i + 1) % n] as usize]);
            cross(a, b, c) > 0.0
                && !idx.iter().any(|&j| {
                    let p = pts[j as usize];
                    p != a && p != b && p != c && cross(a, b, p) >= 0.0 && cross(b, c, p) >= 0.0 && cross(c, a, p) >= 0.0
                })
        });
        let Some(i) = ear else { break };
        m.add_triangle(idx[(i + n - 1) % n], idx[i], idx[(i + 1) % n]);
        idx.remove(i);
    }
    if idx.len() == 3 {
        m.add_triangle(idx[0], idx[1], idx[2]);
    }
    m
}

/// Draws `icon` inside `r` in the color `c` (Icons::Draw).
pub fn draw(p: &Painter, icon: Icon, r: Rect, c: Color32) {
    let g = Pen { p, r, k: r.width() / 24.0, c };
    match icon {
        Icon::Record => g.ellipse(5.0, 5.0, 14.0, 14.0, Some(c)),
        Icon::Pause => {
            g.fill_rect(6.0, 5.0, 4.0, 14.0, c);
            g.fill_rect(14.0, 5.0, 4.0, 14.0, c);
        }
        Icon::Stop => g.fill_rect(6.0, 6.0, 12.0, 12.0, c),
        Icon::Restart => {
            g.arc(5.0, 5.0, 14.0, 14.0, -60.0, 300.0);
            // the arrow head at the end of the arc
            let (tip, ang, sz) = ((18.5f32, 6.5f32), -0.2f32, 5.0f32);
            let pts = [tip, (tip.0 - sz * (ang - 0.45).cos(), tip.1 - sz * (ang - 0.45).sin()),
                       (tip.0 - sz * (ang + 0.45).cos(), tip.1 - sz * (ang + 0.45).sin())];
            g.polygon(&pts, Some(c), false);
        }
        Icon::Cancel => {
            g.line(6.0, 6.0, 18.0, 18.0);
            g.line(18.0, 6.0, 6.0, 18.0);
        }
        Icon::Mic | Icon::MicOff => {
            g.rect(9.0, 3.0, 6.0, 11.0);
            g.arc(6.0, 6.0, 12.0, 11.0, 0.0, 180.0);
            g.line(12.0, 17.0, 12.0, 21.0);
            g.line(8.0, 21.0, 16.0, 21.0);
            if icon == Icon::MicOff {
                g.line(4.0, 4.0, 20.0, 20.0);
            }
        }
        Icon::Speaker | Icon::SpeakerOff => {
            g.polygon(&[(3.0, 9.0), (7.0, 9.0), (12.0, 4.0), (12.0, 20.0), (7.0, 15.0), (3.0, 15.0)], None, true);
            if icon == Icon::Speaker {
                g.arc(11.0, 8.0, 6.0, 8.0, -60.0, 120.0);
                g.arc(11.0, 5.0, 10.0, 14.0, -60.0, 120.0);
            } else {
                g.line(15.0, 9.0, 21.0, 15.0);
                g.line(21.0, 9.0, 15.0, 15.0);
            }
        }
        Icon::Camera => {
            g.rect(3.0, 7.0, 18.0, 12.0);
            g.rect(8.0, 4.0, 8.0, 3.0);
            g.ellipse(8.5, 9.5, 7.0, 7.0, None);
        }
        Icon::Webcam | Icon::WebcamOff => {
            g.ellipse(5.5, 3.0, 13.0, 13.0, None);
            g.ellipse(9.6, 7.1, 4.8, 4.8, Some(c));
            g.line(12.0, 16.0, 12.0, 20.5);
            g.line(7.5, 20.8, 16.5, 20.8);
            if icon == Icon::WebcamOff {
                g.line(3.5, 3.5, 20.5, 20.5);
            }
        }
        Icon::Sliders => {
            // volume: three sliders with knobs
            for i in 0..3 {
                let y = 6.0 + i as f32 * 6.0;
                let k = [15.0, 8.0, 13.0][i];
                g.line(3.0, y, 21.0, y);
                g.ellipse(k - 2.6, y - 2.6, 5.2, 5.2, Some(c));
            }
        }
        Icon::Cursor => {
            let s = [(5.0, 2.0), (5.0, 19.0), (9.5, 14.5), (12.5, 21.0), (15.0, 20.0), (12.0, 13.5), (18.0, 13.5)];
            g.polygon(&s, Some(g.faint()), true);
        }
        Icon::Focus => {
            // spotlight: a dark frame with a bright circle
            g.fill_rect(2.0, 4.0, 20.0, 16.0, g.faint());
            g.rect(2.0, 4.0, 20.0, 16.0);
            g.ellipse(8.0, 7.0, 10.0, 10.0, None);
        }
        Icon::Lock => {
            // a padlock: the shackle over a filled body
            g.arc(7.0, 3.0, 10.0, 12.0, 180.0, 180.0);
            g.line(7.0, 9.0, 7.0, 11.0);
            g.line(17.0, 9.0, 17.0, 11.0);
            g.fill_rect(5.0, 11.0, 14.0, 10.0, c);
        }
        Icon::Pin => {
            // a push pin: the head, the collar and the needle
            g.fill_rect(8.0, 3.0, 8.0, 7.0, c);
            g.fill_rect(5.0, 10.0, 14.0, 3.0, c);
            g.line(12.0, 13.0, 12.0, 21.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    #[test]
    fn triangulates_the_cursor_arrow() {
        let s = [(5.0, 2.0), (5.0, 19.0), (9.5, 14.5), (12.5, 21.0), (15.0, 20.0), (12.0, 13.5), (18.0, 13.5)];
        let pts: Vec<Pos2> = s.iter().map(|&(x, y)| pos2(x, y)).collect();
        let m = triangulate(&pts, Color32::WHITE);
        assert_eq!(m.indices.len(), (s.len() - 2) * 3, "n − 2 triangles");
        // the triangles cover the polygon's area exactly
        let area = |a: Pos2, b: Pos2, c: Pos2| ((b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)).abs() / 2.0;
        let tri: f32 = m.indices.chunks(3).map(|t| area(pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize])).sum();
        let poly: f32 = (0..pts.len()).map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            a.x * b.y - b.x * a.y
        }).sum::<f32>().abs() / 2.0;
        assert!((tri - poly).abs() < 0.01, "{tri} vs {poly}");
    }
}
