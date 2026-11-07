//! Quick styles, tool themes and favorites (src/editor/QuickStyles.cpp; QuickStyleList, ToolThemes, FavoritesProvider,
//! ed_comp_8 §5). A theme has 4 colors; each tool's quick styles are generated from them and the user adds their own.
//! Favorites gather styles of any tool. The files are the C++ ones: ToolThemes.xml, Favorites.xml, ToolStyles.xml.

use std::path::PathBuf;

use snapcore::crt;
use snapcore::doc::*;
use snapcore::image::Image;
use snapcore::xml::{self, Node};

use crate::canvas::{TOOL_COUNT, Tool};
use crate::tr;

fn file(name: &str) -> PathBuf {
    snapcore::paths::app_data_dir().join(name)
}

// ---------------------------------------------------------------- style ⇄ XML (.snagtheme fields)
fn hex(v: u32) -> String {
    format!("#{v:08X}")
}

fn from_hex(s: Option<&str>, def: u32) -> u32 {
    let s = s.unwrap_or("");
    if s.len() < 2 || !s.starts_with('#') {
        return def;
    }
    let v = crt::strtoul(&s[1..], 16);
    if s.len() == 7 { 0xFF000000 | v } else { v }
}

fn seti(n: &mut Node, k: &str, v: impl Into<i64>) {
    n.set(k, v.into().to_string());
}

fn setd(n: &mut Node, k: &str, v: f32) {
    n.set(k, snapcore::snap::fixed4(v as f64));
}

fn geti(n: &Node, k: &str, def: i32) -> i32 {
    n.attr(k).map(crt::atoi).unwrap_or(def)
}

fn getd(n: &Node, k: &str, def: f32) -> f32 {
    n.attr(k).map(|v| crt::atof(v) as f32).unwrap_or(def)
}

fn getb(n: &Node, k: &str, def: bool) -> bool {
    geti(n, k, def as i32) != 0
}

pub fn style_to_xml(s: &Style, name: &str) -> Node {
    let mut n = Node::new(name);
    n.set("Foreground", hex(s.stroke)).set("Background", hex(s.fill)).set("TextColor", hex(s.text_color));
    setd(&mut n, "StrokeWidth", s.width);
    seti(&mut n, "Opacity", s.opacity);
    seti(&mut n, "DropShadowEnabled", s.shadow as i32);
    seti(&mut n, "ShadowDir", s.shadow_dir);
    n.set("ShadowColor", hex(s.shadow_color));
    seti(&mut n, "ShadowOpacity", s.shadow_opacity);
    seti(&mut n, "ShadowBlur", s.shadow_blur);
    seti(&mut n, "ShadowDist", s.shadow_dist);
    n.set("FontFamily", s.font.clone());
    setd(&mut n, "FontSize", s.font_size);
    seti(&mut n, "Bold", s.bold as i32);
    seti(&mut n, "Italic", s.italic as i32);
    seti(&mut n, "Underline", s.underline as i32);
    seti(&mut n, "AlignH", s.align_h);
    seti(&mut n, "AlignV", s.align_v);
    setd(&mut n, "TextPadding", s.padding);
    seti(&mut n, "LineStartCap", s.arrow_start);
    seti(&mut n, "LineEndCap", s.arrow_end);
    setd(&mut n, "StartSize", s.start_size);
    setd(&mut n, "EndSize", s.end_size);
    seti(&mut n, "DashType", s.dash);
    seti(&mut n, "Bezier", s.bezier as i32);
    seti(&mut n, "CalloutShape", s.callout_shape);
    seti(&mut n, "CalloutGradient", s.shaded as i32);
    n.set("ArrowColor", hex(s.tail_color));
    setd(&mut n, "ArrowSize", s.tail_width);
    seti(&mut n, "ToolShape", s.shape_kind);
    seti(&mut n, "StepStyle", s.step_shape);
    seti(&mut n, "StepSequenceType", s.step_type);
    seti(&mut n, "MagnifyShape", s.magnify_shape);
    seti(&mut n, "MagnifyScale", s.magnify_scale);
    seti(&mut n, "MagnifyLink", s.magnify_link as i32);
    seti(&mut n, "HideKind", s.hide_kind);
    seti(&mut n, "Antialiasing", s.antialias as i32);
    seti(&mut n, "Amount", s.amount);
    setd(&mut n, "LineSpacing", s.line_spacing);
    n.set("StepOutlineColor", hex(s.step_outline));
    setd(&mut n, "StepOutlineWidth", s.step_outline_width);
    seti(&mut n, "AreaShape", s.area_shape);
    seti(&mut n, "BackgroundBlur", s.bg_blur as i32);
    seti(&mut n, "BackgroundBlurAmount", s.bg_blur_amount);
    seti(&mut n, "Loupe", s.loupe as i32);
    seti(&mut n, "LoupeHandle", s.loupe_handle as i32);
    seti(&mut n, "TextOnly", s.text_only as i32);
    seti(&mut n, "SnapText", s.snap_text as i32);
    seti(&mut n, "Sketch", s.sketch as i32);
    seti(&mut n, "SketchSeed", s.sketch_seed);
    seti(&mut n, "VerticalText", s.vertical as i32);
    seti(&mut n, "TextOnPath", s.text_on_path as i32);
    snapcore::serial::grad_to_xml(&mut n, &s.grad);
    n
}

