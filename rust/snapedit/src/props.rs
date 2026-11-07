//! The properties panel (src/editor/PropsPanel.cpp): quick styles (theme, gallery, favorites), the stamp gallery and
//! the properties of each tool, then the exact position and size and the object buttons. The C++ panel drew its own
//! controls with GDI+; here they are egui widgets with the same labels, ranges and behavior (the look comes in the
//! visual pass). Dialogs (color, forms, files) are modal loops, so the panel hands them to the window as `Modal` jobs
//! that run on a worker thread and come back as a change to apply.

use std::collections::HashMap;

use egui::{Color32, RichText, Sense, TextureHandle, Vec2};
use snapcore::doc::*;
use snapcore::image::{Image, from_colorref};
use snapui::form::Field;

use crate::canvas::{Canvas, Tool};
use crate::stamps::{self, StampCategory};
use crate::styles::{QuickStyleStore, ToolTheme, quick_style_preview, same_style};
use crate::tr;

/// What the panel asks of the window after a frame.
pub enum PropAction {
    /// the buttons 324–331 (crop / selection / order / delete / group), handled as menu commands
    Command(u32),
    Hint(String),
    /// a favorite of another tool was picked: the toolbar switches to it
    PickTool(Tool),
    /// the tool styles changed (saved at exit, the toolbar's preview follows)
    StylesChanged,
    /// the document changed outside the canvas' own notes (step reset, lock…)
    DocChanged,
    /// a modal dialog: run `job(owner)` on a worker thread, then apply what it returns on the UI thread
    Modal(ModalJob),
}

pub type Apply = Box<dyn FnOnce(&mut PropsPanel, &mut Canvas, &mut QuickStyleStore) -> Vec<PropAction> + Send>;
pub type ModalJob = Box<dyn FnOnce(isize) -> Option<Apply> + Send>;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum C {
    Width,
    OutlineW,
    Opacity,
    StartSize,
    EndSize,
    FontSize,
    TailSize,
    Padding,
    MagScale,
    Amount,
    LineSpacing,
    BgBlur,
    BgBlurAmt,
    FillAlpha,
    StepOutlineW,
    Loupe,
    LoupeHandle,
    TextOnly,
    SnapText,
    Sketch,
    Vertical,
    TextOnPath,
    Aspect,
    Tolerance,
    Bezier,
    Shaded,
    MagLink,
    Antialias,
    Global,
    SmartMove,
    Bold,
    Italic,
    Underline,
    AlignH(i32),
    AlignV(i32),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ColorTarget {
    Color,
    Fill,
    Outline,
    TextColor,
    TailColor,
    StepOutline,
}

/// order of the ends in the list (as in the reference): arrow, wide arrow, open arrow, flat, round, dot, square, diamond
const CAP_ORDER: [i32; 8] = [CAP_ARROW, CAP_ARROW_WIDE, CAP_ARROW_OPEN, CAP_FLAT, CAP_ROUND, CAP_DOT, CAP_SQUARE, CAP_DIAMOND];

const STANDARD: [u32; 40] = [
    0xFFFFFFFF, 0xFFE0E0E0, 0xFFBDBDBD, 0xFF9E9E9E, 0xFF757575, 0xFF424242, 0xFF212121, 0xFF000000, 0xFFFFCDD2, 0xFFFFE0B2,
    0xFFFFF9C4, 0xFFC8E6C9, 0xFFB3E5FC, 0xFFC5CAE9, 0xFFE1BEE7, 0xFFF8BBD0, 0xFFEF5350, 0xFFFFA726, 0xFFFFEE58, 0xFF66BB6A,
    0xFF29B6F6, 0xFF5C6BC0, 0xFFAB47BC, 0xFFEC407A, 0xFFE53935, 0xFFFB8C00, 0xFFFDD835, 0xFF43A047, 0xFF039BE5, 0xFF3949AB,
    0xFF8E24AA, 0xFFD81B60, 0xFFB71C1C, 0xFFE65100, 0xFFF57F17, 0xFF1B5E20, 0xFF01579B, 0xFF1A237E, 0xFF4A148C, 0xFF880E4F,
];

pub fn tool_for_obj(t: ObjType) -> Tool {
    match t {
        ObjType::Arrow => Tool::Arrow,
        ObjType::Line => Tool::Line,
        ObjType::Rect | ObjType::Ellipse | ObjType::RoundRect | ObjType::Shape => Tool::Shape,
        ObjType::Pen => Tool::Pen,
        ObjType::Highlighter => Tool::Highlighter,
        ObjType::Text => Tool::Text,
        ObjType::Callout => Tool::Callout,
        ObjType::Step => Tool::Step,
        ObjType::Stamp => Tool::Stamp,
        ObjType::Blur | ObjType::Pixelate => Tool::Blur,
        ObjType::Spotlight => Tool::Spotlight,
        ObjType::Magnify => Tool::Magnify,
        _ => Tool::Select,
    }
}

pub fn help_for(t: Tool) -> String {
    tr(match t {
        Tool::Arrow => "Arraste para desenhar uma seta. Shift trava em 45°. Com “Curva de Bézier”, arraste a alça do meio para curvar.",
        Tool::Line => "Arraste para desenhar uma linha. Shift trava em 45°.",
        Tool::Shape => "Arraste para desenhar a forma. Shift deixa a forma perfeita (quadrado, círculo, triângulo equilátero, hexágono regular).",
        Tool::Pen => "Arraste para desenhar à mão livre; segure Shift para um trecho reto. Para virar linha, seta, retângulo, elipse ou triângulo, pare o mouse por um instante antes de soltar.",
        Tool::Highlighter => "Arraste sobre o que quer destacar. Em Forma, escolha retângulo, elipse, polígono (clique nos cantos) ou mão livre; Shift deixa a forma perfeita.",
        Tool::Text => "Clique ou arraste para criar uma caixa de texto. Duplo clique edita o texto.",
        Tool::Callout => "Arraste para criar o balão e digite. A alça amarela move a ponta (ou a direção das setas).",
        Tool::Step => "Clique para numerar passos em sequência. O Tipo escolhe números, letras ou algarismos romanos.",
        Tool::Stamp => "Escolha um carimbo na galeria e clique ou arraste na imagem. “Importar…” acrescenta imagens suas.",
        Tool::Fill => "Clique para preencher a área de cor semelhante. O conta-gotas pega a cor de preenchimento da imagem.",
        Tool::Eraser => "Arraste para apagar pixels da imagem (ficam transparentes).",
        Tool::Magnify => "Arraste para criar a lupa. Com a lente separada, a alça amarela escolhe a área ampliada.",
        Tool::Blur => "Arraste sobre a área a ocultar (desfoque ou pixelado). Em Forma, escolha elipse, polígono ou mão livre; Shift deixa a forma perfeita.",
        Tool::Spotlight => "Arraste a área em destaque; o resto escurece. Em Forma, escolha elipse, polígono ou mão livre; Shift deixa a forma perfeita.",
        Tool::Select => "Clique num objeto para selecionar e arrastar. Com Smart Move, arraste uma área da imagem para movê-la; o fundo é preenchido.",
        Tool::Crop => "Arraste a área a manter; Enter ou duplo clique aplica.",
        Tool::Selection | Tool::MagicWand => "Selecione e arraste para mover (Smart Move); Del apaga; Ctrl+C copia.",
        Tool::Cutout => "Arraste sobre a faixa a remover; as partes são unidas.",
        Tool::Eyedropper => "Clique na imagem para pegar a cor.",
        Tool::Favorites => "",
    })
}

/// The TrueType font families, sorted (the font lists of Text, Callout and the arrow labels).
pub fn fonts() -> &'static [String] {
    use std::sync::OnceLock;
    use windows::Win32::Foundation::LPARAM;
    use windows::Win32::Graphics::Gdi::{
        DEFAULT_CHARSET, EnumFontFamiliesExW, GetDC, LOGFONTW, ReleaseDC, TEXTMETRICW, TRUETYPE_FONTTYPE,
    };
    static LIST: OnceLock<Vec<String>> = OnceLock::new();
    unsafe extern "system" fn cb(l: *const LOGFONTW, _: *const TEXTMETRICW, ty: u32, p: LPARAM) -> i32 {
        unsafe {
            let v = &mut *(p.0 as *mut Vec<String>);
            let face = &(*l).lfFaceName;
            let n = face.iter().position(|&c| c == 0).unwrap_or(face.len());
            if n > 0 && face[0] != '@' as u16 && ty & TRUETYPE_FONTTYPE != 0 {
                v.push(String::from_utf16_lossy(&face[..n]));
            }
        }
        1
    }
    LIST.get_or_init(|| {
        let mut v: Vec<String> = Vec::new();
        unsafe {
            let dc = GetDC(None);
            let lf = LOGFONTW { lfCharSet: DEFAULT_CHARSET, ..Default::default() };
            EnumFontFamiliesExW(dc, &lf, Some(cb), LPARAM(&mut v as *mut _ as isize), 0);
            ReleaseDC(None, dc);
        }
        v.sort_by_key(|a| a.to_lowercase());
        v.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        if v.is_empty() {
            v = vec!["Segoe UI".into(), "Arial".into()];
        }
        v
    })
}

fn reg_dword(name: &str, def: u32) -> u32 {
    snapcore::reg::get_dword(std::ffi::OsStr::new(name), def, Some(std::ffi::OsStr::new("Editor")))
}
fn set_reg_dword(name: &str, v: u32) {
    snapcore::reg::set_dword(std::ffi::OsStr::new(name), v, Some(std::ffi::OsStr::new("Editor")));
}
fn reg_string(name: &str) -> String {
    snapcore::reg::get_string(std::ffi::OsStr::new(name), Some(std::ffi::OsStr::new("Editor")))
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}
fn set_reg_string(name: &str, v: &str) {
    snapcore::reg::set_string(std::ffi::OsStr::new(name), std::ffi::OsStr::new(v), Some(std::ffi::OsStr::new("Editor")));
}

pub fn argb32(c: u32) -> Color32 {
    Color32::from_rgba_unmultiplied((c >> 16 & 255) as u8, (c >> 8 & 255) as u8, (c & 255) as u8, (c >> 24) as u8)
}
pub fn to_colorref(c: u32) -> u32 {
    (c & 255) << 16 | (c >> 8 & 255) << 8 | (c >> 16 & 255)
}

#[derive(Default)]
pub struct PropsPanel {
    /// the toolbar's item (Favorites shows the favorite styles of every tool)
    pub bar_tool: Tool,
    /// the document information shown at the bottom
    pub doc_info: String,
    advanced: bool,
    aspect_lock: bool,
    stamp_cats: Vec<StampCategory>,
    stamp_cat: usize,
    stamp_search: String,
    recent: Vec<u32>,
    thumbs: HashMap<String, TextureHandle>,
    geom: [String; 4],
    geom_aspect: f32,
    geom_first: bool,
    geom_focus: bool,
    drag_stamp: Option<usize>,
    refresh_stamps: bool,
    loaded: bool,
}

impl PropsPanel {
    pub fn new() -> PropsPanel {
        let mut p = PropsPanel {
            advanced: reg_dword("PropsAdvanced", 1) != 0,
            aspect_lock: reg_dword("GeometryAspectLock", 0) != 0,
            loaded: true,
            ..Default::default()
        };
        p.fill_stamp_cats();
        p
    }

    // ------------------------------------------------------------ state
    fn obj_selected(cv: &Canvas) -> bool {
        cv.doc().is_some_and(|d| cv.selected >= 0 && (cv.selected as usize) < d.st.objs.len())
    }

    fn sel_obj(cv: &Canvas) -> Option<&DrawObj> {
        let d = cv.doc()?;
        (cv.selected >= 0).then(|| d.st.objs.get(cv.selected as usize).map(|o| &**o)).flatten()
    }

    pub fn prop_tool(cv: &Canvas) -> Tool {
        match Self::sel_obj(cv) {
            Some(o) => tool_for_obj(o.kind),
            None if cv.tool == Tool::MagicWand => Tool::Selection,
            None => cv.tool,
        }
    }

