//! The screen as the capture sees it (Image.cpp FromDC/ToHBITMAP, Util.cpp, SelectOverlay.cpp, CaptureEngine.cpp):
//! copies of screen areas through GDI, the cursor as an image, the monitors, and the top-level windows with their
//! visible frames, titles and programs. Windows are passed around as `isize` (the HWND value), so they can cross
//! threads.

use std::ffi::c_void;

use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleBitmap, CreateCompatibleDC,
    CreateDIBSection, CreateSolidBrush, DIB_RGB_COLORS, DeleteDC, DeleteObject, EnumDisplayMonitors, FillRect, GdiFlush,
    GetDC, GetDIBits, GetMonitorInfoW, GetObjectW, HBITMAP, HDC, HGDIOBJ, HMONITOR, MONITOR_DEFAULTTONEAREST,
    MONITORINFO, MonitorFromPoint, PaintDesktop, ReleaseDC, SRCCOPY, ScreenToClient, SelectObject, SetViewportOrgEx,
};
use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};
use windows::Win32::System::SystemInformation::{OSVERSIONINFOEXW, VER_BUILDNUMBER, VerSetConditionMask, VerifyVersionInfoW};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
                                        QueryFullProcessImageNameW};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CURSOR_SHOWING, CURSORINFO, CWP_SKIPDISABLED, CWP_SKIPINVISIBLE, CWP_SKIPTRANSPARENT, ChildWindowFromPointEx,
    DI_NORMAL, DrawIconEx, EnumWindows, GA_ROOT, GWL_EXSTYLE, GetAncestor, GetClassNameW, GetCursorInfo, GetCursorPos,
    GetIconInfo, GetLayeredWindowAttributes, GetSystemMetrics, GetWindowLongW, GetWindowPlacement, GetWindowRect,
    GetWindowTextW, GetWindowThreadProcessId, HCURSOR, HICON, ICONINFO, IsIconic, IsWindowVisible, LWA_ALPHA,
    SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_SHOWMAXIMIZED,
    WINDOWPLACEMENT, WS_EX_LAYERED, WS_EX_TRANSPARENT,
};
use windows::core::BOOL;

use crate::image::{Image, Rect, argb, ca, cb, cg, cr};

pub fn hwnd(h: isize) -> HWND {
    HWND(h as *mut c_void)
}

pub fn to_rect(r: RECT) -> Rect {
    Rect::new(r.left, r.top, r.right, r.bottom)
}

pub fn to_win(r: Rect) -> RECT {
    RECT { left: r.left, top: r.top, right: r.right, bottom: r.bottom }
}

/// VirtualScreenRect: all the monitors together.
pub fn virtual_screen() -> Rect {
    unsafe {
        let (x, y) = (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN));
        Rect::new(x, y, x + GetSystemMetrics(SM_CXVIRTUALSCREEN), y + GetSystemMetrics(SM_CYVIRTUALSCREEN))
    }
}

/// MonitorRectFromPoint: the monitor nearest the point (its work area: without the taskbar).
pub fn monitor_rect(x: i32, y: i32, work: bool) -> Rect {
    let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe {
        let _ = GetMonitorInfoW(MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST), &mut mi);
    }
    to_rect(if work { mi.rcWork } else { mi.rcMonitor })
}

/// The monitors in reading order (left to right, top to bottom), as /monitor n counts them.
pub fn monitors() -> Vec<Rect> {
    unsafe extern "system" fn add(m: HMONITOR, _: HDC, _: *mut RECT, lp: LPARAM) -> BOOL {
        let v = unsafe { &mut *(lp.0 as *mut Vec<Rect>) };
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if unsafe { GetMonitorInfoW(m, &mut mi) }.as_bool() {
            v.push(to_rect(mi.rcMonitor));
        }
        true.into()
    }
    let mut v: Vec<Rect> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(add), LPARAM(&mut v as *mut _ as isize));
    }
    v.sort_by_key(|r| (r.top, r.left));
    // rows: monitors whose tops are close are side by side
    v.sort_by(|a, b| if (a.top - b.top).abs() < 100 { a.left.cmp(&b.left) } else { a.top.cmp(&b.top) });
    v
}

pub fn cursor_pos() -> (i32, i32) {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    (p.x, p.y)
}

