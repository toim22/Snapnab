//! The painting area (CEditorPaintControl, comp_5 §3.5; src/editor/Canvas.cpp): zoom, scrolling, the tools, the
//! handles and the raster selection. Everything here works in physical pixels of the canvas area, as the C++ did,
//! so 100 % zoom is one image pixel per screen pixel at any DPI; `canvas_ui.rs` feeds it the mouse and draws it.

use std::sync::Arc;
use std::time::Instant;

use snapcore::cv::{self, Mask};
use snapcore::doc::{
    self as d, AREA_FREEHAND, AREA_POLYGON, CAP_ARROW, CAP_FLAT, DASH_SOLID, DrawObj, Gradient, ObjType, PointF, RectF, SHAPE_ELLIPSE,
    SHAPE_HEXAGON, SHAPE_RECT, SHAPE_ROUND_RECT, SHAPE_TRIANGLE, Style, is_point_area,
};
use snapcore::gdip::{pt, rect};
use snapcore::image::{Image, Rect, blend_over, ca, cb, cg, cr};
use snapcore::render;

use crate::doc::Document;
use crate::tools;
use crate::tr;

/// Favorites and MagicWand are toolbar items: Favorites shows the favorite styles of every tool and the Magic wand is
/// the Selection tool in wand mode. The order is the C++ enum's (the e2e suite posts T_TOOL_BASE + index).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Tool {
    Select,
    #[default]
    Arrow,
    Line,
    Shape,
    Pen,
    Highlighter,
    Text,
    Callout,
    Step,
    Stamp,
    Blur,
    Spotlight,
    Magnify,
    Fill,
    Eraser,
    Crop,
    Selection,
    Cutout,
    Eyedropper,
    Favorites,
    MagicWand,
}

pub const TOOL_COUNT: usize = 21;

impl Tool {
    pub const ALL: [Tool; TOOL_COUNT] = [
        Tool::Select, Tool::Arrow, Tool::Line, Tool::Shape, Tool::Pen, Tool::Highlighter, Tool::Text, Tool::Callout, Tool::Step,
        Tool::Stamp, Tool::Blur, Tool::Spotlight, Tool::Magnify, Tool::Fill, Tool::Eraser, Tool::Crop, Tool::Selection, Tool::Cutout,
        Tool::Eyedropper, Tool::Favorites, Tool::MagicWand,
    ];

    pub fn from_index(i: usize) -> Option<Tool> {
        Self::ALL.get(i).copied()
    }

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn name(self) -> String {
        tr(match self {
            Tool::Select => "Mover",
            Tool::Arrow => "Seta",
            Tool::Line => "Linha",
            Tool::Shape => "Forma",
            Tool::Pen => "Caneta",
            Tool::Highlighter => "Marcador",
            Tool::Text => "Texto",
            Tool::Callout => "Balão",
            Tool::Step => "Passos",
            Tool::Stamp => "Carimbo",
            Tool::Blur => "Ocultar",
            Tool::Spotlight => "Destaque",
            Tool::Magnify => "Lupa",
            Tool::Fill => "Preencher",
            Tool::Eraser => "Borracha",
            Tool::Crop => "Cortar",
            Tool::Selection => "Seleção",
            Tool::Cutout => "Recortar faixa",
            Tool::Eyedropper => "Conta-gotas",
            Tool::Favorites => "Favoritos",
            Tool::MagicWand => "Varinha mágica",
        })
    }
}

/// What the canvas tells the window (the C++ callbacks), read after each frame.
#[derive(Clone, Debug, PartialEq)]
pub enum Note {
    DocChanged,
    SelectionChanged,
    AreaChanged,
    ColorPicked(u32),
    ZoomChanged,
    CursorMoved(bool, f32, f32),
    Hint(String),
    /// right click, in physical pixels of the canvas area
    ContextMenu(i32, i32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Drag {
    #[default]
    None,
    Create,
    Move,
    Resize,
    Pan,
    Crop,
    Select,
    Lasso,
    Erase,
    Cut,
    Marquee,
    Polygon,
    Guide,
}

/// Keys held with a mouse event.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

/// The cursor shapes the canvas asks for (IDC_*).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Cursor {
    #[default]
    Arrow,
    Cross,
    SizeAll,
    SizeNs,
    SizeWe,
    SizeNwse,
    SizeNesw,
    Hand,
    Dropper,
}

/// Text typed in place (IInPlaceRichEditOwner): the object's text while it is edited.
#[derive(Clone, Debug)]
pub struct TextEdit {
    pub idx: usize,
    pub new: bool,
    pub text: String,
    /// the edit box takes the focus (and selects everything) on its first frame
    pub fresh: bool,
}

pub type P = (i32, i32);

pub const MARGIN: i32 = 40;
const HOLD_MS: u64 = 700;

fn hypot(x: f32, y: f32) -> f32 {
    x.hypot(y)
}

fn rgb_colorref(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}

fn rf_right(r: &RectF) -> f32 {
    r.X + r.Width
}

fn rf_bottom(r: &RectF) -> f32 {
    r.Y + r.Height
}

fn intersects(a: &RectF, b: &RectF) -> bool {
    a.X < rf_right(b) && b.X < rf_right(a) && a.Y < rf_bottom(b) && b.Y < rf_bottom(a)
}

pub fn obj_extent(o: &DrawObj) -> RectF {
    if o.is_line() || o.is_path() || o.kind == ObjType::Group { o.bounds() } else { o.rect() }
}

/// The text of an object as a Rust string.
pub fn obj_text(o: &DrawObj) -> String {
    String::from_utf16_lossy(&o.text)
}

fn fit_to_points(o: &mut DrawObj) {
    let Some(f) = o.pts.first() else { return };
    let (mut x0, mut y0, mut x1, mut y1) = (f.X, f.Y, f.X, f.Y);
    for q in &o.pts {
        x0 = x0.min(q.X);
        y0 = y0.min(q.Y);
        x1 = x1.max(q.X);
        y1 = y1.max(q.Y);
    }
    o.p1 = pt(x0, y0);
    o.p2 = pt(x1, y1);
}

/// Width/height of the "perfect" version of a shape (Shift while drawing): square, circle, equilateral triangle,
/// regular hexagon; stamps keep the proportion of their image.
fn perfect_aspect(o: &DrawObj) -> f32 {
    let k = if o.kind == ObjType::Shape {
        o.style.shape_kind
    } else if o.is_area_tool() {
        o.style.area_shape
    } else if o.kind == ObjType::Magnify {
        if o.style.magnify_shape == 3 { SHAPE_HEXAGON } else { -1 }
    } else if o.kind == ObjType::Stamp {
        return render::stamp_aspect(&obj_text(o));
    } else {
        -1
    };
    if k == SHAPE_TRIANGLE || k == SHAPE_HEXAGON { 1.1547 } else { 1.0 }
}

fn geom_rect(o: &DrawObj) -> RectF {
    if (o.is_path() || o.is_point_shape()) && !o.pts.is_empty() {
        let (mut x0, mut y0) = (o.pts[0].X, o.pts[0].Y);
        let (mut x1, mut y1) = (x0, y0);
        for p in &o.pts {
            x0 = x0.min(p.X);
            y0 = y0.min(p.Y);
            x1 = x1.max(p.X);
            y1 = y1.max(p.Y);
        }
        return rect(x0, y0, x1 - x0, y1 - y0);
    }
    if o.kind == ObjType::Step {
        let dd = o.style.font_size * 1.5;
        return rect(o.p1.X - dd / 2.0, o.p1.Y - dd / 2.0, dd, dd);
    }
    o.rect()
}

fn family(o: &DrawObj) -> i32 {
    if o.is_shape() {
        ObjType::Shape as i32
    } else if o.kind == ObjType::Pixelate {
        ObjType::Blur as i32
    } else {
        o.kind as i32
    }
}

/// An area (highlighter, spotlight, hide) that changes shape: a polygon or freehand outline starts from the current
/// box (its corners, or an ellipse inside it); going back to a shape drops the points.
fn adapt_area_points(o: &mut DrawObj, old_area: i32) {
    if !o.is_area_tool() {
        return;
    }
    let r = o.rect();
    if is_point_area(o.style.area_shape) && o.pts.len() < 3 {
        o.pts.clear();
        if o.style.area_shape == AREA_POLYGON {
            o.pts = vec![pt(r.X, r.Y), pt(rf_right(&r), r.Y), pt(rf_right(&r), rf_bottom(&r)), pt(r.X, rf_bottom(&r))];
        } else {
            for k in 0..24 {
                let a = k as f64 * 2.0 * std::f64::consts::PI / 24.0;
                o.pts.push(pt(r.X + r.Width / 2.0 + a.cos() as f32 * r.Width / 2.0, r.Y + r.Height / 2.0 + a.sin() as f32 * r.Height / 2.0));
            }
        }
    } else if !is_point_area(o.style.area_shape) && is_point_area(old_area) {
        o.pts.clear();
    }
}

/// The blue tint over the raster selection.
fn make_overlay(m: &[u8], w: i32, b: Rect) -> Image {
    let mut o = Image::sized(b.width(), b.height(), 0);
    for y in 0..o.hi() {
        for x in 0..o.wi() {
            if m[((b.top + y) * w + b.left + x) as usize] != 0 {
                *o.at_mut(x, y) = 0x553C8CFF;
            }
        }
    }
    o
}

/// The built-in styles per tool: the first quick style of the Basic theme (the quick styles replace them later).
pub fn default_styles() -> Vec<Style> {
    let red = 0xFFEF4136;
    let base = Style { stroke: red, ..Style::default() };
    let mut s = vec![base; TOOL_COUNT];
    let i = |t: Tool| t as usize;
    s[i(Tool::Arrow)].width = 8.0;
    s[i(Tool::Line)].arrow_end = CAP_FLAT;
    s[i(Tool::Line)].width = 4.0;
    s[i(Tool::Shape)].width = 4.0;
    s[i(Tool::Shape)].shape_kind = SHAPE_RECT;
    {
        let hl = &mut s[i(Tool::Highlighter)];
        hl.stroke = 0xFFFFEB3B;
        hl.opacity = 50;
        hl.shadow = false;
        hl.snap_text = true;
    }
    {
        let tx = &mut s[i(Tool::Text)];
        tx.text_color = red;
        tx.stroke = 0xFFFFFFFF;
        tx.width = 4.0;
        tx.font_size = 36.0;
    }
    {
        let co = &mut s[i(Tool::Callout)];
        co.callout_shape = 4;
        co.fill = red;
        co.stroke = red;
        co.width = 0.0;
        co.text_color = 0xFFFFFFFF;
        co.tail_color = red;
        co.tail_width = 6.0;
        co.font_size = 20.0;
        co.padding = 8.0;
    }
    s[i(Tool::Step)].font_size = 26.0;
    s[i(Tool::Step)].text_color = 0xFFFFFFFF;
    s[i(Tool::Magnify)].width = 4.0;
    s[i(Tool::Magnify)].magnify_scale = 200;
    s[i(Tool::Blur)].shadow = false;
    s[i(Tool::Spotlight)].shadow = false;
    s[i(Tool::Eraser)].width = 16.0;
    s[i(Tool::Eraser)].shadow = false;
    s[i(Tool::Fill)].shadow = false;
    s
}

pub struct Canvas {
    /// the open documents; `cur` is the one shown
    pub docs: Vec<Document>,
    pub cur: Option<usize>,
    /// the top-level window (owner of the clipboard and of the menus)
    pub hwnd: isize,
    pub tool: Tool,
    pub styles: Vec<Style>,
    /// stamp chosen in the gallery (emoji or "file:path")
    pub stamp_id: String,
    /// Fill eyedropper: the next click picks the fill color
    pub pick_color_once: bool,
    /// snap to canvas: the object's edges and center stick to the image's while moving
    pub snapping: bool,
    /// Move with Smart Move: dragging on the background selects an area and lifts it (inpainting)
    pub smart_move: bool,
    /// 0 rectangle, 1 ellipse, 2 lasso, 3 wand
    pub sel_mode: i32,
    /// 0 transparent, 1 canvas color, 2 smart (inpainting)
    pub sel_fill: i32,
    /// 0 removes a horizontal band, 1 vertical
    pub cut_dir: i32,
    pub tolerance: f32,
    pub wand_global: bool,
    /// COLORREF around the image
    pub background: u32,
    pub zoom: f64,
    /// selected object (the primary one, when there are several)
    pub selected: i32,
    /// View › Rulers: rulers, guides and cursor coordinates
    pub rulers: bool,
    /// selection and crop edges snap to contrast lines (Alt turns it off on the fly)
    pub edge_snap: bool,
    /// pasting text ignores the source formatting (Ctrl+Shift+V does the opposite)
    pub plain_paste: bool,
    /// Pen: drawing a rough shape and holding still turns it into the clean shape (C23)
    pub draw_hold: bool,
    pub notes: Vec<Note>,
    /// the size of the canvas area and of the rulers, in physical pixels (set by the drawing code)
    pub view_w: i32,
    pub view_h: i32,
    pub ruler_px: i32,
    pub cursor: Cursor,

    pub(crate) composed: Image,
    /// bumped each time `composed` is redone (the GPU texture follows it)
    pub(crate) composed_version: u64,
    pub(crate) dirty: bool,
    pub(crate) fx_preview: Option<Arc<Image>>,
    pub(crate) scroll: P,
    pub(crate) fit: bool,
    pub(crate) drag: Drag,
    axis_lock: i32,
    pending_deselect: i32,
    label_ready: bool,
    guide_axis: i32,
    guide_idx: i32,
    pub(crate) link_hot: i32,
    down_screen: P,
    down_doc: PointF,
    pan_start: P,
    pub(crate) preview: Option<DrawObj>,
    /// bumped each time the object being created changes
    pub(crate) preview_version: u64,
    drag_orig: DrawObj,
    drag_multi: Vec<(usize, DrawObj)>,
    pub(crate) extra: Vec<i32>,
    handle: i32,
    changed: bool,
    pub(crate) has_sel: bool,
    sel_mask: Mask,
    pub(crate) sel_bounds: Rect,
    pub(crate) sel_rect: RectF,
    pub(crate) lasso: Vec<PointF>,
    pub(crate) sel_overlay: Image,
    pub(crate) sel_version: u64,
    pub(crate) has_crop: bool,
    pub(crate) crop: RectF,
    pub edit: Option<TextEdit>,
    erase_last: PointF,
    erase_shift: bool,
    erase_anchor: PointF,
    pen_anchor: i32,
    hold_shape: bool,
    pub(crate) hold_at: Option<Instant>,
    ocr_lines: Vec<snapcore::ocr::Line>,
    ocr_base: Option<*const Image>,
    ocr_tried: bool,
    /// last input came from touch or pen: bigger handles
    pub touch_input: bool,
    /// the mouse position over the canvas (rulers), or None
    pub(crate) mouse: Option<P>,
}

impl Default for Canvas {
    fn default() -> Self {
        Canvas {
            docs: Vec::new(),
            cur: None,
            hwnd: 0,
            tool: Tool::Arrow,
            styles: default_styles(),
            stamp_id: "👍".into(),
            pick_color_once: false,
            snapping: true,
            smart_move: false,
            sel_mode: 0,
            sel_fill: 2,
            cut_dir: 0,
            tolerance: 15.0,
            wand_global: false,
            background: rgb_colorref(222, 224, 228),
            zoom: 1.0,
            selected: -1,
            rulers: false,
            edge_snap: true,
            plain_paste: true,
            draw_hold: true,
            notes: Vec::new(),
            view_w: 800,
            view_h: 600,
            ruler_px: 20,
            cursor: Cursor::Arrow,
            composed: Image::default(),
            composed_version: 0,
            dirty: true,
            fx_preview: None,
            scroll: (0, 0),
            fit: true,
            drag: Drag::None,
            axis_lock: 0,
            pending_deselect: -1,
            label_ready: false,
            guide_axis: 0,
            guide_idx: -1,
            link_hot: -1,
            down_screen: (0, 0),
            down_doc: PointF::default(),
            pan_start: (0, 0),
            preview: None,
            preview_version: 0,
            drag_orig: DrawObj::default(),
            drag_multi: Vec::new(),
            extra: Vec::new(),
            handle: -1,
            changed: false,
            has_sel: false,
            sel_mask: Vec::new(),
            sel_bounds: Rect::default(),
            sel_rect: RectF::default(),
            lasso: Vec::new(),
            sel_overlay: Image::default(),
            sel_version: 0,
            has_crop: false,
            crop: RectF::default(),
            edit: None,
            erase_last: PointF::default(),
            erase_shift: false,
            erase_anchor: PointF::default(),
            pen_anchor: -1,
            hold_shape: false,
            hold_at: None,
            ocr_lines: Vec::new(),
            ocr_base: None,
            ocr_tried: false,
            touch_input: false,
            mouse: None,
        }
    }
}

impl Canvas {
    // ---------------------------------------------------------------- documents
    pub fn doc(&self) -> Option<&Document> {
        self.cur.and_then(|i| self.docs.get(i))
    }

