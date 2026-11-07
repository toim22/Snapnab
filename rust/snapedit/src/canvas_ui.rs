//! The canvas on screen (Canvas::Paint and the window procedure of src/editor/Canvas.cpp): the composed image as GPU
//! textures, the overlays (selection, handles, crop, rulers…) drawn with egui's painter, and the mouse and keys fed
//! to `Canvas`, which works in physical pixels of the canvas area.

use std::sync::Arc;
use std::time::Instant;

use egui::{Color32, CursorIcon, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, TextureHandle, TextureOptions, Vec2, pos2, vec2};
use snapcore::doc::{DrawObj, ObjType, RectF};
use snapcore::image::{Image, ca, cb, cg, cr};

use crate::canvas::{Canvas, Cursor, Drag, Mods, P, obj_extent};
use crate::tr;

const BLUE: Color32 = Color32::from_rgb(30, 120, 230);

fn opts() -> TextureOptions {
    // nearest when enlarged (the pixels stay sharp), linear with mipmaps when reduced
    TextureOptions {
        magnification: egui::TextureFilter::Nearest,
        minification: egui::TextureFilter::Linear,
        wrap_mode: egui::TextureWrapMode::ClampToEdge,
        mipmap_mode: Some(egui::TextureFilter::Linear),
    }
}

fn color_image(img: &Image, x0: i32, y0: i32, w: i32, h: i32) -> egui::ColorImage {
    let mut px = Vec::with_capacity((w * h) as usize);
    for y in y0..y0 + h {
        for &p in &img.row(y)[x0 as usize..(x0 + w) as usize] {
            px.push(Color32::from_rgba_unmultiplied(cr(p) as u8, cg(p) as u8, cb(p) as u8, ca(p) as u8));
        }
    }
    egui::ColorImage::new([w as usize, h as usize], px)
}

/// One texture for a small image (thumbnails, previews).
pub fn texture(ctx: &egui::Context, name: &str, img: &Image, linear: bool) -> TextureHandle {
    let (w, h) = (img.wi().max(1), img.hi().max(1));
    let ci = if img.is_empty() { egui::ColorImage::new([1, 1], vec![Color32::TRANSPARENT]) } else { color_image(img, 0, 0, w, h) };
    ctx.load_texture(name, ci, if linear { TextureOptions::LINEAR } else { opts() })
}

/// An image cut into textures no larger than the GPU allows (a long scrolling capture is taller than that).
pub struct Tiled {
    /// (area in image pixels, texture)
    pub tiles: Vec<(Rect, TextureHandle)>,
}

impl Tiled {
    pub fn new(ctx: &egui::Context, name: &str, img: &Image) -> Tiled {
        let side = (ctx.input(|i| i.max_texture_side) as i32).clamp(256, 8192);
        let mut tiles = Vec::new();
        let mut y = 0;
        while y < img.hi() {
            let th = side.min(img.hi() - y);
            let mut x = 0;
            while x < img.wi() {
                let tw = side.min(img.wi() - x);
                let t = ctx.load_texture(format!("{name}-{x}-{y}"), color_image(img, x, y, tw, th), opts());
                tiles.push((Rect::from_min_size(pos2(x as f32, y as f32), vec2(tw as f32, th as f32)), t));
                x += tw;
            }
            y += th;
        }
        Tiled { tiles }
    }

    /// Draws the image so that image pixel (0,0) lands at `at` (points), `scale` points per image pixel.
    pub fn paint(&self, p: &egui::Painter, at: Pos2, scale: f32) {
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        for (r, t) in &self.tiles {
            let dst = Rect::from_min_size(at + r.min.to_vec2() * scale, r.size() * scale);
            p.image(t.id(), dst, uv, Color32::WHITE);
        }
    }
}

/// The canvas on screen: the textures and the mouse state between frames.
pub struct CanvasView {
    composed: Option<Tiled>,
    composed_ver: u64,
    has_alpha: bool,
    fx: Option<(Tiled, usize, i32, i32, bool)>,
    preview: Option<(TextureHandle, RectF)>,
    preview_ver: u64,
    overlay: Option<TextureHandle>,
    overlay_ver: u64,
    checker: Option<TextureHandle>,
    last_down: Option<(Instant, Pos2)>,
    pressed: bool,
    middle: bool,
    right: bool,
    /// the canvas area on screen (points)
    pub rect: Rect,
    pub ppp: f32,
    /// the in-place text box takes the focus on its first frame
    text_focus: bool,
}

