//! The capture window (comp_1 §2, src/capture/main.cpp): the menus on top; the Image / Video tabs on the left; the
//! selection, effects and share (or quality and webcam) fields; the switches; the round capture button; the bottom
//! bar (⚙ Presets, +, Open Editor) and the preset list below it. Drawing returns what the clicks ask for; the app
//! does it. Each control carries the C++ control's name as its UI Automation id (AutomationId "IDC_SELECTION"…), so
//! the end-to-end tests find them as they found the Win32 controls.

use egui::{Color32, RichText};
use snapcore::fx::CaptureEffects;
use snapcore::i18n;
use snapcore::presets::{CaptureKind, Preset, PresetStore, Selection};

use crate::id;
use crate::tr;

/// The outputs of the "Share" box (one per preset; main.cpp Share).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum Share {
    None = 0,
    File,
    Program,
    Text,
    Pin,
    Qr,
    Word,
    PowerPoint,
    Excel,
    OneNote,
    Upload,
}

impl Share {
    pub const COUNT: i32 = 11;

    pub fn from_i32(i: i32) -> Share {
        use Share::*;
        [None, File, Program, Text, Pin, Qr, Word, PowerPoint, Excel, OneNote, Upload][i.clamp(0, Self::COUNT - 1) as usize]
    }

    pub fn names() -> Vec<String> {
        vec![tr("Nenhum"), tr("Arquivo"), tr("Programa externo"), tr("Texto (OCR)"), tr("Fixar na tela"), tr("Ler código QR"), "Word".into(),
             "PowerPoint".into(), "Excel".into(), "OneNote".into(), tr("Upload (copiar o link)")]
    }

    /// The outputs with settings behind the gear.
    pub fn has_gear(self) -> bool {
        matches!(self, Share::File | Share::Program | Share::Upload | Share::PowerPoint)
    }
}

/// ShareOf
pub fn share_of(p: &Preset) -> Share {
    if p.grab_text {
        Share::Text
    } else if p.read_qr {
        Share::Qr
    } else if p.pin_to_screen {
        Share::Pin
    } else if p.upload {
        Share::Upload
    } else if (0..4).contains(&p.office) {
        Share::from_i32(Share::Word as i32 + p.office)
    } else if p.save_to_file {
        Share::File
    } else if p.open_in_program {
        Share::Program
    } else {
        Share::None
    }
}

/// SetShare: one output at a time.
pub fn set_share(p: &mut Preset, s: Share) {
    p.save_to_file = s == Share::File;
    p.open_in_program = s == Share::Program;
    p.grab_text = s == Share::Text;
    p.pin_to_screen = s == Share::Pin;
    p.read_qr = s == Share::Qr;
    p.upload = s == Share::Upload;
    p.office = if (Share::Word as i32..=Share::OneNote as i32).contains(&(s as i32)) { s as i32 - Share::Word as i32 } else { -1 };
}

/// The effects of the "Effects" box, in the order of the list (main.cpp Fx).
pub const FX_NAMES: [&str; 13] = ["Borda", "Filtros", "Sombra", "Informações da captura", "Substituição de cor", "Redimensionar imagem",
                                  "Bordas especiais", "Cortar bordas", "Ajuste de cor", "Girar imagem", "Marca d'água", "Resolução da imagem",
                                  "Embelezar (fundo, margem e sombra)"];

pub fn fx_on(e: &mut CaptureEffects, k: usize) -> &mut bool {
    match k {
        0 => &mut e.border,
        1 => &mut e.filter,
        2 => &mut e.shadow,
        3 => &mut e.caption,
        4 => &mut e.replace_color,
        5 => &mut e.scale,
        6 => &mut e.edge,
        7 => &mut e.trim,
        8 => &mut e.adjust,
        9 => &mut e.rotate,
        10 => &mut e.watermark,
        11 => &mut e.resolution,
        _ => &mut e.beautify,
    }
}

fn fx_count_on(e: &CaptureEffects) -> usize {
    let mut e = e.clone();
    (0..FX_NAMES.len()).filter(|&k| *fx_on(&mut e, k)).count()
}

fn fx_first_on(e: &CaptureEffects) -> Option<usize> {
    let mut e = e.clone();
    (0..FX_NAMES.len()).find(|&k| *fx_on(&mut e, k))
}

