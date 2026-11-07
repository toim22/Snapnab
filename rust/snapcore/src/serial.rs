//! The editor's objects as XML (Serialize.cpp, comp_5 §6): one element per object in the .snap's `<Document>` and on
//! the clipboard, with the embedded images in streams named by `StreamName`. Numbers are written and read with the C
//! runtime's own functions (printf, atoi, atof, strtoul, sscanf_s), so the files keep the bytes the C++ wrote and old
//! or odd values read back the same.

use std::collections::BTreeMap;
use std::path::Path;

use crate::codec;
use crate::crt::{self, Arg};
use crate::doc::*;
use crate::gdip::{pt, rect};
use crate::image::Image;
use crate::net;
use crate::snap;
use crate::xml::{self, Node};

/// The embedded images of a frame (StreamName → image).
pub type Embedded = BTreeMap<String, Image>;

fn type_name(t: ObjType) -> &'static str {
    match t {
        ObjType::Arrow => "Arrow",
        ObjType::Line => "Line",
        ObjType::Rect => "Rect",
        ObjType::Ellipse => "Ellipse",
        ObjType::RoundRect => "RoundRect",
        ObjType::Pen => "Pen",
        ObjType::Highlighter => "Highlight",
        ObjType::Text => "Text",
        ObjType::Callout => "Callout",
        ObjType::Step => "Step",
        ObjType::Stamp => "Stamp",
        ObjType::Blur => "Blur",
        ObjType::Pixelate => "Pixelate",
        ObjType::Spotlight => "Spotlight",
        ObjType::Magnify => "Magnify",
        ObjType::Picture => "Image",
        ObjType::Shape => "Shape",
        ObjType::Group => "Group",
        // a link area has always been written as <Unknown>, and is read back from it
        ObjType::Link => "Unknown",
    }
}

fn type_from_name(n: &str) -> Option<ObjType> {
    ObjType::ALL.iter().copied().find(|&t| type_name(t) == n)
}

// ---------------------------------------------------------------- attribute values as XmlNode reads and writes them
fn get<'a>(n: &'a Node, k: &str) -> &'a str {
    n.attr(k).unwrap_or("")
}
fn geti(n: &Node, k: &str, def: i32) -> i32 {
    n.attr(k).map(crt::atoi).unwrap_or(def)
}
fn getd(n: &Node, k: &str, def: f64) -> f64 {
    n.attr(k).map(crt::atof).unwrap_or(def)
}
fn getf(n: &Node, k: &str, def: f32) -> f32 {
    getd(n, k, def as f64) as f32
}
fn getb(n: &Node, k: &str, def: bool) -> bool {
    geti(n, k, def as i32) != 0
}
fn getw(n: &Node, k: &str) -> Vec<u16> {
    get(n, k).encode_utf16().collect()
}
fn seti(n: &mut Node, k: &str, v: impl Into<i64>) {
    n.set(k, v.into().to_string());
}
fn setd(n: &mut Node, k: &str, v: f64) {
    n.set(k, snap::fixed4(v));
}

fn hex(v: u32) -> String {
    format!("#{v:08X}")
}

/// "#AARRGGBB", or "#RRGGBB" (opaque).
fn from_hex(s: &str, def: u32) -> u32 {
    if s.len() < 2 || !s.starts_with('#') {
        return def;
    }
    let v = crt::strtoul(&s[1..], 16);
    if s.len() == 7 { 0xFF000000 | v } else { v }
}

/// Reads "x,y x,y …"-style lists with sscanf_s: `count` floats per item, as long as whole items read.
fn scan_list(s: &str, format: &str, count: usize, mut item: impl FnMut(&[f32])) {
    let text: Vec<u8> = s.bytes().chain(std::iter::once(0)).collect();
    let mut i = 0;
    let mut f = vec![0f32; count];
    while i < s.len() {
        let (got, used) = crt::scan_floats(&text[i..], format, &mut f);
        if got < count as i32 || used <= 0 {
            break;
        }
        item(&f);
        i += used as usize;
    }
}

// ---------------------------------------------------------------- gradients
/// "0.000:#FF112233;1.000:#FF445566"
pub fn grad_stops_to_string(g: &Gradient) -> String {
    let mut out = String::new();
    for st in &g.stops {
        if !out.is_empty() {
            out.push(';');
        }
        out += &crt::fixed(st.pos as f64, 3);
        out.push(':');
        out += &hex(st.color);
    }
    out
}

