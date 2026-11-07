//! Pinned captures (Pins.cpp, C01; ShareX dossier 01 · F09 PinToScreenForm): images floating above every window, like
//! sticky notes. The wheel zooms (20–500 %), Ctrl+wheel changes the opacity (10–100 %), the arrows move (Shift = 10 px),
//! a middle click resets; lock, click-through, always on top and shadow; quick annotation with the Editor's own objects
//! (pen, highlighter, arrow, rectangle, text); Alt+click copies the color under the cursor. Closed pins stay in a
//! short history (the Pins folder, the last 10) and can be reopened.
//!
//! Each pin is a layered Win32 window drawn with GDI+, as in the C++; its state lives behind GWLP_USERDATA (the window
//! procedure is re-entered by the context menu's own message loop, so no borrow is held across it).

use std::cell::{Cell, RefCell};
use std::path::{Path as FsPath, PathBuf};
use std::sync::Once;

use snapcore::doc::{CAP_ARROW, DrawObj, ObjType, SHAPE_RECT};
use snapcore::gdip::{self, Brush, Font, Graphics, Pen, StringFormat};
use snapcore::image::{Image, Rect, argb, ca, cb, cg, cr, from_colorref};
use snapcore::serial::DocFile;
use snapcore::wide::to_wide;
use snapcore::{clip, codec, color, library, log, paths, render, screen};
use snapnab_app::tr;
use snapui::host::Waker;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BLENDFUNCTION, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, ReleaseDC, ScreenToClient,
    SelectObject,
};
use windows::Win32::Graphics::GdiPlus::{
    FontStyleBold, FontStyleRegular, SmoothingModeAntiAlias, StringAlignmentCenter,
    TextRenderingHintAntiAliasGridFit,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, SetFocus, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VK_ADD, VK_CONTROL, VK_DOWN,
    VK_ESCAPE, VK_LEFT, VK_MENU, VK_OEM_MINUS, VK_OEM_PLUS, VK_RIGHT, VK_SHIFT, VK_SUBTRACT, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GWL_EXSTYLE, GWLP_USERDATA, GetCursorPos, GetWindowLongPtrW, GetWindowLongW,
    GetWindowRect, HWND_NOTOPMOST, HWND_TOPMOST, IDC_ARROW, IDC_CROSS, IDC_HAND, IDC_IBEAM, IDC_SIZEALL, LoadCursorW,
    MA_ACTIVATE, PostMessageW, RegisterClassW, SW_HIDE, SW_SHOW, SW_SHOWNOACTIVATE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SWP_NOZORDER, SetCursor, SetForegroundWindow, SetWindowLongPtrW, SetWindowLongW, SetWindowPos, ShowWindow,
    ULW_ALPHA, UpdateLayeredWindow, WM_APP, WM_DESTROY, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONUP,
    WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCDESTROY, WM_RBUTTONUP, WM_SETCURSOR, WNDCLASSW, WS_EX_LAYERED,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::Win32::Foundation::SIZE;
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXDRAG};
use windows::core::{PCWSTR, w};

use crate::tray::{Item, Menu};

/// What the floating windows (pins, the quick overlay, One Click) ask of the app, through its event loop.
pub enum UiEv {
    Notify(String, String),
    Qr(Image),
    /// upload a capture (the image, and its library file)
    Upload(Image, String),
    /// a library file to open in the Editor
    Editor(String),
    LibraryChanged,
    /// One Click: capture with the current preset
    Capture,
    /// One Click: the gear's menu at a screen point
    OneClickMenu(i32, i32),
    /// the screen color picker's color (COLORREF)
    ColorPicked(u32),
    /// the interval capture's settings, chosen in its form
    IntervalChosen(crate::interval::Settings),
    /// the interval capture's region (and window) chosen with the crosshair
    IntervalArea(snapcore::image::Rect, isize),
    /// the interval capture runs (and how many images so far)
    IntervalStatus(bool, i32),
    /// a change of the presets from a form (run on the app's thread, then saved and the hotkeys registered again)
    Store(crate::dialogs::StoreEdit),
    /// a preset's hotkey chosen in its form: index, modifiers, virtual key
    PresetHotkey(usize, u32, u32),
    /// Options saved: the hotkeys and settings are read again, and the Editor told (RELOAD_CONFIG)
    Reload,
    /// the language changed: Snapnab and the Editor restart
    RestartApps,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tool {
    None,
    Pen,
    Highlighter,
    Arrow,
    Rect,
    Text,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Btn {
    Close,
    Annotate,
    Tool(Tool),
    Color,
    Undo,
    Done,
}

#[derive(Clone, Copy)]
pub(crate) enum Glyph {
    Cancel,
    Copy,
    Save,
    Pin,
    Trash,
    Pen,
    Highlighter,
    Arrow,
    Rect,
    Text,
    Undo,
    Check,
}

const WM_PIN_TEXT: u32 = WM_APP + 2;
const RED: u32 = 0xFFE5322D;

struct PinWnd {
    h: isize,
    base: Image,
    objs: Vec<DrawObj>,
    title: String,
    scale: i32,
    opacity: i32,
    locked: bool,
    click_through: bool,
    topmost: bool,
    shadow: bool,
    tool: Tool,
    color: u32,
    hover: bool,
    /// 0 none, 1 move, 2 drawing
    drag: i32,
    down: POINT,
    win_at_down: POINT,
    creating: Option<DrawObj>,
    composed: Image,
    scaled: Image,
    scaled_for: i32,
    buttons: Vec<(RECT, Btn)>,
    img_origin: (i32, i32),
}

thread_local! {
    static PINS: RefCell<Vec<isize>> = const { RefCell::new(Vec::new()) };
    static HIDDEN: Cell<bool> = const { Cell::new(false) };
}

/// The app's event loop, shared by every thread (the forms and the dialogs run on their own threads).
static SINK: std::sync::Mutex<Option<Waker>> = std::sync::Mutex::new(None);

pub fn init(w: Waker) {
    *SINK.lock().unwrap_or_else(|e| e.into_inner()) = Some(w);
}

pub(crate) fn send(ev: UiEv) {
    if let Some(w) = SINK.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        w.send(ev);
    }
}

fn h(x: isize) -> HWND {
    HWND(x as _)
}

fn sc(v: i32, dpi: u32) -> i32 {
    (v as i64 * dpi as i64 / 96) as i32
}

fn pins_dir() -> PathBuf {
    let d = paths::app_data_dir().join("Pins");
    let _ = std::fs::create_dir_all(&d);
    d
}

/// SAFETY: the pointer in GWLP_USERDATA is the PinWnd of this window until WM_NCDESTROY frees it.
unsafe fn state(hwnd: HWND) -> *mut PinWnd {
    unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut PinWnd }
}

