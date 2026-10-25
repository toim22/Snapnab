#include "Panoramic.h"
#include "Ui.h"

#ifndef WDA_EXCLUDEFROMCAPTURE
#define WDA_EXCLUDEFROMCAPTURE 0x00000011
#endif

namespace snap {

// ---------------------------------------------------------------- junção dos quadros
namespace {

// assinatura de cada linha (ou coluna); linhas de cor única recebem 0 e não contam na comparação
std::vector<uint64_t> Signatures(const Image& img, bool rows) {
    int n = rows ? img.h : img.w, len = rows ? img.w : img.h;
    std::vector<uint64_t> sig(n);
    for (int i = 0; i < n; i++) {
        uint64_t h = 1469598103934665603ull;
        uint32_t first = rows ? img.px[(size_t)i * img.w] : img.px[i];
        bool uniform = true;
        for (int k = 0; k < len; k++) {
            uint32_t p = (rows ? img.px[(size_t)i * img.w + k] : img.px[(size_t)k * img.w + i]) & 0xFFFFFF;
            uniform &= p == (first & 0xFFFFFF);
            h = (h ^ p) * 1099511628211ull;
        }
        sig[i] = uniform ? 0 : (h | 1);
    }
    return sig;
}

} // namespace

// deslocamento d tal que a linha y do quadro novo = linha y + d do anterior (d > 0: o conteúdo subiu)
bool PanoramaStitcher::Offset(const Image& a, const Image& b, bool vertical, int& d) const {
    auto sa = Signatures(a, vertical), sb = Signatures(b, vertical);
    int n = (int)sa.size();
    int minOverlap = std::max(12, n / 6);
    auto score = [&](int dd, int& used) {
        int match = 0; used = 0;
        for (int y = std::max(0, -dd); y < std::min(n, n - dd); y++) {
            if (!sb[y] || !sa[y + dd]) continue;
            used++;
            if (sb[y] == sa[y + dd]) match++;
        }
        return used ? (double)match / used : 0.0;
    };
    int used0 = 0;
    if (score(0, used0) > 0.97 && used0 >= 4) return false;          // nada se moveu
    double best = 0; int bestD = 0, bestUsed = 0;
    for (int dd = -(n - minOverlap); dd <= n - minOverlap; dd++) {
        if (!dd) continue;
        int used = 0;
        double s = score(dd, used);
        // em empate, o menor deslocamento (rolagens são curtas entre dois quadros)
        if (used >= 8 && (s > best + 1e-9 || (std::abs(s - best) < 1e-9 && std::abs(dd) < std::abs(bestD)))) {
            best = s; bestD = dd; bestUsed = used;
        }
    }
    if (best < 0.85 || bestUsed < 8) return false;
    d = bestD;
    return true;
}

bool PanoramaStitcher::Add(const Image& f) {
    if (f.empty()) return false;
    if (canvas_.empty()) { canvas_ = f; prev_ = f; posX_ = posY_ = 0; return true; }
    if (f.w != prev_.w || f.h != prev_.h) return false;
    int d = 0;
    bool vertical = true;
    bool found = false;
    if (axis_ != 2) found = Offset(prev_, f, true, d);
    if (!found && axis_ != 1) { found = Offset(prev_, f, false, d); vertical = !found; }
    if (!found) return false;
    if (!axis_) axis_ = vertical ? 1 : 2;
    int nx = posX_ + (vertical ? 0 : d), ny = posY_ + (vertical ? d : 0);
    // aumenta o resultado para caber o quadro na nova posição
    int left = std::min(0, nx), top = std::min(0, ny);
    int right = std::max(canvas_.w, nx + f.w), bottom = std::max(canvas_.h, ny + f.h);
    if (right - left > kMaxSize || bottom - top > kMaxSize) return false;
    if (left < 0 || top < 0 || right > canvas_.w || bottom > canvas_.h) {
        Image big(right - left, bottom - top, 0xFFFFFFFF);
        big.Blit(canvas_, -left, -top, false);
        canvas_ = std::move(big);
        nx -= left; ny -= top;
    }
    // só a faixa que apareceu agora (conteúdo fixo, como cabeçalhos, não se repete)
    RECT strip{ 0, 0, f.w, f.h };
    if (vertical) { if (d > 0) strip.top = f.h - d; else strip.bottom = -d; }
    else { if (d > 0) strip.left = f.w - d; else strip.right = -d; }
    canvas_.Blit(f.Crop(strip), nx + strip.left, ny + strip.top, false);
    posX_ = nx; posY_ = ny;
    prev_ = f;
    return true;
}

// ---------------------------------------------------------------- sessão (moldura + barra de controle)
namespace {
PanoramicSession* g_pano = nullptr;
constexpr UINT_PTR kGrabTimer = 1;
int Sc(int v, int dpi) { return MulDiv(v, dpi, 96); }
}

void PanoramicSession::Start(RECT area, std::function<void(Image)> done) {
    g_pano = this;
    area_ = area; done_ = std::move(done);
    stitch_ = PanoramaStitcher();
    static bool reg = false;
    HINSTANCE hi = GetModuleHandleW(nullptr);
    if (!reg) {
        WNDCLASSW wc{}; wc.lpfnWndProc = BarProc; wc.hInstance = hi; wc.hCursor = LoadCursor(nullptr, IDC_ARROW); wc.lpszClassName = L"SnapnabPanoBar";
        RegisterClassW(&wc);
        WNDCLASSW bc{}; bc.lpfnWndProc = DefWindowProcW; bc.hInstance = hi; bc.hbrBackground = CreateSolidBrush(RGB(232, 120, 30));
        bc.lpszClassName = L"SnapnabPanoBorder";
        RegisterClassW(&bc);
        reg = true;
    }
    // moldura laranja em volta da região (não sai na captura e não intercepta o mouse)
    int t = 3;
    RECT o{ area.left - t, area.top - t, area.right + t, area.bottom + t };
    border_ = CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE,
                              L"SnapnabPanoBorder", L"", WS_POPUP, o.left, o.top, o.right - o.left, o.bottom - o.top,
                              nullptr, nullptr, hi, nullptr);
    SetLayeredWindowAttributes(border_, 0, 235, LWA_ALPHA);
    HRGN outer = CreateRectRgn(0, 0, o.right - o.left, o.bottom - o.top);
    HRGN inner = CreateRectRgn(t, t, o.right - o.left - t, o.bottom - o.top - t);
    CombineRgn(outer, outer, inner, RGN_DIFF);
    DeleteObject(inner);
    SetWindowRgn(border_, outer, FALSE);
    SetWindowDisplayAffinity(border_, WDA_EXCLUDEFROMCAPTURE);
    ShowWindow(border_, SW_SHOWNOACTIVATE);

