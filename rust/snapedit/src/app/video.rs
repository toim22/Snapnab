//! The video view (src/editor/Video.cpp VideoView): the frame, a timeline with the cut ranges and the selection,
//! playback with the audio, frame steps, marking and cutting ranges, saving the MP4 without the cuts, a GIF of the
//! selection and a frame as a new image. It is drawn inside the main window (the C++ view was a child window); the
//! buttons post WM_COMMAND B_* (2001–2011) to the main window, so automation can press them the same way.

use std::time::Instant;

use egui::{Color32, RichText};
use snapcore::image::Image;
use snapcore::video::{AudioPlayer, HNS, Interval, VideoReader, fmt_time, is_inside_any_cut, normalize};
use snapedit_app::canvas_ui::texture;
use snapedit_app::ids::*;
use snapedit_app::{native, panels, tr};
use snapui::form::{self, Field};
use snapui::host::Ctl;
use windows::Win32::UI::WindowsAndMessaging::{MB_ICONINFORMATION, MB_ICONWARNING};

use super::Editor;
use super::docs::fmt;

#[derive(Default)]
pub struct VideoView {
    reader: Option<VideoReader>,
    pub path: String,
    frame: Image,
    tex: Option<egui::TextureHandle>,
    dirty: bool,
    pos: i64,
    sel_a: i64,
    sel_b: i64,
    cuts: Vec<Interval>,
    cut_undo: Vec<Vec<Interval>>,
    playing: bool,
    play_from: i64,
    play_start: Option<Instant>,
    pending: Option<(Image, i64)>,
    audio: Option<AudioPlayer>,
    scrubbing: bool,
    last_scrub: Option<Instant>,
}

fn strip_ext(p: &str) -> String {
    let path = std::path::Path::new(p);
    match path.extension() {
        Some(_) => path.with_extension("").to_string_lossy().into_owned(),
        None => p.to_string(),
    }
}

impl VideoView {
    pub fn open(&mut self, p: &str) -> bool {
        self.close();
        self.path = p.to_string();
        self.reader = VideoReader::open(p);
        self.pos = 0;
        self.sel_a = -1;
        self.sel_b = -1;
        self.cuts.clear();
        self.cut_undo.clear();
        if self.reader.is_none() {
            return false;
        }
        self.show_at(0);
        true
    }

    pub fn close(&mut self) {
        self.stop_play();
        self.reader = None;
        self.frame = Image::default();
        self.tex = None;
    }

    fn duration(&self) -> i64 {
        self.reader.as_ref().map_or(0, |r| r.duration)
    }

    fn frame_time(&self) -> i64 {
        self.reader.as_ref().map_or(HNS / 30, |r| (HNS as f64 / r.fps.max(1.0)) as i64)
    }

    /// Decodes from the key frame until it reaches t.
    fn show_at(&mut self, t: i64) {
        let ft = self.frame_time();
        let Some(rd) = self.reader.as_mut() else { return };
        let t = t.clamp(0, (rd.duration - 1).max(0));
        rd.seek(t);
        let mut last = 0;
        for _ in 0..600 {
            let Some((img, ts)) = rd.read_frame() else { break };
            self.frame = img;
            self.dirty = true;
            last = ts;
            if ts + ft > t {
                break;
            }
        }
        self.pos = t.max(last);
    }

    fn step(&mut self, dir: i64) {
        self.stop_play();
        self.show_at(self.pos + dir * self.frame_time());
    }

    fn toggle_play(&mut self) {
        if self.playing {
            self.stop_play();
            return;
        }
        if self.reader.is_none() {
            return;
        }
        if self.pos >= self.duration() - HNS / 10 {
            self.show_at(0);
        }
        self.playing = true;
        if let Some(r) = self.reader.as_mut() {
            r.seek(self.pos);
        }
        self.pending = None;
        self.play_from = self.pos;
        self.play_start = Some(Instant::now());
        self.audio = Some(AudioPlayer::start(&self.path, self.pos, normalize(&self.cuts)));
    }

