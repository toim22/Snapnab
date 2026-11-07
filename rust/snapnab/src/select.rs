//! The selection over the frozen screen (SelectOverlay.cpp, CCapturePreparer, comp_1 §4–§6): a window over all the
//! monitors shows the screen as it was, dimmed, with the area under the cursor lit; a drag selects a region, a click
//! takes the highlighted window or control, Enter the monitor. After a drag (QuickCrop off, or Ctrl held) the region
//! stays to be moved and resized with the mouse and the keyboard; Tab recalls the last regions (C18).
//!
//! Everything here is in the window's pixels (the client area starts at the virtual screen's corner); egui works in
//! points, so positions are multiplied by `pixels_per_point` on the way in and divided on the way out. The frozen
//! screen is uploaded once as textures (tiles when it is larger than the GPU allows) and drawn twice: dimmed under
//! everything, then at full brightness inside the selection.

use std::ffi::OsStr;
use std::sync::Arc;

use egui::{Align2, Color32, CursorIcon, FontId, Key, PointerButton, Pos2, Stroke, StrokeKind, TextureHandle, pos2, vec2};
use snapcore::image::{Image, Rect, cb, cg, cr};
use snapcore::{color, i18n, reg};

use crate::tr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Region,
    Window,
    Object,
    Fixed,
    Freehand,
    MultiArea,
    PickWindow,
}

/// A top-level window under the overlay (screen coordinates).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WinInfo {
    pub hwnd: isize,
    pub rect: Rect,
}

/// What was selected (screen coordinates).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selected {
    pub rect: Rect,
    /// top-level window under the click
    pub window: isize,
    /// innermost control under the click
    pub child: isize,
    /// freehand outline
    pub path: Vec<(i32, i32)>,
    /// multiple areas
    pub rects: Vec<Rect>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Selected(Selected),
    Canceled,
    /// H: the recent captures menu at this screen point (the app shows it and may cancel the selection)
    RecentCaptures((i32, i32)),
}

/// The innermost control at a screen point of a top-level window and its rectangle (None: the window itself).
pub type ChildAt = fn(isize, i32, i32) -> Option<(isize, Rect)>;

pub struct Setup {
    pub frozen: Arc<Image>,
    /// the virtual screen (the window covers it)
    pub vs: Rect,
    pub mode: Mode,
    pub fixed: (i32, i32),
    pub video: bool,
    /// the region is captured as soon as the mouse is released (QuickCrop); off, or with Ctrl, it stays adjustable
    pub quick_crop: bool,
    /// top-level windows, from the top
    pub windows: Vec<WinInfo>,
    pub monitors: Vec<Rect>,
    /// last regions, most recent first (screen)
    pub history: Vec<Rect>,
    /// cursor at the start (screen)
    pub cursor: (i32, i32),
    pub child_at: Option<ChildAt>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Grab {
    None,
    New,
    Move,
    Handle(usize),
}

pub struct Overlay {
    s: Setup,
    tiles: Vec<(Rect, TextureHandle)>,
    cur: (i32, i32),
    dragging: bool,
    drag_start: (i32, i32),
    hover: Rect,
    hover_wnd: isize,
    hover_child: isize,
    path: Vec<(i32, i32)>,
    rects: Vec<Rect>,
    adjusting: bool,
    sel: Rect,
    grab: Grab,
    grab_from: (i32, i32),
    sel_from: Rect,
    label: Rect,
    history: Vec<Rect>,
    hist_idx: Option<usize>,
    toast: String,
    toast_until: f64,
    size_edit: Option<(i32, i32)>,
    finished: bool,
}

fn norm(a: (i32, i32), b: (i32, i32)) -> Rect {
    Rect::new(a.0.min(b.0), a.1.min(b.1), a.0.max(b.0) + 1, a.1.max(b.1) + 1)
}

/// The 8 resize handles: corners and the middle of the sides, clockwise from the top left.
fn handle_rect(r: Rect, i: usize, s: i32) -> Rect {
    let xs = [r.left, (r.left + r.right) / 2, r.right];
    let ys = [r.top, (r.top + r.bottom) / 2, r.bottom];
    const HX: [usize; 8] = [0, 1, 2, 2, 2, 1, 0, 0];
    const HY: [usize; 8] = [0, 0, 0, 1, 2, 2, 2, 1];
    let (x, y) = (xs[HX[i]], ys[HY[i]]);
    Rect::new(x - s, y - s, x + s + 1, y + s + 1)
}

/// RegionHistory: "x,y,w,h;" × up to 10, most recent first.
pub fn region_history() -> Vec<Rect> {
    let s = reg::get_string(OsStr::new("RegionHistory"), None).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    s.split(';')
        .filter_map(|t| {
            let v: Vec<i32> = t.split(',').map(|n| n.trim().parse().ok()).collect::<Option<_>>()?;
            (v.len() == 4 && v[2] > 1 && v[3] > 1).then(|| Rect::new(v[0], v[1], v[0] + v[2], v[1] + v[3]))
        })
        .take(10)
        .collect()
}

/// RememberRegion
pub fn remember_region(r: Rect) {
    if r.width() < 2 || r.height() < 2 {
        return;
    }
    let mut v = region_history();
    v.retain(|x| *x != r);
    v.insert(0, r);
    v.truncate(10);
    let s: String = v.iter().map(|x| format!("{},{},{},{};", x.left, x.top, x.width(), x.height())).collect();
    reg::set_string(OsStr::new("RegionHistory"), OsStr::new(&s), None);
}

fn colorref(p: u32) -> u32 {
    cr(p) as u32 | (cg(p) as u32) << 8 | (cb(p) as u32) << 16
}

impl Overlay {
    pub fn new(s: Setup) -> Overlay {
        let vs = s.vs;
        let history = s.history.iter().filter(|r| !r.intersect(&vs).is_empty()).map(|r| r.offset(-vs.left, -vs.top)).collect();
        let cur = (s.cursor.0 - vs.left, s.cursor.1 - vs.top);
        let mut o = Overlay {
            s,
            tiles: Vec::new(),
            cur,
            dragging: false,
            drag_start: (0, 0),
            hover: Rect::default(),
            hover_wnd: 0,
            hover_child: 0,
            path: Vec::new(),
            rects: Vec::new(),
            adjusting: false,
            sel: Rect::default(),
            grab: Grab::None,
            grab_from: (0, 0),
            sel_from: Rect::default(),
            label: Rect::default(),
            history,
            hist_idx: None,
            toast: String::new(),
            toast_until: 0.0,
            size_edit: None,
            finished: false,
        };
        o.update_hover();
        o
    }

