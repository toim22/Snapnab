//! Snapnab.exe, the capture app, in Rust (the C++ src/capture/main.cpp moving over, docs/ARCHITECTURE.md): the window
//! with the presets, the tray icon, the hotkeys, the command line, and the channel to the Editor. One instance runs;
//! a second start hands its command line to it (WM_COPYDATA) and ends.

#![windows_subsystem = "windows"]

mod camwin;
mod desk;
mod dialogs;
mod engine;
use snapui::{form, qr, upload_ui};
mod hotkeys;
mod interval;
mod oneclick;
mod pins;
mod quick;
mod share;
mod rawwin;
mod tray;

use std::ffi::OsStr;
use std::sync::atomic::{AtomicU32, Ordering};

use snapcore::ipc::{self, Channel, Notify};
use snapcore::presets::{CaptureKind, Preset, PresetStore, Selection};
use snapcore::{i18n, instance, log, reg};
use engine::{Engine, EngineEv, Env};
use snapnab_app::window::{self, Action};
use snapnab_app::{cmdline, tr};
use snapui::host::{App, Caught, Ctl, RawMsg, WinKey, WindowSpec};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::DataExchange::{AddClipboardFormatListener, COPYDATASTRUCT, RemoveClipboardFormatListener};
use windows::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
use windows::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    ASFW_ANY, AllowSetForegroundWindow, FindWindowW, IsIconic, KillTimer, MB_ICONINFORMATION,
    MessageBoxW, RegisterWindowMessageW, SW_RESTORE, SW_SHOWNORMAL, SetForegroundWindow, SetTimer, ShowWindow,
    WM_CLIPBOARDUPDATE, WM_COMMAND, WM_CONTEXTMENU, WM_COPYDATA, WM_HOTKEY, WM_LBUTTONDBLCLK, WM_LBUTTONUP,
    WM_RBUTTONUP, WM_TIMER,
};
use windows::core::{PCWSTR, w};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

use snapnab_app::id;

const TRAY_CLICK_TIMER: usize = 0x5E1;
/// the automatic library cleanup (C41): soon after the start, then every 6 h
const CLEANUP_TIMER: usize = 9;
/// NIN_SELECT | NINF_KEY: Enter or Space on the tray icon.
const NIN_KEYSELECT: u32 = 0x401;
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

/// Runs in the window procedure of the capture window: the messages the app answers itself.
fn intercept(hwnd: isize, msg: u32, wp: usize, lp: isize) -> Caught {
    let raw = |text| RawMsg { hwnd, msg, wparam: wp, lparam: lp, text };
    match msg {
        WM_COPYDATA => match unsafe { instance::parse_copy_data(lp as *const COPYDATASTRUCT) } {
            Some(t) => Caught::Queue(raw(Some(t)), 1),
            None => Caught::Handled(0),
        },
        WM_COMMAND | WM_HOTKEY | tray::WM_SNAP_TRAY | WM_CLIPBOARDUPDATE => Caught::Queue(raw(None), 0),
        WM_TIMER if wp == TRAY_CLICK_TIMER || wp == CLEANUP_TIMER => Caught::Queue(raw(None), 0),
        m if m != 0 && m == TASKBAR_CREATED.load(Ordering::Relaxed) => Caught::Queue(raw(None), 0),
        _ => Caught::Pass,
    }
}

struct CaptureApp {
    store: PresetStore,
    main: Option<WinKey>,
    hwnd: isize,
    ipc: Option<Notify>,
    tray_ignore_up: bool,
    engine: Engine,
    last_clip_seq: u32,
    hk: Option<hotkeys::Manager>,
    /// the last capture started, for Repeat (IDM_REPEAT)
    last: Option<Preset>,
    /// the recording keys are registered (only while the video HUD is up)
    video_keys: bool,
    /// the app's event loop, for the worker threads
    waker: Option<snapui::host::Waker>,
    /// the interval capture waiting for its region or window
    pending_interval: Option<interval::Settings>,
    /// what the capture window shows besides the presets
    view: window::View,
}

/// The engine's view of the app (disjoint fields: the engine itself stays borrowable).
macro_rules! env {
    ($s:ident) => {
        Env { hwnd: $s.hwnd, main: $s.main, ipc: $s.ipc.as_ref() }
    };
}

/// The second-instance and IPC events, from other threads.
enum Ev {
    Ipc(ipc::Message),
}

impl CaptureApp {
    fn current(&self) -> Preset {
        self.store.presets.get(self.store.selected as usize).cloned().unwrap_or_default()
    }

    fn show(&self, ctl: &Ctl) {
        let Some(w) = self.main.and_then(|k| ctl.window(k)) else { return };
        w.set_visible(true);
        unsafe {
            let h = HWND(self.hwnd as _);
            if IsIconic(h).as_bool() {
                let _ = ShowWindow(h, SW_RESTORE);
            }
            let _ = SetForegroundWindow(h);
        }
        w.focus_window();
    }

    fn notify(&self, title: &str, text: &str) {
        tray::notify(self.hwnd, title, text);
    }

    fn start_capture(&mut self, ctl: &mut Ctl, p: &Preset) {
        if !self.engine.busy() {
            self.last = Some(p.clone());
        }
        let env = env!(self);
        self.engine.activate(ctl, &env, p);
    }

    /// ToolPreset: a region selection whose only output is the tool (QR, pin, text, upload).
    fn tool_preset(&self, t: cmdline::Tool) -> Preset {
        let mut p = self.store.first_of_kind(CaptureKind::Image);
        p.kind = CaptureKind::Image;
        p.selection = Selection::Region;
        p.region_fixed = false;
        p.delay_seconds = 0;
        p.effects = Default::default();
        (p.preview_in_editor, p.copy_to_clipboard, p.window_objects) = (false, false, false);
        (p.save_to_file, p.open_in_program, p.office) = (false, false, -1);
        p.grab_text = t == cmdline::Tool::Text;
        p.pin_to_screen = t == cmdline::Tool::Pin;
        p.read_qr = t == cmdline::Tool::Qr;
        p.upload = t == cmdline::Tool::Upload;
        p
    }

    /// The presets changed (a form, a menu, an import): saved, the hotkeys registered again, the window redrawn.
    fn presets_changed(&mut self, ctl: &mut Ctl) {
        let n = self.store.presets.len() as i32;
        self.store.selected = self.store.selected.clamp(0, (n - 1).max(0));
        self.store.save();
        self.register_hotkeys();
        if let Some(m) = self.main {
            ctl.repaint(m);
        }
    }

