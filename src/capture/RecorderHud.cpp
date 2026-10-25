#include "RecorderHud.h"
#include "Icons.h"
#include "Ui.h"

#ifndef WDA_EXCLUDEFROMCAPTURE
#define WDA_EXCLUDEFROMCAPTURE 0x00000011
#endif

namespace snap {

enum { BtnRecord = 1, BtnStop, BtnRestart, BtnCancel, BtnMic, BtnSys, BtnCam, BtnSnap };

static RecorderHud* g_hud = nullptr;

void RecorderHud::Layout() {
    int dpi = DpiForWindow(hwnd_);
    int b = ui::Scale(36, dpi), gap = ui::Scale(4, dpi), pad = ui::Scale(8, dpi), sep = ui::Scale(10, dpi);
    buttons_.clear();
    int x = pad, y = pad;
    auto add = [&](int id) { buttons_.push_back({ id, { x, y, x + b, y + b } }); x += b + gap; };
    add(BtnRecord); add(BtnStop); add(BtnRestart); add(BtnCancel);
    x += sep; add(BtnMic); add(BtnSys);
    if (camAvailable) add(BtnCam);
    x += sep; add(BtnSnap);
}

void RecorderHud::Show(RECT area, Recorder* rec, HudCallbacks cb, bool micOpt, bool sysOpt) {
    g_hud = this;
    area_ = area; rec_ = rec; cb_ = std::move(cb);
    micOn = micOpt; sysOn = sysOpt;
    static bool reg = false;
    HINSTANCE hi = GetModuleHandleW(nullptr);
    if (!reg) {
        WNDCLASSW wc{}; wc.lpfnWndProc = Proc; wc.hInstance = hi; wc.hCursor = LoadCursor(nullptr, IDC_ARROW);
        wc.lpszClassName = L"SnapnabRecorderHud";
        RegisterClassW(&wc);
        WNDCLASSW bc{}; bc.lpfnWndProc = DefWindowProcW; bc.hInstance = hi; bc.hbrBackground = CreateSolidBrush(RGB(235, 60, 60));
        bc.lpszClassName = L"SnapnabRecorderBorder";
        RegisterClassW(&bc);
        WNDCLASSW cc{}; cc.lpfnWndProc = CountProc; cc.hInstance = hi; cc.lpszClassName = L"SnapnabCountdown";
        RegisterClassW(&cc);
        reg = true;
    }
    // moldura vermelha em volta da área (fora dela, não aparece na gravação)
    int t = 3;
    RECT o{ area.left - t, area.top - t, area.right + t, area.bottom + t };
    border_ = CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE,
                              L"SnapnabRecorderBorder", L"", WS_POPUP, o.left, o.top, o.right - o.left, o.bottom - o.top,
                              nullptr, nullptr, hi, nullptr);
    SetLayeredWindowAttributes(border_, 0, 230, LWA_ALPHA);
    HRGN outer = CreateRectRgn(0, 0, o.right - o.left, o.bottom - o.top);
    HRGN inner = CreateRectRgn(t, t, o.right - o.left - t, o.bottom - o.top - t);
    CombineRgn(outer, outer, inner, RGN_DIFF);
    DeleteObject(inner);
    SetWindowRgn(border_, outer, FALSE);
    SetWindowDisplayAffinity(border_, WDA_EXCLUDEFROMCAPTURE);
    ShowWindow(border_, SW_SHOWNOACTIVATE);

    // HUD abaixo da área; se não couber, acima; senão dentro, no canto inferior
    hwnd_ = CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, L"SnapnabRecorderHud", Tr(L"Snapnab — gravação"),
                            WS_POPUP, 0, 0, 10, 10, nullptr, nullptr, hi, nullptr);
    Layout();
    int dpi = DpiForWindow(hwnd_);
    int W = buttons_.back().r.right + ui::Scale(210, dpi), H = ui::Scale(52, dpi);
    POINT c{ (area.left + area.right) / 2, area.bottom };
    RECT mon = MonitorRectFromPoint(c, true);
    int x = std::clamp((int)(area.left + area.right) / 2 - W / 2, (int)mon.left, (int)mon.right - W);
    int y = area.bottom + ui::Scale(10, dpi);
    if (y + H > mon.bottom) y = area.top - H - ui::Scale(10, dpi);
    if (y < mon.top) y = area.bottom - H - ui::Scale(16, dpi);
    SetWindowPos(hwnd_, HWND_TOPMOST, x, y, W, H, SWP_NOACTIVATE);
    HRGN rr = CreateRoundRectRgn(0, 0, W + 1, H + 1, ui::Scale(14, dpi), ui::Scale(14, dpi));
    SetWindowRgn(hwnd_, rr, FALSE);
    SetWindowDisplayAffinity(hwnd_, WDA_EXCLUDEFROMCAPTURE);     // o HUD não sai no vídeo
    ShowWindow(hwnd_, SW_SHOWNOACTIVATE);
    SetTimer(hwnd_, 1, 150, nullptr);
}