    pub fn stop_play(&mut self) {
        if !self.playing {
            return;
        }
        self.playing = false;
        self.audio = None;
        self.pending = None;
    }

    /// The wall clock as source video time, skipping the cut ranges (like the audio).
    fn play_clock(&self) -> i64 {
        let el = self.play_start.map_or(0, |s| s.elapsed().as_micros() as i64 * 10);
        let mut t = self.play_from + el;
        for c in normalize(&self.cuts) {
            if c.end > self.play_from && c.start <= t {
                t += c.end - c.start.max(self.play_from);
            }
        }
        t
    }

    /// Shows the most recent frame the clock has reached.
    fn tick(&mut self) {
        let clock = self.play_clock();
        let cuts = normalize(&self.cuts);
        let mut shown = false;
        for _ in 0..16 {
            if self.pending.is_none() {
                let Some(rd) = self.reader.as_mut() else { return };
                let Some((img, ts)) = rd.read_frame() else {
                    if !shown {
                        self.stop_play();
                    }
                    break;
                };
                if let Some(c) = cuts.iter().find(|c| ts >= c.start && ts < c.end) {
                    rd.seek(c.end);
                    continue;
                }
                self.pending = Some((img, ts));
            }
            if self.pending.as_ref().is_some_and(|(_, ts)| *ts > clock) {
                break;
            }
            if let Some((img, ts)) = self.pending.take() {
                self.frame = img;
                self.pos = ts;
                self.dirty = true;
                shown = true;
            }
        }
    }

    fn selection(&self) -> (i64, i64) {
        let a = if self.sel_a >= 0 { self.sel_a } else { 0 };
        let b = if self.sel_b >= 0 { self.sel_b } else { self.duration() };
        (a.min(b), a.max(b))
    }

    fn info(&self) -> String {
        let Some(r) = &self.reader else { return String::new() };
        let mut s = format!("{} / {}", fmt_time(self.pos), fmt_time(r.duration))
            + &fmt(&tr("   •   %d×%d @ %.0f qps"), &[&r.width, &r.height, &format!("{:.0}", r.fps)]);
        if !self.cuts.is_empty() {
            let kept = r.duration - normalize(&self.cuts).iter().map(|c| c.end - c.start).sum::<i64>();
            s += &(tr("   •   duração após cortes: ") + &fmt_time(kept));
        }
        if self.sel_a >= 0 || self.sel_b >= 0 {
            let b = if self.sel_b >= 0 { self.sel_b } else { r.duration };
            s += &format!("{}{} – {}", tr("   •   seleção: "), fmt_time(self.sel_a.max(0)), fmt_time(b));
        }
        s
    }
}

const BUTTONS: [(u32, &str); 11] = [
    (B_PLAY, "▶ Reproduzir"),
    (B_PREV, "◀ Quadro"),
    (B_NEXT, "Quadro ▶"),
    (B_MARKIN, "[ Início"),
    (B_MARKOUT, "Fim ]"),
    (B_CUT, "✂ Cortar seleção"),
    (B_UNDO, "Desfazer corte"),
    (B_SAVEFRAME, "Salvar quadro"),
    (B_GIF, "Criar GIF…"),
    (B_SAVEMP4, "Salvar MP4…"),
    (B_PLAYER, "Abrir no player"),
];

