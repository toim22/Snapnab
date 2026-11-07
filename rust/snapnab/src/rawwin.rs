//! The plain Win32 windows of a recording, beside the egui HUD: the red frame around the area and the countdown
//! (RecorderHud.cpp). Both are layered windows the screen copy never sees (WDA_EXCLUDEFROMCAPTURE): the frame is a
//! window region filled red, the countdown a per-pixel alpha bitmap drawn with GDI+ (antialiased edges, which a window
//! with a GPU surface cannot have here). Also the native tooltips of the HUD's buttons, which an egui tooltip could not
//! show outside the HUD's 52 pt.

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering::Relaxed};
use std::sync::Once;

use snapcore::gdip::{self, Brush, Font, Graphics, Path, Pen};
use snapcore::image::{Image, Rect};
use snapcore::wide::to_wide;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, CombineRgn, CreateCompatibleDC,
    CreateDIBSection, CreateRectRgn, CreateSolidBrush, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, RGN_DIFF, ReleaseDC,
    ScreenToClient, SelectObject, SetWindowRgn,
};
use windows::Win32::Graphics::GdiPlus::{
    FontStyleBold, PixelOffsetModeHalf, SmoothingModeAntiAlias, StringAlignmentCenter, TextRenderingHintAntiAliasGridFit,
};
use windows::Win32::UI::Controls::{WM_MOUSELEAVE, TOOLTIPS_CLASSW, TTF_SUBCLASS, TTM_ADDTOOLW, TTS_ALWAYSTIP, TTS_NOPREFIX, TTTOOLINFOW};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos, GetWindowRect, IDC_ARROW, IDC_HAND, LWA_ALPHA, LoadCursorW,
    MA_NOACTIVATE, RegisterClassW, SW_SHOWNOACTIVATE, SendMessageW, SetCursor, SetLayeredWindowAttributes,
    SetWindowDisplayAffinity, ShowWindow, ULW_ALPHA, UpdateLayeredWindow, WDA_EXCLUDEFROMCAPTURE, WINDOW_EX_STYLE,
    WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_SETCURSOR, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use snapnab_app::tr;

fn h(x: isize) -> HWND {
    HWND(x as _)
}

fn scale(v: i32, dpi: u32) -> i32 {
    (v as i64 * dpi as i64 / 96) as i32
}

unsafe extern "system" fn border_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

fn register() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        let hi: windows::Win32::Foundation::HINSTANCE = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default().into();
        let border = WNDCLASSW {
            lpfnWndProc: Some(border_proc),
            hInstance: hi,
            hbrBackground: CreateSolidBrush(COLORREF(235 | 60 << 8 | 60 << 16)),
            lpszClassName: w!("SnapnabRecorderBorder"),
            ..Default::default()
        };
        RegisterClassW(&border);
        let pano = WNDCLASSW {
            lpfnWndProc: Some(border_proc),
            hInstance: hi,
            hbrBackground: CreateSolidBrush(COLORREF(232 | 120 << 8 | 30 << 16)),
            lpszClassName: w!("SnapnabPanoBorder"),
            ..Default::default()
        };
        RegisterClassW(&pano);
        let count = WNDCLASSW { lpfnWndProc: Some(count_proc), hInstance: hi, lpszClassName: w!("SnapnabCountdown"), ..Default::default() };
        RegisterClassW(&count);
    });
}

pub fn destroy(hwnd: &mut isize) {
    if *hwnd != 0 {
        unsafe {
            let _ = DestroyWindow(h(*hwnd));
        }
        *hwnd = 0;
    }
}

/// The red frame around the recorded area (outside it, so it is not in the video).
pub fn border(area: Rect) -> isize {
    frame(area, w!("SnapnabRecorderBorder"), 230)
}

/// The orange frame around the panoramic region.
pub fn pano_border(area: Rect) -> isize {
    frame(area, w!("SnapnabPanoBorder"), 235)
}

fn frame(area: Rect, class: PCWSTR, alpha: u8) -> isize {
    register();
    let t = 3;
    let o = Rect::new(area.left - t, area.top - t, area.right + t, area.bottom + t);
    unsafe {
        let Ok(b) = CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE,
                                    class, w!(""), WS_POPUP, o.left, o.top, o.width(), o.height(), None, None,
                                    None, None) else { return 0 };
        let _ = SetLayeredWindowAttributes(b, COLORREF(0), alpha, LWA_ALPHA);
        let outer = CreateRectRgn(0, 0, o.width(), o.height());
        let inner = CreateRectRgn(t, t, o.width() - t, o.height() - t);
        CombineRgn(Some(outer), Some(outer), Some(inner), RGN_DIFF);
        let _ = DeleteObject(inner.into());
        SetWindowRgn(b, Some(outer), false);
        let _ = SetWindowDisplayAffinity(b, WDA_EXCLUDEFROMCAPTURE);
        let _ = ShowWindow(b, SW_SHOWNOACTIVATE);
        b.0 as isize
    }
}

