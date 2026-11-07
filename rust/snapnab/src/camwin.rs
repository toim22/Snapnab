//! The webcam bubble of the recordings (WebcamOverlay in Webcam.cpp): an always-on-top layered window inside the
//! recorded area, so the recording copies it into the video. It shows the latest camera frame mirrored, in the chosen
//! shape with a shadow and a white ring, or cut out by the chroma key; it can be dragged anywhere while recording.

use std::sync::{Mutex, Once};

use snapcore::gdip::{self, Brush, Graphics, Path, Pen};
use snapcore::image::{Image, Rect};
use snapcore::log;
use snapcore::webcam::{Camera, chroma_alpha};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::GdiPlus::{InterpolationModeHighQualityBilinear, PixelOffsetModeHalf, SmoothingModeAntiAlias};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetWindowRect, HTCAPTION, HWND_TOPMOST, IDC_SIZEALL, LoadCursorW,
    MA_NOACTIVATE, MSG, PM_REMOVE, PeekMessageW, PostMessageW, RegisterClassW, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE,
    SetWindowPos, ShowWindow, WM_APP, WM_MOUSEACTIVATE, WM_NCHITTEST, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::w;

const WM_CAM_FRAME: u32 = WM_APP + 1;

#[derive(Clone, Copy, Debug)]
pub struct Style {
    /// CamShape: 0 circle, 1 rounded square, 2 square, 3 16:9, 4 4:3
    pub shape: i32,
    /// removes the background of the key color (no frame or shadow)
    pub chroma: bool,
    /// COLORREF
    pub key: u32,
    /// 0..100
    pub tolerance: i32,
}

struct Bubble {
    hwnd: isize,
    cam: Option<Camera>,
    style: Style,
}

static BUBBLE: Mutex<Option<Bubble>> = Mutex::new(None);

fn h(x: isize) -> HWND {
    HWND(x as _)
}

fn sc(v: i32, dpi: u32) -> i32 {
    (v as i64 * dpi as i64 / 96) as i32
}

unsafe extern "system" fn proc(hwnd: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match m {
        WM_CAM_FRAME => {
            // only the most recent frame is drawn
            let mut drop = MSG::default();
            while unsafe { PeekMessageW(&mut drop, Some(hwnd), WM_CAM_FRAME, WM_CAM_FRAME, PM_REMOVE) }.as_bool() {}
            render();
            LRESULT(0)
        }
        WM_NCHITTEST => LRESULT(HTCAPTION as isize), // drag the bubble anywhere
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        _ => unsafe { DefWindowProcW(hwnd, m, wp, lp) },
    }
}

/// Starts the camera (`link` empty: the first one) in the bottom-right corner of `area`.
pub fn start(link: &str, area: Rect, style: Style) -> Result<(), String> {
    stop();
    static REG: Once = Once::new();
    REG.call_once(|| unsafe {
        let wc = WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default().into(),
            hCursor: LoadCursorW(None, IDC_SIZEALL).unwrap_or_default(),
            lpszClassName: w!("SnapnabWebcam"),
            ..Default::default()
        };
        RegisterClassW(&wc);
    });
    let title = snapcore::wide::to_wide(snapnab_app::tr("Snapnab — webcam"));
    let hwnd = unsafe {
        CreateWindowExW(WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE, w!("SnapnabWebcam"),
                        windows::core::PCWSTR(title.as_ptr()), WS_POPUP, area.right - 10, area.bottom - 10, 10, 10, None, None, None,
                        None)
    }
    .map(|h| h.0 as isize)
    .map_err(|e| e.message())?;
    let cam = match Camera::start(link, move || unsafe {
        let _ = PostMessageW(Some(h(hwnd)), WM_CAM_FRAME, WPARAM(0), LPARAM(0));
    }) {
        Ok(c) => c,
        Err(e) => {
            unsafe {
                let _ = DestroyWindow(h(hwnd));
            }
            return Err(e);
        }
    };
    // the size from the shape (with chroma key, the whole camera image, no frame); bottom-right corner of the area
    let dpi = unsafe { GetDpiForWindow(h(hwnd)) }.max(96);
    let hh = sc(if style.chroma { 220 } else { 176 }, dpi);
    let ww = if style.chroma {
        (hh as f64 * cam.aspect).round() as i32
    } else {
        match style.shape {
            3 => hh * 16 / 9,
            4 => hh * 4 / 3,
            _ => hh,
        }
    };
    let sh = if style.chroma { 0 } else { sc(8, dpi) };
    let (bw, bh, m) = (ww + 2 * sh, hh + 2 * sh, sc(12, dpi));
    let (x, y) = (area.left.max(area.right - bw - m), area.top.max(area.bottom - bh - m));
    unsafe {
        let _ = SetWindowPos(h(hwnd), Some(HWND_TOPMOST), x, y, bw, bh, SWP_NOACTIVATE);
    }
    if let Ok(mut b) = BUBBLE.lock() {
        *b = Some(Bubble { hwnd, cam: Some(cam), style });
    }
    render();
    unsafe {
        let _ = ShowWindow(h(hwnd), SW_SHOWNOACTIVATE);
    }
    Ok(())
}

pub fn set_visible(on: bool) {
    if let Some(b) = BUBBLE.lock().ok().and_then(|b| b.as_ref().map(|b| b.hwnd)) {
        unsafe {
            let _ = ShowWindow(h(b), if on { SW_SHOWNOACTIVATE } else { SW_HIDE });
        }
    }
}