impl PinWnd {
    fn dpi(&self) -> u32 {
        unsafe { GetDpiForWindow(h(self.h)) }.max(96)
    }
    fn margin(&self) -> i32 {
        if self.shadow { sc(14, self.dpi()) } else { 1 }
    }
    fn bar_h(&self) -> i32 {
        if self.tool != Tool::None { sc(34, self.dpi()) } else { 0 }
    }
    fn img_size(&self) -> (i32, i32) {
        ((self.base.wi() * self.scale / 100).max(1), (self.base.hi() * self.scale / 100).max(1))
    }
    fn window_rect(&self) -> RECT {
        let mut r = RECT::default();
        unsafe {
            let _ = GetWindowRect(h(self.h), &mut r);
        }
        r
    }

    fn compose(&mut self) {
        let mut objs = self.objs.clone();
        if let Some(c) = &self.creating {
            objs.push(c.clone());
        }
        self.composed = if objs.is_empty() { self.base.clone() } else { render::render(&self.base, &objs, None) };
        self.scaled_for = -1;
    }

    fn refresh(&mut self, recompose: bool) {
        if recompose {
            self.compose();
        }
        self.render();
    }

    /// The window as a layered bitmap: shadow, image, border and the buttons.
    fn render(&mut self) {
        if self.scaled_for != self.scale {
            let (w, hh) = self.img_size();
            self.scaled = if self.scale == 100 { self.composed.clone() } else { gdip::scale_image(&self.composed, w, hh, true) };
            self.scaled_for = self.scale;
        }
        let (dpi, m, bar) = (self.dpi(), self.margin(), self.bar_h());
        let (iw, ih) = (self.scaled.wi(), self.scaled.hi());
        let ww = (iw + 2 * m).max(if bar > 0 { sc(300, dpi) + 2 * m } else { 0 });
        let hh = ih + 2 * m + bar;
        let mut s = Image::sized(ww, hh, 0);
        self.img_origin = (m, m + bar);
        if self.shadow {
            // a soft shadow: the alpha falls off with the distance to the image
            let ir = Rect::new(m, m + bar, m + iw, m + bar + ih);
            for y in 0..hh {
                for x in 0..ww {
                    let dx = (ir.left - x).max(0).max(x - ir.right + 1);
                    let dy = (ir.top - y).max(0).max(y - ir.bottom + 1 - sc(3, dpi));
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let d = ((dx * dx + dy * dy) as f64).sqrt() / m as f64;
                    if d < 1.0 {
                        *s.at_mut(x, y) = argb((70.0 * (1.0 - d) * (1.0 - d)) as i32, 0, 0, 0);
                    }
                }
            }
        }
        s.blit(&self.scaled, m, m + bar, false);
        // a thin border, so the pin stands out from the window below
        for x in (m - 1)..=(m + iw) {
            if x < 0 || x >= ww {
                continue;
            }
            if m + bar - 1 >= 0 {
                *s.at_mut(x, m + bar - 1) = 0xC880_8080;
            }
            if m + bar + ih < hh {
                *s.at_mut(x, m + bar + ih) = 0xC880_8080;
            }
        }
        for y in (m + bar - 1)..=(m + bar + ih) {
            if y < 0 || y >= hh {
                continue;
            }
            if m - 1 >= 0 {
                *s.at_mut(m - 1, y) = 0xC880_8080;
            }
            if m + iw < ww {
                *s.at_mut(m + iw, y) = 0xC880_8080;
            }
        }
        let btns: RefCell<Vec<(RECT, Btn)>> = RefCell::new(Vec::new());
        {
            let bmp = gdip::Bitmap::wrap(&mut s);
            let g = Graphics::new(&bmp);
            g.smoothing(SmoothingModeAntiAlias);
            g.text_hint(TextRenderingHintAntiAliasGridFit);
            let (bs, gap) = (sc(26, dpi), sc(4, dpi));
            let button = |r: RECT, id: Btn, glyph: Glyph, on: bool, bg: u32| {
                let (x, y, w) = (r.left as f32, r.top as f32, (r.right - r.left) as f32);
                g.fill_ellipse(&Brush::solid(bg), x, y, w, w);
                if on {
                    g.draw_ellipse(&Pen::new(0xFFFFFFFF, 2.0), x + 1.0, y + 1.0, w - 2.0, w - 2.0);
                }
                let pad = w / 4.0;
                draw_glyph(&g, glyph, x + pad, y + pad, w - 2.0 * pad, 0xFFFFFFFF);
                btns.borrow_mut().push((r, id));
            };
            if self.tool != Tool::None {
                // the annotation toolbar above the image
                let (mut x, y) = (m, m + (bar - bs) / 2);
                for (t, gl) in [(Tool::Pen, Glyph::Pen), (Tool::Highlighter, Glyph::Highlighter), (Tool::Arrow, Glyph::Arrow),
                                (Tool::Rect, Glyph::Rect), (Tool::Text, Glyph::Text)] {
                    button(RECT { left: x, top: y, right: x + bs, bottom: y + bs }, Btn::Tool(t), gl, self.tool == t, 0xE030_3036);
                    x += bs + gap;
                }
                let cr_ = RECT { left: x, top: y, right: x + bs, bottom: y + bs };
                g.fill_ellipse(&Brush::solid(self.color), x as f32 + 3.0, y as f32 + 3.0, bs as f32 - 6.0, bs as f32 - 6.0);
                btns.borrow_mut().push((cr_, Btn::Color));
                x += bs + gap;
                button(RECT { left: x, top: y, right: x + bs, bottom: y + bs }, Btn::Undo, Glyph::Undo, false, 0xE030_3036);
                x += bs + gap;
                button(RECT { left: x, top: y, right: x + bs, bottom: y + bs }, Btn::Done, Glyph::Check, false, 0xE020_7A3C);
            } else if self.hover && !self.click_through && iw > 3 * bs {
                // close and annotate at the top right of the image
                let (x, y) = (m + iw - bs - gap, m + bar + gap);
                button(RECT { left: x, top: y, right: x + bs, bottom: y + bs }, Btn::Close, Glyph::Cancel, false, 0xD020_2024);
                button(RECT { left: x - bs - gap, top: y, right: x - gap, bottom: y + bs }, Btn::Annotate, Glyph::Pen, false,
                       0xD020_2024);
            }
            g.flush();
        }
        self.buttons = btns.into_inner();
        // premultiplied, with the pin's opacity as the constant alpha
        for px in s.px.iter_mut() {
            let a = ca(*px);
            *px = argb(a, cr(*px) * a / 255, cg(*px) * a / 255, cb(*px) * a / 255);
        }
        let wr = self.window_rect();
        unsafe {
            let sdc = GetDC(None);
            let mem = CreateCompatibleDC(Some(sdc));
            if let Some(hb) = screen::to_hbitmap(&s) {
                let o = SelectObject(mem, hb.into());
                let bf = BLENDFUNCTION {
                    BlendOp: AC_SRC_OVER as u8,
                    BlendFlags: 0,
                    SourceConstantAlpha: (self.opacity * 255 / 100) as u8,
                    AlphaFormat: AC_SRC_ALPHA as u8,
                };
                let _ = UpdateLayeredWindow(h(self.h), Some(sdc), Some(&POINT { x: wr.left, y: wr.top }), Some(&SIZE { cx: ww, cy: hh }),
                                            Some(mem), Some(&POINT::default()), Default::default(), Some(&bf), ULW_ALPHA);
                SelectObject(mem, o);
                let _ = DeleteObject(hb.into());
            }
            let _ = DeleteDC(mem);
            ReleaseDC(None, sdc);
        }
    }

