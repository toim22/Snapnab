//! The effects panel (CImageEffectHandler, ed_comp_5 §4; src/editor/main.cpp MakeEffects/ApplyFx and the EffectsPanel
//! of Panels.cpp): each effect has its settings, and the same function makes the panel thumbnail, the live preview on
//! the image and the applied result. Quick effects bundle border, edge, page curl and shadow in one step.

use std::path::PathBuf;

use egui::{Color32, RichText, TextureHandle};
use snapcore::fx::{self, BOTTOM, Edge, LEFT, RIGHT, TOP};
use snapcore::image::{Image, Rect, from_colorref};
use snapcore::xml::{self, Node};

use crate::ids::*;
use crate::tr;

#[derive(Clone, Debug)]
pub enum Param {
    Choice { label: String, items: Vec<String>, values: Vec<i32>, value: i32 },
    /// COLORREF
    Color { label: String, color: u32 },
    Toggle { label: String, on: bool },
    Text { label: String, text: String },
}

impl Param {
    pub fn value(&self) -> i32 {
        match self {
            Param::Choice { value, .. } => *value,
            Param::Toggle { on, .. } => *on as i32,
            _ => 0,
        }
    }

    pub fn color(&self) -> u32 {
        if let Param::Color { color, .. } = self { *color } else { 0 }
    }

    pub fn text(&self) -> String {
        if let Param::Text { text, .. } = self { text.clone() } else { String::new() }
    }

    pub fn set_value(&mut self, v: i32) {
        match self {
            Param::Choice { value, .. } => *value = v,
            Param::Toggle { on, .. } => *on = v != 0,
            _ => {}
        }
    }
}

fn choice(label: &str, items: Vec<String>, values: &[i32], value: i32) -> Param {
    Param::Choice { label: tr(label), items, values: values.to_vec(), value }
}

fn choice_suffix(label: &str, values: &[i32], suffix: &str, value: i32) -> Param {
    choice(label, values.iter().map(|v| format!("{v}{suffix}")).collect(), values, value)
}

fn signed_pct(label: &str, values: &[i32], value: i32) -> Param {
    choice(label, values.iter().map(|v| format!("{}{v}%", if *v > 0 { "+" } else { "" })).collect(), values, value)
}

fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| tr(s)).collect()
}

fn color(label: &str, c: u32) -> Param {
    Param::Color { label: tr(label), color: c }
}

fn toggle(label: &str, on: bool) -> Param {
    Param::Toggle { label: tr(label), on }
}

fn text(label: &str, t: String) -> Param {
    Param::Text { label: tr(label), text: t }
}

const fn rgb(r: u32, g: u32, b: u32) -> u32 {
    r | g << 8 | b << 16
}

#[derive(Clone, Debug)]
pub struct Effect {
    pub cmd: u32,
    pub name: String,
    pub desc: String,
    pub group: String,
    pub params: Vec<Param>,
}

