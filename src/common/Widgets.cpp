// Controles modernos da janela de captura (ver Widgets.h)
#include "Widgets.h"
#include <dwmapi.h>

namespace snap::mui {

namespace {

constexpr wchar_t kList[] = L"SnapnabComboList";
constexpr UINT WM_MUI_DROP = WM_APP + 399;
constexpr UINT_PTR kAnimTimer = 1;

Palette pal;
HINSTANCE hinst = nullptr;

struct Item { std::wstring text; LPARAM data = 0; UINT flags = 0; };

struct Ctl {
    enum Kind { Toggle, Button, Combo } kind = Button;
    HWND h = nullptr;
    HFONT font = nullptr;
    bool hover = false, pressed = false, checked = false, tracking = false, dropped = false;
    float knob = 0;                 // posição animada da chave (0 = desligada, 1 = ligada)
    int icon = 0;                   // icons::Id + 1
    UINT style = 0;
    HWND tip = nullptr;
    std::vector<Item> items;
    int sel = -1, droppedW = 0;
    bool hasDisplay = false;
    std::wstring display;
    bool gearEnabled = true, gearHover = false;
};

struct List {                       // estado da lista suspensa aberta
    Ctl* owner = nullptr;
    HWND h = nullptr;
    int hot = -1, picked = -1, scroll = 0, contentH = 0, viewH = 0;
    int itemH = 0, sepH = 0, padY = 0, dpi = 96;
    bool done = false;
};

int Sc(int v, int dpi) { return MulDiv(v, dpi, 96); }

COLORREF Mix(COLORREF a, COLORREF b, double t) {
    auto c = [t](int x, int y) { return (BYTE)std::lround(x + (y - x) * t); };
    return RGB(c(GetRValue(a), GetRValue(b)), c(GetGValue(a), GetGValue(b)), c(GetBValue(a), GetBValue(b)));
}

Gdiplus::Color GC(COLORREF c, BYTE a = 255) { return Gdiplus::Color(a, GetRValue(c), GetGValue(c), GetBValue(c)); }

void RoundPath(Gdiplus::GraphicsPath& p, Gdiplus::RectF r, float rad) {
    float d = std::min({ rad * 2, r.Width, r.Height });
    if (d <= 0.5f) { p.AddRectangle(r); return; }
    p.AddArc(r.X, r.Y, d, d, 180, 90);
    p.AddArc(r.X + r.Width - d, r.Y, d, d, 270, 90);
    p.AddArc(r.X + r.Width - d, r.Y + r.Height - d, d, d, 0, 90);
    p.AddArc(r.X, r.Y + r.Height - d, d, d, 90, 90);
    p.CloseFigure();
}

void FillRound(Gdiplus::Graphics& g, Gdiplus::RectF r, float rad, COLORREF fill) {
    Gdiplus::GraphicsPath p; RoundPath(p, r, rad);
    Gdiplus::SolidBrush b(GC(fill)); g.FillPath(&b, &p);
}

void StrokeRound(Gdiplus::Graphics& g, Gdiplus::RectF r, float rad, COLORREF c, float width) {
    Gdiplus::GraphicsPath p; RoundPath(p, r, rad);
    Gdiplus::Pen pen(GC(c), width); g.DrawPath(&pen, &p);
}

Gdiplus::RectF RF(RECT r) { return Gdiplus::RectF((float)r.left, (float)r.top, (float)(r.right - r.left), (float)(r.bottom - r.top)); }

COLORREF ParentBg(HWND h, HDC dc) {
    HBRUSH b = (HBRUSH)SendMessageW(GetParent(h), WM_CTLCOLORBTN, (WPARAM)dc, (LPARAM)h);
    LOGBRUSH lb{};
    if (b && GetObjectW(b, sizeof lb, &lb) == sizeof lb && lb.lbStyle == BS_SOLID) return lb.lbColor;
    return GetSysColor(COLOR_BTNFACE);
}

bool ShowFocus(HWND h) { return GetFocus() == h && !(SendMessageW(h, WM_QUERYUISTATE, 0, 0) & UISF_HIDEFOCUS); }

std::wstring TextOf(HWND h) {
    int n = GetWindowTextLengthW(h);
    std::wstring s(n, L'\0');
    if (n) GetWindowTextW(h, s.data(), n + 1);
    return s;
}

void DrawText_(HDC dc, const std::wstring& t, RECT r, COLORREF c, HFONT f, UINT fmt) {
    HGDIOBJ of = SelectObject(dc, f ? f : GetStockObject(DEFAULT_GUI_FONT));
    SetBkMode(dc, TRANSPARENT);
    SetTextColor(dc, c);
    DrawTextW(dc, t.c_str(), -1, &r, fmt | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | DT_END_ELLIPSIS);
    SelectObject(dc, of);
}

int TextWidth(HDC dc, const std::wstring& t, HFONT f) {
    HGDIOBJ of = SelectObject(dc, f ? f : GetStockObject(DEFAULT_GUI_FONT));
    RECT r{};
    DrawTextW(dc, t.c_str(), -1, &r, DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX);
    SelectObject(dc, of);
    return r.right;
}

void Notify(Ctl* c, WORD code) {
    SendMessageW(GetParent(c->h), WM_COMMAND, MAKEWPARAM(GetDlgCtrlID(c->h), code), (LPARAM)c->h);
}

const COLORREF kText = RGB(28, 28, 32), kTextOff = RGB(158, 158, 164);

// ---------------------------------------------------------------- desenho
void PaintToggle(Ctl* c, HDC dc, RECT rc, COLORREF bg, int dpi) {
    bool en = IsWindowEnabled(c->h) != FALSE;
    float tw = (float)Sc(40, dpi), th = (float)Sc(20, dpi);
    Gdiplus::RectF track(rc.right - tw - Sc(3, dpi), (rc.bottom - th) / 2.0f, tw, th);
    {
        Gdiplus::Graphics g(dc);
        g.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
        g.SetPixelOffsetMode(Gdiplus::PixelOffsetModeHalf);
        COLORREF off = RGB(138, 146, 153);
        COLORREF col = Mix(off, pal.accent, c->knob);
        if (c->hover && en) col = Mix(col, 0, 0.08);
        if (!en) col = Mix(col, bg, 0.55);
        FillRound(g, track, th / 2, col);
        float d = th - Sc(4, dpi);
        float x = track.X + Sc(2, dpi) + c->knob * (tw - Sc(4, dpi) - d);
        Gdiplus::SolidBrush sh(Gdiplus::Color(45, 0, 0, 0));
        g.FillEllipse(&sh, x, track.Y + Sc(2, dpi) + 0.8f, d, d);
        Gdiplus::SolidBrush knob(Gdiplus::Color(255, 255, 255, 255));
        g.FillEllipse(&knob, x, track.Y + Sc(2, dpi), d, d);
        if (ShowFocus(c->h)) {
            Gdiplus::RectF f = track; f.Inflate((float)Sc(3, dpi), (float)Sc(3, dpi));
            StrokeRound(g, f, f.Height / 2, pal.accentText, std::max(1.0f, dpi / 72.0f));
        }
    }
    RECT tr{ 0, 0, (LONG)track.X - Sc(8, dpi), rc.bottom };
    DrawText_(dc, TextOf(c->h), tr, en ? kText : kTextOff, c->font, DT_LEFT);
}

void DrawChevron(Gdiplus::Graphics& g, float cx, float cy, float w, COLORREF c, int dpi, bool up = false) {
    Gdiplus::Pen pen(GC(c), std::max(1.2f, dpi / 70.0f));
    pen.SetStartCap(Gdiplus::LineCapRound); pen.SetEndCap(Gdiplus::LineCapRound); pen.SetLineJoin(Gdiplus::LineJoinRound);
    float hh = w / 4 * (up ? -1 : 1);
    Gdiplus::PointF p[3] = { { cx - w / 2, cy - hh }, { cx, cy + hh }, { cx + w / 2, cy - hh } };
    g.DrawLines(&pen, p, 3);
}

void DrawChevronRight(Gdiplus::Graphics& g, float cx, float cy, float h, COLORREF c, int dpi) {
    Gdiplus::Pen pen(GC(c), std::max(1.6f, dpi / 55.0f));
    pen.SetStartCap(Gdiplus::LineCapRound); pen.SetEndCap(Gdiplus::LineCapRound); pen.SetLineJoin(Gdiplus::LineJoinRound);
    float hw = h / 4;
    Gdiplus::PointF p[3] = { { cx - hw, cy - h / 2 }, { cx + hw, cy }, { cx - hw, cy + h / 2 } };
    g.DrawLines(&pen, p, 3);
}

// "Rótulo            Valor ›": o rótulo alinha com o das chaves, o valor fica na cor de destaque
void PaintLink(Ctl* c, HDC dc, RECT rc, COLORREF bg, int dpi) {
    bool en = IsWindowEnabled(c->h) != FALSE;
    std::wstring value = c->display;
    int cw = Sc(14, dpi);                                 // área da seta
    int vw = TextWidth(dc, value, c->font);
    RECT vr{ rc.right - cw - Sc(4, dpi) - vw, 0, rc.right - cw - Sc(4, dpi), rc.bottom };
    COLORREF vc = en ? (c->hover || c->pressed ? Mix(pal.accentText, 0, 0.25) : pal.accentText) : kTextOff;
    {
        Gdiplus::Graphics g(dc);
        g.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
        g.SetPixelOffsetMode(Gdiplus::PixelOffsetModeHalf);
        if (en && (c->hover || c->pressed)) {
            Gdiplus::RectF hr((float)vr.left - Sc(6, dpi), (float)Sc(2, dpi), (float)(rc.right - vr.left + Sc(6, dpi)) - 0.5f,
                              (float)(rc.bottom - Sc(4, dpi)));
            FillRound(g, hr, (float)Sc(4, dpi), Mix(bg, pal.accent, c->pressed ? 0.18 : 0.09));
        }
        DrawChevronRight(g, (float)(rc.right - cw / 2 - Sc(2, dpi)), rc.bottom / 2.0f, (float)Sc(9, dpi), vc, dpi);
        if (ShowFocus(c->h)) {
            Gdiplus::RectF f = RF(rc); f.Inflate(-1.0f, -1.0f);
            StrokeRound(g, f, (float)Sc(4, dpi), pal.accentText, std::max(1.0f, dpi / 72.0f));
        }
    }
    RECT lr{ 0, 0, vr.left - Sc(8, dpi), rc.bottom };
    DrawText_(dc, TextOf(c->h), lr, en ? kText : kTextOff, c->font, DT_LEFT);
    DrawText_(dc, value, vr, vc, c->font, DT_RIGHT);
}

void PaintButton(Ctl* c, HDC dc, RECT rc, COLORREF bg, int dpi) {
    if (c->style & ButtonLink) { PaintLink(c, dc, rc, bg, dpi); return; }
    bool en = IsWindowEnabled(c->h) != FALSE;
    bool flat = (c->style & (ButtonFlat | ButtonIconOnly)) != 0;
    bool iconOnly = (c->style & ButtonIconOnly) != 0;
    std::wstring text = TextOf(c->h);
    {
        Gdiplus::Graphics g(dc);
        g.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
        g.SetPixelOffsetMode(Gdiplus::PixelOffsetModeHalf);
        Gdiplus::RectF r = RF(rc);
        r.Inflate(-0.5f, -0.5f);
        float rad = (float)Sc(4, dpi);
        if (flat) {
            if (en && (c->hover || c->pressed)) FillRound(g, r, rad, Mix(bg, pal.accent, c->pressed ? 0.20 : 0.10));
        } else {
            COLORREF fill = RGB(255, 255, 255);
            if (!en) fill = Mix(bg, RGB(255, 255, 255), 0.45);
            else if (c->pressed) fill = Mix(RGB(255, 255, 255), pal.accent, 0.16);
            else if (c->hover) fill = Mix(RGB(255, 255, 255), pal.accent, 0.07);
            FillRound(g, r, rad, fill);
            StrokeRound(g, r, rad, Mix(pal.line, 0, en && c->hover ? 0.16 : 0.06), 1.0f);
        }
        if (ShowFocus(c->h)) {
            Gdiplus::RectF f = r; f.Inflate(-1.0f, -1.0f);
            StrokeRound(g, f, rad, pal.accentText, std::max(1.0f, dpi / 72.0f));
        }
    }
    int is = Sc(16, dpi), gap = Sc(6, dpi);
    int tw = iconOnly || text.empty() ? 0 : TextWidth(dc, text, c->font);
    int mw = (c->style & ButtonMenu) ? Sc(10, dpi) : 0;  // seta do menu
    int total = (c->icon ? is : 0) + (c->icon && tw ? gap : 0) + tw + (mw ? Sc(3, dpi) + mw : 0);
    int x = (rc.right - total) / 2;
    COLORREF fg = en ? ((c->style & ButtonAccent) ? pal.accentText : kText) : kTextOff;
    if (c->icon) {
        RECT ir{ x, (rc.bottom - is) / 2, x + is, (rc.bottom - is) / 2 + is };
        icons::DrawOnDC(dc, (icons::Id)(c->icon - 1), ir, en ? (flat ? pal.accentText : kText) : kTextOff);
        x += is + gap;
    }
    if (tw) {
        RECT tr{ x, 0, std::min<LONG>(rc.right - Sc(4, dpi) - (mw ? mw + Sc(3, dpi) : 0), x + tw + 1), rc.bottom };
        DrawText_(dc, text, tr, fg, c->font, DT_LEFT);
        x += tw;
    }
    if (mw) {
        Gdiplus::Graphics g(dc);
        g.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
        DrawChevron(g, x + Sc(3, dpi) + mw / 2.0f, rc.bottom / 2.0f + Sc(2, dpi), (float)Sc(7, dpi), fg, dpi);
    }
}

int GearWidth(const Ctl* c, int dpi) { return (c->style & ComboGear) ? Sc(30, dpi) : 0; }

bool InGear(Ctl* c, int x) {
    RECT rc; GetClientRect(c->h, &rc);
    int gw = GearWidth(c, DpiForWindow(c->h));
    return gw && x >= rc.right - gw;
}

void PaintCombo(Ctl* c, HDC dc, RECT rc, COLORREF bg, int dpi) {
    bool en = IsWindowEnabled(c->h) != FALSE;
    float rad = (float)Sc(4, dpi);
    {
        Gdiplus::Graphics g(dc);
        g.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
        g.SetPixelOffsetMode(Gdiplus::PixelOffsetModeHalf);
        Gdiplus::RectF r = RF(rc);
        r.Inflate(-0.5f, -0.5f);
        COLORREF fill = en ? (c->hover && !c->dropped ? RGB(252, 252, 253) : RGB(255, 255, 255)) : Mix(bg, RGB(255, 255, 255), 0.45);
        FillRound(g, r, rad, fill);
        StrokeRound(g, r, rad, Mix(pal.line, 0, en && (c->hover || c->dropped) ? 0.18 : 0.07), 1.0f);
        if (en && (c->dropped || ShowFocus(c->h))) {          // traço de destaque embaixo, como no Windows 11
            Gdiplus::GraphicsPath clip; RoundPath(clip, r, rad);
            g.SetClip(&clip);
            float t = (float)Sc(2, dpi);
            Gdiplus::SolidBrush b(GC(pal.accent));
            g.FillRectangle(&b, r.X, r.Y + r.Height - t, r.Width, t + 1);
            g.ResetClip();
        }
        int gw = GearWidth(c, dpi);
        if (gw && en && c->gearEnabled && c->gearHover) {
            Gdiplus::RectF gr((float)(rc.right - gw + Sc(2, dpi)), (float)Sc(3, dpi), (float)(gw - Sc(5, dpi)), (float)(rc.bottom - Sc(6, dpi)));
            FillRound(g, gr, (float)Sc(4, dpi), Mix(RGB(255, 255, 255), pal.accent, 0.12));
        }
        DrawChevron(g, (float)rc.right - gw - Sc(15, dpi), (rc.bottom) / 2.0f, (float)Sc(9, dpi), en ? RGB(80, 80, 88) : kTextOff, dpi);
    }
    if (int gw = GearWidth(c, dpi)) {
        int is = Sc(16, dpi);
        RECT ir{ rc.right - gw + (gw - is) / 2, (rc.bottom - is) / 2, 0, 0 };
        ir.right = ir.left + is; ir.bottom = ir.top + is;
        icons::DrawOnDC(dc, icons::Gear, ir, en && c->gearEnabled ? RGB(70, 70, 78) : Mix(kTextOff, RGB(255, 255, 255), 0.35));
    }
    std::wstring t = c->hasDisplay ? c->display : (c->sel >= 0 && c->sel < (int)c->items.size() ? c->items[c->sel].text : L"");
    RECT tr{ Sc(10, dpi), 0, rc.right - Sc(28, dpi) - GearWidth(c, dpi), rc.bottom };
    DrawText_(dc, t, tr, en ? kText : kTextOff, c->font, DT_LEFT);
}

void Paint(Ctl* c) {
    PAINTSTRUCT ps;
    HDC dc = BeginPaint(c->h, &ps);
    RECT rc; GetClientRect(c->h, &rc);
    if (rc.right > 0 && rc.bottom > 0) {
        HDC mem = CreateCompatibleDC(dc);
        HBITMAP bmp = CreateCompatibleBitmap(dc, rc.right, rc.bottom);
        HGDIOBJ ob = SelectObject(mem, bmp);
        COLORREF bg = ParentBg(c->h, mem);
        HBRUSH b = CreateSolidBrush(bg); FillRect(mem, &rc, b); DeleteObject(b);
        int dpi = DpiForWindow(c->h);
        switch (c->kind) {
        case Ctl::Toggle: PaintToggle(c, mem, rc, bg, dpi); break;
        case Ctl::Button: PaintButton(c, mem, rc, bg, dpi); break;
        case Ctl::Combo: PaintCombo(c, mem, rc, bg, dpi); break;
        }
        BitBlt(dc, 0, 0, rc.right, rc.bottom, mem, 0, 0, SRCCOPY);
        SelectObject(mem, ob); DeleteObject(bmp); DeleteDC(mem);
    }
    EndPaint(c->h, &ps);
}

// ---------------------------------------------------------------- lista suspensa
bool IsSep(const Ctl* c, int i) { return c->items[i].flags & ItemSeparator; }

int ItemTop(const List& l, int i) {
    int y = l.padY;
    for (int k = 0; k < i; k++) y += IsSep(l.owner, k) ? l.sepH : l.itemH;
    return y;
}

int ItemAt(const List& l, int y) {
    int yy = l.padY - l.scroll;
    for (int k = 0; k < (int)l.owner->items.size(); k++) {
        int hgt = IsSep(l.owner, k) ? l.sepH : l.itemH;
        if (y >= yy && y < yy + hgt) return IsSep(l.owner, k) ? -1 : k;
        yy += hgt;
    }
    return -1;
}

void EnsureVisible(List& l, int i) {
    if (i < 0) return;
    int top = ItemTop(l, i), bot = top + l.itemH;
    if (top - l.padY < l.scroll) l.scroll = std::max(0, top - l.padY);
    else if (bot + l.padY > l.scroll + l.viewH) l.scroll = bot + l.padY - l.viewH;
    l.scroll = std::clamp(l.scroll, 0, std::max(0, l.contentH - l.viewH));
}

void MoveHot(List& l, int dir) {
    int n = (int)l.owner->items.size();
    int i = l.hot;
    for (int step = 0; step < n; step++) {
        i = i < 0 ? (dir > 0 ? 0 : n - 1) : i + dir;
        if (i < 0 || i >= n) return;
        if (!IsSep(l.owner, i)) { l.hot = i; EnsureVisible(l, i); InvalidateRect(l.h, nullptr, FALSE); return; }
    }
}

void ListKey(List& l, WPARAM vk) {
    switch (vk) {
    case VK_UP: MoveHot(l, -1); break;
    case VK_DOWN: MoveHot(l, 1); break;
    case VK_HOME: l.hot = -1; MoveHot(l, 1); break;
    case VK_END: l.hot = -1; MoveHot(l, -1); break;
    case VK_RETURN: case VK_SPACE: if (l.hot >= 0) l.picked = l.hot; l.done = true; break;
    case VK_ESCAPE: case VK_TAB: case VK_F4: l.done = true; break;
    }
}

void PaintList(List& l) {
    PAINTSTRUCT ps;
    HDC dc = BeginPaint(l.h, &ps);
    RECT rc; GetClientRect(l.h, &rc);
    HDC mem = CreateCompatibleDC(dc);
    HBITMAP bmp = CreateCompatibleBitmap(dc, rc.right, rc.bottom);
    HGDIOBJ ob = SelectObject(mem, bmp);
    Ctl* c = l.owner;
    int dpi = l.dpi;
    bool menu = (c->style & ComboMenu) != 0;
    COLORREF bg = Mix(RGB(252, 252, 253), pal.accent, 0.02);
    HBRUSH b = CreateSolidBrush(bg); FillRect(mem, &rc, b); DeleteObject(b);
    int textX = menu ? Sc(30, dpi) : Sc(18, dpi);
    {
        Gdiplus::Graphics g(mem);
        g.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
        g.SetPixelOffsetMode(Gdiplus::PixelOffsetModeHalf);
        for (int i = 0; i < (int)c->items.size(); i++) {
            int y = ItemTop(l, i) - l.scroll;
            if (IsSep(c, i)) {
                Gdiplus::SolidBrush lb(GC(pal.line));
                g.FillRectangle(&lb, (float)Sc(10, dpi), (float)(y + l.sepH / 2), (float)(rc.right - Sc(20, dpi)), 1.0f);
                continue;
            }
            if (y + l.itemH < 0 || y > rc.bottom) continue;
            Gdiplus::RectF r((float)Sc(4, dpi), (float)y + 1, (float)(rc.right - Sc(8, dpi)), (float)l.itemH - 2);
            bool selected = !menu && i == c->sel;
            if (i == l.hot) FillRound(g, r, (float)Sc(4, dpi), Mix(bg, pal.accent, 0.13));
            else if (selected) FillRound(g, r, (float)Sc(4, dpi), Mix(bg, 0, 0.045));
            if (selected) {
                float ih = (float)Sc(14, dpi);
                FillRound(g, Gdiplus::RectF(r.X + Sc(2, dpi), r.Y + (r.Height - ih) / 2, (float)Sc(3, dpi), ih), (float)Sc(2, dpi), pal.accent);
            }
            if (c->items[i].flags & ItemChecked) {     // marca de efeito ativo
                Gdiplus::Pen pen(GC(pal.accentText), std::max(1.5f, dpi / 60.0f));
                pen.SetStartCap(Gdiplus::LineCapRound); pen.SetEndCap(Gdiplus::LineCapRound); pen.SetLineJoin(Gdiplus::LineJoinRound);
                float cx = (float)Sc(17, dpi), cy = y + l.itemH / 2.0f, s = (float)Sc(4, dpi);
                Gdiplus::PointF p[3] = { { cx - s, cy }, { cx - s * 0.3f, cy + s * 0.75f }, { cx + s, cy - s * 0.8f } };
                g.DrawLines(&pen, p, 3);
            }
        }
    }
    for (int i = 0; i < (int)c->items.size(); i++) {
        if (IsSep(c, i)) continue;
        int y = ItemTop(l, i) - l.scroll;
        if (y + l.itemH < 0 || y > rc.bottom) continue;
        RECT tr{ textX, y, rc.right - Sc(12, dpi), y + l.itemH };
        DrawText_(mem, c->items[i].text, tr, kText, c->font, DT_LEFT);
    }
    HBRUSH lb = CreateSolidBrush(Mix(pal.line, 0, 0.05));
    FrameRect(mem, &rc, lb); DeleteObject(lb);
    BitBlt(dc, 0, 0, rc.right, rc.bottom, mem, 0, 0, SRCCOPY);
    SelectObject(mem, ob); DeleteObject(bmp); DeleteDC(mem);
    EndPaint(l.h, &ps);
}

LRESULT CALLBACK ListProc(HWND h, UINT m, WPARAM w, LPARAM lp) {
    List* l = (List*)GetWindowLongPtrW(h, GWLP_USERDATA);
    if (m == WM_NCCREATE) {
        l = (List*)((CREATESTRUCTW*)lp)->lpCreateParams;
        l->h = h;
        SetWindowLongPtrW(h, GWLP_USERDATA, (LONG_PTR)l);
    }
    if (!l) return DefWindowProcW(h, m, w, lp);
    switch (m) {
    case WM_MOUSEACTIVATE: return MA_NOACTIVATE;
    case WM_ERASEBKGND: return 1;
    case WM_PAINT: PaintList(*l); return 0;
    case WM_MOUSEMOVE: {
        int i = ItemAt(*l, GET_Y_LPARAM(lp));
        if (i != l->hot && i >= 0) { l->hot = i; InvalidateRect(h, nullptr, FALSE); }
        return 0;
    }
    case WM_LBUTTONUP: {
        int i = ItemAt(*l, GET_Y_LPARAM(lp));
        if (i >= 0) { l->picked = i; l->done = true; }
        return 0;
    }
    case WM_MOUSEWHEEL: {
        int d = GET_WHEEL_DELTA_WPARAM(w);
        l->scroll = std::clamp(l->scroll - d * l->itemH * 3 / WHEEL_DELTA, 0, std::max(0, l->contentH - l->viewH));
        InvalidateRect(h, nullptr, FALSE);
        return 0;
    }
    }
    return DefWindowProcW(h, m, w, lp);
}

void DropDown(Ctl* c) {
    if (c->dropped || !IsWindowEnabled(c->h)) return;
    Notify(c, CBN_DROPDOWN);                       // o pai pode refazer a lista agora
    if (c->items.empty()) return;
    List l;
    l.owner = c;
    l.dpi = DpiForWindow(c->h);
    l.itemH = Sc(28, l.dpi); l.sepH = Sc(9, l.dpi); l.padY = Sc(4, l.dpi);
    l.contentH = ItemTop(l, (int)c->items.size()) + l.padY;
    RECT cr; GetWindowRect(c->h, &cr);
    HDC dc = GetDC(c->h);
    int tw = 0;
    for (auto& it : c->items) tw = std::max(tw, TextWidth(dc, it.text, c->font));
    ReleaseDC(c->h, dc);
    int width = std::max({ (int)(cr.right - cr.left), c->droppedW, tw + Sc(52, l.dpi) });
    MONITORINFO mi{ sizeof mi };
    GetMonitorInfoW(MonitorFromRect(&cr, MONITOR_DEFAULTTONEAREST), &mi);
    RECT wa = mi.rcWork;
    int gap = Sc(3, l.dpi);
    int below = wa.bottom - cr.bottom - gap * 2, above = cr.top - wa.top - gap * 2;
    bool down = l.contentH <= below || below >= above;
    l.viewH = std::min(l.contentH, down ? below : above);
    int y = down ? cr.bottom + gap : cr.top - gap - l.viewH;
    int x = std::clamp<int>(cr.left, wa.left, std::max<int>(wa.left, wa.right - width));
    l.hot = (c->style & ComboMenu) ? -1 : c->sel;
    if (l.hot >= 0) EnsureVisible(l, l.hot);
    HWND root = GetAncestor(c->h, GA_ROOT);
    HWND lh = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST, kList, L"", WS_POPUP,
                              x, y, width, l.viewH, root, nullptr, hinst, &l);
    if (!lh) return;
    DWORD corner = 3;   // DWMWCP_ROUNDSMALL (Windows 11: cantos arredondados + sombra)
    DwmSetWindowAttribute(lh, 33 /*DWMWA_WINDOW_CORNER_PREFERENCE*/, &corner, sizeof corner);
    c->dropped = true;
    InvalidateRect(c->h, nullptr, FALSE);
    ShowWindow(lh, SW_SHOWNOACTIVATE);
    bool wasActive = GetForegroundWindow() == root;     // trocar de janela (Alt+Tab) fecha a lista
    // laço modal como o de um menu: clique fora fecha, teclado navega
    MSG msg;
    while (!l.done && IsWindow(lh)) {
        if (!GetMessageW(&msg, nullptr, 0, 0)) { PostQuitMessage((int)msg.wParam); break; }
        switch (msg.message) {
        case WM_LBUTTONDOWN: case WM_RBUTTONDOWN: case WM_MBUTTONDOWN: case WM_LBUTTONDBLCLK:
        case WM_NCLBUTTONDOWN: case WM_NCRBUTTONDOWN:
            if (msg.hwnd != lh) { l.done = true; continue; }
            break;
        case WM_KEYDOWN: case WM_SYSKEYDOWN:
            if (msg.message == WM_SYSKEYDOWN && (msg.wParam == VK_UP || msg.wParam == VK_DOWN)) { l.done = true; continue; }
            ListKey(l, msg.wParam);
            continue;
        case WM_KEYUP: case WM_SYSKEYUP: case WM_CHAR: case WM_SYSCHAR:
            continue;
        case WM_MOUSEWHEEL:
            SendMessageW(lh, WM_MOUSEWHEEL, msg.wParam, msg.lParam);
            continue;
        }
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
        HWND fg = GetForegroundWindow();
        if (wasActive && fg != root && fg != lh) l.done = true;
    }
    if (IsWindow(lh)) DestroyWindow(lh);
    c->dropped = false;
    InvalidateRect(c->h, nullptr, FALSE);
    if (l.picked >= 0) {
        c->sel = l.picked;
        Notify(c, CBN_SELCHANGE);
    }
    Notify(c, CBN_CLOSEUP);
}

void StepSelection(Ctl* c, int dir) {
    int n = (int)c->items.size();
    for (int i = c->sel + dir; i >= 0 && i < n; i += dir)
        if (!IsSep(c, i)) { c->sel = i; InvalidateRect(c->h, nullptr, FALSE); Notify(c, CBN_SELCHANGE); return; }
}

// ---------------------------------------------------------------- chave / botão: clique
void Activate(Ctl* c) {
    if (c->kind == Ctl::Toggle) {
        c->checked = !c->checked;
        SetTimer(c->h, kAnimTimer, 15, nullptr);
    }
    InvalidateRect(c->h, nullptr, FALSE);
    Notify(c, BN_CLICKED);
}

LRESULT Handle(Ctl* c, UINT m, WPARAM w, LPARAM lp) {
    HWND h = c->h;
    switch (m) {
    case WM_NCDESTROY:
        SetWindowLongPtrW(h, GWLP_USERDATA, 0);
        delete c;
        return 0;
    case WM_ERASEBKGND: return 1;
    case WM_PAINT: Paint(c); return 0;
    case WM_SETFONT: c->font = (HFONT)w; if (lp) InvalidateRect(h, nullptr, FALSE); return 0;
    case WM_GETFONT: return (LRESULT)c->font;
    case WM_SETTEXT: { LRESULT r = DefWindowProcW(h, m, w, lp); InvalidateRect(h, nullptr, FALSE); return r; }
    case WM_ENABLE: case WM_SETFOCUS: case WM_KILLFOCUS:
        if (m == WM_KILLFOCUS && c->pressed) { c->pressed = false; ReleaseCapture(); }
        InvalidateRect(h, nullptr, FALSE);
        return 0;
    case WM_UPDATEUISTATE: { LRESULT r = DefWindowProcW(h, m, w, lp); InvalidateRect(h, nullptr, FALSE); return r; }
    case WM_GETDLGCODE: {
        MSG* msg = (MSG*)lp;
        LRESULT code = c->kind == Ctl::Combo ? DLGC_WANTARROWS : DLGC_BUTTON;
        if (msg && msg->message == WM_KEYDOWN && msg->wParam == VK_RETURN) code |= DLGC_WANTMESSAGE;
        return code;
    }
    case WM_MOUSEMOVE:
        if (!c->tracking) {
            TRACKMOUSEEVENT t{ sizeof t, TME_LEAVE, h, 0 };
            TrackMouseEvent(&t);
            c->tracking = true;
        }
        if (!c->hover) { c->hover = true; InvalidateRect(h, nullptr, FALSE); }
        if (c->kind == Ctl::Combo) {
            bool g = InGear(c, GET_X_LPARAM(lp));
            if (g != c->gearHover) { c->gearHover = g; InvalidateRect(h, nullptr, FALSE); }
        }
        return 0;
    case WM_MOUSELEAVE:
        c->tracking = false; c->hover = false; c->gearHover = false;
        InvalidateRect(h, nullptr, FALSE);
        return 0;
    case WM_LBUTTONDOWN: case WM_LBUTTONDBLCLK:
        if (GetFocus() != h) SetFocus(h);
        if (c->kind == Ctl::Combo) {
            if (InGear(c, GET_X_LPARAM(lp))) { if (c->gearEnabled) Notify(c, MUI_GEARCLICK); }
            else DropDown(c);
            return 0;
        }
        c->pressed = true;
        SetCapture(h);
        InvalidateRect(h, nullptr, FALSE);
        return 0;
    case WM_LBUTTONUP:
        if (c->pressed) {
            c->pressed = false;
            ReleaseCapture();
            RECT rc; GetClientRect(h, &rc);
            POINT p{ GET_X_LPARAM(lp), GET_Y_LPARAM(lp) };
            if (PtInRect(&rc, p)) Activate(c);
            else InvalidateRect(h, nullptr, FALSE);
        }
        return 0;
    case WM_KEYDOWN:
        if (c->kind == Ctl::Combo) {
            if (w == VK_SPACE || w == VK_RETURN || w == VK_F4) DropDown(c);
            else if (!(c->style & ComboMenu) && (w == VK_UP || w == VK_LEFT)) StepSelection(c, -1);
            else if (!(c->style & ComboMenu) && (w == VK_DOWN || w == VK_RIGHT)) StepSelection(c, 1);
            return 0;
        }
        if (w == VK_RETURN) Activate(c);
        else if (w == VK_SPACE && !c->pressed) { c->pressed = true; InvalidateRect(h, nullptr, FALSE); }
        return 0;
    case WM_KEYUP:
        if (c->kind != Ctl::Combo && w == VK_SPACE && c->pressed) { c->pressed = false; Activate(c); }
        return 0;
    case WM_SYSKEYDOWN:
        if (c->kind == Ctl::Combo && (w == VK_DOWN || w == VK_UP)) { DropDown(c); return 0; }
        break;
    case WM_TIMER:
        if (w == kAnimTimer) {
            float target = c->checked ? 1.0f : 0.0f;
            float step = 0.2f;
            c->knob = std::abs(target - c->knob) <= step ? target : c->knob + (target > c->knob ? step : -step);
            if (c->knob == target) KillTimer(h, kAnimTimer);
            InvalidateRect(h, nullptr, FALSE);
        }
        return 0;
    case WM_MUI_DROP: DropDown(c); return 0;
    // ---- chave / botão
    case BM_GETCHECK: return c->checked ? BST_CHECKED : BST_UNCHECKED;
    case BM_SETCHECK:
        c->checked = w == BST_CHECKED;
        c->knob = c->checked ? 1.0f : 0.0f;
        KillTimer(h, kAnimTimer);
        InvalidateRect(h, nullptr, FALSE);
        return 0;
    case BM_CLICK: Activate(c); return 0;
    // ---- estilo / ícone
    case MUI_SETICON: c->icon = (int)w; InvalidateRect(h, nullptr, FALSE); return 0;
    case MUI_SETSTYLE:
        c->style = (UINT)w;
        if ((c->style & ButtonIconOnly) && !c->tip) {     // botão só com ícone: o texto vira dica
            c->tip = CreateWindowExW(WS_EX_TOPMOST, TOOLTIPS_CLASSW, nullptr, WS_POPUP | TTS_ALWAYSTIP | TTS_NOPREFIX,
                                     0, 0, 0, 0, h, nullptr, hinst, nullptr);
            TOOLINFOW ti{ sizeof ti };
            ti.uFlags = TTF_IDISHWND | TTF_SUBCLASS;
            ti.hwnd = h; ti.uId = (UINT_PTR)h;
            ti.lpszText = LPSTR_TEXTCALLBACKW;
            SendMessageW(c->tip, TTM_ADDTOOLW, 0, (LPARAM)&ti);
        }
        InvalidateRect(h, nullptr, FALSE);
        return 0;
    case WM_NOTIFY: {
        auto* nm = (NMTTDISPINFOW*)lp;
        if (nm->hdr.code == TTN_GETDISPINFOW) {
            static std::wstring tipText;
            tipText = TextOf(h);
            tipText.erase(std::remove(tipText.begin(), tipText.end(), L'…'), tipText.end());
            nm->lpszText = tipText.data();
            return 0;
        }
        break;
    }
    // ---- combo
    case CB_ADDSTRING: c->items.push_back({ (const wchar_t*)lp }); InvalidateRect(h, nullptr, FALSE); return (LRESULT)c->items.size() - 1;
    case CB_RESETCONTENT: c->items.clear(); c->sel = -1; InvalidateRect(h, nullptr, FALSE); return 0;
    case CB_GETCOUNT: return (LRESULT)c->items.size();
    case CB_GETCURSEL: return c->sel;
    case CB_SETCURSEL:
        c->sel = (int)w < (int)c->items.size() ? (int)w : -1;
        InvalidateRect(h, nullptr, FALSE);
        return c->sel < 0 ? CB_ERR : c->sel;
    case CB_SETITEMDATA:
        if ((int)w < 0 || (int)w >= (int)c->items.size()) return CB_ERR;
        c->items[w].data = lp; return 0;
    case CB_GETITEMDATA:
        if ((int)w < 0 || (int)w >= (int)c->items.size()) return CB_ERR;
        return c->items[w].data;
    case CB_SETDROPPEDWIDTH: c->droppedW = (int)w; return w;
    case CB_GETDROPPEDSTATE: return c->dropped;
    case CB_SHOWDROPDOWN: if (w && !c->dropped) PostMessageW(h, WM_MUI_DROP, 0, 0); return TRUE;
    case MUI_SETITEMFLAGS:
        if ((int)w >= 0 && (int)w < (int)c->items.size()) c->items[w].flags = (UINT)lp;
        return 0;
    case MUI_SETDISPLAYTEXT:
        c->hasDisplay = lp != 0;
        c->display = lp ? (const wchar_t*)lp : L"";
        InvalidateRect(h, nullptr, FALSE);
        return 0;
    case MUI_SETGEAR: c->gearEnabled = w != 0; InvalidateRect(h, nullptr, FALSE); return 0;
    }
    return DefWindowProcW(h, m, w, lp);
}

template <Ctl::Kind K>
LRESULT CALLBACK Proc(HWND h, UINT m, WPARAM w, LPARAM lp) {
    Ctl* c = (Ctl*)GetWindowLongPtrW(h, GWLP_USERDATA);
    if (!c) {
        if (m != WM_NCCREATE) return DefWindowProcW(h, m, w, lp);
        c = new Ctl;
        c->kind = K;
        c->h = h;
        SetWindowLongPtrW(h, GWLP_USERDATA, (LONG_PTR)c);
    }
    return Handle(c, m, w, lp);
}

} // namespace

void Register(HINSTANCE hi) {
    hinst = hi;
    WNDCLASSEXW wc{ sizeof wc };
    wc.hInstance = hi;
    wc.hCursor = LoadCursor(nullptr, IDC_ARROW);
    wc.style = CS_DBLCLKS;
    wc.lpfnWndProc = Proc<Ctl::Toggle>; wc.lpszClassName = kToggle; RegisterClassExW(&wc);
    wc.lpfnWndProc = Proc<Ctl::Button>; wc.lpszClassName = kButton; RegisterClassExW(&wc);
    wc.lpfnWndProc = Proc<Ctl::Combo>; wc.lpszClassName = kCombo; RegisterClassExW(&wc);
    wc.lpfnWndProc = ListProc; wc.lpszClassName = kList; RegisterClassExW(&wc);
}

void SetPalette(const Palette& p) { pal = p; }

} // namespace snap::mui
