// Layers panel (F03): the document's objects as a Photoshop-like list, front to back. It works on the canvas
// selection and on the objects' visibility, lock, name and z-order (CPaintOpReorderComponentSelection in the
// reference, comp_5 §4), each change being an undoable step of the document.
#include "Panels.h"
#include "Ui.h"
#include <commctrl.h>
#include <cmath>

namespace snap::ed {

namespace {
Gdiplus::Color LC(COLORREF c, BYTE a = 255) { return Gdiplus::Color(a, GetRValue(c), GetGValue(c), GetBValue(c)); }

icons::Id TypeIcon(const DrawObj& o) {
    switch (o.type) {
    case ObjType::Arrow: return icons::Arrow;
    case ObjType::Line: return icons::Line;
    case ObjType::Pen: return icons::Pen;
    case ObjType::Highlighter: return icons::Highlighter;
    case ObjType::Text: return icons::Text;
    case ObjType::Callout: return icons::Callout;
    case ObjType::Step: return icons::Step;
    case ObjType::Stamp: return icons::Stamp;
    case ObjType::Blur: case ObjType::Pixelate: return icons::Blur;
    case ObjType::Spotlight: return icons::Spotlight;
    case ObjType::Magnify: return icons::Magnify;
    case ObjType::Picture: return icons::Image;
    case ObjType::Group: return icons::Group;
    case ObjType::Link: return icons::Link;
    default: return o.Kind() == ShapeEllipse ? icons::Ellipse : icons::Rect;
    }
}
}

std::wstring LayersPanel::DisplayName(const DrawObj& o) {
    if (!o.name.empty()) return o.name;
    auto firstLine = [](const std::wstring& t) {
        std::wstring l = t.substr(0, t.find(L'\n'));
        return l.size() > 40 ? l.substr(0, 40) + L"…" : l;
    };
    static const wchar_t* shapes[ShapeCount] = { TR_NOOP(L"Retângulo"), TR_NOOP(L"Retângulo arredondado"), TR_NOOP(L"Elipse"), TR_NOOP(L"Triângulo"),
                                                 TR_NOOP(L"Losango"), TR_NOOP(L"Paralelogramo"), TR_NOOP(L"Hexágono"), TR_NOOP(L"Estrela") };
    switch (o.type) {
    case ObjType::Arrow: return o.text.empty() ? Tr(L"Seta") : Tr(L"Seta") + std::wstring(L": ") + firstLine(o.text);
    case ObjType::Line: return Tr(L"Linha");
    case ObjType::Pen: return Tr(L"Caneta");
    case ObjType::Highlighter: return Tr(L"Marcador");
    case ObjType::Text: return o.text.empty() ? std::wstring(Tr(L"Texto")) : firstLine(o.text);
    case ObjType::Callout: return o.text.empty() ? std::wstring(Tr(L"Balão")) : firstLine(o.text);
    case ObjType::Step: return Format(Tr(L"Passo %s"), StepLabel(o.stepValue, o.style.stepType).c_str());
    case ObjType::Stamp: return o.text.rfind(L"label:", 0) == 0 ? o.text.substr(std::min(o.text.size(), o.text.find(L':', 6) + 1)) : std::wstring(Tr(L"Carimbo"));
    case ObjType::Blur: return Tr(L"Desfoque");
    case ObjType::Pixelate: return Tr(L"Pixelado");
    case ObjType::Spotlight: return Tr(L"Destaque");
    case ObjType::Magnify: return Tr(L"Lupa");
    case ObjType::Picture: return o.floating ? Tr(L"Peça solta") : Tr(L"Imagem");
    case ObjType::Group: return Format(Tr(L"Grupo (%zu)"), o.children.size());
    case ObjType::Link: return Tr(L"Link: ") + o.text;
    default: return Tr(shapes[o.Kind()]);
    }
}

void LayersPanel::Create(HWND parent, Canvas* c) {
    canvas = c;
    static bool reg = false;
    if (!reg) {
        WNDCLASSW wc{}; wc.lpfnWndProc = Proc; wc.hInstance = GetModuleHandleW(nullptr); wc.hCursor = LoadCursor(nullptr, IDC_ARROW);
        wc.lpszClassName = L"SnapnabLayers"; wc.style = CS_DBLCLKS;
        RegisterClassW(&wc); reg = true;
    }
    hwnd = CreateWindowExW(0, L"SnapnabLayers", nullptr, WS_CHILD | WS_CLIPCHILDREN, 0, 0, 10, 10, parent, nullptr, GetModuleHandleW(nullptr), this);
}

void LayersPanel::Refresh() {
    if (!hwnd) return;
    if (edit_) EndRename(false);
    Layout();
}

void LayersPanel::Layout() {
    rows_.clear();
    RECT cr; GetClientRect(hwnd, &cr);
    int dpi = DpiForWindow(hwnd);
    auto S = [dpi](int v) { return ui::Scale(v, dpi); };
    int rh = S(34), y = S(38);
    Document* doc = canvas ? canvas->doc : nullptr;
    if (doc && !doc->isVideo) {
        for (int i = (int)doc->st.objs.size() - 1; i >= 0; i--) {       // front (top) to back
            const DrawObj& o = *doc->st.objs[i];
            rows_.push_back({ i, -1, 0, { 0, y, cr.right, y + rh } });
            y += rh;
            if (o.type == ObjType::Group && expanded_.count(doc->st.objs[i].get()))
                for (int k = (int)o.children.size() - 1; k >= 0; k--) { rows_.push_back({ i, k, 1, { 0, y, cr.right, y + rh } }); y += rh; }
        }
        Row bg; bg.background = true; bg.r = { 0, y, cr.right, y + rh };
        rows_.push_back(bg);
        y += rh;
    }
    content_ = y + S(8);
    scroll_ = std::clamp(scroll_, 0, std::max(0, content_ - (int)cr.bottom));
    sb_.content = content_; sb_.view = cr.bottom; sb_.pos = scroll_;
    sb_.track = { cr.right - S(6), S(40), cr.right - S(1), cr.bottom - S(2) };
    InvalidateRect(hwnd, nullptr, FALSE);
}

int LayersPanel::HitRow(POINT p) const {
    for (size_t i = 0; i < rows_.size(); i++) {
        RECT r = rows_[i].r; OffsetRect(&r, 0, -scroll_);
        if (PtInRect(&r, p)) return (int)i;
    }
    return -1;
}

// drop position while dragging rows: between rows (the top half of a row = above it); in z-order indices
int LayersPanel::DropBefore(POINT p) const {
    if (!canvas || !canvas->doc) return -1;
    int n = (int)canvas->doc->st.objs.size();
    for (auto& row : rows_) {
        if (row.child >= 0) continue;
        RECT r = row.r; OffsetRect(&r, 0, -scroll_);
        if (row.background) { if (p.y < r.bottom) return 0; continue; }
        if (p.y < (r.top + r.bottom) / 2) return row.obj + 1;      // above this row = in front of it
        if (p.y < r.bottom) return row.obj;
    }
    return p.y < (rows_.empty() ? 0 : rows_.front().r.top - scroll_) ? n : 0;
}

const Image& LayersPanel::Thumb(const ObjPtr& o, int px) {
    auto it = thumbs_.find(o.get());
    if (it != thumbs_.end() && it->second.first.lock() == o && it->second.second.w == px) return it->second.second;
    if (thumbs_.size() > 300) thumbs_.clear();
    Image out;
    Gdiplus::RectF b = o->Bounds();
    bool rasterFx = o->type == ObjType::Blur || o->type == ObjType::Pixelate || o->type == ObjType::Spotlight ||
                    o->type == ObjType::Magnify || o->type == ObjType::Link;
    if (!rasterFx && b.Width >= 1 && b.Height >= 1 && b.Width * b.Height <= 2500000) {
        // the object alone, drawn at its size and reduced to the thumbnail
        int w = (int)std::ceil(b.Width) + 2, h = (int)std::ceil(b.Height) + 2;
        Image full(w, h, 0);
        DrawObj c = *o;
        c.hidden = false;
        c.Move(1 - b.X, 1 - b.Y);
        {
            auto bmp = full.WrapGdip();
            Gdiplus::Graphics g(bmp.get());
            g.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
            g.SetTextRenderingHint(Gdiplus::TextRenderingHintAntiAlias);
            RenderObject(g, full, c);
            g.Flush(Gdiplus::FlushIntentionSync);
        }
        double k = std::min((double)px / w, (double)px / h);
        Image sc = ScaleImage(full, std::max(1, (int)(w * k)), std::max(1, (int)(h * k)));
        out = Image(px, px, 0);
        out.Blit(sc, (px - sc.w) / 2, (px - sc.h) / 2, false);
    }
    return (thumbs_[o.get()] = { o, std::move(out) }).second;
}

void LayersPanel::Paint(HDC hdc) {
    const auto& th = EdTheme();
    RECT cr; GetClientRect(hwnd, &cr);
    HDC mem = CreateCompatibleDC(hdc);
    HBITMAP bb = CreateCompatibleBitmap(hdc, std::max(1L, cr.right), std::max(1L, cr.bottom));
    HGDIOBJ ob = SelectObject(mem, bb);
    HBRUSH bg = CreateSolidBrush(th.bgMain); FillRect(mem, &cr, bg); DeleteObject(bg);
    int dpi = DpiForWindow(hwnd);
    auto S = [dpi](int v) { return ui::Scale(v, dpi); };
    Document* doc = canvas ? canvas->doc : nullptr;
    {
        Gdiplus::Graphics g(mem);
        g.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
        g.SetPixelOffsetMode(Gdiplus::PixelOffsetModeHalf);
        for (size_t i = 0; i < rows_.size(); i++) {
            const Row& row = rows_[i];
            RECT r = row.r; OffsetRect(&r, 0, -scroll_);
            if (r.bottom < S(36) || r.top > cr.bottom) continue;
            const DrawObj* o = row.background || !doc ? nullptr : row.child >= 0 ? &doc->st.objs[row.obj]->children[row.child] : doc->st.objs[row.obj].get();
            bool sel = !row.background && row.child < 0 && canvas->IsSelected(row.obj);
            bool childOfSel = !row.background && row.child >= 0 && canvas->IsSelected(row.obj);
            if (sel) { HBRUSH sb = CreateSolidBrush(ui::Mix(RGB(255, 255, 255), th.control, 0.2)); FillRect(mem, &r, sb); DeleteObject(sb); }
            else if (childOfSel || (int)i == hover_) { HBRUSH hb = CreateSolidBrush(ui::Mix(th.bgMain, th.control, 0.07)); FillRect(mem, &r, hb); DeleteObject(hb); }
            HPEN lp = CreatePen(PS_SOLID, 1, ui::Mix(th.line, th.bgMain, 0.4)); HGDIOBJ op = SelectObject(mem, lp);
            MoveToEx(mem, 0, r.bottom - 1, nullptr); LineTo(mem, cr.right, r.bottom - 1);
            SelectObject(mem, op); DeleteObject(lp);
            int x = S(8), cy = (r.top + r.bottom) / 2, is = S(18);
            COLORREF fg = o && o->hidden ? RGB(150, 152, 158) : RGB(40, 42, 48);
            if (!row.background && row.child < 0) {
                icons::Draw(g, o->hidden ? icons::EyeOff : icons::Eye, Gdiplus::RectF((float)x, (float)cy - is / 2, (float)is, (float)is), LC(o->hidden ? RGB(160, 160, 166) : RGB(70, 72, 78)));
                icons::Draw(g, o->locked ? icons::Lock : icons::Unlock, Gdiplus::RectF((float)x + S(26), (float)cy - is / 2, (float)is, (float)is),
                            LC(o->locked ? th.control : RGB(200, 202, 208)));
            }
            x += S(52) + row.depth * S(18);
            if (o && o->type == ObjType::Group && row.child < 0) {      // expander
                bool open = expanded_.count(o) != 0;
                Gdiplus::SolidBrush tb(LC(RGB(80, 80, 88)));
                float k = (float)S(4), ex = (float)x + S(4);
                if (open) { Gdiplus::PointF p[3] = { { ex - k, cy - k / 2 }, { ex + k, cy - k / 2 }, { ex, cy + k * 0.7f } }; g.FillPolygon(&tb, p, 3); }
                else { Gdiplus::PointF p[3] = { { ex - k / 2, cy - k }, { ex + k * 0.7f, (float)cy }, { ex - k / 2, cy + k } }; g.FillPolygon(&tb, p, 3); }
            }
            x += S(14);
            RECT tr{ x, r.top + S(4), x + S(26), r.bottom - S(4) };
            // thumbnail (a checkerboard frame like Photoshop's layer thumbnails)
            HBRUSH tf = CreateSolidBrush(RGB(255, 255, 255)); FillRect(mem, &tr, tf); DeleteObject(tf);
            HBRUSH tfr = CreateSolidBrush(RGB(205, 207, 213)); FrameRect(mem, &tr, tfr); DeleteObject(tfr);
            int tsz = tr.right - tr.left - 2;
            if (row.background) {
                if (doc && doc->st.base && !doc->st.base->empty()) {
                    const Image& base = *doc->st.base;
                    double k = std::min((double)tsz / base.w, (double)tsz / base.h);
                    int w = std::max(1, (int)(base.w * k)), h = std::max(1, (int)(base.h * k));
                    base.DrawTo(mem, tr.left + 1 + (tsz - w) / 2, tr.top + 1 + (tsz - h) / 2, w, h, true);
                }
            } else {
                const Image* img = row.child < 0 ? &Thumb(doc->st.objs[row.obj], tsz) : nullptr;
                if (img && !img->empty()) {
                    Image copy = *img;
                    auto bmp = copy.WrapGdip();
                    g.DrawImage(bmp.get(), Gdiplus::RectF((float)tr.left + 1, (float)tr.top + 1, (float)tsz, (float)tsz));
                } else icons::Draw(g, TypeIcon(*o), Gdiplus::RectF((float)tr.left + S(3), (float)tr.top + S(3), (float)tsz - S(4), (float)tsz - S(4)), LC(RGB(70, 72, 78)));
            }
            RECT nr{ tr.right + S(8), r.top, cr.right - S(10), r.bottom };
            std::wstring name = row.background ? std::wstring(Tr(L"Fundo (imagem)")) : DisplayName(*o);
            HGDIOBJ of = SelectObject(mem, ui::UiFont(dpi, 9, sel));
            SetBkMode(mem, TRANSPARENT); SetTextColor(mem, row.background ? RGB(110, 112, 118) : fg);
            DrawTextW(mem, name.c_str(), -1, &nr, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);
            SelectObject(mem, of);
        }
        // drop line while dragging
        if (dragging_ && dropAt_ >= 0 && doc) {
            int ly = -1, n = (int)doc->st.objs.size();
            for (auto& row : rows_) {
                if (row.child >= 0) continue;
                if (!row.background && row.obj == dropAt_ - 1 && ly < 0) { ly = row.r.top; break; }
                if (row.background && dropAt_ == 0) { ly = row.r.top; break; }
            }
            if (dropAt_ >= n && !rows_.empty()) ly = rows_.front().r.top;
            if (ly >= 0) {
                Gdiplus::Pen dp(LC(th.control), (float)S(2));
                g.DrawLine(&dp, (float)S(6), (float)(ly - scroll_), (float)cr.right - S(10), (float)(ly - scroll_));
            }
        }
        sb_.pos = scroll_;
        sb_.Paint(g);
    }
    // header (drawn last: covers the rows scrolled under it)
    RECT hr{ 0, 0, cr.right, S(36) };
    HBRUSH hb = CreateSolidBrush(th.bgSide); FillRect(mem, &hr, hb); DeleteObject(hb);
    std::wstring title = Tr(L"Camadas");
    if (doc && !doc->isVideo) title += Format(L"  (%zu)", doc->st.objs.size());
    RECT ht{ S(12), 0, cr.right - S(12), S(36) };
    HGDIOBJ of = SelectObject(mem, ui::UiFont(dpi, 9, true));
    SetBkMode(mem, TRANSPARENT); SetTextColor(mem, RGB(40, 42, 48));
    DrawTextW(mem, title.c_str(), -1, &ht, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
    SelectObject(mem, of);
    if (!doc || doc->isVideo) {
        RECT er{ S(12), S(48), cr.right - S(12), S(120) };
        ui::DrawTextCentered(mem, Tr(L"Nenhuma imagem aberta."), er, RGB(120, 120, 128), ui::UiFont(dpi, 9));
    } else if (doc->st.objs.empty()) {
        RECT er{ S(12), rows_.empty() ? S(48) : rows_.back().r.bottom - scroll_ + S(10), cr.right - S(12), (rows_.empty() ? S(48) : rows_.back().r.bottom - scroll_) + S(70) };
        HGDIOBJ f2 = SelectObject(mem, ui::UiFont(dpi, 8));
        SetTextColor(mem, RGB(120, 120, 128));
        DrawTextW(mem, Tr(L"Os objetos desenhados (setas, textos, formas…) aparecem aqui, do mais à frente ao mais atrás."), -1, &er, DT_CENTER | DT_WORDBREAK);
        SelectObject(mem, f2);
    }
    BitBlt(hdc, 0, 0, cr.right, cr.bottom, mem, 0, 0, SRCCOPY);
    SelectObject(mem, ob); DeleteObject(bb); DeleteDC(mem);
}

void LayersPanel::BeginRename(int row) {
    if (row < 0 || row >= (int)rows_.size() || rows_[row].background || rows_[row].child >= 0 || !canvas->doc) return;
    EndRename(false);
    int dpi = DpiForWindow(hwnd);
    RECT r = rows_[row].r; OffsetRect(&r, 0, -scroll_);
    int x = ui::Scale(52 + 14 + 26 + 8, dpi) + ui::Scale(8, dpi);
    editObj_ = rows_[row].obj;
    const DrawObj& o = *canvas->doc->st.objs[editObj_];
    edit_ = CreateWindowExW(0, L"EDIT", DisplayName(o).c_str(), WS_CHILD | WS_VISIBLE | WS_BORDER | ES_AUTOHSCROLL,
                            x, r.top + ui::Scale(5, dpi), r.right - x - ui::Scale(10, dpi), r.bottom - r.top - ui::Scale(10, dpi),
                            hwnd, (HMENU)90, GetModuleHandleW(nullptr), nullptr);
    SendMessageW(edit_, WM_SETFONT, (WPARAM)ui::UiFont(dpi, 9), TRUE);
    SendMessageW(edit_, EM_SETSEL, 0, -1);
    SetFocus(edit_);
    // Enter commits and Esc cancels (subclass of the edit box)
    SetWindowSubclass(edit_, [](HWND h, UINT m, WPARAM w, LPARAM l, UINT_PTR, DWORD_PTR ref) -> LRESULT {
        LayersPanel* self = (LayersPanel*)ref;
        if (m == WM_KEYDOWN && (w == VK_RETURN || w == VK_ESCAPE)) { self->EndRename(w == VK_RETURN); return 0; }
        if (m == WM_CHAR && (w == VK_RETURN || w == VK_ESCAPE)) return 0;
        if (m == WM_KILLFOCUS) { LRESULT res = DefSubclassProc(h, m, w, l); self->EndRename(true); return res; }
        return DefSubclassProc(h, m, w, l);
    }, 1, (DWORD_PTR)this);
}

void LayersPanel::EndRename(bool commit) {
    if (!edit_) return;
    HWND e = edit_;
    edit_ = nullptr;                 // avoids reentrancy through WM_KILLFOCUS
    int n = GetWindowTextLengthW(e);
    std::wstring t(n + 1, L'\0');
    GetWindowTextW(e, t.data(), n + 1);
    t.resize(n);
    DestroyWindow(e);
    if (commit && canvas->doc && editObj_ >= 0 && editObj_ < (int)canvas->doc->st.objs.size()) {
        std::wstring name = Trim(t);
        const DrawObj& o = *canvas->doc->st.objs[editObj_];
        DrawObj tmp = o; tmp.name.clear();
        if (name == DisplayName(tmp)) name.clear();          // the automatic name stays automatic
        canvas->RenameObject(editObj_, name);
    }
    editObj_ = -1;
    Layout();
}

LRESULT CALLBACK LayersPanel::Proc(HWND h, UINT m, WPARAM w, LPARAM l) {
    LayersPanel* self = (LayersPanel*)GetWindowLongPtrW(h, GWLP_USERDATA);
    if (m == WM_NCCREATE) { self = (LayersPanel*)((CREATESTRUCTW*)l)->lpCreateParams; SetWindowLongPtrW(h, GWLP_USERDATA, (LONG_PTR)self); self->hwnd = h; }
    return self ? self->Handle(m, w, l) : DefWindowProcW(h, m, w, l);
}

LRESULT LayersPanel::Handle(UINT m, WPARAM w, LPARAM l) {
    int dpi = DpiForWindow(hwnd);
    auto S = [dpi](int v) { return ui::Scale(v, dpi); };
    Document* doc = canvas ? canvas->doc : nullptr;
    switch (m) {
    case WM_SIZE: Layout(); return 0;
    case WM_ERASEBKGND: return 1;
    case WM_PAINT: { PAINTSTRUCT ps; HDC dc = BeginPaint(hwnd, &ps); Paint(dc); EndPaint(hwnd, &ps); return 0; }
    case WM_MOUSEWHEEL: {
        RECT cr; GetClientRect(hwnd, &cr);
        int ns = std::clamp(scroll_ - GET_WHEEL_DELTA_WPARAM(w) * S(48) / WHEEL_DELTA, 0, std::max(0, content_ - (int)cr.bottom));
        if (ns != scroll_) { scroll_ = ns; if (edit_) EndRename(true); InvalidateRect(hwnd, nullptr, FALSE); }
        return 0;
    }
    case WM_MOUSEMOVE: {
        POINT p{ GET_X_LPARAM(l), GET_Y_LPARAM(l) };
        if (sb_.dragging) { if (sb_.Move(p)) { scroll_ = sb_.pos; InvalidateRect(hwnd, nullptr, FALSE); } return 0; }
        if (press_ >= 0 && (w & MK_LBUTTON)) {
            if (!dragging_ && std::abs(p.y - pressPt_.y) > S(5)) dragging_ = true;
            if (dragging_) { int d = DropBefore(p); if (d != dropAt_) { dropAt_ = d; InvalidateRect(hwnd, nullptr, FALSE); } SetCursor(LoadCursor(nullptr, IDC_SIZENS)); }
            return 0;
        }
        int hv = HitRow(p);
        if (hv != hover_) { hover_ = hv; InvalidateRect(hwnd, nullptr, FALSE); }
        TRACKMOUSEEVENT tme{ sizeof(tme), TME_LEAVE, hwnd, 0 }; TrackMouseEvent(&tme);
        return 0;
    }
    case WM_MOUSELEAVE: hover_ = -1; InvalidateRect(hwnd, nullptr, FALSE); return 0;
    case WM_LBUTTONDOWN: case WM_LBUTTONDBLCLK: {
        POINT p{ GET_X_LPARAM(l), GET_Y_LPARAM(l) };
        SetFocus(hwnd);
        if (sb_.Down(p)) { scroll_ = sb_.pos; if (sb_.dragging) SetCapture(hwnd); InvalidateRect(hwnd, nullptr, FALSE); return 0; }
        int ri = HitRow(p);
        if (ri < 0 || !doc) return 0;
        const Row& row = rows_[ri];
        if (row.background) { canvas->SelectObjects({}); return 0; }
        int x = p.x;
        DrawObj& o = *doc->st.objs[row.obj];
        if (row.child < 0 && x < S(8 + 22)) { canvas->SetHidden({ row.obj }, !o.hidden); return 0; }       // eye
        if (row.child < 0 && x >= S(30) && x < S(30 + 26)) { canvas->SetLocked({ row.obj }, !o.locked); return 0; }  // lock
        int ex = S(52) + row.depth * S(18);
        if (o.type == ObjType::Group && row.child < 0 && x >= ex && x < ex + S(14)) {     // expander
            if (expanded_.count(&o)) expanded_.erase(&o); else expanded_.insert(&o);
            Layout();
            return 0;
        }
        if (m == WM_LBUTTONDBLCLK && row.child < 0) { BeginRename(ri); return 0; }
        if (o.hidden) { if (onHint) onHint(Tr(L"Objeto oculto: clique no olho para mostrá-lo.")); }
        // selection: click, Ctrl+click toggles, Shift+click selects the range
        auto v = canvas->SelectedList();
        bool ctrl = GetKeyState(VK_CONTROL) < 0, shift = GetKeyState(VK_SHIFT) < 0;
        if (shift && anchor_ >= 0 && anchor_ < (int)doc->st.objs.size()) {
            int a = std::min(anchor_, row.obj), b = std::max(anchor_, row.obj);
            std::vector<int> range;
            for (int k = a; k <= b; k++) if (!doc->st.objs[k]->hidden) range.push_back(k);
            canvas->SelectObjects(range);
        } else if (ctrl) {
            if (std::find(v.begin(), v.end(), row.obj) != v.end()) v.erase(std::remove(v.begin(), v.end(), row.obj), v.end());
            else v.push_back(row.obj);
            canvas->SelectObjects(v);
            anchor_ = row.obj;
        } else {
            if (std::find(v.begin(), v.end(), row.obj) == v.end()) canvas->SelectObjects({ row.obj });
            anchor_ = row.obj;
            press_ = ri; pressPt_ = p; dragging_ = false; dropAt_ = -1;
            SetCapture(hwnd);
        }
        return 0;
    }
    case WM_LBUTTONUP: {
        if (sb_.dragging) { sb_.Up(); ReleaseCapture(); return 0; }
        if (press_ < 0) return 0;
        int pressed = press_;
        bool dragged = dragging_;
        press_ = -1; dragging_ = false;
        ReleaseCapture();
        if (dragged && dropAt_ >= 0 && doc) canvas->MoveObjects(canvas->SelectedList(), dropAt_);
        else if (!dragged && pressed < (int)rows_.size() && rows_[pressed].child < 0) {
            int obj = rows_[pressed].obj;
            auto v = canvas->SelectedList();
            if (v.size() > 1) canvas->SelectObjects({ obj });      // plain click on a selected row: only it
        }
        dropAt_ = -1;
        InvalidateRect(hwnd, nullptr, FALSE);
        return 0;
    }
    case WM_CAPTURECHANGED: if ((HWND)l != hwnd) { press_ = -1; dragging_ = false; sb_.Up(); } return 0;
    case WM_RBUTTONUP: {
        POINT p{ GET_X_LPARAM(l), GET_Y_LPARAM(l) };
        int ri = HitRow(p);
        if (ri < 0 || rows_[ri].background || !doc) return 0;
        if (!canvas->IsSelected(rows_[ri].obj)) canvas->SelectObjects({ rows_[ri].obj });
        if (onContextMenu) { ClientToScreen(hwnd, &p); onContextMenu(p); }
        return 0;
    }
    case WM_KEYDOWN:
        if (w == VK_F2) { for (size_t i = 0; i < rows_.size(); i++) if (!rows_[i].background && rows_[i].child < 0 && rows_[i].obj == canvas->selected) { BeginRename((int)i); break; } return 0; }
        if (w == VK_DELETE) { canvas->DeleteSelected(); return 0; }
        break;
    case WM_GETDLGCODE: return DLGC_WANTARROWS;
    }
    return DefWindowProcW(hwnd, m, w, l);
}

} // namespace snap::ed