    /// EditPresetHotkey (after its form): a key another Snapnab action uses is refused; one another program owns is
    /// served by the keyboard hook, and the user is told.
    fn set_preset_hotkey(&mut self, ctl: &mut Ctl, index: usize, mods: u32, vk: u32) {
        let Some(p) = self.store.presets.get_mut(index) else { return };
        let old = (p.hotkey_mods, p.hotkey_vk);
        (p.hotkey_mods, p.hotkey_vk) = (mods, vk);
        let r = match self.hk.as_mut() {
            Some(hk) => hk.set(hotkeys::HK_PRESET_BASE + index as i32, mods, vk),
            None => hotkeys::Result::Ok,
        };
        use windows::Win32::UI::WindowsAndMessaging::{MB_ICONINFORMATION, MB_ICONWARNING};
        if r == hotkeys::Result::InternalConflict {
            (p.hotkey_mods, p.hotkey_vk) = old;
            dialogs::message_async(self.hwnd, tr("Esse atalho já está em uso por outra ação do Snapnab."), tr("Conflito de atalho"), MB_ICONWARNING);
            return;
        }
        self.store.save(); // saved before the notice below (it waits for the user)
        if let Some(m) = self.main {
            ctl.repaint(m);
        }
        if r == hotkeys::Result::HookedFallback {
            dialogs::message_async(self.hwnd, tr("Outro programa já usa esse atalho. O Snapnab vai interceptá-lo por um gancho de teclado."),
                                   tr("Atalho"), MB_ICONINFORMATION);
        }
    }

    /// RestartApps: the Editor restarts itself; this app starts a new copy that waits for it to end, then exits.
    fn restart_apps(&mut self, ctl: &mut Ctl) {
        let editor = unsafe { windows::Win32::UI::WindowsAndMessaging::FindWindowW(w!("SnapnabEditor"), PCWSTR::null()) }
            .is_ok_and(|h| !h.0.is_null());
        if editor && let Some(i) = &self.ipc {
            i.send_int(Channel::Editor, ipc::code::RESTART_EDITOR, 0, true);
        }
        if instance::launch_sibling("Snapnab.exe", &instance::restart_args(), false) {
            self.exit(ctl);
        }
    }

    fn library_changed(&self) {
        if let Some(i) = &self.ipc {
            i.send_int(Channel::Editor, ipc::code::LIBRARY_CHANGED, 0, false);
        }
    }

    /// Library › Clean up now: asks, then removes the captures older than CleanupDays with the Options' rules.
    fn cleanup_now(&self) {
        let days = reg::get_dword(OsStr::new("CleanupDays"), 30, None);
        let q = tr("Remover agora as capturas com mais de %u dias (com as regras das Opções)?").replace("%u", &days.to_string());
        let hwnd = self.hwnd;
        let waker = self.waker.clone();
        std::thread::spawn(move || {
            use windows::Win32::UI::WindowsAndMessaging::{IDYES, MB_ICONQUESTION, MB_YESNO};
            let (t, c) = (snapcore::wide::to_wide(&q), snapcore::wide::to_wide("Snapnab"));
            if unsafe { MessageBoxW(Some(HWND(hwnd as _)), PCWSTR(t.as_ptr()), PCWSTR(c.as_ptr()), MB_YESNO | MB_ICONQUESTION) } != IDYES {
                return;
            }
            let g = |n: &str, d: u32| reg::get_dword(OsStr::new(n), d, None);
            let n = snapcore::library::cleanup(&snapcore::library::library_dir(), g("CleanupDays", 30) as i32, g("CleanupKeepNamed", 1) != 0,
                                               g("CleanupKeepTagged", 1) != 0, g("CleanupRecycle", 1) != 0);
            if let Some(w) = waker {
                w.send(pins::UiEv::LibraryChanged);
                w.send(pins::UiEv::Notify("Snapnab".into(), tr("%d captura(s) removida(s) da biblioteca.").replace("%d", &n.to_string())));
            }
        });
    }

    /// PinFromFile: the Open dialog, then the pin.
    fn pin_from_file(&self) {
        let Some(path) = engine::open_file_dialog(&tr("Fixar imagem na tela")) else { return };
        if !pins::pin_file(&path) {
            engine::message_box(tr("Não foi possível abrir a imagem.\n\n") + &path, "Snapnab".into());
        }
    }

    /// RegisterHotkeys: the image and video keys (Print Screen and Shift+Print Screen by default), the tools that
    /// have one set in Options, and each preset's own key.
    fn register_hotkeys(&mut self) {
        use hotkeys::*;
        let Some(hk) = self.hk.as_mut() else { return };
        hk.clear();
        let get = |name: &str, def: u32| unpack(reg::get_dword(OsStr::new(name), def, None));
        let (m, v) = get("ImageHotkey", pack(0, 0x2C));
        hk.set(HK_IMAGE, m, v);
        let (m, v) = get("VideoHotkey", pack(windows::Win32::UI::Input::KeyboardAndMouse::MOD_SHIFT.0, 0x2C));
        hk.set(HK_VIDEO, m, v);
        // tools without a default key (a global key steals the combination from every program): set in Options
        for (id, name) in [(HK_COLOR, "ColorPickerHotkey"), (HK_CAPTURE_PIN, "CapturePinHotkey"),
                           (HK_PIN_CLIP, "PinClipboardHotkey"), (HK_FOCUS, "FocusHotkey")] {
            let (m, v) = get(name, 0);
            if v != 0 {
                hk.set(id, m, v);
            }
        }
        for (i, p) in self.store.presets.iter().enumerate() {
            if p.hotkey_vk == 0 {
                continue;
            }
            if hk.set(HK_PRESET_BASE + i as i32, p.hotkey_mods, p.hotkey_vk) == Result::InternalConflict {
                tray::notify(self.hwnd, &tr("Conflito de atalho"),
                             &tr("O atalho do preset \"%s\" já está em uso por outra ação.").replace("%s", &i18n::tr_str(&p.name)));
            }
        }
        if self.video_keys {
            self.set_video_keys(true);
        }
    }

    /// SetVideoHotkeys: start/pause/resume, stop and zoom, registered only while the HUD is up (they would steal
    /// Shift+F9…F11 from every program otherwise).
    fn set_video_keys(&mut self, on: bool) {
        use hotkeys::*;
        let Some(hk) = self.hk.as_mut() else { return };
        let shift = windows::Win32::UI::Input::KeyboardAndMouse::MOD_SHIFT.0;
        for (id, name, vk) in [(HK_VIDEO_PAUSE, "VideoStartPauseResumeKey", 0x78), (HK_VIDEO_STOP, "VideoStopKey", 0x79),
                               (HK_VIDEO_ZOOM, "VideoZoomKey", 0x7A)] {
            let (m, v) = if on { unpack(reg::get_dword(OsStr::new(name), pack(shift, vk), None)) } else { (0, 0) };
            hk.set(id, m, v);
        }
        self.video_keys = on;
    }

    /// After the engine ran: the recording keys follow the HUD, and volumes changed in its panel go to the preset
    /// (onPresetVolumes: the preset with the same name and kind).
    fn after_engine(&mut self) {
        oneclick::set_busy(self.engine.busy()); // hidden during a capture
        let active = self.engine.video_active();
        if active != self.video_keys {
            self.set_video_keys(active);
        }
        if let Some(v) = self.engine.take_volumes() {
            if let Some(p) = self.store.presets.iter_mut().find(|p| p.name == v.name && p.kind == v.kind) {
                (p.mic_volume, p.sys_volume, p.auto_gain) = (v.mic_volume, v.sys_volume, v.auto_gain);
                self.store.save();
            }
        }
    }

