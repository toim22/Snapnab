//! Screen tools that live on the desktop itself:
//! - the clean desktop during a capture (CleanDesktop.cpp, C16): the desktop icons hidden (HideDesktopIcons), the
//!   notification banners off while recording (DndWhileRecording) and the screen kept awake (KeepAwake). What was
//!   changed is written to CleanDesktopState first, so a crash is undone at the next start;
//! - screen focus for presentations (ScreenFocus.cpp): the screen darkened (FocusDarkness) except a hole around the
//!   pointer (FocusSize, FocusShape 0 circle, 1 rounded rectangle);
//! - the screen color picker (ColorPicker.cpp, C10): the frozen screen with a magnifier and the color under the
//!   cursor; a click copies it in the default format and opens the menu of the other formats and the recent colors.

use std::cell::{Cell, RefCell};
use std::ffi::OsStr;
use std::sync::Once;

use snapcore::gdip::{self, Brush, Font, Graphics, StringFormat};
use snapcore::image::{Image, Rect};
use snapcore::wide::to_wide;
use snapcore::{color, log, reg, screen};
use snapnab_app::tr;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, COLORONCOLOR, CombineRgn, CreateCompatibleBitmap, CreateCompatibleDC, CreateEllipticRgn, CreateRectRgn,
    CreateRoundRectRgn, DeleteDC, DeleteObject, EndPaint, GetDC, HBITMAP, HDC, HGDIOBJ, InvalidateRect, PAINTSTRUCT, RGN_DIFF,
    ReleaseDC, SRCCOPY, SelectObject, SetStretchBltMode, SetWindowRgn, StretchBlt,
};
use windows::Win32::Graphics::GdiPlus::{FontStyleBold, FontStyleRegular, SmoothingModeAntiAlias, StringAlignmentCenter,
    StringAlignmentNear, TextRenderingHintAntiAliasGridFit};
use windows::Win32::System::Power::{ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED, SetThreadExecutionState};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_DWORD, REG_OPTION_NON_VOLATILE, RRF_RT_REG_DWORD, RegCloseKey, RegCreateKeyExW,
    RegDeleteValueW, RegGetValueW, RegSetValueExW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, MOD_NOREPEAT, RegisterHotKey, SetFocus, UnregisterHotKey, VK_DOWN, VK_ESCAPE, VK_LEFT, VK_RETURN, VK_RIGHT,
    VK_SHIFT, VK_SPACE, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, FindWindowExW, FindWindowW, GetCursorPos, HWND_BROADCAST, IDC_CROSS,
    IsWindowVisible, KillTimer, LWA_ALPHA, LoadCursorW, RegisterClassW, SMTO_ABORTIFHUNG, SW_HIDE, SW_SHOW, SW_SHOWNA,
    SW_SHOWNOACTIVATE, SendMessageTimeoutW, SetCursor, SetCursorPos, SetForegroundWindow, SetLayeredWindowAttributes, SetTimer,
    ShowWindow, WM_CLOSE, WM_DESTROY, WM_DISPLAYCHANGE, WM_ERASEBKGND, WM_HOTKEY, WM_KEYDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    WM_PAINT, WM_RBUTTONUP, WM_SETCURSOR, WM_SETTINGCHANGE, WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use crate::pins::{UiEv, send};

fn h(x: isize) -> HWND {
    HWND(x as _)
}

fn dword(name: &str, def: u32) -> u32 {
    reg::get_dword(OsStr::new(name), def, None)
}

// ---------------------------------------------------------------- the clean desktop
const NOTIF_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Notifications\\Settings");
const TOASTS: PCWSTR = w!("NOC_GLOBAL_SETTING_TOASTS_ENABLED");

#[derive(Default, Clone, Copy)]
struct Clean {
    active: bool,
    icons_hidden: bool,
    toasts_changed: bool,
    awake: bool,
}

thread_local! {
    static CLEAN: Cell<Clean> = Cell::new(Clean::default());
}

