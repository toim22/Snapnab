// Editor properties panel: quick styles (theme, gallery, favorites), stamp gallery and the
// properties of each tool as in the reference (ToolProperties.*, ShadowPropertiesControl, Quick Styles).
#include "Panels.h"
#include "Popups.h"
#include "QuickStyles.h"
#include "Stamps.h"
#include "Emoji.h"
#include "Ui.h"
#include "Widgets.h"
#include <cmath>
#include <set>

namespace snap::ed {

using ui::Mix;

namespace {
enum Kind { K_SWATCH, K_SHADOW, K_SHAPE, K_CAPSTART, K_DASH, K_CAPEND, K_SLIDER, K_CHECK, K_TOGGLE, K_EXPANDER, K_TOGGLEBTN,
            K_LABEL, K_ICONBTN, K_ROWLABEL, K_FRAME };
enum : int {
    C_COLOR = 1, C_FILL, C_OUTLINE, C_TEXTCOLOR, C_TAILCOLOR, C_EYEDROP, C_SHADOW, C_SHAPE, C_CAPSTART, C_DASH, C_CAPEND,
    C_WIDTH, C_OPACITY, C_STARTSIZE, C_ENDSIZE, C_BEZIER, C_FONTSIZE, C_OUTLINEW, C_TAILSIZE, C_PADDING, C_ADVANCED,
    C_BOLD, C_ITALIC, C_UNDERLINE, C_AL_L, C_AL_C, C_AL_R, C_AL_T, C_AL_M, C_AL_B, C_SHADED, C_MAGSCALE, C_MAGLINK,
    C_ANTIALIAS, C_TOLERANCE, C_AMOUNT, C_SMARTMOVE, C_GLOBAL, C_STEPRESET, C_ROWLABEL, C_HINT,
    C_LINESPACING, C_BGBLUR, C_BGBLURAMT, C_FILLALPHA, C_STEPOUTLINE, C_STEPOUTLINEW, C_AREA, C_LOUPE, C_LOUPEHANDLE, C_ASPECT,
};
enum : int {
    H_THEME = 400, H_SEARCH, H_STAMPCAT, H_FONT, H_STEPTYPE, H_SELMODE, H_SELFILL, H_CUTDIR, H_HIDEKIND, H_ORGANIZE, H_IMPORT,
    H_GX = 412, H_GY, H_GW, H_GH, H_LOCK, H_LIFT, H_DROP,
    H_BTN1 = 324, H_BTN2, H_BTN3, H_FRONT, H_BACK, H_DELETE, H_GROUP, H_UNGROUP, H_EDIT = 600,
};
// order of the ends in the list (as in the reference): arrow, wide arrow, open arrow, flat, round, dot, square, diamond
const int kCapOrder[CapCount] = { CapArrow, CapArrowWide, CapArrowOpen, CapFlat, CapRound, CapDot, CapSquare, CapDiamond };
// magnifier shapes (MagnifyShape) as ShapeKind outlines
int MagKind(int m) {
    static const int k[MagnifyKindCount] = { ShapeEllipse, ShapeRect, ShapeRoundRect, ShapeHexagon, ShapeDiamond, ShapeStar };
    return k[std::clamp(m, 0, MagnifyKindCount - 1)];
}

// sample outline of an area shape (Highlighter, Spotlight, Hide): the shapes plus a polygon and a freehand outline
void AreaSample(Gdiplus::GraphicsPath& gp, int area, Gdiplus::RectF r) {
    if (area == AreaPolygon) {
        Gdiplus::PointF p[] = { { r.X + r.Width * 0.1f, r.Y + r.Height * 0.3f }, { r.X + r.Width * 0.55f, r.Y }, { r.GetRight(), r.Y + r.Height * 0.45f },
                                { r.X + r.Width * 0.7f, r.GetBottom() }, { r.X + r.Width * 0.2f, r.Y + r.Height * 0.85f } };
        gp.AddPolygon(p, 5);
    } else if (area == AreaFreehand) {
        Gdiplus::PointF p[] = { { r.X + r.Width * 0.15f, r.Y + r.Height * 0.4f }, { r.X + r.Width * 0.45f, r.Y + r.Height * 0.05f }, { r.X + r.Width * 0.9f, r.Y + r.Height * 0.2f },
                                { r.GetRight(), r.Y + r.Height * 0.7f }, { r.X + r.Width * 0.55f, r.GetBottom() }, { r.X, r.Y + r.Height * 0.8f } };
        gp.AddClosedCurve(p, 6, 0.6f);
    } else ShapePath(gp, std::clamp(area, 0, ShapeCount - 1), r);
}

const wchar_t* kShapeNames[ShapeCount] = { TR_NOOP(L"Retângulo"), TR_NOOP(L"Retângulo arredondado"), TR_NOOP(L"Elipse"), TR_NOOP(L"Triângulo"), TR_NOOP(L"Losango"),
                                           TR_NOOP(L"Paralelogramo"), TR_NOOP(L"Hexágono"), TR_NOOP(L"Estrela") };

std::wstring Filter(const wchar_t* f) { std::wstring s = f; std::replace(s.begin(), s.end(), L'|', L'\0'); return s; }
Gdiplus::Color GC(COLORREF c, BYTE a = 255) { return Gdiplus::Color(a, GetRValue(c), GetGValue(c), GetBValue(c)); }

void RoundPath(Gdiplus::GraphicsPath& p, Gdiplus::RectF r, float rad) {
    float d = std::min({ rad * 2, r.Width, r.Height });
    if (d < 1) { p.AddRectangle(r); return; }
    p.AddArc(r.X, r.Y, d, d, 180, 90); p.AddArc(r.GetRight() - d, r.Y, d, d, 270, 90);
    p.AddArc(r.GetRight() - d, r.GetBottom() - d, d, d, 0, 90); p.AddArc(r.X, r.GetBottom() - d, d, d, 90, 90);
    p.CloseFigure();
}
Gdiplus::RectF RF(RECT r) { return Gdiplus::RectF((float)r.left, (float)r.top, (float)(r.right - r.left), (float)(r.bottom - r.top)); }
void FillRound(Gdiplus::Graphics& g, RECT r, float rad, COLORREF c, BYTE a = 255) {
    Gdiplus::GraphicsPath p; RoundPath(p, RF(r), rad); Gdiplus::SolidBrush b(GC(c, a)); g.FillPath(&b, &p);
}
void StrokeRound(Gdiplus::Graphics& g, RECT r, float rad, COLORREF c, float w) {
    Gdiplus::GraphicsPath p;
    RoundPath(p, Gdiplus::RectF(r.left + w / 2, r.top + w / 2, (r.right - r.left) - w, (r.bottom - r.top) - w), rad);
    Gdiplus::Pen pen(GC(c), w); g.DrawPath(&pen, &p);
}
void Triangle(Gdiplus::Graphics& g, float cx, float cy, float s, COLORREF c) {
    Gdiplus::PointF p[3] = { { cx - s, cy - s / 2 }, { cx + s, cy - s / 2 }, { cx, cy + s / 2 } };
    Gdiplus::SolidBrush b(GC(c)); g.FillPolygon(&b, p, 3);
}

Tool ToolForObj(ObjType t) {
    switch (t) {
    case ObjType::Arrow: return Tool::Arrow;
    case ObjType::Line: return Tool::Line;
    case ObjType::Rect: case ObjType::Ellipse: case ObjType::RoundRect: case ObjType::Shape: return Tool::Shape;
    case ObjType::Pen: return Tool::Pen;
    case ObjType::Highlighter: return Tool::Highlighter;
    case ObjType::Text: return Tool::Text;
    case ObjType::Callout: return Tool::Callout;
    case ObjType::Step: return Tool::Step;
    case ObjType::Stamp: return Tool::Stamp;
    case ObjType::Blur: case ObjType::Pixelate: return Tool::Blur;
    case ObjType::Spotlight: return Tool::Spotlight;
    case ObjType::Magnify: return Tool::Magnify;
    default: return Tool::Select;
    }
}

const std::vector<std::wstring>& Fonts() {
    static std::vector<std::wstring> list = [] {
        std::vector<std::wstring> v;
        HDC dc = GetDC(nullptr);
        LOGFONTW lf{}; lf.lfCharSet = DEFAULT_CHARSET;
        EnumFontFamiliesExW(dc, &lf, [](const LOGFONTW* l, const TEXTMETRICW*, DWORD type, LPARAM p) -> int {
            if (l->lfFaceName[0] != L'@' && (type & TRUETYPE_FONTTYPE)) ((std::vector<std::wstring>*)p)->push_back(l->lfFaceName);
            return 1;
        }, (LPARAM)&v, 0);
        ReleaseDC(nullptr, dc);
        std::sort(v.begin(), v.end(), [](auto& a, auto& b) { return _wcsicmp(a.c_str(), b.c_str()) < 0; });
        v.erase(std::unique(v.begin(), v.end()), v.end());
        if (v.empty()) v = { L"Segoe UI", L"Arial" };
        return v;
    }();
    return list;
}

std::wstring HelpFor(Tool t) {
    switch (t) {
    case Tool::Arrow: return Tr(L"Arraste para desenhar uma seta. Shift trava em 45°. Com “Curva de Bézier”, arraste a alça do meio para curvar.");
    case Tool::Line: return Tr(L"Arraste para desenhar uma linha. Shift trava em 45°.");
    case Tool::Shape: return Tr(L"Arraste para desenhar a forma. Shift deixa a forma perfeita (quadrado, círculo, triângulo equilátero, hexágono regular).");
    case Tool::Pen: return Tr(L"Arraste para desenhar à mão livre.");
    case Tool::Highlighter: return Tr(L"Arraste sobre o que quer destacar. Em Forma, escolha retângulo, elipse, polígono (clique nos cantos) ou mão livre; Shift deixa a forma perfeita.");
    case Tool::Text: return Tr(L"Clique ou arraste para criar uma caixa de texto. Duplo clique edita o texto.");
    case Tool::Callout: return Tr(L"Arraste para criar o balão e digite. A alça amarela move a ponta (ou a direção das setas).");
    case Tool::Step: return Tr(L"Clique para numerar passos em sequência. O Tipo escolhe números, letras ou algarismos romanos.");
    case Tool::Stamp: return Tr(L"Escolha um carimbo na galeria e clique ou arraste na imagem. “Importar…” acrescenta imagens suas.");
    case Tool::Fill: return Tr(L"Clique para preencher a área de cor semelhante. O conta-gotas pega a cor de preenchimento da imagem.");
    case Tool::Eraser: return Tr(L"Arraste para apagar pixels da imagem (ficam transparentes).");
    case Tool::Magnify: return Tr(L"Arraste para criar a lupa. Com a lente separada, a alça amarela escolhe a área ampliada.");
    case Tool::Blur: return Tr(L"Arraste sobre a área a ocultar (desfoque ou pixelado). Em Forma, escolha elipse, polígono ou mão livre; Shift deixa a forma perfeita.");
    case Tool::Spotlight: return Tr(L"Arraste a área em destaque; o resto escurece. Em Forma, escolha elipse, polígono ou mão livre; Shift deixa a forma perfeita.");
    case Tool::Select: return Tr(L"Clique num objeto para selecionar e arrastar. Com Smart Move, arraste uma área da imagem para movê-la; o fundo é preenchido.");
    case Tool::Crop: return Tr(L"Arraste a área a manter; Enter ou duplo clique aplica.");
    case Tool::Selection: return Tr(L"Selecione e arraste para mover (Smart Move); Del apaga; Ctrl+C copia.");
    case Tool::Cutout: return Tr(L"Arraste sobre a faixa a remover; as partes são unidas.");
    case Tool::Eyedropper: return Tr(L"Clique na imagem para pegar a cor.");
    default: return L"";
    }
}
} // namespace

// ================================================================= state
bool PropsPanel::ObjSelected() const {
    return canvas && canvas->doc && canvas->selected >= 0 && canvas->selected < (int)canvas->doc->st.objs.size();
}

Tool PropsPanel::PropTool() const {
    if (ObjSelected()) return ToolForObj(canvas->doc->st.objs[canvas->selected]->type);
    return canvas->tool;
}

Tool PropsPanel::StyleTool() const {
    if (!ObjSelected() && barTool == Tool::Favorites) return Tool::Favorites;
    return PropTool();
}

Style PropsPanel::Cur() const {
    if (ObjSelected()) {
        const DrawObj& o = *canvas->doc->st.objs[canvas->selected];
        Style s = o.style;
        if (o.IsShape()) s.shapeKind = o.Kind();
        if (o.type == ObjType::Pixelate) s.hideKind = 1;
        return s;
    }
    return canvas->styles[PropTool()];
}

void PropsPanel::SetStyle(const Style& s, bool newStep) {
    Tool t = PropTool();
    canvas->styles[t] = s;
    if (ObjSelected()) canvas->ApplyStyleToSelected(s, newStep);
    if (onStylesChanged) onStylesChanged();
    InvalidateRect(hwnd, nullptr, FALSE);
}

void PropsPanel::SetBarTool(Tool t) { barTool = t; gridScroll_ = 0; scroll_ = 0; Load(); }

int PropsPanel::Value(int id) const {
    Style s = Cur();
    switch (id) {
    case C_WIDTH: case C_OUTLINEW: return (int)std::lround(s.width);
    case C_OPACITY: return s.opacity;
    case C_STARTSIZE: return (int)std::lround(s.startSize);
    case C_ENDSIZE: return (int)std::lround(s.endSize);
    case C_FONTSIZE: return (int)std::lround(s.fontSize);
    case C_TAILSIZE: return (int)std::lround(s.tailWidth);
    case C_PADDING: return (int)std::lround(s.padding);
    case C_MAGSCALE: return s.magnifyScale;
    case C_AMOUNT: return s.amount;
    case C_LINESPACING: return (int)std::lround(s.lineSpacing * 100);
    case C_BGBLUR: return s.bgBlur;
    case C_BGBLURAMT: return s.bgBlurAmount;
    case C_FILLALPHA: return (int)std::lround(CA(s.fill) * 100 / 255.0);
    case C_STEPOUTLINEW: return (int)std::lround(s.stepOutlineWidth);
    case C_LOUPE: return s.loupe;
    case C_LOUPEHANDLE: return s.loupeHandle;
    case C_ASPECT: return aspectLock_;
    case C_TOLERANCE: return (int)canvas->tolerance;
    case C_BEZIER: return s.bezier;
    case C_SHADED: return s.shaded;
    case C_MAGLINK: return s.magnifyLink;
    case C_ANTIALIAS: return s.antialias;
    case C_GLOBAL: return canvas->wandGlobal;
    case C_SMARTMOVE: return canvas->smartMove;
    case C_BOLD: return s.bold;
    case C_ITALIC: return s.italic;
    case C_UNDERLINE: return s.underline;
    case C_AL_L: case C_AL_C: case C_AL_R: return s.alignH == id - C_AL_L;
    case C_AL_T: case C_AL_M: case C_AL_B: return s.alignV == id - C_AL_T;
    default: return 0;
    }
}

void PropsPanel::SetValue(int id, int v, bool newStep) {
    switch (id) {
    case C_TOLERANCE: canvas->tolerance = (float)std::clamp(v, 0, 100); return;
    case C_GLOBAL: canvas->wandGlobal = v != 0; InvalidateRect(hwnd, nullptr, FALSE); return;
    case C_SMARTMOVE: canvas->smartMove = v != 0; if (!v) canvas->ClearRasterSelection(); InvalidateRect(hwnd, nullptr, FALSE); return;
    case C_ASPECT: aspectLock_ = v != 0; RegSetDword(L"GeometryAspectLock", aspectLock_, L"Editor"); InvalidateRect(hwnd, nullptr, FALSE); return;
    default: break;
    }
    Style s = Cur();
    switch (id) {
    case C_WIDTH: case C_OUTLINEW: s.width = (float)v; break;
    case C_OPACITY: s.opacity = std::clamp(v, 0, 100); break;
    case C_STARTSIZE: s.startSize = (float)v; break;
    case C_ENDSIZE: s.endSize = (float)v; break;
    case C_FONTSIZE: s.fontSize = (float)std::max(4, v); break;
    case C_TAILSIZE: s.tailWidth = (float)std::max(1, v); break;
    case C_PADDING: s.padding = (float)v; break;
    case C_MAGSCALE: s.magnifyScale = std::clamp(v, 110, 1000); break;
    case C_AMOUNT: s.amount = std::max(1, v); break;
    case C_LINESPACING: s.lineSpacing = std::clamp(v, 80, 300) / 100.0f; break;
    case C_BGBLUR: s.bgBlur = v != 0; break;
    case C_BGBLURAMT: s.bgBlurAmount = std::clamp(v, 1, 40); break;
    case C_FILLALPHA: s.fill = (s.fill & 0x00FFFFFF) | ((uint32_t)std::lround(std::clamp(v, 0, 100) * 2.55) << 24); break;
    case C_STEPOUTLINEW: s.stepOutlineWidth = (float)std::clamp(v, 0, 40); break;
    case C_LOUPE: s.loupe = v != 0; break;
    case C_LOUPEHANDLE: s.loupeHandle = v != 0; break;
    case C_BEZIER: s.bezier = v != 0; break;
    case C_SHADED: s.shaded = v != 0; break;
    case C_MAGLINK: s.magnifyLink = v != 0; break;
    case C_ANTIALIAS: s.antialias = v != 0; break;
    case C_BOLD: s.bold = v != 0; break;
    case C_ITALIC: s.italic = v != 0; break;
    case C_UNDERLINE: s.underline = v != 0; break;
    case C_AL_L: case C_AL_C: case C_AL_R: s.alignH = id - C_AL_L; break;
    case C_AL_T: case C_AL_M: case C_AL_B: s.alignV = id - C_AL_T; break;
    default: return;
    }
    SetStyle(s, newStep);
    if (id == C_BEZIER || id == C_MAGLINK || id == C_BGBLUR || id == C_LOUPE || id == C_FILLALPHA) Layout();
}

// ================================================================= creation
void PropsPanel::ThemeChanged() {
    if (bg_) DeleteObject(bg_);
    bg_ = CreateSolidBrush(EdTheme().bgMain);
    if (hwnd) RedrawWindow(hwnd, nullptr, nullptr, RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN);
}

void PropsPanel::Create(HWND parent, Canvas* c) {
    canvas = c;
    static bool reg = false;
    if (!reg) {
        WNDCLASSW wc{}; wc.lpfnWndProc = Proc; wc.hInstance = GetModuleHandleW(nullptr); wc.hCursor = LoadCursor(nullptr, IDC_ARROW);
        wc.lpszClassName = L"SnapnabProps";
        RegisterClassW(&wc); reg = true;
    }
    if (!bg_) bg_ = CreateSolidBrush(EdTheme().bgMain);
    advanced_ = RegGetDword(L"PropsAdvanced", 1, L"Editor") != 0;
    hwnd = CreateWindowExW(0, L"SnapnabProps", nullptr, WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN, 0, 0, 10, 10, parent, nullptr,
                           GetModuleHandleW(nullptr), this);
    int dpi = DpiForWindow(parent);
    HINSTANCE hi = GetModuleHandleW(nullptr);
    using namespace mui;
    auto add = [&](int id, const wchar_t* cls, const wchar_t* text, DWORD style) {
        HWND h = CreateWindowExW(0, cls, text, WS_CHILD | WS_TABSTOP | style, 0, 0, 10, 10, hwnd, (HMENU)(INT_PTR)id, hi, nullptr);
        SendMessageW(h, WM_SETFONT, (WPARAM)ui::UiFont(dpi), TRUE);
        children_[id] = h;
        return h;
    };
    add(H_THEME, kCombo, nullptr, 0);
    SendMessageW(Child(H_THEME), MUI_SETSTYLE, ComboGear, 0);
    SendMessageW(Child(H_THEME), MUI_SETGEAR, TRUE, 0);
    add(H_SEARCH, L"EDIT", L"", ES_AUTOHSCROLL);
    SendMessageW(Child(H_SEARCH), EM_SETCUEBANNER, TRUE, (LPARAM)Tr(L"Pesquisar carimbos"));
    add(H_STAMPCAT, kCombo, nullptr, 0);
    add(H_FONT, kCombo, nullptr, 0);
    for (auto& f : Fonts()) ComboBox_AddString(Child(H_FONT), f.c_str());
    add(H_STEPTYPE, kCombo, nullptr, 0);
    for (auto s : { L"1, 2, 3…", L"A, B, C…", L"a, b, c…", Tr(L"I, II, III…"), Tr(L"i, ii, iii…") }) ComboBox_AddString(Child(H_STEPTYPE), s);
    add(H_SELMODE, kCombo, nullptr, 0);
    for (auto s : { Tr(L"Retângulo"), Tr(L"Elipse"), Tr(L"Laço (mão livre)"), Tr(L"Varinha mágica") }) ComboBox_AddString(Child(H_SELMODE), s);
    add(H_SELFILL, kCombo, nullptr, 0);
    for (auto s : { Tr(L"Transparência"), Tr(L"Cor do canvas (branco)"), Tr(L"Preenchimento inteligente") }) ComboBox_AddString(Child(H_SELFILL), s);
    add(H_CUTDIR, kCombo, nullptr, 0);
    for (auto s : { Tr(L"Horizontal (remove linhas)"), Tr(L"Vertical (remove colunas)") }) ComboBox_AddString(Child(H_CUTDIR), s);
    add(H_HIDEKIND, kCombo, nullptr, 0);
    for (auto s : { Tr(L"Desfoque"), Tr(L"Pixelado") }) ComboBox_AddString(Child(H_HIDEKIND), s);
    add(H_ORGANIZE, kButton, Tr(L"Organizar carimbos…"), 0);
    add(H_IMPORT, kButton, Tr(L"Importar…"), 0);
    add(H_BTN1, kButton, L"", 0);
    add(H_BTN2, kButton, L"", 0);
    add(H_BTN3, kButton, L"", 0);
    add(H_FRONT, kButton, Tr(L"Trazer para a frente"), 0);
    add(H_BACK, kButton, Tr(L"Enviar para trás"), 0);
    add(H_DELETE, kButton, Tr(L"Excluir objeto"), 0);
    add(H_GROUP, kButton, Tr(L"Agrupar"), 0);
    add(H_UNGROUP, kButton, Tr(L"Desagrupar"), 0);
    add(H_LOCK, kButton, Tr(L"Bloquear"), 0);
    add(H_LIFT, kButton, Tr(L"Levantar a seleção (peça solta)"), 0);
    add(H_DROP, kButton, Tr(L"Soltar a peça na imagem"), 0);
    for (int id : { H_GX, H_GY, H_GW, H_GH }) add(id, L"EDIT", L"", ES_RIGHT | ES_AUTOHSCROLL);
    aspectLock_ = RegGetDword(L"GeometryAspectLock", 0, L"Editor") != 0;
    FillThemeCombo();
    FillStampCats();
}

HWND PropsPanel::EditFor(int ctlId) {
    auto it = edits_.find(ctlId);
    if (it != edits_.end()) return it->second;
    HWND e = CreateWindowExW(0, L"EDIT", L"", WS_CHILD | ES_NUMBER | ES_RIGHT | ES_AUTOHSCROLL, 0, 0, 10, 10, hwnd,
                             (HMENU)(INT_PTR)(H_EDIT + ctlId), GetModuleHandleW(nullptr), nullptr);
    SendMessageW(e, WM_SETFONT, (WPARAM)ui::UiFont(DpiForWindow(hwnd)), TRUE);
    edits_[ctlId] = e;
    return e;
}

void PropsPanel::FillThemeCombo() {
    HWND c = Child(H_THEME);
    ComboBox_ResetContent(c);
    for (auto& t : QS().themes) ComboBox_AddString(c, Tr(t.name).c_str());     // Basic and Industrial, translated
    int sep = ComboBox_AddString(c, L"");
    SendMessageW(c, mui::MUI_SETITEMFLAGS, sep, mui::ItemSeparator);
    ComboBox_AddString(c, Tr(L"Novo tema…"));
    ComboBox_AddString(c, Tr(L"Importar…"));
    ComboBox_SetCurSel(c, QS().current);
}

void PropsPanel::FillStampCats() {
    HWND c = Child(H_STAMPCAT);
    ComboBox_ResetContent(c);
    stampCats_ = StampCategories();
    for (auto& cat : stampCats_) ComboBox_AddString(c, cat.name.c_str());
    // last category by its key (the list may gain "Pinned" at the top); older settings kept only the index
    std::wstring key = RegGetString(L"LastUsedStampCategoryKey", L"", L"Editor");
    int last = (int)RegGetDword(L"LastUsedStampCategory", 1, L"Editor");
    if (key.empty()) {                            // older settings: the index in the list without "Pinned" and "Labels"
        int n = 0;
        for (auto& c : stampCats_) {
            if (c.pinned || c.key == L"labels") continue;
            if (n++ == last) { key = c.key; break; }
        }
    }
    for (int i = 0; i < (int)stampCats_.size(); i++) if (!key.empty() && stampCats_[i].key == key) last = i;
    stampCat_ = std::clamp(last, 0, std::max(0, ComboBox_GetCount(c) - 1));
    ComboBox_SetCurSel(c, stampCat_);
}

// ================================================================= thumbnails
const Image& PropsPanel::Thumb(Tool t, const Style& s, int px) {
    std::wstring key = std::to_wstring((int)t) + L"|" + std::to_wstring(px) + L"|" + Utf8ToW(XmlSerialize(StyleToXml(s), false));
    auto it = thumbs_.find(key);
    if (it != thumbs_.end()) return it->second;
    if (thumbs_.size() > 400) thumbs_.clear();
    return thumbs_[key] = QuickStylePreview(t, s, px);
}

const Image& PropsPanel::StampThumb(const std::wstring& id, int px) {
    std::wstring key = L"stamp|" + std::to_wstring(px) + L"|" + id;
    auto it = thumbs_.find(key);
    if (it != thumbs_.end()) return it->second;
    if (thumbs_.size() > 400) thumbs_.clear();
    Image img;
    if (id.rfind(L"file:", 0) == 0) {
        Image src;
        if (LoadImageFile(id.substr(5), src) && !src.empty()) {
            double k = std::min((double)px / src.w, (double)px / src.h);
            Image sc = ScaleImage(src, std::max(1, (int)(src.w * k)), std::max(1, (int)(src.h * k)));
            img = Image(px, px, 0);
            img.Blit(sc, (px - sc.w) / 2, (px - sc.h) / 2, false);
        }
    } else if (id.rfind(L"label:", 0) == 0) {          // label stamp: the badge fitted to the tile
        Image src = StampImage(id, px * 3);
        if (!src.empty()) {
            double k = std::min((double)px / src.w, (double)px / src.h);
            Image sc = ScaleImage(src, std::max(1, (int)(src.w * k)), std::max(1, (int)(src.h * k)));
            img = Image(px, px, 0);
            img.Blit(sc, (px - sc.w) / 2, (px - sc.h) / 2, false);
        }
    } else img = RenderEmoji(id, px);
    return thumbs_[key] = std::move(img);
}

// ================================================================= layout
void PropsPanel::Load() {
    if (!canvas || !hwnd) return;
    loading_ = true;
    Style s = Cur();
    Tool t = PropTool();
    int fi = 0;
    const auto& fonts = Fonts();
    for (int i = 0; i < (int)fonts.size(); i++) if (!_wcsicmp(fonts[i].c_str(), s.font.c_str())) { fi = i; break; }
    ComboBox_SetCurSel(Child(H_FONT), fi);
    ComboBox_SetCurSel(Child(H_STEPTYPE), s.stepType);
    ComboBox_SetCurSel(Child(H_SELMODE), canvas->selMode);
    ComboBox_SetCurSel(Child(H_SELFILL), canvas->selFill);
    ComboBox_SetCurSel(Child(H_CUTDIR), canvas->cutDir);
    ComboBox_SetCurSel(Child(H_HIDEKIND), s.hideKind);
    ComboBox_SetCurSel(Child(H_THEME), QS().current);
    if (t == Tool::Crop) { SetWindowTextW(Child(H_BTN1), Tr(L"Aplicar recorte")); SetWindowTextW(Child(H_BTN2), Tr(L"Corte automático")); }
    else { SetWindowTextW(Child(H_BTN1), Tr(L"Cortar para a seleção")); SetWindowTextW(Child(H_BTN2), Tr(L"Copiar seleção")); SetWindowTextW(Child(H_BTN3), Tr(L"Excluir seleção")); }
    lastEdit_ = -1;
    Layout();
    loading_ = false;
}

void PropsPanel::Layout() {
    if (!hwnd) return;
    RECT cr; GetClientRect(hwnd, &cr);
    int dpi = DpiForWindow(hwnd);
    auto S = [dpi](int v) { return ui::Scale(v, dpi); };
    int pad = S(12), W = cr.right, x0 = pad, x1 = W - pad;
    ctls_.clear(); tiles_.clear();
    struct Place { int id; RECT r; };
    std::vector<Place> places;
    std::vector<int> editIds;
    Style st = Cur();
    Tool qsTool = StyleTool(), pt = PropTool();
    bool fav = qsTool == Tool::Favorites, stamps = qsTool == Tool::Stamp;
    bool hasQs = fav || stamps || QS().HasQuickStyles(qsTool);
    int y = 0;
    gridRect_ = qsHeader_ = addBtn_ = {};
    if (hasQs) {
        qsHeader_ = { 0, y, W, y + S(34) };
        if (!fav && !stamps) addBtn_ = { x1 - S(26), y + S(4), x1, y + S(30) };
        y += S(38);
        if (!fav && !stamps) { places.push_back({ H_THEME, { x0 + S(48), y, x1, y + S(30) } }); ctls_.push_back({ C_ROWLABEL, K_ROWLABEL, { x0, y, x0 + S(46), y + S(30) }, Tr(L"Tema:") }); y += S(38); }
        if (stamps) {
            places.push_back({ H_SEARCH, { x0 + S(8), y + S(6), x1 - S(8), y + S(26) } });
            y += S(38);
            places.push_back({ H_STAMPCAT, { x0, y, x1, y + S(30) } });
            y += S(38);
        }
        tileStyles_.clear(); stampIds_.clear();
        int n = 0;
        bool addTile = !fav && !stamps;
        if (fav) { for (auto& f : QS().favorites) tileStyles_.push_back(f.style); n = (int)tileStyles_.size(); }
        else if (stamps) {
            if (stampCats_.empty()) stampCats_ = StampCategories();
            const auto& cats = stampCats_;
            std::vector<StampItem> items = stampSearch_.empty() ? (stampCat_ < (int)cats.size() ? cats[stampCat_].items : std::vector<StampItem>{})
                                                                : SearchStamps(cats, stampSearch_);
            for (auto& it : items) stampIds_.push_back(it.id);
            n = (int)stampIds_.size();
        } else { tileStyles_ = QS().StylesFor(qsTool); n = (int)tileStyles_.size(); }
        int cols = 4, gap = S(6);
        int tw = (x1 - x0 - gap * (cols - 1)) / cols, th = tw;
        int count = n + (addTile ? 1 : 0);
        int rows = (count + cols - 1) / cols;
        gridContent_ = rows * (th + gap);
        int maxH = stamps ? std::max(S(170), (int)cr.bottom * 50 / 100) : std::max(S(130), (int)cr.bottom * 42 / 100);
        int gh = (fav && n == 0) || (stamps && n == 0) ? S(64) : std::min(gridContent_, maxH);
        gridRect_ = { x0, y, x1, y + gh };
        gridScroll_ = std::clamp(gridScroll_, 0, std::max(0, gridContent_ - gh));
        for (int i = 0; i < count; i++) {
            int c = i % cols, r = i / cols;
            tiles_.push_back({ { x0 + c * (tw + gap), y + r * (th + gap), x0 + c * (tw + gap) + tw, y + r * (th + gap) + th }, i, addTile && i == n });
        }
        y += gh + S(8);
        if (stamps) {
            int half = (x1 - x0 - S(8)) / 2;
            places.push_back({ H_ORGANIZE, { x0, y, x0 + half, y + S(30) } });
            places.push_back({ H_IMPORT, { x1 - half, y, x1, y + S(30) } });
            y += S(40);
        }
    }
    // ---- tool properties
    tpHeader_ = { 0, y, W, y + S(34) };
    helpBtn_ = { x1 - S(24), y + S(5), x1, y + S(29) };
    y += S(42);
    auto swatches = [&](std::vector<std::pair<int, std::wstring>> items) {
        int n = (int)items.size(), bw = S(52), lh = S(18), bh = S(50);
        int span = (x1 - x0) / std::max(1, n);
        for (int i = 0; i < n; i++) {
            int id = items[i].first, cx = x0 + span * i + span / 2;
            int kind = id == C_SHADOW ? K_SHADOW : id == C_SHAPE || id == C_AREA ? K_SHAPE : K_SWATCH;
            ctls_.push_back({ id, kind, { cx - bw / 2, y, cx + bw / 2, y + lh + bh }, items[i].second });
        }
        y += lh + bh + S(12);
    };
    auto slider = [&](int id, const wchar_t* label, int mn, int mx, bool enabled = true) {
        ctls_.push_back({ id, K_SLIDER, { x0, y, x1, y + S(30) }, label, mn, mx, enabled });
        places.push_back({ H_EDIT + id, { x1 - S(46), y + S(7), x1 - S(6), y + S(24) } });
        editIds.push_back(id);
        y += S(32);
    };
    auto check = [&](int id, const wchar_t* label, bool enabled = true) { ctls_.push_back({ id, K_CHECK, { x0, y, x1, y + S(26) }, label, 0, 1, enabled }); y += S(30); };
    auto combo = [&](int child, const wchar_t* label, int right = 0) {
        ctls_.push_back({ C_ROWLABEL, K_ROWLABEL, { x0, y, x0 + S(92), y + S(30) }, label });
        places.push_back({ child, { x0 + S(96), y, x1 - right, y + S(30) } });
        y += S(36);
    };
    auto buttons = [&](std::vector<int> ids) { for (int id : ids) { places.push_back({ id, { x0, y, x1, y + S(30) } }); y += S(36); } };
    auto capRow = [&](bool caps) {
        if (caps) {
            ctls_.push_back({ C_CAPSTART, K_CAPSTART, { x0, y, x0 + S(58), y + S(28) }, Tr(L"Ponta inicial") });
            ctls_.push_back({ C_DASH, K_DASH, { x0 + S(64), y, x1 - S(64), y + S(28) }, Tr(L"Tracejado") });
            ctls_.push_back({ C_CAPEND, K_CAPEND, { x1 - S(58), y, x1, y + S(28) }, Tr(L"Ponta final") });
        } else {
            ctls_.push_back({ C_ROWLABEL, K_ROWLABEL, { x0, y, x0 + S(92), y + S(28) }, Tr(L"Tracejado") });
            ctls_.push_back({ C_DASH, K_DASH, { x0 + S(96), y, x1, y + S(28) }, Tr(L"Tracejado") });
        }
        y += S(38);
    };
    auto advancedText = [&](bool callout) {
        ctls_.push_back({ C_ADVANCED, K_EXPANDER, { x0, y, x1, y + S(26) }, Tr(L"Avançado") });
        y += S(30);
        if (!advanced_) return;
        ctls_.push_back({ C_ROWLABEL, K_ROWLABEL, { x0, y, x0 + S(92), y + S(28) }, Tr(L"Estilo") });
        for (int i = 0; i < 3; i++) ctls_.push_back({ C_BOLD + i, K_TOGGLEBTN, { x0 + S(96) + i * S(30), y, x0 + S(96) + i * S(30) + S(28), y + S(28) }, L"" });
        y += S(34);
        ctls_.push_back({ C_ROWLABEL, K_ROWLABEL, { x0, y, x0 + S(92), y + S(28) }, Tr(L"Alinhamento") });
        for (int i = 0; i < 3; i++) ctls_.push_back({ C_AL_L + i, K_TOGGLEBTN, { x0 + S(96) + i * S(26), y, x0 + S(96) + i * S(26) + S(25), y + S(28) }, L"" });
        for (int i = 0; i < 3; i++) ctls_.push_back({ C_AL_T + i, K_TOGGLEBTN, { x0 + S(180) + i * S(26), y, x0 + S(180) + i * S(26) + S(25), y + S(28) }, L"" });
        y += S(36);
        slider(C_OPACITY, Tr(L"Opacidade"), 0, 100);
        slider(C_PADDING, Tr(L"Margem interna"), 0, 60);
        slider(C_LINESPACING, Tr(L"Espaçamento entre linhas (%)"), 80, 300);
        if (callout) check(C_SHADED, Tr(L"Sombreado (degradê)"));
    };
    auto hint = [&](const std::wstring& t) { ctls_.push_back({ C_HINT, K_LABEL, { x0, y, x1, y + S(46) }, t }); y += S(50); };
    bool startSz = st.arrowStart != CapFlat && st.arrowStart != CapRound, endSz = st.arrowEnd != CapFlat && st.arrowEnd != CapRound;
    int nsel = canvas->SelectionCount();
    bool isGroup = ObjSelected() && canvas->doc->st.objs[canvas->selected]->type == ObjType::Group;
    if (nsel > 1) hint(Format(Tr(L"%d objetos selecionados. As mudanças de estilo valem para os do mesmo tipo do último clicado."), nsel));
    if (isGroup) hint(Format(Tr(L"Grupo com %zu objetos: arraste para mover, use as alças para redimensionar."),
                             canvas->doc->st.objs[canvas->selected]->children.size()));
    else switch (pt) {
    case Tool::Arrow: case Tool::Line:
        swatches({ { C_COLOR, Tr(L"Cor") }, { C_SHADOW, Tr(L"Sombra") } });
        capRow(true);
        slider(C_WIDTH, Tr(L"Largura"), 1, 100);
        slider(C_OPACITY, Tr(L"Opacidade"), 0, 100);
        slider(C_STARTSIZE, Tr(L"Tamanho inicial"), 1, 10, startSz);
        slider(C_ENDSIZE, Tr(L"Tamanho final"), 1, 10, endSz);
        check(C_BEZIER, Tr(L"Curva de Bézier"));
        if (pt == Tool::Arrow) {                  // label at the tail (type right after drawing the arrow)
            combo(H_FONT, Tr(L"Fonte do texto"));
            slider(C_FONTSIZE, Tr(L"Tamanho do texto"), 6, 200);
            hint(Tr(L"Depois de desenhar a seta, digite para escrever um texto na ponta inicial; duplo clique edita."));
        }
        break;
    case Tool::Pen:
        swatches({ { C_COLOR, Tr(L"Cor") }, { C_SHADOW, Tr(L"Sombra") } });
        capRow(false);
        slider(C_WIDTH, Tr(L"Largura"), 1, 100);
        slider(C_OPACITY, Tr(L"Opacidade"), 0, 100);
        break;
    case Tool::Highlighter:
        swatches({ { C_COLOR, Tr(L"Cor") }, { C_AREA, Tr(L"Forma") } });
        slider(C_OPACITY, Tr(L"Opacidade"), 5, 100);
        break;
    case Tool::Text:
        swatches({ { C_TEXTCOLOR, Tr(L"Preenchimento") }, { C_OUTLINE, Tr(L"Contorno") }, { C_SHADOW, Tr(L"Sombra") } });
        combo(H_FONT, Tr(L"Fonte"));
        slider(C_FONTSIZE, Tr(L"Tamanho da fonte"), 6, 200);
        slider(C_OUTLINEW, Tr(L"Espessura"), 0, 20);
        advancedText(false);
        break;
    case Tool::Callout: {
        bool leader = st.calloutShape == 4 || st.calloutShape == 15;
        swatches({ { C_FILL, Tr(L"Preenchimento") }, { C_OUTLINE, Tr(L"Contorno") }, { C_SHAPE, Tr(L"Forma") }, { C_SHADOW, Tr(L"Sombra") } });
        ctls_.push_back({ C_ROWLABEL, K_ROWLABEL, { x0, y, x0 + S(92), y + S(30) }, Tr(L"Fonte") });
        places.push_back({ H_FONT, { x0 + S(96), y, x1 - S(50), y + S(30) } });
        ctls_.push_back({ C_TEXTCOLOR, K_SWATCH, { x1 - S(44), y + S(2), x1, y + S(28) }, L"" });
        y += S(36);
        slider(C_FONTSIZE, Tr(L"Tamanho da fonte"), 6, 200);
        slider(C_OUTLINEW, Tr(L"Espessura"), 0, 20);
        ctls_.push_back({ C_ROWLABEL, K_ROWLABEL, { x0, y, x0 + S(92), y + S(28) }, Tr(L"Estilo da seta"), 0, 0, leader });
        ctls_.push_back({ C_DASH, K_DASH, { x0 + S(96), y, x1 - S(64), y + S(28) }, Tr(L"Tracejado"), 0, 0, leader });
        ctls_.push_back({ C_CAPEND, K_CAPEND, { x1 - S(58), y, x1, y + S(28) }, Tr(L"Ponta"), 0, 0, leader });
        y += S(36);
        ctls_.push_back({ C_ROWLABEL, K_ROWLABEL, { x0, y, x0 + S(92), y + S(28) }, Tr(L"Cor da seta"), 0, 0, leader });
        ctls_.push_back({ C_TAILCOLOR, K_SWATCH, { x1 - S(44), y + S(2), x1, y + S(26) }, L"", 0, 0, leader });
        y += S(34);
        slider(C_TAILSIZE, Tr(L"Tamanho da seta"), 1, 30, leader);
        check(C_BGBLUR, Tr(L"Desfocar o fundo (vidro fosco)"));
        slider(C_BGBLURAMT, Tr(L"Desfoque"), 1, 40, st.bgBlur);
        slider(C_FILLALPHA, Tr(L"Opacidade do preenchimento"), 0, 100);
        advancedText(true);
        break;
    }
    case Tool::Shape:
        swatches({ { C_FILL, Tr(L"Preenchimento") }, { C_OUTLINE, Tr(L"Contorno") }, { C_SHAPE, Tr(L"Forma") }, { C_SHADOW, Tr(L"Sombra") } });
        slider(C_WIDTH, Tr(L"Espessura"), 0, 50);
        slider(C_OPACITY, Tr(L"Opacidade"), 0, 100);
        break;
    case Tool::Stamp:
        swatches({ { C_SHADOW, Tr(L"Sombra") } });
        slider(C_OPACITY, Tr(L"Opacidade"), 0, 100);
        break;
    case Tool::Fill:
        swatches({ { C_EYEDROP, Tr(L"Conta-gotas") }, { C_COLOR, Tr(L"Preenchimento") } });
        slider(C_OPACITY, Tr(L"Opacidade"), 0, 100);
        slider(C_TOLERANCE, Tr(L"Tolerância"), 0, 100);
        check(C_GLOBAL, Tr(L"Mesma cor em toda a imagem"));
        break;
    case Tool::Step:
        swatches({ { C_COLOR, Tr(L"Preenchimento") }, { C_TEXTCOLOR, Tr(L"Cor do texto") }, { C_SHAPE, Tr(L"Forma") }, { C_SHADOW, Tr(L"Sombra") } });
        slider(C_OPACITY, Tr(L"Opacidade"), 0, 100);
        combo(H_STEPTYPE, Tr(L"Tipo"), S(34));
        ctls_.push_back({ C_STEPRESET, K_ICONBTN, { x1 - S(28), y - S(36), x1, y - S(6) }, Tr(L"Reiniciar a sequência") });
        // outline for contrast on light and dark backgrounds (accessibility)
        ctls_.push_back({ C_ROWLABEL, K_ROWLABEL, { x0, y, x0 + S(92), y + S(28) }, Tr(L"Contorno") });
        ctls_.push_back({ C_STEPOUTLINE, K_SWATCH, { x1 - S(44), y + S(2), x1, y + S(26) }, L"" });
        y += S(34);
        slider(C_STEPOUTLINEW, Tr(L"Espessura do contorno"), 0, 12);
        break;
    case Tool::Eraser:
        slider(C_WIDTH, Tr(L"Largura"), 1, 200);
        slider(C_OPACITY, Tr(L"Opacidade"), 1, 100);
        check(C_ANTIALIAS, Tr(L"Suavizar as bordas"));
        break;
    case Tool::Magnify:
        swatches({ { C_OUTLINE, Tr(L"Contorno") }, { C_SHAPE, Tr(L"Forma") }, { C_SHADOW, Tr(L"Sombra") } });
        slider(C_MAGSCALE, Tr(L"Ampliação (%)"), 110, 800);
        slider(C_WIDTH, Tr(L"Espessura"), 0, 20);
        check(C_MAGLINK, Tr(L"Lente separada da área ampliada"));
        check(C_LOUPE, Tr(L"Lupa realista (aro e reflexo)"));
        check(C_LOUPEHANDLE, Tr(L"Com cabo"), st.loupe);
        break;
    case Tool::Blur:
        swatches({ { C_AREA, Tr(L"Forma") } });
        combo(H_HIDEKIND, Tr(L"Tipo"));
        slider(C_AMOUNT, Tr(L"Intensidade"), 1, 30);
        break;
    case Tool::Spotlight:
        swatches({ { C_AREA, Tr(L"Forma") } });
        slider(C_AMOUNT, Tr(L"Intensidade"), 1, 14);
        break;
    case Tool::Select:
        if (nsel > 1) break;
        ctls_.push_back({ C_SMARTMOVE, K_TOGGLE, { x0, y, x1, y + S(30) }, Tr(L"Smart Move") });
        y += S(36);
        hint(canvas->smartMove ? Tr(L"Arraste uma área da imagem e depois arraste-a para outro lugar; o fundo é preenchido.")
                               : Tr(L"Clique num objeto para selecionar. Ctrl+clique ou arraste um retângulo para selecionar vários."));
        break;
    case Tool::Crop:
        hint(Tr(L"Arraste a área a manter; Enter ou duplo clique aplica."));
        buttons({ H_BTN1, H_BTN2 });
        break;
    case Tool::Selection:
        combo(H_SELMODE, Tr(L"Modo"));
        slider(C_TOLERANCE, Tr(L"Tolerância"), 0, 100);
        check(C_GLOBAL, Tr(L"Seleção global (mesma cor)"));
        combo(H_SELFILL, Tr(L"Ao mover, preencher"));
        buttons({ H_BTN1, H_BTN2, H_BTN3 });
        buttons({ H_LIFT });
        if (canvas->HasFloating()) buttons({ H_DROP });
        break;
    case Tool::Cutout:
        combo(H_CUTDIR, Tr(L"Direção"));
        hint(Tr(L"Arraste sobre a faixa a remover; as partes da imagem são unidas."));
        break;
    case Tool::Eyedropper:
        swatches({ { C_COLOR, Tr(L"Cor") } });
        hint(Tr(L"Clique na imagem para pegar a cor; ela vai para as ferramentas de desenho."));
        break;
    default: break;
    }
    // exact position and size (object, raster selection or crop area), in image pixels
    int gk = canvas->GeometryKind();
    geomShown_ = gk != 0;
    if (gk) {
        y += S(4);
        ctls_.push_back({ C_ROWLABEL, K_ROWLABEL, { x0, y, x1, y + S(22) },
                          gk == 3 ? Tr(L"Área de corte (px)") : gk == 2 ? Tr(L"Seleção (px)") : Tr(L"Posição e tamanho (px)") });
        y += S(26);
        int lw = S(22), cw = (x1 - x0 - S(40) - 2 * lw - S(10)) / 2;
        auto field = [&](int id, RECT fr) {              // edit inside a rounded frame
            ctls_.push_back({ C_ROWLABEL, K_FRAME, fr, L"" });
            places.push_back({ id, { fr.left + S(6), fr.top + S(5), fr.right - S(6), fr.bottom - S(4) } });
        };
        auto row = [&](const wchar_t* a, int ida, const wchar_t* b, int idb) {
            ctls_.push_back({ C_ROWLABEL, K_ROWLABEL, { x0, y, x0 + lw, y + S(26) }, a });
            field(ida, { x0 + lw, y, x0 + lw + cw, y + S(26) });
            ctls_.push_back({ C_ROWLABEL, K_ROWLABEL, { x0 + lw + cw + S(10), y, x0 + 2 * lw + cw + S(10), y + S(26) }, b });
            field(idb, { x0 + 2 * lw + cw + S(10), y, x0 + 2 * lw + 2 * cw + S(10), y + S(26) });
            y += S(32);
        };
        row(L"X", H_GX, L"Y", H_GY);
        int ay = y;
        row(Tr(L"L"), H_GW, Tr(L"A"), H_GH);
        ctls_.push_back({ C_ASPECT, K_TOGGLEBTN, { x1 - S(32), ay - S(16), x1, ay + S(14) }, Tr(L"Manter proporção") });
    }
    if (ObjSelected()) {
        y += S(6);
        if (nsel > 1) buttons({ H_GROUP });
        if (canvas->CanUngroup()) buttons({ H_UNGROUP });
        SetWindowTextW(Child(H_LOCK), canvas->SelectionLocked() ? Tr(L"Desbloquear") : Tr(L"Bloquear"));
        buttons({ H_FRONT, H_BACK, H_LOCK, H_DELETE });
        if (canvas->doc->st.objs[canvas->selected]->floating) buttons({ H_DROP });
    }
    content_ = y + S(12);
    scroll_ = std::clamp(scroll_, 0, std::max(0, content_ - (int)cr.bottom));
    panelSb_.content = content_; panelSb_.view = cr.bottom; panelSb_.pos = scroll_;
    panelSb_.track = { W - S(6), S(2), W - S(1), cr.bottom - S(2) };
    gridSb_.track = { x1 + S(1), gridRect_.top - scroll_, x1 + S(6), gridRect_.bottom - scroll_ };
    gridSb_.content = gridContent_; gridSb_.view = gridRect_.bottom - gridRect_.top; gridSb_.pos = gridScroll_;
    if (!hasQs) gridSb_.content = 0;
    // children: shows the ones used by this tool (positioned with the scrolling) and hides the rest
    std::set<int> used;
    for (auto& p : places) {
        HWND h = p.id >= H_EDIT ? EditFor(p.id - H_EDIT) : Child(p.id);
        if (!h) continue;
        used.insert(p.id);
        // without copying the old pixels: the children overlap while they move, and an edit box painted over a
        // button would travel with it
        SetWindowPos(h, nullptr, p.r.left, p.r.top - scroll_, p.r.right - p.r.left, p.r.bottom - p.r.top,
                     SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOCOPYBITS);
        ShowWindow(h, SW_SHOWNA);
    }
    for (auto& [id, h] : children_) if (!used.count(id)) ShowWindow(h, SW_HIDE);
    for (auto& [id, h] : edits_) {
        if (!used.count(H_EDIT + id)) { ShowWindow(h, SW_HIDE); continue; }
        bool en = true;
        for (auto& c : ctls_) if (c.id == id && c.kind == K_SLIDER) en = c.enabled;
        EnableWindow(h, en);
        if (GetFocus() != h) {
            bool old = loading_; loading_ = true;
            SetWindowTextW(h, std::to_wstring(Value(id)).c_str());
            loading_ = old;
        }
    }
    EnableWindow(Child(H_FONT), true);
    bool step = ObjSelected() && canvas->doc->st.objs[canvas->selected]->type == ObjType::Step;   // steps: position only
    EnableWindow(Child(H_GW), !step); EnableWindow(Child(H_GH), !step);
    UpdateGeometryFields();
    InvalidateRect(hwnd, nullptr, FALSE);
}

// the fields appear or disappear with the area (layout), otherwise only their values change
void PropsPanel::AreaChanged() {
    if (!canvas) return;
    if (geomShown_ != (canvas->GeometryKind() != 0)) Load();
    else UpdateGeometryFields();
}

// geometry fields ← canvas (the field being typed in keeps its text)
void PropsPanel::UpdateGeometryFields() {
    if (!geomShown_ || !canvas) return;
    Gdiplus::RectF r = canvas->Geometry();
    int v[4] = { (int)std::lround(r.X), (int)std::lround(r.Y), (int)std::lround(r.Width), (int)std::lround(r.Height) };
    bool old = loading_; loading_ = true;
    int ids[4] = { H_GX, H_GY, H_GW, H_GH };
    for (int i = 0; i < 4; i++)
        if (GetFocus() != Child(ids[i])) SetWindowTextW(Child(ids[i]), std::to_wstring(v[i]).c_str());
    loading_ = old;
    if (GetFocus() != Child(H_GW) && GetFocus() != Child(H_GH) && r.Height > 0) geomAspect_ = r.Width / r.Height;
}

// ================================================================= drawing
void PropsPanel::PaintCtl(Gdiplus::Graphics& g, HDC dc, const Ctl& c) {
    const auto& th = EdTheme();
    int dpi = DpiForWindow(hwnd);
    auto S = [dpi](int v) { return ui::Scale(v, dpi); };
    Style s = Cur();
    Tool pt = PropTool();
    RECT r = c.r;
    OffsetRect(&r, 0, -scroll_);
    COLORREF text = c.enabled ? RGB(40, 40, 46) : RGB(165, 165, 172);
    bool hot = hoverCtl_ >= 0 && hoverCtl_ < (int)ctls_.size() && &ctls_[hoverCtl_] == &c;
    auto box = [&](RECT b, bool pressed = false) {
        FillRound(g, b, (float)S(5), pressed ? Mix(RGB(255, 255, 255), th.control, 0.16) : hot && c.enabled ? RGB(246, 247, 250) : RGB(255, 255, 255));
        StrokeRound(g, b, (float)S(5), Mix(th.line, 0, 0.08), 1.0f);
    };
    switch (c.kind) {
    case K_FRAME:
        FillRound(g, r, (float)S(4), RGB(255, 255, 255));
        StrokeRound(g, r, (float)S(4), Mix(th.line, 0, 0.08), 1.0f);
        break;
    case K_ROWLABEL: case K_LABEL: {
        HGDIOBJ of = SelectObject(dc, ui::UiFont(dpi, c.kind == K_LABEL ? 8 : 9));
        SetBkMode(dc, TRANSPARENT); SetTextColor(dc, c.kind == K_LABEL ? RGB(110, 110, 118) : text);
        DrawTextW(dc, c.label.c_str(), -1, &r, c.kind == K_LABEL ? DT_LEFT | DT_WORDBREAK : DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
        SelectObject(dc, of);
        break;
    }
    case K_SWATCH: {
        uint32_t col = c.id == C_FILL ? s.fill : c.id == C_TEXTCOLOR ? s.textColor : c.id == C_TAILCOLOR ? s.tailColor :
                       c.id == C_STEPOUTLINE ? s.stepOutline : s.stroke;
        if (c.id == C_STEPOUTLINE && s.stepOutlineWidth <= 0) col &= 0x00FFFFFF;
        bool gradSw = s.grad.Active() && ((c.id == C_FILL && pt == Tool::Shape) || (c.id == C_COLOR && pt == Tool::Fill));
        if (c.id == C_OUTLINE && (pt == Tool::Text || pt == Tool::Callout || pt == Tool::Shape) && s.width <= 0) col &= 0x00FFFFFF;
        RECT b = r;
        bool compact = c.label.empty();
        if (!compact) {
            RECT lr{ r.left - S(16), r.top, r.right + S(16), r.top + S(16) };
            ui::DrawTextCentered(dc, c.label, lr, text, ui::UiFont(dpi, c.label.size() > 9 ? 7 : 8));
            b.top += S(18);
        }
        box(b);
        if (c.id == C_EYEDROP) { icons::Draw(g, icons::Eyedropper, Gdiplus::RectF((float)(b.left + b.right) / 2 - S(12), (float)(b.top + b.bottom) / 2 - S(12), (float)S(24), (float)S(24)), GC(RGB(60, 60, 66))); break; }
        RECT sw = compact ? RECT{ b.left + S(5), b.top + S(5), b.right - S(16), b.bottom - S(5) }
                        : RECT{ b.left + S(9), b.top + S(6), b.right - S(9), b.bottom - S(16) };
        if (gradSw) {                                // gradient preview
            Gdiplus::GraphicsPath rp; RoundPath(rp, RF(sw), (float)S(3));
            auto br = FillBrush(col, s.grad, RF(sw));
            g.FillPath(br.get(), &rp);
            StrokeRound(g, sw, (float)S(3), RGB(150, 150, 158), 1.0f);
        } else if ((col >> 24) == 0) {
            FillRound(g, sw, (float)S(3), RGB(255, 255, 255));
            StrokeRound(g, sw, (float)S(3), RGB(200, 200, 205), 1.0f);
            Gdiplus::Pen rp(Gdiplus::Color(255, 220, 40, 40), 2.0f);
            g.DrawLine(&rp, (float)sw.left + 2, (float)sw.bottom - 2, (float)sw.right - 2, (float)sw.top + 2);
        } else {
            FillRound(g, sw, (float)S(3), ToColorref(col));
            StrokeRound(g, sw, (float)S(3), Mix(ToColorref(col), 0, 0.25), 1.0f);
        }
        if (compact) Triangle(g, (float)b.right - S(9), (float)(b.top + b.bottom) / 2 + S(1), (float)S(3), RGB(90, 90, 96));
        else Triangle(g, (float)(b.left + b.right) / 2, (float)b.bottom - S(8), (float)S(3), RGB(90, 90, 96));
        break;
    }
    case K_SHADOW: {
        RECT lr{ r.left - S(10), r.top, r.right + S(10), r.top + S(16) };
        ui::DrawTextCentered(dc, Tr(L"Sombra ▾"), lr, text, ui::UiFont(dpi, 8));
        int cs = S(15), gx = (r.left + r.right) / 2 - cs * 3 / 2, gy = r.top + S(22);
        for (int i = 0; i < 9; i++) {
            RECT cell{ gx + (i % 3) * cs, gy + (i / 3) * cs, gx + (i % 3 + 1) * cs, gy + (i / 3 + 1) * cs };
            bool on = s.shadow && s.shadowDir == i;
            Gdiplus::SolidBrush b(on ? GC(Mix(RGB(255, 255, 255), th.control, 0.18)) : Gdiplus::Color(255, 255, 255, 255));
            g.FillRectangle(&b, RF(cell));
            Gdiplus::Pen p(GC(Mix(th.line, 0, 0.1)), 1);
            g.DrawRectangle(&p, Gdiplus::RectF((float)cell.left, (float)cell.top, (float)cs, (float)cs));
            if (on) {
                Gdiplus::Pen ck(GC(th.control), (float)S(2));
                ck.SetStartCap(Gdiplus::LineCapRound); ck.SetEndCap(Gdiplus::LineCapRound);
                float x = (float)cell.left, yy = (float)cell.top, k = (float)cs;
                Gdiplus::PointF pts[3] = { { x + k * 0.22f, yy + k * 0.52f }, { x + k * 0.42f, yy + k * 0.72f }, { x + k * 0.78f, yy + k * 0.3f } };
                g.DrawLines(&ck, pts, 3);
            }
        }
        break;
    }
    case K_SHAPE: {
        RECT lr{ r.left - S(10), r.top, r.right + S(10), r.top + S(16) };
        ui::DrawTextCentered(dc, c.label, lr, text, ui::UiFont(dpi, 8));
        RECT b = r; b.top += S(18);
        box(b);
        Gdiplus::RectF ir((float)b.left + S(10), (float)b.top + S(8), (float)(b.right - b.left) - S(20), (float)(b.bottom - b.top) - S(22));
        Gdiplus::GraphicsPath gp(Gdiplus::FillModeWinding);
        if (c.id == C_AREA) AreaSample(gp, s.areaShape, ir);
        else if (pt == Tool::Callout) { CalloutBodyPath(gp, s.calloutShape, ir); gp.Outline(); }
        else if (pt == Tool::Step) { if (s.stepShape == 2) gp.AddRectangle(ir); else if (s.stepShape == 3) ShapePath(gp, ShapeRoundRect, ir); else gp.AddEllipse(Gdiplus::RectF(ir.X + ir.Width / 2 - ir.Height / 2, ir.Y, ir.Height, ir.Height)); }
        else if (pt == Tool::Magnify) ShapePath(gp, MagKind(s.magnifyShape), ir);
        else ShapePath(gp, s.shapeKind, ir);
        Gdiplus::Pen p(Gdiplus::Color(255, 70, 70, 76), 1.5f);
        g.DrawPath(&p, &gp);
        Triangle(g, (float)(b.left + b.right) / 2, (float)b.bottom - S(7), (float)S(3), RGB(90, 90, 96));
        break;
    }
    case K_CAPSTART: case K_CAPEND: case K_DASH: {
        box(r);
        Gdiplus::RectF ir((float)r.left + S(6), (float)r.top + S(4), (float)(r.right - r.left) - S(22), (float)(r.bottom - r.top) - S(8));
        Gdiplus::Color col = GC(c.enabled ? RGB(50, 50, 56) : RGB(175, 175, 180));
        if (c.kind == K_DASH) {
            Gdiplus::Pen p(col, (float)S(3)); ApplyDash(p, s.dash);
            float yy = ir.Y + ir.Height / 2;
            g.DrawLine(&p, ir.X + S(2), yy, ir.GetRight() - S(2), yy);
        } else DrawCapGlyph(g, c.kind == K_CAPSTART ? s.arrowStart : s.arrowEnd, ir, col, c.kind == K_CAPSTART);
        Triangle(g, (float)r.right - S(9), (float)(r.top + r.bottom) / 2 + S(1), (float)S(3), c.enabled ? RGB(90, 90, 96) : RGB(180, 180, 185));
        break;
    }
    case K_SLIDER: {
        RECT lr{ r.left, r.top, r.left + S(94), r.bottom };
        HGDIOBJ of = SelectObject(dc, ui::UiFont(dpi, 8));
        SetBkMode(dc, TRANSPARENT); SetTextColor(dc, text);
        DrawTextW(dc, c.label.c_str(), -1, &lr, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
        SelectObject(dc, of);
        int tx0 = r.left + S(98), tx1 = r.right - S(56), ty = (r.top + r.bottom) / 2;
        int v = Value(c.id);
        double f = c.maxV > c.minV ? std::clamp((double)(v - c.minV) / (c.maxV - c.minV), 0.0, 1.0) : 0;
        int kx = tx0 + (int)((tx1 - tx0) * f);
        Gdiplus::Pen tr(GC(RGB(214, 216, 222)), (float)S(4)); tr.SetStartCap(Gdiplus::LineCapRound); tr.SetEndCap(Gdiplus::LineCapRound);
        g.DrawLine(&tr, (float)tx0, (float)ty, (float)tx1, (float)ty);
        if (c.enabled) {
            Gdiplus::Pen on(GC(th.control), (float)S(4)); on.SetStartCap(Gdiplus::LineCapRound); on.SetEndCap(Gdiplus::LineCapRound);
            if (kx > tx0) g.DrawLine(&on, (float)tx0, (float)ty, (float)kx, (float)ty);
        }
        float kr = (float)S(7);
        Gdiplus::SolidBrush kb(Gdiplus::Color(255, 255, 255, 255));
        g.FillEllipse(&kb, kx - kr, ty - kr, kr * 2, kr * 2);
        Gdiplus::Pen kp(GC(c.enabled ? th.control : RGB(190, 190, 196)), (float)S(2));
        g.DrawEllipse(&kp, kx - kr, ty - kr, kr * 2, kr * 2);
        RECT eb{ r.right - S(50), r.top + S(3), r.right, r.bottom - S(3) };
        FillRound(g, eb, (float)S(4), RGB(255, 255, 255));
        StrokeRound(g, eb, (float)S(4), Mix(th.line, 0, 0.08), 1.0f);
        break;
    }
    case K_CHECK: {
        int bs = S(16), by = (r.top + r.bottom) / 2 - bs / 2;
        RECT b{ r.left, by, r.left + bs, by + bs };
        bool on = Value(c.id) != 0;
        if (on) FillRound(g, b, (float)S(3), c.enabled ? th.control : RGB(190, 190, 196));
        else { FillRound(g, b, (float)S(3), RGB(255, 255, 255)); StrokeRound(g, b, (float)S(3), RGB(150, 150, 158), 1.2f); }
        if (on) {
            Gdiplus::Pen ck(Gdiplus::Color(255, 255, 255, 255), (float)S(2));
            ck.SetStartCap(Gdiplus::LineCapRound); ck.SetEndCap(Gdiplus::LineCapRound);
            Gdiplus::PointF pts[3] = { { b.left + bs * 0.22f, b.top + bs * 0.52f }, { b.left + bs * 0.42f, b.top + bs * 0.72f }, { b.left + bs * 0.78f, b.top + bs * 0.3f } };
            g.DrawLines(&ck, pts, 3);
        }
        RECT lr{ r.left + bs + S(8), r.top, r.right, r.bottom };
        HGDIOBJ of = SelectObject(dc, ui::UiFont(dpi, 9));
        SetBkMode(dc, TRANSPARENT); SetTextColor(dc, text);
        DrawTextW(dc, c.label.c_str(), -1, &lr, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
        SelectObject(dc, of);
        break;
    }
    case K_TOGGLE: {
        HGDIOBJ of = SelectObject(dc, ui::UiFont(dpi, 9));
        SetBkMode(dc, TRANSPARENT); SetTextColor(dc, text);
        RECT lr = r; lr.right -= S(50);
        DrawTextW(dc, c.label.c_str(), -1, &lr, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
        SelectObject(dc, of);
        bool on = Value(c.id) != 0;
        RECT sw{ r.right - S(42), (r.top + r.bottom) / 2 - S(10), r.right, (r.top + r.bottom) / 2 + S(10) };
        FillRound(g, sw, (float)S(10), on ? th.control : RGB(255, 255, 255));
        if (!on) StrokeRound(g, sw, (float)S(10), RGB(120, 120, 128), 1.2f);
        float d = (float)S(12), kx = on ? sw.right - S(4) - d : sw.left + S(4);
        Gdiplus::SolidBrush kb(on ? Gdiplus::Color(255, 255, 255, 255) : Gdiplus::Color(255, 90, 90, 98));
        g.FillEllipse(&kb, kx, (sw.top + sw.bottom) / 2.0f - d / 2, d, d);
        break;
    }
    case K_EXPANDER: {
        float cx = (float)r.left + S(6), cy = (float)(r.top + r.bottom) / 2;
        Gdiplus::SolidBrush b(GC(RGB(70, 70, 76)));
        float k = (float)S(4);
        if (advanced_) { Gdiplus::PointF p[3] = { { cx - k, cy - k / 2 }, { cx + k, cy - k / 2 }, { cx, cy + k * 0.7f } }; g.FillPolygon(&b, p, 3); }
        else { Gdiplus::PointF p[3] = { { cx - k / 2, cy - k }, { cx + k * 0.7f, cy }, { cx - k / 2, cy + k } }; g.FillPolygon(&b, p, 3); }
        RECT lr{ r.left + S(16), r.top, r.right, r.bottom };
        HGDIOBJ of = SelectObject(dc, ui::UiFont(dpi, 9, true));
        SetBkMode(dc, TRANSPARENT); SetTextColor(dc, RGB(50, 50, 56));
        DrawTextW(dc, c.label.c_str(), -1, &lr, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
        SelectObject(dc, of);
        break;
    }
    case K_TOGGLEBTN: {
        bool on = Value(c.id) != 0;
        box(r, on);
        Gdiplus::Color col = GC(on ? th.accentText : RGB(60, 60, 66));
        if (c.id == C_ASPECT) {                      // aspect lock: chain (linked) or broken chain
            icons::Draw(g, icons::Link, Gdiplus::RectF((float)r.left + S(5), (float)r.top + S(6), (float)(r.right - r.left) - S(10), (float)(r.bottom - r.top) - S(12)), col);
            if (!on) { Gdiplus::Pen x(GC(RGB(200, 60, 60)), 1.5f); g.DrawLine(&x, (float)r.left + S(8), (float)r.bottom - S(8), (float)r.right - S(8), (float)r.top + S(8)); }
        } else if (c.id <= C_UNDERLINE) {
            static const wchar_t* t[3] = { L"B", L"I", L"U" };
            LOGFONTW lf{}; lf.lfHeight = -S(15); lf.lfWeight = c.id == C_BOLD ? FW_BOLD : FW_NORMAL;
            lf.lfItalic = c.id == C_ITALIC; lf.lfUnderline = c.id == C_UNDERLINE; wcscpy_s(lf.lfFaceName, L"Georgia");
            HFONT f = CreateFontIndirectW(&lf);
            ui::DrawTextCentered(dc, t[c.id - C_BOLD], r, on ? th.accentText : RGB(50, 50, 56), f);
            DeleteObject(f);
        } else {
            Gdiplus::Pen p(col, (float)S(2));
            float x = (float)r.left + S(6), w = (float)(r.right - r.left) - S(12), yy = (float)r.top + S(7), h = (float)(r.bottom - r.top) - S(14);
            if (c.id <= C_AL_R) {          // aligned lines
                int a = c.id - C_AL_L;
                for (int i = 0; i < 4; i++) {
                    float lw = i % 2 ? w * 0.6f : w;
                    float lx = a == 0 ? x : a == 1 ? x + (w - lw) / 2 : x + w - lw;
                    g.DrawLine(&p, lx, yy + i * h / 3, lx + lw, yy + i * h / 3);
                }
            } else {                       // bar + block at the top/middle/bottom
                int a = c.id - C_AL_T;
                Gdiplus::Pen thin(col, 1.0f);
                g.DrawLine(&thin, x, yy, x + w, yy); g.DrawLine(&thin, x, yy + h, x + w, yy + h);
                float by = a == 0 ? yy + S(2) : a == 1 ? yy + h / 2 - S(3) : yy + h - S(8);
                Gdiplus::SolidBrush sb(col);
                g.FillRectangle(&sb, x + w * 0.25f, by, w * 0.5f, (float)S(6));
            }
        }
        break;
    }
    case K_ICONBTN: {
        if (hot) FillRound(g, r, (float)S(5), RGB(236, 237, 241));
        icons::Draw(g, icons::Restart, Gdiplus::RectF((float)r.left + S(4), (float)r.top + S(5), (float)(r.right - r.left) - S(8), (float)(r.bottom - r.top) - S(10)), GC(RGB(60, 60, 66)));
        break;
    }
    }
}

void PropsPanel::Paint(HDC hdc) {
    const auto& th = EdTheme();
    RECT cr; GetClientRect(hwnd, &cr);
    HDC mem = CreateCompatibleDC(hdc);
    HBITMAP bb = CreateCompatibleBitmap(hdc, std::max(1L, cr.right), std::max(1L, cr.bottom));
    HGDIOBJ ob = SelectObject(mem, bb);
    FillRect(mem, &cr, bg_);
    int dpi = DpiForWindow(hwnd);
    auto S = [dpi](int v) { return ui::Scale(v, dpi); };
    Tool qsTool = StyleTool();
    bool fav = qsTool == Tool::Favorites, stamps = qsTool == Tool::Stamp;
    Style cur = Cur();
    {
        Gdiplus::Graphics g(mem);
        g.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
        g.SetPixelOffsetMode(Gdiplus::PixelOffsetModeHalf);
        g.SetInterpolationMode(Gdiplus::InterpolationModeHighQualityBicubic);
        auto header = [&](RECT h, const std::wstring& t) {
            OffsetRect(&h, 0, -scroll_);
            HBRUSH hb = CreateSolidBrush(th.bgSide); FillRect(mem, &h, hb); DeleteObject(hb);
            RECT ln{ h.left, h.bottom - 1, h.right, h.bottom };
            HBRUSH lb = CreateSolidBrush(th.line); FillRect(mem, &ln, lb); DeleteObject(lb);
            ui::DrawTextCentered(mem, t, h, RGB(40, 40, 46), ui::UiFont(dpi, 10));
        };
        if (qsHeader_.bottom > qsHeader_.top) {
            header(qsHeader_, fav ? Tr(L"Favoritos") : stamps ? Tr(L"Carimbos") : Tr(L"Estilos rápidos"));
            if (addBtn_.right > addBtn_.left) {
                RECT a = addBtn_; OffsetRect(&a, 0, -scroll_);
                if (hoverAdd_) FillRound(g, a, (float)S(5), Mix(th.bgSide, 0, 0.06));
                Gdiplus::Pen p(GC(RGB(60, 60, 66)), (float)S(2));
                float cx = (a.left + a.right) / 2.0f, cy = (a.top + a.bottom) / 2.0f, k = (float)S(6);
                g.DrawLine(&p, cx - k, cy, cx + k, cy); g.DrawLine(&p, cx, cy - k, cx, cy + k);
            }
            // gallery (with its own scrolling)
            RECT gr = gridRect_; OffsetRect(&gr, 0, -scroll_);
            HRGN clip = CreateRectRgn(gr.left - S(2), gr.top, gr.right + S(2), gr.bottom);
            Gdiplus::Region rg(clip);
            g.SetClip(&rg);
            SelectClipRgn(mem, clip);
            if (tiles_.empty()) {
                HGDIOBJ of = SelectObject(mem, ui::UiFont(dpi, 8));
                SetBkMode(mem, TRANSPARENT); SetTextColor(mem, RGB(110, 110, 118));
                RECT t = gr;
                DrawTextW(mem, fav ? Tr(L"Clique na estrela de um estilo rápido para adicioná-lo aos favoritos.")
                                   : stampSearch_.empty() ? Tr(L"Nenhum carimbo nesta categoria. Use “Importar…” para acrescentar imagens.")
                                                          : Tr(L"Nenhum carimbo encontrado."), -1, &t, DT_CENTER | DT_WORDBREAK | DT_VCENTER);
                SelectObject(mem, of);
            }
            for (size_t i = 0; i < tiles_.size(); i++) {
                const Tile& t = tiles_[i];
                RECT r = t.r; OffsetRect(&r, 0, -scroll_ - gridScroll_);
                if (r.bottom < gr.top || r.top > gr.bottom) continue;
                bool sel = false;
                Tool tileTool = fav && t.index < (int)QS().favorites.size() ? QS().favorites[t.index].tool : qsTool;
                if (stamps) sel = t.index < (int)stampIds_.size() && stampIds_[t.index] == canvas->stampId;
                else if (!t.add && t.index < (int)tileStyles_.size()) sel = (fav ? tileTool == PropTool() : true) && SameStyle(tileStyles_[t.index], cur);
                bool hot = (int)i == hoverTile_;
                if (sel) FillRound(g, r, (float)S(6), Mix(RGB(255, 255, 255), th.control, 0.22));
                else if (hot) FillRound(g, r, (float)S(6), Mix(th.bgMain, 0, 0.05));
                int isz = (r.right - r.left) - S(8);
                RECT ir{ r.left + S(4), r.top + S(4), r.left + S(4) + isz, r.top + S(4) + isz };
                const Image* img = nullptr;
                if (stamps) { if (t.index < (int)stampIds_.size()) img = &StampThumb(stampIds_[t.index], isz); }
                else if (t.add) img = &Thumb(PropTool(), cur, isz);
                else if (t.index < (int)tileStyles_.size()) img = &Thumb(tileTool, tileStyles_[t.index], isz);
                if (img && !img->empty()) {
                    Image copy = *img;
                    auto bmp = copy.WrapGdip();
                    g.DrawImage(bmp.get(), RF(ir));
                }
                if (stamps && t.index < (int)stampIds_.size()) {
                    if (IsStampPinned(stampIds_[t.index])) {   // pinned: pin on the corner
                        float ps = (float)S(14);
                        icons::Draw(g, icons::Pin, Gdiplus::RectF((float)r.right - ps - S(1), (float)r.top + S(1), ps, ps), GC(th.control));
                    }
                    if (stampDragging_ && (int)i == stampDropAt_ && stampDropAt_ != stampPress_) {     // drop position
                        Gdiplus::Pen dp(GC(th.control), (float)S(2));
                        bool after = stampDropAt_ > stampPress_;
                        float x = after ? (float)r.right - 1 : (float)r.left + 1;
                        g.DrawLine(&dp, x, (float)r.top + S(2), x, (float)r.bottom - S(2));
                    }
                }
                if (t.add) {                    // "add the current style": dashed outline + "+"
                    Gdiplus::Pen dp(GC(th.control), 1.5f); dp.SetDashStyle(Gdiplus::DashStyleDash);
                    g.DrawRectangle(&dp, Gdiplus::RectF((float)r.left + 1, (float)r.top + 1, (float)(r.right - r.left) - 3, (float)(r.bottom - r.top) - 3));
                    RECT pb{ r.right - S(18), r.bottom - S(18), r.right - S(2), r.bottom - S(2) };
                    FillRound(g, pb, (float)S(3), th.control);
                    Gdiplus::Pen pp(Gdiplus::Color(255, 255, 255, 255), (float)S(2));
                    float cx = (pb.left + pb.right) / 2.0f, cy = (pb.top + pb.bottom) / 2.0f, k = (float)S(4);
                    g.DrawLine(&pp, cx - k, cy, cx + k, cy); g.DrawLine(&pp, cx, cy - k, cx, cy + k);
                } else if (!stamps && t.index < (int)tileStyles_.size()) {
                    bool isFav = fav || QS().IsFavorite(tileTool, tileStyles_[t.index]);
                    if (isFav || hot) {         // estrela de favorito no canto
                        float sz = (float)S(13), sx = (float)r.right - sz - S(2), sy = (float)r.top + S(2);
                        Gdiplus::PointF p[10];
                        for (int k = 0; k < 10; k++) {
                            double a = -3.14159265 / 2 + k * 3.14159265 / 5;
                            float rr = k % 2 ? sz * 0.22f : sz * 0.5f;
                            p[k] = Gdiplus::PointF(sx + sz / 2 + (float)std::cos(a) * rr, sy + sz / 2 + (float)std::sin(a) * rr);
                        }
                        if (isFav) { Gdiplus::SolidBrush yb(sel ? Gdiplus::Color(255, 255, 255, 255) : Gdiplus::Color(255, 255, 196, 0)); g.FillPolygon(&yb, p, 10); }
                        Gdiplus::Pen sp(isFav ? Gdiplus::Color(255, 200, 140, 0) : Gdiplus::Color(255, 140, 140, 148), 1.2f);
                        if (hoverStar_ && hot) sp.SetColor(Gdiplus::Color(255, 230, 150, 0));
                        g.DrawPolygon(&sp, p, 10);
                    }
                }
            }
            g.ResetClip();
            SelectClipRgn(mem, nullptr);
            DeleteObject(clip);
            gridSb_.pos = gridScroll_;
            gridSb_.Paint(g);
            if (stamps) {                       // search box
                for (auto& [id, h] : children_)
                    if (id == H_SEARCH && IsWindowVisible(h)) {
                        RECT er; GetWindowRect(h, &er); MapWindowPoints(nullptr, hwnd, (POINT*)&er, 2);
                        InflateRect(&er, S(8), S(6));
                        FillRound(g, er, (float)S(5), RGB(255, 255, 255));
                        StrokeRound(g, er, (float)S(5), Mix(th.line, 0, 0.08), 1.0f);
                        icons::Draw(g, icons::Magnify, Gdiplus::RectF((float)er.right - S(22), (float)(er.top + er.bottom) / 2 - S(8), (float)S(16), (float)S(16)), GC(RGB(120, 120, 128)));
                    }
            }
        }
        header(tpHeader_, Tr(L"Propriedades da ferramenta"));
        {
            RECT hb = helpBtn_; OffsetRect(&hb, 0, -scroll_);
            Gdiplus::Pen p(GC(RGB(90, 90, 96)), 1.3f);
            g.DrawEllipse(&p, Gdiplus::RectF((float)hb.left + S(2), (float)hb.top + S(2), (float)(hb.right - hb.left) - S(4), (float)(hb.bottom - hb.top) - S(4)));
            ui::DrawTextCentered(mem, L"?", hb, RGB(60, 60, 66), ui::UiFont(dpi, 9, true));
        }
        for (auto& c : ctls_) PaintCtl(g, mem, c);
        panelSb_.pos = scroll_;
        panelSb_.Paint(g);
    }
    // document information at the bottom, if it fits
    if (!docInfo_.empty() && cr.bottom - S(90) > content_ - scroll_) {
        RECT tr{ S(14), cr.bottom - S(84), cr.right - S(10), cr.bottom - S(8) };
        HGDIOBJ of = SelectObject(mem, ui::UiFont(dpi, 8));
        SetBkMode(mem, TRANSPARENT); SetTextColor(mem, RGB(110, 110, 118));
        DrawTextW(mem, docInfo_.c_str(), -1, &tr, DT_LEFT | DT_BOTTOM | DT_WORDBREAK | DT_END_ELLIPSIS);
        SelectObject(mem, of);
    }
    BitBlt(hdc, 0, 0, cr.right, cr.bottom, mem, 0, 0, SRCCOPY);
    SelectObject(mem, ob); DeleteObject(bb); DeleteDC(mem);
}

// ================================================================= interaction
void PropsPanel::ClickTile(int i, bool star) {
    if (i < 0 || i >= (int)tiles_.size()) return;
    const Tile& t = tiles_[i];
    Tool qsTool = StyleTool();
    if (qsTool == Tool::Stamp) {
        if (t.index < (int)stampIds_.size()) { canvas->SetSelectedStamp(stampIds_[t.index]); InvalidateRect(hwnd, nullptr, FALSE); }
        return;
    }
    if (t.add) { QS().AddCustom(PropTool(), Cur()); Layout(); return; }
    if (qsTool == Tool::Favorites) {
        if (t.index >= (int)QS().favorites.size()) return;
        Favorite f = QS().favorites[t.index];
        if (star) { QS().ToggleFavorite(f.tool, f.style); Layout(); return; }
        if (onPickTool) onPickTool(f.tool);
        canvas->styles[f.tool] = f.style;
        if (onStylesChanged) onStylesChanged();
        Load();
        return;
    }
    if (t.index >= (int)tileStyles_.size()) return;
    if (star) { QS().ToggleFavorite(qsTool, tileStyles_[t.index]); InvalidateRect(hwnd, nullptr, FALSE); return; }
    Style s = tileStyles_[t.index];
    SetStyle(s, true);
    Load();
}

void PropsPanel::TileMenu(int i, POINT pt) {
    if (i < 0 || i >= (int)tiles_.size() || tiles_[i].add) return;
    Tool qsTool = StyleTool();
    if (qsTool == Tool::Stamp) {                    // pin a stamp to the top (and the "Pinned" category)
        if (tiles_[i].index >= (int)stampIds_.size()) return;
        std::wstring id = stampIds_[tiles_[i].index];
        bool pinned = IsStampPinned(id);
        HMENU m = CreatePopupMenu();
        AppendMenuW(m, MF_STRING, 1, pinned ? Tr(L"Desafixar") : Tr(L"Fixar no topo"));
        int cmd = TrackPopupMenu(m, TPM_RETURNCMD | TPM_RIGHTBUTTON, pt.x, pt.y, 0, hwnd, nullptr);
        DestroyMenu(m);
        if (cmd != 1) return;
        SetStampPinned(id, !pinned);
        std::wstring key = stampCat_ < (int)stampCats_.size() ? stampCats_[stampCat_].key : L"";
        if (!key.empty()) RegSetString(L"LastUsedStampCategoryKey", key, L"Editor");
        FillStampCats();
        Layout();
        if (onHint) onHint(pinned ? Tr(L"Carimbo desafixado.") : Tr(L"Carimbo fixado: aparece primeiro na categoria e em “Fixados”."));
        return;
    }
    const Tile& t = tiles_[i];
    bool fav = qsTool == Tool::Favorites;
    Tool tool = fav ? QS().favorites[t.index].tool : qsTool;
    Style s = tileStyles_[t.index];
    int gen = fav ? 0 : QS().GeneratedCount(qsTool);
    HMENU m = CreatePopupMenu();
    AppendMenuW(m, MF_STRING, 1, Tr(L"Aplicar este estilo"));
    AppendMenuW(m, MF_STRING, 2, QS().IsFavorite(tool, s) ? Tr(L"Remover dos favoritos") : Tr(L"Adicionar aos favoritos"));
    if (!fav && t.index >= gen) AppendMenuW(m, MF_STRING, 3, Tr(L"Excluir este estilo rápido"));
    int cmd = TrackPopupMenu(m, TPM_RETURNCMD | TPM_RIGHTBUTTON, pt.x, pt.y, 0, hwnd, nullptr);
    DestroyMenu(m);
    if (cmd == 1) ClickTile(i, false);
    else if (cmd == 2) { QS().ToggleFavorite(tool, s); Layout(); }
    else if (cmd == 3) { QS().RemoveCustom(qsTool, t.index - gen); Layout(); }
}

void PropsPanel::ThemeMenu() {
    HWND owner = GetAncestor(hwnd, GA_ROOT);
    int cur = QS().current;
    const ToolTheme& th = QS().Theme();
    HMENU m = CreatePopupMenu();
    AppendMenuW(m, MF_STRING, 1, th.builtin ? Tr(L"Editar uma cópia do tema…") : Tr(L"Editar o tema…"));
    AppendMenuW(m, MF_STRING, 2, Tr(L"Duplicar"));
    AppendMenuW(m, MF_STRING, 3, Tr(L"Exportar…"));
    AppendMenuW(m, MF_SEPARATOR, 0, nullptr);
    AppendMenuW(m, MF_STRING | (th.builtin ? MF_GRAYED : 0), 4, Tr(L"Excluir"));
    RECT r; GetWindowRect(Child(H_THEME), &r);
    int cmd = TrackPopupMenu(m, TPM_RETURNCMD | TPM_RIGHTALIGN, r.right, r.bottom, 0, hwnd, nullptr);
    DestroyMenu(m);
    auto edit = [&](ToolTheme t, bool copy) {
        std::vector<ui::Field> f = { ui::Field::MakeText(Tr(L"Nome"), copy ? t.name + Tr(L" (cópia)") : t.name) };
        for (int i = 0; i < 4; i++) f.push_back(ui::Field::MakeColor(Format(Tr(L"Cor %d"), i + 1), ToColorref(t.colors[i])));
        f.push_back(ui::Field::MakeLabel(Tr(L"Os estilos rápidos de todas as ferramentas são gerados com estas cores.")));
        if (!ui::RunForm(owner, copy ? Tr(L"Novo tema") : Tr(L"Editar tema"), f) || Trim(f[0].text).empty()) return;
        t.name = Trim(f[0].text);
        for (int i = 0; i < 4; i++) t.colors[i] = FromColorref(f[1 + i].color);
        if (copy) QS().AddTheme(t);
        else { QS().themes[cur] = t; QS().Save(); }
    };
    switch (cmd) {
    case 1: edit(th, th.builtin); break;
    case 2: { ToolTheme t = th; t.name += Tr(L" (cópia)"); QS().AddTheme(t); break; }
    case 3: {
        std::wstring path = DefaultSaveDir() + L"\\" + th.name + L".snaptheme";
        int fi = 1;
        if (ui::SaveFileDialog(owner, Filter(Tr(L"Tema do Snapnab (*.snaptheme)|*.snaptheme|")), path, fi, L"snaptheme", Tr(L"Exportar tema")))
            if (!QS().ExportTheme(cur, path)) MessageBoxW(owner, Tr(L"Não foi possível exportar o tema."), L"Snapnab Editor", MB_ICONWARNING);
        break;
    }
    case 4:
        if (MessageBoxW(owner, Format(Tr(L"Excluir o tema \"%s\"?"), Tr(th.name).c_str()).c_str(), L"Snapnab Editor",
                        MB_YESNO | MB_ICONQUESTION) == IDYES) QS().DeleteTheme(cur);
        break;
    default: return;
    }
    FillThemeCombo();
    Load();
}

void PropsPanel::ClickCtl(const Ctl& c, POINT p) {
    if (!c.enabled) return;
    HWND owner = GetAncestor(hwnd, GA_ROOT);
    int dpi = DpiForWindow(hwnd);
    auto S = [dpi](int v) { return ui::Scale(v, dpi); };
    RECT r = c.r; OffsetRect(&r, 0, -scroll_);
    POINT below{ r.left, r.bottom + S(2) };
    ClientToScreen(hwnd, &below);
    Style s = Cur();
    Tool pt = PropTool();
    switch (c.kind) {
    case K_SWATCH: {
        if (c.id == C_EYEDROP) {
            canvas->pickColorOnce = true;
            if (onHint) onHint(Tr(L"Conta-gotas: clique na imagem para pegar a cor de preenchimento."));
            return;
        }
        uint32_t& col = c.id == C_FILL ? s.fill : c.id == C_TEXTCOLOR ? s.textColor : c.id == C_TAILCOLOR ? s.tailColor :
                        c.id == C_STEPOUTLINE ? s.stepOutline : s.stroke;
        bool outline = (c.id == C_OUTLINE && (pt == Tool::Text || pt == Tool::Callout || pt == Tool::Shape)) || c.id == C_STEPOUTLINE;
        float& ow = c.id == C_STEPOUTLINE ? s.stepOutlineWidth : s.width;
        uint32_t v = outline && ow <= 0 ? (col & 0x00FFFFFF) : col;
        // gradients: shape fill and the Fill tool color
        Gradient* grad = (c.id == C_FILL && pt == Tool::Shape) || (c.id == C_COLOR && pt == Tool::Fill) ? &s.grad : nullptr;
        if (!ColorPopup(hwnd, below, v, c.id == C_FILL || outline, QS().Theme().colors, grad)) return;
        col = v;
        if (outline && (v >> 24) && ow <= 0) ow = c.id == C_STEPOUTLINE ? 3.0f : pt == Tool::Shape ? 4.0f : 3.0f;
        if (outline && !(v >> 24)) ow = 0;
        SetStyle(s, true);
        Load();
        return;
    }
    case K_SHADOW: {
        if (p.y < r.top + S(20)) {          // "Shadow ▾": color, opacity, blur and distance
            HMENU m = CreatePopupMenu(), mo = CreatePopupMenu(), mb = CreatePopupMenu(), md = CreatePopupMenu();
            for (int v : { 20, 30, 45, 60, 80, 100 }) AppendMenuW(mo, MF_STRING | (s.shadowOpacity == v ? MF_CHECKED : 0), 100 + v, (std::to_wstring(v) + L"%").c_str());
            for (int v : { 0, 2, 4, 6, 10, 16 }) AppendMenuW(mb, MF_STRING | (s.shadowBlur == v ? MF_CHECKED : 0), 300 + v, (std::to_wstring(v) + L" px").c_str());
            for (int v : { 0, 2, 5, 8, 12, 20 }) AppendMenuW(md, MF_STRING | (s.shadowDist == v ? MF_CHECKED : 0), 500 + v, (std::to_wstring(v) + L" px").c_str());
            AppendMenuW(m, MF_STRING | (s.shadow ? MF_CHECKED : 0), 1, Tr(L"Mostrar sombra"));
            AppendMenuW(m, MF_STRING, 2, Tr(L"Cor da sombra…"));
            AppendMenuW(m, MF_POPUP, (UINT_PTR)mo, Tr(L"Opacidade"));
            AppendMenuW(m, MF_POPUP, (UINT_PTR)mb, Tr(L"Desfoque"));
            AppendMenuW(m, MF_POPUP, (UINT_PTR)md, Tr(L"Distância"));
            int cmd = TrackPopupMenu(m, TPM_RETURNCMD, below.x, below.y, 0, hwnd, nullptr);
            DestroyMenu(m);
            if (cmd == 1) s.shadow = !s.shadow;
            else if (cmd == 2) { uint32_t v = s.shadowColor; if (!ColorPopup(hwnd, below, v, false, QS().Theme().colors)) return; s.shadowColor = v; s.shadow = true; }
            else if (cmd >= 500) { s.shadowDist = cmd - 500; s.shadow = true; }
            else if (cmd >= 300) { s.shadowBlur = cmd - 300; s.shadow = true; }
            else if (cmd >= 100) { s.shadowOpacity = cmd - 100; s.shadow = true; }
            else return;
            SetStyle(s, true);
            return;
        }
        int cs = S(15), gx = (r.left + r.right) / 2 - cs * 3 / 2, gy = r.top + S(22);
        int cx = (p.x - gx) / cs, cy = (p.y - gy) / cs;
        if (p.x < gx || p.y < gy || cx > 2 || cy > 2) return;
        int dir = cy * 3 + cx;
        if (s.shadow && s.shadowDir == dir) s.shadow = false;
        else { s.shadow = true; s.shadowDir = dir; }
        SetStyle(s, true);
        return;
    }
    case K_SHAPE: {
        bool area = c.id == C_AREA;
        int count = area ? AreaShapeCount : pt == Tool::Callout ? kCalloutShapes : pt == Tool::Step ? 4 : pt == Tool::Magnify ? MagnifyKindCount : ShapeCount;
        int sel = area ? s.areaShape : pt == Tool::Callout ? s.calloutShape : pt == Tool::Step ? s.stepShape : pt == Tool::Magnify ? s.magnifyShape : s.shapeKind;
        SIZE cell{ S(52), S(44) };
        int idx = GridPopup(hwnd, below, 4, count, cell, sel, [&](Gdiplus::Graphics& g, HDC, int i, RECT rc, bool, bool) {
            Gdiplus::RectF ir((float)rc.left + S(10), (float)rc.top + S(9), (float)(rc.right - rc.left) - S(20), (float)(rc.bottom - rc.top) - S(18));
            Gdiplus::Pen pen(Gdiplus::Color(255, 60, 60, 66), 1.5f);
            if (area) {
                Gdiplus::GraphicsPath gp; AreaSample(gp, i, ir);
                g.DrawPath(&pen, &gp);
            } else if (pt == Tool::Callout) {
                DrawObj o; o.type = ObjType::Callout; o.style.calloutShape = i;
                o.p1 = Gdiplus::PointF(ir.X, ir.Y); o.p2 = Gdiplus::PointF(ir.GetRight(), ir.GetBottom() - S(6));
                if (i >= 5 && i <= 11) { o.p2.Y = ir.GetBottom(); o.tail = Gdiplus::PointF(ir.GetRight() + 50, ir.Y + ir.Height / 2); }
                else o.tail = Gdiplus::PointF(ir.X + ir.Width * 0.2f, ir.GetBottom() + S(4));
                Gdiplus::GraphicsPath gp(Gdiplus::FillModeWinding); CalloutBodyPath(gp, i, o.Rect()); gp.Outline();
                if (i == 4 || i == 15) {
                    Gdiplus::GraphicsPath box; CalloutBodyPath(box, i, Gdiplus::RectF(ir.X + ir.Width * 0.35f, ir.Y, ir.Width * 0.65f, ir.Height * 0.6f));
                    g.DrawPath(&pen, &box);
                    g.DrawLine(&pen, ir.X + ir.Width * 0.45f, ir.Y + ir.Height * 0.6f, ir.X + 2, ir.GetBottom());
                } else if (i >= 5 && i <= 11) g.DrawPath(&pen, &gp);
                else {
                    g.DrawPath(&pen, &gp);
                    bool tail = !(i >= 16 && i <= 21);
                    if (tail) g.DrawLine(&pen, ir.X + ir.Width * 0.3f, o.p2.Y - 1, ir.X + ir.Width * 0.18f, ir.GetBottom() + S(3));
                }
            } else if (pt == Tool::Step) {
                Gdiplus::RectF q(ir.X + ir.Width / 2 - ir.Height / 2, ir.Y, ir.Height, ir.Height);
                Gdiplus::GraphicsPath gp;
                if (i == 2) gp.AddRectangle(q); else if (i == 3) ShapePath(gp, ShapeRoundRect, q); else gp.AddEllipse(q);
                Gdiplus::SolidBrush b(Gdiplus::Color(255, 90, 90, 96)); g.FillPath(&b, &gp);
                if (i == 1) { Gdiplus::PointF tri[3] = { { q.X + q.Width / 2, q.Y + 1 }, { q.GetRight() + q.Width * 0.45f, q.Y + q.Height / 2 }, { q.X + q.Width / 2, q.GetBottom() - 1 } }; g.FillPolygon(&b, tri, 3); }
            } else {
                Gdiplus::GraphicsPath gp;
                ShapePath(gp, pt == Tool::Magnify ? MagKind(i) : i, ir);
                g.DrawPath(&pen, &gp);
            }
        });
        if (idx < 0) return;
        if (area) {
            s.areaShape = idx;
            if (IsPointArea(idx) && onHint) onHint(idx == AreaPolygon ? Tr(L"Polígono: clique nos cantos; clique no primeiro ponto ou dê um duplo clique para fechar.") : Tr(L"Mão livre: arraste contornando a área."));
        } else if (pt == Tool::Callout) s.calloutShape = idx; else if (pt == Tool::Step) s.stepShape = idx;
        else if (pt == Tool::Magnify) s.magnifyShape = idx; else s.shapeKind = idx;
        SetStyle(s, true);
        Load();
        return;
    }
    case K_CAPSTART: case K_CAPEND: {
        int cur = c.kind == K_CAPSTART ? s.arrowStart : s.arrowEnd, sel = 0;
        for (int i = 0; i < CapCount; i++) if (kCapOrder[i] == cur) sel = i;
        int idx = GridPopup(hwnd, below, 1, CapCount, { S(70), S(28) }, sel, [&](Gdiplus::Graphics& g, HDC, int i, RECT rc, bool, bool) {
            DrawCapGlyph(g, kCapOrder[i], Gdiplus::RectF((float)rc.left + S(6), (float)rc.top + S(4), (float)(rc.right - rc.left) - S(12), (float)(rc.bottom - rc.top) - S(8)),
                         Gdiplus::Color(255, 50, 50, 56), c.kind == K_CAPSTART);
        });
        if (idx < 0) return;
        (c.kind == K_CAPSTART ? s.arrowStart : s.arrowEnd) = kCapOrder[idx];
        SetStyle(s, true);
        Load();
        return;
    }
    case K_DASH: {
        int idx = GridPopup(hwnd, below, 1, DashCount, { std::max(S(120), (int)(r.right - r.left)), S(26) }, s.dash, [&](Gdiplus::Graphics& g, HDC, int i, RECT rc, bool, bool) {
            Gdiplus::Pen pen(Gdiplus::Color(255, 50, 50, 56), (float)S(3)); ApplyDash(pen, i);
            float yy = (rc.top + rc.bottom) / 2.0f;
            g.DrawLine(&pen, (float)rc.left + S(10), yy, (float)rc.right - S(10), yy);
        });
        if (idx < 0) return;
        s.dash = idx;
        SetStyle(s, true);
        return;
    }
    case K_CHECK: case K_TOGGLE: case K_TOGGLEBTN:
        if (c.id >= C_AL_L && c.id <= C_AL_B) SetValue(c.id, 1, true);
        else SetValue(c.id, !Value(c.id), true);
        if (c.id == C_SMARTMOVE) Layout();
        return;
    case K_EXPANDER:
        advanced_ = !advanced_;
        RegSetDword(L"PropsAdvanced", advanced_, L"Editor");
        Layout();
        return;
    case K_ICONBTN:
        if (c.id == C_STEPRESET && canvas->doc) { canvas->doc->st.nextStep = 1; if (onHint) onHint(Tr(L"A sequência de passos recomeça em 1.")); }
        return;
    }
}

LRESULT CALLBACK PropsPanel::Proc(HWND h, UINT m, WPARAM w, LPARAM l) {
    PropsPanel* self = (PropsPanel*)GetWindowLongPtrW(h, GWLP_USERDATA);
    if (m == WM_NCCREATE) { self = (PropsPanel*)((CREATESTRUCTW*)l)->lpCreateParams; SetWindowLongPtrW(h, GWLP_USERDATA, (LONG_PTR)self); self->hwnd = h; }
    return self ? self->Handle(m, w, l) : DefWindowProcW(h, m, w, l);
}

LRESULT PropsPanel::Handle(UINT m, WPARAM w, LPARAM l) {
    int dpi = DpiForWindow(hwnd);
    auto S = [dpi](int v) { return ui::Scale(v, dpi); };
    auto hitTile = [&](POINT p, bool& star) {
        star = false;
        RECT gr = gridRect_; OffsetRect(&gr, 0, -scroll_);
        if (!PtInRect(&gr, p)) return -1;
        for (size_t i = 0; i < tiles_.size(); i++) {
            RECT r = tiles_[i].r; OffsetRect(&r, 0, -scroll_ - gridScroll_);
            if (PtInRect(&r, p)) { star = p.x > r.right - S(18) && p.y < r.top + S(18); return (int)i; }
        }
        return -1;
    };
    auto hitCtl = [&](POINT p) {
        for (size_t i = 0; i < ctls_.size(); i++) {
            if (ctls_[i].kind == K_ROWLABEL || ctls_[i].kind == K_LABEL || ctls_[i].kind == K_FRAME) continue;
            RECT r = ctls_[i].r; OffsetRect(&r, 0, -scroll_);
            if (ctls_[i].kind == K_SLIDER) r.right -= S(52);
            if (PtInRect(&r, p)) return (int)i;
        }
        return -1;
    };
    auto sliderValue = [&](const Ctl& c, int x) {
        RECT r = c.r;
        int tx0 = r.left + S(98), tx1 = r.right - S(56);
        double f = std::clamp((double)(x - tx0) / std::max(1, tx1 - tx0), 0.0, 1.0);
        return c.minV + (int)std::lround(f * (c.maxV - c.minV));
    };
    switch (m) {
    case WM_SIZE: Layout(); return 0;
    case WM_ERASEBKGND: return 1;
    case WM_PAINT: { PAINTSTRUCT ps; HDC dc = BeginPaint(hwnd, &ps); Paint(dc); EndPaint(hwnd, &ps); return 0; }
    case WM_CTLCOLORSTATIC: case WM_CTLCOLORBTN: SetBkMode((HDC)w, TRANSPARENT); return (LRESULT)bg_;
    case WM_CTLCOLOREDIT: {
        SetBkColor((HDC)w, RGB(255, 255, 255)); SetTextColor((HDC)w, RGB(28, 28, 32));
        static HBRUSH white = CreateSolidBrush(RGB(255, 255, 255));
        return (LRESULT)white;
    }
    case WM_MOUSEMOVE: {
        POINT p{ GET_X_LPARAM(l), GET_Y_LPARAM(l) };
        if (panelSb_.dragging) { if (panelSb_.Move(p)) { scroll_ = panelSb_.pos; Layout(); } return 0; }
        if (gridSb_.dragging) { if (gridSb_.Move(p)) { gridScroll_ = gridSb_.pos; InvalidateRect(hwnd, nullptr, FALSE); } return 0; }
        {
            bool ph = panelSb_.Hit(p), gh = gridSb_.Hit(p);
            if (ph != panelSb_.hot || gh != gridSb_.hot) { panelSb_.hot = ph; gridSb_.hot = gh; InvalidateRect(hwnd, nullptr, FALSE); }
        }
        if (stampPress_ >= 0) {
            if (!stampDragging_ && stampSearch_.empty() && std::abs(p.x - stampPressPt_.x) + std::abs(p.y - stampPressPt_.y) > S(5)) stampDragging_ = true;
            if (stampDragging_) {
                bool st = false;
                int t = hitTile(p, st);
                if (t >= 0 && t != stampDropAt_) { stampDropAt_ = t; InvalidateRect(hwnd, nullptr, FALSE); }
                SetCursor(LoadCursor(nullptr, IDC_SIZEALL));
            }
            return 0;
        }
        if (dragCtl_ >= 0 && dragCtl_ < (int)ctls_.size()) {
            int v = sliderValue(ctls_[dragCtl_], p.x);
            if (v != Value(ctls_[dragCtl_].id)) {
                SetValue(ctls_[dragCtl_].id, v, dragFirst_);
                dragFirst_ = false;
                if (HWND e = edits_.count(ctls_[dragCtl_].id) ? edits_[ctls_[dragCtl_].id] : nullptr) {
                    loading_ = true; SetWindowTextW(e, std::to_wstring(v).c_str()); loading_ = false;
                }
            }
            return 0;
        }
        bool star = false;
        int t = hitTile(p, star), c = t < 0 ? hitCtl(p) : -1;
        RECT a = addBtn_; OffsetRect(&a, 0, -scroll_);
        bool add = PtInRect(&a, p) != 0;
        if (t != hoverTile_ || star != hoverStar_ || c != hoverCtl_ || add != hoverAdd_) {
            hoverTile_ = t; hoverStar_ = star; hoverCtl_ = c; hoverAdd_ = add;
            InvalidateRect(hwnd, nullptr, FALSE);
        }
        TRACKMOUSEEVENT tme{ sizeof(tme), TME_LEAVE, hwnd, 0 }; TrackMouseEvent(&tme);
        return 0;
    }
    case WM_MOUSELEAVE: hoverTile_ = hoverCtl_ = -1; hoverStar_ = hoverAdd_ = false; panelSb_.hot = gridSb_.hot = false; InvalidateRect(hwnd, nullptr, FALSE); return 0;
    case WM_LBUTTONDOWN: {
        POINT p{ GET_X_LPARAM(l), GET_Y_LPARAM(l) };
        SetFocus(hwnd);
        if (panelSb_.Down(p)) { scroll_ = panelSb_.pos; if (panelSb_.dragging) SetCapture(hwnd); Layout(); return 0; }
        if (gridSb_.Down(p)) { gridScroll_ = gridSb_.pos; if (gridSb_.dragging) SetCapture(hwnd); InvalidateRect(hwnd, nullptr, FALSE); return 0; }
        bool star = false;
        int t = hitTile(p, star);
        if (t >= 0 && StyleTool() == Tool::Stamp) {         // stamps: click selects, dragging reorders (on release)
            stampPress_ = t; stampPressPt_ = p; stampDragging_ = false; stampDropAt_ = t;
            SetCapture(hwnd);
            return 0;
        }
        if (t >= 0) { ClickTile(t, star); return 0; }
        RECT a = addBtn_; OffsetRect(&a, 0, -scroll_);
        if (PtInRect(&a, p)) { QS().AddCustom(PropTool(), Cur()); Layout(); return 0; }
        RECT hb = helpBtn_; OffsetRect(&hb, 0, -scroll_);
        if (PtInRect(&hb, p)) {
            std::wstring h = HelpFor(PropTool());
            if (!h.empty()) MessageBoxW(GetAncestor(hwnd, GA_ROOT), h.c_str(), ToolName(PropTool()), MB_ICONINFORMATION);
            return 0;
        }
        int c = hitCtl(p);
        if (c < 0) return 0;
        if (ctls_[c].kind == K_SLIDER && ctls_[c].enabled) {
            dragCtl_ = c; dragFirst_ = true;
            SetCapture(hwnd);
            SetValue(ctls_[c].id, sliderValue(ctls_[c], p.x), true);
            dragFirst_ = false;
            if (edits_.count(ctls_[c].id)) { loading_ = true; SetWindowTextW(edits_[ctls_[c].id], std::to_wstring(Value(ctls_[c].id)).c_str()); loading_ = false; }
            return 0;
        }
        Ctl copy = ctls_[c];
        ClickCtl(copy, p);
        return 0;
    }
    case WM_LBUTTONUP:
        if (stampPress_ >= 0) {
            int from = stampPress_, to = stampDropAt_;
            bool dragged = stampDragging_;
            stampPress_ = -1; stampDragging_ = false;
            ReleaseCapture();
            if (!dragged) { ClickTile(from, false); return 0; }
            if (to >= 0 && to != from && stampCat_ < (int)stampCats_.size() && from < (int)tiles_.size() && to < (int)tiles_.size()) {
                MoveStamp(stampCats_[stampCat_], tiles_[from].index, tiles_[to].index);
                std::wstring key = stampCats_[stampCat_].key;
                RegSetString(L"LastUsedStampCategoryKey", key, L"Editor");
                FillStampCats();
                Layout();
            }
            InvalidateRect(hwnd, nullptr, FALSE);
            return 0;
        }
        if (panelSb_.dragging || gridSb_.dragging) { panelSb_.Up(); gridSb_.Up(); ReleaseCapture(); InvalidateRect(hwnd, nullptr, FALSE); return 0; }
        if (dragCtl_ >= 0) { dragCtl_ = -1; ReleaseCapture(); }
        return 0;
    case WM_CAPTURECHANGED: dragCtl_ = -1; panelSb_.Up(); gridSb_.Up(); if (stampPress_ >= 0 && (HWND)l != hwnd) { stampPress_ = -1; stampDragging_ = false; } return 0;
    case WM_RBUTTONUP: {
        POINT p{ GET_X_LPARAM(l), GET_Y_LPARAM(l) };
        bool star = false;
        int t = hitTile(p, star);
        if (t >= 0) { POINT s = p; ClientToScreen(hwnd, &s); TileMenu(t, s); }
        return 0;
    }
    case WM_MOUSEWHEEL: {
        POINT p{ GET_X_LPARAM(l), GET_Y_LPARAM(l) };
        ScreenToClient(hwnd, &p);
        int d = -GET_WHEEL_DELTA_WPARAM(w) * S(48) / WHEEL_DELTA;
        RECT gr = gridRect_; OffsetRect(&gr, 0, -scroll_);
        if (PtInRect(&gr, p) && gridContent_ > gridRect_.bottom - gridRect_.top) {
            gridScroll_ = std::clamp(gridScroll_ + d, 0, gridContent_ - (int)(gridRect_.bottom - gridRect_.top));
            InvalidateRect(hwnd, nullptr, FALSE);
        } else {
            RECT cr; GetClientRect(hwnd, &cr);
            int ns = std::clamp(scroll_ + d, 0, std::max(0, content_ - (int)cr.bottom));
            if (ns != scroll_) { scroll_ = ns; Layout(); }
        }
        return 0;
    }
    case WM_COMMAND: {
        int id = LOWORD(w), code = HIWORD(w);
        if (loading_) return 0;
        if (id >= H_EDIT) {
            if (code == EN_CHANGE) {
                int ctl = id - H_EDIT;
                wchar_t buf[16]; GetWindowTextW((HWND)l, buf, 16);
                if (!buf[0]) return 0;
                int v = (int)wcstol(buf, nullptr, 10);
                for (auto& c : ctls_) if (c.id == ctl && c.kind == K_SLIDER) v = std::clamp(v, c.minV, c.maxV);
                SetValue(ctl, v, lastEdit_ != ctl);
                lastEdit_ = ctl;
            } else if (code == EN_KILLFOCUS) { lastEdit_ = -1; Layout(); }
            return 0;
        }
        Style s = Cur();
        switch (id) {
        case H_THEME:
            if (code == mui::MUI_GEARCLICK) { ThemeMenu(); return 0; }
            if (code == CBN_SELCHANGE) {
                int i = ComboBox_GetCurSel(Child(H_THEME)), n = (int)QS().themes.size();
                if (i >= 0 && i < n) { QS().current = i; QS().Save(); Load(); return 0; }
                HWND owner = GetAncestor(hwnd, GA_ROOT);
                ComboBox_SetCurSel(Child(H_THEME), QS().current);
                if (i == n + 1) {              // New theme…
                    ToolTheme t = QS().Theme(); t.custom.clear(); t.name = Tr(L"Meu tema");
                    std::vector<ui::Field> f = { ui::Field::MakeText(Tr(L"Nome"), t.name) };
                    for (int k = 0; k < 4; k++) f.push_back(ui::Field::MakeColor(Format(Tr(L"Cor %d"), k + 1), ToColorref(t.colors[k])));
                    f.push_back(ui::Field::MakeLabel(Tr(L"Os estilos rápidos de todas as ferramentas são gerados com estas cores.")));
                    if (ui::RunForm(owner, Tr(L"Novo tema"), f) && !Trim(f[0].text).empty()) {
                        t.name = Trim(f[0].text);
                        for (int k = 0; k < 4; k++) t.colors[k] = FromColorref(f[1 + k].color);
                        QS().AddTheme(t);
                    }
                } else if (i == n + 2) {       // Import…
                    std::wstring path, err;
                    if (ui::OpenFileDialog(owner, Filter(Tr(L"Tema do Snapnab (*.snaptheme)|*.snaptheme|Todos os arquivos|*.*|")), path, Tr(L"Importar tema")))
                        if (!QS().ImportTheme(path, err)) MessageBoxW(owner, err.c_str(), L"Snapnab Editor", MB_ICONWARNING);
                }
                FillThemeCombo();
                Load();
            }
            return 0;
        case H_SEARCH:
            if (code == EN_CHANGE) {
                wchar_t buf[128]; GetWindowTextW(Child(H_SEARCH), buf, 128);
                stampSearch_ = buf; gridScroll_ = 0;
                Layout();
            }
            return 0;
        case H_STAMPCAT:
            if (code == CBN_SELCHANGE) {
                stampCat_ = std::max(0, ComboBox_GetCurSel(Child(H_STAMPCAT)));
                RegSetDword(L"LastUsedStampCategory", stampCat_, L"Editor");
                if (stampCat_ < (int)stampCats_.size()) RegSetString(L"LastUsedStampCategoryKey", stampCats_[stampCat_].key, L"Editor");
                SetWindowTextW(Child(H_SEARCH), L"");
                stampSearch_.clear(); gridScroll_ = 0;
                Layout();
            }
            return 0;
        case H_FONT:
            if (code == CBN_SELCHANGE) { int i = ComboBox_GetCurSel(Child(H_FONT)); if (i >= 0 && i < (int)Fonts().size()) { s.font = Fonts()[i]; SetStyle(s, true); } }
            return 0;
        case H_STEPTYPE: if (code == CBN_SELCHANGE) { s.stepType = std::max(0, ComboBox_GetCurSel(Child(H_STEPTYPE))); SetStyle(s, true); } return 0;
        case H_HIDEKIND: if (code == CBN_SELCHANGE) { s.hideKind = std::max(0, ComboBox_GetCurSel(Child(H_HIDEKIND))); SetStyle(s, true); } return 0;
        case H_SELMODE: if (code == CBN_SELCHANGE) { canvas->selMode = std::max(0, ComboBox_GetCurSel(Child(H_SELMODE))); canvas->ClearRasterSelection(); } return 0;
        case H_SELFILL: if (code == CBN_SELCHANGE) canvas->selFill = std::max(0, ComboBox_GetCurSel(Child(H_SELFILL))); return 0;
        case H_CUTDIR: if (code == CBN_SELCHANGE) canvas->cutDir = std::max(0, ComboBox_GetCurSel(Child(H_CUTDIR))); return 0;
        case H_ORGANIZE: ShellExecuteW(nullptr, L"open", MyStampsDir().c_str(), nullptr, nullptr, SW_SHOWNORMAL); return 0;
        case H_IMPORT: {
            int n = ImportStamps(GetAncestor(hwnd, GA_ROOT));
            if (n > 0) { FillStampCats(); stampCat_ = 0; ComboBox_SetCurSel(Child(H_STAMPCAT), 0); thumbs_.clear(); Layout(); if (onHint) onHint(Format(Tr(L"%d carimbo(s) importado(s) para Meus carimbos."), n)); }
            return 0;
        }
        case H_BTN1: case H_BTN2: case H_BTN3: case H_FRONT: case H_BACK: case H_DELETE: case H_GROUP: case H_UNGROUP:
            if (code == BN_CLICKED && onCommand) onCommand(id);
            return 0;
        case H_LOCK:
            if (code == BN_CLICKED) { auto v = canvas->SelectedList(); canvas->SetLocked(v, !canvas->SelectionLocked()); Layout(); }
            return 0;
        case H_LIFT:
            if (code != BN_CLICKED) return 0;
            if (!canvas->HasRasterSelection()) { if (onHint) onHint(Tr(L"Selecione uma área com a ferramenta Seleção para levantá-la.")); return 0; }
            if (canvas->LiftSelection() && onHint) onHint(Tr(L"Peça solta: ajuste o resto da imagem (cortar, recortar faixa…), posicione a peça e tecle Enter para soltá-la."));
            Load();
            return 0;
        case H_DROP:
            if (code == BN_CLICKED && canvas->DropFloating()) { if (onHint) onHint(Tr(L"Peça solta na imagem.")); Load(); }
            return 0;
        case H_GX: case H_GY: case H_GW: case H_GH: {
            if (code == EN_SETFOCUS) {
                geomFirst_ = true;
                Gdiplus::RectF r0 = canvas->Geometry(); if (r0.Height > 0) geomAspect_ = r0.Width / r0.Height;
                PostMessageW((HWND)l, EM_SETSEL, 0, -1);           // typing replaces the value
                return 0;
            }
            if (code == EN_KILLFOCUS) { UpdateGeometryFields(); return 0; }
            if (code != EN_CHANGE) return 0;
            auto num = [&](int cid, float& out) {
                wchar_t buf[24]; GetWindowTextW(Child(cid), buf, 24);
                if (!buf[0] || !wcscmp(buf, L"-")) return false;
                out = (float)wcstol(buf, nullptr, 10);
                return true;
            };
            Gdiplus::RectF r = canvas->Geometry();
            float x = r.X, yv = r.Y, w = r.Width, h = r.Height;
            if (!num(H_GX, x) || !num(H_GY, yv) || !num(H_GW, w) || !num(H_GH, h)) return 0;
            if (w < 1 || h < 1) return 0;
            if (aspectLock_ && geomAspect_ > 0) {        // aspect lock: the other side follows
                bool old = loading_; loading_ = true;
                if (id == H_GW) { h = std::max(1.0f, std::round(w / geomAspect_)); SetWindowTextW(Child(H_GH), std::to_wstring((int)h).c_str()); }
                if (id == H_GH) { w = std::max(1.0f, std::round(h * geomAspect_)); SetWindowTextW(Child(H_GW), std::to_wstring((int)w).c_str()); }
                loading_ = old;
            }
            canvas->SetGeometry(Gdiplus::RectF(x, yv, w, h), geomFirst_);
            geomFirst_ = false;
            return 0;
        }
        }
        return 0;
    }
    }
    return DefWindowProcW(hwnd, m, w, l);
}

} // namespace snap::ed