/// A 32-bit top-down DIB section and its pixels.
fn dib(dc: Option<HDC>, w: i32, h: i32) -> Option<(HBITMAP, *mut u32)> {
    let bi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = std::ptr::null_mut();
    let hb = unsafe { CreateDIBSection(dc, &bi, DIB_RGB_COLORS, &mut bits, None, 0) }.ok()?;
    if bits.is_null() {
        unsafe {
            let _ = DeleteObject(hb.into());
        }
        return None;
    }
    Some((hb, bits as *mut u32))
}

/// Image::FromDC: a copy of an area of a device context (the screen when `dc` is None), opaque.
pub fn grab_dc(dc: Option<HDC>, r: Rect) -> Image {
    let (w, h) = (r.width(), r.height());
    if w <= 0 || h <= 0 {
        return Image::default();
    }
    unsafe {
        let screen = GetDC(None);
        let src = dc.unwrap_or(screen);
        let mut img = Image::default();
        if let Some((hb, bits)) = dib(Some(src), w, h) {
            let mem = CreateCompatibleDC(Some(src));
            let old = SelectObject(mem, hb.into());
            let _ = BitBlt(mem, 0, 0, w, h, Some(src), r.left, r.top, SRCCOPY | CAPTUREBLT);
            let _ = GdiFlush();
            img = Image::sized(w, h, 0);
            std::ptr::copy_nonoverlapping(bits, img.px.as_mut_ptr(), img.px.len());
            SelectObject(mem, old);
            let _ = DeleteDC(mem);
            let _ = DeleteObject(hb.into());
            img.make_opaque();
        }
        ReleaseDC(None, screen);
        img
    }
}

/// A copy of a screen area (screen coordinates).
pub fn grab(r: Rect) -> Image {
    grab_dc(None, r)
}

/// Image::ToHBITMAP: a DIB section with the pixels (the caller deletes it).
pub fn to_hbitmap(img: &Image) -> Option<HBITMAP> {
    let (hb, bits) = dib(None, img.wi(), img.hi())?;
    unsafe { std::ptr::copy_nonoverlapping(img.px.as_ptr(), bits, img.px.len()) };
    Some(hb)
}

/// Image::FromHBITMAP: the pixels of a bitmap; opaque when it has no alpha (or an all-zero one).
pub fn from_hbitmap(hb: HBITMAP) -> Image {
    unsafe {
        let mut bm = BITMAP::default();
        if GetObjectW(HGDIOBJ(hb.0), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut c_void)) == 0 {
            return Image::default();
        }
        let mut img = Image::sized(bm.bmWidth, bm.bmHeight.abs(), 0);
        let mut bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: img.wi(),
                biHeight: -img.hi(),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let dc = GetDC(None);
        GetDIBits(dc, hb, 0, img.h, Some(img.px.as_mut_ptr() as *mut c_void), &mut bi, DIB_RGB_COLORS);
        ReleaseDC(None, dc);
        if bm.bmBitsPixel < 32 || img.px.iter().all(|&p| ca(p) == 0) {
            img.make_opaque();
        }
        img
    }
}

fn draw_icon(img: &Image, hc: HCURSOR, x: i32, y: i32, w: i32, h: i32) -> Image {
    let Some(hb) = to_hbitmap(img) else { return Image::default() };
    unsafe {
        let dc = CreateCompatibleDC(None);
        let old = SelectObject(dc, hb.into());
        let _ = DrawIconEx(dc, x, y, HICON(hc.0), w, h, 0, None, DI_NORMAL);
        SelectObject(dc, old);
        let _ = DeleteDC(dc);
        let r = from_hbitmap(hb);
        let _ = DeleteObject(hb.into());
        r
    }
}