/// -1: the value does not exist (Windows' default: banners on).
fn read_toasts() -> i32 {
    let mut v = 0u32;
    let mut cb = 4u32;
    let ok = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, NOTIF_KEY, TOASTS, RRF_RT_REG_DWORD, None, Some(&mut v as *mut u32 as _), Some(&mut cb))
    };
    if ok.is_ok() { v as i32 } else { -1 }
}

fn write_toasts(v: i32) {
    unsafe {
        let mut k = HKEY::default();
        if RegCreateKeyExW(HKEY_CURRENT_USER, NOTIF_KEY, None, None, REG_OPTION_NON_VOLATILE, KEY_SET_VALUE, None, &mut k, None).is_err() {
            return;
        }
        if v < 0 {
            let _ = RegDeleteValueW(k, TOASTS);
        } else {
            let d = (v as u32).to_le_bytes();
            let _ = RegSetValueExW(k, TOASTS, None, REG_DWORD, Some(&d));
        }
        let _ = RegCloseKey(k);
        let mut r = 0usize;
        let _ = SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, WPARAM(0), LPARAM(w!("Notifications").as_ptr() as isize),
                                    SMTO_ABORTIFHUNG, 200, Some(&mut r));
    }
}

fn save_state(icons: bool, before: i32, changed: bool) {
    let s = if !icons && !changed {
        String::new()
    } else {
        format!("icons={};toasts={before};changed={}", icons as i32, changed as i32)
    };
    reg::set_string(OsStr::new("CleanDesktopState"), OsStr::new(&s), None);
}

/// The list view of the desktop icons: under Progman, or a WorkerW (wallpaper slideshow, Win+Tab).
fn desktop_icons_view() -> Option<HWND> {
    unsafe {
        let find = |parent: HWND| -> Option<HWND> {
            let def = FindWindowExW(Some(parent), None, w!("SHELLDLL_DefView"), PCWSTR::null()).ok().filter(|x| !x.0.is_null())?;
            FindWindowExW(Some(def), None, w!("SysListView32"), PCWSTR::null()).ok().filter(|x| !x.0.is_null())
        };
        if let Some(p) = FindWindowW(w!("Progman"), PCWSTR::null()).ok().filter(|x| !x.0.is_null())
            && let Some(v) = find(p)
        {
            return Some(v);
        }
        let mut wk: Option<HWND> = None;
        loop {
            let n = FindWindowExW(None, wk, w!("WorkerW"), PCWSTR::null()).ok().filter(|x| !x.0.is_null())?;
            if let Some(v) = find(n) {
                return Some(v);
            }
            wk = Some(n);
        }
    }
}

fn state_toasts_before(st: &str) -> i32 {
    st.split(';').find_map(|p| p.strip_prefix("toasts=")).and_then(|v| v.parse().ok()).unwrap_or(-1)
}

/// Before the screen is frozen (or a recording starts).
pub fn clean_begin(recording: bool) {
    let mut c = CLEAN.with(|c| c.get());
    if c.active {
        return;
    }
    c.active = true;
    let before = read_toasts();
    if dword("HideDesktopIcons", 0) != 0
        && let Some(v) = desktop_icons_view()
        && unsafe { IsWindowVisible(v) }.as_bool()
    {
        c.icons_hidden = true;
    }
    c.toasts_changed = recording && dword("DndWhileRecording", 0) != 0 && before != 0;
    save_state(c.icons_hidden, before, c.toasts_changed); // first the record, then the change
    if c.icons_hidden && let Some(v) = desktop_icons_view() {
        unsafe {
            let _ = ShowWindow(v, SW_HIDE);
        }
        log::write("Clean desktop: icons hidden");
    }
    if c.toasts_changed {
        write_toasts(0);
        log::write("Clean desktop: notification banners off");
    }
    if dword("KeepAwake", 1) != 0 {
        unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED) };
        c.awake = true;
    }
    CLEAN.with(|x| x.set(c));
}