/// What the window shows that is not in the presets.
#[derive(Clone, Debug, Default)]
pub struct View {
    /// the preset list is open below the bottom bar (CapturePresetsOpen)
    pub presets_open: bool,
    /// One Click is on (the View menu's check)
    pub one_click: bool,
    /// the image and video hotkeys, for the capture button's tooltip
    pub image_hotkey: String,
    pub video_hotkey: String,
    /// the webcams, listed when the webcam menu opens (name, link)
    pub cams: Vec<(String, String)>,
    /// the effect the "Effects" box shows (target of "Remove this effect")
    pub fx_current: Option<usize>,
    /// the effects list shows only the effects that are off ("Add another effect")
    pub fx_add_mode: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// the presets changed (to be saved)
    Changed,
    /// a command of the menus (main.cpp IDM_*)
    Command(u32),
    Capture,
    /// the settings of effect k (its form; OK turns it on)
    EditFx(usize),
    /// a new output chosen in the "Share" box (Office not installed / upload not configured are said)
    ShareChosen(Share),
    /// the gear of the "Share" box (file, program, upload or PowerPoint settings)
    ShareGear,
    /// Region › Fixed region…
    FixedRegion,
    /// the gear of the "Quality" box (video options)
    VideoOptions,
    /// Webcam › Remove the background…
    ChromaKey,
    /// the webcam menu opens: the cameras are listed (into `View::cams`)
    ListCameras,
    /// the hotkey label of a preset row (its form)
    PresetHotkey(usize),
    /// the preset list opened or closed (the window grows or shrinks)
    PresetsToggled,
}

/// Tags a widget with the C++ control's name: its UI Automation id.
fn tag(ui: &egui::Ui, r: &egui::Response, name: &str) {
    ui.ctx().accesskit_node_builder(r.id, |n| n.set_author_id(name.to_owned()));
}

fn tag_value(ui: &egui::Ui, r: &egui::Response, name: &str, value: &str) {
    ui.ctx().accesskit_node_builder(r.id, |n| {
        n.set_author_id(name.to_owned());
        n.set_value(value.to_owned());
    });
}

/// The window's height: the content and the bottom bar, plus the preset list when it is open.
pub fn height(store: &PresetStore, view: &View) -> f32 {
    let base = 262.0;
    if view.presets_open { base + 8.0 + 30.0 * store.presets.len().clamp(2, 8) as f32 } else { base }
}

pub const WIDTH: f32 = 760.0;

fn menu_item(ui: &mut egui::Ui, out: &mut Vec<Action>, text: &str, cmd: u32) {
    if ui.button(tr(text)).clicked() {
        out.push(Action::Command(cmd));
        ui.close();
    }
}

fn check_item(ui: &mut egui::Ui, out: &mut Vec<Action>, text: &str, on: bool, cmd: u32) {
    let mut v = on;
    if ui.checkbox(&mut v, tr(text)).clicked() {
        out.push(Action::Command(cmd));
        ui.close();
    }
}

