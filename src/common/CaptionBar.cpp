#include "CaptionBar.h"
#include "Icons.h"
#include "Ui.h"

namespace snap::ui {

HHOOK CaptionBar::hook_ = nullptr;
CaptionBar* CaptionBar::active_ = nullptr;

enum { HitMin = 100, HitMax = 101, HitClose = 102 };

void CaptionBar::Init(HWND frame, std::vector<Menu> menus, bool resizable, bool editorIcon) {
    frame_ = frame; menus_ = std::move(menus); resizable_ = resizable; editorIcon_ = editorIcon;
    // translated names may share an initial: the next menu takes another Latin letter of its own name
    for (size_t i = 1; i < menus_.size(); i++) {
        auto used = [&](wchar_t k) { for (size_t j = 0; j < i; j++) if (menus_[j].key == k) return true; return false; };
        if (!used(menus_[i].key)) continue;
        wchar_t pick = 0;
        for (wchar_t ch : menus_[i].name) {
            wchar_t up = (wchar_t)towupper(ch);
            if (up >= L'A' && up <= L'Z' && !used(up)) { pick = up; break; }
        }
        menus_[i].key = pick;
    }
}

void CaptionBar::Layout(int width, int dpi) {
    width_ = width; dpi_ = dpi;
    h_ = Scale(32, dpi);
    if (iconDpi_ != dpi) {
        if (icon_) DestroyIcon(icon_);
        icon_ = icons::MakeAppIcon(Scale(16, dpi), editorIcon_);
        iconDpi_ = dpi;
    }
    HDC dc = GetDC(frame_);
    HGDIOBJ of = SelectObject(dc, UiFont(dpi));
    menuRects_.clear();
    int x = Scale(38, dpi);
    for (auto& m : menus_) {
        RECT t{}; DrawTextW(dc, m.name.c_str(), -1, &t, DT_CALCRECT | DT_SINGLELINE);
        int w = t.right + Scale(16, dpi);
        menuRects_.push_back({ x, Scale(4, dpi), x + w, h_ - Scale(4, dpi) });
        x += w + Scale(2, dpi);
    }
    SelectObject(dc, of);
    ReleaseDC(frame_, dc);
    titleX_ = x + Scale(18, dpi);
    int bw = Scale(46, dpi), r = width;
    btn_[2] = { r - bw, 0, r, h_ }; r -= bw;
    if (resizable_) { btn_[1] = { r - bw, 0, r, h_ }; r -= bw; } else btn_[1] = {};
    btn_[0] = { r - bw, 0, r, h_ };
}

int CaptionBar::Hit(POINT p) const {
    if (p.y < 0 || p.y >= h_) return -1;
    for (int i = 0; i < (int)menuRects_.size(); i++) {
        RECT r = menuRects_[i]; r.top = 0; r.bottom = h_;
        if (PtInRect(&r, p)) return i;
    }
    for (int i = 0; i < 3; i++) if (PtInRect(&btn_[i], p)) return HitMin + i;
    return -1;
}

void CaptionBar::Invalidate() {
    if (!frame_) return;
    RECT r{ 0, 0, width_, h_ };
    InvalidateRect(frame_, &r, TRUE);
}

void CaptionBar::Paint(HDC dc) {
    RECT cap{ 0, 0, width_, h_ };
    HBRUSH bg = CreateSolidBrush(th_.bgCaption); FillRect(dc, &cap, bg); DeleteObject(bg);
    if (!th_.captionAccent) {          // separator line at the bottom
        RECT l{ 0, h_ - 1, width_, h_ };
        HBRUSH lb = CreateSolidBrush(Mix(th_.bgCaption, th_.line, 0.5)); FillRect(dc, &l, lb); DeleteObject(lb);
    }
    int is = Scale(16, dpi_);
    if (icon_) DrawIconEx(dc, Scale(12, dpi_), (h_ - is) / 2, icon_, is, is, 0, nullptr, DI_NORMAL);
    COLORREF hot = th_.captionAccent ? Mix(th_.bgCaption, RGB(255, 255, 255), 0.18) : Mix(th_.bgCaption, th_.accent, 0.16);
    Gdiplus::Graphics gr(dc);
    gr.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
    gr.SetPixelOffsetMode(Gdiplus::PixelOffsetModeHalf);
    for (int i = 0; i < (int)menuRects_.size(); i++) {
        if (!(i == open_ || (open_ < 0 && i == hot_))) continue;
        RECT r = menuRects_[i];
        Gdiplus::GraphicsPath p;
        float d = (float)Scale(4, dpi_) * 2, x = (float)r.left, y = (float)r.top, w = (float)(r.right - r.left), h = (float)(r.bottom - r.top);
        p.AddArc(x, y, d, d, 180, 90); p.AddArc(x + w - d, y, d, d, 270, 90);
        p.AddArc(x + w - d, y + h - d, d, d, 0, 90); p.AddArc(x, y + h - d, d, d, 90, 90);
        p.CloseFigure();
        Gdiplus::SolidBrush b(Gdiplus::Color(255, GetRValue(hot), GetGValue(hot), GetBValue(hot)));
        gr.FillPath(&b, &p);
    }
    HGDIOBJ of = SelectObject(dc, UiFont(dpi_));
    SetBkMode(dc, TRANSPARENT);
    SetTextColor(dc, th_.captionText);
    for (int i = 0; i < (int)menuRects_.size(); i++) {
        RECT r = menuRects_[i];
        DrawTextW(dc, menus_[i].name.c_str(), -1, &r, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
    }
    if (!title_.empty()) {             // document title, subdued, after the menus
        RECT tr{ titleX_, 0, btn_[0].left - Scale(12, dpi_), h_ };
        SetTextColor(dc, Mix(th_.captionText, th_.bgCaption, 0.35));
        DrawTextW(dc, title_.c_str(), -1, &tr, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS);
    }
    SelectObject(dc, of);
    // window buttons
    for (int i = 0; i < 3; i++) {
        if (IsRectEmpty(&btn_[i]) || hot_ != HitMin + i) continue;
        COLORREF c = i == 2 ? (pressed_ == HitClose ? RGB(168, 36, 24) : RGB(196, 43, 28)) : hot;
        HBRUSH b = CreateSolidBrush(c); FillRect(dc, &btn_[i], b); DeleteObject(b);
    }
    COLORREF gc = th_.captionText;
    float pw = std::max(1.0f, dpi_ / 96.0f), s = (float)Scale(10, dpi_) / 2, my = h_ / 2.0f;
    Gdiplus::Pen pen(Gdiplus::Color(255, GetRValue(gc), GetGValue(gc), GetBValue(gc)), pw);
    float mx = (btn_[0].left + btn_[0].right) / 2.0f;
    gr.DrawLine(&pen, mx - s, my, mx + s, my);
    if (resizable_) {
        float cx = (btn_[1].left + btn_[1].right) / 2.0f;
        if (IsZoomed(frame_)) {        // restore: two overlapping squares
            float o = (float)Scale(2, dpi_);
            gr.DrawRectangle(&pen, cx - s + o, my - s, 2 * s - o, 2 * s - o);
            gr.DrawRectangle(&pen, cx - s, my - s + o, 2 * s - o, 2 * s - o);
        } else gr.DrawRectangle(&pen, cx - s, my - s, 2 * s, 2 * s);
    }
    float cx = (btn_[2].left + btn_[2].right) / 2.0f;
    Gdiplus::Color xc = hot_ == HitClose ? Gdiplus::Color(255, 255, 255, 255) : Gdiplus::Color(255, GetRValue(gc), GetGValue(gc), GetBValue(gc));
    Gdiplus::Pen xp(xc, pw);
    gr.DrawLine(&xp, cx - s, my - s, cx + s, my + s);
    gr.DrawLine(&xp, cx + s, my - s, cx - s, my + s);
}

bool CaptionBar::NcCalcSize(WPARAM w, LPARAM l, LRESULT& result) {
    if (!w) return false;
    if (resizable_) {
        // keeps the system resize borders (invisible on Windows 11) and removes only the title strip;
        // when maximized, the window overflows the screen by the frame thickness, so the top moves down by the same amount
        auto* p = (NCCALCSIZE_PARAMS*)l;
        LONG top = p->rgrc[0].top;
        DefWindowProcW(frame_, WM_NCCALCSIZE, w, l);
        p->rgrc[0].top = top;
        if (IsZoomed(frame_)) {
            int dpi = DpiForWindow(frame_);
            p->rgrc[0].top += GetSystemMetricsForDpi(SM_CYFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
        }
    }
    result = 0;
    return true;
}

bool CaptionBar::NcHitTest(LPARAM l, LRESULT& result) {
    if (resizable_) {
        LRESULT d = DefWindowProcW(frame_, WM_NCHITTEST, 0, l);
        if (d != HTCLIENT && d != HTCAPTION && d != HTNOWHERE) { result = d; return true; }   // system borders
    }
    POINT p{ GET_X_LPARAM(l), GET_Y_LPARAM(l) };
    ScreenToClient(frame_, &p);
    if (p.y < 0 || p.y >= h_) { result = HTCLIENT; return true; }
    if (resizable_ && !IsZoomed(frame_) && p.y < Scale(5, dpi_)) {     // top border (the title strip takes its place)
        int c = Scale(12, dpi_);
        result = p.x < c ? HTTOPLEFT : p.x >= width_ - c ? HTTOPRIGHT : HTTOP;
        return true;
    }
    result = Hit(p) >= 0 ? HTCLIENT : HTCAPTION;
    return true;
}

bool CaptionBar::MouseMove(POINT p) {
    int hit = Hit(p);
    if (hit != hot_) { hot_ = hit; Invalidate(); }
    if (!tracking_) { TRACKMOUSEEVENT t{ sizeof t, TME_LEAVE, frame_, 0 }; TrackMouseEvent(&t); tracking_ = true; }
    return p.y >= 0 && p.y < h_;
}

void CaptionBar::MouseLeave() {
    tracking_ = false;
    if (hot_ != -1) { hot_ = -1; Invalidate(); }
}

bool CaptionBar::LButtonDown(POINT p) {
    int hit = Hit(p);
    if (hit >= 0 && hit < (int)menus_.size()) { OpenMenu(hit); return true; }
    if (hit >= HitMin) { pressed_ = hit; SetCapture(frame_); Invalidate(); return true; }
    return p.y >= 0 && p.y < h_;
}

bool CaptionBar::LButtonUp(POINT p) {
    if (pressed_ < 0) return false;
    ReleaseCapture();
    int was = pressed_; pressed_ = -1;
    Invalidate();
    if (Hit(p) == was) {
        if (was == HitMin) ShowWindow(frame_, SW_MINIMIZE);
        else if (was == HitMax) ShowWindow(frame_, IsZoomed(frame_) ? SW_RESTORE : SW_MAXIMIZE);
        else PostMessageW(frame_, WM_CLOSE, 0, 0);
    }
    return true;
}

bool CaptionBar::RButtonUp(WPARAM w, LPARAM l) {
    if (w != HTCAPTION) return false;
    HMENU sys = GetSystemMenu(frame_, FALSE);
    int cmd = TrackPopupMenu(sys, TPM_RETURNCMD | TPM_RIGHTBUTTON, GET_X_LPARAM(l), GET_Y_LPARAM(l), 0, frame_, nullptr);
    if (cmd) PostMessageW(frame_, WM_SYSCOMMAND, cmd, 0);
    return true;
}

LRESULT CALLBACK CaptionBar::MenuFilter(int code, WPARAM w, LPARAM l) {
    CaptionBar* s = active_;
    if (code == MSGF_MENU && s) {
        MSG* m = (MSG*)l;
        int n = (int)s->menus_.size();
        if (m->message == WM_MOUSEMOVE) {
            POINT p = m->pt; ScreenToClient(s->frame_, &p);
            int k = s->Hit(p);
            if (k >= 0 && k < n && k != s->open_) { s->next_ = k; EndMenu(); }
        } else if (m->message == WM_KEYDOWN && (m->wParam == VK_LEFT || m->wParam == VK_RIGHT)) {
            s->next_ = (s->open_ + (m->wParam == VK_RIGHT ? 1 : n - 1)) % n;
            EndMenu();
        }
    }
    return CallNextHookEx(hook_, code, w, l);
}

void CaptionBar::OpenMenu(int i) {
    while (i >= 0 && i < (int)menus_.size()) {
        open_ = i; next_ = -1;
        if (onInitMenu) onInitMenu(menus_[i].menu, i);
        Invalidate(); UpdateWindow(frame_);
        RECT r = menuRects_[i];
        MapWindowPoints(frame_, nullptr, (POINT*)&r, 2);
        TPMPARAMS tp{ sizeof tp, r };
        active_ = this;
        hook_ = SetWindowsHookExW(WH_MSGFILTER, MenuFilter, nullptr, GetCurrentThreadId());
        TrackPopupMenuEx(menus_[i].menu, TPM_LEFTALIGN | TPM_TOPALIGN | TPM_VERTICAL, r.left, r.bottom + Scale(2, dpi_), frame_, &tp);
        UnhookWindowsHookEx(hook_); hook_ = nullptr; active_ = nullptr;
        i = next_;
    }
    open_ = -1;
    Invalidate();
}

bool CaptionBar::PreTranslate(const MSG& msg) {
    if (msg.message != WM_SYSKEYDOWN || !frame_) return false;
    if (msg.hwnd != frame_ && !IsChild(frame_, msg.hwnd)) return false;
    if (msg.wParam == VK_F10) { OpenMenu(0); return true; }
    if (!(msg.lParam & (1 << 29))) return false;
    for (int i = 0; i < (int)menus_.size(); i++)
        if (msg.wParam == (WPARAM)menus_[i].key) { OpenMenu(i); return true; }
    return false;
}

} // namespace snap::ui