// ---------------------------------------------------------------- countdown
/// "Skip" was clicked (read by the HUD on its next frame).
pub static COUNT_SKIP: AtomicBool = AtomicBool::new(false);
static COUNT_HOT: AtomicBool = AtomicBool::new(false);
static COUNT_LEFT: AtomicI32 = AtomicI32::new(0);

fn count_size(dpi: u32) -> i32 {
    scale(150, dpi)
}

fn skip_rect(dpi: u32) -> RECT {
    let (s, w, hh) = (count_size(dpi), scale(104, dpi), scale(32, dpi));
    RECT { left: (s - w) / 2, top: s + scale(14, dpi), right: (s + w) / 2, bottom: s + scale(14, dpi) + hh }
}

/// A dark translucent circle with the number and, below it, the "Skip" pill, centered on the area.
pub fn countdown(area: Rect, secs: i32) -> isize {
    register();
    COUNT_SKIP.store(false, Relaxed);
    COUNT_HOT.store(false, Relaxed);
    COUNT_LEFT.store(secs, Relaxed);
    let (cx, cy) = ((area.left + area.right) / 2, (area.top + area.bottom) / 2);
    unsafe {
        // created small, then sized for the DPI of where it is
        let Ok(c) = CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_NOACTIVATE, w!("SnapnabCountdown"),
                                    w!(""), WS_POPUP, cx, cy, 1, 1, None, None, None, None) else { return 0 };
        let dpi = GetDpiForWindow(c);
        let s = count_size(dpi);
        let (ww, hh) = (s, s + scale(14 + 32 + 2, dpi));
        use windows::Win32::UI::WindowsAndMessaging::{HWND_TOPMOST, SWP_NOACTIVATE, SetWindowPos};
        let _ = SetWindowPos(c, Some(HWND_TOPMOST), cx - ww / 2, cy - s / 2, ww, hh, SWP_NOACTIVATE);
        let _ = SetWindowDisplayAffinity(c, WDA_EXCLUDEFROMCAPTURE);
        render_count(c.0 as isize);
        let _ = ShowWindow(c, SW_SHOWNOACTIVATE);
        c.0 as isize
    }
}

pub fn set_count(hwnd: isize, left: i32) {
    if COUNT_LEFT.swap(left, Relaxed) != left {
        render_count(hwnd);
    }
}

fn render_count(hwnd: isize) {
    if hwnd == 0 {
        return;
    }
    let mut wr = RECT::default();
    unsafe {
        let _ = GetWindowRect(h(hwnd), &mut wr);
    }
    let (ww, hh) = (wr.right - wr.left, wr.bottom - wr.top);
    let dpi = unsafe { GetDpiForWindow(h(hwnd)) };
    let k = |v: f32| v * dpi as f32 / 96.0;
    let s = count_size(dpi) as f32;
    let hot = COUNT_HOT.load(Relaxed);
    let mut img = Image::sized(ww, hh, 0);
    {
        let bmp = gdip::Bitmap::wrap(&mut img);
        let g = Graphics::new(&bmp);
        g.smoothing(SmoothingModeAntiAlias);
        g.text_hint(TextRenderingHintAntiAliasGridFit);
        g.pixel_offset(PixelOffsetModeHalf);
        g.fill_ellipse(&Brush::solid(0xC818181A), 0.5, 0.5, s - 1.0, s - 1.0);
        g.draw_ellipse(&Pen::new(0x5AFFFFFF, k(2.0)), k(2.0), k(2.0), s - k(4.0), s - k(4.0));
        let center = gdip::StringFormat::new();
        center.align(StringAlignmentCenter);
        center.line_align(StringAlignmentCenter);
        let white = Brush::solid(0xFFFFFFFF);
        let n: Vec<u16> = COUNT_LEFT.load(Relaxed).to_string().encode_utf16().collect();
        g.draw_string(&n, &Font::new("Segoe UI", k(68.0), FontStyleBold), gdip::rect(0.0, 0.0, s, s + k(4.0)), &center, &white);
        // the "Skip" pill
        let kr = skip_rect(dpi);
        let r = gdip::rect(kr.left as f32, kr.top as f32, (kr.right - kr.left) as f32, (kr.bottom - kr.top) as f32);
        let pill = Path::new();
        let d = r.Height;
        pill.arc(r.X, r.Y, d, d, 90.0, 180.0);
        pill.arc(r.X + r.Width - d, r.Y, d, d, 270.0, 180.0);
        pill.close_figure();
        g.fill_path(&Brush::solid(if hot { 0xEB3C3C42 } else { 0xD718181A }), &pill);
        g.draw_path(&Pen::new(if hot { 0x96FFFFFF } else { 0x5AFFFFFF }, 1.0), &pill);
        let label: Vec<u16> = tr("Pular").encode_utf16().collect();
        let mut tr_ = r;
        tr_.Width -= k(14.0);
        g.draw_string(&label, &Font::new("Segoe UI", k(13.0), FontStyleBold), tr_, &center, &white);
        // the forward chevrons (»)
        let chev = Pen::new(0xFFFFFFFF, (dpi as f32 / 55.0).max(1.6));
        let (cx, cy, a) = (r.X + r.Width - k(26.0), r.Y + r.Height / 2.0, k(4.0));
        for i in 0..2 {
            let x = cx + i as f32 * k(5.0);
            g.draw_lines(&chev, &[gdip::pt(x - a / 2.0, cy - a), gdip::pt(x + a / 2.0, cy), gdip::pt(x - a / 2.0, cy + a)]);
        }
    }
    present(hwnd, &img, (wr.left, wr.top));
}