    fn size(&self) -> (i32, i32) {
        (self.s.vs.width(), self.s.vs.height())
    }

    /// the monitor under a point of the window
    fn monitor_at(&self, p: (i32, i32)) -> Rect {
        let (sx, sy) = (p.0 + self.s.vs.left, p.1 + self.s.vs.top);
        let m = self.s.monitors.iter().find(|m| m.contains(sx, sy)).copied().unwrap_or(self.s.vs);
        m.offset(-self.s.vs.left, -self.s.vs.top)
    }

    /// UpdateHover: the window (or control) under the cursor, or the whole monitor.
    fn update_hover(&mut self) {
        let (sx, sy) = (self.cur.0 + self.s.vs.left, self.cur.1 + self.s.vs.top);
        self.hover = Rect::default();
        self.hover_wnd = 0;
        self.hover_child = 0;
        if let Some(w) = self.s.windows.iter().find(|w| w.rect.contains(sx, sy)).copied() {
            self.hover_wnd = w.hwnd;
            let mut r = w.rect;
            if matches!(self.s.mode, Mode::Object | Mode::Region | Mode::PickWindow) {
                if let Some((c, cr)) = self.s.child_at.and_then(|f| f(w.hwnd, sx, sy)) {
                    self.hover_child = c;
                    if self.s.mode != Mode::Region {
                        r = cr;
                    }
                }
                if self.s.mode == Mode::PickWindow && self.hover_child == 0 {
                    self.hover_child = w.hwnd;
                }
            }
            self.hover = r.offset(-self.s.vs.left, -self.s.vs.top);
            return;
        }
        self.hover = self.monitor_at(self.cur);
    }

    fn fixed_rect(&self) -> Rect {
        let (fw, fh) = self.s.fixed;
        let mut r = Rect::new(self.cur.0 - fw / 2, self.cur.1 - fh / 2, 0, 0);
        r.right = r.left + fw;
        r.bottom = r.top + fh;
        let (w, h) = self.size();
        if r.left < 0 {
            r = r.offset(-r.left, 0);
        }
        if r.top < 0 {
            r = r.offset(0, -r.top);
        }
        if r.right > w {
            r = r.offset(w - r.right, 0);
        }
        if r.bottom > h {
            r = r.offset(0, h - r.bottom);
        }
        r
    }