    pub fn doc_mut(&mut self) -> Option<&mut Document> {
        self.cur.and_then(|i| self.docs.get_mut(i))
    }

    /// The current document (callers check `has_image` / `doc()` first).
    fn d(&self) -> &Document {
        &self.docs[self.cur.unwrap()]
    }

    fn dm(&mut self) -> &mut Document {
        let i = self.cur.unwrap();
        &mut self.docs[i]
    }

    /// There is an image document (not a video) on the canvas.
    pub fn has_image(&self) -> bool {
        self.doc().is_some_and(|d| !d.is_video && !d.st.base.is_empty())
    }

    fn nobjs(&self) -> usize {
        self.doc().map_or(0, |d| d.st.objs.len())
    }

    fn obj(&self, i: usize) -> &DrawObj {
        &self.d().st.objs[i]
    }

    fn note(&mut self, n: Note) {
        self.notes.push(n);
    }

    fn doc_changed(&mut self) {
        self.note(Note::DocChanged);
    }

    fn sel_changed(&mut self) {
        self.note(Note::SelectionChanged);
    }

    pub fn current_style(&mut self) -> &mut Style {
        &mut self.styles[self.tool.index()]
    }

    pub fn style_of(&self, t: Tool) -> &Style {
        &self.styles[t.index()]
    }

    /// The document shown changes (or goes away).
    pub fn set_document(&mut self, idx: Option<usize>) {
        self.commit_text();
        self.cur = idx.filter(|i| *i < self.docs.len());
        self.selected = -1;
        self.extra.clear();
        self.has_sel = false;
        self.has_crop = false;
        self.drag = Drag::None;
        self.preview = None;
        self.label_ready = false;
        self.link_hot = -1;
        self.fit = true;
        self.dirty = true;
        self.fx_preview = None;
        self.fit_to_window();
        self.sel_changed();
    }

    pub fn set_tool(&mut self, t: Tool) {
        self.commit_text();
        self.reset_polygon();
        self.label_ready = false;
        let mut t = t;
        if t == Tool::MagicWand {
            self.sel_mode = 3;
            t = Tool::Selection;
        }
        if t == Tool::Favorites {
            t = self.tool;
        }
        if t != Tool::Selection && !(t == Tool::Select && self.smart_move) {
            self.clear_raster_selection();
        }
        if t != Tool::Crop {
            self.has_crop = false;
        }
        self.tool = t;
        if t != Tool::Select && self.selected >= 0 && self.doc().is_some() {
            self.selected = -1;
            self.extra.clear();
            self.sel_changed();
        }
    }

    /// The document changed outside the canvas (undo, an effect…): redraw it and keep the selection valid.
    pub fn refresh(&mut self, doc_changed: bool) {
        if doc_changed {
            self.dirty = true;
        }
        let n = self.nobjs() as i32;
        if self.selected >= n {
            self.selected = -1;
        }
        if self.doc().is_none() || self.selected < 0 {
            self.extra.clear();
        } else {
            let sel = self.selected;
            self.extra.retain(|&i| i >= 0 && i < n && i != sel);
        }
        self.clamp_scroll();
    }

    /// The composed image (objects drawn on the base), redone when the document changed.
    pub fn recompose(&mut self) {
        let Some(d) = self.doc() else { return };
        if d.is_video || !self.dirty {
            return;
        }
        let hide = self.edit.as_ref().map(|e| e.idx).filter(|i| *i < d.st.objs.len());
        self.composed = d.render(hide);
        self.composed_version += 1;
        self.dirty = false;
    }

    pub fn composed(&mut self) -> &Image {
        self.recompose();
        &self.composed
    }

    // ---------------------------------------------------------------- coordinates
    pub fn ruler_size(&self) -> i32 {
        if self.rulers && self.has_image() { self.ruler_px } else { 0 }
    }

    /// Area where the image is shown: the canvas without the rulers (top and left): left, top, right, bottom.
    pub fn view(&self) -> (i32, i32, i32, i32) {
        let r = self.ruler_size();
        (r, r, self.view_w.max(r), self.view_h.max(r))
    }

    fn image_size_on_screen(&self) -> P {
        match self.doc() {
            Some(d) => ((d.w() as f64 * self.zoom).round() as i32, (d.h() as f64 * self.zoom).round() as i32),
            None => (0, 0),
        }
    }

    pub fn origin(&self) -> P {
        let (l, t, r, b) = self.view();
        let (vw, vh) = (r - l, b - t);
        let (iw, ih) = self.image_size_on_screen();
        (
            l + if iw + 2 * MARGIN <= vw { (vw - iw) / 2 } else { MARGIN - self.scroll.0 },
            t + if ih + 2 * MARGIN <= vh { (vh - ih) / 2 } else { MARGIN - self.scroll.1 },
        )
    }

    pub fn to_doc(&self, p: P) -> PointF {
        let o = self.origin();
        pt(((p.0 - o.0) as f64 / self.zoom) as f32, ((p.1 - o.1) as f64 / self.zoom) as f32)
    }

    pub fn to_screen(&self, p: PointF) -> P {
        let o = self.origin();
        ((o.0 as f64 + p.X as f64 * self.zoom).round() as i32, (o.1 as f64 + p.Y as f64 * self.zoom).round() as i32)
    }

    /// left, top, right, bottom on screen
    pub fn to_screen_rect(&self, r: RectF) -> (i32, i32, i32, i32) {
        let a = self.to_screen(pt(r.X, r.Y));
        let b = self.to_screen(pt(rf_right(&r), rf_bottom(&r)));
        (a.0, a.1, b.0, b.1)
    }

    /// The scrolling range: the image plus the margins, in physical pixels (content size, page size) per axis.
    pub fn scroll_range(&self) -> ((i32, i32), (i32, i32)) {
        let (l, t, r, b) = self.view();
        let (iw, ih) = self.image_size_on_screen();
        ((iw + 2 * MARGIN, r - l), (ih + 2 * MARGIN, b - t))
    }

    pub fn scroll_pos(&self) -> P {
        self.scroll
    }

    pub fn set_scroll(&mut self, x: i32, y: i32) {
        self.scroll = (x, y);
        self.clamp_scroll();
    }

    fn clamp_scroll(&mut self) {
        let ((cw, vw), (ch, vh)) = self.scroll_range();
        self.scroll.0 = self.scroll.0.clamp(0, (cw - vw).max(0));
        self.scroll.1 = self.scroll.1.clamp(0, (ch - vh).max(0));
    }

    /// The canvas area changed size.
    pub fn set_view_size(&mut self, w: i32, h: i32) {
        if (w, h) == (self.view_w, self.view_h) {
            return;
        }
        self.view_w = w.max(1);
        self.view_h = h.max(1);
        if self.fit {
            self.fit_to_window();
        } else {
            self.clamp_scroll();
        }
    }

    pub fn zoom_to(&mut self, z: f64, anchor: Option<P>) {
        if self.doc().is_none() {
            return;
        }
        let z = z.clamp(0.05, 16.0);
        let (l, t, r, b) = self.view();
        let a = anchor.unwrap_or(((l + r) / 2, (t + b) / 2));
        let dd = self.to_doc(a);
        self.zoom = z;
        self.fit = false;
        // keeps the document point under the anchor (the mouse, for the wheel)
        self.scroll.0 = (MARGIN as f64 + dd.X as f64 * self.zoom - (a.0 - l) as f64).round() as i32;
        self.scroll.1 = (MARGIN as f64 + dd.Y as f64 * self.zoom - (a.1 - t) as f64).round() as i32;
        self.clamp_scroll();
        if self.edit.is_some() {
            self.commit_text();
        }
        self.note(Note::ZoomChanged);
    }

    pub fn zoom_in(&mut self) {
        self.zoom_to(self.zoom * 1.25, None);
    }

    pub fn zoom_out(&mut self) {
        self.zoom_to(self.zoom / 1.25, None);
    }

    pub fn fit_to_window(&mut self) {
        let (l, t, r, b) = self.view();
        let (cw, ch) = (r - l, b - t);
        self.zoom = match self.doc() {
            Some(d) if d.w() > 0 && cw > 2 * MARGIN && ch > 2 * MARGIN => {
                let z = ((cw - 2 * MARGIN) as f64 / d.w() as f64).min((ch - 2 * MARGIN) as f64 / d.h() as f64);
                z.min(1.0)
            }
            _ => 1.0,
        };
        self.scroll = (0, 0);
        self.fit = true;
        self.note(Note::ZoomChanged);
    }

    // ---------------------------------------------------------------- handles and selection
    pub fn handle_size(&self) -> i32 {
        if self.touch_input { 10 } else { 5 }
    }

    pub fn handle_points(&self, o: &DrawObj) -> Vec<P> {
        let mut h = Vec::new();
        if o.is_line() {
            h.push(self.to_screen(o.p1));
            h.push(self.to_screen(o.p2));
            if o.style.bezier {
                // curve handle: the midpoint of the curve
                h.push(self.to_screen(pt(
                    0.25 * o.p1.X + 0.5 * o.ctrl.X + 0.25 * o.p2.X,
                    0.25 * o.p1.Y + 0.5 * o.ctrl.Y + 0.25 * o.p2.Y,
                )));
            }
            return h;
        }
        if o.kind == ObjType::Step {
            return h;
        }
        let r = if o.is_path() {
            let mut b = o.bounds();
            let pad = o.style.width / 2.0 + if o.style.shadow { 4.0 } else { 0.0 };
            d::inflate(&mut b, -pad, -pad);
            b
        } else {
            o.rect()
        };
        let (l, t, rr, bb) = self.to_screen_rect(r);
        let (mx, my) = ((l + rr) / 2, (t + bb) / 2);
        h = vec![(l, t), (mx, t), (rr, t), (rr, my), (rr, bb), (mx, bb), (l, bb), (l, my)];
        if (o.kind == ObjType::Callout && o.callout_has_tail()) || (o.kind == ObjType::Magnify && o.style.magnify_link) {
            h.push(self.to_screen(o.tail));
        }
        h
    }

    /// The selected objects' indices in ascending order (the primary one included).
    pub fn selected_list(&self) -> Vec<usize> {
        let n = self.nobjs() as i32;
        if self.doc().is_none() || self.selected < 0 || self.selected >= n {
            return Vec::new();
        }
        let mut v = vec![self.selected as usize];
        for &i in &self.extra {
            if i >= 0 && i < n && i != self.selected {
                v.push(i as usize);
            }
        }
        v.sort();
        v.dedup();
        v
    }

    pub fn selection_count(&self) -> usize {
        self.selected_list().len()
    }

    pub fn is_selected(&self, i: usize) -> bool {
        self.selected_list().contains(&i)
    }

    /// Selects these objects; the last one becomes the primary.
    pub fn select_objects(&mut self, idx: &[usize]) {
        self.extra.clear();
        self.selected = -1;
        let n = self.nobjs();
        for &i in idx {
            if i < n {
                if self.selected >= 0 {
                    self.extra.push(self.selected);
                }
                self.selected = i as i32;
            }
        }
        self.sel_changed();
    }

    pub fn select_all_objects(&mut self) {
        if self.doc().is_none() {
            return;
        }
        let all: Vec<usize> = (0..self.nobjs()).collect();
        if self.tool != Tool::Select {
            self.set_tool(Tool::Select);
        }
        self.select_objects(&all);
    }

    pub fn can_ungroup(&self) -> bool {
        self.selected_list().iter().any(|&i| self.obj(i).kind == ObjType::Group)
    }

    /// Group (CPaintOpGroup): the selected objects become a single one, at the position of the frontmost.
    pub fn group_selected(&mut self) {
        let v = self.selected_list();
        if self.doc().is_none() || v.len() < 2 {
            return;
        }
        self.commit_text();
        self.dm().checkpoint();
        let mut g = DrawObj::new(ObjType::Group);
        g.style.shadow = false;
        for &i in &v {
            g.children.push(self.obj(i).clone());
        }
        g.update_group_rect();
        let at = v[v.len() - 1] - (v.len() - 1);
        let objs = &mut self.dm().st.objs;
        for &i in v.iter().rev() {
            objs.remove(i);
        }
        objs.insert(at, Arc::new(g));
        self.extra.clear();
        self.selected = at as i32;
        self.refresh(true);
        self.doc_changed();
        self.sel_changed();
    }

    /// Ungroup: each selected group turns back into its objects, which stay selected.
    pub fn ungroup_selected(&mut self) {
        let v = self.selected_list();
        if self.doc().is_none() || !self.can_ungroup() {
            return;
        }
        self.commit_text();
        self.dm().checkpoint();
        let mut out: Vec<Arc<DrawObj>> = Vec::new();
        let mut new_sel = Vec::new();
        for (i, o) in self.d().st.objs.iter().enumerate() {
            let sel = v.contains(&i);
            if sel && o.kind == ObjType::Group {
                for c in &o.children {
                    new_sel.push(out.len());
                    out.push(Arc::new(c.clone()));
                }
            } else {
                if sel {
                    new_sel.push(out.len());
                }
                out.push(o.clone());
            }
        }
        self.dm().st.objs = out;
        self.dirty = true;
        self.select_objects(&new_sel);
        self.refresh(true);
        self.doc_changed();
    }

    fn hit_handle(&self, p: P) -> i32 {
        if self.doc().is_none() || self.selected < 0 || self.selected as usize >= self.nobjs() || !self.extra.is_empty() {
            return -1;
        }
        let o = self.obj(self.selected as usize);
        if o.locked {
            return -1;
        }
        let hs = self.handle_points(o);
        let tol = self.handle_size() + 2;
        for i in (0..hs.len()).rev() {
            if (p.0 - hs[i].0).abs() <= tol && (p.1 - hs[i].1).abs() <= tol {
                return i as i32;
            }
        }
        -1
    }

    pub fn hit_object(&self, p: PointF) -> i32 {
        let Some(d) = self.doc() else { return -1 };
        let tol = (4.0 / self.zoom) as f32;
        for i in (0..d.st.objs.len()).rev() {
            let o = &d.st.objs[i];
            if o.hidden || (o.kind == ObjType::Link && self.tool != Tool::Select) {
                continue;
            }
            if o.hit_test(p, tol) {
                return i as i32;
            }
        }
        -1
    }

    // ---------------------------------------------------------------- object management (layers panel, lock)
    pub fn selection_locked(&self) -> bool {
        self.selected_list().iter().any(|&i| self.obj(i).locked)
    }

    pub fn set_locked(&mut self, idx: &[usize], lock: bool) {
        if self.doc().is_none() || idx.is_empty() {
            return;
        }
        self.commit_text();
        self.dm().checkpoint();
        for &i in idx {
            if let Some(o) = self.dm().obj_mut(i) {
                o.locked = lock;
            }
        }
        self.refresh(true);
        self.doc_changed();
        self.sel_changed();
    }

