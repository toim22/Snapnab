//! The capture engine (CaptureEngine.cpp, CCaptureEngine, ds_pseudocode §3, comp_1 §4–§7): a state machine that
//! takes a preset from activation to output. Idle → Activated (the capture window hides, an optional countdown) →
//! InputSelecting (the screen is frozen and the selection shows over it) → InputSelected → Capturing → HaveInput
//! (effects) → Outputting (library, clipboard, files, the Editor) → Idle. Each change is logged as
//! "CaptureEngine: A → B", which the e2e suite follows.
//!
//! The windows of a capture (the selection, the countdown, the recognized text) are egui windows of the host; their
//! frames come through `ui`, and the engine moves on when one of them finishes.
//!
//! Not here yet (they come with later steps of phase 5): video recording and its HUD, scrolling, panoramic and
//! extended-window captures, links, pins, QR reading, Office, upload, the quick access overlay, the clean desktop.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Align2, Color32, FontId};
use snapcore::codec::{self, Format, SaveOptions};
use snapcore::image::{Image, Rect, from_colorref};
use snapcore::ipc::{self, Channel, Notify};
use snapcore::library::{self, CaptureInfo};
use snapcore::presets::{CaptureKind, Preset, Selection};
use snapcore::wide::to_wide;
use snapcore::xml::Node;
use snapcore::links::{self, CapturedLink};
use snapcore::{clip, cv, fx, gdip, hdr, i18n, instance, log, ocr, paths, reg, screen, sounds};
use snapnab_app::select::{self, Mode, Outcome, Overlay, Selected, Setup, WinInfo};
use snapnab_app::tr;
use snapui::host::{Caught, Ctl, RawMsg, WinKey, WindowSpec};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{CreateRoundRectRgn, SetWindowRgn};
use windows::Win32::UI::Input::KeyboardAndMouse::{MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey, VK_ESCAPE};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    ASFW_ANY, AllowSetForegroundWindow, FindWindowW, GWL_EXSTYLE, GetWindowLongW, HWND_TOPMOST, MB_ICONWARNING,
    MessageBoxW, SW_SHOWNOACTIVATE, SW_SHOWNORMAL, SWP_NOACTIVATE, SWP_NOSIZE, SetForegroundWindow, SetWindowDisplayAffinity,
    SetWindowLongW, SetWindowPos, ShowWindow, WDA_EXCLUDEFROMCAPTURE, WM_DPICHANGED, WM_HOTKEY, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW,
};
use windows::core::{PCWSTR, w};

use crate::{VERSION, tray};

mod scroll;
mod video;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Idle,
    Activated,
    InputSelecting,
    InputSelected,
    Capturing,
    HaveInput,
    Outputting,
    /// a paused recording
    Suspended,
}

/// Sent by the engine's own timers.
pub enum EngineEv {
    /// the capture window had time to disappear: the screen can be frozen
    BeginInput,
    /// the recorder's disk and error notes, from its threads
    RecorderNote(snapcore::recorder::Note),
    /// the answer to the restart question (its own thread)
    Restart { yes: bool, dont_ask: bool, was_recording: bool },
    /// the scrolling or extended window capture finished on its thread (empty: canceled)
    Grabbed(Image, CaptureInfo),
}

/// What the engine needs of the capture app.
pub struct Env<'a> {
    /// the capture window (clipboard owner, tray balloons)
    pub hwnd: isize,
    pub main: Option<WinKey>,
    pub ipc: Option<&'a Notify>,
}

impl Env<'_> {
    fn notify(&self, title: &str, text: &str) {
        tray::notify(self.hwnd, title, text);
    }
}

/// Layers stored in the library instead of the plain image: windows as objects, the cursor as an object.
pub struct Layers {
    pub base: Image,
    pub objects: Node,
    pub embedded: BTreeMap<String, Image>,
}

struct OverlayWin {
    key: WinKey,
    hwnd: isize,
    o: Overlay,
}

struct DelayWin {
    key: WinKey,
    start: Instant,
    secs: i32,
}

struct TextWin {
    key: WinKey,
    text: String,
}

const SELECT_HOTKEY: i32 = 3;
const SELECT_CLASS: &str = "SnapnabSelectWindowClass";

