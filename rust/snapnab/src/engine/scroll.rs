//! The captures that move the screen (CaptureEngine.cpp, "scrolling" and "extended window", and Panoramic.cpp):
//! the scrolling window (the wheel scrolls it and the frames are stitched), the extended window (SnapEx64 lets it grow
//! beyond the screen while PrintWindow takes it) and the panoramic region (the user scrolls, the region is copied
//! several times per second). The first two run on their own thread (they wait for the window between steps) and come
//! back as `EngineEv::Grabbed`.

use std::time::{Duration, Instant};

use egui::{Align2, Color32, CornerRadius, FontId, Rect as ERect, Sense, pos2, vec2};
use snapcore::image::{Image, Rect};
use snapcore::library::CaptureInfo;
use snapcore::panorama::Stitcher;
use snapcore::{cv, log, paths, screen};
use snapnab_app::tr;
use snapui::host::{Caught, Ctl, RawMsg, WinKey, WindowSpec};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::MapWindowPoints;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_MOUSE, MOD_NOREPEAT, MOUSEEVENTF_WHEEL, MOUSEINPUT, RegisterHotKey, SendInput,
    UnregisterHotKey, VK_ESCAPE, VK_RETURN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClientRect, GetCursorPos, GetWindowPlacement, GetWindowThreadProcessId, IsZoomed, MA_NOACTIVATE, RegisterWindowMessageW,
    SMTO_ABORTIFHUNG, SW_RESTORE, SWP_NOACTIVATE, SWP_NOZORDER, SendMessageTimeoutW, SetCursorPos, SetForegroundWindow,
    SetWindowPlacement, SetWindowPos, ShowWindow, WHEEL_DELTA, WINDOWPLACEMENT, WM_HOTKEY, WM_MOUSEACTIVATE,
};
use windows::core::{PCSTR, PCWSTR, w};

use super::{Engine, EngineEv, Env, video::float_window};
use crate::{form, rawwin};

fn h(x: isize) -> HWND {
    HWND(x as _)
}

fn sleep(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

// ---------------------------------------------------------------- scrolling (CScroll::Capture, §3.2)
/// Almost every sampled pixel is the same: the window did not scroll.
fn same_image(a: &Image, b: &Image) -> bool {
    if a.w != b.w || a.h != b.h {
        return false;
    }
    let diff = a.px.iter().zip(&b.px).step_by(7).filter(|(x, y)| (**x & 0xFF_FFFF) != (**y & 0xFF_FFFF)).count();
    diff * 7 < a.px.len() / 2000 + 1
}

fn wheel(notches: i32) {
    let i = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT { dwFlags: MOUSEEVENTF_WHEEL, mouseData: (notches * WHEEL_DELTA as i32) as u32, ..Default::default() },
        },
    };
    unsafe {
        SendInput(&[i], std::mem::size_of::<INPUT>() as i32);
    }
}

fn esc_down() -> bool {
    unsafe { GetAsyncKeyState(VK_ESCAPE.0 as i32) as u16 & 0x8000 != 0 }
}

/// Scrolls the control under the click (or the window's client area) from the top down, stitching what appears.
pub fn scroll_capture(top: isize, child: isize, vs: Rect) -> Image {
    if top == 0 {
        return Image::default();
    }
    let area = if child != 0 && child != top { child } else { top };
    let mut r = Default::default();
    unsafe {
        let _ = GetClientRect(h(area), &mut r);
        let mut pts = [POINT { x: r.left, y: r.top }, POINT { x: r.right, y: r.bottom }];
        MapWindowPoints(Some(h(area)), None, &mut pts);
        r = windows::Win32::Foundation::RECT { left: pts[0].x, top: pts[0].y, right: pts[1].x, bottom: pts[1].y };
    }
    let r = screen::to_rect(r).intersect(&screen::window_frame_rect(top)).intersect(&vs);
    if r.width() < 32 || r.height() < 32 {
        return Image::default();
    }
    let mut saved = POINT::default();
    unsafe {
        let _ = SetForegroundWindow(h(top));
        let _ = GetCursorPos(&mut saved);
        let _ = SetCursorPos((r.left + r.right) / 2, (r.top + r.bottom) / 2);
    }
    sleep(300);
    let grab = || screen::grab(r);
    // starts from the top
    let mut cur = grab();
    for _ in 0..40 {
        if esc_down() {
            break;
        }
        wheel(10);
        sleep(120);
        let n = grab();
        if same_image(&n, &cur) {
            break;
        }
        cur = n;
    }
    sleep(250);
    cur = grab();
    let mut result = cur.clone();
    let mut stable = 0;
    for _ in 0..400 {
        if esc_down() {
            break;
        }
        wheel(-3);
        sleep(320);
        let next = grab();
        if same_image(&next, &cur) {
            stable += 1;
            if stable >= 2 {
                break;
            }
            continue;
        }
        stable = 0;
        let ((_, ty), conf) = cv::detect_translation(&cur, &next);
        let s = -ty; // the content moved up by s pixels: next(y) = cur(y + s)
        if s <= 0 || s >= cur.hi() {
            log::write(&format!("Scrolling: invalid offset ({s}, conf {conf:.1}) — stopping"));
            break;
        }
        let mut grown = Image::sized(result.wi(), result.hi() + s, 0);
        grown.blit(&result, 0, 0, false);
        grown.blit(&next.crop(Rect::new(0, next.hi() - s, next.wi(), next.hi())), 0, result.hi(), false);
        result = grown;
        cur = next;
        if result.hi() > 30000 {
            break;
        }
    }
    unsafe {
        let _ = SetCursorPos(saved.x, saved.y);
    }
    log::write(&format!("Scrolling: result {}x{}", result.w, result.h));
    result
}

