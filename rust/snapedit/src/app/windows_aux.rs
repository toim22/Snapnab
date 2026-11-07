//! The Editor's other windows, each an egui window of its own with the C++ window class: the text windows (Grab Text,
//! shortcuts — SnapnabTextWnd), Customize the toolbar (SnapnabTbCustomize), the Magnify intro (SnapnabToolIntro),
//! Compare images (SnapnabCompare), the contrast checker (SnapnabContrast) and the AI assistant (SnapnabAssistant).
//! They do not disable the Editor while open (the C++ ones were modal loops).

use std::path::Path;

use egui::{Color32, RichText};
use snapcore::color;
use snapcore::image::{Image, argb, cb, cg, cr};
use snapcore::xml::Node;
use snapedit_app::canvas::Tool;
use snapedit_app::canvas_ui::texture;
use snapedit_app::edtools::{self, CombineOptions};
use snapedit_app::ids::*;
use snapedit_app::menu::TOOLS;
use snapedit_app::native;
use snapedit_app::panels::{self, COMMANDS, Entry, EntryKind, OUTPUTS};
use snapedit_app::tr;
use snapui::form::{self, Field};
use snapui::host::{Ctl, WinKey, WindowSpec};

use super::docs::fmt;
use super::{Editor, Ev, VERSION, reg_dw, set_reg_dw};

pub struct TextWin {
    key: WinKey,
    text: String,
}

pub struct Customize {
    key: WinKey,
    before: (bool, Vec<Entry>, Vec<Tool>, usize),
    list: Vec<Entry>,
}

pub struct Compare {
    key: WinKey,
    a: Image,
    b: Image,
    na: String,
    nb: String,
    view: usize,
    amount: i32,
    tex: Option<(egui::TextureHandle, (usize, i32, bool))>,
    swapped: bool,
}

pub struct Contrast {
    key: WinKey,
    fg: u32,
    bg: u32,
}

pub struct Assistant {
    key: WinKey,
    img: Image,
    s: AiSettings,
    question: String,
    answer: String,
    status: String,
    busy: bool,
    generation: u64,
}

#[derive(Default)]
pub struct Aux {
    pub pending_copy_as: Option<(String, Image, String)>,
    texts: Vec<TextWin>,
    customize: Option<Customize>,
    intro: Option<WinKey>,
    compare: Option<Compare>,
    contrast: Option<Contrast>,
    assistant: Option<Assistant>,
}

fn spec(class: &'static str, title: String, size: (f32, f32)) -> WindowSpec {
    WindowSpec { class, title, size, clear_color: [0.96, 0.96, 0.97, 1.0], ..Default::default() }
}

const SHORTCUTS: &str = "Ferramentas (com a área de pintura em foco):\n  V Mover   A Seta   L Linha   R Forma   P Caneta   H Marcador\n  T Texto   C Balão   N Passo   K Carimbo   B Ocultar   G Destaque   Z Lupa\n  F Preencher   E Borracha   X Cortar   S Seleção   U Recortar faixa   I Conta-gotas\n\nEdição:  Ctrl+Z/Ctrl+Y desfazer/refazer • Ctrl+C/X/V • Ctrl+Shift+C copiar tudo • Del excluir\n         Setas movem o objeto (Shift = 10 px) • Shift restringe proporção/ângulo\n         Duplo clique edita texto • Esc cancela • Enter aplica o recorte\nExibir:  Ctrl+roda = zoom • Ctrl+0 ajustar • Ctrl+1 tamanho real • botão do meio arrasta\nArquivo: Ctrl+S salvar na biblioteca • Ctrl+Shift+S salvar como (PNG/JPG/BMP/GIF/TIFF/PDF/SNAP)\nTexto:   Ctrl+Shift+O Grab Text (OCR) • Ctrl+Shift+D data e hora\nFerramentas: Ctrl+Shift+Q ler código QR • Ctrl+Shift+E ocultar dados sensíveis • Ctrl+Shift+P fixar na tela\nImagem:  Ctrl+Shift+F/B um passo à frente/atrás • Ctrl+T mesclar o objeto • Ctrl+Shift+T mesclar tudo • Ctrl+; encaixe\nVídeo:   Espaço reproduz/pausa • ←/→ quadro • I/O marcam início/fim\n";

// ---------------------------------------------------------------- AI assistant settings (Assistant.cpp)
#[derive(Clone, Debug, Default)]
pub struct AiSettings {
    pub enabled: bool,
    pub provider: i32,
    pub model: String,
    pub endpoint: String,
}

const PROVIDERS: i32 = 5;

fn provider_name(p: i32) -> String {
    match p {
        0 => "OpenAI".into(),
        1 => "Google Gemini".into(),
        2 => "Anthropic".into(),
        3 => tr("Compatível com OpenAI"),
        _ => tr("Modelo local (Ollama)"),
    }
}