pub fn stop() {
    // taken out of the lock first: dropping the camera waits for its thread
    let b = BUBBLE.lock().ok().and_then(|mut b| b.take());
    if let Some(mut b) = b {
        drop(b.cam.take());
        unsafe {
            let _ = DestroyWindow(h(b.hwnd));
        }
        log::write("Webcam: stopped");
    }
}

/// The outline of the shape in `r` (x, y, w, h).
fn shape_path(shape: i32, r: (f32, f32, f32, f32), dpi: u32) -> Path {
    let p = Path::new();
    let (x, y, w, hh) = r;
    let rad = match shape {
        0 => {
            p.ellipse(x, y, w, hh);
            return p;
        }
        1 => hh * 0.22,
        2 => sc(3, dpi) as f32,
        _ => sc(14, dpi) as f32,
    };
    let d = rad * 2.0;
    p.arc(x, y, d, d, 180.0, 90.0);
    p.arc(x + w - d, y, d, d, 270.0, 90.0);
    p.arc(x + w - d, y + hh - d, d, d, 0.0, 90.0);
    p.arc(x, y + hh - d, d, d, 90.0, 90.0);
    p.close_figure();
    p
}

fn render() {
    let (hwnd, frame, st) = {
        let Ok(b) = BUBBLE.lock() else { return };
        let Some(b) = b.as_ref() else { return };
        (b.hwnd, b.cam.as_ref().map(|c| c.frame()).unwrap_or_default(), b.style)
    };
    let mut wr = RECT::default();
    unsafe {
        let _ = GetWindowRect(h(hwnd), &mut wr);
    }
    let (ww, hh) = (wr.right - wr.left, wr.bottom - wr.top);
    let dpi = unsafe { GetDpiForWindow(h(hwnd)) }.max(96);
    let sh = if st.chroma { 0 } else { sc(8, dpi) };
    let (cw, ch) = ((ww - 2 * sh) as f32, (hh - 2 * sh) as f32);
    if cw < 4.0 || ch < 4.0 {
        return;
    }
    let mut canvas = Image::sized(ww, hh, 0);
    // the frame mirrored (like a mirror, as in video calls), cropped to the window's aspect ratio, keyed out
    let mut f = frame;
    if !f.is_empty() {
        let w = f.w as usize;
        for row in f.px.chunks_mut(w) {
            row.reverse();
        }
        if st.chroma {
            // background removed: per-pixel alpha, the edge color pulled toward gray so no halo of the key is left
            for p in f.px.iter_mut() {
                let a = chroma_alpha(*p, st.key, st.tolerance);
                if a == 255 {
                    continue;
                }
                let (mut r, mut g, mut b) = (((*p >> 16) & 255) as f64, ((*p >> 8) & 255) as f64, (*p & 255) as f64);
                let y = ((r as i32 * 77 + g as i32 * 150 + b as i32 * 29) >> 8) as f64;
                let k = 1.0 - a as f64 / 255.0;
                (r, g, b) = (r + (y - r) * k, g + (y - g) * k, b + (y - b) * k);
                *p = ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | b as u32;
            }
        }
    }
    {
        let bmp = gdip::Bitmap::wrap(&mut canvas);
        let g = Graphics::new(&bmp);
        g.smoothing(SmoothingModeAntiAlias);
        g.interpolation(InterpolationModeHighQualityBilinear);
        g.pixel_offset(PixelOffsetModeHalf);
        let cam = (sh as f32, sh as f32, cw, ch);
        let outline = shape_path(st.shape, cam, dpi);
        if !st.chroma {
            for i in (1..=sh).rev() {
                // the shadow
                let t = 1.0 - i as f64 / (sh + 1) as f64;
                let (fi, dy) = (i as f32, i as f32 * 0.4);
                let sp = shape_path(st.shape, (cam.0 - fi, cam.1 - fi + dy, cam.2 + 2.0 * fi, cam.3 + 2.0 * fi), dpi);
                g.fill_path(&Brush::solid(((14.0 * t * t).round() as u32) << 24), &sp);
            }
            g.fill_path(&Brush::solid(0xFF28282C), &outline);
        }
        if !f.is_empty() {
            let want = (cw / ch) as f64;
            let (mut sw, mut shh) = (f.w as f64, f.h as f64);
            if sw / shh > want {
                sw = shh * want;
            } else {
                shh = sw / want;
            }
            let src = gdip::Bitmap::wrap_read(&f);
            let st_ = g.save();
            if !st.chroma {
                g.clip_path(&outline);
            }
            g.draw_image_src(&src, gdip::rect(cam.0, cam.1, cw, ch),
                             gdip::rect(((f.w as f64 - sw) / 2.0) as f32, ((f.h as f64 - shh) / 2.0) as f32, sw as f32, shh as f32));
            g.restore(st_);
        }
        if !st.chroma {
            let ring = shape_path(st.shape, (cam.0 + 1.0, cam.1 + 1.0, cw - 2.0, ch - 2.0), dpi);
            g.draw_path(&Pen::new(0xFFFFFFFF, sc(3, dpi) as f32), &ring);
        }
        g.flush();
    }
    crate::rawwin::present(hwnd, &canvas, (wr.left, wr.top));
}