fn wtext(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn text(w: &[u16]) -> String {
    String::from_utf16_lossy(w)
}

fn dword(name: &str, def: u32) -> u32 {
    reg::get_dword(OsStr::new(name), def, None)
}

/// Options › After capture › When done (AfterCaptureMode): 0 the Editor, 1 the overlay, 2 both, 3 only the clipboard.
pub(crate) fn after_capture_mode() -> u32 {
    dword("AfterCaptureMode", 0).min(3)
}

/// DefaultSaveDir: Options › Output folder, or Pictures\Snapnab.
pub(crate) fn default_save_dir() -> String {
    if let Some(d) = reg::get_string(OsStr::new("OutputFolder"), None).filter(|d| !d.is_empty()) {
        return d.to_string_lossy().into_owned();
    }
    use windows::Win32::UI::Shell::{FOLDERID_Pictures, KNOWN_FOLDER_FLAG, SHGetKnownFolderPath};
    let pics = unsafe { SHGetKnownFolderPath(&FOLDERID_Pictures, KNOWN_FOLDER_FLAG(0), None) }
        .ok()
        .and_then(|p| unsafe { p.to_string() }.ok())
        .unwrap_or_default();
    format!("{pics}\\Snapnab")
}

/// SaveImageFile: the format from the extension (PNG when unknown), JPEG at 90.
fn save_image(path: &str, img: &Image) -> bool {
    let ext = Path::new(path).extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    codec::save_file(Path::new(path), img, Format::from_ext(&ext), &SaveOptions { jpeg_quality: 90, ..SaveOptions::from_settings() },
                     VERSION)
}

pub(crate) fn message_box(text: String, title: String) {
    std::thread::spawn(move || unsafe {
        let (t, c) = (to_wide(&text), to_wide(&title));
        MessageBoxW(None, PCWSTR(t.as_ptr()), PCWSTR(c.as_ptr()), MB_ICONWARNING);
    });
}

/// The selection window's own messages: Esc as a hotkey (it works even when another window has the focus), and the
/// DPI change winit would answer by resizing the window (it must keep covering the virtual screen).
fn select_intercept(hwnd: isize, msg: u32, wp: usize, lp: isize) -> Caught {
    match msg {
        WM_HOTKEY => Caught::Queue(RawMsg { hwnd, msg, wparam: wp, lparam: lp, text: None }, 0),
        WM_DPICHANGED => Caught::Handled(0),
        _ => Caught::Pass,
    }
}

fn child_at(top: isize, x: i32, y: i32) -> Option<(isize, Rect)> {
    screen::child_at(top, x, y).map(|c| (c, screen::window_rect(c)))
}

pub struct Engine {
    /// the selection only picks an area for the interval capture (nothing is captured)
    pick_only: bool,
    /// where the last capture was on the screen (a pinned capture opens there)
    origin: Option<(i32, i32)>,
    state: State,
    since: Instant,
    preset: Preset,
    frozen: Image,
    vs: Rect,
    /// the cursor kept apart (Options › cursor as an object, C25) and its screen position
    cursor: Option<(Image, (i32, i32))>,
    /// the links of web pages and documents under the captured area (F32), in screen coordinates
    links: Vec<CapturedLink>,
    overlay: Option<OverlayWin>,
    delay: Option<DelayWin>,
    texts: Vec<TextWin>,
    video: video::Video,
    /// a scrolling or extended window capture runs on its thread
    grabbing: bool,
    pano: Option<scroll::Pano>,
}

impl Engine {
    pub fn new() -> Engine {
        Engine {
            pick_only: false,
            origin: None,
            state: State::Idle,
            since: Instant::now(),
            preset: Preset::default(),
            frozen: Image::default(),
            vs: Rect::default(),
            cursor: None,
            links: Vec::new(),
            overlay: None,
            delay: None,
            texts: Vec::new(),
            video: video::Video::default(),
            grabbing: false,
            pano: None,
        }
    }

    pub fn busy(&self) -> bool {
        self.state != State::Idle
    }

    fn set_state(&mut self, s: State) {
        if s == self.state {
            return;
        }
        log::write(&format!("CaptureEngine: {:?} → {:?}", self.state, s));
        self.state = s;
        self.since = Instant::now();
        if s == State::Idle {
            crate::desk::clean_end(); // desktop icons and notifications back as they were
        }
    }

    /// Something of the capture still on the screen, or a capture being processed (outputs, effects).
    fn ui_alive(&self, ctl: &Ctl) -> bool {
        self.overlay.as_ref().is_some_and(|o| ctl.exists(o.key))
            || self.delay.as_ref().is_some_and(|d| ctl.exists(d.key))
            || self.video_active()
            || self.scroll_alive()
            || self.since.elapsed() < Duration::from_secs(15)
    }

    /// The entry point (hotkey, button, tray, command line, the Editor).
    /// The interval capture's region or window, chosen with the crosshair; the answer is `UiEv::IntervalArea`.
    pub fn pick_area(&mut self, ctl: &mut Ctl, env: &Env, window: bool) -> bool {
        let mut p = Preset::default();
        p.kind = CaptureKind::Image;
        p.selection = if window { Selection::Window } else { Selection::Region };
        p.name = "interval".into();
        self.pick_only = true;
        let ok = self.activate(ctl, env, &p);
        if !ok {
            self.pick_only = false;
        }
        ok
    }

    pub fn activate(&mut self, ctl: &mut Ctl, env: &Env, preset: &Preset) -> bool {
        if self.state != State::Idle {
            if self.ui_alive(ctl) {
                log::write(&format!("CaptureEngine: busy ({:?}), activation ignored", self.state));
                return false;
            }
            // nothing of a capture left on the screen: a capture that ended without coming back to Idle would block
            // every new one until Snapnab restarted
            log::write(&format!("CaptureEngine: stale state {:?} with nothing on the screen, reset", self.state));
            self.close_windows(ctl);
            self.frozen = Image::default();
            self.cursor = None;
            self.set_state(State::Idle);
        }
        self.preset = preset.clone();
        log::write(&format!("CaptureEngine: preset \"{}\"{}", self.preset.name,
                            if self.preset.effects.any() { " with effects" } else { "" }));
        self.set_state(State::Activated);
        // the screen-based selections: the desktop icons hidden before the screen is frozen (C16)
        if !self.pick_only && !matches!(self.preset.selection, Selection::Clipboard | Selection::File) {
            crate::desk::clean_begin(self.preset.kind == CaptureKind::Video);
        }
        if let Some(w) = env.main.and_then(|k| ctl.window(k))
            && w.is_visible() == Some(true)
        {
            w.set_visible(false);
        }
        let mut delay = if self.preset.kind == CaptureKind::Image { self.preset.delay_seconds } else { 0 };
        if self.preset.selection == Selection::Menu && delay == 0 {
            delay = 3; // time to open the menu
        }
        if delay > 0 {
            self.show_delay(ctl, delay);
        } else {
            go(ctl);
        }
        true
    }

    fn close_windows(&mut self, ctl: &mut Ctl) {
        if let Some(o) = self.overlay.take() {
            unsafe {
                let _ = UnregisterHotKey(Some(HWND(o.hwnd as _)), SELECT_HOTKEY);
            }
            ctl.close(o.key);
        }
        if let Some(d) = self.delay.take() {
            ctl.close(d.key);
        }
    }

    pub fn event(&mut self, ctl: &mut Ctl, env: &Env, ev: EngineEv) {
        match ev {
            EngineEv::BeginInput => {
                if self.state == State::Activated {
                    self.begin_input(ctl, env);
                }
            }
            EngineEv::RecorderNote(n) => self.on_recorder_note(ctl, env, n),
            EngineEv::Restart { yes, dont_ask, was_recording } => self.restart_answer(yes, dont_ask, was_recording),
            EngineEv::Grabbed(img, info) => self.grabbed(ctl, env, img, info),
        }
    }

    // ------------------------------------------------------------ countdown
    fn show_delay(&mut self, ctl: &mut Ctl, secs: i32) {
        let spec = WindowSpec {
            class: "SnapnabDelay",
            title: "Snapnab".into(),
            size: (220.0, 54.0),
            visible: false,
            decorations: false,
            resizable: false,
            topmost: true,
            skip_taskbar: true,
            clear_color: [28.0 / 255.0, 28.0 / 255.0, 30.0 / 255.0, 1.0],
            ..Default::default()
        };
        let Some(k) = ctl.create(&spec) else {
            go(ctl);
            return;
        };
        let h = HWND(ctl.hwnd(k) as _);
        if let Some(win) = ctl.window(k) {
            // bottom right of the work area under the cursor, not activated, rounded, left out of captures
            let (cx, cy) = screen::cursor_pos();
            let wa = screen::monitor_rect(cx, cy, true);
            let sz = win.outer_size();
            let (ww, wh) = (sz.width as i32, sz.height as i32);
            let m = (16.0 * win.scale_factor()) as i32;
            let r = (14.0 * win.scale_factor()) as i32;
            unsafe {
                let _ = SetWindowPos(h, Some(HWND_TOPMOST), wa.right - ww - m, wa.bottom - wh - m, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
                let ex = GetWindowLongW(h, GWL_EXSTYLE) as u32 | WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0;
                SetWindowLongW(h, GWL_EXSTYLE, ex as i32);
                SetWindowRgn(h, Some(CreateRoundRectRgn(0, 0, ww + 1, wh + 1, r, r)), false);
                let _ = SetWindowDisplayAffinity(h, WDA_EXCLUDEFROMCAPTURE);
                let _ = ShowWindow(h, SW_SHOWNOACTIVATE);
            }
        }
        self.delay = Some(DelayWin { key: k, start: Instant::now(), secs });
    }

    fn delay_ui(&mut self, ctl: &mut Ctl, ui: &mut egui::Ui) {
        let Some(d) = &self.delay else { return };
        let el = d.start.elapsed().as_secs_f64();
        let left = d.secs - el.floor() as i32;
        if left <= 0 {
            let k = d.key;
            self.delay = None;
            ctl.close(k);
            go(ctl);
            return;
        }
        let r = ui.max_rect();
        ui.painter().rect_filled(r, 0.0, Color32::from_rgb(28, 28, 30));
        ui.painter().text(r.center(), Align2::CENTER_CENTER, i18n::fill(&tr("Captura em %d s…"), &[&left]),
                          FontId::proportional(16.0), Color32::WHITE);
        ui.ctx().request_repaint_after(Duration::from_secs_f64(1.0 - el.fract()));
    }

    // ------------------------------------------------------------ input
    /// FreezeScreen: a copy of a screen area (tone-mapped on HDR monitors), with the cursor drawn in or kept apart.
    fn freeze(&mut self, r: Rect, with_cursor: bool) -> Image {
        let mut img = screen::grab(r);
        if hdr::enabled() && hdr::is_hdr_area(screen::to_win(r)) {
            hdr::fix_capture(screen::to_win(r), &mut img); // HDR monitors (C02)
        }
        if with_cursor && dword("CursorAsObject", 0) != 0 {
            // kept apart: input_captured makes it an object (or draws it in when effects change the geometry)
            self.cursor = screen::cursor_image();
            return img;
        }
        if with_cursor {
            screen::draw_cursor(&mut img, (r.left, r.top));
        }
        img
    }

    fn input_canceled(&mut self) {
        self.pick_only = false;
        self.frozen = Image::default();
        self.cursor = None;
        self.links.clear();
        self.set_state(State::Idle);
    }

    fn begin_input(&mut self, ctl: &mut Ctl, env: &Env) {
        self.set_state(State::InputSelecting);
        self.cursor = None;
        self.links.clear();
        let p = self.preset.clone();
        let video = p.kind == CaptureKind::Video;
        self.vs = screen::virtual_screen();
        if video {
            let (cx, cy) = screen::cursor_pos();
            let fixed = p.selection == Selection::FixedRegion || (p.selection == Selection::Region && p.region_fixed);
            let direct = if p.audio_only {
                // audio only (C32): no area, the HUD on the monitor of the cursor
                Some(screen::monitor_rect(cx, cy, false))
            } else if fixed && p.region_use_pos {
                Some(Rect::new(p.region_x, p.region_y, p.region_x + p.fixed_w, p.region_y + p.fixed_h).intersect(&self.vs))
            } else if p.selection == Selection::FullScreen {
                Some(self.vs)
            } else if p.selection == Selection::Monitor {
                Some(screen::monitor_rect(cx, cy, false))
            } else {
                None
            };
            if let Some(r) = direct {
                if r.width() < 2 || r.height() < 2 {
                    self.input_canceled();
                    return;
                }
                self.start_video_hud(ctl, env, r);
                return;
            }
        }
        let mut info = CaptureInfo { mode: wtext(&p.selection.name()), ..Default::default() };
        let fixed_region = p.selection == Selection::FixedRegion || (p.selection == Selection::Region && p.region_fixed);
        if fixed_region && p.region_use_pos {
            // a fixed region with a position: the input is ready without a selection (INPUT_REGION_FIXED, comp_1 §6)
            let r = Rect::new(p.region_x, p.region_y, p.region_x + p.fixed_w, p.region_y + p.fixed_h).intersect(&self.vs);
            if r.is_empty() {
                env.notify("Snapnab", &tr("A região fixa está fora da tela. Ajuste a posição na engrenagem da Seleção."));
                self.input_canceled();
                return;
            }
            info.mode = wtext(&tr("Região fixa"));
            self.set_state(State::Capturing);
            let img = self.freeze(r, p.capture_cursor);
            self.input_captured(ctl, env, img, info, Some((r.left, r.top)));
            return;
        }
        match p.selection {
            Selection::FullScreen | Selection::Monitor => {
                let (cx, cy) = screen::cursor_pos();
                let r = if p.selection == Selection::FullScreen { self.vs } else { screen::monitor_rect(cx, cy, false) };
                self.set_state(State::Capturing);
                let img = self.freeze(r, p.capture_cursor);
                let links = if p.capture_links { links::find_links(r, 900) } else { Vec::new() };
                if p.window_objects {
                    // each window as a separate object over the desktop background (the outputs get the screen as it was)
                    let plain = self.freeze(r, false);
                    if let Some(mut ly) = capture_windows_as_objects(r, &plain) {
                        sounds::play_capture_sound();
                        for l in &links {
                            add_link(&mut ly.objects, l.rect.offset(-r.left, -r.top), &l.url);
                        }
                        info.mode = wtext(&tr("Janelas como objetos"));
                        self.set_state(State::Outputting);
                        self.output(ctl, env, &img, &info, Some(ly));
                        self.set_state(State::Idle);
                        return;
                    }
                }
                self.links = links;
                self.input_captured(ctl, env, img, info, Some((r.left, r.top)));
                return;
            }
            Selection::Clipboard => {
                let Some(img) = clip::get_image() else {
                    env.notify("Snapnab", &tr("A área de transferência não contém imagem."));
                    self.input_canceled();
                    return;
                };
                info.application = wtext(&tr("Área de transferência"));
                self.input_captured(ctl, env, img, info, None);
                return;
            }
            Selection::File => {
                let Some(path) = open_file_dialog(&tr("Capturar de arquivo gráfico")) else {
                    self.input_canceled();
                    return;
                };
                let Ok(img) = codec::load(Path::new(&path)) else {
                    message_box(tr("Não foi possível abrir a imagem."), "Snapnab".into());
                    self.input_canceled();
                    return;
                };
                info.window_title = wtext(Path::new(&path).file_name().map(|f| f.to_string_lossy()).as_deref().unwrap_or(""));
                self.input_captured(ctl, env, img, info, None);
                return;
            }
            Selection::Menu => {
                self.set_state(State::Capturing);
                if let Some(img) = self.capture_menus() {
                    self.input_captured(ctl, env, img, info, None);
                    return;
                }
                env.notify("Snapnab", &tr("Nenhum menu aberto encontrado; selecione a região."));
                // falls back to the region selection
            }
            _ => {}
        }

        self.frozen = self.freeze(self.vs, false);
        let mut shown = self.frozen.clone();
        if p.capture_cursor {
            shown = self.freeze(self.vs, true);
            self.frozen = shown.clone();
            // cursor as an object: the selection shows it, the frozen copy stays without it
            if let Some((c, pos)) = &self.cursor {
                shown.blit(c, pos.0 - self.vs.left, pos.1 - self.vs.top, true);
            }
        }
        if !video {
            crate::desk::clean_end(); // the frozen copy is taken: the icons can come back
        }
        let mode = match p.selection {
            Selection::Window => Mode::Window,
            Selection::Object => Mode::Object,
            Selection::FixedRegion => Mode::Fixed,
            Selection::Freehand => Mode::Freehand,
            Selection::MultiArea => Mode::MultiArea,
            // scrolling and extended window pick a window; panoramic is a plain region, scrolled after the selection
            Selection::Scrolling | Selection::ExtendedWindow => Mode::PickWindow,
            _ if fixed_region => Mode::Fixed,
            _ => Mode::Region,
        };
        let setup = Setup {
            frozen: Arc::new(shown),
            vs: self.vs,
            mode,
            fixed: (p.fixed_w, p.fixed_h),
            video,
            quick_crop: dword("QuickCrop", 1) != 0,
            windows: screen::top_windows().into_iter().map(|w| WinInfo { hwnd: w.hwnd, rect: w.rect }).collect(),
            monitors: screen::monitors(),
            history: select::region_history(),
            cursor: screen::cursor_pos(),
            child_at: Some(child_at),
        };
        self.open_overlay(ctl, setup);
    }

    fn open_overlay(&mut self, ctl: &mut Ctl, setup: Setup) {
        let vs = setup.vs;
        let spec = WindowSpec {
            class: SELECT_CLASS,
            title: tr("Snapnab — seleção"),
            size: (vs.width() as f32, vs.height() as f32),
            physical_size: true,
            position: Some((vs.left, vs.top)),
            visible: true,
            decorations: false,
            resizable: false,
            topmost: true,
            skip_taskbar: true,
            intercept: Some(select_intercept),
            clear_color: [0.0, 0.0, 0.0, 1.0],
        };
        let Some(k) = ctl.create(&spec) else {
            log::write("Selection: the window could not be created");
            self.input_canceled();
            return;
        };
        let hwnd = ctl.hwnd(k);
        unsafe {
            let h = HWND(hwnd as _);
            // Esc as a hotkey while the crosshair is up (comp_7 §4.1)
            let _ = RegisterHotKey(Some(h), SELECT_HOTKEY, MOD_NOREPEAT, VK_ESCAPE.0 as u32);
            let _ = SetForegroundWindow(h);
        }
        if let Some(w) = ctl.window(k) {
            w.focus_window();
        }
        self.overlay = Some(OverlayWin { key: k, hwnd, o: Overlay::new(setup) });
    }

    fn overlay_done(&mut self, ctl: &mut Ctl, env: &Env, out: Outcome) {
        match out {
            Outcome::RecentCaptures(pt) => self.recent_captures(ctl, env, pt),
            Outcome::Canceled => {
                self.close_windows(ctl);
                self.input_canceled();
            }
            Outcome::Selected(s) => {
                self.close_windows(ctl);
                self.input_selected(ctl, env, s);
            }
        }
    }

    /// H in the crosshair: the last captures; the chosen one opens in the Editor and the selection closes (C18).
    fn recent_captures(&mut self, ctl: &mut Ctl, env: &Env, pt: (i32, i32)) {
        let Some(owner) = self.overlay.as_ref().map(|o| o.hwnd) else { return };
        let dir = library::library_dir();
        let items = library::list(&dir, 10);
        let labels: Vec<String> = items
            .iter()
            .map(|it| {
                let mut l = local_time(it.modified);
                let name = text(&library::name(&dir, &it.path));
                if !name.is_empty() {
                    l += &format!("  —  {name}");
                }
                if it.video {
                    l += &format!("  ({})", tr("vídeo"));
                }
                l
            })
            .collect();
        let empty = tr("A biblioteca está vazia");
        let menu = if items.is_empty() {
            tray::Menu::new(vec![tray::Item::Disabled(&empty)])
        } else {
            tray::Menu::new(labels.iter().enumerate().map(|(i, l)| tray::Item::Cmd(100 + i as u32, l)).collect())
        };
        let id = menu.choose(owner, pt.0, pt.1) as usize;
        if (100..100 + items.len()).contains(&id) {
            let path = text(&items[id - 100].path);
            snapcore::log::write("Selection: finished (canceled)");
            self.close_windows(ctl);
            self.input_canceled();
            deliver_to_editor(env, &path);
        }
    }

    fn input_selected(&mut self, ctl: &mut Ctl, env: &Env, s: Selected) {
        self.set_state(State::InputSelected);
        if self.pick_only {
            self.pick_only = false;
            self.frozen = Image::default();
            self.set_state(State::Idle);
            crate::pins::send(crate::pins::UiEv::IntervalArea(s.rect, if self.preset.selection == Selection::Window { s.window } else { 0 }));
            return;
        }
        let p = self.preset.clone();
        if p.kind == CaptureKind::Video {
            self.frozen = Image::default();
            let r = if s.window != 0 && p.selection == Selection::Window { screen::window_frame_rect(s.window) } else { s.rect };
            let r = r.intersect(&self.vs);
            if r.width() < 2 || r.height() < 2 {
                self.input_canceled();
                return;
            }
            self.start_video_hud(ctl, env, r);
            return;
        }
        // regions recalled with Tab in the crosshair (C18)
        if matches!(p.selection, Selection::Region | Selection::Window | Selection::Object) {
            select::remember_region(s.rect);
        }
        let mut info = CaptureInfo { mode: wtext(&p.selection.name()), ..Default::default() };
        if s.window != 0 {
            info.window_title = wtext(&screen::window_title(s.window));
            info.application = wtext(&screen::window_process_name(s.window));
        }
        self.set_state(State::Capturing);
        match p.selection {
            Selection::Panoramic => return self.start_pano(ctl, s.rect, info),
            Selection::Scrolling | Selection::ExtendedWindow => {
                self.frozen = Image::default();
                return self.grab_on_thread(ctl, info, p.selection == Selection::ExtendedWindow, s.window, s.child);
            }
            _ => {}
        }
        let local = s.rect.offset(-self.vs.left, -self.vs.top);
        // links of web pages and documents under the captured area (found before the screen changes)
        if p.capture_links {
            self.links = links::find_links(s.rect, 900);
        }
        let frozen = std::mem::take(&mut self.frozen);
        let img = match p.selection {
            Selection::Freehand => {
                let mut img = frozen.crop(local);
                let pts: Vec<(i32, i32)> = s.path.iter().map(|&(x, y)| (x - s.rect.left, y - s.rect.top)).collect();
                let m = cv::polygon_mask(img.wi(), img.hi(), &pts);
                for (px, &on) in img.px.iter_mut().zip(m.iter()) {
                    if on == 0 {
                        *px = 0; // outside the outline: transparent
                    }
                }
                img
            }
            Selection::MultiArea => {
                let mut img = Image::sized(local.width(), local.height(), 0);
                for a in &s.rects {
                    let a = a.offset(-self.vs.left, -self.vs.top);
                    img.blit(&frozen.crop(a), a.left - local.left, a.top - local.top, false);
                }
                img
            }
            Selection::Window => {
                let mut img = frozen.crop(local);
                // transparent rounded corners and, optionally, a shadow like the window's own (C06)
                if s.window != 0 && dword("WindowRoundCorners", 1) != 0 {
                    screen::round_window_corners(&mut img, s.window);
                }
                if s.window != 0 && dword("WindowShadow", 0) != 0 {
                    if let Some((c, pos)) = self.cursor.take() {
                        img.blit(&c, pos.0 - s.rect.left, pos.1 - s.rect.top, true);
                    }
                    img = fx::shadow(&img, 0, 14, 45);
                }
                img
            }
            _ => frozen.crop(local),
        };
        if img.is_empty() {
            self.input_canceled();
            return;
        }
        self.input_captured(ctl, env, img, info, Some((s.rect.left, s.rect.top)));
    }

    /// InputCaptured: the preset's effects, the cursor as an object, then the outputs.
    fn input_captured(&mut self, ctl: &mut Ctl, env: &Env, mut img: Image, info: CaptureInfo, origin: Option<(i32, i32)>) {
        self.set_state(State::HaveInput);
        self.origin = origin;
        let o = origin.unwrap_or((0, 0));
        if !matches!(self.preset.selection, Selection::Clipboard | Selection::File) {
            sounds::play_capture_sound(); // C31
        }
        // the captured cursor (C25): an object over the image, or drawn in when an effect changes the geometry
        let effects = self.preset.effects.any();
        let mut cursor = self.cursor.take().map(|(c, pos)| (c, (pos.0 - o.0, pos.1 - o.1)));
        if let Some((c, cp)) = &cursor {
            let cr = Rect::new(cp.0, cp.1, cp.0 + c.wi(), cp.1 + c.hi());
            if cr.intersect(&Rect::new(0, 0, img.wi(), img.hi())).is_empty() {
                cursor = None;
            } else if effects {
                img.blit(c, cp.0, cp.1, true);
                cursor = None;
            }
        }
        let out = fx::apply_capture_effects(&img, &self.preset.effects); // the preset's effects (CEffectSettings*)
        self.set_state(State::Outputting);
        // the links follow the image: scale and border move them; other effects that change the geometry drop them
        let links = std::mem::take(&mut self.links);
        let e = &self.preset.effects;
        let mut objects = Node::new("Objects");
        if !links.is_empty() && !e.trim && !e.rotate && !e.caption && !e.edge && !e.shadow {
            let k = if e.scale { e.scale_percent as f64 / 100.0 } else { 1.0 };
            let off = if e.border { e.border_width } else { 0 };
            let at = |v: i32, o: i32| ((v - o) as f64 * k).round() as i32 + off;
            for l in &links {
                let a = Rect::new(at(l.rect.left, o.0), at(l.rect.top, o.1), at(l.rect.right, o.0), at(l.rect.bottom, o.1));
                let vis = a.intersect(&Rect::new(0, 0, out.wi(), out.hi()));
                if !vis.is_empty() {
                    add_link(&mut objects, vis, &l.url);
                }
            }
        }
        match cursor {
            Some((c, cp)) => {
                objects
                    .add("Image")
                    .set("X1", cp.0.to_string())
                    .set("Y1", cp.1.to_string())
                    .set("X2", (cp.0 + c.wi()).to_string())
                    .set("Y2", (cp.1 + c.hi()).to_string())
                    .set("StreamName", "Cursor.png")
                    .set("PenWidth", "0")
                    .set("Shadow", "0")
                    .set("Name", tr("Cursor do mouse"));
                let mut flat = out.clone(); // the outputs (file, clipboard) get the image as seen, with the cursor
                flat.blit(&c, cp.0, cp.1, true);
                let embedded = BTreeMap::from([("Cursor.png".to_string(), c)]);
                self.output(ctl, env, &flat, &info, Some(Layers { base: out, objects, embedded }));
            }
            None if !objects.children.is_empty() => {
                self.output(ctl, env, &out, &info, Some(Layers { base: out.clone(), objects, embedded: BTreeMap::new() }))
            }
            None => self.output(ctl, env, &out, &info, None),
        }
        self.set_state(State::Idle);
    }

    /// CaptureMenus: the open menus (and the active window's menu bar) cut out of the screen, transparent around.
    fn capture_menus(&mut self) -> Option<Image> {
        let (rects, bar) = screen::open_menus();
        let first = *rects.first()?;
        let u = rects.iter().chain(bar.iter()).fold(first, |u, r| u.union(r));
        let full = self.freeze(u, self.preset.capture_cursor);
        let mut out = Image::sized(u.width(), u.height(), 0);
        for r in rects.iter().chain(bar.iter()) {
            let r = r.offset(-u.left, -u.top);
            out.blit(&full.crop(r), r.left, r.top, false);
        }
        Some(out)
    }

    // ------------------------------------------------------------ output (COutputter)
    /// Grab Text: recognizes the text, copies it and shows it in a window.
    fn grab_text(&mut self, ctl: &mut Ctl, env: &Env, img: &Image) {
        match ocr::run_ocr(img) {
            Err(e) => message_box(format!("{}{}", tr("Não foi possível reconhecer o texto.\n\n"), e.message()),
                                  tr("Snapnab — Capturar texto")),
            Ok(t) => {
                let t = text(&t);
                if t.trim().is_empty() {
                    env.notify(&tr("Snapnab — Capturar texto"), &tr("Nenhum texto encontrado na captura."));
                    return;
                }
                clip::copy_text(env.hwnd, &t);
                self.show_text(ctl, tr("Capturar texto — copiado para a área de transferência"), t);
            }
        }
    }

    fn show_text(&mut self, ctl: &mut Ctl, title: String, text: String) {
        let spec = WindowSpec { class: "SnapnabTextWnd", title, size: (560.0, 420.0), clear_color: [0.1, 0.1, 0.1, 1.0], ..Default::default() };
        if let Some(k) = ctl.create(&spec) {
            self.texts.push(TextWin { key: k, text });
        }
    }

    pub fn output(&mut self, ctl: &mut Ctl, env: &Env, img: &Image, info: &CaptureInfo, layers: Option<Layers>) {
        let p = self.preset.clone();
        if p.grab_text {
            self.grab_text(ctl, env, img);
        }
        let thumb = gdip::make_thumbnail(img, 160, 120);
        let (base, objects, embedded) = match layers {
            Some(l) => (if l.base.is_empty() { img.clone() } else { l.base }, Some(l.objects), l.embedded),
            None => (img.clone(), None, BTreeMap::new()),
        };
        let lib = library::add(&library::library_dir(), base, &thumb, info, objects, embedded, VERSION).map(|p| text(&p));
        let mut done: Vec<String> = Vec::new();
        // every capture goes to the clipboard (Options › CopyEveryCapture, on by default) besides the preset's own
        // "Copy to the clipboard"; Grab Text and Read QR code keep their text there instead
        let mode = if p.output_clipboard_only { 0 } else { after_capture_mode() };
        let auto_copy = dword("CopyEveryCapture", 1) != 0 || mode == 3;
        if (p.copy_to_clipboard || auto_copy || p.output_clipboard_only) && !p.grab_text && !p.read_qr
            && clip::copy_image(env.hwnd, img, VERSION)
        {
            done.push(tr("copiada para a área de transferência"));
        }
        if !p.output_path.is_empty() {
            // command line /output "file"
            if let Some(d) = Path::new(&p.output_path).parent() {
                let _ = std::fs::create_dir_all(d);
            }
            if save_image(&p.output_path, img) {
                done.push(format!("{}{}", tr("salva em "), p.output_path));
            } else {
                log::write(&format!("Output: failed to save {}", p.output_path));
            }
        }
        if p.read_qr {
            snapui::qr::show(img.clone());
        }
        if p.pin_to_screen {
            let t = if info.window_title.is_empty() { text(&info.mode) } else { text(&info.window_title) };
            crate::pins::pin(img.clone(), &t, self.origin.take());
        }
        if p.office >= 0 {
            crate::share::office(img.clone(), p.office);
        }
        if p.upload {
            let title = if info.window_title.is_empty() { String::new() } else { text(&info.window_title) };
            crate::share::upload(env.hwnd, img.clone(), lib.clone().unwrap_or_default(), title);
        }
        let mut saved = String::new();
        if p.save_to_file {
            let folder = if p.folder.is_empty() { default_save_dir() } else { p.folder.clone() };
            let _ = std::fs::create_dir_all(&folder);
            // the default prefix follows the language; one the user typed is kept as is
            let prefix = if p.file_prefix == "Captura" { tr("Captura") } else { p.file_prefix.clone() };
            saved = text(&library::next_auto_name(&wtext(&folder), &wtext(&prefix), &wtext(p.file_format.ext()), 3));
            if save_image(&saved, img) {
                done.push(format!("{}{saved}", tr("salva em ")));
            } else {
                log::write(&format!("Output: failed to save {saved}"));
                saved.clear();
            }
        }
        if p.open_in_program && !p.program_path.is_empty() {
            let mut file = saved.clone();
            if file.is_empty() {
                file = format!("{}Snapnab_{}.png", std::env::temp_dir().display(), text(&library::time_stamp(false)));
                save_image(&file, img);
            }
            let (prog, arg) = (to_wide(&p.program_path), to_wide(format!("\"{file}\"")));
            unsafe {
                ShellExecuteW(None, w!("open"), PCWSTR(prog.as_ptr()), PCWSTR(arg.as_ptr()), PCWSTR::null(), SW_SHOWNORMAL);
            }
        }
        if let Some(i) = env.ipc {
            i.send_int(Channel::Editor, ipc::code::LIBRARY_CHANGED, 0, false);
        }
        // after the capture: the Editor (as the preset says), the quick access overlay (C08) instead of it, both, or
        // neither (only the clipboard, with the balloon below)
        let other = p.grab_text || p.read_qr || p.pin_to_screen; // outputs that are already the feedback
        let editor = mode == 0 || mode == 2;
        let overlay = mode == 1 || mode == 2;
        if overlay && !other && let Some(l) = &lib {
            let title = if info.window_title.is_empty() { text(&info.mode) } else { text(&info.window_title) };
            crate::quick::show(img, l, false, &title);
        }
        match lib {
            Some(lib) if p.preview_in_editor && editor && !p.output_clipboard_only => deliver_to_editor(env, &lib),
            _ if other || overlay => {}
            _ => {
                let mut t = tr("Captura adicionada à biblioteca");
                for d in &done {
                    t += "; ";
                    t += d;
                }
                env.notify("Snapnab", &(t + "."));
            }
        }
    }

    // ------------------------------------------------------------ windows of the capture
    pub fn owns(&self, k: WinKey) -> bool {
        self.overlay.as_ref().is_some_and(|o| o.key == k)
            || self.delay.as_ref().is_some_and(|d| d.key == k)
            || self.texts.iter().any(|t| t.key == k)
            || self.video_owns(k)
            || self.pano_owns(k)
    }

    pub fn ui(&mut self, ctl: &mut Ctl, env: &Env, k: WinKey, ui: &mut egui::Ui) {
        if let Some(o) = self.overlay.as_mut().filter(|o| o.key == k) {
            if let Some(out) = o.o.ui(ui) {
                self.overlay_done(ctl, env, out);
            }
        } else if self.delay.as_ref().is_some_and(|d| d.key == k) {
            self.delay_ui(ctl, ui);
        } else if self.video_owns(k) {
            self.video_ui(ctl, env, k, ui);
        } else if self.pano_owns(k) {
            self.pano_ui(ctl, env, ui);
        } else if let Some(i) = self.texts.iter().position(|t| t.key == k) {
            let mut close = false;
            egui::Panel::bottom("buttons").show(ui, |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close = ui.button(tr("Fechar")).clicked();
                });
            });
            egui::CentralPanel::default().show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let mut t = self.texts[i].text.as_str();
                    ui.add(egui::TextEdit::multiline(&mut t).desired_width(f32::INFINITY));
                });
            });
            if close {
                ctl.close(k);
                self.texts.remove(i);
            }
        }
    }

    pub fn close_requested(&mut self, ctl: &mut Ctl, env: &Env, k: WinKey) {
        if self.overlay.as_ref().is_some_and(|o| o.key == k) {
            log::write("Selection: WM_CLOSE");
            self.overlay_done(ctl, env, Outcome::Canceled);
        } else if self.delay.as_ref().is_some_and(|d| d.key == k) {
            // closed from outside: the capture goes on, as when the countdown ends
            self.delay = None;
            ctl.close(k);
            go(ctl);
        } else if self.video_owns(k) {
            self.video_close_requested(ctl, k);
        } else if self.pano_owns(k) {
            self.pano_close_requested(ctl, env);
        } else {
            self.texts.retain(|t| t.key != k);
            ctl.close(k);
        }
    }

    /// The Esc hotkey of the selection.
    pub fn message(&mut self, ctl: &mut Ctl, env: &Env, m: &RawMsg) -> bool {
        if m.msg == WM_HOTKEY && m.wparam == SELECT_HOTKEY as usize && self.overlay.as_ref().is_some_and(|o| o.hwnd == m.hwnd) {
            snapcore::log::write("Selection: finished (canceled)");
            self.overlay_done(ctl, env, Outcome::Canceled);
            return true;
        }
        self.pano_message(ctl, env, m)
    }
}