fn default_model(p: i32) -> &'static str {
    match p {
        0 => "gpt-4o-mini",
        1 => "gemini-2.5-flash",
        2 => "claude-opus-5-5",
        4 => "llava",
        _ => "",
    }
}

fn default_endpoint(p: i32) -> &'static str {
    match p {
        0 => "https://api.openai.com/v1",
        1 => "https://generativelanguage.googleapis.com/v1beta",
        2 => "https://api.anthropic.com/v1",
        4 => "http://localhost:11434/v1",
        _ => "",
    }
}

impl AiSettings {
    fn file() -> std::path::PathBuf {
        snapcore::paths::app_data_dir().join("AiSettings.xml")
    }

    pub fn load() -> AiSettings {
        let mut s = AiSettings::default();
        if let Some(n) = snapcore::xml::load_file(&Self::file()) {
            let i = |k: &str| n.attr(k).and_then(|v| v.trim().parse::<i32>().ok()).unwrap_or(0);
            s.enabled = i("Enabled") != 0;
            s.provider = i("Provider").clamp(0, PROVIDERS - 1);
            s.model = n.attr("Model").unwrap_or("").to_string();
            s.endpoint = n.attr("Endpoint").unwrap_or("").to_string();
        }
        if s.model.is_empty() {
            s.model = default_model(s.provider).into();
        }
        if s.endpoint.is_empty() {
            s.endpoint = default_endpoint(s.provider).into();
        }
        s
    }

    fn save(&self) {
        let mut n = Node::new("AiSettings");
        n.set("Enabled", (self.enabled as i32).to_string());
        n.set("Provider", self.provider.to_string());
        n.set("Model", self.model.clone());
        n.set("Endpoint", self.endpoint.clone());
        snapcore::xml::save_file(&Self::file(), &n);
    }

    fn secret_name(p: i32) -> String {
        format!("AI/{p}")
    }

    fn needs_key(&self) -> bool {
        self.provider <= 2
    }
}

fn host_of(url: &str) -> String {
    let a = url.find("://").map_or(0, |i| i + 3);
    let rest = &url[a..];
    rest[..rest.find(['/', ':', '?']).unwrap_or(rest.len())].to_string()
}

fn notice(s: &AiSettings) -> String {
    fmt(&tr("A imagem e a pergunta serão enviadas para %s (%s). Nada é enviado até você perguntar."), &[&provider_name(s.provider), &host_of(&s.endpoint)])
}

/// AssistantSettingsDialog (on a worker thread).
pub fn assistant_settings_dialog(owner: isize) -> bool {
    let mut s = AiSettings::load();
    let provs: Vec<String> = (0..PROVIDERS).map(provider_name).collect();
    let has_key = !snapcore::net::load_secret(&AiSettings::secret_name(s.provider)).is_empty();
    let keep = tr("(guardada — deixe assim para manter)");
    let prev = s.provider;
    let mut f = vec![
        Field::Check { label: tr("Ativar o assistente de IA"), value: s.enabled },
        Field::Label(tr("Ao perguntar, a imagem é enviada ao provedor escolhido. Use a sua própria chave (cobrança na sua conta).")),
        Field::Combo { label: tr("Provedor"), items: provs, value: s.provider },
        Field::Text { label: tr("Modelo"), text: s.model.clone() },
        Field::Text { label: tr("Endereço da API"), text: s.endpoint.clone() },
        Field::Text { label: tr("Chave da API"), text: if has_key { keep.clone() } else { String::new() } },
        Field::Label(tr("A chave fica no Gerenciador de Credenciais do Windows, nunca em arquivos. Modelo local: Ollama em http://localhost:11434/v1 (sem chave).")),
    ];
    loop {
        if !form::run(owner, &tr("Assistente de IA"), &mut f) {
            return false;
        }
        let p = f[2].int();
        // changing the provider fills its defaults (unless the user typed something else)
        let mut model = f[3].text().trim().to_string();
        let mut endpoint = f[4].text().trim().to_string();
        if p != prev {
            if model.is_empty() || model == default_model(prev) {
                model = default_model(p).into();
            }
            if endpoint.is_empty() || endpoint == default_endpoint(prev) {
                endpoint = default_endpoint(p).into();
            }
        }
        let problem = if f[0].on() && endpoint.is_empty() {
            tr("Informe o endereço da API.")
        } else if f[0].on() && model.is_empty() && p != 3 {
            tr("Informe o modelo.")
        } else {
            s.enabled = f[0].on();
            s.provider = p;
            s.model = if model.is_empty() { default_model(p).into() } else { model };
            s.endpoint = if endpoint.is_empty() { default_endpoint(p).into() } else { endpoint };
            break;
        };
        native::message(owner, &problem, &tr("Assistente de IA"), windows::Win32::UI::WindowsAndMessaging::MB_ICONWARNING);
    }
    let key = f[5].text().trim().to_string();
    if key != keep {
        if key.is_empty() {
            snapcore::net::delete_secret(&AiSettings::secret_name(s.provider));
        } else {
            snapcore::net::save_secret(&AiSettings::secret_name(s.provider), &key);
        }
    } else if s.provider != prev {
        // the kept key belonged to the old provider: move it
        let old = snapcore::net::load_secret(&AiSettings::secret_name(prev));
        if !old.is_empty() {
            snapcore::net::save_secret(&AiSettings::secret_name(s.provider), &old);
        }
    }
    s.save();
    true
}

