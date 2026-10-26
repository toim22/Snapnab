#include "Canvas.h"
#include "Effects.h"
#include "Ui.h"
#include "Icons.h"
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
    // built-in styles per tool: the first quick style of the Basic theme (QuickStyles replaces them later)
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
    if (drag_ == Drag::Polygon) ReleaseCapture();
    doc = d;
    selected = -1;
    extra_.clear();
    hasSel_ = false; hasCrop_ = false;
    drag_ = Drag::None;
    preview_.reset();
    labelReady_ = false;
    linkHot_ = -1;
    fit_ = true;
    dirty_ = true;
    FitToWindow();
    if (onSelectionChanged) onSelectionChanged();
}

void Canvas::SetTool(Tool t) {
    CommitText();
    ResetPolygon();
    labelReady_ = false;
    if (t == Tool::MagicWand) { selMode = 3; t = Tool::Selection; }
    if (t == Tool::Favorites) t = tool;
    if (t != Tool::Selection && !(t == Tool::Select && smartMove)) ClearRasterSelection();
    if (t != Tool::Crop) hasCrop_ = false;
    tool = t;
    if (t != Tool::Select && selected >= 0 && doc) {
        // keeps the selection only if the object is of the tool's kind
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
    if (!doc || doc->isVideo) return;
    if (!dirty_ && erasePending_) {
        // eraser stroke over a picture without objects: composed_ is the base, so only the touched pixels are copied
        const Image& base = *doc->st.base;
        if (composed_.w == base.w && composed_.h == base.h) {
            for (LONG y = eraseRect_.top; y < eraseRect_.bottom; y++)
                memcpy(&composed_.at(eraseRect_.left, y), base.px.data() + (size_t)y * base.w + eraseRect_.left,
                       (eraseRect_.right - eraseRect_.left) * 4);
            composedTransparent_ = true;
            if (displayValid_) UpdateDisplay(composed_, &eraseRect_);
            if (scaledPartial_) UnionRect(&scaledDirty_, &scaledDirty_, &eraseRect_);
            else { scaledDirty_ = eraseRect_; scaledPartial_ = true; }
            erasePending_ = false;
            return;
        }
        dirty_ = true;
    }
    erasePending_ = false;
    if (!dirty_) return;
    const DrawObj* hide = (edit_ && editIdx_ >= 0 && editIdx_ < (int)doc->st.objs.size()) ? doc->st.objs[editIdx_].get() : nullptr;
    composed_ = doc->Render(hide);
    composedTransparent_ = composed_.HasTransparency();
    displayValid_ = scaledValid_ = scaledPartial_ = false;
    dirty_ = false;
}

void Canvas::DrawComposed(HDC mem, RECT ir) {
    if (composedTransparent_ && (!displayValid_ || displayZoom_ != zoom)) { UpdateDisplay(composed_, nullptr); scaledValid_ = false; }
    const Image& src = composedTransparent_ ? display_ : composed_;
    int iw = ir.right - ir.left, ih = ir.bottom - ir.top;
    if (iw <= 0 || ih <= 0 || src.empty()) return;
    HDC sdc = CreateCompatibleDC(mem);
    if (!scaled_ || scaledW_ != iw || scaledH_ != ih) {
        if (scaled_) DeleteObject(scaled_);
        BITMAPINFO bi{}; bi.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
        bi.bmiHeader.biWidth = iw; bi.bmiHeader.biHeight = -ih; bi.bmiHeader.biPlanes = 1; bi.bmiHeader.biBitCount = 32;
        void* bits = nullptr;
        scaled_ = CreateDIBSection(mem, &bi, DIB_RGB_COLORS, &bits, nullptr, 0);
        scaledW_ = iw; scaledH_ = ih; scaledValid_ = false;
    }
    HGDIOBJ os = SelectObject(sdc, scaled_);
    if (!scaledValid_) {
        src.DrawTo(sdc, 0, 0, iw, ih, zoom < 1.0);
        scaledValid_ = true;
    } else if (scaledPartial_) {
        // only the erased rows/columns, with a margin for the HALFTONE filter; the source rows are passed as their
        // own top-down DIB (StretchDIBits' ySrc is unreliable for top-down bitmaps)
        double sx = (double)iw / src.w, sy = (double)ih / src.h;
        int dx0 = std::max(0, (int)std::floor(scaledDirty_.left * sx) - 2), dx1 = std::min(iw, (int)std::ceil(scaledDirty_.right * sx) + 2);
        int dy0 = std::max(0, (int)std::floor(scaledDirty_.top * sy) - 2), dy1 = std::min(ih, (int)std::ceil(scaledDirty_.bottom * sy) + 2);
        int x0 = std::clamp((int)std::floor(dx0 / sx), 0, src.w - 1), x1 = std::clamp((int)std::ceil(dx1 / sx), x0 + 1, src.w);
        int y0 = std::clamp((int)std::floor(dy0 / sy), 0, src.h - 1), y1 = std::clamp((int)std::ceil(dy1 / sy), y0 + 1, src.h);
        dx0 = (int)std::lround(x0 * sx); dx1 = (int)std::lround(x1 * sx);    // destination = exactly the source cut
        dy0 = (int)std::lround(y0 * sy); dy1 = (int)std::lround(y1 * sy);
        BITMAPINFO bi{}; bi.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
        bi.bmiHeader.biWidth = src.w; bi.bmiHeader.biHeight = -(y1 - y0); bi.bmiHeader.biPlanes = 1; bi.bmiHeader.biBitCount = 32;
        int oldMode = SetStretchBltMode(sdc, zoom < 1.0 ? HALFTONE : COLORONCOLOR);
        SetBrushOrgEx(sdc, 0, 0, nullptr);
        StretchDIBits(sdc, dx0, dy0, dx1 - dx0, dy1 - dy0, x0, 0, x1 - x0, y1 - y0, src.px.data() + (size_t)y0 * src.w,
                      &bi, DIB_RGB_COLORS, SRCCOPY);
        SetStretchBltMode(sdc, oldMode);
    }
    scaledPartial_ = false;
    BitBlt(mem, ir.left, ir.top, iw, ih, sdc, 0, 0, SRCCOPY);
    SelectObject(sdc, os);
    DeleteDC(sdc);
}

// the checkerboard in screen coordinates is approximated by light gray under transparent pixels
static void BlendChecker(Image& out, const Image& frame, double zoom, RECT r) {
    for (LONG y = r.top; y < r.bottom; y++)
        for (LONG x = r.left; x < r.right; x++) {
            uint32_t bg = (((int)(x * zoom) / 8 + (int)(y * zoom) / 8) % 2) ? 0xFFFFFFFF : 0xFFDADADA;
            out.at(x, y) = BlendOver(bg, frame.at(x, y));
        }
}

void Canvas::UpdateDisplay(const Image& frame, const RECT* only) {
    if (!only) { display_ = Image(frame.w, frame.h); displayZoom_ = zoom; displayValid_ = true; }
    BlendChecker(display_, frame, zoom, only ? *only : RECT{ 0, 0, frame.w, frame.h });
}

// ---------------------------------------------------------------- coordinates
int Canvas::RulerSize() const { return rulers && doc && !doc->isVideo ? ui::Scale(20, DpiForWindow(hwnd)) : 0; }

// area where the image is shown: the client area without the rulers (top and left)
RECT Canvas::View() const {
    RECT cr; GetClientRect(hwnd, &cr);
    int r = RulerSize();
    return { r, r, std::max((LONG)r, cr.right), std::max((LONG)r, cr.bottom) };
}

POINT Canvas::Origin() const {
    RECT v = View();
    int vw = v.right - v.left, vh = v.bottom - v.top;
    int iw = doc ? (int)std::lround(doc->w() * zoom) : 0, ih = doc ? (int)std::lround(doc->h() * zoom) : 0;
    POINT o;
    o.x = v.left + (iw + 2 * kMargin <= vw ? (vw - iw) / 2 : kMargin - scroll_.x);
    o.y = v.top + (ih + 2 * kMargin <= vh ? (vh - ih) / 2 : kMargin - scroll_.y);
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
    RECT v = View();
    int vw = v.right - v.left, vh = v.bottom - v.top;
    int iw = doc ? (int)std::lround(doc->w() * zoom) : 0, ih = doc ? (int)std::lround(doc->h() * zoom) : 0;
    int cw = iw + 2 * kMargin, ch = ih + 2 * kMargin;
    scroll_.x = std::clamp((int)scroll_.x, 0, std::max(0, cw - vw));
    scroll_.y = std::clamp((int)scroll_.y, 0, std::max(0, ch - vh));
    SCROLLINFO si{ sizeof(si), SIF_ALL | SIF_DISABLENOSCROLL };
    si.nMin = 0; si.nMax = cw - 1; si.nPage = vw; si.nPos = scroll_.x;
    SetScrollInfo(hwnd, SB_HORZ, &si, TRUE);
    si.nMax = ch - 1; si.nPage = vh; si.nPos = scroll_.y;
    SetScrollInfo(hwnd, SB_VERT, &si, TRUE);
}

void Canvas::ZoomTo(double z, POINT* anchor) {
    if (!doc) return;
    z = std::clamp(z, 0.05, 16.0);
    RECT v = View();
    POINT a = anchor ? *anchor : POINT{ (v.left + v.right) / 2, (v.top + v.bottom) / 2 };
    PointF d = ToDoc(a);
    zoom = z;
    fit_ = false;
    // keeps the document point under the anchor (the mouse, for the wheel): on each axis where the image is larger
    // than the view, the scroll position is chosen so that point stays under the cursor
    scroll_.x = (LONG)std::lround(kMargin + d.X * zoom - (a.x - v.left));
    scroll_.y = (LONG)std::lround(kMargin + d.Y * zoom - (a.y - v.top));
    UpdateScrollBars();
    InvalidateRect(hwnd, nullptr, FALSE);
    if (edit_) { CommitText(); }
    if (onZoomChanged) onZoomChanged();
}

void Canvas::FitToWindow() {
    if (!hwnd) return;
    RECT v = View();
    RECT cr{ 0, 0, v.right - v.left, v.bottom - v.top };
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

// ---------------------------------------------------------------- handles
std::vector<POINT> Canvas::HandlePoints(const DrawObj& o) const {
    std::vector<POINT> h;
    if (o.IsLine()) {
        h.push_back(ToScreen(o.p1)); h.push_back(ToScreen(o.p2));
        if (o.style.bezier)        // curve handle: the midpoint of the curve
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

// Group (CPaintOpGroup): the selected objects become a single one, at the position of the frontmost
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

// Ungroup: each selected group turns back into its objects, which stay selected
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
    if (doc->st.objs[selected]->locked) return -1;              // locked: cannot be resized
    auto hs = HandlePoints(*doc->st.objs[selected]);
    for (int i = (int)hs.size() - 1; i >= 0; i--)
        if (std::abs(p.x - hs[i].x) <= kHandle + 2 && std::abs(p.y - hs[i].y) <= kHandle + 2) return i;
    return -1;
}

int Canvas::HitObject(PointF p) const {
    if (!doc) return -1;
    float tol = (float)(4 / zoom);
    for (int i = (int)doc->st.objs.size() - 1; i >= 0; i--) {
        const DrawObj& o = *doc->st.objs[i];
        if (o.hidden || (o.type == ObjType::Link && tool != Tool::Select)) continue;     // hidden ones are not clickable
        if (o.HitTest(p, tol)) return i;
    }
    return -1;
}

static void SetSelectionFromMask(Canvas* c, cv::Mask m, bool& hasSel, cv::Mask& selMask, RECT& bounds, Image& overlay);

// ---------------------------------------------------------------- object management (layers panel, lock)
bool Canvas::SelectionLocked() const {
    for (int i : SelectedList()) if (doc->st.objs[i]->locked) return true;
    return false;
}

void Canvas::SetLocked(const std::vector<int>& idx, bool lock) {
    if (!doc || idx.empty()) return;
    CommitText();
    doc->Checkpoint();
    for (int i : idx) if (i >= 0 && i < (int)doc->st.objs.size()) doc->Mutable(i)->locked = lock;
    Refresh(); if (onDocChanged) onDocChanged(); if (onSelectionChanged) onSelectionChanged();
}

void Canvas::SetHidden(const std::vector<int>& idx, bool hidden) {
    if (!doc || idx.empty()) return;
    CommitText();
    doc->Checkpoint();
    for (int i : idx) if (i >= 0 && i < (int)doc->st.objs.size()) doc->Mutable(i)->hidden = hidden;
    if (hidden) {                                // a hidden object leaves the selection
        auto v = SelectedList();
        v.erase(std::remove_if(v.begin(), v.end(), [&](int i) { return doc->st.objs[i]->hidden; }), v.end());
        extra_.clear(); selected = -1;
        for (int i : v) { if (selected >= 0) extra_.push_back(selected); selected = i; }
    }
    Refresh(); if (onDocChanged) onDocChanged(); if (onSelectionChanged) onSelectionChanged();
}

void Canvas::RenameObject(int idx, const std::wstring& name) {
    if (!doc || idx < 0 || idx >= (int)doc->st.objs.size() || doc->st.objs[idx]->name == name) return;
    doc->Checkpoint();
    doc->Mutable(idx)->name = name;
    if (onDocChanged) onDocChanged();
}

// reorder (layers panel): the objects keep their order among themselves and go just below 'before'
void Canvas::MoveObjects(const std::vector<int>& idx, int before) {
    if (!doc || idx.empty()) return;
    auto& objs = doc->st.objs;
    std::vector<char> sel(objs.size(), 0);
    for (int i : idx) if (i >= 0 && i < (int)objs.size()) sel[i] = 1;
    ObjPtr primary = selected >= 0 && selected < (int)objs.size() ? objs[selected] : nullptr;
    std::vector<ObjPtr> moving, rest;
    int insertAt = 0;
    for (int i = 0; i < (int)objs.size(); i++) {
        if (sel[i]) moving.push_back(objs[i]);
        else { if (i < before) insertAt++; rest.push_back(objs[i]); }
    }
    std::vector<ObjPtr> out(rest.begin(), rest.begin() + insertAt);
    out.insert(out.end(), moving.begin(), moving.end());
    out.insert(out.end(), rest.begin() + insertAt, rest.end());
    if (out == objs) return;
    CommitText();
    doc->Checkpoint();
    objs = std::move(out);
    extra_.clear(); selected = -1;
    for (int i = 0; i < (int)objs.size(); i++) {
        if (std::find(moving.begin(), moving.end(), objs[i]) == moving.end()) continue;
        if (objs[i] == primary) { if (selected >= 0) extra_.push_back(selected); selected = i; }
        else if (selected >= 0 && objs[selected] == primary) extra_.push_back(i);
        else { if (selected >= 0) extra_.push_back(selected); selected = i; }
    }
    Refresh(); if (onDocChanged) onDocChanged(); if (onSelectionChanged) onSelectionChanged();
}

// Lift out: the raster selection becomes a floating image (the hole is filled as in Smart Move); the rest of the
// image can be cropped or cut out and the piece is put back with "Drop piece", which merges it
bool Canvas::LiftSelection() {
    if (!doc || doc->isVideo || !hasSel_) return false;
    CommitText();
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
    o->floating = true;
    o->p1 = PointF((float)selBounds_.left, (float)selBounds_.top);
    o->p2 = PointF((float)selBounds_.right, (float)selBounds_.bottom);
    doc->st.objs.push_back(o);
    ClearRasterSelection();
    selected = (int)doc->st.objs.size() - 1; extra_.clear();
    Refresh(); if (onDocChanged) onDocChanged(); if (onSelectionChanged) onSelectionChanged();
    return true;
}

bool Canvas::HasFloating() const {
    if (!doc) return false;
    for (auto& o : doc->st.objs) if (o->floating) return true;
    return false;
}

bool Canvas::DropFloating() {
    if (!doc || !HasFloating()) return false;
    CommitText();
    std::vector<int> v;
    for (int i = 0; i < (int)doc->st.objs.size(); i++) if (doc->st.objs[i]->floating) v.push_back(i);
    doc->Checkpoint();
    Image base = *doc->st.base;
    {
        auto bmp = base.WrapGdip();
        Gdiplus::Graphics g(bmp.get());
        g.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
        g.SetPixelOffsetMode(Gdiplus::PixelOffsetModeHalf);
        for (int i : v) if (!doc->st.objs[i]->hidden) RenderObject(g, base, *doc->st.objs[i]);
        g.Flush(Gdiplus::FlushIntentionSync);
    }
    doc->SetBase(std::move(base));
    for (auto it = v.rbegin(); it != v.rend(); ++it) doc->st.objs.erase(doc->st.objs.begin() + *it);
    selected = -1; extra_.clear();
    Refresh(); if (onDocChanged) onDocChanged(); if (onSelectionChanged) onSelectionChanged();
    return true;
}

// ---------------------------------------------------------------- exact geometry (X, Y, width, height)
int Canvas::GeometryKind() const {
    if (!doc || doc->isVideo) return 0;
    if (tool == Tool::Crop) return 3;
    if ((hasSel_ || drag_ == Drag::Select) && (tool == Tool::Selection || tool == Tool::Select)) return 2;
    if (selected >= 0 && selected < (int)doc->st.objs.size() && SelectionCount() == 1) return 1;
    return 0;
}

static RectF GeomRect(const DrawObj& o) {
    if (o.IsPath() || o.IsPointShape()) {
        float x0 = o.pts[0].X, y0 = o.pts[0].Y, x1 = x0, y1 = y0;
        for (auto& p : o.pts) { x0 = std::min(x0, p.X); y0 = std::min(y0, p.Y); x1 = std::max(x1, p.X); y1 = std::max(y1, p.Y); }
        return RectF(x0, y0, x1 - x0, y1 - y0);
    }
    if (o.type == ObjType::Step) { float d = o.style.fontSize * 1.5f; return RectF(o.p1.X - d / 2, o.p1.Y - d / 2, d, d); }
    return o.Rect();
}

RectF Canvas::Geometry() const {
    switch (GeometryKind()) {
    case 1: return GeomRect(*doc->st.objs[selected]);
    case 2:
        if (drag_ == Drag::Select) return selRect_;
        return RectF((float)selBounds_.left, (float)selBounds_.top, (float)(selBounds_.right - selBounds_.left), (float)(selBounds_.bottom - selBounds_.top));
    case 3: return hasCrop_ ? crop_ : RectF(0, 0, (float)doc->w(), (float)doc->h());
    default: return RectF();
    }
}

void Canvas::SetGeometry(RectF r, bool newStep) {
    int kind = GeometryKind();
    if (!kind) return;
    r.Width = std::max(1.0f, r.Width); r.Height = std::max(1.0f, r.Height);
    if (kind == 1) {
        if (doc->st.objs[selected]->locked) return;
        RectF from = GeomRect(*doc->st.objs[selected]);
        if (newStep) doc->Checkpoint();
        DrawObj* o = doc->Mutable(selected);
        if (o->type == ObjType::Step) o->Move(r.X - from.X, r.Y - from.Y);             // steps: position only
        else if (o->type == ObjType::Callout || o->type == ObjType::Magnify) {        // the tip stays where it points
            PointF tail = o->tail;
            o->p1 = PointF(r.X, r.Y); o->p2 = PointF(r.GetRight(), r.GetBottom()); o->tail = tail;
        } else if (o->IsLine()) {
            // keeps the direction of the line inside the new box
            auto map = [&](PointF q) {
                float u = from.Width > 0.01f ? (q.X - from.X) / from.Width : 0, v = from.Height > 0.01f ? (q.Y - from.Y) / from.Height : 0;
                return PointF(r.X + u * r.Width, r.Y + v * r.Height);
            };
            o->p1 = map(o->p1); o->p2 = map(o->p2); o->ctrl = map(o->ctrl);
        } else { o->Scale(from, r); o->UpdateGroupRect(); }
        dirty_ = true;
    } else if (kind == 2) {
        RECT rr{ (LONG)std::lround(r.X), (LONG)std::lround(r.Y), (LONG)std::lround(r.GetRight()), (LONG)std::lround(r.GetBottom()) };
        rr.left = std::clamp(rr.left, 0L, (LONG)doc->w() - 1); rr.top = std::clamp(rr.top, 0L, (LONG)doc->h() - 1);
        rr.right = std::clamp(rr.right, rr.left + 1, (LONG)doc->w()); rr.bottom = std::clamp(rr.bottom, rr.top + 1, (LONG)doc->h());
        cv::Mask m = selMode == 1 ? cv::EllipseMask(doc->w(), doc->h(), rr) : cv::RectMask(doc->w(), doc->h(), rr);
        SetSelectionFromMask(this, std::move(m), hasSel_, selMask_, selBounds_, selOverlay_);
    } else {
        float x0 = std::clamp(std::round(r.X), 0.0f, (float)doc->w() - 1), y0 = std::clamp(std::round(r.Y), 0.0f, (float)doc->h() - 1);
        float x1 = std::clamp(std::round(r.GetRight()), x0 + 1, (float)doc->w()), y1 = std::clamp(std::round(r.GetBottom()), y0 + 1, (float)doc->h());
        crop_ = RectF(x0, y0, x1 - x0, y1 - y0);
        hasCrop_ = true;
    }
    Refresh(kind == 1);
    if (kind == 1 && onDocChanged) onDocChanged();
}

// ---------------------------------------------------------------- paste
std::vector<DrawObj> Canvas::SelectedObjects() const {
    std::vector<DrawObj> v;
    for (int i : SelectedList()) v.push_back(*doc->st.objs[i]);
    return v;
}

// objects copied in the Editor: in place keeps the coordinates (Paste in place, also into another image); the
// normal paste, in the same image, shifts them a little so the copy is visible
void Canvas::PasteObjects(const std::vector<DrawObj>& objs, bool inPlace, bool sameDoc) {
    if (!doc || doc->isVideo || objs.empty()) return;
    CommitText();
    doc->Checkpoint();
    float off = !inPlace && sameDoc ? 16.0f : 0.0f;
    std::vector<int> idx;
    for (auto& src : objs) {
        auto o = std::make_shared<DrawObj>(src);
        o->locked = false; o->hidden = false;
        if (off) o->Move(off, off);
        doc->st.objs.push_back(o);
        idx.push_back((int)doc->st.objs.size() - 1);
    }
    if (tool != Tool::Select) SetTool(Tool::Select);
    dirty_ = true;
    SelectObjects(idx);
    Refresh(); if (onDocChanged) onDocChanged();
}

void Canvas::PasteImageAt(const Image& img, PointF at) {
    if (!doc || img.empty()) return;
    PasteImage(img);
    if (selected < 0) return;
    DrawObj* o = doc->st.objs[selected].get();          // just created by PasteImage (not shared with the history)
    o->Move(at.X - std::min(o->p1.X, o->p2.X), at.Y - std::min(o->p1.Y, o->p2.Y));
    Refresh(); if (onDocChanged) onDocChanged();
}

// clipboard text: goes into the text being edited or becomes a Text object in the center of the view. 'format'
// (the source formatting, when it is kept) replaces the font, size, color and style of the text
void Canvas::PasteText(const std::wstring& text, const Style* format) {
    if (!doc || doc->isVideo || text.empty()) return;
    std::wstring t;
    for (wchar_t c : text) if (c != L'\r') t += c;
    if (edit_) {
        std::wstring crlf;
        for (wchar_t c : t) { if (c == L'\n') crlf += L"\r\n"; else crlf += c; }
        SendMessageW(edit_, EM_REPLACESEL, TRUE, (LPARAM)crlf.c_str());
        if (format && editIdx_ >= 0 && editIdx_ < (int)doc->st.objs.size()) {
            DrawObj* w = doc->Mutable(editIdx_);
            w->style.font = format->font; w->style.fontSize = format->fontSize; w->style.textColor = format->textColor;
            w->style.bold = format->bold; w->style.italic = format->italic; w->style.underline = format->underline;
            int idx = editIdx_;
            std::wstring cur(GetWindowTextLengthW(edit_) + 1, L'\0');
            GetWindowTextW(edit_, cur.data(), (int)cur.size());
            CommitText();
            BeginTextEdit(idx, false);
            SetWindowTextW(edit_, cur.c_str());
            SendMessageW(edit_, EM_SETSEL, (WPARAM)-1, -1);
        }
        return;
    }
    CommitText();
    Style st = styles[Tool::Text];
    if (format) {
        st.font = format->font; st.fontSize = format->fontSize; st.textColor = format->textColor;
        st.bold = format->bold; st.italic = format->italic; st.underline = format->underline;
    }
    auto o = std::make_shared<DrawObj>();
    o->type = ObjType::Text; o->style = st; o->text = t;
    RECT v = View();
    PointF c = ToDoc({ (v.left + v.right) / 2, (v.top + v.bottom) / 2 });
    c.X = std::clamp(c.X, 0.0f, (float)doc->w()); c.Y = std::clamp(c.Y, 0.0f, (float)doc->h());
    float w = std::min(std::max(200.0f, (float)doc->w() * 0.5f), std::max(60.0f, (float)doc->w() - 20));
    o->p1 = PointF(c.X - w / 2, c.Y); o->p2 = PointF(c.X + w / 2, c.Y + 10);
    float hgt = TextBlockHeight(*o, w - 2 * st.padding) + 2 * st.padding;
    o->p1.Y = c.Y - hgt / 2; o->p2.Y = c.Y + hgt / 2;
    doc->Checkpoint();
    doc->st.objs.push_back(o);
    if (tool != Tool::Select) SetTool(Tool::Select);
    selected = (int)doc->st.objs.size() - 1; extra_.clear();
    Refresh(); if (onDocChanged) onDocChanged(); if (onSelectionChanged) onSelectionChanged();
}

// ---------------------------------------------------------------- painting
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
        // effect preview: the resulting image, centered, at the current zoom (reduced if it does not fit); no handles
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
        // displayed frame = composition (+ object being created)
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
        // "paper" shadow
        RECT sh = ir; OffsetRect(&sh, 3, 3);
        HBRUSH shb = CreateSolidBrush(RGB(180, 182, 186)); FillRect(mem, &sh, shb); DeleteObject(shb);
        if (frame == &composed_) DrawComposed(mem, ir);          // cached at this zoom
        else if (frame->HasTransparency()) {                      // object being created: a one-off frame
            Image disp(frame->w, frame->h);
            BlendChecker(disp, *frame, zoom, { 0, 0, frame->w, frame->h });
            disp.DrawTo(mem, ir.left, ir.top, iw, ih, zoom < 1.0);
        } else frame->DrawTo(mem, ir.left, ir.top, iw, ih, zoom < 1.0);

        Gdiplus::Graphics g(mem);
        g.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
        // lifted pieces: dashed frame while they float
        for (auto& op : doc->st.objs) {
            if (!op->floating || op->hidden) continue;
            RECT fr = ToScreenRect(op->Rect());
            Gdiplus::Pen w1(Gdiplus::Color(255, 255, 255, 255), 1.5f), d1(Gdiplus::Color(255, 230, 120, 0), 1.5f);
            d1.SetDashStyle(Gdiplus::DashStyleDash);
            g.DrawRectangle(&w1, GR(fr)); g.DrawRectangle(&d1, GR(fr));
        }
        // link areas captured with the image: outline and address under the cursor (and when selected)
        for (int i = 0; i < (int)doc->st.objs.size(); i++) {
            const DrawObj& lo = *doc->st.objs[i];
            if (lo.type != ObjType::Link || lo.hidden || (i != linkHot_ && !IsSelected(i))) continue;
            RECT lr = ToScreenRect(lo.Rect());
            Gdiplus::SolidBrush lf(Gdiplus::Color(40, 30, 120, 230));
            g.FillRectangle(&lf, GR(lr));
            Gdiplus::Pen lp(Gdiplus::Color(255, 30, 120, 230), 1.2f); lp.SetDashStyle(Gdiplus::DashStyleDash);
            g.DrawRectangle(&lp, GR(lr));
            if (i == linkHot_) {
                std::wstring tip = lo.text + L"   " + Tr(L"(duplo clique abre)");
                RECT tr{ lr.left, lr.bottom + 4, lr.left + ui::Scale(420, dpi), lr.bottom + 4 + ui::Scale(22, dpi) };
                HGDIOBJ of = SelectObject(mem, ui::UiFont(dpi, 9));
                RECT m{ 0, 0, 0, 0 }; DrawTextW(mem, tip.c_str(), -1, &m, DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX);
                SelectObject(mem, of);
                tr.right = std::min(tr.right, tr.left + m.right + ui::Scale(16, dpi));
                ui::FillRoundRect(mem, tr, 6, RGB(32, 34, 40), RGB(32, 34, 40));
                ui::DrawTextCentered(mem, tip, tr, RGB(255, 255, 255), ui::UiFont(dpi, 9));
            }
        }
        // polygon being clicked: marks the first corner (clicking it closes the polygon)
        if (drag_ == Drag::Polygon && preview_ && !preview_->pts.empty()) {
            std::vector<Gdiplus::Point> pl;
            for (auto& q : preview_->pts) { POINT sp = ToScreen(q); pl.push_back({ sp.x, sp.y }); }
            Gdiplus::Pen pp(Gdiplus::Color(255, 30, 120, 230), 1.5f); pp.SetDashStyle(Gdiplus::DashStyleDash);
            if (pl.size() > 1) g.DrawLines(&pp, pl.data(), (INT)pl.size());
            Gdiplus::SolidBrush wb2(Gdiplus::Color(255, 255, 255, 255));
            Gdiplus::Pen fp(Gdiplus::Color(255, 30, 120, 230), 2.0f);
            g.FillEllipse(&wb2, pl[0].X - 5, pl[0].Y - 5, 10, 10);
            g.DrawEllipse(&fp, pl[0].X - 5, pl[0].Y - 5, 10, 10);
        }
        // raster selection: blue highlight + dashed outline
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
        // text box: dashed while being dragged and while the text is typed
        if ((preview_ && preview_->type == ObjType::Text && drag_ == Drag::Create) ||
            (edit_ && editIdx_ >= 0 && editIdx_ < (int)doc->st.objs.size() && doc->st.objs[editIdx_]->type == ObjType::Text)) {
            const DrawObj& tb = preview_ && drag_ == Drag::Create ? *preview_ : *doc->st.objs[editIdx_];
            RECT s = ToScreenRect(tb.Rect());
            Gdiplus::Pen w1(Gdiplus::Color(255, 255, 255, 255), 1.0f), p(Gdiplus::Color(255, 30, 120, 230), 1.0f);
            p.SetDashStyle(Gdiplus::DashStyleDash);
            g.DrawRectangle(&w1, GR(s));
            g.DrawRectangle(&p, GR(s));
        }
        // object selection rectangle
        if (drag_ == Drag::Marquee) {
            RECT s = ToScreenRect(selRect_);
            Gdiplus::SolidBrush fillB(Gdiplus::Color(40, 30, 120, 230));
            g.FillRectangle(&fillB, GR(s));
            Gdiplus::Pen p(Gdiplus::Color(255, 30, 120, 230), 1.2f); p.SetDashStyle(Gdiplus::DashStyleDash);
            g.DrawRectangle(&p, GR(s));
        }
        // several objects selected: outline of each one and of the area of the set
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
        // selection in progress (rectangle/ellipse/lasso)
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
        // crop: darkens the outside
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
        // band of the cut-out
        if (drag_ == Drag::Cut) {
            RECT s = ToScreenRect(selRect_);
            Gdiplus::SolidBrush b(Gdiplus::Color(90, 230, 50, 50));
            if (cutDir == 0) g.FillRectangle(&b, ir.left, s.top, iw, s.bottom - s.top);
            else g.FillRectangle(&b, s.left, ir.top, s.right - s.left, ih);
        }
        // locked objects: lock badge on the corner, no handles
        if (!edit_) {
            for (int i : SelectedList()) {
                const DrawObj& lo = *doc->st.objs[i];
                if (!lo.locked) continue;
                RECT b = ToScreenRect(lo.Bounds());
                int sz = ui::Scale(18, dpi);
                RECT badge{ b.right - sz / 2, b.top - sz / 2, b.right + sz / 2, b.top + sz / 2 };
                ui::FillRoundRect(mem, badge, sz / 2, RGB(255, 255, 255), RGB(30, 120, 230));
                InflateRect(&badge, -ui::Scale(3, dpi), -ui::Scale(3, dpi));
                icons::Draw(g, icons::Lock, Gdiplus::RectF((float)badge.left, (float)badge.top, (float)(badge.right - badge.left), (float)(badge.bottom - badge.top)),
                            Gdiplus::Color(255, 30, 120, 230));
                if (SelectionCount() == 1) {
                    Gdiplus::Pen lp(Gdiplus::Color(200, 30, 120, 230), 1); lp.SetDashStyle(Gdiplus::DashStyleDot);
                    g.DrawRectangle(&lp, GR(b));
                }
            }
        }
        // handles of the selected object
        if (selected >= 0 && selected < (int)doc->st.objs.size() && !edit_ && SelectionCount() == 1 && !doc->st.objs[selected]->locked) {
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
    if (doc && !doc->isVideo && !fxPreview_) {
        Gdiplus::Graphics g(mem);
        PaintRulers(mem, g, cr);
    }
    BitBlt(hdc, 0, 0, cr.right, cr.bottom, mem, 0, 0, SRCCOPY);
    SelectObject(mem, ob);
    DeleteObject(bb); DeleteDC(mem);
}

// rulers (top and left, in image pixels, following zoom and scrolling), guides and the cursor position
void Canvas::PaintRulers(HDC dc, Gdiplus::Graphics& g, RECT cr) {
    if (!rulers) return;
    int rs = RulerSize(), dpi = DpiForWindow(hwnd);
    RECT v = View();
    // guides across the view
    {
        Gdiplus::GraphicsState gs = g.Save();
        g.SetClip(Gdiplus::Rect(v.left, v.top, v.right - v.left, v.bottom - v.top));
        Gdiplus::Pen gp(Gdiplus::Color(255, 0, 190, 255), 1.0f);
        for (float x : doc->guidesX) { int sx = ToScreen(PointF(x, 0)).x; g.DrawLine(&gp, sx, v.top, sx, v.bottom); }
        for (float y : doc->guidesY) { int sy = ToScreen(PointF(0, y)).y; g.DrawLine(&gp, v.left, sy, v.right, sy); }
        g.Restore(gs);
    }
    COLORREF bg = RGB(246, 247, 249), line = RGB(160, 164, 172), text = RGB(90, 94, 102);
    RECT top{ rs, 0, cr.right, rs }, left{ 0, rs, rs, cr.bottom }, corner{ 0, 0, rs, rs };
    HBRUSH b = CreateSolidBrush(bg);
    FillRect(dc, &top, b); FillRect(dc, &left, b); FillRect(dc, &corner, b);
    DeleteObject(b);
    // step between numbers: the first "nice" value at least ~60 screen pixels wide
    static const int steps[] = { 1, 2, 5, 10, 20, 25, 50, 100, 200, 250, 500, 1000, 2000, 5000, 10000 };
    int step = steps[std::size(steps) - 1];
    for (int st : steps) if (st * zoom >= ui::Scale(60, dpi)) { step = st; break; }
    int minor = step % 10 == 0 ? step / 10 : step % 5 == 0 ? step / 5 : step;
    if (minor * zoom < 5) minor = step;
    HPEN pen = CreatePen(PS_SOLID, 1, line);
    HGDIOBJ op = SelectObject(dc, pen), of = SelectObject(dc, ui::UiFont(dpi, 7));
    SetBkMode(dc, TRANSPARENT); SetTextColor(dc, text);
    PointF a = ToDoc({ rs, rs }), z = ToDoc({ cr.right, cr.bottom });
    for (int axis = 0; axis < 2; axis++) {
        float from = axis ? a.Y : a.X, to = axis ? z.Y : z.X;
        int first = (int)std::floor(from / minor) * minor;
        for (int u = first; u <= to; u += minor) {
            int sp = axis ? ToScreen(PointF(0, (float)u)).y : ToScreen(PointF((float)u, 0)).x;
            if (sp < rs) continue;
            bool major = u % step == 0;
            int len = major ? rs : rs / 3;
            if (axis) { MoveToEx(dc, rs - len, sp, nullptr); LineTo(dc, rs, sp); }
            else { MoveToEx(dc, sp, rs - len, nullptr); LineTo(dc, sp, rs); }
            if (!major) continue;
            std::wstring n = std::to_wstring(u);
            if (axis) {                                    // vertical ruler: digits stacked
                for (size_t k = 0; k < n.size(); k++) {
                    RECT tr{ 1, sp + 2 + (int)k * ui::Scale(8, dpi), rs - ui::Scale(5, dpi), sp + 2 + (int)(k + 1) * ui::Scale(8, dpi) };
                    DrawTextW(dc, n.substr(k, 1).c_str(), 1, &tr, DT_CENTER | DT_SINGLELINE | DT_NOPREFIX);
                }
            } else {
                RECT tr{ sp + 3, 1, sp + ui::Scale(60, dpi), rs / 2 + 2 };
                DrawTextW(dc, n.c_str(), -1, &tr, DT_LEFT | DT_SINGLELINE | DT_NOPREFIX);
            }
        }
    }
    MoveToEx(dc, rs - 1, 0, nullptr); LineTo(dc, rs - 1, cr.bottom);
    MoveToEx(dc, 0, rs - 1, nullptr); LineTo(dc, cr.right, rs - 1);
    // cursor position marks
    POINT cp; GetCursorPos(&cp); ScreenToClient(hwnd, &cp);
    if (PtInRect(&cr, cp)) {
        HPEN cpen = CreatePen(PS_SOLID, 1, RGB(230, 50, 50));
        SelectObject(dc, cpen);
        if (cp.x >= rs) { MoveToEx(dc, cp.x, 0, nullptr); LineTo(dc, cp.x, rs); }
        if (cp.y >= rs) { MoveToEx(dc, 0, cp.y, nullptr); LineTo(dc, rs, cp.y); }
        SelectObject(dc, pen);
        DeleteObject(cpen);
    }
    SelectObject(dc, of); SelectObject(dc, op);
    DeleteObject(pen);
}

// ---------------------------------------------------------------- raster selection
void Canvas::ClearRasterSelection() {
    if (!hasSel_) return;
    hasSel_ = false;
    selMask_.clear();
    selOverlay_ = Image();
    InvalidateRect(hwnd, nullptr, FALSE);
    if (onAreaChanged) onAreaChanged();
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
    if (b.right <= b.left) { hasSel = false; if (c->onAreaChanged) c->onAreaChanged(); return; }
    selMask = std::move(m);
    bounds = b;
    overlay = MakeOverlay(selMask, c->doc->w(), b);
    hasSel = true;
    if (c->onAreaChanged) c->onAreaChanged();
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
    size_t total = v.size();
    v.erase(std::remove_if(v.begin(), v.end(), [&](int i) { return doc->st.objs[i]->locked; }), v.end());   // locked: kept
    if (v.size() < total && onHint) onHint(Tr(L"Objetos bloqueados não são excluídos (desbloqueie pelo menu de contexto ou pelo painel Camadas)."));
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

// order: the selected objects move together, keeping their order among themselves
static void Reorder(Document* doc, const std::vector<int>& v, int mode, int& selected, std::vector<int>& extra) {
    auto& objs = doc->st.objs;
    ObjPtr primary = selected >= 0 ? objs[selected] : nullptr;
    std::vector<char> sel(objs.size(), 0);
    for (int i : v) sel[i] = true;
    if (mode == 0 || mode == 3) {              // 0 bring to front, 3 send to back
        std::vector<ObjPtr> a, b;
        for (size_t i = 0; i < objs.size(); i++) (sel[i] ? a : b).push_back(objs[i]);
        objs = mode == 0 ? b : a;
        objs.insert(objs.end(), (mode == 0 ? a : b).begin(), (mode == 0 ? a : b).end());
        // the marks follow the objects to their new positions (the selected block is now last or first)
        std::fill(sel.begin(), sel.end(), 0);
        size_t first = mode == 0 ? b.size() : 0;
        std::fill(sel.begin() + first, sel.begin() + first + a.size(), 1);
    } else if (mode == 1) {                    // one step forward
        for (int i = (int)objs.size() - 2; i >= 0; i--)
            if (sel[i] && !sel[i + 1]) { std::swap(objs[i], objs[i + 1]); std::swap(sel[i], sel[i + 1]); }
    } else {                                   // one step backward
        for (int i = 1; i < (int)objs.size(); i++)
            if (sel[i] && !sel[i - 1]) { std::swap(objs[i], objs[i - 1]); std::swap(sel[i], sel[i - 1]); }
    }
    extra.clear();
    selected = -1;
    for (size_t i = 0; i < objs.size(); i++) {         // selection rebuilt at the new positions
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

// align: one object, relative to the image; several, among themselves (by the area they cover together)
void Canvas::AlignSelected(int mode) {
    auto v = SelectedList();
    if (doc) v.erase(std::remove_if(v.begin(), v.end(), [&](int i) { return doc->st.objs[i]->locked; }), v.end());
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

// distribute (as in PowerPoint): the first and last objects stay; the ones between are spread so the gaps are equal
void Canvas::DistributeSelected(bool horizontal) {
    auto v = SelectedList();
    if (doc) v.erase(std::remove_if(v.begin(), v.end(), [&](int i) { return doc->st.objs[i]->locked; }), v.end());
    if (!doc || v.size() < 3) { if (onHint) onHint(Tr(L"Selecione 3 ou mais objetos para distribuir.")); return; }
    std::vector<std::pair<int, RectF>> items;
    for (int i : v) items.push_back({ i, ObjExtent(*doc->st.objs[i]) });
    auto start = [&](const RectF& r) { return horizontal ? r.X : r.Y; };
    auto size = [&](const RectF& r) { return horizontal ? r.Width : r.Height; };
    std::stable_sort(items.begin(), items.end(), [&](auto& a, auto& b) { return start(a.second) + size(a.second) / 2 < start(b.second) + size(b.second) / 2; });
    float first = start(items.front().second), last = start(items.back().second) + size(items.back().second), sum = 0;
    for (auto& it : items) sum += size(it.second);
    float gap = (last - first - sum) / (items.size() - 1), pos = first + size(items.front().second) + gap;
    std::vector<std::pair<int, float>> moves;
    for (size_t k = 1; k + 1 < items.size(); k++) {
        float d = pos - start(items[k].second);
        if (std::fabs(d) > 0.01f) moves.push_back({ items[k].first, d });
        pos += size(items[k].second) + gap;
    }
    if (moves.empty()) return;
    doc->Checkpoint();
    for (auto& [i, d] : moves) doc->Mutable(i)->Move(horizontal ? d : 0, horizontal ? 0 : d);
    Refresh(); if (onDocChanged) onDocChanged();
}

bool Canvas::FlipSelected(bool horizontal) {
    auto v = SelectedList();
    if (doc) v.erase(std::remove_if(v.begin(), v.end(), [&](int i) { return doc->st.objs[i]->locked; }), v.end());
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

// an area (highlighter, spotlight, hide) that changes shape: a polygon or freehand outline starts from the current box
// (its corners, or an ellipse inside it); going back to a shape drops the points. Old freehand highlighters (points
// with a shape style) keep theirs.
static void AdaptAreaPoints(DrawObj& o, int oldArea) {
    if (!o.IsAreaTool()) return;
    RectF r = o.Rect();
    if (IsPointArea(o.style.areaShape) && o.pts.size() < 3) {
        o.pts.clear();
        if (o.style.areaShape == AreaPolygon)
            o.pts = { PointF(r.X, r.Y), PointF(r.GetRight(), r.Y), PointF(r.GetRight(), r.GetBottom()), PointF(r.X, r.GetBottom()) };
        else
            for (int k = 0; k < 24; k++) {
                double a = k * 2 * 3.14159265 / 24;
                o.pts.push_back(PointF(r.X + r.Width / 2 + (float)std::cos(a) * r.Width / 2, r.Y + r.Height / 2 + (float)std::sin(a) * r.Height / 2));
            }
    } else if (!IsPointArea(o.style.areaShape) && IsPointArea(oldArea)) o.pts.clear();
}

void Canvas::ApplyStyleToSelected(const Style& s, bool newStep) {
    if (!doc || selected < 0 || selected >= (int)doc->st.objs.size()) return;
    if (newStep) doc->Checkpoint();
    int fam = Family(*doc->st.objs[selected]);
    for (int i : SelectedList()) {                // several selected: the style applies to those of the same kind as the primary
        if (i == selected || Family(*doc->st.objs[i]) != fam || doc->st.objs[i]->type == ObjType::Group) continue;
        DrawObj* x = doc->Mutable(i);
        int oldArea = x->style.areaShape;
        x->style = s;
        AdaptAreaPoints(*x, oldArea);
        if (x->IsShape()) x->type = ObjType::Shape;
        if (x->type == ObjType::Blur || x->type == ObjType::Pixelate) x->type = s.hideKind == 1 ? ObjType::Pixelate : ObjType::Blur;
    }
    if (doc->st.objs[selected]->type == ObjType::Group) { Refresh(); if (onDocChanged) onDocChanged(); return; }
    DrawObj* o = doc->Mutable(selected);
    int oldArea = o->style.areaShape;
    o->style = s;
    AdaptAreaPoints(*o, oldArea);
    if (o->IsShape()) o->type = ObjType::Shape;                  // old shapes become Shape with the style's kind
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
    if (doc) v.erase(std::remove_if(v.begin(), v.end(), [&](int i) { return doc->st.objs[i]->locked; }), v.end());
    if (!doc || v.empty()) return;
    doc->Checkpoint();
    for (int i : v) doc->Mutable(i)->Move((float)dx, (float)dy);
    Refresh(); if (onDocChanged) onDocChanged();
}

// ---------------------------------------------------------------- in-place text (IInPlaceRichEditOwner)
void Canvas::BeginTextEdit(int idx, bool isNew) {
    CommitText();
    if (!doc || idx < 0 || idx >= (int)doc->st.objs.size()) return;
    const DrawObj& o = *doc->st.objs[idx];
    if (!o.HasText() && !o.HasLabel()) return;
    if (!isNew) doc->Checkpoint();
    editIdx_ = idx; editNew_ = isNew;
    labelReady_ = false;
    RectF tr = o.Rect();
    if (o.type == ObjType::Callout) tr = CalloutTextRect(o);
    else if (o.HasLabel()) tr = ArrowLabelRect(o);
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
    DWORD align = o.HasLabel() || o.style.alignH == 1 ? ES_CENTER : o.style.alignH == 2 ? ES_RIGHT : ES_LEFT;
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

// The width is the box's (dragged or default); the height follows the typed text — the text box grows
// downward when the text goes past it. The edit box sits at the text's height, with the chosen vertical alignment,
// so what is seen while typing is the same as what ends up in the image.
void Canvas::LayoutTextEdit() {
    if (!edit_ || !doc || editIdx_ < 0 || editIdx_ >= (int)doc->st.objs.size()) return;
    ObjType type = doc->st.objs[editIdx_]->type;
    int alignV = doc->st.objs[editIdx_]->style.alignV;
    float pad = doc->st.objs[editIdx_]->style.padding;
    int n = GetWindowTextLengthW(edit_);
    std::wstring t(n + 1, L'\0');
    GetWindowTextW(edit_, t.data(), n + 1);
    t.resize(n);
    auto area = [&] {
        const DrawObj& o = *doc->st.objs[editIdx_];
        RectF a = o.Rect();
        if (type == ObjType::Callout) a = CalloutTextRect(o);
        else if (o.HasLabel()) {                // arrow label: the box follows the typed text
            DrawObj tmp = o;
            tmp.text.clear();
            for (wchar_t c : t) if (c != L'\r') tmp.text += c;
            a = ArrowLabelRect(tmp);
        } else a.Inflate(-pad, -pad);
        RECT r = ToScreenRect(a);
        if (o.HasLabel()) {                                   // the edit box has margins: a little wider than the text
            DWORD mg = (DWORD)SendMessageW(edit_, EM_GETMARGINS, 0, 0);
            int extra = (int)LOWORD(mg) + (int)HIWORD(mg) + (int)(o.style.fontSize * zoom * 0.8) + 8;
            r.left -= extra / 2; r.right += extra - extra / 2;
        }
        if (r.right - r.left < 60) { int c = (r.left + r.right) / 2; r.left = c - 30; r.right = c + 30; }
        return r;
    };
    RECT r = area();
    if (t.empty() || t.back() == L'\n') t += L"Ag";        // a new, still empty line takes up height too
    HDC dc = GetDC(edit_);
    HGDIOBJ of = SelectObject(dc, editFont_);
    DWORD mg = (DWORD)SendMessageW(edit_, EM_GETMARGINS, 0, 0);
    RECT m{ 0, 0, std::max(10, (int)(r.right - r.left) - (int)LOWORD(mg) - (int)HIWORD(mg) - 2), 0 };
    DrawTextW(dc, t.c_str(), -1, &m, DT_CALCRECT | DT_WORDBREAK | DT_EDITCONTROL | DT_NOPREFIX);
    SelectObject(dc, of);
    ReleaseDC(edit_, dc);
    int textH = m.bottom + 4, areaH = r.bottom - r.top;
    if (type == ObjType::Text && textH > areaH) {          // grows downward
        float grow = (textH - areaH) / (float)zoom;
        DrawObj* w = doc->Mutable(editIdx_);
        if (w->p2.Y >= w->p1.Y) w->p2.Y += grow; else w->p1.Y += grow;
        dirty_ = true;
        r = area();
        areaH = r.bottom - r.top;
    }
    int h = type == ObjType::Arrow ? std::max(textH, areaH) : std::min(textH, areaH);
    if (type == ObjType::Arrow) areaH = h;
    int top = r.top;
    if (h < areaH) top = alignV == 1 ? r.top + (areaH - h) / 2 : alignV == 2 ? r.bottom - h : r.top;
    MoveWindow(edit_, r.left, top, r.right - r.left, std::max(h, 8), TRUE);
    if (textH <= areaH) SendMessageW(edit_, EM_LINESCROLL, 0, -1000);   // everything fits: no internal scrolling
    InvalidateRect(hwnd, nullptr, FALSE);
}

void Canvas::CommitText() {
    if (!edit_) return;
    HWND e = edit_;
    edit_ = nullptr;           // avoids reentrancy through EN_KILLFOCUS
    int n = GetWindowTextLengthW(e);
    std::wstring t(n + 1, L'\0');
    GetWindowTextW(e, t.data(), n + 1);
    t.resize(n);
    std::wstring clean;
    for (wchar_t c : t) if (c != L'\r') clean += c;
    DestroyWindow(e);
    if (doc && editIdx_ >= 0 && editIdx_ < (int)doc->st.objs.size()) {
        if (clean.empty() && doc->st.objs[editIdx_]->type == ObjType::Text) {      // (an arrow only loses its label)
            doc->st.objs.erase(doc->st.objs.begin() + editIdx_);
            selected = -1;
            if (onSelectionChanged) onSelectionChanged();
        } else if (doc->st.objs[editIdx_]->text != clean) {
            DrawObj* w = doc->Mutable(editIdx_);
            w->text = clean;
            if (w->type == ObjType::Text) {                     // ensures the height for the whole text (no cut-off lines)
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
    for (int i : SelectedList()) if (!doc->st.objs[i]->locked) dragMulti_.push_back({ i, *doc->st.objs[i] });   // locked ones stay
    if (selected >= 0) dragOrig_ = *doc->st.objs[selected];
    drag_ = Drag::Move;
    axisLock_ = 0;
    InvalidateRect(hwnd, nullptr, FALSE);
}

void Canvas::StartSmartMove(PointF p) {
    // lifts the selection's pixels as an object and fills the hole (Smart Move, TSCV.Inpainting)
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

// eraser: removes alpha from the image at the chosen opacity; with smoothing, the brush edge is gradual
void Canvas::EraseAt(PointF p) {
    Image& base = const_cast<Image&>(*doc->st.base);
    const Style& s = CurrentStyle();
    float r = std::max(1.0f, s.width / 2);
    int op = std::clamp(s.opacity, 1, 100);
    // area touched, for the partial repaint (Recompose)
    eraseRect_.left = std::max(0L, std::min(eraseRect_.left, (LONG)(p.X - r - 1)));
    eraseRect_.top = std::max(0L, std::min(eraseRect_.top, (LONG)(p.Y - r - 1)));
    eraseRect_.right = std::min((LONG)base.w, std::max(eraseRect_.right, (LONG)(p.X + r + 2)));
    eraseRect_.bottom = std::min((LONG)base.h, std::max(eraseRect_.bottom, (LONG)(p.Y + r + 2)));
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

void Canvas::EraseLine(PointF from, PointF to) {
    // WM_MOUSEMOVE positions can be far apart in a quick stroke: dabs every half radius cover the segment between them
    float dx = to.X - from.X, dy = to.Y - from.Y, len = std::sqrt(dx * dx + dy * dy);
    float step = std::max(1.0f, std::max(1.0f, CurrentStyle().width / 2) / 2);
    int n = std::max(1, (int)std::ceil(len / step));
    for (int i = 1; i <= n; i++) EraseAt(PointF(from.X + dx * i / n, from.Y + dy * i / n));
}

// helpers for the drawing constraints (Shift)
static void FitToPoints(DrawObj& o) {
    if (o.pts.empty()) return;
    float x0 = o.pts[0].X, y0 = o.pts[0].Y, x1 = x0, y1 = y0;
    for (auto& q : o.pts) { x0 = std::min(x0, q.X); y0 = std::min(y0, q.Y); x1 = std::max(x1, q.X); y1 = std::max(y1, q.Y); }
    o.p1 = PointF(x0, y0); o.p2 = PointF(x1, y1);
}

// width/height of the "perfect" version of a shape (Shift while drawing): square, circle, equilateral triangle,
// regular hexagon; stamps keep the proportion of their image
static float PerfectAspect(const DrawObj& o) {
    int k = -1;
    if (o.type == ObjType::Shape) k = o.style.shapeKind;
    else if (o.IsAreaTool()) k = o.style.areaShape;
    else if (o.type == ObjType::Magnify) k = o.style.magnifyShape == MagHexagon ? ShapeHexagon : -1;
    else if (o.type == ObjType::Stamp) return StampAspect(o.text);
    return k == ShapeTriangle || k == ShapeHexagon ? 1.1547f : 1.0f;
}

PointF Canvas::Constrain45(PointF from, PointF to) const {
    double dx = to.X - from.X, dy = to.Y - from.Y;
    double ang = std::round(std::atan2(dy, dx) / (3.14159265 / 4)) * (3.14159265 / 4), len = std::hypot(dx, dy);
    return PointF(from.X + (float)(std::cos(ang) * len), from.Y + (float)(std::sin(ang) * len));
}

// polygon (Highlighter, Spotlight and Hide): each click adds a corner; a click on the first corner, a double click
// or Enter closes it; Shift snaps the segment to 45°. The last point follows the mouse.
void Canvas::PolygonClick(POINT p, bool dbl) {
    DrawObj& o = *preview_;
    if (dbl) { FinishPolygon(); return; }
    POINT first = ToScreen(o.pts.front());
    if (o.pts.size() >= 4 && std::abs(p.x - first.x) <= 8 && std::abs(p.y - first.y) <= 8) {
        FinishPolygon();                                      // (it drops the point that followed the mouse)
        return;
    }
    PointF d = ToDoc(p);
    if (GetKeyState(VK_SHIFT) < 0 && o.pts.size() >= 2) d = Constrain45(o.pts[o.pts.size() - 2], d);
    o.pts.back() = d;
    o.pts.push_back(d);
    SetCapture(hwnd);
    InvalidateRect(hwnd, nullptr, FALSE);
}

void Canvas::FinishPolygon() {
    if (drag_ != Drag::Polygon || !preview_) return;
    ReleaseCapture();
    drag_ = Drag::None;
    DrawObj& o = *preview_;
    if (o.pts.size() >= 2) o.pts.pop_back();                 // the point that followed the mouse
    std::vector<PointF> clean;                               // repeated clicks (double click) count once
    for (auto& q : o.pts) if (clean.empty() || std::hypot(q.X - clean.back().X, q.Y - clean.back().Y) * zoom > 2) clean.push_back(q);
    o.pts = std::move(clean);
    if (o.pts.size() < 3) { preview_.reset(); InvalidateRect(hwnd, nullptr, FALSE); return; }
    FitToPoints(o);
    FinishCreate();
    Refresh();
    if (onDocChanged) onDocChanged();
}

bool Canvas::WantsTyping() const {
    return labelReady_ && !edit_ && doc && selected >= 0 && selected < (int)doc->st.objs.size() &&
           doc->st.objs[selected]->HasLabel() && doc->st.objs[selected]->text.empty();
}

void Canvas::ResetPolygon() {
    if (drag_ != Drag::Polygon) return;
    ReleaseCapture();
    drag_ = Drag::None;
    preview_.reset();
    InvalidateRect(hwnd, nullptr, FALSE);
}

bool Canvas::HitGuide(POINT p, int& axis, int& idx) const {
    if (!rulers || !doc) return false;
    for (int a = 0; a < 2; a++) {
        const auto& list = a ? doc->guidesY : doc->guidesX;
        for (int i = (int)list.size() - 1; i >= 0; i--) {
            POINT s = ToScreen(a ? PointF(0, list[i]) : PointF(list[i], 0));
            if (std::abs((a ? p.y - s.y : p.x - s.x)) <= 3) { axis = a; idx = i; return true; }
        }
    }
    return false;
}

// edge snapping by contrast: looks, within a few screen pixels of 'pos', for the column (vertical edge) or row
// whose pixels differ most from the previous one along the edge (a0..a1) — window borders, buttons, table lines
float Canvas::SnapEdge(bool vertical, float pos, float a0, float a1) {
    Recompose();
    const Image& img = composed_;
    if (img.empty()) return pos;
    int lim = vertical ? img.w : img.h, other = vertical ? img.h : img.w;
    int win = std::max(2, (int)std::lround(6 / zoom));
    int lo = (int)std::floor(std::min(a0, a1)), hi = (int)std::ceil(std::max(a0, a1));
    if (hi - lo < 6) { lo -= 12; hi += 12; }
    lo = std::clamp(lo, 0, other - 1); hi = std::clamp(hi, lo + 1, other);
    int stepA = std::max(1, (hi - lo) / 240);
    int c0 = (int)std::lround(pos);
    float best = pos, bestScore = 0;
    for (int c = std::max(1, c0 - win); c <= std::min(lim - 1, c0 + win); c++) {
        long sum = 0; int n = 0;
        for (int a = lo; a < hi; a += stepA) {
            uint32_t p0 = vertical ? img.at(c - 1, a) : img.at(a, c - 1), p1 = vertical ? img.at(c, a) : img.at(a, c);
            sum += std::max({ std::abs(CR(p0) - CR(p1)), std::abs(CG(p0) - CG(p1)), std::abs(CB(p0) - CB(p1)), std::abs(CA(p0) - CA(p1)) });
            n++;
        }
        float score = n ? (float)sum / n - std::abs(c - c0) * 0.8f : 0;
        if (score > bestScore) { bestScore = score; best = (float)c; }
    }
    return bestScore >= 22 ? best : pos;                    // only a clear edge pulls the selection
}

void Canvas::OnDown(POINT p, bool dbl) {
    if (!doc || doc->isVideo || fxPreview_) return;         // during an effect preview the image cannot be edited
    if (edit_) { CommitText(); return; }      // clicking outside only commits the text (does not create another object)
    SetFocus(hwnd);
    if (drag_ == Drag::Polygon && preview_) { PolygonClick(p, dbl); return; }
    labelReady_ = false;
    PointF d = ToDoc(p);
    if (pickColorOnce) {                      // Fill panel eyedropper
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
    pendingDeselect_ = -1;
    int rs = RulerSize();
    if (rs && (p.x < rs || p.y < rs)) {                     // dragging out of a ruler creates a guide
        if (p.x < rs && p.y < rs) return;
        guideAxis_ = p.y < rs ? 1 : 0;
        auto& list = guideAxis_ ? doc->guidesY : doc->guidesX;
        list.push_back(guideAxis_ ? d.Y : d.X);
        guideIdx_ = (int)list.size() - 1;
        drag_ = Drag::Guide;
        SetCapture(hwnd);
        InvalidateRect(hwnd, nullptr, FALSE);
        return;
    }
    SetCapture(hwnd);

    // the selected object's handle takes priority in any drawing tool (and the lifted piece's in the Selection tool)
    int hh = HitHandle(p);
    bool floatingSel = selected >= 0 && selected < (int)doc->st.objs.size() && doc->st.objs[selected]->floating;
    if (hh >= 0 && tool != Tool::Crop && (tool != Tool::Selection || floatingSel) && tool != Tool::Fill && tool != Tool::Eyedropper) {
        doc->Checkpoint();
        dragOrig_ = *doc->st.objs[selected];
        handle_ = hh; drag_ = Drag::Resize;
        return;
    }
    if (tool == Tool::Selection) {                          // the lifted piece moves with the Selection tool
        int hit = HitObject(d);
        if (hit >= 0 && doc->st.objs[hit]->floating) {
            if (hit != selected) { selected = hit; extra_.clear(); if (onSelectionChanged) onSelectionChanged(); }
            BeginMove();
            return;
        }
    }
    int ga, gi;
    if (tool == Tool::Select && HitGuide(p, ga, gi)) {        // guides are moved with the Move tool
        guideAxis_ = ga; guideIdx_ = gi; drag_ = Drag::Guide;
        return;
    }
    bool drawTool = tool != Tool::Select && tool != Tool::Crop && tool != Tool::Selection && tool != Tool::Fill &&
                    tool != Tool::Eraser && tool != Tool::Eyedropper && tool != Tool::Cutout;
    if (tool == Tool::Select || drawTool) {
        int hit = HitObject(d);
        // with a drawing tool, clicking an existing object selects/moves it (like the original)
        if (hit >= 0 && (tool == Tool::Select || hit == selected || !dbl)) {
            // a drawing tool only picks objects of its own kind (arrow picks arrow); over the others it draws —
            // so a text can be highlighted or pointed at. Ctrl forces drawing over the same kind too.
            ObjType ht = doc->st.objs[hit]->type;
            bool sameKind = (tool == Tool::Arrow && ht == ObjType::Arrow) || (tool == Tool::Line && ht == ObjType::Line) ||
                            (tool == Tool::Shape && doc->st.objs[hit]->IsShape()) || (tool == Tool::Pen && ht == ObjType::Pen) ||
                            (tool == Tool::Highlighter && ht == ObjType::Highlighter) || (tool == Tool::Text && ht == ObjType::Text) ||
                            (tool == Tool::Callout && ht == ObjType::Callout) || (tool == Tool::Magnify && ht == ObjType::Magnify) ||
                            (tool == Tool::Blur && (ht == ObjType::Blur || ht == ObjType::Pixelate)) || (tool == Tool::Spotlight && ht == ObjType::Spotlight);
            if (tool == Tool::Select || hit == selected || (GetKeyState(VK_CONTROL) >= 0 && sameKind)) {
                if (dbl && doc->st.objs[hit]->HasText()) { selected = hit; extra_.clear(); BeginTextEdit(hit, false); ReleaseCapture(); return; }
                bool toggle = tool == Tool::Select && (GetKeyState(VK_CONTROL) < 0 || GetKeyState(VK_SHIFT) < 0);
                if (toggle && IsSelected(hit)) {
                    // Ctrl/Shift+click on a selected object removes it from the selection when released without
                    // moving; dragging moves the selection (Shift+drag locks the axis)
                    pendingDeselect_ = hit;
                    BeginMove();
                    return;
                }
                if (toggle) { if (selected >= 0) extra_.push_back(selected); selected = hit; }
                else if (!IsSelected(hit)) { selected = hit; extra_.clear(); }
                else if (hit != selected) { extra_.push_back(selected); selected = hit; }     // keeps all of them, this one is the primary
                if (onSelectionChanged) onSelectionChanged();
                BeginMove();
                return;
            }
        }
        if (tool == Tool::Select) {
            bool keep = GetKeyState(VK_CONTROL) < 0 || GetKeyState(VK_SHIFT) < 0;
            if (selected != -1 && !keep) { selected = -1; extra_.clear(); if (onSelectionChanged) onSelectionChanged(); InvalidateRect(hwnd, nullptr, FALSE); }
            if (smartMove) {               // Smart Move: dragging inside the area lifts the pixels; outside, marks another area
                int x = (int)d.X, y = (int)d.Y;
                bool inside = hasSel_ && x >= 0 && y >= 0 && x < doc->w() && y < doc->h() && selMask_[(size_t)y * doc->w() + x];
                if (inside) { StartSmartMove(d); return; }
                ClearRasterSelection();
                drag_ = Drag::Select; selRect_ = RectF(d.X, d.Y, 0, 0);
                return;
            }
            drag_ = Drag::Marquee; selRect_ = RectF(d.X, d.Y, 0, 0);    // object selection rectangle
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
        if (s.grad.Active()) FillGradientMask(base, m, cv::MaskBounds(m, base.w, base.h), s.grad, s.opacity);    // gradient over the filled area
        else {
            uint32_t c = (s.stroke & 0x00FFFFFF) | ((uint32_t)(255 * s.opacity / 100) << 24);
            for (size_t i = 0; i < m.size(); i++) if (m[i]) base.px[i] = BlendOver(base.px[i], c);
        }
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
        eraseLast_ = d;
        eraseShift_ = false;
        axisLock_ = 0;
        Refresh();
        return;
    case Tool::Crop:
        if (edgeSnap && GetKeyState(VK_MENU) >= 0) { downDoc_.X = SnapEdge(true, d.X, d.Y, d.Y); downDoc_.Y = SnapEdge(false, d.Y, d.X, d.X); }
        drag_ = Drag::Crop; hasCrop_ = false; crop_ = RectF(downDoc_.X, downDoc_.Y, 0, 0);
        return;
    case Tool::Cutout:
        drag_ = Drag::Cut; selRect_ = RectF(d.X, d.Y, 0, 0);
        return;
    case Tool::Selection: {
        int x = (int)d.X, y = (int)d.Y;
        bool inside = hasSel_ && x >= 0 && y >= 0 && x < doc->w() && y < doc->h() && selMask_[(size_t)y * doc->w() + x];
        if (inside) { StartSmartMove(d); return; }
        ClearRasterSelection();
        if (selMode == 3) {     // wand (CSelectTool::DoWand)
            cv::Mask m = cv::DoWand(*doc->st.base, { x, y }, tolerance, wandGlobal);
            if (!m.empty()) SetSelectionFromMask(this, std::move(m), hasSel_, selMask_, selBounds_, selOverlay_);
            InvalidateRect(hwnd, nullptr, FALSE);
            ReleaseCapture();
            return;
        }
        if (selMode == 2) { drag_ = Drag::Lasso; lasso_ = { d }; }
        else {
            if (edgeSnap && GetKeyState(VK_MENU) >= 0) { downDoc_.X = SnapEdge(true, d.X, d.Y, d.Y); downDoc_.Y = SnapEdge(false, d.Y, d.X, d.X); }
            drag_ = Drag::Select; selRect_ = RectF(downDoc_.X, downDoc_.Y, 0, 0);
        }
        return;
    }
    default: break;
    }
    if (!drawTool) { ReleaseCapture(); return; }

    // object creation
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
    // highlighter (CPaintOpHighlight, <Highlight> object, comp_5 §3.2): always a dragged rectangle, in the chosen
    // color and opacity
    case Tool::Highlighter: o.type = ObjType::Highlighter; break;
    case Tool::Text: o.type = ObjType::Text; break;
    case Tool::Callout: o.type = ObjType::Callout; break;
    case Tool::Stamp: o.type = ObjType::Stamp; o.text = stampId; break;
    case Tool::Blur: o.type = s.hideKind == 1 ? ObjType::Pixelate : ObjType::Blur; break;
    case Tool::Spotlight: o.type = ObjType::Spotlight; break;
    case Tool::Magnify: o.type = ObjType::Magnify; break;
    default: break;
    }
    if (o.IsAreaTool() && o.style.areaShape == AreaPolygon) {      // polygon: corners clicked one by one
        o.pts = { d, d };
        drag_ = Drag::Polygon;
        if (onHint) onHint(Tr(L"Clique nos cantos do polígono; clique no primeiro ponto, dê um duplo clique ou tecle Enter para fechar (Esc cancela)."));
        return;
    }
    if (o.IsAreaTool() && o.style.areaShape == AreaFreehand) o.pts = { d };
    drag_ = Drag::Create;
}

void Canvas::OnMove(POINT p, WPARAM keys) {
    if (!doc || doc->isVideo) return;
    PointF d = ToDoc(p);
    bool shift = (keys & MK_SHIFT) != 0;
    float dx = d.X - downDoc_.X, dy = d.Y - downDoc_.Y;
    if (rulers) {                                            // coordinates in the status bar and marks on the rulers
        if (onCursorMoved) onCursorMoved(d.X >= 0 && d.Y >= 0 && d.X < doc->w() && d.Y < doc->h(), d);
        RECT cr; GetClientRect(hwnd, &cr);
        int rs = RulerSize();
        RECT top{ 0, 0, cr.right, rs }, left{ 0, 0, rs, cr.bottom };
        InvalidateRect(hwnd, &top, FALSE); InvalidateRect(hwnd, &left, FALSE);
    }
    switch (drag_) {
    case Drag::None: {
        int hot = -1;                                        // link areas: outline and address under the cursor
        int hit = HitObject(d);
        if (hit >= 0 && doc->st.objs[hit]->type == ObjType::Link) hot = hit;
        else for (int i = (int)doc->st.objs.size() - 1; i >= 0; i--)
            if (doc->st.objs[i]->type == ObjType::Link && !doc->st.objs[i]->hidden && doc->st.objs[i]->Rect().Contains(d)) { hot = i; break; }
        if (hot != linkHot_) { linkHot_ = hot; InvalidateRect(hwnd, nullptr, FALSE); }
        SetCursorForPoint(p);
        return;
    }
    case Drag::Polygon: {
        DrawObj& o = *preview_;
        PointF e = d;
        if (shift && o.pts.size() >= 2) e = Constrain45(o.pts[o.pts.size() - 2], e);
        o.pts.back() = e;
        FitToPoints(o);
        SetCursor(LoadCursor(nullptr, IDC_CROSS));
        break;
    }
    case Drag::Guide: {
        auto& list = guideAxis_ ? doc->guidesY : doc->guidesX;
        if (guideIdx_ >= 0 && guideIdx_ < (int)list.size()) list[guideIdx_] = std::round(guideAxis_ ? d.Y : d.X);
        doc->modified = true;
        SetCursor(LoadCursor(nullptr, guideAxis_ ? IDC_SIZENS : IDC_SIZEWE));
        break;
    }
    case Drag::Pan: {
        scroll_.x = panStart_.x - (p.x - downScreen_.x);
        scroll_.y = panStart_.y - (p.y - downScreen_.y);
        UpdateScrollBars();
        break;
    }
    case Drag::Create: {
        DrawObj& o = *preview_;
        bool freehand = o.IsAreaTool() && o.style.areaShape == AreaFreehand;
        if ((o.IsPath() || freehand) && !o.pts.empty()) {
            PointF last = o.pts.back();
            if (std::hypot(d.X - last.X, d.Y - last.Y) * zoom > 1.5) o.pts.push_back(d);
            if (freehand) FitToPoints(o);
        } else {
            PointF e = d;
            if (shift) {
                // perfect geometry: lines at multiples of 45°; rectangle → square, ellipse → circle, rounded square,
                // equilateral triangle, regular hexagon… in every shape-based tool
                if (o.IsLine()) e = Constrain45(downDoc_, d);
                else {
                    float aspect = PerfectAspect(o);
                    float w = std::max(std::fabs(dx), std::fabs(dy) * aspect), h = w / aspect;
                    e = PointF(downDoc_.X + (dx < 0 ? -w : w), downDoc_.Y + (dy < 0 ? -h : h));
                }
            }
            o.p2 = e;
            o.ctrl = PointF((o.p1.X + o.p2.X) / 2, (o.p1.Y + o.p2.Y) / 2);
            RectF r = o.Rect();
            if (o.type == ObjType::Callout) {
                int sh = o.style.calloutShape;
                if (sh >= 5 && sh <= 11) o.tail = PointF(r.GetRight() + 30, r.Y + r.Height / 2);           // arrows: pointing right
                else if (sh == 4 || sh == 15) o.tail = PointF(r.GetRight() + std::max(40.0f, r.Width * 0.5f), r.GetBottom() + std::max(30.0f, r.Height * 0.6f));
                else o.tail = PointF(r.X + r.Width * 0.25f, r.GetBottom() + std::max(20.0f, r.Height * 0.45f));
            }
            if (o.type == ObjType::Magnify) o.tail = PointF(r.X - r.Width * 0.4f, r.GetBottom() + r.Height * 0.3f);
        }
        break;
    }
    case Drag::Move: {
        if (std::abs(p.x - downScreen_.x) + std::abs(p.y - downScreen_.y) < 2 && !changed_) return;
        if (dragMulti_.empty()) { if (onHint && SelectionLocked()) onHint(Tr(L"Objeto bloqueado: desbloqueie para mover.")); return; }
        changed_ = true;
        // Shift: locks the movement to the axis the mouse moved along first (arrow-key nudging is unchanged)
        if (shift) {
            if (!axisLock_) {
                int sx = std::abs(p.x - downScreen_.x), sy = std::abs(p.y - downScreen_.y);
                if (sx + sy >= 4) axisLock_ = sx >= sy ? 1 : 2;
            }
        } else axisLock_ = 0;
        if (axisLock_ == 1) dy = 0; else if (axisLock_ == 2) dx = 0;
        if (snapping && !(keys & MK_ALT)) {      // snapping: edges and center of the set to the edges and center of the image and to the guides
            RectF r = ObjExtent(dragMulti_[0].second);
            for (auto& m : dragMulti_) RectF::Union(r, r, ObjExtent(m.second));
            float tol = (float)(8 / zoom), W = (float)doc->w(), H = (float)doc->h();
            auto snap = [&](float a0, float len, float total, const std::vector<float>& guides, float d) {
                float best = d, bestDist = tol;
                std::vector<float> edges = { 0.0f, total / 2, total };
                if (rulers) edges.insert(edges.end(), guides.begin(), guides.end());
                for (float edge : edges)
                    for (float off : { 0.0f, len / 2, len }) {
                        float cand = edge - off - a0, dist = std::fabs(cand - d);
                        if (dist < bestDist) { bestDist = dist; best = cand; }
                    }
                return best;
            };
            if (axisLock_ != 2) dx = snap(r.X, r.Width, W, doc->guidesX, dx);
            if (axisLock_ != 1) dy = snap(r.Y, r.Height, H, doc->guidesY, dy);
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
            if (handle_ == 0) o->p1 = shift ? Constrain45(o->p2, d) : d;          // Shift: 45° steps
            else if (handle_ == 1) o->p2 = shift ? Constrain45(o->p1, d) : d;
            else o->ctrl = PointF(2 * d.X - 0.5f * (o->p1.X + o->p2.X), 2 * d.Y - 0.5f * (o->p1.Y + o->p2.Y));   // the curve goes through the point
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
            if (shift && (handle_ % 2 == 0) && r0.Width > 0 && r0.Height > 0) {     // proportional
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
        if (drag_ != Drag::Cut && shift) {                          // Shift: square (circle with the ellipse selection)
            float m = std::max(std::fabs(dx), std::fabs(dy));
            d = PointF(downDoc_.X + (dx < 0 ? -m : m), downDoc_.Y + (dy < 0 ? -m : m));
        } else if (drag_ != Drag::Cut && edgeSnap && !(keys & MK_ALT)) {    // the moving corner snaps to contrast edges
            d.X = SnapEdge(true, d.X, downDoc_.Y, d.Y);
            d.Y = SnapEdge(false, d.Y, downDoc_.X, d.X);
        }
        float x0 = std::clamp(std::min(downDoc_.X, d.X), 0.0f, (float)doc->w()), y0 = std::clamp(std::min(downDoc_.Y, d.Y), 0.0f, (float)doc->h());
        float x1 = std::clamp(std::max(downDoc_.X, d.X), 0.0f, (float)doc->w()), y1 = std::clamp(std::max(downDoc_.Y, d.Y), 0.0f, (float)doc->h());
        RectF r((float)(int)x0, (float)(int)y0, (float)(int)(x1 - (int)x0 + 0.5f), (float)(int)(y1 - (int)y0 + 0.5f));
        if (drag_ == Drag::Crop) crop_ = r; else selRect_ = r;
        if (drag_ != Drag::Cut && onAreaChanged) onAreaChanged();
        break;
    }
    case Drag::Lasso:
        if (std::hypot(d.X - lasso_.back().X, d.Y - lasso_.back().Y) * zoom > 2) lasso_.push_back(d);
        break;
    case Drag::Marquee:
        selRect_ = RectF(std::min(downDoc_.X, d.X), std::min(downDoc_.Y, d.Y), std::fabs(d.X - downDoc_.X), std::fabs(d.Y - downDoc_.Y));
        break;
    case Drag::Erase: {
        // with objects on top the whole composition is redone; without them only the erased pixels are refreshed
        bool objects = std::any_of(doc->st.objs.begin(), doc->st.objs.end(),
                                   [](const ObjPtr& o) { return !o->hidden && o->type != ObjType::Link; });
        // Shift: erases in a straight horizontal or vertical line from where Shift was pressed, along the axis the
        // mouse moved first (like Shift while moving objects)
        if (shift) {
            if (!eraseShift_) { eraseShift_ = true; eraseAnchor_ = eraseLast_; axisLock_ = 0; }
            if (!axisLock_) {
                float sx = std::fabs(d.X - eraseAnchor_.X) * (float)zoom, sy = std::fabs(d.Y - eraseAnchor_.Y) * (float)zoom;
                if (sx + sy < 4) break;
                axisLock_ = sx >= sy ? 1 : 2;
            }
            if (axisLock_ == 1) d.Y = eraseAnchor_.Y; else d.X = eraseAnchor_.X;
        } else { eraseShift_ = false; axisLock_ = 0; }
        if (!erasePending_) eraseRect_ = { doc->w(), doc->h(), 0, 0 };
        EraseLine(eraseLast_, d);
        eraseLast_ = d;
        if (objects || eraseRect_.left >= eraseRect_.right) dirty_ = true;
        else erasePending_ = true;
        break;
    }
    }
    InvalidateRect(hwnd, nullptr, FALSE);
}

void Canvas::FinishCreate() {
    DrawObj o = *preview_;
    preview_.reset();
    bool pointArea = o.IsAreaTool() && IsPointArea(o.style.areaShape);
    if (pointArea) {                                        // polygon / freehand outline: needs a real area
        if (o.pts.size() < 3) return;
        FitToPoints(o);
    }
    RectF r = o.Rect();
    float minSz = (float)(6 / zoom);
    bool tiny = o.IsPath() && !o.pts.empty() ? false : (o.IsLine() ? std::hypot(o.p2.X - o.p1.X, o.p2.Y - o.p1.Y) < minSz
                                                                                   : (r.Width < minSz || r.Height < minSz));
    if (pointArea) tiny = r.Width < minSz && r.Height < minSz;
    if (o.type == ObjType::Text && r.Width >= minSz * 3) {   // dragged box: its width, minimum height of one line
        tiny = false;
        float minH = o.style.fontSize * 1.4f + 2 * o.style.padding;
        if (r.Height < minH) { o.p1 = PointF(r.X, r.Y); o.p2 = PointF(r.GetRight(), r.Y + minH); }
    }
    if (tiny) {
        if (o.type == ObjType::Text) { o.p2 = PointF(o.p1.X + 260, o.p1.Y + o.style.fontSize * 1.6f); }
        else if (o.type == ObjType::Callout) {
            o.p2 = PointF(o.p1.X + 220, o.p1.Y + 90);
            o.tail = PointF(o.p1.X + 50, o.p1.Y + 150);
        } else if (o.type == ObjType::Stamp) {
            float a = StampAspect(o.text);
            float w = a > 1.2f ? 64 * a : 96.0f, h = a > 1.2f ? 64.0f : 96.0f;       // label stamps keep their proportion
            o.p1 = PointF(o.p1.X - w / 2, o.p1.Y - h / 2); o.p2 = PointF(o.p1.X + w, o.p1.Y + h);
        }
        else return;
    }
    doc->Checkpoint();
    doc->st.objs.push_back(std::make_shared<DrawObj>(o));
    selected = (int)doc->st.objs.size() - 1; extra_.clear();
    dirty_ = true;
    if (onSelectionChanged) onSelectionChanged();
    if (o.HasText()) BeginTextEdit(selected, true);
    if (o.HasLabel() && o.text.empty()) {                  // Skitch style: typing right away adds a label at the tail
        labelReady_ = true;
        if (onHint) onHint(Tr(L"Digite para escrever um texto na ponta inicial da seta."));
    }
}

void Canvas::OnUp(POINT p) {
    ReleaseCapture();
    if (!doc) return;
    if (drag_ == Drag::Polygon) return;                     // the polygon continues until it is closed
    Drag d = drag_;
    drag_ = Drag::None;
    switch (d) {
    case Drag::Create: FinishCreate(); break;
    case Drag::Move: case Drag::Resize:
        if (!changed_) {
            doc->DiscardCheckpoint();
            if (d == Drag::Move && pendingDeselect_ >= 0) {  // Ctrl/Shift+click without moving: leaves the selection
                auto v = SelectedList();
                v.erase(std::remove(v.begin(), v.end(), pendingDeselect_), v.end());
                SelectObjects(v);
            }
        }
        pendingDeselect_ = -1;
        break;
    case Drag::Guide: {                                     // dropped back on a ruler (or outside the view): removed
        RECT v = View();
        auto& list = guideAxis_ ? doc->guidesY : doc->guidesX;
        if (!PtInRect(&v, p) && guideIdx_ >= 0 && guideIdx_ < (int)list.size()) list.erase(list.begin() + guideIdx_);
        guideIdx_ = -1;
        doc->modified = true;
        InvalidateRect(hwnd, nullptr, FALSE);
        return;
    }
    case Drag::Crop:
        hasCrop_ = crop_.Width >= 2 && crop_.Height >= 2;
        if (onAreaChanged) onAreaChanged();
        break;
    case Drag::Select: {
        if (selRect_.Width >= 2 && selRect_.Height >= 2) {
            RECT r{ (LONG)selRect_.X, (LONG)selRect_.Y, (LONG)selRect_.GetRight(), (LONG)selRect_.GetBottom() };
            cv::Mask m = selMode == 1 ? cv::EllipseMask(doc->w(), doc->h(), r) : cv::RectMask(doc->w(), doc->h(), r);
            SetSelectionFromMask(this, std::move(m), hasSel_, selMask_, selBounds_, selOverlay_);
        } else if (onAreaChanged) onAreaChanged();          // a click without an area: no selection
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
        // CutOut (CPaintOpTearOutTool): removes the band and joins the parts
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
    case Drag::Marquee: {                       // selects the objects the rectangle touches (adding to the previous ones with Ctrl/Shift)
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
    int ga, gi, rs = RulerSize();
    if (rs && (p.x < rs || p.y < rs)) c = p.x < rs && p.y < rs ? IDC_ARROW : p.y < rs ? IDC_SIZENS : IDC_SIZEWE;
    else if (hh < 0 && tool == Tool::Select && HitGuide(p, ga, gi)) c = ga ? IDC_SIZENS : IDC_SIZEWE;
    else if (hh >= 0) {
        static const LPCWSTR rc[] = { IDC_SIZENWSE, IDC_SIZENS, IDC_SIZENESW, IDC_SIZEWE, IDC_SIZENWSE, IDC_SIZENS, IDC_SIZENESW, IDC_SIZEWE, IDC_CROSS };
        const DrawObj& o = *doc->st.objs[selected];
        c = o.IsLine() ? IDC_CROSS : rc[std::min(hh, 8)];
    } else if (tool == Tool::Select) {
        c = HitObject(ToDoc(p)) >= 0 ? IDC_SIZEALL : IDC_ARROW;
    } else if (tool == Tool::Selection) {
        PointF d = ToDoc(p); int x = (int)d.X, y = (int)d.Y;
        bool inside = hasSel_ && x >= 0 && y >= 0 && x < doc->w() && y < doc->h() && selMask_[(size_t)y * doc->w() + x];
        int hit = HitObject(d);
        if (hit >= 0 && doc->st.objs[hit]->floating) inside = true;
        c = inside ? IDC_SIZEALL : IDC_CROSS;
    } else if (tool == Tool::Fill || tool == Tool::Eyedropper) c = IDC_HAND;
    else {
        int hit = HitObject(ToDoc(p));
        c = (hit >= 0 && hit == selected) ? IDC_SIZEALL : IDC_CROSS;
    }
    SetCursor(LoadCursor(nullptr, c));
}

// ---------------------------------------------------------------- messages
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
        if (drag_ == Drag::Polygon) { OnDown(p, true); return 0; }
        if (doc && !doc->isVideo) {
            int hit = HitObject(ToDoc(p));
            if (hit >= 0) {
                const DrawObj& o = *doc->st.objs[hit];
                if (o.HasText() || o.HasLabel()) { selected = hit; extra_.clear(); BeginTextEdit(hit, false); return 0; }
                if (o.floating) { DropFloating(); return 0; }                 // double click puts the piece back
                if (o.type == ObjType::Link) {                               // opens the captured link
                    if (o.text.rfind(L"http://", 0) == 0 || o.text.rfind(L"https://", 0) == 0 || o.text.rfind(L"mailto:", 0) == 0)
                        ShellExecuteW(nullptr, L"open", o.text.c_str(), nullptr, nullptr, SW_SHOWNORMAL);
                    return 0;
                }
            }
        }
        OnDown(p, true);
        return 0;
    }
    case WM_MOUSEMOVE: OnMove({ GET_X_LPARAM(l), GET_Y_LPARAM(l) }, w); return 0;
    case WM_LBUTTONUP: OnUp({ GET_X_LPARAM(l), GET_Y_LPARAM(l) }); return 0;
    case WM_MBUTTONDOWN: SetCapture(hwnd); drag_ = Drag::Pan; downScreen_ = { GET_X_LPARAM(l), GET_Y_LPARAM(l) }; panStart_ = { scroll_.x, scroll_.y }; return 0;
    case WM_MBUTTONUP: ReleaseCapture(); drag_ = Drag::None; return 0;
    case WM_RBUTTONDOWN: {                      // right click: selects the object below (keeping the tool) for the context menu
        if (!doc || doc->isVideo || drag_ != Drag::None || fxPreview_) return 0;
        CommitText();
        SetFocus(hwnd);
        POINT p{ GET_X_LPARAM(l), GET_Y_LPARAM(l) };
        int hit = HitObject(ToDoc(p));
        if (hit >= 0 && !IsSelected(hit)) { selected = hit; extra_.clear(); if (onSelectionChanged) onSelectionChanged(); InvalidateRect(hwnd, nullptr, FALSE); }
        return 0;
    }
    case WM_CONTEXTMENU: {
        POINT s{ GET_X_LPARAM(l), GET_Y_LPARAM(l) };
        if (s.x == -1 && s.y == -1) GetCursorPos(&s);
        if (onContextMenu && doc && !doc->isVideo) onContextMenu(s);
        return 0;
    }
    case WM_CHAR:
        // an arrow was just drawn: the first typed character starts its label
        if (labelReady_ && !edit_ && w >= 32 && doc && selected >= 0 && selected < (int)doc->st.objs.size() &&
            doc->st.objs[selected]->HasLabel() && doc->st.objs[selected]->text.empty()) {
            BeginTextEdit(selected, false);
            if (edit_) {
                wchar_t ch[2] = { (wchar_t)w, 0 };
                SetWindowTextW(edit_, ch);
                SendMessageW(edit_, EM_SETSEL, 1, 1);
                LayoutTextEdit();
            }
            return 0;
        }
        break;
    case WM_SETCURSOR:
        if (LOWORD(l) == HTCLIENT && doc && !doc->isVideo) { POINT p; GetCursorPos(&p); ScreenToClient(hwnd, &p); SetCursorForPoint(p); return TRUE; }
        break;
    case WM_MOUSEWHEEL: {
        int delta = GET_WHEEL_DELTA_WPARAM(w);
        if (GET_KEYSTATE_WPARAM(w) & MK_CONTROL) {
            // zoom centered on the mouse; touchpads (pinch) send small deltas, which zoom proportionally
            POINT p{ GET_X_LPARAM(l), GET_Y_LPARAM(l) }; ScreenToClient(hwnd, &p);
            ZoomTo(zoom * std::pow(1.15, std::clamp(delta / (double)WHEEL_DELTA, -4.0, 4.0)), &p);
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
        case VK_DELETE: case VK_BACK:
            if (drag_ == Drag::Polygon && preview_) {          // removes the last corner of the polygon
                auto& pts = preview_->pts;
                if (pts.size() > 2) { pts.erase(pts.end() - 2); InvalidateRect(hwnd, nullptr, FALSE); } else ResetPolygon();
                return 0;
            }
            DeleteSelected();
            return 0;
        case VK_ESCAPE:
            if (drag_ != Drag::None) { if ((drag_ == Drag::Move || drag_ == Drag::Resize) && changed_) { doc->Undo(); dirty_ = true; } else if (drag_ == Drag::Move || drag_ == Drag::Resize) doc->DiscardCheckpoint(); ReleaseCapture(); drag_ = Drag::None; preview_.reset(); }
            if (hasCrop_) { hasCrop_ = false; if (onAreaChanged) onAreaChanged(); }
            ClearRasterSelection();
            labelReady_ = false;
            if (selected >= 0) { selected = -1; extra_.clear(); if (onSelectionChanged) onSelectionChanged(); }
            InvalidateRect(hwnd, nullptr, FALSE);
            return 0;
        case VK_RETURN:
            if (drag_ == Drag::Polygon) FinishPolygon();
            else if (hasCrop_) ApplyCrop();
            else if (doc && selected >= 0 && selected < (int)doc->st.objs.size() && doc->st.objs[selected]->floating) DropFloating();
            return 0;
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