    fn on_hotkey(&mut self, ctl: &mut Ctl, id: i32) {
        use hotkeys::*;
        match id {
            HK_IMAGE => self.start_capture(ctl, &self.store.first_of_kind(CaptureKind::Image)),
            HK_VIDEO => {
                if self.engine.recording() {
                    let env = env!(self);
                    self.engine.video_stop(ctl, &env);
                } else {
                    self.start_capture(ctl, &self.store.first_of_kind(CaptureKind::Video));
                }
            }
            HK_VIDEO_PAUSE => {
                let env = env!(self);
                self.engine.video_start_pause_resume(ctl, &env);
            }
            HK_VIDEO_STOP => {
                let env = env!(self);
                self.engine.video_stop(ctl, &env);
            }
            HK_VIDEO_ZOOM => self.engine.video_toggle_zoom(),
            HK_COLOR => desk::run_color_picker(),
            HK_FOCUS => desk::focus_toggle(),
            HK_CAPTURE_PIN => self.start_capture(ctl, &self.tool_preset(cmdline::Tool::Pin)),
            HK_PIN_CLIP => {
                pins::pin_clipboard();
            }
            i if i >= HK_PRESET_BASE && ((i - HK_PRESET_BASE) as usize) < self.store.presets.len() => {
                let p = self.store.presets[(i - HK_PRESET_BASE) as usize].clone();
                self.start_capture(ctl, &p);
            }
            _ => log::write(&format!("Hotkey {id}: not in the Rust app yet")),
        }
    }

    /// The clipboard monitor (A13): an image copied by another program (only the Snipping Tool by default) opens in
    /// the Editor as a new image.
    fn on_clipboard_update(&mut self) {
        use windows::Win32::System::DataExchange::{GetClipboardOwner, GetClipboardSequenceNumber};
        let mode = reg::get_dword(OsStr::new("ClipboardMonitor"), 1, None);
        let seq = unsafe { GetClipboardSequenceNumber() };
        if mode == 0 || seq == self.last_clip_seq || self.engine.busy() {
            return;
        }
        self.last_clip_seq = seq;
        if !snapcore::clip::has_image() {
            return;
        }
        let owner = unsafe { GetClipboardOwner() }.map(|h| h.0 as isize).unwrap_or(0);
        if owner != 0 && snapcore::screen::window_pid(owner) == std::process::id() {
            return;
        }
        let proc = if owner != 0 { snapcore::screen::window_process_name(owner).to_lowercase() } else { String::new() };
        if proc == "snapnabeditor.exe" {
            return;
        }
        let snip = matches!(proc.as_str(), "snippingtool.exe" | "screenclippinghost.exe" | "screensketch.exe");
        if mode == 1 && !snip {
            return;
        }
        log::write(&format!("Clipboard: image detected from '{proc}'"));
        // the Editor creates the new image from the clipboard (IPC 0x1E, ed_comp_14 §3.4)
        if ipc::mutex_exists(&snapcore::wide::to_wide(ipc::EDITOR_MUTEX)) {
            if let Some(i) = &self.ipc {
                i.send_int(Channel::Editor, ipc::code::NEW_IMAGE_FROM_CLIPBOARD, 0, false);
            }
        } else {
            instance::launch_sibling("SnapnabEditor.exe", "/NewFromClipboard", false);
        }
    }

    fn open_editor(&self) {
        let ed = unsafe { FindWindowW(w!("SnapnabEditor"), PCWSTR::null()) }.ok().filter(|h| !h.0.is_null());
        match ed {
            Some(h) => unsafe {
                let _ = AllowSetForegroundWindow(ASFW_ANY);
                if let Some(i) = &self.ipc {
                    i.send_int(Channel::Editor, ipc::code::SHOW_EDITOR, 1, false);
                }
                if IsIconic(h).as_bool() {
                    let _ = ShowWindow(h, SW_RESTORE);
                }
                let _ = SetForegroundWindow(h);
            },
            None => {
                instance::launch_sibling("SnapnabEditor.exe", "", false);
            }
        }
    }

    fn exit(&mut self, ctl: &mut Ctl) {
        let env = env!(self);
        if !self.engine.confirm_exit(ctl, &env, self.hwnd) {
            return;
        }
        unsafe {
            let _ = RemoveClipboardFormatListener(HWND(self.hwnd as _));
        }
        self.hk = None; // unregisters the keys and removes the hook
        tray::remove(self.hwnd);
        if let Some(i) = &self.ipc {
            i.send_int(Channel::Editor, ipc::code::CLOSE_EDITOR_IF_HIDDEN, 0, false);
        }
        self.ipc = None;
        ctl.exit();
    }