    pub fn set_hidden(&mut self, idx: &[usize], hidden: bool) {
        if self.doc().is_none() || idx.is_empty() {
            return;
        }
        self.commit_text();
        self.dm().checkpoint();
        for &i in idx {
            if let Some(o) = self.dm().obj_mut(i) {
                o.hidden = hidden;
            }
        }
        if hidden {
            // a hidden object leaves the selection
            let v: Vec<usize> = self.selected_list().into_iter().filter(|&i| !self.obj(i).hidden).collect();
            self.extra.clear();
            self.selected = -1;
            for i in v {
                if self.selected >= 0 {
                    self.extra.push(self.selected);
                }
                self.selected = i as i32;
            }
        }
        self.refresh(true);
        self.doc_changed();
        self.sel_changed();
    }

    pub fn rename_object(&mut self, idx: usize, name: &str) {
        if idx >= self.nobjs() || self.obj(idx).name == name {
            return;
        }
        self.dm().checkpoint();
        if let Some(o) = self.dm().obj_mut(idx) {
            o.name = name.to_owned();
        }
        self.doc_changed();
    }

    /// Reorder (layers panel): the objects keep their order among themselves and go just below `before`.
    pub fn move_objects(&mut self, idx: &[usize], before: usize) {
        if self.doc().is_none() || idx.is_empty() {
            return;
        }
        let objs = self.d().st.objs.clone();
        let sel: Vec<bool> = (0..objs.len()).map(|i| idx.contains(&i)).collect();
        let primary = (self.selected >= 0).then(|| objs.get(self.selected as usize).cloned()).flatten();
        let (mut moving, mut rest) = (Vec::new(), Vec::new());
        let mut insert_at = 0;
        for (i, o) in objs.iter().enumerate() {
            if sel[i] {
                moving.push(o.clone());
            } else {
                if i < before {
                    insert_at += 1;
                }
                rest.push(o.clone());
            }
        }
        let mut out: Vec<Arc<DrawObj>> = rest[..insert_at].to_vec();
        out.extend(moving.iter().cloned());
        out.extend(rest[insert_at..].iter().cloned());
        if out.iter().zip(objs.iter()).all(|(a, b)| Arc::ptr_eq(a, b)) {
            return;
        }
        self.commit_text();
        self.dm().checkpoint();
        self.dm().st.objs = out.clone();
        self.extra.clear();
        self.selected = -1;
        for (i, o) in out.iter().enumerate() {
            if !moving.iter().any(|m| Arc::ptr_eq(m, o)) {
                continue;
            }
            let is_primary = primary.as_ref().is_some_and(|p| Arc::ptr_eq(p, o));
            let cur_is_primary =
                self.selected >= 0 && primary.as_ref().is_some_and(|p| Arc::ptr_eq(p, &out[self.selected as usize]));
            if is_primary {
                if self.selected >= 0 {
                    self.extra.push(self.selected);
                }
                self.selected = i as i32;
            } else if cur_is_primary {
                self.extra.push(i as i32);
            } else {
                if self.selected >= 0 {
                    self.extra.push(self.selected);
                }
                self.selected = i as i32;
            }
        }
        self.refresh(true);
        self.doc_changed();
        self.sel_changed();
    }

    fn lift_piece(&mut self) -> DrawObj {
        let b = self.sel_bounds;
        let mask = std::mem::take(&mut self.sel_mask);
        let fill = self.sel_fill;
        let base = self.dm().base_mut();
        let mut piece = base.crop(b);
        for y in 0..piece.hi() {
            for x in 0..piece.wi() {
                if mask[((b.top + y) * base.wi() + b.left + x) as usize] == 0 {
                    *piece.at_mut(x, y) = 0;
                }
            }
        }
        fill_hole(base, &mask, fill);
        self.sel_mask = mask;
        let mut o = DrawObj::new(ObjType::Picture);
        o.picture = Some(piece);
        o.style.width = 0.0;
        o.style.shadow = false;
        o.p1 = pt(b.left as f32, b.top as f32);
        o.p2 = pt(b.right as f32, b.bottom as f32);
        o
    }

    /// Lift out: the raster selection becomes a floating image (the hole is filled as in Smart Move); it is put back
    /// with "Drop piece", which merges it.
    pub fn lift_selection(&mut self) -> bool {
        if !self.has_image() || !self.has_sel {
            return false;
        }
        self.commit_text();
        self.dm().checkpoint();
        let mut o = self.lift_piece();
        o.floating = true;
        self.dm().st.objs.push(Arc::new(o));
        self.clear_raster_selection();
        self.selected = self.nobjs() as i32 - 1;
        self.extra.clear();
        self.refresh(true);
        self.doc_changed();
        self.sel_changed();
        true
    }

    pub fn has_floating(&self) -> bool {
        self.doc().is_some_and(|d| d.st.objs.iter().any(|o| o.floating))
    }

    /// Merges these objects into the base image and removes them.
    fn flatten_indices(&mut self, v: &[usize], skip_hidden: bool) {
        let objs: Vec<DrawObj> =
            v.iter().map(|&i| self.obj(i).clone()).filter(|o| !(skip_hidden && o.hidden)).map(|mut o| {
                o.hidden = false;
                o
            }).collect();
        let base = (*self.d().st.base).clone();
        let merged = render::render(&base, &objs, None);
        self.dm().set_base(merged);
        let objs = &mut self.dm().st.objs;
        for &i in v.iter().rev() {
            objs.remove(i);
        }
    }

    pub fn drop_floating(&mut self) -> bool {
        if !self.has_floating() {
            return false;
        }
        self.commit_text();
        let v: Vec<usize> = (0..self.nobjs()).filter(|&i| self.obj(i).floating).collect();
        self.dm().checkpoint();
        self.flatten_indices(&v, true);
        self.selected = -1;
        self.extra.clear();
        self.refresh(true);
        self.doc_changed();
        self.sel_changed();
        true
    }

    // ---------------------------------------------------------------- exact geometry (X, Y, width, height)
    /// 0 nothing, 1 object, 2 raster selection, 3 crop area
    pub fn geometry_kind(&self) -> i32 {
        if !self.has_image() {
            return 0;
        }
        if self.tool == Tool::Crop {
            return 3;
        }
        if (self.has_sel || self.drag == Drag::Select) && (self.tool == Tool::Selection || self.tool == Tool::Select) {
            return 2;
        }
        if self.selected >= 0 && (self.selected as usize) < self.nobjs() && self.selection_count() == 1 {
            return 1;
        }
        0
    }

    pub fn geometry(&self) -> RectF {
        match self.geometry_kind() {
            1 => geom_rect(self.obj(self.selected as usize)),
            2 => {
                if self.drag == Drag::Select {
                    return self.sel_rect;
                }
                let b = self.sel_bounds;
                rect(b.left as f32, b.top as f32, b.width() as f32, b.height() as f32)
            }
            3 => {
                if self.has_crop {
                    self.crop
                } else {
                    rect(0.0, 0.0, self.d().w() as f32, self.d().h() as f32)
                }
            }
            _ => RectF::default(),
        }
    }

    pub fn set_geometry(&mut self, mut r: RectF, new_step: bool) {
        let kind = self.geometry_kind();
        if kind == 0 {
            return;
        }
        r.Width = r.Width.max(1.0);
        r.Height = r.Height.max(1.0);
        let (dw, dh) = (self.d().w(), self.d().h());
        if kind == 1 {
            let sel = self.selected as usize;
            if self.obj(sel).locked {
                return;
            }
            let from = geom_rect(self.obj(sel));
            if new_step {
                self.dm().checkpoint();
            }
            let o = self.dm().obj_mut(sel).unwrap();
            if o.kind == ObjType::Step {
                o.move_by(r.X - from.X, r.Y - from.Y);
            } else if o.kind == ObjType::Callout || o.kind == ObjType::Magnify {
                // the tip stays where it points
                let tail = o.tail;
                o.p1 = pt(r.X, r.Y);
                o.p2 = pt(rf_right(&r), rf_bottom(&r));
                o.tail = tail;
            } else if o.is_line() {
                // keeps the direction of the line inside the new box
                let map = |q: PointF| {
                    let u = if from.Width > 0.01 { (q.X - from.X) / from.Width } else { 0.0 };
                    let v = if from.Height > 0.01 { (q.Y - from.Y) / from.Height } else { 0.0 };
                    pt(r.X + u * r.Width, r.Y + v * r.Height)
                };
                o.p1 = map(o.p1);
                o.p2 = map(o.p2);
                o.ctrl = map(o.ctrl);
            } else {
                o.scale(from, r);
                o.update_group_rect();
            }
            self.dirty = true;
        } else if kind == 2 {
            let mut rr = Rect {
                left: r.X.round() as i32,
                top: r.Y.round() as i32,
                right: rf_right(&r).round() as i32,
                bottom: rf_bottom(&r).round() as i32,
            };
            rr.left = rr.left.clamp(0, dw - 1);
            rr.top = rr.top.clamp(0, dh - 1);
            rr.right = rr.right.clamp(rr.left + 1, dw);
            rr.bottom = rr.bottom.clamp(rr.top + 1, dh);
            let m = if self.sel_mode == 1 { cv::ellipse_mask(dw, dh, rr) } else { cv::rect_mask(dw, dh, rr) };
            self.set_selection_from_mask(m);
        } else {
            let x0 = r.X.round().clamp(0.0, dw as f32 - 1.0);
            let y0 = r.Y.round().clamp(0.0, dh as f32 - 1.0);
            let x1 = rf_right(&r).round().clamp(x0 + 1.0, dw as f32);
            let y1 = rf_bottom(&r).round().clamp(y0 + 1.0, dh as f32);
            self.crop = rect(x0, y0, x1 - x0, y1 - y0);
            self.has_crop = true;
        }
        self.refresh(kind == 1);
        if kind == 1 {
            self.doc_changed();
        }
    }

    // ---------------------------------------------------------------- paste
    pub fn selected_objects(&self) -> Vec<DrawObj> {
        self.selected_list().iter().map(|&i| self.obj(i).clone()).collect()
    }

    /// Objects copied in the Editor: in place keeps the coordinates; the normal paste, in the same image, shifts them
    /// a little so the copy is visible.
    pub fn paste_objects(&mut self, objs: &[DrawObj], in_place: bool, same_doc: bool) {
        if !self.has_image() || objs.is_empty() {
            return;
        }
        self.commit_text();
        self.dm().checkpoint();
        let off = if !in_place && same_doc { 16.0 } else { 0.0 };
        let mut idx = Vec::new();
        for src in objs {
            let mut o = src.clone();
            o.locked = false;
            o.hidden = false;
            if off != 0.0 {
                o.move_by(off, off);
            }
            self.dm().st.objs.push(Arc::new(o));
            idx.push(self.nobjs() - 1);
        }
        if self.tool != Tool::Select {
            self.set_tool(Tool::Select);
        }
        self.dirty = true;
        self.select_objects(&idx);
        self.refresh(true);
        self.doc_changed();
    }

    pub fn paste_image(&mut self, img: &Image) {
        if self.doc().is_none() || img.is_empty() {
            return;
        }
        self.commit_text();
        self.dm().checkpoint();
        let mut o = DrawObj::new(ObjType::Picture);
        o.picture = Some(img.clone());
        o.style.width = 0.0;
        o.style.shadow = false;
        let x = ((self.d().w() - img.wi()) as f32 / 2.0).max(0.0);
        let y = ((self.d().h() - img.hi()) as f32 / 2.0).max(0.0);
        o.p1 = pt(x, y);
        o.p2 = pt(x + img.w as f32, y + img.h as f32);
        self.dm().st.objs.push(Arc::new(o));
        self.set_tool(Tool::Select);
        self.selected = self.nobjs() as i32 - 1;
        self.extra.clear();
        self.refresh(true);
        self.doc_changed();
        self.sel_changed();
    }

    pub fn paste_image_at(&mut self, img: &Image, at: PointF) {
        if self.doc().is_none() || img.is_empty() {
            return;
        }
        self.paste_image(img);
        if self.selected < 0 {
            return;
        }
        let sel = self.selected as usize;
        let o = self.dm().obj_mut(sel).unwrap();
        let (dx, dy) = (at.X - o.p1.X.min(o.p2.X), at.Y - o.p1.Y.min(o.p2.Y));
        o.move_by(dx, dy);
        self.refresh(true);
        self.doc_changed();
    }

    /// Clipboard text: goes into the text being edited or becomes a Text object in the center of the view; `format`
    /// (the source formatting, when it is kept) replaces the font, size, color and style of the text.
    pub fn paste_text(&mut self, text: &str, format: Option<&Style>) {
        if !self.has_image() || text.is_empty() {
            return;
        }
        let t: String = text.chars().filter(|c| *c != '\r').collect();
        let apply = |st: &mut Style, f: &Style| {
            st.font = f.font.clone();
            st.font_size = f.font_size;
            st.text_color = f.text_color;
            st.bold = f.bold;
            st.italic = f.italic;
            st.underline = f.underline;
        };
        if let Some(e) = &mut self.edit {
            e.text.push_str(&t);
            let idx = e.idx;
            if let Some(f) = format
                && let Some(o) = self.dm().obj_mut(idx)
            {
                apply(&mut o.style, f);
                self.dirty = true;
            }
            return;
        }
        self.commit_text();
        let mut st = self.style_of(Tool::Text).clone();
        if let Some(f) = format {
            apply(&mut st, f);
        }
        let mut o = DrawObj::new(ObjType::Text);
        o.style = st.clone();
        o.text = t.encode_utf16().collect();
        let (l, tp, r, b) = self.view();
        let mut c = self.to_doc(((l + r) / 2, (tp + b) / 2));
        let (dw, dh) = (self.d().w() as f32, self.d().h() as f32);
        c.X = c.X.clamp(0.0, dw);
        c.Y = c.Y.clamp(0.0, dh);
        let w = (200.0f32).max(dw * 0.5).min((dw - 20.0).max(60.0));
        o.p1 = pt(c.X - w / 2.0, c.Y);
        o.p2 = pt(c.X + w / 2.0, c.Y + 10.0);
        let hgt = render::text_block_height(&o, w - 2.0 * st.padding) + 2.0 * st.padding;
        o.p1.Y = c.Y - hgt / 2.0;
        o.p2.Y = c.Y + hgt / 2.0;
        self.dm().checkpoint();
        self.dm().st.objs.push(Arc::new(o));
        if self.tool != Tool::Select {
            self.set_tool(Tool::Select);
        }
        self.selected = self.nobjs() as i32 - 1;
        self.extra.clear();
        self.refresh(true);
        self.doc_changed();
        self.sel_changed();
    }

    // ---------------------------------------------------------------- raster selection
    pub fn has_raster_selection(&self) -> bool {
        self.has_sel
    }

    pub fn has_crop(&self) -> bool {
        self.has_crop
    }

    pub fn raster_selection_bounds(&self) -> Rect {
        self.sel_bounds
    }

    pub fn clear_raster_selection(&mut self) {
        if !self.has_sel {
            return;
        }
        self.has_sel = false;
        self.sel_mask.clear();
        self.sel_overlay = Image::default();
        self.sel_version += 1;
        self.note(Note::AreaChanged);
    }

    fn set_selection_from_mask(&mut self, m: Mask) {
        let (w, h) = (self.d().w(), self.d().h());
        let b = cv::mask_bounds(&m, w, h);
        if b.right <= b.left {
            self.has_sel = false;
            self.note(Note::AreaChanged);
            return;
        }
        self.sel_overlay = make_overlay(&m, w, b);
        self.sel_mask = m;
        self.sel_bounds = b;
        self.has_sel = true;
        self.sel_version += 1;
        self.note(Note::AreaChanged);
    }