/// FillGradient / GradientAngle / GradientStops, written only for an active gradient.
pub fn grad_to_xml(n: &mut Node, g: &Gradient) {
    if !g.active() {
        return;
    }
    seti(n, "FillGradient", g.kind);
    setd(n, "GradientAngle", g.angle as f64);
    n.set("GradientStops", grad_stops_to_string(g));
}

/// The gradient from its three attributes (each may be missing).
pub fn grad_from_attrs(kind: Option<&str>, angle: Option<&str>, stops: Option<&str>) -> Gradient {
    let mut g = Gradient {
        kind: kind.map(crt::atoi).unwrap_or(0).clamp(0, 2),
        angle: angle.map(crt::atof).unwrap_or(90.0) as f32,
        stops: Vec::new(),
    };
    for item in stops.unwrap_or("").split(';') {
        if let Some(c) = item.find(':') {
            g.stops.push(GradStop { pos: (crt::atof(&item[..c]) as f32).clamp(0.0, 1.0), color: from_hex(&item[c + 1..], 0xFF000000) });
        }
    }
    g.stops.sort_by(|a, b| a.pos.partial_cmp(&b.pos).unwrap_or(std::cmp::Ordering::Equal));
    if g.stops.len() < 2 {
        g.kind = 0;
    }
    g
}

pub fn grad_from_xml(n: &Node) -> Gradient {
    grad_from_attrs(n.attr("FillGradient"), n.attr("GradientAngle"), n.attr("GradientStops"))
}