    fn command(&mut self, ctl: &mut Ctl, cmd: u32) {
        let n = self.store.presets.len() as u32;
        if (id::PRESET_BASE..id::PRESET_BASE + n).contains(&cmd) {
            let p = self.store.presets[(cmd - id::PRESET_BASE) as usize].clone();
            self.start_capture(ctl, &p);
            return;
        }
        match cmd {
            id::SHOW => self.show(ctl),
            id::EXIT => self.exit(ctl),
            id::OPEN_EDITOR | id::IDC_OPEN_EDITOR => self.open_editor(),
            id::CAPTURE_IMAGE => self.start_capture(ctl, &self.store.first_of_kind(CaptureKind::Image)),
            id::CAPTURE_VIDEO => self.start_capture(ctl, &self.store.first_of_kind(CaptureKind::Video)),
            id::IDC_CAPTURE => self.start_capture(ctl, &self.current()),
            id::CAPTURE_CLIPBOARD => {
                let mut p = self.store.first_of_kind(CaptureKind::Image);
                p.selection = Selection::Clipboard;
                p.delay_seconds = 0;
                self.start_capture(ctl, &p);
            }
            id::REPEAT => {
                let p = self.last.clone().unwrap_or_else(|| self.current());
                self.start_capture(ctl, &p);
            }
            id::LIBRARY_FOLDER => shell_open(&String::from_utf16_lossy(&snapcore::library::library_dir())),
            id::LOGS => shell_open(&snapcore::paths::logs_dir().to_string_lossy()),
            id::ABOUT => about(),
            id::OPTIONS => dialogs::options(self.hwnd),
            id::PRESET_NEW => dialogs::preset_new(self.hwnd, &self.store, &self.current()),
            id::PRESET_FROM_CURRENT => dialogs::preset_from_current(self.hwnd, &self.current()),
            id::PRESET_RENAME => dialogs::preset_rename(self.hwnd, self.store.selected.max(0) as usize, &self.current().name),
            id::PRESET_DUP => {
                dialogs::preset_duplicate(&mut self.store);
                self.presets_changed(ctl);
            }
            id::PRESET_DEL => dialogs::preset_delete(self.hwnd, &self.store),
            id::PRESET_HOTKEY => dialogs::preset_hotkey(self.hwnd, self.store.selected.max(0) as usize, &self.current()),
            id::PRESET_HELP => dialogs::preset_help(self.hwnd),
            id::PRESET_RUN => self.start_capture(ctl, &self.current()),
            id::PRESET_EXPORT => dialogs::presets_export(self.hwnd, &self.store),
            id::PRESET_IMPORT => dialogs::presets_import(self.hwnd),
            id::SETTINGS_EXPORT => {
                self.store.save();
                dialogs::settings_export(self.hwnd);
            }
            id::SETTINGS_IMPORT => dialogs::settings_import(self.hwnd, &self.store),
            id::SETTINGS_BACKUPS => {
                let d = snapcore::settings::backup_dir();
                let _ = std::fs::create_dir_all(&d);
                shell_open(&d.to_string_lossy());
            }
            id::UPLOAD_SETTINGS => upload_ui::configure(self.hwnd),
            id::IDC_TAB_IMAGE | id::IDC_TAB_VIDEO => {
                let k = if cmd == id::IDC_TAB_VIDEO { CaptureKind::Video } else { CaptureKind::Image };
                let i = self.store.selected.max(0) as usize;
                if let Some(p) = self.store.presets.get_mut(i)
                    && p.kind != k
                {
                    window::set_kind(p, k);
                    self.presets_changed(ctl);
                }
            }
            id::SHOW_PRESETS | id::IDC_PRESETS_LABEL => {
                let open = reg::get_dword(OsStr::new("CapturePresetsOpen"), 0, None) == 0;
                reg::set_dword(OsStr::new("CapturePresetsOpen"), open as u32, None);
                if let Some(m) = self.main {
                    ctl.repaint(m);
                }
            }
            id::ONECLICK => set_one_click(!oneclick::enabled()),
            id::COLOR_PICKER => desk::run_color_picker(),
            id::FOCUS => desk::focus_toggle(),
            id::INTERVAL => {
                if interval::running() {
                    interval::stop();
                } else {
                    interval::setup(self.hwnd);
                }
            }
            id::RECORD_AUDIO => {
                // audio only (C32): the first video preset's audio settings, no video
                let mut p = self.store.first_of_kind(CaptureKind::Video);
                (p.audio_only, p.delay_seconds, p.webcam) = (true, 0, false);
                self.start_capture(ctl, &p);
            }
            id::AUDIO_FOLDER => {
                let d = String::from_utf16_lossy(&snapcore::library::audio_dir());
                let _ = std::fs::create_dir_all(&d);
                shell_open(&d);
            }
            id::AFTER_EDITOR => toggle_after_capture_editor(),
            id::CLEANUP_NOW => self.cleanup_now(),
            id::MANUAL => open_manual("capture-window"),
            id::CAPTURE_FILE => {
                let mut p = self.store.first_of_kind(CaptureKind::Image);
                p.selection = Selection::File;
                p.delay_seconds = 0;
                self.start_capture(ctl, &p);
            }
            id::QR_REGION => self.start_capture(ctl, &self.tool_preset(cmdline::Tool::Qr)),
            id::CAPTURE_PIN => self.start_capture(ctl, &self.tool_preset(cmdline::Tool::Pin)),
            id::PIN_CLIPBOARD => {
                pins::pin_clipboard();
            }
            id::PIN_FILE => self.pin_from_file(),
            id::PINS_SHOW => pins::show_all(true),
            id::PINS_HIDE => pins::show_all(false),
            id::PINS_CLOSE => pins::close_all(),
            id::PINS_REOPEN => {
                pins::reopen_history(0);
            }
            id::PINS_CLICKTHROUGH => pins::click_through_off_all(),
            h if (id::PIN_HISTORY_BASE..id::PIN_HISTORY_BASE + 10).contains(&h) => {
                pins::reopen_history((h - id::PIN_HISTORY_BASE) as usize);
            }
            _ => log::write(&format!("Command {cmd}: not in the Rust app yet")),
        }
    }

    /// /clipboard: the current preset's outputs for the image on the clipboard.
    fn capture_clipboard(&mut self, ctl: &mut Ctl) {
        let mut p = self.current();
        p.kind = CaptureKind::Image;
        p.selection = Selection::Clipboard;
        self.start_capture(ctl, &p);
    }

    fn tray_menu(&self) {
        use tray::Item::*;
        let presets: Vec<_> = self.store.presets.iter().enumerate()
            .map(|(i, p)| (id::PRESET_BASE + i as u32, i18n::tr_str(&p.name))).collect();
        let sub = tray::Menu::new(presets.iter().map(|(i, n)| Cmd(*i, n)).collect());
        let (img, vid, show, ed, quit, pre) = (tr("Capturar imagem"), tr("Gravar vídeo"), tr("Abrir a janela de captura"),
                                               tr("Abrir o Editor"), tr("Sair"), tr("Presets"));
        let qr = tr("Ler código QR");
        let one = tr("Mostrar o One Click");
        let n = pins::count();
        let pins_t = if n > 0 { tr("Imagens fixadas (%zu)").replace("%zu", &n.to_string()) } else { tr("Imagens fixadas") };
        let t = [tr("Gravar só o áudio"), tr("Conta-gotas de tela"),
                 if interval::running() {
                     tr("Parar a captura por intervalo (%d)").replace("%d", &interval::count().to_string())
                 } else {
                     tr("Captura por intervalo…")
                 },
                 tr("Foco de tela (apresentações)"), tr("Abrir o Editor depois da captura"), tr("Opções…")];
        let after = engine::after_capture_mode() != 3;
        let m = tray::Menu::new(vec![Cmd(id::CAPTURE_IMAGE, &img), Cmd(id::CAPTURE_VIDEO, &vid), Cmd(id::RECORD_AUDIO, &t[0]),
                                     Sub(&pre, sub), Separator,
                                     Cmd(id::COLOR_PICKER, &t[1]), Cmd(id::QR_REGION, &qr), Sub(&pins_t, pins_submenu()),
                                     Cmd(id::INTERVAL, &t[2]), Checked(id::FOCUS, &t[3], desk::focus_active()), Separator,
                                     Cmd(id::SHOW, &show), Checked(id::ONECLICK, &one, oneclick::enabled()),
                                     Cmd(id::OPEN_EDITOR, &ed), Checked(id::AFTER_EDITOR, &t[4], after), Cmd(id::OPTIONS, &t[5]),
                                     Separator, Cmd(id::EXIT, &quit)]);
        m.track(self.hwnd, Some(id::SHOW));
    }