    pub fn select_all(&mut self) {
        if !self.has_image() {
            return;
        }
        if self.tool != Tool::Selection {
            self.set_tool(Tool::Selection);
        }
        let n = (self.d().w() * self.d().h()) as usize;
        self.set_selection_from_mask(vec![255; n]);
    }

    /// The image of the raster selection (or of the selected objects, or everything).
    pub fn selection_image(&mut self) -> Image {
        if !self.has_image() {
            return Image::default();
        }
        let comp = self.d().render(None);
        if self.has_sel {
            let b = self.sel_bounds;
            let mut out = comp.crop(b);
            let w = self.d().w();
            for y in 0..out.hi() {
                for x in 0..out.wi() {
                    if self.sel_mask[((b.top + y) * w + b.left + x) as usize] == 0 {
                        *out.at_mut(x, y) = 0;
                    }
                }
            }
            return out;
        }
        let v = self.selected_list();
        if let Some(&first) = v.first() {
            let mut b = self.obj(first).bounds();
            for &i in &v {
                b = d::union(&b, &self.obj(i).bounds());
            }
            return comp.crop(Rect {
                left: b.X as i32,
                top: b.Y as i32,
                right: rf_right(&b).ceil() as i32,
                bottom: rf_bottom(&b).ceil() as i32,
            });
        }
        comp
    }

    pub fn copy_selection(&mut self, cut: bool) -> bool {
        let img = self.selection_image();
        if img.is_empty() {
            return false;
        }
        snapcore::clip::copy_image(self.hwnd, &img, crate::VERSION);
        if cut {
            self.delete_selected();
        }
        true
    }

    pub fn delete_selected(&mut self) {
        if self.doc().is_none() {
            return;
        }
        if self.has_sel {
            self.dm().checkpoint();
            let mask = std::mem::take(&mut self.sel_mask);
            let fill = self.sel_fill;
            fill_hole(self.dm().base_mut(), &mask, fill);
            self.sel_mask = mask;
            self.clear_raster_selection();
            self.refresh(true);
            self.doc_changed();
            return;
        }
        let all = self.selected_list();
        let v: Vec<usize> = all.iter().copied().filter(|&i| !self.obj(i).locked).collect();
        if v.len() < all.len() {
            self.note(Note::Hint(tr(
                "Objetos bloqueados não são excluídos (desbloqueie pelo menu de contexto ou pelo painel Camadas).",
            )));
        }
        if !v.is_empty() {
            self.dm().checkpoint();
            for &i in v.iter().rev() {
                self.dm().st.objs.remove(i);
            }
            self.selected = -1;
            self.extra.clear();
            self.refresh(true);
            self.doc_changed();
            self.sel_changed();
        }
    }

    pub fn crop_to_selection(&mut self) {
        if self.doc().is_none() {
            return;
        }
        let b = if self.has_sel {
            self.sel_bounds
        } else if self.has_crop {
            let c = self.crop;
            Rect { left: c.X as i32, top: c.Y as i32, right: rf_right(&c) as i32, bottom: rf_bottom(&c) as i32 }
        } else {
            return;
        };
        if b.width() < 1 || b.height() < 1 {
            return;
        }
        self.dm().checkpoint();
        let img = self.d().st.base.crop(b);
        self.dm().set_base(img);
        for i in 0..self.nobjs() {
            self.dm().obj_mut(i).unwrap().move_by(-b.left as f32, -b.top as f32);
        }
        self.clear_raster_selection();
        self.has_crop = false;
        self.selected = -1;
        self.dirty = true;
        self.fit_to_window();
        self.refresh(true);
        self.doc_changed();
    }

    pub fn apply_crop(&mut self) {
        if self.has_crop {
            self.crop_to_selection();
        }
    }

    // ---------------------------------------------------------------- order, alignment, flips
    fn reorder(&mut self, mode: i32) {
        let v = self.selected_list();
        if self.doc().is_none() || v.is_empty() {
            return;
        }
        self.dm().checkpoint();
        let mut objs = self.d().st.objs.clone();
        let primary = (self.selected >= 0).then(|| objs[self.selected as usize].clone());
        let mut sel: Vec<bool> = (0..objs.len()).map(|i| v.contains(&i)).collect();
        if mode == 0 || mode == 3 {
            // 0 bring to front, 3 send to back
            let (a, b): (Vec<_>, Vec<_>) = objs.iter().cloned().enumerate().partition(|(i, _)| sel[*i]);
            let a: Vec<_> = a.into_iter().map(|x| x.1).collect();
            let b: Vec<_> = b.into_iter().map(|x| x.1).collect();
            let first = if mode == 0 { b.len() } else { 0 };
            let na = a.len();
            objs = if mode == 0 { b.into_iter().chain(a).collect() } else { a.into_iter().chain(b).collect() };
            sel = (0..objs.len()).map(|i| i >= first && i < first + na).collect();
        } else if mode == 1 {
            // one step forward
            for i in (0..objs.len().saturating_sub(1)).rev() {
                if sel[i] && !sel[i + 1] {
                    objs.swap(i, i + 1);
                    sel.swap(i, i + 1);
                }
            }
        } else {
            // one step backward
            for i in 1..objs.len() {
                if sel[i] && !sel[i - 1] {
                    objs.swap(i, i - 1);
                    sel.swap(i, i - 1);
                }
            }
        }
        self.extra.clear();
        self.selected = -1;
        for i in 0..objs.len() {
            if !sel[i] {
                continue;
            }
            if primary.as_ref().is_some_and(|p| Arc::ptr_eq(p, &objs[i])) {
                self.selected = i as i32;
            } else {
                self.extra.push(i as i32);
            }
        }
        if self.selected < 0
            && let Some(l) = self.extra.pop()
        {
            self.selected = l;
        }
        self.dm().st.objs = objs;
        self.refresh(true);
        self.doc_changed();
    }

    pub fn bring_to_front(&mut self) {
        self.reorder(0);
    }

    pub fn send_to_back(&mut self) {
        self.reorder(3);
    }

    pub fn bring_forward(&mut self) {
        self.reorder(1);
    }

    pub fn send_backward(&mut self) {
        self.reorder(2);
    }

    fn unlocked_selection(&self) -> Vec<usize> {
        self.selected_list().into_iter().filter(|&i| !self.obj(i).locked).collect()
    }

    /// Align: one object, relative to the image; several, among themselves (by the area they cover together).
    /// 0 left, 1 center, 2 right, 3 top, 4 middle, 5 bottom.
    pub fn align_selected(&mut self, mode: i32) {
        let v = self.unlocked_selection();
        if v.is_empty() {
            return;
        }
        let mut refr = rect(0.0, 0.0, self.d().w() as f32, self.d().h() as f32);
        if v.len() > 1 {
            refr = obj_extent(self.obj(v[0]));
            for &i in &v {
                refr = d::union(&refr, &obj_extent(self.obj(i)));
            }
        }
        let mut moves = Vec::new();
        for &i in &v {
            let r = obj_extent(self.obj(i));
            let (mut dx, mut dy) = (0.0, 0.0);
            match mode {
                0 => dx = refr.X - r.X,
                1 => dx = refr.X + (refr.Width - r.Width) / 2.0 - r.X,
                2 => dx = rf_right(&refr) - rf_right(&r),
                3 => dy = refr.Y - r.Y,
                4 => dy = refr.Y + (refr.Height - r.Height) / 2.0 - r.Y,
                5 => dy = rf_bottom(&refr) - rf_bottom(&r),
                _ => {}
            }
            if dx.abs() > 0.01 || dy.abs() > 0.01 {
                moves.push((i, dx, dy));
            }
        }
        if moves.is_empty() {
            return;
        }
        self.dm().checkpoint();
        for (i, dx, dy) in moves {
            self.dm().obj_mut(i).unwrap().move_by(dx, dy);
        }
        self.refresh(true);
        self.doc_changed();
    }

    /// Distribute (as in PowerPoint): the first and last objects stay; the ones between are spread so the gaps are
    /// equal.
    pub fn distribute_selected(&mut self, horizontal: bool) {
        let v = if self.doc().is_some() { self.unlocked_selection() } else { Vec::new() };
        if v.len() < 3 {
            self.note(Note::Hint(tr("Selecione 3 ou mais objetos para distribuir.")));
            return;
        }
        let start = |r: &RectF| if horizontal { r.X } else { r.Y };
        let size = |r: &RectF| if horizontal { r.Width } else { r.Height };
        let mut items: Vec<(usize, RectF)> = v.iter().map(|&i| (i, obj_extent(self.obj(i)))).collect();
        items.sort_by(|a, b| (start(&a.1) + size(&a.1) / 2.0).partial_cmp(&(start(&b.1) + size(&b.1) / 2.0)).unwrap());
        let first = start(&items[0].1);
        let last = start(&items[items.len() - 1].1) + size(&items[items.len() - 1].1);
        let sum: f32 = items.iter().map(|it| size(&it.1)).sum();
        let gap = (last - first - sum) / (items.len() - 1) as f32;
        let mut pos = first + size(&items[0].1) + gap;
        let mut moves = Vec::new();
        for it in &items[1..items.len() - 1] {
            let dd = pos - start(&it.1);
            if dd.abs() > 0.01 {
                moves.push((it.0, dd));
            }
            pos += size(&it.1) + gap;
        }
        if moves.is_empty() {
            return;
        }
        self.dm().checkpoint();
        for (i, dd) in moves {
            self.dm().obj_mut(i).unwrap().move_by(if horizontal { dd } else { 0.0 }, if horizontal { 0.0 } else { dd });
        }
        self.refresh(true);
        self.doc_changed();
    }

    /// Flips the selected objects; false when there is none.
    pub fn flip_selected(&mut self, horizontal: bool) -> bool {
        let v = if self.doc().is_some() { self.unlocked_selection() } else { Vec::new() };
        if v.is_empty() {
            return false;
        }
        let mut u = obj_extent(self.obj(v[0]));
        for &i in &v {
            u = d::union(&u, &obj_extent(self.obj(i)));
        }
        let (cx, cy) = (u.X + u.Width / 2.0, u.Y + u.Height / 2.0);
        self.dm().checkpoint();
        fn flip_pics(o: &mut DrawObj, horizontal: bool) {
            if let Some(p) = &o.picture {
                o.picture = Some(if horizontal { snapcore::fx::flip_h(p) } else { snapcore::fx::flip_v(p) });
            }
            for c in &mut o.children {
                flip_pics(c, horizontal);
            }
        }
        for i in v {
            let o = self.dm().obj_mut(i).unwrap();
            o.map_points(&|p: PointF| if horizontal { pt(2.0 * cx - p.X, p.Y) } else { pt(p.X, 2.0 * cy - p.Y) });
            flip_pics(o, horizontal);
            o.update_group_rect();
        }
        self.refresh(true);
        self.doc_changed();
        true
    }

    /// Flattens only the selected objects into the image.
    pub fn flatten_selected(&mut self) {
        let v = self.selected_list();
        if v.is_empty() {
            return;
        }
        self.commit_text();
        self.dm().checkpoint();
        self.flatten_indices(&v, false);
        self.selected = -1;
        self.extra.clear();
        self.refresh(true);
        self.doc_changed();
        self.sel_changed();
    }

    pub fn flatten_all(&mut self) {
        if self.doc().is_none_or(|d| d.st.objs.is_empty()) {
            return;
        }
        self.commit_text();
        self.dm().checkpoint();
        let img = self.d().render(None);
        self.dm().set_base(img);
        self.dm().st.objs.clear();
        self.selected = -1;
        self.refresh(true);
        self.doc_changed();
        self.sel_changed();
    }

    /// The style of the properties panel goes to the selected objects (`new_step` false merges with the previous edit).
    pub fn apply_style_to_selected(&mut self, s: &Style, new_step: bool) {
        if self.selected < 0 || self.selected as usize >= self.nobjs() {
            return;
        }
        if new_step {
            self.dm().checkpoint();
        }
        let sel = self.selected as usize;
        let fam = family(self.obj(sel));
        for i in self.selected_list() {
            // several selected: the style applies to those of the same kind as the primary
            if i == sel || family(self.obj(i)) != fam || self.obj(i).kind == ObjType::Group {
                continue;
            }
            let x = self.dm().obj_mut(i).unwrap();
            let old_area = x.style.area_shape;
            x.style = s.clone();
            adapt_area_points(x, old_area);
            if x.is_shape() {
                x.kind = ObjType::Shape;
            }
            if x.kind == ObjType::Blur || x.kind == ObjType::Pixelate {
                x.kind = if s.hide_kind == 1 { ObjType::Pixelate } else { ObjType::Blur };
            }
        }
        if self.obj(sel).kind == ObjType::Group {
            self.refresh(true);
            self.doc_changed();
            return;
        }
        let o = self.dm().obj_mut(sel).unwrap();
        let old_area = o.style.area_shape;
        let became_vertical = !o.style.vertical && s.vertical && o.has_text();
        o.style = s.clone();
        if became_vertical {
            // vertical text (C37): the box turns into a tall column
            let r = o.rect();
            let col_w = (s.font_size * 1.6 + 2.0 * s.padding).max(24.0);
            let col_h = r.Height.max(r.Width).max(s.font_size * 1.25 * o.text.len().max(2) as f32 + 2.0 * s.padding);
            let lines = 1 + o.text.iter().filter(|c| **c == b'\n' as u16).count();
            o.p1 = pt(r.X, r.Y);
            o.p2 = pt(r.X + col_w * lines as f32, r.Y + col_h);
        }
        adapt_area_points(o, old_area);
        if o.is_shape() {
            o.kind = ObjType::Shape;
        }
        if o.kind == ObjType::Blur || o.kind == ObjType::Pixelate {
            o.kind = if s.hide_kind == 1 { ObjType::Pixelate } else { ObjType::Blur };
        }
        if o.kind == ObjType::Magnify && s.magnify_link && o.tail.X == 0.0 && o.tail.Y == 0.0 {
            let r = o.rect();
            o.tail = pt(r.X - r.Width * 0.4, rf_bottom(&r) + r.Height * 0.3);
        }
        if o.kind == ObjType::Callout && o.callout_has_tail() && d::contains(&o.rect(), o.tail) {
            let r = o.rect();
            o.tail = pt(r.X + r.Width * 0.25, rf_bottom(&r) + (20.0f32).max(r.Height * 0.45));
        }
        let list = self.selected_list();
        self.refresh_text_boxes(&list);
        self.refresh(true);
        self.doc_changed();
    }

    pub fn set_selected_stamp(&mut self, id: &str) {
        self.stamp_id = id.to_owned();
        if self.selected < 0 || self.selected as usize >= self.nobjs() {
            return;
        }
        let sel = self.selected as usize;
        if self.obj(sel).kind != ObjType::Stamp || obj_text(self.obj(sel)) == id {
            return;
        }
        self.dm().checkpoint();
        self.dm().obj_mut(sel).unwrap().text = id.encode_utf16().collect();
        self.refresh(true);
        self.doc_changed();
    }

    pub fn nudge(&mut self, dx: i32, dy: i32) {
        let v = if self.doc().is_some() { self.unlocked_selection() } else { Vec::new() };
        if v.is_empty() {
            return;
        }
        self.dm().checkpoint();
        for &i in &v {
            self.dm().obj_mut(i).unwrap().move_by(dx as f32, dy as f32);
        }
        self.refresh_text_boxes(&v);
        self.refresh(true);
        self.doc_changed();
    }

    /// Live preview of an effect: shows the image in place of the document until it is applied or canceled.
    pub fn set_effect_preview(&mut self, img: Option<Arc<Image>>) {
        self.fx_preview = img;
    }

    pub fn has_effect_preview(&self) -> bool {
        self.fx_preview.is_some()
    }