// ---------------------------------------------------------------- objects
/// The object as an element; a picture goes into `embedded` as Image_{counter}.png.
pub fn obj_to_xml(o: &DrawObj, embedded: &mut Embedded, counter: &mut i32) -> Node {
    let mut n = Node::new(type_name(o.kind));
    setd(&mut n, "X1", o.p1.X as f64);
    setd(&mut n, "Y1", o.p1.Y as f64);
    setd(&mut n, "X2", o.p2.X as f64);
    setd(&mut n, "Y2", o.p2.Y as f64);
    // layers panel (F03, F22): name, visibility and lock; lifted piece (F25) — written only when set
    if !o.name.is_empty() {
        n.set("Name", o.name.as_str());
    }
    if o.hidden {
        seti(&mut n, "Hidden", 1);
    }
    if o.locked {
        seti(&mut n, "Locked", 1);
    }
    if o.floating {
        seti(&mut n, "Floating", 1);
    }
    if o.kind == ObjType::Group {
        for c in &o.children {
            n.children.push(obj_to_xml(c, embedded, counter));
        }
        return n;
    }
    if o.kind == ObjType::Link {
        n.set("Url", String::from_utf16_lossy(&o.text));
        return n;
    }
    let s = &o.style;
    n.set("PenColor", hex(s.stroke)).set("FillColor", hex(s.fill)).set("TextColor", hex(s.text_color));
    setd(&mut n, "PenWidth", s.width as f64);
    seti(&mut n, "Opacity", s.opacity);
    seti(&mut n, "Shadow", s.shadow);
    seti(&mut n, "ShadowDir", s.shadow_dir);
    n.set("ShadowColor", hex(s.shadow_color));
    seti(&mut n, "ShadowOpacity", s.shadow_opacity);
    seti(&mut n, "ShadowBlur", s.shadow_blur);
    seti(&mut n, "ShadowDist", s.shadow_dist);
    if o.is_line() || o.kind == ObjType::Callout || o.kind == ObjType::Pen || o.is_shape() {
        seti(&mut n, "ArrowStartCap", s.arrow_start);
        seti(&mut n, "ArrowEndCap", s.arrow_end);
        setd(&mut n, "StartSize", s.start_size as f64);
        setd(&mut n, "EndSize", s.end_size as f64);
        seti(&mut n, "DashType", s.dash);
    }
    if o.is_line() {
        seti(&mut n, "Bezier", s.bezier);
        setd(&mut n, "CtrlX", o.ctrl.X as f64);
        setd(&mut n, "CtrlY", o.ctrl.Y as f64);
    }
    let label = o.has_label() && !o.text.is_empty();
    if o.has_text() || o.kind == ObjType::Step || label {
        n.set("FaceName", s.font.as_str());
        setd(&mut n, "FontSize", s.font_size as f64);
        seti(&mut n, "Bold", s.bold);
        seti(&mut n, "Italic", s.italic);
        seti(&mut n, "Underline", s.underline);
        seti(&mut n, "AlignH", s.align_h);
        seti(&mut n, "AlignV", s.align_v);
        setd(&mut n, "TextPadding", s.padding as f64);
    }
    if o.has_text() || o.kind == ObjType::Stamp || label {
        n.set("Text", String::from_utf16_lossy(&o.text));
    }
    if o.has_text() {
        setd(&mut n, "LineSpacing", s.line_spacing as f64);
    }
    if o.kind == ObjType::Callout && s.bg_blur {
        seti(&mut n, "BackgroundBlur", 1);
        seti(&mut n, "BackgroundBlurAmount", s.bg_blur_amount);
    }
    if o.is_area_tool() {
        seti(&mut n, "AreaShape", s.area_shape);
    }
    if o.is_shape() {
        grad_to_xml(&mut n, &s.grad);
    }
    if o.kind == ObjType::Callout {
        seti(&mut n, "CalloutKind", s.callout_shape);
        setd(&mut n, "TailX", o.tail.X as f64);
        setd(&mut n, "TailY", o.tail.Y as f64);
        seti(&mut n, "Shaded", s.shaded);
        n.set("TailColor", hex(s.tail_color));
        setd(&mut n, "TailWidth", s.tail_width as f64);
    }
    if o.is_shape() {
        seti(&mut n, "ToolShape", o.shape_kind());
    }
    if o.kind == ObjType::Step {
        seti(&mut n, "StepValue", o.step_value);
        seti(&mut n, "StepStyle", s.step_shape);
        seti(&mut n, "StepSequenceType", s.step_type);
        if s.step_outline_width > 0.0 {
            n.set("StepOutlineColor", hex(s.step_outline));
            setd(&mut n, "StepOutlineWidth", s.step_outline_width as f64);
        }
    }
    if o.kind == ObjType::Magnify {
        seti(&mut n, "MagnifyShape", s.magnify_shape);
        seti(&mut n, "MagnifyScale", s.magnify_scale);
        seti(&mut n, "MagnifyLink", s.magnify_link);
        setd(&mut n, "TailX", o.tail.X as f64);
        setd(&mut n, "TailY", o.tail.Y as f64);
        if s.loupe {
            seti(&mut n, "Loupe", 1);
            seti(&mut n, "LoupeHandle", s.loupe_handle);
        }
    }
    if matches!(o.kind, ObjType::Blur | ObjType::Pixelate | ObjType::Spotlight | ObjType::Magnify) {
        seti(&mut n, "Amount", s.amount);
    }
    if o.is_path() || o.is_point_shape() {
        let mut pts = String::new();
        for p in &o.pts {
            pts += &crt::sprintf("%.1f,%.1f ", &[Arg::F(p.X as f64), Arg::F(p.Y as f64)]);
        }
        n.set("Points", pts);
    }
    // competitor feature request: written only when set, so older versions read the file as before
    if s.text_only {
        seti(&mut n, "TextOnly", 1);
        let mut boxes = String::new();
        for r in &o.text_boxes {
            boxes += &crt::sprintf(
                "%.1f,%.1f,%.1f,%.1f ",
                &[Arg::F(r.X as f64), Arg::F(r.Y as f64), Arg::F(r.Width as f64), Arg::F(r.Height as f64)],
            );
        }
        n.set("TextBoxes", boxes);
    }
    if s.snap_text {
        seti(&mut n, "SnapText", 1);
    }
    if s.sketch {
        seti(&mut n, "Sketch", 1);
        seti(&mut n, "SketchSeed", s.sketch_seed);
    }
    if s.vertical {
        seti(&mut n, "VerticalText", 1);
    }
    if s.text_on_path {
        seti(&mut n, "TextOnPath", 1);
    }
    if o.kind == ObjType::Pen && !o.pressure.is_empty() && o.pressure.len() == o.pts.len() {
        let mut pr = String::new();
        for &v in &o.pressure {
            pr += &crt::fixed(v as f64, 2);
            pr.push(' ');
        }
        n.set("Pressure", pr);
    }
    if o.kind == ObjType::Picture
        && let Some(pic) = &o.picture
    {
        let name = format!("Image_{counter}.png");
        *counter += 1;
        embedded.insert(name.clone(), pic.clone());
        n.set("StreamName", name);
    }
    n
}

