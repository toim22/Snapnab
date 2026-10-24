#include "Canvas.h"
#include "Effects.h"
#include "Ui.h"
#include <cmath>

namespace snap::ed {

using Gdiplus::PointF;
using Gdiplus::RectF;

const wchar_t* ToolName(Tool t) {
    switch (t) {
    case Tool::Select: return L"Mover";
    case Tool::Arrow: return L"Seta";
    case Tool::Line: return L"Linha";
    case Tool::Shape: return L"Forma";
    case Tool::Pen: return L"Caneta";
    case Tool::Highlighter: return L"Marcador";
    case Tool::Text: return L"Texto";
    case Tool::Callout: return L"Balão";
    case Tool::Step: return L"Passo";
    case Tool::Stamp: return L"Carimbo";
    case Tool::Blur: return L"Ocultar";
    case Tool::Spotlight: return L"Destaque";
    case Tool::Magnify: return L"Lupa";
    case Tool::Fill: return L"Preencher";
    case Tool::Eraser: return L"Borracha";
    case Tool::Crop: return L"Cortar";
    case Tool::Selection: return L"Seleção";
    case Tool::Cutout: return L"Recortar faixa";
    case Tool::Eyedropper: return L"Conta-gotas";
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
    // estilos de fábrica por ferramenta (quick styles)
    Style base;
    styles[Tool::Arrow] = base; styles[Tool::Arrow].width = 6;
    styles[Tool::Line] = base;
    styles[Tool::Shape] = base;
    styles[Tool::Pen] = base;
    Style hl; hl.stroke = 0xFFFFE600; hl.width = 18; hl.shadow = false; styles[Tool::Highlighter] = hl;
    Style tx; tx.textColor = 0xFFE5322D; tx.fontSize = 28; tx.width = 0; styles[Tool::Text] = tx;
    Style co; co.fill = 0xFF1E78E6; co.stroke = 0xFF145AB4; co.width = 0; co.textColor = 0xFFFFFFFF; co.fontSize = 22; styles[Tool::Callout] = co;
    Style st; st.fontSize = 26; st.textColor = 0xFFFFFFFF; styles[Tool::Step] = st;
    styles[Tool::Stamp] = base;
    Style bl; bl.amount = 10; bl.shadow = false; styles[Tool::Blur] = bl;
    Style sp; sp.amount = 10; sp.shadow = false; styles[Tool::Spotlight] = sp;
    Style mg; mg.amount = 10; mg.stroke = 0xFF404040; mg.width = 3; styles[Tool::Magnify] = mg;
    styles[Tool::Fill] = base;
    Style er; er.width = 24; styles[Tool::Eraser] = er;
    styles[Tool::Select] = base; styles[Tool::Crop] = base; styles[Tool::Selection] = base; styles[Tool::Cutout] = base; styles[Tool::Eyedropper] = base;

    hwnd = CreateWindowExW(0, L"SnapnabCanvas", nullptr, WS_CHILD | WS_VISIBLE | WS_HSCROLL | WS_VSCROLL | WS_CLIPCHILDREN,
                           0, 0, 10, 10, parent, nullptr, GetModuleHandleW(nullptr), this);
}

Style& Canvas::CurrentStyle() { return styles[tool]; }

void Canvas::SetDocument(Document* d) {
    CommitText();
    doc = d;
    selected = -1;
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
    if (t != Tool::Selection) ClearRasterSelection();
    if (t != Tool::Crop) hasCrop_ = false;
    tool = t;
    if (t != Tool::Select && selected >= 0 && doc) {
        // mantém a seleção só se o objeto é do tipo da ferramenta
        selected = -1;
        if (onSelectionChanged) onSelectionChanged();
    }
    InvalidateRect(hwnd, nullptr, FALSE);
}

void Canvas::Refresh(bool docChanged) {
    if (docChanged) dirty_ = true;
    if (doc && selected >= (int)doc->st.objs.size()) selected = -1;
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
    if (o.IsLine()) { h.push_back(ToScreen(o.p1)); h.push_back(ToScreen(o.p2)); return h; }
    if (o.type == ObjType::Step) return h;
    RectF r = o.IsPath() ? [&] { RectF b = o.Bounds(); float pad = o.style.width / 2 + (o.style.shadow ? 4 : 0); b.Inflate(-pad, -pad); return b; }() : o.Rect();
    RECT s = ToScreenRect(r);
    int mx = (s.left + s.right) / 2, my = (s.top + s.bottom) / 2;
    h = { { s.left, s.top }, { mx, s.top }, { s.right, s.top }, { s.right, my },
          { s.right, s.bottom }, { mx, s.bottom }, { s.left, s.bottom }, { s.left, my } };
    if (o.type == ObjType::Callout) h.push_back(ToScreen(o.tail));
    return h;
}

int Canvas::HitHandle(POINT p) const {
    if (!doc || selected < 0 || selected >= (int)doc->st.objs.size()) return -1;
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
        ui::DrawTextCentered(mem, doc ? L"" : L"Nenhuma captura aberta — use “Capturar” ou abra um arquivo (Ctrl+O).",
                             cr, RGB(90, 90, 90), ui::UiFont(dpi, 11));
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
            std::wstring t = Format(L"%d × %d  — Enter aplica, Esc cancela", (int)crop_.Width, (int)crop_.Height);
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
        if (selected >= 0 && selected < (int)doc->st.objs.size() && !edit_) {
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
                bool tail = obj.type == ObjType::Callout && i == 8;
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
    if (selected >= 0 && selected < (int)doc->st.objs.size()) {
        RectF b = doc->st.objs[selected]->Bounds();
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
    if (selected >= 0 && selected < (int)doc->st.objs.size()) {
        doc->Checkpoint();
        doc->st.objs.erase(doc->st.objs.begin() + selected);
        selected = -1;
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
    selected = (int)doc->st.objs.size() - 1;
    SetTool(Tool::Select);
    selected = (int)doc->st.objs.size() - 1;
    Refresh(); if (onDocChanged) onDocChanged(); if (onSelectionChanged) onSelectionChanged();
}

void Canvas::BringToFront() {
    if (!doc || selected < 0) return;
    doc->Checkpoint();
    auto o = doc->st.objs[selected];
    doc->st.objs.erase(doc->st.objs.begin() + selected);
    doc->st.objs.push_back(o);
    selected = (int)doc->st.objs.size() - 1;
    Refresh(); if (onDocChanged) onDocChanged();
}

void Canvas::SendToBack() {
    if (!doc || selected < 0) return;
    doc->Checkpoint();
    auto o = doc->st.objs[selected];
    doc->st.objs.erase(doc->st.objs.begin() + selected);
    doc->st.objs.insert(doc->st.objs.begin(), o);
    selected = 0;
    Refresh(); if (onDocChanged) onDocChanged();
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

void Canvas::ApplyStyleToSelected(const Style& s) {
    if (!doc || selected < 0 || selected >= (int)doc->st.objs.size()) return;
    doc->Checkpoint();
    DrawObj* o = doc->Mutable(selected);
    o->style = s;
    Refresh(); if (onDocChanged) onDocChanged();
}

void Canvas::ChangeSelectedKind() {
    if (!doc || selected < 0 || selected >= (int)doc->st.objs.size()) return;
    const DrawObj& cur = *doc->st.objs[selected];
    ObjType nt = cur.type;
    if (cur.type == ObjType::Rect || cur.type == ObjType::Ellipse || cur.type == ObjType::RoundRect)
        nt = shapeKind == 1 ? ObjType::Ellipse : shapeKind == 2 ? ObjType::RoundRect : ObjType::Rect;
    else if (cur.type == ObjType::Blur || cur.type == ObjType::Pixelate)
        nt = hideKind == 1 ? ObjType::Pixelate : ObjType::Blur;
    else if (cur.type == ObjType::Stamp) {
        auto g = StampGlyphs();
        if (stampIndex >= 0 && stampIndex < (int)g.size() && g[stampIndex] != cur.text) {
            doc->Checkpoint(); doc->Mutable(selected)->text = g[stampIndex]; Refresh(); if (onDocChanged) onDocChanged();
        }
        return;
    }
    if (nt == cur.type) return;
    doc->Checkpoint();
    doc->Mutable(selected)->type = nt;
    Refresh(); if (onDocChanged) onDocChanged();
}

void Canvas::Nudge(int dx, int dy) {
    if (!doc || selected < 0 || selected >= (int)doc->st.objs.size()) return;
    doc->Checkpoint();
    doc->Mutable(selected)->Move((float)dx, (float)dy);
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
    RECT r = ToScreenRect(o.Rect());
    if (o.type == ObjType::Callout) {
        int in = (int)((std::min(r.right - r.left, r.bottom - r.top)) * (o.style.calloutShape >= 2 ? 0.18 : 0.08)) + 4;
        InflateRect(&r, -in, -in);
    }
    if (r.right - r.left < 60) r.right = r.left + 60;
    if (r.bottom - r.top < 30) r.bottom = r.top + 30;
    LOGFONTW lf{};
    lf.lfHeight = -std::max(8, (int)std::lround(o.style.fontSize * zoom));
    lf.lfWeight = o.style.bold ? FW_BOLD : FW_NORMAL;
    lf.lfQuality = CLEARTYPE_QUALITY;
    wcsncpy_s(lf.lfFaceName, o.style.font.c_str(), _TRUNCATE);
    if (editFont_) DeleteObject(editFont_);
    editFont_ = CreateFontIndirectW(&lf);
    DWORD align = o.type == ObjType::Callout ? ES_CENTER : ES_LEFT;
    edit_ = CreateWindowExW(0, L"EDIT", nullptr, WS_CHILD | WS_VISIBLE | ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN | align,
                            r.left, r.top, r.right - r.left, r.bottom - r.top, hwnd, (HMENU)77, GetModuleHandleW(nullptr), nullptr);
    SendMessageW(edit_, WM_SETFONT, (WPARAM)editFont_, TRUE);
    std::wstring t;
    for (wchar_t c : o.text) { if (c == L'\n') t += L"\r\n"; else t += c; }
    SetWindowTextW(edit_, t.c_str());
    SendMessageW(edit_, EM_SETSEL, 0, -1);
    SetFocus(edit_);
    dirty_ = true;
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
            doc->Mutable(editIdx_)->text = clean;
        } else if (!editNew_) doc->DiscardCheckpoint();
    }
    editIdx_ = -1;
    dirty_ = true;
    InvalidateRect(hwnd, nullptr, FALSE);
    if (onDocChanged) onDocChanged();
    SetFocus(hwnd);
}

// ---------------------------------------------------------------- mouse
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
    selected = (int)doc->st.objs.size() - 1;
    dragOrig_ = *o;
    drag_ = Drag::Move;
    changed_ = true;
    dirty_ = true;
    if (onSelectionChanged) onSelectionChanged();
}

void Canvas::EraseAt(PointF p) {
    Image& base = const_cast<Image&>(*doc->st.base);
    float r = std::max(2.0f, CurrentStyle().width / 2);
    for (int y = (int)(p.Y - r); y <= (int)(p.Y + r); y++)
        for (int x = (int)(p.X - r); x <= (int)(p.X + r); x++) {
            if (x < 0 || y < 0 || x >= base.w || y >= base.h) continue;
            float dx = x + 0.5f - p.X, dy = y + 0.5f - p.Y;
            if (dx * dx + dy * dy <= r * r) base.at(x, y) = 0;
        }
}

void Canvas::OnDown(POINT p, bool dbl) {
    if (!doc || doc->isVideo) return;
    if (edit_) { CommitText(); return; }      // clicar fora só confirma o texto (não cria outro objeto)
    SetFocus(hwnd);
    PointF d = ToDoc(p);
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
            if (tool == Tool::Select || hit == selected || (GetKeyState(VK_CONTROL) >= 0 && tool != Tool::Step && tool != Tool::Stamp)) {
                if (dbl && doc->st.objs[hit]->HasText()) { selected = hit; BeginTextEdit(hit, false); ReleaseCapture(); return; }
                if (hit != selected) { selected = hit; if (onSelectionChanged) onSelectionChanged(); }
                doc->Checkpoint();
                dragOrig_ = *doc->st.objs[hit];
                drag_ = Drag::Move;
                InvalidateRect(hwnd, nullptr, FALSE);
                return;
            }
        }
        if (tool == Tool::Select) {
            if (selected != -1) { selected = -1; if (onSelectionChanged) onSelectionChanged(); InvalidateRect(hwnd, nullptr, FALSE); }
            drag_ = Drag::Pan; panStart_ = { scroll_.x, scroll_.y };
            return;
        }
    }
    if (selected != -1 && drawTool) { selected = -1; if (onSelectionChanged) onSelectionChanged(); }

    Style s = CurrentStyle();
    switch (tool) {
    case Tool::Step: {
        doc->Checkpoint();
        auto o = std::make_shared<DrawObj>();
        o->type = ObjType::Step; o->style = s; o->p1 = o->p2 = d;
        o->stepValue = doc->st.nextStep++;
        doc->st.objs.push_back(o);
        selected = (int)doc->st.objs.size() - 1;
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
    switch (tool) {
    case Tool::Arrow: o.type = ObjType::Arrow; break;
    case Tool::Line: o.type = ObjType::Line; o.style.arrowEnd = 0; break;
    case Tool::Shape: o.type = shapeKind == 1 ? ObjType::Ellipse : shapeKind == 2 ? ObjType::RoundRect : ObjType::Rect; break;
    case Tool::Pen: o.type = ObjType::Pen; o.pts = { d }; break;
    case Tool::Highlighter: o.type = ObjType::Highlighter; o.pts = { d }; break;
    case Tool::Text: o.type = ObjType::Text; break;
    case Tool::Callout: o.type = ObjType::Callout; break;
    case Tool::Stamp: { o.type = ObjType::Stamp; auto g = StampGlyphs(); o.text = g[std::clamp(stampIndex, 0, (int)g.size() - 1)]; break; }
    case Tool::Blur: o.type = hideKind == 1 ? ObjType::Pixelate : ObjType::Blur; break;
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
        if (o.IsPath()) {
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
            if (o.type == ObjType::Callout) {
                RectF r = o.Rect();
                o.tail = PointF(r.X + r.Width * 0.25f, r.GetBottom() + std::max(20.0f, r.Height * 0.45f));
            }
        }
        break;
    }
    case Drag::Move: {
        if (std::abs(p.x - downScreen_.x) + std::abs(p.y - downScreen_.y) < 2 && !changed_) return;
        changed_ = true;
        DrawObj* o = doc->Mutable(selected);
        *o = dragOrig_;
        o->Move(dx, dy);
        dirty_ = true;
        break;
    }
    case Drag::Resize: {
        changed_ = true;
        DrawObj* o = doc->Mutable(selected);
        *o = dragOrig_;
        if (o->IsLine()) { if (handle_ == 0) o->p1 = d; else o->p2 = d; }
        else if (o->type == ObjType::Callout && handle_ == 8) o->tail = d;
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
            if (o->type == ObjType::Callout) {
                PointF tail = dragOrig_.tail;
                o->p1 = PointF(to.X, to.Y); o->p2 = PointF(to.GetRight(), to.GetBottom()); o->tail = tail;
            } else o->Scale(r0, to);
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
    case Drag::Erase: EraseAt(d); dirty_ = true; break;
    }
    InvalidateRect(hwnd, nullptr, FALSE);
}

void Canvas::FinishCreate() {
    DrawObj o = *preview_;
    preview_.reset();
    RectF r = o.Rect();
    float minSz = (float)(6 / zoom);
    bool tiny = o.IsPath() ? o.pts.size() < 2 && o.type == ObjType::Pen && false : (o.IsLine() ? std::hypot(o.p2.X - o.p1.X, o.p2.Y - o.p1.Y) < minSz
                                                                                              : (r.Width < minSz || r.Height < minSz));
    if (tiny) {
        if (o.type == ObjType::Text) { o.p2 = PointF(o.p1.X + 260, o.p1.Y + o.style.fontSize * 1.6f); }
        else if (o.type == ObjType::Callout) {
            o.p2 = PointF(o.p1.X + 220, o.p1.Y + 90);
            o.tail = PointF(o.p1.X + 50, o.p1.Y + 150);
        } else if (o.type == ObjType::Stamp) { o.p1 = PointF(o.p1.X - 36, o.p1.Y - 36); o.p2 = PointF(o.p1.X + 72, o.p1.Y + 72); }
        else return;
    }
    doc->Checkpoint();
    doc->st.objs.push_back(std::make_shared<DrawObj>(o));
    selected = (int)doc->st.objs.size() - 1;
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
    } else if (tool == Tool::Text) c = IDC_IBEAM;
    else if (tool == Tool::Fill || tool == Tool::Eyedropper) c = IDC_HAND;
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
            if (hit >= 0 && doc->st.objs[hit]->HasText()) { selected = hit; BeginTextEdit(hit, false); return 0; }
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
            if (selected >= 0) { selected = -1; if (onSelectionChanged) onSelectionChanged(); }
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