    // ---------------------------------------------------------------- text under the areas (C12)
    fn ocr_text(&mut self) -> &[snapcore::ocr::Line] {
        let Some(img) = self.doc().map(|d| d.st.base.clone()) else {
            self.ocr_lines.clear();
            return &self.ocr_lines;
        };
        let base = Arc::as_ptr(&img);
        if self.ocr_base != Some(base) || !self.ocr_tried {
            self.ocr_base = Some(base);
            self.ocr_tried = true;
            match snapcore::ocr::run_ocr_lines(&img) {
                Ok(l) => self.ocr_lines = l,
                Err(e) => {
                    self.ocr_lines.clear();
                    let m = tr("OCR indisponível: ") + &e.message();
                    self.note(Note::Hint(m));
                }
            }
        }
        &self.ocr_lines
    }

    fn update_text_boxes(&mut self, o: &mut DrawObj) {
        o.text_boxes.clear();
        if !o.style.text_only || !o.is_area_tool() {
            return;
        }
        let area = if o.is_point_shape() { o.bounds() } else { o.rect() };
        o.text_boxes = tools::words_in_area(self.ocr_text(), area);
        if o.text_boxes.is_empty() {
            self.note(Note::Hint(tr("Nenhum texto reconhecido nesta área (\"Somente o texto\" ligado).")));
        }
    }

    /// Text-only areas (C12): recomputes the word boxes of the objects that have "text only" on.
    pub fn refresh_text_boxes(&mut self, idx: &[usize]) {
        for &i in idx {
            if i >= self.nobjs() {
                continue;
            }
            let o = self.obj(i);
            if !o.is_area_tool() || (!o.style.text_only && o.text_boxes.is_empty()) {
                continue;
            }
            let mut c = o.clone();
            self.update_text_boxes(&mut c);
            *self.dm().obj_mut(i).unwrap() = c;
        }
    }

    /// Smart highlighter: snaps to the text lines (one rectangle or a group).
    fn snap_highlighter(&mut self, o: &mut DrawObj) -> bool {
        if o.style.area_shape == AREA_POLYGON {
            return false;
        }
        let area = if o.is_point_shape() || o.is_path() { o.bounds() } else { o.rect() };
        let rects = tools::snap_to_text_lines(self.ocr_text(), area);
        if rects.is_empty() {
            self.note(Note::Hint(tr("Nenhuma linha de texto reconhecida aqui: o marca-texto ficou como foi desenhado.")));
            return false;
        }
        let piece = |r: &RectF| {
            let mut h = o.clone();
            h.pts.clear();
            h.style.area_shape = if o.style.area_shape == SHAPE_ROUND_RECT { SHAPE_ROUND_RECT } else { SHAPE_RECT };
            h.p1 = pt(r.X, r.Y);
            h.p2 = pt(rf_right(r), rf_bottom(r));
            h
        };
        if rects.len() == 1 {
            *o = piece(&rects[0]);
            return true;
        }
        let mut g = DrawObj::new(ObjType::Group);
        g.style = o.style.clone();
        g.children = rects.iter().map(piece).collect();
        g.update_group_rect();
        *o = g;
        true
    }

    // ---------------------------------------------------------------- draw & hold (C23)
    pub fn recognize_hold(&mut self) {
        self.hold_at = None;
        if self.drag != Drag::Create || self.hold_shape || !self.draw_hold {
            return;
        }
        let Some(p) = &self.preview else { return };
        if p.kind != ObjType::Pen {
            return;
        }
        let (k, a, b) = tools::recognize_stroke(&p.pts);
        if k == tools::REC_NONE {
            return;
        }
        let mut o = DrawObj::default();
        o.style = p.style.clone();
        if k == tools::REC_LINE || k == tools::REC_ARROW {
            o.kind = if k == tools::REC_ARROW { ObjType::Arrow } else { ObjType::Line };
            let base = self.style_of(if k == tools::REC_ARROW { Tool::Arrow } else { Tool::Line }).clone();
            o.style.arrow_start = CAP_FLAT;
            o.style.arrow_end = if k == tools::REC_ARROW {
                if base.arrow_end != CAP_FLAT { base.arrow_end } else { CAP_ARROW }
            } else {
                CAP_FLAT
            };
            o.style.start_size = base.start_size;
            o.style.end_size = base.end_size;
            o.style.bezier = false;
            o.style.dash = DASH_SOLID;
            o.p1 = a;
            o.p2 = b;
            o.ctrl = pt((a.X + b.X) / 2.0, (a.Y + b.Y) / 2.0);
        } else {
            o.kind = ObjType::Shape;
            o.style.shape_kind = if k == tools::REC_ELLIPSE {
                SHAPE_ELLIPSE
            } else if k == tools::REC_TRIANGLE {
                SHAPE_TRIANGLE
            } else {
                SHAPE_RECT
            };
            o.style.fill = 0;
            o.style.grad = Gradient::default();
            o.p1 = a;
            o.p2 = b;
        }
        self.preview = Some(o);
        self.preview_version += 1;
        self.hold_shape = true;
        let names = ["", "linha", "seta", "retângulo", "elipse", "triângulo"];
        let m = tr("Forma reconhecida: ") + &tr(names[k as usize]) + &tr(" (Ctrl+Z desfaz; Editar › Desenhar e segurar desliga).");
        self.note(Note::Hint(m));
    }

    // ---------------------------------------------------------------- in-place text
    pub fn editing_text(&self) -> bool {
        self.edit.is_some()
    }

    /// The area where the text of an object is typed, in image coordinates.
    pub fn text_area(&self, o: &DrawObj, text: Option<&str>) -> RectF {
        if o.kind == ObjType::Callout {
            render::callout_text_rect(o)
        } else if o.has_label() {
            let mut tmp = o.clone();
            if let Some(t) = text {
                tmp.text = t.encode_utf16().collect();
            }
            render::arrow_label_rect(&tmp)
        } else {
            let mut a = o.rect();
            d::inflate(&mut a, -o.style.padding, -o.style.padding);
            a
        }
    }

    pub fn begin_text_edit(&mut self, idx: usize, is_new: bool) {
        self.commit_text();
        if idx >= self.nobjs() {
            return;
        }
        let o = self.obj(idx);
        if !o.has_text() && !o.has_label() {
            return;
        }
        let text = obj_text(o);
        if !is_new {
            self.dm().checkpoint();
        }
        self.edit = Some(TextEdit { idx, new: is_new, text, fresh: true });
        self.label_ready = false;
        self.dirty = true;
    }

    /// The text box grows downward when the typed text goes past it.
    pub fn layout_text_edit(&mut self) {
        let Some(e) = &self.edit else { return };
        let (idx, text) = (e.idx, e.text.clone());
        if idx >= self.nobjs() {
            return;
        }
        let o = self.obj(idx);
        if o.kind != ObjType::Text || o.style.vertical {
            return;
        }
        let mut tmp = o.clone();
        tmp.text = if text.is_empty() || text.ends_with('\n') { format!("{text}Ag") } else { text }.encode_utf16().collect();
        let r = o.rect();
        let need = render::text_block_height(&tmp, r.Width - 2.0 * o.style.padding) + 2.0 * o.style.padding;
        if need > r.Height + 0.5 {
            let grow = need - r.Height;
            let w = self.dm().obj_mut(idx).unwrap();
            if w.p2.Y >= w.p1.Y {
                w.p2.Y += grow;
            } else {
                w.p1.Y += grow;
            }
            self.dirty = true;
        }
    }

    pub fn commit_text(&mut self) {
        let Some(e) = self.edit.take() else { return };
        let clean: String = e.text.chars().filter(|c| *c != '\r').collect();
        if e.idx < self.nobjs() {
            let o = self.obj(e.idx);
            if clean.is_empty() && o.kind == ObjType::Text {
                // (an arrow only loses its label)
                self.dm().st.objs.remove(e.idx);
                self.selected = -1;
                self.sel_changed();
            } else if obj_text(o) != clean {
                let w = self.dm().obj_mut(e.idx).unwrap();
                w.text = clean.encode_utf16().collect();
                if w.kind == ObjType::Text {
                    // ensures the height for the whole text (no cut-off lines)
                    let r = w.rect();
                    let need = render::text_block_height(w, r.Width - 2.0 * w.style.padding) + 2.0 * w.style.padding;
                    if r.Height < need {
                        if w.p2.Y >= w.p1.Y {
                            w.p2.Y = w.p1.Y + need;
                        } else {
                            w.p1.Y = w.p2.Y + need;
                        }
                    }
                }
            } else if !e.new {
                self.dm().discard_checkpoint();
            }
        }
        self.dirty = true;
        self.doc_changed();
    }

    /// An arrow was just drawn (still selected, no label): typed keys start its label.
    pub fn wants_typing(&self) -> bool {
        self.label_ready
            && self.edit.is_none()
            && self.selected >= 0
            && (self.selected as usize) < self.nobjs()
            && self.obj(self.selected as usize).has_label()
            && self.obj(self.selected as usize).text.is_empty()
    }

    /// The first typed character of an arrow's label.
    pub fn type_label(&mut self, ch: &str) {
        if !self.wants_typing() {
            return;
        }
        self.begin_text_edit(self.selected as usize, false);
        if let Some(e) = &mut self.edit {
            e.text = ch.to_owned();
            e.fresh = true;
        }
    }

    // ---------------------------------------------------------------- mouse
    fn begin_move(&mut self) {
        self.dm().checkpoint();
        self.drag_multi = self.selected_list().into_iter().filter(|&i| !self.obj(i).locked).map(|i| (i, self.obj(i).clone())).collect();
        if self.selected >= 0 {
            self.drag_orig = self.obj(self.selected as usize).clone();
        }
        self.drag = Drag::Move;
        self.axis_lock = 0;
    }

    /// Lifts the selection's pixels as an object and fills the hole (Smart Move, TSCV.Inpainting).
    fn start_smart_move(&mut self) {
        self.dm().checkpoint();
        let o = self.lift_piece();
        self.dm().st.objs.push(Arc::new(o.clone()));
        self.clear_raster_selection();
        self.selected = self.nobjs() as i32 - 1;
        self.extra.clear();
        self.drag_orig = o.clone();
        self.drag_multi = vec![(self.selected as usize, o)];
        self.drag = Drag::Move;
        self.changed = true;
        self.dirty = true;
        self.sel_changed();
    }

    /// Eraser: removes alpha from the image at the chosen opacity; with smoothing, the brush edge is gradual.
    fn erase_at(&mut self, p: PointF) {
        let s = self.styles[Tool::Eraser.index()].clone();
        let r = (s.width / 2.0).max(1.0);
        let op = s.opacity.clamp(1, 100);
        let base = self.dm().base_mut();
        let (bw, bh) = (base.wi(), base.hi());
        let (y0, y1) = ((p.Y - r - 1.0) as i32, (p.Y + r + 1.0) as i32);
        let (x0, x1) = ((p.X - r - 1.0) as i32, (p.X + r + 1.0) as i32);
        for y in y0..=y1 {
            for x in x0..=x1 {
                if x < 0 || y < 0 || x >= bw || y >= bh {
                    continue;
                }
                let (dx, dy) = (x as f32 + 0.5 - p.X, y as f32 + 0.5 - p.Y);
                let dd = hypot(dx, dy);
                let cover = if s.antialias { (r + 0.5 - dd).clamp(0.0, 1.0) } else if dd <= r { 1.0 } else { 0.0 };
                if cover <= 0.0 {
                    continue;
                }
                let keep = 255 - (255.0 * cover * op as f32 / 100.0) as i32;
                let px = base.at_mut(x, y);
                let na = ca(*px).min(ca(*px) * keep / 255);
                *px = if na != 0 { (*px & 0x00FFFFFF) | (na as u32) << 24 } else { 0 };
            }
        }
    }

    /// Mouse positions can be far apart in a quick stroke: dabs every half radius cover the segment between them.
    fn erase_line(&mut self, from: PointF, to: PointF) {
        let (dx, dy) = (to.X - from.X, to.Y - from.Y);
        let len = hypot(dx, dy);
        let w = self.styles[Tool::Eraser.index()].width;
        let step = ((w / 2.0).max(1.0) / 2.0).max(1.0);
        let n = ((len / step).ceil() as i32).max(1);
        for i in 1..=n {
            self.erase_at(pt(from.X + dx * i as f32 / n as f32, from.Y + dy * i as f32 / n as f32));
        }
    }

    fn constrain45(from: PointF, to: PointF) -> PointF {
        let (dx, dy) = ((to.X - from.X) as f64, (to.Y - from.Y) as f64);
        let q = std::f64::consts::PI / 4.0;
        let ang = (dy.atan2(dx) / q).round() * q;
        let len = dx.hypot(dy);
        pt(from.X + (ang.cos() * len) as f32, from.Y + (ang.sin() * len) as f32)
    }

    /// Polygon (Highlighter, Spotlight and Hide): each click adds a corner; a click on the first corner, a double
    /// click or Enter closes it; Shift snaps the segment to 45°. The last point follows the mouse.
    fn polygon_click(&mut self, p: P, dbl: bool, m: Mods) {
        if dbl {
            self.finish_polygon();
            return;
        }
        let first = self.to_screen(self.preview.as_ref().unwrap().pts[0]);
        if self.preview.as_ref().unwrap().pts.len() >= 4 && (p.0 - first.0).abs() <= 8 && (p.1 - first.1).abs() <= 8 {
            self.finish_polygon();
            return;
        }
        let mut dd = self.to_doc(p);
        let o = self.preview.as_mut().unwrap();
        if m.shift && o.pts.len() >= 2 {
            dd = Self::constrain45(o.pts[o.pts.len() - 2], dd);
        }
        *o.pts.last_mut().unwrap() = dd;
        o.pts.push(dd);
        self.preview_version += 1;
    }

    pub fn finish_polygon(&mut self) {
        if self.drag != Drag::Polygon || self.preview.is_none() {
            return;
        }
        self.drag = Drag::None;
        let zoom = self.zoom as f32;
        let o = self.preview.as_mut().unwrap();
        if o.pts.len() >= 2 {
            o.pts.pop(); // the point that followed the mouse
        }
        let mut clean: Vec<PointF> = Vec::new();
        for q in &o.pts {
            if clean.last().is_none_or(|l| hypot(q.X - l.X, q.Y - l.Y) * zoom > 2.0) {
                clean.push(*q);
            }
        }
        o.pts = clean;
        if o.pts.len() < 3 {
            self.preview = None;
            return;
        }
        fit_to_points(o);
        self.finish_create();
        self.refresh(true);
        self.doc_changed();
    }

    pub fn reset_polygon(&mut self) {
        if self.drag != Drag::Polygon {
            return;
        }
        self.drag = Drag::None;
        self.preview = None;
    }

    pub fn polygon_back(&mut self) -> bool {
        if self.drag != Drag::Polygon {
            return false;
        }
        let Some(o) = &mut self.preview else { return false };
        if o.pts.len() > 2 {
            let n = o.pts.len();
            o.pts.remove(n - 2);
            self.preview_version += 1;
        } else {
            self.reset_polygon();
        }
        true
    }

    fn hit_guide(&self, p: P) -> Option<(i32, usize)> {
        let d = self.doc()?;
        if !self.rulers {
            return None;
        }
        for a in 0..2 {
            let list = if a == 1 { &d.guides_y } else { &d.guides_x };
            for i in (0..list.len()).rev() {
                let s = self.to_screen(if a == 1 { pt(0.0, list[i]) } else { pt(list[i], 0.0) });
                if (if a == 1 { p.1 - s.1 } else { p.0 - s.0 }).abs() <= 3 {
                    return Some((a, i));
                }
            }
        }
        None
    }