    /// OneClickMenu: the gear of the One Click panel.
    fn one_click_menu(&self, x: i32, y: i32) {
        use tray::Item::*;
        let presets: Vec<_> = self.store.presets.iter().enumerate()
            .map(|(i, p)| (id::PRESET_BASE + i as u32, i18n::tr_str(&p.name))).collect();
        let sub = tray::Menu::new(presets.iter().map(|(i, n)| Cmd(*i, n)).collect());
        let t = [tr("Capturar imagem"), tr("Gravar vídeo"), tr("Presets"), tr("Abrir a janela de captura"), tr("Ocultar o One Click")];
        let m = tray::Menu::new(vec![Cmd(id::CAPTURE_IMAGE, &t[0]), Cmd(id::CAPTURE_VIDEO, &t[1]), Sub(&t[2], sub), Separator,
                                     Cmd(id::SHOW, &t[3]), Cmd(id::ONECLICK, &t[4])]);
        let c = m.choose(self.hwnd, x, y);
        if c != 0 {
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(Some(HWND(self.hwnd as _)), WM_COMMAND,
                                                                              WPARAM(c as usize), LPARAM(0));
            }
        }
    }

    fn tray_event(&mut self, ctl: &mut Ctl, lp: isize) {
        let ev = (lp & 0xFFFF) as u32;
        let h = HWND(self.hwnd as _);
        match ev {
            // a click captures, a double click opens the window: the single click waits for the double-click time
            WM_LBUTTONUP => {
                if self.tray_ignore_up {
                    self.tray_ignore_up = false;
                } else {
                    unsafe { SetTimer(Some(h), TRAY_CLICK_TIMER, GetDoubleClickTime(), None) };
                }
            }
            WM_LBUTTONDBLCLK => {
                unsafe {
                    let _ = KillTimer(Some(h), TRAY_CLICK_TIMER);
                }
                self.tray_ignore_up = true;
                self.show(ctl);
            }
            NIN_KEYSELECT => self.start_capture(ctl, &self.current()),
            WM_RBUTTONUP | WM_CONTEXTMENU => {
                unsafe {
                    let _ = KillTimer(Some(h), TRAY_CLICK_TIMER);
                }
                self.tray_menu();
            }
            _ => {}
        }
    }

    /// ProcessCommandLine: the switches of this start or of a later one (forwarded); without any, the window shows.
    fn command_line(&mut self, ctl: &mut Ctl, line: &str, startup: bool) {
        use cmdline::Cmd;
        let args = instance::split_command_line(line);
        if let Some(first) = args.first() {
            log::write(&format!("Command line: {} argument(s), first {first}", args.len()));
        }
        let (mut hide, mut handled) = (false, false);
        // automation (C44): an area, a tool and an output combine into one capture
        let mut req = self.store.first_of_kind(CaptureKind::Image);
        req.kind = CaptureKind::Image;
        let (mut do_capture, mut tool_chosen) = (false, false);
        let mut output = String::new();
        let area = |req: &mut Preset, r: snapcore::image::Rect| {
            req.selection = Selection::Region;
            (req.region_fixed, req.region_use_pos) = (true, true);
            (req.region_x, req.region_y, req.fixed_w, req.fixed_h) = (r.left, r.top, r.width(), r.height());
        };
        for c in cmdline::parse(&args) {
            match c {
                Cmd::Hide => hide = true,
                Cmd::Exit => {
                    unsafe {
                        let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                            Some(HWND(self.hwnd as _)), WM_COMMAND, WPARAM(id::EXIT as usize), LPARAM(0));
                    }
                    handled = true;
                }
                Cmd::Help => {
                    std::thread::spawn(|| unsafe {
                        let text = snapcore::wide::to_wide(tr(cmdline::HELP));
                        let title = snapcore::wide::to_wide(tr("Snapnab — linha de comando"));
                        MessageBoxW(None, PCWSTR(text.as_ptr()), PCWSTR(title.as_ptr()), MB_ICONINFORMATION);
                    });
                    handled = true;
                }
                Cmd::CaptureImage => {
                    self.start_capture(ctl, &self.store.first_of_kind(CaptureKind::Image));
                    handled = true;
                }
                Cmd::CaptureVideo => {
                    self.start_capture(ctl, &self.store.first_of_kind(CaptureKind::Video));
                    handled = true;
                }
                Cmd::Clipboard => {
                    self.capture_clipboard(ctl);
                    handled = true;
                }
                Cmd::Preset(name) => {
                    let found = self.store.presets.iter().find(|p| p.name.to_lowercase() == name.to_lowercase()).cloned();
                    if let Some(p) = found {
                        self.start_capture(ctl, &p);
                        handled = true;
                    }
                }
                Cmd::Region(r) => {
                    match r {
                        Some((x, y, w, h)) => {
                            area(&mut req, snapcore::image::Rect::new(x, y, x + w, y + h));
                            do_capture = true;
                        }
                        None => self.notify("Snapnab", &tr("/region espera x,y,largura,altura (ex.: /region 100,100,800,600).")),
                    }
                    handled = true;
                }
                Cmd::Window(t) => {
                    match (!t.is_empty()).then(|| snapcore::screen::find_window_by_title(&t)).flatten() {
                        Some(w) => {
                            area(&mut req, snapcore::screen::window_frame_rect(w));
                            do_capture = true;
                        }
                        None => {
                            log::write(&format!("Command line: no window titled '{t}'"));
                            self.notify("Snapnab", &(tr("Nenhuma janela com esse título: ") + &t));
                        }
                    }
                    handled = true;
                }
                Cmd::Monitor(n) => {
                    let mons = snapcore::screen::monitors();
                    if n >= 1 && n as usize <= mons.len() {
                        area(&mut req, mons[n as usize - 1]);
                        do_capture = true;
                    } else {
                        self.notify("Snapnab",
                                    &tr("Monitor %d não existe (há %zu).").replacen("%d", &n.to_string(), 1).replacen("%zu", &mons.len().to_string(), 1));
                    }
                    handled = true;
                }
                Cmd::FullScreen => {
                    req.selection = Selection::FullScreen;
                    (do_capture, handled) = (true, true);
                }
                Cmd::Delay(d) => req.delay_seconds = d,
                Cmd::Tool(t) => {
                    // SetShare: one share at a time
                    (req.save_to_file, req.open_in_program, req.office) = (false, false, -1);
                    req.grab_text = t == cmdline::Tool::Text;
                    req.pin_to_screen = t == cmdline::Tool::Pin;
                    req.read_qr = t == cmdline::Tool::Qr;
                    req.upload = t == cmdline::Tool::Upload;
                    (tool_chosen, do_capture, handled) = (true, true, true);
                }
                Cmd::PinFile(f) => {
                    if !pins::pin_file(&f) {
                        self.notify("Snapnab", &(tr("Não foi possível fixar: ") + &f));
                    }
                    handled = true;
                }
                Cmd::PinClipboard => {
                    pins::pin_clipboard();
                    handled = true;
                }
                Cmd::ColorPicker => {
                    desk::run_color_picker();
                    handled = true;
                }
                Cmd::Focus => {
                    desk::focus_toggle();
                    handled = true;
                }
                Cmd::Interval { stop } => {
                    if stop {
                        interval::stop();
                    } else {
                        interval::start_saved();
                    }
                    handled = true;
                }
                Cmd::Output(o) => output = o,
            }
        }
        if do_capture {
            match output.to_lowercase().as_str() {
                "clipboard" => (req.output_clipboard_only, req.preview_in_editor) = (true, false),
                "editor" => req.preview_in_editor = true,
                "none" => req.preview_in_editor = false,
                "" => {
                    if tool_chosen {
                        req.preview_in_editor = false;
                    }
                }
                _ => (req.output_path, req.preview_in_editor) = (output.clone(), false),
            }
            if tool_chosen || !output.is_empty() {
                req.save_to_file = false;
            }
            self.start_capture(ctl, &req);
        }
        if !handled && (!startup || (!hide && reg::get_dword(OsStr::new("ShowCaptureWindow"), 1, None) != 0)) {
            self.show(ctl);
        }
    }

    fn on_ipc(&mut self, ctl: &mut Ctl, m: ipc::Message) {
        match m.code {
            ipc::code::EDITOR_START_CAPTURE => {
                let k = if m.ivalue == 1 { CaptureKind::Video } else { CaptureKind::Image };
                self.start_capture(ctl, &self.store.first_of_kind(k));
            }
            ipc::code::EDITOR_CLOSED => log::write("Editor closed"),
            ipc::code::PIN_FILE => {
                if !m.text.is_empty() && !pins::pin_file(&m.text) {
                    log::write(&format!("Pin: could not open {}", m.text));
                }
            }
            ipc::code::RELOAD_CONFIG => {
                self.store = PresetStore::load();
                self.register_hotkeys();
            }
            _ => {}
        }
    }

    /// The capture window (drawn by `window::draw`) and what its clicks ask for.
    fn main_ui(&mut self, ctl: &mut Ctl, ui: &mut egui::Ui) {
        self.view.one_click = oneclick::enabled();
        self.view.presets_open = reg::get_dword(OsStr::new("CapturePresetsOpen"), 0, None) != 0;
        let get = |n: &str, d: u32| hotkeys::unpack(reg::get_dword(OsStr::new(n), d, None));
        let ((im, iv), (vm, vv)) = (get("ImageHotkey", hotkeys::pack(0, 0x2C)),
                                    get("VideoHotkey", hotkeys::pack(windows::Win32::UI::Input::KeyboardAndMouse::MOD_SHIFT.0, 0x2C)));
        (self.view.image_hotkey, self.view.video_hotkey) = (hotkeys::to_text(im, iv), hotkeys::to_text(vm, vv));
        let actions = window::draw(&mut self.store, &mut self.view, &hotkeys::to_text, ui);
        for a in actions {
            let (hwnd, i) = (self.hwnd, self.store.selected.max(0) as usize);
            match a {
                Action::Changed => {
                    self.store.save();
                    self.register_hotkeys();
                }
                Action::Command(c) => self.command(ctl, c),
                Action::Capture => self.start_capture(ctl, &self.current()),
                Action::EditFx(k) => dialogs::edit_fx(hwnd, i, k, &self.current()),
                Action::ShareChosen(s) => share_chosen(hwnd, s),
                Action::ShareGear => match window::share_of(&self.current()) {
                    window::Share::File => dialogs::file_output(hwnd, i, &self.current()),
                    window::Share::Program => dialogs::program_output(hwnd, i, &self.current()),
                    window::Share::Upload => upload_ui::configure(hwnd),
                    window::Share::PowerPoint => dialogs::powerpoint_options(hwnd),
                    _ => {}
                },
                Action::FixedRegion => dialogs::fixed_region(hwnd, i, &self.current()),
                Action::VideoOptions => dialogs::video_options(hwnd, i, &self.current()),
                Action::ChromaKey => dialogs::chroma_key(hwnd, i, &self.current()),
                Action::ListCameras => {
                    self.view.cams = snapcore::webcam::list_cameras().into_iter().map(|c| (c.name, c.link)).collect();
                }
                Action::PresetHotkey(i) => {
                    if let Some(p) = self.store.presets.get(i) {
                        dialogs::preset_hotkey(hwnd, i, p);
                    }
                }
                Action::PresetsToggled => {}
            }
        }
        self.fit_window(ctl);
    }

    /// ResizeToContent: the window grows with the preset list.
    fn fit_window(&self, ctl: &Ctl) {
        let mut v = self.view.clone();
        v.presets_open = reg::get_dword(OsStr::new("CapturePresetsOpen"), 0, None) != 0;
        let h = window::height(&self.store, &v);
        if let Some(w) = self.main.and_then(|k| ctl.window(k)) {
            let scale = w.scale_factor();
            let cur = w.inner_size().to_logical::<f32>(scale);
            if (cur.height - h).abs() > 1.0 || (cur.width - window::WIDTH).abs() > 1.0 {
                let _ = w.request_inner_size(winit::dpi::LogicalSize::new(window::WIDTH, h));
            }
        }
    }
}

