//! Video in the capture engine (CaptureEngine.cpp, "video"): the HUD over the selected area, the countdown, the
//! recorder's start, pause, stop, restart and cancel, the snapshot during a recording, and what happens to the video
//! (the library, the clipboard, the Editor).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use snapcore::image::Rect;
use snapcore::library::{self, CaptureInfo};
use snapcore::presets::Preset;
use snapcore::recorder::{Note, RecordSettings, Recorder, State as RecState};
use snapcore::{clip, gdip, ipc, log, screen};
use snapnab_app::hud::{self, Btn, View};
use snapnab_app::tr;
use snapui::host::{Caught, Ctl, WinKey, WindowSpec};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{CreateRoundRectRgn, SetWindowRgn};
use windows::Win32::UI::Input::KeyboardAndMouse::MOD_SHIFT;
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetWindowLongW, GetWindowRect, HWND_TOPMOST, MA_NOACTIVATE, MB_ICONERROR, MB_ICONQUESTION, MB_YESNO,
    MessageBoxW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOSIZE, SetWindowDisplayAffinity, SetWindowLongW, SetWindowPos,
    ShowWindow, WDA_EXCLUDEFROMCAPTURE, WM_MOUSEACTIVATE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};
use windows::core::PCWSTR;

use super::{Engine, EngineEv, Env, State, dword, message_box, text, wtext};
use crate::rawwin;
use crate::{VERSION, hotkeys};

struct HudWin {
    key: WinKey,
    hwnd: isize,
    tip: isize,
}

struct Count {
    hwnd: isize,
    start: Instant,
    secs: i32,
}

#[derive(Default)]
pub(super) struct Video {
    pub rec: Recorder,
    hud: Option<HudWin>,
    vol: Option<(WinKey, isize)>,
    view: View,
    border: isize,
    count: Option<Count>,
    area: Rect,
    /// the restart question is open (the HUD waits)
    asking: bool,
    /// the volumes changed in the panel: the app saves them in the preset
    pub volumes: Option<Preset>,
}

/// The HUD and the volume panel never take the focus from the program being recorded.
fn no_activate(_hwnd: isize, msg: u32, _wp: usize, _lp: isize) -> Caught {
    if msg == WM_MOUSEACTIVATE { Caught::Handled(MA_NOACTIVATE as isize) } else { Caught::Pass }
}