    bar_ = CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, L"SnapnabPanoBar", Tr(L"Snapnab — rolagem panorâmica"),
                           WS_POPUP, 0, 0, 10, 10, nullptr, nullptr, hi, nullptr);
    int dpi = DpiForWindow(bar_);
    int W = Sc(420, dpi), H = Sc(48, dpi);
    POINT c{ (area.left + area.right) / 2, area.bottom };
    RECT mon = MonitorRectFromPoint(c, true);
    int x = std::clamp((int)(area.left + area.right) / 2 - W / 2, (int)mon.left, (int)mon.right - W);
    int y = area.bottom + Sc(10, dpi);
    if (y + H > mon.bottom) y = area.top - H - Sc(10, dpi);
    if (y < mon.top) y = area.bottom - H - Sc(14, dpi);
    SetWindowPos(bar_, HWND_TOPMOST, x, y, W, H, SWP_NOACTIVATE);
    SetWindowRgn(bar_, CreateRoundRectRgn(0, 0, W + 1, H + 1, Sc(14, dpi), Sc(14, dpi)), FALSE);
    SetWindowDisplayAffinity(bar_, WDA_EXCLUDEFROMCAPTURE);
    int bh = Sc(30, dpi), by = (H - bh) / 2;
    btnCancel_ = { W - Sc(10, dpi) - Sc(92, dpi), by, W - Sc(10, dpi), by + bh };
    btnStop_ = { btnCancel_.left - Sc(8, dpi) - Sc(92, dpi), by, btnCancel_.left - Sc(8, dpi), by + bh };
    RegisterHotKey(bar_, 1, MOD_NOREPEAT, VK_ESCAPE);
    RegisterHotKey(bar_, 2, MOD_NOREPEAT, VK_RETURN);
    ShowWindow(bar_, SW_SHOWNOACTIVATE);
    Grab();
    SetTimer(bar_, kGrabTimer, 90, nullptr);
    Log(L"Panorâmica: iniciada (%dx%d)", area.right - area.left, area.bottom - area.top);
}

void PanoramicSession::Grab() {
    HDC screen = GetDC(nullptr);
    Image f = Image::FromDC(screen, area_);
    ReleaseDC(nullptr, screen);
    f.MakeOpaque();
    if (stitch_.Add(f) && bar_) InvalidateRect(bar_, nullptr, FALSE);
}

