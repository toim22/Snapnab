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
    }
    return "Unknown";
}

static bool TypeFromName(const std::string& n, ObjType& t) {
    for (int i = 0; i <= (int)ObjType::Picture; i++)
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
    const Style& s = o.style;
    n.set("PenColor", Hex(s.stroke)).set("FillColor", Hex(s.fill)).set("TextColor", Hex(s.textColor));
    n.setd("PenWidth", s.width).seti("Opacity", s.opacity).seti("Shadow", s.shadow);
    if (o.IsLine()) n.seti("ArrowStartCap", s.arrowStart).seti("ArrowEndCap", s.arrowEnd);
    if (o.HasText() || o.type == ObjType::Step) {
        n.setW("FaceName", s.font).setd("FontSize", s.fontSize).seti("Bold", s.bold);
    }
    if (o.HasText() || o.type == ObjType::Stamp) n.setW("Text", o.text);
    if (o.type == ObjType::Callout) n.seti("CalloutShape", s.calloutShape).setd("TailX", o.tail.X).setd("TailY", o.tail.Y);
    if (o.type == ObjType::Step) n.seti("StepValue", o.stepValue);
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
    Style& s = o->style;
    s.stroke = FromHex(n.get("PenColor"), s.stroke);
    s.fill = FromHex(n.get("FillColor"), s.fill);
    s.textColor = FromHex(n.get("TextColor"), s.textColor);
    s.width = (float)n.getd("PenWidth", s.width);
    s.opacity = n.geti("Opacity", 100);
    s.shadow = n.getb("Shadow", true);
    s.arrowStart = n.geti("ArrowStartCap", 0);
    s.arrowEnd = n.geti("ArrowEndCap", 1);
    s.font = n.getW("FaceName", s.font);
    s.fontSize = (float)n.getd("FontSize", s.fontSize);
    s.bold = n.getb("Bold", true);
    s.calloutShape = n.geti("CalloutShape", 0);
    s.amount = n.geti("Amount", 10);
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
    if (t == ObjType::Picture) {
        auto it = frame.embedded.find(n.get("StreamName"));
        if (it == frame.embedded.end()) return nullptr;
        o->picture = std::make_shared<Image>(it->second);
    }
    return o;
}

} // namespace snap::ed