// ---------------------------------------------------------------- extended window (SnapEx64, §6)
type Snap5000 = unsafe extern "system" fn(u32, u32, u32) -> i32;
type Snap5001 = unsafe extern "system" fn();

/// Asks for the new size, lets the window grow beyond the screen, takes it with PrintWindow and puts it back.
pub fn extended_capture(top: isize) -> Image {
    use windows::Win32::Foundation::FreeLibrary;
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    if top == 0 {
        return Image::default();
    }
    let wr = screen::window_rect(top);
    let mut f = [
        form::Field::Label(tr("A janela será ampliada além da tela, capturada e restaurada.")),
        form::Field::Int { label: tr("Nova largura (px)"), value: wr.width(), min: 100, max: 19500 },
        form::Field::Int { label: tr("Nova altura (px)"), value: (wr.height() * 3).min(19500), min: 100, max: 19500 },
    ];
    if !form::run(0, &tr("Janela estendida"), &mut f) {
        return Image::default();
    }
    let (form::Field::Int { value: nw, .. }, form::Field::Int { value: nh, .. }) = (&f[1], &f[2]) else { return Image::default() };
    let (nw, nh) = (*nw, *nh);

    let dll_path = snapcore::wide::to_wide(paths::exe_dir().join("SnapEx64.dll"));
    let dll = unsafe { LoadLibraryW(PCWSTR(dll_path.as_ptr())) }.ok();
    let proc = |name: &[u8]| dll.and_then(|d| unsafe { GetProcAddress(d, PCSTR(name.as_ptr())) });
    // SAFETY: the two exports of SnapEx64.dll have these signatures
    let s5000: Option<Snap5000> = proc(b"Snap5000\0").map(|p| unsafe { std::mem::transmute(p) });
    let s5001: Option<Snap5001> = proc(b"Snap5001\0").map(|p| unsafe { std::mem::transmute(p) });
    let (enter, leave) = unsafe { (RegisterWindowMessageW(w!("SnapnabExtendedWindowEnter")), RegisterWindowMessageW(w!("SnapnabExtendedWindowLeave"))) };
    let send = |m: u32| unsafe {
        SendMessageTimeoutW(h(top), m, WPARAM(0), LPARAM(0), SMTO_ABORTIFHUNG, 2000, None);
    };
    let mut hooked = false;
    if let (Some(s5000), Some(_)) = (s5000, s5001) {
        let tid = unsafe { GetWindowThreadProcessId(h(top), None) };
        hooked = unsafe { s5000(enter, leave, tid) } != 0;
        if hooked {
            send(enter); // the window is subclassed
        }
    }
    if !hooked {
        log::write("Extended window: hook unavailable (DLL or 32-bit process); trying without it");
    }
    let mut wp = WINDOWPLACEMENT { length: std::mem::size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
    let img;
    unsafe {
        let _ = GetWindowPlacement(h(top), &mut wp);
        if IsZoomed(h(top)).as_bool() {
            let _ = ShowWindow(h(top), SW_RESTORE);
        }
        let wr = screen::window_rect(top);
        let _ = SetWindowPos(h(top), None, wr.left, wr.top, nw, nh, SWP_NOZORDER | SWP_NOACTIVATE);
        sleep(700);
        // without the invisible border (DWM shadow)
        img = screen::print_window(top, screen::window_frame_rect(top)).unwrap_or_default();
        let _ = SetWindowPlacement(h(top), &wp);
        let _ = SetWindowPos(h(top), None, wr.left, wr.top, wr.width(), wr.height(), SWP_NOZORDER | SWP_NOACTIVATE);
        if hooked {
            send(leave);
            if let Some(s5001) = s5001 {
                s5001();
            }
        }
        if let Some(d) = dll {
            let _ = FreeLibrary(d);
        }
    }
    log::write(&format!("Extended window: {}x{}", img.w, img.h));
    img
}

// ---------------------------------------------------------------- panoramic (PanoramicSession)
pub(super) struct Pano {
    key: WinKey,
    hwnd: isize,
    border: isize,
    area: Rect,
    stitch: Stitcher,
    last: Instant,
    info: CaptureInfo,
}

const PANO_ESC: i32 = 1;
const PANO_ENTER: i32 = 2;
const ORANGE: Color32 = Color32::from_rgb(232, 120, 30);

/// The bar never takes the focus from the program being scrolled; Esc and Enter come as hotkeys.
fn pano_intercept(hwnd: isize, msg: u32, wp: usize, lp: isize) -> Caught {
    match msg {
        WM_MOUSEACTIVATE => Caught::Handled(MA_NOACTIVATE as isize),
        WM_HOTKEY => Caught::Queue(RawMsg { hwnd, msg, wparam: wp, lparam: lp, text: None }, 0),
        _ => Caught::Pass,
    }
}

impl Pano {
    fn grab(&mut self) -> bool {
        let mut f = screen::grab(self.area);
        f.make_opaque();
        self.last = Instant::now();
        self.stitch.add(&f)
    }
}

impl Engine {
    /// Scrolling and extended window: on their own thread, back as `EngineEv::Grabbed`.
    pub(super) fn grab_on_thread(&mut self, ctl: &Ctl, info: CaptureInfo, extended: bool, top: isize, child: isize) {
        self.cursor = None;
        self.grabbing = true;
        let (w, vs) = (ctl.waker(), self.vs);
        std::thread::spawn(move || {
            unsafe {
                let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
            }
            // always answers, even after a panic: otherwise the engine would stay busy
            let img = std::panic::catch_unwind(|| if extended { extended_capture(top) } else { scroll_capture(top, child, vs) })
                .unwrap_or_default();
            w.send(EngineEv::Grabbed(img, info));
        });
    }

    pub(super) fn grabbed(&mut self, ctl: &mut Ctl, env: &Env, img: Image, info: CaptureInfo) {
        self.grabbing = false;
        if img.is_empty() {
            self.input_canceled();
        } else {
            self.input_captured(ctl, env, img, info, None);
        }
    }

    /// PanoramicSession::Start: the orange frame, the bar below the region and a copy every 90 ms.
    pub(super) fn start_pano(&mut self, ctl: &mut Ctl, area: Rect, info: CaptureInfo) {
        self.frozen = Image::default(); // the frozen screen is no use: the region is copied live
        let (w, hh) = (420.0f32, 48.0f32);
        let spec = WindowSpec {
            class: "SnapnabPanoBar",
            title: tr("Snapnab — rolagem panorâmica"),
            size: (w, hh),
            visible: false,
            decorations: false,
            resizable: false,
            topmost: true,
            skip_taskbar: true,
            intercept: Some(pano_intercept),
            clear_color: [32.0 / 255.0, 32.0 / 255.0, 34.0 / 255.0, 1.0],
            ..Default::default()
        };
        let Some(k) = ctl.create(&spec) else {
            self.input_canceled();
            return;
        };
        let hwnd = ctl.hwnd(k);
        let sf = ctl.window(k).map_or(1.0, |w| w.scale_factor());
        let s = |v: f32| (v as f64 * sf) as i32;
        let (pw, ph) = (s(w), s(hh));
        let mon = screen::monitor_rect((area.left + area.right) / 2, area.bottom, true);
        let x = ((area.left + area.right) / 2 - pw / 2).clamp(mon.left, (mon.right - pw).max(mon.left));
        let mut y = area.bottom + s(10.0);
        if y + ph > mon.bottom {
            y = area.top - ph - s(10.0);
        }
        if y < mon.top {
            y = area.bottom - ph - s(14.0);
        }
        float_window(hwnd, x, y, pw, ph, s(14.0));
        unsafe {
            let _ = RegisterHotKey(Some(h(hwnd)), PANO_ESC, MOD_NOREPEAT, VK_ESCAPE.0 as u32);
            let _ = RegisterHotKey(Some(h(hwnd)), PANO_ENTER, MOD_NOREPEAT, VK_RETURN.0 as u32);
        }
        let mut p = Pano { key: k, hwnd, border: rawwin::pano_border(area), area, stitch: Stitcher::default(), last: Instant::now(), info };
        p.grab();
        self.pano = Some(p);
        log::write(&format!("Panoramic: started ({}x{})", area.width(), area.height()));
    }

    fn pano_finish(&mut self, ctl: &mut Ctl, env: &Env, ok: bool) {
        let Some(mut p) = self.pano.take() else { return };
        if ok {
            p.grab();
        }
        unsafe {
            let _ = UnregisterHotKey(Some(h(p.hwnd)), PANO_ESC);
            let _ = UnregisterHotKey(Some(h(p.hwnd)), PANO_ENTER);
        }
        ctl.close(p.key);
        rawwin::destroy(&mut p.border);
        let img = if ok { p.stitch.result().clone() } else { Image::default() };
        log::write(&format!("Panoramic: {} ({}x{})", if ok { "done" } else { "canceled" }, img.w, img.h));
        if img.is_empty() {
            self.input_canceled();
        } else {
            self.input_captured(ctl, env, img, p.info, None);
        }
    }

    pub(super) fn pano_owns(&self, k: WinKey) -> bool {
        self.pano.as_ref().is_some_and(|p| p.key == k)
    }

    pub(super) fn pano_message(&mut self, ctl: &mut Ctl, env: &Env, m: &RawMsg) -> bool {
        if m.msg == WM_HOTKEY && self.pano.as_ref().is_some_and(|p| p.hwnd == m.hwnd) {
            self.pano_finish(ctl, env, m.wparam == PANO_ENTER as usize);
            return true;
        }
        false
    }

    pub(super) fn pano_close_requested(&mut self, ctl: &mut Ctl, env: &Env) {
        self.pano_finish(ctl, env, false);
    }

    pub(super) fn pano_ui(&mut self, ctl: &mut Ctl, env: &Env, ui: &mut egui::Ui) {
        let Some(p) = self.pano.as_mut() else { return };
        ui.ctx().request_repaint_after(Duration::from_millis(90));
        if p.last.elapsed() >= Duration::from_millis(90) {
            p.grab();
        }
        let full = ui.max_rect();
        let painter = ui.painter().clone();
        painter.rect_filled(full, 0.0, Color32::from_rgb(32, 32, 34));
        let bg = ui.interact(full, ui.scope_id().with("pano-bg"), Sense::click_and_drag());
        if bg.drag_started()
            && let Some(w) = ctl.window(p.key)
        {
            let _ = w.drag_window();
        }
        let (bw, bh) = (92.0, 30.0);
        let by = full.center().y - bh / 2.0;
        let cancel = ERect::from_min_size(pos2(full.right() - 10.0 - bw, by), vec2(bw, bh));
        let stop = cancel.translate(vec2(-8.0 - bw, 0.0));
        let r = p.stitch.result();
        let t = if p.stitch.axis() == 0 {
            tr("Role o conteúdo dentro da moldura…")
        } else {
            tr("Capturando: %d × %d px").replacen("%d", &r.w.to_string(), 1).replacen("%d", &r.h.to_string(), 1)
        };
        let text_clip = ERect::from_min_max(pos2(full.left() + 16.0, full.top()), pos2(stop.left() - 8.0, full.bottom()));
        painter.with_clip_rect(text_clip).text(pos2(text_clip.left(), full.center().y), Align2::LEFT_CENTER, t,
                                               FontId::proportional(13.0), Color32::from_rgb(236, 236, 238));
        let mut clicked = 0;
        for (i, (b, label, primary)) in [(stop, tr("Concluir"), true), (cancel, tr("Cancelar"), false)].into_iter().enumerate() {
            let resp = ui.interact(b, ui.scope_id().with(("pano-btn", i)), Sense::click());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
            let hot = resp.hovered();
            let fill = match (primary, hot) {
                (true, true) => Color32::from_rgb(240, 135, 50),
                (true, false) => ORANGE,
                (false, true) => Color32::from_rgb(70, 70, 76),
                (false, false) => Color32::from_rgb(54, 54, 58),
            };
            painter.rect_filled(b, CornerRadius::same(8), fill);
            painter.text(b.center(), Align2::CENTER_CENTER, label, FontId::proportional(12.0), Color32::WHITE);
            if resp.clicked() {
                clicked = i + 1;
            }
        }
        if clicked != 0 {
            self.pano_finish(ctl, env, clicked == 1);
        }
    }

    /// Something of these captures is still going (for the stale state check).
    pub(super) fn scroll_alive(&self) -> bool {
        self.grabbing || self.pano.is_some()
    }
}