/// The object an element describes; None for an unknown element, an empty group or link, or a missing image.
pub fn obj_from_xml(n: &Node, embedded: &Embedded) -> Option<DrawObj> {
    let t = type_from_name(&n.name)?; // unknown element: ignored (CPaintOpUnknown in the original)
    let mut o = DrawObj::new(t);
    o.p1 = pt(getf(n, "X1", 0.0), getf(n, "Y1", 0.0));
    o.p2 = pt(getf(n, "X2", 0.0), getf(n, "Y2", 0.0));
    o.name = get(n, "Name").to_owned();
    o.hidden = getb(n, "Hidden", false);
    o.locked = getb(n, "Locked", false);
    o.floating = getb(n, "Floating", false);
    if t == ObjType::Link {
        o.text = getw(n, "Url");
        return if o.text.is_empty() { None } else { Some(o) };
    }
    if t == ObjType::Group {
        o.children = n.children.iter().filter_map(|c| obj_from_xml(c, embedded)).collect();
        if o.children.is_empty() {
            return None;
        }
        o.update_group_rect();
        return Some(o);
    }
    let mut s = Style::default();
    s.stroke = from_hex(get(n, "PenColor"), s.stroke);
    s.fill = from_hex(get(n, "FillColor"), s.fill);
    s.text_color = from_hex(get(n, "TextColor"), s.text_color);
    s.width = getf(n, "PenWidth", s.width);
    s.opacity = geti(n, "Opacity", 100);
    s.shadow = getb(n, "Shadow", true);
    s.shadow_dir = geti(n, "ShadowDir", 8).clamp(0, 8);
    s.shadow_color = from_hex(get(n, "ShadowColor"), s.shadow_color);
    s.shadow_opacity = geti(n, "ShadowOpacity", s.shadow_opacity);
    s.shadow_blur = geti(n, "ShadowBlur", s.shadow_blur);
    s.shadow_dist = geti(n, "ShadowDist", s.shadow_dist);
    s.arrow_start = geti(n, "ArrowStartCap", 0).clamp(0, CAP_COUNT - 1);
    s.arrow_end = geti(n, "ArrowEndCap", 1).clamp(0, CAP_COUNT - 1);
    s.start_size = getf(n, "StartSize", s.start_size);
    s.end_size = getf(n, "EndSize", s.end_size);
    s.dash = geti(n, "DashType", 0).clamp(0, DASH_COUNT - 1);
    s.bezier = getb(n, "Bezier", false);
    o.ctrl = pt(getf(n, "CtrlX", (o.p1.X + o.p2.X) / 2.0), getf(n, "CtrlY", (o.p1.Y + o.p2.Y) / 2.0));
    if let Some(f) = n.attr("FaceName") {
        s.font = f.to_owned();
    }
    s.font_size = getf(n, "FontSize", s.font_size);
    s.bold = getb(n, "Bold", true);
    s.italic = getb(n, "Italic", false);
    s.underline = getb(n, "Underline", false);
    let text_default = if t == ObjType::Text { 0 } else { 1 };
    s.align_h = geti(n, "AlignH", text_default).clamp(0, 2);
    s.align_v = geti(n, "AlignV", text_default).clamp(0, 2);
    s.padding = getf(n, "TextPadding", s.padding);
    s.callout_shape = if n.attr("CalloutKind").is_some() {
        geti(n, "CalloutKind", 0).clamp(0, CALLOUT_SHAPES - 1)
    } else {
        const LEGACY: [i32; 4] = [13, 12, 14, 22];
        LEGACY[geti(n, "CalloutShape", 0).clamp(0, 3) as usize]
    };
    s.shaded = getb(n, "Shaded", false);
    s.tail_color = from_hex(get(n, "TailColor"), s.stroke);
    s.tail_width = getf(n, "TailWidth", s.tail_width);
    s.shape_kind = geti(n, "ToolShape", 0).clamp(0, SHAPE_COUNT - 1);
    s.step_shape = geti(n, "StepStyle", 0).clamp(0, 3);
    s.step_type = geti(n, "StepSequenceType", 0).clamp(0, STEP_TYPE_COUNT - 1);
    s.amount = geti(n, "Amount", 10);
    s.magnify_shape = geti(n, "MagnifyShape", 0).clamp(0, MAGNIFY_KIND_COUNT - 1);
    s.magnify_scale = geti(n, "MagnifyScale", 150.max(s.amount.wrapping_mul(20))).clamp(110, 1000);
    s.magnify_link = getb(n, "MagnifyLink", false);
    s.loupe = getb(n, "Loupe", false);
    s.loupe_handle = getb(n, "LoupeHandle", true);
    s.line_spacing = (getd(n, "LineSpacing", 1.0) as f32).clamp(0.8, 3.0);
    s.step_outline = from_hex(get(n, "StepOutlineColor"), s.step_outline);
    s.step_outline_width = (getd(n, "StepOutlineWidth", 0.0) as f32).clamp(0.0, 40.0);
    s.area_shape = geti(n, "AreaShape", SHAPE_RECT).clamp(0, AREA_SHAPE_COUNT - 1);
    s.bg_blur = getb(n, "BackgroundBlur", false);
    s.bg_blur_amount = geti(n, "BackgroundBlurAmount", 8).clamp(1, 40);
    s.grad = grad_from_xml(n);
    s.text_only = getb(n, "TextOnly", false);
    s.snap_text = getb(n, "SnapText", false);
    s.sketch = getb(n, "Sketch", false);
    s.sketch_seed = geti(n, "SketchSeed", 1);
    s.vertical = getb(n, "VerticalText", false);
    s.text_on_path = getb(n, "TextOnPath", false);
    scan_list(get(n, "TextBoxes"), "%f,%f,%f,%f %n", 4, |f| o.text_boxes.push(rect(f[0], f[1], f[2], f[3])));
    scan_list(get(n, "Pressure"), "%f %n", 1, |f| o.pressure.push(f[0].clamp(0.0, 1.0)));
    o.text = getw(n, "Text");
    o.tail = pt(getf(n, "TailX", 0.0), getf(n, "TailY", 0.0));
    o.step_value = geti(n, "StepValue", 1);
    scan_list(get(n, "Points"), "%f,%f %n", 2, |f| o.pts.push(pt(f[0], f[1])));
    o.style = s;
    if o.is_shape() && t != ObjType::Shape {
        // old rectangle / ellipse / rounded rectangle
        o.style.shape_kind = o.shape_kind();
        o.kind = ObjType::Shape;
    }
    if t == ObjType::Pixelate {
        o.style.hide_kind = 1;
    }
    if t == ObjType::Picture {
        o.picture = Some(embedded.get(get(n, "StreamName"))?.clone());
    }
    Some(o)
}