pub fn clean_end() {
    let mut c = CLEAN.with(|c| c.get());
    if !c.active {
        return;
    }
    c.active = false;
    let st = reg::get_string(OsStr::new("CleanDesktopState"), None).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    if c.icons_hidden {
        if let Some(v) = desktop_icons_view() {
            unsafe {
                let _ = ShowWindow(v, SW_SHOWNA);
            }
        }
        c.icons_hidden = false;
    }
    if c.toasts_changed {
        write_toasts(state_toasts_before(&st));
        c.toasts_changed = false;
    }
    if c.awake {
        unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
        c.awake = false;
    }
    reg::set_string(OsStr::new("CleanDesktopState"), OsStr::new(""), None);
    CLEAN.with(|x| x.set(c));
}

/// At the start: icons or banners left hidden by a crash come back.
pub fn restore_after_crash() {
    let st = reg::get_string(OsStr::new("CleanDesktopState"), None).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    if st.is_empty() {
        return;
    }
    log::write(&format!("Clean desktop: restoring after an unexpected exit ({st})"));
    if st.contains("icons=1")
        && let Some(v) = desktop_icons_view()
    {
        unsafe {
            let _ = ShowWindow(v, SW_SHOWNA);
        }
    }
    if st.contains("changed=1") {
        write_toasts(state_toasts_before(&st));
    }
    reg::set_string(OsStr::new("CleanDesktopState"), OsStr::new(""), None);
}

// ---------------------------------------------------------------- screen focus
thread_local! {
    static FOCUS: Cell<(isize, POINT, Rect)> = Cell::new((0, POINT { x: -1, y: -1 }, Rect::default()));
}

fn focus_update() {
    let (wnd, last, vs) = FOCUS.with(|f| f.get());
    if wnd == 0 {
        return;
    }
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    if p.x == last.x && p.y == last.y {
        return;
    }
    FOCUS.with(|f| f.set((wnd, p, vs)));
    let r = dword("FocusSize", 160).clamp(40, 1200) as i32;
    let (x, y) = (p.x - vs.left, p.y - vs.top);
    unsafe {
        let all = CreateRectRgn(0, 0, vs.width(), vs.height());
        let hole = if dword("FocusShape", 0) == 1 {
            CreateRoundRectRgn(x - r * 3 / 2, y - r, x + r * 3 / 2, y + r, 24, 24)
        } else {
            CreateEllipticRgn(x - r, y - r, x + r, y + r)
        };
        CombineRgn(Some(all), Some(all), Some(hole), RGN_DIFF);
        let _ = DeleteObject(hole.into());
        SetWindowRgn(h(wnd), Some(all), true); // the window owns the region from here on
    }
}

unsafe extern "system" fn focus_proc(hwnd: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match m {
        WM_TIMER => {
            focus_update();
            LRESULT(0)
        }
        WM_DISPLAYCHANGE => {
            focus_stop();
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, m, wp, lp) },
    }
}

pub fn focus_active() -> bool {
    FOCUS.with(|f| f.get().0 != 0)
}

pub fn focus_stop() {
    let (wnd, _, _) = FOCUS.with(|f| f.get());
    if wnd == 0 {
        return;
    }
    FOCUS.with(|f| f.set((0, POINT { x: -1, y: -1 }, Rect::default())));
    unsafe {
        let _ = DestroyWindow(h(wnd));
    }
    log::write("Screen focus: off");
}