const AI_PRESETS: [(&str, &str); 4] = [
    ("Explicar", "Explain what this screenshot shows and what the user is probably trying to do."),
    ("Descrever", "Describe this image in detail, suitable as alternative text."),
    ("Extrair os passos", "List the steps shown or implied in this screenshot as a numbered list."),
    ("Extrair o texto", "Transcribe all the text visible in this image, keeping the layout as much as possible."),
];

/// One request: the image (PNG, at most 2000 px) and the question, answered in the interface language.
fn ask(s: &AiSettings, key: &str, img: &Image, question: &str) -> Result<String, String> {
    let long = img.wi().max(img.hi());
    let im = if long > 2000 {
        snapcore::gdip::scale_image(img, (img.wi() * 2000 / long).max(1), (img.hi() * 2000 / long).max(1), true)
    } else {
        img.clone()
    };
    let png = snapcore::codec::encode(&im, snapcore::codec::Format::Png, &snapcore::codec::SaveOptions::from_settings(), VERSION)
        .map_err(|_| tr("Não foi possível preparar a imagem."))?;
    let cur = snapcore::i18n::current();
    let lang = snapcore::i18n::LANGUAGES.iter().find(|(c, _)| *c == cur).map(|(_, n)| *n);
    let q = match lang {
        Some(n) => format!("{question}\n\n(Answer in {n}.)"),
        None => question.to_string(),
    };
    snapcore::ai::ask(s.provider, &s.endpoint, &s.model, key, &png, &q)
}

// ---------------------------------------------------------------- comparison
fn fit(img: &Image, w: i32, h: i32) -> Image {
    let mut out = Image::sized(w, h, 0xFFFF_FFFF);
    out.blit(img, 0, 0, false);
    out
}

/// The composed view at the images' own size: 0 overlay, 1 swipe, 2 difference, 3 side by side.
pub fn compose(a: &Image, b: &Image, view: usize, amount: i32) -> Image {
    let (w, h) = (a.wi().max(b.wi()), a.hi().max(b.hi()));
    match view {
        0 => {
            let (mut out, bb) = (fit(a, w, h), fit(b, w, h));
            let k = amount * 255 / 100;
            for (p, &q) in out.px.iter_mut().zip(&bb.px) {
                let m = |x: i32, y: i32| x + (y - x) * k / 255;
                *p = argb(255, m(cr(*p), cr(q)), m(cg(*p), cg(q)), m(cb(*p), cb(q)));
            }
            out
        }
        1 => {
            let (mut out, bb) = (fit(a, w, h), fit(b, w, h));
            let cut = w * amount / 100;
            for y in 0..h {
                for x in cut..w {
                    *out.at_mut(x, y) = bb.at(x, y);
                }
                for x in (cut - 1).max(0)..(cut + 2).min(w) {
                    *out.at_mut(x, y) = 0xFFE5_322D;
                }
            }
            out
        }
        2 => edtools::diff_image(a, b, amount * 255 / 100).0,
        _ => {
            let o = CombineOptions { layout: 0, gap: 12, align: 0, background: color::rgb(240, 240, 244), ..Default::default() };
            edtools::combine_images(&[a.clone(), b.clone()], &o, None)
        }
    }
}

fn export_gif(path: &str, a: &Image, b: &Image) -> bool {
    let (w, h) = (a.wi().max(b.wi()), a.hi().max(b.hi()));
    let Some(mut g) = snapcore::gif::GifWriter::open(Path::new(path), w, h, 1.0, 8, true, false, b"") else { return false };
    g.add_frame(&fit(a, w, h), 100);
    g.add_frame(&fit(b, w, h), 100);
    g.close()
}