pub fn style_from_xml(n: &Node) -> Style {
    let d = Style::default();
    Style {
        stroke: from_hex(n.attr("Foreground"), d.stroke),
        fill: from_hex(n.attr("Background"), d.fill),
        text_color: from_hex(n.attr("TextColor"), d.text_color),
        width: getd(n, "StrokeWidth", d.width),
        opacity: geti(n, "Opacity", d.opacity).clamp(0, 100),
        shadow: getb(n, "DropShadowEnabled", d.shadow),
        shadow_dir: geti(n, "ShadowDir", d.shadow_dir).clamp(0, 8),
        shadow_color: from_hex(n.attr("ShadowColor"), d.shadow_color),
        shadow_opacity: geti(n, "ShadowOpacity", d.shadow_opacity),
        shadow_blur: geti(n, "ShadowBlur", d.shadow_blur),
        shadow_dist: geti(n, "ShadowDist", d.shadow_dist),
        font: n.attr("FontFamily").map(str::to_owned).unwrap_or(d.font.clone()),
        font_size: getd(n, "FontSize", d.font_size),
        bold: getb(n, "Bold", d.bold),
        italic: getb(n, "Italic", false),
        underline: getb(n, "Underline", false),
        align_h: geti(n, "AlignH", d.align_h).clamp(0, 2),
        align_v: geti(n, "AlignV", d.align_v).clamp(0, 2),
        padding: getd(n, "TextPadding", d.padding),
        arrow_start: geti(n, "LineStartCap", d.arrow_start).clamp(0, CAP_COUNT - 1),
        arrow_end: geti(n, "LineEndCap", d.arrow_end).clamp(0, CAP_COUNT - 1),
        start_size: getd(n, "StartSize", d.start_size),
        end_size: getd(n, "EndSize", d.end_size),
        dash: geti(n, "DashType", 0).clamp(0, DASH_COUNT - 1),
        bezier: getb(n, "Bezier", false),
        callout_shape: geti(n, "CalloutShape", d.callout_shape).clamp(0, CALLOUT_SHAPES - 1),
        shaded: getb(n, "CalloutGradient", false),
        tail_color: from_hex(n.attr("ArrowColor"), d.tail_color),
        tail_width: getd(n, "ArrowSize", d.tail_width),
        shape_kind: geti(n, "ToolShape", 0).clamp(0, SHAPE_COUNT - 1),
        step_shape: geti(n, "StepStyle", 0).clamp(0, 3),
        step_type: geti(n, "StepSequenceType", 0).clamp(0, STEP_TYPE_COUNT - 1),
        magnify_shape: geti(n, "MagnifyShape", 0).clamp(0, MAGNIFY_KIND_COUNT - 1),
        magnify_scale: geti(n, "MagnifyScale", 200).clamp(110, 1000),
        magnify_link: getb(n, "MagnifyLink", false),
        hide_kind: geti(n, "HideKind", 0).clamp(0, 1),
        antialias: getb(n, "Antialiasing", true),
        amount: geti(n, "Amount", d.amount),
        line_spacing: getd(n, "LineSpacing", 1.0).clamp(0.8, 3.0),
        step_outline: from_hex(n.attr("StepOutlineColor"), d.step_outline),
        step_outline_width: getd(n, "StepOutlineWidth", 0.0).clamp(0.0, 40.0),
        area_shape: geti(n, "AreaShape", SHAPE_RECT).clamp(0, AREA_SHAPE_COUNT - 1),
        bg_blur: getb(n, "BackgroundBlur", false),
        bg_blur_amount: geti(n, "BackgroundBlurAmount", 8).clamp(1, 40),
        loupe: getb(n, "Loupe", false),
        loupe_handle: getb(n, "LoupeHandle", true),
        text_only: getb(n, "TextOnly", false),
        // styles saved before the option: on (its default)
        snap_text: if n.attr("SnapText").is_some() { getb(n, "SnapText", true) } else { true },
        sketch: getb(n, "Sketch", false),
        sketch_seed: geti(n, "SketchSeed", 1),
        vertical: getb(n, "VerticalText", false),
        text_on_path: getb(n, "TextOnPath", false),
        grad: snapcore::serial::grad_from_xml(n),
    }
}

pub fn same_style(a: &Style, b: &Style) -> bool {
    xml::serialize(&style_to_xml(a, "Style"), false) == xml::serialize(&style_to_xml(b, "Style"), false)
}