/// The short wait for the capture window to disappear before the screen is frozen.
fn go(ctl: &Ctl) {
    let w = ctl.waker();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(180));
        w.send(EngineEv::BeginInput);
    });
}

/// DeliverToEditor: the Editor open gets the file through its command line (WM_COPYDATA) and IPC 6 (show); closed,
/// it starts with /Open "<file>".
pub fn deliver_to_editor(env: &Env, path: &str) {
    let exe = paths::exe_dir().join("SnapnabEditor.exe");
    let cmd = format!("\"{}\" /Open \"{path}\"", exe.display());
    let ed = unsafe { FindWindowW(w!("SnapnabEditor"), PCWSTR::null()) }.ok().filter(|h| !h.0.is_null());
    unsafe {
        let _ = AllowSetForegroundWindow(ASFW_ANY);
    }
    if let Some(h) = ed
        && instance::send_command_line(h, &cmd)
    {
        if let Some(i) = env.ipc {
            i.send_int(Channel::Editor, ipc::code::SHOW_EDITOR, 1, false);
        }
        return;
    }
    instance::launch_sibling("SnapnabEditor.exe", &format!("/Open \"{path}\""), false);
}

/// CaptureWindowsAsObjects (F31): the desktop background with the taskbars, and each window as an image object (even
/// the parts other windows cover), from the back to the front, named after its title.
/// A `<Link>` object: a clickable area of the image with its URL.
fn add_link(objects: &mut Node, r: Rect, url: &str) {
    objects
        .add("Link")
        .set("X1", r.left.to_string())
        .set("Y1", r.top.to_string())
        .set("X2", r.right.to_string())
        .set("Y2", r.bottom.to_string())
        .set("Url", url);
}