// ---------------------------------------------------------------- objects on the clipboard (copy / paste in place)
fn png(img: &Image) -> Option<Vec<u8>> {
    // internal copies carry no metadata (EncodeToMemory)
    codec::encode(img, codec::Format::Png, &codec::SaveOptions { strip_metadata: true, ..Default::default() }, "").ok()
}

/// Base64Decode: characters outside the alphabet are skipped.
fn base64_decode(s: &str) -> Vec<u8> {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut v: u32 = 0;
    let mut bits = 0;
    for c in s.bytes() {
        let Some(d) = T.iter().position(|&x| x == c) else { continue };
        v = v << 6 | d as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((v >> bits & 255) as u8);
        }
    }
    out
}

/// The objects as XML (as in the .snap), with their images as base64 PNG; `source` names the document.
pub fn objects_to_clip(objs: &[DrawObj], source: &str) -> String {
    let mut embedded = Embedded::new();
    let mut root = Node::new("SnapnabObjects");
    root.set("Source", source);
    let mut list = Node::new("Objects");
    let mut counter = 1;
    for o in objs {
        list.children.push(obj_to_xml(o, &mut embedded, &mut counter));
    }
    root.children.push(list);
    for (name, img) in &embedded {
        if let Some(png) = png(img) {
            root.add("Embedded").set("Name", name.as_str()).set("Data", net::base64(&png));
        }
    }
    xml::serialize(&root, false)
}

/// The objects of a clipboard text, and its source (None when the text is not Snapnab objects).
pub fn objects_from_clip(text: &[u8]) -> (Vec<DrawObj>, Option<String>) {
    let Some(root) = xml::parse(text).filter(|r| r.name == "SnapnabObjects") else { return (Vec::new(), None) };
    let source = get(&root, "Source").to_owned();
    let mut embedded = Embedded::new();
    for c in root.children.iter().filter(|c| c.name == "Embedded") {
        let png = base64_decode(get(c, "Data"));
        if png.is_empty() {
            continue;
        }
        if let Ok(img) = codec::decode(&png) {
            embedded.insert(get(c, "Name").to_owned(), img);
        }
    }
    let objs = root.child("Objects").map(|l| l.children.iter().filter_map(|n| obj_from_xml(n, &embedded)).collect()).unwrap_or_default();
    (objs, Some(source))
}

