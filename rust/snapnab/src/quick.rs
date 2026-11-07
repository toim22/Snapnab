//! The quick access overlay after a capture (QuickOverlay.cpp, C08; ShareX dossier 01 · F08): a small floating
//! thumbnail in a corner of the screen with Copy, Save, Annotate (Editor), Pin, Delete and Close. A click on the picture
//! opens it in the Editor; it closes by itself after OverlaySeconds (paused while the mouse is over it); up to five
//! stack from the corner (OverlayCorner: 0 bottom right, 1 bottom left, 2 top right, 3 top left). Right click: read
//! the QR code, the same actions, close all. Drag-out and "Copy as…" come later.

use std::cell::RefCell;
use std::ffi::OsStr;
use std::path::Path as FsPath;
use std::sync::Once;

use snapcore::gdip::{self, Brush, Font, Graphics, StringFormat};
use snapcore::image::Image;
use snapcore::{clip, codec, library, log, reg, screen};
use snapnab_app::tr;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, ClientToScreen, CreateCompatibleDC, DeleteDC, DeleteObject, EndPaint, InvalidateRect, PAINTSTRUCT,
    SRCCOPY, SelectObject,
};
use windows::Win32::Graphics::GdiPlus::{FontStyleBold, SmoothingModeAntiAlias, StringAlignmentCenter, TextRenderingHintAntiAliasGridFit};
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXDRAG, SM_CYDRAG};
use windows::Win32::UI::WindowsAndMessaging::{
    CS_DROPSHADOW, CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA, GetClientRect, GetWindowLongPtrW,
    HWND_TOPMOST, IDC_ARROW, IDC_HAND, LoadCursorW, MA_NOACTIVATE, RegisterClassW, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetCursor,
    SetTimer, SetWindowLongPtrW, SetWindowPos, WM_DESTROY, WM_ERASEBKGND, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE,
    WM_MOUSEMOVE, WM_NCDESTROY, WM_PAINT, WM_RBUTTONUP, WM_SETCURSOR, WM_TIMER, WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::w;

use crate::pins::{Glyph, UiEv, draw_glyph, send};
use crate::tray::{Item, Menu};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Act {
    Copy,
    Save,
    Annotate,
    Pin,
    Delete,
    Close,
}

const ACTS: [Act; 6] = [Act::Copy, Act::Save, Act::Annotate, Act::Pin, Act::Delete, Act::Close];

fn tip(a: Act) -> String {
    tr(match a {
        Act::Copy => "Copiar",
        Act::Save => "Salvar como…",
        Act::Annotate => "Anotar no Editor",
        Act::Pin => "Fixar na tela",
        Act::Delete => "Excluir da biblioteca",
        Act::Close => "Fechar",
    })
}

struct Card {
    h: isize,
    img: Image,
    thumb: Image,
    lib: String,
    title: String,
    video: bool,
    hover: bool,
    hot: Option<Act>,
    /// seconds until it closes (0 = never)
    left: i32,
    pressed: bool,
    /// where the button went down (client coordinates)
    down: POINT,
    thumb_r: RECT,
    btn: Vec<(RECT, Act)>,
}

thread_local! {
    static CARDS: RefCell<Vec<isize>> = const { RefCell::new(Vec::new()) };
}

fn h(x: isize) -> HWND {
    HWND(x as _)
}

fn sc(v: i32, dpi: u32) -> i32 {
    (v as i64 * dpi as i64 / 96) as i32
}

unsafe fn card(hwnd: HWND) -> *mut Card {
    unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Card }
}

fn dpi_of(x: isize) -> u32 {
    unsafe { GetDpiForWindow(h(x)) }.max(96)
}

fn card_size(c: &Card, dpi: u32) -> (i32, i32) {
    let (pad, bar) = (sc(8, dpi), sc(36, dpi));
    (c.thumb.wi() + 2 * pad, c.thumb.hi() + 2 * pad + bar)
}