pub fn make_effects() -> Vec<Effect> {
    let e = |cmd, name: &str, desc: &str, group: &str, params| Effect { cmd, name: tr(name), desc: tr(desc), group: tr(group), params };
    let pct9 = [-60, -45, -30, -15, 0, 15, 30, 45, 60];
    let adj = [-60, -40, -20, -10, 0, 10, 20, 40, 60];
    vec![
        e(M_FX_BORDER, "Borda", "Contorno sólido em volta da imagem, na largura e cor que você escolher.", "Bordas e sombras", vec![
            choice_suffix("Largura", &[1, 2, 3, 4, 6, 8, 10, 12, 16, 20, 24, 32], " px", 4),
            color("Cor", rgb(45, 45, 48)),
        ]),
        e(M_FX_SHADOW, "Sombra", "Sombra suave atrás da imagem, que destaca a captura do fundo.", "Bordas e sombras", vec![
            choice_suffix("Deslocamento", &[0, 2, 4, 6, 8, 12, 16, 20], " px", 6),
            choice_suffix("Desfoque", &[2, 4, 6, 8, 12, 16, 24], " px", 8),
            choice_suffix("Opacidade", &[20, 35, 50, 65, 80], "%", 50),
        ]),
        e(M_FX_PERSPECTIVE, "Perspectiva", "Inclina a imagem como se estivesse de lado, em pé ou deitada.", "Bordas e sombras", vec![
            signed_pct("Horizontal", &pct9, 30),
            signed_pct("Vertical", &pct9, 0),
            signed_pct("Inclinação", &[-40, -20, 0, 20, 40], 0),
        ]),
        e(M_FX_PAGECURL, "Dobra de página", "Dobra um canto da imagem, como a ponta de uma folha de papel.", "Bordas e sombras", vec![
            choice("Tamanho", names(&["Pequena", "Média", "Grande"]), &[10, 18, 28], 18),
            choice("Canto", names(&["Superior esquerdo", "Superior direito", "Inferior direito", "Inferior esquerdo"]), &[0, 1, 2, 3], 2),
        ]),
        e(M_FX_EDGE, "Bordas", "Rasgada, onda, serrilhada, tubarão, esmaecida ou chanfrada.", "Bordas e sombras", vec![
            choice("Estilo", names(&["Rasgada", "Onda", "Dente de serra", "Dente de tubarão", "Esmaecer", "Chanfrada"]), &[0, 1, 2, 3, 4, 5], 0),
            choice_suffix("Tamanho", &[6, 10, 14, 20, 28, 40], " px", 14),
            toggle("Em cima", false),
            toggle("Direita", true),
            toggle("Embaixo", true),
            toggle("Esquerda", false),
            toggle("Sombra", true),
        ]),
        e(M_FX_FILTER, "Filtros", "Cinza, sépia, inverter, P&B, relevo, mosaico, nitidez, desfoque.", "Cores", vec![
            choice("Filtro", names(&["Tons de cinza", "Sépia", "Inverter cores", "Preto e branco", "Relevo", "Mosaico", "Nitidez", "Desfoque"]),
                   &[0, 1, 2, 3, 4, 5, 6, 7], 0),
            choice("Intensidade", names(&["Baixa", "Média", "Alta"]), &[1, 2, 3], 2),
        ]),
        e(M_FX_ADJUST, "Ajuste de cores", "Brilho, contraste, gama, matiz e saturação.", "Cores", vec![
            signed_pct("Brilho", &adj, 0),
            signed_pct("Contraste", &adj, 20),
            choice("Gama", ["0,5", "0,7", "0,85", "1,0", "1,2", "1,5", "2,0"].iter().map(|s| s.to_string()).collect(),
                   &[50, 70, 85, 100, 120, 150, 200], 100),
            choice("Matiz", ["−90°", "−45°", "0°", "+45°", "+90°", "+180°"].iter().map(|s| s.to_string()).collect(),
                   &[-90, -45, 0, 45, 90, 180], 0),
            signed_pct("Saturação", &[-100, -60, -30, 0, 30, 60, 100], 30),
        ]),
        e(M_FX_REPLACE, "Substituição de cor", "Troca uma cor por outra (ou por transparente).", "Cores", vec![
            color("Cor atual", rgb(255, 255, 255)),
            color("Nova cor", rgb(255, 244, 196)),
            toggle("Tornar transparente", false),
            choice("Tolerância", ["0", "5", "10", "20", "35", "50"].iter().map(|s| s.to_string()).collect(), &[0, 5, 10, 20, 35, 50], 10),
        ]),
        e(M_FX_SPOTLIGHT, "Spotlight e lupa", "Destaca a seleção (ou o meio da imagem) e escurece o resto.", "Destaque", vec![
            choice_suffix("Escurecer", &[0, 20, 35, 50, 65, 80], "%", 50),
            choice_suffix("Desfoque", &[0, 2, 4, 8, 12], " px", 4),
            choice_suffix("Ampliar", &[100, 125, 150, 200], "%", 100),
            toggle("Sombra na área em foco", true),
        ]),
        e(M_FX_WATERMARK, "Marca d'água", "Texto translúcido sobre a imagem, uma vez ou em mosaico.", "Texto", vec![
            text("Texto", tr("Confidencial")),
            choice_suffix("Opacidade", &[15, 25, 35, 50, 70], "%", 35),
            toggle("Repetir em mosaico", true),
            toggle("Relevo", false),
        ]),
        e(M_FX_CAPTION, "Informações da captura", "Faixa de texto em cima ou embaixo; aceita {date} e {time}.", "Texto", vec![
            text("Texto", "{date} {time} — {user}@{computer}".into()),
            choice("Posição", names(&["Embaixo", "Em cima"]), &[0, 1], 0),
            choice_suffix("Tamanho", &[10, 12, 14, 16, 20, 24, 32], " px", 16),
            color("Cor do texto", rgb(255, 255, 255)),
            color("Cor da faixa", rgb(32, 32, 36)),
        ]),
    ]
}