// ---------------------------------------------------------------- generating a theme's styles
const BLACK: u32 = 0xFF1E1E1E;
const WHITE: u32 = 0xFFFFFFFF;
const GRAY: u32 = 0xFF9E9E9E;
const MAG_ROUND_RECT: i32 = 2;
const MAG_HEXAGON: i32 = 3;

pub fn generate_styles(t: Tool, c: &[u32; 4]) -> Vec<Style> {
    let mut v = Vec::new();
    let b = Style::default();
    match t {
        Tool::Arrow => {
            for row in 0..3 {
                for &ci in c {
                    let mut s = Style { stroke: ci, width: 8.0, ..b.clone() };
                    match row {
                        0 => (s.arrow_start, s.arrow_end) = (CAP_FLAT, CAP_ARROW),
                        1 => (s.arrow_start, s.arrow_end, s.width) = (CAP_ROUND, CAP_ARROW_WIDE, 6.0),
                        _ => {
                            (s.arrow_start, s.arrow_end, s.dash, s.width) = (CAP_ARROW, CAP_ARROW, DASH_DOT, 6.0);
                            (s.start_size, s.end_size) = (2.5, 2.5);
                        }
                    }
                    v.push(s);
                }
            }
            for (i, &ci) in c.iter().enumerate() {
                // hand-drawn (C35)
                v.push(Style {
                    stroke: ci,
                    width: 5.0,
                    arrow_start: CAP_FLAT,
                    arrow_end: CAP_ARROW_OPEN,
                    sketch: true,
                    sketch_seed: i as i32 + 1,
                    shadow: false,
                    ..b.clone()
                });
            }
        }
        Tool::Line => {
            for row in 0..3 {
                for &ci in c {
                    let mut s = Style { stroke: ci, arrow_start: CAP_FLAT, arrow_end: CAP_FLAT, width: 4.0, ..b.clone() };
                    if row == 1 {
                        s.dash = DASH_DASH;
                    }
                    if row == 2 {
                        (s.width, s.arrow_start, s.arrow_end) = (8.0, CAP_ROUND, CAP_ROUND);
                    }
                    v.push(s);
                }
            }
            for (i, &ci) in c.iter().enumerate() {
                v.push(Style {
                    stroke: ci,
                    arrow_start: CAP_FLAT,
                    arrow_end: CAP_FLAT,
                    width: 4.0,
                    sketch: true,
                    sketch_seed: i as i32 + 1,
                    shadow: false,
                    ..b.clone()
                });
            }
        }
        Tool::Pen => {
            for row in 0..2 {
                for &ci in c {
                    v.push(Style { stroke: ci, width: if row == 1 { 10.0 } else { 4.0 }, ..b.clone() });
                }
            }
        }
        Tool::Highlighter => {
            let hl = [0xFFFFEB3B, 0xFF76FF03, 0xFFFF4081, 0xFF40C4FF];
            for i in 0..8 {
                v.push(Style {
                    stroke: if i < 4 { hl[i] } else { c[i - 4] },
                    opacity: if i < 4 { 50 } else { 40 },
                    shadow: false,
                    snap_text: true,
                    ..b.clone()
                });
            }
            for (i, &h) in hl.iter().enumerate() {
                v.push(Style {
                    stroke: h,
                    opacity: 45,
                    shadow: false,
                    area_shape: if i < 2 { SHAPE_ELLIPSE } else { SHAPE_ROUND_RECT },
                    ..b.clone()
                });
            }
        }
        Tool::Text => {
            let neu = [[BLACK, WHITE], [WHITE, BLACK], [BLACK, 0], [GRAY, 0]];
            for row in 0..4 {
                for i in 0..4 {
                    let mut s = Style { font_size: 36.0, bold: true, align_h: 1, align_v: 1, ..b.clone() };
                    if row % 2 == 0 {
                        (s.text_color, s.stroke) = (c[i], WHITE);
                    } else {
                        s.text_color = neu[i][0];
                        s.stroke = if neu[i][1] != 0 { neu[i][1] } else { 0x00FFFFFF };
                    }
                    s.width = if row < 2 { 4.0 } else { 8.0 };
                    if row % 2 == 1 && neu[i][1] == 0 {
                        s.width = 0.0;
                    }
                    s.shadow = row >= 2;
                    v.push(s);
                }
            }
            for &ci in c {
                // hand-drawn (C35): handwriting font
                v.push(Style {
                    font_size: 36.0,
                    bold: false,
                    align_h: 1,
                    align_v: 1,
                    font: "Ink Free".into(),
                    sketch: true,
                    text_color: ci,
                    stroke: WHITE,
                    width: 4.0,
                    shadow: false,
                    ..b.clone()
                });
            }
        }
        Tool::Callout => {
            let shapes = [4, 12, 6, 12, 0];
            for (row, &shape) in shapes.iter().enumerate() {
                for &ci in c {
                    let mut s = Style {
                        font_size: 20.0,
                        bold: true,
                        padding: 8.0,
                        text_color: WHITE,
                        fill: ci,
                        stroke: ci,
                        width: 0.0,
                        tail_color: ci,
                        tail_width: 6.0,
                        callout_shape: shape,
                        ..b.clone()
                    };
                    if row == 3 {
                        (s.fill, s.width, s.text_color) = (WHITE, 3.0, 0xFF202020);
                    }
                    v.push(s);
                }
            }
        }
        Tool::Shape => {
            for &ci in c {
                v.push(Style { stroke: ci, width: 4.0, shape_kind: SHAPE_RECT, ..b.clone() });
            }
            for (i, &ci) in c.iter().enumerate() {
                v.push(Style { stroke: ci, width: 4.0, shape_kind: if i < 2 { SHAPE_ELLIPSE } else { SHAPE_ROUND_RECT }, ..b.clone() });
            }
            for (i, &ci) in c.iter().enumerate() {
                v.push(Style {
                    stroke: ci,
                    width: 4.0,
                    shape_kind: if i % 2 == 1 { SHAPE_ELLIPSE } else { SHAPE_RECT },
                    sketch: true,
                    sketch_seed: i as i32 + 1,
                    shadow: false,
                    ..b.clone()
                });
            }
            let mut s = Style { shape_kind: SHAPE_RECT, width: 0.0, fill: BLACK, stroke: BLACK, ..b.clone() };
            v.push(s.clone());
            (s.fill, s.stroke) = (WHITE, WHITE);
            v.push(s.clone());
            (s.fill, s.width, s.stroke) = (0, 6.0, BLACK);
            v.push(s.clone());
            s.stroke = WHITE;
            v.push(s);
            let mut p = Style { shape_kind: SHAPE_PARALLELOGRAM, width: 0.0, fill: BLACK, stroke: BLACK, ..b.clone() };
            v.push(p.clone());
            (p.fill, p.stroke) = (WHITE, WHITE);
            v.push(p);
        }
        Tool::Step => {
            for row in 0..4 {
                for &ci in c {
                    v.push(Style {
                        stroke: ci,
                        text_color: WHITE,
                        font_size: 26.0,
                        step_shape: (row % 2 == 1) as i32,
                        step_type: if row >= 2 { STEP_UPPER } else { STEP_NUMBERS },
                        ..b.clone()
                    });
                }
            }
            // with an outline (contrast on light and dark backgrounds)
            for row in 0..2 {
                for &ci in c {
                    let mut s = Style { font_size: 26.0, step_outline_width: 3.0, ..b.clone() };
                    if row == 0 {
                        (s.stroke, s.text_color, s.step_outline) = (ci, WHITE, WHITE);
                    } else {
                        (s.stroke, s.text_color, s.step_outline) = (WHITE, ci, ci);
                    }
                    v.push(s);
                }
            }
        }
        Tool::Fill => {
            let extra = [WHITE, 0xFFC8C8C8, 0xFF646464, 0xFF000000];
            for i in 0..8 {
                v.push(Style { stroke: if i < 4 { c[i] } else { extra[i - 4] }, shadow: false, ..b.clone() });
            }
        }
        Tool::Eraser => {
            for x in [4, 8, 12, 16, 24, 32, 48, 64] {
                v.push(Style { width: x as f32, shadow: false, ..b.clone() });
            }
        }
        Tool::Magnify => {
            for row in 0..4 {
                for &ci in c {
                    v.push(Style {
                        stroke: ci,
                        width: 4.0,
                        magnify_scale: 200,
                        magnify_shape: (row >= 2) as i32,
                        magnify_link: row % 2 == 1,
                        ..b.clone()
                    });
                }
            }
            // loupe: graphite, silver, gold and the theme's first color, with a handle
            let rim = [0xFF3C4248, 0xFFB8BEC6, 0xFFC9A227, c[0]];
            for &r in &rim {
                v.push(Style { stroke: r, width: 4.0, magnify_scale: 200, loupe: true, loupe_handle: true, ..b.clone() });
            }
            for (i, &r) in rim.iter().enumerate() {
                v.push(Style {
                    stroke: r,
                    width: 4.0,
                    magnify_scale: 250,
                    loupe: true,
                    loupe_handle: false,
                    magnify_shape: if i < 2 { MAG_ROUND_RECT } else { MAG_HEXAGON },
                    ..b.clone()
                });
            }
        }
        Tool::Blur => {
            for k in 0..6 {
                v.push(Style { shadow: false, hide_kind: k / 3, amount: (k % 3 + 1) * 6, ..b.clone() });
            }
            for k in 0..2 {
                v.push(Style { shadow: false, hide_kind: k, amount: 12, area_shape: SHAPE_ELLIPSE, ..b.clone() });
            }
        }
        Tool::Spotlight => {
            for k in 0..3 {
                v.push(Style { shadow: false, amount: 6 + k * 4, ..b.clone() });
            }
            v.push(Style { shadow: false, amount: 8, area_shape: SHAPE_ELLIPSE, ..b.clone() });
        }
        _ => {}
    }
    v
}