pub fn focus_toggle() {
    if focus_active() {
        focus_stop();
        return;
    }
    static REG: Once = Once::new();
    REG.call_once(|| unsafe {
        let wc = WNDCLASSW {
            lpfnWndProc: Some(focus_proc),
            hInstance: windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default().into(),
            hbrBackground: windows::Win32::Graphics::Gdi::HBRUSH(
                windows::Win32::Graphics::Gdi::GetStockObject(windows::Win32::Graphics::Gdi::BLACK_BRUSH).0),
            lpszClassName: w!("SnapnabScreenFocus"),
            ..Default::default()
        };
        RegisterClassW(&wc);
    });
    let vs = screen::virtual_screen();
    let Ok(wnd) = (unsafe {
        CreateWindowExW(WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, w!("SnapnabScreenFocus"),
                        w!(""), WS_POPUP, vs.left, vs.top, vs.width(), vs.height(), None, None, None, None)
    }) else {
        return;
    };
    let dark = dword("FocusDarkness", 60).clamp(10, 95);
    unsafe {
        let _ = SetLayeredWindowAttributes(wnd, Default::default(), (dark * 255 / 100) as u8, LWA_ALPHA);
    }
    FOCUS.with(|f| f.set((wnd.0 as isize, POINT { x: -1, y: -1 }, vs)));
    focus_update();
    unsafe {
        let _ = ShowWindow(wnd, SW_SHOWNOACTIVATE);
        SetTimer(Some(wnd), 1, 15, None); // follows the pointer
    }
    log::write("Screen focus: on");
}

// ---------------------------------------------------------------- the screen color picker
struct Picker {
    frozen: Image,
    vs: Rect,
    dc: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
    cur: POINT,
    finished: bool,
}

thread_local! {
    static PICKER: RefCell<Option<(isize, Picker)>> = const { RefCell::new(None) };
}

const PICK_HOTKEY: i32 = 3;

fn colorref_at(p: &Picker, c: POINT) -> u32 {
    let x = c.x.clamp(0, p.frozen.wi() - 1);
    let y = c.y.clamp(0, p.frozen.hi() - 1);
    let v = p.frozen.at(x, y);
    ((v >> 16) & 0xFF) | (v & 0xFF00) | ((v & 0xFF) << 16)
}

fn sc(v: i32, dpi: u32) -> i32 {
    (v as i64 * dpi as i64 / 96) as i32
}

/// A small image drawn with GDI+: the values under the magnifier, or the hint at the top.
fn text_panel(w: i32, hh: i32, dpi: u32, c: Option<u32>, lines: &[String]) -> Image {
    let mut img = Image::sized(w, hh, 0xFF1E_1E20);
    {
        let bmp = gdip::Bitmap::wrap(&mut img);
        let g = Graphics::new(&bmp);
        g.smoothing(SmoothingModeAntiAlias);
        g.text_hint(TextRenderingHintAntiAliasGridFit);
        let mut x = sc(8, dpi) as f32;
        let f = StringFormat::new();
        f.line_align(StringAlignmentCenter);
        if let Some(c) = c {
            let s = sc(26, dpi) as f32;
            g.fill_rect(&Brush::solid(0xFFFF_FFFF), gdip::rect(x - 1.0, x - 1.0, s + 2.0, s + 2.0));
            g.fill_rect(&Brush::solid(snapcore::image::from_colorref(c)), gdip::rect(x, x, s, s));
            x += s + sc(8, dpi) as f32;
            f.align(StringAlignmentNear);
        } else {
            f.align(StringAlignmentCenter);
            x = 0.0;
        }
        let lh = sc(18, dpi) as f32;
        for (i, l) in lines.iter().enumerate() {
            let t: Vec<u16> = l.encode_utf16().collect();
            let font = Font::new("Segoe UI", sc(if i == 0 && c.is_some() { 13 } else { 12 }, dpi) as f32,
                                 if i == 0 && c.is_some() { FontStyleBold } else { FontStyleRegular });
            let (lx, ly, lw) = if c.is_some() && i == 2 {
                (sc(8, dpi) as f32, sc(4, dpi) as f32 + lh * i as f32, w as f32)
            } else if c.is_some() {
                (x, sc(4, dpi) as f32 + lh * i as f32, w as f32 - x)
            } else {
                (0.0, 0.0, w as f32)
            };
            let height = if c.is_some() { lh } else { hh as f32 };
            g.draw_string(&t, &font, gdip::rect(lx, ly, lw, height), &f, &Brush::solid(0xFFEB_EBEB));
        }
        g.flush();
    }
    img
}