/// CursorBitmap: the cursor with transparency, drawn over black and over white (the difference gives the alpha, also
/// for the old monochrome cursors), and its hotspot.
fn cursor_bitmap(hc: HCURSOR) -> Option<(Image, (i32, i32))> {
    let mut ii = ICONINFO::default();
    unsafe { GetIconInfo(HICON(hc.0), &mut ii) }.ok()?;
    let mut bm = BITMAP::default();
    let src = if !ii.hbmColor.is_invalid() { ii.hbmColor } else { ii.hbmMask };
    unsafe {
        GetObjectW(HGDIOBJ(src.0), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut c_void));
    }
    let (w, h) = (bm.bmWidth, if !ii.hbmColor.is_invalid() { bm.bmHeight } else { bm.bmHeight / 2 });
    let hot = (ii.xHotspot as i32, ii.yHotspot as i32);
    unsafe {
        if !ii.hbmMask.is_invalid() {
            let _ = DeleteObject(ii.hbmMask.into());
        }
        if !ii.hbmColor.is_invalid() {
            let _ = DeleteObject(ii.hbmColor.into());
        }
    }
    if w <= 0 || h <= 0 || w > 512 || h > 512 {
        return None;
    }
    let b = draw_icon(&Image::sized(w, h, 0xFF00_0000), hc, 0, 0, w, h);
    let wt = draw_icon(&Image::sized(w, h, 0xFFFF_FFFF), hc, 0, 0, w, h);
    if b.wi() != w || wt.wi() != w {
        return None;
    }
    let mut out = Image::sized(w, h, 0);
    for i in 0..out.px.len() {
        let (pb, pw) = (b.px[i], wt.px[i]);
        let a = (255 - ((cr(pw) - cr(pb)) + (cg(pw) - cg(pb)) + (cb(pw) - cb(pb))) / 3).clamp(0, 255);
        if a == 0 {
            continue;
        }
        out.px[i] = argb(a, (cr(pb) * 255 / a).min(255), (cg(pb) * 255 / a).min(255), (cb(pb) * 255 / a).min(255));
    }
    Some((out, hot))
}

fn cursor_info() -> Option<CURSORINFO> {
    let mut ci = CURSORINFO { cbSize: std::mem::size_of::<CURSORINFO>() as u32, ..Default::default() };
    unsafe { GetCursorInfo(&mut ci) }.ok()?;
    (ci.flags.0 & CURSOR_SHOWING.0 != 0).then_some(ci)
}

/// The cursor on the screen as an image with transparency, and the screen position of its top-left corner.
pub fn cursor_image() -> Option<(Image, (i32, i32))> {
    let ci = cursor_info()?;
    let (img, hot) = cursor_bitmap(ci.hCursor)?;
    Some((img, (ci.ptScreenPos.x - hot.0, ci.ptScreenPos.y - hot.1)))
}

/// Draws the cursor into a copy of the screen area that starts at `origin` (FreezeScreen with the cursor).
pub fn draw_cursor(img: &mut Image, origin: (i32, i32)) {
    let Some(ci) = cursor_info() else { return };
    let mut ii = ICONINFO::default();
    if unsafe { GetIconInfo(HICON(ci.hCursor.0), &mut ii) }.is_err() {
        return;
    }
    unsafe {
        if !ii.hbmMask.is_invalid() {
            let _ = DeleteObject(ii.hbmMask.into());
        }
        if !ii.hbmColor.is_invalid() {
            let _ = DeleteObject(ii.hbmColor.into());
        }
    }
    let x = ci.ptScreenPos.x - origin.0 - ii.xHotspot as i32;
    let y = ci.ptScreenPos.y - origin.1 - ii.yHotspot as i32;
    let mut out = draw_icon(img, ci.hCursor, x, y, 0, 0);
    if !out.is_empty() {
        out.make_opaque();
        *img = out;
    }
}

/// WindowFrameRect: the visible frame (DWMWA_EXTENDED_FRAME_BOUNDS, without the invisible shadow).
pub fn window_frame_rect(h: isize) -> Rect {
    let mut r = RECT::default();
    let ok = unsafe {
        DwmGetWindowAttribute(hwnd(h), DWMWA_EXTENDED_FRAME_BOUNDS, &mut r as *mut _ as *mut c_void,
                              std::mem::size_of::<RECT>() as u32)
    }
    .is_ok();
    if !ok || to_rect(r).is_empty() {
        return window_rect(h);
    }
    to_rect(r)
}

pub fn window_rect(h: isize) -> Rect {
    let mut r = RECT::default();
    unsafe {
        let _ = GetWindowRect(hwnd(h), &mut r);
    }
    to_rect(r)
}

/// WindowTitle: the title of the top-level window the window belongs to.
pub fn window_title(h: isize) -> String {
    let mut b = [0u16; 512];
    let n = unsafe { GetWindowTextW(GetAncestor(hwnd(h), GA_ROOT), &mut b) };
    String::from_utf16_lossy(&b[..n.max(0) as usize])
}

pub fn class_name(h: isize) -> String {
    let mut b = [0u16; 128];
    let n = unsafe { GetClassNameW(hwnd(h), &mut b) };
    String::from_utf16_lossy(&b[..n.max(0) as usize])
}

pub fn window_pid(h: isize) -> u32 {
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd(h), Some(&mut pid)) };
    pid
}