fn pv(p: &[Param], i: usize) -> i32 {
    p.get(i).map_or(0, |x| x.value())
}

fn pcol(p: &[Param], i: usize) -> u32 {
    p.get(i).map_or(0, |x| x.color())
}

fn ptxt(p: &[Param], i: usize) -> Vec<u16> {
    p.get(i).map_or(Vec::new(), |x| x.text().encode_utf16().collect())
}

/// The Spotlight focus: the raster selection, or the middle of the image.
pub fn spotlight_focus(sel: Option<Rect>, w: i32, h: i32) -> Rect {
    sel.unwrap_or(Rect { left: w / 4, top: h / 4, right: w * 3 / 4, bottom: h * 3 / 4 })
}

/// Applies a panel effect; returns the image and how far the objects must move (effects that enlarge the image).
/// Pixel values are scaled by `k` (the thumbnail is smaller than the document).
pub fn apply_fx(cmd: u32, p: &[Param], src: &Image, k: f64, sel: Option<Rect>, doc_w: i32, doc_h: i32) -> (Image, (i32, i32)) {
    let px = |v: i32| ((v as f64 * k).round() as i32).max(if v > 0 { 1 } else { 0 });
    let mut img = src.clone();
    let full = Rect { left: 0, top: 0, right: img.wi(), bottom: img.hi() };
    match cmd {
        M_FX_BORDER => {
            let w = px(pv(p, 0));
            return (fx::border(&img, w, from_colorref(pcol(p, 1))), (w, w));
        }
        M_FX_SHADOW => {
            let blur = px(pv(p, 1)).max(1);
            return (fx::shadow(&img, px(pv(p, 0)), blur, pv(p, 2)), (blur * 2, blur * 2));
        }
        M_FX_PERSPECTIVE => return (fx::perspective(&img, pv(p, 0), pv(p, 1), pv(p, 2)), (0, 0)),
        M_FX_PAGECURL => return (fx::page_curl(&img, img.wi().min(img.hi()) * pv(p, 0) / 100, pv(p, 1)), (0, 0)),
        M_FX_EDGE => {
            let sides = if pv(p, 2) != 0 { TOP } else { 0 }
                | if pv(p, 3) != 0 { RIGHT } else { 0 }
                | if pv(p, 4) != 0 { BOTTOM } else { 0 }
                | if pv(p, 5) != 0 { LEFT } else { 0 };
            let sides = if sides != 0 { sides } else { BOTTOM | RIGHT };
            return (fx::edge_effect(&img, Edge::from_i32(pv(p, 0)), px(pv(p, 1)).max(2), sides, pv(p, 6) != 0), (0, 0));
        }
        M_FX_FILTER => {
            let n = pv(p, 1).clamp(1, 3);
            match pv(p, 0) {
                0 => fx::grayscale(&mut img),
                1 => fx::sepia(&mut img),
                2 => fx::invert(&mut img),
                3 => fx::monochrome(&mut img, 168 - n * 20),
                4 => fx::emboss(&mut img, n * 3),
                5 => fx::pixelate(&mut img, full, px(n * 6).max(2)),
                6 => fx::sharpen(&mut img, n * 33),
                7 => fx::box_blur(&mut img, full, px(n * 2).max(1)),
                _ => {}
            }
        }
        M_FX_ADJUST => {
            fx::brightness_contrast(&mut img, pv(p, 0), pv(p, 1));
            if pv(p, 2) != 100 {
                fx::gamma(&mut img, pv(p, 2) as f64 / 100.0);
            }
            if pv(p, 3) != 0 || pv(p, 4) != 0 {
                fx::hue_saturation(&mut img, pv(p, 3), pv(p, 4));
            }
        }
        M_FX_REPLACE => {
            let to = if pv(p, 2) != 0 { 0 } else { from_colorref(pcol(p, 1)) };
            fx::replace_color(&mut img, from_colorref(pcol(p, 0)), to, pv(p, 3));
        }
        M_FX_SPOTLIGHT => {
            let f = spotlight_focus(sel, doc_w, doc_h);
            let s = |v: i32| (v as f64 * k).round() as i32;
            let fk = Rect { left: s(f.left), top: s(f.top), right: s(f.right), bottom: s(f.bottom) };
            return (fx::spotlight(&img, fk, pv(p, 0), px(pv(p, 1)), pv(p, 2), pv(p, 3) != 0), (0, 0));
        }
        M_FX_WATERMARK => fx::watermark(&mut img, &ptxt(p, 0), pv(p, 1), pv(p, 2) != 0, pv(p, 3) != 0),
        M_FX_CAPTION => {
            let r = fx::caption(&img, &ptxt(p, 0), pv(p, 1) == 1, px(pv(p, 2)).max(6), from_colorref(pcol(p, 3)), from_colorref(pcol(p, 4)));
            let off = if pv(p, 1) == 1 { (0, r.hi() - img.hi()) } else { (0, 0) };
            return (r, off);
        }
        _ => {}
    }
    (img, (0, 0))
}