/// A new output in the "Share" box: Office that is not installed is said; an upload not configured opens its settings.
fn share_chosen(hwnd: isize, s: window::Share) {
    use window::Share;
    use windows::Win32::UI::WindowsAndMessaging::MB_ICONINFORMATION;
    if (Share::Word as i32..=Share::OneNote as i32).contains(&(s as i32)) {
        if let Some(t) = snapcore::office::Target::from_i32(s as i32 - Share::Word as i32)
            && !snapcore::office::available(t)
        {
            dialogs::message_async(hwnd, tr("O %s não está instalado neste computador (ou é a versão da Microsoft Store, sem automação).")
                                       .replace("%s", t.name()), "Snapnab".into(), MB_ICONINFORMATION);
        }
    }
    if s == Share::Upload && !snapcore::upload::Settings::load().configured() {
        upload_ui::configure(hwnd);
    }
}

/// PinsSubmenu: pin from the clipboard, a file or a capture; the pins on the screen; the history.
fn pins_submenu() -> tray::Menu {
    use tray::Item::*;
    let t = [tr("Fixar da área de transferência"), tr("Fixar um arquivo…"), tr("Capturar e fixar"), tr("Mostrar todas"),
             tr("Ocultar todas"), tr("Fechar todas"), tr("Desligar \"atravessar cliques\""), tr("Reabrir a última fechada"),
             tr("Histórico")];
    let has = pins::count() > 0;
    let hist = pins::history_files();
    let when: Vec<String> = hist
        .iter()
        .take(10)
        .map(|f| {
            let ft = std::fs::metadata(f)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| (d.as_nanos() / 100) as u64 + 116_444_736_000_000_000)
                .unwrap_or(0);
            engine::local_time(ft)
        })
        .collect();
    let on = |id: u32, i: usize, en: bool| if en { Cmd(id, &t[i]) } else { Disabled(&t[i]) };
    let mut items = vec![Cmd(id::PIN_CLIPBOARD, &t[0]), Cmd(id::PIN_FILE, &t[1]), Cmd(id::CAPTURE_PIN, &t[2]), Separator,
                         on(id::PINS_SHOW, 3, has), on(id::PINS_HIDE, 4, has), on(id::PINS_CLOSE, 5, has),
                         on(id::PINS_CLICKTHROUGH, 6, has), on(id::PINS_REOPEN, 7, !hist.is_empty())];
    if !when.is_empty() {
        let h = tray::Menu::new(when.iter().enumerate().map(|(i, w)| Cmd(id::PIN_HISTORY_BASE + i as u32, w)).collect());
        items.push(Sub(&t[8], h));
    }
    tray::Menu::new(items)
}