/// WindowProcessName: the file name of the window's program ("notepad.exe").
pub fn window_process_name(h: isize) -> String {
    let Ok(p) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, window_pid(h)) }) else {
        return String::new();
    };
    let mut b = [0u16; 1024];
    let mut n = b.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(p, PROCESS_NAME_WIN32, windows::core::PWSTR(b.as_mut_ptr()), &mut n) }.is_ok();
    unsafe {
        let _ = CloseHandle(p);
    }
    if !ok {
        return String::new();
    }
    let full = String::from_utf16_lossy(&b[..n as usize]);
    full.rsplit(['\\', '/']).next().unwrap_or("").to_owned()
}

pub fn is_cloaked(h: isize) -> bool {
    let mut c = 0u32;
    unsafe { DwmGetWindowAttribute(hwnd(h), DWMWA_CLOAKED, &mut c as *mut _ as *mut c_void, 4) }.is_ok() && c != 0
}

/// A top-level window with its visible frame (screen coordinates).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TopWindow {
    pub hwnd: isize,
    pub rect: Rect,
}

/// The visible top-level windows of other programs, from the top to the bottom (the selection's EnumProc):
/// minimized, cloaked (other virtual desktops, suspended apps) and click-through overlays left out.
pub fn top_windows() -> Vec<TopWindow> {
    unsafe extern "system" fn add(h: HWND, lp: LPARAM) -> BOOL {
        let v = unsafe { &mut *(lp.0 as *mut Vec<TopWindow>) };
        let k = h.0 as isize;
        unsafe {
            if !IsWindowVisible(h).as_bool() || IsIconic(h).as_bool() || window_pid(k) == std::process::id() || is_cloaked(k) {
                return true.into();
            }
            let ex = GetWindowLongW(h, GWL_EXSTYLE) as u32;
            if ex & WS_EX_TRANSPARENT.0 != 0 && ex & WS_EX_LAYERED.0 != 0 {
                return true.into();
            }
        }
        let r = window_frame_rect(k);
        if r.width() >= 8 && r.height() >= 8 {
            v.push(TopWindow { hwnd: k, rect: r });
        }
        true.into()
    }
    let mut v: Vec<TopWindow> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(add), LPARAM(&mut v as *mut _ as isize));
    }
    v
}

/// FindWindowByTitle (the command line's /window): the first visible window of another program whose title contains
/// `text` (case-insensitive); owned windows count only with a title bar (dialogs).
pub fn find_window_by_title(text: &str) -> Option<isize> {
    struct Ctx {
        t: String,
        found: isize,
    }
    unsafe extern "system" fn each(h: HWND, lp: LPARAM) -> BOOL {
        let c = unsafe { &mut *(lp.0 as *mut Ctx) };
        let k = h.0 as isize;
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{GW_OWNER, GWL_STYLE, GetWindow, WS_CAPTION};
            let owned = GetWindow(h, GW_OWNER).map(|o| !o.0.is_null()).unwrap_or(false);
            if !IsWindowVisible(h).as_bool() || IsIconic(h).as_bool()
               || (owned && GetWindowLongW(h, GWL_STYLE) as u32 & WS_CAPTION.0 == 0)
            {
                return true.into();
            }
        }
        if window_pid(k) == std::process::id() {
            return true.into();
        }
        let t = window_title(k).to_lowercase();
        if !t.is_empty() && t.contains(&c.t) {
            c.found = k;
            return false.into();
        }
        true.into()
    }
    let mut c = Ctx { t: text.to_lowercase(), found: 0 };
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut c as *mut _ as isize));
    }
    (c.found != 0).then_some(c.found)
}

/// The innermost visible control at a screen point inside a top-level window (ChildWindowFromPointEx, level by
/// level), or None when the point is on the window itself.
pub fn child_at(top: isize, x: i32, y: i32) -> Option<isize> {
    let mut cur = hwnd(top);
    for _ in 0..16 {
        let mut cp = POINT { x, y };
        unsafe {
            let _ = ScreenToClient(cur, &mut cp);
        }
        let c = unsafe { ChildWindowFromPointEx(cur, cp, CWP_SKIPINVISIBLE | CWP_SKIPTRANSPARENT | CWP_SKIPDISABLED) };
        if c.0.is_null() || c == cur {
            break;
        }
        cur = c;
    }
    (cur != hwnd(top)).then_some(cur.0 as isize)
}

