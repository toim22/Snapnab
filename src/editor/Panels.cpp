#include "Panels.h"
#include "Ui.h"
#include "Video.h"
#include "Widgets.h"
#include <cmath>

namespace snap::ed {

using ui::Mix;

static ui::ThemeColors g_theme;
const ui::ThemeColors& EdTheme() { return g_theme; }
void SetEdTheme(const ui::ThemeColors& t) { g_theme = t; }

static int Sc(int v, HWND h) { return ui::Scale(v, DpiForWindow(h)); }

static Gdiplus::Color GC(COLORREF c, BYTE a = 255) { return Gdiplus::Color(a, GetRValue(c), GetGValue(c), GetBValue(c)); }

static void RoundPath(Gdiplus::GraphicsPath& p, Gdiplus::RectF r, float rad) {
    float d = std::min({ rad * 2, r.Width, r.Height });
    p.AddArc(r.X, r.Y, d, d, 180, 90); p.AddArc(r.GetRight() - d, r.Y, d, d, 270, 90);
    p.AddArc(r.GetRight() - d, r.GetBottom() - d, d, d, 0, 90); p.AddArc(r.X, r.GetBottom() - d, d, d, 90, 90);
    p.CloseFigure();
}

static void FillRound(Gdiplus::Graphics& g, RECT r, float rad, COLORREF c, BYTE a = 255) {
    Gdiplus::GraphicsPath p;
    RoundPath(p, Gdiplus::RectF((float)r.left, (float)r.top, (float)(r.right - r.left), (float)(r.bottom - r.top)), rad);
    Gdiplus::SolidBrush b(GC(c, a)); g.FillPath(&b, &p);
}

static void StrokeRound(Gdiplus::Graphics& g, RECT r, float rad, COLORREF c, float w) {
    Gdiplus::GraphicsPath p;
    RoundPath(p, Gdiplus::RectF(r.left + w / 2, r.top + w / 2, (r.right - r.left) - w, (r.bottom - r.top) - w), rad);
    Gdiplus::Pen pen(GC(c), w); g.DrawPath(&pen, &p);
}

// ================================================================= barra de rolagem
RECT LiteScroll::Thumb() const {
    int th = track.bottom - track.top;
    int h = std::max(std::min(th, (int)(track.right - track.left) * 4), view > 0 && content > 0 ? th * view / content : th);
    int top = track.top + (MaxPos() > 0 ? (th - h) * pos / MaxPos() : 0);
    return { track.left, top, track.right, top + h };
}

bool LiteScroll::Hit(POINT p) const {
    if (!Visible()) return false;
    RECT r = track; InflateRect(&r, 3, 0);
    return PtInRect(&r, p) != 0;
}

bool LiteScroll::Down(POINT p) {
    if (!Hit(p)) return false;
    RECT t = Thumb();
    if (p.y >= t.top && p.y < t.bottom) { dragging = true; grabY = p.y; grabPos = pos; return true; }
    pos = std::clamp(pos + (p.y < t.top ? -1 : 1) * std::max(1, view * 9 / 10), 0, MaxPos());
    return true;
}

bool LiteScroll::Move(POINT p) {
    if (!dragging) return false;
    RECT t = Thumb();
    int free = (track.bottom - track.top) - (t.bottom - t.top);
    int np = free > 0 ? grabPos + (p.y - grabY) * MaxPos() / free : 0;
    np = std::clamp(np, 0, MaxPos());
    if (np == pos) return false;
    pos = np;
    return true;
}

void LiteScroll::Paint(Gdiplus::Graphics& g) const {
    if (!Visible()) return;
    if (hot || dragging) FillRound(g, track, (float)(track.right - track.left) / 2, RGB(226, 228, 233));
    RECT t = Thumb();
    if (!hot && !dragging) InflateRect(&t, -1, 0);
    FillRound(g, t, (float)(t.right - t.left) / 2, dragging ? RGB(120, 122, 130) : hot ? RGB(150, 152, 160) : RGB(188, 190, 196));
}

// ================================================================= barra de ferramentas
void Toolbar::Create(HWND parent) {
    static bool reg = false;
    if (!reg) {
        WNDCLASSW wc{}; wc.lpfnWndProc = Proc; wc.hInstance = GetModuleHandleW(nullptr); wc.hCursor = LoadCursor(nullptr, IDC_ARROW);
        wc.lpszClassName = L"SnapnabToolbar";
        RegisterClassW(&wc); reg = true;
    }
    hwnd = CreateWindowExW(0, L"SnapnabToolbar", nullptr, WS_CHILD | WS_VISIBLE, 0, 0, 10, 10, parent, nullptr, GetModuleHandleW(nullptr), this);
}

int Toolbar::Height() const { return Sc(64, hwnd); }

void Toolbar::Layout() {
    RECT cr; GetClientRect(hwnd, &cr);
    int pad = Sc(6, hwnd), gap = Sc(1, hwnd);
    int tw = Sc(46, hwnd), sw = Sc(12, hwnd);
    // botões de ação: largura pelo rótulo (no mínimo a de uma ferramenta)
    HDC dc = GetDC(hwnd);
    HGDIOBJ of = SelectObject(dc, ui::UiFont(DpiForWindow(hwnd), 8));
    auto widthOf = [&](const std::wstring& label, int minW) {
        RECT t{}; DrawTextW(dc, label.c_str(), -1, &t, DT_CALCRECT | DT_SINGLELINE);
        return std::max(minW, (int)t.right + Sc(18, hwnd));
    };
    slots_.clear();
    int x = Sc(10, hwnd);
    for (int i = 0; i < (int)items.size(); i++) {
        if (items[i].right) continue;
        int aw = widthOf(items[i].label, Sc(62, hwnd));
        slots_.push_back({ { x, pad, x + aw, cr.bottom - pad }, 0, i });
        x += aw + gap;
    }
    int leftEnd = x + Sc(12, hwnd);
    int xr = cr.right - Sc(10, hwnd);
    for (int i = (int)items.size() - 1; i >= 0; i--) {
        if (!items[i].right) continue;
        int aw = widthOf(items[i].label, Sc(62, hwnd));
        slots_.push_back({ { xr - aw, pad, xr, cr.bottom - pad }, 0, i });
        xr -= aw + gap;
    }
    int rightStart = xr - Sc(12, hwnd);
    // itens personalizáveis + "Mais", no meio da janela (centralizados no espaço livre se não couberem no centro)
    std::vector<int> widths;
    int total = 0, avail = rightStart - leftEnd;
    for (auto& e : center) {
        int w = e.type == ToolbarEntry::Spacer ? sw : tw;
        if (e.type != ToolbarEntry::Spacer && describe) w = std::max(tw, widthOf(describe(e).label, tw) - Sc(12, hwnd));
        widths.push_back(w); total += w + gap;
    }
    total += tw;
    if (total > avail && total > 0) {          // aperta tudo proporcionalmente
        double k = std::max(0.55, (double)avail / total);
        total = 0;
        for (auto& w : widths) { w = (int)(w * k); total += w + gap; }
        total += (int)(tw * k);
    }
    int cx = std::clamp((int)(cr.right - total) / 2, leftEnd, std::max(leftEnd, rightStart - total));
    for (int i = 0; i < (int)center.size(); i++) { slots_.push_back({ { cx, pad, cx + widths[i], cr.bottom - pad }, 1, i }); cx += widths[i] + gap; }
    int mw = total > avail ? (int)(tw * std::max(0.55, (double)avail / std::max(1, total))) : tw;
    slots_.push_back({ { cx, pad, cx + std::max(mw, Sc(34, hwnd)), cr.bottom - pad }, 2, 0 });
    SelectObject(dc, of);
    ReleaseDC(hwnd, dc);
}

int Toolbar::Hit(POINT p) const {
    for (size_t i = 0; i < slots_.size(); i++) if (PtInRect(&slots_[i].r, p)) return (int)i;
    return -1;
}

// posição de inserção entre os itens personalizáveis (pelo meio de cada item)
int Toolbar::InsertIndexAt(POINT p) const {
    RECT cr; GetClientRect(hwnd, &cr);
    if (p.y < 0 || p.y > cr.bottom) return -1;
    int first = INT_MAX, last = INT_MIN;
    for (auto& s : slots_) if (s.kind == 1 || s.kind == 2) { first = std::min(first, (int)s.r.left); last = std::max(last, (int)s.r.right); }
    if (first == INT_MAX || p.x < first - Sc(60, hwnd) || p.x > last + Sc(60, hwnd)) return -1;
    for (auto& s : slots_) if (s.kind == 1 && p.x < (s.r.left + s.r.right) / 2) return s.index;
    return (int)center.size();
}

int Toolbar::DropIndex(POINT screen) const {
    POINT p = screen;
    ScreenToClient(hwnd, &p);
    return InsertIndexAt(p);
}

void Toolbar::InsertAt(int index, const ToolbarEntry& e) {
    index = std::clamp(index, 0, (int)center.size());
    if (e.type != ToolbarEntry::Spacer) {
        auto it = std::find(center.begin(), center.end(), e);
        if (it != center.end()) {
            int from = (int)(it - center.begin());
            center.erase(it);
            if (from < index) index--;
        }
    }
    center.insert(center.begin() + index, e);
    Layout();
    InvalidateRect(hwnd, nullptr, FALSE);
    if (onReorder) onReorder(center);
}

void Toolbar::Paint(HDC hdc) {
    const auto& th = EdTheme();
    RECT cr; GetClientRect(hwnd, &cr);
    HDC mem = CreateCompatibleDC(hdc);
    HBITMAP bb = CreateCompatibleBitmap(hdc, std::max(1L, cr.right), std::max(1L, cr.bottom));
    HGDIOBJ ob = SelectObject(mem, bb);
    HBRUSH bg = CreateSolidBrush(th.bgMain); FillRect(mem, &cr, bg); DeleteObject(bg);
    RECT line{ 0, cr.bottom - 1, cr.right, cr.bottom };
    HBRUSH lb = CreateSolidBrush(th.line); FillRect(mem, &line, lb); DeleteObject(lb);
    int dpi = DpiForWindow(hwnd);
    bool activeVisible = std::find(center.begin(), center.end(), ToolbarEntry{ ToolbarEntry::Tool, (int)active }) != center.end();
    {
        Gdiplus::Graphics g(mem);
        g.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
        g.SetPixelOffsetMode(Gdiplus::PixelOffsetModeHalf);
        int caretX = -1;
        if (caret_ >= 0) {
            for (auto& s : slots_) if (s.kind == 1 && s.index == caret_) caretX = s.r.left - 1;
            if (caretX < 0) for (auto& s : slots_) if (s.kind == 2) caretX = s.r.left - 1;
        }
        if (customizing) {             // área personalizável destacada enquanto a janela Personalizar está aberta
            int a = INT_MAX, b = INT_MIN;
            for (auto& s : slots_) if (s.kind == 1 || s.kind == 2) { a = std::min(a, (int)s.r.left); b = std::max(b, (int)s.r.right); }
            if (a < b) {
                RECT ar{ a - ui::Scale(6, dpi), ui::Scale(3, dpi), b + ui::Scale(6, dpi), cr.bottom - ui::Scale(3, dpi) };
                Gdiplus::Pen dp(GC(th.control), 1.2f); dp.SetDashStyle(Gdiplus::DashStyleDash);
                Gdiplus::GraphicsPath p; RoundPath(p, Gdiplus::RectF((float)ar.left, (float)ar.top, (float)(ar.right - ar.left), (float)(ar.bottom - ar.top)), (float)ui::Scale(8, dpi));
                g.DrawPath(&dp, &p);
            }
        }
        for (int i = 0; i < (int)slots_.size(); i++) {
            const Slot& s = slots_[i];
            RECT r = s.r;
            if (s.kind == 1 && center[s.index].type == ToolbarEntry::Spacer) {     // espaçador: fio vertical
                Gdiplus::Pen p(GC(th.line), 1.0f);
                float x = (r.left + r.right) / 2.0f;
                g.DrawLine(&p, x, (float)r.top + ui::Scale(10, dpi), x, (float)r.bottom - ui::Scale(10, dpi));
                if (dragging_ && i == dragFrom_) FillRound(g, r, (float)ui::Scale(4, dpi), Mix(th.bgMain, th.control, 0.10));
                continue;
            }
            bool hot = i == hover_ && !dragging_, down = i == pressed_ && hot;
            bool primary = s.kind == 0 && items[s.index].primary;
            bool act = !customizing && ((s.kind == 1 && center[s.index].type == ToolbarEntry::Tool && center[s.index].id == (int)active) ||
                                        (s.kind == 2 && !activeVisible));
            float rad = (float)ui::Scale(8, dpi);
            COLORREF fg = RGB(56, 56, 62);
            if (primary) {
                COLORREF c = down ? Mix(ui::kCaptureRed, 0, 0.16) : hot ? Mix(ui::kCaptureRed, RGB(255, 255, 255), 0.10) : ui::kCaptureRed;
                FillRound(g, r, rad, c);
                fg = RGB(255, 255, 255);
            } else if (act) {
                FillRound(g, r, rad, Mix(th.bgMain, th.control, down ? 0.24 : 0.16));
                fg = th.accentText;
            } else if (hot) {
                FillRound(g, r, rad, Mix(th.bgMain, th.control, down ? 0.16 : 0.08));
            }
            if (dragging_ && s.kind == 1 && i == dragFrom_) { FillRound(g, r, rad, Mix(th.bgMain, th.control, 0.10)); fg = Mix(fg, th.bgMain, dragOut_ ? 0.7 : 0.4); }
            int isz = ui::Scale(22, dpi);
            int ix = (r.left + r.right - isz) / 2, iy = r.top + ui::Scale(6, dpi);
            Gdiplus::RectF ir((float)ix, (float)iy, (float)isz, (float)isz);
            std::wstring label;
            if (s.kind == 0) { icons::Draw(g, items[s.index].icon, ir, GC(fg)); label = items[s.index].label; }
            else if (s.kind == 1) {
                ToolbarEntryInfo in = describe ? describe(center[s.index]) : ToolbarEntryInfo{};
                icons::Draw(g, in.icon, ir, GC(fg));
                label = in.label;
            } else {                                   // "Mais": três pontos
                Gdiplus::SolidBrush b(GC(fg));
                float d = (float)ui::Scale(4, dpi), cy = ir.Y + ir.Height / 2 - d / 2;
                for (int k = -1; k <= 1; k++) g.FillEllipse(&b, ir.X + ir.Width / 2 - d / 2 + k * d * 1.9f, cy, d, d);
                label = Tr(L"Mais");
            }
            RECT tr{ r.left - 2, r.bottom - ui::Scale(20, dpi), r.right + 2, r.bottom - ui::Scale(3, dpi) };
            ui::DrawTextCentered(mem, label, tr, fg, ui::UiFont(dpi, 8));
        }
        if (caretX >= 0) {             // onde o item arrastado vai entrar
            Gdiplus::SolidBrush cb(GC(th.control));
            g.FillRectangle(&cb, (float)caretX - ui::Scale(1, dpi), (float)ui::Scale(8, dpi), (float)ui::Scale(3, dpi), (float)cr.bottom - ui::Scale(16, dpi));
        }
    }
    BitBlt(hdc, 0, 0, cr.right, cr.bottom, mem, 0, 0, SRCCOPY);
    SelectObject(mem, ob); DeleteObject(bb); DeleteDC(mem);
}

LRESULT CALLBACK Toolbar::Proc(HWND h, UINT m, WPARAM w, LPARAM l) {
    Toolbar* self = (Toolbar*)GetWindowLongPtrW(h, GWLP_USERDATA);
    if (m == WM_NCCREATE) { self = (Toolbar*)((CREATESTRUCTW*)l)->lpCreateParams; SetWindowLongPtrW(h, GWLP_USERDATA, (LONG_PTR)self); self->hwnd = h; }
    if (!self) return DefWindowProcW(h, m, w, l);
    switch (m) {
    case WM_SIZE: self->Layout(); InvalidateRect(h, nullptr, FALSE); return 0;
    case WM_ERASEBKGND: return 1;
    case WM_PAINT: { PAINTSTRUCT ps; HDC dc = BeginPaint(h, &ps); self->Paint(dc); EndPaint(h, &ps); return 0; }
    case WM_MOUSEMOVE: {
        POINT p{ GET_X_LPARAM(l), GET_Y_LPARAM(l) };
        int hv = self->Hit(p);
        if (self->pressed_ >= 0 && self->slots_[self->pressed_].kind == 1 && !self->dragging_ &&
            std::abs(p.x - self->downPt_.x) + std::abs(p.y - self->downPt_.y) > Sc(8, h)) {
            self->dragging_ = true; self->dragFrom_ = self->pressed_;
        }
        if (self->dragging_) {
            int ins = self->InsertIndexAt(p);
            self->dragOut_ = self->customizing && ins < 0;
            self->ShowDropCaret(ins);
            InvalidateRect(h, nullptr, FALSE);
            SetCursor(LoadCursor(nullptr, self->dragOut_ ? IDC_NO : IDC_SIZEALL));
            return 0;
        }
        if (hv != self->hover_) {
            self->hover_ = hv; InvalidateRect(h, nullptr, FALSE);
            if (self->onHint) {
                std::wstring hint;
                if (hv >= 0) {
                    const Slot& s = self->slots_[hv];
                    if (s.kind == 0) hint = self->items[s.index].hint;
                    else if (s.kind == 1 && self->describe) hint = self->describe(self->center[s.index]).hint + Tr(L"  •  arraste para mudar de lugar");
                    else if (s.kind == 2) hint = Tr(L"Mais ferramentas e personalização da barra");
                }
                self->onHint(hint);
            }
        }
        TRACKMOUSEEVENT t{ sizeof(t), TME_LEAVE, h, 0 }; TrackMouseEvent(&t);
        return 0;
    }
    case WM_MOUSELEAVE: if (!self->dragging_) { self->hover_ = -1; InvalidateRect(h, nullptr, FALSE); if (self->onHint) self->onHint(L""); } return 0;
    case WM_LBUTTONDOWN:
        self->pressed_ = self->Hit({ GET_X_LPARAM(l), GET_Y_LPARAM(l) });
        self->downPt_ = { GET_X_LPARAM(l), GET_Y_LPARAM(l) };
        SetCapture(h); InvalidateRect(h, nullptr, FALSE);
        return 0;
    case WM_LBUTTONUP: {
        POINT pt{ GET_X_LPARAM(l), GET_Y_LPARAM(l) };
        int hit = self->Hit(pt);
        int p = self->pressed_;
        self->pressed_ = -1;
        bool wasDragging = self->dragging_, out = self->dragOut_;
        int dragSlot = self->dragFrom_;
        self->dragging_ = self->dragOut_ = false; self->dragFrom_ = -1;
        ReleaseCapture();                              // (WM_CAPTURECHANGED já não cancela: o arraste foi encerrado acima)
        if (wasDragging && dragSlot >= 0) {
            int from = self->slots_[dragSlot].index, to = self->InsertIndexAt(pt);
            self->ShowDropCaret(-1);
            InvalidateRect(h, nullptr, FALSE);
            if (out) {                                 // arrastado para fora durante a personalização: sai da barra
                self->center.erase(self->center.begin() + from);
                self->Layout();
                if (self->onReorder) self->onReorder(self->center);
            } else if (to >= 0 && to != from && to != from + 1) {
                ToolbarEntry e = self->center[from];
                self->center.erase(self->center.begin() + from);
                if (from < to) to--;
                self->center.insert(self->center.begin() + to, e);
                self->Layout();
                if (self->onReorder) self->onReorder(self->center);
            }
            return 0;
        }
        InvalidateRect(h, nullptr, FALSE);
        if (hit < 0 || hit != p || self->customizing) return 0;
        const Slot s = self->slots_[hit];
        if (s.kind == 0 && self->onClick) self->onClick(self->items[s.index].id);
        else if (s.kind == 1 && self->onEntry && self->center[s.index].type != ToolbarEntry::Spacer) self->onEntry(self->center[s.index]);
        else if (s.kind == 2 && self->onMore) {
            POINT mp{ s.r.left, s.r.bottom };
            ClientToScreen(h, &mp);
            self->onMore(mp);
        }
        return 0;
    }
    case WM_RBUTTONUP:           // clique direito na barra: personalizar (como o menu da barra BCG)
        if (self->onMore && !self->customizing) { POINT pt; GetCursorPos(&pt); self->onMore(pt); }
        return 0;
    case WM_CAPTURECHANGED:
        if (self->dragging_ && (HWND)l != h) { self->dragging_ = self->dragOut_ = false; self->dragFrom_ = -1; self->ShowDropCaret(-1); InvalidateRect(h, nullptr, FALSE); }
        return 0;
    }
    return DefWindowProcW(h, m, w, l);
}

// ================================================================= personalizar a barra
// Grade de itens por grupo; arrastar um item até a barra o insere ali (a barra mostra onde). Enquanto a janela está
// aberta, a barra fica em modo de personalização: arrastar um item dela para fora o remove.
namespace {
struct CustomizeState {
    struct Cell { RECT r; ToolbarEntry e; };
    struct Group { std::wstring name; int y; };
    std::vector<Cell> cells;
    std::vector<Group> groups;
    std::function<ToolbarEntryInfo(const ToolbarEntry&)> describe;
    std::function<std::vector<ToolbarEntry>()> defaults;
    std::function<void(int)> onToolCount;
    Toolbar* bar = nullptr;
    HWND h = nullptr, drag = nullptr, countCombo = nullptr;
    HBRUSH footBrush = nullptr;
    RECT countR{};
    int scroll = 0, contentH = 0, hot = -1, down = -1;
    POINT downPt{};
    bool dragging = false, done = false, ok = false;
    RECT viewR{}, helpR{}, restoreR{}, doneR{}, cancelR{};
    int hotBtn = -1;
    LiteScroll sb;
};
CustomizeState* g_cust = nullptr;

void CustomizeLayout(CustomizeState& s, const std::vector<std::pair<std::wstring, std::vector<ToolbarEntry>>>& catalog) {
    int dpi = DpiForWindow(s.h);
    RECT cr; GetClientRect(s.h, &cr);
    int pad = ui::Scale(20, dpi), cw = ui::Scale(92, dpi), ch = ui::Scale(60, dpi);
    s.viewR = { 0, ui::Scale(40, dpi), cr.right, cr.bottom - ui::Scale(100, dpi) };
    s.countR = { pad, s.viewR.bottom + ui::Scale(8, dpi), cr.right - pad, s.viewR.bottom + ui::Scale(40, dpi) };
    int cols = std::max(1, (int)(cr.right - 2 * pad) / cw);
    s.cells.clear(); s.groups.clear();
    int y = ui::Scale(8, dpi);
    for (auto& [name, entries] : catalog) {
        s.groups.push_back({ name, y });
        y += ui::Scale(30, dpi);
        for (size_t i = 0; i < entries.size(); i++) {
            int c = (int)i % cols, r = (int)i / cols;
            s.cells.push_back({ { pad + c * cw, y + r * ch, pad + (c + 1) * cw, y + (r + 1) * ch }, entries[i] });
        }
        y += (int)((entries.size() + cols - 1) / cols) * ch + ui::Scale(14, dpi);
    }
    s.contentH = y;
    int by = cr.bottom - ui::Scale(44, dpi), bh = ui::Scale(32, dpi);
    s.helpR = { pad, by, pad + bh, by + bh };
    s.restoreR = { pad + bh + ui::Scale(10, dpi), by, pad + bh + ui::Scale(160, dpi), by + bh };
    s.cancelR = { cr.right - pad - ui::Scale(100, dpi), by, cr.right - pad, by + bh };
    s.doneR = { s.cancelR.left - ui::Scale(110, dpi), by, s.cancelR.left - ui::Scale(10, dpi), by + bh };
}

int CustomizeHit(CustomizeState& s, POINT p) {
    if (!PtInRect(&s.viewR, p)) return -1;
    POINT q{ p.x, p.y - s.viewR.top + s.scroll };
    for (size_t i = 0; i < s.cells.size(); i++) if (PtInRect(&s.cells[i].r, q)) return (int)i;
    return -1;
}

// janelinha que segue o cursor com o ícone do item arrastado
HWND MakeDragImage(HWND owner, const ToolbarEntryInfo& in, int dpi) {
    int w = ui::Scale(64, dpi), hh = ui::Scale(52, dpi);
    Image img(w, hh, 0);
    {
        auto bmp = img.WrapGdip();
        Gdiplus::Graphics g(bmp.get());
        g.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
        Gdiplus::GraphicsPath p; RoundPath(p, Gdiplus::RectF(1, 1, (float)w - 2, (float)hh - 2), (float)ui::Scale(8, dpi));
        Gdiplus::SolidBrush b(Gdiplus::Color(235, 255, 255, 255)); g.FillPath(&b, &p);
        Gdiplus::Pen pen(GC(EdTheme().control), 1.5f); g.DrawPath(&pen, &p);
        int is = ui::Scale(24, dpi);
        icons::Draw(g, in.icon, Gdiplus::RectF((float)(w - is) / 2, (float)ui::Scale(6, dpi), (float)is, (float)is), Gdiplus::Color(255, 50, 50, 56));
        Gdiplus::FontFamily ff(L"Segoe UI"); Gdiplus::Font f(&ff, (float)ui::Scale(9, dpi), Gdiplus::FontStyleRegular, Gdiplus::UnitPixel);
        Gdiplus::StringFormat sf; sf.SetAlignment(Gdiplus::StringAlignmentCenter); sf.SetTrimming(Gdiplus::StringTrimmingEllipsisCharacter);
        sf.SetFormatFlags(Gdiplus::StringFormatFlagsNoWrap);
        Gdiplus::SolidBrush tb(Gdiplus::Color(255, 50, 50, 56));
        g.DrawString(in.label.c_str(), -1, &f, Gdiplus::RectF(2, (float)ui::Scale(32, dpi), (float)w - 4, (float)ui::Scale(16, dpi)), &sf, &tb);
        g.Flush(Gdiplus::FlushIntentionSync);
    }
    HWND d = CreateWindowExW(WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE, L"STATIC", nullptr, WS_POPUP,
                             0, 0, w, hh, owner, nullptr, GetModuleHandleW(nullptr), nullptr);
    HDC sdc = GetDC(nullptr);
    HDC mem = CreateCompatibleDC(sdc);
    // premultiplica para o UpdateLayeredWindow
    Image pm = img;
    for (auto& px : pm.px) { int a = CA(px); px = ARGB(a, CR(px) * a / 255, CG(px) * a / 255, CB(px) * a / 255); }
    HBITMAP hb = pm.ToHBITMAP();
    HGDIOBJ ob = SelectObject(mem, hb);
    POINT src{ 0, 0 }; SIZE sz{ w, hh };
    BLENDFUNCTION bf{ AC_SRC_OVER, 0, 255, AC_SRC_ALPHA };
    UpdateLayeredWindow(d, sdc, nullptr, &sz, mem, &src, 0, &bf, ULW_ALPHA);
    SelectObject(mem, ob); DeleteObject(hb); DeleteDC(mem); ReleaseDC(nullptr, sdc);
    return d;
}

LRESULT CALLBACK CustomizeProc(HWND h, UINT m, WPARAM w, LPARAM l) {
    CustomizeState* s = g_cust;
    if (!s) return DefWindowProcW(h, m, w, l);
    int dpi = DpiForWindow(h);
    const auto& th = EdTheme();
    switch (m) {
    case WM_ERASEBKGND: return 1;
    case WM_PAINT: {
        PAINTSTRUCT ps; HDC dc = BeginPaint(h, &ps);
        RECT cr; GetClientRect(h, &cr);
        HDC mem = CreateCompatibleDC(dc);
        HBITMAP bb = CreateCompatibleBitmap(dc, cr.right, cr.bottom);
        HGDIOBJ ob = SelectObject(mem, bb);
        HBRUSH bgb = CreateSolidBrush(th.bgMain); FillRect(mem, &cr, bgb); DeleteObject(bgb);
        SetBkMode(mem, TRANSPARENT);
        RECT tr{ ui::Scale(20, dpi), 0, cr.right - ui::Scale(20, dpi), ui::Scale(40, dpi) };
        HGDIOBJ of = SelectObject(mem, ui::UiFont(dpi, 10));
        SetTextColor(mem, RGB(40, 40, 46));
        DrawTextW(mem, Tr(L"Arraste suas ferramentas, efeitos e comandos favoritos para a barra…"), -1, &tr, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
        {
            Gdiplus::Graphics g(mem);
            g.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
            g.SetPixelOffsetMode(Gdiplus::PixelOffsetModeHalf);
            RECT v = s->viewR;
            HRGN clip = CreateRectRgn(v.left, v.top, v.right, v.bottom);
            SelectClipRgn(mem, clip);
            Gdiplus::Region rg(clip); g.SetClip(&rg);
            int oy = v.top - s->scroll;
            for (auto& gr : s->groups) {
                RECT hr{ ui::Scale(20, dpi), oy + gr.y, cr.right - ui::Scale(20, dpi), oy + gr.y + ui::Scale(24, dpi) };
                SelectObject(mem, ui::UiFont(dpi, 10, true));
                SetTextColor(mem, RGB(40, 40, 46));
                DrawTextW(mem, gr.name.c_str(), -1, &hr, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
                RECT ln{ hr.left, hr.bottom, hr.right, hr.bottom + 1 };
                HBRUSH lb = CreateSolidBrush(RGB(60, 60, 66)); FillRect(mem, &ln, lb); DeleteObject(lb);
            }
            for (size_t i = 0; i < s->cells.size(); i++) {
                RECT r = s->cells[i].r; OffsetRect(&r, 0, oy);
                if (r.bottom < v.top || r.top > v.bottom) continue;
                const ToolbarEntry& e = s->cells[i].e;
                ToolbarEntryInfo in = e.type == ToolbarEntry::Spacer ? ToolbarEntryInfo{ icons::Rect, Tr(L"Espaçador"), L"" } : s->describe(e);
                bool onBar = e.type != ToolbarEntry::Spacer && std::find(s->bar->center.begin(), s->bar->center.end(), e) != s->bar->center.end();
                if ((int)i == s->hot) FillRound(g, r, (float)ui::Scale(6, dpi), Mix(th.bgMain, th.control, 0.10));
                int is = ui::Scale(24, dpi);
                COLORREF fg = onBar ? th.accentText : RGB(56, 56, 62);
                icons::Draw(g, in.icon, Gdiplus::RectF((float)(r.left + r.right - is) / 2, (float)r.top + ui::Scale(6, dpi), (float)is, (float)is), GC(fg));
                RECT lr{ r.left, r.top + ui::Scale(34, dpi), r.right, r.bottom - ui::Scale(4, dpi) };
                SelectObject(mem, ui::UiFont(dpi, 8));
                SetTextColor(mem, fg);
                DrawTextW(mem, in.label.c_str(), -1, &lr, DT_CENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
            }
            g.ResetClip();
            SelectClipRgn(mem, nullptr);
            DeleteObject(clip);
            s->sb.track = { v.right - ui::Scale(12, dpi), v.top + ui::Scale(4, dpi), v.right - ui::Scale(4, dpi), v.bottom - ui::Scale(4, dpi) };
            s->sb.content = s->contentH; s->sb.view = v.bottom - v.top; s->sb.pos = s->scroll;
            s->sb.Paint(g);
            RECT foot{ 0, v.bottom, cr.right, cr.bottom };
            HBRUSH fb = CreateSolidBrush(th.bgBar); FillRect(mem, &foot, fb); DeleteObject(fb);
            RECT fl{ 0, v.bottom, cr.right, v.bottom + 1 };
            HBRUSH flb = CreateSolidBrush(th.line); FillRect(mem, &fl, flb); DeleteObject(flb);
            SelectObject(mem, ui::UiFont(dpi, 9));
            SetTextColor(mem, RGB(40, 40, 46));
            RECT cl = s->countR; cl.right -= ui::Scale(120, dpi);
            DrawTextW(mem, Tr(L"Número de ferramentas na barra automática:"), -1, &cl, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
            auto button = [&](RECT r, const wchar_t* t, bool accent, int idx) {
                bool hot = s->hotBtn == idx;
                FillRound(g, r, (float)ui::Scale(6, dpi), accent ? (hot ? Mix(th.control, 0, 0.12) : th.control) : (hot ? RGB(238, 239, 243) : RGB(255, 255, 255)));
                if (!accent) StrokeRound(g, r, (float)ui::Scale(6, dpi), Mix(th.line, 0, 0.1), 1.0f);
                ui::DrawTextCentered(mem, t, r, accent ? RGB(255, 255, 255) : RGB(40, 40, 46), ui::UiFont(dpi, 9));
            };
            button(s->helpR, L"?", false, 0);
            button(s->restoreR, Tr(L"Restaurar padrões"), false, 1);
            button(s->doneR, Tr(L"Concluído"), true, 2);
            button(s->cancelR, Tr(L"Cancelar"), false, 3);
        }
        SelectObject(mem, of);
        BitBlt(dc, 0, 0, cr.right, cr.bottom, mem, 0, 0, SRCCOPY);
        SelectObject(mem, ob); DeleteObject(bb); DeleteDC(mem);
        EndPaint(h, &ps);
        return 0;
    }
    case WM_MOUSEWHEEL: {
        int d = -GET_WHEEL_DELTA_WPARAM(w) * ui::Scale(60, dpi) / WHEEL_DELTA;
        int vh = s->viewR.bottom - s->viewR.top;
        s->scroll = std::clamp(s->scroll + d, 0, std::max(0, s->contentH - vh));
        InvalidateRect(h, nullptr, FALSE);
        return 0;
    }
    case WM_MOUSEMOVE: {
        POINT p{ GET_X_LPARAM(l), GET_Y_LPARAM(l) };
        if (s->sb.dragging) { if (s->sb.Move(p)) { s->scroll = s->sb.pos; InvalidateRect(h, nullptr, FALSE); } return 0; }
        bool sbHot = s->sb.Hit(p);
        if (sbHot != s->sb.hot) { s->sb.hot = sbHot; InvalidateRect(h, nullptr, FALSE); }
        if (s->down >= 0 && !s->dragging && std::abs(p.x - s->downPt.x) + std::abs(p.y - s->downPt.y) > ui::Scale(6, dpi)) {
            s->dragging = true;
            const ToolbarEntry& e = s->cells[s->down].e;
            s->drag = MakeDragImage(h, e.type == ToolbarEntry::Spacer ? ToolbarEntryInfo{ icons::Rect, Tr(L"Espaçador"), L"" } : s->describe(e), dpi);
        }
        if (s->dragging) {
            POINT sp = p; ClientToScreen(h, &sp);
            RECT dr; GetWindowRect(s->drag, &dr);
            SetWindowPos(s->drag, HWND_TOPMOST, sp.x - (dr.right - dr.left) / 2, sp.y - ui::Scale(14, dpi), 0, 0, SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
            int idx = s->bar->DropIndex(sp);
            s->bar->ShowDropCaret(idx);
            SetCursor(LoadCursor(nullptr, idx >= 0 ? IDC_ARROW : IDC_NO));
            return 0;
        }
        int hv = CustomizeHit(*s, p), hb = -1;
        RECT* btns[4] = { &s->helpR, &s->restoreR, &s->doneR, &s->cancelR };
        for (int i = 0; i < 4; i++) if (PtInRect(btns[i], p)) hb = i;
        if (hv != s->hot || hb != s->hotBtn) { s->hot = hv; s->hotBtn = hb; InvalidateRect(h, nullptr, FALSE); }
        TRACKMOUSEEVENT t{ sizeof(t), TME_LEAVE, h, 0 }; TrackMouseEvent(&t);
        return 0;
    }
    case WM_MOUSELEAVE: if (!s->dragging) { s->hot = s->hotBtn = -1; s->sb.hot = false; InvalidateRect(h, nullptr, FALSE); } return 0;
    case WM_LBUTTONDOWN: {
        POINT p{ GET_X_LPARAM(l), GET_Y_LPARAM(l) };
        if (s->sb.Down(p)) { s->scroll = s->sb.pos; if (s->sb.dragging) SetCapture(h); InvalidateRect(h, nullptr, FALSE); return 0; }
        s->down = CustomizeHit(*s, p);
        s->downPt = p;
        if (s->down >= 0) SetCapture(h);
        return 0;
    }
    case WM_LBUTTONUP: {
        POINT p{ GET_X_LPARAM(l), GET_Y_LPARAM(l) };
        if (s->sb.dragging) { s->sb.Up(); if (GetCapture() == h) ReleaseCapture(); InvalidateRect(h, nullptr, FALSE); return 0; }
        bool wasDragging = s->dragging;
        int down = s->down;
        s->dragging = false;                            // antes do ReleaseCapture (o WM_CAPTURECHANGED cancelaria o arraste)
        if (GetCapture() == h) ReleaseCapture();
        if (wasDragging) {
            POINT sp = p; ClientToScreen(h, &sp);
            int idx = s->bar->DropIndex(sp);
            if (idx >= 0 && down >= 0) s->bar->InsertAt(idx, s->cells[down].e);
            s->bar->ShowDropCaret(-1);
            if (s->drag) { DestroyWindow(s->drag); s->drag = nullptr; }
            s->down = -1;
            InvalidateRect(h, nullptr, FALSE);
            return 0;
        }
        s->down = -1;
        if (PtInRect(&s->helpR, p))
            MessageBoxW(h, Tr(L"Arraste um item desta janela para a barra de ferramentas para acrescentá-lo; a barra mostra onde ele vai entrar.\n\n"
                           L"Na barra, arraste um item para mudar de lugar, ou para fora da barra para removê-lo.\n"
                           L"“Restaurar padrões” volta à barra automática: Mover, Seleção, Texto, Marcador, Forma, as usadas por último e Lupa. "
                           L"O número de ferramentas dela é escolhido embaixo (de 6 a 16)."),
                        Tr(L"Personalizar a barra de ferramentas"), MB_ICONINFORMATION);
        else if (PtInRect(&s->restoreR, p)) { if (s->defaults) { s->bar->SetCenter(s->defaults()); if (s->bar->onReorder) s->bar->onReorder(s->bar->center); } InvalidateRect(h, nullptr, FALSE); }
        else if (PtInRect(&s->doneR, p)) { s->ok = true; s->done = true; }
        else if (PtInRect(&s->cancelR, p)) s->done = true;
        return 0;
    }
    case WM_CAPTURECHANGED:
        s->sb.Up();
        if (s->dragging && (HWND)l != h) { DestroyWindow(s->drag); s->drag = nullptr; s->dragging = false; s->down = -1; s->bar->ShowDropCaret(-1); }
        return 0;
    case WM_CTLCOLORBTN: case WM_CTLCOLORSTATIC: SetBkMode((HDC)w, TRANSPARENT); return (LRESULT)s->footBrush;
    case WM_COMMAND:
        if ((HWND)l == s->countCombo && HIWORD(w) == CBN_SELCHANGE && s->onToolCount) {
            s->onToolCount(6 + std::max(0, ComboBox_GetCurSel(s->countCombo)));
            InvalidateRect(h, nullptr, FALSE);
        }
        return 0;
    case WM_CLOSE: s->done = true; return 0;
    }
    return DefWindowProcW(h, m, w, l);
}
} // namespace

bool CustomizeToolbarDialog(HWND owner, Toolbar& bar, const std::vector<std::pair<std::wstring, std::vector<ToolbarEntry>>>& catalog,
                            std::function<ToolbarEntryInfo(const ToolbarEntry&)> describe, std::function<std::vector<ToolbarEntry>()> defaults,
                            int toolCount, std::function<void(int)> onToolCount) {
    static bool reg = false;
    HINSTANCE hi = GetModuleHandleW(nullptr);
    if (!reg) {
        WNDCLASSW wc{}; wc.lpfnWndProc = CustomizeProc; wc.hInstance = hi; wc.hCursor = LoadCursor(nullptr, IDC_ARROW);
        wc.lpszClassName = L"SnapnabTbCustomize";
        RegisterClassW(&wc); reg = true;
    }
    CustomizeState st;
    st.describe = describe; st.defaults = defaults; st.bar = &bar; st.onToolCount = onToolCount;
    st.footBrush = CreateSolidBrush(EdTheme().bgBar);
    g_cust = &st;
    int dpi = DpiForWindow(owner);
    int W = ui::Scale(600, dpi), H = ui::Scale(520, dpi);
    RECT orc; GetWindowRect(owner, &orc);
    RECT wr{ 0, 0, W, H };
    AdjustWindowRectExForDpi(&wr, WS_CAPTION | WS_SYSMENU, FALSE, WS_EX_TOOLWINDOW, dpi);
    int ww = wr.right - wr.left, wh = wr.bottom - wr.top;
    // fica embaixo da barra, no meio da janela, para dar espaço de arrastar até ela
    RECT tb; GetWindowRect(bar.hwnd, &tb);
    st.h = CreateWindowExW(WS_EX_TOOLWINDOW, L"SnapnabTbCustomize", Tr(L"Personalizar a barra de ferramentas"), WS_CAPTION | WS_SYSMENU | WS_POPUP,
                           (orc.left + orc.right - ww) / 2, tb.bottom + ui::Scale(40, dpi), ww, wh, owner, nullptr, hi, nullptr);
    CustomizeLayout(st, catalog);
    st.countCombo = CreateWindowExW(0, mui::kCombo, nullptr, WS_CHILD | WS_VISIBLE | WS_TABSTOP, st.countR.right - ui::Scale(80, dpi),
                                    st.countR.top + ui::Scale(1, dpi), ui::Scale(80, dpi), ui::Scale(30, dpi), st.h, nullptr, hi, nullptr);
    SendMessageW(st.countCombo, WM_SETFONT, (WPARAM)ui::UiFont(dpi), TRUE);
    for (int n = 6; n <= 16; n++) ComboBox_AddString(st.countCombo, std::to_wstring(n).c_str());
    ComboBox_SetCurSel(st.countCombo, std::clamp(toolCount, 6, 16) - 6);
    bar.customizing = true;
    InvalidateRect(bar.hwnd, nullptr, FALSE);
    ShowWindow(st.h, SW_SHOW);
    MSG msg;
    while (!st.done && GetMessageW(&msg, nullptr, 0, 0)) {
        if (msg.message == WM_KEYDOWN && msg.wParam == VK_ESCAPE) { st.done = true; continue; }
        if (msg.message == WM_QUIT) { PostQuitMessage((int)msg.wParam); break; }
        TranslateMessage(&msg); DispatchMessageW(&msg);
    }
    bar.customizing = false;
    InvalidateRect(bar.hwnd, nullptr, FALSE);
    if (st.drag) DestroyWindow(st.drag);
    DestroyWindow(st.h);
    DeleteObject(st.footBrush);
    SetForegroundWindow(owner);
    g_cust = nullptr;
    return st.ok;
}

// ================================================================= efeitos
namespace { enum { FX_APPLY = 90, FX_CANCEL = 91, FX_PARAM = 200 }; constexpr UINT_PTR kFxTextTimer = 1; }

void EffectsPanel::Create(HWND parent) {
    static bool reg = false;
    if (!reg) {
        WNDCLASSW wc{}; wc.lpfnWndProc = Proc; wc.hInstance = GetModuleHandleW(nullptr); wc.hCursor = LoadCursor(nullptr, IDC_ARROW);
        wc.lpszClassName = L"SnapnabEffects";
        RegisterClassW(&wc); reg = true;
    }
    bg_ = CreateSolidBrush(EdTheme().bgMain);
    hwnd = CreateWindowExW(0, L"SnapnabEffects", nullptr, WS_CHILD | WS_VSCROLL | WS_CLIPCHILDREN, 0, 0, 10, 10, parent, nullptr,
                           GetModuleHandleW(nullptr), this);
}

// imagem de exemplo quando não há documento aberto: janela estilizada com faixa colorida e texto
static Image SampleImage() {
    Image img(240, 160, 0xFFFFFFFF);
    auto bmp = img.WrapGdip();
    Gdiplus::Graphics g(bmp.get());
    g.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
    Gdiplus::LinearGradientBrush lg(Gdiplus::PointF(0, 0), Gdiplus::PointF(240, 160), Gdiplus::Color(255, 70, 140, 230), Gdiplus::Color(255, 150, 90, 220));
    g.FillRectangle(&lg, 0, 0, 240, 56);
    Gdiplus::SolidBrush yb(Gdiplus::Color(255, 255, 196, 60)), gb(Gdiplus::Color(255, 60, 180, 120)), tb(Gdiplus::Color(255, 60, 60, 70));
    g.FillEllipse(&yb, 22, 78, 54, 54);
    g.FillRectangle(&gb, 96, 84, 120, 14);
    g.FillRectangle(&tb, 96, 108, 96, 10);
    g.FillRectangle(&tb, 96, 126, 70, 10);
    Gdiplus::FontFamily ff(L"Segoe UI");
    Gdiplus::Font f(&ff, 26, Gdiplus::FontStyleBold, Gdiplus::UnitPixel);
    Gdiplus::SolidBrush wb(Gdiplus::Color(255, 255, 255, 255));
    g.DrawString(L"Aa", -1, &f, Gdiplus::PointF(14, 12), &wb);
    return img;
}

const EffectsPanel::Effect* EffectsPanel::Find(int cmd) const {
    for (auto& e : effects) if (e.cmd == cmd) return &e;
    return nullptr;
}

void EffectsPanel::SetSource(const Image& img) {
    src_ = img.empty() ? SampleImage() : MakeThumbnail(img, 240, 160);
    BuildPreviews();
    InvalidateRect(hwnd, nullptr, FALSE);
}

void EffectsPanel::SetQuick(std::vector<std::wstring> names) {
    quick = std::move(names);
    BuildPreviews();
    Relayout();
}

void EffectsPanel::BuildPreviews() {
    previews_.clear(); quickPreviews_.clear();
    if (src_.empty()) return;
    for (auto& e : effects) previews_.push_back(makePreview ? makePreview(e.cmd, e.params, src_) : src_);
    for (int i = 0; i < (int)quick.size(); i++) quickPreviews_.push_back(makePreview ? makePreview(-i - 1, {}, src_) : src_);
}

int EffectsPanel::SettingsHeight() const {
    if (open_ < 0) return 0;
    return Sc(10, hwnd) + (int)effects[open_].params.size() * Sc(36, hwnd) + Sc(46, hwnd);
}

void EffectsPanel::Relayout() {
    if (!hwnd) return;
    RECT cr; GetClientRect(hwnd, &cr);
    int pad = Sc(10, hwnd), W = cr.right;
    int y = Sc(8, hwnd) + Sc(26, hwnd);
    // galeria de efeitos rápidos: 3 colunas; a última peça é "Adicionar"
    int cols = 3, gap = Sc(8, hwnd);
    int tw = std::max(Sc(40, hwnd), (W - 2 * pad - gap * (cols - 1)) / cols), th = tw * 3 / 4 + Sc(22, hwnd);
    tiles_.clear();
    for (int i = 0; i <= (int)quick.size(); i++) {
        int c = i % cols, r = i / cols;
        tiles_.push_back({ pad + c * (tw + gap), y + r * (th + gap), pad + c * (tw + gap) + tw, y + r * (th + gap) + th });
    }
    y = tiles_.back().bottom + Sc(14, hwnd);
    // efeitos, com o cabeçalho do grupo quando ele muda e os ajustes embaixo do efeito aberto
    int rowH = Sc(80, hwnd), headH = Sc(28, hwnd);
    rows_.clear(); heads_.clear();
    settings_ = {};
    for (size_t i = 0; i < effects.size(); i++) {
        if (i == 0 || effects[i].group != effects[i - 1].group) { heads_.push_back({ y, effects[i].group }); y += headH; }
        rows_.push_back({ pad / 2, y, W - pad / 2, y + rowH });
        y += rowH + Sc(4, hwnd);
        if ((int)i == open_) { settings_ = { pad / 2, y - Sc(4, hwnd), W - pad / 2, y - Sc(4, hwnd) + SettingsHeight() }; y = settings_.bottom + Sc(8, hwnd); }
    }
    contentH_ = y + Sc(10, hwnd);
    scroll_ = std::clamp(scroll_, 0, std::max(0, contentH_ - (int)cr.bottom));
    SCROLLINFO si{ sizeof si, SIF_ALL };
    si.nMax = contentH_; si.nPage = cr.bottom; si.nPos = scroll_;
    SetScrollInfo(hwnd, SB_VERT, &si, TRUE);
    // controles do efeito aberto: rótulo à esquerda (desenhado), controle à direita; botões embaixo
    if (open_ >= 0) {
        int lx = settings_.left + Sc(12, hwnd), cx = settings_.left + Sc(104, hwnd), cw = settings_.right - Sc(12, hwnd) - cx;
        int yy = settings_.top + Sc(10, hwnd) - scroll_;
        const auto& ps = effects[open_].params;
        for (size_t k = 0; k < ps.size() && k < ctls_.size(); k++) {
            int ch = ps[k].kind == FxParam::Text ? Sc(26, hwnd) : Sc(28, hwnd);
            if (ps[k].kind == FxParam::Toggle) MoveWindow(ctls_[k], lx, yy + Sc(2, hwnd), settings_.right - Sc(12, hwnd) - lx, Sc(28, hwnd), TRUE);
            else MoveWindow(ctls_[k], cx, yy + (Sc(32, hwnd) - ch) / 2, cw, ch, TRUE);
            yy += Sc(36, hwnd);
        }
        int bw = Sc(92, hwnd), bh = Sc(30, hwnd), by = settings_.bottom - Sc(40, hwnd) - scroll_;
        MoveWindow(apply_, settings_.right - Sc(12, hwnd) - bw, by, bw, bh, TRUE);
        MoveWindow(cancel_, settings_.right - Sc(20, hwnd) - 2 * bw, by, bw, bh, TRUE);
    }
    InvalidateRect(hwnd, nullptr, FALSE);
}

void EffectsPanel::CreateControls() {
    DestroyControls();
    if (open_ < 0) return;
    HINSTANCE hi = GetModuleHandleW(nullptr);
    int dpi = DpiForWindow(hwnd);
    using namespace mui;
    auto& ps = effects[open_].params;
    for (size_t k = 0; k < ps.size(); k++) {
        const FxParam& p = ps[k];
        HWND c = nullptr;
        int id = FX_PARAM + (int)k;
        switch (p.kind) {
        case FxParam::Choice: {
            c = CreateWindowExW(0, kCombo, nullptr, WS_CHILD | WS_VISIBLE | WS_TABSTOP, 0, 0, 10, 10, hwnd, (HMENU)(INT_PTR)id, hi, nullptr);
            for (auto& o : p.options) ComboBox_AddString(c, o.c_str());
            for (size_t i = 0; i < p.values.size(); i++) if (p.values[i] == p.value) ComboBox_SetCurSel(c, (int)i);
            break;
        }
        case FxParam::Toggle:
            c = CreateWindowExW(0, kToggle, p.label.c_str(), WS_CHILD | WS_VISIBLE | WS_TABSTOP, 0, 0, 10, 10, hwnd, (HMENU)(INT_PTR)id, hi, nullptr);
            Button_SetCheck(c, p.value ? BST_CHECKED : BST_UNCHECKED);
            break;
        case FxParam::Color:
            c = CreateWindowExW(0, L"BUTTON", nullptr, WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW, 0, 0, 10, 10, hwnd, (HMENU)(INT_PTR)id, hi, nullptr);
            break;
        case FxParam::Text:
            c = CreateWindowExW(WS_EX_CLIENTEDGE, L"EDIT", p.text.c_str(), WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL, 0, 0, 10, 10, hwnd,
                                (HMENU)(INT_PTR)id, hi, nullptr);
            break;
        }
        SendMessageW(c, WM_SETFONT, (WPARAM)ui::UiFont(dpi), TRUE);
        ctls_.push_back(c);
    }
    apply_ = CreateWindowExW(0, kButton, Tr(L"Aplicar"), WS_CHILD | WS_VISIBLE | WS_TABSTOP, 0, 0, 10, 10, hwnd, (HMENU)FX_APPLY, hi, nullptr);
    SendMessageW(apply_, MUI_SETSTYLE, ButtonAccent, 0);
    cancel_ = CreateWindowExW(0, kButton, Tr(L"Cancelar"), WS_CHILD | WS_VISIBLE | WS_TABSTOP, 0, 0, 10, 10, hwnd, (HMENU)FX_CANCEL, hi, nullptr);
    for (HWND b : { apply_, cancel_ }) SendMessageW(b, WM_SETFONT, (WPARAM)ui::UiFont(dpi), TRUE);
}

void EffectsPanel::DestroyControls() {
    for (HWND c : ctls_) DestroyWindow(c);
    ctls_.clear();
    if (apply_) { DestroyWindow(apply_); apply_ = nullptr; }
    if (cancel_) { DestroyWindow(cancel_); cancel_ = nullptr; }
}

void EffectsPanel::Expand(int cmd) {
    int idx = -1;
    for (int i = 0; i < (int)effects.size(); i++) if (effects[i].cmd == cmd) idx = i;
    if (idx < 0) return;
    if (open_ == idx) return;
    if (open_ >= 0) Collapse();
    open_ = idx;
    CreateControls();
    Relayout();
    // rola até o efeito aberto e seus ajustes
    RECT cr; GetClientRect(hwnd, &cr);
    int top = rows_[idx].top - Sc(8, hwnd), bottom = settings_.bottom + Sc(8, hwnd);
    if (top < scroll_) scroll_ = top;
    else if (bottom > scroll_ + cr.bottom) scroll_ = std::min(top, bottom - (int)cr.bottom);
    Relayout();
    if (onPreview) onPreview(effects[idx].cmd, effects[idx].params);
}

void EffectsPanel::Collapse() {
    if (open_ < 0) return;
    KillTimer(hwnd, kFxTextTimer);
    open_ = -1;
    DestroyControls();
    Relayout();
    if (onCancel) onCancel();
}

void EffectsPanel::ParamChanged(int k) {
    if (open_ < 0) return;
    auto& e = effects[open_];
    if (k >= 0 && k < (int)e.params.size()) {
        FxParam& p = e.params[k];
        HWND c = ctls_[k];
        switch (p.kind) {
        case FxParam::Choice: { int i = ComboBox_GetCurSel(c); if (i >= 0 && i < (int)p.values.size()) p.value = p.values[i]; break; }
        case FxParam::Toggle: p.value = Button_GetCheck(c) == BST_CHECKED; break;
        case FxParam::Text: { int n = GetWindowTextLengthW(c); std::wstring t(n, L'\0'); if (n) GetWindowTextW(c, t.data(), n + 1); p.text = t; break; }
        default: break;
        }
    }
    if (makePreview && open_ < (int)previews_.size()) previews_[open_] = makePreview(e.cmd, e.params, src_);
    InvalidateRect(hwnd, nullptr, FALSE);
    if (onPreview) onPreview(e.cmd, e.params);
}

int EffectsPanel::HitRow(POINT p) const {
    for (size_t i = 0; i < rows_.size(); i++) { RECT r = rows_[i]; OffsetRect(&r, 0, -scroll_); if (PtInRect(&r, p)) return (int)i; }
    return -1;
}

int EffectsPanel::HitTile(POINT p) const {
    for (size_t i = 0; i < tiles_.size(); i++) { RECT r = tiles_[i]; OffsetRect(&r, 0, -scroll_); if (PtInRect(&r, p)) return (int)i; }
    return -1;
}

static void DrawThumbCard(Gdiplus::Graphics& g, const Image& p, RECT card, int dpi, const ui::ThemeColors& th) {
    RECT shadow = card; OffsetRect(&shadow, 0, 1);
    FillRound(g, shadow, (float)ui::Scale(6, dpi), RGB(0, 0, 0), 22);
    FillRound(g, card, (float)ui::Scale(6, dpi), Mix(th.bgSide, RGB(255, 255, 255), 0.5));
    if (p.empty()) return;
    int bw = card.right - card.left - ui::Scale(8, dpi), bh = card.bottom - card.top - ui::Scale(8, dpi);
    double s = std::min((double)bw / p.w, (double)bh / p.h);
    int w = std::max(1, (int)(p.w * s)), h = std::max(1, (int)(p.h * s));
    auto gb = const_cast<Image&>(p).WrapGdip();
    g.SetInterpolationMode(Gdiplus::InterpolationModeHighQualityBicubic);
    g.DrawImage(gb.get(), (INT)(card.left + (card.right - card.left - w) / 2), (INT)(card.top + (card.bottom - card.top - h) / 2), w, h);
}

void EffectsPanel::Paint(HDC hdc) {
    const auto& th = EdTheme();
    RECT cr; GetClientRect(hwnd, &cr);
    HDC mem = CreateCompatibleDC(hdc);
    HBITMAP bb = CreateCompatibleBitmap(hdc, std::max(1L, cr.right), std::max(1L, cr.bottom));
    HGDIOBJ ob = SelectObject(mem, bb);
    FillRect(mem, &cr, bg_);
    int dpi = DpiForWindow(hwnd);
    int pad = ui::Scale(10, dpi), sy = -scroll_;
    SetBkMode(mem, TRANSPARENT);
    HGDIOBJ of = SelectObject(mem, ui::UiFont(dpi, 8, true));
    auto head = [&](int y, const std::wstring& name) {
        RECT hr{ pad, y + sy + ui::Scale(6, dpi), cr.right - pad, y + sy + ui::Scale(28, dpi) };
        std::wstring up = name;
        CharUpperBuffW(up.data(), (DWORD)up.size());      // respeita acentos (towupper no locale "C" não)
        SelectObject(mem, ui::UiFont(dpi, 8, true));
        SetTextColor(mem, th.accentText);
        DrawTextW(mem, up.c_str(), -1, &hr, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
    };
    head(ui::Scale(2, dpi), Tr(L"Efeitos rápidos"));
    {
        Gdiplus::Graphics g(mem);
        g.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
        g.SetPixelOffsetMode(Gdiplus::PixelOffsetModeHalf);
        // galeria
        for (int i = 0; i < (int)tiles_.size(); i++) {
            RECT t = tiles_[i]; OffsetRect(&t, 0, sy);
            if (t.bottom < 0 || t.top > cr.bottom) continue;
            bool add = i == (int)quick.size(), hot = i == hoverTile_;
            RECT card{ t.left, t.top, t.right, t.bottom - ui::Scale(22, dpi) };
            if (add) {
                FillRound(g, card, (float)ui::Scale(6, dpi), hot ? Mix(th.bgMain, th.control, 0.10) : th.bgMain);
                Gdiplus::Pen dash(GC(hot ? th.control : th.line), (float)ui::Scale(1, dpi) + 0.5f);
                dash.SetDashStyle(Gdiplus::DashStyleDash);
                Gdiplus::GraphicsPath p;
                RoundPath(p, Gdiplus::RectF((float)card.left + 1, (float)card.top + 1, (float)(card.right - card.left - 2), (float)(card.bottom - card.top - 2)), (float)ui::Scale(6, dpi));
                g.DrawPath(&dash, &p);
                Gdiplus::Pen plus(GC(hot ? th.control : RGB(110, 110, 118)), (float)ui::Scale(2, dpi));
                float cx = (card.left + card.right) / 2.0f, cy = (card.top + card.bottom) / 2.0f, s = (float)ui::Scale(8, dpi);
                g.DrawLine(&plus, cx - s, cy, cx + s, cy); g.DrawLine(&plus, cx, cy - s, cx, cy + s);
            } else {
                DrawThumbCard(g, i < (int)quickPreviews_.size() ? quickPreviews_[i] : Image(), card, dpi, th);
                if (hot) StrokeRound(g, card, (float)ui::Scale(6, dpi), th.control, (float)ui::Scale(2, dpi));
            }
        }
        // efeitos
        for (size_t i = 0; i < rows_.size(); i++) {
            RECT r = rows_[i]; OffsetRect(&r, 0, sy);
            bool isOpen = (int)i == open_;
            if (isOpen) {                       // cartão do efeito aberto, com os ajustes
                RECT box{ r.left, r.top, r.right, settings_.bottom + sy };
                FillRound(g, box, (float)ui::Scale(8, dpi), Mix(th.bgMain, th.control, 0.06));
                StrokeRound(g, box, (float)ui::Scale(8, dpi), Mix(th.bgMain, th.control, 0.55), 1.0f);
            } else if ((int)i == hover_) {
                FillRound(g, r, (float)ui::Scale(8, dpi), Mix(th.bgMain, th.control, 0.09));
                StrokeRound(g, r, (float)ui::Scale(8, dpi), Mix(th.bgMain, th.control, 0.45), 1.0f);
            }
            if (r.bottom < 0 || r.top > cr.bottom) continue;
            int pw = ui::Scale(84, dpi), ph = ui::Scale(56, dpi);
            RECT card{ r.left + pad / 2, r.top + (r.bottom - r.top - ph) / 2, r.left + pad / 2 + pw, r.top + (r.bottom - r.top - ph) / 2 + ph };
            DrawThumbCard(g, i < previews_.size() ? previews_[i] : Image(), card, dpi, th);
            // seta de abrir/fechar
            Gdiplus::Pen cv(GC(isOpen ? th.accentText : RGB(120, 120, 128)), std::max(1.4f, dpi / 64.0f));
            cv.SetStartCap(Gdiplus::LineCapRound); cv.SetEndCap(Gdiplus::LineCapRound);
            float ax = (float)r.right - ui::Scale(14, dpi), ay = (float)r.top + ui::Scale(20, dpi), k = (float)ui::Scale(4, dpi);
            Gdiplus::PointF chev[3] = { { ax - k, ay - (isOpen ? -k / 2 : k / 2) }, { ax, ay + (isOpen ? -k / 2 : k / 2) }, { ax + k, ay - (isOpen ? -k / 2 : k / 2) } };
            g.DrawLines(&cv, chev, 3);
        }
    }
    // nomes da galeria
    SelectObject(mem, ui::UiFont(dpi, 8));
    for (int i = 0; i < (int)tiles_.size(); i++) {
        RECT t = tiles_[i]; OffsetRect(&t, 0, sy);
        RECT lr{ t.left - 2, t.bottom - ui::Scale(21, dpi), t.right + 2, t.bottom };
        SetTextColor(mem, i == hoverTile_ ? th.accentText : RGB(70, 70, 78));
        std::wstring name = i == (int)quick.size() ? Tr(L"Adicionar") : quick[i];
        DrawTextW(mem, name.c_str(), -1, &lr, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);
    }
    for (auto& [hy, name] : heads_) head(hy, name);
    for (size_t i = 0; i < rows_.size(); i++) {
        RECT r = rows_[i]; OffsetRect(&r, 0, sy);
        if (r.bottom < 0 || r.top > cr.bottom) continue;
        int pw = ui::Scale(84, dpi);
        RECT nr{ r.left + pad + pw, r.top + ui::Scale(9, dpi), r.right - ui::Scale(24, dpi), r.top + ui::Scale(29, dpi) };
        SelectObject(mem, ui::UiFont(dpi, 10, true));
        SetTextColor(mem, (int)i == hover_ || (int)i == open_ ? th.accentText : RGB(28, 28, 32));
        DrawTextW(mem, effects[i].name.c_str(), -1, &nr, DT_LEFT | DT_SINGLELINE | DT_END_ELLIPSIS | DT_VCENTER | DT_NOPREFIX);
        RECT dr{ nr.left, nr.bottom, r.right - ui::Scale(8, dpi), r.bottom - ui::Scale(6, dpi) };
        SelectObject(mem, ui::UiFont(dpi, 8));
        SetTextColor(mem, RGB(104, 104, 112));
        DrawTextW(mem, effects[i].desc.c_str(), -1, &dr, DT_LEFT | DT_WORDBREAK | DT_END_ELLIPSIS | DT_NOPREFIX);
    }
    // rótulos dos ajustes
    if (open_ >= 0) {
        SelectObject(mem, ui::UiFont(dpi, 9));
        SetTextColor(mem, RGB(48, 48, 54));
        int yy = settings_.top + ui::Scale(10, dpi) + sy;
        for (auto& p : effects[open_].params) {
            if (p.kind != FxParam::Toggle) {
                RECT lr{ settings_.left + ui::Scale(12, dpi), yy, settings_.left + ui::Scale(100, dpi), yy + ui::Scale(32, dpi) };
                DrawTextW(mem, p.label.c_str(), -1, &lr, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);
            }
            yy += ui::Scale(36, dpi);
        }
    }
    SelectObject(mem, of);
    BitBlt(hdc, 0, 0, cr.right, cr.bottom, mem, 0, 0, SRCCOPY);
    SelectObject(mem, ob); DeleteObject(bb); DeleteDC(mem);
}

LRESULT CALLBACK EffectsPanel::Proc(HWND h, UINT m, WPARAM w, LPARAM l) {
    EffectsPanel* self = (EffectsPanel*)GetWindowLongPtrW(h, GWLP_USERDATA);
    if (m == WM_NCCREATE) { self = (EffectsPanel*)((CREATESTRUCTW*)l)->lpCreateParams; SetWindowLongPtrW(h, GWLP_USERDATA, (LONG_PTR)self); self->hwnd = h; }
    return self ? self->Handle(m, w, l) : DefWindowProcW(h, m, w, l);
}

LRESULT EffectsPanel::Handle(UINT m, WPARAM w, LPARAM l) {
    switch (m) {
    case WM_ERASEBKGND: return 1;
    case WM_PAINT: { PAINTSTRUCT ps; HDC dc = BeginPaint(hwnd, &ps); Paint(dc); EndPaint(hwnd, &ps); return 0; }
    case WM_SIZE: Relayout(); return 0;
    case WM_CTLCOLORSTATIC: case WM_CTLCOLORBTN: SetBkMode((HDC)w, TRANSPARENT); return (LRESULT)bg_;
    case WM_VSCROLL: {
        SCROLLINFO si{ sizeof si, SIF_ALL }; GetScrollInfo(hwnd, SB_VERT, &si);
        switch (LOWORD(w)) {
        case SB_LINEUP: scroll_ -= 40; break; case SB_LINEDOWN: scroll_ += 40; break;
        case SB_PAGEUP: scroll_ -= si.nPage; break; case SB_PAGEDOWN: scroll_ += si.nPage; break;
        case SB_THUMBTRACK: case SB_THUMBPOSITION: scroll_ = si.nTrackPos; break;
        }
        Relayout();
        return 0;
    }
    case WM_MOUSEWHEEL: scroll_ -= GET_WHEEL_DELTA_WPARAM(w) * Sc(60, hwnd) / WHEEL_DELTA; Relayout(); return 0;
    case WM_MOUSEMOVE: {
        POINT p{ GET_X_LPARAM(l), GET_Y_LPARAM(l) };
        int hv = HitRow(p), ht = HitTile(p);
        if (hv != hover_ || ht != hoverTile_) { hover_ = hv; hoverTile_ = ht; InvalidateRect(hwnd, nullptr, FALSE); }
        SetCursor(LoadCursor(nullptr, hv >= 0 || ht >= 0 ? IDC_HAND : IDC_ARROW));
        TRACKMOUSEEVENT t{ sizeof(t), TME_LEAVE, hwnd, 0 }; TrackMouseEvent(&t);
        return 0;
    }
    case WM_SETCURSOR: if ((HWND)w == hwnd && LOWORD(l) == HTCLIENT) return TRUE; break;
    case WM_MOUSELEAVE: hover_ = hoverTile_ = -1; InvalidateRect(hwnd, nullptr, FALSE); return 0;
    case WM_LBUTTONUP: {
        POINT p{ GET_X_LPARAM(l), GET_Y_LPARAM(l) };
        int t = HitTile(p);
        if (t >= 0) {
            if (t == (int)quick.size()) { if (onAddQuick) onAddQuick(); }
            else { if (open_ >= 0) Collapse(); if (onQuick) onQuick(t); }
            return 0;
        }
        int i = HitRow(p);
        if (i < 0) return 0;
        if (i == open_) Collapse(); else Expand(effects[i].cmd);
        return 0;
    }
    case WM_RBUTTONUP: {
        POINT p{ GET_X_LPARAM(l), GET_Y_LPARAM(l) };
        int t = HitTile(p);
        if (t < 0 || t >= (int)quick.size()) return 0;
        HMENU mm = CreatePopupMenu();
        AppendMenuW(mm, MF_STRING, 1, Tr(L"Aplicar"));
        AppendMenuW(mm, MF_SEPARATOR, 0, nullptr);
        AppendMenuW(mm, MF_STRING, 2, Tr(L"Excluir este efeito rápido"));
        POINT sp; GetCursorPos(&sp);
        int cmd = TrackPopupMenu(mm, TPM_RETURNCMD | TPM_RIGHTBUTTON, sp.x, sp.y, 0, hwnd, nullptr);
        DestroyMenu(mm);
        if (cmd == 1 && onQuick) onQuick(t);
        else if (cmd == 2 && onDeleteQuick) onDeleteQuick(t);
        return 0;
    }
    case WM_TIMER: if (w == kFxTextTimer) { KillTimer(hwnd, w); ParamChanged(-1); } return 0;
    case WM_DRAWITEM: {          // botão de cor: amostra + valor
        auto* di = (DRAWITEMSTRUCT*)l;
        int k = (int)di->CtlID - FX_PARAM;
        if (open_ < 0 || k < 0 || k >= (int)effects[open_].params.size()) return FALSE;
        COLORREF c = effects[open_].params[k].color;
        const auto& th = EdTheme();
        int dpi = DpiForWindow(hwnd);
        FillRect(di->hDC, &di->rcItem, bg_);
        RECT r = di->rcItem;
        {
            Gdiplus::Graphics g(di->hDC);
            g.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
            g.SetPixelOffsetMode(Gdiplus::PixelOffsetModeHalf);
            FillRound(g, r, (float)ui::Scale(4, dpi), RGB(255, 255, 255));
            StrokeRound(g, r, (float)ui::Scale(4, dpi), Mix(th.line, 0, 0.07), 1.0f);
            RECT sw{ r.left + ui::Scale(6, dpi), r.top + ui::Scale(6, dpi), r.left + ui::Scale(6, dpi) + (r.bottom - r.top - ui::Scale(12, dpi)) * 2, r.bottom - ui::Scale(6, dpi) };
            FillRound(g, sw, (float)ui::Scale(3, dpi), c);
            StrokeRound(g, sw, (float)ui::Scale(3, dpi), Mix(c, 0, 0.25), 1.0f);
            r.left = sw.right + ui::Scale(10, dpi);
        }
        HGDIOBJ of = SelectObject(di->hDC, ui::UiFont(dpi));
        SetBkMode(di->hDC, TRANSPARENT); SetTextColor(di->hDC, RGB(40, 40, 44));
        std::wstring t = ColorToHex(c);
        DrawTextW(di->hDC, t.c_str(), -1, &r, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
        SelectObject(di->hDC, of);
        return TRUE;
    }
    case WM_COMMAND: {
        int id = LOWORD(w), code = HIWORD(w);
        if (id == FX_APPLY && open_ >= 0) {
            int idx = open_;
            KillTimer(hwnd, kFxTextTimer);           // o texto já foi lido a cada tecla (EN_CHANGE)
            if (onApply) onApply(effects[idx].cmd, effects[idx].params);
            open_ = -1; DestroyControls(); Relayout();
            return 0;
        }
        if (id == FX_CANCEL) { Collapse(); return 0; }
        int k = id - FX_PARAM;
        if (open_ < 0 || k < 0 || k >= (int)effects[open_].params.size()) return 0;
        FxParam& p = effects[open_].params[k];
        if (p.kind == FxParam::Color && code == BN_CLICKED) {
            COLORREF c = p.color;
            if (PickColor(GetAncestor(hwnd, GA_ROOT), c)) { p.color = c; InvalidateRect(ctls_[k], nullptr, FALSE); ParamChanged(k); }
        } else if (p.kind == FxParam::Text && code == EN_CHANGE) {
            int n = GetWindowTextLengthW(ctls_[k]); std::wstring t(n, L'\0'); if (n) GetWindowTextW(ctls_[k], t.data(), n + 1);
            p.text = t;
            SetTimer(hwnd, kFxTextTimer, 350, nullptr);     // digitação: prévia depois de uma pausa
        } else if ((p.kind == FxParam::Choice && code == CBN_SELCHANGE) || (p.kind == FxParam::Toggle && code == BN_CLICKED)) ParamChanged(k);
        return 0;
    }
    }
    return DefWindowProcW(hwnd, m, w, l);
}

// ================================================================= bandeja de capturas recentes e biblioteca
static int RecentMax() { return std::clamp((int)RegGetDword(L"RecentTrayDocs", 10, L"Editor"), 1, 100); }
static std::wstring TrayFile() { return LibraryDir() + L"\\Tray.bin"; }

void LibraryTray::Create(HWND parent, Mode m) {
    static bool reg = false;
    if (!reg) {
        WNDCLASSW wc{}; wc.lpfnWndProc = Proc; wc.hInstance = GetModuleHandleW(nullptr); wc.hCursor = LoadCursor(nullptr, IDC_ARROW);
        wc.lpszClassName = L"SnapnabTray";
        wc.style = CS_DBLCLKS;
        RegisterClassW(&wc); reg = true;
    }
    mode = m;
    hwnd = CreateWindowExW(0, L"SnapnabTray", nullptr, WS_CHILD | WS_TABSTOP | (m == Tray ? WS_HSCROLL | WS_VISIBLE : WS_VSCROLL), 0, 0, 10, 10, parent,
                           nullptr, GetModuleHandleW(nullptr), this);
    DragAcceptFiles(hwnd, TRUE);                  // arquivos soltos do Explorer
    if (m == Tray) LoadRecent();
}

int LibraryTray::Height() const { return Sc(142, hwnd) + GetSystemMetricsForDpi(SM_CYHSCROLL, DpiForWindow(hwnd)); }

// Tray.bin: um caminho por linha (UTF-16), na ordem da bandeja; "-caminho" = tirado da bandeja pelo usuário
void LibraryTray::LoadRecent() {
    recent_.clear();
    hidden_.clear();
    HANDLE f = CreateFileW(TrayFile().c_str(), GENERIC_READ, FILE_SHARE_READ, nullptr, OPEN_EXISTING, 0, nullptr);
    if (f == INVALID_HANDLE_VALUE) return;
    DWORD size = GetFileSize(f, nullptr), rd = 0;
    std::wstring buf(size / sizeof(wchar_t), L'\0');
    if (size) ReadFile(f, buf.data(), size, &rd, nullptr);
    CloseHandle(f);
    size_t i = 0;
    while (i < buf.size()) {
        size_t j = buf.find(L'\n', i);
        std::wstring line = Trim(buf.substr(i, j == std::wstring::npos ? std::wstring::npos : j - i));
        if (!line.empty() && line[0] == 0xFEFF) line = line.substr(1);
        if (line.size() > 1 && line[0] == L'-') hidden_.push_back(line.substr(1));
        else if (!line.empty()) recent_.push_back(line);
        if (j == std::wstring::npos) break;
        i = j + 1;
    }
}

void LibraryTray::SaveRecent() const {
    if (mode != Tray) return;
    std::wstring s;
    for (auto& p : recent_) s += p + L"\r\n";
    for (auto& p : hidden_) s += L"-" + p + L"\r\n";
    HANDLE f = CreateFileW(TrayFile().c_str(), GENERIC_WRITE, 0, nullptr, CREATE_ALWAYS, 0, nullptr);
    if (f == INVALID_HANDLE_VALUE) return;
    DWORD wr;
    WriteFile(f, s.data(), (DWORD)(s.size() * sizeof(wchar_t)), &wr, nullptr);
    CloseHandle(f);
}

void LibraryTray::AddRecent(const std::wstring& path) {
    if (mode != Tray || path.empty() || !IsInLibrary(path)) return;
    recent_.erase(std::remove_if(recent_.begin(), recent_.end(), [&](auto& r) { return !_wcsicmp(r.c_str(), path.c_str()); }), recent_.end());
    recent_.insert(recent_.begin(), path);
    if ((int)recent_.size() > RecentMax()) recent_.resize(RecentMax());
    hidden_.erase(std::remove_if(hidden_.begin(), hidden_.end(), [&](auto& r) { return !_wcsicmp(r.c_str(), path.c_str()); }), hidden_.end());
    SaveRecent();
}

void LibraryTray::RemoveFromTray(const std::vector<std::wstring>& paths) {
    for (auto& p : paths) {
        recent_.erase(std::remove_if(recent_.begin(), recent_.end(), [&](auto& r) { return !_wcsicmp(r.c_str(), p.c_str()); }), recent_.end());
        hidden_.push_back(p);                     // não volta pelo preenchimento automático
    }
    SaveRecent();
    Reload();
}

void LibraryTray::Reload() {
    // a seleção sobrevive à recarga (por caminho)
    std::vector<std::wstring> keep = Selection();
    std::vector<LibraryItem> all = ListLibrary(100000);
    if (mode == Tray) {
        auto lower = [](std::wstring s) { return ToLower(s); };
        if (known_.empty()) {
            for (auto& it : all) known_.push_back(lower(it.path));
            if (recent_.empty())               // primeira vez (sem Tray.bin): as mais recentes da biblioteca
                for (size_t i = 0; i < all.size() && (int)i < RecentMax(); i++) recent_.push_back(all[i].path);
        } else {
            // capturas novas na biblioteca entram na frente da bandeja (da mais antiga para a mais nova)
            std::vector<std::wstring> fresh;
            for (auto& it : all) if (std::find(known_.begin(), known_.end(), lower(it.path)) == known_.end()) fresh.push_back(it.path);
            for (auto it = fresh.rbegin(); it != fresh.rend(); ++it) {
                recent_.erase(std::remove_if(recent_.begin(), recent_.end(), [&](auto& r) { return !_wcsicmp(r.c_str(), it->c_str()); }), recent_.end());
                recent_.insert(recent_.begin(), *it);
            }
            known_.clear();
            for (auto& it : all) known_.push_back(lower(it.path));
        }
        recent_.erase(std::remove_if(recent_.begin(), recent_.end(), [](auto& p) { return !FileExists(p); }), recent_.end());
        hidden_.erase(std::remove_if(hidden_.begin(), hidden_.end(), [](auto& p) { return !FileExists(p); }), hidden_.end());
        if ((int)recent_.size() > RecentMax()) recent_.resize(RecentMax());
        // um item excluído abre vaga: entra a captura seguinte da biblioteca (mais antiga que a última da bandeja)
        if ((int)recent_.size() < RecentMax()) {
            auto has = [](const std::vector<std::wstring>& v, const std::wstring& p) {
                return std::any_of(v.begin(), v.end(), [&](auto& r) { return !_wcsicmp(r.c_str(), p.c_str()); });
            };
            size_t start = 0;
            if (!recent_.empty()) {
                auto last = std::find_if(all.begin(), all.end(), [&](auto& it) { return !_wcsicmp(it.path.c_str(), recent_.back().c_str()); });
                start = last == all.end() ? all.size() : (size_t)(last - all.begin()) + 1;
            }
            for (size_t i = start; i < all.size() && (int)recent_.size() < RecentMax(); i++)
                if (!has(recent_, all[i].path) && !has(hidden_, all[i].path)) recent_.push_back(all[i].path);
        }
        SaveRecent();
        items_.clear();
        for (auto& p : recent_) {
            auto f = std::find_if(all.begin(), all.end(), [&](auto& it) { return !_wcsicmp(it.path.c_str(), p.c_str()); });
            if (f != all.end()) items_.push_back(*f);
        }
    } else items_ = std::move(all);
    sel_.assign(items_.size(), false);
    for (size_t i = 0; i < items_.size(); i++)
        for (auto& k : keep) if (!_wcsicmp(k.c_str(), items_[i].path.c_str())) sel_[i] = true;
    if (anchor_ >= (int)items_.size()) anchor_ = -1;
    hover_ = hoverClose_ = -1;
    // miniaturas vazias (arquivo ainda sendo gravado) são tentadas de novo
    for (auto it = thumbs_.begin(); it != thumbs_.end();) it = it->second.empty() ? thumbs_.erase(it) : std::next(it);
    UpdateScroll();
}

void LibraryTray::SetCurrent(const std::wstring& p) {
    current = p;
    if (mode == Tray) { AddRecent(p); Reload(); }
    // abrir um item deixa só ele marcado
    for (size_t i = 0; i < items_.size(); i++) sel_[i] = !_wcsicmp(items_[i].path.c_str(), p.c_str());
    EnsureVisible();
    InvalidateRect(hwnd, nullptr, FALSE);
}

void LibraryTray::SelectAll() {
    std::fill(sel_.begin(), sel_.end(), true);
    InvalidateRect(hwnd, nullptr, FALSE);
}

std::vector<std::wstring> LibraryTray::Selection() const {
    std::vector<std::wstring> v;
    for (size_t i = 0; i < items_.size() && i < sel_.size(); i++) if (sel_[i]) v.push_back(items_[i].path);
    return v;
}

void LibraryTray::InvalidateThumb(const std::wstring& p) { thumbs_.erase(ToLower(p)); InvalidateRect(hwnd, nullptr, FALSE); }

const Image& LibraryTray::Thumb(const std::wstring& path, bool video) {
    std::wstring key = ToLower(path);
    auto it = thumbs_.find(key);
    if (it != thumbs_.end()) return it->second;
    Image t = LoadThumbnail(path);
    if (t.empty() && video) {
        VideoReader r; Image f; LONGLONG ts;
        if (r.Open(path) && r.ReadFrame(f, ts)) { WriteThumbnail(path, f); t = MakeThumbnail(f, 160, 120); }
    } else if (t.empty()) {
        std::vector<SnapFrame> fr;
        if (LoadSnapFile(path, fr)) { WriteThumbnail(path, fr[0].base); t = MakeThumbnail(fr[0].base, 160, 120); }
    }
    return thumbs_[key] = std::move(t);
}

static void CellMetrics(HWND h, int& cw, int& ch, int& pad, int& top) {
    int dpi = DpiForWindow(h);
    cw = ui::Scale(128, dpi); ch = ui::Scale(100, dpi); pad = ui::Scale(10, dpi); top = ui::Scale(36, dpi);
}

int LibraryTray::Columns() const {
    if (mode == Tray) return 1 << 20;
    RECT cr; GetClientRect(hwnd, &cr);
    int cw, ch, pad, top; CellMetrics(hwnd, cw, ch, pad, top);
    return std::max(1, (int)(cr.right - pad) / (cw + pad));
}

int LibraryTray::ContentExtent() const {
    int cw, ch, pad, top; CellMetrics(hwnd, cw, ch, pad, top);
    if (mode == Tray) return pad + (int)items_.size() * (cw + pad);
    int rows = ((int)items_.size() + Columns() - 1) / Columns();
    return top + rows * (ch + pad) + pad;
}

RECT LibraryTray::CellRect(int i) const {
    int cw, ch, pad, top; CellMetrics(hwnd, cw, ch, pad, top);
    if (mode == Tray) {
        int x0 = pad + i * (cw + pad) - scroll_;
        return { x0, top, x0 + cw, top + ch };
    }
    int cols = Columns();
    RECT cr; GetClientRect(hwnd, &cr);
    int used = cols * (cw + pad) - pad, x0 = std::max(pad, (int)(cr.right - used) / 2);
    int x = x0 + (i % cols) * (cw + pad), y = top + (i / cols) * (ch + pad) - scroll_;
    return { x, y, x + cw, y + ch };
}

RECT LibraryTray::CloseRect(int i) const {
    RECT c = CellRect(i);
    int s = Sc(20, hwnd), m = Sc(5, hwnd);
    return { c.right - m - s, c.top + m, c.right - m, c.top + m + s };
}

RECT LibraryTray::HeaderButton(int which) const {
    // faixa de cima, à direita: [Exportar (n)…] [Excluir (n)] [Limpar seleção]   [Biblioteca | Voltar ao editor]
    RECT cr; GetClientRect(hwnd, &cr);
    int h = Sc(26, hwnd), y = Sc(5, hwnd), gap = Sc(6, hwnd);
    int w[4] = { Sc(118, hwnd), Sc(100, hwnd), Sc(110, hwnd), Sc(mode == Tray ? 104 : 130, hwnd) };
    int x = cr.right - Sc(10, hwnd);
    RECT r[4];
    r[3] = { x - w[3], y, x, y + h }; x -= w[3] + gap * 3;
    for (int k = 2; k >= 0; k--) { r[k] = { x - w[k], y, x, y + h }; x -= w[k] + gap; }
    return r[which];
}

int LibraryTray::Hit(POINT p) const {
    int top; { int cw, ch, pad; CellMetrics(hwnd, cw, ch, pad, top); }
    if (p.y < top - Sc(4, hwnd)) return -1;              // faixa de cima
    for (int i = 0; i < (int)items_.size(); i++) {
        RECT c = CellRect(i);
        if (PtInRect(&c, p)) return i;
    }
    return -1;
}

void LibraryTray::UpdateScroll() {
    RECT cr; GetClientRect(hwnd, &cr);
    int view = mode == Tray ? cr.right : cr.bottom;
    scroll_ = std::clamp(scroll_, 0, std::max(0, ContentExtent() - view));
    SCROLLINFO si{ sizeof(si), SIF_ALL };
    si.nMax = ContentExtent(); si.nPage = view; si.nPos = scroll_;
    SetScrollInfo(hwnd, mode == Tray ? SB_HORZ : SB_VERT, &si, TRUE);
    InvalidateRect(hwnd, nullptr, FALSE);
}

void LibraryTray::EnsureVisible() {
    RECT cr; GetClientRect(hwnd, &cr);
    for (size_t i = 0; i < items_.size(); i++) {
        if (_wcsicmp(items_[i].path.c_str(), current.c_str())) continue;
        RECT c = CellRect((int)i);
        if (mode == Tray) {
            if (c.left < 0) scroll_ += c.left - Sc(10, hwnd);
            else if (c.right > cr.right) scroll_ += c.right - cr.right + Sc(10, hwnd);
        } else {
            if (c.top < Sc(36, hwnd)) scroll_ += c.top - Sc(36, hwnd);
            else if (c.bottom > cr.bottom) scroll_ += c.bottom - cr.bottom + Sc(10, hwnd);
        }
    }
    UpdateScroll();
}

bool LibraryTray::s_draggingOut = false;

namespace {
// IDataObject com os arquivos em dois formatos: CF_HDROP (Explorer, navegadores, mensageiros, Office) e a lista de itens
// do Shell (CFSTR_SHELLIDLIST, que os apps empacotados como o Paint e o Fotos usam). Também guarda os formatos que o
// IDragSourceHelper grava (a imagem que acompanha o cursor).
class FileDropData : public IDataObject {
    LONG ref_ = 1;
    HGLOBAL hdrop_ = nullptr, idlist_ = nullptr;
    CLIPFORMAT cfIdList_ = (CLIPFORMAT)RegisterClipboardFormatW(CFSTR_SHELLIDLIST);
    std::vector<std::pair<FORMATETC, STGMEDIUM>> extra_;
    static HGLOBAL Dup(HGLOBAL h) {
        SIZE_T n = GlobalSize(h);
        HGLOBAL c = GlobalAlloc(GMEM_MOVEABLE, n);
        if (!c) return nullptr;
        void* d = GlobalLock(c); void* s = GlobalLock(h);
        memcpy(d, s, n);
        GlobalUnlock(h); GlobalUnlock(c);
        return c;
    }
public:
    explicit FileDropData(const std::vector<std::wstring>& files) {
        size_t chars = 1;
        for (auto& f : files) chars += f.size() + 1;
        hdrop_ = GlobalAlloc(GHND, sizeof(DROPFILES) + chars * sizeof(wchar_t));
        auto* df = (DROPFILES*)GlobalLock(hdrop_);
        df->pFiles = sizeof(DROPFILES);
        df->fWide = TRUE;
        wchar_t* p = (wchar_t*)((BYTE*)df + sizeof(DROPFILES));
        for (auto& f : files) { memcpy(p, f.c_str(), f.size() * sizeof(wchar_t)); p += f.size(); *p++ = 0; }
        *p = 0;
        GlobalUnlock(hdrop_);
        // CIDA: pasta-mãe = área de trabalho (PIDL vazio) e cada item com o PIDL completo
        std::vector<PIDLIST_ABSOLUTE> pidls;
        for (auto& f : files) { PIDLIST_ABSOLUTE id = nullptr; if (SUCCEEDED(SHParseDisplayName(f.c_str(), nullptr, &id, 0, nullptr)) && id) pidls.push_back(id); }
        if (!pidls.empty()) {
            UINT n = (UINT)pidls.size();
            size_t header = sizeof(UINT) * (n + 2), total = header + 2;
            for (auto id : pidls) total += ILGetSize(id);
            idlist_ = GlobalAlloc(GHND, total);
            BYTE* b = (BYTE*)GlobalLock(idlist_);
            UINT* u = (UINT*)b;
            u[0] = n;
            size_t off = header;
            u[1] = (UINT)off; off += 2;                       // PIDL vazio = área de trabalho
            for (UINT i = 0; i < n; i++) {
                UINT sz = ILGetSize(pidls[i]);
                u[2 + i] = (UINT)off;
                memcpy(b + off, pidls[i], sz);
                off += sz;
            }
            GlobalUnlock(idlist_);
        }
        for (auto id : pidls) CoTaskMemFree(id);
    }
    ~FileDropData() {
        if (hdrop_) GlobalFree(hdrop_);
        if (idlist_) GlobalFree(idlist_);
        for (auto& e : extra_) ReleaseStgMedium(&e.second);
    }
    STDMETHODIMP QueryInterface(REFIID riid, void** pv) override {
        if (riid == IID_IUnknown || riid == IID_IDataObject) { *pv = static_cast<IDataObject*>(this); AddRef(); return S_OK; }
        *pv = nullptr; return E_NOINTERFACE;
    }
    STDMETHODIMP_(ULONG) AddRef() override { return InterlockedIncrement(&ref_); }
    STDMETHODIMP_(ULONG) Release() override { LONG r = InterlockedDecrement(&ref_); if (!r) delete this; return r; }
    STDMETHODIMP GetData(FORMATETC* fe, STGMEDIUM* m) override {
        if (!fe || !m) return E_INVALIDARG;
        if ((fe->cfFormat == CF_HDROP || (fe->cfFormat == cfIdList_ && idlist_)) && (fe->tymed & TYMED_HGLOBAL)) {
            m->tymed = TYMED_HGLOBAL; m->hGlobal = Dup(fe->cfFormat == CF_HDROP ? hdrop_ : idlist_); m->pUnkForRelease = nullptr;
            return m->hGlobal ? S_OK : E_OUTOFMEMORY;
        }
        for (auto& e : extra_)
            if (e.first.cfFormat == fe->cfFormat && (e.first.tymed & fe->tymed) && e.second.tymed == TYMED_HGLOBAL) {
                m->tymed = TYMED_HGLOBAL; m->hGlobal = Dup(e.second.hGlobal); m->pUnkForRelease = nullptr;
                return m->hGlobal ? S_OK : E_OUTOFMEMORY;
            }
        return DV_E_FORMATETC;
    }
    STDMETHODIMP GetDataHere(FORMATETC*, STGMEDIUM*) override { return E_NOTIMPL; }
    STDMETHODIMP QueryGetData(FORMATETC* fe) override {
        if (!fe) return E_INVALIDARG;
        if ((fe->cfFormat == CF_HDROP || (fe->cfFormat == cfIdList_ && idlist_)) && (fe->tymed & TYMED_HGLOBAL)) return S_OK;
        for (auto& e : extra_) if (e.first.cfFormat == fe->cfFormat && (e.first.tymed & fe->tymed)) return S_OK;
        return DV_E_FORMATETC;
    }
    STDMETHODIMP GetCanonicalFormatEtc(FORMATETC*, FORMATETC* out) override { if (out) out->ptd = nullptr; return DATA_S_SAMEFORMATETC; }
    STDMETHODIMP SetData(FORMATETC* fe, STGMEDIUM* m, BOOL release) override {
        if (!fe || !m || m->tymed != TYMED_HGLOBAL) return E_NOTIMPL;
        STGMEDIUM copy{};
        copy.tymed = TYMED_HGLOBAL;
        copy.hGlobal = release ? m->hGlobal : Dup(m->hGlobal);
        for (auto it = extra_.begin(); it != extra_.end(); ++it)
            if (it->first.cfFormat == fe->cfFormat) { ReleaseStgMedium(&it->second); extra_.erase(it); break; }
        FORMATETC f = *fe; f.ptd = nullptr;
        extra_.push_back({ f, copy });
        return S_OK;
    }
    STDMETHODIMP EnumFormatEtc(DWORD dir, IEnumFORMATETC** e) override {
        if (dir != DATADIR_GET) return E_NOTIMPL;
        std::vector<FORMATETC> list = { { CF_HDROP, nullptr, DVASPECT_CONTENT, -1, TYMED_HGLOBAL } };
        if (idlist_) list.push_back({ cfIdList_, nullptr, DVASPECT_CONTENT, -1, TYMED_HGLOBAL });
        for (auto& x : extra_) list.push_back(x.first);
        return SHCreateStdEnumFmtEtc((UINT)list.size(), list.data(), e);
    }
    STDMETHODIMP DAdvise(FORMATETC*, DWORD, IAdviseSink*, DWORD*) override { return OLE_E_ADVISENOTSUPPORTED; }
    STDMETHODIMP DUnadvise(DWORD) override { return OLE_E_ADVISENOTSUPPORTED; }
    STDMETHODIMP EnumDAdvise(IEnumSTATDATA**) override { return OLE_E_ADVISENOTSUPPORTED; }
};
} // namespace

// arrastar itens para outros programas (TrayDragDrop): as imagens vão como PNG (com as anotações) e os vídeos como MP4
void LibraryTray::StartDrag(int item) {
    std::vector<std::wstring> paths;
    if (item < (int)sel_.size() && sel_[item]) paths = Selection();
    else paths = { items_[item].path };
    HCURSOR old = SetCursor(LoadCursor(nullptr, IDC_WAIT));
    std::vector<std::wstring> files = filesForDrag ? filesForDrag(paths) : paths;
    SetCursor(old);
    if (files.empty()) { Log(L"Bandeja: nada para arrastar (%zu itens)", paths.size()); return; }
    auto* obj = new FileDropData(files);
    // imagem que acompanha o cursor: a miniatura do item (com o número de itens, se forem vários)
    IDragSourceHelper* helper = nullptr;
    if (SUCCEEDED(CoCreateInstance(CLSID_DragDropHelper, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&helper)))) {
        const Image& t = Thumb(items_[item].path, items_[item].video);
        if (!t.empty()) {
            int maxS = Sc(96, hwnd);
            double k = std::min({ 1.0, (double)maxS / t.w, (double)maxS / t.h });
            Image img = ScaleImage(t, std::max(1, (int)(t.w * k)), std::max(1, (int)(t.h * k)));
            for (auto& px : img.px) { int a = CA(px) * 220 / 255; px = ARGB(a, CR(px) * a / 255, CG(px) * a / 255, CB(px) * a / 255); }
            SHDRAGIMAGE di{};
            di.sizeDragImage = { img.w, img.h };
            di.ptOffset = { img.w / 2, img.h / 2 };
            di.hbmpDragImage = img.ToHBITMAP();
            di.crColorKey = CLR_NONE;
            if (FAILED(helper->InitializeFromBitmap(&di, obj))) DeleteObject(di.hbmpDragImage);
        }
        helper->Release();
    }
    // o Explorer segue o efeito preferido (copiar); alguns apps, como o Paint, só aceitam quando "mover" também é permitido.
    // Mover só vale para as cópias temporárias: um vídeo vai com o caminho da biblioteca e não pode sair de lá
    FORMATETC pref{ (CLIPFORMAT)RegisterClipboardFormatW(CFSTR_PREFERREDDROPEFFECT), nullptr, DVASPECT_CONTENT, -1, TYMED_HGLOBAL };
    STGMEDIUM prefData{};
    prefData.tymed = TYMED_HGLOBAL;
    if ((prefData.hGlobal = GlobalAlloc(GHND, sizeof(DWORD))) != nullptr) {
        *(DWORD*)GlobalLock(prefData.hGlobal) = DROPEFFECT_COPY;
        GlobalUnlock(prefData.hGlobal);
        obj->SetData(&pref, &prefData, TRUE);
    }
    bool original = std::any_of(files.begin(), files.end(), [&](const std::wstring& f) { return std::find(paths.begin(), paths.end(), f) != paths.end(); });
    DWORD allowed = DROPEFFECT_COPY | DROPEFFECT_LINK | (original ? 0 : DROPEFFECT_MOVE);
    DWORD effect = 0;
    s_draggingOut = true;
    HRESULT hr = SHDoDragDrop(hwnd, obj, nullptr, allowed, &effect);
    // soltar de volta numa janela do próprio Editor (bandeja, biblioteca, tela) chega como WM_DROPFILES postado só depois
    // que o arrasto termina: descarta aqui para não importar uma cópia do item
    MSG pending;
    while (PeekMessageW(&pending, nullptr, WM_DROPFILES, WM_DROPFILES, PM_REMOVE)) DragFinish((HDROP)pending.wParam);
    s_draggingOut = false;
    Log(L"Bandeja: arrastou %zu arquivo(s), resultado 0x%08X, efeito %u", files.size(), (unsigned)hr, (unsigned)effect);
    obj->Release();
}

RECT LibraryTray::BandRect() const {
    POINT a = bandStart_, b = bandEnd_;
    if (mode == Tray) { a.x -= scroll_; b.x -= scroll_; } else { a.y -= scroll_; b.y -= scroll_; }
    return { std::min(a.x, b.x), std::min(a.y, b.y), std::max(a.x, b.x), std::max(a.y, b.y) };
}

// retângulo de seleção: marca as miniaturas que ele toca (com Ctrl/Shift soma à seleção de antes); rola sozinho na borda
void LibraryTray::UpdateBand(POINT p) {
    RECT cr; GetClientRect(hwnd, &cr);
    int edge = Sc(24, hwnd), step = 0;
    if (mode == Tray) { if (p.x < edge) step = -Sc(24, hwnd); else if (p.x > cr.right - edge) step = Sc(24, hwnd); }
    else { if (p.y < Sc(36, hwnd) + edge) step = -Sc(24, hwnd); else if (p.y > cr.bottom - edge) step = Sc(24, hwnd); }
    if (step) { scroll_ += step; UpdateScroll(); }
    bandEnd_ = mode == Tray ? POINT{ p.x + scroll_, p.y } : POINT{ p.x, p.y + scroll_ };
    RECT band = BandRect();
    sel_ = bandBase_;
    sel_.resize(items_.size(), false);
    for (size_t i = 0; i < items_.size(); i++) {
        RECT c = CellRect((int)i), x{};
        if (IntersectRect(&x, &c, &band)) sel_[i] = true;
    }
    InvalidateRect(hwnd, nullptr, FALSE);
}

void LibraryTray::Paint(HDC hdc) {
    const auto& th = EdTheme();
    RECT cr; GetClientRect(hwnd, &cr);
    HDC mem = CreateCompatibleDC(hdc);
    HBITMAP bb = CreateCompatibleBitmap(hdc, std::max(1L, cr.right), std::max(1L, cr.bottom));
    HGDIOBJ ob = SelectObject(mem, bb);
    HBRUSH bg = CreateSolidBrush(mode == Tray ? th.bgSide : th.bgMain); FillRect(mem, &cr, bg); DeleteObject(bg);
    RECT line{ 0, 0, cr.right, 1 }; HBRUSH lb = CreateSolidBrush(th.line); FillRect(mem, &line, lb); DeleteObject(lb);
    int dpi = DpiForWindow(hwnd);
    int nsel = (int)std::count(sel_.begin(), sel_.end(), true);
    int cw, ch, pad, top; CellMetrics(hwnd, cw, ch, pad, top);
    Gdiplus::Graphics g(mem);
    g.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
    g.SetPixelOffsetMode(Gdiplus::PixelOffsetModeHalf);
    COLORREF accent = th.control;
    SetBkMode(mem, TRANSPARENT);
    HGDIOBJ of = SelectObject(mem, ui::UiFont(dpi, 8));
    for (size_t i = 0; i < items_.size(); i++) {
        RECT cell = CellRect((int)i);
        if (cell.right < 0 || cell.bottom < top - ch) continue;
        if (cell.left > cr.right || cell.top > cr.bottom) { if (mode == Tray) break; else continue; }
        bool cur = !_wcsicmp(items_[i].path.c_str(), current.c_str());
        bool hot = (int)i == hover_;
        bool sel = i < sel_.size() && sel_[i];
        float rad = (float)ui::Scale(8, dpi);
        if (hot) {                         // "levanta" a miniatura: sombra mais forte
            RECT s1 = cell; OffsetRect(&s1, 0, ui::Scale(2, dpi)); InflateRect(&s1, 1, 1);
            FillRound(g, s1, rad + 1, RGB(0, 0, 0), 26);
        } else {
            RECT s1 = cell; OffsetRect(&s1, 0, 1);
            FillRound(g, s1, rad, RGB(0, 0, 0), 14);
        }
        FillRound(g, cell, rad, sel ? Mix(RGB(255, 255, 255), accent, 0.14) : hot ? RGB(255, 255, 255) : Mix(th.bgSide, RGB(255, 255, 255), 0.6));
        if (hot || sel || cur) StrokeRound(g, cell, rad, hot || sel ? accent : Mix(accent, RGB(255, 255, 255), 0.35), (float)ui::Scale(hot || sel ? 2 : 1, dpi));
        const Image& t = Thumb(items_[i].path, items_[i].video);
        int th2 = ch - ui::Scale(28, dpi), tw = cw - ui::Scale(14, dpi);
        if (!t.empty()) {
            double s = std::min((double)tw / t.w, (double)th2 / t.h);
            int w = std::max(1, (int)(t.w * s)), h = std::max(1, (int)(t.h * s));
            int ix = cell.left + (cw - w) / 2, iy = cell.top + ui::Scale(7, dpi) + (th2 - h) / 2;
            if (t.HasTransparency()) { Image o(t.w, t.h, 0xFFFFFFFF); o.Blit(t, 0, 0); o.DrawTo(mem, ix, iy, w, h, true); }
            else t.DrawTo(mem, ix, iy, w, h, true);
            if (items_[i].video) {
                RECT badge{ ix + 4, iy + h - ui::Scale(20, dpi), ix + ui::Scale(28, dpi), iy + h - 4 };
                FillRound(g, badge, (float)ui::Scale(5, dpi), RGB(20, 20, 20), 210);
                icons::DrawOnDC(mem, icons::VideoFill, { badge.left + 4, badge.top + 2, badge.right - 4, badge.bottom - 2 }, RGB(255, 255, 255));
            }
        }
        if (sel && nsel > 1) {             // marca da seleção múltipla
            int s = ui::Scale(18, dpi), m = ui::Scale(6, dpi);
            RECT c{ cell.left + m, cell.top + m, cell.left + m + s, cell.top + m + s };
            Gdiplus::SolidBrush cb(GC(accent));
            g.FillEllipse(&cb, (float)c.left, (float)c.top, (float)s, (float)s);
            Gdiplus::Pen ck(Gdiplus::Color(255, 255, 255, 255), std::max(1.6f, dpi / 55.0f));
            ck.SetStartCap(Gdiplus::LineCapRound); ck.SetEndCap(Gdiplus::LineCapRound); ck.SetLineJoin(Gdiplus::LineJoinRound);
            float cx = c.left + s / 2.0f, cy = c.top + s / 2.0f, k = s * 0.22f;
            Gdiplus::PointF p[3] = { { cx - k * 1.2f, cy }, { cx - k * 0.2f, cy + k }, { cx + k * 1.3f, cy - k } };
            g.DrawLines(&ck, p, 3);
        }
        if (hot) {                         // X para excluir direto da miniatura
            RECT x = CloseRect((int)i);
            bool xh = hoverClose_ == (int)i;
            Gdiplus::SolidBrush xb(xh ? Gdiplus::Color(255, 214, 50, 38) : Gdiplus::Color(200, 30, 30, 34));
            g.FillEllipse(&xb, (float)x.left, (float)x.top, (float)(x.right - x.left), (float)(x.bottom - x.top));
            Gdiplus::Pen xp(Gdiplus::Color(255, 255, 255, 255), std::max(1.4f, dpi / 64.0f));
            float m = (x.right - x.left) * 0.32f;
            g.DrawLine(&xp, x.left + m, x.top + m, x.right - m, x.bottom - m);
            g.DrawLine(&xp, x.right - m, x.top + m, x.left + m, x.bottom - m);
        }
        SYSTEMTIME st, lt; FileTimeToSystemTime(&items_[i].modified, &st); SystemTimeToTzSpecificLocalTime(nullptr, &st, &lt);
        std::wstring label = Format(L"%02d/%02d %02d:%02d", lt.wDay, lt.wMonth, lt.wHour, lt.wMinute);
        RECT lr{ cell.left + 4, cell.bottom - ui::Scale(21, dpi), cell.right - 4, cell.bottom - 3 };
        SetTextColor(mem, hot || sel ? th.accentText : RGB(84, 84, 92));
        DrawTextW(mem, label.c_str(), -1, &lr, DT_CENTER | DT_SINGLELINE | DT_VCENTER);
    }
    if (band_) {                           // retângulo de seleção
        RECT b = BandRect();
        if (b.right - b.left > 1 || b.bottom - b.top > 1) {
            Gdiplus::SolidBrush fb(GC(accent, 40));
            g.FillRectangle(&fb, (float)b.left, (float)b.top, (float)(b.right - b.left), (float)(b.bottom - b.top));
            Gdiplus::Pen bp(GC(accent, 220), 1.0f);
            g.DrawRectangle(&bp, (float)b.left, (float)b.top, (float)(b.right - b.left), (float)(b.bottom - b.top));
        }
    }
    // faixa de cima (por cima das miniaturas que rolaram na biblioteca)
    RECT band{ 0, 1, cr.right, top - ui::Scale(4, dpi) };
    HBRUSH bandB = CreateSolidBrush(mode == Tray ? th.bgSide : th.bgMain); FillRect(mem, &band, bandB); DeleteObject(bandB);
    RECT hr{ ui::Scale(12, dpi), ui::Scale(4, dpi), cr.right, ui::Scale(32, dpi) };
    SelectObject(mem, ui::UiFont(dpi, 9, true));
    SetTextColor(mem, RGB(70, 70, 78));
    std::wstring head = mode == Tray ? Tr(L"Capturas recentes") : Format(Tr(L"Biblioteca  •  %zu capturas"), items_.size());
    if (nsel > 1) head += Format(Tr(L"  •  %d selecionadas"), nsel);
    DrawTextW(mem, head.c_str(), -1, &hr, DT_LEFT | DT_SINGLELINE | DT_VCENTER);
    for (int k = 0; k < 4; k++) {
        if (k < 3 && nsel <= 1) continue;
        RECT b = HeaderButton(k);
        bool hot = hoverHeader_ == k;
        COLORREF fill = k == 1 ? (hot ? RGB(196, 43, 28) : RGB(214, 60, 46)) : hot ? Mix(RGB(255, 255, 255), th.control, 0.10) : RGB(255, 255, 255);
        FillRound(g, b, (float)ui::Scale(5, dpi), fill);
        if (k != 1) StrokeRound(g, b, (float)ui::Scale(5, dpi), Mix(th.line, 0, 0.08), 1.0f);
        std::wstring t = k == 0 ? Format(Tr(L"Exportar (%d)…"), nsel) : k == 1 ? Format(Tr(L"Excluir (%d)"), nsel) : k == 2 ? Tr(L"Limpar seleção")
                       : mode == Tray ? Tr(L"Biblioteca") : Tr(L"Voltar ao editor");
        ui::DrawTextCentered(mem, t, b, k == 1 ? RGB(255, 255, 255) : k == 3 ? th.accentText : RGB(36, 36, 40), ui::UiFont(dpi, 8));
    }
    if (items_.empty()) {
        RECT er{ 0, top, cr.right, top + ch };
        ui::DrawTextCentered(mem, mode == Tray ? Tr(L"Nenhuma captura recente. Capture com o Snapnab (Print Screen) ou solte arquivos aqui.")
                                               : Tr(L"A biblioteca está vazia."), er, RGB(120, 120, 125), ui::UiFont(dpi, 9));
    }
    SelectObject(mem, of);
    BitBlt(hdc, 0, 0, cr.right, cr.bottom, mem, 0, 0, SRCCOPY);
    SelectObject(mem, ob); DeleteObject(bb); DeleteDC(mem);
}

LRESULT CALLBACK LibraryTray::Proc(HWND h, UINT m, WPARAM w, LPARAM l) {
    LibraryTray* self = (LibraryTray*)GetWindowLongPtrW(h, GWLP_USERDATA);
    if (m == WM_NCCREATE) { self = (LibraryTray*)((CREATESTRUCTW*)l)->lpCreateParams; SetWindowLongPtrW(h, GWLP_USERDATA, (LONG_PTR)self); self->hwnd = h; }
    return self ? self->Handle(m, w, l) : DefWindowProcW(h, m, w, l);
}

LRESULT LibraryTray::Handle(UINT m, WPARAM w, LPARAM l) {
    switch (m) {
    case WM_SIZE: UpdateScroll(); return 0;
    case WM_ERASEBKGND: return 1;
    case WM_PAINT: { PAINTSTRUCT ps; HDC dc = BeginPaint(hwnd, &ps); Paint(dc); EndPaint(hwnd, &ps); return 0; }
    case WM_GETDLGCODE: return DLGC_WANTARROWS | DLGC_WANTCHARS;
    case WM_HSCROLL: case WM_VSCROLL: {
        SCROLLINFO si{ sizeof(si), SIF_ALL }; GetScrollInfo(hwnd, m == WM_HSCROLL ? SB_HORZ : SB_VERT, &si);
        switch (LOWORD(w)) {
        case SB_LINELEFT: scroll_ -= 60; break; case SB_LINERIGHT: scroll_ += 60; break;
        case SB_PAGELEFT: scroll_ -= si.nPage; break; case SB_PAGERIGHT: scroll_ += si.nPage; break;
        case SB_THUMBTRACK: case SB_THUMBPOSITION: scroll_ = si.nTrackPos; break;
        }
        UpdateScroll();
        return 0;
    }
    case WM_MOUSEWHEEL: scroll_ -= GET_WHEEL_DELTA_WPARAM(w); UpdateScroll(); return 0;
    case WM_MOUSEMOVE: {
        POINT p{ GET_X_LPARAM(l), GET_Y_LPARAM(l) };
        if (band_) { UpdateBand(p); return 0; }
        if ((w & MK_LBUTTON) && downItem_ >= 0 &&
            (std::abs(p.x - downPt_.x) > GetSystemMetrics(SM_CXDRAG) || std::abs(p.y - downPt_.y) > GetSystemMetrics(SM_CYDRAG))) {
            int item = downItem_;
            downItem_ = -2;                    // o botão solto depois do arraste não abre o item
            if (GetCapture() == hwnd) ReleaseCapture();
            StartDrag(item);
            return 0;
        }
        int hv = Hit(p);
        int hc = -1;
        if (hv >= 0) { RECT x = CloseRect(hv); if (PtInRect(&x, p)) hc = hv; }
        int hh = -1;
        for (int k = 0; k < 4; k++) {
            if (k < 3 && std::count(sel_.begin(), sel_.end(), true) <= 1) continue;
            RECT b = HeaderButton(k); if (PtInRect(&b, p)) hh = k;
        }
        if (hv != hover_ || hc != hoverClose_ || hh != hoverHeader_) { hover_ = hv; hoverClose_ = hc; hoverHeader_ = hh; InvalidateRect(hwnd, nullptr, FALSE); }
        SetCursor(LoadCursor(nullptr, hc >= 0 || hh >= 0 || hv >= 0 ? IDC_HAND : IDC_ARROW));
        TRACKMOUSEEVENT t{ sizeof(t), TME_LEAVE, hwnd, 0 }; TrackMouseEvent(&t);
        return 0;
    }
    case WM_SETCURSOR: if (LOWORD(l) == HTCLIENT) return TRUE; break;
    case WM_MOUSELEAVE: hover_ = hoverClose_ = hoverHeader_ = -1; InvalidateRect(hwnd, nullptr, FALSE); return 0;
    case WM_CAPTURECHANGED: if (band_ && (HWND)l != hwnd) { band_ = false; InvalidateRect(hwnd, nullptr, FALSE); } return 0;
    case WM_LBUTTONDOWN: {
        SetFocus(hwnd);
        POINT p{ GET_X_LPARAM(l), GET_Y_LPARAM(l) };
        int i = Hit(p);
        downItem_ = -1;
        if (i >= 0) {                          // na miniatura: segura o mouse para o arraste começar mesmo se ele sair da bandeja
            RECT x = CloseRect(i);
            if (!PtInRect(&x, p)) { downItem_ = i; downPt_ = p; SetCapture(hwnd); }
            return 0;
        }
        for (int k = 0; k < 4; k++) {          // botões da faixa de cima: tratados ao soltar
            if (k < 3 && std::count(sel_.begin(), sel_.end(), true) <= 1) continue;
            RECT x = HeaderButton(k); if (PtInRect(&x, p)) return 0;
        }
        // espaço vazio (também a faixa do título, já que a bandeja cheia quase não tem fresta): começa o retângulo de seleção
        bool add = (w & (MK_CONTROL | MK_SHIFT)) != 0;
        bandBase_ = add ? sel_ : std::vector<bool>(items_.size(), false);
        band_ = true;
        bandStart_ = bandEnd_ = mode == Tray ? POINT{ p.x + scroll_, p.y } : POINT{ p.x, p.y + scroll_ };
        sel_ = bandBase_;
        SetCapture(hwnd);
        InvalidateRect(hwnd, nullptr, FALSE);
        return 0;
    }
    case WM_LBUTTONDBLCLK: {
        int i = Hit({ GET_X_LPARAM(l), GET_Y_LPARAM(l) });
        if (i >= 0 && mode == Library && onOpen) onOpen(items_[i].path);      // biblioteca: duplo clique abre
        return 0;
    }
    case WM_LBUTTONUP: {
        POINT p{ GET_X_LPARAM(l), GET_Y_LPARAM(l) };
        if (band_) {
            band_ = false;
            ReleaseCapture();
            for (size_t i = 0; i < sel_.size(); i++) if (sel_[i]) { anchor_ = (int)i; break; }
            InvalidateRect(hwnd, nullptr, FALSE);
            return 0;
        }
        bool dragged = downItem_ == -2;
        downItem_ = -1;
        if (GetCapture() == hwnd) ReleaseCapture();
        if (dragged) return 0;
        for (int k = 0; k < 4; k++) {
            if (k < 3 && std::count(sel_.begin(), sel_.end(), true) <= 1) continue;
            RECT b = HeaderButton(k);
            if (!PtInRect(&b, p)) continue;
            auto s = Selection();
            if (k == 0 && onExport) onExport(s);
            else if (k == 1 && onDelete) onDelete(s);
            else if (k == 2) { std::fill(sel_.begin(), sel_.end(), false); InvalidateRect(hwnd, nullptr, FALSE); }
            else if (k == 3 && onShowLibrary) onShowLibrary();
            return 0;
        }
        int i = Hit(p);
        if (i < 0) return 0;
        RECT x = CloseRect(i);
        if (PtInRect(&x, p)) { if (onDelete) onDelete({ items_[i].path }); return 0; }
        bool ctrl = (w & MK_CONTROL) != 0, shift = (w & MK_SHIFT) != 0;
        if (shift && anchor_ >= 0) {
            if (!ctrl) std::fill(sel_.begin(), sel_.end(), false);
            for (int k = std::min(anchor_, i); k <= std::max(anchor_, i); k++) sel_[k] = true;
        } else if (ctrl) {
            sel_[i] = !sel_[i];
            anchor_ = i;
        } else {
            anchor_ = i;
            std::wstring path = items_[i].path;
            std::fill(sel_.begin(), sel_.end(), false);
            sel_[i] = true;
            if (mode == Tray && onOpen) onOpen(path);        // bandeja: um clique abre
        }
        InvalidateRect(hwnd, nullptr, FALSE);
        return 0;
    }
    case WM_KEYDOWN:
        if (w == VK_DELETE) { auto s = Selection(); if (!s.empty() && onDelete) onDelete(s); return 0; }
        if (w == VK_ESCAPE) { std::fill(sel_.begin(), sel_.end(), false); InvalidateRect(hwnd, nullptr, FALSE); return 0; }
        if (w == VK_RETURN && mode == Library) { auto s = Selection(); if (s.size() == 1 && onOpen) onOpen(s[0]); return 0; }
        if (w == 'A' && GetKeyState(VK_CONTROL) < 0) { SelectAll(); return 0; }
        return 0;
    case WM_DROPFILES: {                       // arquivos do Explorer soltos na bandeja/biblioteca
        HDROP d = (HDROP)w;
        std::vector<std::wstring> files;
        UINT n = DragQueryFileW(d, 0xFFFFFFFF, nullptr, 0);
        for (UINT i = 0; i < n; i++) { wchar_t f[MAX_PATH]; if (DragQueryFileW(d, i, f, MAX_PATH)) files.push_back(f); }
        DragFinish(d);
        if (!files.empty() && onDropFiles && !s_draggingOut) onDropFiles(files);   // soltar de volta na bandeja não duplica
        return 0;
    }
    case WM_RBUTTONUP: {
        int i = Hit({ GET_X_LPARAM(l), GET_Y_LPARAM(l) });
        if (i < 0) return 0;
        if (!sel_[i]) { std::fill(sel_.begin(), sel_.end(), false); sel_[i] = true; anchor_ = i; InvalidateRect(hwnd, nullptr, FALSE); }
        auto s = Selection();
        std::wstring path = items_[i].path;
        HMENU mm = CreatePopupMenu();
        if (s.size() == 1) {
            AppendMenuW(mm, MF_STRING, 1, Tr(L"Abrir"));
            AppendMenuW(mm, MF_STRING, 2, Tr(L"Mostrar na pasta"));
            AppendMenuW(mm, MF_SEPARATOR, 0, nullptr);
        }
        AppendMenuW(mm, MF_STRING, 4, s.size() > 1 ? Format(Tr(L"Exportar %zu itens…"), s.size()).c_str() : Tr(L"Exportar…"));
        AppendMenuW(mm, MF_STRING, 5, Tr(L"Selecionar tudo\tCtrl+A"));
        if (mode == Tray) AppendMenuW(mm, MF_STRING, 6, s.size() > 1 ? Tr(L"Remover da bandeja") : Tr(L"Remover da bandeja (continua na biblioteca)"));
        AppendMenuW(mm, MF_SEPARATOR, 0, nullptr);
        AppendMenuW(mm, MF_STRING, 3, s.size() > 1 ? Format(Tr(L"Excluir %zu itens (Lixeira)\tDel"), s.size()).c_str() : Tr(L"Excluir (Lixeira)\tDel"));
        POINT p; GetCursorPos(&p);
        int cmd = TrackPopupMenu(mm, TPM_RETURNCMD | TPM_RIGHTBUTTON, p.x, p.y, 0, hwnd, nullptr);
        DestroyMenu(mm);
        if (cmd == 1 && onOpen) onOpen(path);
        else if (cmd == 2) ShellExecuteW(nullptr, L"open", L"explorer.exe", (L"/select,\"" + path + L"\"").c_str(), nullptr, SW_SHOWNORMAL);
        else if (cmd == 3 && onDelete) onDelete(s);
        else if (cmd == 4 && onExport) onExport(s);
        else if (cmd == 5) SelectAll();
        else if (cmd == 6) RemoveFromTray(s);
        return 0;
    }
    }
    return DefWindowProcW(hwnd, m, w, l);
}

} // namespace snap::ed
