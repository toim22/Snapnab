//! One Click (OneClick.cpp; OneClickWindow, comp_7 §3): a floating panel docked to an edge of the screen. With the
//! mouse over it, it opens with the capture button and the gear; otherwise it collapses to a thin strip on the edge.
//! It can be dragged anywhere and snaps to the nearest edge when dropped (OneClickEdge, OneClickPos, OneClickMonX/Y).
//! It never shows up in captures (WDA_EXCLUDEFROMCAPTURE) and hides while a capture runs.

use std::cell::RefCell;
use std::ffi::OsStr;
use std::sync::Once;

use snapcore::gdip::{self, Brush, Graphics, Path, Pen};
use snapcore::image::Image;
use snapcore::reg;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint};
use windows::Win32::Graphics::GdiPlus::{PixelOffsetModeHalf, SmoothingModeAntiAlias};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetCursorPos, IDC_ARROW, KillTimer, LoadCursorW, MA_NOACTIVATE, RegisterClassW, SW_HIDE,
    SW_SHOWNOACTIVATE, SetTimer, SetWindowDisplayAffinity, ShowWindow, WDA_EXCLUDEFROMCAPTURE, WM_CAPTURECHANGED,
    WM_DISPLAYCHANGE, WM_DPICHANGED, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_TIMER, WNDCLASSW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::Win32::UI::Controls::{
    TOOLTIPS_CLASSW, TTF_ABSOLUTE, TTF_TRACK, TTM_ADDTOOLW, TTM_TRACKACTIVATE, TTM_TRACKPOSITION, TTS_ALWAYSTIP, TTS_NOPREFIX,
    TTTOOLINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::{SendMessageW, WINDOW_STYLE};
use windows::core::{PCWSTR, PWSTR, w};

use snapnab_app::tr;

use crate::pins::{UiEv, send};

const TIMER: usize = 1;
const ACCENT: u32 = 0xFF00_78D7;
const BG: u32 = 0xFFFA_FAFC;
const LINE: u32 = 0xFFDE_E0E4;
const RED: u32 = 0xFFE2_382C;

#[derive(Default)]
struct State {
    hwnd: isize,
    enabled: bool,
    busy: bool,
    /// 0 top, 1 right, 2 bottom, 3 left
    edge: i32,
    /// along the edge, 0..1
    pos: f64,
    mon_pt: POINT,
    /// 0 collapsed … 1 open (animated)
    open: f64,
    want_open: bool,
    dragging: bool,
    down: bool,
    menu: bool,
    hot: i32,
    pressed: i32,
    drag_start: POINT,
    drag_win: POINT,
    win: RECT,
    panel_at: POINT,
    left_at: u64,
    /// the "Start a capture" tooltip under the capture button
    tip: isize,
    tip_on: bool,
}

thread_local! {
    static S: RefCell<State> = RefCell::new(State { pos: 0.82, ..Default::default() });
}

fn sc(v: i32, dpi: u32) -> i32 {
    (v as i64 * dpi as i64 / 96) as i32
}

fn mix(a: u32, b: u32, t: f64) -> u32 {
    let c = |s: u32| {
        let (x, y) = (((a >> s) & 255) as f64, ((b >> s) & 255) as f64);
        ((x + (y - x) * t).round() as u32) << s
    };
    0xFF00_0000 | c(16) | c(8) | c(0)
}

fn round_path(x: f32, y: f32, w: f32, h: f32, rad: f32) -> Path {
    let p = Path::new();
    let d = (rad * 2.0).min(w).min(h);
    p.arc(x, y, d, d, 180.0, 90.0);
    p.arc(x + w - d, y, d, d, 270.0, 90.0);
    p.arc(x + w - d, y + h - d, d, d, 0.0, 90.0);
    p.arc(x, y + h - d, d, d, 90.0, 90.0);
    p.close_figure();
    p
}

fn now_ms() -> u64 {
    unsafe { windows::Win32::System::SystemInformation::GetTickCount64() }
}

impl State {
    fn dpi(&self) -> u32 {
        unsafe { GetDpiForWindow(HWND(self.hwnd as _)) }.max(96)
    }
    fn monitor(&self) -> RECT {
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        unsafe {
            let _ = GetMonitorInfoW(MonitorFromPoint(self.mon_pt, MONITOR_DEFAULTTOPRIMARY), &mut mi);
        }
        mi.rcMonitor
    }
    fn panel_size(&self, dpi: u32) -> (i32, i32) {
        if self.edge == 0 || self.edge == 2 || self.dragging { (sc(128, dpi), sc(74, dpi)) } else { (sc(74, dpi), sc(128, dpi)) }
    }

    /// The centers of the capture circle and of the gear, inside the panel.
    fn centers(&self, dpi: u32) -> ((f64, f64), (f64, f64)) {
        let (pw, ph) = self.panel_size(dpi);
        let band = sc(4, dpi) as f64;
        if pw > ph {
            let cy = ph as f64 / 2.0
                + if self.edge == 0 && !self.dragging { -band / 2.0 } else if self.edge == 2 && !self.dragging { band / 2.0 } else { 0.0 };
            ((pw as f64 * 0.40, cy), ((pw - sc(24, dpi)) as f64, cy))
        } else {
            let cx = pw as f64 / 2.0 + if self.edge == 3 { -band / 2.0 } else { band / 2.0 };
            ((cx, ph as f64 * 0.40), (cx, (ph - sc(24, dpi)) as f64))
        }
    }

    /// 1 the capture button, 2 the gear, 0 the panel, -1 outside.
    fn hit(&self, p: POINT) -> i32 {
        let r = self.win;
        if p.x < r.left || p.x >= r.right || p.y < r.top || p.y >= r.bottom {
            return -1;
        }
        let dpi = self.dpi();
        let (pw, ph) = self.panel_size(dpi);
        let (x, y) = (p.x - r.left - self.panel_at.x, p.y - r.top - self.panel_at.y);
        if x < 0 || y < 0 || x >= pw || y >= ph {
            return -1;
        }
        let ((cx, cy), (gx, gy)) = self.centers(dpi);
        let (x, y) = (x as f64, y as f64);
        let (rr, gr) = (sc(23, dpi) as f64, sc(14, dpi) as f64);
        if (x - cx).powi(2) + (y - cy).powi(2) <= rr * rr {
            1
        } else if (x - gx).powi(2) + (y - gy).powi(2) <= gr * gr {
            2
        } else {
            0
        }
    }

    /// Paints and positions the window (UpdateLayeredWindow).
    fn render(&mut self) {
        if self.hwnd == 0 {
            return;
        }
        let dpi = self.dpi();
        let (pw, ph) = self.panel_size(dpi);
        let (sh, strip, band) = (sc(10, dpi), sc(5, dpi), sc(4, dpi));
        let mon = self.monitor();
        let depth = if self.edge == 0 || self.edge == 2 { ph } else { pw };
        let vis = strip + ((depth - strip) as f64 * self.open).round() as i32;
        if self.dragging {
            self.win = RECT { left: self.drag_win.x, top: self.drag_win.y, right: self.drag_win.x + pw + 2 * sh,
                              bottom: self.drag_win.y + ph + 2 * sh };
            self.panel_at = POINT { x: sh, y: sh };
        } else {
            let (mw, mh) = (mon.right - mon.left, mon.bottom - mon.top);
            if self.edge == 0 || self.edge == 2 {
                let (w, h) = (pw + 2 * sh, vis + sh);
                let x = mon.left + (self.pos * (mw - w).max(0) as f64).round() as i32;
                let y = if self.edge == 0 { mon.top } else { mon.bottom - h };
                self.win = RECT { left: x, top: y, right: x + w, bottom: y + h };
                self.panel_at = POINT { x: sh, y: if self.edge == 0 { vis - ph } else { sh } };
            } else {
                let (w, h) = (vis + sh, ph + 2 * sh);
                let y = mon.top + (self.pos * (mh - h).max(0) as f64).round() as i32;
                let x = if self.edge == 3 { mon.left } else { mon.right - w };
                self.win = RECT { left: x, top: y, right: x + w, bottom: y + h };
                self.panel_at = POINT { x: if self.edge == 3 { vis - pw } else { sh }, y: sh };
            }
        }
        let (ww, hh) = (self.win.right - self.win.left, self.win.bottom - self.win.top);
        let mut canvas = Image::sized(ww, hh, 0);
        {
            let bmp = gdip::Bitmap::wrap(&mut canvas);
            let g = Graphics::new(&bmp);
            g.smoothing(SmoothingModeAntiAlias);
            g.pixel_offset(PixelOffsetModeHalf);
            let rad = sc(12, dpi) as f32;
            let (px, py, pwf, phf) = (self.panel_at.x as f32, self.panel_at.y as f32, pw as f32, ph as f32);
            for i in (1..=sh).rev() {
                // the soft shadow, in layers of decreasing opacity
                let fi = i as f32;
                let sp = round_path(px - fi, py - fi + fi * 0.35, pwf + 2.0 * fi, phf + 2.0 * fi, rad + fi);
                let t = 1.0 - i as f64 / (sh + 1) as f64;
                g.fill_path(&Brush::solid(((10.0 * t * t).round() as u32) << 24), &sp);
            }
            let panel = round_path(px, py, pwf, phf, rad);
            g.fill_path(&Brush::solid(BG), &panel);
            if !self.dragging {
                // the accent strip on the inner side: what stays visible while the panel is collapsed
                let b = band as f32;
                let r = match self.edge {
                    0 => gdip::rect(px, py + phf - b, pwf, b),
                    2 => gdip::rect(px, py, pwf, b),
                    1 => gdip::rect(px, py, b, phf),
                    _ => gdip::rect(px + pwf - b, py, b, phf),
                };
                let st = g.save();
                g.clip_path(&panel);
                g.fill_rect(&Brush::solid(ACCENT), r);
                g.restore(st);
            }
            g.draw_path(&Pen::new(LINE, 1.0), &panel);
            let ((cx, cy), (gx, gy)) = self.centers(dpi);
            let (cx, cy, gx, gy) = (px + cx as f32, py + cy as f32, px + gx as f32, py + gy as f32);
            let (d, gs) = (sc(46, dpi) as f32, sc(28, dpi) as f32);
            let cap = if self.pressed == 1 && self.hot == 1 {
                mix(RED, 0xFF00_0000, 0.18)
            } else if self.hot == 1 {
                mix(RED, 0xFFFF_FFFF, 0.12)
            } else {
                RED
            };
            g.fill_ellipse(&Brush::solid(0x2800_0000), cx - d / 2.0, cy - d / 2.0 + sc(2, dpi) as f32, d, d);
            g.fill_ellipse(&Brush::solid(cap), cx - d / 2.0, cy - d / 2.0, d, d);
            if self.hot == 2 {
                g.fill_ellipse(&Brush::solid(mix(BG, ACCENT, if self.pressed == 2 { 0.22 } else { 0.12 })), gx - gs / 2.0, gy - gs / 2.0,
                               gs, gs);
            }
            draw_gear(&g, gx, gy, sc(16, dpi) as f32, 0xFF50_5058);
            g.flush();
        }
        crate::rawwin::present(self.hwnd, &canvas, (self.win.left, self.win.top));
    }

    fn tick(&mut self) {
        if !self.enabled || self.busy {
            return;
        }
        let mut p = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut p);
        }
        let hit = self.hit(p);
        let now = now_ms();
        if hit >= 0 || self.dragging {
            self.want_open = true;
            self.left_at = 0;
        } else if !self.menu {
            if self.left_at == 0 {
                self.left_at = now;
            }
            if now - self.left_at > 450 {
                self.want_open = false;
            }
        }
        let mut dirty = false;
        let target = if self.want_open { 1.0 } else { 0.0 };
        if self.open != target {
            self.open = if (target - self.open).abs() <= 0.2 { target } else { self.open + if target > self.open { 0.2 } else { -0.2 } };
            dirty = true;
        }
        let h = if self.open >= 1.0 && !self.dragging { hit.max(0) } else { 0 };
        if h != self.hot {
            self.hot = h;
            dirty = true;
        }
        if dirty {
            self.render();
        }
        self.show_tip(self.hot == 1 && !self.down);
    }

    /// ShowTip: the tooltip next to the panel, on the side away from the edge.
    fn show_tip(&mut self, on: bool) {
        if self.tip == 0 || on == self.tip_on {
            return;
        }
        self.tip_on = on;
        let ti = TTTOOLINFOW { cbSize: std::mem::size_of::<TTTOOLINFOW>() as u32, hwnd: HWND(self.hwnd as _), uId: 1, ..Default::default() };
        let tip = HWND(self.tip as _);
        unsafe {
            if on {
                let dpi = self.dpi();
                let r = self.win;
                let (mut x, mut y) = (r.left + self.panel_at.x + self.panel_size(dpi).0 / 2, r.bottom + sc(4, dpi));
                match self.edge {
                    2 => y = r.top - sc(28, dpi),
                    1 => (x, y) = (r.left - sc(120, dpi), (r.top + r.bottom) / 2),
                    3 => (x, y) = (r.right + sc(4, dpi), (r.top + r.bottom) / 2),
                    _ => {}
                }
                let xy = ((x as u32) & 0xFFFF) | (((y as u32) & 0xFFFF) << 16);
                SendMessageW(tip, TTM_TRACKPOSITION, None, Some(LPARAM(xy as isize)));
            }
            SendMessageW(tip, TTM_TRACKACTIVATE, Some(WPARAM(on as usize)), Some(LPARAM(&ti as *const _ as isize)));
        }
    }

    /// Snaps to the edge of the monitor nearest to where the panel was dropped.
    fn snap(&mut self, c: POINT) {
        self.mon_pt = c;
        let mon = self.monitor();
        let (cx, cy) = ((self.win.left + self.win.right) / 2, (self.win.top + self.win.bottom) / 2);
        let dist = [cy - mon.top, mon.right - cx, mon.bottom - cy, cx - mon.left];
        self.edge = (0..4).min_by_key(|&i| dist[i]).unwrap_or(0) as i32;
        let (w, h) = (self.win.right - self.win.left, self.win.bottom - self.win.top);
        self.pos = if self.edge == 0 || self.edge == 2 {
            ((self.win.left - mon.left) as f64 / ((mon.right - mon.left) - w).max(1) as f64).clamp(0.0, 1.0)
        } else {
            ((self.win.top - mon.top) as f64 / ((mon.bottom - mon.top) - h).max(1) as f64).clamp(0.0, 1.0)
        };
        let set = |n: &str, v: u32| reg::set_dword(OsStr::new(n), v, None);
        set("OneClickEdge", self.edge as u32);
        set("OneClickPos", (self.pos * 1000.0).round() as u32);
        set("OneClickMonX", self.mon_pt.x as u32);
        set("OneClickMonY", self.mon_pt.y as u32);
    }
}