/// Windows 11 (build 22000 or later).
pub fn is_windows_11() -> bool {
    let mut ov = OSVERSIONINFOEXW {
        dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOEXW>() as u32,
        dwBuildNumber: 22000,
        ..Default::default()
    };
    unsafe {
        let mask = VerSetConditionMask(0, VER_BUILDNUMBER, 3 /* VER_GREATER_EQUAL */);
        VerifyVersionInfoW(&mut ov, VER_BUILDNUMBER, mask).is_ok()
    }
}

/// RoundWindowCorners (C06): the Windows 11 rounded corners of a captured window made transparent; not for
/// maximized windows nor for windows that ask for square corners (DWMWA_WINDOW_CORNER_PREFERENCE).
pub fn round_window_corners(img: &mut Image, h: isize) {
    if img.is_empty() || h == 0 || !is_windows_11() {
        return;
    }
    let root = unsafe { GetAncestor(hwnd(h), GA_ROOT) };
    let mut wp = WINDOWPLACEMENT { length: std::mem::size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
    if unsafe { GetWindowPlacement(root, &mut wp) }.is_err() || wp.showCmd == SW_SHOWMAXIMIZED.0 as u32 {
        return;
    }
    let mut pref = 0u32; // DWMWCP_DEFAULT / ROUND = 8 px, ROUNDSMALL = 4 px, DONOTROUND
    unsafe {
        let _ = DwmGetWindowAttribute(root, windows::Win32::Graphics::Dwm::DWMWINDOWATTRIBUTE(33),
                                      &mut pref as *mut _ as *mut c_void, 4);
    }
    if pref == 1 {
        return;
    }
    let dpi = unsafe { GetDpiForWindow(hwnd(h)) } as i32;
    let rad = 2.max((if pref == 3 { 4 } else { 8 }) * dpi / 96);
    for y in 0..rad.min(img.hi()) {
        for x in 0..rad.min(img.wi()) {
            let (dx, dy) = (rad as f32 - x as f32 - 0.5, rad as f32 - y as f32 - 0.5);
            let d = (dx * dx + dy * dy).sqrt();
            let a = ((rad as f32 - d + 0.5) * 255.0).clamp(0.0, 255.0) as i32;
            if a >= 255 {
                continue;
            }
            let (w, hh) = (img.wi(), img.hi());
            for (px, py) in [(x, y), (w - 1 - x, y), (x, hh - 1 - y), (w - 1 - x, hh - 1 - y)] {
                let p = img.at_mut(px, py);
                *p = (*p & 0x00FF_FFFF) | (((ca(*p) * a / 255) as u32) << 24);
            }
        }
    }
}

/// The image of one window, even the parts other windows cover (PrintWindow with the DWM content), cropped to its
/// visible frame; None when the program does not draw that way (all black).
pub fn print_window(h: isize, frame: Rect) -> Option<Image> {
    let wr = window_rect(h);
    let (w, hh) = (wr.width(), wr.height());
    if w <= 0 || hh <= 0 || (w as i64) * (hh as i64) >= 80_000_000 {
        return None;
    }
    let mut img = unsafe {
        let sdc = GetDC(None);
        let (hb, bits) = dib(Some(sdc), w, hh)?;
        let mem = CreateCompatibleDC(Some(sdc));
        let old = SelectObject(mem, hb.into());
        let ok = PrintWindow(hwnd(h), mem, PRINT_WINDOW_FLAGS(2) /* PW_RENDERFULLCONTENT */).as_bool();
        let _ = GdiFlush();
        let mut img = Image::default();
        if ok {
            img = Image::sized(w, hh, 0);
            std::ptr::copy_nonoverlapping(bits, img.px.as_mut_ptr(), img.px.len());
        }
        SelectObject(mem, old);
        let _ = DeleteObject(hb.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, sdc);
        img
    };
    if img.is_empty() {
        return None;
    }
    img.make_opaque();
    let crop = frame.offset(-wr.left, -wr.top);
    if !crop.is_empty() {
        img = img.crop(crop);
    }
    let dark = img.px.iter().filter(|&&p| p & 0x00FF_FFFF == 0).count();
    (dark <= img.px.len() * 97 / 100).then_some(img)
}

/// The desktop wallpaper of a screen area as Windows paints it (PaintDesktop), over the desktop color.
pub fn desktop_background(area: Rect, desktop_color: u32) -> Image {
    let (w, h) = (area.width(), area.height());
    unsafe {
        let sdc = GetDC(None);
        let mem = CreateCompatibleDC(Some(sdc));
        let bmp = CreateCompatibleBitmap(sdc, w, h);
        let old = SelectObject(mem, bmp.into());
        let brush = CreateSolidBrush(windows::Win32::Foundation::COLORREF(desktop_color));
        let _ = FillRect(mem, &RECT { left: 0, top: 0, right: w, bottom: h }, brush);
        let _ = DeleteObject(brush.into());
        let _ = SetViewportOrgEx(mem, -area.left, -area.top, None);
        let _ = PaintDesktop(mem);
        let _ = SetViewportOrgEx(mem, 0, 0, None);
        SelectObject(mem, old);
        let mut img = from_hbitmap(bmp);
        img.make_opaque();
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, sdc);
        img
    }
}