/// Effects that only enlarge the image keep the objects editable (they move along); the others flatten them first.
pub fn keeps_objects(cmd: u32) -> bool {
    matches!(cmd, M_FX_BORDER | M_FX_SHADOW | M_FX_CAPTION)
}

// ---------------------------------------------------------------- quick effects
/// CInteropQuickEffect: border + edge effect + shadow + page curl in a single step.
#[derive(Clone, Debug, PartialEq)]
pub struct QuickEffect {
    pub name: String,
    pub border: bool,
    pub border_w: i32,
    pub border_color: u32,
    pub edge: bool,
    pub edge_style: i32,
    pub edge_size: i32,
    pub edge_sides: i32,
    pub shadow: bool,
    pub shadow_offset: i32,
    pub shadow_blur: i32,
    pub shadow_opacity: i32,
    pub curl: bool,
    pub curl_size: i32,
    pub curl_corner: i32,
}

impl Default for QuickEffect {
    fn default() -> Self {
        QuickEffect {
            name: String::new(),
            border: false,
            border_w: 4,
            border_color: rgb(45, 45, 48),
            edge: false,
            edge_style: 0,
            edge_size: 14,
            edge_sides: BOTTOM | RIGHT,
            shadow: false,
            shadow_offset: 6,
            shadow_blur: 6,
            shadow_opacity: 50,
            curl: false,
            curl_size: 18,
            curl_corner: 2,
        }
    }
}

impl QuickEffect {
    /// Quick effects without edges or a page curl keep the objects editable.
    pub fn keeps_objects(&self) -> bool {
        !self.edge && !self.curl
    }
}

/// border → edge effect → page curl → shadow
pub fn apply_quick(q: &QuickEffect, src: &Image, k: f64) -> (Image, (i32, i32)) {
    let px = |v: i32| ((v as f64 * k).round() as i32).max(if v > 0 { 1 } else { 0 });
    let mut img = src.clone();
    let mut shift = 0;
    if q.border {
        let w = px(q.border_w);
        img = fx::border(&img, w, from_colorref(q.border_color));
        shift += w;
    }
    if q.edge {
        let sides = if q.edge_sides != 0 { q.edge_sides } else { BOTTOM | RIGHT };
        img = fx::edge_effect(&img, Edge::from_i32(q.edge_style), px(q.edge_size).max(2), sides, false);
    }
    if q.curl {
        img = fx::page_curl(&img, img.wi().min(img.hi()) * q.curl_size / 100, q.curl_corner);
    }
    if q.shadow {
        let blur = px(q.shadow_blur).max(1);
        img = fx::shadow(&img, px(q.shadow_offset), blur, q.shadow_opacity);
        shift += blur * 2;
    }
    (img, (shift, shift))
}