/// The tray's "Open the Editor after capturing" (issue #3): off = Options › When done › Only copy to the clipboard;
/// on again = the mode chosen before.
fn toggle_after_capture_editor() {
    let mode = engine::after_capture_mode();
    let new = if mode == 3 {
        reg::get_dword(OsStr::new("AfterCaptureModeOn"), 0, None).min(2)
    } else {
        reg::set_dword(OsStr::new("AfterCaptureModeOn"), mode, None);
        3
    };
    reg::set_dword(OsStr::new("AfterCaptureMode"), new, None);
}

/// OpenManual: help\Snapnab-Manual.html, at `topic` in the default browser (opening the .html through its
/// association would drop the #fragment).
fn open_manual(topic: &str) {
    let path = snapcore::paths::exe_dir().join("help").join("Snapnab-Manual.html");
    if !path.exists() {
        engine::message_box(tr("O manual não foi encontrado:\n") + &path.to_string_lossy(), "Snapnab".into());
        return;
    }
    let url = format!("file:///{}#{topic}", path.to_string_lossy().replace('\\', "/").replace(' ', "%20"));
    let browser = {
        use windows::Win32::UI::Shell::{ASSOCF_IS_PROTOCOL, ASSOCSTR_EXECUTABLE, AssocQueryStringW};
        let mut buf = vec![0u16; 520];
        let mut n = buf.len() as u32;
        let ok = unsafe {
            AssocQueryStringW(ASSOCF_IS_PROTOCOL, ASSOCSTR_EXECUTABLE, w!("http"), w!("open"), Some(windows::core::PWSTR(buf.as_mut_ptr())),
                              &mut n)
        }
        .is_ok();
        ok.then(|| String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(0)]))
    };
    if let Some(b) = browser.filter(|b| !b.is_empty()) {
        let (bw, arg) = (snapcore::wide::to_wide(&b), snapcore::wide::to_wide(format!("\"{url}\"")));
        let r = unsafe { ShellExecuteW(None, PCWSTR::null(), PCWSTR(bw.as_ptr()), PCWSTR(arg.as_ptr()), PCWSTR::null(), SW_SHOWNORMAL) };
        if r.0 as isize > 32 {
            log::write(&format!("Manual: opened #{topic} in {b}"));
            return;
        }
    }
    shell_open(&path.to_string_lossy());
}

/// SetOneClick: the panel and its setting.
fn set_one_click(on: bool) {
    reg::set_dword(OsStr::new("OneClickEnabled"), on as u32, None);
    oneclick::show(on);
}