/// The layered-window alpha of a window (255 when it has none).
pub fn layered_alpha(h: isize) -> u8 {
    let (mut alpha, mut flags) = (255u8, Default::default());
    let ok = unsafe { GetLayeredWindowAttributes(hwnd(h), None, Some(&mut alpha), Some(&mut flags)) }.is_ok();
    if ok && flags & LWA_ALPHA == LWA_ALPHA { alpha } else { 255 }
}

/// The windows that become objects of a full-screen capture (CaptureWindowsAsObjects, F31), from the top, and the
/// taskbars (they stay in the background). At most 40 windows; tiny, click-through and almost invisible ones are left
/// out, as are the desktop and this program.
pub fn windows_for_objects(area: Rect) -> (Vec<TopWindow>, Vec<isize>) {
    struct Ctx {
        area: Rect,
        wins: Vec<TopWindow>,
        bars: Vec<isize>,
    }
    unsafe extern "system" fn add(h: HWND, lp: LPARAM) -> BOOL {
        let c = unsafe { &mut *(lp.0 as *mut Ctx) };
        let k = h.0 as isize;
        unsafe {
            if !IsWindowVisible(h).as_bool() || IsIconic(h).as_bool() || is_cloaked(k) || window_pid(k) == std::process::id() {
                return true.into();
            }
        }
        let cls = class_name(k);
        if cls == "Shell_TrayWnd" || cls == "Shell_SecondaryTrayWnd" {
            c.bars.push(k);
            return true.into();
        }
        if cls == "Progman" || cls == "WorkerW" {
            return true.into();
        }
        let ex = unsafe { GetWindowLongW(h, GWL_EXSTYLE) } as u32;
        if ex & WS_EX_TRANSPARENT.0 != 0 || (ex & WS_EX_LAYERED.0 != 0 && layered_alpha(k) < 8) {
            return true.into();
        }
        let fr = window_frame_rect(k);
        if fr.width() < 16 || fr.height() < 16 || fr.intersect(&c.area).is_empty() {
            return true.into();
        }
        if c.wins.len() < 40 {
            c.wins.push(TopWindow { hwnd: k, rect: fr });
        }
        true.into()
    }
    let mut c = Ctx { area, wins: Vec::new(), bars: Vec::new() };
    unsafe {
        let _ = EnumWindows(Some(add), LPARAM(&mut c as *mut _ as isize));
    }
    (c.wins, c.bars)
}

/// The open popup menus (class #32768) and the menu bar of the active window, if it has one (CaptureMenus).
pub fn open_menus() -> (Vec<Rect>, Option<Rect>) {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetMenuBarInfo, MENUBARINFO, OBJID_MENU};
    unsafe extern "system" fn add(h: HWND, lp: LPARAM) -> BOOL {
        let v = unsafe { &mut *(lp.0 as *mut Vec<Rect>) };
        let k = h.0 as isize;
        if unsafe { IsWindowVisible(h) }.as_bool() && class_name(k) == "#32768" {
            v.push(window_rect(k));
        }
        true.into()
    }
    let mut v: Vec<Rect> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(add), LPARAM(&mut v as *mut _ as isize));
    }
    let fg = unsafe { GetForegroundWindow() };
    let mut mbi = MENUBARINFO { cbSize: std::mem::size_of::<MENUBARINFO>() as u32, ..Default::default() };
    let bar = (!fg.0.is_null() && unsafe { GetMenuBarInfo(fg, OBJID_MENU, 0, &mut mbi) }.is_ok())
        .then(|| to_rect(mbi.rcBar))
        .filter(|r| !r.is_empty());
    (v, bar)
}

/// GetSysColor(COLOR_DESKTOP) as a COLORREF.
pub fn desktop_color() -> u32 {
    unsafe { windows::Win32::Graphics::Gdi::GetSysColor(windows::Win32::Graphics::Gdi::COLOR_BACKGROUND) }
}
