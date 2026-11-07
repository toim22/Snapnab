//! The notification-area icon (Shell_NotifyIcon, comp_1 §2): a click captures, a double click opens the capture
//! window, the right button opens the menu. The menu is a native Win32 popup menu: Windows draws it with the system
//! look, and its WM_COMMAND reaches the capture window like the C++ app's did.

use std::sync::atomic::{AtomicIsize, Ordering};

use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS, DeleteObject, HBITMAP,
};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIIF_NOSOUND, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION,
    NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, DestroyMenu, GetCursorPos, HICON, HMENU, ICONINFO, MF_CHECKED,
    MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, PostMessageW, SetForegroundWindow, SetMenuDefaultItem, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    TrackPopupMenu, WM_APP, WM_NULL,
};
use windows::core::PCWSTR;

use snapcore::wide::to_wide;

/// The tray icon's callback message (WM_SNAP_TRAY of the C++ app).
pub const WM_SNAP_TRAY: u32 = WM_APP + 0x41;

static ICON_SMALL: AtomicIsize = AtomicIsize::new(0);
static ICON_BUSY: AtomicIsize = AtomicIsize::new(0);

/// The app icon drawn at run time (Icons.cpp MakeAppIcon): a blue rounded square with a white capture frame.
pub fn app_icon(size: i32) -> HICON {
    let s = size as f32;
    let mut px = vec![0u32; (size * size) as usize];
    const SS: i32 = 4; // 4×4 samples per pixel
    for y in 0..size {
        for x in 0..size {
            let (mut cover, mut white) = (0.0f32, 0.0f32);
            for sy in 0..SS {
                for sx in 0..SS {
                    let fx = x as f32 + (sx as f32 + 0.5) / SS as f32;
                    let fy = y as f32 + (sy as f32 + 0.5) / SS as f32;
                    if !in_round_rect(fx, fy, s, s * 0.11) {
                        continue;
                    }
                    cover += 1.0;
                    if in_glyph(fx / s, fy / s) {
                        white += 1.0;
                    }
                }
            }
            let n = (SS * SS) as f32;
            if cover == 0.0 {
                continue;
            }
            let t = (x + y) as f32 / (2.0 * s);
            let mix = |a: f32, b: f32| a + (b - a) * t;
            let (r, g, b) = (mix(30.0, 20.0), mix(140.0, 70.0), mix(255.0, 200.0));
            let w = white / cover;
            let ch = |c: f32| (c + (255.0 - c) * w).round() as u32;
            let a = (cover / n * 255.0).round() as u32;
            px[(y * size + x) as usize] = a << 24 | ch(r) << 16 | ch(g) << 8 | ch(b);
        }
    }
    unsafe {
        let bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size,
                biHeight: -size,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let Ok(color) = CreateDIBSection(None, &bi, DIB_RGB_COLORS, &mut bits, None, 0) else { return HICON::default() };
        std::ptr::copy_nonoverlapping(px.as_ptr(), bits as *mut u32, px.len());
        let mask: HBITMAP = CreateBitmap(size, size, 1, 1, None);
        let info = ICONINFO { fIcon: true.into(), xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&info).unwrap_or_default();
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        icon
    }
}

fn in_round_rect(x: f32, y: f32, s: f32, r: f32) -> bool {
    let cx = x.clamp(r, s - r);
    let cy = y.clamp(r, s - r);
    (x - cx).powi(2) + (y - cy).powi(2) <= r * r
}

/// Corner brackets and a lens, in the unit square.
fn in_glyph(u: f32, v: f32) -> bool {
    let (a, b, t, l) = (0.2, 0.8, 0.07, 0.2);
    let bracket = |p: f32, q: f32| {
        let near_p = (p >= a && p <= a + t) || (p >= b - t && p <= b);
        let in_q = (q >= a && q <= a + l) || (q >= b - l && q <= b);
        near_p && in_q
    };
    let d = ((u - 0.5).powi(2) + (v - 0.5).powi(2)).sqrt();
    bracket(u, v) || bracket(v, u) || (d <= 0.17 && d >= 0.09)
}

fn nid(hwnd: isize) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: HWND(hwnd as _),
        uID: 1,
        ..Default::default()
    }
}

fn copy(dst: &mut [u16], s: &str) {
    let w: Vec<u16> = s.encode_utf16().take(dst.len() - 1).collect();
    dst[..w.len()].copy_from_slice(&w);
    dst[w.len()] = 0;
}

pub fn add(hwnd: isize, tip: &str) {
    let mut icon = ICON_SMALL.load(Ordering::Relaxed);
    if icon == 0 {
        icon = app_icon(16).0 as isize;
        ICON_SMALL.store(icon, Ordering::Relaxed);
    }
    let mut n = nid(hwnd);
    n.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    n.uCallbackMessage = WM_SNAP_TRAY;
    n.hIcon = HICON(icon as _);
    copy(&mut n.szTip, tip);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_ADD, &n);
        n.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        let _ = Shell_NotifyIconW(NIM_SETVERSION, &n);
    }
}

pub fn remove(hwnd: isize) {
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid(hwnd));
    }
}