pub fn default_quick_effects() -> Vec<QuickEffect> {
    let d = QuickEffect::default;
    let all = TOP | RIGHT | BOTTOM | LEFT;
    vec![
        QuickEffect { name: "Sombra".into(), shadow: true, shadow_offset: 6, shadow_blur: 8, ..d() },
        QuickEffect { name: "Borda e sombra".into(), border: true, border_w: 3, border_color: rgb(90, 90, 96), shadow: true, ..d() },
        QuickEffect { name: "Papel rasgado".into(), edge: true, edge_style: 0, edge_size: 14, shadow: true, shadow_offset: 4, ..d() },
        QuickEffect {
            name: "Dobra de página".into(), curl: true, curl_size: 20, shadow: true, shadow_offset: 4, shadow_blur: 6, shadow_opacity: 40, ..d()
        },
        QuickEffect { name: "Onda".into(), edge: true, edge_style: 1, edge_size: 10, edge_sides: all, ..d() },
        QuickEffect { name: "Serrilhado".into(), edge: true, edge_style: 2, edge_size: 12, edge_sides: BOTTOM, shadow: true, shadow_offset: 4, ..d() },
        QuickEffect { name: "Esmaecer".into(), edge: true, edge_style: 4, edge_size: 24, edge_sides: all, ..d() },
        QuickEffect {
            name: "Chanfrado".into(), edge: true, edge_style: 5, edge_size: 10, edge_sides: all, border: true, border_w: 1,
            border_color: rgb(60, 60, 66), ..d()
        },
    ]
}

fn quick_file() -> PathBuf {
    snapcore::paths::app_data_dir().join("QuickEffects.xml")
}

fn hex(c: u32) -> String {
    snapcore::color::color_to_hex(c)
}

fn unhex(s: Option<&str>, def: u32) -> u32 {
    s.map_or(def, |s| snapcore::color::hex_to_color(&s.encode_utf16().collect::<Vec<_>>(), def))
}

fn geti(n: &Node, k: &str, def: i32) -> i32 {
    n.attr(k).and_then(|v| v.trim().parse().ok()).unwrap_or(def)
}

pub fn load_quick_effects() -> Vec<QuickEffect> {
    let Some(root) = xml::load_file(&quick_file()).filter(|r| r.name == "QuickEffects") else { return default_quick_effects() };
    let mut v = Vec::new();
    for c in root.children.iter().filter(|c| c.name == "QuickEffect") {
        let d = QuickEffect::default();
        v.push(QuickEffect {
            name: c.attr("Name").map_or_else(|| tr("Efeito"), str::to_owned),
            border: geti(c, "Border", 0) != 0,
            border_w: geti(c, "BorderWidth", 4).clamp(1, 100),
            border_color: unhex(c.attr("BorderColor"), d.border_color),
            edge: geti(c, "Edge", 0) != 0,
            edge_style: geti(c, "EdgeStyle", 0).clamp(0, 5),
            edge_size: geti(c, "EdgeSize", 14).clamp(2, 100),
            edge_sides: geti(c, "EdgeSides", d.edge_sides) & 15,
            shadow: geti(c, "Shadow", 0) != 0,
            shadow_offset: geti(c, "ShadowOffset", 6).clamp(0, 60),
            shadow_blur: geti(c, "ShadowBlur", 6).clamp(1, 60),
            shadow_opacity: geti(c, "ShadowOpacity", 50).clamp(5, 100),
            curl: geti(c, "PageCurl", 0) != 0,
            curl_size: geti(c, "PageCurlSize", 18).clamp(5, 45),
            curl_corner: geti(c, "PageCurlPos", 2).clamp(0, 3),
        });
    }
    v
}