// ---------------------------------------------------------------- store
#[derive(Clone, Debug, Default)]
pub struct ToolTheme {
    pub name: String,
    pub colors: [u32; 4],
    pub builtin: bool,
    /// the user's own quick styles, per tool index
    pub custom: std::collections::BTreeMap<i32, Vec<Style>>,
}

#[derive(Clone, Debug)]
pub struct Favorite {
    pub tool: Tool,
    pub style: Style,
}

#[derive(Default)]
pub struct QuickStyleStore {
    pub themes: Vec<ToolTheme>,
    pub current: usize,
    pub favorites: Vec<Favorite>,
}

fn builtin_themes() -> Vec<ToolTheme> {
    let t = |name: &str, colors| ToolTheme { name: name.into(), colors, builtin: true, ..Default::default() };
    vec![
        t("Básico", [0xFFEF4136, 0xFF6A2C91, 0xFF15A589, 0xFFF7A823]),
        t("Industrial", [0xFFF5B71E, 0xFFF2845C, 0xFF2B3442, 0xFF9A8894]),
    ]
}

fn theme_to_xml(t: &ToolTheme) -> Node {
    let mut n = Node::new("Theme");
    n.set("Name", t.name.clone());
    seti(&mut n, "Builtin", t.builtin as i32);
    n.set("Version", "2.0");
    for (i, c) in t.colors.iter().enumerate() {
        n.set(&format!("Color{i}"), hex(*c));
    }
    for (tool, list) in &t.custom {
        let q = n.add("QuickStyles");
        seti(q, "ToolMode", *tool);
        for s in list {
            q.children.push(style_to_xml(s, "Style"));
        }
    }
    n
}