/// MakeBusyIcon: the app icon with a red dot (the interval capture runs).
fn busy_icon(size: i32) -> HICON {
    let base = app_icon(size);
    let mut px = vec![0u32; (size * size) as usize];
    unsafe {
        use windows::Win32::Graphics::Gdi::{CreateCompatibleDC, DeleteDC, SelectObject};
        use windows::Win32::UI::WindowsAndMessaging::{DI_NORMAL, DestroyIcon, DrawIconEx};
        let bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size,
                biHeight: -size,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let Ok(color) = CreateDIBSection(None, &bi, DIB_RGB_COLORS, &mut bits, None, 0) else { return base };
        let dc = CreateCompatibleDC(None);
        let o = SelectObject(dc, color.into());
        let _ = DrawIconEx(dc, 0, 0, base, size, size, 0, None, DI_NORMAL);
        SelectObject(dc, o);
        let _ = DeleteDC(dc);
        std::ptr::copy_nonoverlapping(bits as *const u32, px.as_mut_ptr(), px.len());
        let r = (size * 3 / 8) as f32;
        for y in 0..size {
            for x in 0..size {
                let (dx, dy) = (x as f32 + 0.5 - (size as f32 - r), y as f32 + 0.5 - (size as f32 - r));
                if dx * dx + dy * dy <= r * r {
                    px[(y * size + x) as usize] = 0xFFE5_3935;
                }
            }
        }
        std::ptr::copy_nonoverlapping(px.as_ptr(), bits as *mut u32, px.len());
        let mask: HBITMAP = CreateBitmap(size, size, 1, 1, None);
        let info = ICONINFO { fIcon: true.into(), xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&info).unwrap_or_default();
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        let _ = DestroyIcon(base);
        icon
    }
}

/// UpdateTrayStatus: the red dot and the count while the interval capture runs.
pub fn set_status(hwnd: isize, busy: bool, tip: &str) {
    let icon = if busy {
        let mut b = ICON_BUSY.load(Ordering::Relaxed);
        if b == 0 {
            b = busy_icon(16).0 as isize;
            ICON_BUSY.store(b, Ordering::Relaxed);
        }
        b
    } else {
        ICON_SMALL.load(Ordering::Relaxed)
    };
    let mut n = nid(hwnd);
    n.uFlags = NIF_ICON | NIF_TIP;
    n.hIcon = HICON(icon as _);
    copy(&mut n.szTip, tip);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &n);
    }
}

/// A balloon notification from the tray icon.
pub fn notify(hwnd: isize, title: &str, text: &str) {
    let mut n = nid(hwnd);
    n.uFlags = NIF_INFO;
    copy(&mut n.szInfoTitle, title);
    copy(&mut n.szInfo, text);
    n.dwInfoFlags = NIIF_INFO | NIIF_NOSOUND;
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &n);
    }
}

/// A native popup menu under construction.
pub struct Menu(HMENU);

pub enum Item<'a> {
    Cmd(u32, &'a str),
    Checked(u32, &'a str, bool),
    Sub(&'a str, Menu),
    /// grayed out
    Disabled(&'a str),
    Separator,
}

impl Menu {
    pub fn new(items: Vec<Item>) -> Menu {
        let m = unsafe { CreatePopupMenu() }.unwrap_or_default();
        for it in items {
            unsafe {
                let _ = match it {
                    Item::Cmd(id, t) => AppendMenuW(m, MF_STRING, id as usize, PCWSTR(to_wide(t).as_ptr())),
                    Item::Checked(id, t, on) => AppendMenuW(m, MF_STRING | if on { MF_CHECKED } else { Default::default() },
                                                            id as usize, PCWSTR(to_wide(t).as_ptr())),
                    Item::Sub(t, sub) => {
                        let h = sub.0;
                        std::mem::forget(sub); // owned by the parent menu from now on
                        AppendMenuW(m, MF_POPUP, h.0 as usize, PCWSTR(to_wide(t).as_ptr()))
                    }
                    Item::Disabled(t) => AppendMenuW(m, MF_STRING | MF_GRAYED, 0, PCWSTR(to_wide(t).as_ptr())),
                    Item::Separator => AppendMenuW(m, MF_SEPARATOR, 0, PCWSTR::null()),
                };
            }
        }
        Menu(m)
    }

    /// Shows the menu at the cursor; the chosen item arrives as WM_COMMAND at `owner`.
    pub fn track(&self, owner: isize, default_item: Option<u32>) {
        unsafe {
            if let Some(d) = default_item {
                let _ = SetMenuDefaultItem(self.0, d, 0);
            }
            let mut p = POINT::default();
            let _ = GetCursorPos(&mut p);
            let owner = HWND(owner as _);
            let _ = SetForegroundWindow(owner);
            let _ = TrackPopupMenu(self.0, TPM_RIGHTBUTTON, p.x, p.y, None, owner, None);
            let _ = PostMessageW(Some(owner), WM_NULL, Default::default(), Default::default());
        }
    }
}

impl Menu {
    /// Shows the menu at a screen point and returns the chosen item (0: none).
    pub fn choose(&self, owner: isize, x: i32, y: i32) -> u32 {
        unsafe {
            let owner = HWND(owner as _);
            let _ = SetForegroundWindow(owner);
            TrackPopupMenu(self.0, TPM_RETURNCMD | TPM_RIGHTBUTTON, x, y, None, owner, None).0 as u32
        }
    }
}

impl Drop for Menu {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyMenu(self.0);
        }
    }
}
