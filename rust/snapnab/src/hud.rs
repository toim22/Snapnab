//! The recording HUD (RecorderHud.cpp: RecorderUI / snagitminirecordinghud, comp_3 §3.3) and its volume panel, drawn
//! in egui. The layout is the C++ one in points (36 pt buttons, 4 pt apart, 8 pt of padding, 10 pt between groups),
//! which the e2e suite clicks by position. What the buttons do is up to the engine: `draw` only returns actions.

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Ui, WidgetInfo, pos2, vec2};
use snapui::icons::{self, Icon};

use crate::tr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Btn {
    Record,
    Stop,
    Restart,
    Cancel,
    Mic,
    Sys,
    Volume,
    Cam,
    Snapshot,
    Zoom,
    Overlay,
}

/// What the recorder is doing, for the HUD's look.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecState {
    Idle,
    Recording,
    Paused,
}

/// The recorder as the HUD shows it.
#[derive(Clone, Debug)]
pub struct Status {
    pub state: RecState,
    pub elapsed: f64,
    pub mic_level: f32,
    pub sys_level: f32,
    pub mic_muted: bool,
    pub sys_muted: bool,
    pub has_mic: bool,
    pub has_sys: bool,
    pub zoomed: bool,
    /// cursor highlight, clicks or keys are on (C07)
    pub overlays: bool,
    /// the recording dot blinks: on in this half second
    pub blink: bool,
}

impl Default for Status {
    fn default() -> Self {
        Status {
            state: RecState::Idle,
            elapsed: 0.0,
            mic_level: 0.0,
            sys_level: 0.0,
            mic_muted: false,
            sys_muted: false,
            has_mic: false,
            has_sys: false,
            zoomed: false,
            overlays: false,
            blink: false,
        }
    }
}

/// The HUD's own state: the audio choices before the recording, the webcam button and the volume panel.
#[derive(Clone, Debug)]
pub struct View {
    pub mic_on: bool,
    pub sys_on: bool,
    /// the webcam button shows only when the preset turned the webcam on
    pub cam_available: bool,
    pub cam_on: bool,
    /// audio only (C32): no video buttons
    pub audio_only: bool,
    pub mic_volume: i32,
    pub sys_volume: i32,
    pub auto_gain: bool,
    pub volume_open: bool,
    vol_drag: Option<usize>,
}