    /// Edge snapping by contrast: looks, within a few screen pixels of `pos`, for the column (vertical edge) or row
    /// whose pixels differ most from the previous one along the edge (a0..a1).
    fn snap_edge(&mut self, vertical: bool, pos: f32, a0: f32, a1: f32) -> f32 {
        self.recompose();
        let img = &self.composed;
        if img.is_empty() {
            return pos;
        }
        let (lim, other) = if vertical { (img.wi(), img.hi()) } else { (img.hi(), img.wi()) };
        let win = ((6.0 / self.zoom).round() as i32).max(2);
        let (mut lo, mut hi) = (a0.min(a1).floor() as i32, a0.max(a1).ceil() as i32);
        if hi - lo < 6 {
            lo -= 12;
            hi += 12;
        }
        let lo = lo.clamp(0, other - 1);
        let hi = hi.clamp(lo + 1, other);
        let step_a = ((hi - lo) / 240).max(1);
        let c0 = pos.round() as i32;
        let (mut best, mut best_score) = (pos, 0.0f32);
        for c in (c0 - win).max(1)..=(c0 + win).min(lim - 1) {
            let (mut sum, mut n) = (0i64, 0i32);
            let mut a = lo;
            while a < hi {
                let (p0, p1) = if vertical { (img.at(c - 1, a), img.at(c, a)) } else { (img.at(a, c - 1), img.at(a, c)) };
                sum += (cr(p0) - cr(p1)).abs().max((cg(p0) - cg(p1)).abs()).max((cb(p0) - cb(p1)).abs()).max((ca(p0) - ca(p1)).abs())
                    as i64;
                n += 1;
                a += step_a;
            }
            let score = if n > 0 { sum as f32 / n as f32 - (c - c0).abs() as f32 * 0.8 } else { 0.0 };
            if score > best_score {
                best_score = score;
                best = c as f32;
            }
        }
        if best_score >= 22.0 { best } else { pos }
    }

    fn select_hit(&mut self, hit: usize) {
        self.selected = hit as i32;
        self.extra.clear();
    }

    pub fn on_down(&mut self, p: P, dbl: bool, m: Mods) {
        // clicking outside only commits the text (does not create another object)
        if self.edit.is_some() {
            self.commit_text();
            return;
        }
        if !self.has_image() || self.fx_preview.is_some() {
            return;
        }
        if self.drag == Drag::Polygon && self.preview.is_some() {
            self.polygon_click(p, dbl, m);
            return;
        }
        self.label_ready = false;
        let dd = self.to_doc(p);
        if self.pick_color_once {
            // Fill panel eyedropper
            self.pick_color_once = false;
            self.recompose();
            let (x, y) = (dd.X as i32, dd.Y as i32);
            if x >= 0 && y >= 0 && x < self.composed.wi() && y < self.composed.hi() {
                let c = self.composed.at(x, y) | 0xFF000000;
                self.current_style().stroke = c;
                self.sel_changed();
            }
            return;
        }
        self.down_screen = p;
        self.down_doc = dd;
        self.changed = false;
        self.pending_deselect = -1;
        let rs = self.ruler_size();
        if rs > 0 && (p.0 < rs || p.1 < rs) {
            // dragging out of a ruler creates a guide
            if p.0 < rs && p.1 < rs {
                return;
            }
            self.guide_axis = if p.1 < rs { 1 } else { 0 };
            let axis = self.guide_axis;
            let dm = self.dm();
            let list = if axis == 1 { &mut dm.guides_y } else { &mut dm.guides_x };
            list.push(if axis == 1 { dd.Y } else { dd.X });
            self.guide_idx = list.len() as i32 - 1;
            self.drag = Drag::Guide;
            return;
        }
        // the selected object's handle takes priority in any drawing tool (and the lifted piece's in the Selection tool)
        let mut hh = self.hit_handle(p);
        let n = self.nobjs();
        let floating_sel = self.selected >= 0 && (self.selected as usize) < n && self.obj(self.selected as usize).floating;
        if hh >= 0 && self.tool == Tool::Pen && self.obj(self.selected as usize).kind == ObjType::Pen {
            hh = -1; // the pen draws next to its strokes
        }
        if hh >= 0
            && self.tool != Tool::Crop
            && (self.tool != Tool::Selection || floating_sel)
            && self.tool != Tool::Fill
            && self.tool != Tool::Eyedropper
        {
            self.dm().checkpoint();
            self.drag_orig = self.obj(self.selected as usize).clone();
            self.handle = hh;
            self.drag = Drag::Resize;
            return;
        }
        if self.tool == Tool::Selection {
            // the lifted piece moves with the Selection tool
            let hit = self.hit_object(dd);
            if hit >= 0 && self.obj(hit as usize).floating {
                if hit != self.selected {
                    self.select_hit(hit as usize);
                    self.sel_changed();
                }
                self.begin_move();
                return;
            }
        }
        if self.tool == Tool::Select
            && let Some((ga, gi)) = self.hit_guide(p)
        {
            self.guide_axis = ga;
            self.guide_idx = gi as i32;
            self.drag = Drag::Guide;
            return;
        }
        let t = self.tool;
        let draw_tool = !matches!(
            t,
            Tool::Select | Tool::Crop | Tool::Selection | Tool::Fill | Tool::Eraser | Tool::Eyedropper | Tool::Cutout
        );
        if t == Tool::Select || draw_tool {
            let mut hit = self.hit_object(dd);
            // the pen always draws: strokes are made close to each other (the Move tool moves them)
            if t == Tool::Pen {
                hit = -1;
            }
            // with a drawing tool, clicking an existing object selects/moves it (like the original)
            if hit >= 0 && (t == Tool::Select || hit == self.selected || !dbl) {
                // a drawing tool only picks objects of its own kind; over the others it draws. Ctrl forces drawing.
                let ho = self.obj(hit as usize);
                let ht = ho.kind;
                let same_kind = (t == Tool::Arrow && ht == ObjType::Arrow)
                    || (t == Tool::Line && ht == ObjType::Line)
                    || (t == Tool::Shape && ho.is_shape())
                    || (t == Tool::Pen && ht == ObjType::Pen)
                    || (t == Tool::Highlighter && ht == ObjType::Highlighter)
                    || (t == Tool::Text && ht == ObjType::Text)
                    || (t == Tool::Callout && ht == ObjType::Callout)
                    || (t == Tool::Magnify && ht == ObjType::Magnify)
                    || (t == Tool::Blur && (ht == ObjType::Blur || ht == ObjType::Pixelate))
                    || (t == Tool::Spotlight && ht == ObjType::Spotlight);
                if t == Tool::Select || hit == self.selected || (!m.ctrl && same_kind) {
                    if dbl && ho.has_text() {
                        self.select_hit(hit as usize);
                        self.begin_text_edit(hit as usize, false);
                        return;
                    }
                    let toggle = t == Tool::Select && (m.ctrl || m.shift);
                    if toggle && self.is_selected(hit as usize) {
                        // Ctrl/Shift+click on a selected object removes it from the selection when released without
                        // moving; dragging moves the selection
                        self.pending_deselect = hit;
                        self.begin_move();
                        return;
                    }
                    if toggle {
                        if self.selected >= 0 {
                            self.extra.push(self.selected);
                        }
                        self.selected = hit;
                    } else if !self.is_selected(hit as usize) {
                        self.select_hit(hit as usize);
                    } else if hit != self.selected {
                        self.extra.push(self.selected);
                        self.selected = hit;
                    }
                    self.sel_changed();
                    self.begin_move();
                    return;
                }
            }
            if t == Tool::Select {
                let keep = m.ctrl || m.shift;
                if self.selected != -1 && !keep {
                    self.selected = -1;
                    self.extra.clear();
                    self.sel_changed();
                }
                if self.smart_move {
                    // Smart Move: dragging inside the area lifts the pixels; outside, marks another area
                    if self.inside_selection(dd) {
                        self.start_smart_move();
                        return;
                    }
                    self.clear_raster_selection();
                    self.drag = Drag::Select;
                    self.sel_rect = rect(dd.X, dd.Y, 0.0, 0.0);
                    return;
                }
                self.drag = Drag::Marquee; // object selection rectangle
                self.sel_rect = rect(dd.X, dd.Y, 0.0, 0.0);
                return;
            }
        }
        if self.selected != -1 && draw_tool {
            self.selected = -1;
            self.extra.clear();
            self.sel_changed();
        }
        let s = self.current_style().clone();
        match t {
            Tool::Step => {
                self.dm().checkpoint();
                let mut o = DrawObj::new(ObjType::Step);
                o.style = s;
                o.p1 = dd;
                o.p2 = dd;
                let dm = self.dm();
                o.step_value = dm.st.next_step;
                dm.st.next_step += 1;
                dm.st.objs.push(Arc::new(o));
                self.selected = self.nobjs() as i32 - 1;
                self.extra.clear();
                self.refresh(true);
                self.doc_changed();
                self.sel_changed();
                return;
            }
            Tool::Fill => {
                let m = cv::do_wand(&self.d().st.base, (dd.X as i32, dd.Y as i32), self.tolerance, self.wand_global);
                if !m.is_empty() && !cv::mask_empty(&m) {
                    self.dm().checkpoint();
                    let base = self.dm().base_mut();
                    if s.grad.active() {
                        // gradient over the filled area
                        let b = cv::mask_bounds(&m, base.wi(), base.hi());
                        render::fill_gradient_mask(base, &m, b, &s.grad, s.opacity);
                    } else {
                        let c = (s.stroke & 0x00FFFFFF) | ((255 * s.opacity / 100) as u32) << 24;
                        for (i, v) in m.iter().enumerate() {
                            if *v != 0 {
                                base.px[i] = blend_over(base.px[i], c);
                            }
                        }
                    }
                    self.refresh(true);
                    self.doc_changed();
                }
                return;
            }
            Tool::Eyedropper => {
                self.recompose();
                let (x, y) = (dd.X as i32, dd.Y as i32);
                if x >= 0 && y >= 0 && x < self.composed.wi() && y < self.composed.hi() {
                    let c = self.composed.at(x, y) | 0xFF000000;
                    self.note(Note::ColorPicked(c));
                }
                return;
            }
            Tool::Eraser => {
                self.dm().checkpoint();
                self.dm().base_mut();
                self.drag = Drag::Erase;
                self.erase_at(dd);
                self.erase_last = dd;
                self.erase_shift = false;
                self.axis_lock = 0;
                self.dirty = true;
                return;
            }
            Tool::Crop => {
                if self.edge_snap && !m.alt {
                    self.down_doc.X = self.snap_edge(true, dd.X, dd.Y, dd.Y);
                    self.down_doc.Y = self.snap_edge(false, dd.Y, dd.X, dd.X);
                }
                self.drag = Drag::Crop;
                self.has_crop = false;
                self.crop = rect(self.down_doc.X, self.down_doc.Y, 0.0, 0.0);
                return;
            }
            Tool::Cutout => {
                self.drag = Drag::Cut;
                self.sel_rect = rect(dd.X, dd.Y, 0.0, 0.0);
                return;
            }
            Tool::Selection => {
                if self.inside_selection(dd) {
                    self.start_smart_move();
                    return;
                }
                self.clear_raster_selection();
                if self.sel_mode == 3 {
                    // wand (CSelectTool::DoWand)
                    let m = cv::do_wand(&self.d().st.base, (dd.X as i32, dd.Y as i32), self.tolerance, self.wand_global);
                    if !m.is_empty() {
                        self.set_selection_from_mask(m);
                    }
                    return;
                }
                if self.sel_mode == 2 {
                    self.drag = Drag::Lasso;
                    self.lasso = vec![dd];
                } else {
                    if self.edge_snap && !m.alt {
                        self.down_doc.X = self.snap_edge(true, dd.X, dd.Y, dd.Y);
                        self.down_doc.Y = self.snap_edge(false, dd.Y, dd.X, dd.X);
                    }
                    self.drag = Drag::Select;
                    self.sel_rect = rect(self.down_doc.X, self.down_doc.Y, 0.0, 0.0);
                }
                return;
            }
            _ => {}
        }
        if !draw_tool {
            return;
        }
        // object creation
        let mut o = DrawObj::new(match t {
            Tool::Arrow => ObjType::Arrow,
            Tool::Line => ObjType::Line,
            Tool::Shape => ObjType::Shape,
            Tool::Pen => ObjType::Pen,
            Tool::Highlighter => ObjType::Highlighter,
            Tool::Text => ObjType::Text,
            Tool::Callout => ObjType::Callout,
            Tool::Stamp => ObjType::Stamp,
            Tool::Blur => {
                if s.hide_kind == 1 {
                    ObjType::Pixelate
                } else {
                    ObjType::Blur
                }
            }
            Tool::Spotlight => ObjType::Spotlight,
            Tool::Magnify => ObjType::Magnify,
            _ => ObjType::Shape,
        });
        o.style = s;
        o.p1 = dd;
        o.p2 = dd;
        o.ctrl = dd;
        self.pen_anchor = -1;
        if t == Tool::Pen {
            o.pts = vec![dd];
            self.hold_shape = false;
        }
        if t == Tool::Stamp {
            o.text = self.stamp_id.encode_utf16().collect();
        }
        if o.is_area_tool() && o.style.area_shape == AREA_POLYGON {
            // polygon: corners clicked one by one
            o.pts = vec![dd, dd];
            self.preview = Some(o);
            self.preview_version += 1;
            self.drag = Drag::Polygon;
            self.note(Note::Hint(tr(
                "Clique nos cantos do polígono; clique no primeiro ponto, dê um duplo clique ou tecle Enter para fechar (Esc cancela).",
            )));
            return;
        }
        if o.is_area_tool() && o.style.area_shape == AREA_FREEHAND {
            o.pts = vec![dd];
        }
        self.preview = Some(o);
        self.preview_version += 1;
        self.drag = Drag::Create;
    }

    fn inside_selection(&self, dd: PointF) -> bool {
        let (x, y) = (dd.X as i32, dd.Y as i32);
        let (w, h) = (self.d().w(), self.d().h());
        self.has_sel && x >= 0 && y >= 0 && x < w && y < h && self.sel_mask[(y * w + x) as usize] != 0
    }