fn export_combined(path: &str, a: &Image, b: &Image) -> bool {
    let font = (a.hi().max(b.hi()) / 22).clamp(14, 48);
    let w = |s: &str| tr(s).encode_utf16().collect::<Vec<_>>();
    let ca = snapcore::fx::caption(a, &w("Antes"), true, font, 0xFF20_2226, 0xFFF0_F0F4);
    let cbi = snapcore::fx::caption(b, &w("Depois"), true, font, 0xFF20_2226, 0xFFF0_F0F4);
    let o = CombineOptions {
        layout: if a.w >= a.h * 2 { 1 } else { 0 },
        gap: 16,
        margin: 16,
        align: 0,
        background: color::rgb(240, 240, 244),
        ..Default::default()
    };
    snapedit_app::files::save_image(path, &edtools::combine_images(&[ca, cbi], &o, None), 90).is_ok()
}

fn swatch_color(c: u32) -> Color32 {
    Color32::from_rgb((c & 0xFF) as u8, ((c >> 8) & 0xFF) as u8, ((c >> 16) & 0xFF) as u8)
}

impl Editor {
    /// ShowTextWindow: the text (selectable, copyable) and a copy button.
    pub fn show_text(&mut self, ctl: &mut Ctl, title: String, text: String) {
        if let Some(k) = ctl.create(&spec("SnapnabTextWnd", title, (560.0, 420.0))) {
            self.aux.texts.push(TextWin { key: k, text });
        }
    }

    pub fn show_shortcuts(&mut self, ctl: &mut Ctl) {
        self.show_text(ctl, tr("Atalhos do Snapnab Editor"), tr(SHORTCUTS));
    }

    /// The first time the Magnify tool is chosen: what it does.
    pub fn show_tool_intro(&mut self) {
        if reg_dw("MagnifyIntroShown", 0) != 0 {
            return;
        }
        set_reg_dw("MagnifyIntroShown", 1);
        if let Some(w) = &self.waker {
            w.send(Ev::Do(Box::new(|e: &mut Editor, ctl: &mut Ctl| {
                e.aux.intro = ctl.create(&spec("SnapnabToolIntro", tr("Ampliar parte da imagem"), (520.0, 220.0)));
            })));
        }
    }

    /// Customize (CBCGPToolbarCustomize): the catalog to add from, the bar's items in order; Cancel restores it.
    pub fn customize_toolbar(&mut self, ctl: &mut Ctl) {
        if let Some(c) = &self.aux.customize {
            if let Some(w) = ctl.window(c.key) {
                w.focus_window();
            }
            return;
        }
        let before = (self.tb_custom, self.tb_custom_list.clone(), self.tb_recent.clone(), self.tb_tool_count);
        let list = self.center_entries();
        if let Some(k) = ctl.create(&spec("SnapnabTbCustomize", tr("Personalizar a barra de ferramentas"), (720.0, 560.0))) {
            self.aux.customize = Some(Customize { key: k, before, list });
        }
    }

    pub fn show_contrast(&mut self, ctl: &mut Ctl, fg: u32, bg: u32) {
        if let Some(c) = self.aux.contrast.take() {
            ctl.close(c.key);
        }
        if let Some(k) = ctl.create(&spec("SnapnabContrast", tr("Verificador de contraste (WCAG)"), (460.0, 400.0))) {
            self.aux.contrast = Some(Contrast { key: k, fg, bg });
        }
    }

    pub fn show_compare(&mut self, ctl: &mut Ctl, a: (Image, String), b: (Image, String)) {
        if let Some(c) = self.aux.compare.take() {
            ctl.close(c.key);
        }
        if let Some(k) = ctl.create(&spec("SnapnabCompare", tr("Comparar imagens"), (960.0, 640.0))) {
            self.aux.compare =
                Some(Compare { key: k, a: a.0, b: b.0, na: a.1, nb: b.1, view: 0, amount: 50, tex: None, swapped: false });
        }
    }

    pub fn show_assistant(&mut self, ctl: &mut Ctl, img: Image) {
        if let Some(a) = self.aux.assistant.take() {
            ctl.close(a.key);
        }
        if let Some(k) = ctl.create(&spec("SnapnabAssistant", tr("Assistente de IA"), (640.0, 520.0))) {
            self.aux.assistant = Some(Assistant {
                key: k,
                img,
                s: AiSettings::load(),
                question: String::new(),
                answer: String::new(),
                status: String::new(),
                busy: false,
                generation: 0,
            });
        }
    }

    pub fn aux_close(&mut self, ctl: &mut Ctl, k: WinKey) {
        if self.aux.customize.as_ref().is_some_and(|c| c.key == k) {
            self.customize_done(false);
        }
        self.aux.texts.retain(|t| t.key != k);
        if self.aux.intro == Some(k) {
            self.aux.intro = None;
        }
        if self.aux.compare.as_ref().is_some_and(|c| c.key == k) {
            self.aux.compare = None;
        }
        if self.aux.contrast.as_ref().is_some_and(|c| c.key == k) {
            self.aux.contrast = None;
        }
        if self.aux.assistant.as_ref().is_some_and(|c| c.key == k) {
            self.aux.assistant = None;
        }
        ctl.close(k);
    }