impl Default for View {
    fn default() -> Self {
        View {
            mic_on: true,
            sys_on: true,
            cam_available: false,
            cam_on: false,
            audio_only: false,
            mic_volume: 100,
            sys_volume: 100,
            auto_gain: true,
            volume_open: false,
            vol_drag: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    Click(Btn),
    /// the background was pressed: the window moves with the mouse
    Drag,
}

const B: f32 = 36.0;
const GAP: f32 = 4.0;
const PAD: f32 = 8.0;
const SEP: f32 = 10.0;
pub const HEIGHT: f32 = 52.0;

/// RecorderHud::Layout: the buttons and where they are, in points.
pub fn buttons(v: &View) -> Vec<(Btn, Rect)> {
    let mut out = Vec::new();
    let mut x = PAD;
    let mut add = |b: Btn, x: &mut f32| {
        out.push((b, Rect::from_min_size(pos2(*x, PAD), vec2(B, B))));
        *x += B + GAP;
    };
    for b in [Btn::Record, Btn::Stop, Btn::Restart, Btn::Cancel] {
        add(b, &mut x);
    }
    x += SEP;
    for b in [Btn::Mic, Btn::Sys, Btn::Volume] {
        add(b, &mut x);
    }
    if !v.audio_only {
        if v.cam_available {
            add(Btn::Cam, &mut x);
        }
        x += SEP;
        for b in [Btn::Snapshot, Btn::Zoom, Btn::Overlay] {
            add(b, &mut x);
        }
    }
    out
}

/// The HUD's size in points: the buttons, the meters and the time.
pub fn size(v: &View) -> (f32, f32) {
    (buttons(v).last().map_or(0.0, |b| b.1.right()) + 210.0, HEIGHT)
}

/// The name of a button (its tooltip), with its shortcut when it has one.
pub fn tip(b: Btn, key: impl Fn(Btn) -> Option<String>) -> String {
    let k = key(b).map(|k| format!(" ({k})")).unwrap_or_default();
    match b {
        Btn::Record => tr("Gravar / pausar") + &k,
        Btn::Stop => tr("Parar e salvar") + &k,
        Btn::Restart => tr("Recomeçar a gravação"),
        Btn::Cancel => tr("Cancelar (descarta a gravação)"),
        Btn::Mic => tr("Microfone: liga / desliga"),
        Btn::Sys => tr("Áudio do sistema: liga / desliga"),
        Btn::Volume => tr("Volume do microfone e do sistema"),
        Btn::Cam => tr("Webcam: mostra / oculta"),
        Btn::Snapshot => tr("Capturar uma imagem sem parar a gravação"),
        Btn::Zoom => tr("Zoom em volta do ponteiro") + &k,
        Btn::Overlay => tr("No vídeo: destaque do cursor, cliques e teclas"),
    }
}

const ACCENT: Color32 = Color32::from_rgb(120, 190, 255);

fn meter(ui: &Ui, x: f32, y: f32, w: f32, h: f32, lv: f32, on: bool) {
    let p = ui.painter();
    p.rect_filled(Rect::from_min_size(pos2(x, y), vec2(w, h)), 0.0, Color32::from_rgb(60, 60, 60));
    if on {
        let c = if lv > 0.9 { Color32::from_rgb(240, 70, 70) } else { Color32::from_rgb(80, 200, 110) };
        p.rect_filled(Rect::from_min_size(pos2(x, y), vec2(w * lv.min(1.0), h)), 0.0, c);
    }
}

/// Paints the HUD and returns what was clicked (RecorderHud::Paint and its WM_LBUTTONUP).
pub fn draw(ui: &mut Ui, v: &View, st: &Status) -> Vec<Action> {
    let mut out = Vec::new();
    let full = ui.max_rect();
    ui.painter().rect_filled(full, 0.0, Color32::from_rgb(30, 30, 32));
    // the background drags the window (HTCAPTION in the C++); the buttons are on top of it
    let bg = ui.interact(full, ui.scope_id().with("hud-bg"), Sense::click_and_drag());
    if bg.drag_started() {
        out.push(Action::Drag);
    }
    let idle = st.state == RecState::Idle;
    let bts = buttons(v);
    for &(b, r) in &bts {
        let r = r.translate(full.min.to_vec2());
        let mut enabled = true;
        let mut col = Color32::from_rgb(235, 235, 235);
        let icon = match b {
            Btn::Record if st.state == RecState::Recording => Icon::Pause,
            Btn::Record => {
                col = Color32::from_rgb(240, 70, 70);
                Icon::Record
            }
            Btn::Stop => {
                enabled = !idle;
                Icon::Stop
            }
            Btn::Restart => {
                enabled = !idle;
                Icon::Restart
            }
            Btn::Cancel => Icon::Cancel,
            Btn::Mic => {
                enabled = idle || st.has_mic;
                if st.mic_muted || !v.mic_on { Icon::MicOff } else { Icon::Mic }
            }
            Btn::Sys => {
                enabled = idle || st.has_sys;
                if st.sys_muted || !v.sys_on { Icon::SpeakerOff } else { Icon::Speaker }
            }
            Btn::Cam => if v.cam_on { Icon::Webcam } else { Icon::WebcamOff },
            Btn::Snapshot => {
                enabled = !idle;
                Icon::Camera
            }
            Btn::Volume => {
                if v.volume_open {
                    col = ACCENT;
                }
                Icon::Sliders
            }
            Btn::Zoom => {
                enabled = !idle;
                if st.zoomed {
                    col = ACCENT;
                }
                Icon::Focus
            }
            Btn::Overlay => {
                if st.overlays {
                    col = ACCENT;
                }
                Icon::Cursor
            }
        };
        let resp = ui.interact(r, ui.scope_id().with(("hud-btn", b as u8)), Sense::click());
        let name = tip(b, |_| None);
        resp.widget_info(|| WidgetInfo::labeled(egui::Role::Button, enabled, &name));
        if resp.hovered() && enabled {
            ui.painter().rect_filled(r, 0.0, Color32::from_rgb(60, 60, 66));
        }
        if !enabled {
            col = Color32::from_rgb(100, 100, 100);
        }
        icons::draw(ui.painter(), icon, r.shrink(8.0), col);
        if resp.clicked() {
            out.push(Action::Click(b));
        }
    }
    // the level meters (before the recording they come from the monitor and follow the mic and system buttons)
    let last = bts.last().map_or(0.0, |b| b.1.right());
    let x0 = full.left() + last + 14.0;
    meter(ui, x0, full.top() + 17.0, 60.0, 5.0, st.mic_level, if idle { v.mic_on } else { !st.mic_muted });
    meter(ui, x0, full.top() + 29.0, 60.0, 5.0, st.sys_level, if idle { v.sys_on } else { !st.sys_muted });
    // the time
    let s = st.elapsed as i64;
    let t = match st.state {
        RecState::Idle => tr("Pronto"),
        RecState::Paused => format!("{:02}:{:02}:{:02}  ❚❚", s / 3600, (s / 60) % 60, s % 60),
        RecState::Recording => format!("{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60),
    };
    let tl = full.left() + last + 84.0;
    if st.state == RecState::Recording && st.blink {
        ui.painter().circle_filled(pos2(tl, full.center().y), 4.0, Color32::from_rgb(240, 70, 70));
    }
    ui.painter().text(pos2(tl + 8.0, full.center().y), Align2::LEFT_CENTER, t, FontId::proportional(14.5),
                      Color32::from_rgb(240, 240, 240));
    out
}

// ---------------------------------------------------------------- the volume panel
pub const VOLUME_HEIGHT: f32 = 108.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VolAction {
    /// the volumes or the automatic gain changed (saved in the preset)
    Changed,
}

/// The slider tracks in a panel `w` points wide (0 microphone, 1 system).
fn track(which: usize, w: f32) -> Rect {
    let y = if which == 0 { 22.0 } else { 54.0 };
    Rect::from_min_max(pos2(118.0, y - 6.0), pos2(w - 58.0, y + 6.0))
}

fn check_rect() -> Rect {
    Rect::from_min_max(pos2(12.0, 76.0), pos2(300.0, 98.0))
}

fn set_from_x(v: &mut View, which: usize, x: f32, w: f32) {
    let t = track(which, w);
    let mut val = (((x - t.left()) / t.width().max(1.0)).clamp(0.0, 1.0) * 200.0).round() as i32;
    if (val - 100).abs() <= 4 {
        val = 100; // sticks to 100 %
    }
    if which == 0 { v.mic_volume = val } else { v.sys_volume = val }
}

/// The volume panel below (or above) the HUD: a slider per source (0–200 %) and the automatic microphone gain. It
/// works before the recording (the meters come from the monitor) and during it.
pub fn draw_volume(ui: &mut Ui, v: &mut View, mic_level: f32, sys_level: f32) -> Vec<VolAction> {
    let mut out = Vec::new();
    let full = ui.max_rect();
    let o = full.min.to_vec2();
    let w = full.width();
    let p = ui.painter().clone();
    p.rect_filled(full, 0.0, Color32::from_rgb(30, 30, 32));
    let area = ui.interact(full, ui.scope_id().with("vol"), Sense::click_and_drag());
    area.widget_info(|| WidgetInfo::labeled(egui::Role::Slider, true, tr("Volume")));
    let at = |q: Pos2| q - o;
    if area.drag_started() || area.clicked() {
        if let Some(q) = area.interact_pointer_pos().map(at) {
            if let Some(which) = (0..2).find(|&i| track(i, w).expand2(vec2(10.0, 8.0)).contains(q)) {
                v.vol_drag = Some(which);
                set_from_x(v, which, q.x, w);
                out.push(VolAction::Changed);
            } else if check_rect().contains(q) && area.clicked() {
                v.auto_gain = !v.auto_gain;
                out.push(VolAction::Changed);
            }
        }
    } else if area.dragged()
        && let (Some(which), Some(q)) = (v.vol_drag, area.interact_pointer_pos().map(at))
    {
        set_from_x(v, which, q.x, w);
        out.push(VolAction::Changed);
    }
    if area.drag_stopped() || area.clicked() {
        v.vol_drag = None;
    }
    let font = FontId::proportional(12.0);
    for which in 0..2 {
        let t = track(which, w).translate(o);
        let cy = t.center().y;
        let label = if which == 0 { tr("Microfone") } else { tr("Áudio do sistema") };
        p.text(pos2(full.left() + 12.0, cy), Align2::LEFT_CENTER, label, font.clone(), Color32::from_rgb(230, 230, 232));
        // the level after the volume, under the track
        let lv = if which == 0 { mic_level } else { sys_level };
        let bar = Rect::from_min_size(pos2(t.left(), cy - 3.0), vec2(t.width(), 6.0));
        p.rect_filled(bar, 0.0, Color32::from_rgb(64, 64, 68));
        let c = if lv > 0.9 { Color32::from_rgb(240, 70, 70) } else { Color32::from_rgb(80, 200, 110) };
        p.rect_filled(Rect::from_min_size(bar.min, vec2(t.width() * lv.min(1.0), 6.0)), 0.0, c);
        let mx = t.left() + t.width() / 2.0; // the 100 % mark
        p.line_segment([pos2(mx, t.top() - 2.0), pos2(mx, t.bottom() + 2.0)], (1.0, Color32::from_white_alpha(120)));
        let val = if which == 0 { v.mic_volume } else { v.sys_volume };
        p.circle_filled(pos2(t.left() + t.width() * val as f32 / 200.0, cy), 7.0, Color32::from_rgb(240, 240, 242));
        p.text(pos2(full.right() - 8.0, cy), Align2::RIGHT_CENTER, format!("{val}%"), font.clone(), Color32::from_rgb(230, 230, 232));
    }
    let c = check_rect().translate(o);
    let bx = Rect::from_min_size(pos2(c.left(), c.center().y - 7.0), vec2(14.0, 14.0));
    p.rect_filled(bx, 0.0, if v.auto_gain { Color32::from_rgb(70, 150, 240) } else { Color32::from_rgb(70, 70, 74) });
    if v.auto_gain {
        let pt = |x: f32, y: f32| bx.min + vec2(14.0 * x, 14.0 * y);
        p.add(egui::Shape::line(vec![pt(0.2, 0.52), pt(0.42, 0.74), pt(0.8, 0.28)], (2.0, Color32::WHITE)));
    }
    p.text(pos2(bx.right() + 8.0, c.center().y), Align2::LEFT_CENTER, tr("Ganho automático do microfone"), font,
           Color32::from_rgb(210, 210, 214));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_matches_the_cpp_hud() {
        let v = View::default();
        let b = buttons(&v);
        let names: Vec<Btn> = b.iter().map(|x| x.0).collect();
        assert_eq!(names, [Btn::Record, Btn::Stop, Btn::Restart, Btn::Cancel, Btn::Mic, Btn::Sys, Btn::Volume, Btn::Snapshot,
                           Btn::Zoom, Btn::Overlay]);
        // the mic button: 8 + 4 × 40 + 10
        assert_eq!(b[4].1.left(), 178.0);
        assert_eq!(size(&v), (b[9].1.right() + 210.0, 52.0));
        let audio = View { audio_only: true, ..View::default() };
        assert_eq!(buttons(&audio).len(), 7);
    }
}