void PanoramicSession::Finish(bool ok) {
    if (!bar_) return;
    KillTimer(bar_, kGrabTimer);
    if (ok) Grab();
    UnregisterHotKey(bar_, 1); UnregisterHotKey(bar_, 2);
    DestroyWindow(bar_); bar_ = nullptr;
    if (border_) { DestroyWindow(border_); border_ = nullptr; }
    Image img = ok ? stitch_.result() : Image();
    Log(L"Panorâmica: %s (%dx%d)", ok ? L"concluída" : L"cancelada", img.w, img.h);
    stitch_ = PanoramaStitcher();
    auto done = std::move(done_);
    if (done) done(std::move(img));
}

int PanoramicSession::HitButton(POINT p) const {
    if (PtInRect(&btnStop_, p)) return 1;
    if (PtInRect(&btnCancel_, p)) return 2;
    return 0;
}

void PanoramicSession::PaintBar(HDC hdc) {
    RECT cr; GetClientRect(bar_, &cr);
    HDC mem = CreateCompatibleDC(hdc);
    HBITMAP bb = CreateCompatibleBitmap(hdc, cr.right, cr.bottom);
    HGDIOBJ ob = SelectObject(mem, bb);
    HBRUSH bg = CreateSolidBrush(RGB(32, 32, 34)); FillRect(mem, &cr, bg); DeleteObject(bg);
    int dpi = DpiForWindow(bar_);
    const Image& r = stitch_.result();
    std::wstring t = stitch_.axis() == 0 ? Tr(L"Role o conteúdo dentro da moldura…")
                   : Format(Tr(L"Capturando: %d × %d px"), r.w, r.h);
    RECT tr{ Sc(16, dpi), 0, btnStop_.left - Sc(8, dpi), cr.bottom };
    HGDIOBJ of = SelectObject(mem, ui::UiFont(dpi, 10));
    SetBkMode(mem, TRANSPARENT); SetTextColor(mem, RGB(236, 236, 238));
    DrawTextW(mem, t.c_str(), -1, &tr, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
    SelectObject(mem, of);
    auto button = [&](RECT b, const wchar_t* text, bool primary, bool hot) {
        COLORREF f = primary ? (hot ? RGB(240, 135, 50) : RGB(232, 120, 30)) : (hot ? RGB(70, 70, 76) : RGB(54, 54, 58));
        ui::FillRoundRect(mem, b, Sc(8, dpi), f, f);
        ui::DrawTextCentered(mem, text, b, RGB(255, 255, 255), ui::UiFont(dpi, 9, true));
    };
    button(btnStop_, Tr(L"Concluir"), true, hover_ == 1);
    button(btnCancel_, Tr(L"Cancelar"), false, hover_ == 2);
    BitBlt(hdc, 0, 0, cr.right, cr.bottom, mem, 0, 0, SRCCOPY);
    SelectObject(mem, ob); DeleteObject(bb); DeleteDC(mem);
}

LRESULT CALLBACK PanoramicSession::BarProc(HWND h, UINT m, WPARAM w, LPARAM l) {
    PanoramicSession* s = g_pano;
    if (!s || s->bar_ != h) return DefWindowProcW(h, m, w, l);
    switch (m) {
    case WM_PAINT: { PAINTSTRUCT ps; HDC dc = BeginPaint(h, &ps); s->PaintBar(dc); EndPaint(h, &ps); return 0; }
    case WM_ERASEBKGND: return 1;
    case WM_TIMER: if (w == kGrabTimer) s->Grab(); return 0;
    case WM_HOTKEY: s->Finish(w == 2); return 0;
    case WM_NCHITTEST: {
        POINT p{ GET_X_LPARAM(l), GET_Y_LPARAM(l) }; ScreenToClient(h, &p);
        return s->HitButton(p) ? HTCLIENT : HTCAPTION;
    }
    case WM_MOUSEMOVE: {
        POINT p{ GET_X_LPARAM(l), GET_Y_LPARAM(l) };
        int hv = s->HitButton(p);
        if (hv != s->hover_) { s->hover_ = hv; InvalidateRect(h, nullptr, FALSE); }
        TRACKMOUSEEVENT tme{ sizeof tme, TME_LEAVE, h, 0 }; TrackMouseEvent(&tme);
        return 0;
    }
    case WM_MOUSELEAVE: s->hover_ = 0; InvalidateRect(h, nullptr, FALSE); return 0;
    case WM_LBUTTONUP: {
        POINT p{ GET_X_LPARAM(l), GET_Y_LPARAM(l) };
        int b = s->HitButton(p);
        if (b) s->Finish(b == 1);
        return 0;
    }
    case WM_MOUSEACTIVATE: return MA_NOACTIVATE;
    }
    return DefWindowProcW(h, m, w, l);
}

} // namespace snap