fn capture_windows_as_objects(area: Rect, screen_img: &Image) -> Option<Layers> {
    let (wins, bars) = screen::windows_for_objects(area);
    if wins.is_empty() {
        return None;
    }
    let (w, h) = (area.width(), area.height());
    let desk = screen::desktop_color();
    let mut base = screen::desktop_background(area, desk);
    if base.wi() != w || base.hi() != h {
        base = Image::sized(w, h, 0xFF00_0000 | from_colorref(desk));
    }
    for b in bars {
        let vis = screen::window_rect(b).intersect(&area);
        if vis.is_empty() {
            continue;
        }
        let vis = vis.offset(-area.left, -area.top);
        base.blit(&screen_img.crop(vis), vis.left, vis.top, false);
    }
    let mut objects = Node::new("Objects");
    let mut embedded = BTreeMap::new();
    let mut counter = 1;
    for win in wins.iter().rev() {
        let img = screen::print_window(win.hwnd, win.rect).or_else(|| {
            // the program does not draw that way: what was visible on the screen
            let vis = win.rect.intersect(&area);
            (!vis.is_empty()).then(|| screen_img.crop(vis.offset(-area.left, -area.top)))
        });
        let Some(mut img) = img.filter(|i| !i.is_empty()) else { continue };
        screen::round_window_corners(&mut img, win.hwnd);
        let stream = format!("Image_{counter}.png");
        counter += 1;
        embedded.insert(stream.clone(), img);
        let mut title = screen::window_title(win.hwnd);
        if title.is_empty() {
            title = screen::window_process_name(win.hwnd);
        }
        objects
            .add("Image")
            .set("X1", (win.rect.left - area.left).to_string())
            .set("Y1", (win.rect.top - area.top).to_string())
            .set("X2", (win.rect.right - area.left).to_string())
            .set("Y2", (win.rect.bottom - area.top).to_string())
            .set("StreamName", stream)
            .set("PenWidth", "0")
            .set("Shadow", "0")
            .set("Name", title);
    }
    log::write(&format!("Windows as objects: {} window(s)", counter - 1));
    (counter > 1).then_some(Layers { base, objects, embedded })
}