fn blit_image(dc: HDC, img: &Image, x: i32, y: i32) {
    unsafe {
        if let Some(hb) = screen::to_hbitmap(img) {
            let mem = CreateCompatibleDC(Some(dc));
            let o = SelectObject(mem, hb.into());
            let _ = BitBlt(dc, x, y, img.wi(), img.hi(), Some(mem), 0, 0, SRCCOPY);
            SelectObject(mem, o);
            let _ = DeleteObject(hb.into());
            let _ = DeleteDC(mem);
        }
    }
}

fn fill(dc: HDC, r: RECT, c: u32) {
    unsafe {
        let b = windows::Win32::Graphics::Gdi::CreateSolidBrush(windows::Win32::Foundation::COLORREF(c));
        windows::Win32::Graphics::Gdi::FillRect(dc, &r, b);
        let _ = DeleteObject(b.into());
    }
}

fn picker_paint(hwnd: HWND, p: &Picker, hdc: HDC) {
    let (ww, hh) = (p.vs.width(), p.vs.height());
    let dpi = unsafe { windows::Win32::UI::HiDpi::GetDpiForWindow(hwnd) }.max(96);
    unsafe {
        let mem = CreateCompatibleDC(Some(hdc));
        let bb = CreateCompatibleBitmap(hdc, ww, hh);
        let ob = SelectObject(mem, bb.into());
        let _ = BitBlt(mem, 0, 0, ww, hh, Some(p.dc), 0, 0, SRCCOPY);
        // the magnifier: 15 × 15 pixels at 10× with the center pixel outlined
        let (n, z) = (15, sc(10, dpi));
        let (bx0, info_h) = (n * z, sc(74, dpi));
        let mut bx = p.cur.x + sc(26, dpi);
        let mut by = p.cur.y + sc(26, dpi);
        if bx + bx0 > ww {
            bx = p.cur.x - sc(26, dpi) - bx0;
        }
        if by + bx0 + info_h > hh {
            by = p.cur.y - sc(26, dpi) - bx0 - info_h;
        }
        SetStretchBltMode(mem, COLORONCOLOR);
        let _ = StretchBlt(mem, bx, by, bx0, bx0, Some(p.dc), p.cur.x - n / 2, p.cur.y - n / 2, n, n, SRCCOPY);
        for i in 1..n {
            fill(mem, RECT { left: bx + i * z, top: by, right: bx + i * z + 1, bottom: by + bx0 }, 0x5A5A5A);
            fill(mem, RECT { left: bx, top: by + i * z, right: bx + bx0, bottom: by + i * z + 1 }, 0x5A5A5A);
        }
        let frame = |r: RECT, c: u32, t: i32| {
            fill(mem, RECT { bottom: r.top + t, ..r }, c);
            fill(mem, RECT { top: r.bottom - t, ..r }, c);
            fill(mem, RECT { right: r.left + t, ..r }, c);
            fill(mem, RECT { left: r.right - t, ..r }, c);
        };
        frame(RECT { left: bx - 2, top: by - 2, right: bx + bx0 + 2, bottom: by + bx0 + 2 }, 0xFFFFFF, 2);
        let cx = bx + (n / 2) * z;
        let cy = by + (n / 2) * z;
        frame(RECT { left: cx - 1, top: cy - 1, right: cx + z + 1, bottom: cy + z + 1 }, 0x2828FF, 2);
        // the values
        let c = colorref_at(p, p.cur);
        let lines = [color::to_text(c, 0), format!("RGB {}", color::to_text(c, 1)),
                     format!("{}   {}, {}", color::to_text(c, 2), p.cur.x + p.vs.left, p.cur.y + p.vs.top)];
        blit_image(mem, &text_panel(bx0 + 4, info_h - 1, dpi, Some(c), &lines), bx - 2, by + bx0 + 2);
        // the hint at the top of the monitor under the cursor
        let mon = screen::monitor_rect(p.cur.x + p.vs.left, p.cur.y + p.vs.top, false).offset(-p.vs.left, -p.vs.top);
        let (hw, hhh) = (sc(640, dpi), sc(32, dpi));
        let hint = tr("Conta-gotas: clique para copiar a cor • setas movem 1 px (Shift = 10) • Enter = copiar • Esc = cancelar");
        blit_image(mem, &text_panel(hw, hhh, dpi, None, &[hint]), (mon.left + mon.right - hw) / 2, mon.top + sc(16, dpi));
        let _ = BitBlt(hdc, 0, 0, ww, hh, Some(mem), 0, 0, SRCCOPY);
        SelectObject(mem, ob);
        let _ = DeleteObject(bb.into());
        let _ = DeleteDC(mem);
    }
}

