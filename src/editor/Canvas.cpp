#include "Canvas.h"
#include "Effects.h"
#include "Ui.h"
#include <cmath>

namespace snap::ed {

using Gdiplus::PointF;
using Gdiplus::RectF;

const wchar_t* ToolName(Tool t) {
    switch (t) {
    case Tool::Select: return Tr(L"Mover");
    case Tool::Arrow: return Tr(L"Seta");
    case Tool::Line: return Tr(L"Linha");
    case Tool::Shape: return Tr(L"Forma");
    case Tool::Pen: return Tr(L"Caneta");
    case Tool::Highlighter: return Tr(L"Marcador");
    case Tool::Text: return Tr(L"Texto");
    case Tool::Callout: return Tr(L"Balão");
    case Tool::Step: return Tr(L"Passos");
    case Tool::Stamp: return Tr(L"Carimbo");
    case Tool::Blur: return Tr(L"Ocultar");
    case Tool::Spotlight: return Tr(L"Destaque");
    case Tool::Magnify: return Tr(L"Lupa");
    case Tool::Fill: return Tr(L"Preencher");
    case Tool::Eraser: return Tr(L"Borracha");
    case Tool::Crop: return Tr(L"Cortar");
    case Tool::Selection: return Tr(L"Seleção");
    case Tool::Cutout: return Tr(L"Recortar faixa");
    case Tool::Eyedropper: return Tr(L"Conta-gotas");
    case Tool::Favorites: return Tr(L"Favoritos");
    case Tool::MagicWand: return Tr(L"Varinha mágica");
    default: return L"";
    }
}

static const int kMargin = 40;
static Gdiplus::Rect GR(RECT r) { return Gdiplus::Rect(r.left, r.top, r.right - r.left, r.bottom - r.top); }
static const int kHandle = 5;

void Canvas::Create(HWND parent) {
    static bool reg = false;
    if (!reg) {
        WNDCLASSW wc{}; wc.lpfnWndProc = Proc; wc.hInstance = GetModuleHandleW(nullptr);
        wc.hCursor = LoadCursor(nullptr, IDC_ARROW); wc.lpszClassName = L"SnapnabCanvas";
        wc.style = CS_DBLCLKS;
        RegisterClassW(&wc); reg = true;
    }
    // estilos de fábrica por ferramenta: o primeiro estilo rápido do tema Básico (QuickStyles troca depois)
    const uint32_t red = 0xFFEF4136;
    Style base; base.stroke = red;
    for (int t = 0; t < (int)Tool::Count; t++) styles[(Tool)t] = base;
    Style& ar = styles[Tool::Arrow]; ar.width = 8;
    Style& ln = styles[Tool::Line]; ln.arrowEnd = CapFlat; ln.width = 4;
    Style& sh = styles[Tool::Shape]; sh.width = 4; sh.shapeKind = ShapeRect;
    Style& hl = styles[Tool::Highlighter]; hl.stroke = 0xFFFFEB3B; hl.opacity = 50; hl.shadow = false;
    Style& tx = styles[Tool::Text]; tx.textColor = red; tx.stroke = 0xFFFFFFFF; tx.width = 4; tx.fontSize = 36;
    Style& co = styles[Tool::Callout]; co.calloutShape = 4; co.fill = red; co.stroke = red; co.width = 0; co.textColor = 0xFFFFFFFF;
    co.tailColor = red; co.tailWidth = 6; co.fontSize = 20; co.padding = 8;
    Style& st = styles[Tool::Step]; st.fontSize = 26; st.textColor = 0xFFFFFFFF;
    Style& mg = styles[Tool::Magnify]; mg.width = 4; mg.magnifyScale = 200;
    styles[Tool::Blur].shadow = false; styles[Tool::Spotlight].shadow = false;
    Style& er = styles[Tool::Eraser]; er.width = 16; er.shadow = false;
    styles[Tool::Fill].shadow = false;

    hwnd = CreateWindowExW(0, L"SnapnabCanvas", nullptr, WS_CHILD | WS_VISIBLE | WS_HSCROLL | WS_VSCROLL | WS_CLIPCHILDREN,
                           0, 0, 10, 10, parent, nullptr, GetModuleHandleW(nullptr), this);
}

Style& Canvas::CurrentStyle() { return styles[tool]; }

void Canvas::SetDocument(Document* d) {
    CommitText();
    doc = d;
    selected = -1;
    extra_.clear();
    hasSel_ = false; hasCrop_ = false;
    drag_ = Drag::None;
    preview_.reset();
    fit_ = true;
    dirty_ = true;
    FitToWindow();
    if (onSelectionChanged) onSelectionChanged();
}

void Canvas::SetTool(Tool t) {
    CommitText();
    if (t == Tool::MagicWand) { selMode = 3; t = Tool::Selection; }
    if (t == Tool::Favorites) t = tool;
    if (t != Tool::Selection && !(t == Tool::Select && smartMove)) ClearRasterSelection();
    if (t != Tool::Crop) hasCrop_ = false;
    tool = t;
    if (t != Tool::Select && selected >= 0 && doc) {
        // mantém a seleção só se o objeto é do tipo da ferramenta
        selected = -1;
        extra_.clear();
        if (onSelectionChanged) onSelectionChanged();
    }
    InvalidateRect(hwnd, nullptr, FALSE);
}

void Canvas::Refresh(bool docChanged) {
    if (docChanged) dirty_ = true;
    if (doc && selected >= (int)doc->st.objs.size()) selected = -1;
    if (!doc || selected < 0) extra_.clear();
    else extra_.erase(std::remove_if(extra_.begin(), extra_.end(), [&](int i) { return i < 0 || i >= (int)doc->st.objs.size() || i == selected; }), extra_.end());
    UpdateScrollBars();
    InvalidateRect(hwnd, nullptr, FALSE);
}

void Canvas::Recompose() {
    if (!dirty_ || !doc || doc->isVideo) return;
    const DrawObj* hide = (edit_ && editIdx_ >= 0 && editIdx_ < (int)doc->st.objs.size()) ? doc->st.objs[editIdx_].get() : nullptr;
    composed_ = doc->Render(hide);
    dirty_ = false;
}

// ---------------------------------------------------------------- coordenadas
POINT Canvas::Origin() const {
    RECT cr; GetClientRect(hwnd, &cr);
    int iw = doc ? (int)std::lround(doc->w() * zoom) : 0, ih = doc ? (int)std::lround(doc->h() * zoom) : 0;
    POINT o;
    o.x = iw + 2 * kMargin <= cr.right ? (cr.right - iw) / 2 : kMargin - scroll_.x;
    o.y = ih + 2 * kMargin <= cr.bottom ? (cr.bottom - ih) / 2 : kMargin - scroll_.y;
    return o;
}

PointF Canvas::ToDoc(POINT p) const {
    POINT o = Origin();
    return PointF((float)((p.x - o.x) / zoom), (float)((p.y - o.y) / zoom));
}

POINT Canvas::ToScreen(PointF p) const {
    POINT o = Origin();
    return { (LONG)std::lround(o.x + p.X * zoom), (LONG)std::lround(o.y + p.Y * zoom) };
}

RECT Canvas::ToScreenRect(RectF r) const {
    POINT a = ToScreen(PointF(r.X, r.Y)), b = ToScreen(PointF(r.GetRight(), r.GetBottom()));
    return { a.x, a.y, b.x, b.y };
}

void Canvas::UpdateScrollBars() {
    if (!hwnd) return;
    RECT cr; GetClientRect(hwnd, &cr);
    int iw = doc ? (int)std::lround(doc->w() * zoom) : 0, ih = doc ? (int)std::lround(doc->h() * zoom) : 0;
    int cw = iw + 2 * kMargin, ch = ih + 2 * kMargin;
    scroll_.x = std::clamp((int)scroll_.x, 0, std::max(0, cw - (int)cr.right));
    scroll_.y = std::clamp((int)scroll_.y, 0, std::max(0, ch - (int)cr.bottom));
    SCROLLINFO si{ sizeof(si), SIF_ALL | SIF_DISABLENOSCROLL };
    si.nMin = 0; si.nMax = cw - 1; si.nPage = cr.right; si.nPos = scroll_.x;
    SetScrollInfo(hwnd, SB_HORZ, &si, TRUE);
    si.nMax = ch - 1; si.nPage = cr.bottom; si.nPos = scroll_.y;
    SetScrollInfo(hwnd, SB_VERT, &si, TRUE);
}

void Canvas::ZoomTo(double z, POINT* anchor) {
    if (!doc) return;
    z = std::clamp(z, 0.05, 16.0);
    RECT cr; GetClientRect(hwnd, &cr);
    POINT a = anchor ? *anchor : POINT{ cr.right / 2, cr.bottom / 2 };
    PointF d = ToDoc(a);
    zoom = z;
    fit_ = false;
    // mantém o ponto do documento sob a âncora
    int iw = (int)std::lround(doc->w() * zoom), ih = (int)std::lround(doc->h() * zoom);
    scroll_.x = (LONG)std::lround(kMargin + d.X * zoom - a.x);
    scroll_.y = (LONG)std::lround(kMargin + d.Y * zoom - a.y);
    (void)iw; (void)ih;
    UpdateScrollBars();
    InvalidateRect(hwnd, nullptr, FALSE);
    if (edit_) { CommitText(); }
    if (onZoomChanged) onZoomChanged();
}

void Canvas::FitToWindow() {
    if (!hwnd) return;
    RECT cr; GetClientRect(hwnd, &cr);
    if (doc && doc->w() > 0 && cr.right > 2 * kMargin && cr.bottom > 2 * kMargin) {
        double z = std::min((double)(cr.right - 2 * kMargin) / doc->w(), (double)(cr.bottom - 2 * kMargin) / doc->h());
        zoom = std::min(1.0, z);
    } else zoom = 1.0;
    scroll_ = {};
    fit_ = true;
    UpdateScrollBars();
    InvalidateRect(hwnd, nullptr, FALSE);
    if (onZoomChanged) onZoomChanged();
}

// ---------------------------------------------------------------- alças
std::vector<POINT> Canvas::HandlePoints(const DrawObj& o) const {
    std::vector<POINT> h;
    if (o.IsLine()) {
        h.push_back(ToScreen(o.p1)); h.push_back(ToScreen(o.p2));
        if (o.style.bezier)        // alça da curva: o ponto do meio da curva
            h.push_back(ToScreen(PointF(0.25f * o.p1.X + 0.5f * o.ctrl.X + 0.25f * o.p2.X, 0.25f * o.p1.Y + 0.5f * o.ctrl.Y + 0.25f * o.p2.Y)));
        return h;
    }
    if (o.type == ObjType::Step) return h;
    RectF r = o.IsPath() ? [&] { RectF b = o.Bounds(); float pad = o.style.width / 2 + (o.style.shadow ? 4 : 0); b.Inflate(-pad, -pad); return b; }() : o.Rect();
    RECT s = ToScreenRect(r);
    int mx = (s.left + s.right) / 2, my = (s.top + s.bottom) / 2;
    h = { { s.left, s.top }, { mx, s.top }, { s.right, s.top }, { s.right, my },
          { s.right, s.bottom }, { mx, s.bottom }, { s.left, s.bottom }, { s.left, my } };
    if ((o.type == ObjType::Callout && o.CalloutHasTail()) || (o.type == ObjType::Magnify && o.style.magnifyLink)) h.push_back(ToScreen(o.tail));
    return h;
}

std::vector<int> Canvas::SelectedList() const {
    std::vector<int> v;
    if (!doc || selected < 0 || selected >= (int)doc->st.objs.size()) return v;
    v.push_back(selected);
    for (int i : extra_) if (i >= 0 && i < (int)doc->st.objs.size() && i != selected) v.push_back(i);
    std::sort(v.begin(), v.end());
    v.erase(std::unique(v.begin(), v.end()), v.end());
    return v;
}

bool Canvas::IsSelected(int i) const {
    auto v = SelectedList();
    return std::find(v.begin(), v.end(), i) != v.end();
}

void Canvas::SelectObjects(const std::vector<int>& idx) {
    extra_.clear();
    selected = -1;
    for (int i : idx) if (doc && i >= 0 && i < (int)doc->st.objs.size()) { if (selected >= 0) extra_.push_back(selected); selected = i; }
    InvalidateRect(hwnd, nullptr, FALSE);
    if (onSelectionChanged) onSelectionChanged();
}

void Canvas::SelectAllObjects() {
    if (!doc) return;
    std::vector<int> all;
    for (int i = 0; i < (int)doc->st.objs.size(); i++) all.push_back(i);
    if (tool != Tool::Select) SetTool(Tool::Select);
    SelectObjects(all);
}

bool Canvas::CanUngroup() const {
    for (int i : SelectedList()) if (doc->st.objs[i]->type == ObjType::Group) return true;
    return false;
}

// Agrupar (CPaintOpGroup): os selecionados viram um objeto só, na posição do que estava mais à frente
void Canvas::GroupSelected() {
    auto v = SelectedList();
    if (!doc || v.size() < 2) return;
    CommitText();
    doc->Checkpoint();
    auto g = std::make_shared<DrawObj>();
    g->type = ObjType::Group;
    g->style.shadow = false;
    for (int i : v) g->children.push_back(*doc->st.objs[i]);
    g->UpdateGroupRect();
    int at = v.back() - (int)(v.size() - 1);
    for (auto it = v.rbegin(); it != v.rend(); ++it) doc->st.objs.erase(doc->st.objs.begin() + *it);
    doc->st.objs.insert(doc->st.objs.begin() + at, g);
    extra_.clear();
    selected = at;
    Refresh(); if (onDocChanged) onDocChanged(); if (onSelectionChanged) onSelectionChanged();
}

// Desagrupar: cada grupo selecionado volta a ser os seus objetos, que ficam selecionados
void Canvas::UngroupSelected() {
    auto v = SelectedList();
    if (!doc || !CanUngroup()) return;
    CommitText();
    doc->Checkpoint();
    std::vector<ObjPtr> out;
    std::vector<int> newSel;
    for (int i = 0; i < (int)doc->st.objs.size(); i++) {
        bool sel = std::find(v.begin(), v.end(), i) != v.end();
        const DrawObj& o = *doc->st.objs[i];
        if (sel && o.type == ObjType::Group) {
            for (auto& c : o.children) { newSel.push_back((int)out.size()); out.push_back(std::make_shared<DrawObj>(c)); }
        } else {
            if (sel) newSel.push_back((int)out.size());
            out.push_back(doc->st.objs[i]);
        }
    }
    doc->st.objs = std::move(out);
    dirty_ = true;
    SelectObjects(newSel);
    Refresh(); if (onDocChanged) onDocChanged();
}

int Canvas::HitHandle(POINT p) const {
    if (!doc || selected < 0 || selected >= (int)doc->st.objs.size() || !extra_.empty()) return -1;
    auto hs = HandlePoints(*doc->st.objs[selected]);
    for (int i = (int)hs.size() - 1; i >= 0; i--)
        if (std::abs(p.x - hs[i].x) <= kHandle + 2 && std::abs(p.y - hs[i].y) <= kHandle + 2) return i;
    return -1;
}

int Canvas::HitObject(PointF p) const {
    if (!doc) return -1;
    float tol = (float)(4 / zoom);
    for (int i = (int)doc->st.objs.size() - 1; i >= 0; i--)
        if (doc->st.objs[i]->HitTest(p, tol)) return i;
    return -1;
}

// ---------------------------------------------------------------- pintura
static HBRUSH CheckerBrush() {
    static HBRUSH b = [] {
        uint32_t px[16 * 16];
        for (int y = 0; y < 16; y++) for (int x = 0; x < 16; x++) px[y * 16 + x] = ((x / 8 + y / 8) % 2) ? 0xFFFFFFFF : 0xFFDADADA;
        HBITMAP bm = CreateBitmap(16, 16, 1, 32, px);
        return CreatePatternBrush(bm);
    }();
    return b;
}

void Canvas::Paint(HDC hdc) {
    RECT cr; GetClientRect(hwnd, &cr);
    HDC mem = CreateCompatibleDC(hdc);
    HBITMAP bb = CreateCompatibleBitmap(hdc, std::max(1L, cr.right), std::max(1L, cr.bottom));
    HGDIOBJ ob = SelectObject(mem, bb);
    HBRUSH bgb = CreateSolidBrush(background);
    FillRect(mem, &cr, bgb); DeleteObject(bgb);
    int dpi = DpiForWindow(hwnd);
    if (!doc || doc->isVideo) {
        ui::DrawTextCentered(mem, doc ? L"" : Tr(L"Nenhuma captura aberta — use “Capturar” ou abra um arquivo (Ctrl+O)."),
                             cr, RGB(90, 90, 90), ui::UiFont(dpi, 11));
    } else if (fxPreview_ && !fxPreview_->empty()) {
        // prévia do efeito: a imagem resultante, centrada, no zoom atual (reduzida se não couber); sem alças
        const Image& f = *fxPreview_;
        double z = std::min({ zoom, (double)(cr.right - 2 * kMargin) / f.w, (double)(cr.bottom - 2 * kMargin) / f.h });
        z = std::max(z, 0.02);
        int iw = std::max(1, (int)std::lround(f.w * z)), ih = std::max(1, (int)std::lround(f.h * z));
        RECT ir{ (cr.right - iw) / 2, (cr.bottom - ih) / 2, 0, 0 };
        ir.right = ir.left + iw; ir.bottom = ir.top + ih;
        if (f.HasTransparency()) {
            Image disp(f.w, f.h);
            for (size_t i = 0; i < f.px.size(); i++) {
                int x = (int)(i % f.w), y = (int)(i / f.w);
                uint32_t bg = (((int)(x * z) / 8 + (int)(y * z) / 8) % 2) ? 0xFFFFFFFF : 0xFFDADADA;
                disp.px[i] = BlendOver(bg, f.px[i]);
            }
            disp.DrawTo(mem, ir.left, ir.top, iw, ih, z < 1.0);
        } else {
            RECT sh = ir; OffsetRect(&sh, 3, 3);
            HBRUSH shb = CreateSolidBrush(RGB(180, 182, 186)); FillRect(mem, &sh, shb); DeleteObject(shb);
            f.DrawTo(mem, ir.left, ir.top, iw, ih, z < 1.0);
        }
        RECT tag{ cr.right / 2 - ui::Scale(130, dpi), ui::Scale(10, dpi), cr.right / 2 + ui::Scale(130, dpi), ui::Scale(36, dpi) };
        ui::FillRoundRect(mem, tag, ui::Scale(13, dpi), RGB(32, 32, 36), RGB(32, 32, 36));
        ui::DrawTextCentered(mem, Tr(L"Prévia — Aplicar ou Cancelar no painel"), tag, RGB(255, 255, 255), ui::UiFont(dpi, 9));
    } else {
        Recompose();
        // quadro exibido = composição (+ objeto em criação)
        const Image* frame = &composed_;
        Image tmp;
        if (preview_) {
            tmp = composed_;
            auto bmp = tmp.WrapGdip();
            Gdiplus::Graphics g(bmp.get());
            g.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
            g.SetTextRenderingHint(Gdiplus::TextRenderingHintAntiAliasGridFit);
            RenderObject(g, tmp, *preview_);
            g.Flush(Gdiplus::FlushIntentionSync);
            frame = &tmp;
        }
        POINT o = Origin();
        int iw = (int)std::lround(doc->w() * zoom), ih = (int)std::lround(doc->h() * zoom);
        RECT ir{ o.x, o.y, o.x + iw, o.y + ih };
        // sombra do "papel"
        RECT sh = ir; OffsetRect(&sh, 3, 3);
        HBRUSH shb = CreateSolidBrush(RGB(180, 182, 186)); FillRect(mem, &sh, shb); DeleteObject(shb);
        if (frame->HasTransparency()) {
            FillRect(mem, &ir, CheckerBrush());
            Image disp(frame->w, frame->h);
            // xadrez em coordenadas de tela é aproximado por cinza claro sob pixels transparentes
            for (size_t i = 0; i < frame->px.size(); i++) {
                int x = (int)(i % frame->w), y = (int)(i / frame->w);
                uint32_t bg = (((int)(x * zoom) / 8 + (int)(y * zoom) / 8) % 2) ? 0xFFFFFFFF : 0xFFDADADA;
                disp.px[i] = BlendOver(bg, frame->px[i]);
            }
            disp.DrawTo(mem, ir.left, ir.top, iw, ih, zoom < 1.0);
        } else frame->DrawTo(mem, ir.left, ir.top, iw, ih, zoom < 1.0);

        Gdiplus::Graphics g(mem);
        g.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
        // seleção raster: realce azul + contorno tracejado
        if (hasSel_ && !selOverlay_.empty()) {
            auto ob2 = selOverlay_.WrapGdip();
            RECT sr = ToScreenRect(RectF((float)selBounds_.left, (float)selBounds_.top,
                                         (float)(selBounds_.right - selBounds_.left), (float)(selBounds_.bottom - selBounds_.top)));
            g.SetInterpolationMode(Gdiplus::InterpolationModeNearestNeighbor);
            g.DrawImage(ob2.get(), Gdiplus::Rect(sr.left, sr.top, sr.right - sr.left, sr.bottom - sr.top));
            Gdiplus::Pen p1(Gdiplus::Color(255, 255, 255, 255), 1), p2(Gdiplus::Color(255, 0, 0, 0), 1);
            p2.SetDashStyle(Gdiplus::DashStyleDash);
            g.DrawRectangle(&p1, GR(sr));
            g.DrawRectangle(&p2, GR(sr));
        }
        // caixa de texto: tracejada enquanto é arrastada e enquanto o texto é digitado
        if ((preview_ && preview_->type == ObjType::Text && drag_ == Drag::Create) ||
            (edit_ && editIdx_ >= 0 && editIdx_ < (int)doc->st.objs.size() && doc->st.objs[editIdx_]->type == ObjType::Text)) {
            const DrawObj& tb = preview_ && drag_ == Drag::Create ? *preview_ : *doc->st.objs[editIdx_];
            RECT s = ToScreenRect(tb.Rect());
            Gdiplus::Pen w1(Gdiplus::Color(255, 255, 255, 255), 1.0f), p(Gdiplus::Color(255, 30, 120, 230), 1.0f);
            p.SetDashStyle(Gdiplus::DashStyleDash);
            g.DrawRectangle(&w1, GR(s));
            g.DrawRectangle(&p, GR(s));
        }
        // retângulo de seleção de objetos
        if (drag_ == Drag::Marquee) {
            RECT s = ToScreenRect(selRect_);
            Gdiplus::SolidBrush fillB(Gdiplus::Color(40, 30, 120, 230));
            g.FillRectangle(&fillB, GR(s));
            Gdiplus::Pen p(Gdiplus::Color(255, 30, 120, 230), 1.2f); p.SetDashStyle(Gdiplus::DashStyleDash);
            g.DrawRectangle(&p, GR(s));
        }
        // vários objetos selecionados: contorno de cada um e da área do conjunto
        if (!edit_ && SelectionCount() > 1) {
            Gdiplus::Pen p(Gdiplus::Color(220, 30, 120, 230), 1.4f); p.SetDashStyle(Gdiplus::DashStyleDot);
            RectF u;
            bool first = true;
            for (int i : SelectedList()) {
                RectF b = doc->st.objs[i]->Bounds();
                g.DrawRectangle(&p, GR(ToScreenRect(b)));
                if (first) { u = b; first = false; } else RectF::Union(u, u, b);
            }
            Gdiplus::Pen up(Gdiplus::Color(255, 30, 120, 230), 1.0f); up.SetDashStyle(Gdiplus::DashStyleDash);
            RECT ur = ToScreenRect(u); InflateRect(&ur, 3, 3);
            g.DrawRectangle(&up, GR(ur));
        }
        // seleção em andamento (retângulo/elipse/laço)
        if (drag_ == Drag::Select || drag_ == Drag::Lasso) {
            Gdiplus::Pen p(Gdiplus::Color(255, 30, 120, 230), 1.5f); p.SetDashStyle(Gdiplus::DashStyleDash);
            if (drag_ == Drag::Lasso && lasso_.size() > 1) {
                std::vector<Gdiplus::Point> pts;
                for (auto& q : lasso_) { POINT s = ToScreen(q); pts.push_back({ s.x, s.y }); }
                g.DrawLines(&p, pts.data(), (INT)pts.size());
            } else {
                RECT s = ToScreenRect(selRect_);
                if (selMode == 1) g.DrawEllipse(&p, GR(s));
                else g.DrawRectangle(&p, GR(s));
            }
        }
        // recorte: escurece fora
        if (hasCrop_ || drag_ == Drag::Crop) {
            RECT c = ToScreenRect(crop_);
            Gdiplus::SolidBrush dim(Gdiplus::Color(140, 0, 0, 0));
            Gdiplus::Region rg(Gdiplus::Rect(ir.left, ir.top, iw, ih));
            rg.Exclude(Gdiplus::Rect(c.left, c.top, c.right - c.left, c.bottom - c.top));
            g.FillRegion(&dim, &rg);
            Gdiplus::Pen p(Gdiplus::Color(255, 255, 255, 255), 2);
            g.DrawRectangle(&p, GR(c));
            std::wstring t = Format(Tr(L"%d × %d  — Enter aplica, Esc cancela"), (int)crop_.Width, (int)crop_.Height);
            RECT tr{ c.left, c.top - ui::Scale(26, dpi), c.left + ui::Scale(300, dpi), c.top - ui::Scale(4, dpi) };
            if (tr.top < 0) OffsetRect(&tr, 0, ui::Scale(32, dpi));
            ui::FillRoundRect(mem, tr, 8, RGB(30, 30, 30), RGB(30, 30, 30));
            ui::DrawTextCentered(mem, t, tr, RGB(255, 255, 255), ui::UiFont(dpi, 9));
        }
        // faixa do recorte de faixa
        if (drag_ == Drag::Cut) {
            RECT s = ToScreenRect(selRect_);
            Gdiplus::SolidBrush b(Gdiplus::Color(90, 230, 50, 50));
            if (cutDir == 0) g.FillRectangle(&b, ir.left, s.top, iw, s.bottom - s.top);
            else g.FillRectangle(&b, s.left, ir.top, s.right - s.left, ih);
        }
        // alças do objeto selecionado
        if (selected >= 0 && selected < (int)doc->st.objs.size() && !edit_ && SelectionCount() == 1) {
            const DrawObj& obj = *doc->st.objs[selected];
            RECT b = ToScreenRect(obj.IsLine() ? RectF() : obj.Bounds());
            if (!obj.IsLine()) {
                Gdiplus::Pen p(Gdiplus::Color(200, 30, 120, 230), 1); p.SetDashStyle(Gdiplus::DashStyleDot);
                g.DrawRectangle(&p, GR(b));
            }
            Gdiplus::SolidBrush wb(Gdiplus::Color(255, 255, 255, 255));
            Gdiplus::Pen bp(Gdiplus::Color(255, 30, 120, 230), 1.5f);
            auto hs = HandlePoints(obj);
            for (size_t i = 0; i < hs.size(); i++) {
                bool tail = (i == 8 && !obj.IsLine()) || (obj.IsLine() && i == 2);
                Gdiplus::SolidBrush yb(Gdiplus::Color(255, 255, 200, 0));
                g.FillEllipse(tail ? &yb : &wb, hs[i].x - kHandle, hs[i].y - kHandle, kHandle * 2, kHandle * 2);
                g.DrawEllipse(&bp, hs[i].x - kHandle, hs[i].y - kHandle, kHandle * 2, kHandle * 2);
            }
        }
    }
    BitBlt(hdc, 0, 0, cr.right, cr.bottom, mem, 0, 0, SRCCOPY);
    SelectObject(mem, ob);
    DeleteObject(bb); DeleteDC(mem);
}

// ---------------------------------------------------------------- seleção raster
void Canvas::ClearRasterSelection() {
    if (!hasSel_) return;
    hasSel_ = false;
    selMask_.clear();
    selOverlay_ = Image();
    InvalidateRect(hwnd, nullptr, FALSE);
}

static Image MakeOverlay(const cv::Mask& m, int w, RECT b) {
    Image o(b.right - b.left, b.bottom - b.top, 0);
    for (int y = 0; y < o.h; y++)
        for (int x = 0; x < o.w; x++)
            if (m[(size_t)(b.top + y) * w + b.left + x]) o.at(x, y) = 0x553C8CFF;
    return o;
}

static void SetSelectionFromMask(Canvas* c, cv::Mask m, bool& hasSel, cv::Mask& selMask, RECT& bounds, Image& overlay) {
    RECT b = cv::MaskBounds(m, c->doc->w(), c->doc->h());
    if (b.right <= b.left) { hasSel = false; return; }
    selMask = std::move(m);
    bounds = b;
    overlay = MakeOverlay(selMask, c->doc->w(), b);
    hasSel = true;
}

void Canvas::SelectAll() {
    if (!doc || doc->isVideo) return;
    if (tool != Tool::Selection) SetTool(Tool::Selection);
    SetSelectionFromMask(this, cv::Mask((size_t)doc->w() * doc->h(), 255), hasSel_, selMask_, selBounds_, selOverlay_);
    InvalidateRect(hwnd, nullptr, FALSE);
}

void Canvas::FillHole(Image& base, const cv::Mask& m) {
    switch (selFill) {
    case 0: for (size_t i = 0; i < m.size(); i++) if (m[i]) base.px[i] = 0; break;
    case 1: { uint32_t c = 0xFFFFFFFF; for (size_t i = 0; i < m.size(); i++) if (m[i]) base.px[i] = c; break; }
    default: cv::SmartFill(base, m); break;           // Smart Move: inpainting
    }
}

Image Canvas::SelectionImage() const {
    if (!doc || doc->isVideo) return {};
    Image comp = doc->Render();
    if (hasSel_) {
        Image out = comp.Crop(selBounds_);
        for (int y = 0; y < out.h; y++)
            for (int x = 0; x < out.w; x++)
                if (!selMask_[(size_t)(selBounds_.top + y) * doc->w() + selBounds_.left + x]) out.at(x, y) = 0;
        return out;
    }
    auto v = SelectedList();
    if (!v.empty()) {
        RectF b = doc->st.objs[v[0]]->Bounds();
        for (int i : v) RectF::Union(b, b, doc->st.objs[i]->Bounds());
        return comp.Crop({ (LONG)b.X, (LONG)b.Y, (LONG)std::ceil(b.GetRight()), (LONG)std::ceil(b.GetBottom()) });
    }
    return comp;
}

bool Canvas::CopySelection(bool cut) {
    Image img = SelectionImage();
    if (img.empty()) return false;
    CopyImageToClipboard(GetAncestor(hwnd, GA_ROOT), img);
    if (cut) DeleteSelected();
    return true;
}

void Canvas::DeleteSelected() {
    if (!doc) return;
    if (hasSel_) {
        doc->Checkpoint();
        FillHole(doc->MutableBase(), selMask_);
        ClearRasterSelection();
        Refresh(); if (onDocChanged) onDocChanged();
        return;
    }
    auto v = SelectedList();
    if (!v.empty()) {
        doc->Checkpoint();
        for (auto it = v.rbegin(); it != v.rend(); ++it) doc->st.objs.erase(doc->st.objs.begin() + *it);
        selected = -1;
        extra_.clear();
        Refresh(); if (onDocChanged) onDocChanged(); if (onSelectionChanged) onSelectionChanged();
    }
}

void Canvas::CropToSelection() {
    if (!doc) return;
    RECT b{};
    if (hasSel_) b = selBounds_;
    else if (hasCrop_) b = { (LONG)crop_.X, (LONG)crop_.Y, (LONG)crop_.GetRight(), (LONG)crop_.GetBottom() };
    else return;
    if (b.right - b.left < 1 || b.bottom - b.top < 1) return;
    doc->Checkpoint();
    doc->SetBase(doc->st.base->Crop(b));
    for (size_t i = 0; i < doc->st.objs.size(); i++) doc->Mutable(i)->Move((float)-b.left, (float)-b.top);
    ClearRasterSelection();
    hasCrop_ = false;
    selected = -1;
    FitToWindow();
    Refresh(); if (onDocChanged) onDocChanged();
}

void Canvas::ApplyCrop() { if (hasCrop_) CropToSelection(); }

void Canvas::PasteImage(const Image& img) {
    if (!doc || img.empty()) return;
    CommitText();
    doc->Checkpoint();
    auto o = std::make_shared<DrawObj>();
    o->type = ObjType::Picture;
    o->picture = std::make_shared<Image>(img);
    o->style.width = 0; o->style.shadow = false;
    float x = std::max(0.0f, (doc->w() - img.w) / 2.0f), y = std::max(0.0f, (doc->h() - img.h) / 2.0f);
    o->p1 = PointF(x, y); o->p2 = PointF(x + img.w, y + img.h);
    doc->st.objs.push_back(o);
    selected = (int)doc->st.objs.size() - 1; extra_.clear();
    SetTool(Tool::Select);
    selected = (int)doc->st.objs.size() - 1; extra_.clear();
    Refresh(); if (onDocChanged) onDocChanged(); if (onSelectionChanged) onSelectionChanged();
}

// ordem: os selecionados andam juntos, mantendo a ordem entre eles
static void Reorder(Document* doc, const std::vector<int>& v, int mode, int& selected, std::vector<int>& extra) {
    auto& objs = doc->st.objs;
    ObjPtr primary = selected >= 0 ? objs[selected] : nullptr;
    std::vector<char> sel(objs.size(), 0);
    for (int i : v) sel[i] = true;
    if (mode == 0 || mode == 3) {              // 0 primeiro plano, 3 plano de fundo
        std::vector<ObjPtr> a, b;
        for (size_t i = 0; i < objs.size(); i++) (sel[i] ? a : b).push_back(objs[i]);
        objs = mode == 0 ? b : a;
        objs.insert(objs.end(), (mode == 0 ? a : b).begin(), (mode == 0 ? a : b).end());
    } else if (mode == 1) {                    // um passo à frente
        for (int i = (int)objs.size() - 2; i >= 0; i--)
            if (sel[i] && !sel[i + 1]) { std::swap(objs[i], objs[i + 1]); std::swap(sel[i], sel[i + 1]); }
    } else {                                   // um passo para trás
        for (int i = 1; i < (int)objs.size(); i++)
            if (sel[i] && !sel[i - 1]) { std::swap(objs[i], objs[i - 1]); std::swap(sel[i], sel[i - 1]); }
    }
    extra.clear();
    selected = -1;
    for (size_t i = 0; i < objs.size(); i++) {         // seleção refeita nas novas posições
        if (!sel[i]) continue;
        if (objs[i] == primary) selected = (int)i; else extra.push_back((int)i);
    }
    if (selected < 0 && !extra.empty()) { selected = extra.back(); extra.pop_back(); }
}

void Canvas::BringToFront() {
    auto v = SelectedList();
    if (!doc || v.empty()) return;
    doc->Checkpoint();
    Reorder(doc, v, 0, selected, extra_);
    Refresh(); if (onDocChanged) onDocChanged();
}

void Canvas::SendToBack() {
    auto v = SelectedList();
    if (!doc || v.empty()) return;
    doc->Checkpoint();
    Reorder(doc, v, 3, selected, extra_);
    Refresh(); if (onDocChanged) onDocChanged();
}

void Canvas::BringForward() {
    auto v = SelectedList();
    if (!doc || v.empty()) return;
    doc->Checkpoint();
    Reorder(doc, v, 1, selected, extra_);
    Refresh(); if (onDocChanged) onDocChanged();
}

void Canvas::SendBackward() {
    auto v = SelectedList();
    if (!doc || v.empty()) return;
    doc->Checkpoint();
    Reorder(doc, v, 2, selected, extra_);
    Refresh(); if (onDocChanged) onDocChanged();
}

static RectF ObjExtent(const DrawObj& o) { return o.IsLine() || o.IsPath() || o.type == ObjType::Group ? o.Bounds() : o.Rect(); }

// alinhar: um objeto, em relação à imagem; vários, entre si (pela área que ocupam juntos)
void Canvas::AlignSelected(int mode) {
    auto v = SelectedList();
    if (!doc || v.empty()) return;
    RectF ref(0, 0, (float)doc->w(), (float)doc->h());
    if (v.size() > 1) { ref = ObjExtent(*doc->st.objs[v[0]]); for (int i : v) RectF::Union(ref, ref, ObjExtent(*doc->st.objs[i])); }
    bool any = false;
    std::vector<std::pair<int, PointF>> moves;
    for (int i : v) {
        RectF r = ObjExtent(*doc->st.objs[i]);
        float dx = 0, dy = 0;
        switch (mode) {
        case 0: dx = ref.X - r.X; break;
        case 1: dx = ref.X + (ref.Width - r.Width) / 2 - r.X; break;
        case 2: dx = ref.GetRight() - r.GetRight(); break;
        case 3: dy = ref.Y - r.Y; break;
        case 4: dy = ref.Y + (ref.Height - r.Height) / 2 - r.Y; break;
        case 5: dy = ref.GetBottom() - r.GetBottom(); break;
        }
        if (std::fabs(dx) > 0.01f || std::fabs(dy) > 0.01f) { any = true; moves.push_back({ i, PointF(dx, dy) }); }
    }
    if (!any) return;
    doc->Checkpoint();
    for (auto& [i, d] : moves) doc->Mutable(i)->Move(d.X, d.Y);
    Refresh(); if (onDocChanged) onDocChanged();
}

bool Canvas::FlipSelected(bool horizontal) {
    auto v = SelectedList();
    if (!doc || v.empty()) return false;
    RectF u = ObjExtent(*doc->st.objs[v[0]]);
    for (int i : v) RectF::Union(u, u, ObjExtent(*doc->st.objs[i]));
    float cx = u.X + u.Width / 2, cy = u.Y + u.Height / 2;
    doc->Checkpoint();
    for (int i : v) {
        DrawObj* o = doc->Mutable(i);
        std::function<void(DrawObj&)> flipObj = [&](DrawObj& ob) {
            ob.MapPoints([&](PointF p) { return horizontal ? PointF(2 * cx - p.X, p.Y) : PointF(p.X, 2 * cy - p.Y); });
            if (ob.picture) ob.picture = std::make_shared<Image>(horizontal ? fx::FlipH(*ob.picture) : fx::FlipV(*ob.picture));
            for (auto& c : ob.children) if (c.picture) c.picture = std::make_shared<Image>(horizontal ? fx::FlipH(*c.picture) : fx::FlipV(*c.picture));
        };
        flipObj(*o);
        o->UpdateGroupRect();
    }
    Refresh(); if (onDocChanged) onDocChanged();
    return true;
}

void Canvas::FlattenSelected() {
    auto v = SelectedList();
    if (!doc || v.empty()) return;
    CommitText();
    doc->Checkpoint();
    Image base = *doc->st.base;
    {
        auto bmp = base.WrapGdip();
        Gdiplus::Graphics g(bmp.get());
        g.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
        g.SetTextRenderingHint(Gdiplus::TextRenderingHintAntiAliasGridFit);
        g.SetPixelOffsetMode(Gdiplus::PixelOffsetModeHalf);
        for (int i : v) RenderObject(g, base, *doc->st.objs[i]);
        g.Flush(Gdiplus::FlushIntentionSync);
    }
    doc->SetBase(std::move(base));
    for (auto it = v.rbegin(); it != v.rend(); ++it) doc->st.objs.erase(doc->st.objs.begin() + *it);
    selected = -1;
    extra_.clear();
    Refresh(); if (onDocChanged) onDocChanged(); if (onSelectionChanged) onSelectionChanged();
}

void Canvas::FlattenAll() {
    if (!doc || doc->st.objs.empty()) return;
    CommitText();
    doc->Checkpoint();
    doc->SetBase(doc->Render());
    doc->st.objs.clear();
    selected = -1;
    Refresh(); if (onDocChanged) onDocChanged(); if (onSelectionChanged) onSelectionChanged();
}

static int Family(const DrawObj& o) {
    if (o.IsShape()) return (int)ObjType::Shape;
    if (o.type == ObjType::Pixelate) return (int)ObjType::Blur;
    return (int)o.type;
}

void Canvas::ApplyStyleToSelected(const Style& s, bool newStep) {
    if (!doc || selected < 0 || selected >= (int)doc->st.objs.size()) return;
    if (newStep) doc->Checkpoint();
    int fam = Family(*doc->st.objs[selected]);
    for (int i : SelectedList()) {                // vários selecionados: o estilo vale para os do mesmo tipo do principal
        if (i == selected || Family(*doc->st.objs[i]) != fam || doc->st.objs[i]->type == ObjType::Group) continue;
        DrawObj* x = doc->Mutable(i);
        x->style = s;
        if (x->IsShape()) x->type = ObjType::Shape;
        if (x->type == ObjType::Blur || x->type == ObjType::Pixelate) x->type = s.hideKind == 1 ? ObjType::Pixelate : ObjType::Blur;
    }
    if (doc->st.objs[selected]->type == ObjType::Group) { Refresh(); if (onDocChanged) onDocChanged(); return; }
    DrawObj* o = doc->Mutable(selected);
    o->style = s;
    if (o->IsShape()) o->type = ObjType::Shape;                  // formas antigas viram Shape com o tipo do estilo
    if (o->type == ObjType::Blur || o->type == ObjType::Pixelate) o->type = s.hideKind == 1 ? ObjType::Pixelate : ObjType::Blur;
    if (o->type == ObjType::Magnify && s.magnifyLink && o->tail.X == 0 && o->tail.Y == 0) {
        RectF r = o->Rect(); o->tail = PointF(r.X - r.Width * 0.4f, r.GetBottom() + r.Height * 0.3f);
    }
    if (o->type == ObjType::Callout && o->CalloutHasTail() && o->Rect().Contains(o->tail)) {
        RectF r = o->Rect(); o->tail = PointF(r.X + r.Width * 0.25f, r.GetBottom() + std::max(20.0f, r.Height * 0.45f));
    }
    Refresh(); if (onDocChanged) onDocChanged();
}

void Canvas::SetSelectedStamp(const std::wstring& id) {
    stampId = id;
    if (!doc || selected < 0 || selected >= (int)doc->st.objs.size() || doc->st.objs[selected]->type != ObjType::Stamp) return;
    if (doc->st.objs[selected]->text == id) return;
    doc->Checkpoint();
    doc->Mutable(selected)->text = id;
    Refresh(); if (onDocChanged) onDocChanged();
}

void Canvas::Nudge(int dx, int dy) {
    auto v = SelectedList();
    if (!doc || v.empty()) return;
    doc->Checkpoint();
    for (int i : v) doc->Mutable(i)->Move((float)dx, (float)dy);
    Refresh(); if (onDocChanged) onDocChanged();
}

// ---------------------------------------------------------------- texto no lugar (IInPlaceRichEditOwner)
void Canvas::BeginTextEdit(int idx, bool isNew) {
    CommitText();
    if (!doc || idx < 0 || idx >= (int)doc->st.objs.size()) return;
    const DrawObj& o = *doc->st.objs[idx];
    if (!o.HasText()) return;
    if (!isNew) doc->Checkpoint();
    editIdx_ = idx; editNew_ = isNew;
    RectF tr = o.Rect();
    if (o.type == ObjType::Callout) tr = CalloutTextRect(o);
    else tr.Inflate(-o.style.padding, -o.style.padding);
    RECT r = ToScreenRect(tr);
    if (r.right - r.left < 60) r.right = r.left + 60;
    if (r.bottom - r.top < 30) r.bottom = r.top + 30;
    LOGFONTW lf{};
    lf.lfHeight = -std::max(8, (int)std::lround(o.style.fontSize * zoom));
    lf.lfWeight = o.style.bold ? FW_BOLD : FW_NORMAL;
    lf.lfItalic = o.style.italic; lf.lfUnderline = o.style.underline;
    lf.lfQuality = CLEARTYPE_QUALITY;
    wcsncpy_s(lf.lfFaceName, o.style.font.c_str(), _TRUNCATE);
    if (editFont_) DeleteObject(editFont_);
    editFont_ = CreateFontIndirectW(&lf);
    DWORD align = o.style.alignH == 1 ? ES_CENTER : o.style.alignH == 2 ? ES_RIGHT : ES_LEFT;
    edit_ = CreateWindowExW(0, L"EDIT", nullptr, WS_CHILD | WS_VISIBLE | ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN | align,
                            r.left, r.top, r.right - r.left, r.bottom - r.top, hwnd, (HMENU)77, GetModuleHandleW(nullptr), nullptr);
    SendMessageW(edit_, WM_SETFONT, (WPARAM)editFont_, TRUE);
    std::wstring t;
    for (wchar_t c : o.text) { if (c == L'\n') t += L"\r\n"; else t += c; }
    SetWindowTextW(edit_, t.c_str());
    SendMessageW(edit_, EM_SETSEL, 0, -1);
    SetFocus(edit_);
    dirty_ = true;
    LayoutTextEdit();
    InvalidateRect(hwnd, nullptr, FALSE);
}

// A largura é a da caixa (arrastada ou a padrão); a altura acompanha o texto digitado — a caixa de texto cresce para
// baixo quando o texto passa dela. A caixa de edição fica na altura do texto, no alinhamento vertical escolhido,
// para o que se vê ao digitar ser o mesmo que fica na imagem.
void Canvas::LayoutTextEdit() {
    if (!edit_ || !doc || editIdx_ < 0 || editIdx_ >= (int)doc->st.objs.size()) return;
    ObjType type = doc->st.objs[editIdx_]->type;
    int alignV = doc->st.objs[editIdx_]->style.alignV;
    float pad = doc->st.objs[editIdx_]->style.padding;
    auto area = [&] {
        const DrawObj& o = *doc->st.objs[editIdx_];
        RectF a = o.Rect();
        if (type == ObjType::Callout) a = CalloutTextRect(o); else a.Inflate(-pad, -pad);
        RECT r = ToScreenRect(a);
        if (r.right - r.left < 60) r.right = r.left + 60;
        return r;
    };
    RECT r = area();
    int n = GetWindowTextLengthW(edit_);
    std::wstring t(n + 1, L'\0');
    GetWindowTextW(edit_, t.data(), n + 1);
    t.resize(n);
    if (t.empty() || t.back() == L'\n') t += L"Ag";        // a linha nova ainda vazia também ocupa altura
    HDC dc = GetDC(edit_);
    HGDIOBJ of = SelectObject(dc, editFont_);
    DWORD mg = (DWORD)SendMessageW(edit_, EM_GETMARGINS, 0, 0);
    RECT m{ 0, 0, std::max(10, (int)(r.right - r.left) - (int)LOWORD(mg) - (int)HIWORD(mg) - 2), 0 };
    DrawTextW(dc, t.c_str(), -1, &m, DT_CALCRECT | DT_WORDBREAK | DT_EDITCONTROL | DT_NOPREFIX);
    SelectObject(dc, of);
    ReleaseDC(edit_, dc);
    int textH = m.bottom + 4, areaH = r.bottom - r.top;
    if (type == ObjType::Text && textH > areaH) {          // cresce para baixo
        float grow = (textH - areaH) / (float)zoom;
        DrawObj* w = doc->Mutable(editIdx_);
        if (w->p2.Y >= w->p1.Y) w->p2.Y += grow; else w->p1.Y += grow;
        dirty_ = true;
        r = area();
        areaH = r.bottom - r.top;
    }
    int h = std::min(textH, areaH);
    int top = r.top;
    if (h < areaH) top = alignV == 1 ? r.top + (areaH - h) / 2 : alignV == 2 ? r.bottom - h : r.top;
    MoveWindow(edit_, r.left, top, r.right - r.left, std::max(h, 8), TRUE);
    if (textH <= areaH) SendMessageW(edit_, EM_LINESCROLL, 0, -1000);   // tudo cabe: sem rolagem interna
    InvalidateRect(hwnd, nullptr, FALSE);
}

void Canvas::CommitText() {
    if (!edit_) return;
    HWND e = edit_;
    edit_ = nullptr;           // evita reentrância via EN_KILLFOCUS
    int n = GetWindowTextLengthW(e);
    std::wstring t(n + 1, L'\0');
    GetWindowTextW(e, t.data(), n + 1);
    t.resize(n);
    std::wstring clean;
    for (wchar_t c : t) if (c != L'\r') clean += c;
    DestroyWindow(e);
    if (doc && editIdx_ >= 0 && editIdx_ < (int)doc->st.objs.size()) {
        if (clean.empty() && doc->st.objs[editIdx_]->type == ObjType::Text) {
            doc->st.objs.erase(doc->st.objs.begin() + editIdx_);
            selected = -1;
            if (onSelectionChanged) onSelectionChanged();
        } else if (doc->st.objs[editIdx_]->text != clean) {
            DrawObj* w = doc->Mutable(editIdx_);
            w->text = clean;
            if (w->type == ObjType::Text) {                     // garante a altura para o texto inteiro (sem cortar linhas)
                RectF r = w->Rect();
                float need = TextBlockHeight(*w, r.Width - 2 * w->style.padding) + 2 * w->style.padding;
                if (r.Height < need) { if (w->p2.Y >= w->p1.Y) w->p2.Y = w->p1.Y + need; else w->p1.Y = w->p2.Y + need; }
            }
        } else if (!editNew_) doc->DiscardCheckpoint();
    }
    editIdx_ = -1;
    dirty_ = true;
    InvalidateRect(hwnd, nullptr, FALSE);
    if (onDocChanged) onDocChanged();
    SetFocus(hwnd);
}

// ---------------------------------------------------------------- mouse
void Canvas::BeginMove() {
    doc->Checkpoint();
    dragMulti_.clear();
    for (int i : SelectedList()) dragMulti_.push_back({ i, *doc->st.objs[i] });
    if (selected >= 0) dragOrig_ = *doc->st.objs[selected];
    drag_ = Drag::Move;
    InvalidateRect(hwnd, nullptr, FALSE);
}

void Canvas::StartSmartMove(PointF p) {
    // levanta os pixels da seleção como objeto e preenche o buraco (Smart Move, TSCV.Inpainting)
    doc->Checkpoint();
    Image& base = doc->MutableBase();
    Image piece = base.Crop(selBounds_);
    for (int y = 0; y < piece.h; y++)
        for (int x = 0; x < piece.w; x++)
            if (!selMask_[(size_t)(selBounds_.top + y) * base.w + selBounds_.left + x]) piece.at(x, y) = 0;
    FillHole(base, selMask_);
    auto o = std::make_shared<DrawObj>();
    o->type = ObjType::Picture;
    o->picture = std::make_shared<Image>(std::move(piece));
    o->style.width = 0; o->style.shadow = false;
    o->p1 = PointF((float)selBounds_.left, (float)selBounds_.top);
    o->p2 = PointF((float)selBounds_.right, (float)selBounds_.bottom);
    doc->st.objs.push_back(o);
    ClearRasterSelection();
    selected = (int)doc->st.objs.size() - 1; extra_.clear();
    extra_.clear();
    dragOrig_ = *o;
    dragMulti_ = { { selected, *o } };
    drag_ = Drag::Move;
    changed_ = true;
    dirty_ = true;
    if (onSelectionChanged) onSelectionChanged();
}

// borracha: tira alfa da imagem na opacidade escolhida; com suavização a borda do pincel é gradual
void Canvas::EraseAt(PointF p) {
    Image& base = const_cast<Image&>(*doc->st.base);
    const Style& s = CurrentStyle();
    float r = std::max(1.0f, s.width / 2);
    int op = std::clamp(s.opacity, 1, 100);
    for (int y = (int)(p.Y - r - 1); y <= (int)(p.Y + r + 1); y++)
        for (int x = (int)(p.X - r - 1); x <= (int)(p.X + r + 1); x++) {
            if (x < 0 || y < 0 || x >= base.w || y >= base.h) continue;
            float dx = x + 0.5f - p.X, dy = y + 0.5f - p.Y, d = std::sqrt(dx * dx + dy * dy);
            float cover = s.antialias ? std::clamp(r + 0.5f - d, 0.0f, 1.0f) : (d <= r ? 1.0f : 0.0f);
            if (cover <= 0) continue;
            int keep = 255 - (int)(255 * cover * op / 100);
            uint32_t& px = base.at(x, y);
            int na = std::min(CA(px), CA(px) * keep / 255);
            px = na ? (px & 0x00FFFFFF) | ((uint32_t)na << 24) : 0;
        }
}

void Canvas::OnDown(POINT p, bool dbl) {
    if (!doc || doc->isVideo || fxPreview_) return;         // durante a prévia de um efeito a imagem não é editável
    if (edit_) { CommitText(); return; }      // clicar fora só confirma o texto (não cria outro objeto)
    SetFocus(hwnd);
    PointF d = ToDoc(p);
    if (pickColorOnce) {                      // conta-gotas do painel do Preencher
        pickColorOnce = false;
        Recompose();
        int x = (int)d.X, y = (int)d.Y;
        if (x >= 0 && y >= 0 && x < composed_.w && y < composed_.h) {
            styles[tool].stroke = composed_.at(x, y) | 0xFF000000;
            if (onSelectionChanged) onSelectionChanged();
        }
        return;
    }
    downScreen_ = p; downDoc_ = d; changed_ = false;
    SetCapture(hwnd);

    // alça do objeto selecionado tem prioridade em qualquer ferramenta de desenho
    int hh = HitHandle(p);
    if (hh >= 0 && tool != Tool::Crop && tool != Tool::Selection && tool != Tool::Fill && tool != Tool::Eyedropper) {
        doc->Checkpoint();
        dragOrig_ = *doc->st.objs[selected];
        handle_ = hh; drag_ = Drag::Resize;
        return;
    }
    bool drawTool = tool != Tool::Select && tool != Tool::Crop && tool != Tool::Selection && tool != Tool::Fill &&
                    tool != Tool::Eraser && tool != Tool::Eyedropper && tool != Tool::Cutout;
    if (tool == Tool::Select || drawTool) {
        int hit = HitObject(d);
        // com ferramenta de desenho, clicar num objeto existente o seleciona/move (como no original)
        if (hit >= 0 && (tool == Tool::Select || hit == selected || !dbl)) {
            // ferramenta de desenho só pega objetos do próprio tipo (seta pega seta); sobre os outros ela desenha —
            // assim dá para marcar ou apontar um texto. Ctrl força desenhar também sobre o mesmo tipo.
            ObjType ht = doc->st.objs[hit]->type;
            bool sameKind = (tool == Tool::Arrow && ht == ObjType::Arrow) || (tool == Tool::Line && ht == ObjType::Line) ||
                            (tool == Tool::Shape && doc->st.objs[hit]->IsShape()) || (tool == Tool::Pen && ht == ObjType::Pen) ||
                            (tool == Tool::Highlighter && ht == ObjType::Highlighter) || (tool == Tool::Text && ht == ObjType::Text) ||
                            (tool == Tool::Callout && ht == ObjType::Callout) || (tool == Tool::Magnify && ht == ObjType::Magnify) ||
                            (tool == Tool::Blur && (ht == ObjType::Blur || ht == ObjType::Pixelate)) || (tool == Tool::Spotlight && ht == ObjType::Spotlight);
            if (tool == Tool::Select || hit == selected || (GetKeyState(VK_CONTROL) >= 0 && sameKind)) {
                if (dbl && doc->st.objs[hit]->HasText()) { selected = hit; extra_.clear(); BeginTextEdit(hit, false); ReleaseCapture(); return; }
                bool toggle = tool == Tool::Select && (GetKeyState(VK_CONTROL) < 0 || GetKeyState(VK_SHIFT) < 0);
                if (toggle && IsSelected(hit)) {         // Ctrl/Shift+clique num selecionado: tira da seleção
                    auto v = SelectedList();
                    v.erase(std::remove(v.begin(), v.end(), hit), v.end());
                    SelectObjects(v);
                    ReleaseCapture();
                    return;
                }
                if (toggle) { if (selected >= 0) extra_.push_back(selected); selected = hit; }
                else if (!IsSelected(hit)) { selected = hit; extra_.clear(); }
                else if (hit != selected) { extra_.push_back(selected); selected = hit; }     // continua com todos, este é o principal
                if (onSelectionChanged) onSelectionChanged();
                BeginMove();
                return;
            }
        }
        if (tool == Tool::Select) {
            bool keep = GetKeyState(VK_CONTROL) < 0 || GetKeyState(VK_SHIFT) < 0;
            if (selected != -1 && !keep) { selected = -1; extra_.clear(); if (onSelectionChanged) onSelectionChanged(); InvalidateRect(hwnd, nullptr, FALSE); }
            if (smartMove) {               // Smart Move: arrastar dentro da área levanta os pixels; fora, marca outra área
                int x = (int)d.X, y = (int)d.Y;
                bool inside = hasSel_ && x >= 0 && y >= 0 && x < doc->w() && y < doc->h() && selMask_[(size_t)y * doc->w() + x];
                if (inside) { StartSmartMove(d); return; }
                ClearRasterSelection();
                drag_ = Drag::Select; selRect_ = RectF(d.X, d.Y, 0, 0);
                return;
            }
            drag_ = Drag::Marquee; selRect_ = RectF(d.X, d.Y, 0, 0);    // retângulo de seleção de objetos
            return;
        }
    }
    if (selected != -1 && drawTool) { selected = -1; extra_.clear(); if (onSelectionChanged) onSelectionChanged(); }

    Style s = CurrentStyle();
    switch (tool) {
    case Tool::Step: {
        doc->Checkpoint();
        auto o = std::make_shared<DrawObj>();
        o->type = ObjType::Step; o->style = s; o->p1 = o->p2 = d;
        o->stepValue = doc->st.nextStep++;
        doc->st.objs.push_back(o);
        selected = (int)doc->st.objs.size() - 1; extra_.clear();
        Refresh(); if (onDocChanged) onDocChanged(); if (onSelectionChanged) onSelectionChanged();
        ReleaseCapture();
        return;
    }
    case Tool::Fill: {
        Image cur = *doc->st.base;
        cv::Mask m = cv::DoWand(cur, { (LONG)d.X, (LONG)d.Y }, tolerance, wandGlobal);
        if (m.empty()) break;
        doc->Checkpoint();
        Image& base = doc->MutableBase();
        uint32_t c = (s.stroke & 0x00FFFFFF) | ((uint32_t)(255 * s.opacity / 100) << 24);
        for (size_t i = 0; i < m.size(); i++) if (m[i]) base.px[i] = BlendOver(base.px[i], c);
        Refresh(); if (onDocChanged) onDocChanged();
        break;
    }
    case Tool::Eyedropper: {
        Recompose();
        int x = (int)d.X, y = (int)d.Y;
        if (x >= 0 && y >= 0 && x < composed_.w && y < composed_.h && onColorPicked) onColorPicked(composed_.at(x, y) | 0xFF000000);
        break;
    }
    case Tool::Eraser:
        doc->Checkpoint();
        doc->MutableBase();
        drag_ = Drag::Erase;
        EraseAt(d);
        Refresh();
        return;
    case Tool::Crop:
        drag_ = Drag::Crop; hasCrop_ = false; crop_ = RectF(d.X, d.Y, 0, 0);
        return;
    case Tool::Cutout:
        drag_ = Drag::Cut; selRect_ = RectF(d.X, d.Y, 0, 0);
        return;
    case Tool::Selection: {
        int x = (int)d.X, y = (int)d.Y;
        bool inside = hasSel_ && x >= 0 && y >= 0 && x < doc->w() && y < doc->h() && selMask_[(size_t)y * doc->w() + x];
        if (inside) { StartSmartMove(d); return; }
        ClearRasterSelection();
        if (selMode == 3) {     // varinha (CSelectTool::DoWand)
            cv::Mask m = cv::DoWand(*doc->st.base, { x, y }, tolerance, wandGlobal);
            if (!m.empty()) SetSelectionFromMask(this, std::move(m), hasSel_, selMask_, selBounds_, selOverlay_);
            InvalidateRect(hwnd, nullptr, FALSE);
            ReleaseCapture();
            return;
        }
        if (selMode == 2) { drag_ = Drag::Lasso; lasso_ = { d }; }
        else { drag_ = Drag::Select; selRect_ = RectF(d.X, d.Y, 0, 0); }
        return;
    }
    default: break;
    }
    if (!drawTool) { ReleaseCapture(); return; }

    // criação de objeto
    preview_ = std::make_shared<DrawObj>();
    DrawObj& o = *preview_;
    o.style = s;
    o.p1 = o.p2 = d;
    o.ctrl = d;
    switch (tool) {
    case Tool::Arrow: o.type = ObjType::Arrow; break;
    case Tool::Line: o.type = ObjType::Line; break;
    case Tool::Shape: o.type = ObjType::Shape; break;
    case Tool::Pen: o.type = ObjType::Pen; o.pts = { d }; break;
    // marcador (CPaintOpHighlight, objeto <Highlight>, comp_5 §3.2): sempre um retângulo arrastado, na cor e na
    // opacidade escolhidas
    case Tool::Highlighter: o.type = ObjType::Highlighter; break;
    case Tool::Text: o.type = ObjType::Text; break;
    case Tool::Callout: o.type = ObjType::Callout; break;
    case Tool::Stamp: o.type = ObjType::Stamp; o.text = stampId; break;
    case Tool::Blur: o.type = s.hideKind == 1 ? ObjType::Pixelate : ObjType::Blur; break;
    case Tool::Spotlight: o.type = ObjType::Spotlight; break;
    case Tool::Magnify: o.type = ObjType::Magnify; break;
    default: break;
    }
    drag_ = Drag::Create;
}

void Canvas::OnMove(POINT p, WPARAM keys) {
    if (!doc || doc->isVideo) return;
    PointF d = ToDoc(p);
    bool shift = (keys & MK_SHIFT) != 0;
    float dx = d.X - downDoc_.X, dy = d.Y - downDoc_.Y;
    switch (drag_) {
    case Drag::None: SetCursorForPoint(p); return;
    case Drag::Pan: {
        scroll_.x = panStart_.x - (p.x - downScreen_.x);
        scroll_.y = panStart_.y - (p.y - downScreen_.y);
        UpdateScrollBars();
        break;
    }
    case Drag::Create: {
        DrawObj& o = *preview_;
        if (o.IsPath() && !o.pts.empty()) {
            PointF last = o.pts.back();
            if (std::hypot(d.X - last.X, d.Y - last.Y) * zoom > 1.5) o.pts.push_back(d);
        } else {
            PointF e = d;
            if (shift) {
                if (o.IsLine()) {     // trava em múltiplos de 45°
                    double ang = std::atan2(dy, dx), len = std::hypot(dx, dy);
                    ang = std::round(ang / (3.14159265 / 4)) * (3.14159265 / 4);
                    e = PointF(downDoc_.X + (float)(std::cos(ang) * len), downDoc_.Y + (float)(std::sin(ang) * len));
                } else {
                    float m = std::max(std::fabs(dx), std::fabs(dy));
                    e = PointF(downDoc_.X + (dx < 0 ? -m : m), downDoc_.Y + (dy < 0 ? -m : m));
                }
            }
            o.p2 = e;
            o.ctrl = PointF((o.p1.X + o.p2.X) / 2, (o.p1.Y + o.p2.Y) / 2);
            RectF r = o.Rect();
            if (o.type == ObjType::Callout) {
                int sh = o.style.calloutShape;
                if (sh >= 5 && sh <= 11) o.tail = PointF(r.GetRight() + 30, r.Y + r.Height / 2);           // setas: aponta para a direita
                else if (sh == 4 || sh == 15) o.tail = PointF(r.GetRight() + std::max(40.0f, r.Width * 0.5f), r.GetBottom() + std::max(30.0f, r.Height * 0.6f));
                else o.tail = PointF(r.X + r.Width * 0.25f, r.GetBottom() + std::max(20.0f, r.Height * 0.45f));
            }
            if (o.type == ObjType::Magnify) o.tail = PointF(r.X - r.Width * 0.4f, r.GetBottom() + r.Height * 0.3f);
        }
        break;
    }
    case Drag::Move: {
        if (std::abs(p.x - downScreen_.x) + std::abs(p.y - downScreen_.y) < 2 && !changed_) return;
        changed_ = true;
        if (dragMulti_.empty()) dragMulti_ = { { selected, dragOrig_ } };
        if (snapping && !(keys & MK_ALT)) {      // encaixe: bordas e centro do conjunto nas bordas e no centro da imagem
            RectF r = ObjExtent(dragMulti_[0].second);
            for (auto& m : dragMulti_) RectF::Union(r, r, ObjExtent(m.second));
            float tol = (float)(8 / zoom), W = (float)doc->w(), H = (float)doc->h();
            auto snap = [&](float a0, float len, float total, float d) {
                float best = d, bestDist = tol;
                for (float edge : { 0.0f, total / 2, total })
                    for (float off : { 0.0f, len / 2, len }) {
                        float cand = edge - off - a0, dist = std::fabs(cand - d);
                        if (dist < bestDist) { bestDist = dist; best = cand; }
                    }
                return best;
            };
            dx = snap(r.X, r.Width, W, dx);
            dy = snap(r.Y, r.Height, H, dy);
        }
        for (auto& m : dragMulti_) {
            if (m.first < 0 || m.first >= (int)doc->st.objs.size()) continue;
            DrawObj* o = doc->Mutable(m.first);
            *o = m.second;
            o->Move(dx, dy);
        }
        dirty_ = true;
        break;
    }
    case Drag::Resize: {
        changed_ = true;
        DrawObj* o = doc->Mutable(selected);
        *o = dragOrig_;
        if (o->IsLine()) {
            if (handle_ == 0) o->p1 = d;
            else if (handle_ == 1) o->p2 = d;
            else o->ctrl = PointF(2 * d.X - 0.5f * (o->p1.X + o->p2.X), 2 * d.Y - 0.5f * (o->p1.Y + o->p2.Y));   // a curva passa pelo ponto
            if (handle_ != 2 && !o->style.bezier) o->ctrl = PointF((o->p1.X + o->p2.X) / 2, (o->p1.Y + o->p2.Y) / 2);
        }
        else if ((o->type == ObjType::Callout || o->type == ObjType::Magnify) && handle_ == 8) o->tail = d;
        else {
            RectF r0 = o->IsPath() ? [&] { RectF b = dragOrig_.Bounds(); float pad = dragOrig_.style.width / 2 + (dragOrig_.style.shadow ? 4 : 0); b.Inflate(-pad, -pad); return b; }() : dragOrig_.Rect();
            float l = r0.X, t = r0.Y, r = r0.GetRight(), b = r0.GetBottom();
            switch (handle_) {
            case 0: l += dx; t += dy; break; case 1: t += dy; break; case 2: r += dx; t += dy; break; case 3: r += dx; break;
            case 4: r += dx; b += dy; break; case 5: b += dy; break; case 6: l += dx; b += dy; break; case 7: l += dx; break;
            }
            if (shift && (handle_ % 2 == 0) && r0.Width > 0 && r0.Height > 0) {     // proporcional
                float sx = (r - l) / r0.Width, sy = (b - t) / r0.Height, s = std::max(std::fabs(sx), std::fabs(sy));
                float nw = r0.Width * s, nh = r0.Height * s;
                if (handle_ == 0) { l = r - nw; t = b - nh; } else if (handle_ == 2) { r = l + nw; t = b - nh; }
                else if (handle_ == 4) { r = l + nw; b = t + nh; } else { l = r - nw; b = t + nh; }
            }
            RectF to(std::min(l, r), std::min(t, b), std::fabs(r - l), std::fabs(b - t));
            if (o->type == ObjType::Callout || o->type == ObjType::Magnify) {
                PointF tail = dragOrig_.tail;
                o->p1 = PointF(to.X, to.Y); o->p2 = PointF(to.GetRight(), to.GetBottom()); o->tail = tail;
            } else { o->Scale(r0, to); o->UpdateGroupRect(); }
        }
        dirty_ = true;
        break;
    }
    case Drag::Crop: case Drag::Select: case Drag::Cut: {
        float x0 = std::clamp(std::min(downDoc_.X, d.X), 0.0f, (float)doc->w()), y0 = std::clamp(std::min(downDoc_.Y, d.Y), 0.0f, (float)doc->h());
        float x1 = std::clamp(std::max(downDoc_.X, d.X), 0.0f, (float)doc->w()), y1 = std::clamp(std::max(downDoc_.Y, d.Y), 0.0f, (float)doc->h());
        RectF r((float)(int)x0, (float)(int)y0, (float)(int)(x1 - (int)x0 + 0.5f), (float)(int)(y1 - (int)y0 + 0.5f));
        if (drag_ == Drag::Crop) crop_ = r; else selRect_ = r;
        break;
    }
    case Drag::Lasso:
        if (std::hypot(d.X - lasso_.back().X, d.Y - lasso_.back().Y) * zoom > 2) lasso_.push_back(d);
        break;
    case Drag::Marquee:
        selRect_ = RectF(std::min(downDoc_.X, d.X), std::min(downDoc_.Y, d.Y), std::fabs(d.X - downDoc_.X), std::fabs(d.Y - downDoc_.Y));
        break;
    case Drag::Erase: EraseAt(d); dirty_ = true; break;
    }
    InvalidateRect(hwnd, nullptr, FALSE);
}

void Canvas::FinishCreate() {
    DrawObj o = *preview_;
    preview_.reset();
    RectF r = o.Rect();
    float minSz = (float)(6 / zoom);
    bool tiny = o.IsPath() && !o.pts.empty() ? false : (o.IsLine() ? std::hypot(o.p2.X - o.p1.X, o.p2.Y - o.p1.Y) < minSz
                                                                                   : (r.Width < minSz || r.Height < minSz));
    if (o.type == ObjType::Text && r.Width >= minSz * 3) {   // caixa arrastada: largura dela, altura mínima de uma linha
        tiny = false;
        float minH = o.style.fontSize * 1.4f + 2 * o.style.padding;
        if (r.Height < minH) { o.p1 = PointF(r.X, r.Y); o.p2 = PointF(r.GetRight(), r.Y + minH); }
    }
    if (tiny) {
        if (o.type == ObjType::Text) { o.p2 = PointF(o.p1.X + 260, o.p1.Y + o.style.fontSize * 1.6f); }
        else if (o.type == ObjType::Callout) {
            o.p2 = PointF(o.p1.X + 220, o.p1.Y + 90);
            o.tail = PointF(o.p1.X + 50, o.p1.Y + 150);
        } else if (o.type == ObjType::Stamp) { o.p1 = PointF(o.p1.X - 48, o.p1.Y - 48); o.p2 = PointF(o.p1.X + 96, o.p1.Y + 96); }
        else return;
    }
    doc->Checkpoint();
    doc->st.objs.push_back(std::make_shared<DrawObj>(o));
    selected = (int)doc->st.objs.size() - 1; extra_.clear();
    dirty_ = true;
    if (onSelectionChanged) onSelectionChanged();
    if (o.HasText()) BeginTextEdit(selected, true);
}

void Canvas::OnUp(POINT p) {
    ReleaseCapture();
    if (!doc) return;
    Drag d = drag_;
    drag_ = Drag::None;
    switch (d) {
    case Drag::Create: FinishCreate(); break;
    case Drag::Move: case Drag::Resize:
        if (!changed_) doc->DiscardCheckpoint();
        break;
    case Drag::Crop:
        hasCrop_ = crop_.Width >= 2 && crop_.Height >= 2;
        break;
    case Drag::Select: {
        if (selRect_.Width >= 2 && selRect_.Height >= 2) {
            RECT r{ (LONG)selRect_.X, (LONG)selRect_.Y, (LONG)selRect_.GetRight(), (LONG)selRect_.GetBottom() };
            cv::Mask m = selMode == 1 ? cv::EllipseMask(doc->w(), doc->h(), r) : cv::RectMask(doc->w(), doc->h(), r);
            SetSelectionFromMask(this, std::move(m), hasSel_, selMask_, selBounds_, selOverlay_);
        }
        break;
    }
    case Drag::Lasso: {
        if (lasso_.size() >= 3) {
            std::vector<POINT> pts;
            for (auto& q : lasso_) pts.push_back({ (LONG)q.X, (LONG)q.Y });
            SetSelectionFromMask(this, cv::PolygonMask(doc->w(), doc->h(), pts), hasSel_, selMask_, selBounds_, selOverlay_);
        }
        lasso_.clear();
        break;
    }
    case Drag::Cut: {
        // CutOut (CPaintOpTearOutTool): remove a faixa e junta as partes
        const Image& b = *doc->st.base;
        int a0 = cutDir == 0 ? (int)selRect_.Y : (int)selRect_.X;
        int a1 = cutDir == 0 ? (int)selRect_.GetBottom() : (int)selRect_.GetRight();
        int len = a1 - a0;
        int total = cutDir == 0 ? b.h : b.w;
        if (len < 2 || len >= total) break;
        doc->Checkpoint();
        Image out(cutDir == 0 ? b.w : b.w - len, cutDir == 0 ? b.h - len : b.h);
        for (int y = 0; y < out.h; y++)
            for (int x = 0; x < out.w; x++) {
                int sx = x, sy = y;
                if (cutDir == 0 && y >= a0) sy = y + len;
                if (cutDir == 1 && x >= a0) sx = x + len;
                out.at(x, y) = b.at(sx, sy);
            }
        doc->SetBase(std::move(out));
        for (size_t i = 0; i < doc->st.objs.size(); i++) {
            RectF ob = doc->st.objs[i]->Bounds();
            float pos = cutDir == 0 ? ob.Y : ob.X;
            if (pos >= a1) doc->Mutable(i)->Move(cutDir == 1 ? (float)-len : 0, cutDir == 0 ? (float)-len : 0);
        }
        dirty_ = true;
        FitToWindow();
        break;
    }
    case Drag::Erase: dirty_ = true; break;
    case Drag::Marquee: {                       // seleciona os objetos que o retângulo toca (somando aos de antes com Ctrl/Shift)
        std::vector<int> v;
        if (GetKeyState(VK_CONTROL) < 0 || GetKeyState(VK_SHIFT) < 0) v = SelectedList();
        if (selRect_.Width * zoom >= 3 || selRect_.Height * zoom >= 3)
            for (int i = 0; i < (int)doc->st.objs.size(); i++) {
                RectF b = doc->st.objs[i]->Bounds();
                if (b.IntersectsWith(selRect_) && std::find(v.begin(), v.end(), i) == v.end()) v.push_back(i);
            }
        SelectObjects(v);
        break;
    }
    default: break;
    }
    Refresh();
    if (d == Drag::Create || d == Drag::Move || d == Drag::Resize || d == Drag::Cut || d == Drag::Erase)
        if (onDocChanged) onDocChanged();
    if (d == Drag::Move || d == Drag::Resize) if (onSelectionChanged) onSelectionChanged();
}

void Canvas::SetCursorForPoint(POINT p) {
    LPCWSTR c = IDC_ARROW;
    int hh = HitHandle(p);
    if (hh >= 0) {
        static const LPCWSTR rc[] = { IDC_SIZENWSE, IDC_SIZENS, IDC_SIZENESW, IDC_SIZEWE, IDC_SIZENWSE, IDC_SIZENS, IDC_SIZENESW, IDC_SIZEWE, IDC_CROSS };
        const DrawObj& o = *doc->st.objs[selected];
        c = o.IsLine() ? IDC_CROSS : rc[std::min(hh, 8)];
    } else if (tool == Tool::Select) {
        c = HitObject(ToDoc(p)) >= 0 ? IDC_SIZEALL : IDC_ARROW;
    } else if (tool == Tool::Selection) {
        PointF d = ToDoc(p); int x = (int)d.X, y = (int)d.Y;
        bool inside = hasSel_ && x >= 0 && y >= 0 && x < doc->w() && y < doc->h() && selMask_[(size_t)y * doc->w() + x];
        c = inside ? IDC_SIZEALL : IDC_CROSS;
    } else if (tool == Tool::Fill || tool == Tool::Eyedropper) c = IDC_HAND;
    else {
        int hit = HitObject(ToDoc(p));
        c = (hit >= 0 && hit == selected) ? IDC_SIZEALL : IDC_CROSS;
    }
    SetCursor(LoadCursor(nullptr, c));
}

// ---------------------------------------------------------------- mensagens
LRESULT CALLBACK Canvas::Proc(HWND h, UINT m, WPARAM w, LPARAM l) {
    Canvas* self = (Canvas*)GetWindowLongPtrW(h, GWLP_USERDATA);
    if (m == WM_NCCREATE) {
        self = (Canvas*)((CREATESTRUCTW*)l)->lpCreateParams;
        SetWindowLongPtrW(h, GWLP_USERDATA, (LONG_PTR)self);
        self->hwnd = h;
    }
    if (!self) return DefWindowProcW(h, m, w, l);
    return self->Handle(m, w, l);
}

LRESULT Canvas::Handle(UINT m, WPARAM w, LPARAM l) {
    switch (m) {
    case WM_ERASEBKGND: return 1;
    case WM_PAINT: { PAINTSTRUCT ps; HDC dc = BeginPaint(hwnd, &ps); Paint(dc); EndPaint(hwnd, &ps); return 0; }
    case WM_SIZE: if (fit_) FitToWindow(); else UpdateScrollBars(); return 0;
    case WM_LBUTTONDOWN: OnDown({ GET_X_LPARAM(l), GET_Y_LPARAM(l) }, false); return 0;
    case WM_LBUTTONDBLCLK: {
        POINT p{ GET_X_LPARAM(l), GET_Y_LPARAM(l) };
        if (tool == Tool::Crop && hasCrop_) { ApplyCrop(); return 0; }
        if (doc) {
            int hit = HitObject(ToDoc(p));
            if (hit >= 0 && doc->st.objs[hit]->HasText()) { selected = hit; extra_.clear(); BeginTextEdit(hit, false); return 0; }
        }
        OnDown(p, true);
        return 0;
    }
    case WM_MOUSEMOVE: OnMove({ GET_X_LPARAM(l), GET_Y_LPARAM(l) }, w); return 0;
    case WM_LBUTTONUP: OnUp({ GET_X_LPARAM(l), GET_Y_LPARAM(l) }); return 0;
    case WM_MBUTTONDOWN: SetCapture(hwnd); drag_ = Drag::Pan; downScreen_ = { GET_X_LPARAM(l), GET_Y_LPARAM(l) }; panStart_ = { scroll_.x, scroll_.y }; return 0;
    case WM_MBUTTONUP: ReleaseCapture(); drag_ = Drag::None; return 0;
    case WM_SETCURSOR:
        if (LOWORD(l) == HTCLIENT && doc && !doc->isVideo) { POINT p; GetCursorPos(&p); ScreenToClient(hwnd, &p); SetCursorForPoint(p); return TRUE; }
        break;
    case WM_MOUSEWHEEL: {
        int delta = GET_WHEEL_DELTA_WPARAM(w);
        if (GET_KEYSTATE_WPARAM(w) & MK_CONTROL) {
            POINT p{ GET_X_LPARAM(l), GET_Y_LPARAM(l) }; ScreenToClient(hwnd, &p);
            ZoomTo(zoom * (delta > 0 ? 1.15 : 1 / 1.15), &p);
        } else {
            CommitText();
            if (GET_KEYSTATE_WPARAM(w) & MK_SHIFT) scroll_.x -= delta; else scroll_.y -= delta;
            UpdateScrollBars(); InvalidateRect(hwnd, nullptr, FALSE);
        }
        return 0;
    }
    case WM_MOUSEHWHEEL: scroll_.x += GET_WHEEL_DELTA_WPARAM(w); UpdateScrollBars(); InvalidateRect(hwnd, nullptr, FALSE); return 0;
    case WM_HSCROLL: case WM_VSCROLL: {
        CommitText();
        int bar = m == WM_HSCROLL ? SB_HORZ : SB_VERT;
        SCROLLINFO si{ sizeof(si), SIF_ALL };
        GetScrollInfo(hwnd, bar, &si);
        int pos = si.nPos;
        switch (LOWORD(w)) {
        case SB_LINEUP: pos -= 30; break; case SB_LINEDOWN: pos += 30; break;
        case SB_PAGEUP: pos -= si.nPage; break; case SB_PAGEDOWN: pos += si.nPage; break;
        case SB_THUMBTRACK: case SB_THUMBPOSITION: pos = si.nTrackPos; break;
        }
        (bar == SB_HORZ ? scroll_.x : scroll_.y) = pos;
        UpdateScrollBars(); InvalidateRect(hwnd, nullptr, FALSE);
        return 0;
    }
    case WM_KEYDOWN:
        switch (w) {
        case VK_DELETE: case VK_BACK: DeleteSelected(); return 0;
        case VK_ESCAPE:
            if (drag_ != Drag::None) { if ((drag_ == Drag::Move || drag_ == Drag::Resize) && changed_) { doc->Undo(); dirty_ = true; } else if (drag_ == Drag::Move || drag_ == Drag::Resize) doc->DiscardCheckpoint(); ReleaseCapture(); drag_ = Drag::None; preview_.reset(); }
            hasCrop_ = false; ClearRasterSelection();
            if (selected >= 0) { selected = -1; extra_.clear(); if (onSelectionChanged) onSelectionChanged(); }
            InvalidateRect(hwnd, nullptr, FALSE);
            return 0;
        case VK_RETURN: if (hasCrop_) ApplyCrop(); return 0;
        case VK_LEFT: Nudge(GetKeyState(VK_SHIFT) < 0 ? -10 : -1, 0); return 0;
        case VK_RIGHT: Nudge(GetKeyState(VK_SHIFT) < 0 ? 10 : 1, 0); return 0;
        case VK_UP: Nudge(0, GetKeyState(VK_SHIFT) < 0 ? -10 : -1); return 0;
        case VK_DOWN: Nudge(0, GetKeyState(VK_SHIFT) < 0 ? 10 : 1); return 0;
        }
        break;
    case WM_GETDLGCODE: return DLGC_WANTARROWS | DLGC_WANTCHARS;
    case WM_COMMAND:
        if (LOWORD(w) == 77 && HIWORD(w) == EN_KILLFOCUS) { CommitText(); return 0; }
        if (LOWORD(w) == 77 && HIWORD(w) == EN_CHANGE) { LayoutTextEdit(); return 0; }
        break;
    case WM_CTLCOLOREDIT: {
        HDC dc = (HDC)w;
        if (doc && editIdx_ >= 0 && editIdx_ < (int)doc->st.objs.size()) {
            const Style& s = doc->st.objs[editIdx_]->style;
            SetTextColor(dc, ToColorref(s.textColor));
            uint32_t bg = (s.fill >> 24) ? s.fill : 0xFFFFFFFF;
            SetBkColor(dc, ToColorref(bg));
            static HBRUSH br = nullptr;
            if (br) DeleteObject(br);
            br = CreateSolidBrush(ToColorref(bg));
            return (LRESULT)br;
        }
        break;
    }
    }
    return DefWindowProcW(hwnd, m, w, l);
}

} // namespace snap::ed
