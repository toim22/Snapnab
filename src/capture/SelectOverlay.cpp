#include "SelectOverlay.h"
#include "Ui.h"
#include <dwmapi.h>
#include <psapi.h>

namespace snap {

RECT WindowFrameRect(HWND h) {
    RECT r{};
    if (FAILED(DwmGetWindowAttribute(h, DWMWA_EXTENDED_FRAME_BOUNDS, &r, sizeof r)) || IsRectEmpty(&r)) GetWindowRect(h, &r);
    return r;
}

std::wstring WindowTitle(HWND h) {
    wchar_t b[512] = L"";
    GetWindowTextW(GetAncestor(h, GA_ROOT), b, 512);
    return b;
}

std::wstring WindowProcessName(HWND h) {
    DWORD pid = 0; GetWindowThreadProcessId(h, &pid);
    HANDLE p = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid);
    if (!p) return L"";
    wchar_t b[MAX_PATH] = L""; DWORD n = MAX_PATH;
    QueryFullProcessImageNameW(p, 0, b, &n);
    CloseHandle(p);
    return FileNameOf(b);
}

namespace {

struct WinInfo { HWND h; RECT r; };

struct Overlay {
    Image frozen;
    RECT vs{};
    SelectMode mode = SelectMode::Region;
    SIZE fixed{ 640, 480 };
    bool video = false;
    std::function<void(const SelectResult&)> done;
    std::vector<WinInfo> windows;
    HBITMAP bright = nullptr, dim = nullptr;
    HDC brightDC = nullptr, dimDC = nullptr;
    HWND hwnd = nullptr;
    POINT cur{};                 // cursor in client coordinates
    bool dragging = false;
    POINT dragStart{};
    RECT hover{};                // highlight under the cursor (client)
    HWND hoverWnd = nullptr, hoverChild = nullptr;
    std::vector<POINT> path;     // client
    std::vector<RECT> rects;     // client
    bool finished = false;
};

RECT Norm(POINT a, POINT b) { return { std::min(a.x, b.x), std::min(a.y, b.y), std::max(a.x, b.x) + 1, std::max(a.y, b.y) + 1 }; }

BOOL CALLBACK EnumProc(HWND h, LPARAM lp) {
    auto* o = (Overlay*)lp;
    if (!IsWindowVisible(h) || IsIconic(h) || h == o->hwnd) return TRUE;
    DWORD pid; GetWindowThreadProcessId(h, &pid);
    if (pid == GetCurrentProcessId()) return TRUE;
    BOOL cloaked = FALSE;
    DwmGetWindowAttribute(h, DWMWA_CLOAKED, &cloaked, sizeof cloaked);
    if (cloaked) return TRUE;
    LONG ex = GetWindowLongW(h, GWL_EXSTYLE);
    if ((ex & WS_EX_TRANSPARENT) && (ex & WS_EX_LAYERED)) return TRUE;
    RECT r = WindowFrameRect(h);
    if (r.right - r.left < 8 || r.bottom - r.top < 8) return TRUE;
    o->windows.push_back({ h, r });
    return TRUE;
}

void UpdateHover(Overlay* o) {
    POINT sp{ o->cur.x + o->vs.left, o->cur.y + o->vs.top };
    o->hover = {}; o->hoverWnd = o->hoverChild = nullptr;
    for (auto& w : o->windows) {
        if (!PtInRect(&w.r, sp)) continue;
        o->hoverWnd = w.h;
        RECT r = w.r;
        if (o->mode == SelectMode::Object || o->mode == SelectMode::Region || o->mode == SelectMode::PickWindow) {
            // goes down to the innermost control (ChildWindowFromPointEx)
            HWND cur = w.h;
            for (int depth = 0; depth < 16; depth++) {
                POINT cp = sp; ScreenToClient(cur, &cp);
                HWND c = ChildWindowFromPointEx(cur, cp, CWP_SKIPINVISIBLE | CWP_SKIPTRANSPARENT | CWP_SKIPDISABLED);
                if (!c || c == cur) break;
                cur = c;
            }
            if (cur != w.h) {
                o->hoverChild = cur;
                if (o->mode != SelectMode::Region) GetWindowRect(cur, &r);
            }
            if (o->mode == SelectMode::PickWindow && !o->hoverChild) o->hoverChild = w.h;
        }
        OffsetRect(&r, -o->vs.left, -o->vs.top);
        o->hover = r;
        return;
    }
    // no window: whole monitor
    RECT m = MonitorRectFromPoint(sp);
    OffsetRect(&m, -o->vs.left, -o->vs.top);
    o->hover = m;
}

RECT FixedRect(Overlay* o) {
    RECT r{ o->cur.x - o->fixed.cx / 2, o->cur.y - o->fixed.cy / 2, 0, 0 };
    r.right = r.left + o->fixed.cx; r.bottom = r.top + o->fixed.cy;
    int W = o->vs.right - o->vs.left, H = o->vs.bottom - o->vs.top;
    if (r.left < 0) OffsetRect(&r, -r.left, 0);
    if (r.top < 0) OffsetRect(&r, 0, -r.top);
    if (r.right > W) OffsetRect(&r, W - r.right, 0);
    if (r.bottom > H) OffsetRect(&r, 0, H - r.bottom);
    return r;
}

void Finish(Overlay* o, bool ok) {
    if (o->finished) return;
    o->finished = true;
    Log(L"Selection: finished (%s)", ok ? L"ok" : L"canceled");
    SelectResult res;
    res.ok = ok;
    if (ok) {
        RECT r{};
        auto toScreen = [&](RECT c) { OffsetRect(&c, o->vs.left, o->vs.top); return c; };
        switch (o->mode) {
        case SelectMode::Fixed: r = FixedRect(o); break;
        case SelectMode::Freehand: {
            if (o->path.size() < 3) { res.ok = false; break; }
            RECT b{ LONG_MAX, LONG_MAX, LONG_MIN, LONG_MIN };
            for (auto p : o->path) {
                b.left = std::min(b.left, p.x); b.top = std::min(b.top, p.y);
                b.right = std::max(b.right, p.x + 1); b.bottom = std::max(b.bottom, p.y + 1);
                res.path.push_back({ p.x + o->vs.left, p.y + o->vs.top });
            }
            r = b;
            break;
        }
        case SelectMode::MultiArea: {
            if (o->rects.empty()) { res.ok = false; break; }
            RECT b = o->rects[0];
            for (auto& x : o->rects) { UnionRect(&b, &b, &x); res.rects.push_back(toScreen(x)); }
            r = b;
            break;
        }
        default:
            r = o->dragging ? Norm(o->dragStart, o->cur) : o->hover;
        }
        res.rect = toScreen(r);
        res.window = o->hoverWnd;
        res.child = o->hoverChild;
        if (res.rect.right - res.rect.left < 2 || res.rect.bottom - res.rect.top < 2) res.ok = false;
    }
    UnregisterHotKey(o->hwnd, 3);
    auto done = o->done;
    DestroyWindow(o->hwnd);
    if (done) done(res);
}

void Paint(Overlay* o, HDC hdc) {
    RECT cr; GetClientRect(o->hwnd, &cr);
    int W = cr.right, H = cr.bottom;
    HDC mem = CreateCompatibleDC(hdc);
    HBITMAP bb = CreateCompatibleBitmap(hdc, W, H);
    HGDIOBJ ob = SelectObject(mem, bb);
    BitBlt(mem, 0, 0, W, H, o->dimDC, 0, 0, SRCCOPY);
    int dpi = DpiForWindow(o->hwnd);
    COLORREF accent = o->video ? RGB(235, 60, 60) : RGB(30, 144, 255);
    HPEN pen = CreatePen(PS_SOLID, std::max(2, ui::Scale(2, dpi)), accent);
    HGDIOBJ op = SelectObject(mem, pen);
    HGDIOBJ obr = SelectObject(mem, GetStockObject(NULL_BRUSH));

    auto lit = [&](RECT r) {
        BitBlt(mem, r.left, r.top, r.right - r.left, r.bottom - r.top, o->brightDC, r.left, r.top, SRCCOPY);
        Rectangle(mem, r.left, r.top, r.right, r.bottom);
    };
    RECT sel{};
    bool haveSel = false;
    switch (o->mode) {
    case SelectMode::Fixed: sel = FixedRect(o); haveSel = true; lit(sel); break;
    case SelectMode::Freehand:
        if (o->path.size() > 1) {
            Polyline(mem, o->path.data(), (int)o->path.size());
        }
        break;
    case SelectMode::MultiArea:
        for (auto& r : o->rects) lit(r);
        if (o->dragging) { sel = Norm(o->dragStart, o->cur); haveSel = true; lit(sel); }
        break;
    default:
        if (o->dragging) { sel = Norm(o->dragStart, o->cur); haveSel = true; lit(sel); }
        else if (!IsRectEmpty(&o->hover)) { sel = o->hover; haveSel = true; lit(sel); }
    }

    // crosshair
    HPEN cross = CreatePen(PS_DOT, 1, RGB(255, 255, 255));
    SelectObject(mem, cross);
    SetBkMode(mem, OPAQUE); SetBkColor(mem, RGB(0, 0, 0));
    if (o->mode != SelectMode::Window && o->mode != SelectMode::PickWindow) {
        MoveToEx(mem, 0, o->cur.y, nullptr); LineTo(mem, W, o->cur.y);
        MoveToEx(mem, o->cur.x, 0, nullptr); LineTo(mem, o->cur.x, H);
    }
    SelectObject(mem, op); SelectObject(mem, obr);
    DeleteObject(pen); DeleteObject(cross);

    HFONT font = ui::UiFont(dpi, 9, true);
    // size label
    if (haveSel) {
        std::wstring t = Format(L" %d × %d ", sel.right - sel.left, sel.bottom - sel.top);
        RECT lr{ sel.left, sel.top - ui::Scale(26, dpi), sel.left + ui::Scale(120, dpi), sel.top - ui::Scale(4, dpi) };
        if (lr.top < 0) OffsetRect(&lr, 0, sel.top - lr.top + ui::Scale(6, dpi));
        ui::FillRoundRect(mem, lr, 8, accent, accent);
        ui::DrawTextCentered(mem, t, lr, RGB(255, 255, 255), font);
    }
    // magnifier
    {
        int Z = 8, N = 15, box = Z * N;
        int bx = o->cur.x + ui::Scale(24, dpi), by = o->cur.y + ui::Scale(24, dpi);
        int lh = ui::Scale(44, dpi);
        if (bx + box > W) bx = o->cur.x - ui::Scale(24, dpi) - box;
        if (by + box + lh > H) by = o->cur.y - ui::Scale(24, dpi) - box - lh;
        SetStretchBltMode(mem, COLORONCOLOR);
        StretchBlt(mem, bx, by, box, box, o->brightDC, o->cur.x - N / 2, o->cur.y - N / 2, N, N, SRCCOPY);
        HPEN bp = CreatePen(PS_SOLID, 1, RGB(255, 255, 255));
        HGDIOBJ p2 = SelectObject(mem, bp);
        SelectObject(mem, GetStockObject(NULL_BRUSH));
        Rectangle(mem, bx - 1, by - 1, bx + box + 1, by + box + 1);
        Rectangle(mem, bx + (N / 2) * Z, by + (N / 2) * Z, bx + (N / 2 + 1) * Z + 1, by + (N / 2 + 1) * Z + 1);
        SelectObject(mem, p2); DeleteObject(bp);
        int sx = std::clamp((int)o->cur.x, 0, o->frozen.w - 1), sy = std::clamp((int)o->cur.y, 0, o->frozen.h - 1);
        uint32_t px = o->frozen.at(sx, sy);
        RECT tr{ bx - 1, by + box + 1, bx + box + 1, by + box + lh };
        HBRUSH bg = CreateSolidBrush(RGB(32, 32, 32)); FillRect(mem, &tr, bg); DeleteObject(bg);
        RECT t1 = tr; t1.bottom = tr.top + lh / 2;
        RECT t2 = tr; t2.top = t1.bottom;
        ui::DrawTextCentered(mem, Format(L"%d, %d", o->cur.x + o->vs.left, o->cur.y + o->vs.top), t1, RGB(255, 255, 255), ui::UiFont(dpi, 8));
        RECT sw{ t2.left + ui::Scale(8, dpi), t2.top + ui::Scale(4, dpi), t2.left + ui::Scale(22, dpi), t2.bottom - ui::Scale(4, dpi) };
        ui::FillRoundRect(mem, sw, 4, ToColorref(px), RGB(255, 255, 255));
        ui::DrawTextCentered(mem, ColorToHex(ToColorref(px)), t2, RGB(255, 255, 255), ui::UiFont(dpi, 8, true));
    }
    // hint
    {
        std::wstring hint;
        switch (o->mode) {
        case SelectMode::Region: hint = Tr(L"Arraste para selecionar uma região • clique para capturar a janela/área destacada • Enter = monitor • Esc = cancelar"); break;
        case SelectMode::Window: hint = Tr(L"Clique na janela a capturar • Esc = cancelar"); break;
        case SelectMode::Object: hint = Tr(L"Clique no controle/objeto a capturar • Esc = cancelar"); break;
        case SelectMode::Fixed: hint = Format(Tr(L"Região fixa %d × %d — clique para capturar • Esc = cancelar"), o->fixed.cx, o->fixed.cy); break;
        case SelectMode::Freehand: hint = Tr(L"Desenhe o contorno com o botão pressionado • Esc = cancelar"); break;
        case SelectMode::MultiArea: hint = Tr(L"Arraste várias áreas • Backspace desfaz • Enter ou botão direito conclui • Esc = cancelar"); break;
        case SelectMode::PickWindow: hint = Tr(L"Clique na janela/área rolável • Esc = cancelar"); break;
        }
        if (o->video) hint = Tr(L"Gravação: ") + hint;
        POINT sp{ o->cur.x + o->vs.left, o->cur.y + o->vs.top };
        RECT mon = MonitorRectFromPoint(sp);
        OffsetRect(&mon, -o->vs.left, -o->vs.top);
        int hw = ui::Scale(760, dpi), hh = ui::Scale(32, dpi);
        RECT hr{ (mon.left + mon.right - hw) / 2, mon.top + ui::Scale(16, dpi), 0, 0 };
        hr.right = hr.left + hw; hr.bottom = hr.top + hh;
        ui::FillRoundRect(mem, hr, 14, RGB(28, 28, 28), RGB(80, 80, 80));
        ui::DrawTextCentered(mem, hint, hr, RGB(235, 235, 235), ui::UiFont(dpi, 9));
    }
    BitBlt(hdc, 0, 0, W, H, mem, 0, 0, SRCCOPY);
    SelectObject(mem, ob);
    DeleteObject(bb); DeleteDC(mem);
}

LRESULT CALLBACK OverlayProc(HWND h, UINT m, WPARAM w, LPARAM l) {
    auto* o = (Overlay*)GetWindowLongPtrW(h, GWLP_USERDATA);
    switch (m) {
    case WM_NCCREATE:
        SetWindowLongPtrW(h, GWLP_USERDATA, (LONG_PTR)((CREATESTRUCTW*)l)->lpCreateParams);
        return DefWindowProcW(h, m, w, l);
    case WM_ERASEBKGND: return 1;
    case WM_PAINT: { PAINTSTRUCT ps; HDC dc = BeginPaint(h, &ps); Paint(o, dc); EndPaint(h, &ps); return 0; }
    case WM_SETCURSOR: SetCursor(LoadCursor(nullptr, (o->mode == SelectMode::Window || o->mode == SelectMode::PickWindow) ? IDC_HAND : IDC_CROSS)); return TRUE;
    case WM_MOUSEMOVE:
        o->cur = { GET_X_LPARAM(l), GET_Y_LPARAM(l) };
        if (o->mode == SelectMode::Freehand && o->dragging) o->path.push_back(o->cur);
        if (!o->dragging) UpdateHover(o);
        InvalidateRect(h, nullptr, FALSE);
        return 0;
    case WM_LBUTTONDOWN:
        SetCapture(h);
        o->cur = { GET_X_LPARAM(l), GET_Y_LPARAM(l) };
        if (o->mode == SelectMode::Fixed || o->mode == SelectMode::Window || o->mode == SelectMode::Object ||
            o->mode == SelectMode::PickWindow) return 0;
        o->dragging = true; o->dragStart = o->cur;
        if (o->mode == SelectMode::Freehand) { o->path.clear(); o->path.push_back(o->cur); }
        return 0;
    case WM_LBUTTONUP: {
        ReleaseCapture();
        o->cur = { GET_X_LPARAM(l), GET_Y_LPARAM(l) };
        bool moved = o->dragging && (std::abs(o->cur.x - o->dragStart.x) > 3 || std::abs(o->cur.y - o->dragStart.y) > 3);
        if (o->mode == SelectMode::MultiArea) {
            if (moved) o->rects.push_back(Norm(o->dragStart, o->cur));
            o->dragging = false;
            InvalidateRect(h, nullptr, FALSE);
            return 0;
        }
        if (o->mode == SelectMode::Freehand) { Finish(o, true); return 0; }
        if (!moved) o->dragging = false;         // single click = highlighted area
        Finish(o, true);
        return 0;
    }
    case WM_RBUTTONUP:
        Finish(o, o->mode == SelectMode::MultiArea && !o->rects.empty());
        return 0;
    case WM_HOTKEY:
        if (w == 3) Finish(o, false);
        return 0;
    case WM_KEYDOWN:
        switch (w) {
        case VK_ESCAPE: Finish(o, false); return 0;
        case VK_RETURN:
            if (o->mode == SelectMode::MultiArea) { Finish(o, !o->rects.empty()); return 0; }
            if (o->mode == SelectMode::Region) {
                POINT sp{ o->cur.x + o->vs.left, o->cur.y + o->vs.top };
                RECT mr = MonitorRectFromPoint(sp);
                OffsetRect(&mr, -o->vs.left, -o->vs.top);
                o->dragging = false; o->hover = mr; o->hoverWnd = nullptr;
                Finish(o, true);
            }
            return 0;
        case VK_BACK: if (!o->rects.empty()) { o->rects.pop_back(); InvalidateRect(h, nullptr, FALSE); } return 0;
        case VK_LEFT: case VK_RIGHT: case VK_UP: case VK_DOWN: {
            POINT p; GetCursorPos(&p);
            int step = (GetKeyState(VK_SHIFT) < 0) ? 10 : 1;
            if (w == VK_LEFT) p.x -= step; if (w == VK_RIGHT) p.x += step;
            if (w == VK_UP) p.y -= step; if (w == VK_DOWN) p.y += step;
            SetCursorPos(p.x, p.y);
            return 0;
        }
        }
        return 0;
    case WM_CLOSE: Log(L"Selection: WM_CLOSE"); Finish(o, false); return 0;
    case WM_ACTIVATE: if (LOWORD(w) == WA_INACTIVE) Log(L"Selection: window deactivated"); break;
    case WM_DESTROY: {
        if (!o->finished) {
            // destroyed from outside (must not leave the engine stuck in InputSelecting)
            Log(L"Selection: destroyed before finishing");
            o->finished = true;
            auto done = o->done;
            SelectResult res;
            PostMessageW(nullptr, WM_NULL, 0, 0);
            if (done) done(res);
        }
        SelectObject(o->brightDC, GetStockObject(BLACK_PEN));
        DeleteDC(o->brightDC); DeleteDC(o->dimDC);
        DeleteObject(o->bright); DeleteObject(o->dim);
        delete o;
        SetWindowLongPtrW(h, GWLP_USERDATA, 0);
        return 0;
    }
    }
    return DefWindowProcW(h, m, w, l);
}

} // namespace