/// BuildMenu: File, Capture, View, Help.
fn menu_bar(ui: &mut egui::Ui, view: &View, out: &mut Vec<Action>) {
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button(tr("Arquivo"), |ui| {
            menu_item(ui, out, "Abrir o Editor", id::OPEN_EDITOR);
            menu_item(ui, out, "Abrir a pasta da biblioteca", id::LIBRARY_FOLDER);
            menu_item(ui, out, "Abrir a pasta de logs", id::LOGS);
            ui.separator();
            menu_item(ui, out, "Importar presets…", id::PRESET_IMPORT);
            menu_item(ui, out, "Exportar presets…", id::PRESET_EXPORT);
            ui.separator();
            menu_item(ui, out, "Exportar todas as configurações…", id::SETTINGS_EXPORT);
            menu_item(ui, out, "Importar todas as configurações…", id::SETTINGS_IMPORT);
            menu_item(ui, out, "Abrir a pasta de backups", id::SETTINGS_BACKUPS);
            ui.separator();
            menu_item(ui, out, "Configurar o upload…", id::UPLOAD_SETTINGS);
            menu_item(ui, out, "Limpar a biblioteca agora", id::CLEANUP_NOW);
            menu_item(ui, out, "Opções…", id::OPTIONS);
            ui.separator();
            menu_item(ui, out, "Sair do Snapnab", id::EXIT);
        });
        ui.menu_button(tr("Captura"), |ui| {
            menu_item(ui, out, "Capturar imagem", id::CAPTURE_IMAGE);
            menu_item(ui, out, "Gravar vídeo", id::CAPTURE_VIDEO);
            menu_item(ui, out, "Repetir a última captura", id::REPEAT);
            ui.separator();
            menu_item(ui, out, "Da área de transferência", id::CAPTURE_CLIPBOARD);
            menu_item(ui, out, "De um arquivo gráfico…", id::CAPTURE_FILE);
            ui.separator();
            menu_item(ui, out, "Conta-gotas de tela", id::COLOR_PICKER);
            menu_item(ui, out, "Ler código QR", id::QR_REGION);
            ui.menu_button(tr("Fixar na tela"), |ui| {
                menu_item(ui, out, "Capturar e fixar", id::CAPTURE_PIN);
                menu_item(ui, out, "Fixar da área de transferência", id::PIN_CLIPBOARD);
                menu_item(ui, out, "Fixar um arquivo…", id::PIN_FILE);
                menu_item(ui, out, "Fechar todas", id::PINS_CLOSE);
            });
            menu_item(ui, out, "Captura por intervalo…", id::INTERVAL);
            menu_item(ui, out, "Foco de tela (apresentações)", id::FOCUS);
            menu_item(ui, out, "Gravar só o áudio", id::RECORD_AUDIO);
            menu_item(ui, out, "Abrir a pasta dos áudios", id::AUDIO_FOLDER);
        });
        ui.menu_button(tr("Exibir"), |ui| {
            check_item(ui, out, "Mostrar o One Click", view.one_click, id::ONECLICK);
            check_item(ui, out, "Lista de presets", view.presets_open, id::SHOW_PRESETS);
        });
        ui.menu_button(tr("Ajuda"), |ui| {
            menu_item(ui, out, "Manual do Snapnab", id::MANUAL);
            menu_item(ui, out, "Como usar os presets", id::PRESET_HELP);
            ui.separator();
            menu_item(ui, out, "Sobre o Snapnab", id::ABOUT);
        });
    });
}

/// The Image / Video tabs (vertical, as in the Windows 11 navigation).
fn tabs(ui: &mut egui::Ui, p: &mut Preset, out: &mut Vec<Action>) {
    for (k, name, cid, icon) in [(CaptureKind::Image, tr("Imagem"), "IDC_TAB_IMAGE", "📷"), (CaptureKind::Video, tr("Vídeo"), "IDC_TAB_VIDEO", "🎬")] {
        let on = p.kind == k;
        let b = egui::Button::selectable(on, RichText::new(format!("{icon}\n{name}")).size(13.0)).min_size(egui::vec2(62.0, 62.0));
        let r = ui.add(b);
        tag(ui, &r, cid);
        if r.clicked() && !on {
            set_kind(p, k);
            out.push(Action::Changed);
        }
        ui.add_space(4.0);
    }
}

/// The tab changes the preset's kind; a selection the new kind lacks becomes Region.
pub fn set_kind(p: &mut Preset, k: CaptureKind) {
    p.kind = k;
    if !Selection::for_kind(k).contains(&p.selection) {
        p.selection = Selection::Region;
    }
}

fn gear(ui: &mut egui::Ui, name: &str) -> egui::Response {
    let r = ui.add(egui::Button::new("⚙").frame(false)).on_hover_text(tr("Configurações"));
    tag(ui, &r, name);
    r
}

/// The region gear: Region (chosen during the capture or fixed) and, for images, each window as an object and the
/// page's links.
fn selection_has_gear(p: &Preset) -> bool {
    p.selection == Selection::Region
        || (p.kind == CaptureKind::Image
            && matches!(p.selection, Selection::Window | Selection::FullScreen | Selection::Monitor | Selection::Object | Selection::Freehand
                                     | Selection::MultiArea))
}