fn picker_finish(ok: bool) {
    let Some((hwnd, c)) = PICKER.with(|p| {
        let mut b = p.borrow_mut();
        let (hw, pk) = b.as_mut()?;
        if pk.finished {
            return None;
        }
        pk.finished = true;
        Some((*hw, colorref_at(pk, pk.cur)))
    }) else {
        return;
    };
    unsafe {
        let _ = DestroyWindow(h(hwnd));
    }
    if ok {
        send(UiEv::ColorPicked(c));
    }
}

unsafe extern "system" fn picker_proc(hwnd: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let pt = || POINT { x: (lp.0 & 0xFFFF) as i16 as i32, y: ((lp.0 >> 16) & 0xFFFF) as i16 as i32 };
    match m {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            unsafe {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                PICKER.with(|p| {
                    if let Some((_, pk)) = p.borrow().as_ref() {
                        picker_paint(hwnd, pk, dc);
                    }
                });
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_SETCURSOR => {
            unsafe {
                SetCursor(LoadCursorW(None, IDC_CROSS).ok());
            }
            LRESULT(1)
        }
        WM_MOUSEMOVE => {
            PICKER.with(|p| {
                if let Some((_, pk)) = p.borrow_mut().as_mut() {
                    pk.cur = pt();
                }
            });
            unsafe {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            PICKER.with(|p| {
                if let Some((_, pk)) = p.borrow_mut().as_mut() {
                    pk.cur = pt();
                }
            });
            picker_finish(true);
            LRESULT(0)
        }
        WM_RBUTTONUP | WM_CLOSE => {
            picker_finish(false);
            LRESULT(0)
        }
        WM_HOTKEY if wp.0 as i32 == PICK_HOTKEY => {
            picker_finish(false);
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let vk = wp.0 as u16;
            if vk == VK_ESCAPE.0 {
                picker_finish(false);
            } else if vk == VK_RETURN.0 || vk == VK_SPACE.0 {
                picker_finish(true);
            } else if [VK_LEFT.0, VK_RIGHT.0, VK_UP.0, VK_DOWN.0].contains(&vk) {
                let mut c = POINT::default();
                unsafe {
                    let _ = GetCursorPos(&mut c);
                }
                let s = if unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0 { 10 } else { 1 };
                c.x += if vk == VK_LEFT.0 { -s } else if vk == VK_RIGHT.0 { s } else { 0 };
                c.y += if vk == VK_UP.0 { -s } else if vk == VK_DOWN.0 { s } else { 0 };
                unsafe {
                    let _ = SetCursorPos(c.x, c.y);
                }
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe {
                let _ = UnregisterHotKey(Some(hwnd), PICK_HOTKEY);
                let _ = KillTimer(Some(hwnd), 1);
            }
            if let Some((_, pk)) = PICKER.with(|p| p.borrow_mut().take()) {
                unsafe {
                    SelectObject(pk.dc, pk.old);
                    let _ = DeleteDC(pk.dc);
                    let _ = DeleteObject(pk.bmp.into());
                }
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, m, wp, lp) },
    }
}

pub fn picker_active() -> bool {
    PICKER.with(|p| p.borrow().is_some())
}

/// The full-screen picker; the color comes back as `UiEv::ColorPicked`.
pub fn run_color_picker() {
    if picker_active() {
        return;
    }
    static REG: Once = Once::new();
    REG.call_once(|| unsafe {
        let wc = WNDCLASSW {
            lpfnWndProc: Some(picker_proc),
            hInstance: windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default().into(),
            hCursor: LoadCursorW(None, IDC_CROSS).unwrap_or_default(),
            lpszClassName: w!("SnapnabColorPicker"),
            ..Default::default()
        };
        RegisterClassW(&wc);
    });
    let vs = screen::virtual_screen();
    let frozen = screen::grab(vs);
    let Some(bmp) = screen::to_hbitmap(&frozen) else { return };
    let mut c = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut c);
    }
    let (dc, old) = unsafe {
        let sdc = GetDC(None);
        let dc = CreateCompatibleDC(Some(sdc));
        ReleaseDC(None, sdc);
        let old = SelectObject(dc, bmp.into());
        (dc, old)
    };
    let title = to_wide(tr("Snapnab — conta-gotas"));
    let Ok(hw) = (unsafe {
        CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW, w!("SnapnabColorPicker"), PCWSTR(title.as_ptr()), WS_POPUP, vs.left, vs.top,
                        vs.width(), vs.height(), None, None, None, None)
    }) else {
        unsafe {
            SelectObject(dc, old);
            let _ = DeleteDC(dc);
            let _ = DeleteObject(bmp.into());
        }
        return;
    };
    PICKER.with(|p| {
        *p.borrow_mut() = Some((hw.0 as isize, Picker {
            frozen,
            vs,
            dc,
            bmp,
            old,
            cur: POINT { x: c.x - vs.left, y: c.y - vs.top },
            finished: false,
        }))
    });
    unsafe {
        let _ = RegisterHotKey(Some(hw), PICK_HOTKEY, MOD_NOREPEAT, VK_ESCAPE.0 as u32);
        let _ = ShowWindow(hw, SW_SHOW);
        let _ = SetForegroundWindow(hw);
        let _ = SetFocus(Some(hw));
    }
    log::write("Color picker: on");
}