pub(crate) fn shell_open(path: &str) {
    let p = snapcore::wide::to_wide(path);
    unsafe {
        ShellExecuteW(None, w!("open"), PCWSTR(p.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

fn about() {
    let text = format!("Snapnab {VERSION}{}\nCopyright © 2026 Antonio Rezende\n{}\n\n{}{}", tr(" — captura de tela e gravação de vídeo"),
                       tr("Software livre sob a GNU General Public License versão 3."),
                       tr("Processo de captura (Snapnab.exe) + Editor (SnapnabEditor.exe), comunicando-se por memória\n\
                           compartilhada (canal IPC) e WM_COPYDATA. Capturas ficam na biblioteca:\n"),
                       String::from_utf16_lossy(&snapcore::library::library_dir()));
    std::thread::spawn(move || unsafe {
        let t = snapcore::wide::to_wide(text);
        let title = snapcore::wide::to_wide(tr("Sobre o Snapnab"));
        MessageBoxW(None, PCWSTR(t.as_ptr()), PCWSTR(title.as_ptr()), MB_ICONINFORMATION);
    });
}

impl App for CaptureApp {
    fn start(&mut self, ctl: &mut Ctl) {
        TASKBAR_CREATED.store(unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) }, Ordering::Relaxed);
        let spec = WindowSpec {
            class: ipc::CAPTURE_WND_CLASS,
            title: "Snapnab".into(),
            size: {
                let v = window::View { presets_open: reg::get_dword(OsStr::new("CapturePresetsOpen"), 0, None) != 0, ..Default::default() };
                (window::WIDTH, window::height(&self.store, &v))
            },
            visible: false,
            resizable: false,
            intercept: Some(intercept),
            clear_color: [0.97, 0.97, 0.98, 1.0],
            ..Default::default()
        };
        // the hotkeys work from the moment the window exists, before its GPU setup
        let early = |h: isize| {
            self.hwnd = h;
            self.hk = Some(hotkeys::Manager::new(h));
            self.register_hotkeys();
        };
        let Some(k) = ctl.create_with(&spec, early) else {
            log::write("Capture window: could not be created");
            ctl.exit();
            return;
        };
        self.main = Some(k);
        self.hwnd = ctl.hwnd(k);
        center_on_primary(self.hwnd);
        tray::add(self.hwnd, &tr("Snapnab — clique para capturar"));
        unsafe {
            let _ = AddClipboardFormatListener(HWND(self.hwnd as _));
            self.last_clip_seq = windows::Win32::System::DataExchange::GetClipboardSequenceNumber();
        }
        pins::init(ctl.waker());
        self.waker = Some(ctl.waker());
        desk::restore_after_crash(); // desktop icons / notifications left hidden by a crash (C16)
        unsafe {
            SetTimer(Some(HWND(self.hwnd as _)), CLEANUP_TIMER, 30_000, None); // the first library cleanup half a minute after the start
        }
        if reg::get_dword(OsStr::new("OneClickEnabled"), 0, None) != 0 {
            oneclick::show(true);
        }
        let waker = ctl.waker();
        self.ipc = Notify::start(Channel::Capture, move |m| waker.send(Ev::Ipc(m)));
        // recordings a crash or a power cut interrupted (C14): remuxed into the library
        let rec = snapcore::recorder::recover_interrupted();
        if !rec.is_empty() {
            let name = std::path::Path::new(&rec[0]).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
            self.notify(&tr("Snapnab — gravação recuperada"),
                        &(tr("%zu gravação(ões) interrompida(s) foi(ram) recuperada(s) na biblioteca.").replace("%zu", &rec.len().to_string()) + "\n" + &name));
            if let Some(i) = &self.ipc {
                i.send_int(Channel::Editor, ipc::code::LIBRARY_CHANGED, 0, false);
            }
        }
        self.command_line(ctl, &instance::command_line(), true);
    }

    fn ui(&mut self, ctl: &mut Ctl, win: WinKey, ui: &mut egui::Ui) {
        if Some(win) == self.main {
            self.main_ui(ctl, ui);
        } else if self.engine.owns(win) {
            let env = env!(self);
            self.engine.ui(ctl, &env, win, ui);
            self.after_engine();
        }
    }

    fn message(&mut self, ctl: &mut Ctl, m: RawMsg) {
        let env = env!(self);
        if self.engine.message(ctl, &env, &m) {
            return;
        }
        match m.msg {
            WM_COPYDATA => {
                if let Some(t) = m.text {
                    self.command_line(ctl, &t, false);
                }
            }
            WM_COMMAND => self.command(ctl, (m.wparam & 0xFFFF) as u32),
            tray::WM_SNAP_TRAY => self.tray_event(ctl, m.lparam),
            WM_TIMER if m.wparam == CLEANUP_TIMER => {
                unsafe {
                    SetTimer(Some(HWND(self.hwnd as _)), CLEANUP_TIMER, 6 * 3600 * 1000, None);
                }
                if snapcore::library::cleanup_now() > 0 {
                    self.library_changed();
                }
            }
            WM_TIMER => {
                unsafe {
                    let _ = KillTimer(Some(HWND(self.hwnd as _)), TRAY_CLICK_TIMER);
                }
                self.start_capture(ctl, &self.current());
            }
            WM_CLIPBOARDUPDATE => self.on_clipboard_update(),
            WM_HOTKEY => self.on_hotkey(ctl, m.wparam as i32),
            x if x == TASKBAR_CREATED.load(Ordering::Relaxed) => tray::add(self.hwnd, &tr("Snapnab — clique para capturar")),
            _ => {}
        }
        self.after_engine();
    }

    fn close_requested(&mut self, ctl: &mut Ctl, win: WinKey) {
        if Some(win) == self.main {
            // the capture window hides; the app stays in the tray
            if let Some(w) = ctl.window(win) {
                w.set_visible(false);
            }
        } else if self.engine.owns(win) {
            let env = env!(self);
            self.engine.close_requested(ctl, &env, win);
            self.after_engine();
        } else {
            ctl.close(win);
        }
    }

    fn event(&mut self, ctl: &mut Ctl, ev: Box<dyn std::any::Any + Send>) {
        let ev = match ev.downcast::<Ev>() {
            Ok(ev) => {
                match *ev {
                    Ev::Ipc(m) => self.on_ipc(ctl, m),
                }
                return;
            }
            Err(ev) => ev,
        };
        let ev = match ev.downcast::<pins::UiEv>() {
            Ok(ev) => {
                match *ev {
                    pins::UiEv::Notify(t, x) => self.notify(&t, &x),
                    pins::UiEv::Qr(img) => qr::show(img),
                    pins::UiEv::Upload(img, lib) => {
                        let title = std::path::Path::new(&lib).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        share::upload(self.hwnd, img, lib, title);
                    }
                    pins::UiEv::Editor(path) => {
                        if let Some(i) = &self.ipc {
                            i.send_int(Channel::Editor, ipc::code::LIBRARY_CHANGED, 0, false);
                        }
                        engine::deliver_to_editor(&env!(self), &path);
                    }
                    pins::UiEv::LibraryChanged => {
                        if let Some(i) = &self.ipc {
                            i.send_int(Channel::Editor, ipc::code::LIBRARY_CHANGED, 0, false);
                        }
                    }
                    pins::UiEv::Capture => self.start_capture(ctl, &self.current()),
                    pins::UiEv::ColorPicked(c) => {
                        snapcore::color::add_recent(c);
                        // copied right away; the menu offers the other formats
                        snapcore::clip::copy_text(self.hwnd, &snapcore::color::to_text(c, snapcore::color::default_format()));
                        let (x, y) = snapcore::screen::cursor_pos();
                        desk::color_formats_menu(self.hwnd, c, (x, y));
                    }
                    pins::UiEv::IntervalChosen(s) => {
                        if s.area == 2 || s.area == 3 {
                            // the region or window is chosen with the crosshair; the capture starts when it is chosen
                            self.pending_interval = Some(s.clone());
                            let env = env!(self);
                            if !self.engine.pick_area(ctl, &env, s.area == 3) {
                                self.pending_interval = None;
                            }
                        } else {
                            interval::begin(s, snapcore::image::Rect::default(), 0);
                        }
                    }
                    pins::UiEv::IntervalArea(r, w) => {
                        if let Some(s) = self.pending_interval.take() {
                            interval::begin(s, r, w);
                        }
                    }
                    pins::UiEv::IntervalStatus(on, n) => {
                        let tip = if on {
                            tr("Snapnab — captura por intervalo: %d imagem(ns)").replace("%d", &n.to_string())
                        } else {
                            tr("Snapnab — clique para capturar")
                        };
                        tray::set_status(self.hwnd, on, &tip);
                    }
                    pins::UiEv::OneClickMenu(x, y) => {
                        self.one_click_menu(x, y);
                        oneclick::menu_closed();
                    }
                    pins::UiEv::Store(f) => {
                        f(&mut self.store);
                        self.presets_changed(ctl);
                    }
                    pins::UiEv::PresetHotkey(i, m, v) => self.set_preset_hotkey(ctl, i, m, v),
                    pins::UiEv::Reload => {
                        self.register_hotkeys();
                        if let Some(i) = &self.ipc {
                            i.send_int(Channel::Editor, ipc::code::RELOAD_CONFIG, 0, false);
                        }
                    }
                    pins::UiEv::RestartApps => self.restart_apps(ctl),
                }
                return;
            }
            Err(ev) => ev,
        };
        if let Ok(ev) = ev.downcast::<EngineEv>() {
            let env = env!(self);
            self.engine.event(ctl, &env, *ev);
            self.after_engine();
        }
    }
}

/// The capture window starts in the middle of the primary screen's work area, a third of the way down.
fn center_on_primary(hwnd: isize) {
    use windows::Win32::Foundation::{POINT, RECT};
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint};
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos};
    unsafe {
        let h = HWND(hwnd as _);
        let mut wr = RECT::default();
        let _ = GetWindowRect(h, &mut wr);
        let mon = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut mi);
        let wa = mi.rcWork;
        let (ww, wh) = (wr.right - wr.left, wr.bottom - wr.top);
        let _ = SetWindowPos(h, None, wa.left + (wa.right - wa.left - ww) / 2, wa.top + (wa.bottom - wa.top - wh) / 3, 0, 0,
                             SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

fn main() {
    instance::wait_for_previous_instance(ipc::CAPTURE_WND_CLASS);
    let Some(mutex) = instance::claim_or_forward(ipc::CAPTURE_MUTEX, ipc::CAPTURE_WND_CLASS) else {
        // forwarded: ends right away, without the DLL unload notifications (third-party overlays injected into
        // every process crashed in them when the process ended this early)
        unsafe {
            let _ = TerminateProcess(GetCurrentProcess(), 0);
        }
        return;
    };
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    log::init("Snapnab", VERSION);
    // a windowed program has no console: a panic goes to the log before the process ends
    std::panic::set_hook(Box::new(|info| log::write(&format!("PANIC: {info}"))));
    i18n::init();
    snapcore::gdip::startup();
    let app = CaptureApp {
        store: PresetStore::load(),
        main: None,
        hwnd: 0,
        ipc: None,
        tray_ignore_up: false,
        engine: Engine::new(),
        last_clip_seq: 0,
        hk: None,
        last: None,
        video_keys: false,
        pending_interval: None,
        view: Default::default(),
        waker: None,
    };
    if let Err(e) = snapui::host::run(app, |_| {}) {
        log::write(&format!("Event loop: {e}"));
    }
    log::write("Snapnab exited");
    drop(mutex);
}

/// The name of effect k (translated where it is shown).
pub(crate) fn window_fx_name(k: usize) -> &'static str {
    window::FX_NAMES[k.min(window::FX_NAMES.len() - 1)]
}