    pub fn on_move(&mut self, p: P, m: Mods) {
        self.mouse = Some(p);
        if !self.has_image() {
            return;
        }
        let mut dd = self.to_doc(p);
        let shift = m.shift;
        let (mut dx, mut dy) = (dd.X - self.down_doc.X, dd.Y - self.down_doc.Y);
        if self.rulers {
            let inside = dd.X >= 0.0 && dd.Y >= 0.0 && dd.X < self.d().w() as f32 && dd.Y < self.d().h() as f32;
            self.note(Note::CursorMoved(inside, dd.X, dd.Y));
        }
        match self.drag {
            Drag::None => {
                // link areas: outline and address under the cursor
                let mut hot = -1;
                let hit = self.hit_object(dd);
                if hit >= 0 && self.obj(hit as usize).kind == ObjType::Link {
                    hot = hit;
                } else {
                    for i in (0..self.nobjs()).rev() {
                        let o = self.obj(i);
                        if o.kind == ObjType::Link && !o.hidden && d::contains(&o.rect(), dd) {
                            hot = i as i32;
                            break;
                        }
                    }
                }
                self.link_hot = hot;
                self.cursor = self.cursor_for_point(p);
                return;
            }
            Drag::Polygon => {
                let o = self.preview.as_mut().unwrap();
                let mut e = dd;
                if shift && o.pts.len() >= 2 {
                    e = Self::constrain45(o.pts[o.pts.len() - 2], e);
                }
                *o.pts.last_mut().unwrap() = e;
                fit_to_points(o);
                self.preview_version += 1;
                self.cursor = Cursor::Cross;
            }
            Drag::Guide => {
                let (axis, gi) = (self.guide_axis, self.guide_idx);
                let dm = self.dm();
                let list = if axis == 1 { &mut dm.guides_y } else { &mut dm.guides_x };
                if gi >= 0 && (gi as usize) < list.len() {
                    list[gi as usize] = if axis == 1 { dd.Y } else { dd.X }.round();
                }
                dm.modified = true;
                self.cursor = if axis == 1 { Cursor::SizeNs } else { Cursor::SizeWe };
            }
            Drag::Pan => {
                self.scroll = (self.pan_start.0 - (p.0 - self.down_screen.0), self.pan_start.1 - (p.1 - self.down_screen.1));
                self.clamp_scroll();
            }
            Drag::Create => {
                let zoom = self.zoom as f32;
                let (hold_shape, draw_hold, down) = (self.hold_shape, self.draw_hold, self.down_doc);
                let mut pen_anchor = self.pen_anchor;
                let mut hold = None;
                let o = self.preview.as_mut().unwrap();
                let freehand = o.is_area_tool() && o.style.area_shape == AREA_FREEHAND;
                if hold_shape {
                    // recognized shape (C23): a line keeps following the pen
                    if o.is_line() {
                        o.p2 = dd;
                    }
                } else if (o.is_path() || freehand) && !o.pts.is_empty() {
                    if shift {
                        // a straight segment (45° steps) from where Shift was pressed
                        if pen_anchor < 0 || pen_anchor as usize >= o.pts.len() {
                            pen_anchor = o.pts.len() as i32 - 1;
                        }
                        o.pts.truncate(pen_anchor as usize + 1);
                        let a = o.pts[pen_anchor as usize];
                        o.pts.push(Self::constrain45(a, dd));
                        if !o.pressure.is_empty() {
                            o.pressure.truncate(pen_anchor as usize + 1);
                            let l = *o.pressure.last().unwrap();
                            o.pressure.push(l);
                        }
                        hold = Some(None);
                        if freehand {
                            fit_to_points(o);
                        }
                    } else {
                        pen_anchor = -1;
                        let last = *o.pts.last().unwrap();
                        if hypot(dd.X - last.X, dd.Y - last.Y) * zoom > 1.5 {
                            o.pts.push(dd);
                            if !o.pressure.is_empty() {
                                let l = *o.pressure.last().unwrap();
                                o.pressure.push(l);
                            }
                            if o.kind == ObjType::Pen && draw_hold {
                                hold = Some(Some(Instant::now() + std::time::Duration::from_millis(HOLD_MS)));
                            }
                        }
                        if freehand {
                            fit_to_points(o);
                        }
                    }
                } else {
                    let mut e = dd;
                    if shift {
                        // perfect geometry: lines at multiples of 45°; square, circle, equilateral triangle…
                        if o.is_line() {
                            e = Self::constrain45(down, dd);
                        } else {
                            let aspect = perfect_aspect(o);
                            let w = dx.abs().max(dy.abs() * aspect);
                            let h = w / aspect;
                            e = pt(down.X + if dx < 0.0 { -w } else { w }, down.Y + if dy < 0.0 { -h } else { h });
                        }
                    }
                    o.p2 = e;
                    o.ctrl = pt((o.p1.X + o.p2.X) / 2.0, (o.p1.Y + o.p2.Y) / 2.0);
                    let r = o.rect();
                    if o.kind == ObjType::Callout {
                        let sh = o.style.callout_shape;
                        o.tail = if (5..=11).contains(&sh) {
                            pt(rf_right(&r) + 30.0, r.Y + r.Height / 2.0) // arrows: pointing right
                        } else if sh == 4 || sh == 15 {
                            pt(rf_right(&r) + (40.0f32).max(r.Width * 0.5), rf_bottom(&r) + (30.0f32).max(r.Height * 0.6))
                        } else {
                            pt(r.X + r.Width * 0.25, rf_bottom(&r) + (20.0f32).max(r.Height * 0.45))
                        };
                    }
                    if o.kind == ObjType::Magnify {
                        o.tail = pt(r.X - r.Width * 0.4, rf_bottom(&r) + r.Height * 0.3);
                    }
                }
                self.pen_anchor = pen_anchor;
                if let Some(h) = hold {
                    self.hold_at = h;
                }
                self.preview_version += 1;
            }
            Drag::Move => {
                if (p.0 - self.down_screen.0).abs() + (p.1 - self.down_screen.1).abs() < 2 && !self.changed {
                    return;
                }
                if self.drag_multi.is_empty() {
                    if self.selection_locked() {
                        self.note(Note::Hint(tr("Objeto bloqueado: desbloqueie para mover.")));
                    }
                    return;
                }
                self.changed = true;
                // Shift: locks the movement to the axis the mouse moved along first
                if shift {
                    if self.axis_lock == 0 {
                        let (sx, sy) = ((p.0 - self.down_screen.0).abs(), (p.1 - self.down_screen.1).abs());
                        if sx + sy >= 4 {
                            self.axis_lock = if sx >= sy { 1 } else { 2 };
                        }
                    }
                } else {
                    self.axis_lock = 0;
                }
                if self.axis_lock == 1 {
                    dy = 0.0;
                } else if self.axis_lock == 2 {
                    dx = 0.0;
                }
                if self.snapping && !m.alt {
                    // snapping: edges and center of the set to the edges and center of the image and to the guides
                    let mut r = obj_extent(&self.drag_multi[0].1);
                    for (_, o) in &self.drag_multi {
                        r = d::union(&r, &obj_extent(o));
                    }
                    let tol = (8.0 / self.zoom) as f32;
                    let (ww, hh) = (self.d().w() as f32, self.d().h() as f32);
                    let rulers = self.rulers;
                    let snap = |a0: f32, len: f32, total: f32, guides: &[f32], dd: f32| {
                        let (mut best, mut best_dist) = (dd, tol);
                        let mut edges = vec![0.0, total / 2.0, total];
                        if rulers {
                            edges.extend_from_slice(guides);
                        }
                        for edge in edges {
                            for off in [0.0, len / 2.0, len] {
                                let cand = edge - off - a0;
                                let dist = (cand - dd).abs();
                                if dist < best_dist {
                                    best_dist = dist;
                                    best = cand;
                                }
                            }
                        }
                        best
                    };
                    if self.axis_lock != 2 {
                        dx = snap(r.X, r.Width, ww, &self.d().guides_x, dx);
                    }
                    if self.axis_lock != 1 {
                        dy = snap(r.Y, r.Height, hh, &self.d().guides_y, dy);
                    }
                }
                let multi = self.drag_multi.clone();
                let n = self.nobjs();
                for (i, orig) in multi {
                    if i >= n {
                        continue;
                    }
                    let o = self.dm().obj_mut(i).unwrap();
                    *o = orig;
                    o.move_by(dx, dy);
                }
                self.dirty = true;
            }
            Drag::Resize => {
                self.changed = true;
                let (handle, orig) = (self.handle, self.drag_orig.clone());
                let sel = self.selected as usize;
                let o = self.dm().obj_mut(sel).unwrap();
                *o = orig.clone();
                if o.is_line() {
                    if handle == 0 {
                        o.p1 = if shift { Self::constrain45(o.p2, dd) } else { dd };
                    } else if handle == 1 {
                        o.p2 = if shift { Self::constrain45(o.p1, dd) } else { dd };
                    } else {
                        // the curve goes through the point
                        o.ctrl = pt(2.0 * dd.X - 0.5 * (o.p1.X + o.p2.X), 2.0 * dd.Y - 0.5 * (o.p1.Y + o.p2.Y));
                    }
                    if handle != 2 && !o.style.bezier {
                        o.ctrl = pt((o.p1.X + o.p2.X) / 2.0, (o.p1.Y + o.p2.Y) / 2.0);
                    }
                } else if (o.kind == ObjType::Callout || o.kind == ObjType::Magnify) && handle == 8 {
                    o.tail = dd;
                } else {
                    let r0 = if o.is_path() {
                        let mut b = orig.bounds();
                        let pad = orig.style.width / 2.0 + if orig.style.shadow { 4.0 } else { 0.0 };
                        d::inflate(&mut b, -pad, -pad);
                        b
                    } else {
                        orig.rect()
                    };
                    let (mut l, mut t, mut r, mut b) = (r0.X, r0.Y, rf_right(&r0), rf_bottom(&r0));
                    match handle {
                        0 => {
                            l += dx;
                            t += dy
                        }
                        1 => t += dy,
                        2 => {
                            r += dx;
                            t += dy
                        }
                        3 => r += dx,
                        4 => {
                            r += dx;
                            b += dy
                        }
                        5 => b += dy,
                        6 => {
                            l += dx;
                            b += dy
                        }
                        7 => l += dx,
                        _ => {}
                    }
                    if shift && handle % 2 == 0 && r0.Width > 0.0 && r0.Height > 0.0 {
                        // proportional
                        let (sx, sy) = ((r - l) / r0.Width, (b - t) / r0.Height);
                        let s = sx.abs().max(sy.abs());
                        let (nw, nh) = (r0.Width * s, r0.Height * s);
                        match handle {
                            0 => {
                                l = r - nw;
                                t = b - nh
                            }
                            2 => {
                                r = l + nw;
                                t = b - nh
                            }
                            4 => {
                                r = l + nw;
                                b = t + nh
                            }
                            _ => {
                                l = r - nw;
                                b = t + nh
                            }
                        }
                    }
                    let to = rect(l.min(r), t.min(b), (r - l).abs(), (b - t).abs());
                    if o.kind == ObjType::Callout || o.kind == ObjType::Magnify {
                        o.p1 = pt(to.X, to.Y);
                        o.p2 = pt(rf_right(&to), rf_bottom(&to));
                        o.tail = orig.tail;
                    } else {
                        o.scale(r0, to);
                        o.update_group_rect();
                    }
                }
                self.dirty = true;
            }
            Drag::Crop | Drag::Select | Drag::Cut => {
                let down = self.down_doc;
                if self.drag != Drag::Cut && shift {
                    // Shift: square (circle with the ellipse selection)
                    let mm = dx.abs().max(dy.abs());
                    dd = pt(down.X + if dx < 0.0 { -mm } else { mm }, down.Y + if dy < 0.0 { -mm } else { mm });
                } else if self.drag != Drag::Cut && self.edge_snap && !m.alt {
                    // the moving corner snaps to contrast edges
                    dd.X = self.snap_edge(true, dd.X, down.Y, dd.Y);
                    dd.Y = self.snap_edge(false, dd.Y, down.X, dd.X);
                }
                let (w, h) = (self.d().w() as f32, self.d().h() as f32);
                let x0 = down.X.min(dd.X).clamp(0.0, w);
                let y0 = down.Y.min(dd.Y).clamp(0.0, h);
                let x1 = down.X.max(dd.X).clamp(0.0, w);
                let y1 = down.Y.max(dd.Y).clamp(0.0, h);
                let r = rect(
                    (x0 as i32) as f32,
                    (y0 as i32) as f32,
                    ((x1 - (x0 as i32) as f32 + 0.5) as i32) as f32,
                    ((y1 - (y0 as i32) as f32 + 0.5) as i32) as f32,
                );
                if self.drag == Drag::Crop {
                    self.crop = r;
                } else {
                    self.sel_rect = r;
                }
                if self.drag != Drag::Cut {
                    self.note(Note::AreaChanged);
                }
            }
            Drag::Lasso => {
                let last = *self.lasso.last().unwrap();
                if hypot(dd.X - last.X, dd.Y - last.Y) * self.zoom as f32 > 2.0 {
                    self.lasso.push(dd);
                }
            }
            Drag::Marquee => {
                let down = self.down_doc;
                self.sel_rect = rect(down.X.min(dd.X), down.Y.min(dd.Y), (dd.X - down.X).abs(), (dd.Y - down.Y).abs());
            }
            Drag::Erase => {
                // Shift: erases in a straight horizontal or vertical line from where Shift was pressed, along the axis
                // the mouse moved first
                if shift {
                    if !self.erase_shift {
                        self.erase_shift = true;
                        self.erase_anchor = self.erase_last;
                        self.axis_lock = 0;
                    }
                    if self.axis_lock == 0 {
                        let sx = (dd.X - self.erase_anchor.X).abs() * self.zoom as f32;
                        let sy = (dd.Y - self.erase_anchor.Y).abs() * self.zoom as f32;
                        if sx + sy < 4.0 {
                            return;
                        }
                        self.axis_lock = if sx >= sy { 1 } else { 2 };
                    }
                    if self.axis_lock == 1 {
                        dd.Y = self.erase_anchor.Y;
                    } else {
                        dd.X = self.erase_anchor.X;
                    }
                } else {
                    self.erase_shift = false;
                    self.axis_lock = 0;
                }
                let from = self.erase_last;
                self.erase_line(from, dd);
                self.erase_last = dd;
                self.dirty = true;
            }
        }
    }

    fn finish_create(&mut self) {
        let Some(mut o) = self.preview.take() else { return };
        let point_area = o.is_area_tool() && is_point_area(o.style.area_shape);
        if point_area {
            // polygon / freehand outline: needs a real area
            if o.pts.len() < 3 {
                return;
            }
            fit_to_points(&mut o);
        }
        let r = o.rect();
        let min_sz = (6.0 / self.zoom) as f32;
        let mut tiny = if o.is_path() && !o.pts.is_empty() {
            false
        } else if o.is_line() {
            hypot(o.p2.X - o.p1.X, o.p2.Y - o.p1.Y) < min_sz
        } else {
            r.Width < min_sz || r.Height < min_sz
        };
        if point_area {
            tiny = r.Width < min_sz && r.Height < min_sz;
        }
        if o.kind == ObjType::Text && r.Width >= min_sz * 3.0 {
            // dragged box: its width, minimum height of one line
            tiny = false;
            let min_h = o.style.font_size * 1.4 + 2.0 * o.style.padding;
            if r.Height < min_h {
                o.p1 = pt(r.X, r.Y);
                o.p2 = pt(rf_right(&r), r.Y + min_h);
            }
        }
        if tiny {
            if o.kind == ObjType::Text && o.style.vertical {
                // vertical text (C37): a column, about eight characters tall
                o.p2 = pt(
                    o.p1.X + (o.style.font_size * 1.6 + 2.0 * o.style.padding).max(24.0),
                    o.p1.Y + o.style.font_size * 1.25 * 8.0 + 2.0 * o.style.padding,
                );
            } else if o.kind == ObjType::Text {
                o.p2 = pt(o.p1.X + 260.0, o.p1.Y + o.style.font_size * 1.6);
            } else if o.kind == ObjType::Callout {
                o.p2 = pt(o.p1.X + 220.0, o.p1.Y + 90.0);
                o.tail = pt(o.p1.X + 50.0, o.p1.Y + 150.0);
            } else if o.kind == ObjType::Stamp {
                let a = render::stamp_aspect(&obj_text(&o));
                let (w, h) = if a > 1.2 { (64.0 * a, 64.0) } else { (96.0, 96.0) };
                o.p1 = pt(o.p1.X - w / 2.0, o.p1.Y - h / 2.0);
                o.p2 = pt(o.p1.X + w, o.p1.Y + h);
            } else {
                return;
            }
        }
        if o.kind == ObjType::Highlighter && o.style.snap_text && !o.style.text_only {
            self.snap_highlighter(&mut o); // C12
        }
        if o.style.text_only && o.is_area_tool() {
            self.update_text_boxes(&mut o);
        }
        self.dm().checkpoint();
        let (has_text, has_label, is_pen) = (o.has_text(), o.has_label() && o.text.is_empty(), o.kind == ObjType::Pen);
        self.dm().st.objs.push(Arc::new(o));
        self.selected = self.nobjs() as i32 - 1;
        self.extra.clear();
        if is_pen && self.tool == Tool::Pen {
            self.selected = -1; // freehand strokes follow one another unselected
        }
        self.dirty = true;
        self.sel_changed();
        if has_text {
            self.begin_text_edit(self.selected as usize, true);
        }
        if has_label {
            // Skitch style: typing right away adds a label at the tail
            self.label_ready = true;
            self.note(Note::Hint(tr("Digite para escrever um texto na ponta inicial da seta.")));
        }
    }