    fn clamp_sel(&self, mut r: Rect) -> Rect {
        let (w, h) = self.size();
        if r.width() > w {
            r.left = 0;
            r.right = w;
        }
        if r.height() > h {
            r.top = 0;
            r.bottom = h;
        }
        if r.left < 0 {
            r = r.offset(-r.left, 0);
        }
        if r.top < 0 {
            r = r.offset(0, -r.top);
        }
        if r.right > w {
            r = r.offset(w - r.right, 0);
        }
        if r.bottom > h {
            r = r.offset(0, h - r.bottom);
        }
        r
    }

    fn start_adjust(&mut self, r: Rect) {
        self.sel = self.clamp_sel(r);
        self.adjusting = true;
        self.dragging = false;
        self.grab = Grab::None;
    }

    fn hit_handle(&self, p: (i32, i32), ppp: f32) -> Option<usize> {
        let s = (6.0 * ppp).round() as i32;
        (0..8).find(|&i| handle_rect(self.sel, i, s).contains(p.0, p.1))
    }

    fn toast(&mut self, t: String, now: f64) {
        self.toast = t;
        self.toast_until = now + 1.6;
    }

    /// Finish: the selection in screen coordinates (None when there is nothing to capture).
    fn finish(&mut self, ok: bool) -> Outcome {
        self.finished = true;
        snapcore::log::write(&format!("Selection: finished ({})", if ok { "ok" } else { "canceled" }));
        if !ok {
            return Outcome::Canceled;
        }
        let vs = self.s.vs;
        let mut res = Selected::default();
        let r = match self.s.mode {
            Mode::Fixed => self.fixed_rect(),
            Mode::Freehand => {
                if self.path.len() < 3 {
                    return Outcome::Canceled;
                }
                let mut b = Rect::new(i32::MAX, i32::MAX, i32::MIN, i32::MIN);
                for &(x, y) in &self.path {
                    b = Rect::new(b.left.min(x), b.top.min(y), b.right.max(x + 1), b.bottom.max(y + 1));
                    res.path.push((x + vs.left, y + vs.top));
                }
                b
            }
            Mode::MultiArea => {
                let Some(first) = self.rects.first().copied() else { return Outcome::Canceled };
                let mut b = first;
                for x in &self.rects {
                    b = b.union(x);
                    res.rects.push(x.offset(vs.left, vs.top));
                }
                b
            }
            _ if self.adjusting => self.sel,
            _ if self.dragging => norm(self.drag_start, self.cur),
            _ => self.hover,
        };
        res.rect = r.offset(vs.left, vs.top);
        res.window = self.hover_wnd;
        res.child = self.hover_child;
        if res.rect.width() < 2 || res.rect.height() < 2 {
            return Outcome::Canceled;
        }
        Outcome::Selected(res)
    }

    /// The window's frame: input, then drawing. Returns once the selection is over.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<Outcome> {
        if self.finished {
            return None;
        }
        let ppp = ui.ctx().pixels_per_point();
        let now = ui.input(|i| i.time);
        self.upload(ui.ctx());
        if let Some(o) = self.input(ui, ppp, now) {
            return Some(o);
        }
        if let Some(o) = self.size_dialog(ui) {
            return Some(o);
        }
        self.paint(ui, ppp, now);
        ui.ctx().set_cursor_icon(self.cursor_icon(ppp));
        None
    }

    fn cursor_icon(&self, ppp: f32) -> CursorIcon {
        let mut c = if matches!(self.s.mode, Mode::Window | Mode::PickWindow) { CursorIcon::PointingHand } else { CursorIcon::Crosshair };
        if self.adjusting {
            let h = match self.grab {
                Grab::Handle(i) => Some(i),
                _ => self.hit_handle(self.cur, ppp),
            };
            const RC: [CursorIcon; 8] = [
                CursorIcon::ResizeNwSe,
                CursorIcon::ResizeVertical,
                CursorIcon::ResizeNeSw,
                CursorIcon::ResizeHorizontal,
                CursorIcon::ResizeNwSe,
                CursorIcon::ResizeVertical,
                CursorIcon::ResizeNeSw,
                CursorIcon::ResizeHorizontal,
            ];
            if let Some(h) = h {
                c = RC[h];
            } else if self.label.contains(self.cur.0, self.cur.1) {
                c = CursorIcon::PointingHand;
            } else if self.sel.contains(self.cur.0, self.cur.1) || self.grab == Grab::Move {
                c = CursorIcon::Move;
            }
        }
        c
    }