/// "dd/mm hh:mm" of a FILETIME in local time.
pub(crate) fn local_time(ft: u64) -> String {
    use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
    let f = FILETIME { dwLowDateTime: ft as u32, dwHighDateTime: (ft >> 32) as u32 };
    let (mut st, mut lt) = (SYSTEMTIME::default(), SYSTEMTIME::default());
    unsafe {
        let _ = FileTimeToSystemTime(&f, &mut st);
        let _ = SystemTimeToTzSpecificLocalTime(None, &st, &mut lt);
    }
    format!("{:02}/{:02} {:02}:{:02}", lt.wDay, lt.wMonth, lt.wHour, lt.wMinute)
}

/// The Open dialog for an image (ImageOpenFilter).
pub(crate) fn open_file_dialog(title: &str) -> Option<String> {
    use windows::Win32::UI::Controls::Dialogs::{GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_PATHMUSTEXIST, OPENFILENAMEW};
    let filter: Vec<u16> = tr("Imagens e documentos|*.snap;*.png;*.jpg;*.jpeg;*.bmp;*.gif;*.tif;*.tiff;*.ico;*.webp;*.avif;*.heic;*.heif;*.mp4|Documento Snapnab (*.snap)|*.snap|Vídeo MP4 (*.mp4)|*.mp4|Todos os arquivos (*.*)|*.*|")
        .encode_utf16()
        .map(|c| if c == '|' as u16 { 0 } else { c })
        .chain([0])
        .collect();
    let t = to_wide(title);
    let mut buf = vec![0u16; 4096];
    let mut ofn = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: windows::core::PWSTR(buf.as_mut_ptr()),
        nMaxFile: buf.len() as u32,
        lpstrTitle: PCWSTR(t.as_ptr()),
        Flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST,
        ..Default::default()
    };
    if !unsafe { GetOpenFileNameW(&mut ofn) }.as_bool() {
        return None;
    }
    let n = buf.iter().position(|&c| c == 0).unwrap_or(0);
    Some(String::from_utf16_lossy(&buf[..n]))
}