fn selection_row(ui: &mut egui::Ui, p: &mut Preset, out: &mut Vec<Action>) {
    let label = ui.label(tr("Seleção:"));
    ui.horizontal(|ui| {
        let list = Selection::for_kind(p.kind);
        let shown = if p.selection == Selection::Region && p.region_fixed { tr("Região fixa") } else { p.selection.name() };
        let mut changed = false;
        let r = egui::ComboBox::from_id_salt("IDC_SELECTION")
            .width(150.0)
            .selected_text(&shown)
            .show_ui(ui, |ui| {
                for (i, &s) in list.iter().enumerate() {
                    let r = ui.selectable_value(&mut p.selection, s, s.name());
                    tag(ui, &r, &format!("IDC_SELECTION.{i}"));
                    changed |= r.changed();
                }
            })
            .response
            .labelled_by(label.id);
        tag_value(ui, &r, "IDC_SELECTION", &shown);
        if changed {
            out.push(Action::Changed);
        }
        if selection_has_gear(p) {
            let g = gear(ui, "IDC_SELECTION.gear");
            egui::Popup::menu(&g).show(|ui| {
                if p.selection == Selection::Region {
                    if ui.radio(!p.region_fixed, tr("Selecionar a região na captura")).clicked() {
                        p.region_fixed = false;
                        out.push(Action::Changed);
                        ui.close();
                    }
                    if ui.radio(p.region_fixed, tr("Região fixa…")).clicked() {
                        out.push(Action::FixedRegion);
                        ui.close();
                    }
                }
                if p.kind == CaptureKind::Image {
                    if p.selection == Selection::Region {
                        ui.separator();
                    }
                    if matches!(p.selection, Selection::FullScreen | Selection::Monitor)
                        && ui.checkbox(&mut p.window_objects, tr("Cada janela como um objeto separado")).clicked()
                    {
                        out.push(Action::Changed);
                        ui.close();
                    }
                    if ui.checkbox(&mut p.capture_links, tr("Guardar os links da página (clicáveis, também no PDF)")).clicked() {
                        out.push(Action::Changed);
                        ui.close();
                    }
                }
            });
        }
    });
    ui.end_row();
}

/// The "Effects" box: the current effect (+ how many others); the list turns effects on (their form), off, or all off.
fn effects_row(ui: &mut egui::Ui, p: &mut Preset, view: &mut View, out: &mut Vec<Action>) {
    let label = ui.label(tr("Efeitos:"));
    let e = &mut p.effects;
    if view.fx_current.is_none_or(|k| !*fx_on(e, k)) {
        view.fx_current = fx_first_on(e);
    }
    let shown = match view.fx_current {
        None => tr("Nenhum"),
        Some(k) => {
            let others = fx_count_on(e) - 1;
            let t = tr(FX_NAMES[k]);
            if others > 0 { format!("{t}  +{others}") } else { t }
        }
    };
    let add_mode = view.fx_add_mode;
    let r = egui::ComboBox::from_id_salt("IDC_EFFECTS")
        .width(150.0)
        .selected_text(&shown)
        .show_ui(ui, |ui| {
            if add_mode {
                for k in 0..FX_NAMES.len() {
                    if !*fx_on(e, k) && ui.button(tr(FX_NAMES[k])).clicked() {
                        out.push(Action::EditFx(k));
                        ui.close();
                    }
                }
                return;
            }
            if ui.button(tr("Nenhum")).clicked() {
                for k in 0..FX_NAMES.len() {
                    *fx_on(e, k) = false;
                }
                view.fx_current = None;
                out.push(Action::Changed);
                ui.close();
            }
            for k in 0..FX_NAMES.len() {
                let on = *fx_on(e, k);
                let r = ui.add(egui::Button::selectable(false, tr(FX_NAMES[k])).right_text(if on { "✔" } else { "" }));
                tag(ui, &r, &format!("IDC_EFFECTS.{k}"));
                if r.clicked() {
                    out.push(Action::EditFx(k));
                    ui.close();
                }
            }
            ui.separator();
            if ui.button(tr("Adicionar outro efeito")).clicked() {
                view.fx_add_mode = true;
                ui.close();
            }
            if ui.button(tr("Remover este efeito")).clicked() {
                if let Some(k) = view.fx_current.take() {
                    *fx_on(e, k) = false;
                    out.push(Action::Changed);
                }
                ui.close();
            }
        })
        .response
        .labelled_by(label.id);
    tag_value(ui, &r, "IDC_EFFECTS", &shown);
    // "Add another effect" reopens the list with only the effects that are off
    let popup = egui::Popup::default_response_id(&r);
    if view.fx_add_mode && !egui::Popup::is_id_open(ui.ctx(), popup) {
        if add_mode {
            view.fx_add_mode = false;
        } else {
            egui::Popup::open_id(ui.ctx(), popup);
        }
    }
    ui.end_row();
}