/// A gear of eight teeth around a hole, centered at (x, y) in a square of side `s`.
fn draw_gear(g: &Graphics, x: f32, y: f32, s: f32, c: u32) {
    let (ro, ri) = (s / 2.0, s * 0.36);
    let mut pts = Vec::new();
    for i in 0..32 {
        let a = i as f32 * std::f32::consts::PI * 2.0 / 32.0;
        let r = if (i / 2) % 2 == 0 { ro } else { ri };
        pts.push(gdip::pt(x + r * a.cos(), y + r * a.sin()));
    }
    g.fill_polygon(&Brush::solid(c), &pts);
    let h = s * 0.32;
    g.fill_ellipse(&Brush::solid(BG), x - h / 2.0, y - h / 2.0, h, h);
}

fn cursor() -> POINT {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    p
}

unsafe extern "system" fn proc(hwnd: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match m {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_TIMER if wp.0 == TIMER => {
            S.with(|s| s.borrow_mut().tick());
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let p = cursor();
            S.with(|s| {
                let mut s = s.borrow_mut();
                s.show_tip(false);
                s.down = true;
                s.pressed = s.hit(p).max(0);
                s.drag_start = p;
                s.drag_win = POINT { x: s.win.left, y: s.win.top };
                s.render();
            });
            unsafe { SetCapture(hwnd) };
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let p = cursor();
            S.with(|s| {
                let mut s = s.borrow_mut();
                if !s.down {
                    return;
                }
                let dpi = s.dpi();
                let th = sc(5, dpi);
                if !s.dragging && ((p.x - s.drag_start.x).abs() > th || (p.y - s.drag_start.y).abs() > th) {
                    // the panel comes loose, whole, under the mouse
                    s.dragging = true;
                    let sh = sc(10, dpi);
                    let (pw, ph) = s.panel_size(dpi);
                    s.drag_win = POINT { x: p.x - pw / 2 - sh, y: p.y - ph / 2 - sh };
                    s.drag_start = p;
                    s.hot = 0;
                }
                if s.dragging {
                    s.drag_win = POINT { x: s.drag_win.x + p.x - s.drag_start.x, y: s.drag_win.y + p.y - s.drag_start.y };
                    s.drag_start = p;
                    s.mon_pt = p;
                    s.render();
                }
            });
            LRESULT(0)
        }
        WM_LBUTTONUP | WM_CAPTURECHANGED => {
            let p = cursor();
            // what to do after the borrow ends: 1 capture, 2 the menu at a point
            let act = S.with(|s| {
                let mut s = s.borrow_mut();
                if !s.down {
                    return None;
                }
                s.down = false;
                if s.dragging {
                    s.snap(p);
                    s.dragging = false;
                    s.open = 1.0;
                    s.pressed = 0;
                    s.render();
                    return None;
                }
                let (hit, was) = (s.hit(p), s.pressed);
                s.pressed = 0;
                s.render();
                if m != WM_LBUTTONUP || hit != was {
                    return None;
                }
                if hit == 1 {
                    s.want_open = false;
                    s.open = 0.0;
                    s.hot = 0;
                    s.render();
                    return Some(UiEv::Capture);
                }
                if hit == 2 {
                    let dpi = s.dpi();
                    let x = s.win.left + s.panel_at.x + s.panel_size(dpi).0 - sc(24, dpi);
                    let y = if s.edge == 2 { s.win.top } else { s.win.bottom };
                    s.menu = true;
                    return Some(UiEv::OneClickMenu(x, y));
                }
                None
            });
            if m == WM_LBUTTONUP {
                unsafe {
                    let _ = ReleaseCapture();
                }
            }
            if let Some(a) = act {
                send(a);
            }
            LRESULT(0)
        }
        WM_DPICHANGED | WM_DISPLAYCHANGE => {
            S.with(|s| s.borrow_mut().render());
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, m, wp, lp) },
    }
}