    fn to_image(&self, c: POINT) -> (i32, i32) {
        ((c.x - self.img_origin.0) * 100 / self.scale, (c.y - self.img_origin.1) * 100 / self.scale)
    }

    fn hit(&self, c: POINT) -> Option<Btn> {
        self.buttons.iter().find(|(r, _)| c.x >= r.left && c.x < r.right && c.y >= r.top && c.y < r.bottom).map(|b| b.1)
    }

    /// Moves the window so that the image's top-left corner stays put when the margins change.
    fn keep_image(&self, old_margin: i32, old_bar: i32) {
        let wr = self.window_rect();
        let (dx, dy) = (self.margin() - old_margin, self.margin() + self.bar_h() - (old_margin + old_bar));
        unsafe {
            let _ = SetWindowPos(h(self.h), None, wr.left - dx, wr.top - dy, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }

    fn set_tool(&mut self, t: Tool) {
        let (m, b) = (self.margin(), self.bar_h());
        self.tool = t;
        self.keep_image(m, b);
        self.render();
    }

    /// The point under the cursor stays where it is.
    fn set_scale(&mut self, s: i32, anchor: POINT) {
        let s = s.clamp(20, 500);
        if s == self.scale {
            return;
        }
        let wr = self.window_rect();
        let ix = (anchor.x - wr.left - self.img_origin.0) as f64 * 100.0 / self.scale as f64;
        let iy = (anchor.y - wr.top - self.img_origin.1) as f64 * 100.0 / self.scale as f64;
        self.scale = s;
        let nx = (anchor.x as f64 - self.img_origin.0 as f64 - ix * s as f64 / 100.0).round() as i32;
        let ny = (anchor.y as f64 - self.img_origin.1 as f64 - iy * s as f64 / 100.0).round() as i32;
        unsafe {
            let _ = SetWindowPos(h(self.h), None, nx, ny, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
        self.render();
    }

    fn apply_styles(&self) {
        unsafe {
            let ex = GetWindowLongW(h(self.h), GWL_EXSTYLE) as u32;
            let ex = if self.click_through { ex | WS_EX_TRANSPARENT.0 } else { ex & !WS_EX_TRANSPARENT.0 };
            SetWindowLongW(h(self.h), GWL_EXSTYLE, ex as i32);
            let _ = SetWindowPos(h(self.h), Some(if self.topmost { HWND_TOPMOST } else { HWND_NOTOPMOST }), 0, 0, 0, 0,
                                 SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED);
        }
    }

    fn undo(&mut self) {
        if self.objs.pop().is_some() {
            self.refresh(true);
        }
    }

    fn pick_color_at(&self, c: POINT) {
        let (x, y) = self.to_image(c);
        if x < 0 || y < 0 || x >= self.composed.wi() || y >= self.composed.hi() {
            return;
        }
        let p = self.composed.at(x, y);
        let cref = (p >> 16 & 0xFF) | (p & 0xFF00) | ((p & 0xFF) << 16);
        let t = color::to_text(cref, color::default_format());
        clip::copy_text(self.h, &t);
        color::add_recent(cref);
        send(UiEv::Notify("Snapnab".into(), tr("Cor copiada: ") + &t));
    }

    fn copy(&self) {
        clip::copy_image(self.h, &self.composed, crate::VERSION);
    }

    fn save_as(&self) {
        let name = if self.title.is_empty() { tr("Fixada") } else { self.title.replace([':', '\\', '/', '*', '?', '"', '<', '>', '|'], "_") };
        let Some(mut path) = save_file_dialog(self.h, &tr("Salvar a imagem fixada"), &name, &image_filter(), "png") else { return };
        if FsPath::new(&path).extension().is_none() {
            path += ".png";
        }
        let ext = FsPath::new(&path).extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
        if !codec::save_file(FsPath::new(&path), &self.composed, codec::Format::from_ext(&ext), &codec::SaveOptions::from_settings(),
                             crate::VERSION)
        {
            crate::engine::message_box(tr("Não foi possível salvar:\n") + &path, "Snapnab".into());
        }
    }

    fn open_in_editor(&self) {
        let path = String::from_utf16_lossy(&library::library_dir()) + "\\" + &String::from_utf16_lossy(&library::new_guid()) + ".snap";
        let d = DocFile {
            base: self.base.clone(),
            objs: self.objs.clone(),
            capture_mode: tr("Imagem fixada"),
            window_title: self.title.clone(),
            created: String::from_utf16_lossy(&library::time_stamp(true)),
            next_step: 1,
            ..Default::default()
        };
        if !d.save(FsPath::new(&path), crate::VERSION) {
            return;
        }
        library::write_thumbnail(&library::library_dir(), &path.encode_utf16().collect::<Vec<_>>(),
                                 &gdip::make_thumbnail(&self.composed, 160, 120), crate::VERSION);
        send(UiEv::Editor(path));
    }

    fn save_history(&self) {
        let dir = pins_dir();
        let stamp = String::from_utf16_lossy(&library::time_stamp(false));
        let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_millis()).unwrap_or(0);
        let f = dir.join(format!("pin_{stamp}_{ms:03}.png"));
        codec::save_file(&f, &self.composed, codec::Format::Png, &codec::SaveOptions::from_settings(), crate::VERSION);
        let mut files = history_files();
        files.reverse(); // oldest first
        while files.len() > 10 {
            let _ = std::fs::remove_file(files.remove(0));
        }
    }

    fn new_obj(&self, ip: (i32, i32)) -> Option<DrawObj> {
        let pt = gdip::pt(ip.0 as f32, ip.1 as f32);
        let kind = match self.tool {
            Tool::Pen => ObjType::Pen,
            Tool::Highlighter => ObjType::Highlighter,
            Tool::Arrow => ObjType::Arrow,
            Tool::Rect => ObjType::Shape,
            _ => return None,
        };
        let mut o = DrawObj::new(kind);
        (o.p1, o.p2, o.ctrl) = (pt, pt, pt);
        o.style.stroke = self.color;
        o.style.shadow = false;
        match self.tool {
            Tool::Pen => {
                o.style.width = 4.0;
                o.pts = vec![pt];
            }
            Tool::Highlighter => {
                o.style.stroke = if self.color == RED { 0xFFFFEB3B } else { self.color };
                o.style.opacity = 50;
            }
            Tool::Arrow => {
                o.style.width = 5.0;
                o.style.arrow_end = CAP_ARROW;
                o.style.shadow = true;
            }
            _ => {
                o.style.shape_kind = SHAPE_RECT;
                o.style.width = 3.0;
            }
        }
        Some(o)
    }

    fn add_text(&mut self, ip: (i32, i32), text: &str) {
        if text.trim().is_empty() {
            return;
        }
        let mut o = DrawObj::new(ObjType::Text);
        o.text = text.encode_utf16().collect();
        o.style.text_color = self.color;
        o.style.stroke = if self.color & 0xFF_FFFF == 0xFF_FFFF { 0xFF00_0000 } else { 0xFFFF_FFFF }; // outline in the opposite tone
        o.style.width = 3.0;
        o.style.font_size = 24.0;
        o.style.bold = true;
        (o.style.align_h, o.style.align_v) = (0, 0);
        o.style.shadow = false;
        o.style.padding = 2.0;
        let w = 80f32.max((self.base.wi() - ip.0) as f32);
        o.p1 = gdip::pt(ip.0 as f32, ip.1 as f32);
        o.p2 = gdip::pt(ip.0 as f32 + w, ip.1 as f32 + render::text_block_height(&o, w) + 4.0);
        self.objs.push(o);
        self.refresh(true);
    }
}

/// Simple white glyphs for the pin's buttons, in the square (x, y, s).
pub(crate) fn draw_glyph(g: &Graphics, glyph: Glyph, x: f32, y: f32, s: f32, c: u32) {
    let p = Pen::new(c, (s / 9.0).max(1.6));
    let at = |fx: f32, fy: f32| gdip::pt(x + fx * s, y + fy * s);
    match glyph {
        Glyph::Cancel => {
            g.line(&p, at(0.15, 0.15), at(0.85, 0.85));
            g.line(&p, at(0.85, 0.15), at(0.15, 0.85));
        }
        Glyph::Pen => {
            g.line(&p, at(0.2, 0.8), at(0.8, 0.2));
            g.line(&p, at(0.12, 0.88), at(0.2, 0.8));
        }
        Glyph::Highlighter => g.line(&Pen::new(0xC0FF_FFFF, s / 3.5), at(0.2, 0.75), at(0.8, 0.25)),
        Glyph::Arrow => {
            g.line(&p, at(0.15, 0.85), at(0.85, 0.15));
            g.fill_polygon(&Brush::solid(c), &[at(0.9, 0.1), at(0.45, 0.2), at(0.8, 0.55)]);
        }
        Glyph::Rect => g.draw_rect(&p, gdip::rect(x + 0.12 * s, y + 0.22 * s, 0.76 * s, 0.56 * s)),
        Glyph::Text => {
            let f = StringFormat::new();
            f.align(StringAlignmentCenter);
            f.line_align(StringAlignmentCenter);
            let t: Vec<u16> = "T".encode_utf16().collect();
            g.draw_string(&t, &Font::new("Segoe UI", s * 0.95, FontStyleBold), gdip::rect(x, y, s, s), &f, &Brush::solid(c));
        }
        Glyph::Undo => {
            g.draw_arc(&p, gdip::rect(x + 0.2 * s, y + 0.25 * s, 0.65 * s, 0.6 * s), 180.0, 220.0);
            g.fill_polygon(&Brush::solid(c), &[at(0.05, 0.5), at(0.35, 0.5), at(0.2, 0.25)]);
        }
        Glyph::Check => g.draw_lines(&p, &[at(0.15, 0.5), at(0.4, 0.75), at(0.85, 0.25)]),
        Glyph::Copy => {
            g.draw_rect(&p, gdip::rect(x + 0.1 * s, y + 0.1 * s, 0.55 * s, 0.6 * s));
            g.draw_rect(&p, gdip::rect(x + 0.35 * s, y + 0.3 * s, 0.55 * s, 0.6 * s));
        }
        Glyph::Save => {
            g.draw_rect(&p, gdip::rect(x + 0.1 * s, y + 0.1 * s, 0.8 * s, 0.8 * s));
            g.draw_rect(&p, gdip::rect(x + 0.3 * s, y + 0.1 * s, 0.4 * s, 0.25 * s));
            g.draw_rect(&p, gdip::rect(x + 0.25 * s, y + 0.55 * s, 0.5 * s, 0.35 * s));
        }
        Glyph::Pin => {
            g.fill_ellipse(&Brush::solid(c), x + 0.3 * s, y + 0.05 * s, 0.4 * s, 0.4 * s);
            g.line(&p, at(0.5, 0.45), at(0.5, 0.95));
            g.line(&p, at(0.25, 0.45), at(0.75, 0.45));
        }
        Glyph::Trash => {
            g.line(&p, at(0.1, 0.22), at(0.9, 0.22));
            g.line(&p, at(0.38, 0.08), at(0.62, 0.08));
            g.draw_lines(&p, &[at(0.2, 0.22), at(0.27, 0.92), at(0.73, 0.92), at(0.8, 0.22)]);
            g.line(&p, at(0.42, 0.38), at(0.42, 0.78));
            g.line(&p, at(0.58, 0.38), at(0.58, 0.78));
        }
    }
}

fn lp_point(lp: LPARAM) -> POINT {
    POINT { x: (lp.0 & 0xFFFF) as i16 as i32, y: ((lp.0 >> 16) & 0xFFFF) as i16 as i32 }
}

fn key_down(vk: u16) -> bool {
    unsafe { GetKeyState(vk as i32) < 0 }
}

fn cursor() -> POINT {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    p
}

fn close_pin(hwnd: isize) {
    let p = unsafe { state(h(hwnd)) };
    if p.is_null() {
        return;
    }
    unsafe { (*p).save_history() };
    unsafe {
        let _ = DestroyWindow(h(hwnd));
    }
}

fn color_menu(hwnd: isize, at: POINT) {
    let cols = [RED, 0xFFFFC107, 0xFF2EAD4B, 0xFF1E88E5, 0xFF8E24AA, 0xFF000000, 0xFFFFFFFF];
    let names = ["Vermelho", "Amarelo", "Verde", "Azul", "Roxo", "Preto", "Branco"].map(tr);
    let cur = unsafe { (*state(h(hwnd))).color };
    let m = Menu::new(names.iter().enumerate().map(|(i, n)| Item::Checked(1 + i as u32, n, cur == cols[i])).collect());
    let id = m.choose(hwnd, at.x, at.y);
    let p = unsafe { state(h(hwnd)) };
    if (1..=7).contains(&id) && !p.is_null() {
        unsafe {
            (*p).color = cols[id as usize - 1];
            (*p).render();
        }
    }
}

fn context_menu(hwnd: isize, at: POINT) {
    const COPY: u32 = 1;
    const SAVE: u32 = 2;
    const EDITOR: u32 = 3;
    const ANNOTATE: u32 = 4;
    const UNDO: u32 = 5;
    const ZOOM: u32 = 10; // + 50, 100, 200 → 10, 11, 12
    const OP: u32 = 20; // + 100, 75, 50, 25
    const LOCK: u32 = 30;
    const THROUGH: u32 = 31;
    const TOP: u32 = 32;
    const SHADOW: u32 = 33;
    const QR: u32 = 40;
    const COLOR: u32 = 41;
    const CLOSE: u32 = 50;
    const CLOSE_ALL: u32 = 51;
    let p = unsafe { state(h(hwnd)) };
    if p.is_null() {
        return;
    }
    // what the menu shows, read before the menu runs its own message loop
    let (tool, has_objs, scale, opacity, locked, through, top, shadow) = unsafe {
        let p = &*p;
        (p.tool, !p.objs.is_empty(), p.scale, p.opacity, p.locked, p.click_through, p.topmost, p.shadow)
    };
    let t = [tr("Copiar\tCtrl+C"), tr("Salvar como…\tCtrl+S"), tr("Abrir no Editor"),
             tr("Anotar (caneta, marcador, seta, retângulo, texto)"), tr("Desfazer a anotação\tCtrl+Z"),
             format!("{}  ({scale}%)", tr("Zoom")), format!("{}  ({opacity}%)", tr("Opacidade")), tr("Travar a posição"),
             tr("Atravessar cliques (desligue pelo menu da bandeja)"), tr("Sempre visível"), tr("Sombra"), tr("Ler código QR"),
             tr("Copiar a cor neste ponto\tAlt+clique"), tr("Arrastar para outro programa: Ctrl+arrastar"), tr("Fechar\tEsc"),
             tr("Fechar todas as imagens fixadas")];
    let zooms = [50, 100, 200].map(|z| format!("{z}%"));
    let ops = [100, 75, 50, 25];
    let op_t = ops.map(|o| format!("{o}%"));
    let zoom = Menu::new(zooms.iter().enumerate().map(|(i, z)| Item::Checked(ZOOM + i as u32, z, scale == [50, 100, 200][i])).collect());
    let op = Menu::new(op_t.iter().enumerate().map(|(i, o)| Item::Checked(OP + i as u32, o, opacity == ops[i])).collect());
    let m = Menu::new(vec![
        Item::Cmd(COPY, &t[0]),
        Item::Cmd(SAVE, &t[1]),
        Item::Cmd(EDITOR, &t[2]),
        Item::Separator,
        Item::Checked(ANNOTATE, &t[3], tool != Tool::None),
        if has_objs { Item::Cmd(UNDO, &t[4]) } else { Item::Disabled(&t[4]) },
        Item::Sub(&t[5], zoom),
        Item::Sub(&t[6], op),
        Item::Separator,
        Item::Checked(LOCK, &t[7], locked),
        Item::Checked(THROUGH, &t[8], through),
        Item::Checked(TOP, &t[9], top),
        Item::Checked(SHADOW, &t[10], shadow),
        Item::Separator,
        Item::Cmd(QR, &t[11]),
        Item::Cmd(COLOR, &t[12]),
        Item::Disabled(&t[13]),
        Item::Separator,
        Item::Cmd(CLOSE, &t[14]),
        Item::Cmd(CLOSE_ALL, &t[15]),
    ]);
    let id = m.choose(hwnd, at.x, at.y);
    let p = unsafe { state(h(hwnd)) };
    if p.is_null() || id == 0 {
        return;
    }
    let mut cl = at;
    unsafe {
        let _ = ScreenToClient(h(hwnd), &mut cl);
    }
    let pin = unsafe { &mut *p };
    match id {
        COPY => pin.copy(),
        SAVE => pin.save_as(),
        EDITOR => pin.open_in_editor(),
        ANNOTATE => pin.set_tool(if pin.tool != Tool::None { Tool::None } else { Tool::Pen }),
        UNDO => pin.undo(),
        z if (ZOOM..ZOOM + 3).contains(&z) => pin.set_scale([50, 100, 200][(z - ZOOM) as usize], at),
        o if (OP..OP + 4).contains(&o) => {
            pin.opacity = ops[(o - OP) as usize];
            pin.render();
        }
        LOCK => pin.locked = !pin.locked,
        THROUGH => {
            pin.click_through = !pin.click_through;
            pin.hover = false;
            pin.apply_styles();
            pin.render();
        }
        TOP => {
            pin.topmost = !pin.topmost;
            pin.apply_styles();
        }
        SHADOW => {
            let (m0, b0) = (pin.margin(), pin.bar_h());
            pin.shadow = !pin.shadow;
            pin.keep_image(m0, b0);
            pin.render();
        }
        QR => send(UiEv::Qr(pin.composed.clone())),
        COLOR => pin.pick_color_at(cl),
        CLOSE => close_pin(hwnd),
        CLOSE_ALL => close_all(),
        _ => {}
    }
}

/// The text of the text tool, asked in a form on its own thread and posted back.
fn ask_text(hwnd: isize, ip: (i32, i32)) {
    std::thread::spawn(move || {
        let mut f = [crate::form::Field::Text { label: tr("Texto"), text: String::new() }];
        if crate::form::run(hwnd, &tr("Texto na imagem fixada"), &mut f) {
            let crate::form::Field::Text { text, .. } = &f[0] else { return };
            let b = Box::new((ip, text.clone()));
            unsafe {
                if PostMessageW(Some(h(hwnd)), WM_PIN_TEXT, WPARAM(0), LPARAM(Box::into_raw(b) as isize)).is_err() {
                    // the pin is gone: the box is not freed (a few bytes)
                }
            }
        }
    });
}

unsafe extern "system" fn pin_proc(hwnd: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let ptr = unsafe { state(hwnd) };
    if ptr.is_null() {
        return unsafe { DefWindowProcW(hwnd, m, wp, lp) };
    }
    let hw = hwnd.0 as isize;
    match m {
        WM_MOUSEACTIVATE => LRESULT(MA_ACTIVATE as isize),
        WM_SETCURSOR => {
            let p = unsafe { &*ptr };
            let mut c = cursor();
            unsafe {
                let _ = ScreenToClient(hwnd, &mut c);
            }
            let cur = if p.hit(c).is_some() {
                IDC_HAND
            } else if key_down(VK_MENU.0) {
                IDC_CROSS
            } else if p.tool == Tool::Text {
                IDC_IBEAM
            } else if p.tool != Tool::None {
                IDC_CROSS
            } else if p.locked {
                IDC_ARROW
            } else {
                IDC_SIZEALL
            };
            unsafe {
                SetCursor(LoadCursorW(None, cur).ok());
            }
            LRESULT(1)
        }
        WM_MOUSEMOVE => {
            let p = unsafe { &mut *ptr };
            let c = lp_point(lp);
            if !p.hover {
                p.hover = true;
                let mut t = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd,
                                              dwHoverTime: 0 };
                unsafe {
                    let _ = TrackMouseEvent(&mut t);
                }
                p.render();
            }
            if p.drag == 1 {
                let s = cursor();
                unsafe {
                    let _ = SetWindowPos(hwnd, None, p.win_at_down.x + s.x - p.down.x, p.win_at_down.y + s.y - p.down.y, 0, 0,
                                         SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
                }
            } else if p.drag == 2 && p.creating.is_some() {
                let ip = p.to_image(c);
                let pt = gdip::pt(ip.0 as f32, ip.1 as f32);
                if let Some(o) = p.creating.as_mut() {
                    if o.kind == ObjType::Pen {
                        o.pts.push(pt);
                    } else {
                        o.p2 = pt;
                        o.ctrl = gdip::pt((o.p1.X + pt.X) / 2.0, (o.p1.Y + pt.Y) / 2.0);
                    }
                }
                p.refresh(true);
            } else if p.drag == 3 {
                let s = cursor();
                if (s.x - p.down.x).abs() + (s.y - p.down.y).abs() > unsafe { GetSystemMetrics(SM_CXDRAG) } {
                    p.drag = 0;
                    unsafe {
                        let _ = ReleaseCapture();
                    }
                    let name = if p.title.is_empty() { "Snapnab".to_string() } else { p.title.clone() };
                    let img = p.composed.clone();
                    if let Some(f) = snapcore::dragout::temp_image_file(&img, &name, false, crate::VERSION) {
                        snapcore::dragout::drag_files_out(hw, &[f], Some(&img), true);
                    }
                }
            }
            LRESULT(0)
        }
        0x02A3 /* WM_MOUSELEAVE */ => {
            let p = unsafe { &mut *ptr };
            if p.hover {
                p.hover = false;
                p.render();
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let c = lp_point(lp);
            unsafe {
                let _ = SetFocus(Some(hwnd));
            }
            let b = unsafe { (*ptr).hit(c) };
            if let Some(b) = b {
                match b {
                    Btn::Close => close_pin(hw),
                    Btn::Annotate => unsafe { (*ptr).set_tool(Tool::Pen) },
                    Btn::Tool(t) => unsafe { (*ptr).set_tool(t) },
                    Btn::Color => {
                        let mut s = c;
                        unsafe {
                            let _ = windows::Win32::Graphics::Gdi::ClientToScreen(hwnd, &mut s);
                        }
                        color_menu(hw, s);
                    }
                    Btn::Undo => unsafe { (*ptr).undo() },
                    Btn::Done => unsafe { (*ptr).set_tool(Tool::None) },
                }
                return LRESULT(0);
            }
            let p = unsafe { &mut *ptr };
            if key_down(VK_MENU.0) {
                p.pick_color_at(c); // Alt+click: the color (C10)
                return LRESULT(0);
            }
            let ip = p.to_image(c);
            let on_image = ip.0 >= 0 && ip.1 >= 0 && ip.0 < p.base.wi() && ip.1 < p.base.hi();
            if p.tool == Tool::Text && on_image {
                ask_text(hw, ip);
                return LRESULT(0);
            }
            if p.tool != Tool::None && on_image {
                p.creating = p.new_obj(ip);
                if p.creating.is_some() {
                    p.drag = 2;
                    unsafe { SetCapture(hwnd) };
                }
                return LRESULT(0);
            }
            p.down = cursor();
            let wr = p.window_rect();
            p.win_at_down = POINT { x: wr.left, y: wr.top };
            if wp.0 & 0x0008 != 0 {
                // MK_CONTROL — Ctrl+drag: the image goes out as a file
                p.drag = 3;
                unsafe { SetCapture(hwnd) };
                return LRESULT(0);
            }
            if !p.locked {
                p.drag = 1;
                unsafe { SetCapture(hwnd) };
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let p = unsafe { &mut *ptr };
            if p.drag == 2
                && let Some(o) = p.creating.take()
            {
                let r = o.rect();
                let tiny = if o.kind == ObjType::Pen {
                    o.pts.len() < 2
                } else if o.is_line() {
                    (o.p2.X - o.p1.X).hypot(o.p2.Y - o.p1.Y) < 3.0
                } else {
                    r.Width < 3.0 || r.Height < 3.0
                };
                if !tiny {
                    p.objs.push(o);
                }
                p.refresh(true);
            }
            if p.drag != 0 {
                p.drag = 0;
                unsafe {
                    let _ = ReleaseCapture();
                }
            }
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            let mut s = lp_point(lp);
            unsafe {
                let _ = windows::Win32::Graphics::Gdi::ClientToScreen(hwnd, &mut s);
            }
            context_menu(hw, s);
            LRESULT(0)
        }
        WM_MBUTTONUP => {
            let p = unsafe { &mut *ptr };
            let wr = p.window_rect();
            let a = POINT { x: wr.left + p.img_origin.0, y: wr.top + p.img_origin.1 };
            p.opacity = 100;
            p.set_scale(100, a);
            p.render();
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let p = unsafe { &mut *ptr };
            let d = if ((wp.0 >> 16) as u16 as i16) > 0 { 1 } else { -1 };
            let keys = wp.0 & 0xFFFF;
            if keys & 0x8 != 0 {
                // MK_CONTROL: the opacity
                p.opacity = (p.opacity + d * 10).clamp(10, 100);
                p.render();
            } else if keys & 0x4 == 0 {
                // not MK_SHIFT: the zoom
                p.set_scale(p.scale + d * if p.scale >= 200 { 25 } else { 10 }, lp_point(lp));
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let p = unsafe { &mut *ptr };
            let vk = wp.0 as u16;
            let ctrl = key_down(VK_CONTROL.0);
            if vk == VK_ESCAPE.0 {
                if p.tool != Tool::None { p.set_tool(Tool::None) } else { close_pin(hw) }
            } else if [VK_LEFT.0, VK_RIGHT.0, VK_UP.0, VK_DOWN.0].contains(&vk) {
                if !p.locked {
                    let s = if key_down(VK_SHIFT.0) { 10 } else { 1 };
                    let wr = p.window_rect();
                    let dx = if vk == VK_LEFT.0 { -s } else if vk == VK_RIGHT.0 { s } else { 0 };
                    let dy = if vk == VK_UP.0 { -s } else if vk == VK_DOWN.0 { s } else { 0 };
                    unsafe {
                        let _ = SetWindowPos(hwnd, None, wr.left + dx, wr.top + dy, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
                    }
                }
            } else if [VK_ADD.0, VK_OEM_PLUS.0, VK_SUBTRACT.0, VK_OEM_MINUS.0].contains(&vk) {
                let wr = p.window_rect();
                let a = POINT { x: wr.left + p.img_origin.0, y: wr.top + p.img_origin.1 };
                let up = vk == VK_ADD.0 || vk == VK_OEM_PLUS.0;
                p.set_scale(p.scale + if up { 10 } else { -10 }, a);
            } else if ctrl && vk == b'C' as u16 {
                p.copy();
            } else if ctrl && vk == b'S' as u16 {
                p.save_as();
            } else if ctrl && vk == b'Z' as u16 {
                p.undo();
            }
            LRESULT(0)
        }
        WM_PIN_TEXT => {
            // SAFETY: posted by ask_text with a boxed (point, text)
            let b = unsafe { Box::from_raw(lp.0 as *mut ((i32, i32), String)) };
            unsafe { (*ptr).add_text(b.0, &b.1) };
            LRESULT(0)
        }
        WM_DESTROY => {
            PINS.with(|l| l.borrow_mut().retain(|&x| x != hw));
            LRESULT(0)
        }
        WM_NCDESTROY => {
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(ptr));
                DefWindowProcW(hwnd, m, wp, lp)
            }
        }
        _ => unsafe { DefWindowProcW(hwnd, m, wp, lp) },
    }
}

fn create(img: Image, objs: Vec<DrawObj>, title: &str, at: Option<(i32, i32)>) {
    static REG: Once = Once::new();
    REG.call_once(|| unsafe {
        let wc = WNDCLASSW {
            lpfnWndProc: Some(pin_proc),
            hInstance: windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default().into(),
            hCursor: LoadCursorW(None, IDC_SIZEALL).unwrap_or_default(),
            lpszClassName: w!("SnapnabPin"),
            ..Default::default()
        };
        RegisterClassW(&wc);
    });
    if img.is_empty() {
        return;
    }
    let c = cursor();
    let wa = match at {
        Some(a) => screen::monitor_rect(a.0, a.1, true),
        None => screen::monitor_rect(c.x, c.y, true),
    };
    // large images start reduced to fit 80 % of the work area
    let fit = 100f64.min(wa.width() as f64 * 80.0 / img.w as f64).min(wa.height() as f64 * 80.0 / img.h as f64) as i32;
    let t = to_wide(if title.is_empty() { tr("Imagem fixada") } else { title.to_string() });
    let Ok(hw) = (unsafe {
        CreateWindowExW(WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST, w!("SnapnabPin"), PCWSTR(t.as_ptr()), WS_POPUP, 0, 0, 10, 10, None,
                        None, None, None)
    }) else {
        return;
    };
    let (iw, ih) = (img.wi(), img.hi());
    let mut p = Box::new(PinWnd {
        h: hw.0 as isize,
        base: img,
        objs,
        title: title.to_string(),
        scale: fit.max(20),
        opacity: 100,
        locked: false,
        click_through: false,
        topmost: true,
        shadow: snapcore::reg::get_dword(std::ffi::OsStr::new("PinShadow"), 1, None) != 0,
        tool: Tool::None,
        color: RED,
        hover: false,
        drag: 0,
        down: POINT::default(),
        win_at_down: POINT::default(),
        creating: None,
        composed: Image::default(),
        scaled: Image::default(),
        scaled_for: -1,
        buttons: Vec::new(),
        img_origin: (0, 0),
    });
    p.compose();
    let (sw, sh) = p.img_size();
    let m = p.margin();
    let (x, y) = match at {
        Some(a) => (a.0 - m, a.1 - m),
        None => ((wa.left + wa.right - sw) / 2 - m, (wa.top + wa.bottom - sh) / 2 - m),
    };
    let ptr = Box::into_raw(p);
    unsafe {
        SetWindowLongPtrW(hw, GWLP_USERDATA, ptr as isize);
        let _ = SetWindowPos(hw, Some(HWND_TOPMOST), x, y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
        PINS.with(|l| l.borrow_mut().push(hw.0 as isize));
        (*ptr).render();
        let _ = ShowWindow(hw, if HIDDEN.with(|x| x.get()) { SW_HIDE } else { SW_SHOW });
        let _ = SetForegroundWindow(hw);
    }
    log::write(&format!("Pin: {iw}x{ih} '{title}'"));
}

// ---------------------------------------------------------------- the manager (PinManager)
/// Pins an image; `at` = the screen position of its top-left corner (where it was captured), else centered.
pub fn pin(img: Image, title: &str, at: Option<(i32, i32)>) {
    if HIDDEN.with(|x| x.get()) {
        show_all(true);
    }
    create(img, Vec::new(), title, at);
}

/// An image file or a .snap (with its objects).
pub fn pin_file(path: &str) -> bool {
    let p = FsPath::new(path);
    if !p.exists() {
        return false;
    }
    if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("snap")) {
        let Some(d) = DocFile::load(p) else { return false };
        if d.base.is_empty() {
            return false;
        }
        if HIDDEN.with(|x| x.get()) {
            show_all(true);
        }
        let wp: Vec<u16> = path.encode_utf16().collect();
        let name = String::from_utf16_lossy(&library::name(&library::dir_of(&wp), &wp));
        let title = if name.is_empty() { d.window_title.clone() } else { name };
        create(d.base, d.objs, &title, None);
        return true;
    }
    let Ok(img) = codec::load(p) else { return false };
    pin(img, &p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(), None);
    true
}

/// The image of a color: a swatch with its value.
pub fn color_card(c: u32) -> Image {
    let (ww, hh, sw) = (220, 120, 90);
    let mut img = Image::sized(ww, hh, 0xFFFF_FFFF);
    img.fill_rect(Rect::new(0, 0, ww, sw), from_colorref(c));
    {
        let bmp = gdip::Bitmap::wrap(&mut img);
        let g = Graphics::new(&bmp);
        g.text_hint(TextRenderingHintAntiAliasGridFit);
        let f = StringFormat::new();
        f.align(StringAlignmentCenter);
        f.line_align(StringAlignmentCenter);
        let t: Vec<u16> = format!("{}   {}", color::to_text(c, 0), color::to_text(c, 1)).encode_utf16().collect();
        g.draw_string(&t, &Font::new("Segoe UI", 16.0, FontStyleBold), gdip::rect(0.0, sw as f32, ww as f32, (hh - sw) as f32), &f,
                      &Brush::solid(0xFF1E1E22));
        g.flush();
    }
    img
}

/// The image of a text: a card.
pub fn text_card(text: &str) -> Image {
    let text: String = if text.chars().count() > 4000 { text.chars().take(4000).collect::<String>() + "…" } else { text.to_string() };
    let t: Vec<u16> = text.encode_utf16().collect();
    let font = Font::new("Segoe UI", 11.0 * 96.0 / 72.0, FontStyleRegular);
    let fmt = StringFormat::new();
    let (bw, bh) = {
        let mut probe = Image::sized(4, 4, 0);
        let bmp = gdip::Bitmap::wrap(&mut probe);
        let g = Graphics::new(&bmp);
        g.text_hint(TextRenderingHintAntiAliasGridFit);
        g.measure_in(&t, &font, 520.0, 4000.0, &fmt)
    };
    let pad = 14;
    let (ww, hh) = (80.max(bw.ceil() as i32 + 2 * pad), 40.max(bh.ceil() as i32 + 2 * pad));
    let mut img = Image::sized(ww, hh, 0xFFFF_FDF2);
    {
        let bmp = gdip::Bitmap::wrap(&mut img);
        let g = Graphics::new(&bmp);
        g.text_hint(TextRenderingHintAntiAliasGridFit);
        g.draw_string(&t, &font, gdip::rect(pad as f32, pad as f32, (ww - 2 * pad) as f32, (hh - 2 * pad) as f32), &fmt,
                      &Brush::solid(0xFF20_2024));
        g.flush();
    }
    img
}

/// #RRGGBB, RRGGBB, rgb(r, g, b) or "r, g, b" → COLORREF.
pub fn parse_color(s: &str) -> Option<u32> {
    let s = s.trim();
    if s.len() > 40 {
        return None;
    }
    let low = s.to_lowercase();
    let inner = low.strip_prefix("rgb(").and_then(|r| r.strip_suffix(')')).unwrap_or(&low);
    let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
    if parts.len() == 3 {
        let v: Vec<i32> = parts.iter().filter_map(|p| p.parse::<i32>().ok()).collect();
        if v.len() == 3 {
            return v.iter().all(|&x| (0..=255).contains(&x)).then(|| v[0] as u32 | (v[1] as u32) << 8 | (v[2] as u32) << 16);
        }
        return None;
    }
    let hex = s.strip_prefix('#').unwrap_or(s);
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let v = u32::from_str_radix(hex, 16).ok()?;
    Some((v >> 16) | (v & 0xFF00) | ((v & 0xFF) << 16))
}

/// The clipboard's image, or its text as a card, or a color as a swatch.
pub fn pin_clipboard() -> bool {
    if clip::has_image()
        && let Some(img) = clip::get_image()
    {
        pin(img, &tr("Área de transferência"), None);
        return true;
    }
    let text = clip::get_text();
    if text.trim().is_empty() {
        send(UiEv::Notify("Snapnab".into(), tr("A área de transferência não tem imagem nem texto para fixar.")));
        return false;
    }
    match parse_color(&text) {
        Some(c) => pin(color_card(c), &color::to_text(c, 0), None),
        None => pin(text_card(&text), &tr("Texto"), None),
    }
    true
}

pub fn show_all(show: bool) {
    HIDDEN.with(|x| x.set(!show));
    for p in PINS.with(|l| l.borrow().clone()) {
        unsafe {
            let _ = ShowWindow(h(p), if show { SW_SHOWNOACTIVATE } else { SW_HIDE });
        }
    }
}

pub fn close_all() {
    for p in PINS.with(|l| l.borrow().clone()) {
        close_pin(p);
    }
}

pub fn click_through_off_all() {
    for p in PINS.with(|l| l.borrow().clone()) {
        let s = unsafe { state(h(p)) };
        if !s.is_null() && unsafe { (*s).click_through } {
            unsafe {
                (*s).click_through = false;
                (*s).apply_styles();
                (*s).render();
            }
        }
    }
}

pub fn count() -> usize {
    PINS.with(|l| l.borrow().len())
}

/// The closed pins, newest first.
pub fn history_files() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(pins_dir())
        .map(|d| {
            d.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("pin_")) && p.extension().is_some_and(|e| e == "png"))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v.reverse();
    v
}

/// Back on the screen: it leaves the history (closing it adds it again).
pub fn reopen_history(i: usize) -> bool {
    let hist = history_files();
    let Some(f) = hist.get(i) else { return false };
    let Ok(img) = codec::load(f) else { return false };
    let _ = std::fs::remove_file(f);
    pin(img, &tr("Imagem fixada"), None);
    true
}

/// ImageSaveFilter, "|" between the parts.
pub(crate) fn image_filter() -> String {
    tr("PNG (*.png)|*.png|JPEG (*.jpg)|*.jpg|BMP (*.bmp)|*.bmp|GIF (*.gif)|*.gif|TIFF (*.tif)|*.tif|WebP (*.webp)|*.webp|")
}

/// The Save As dialog, in the default save folder; `filter` with "|" between the parts.
pub(crate) fn save_file_dialog(owner: isize, title: &str, name: &str, filter: &str, def_ext: &str) -> Option<String> {
    use windows::Win32::UI::Controls::Dialogs::{GetSaveFileNameW, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW};
    let filter: Vec<u16> = filter
        .encode_utf16()
        .map(|c| if c == '|' as u16 { 0 } else { c })
        .chain([0])
        .collect();
    let t = to_wide(title);
    let ext = to_wide(def_ext);
    let dir = to_wide(crate::engine::default_save_dir());
    let mut buf = vec![0u16; 4096];
    for (d, s) in buf.iter_mut().zip(name.encode_utf16()) {
        *d = s;
    }
    let mut ofn = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: h(owner),
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: windows::core::PWSTR(buf.as_mut_ptr()),
        nMaxFile: buf.len() as u32,
        lpstrInitialDir: PCWSTR(dir.as_ptr()),
        lpstrTitle: PCWSTR(t.as_ptr()),
        lpstrDefExt: PCWSTR(ext.as_ptr()),
        Flags: OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST,
        ..Default::default()
    };
    if !unsafe { GetSaveFileNameW(&mut ofn) }.as_bool() {
        return None;
    }
    let n = buf.iter().position(|&c| c == 0).unwrap_or(0);
    Some(String::from_utf16_lossy(&buf[..n]))
}

#[cfg(test)]
mod tests {
    use super::parse_color;

    #[test]
    fn reads_colors_like_the_cpp() {
        assert_eq!(parse_color("#FF8000"), Some(0x0080FF));
        assert_eq!(parse_color("ff8000"), Some(0x0080FF));
        assert_eq!(parse_color("rgb(255, 128, 0)"), Some(0x0080FF));
        assert_eq!(parse_color(" 255,128,0 "), Some(0x0080FF));
        assert_eq!(parse_color("rgb(300, 0, 0)"), None);
        assert_eq!(parse_color("hello"), None);
    }
}