fn to_c32(c: (u8, u8, u8)) -> Color32 {
    Color32::from_rgb(c.0, c.1, c.2)
}

fn colorref(c: u32) -> Color32 {
    Color32::from_rgb((c & 255) as u8, (c >> 8 & 255) as u8, (c >> 16 & 255) as u8)
}

fn dashed_rect(p: &egui::Painter, r: Rect, stroke: Stroke, dash: f32, gap: f32) {
    let pts = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
    p.extend(Shape::dashed_line(&pts, stroke, dash, gap));
}

/// The cursor shape for egui.
fn cursor_icon(c: Cursor) -> CursorIcon {
    match c {
        Cursor::Arrow => CursorIcon::Default,
        Cursor::Cross | Cursor::Dropper => CursorIcon::Crosshair,
        Cursor::SizeAll => CursorIcon::Move,
        Cursor::SizeNs => CursorIcon::ResizeVertical,
        Cursor::SizeWe => CursorIcon::ResizeHorizontal,
        Cursor::SizeNwse => CursorIcon::ResizeNwSe,
        Cursor::SizeNesw => CursorIcon::ResizeNeSw,
        Cursor::Hand => CursorIcon::PointingHand,
    }
}

/// The double-click time and distance of Windows.
fn double_click_ms() -> u128 {
    unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime() as u128 }
}

impl Default for CanvasView {
    fn default() -> Self {
        Self {
            composed: None,
            composed_ver: 0,
            has_alpha: false,
            fx: None,
            preview: None,
            preview_ver: 0,
            overlay: None,
            overlay_ver: 0,
            checker: None,
            last_down: None,
            pressed: false,
            middle: false,
            right: false,
            rect: Rect::NOTHING,
            ppp: 1.0,
            text_focus: false,
        }
    }
}

impl CanvasView {
    /// Physical pixels of the canvas area → points on screen.
    fn pt(&self, p: P) -> Pos2 {
        self.rect.min + vec2(p.0 as f32, p.1 as f32) / self.ppp
    }

    fn srect(&self, r: (i32, i32, i32, i32)) -> Rect {
        Rect::from_min_max(self.pt((r.0, r.1)), self.pt((r.2, r.3)))
    }

    fn phys(&self, pos: Pos2) -> P {
        let v = (pos - self.rect.min) * self.ppp;
        (v.x.round() as i32, v.y.round() as i32)
    }

    /// Draws the canvas in `rect` and feeds it the input; returns its response (right click opens the context menu
    /// through `Note::ContextMenu`).
    pub fn show(&mut self, ui: &mut egui::Ui, rect: Rect, c: &mut Canvas) -> egui::Response {
        self.ppp = ui.ctx().pixels_per_point();
        self.rect = rect;
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        ui.ctx().accesskit_node_builder(resp.id, |n| n.set_author_id("SnapnabCanvas".to_owned()));
        c.set_view_size((rect.width() * self.ppp).round() as i32, (rect.height() * self.ppp).round() as i32);
        self.input(ui, &resp, c);
        if let Some(t) = c.hold_at {
            let now = Instant::now();
            if now >= t {
                c.hold_at = None;
                c.recognize_hold();
            } else {
                ui.ctx().request_repaint_after(t - now);
            }
        }
        let painter = ui.painter_at(rect);
        self.paint(ui, &painter, c);
        self.text_box(ui, c);
        resp
    }