/// Stacks the cards from the corner (the newest at the corner).
fn layout() {
    let cards = CARDS.with(|l| l.borrow().clone());
    let Some(&last) = cards.last() else { return };
    let (cx, cy) = screen::cursor_pos();
    let wa = screen::monitor_rect(cx, cy, true);
    let corner = reg::get_dword(OsStr::new("OverlayCorner"), 0, None).min(3);
    let dpi = dpi_of(last);
    let gap = sc(10, dpi);
    let mut y = if corner < 2 { wa.bottom - gap } else { wa.top + gap };
    for &hc in cards.iter().rev() {
        let c = unsafe { card(h(hc)) };
        if c.is_null() {
            continue;
        }
        let (w, hh) = card_size(unsafe { &*c }, dpi);
        let x = if corner == 0 || corner == 2 { wa.right - gap - w } else { wa.left + gap };
        let top = if corner < 2 { y - hh } else { y };
        unsafe {
            let _ = SetWindowPos(h(hc), Some(HWND_TOPMOST), x, top, w, hh, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        }
        y = if corner < 2 { top - gap } else { top + hh + gap };
    }
}

fn close_card(x: isize) {
    unsafe {
        let _ = DestroyWindow(h(x));
    }
}

fn paint(c: &mut Card) -> Image {
    let mut cr = RECT::default();
    unsafe {
        let _ = GetClientRect(h(c.h), &mut cr);
    }
    let dpi = dpi_of(c.h);
    let pad = sc(8, dpi);
    let mut out = Image::sized(cr.right.max(1), cr.bottom.max(1), 0xFF20_2024);
    c.thumb_r = RECT { left: pad, top: pad, right: pad + c.thumb.wi(), bottom: pad + c.thumb.hi() };
    out.blit(&c.thumb, pad, pad, false);
    {
        let bmp = gdip::Bitmap::wrap(&mut out);
        let g = Graphics::new(&bmp);
        g.smoothing(SmoothingModeAntiAlias);
        g.text_hint(TextRenderingHintAntiAliasGridFit);
        if c.video {
            // the play badge on video thumbnails
            let s = sc(30, dpi) as f32;
            let (x, y) = ((c.thumb_r.left + c.thumb_r.right) as f32 / 2.0, (c.thumb_r.top + c.thumb_r.bottom) as f32 / 2.0);
            g.fill_ellipse(&Brush::solid(0xAA00_0000), x - s / 2.0, y - s / 2.0, s, s);
            g.fill_polygon(&Brush::solid(0xFFFF_FFFF),
                           &[gdip::pt(x - s / 6.0, y - s / 4.0), gdip::pt(x + s / 4.0, y), gdip::pt(x - s / 6.0, y + s / 4.0)]);
        }
        // the buttons (always visible: the overlay is small and quick)
        let (bs, gap) = (sc(28, dpi), sc(4, dpi));
        let y = cr.bottom - bs - sc(4, dpi);
        let n = ACTS.len() as i32;
        let mut x = (cr.right - (n * bs + (n - 1) * gap)) / 2;
        c.btn.clear();
        for a in ACTS {
            let r = RECT { left: x, top: y, right: x + bs, bottom: y + bs };
            c.btn.push((r, a));
            let disabled = c.video && a == Act::Pin;
            if c.hot == Some(a) && !disabled {
                g.fill_ellipse(&Brush::solid(if a == Act::Delete { 0xFFC4_2B1C } else { 0xFF46_464E }), x as f32, y as f32, bs as f32,
                               bs as f32);
            }
            let ip = (bs / 4) as f32;
            let glyph = match a {
                Act::Copy => Glyph::Copy,
                Act::Save => Glyph::Save,
                Act::Annotate => Glyph::Pen,
                Act::Pin => Glyph::Pin,
                Act::Delete => Glyph::Trash,
                Act::Close => Glyph::Cancel,
            };
            draw_glyph(&g, glyph, x as f32 + ip, y as f32 + ip, bs as f32 - 2.0 * ip,
                       if disabled { 0xFF64_6468 } else { 0xFFEB_EBF0 });
            x += bs + gap;
        }
        if let Some(a) = c.hot {
            // the button's name, over the bottom of the picture
            let t: Vec<u16> = tip(a).encode_utf16().collect();
            let f = Font::new("Segoe UI", sc(12, dpi) as f32, FontStyleBold);
            let fmt = StringFormat::new();
            let (tw0, _) = g.measure_in(&t, &f, 1000.0, 100.0, &fmt);
            let (tw, th) = (tw0.ceil() as i32 + sc(18, dpi), sc(22, dpi));
            let (l, tp) = ((cr.right - tw) / 2, c.thumb_r.bottom - th - sc(4, dpi));
            let pill = gdip::Path::new();
            let d = th as f32;
            pill.arc(l as f32, tp as f32, d, d, 90.0, 180.0);
            pill.arc((l + tw) as f32 - d, tp as f32, d, d, 270.0, 180.0);
            pill.close_figure();
            g.fill_path(&Brush::solid(0xFF14_1416), &pill);
            fmt.align(StringAlignmentCenter);
            fmt.line_align(StringAlignmentCenter);
            g.draw_string(&t, &f, gdip::rect(l as f32, tp as f32, tw as f32, th as f32), &fmt, &Brush::solid(0xFFFF_FFFF));
        }
        let full = reg::get_dword(OsStr::new("OverlaySeconds"), 8, None) as i32;
        if c.left > 0 && !c.hover && full > 0 {
            // the countdown along the bottom
            let k = c.left as f32 / full as f32;
            g.fill_rect(&Brush::solid(0xFF00_78D4), gdip::rect(0.0, cr.bottom as f32 - 3.0, cr.right as f32 * k, 3.0));
        }
        g.flush();
    }
    out
}

fn hit(c: &Card, p: POINT) -> Option<Act> {
    c.btn.iter().find(|(r, _)| p.x >= r.left && p.x < r.right && p.y >= r.top && p.y < r.bottom).map(|b| b.1)
}

fn in_rect(r: &RECT, p: POINT) -> bool {
    p.x >= r.left && p.x < r.right && p.y >= r.top && p.y < r.bottom
}

fn do_act(hc: isize, a: Act) {
    let c = unsafe { card(h(hc)) };
    if c.is_null() {
        return;
    }
    let c = unsafe { &*c };
    match a {
        Act::Copy => {
            if c.video {
                clip::copy_files(c.h, std::slice::from_ref(&c.lib));
            } else {
                clip::copy_image(c.h, &c.img, crate::VERSION);
            }
            send(UiEv::Notify("Snapnab".into(), tr("Copiado para a área de transferência.")));
        }
        Act::Save => {
            if c.video {
                let name = if c.title.is_empty() { tr("Vídeo") } else { c.title.clone() };
                if let Some(p) = crate::pins::save_file_dialog(c.h, &tr("Salvar o vídeo"), &name, &tr("Vídeo MP4 (*.mp4)|*.mp4|"), "mp4") {
                    let _ = std::fs::copy(&c.lib, p);
                }
            } else {
                let name = if c.title.is_empty() { tr("Captura") } else { c.title.clone() };
                let name = name.replace([':', '\\', '/', '*', '?', '"', '<', '>', '|'], "_");
                if let Some(mut p) = crate::pins::save_file_dialog(c.h, &tr("Salvar como"), &name, &crate::pins::image_filter(), "png") {
                    if FsPath::new(&p).extension().is_none() {
                        p += ".png";
                    }
                    let ext = FsPath::new(&p).extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
                    if !codec::save_file(FsPath::new(&p), &c.img, codec::Format::from_ext(&ext), &codec::SaveOptions::from_settings(),
                                         crate::VERSION)
                    {
                        crate::engine::message_box(tr("Não foi possível salvar:\n") + &p, "Snapnab".into());
                    }
                }
            }
        }
        Act::Annotate => {
            if !c.lib.is_empty() {
                send(UiEv::Editor(c.lib.clone()));
            }
            close_card(hc);
        }
        Act::Pin => {
            if !c.video {
                let (img, title) = (c.img.clone(), c.title.clone());
                close_card(hc);
                crate::pins::pin(img, &title, None);
            }
        }
        Act::Delete => {
            if !c.lib.is_empty() {
                let item: Vec<u16> = c.lib.encode_utf16().collect();
                if library::delete_item(&library::library_dir(), &item, h(c.h)) {
                    send(UiEv::LibraryChanged);
                    close_card(hc);
                }
            }
        }
        Act::Close => close_card(hc),
    }
}

fn context_menu(hc: isize, at: POINT) {
    const COPY_AS: u32 = 1;
    const QR: u32 = 2;
    const UPLOAD: u32 = 3;
    const CLOSE_ALL: u32 = 4;
    let c = unsafe { card(h(hc)) };
    if c.is_null() {
        return;
    }
    let video = unsafe { (*c).video };
    let has_img = unsafe { !(*c).img.is_empty() };
    let copy_as = tr("Copiar como…");
    let qr = tr("Ler código QR");
    let up = tr("Enviar (upload) e copiar o link");
    let all = tr("Fechar todas");
    let names: Vec<String> = ACTS.iter().map(|&a| tip(a)).collect();
    let mut items = vec![Item::Cmd(COPY_AS, &copy_as)];
    if !video {
        items.push(Item::Cmd(QR, &qr));
    }
    if has_img {
        items.push(Item::Cmd(UPLOAD, &up));
    }
    items.push(Item::Separator);
    for (i, a) in ACTS.iter().enumerate() {
        if !(video && *a == Act::Pin) {
            items.push(Item::Cmd(100 + i as u32, &names[i]));
        }
    }
    items.push(Item::Cmd(CLOSE_ALL, &all));
    let id = Menu::new(items).choose(hc, at.x, at.y);
    let c = unsafe { card(h(hc)) };
    if c.is_null() {
        return;
    }
    match id {
        COPY_AS => {
            let (img, file, title) = unsafe { ((*c).img.clone(), if (*c).video { (*c).lib.clone() } else { String::new() }, (*c).title.clone()) };
            crate::share::copy_as_menu(hc, (at.x, at.y), &img, &file, &title);
        }
        QR => send(UiEv::Qr(unsafe { (*c).img.clone() })),
        UPLOAD => send(UiEv::Upload(unsafe { (*c).img.clone() }, unsafe { (*c).lib.clone() })),
        CLOSE_ALL => close_all(),
        i if (100..100 + ACTS.len() as u32).contains(&i) => do_act(hc, ACTS[(i - 100) as usize]),
        _ => {}
    }
}

fn lp_point(lp: LPARAM) -> POINT {
    POINT { x: (lp.0 & 0xFFFF) as i16 as i32, y: ((lp.0 >> 16) & 0xFFFF) as i16 as i32 }
}

fn invalidate(x: isize) {
    unsafe {
        let _ = InvalidateRect(Some(h(x)), None, false);
    }
}

unsafe extern "system" fn card_proc(hwnd: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let ptr = unsafe { card(hwnd) };
    if ptr.is_null() {
        return unsafe { DefWindowProcW(hwnd, m, wp, lp) };
    }
    let hc = hwnd.0 as isize;
    match m {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let img = paint(unsafe { &mut *ptr });
            unsafe {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                if let Some(hb) = screen::to_hbitmap(&img) {
                    let mem = CreateCompatibleDC(Some(dc));
                    let o = SelectObject(mem, hb.into());
                    let _ = BitBlt(dc, 0, 0, img.wi(), img.hi(), Some(mem), 0, 0, SRCCOPY);
                    SelectObject(mem, o);
                    let _ = DeleteObject(hb.into());
                    let _ = DeleteDC(mem);
                }
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_SETCURSOR => {
            let hot = unsafe { (*ptr).hot.is_some() };
            unsafe {
                SetCursor(LoadCursorW(None, if hot { IDC_HAND } else { IDC_ARROW }).ok());
            }
            LRESULT(1)
        }
        WM_MOUSEMOVE => {
            let c = unsafe { &mut *ptr };
            if !c.hover {
                c.hover = true;
                let mut t = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd,
                                              dwHoverTime: 0 };
                unsafe {
                    let _ = TrackMouseEvent(&mut t);
                }
            }
            let p = lp_point(lp);
            let hot = hit(c, p);
            if hot != c.hot {
                c.hot = hot;
                invalidate(hc);
            }
            let (dx, dy) = unsafe { (GetSystemMetrics(SM_CXDRAG), GetSystemMetrics(SM_CYDRAG)) };
            if c.pressed && in_rect(&c.thumb_r, c.down) && ((p.x - c.down.x).abs() > dx || (p.y - c.down.y).abs() > dy) {
                c.pressed = false;
                unsafe {
                    let _ = ReleaseCapture();
                }
                // the capture goes into another program
                let file = if c.video {
                    Some(c.lib.clone())
                } else {
                    let name = if c.title.is_empty() { format!("Snapnab_{}", String::from_utf16_lossy(&library::time_stamp(false))) } else { c.title.clone() };
                    snapcore::dragout::temp_image_file(&c.img, &name, false, crate::VERSION)
                };
                if let Some(f) = file.filter(|f| !f.is_empty()) {
                    let (thumb, video) = (c.thumb.clone(), c.video);
                    snapcore::dragout::drag_files_out(hwnd.0 as isize, &[f], Some(&thumb), !video);
                }
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            let c = unsafe { &mut *ptr };
            (c.hover, c.hot) = (false, None);
            invalidate(hc);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            unsafe {
                (*ptr).down = lp_point(lp);
                (*ptr).pressed = true;
                SetCapture(hwnd);
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let p = lp_point(lp);
            let c = unsafe { &mut *ptr };
            let click = c.pressed;
            c.pressed = false;
            unsafe {
                let _ = ReleaseCapture();
            }
            if click {
                // a click on the picture opens it in the Editor
                let a = hit(c, p).or(in_rect(&c.thumb_r, p).then_some(Act::Annotate));
                if let Some(a) = a {
                    do_act(hc, a);
                }
            }
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            let mut s = lp_point(lp);
            unsafe {
                let _ = ClientToScreen(hwnd, &mut s);
            }
            context_menu(hc, s);
            LRESULT(0)
        }
        WM_TIMER => {
            let c = unsafe { &mut *ptr };
            if c.hover || c.left <= 0 {
                return LRESULT(0); // paused while the mouse is over it
            }
            c.left -= 1;
            if c.left <= 0 {
                close_card(hc);
            } else {
                invalidate(hc);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            CARDS.with(|l| l.borrow_mut().retain(|&x| x != hc));
            LRESULT(0)
        }
        WM_NCDESTROY => unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            drop(Box::from_raw(ptr));
            layout();
            DefWindowProcW(hwnd, m, wp, lp)
        },
        _ => unsafe { DefWindowProcW(hwnd, m, wp, lp) },
    }
}

/// A new card for a capture (`lib` its library file) or a recording (`video`, `img` its first frame).
pub fn show(img: &Image, lib: &str, video: bool, title: &str) {
    static REG: Once = Once::new();
    REG.call_once(|| unsafe {
        let wc = WNDCLASSW {
            style: CS_DROPSHADOW,
            lpfnWndProc: Some(card_proc),
            hInstance: windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default().into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: w!("SnapnabQuickOverlay"),
            ..Default::default()
        };
        RegisterClassW(&wc);
    });
    if img.is_empty() {
        return;
    }
    while CARDS.with(|l| l.borrow().len()) >= 5 {
        // up to five stacked
        let first = CARDS.with(|l| l.borrow()[0]);
        close_card(first);
    }
    let Ok(hw) = (unsafe {
        CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE, w!("SnapnabQuickOverlay"), w!("Snapnab"), WS_POPUP, 0, 0,
                        10, 10, None, None, None, None)
    }) else {
        return;
    };
    let dpi = dpi_of(hw.0 as isize);
    let mut thumb = gdip::make_thumbnail(img, sc(240, dpi), sc(150, dpi));
    let min_w = sc(200, dpi);
    let mut flat = Image::sized(thumb.wi().max(min_w), thumb.hi(), 0xFF20_2024);
    flat.blit(&thumb, (flat.wi() - thumb.wi()) / 2, 0, true); // narrow captures: the card keeps room for the buttons
    thumb = flat;
    let left = reg::get_dword(OsStr::new("OverlaySeconds"), 8, None) as i32;
    let c = Box::new(Card {
        h: hw.0 as isize,
        img: img.clone(),
        thumb,
        lib: lib.to_string(),
        title: title.to_string(),
        video,
        hover: false,
        hot: None,
        left,
        pressed: false,
        down: POINT::default(),
        thumb_r: RECT::default(),
        btn: Vec::new(),
    });
    unsafe {
        SetWindowLongPtrW(hw, GWLP_USERDATA, Box::into_raw(c) as isize);
        let pref = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(hw, DWMWA_WINDOW_CORNER_PREFERENCE, &pref as *const _ as _, std::mem::size_of_val(&pref) as u32);
        if left > 0 {
            SetTimer(Some(hw), 1, 1000, None);
        }
    }
    CARDS.with(|l| l.borrow_mut().push(hw.0 as isize));
    layout();
    log::write(&format!("Quick overlay: {}x{}{}", img.w, img.h, if video { " (video)" } else { "" }));
}

pub fn close_all() {
    for c in CARDS.with(|l| l.borrow().clone()) {
        close_card(c);
    }
}