    pub fn aux_ui(&mut self, ctl: &mut Ctl, k: WinKey, ui: &mut egui::Ui) {
        if let Some(i) = self.aux.texts.iter().position(|t| t.key == k) {
            self.text_ui(ctl, i, ui);
        } else if self.aux.customize.as_ref().is_some_and(|c| c.key == k) {
            self.customize_ui(ctl, ui);
        } else if self.aux.intro == Some(k) {
            self.intro_ui(ctl, ui);
        } else if self.aux.compare.as_ref().is_some_and(|c| c.key == k) {
            self.compare_ui(ctl, ui);
        } else if self.aux.contrast.as_ref().is_some_and(|c| c.key == k) {
            self.contrast_ui(ctl, ui);
        } else if self.aux.assistant.as_ref().is_some_and(|c| c.key == k) {
            self.assistant_ui(ctl, ui);
        }
    }

    fn text_ui(&mut self, ctl: &mut Ctl, i: usize, ui: &mut egui::Ui) {
        let (mut close, mut copy) = (false, false);
        egui::Panel::bottom("buttons").show(ui, |ui| {
            ui.add_space(4.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                close = ui.button(tr("Fechar")).clicked();
                copy = ui.button(tr("Copiar tudo")).clicked();
            });
            ui.add_space(4.0);
        });
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let mut t = self.aux.texts[i].text.as_str();
                let r = ui.add(egui::TextEdit::multiline(&mut t).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace));
                panels::tag(ui, &r, "SnapnabTextWnd.Text");
            });
        });
        if copy {
            snapcore::clip::copy_text(self.hwnd, &self.aux.texts[i].text);
            self.status = tr("Copiado.");
        }
        if close {
            let k = self.aux.texts[i].key;
            self.aux_close(ctl, k);
        }
    }

    fn intro_ui(&mut self, ctl: &mut Ctl, ui: &mut egui::Ui) {
        let mut done = ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Escape));
        egui::CentralPanel::default().show(ui, |ui| {
            ui.add_space(8.0);
            ui.label(RichText::new(tr("Ampliar parte da imagem")).size(20.0).strong());
            ui.add_space(8.0);
            ui.label(tr("Amplie partes da imagem para destacar os detalhes importantes. Arraste para criar a lupa; com a lente separada, a alça amarela escolhe a área ampliada."));
            ui.add_space(16.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                done |= ui.button(tr("Entendi!")).clicked();
            });
        });
        if done && let Some(k) = self.aux.intro.take() {
            ctl.close(k);
        }
    }

    // ---------------------------------------------------------------- customize
    fn customize_done(&mut self, ok: bool) {
        let Some(c) = self.aux.customize.take() else { return };
        if !ok {
            (self.tb_custom, self.tb_custom_list, self.tb_recent, self.tb_tool_count) = c.before;
            self.save_toolbar_config();
            self.status = tr("Personalização cancelada.");
            return;
        }
        let saved_custom = self.tb_custom;
        self.tb_custom = false;
        let defaults = self.center_entries();
        self.tb_custom = saved_custom;
        self.tb_custom = c.list != defaults;
        if self.tb_custom {
            self.tb_custom_list = c.list;
        }
        self.save_toolbar_config();
        self.status = if self.tb_custom { tr("Barra de ferramentas personalizada.") } else { tr("Barra de ferramentas automática.") };
    }

    fn customize_ui(&mut self, ctl: &mut Ctl, ui: &mut egui::Ui) {
        let fx_names: std::collections::HashMap<u32, String> = self.fx.effects.iter().map(|e| (e.cmd, e.name.clone())).collect();
        let name_of = |e: &Entry| -> String {
            if e.kind == EntryKind::Spacer {
                return tr("— espaço —");
            }
            panels::describe(e, &|c| fx_names.get(&c).cloned()).0
        };
        let catalog: Vec<(String, Vec<Entry>)> = vec![
            (tr("Ferramentas de desenho"), TOOLS.iter().map(|(t, _)| Entry { kind: EntryKind::Tool, id: t.index() as u32 }).collect()),
            (tr("Itens de menu"), COMMANDS.iter().map(|c| Entry { kind: EntryKind::Command, id: c.id }).collect()),
            (tr("Destinos de compartilhamento"), OUTPUTS.iter().map(|c| Entry { kind: EntryKind::Output, id: c.id }).collect()),
            (tr("Efeitos"), EFFECTS.iter().map(|&c| Entry { kind: EntryKind::Effect, id: c }).collect()),
            (tr("Diversos"), vec![Entry { kind: EntryKind::Spacer, id: 0 }]),
        ];
        let (mut finish, mut count) = (None, None);
        let mut defaults_now = false;
        egui::Panel::bottom("buttons").show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let mut n = self.tb_tool_count as i32;
                ui.label(tr("Ferramentas na barra automática:"));
                if ui.add(egui::DragValue::new(&mut n).range(6..=16)).changed() {
                    count = Some(n as usize);
                }
                defaults_now = ui.button(tr("Restaurar o padrão")).clicked();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(tr("Cancelar")).clicked() {
                        finish = Some(false);
                    }
                    if ui.button("OK").clicked() {
                        finish = Some(true);
                    }
                });
            });
            ui.add_space(6.0);
        });
        let Some(c) = self.aux.customize.as_mut() else { return };
        egui::Panel::right("bar").exact_size(300.0).resizable(false).show(ui, |ui| {
            ui.heading(tr("Na barra"));
            egui::ScrollArea::vertical().id_salt("bar-list").show(ui, |ui| {
                let mut act = None;
                for (i, e) in c.list.iter().enumerate() {
                    ui.horizontal(|ui| {
                        if ui.small_button("↑").clicked() && i > 0 {
                            act = Some((i, 0));
                        }
                        if ui.small_button("↓").clicked() {
                            act = Some((i, 1));
                        }
                        if ui.small_button("✕").clicked() {
                            act = Some((i, 2));
                        }
                        ui.label(name_of(e));
                    });
                }
                match act {
                    Some((i, 0)) => c.list.swap(i, i - 1),
                    Some((i, 1)) if i + 1 < c.list.len() => c.list.swap(i, i + 1),
                    Some((i, 2)) => {
                        c.list.remove(i);
                    }
                    _ => {}
                }
            });
        });
        egui::CentralPanel::default().show(ui, |ui| {
            ui.label(tr("Clique num item para acrescentá-lo ao fim da barra."));
            egui::ScrollArea::vertical().id_salt("catalog").show(ui, |ui| {
                for (title, items) in &catalog {
                    ui.add_space(4.0);
                    ui.label(RichText::new(title).strong());
                    ui.horizontal_wrapped(|ui| {
                        for e in items {
                            let present = e.kind != EntryKind::Spacer && c.list.contains(e);
                            if ui.add_enabled(!present, egui::Button::new(name_of(e))).clicked() {
                                c.list.push(*e);
                            }
                        }
                    });
                }
            });
        });
        if let Some(n) = count {
            // the number of tools: back to the automatic toolbar with that size
            self.tb_tool_count = n;
            self.fit_recent_tools();
            self.tb_custom = false;
            let l = self.center_entries();
            if let Some(c) = self.aux.customize.as_mut() {
                c.list = l;
            }
        }
        if defaults_now {
            self.tb_custom = false;
            let l = self.center_entries();
            if let Some(c) = self.aux.customize.as_mut() {
                c.list = l;
            }
        }
        if let Some(ok) = finish {
            let k = self.aux.customize.as_ref().map(|c| c.key);
            self.customize_done(ok);
            if let Some(k) = k {
                ctl.close(k);
            }
        }
    }

    // ---------------------------------------------------------------- compare
    fn compare_ui(&mut self, ctl: &mut Ctl, ui: &mut egui::Ui) {
        let owner = self.hwnd;
        let Some(c) = self.aux.compare.as_mut() else { return };
        let mut close = ui.input(|i| i.key_pressed(egui::Key::Escape));
        let (mut gif, mut png) = (false, false);
        egui::Panel::top("tools").show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let views = [tr("Sobreposição"), tr("Deslizar (antes | depois)"), tr("Diferença"), tr("Lado a lado")];
                let before = c.view;
                egui::ComboBox::from_id_salt("view").selected_text(views[c.view].clone()).show_ui(ui, |ui| {
                    for (i, v) in views.iter().enumerate() {
                        ui.selectable_value(&mut c.view, i, v);
                    }
                });
                if c.view != before {
                    c.amount = if c.view == 2 { 6 } else { 50 };
                }
                if c.view != 3 {
                    let label = [tr("Transparência de B"), tr("Divisória"), tr("Tolerância")][c.view].clone();
                    ui.label(label);
                    ui.add(egui::Slider::new(&mut c.amount, 0..=100));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close |= ui.button(tr("Fechar")).clicked();
                    png = ui.button(tr("Exportar imagem…")).clicked();
                    gif = ui.button(tr("Exportar GIF…")).clicked();
                    if ui.button(tr("A ⇄ B")).clicked() {
                        std::mem::swap(&mut c.a, &mut c.b);
                        std::mem::swap(&mut c.na, &mut c.nb);
                        c.swapped = !c.swapped;
                    }
                });
            });
            ui.add_space(6.0);
        });
        egui::Panel::bottom("info").show(ui, |ui| {
            let mut t = fmt(&tr("A: %s (%d × %d)   B: %s (%d × %d)"), &[&c.na, &c.a.w, &c.a.h, &c.nb, &c.b.w, &c.b.h]);
            if c.view == 2 {
                let (_, changed, _) = edtools::diff_image(&c.a, &c.b, c.amount * 255 / 100);
                let total = (c.a.w.max(c.b.w) * c.a.h.max(c.b.h)) as f64;
                let pct = if total > 0.0 { 100.0 * changed as f64 / total } else { 0.0 };
                t += &fmt(&tr("   •   %d pixels diferentes (%.2f%%)"), &[&changed, &format!("{pct:.2}")]);
            }
            ui.label(t);
        });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::from_rgb(222, 224, 228))).show(ui, |ui| {
            let key = (c.view, c.amount, c.swapped);
            if c.tex.as_ref().is_none_or(|(_, k)| *k != key) {
                let img = compose(&c.a, &c.b, c.view, c.amount);
                c.tex = Some((texture(ui.ctx(), "compare", &img, true), key));
            }
            let (t, _) = c.tex.as_ref().unwrap();
            let area = ui.available_rect_before_wrap();
            let size = t.size_vec2();
            let k = (area.width() / size.x).min(area.height() / size.y).min(1.0);
            let r = egui::Rect::from_center_size(area.center(), size * k);
            let resp = ui.allocate_rect(r, egui::Sense::drag());
            ui.painter().image(t.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            if c.view == 1 {
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                }
                if (resp.dragged() || resp.drag_started()) && let Some(p) = resp.interact_pointer_pos() {
                    c.amount = (((p.x - r.left()) / r.width().max(1.0)) * 100.0).clamp(0.0, 100.0) as i32;
                }
            }
        });
        if gif || png {
            let (a, b) = (c.a.clone(), c.b.clone());
            self.modal(
                move |o| {
                    let (filter, ext, name) = if gif {
                        (tr("GIF animado (*.gif)|*.gif|"), "gif", tr("Antes-depois") + ".gif")
                    } else {
                        ("PNG (*.png)|*.png|JPEG (*.jpg)|*.jpg|WebP (*.webp)|*.webp|".to_string(), "png", tr("Antes-depois") + ".png")
                    };
                    let (p, _) = native::save_file(o, &tr("Salvar"), &name, &filter, ext, 1)?;
                    Some(if gif { export_gif(&p, &a, &b) } else { export_combined(&p, &a, &b) })
                },
                |e, _, r| match r {
                    Some(true) => e.status = tr("Exportado."),
                    Some(false) => e.warn(tr("Não foi possível salvar o arquivo.")),
                    None => {}
                },
            );
            let _ = owner;
        }
        if close {
            let k = self.aux.compare.as_ref().map(|c| c.key);
            if let Some(k) = k {
                self.aux_close(ctl, k);
            }
        }
    }

    // ---------------------------------------------------------------- contrast
    fn contrast_ui(&mut self, ctl: &mut Ctl, ui: &mut egui::Ui) {
        let Some(c) = self.aux.contrast.as_mut() else { return };
        let mut close = ui.input(|i| i.key_pressed(egui::Key::Escape));
        let mut pick: Option<bool> = None;
        egui::CentralPanel::default().show(ui, |ui| {
            ui.columns(2, |cols| {
                for (i, col) in cols.iter_mut().enumerate() {
                    let (label, v) = if i == 0 { (tr("Texto (primeiro plano)"), c.fg) } else { (tr("Fundo"), c.bg) };
                    col.label(label);
                    col.horizontal(|ui| {
                        let (r, p) = ui.allocate_painter(egui::vec2(34.0, 34.0), egui::Sense::click());
                        p.rect_filled(r.rect, 4.0, swatch_color(v));
                        p.rect_stroke(r.rect, 4.0, egui::Stroke::new(1.0, Color32::GRAY), egui::StrokeKind::Inside);
                        if r.clicked() {
                            pick = Some(i == 0);
                        }
                        ui.vertical(|ui| {
                            ui.label(RichText::new(color::color_to_hex(v)).strong());
                            ui.horizontal(|ui| {
                                if ui.small_button(tr("Escolher cor…")).clicked() {
                                    pick = Some(i == 0);
                                }
                                if ui.small_button(tr("Copiar HEX")).clicked() {
                                    snapcore::clip::copy_text(0, &color::color_to_hex(v));
                                }
                            });
                        });
                    });
                }
            });
            ui.add_space(8.0);
            egui::Frame::new().fill(swatch_color(c.bg)).corner_radius(6.0).inner_margin(12.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(tr("Texto grande Aa")).size(24.0).strong().color(swatch_color(c.fg)));
                ui.label(RichText::new(tr("Texto normal — o rato roeu a roupa do rei")).size(14.0).color(swatch_color(c.fg)));
            });
            ui.add_space(8.0);
            ui.label(edtools::contrast_report(c.fg, c.bg));
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button(tr("Inverter as cores")).clicked() {
                    std::mem::swap(&mut c.fg, &mut c.bg);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close |= ui.button(tr("Fechar")).clicked();
                });
            });
        });
        if let Some(fg) = pick {
            let init = if fg { c.fg } else { c.bg };
            let owner = ctl.hwnd(c.key);
            self.modal(move |_| native::choose_color(owner, init), move |e, _, r| {
                if let (Some(v), Some(c)) = (r, e.aux.contrast.as_mut()) {
                    if fg {
                        c.fg = v;
                    } else {
                        c.bg = v;
                    }
                }
            });
        }
        if close {
            let k = self.aux.contrast.as_ref().map(|c| c.key);
            if let Some(k) = k {
                self.aux_close(ctl, k);
            }
        }
    }

    // ---------------------------------------------------------------- assistant
    fn assistant_start(&mut self, question: String) {
        let Some(a) = self.aux.assistant.as_mut() else { return };
        if a.busy || question.trim().is_empty() {
            return;
        }
        let key = if a.s.needs_key() || a.s.provider == 3 { snapcore::net::load_secret(&AiSettings::secret_name(a.s.provider)) } else { String::new() };
        if a.s.needs_key() && key.is_empty() {
            a.status = tr("Configure a chave do provedor primeiro.");
            return;
        }
        a.busy = true;
        a.generation += 1;
        a.status = tr("Perguntando…");
        let (s, img, generation) = (a.s.clone(), a.img.clone(), a.generation);
        self.modal(move |_| ask(&s, &key, &img, &question), move |e, _, r| {
            let Some(a) = e.aux.assistant.as_mut().filter(|a| a.generation == generation) else { return };
            a.busy = false;
            match r {
                Ok(t) => {
                    a.answer = t;
                    a.status = fmt(&tr("Resposta de %s (%s)."), &[&provider_name(a.s.provider), &a.s.model]);
                }
                Err(err) => a.status = err,
            }
        });
    }

    fn assistant_ui(&mut self, ctl: &mut Ctl, ui: &mut egui::Ui) {
        let Some(a) = self.aux.assistant.as_mut() else { return };
        let mut close = ui.input(|i| i.key_pressed(egui::Key::Escape));
        let (mut start, mut add, mut settings) = (None, false, false);
        egui::Panel::bottom("buttons").show(ui, |ui| {
            ui.add_space(4.0);
            ui.label(&a.status);
            ui.horizontal(|ui| {
                if ui.button(tr("Copiar")).clicked() && !a.answer.trim().is_empty() {
                    snapcore::clip::copy_text(0, &a.answer.replace('\r', ""));
                    a.status = tr("Resposta copiada.");
                }
                add = ui.button(tr("Adicionar como texto")).clicked() && !a.answer.trim().is_empty();
                settings = ui.button(tr("Configurações…")).clicked();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close |= ui.button(tr("Fechar")).clicked();
                });
            });
            ui.add_space(4.0);
        });
        egui::CentralPanel::default().show(ui, |ui| {
            ui.label(RichText::new(notice(&a.s)).color(Color32::from_rgb(160, 80, 0)));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                for (label, prompt) in AI_PRESETS {
                    if ui.add_enabled(!a.busy, egui::Button::new(tr(label))).clicked() {
                        start = Some(prompt.to_string());
                    }
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut a.question).hint_text(tr("Faça uma pergunta sobre a imagem…")).desired_width(ui.available_width() - 110.0));
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (ui.add_enabled(!a.busy, egui::Button::new(tr("Perguntar"))).clicked() || enter) && !a.busy {
                    start = Some(a.question.clone());
                }
            });
            ui.add_space(6.0);
            egui::ScrollArea::vertical().show(ui, |ui| {
                let mut t = a.answer.as_str();
                ui.add(egui::TextEdit::multiline(&mut t).desired_width(f32::INFINITY).desired_rows(14));
            });
        });
        if let Some(q) = start {
            self.assistant_start(q);
        }
        if settings {
            self.modal(assistant_settings_dialog, |e, _, ok| {
                if ok && let Some(a) = e.aux.assistant.as_mut() {
                    a.s = AiSettings::load();
                }
            });
        }
        if add {
            let t = self.aux.assistant.as_ref().map(|a| a.answer.replace('\r', "")).unwrap_or_default();
            self.cv.paste_text(&t, None);
            self.doc_changed();
            close = true;
        }
        if close {
            let k = self.aux.assistant.as_ref().map(|c| c.key);
            if let Some(k) = k {
                self.aux_close(ctl, k);
            }
        }
    }
}
