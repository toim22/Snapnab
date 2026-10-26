// XML serialization of the objects (comp_5 §6: one object = one element; binary data in streams by StreamName)
#include "Document.h"

namespace snap::ed {

static const char* TypeName(ObjType t) {
    switch (t) {
    case ObjType::Arrow: return "Arrow";
    case ObjType::Line: return "Line";
    case ObjType::Rect: return "Rect";
    case ObjType::Ellipse: return "Ellipse";
    case ObjType::RoundRect: return "RoundRect";
    case ObjType::Pen: return "Pen";
    case ObjType::Highlighter: return "Highlight";
    case ObjType::Text: return "Text";
    case ObjType::Callout: return "Callout";
    case ObjType::Step: return "Step";
    case ObjType::Stamp: return "Stamp";
    case ObjType::Blur: return "Blur";
    case ObjType::Pixelate: return "Pixelate";
    case ObjType::Spotlight: return "Spotlight";
    case ObjType::Magnify: return "Magnify";
    case ObjType::Picture: return "Image";
    case ObjType::Shape: return "Shape";
    case ObjType::Group: return "Group";
    default: break;
    }
    return "Unknown";
}

static bool TypeFromName(const std::string& n, ObjType& t) {
    for (int i = 0; i < (int)ObjType::Count; i++)
        if (n == TypeName((ObjType)i)) { t = (ObjType)i; return true; }
    return false;
}

static std::string Hex(uint32_t v) { char b[16]; snprintf(b, sizeof b, "#%08X", v); return b; }
static uint32_t FromHex(const std::string& s, uint32_t def) {
    if (s.size() < 2 || s[0] != '#') return def;
    unsigned long v = strtoul(s.c_str() + 1, nullptr, 16);
    return s.size() == 7 ? (0xFF000000u | (uint32_t)v) : (uint32_t)v;
}

std::string GradStopsToString(const Gradient& g) {
    std::string out;
    for (auto& st : g.stops) out += FormatA("%s%.3f:%s", out.empty() ? "" : ";", st.pos, Hex(st.color).c_str());
    return out;
}

void GradToXml(XmlNode& n, const Gradient& g) {
    if (!g.Active()) return;
    n.seti("FillGradient", g.type).setd("GradientAngle", g.angle).set("GradientStops", GradStopsToString(g));
}

Gradient GradFromXml(const XmlNode& n) {
    Gradient g;
    g.type = std::clamp(n.geti("FillGradient", 0), 0, 2);
    g.angle = (float)n.getd("GradientAngle", 90);
    std::string st = n.get("GradientStops");
    for (size_t i = 0; i < st.size();) {
        size_t j = st.find(';', i);
        std::string item = st.substr(i, j == std::string::npos ? std::string::npos : j - i);
        size_t c = item.find(':');
        if (c != std::string::npos) g.stops.push_back({ std::clamp((float)atof(item.substr(0, c).c_str()), 0.0f, 1.0f), FromHex(item.substr(c + 1), 0xFF000000) });
        if (j == std::string::npos) break;
        i = j + 1;
    }
    std::sort(g.stops.begin(), g.stops.end(), [](const GradStop& a, const GradStop& b) { return a.pos < b.pos; });
    if (g.stops.size() < 2) g.type = 0;
    return g;
}

XmlNode ObjToXml(const DrawObj& o, SnapFrame& frame, int& counter) {
    XmlNode n(TypeName(o.type));
    n.setd("X1", o.p1.X).setd("Y1", o.p1.Y).setd("X2", o.p2.X).setd("Y2", o.p2.Y);
    // layers panel (F03, F22): name, visibility and lock; lifted piece (F25) — written only when set
    if (!o.name.empty()) n.setW("Name", o.name);
    if (o.hidden) n.seti("Hidden", 1);
    if (o.locked) n.seti("Locked", 1);
    if (o.floating) n.seti("Floating", 1);
    if (o.type == ObjType::Group) {            // <Group …> with the child objects inside
        for (auto& c : o.children) n.children.push_back(ObjToXml(c, frame, counter));
        return n;
    }
    if (o.type == ObjType::Link) { n.setW("Url", o.text); return n; }   // clickable link area (F32)
    const Style& s = o.style;
    n.set("PenColor", Hex(s.stroke)).set("FillColor", Hex(s.fill)).set("TextColor", Hex(s.textColor));
    n.setd("PenWidth", s.width).seti("Opacity", s.opacity).seti("Shadow", s.shadow);
    n.seti("ShadowDir", s.shadowDir).set("ShadowColor", Hex(s.shadowColor)).seti("ShadowOpacity", s.shadowOpacity)
     .seti("ShadowBlur", s.shadowBlur).seti("ShadowDist", s.shadowDist);
    if (o.IsLine() || o.type == ObjType::Callout || o.type == ObjType::Pen || o.IsShape())
        n.seti("ArrowStartCap", s.arrowStart).seti("ArrowEndCap", s.arrowEnd).setd("StartSize", s.startSize)
         .setd("EndSize", s.endSize).seti("DashType", s.dash);
    if (o.IsLine()) n.seti("Bezier", s.bezier).setd("CtrlX", o.ctrl.X).setd("CtrlY", o.ctrl.Y);
    bool label = o.HasLabel() && !o.text.empty();
    if (o.HasText() || o.type == ObjType::Step || label) {
        n.setW("FaceName", s.font).setd("FontSize", s.fontSize).seti("Bold", s.bold).seti("Italic", s.italic)
         .seti("Underline", s.underline).seti("AlignH", s.alignH).seti("AlignV", s.alignV).setd("TextPadding", s.padding);
    }
    if (o.HasText() || o.type == ObjType::Stamp || label) n.setW("Text", o.text);
    if (o.HasText()) n.setd("LineSpacing", s.lineSpacing);
    if (o.type == ObjType::Callout && s.bgBlur) n.seti("BackgroundBlur", 1).seti("BackgroundBlurAmount", s.bgBlurAmount);
    if (o.IsAreaTool()) n.seti("AreaShape", s.areaShape);
    if (o.IsShape()) GradToXml(n, s.grad);
    if (o.type == ObjType::Callout)
        n.seti("CalloutKind", s.calloutShape).setd("TailX", o.tail.X).setd("TailY", o.tail.Y).seti("Shaded", s.shaded)
         .set("TailColor", Hex(s.tailColor)).setd("TailWidth", s.tailWidth);
    if (o.IsShape()) n.seti("ToolShape", o.Kind());
    if (o.type == ObjType::Step) {
        n.seti("StepValue", o.stepValue).seti("StepStyle", s.stepShape).seti("StepSequenceType", s.stepType);
        if (s.stepOutlineWidth > 0) n.set("StepOutlineColor", Hex(s.stepOutline)).setd("StepOutlineWidth", s.stepOutlineWidth);
    }
    if (o.type == ObjType::Magnify) {
        n.seti("MagnifyShape", s.magnifyShape).seti("MagnifyScale", s.magnifyScale).seti("MagnifyLink", s.magnifyLink)
         .setd("TailX", o.tail.X).setd("TailY", o.tail.Y);
        if (s.loupe) n.seti("Loupe", 1).seti("LoupeHandle", s.loupeHandle);
    }
    if (o.type == ObjType::Blur || o.type == ObjType::Pixelate || o.type == ObjType::Spotlight || o.type == ObjType::Magnify)
        n.seti("Amount", s.amount);
    if (o.IsPath() || o.IsPointShape()) {
        std::string pts;
        for (auto& p : o.pts) pts += FormatA("%.1f,%.1f ", p.X, p.Y);
        n.set("Points", pts);
    }
    if (o.type == ObjType::Picture && o.picture) {
        std::string name = FormatA("Image_%d.png", counter++);
        frame.embedded[name] = *o.picture;
        n.set("StreamName", name);
    }
    return n;
}

ObjPtr ObjFromXml(const XmlNode& n, const SnapFrame& frame) {
    ObjType t;
    if (!TypeFromName(n.name, t)) return nullptr;     // unknown element: ignored (CPaintOpUnknown in the original)
    auto o = std::make_shared<DrawObj>();
    o->type = t;
    o->p1 = { (float)n.getd("X1"), (float)n.getd("Y1") };
    o->p2 = { (float)n.getd("X2"), (float)n.getd("Y2") };
    o->name = n.getW("Name");
    o->hidden = n.getb("Hidden");
    o->locked = n.getb("Locked");
    o->floating = n.getb("Floating");
    if (t == ObjType::Link) { o->text = n.getW("Url"); return o->text.empty() ? nullptr : o; }
    if (t == ObjType::Group) {
        for (auto& c : n.children) if (auto child = ObjFromXml(c, frame)) o->children.push_back(*child);
        if (o->children.empty()) return nullptr;
        o->UpdateGroupRect();
        return o;
    }
    Style& s = o->style;
    s.stroke = FromHex(n.get("PenColor"), s.stroke);
    s.fill = FromHex(n.get("FillColor"), s.fill);
    s.textColor = FromHex(n.get("TextColor"), s.textColor);
    s.width = (float)n.getd("PenWidth", s.width);
    s.opacity = n.geti("Opacity", 100);
    s.shadow = n.getb("Shadow", true);
    s.shadowDir = std::clamp(n.geti("ShadowDir", 8), 0, 8);
    s.shadowColor = FromHex(n.get("ShadowColor"), s.shadowColor);
    s.shadowOpacity = n.geti("ShadowOpacity", s.shadowOpacity);
    s.shadowBlur = n.geti("ShadowBlur", s.shadowBlur);
    s.shadowDist = n.geti("ShadowDist", s.shadowDist);
    s.arrowStart = std::clamp(n.geti("ArrowStartCap", 0), 0, CapCount - 1);
    s.arrowEnd = std::clamp(n.geti("ArrowEndCap", 1), 0, CapCount - 1);
    s.startSize = (float)n.getd("StartSize", s.startSize);
    s.endSize = (float)n.getd("EndSize", s.endSize);
    s.dash = std::clamp(n.geti("DashType", 0), 0, DashCount - 1);
    s.bezier = n.getb("Bezier");
    o->ctrl = { (float)n.getd("CtrlX", (o->p1.X + o->p2.X) / 2), (float)n.getd("CtrlY", (o->p1.Y + o->p2.Y) / 2) };
    s.font = n.getW("FaceName", s.font);
    s.fontSize = (float)n.getd("FontSize", s.fontSize);
    s.bold = n.getb("Bold", true);
    s.italic = n.getb("Italic");
    s.underline = n.getb("Underline");
    s.alignH = std::clamp(n.geti("AlignH", t == ObjType::Text ? 0 : 1), 0, 2);
    s.alignV = std::clamp(n.geti("AlignV", t == ObjType::Text ? 0 : 1), 0, 2);
    s.padding = (float)n.getd("TextPadding", s.padding);
    if (n.attr("CalloutKind")) s.calloutShape = std::clamp(n.geti("CalloutKind"), 0, kCalloutShapes - 1);
    else { static const int legacy[] = { 13, 12, 14, 22 }; s.calloutShape = legacy[std::clamp(n.geti("CalloutShape", 0), 0, 3)]; }
    s.shaded = n.getb("Shaded");
    s.tailColor = FromHex(n.get("TailColor"), s.stroke);
    s.tailWidth = (float)n.getd("TailWidth", s.tailWidth);
    s.shapeKind = std::clamp(n.geti("ToolShape", 0), 0, ShapeCount - 1);
    s.stepShape = std::clamp(n.geti("StepStyle", 0), 0, 3);
    s.stepType = std::clamp(n.geti("StepSequenceType", 0), 0, StepTypeCount - 1);
    s.amount = n.geti("Amount", 10);
    s.magnifyShape = std::clamp(n.geti("MagnifyShape", 0), 0, MagnifyKindCount - 1);
    s.magnifyScale = std::clamp(n.geti("MagnifyScale", std::max(150, s.amount * 20)), 110, 1000);
    s.magnifyLink = n.getb("MagnifyLink");
    s.loupe = n.getb("Loupe");
    s.loupeHandle = n.getb("LoupeHandle", true);
    s.lineSpacing = std::clamp((float)n.getd("LineSpacing", 1.0), 0.8f, 3.0f);
    s.stepOutline = FromHex(n.get("StepOutlineColor"), s.stepOutline);
    s.stepOutlineWidth = std::clamp((float)n.getd("StepOutlineWidth", 0), 0.0f, 40.0f);
    s.areaShape = std::clamp(n.geti("AreaShape", ShapeRect), 0, AreaShapeCount - 1);
    s.bgBlur = n.getb("BackgroundBlur");
    s.bgBlurAmount = std::clamp(n.geti("BackgroundBlurAmount", 8), 1, 40);
    s.grad = GradFromXml(n);
    o->text = n.getW("Text");
    o->tail = { (float)n.getd("TailX"), (float)n.getd("TailY") };
    o->stepValue = n.geti("StepValue", 1);
    std::string pts = n.get("Points");
    for (size_t i = 0; i < pts.size();) {
        float x, y; int used = 0;
        if (sscanf_s(pts.c_str() + i, "%f,%f %n", &x, &y, &used) < 2 || used <= 0) break;
        o->pts.push_back({ x, y });
        i += used;
    }
    if (o->IsShape() && t != ObjType::Shape) { s.shapeKind = o->Kind(); o->type = ObjType::Shape; }    // old rectangle/ellipse
    if (t == ObjType::Pixelate) s.hideKind = 1;
    if (t == ObjType::Picture) {
        auto it = frame.embedded.find(n.get("StreamName"));
        if (it == frame.embedded.end()) return nullptr;
        o->picture = std::make_shared<Image>(it->second);
    }
    return o;
}

// ---------------------------------------------------------------- objects on the clipboard (copy / paste in place)
// the objects as XML (as in the .snap), with the embedded images in base64 PNG; 'source' identifies the document
std::string ObjectsToClip(const std::vector<DrawObj>& objs, const std::wstring& source) {
    SnapFrame f;
    XmlNode root("SnapnabObjects");
    root.setW("Source", source);
    XmlNode& list = root.add("Objects");
    int counter = 1;
    for (auto& o : objs) list.children.push_back(ObjToXml(o, f, counter));
    for (auto& [name, img] : f.embedded) {
        std::vector<BYTE> png;
        if (EncodeToMemory(img, ImageFormat::PNG, png)) root.add("Embedded").set("Name", name).set("Data", Base64Encode(png));
    }
    return XmlSerialize(root, false);
}

std::vector<DrawObj> ObjectsFromClip(const std::string& xml, std::wstring* source) {
    std::vector<DrawObj> out;
    XmlNode root;
    if (!XmlParse(xml, root) || root.name != "SnapnabObjects") return out;
    if (source) *source = root.getW("Source");
    SnapFrame f;
    for (auto& c : root.children) {
        if (c.name != "Embedded") continue;
        auto png = Base64Decode(c.get("Data"));
        Image img;
        if (!png.empty() && DecodeFromMemory(png.data(), png.size(), img)) f.embedded[c.get("Name")] = std::move(img);
    }
    if (const XmlNode* list = root.child("Objects"))
        for (auto& n : list->children) if (auto o = ObjFromXml(n, f)) out.push_back(*o);
    return out;
}

} // namespace snap::ed
