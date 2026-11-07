//! Automatic interval capture (IntervalCapture.cpp, C24; ShareX dossier 01 · A6 AutoCaptureForm): the full screen,
//! the monitor of the cursor, a region or a window every N seconds (1–86 400), for a set time or until stopped, into a
//! folder with automatic names (<Prefix><number>). The settings are remembered in the registry (Interval*); a red dot
//! on the tray icon shows it is running.

use std::cell::RefCell;
use std::ffi::OsStr;
use std::path::Path;
use std::sync::Once;
use std::time::{Duration, Instant};

use snapcore::image::Rect;
use snapcore::{codec, hdr, library, log, reg, screen};
use snapnab_app::tr;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, HWND_MESSAGE, IsIconic, IsWindow, KillTimer, RegisterClassW, SetTimer, WINDOW_EX_STYLE,
    WM_TIMER, WNDCLASSW, WS_OVERLAPPED,
};
use windows::core::w;

use crate::form::Field;
use crate::pins::{UiEv, send};

const EXT: [&str; 3] = [".png", ".jpg", ".webp"];

/// The settings of a run.
#[derive(Clone, Debug, Default)]
pub struct Settings {
    /// 0 full screen, 1 monitor, 2 region, 3 window
    pub area: i32,
    pub seconds: i32,
    pub minutes: i32,
    pub format: i32,
    pub prefix: String,
    pub folder: String,
    pub cursor: bool,
}

#[derive(Default)]
struct State {
    wnd: isize,
    running: bool,
    count: i32,
    end_at: Option<Instant>,
    s: Settings,
    region: Rect,
    window: isize,
}

thread_local! {
    static S: RefCell<State> = RefCell::new(State::default());
}

fn dword(n: &str, d: u32) -> u32 {
    reg::get_dword(OsStr::new(n), d, None)
}

fn string(n: &str, d: String) -> String {
    reg::get_string(OsStr::new(n), None).map(|s| s.to_string_lossy().into_owned()).filter(|s| !s.is_empty()).unwrap_or(d)
}

/// The remembered settings.
pub fn saved() -> Settings {
    Settings {
        area: dword("IntervalArea", 0).min(3) as i32,
        seconds: dword("IntervalSeconds", 10).clamp(1, 86400) as i32,
        minutes: dword("IntervalMinutes", 0).min(10080) as i32,
        format: dword("IntervalFormat", 0).min(2) as i32,
        prefix: string("IntervalPrefix", tr("Intervalo")),
        folder: string("IntervalFolder", format!("{}\\{}", crate::engine::default_save_dir(), tr("Intervalo"))),
        cursor: dword("IntervalCursor", 0) != 0,
    }
}

fn save(s: &Settings) {
    let d = |n: &str, v: i32| reg::set_dword(OsStr::new(n), v as u32, None);
    d("IntervalArea", s.area);
    d("IntervalSeconds", s.seconds);
    d("IntervalMinutes", s.minutes);
    d("IntervalFormat", s.format);
    d("IntervalCursor", s.cursor as i32);
    reg::set_string(OsStr::new("IntervalPrefix"), OsStr::new(&s.prefix), None);
    reg::set_string(OsStr::new("IntervalFolder"), OsStr::new(&s.folder), None);
}

pub fn running() -> bool {
    S.with(|s| s.borrow().running)
}

pub fn count() -> i32 {
    S.with(|s| s.borrow().count)
}

/// The settings form, on its own thread; OK sends `UiEv::IntervalChosen`.
pub fn setup(owner: isize) {
    let cur = saved();
    std::thread::spawn(move || {
        let mut f = vec![
            Field::Combo {
                label: tr("Área"),
                items: vec![tr("Tela inteira"), tr("Monitor do cursor"), tr("Região (escolher agora)"), tr("Janela (escolher agora)")],
                value: cur.area,
            },
            Field::Int { label: tr("Intervalo (segundos)"), value: cur.seconds, min: 1, max: 86400 },
            Field::Int { label: tr("Duração (minutos, 0 = até parar)"), value: cur.minutes, min: 0, max: 10080 },
            Field::Combo { label: tr("Formato"), items: vec!["PNG".into(), "JPEG".into(), "WebP".into()], value: cur.format },
            Field::Text { label: tr("Prefixo do nome"), text: cur.prefix.clone() },
            Field::Text { label: tr("Pasta"), text: cur.folder.clone() },
            Field::Check { label: tr("Capturar o cursor"), value: cur.cursor },
            Field::Label(tr("Um ponto vermelho no ícone da bandeja indica que está capturando; pare pelo menu da bandeja.")),
        ];
        loop {
            if !crate::form::run(owner, &tr("Captura por intervalo"), &mut f) {
                return;
            }
            let mut s = Settings::default();
            for (i, x) in f.iter().enumerate() {
                match (i, x) {
                    (0, Field::Combo { value, .. }) => s.area = *value,
                    (1, Field::Int { value, .. }) => s.seconds = *value,
                    (2, Field::Int { value, .. }) => s.minutes = *value,
                    (3, Field::Combo { value, .. }) => s.format = *value,
                    (4, Field::Text { text, .. }) => s.prefix = text.clone(),
                    (5, Field::Text { text, .. }) => s.folder = text.trim().to_string(),
                    (6, Field::Check { value, .. }) => s.cursor = *value,
                    _ => {}
                }
            }
            let err = if s.prefix.is_empty() || s.prefix.contains(['\\', '/', ':', '*', '?', '"', '<', '>', '|']) {
                tr("O prefixo contém caracteres inválidos.")
            } else if s.folder.is_empty() {
                tr("Escolha uma pasta.")
            } else {
                String::new()
            };
            if err.is_empty() {
                save(&s);
                send(UiEv::IntervalChosen(s));
                return;
            }
            crate::engine::message_box(err, tr("Captura por intervalo"));
        }
    });
}