    pub fn on_up(&mut self, p: P, m: Mods) {
        self.hold_at = None;
        if self.doc().is_none() || self.drag == Drag::Polygon {
            return; // the polygon continues until it is closed
        }
        let dg = std::mem::take(&mut self.drag);
        match dg {
            Drag::Create => self.finish_create(),
            Drag::Move | Drag::Resize => {
                if !self.changed {
                    self.dm().discard_checkpoint();
                    if dg == Drag::Move && self.pending_deselect >= 0 {
                        // Ctrl/Shift+click without moving: leaves the selection
                        let pd = self.pending_deselect as usize;
                        let v: Vec<usize> = self.selected_list().into_iter().filter(|&i| i != pd).collect();
                        self.select_objects(&v);
                    }
                }
                self.pending_deselect = -1;
            }
            Drag::Guide => {
                // dropped back on a ruler (or outside the view): removed
                let (l, t, r, b) = self.view();
                let inside = p.0 >= l && p.0 < r && p.1 >= t && p.1 < b;
                let (axis, gi) = (self.guide_axis, self.guide_idx);
                let dm = self.dm();
                let list = if axis == 1 { &mut dm.guides_y } else { &mut dm.guides_x };
                if !inside && gi >= 0 && (gi as usize) < list.len() {
                    list.remove(gi as usize);
                }
                dm.modified = true;
                self.guide_idx = -1;
                return;
            }
            Drag::Crop => {
                self.has_crop = self.crop.Width >= 2.0 && self.crop.Height >= 2.0;
                self.note(Note::AreaChanged);
            }
            Drag::Select => {
                let s = self.sel_rect;
                if s.Width >= 2.0 && s.Height >= 2.0 {
                    let r = Rect { left: s.X as i32, top: s.Y as i32, right: rf_right(&s) as i32, bottom: rf_bottom(&s) as i32 };
                    let (w, h) = (self.d().w(), self.d().h());
                    let mm = if self.sel_mode == 1 { cv::ellipse_mask(w, h, r) } else { cv::rect_mask(w, h, r) };
                    self.set_selection_from_mask(mm);
                } else {
                    self.note(Note::AreaChanged); // a click without an area: no selection
                }
            }
            Drag::Lasso => {
                if self.lasso.len() >= 3 {
                    let pts: Vec<(i32, i32)> = self.lasso.iter().map(|q| (q.X as i32, q.Y as i32)).collect();
                    let (w, h) = (self.d().w(), self.d().h());
                    self.set_selection_from_mask(cv::polygon_mask(w, h, &pts));
                }
                self.lasso.clear();
            }
            Drag::Cut => {
                // CutOut (CPaintOpTearOutTool): removes the band and joins the parts
                let s = self.sel_rect;
                let horizontal = self.cut_dir == 0;
                let (a0, a1) = if horizontal { (s.Y as i32, rf_bottom(&s) as i32) } else { (s.X as i32, rf_right(&s) as i32) };
                let len = a1 - a0;
                let b = self.d().st.base.clone();
                let total = if horizontal { b.hi() } else { b.wi() };
                if len >= 2 && len < total {
                    self.dm().checkpoint();
                    let mut out =
                        Image::sized(if horizontal { b.wi() } else { b.wi() - len }, if horizontal { b.hi() - len } else { b.hi() }, 0);
                    for y in 0..out.hi() {
                        for x in 0..out.wi() {
                            let sx = if !horizontal && x >= a0 { x + len } else { x };
                            let sy = if horizontal && y >= a0 { y + len } else { y };
                            *out.at_mut(x, y) = b.at(sx, sy);
                        }
                    }
                    self.dm().set_base(out);
                    for i in 0..self.nobjs() {
                        let ob = self.obj(i).bounds();
                        let pos = if horizontal { ob.Y } else { ob.X };
                        if pos >= a1 as f32 {
                            self.dm().obj_mut(i).unwrap().move_by(
                                if horizontal { 0.0 } else { -len as f32 },
                                if horizontal { -len as f32 } else { 0.0 },
                            );
                        }
                    }
                    self.dirty = true;
                    self.fit_to_window();
                }
            }
            Drag::Erase => self.dirty = true,
            Drag::Marquee => {
                // selects the objects the rectangle touches (adding to the previous ones with Ctrl/Shift)
                let mut v = if m.ctrl || m.shift { self.selected_list() } else { Vec::new() };
                let s = self.sel_rect;
                if s.Width * self.zoom as f32 >= 3.0 || s.Height * self.zoom as f32 >= 3.0 {
                    for i in 0..self.nobjs() {
                        let b = self.obj(i).bounds();
                        if intersects(&b, &s) && !v.contains(&i) {
                            v.push(i);
                        }
                    }
                }
                self.select_objects(&v);
            }
            _ => {}
        }
        if (dg == Drag::Move || dg == Drag::Resize) && self.changed {
            let l = self.selected_list();
            self.refresh_text_boxes(&l);
        }
        self.refresh(true);
        if matches!(dg, Drag::Create | Drag::Move | Drag::Resize | Drag::Cut | Drag::Erase) {
            self.doc_changed();
        }
        if matches!(dg, Drag::Move | Drag::Resize) {
            self.sel_changed();
        }
    }

    /// Double click: crop applies, a text is edited, a lifted piece is dropped, a link opens; otherwise a click.
    pub fn on_double(&mut self, p: P, m: Mods) {
        if self.tool == Tool::Crop && self.has_crop {
            self.apply_crop();
            return;
        }
        if self.drag == Drag::Polygon {
            self.on_down(p, true, m);
            return;
        }
        if self.has_image() {
            let hit = self.hit_object(self.to_doc(p));
            if hit >= 0 {
                let o = self.obj(hit as usize);
                if o.has_text() || o.has_label() {
                    self.select_hit(hit as usize);
                    self.begin_text_edit(hit as usize, false);
                    return;
                }
                if o.floating {
                    self.drop_floating();
                    return;
                }
                if o.kind == ObjType::Link {
                    let t = obj_text(o);
                    if t.starts_with("http://") || t.starts_with("https://") || t.starts_with("mailto:") {
                        crate::shell_open(&t);
                    }
                    return;
                }
            }
        }
        self.on_down(p, true, m);
    }

    pub fn begin_pan(&mut self, p: P) {
        self.drag = Drag::Pan;
        self.down_screen = p;
        self.pan_start = self.scroll;
    }

    pub fn end_pan(&mut self) {
        if self.drag == Drag::Pan {
            self.drag = Drag::None;
        }
    }

    /// Right button: selects the object below (keeping the tool) for the context menu.
    pub fn on_right_down(&mut self, p: P) {
        if !self.has_image() || self.drag != Drag::None || self.fx_preview.is_some() {
            return;
        }
        self.commit_text();
        let hit = self.hit_object(self.to_doc(p));
        if hit >= 0 && !self.is_selected(hit as usize) {
            self.select_hit(hit as usize);
            self.sel_changed();
        }
    }

    pub fn on_wheel(&mut self, p: P, delta: i32, ctrl: bool, shift: bool) {
        if ctrl {
            // zoom centered on the mouse; touchpads send small deltas, which zoom proportionally
            self.zoom_to(self.zoom * 1.15f64.powf((delta as f64 / 120.0).clamp(-4.0, 4.0)), Some(p));
        } else {
            self.commit_text();
            if shift {
                self.scroll.0 -= delta;
            } else {
                self.scroll.1 -= delta;
            }
            self.clamp_scroll();
        }
    }

    pub fn cursor_for_point(&self, p: P) -> Cursor {
        if !self.has_image() {
            return Cursor::Arrow;
        }
        let hh = self.hit_handle(p);
        let rs = self.ruler_size();
        if rs > 0 && (p.0 < rs || p.1 < rs) {
            return if p.0 < rs && p.1 < rs {
                Cursor::Arrow
            } else if p.1 < rs {
                Cursor::SizeNs
            } else {
                Cursor::SizeWe
            };
        }
        if hh < 0
            && self.tool == Tool::Select
            && let Some((ga, _)) = self.hit_guide(p)
        {
            return if ga == 1 { Cursor::SizeNs } else { Cursor::SizeWe };
        }
        if hh >= 0 {
            const RC: [Cursor; 9] = [
                Cursor::SizeNwse,
                Cursor::SizeNs,
                Cursor::SizeNesw,
                Cursor::SizeWe,
                Cursor::SizeNwse,
                Cursor::SizeNs,
                Cursor::SizeNesw,
                Cursor::SizeWe,
                Cursor::Cross,
            ];
            return if self.obj(self.selected as usize).is_line() { Cursor::Cross } else { RC[(hh as usize).min(8)] };
        }
        match self.tool {
            Tool::Select => {
                if self.hit_object(self.to_doc(p)) >= 0 {
                    Cursor::SizeAll
                } else {
                    Cursor::Arrow
                }
            }
            Tool::Selection => {
                let dd = self.to_doc(p);
                let hit = self.hit_object(dd);
                if self.inside_selection(dd) || (hit >= 0 && self.obj(hit as usize).floating) { Cursor::SizeAll } else { Cursor::Cross }
            }
            Tool::Eyedropper => Cursor::Dropper,
            Tool::Fill => Cursor::Hand,
            t => {
                let hit = if t == Tool::Pen { -1 } else { self.hit_object(self.to_doc(p)) };
                if hit >= 0 && hit == self.selected { Cursor::SizeAll } else { Cursor::Cross }
            }
        }
    }

    // ---------------------------------------------------------------- keys
    /// Escape: cancels what is being done, then the areas, then the selection.
    pub fn escape(&mut self) {
        if self.drag != Drag::None {
            if matches!(self.drag, Drag::Move | Drag::Resize) && self.changed {
                self.dm().undo();
                self.dirty = true;
            } else if matches!(self.drag, Drag::Move | Drag::Resize) {
                self.dm().discard_checkpoint();
            }
            self.drag = Drag::None;
            self.preview = None;
        }
        if self.has_crop {
            self.has_crop = false;
            self.note(Note::AreaChanged);
        }
        self.clear_raster_selection();
        self.label_ready = false;
        if self.selected >= 0 {
            self.selected = -1;
            self.extra.clear();
            self.sel_changed();
        }
    }

    /// Enter: closes the polygon, applies the crop or drops the lifted piece.
    pub fn enter(&mut self) {
        if self.drag == Drag::Polygon {
            self.finish_polygon();
        } else if self.has_crop {
            self.apply_crop();
        } else if self.selected >= 0 && (self.selected as usize) < self.nobjs() && self.obj(self.selected as usize).floating {
            self.drop_floating();
        }
    }

    /// Delete or Backspace: the last polygon corner, or the selection.
    pub fn delete_key(&mut self) {
        if !self.polygon_back() {
            self.delete_selected();
        }
    }

    pub fn drag_state(&self) -> Drag {
        self.drag
    }

    pub fn crop_rect(&self) -> RectF {
        self.crop
    }

    pub fn selection_rect(&self) -> RectF {
        self.sel_rect
    }

    pub fn lasso_points(&self) -> &[PointF] {
        &self.lasso
    }
}

/// Fills the hole a lifted or deleted area leaves: transparent, the canvas color or smart (inpainting).
fn fill_hole(base: &mut Image, m: &[u8], sel_fill: i32) {
    match sel_fill {
        0 => {
            for (i, v) in m.iter().enumerate() {
                if *v != 0 {
                    base.px[i] = 0;
                }
            }
        }
        1 => {
            for (i, v) in m.iter().enumerate() {
                if *v != 0 {
                    base.px[i] = 0xFFFFFFFF;
                }
            }
        }
        _ => cv::smart_fill(base, m),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canvas_with(w: u32, h: u32) -> Canvas {
        let mut c = Canvas { view_w: 1000, view_h: 700, ..Default::default() };
        c.docs.push(Document::from_image(Image::new(w, h, 0xFFFFFFFF), "t"));
        c.set_document(Some(0));
        c
    }

    fn drag(c: &mut Canvas, pts: &[(f32, f32)]) {
        let s: Vec<P> = pts.iter().map(|p| c.to_screen(pt(p.0, p.1))).collect();
        c.on_down(s[0], false, Mods::default());
        for &p in &s[1..] {
            c.on_move(p, Mods::default());
        }
        c.on_up(*s.last().unwrap(), Mods::default());
    }

    #[test]
    fn fit_zoom_and_coordinates_match_the_cpp() {
        let c = canvas_with(1600, 1000);
        let z = ((1000 - 80) as f64 / 1600.0).min((700 - 80) as f64 / 1000.0);
        assert!((c.zoom - z).abs() < 1e-9);
        let o = c.origin();
        let iw = (1600.0 * z).round() as i32;
        assert_eq!(o.0, (1000 - iw) / 2);
        let p = c.to_doc(c.to_screen(pt(300.0, 200.0)));
        assert!((p.X - 300.0).abs() < 2.0 && (p.Y - 200.0).abs() < 2.0);
    }

    #[test]
    fn a_drag_creates_an_arrow_and_undo_removes_it() {
        let mut c = canvas_with(800, 600);
        c.set_tool(Tool::Arrow);
        drag(&mut c, &[(100.0, 100.0), (200.0, 150.0), (300.0, 200.0)]);
        assert_eq!(c.doc().unwrap().st.objs.len(), 1);
        assert_eq!(c.doc().unwrap().st.objs[0].kind, ObjType::Arrow);
        c.doc_mut().unwrap().undo();
        assert!(c.doc().unwrap().st.objs.is_empty());
    }

    #[test]
    fn steps_number_in_sequence_and_move_tool_moves() {
        let mut c = canvas_with(800, 600);
        c.set_tool(Tool::Step);
        for x in [100.0, 200.0, 300.0] {
            let p = c.to_screen(pt(x, 500.0));
            c.on_down(p, false, Mods::default());
            c.on_up(p, Mods::default());
        }
        let v: Vec<i32> = c.doc().unwrap().st.objs.iter().map(|o| o.step_value).collect();
        assert_eq!(v, [1, 2, 3]);
        c.set_tool(Tool::Shape);
        drag(&mut c, &[(100.0, 100.0), (200.0, 200.0)]);
        c.set_tool(Tool::Select);
        let x1 = c.doc().unwrap().st.objs[3].p1.X;
        drag(&mut c, &[(100.0, 125.0), (200.0, 200.0), (400.0, 300.0)]);
        assert!(c.doc().unwrap().st.objs[3].p1.X > x1 + 200.0);
    }

    #[test]
    fn eraser_covers_the_whole_stroke() {
        let mut c = canvas_with(400, 300);
        c.set_tool(Tool::Eraser);
        drag(&mut c, &[(50.0, 100.0), (350.0, 100.0)]);
        let b = &c.doc().unwrap().st.base;
        assert!((60..340).step_by(10).all(|x| ca(b.at(x, 100)) == 0));
        assert_eq!(ca(b.at(200, 160)), 255);
    }

    #[test]
    fn cutout_and_selection_crop_change_the_size() {
        let mut c = canvas_with(400, 300);
        c.set_tool(Tool::Cutout);
        drag(&mut c, &[(10.0, 100.0), (390.0, 150.0)]);
        assert_eq!(c.doc().unwrap().h(), 250);
        c.edge_snap = false;
        c.set_tool(Tool::Selection);
        drag(&mut c, &[(100.0, 50.0), (200.0, 150.0)]);
        c.crop_to_selection();
        assert_eq!((c.doc().unwrap().w(), c.doc().unwrap().h()), (100, 100));
    }

    #[test]
    fn group_ungroup_and_order() {
        let mut c = canvas_with(800, 600);
        c.set_tool(Tool::Shape);
        drag(&mut c, &[(100.0, 100.0), (200.0, 200.0)]);
        c.set_tool(Tool::Line);
        drag(&mut c, &[(300.0, 300.0), (500.0, 300.0)]);
        c.send_to_back();
        assert_eq!(c.doc().unwrap().st.objs[0].kind, ObjType::Line);
        c.select_all_objects();
        c.group_selected();
        assert_eq!(c.doc().unwrap().st.objs.len(), 1);
        c.ungroup_selected();
        assert_eq!(c.doc().unwrap().st.objs.len(), 2);
        assert_eq!(c.selection_count(), 2);
    }
}