void RunSelectOverlay(const Image& frozen, RECT vs, SelectMode mode, SIZE fixedSize, bool video,
                      std::function<void(const SelectResult&)> done) {
    static bool reg = false;
    if (!reg) {
        WNDCLASSW wc{}; wc.lpfnWndProc = OverlayProc; wc.hInstance = GetModuleHandleW(nullptr);
        wc.hCursor = LoadCursor(nullptr, IDC_CROSS); wc.lpszClassName = L"SnapnabSelectWindowClass";
        RegisterClassW(&wc); reg = true;
    }
    auto* o = new Overlay();
    o->frozen = frozen; o->vs = vs; o->mode = mode; o->fixed = fixedSize; o->video = video; o->done = std::move(done);
    // light and darkened versions of the frozen screen
    o->bright = frozen.ToHBITMAP();
    Image d = frozen;
    for (auto& p : d.px) p = ARGB(255, CR(p) * 45 / 100, CG(p) * 45 / 100, CB(p) * 45 / 100);
    o->dim = d.ToHBITMAP();
    HDC sdc = GetDC(nullptr);
    o->brightDC = CreateCompatibleDC(sdc); SelectObject(o->brightDC, o->bright);
    o->dimDC = CreateCompatibleDC(sdc); SelectObject(o->dimDC, o->dim);
    ReleaseDC(nullptr, sdc);
    EnumWindows(EnumProc, (LPARAM)o);
    o->hwnd = CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW, L"SnapnabSelectWindowClass", Tr(L"Snapnab — seleção"),
                              WS_POPUP, vs.left, vs.top, vs.right - vs.left, vs.bottom - vs.top, nullptr, nullptr,
                              GetModuleHandleW(nullptr), o);
    POINT cp; GetCursorPos(&cp);
    o->cur = { cp.x - vs.left, cp.y - vs.top };
    UpdateHover(o);
    // Esc registered as hotkey id 3 while the crosshair is active (comp_7 §4.1)
    RegisterHotKey(o->hwnd, 3, MOD_NOREPEAT, VK_ESCAPE);
    ShowWindow(o->hwnd, SW_SHOW);
    SetForegroundWindow(o->hwnd);
    SetFocus(o->hwnd);
}

} // namespace snap