    fn style_tool(&self, cv: &Canvas) -> Tool {
        if !Self::obj_selected(cv) && self.bar_tool == Tool::Favorites { Tool::Favorites } else { Self::prop_tool(cv) }
    }

    fn cur(cv: &Canvas) -> Style {
        if let Some(o) = Self::sel_obj(cv) {
            let mut s = o.style.clone();
            if o.is_shape() {
                s.shape_kind = o.shape_kind();
            }
            if o.kind == ObjType::Pixelate {
                s.hide_kind = 1;
            }
            return s;
        }
        cv.styles[Self::prop_tool(cv).index()].clone()
    }

    fn set_style(cv: &mut Canvas, s: Style, new_step: bool, acts: &mut Vec<PropAction>) {
        let t = Self::prop_tool(cv);
        cv.styles[t.index()] = s.clone();
        if Self::obj_selected(cv) {
            cv.apply_style_to_selected(&s, new_step);
        }
        acts.push(PropAction::StylesChanged);
    }

    fn value(&self, cv: &Canvas, id: C) -> i32 {
        let s = Self::cur(cv);
        match id {
            C::Width | C::OutlineW => s.width.round() as i32,
            C::Opacity => s.opacity,
            C::StartSize => s.start_size.round() as i32,
            C::EndSize => s.end_size.round() as i32,
            C::FontSize => s.font_size.round() as i32,
            C::TailSize => s.tail_width.round() as i32,
            C::Padding => s.padding.round() as i32,
            C::MagScale => s.magnify_scale,
            C::Amount => s.amount,
            C::LineSpacing => (s.line_spacing * 100.0).round() as i32,
            C::BgBlur => s.bg_blur as i32,
            C::BgBlurAmt => s.bg_blur_amount,
            C::FillAlpha => ((s.fill >> 24) as f64 * 100.0 / 255.0).round() as i32,
            C::StepOutlineW => s.step_outline_width.round() as i32,
            C::Loupe => s.loupe as i32,
            C::LoupeHandle => s.loupe_handle as i32,
            C::TextOnly => s.text_only as i32,
            C::SnapText => s.snap_text as i32,
            C::Sketch => s.sketch as i32,
            C::Vertical => s.vertical as i32,
            C::TextOnPath => s.text_on_path as i32,
            C::Aspect => self.aspect_lock as i32,
            C::Tolerance => cv.tolerance as i32,
            C::Bezier => s.bezier as i32,
            C::Shaded => s.shaded as i32,
            C::MagLink => s.magnify_link as i32,
            C::Antialias => s.antialias as i32,
            C::Global => cv.wand_global as i32,
            C::SmartMove => cv.smart_move as i32,
            C::Bold => s.bold as i32,
            C::Italic => s.italic as i32,
            C::Underline => s.underline as i32,
            C::AlignH(a) => (s.align_h == a) as i32,
            C::AlignV(a) => (s.align_v == a) as i32,
        }
    }

    fn set_value(&mut self, cv: &mut Canvas, id: C, v: i32, new_step: bool, acts: &mut Vec<PropAction>) {
        match id {
            C::Tolerance => {
                cv.tolerance = v.clamp(0, 100) as f32;
                return;
            }
            C::Global => {
                cv.wand_global = v != 0;
                return;
            }
            C::SmartMove => {
                cv.smart_move = v != 0;
                if v == 0 {
                    cv.clear_raster_selection();
                }
                return;
            }
            C::Aspect => {
                self.aspect_lock = v != 0;
                set_reg_dword("GeometryAspectLock", v as u32);
                return;
            }
            _ => {}
        }
        let mut s = Self::cur(cv);
        let b = v != 0;
        match id {
            C::Width | C::OutlineW => s.width = v as f32,
            C::Opacity => s.opacity = v.clamp(0, 100),
            C::StartSize => s.start_size = v as f32,
            C::EndSize => s.end_size = v as f32,
            C::FontSize => s.font_size = v.max(4) as f32,
            C::TailSize => s.tail_width = v.max(1) as f32,
            C::Padding => s.padding = v as f32,
            C::MagScale => s.magnify_scale = v.clamp(110, 1000),
            C::Amount => s.amount = v.max(1),
            C::LineSpacing => s.line_spacing = v.clamp(80, 300) as f32 / 100.0,
            C::BgBlur => s.bg_blur = b,
            C::BgBlurAmt => s.bg_blur_amount = v.clamp(1, 40),
            C::FillAlpha => s.fill = (s.fill & 0x00FFFFFF) | (((v.clamp(0, 100) as f64 * 2.55).round() as u32) << 24),
            C::StepOutlineW => s.step_outline_width = v.clamp(0, 40) as f32,
            C::Loupe => s.loupe = b,
            C::LoupeHandle => s.loupe_handle = b,
            C::TextOnly => s.text_only = b,
            C::SnapText => s.snap_text = b,
            C::Sketch => {
                s.sketch = b;
                if s.sketch && s.sketch_seed < 1 {
                    s.sketch_seed = 1;
                }
            }
            C::Vertical => s.vertical = b,
            C::TextOnPath => s.text_on_path = b,
            C::Bezier => s.bezier = b,
            C::Shaded => s.shaded = b,
            C::MagLink => s.magnify_link = b,
            C::Antialias => s.antialias = b,
            C::Bold => s.bold = b,
            C::Italic => s.italic = b,
            C::Underline => s.underline = b,
            C::AlignH(a) => s.align_h = a,
            C::AlignV(a) => s.align_v = a,
            _ => return,
        }
        Self::set_style(cv, s, new_step, acts);
    }

    // ------------------------------------------------------------ stamps
    fn fill_stamp_cats(&mut self) {
        self.stamp_cats = stamps::categories();
        // the last category by its key (the list may gain "Pinned" at the top); older settings kept only the index
        let mut key = reg_string("LastUsedStampCategoryKey");
        let mut last = reg_dword("LastUsedStampCategory", 1) as usize;
        if key.is_empty() {
            key = self
                .stamp_cats
                .iter()
                .filter(|c| !c.pinned && c.key != "labels")
                .nth(last)
                .map(|c| c.key.clone())
                .unwrap_or_default();
        }
        if let Some(i) = self.stamp_cats.iter().position(|c| !key.is_empty() && c.key == key) {
            last = i;
        }
        self.stamp_cat = last.min(self.stamp_cats.len().saturating_sub(1));
    }

    fn thumb(&mut self, ctx: &egui::Context, key: String, make: impl FnOnce() -> Image) -> TextureHandle {
        if let Some(t) = self.thumbs.get(&key) {
            return t.clone();
        }
        if self.thumbs.len() > 400 {
            self.thumbs.clear();
        }
        let t = crate::canvas_ui::texture(ctx, &key, &make(), true);
        self.thumbs.insert(key, t.clone());
        t
    }

    fn style_thumb(&mut self, ctx: &egui::Context, t: Tool, s: &Style, px: i32) -> TextureHandle {
        let key = format!("{}|{px}|{}", t.index(), snapcore::xml::serialize(&crate::styles::style_to_xml(s, "S"), false));
        let s = s.clone();
        self.thumb(ctx, key, move || quick_style_preview(t, &s, px))
    }

    fn stamp_thumb(&mut self, ctx: &egui::Context, id: &str, px: i32) -> TextureHandle {
        let key = format!("stamp|{px}|{id}");
        let id = id.to_owned();
        self.thumb(ctx, key, move || {
            let src = snapcore::render::stamp_image(&id, if id.starts_with("label:") { px * 3 } else { px });
            if src.is_empty() {
                return Image::default();
            }
            let k = (px as f64 / src.wi() as f64).min(px as f64 / src.hi() as f64);
            let sc = snapcore::gdip::scale_image(&src, ((src.wi() as f64 * k) as i32).max(1), ((src.hi() as f64 * k) as i32).max(1), true);
            let mut img = Image::sized(px, px, 0);
            img.blit(&sc, (px - sc.wi()) / 2, (px - sc.hi()) / 2, false);
            img
        })
    }

    /// The picker cells (shapes, ends, dashes) drawn as small quick-style previews of the current style.
    fn variant_thumb(&mut self, ctx: &egui::Context, t: Tool, s: &Style) -> TextureHandle {
        let mut s = s.clone();
        s.shadow = false;
        self.style_thumb(ctx, t, &s, 40)
    }