fn share_row(ui: &mut egui::Ui, p: &mut Preset, out: &mut Vec<Action>) {
    let label = ui.label(tr("Compartilhar:"));
    ui.horizontal(|ui| {
        let cur = share_of(p);
        let names = Share::names();
        let r = egui::ComboBox::from_id_salt("IDC_SHARE")
            .width(150.0)
            .selected_text(&names[cur as usize])
            .show_ui(ui, |ui| {
                for (i, n) in names.iter().enumerate() {
                    let r = ui.selectable_label(i == cur as usize, n);
                    tag(ui, &r, &format!("IDC_SHARE.{i}"));
                    if r.clicked() {
                        let s = Share::from_i32(i as i32);
                        set_share(p, s);
                        out.push(Action::Changed);
                        out.push(Action::ShareChosen(s));
                    }
                }
            })
            .response
            .labelled_by(label.id);
        tag_value(ui, &r, "IDC_SHARE", &names[cur as usize]);
        if share_of(p).has_gear() && gear(ui, "IDC_SHARE.gear").clicked() {
            out.push(Action::ShareGear);
        }
    });
    ui.end_row();
}

fn quality_row(ui: &mut egui::Ui, p: &mut Preset, out: &mut Vec<Action>) {
    let label = ui.label(tr("Qualidade:"));
    ui.horizontal(|ui| {
        let names = [tr("Muito baixa"), tr("Baixa"), tr("Média"), tr("Alta")];
        let q = p.quality.clamp(0, 3) as usize;
        let mut changed = false;
        let r = egui::ComboBox::from_id_salt("IDC_QUALITY")
            .width(150.0)
            .selected_text(&names[q])
            .show_ui(ui, |ui| {
                for (i, n) in names.iter().enumerate() {
                    let r = ui.selectable_value(&mut p.quality, i as i32, n);
                    tag(ui, &r, &format!("IDC_QUALITY.{i}"));
                    changed |= r.changed();
                }
            })
            .response
            .labelled_by(label.id);
        tag_value(ui, &r, "IDC_QUALITY", &names[q]);
        if changed {
            out.push(Action::Changed);
        }
        if gear(ui, "IDC_QUALITY.gear").clicked() {
            out.push(Action::VideoOptions);
        }
    });
    ui.end_row();
}

fn webcam_row(ui: &mut egui::Ui, p: &mut Preset, view: &View, out: &mut Vec<Action>) {
    ui.label(tr("Webcam:"));
    let text = if p.webcam { tr("Ligada") } else { tr("Desligada") };
    let r = ui.add(egui::Button::new(format!("{text} ›")).frame(false));
    tag(ui, &r, "IDC_WEBCAM");
    if r.clicked() {
        out.push(Action::ListCameras);
    }
    egui::Popup::menu(&r).show(|ui| {
        if ui.radio(!p.webcam, tr("Desligada")).clicked() {
            p.webcam = false;
            out.push(Action::Changed);
            ui.close();
        }
        ui.separator();
        if view.cams.is_empty() {
            ui.add_enabled(false, egui::Button::new(tr("Nenhuma webcam encontrada")));
        }
        for (i, (name, link)) in view.cams.iter().enumerate() {
            let on = p.webcam && (p.webcam_device == *link || (p.webcam_device.is_empty() && i == 0));
            if ui.radio(on, name).clicked() {
                (p.webcam, p.webcam_device, p.webcam_name) = (true, link.clone(), name.clone());
                out.push(Action::Changed);
                ui.close();
            }
        }
        ui.separator();
        ui.add_enabled_ui(!p.chroma_key, |ui| {
            ui.menu_button(tr("Formato"), |ui| {
                for (s, n) in snapcore::webcam::SHAPE_NAMES.iter().enumerate() {
                    if ui.radio(p.webcam_shape == s as i32, tr(n)).clicked() {
                        p.webcam_shape = s as i32;
                        out.push(Action::Changed);
                        ui.close();
                    }
                }
            });
        });
        let mut ck = p.chroma_key;
        if ui.checkbox(&mut ck, tr("Remover o fundo (chroma key)…")).clicked() {
            out.push(Action::ChromaKey);
            ui.close();
        }
    });
    ui.end_row();
}

fn switch(ui: &mut egui::Ui, v: &mut bool, text: &str, name: &str) -> bool {
    let r = ui.checkbox(v, tr(text));
    tag(ui, &r, name);
    r.changed()
}