fn theme_from_xml(n: &Node) -> ToolTheme {
    let mut t = ToolTheme {
        name: n.attr("Name").map(str::to_owned).unwrap_or_else(|| tr("Tema")),
        builtin: getb(n, "Builtin", false),
        ..Default::default()
    };
    for i in 0..4 {
        t.colors[i] = from_hex(n.attr(&format!("Color{i}")), 0xFF808080);
    }
    for q in n.children.iter().filter(|q| q.name == "QuickStyles") {
        let list = t.custom.entry(geti(q, "ToolMode", 0)).or_default();
        list.extend(q.children.iter().map(style_from_xml));
    }
    t
}

impl QuickStyleStore {
    /// ToolThemes.xml + Favorites.xml (the defaults when missing).
    pub fn load() -> QuickStyleStore {
        let mut s = QuickStyleStore::default();
        if let Some(root) = xml::load_file(&file("ToolThemes.xml")) {
            s.themes = root.children.iter().filter(|c| c.name == "Theme").map(theme_from_xml).collect();
            s.current = geti(&root, "Current", 0).max(0) as usize;
        }
        for b in builtin_themes() {
            // the built-in themes always exist (with the original colors)
            match s.themes.iter_mut().find(|t| t.builtin && t.name == b.name) {
                Some(t) => t.colors = b.colors,
                None => {
                    let at = s.themes.len().min(if b.name == "Básico" { 0 } else { 1 });
                    s.themes.insert(at, b);
                }
            }
        }
        s.current = s.current.min(s.themes.len() - 1);
        match xml::load_file(&file("Favorites.xml")) {
            Some(fav) => {
                for c in fav.children.iter().filter(|c| c.name == "Favorite" && !c.children.is_empty()) {
                    let t = geti(c, "ToolMode", -1);
                    if let Some(tool) = (t >= 0).then(|| Tool::from_index(t as usize)).flatten() {
                        s.favorites.push(Favorite { tool, style: style_from_xml(&c.children[0]) });
                    }
                }
            }
            None => {
                // LoadDefaultFavorites: the theme's arrow, text, rectangle and ellipse
                let c = s.themes[0].colors;
                let (a, tx, sh) = (generate_styles(Tool::Arrow, &c), generate_styles(Tool::Text, &c), generate_styles(Tool::Shape, &c));
                s.favorites = vec![
                    Favorite { tool: Tool::Arrow, style: a[0].clone() },
                    Favorite { tool: Tool::Text, style: tx[0].clone() },
                    Favorite { tool: Tool::Shape, style: sh[0].clone() },
                    Favorite { tool: Tool::Shape, style: sh[4].clone() },
                ];
            }
        }
        s
    }

    pub fn save(&self) {
        let mut root = Node::new("ToolThemes");
        seti(&mut root, "Current", self.current as i32);
        root.children.extend(self.themes.iter().map(theme_to_xml));
        xml::save_file(&file("ToolThemes.xml"), &root);
    }

    pub fn save_favorites(&self) {
        let mut root = Node::new("Favorites");
        for f in &self.favorites {
            let n = root.add("Favorite");
            seti(n, "ToolMode", f.tool.index() as i32);
            n.children.push(style_to_xml(&f.style, "Style"));
        }
        xml::save_file(&file("Favorites.xml"), &root);
    }

