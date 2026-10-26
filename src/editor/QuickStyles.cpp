#include "QuickStyles.h"
#include "Icons.h"
#include <cmath>

namespace snap::ed {

using Gdiplus::PointF;
using Gdiplus::RectF;

QuickStyleStore& QS() { static QuickStyleStore s; return s; }

static std::wstring ThemesFile() { return AppDataDir() + L"\\ToolThemes.xml"; }
static std::wstring FavoritesFile() { return AppDataDir() + L"\\Favorites.xml"; }
static std::wstring ToolStylesFile() { return AppDataDir() + L"\\ToolStyles.xml"; }

// ---------------------------------------------------------------- style ⇄ XML (.snagtheme fields)
static std::string Hex(uint32_t v) { char b[16]; snprintf(b, sizeof b, "#%08X", v); return b; }
static uint32_t FromHexA(const std::string& s, uint32_t def) {
    if (s.size() < 2 || s[0] != '#') return def;
    unsigned long v = strtoul(s.c_str() + 1, nullptr, 16);
    return s.size() == 7 ? (0xFF000000u | (uint32_t)v) : (uint32_t)v;
}

XmlNode StyleToXml(const Style& s, const char* name) {
    XmlNode n(name);
    n.set("Foreground", Hex(s.stroke)).set("Background", Hex(s.fill)).set("TextColor", Hex(s.textColor));
    n.setd("StrokeWidth", s.width).seti("Opacity", s.opacity).seti("DropShadowEnabled", s.shadow);
    n.seti("ShadowDir", s.shadowDir).set("ShadowColor", Hex(s.shadowColor)).seti("ShadowOpacity", s.shadowOpacity);
    n.seti("ShadowBlur", s.shadowBlur).seti("ShadowDist", s.shadowDist);
    n.setW("FontFamily", s.font).setd("FontSize", s.fontSize).seti("Bold", s.bold).seti("Italic", s.italic).seti("Underline", s.underline);
    n.seti("AlignH", s.alignH).seti("AlignV", s.alignV).setd("TextPadding", s.padding);
    n.seti("LineStartCap", s.arrowStart).seti("LineEndCap", s.arrowEnd).setd("StartSize", s.startSize).setd("EndSize", s.endSize);
    n.seti("DashType", s.dash).seti("Bezier", s.bezier).seti("CalloutShape", s.calloutShape).seti("CalloutGradient", s.shaded);
    n.set("ArrowColor", Hex(s.tailColor)).setd("ArrowSize", s.tailWidth).seti("ToolShape", s.shapeKind);
    n.seti("StepStyle", s.stepShape).seti("StepSequenceType", s.stepType).seti("MagnifyShape", s.magnifyShape);
    n.seti("MagnifyScale", s.magnifyScale).seti("MagnifyLink", s.magnifyLink).seti("HideKind", s.hideKind);
    n.seti("Antialiasing", s.antialias).seti("Amount", s.amount);
    n.setd("LineSpacing", s.lineSpacing).set("StepOutlineColor", Hex(s.stepOutline)).setd("StepOutlineWidth", s.stepOutlineWidth);
    n.seti("AreaShape", s.areaShape).seti("BackgroundBlur", s.bgBlur).seti("BackgroundBlurAmount", s.bgBlurAmount);
    n.seti("Loupe", s.loupe).seti("LoupeHandle", s.loupeHandle);
    GradToXml(n, s.grad);
    return n;
}

Style StyleFromXml(const XmlNode& n) {
    Style s;
    s.stroke = FromHexA(n.get("Foreground"), s.stroke);
    s.fill = FromHexA(n.get("Background"), s.fill);
    s.textColor = FromHexA(n.get("TextColor"), s.textColor);
    s.width = (float)n.getd("StrokeWidth", s.width);
    s.opacity = std::clamp(n.geti("Opacity", s.opacity), 0, 100);
    s.shadow = n.getb("DropShadowEnabled", s.shadow);
    s.shadowDir = std::clamp(n.geti("ShadowDir", s.shadowDir), 0, 8);
    s.shadowColor = FromHexA(n.get("ShadowColor"), s.shadowColor);
    s.shadowOpacity = n.geti("ShadowOpacity", s.shadowOpacity);
    s.shadowBlur = n.geti("ShadowBlur", s.shadowBlur);
    s.shadowDist = n.geti("ShadowDist", s.shadowDist);
    s.font = n.getW("FontFamily", s.font);
    s.fontSize = (float)n.getd("FontSize", s.fontSize);
    s.bold = n.getb("Bold", s.bold); s.italic = n.getb("Italic"); s.underline = n.getb("Underline");
    s.alignH = std::clamp(n.geti("AlignH", s.alignH), 0, 2); s.alignV = std::clamp(n.geti("AlignV", s.alignV), 0, 2);
    s.padding = (float)n.getd("TextPadding", s.padding);
    s.arrowStart = std::clamp(n.geti("LineStartCap", s.arrowStart), 0, CapCount - 1);
    s.arrowEnd = std::clamp(n.geti("LineEndCap", s.arrowEnd), 0, CapCount - 1);
    s.startSize = (float)n.getd("StartSize", s.startSize); s.endSize = (float)n.getd("EndSize", s.endSize);
    s.dash = std::clamp(n.geti("DashType", 0), 0, DashCount - 1);
    s.bezier = n.getb("Bezier");
    s.calloutShape = std::clamp(n.geti("CalloutShape", s.calloutShape), 0, kCalloutShapes - 1);
    s.shaded = n.getb("CalloutGradient");
    s.tailColor = FromHexA(n.get("ArrowColor"), s.tailColor);
    s.tailWidth = (float)n.getd("ArrowSize", s.tailWidth);
    s.shapeKind = std::clamp(n.geti("ToolShape", 0), 0, ShapeCount - 1);
    s.stepShape = std::clamp(n.geti("StepStyle", 0), 0, 3);
    s.stepType = std::clamp(n.geti("StepSequenceType", 0), 0, StepTypeCount - 1);
    s.magnifyShape = std::clamp(n.geti("MagnifyShape", 0), 0, MagnifyKindCount - 1);
    s.magnifyScale = std::clamp(n.geti("MagnifyScale", 200), 110, 1000);
    s.magnifyLink = n.getb("MagnifyLink");
    s.hideKind = std::clamp(n.geti("HideKind", 0), 0, 1);
    s.antialias = n.getb("Antialiasing", true);
    s.amount = n.geti("Amount", s.amount);
    s.lineSpacing = std::clamp((float)n.getd("LineSpacing", 1.0), 0.8f, 3.0f);
    s.stepOutline = FromHexA(n.get("StepOutlineColor"), s.stepOutline);
    s.stepOutlineWidth = std::clamp((float)n.getd("StepOutlineWidth", 0), 0.0f, 40.0f);
    s.areaShape = std::clamp(n.geti("AreaShape", ShapeRect), 0, AreaShapeCount - 1);
    s.bgBlur = n.getb("BackgroundBlur");
    s.bgBlurAmount = std::clamp(n.geti("BackgroundBlurAmount", 8), 1, 40);
    s.loupe = n.getb("Loupe");
    s.loupeHandle = n.getb("LoupeHandle", true);
    s.grad = GradFromXml(n);
    return s;
}

bool SameStyle(const Style& a, const Style& b) {
    return XmlSerialize(StyleToXml(a), false) == XmlSerialize(StyleToXml(b), false);
}

// ---------------------------------------------------------------- generating a theme's styles
static const uint32_t kBlack = 0xFF1E1E1E, kWhite = 0xFFFFFFFF, kGray = 0xFF9E9E9E;

std::vector<Style> GenerateStyles(Tool t, const std::array<uint32_t, 4>& c) {
    std::vector<Style> v;
    Style b;
    switch (t) {
    case Tool::Arrow:
        for (int row = 0; row < 3; row++)
            for (int i = 0; i < 4; i++) {
                Style s = b; s.stroke = c[i]; s.width = 8;
                if (row == 0) { s.arrowStart = CapFlat; s.arrowEnd = CapArrow; }
                if (row == 1) { s.arrowStart = CapRound; s.arrowEnd = CapArrowWide; s.width = 6; }
                if (row == 2) { s.arrowStart = CapArrow; s.arrowEnd = CapArrow; s.dash = DashDot; s.width = 6; s.startSize = s.endSize = 2.5f; }
                v.push_back(s);
            }
        break;
    case Tool::Line:
        for (int row = 0; row < 3; row++)
            for (int i = 0; i < 4; i++) {
                Style s = b; s.stroke = c[i]; s.arrowStart = s.arrowEnd = CapFlat; s.width = 4;
                if (row == 1) s.dash = DashDash;
                if (row == 2) { s.width = 8; s.arrowStart = s.arrowEnd = CapRound; }
                v.push_back(s);
            }
        break;
    case Tool::Pen:
        for (int row = 0; row < 2; row++)
            for (int i = 0; i < 4; i++) { Style s = b; s.stroke = c[i]; s.width = row ? 10.0f : 4.0f; v.push_back(s); }
        break;
    case Tool::Highlighter: {
        const uint32_t hl[4] = { 0xFFFFEB3B, 0xFF76FF03, 0xFFFF4081, 0xFF40C4FF };
        for (int i = 0; i < 8; i++) { Style s = b; s.stroke = i < 4 ? hl[i] : c[i - 4]; s.opacity = i < 4 ? 50 : 40; s.shadow = false; v.push_back(s); }
        for (int i = 0; i < 4; i++) { Style s = b; s.stroke = hl[i]; s.opacity = 45; s.shadow = false; s.areaShape = i < 2 ? ShapeEllipse : ShapeRoundRect; v.push_back(s); }
        break;
    }
    case Tool::Text: {
        const uint32_t neu[4][2] = { { kBlack, kWhite }, { kWhite, kBlack }, { kBlack, 0 }, { kGray, 0 } };
        for (int row = 0; row < 4; row++)
            for (int i = 0; i < 4; i++) {
                Style s = b; s.fontSize = 36; s.bold = true; s.alignH = s.alignV = 1;
                if (row % 2 == 0) { s.textColor = c[i]; s.stroke = kWhite; }
                else { s.textColor = neu[i][0]; s.stroke = neu[i][1] ? neu[i][1] : 0x00FFFFFF; }
                s.width = row < 2 ? 4.0f : 8.0f;
                if (row % 2 == 1 && !neu[i][1]) s.width = 0;
                s.shadow = row >= 2;
                v.push_back(s);
            }
        break;
    }
    case Tool::Callout:
        for (int row = 0; row < 5; row++)
            for (int i = 0; i < 4; i++) {
                Style s = b; s.fontSize = 20; s.bold = true; s.padding = 8; s.textColor = kWhite;
                s.fill = c[i]; s.stroke = c[i]; s.width = 0; s.tailColor = c[i]; s.tailWidth = 6;
                static const int shapes[5] = { 4, 12, 6, 12, 0 };
                s.calloutShape = shapes[row];
                if (row == 3) { s.fill = kWhite; s.width = 3; s.textColor = 0xFF202020; }
                v.push_back(s);
            }
        break;
    case Tool::Shape:
        for (int i = 0; i < 4; i++) { Style s = b; s.stroke = c[i]; s.width = 4; s.shapeKind = ShapeRect; v.push_back(s); }
        for (int i = 0; i < 4; i++) { Style s = b; s.stroke = c[i]; s.width = 4; s.shapeKind = i < 2 ? ShapeEllipse : ShapeRoundRect; v.push_back(s); }
        {
            Style s = b; s.shapeKind = ShapeRect; s.width = 0;
            s.fill = kBlack; s.stroke = kBlack; v.push_back(s);
            s.fill = kWhite; s.stroke = kWhite; v.push_back(s);
            s.fill = 0; s.width = 6; s.stroke = kBlack; v.push_back(s);
            s.stroke = kWhite; v.push_back(s);
            Style p = b; p.shapeKind = ShapeParallelogram; p.width = 0; p.fill = kBlack; p.stroke = kBlack; v.push_back(p);
            p.fill = kWhite; p.stroke = kWhite; v.push_back(p);
        }
        break;
    case Tool::Step:
        for (int row = 0; row < 4; row++)
            for (int i = 0; i < 4; i++) {
                Style s = b; s.stroke = c[i]; s.textColor = kWhite; s.fontSize = 26;
                s.stepShape = row % 2 ? 1 : 0; s.stepType = row >= 2 ? StepUpper : StepNumbers;
                v.push_back(s);
            }
        // with an outline (contrast on light and dark backgrounds): white outline, and white steps outlined in the color
        for (int row = 0; row < 2; row++)
            for (int i = 0; i < 4; i++) {
                Style s = b; s.fontSize = 26; s.stepOutlineWidth = 3;
                if (row == 0) { s.stroke = c[i]; s.textColor = kWhite; s.stepOutline = kWhite; }
                else { s.stroke = kWhite; s.textColor = c[i]; s.stepOutline = c[i]; }
                v.push_back(s);
            }
        break;
    case Tool::Fill: {
        const uint32_t extra[4] = { kWhite, 0xFFC8C8C8, 0xFF646464, 0xFF000000 };
        for (int i = 0; i < 8; i++) { Style s = b; s.stroke = i < 4 ? c[i] : extra[i - 4]; s.shadow = false; v.push_back(s); }
        break;
    }
    case Tool::Eraser: {
        const int w[8] = { 4, 8, 12, 16, 24, 32, 48, 64 };
        for (int x : w) { Style s = b; s.width = (float)x; s.shadow = false; v.push_back(s); }
        break;
    }
    case Tool::Magnify:
        for (int row = 0; row < 4; row++)
            for (int i = 0; i < 4; i++) {
                Style s = b; s.stroke = c[i]; s.width = 4; s.magnifyScale = 200;
                s.magnifyShape = row >= 2 ? 1 : 0; s.magnifyLink = row % 2 == 1;
                v.push_back(s);
            }
        {                                         // loupe: graphite, silver, gold and the theme's first color, with a handle
            const uint32_t rim[4] = { 0xFF3C4248, 0xFFB8BEC6, 0xFFC9A227, c[0] };
            for (int i = 0; i < 4; i++) { Style s = b; s.stroke = rim[i]; s.width = 4; s.magnifyScale = 200; s.loupe = true; s.loupeHandle = true; v.push_back(s); }
            for (int i = 0; i < 4; i++) { Style s = b; s.stroke = rim[i]; s.width = 4; s.magnifyScale = 250; s.loupe = true; s.loupeHandle = false; s.magnifyShape = i < 2 ? MagRoundRect : MagHexagon; v.push_back(s); }
        }
        break;
    case Tool::Blur:
        for (int k = 0; k < 6; k++) { Style s = b; s.shadow = false; s.hideKind = k / 3; s.amount = (k % 3 + 1) * 6; v.push_back(s); }
        for (int k = 0; k < 2; k++) { Style s = b; s.shadow = false; s.hideKind = k; s.amount = 12; s.areaShape = ShapeEllipse; v.push_back(s); }
        break;
    case Tool::Spotlight:
        for (int k = 0; k < 3; k++) { Style s = b; s.shadow = false; s.amount = 6 + k * 4; v.push_back(s); }
        { Style s = b; s.shadow = false; s.amount = 8; s.areaShape = ShapeEllipse; v.push_back(s); }
        break;
    default: break;
    }
    return v;
}

// ---------------------------------------------------------------- store
static ToolTheme MakeTheme(const wchar_t* name, std::array<uint32_t, 4> c, bool builtin) {
    ToolTheme t; t.name = name; t.colors = c; t.builtin = builtin; return t;
}

static std::vector<ToolTheme> BuiltinThemes() {
    return { MakeTheme(TR_NOOP(L"Básico"), { 0xFFEF4136, 0xFF6A2C91, 0xFF15A589, 0xFFF7A823 }, true),
             MakeTheme(TR_NOOP(L"Industrial"), { 0xFFF5B71E, 0xFFF2845C, 0xFF2B3442, 0xFF9A8894 }, true) };
}

static XmlNode ThemeToXml(const ToolTheme& t) {
    XmlNode n("Theme");
    n.setW("Name", t.name).seti("Builtin", t.builtin).set("Version", "2.0");
    for (int i = 0; i < 4; i++) n.set(FormatA("Color%d", i), Hex(t.colors[i]));
    for (auto& [tool, list] : t.custom) {
        XmlNode& q = n.add("QuickStyles");
        q.seti("ToolMode", tool);
        for (auto& s : list) q.children.push_back(StyleToXml(s));
    }
    return n;
}

static ToolTheme ThemeFromXml(const XmlNode& n) {
    ToolTheme t;
    t.name = n.getW("Name", Tr(L"Tema"));
    t.builtin = n.getb("Builtin");
    for (int i = 0; i < 4; i++) t.colors[i] = FromHexA(n.get(FormatA("Color%d", i)), 0xFF808080);
    for (auto& q : n.children) {
        if (q.name != "QuickStyles") continue;
        auto& list = t.custom[q.geti("ToolMode")];
        for (auto& s : q.children) list.push_back(StyleFromXml(s));
    }
    return t;
}

void QuickStyleStore::Load() {
    themes.clear();
    XmlNode root;
    if (XmlLoadFile(ThemesFile(), root)) {
        for (auto& c : root.children) if (c.name == "Theme") themes.push_back(ThemeFromXml(c));
        current = root.geti("Current", 0);
    }
    for (auto& b : BuiltinThemes()) {           // the built-in themes always exist (with the original colors)
        auto it = std::find_if(themes.begin(), themes.end(), [&](auto& t) { return t.builtin && t.name == b.name; });
        if (it == themes.end()) themes.insert(themes.begin() + std::min<size_t>(themes.size(), b.name == L"Básico" ? 0 : 1), b);
        else it->colors = b.colors;
    }
    current = std::clamp(current, 0, (int)themes.size() - 1);
    favorites.clear();
    XmlNode fav;
    if (XmlLoadFile(FavoritesFile(), fav)) {
        for (auto& c : fav.children) {
            if (c.name != "Favorite" || c.children.empty()) continue;
            int t = c.geti("ToolMode", -1);
            if (t < 0 || t >= (int)Tool::Count) continue;
            favorites.push_back({ (Tool)t, StyleFromXml(c.children[0]) });
        }
    } else {                                    // LoadDefaultFavorites: the theme's arrow, text, rectangle and ellipse
        auto a = GenerateStyles(Tool::Arrow, themes[0].colors), tx = GenerateStyles(Tool::Text, themes[0].colors);
        auto sh = GenerateStyles(Tool::Shape, themes[0].colors);
        favorites = { { Tool::Arrow, a[0] }, { Tool::Text, tx[0] }, { Tool::Shape, sh[0] }, { Tool::Shape, sh[4] } };
    }
}

void QuickStyleStore::Save() const {
    XmlNode root("ToolThemes");
    root.seti("Current", current);
    for (auto& t : themes) root.children.push_back(ThemeToXml(t));
    XmlSaveFile(ThemesFile(), root);
}

void QuickStyleStore::SaveFavorites() const {
    XmlNode root("Favorites");
    for (auto& f : favorites) {
        XmlNode& n = root.add("Favorite");
        n.seti("ToolMode", (int)f.tool);
        n.children.push_back(StyleToXml(f.style));
    }
    XmlSaveFile(FavoritesFile(), root);
}

bool QuickStyleStore::HasQuickStyles(Tool t) const { return !GenerateStyles(t, Theme().colors).empty(); }
int QuickStyleStore::GeneratedCount(Tool t) const { return (int)GenerateStyles(t, Theme().colors).size(); }

std::vector<Style> QuickStyleStore::StylesFor(Tool t) const {
    std::vector<Style> v = GenerateStyles(t, Theme().colors);
    auto it = Theme().custom.find((int)t);
    if (it != Theme().custom.end()) v.insert(v.end(), it->second.begin(), it->second.end());
    return v;
}

void QuickStyleStore::AddCustom(Tool t, const Style& s) {
    for (auto& e : StylesFor(t)) if (SameStyle(e, s)) return;
    themes[current].custom[(int)t].push_back(s);
    Save();
}

void QuickStyleStore::RemoveCustom(Tool t, int i) {
    auto& list = themes[current].custom[(int)t];
    if (i >= 0 && i < (int)list.size()) list.erase(list.begin() + i);
    Save();
}

bool QuickStyleStore::IsFavorite(Tool t, const Style& s) const {
    for (auto& f : favorites) if (f.tool == t && SameStyle(f.style, s)) return true;
    return false;
}

void QuickStyleStore::ToggleFavorite(Tool t, const Style& s) {
    for (auto it = favorites.begin(); it != favorites.end(); ++it)
        if (it->tool == t && SameStyle(it->style, s)) { favorites.erase(it); SaveFavorites(); return; }
    favorites.push_back({ t, s });
    SaveFavorites();
}

int QuickStyleStore::AddTheme(ToolTheme th) {
    std::wstring base = th.name.empty() ? Tr(L"Tema") : th.name;
    th.name = base;
    for (int n = 2; std::any_of(themes.begin(), themes.end(), [&](auto& t) { return t.name == th.name; }); n++)
        th.name = base + Format(L" (%d)", n);
    th.builtin = false;
    themes.push_back(th);
    current = (int)themes.size() - 1;
    Save();
    return current;
}

bool QuickStyleStore::ImportTheme(const std::wstring& path, std::wstring& err) {
    XmlNode n;
    if (!XmlLoadFile(path, n) || n.name != "Theme") { err = Tr(L"O arquivo não é um tema do Snapnab (.snaptheme)."); return false; }
    AddTheme(ThemeFromXml(n));
    return true;
}

bool QuickStyleStore::ExportTheme(int i, const std::wstring& path) const {
    if (i < 0 || i >= (int)themes.size()) return false;
    ToolTheme t = themes[i];
    t.builtin = false;
    return XmlSaveFile(path, ThemeToXml(t));
}

void QuickStyleStore::DeleteTheme(int i) {
    if (i < 0 || i >= (int)themes.size() || themes[i].builtin) return;
    themes.erase(themes.begin() + i);
    current = std::clamp(current >= i ? current - 1 : current, 0, (int)themes.size() - 1);
    Save();
}

void SaveToolStyles(const std::map<Tool, Style>& styles) {
    XmlNode root("ToolStyles");
    for (auto& [t, s] : styles) {
        XmlNode n = StyleToXml(s, "Tool");
        n.seti("ToolMode", (int)t);
        root.children.push_back(n);
    }
    XmlSaveFile(ToolStylesFile(), root);
}

void LoadToolStyles(std::map<Tool, Style>& styles) {
    XmlNode root;
    if (!XmlLoadFile(ToolStylesFile(), root)) return;
    for (auto& n : root.children) {
        int t = n.geti("ToolMode", -1);
        if (t >= 0 && t < (int)Tool::Count) styles[(Tool)t] = StyleFromXml(n);
    }
}

// ---------------------------------------------------------------- thumbnails
static Image Landscape(int s) {       // background of the magnifier thumbnails: sky, mountains and snow
    Image img(s, s, 0xFF7FB2E5);
    auto bmp = img.WrapGdip();
    Gdiplus::Graphics g(bmp.get());
    g.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
    Gdiplus::LinearGradientBrush sky(PointF(0, 0), PointF(0, (float)s), Gdiplus::Color(255, 120, 170, 225), Gdiplus::Color(255, 200, 225, 245));
    g.FillRectangle(&sky, 0, 0, s, s);
    PointF m1[] = { { 0, s * 0.75f }, { s * 0.3f, s * 0.35f }, { s * 0.55f, s * 0.65f }, { s * 0.8f, s * 0.3f }, { (float)s, s * 0.6f }, { (float)s, (float)s }, { 0, (float)s } };
    Gdiplus::SolidBrush rock(Gdiplus::Color(255, 70, 90, 120));
    g.FillPolygon(&rock, m1, 7);
    PointF snow[] = { { s * 0.22f, s * 0.47f }, { s * 0.3f, s * 0.35f }, { s * 0.38f, s * 0.47f } };
    Gdiplus::SolidBrush w(Gdiplus::Color(255, 245, 248, 252));
    g.FillPolygon(&w, snow, 3);
    PointF snow2[] = { { s * 0.72f, s * 0.42f }, { s * 0.8f, s * 0.3f }, { s * 0.88f, s * 0.42f } };
    g.FillPolygon(&w, snow2, 3);
    Gdiplus::SolidBrush grass(Gdiplus::Color(255, 60, 110, 70));
    g.FillRectangle(&grass, 0.0f, s * 0.85f, (float)s, s * 0.15f);
    g.Flush(Gdiplus::FlushIntentionSync);
    return img;
}

static Image Lines(int s) {           // background of blur/highlight: "text" in lines
    Image img(s, s, 0xFFFFFFFF);
    for (int y = s / 8; y < s - s / 8; y += s / 8)
        for (int x = s / 10; x < s - s / 10; x++)
            for (int k = 0; k < std::max(2, s / 24); k++)
                if (((x / (s / 12 + 1)) % 3) != 2) img.at(x, y + k) = (x / 3 + y) % 2 ? 0xFF3A5A8A : 0xFF2D2D30;
    return img;
}

Image QuickStylePreview(Tool t, const Style& st, int px) {
    const int S = 96;
    Image canvas(S, S, 0);
    Style s = st;
    s.shadowDist = std::max(2, s.shadowDist / 2); s.shadowBlur = std::max(2, s.shadowBlur / 2);
    auto obj = std::make_shared<DrawObj>();
    obj->style = s;
    bool draw = true;
    switch (t) {
    case Tool::Arrow: case Tool::Line:
        obj->type = t == Tool::Arrow ? ObjType::Arrow : ObjType::Line;
        obj->p1 = { 16, 80 }; obj->p2 = { 80, 16 };
        obj->style.width = std::clamp(s.width, 3.0f, 12.0f);
        break;
    case Tool::Pen:
        obj->type = ObjType::Pen;
        for (int i = 0; i <= 16; i++) obj->pts.push_back(PointF(14 + i * 4.2f, 48 - (float)std::sin(i * 0.5) * 22));
        obj->style.width = std::clamp(s.width, 3.0f, 12.0f);
        break;
    case Tool::Highlighter:
        canvas = Lines(S);
        obj->type = ObjType::Highlighter; obj->p1 = { 8, 30 }; obj->p2 = { 88, 66 };
        if (s.areaShape != ShapeRect) { obj->p1 = { 12, 16 }; obj->p2 = { 84, 80 }; }      // taller: the shape shows
        if (IsPointArea(s.areaShape)) obj->pts = { { 10, 40 }, { 46, 22 }, { 86, 34 }, { 74, 70 }, { 22, 72 } };
        break;
    case Tool::Shape:
        obj->type = ObjType::Shape; obj->p1 = { 14, 24 }; obj->p2 = { 82, 72 };
        obj->style.width = std::clamp(s.width, 0.0f, 8.0f);
        if (s.width > 0 && s.width < 3) obj->style.width = 3;
        break;
    case Tool::Text:
        obj->type = ObjType::Text; obj->p1 = { 0, 0 }; obj->p2 = { 96, 96 }; obj->text = L"A";
        obj->style.fontSize = 66; obj->style.alignH = obj->style.alignV = 1; obj->style.padding = 0;
        obj->style.width = s.width > 0 ? std::clamp(s.width * 0.9f, 3.0f, 9.0f) : 0;
        break;
    case Tool::Callout: {
        obj->type = ObjType::Callout; obj->text = L"A";
        obj->style.fontSize = 22; obj->style.padding = 2; obj->style.tailWidth = 5; obj->style.endSize = 2.5f;
        obj->style.width = std::min(s.width, 4.0f);
        int sh = s.calloutShape;
        if (sh >= 5 && sh <= 11) { obj->p1 = { 10, 26 }; obj->p2 = { 86, 70 }; obj->tail = { 120, 48 }; }
        else if (sh == 4 || sh == 15) { obj->p1 = { 34, 12 }; obj->p2 = { 86, 50 }; obj->tail = { 12, 84 }; }
        else if (sh >= 16 && sh <= 21) { obj->p1 = { 12, 20 }; obj->p2 = { 84, 76 }; obj->tail = { 48, 48 }; }
        else { obj->p1 = { 14, 12 }; obj->p2 = { 82, 62 }; obj->tail = { 28, 86 }; }
        break;
    }
    case Tool::Step:
        obj->type = ObjType::Step; obj->p1 = obj->p2 = s.stepShape == 1 ? PointF(40, 48) : PointF(48, 48);
        obj->style.fontSize = 40; obj->stepValue = 1;
        break;
    case Tool::Magnify: {
        canvas = Landscape(S);
        obj->type = ObjType::Magnify; obj->style.width = std::clamp(s.width, 2.0f, 5.0f);
        if (s.magnifyLink) { obj->p1 = { 44, 8 }; obj->p2 = { 88, 52 }; obj->tail = { 26, 70 }; }
        else if (s.loupe && s.loupeHandle) { obj->p1 = { 10, 10 }; obj->p2 = { 64, 64 }; }
        else { obj->p1 = { 18, 18 }; obj->p2 = { 78, 78 }; }
        break;
    }
    case Tool::Blur: case Tool::Spotlight:
        canvas = Lines(S);
        obj->type = t == Tool::Spotlight ? ObjType::Spotlight : s.hideKind ? ObjType::Pixelate : ObjType::Blur;
        obj->p1 = { 20, 20 }; obj->p2 = { 76, 76 };
        obj->style.amount = std::max(2, s.amount / 2);
        if (IsPointArea(s.areaShape)) obj->pts = { { 22, 30 }, { 52, 14 }, { 82, 34 }, { 70, 80 }, { 18, 70 } };
        break;
    case Tool::Fill: case Tool::Eraser: draw = false; break;
    default: draw = false; break;
    }
    if (draw) {
        auto bmp = canvas.WrapGdip();
        Gdiplus::Graphics g(bmp.get());
        g.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
        RenderObject(g, canvas, *obj);
        g.Flush(Gdiplus::FlushIntentionSync);
    } else if (t == Tool::Fill) {      // paint bucket in the color
        auto bmp = canvas.WrapGdip();
        Gdiplus::Graphics g(bmp.get());
        g.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
        Gdiplus::Color c = Gc(s.stroke);
        Gdiplus::GraphicsPath gp;
        PointF body[] = { { 20, 46 }, { 48, 18 }, { 76, 46 }, { 48, 74 } };
        gp.AddPolygon(body, 4);
        auto b = FillBrush(s.stroke, s.grad, Gdiplus::RectF(20, 18, 56, 56));
        g.FillPath(b.get(), &gp);
        Gdiplus::Pen pen(Gdiplus::Color(255, std::max(0, c.GetR() - 70), std::max(0, c.GetG() - 70), std::max(0, c.GetB() - 70)), 3);
        if ((s.stroke & 0xFFFFFF) == 0xFFFFFF) pen.SetColor(Gdiplus::Color(255, 150, 150, 150));
        g.DrawPath(&pen, &gp);
        g.FillEllipse(b.get(), 70.0f, 56.0f, 14.0f, 20.0f);
        g.DrawEllipse(&pen, 70.0f, 56.0f, 14.0f, 20.0f);
        g.Flush(Gdiplus::FlushIntentionSync);
    } else if (t == Tool::Eraser) {    // brush size: checkerboard inside the circle
        float d = std::clamp(s.width * 1.2f, 6.0f, 80.0f);
        for (int y = 0; y < S; y++)
            for (int x = 0; x < S; x++) {
                float dx = x + 0.5f - 48, dy = y + 0.5f - 48;
                float r = std::sqrt(dx * dx + dy * dy);
                if (r <= d / 2) canvas.at(x, y) = ((x / 6 + y / 6) % 2) ? 0xFFFFFFFF : 0xFFD0D0D4;
                if (std::fabs(r - d / 2) < 1.2f) canvas.at(x, y) = 0xFF8A8A90;
            }
    }
    return ScaleImage(canvas, px, px);
}

} // namespace snap::ed