/// The gear's menu has closed: the panel may collapse again.
pub fn menu_closed() {
    S.with(|s| s.borrow_mut().menu = false);
}

pub fn enabled() -> bool {
    S.with(|s| s.borrow().enabled)
}

/// Shows or hides the panel (the setting is saved by the caller).
pub fn show(on: bool) {
    S.with(|s| {
        let mut s = s.borrow_mut();
        s.enabled = on;
        if !on {
            s.show_tip(false);
            if s.hwnd != 0 {
                unsafe {
                    let _ = KillTimer(Some(HWND(s.hwnd as _)), TIMER);
                    let _ = ShowWindow(HWND(s.hwnd as _), SW_HIDE);
                }
            }
            return;
        }
        if s.hwnd == 0 {
            static REG: Once = Once::new();
            REG.call_once(|| unsafe {
                let wc = WNDCLASSW {
                    lpfnWndProc: Some(proc),
                    hInstance: windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default().into(),
                    hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                    lpszClassName: w!("SnapnabOneClick"),
                    ..Default::default()
                };
                RegisterClassW(&wc);
            });
            let h = unsafe {
                CreateWindowExW(WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE, w!("SnapnabOneClick"),
                                w!("Snapnab One Click"), WS_POPUP, 0, 0, 10, 10, None, None, None, None)
            };
            let Ok(h) = h else { return };
            unsafe {
                let _ = SetWindowDisplayAffinity(h, WDA_EXCLUDEFROMCAPTURE); // never in the captures
            }
            s.hwnd = h.0 as isize;
            if let Ok(tip) = unsafe {
                CreateWindowExW(WS_EX_TOPMOST, TOOLTIPS_CLASSW, PCWSTR::null(), WS_POPUP | WINDOW_STYLE(TTS_NOPREFIX | TTS_ALWAYSTIP), 0, 0, 0,
                                0, Some(h), None, None, None)
            } {
                let text = snapcore::wide::to_wide(tr("Iniciar uma captura"));
                let ti = TTTOOLINFOW {
                    cbSize: std::mem::size_of::<TTTOOLINFOW>() as u32,
                    uFlags: TTF_TRACK | TTF_ABSOLUTE,
                    hwnd: h,
                    uId: 1,
                    lpszText: PWSTR(text.as_ptr() as *mut u16),
                    ..Default::default()
                };
                unsafe {
                    let _ = SetWindowDisplayAffinity(tip, WDA_EXCLUDEFROMCAPTURE);
                    SendMessageW(tip, TTM_ADDTOOLW, None, Some(LPARAM(&ti as *const _ as isize)));
                }
                s.tip = tip.0 as isize;
            }
        }
        let get = |n: &str, d: u32| reg::get_dword(OsStr::new(n), d, None);
        s.edge = (get("OneClickEdge", 0) as i32).clamp(0, 3);
        s.pos = (get("OneClickPos", 820) as f64 / 1000.0).clamp(0.0, 1.0);
        s.mon_pt = POINT { x: get("OneClickMonX", 0) as i32, y: get("OneClickMonY", 0) as i32 };
        (s.open, s.want_open) = (0.0, false);
        s.render();
        unsafe {
            if !s.busy {
                let _ = ShowWindow(HWND(s.hwnd as _), SW_SHOWNOACTIVATE);
            }
            SetTimer(Some(HWND(s.hwnd as _)), TIMER, 30, None);
        }
    });
}

/// Hidden while a capture runs (DisableOneClickUI).
pub fn set_busy(busy: bool) {
    S.with(|s| {
        let mut s = s.borrow_mut();
        if s.busy == busy {
            return;
        }
        s.busy = busy;
        if s.hwnd == 0 || !s.enabled {
            return;
        }
        if busy {
            s.show_tip(false);
            unsafe {
                let _ = ShowWindow(HWND(s.hwnd as _), SW_HIDE);
            }
        } else {
            (s.open, s.want_open) = (0.0, false);
            s.render();
            unsafe {
                let _ = ShowWindow(HWND(s.hwnd as _), SW_SHOWNOACTIVATE);
            }
        }
    });
}