    pub fn theme(&self) -> &ToolTheme {
        &self.themes[self.current.min(self.themes.len() - 1)]
    }

    pub fn generated_count(&self, t: Tool) -> usize {
        generate_styles(t, &self.theme().colors).len()
    }

    /// Generated from the theme, then the custom ones.
    pub fn styles_for(&self, t: Tool) -> Vec<Style> {
        let mut v = generate_styles(t, &self.theme().colors);
        if let Some(c) = self.theme().custom.get(&(t.index() as i32)) {
            v.extend(c.iter().cloned());
        }
        v
    }

    pub fn add_custom(&mut self, t: Tool, s: &Style) {
        if self.styles_for(t).iter().any(|e| same_style(e, s)) {
            return;
        }
        let cur = self.current;
        self.themes[cur].custom.entry(t.index() as i32).or_default().push(s.clone());
        self.save();
    }

    pub fn remove_custom(&mut self, t: Tool, i: usize) {
        let cur = self.current;
        let list = self.themes[cur].custom.entry(t.index() as i32).or_default();
        if i < list.len() {
            list.remove(i);
        }
        self.save();
    }

    pub fn is_favorite(&self, t: Tool, s: &Style) -> bool {
        self.favorites.iter().any(|f| f.tool == t && same_style(&f.style, s))
    }

    pub fn toggle_favorite(&mut self, t: Tool, s: &Style) {
        if let Some(i) = self.favorites.iter().position(|f| f.tool == t && same_style(&f.style, s)) {
            self.favorites.remove(i);
        } else {
            self.favorites.push(Favorite { tool: t, style: s.clone() });
        }
        self.save_favorites();
    }

    /// A repeated name gets " (2)" (HandleCollisionAndAddTheme).
    pub fn add_theme(&mut self, mut th: ToolTheme) -> usize {
        let base = if th.name.is_empty() { tr("Tema") } else { th.name.clone() };
        th.name = base.clone();
        let mut n = 2;
        while self.themes.iter().any(|t| t.name == th.name) {
            th.name = format!("{base} ({n})");
            n += 1;
        }
        th.builtin = false;
        self.themes.push(th);
        self.current = self.themes.len() - 1;
        self.save();
        self.current
    }

    pub fn import_theme(&mut self, path: &str) -> Result<(), String> {
        match xml::load_file(std::path::Path::new(path)) {
            Some(n) if n.name == "Theme" => {
                self.add_theme(theme_from_xml(&n));
                Ok(())
            }
            _ => Err(tr("O arquivo não é um tema do Snapnab (.snaptheme).")),
        }
    }

    pub fn export_theme(&self, i: usize, path: &str) -> bool {
        let Some(t) = self.themes.get(i) else { return false };
        let t = ToolTheme { builtin: false, ..t.clone() };
        xml::save_file(std::path::Path::new(path), &theme_to_xml(&t))
    }

    pub fn delete_theme(&mut self, i: usize) {
        if i >= self.themes.len() || self.themes[i].builtin {
            return;
        }
        self.themes.remove(i);
        self.current = (if self.current >= i { self.current.saturating_sub(1) } else { self.current }).min(self.themes.len() - 1);
        self.save();
    }
}

/// Each tool's style from the previous session (ToolStyles.xml).
pub fn save_tool_styles(styles: &[Style]) {
    let mut root = Node::new("ToolStyles");
    for (t, s) in styles.iter().enumerate() {
        let mut n = style_to_xml(s, "Tool");
        seti(&mut n, "ToolMode", t as i32);
        root.children.push(n);
    }
    xml::save_file(&file("ToolStyles.xml"), &root);
}

pub fn load_tool_styles(styles: &mut [Style]) {
    let Some(root) = xml::load_file(&file("ToolStyles.xml")) else { return };
    for n in &root.children {
        let t = geti(n, "ToolMode", -1);
        if t >= 0 && (t as usize) < TOOL_COUNT.min(styles.len()) {
            styles[t as usize] = style_from_xml(n);
        }
    }
}