unsafe extern "system" fn proc(hwnd: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if m == WM_TIMER {
        tick();
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, m, wp, lp) }
}

/// Starts with `s`; `region` / `window` for the areas chosen with the crosshair.
pub fn begin(s: Settings, region: Rect, window: isize) {
    if running() {
        return;
    }
    if std::fs::create_dir_all(&s.folder).is_err() {
        send(UiEv::Notify("Snapnab".into(), tr("Não foi possível criar a pasta: ") + &s.folder));
        return;
    }
    static REG: Once = Once::new();
    REG.call_once(|| unsafe {
        let wc = WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default().into(),
            lpszClassName: w!("SnapnabInterval"),
            ..Default::default()
        };
        RegisterClassW(&wc);
    });
    let wnd = S.with(|st| st.borrow().wnd);
    let wnd = if wnd != 0 {
        wnd
    } else {
        unsafe {
            CreateWindowExW(WINDOW_EX_STYLE(0), w!("SnapnabInterval"), w!(""), WS_OVERLAPPED, 0, 0, 0, 0, Some(HWND_MESSAGE), None, None,
                            None)
        }
        .map(|h| h.0 as isize)
        .unwrap_or(0)
    };
    log::write(&format!("Interval capture: every {} s, area {}, {} min, {}", s.seconds, s.area, s.minutes, s.folder));
    S.with(|st| {
        let mut st = st.borrow_mut();
        st.wnd = wnd;
        st.running = true;
        st.count = 0;
        st.end_at = (s.minutes > 0).then(|| Instant::now() + Duration::from_secs(s.minutes as u64 * 60));
        st.s = s;
        st.region = region;
        st.window = window;
    });
    unsafe {
        SetTimer(Some(HWND(wnd as _)), 1, 250, None); // the first capture right away
    }
    send(UiEv::IntervalStatus(true, 0));
}

/// The command line /interval: the remembered settings, the screen or the monitor only.
pub fn start_saved() {
    let mut s = saved();
    s.area = s.area.min(1);
    begin(s, Rect::default(), 0);
}

pub fn stop() {
    let (wnd, count, folder) = S.with(|st| {
        let mut st = st.borrow_mut();
        let was = st.running;
        st.running = false;
        (if was { st.wnd } else { 0 }, st.count, st.s.folder.clone())
    });
    if wnd == 0 {
        return;
    }
    unsafe {
        let _ = KillTimer(Some(HWND(wnd as _)), 1);
    }
    log::write(&format!("Interval capture: stopped after {count} capture(s)"));
    send(UiEv::IntervalStatus(false, count));
    send(UiEv::Notify("Snapnab".into(),
                      tr("Captura por intervalo parada: %d imagem(ns) em %s").replacen("%d", &count.to_string(), 1).replacen("%s", &folder, 1)));
}

fn tick() {
    let st = S.with(|st| {
        let st = st.borrow();
        st.running.then(|| (st.wnd, st.end_at, st.s.clone(), st.region, st.window))
    });
    let Some((wnd, end_at, s, region, window)) = st else { return };
    if end_at.is_some_and(|e| Instant::now() >= e) {
        stop();
        return;
    }
    unsafe {
        SetTimer(Some(HWND(wnd as _)), 1, s.seconds as u32 * 1000, None);
    }
    let r = match s.area {
        1 => {
            let (x, y) = screen::cursor_pos();
            screen::monitor_rect(x, y, false)
        }
        2 => region,
        3 => {
            if window == 0 || !unsafe { IsWindow(Some(HWND(window as _))) }.as_bool() {
                stop(); // the window was closed
                return;
            }
            if unsafe { IsIconic(HWND(window as _)) }.as_bool() {
                return; // minimized: skipped this time
            }
            screen::window_frame_rect(window)
        }
        _ => screen::virtual_screen(),
    };
    let r = r.intersect(&screen::virtual_screen());
    if r.is_empty() {
        return;
    }
    let mut img = screen::grab(r);
    if img.is_empty() {
        return;
    }
    if hdr::enabled() && hdr::is_hdr_area(screen::to_win(r)) {
        hdr::fix_capture(screen::to_win(r), &mut img);
    }
    if s.cursor {
        screen::draw_cursor(&mut img, (r.left, r.top));
        img.make_opaque();
    }
    let ext = EXT[s.format.clamp(0, 2) as usize];
    let w = |x: &str| x.encode_utf16().collect::<Vec<u16>>();
    let path = String::from_utf16_lossy(&library::next_auto_name(&w(&s.folder), &w(&s.prefix), &w(ext), 4));
    let ok = codec::save_file(Path::new(&path), &img, codec::Format::from_ext(ext), &codec::SaveOptions::from_settings(), crate::VERSION);
    if ok {
        S.with(|st| st.borrow_mut().count += 1);
    } else {
        log::write(&format!("Interval capture: failed to save {path}"));
    }
    send(UiEv::IntervalStatus(true, count()));
}