    fn input(&mut self, ui: &mut egui::Ui, resp: &egui::Response, c: &mut Canvas) {
        let events = ui.input(|i| i.events.clone());
        let mods_now = ui.input(|i| i.modifiers);
        let m = Mods { shift: mods_now.shift, ctrl: mods_now.ctrl, alt: mods_now.alt };
        let inside = |p: Pos2| self.rect.contains(p);
        let blocked = !resp.hovered() && !self.pressed;
        for e in events {
            match e {
                egui::Event::PointerButton { pos, button, pressed, modifiers } => {
                    let m = Mods { shift: modifiers.shift, ctrl: modifiers.ctrl, alt: modifiers.alt };
                    let p = self.phys(pos);
                    match (button, pressed) {
                        (egui::PointerButton::Primary, true) if inside(pos) && !blocked => {
                            if c.edit.is_some() && c.text_area_screen(self).is_some_and(|r| r.contains(pos)) {
                                continue; // a click inside the text box belongs to the text
                            }
                            let now = Instant::now();
                            let dbl = self.last_down.is_some_and(|(t, q)| {
                                now.duration_since(t).as_millis() <= double_click_ms() && (q - pos).length() * self.ppp <= 4.0
                            });
                            self.pressed = true;
                            if dbl {
                                self.last_down = None;
                                c.on_double(p, m);
                            } else {
                                self.last_down = Some((now, pos));
                                c.on_down(p, false, m);
                            }
                        }
                        (egui::PointerButton::Primary, false) if self.pressed => {
                            self.pressed = false;
                            c.on_up(p, m);
                        }
                        (egui::PointerButton::Middle, true) if inside(pos) => {
                            self.middle = true;
                            c.begin_pan(p);
                        }
                        (egui::PointerButton::Middle, false) if self.middle => {
                            self.middle = false;
                            c.end_pan();
                        }
                        (egui::PointerButton::Secondary, true) if inside(pos) && !blocked => {
                            self.right = true;
                            c.on_right_down(p);
                        }
                        (egui::PointerButton::Secondary, false) if self.right => {
                            self.right = false;
                            if inside(pos) && c.has_image() {
                                c.notes.push(crate::canvas::Note::ContextMenu(p.0, p.1));
                            }
                        }
                        _ => {}
                    }
                }
                egui::Event::PointerMoved(pos) => {
                    if self.pressed || self.middle || inside(pos) {
                        c.on_move(self.phys(pos), m);
                    }
                }
                egui::Event::PointerGone => {
                    if !self.pressed {
                        c.mouse = None;
                    }
                }
                egui::Event::MouseWheel { delta, modifiers, unit, .. } => {
                    let Some(pos) = ui.input(|i| i.pointer.hover_pos()) else { continue };
                    if !inside(pos) || blocked {
                        continue;
                    }
                    // a wheel notch is 120 (WHEEL_DELTA); egui reports lines (or points for touchpads)
                    let (dx, dy) = match unit {
                        egui::MouseWheelUnit::Line => (delta.x * 40.0, delta.y * 40.0),
                        egui::MouseWheelUnit::Page => (delta.x * 120.0, delta.y * 120.0),
                        egui::MouseWheelUnit::Point => (delta.x * self.ppp, delta.y * self.ppp),
                    };
                    let shift = modifiers.shift;
                    let d = if dy != 0.0 { dy } else { dx };
                    let d = if modifiers.ctrl { d * 3.0 } else { d };
                    c.on_wheel(self.phys(pos), d.round() as i32, modifiers.ctrl, shift || (dy == 0.0 && dx != 0.0));
                }
                _ => {}
            }
        }
        if resp.hovered() || self.pressed {
            if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
                let icon = cursor_icon(c.cursor_for_point(self.phys(pos)));
                ui.ctx().set_cursor_icon(icon);
            }
        }
    }

    fn paint(&mut self, ui: &egui::Ui, p: &egui::Painter, c: &mut Canvas) {
        let ctx = ui.ctx().clone();
        p.rect_filled(self.rect, 0.0, colorref(c.background));
        if c.doc().is_none_or(|d| d.is_video) {
            if c.doc().is_none() {
                p.text(self.rect.center(), egui::Align2::CENTER_CENTER,
                       tr("Nenhuma captura aberta — use “Capturar” ou abra um arquivo (Ctrl+O)."),
                       egui::FontId::proportional(14.0), Color32::from_gray(90));
            }
            return;
        }
        let s = 1.0 / self.ppp;
        if let Some(f) = c.fx_preview.clone().filter(|f| !f.is_empty()) {
            self.paint_fx(&ctx, p, c, &f);
            return;
        }
        c.recompose();
        if self.composed.is_none() || self.composed_ver != c.composed_version {
            self.composed = Some(Tiled::new(&ctx, "composed", &c.composed));
            self.composed_ver = c.composed_version;
            self.has_alpha = c.composed.has_transparency();
        }
        let zoom = c.zoom as f32;
        let o = c.origin();
        let (dw, dh) = c.doc().map_or((0, 0), |d| (d.w(), d.h()));
        let ir = Rect::from_min_size(self.pt(o), vec2(dw as f32 * zoom, dh as f32 * zoom) * s);
        // "paper" shadow
        p.rect_filled(ir.translate(vec2(3.0, 3.0) * s), 0.0, Color32::from_rgb(180, 182, 186));
        if self.has_alpha {
            self.checker(&ctx, p, ir);
        }
        if let Some(t) = &self.composed {
            t.paint(p, ir.min, zoom * s);
        }
        // the object being created, drawn over its part of the image
        if c.preview.is_some() {
            if self.preview_ver != c.preview_version || self.preview.is_none() {
                self.preview_ver = c.preview_version;
                self.preview = render_preview(&ctx, c);
            }
            if let Some((t, r)) = &self.preview {
                let a = self.pt(c.to_screen(snapcore::gdip::pt(r.X, r.Y)));
                let dst = Rect::from_min_size(a, vec2(r.Width, r.Height) * zoom * s);
                p.image(t.id(), dst, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
        } else {
            self.preview = None;
        }
        self.overlays(&ctx, p, c, ir);
        self.rulers(p, c);
    }

    fn checker(&mut self, ctx: &egui::Context, p: &egui::Painter, r: Rect) {
        let t = self.checker.get_or_insert_with(|| {
            let mut px = Vec::with_capacity(256);
            for y in 0..16 {
                for x in 0..16 {
                    px.push(if (x / 8 + y / 8) % 2 == 1 { Color32::WHITE } else { Color32::from_gray(0xDA) });
                }
            }
            ctx.load_texture("checker", egui::ColorImage::new([16, 16], px),
                             TextureOptions { wrap_mode: egui::TextureWrapMode::Repeat, ..TextureOptions::NEAREST })
        });
        let n = r.size() * self.ppp / 16.0;
        let mut mesh = egui::Mesh::with_texture(t.id());
        mesh.add_rect_with_uv(r, Rect::from_min_max(pos2(0.0, 0.0), pos2(n.x, n.y)), Color32::WHITE);
        p.add(mesh);
    }

    /// Effect preview: the resulting image, centered, at the current zoom (reduced if it does not fit); no handles.
    fn paint_fx(&mut self, ctx: &egui::Context, p: &egui::Painter, c: &Canvas, f: &Arc<Image>) {
        let key = Arc::as_ptr(f) as usize;
        if self.fx.as_ref().is_none_or(|x| x.1 != key || x.2 != f.wi() || x.3 != f.hi()) {
            self.fx = Some((Tiled::new(ctx, "fxpreview", f), key, f.wi(), f.hi(), f.has_transparency()));
        }
        let (vw, vh) = (c.view_w as f64, c.view_h as f64);
        let m = crate::canvas::MARGIN as f64;
        let z = c.zoom.min((vw - 2.0 * m) / f.w as f64).min((vh - 2.0 * m) / f.h as f64).max(0.02) as f32;
        let s = 1.0 / self.ppp;
        let size = vec2(f.w as f32 * z, f.h as f32 * z) * s;
        let ir = Rect::from_center_size(self.rect.center(), size);
        let alpha = self.fx.as_ref().is_some_and(|x| x.4);
        if alpha {
            self.checker(ctx, p, ir);
        } else {
            p.rect_filled(ir.translate(vec2(3.0, 3.0) * s), 0.0, Color32::from_rgb(180, 182, 186));
        }
        if let Some(x) = &self.fx {
            x.0.paint(p, ir.min, z * s);
        }
        let tag = Rect::from_center_size(pos2(self.rect.center().x, self.rect.top() + 23.0), vec2(260.0, 26.0));
        p.rect_filled(tag, 13.0, Color32::from_rgb(32, 32, 36));
        p.text(tag.center(), egui::Align2::CENTER_CENTER, tr("Prévia — Aplicar ou Cancelar no painel"), egui::FontId::proportional(12.0),
               Color32::WHITE);
    }

    fn overlays(&mut self, ctx: &egui::Context, p: &egui::Painter, c: &Canvas, ir: Rect) {
        let Some(d) = c.doc() else { return };
        let objs = &d.st.objs;
        let (origin, ppp) = (self.rect.min, self.ppp);
        let sr = move |r: RectF| {
            let r = c.to_screen_rect(r);
            Rect::from_min_max(origin + vec2(r.0 as f32, r.1 as f32) / ppp, origin + vec2(r.2 as f32, r.3 as f32) / ppp)
        };
        // lifted pieces: dashed frame while they float
        for o in objs.iter().filter(|o| o.floating && !o.hidden) {
            let r = sr(o.rect());
            p.rect_stroke(r, 0.0, Stroke::new(1.5, Color32::WHITE), StrokeKind::Middle);
            dashed_rect(p, r, Stroke::new(1.5, Color32::from_rgb(230, 120, 0)), 6.0, 4.0);
        }
        // link areas: outline and address under the cursor (and when selected)
        for (i, o) in objs.iter().enumerate() {
            if o.kind != ObjType::Link || o.hidden || (i as i32 != c.link_hot && !c.is_selected(i)) {
                continue;
            }
            let r = sr(o.rect());
            p.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(30, 120, 230, 40));
            dashed_rect(p, r, Stroke::new(1.2, BLUE), 6.0, 4.0);
            if i as i32 == c.link_hot {
                let tip = format!("{}   {}", String::from_utf16_lossy(&o.text), tr("(duplo clique abre)"));
                let font = egui::FontId::proportional(12.0);
                let g = p.layout_no_wrap(tip, font, Color32::WHITE);
                let tr_ = Rect::from_min_size(pos2(r.left(), r.bottom() + 4.0), vec2((g.size().x + 16.0).min(420.0), 22.0));
                p.rect_filled(tr_, 6.0, Color32::from_rgb(32, 34, 40));
                p.galley(pos2(tr_.left() + 8.0, tr_.center().y - g.size().y / 2.0), g, Color32::WHITE);
            }
        }
        // polygon being clicked: marks the first corner (clicking it closes the polygon)
        if c.drag == Drag::Polygon
            && let Some(pv) = &c.preview
            && !pv.pts.is_empty()
        {
            let pl: Vec<Pos2> = pv.pts.iter().map(|q| self.pt(c.to_screen(*q))).collect();
            if pl.len() > 1 {
                p.extend(Shape::dashed_line(&pl, Stroke::new(1.5, BLUE), 6.0, 4.0));
            }
            p.circle(pl[0], 5.0, Color32::WHITE, Stroke::new(2.0, BLUE));
        }
        // raster selection: blue highlight + dashed outline
        if c.has_sel && !c.sel_overlay.is_empty() {
            if self.overlay.is_none() || self.overlay_ver != c.sel_version {
                self.overlay = Some(texture(ctx, "sel-overlay", &c.sel_overlay, false));
                self.overlay_ver = c.sel_version;
            }
            let b = c.sel_bounds;
            let r = sr(snapcore::gdip::rect(b.left as f32, b.top as f32, b.width() as f32, b.height() as f32));
            if let Some(t) = &self.overlay {
                p.image(t.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            p.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::WHITE), StrokeKind::Middle);
            dashed_rect(p, r, Stroke::new(1.0, Color32::BLACK), 4.0, 4.0);
        }
        // text box: dashed while being dragged and while the text is typed
        let text_box = match (&c.preview, &c.edit) {
            (Some(pv), _) if pv.kind == ObjType::Text && c.drag == Drag::Create => Some(pv.rect()),
            (_, Some(e)) if e.idx < objs.len() && objs[e.idx].kind == ObjType::Text => Some(objs[e.idx].rect()),
            _ => None,
        };
        if let Some(r) = text_box {
            let r = sr(r);
            p.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::WHITE), StrokeKind::Middle);
            dashed_rect(p, r, Stroke::new(1.0, BLUE), 4.0, 3.0);
        }
        // object selection rectangle
        if c.drag == Drag::Marquee {
            let r = sr(c.sel_rect);
            p.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(30, 120, 230, 40));
            dashed_rect(p, r, Stroke::new(1.2, BLUE), 6.0, 4.0);
        }
        // several objects selected: outline of each one and of the area of the set
        let sel = c.selected_list();
        if c.edit.is_none() && sel.len() > 1 {
            let mut u: Option<RectF> = None;
            for &i in &sel {
                let b = objs[i].bounds();
                dashed_rect(p, sr(b), Stroke::new(1.4, Color32::from_rgba_unmultiplied(30, 120, 230, 220)), 2.0, 2.0);
                u = Some(u.map_or(b, |a| snapcore::doc::union(&a, &b)));
            }
            if let Some(u) = u {
                dashed_rect(p, sr(u).expand(3.0 / self.ppp), Stroke::new(1.0, BLUE), 6.0, 4.0);
            }
        }
        // selection in progress (rectangle/ellipse/lasso)
        if matches!(c.drag, Drag::Select | Drag::Lasso) {
            let st = Stroke::new(1.5, BLUE);
            if c.drag == Drag::Lasso && c.lasso.len() > 1 {
                let pts: Vec<Pos2> = c.lasso.iter().map(|q| self.pt(c.to_screen(*q))).collect();
                p.extend(Shape::dashed_line(&pts, st, 6.0, 4.0));
            } else {
                let r = sr(c.sel_rect);
                if c.sel_mode == 1 {
                    let n = 64;
                    let pts: Vec<Pos2> = (0..=n)
                        .map(|k| {
                            let a = k as f32 / n as f32 * std::f32::consts::TAU;
                            r.center() + vec2(a.cos() * r.width() / 2.0, a.sin() * r.height() / 2.0)
                        })
                        .collect();
                    p.extend(Shape::dashed_line(&pts, st, 6.0, 4.0));
                } else {
                    dashed_rect(p, r, st, 6.0, 4.0);
                }
            }
        }
        // crop: darkens the outside
        if c.has_crop || c.drag == Drag::Crop {
            let cr_ = sr(c.crop);
            let dim = Color32::from_rgba_unmultiplied(0, 0, 0, 140);
            let cl = cr_.intersect(ir);
            p.rect_filled(Rect::from_min_max(ir.min, pos2(ir.right(), cl.top().max(ir.top()))), 0.0, dim);
            p.rect_filled(Rect::from_min_max(pos2(ir.left(), cl.bottom().min(ir.bottom())), ir.max), 0.0, dim);
            p.rect_filled(Rect::from_min_max(pos2(ir.left(), cl.top()), pos2(cl.left(), cl.bottom())), 0.0, dim);
            p.rect_filled(Rect::from_min_max(pos2(cl.right(), cl.top()), pos2(ir.right(), cl.bottom())), 0.0, dim);
            p.rect_stroke(cr_, 0.0, Stroke::new(2.0, Color32::WHITE), StrokeKind::Middle);
            let t = tr("%d × %d  — Enter aplica, Esc cancela")
                .replacen("%d", &(c.crop.Width as i32).to_string(), 1)
                .replacen("%d", &(c.crop.Height as i32).to_string(), 1);
            let mut tag = Rect::from_min_size(pos2(cr_.left(), cr_.top() - 26.0), vec2(300.0, 22.0));
            if tag.top() < self.rect.top() {
                tag = tag.translate(vec2(0.0, 32.0));
            }
            p.rect_filled(tag, 8.0, Color32::from_rgb(30, 30, 30));
            p.text(tag.center(), egui::Align2::CENTER_CENTER, t, egui::FontId::proportional(12.0), Color32::WHITE);
        }
        // band of the cut-out
        if c.drag == Drag::Cut {
            let r = sr(c.sel_rect);
            let band = if c.cut_dir == 0 {
                Rect::from_min_max(pos2(ir.left(), r.top()), pos2(ir.right(), r.bottom()))
            } else {
                Rect::from_min_max(pos2(r.left(), ir.top()), pos2(r.right(), ir.bottom()))
            };
            p.rect_filled(band, 0.0, Color32::from_rgba_unmultiplied(230, 50, 50, 90));
        }
        if c.edit.is_some() {
            return;
        }
        // locked objects: lock badge on the corner, no handles
        for &i in &sel {
            let o = &objs[i];
            if !o.locked {
                continue;
            }
            let b = sr(o.bounds());
            let badge = Rect::from_center_size(b.right_top(), vec2(18.0, 18.0));
            p.circle(badge.center(), 9.0, Color32::WHITE, Stroke::new(1.0, BLUE));
            snapui::icons::draw(p, snapui::icons::Icon::Lock, badge.shrink(3.0), BLUE);
            if sel.len() == 1 {
                dashed_rect(p, b, Stroke::new(1.0, Color32::from_rgba_unmultiplied(30, 120, 230, 200)), 2.0, 2.0);
            }
        }
        // handles of the selected object
        if c.selected >= 0 && (c.selected as usize) < objs.len() && sel.len() == 1 && !objs[c.selected as usize].locked {
            let o: &DrawObj = &objs[c.selected as usize];
            if !o.is_line() {
                dashed_rect(p, sr(o.bounds()), Stroke::new(1.0, Color32::from_rgba_unmultiplied(30, 120, 230, 200)), 2.0, 2.0);
            }
            let hz = c.handle_size() as f32 / self.ppp;
            for (i, h) in c.handle_points(o).into_iter().enumerate() {
                let tail = (i == 8 && !o.is_line()) || (o.is_line() && i == 2);
                let fill = if tail { Color32::from_rgb(255, 200, 0) } else { Color32::WHITE };
                p.circle(self.pt(h), hz, fill, Stroke::new(1.5, BLUE));
            }
        }
    }

    /// Rulers (top and left, in image pixels), guides and the cursor marks.
    fn rulers(&self, p: &egui::Painter, c: &Canvas) {
        if !c.rulers {
            return;
        }
        let Some(d) = c.doc() else { return };
        let rs = c.ruler_size();
        if rs == 0 {
            return;
        }
        let s = 1.0 / self.ppp;
        let (l, t, r, b) = c.view();
        let view = self.srect((l, t, r, b));
        let gp = Stroke::new(1.0, Color32::from_rgb(0, 190, 255));
        let vp = p.with_clip_rect(view);
        for &x in &d.guides_x {
            let sx = self.pt(c.to_screen(snapcore::gdip::pt(x, 0.0))).x;
            vp.line_segment([pos2(sx, view.top()), pos2(sx, view.bottom())], gp);
        }
        for &y in &d.guides_y {
            let sy = self.pt(c.to_screen(snapcore::gdip::pt(0.0, y))).y;
            vp.line_segment([pos2(view.left(), sy), pos2(view.right(), sy)], gp);
        }
        let bg = to_c32((246, 247, 249));
        let line = Stroke::new(1.0, to_c32((160, 164, 172)));
        let text = to_c32((90, 94, 102));
        let rsp = rs as f32 * s;
        let top = Rect::from_min_max(self.rect.min, pos2(self.rect.right(), self.rect.top() + rsp));
        let left = Rect::from_min_max(self.rect.min, pos2(self.rect.left() + rsp, self.rect.bottom()));
        p.rect_filled(top, 0.0, bg);
        p.rect_filled(left, 0.0, bg);
        const STEPS: [i32; 15] = [1, 2, 5, 10, 20, 25, 50, 100, 200, 250, 500, 1000, 2000, 5000, 10000];
        let zoom = c.zoom;
        let step = STEPS.iter().copied().find(|st| *st as f64 * zoom >= 60.0 * self.ppp as f64).unwrap_or(10000);
        let mut minor = if step % 10 == 0 { step / 10 } else if step % 5 == 0 { step / 5 } else { step };
        if (minor as f64) * zoom < 5.0 {
            minor = step;
        }
        let a = c.to_doc((rs, rs));
        let z = c.to_doc((c.view_w, c.view_h));
        let font = egui::FontId::proportional(9.0);
        for axis in 0..2 {
            let (from, to) = if axis == 1 { (a.Y, z.Y) } else { (a.X, z.X) };
            let mut u = (from / minor as f32).floor() as i32 * minor;
            while u as f32 <= to {
                let sp = if axis == 1 {
                    c.to_screen(snapcore::gdip::pt(0.0, u as f32)).1
                } else {
                    c.to_screen(snapcore::gdip::pt(u as f32, 0.0)).0
                };
                if sp >= rs {
                    let major = u % step == 0;
                    let len = if major { rs } else { rs / 3 };
                    if axis == 1 {
                        p.line_segment([self.pt((rs - len, sp)), self.pt((rs, sp))], line);
                    } else {
                        p.line_segment([self.pt((sp, rs - len)), self.pt((sp, rs))], line);
                    }
                    if major {
                        let n = u.to_string();
                        if axis == 1 {
                            for (k, ch) in n.chars().enumerate() {
                                let at = self.pt((1, sp + 2)) + vec2(0.0, k as f32 * 8.0);
                                p.text(pos2(at.x + (rsp - 6.0) / 2.0, at.y), egui::Align2::CENTER_TOP, ch, font.clone(), text);
                            }
                        } else {
                            p.text(self.pt((sp + 3, 1)), egui::Align2::LEFT_TOP, n, font.clone(), text);
                        }
                    }
                }
                u += minor;
            }
        }
        p.line_segment([pos2(self.rect.left() + rsp, self.rect.top()), pos2(self.rect.left() + rsp, self.rect.bottom())], line);
        p.line_segment([pos2(self.rect.left(), self.rect.top() + rsp), pos2(self.rect.right(), self.rect.top() + rsp)], line);
        p.rect_filled(Rect::from_min_size(self.rect.min, Vec2::splat(rsp)), 0.0, bg);
        if let Some(m) = c.mouse {
            let red = Stroke::new(1.0, to_c32((230, 50, 50)));
            if m.0 >= rs {
                p.line_segment([self.pt((m.0, 0)), self.pt((m.0, rs))], red);
            }
            if m.1 >= rs {
                p.line_segment([self.pt((0, m.1)), self.pt((rs, m.1))], red);
            }
        }
    }

    /// The text typed in place: an egui text box over the object's text area.
    fn text_box(&mut self, ui: &mut egui::Ui, c: &mut Canvas) {
        let Some(r) = c.text_area_screen(self) else {
            self.text_focus = false;
            return;
        };
        let Some(e) = c.edit.clone() else { return };
        let Some(o) = c.doc().and_then(|d| d.st.objs.get(e.idx)).map(|o| (**o).clone()) else { return };
        let size = (o.style.font_size * c.zoom as f32 / self.ppp).clamp(4.0, 400.0);
        let tc = o.style.text_color;
        let color = Color32::from_rgb((tc >> 16 & 255) as u8, (tc >> 8 & 255) as u8, (tc & 255) as u8);
        let mut text = e.text.clone();
        let id = egui::Id::NULL.with("canvas-text-edit");
        let out = egui::TextEdit::multiline(&mut text)
            .id(id)
            .font(egui::FontId::proportional(size))
            .text_color(color)
            .frame(egui::Frame::NONE)
            .background_color(Color32::TRANSPARENT)
            .desired_width(r.width())
            .margin(egui::Margin::ZERO)
            .show(&mut ui.new_child(egui::UiBuilder::new().max_rect(r)));
        ui.ctx().accesskit_node_builder(out.response.id, |n| n.set_author_id("SnapnabCanvas.Text".to_owned()));
        if e.fresh {
            out.response.request_focus();
            let all = egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(text.chars().count()));
            let mut st = out.state;
            st.cursor.set_char_range(Some(all));
            st.store(ui.ctx(), id);
            if let Some(ed) = &mut c.edit {
                ed.fresh = false;
            }
        }
        if text != e.text {
            if let Some(ed) = &mut c.edit {
                ed.text = text;
            }
            c.layout_text_edit();
        }
    }
}