    // ------------------------------------------------------------ drawing
    pub fn ui(&mut self, ui: &mut egui::Ui, cv: &mut Canvas, qs: &mut QuickStyleStore) -> Vec<PropAction> {
        let mut acts = Vec::new();
        if !self.loaded {
            *self = PropsPanel::new();
        }
        if cv.doc().is_none() {
            ui.label(RichText::new(tr("Abra uma imagem para editar.")).color(Color32::from_gray(110)));
            return acts;
        }
        egui::ScrollArea::vertical().id_salt("props-scroll").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            self.quick_styles(ui, cv, qs, &mut acts);
            self.tool_props(ui, cv, qs, &mut acts);
            self.geometry(ui, cv, &mut acts);
            self.object_buttons(ui, cv, &mut acts);
            if !self.doc_info.is_empty() {
                ui.add_space(14.0);
                ui.label(RichText::new(&self.doc_info).size(11.0).color(Color32::from_gray(110)));
            }
        });
        acts
    }

    fn quick_styles(&mut self, ui: &mut egui::Ui, cv: &mut Canvas, qs: &mut QuickStyleStore, acts: &mut Vec<PropAction>) {
        let qs_tool = self.style_tool(cv);
        let fav = qs_tool == Tool::Favorites;
        let stamps = qs_tool == Tool::Stamp;
        if !(fav || stamps || qs.generated_count(qs_tool) > 0) {
            return;
        }
        let cur = Self::cur(cv);
        let pt = Self::prop_tool(cv);
        ui.horizontal(|ui| {
            ui.label(RichText::new(if fav { tr("Favoritos") } else if stamps { tr("Carimbos") } else { tr("Estilos rápidos") }).strong().size(14.0));
            if !fav && !stamps {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let r = ui.button("+").on_hover_text(tr("Adicionar o estilo atual aos estilos rápidos"));
                    crate::panels::tag(ui, &r, "SnapnabProps.AddStyle");
                    if r.clicked() {
                        qs.add_custom(pt, &cur);
                    }
                });
            }
        });
        ui.separator();
        if !fav && !stamps {
            self.theme_row(ui, qs, acts);
        }
        if stamps {
            let r = ui.add(egui::TextEdit::singleline(&mut self.stamp_search).hint_text(tr("Pesquisar carimbos")).desired_width(f32::INFINITY));
            crate::panels::tag(ui, &r, "SnapnabProps.StampSearch");
            let names: Vec<String> = self.stamp_cats.iter().map(|c| c.name.clone()).collect();
            let mut sel = self.stamp_cat.min(names.len().saturating_sub(1));
            let r = egui::ComboBox::from_id_salt("stamp-cat")
                .width(ui.available_width())
                .selected_text(names.get(sel).cloned().unwrap_or_default())
                .show_index(ui, &mut sel, names.len(), |i| names[i].clone());
            crate::panels::tag(ui, &r, "SnapnabProps.StampCategory");
            if sel != self.stamp_cat {
                self.stamp_cat = sel;
                set_reg_dword("LastUsedStampCategory", sel as u32);
                if let Some(c) = self.stamp_cats.get(sel) {
                    set_reg_string("LastUsedStampCategoryKey", &c.key);
                }
                self.stamp_search.clear();
            }
        }
        // the gallery (with its own scrolling)
        let avail_h = ui.ctx().content_rect().height();
        let max_h = if stamps { (avail_h * 0.5).max(170.0) } else { (avail_h * 0.42).max(130.0) };
        if stamps {
            self.stamp_grid(ui, cv, max_h, acts);
            ui.horizontal(|ui| {
                let w = (ui.available_width() - 8.0) / 2.0;
                let r = ui.add_sized([w, 28.0], egui::Button::new(tr("Organizar carimbos…")));
                crate::panels::tag(ui, &r, "SnapnabProps.OrganizeStamps");
                if r.clicked() {
                    crate::shell_open(&stamps::my_stamps_dir());
                }
                let r = ui.add_sized([w, 28.0], egui::Button::new(tr("Importar…")));
                crate::panels::tag(ui, &r, "SnapnabProps.ImportStamps");
                if r.clicked() {
                    acts.push(PropAction::Modal(Box::new(|owner| {
                        let n = stamps::import(owner);
                        (n > 0).then(|| -> Apply {
                            Box::new(move |p: &mut PropsPanel, _: &mut Canvas, _: &mut QuickStyleStore| {
                                p.fill_stamp_cats();
                                p.stamp_cat = 0;
                                p.thumbs.clear();
                                vec![PropAction::Hint(tr("%d carimbo(s) importado(s) para Meus carimbos.").replace("%d", &n.to_string()))]
                            })
                        })
                    })));
                }
            });
        } else {
            self.style_grid(ui, cv, qs, fav, max_h, acts);
        }
        ui.add_space(6.0);
    }

    fn theme_row(&mut self, ui: &mut egui::Ui, qs: &mut QuickStyleStore, acts: &mut Vec<PropAction>) {
        ui.horizontal(|ui| {
            ui.label(tr("Tema:"));
            let n = qs.themes.len();
            let mut names: Vec<String> = qs.themes.iter().map(|t| tr(&t.name)).collect();
            names.push(tr("Novo tema…"));
            names.push(tr("Importar…"));
            let mut sel = qs.current;
            let r = egui::ComboBox::from_id_salt("theme")
                .width(ui.available_width() - 34.0)
                .selected_text(names.get(qs.current).cloned().unwrap_or_default())
                .show_index(ui, &mut sel, names.len(), |i| names[i].clone());
            crate::panels::tag(ui, &r, "SnapnabProps.Theme");
            if sel != qs.current {
                if sel < n {
                    qs.current = sel;
                    qs.save();
                } else if sel == n {
                    // New theme…: the current one's colors under a new name, without its custom styles
                    let mut t = qs.theme().clone();
                    t.custom.clear();
                    t.name = tr("Meu tema");
                    let name = t.name.clone();
                    acts.push(theme_form(t, name, None));
                } else {
                    acts.push(PropAction::Modal(Box::new(|owner| {
                        let filter = format!("{}|*.snaptheme|{}|*.*|", tr("Tema do Snapnab (*.snaptheme)"), tr("Todos os arquivos"));
                        let path = crate::native::open_file(owner, &tr("Importar tema"), &filter)?;
                        Some(Box::new(move |_: &mut PropsPanel, _: &mut Canvas, qs: &mut QuickStyleStore| {
                            match qs.import_theme(&path) {
                                Ok(()) => vec![],
                                Err(e) => vec![PropAction::Modal(Box::new(move |owner| {
                                    crate::native::message(owner, &e, "Snapnab Editor", windows::Win32::UI::WindowsAndMessaging::MB_ICONWARNING);
                                    None
                                }))],
                            }
                        }) as Apply)
                    })));
                }
            }
            // the gear: edit, duplicate, export, delete
            let cur = qs.current;
            let th = qs.theme().clone();
            ui.menu_button("⚙", |ui| {
                let edit = if th.builtin { tr("Editar uma cópia do tema…") } else { tr("Editar o tema…") };
                if ui.button(edit).clicked() {
                    let name = if th.builtin { th.name.clone() + &tr(" (cópia)") } else { th.name.clone() };
                    acts.push(theme_form(th.clone(), name, if th.builtin { None } else { Some(cur) }));
                    ui.close();
                }
                if ui.button(tr("Duplicar")).clicked() {
                    let mut t = th.clone();
                    t.name += &tr(" (cópia)");
                    qs.add_theme(t);
                    ui.close();
                }
                if ui.button(tr("Exportar…")).clicked() {
                    let name = th.name.clone();
                    acts.push(PropAction::Modal(Box::new(move |owner| {
                        let initial = format!("{}\\{}.snaptheme", crate::files::default_save_dir(), crate::files::safe_name(&name, ""));
                        let filter = format!("{}|*.snaptheme|", tr("Tema do Snapnab (*.snaptheme)"));
                        let (path, _) = crate::native::save_file(owner, &tr("Exportar tema"), &initial, &filter, "snaptheme", 1)?;
                        Some(Box::new(move |_: &mut PropsPanel, _: &mut Canvas, qs: &mut QuickStyleStore| {
                            if qs.export_theme(cur, &path) {
                                vec![]
                            } else {
                                vec![PropAction::Modal(Box::new(|owner| {
                                    crate::native::message(owner, &tr("Não foi possível exportar o tema."), "Snapnab Editor",
                                                           windows::Win32::UI::WindowsAndMessaging::MB_ICONWARNING);
                                    None
                                }))]
                            }
                        }) as Apply)
                    })));
                    ui.close();
                }
                ui.separator();
                if ui.add_enabled(!th.builtin, egui::Button::new(tr("Excluir"))).clicked() {
                    let name = tr(&th.name);
                    acts.push(PropAction::Modal(Box::new(move |owner| {
                        use windows::Win32::UI::WindowsAndMessaging::{IDYES, MB_ICONQUESTION, MB_YESNO};
                        let q = tr("Excluir o tema \"%s\"?").replace("%s", &name);
                        (crate::native::message(owner, &q, "Snapnab Editor", MB_YESNO | MB_ICONQUESTION) == IDYES.0).then(|| -> Apply {
                            Box::new(move |_: &mut PropsPanel, _: &mut Canvas, qs: &mut QuickStyleStore| {
                                qs.delete_theme(cur);
                                vec![]
                            })
                        })
                    })));
                    ui.close();
                }
            });
        });
    }

    fn tile_size(ui: &egui::Ui) -> f32 {
        ((ui.available_width() - 6.0 * 3.0) / 4.0).floor().max(24.0)
    }

    fn style_grid(&mut self, ui: &mut egui::Ui, cv: &mut Canvas, qs: &mut QuickStyleStore, fav: bool, max_h: f32,
                  acts: &mut Vec<PropAction>) {
        let qs_tool = self.style_tool(cv);
        let pt = Self::prop_tool(cv);
        let cur = Self::cur(cv);
        let list: Vec<(Tool, Style)> = if fav {
            qs.favorites.iter().map(|f| (f.tool, f.style.clone())).collect()
        } else {
            qs.styles_for(qs_tool).into_iter().map(|s| (qs_tool, s)).collect()
        };
        if fav && list.is_empty() {
            ui.label(RichText::new(tr("Clique na estrela de um estilo rápido para adicioná-lo aos favoritos.")).size(11.0).color(Color32::from_gray(110)));
            return;
        }
        let gen_n = if fav { 0 } else { qs.generated_count(qs_tool) };
        let ts = Self::tile_size(ui);
        let px = (ts * ui.ctx().pixels_per_point()) as i32 - 8;
        egui::ScrollArea::vertical().id_salt("qs-grid").max_height(max_h).show(ui, |ui| {
            egui::Grid::new("qs-tiles").spacing([6.0, 6.0]).show(ui, |ui| {
                for (i, (tool, st)) in list.iter().enumerate() {
                    let tex = self.style_thumb(ui.ctx(), *tool, st, px.max(16));
                    let sel = (!fav || *tool == pt) && same_style(st, &cur);
                    let is_fav = fav || qs.is_favorite(*tool, st);
                    let (r, star) = Self::tile(ui, &tex, ts, sel, Some(is_fav));
                    crate::panels::tag(ui, &r, &format!("SnapnabProps.Style.{i}"));
                    if r.clicked() {
                        if star {
                            qs.toggle_favorite(*tool, st);
                        } else if fav {
                            acts.push(PropAction::PickTool(*tool));
                            cv.styles[tool.index()] = st.clone();
                            acts.push(PropAction::StylesChanged);
                        } else {
                            Self::set_style(cv, st.clone(), true, acts);
                        }
                    }
                    r.context_menu(|ui| {
                        if ui.button(tr("Aplicar este estilo")).clicked() {
                            if fav {
                                acts.push(PropAction::PickTool(*tool));
                                cv.styles[tool.index()] = st.clone();
                                acts.push(PropAction::StylesChanged);
                            } else {
                                Self::set_style(cv, st.clone(), true, acts);
                            }
                            ui.close();
                        }
                        let t = if qs.is_favorite(*tool, st) { tr("Remover dos favoritos") } else { tr("Adicionar aos favoritos") };
                        if ui.button(t).clicked() {
                            qs.toggle_favorite(*tool, st);
                            ui.close();
                        }
                        if !fav && i >= gen_n && ui.button(tr("Excluir este estilo rápido")).clicked() {
                            qs.remove_custom(qs_tool, i - gen_n);
                            ui.close();
                        }
                    });
                    if i % 4 == 3 {
                        ui.end_row();
                    }
                }
                if !fav {
                    // "add the current style": a dashed tile with "+"
                    let tex = self.style_thumb(ui.ctx(), pt, &cur, px.max(16));
                    let (r, _) = Self::tile(ui, &tex, ts, false, None);
                    let p = ui.painter();
                    p.rect_stroke(r.rect.shrink(1.0), 4.0, egui::Stroke::new(1.5, ui.visuals().selection.bg_fill), egui::StrokeKind::Inside);
                    p.text(r.rect.right_bottom() - egui::vec2(9.0, 9.0), egui::Align2::CENTER_CENTER, "+", egui::FontId::proportional(16.0),
                           ui.visuals().selection.bg_fill);
                    crate::panels::tag(ui, &r, "SnapnabProps.AddTile");
                    if r.clicked() {
                        qs.add_custom(pt, &cur);
                    }
                }
            });
        });
    }

    /// One gallery tile; returns the response and whether the click was on the favorite star.
    fn tile(ui: &mut egui::Ui, tex: &TextureHandle, size: f32, sel: bool, star: Option<bool>) -> (egui::Response, bool) {
        let (rect, r) = ui.allocate_exact_size(Vec2::splat(size), Sense::click_and_drag());
        let p = ui.painter();
        let accent = ui.visuals().selection.bg_fill;
        if sel {
            p.rect_filled(rect, 6.0, accent.gamma_multiply(0.25));
        } else if r.hovered() {
            p.rect_filled(rect, 6.0, Color32::from_black_alpha(14));
        }
        p.image(tex.id(), rect.shrink(4.0), egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        let mut on_star = false;
        if let Some(is_fav) = star {
            let sr = egui::Rect::from_min_size(egui::pos2(rect.right() - 16.0, rect.top() + 2.0), Vec2::splat(14.0));
            on_star = r.interact_pointer_pos().or(r.hover_pos()).is_some_and(|q| sr.expand(2.0).contains(q));
            if is_fav || r.hovered() {
                let pts: Vec<egui::Pos2> = (0..10)
                    .map(|k| {
                        let a = -std::f32::consts::FRAC_PI_2 + k as f32 * std::f32::consts::PI / 5.0;
                        let rr = if k % 2 == 1 { 3.1 } else { 7.0 };
                        sr.center() + egui::vec2(a.cos() * rr, a.sin() * rr)
                    })
                    .collect();
                let fill = if is_fav { Color32::from_rgb(255, 196, 0) } else { Color32::TRANSPARENT };
                let line = if on_star { Color32::from_rgb(230, 150, 0) } else if is_fav { Color32::from_rgb(200, 140, 0) } else { Color32::from_gray(140) };
                p.add(egui::Shape::convex_polygon(pts.clone(), Color32::TRANSPARENT, egui::Stroke::NONE));
                // a star is concave: fill it as triangles from the center
                if is_fav {
                    for k in 0..10 {
                        p.add(egui::Shape::convex_polygon(vec![sr.center(), pts[k], pts[(k + 1) % 10]], fill, egui::Stroke::NONE));
                    }
                }
                p.add(egui::Shape::closed_line(pts, egui::Stroke::new(1.2, line)));
            }
        }
        (r, on_star)
    }

    fn stamp_grid(&mut self, ui: &mut egui::Ui, cv: &mut Canvas, max_h: f32, acts: &mut Vec<PropAction>) {
        let items: Vec<stamps::StampItem> = if self.stamp_search.trim().is_empty() {
            self.stamp_cats.get(self.stamp_cat).map(|c| c.items.clone()).unwrap_or_default()
        } else {
            stamps::search(&self.stamp_cats, &self.stamp_search)
        };
        if items.is_empty() {
            let t = if self.stamp_search.trim().is_empty() {
                tr("Nenhum carimbo nesta categoria. Use “Importar…” para acrescentar imagens.")
            } else {
                tr("Nenhum carimbo encontrado.")
            };
            ui.label(RichText::new(t).size(11.0).color(Color32::from_gray(110)));
            return;
        }
        let ts = Self::tile_size(ui);
        let px = ((ts * ui.ctx().pixels_per_point()) as i32 - 8).max(16);
        let can_drag = self.stamp_search.trim().is_empty();
        let mut rects = Vec::new();
        let mut released_at: Option<usize> = None;
        egui::ScrollArea::vertical().id_salt("stamp-grid").max_height(max_h).show(ui, |ui| {
            egui::Grid::new("stamp-tiles").spacing([6.0, 6.0]).show(ui, |ui| {
                for (i, it) in items.iter().enumerate() {
                    let tex = self.stamp_thumb(ui.ctx(), &it.id, px);
                    let (r, _) = Self::tile(ui, &tex, ts, it.id == cv.stamp_id, None);
                    crate::panels::tag(ui, &r, &format!("SnapnabProps.Stamp.{i}"));
                    let r = r.on_hover_text(&it.name);
                    rects.push(r.rect);
                    if stamps::is_pinned(&it.id) {
                        snapui::icons::draw(ui.painter(), snapui::icons::Icon::Pin,
                                            egui::Rect::from_min_size(egui::pos2(r.rect.right() - 15.0, r.rect.top() + 1.0), Vec2::splat(14.0)),
                                            ui.visuals().selection.bg_fill);
                    }
                    if r.clicked() {
                        cv.set_selected_stamp(&it.id);
                    }
                    if can_drag && r.drag_started() {
                        self.drag_stamp = Some(i);
                    }
                    if r.drag_stopped() {
                        released_at = Some(i);
                    }
                    let pinned = stamps::is_pinned(&it.id);
                    r.context_menu(|ui| {
                        if ui.button(if pinned { tr("Desafixar") } else { tr("Fixar no topo") }).clicked() {
                            stamps::set_pinned(&it.id, !pinned);
                            self.refresh_stamps = true;
                            if let Some(c) = self.stamp_cats.get(self.stamp_cat) {
                                set_reg_string("LastUsedStampCategoryKey", &c.key);
                            }
                            acts.push(PropAction::Hint(if pinned {
                                tr("Carimbo desafixado.")
                            } else {
                                tr("Carimbo fixado: aparece primeiro na categoria e em “Fixados”.")
                            }));
                            ui.close();
                        }
                        if ui.button(tr("Usar este carimbo")).clicked() {
                            cv.set_selected_stamp(&it.id);
                            ui.close();
                        }
                    });
                    if i % 4 == 3 {
                        ui.end_row();
                    }
                }
            });
        });
        // dragging a tile reorders the category (the drop mark is the tile under the pointer)
        if let Some(from) = self.drag_stamp {
            let pos = ui.ctx().pointer_latest_pos();
            if let Some(to) = pos.and_then(|q| rects.iter().position(|r| r.contains(q))) {
                if to != from {
                    let r = rects[to];
                    let x = if to > from { r.right() - 1.0 } else { r.left() + 1.0 };
                    ui.painter().line_segment([egui::pos2(x, r.top() + 2.0), egui::pos2(x, r.bottom() - 2.0)],
                                              egui::Stroke::new(2.0, ui.visuals().selection.bg_fill));
                }
                if released_at.is_some() {
                    if to != from
                        && let Some(cat) = self.stamp_cats.get(self.stamp_cat)
                    {
                        stamps::move_stamp(cat, from, to);
                        set_reg_string("LastUsedStampCategoryKey", &cat.key);
                        self.refresh_stamps = true;
                    }
                    self.drag_stamp = None;
                }
            }
            if released_at.is_some() || !ui.input(|i| i.pointer.any_down()) {
                self.drag_stamp = None;
            }
        }
        if std::mem::take(&mut self.refresh_stamps) {
            self.fill_stamp_cats();
        }
    }

    // ------------------------------------------------------------ tool properties
    fn tool_props(&mut self, ui: &mut egui::Ui, cv: &mut Canvas, qs: &QuickStyleStore, acts: &mut Vec<PropAction>) {
        let pt = Self::prop_tool(cv);
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr("Propriedades da ferramenta")).strong().size(14.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let r = ui.small_button("?").on_hover_text(tr("Ajuda"));
                crate::panels::tag(ui, &r, "SnapnabProps.Help");
                if r.clicked() {
                    let h = help_for(pt);
                    let title = pt.name();
                    if !h.is_empty() {
                        acts.push(PropAction::Modal(Box::new(move |owner| {
                            crate::native::message(owner, &h, &title, crate::native::info_flags());
                            None
                        })));
                    }
                }
            });
        });
        ui.separator();
        let st = Self::cur(cv);
        let nsel = cv.selection_count();
        let group = Self::sel_obj(cv).filter(|o| o.kind == ObjType::Group).map(|o| o.children.len());
        let hint = |ui: &mut egui::Ui, t: String| {
            ui.label(RichText::new(t).size(11.0).color(Color32::from_gray(110)));
        };
        if nsel > 1 {
            hint(ui, tr("%d objetos selecionados. As mudanças de estilo valem para os do mesmo tipo do último clicado.").replace("%d", &nsel.to_string()));
        }
        if let Some(n) = group {
            hint(ui, tr("Grupo com %zu objetos: arraste para mover, use as alças para redimensionar.").replace("%zu", &n.to_string()));
            return;
        }
        let theme = qs.theme().colors;
        let start_sz = st.arrow_start != CAP_FLAT && st.arrow_start != CAP_ROUND;
        let end_sz = st.arrow_end != CAP_FLAT && st.arrow_end != CAP_ROUND;
        match pt {
            Tool::Arrow | Tool::Line => {
                ui.horizontal(|ui| {
                    self.swatch(ui, cv, &theme, ColorTarget::Color, &tr("Cor"), acts);
                    self.shadow(ui, cv, &theme, acts);
                });
                self.cap_row(ui, cv, true, true, acts);
                self.slider(ui, cv, C::Width, &tr("Largura"), 1, 100, true, acts);
                self.slider(ui, cv, C::Opacity, &tr("Opacidade"), 0, 100, true, acts);
                self.slider(ui, cv, C::StartSize, &tr("Tamanho inicial"), 1, 10, start_sz, acts);
                self.slider(ui, cv, C::EndSize, &tr("Tamanho final"), 1, 10, end_sz, acts);
                self.check(ui, cv, C::Bezier, &tr("Curva de Bézier"), true, acts);
                self.sketch(ui, cv, &tr("Desenhado à mão"), acts);
                self.check(ui, cv, C::TextOnPath, &tr("Texto ao longo da linha"), true, acts);
                if pt == Tool::Arrow || st.text_on_path {
                    self.font_combo(ui, cv, &tr("Fonte do texto"), acts);
                    self.slider(ui, cv, C::FontSize, &tr("Tamanho do texto"), 6, 200, true, acts);
                    hint(ui, if st.text_on_path {
                        tr("Depois de desenhar, digite o texto: ele acompanha a linha ou a curva; duplo clique edita.")
                    } else {
                        tr("Depois de desenhar a seta, digite para escrever um texto na ponta inicial; duplo clique edita.")
                    });
                }
            }
            Tool::Pen => {
                ui.horizontal(|ui| {
                    self.swatch(ui, cv, &theme, ColorTarget::Color, &tr("Cor"), acts);
                    self.shadow(ui, cv, &theme, acts);
                });
                self.cap_row(ui, cv, false, true, acts);
                self.slider(ui, cv, C::Width, &tr("Largura"), 1, 100, true, acts);
                self.slider(ui, cv, C::Opacity, &tr("Opacidade"), 0, 100, true, acts);
            }
            Tool::Highlighter => {
                ui.horizontal(|ui| {
                    self.swatch(ui, cv, &theme, ColorTarget::Color, &tr("Cor"), acts);
                    self.shape_picker(ui, cv, true, acts);
                });
                self.slider(ui, cv, C::Opacity, &tr("Opacidade"), 5, 100, true, acts);
                self.check(ui, cv, C::SnapText, &tr("Encaixar nas linhas de texto"), true, acts);
                self.check(ui, cv, C::TextOnly, &tr("Somente o texto (palavras na área)"), true, acts);
            }
            Tool::Text => {
                ui.horizontal(|ui| {
                    self.swatch(ui, cv, &theme, ColorTarget::TextColor, &tr("Preenchimento"), acts);
                    self.swatch(ui, cv, &theme, ColorTarget::Outline, &tr("Contorno"), acts);
                    self.shadow(ui, cv, &theme, acts);
                });
                self.font_combo(ui, cv, &tr("Fonte"), acts);
                self.slider(ui, cv, C::FontSize, &tr("Tamanho da fonte"), 6, 200, true, acts);
                self.slider(ui, cv, C::OutlineW, &tr("Espessura"), 0, 20, true, acts);
                self.check(ui, cv, C::Vertical, &tr("Texto vertical"), true, acts);
                self.check(ui, cv, C::Sketch, &tr("Letra à mão"), true, acts);
                self.advanced_text(ui, cv, false, acts);
            }
            Tool::Callout => {
                let leader = st.callout_shape == 4 || st.callout_shape == 15;
                ui.horizontal(|ui| {
                    self.swatch(ui, cv, &theme, ColorTarget::Fill, &tr("Preenchimento"), acts);
                    self.swatch(ui, cv, &theme, ColorTarget::Outline, &tr("Contorno"), acts);
                    self.shape_picker(ui, cv, false, acts);
                    self.shadow(ui, cv, &theme, acts);
                });
                ui.horizontal(|ui| {
                    self.font_combo(ui, cv, &tr("Fonte"), acts);
                    self.swatch(ui, cv, &theme, ColorTarget::TextColor, "", acts);
                });
                self.slider(ui, cv, C::FontSize, &tr("Tamanho da fonte"), 6, 200, true, acts);
                self.slider(ui, cv, C::OutlineW, &tr("Espessura"), 0, 20, true, acts);
                ui.add_enabled_ui(leader, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(tr("Estilo da seta"));
                        self.dash_picker(ui, cv, acts);
                        self.cap_picker(ui, cv, false, acts);
                    });
                    ui.horizontal(|ui| {
                        ui.label(tr("Cor da seta"));
                        self.swatch(ui, cv, &theme, ColorTarget::TailColor, "", acts);
                    });
                });
                self.slider(ui, cv, C::TailSize, &tr("Tamanho da seta"), 1, 30, leader, acts);
                self.check(ui, cv, C::BgBlur, &tr("Desfocar o fundo (vidro fosco)"), true, acts);
                self.slider(ui, cv, C::BgBlurAmt, &tr("Desfoque"), 1, 40, st.bg_blur, acts);
                self.slider(ui, cv, C::FillAlpha, &tr("Opacidade do preenchimento"), 0, 100, true, acts);
                self.check(ui, cv, C::Vertical, &tr("Texto vertical"), true, acts);
                self.advanced_text(ui, cv, true, acts);
            }
            Tool::Shape => {
                ui.horizontal(|ui| {
                    self.swatch(ui, cv, &theme, ColorTarget::Fill, &tr("Preenchimento"), acts);
                    self.swatch(ui, cv, &theme, ColorTarget::Outline, &tr("Contorno"), acts);
                    self.shape_picker(ui, cv, false, acts);
                    self.shadow(ui, cv, &theme, acts);
                });
                self.slider(ui, cv, C::Width, &tr("Espessura"), 0, 50, true, acts);
                self.slider(ui, cv, C::Opacity, &tr("Opacidade"), 0, 100, true, acts);
                self.sketch(ui, cv, &tr("Desenhado à mão"), acts);
            }
            Tool::Stamp => {
                self.shadow(ui, cv, &theme, acts);
                self.slider(ui, cv, C::Opacity, &tr("Opacidade"), 0, 100, true, acts);
            }
            Tool::Fill => {
                ui.horizontal(|ui| {
                    let r = ui.add_sized([52.0, 50.0], egui::Button::new(tr("Conta-gotas")).wrap());
                    crate::panels::tag(ui, &r, "SnapnabProps.Eyedrop");
                    if r.clicked() {
                        cv.pick_color_once = true;
                        acts.push(PropAction::Hint(tr("Conta-gotas: clique na imagem para pegar a cor de preenchimento.")));
                    }
                    self.swatch(ui, cv, &theme, ColorTarget::Color, &tr("Preenchimento"), acts);
                });
                self.slider(ui, cv, C::Opacity, &tr("Opacidade"), 0, 100, true, acts);
                self.slider(ui, cv, C::Tolerance, &tr("Tolerância"), 0, 100, true, acts);
                self.check(ui, cv, C::Global, &tr("Mesma cor em toda a imagem"), true, acts);
            }
            Tool::Step => {
                ui.horizontal(|ui| {
                    self.swatch(ui, cv, &theme, ColorTarget::Color, &tr("Preenchimento"), acts);
                    self.swatch(ui, cv, &theme, ColorTarget::TextColor, &tr("Cor do texto"), acts);
                    self.shape_picker(ui, cv, false, acts);
                    self.shadow(ui, cv, &theme, acts);
                });
                self.slider(ui, cv, C::Opacity, &tr("Opacidade"), 0, 100, true, acts);
                ui.horizontal(|ui| {
                    ui.label(tr("Tipo"));
                    let items = ["1, 2, 3…".to_owned(), "A, B, C…".to_owned(), "a, b, c…".to_owned(), tr("I, II, III…"), tr("i, ii, iii…")];
                    let mut sel = st.step_type.clamp(0, 4) as usize;
                    let r = egui::ComboBox::from_id_salt("step-type")
                        .selected_text(items[sel].clone())
                        .show_index(ui, &mut sel, items.len(), |i| items[i].clone());
                    crate::panels::tag(ui, &r, "SnapnabProps.StepType");
                    if sel as i32 != st.step_type {
                        let mut s = Self::cur(cv);
                        s.step_type = sel as i32;
                        Self::set_style(cv, s, true, acts);
                    }
                    let r = ui.button("⟲").on_hover_text(tr("Reiniciar a sequência"));
                    crate::panels::tag(ui, &r, "SnapnabProps.StepReset");
                    if r.clicked()
                        && let Some(d) = cv.doc_mut()
                    {
                        d.st.next_step = 1;
                        acts.push(PropAction::Hint(tr("A sequência de passos recomeça em 1.")));
                    }
                });
                // outline for contrast on light and dark backgrounds (accessibility)
                ui.horizontal(|ui| {
                    ui.label(tr("Contorno"));
                    self.swatch(ui, cv, &theme, ColorTarget::StepOutline, "", acts);
                });
                self.slider(ui, cv, C::StepOutlineW, &tr("Espessura do contorno"), 0, 12, true, acts);
            }
            Tool::Eraser => {
                self.slider(ui, cv, C::Width, &tr("Largura"), 1, 200, true, acts);
                self.slider(ui, cv, C::Opacity, &tr("Opacidade"), 1, 100, true, acts);
                self.check(ui, cv, C::Antialias, &tr("Suavizar as bordas"), true, acts);
            }
            Tool::Magnify => {
                ui.horizontal(|ui| {
                    self.swatch(ui, cv, &theme, ColorTarget::Outline, &tr("Contorno"), acts);
                    self.shape_picker(ui, cv, false, acts);
                    self.shadow(ui, cv, &theme, acts);
                });
                self.slider(ui, cv, C::MagScale, &tr("Ampliação (%)"), 110, 800, true, acts);
                self.slider(ui, cv, C::Width, &tr("Espessura"), 0, 20, true, acts);
                self.check(ui, cv, C::MagLink, &tr("Lente separada da área ampliada"), true, acts);
                self.check(ui, cv, C::Loupe, &tr("Lupa realista (aro e reflexo)"), true, acts);
                self.check(ui, cv, C::LoupeHandle, &tr("Com cabo"), st.loupe, acts);
            }
            Tool::Blur => {
                self.shape_picker(ui, cv, true, acts);
                ui.horizontal(|ui| {
                    ui.label(tr("Tipo"));
                    let items = [tr("Desfoque"), tr("Pixelado")];
                    let mut sel = st.hide_kind.clamp(0, 1) as usize;
                    let r = egui::ComboBox::from_id_salt("hide-kind")
                        .selected_text(items[sel].clone())
                        .show_index(ui, &mut sel, 2, |i| items[i].clone());
                    crate::panels::tag(ui, &r, "SnapnabProps.HideKind");
                    if sel as i32 != st.hide_kind {
                        let mut s = Self::cur(cv);
                        s.hide_kind = sel as i32;
                        Self::set_style(cv, s, true, acts);
                    }
                });
                self.slider(ui, cv, C::Amount, &tr("Intensidade"), 1, 30, true, acts);
                self.check(ui, cv, C::TextOnly, &tr("Somente o texto (palavras na área)"), true, acts);
            }
            Tool::Spotlight => {
                self.shape_picker(ui, cv, true, acts);
                self.slider(ui, cv, C::Amount, &tr("Intensidade"), 1, 14, true, acts);
                self.check(ui, cv, C::TextOnly, &tr("Somente o texto (palavras na área)"), true, acts);
            }
            Tool::Select => {
                if nsel <= 1 {
                    let mut on = cv.smart_move;
                    let r = ui.add(egui::Checkbox::new(&mut on, tr("Smart Move")));
                    crate::panels::tag(ui, &r, "SnapnabProps.SmartMove");
                    if r.changed() {
                        self.set_value(cv, C::SmartMove, on as i32, true, acts);
                    }
                    hint(ui, if cv.smart_move {
                        tr("Arraste uma área da imagem e depois arraste-a para outro lugar; o fundo é preenchido.")
                    } else {
                        tr("Clique num objeto para selecionar. Ctrl+clique ou arraste um retângulo para selecionar vários.")
                    });
                }
            }
            Tool::Crop => {
                hint(ui, tr("Arraste a área a manter; Enter ou duplo clique aplica."));
                self.button(ui, 324, &tr("Aplicar recorte"), acts);
                self.button(ui, 325, &tr("Corte automático"), acts);
            }
            Tool::Selection | Tool::MagicWand => {
                self.canvas_combo(ui, cv, "sel-mode", &tr("Modo"),
                                  &[tr("Retângulo"), tr("Elipse"), tr("Laço (mão livre)"), tr("Varinha mágica")], 0);
                self.slider(ui, cv, C::Tolerance, &tr("Tolerância"), 0, 100, true, acts);
                self.check(ui, cv, C::Global, &tr("Seleção global (mesma cor)"), true, acts);
                self.canvas_combo(ui, cv, "sel-fill", &tr("Ao mover, preencher"),
                                  &[tr("Transparência"), tr("Cor do canvas (branco)"), tr("Preenchimento inteligente")], 1);
                self.button(ui, 324, &tr("Cortar para a seleção"), acts);
                self.button(ui, 325, &tr("Copiar seleção"), acts);
                self.button(ui, 326, &tr("Excluir seleção"), acts);
                let r = ui.add_sized([ui.available_width(), 28.0], egui::Button::new(tr("Levantar a seleção (peça solta)")));
                crate::panels::tag(ui, &r, "SnapnabProps.Lift");
                if r.clicked() {
                    if !cv.has_raster_selection() {
                        acts.push(PropAction::Hint(tr("Selecione uma área com a ferramenta Seleção para levantá-la.")));
                    } else if cv.lift_selection() {
                        acts.push(PropAction::Hint(tr("Peça solta: ajuste o resto da imagem (cortar, recortar faixa…), posicione a peça e tecle Enter para soltá-la.")));
                    }
                }
                if cv.has_floating() {
                    self.drop_button(ui, cv, acts);
                }
            }
            Tool::Cutout => {
                self.canvas_combo(ui, cv, "cut-dir", &tr("Direção"), &[tr("Horizontal (remove linhas)"), tr("Vertical (remove colunas)")], 2);
                hint(ui, tr("Arraste sobre a faixa a remover; as partes da imagem são unidas."));
            }
            Tool::Eyedropper => {
                self.swatch(ui, cv, &theme, ColorTarget::Color, &tr("Cor"), acts);
                hint(ui, tr("Clique na imagem para pegar a cor; ela vai para as ferramentas de desenho."));
            }
            Tool::Favorites => {}
        }
    }

    fn button(&self, ui: &mut egui::Ui, id: u32, text: &str, acts: &mut Vec<PropAction>) {
        let r = ui.add_sized([ui.available_width(), 28.0], egui::Button::new(text));
        crate::panels::tag(ui, &r, &format!("SnapnabProps.{id}"));
        if r.clicked() {
            acts.push(PropAction::Command(id));
        }
    }

    fn drop_button(&self, ui: &mut egui::Ui, cv: &mut Canvas, acts: &mut Vec<PropAction>) {
        let r = ui.add_sized([ui.available_width(), 28.0], egui::Button::new(tr("Soltar a peça na imagem")));
        crate::panels::tag(ui, &r, "SnapnabProps.Drop");
        if r.clicked() && cv.drop_floating() {
            acts.push(PropAction::Hint(tr("Peça solta na imagem.")));
            acts.push(PropAction::DocChanged);
        }
    }

    fn canvas_combo(&self, ui: &mut egui::Ui, cv: &mut Canvas, id: &str, label: &str, items: &[String], which: u8) {
        ui.horizontal(|ui| {
            ui.add_sized([92.0, 20.0], egui::Label::new(label).truncate());
            let v = match which {
                0 => &mut cv.sel_mode,
                1 => &mut cv.sel_fill,
                _ => &mut cv.cut_dir,
            };
            let mut sel = (*v).clamp(0, items.len() as i32 - 1) as usize;
            let r = egui::ComboBox::from_id_salt(id)
                .width(ui.available_width())
                .selected_text(items[sel].clone())
                .show_index(ui, &mut sel, items.len(), |i| items[i].clone());
            crate::panels::tag(ui, &r, &format!("SnapnabProps.{id}"));
            if sel as i32 != *v {
                *v = sel as i32;
                if which == 0 {
                    cv.clear_raster_selection();
                }
            }
        });
    }

    fn slider(&mut self, ui: &mut egui::Ui, cv: &mut Canvas, id: C, label: &str, min: i32, max: i32, enabled: bool,
              acts: &mut Vec<PropAction>) {
        let mut v = self.value(cv, id);
        ui.add_enabled_ui(enabled, |ui| {
            ui.horizontal(|ui| {
                ui.add_sized([94.0, 20.0], egui::Label::new(label).truncate());
                ui.spacing_mut().slider_width = (ui.available_width() - 60.0).max(40.0);
                let r = ui.add(egui::Slider::new(&mut v, min..=max));
                crate::panels::tag(ui, &r, &format!("SnapnabProps.{id:?}"));
                if r.changed() {
                    // one undo step per drag (the first change opens it)
                    let new_step = r.drag_started() || !r.dragged();
                    self.set_value(cv, id, v.clamp(min, max), new_step, acts);
                }
            });
        });
    }

    fn check(&mut self, ui: &mut egui::Ui, cv: &mut Canvas, id: C, label: &str, enabled: bool, acts: &mut Vec<PropAction>) {
        let mut on = self.value(cv, id) != 0;
        let r = ui.add_enabled(enabled, egui::Checkbox::new(&mut on, label));
        crate::panels::tag(ui, &r, &format!("SnapnabProps.{id:?}"));
        if r.changed() {
            self.set_value(cv, id, on as i32, true, acts);
        }
    }

    /// Hand-drawn look with a "vary the stroke" button beside it (C35).
    fn sketch(&mut self, ui: &mut egui::Ui, cv: &mut Canvas, label: &str, acts: &mut Vec<PropAction>) {
        ui.horizontal(|ui| {
            self.check(ui, cv, C::Sketch, label, true, acts);
            let on = Self::cur(cv).sketch;
            let r = ui.add_enabled(on, egui::Button::new("✨")).on_hover_text(tr("Variar o traço"));
            crate::panels::tag(ui, &r, "SnapnabProps.SketchSeed");
            if r.clicked() {
                let mut s = Self::cur(cv);
                s.sketch_seed = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(1) % 9973) as i32 + 1;
                Self::set_style(cv, s, true, acts);
            }
        });
    }

    fn font_combo(&mut self, ui: &mut egui::Ui, cv: &mut Canvas, label: &str, acts: &mut Vec<PropAction>) {
        ui.horizontal(|ui| {
            ui.add_sized([92.0, 20.0], egui::Label::new(label).truncate());
            let s = Self::cur(cv);
            let list = fonts();
            let r = egui::ComboBox::from_id_salt("font").width((ui.available_width() - 4.0).max(80.0)).selected_text(&s.font).show_ui(ui, |ui| {
                let mut chosen = None;
                for f in list {
                    if ui.selectable_label(f.eq_ignore_ascii_case(&s.font), f).clicked() {
                        chosen = Some(f.clone());
                    }
                }
                chosen
            });
            crate::panels::tag(ui, &r.response, "SnapnabProps.Font");
            if let Some(Some(f)) = r.inner {
                let mut s = s;
                s.font = f;
                Self::set_style(cv, s, true, acts);
            }
        });
    }

    fn advanced_text(&mut self, ui: &mut egui::Ui, cv: &mut Canvas, callout: bool, acts: &mut Vec<PropAction>) {
        let r = ui.add(egui::Button::new(RichText::new(format!("{} {}", if self.advanced { "▾" } else { "▸" }, tr("Avançado"))).strong()).frame(false));
        crate::panels::tag(ui, &r, "SnapnabProps.Advanced");
        if r.clicked() {
            self.advanced = !self.advanced;
            set_reg_dword("PropsAdvanced", self.advanced as u32);
        }
        if !self.advanced {
            return;
        }
        ui.horizontal(|ui| {
            ui.add_sized([92.0, 20.0], egui::Label::new(tr("Estilo")));
            for (id, t) in [(C::Bold, RichText::new("B").strong()), (C::Italic, RichText::new("I").italics()), (C::Underline, RichText::new("U").underline())] {
                let on = self.value(cv, id) != 0;
                let r = ui.add(egui::Button::new(t).selected(on).min_size(Vec2::new(26.0, 26.0)));
                crate::panels::tag(ui, &r, &format!("SnapnabProps.{id:?}"));
                if r.clicked() {
                    self.set_value(cv, id, !on as i32, true, acts);
                }
            }
        });
        ui.horizontal(|ui| {
            ui.add_sized([92.0, 20.0], egui::Label::new(tr("Alinhamento")));
            for (a, t) in [(0, "⇤"), (1, "↔"), (2, "⇥")] {
                let on = self.value(cv, C::AlignH(a)) != 0;
                let r = ui.add(egui::Button::new(t).selected(on).min_size(Vec2::new(24.0, 26.0)));
                crate::panels::tag(ui, &r, &format!("SnapnabProps.AlignH.{a}"));
                if r.clicked() {
                    self.set_value(cv, C::AlignH(a), 1, true, acts);
                }
            }
            ui.add_space(6.0);
            for (a, t) in [(0, "⤒"), (1, "↕"), (2, "⤓")] {
                let on = self.value(cv, C::AlignV(a)) != 0;
                let r = ui.add(egui::Button::new(t).selected(on).min_size(Vec2::new(24.0, 26.0)));
                crate::panels::tag(ui, &r, &format!("SnapnabProps.AlignV.{a}"));
                if r.clicked() {
                    self.set_value(cv, C::AlignV(a), 1, true, acts);
                }
            }
        });
        self.slider(ui, cv, C::Opacity, &tr("Opacidade"), 0, 100, true, acts);
        self.slider(ui, cv, C::Padding, &tr("Margem interna"), 0, 60, true, acts);
        self.slider(ui, cv, C::LineSpacing, &tr("Espaçamento entre linhas (%)"), 80, 300, true, acts);
        if callout {
            self.check(ui, cv, C::Shaded, &tr("Sombreado (degradê)"), true, acts);
        }
    }

    // ------------------------------------------------------------ colors
    fn color_of(s: &Style, t: ColorTarget) -> u32 {
        match t {
            ColorTarget::Fill => s.fill,
            ColorTarget::TextColor => s.text_color,
            ColorTarget::TailColor => s.tail_color,
            ColorTarget::StepOutline => s.step_outline,
            ColorTarget::Color | ColorTarget::Outline => s.stroke,
        }
    }

    /// Writes a chosen color into the style (an outline color turns the outline on; "no color" turns it off).
    fn put_color(s: &mut Style, pt: Tool, t: ColorTarget, v: u32) {
        let outline = (t == ColorTarget::Outline && matches!(pt, Tool::Text | Tool::Callout | Tool::Shape)) || t == ColorTarget::StepOutline;
        match t {
            ColorTarget::Fill => s.fill = v,
            ColorTarget::TextColor => s.text_color = v,
            ColorTarget::TailColor => s.tail_color = v,
            ColorTarget::StepOutline => s.step_outline = v,
            ColorTarget::Color | ColorTarget::Outline => s.stroke = v,
        }
        if outline {
            let ow = if t == ColorTarget::StepOutline { &mut s.step_outline_width } else { &mut s.width };
            if v >> 24 != 0 && *ow <= 0.0 {
                *ow = if t != ColorTarget::StepOutline && pt == Tool::Shape { 4.0 } else { 3.0 };
            }
            if v >> 24 == 0 {
                *ow = 0.0;
            }
        }
    }

    fn swatch(&mut self, ui: &mut egui::Ui, cv: &mut Canvas, theme: &[u32; 4], t: ColorTarget, label: &str, acts: &mut Vec<PropAction>) {
        let pt = Self::prop_tool(cv);
        let s = Self::cur(cv);
        let mut col = Self::color_of(&s, t);
        let outline = (t == ColorTarget::Outline && matches!(pt, Tool::Text | Tool::Callout | Tool::Shape)) || t == ColorTarget::StepOutline;
        let ow = if t == ColorTarget::StepOutline { s.step_outline_width } else { s.width };
        if outline && ow <= 0.0 {
            col &= 0x00FFFFFF;
        }
        let gradient = (t == ColorTarget::Fill && pt == Tool::Shape) || (t == ColorTarget::Color && pt == Tool::Fill);
        let grad_on = gradient && s.grad.active();
        let allow_none = t == ColorTarget::Fill || outline;
        let r = ui
            .vertical(|ui| {
                if !label.is_empty() {
                    ui.label(RichText::new(label).size(11.0));
                }
                let size = if label.is_empty() { Vec2::new(40.0, 24.0) } else { Vec2::new(52.0, 30.0) };
                let (rect, r) = ui.allocate_exact_size(size, Sense::click());
                let p = ui.painter();
                p.rect_filled(rect, 5.0, if r.hovered() { Color32::from_gray(246) } else { Color32::WHITE });
                p.rect_stroke(rect, 5.0, egui::Stroke::new(1.0, Color32::from_gray(200)), egui::StrokeKind::Inside);
                let sw = rect.shrink2(Vec2::new(8.0, 6.0));
                if grad_on {
                    let n = 12;
                    for k in 0..n {
                        let f = k as f32 / (n - 1) as f32;
                        let c = grad_color(&s.grad, f);
                        let x0 = sw.left() + sw.width() * k as f32 / n as f32;
                        p.rect_filled(egui::Rect::from_min_max(egui::pos2(x0, sw.top()), egui::pos2(x0 + sw.width() / n as f32 + 0.5, sw.bottom())), 0.0, argb32(c));
                    }
                } else if col >> 24 == 0 {
                    p.rect_filled(sw, 3.0, Color32::WHITE);
                    p.line_segment([sw.left_bottom(), sw.right_top()], egui::Stroke::new(2.0, Color32::from_rgb(220, 40, 40)));
                } else {
                    p.rect_filled(sw, 3.0, argb32(col | 0xFF000000));
                }
                r
            })
            .inner;
        crate::panels::tag(ui, &r, &format!("SnapnabProps.Color.{t:?}"));
        egui::Popup::from_toggle_button_response(&r)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                ui.set_width(8.0 * 24.0);
                let mut chosen: Option<u32> = None;
                let mut row = |ui: &mut egui::Ui, title: &str, list: &[u32]| {
                    ui.label(RichText::new(title).size(11.0).strong());
                    for line in list.chunks(8) {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            for &c in line {
                                let (rect, r) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
                                let sel = (c & 0xFFFFFF) == (col & 0xFFFFFF) && col >> 24 != 0 && !grad_on;
                                ui.painter().rect_filled(rect.shrink(2.0), 2.0, argb32(c));
                                ui.painter().rect_stroke(rect.shrink(2.0), 2.0,
                                                         egui::Stroke::new(if sel || r.hovered() { 2.0 } else { 1.0 },
                                                                           if sel { Color32::from_rgb(0, 100, 200) } else { Color32::from_black_alpha(60) }),
                                                         egui::StrokeKind::Outside);
                                if r.clicked() {
                                    chosen = Some(c);
                                }
                            }
                        });
                    }
                };
                row(ui, &tr("Cores do tema"), &[theme[0], theme[1], theme[2], theme[3], 0xFF000000, 0xFF505050, 0xFFC8C8C8, 0xFFFFFFFF]);
                row(ui, &tr("Cores padrão"), &STANDARD);
                if !self.recent.is_empty() {
                    let rec = self.recent.clone();
                    row(ui, &tr("Recentes"), &rec);
                }
                let mut preset: Option<Gradient> = None;
                if gradient {
                    ui.label(RichText::new(tr("Gradientes")).size(11.0).strong());
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        for g in gradient_presets(theme) {
                            let (rect, r) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
                            let rr = rect.shrink(2.0);
                            for k in 0..8 {
                                let f = k as f32 / 7.0;
                                let x0 = rr.left() + rr.width() * k as f32 / 8.0;
                                ui.painter().rect_filled(egui::Rect::from_min_max(egui::pos2(x0, rr.top()), egui::pos2(x0 + rr.width() / 8.0 + 0.5, rr.bottom())),
                                                         0.0, argb32(grad_color(&g, f)));
                            }
                            if r.hovered() || s.grad == g {
                                ui.painter().rect_stroke(rr, 0.0, egui::Stroke::new(2.0, Color32::from_rgb(0, 100, 200)), egui::StrokeKind::Outside);
                            }
                            if r.clicked() {
                                preset = Some(g);
                            }
                        }
                    });
                }
                ui.horizontal(|ui| {
                    if allow_none && ui.button(tr("Sem cor")).clicked() {
                        chosen = Some(col & 0x00FFFFFF);
                    }
                    if ui.button(tr("Mais cores…")).clicked() {
                        let initial = to_colorref(col);
                        acts.push(PropAction::Modal(Box::new(move |owner| {
                            let c = crate::native::choose_color(owner, initial)?;
                            Some(Box::new(move |p: &mut PropsPanel, cv: &mut Canvas, _: &mut QuickStyleStore| {
                                let v = from_colorref(c);
                                p.push_recent(v);
                                let mut s = Self::cur(cv);
                                if gradient {
                                    s.grad.kind = 0;
                                }
                                Self::put_color(&mut s, Self::prop_tool(cv), t, v);
                                let mut a = Vec::new();
                                Self::set_style(cv, s, true, &mut a);
                                a
                            }) as Apply)
                        })));
                        ui.close();
                    }
                });
                if gradient && ui.button(tr("Editar gradiente…")).clicked() {
                    let (g, base) = (s.grad.clone(), col);
                    acts.push(PropAction::Modal(Box::new(move |owner| {
                        let g = edit_gradient(owner, &g, base)?;
                        Some(Box::new(move |_: &mut PropsPanel, cv: &mut Canvas, _: &mut QuickStyleStore| {
                            let mut s = Self::cur(cv);
                            let first = g.stops.first().map(|x| x.color).unwrap_or(base);
                            s.grad = g;
                            Self::put_color(&mut s, Self::prop_tool(cv), t, first);
                            let mut a = Vec::new();
                            Self::set_style(cv, s, true, &mut a);
                            a
                        }) as Apply)
                    })));
                    ui.close();
                }
                if let Some(g) = preset {
                    // a gradient preset: the solid color becomes its first stop
                    let mut s2 = Self::cur(cv);
                    let first = g.stops[0].color;
                    s2.grad = g;
                    Self::put_color(&mut s2, pt, t, first);
                    Self::set_style(cv, s2, true, acts);
                    ui.close();
                }
                if let Some(v) = chosen {
                    // a solid color (or none) turns the gradient off
                    self.push_recent(v);
                    let mut s2 = Self::cur(cv);
                    if gradient {
                        s2.grad.kind = 0;
                    }
                    Self::put_color(&mut s2, pt, t, v);
                    Self::set_style(cv, s2, true, acts);
                    ui.close();
                }
            });
    }

    fn push_recent(&mut self, v: u32) {
        if v >> 24 != 0 {
            self.recent.retain(|&c| c != v);
            self.recent.insert(0, v);
            self.recent.truncate(8);
        }
    }

    /// "Sombra ▾" (color, opacity, blur, distance) over the 3×3 direction grid.
    fn shadow(&mut self, ui: &mut egui::Ui, cv: &mut Canvas, theme: &[u32; 4], acts: &mut Vec<PropAction>) {
        let _ = theme;
        let s = Self::cur(cv);
        ui.vertical(|ui| {
            let mut ns: Option<Style> = None;
            let r = ui.menu_button(RichText::new(tr("Sombra ▾")).size(11.0), |ui| {
                let mut on = s.shadow;
                if ui.checkbox(&mut on, tr("Mostrar sombra")).clicked() {
                    ns = Some(Style { shadow: on, ..s.clone() });
                    ui.close();
                }
                if ui.button(tr("Cor da sombra…")).clicked() {
                    let initial = to_colorref(s.shadow_color);
                    acts.push(PropAction::Modal(Box::new(move |owner| {
                        let c = crate::native::choose_color(owner, initial)?;
                        Some(Box::new(move |_: &mut PropsPanel, cv: &mut Canvas, _: &mut QuickStyleStore| {
                            let mut s = Self::cur(cv);
                            s.shadow_color = from_colorref(c);
                            s.shadow = true;
                            let mut a = Vec::new();
                            Self::set_style(cv, s, true, &mut a);
                            a
                        }) as Apply)
                    })));
                    ui.close();
                }
                ui.menu_button(tr("Opacidade"), |ui| {
                    for v in [20, 30, 45, 60, 80, 100] {
                        if ui.radio(s.shadow_opacity == v, format!("{v}%")).clicked() {
                            ns = Some(Style { shadow_opacity: v, shadow: true, ..s.clone() });
                            ui.close();
                        }
                    }
                });
                ui.menu_button(tr("Desfoque"), |ui| {
                    for v in [0, 2, 4, 6, 10, 16] {
                        if ui.radio(s.shadow_blur == v, format!("{v} px")).clicked() {
                            ns = Some(Style { shadow_blur: v, shadow: true, ..s.clone() });
                            ui.close();
                        }
                    }
                });
                ui.menu_button(tr("Distância"), |ui| {
                    for v in [0, 2, 5, 8, 12, 20] {
                        if ui.radio(s.shadow_dist == v, format!("{v} px")).clicked() {
                            ns = Some(Style { shadow_dist: v, shadow: true, ..s.clone() });
                            ui.close();
                        }
                    }
                });
            });
            crate::panels::tag(ui, &r.response, "SnapnabProps.ShadowMenu");
            // the direction grid: a click picks the side, a click on the chosen one turns the shadow off
            let cs = 15.0;
            let (rect, gr) = ui.allocate_exact_size(Vec2::new(cs * 3.0 + 8.0, cs * 3.0), Sense::click());
            crate::panels::tag(ui, &gr, "SnapnabProps.Shadow");
            let gx = rect.center().x - cs * 1.5;
            let accent = ui.visuals().selection.bg_fill;
            for i in 0..9 {
                let cell = egui::Rect::from_min_size(egui::pos2(gx + (i % 3) as f32 * cs, rect.top() + (i / 3) as f32 * cs), Vec2::splat(cs));
                let on = s.shadow && s.shadow_dir == i;
                ui.painter().rect_filled(cell, 0.0, if on { accent.gamma_multiply(0.3) } else { Color32::WHITE });
                ui.painter().rect_stroke(cell, 0.0, egui::Stroke::new(1.0, Color32::from_gray(200)), egui::StrokeKind::Inside);
                if on {
                    let k = cs;
                    let (x, y) = (cell.left(), cell.top());
                    ui.painter().add(egui::Shape::line(
                        vec![egui::pos2(x + k * 0.22, y + k * 0.52), egui::pos2(x + k * 0.42, y + k * 0.72), egui::pos2(x + k * 0.78, y + k * 0.3)],
                        egui::Stroke::new(2.0, accent),
                    ));
                }
            }
            if gr.clicked()
                && let Some(q) = gr.interact_pointer_pos()
            {
                let (cx, cy) = (((q.x - gx) / cs).floor() as i32, ((q.y - rect.top()) / cs).floor() as i32);
                if (0..3).contains(&cx) && (0..3).contains(&cy) {
                    let dir = cy * 3 + cx;
                    let mut s2 = s.clone();
                    if s.shadow && s.shadow_dir == dir {
                        s2.shadow = false;
                    } else {
                        s2.shadow = true;
                        s2.shadow_dir = dir;
                    }
                    ns = Some(s2);
                }
            }
            if let Some(s2) = ns {
                Self::set_style(cv, s2, true, acts);
            }
        });
    }

    /// Shape / area / callout / step / magnifier shape, as a grid of previews.
    fn shape_picker(&mut self, ui: &mut egui::Ui, cv: &mut Canvas, area: bool, acts: &mut Vec<PropAction>) {
        let pt = Self::prop_tool(cv);
        let s = Self::cur(cv);
        let (count, sel) = if area {
            (AREA_SHAPE_COUNT, s.area_shape)
        } else {
            match pt {
                Tool::Callout => (CALLOUT_SHAPES, s.callout_shape),
                Tool::Step => (4, s.step_shape),
                Tool::Magnify => (MAGNIFY_KIND_COUNT, s.magnify_shape),
                _ => (SHAPE_COUNT, s.shape_kind),
            }
        };
        let with = |i: i32| {
            let mut v = s.clone();
            if area {
                v.area_shape = i;
            } else {
                match pt {
                    Tool::Callout => v.callout_shape = i,
                    Tool::Step => v.step_shape = i,
                    Tool::Magnify => v.magnify_shape = i,
                    _ => v.shape_kind = i,
                }
            }
            v
        };
        let preview_tool = if area && pt == Tool::Highlighter { Tool::Highlighter } else if area { pt } else { pt };
        ui.vertical(|ui| {
            ui.label(RichText::new(tr("Forma")).size(11.0));
            let tex = self.variant_thumb(ui.ctx(), preview_tool, &with(sel));
            let r = ui.add(egui::Button::image(egui::Image::new((tex.id(), Vec2::splat(30.0)))).min_size(Vec2::new(52.0, 30.0)));
            crate::panels::tag(ui, &r, "SnapnabProps.Shape");
            let mut picked: Option<i32> = None;
            egui::Popup::from_toggle_button_response(&r).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                egui::Grid::new("shape-grid").spacing([2.0, 2.0]).show(ui, |ui| {
                    for i in 0..count {
                        let tex = self.variant_thumb(ui.ctx(), preview_tool, &with(i));
                        let r = ui.add(egui::Button::image(egui::Image::new((tex.id(), Vec2::splat(36.0)))).selected(i == sel));
                        crate::panels::tag(ui, &r, &format!("SnapnabProps.Shape.{i}"));
                        if r.clicked() {
                            picked = Some(i);
                            ui.close();
                        }
                        if i % 4 == 3 {
                            ui.end_row();
                        }
                    }
                });
            });
            if let Some(i) = picked {
                if area && is_point_area(i) {
                    acts.push(PropAction::Hint(if i == AREA_POLYGON {
                        tr("Polígono: clique nos cantos; clique no primeiro ponto ou dê um duplo clique para fechar.")
                    } else {
                        tr("Mão livre: arraste contornando a área.")
                    }));
                }
                Self::set_style(cv, with(i), true, acts);
            }
        });
    }

    fn cap_row(&mut self, ui: &mut egui::Ui, cv: &mut Canvas, caps: bool, _dash: bool, acts: &mut Vec<PropAction>) {
        ui.horizontal(|ui| {
            if caps {
                self.cap_picker(ui, cv, true, acts);
            } else {
                ui.add_sized([92.0, 20.0], egui::Label::new(tr("Tracejado")));
            }
            self.dash_picker(ui, cv, acts);
            if caps {
                self.cap_picker(ui, cv, false, acts);
            }
        });
    }

    fn line_variant(s: &Style) -> Style {
        let mut v = s.clone();
        v.stroke = 0xFF323238;
        v.width = 6.0;
        v.shadow = false;
        v
    }

    fn cap_picker(&mut self, ui: &mut egui::Ui, cv: &mut Canvas, start: bool, acts: &mut Vec<PropAction>) {
        let s = Self::cur(cv);
        let cur = if start { s.arrow_start } else { s.arrow_end };
        let with = |c: i32| {
            let mut v = Self::line_variant(&s);
            (v.arrow_start, v.arrow_end, v.dash) = if start { (c, CAP_FLAT, 0) } else { (CAP_FLAT, c, 0) };
            v
        };
        let tex = self.variant_thumb(ui.ctx(), Tool::Line, &with(cur));
        let r = ui.add(egui::Button::image(egui::Image::new((tex.id(), Vec2::splat(24.0)))))
            .on_hover_text(if start { tr("Ponta inicial") } else { tr("Ponta final") });
        crate::panels::tag(ui, &r, if start { "SnapnabProps.CapStart" } else { "SnapnabProps.CapEnd" });
        let mut picked = None;
        egui::Popup::from_toggle_button_response(&r).show(|ui| {
            for (i, &c) in CAP_ORDER.iter().enumerate() {
                let tex = self.variant_thumb(ui.ctx(), Tool::Line, &with(c));
                let b = ui.add(egui::Button::image(egui::Image::new((tex.id(), Vec2::splat(28.0)))).selected(c == cur));
                crate::panels::tag(ui, &b, &format!("SnapnabProps.Cap.{i}"));
                if b.clicked() {
                    picked = Some(c);
                }
            }
        });
        if let Some(c) = picked {
            let mut s2 = Self::cur(cv);
            if start {
                s2.arrow_start = c;
            } else {
                s2.arrow_end = c;
            }
            Self::set_style(cv, s2, true, acts);
        }
    }

    fn dash_picker(&mut self, ui: &mut egui::Ui, cv: &mut Canvas, acts: &mut Vec<PropAction>) {
        let s = Self::cur(cv);
        let with = |d: i32| {
            let mut v = Self::line_variant(&s);
            (v.arrow_start, v.arrow_end, v.dash) = (CAP_FLAT, CAP_FLAT, d);
            v
        };
        let tex = self.variant_thumb(ui.ctx(), Tool::Line, &with(s.dash));
        let r = ui.add(egui::Button::image(egui::Image::new((tex.id(), Vec2::splat(24.0)))).min_size(Vec2::new(60.0, 24.0)))
            .on_hover_text(tr("Tracejado"));
        crate::panels::tag(ui, &r, "SnapnabProps.Dash");
        let mut picked = None;
        egui::Popup::from_toggle_button_response(&r).show(|ui| {
            for d in 0..DASH_COUNT {
                let tex = self.variant_thumb(ui.ctx(), Tool::Line, &with(d));
                let b = ui.add(egui::Button::image(egui::Image::new((tex.id(), Vec2::splat(28.0)))).selected(d == s.dash));
                crate::panels::tag(ui, &b, &format!("SnapnabProps.Dash.{d}"));
                if b.clicked() {
                    picked = Some(d);
                }
            }
        });
        if let Some(d) = picked {
            let mut s2 = Self::cur(cv);
            s2.dash = d;
            Self::set_style(cv, s2, true, acts);
        }
    }

    // ------------------------------------------------------------ geometry and object buttons
    fn geometry(&mut self, ui: &mut egui::Ui, cv: &mut Canvas, acts: &mut Vec<PropAction>) {
        let gk = cv.geometry_kind();
        if gk == 0 {
            return;
        }
        let _ = acts;
        let r = cv.geometry();
        let vals = [r.X.round() as i32, r.Y.round() as i32, r.Width.round() as i32, r.Height.round() as i32];
        if !self.geom_focus {
            for i in 0..4 {
                self.geom[i] = vals[i].to_string();
            }
            if r.Height > 0.0 {
                self.geom_aspect = r.Width / r.Height;
            }
        }
        ui.add_space(4.0);
        ui.label(if gk == 3 { tr("Área de corte (px)") } else if gk == 2 { tr("Seleção (px)") } else { tr("Posição e tamanho (px)") });
        // steps: position only
        let step = Self::sel_obj(cv).is_some_and(|o| o.kind == ObjType::Step);
        let mut any_focus = false;
        let mut edited: Option<usize> = None;
        let labels = ["X".to_owned(), "Y".to_owned(), tr("L"), tr("A")];
        for row in 0..2 {
            ui.horizontal(|ui| {
                for k in 0..2 {
                    let i = row * 2 + k;
                    ui.add_sized([18.0, 20.0], egui::Label::new(&labels[i]));
                    let en = !(step && i >= 2);
                    let w = ((ui.available_width() - 60.0) / (2 - k) as f32).max(40.0);
                    let r = ui.add_enabled(en, egui::TextEdit::singleline(&mut self.geom[i]).desired_width(w).align(egui::Align2::RIGHT_CENTER));
                    crate::panels::tag(ui, &r, &format!("SnapnabProps.Geometry.{i}"));
                    if r.gained_focus() {
                        self.geom_first = true;
                    }
                    any_focus |= r.has_focus();
                    if r.changed() {
                        edited = Some(i);
                    }
                }
                if row == 1 {
                    let on = self.aspect_lock;
                    let r = ui.add(egui::Button::new("🔗").selected(on)).on_hover_text(tr("Manter proporção"));
                    crate::panels::tag(ui, &r, "SnapnabProps.Aspect");
                    if r.clicked() {
                        let mut a = Vec::new();
                        self.set_value(cv, C::Aspect, !on as i32, true, &mut a);
                    }
                }
            });
        }
        self.geom_focus = any_focus;
        if let Some(i) = edited {
            let num = |s: &str| s.trim().parse::<i32>().ok().map(|v| v as f32);
            let (Some(x), Some(y), Some(mut w), Some(mut h)) = (num(&self.geom[0]), num(&self.geom[1]), num(&self.geom[2]), num(&self.geom[3])) else {
                return;
            };
            if w < 1.0 || h < 1.0 {
                return;
            }
            if self.aspect_lock && self.geom_aspect > 0.0 {
                // aspect lock: the other side follows
                if i == 2 {
                    h = (w / self.geom_aspect).round().max(1.0);
                    self.geom[3] = (h as i32).to_string();
                }
                if i == 3 {
                    w = (h * self.geom_aspect).round().max(1.0);
                    self.geom[2] = (w as i32).to_string();
                }
            }
            cv.set_geometry(RectF { X: x, Y: y, Width: w, Height: h }, self.geom_first);
            self.geom_first = false;
        }
    }

    fn object_buttons(&mut self, ui: &mut egui::Ui, cv: &mut Canvas, acts: &mut Vec<PropAction>) {
        let Some(floating) = Self::sel_obj(cv).map(|o| o.floating) else { return };
        ui.add_space(6.0);
        if cv.selection_count() > 1 {
            self.button(ui, 330, &tr("Agrupar"), acts);
        }
        if cv.can_ungroup() {
            self.button(ui, 331, &tr("Desagrupar"), acts);
        }
        self.button(ui, 327, &tr("Trazer para a frente"), acts);
        self.button(ui, 328, &tr("Enviar para trás"), acts);
        let locked = cv.selection_locked();
        let r = ui.add_sized([ui.available_width(), 28.0], egui::Button::new(if locked { tr("Desbloquear") } else { tr("Bloquear") }));
        crate::panels::tag(ui, &r, "SnapnabProps.Lock");
        if r.clicked() {
            let v = cv.selected_list();
            cv.set_locked(&v, !locked);
            acts.push(PropAction::DocChanged);
        }
        self.button(ui, 329, &tr("Excluir objeto"), acts);
        if floating {
            self.drop_button(ui, cv, acts);
        }
    }

    /// The panel shows a new tool or object: the geometry fields take the canvas' values again.
    pub fn reload(&mut self) {
        self.geom_focus = false;
    }

    /// Thumbnails depend on the theme and the stamps on disk.
    pub fn invalidate(&mut self) {
        self.thumbs.clear();
        self.fill_stamp_cats();
    }
}

