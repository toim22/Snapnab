#include "OneClick.h"
#include "Icons.h"
#include "Ui.h"

#ifndef WDA_EXCLUDEFROMCAPTURE
#define WDA_EXCLUDEFROMCAPTURE 0x00000011
#endif

namespace snap {

namespace {

constexpr wchar_t kClass[] = L"SnapnabOneClick";
constexpr UINT_PTR kTimer = 1;
OneClick* g_self = nullptr;

Gdiplus::Color GC(COLORREF c, BYTE a = 255) { return Gdiplus::Color(a, GetRValue(c), GetGValue(c), GetBValue(c)); }

COLORREF Mix(COLORREF a, COLORREF b, double t) {
    auto c = [t](int x, int y) { return (BYTE)std::lround(x + (y - x) * t); };
    return RGB(c(GetRValue(a), GetRValue(b)), c(GetGValue(a), GetGValue(b)), c(GetBValue(a), GetBValue(b)));
}

void RoundPath(Gdiplus::GraphicsPath& p, Gdiplus::RectF r, float rad) {
    float d = std::min({ rad * 2, r.Width, r.Height });
    p.AddArc(r.X, r.Y, d, d, 180, 90);
    p.AddArc(r.X + r.Width - d, r.Y, d, d, 270, 90);
    p.AddArc(r.X + r.Width - d, r.Y + r.Height - d, d, d, 0, 90);
    p.AddArc(r.X, r.Y + r.Height - d, d, d, 90, 90);
    p.CloseFigure();
}

int Sc(int v, int dpi) { return MulDiv(v, dpi, 96); }

} // namespace

int OneClick::Dpi() const {
    UINT dx = 96, dy = 96;
    HMONITOR m = MonitorFromPoint(monPt_, MONITOR_DEFAULTTOPRIMARY);
    using Fn = HRESULT(WINAPI*)(HMONITOR, int, UINT*, UINT*);
    static Fn fn = (Fn)GetProcAddress(LoadLibraryW(L"shcore.dll"), "GetDpiForMonitor");
    if (fn && SUCCEEDED(fn(m, 0, &dx, &dy))) return (int)dx;
    return 96;
}

RECT OneClick::MonitorRect() const {
    MONITORINFO mi{ sizeof mi };
    GetMonitorInfoW(MonitorFromPoint(monPt_, MONITOR_DEFAULTTOPRIMARY), &mi);
    return mi.rcMonitor;
}

SIZE OneClick::PanelSize(int dpi) const {
    bool horiz = edge_ == 0 || edge_ == 2 || dragging_;
    return horiz ? SIZE{ Sc(128, dpi), Sc(74, dpi) } : SIZE{ Sc(74, dpi), Sc(128, dpi) };
}

void OneClick::SetTheme(const OneClickTheme& t) { th_ = t; if (hwnd_ && enabled_) Render(); }

void OneClick::Create() {
    static bool reg = false;
    HINSTANCE hi = GetModuleHandleW(nullptr);
    if (!reg) {
        WNDCLASSW wc{}; wc.lpfnWndProc = Proc; wc.hInstance = hi; wc.hCursor = LoadCursor(nullptr, IDC_ARROW); wc.lpszClassName = kClass;
        RegisterClassW(&wc);
        reg = true;
    }
    g_self = this;
    hwnd_ = CreateWindowExW(WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE, kClass, L"Snapnab One Click",
                            WS_POPUP, 0, 0, 10, 10, nullptr, nullptr, hi, nullptr);
    SetWindowDisplayAffinity(hwnd_, WDA_EXCLUDEFROMCAPTURE);         // never shows up in captures
    tipWnd_ = CreateWindowExW(WS_EX_TOPMOST, TOOLTIPS_CLASSW, nullptr, WS_POPUP | TTS_NOPREFIX | TTS_ALWAYSTIP,
                              0, 0, 0, 0, hwnd_, nullptr, hi, nullptr);
    TOOLINFOW ti{ sizeof ti };
    ti.uFlags = TTF_TRACK | TTF_ABSOLUTE;
    ti.hwnd = hwnd_; ti.uId = 1;
    ti.lpszText = LPSTR_TEXTCALLBACKW;
    SendMessageW(tipWnd_, TTM_ADDTOOLW, 0, (LPARAM)&ti);
}

void OneClick::Show(bool on) {
    enabled_ = on;
    if (!on) {
        if (hwnd_) { KillTimer(hwnd_, kTimer); ShowTip(false); ShowWindow(hwnd_, SW_HIDE); }
        return;
    }
    if (!hwnd_) Create();
    edge_ = std::clamp((int)RegGetDword(L"OneClickEdge", 0), 0, 3);
    pos_ = std::clamp(RegGetDword(L"OneClickPos", 820) / 1000.0, 0.0, 1.0);
    monPt_ = { (LONG)(int)RegGetDword(L"OneClickMonX", 0), (LONG)(int)RegGetDword(L"OneClickMonY", 0) };
    open_ = 0; wantOpen_ = false;
    Render();
    if (!busy_) ShowWindow(hwnd_, SW_SHOWNOACTIVATE);
    SetTimer(hwnd_, kTimer, 30, nullptr);
}

void OneClick::SetBusy(bool busy) {
    busy_ = busy;
    if (!hwnd_ || !enabled_) return;
    if (busy) { ShowTip(false); ShowWindow(hwnd_, SW_HIDE); }
    else { open_ = 0; wantOpen_ = false; Render(); ShowWindow(hwnd_, SW_SHOWNOACTIVATE); }
}

void OneClick::Render() {
    if (!hwnd_) return;
    int dpi = Dpi();
    SIZE ps = PanelSize(dpi);
    int sh = Sc(10, dpi);                 // shadow margin
    int strip = Sc(5, dpi);               // what stays visible when collapsed
    int band = Sc(4, dpi);                // accent-colored strip on the edge side
    RECT mon = MonitorRect();
    int depth = (edge_ == 0 || edge_ == 2) ? ps.cy : ps.cx;
    int vis = strip + (int)std::lround((depth - strip) * open_);   // visible depth of the panel
    // window = visible part of the panel + shadow toward the inside of the screen
    if (dragging_) {
        win_ = { dragWin_.x, dragWin_.y, dragWin_.x + ps.cx + 2 * sh, dragWin_.y + ps.cy + 2 * sh };
        panelAt_ = { sh, sh };
    } else {
        int monW = mon.right - mon.left, monH = mon.bottom - mon.top;
        switch (edge_) {
        case 0: case 2: {
            int w = ps.cx + 2 * sh, h = vis + sh;
            int x = mon.left + (int)std::lround(pos_ * std::max(0, monW - w));
            int y = edge_ == 0 ? mon.top : mon.bottom - h;
            win_ = { x, y, x + w, y + h };
            panelAt_ = { sh, edge_ == 0 ? vis - ps.cy : sh };
            break;
        }
        default: {
            int w = vis + sh, h = ps.cy + 2 * sh;
            int y = mon.top + (int)std::lround(pos_ * std::max(0, monH - h));
            int x = edge_ == 3 ? mon.left : mon.right - w;
            win_ = { x, y, x + w, y + h };
            panelAt_ = { edge_ == 3 ? vis - ps.cx : sh, sh };
            break;
        }
        }
    }
    int W = win_.right - win_.left, H = win_.bottom - win_.top;
    BITMAPINFO bi{};
    bi.bmiHeader = { sizeof(BITMAPINFOHEADER), W, -H, 1, 32, BI_RGB };
    void* bits = nullptr;
    HDC screen = GetDC(nullptr);
    HDC mem = CreateCompatibleDC(screen);
    HBITMAP bmp = CreateDIBSection(screen, &bi, DIB_RGB_COLORS, &bits, nullptr, 0);
    HGDIOBJ ob = SelectObject(mem, bmp);
    {
        Gdiplus::Bitmap canvas(W, H, W * 4, PixelFormat32bppPARGB, (BYTE*)bits);
        Gdiplus::Graphics g(&canvas);
        g.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
        g.SetPixelOffsetMode(Gdiplus::PixelOffsetModeHalf);
        g.Clear(Gdiplus::Color(0, 0, 0, 0));
        float rad = (float)Sc(12, dpi);
        Gdiplus::RectF pr((float)panelAt_.x, (float)panelAt_.y, (float)ps.cx, (float)ps.cy);
        // soft shadow (layers of decreasing opacity)
        for (int i = sh; i >= 1; i -= 1) {
            Gdiplus::RectF s = pr; s.Inflate((float)i, (float)i); s.Offset(0, i * 0.35f);
            Gdiplus::GraphicsPath sp; RoundPath(sp, s, rad + i);
            double t = 1.0 - (double)i / (sh + 1);
            Gdiplus::SolidBrush sb(Gdiplus::Color((BYTE)std::lround(10 * t * t), 0, 0, 0));
            g.FillPath(&sb, &sp);
        }
        Gdiplus::GraphicsPath panel; RoundPath(panel, pr, rad);
        Gdiplus::SolidBrush bg(GC(th_.bg));
        g.FillPath(&bg, &panel);
        // accent strip on the edge side (it is what remains visible when collapsed)
        g.SetClip(&panel);
        Gdiplus::SolidBrush ab(GC(th_.accent));
        Gdiplus::RectF br = pr;
        if (!dragging_) {
            switch (edge_) {
            case 0: br.Y = pr.GetBottom() - band; br.Height = (float)band; break;
            case 2: br.Height = (float)band; break;
            case 1: br.Width = (float)band; break;
            case 3: br.X = pr.GetRight() - band; br.Width = (float)band; break;
            }
            // the strip is on the inner side (opposite the edge), visible while the panel is collapsed
            g.FillRectangle(&ab, br);
        }
        g.ResetClip();
        Gdiplus::Pen outline(GC(th_.line), 1.0f);
        g.DrawPath(&outline, &panel);
        // content: capture circle and gear
        bool horiz = ps.cx > ps.cy;
        float d = (float)Sc(46, dpi), gs = (float)Sc(28, dpi);
        Gdiplus::PointF cc, gc;
        if (horiz) {
            float cy = pr.Y + pr.Height / 2 + (edge_ == 0 && !dragging_ ? -band / 2.0f : edge_ == 2 && !dragging_ ? band / 2.0f : 0);
            cc = { pr.X + pr.Width * 0.40f, cy }; gc = { pr.GetRight() - Sc(24, dpi), cy };
        } else {
            float cx = pr.X + pr.Width / 2 + (edge_ == 3 ? -band / 2.0f : band / 2.0f);
            cc = { cx, pr.Y + pr.Height * 0.40f }; gc = { cx, pr.GetBottom() - Sc(24, dpi) };
        }
        COLORREF cap = th_.capture;
        if (pressed_ == 1 && hot_ == 1) cap = Mix(cap, 0, 0.18);
        else if (hot_ == 1) cap = Mix(cap, RGB(255, 255, 255), 0.12);
        Gdiplus::SolidBrush cs(Gdiplus::Color(40, 0, 0, 0));
        g.FillEllipse(&cs, cc.X - d / 2, cc.Y - d / 2 + Sc(2, dpi), d, d);
        Gdiplus::SolidBrush cb(GC(cap));
        g.FillEllipse(&cb, cc.X - d / 2, cc.Y - d / 2, d, d);
        if (hot_ == 2) {
            Gdiplus::SolidBrush hb(GC(Mix(th_.bg, th_.accent, pressed_ == 2 ? 0.22 : 0.12)));
            g.FillEllipse(&hb, gc.X - gs / 2, gc.Y - gs / 2, gs, gs);
        }
        float is = (float)Sc(16, dpi);
        icons::Draw(g, icons::Gear, Gdiplus::RectF(gc.X - is / 2, gc.Y - is / 2, is, is), GC(RGB(80, 80, 88)));
    }
    POINT src{ 0, 0 }, dst{ win_.left, win_.top };
    SIZE sz{ W, H };
    BLENDFUNCTION bf{ AC_SRC_OVER, 0, 255, AC_SRC_ALPHA };
    UpdateLayeredWindow(hwnd_, screen, &dst, &sz, mem, &src, 0, &bf, ULW_ALPHA);
    SelectObject(mem, ob); DeleteObject(bmp); DeleteDC(mem);
    ReleaseDC(nullptr, screen);
}

int OneClick::Hit(POINT p) const {
    if (!PtInRect(&win_, p)) return -1;
    int dpi = Dpi();
    SIZE ps = PanelSize(dpi);
    int x = p.x - win_.left - panelAt_.x, y = p.y - win_.top - panelAt_.y;
    if (x < 0 || y < 0 || x >= ps.cx || y >= ps.cy) return -1;
    bool horiz = ps.cx > ps.cy;
    int band = Sc(4, dpi);
    double cx, cy, gx, gy;
    if (horiz) {
        cy = ps.cy / 2.0 + (edge_ == 0 && !dragging_ ? -band / 2.0 : edge_ == 2 && !dragging_ ? band / 2.0 : 0);
        cx = ps.cx * 0.40; gx = ps.cx - Sc(24, dpi); gy = cy;
    } else {
        cx = ps.cx / 2.0 + (edge_ == 3 ? -band / 2.0 : band / 2.0);
        cy = ps.cy * 0.40; gx = cx; gy = ps.cy - Sc(24, dpi);
    }
    double r = Sc(23, dpi), gr = Sc(14, dpi);
    if ((x - cx) * (x - cx) + (y - cy) * (y - cy) <= r * r) return 1;
    if ((x - gx) * (x - gx) + (y - gy) * (y - gy) <= gr * gr) return 2;
    return 0;
}

void OneClick::ShowTip(bool on) {
    if (!tipWnd_ || on == tipOn_) return;
    tipOn_ = on;
    TOOLINFOW ti{ sizeof ti }; ti.hwnd = hwnd_; ti.uId = 1;
    if (on) {
        int dpi = Dpi();
        POINT p{ win_.left + panelAt_.x + PanelSize(dpi).cx / 2, win_.bottom + Sc(4, dpi) };
        if (edge_ == 2) p.y = win_.top - Sc(28, dpi);
        if (edge_ == 1) { p.x = win_.left - Sc(120, dpi); p.y = (win_.top + win_.bottom) / 2; }
        if (edge_ == 3) { p.x = win_.right + Sc(4, dpi); p.y = (win_.top + win_.bottom) / 2; }
        SendMessageW(tipWnd_, TTM_TRACKPOSITION, 0, MAKELPARAM(p.x, p.y));
    }
    SendMessageW(tipWnd_, TTM_TRACKACTIVATE, on, (LPARAM)&ti);
}

void OneClick::Tick() {
    if (!enabled_ || busy_) return;
    POINT p; GetCursorPos(&p);
    int hit = Hit(p);
    bool inside = hit >= 0 || dragging_;
    ULONGLONG now = GetTickCount64();
    if (inside) { wantOpen_ = true; leftAt_ = 0; }
    else if (!menu_) {
        if (!leftAt_) leftAt_ = now;
        if (now - leftAt_ > 450) wantOpen_ = false;
    }
    bool dirty = false;
    double target = wantOpen_ ? 1.0 : 0.0;
    if (open_ != target) {
        double step = 0.2;
        open_ = std::abs(target - open_) <= step ? target : open_ + (target > open_ ? step : -step);
        dirty = true;
    }
    int h = open_ >= 1.0 && !dragging_ ? std::max(hit, 0) : 0;
    if (h != hot_) { hot_ = h; dirty = true; }
    ShowTip(hot_ == 1 && !down_);
    if (dirty) Render();
}

void OneClick::Snap(POINT c) {
    // edge closest to the monitor where the mouse dropped the panel
    monPt_ = c;
    RECT mon = MonitorRect();
    int cx = (win_.left + win_.right) / 2, cy = (win_.top + win_.bottom) / 2;
    int dist[4] = { cy - mon.top, mon.right - cx, mon.bottom - cy, cx - mon.left };
    edge_ = (int)(std::min_element(dist, dist + 4) - dist);
    int W = win_.right - win_.left, H = win_.bottom - win_.top;
    if (edge_ == 0 || edge_ == 2) pos_ = std::clamp((double)(win_.left - mon.left) / std::max(1L, (mon.right - mon.left) - W), 0.0, 1.0);
    else pos_ = std::clamp((double)(win_.top - mon.top) / std::max(1L, (mon.bottom - mon.top) - H), 0.0, 1.0);
    RegSetDword(L"OneClickEdge", edge_);
    RegSetDword(L"OneClickPos", (DWORD)std::lround(pos_ * 1000));
    RegSetDword(L"OneClickMonX", (DWORD)monPt_.x);
    RegSetDword(L"OneClickMonY", (DWORD)monPt_.y);
}

LRESULT CALLBACK OneClick::Proc(HWND h, UINT m, WPARAM w, LPARAM l) {
    OneClick* s = g_self;
    if (!s) return DefWindowProcW(h, m, w, l);
    switch (m) {
    case WM_MOUSEACTIVATE: return MA_NOACTIVATE;
    case WM_TIMER: if (w == kTimer) s->Tick(); return 0;
    case WM_NOTIFY: {
        auto* nm = (NMTTDISPINFOW*)l;
        if (nm->hdr.code == TTN_GETDISPINFOW) { nm->lpszText = const_cast<wchar_t*>(Tr(s->tip_.c_str())); return 0; }
        break;
    }
    case WM_LBUTTONDOWN: {
        POINT p; GetCursorPos(&p);
        s->down_ = true;
        s->pressed_ = std::max(s->Hit(p), 0);
        s->dragStart_ = p;
        s->dragWin_ = { s->win_.left, s->win_.top };
        SetCapture(h);
        s->ShowTip(false);
        s->Render();
        return 0;
    }
    case WM_MOUSEMOVE: {
        if (!s->down_) return 0;
        POINT p; GetCursorPos(&p);
        int th = Sc(5, s->Dpi());
        if (!s->dragging_ && (std::abs(p.x - s->dragStart_.x) > th || std::abs(p.y - s->dragStart_.y) > th)) {
            // dragging starts: the panel comes loose, whole, under the mouse
            s->dragging_ = true;
            int dpi = s->Dpi(), sh = Sc(10, dpi);
            SIZE ps = s->PanelSize(dpi);
            s->dragWin_ = { p.x - ps.cx / 2 - sh, p.y - ps.cy / 2 - sh };
            s->dragStart_ = p;
            s->hot_ = 0;
        }
        if (s->dragging_) {
            POINT o{ s->dragWin_.x + p.x - s->dragStart_.x, s->dragWin_.y + p.y - s->dragStart_.y };
            s->dragWin_ = o; s->dragStart_ = p;
            s->monPt_ = p;
            s->Render();
        }
        return 0;
    }
    case WM_LBUTTONUP: case WM_CAPTURECHANGED: {
        if (!s->down_) return 0;
        s->down_ = false;
        if (m == WM_LBUTTONUP) ReleaseCapture();
        POINT p; GetCursorPos(&p);
        if (s->dragging_) {
            s->Snap(p);
            s->dragging_ = false;
            s->open_ = 1;
            s->pressed_ = 0;
            s->Render();
            return 0;
        }
        int hit = s->Hit(p), was = s->pressed_;
        s->pressed_ = 0;
        s->Render();
        if (m != WM_LBUTTONUP || hit != was) return 0;
        if (hit == 1 && s->onCapture) {
            s->wantOpen_ = false; s->open_ = 0; s->hot_ = 0; s->Render();
            s->onCapture();
        } else if (hit == 2 && s->onMenu) {
            int dpi = s->Dpi();
            POINT mp{ s->win_.left + s->panelAt_.x + s->PanelSize(dpi).cx - Sc(24, dpi), s->win_.bottom };
            if (s->edge_ == 2) mp.y = s->win_.top;
            s->menu_ = true;
            s->onMenu(mp);
            s->menu_ = false;
        }
        return 0;
    }
    case WM_DPICHANGED: s->Render(); return 0;
    case WM_DISPLAYCHANGE: s->Render(); return 0;
    }
    return DefWindowProcW(h, m, w, l);
}

} // namespace snap
