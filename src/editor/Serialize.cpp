// Serialização XML dos objetos (comp_5 §6: um objeto = um elemento; dados binários em fluxos por StreamName)
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

XmlNode ObjToXml(const DrawObj& o, SnapFrame& frame, int& counter) {
    XmlNode n(TypeName(o.type));
    n.setd("X1", o.p1.X).setd("Y1", o.p1.Y).setd("X2", o.p2.X).setd("Y2", o.p2.Y);
    if (o.type == ObjType::Group) {            // <Group …> com os objetos filhos dentro
        for (auto& c : o.children) n.children.push_back(ObjToXml(c, frame, counter));
        return n;
    }
    const Style& s = o.style;
    n.set("PenColor", Hex(s.stroke)).set("FillColor", Hex(s.fill)).set("TextColor", Hex(s.textColor));
    n.setd("PenWidth", s.width).seti("Opacity", s.opacity).seti("Shadow", s.shadow);
    n.seti("ShadowDir", s.shadowDir).set("ShadowColor", Hex(s.shadowColor)).seti("ShadowOpacity", s.shadowOpacity)
     .seti("ShadowBlur", s.shadowBlur).seti("ShadowDist", s.shadowDist);
    if (o.IsLine() || o.type == ObjType::Callout || o.type == ObjType::Pen || o.IsShape())
        n.seti("ArrowStartCap", s.arrowStart).seti("ArrowEndCap", s.arrowEnd).setd("StartSize", s.startSize)
         .setd("EndSize", s.endSize).seti("DashType", s.dash);
    if (o.IsLine()) n.seti("Bezier", s.bezier).setd("CtrlX", o.ctrl.X).setd("CtrlY", o.ctrl.Y);
    if (o.HasText() || o.type == ObjType::Step) {
        n.setW("FaceName", s.font).setd("FontSize", s.fontSize).seti("Bold", s.bold).seti("Italic", s.italic)
         .seti("Underline", s.underline).seti("AlignH", s.alignH).seti("AlignV", s.alignV).setd("TextPadding", s.padding);
    }
    if (o.HasText() || o.type == ObjType::Stamp) n.setW("Text", o.text);
    if (o.type == ObjType::Callout)
        n.seti("CalloutKind", s.calloutShape).setd("TailX", o.tail.X).setd("TailY", o.tail.Y).seti("Shaded", s.shaded)
         .set("TailColor", Hex(s.tailColor)).setd("TailWidth", s.tailWidth);
    if (o.IsShape()) n.seti("ToolShape", o.Kind());
    if (o.type == ObjType::Step) n.seti("StepValue", o.stepValue).seti("StepStyle", s.stepShape).seti("StepSequenceType", s.stepType);
    if (o.type == ObjType::Magnify)
        n.seti("MagnifyShape", s.magnifyShape).seti("MagnifyScale", s.magnifyScale).seti("MagnifyLink", s.magnifyLink)
         .setd("TailX", o.tail.X).setd("TailY", o.tail.Y);
    if (o.type == ObjType::Blur || o.type == ObjType::Pixelate || o.type == ObjType::Spotlight || o.type == ObjType::Magnify)
        n.seti("Amount", s.amount);
    if (o.IsPath()) {
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
    if (!TypeFromName(n.name, t)) return nullptr;     // elemento desconhecido: ignorado (CPaintOpUnknown no original)
    auto o = std::make_shared<DrawObj>();
    o->type = t;
    o->p1 = { (float)n.getd("X1"), (float)n.getd("Y1") };
    o->p2 = { (float)n.getd("X2"), (float)n.getd("Y2") };
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
    s.magnifyShape = std::clamp(n.geti("MagnifyShape", 0), 0, 2);
    s.magnifyScale = std::clamp(n.geti("MagnifyScale", std::max(150, s.amount * 20)), 110, 1000);
    s.magnifyLink = n.getb("MagnifyLink");
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
    if (o->IsShape() && t != ObjType::Shape) { s.shapeKind = o->Kind(); o->type = ObjType::Shape; }    // retângulo/elipse antigos
    if (t == ObjType::Pixelate) s.hideKind = 1;
    if (t == ObjType::Picture) {
        auto it = frame.embedded.find(n.get("StreamName"));
        if (it == frame.embedded.end()) return nullptr;
        o->picture = std::make_shared<Image>(it->second);
    }
    return o;
}

} // namespace snap::ed