/// UpdateLayeredWindow with `img` (straight ARGB, premultiplied here) at `pos`.
pub fn present(hwnd: isize, img: &Image, pos: (i32, i32)) {
    let (ww, hh) = (img.wi(), img.hi());
    if ww <= 0 || hh <= 0 {
        return;
    }
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: ww,
                biHeight: -hh,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        if let Ok(bmp) = CreateDIBSection(Some(screen), &bi, DIB_RGB_COLORS, &mut bits, None, 0) {
            let dst = std::slice::from_raw_parts_mut(bits as *mut u32, (ww * hh) as usize);
            for (d, &p) in dst.iter_mut().zip(img.px.iter()) {
                let a = p >> 24;
                let pm = |c: u32| (c * a + 127) / 255;
                *d = (a << 24) | (pm((p >> 16) & 0xFF) << 16) | (pm((p >> 8) & 0xFF) << 8) | pm(p & 0xFF);
            }
            let old = SelectObject(mem, bmp.into());
            let (src, dst_pt, sz) = (POINT { x: 0, y: 0 }, POINT { x: pos.0, y: pos.1 }, SIZE { cx: ww, cy: hh });
            let bf = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
            let _ = UpdateLayeredWindow(h(hwnd), Some(screen), Some(&dst_pt), Some(&sz), Some(mem), Some(&src), COLORREF(0), Some(&bf),
                                        ULW_ALPHA);
            SelectObject(mem, old);
            let _ = DeleteObject(bmp.into());
        }
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
    }
}

fn in_skip(hwnd: HWND, p: POINT) -> bool {
    let k = skip_rect(unsafe { GetDpiForWindow(hwnd) });
    p.x >= k.left && p.x < k.right && p.y >= k.top && p.y < k.bottom
}

fn lparam_pt(l: LPARAM) -> POINT {
    POINT { x: (l.0 & 0xFFFF) as i16 as i32, y: ((l.0 >> 16) & 0xFFFF) as i16 as i32 }
}

unsafe extern "system" fn count_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_SETCURSOR => {
            let mut p = POINT::default();
            unsafe {
                let _ = GetCursorPos(&mut p);
                let _ = ScreenToClient(hwnd, &mut p);
                let c = LoadCursorW(None, if in_skip(hwnd, p) { IDC_HAND } else { IDC_ARROW }).ok();
                SetCursor(c);
            }
            LRESULT(1)
        }
        WM_MOUSEMOVE => {
            let hot = in_skip(hwnd, lparam_pt(lp));
            if COUNT_HOT.swap(hot, Relaxed) != hot {
                render_count(hwnd.0 as isize);
            }
            let mut t = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd,
                                          dwHoverTime: 0 };
            unsafe {
                let _ = TrackMouseEvent(&mut t);
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            if COUNT_HOT.swap(false, Relaxed) {
                render_count(hwnd.0 as isize);
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            if in_skip(hwnd, lparam_pt(lp)) {
                COUNT_SKIP.store(true, Relaxed);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

// ---------------------------------------------------------------- tooltips
/// The names of the HUD's buttons on hover (`rects` in client pixels); not in the video either.
pub fn tooltips(owner: isize, items: &[(RECT, String)]) -> isize {
    unsafe {
        let Ok(tip) = CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW, TOOLTIPS_CLASSW, PCWSTR::null(),
                                      WS_POPUP | windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(TTS_ALWAYSTIP | TTS_NOPREFIX),
                                      0, 0, 0, 0, Some(h(owner)), None, None, None) else { return 0 };
        let _ = SetWindowDisplayAffinity(tip, WDA_EXCLUDEFROMCAPTURE);
        for (i, (r, text)) in items.iter().enumerate() {
            let t = to_wide(text);
            let ti = TTTOOLINFOW {
                cbSize: std::mem::size_of::<TTTOOLINFOW>() as u32,
                uFlags: TTF_SUBCLASS,
                hwnd: h(owner),
                uId: i + 1,
                rect: *r,
                lpszText: windows::core::PWSTR(t.as_ptr() as *mut u16),
                ..Default::default()
            };
            SendMessageW(tip, TTM_ADDTOOLW, None, Some(LPARAM(&ti as *const _ as isize)));
        }
        tip.0 as isize
    }
}

#[allow(dead_code)]
const _: WINDOW_EX_STYLE = WS_EX_LAYERED;