/// Topmost tool window that is not activated, has rounded corners and stays out of captures.
pub(super) fn float_window(hwnd: isize, x: i32, y: i32, w: i32, h: i32, radius: i32) {
    let h_ = HWND(hwnd as _);
    unsafe {
        let ex = GetWindowLongW(h_, GWL_EXSTYLE) as u32 | WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0;
        SetWindowLongW(h_, GWL_EXSTYLE, ex as i32);
        let _ = SetWindowPos(h_, Some(HWND_TOPMOST), x, y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
        SetWindowRgn(h_, Some(CreateRoundRectRgn(0, 0, w + 1, h + 1, radius, radius)), false);
        let _ = SetWindowDisplayAffinity(h_, WDA_EXCLUDEFROMCAPTURE);
        let _ = ShowWindow(h_, SW_SHOWNOACTIVATE);
    }
}

fn window_rect(hwnd: isize) -> RECT {
    let mut r = RECT::default();
    unsafe {
        let _ = GetWindowRect(HWND(hwnd as _), &mut r);
    }
    r
}

fn hud_status(rec: &Recorder, v: &View) -> hud::Status {
    let state = match rec.state() {
        RecState::Recording => hud::RecState::Recording,
        RecState::Paused => hud::RecState::Paused,
        _ => hud::RecState::Idle,
    };
    let _ = v;
    hud::Status {
        state,
        elapsed: rec.elapsed(),
        mic_level: rec.mic_level(),
        sys_level: rec.sys_level(),
        mic_muted: rec.mic_muted(),
        sys_muted: rec.sys_muted(),
        has_mic: rec.has_mic(),
        has_sys: rec.has_sys(),
        zoomed: rec.zoomed(),
        overlays: rec.highlight_on() || rec.clicks_on() || rec.key_mode() > 0,
        blink: (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() / 500) % 2 == 1,
    }
}

/// The shortcut of a HUD button, from the settings (Options › shortcuts).
fn button_key(b: Btn) -> Option<String> {
    let (name, def) = match b {
        Btn::Record => ("VideoStartPauseResumeKey", hotkeys::pack(MOD_SHIFT.0, 0x78)),
        Btn::Stop => ("VideoStopKey", hotkeys::pack(MOD_SHIFT.0, 0x79)),
        Btn::Zoom => ("VideoZoomKey", hotkeys::pack(MOD_SHIFT.0, 0x7A)),
        _ => return None,
    };
    let (m, v) = hotkeys::unpack(dword(name, def));
    (v != 0).then(|| hotkeys::to_text(m, v))
}

/// The restart question (C33) with its "do not ask again" box: (yes, do not ask again).
fn ask_restart(owner: isize) -> (bool, bool) {
    use windows::Win32::UI::Controls::{
        TASKDIALOG_FLAGS, TASKDIALOGCONFIG, TASKDIALOGCONFIG_0, TDCBF_NO_BUTTON, TDCBF_YES_BUTTON, TDF_POSITION_RELATIVE_TO_WINDOW,
        TaskDialogIndirect,
    };
    let (title, main, content, verify) = (snapcore::wide::to_wide("Snapnab"), snapcore::wide::to_wide(tr("Recomeçar a gravação?")),
                                          snapcore::wide::to_wide(tr("O que foi gravado até agora será descartado.")),
                                          snapcore::wide::to_wide(tr("Não perguntar de novo")));
    let tc = TASKDIALOGCONFIG {
        cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
        hwndParent: HWND(owner as _),
        dwFlags: TASKDIALOG_FLAGS(TDF_POSITION_RELATIVE_TO_WINDOW.0),
        dwCommonButtons: TDCBF_YES_BUTTON | TDCBF_NO_BUTTON,
        pszWindowTitle: PCWSTR(title.as_ptr()),
        Anonymous1: TASKDIALOGCONFIG_0 { pszMainIcon: PCWSTR(0xFFFF as *const u16) }, // TD_WARNING_ICON
        pszMainInstruction: PCWSTR(main.as_ptr()),
        pszContent: PCWSTR(content.as_ptr()),
        pszVerificationText: PCWSTR(verify.as_ptr()),
        nDefaultButton: 7, // IDNO
        ..Default::default()
    };
    let (mut btn, mut dont) = (0i32, windows::core::BOOL(0));
    let ok = unsafe { TaskDialogIndirect(&tc, Some(&mut btn), None, Some(&mut dont)) }.is_ok();
    (ok && btn == 6, dont.as_bool())
}

impl Engine {
    /// The HUD is up (from the selection to the end of the recording): the recording hotkeys are registered meanwhile.
    pub fn video_active(&self) -> bool {
        self.video.hud.is_some()
    }

    pub fn recording(&self) -> bool {
        self.video.rec.state() != RecState::Idle
    }

    pub fn take_volumes(&mut self) -> Option<Preset> {
        self.video.volumes.take()
    }

    pub(super) fn video_owns(&self, k: WinKey) -> bool {
        self.video.hud.as_ref().is_some_and(|h| h.key == k) || self.video.vol.is_some_and(|v| v.0 == k)
    }

    /// StartVideoHud: the HUD below the area, the red frame around it, the meters already live.
    pub(super) fn start_video_hud(&mut self, ctl: &mut Ctl, env: &Env, area: Rect) {
        // an even size
        let area = Rect::new(area.left, area.top, area.left + (area.width() & !1), area.top + (area.height() & !1));
        self.video.area = area;
        self.set_state(State::InputSelected);
        let p = self.preset.clone();
        let v = &mut self.video;
        v.view = View::default();
        (v.view.mic_on, v.view.sys_on, v.view.audio_only) = (p.microphone, p.system_audio, p.audio_only);
        (v.view.mic_volume, v.view.sys_volume, v.view.auto_gain) = (p.mic_volume, p.sys_volume, p.auto_gain);
        if p.webcam && !p.audio_only {
            let st = crate::camwin::Style { shape: p.webcam_shape.clamp(0, 4), chroma: p.chroma_key, key: p.chroma_color,
                                            tolerance: p.chroma_tolerance };
            match crate::camwin::start(&p.webcam_device, area, st) {
                Ok(()) => (v.view.cam_available, v.view.cam_on) = (true, true),
                Err(e) => env.notify(&tr("Snapnab — webcam"), &e),
            }
        }
        v.rec.set_mic_volume(p.mic_volume);
        v.rec.set_sys_volume(p.sys_volume);
        v.rec.set_auto_gain(p.auto_gain);
        v.rec.start_monitor(p.microphone, p.system_audio);
        if !p.audio_only {
            v.border = rawwin::border(area);
        }
        let (w, h) = hud::size(&v.view);
        let spec = WindowSpec {
            class: "SnapnabRecorderHud",
            title: tr("Snapnab — gravação"),
            size: (w, h),
            visible: false,
            decorations: false,
            resizable: false,
            topmost: true,
            skip_taskbar: true,
            intercept: Some(no_activate),
            clear_color: [30.0 / 255.0, 30.0 / 255.0, 32.0 / 255.0, 1.0],
            ..Default::default()
        };
        let Some(k) = ctl.create(&spec) else {
            log::write("Recorder: the HUD could not be created");
            self.video_cancel(ctl);
            return;
        };
        let hwnd = ctl.hwnd(k);
        let sf = ctl.window(k).map_or(1.0, |w| w.scale_factor());
        let (pw, ph) = ((w as f64 * sf) as i32, (h as f64 * sf) as i32);
        let s = |v: f64| (v * sf) as i32;
        // below the area; if it does not fit, above; otherwise inside, in the bottom corner
        let mut c = ((area.left + area.right) / 2, area.bottom);
        if p.audio_only {
            c = ((area.left + area.right) / 2, (area.top + area.bottom) / 2);
        }
        let mon = screen::monitor_rect(c.0, c.1, true);
        let x = ((area.left + area.right) / 2 - pw / 2).clamp(mon.left, (mon.right - pw).max(mon.left));
        let mut y = area.bottom + s(10.0);
        if p.audio_only {
            y = mon.bottom - ph - s(90.0);
        }
        if y + ph > mon.bottom {
            y = area.top - ph - s(10.0);
        }
        if y < mon.top {
            y = area.bottom - ph - s(16.0);
        }
        float_window(hwnd, x, y, pw, ph, s(14.0));
        let tips: Vec<(RECT, String)> = hud::buttons(&self.video.view)
            .iter()
            .map(|(b, r)| {
                let rr = RECT { left: s(r.left() as f64), top: s(r.top() as f64), right: s(r.right() as f64), bottom: s(r.bottom() as f64) };
                (rr, hud::tip(*b, button_key))
            })
            .collect();
        let tip = rawwin::tooltips(hwnd, &tips);
        self.video.hud = Some(HudWin { key: k, hwnd, tip });
        // audio only (C32): no area to frame, so it starts by itself after the countdown (Skip or Cancel still work)
        if p.audio_only {
            self.countdown(3);
        }
        let _ = env;
    }

    fn countdown(&mut self, secs: i32) {
        let hwnd = rawwin::countdown(self.video.area, secs);
        self.video.count = Some(Count { hwnd, start: Instant::now(), secs });
    }

    /// "Skip", the record button or the hotkey during the countdown: starts right away.
    fn skip_countdown(&mut self, ctl: &mut Ctl, env: &Env) {
        if let Some(mut c) = self.video.count.take() {
            rawwin::destroy(&mut c.hwnd);
            self.start_recording(ctl, env);
        }
    }

    fn close_hud(&mut self, ctl: &mut Ctl) {
        let v = &mut self.video;
        if let Some(mut c) = v.count.take() {
            rawwin::destroy(&mut c.hwnd);
        }
        if let Some((k, _)) = v.vol.take() {
            ctl.close(k);
        }
        if let Some(mut h) = v.hud.take() {
            rawwin::destroy(&mut h.tip);
            ctl.close(h.key);
        }
        rawwin::destroy(&mut v.border);
        v.view.volume_open = false;
        crate::camwin::stop();
    }

    fn start_recording(&mut self, ctl: &mut Ctl, env: &Env) {
        let p = self.preset.clone();
        let v = &self.video.view;
        let path = if p.audio_only {
            text(&library::new_audio_path(p.audio_format == 1))
        } else {
            text(&library::new_video_path(&library::library_dir()))
        };
        let rs = RecordSettings {
            rect: self.video.area,
            cursor: p.capture_cursor,
            microphone: v.mic_on,
            system_audio: v.sys_on,
            mic_volume: v.mic_volume,
            sys_volume: v.sys_volume,
            auto_gain: v.auto_gain,
            fps: p.fps,
            quality: p.quality,
            downscale_1080: p.downscale_1080,
            hardware: p.hardware_encode,
            audio_only: p.audio_only,
            path,
            // drawn in the video (C07), zoom (C15) and crash safety (C14): Options › Video options
            highlight_cursor: dword("RecHighlightCursor", 0) != 0,
            highlight_color: dword("RecHighlightColor", 0x00D6FF),
            highlight_size: (dword("RecHighlightSize", 44) as i32).clamp(16, 200),
            highlight_opacity: (dword("RecHighlightOpacity", 35) as i32).clamp(5, 100),
            click_effects: dword("RecClickEffects", 0) != 0,
            left_click_color: dword("RecLeftClickColor", 0x2D32E5),
            right_click_color: dword("RecRightClickColor", 0xD77800),
            key_mode: (dword("RecKeyMode", 0) as i32).clamp(0, 2),
            auto_zoom: dword("RecAutoZoom", 0) != 0,
            zoom_percent: (dword("RecZoomPercent", 200) as i32).clamp(120, 400),
            crash_safe: dword("RecCrashSafe", 1) != 0,
            ..Default::default()
        };
        let waker = Mutex::new(ctl.waker());
        let notify = Arc::new(move |n: Note| {
            if let Ok(w) = waker.lock() {
                w.send(EngineEv::RecorderNote(n));
            }
        });
        if !self.video.rec.start(&rs, notify) {
            message_box(tr("Não foi possível iniciar a gravação.\n\n") + &self.video.rec.last_error(), "Snapnab".into());
            self.video.rec.stop_monitor();
            self.close_hud(ctl);
            self.set_state(State::Idle);
            return;
        }
        self.set_state(State::Capturing);
        let _ = env;
    }

    /// The record button or VideoStartPauseResumeKey.
    pub fn video_start_pause_resume(&mut self, ctl: &mut Ctl, env: &Env) {
        if self.video.count.is_some() {
            self.skip_countdown(ctl, env); // during the countdown: start right away
            return;
        }
        let rs = self.video.rec.state();
        if self.state == State::InputSelected && rs == RecState::Idle {
            self.countdown(3);
        } else if rs == RecState::Recording {
            self.video.rec.pause(); // SuspendCapture
            self.set_state(State::Suspended);
        } else if rs == RecState::Paused {
            self.video.rec.resume(); // ResumeCapture
            self.set_state(State::Capturing);
        }
    }

    pub fn video_toggle_zoom(&mut self) {
        if self.video.rec.state() != RecState::Idle && !self.preset.audio_only {
            self.video.rec.toggle_zoom();
        }
    }

    /// The overlay menu (C07): cursor highlight, clicks, keys on screen.
    fn video_overlay_menu(&mut self, at: (i32, i32)) {
        let Some(owner) = self.video.hud.as_ref().map(|h| h.hwnd) else { return };
        let r = &self.video.rec;
        let live = r.state() != RecState::Idle;
        let mut hl = if live { r.highlight_on() } else { dword("RecHighlightCursor", 0) != 0 };
        let mut cl = if live { r.clicks_on() } else { dword("RecClickEffects", 0) != 0 };
        let mut km = if live { r.key_mode() } else { dword("RecKeyMode", 0) as i32 };
        let t = [tr("Destacar o cursor"), tr("Mostrar os cliques"), tr("Teclas: não mostrar"),
                 tr("Teclas: só atalhos (Ctrl, Alt, Win, F1…)"), tr("Teclas: todas (inclusive o que for digitado)")];
        use crate::tray::{Item, Menu};
        let m = Menu::new(vec![Item::Checked(1, &t[0], hl), Item::Checked(2, &t[1], cl), Item::Separator, Item::Checked(10, &t[2], km == 0),
                               Item::Checked(11, &t[3], km == 1), Item::Checked(12, &t[4], km == 2)]);
        match m.choose(owner, at.0, at.1) {
            0 => return,
            1 => hl = !hl,
            2 => cl = !cl,
            c => km = c as i32 - 10,
        }
        use std::ffi::OsStr;
        snapcore::reg::set_dword(OsStr::new("RecHighlightCursor"), hl as u32, None);
        snapcore::reg::set_dword(OsStr::new("RecClickEffects"), cl as u32, None);
        snapcore::reg::set_dword(OsStr::new("RecKeyMode"), km as u32, None);
        self.video.rec.set_overlay(hl, cl, km);
    }

    /// The stop button or VideoStopKey.
    pub fn video_stop(&mut self, ctl: &mut Ctl, env: &Env) {
        if self.video.rec.state() == RecState::Idle {
            if self.state == State::InputSelected {
                self.video_cancel(ctl);
            }
            return;
        }
        self.set_state(State::Outputting);
        let ok = self.video.rec.stop();
        self.finish_video(ctl, env, ok);
    }

    pub fn video_cancel(&mut self, ctl: &mut Ctl) {
        if self.video.rec.state() != RecState::Idle {
            self.video.rec.cancel();
        }
        self.video.rec.stop_monitor();
        self.close_hud(ctl);
        self.set_state(State::Idle);
    }

    /// Exit while recording: asks, then stops and saves. False = keep running.
    pub fn confirm_exit(&mut self, ctl: &mut Ctl, env: &Env, owner: isize) -> bool {
        if !self.recording() {
            return true;
        }
        let (t, c) = (snapcore::wide::to_wide(tr("Há uma gravação em andamento. Parar e sair?")), snapcore::wide::to_wide("Snapnab"));
        let yes = unsafe { MessageBoxW(Some(HWND(owner as _)), PCWSTR(t.as_ptr()), PCWSTR(c.as_ptr()), MB_YESNO | MB_ICONQUESTION) }.0 == 6;
        if yes {
            self.video_stop(ctl, env);
        }
        yes
    }

    fn snapshot(&mut self, env: &Env) {
        let img = self.video.rec.take_snapshot();
        let info = CaptureInfo { mode: wtext(&tr("Imagem durante gravação")), ..Default::default() };
        let thumb = gdip::make_thumbnail(&img, 160, 120);
        let _ = library::add(&library::library_dir(), img, &thumb, &info, None, Default::default(), VERSION);
        if let Some(i) = env.ipc {
            i.send_int(ipc::Channel::Editor, ipc::code::LIBRARY_CHANGED, 0, false);
        }
        env.notify("Snapnab", &tr("Imagem capturada sem parar a gravação."));
    }

    /// The restart button: asks first (C33; the recording pauses meanwhile), then throws away and counts down again.
    fn restart(&mut self, ctl: &mut Ctl, env: &Env) {
        if dword("ConfirmRestart", 1) == 0 {
            self.restart_now();
            return;
        }
        if self.video.asking {
            return;
        }
        let was_recording = self.video.rec.state() == RecState::Recording;
        if was_recording {
            self.video.rec.pause();
        }
        self.video.asking = true;
        let owner = self.video.hud.as_ref().map_or(0, |h| h.hwnd);
        let w = ctl.waker();
        // on its own thread: the HUD and the meters keep drawing while the question is open
        std::thread::spawn(move || {
            let (yes, dont_ask) = ask_restart(owner);
            w.send(EngineEv::Restart { yes, dont_ask, was_recording });
        });
        let _ = env;
    }

    fn restart_now(&mut self) {
        self.video.rec.cancel();
        self.countdown(3);
    }

    pub(super) fn restart_answer(&mut self, yes: bool, dont_ask: bool, was_recording: bool) {
        self.video.asking = false;
        if self.video.hud.is_none() {
            return;
        }
        if !yes {
            if was_recording {
                self.video.rec.resume();
            }
            return;
        }
        if dont_ask {
            snapcore::reg::set_dword(std::ffi::OsStr::new("ConfirmRestart"), 0, None);
        }
        self.restart_now();
    }

    /// FinishVideo: the thumbnail, the clipboard (the file), the Editor or a balloon.
    fn finish_video(&mut self, ctl: &mut Ctl, env: &Env, ok: bool) {
        self.video.rec.stop_monitor();
        self.close_hud(ctl);
        let path = self.video.rec.path().to_string();
        let exists = std::path::Path::new(&path).exists();
        if ok && exists && self.preset.audio_only {
            // audio only (C32): outside the library
            if dword("CopyEveryCapture", 1) != 0 {
                clip::copy_files(env.hwnd, &[path.clone()]);
            }
            env.notify(&tr("Snapnab — áudio salvo"), &path);
            self.set_state(State::Idle);
            return;
        }
        if ok && exists {
            let first = self.video.rec.first_frame();
            if !first.is_empty() {
                library::write_thumbnail(&library::library_dir(), &wtext(&path), &gdip::make_thumbnail(&first, 160, 120), VERSION);
            }
            let mode = super::after_capture_mode();
            if dword("CopyEveryCapture", 1) != 0 || mode == 3 {
                clip::copy_files(env.hwnd, &[path.clone()]); // videos: the file
            }
            if let Some(i) = env.ipc {
                i.send_int(ipc::Channel::Editor, ipc::code::LIBRARY_CHANGED, 0, false);
            }
            let overlay = mode == 1 || mode == 2;
            if overlay && !first.is_empty() {
                let name = std::path::Path::new(&path).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
                crate::quick::show(&first, &path, true, &name);
            }
            if self.preset.preview_in_editor && (mode == 0 || mode == 2) {
                super::deliver_to_editor(env, &path);
            } else if !overlay {
                let name = std::path::Path::new(&path).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
                env.notify("Snapnab", &(tr("Vídeo salvo na biblioteca: ") + &name));
            }
        } else {
            let (t, c) = (tr("A gravação falhou.\n\n") + &self.video.rec.last_error(), "Snapnab".to_string());
            std::thread::spawn(move || unsafe {
                let (t, c) = (snapcore::wide::to_wide(&t), snapcore::wide::to_wide(&c));
                MessageBoxW(None, PCWSTR(t.as_ptr()), PCWSTR(c.as_ptr()), MB_ICONERROR);
            });
        }
        self.set_state(State::Idle);
    }

    pub(super) fn on_recorder_note(&mut self, ctl: &mut Ctl, env: &Env, n: Note) {
        match n {
            Note::DiskLow => env.notify(&tr("Snapnab — pouco espaço em disco"), &tr("Restam menos de 1 GB livres na unidade da biblioteca.")),
            Note::DiskFull => {
                env.notify("Snapnab", &tr("Gravação interrompida: espaço em disco insuficiente."));
                self.video_stop(ctl, env);
            }
            Note::Error => self.video_stop(ctl, env),
        }
    }

    fn toggle_volume(&mut self, ctl: &mut Ctl) {
        if let Some((k, _)) = self.video.vol.take() {
            ctl.close(k);
            self.video.view.volume_open = false;
            return;
        }
        let Some(hw) = self.video.hud.as_ref().map(|h| h.hwnd) else { return };
        let (w, _) = hud::size(&self.video.view);
        let spec = WindowSpec {
            class: "SnapnabHudVolume",
            title: tr("Volume"),
            size: (w.max(330.0), hud::VOLUME_HEIGHT),
            visible: false,
            decorations: false,
            resizable: false,
            topmost: true,
            skip_taskbar: true,
            intercept: Some(no_activate),
            clear_color: [30.0 / 255.0, 30.0 / 255.0, 32.0 / 255.0, 1.0],
            ..Default::default()
        };
        let Some(k) = ctl.create(&spec) else { return };
        let hwnd = ctl.hwnd(k);
        let sf = ctl.window(k).map_or(1.0, |w| w.scale_factor());
        let (pw, ph) = ((w.max(330.0) as f64 * sf) as i32, (hud::VOLUME_HEIGHT as f64 * sf) as i32);
        let (x, y) = Self::volume_pos(hw, ph, sf);
        float_window(hwnd, x, y, pw, ph, (14.0 * sf) as i32);
        self.video.vol = Some((k, hwnd));
        self.video.view.volume_open = true;
    }

    /// Below the HUD, or above it when there is no room.
    fn volume_pos(hud_hwnd: isize, vol_h: i32, sf: f64) -> (i32, i32) {
        let hr = window_rect(hud_hwnd);
        let mon = screen::monitor_rect(hr.left, hr.top, true);
        let mut y = hr.bottom + (6.0 * sf) as i32;
        if y + vol_h > mon.bottom {
            y = hr.top - vol_h - (6.0 * sf) as i32;
        }
        (hr.left, y)
    }

    pub(super) fn video_ui(&mut self, ctl: &mut Ctl, env: &Env, k: WinKey, ui: &mut egui::Ui) {
        ui.ctx().request_repaint_after(Duration::from_millis(150));
        if self.video.vol.is_some_and(|v| v.0 == k) {
            let (ml, sl) = (self.video.rec.mic_level(), self.video.rec.sys_level());
            for a in hud::draw_volume(ui, &mut self.video.view, ml, sl) {
                let hud::VolAction::Changed = a;
                let v = &self.video.view;
                self.video.rec.set_mic_volume(v.mic_volume);
                self.video.rec.set_sys_volume(v.sys_volume);
                self.video.rec.set_auto_gain(v.auto_gain);
                let mut p = self.preset.clone();
                (p.mic_volume, p.sys_volume, p.auto_gain) = (v.mic_volume, v.sys_volume, v.auto_gain);
                self.preset = p.clone();
                self.video.volumes = Some(p);
            }
            return;
        }
        let Some(hw) = self.video.hud.as_ref().filter(|h| h.key == k).map(|h| h.hwnd) else { return };
        // the countdown ticks with the HUD's frames
        if let Some(c) = &self.video.count {
            let left = c.secs - c.start.elapsed().as_secs() as i32;
            if left <= 0 || rawwin::COUNT_SKIP.swap(false, std::sync::atomic::Ordering::Relaxed) {
                self.skip_countdown(ctl, env);
            } else {
                rawwin::set_count(c.hwnd, left);
            }
        }
        // the volume panel follows the HUD
        if let Some((vk, vh)) = self.video.vol {
            let sf = ctl.window(vk).map_or(1.0, |w| w.scale_factor());
            let vr = window_rect(vh);
            let (x, y) = Self::volume_pos(hw, vr.bottom - vr.top, sf);
            if (x, y) != (vr.left, vr.top) {
                unsafe {
                    let _ = SetWindowPos(HWND(vh as _), Some(HWND_TOPMOST), x, y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
                }
            }
        }
        let st = hud_status(&self.video.rec, &self.video.view);
        let actions = hud::draw(ui, &self.video.view, &st);
        let idle = self.video.rec.state() == RecState::Idle;
        for a in actions {
            match a {
                hud::Action::Drag => {
                    if let Some(w) = ctl.window(k) {
                        let _ = w.drag_window();
                    }
                }
                hud::Action::Click(b) => match b {
                    Btn::Record => self.video_start_pause_resume(ctl, env),
                    Btn::Stop if !idle => self.video_stop(ctl, env),
                    Btn::Restart if !idle => self.restart(ctl, env),
                    Btn::Cancel => self.video_cancel(ctl),
                    Btn::Mic => {
                        if idle {
                            self.video.view.mic_on = !self.video.view.mic_on;
                        } else if self.video.rec.has_mic() {
                            self.video.rec.set_mic_muted(!self.video.rec.mic_muted());
                        }
                    }
                    Btn::Sys => {
                        if idle {
                            self.video.view.sys_on = !self.video.view.sys_on;
                        } else if self.video.rec.has_sys() {
                            self.video.rec.set_sys_muted(!self.video.rec.sys_muted());
                        }
                    }
                    Btn::Cam => {
                        self.video.view.cam_on = !self.video.view.cam_on;
                        crate::camwin::set_visible(self.video.view.cam_on);
                    }
                    Btn::Snapshot if !idle => self.snapshot(env),
                    Btn::Volume => self.toggle_volume(ctl),
                    Btn::Zoom if !idle => self.video_toggle_zoom(),
                    Btn::Overlay => {
                        // the menu opens below the button
                        let sf = ctl.window(k).map_or(1.0, |w| w.scale_factor());
                        let r = hud::buttons(&self.video.view).into_iter().find(|b| b.0 == Btn::Overlay).map(|b| b.1);
                        let hr = window_rect(hw);
                        if let Some(r) = r {
                            self.video_overlay_menu((hr.left + (r.left() as f64 * sf) as i32, hr.top + (r.bottom() as f64 * sf) as i32));
                        }
                    }
                    _ => {}
                },
            }
            if self.video.hud.is_none() {
                break;
            }
        }
    }

    pub(super) fn video_close_requested(&mut self, ctl: &mut Ctl, k: WinKey) {
        if self.video.vol.is_some_and(|v| v.0 == k) {
            self.toggle_volume(ctl);
        } else {
            self.video_cancel(ctl);
        }
    }
}