/// The object being created, rendered over its part of the composed image (the whole image for the objects whose
/// look depends on all of it).
fn render_preview(ctx: &egui::Context, c: &Canvas) -> Option<(TextureHandle, RectF)> {
    let o = c.preview.as_ref()?;
    let (w, h) = (c.composed.wi(), c.composed.hi());
    let whole = matches!(o.kind, ObjType::Spotlight | ObjType::Magnify);
    let r = if whole {
        snapcore::image::Rect { left: 0, top: 0, right: w, bottom: h }
    } else {
        let e = obj_extent(o);
        let m = (o.style.width * 3.0 + o.style.font_size + 40.0) as i32;
        snapcore::image::Rect {
            left: (e.X.min(e.X + e.Width) as i32 - m).max(0),
            top: (e.Y.min(e.Y + e.Height) as i32 - m).max(0),
            right: ((e.X.max(e.X + e.Width)) as i32 + m).min(w),
            bottom: ((e.Y.max(e.Y + e.Height)) as i32 + m).min(h),
        }
    };
    if r.width() <= 0 || r.height() <= 0 {
        return None;
    }
    let base = c.composed.crop(r);
    let mut moved = o.clone();
    moved.move_by(-r.left as f32, -r.top as f32);
    let img = snapcore::render::render(&base, &[moved], None);
    let t = texture(ctx, "preview", &img, false);
    Some((t, snapcore::gdip::rect(r.left as f32, r.top as f32, r.width() as f32, r.height() as f32)))
}

impl Canvas {
    /// The in-place text box on screen (points), while a text is typed.
    pub(crate) fn text_area_screen(&self, v: &CanvasView) -> Option<Rect> {
        let e = self.edit.as_ref()?;
        let o = self.doc()?.st.objs.get(e.idx)?;
        let a = self.text_area(o, Some(&e.text));
        let r = v.srect(self.to_screen_rect(a));
        Some(Rect::from_min_max(r.min, pos2(r.max.x.max(r.min.x + 20.0), r.max.y.max(r.min.y + 14.0))))
    }
}