/// "%d s" or "Off"
pub fn delay_text(s: i32) -> String {
    if s <= 0 { tr("Desligado") } else { tr("%d s").replace("%d", &s.to_string()) }
}

fn delay_row(ui: &mut egui::Ui, p: &mut Preset, out: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        ui.label(tr("Atraso"));
        let r = ui.add(egui::Button::new(format!("{} ›", delay_text(p.delay_seconds))).frame(false));
        tag_value(ui, &r, "IDC_DELAY", &delay_text(p.delay_seconds));
        egui::Popup::menu(&r).show(|ui| {
            for s in [0, 1, 2, 3, 5, 10, 15, 30, 60] {
                let t = match s {
                    0 => tr("Desligado"),
                    1 => tr("1 segundo"),
                    _ => tr("%d segundos").replace("%d", &s.to_string()),
                };
                let r = ui.radio(p.delay_seconds == s, t);
                tag(ui, &r, &format!("IDC_DELAY.{s}"));
                if r.clicked() {
                    p.delay_seconds = s;
                    out.push(Action::Changed);
                    ui.close();
                }
            }
        });
    });
}

/// The round capture button (red for video); the tooltip says the hotkey.
fn capture_button(ui: &mut egui::Ui, p: &Preset, view: &View, out: &mut Vec<Action>) {
    let video = p.kind == CaptureKind::Video;
    let fill = if video { Color32::from_rgb(214, 40, 60) } else { Color32::from_rgb(30, 120, 230) };
    let icon = if video { "⏺" } else { "📷" };
    let b = egui::Button::new(RichText::new(format!("{icon}\n{}", tr("Capturar"))).size(14.0).color(Color32::WHITE))
        .fill(fill)
        .min_size(egui::vec2(84.0, 84.0))
        .corner_radius(42.0);
    let tip = if video { tr("Gravar vídeo  (") + &view.video_hotkey + ")" } else { tr("Capturar imagem  (") + &view.image_hotkey + ")" };
    let r = ui.add(b).on_hover_text(tip);
    tag(ui, &r, "IDC_CAPTURE");
    if r.clicked() {
        out.push(Action::Capture);
    }
}

/// HotkeyLabel: the preset's hotkey, or "Add hotkey".
fn hotkey_label(p: &Preset, text: &dyn Fn(u32, u32) -> String) -> String {
    if p.hotkey_vk != 0 { text(p.hotkey_mods, p.hotkey_vk) } else { tr("Adicionar atalho") }
}

/// The preset list: the kind's icon, the name, the hotkey (or "Add hotkey") and the gear; a click selects, a double
/// click captures.
fn preset_list(ui: &mut egui::Ui, store: &mut PresetStore, hk_text: &dyn Fn(u32, u32) -> String, out: &mut Vec<Action>) {
    let list = egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for i in 0..store.presets.len() {
            let sel = i as i32 == store.selected;
            let p = &store.presets[i];
            let icon = if p.kind == CaptureKind::Video { "🎬" } else { "📷" };
            let name = i18n::tr_str(&p.name);
            let hk = hotkey_label(p, hk_text);
            ui.horizontal(|ui| {
                ui.set_min_height(26.0);
                let w = ui.available_width() - 150.0;
                let row = ui.add_sized([w.max(80.0), 26.0], egui::Button::selectable(sel, format!("{icon}  {name}")));
                tag(ui, &row, &format!("IDC_PRESETS.{i}"));
                if row.clicked() && !sel {
                    store.selected = i as i32;
                    out.push(Action::Changed);
                }
                if row.double_clicked() {
                    store.selected = i as i32;
                    out.push(Action::Capture);
                }
                let h = ui.add_sized([110.0, 26.0], egui::Button::new(RichText::new(hk).weak()).frame(false));
                tag(ui, &h, &format!("IDC_PRESETS.{i}.hotkey"));
                if h.clicked() {
                    if !sel {
                        store.selected = i as i32;
                        out.push(Action::Changed);
                    }
                    out.push(Action::PresetHotkey(i));
                }
                let g = gear(ui, &format!("IDC_PRESETS.{i}.gear"));
                if g.clicked() && !sel {
                    store.selected = i as i32;
                    out.push(Action::Changed);
                }
                egui::Popup::menu(&g).show(|ui| {
                    menu_item(ui, out, "Capturar com este preset", id::PRESET_RUN);
                    ui.separator();
                    menu_item(ui, out, "Renomear…", id::PRESET_RENAME);
                    menu_item(ui, out, "Atalho…", id::PRESET_HOTKEY);
                    menu_item(ui, out, "Duplicar", id::PRESET_DUP);
                    ui.separator();
                    menu_item(ui, out, "Excluir", id::PRESET_DEL);
                });
            });
        }
    });
    let _ = list;
}