void RecorderHud::Hide() {
    if (hwnd_) { KillTimer(hwnd_, 1); DestroyWindow(hwnd_); hwnd_ = nullptr; }
    if (border_) { DestroyWindow(border_); border_ = nullptr; }
    if (count_) { DestroyWindow(count_); count_ = nullptr; }
}

void RecorderHud::Refresh() { if (hwnd_) InvalidateRect(hwnd_, nullptr, FALSE); }

int RecorderHud::HitButton(POINT p) {
    for (auto& b : buttons_) if (PtInRect(&b.r, p)) return b.id;
    return -1;
}

void RecorderHud::Paint(HDC hdc) {
    RECT cr; GetClientRect(hwnd_, &cr);
    HDC mem = CreateCompatibleDC(hdc);
    HBITMAP bb = CreateCompatibleBitmap(hdc, cr.right, cr.bottom);
    HGDIOBJ ob = SelectObject(mem, bb);
    HBRUSH bg = CreateSolidBrush(RGB(30, 30, 32));
    FillRect(mem, &cr, bg); DeleteObject(bg);
    int dpi = DpiForWindow(hwnd_);
    Recorder::State st = rec_ ? rec_->state() : Recorder::Idle;
    {
        Gdiplus::Graphics g(mem);
        g.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
        for (auto& b : buttons_) {
            bool enabled = true;
            icons::Id id = icons::Record;
            Gdiplus::Color col(255, 235, 235, 235);
            switch (b.id) {
            case BtnRecord:
                if (st == Recorder::Recording) id = icons::Pause;
                else { id = icons::Record; col = Gdiplus::Color(255, 240, 70, 70); }
                break;
            case BtnStop: id = icons::Stop; enabled = st != Recorder::Idle; break;
            case BtnRestart: id = icons::Restart; enabled = st != Recorder::Idle; break;
            case BtnCancel: id = icons::Cancel; break;
            case BtnMic:
                id = (rec_ && rec_->micMuted()) || !micOn ? icons::MicOff : icons::Mic;
                enabled = st == Recorder::Idle ? true : rec_->hasMic();
                break;
            case BtnSys:
                id = (rec_ && rec_->sysMuted()) || !sysOn ? icons::SpeakerOff : icons::Speaker;
                enabled = st == Recorder::Idle ? true : rec_->hasSys();
                break;
            case BtnCam: id = camOn ? icons::Webcam : icons::WebcamOff; break;
            case BtnSnap: id = icons::Camera; enabled = st != Recorder::Idle; break;
            }
            if (hover_ == b.id && enabled) {
                Gdiplus::SolidBrush hb(Gdiplus::Color(255, 60, 60, 66));
                g.FillRectangle(&hb, (INT)b.r.left, (INT)b.r.top, (INT)(b.r.right - b.r.left), (INT)(b.r.bottom - b.r.top));
            }
            if (!enabled) col = Gdiplus::Color(255, 100, 100, 100);
            int in = ui::Scale(8, dpi);
            icons::Draw(g, id, Gdiplus::RectF((float)b.r.left + in, (float)b.r.top + in, (float)(b.r.right - b.r.left - 2 * in),
                                              (float)(b.r.bottom - b.r.top - 2 * in)), col);
        }
        // medidores de nível (mic e sistema)
        int x0 = buttons_.back().r.right + ui::Scale(14, dpi);
        auto meter = [&](int y, float lv, bool on) {
            Gdiplus::SolidBrush back(Gdiplus::Color(255, 60, 60, 60));
            int w = ui::Scale(60, dpi), h = ui::Scale(5, dpi);
            g.FillRectangle(&back, x0, y, w, h);
            Gdiplus::SolidBrush fg(lv > 0.9f ? Gdiplus::Color(255, 240, 70, 70) : Gdiplus::Color(255, 80, 200, 110));
            if (on) g.FillRectangle(&fg, x0, y, (INT)(w * std::min(1.0f, lv)), h);
        };
        meter(ui::Scale(17, dpi), rec_ ? rec_->micLevel() : 0, rec_ && !rec_->micMuted());
        meter(ui::Scale(29, dpi), rec_ ? rec_->sysLevel() : 0, rec_ && !rec_->sysMuted());
    }
    // tempo
    double e = rec_ ? rec_->Elapsed() : 0;
    int s = (int)e;
    std::wstring t = Format(L"%02d:%02d:%02d", s / 3600, (s / 60) % 60, s % 60);
    if (st == Recorder::Paused) t += L"  ❚❚";
    else if (st == Recorder::Idle) t = Tr(L"Pronto");
    RECT tr{ buttons_.back().r.right + ui::Scale(84, dpi), 0, cr.right - ui::Scale(8, dpi), cr.bottom };
    if (st == Recorder::Recording && ((GetTickCount64() / 500) % 2)) {
        RECT dot{ tr.left - ui::Scale(4, dpi), cr.bottom / 2 - ui::Scale(4, dpi), tr.left + ui::Scale(4, dpi), cr.bottom / 2 + ui::Scale(4, dpi) };
        ui::FillRoundRect(mem, dot, 8, RGB(240, 70, 70), RGB(240, 70, 70));
    }
    tr.left += ui::Scale(8, dpi);
    HGDIOBJ of = SelectObject(mem, ui::UiFont(dpi, 11, true));
    SetBkMode(mem, TRANSPARENT); SetTextColor(mem, RGB(240, 240, 240));
    DrawTextW(mem, t.c_str(), -1, &tr, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
    SelectObject(mem, of);
    BitBlt(hdc, 0, 0, cr.right, cr.bottom, mem, 0, 0, SRCCOPY);
    SelectObject(mem, ob); DeleteObject(bb); DeleteDC(mem);
}

LRESULT CALLBACK RecorderHud::Proc(HWND h, UINT m, WPARAM w, LPARAM l) {
    RecorderHud* self = g_hud;
    if (!self) return DefWindowProcW(h, m, w, l);
    switch (m) {
    case WM_PAINT: { PAINTSTRUCT ps; HDC dc = BeginPaint(h, &ps); self->Paint(dc); EndPaint(h, &ps); return 0; }
    case WM_ERASEBKGND: return 1;
    case WM_TIMER: InvalidateRect(h, nullptr, FALSE); return 0;
    case WM_NCHITTEST: {
        POINT p{ GET_X_LPARAM(l), GET_Y_LPARAM(l) }; ScreenToClient(h, &p);
        return self->HitButton(p) > 0 ? HTCLIENT : HTCAPTION;     // arrasta pelo fundo
    }
    case WM_MOUSEMOVE: {
        POINT p{ GET_X_LPARAM(l), GET_Y_LPARAM(l) };
        int hv = self->HitButton(p);
        if (hv != self->hover_) { self->hover_ = hv; InvalidateRect(h, nullptr, FALSE); }
        TRACKMOUSEEVENT tme{ sizeof(tme), TME_LEAVE, h, 0 }; TrackMouseEvent(&tme);
        return 0;
    }
    case WM_MOUSELEAVE: self->hover_ = -1; InvalidateRect(h, nullptr, FALSE); return 0;
    case WM_LBUTTONUP: {
        POINT p{ GET_X_LPARAM(l), GET_Y_LPARAM(l) };
        Recorder::State st = self->rec_ ? self->rec_->state() : Recorder::Idle;
        switch (self->HitButton(p)) {
        case BtnRecord:
            if (self->count_) self->SkipCountdown();          // durante a contagem, gravar = começar já
            else if (self->cb_.onRecord) self->cb_.onRecord();
            break;
        case BtnStop: if (st != Recorder::Idle && self->cb_.onStop) self->cb_.onStop(); break;
        case BtnRestart: if (st != Recorder::Idle && self->cb_.onRestart) self->cb_.onRestart(); break;
        case BtnCancel: if (self->cb_.onCancel) self->cb_.onCancel(); break;
        case BtnMic:
            if (st == Recorder::Idle) self->micOn = !self->micOn;
            else if (self->rec_->hasMic()) { self->rec_->SetMicMuted(!self->rec_->micMuted()); if (self->cb_.onMicMute) self->cb_.onMicMute(self->rec_->micMuted()); }
            break;
        case BtnSys:
            if (st == Recorder::Idle) self->sysOn = !self->sysOn;
            else if (self->rec_->hasSys()) { self->rec_->SetSysMuted(!self->rec_->sysMuted()); if (self->cb_.onSysMute) self->cb_.onSysMute(self->rec_->sysMuted()); }
            break;
        case BtnCam:
            self->camOn = !self->camOn;
            if (self->cb_.onWebcam) self->cb_.onWebcam(self->camOn);
            break;
        case BtnSnap: if (st != Recorder::Idle && self->cb_.onSnapshot) self->cb_.onSnapshot(); break;
        }
        InvalidateRect(h, nullptr, FALSE);
        return 0;
    }
    case WM_MOUSEACTIVATE: return MA_NOACTIVATE;
    }
    return DefWindowProcW(h, m, w, l);
}

// contagem regressiva: círculo escuro translúcido com o número e, embaixo, a pílula "Pular" (janela em camadas,
// bordas suavizadas). Só a pílula recebe clique; o resto da janela tem alfa e não atrapalha.
static int CountSize(int dpi) { return ui::Scale(150, dpi); }

RECT RecorderHud::SkipRect() const {
    int dpi = DpiForWindow(count_);
    int s = CountSize(dpi), w = ui::Scale(104, dpi), h = ui::Scale(32, dpi);
    return { (s - w) / 2, s + ui::Scale(14, dpi), (s + w) / 2, s + ui::Scale(14, dpi) + h };
}

void RecorderHud::RenderCount() {
    if (!count_) return;
    RECT wr; GetWindowRect(count_, &wr);
    int W = wr.right - wr.left, H = wr.bottom - wr.top;
    int dpi = DpiForWindow(count_);
    int s = CountSize(dpi);
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
        g.SetTextRenderingHint(Gdiplus::TextRenderingHintAntiAliasGridFit);
        g.SetPixelOffsetMode(Gdiplus::PixelOffsetModeHalf);
        g.Clear(Gdiplus::Color(0, 0, 0, 0));
        Gdiplus::SolidBrush disc(Gdiplus::Color(200, 24, 24, 26));
        g.FillEllipse(&disc, 0.5f, 0.5f, (float)s - 1, (float)s - 1);
        Gdiplus::Pen ring(Gdiplus::Color(90, 255, 255, 255), (float)ui::Scale(2, dpi));
        g.DrawEllipse(&ring, (float)ui::Scale(2, dpi), (float)ui::Scale(2, dpi), (float)s - ui::Scale(4, dpi), (float)s - ui::Scale(4, dpi));
        Gdiplus::FontFamily ff(L"Segoe UI");
        Gdiplus::Font big(&ff, (float)ui::Scale(68, dpi), Gdiplus::FontStyleBold, Gdiplus::UnitPixel);
        Gdiplus::StringFormat sf; sf.SetAlignment(Gdiplus::StringAlignmentCenter); sf.SetLineAlignment(Gdiplus::StringAlignmentCenter);
        Gdiplus::SolidBrush white(Gdiplus::Color(255, 255, 255, 255));
        std::wstring n = std::to_wstring(countLeft_);
        g.DrawString(n.c_str(), -1, &big, Gdiplus::RectF(0, 0, (float)s, (float)s + ui::Scale(4, dpi)), &sf, &white);
        // pílula "Pular"
        RECT k = SkipRect();
        Gdiplus::RectF kr((float)k.left, (float)k.top, (float)(k.right - k.left), (float)(k.bottom - k.top));
        Gdiplus::GraphicsPath pill;
        float d = kr.Height;
        pill.AddArc(kr.X, kr.Y, d, d, 90, 180); pill.AddArc(kr.GetRight() - d, kr.Y, d, d, 270, 180); pill.CloseFigure();
        Gdiplus::SolidBrush pb(skipHot_ ? Gdiplus::Color(235, 60, 60, 66) : Gdiplus::Color(215, 24, 24, 26));
        g.FillPath(&pb, &pill);
        Gdiplus::Pen pr(Gdiplus::Color(skipHot_ ? 150 : 90, 255, 255, 255), 1.0f);
        g.DrawPath(&pr, &pill);
        Gdiplus::Font label(&ff, (float)ui::Scale(13, dpi), Gdiplus::FontStyleBold, Gdiplus::UnitPixel);
        // texto + ícone de avançar (»)
        Gdiplus::RectF tr = kr; tr.Width -= ui::Scale(14, dpi);
        g.DrawString(Tr(L"Pular"), -1, &label, tr, &sf, &white);
        Gdiplus::Pen chev(Gdiplus::Color(255, 255, 255, 255), std::max(1.6f, dpi / 55.0f));
        chev.SetStartCap(Gdiplus::LineCapRound); chev.SetEndCap(Gdiplus::LineCapRound); chev.SetLineJoin(Gdiplus::LineJoinRound);
        float cx = kr.GetRight() - ui::Scale(26, dpi), cy = kr.Y + kr.Height / 2, a = (float)ui::Scale(4, dpi);
        for (int i = 0; i < 2; i++) {
            float x = cx + i * ui::Scale(5, dpi);
            Gdiplus::PointF p[3] = { { x - a / 2, cy - a }, { x + a / 2, cy }, { x - a / 2, cy + a } };
            g.DrawLines(&chev, p, 3);
        }
    }
    POINT src{ 0, 0 }, dst{ wr.left, wr.top };
    SIZE sz{ W, H };
    BLENDFUNCTION bf{ AC_SRC_OVER, 0, 255, AC_SRC_ALPHA };
    UpdateLayeredWindow(count_, screen, &dst, &sz, mem, &src, 0, &bf, ULW_ALPHA);
    SelectObject(mem, ob); DeleteObject(bmp); DeleteDC(mem);
    ReleaseDC(nullptr, screen);
}