    /// The frozen screen as textures (once).
    fn upload(&mut self, ctx: &egui::Context) {
        if !self.tiles.is_empty() || self.s.frozen.is_empty() {
            return;
        }
        let img = &self.s.frozen;
        let side = ctx.input(|i| i.max_texture_side).max(256) as i32;
        let mut ty = 0;
        while ty < img.hi() {
            let mut tx = 0;
            while tx < img.wi() {
                let r = Rect::new(tx, ty, (tx + side).min(img.wi()), (ty + side).min(img.hi()));
                let mut px = Vec::with_capacity((r.width() * r.height()) as usize);
                for y in r.top..r.bottom {
                    px.extend(img.row(y)[r.left as usize..r.right as usize].iter().map(|&p| {
                        Color32::from_rgb(cr(p) as u8, cg(p) as u8, cb(p) as u8)
                    }));
                }
                let ci = egui::ColorImage::new([r.width() as usize, r.height() as usize], px);
                self.tiles.push((r, ctx.load_texture(format!("frozen{tx},{ty}"), ci, egui::TextureOptions::NEAREST)));
                tx += side;
            }
            ty += side;
        }
    }

    fn input(&mut self, ui: &mut egui::Ui, ppp: f32, now: f64) -> Option<Outcome> {
        let (events, double) = ui.input(|i| (i.events.clone(), i.pointer.button_double_clicked(PointerButton::Primary)));
        let px = |p: Pos2| ((p.x * ppp).floor() as i32, (p.y * ppp).floor() as i32);
        let editing = self.size_edit.is_some();
        for e in events {
            match e {
                egui::Event::PointerMoved(p) if !editing => {
                    self.cur = px(p);
                    self.mouse_move();
                }
                egui::Event::PointerButton { pos, button, pressed, modifiers } if !editing => {
                    self.cur = px(pos);
                    match (button, pressed) {
                        (PointerButton::Primary, true) => {
                            if double && self.adjusting && self.sel.contains(self.cur.0, self.cur.1) {
                                return Some(self.finish(true));
                            }
                            self.mouse_down(ppp);
                        }
                        (PointerButton::Primary, false) => {
                            if let Some(o) = self.mouse_up(modifiers.ctrl) {
                                return Some(o);
                            }
                        }
                        (PointerButton::Secondary, false) => {
                            let ok = self.s.mode == Mode::MultiArea && !self.rects.is_empty();
                            return Some(self.finish(ok));
                        }
                        _ => {}
                    }
                }
                egui::Event::Key { key, pressed: true, modifiers, .. } if !editing || key == Key::Escape => {
                    if editing {
                        self.size_edit = None;
                        continue;
                    }
                    if let Some(o) = self.key(key, modifiers.ctrl, modifiers.shift, now) {
                        return Some(o);
                    }
                }
                _ => {}
            }
        }
        if self.toast_until > now {
            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(self.toast_until - now));
        }
        None
    }

    fn mouse_move(&mut self) {
        if self.adjusting && matches!(self.grab, Grab::Move | Grab::Handle(_)) {
            let (dx, dy) = (self.cur.0 - self.grab_from.0, self.cur.1 - self.grab_from.1);
            let mut r = self.sel_from;
            match self.grab {
                Grab::Move => r = self.clamp_sel(r.offset(dx, dy)),
                Grab::Handle(h) => {
                    const SX: [i32; 8] = [-1, 0, 1, 1, 1, 0, -1, -1];
                    const SY: [i32; 8] = [-1, -1, -1, 0, 1, 1, 1, 0];
                    if SX[h] < 0 {
                        r.left += dx;
                    }
                    if SX[h] > 0 {
                        r.right += dx;
                    }
                    if SY[h] < 0 {
                        r.top += dy;
                    }
                    if SY[h] > 0 {
                        r.bottom += dy;
                    }
                    r = Rect::new(r.left.min(r.right), r.top.min(r.bottom), r.left.max(r.right), r.top.max(r.bottom));
                }
                _ => {}
            }
            self.sel = r;
            return;
        }
        if self.s.mode == Mode::Freehand && self.dragging {
            self.path.push(self.cur);
        }
        if !self.dragging && !self.adjusting {
            self.update_hover();
        }
    }

    fn mouse_down(&mut self, ppp: f32) {
        if self.adjusting {
            self.grab_from = self.cur;
            self.sel_from = self.sel;
            if self.label.contains(self.cur.0, self.cur.1) {
                self.grab = Grab::None;
                return;
            }
            if let Some(h) = self.hit_handle(self.cur, ppp) {
                self.grab = Grab::Handle(h);
                return;
            }
            if self.sel.contains(self.cur.0, self.cur.1) {
                self.grab = Grab::Move;
                return;
            }
            self.adjusting = false; // outside: a new selection
        }
        if matches!(self.s.mode, Mode::Fixed | Mode::Window | Mode::Object | Mode::PickWindow) {
            return;
        }
        self.dragging = true;
        self.drag_start = self.cur;
        self.grab = Grab::New;
        if self.s.mode == Mode::Freehand {
            self.path = vec![self.cur];
        }
    }

    fn mouse_up(&mut self, ctrl: bool) -> Option<Outcome> {
        if self.adjusting {
            if self.grab == Grab::None && self.label.contains(self.cur.0, self.cur.1) {
                self.size_edit = Some((self.sel.width(), self.sel.height())); // types the exact size
            }
            self.grab = Grab::None;
            return None;
        }
        let moved = self.dragging && ((self.cur.0 - self.drag_start.0).abs() > 3 || (self.cur.1 - self.drag_start.1).abs() > 3);
        self.grab = Grab::None;
        if moved && self.s.mode == Mode::Region && (!self.s.quick_crop || ctrl) {
            self.start_adjust(norm(self.drag_start, self.cur)); // stays to be adjusted; Enter captures
            return None;
        }
        if self.s.mode == Mode::MultiArea {
            if moved {
                self.rects.push(norm(self.drag_start, self.cur));
            }
            self.dragging = false;
            return None;
        }
        if self.s.mode == Mode::Freehand {
            return Some(self.finish(true));
        }
        if !moved {
            self.dragging = false; // a single click: the highlighted area
        }
        Some(self.finish(true))
    }

    fn key(&mut self, key: Key, ctrl: bool, shift: bool, now: f64) -> Option<Outcome> {
        if self.s.mode == Mode::Region && key == Key::Tab {
            // previous regions (C18)
            if self.history.is_empty() {
                self.toast(tr("Nenhuma região anterior ainda"), now);
                return None;
            }
            let n = self.history.len();
            let i = match self.hist_idx {
                None => if shift { n - 1 } else { 0 },
                Some(i) => (i + if shift { n - 1 } else { 1 }) % n,
            };
            self.hist_idx = Some(i);
            self.start_adjust(self.history[i]);
            self.toast(i18n::fill(&tr("Região anterior %d de %d"), &[&(i + 1), &n]), now);
            return None;
        }
        if key == Key::C && !ctrl {
            // the color under the cursor (C10)
            let f = &self.s.frozen;
            if !f.is_empty() {
                let p = f.at(self.cur.0.clamp(0, f.wi() - 1), self.cur.1.clamp(0, f.hi() - 1));
                let c = colorref(p);
                let t = color::to_text(c, color::default_format());
                snapcore::clip::copy_text(0, &t);
                color::add_recent(c);
                self.toast(format!("{}{t}", tr("Cor copiada: ")), now);
            }
            return None;
        }
        if key == Key::H && !ctrl {
            return Some(Outcome::RecentCaptures((self.cur.0 + self.s.vs.left, self.cur.1 + self.s.vs.top)));
        }
        if self.adjusting {
            match key {
                Key::Enter => return Some(self.finish(true)),
                Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown => {
                    let mut r = self.sel;
                    let dx = match key { Key::ArrowLeft => -1, Key::ArrowRight => 1, _ => 0 };
                    let dy = match key { Key::ArrowUp => -1, Key::ArrowDown => 1, _ => 0 };
                    if ctrl {
                        // the edge on that side grows (Shift: shrinks)
                        let k = if shift { -1 } else { 1 };
                        if dx < 0 {
                            r.left -= k;
                        }
                        if dx > 0 {
                            r.right += k;
                        }
                        if dy < 0 {
                            r.top -= k;
                        }
                        if dy > 0 {
                            r.bottom += k;
                        }
                        if r.width() < 2 || r.height() < 2 {
                            return None;
                        }
                    } else {
                        let step = if shift { 10 } else { 1 };
                        r = r.offset(dx * step, dy * step);
                    }
                    self.sel = self.clamp_sel(r);
                    return None;
                }
                _ => {}
            }
        }
        match key {
            Key::Escape => return Some(self.finish(false)),
            Key::Enter => {
                if self.s.mode == Mode::MultiArea {
                    let ok = !self.rects.is_empty();
                    return Some(self.finish(ok));
                }
                if self.s.mode == Mode::Region {
                    self.dragging = false;
                    self.hover = self.monitor_at(self.cur);
                    self.hover_wnd = 0;
                    return Some(self.finish(true));
                }
            }
            Key::Backspace => {
                self.rects.pop();
            }
            Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown => {
                let step = if shift { 10 } else { 1 };
                let (mut x, mut y) = snapcore::screen::cursor_pos();
                match key {
                    Key::ArrowLeft => x -= step,
                    Key::ArrowRight => x += step,
                    Key::ArrowUp => y -= step,
                    _ => y += step,
                }
                unsafe {
                    let _ = windows::Win32::UI::WindowsAndMessaging::SetCursorPos(x, y);
                }
            }
            _ => {}
        }
        None
    }

    /// The size typed after a click on the size label.
    fn size_dialog(&mut self, ui: &mut egui::Ui) -> Option<Outcome> {
        let (mut w, mut h) = self.size_edit?;
        let (vw, vh) = self.size();
        let mut done = None;
        egui::Window::new(tr("Tamanho da seleção"))
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ui.ctx(), |ui| {
                egui::Grid::new("size").num_columns(2).show(ui, |ui| {
                    let l = ui.label(tr("Largura (px)"));
                    ui.add(egui::DragValue::new(&mut w).range(2..=vw)).labelled_by(l.id);
                    ui.end_row();
                    let l = ui.label(tr("Altura (px)"));
                    ui.add(egui::DragValue::new(&mut h).range(2..=vh)).labelled_by(l.id);
                    ui.end_row();
                });
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                        done = Some(true);
                    }
                    if ui.button(tr("Cancelar")).clicked() {
                        done = Some(false);
                    }
                });
            });
        self.size_edit = Some((w, h));
        match done {
            Some(true) => {
                let r = Rect::new(self.sel.left, self.sel.top, self.sel.left + w, self.sel.top + h);
                self.sel = self.clamp_sel(r);
                self.size_edit = None;
            }
            Some(false) => self.size_edit = None,
            None => {}
        }
        None
    }

    /// Draws the part `src` of the frozen screen (pixels) into `dst` (points), tile by tile.
    fn draw_frozen(&self, p: &egui::Painter, src: Rect, dst: egui::Rect, tint: Color32) {
        if src.is_empty() {
            return;
        }
        let (kx, ky) = (dst.width() / src.width() as f32, dst.height() / src.height() as f32);
        for (tr, tex) in &self.tiles {
            let x = src.intersect(tr);
            if x.is_empty() {
                continue;
            }
            let uv = egui::Rect::from_min_max(
                pos2((x.left - tr.left) as f32 / tr.width() as f32, (x.top - tr.top) as f32 / tr.height() as f32),
                pos2((x.right - tr.left) as f32 / tr.width() as f32, (x.bottom - tr.top) as f32 / tr.height() as f32),
            );
            let d = egui::Rect::from_min_max(
                dst.min + vec2((x.left - src.left) as f32 * kx, (x.top - src.top) as f32 * ky),
                dst.min + vec2((x.right - src.left) as f32 * kx, (x.bottom - src.top) as f32 * ky),
            );
            p.image(tex.id(), d, uv, tint);
        }
    }

    fn paint(&mut self, ui: &mut egui::Ui, ppp: f32, now: f64) {
        let p = ui.painter().clone();
        let pt = |x: i32, y: i32| pos2(x as f32 / ppp, y as f32 / ppp);
        let rp = |r: Rect| egui::Rect::from_min_max(pt(r.left, r.top), pt(r.right, r.bottom));
        let (w, h) = self.size();
        let all = Rect::new(0, 0, w, h);
        p.rect_filled(rp(all), 0.0, Color32::BLACK);
        self.draw_frozen(&p, all, rp(all), Color32::from_gray(115)); // 45 % brightness
        let accent = if self.s.video { Color32::from_rgb(235, 60, 60) } else { Color32::from_rgb(30, 144, 255) };
        let line = Stroke::new(2.0f32.max(2.0 * ppp) / ppp, accent);
        let lit = |r: Rect| {
            self.draw_frozen(&p, r, rp(r), Color32::WHITE);
            p.rect_stroke(rp(r), 0.0, line, StrokeKind::Middle);
        };
        let mut sel = None;
        match self.s.mode {
            Mode::Fixed => {
                let r = self.fixed_rect();
                lit(r);
                sel = Some(r);
            }
            Mode::Freehand => {
                if self.path.len() > 1 {
                    p.add(egui::Shape::line(self.path.iter().map(|&(x, y)| pt(x, y)).collect(), line));
                }
            }
            Mode::MultiArea => {
                for &r in &self.rects {
                    lit(r);
                }
                if self.dragging {
                    let r = norm(self.drag_start, self.cur);
                    lit(r);
                    sel = Some(r);
                }
            }
            _ => {
                let r = if self.adjusting {
                    Some(self.sel)
                } else if self.dragging {
                    Some(norm(self.drag_start, self.cur))
                } else {
                    (!self.hover.is_empty()).then_some(self.hover)
                };
                if let Some(r) = r {
                    lit(r);
                    sel = Some(r);
                }
            }
        }
        if self.adjusting {
            let s = (4.0 * ppp).round() as i32;
            for i in 0..8 {
                let hr = rp(handle_rect(self.sel, i, s));
                p.rect_filled(hr, 0.0, Color32::WHITE);
                p.rect_stroke(hr, 0.0, Stroke::new(1.0 / ppp, accent), StrokeKind::Inside);
            }
        }
        // crosshair
        if !matches!(self.s.mode, Mode::Window | Mode::PickWindow) && !self.adjusting {
            let c = pt(self.cur.0, self.cur.1);
            let (fw, fh) = (w as f32 / ppp, h as f32 / ppp);
            for seg in [[pos2(0.0, c.y), pos2(fw, c.y)], [pos2(c.x, 0.0), pos2(c.x, fh)]] {
                p.line_segment(seg, Stroke::new(1.0 / ppp, Color32::from_black_alpha(160)));
                p.extend(egui::Shape::dashed_line(&seg, Stroke::new(1.0 / ppp, Color32::WHITE), 2.0, 2.0));
            }
        }
        let font = |pt_size: f32| FontId::proportional(pt_size * 4.0 / 3.0);
        // size label
        self.label = Rect::default();
        if let Some(r) = sel {
            let mut t = format!(" {} × {} ", r.width(), r.height());
            if self.adjusting {
                t.push_str(" ✎ "); // a click types the size
            }
            let sc = |v: f32| (v * ppp).round() as i32;
            let mut lr = Rect::new(r.left, r.top - sc(26.0), r.left + sc(if self.adjusting { 140.0 } else { 120.0 }), r.top - sc(4.0));
            if lr.top < 0 {
                lr = lr.offset(0, r.top - lr.top + sc(6.0));
            }
            p.rect_filled(rp(lr), 8.0, accent);
            p.text(rp(lr).center(), Align2::CENTER_CENTER, t, FontId::proportional(12.0), Color32::WHITE);
            self.label = lr;
        }
        // magnifier
        if !self.adjusting || self.grab != Grab::None {
            self.magnifier(&p, ppp);
        }
        // hint
        let hint = match self.s.mode {
            Mode::Region if self.adjusting => tr("Setas movem (Shift = 10 px) • Ctrl+setas aumentam, Ctrl+Shift+setas diminuem • clique no tamanho para digitar • Enter ou duplo clique captura • Esc = cancelar"),
            Mode::Region => tr("Arraste para selecionar • clique captura a janela/área destacada • Enter = monitor • Tab = regiões anteriores • H = capturas recentes • C = copiar a cor • Esc = cancelar"),
            Mode::Window => tr("Clique na janela a capturar • Esc = cancelar"),
            Mode::Object => tr("Clique no controle/objeto a capturar • Esc = cancelar"),
            Mode::Fixed => i18n::fill(&tr("Região fixa %d × %d — clique para capturar • Esc = cancelar"), &[&self.s.fixed.0, &self.s.fixed.1]),
            Mode::Freehand => tr("Desenhe o contorno com o botão pressionado • Esc = cancelar"),
            Mode::MultiArea => tr("Arraste várias áreas • Backspace desfaz • Enter ou botão direito conclui • Esc = cancelar"),
            Mode::PickWindow => tr("Clique na janela/área rolável • Esc = cancelar"),
        };
        let hint = if self.s.video { format!("{}{hint}", tr("Gravação: ")) } else { hint };
        let mon = rp(self.monitor_at(self.cur));
        // one line when it fits in the monitor, wrapped when it does not
        let galley = p.layout(hint, font(9.0), Color32::from_gray(235), (mon.width() - 72.0).max(100.0));
        let hw = (galley.size().x + 40.0).max(760.0 / ppp.max(1.0)).min(mon.width() - 32.0);
        let hr = egui::Rect::from_min_size(pos2(mon.center().x - hw / 2.0, mon.top() + 16.0),
                                           vec2(hw, (galley.size().y + 14.0).max(32.0)));
        p.rect(hr, 14.0, Color32::from_gray(28), Stroke::new(1.0, Color32::from_gray(80)), StrokeKind::Inside);
        p.galley(hr.center() - galley.size() / 2.0, galley, Color32::WHITE);
        if !self.toast.is_empty() && now < self.toast_until {
            let tr_ = egui::Rect::from_center_size(pos2(hr.center().x, hr.bottom() + 8.0 + 16.0), vec2(360.0, 32.0));
            p.rect_filled(tr_, 14.0, accent);
            p.text(tr_.center(), Align2::CENTER_CENTER, &self.toast, FontId::proportional(12.0), Color32::WHITE);
        }
    }

    /// 15×15 pixels around the cursor, 8× larger, with the position and the color under it.
    fn magnifier(&self, p: &egui::Painter, ppp: f32) {
        const Z: f32 = 8.0;
        const N: i32 = 15;
        let (w, h) = self.size();
        let (fw, fh) = (w as f32 / ppp, h as f32 / ppp);
        let c = pos2(self.cur.0 as f32 / ppp, self.cur.1 as f32 / ppp);
        let bx_size = Z * N as f32 / ppp; // 8 screen pixels per frozen pixel
        let lh = 44.0;
        let mut b = pos2(c.x + 24.0, c.y + 24.0);
        if b.x + bx_size > fw {
            b.x = c.x - 24.0 - bx_size;
        }
        if b.y + bx_size + lh > fh {
            b.y = c.y - 24.0 - bx_size - lh;
        }
        let bx = egui::Rect::from_min_size(b, vec2(bx_size, bx_size));
        p.rect_filled(bx, 0.0, Color32::BLACK);
        let src = Rect::new(self.cur.0 - N / 2, self.cur.1 - N / 2, self.cur.0 - N / 2 + N, self.cur.1 - N / 2 + N);
        // tiles clip what is outside the screen; the rest maps pixel to 8×8 block
        let all = Rect::new(0, 0, w, h);
        let vis = src.intersect(&all);
        if !vis.is_empty() {
            let k = bx_size / N as f32;
            let d = egui::Rect::from_min_max(
                b + vec2((vis.left - src.left) as f32 * k, (vis.top - src.top) as f32 * k),
                b + vec2((vis.right - src.left) as f32 * k, (vis.bottom - src.top) as f32 * k),
            );
            self.draw_frozen(p, vis, d, Color32::WHITE);
        }
        let one = 1.0 / ppp;
        p.rect_stroke(bx.expand(one), 0.0, Stroke::new(one, Color32::WHITE), StrokeKind::Inside);
        let k = bx_size / N as f32;
        let mid = egui::Rect::from_min_size(b + vec2((N / 2) as f32 * k, (N / 2) as f32 * k), vec2(k, k));
        p.rect_stroke(mid, 0.0, Stroke::new(one, Color32::WHITE), StrokeKind::Outside);
        let f = &self.s.frozen;
        let px = if f.is_empty() { 0 } else { f.at(self.cur.0.clamp(0, f.wi() - 1), self.cur.1.clamp(0, f.hi() - 1)) };
        let tr_ = egui::Rect::from_min_size(pos2(bx.left() - one, bx.bottom() + one), vec2(bx.width() + 2.0 * one, lh - one));
        p.rect_filled(tr_, 0.0, Color32::from_gray(32));
        let (t1, t2) = tr_.split_top_bottom_at_fraction(0.5);
        p.text(t1.center(), Align2::CENTER_CENTER, format!("{}, {}", self.cur.0 + self.s.vs.left, self.cur.1 + self.s.vs.top),
               FontId::proportional(10.5), Color32::WHITE);
        let sw = egui::Rect::from_min_max(pos2(t2.left() + 8.0, t2.top() + 4.0), pos2(t2.left() + 22.0, t2.bottom() - 4.0));
        p.rect(sw, 4.0, Color32::from_rgb(cr(px) as u8, cg(px) as u8, cb(px) as u8), Stroke::new(1.0, Color32::WHITE),
               StrokeKind::Inside);
        p.text(t2.center(), Align2::CENTER_CENTER, color::color_to_hex(colorref(px)), FontId::proportional(10.5), Color32::WHITE);
    }
}