/// Draws the window; `hk_text` writes a hotkey ("Ctrl+Alt+F12").
pub fn draw(store: &mut PresetStore, view: &mut View, hk_text: &dyn Fn(u32, u32) -> String, ui: &mut egui::Ui) -> Vec<Action> {
    let mut out = Vec::new();
    egui::Panel::top("menu").show(ui, |ui| menu_bar(ui, view, &mut out));
    if store.presets.is_empty() {
        return out;
    }
    store.selected = store.selected.clamp(0, store.presets.len() as i32 - 1);
    if view.presets_open {
        let h = 8.0 + 30.0 * store.presets.len().clamp(2, 8) as f32;
        egui::Panel::bottom("IDC_PRESETS").exact_size(h).resizable(false).show(ui, |ui| preset_list(ui, store, hk_text, &mut out));
    }
    egui::Panel::bottom("bar").exact_size(40.0).resizable(false).show(ui, |ui| {
        ui.horizontal_centered(|ui| {
            let r = ui.add(egui::Button::selectable(view.presets_open, format!("⚙ {}", tr("Presets"))));
            tag(ui, &r, "IDC_PRESETS_LABEL");
            if r.clicked() {
                out.push(Action::Command(id::SHOW_PRESETS));
                out.push(Action::PresetsToggled);
            }
            let add = ui.add(egui::Button::new(RichText::new("+").size(16.0)).frame(false)).on_hover_text(tr("Novo preset"));
            tag(ui, &add, "IDC_PRESET_ADD");
            egui::Popup::menu(&add).show(|ui| {
                menu_item(ui, &mut out, "Novo preset com as configurações atuais", id::PRESET_FROM_CURRENT);
                menu_item(ui, &mut out, "Novo preset…", id::PRESET_NEW);
                ui.separator();
                menu_item(ui, &mut out, "Ajuda", id::PRESET_HELP);
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let r = ui.button(format!("✏ {}", tr("Abrir Editor")));
                tag(ui, &r, "IDC_OPEN_EDITOR");
                if r.clicked() {
                    out.push(Action::Command(id::OPEN_EDITOR));
                }
            });
        });
    });
    egui::Panel::left("tabs").exact_size(76.0).resizable(false).show(ui, |ui| {
        ui.add_space(8.0);
        let i = store.selected as usize;
        tabs(ui, &mut store.presets[i], &mut out);
    });
    egui::CentralPanel::default().show(ui, |ui| {
        let i = store.selected as usize;
        let p = &mut store.presets[i];
        let video = p.kind == CaptureKind::Video;
        ui.add_space(10.0);
        ui.horizontal_top(|ui| {
            egui::Grid::new("fields").num_columns(2).spacing([10.0, 10.0]).min_row_height(26.0).show(ui, |ui| {
                selection_row(ui, p, &mut out);
                if video {
                    quality_row(ui, p, &mut out);
                    webcam_row(ui, p, view, &mut out);
                } else {
                    effects_row(ui, p, view, &mut out);
                    share_row(ui, p, &mut out);
                }
            });
            ui.add_space(18.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                let mut changed = switch(ui, &mut p.preview_in_editor, "Visualizar no Editor", "IDC_PREVIEW");
                if video {
                    changed |= switch(ui, &mut p.capture_cursor, "Capturar o cursor", "IDC_CURSOR");
                    changed |= switch(ui, &mut p.microphone, "Gravar microfone", "IDC_MIC");
                    changed |= switch(ui, &mut p.system_audio, "Gravar áudio do sistema", "IDC_SYS");
                } else {
                    changed |= switch(ui, &mut p.copy_to_clipboard, "Copiar para a área de transferência", "IDC_CLIPBOARD");
                    changed |= switch(ui, &mut p.capture_cursor, "Capturar o cursor", "IDC_CURSOR");
                    delay_row(ui, p, &mut out);
                }
                if changed {
                    out.push(Action::Changed);
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                ui.add_space(8.0);
                capture_button(ui, p, view, &mut out);
            });
        });
    });
    out
}