pub fn save_quick_effects(v: &[QuickEffect]) {
    let mut root = Node::new("QuickEffects");
    for q in v {
        let c = root.add("QuickEffect");
        let b = |x: bool| (x as i32).to_string();
        c.set("Name", q.name.clone());
        c.set("Border", b(q.border)).set("BorderWidth", q.border_w.to_string()).set("BorderColor", hex(q.border_color));
        c.set("Edge", b(q.edge)).set("EdgeStyle", q.edge_style.to_string()).set("EdgeSize", q.edge_size.to_string());
        c.set("EdgeSides", q.edge_sides.to_string());
        c.set("Shadow", b(q.shadow)).set("ShadowOffset", q.shadow_offset.to_string()).set("ShadowBlur", q.shadow_blur.to_string());
        c.set("ShadowOpacity", q.shadow_opacity.to_string());
        c.set("PageCurl", b(q.curl)).set("PageCurlSize", q.curl_size.to_string()).set("PageCurlPos", q.curl_corner.to_string());
    }
    xml::save_file(&quick_file(), &root);
}

/// A quick effect from the panel's current Border, Edge, Shadow and Page curl settings (Add to quick effects).
pub fn quick_from_panel(effects: &[Effect], name: String, border: bool, edge: bool, shadow: bool, curl: bool) -> QuickEffect {
    let par = |cmd| effects.iter().find(|e| e.cmd == cmd).map(|e| e.params.clone()).unwrap_or_default();
    let (b, ed, sh, cu) = (par(M_FX_BORDER), par(M_FX_EDGE), par(M_FX_SHADOW), par(M_FX_PAGECURL));
    QuickEffect {
        name,
        border,
        border_w: pv(&b, 0),
        border_color: pcol(&b, 1),
        edge,
        edge_style: pv(&ed, 0),
        edge_size: pv(&ed, 1),
        edge_sides: if pv(&ed, 2) != 0 { TOP } else { 0 }
            | if pv(&ed, 3) != 0 { RIGHT } else { 0 }
            | if pv(&ed, 4) != 0 { BOTTOM } else { 0 }
            | if pv(&ed, 5) != 0 { LEFT } else { 0 },
        shadow,
        shadow_offset: pv(&sh, 0),
        shadow_blur: pv(&sh, 1),
        shadow_opacity: pv(&sh, 2),
        curl,
        curl_size: pv(&cu, 0),
        curl_corner: pv(&cu, 1),
    }
}

// ---------------------------------------------------------------- the panel
/// What the panel asks the window to do.
#[derive(Clone, Debug)]
pub enum FxAction {
    Preview(u32),
    Apply(u32),
    Cancel,
    Quick(usize),
    AddQuick,
    DeleteQuick(usize),
    /// a color parameter's swatch was clicked: (effect, parameter)
    PickColor(u32, usize),
}

#[derive(Default)]
pub struct EffectsPanel {
    pub effects: Vec<Effect>,
    pub quick: Vec<QuickEffect>,
    /// the effect whose settings are open (its preview is on the image)
    pub open: Option<u32>,
    /// the thumbnails (quick effects first, then the effects), redone when the document changes
    thumbs: Vec<Option<TextureHandle>>,
    thumbs_key: u64,
    /// the panel's source image (a small copy of the document)
    pub source: Option<Image>,
    pub source_key: u64,
}

impl EffectsPanel {
    pub fn new() -> EffectsPanel {
        EffectsPanel { effects: make_effects(), quick: load_quick_effects(), ..Default::default() }
    }

    pub fn find(&self, cmd: u32) -> Option<&Effect> {
        self.effects.iter().find(|e| e.cmd == cmd)
    }

    pub fn find_mut(&mut self, cmd: u32) -> Option<&mut Effect> {
        self.effects.iter_mut().find(|e| e.cmd == cmd)
    }

    pub fn expand(&mut self, cmd: u32) {
        self.open = Some(cmd);
    }