// ---------------------------------------------------------------- the .snap of a document
/// What a .snap stores of an edited image: the base, the objects and the capture's details.
#[derive(Clone, Debug, Default)]
pub struct DocFile {
    pub base: Image,
    pub objs: Vec<DrawObj>,
    pub capture_mode: String,
    pub window_title: String,
    pub application: String,
    pub created: String,
    pub tags: String,
    pub next_step: i32,
    /// ruler guides (image pixels)
    pub guides_x: Vec<f32>,
    pub guides_y: Vec<f32>,
}

impl DocFile {
    /// Writes the .snap (one frame); `version` goes into its index.
    pub fn save(&self, path: &Path, version: &str) -> bool {
        let mut f = snap::Frame::new(self.base.clone());
        let d = &mut f.doc;
        d.set("CaptureMode", self.capture_mode.as_str())
            .set("WindowTitle", self.window_title.as_str())
            .set("Application", self.application.as_str())
            .set("Created", self.created.as_str());
        seti(d, "NextStep", self.next_step);
        d.set("Tags", self.tags.as_str());
        if !self.guides_x.is_empty() || !self.guides_y.is_empty() {
            let gn = d.add("Guides");
            for (axis, list) in [("X", &self.guides_x), ("Y", &self.guides_y)] {
                for &v in list {
                    let g = gn.add("Guide");
                    g.set("Axis", axis);
                    setd(g, "Pos", v as f64);
                }
            }
        }
        let mut objs = Node::new("Objects");
        let mut counter = 1;
        for o in &self.objs {
            objs.children.push(obj_to_xml(o, &mut f.embedded, &mut counter));
        }
        f.doc.children.push(objs);
        snap::save(path, &[f], version)
    }

    /// Reads the first frame of a .snap.
    pub fn load(path: &Path) -> Option<DocFile> {
        let f = snap::load(path)?.into_iter().next()?;
        let d = &f.doc;
        let mut doc = DocFile {
            capture_mode: get(d, "CaptureMode").to_owned(),
            window_title: get(d, "WindowTitle").to_owned(),
            application: get(d, "Application").to_owned(),
            created: get(d, "Created").to_owned(),
            next_step: geti(d, "NextStep", 1),
            tags: get(d, "Tags").to_owned(),
            ..Default::default()
        };
        if let Some(gn) = d.child("Guides") {
            for c in &gn.children {
                let v = getd(c, "Pos", 0.0) as f32;
                if get(c, "Axis") == "Y" { doc.guides_y.push(v) } else { doc.guides_x.push(v) }
            }
        }
        if let Some(list) = d.child("Objects") {
            doc.objs = list.children.iter().filter_map(|n| obj_from_xml(n, &f.embedded)).collect();
        }
        doc.base = f.base;
        Some(doc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn objects_round_trip() {
        let mut a = DrawObj::new(ObjType::Arrow);
        a.p1 = pt(1.0, 2.0);
        a.p2 = pt(30.0, 40.0);
        a.text = "Ação <&>".encode_utf16().collect();
        let mut pic = DrawObj::new(ObjType::Picture);
        pic.picture = Some(Image { w: 2, h: 1, px: vec![0xFF112233, 0x80445566], dpi: 0.0, meta: None });
        let clip = objects_to_clip(&[a.clone(), pic], "doc.snap");
        let (back, src) = objects_from_clip(clip.as_bytes());
        assert_eq!(src.as_deref(), Some("doc.snap"));
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].text, a.text);
        assert_eq!(back[1].picture.as_ref().unwrap().px, vec![0xFF112233, 0x80445566]);
        assert_eq!(objects_from_clip(b"<Other/>").1, None);
    }

    #[test]
    fn legacy_values() {
        let n = xml::parse(b"<Rect X1=\"0x1A\" PenColor=\"#123456\" GradientStops=\"1:#FF0000FF;0:#FF00FF00\" FillGradient=\"1\"/>").unwrap();
        let o = obj_from_xml(&n, &Embedded::new()).unwrap();
        assert_eq!(o.kind, ObjType::Shape);
        assert_eq!(o.p1.X, 26.0);
        assert_eq!(o.style.stroke, 0xFF123456);
        assert_eq!(o.style.grad.stops[0].color, 0xFF00FF00);
        assert!(obj_from_xml(&xml::parse(b"<Link Url=\"x\"/>").unwrap(), &Embedded::new()).is_none());
        assert!(obj_from_xml(&xml::parse(b"<Unknown Url=\"x\"/>").unwrap(), &Embedded::new()).is_some());
    }
}