// ---------------------------------------------------------------- thumbnails
fn landscape(s: i32) -> Image {
    // background of the magnifier thumbnails: sky, mountains and snow
    let mut img = Image::new(s as u32, s as u32, 0xFF7FB2E5);
    let sf = s as f32;
    for y in 0..s {
        let t = y as f32 / sf;
        let mix = |a: u32, b: u32| (a as f32 + (b as f32 - a as f32) * t) as u32;
        let c = 0xFF000000 | mix(120, 200) << 16 | mix(170, 225) << 8 | mix(225, 245);
        for x in 0..s {
            *img.at_mut(x, y) = c;
        }
    }
    let inside = |pts: &[(f32, f32)], x: f32, y: f32| {
        let mut c = false;
        let n = pts.len();
        for i in 0..n {
            let (a, b) = (pts[i], pts[(i + n - 1) % n]);
            if (a.1 > y) != (b.1 > y) && x < (b.0 - a.0) * (y - a.1) / (b.1 - a.1) + a.0 {
                c = !c;
            }
        }
        c
    };
    let m1 = [(0.0, sf * 0.75), (sf * 0.3, sf * 0.35), (sf * 0.55, sf * 0.65), (sf * 0.8, sf * 0.3), (sf, sf * 0.6), (sf, sf), (0.0, sf)];
    let snow = [(sf * 0.22, sf * 0.47), (sf * 0.3, sf * 0.35), (sf * 0.38, sf * 0.47)];
    let snow2 = [(sf * 0.72, sf * 0.42), (sf * 0.8, sf * 0.3), (sf * 0.88, sf * 0.42)];
    for y in 0..s {
        for x in 0..s {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            if inside(&m1, fx, fy) {
                *img.at_mut(x, y) = 0xFF465A78;
            }
            if inside(&snow, fx, fy) || inside(&snow2, fx, fy) {
                *img.at_mut(x, y) = 0xFFF5F8FC;
            }
            if fy >= sf * 0.85 {
                *img.at_mut(x, y) = 0xFF3C6E46;
            }
        }
    }
    img
}

fn lines(s: i32) -> Image {
    // background of blur/highlight: "text" in lines
    let mut img = Image::new(s as u32, s as u32, 0xFFFFFFFF);
    let mut y = s / 8;
    while y < s - s / 8 {
        for x in s / 10..s - s / 10 {
            for k in 0..(s / 24).max(2) {
                if (x / (s / 12 + 1)) % 3 != 2 {
                    *img.at_mut(x, y + k) = if (x / 3 + y) % 2 == 1 { 0xFF3A5A8A } else { 0xFF2D2D30 };
                }
            }
        }
        y += s / 8;
    }
    img
}

fn pf(x: f32, y: f32) -> PointF {
    PointF { X: x, Y: y }
}