    pub fn collapse(&mut self) {
        self.open = None;
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// The thumbnails follow the document: `key` changes when it does.
    pub fn set_source(&mut self, img: Option<Image>, key: u64) {
        self.source = img;
        self.source_key = key;
    }

    pub fn invalidate_thumbs(&mut self) {
        self.thumbs_key = u64::MAX;
    }

    fn make_thumbs(&mut self, ctx: &egui::Context, doc_w: i32, doc_h: i32, sel: Option<Rect>) {
        if self.thumbs_key == self.source_key && self.thumbs.len() == self.quick.len() + self.effects.len() {
            return;
        }
        self.thumbs_key = self.source_key;
        self.thumbs.clear();
        let Some(src) = &self.source else {
            self.thumbs = vec![None; self.quick.len() + self.effects.len()];
            return;
        };
        let k = if doc_w > 0 { src.wi() as f64 / doc_w as f64 } else { 0.35 };
        let mut out = Vec::new();
        for q in &self.quick {
            out.push(apply_quick(q, src, k).0);
        }
        for e in &self.effects {
            out.push(apply_fx(e.cmd, &e.params, src, k, sel, doc_w, doc_h).0);
        }
        for (i, img) in out.into_iter().enumerate() {
            self.thumbs.push(Some(crate::canvas_ui::texture(ctx, &format!("fx-thumb-{i}"), &img, true)));
        }
    }

    /// Draws the panel; returns what the user did.
    pub fn ui(&mut self, ui: &mut egui::Ui, doc_w: i32, doc_h: i32, sel: Option<Rect>, enabled: bool) -> Vec<FxAction> {
        let mut acts = Vec::new();
        if enabled {
            self.make_thumbs(ui.ctx(), doc_w, doc_h, sel);
        }
        let nq = self.quick.len();
        egui::ScrollArea::vertical().id_salt("fx-scroll").auto_shrink([false, false]).show(ui, |ui| {
            ui.add_enabled_ui(enabled, |ui| {
                if let Some(cmd) = self.open {
                    self.settings_ui(ui, cmd, &mut acts);
                    return;
                }
                ui.label(RichText::new(tr("Efeitos rápidos")).strong());
                ui.horizontal_wrapped(|ui| {
                    for i in 0..nq {
                        let name = tr(&self.quick[i].name);
                        let r = self.thumb_button(ui, i, &name);
                        crate::panels::tag(ui, &r, &format!("SnapnabEffects.Quick.{i}"));
                        if r.clicked() {
                            acts.push(FxAction::Quick(i));
                        }
                        r.context_menu(|ui| {
                            if ui.button(tr("Excluir")).clicked() {
                                acts.push(FxAction::DeleteQuick(i));
                                ui.close();
                            }
                        });
                    }
                });
                let r = ui.button(tr("+ Adicionar aos efeitos rápidos"));
                crate::panels::tag(ui, &r, "SnapnabEffects.AddQuick");
                if r.clicked() {
                    acts.push(FxAction::AddQuick);
                }
                ui.separator();
                let mut group = String::new();
                for (j, e) in self.effects.iter().enumerate() {
                    if e.group != group {
                        group = e.group.clone();
                        ui.add_space(4.0);
                        ui.label(RichText::new(&group).strong());
                    }
                    let name = e.name.clone();
                    let r = self.thumb_button(ui, nq + j, &name).on_hover_text(&self.effects[j].desc);
                    crate::panels::tag(ui, &r, &format!("SnapnabEffects.{}", self.effects[j].cmd));
                    if r.clicked() {
                        acts.push(FxAction::Preview(self.effects[j].cmd));
                        self.open = Some(self.effects[j].cmd);
                    }
                }
            });
        });
        acts
    }

    fn thumb_button(&self, ui: &mut egui::Ui, i: usize, name: &str) -> egui::Response {
        match self.thumbs.get(i).and_then(|t| t.as_ref()) {
            Some(t) => {
                let sz = t.size_vec2();
                let s = (110.0 / sz.x.max(sz.y)).min(1.0);
                let img = egui::Image::new((t.id(), sz * s));
                ui.add(egui::Button::image_and_text(img, name))
            }
            None => ui.button(name),
        }
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui, cmd: u32, acts: &mut Vec<FxAction>) {
        let Some(e) = self.effects.iter_mut().find(|e| e.cmd == cmd) else { return };
        ui.label(RichText::new(&e.name).strong().size(15.0));
        ui.label(RichText::new(&e.desc).color(Color32::from_gray(90)));
        ui.add_space(6.0);
        let mut changed = false;
        for (i, p) in e.params.iter_mut().enumerate() {
            match p {
                Param::Choice { label, items, values, value } => {
                    let cur = values.iter().position(|v| v == value).unwrap_or(0);
                    let mut sel = cur;
                    let r = egui::ComboBox::from_label(label.as_str())
                        .selected_text(items.get(cur).cloned().unwrap_or_default())
                        .show_index(ui, &mut sel, items.len(), |k| items[k].clone());
                    crate::panels::tag(ui, &r, &format!("SnapnabEffects.Param.{i}"));
                    if sel != cur {
                        *value = values[sel];
                        changed = true;
                    }
                }
                Param::Toggle { label, on } => {
                    let r = ui.checkbox(on, label.as_str());
                    crate::panels::tag(ui, &r, &format!("SnapnabEffects.Param.{i}"));
                    changed |= r.changed();
                }
                Param::Text { label, text } => {
                    ui.label(label.as_str());
                    let r = ui.text_edit_singleline(text);
                    crate::panels::tag(ui, &r, &format!("SnapnabEffects.Param.{i}"));
                    changed |= r.changed();
                }
                Param::Color { label, color } => {
                    ui.horizontal(|ui| {
                        let c = *color;
                        let fill = Color32::from_rgb((c & 255) as u8, (c >> 8 & 255) as u8, (c >> 16 & 255) as u8);
                        let r = ui.add(egui::Button::new("    ").fill(fill));
                        crate::panels::tag(ui, &r, &format!("SnapnabEffects.Param.{i}"));
                        if r.clicked() {
                            acts.push(FxAction::PickColor(cmd, i));
                        }
                        ui.label(label.as_str());
                    });
                }
            }
        }
        if changed {
            acts.push(FxAction::Preview(cmd));
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let r = ui.add(egui::Button::new(RichText::new(tr("Aplicar")).strong()).min_size(egui::vec2(90.0, 28.0)));
            crate::panels::tag(ui, &r, "SnapnabEffects.Apply");
            if r.clicked() {
                acts.push(FxAction::Apply(cmd));
            }
            let r = ui.add(egui::Button::new(tr("Cancelar")).min_size(egui::vec2(90.0, 28.0)));
            crate::panels::tag(ui, &r, "SnapnabEffects.Cancel");
            if r.clicked() {
                acts.push(FxAction::Cancel);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn border_grows_and_moves_the_objects() {
        let img = Image::new(40, 30, 0xFFFFFFFF);
        let e = make_effects();
        let b = e.iter().find(|e| e.cmd == M_FX_BORDER).unwrap();
        let (out, off) = apply_fx(M_FX_BORDER, &b.params, &img, 1.0, None, 40, 30);
        assert_eq!((out.w, out.h), (48, 38));
        assert_eq!(off, (4, 4));
    }

    #[test]
    fn grayscale_filter_takes_out_the_color() {
        let img = Image::new(4, 4, 0xFF1E50DC);
        let e = make_effects();
        let f = e.iter().find(|e| e.cmd == M_FX_FILTER).unwrap();
        let (out, _) = apply_fx(M_FX_FILTER, &f.params, &img, 1.0, None, 4, 4);
        let p = out.at(1, 1);
        assert!((snapcore::image::cr(p) - snapcore::image::cb(p)).abs() <= 2);
    }

    #[test]
    fn eight_default_quick_effects() {
        assert_eq!(default_quick_effects().len(), 8);
        let (out, _) = apply_quick(&default_quick_effects()[0], &Image::new(20, 20, 0xFFFFFFFF), 1.0);
        assert!(out.w > 20);
    }
}