impl Editor {
    pub fn video_ui(&mut self, ctl: &mut Ctl, ui: &mut egui::Ui) {
        let _ = ctl;
        let v = &mut self.video;
        if v.playing {
            v.tick();
            let fps = v.reader.as_ref().map_or(30.0, |r| r.fps.max(1.0));
            ui.ctx().request_repaint_after(std::time::Duration::from_millis((500.0 / fps).max(10.0) as u64));
        }
        let full = ui.available_rect_before_wrap();
        ui.painter().rect_filled(full, 0.0, Color32::from_rgb(32, 33, 36));
        let resp = ui.allocate_rect(full, egui::Sense::click_and_drag());
        panels::tag(ui, &resp, "SnapnabVideoView");
        if v.reader.is_none() {
            ui.painter().text(
                full.center(),
                egui::Align2::CENTER_CENTER,
                tr("Não foi possível abrir o vídeo (somente MP4)."),
                egui::FontId::proportional(15.0),
                Color32::from_gray(220),
            );
            return;
        }
        let tl = egui::Rect::from_min_max(egui::pos2(full.left() + 16.0, full.bottom() - 112.0), egui::pos2(full.right() - 16.0, full.bottom() - 76.0));
        let area = egui::Rect::from_min_max(egui::pos2(full.left() + 16.0, full.top() + 12.0), egui::pos2(full.right() - 16.0, tl.top() - 26.0));
        // the frame
        if v.dirty || v.tex.is_none() {
            v.tex = Some(texture(ui.ctx(), "video-frame", &v.frame, true));
            v.dirty = false;
        }
        if let Some(t) = &v.tex
            && !v.frame.is_empty()
            && area.width() > 0.0
            && area.height() > 0.0
        {
            let sz = t.size_vec2();
            let s = (area.width() / sz.x).min(area.height() / sz.y);
            let r = egui::Rect::from_center_size(area.center(), sz * s);
            ui.painter().image(t.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            if is_inside_any_cut(&v.cuts, v.pos) {
                let tag = egui::Rect::from_min_size(area.min + egui::vec2(8.0, 8.0), egui::vec2(152.0, 26.0));
                ui.painter().rect_filled(tag, 8.0, Color32::from_rgb(200, 40, 40));
                ui.painter().text(tag.center(), egui::Align2::CENTER_CENTER, tr("trecho cortado"), egui::FontId::proportional(12.0), Color32::WHITE);
            }
        }
        // the timeline
        let dur = v.duration().max(1);
        let x_of = |t: i64| tl.left() + (t as f32 / dur as f32) * tl.width();
        let p = ui.painter();
        p.rect_filled(tl, 0.0, Color32::from_rgb(70, 72, 78));
        for c in normalize(&v.cuts) {
            let r = egui::Rect::from_min_max(egui::pos2(x_of(c.start), tl.top()), egui::pos2(x_of(c.end), tl.bottom()));
            p.rect_filled(r, 0.0, Color32::from_rgb(120, 30, 30));
            let mut x = r.left() - tl.height();
            while x < r.right() {
                let a = egui::pos2(x.max(r.left()), tl.bottom() - (x.max(r.left()) - x));
                let bx = (x + tl.height()).min(r.right());
                let b = egui::pos2(bx, tl.bottom() - (bx - x));
                p.line_segment([a, b], egui::Stroke::new(1.5, Color32::from_rgb(230, 70, 70)));
                x += 7.0;
            }
        }
        if v.sel_a >= 0 || v.sel_b >= 0 {
            let (a, b) = v.selection();
            p.rect_filled(egui::Rect::from_min_max(egui::pos2(x_of(a), tl.top() - 4.0), egui::pos2(x_of(b), tl.top())), 0.0, Color32::from_rgb(60, 150, 255));
        }
        let px = x_of(v.pos);
        p.rect_filled(egui::Rect::from_min_max(egui::pos2(px - 1.0, tl.top() - 6.0), egui::pos2(px + 2.0, tl.bottom() + 6.0)), 0.0, Color32::WHITE);
        p.text(egui::pos2(tl.left(), tl.bottom() + 11.0), egui::Align2::LEFT_CENTER, v.info(), egui::FontId::proportional(12.0), Color32::from_gray(220));
        // scrubbing
        let hit = tl.expand2(egui::vec2(0.0, 10.0));
        let time_at = |x: f32| (((x - tl.left()) / tl.width().max(1.0)).clamp(0.0, 1.0) as f64 * dur as f64) as i64;
        if resp.drag_started() || resp.clicked() {
            if let Some(pp) = resp.interact_pointer_pos()
                && hit.contains(pp)
            {
                v.stop_play();
                v.scrubbing = true;
                v.last_scrub = Some(Instant::now());
                v.show_at(time_at(pp.x));
            }
        }
        if v.scrubbing && resp.dragged()
            && let Some(pp) = resp.interact_pointer_pos()
            && v.last_scrub.is_none_or(|t| t.elapsed().as_millis() > 60)
        {
            v.last_scrub = Some(Instant::now());
            v.show_at(time_at(pp.x));
        }
        if v.scrubbing && (resp.drag_stopped() || resp.clicked()) {
            v.scrubbing = false;
            if let Some(pp) = resp.interact_pointer_pos() {
                v.show_at(time_at(pp.x));
            }
        }
        // the buttons
        let row = egui::Rect::from_min_max(egui::pos2(full.left() + 16.0, full.bottom() - 46.0), egui::pos2(full.right() - 16.0, full.bottom() - 12.0));
        let playing = v.playing;
        let mut clicked = None;
        ui.scope_builder(egui::UiBuilder::new().max_rect(row), |ui| {
            ui.horizontal(|ui| {
                for (id, label) in BUTTONS {
                    let text = if id == B_PLAY && playing { tr("❚❚ Pausar") } else { tr(label) };
                    let r = ui.button(RichText::new(text));
                    panels::tag(ui, &r, &format!("SnapnabVideoView.{}", id - 2000));
                    if r.clicked() {
                        clicked = Some(id);
                    }
                }
            });
        });
        if let Some(id) = clicked {
            native::post_command(self.hwnd, id);
        }
    }

    /// Space plays or pauses, ←/→ step one frame, I/O mark the start and the end.
    pub fn video_key(&mut self, ctl: &mut Ctl, key: egui::Key) -> bool {
        let id = match key {
            egui::Key::Space => B_PLAY,
            egui::Key::ArrowLeft => B_PREV,
            egui::Key::ArrowRight => B_NEXT,
            egui::Key::I => B_MARKIN,
            egui::Key::O => B_MARKOUT,
            _ => return false,
        };
        self.video_command(ctl, id);
        true
    }

    pub fn video_command(&mut self, ctl: &mut Ctl, id: u32) {
        let _ = ctl;
        let v = &mut self.video;
        if v.reader.is_none() && id != B_PLAYER {
            return;
        }
        match id {
            B_PLAY => v.toggle_play(),
            B_PREV => v.step(-1),
            B_NEXT => v.step(1),
            B_MARKIN => {
                v.sel_a = v.pos;
                if v.sel_b >= 0 && v.sel_b < v.sel_a {
                    v.sel_b = -1;
                }
            }
            B_MARKOUT => {
                v.sel_b = v.pos;
                if v.sel_a > v.sel_b {
                    v.sel_a = -1;
                }
            }
            B_CUT => {
                let a = v.sel_a.max(0);
                let b = v.sel_b;
                if b <= a {
                    let owner = self.hwnd;
                    self.modal(
                        move |_| native::message(owner, &tr("Marque o início e o fim do trecho a remover."), "Snapnab", MB_ICONINFORMATION),
                        |_, _, _| {},
                    );
                    return;
                }
                v.cut_undo.push(v.cuts.clone());
                v.cuts.push(Interval { start: a, end: b });
                v.cuts = normalize(&v.cuts);
                v.sel_a = -1;
                v.sel_b = -1;
            }
            B_UNDO => {
                if let Some(c) = v.cut_undo.pop() {
                    v.cuts = c;
                }
            }
            B_SAVEFRAME => {
                if !v.frame.is_empty() {
                    let img = v.frame.clone();
                    self.new_doc_from_image(img, &tr("Quadro do vídeo"), &tr("Quadro de vídeo"));
                }
            }
            B_GIF => self.video_save_gif(),
            B_SAVEMP4 => self.video_save_mp4(),
            B_PLAYER => {
                v.stop_play();
                super::extras::shell_open(&v.path);
            }
            _ => {}
        }
    }

    fn video_save_mp4(&mut self) {
        let v = &mut self.video;
        v.stop_play();
        let (src, cuts) = (v.path.clone(), v.cuts.clone());
        self.modal(
            move |o| {
                let init = strip_ext(&src) + "_editado.mp4";
                let (out, _) = native::save_file(o, &tr("Salvar vídeo"), &init, &tr("Vídeo MP4 (*.mp4)|*.mp4|"), "mp4", 1)?;
                if out.eq_ignore_ascii_case(&src) {
                    native::message(o, &tr("Escolha um arquivo diferente do original."), "Snapnab", MB_ICONWARNING);
                    return None;
                }
                let progress = native::Progress::open(o, &tr("Salvando vídeo…"));
                let r = snapcore::video::transcode_with_cuts(&src, &out, &cuts, &mut |pct| {
                    progress.as_ref().is_none_or(|p| {
                        p.set("", pct.clamp(0, 100) as usize, 100);
                        !p.canceled()
                    })
                });
                drop(progress);
                if let Err(e) = &r {
                    native::message(o, &(tr("Não foi possível salvar o vídeo.\n") + e), "Snapnab", MB_ICONWARNING);
                }
                r.ok().map(|_| out)
            },
            |e, _, r| {
                if let Some(out) = r {
                    e.tray.reload();
                    e.status = tr("Vídeo salvo: ") + &out;
                }
            },
        );
    }

    fn video_save_gif(&mut self) {
        let v = &mut self.video;
        v.stop_play();
        let Some(r) = &v.reader else { return };
        let marked = v.sel_a >= 0 || v.sel_b >= 0;
        let (a, b) = v.selection();
        let (src, width) = (v.path.clone(), r.width);
        self.modal(
            move |o| {
                let mut f = vec![
                    Field::Label(if marked {
                        tr("Intervalo: a seleção marcada.")
                    } else {
                        tr("Intervalo: o vídeo inteiro (marque início/fim para limitar).")
                    }),
                    Field::Int { label: tr("Quadros por segundo"), value: 10, min: 1, max: 30 },
                    Field::Int { label: tr("Largura (px)"), value: width.min(640), min: 32, max: width },
                    Field::Combo { label: tr("Cores"), items: vec![tr("256 cores"), tr("16 cores (arquivo menor)")], value: 0 },
                    Field::Check { label: tr("Pontilhado (dithering)"), value: true },
                    Field::Check { label: tr("Quadros delta (só o que mudou)"), value: true },
                ];
                if !form::run(o, &tr("Criar GIF animado"), &mut f) {
                    return None;
                }
                let (out, _) = native::save_file(o, &tr("Salvar GIF"), &(strip_ext(&src) + ".gif"), &tr("GIF animado (*.gif)|*.gif|"), "gif", 1)?;
                let bpp = if f[3].int() == 1 { 4 } else { 8 };
                let progress = native::Progress::open(o, &tr("Criando GIF…"));
                let r = snapcore::video::create_gif_from_video(&src, &out, a, b, f[1].int(), f[2].int(), bpp, f[4].on(), f[5].on(), &mut |pct| {
                    progress.as_ref().is_none_or(|p| {
                        p.set("", pct.clamp(0, 100) as usize, 100);
                        !p.canceled()
                    })
                });
                drop(progress);
                match r {
                    Ok(()) => {
                        let _ = std::process::Command::new("explorer.exe").arg(format!("/select,{out}")).spawn();
                        Some(out)
                    }
                    Err(e) => {
                        native::message(o, &(tr("Não foi possível criar o GIF.\n") + &e), "Snapnab", MB_ICONWARNING);
                        None
                    }
                }
            },
            |e, _, r| {
                if let Some(out) = r {
                    e.status = tr("GIF salvo: ") + &out;
                }
            },
        );
    }
}