fn grad_color(g: &Gradient, f: f32) -> u32 {
    let st = &g.stops;
    if st.is_empty() {
        return 0xFF000000;
    }
    if f <= st[0].pos {
        return st[0].color;
    }
    for w in st.windows(2) {
        if f <= w[1].pos {
            let t = ((f - w[0].pos) / (w[1].pos - w[0].pos).max(1e-4)).clamp(0.0, 1.0);
            let mix = |sh: u32| {
                let (a, b) = ((w[0].color >> sh & 255) as f32, (w[1].color >> sh & 255) as f32);
                ((a + (b - a) * t).round() as u32) << sh
            };
            return mix(24) | mix(16) | mix(8) | mix(0);
        }
    }
    st[st.len() - 1].color
}

fn gradient_presets(t: &[u32; 4]) -> Vec<Gradient> {
    let stop = |pos: f32, color: u32| GradStop { pos, color };
    let lin = |a: f32, s: Vec<GradStop>| Gradient { kind: 1, angle: a, stops: s };
    let rad = |s: Vec<GradStop>| Gradient { kind: 2, angle: 90.0, stops: s };
    vec![
        lin(90.0, vec![stop(0.0, 0xFFFFFFFF), stop(1.0, t[0])]),
        lin(0.0, vec![stop(0.0, t[1]), stop(1.0, t[2])]),
        lin(45.0, vec![stop(0.0, t[2]), stop(1.0, t[3])]),
        lin(0.0, vec![stop(0.0, t[0]), stop(0.5, t[1]), stop(1.0, t[3])]),
        rad(vec![stop(0.0, 0xFFFFFFFF), stop(1.0, t[0])]),
        lin(90.0, vec![stop(0.0, 0xFF505050), stop(1.0, 0xFF000000)]),
        lin(90.0, vec![stop(0.0, 0xFFF4F5F7), stop(1.0, 0xFFB0B4BC)]),
        rad(vec![stop(0.0, t[3]), stop(1.0, 0xFF101010)]),
    ]
}