/// ColorFormatsMenu: every format of the color (the default one marked), the recent colors and the default format.
pub fn color_formats_menu(owner: isize, c: u32, at: (i32, i32)) {
    use crate::tray::{Item, Menu};
    let def = color::default_format();
    let n = color::FORMATS.len() as i32;
    let texts: Vec<String> = (0..n).map(|f| format!("{}\t{}", color::to_text(c, f), color::FORMATS[f as usize])).collect();
    let recent = color::recent();
    let rec_t: Vec<String> = recent.iter().map(|&r| color::to_text(r, def)).collect();
    let rm = Menu::new(rec_t.iter().enumerate().map(|(i, t)| Item::Cmd(200 + i as u32, t)).collect());
    let dm = Menu::new((0..n).map(|f| Item::Checked(300 + f as u32, color::FORMATS[f as usize], f == def)).collect());
    let (rec_label, def_label) = (tr("Cores recentes"), tr("Formato padrão"));
    let mut items: Vec<Item> = texts.iter().enumerate().map(|(f, t)| Item::Checked(100 + f as u32, t, f as i32 == def)).collect();
    if !recent.is_empty() {
        items.push(Item::Separator);
        items.push(Item::Sub(&rec_label, rm));
    }
    items.push(Item::Sub(&def_label, dm));
    let id = Menu::new(items).choose(owner, at.0, at.1) as i32;
    if (100..100 + n).contains(&id) {
        snapcore::clip::copy_text(owner, &color::to_text(c, id - 100));
    } else if (200..200 + recent.len() as i32).contains(&id) {
        let r = recent[(id - 200) as usize];
        snapcore::clip::copy_text(owner, &color::to_text(r, def));
        color::add_recent(r);
    } else if (300..300 + n).contains(&id) {
        color::set_default_format(id - 300);
        snapcore::clip::copy_text(owner, &color::to_text(c, id - 300));
    }
}