void RecorderHud::SkipCountdown() {
    if (!count_) return;
    KillTimer(count_, 1);
    DestroyWindow(count_);
    count_ = nullptr;
    auto then = std::move(countThen_);
    if (then) then();
}

LRESULT CALLBACK RecorderHud::CountProc(HWND h, UINT m, WPARAM w, LPARAM l) {
    RecorderHud* self = g_hud;
    if (!self || self->count_ != h) return DefWindowProcW(h, m, w, l);
    switch (m) {
    case WM_TIMER:
        if (--self->countLeft_ <= 0) self->SkipCountdown();
        else self->RenderCount();
        return 0;
    case WM_MOUSEACTIVATE: return MA_NOACTIVATE;
    case WM_SETCURSOR: {
        POINT p; GetCursorPos(&p); ScreenToClient(h, &p);
        RECT k = self->SkipRect();
        SetCursor(LoadCursor(nullptr, PtInRect(&k, p) ? IDC_HAND : IDC_ARROW));
        return TRUE;
    }
    case WM_MOUSEMOVE: {
        POINT p{ GET_X_LPARAM(l), GET_Y_LPARAM(l) };
        RECT k = self->SkipRect();
        bool hot = PtInRect(&k, p) != FALSE;
        if (hot != self->skipHot_) { self->skipHot_ = hot; self->RenderCount(); }
        TRACKMOUSEEVENT t{ sizeof t, TME_LEAVE, h, 0 }; TrackMouseEvent(&t);
        return 0;
    }
    case WM_MOUSELEAVE: if (self->skipHot_) { self->skipHot_ = false; self->RenderCount(); } return 0;
    case WM_LBUTTONUP: {
        POINT p{ GET_X_LPARAM(l), GET_Y_LPARAM(l) };
        RECT k = self->SkipRect();
        if (PtInRect(&k, p)) self->SkipCountdown();
        return 0;
    }
    }
    return DefWindowProcW(h, m, w, l);
}

void RecorderHud::Countdown(int seconds, std::function<void()> then) {
    if (seconds <= 0) { if (then) then(); return; }
    countLeft_ = seconds;
    countThen_ = std::move(then);
    skipHot_ = false;
    int dpi = DpiForWindow(hwnd_);
    int s = CountSize(dpi);
    int W = s, H = s + ui::Scale(14 + 32 + 2, dpi);
    int cx = (area_.left + area_.right) / 2, cy = (area_.top + area_.bottom) / 2;
    count_ = CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_NOACTIVATE,
                             L"SnapnabCountdown", L"", WS_POPUP, cx - W / 2, cy - s / 2, W, H, nullptr, nullptr,
                             GetModuleHandleW(nullptr), nullptr);
    SetWindowDisplayAffinity(count_, WDA_EXCLUDEFROMCAPTURE);
    RenderCount();
    ShowWindow(count_, SW_SHOWNOACTIVATE);
    SetTimer(count_, 1, 1000, nullptr);
}

} // namespace snap