/// EditGradient: type, angle, the two end colors and up to two middle stops.
pub fn edit_gradient(owner: isize, gr: &Gradient, base: u32) -> Option<Gradient> {
    let mut g = gr.clone();
    if !g.active() {
        g = Gradient { kind: 1, angle: 90.0, stops: vec![GradStop { pos: 0.0, color: 0xFFFFFFFF }, GradStop { pos: 1.0, color: base | 0xFF000000 }] };
    }
    let has1 = g.stops.len() >= 3;
    let has2 = g.stops.len() >= 4;
    let mid1 = if has1 { g.stops[1] } else { GradStop { pos: 0.33, color: 0xFFFFFFFF } };
    let mid2 = if has2 { g.stops[2] } else { GradStop { pos: 0.66, color: 0xFF808080 } };
    let mut f = vec![
        Field::Combo { label: tr("Tipo"), items: vec![tr("Linear"), tr("Radial (do centro)")], value: if g.kind == 2 { 1 } else { 0 } },
        Field::Int { label: tr("Ângulo (graus)"), value: g.angle as i32, min: 0, max: 359 },
        Field::Color { label: tr("Cor inicial"), color: to_colorref(g.stops[0].color) },
        Field::Color { label: tr("Cor final"), color: to_colorref(g.stops[g.stops.len() - 1].color) },
        Field::Check { label: tr("Cor intermediária"), value: has1 },
        Field::Color { label: tr("Cor"), color: to_colorref(mid1.color) },
        Field::Int { label: tr("Posição (%)"), value: (mid1.pos * 100.0).round() as i32, min: 1, max: 99 },
        Field::Check { label: tr("Segunda cor intermediária"), value: has2 },
        Field::Color { label: tr("Cor"), color: to_colorref(mid2.color) },
        Field::Int { label: tr("Posição (%)"), value: (mid2.pos * 100.0).round() as i32, min: 1, max: 99 },
        Field::Label(tr("O ângulo vale para o gradiente linear: 0° da esquerda para a direita, 90° de cima para baixo.")),
    ];
    if !snapui::form::run(owner, &tr("Gradiente"), &mut f) {
        return None;
    }
    let mut out = Gradient {
        kind: if f[0].int() == 1 { 2 } else { 1 },
        angle: f[1].int() as f32,
        stops: vec![GradStop { pos: 0.0, color: from_colorref(f[2].color()) }, GradStop { pos: 1.0, color: from_colorref(f[3].color()) }],
    };
    if f[4].on() {
        out.stops.push(GradStop { pos: f[6].int() as f32 / 100.0, color: from_colorref(f[5].color()) });
    }
    if f[7].on() {
        out.stops.push(GradStop { pos: f[9].int() as f32 / 100.0, color: from_colorref(f[8].color()) });
    }
    out.stops.sort_by(|a, b| a.pos.total_cmp(&b.pos));
    Some(out)
}