/// The thumbnail of a style in the quick styles gallery.
pub fn quick_style_preview(t: Tool, st: &Style, px: i32) -> Image {
    const S: i32 = 96;
    let mut canvas = Image::new(S as u32, S as u32, 0);
    let mut s = st.clone();
    s.shadow_dist = (s.shadow_dist / 2).max(2);
    s.shadow_blur = (s.shadow_blur / 2).max(2);
    let mut o = DrawObj::new(ObjType::Arrow);
    o.style = s.clone();
    let mut draw = true;
    match t {
        Tool::Arrow | Tool::Line => {
            o.kind = if t == Tool::Arrow { ObjType::Arrow } else { ObjType::Line };
            (o.p1, o.p2) = (pf(16.0, 80.0), pf(80.0, 16.0));
            o.style.width = s.width.clamp(3.0, 12.0);
        }
        Tool::Pen => {
            o.kind = ObjType::Pen;
            o.pts = (0..=16).map(|i| pf(14.0 + i as f32 * 4.2, 48.0 - (i as f64 * 0.5).sin() as f32 * 22.0)).collect();
            o.style.width = s.width.clamp(3.0, 12.0);
        }
        Tool::Highlighter => {
            canvas = lines(S);
            o.kind = ObjType::Highlighter;
            (o.p1, o.p2) = (pf(8.0, 30.0), pf(88.0, 66.0));
            if s.area_shape != SHAPE_RECT {
                (o.p1, o.p2) = (pf(12.0, 16.0), pf(84.0, 80.0));
            }
            if is_point_area(s.area_shape) {
                o.pts = vec![pf(10.0, 40.0), pf(46.0, 22.0), pf(86.0, 34.0), pf(74.0, 70.0), pf(22.0, 72.0)];
            }
        }
        Tool::Shape => {
            o.kind = ObjType::Shape;
            (o.p1, o.p2) = (pf(14.0, 24.0), pf(82.0, 72.0));
            o.style.width = s.width.clamp(0.0, 8.0);
            if s.width > 0.0 && s.width < 3.0 {
                o.style.width = 3.0;
            }
        }
        Tool::Text => {
            o.kind = ObjType::Text;
            (o.p1, o.p2) = (pf(0.0, 0.0), pf(96.0, 96.0));
            o.text = "A".encode_utf16().collect();
            o.style.font_size = 66.0;
            (o.style.align_h, o.style.align_v, o.style.padding) = (1, 1, 0.0);
            o.style.width = if s.width > 0.0 { (s.width * 0.9).clamp(3.0, 9.0) } else { 0.0 };
        }
        Tool::Callout => {
            o.kind = ObjType::Callout;
            o.text = "A".encode_utf16().collect();
            (o.style.font_size, o.style.padding, o.style.tail_width, o.style.end_size) = (22.0, 2.0, 5.0, 2.5);
            o.style.width = s.width.min(4.0);
            let sh = s.callout_shape;
            (o.p1, o.p2, o.tail) = if (5..=11).contains(&sh) {
                (pf(10.0, 26.0), pf(86.0, 70.0), pf(120.0, 48.0))
            } else if sh == 4 || sh == 15 {
                (pf(34.0, 12.0), pf(86.0, 50.0), pf(12.0, 84.0))
            } else if (16..=21).contains(&sh) {
                (pf(12.0, 20.0), pf(84.0, 76.0), pf(48.0, 48.0))
            } else {
                (pf(14.0, 12.0), pf(82.0, 62.0), pf(28.0, 86.0))
            };
        }
        Tool::Step => {
            o.kind = ObjType::Step;
            let c = if s.step_shape == 1 { pf(40.0, 48.0) } else { pf(48.0, 48.0) };
            (o.p1, o.p2) = (c, c);
            o.style.font_size = 40.0;
            o.step_value = 1;
        }
        Tool::Magnify => {
            canvas = landscape(S);
            o.kind = ObjType::Magnify;
            o.style.width = s.width.clamp(2.0, 5.0);
            if s.magnify_link {
                (o.p1, o.p2, o.tail) = (pf(44.0, 8.0), pf(88.0, 52.0), pf(26.0, 70.0));
            } else if s.loupe && s.loupe_handle {
                (o.p1, o.p2) = (pf(10.0, 10.0), pf(64.0, 64.0));
            } else {
                (o.p1, o.p2) = (pf(18.0, 18.0), pf(78.0, 78.0));
            }
        }
        Tool::Blur | Tool::Spotlight => {
            canvas = lines(S);
            o.kind = if t == Tool::Spotlight {
                ObjType::Spotlight
            } else if s.hide_kind != 0 {
                ObjType::Pixelate
            } else {
                ObjType::Blur
            };
            (o.p1, o.p2) = (pf(20.0, 20.0), pf(76.0, 76.0));
            o.style.amount = (s.amount / 2).max(2);
            if is_point_area(s.area_shape) {
                o.pts = vec![pf(22.0, 30.0), pf(52.0, 14.0), pf(82.0, 34.0), pf(70.0, 80.0), pf(18.0, 70.0)];
            }
        }
        _ => draw = false,
    }
    if draw {
        canvas = snapcore::render::render(&canvas, &[o], None);
    } else if t == Tool::Fill {
        // paint bucket in the color: a diamond and a drop
        let c = s.stroke | 0xFF000000;
        let edge = if s.stroke & 0xFFFFFF == 0xFFFFFF {
            0xFF969696
        } else {
            let d = |v: u32| v.saturating_sub(70);
            0xFF000000 | d(c >> 16 & 255) << 16 | d(c >> 8 & 255) << 8 | d(c & 255)
        };
        for y in 0..S {
            for x in 0..S {
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                let dd = (fx - 48.0).abs() + (fy - 46.0).abs();
                if dd <= 28.0 {
                    *canvas.at_mut(x, y) = if dd > 25.5 { edge } else { c };
                }
                let (ex, ey) = ((fx - 77.0) / 7.0, (fy - 66.0) / 10.0);
                let e = ex * ex + ey * ey;
                if e <= 1.0 {
                    *canvas.at_mut(x, y) = if e > 0.75 { edge } else { c };
                }
            }
        }
    } else if t == Tool::Eraser {
        // brush size: checkerboard inside the circle
        let d = (s.width * 1.2).clamp(6.0, 80.0);
        for y in 0..S {
            for x in 0..S {
                let (dx, dy) = (x as f32 + 0.5 - 48.0, y as f32 + 0.5 - 48.0);
                let r = (dx * dx + dy * dy).sqrt();
                if r <= d / 2.0 {
                    *canvas.at_mut(x, y) = if (x / 6 + y / 6) % 2 == 1 { 0xFFFFFFFF } else { 0xFFD0D0D4 };
                }
                if (r - d / 2.0).abs() < 1.2 {
                    *canvas.at_mut(x, y) = 0xFF8A8A90;
                }
            }
        }
    }
    snapcore::gdip::scale_image(&canvas, px, px, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_style_survives_its_xml() {
        let s = Style { stroke: 0xFF112233, width: 7.5, shape_kind: SHAPE_ELLIPSE, sketch: true, ..Style::default() };
        let back = style_from_xml(&style_to_xml(&s, "Style"));
        assert!(same_style(&s, &back));
    }

    #[test]
    fn every_tool_with_quick_styles_has_them() {
        let c = builtin_themes()[0].colors;
        assert_eq!(generate_styles(Tool::Arrow, &c).len(), 16);
        assert_eq!(generate_styles(Tool::Shape, &c).len(), 18);
        assert!(generate_styles(Tool::Select, &c).is_empty());
        assert_eq!(quick_style_preview(Tool::Arrow, &generate_styles(Tool::Arrow, &c)[0], 48).w, 48);
    }
}