/// The theme form (Name + Color 1..4): a new theme (`replace` None) or the edited one in place.
fn theme_form(t: ToolTheme, name: String, replace: Option<usize>) -> PropAction {
    PropAction::Modal(Box::new(move |owner| {
        let mut f = vec![Field::Text { label: tr("Nome"), text: name }];
        for i in 0..4 {
            f.push(Field::Color { label: tr("Cor %d").replace("%d", &(i + 1).to_string()), color: to_colorref(t.colors[i]) });
        }
        f.push(Field::Label(tr("Os estilos rápidos de todas as ferramentas são gerados com estas cores.")));
        let title = if replace.is_none() { tr("Novo tema") } else { tr("Editar tema") };
        if !snapui::form::run(owner, &title, &mut f) || f[0].text().trim().is_empty() {
            return None;
        }
        let mut t = t;
        t.name = f[0].text().trim().to_owned();
        for i in 0..4 {
            t.colors[i] = from_colorref(f[1 + i].color());
        }
        Some(Box::new(move |p: &mut PropsPanel, _: &mut Canvas, qs: &mut QuickStyleStore| {
            match replace {
                Some(i) if i < qs.themes.len() => {
                    qs.themes[i] = t;
                    qs.save();
                }
                _ => {
                    qs.add_theme(t);
                }
            }
            p.thumbs.clear();
            vec![]
        }) as Apply)
    }))
}
