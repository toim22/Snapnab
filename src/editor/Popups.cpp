#include "Popups.h"
#include "Panels.h"
#include "Document.h"
#include "Ui.h"
#include <dwmapi.h>

namespace snap::ed {

namespace {
struct Cell { RECT r; int id; };
struct PopupState {
    std::vector<Cell> cells;
    std::function<void(Gdiplus::Graphics&, HDC, const Cell&, bool hot)> drawCell;
    std::function<void(Gdiplus::Graphics&, HDC, RECT client)> drawBack;
    int hot = -1, result = -1;
    bool done = false;
};
PopupState* g_pop = nullptr;

int HitCell(const PopupState& s, POINT p) {
    for (size_t i = 0; i < s.cells.size(); i++) if (PtInRect(&s.cells[i].r, p)) return (int)i;
    return -1;
}

LRESULT CALLBACK PopupProc(HWND h, UINT m, WPARAM w, LPARAM l) {
    PopupState* s = g_pop;
    if (!s) return DefWindowProcW(h, m, w, l);
    switch (m) {
    case WM_ERASEBKGND: return 1;
    case WM_PAINT: {
        PAINTSTRUCT ps; HDC dc = BeginPaint(h, &ps);
        RECT cr; GetClientRect(h, &cr);
        HDC mem = CreateCompatibleDC(dc);
        HBITMAP bb = CreateCompatibleBitmap(dc, cr.right, cr.bottom);
        HGDIOBJ ob = SelectObject(mem, bb);
        HBRUSH bg = CreateSolidBrush(RGB(255, 255, 255)); FillRect(mem, &cr, bg); DeleteObject(bg);
        {
            Gdiplus::Graphics g(mem);
            g.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
            g.SetPixelOffsetMode(Gdiplus::PixelOffsetModeHalf);
            if (s->drawBack) s->drawBack(g, mem, cr);
            for (size_t i = 0; i < s->cells.size(); i++) s->drawCell(g, mem, s->cells[i], (int)i == s->hot);
        }
        if (!GetWindowLongPtrW(h, GWLP_USERDATA)) { HBRUSH fr = CreateSolidBrush(RGB(200, 202, 208)); FrameRect(mem, &cr, fr); DeleteObject(fr); }
        BitBlt(dc, 0, 0, cr.right, cr.bottom, mem, 0, 0, SRCCOPY);
        SelectObject(mem, ob); DeleteObject(bb); DeleteDC(mem);
        EndPaint(h, &ps);
        return 0;
    }
    case WM_MOUSEMOVE: {
        int hv = HitCell(*s, { GET_X_LPARAM(l), GET_Y_LPARAM(l) });
        if (hv != s->hot) { s->hot = hv; InvalidateRect(h, nullptr, FALSE); }
        return 0;
    }
    case WM_LBUTTONDOWN: case WM_RBUTTONDOWN: {
        POINT p{ GET_X_LPARAM(l), GET_Y_LPARAM(l) };
        RECT cr; GetClientRect(h, &cr);
        if (!PtInRect(&cr, p)) { s->done = true; return 0; }       // a click outside closes it
        return 0;
    }
    case WM_LBUTTONUP: {
        POINT p{ GET_X_LPARAM(l), GET_Y_LPARAM(l) };
        int i = HitCell(*s, p);
        if (i >= 0) { s->result = s->cells[i].id; s->done = true; }
        return 0;
    }
    case WM_CAPTURECHANGED: if ((HWND)l != h) s->done = true; return 0;
    }
    return DefWindowProcW(h, m, w, l);
}

int RunPopup(HWND owner, POINT anchor, SIZE size, PopupState& st, DWORD corner = 3 /*DWMWCP_ROUNDSMALL*/, bool frame = true) {
    static bool reg = false;
    HINSTANCE hi = GetModuleHandleW(nullptr);
    if (!reg) {
        WNDCLASSW wc{}; wc.lpfnWndProc = PopupProc; wc.hInstance = hi; wc.hCursor = LoadCursor(nullptr, IDC_ARROW);
        wc.lpszClassName = L"SnapnabPopupGrid"; wc.style = CS_DROPSHADOW;
        RegisterClassW(&wc); reg = true;
    }
    HMONITOR mon = MonitorFromPoint(anchor, MONITOR_DEFAULTTONEAREST);
    MONITORINFO mi{ sizeof(mi) }; GetMonitorInfoW(mon, &mi);
    int x = std::clamp((int)anchor.x, (int)mi.rcWork.left, (int)mi.rcWork.right - (int)size.cx);
    int y = anchor.y;
    if (y + size.cy > mi.rcWork.bottom) y = std::max((int)mi.rcWork.top, (int)mi.rcWork.bottom - (int)size.cy);
    g_pop = &st;
    HWND root = GetAncestor(owner, GA_ROOT);
    HWND h = CreateWindowExW(WS_EX_TOOLWINDOW, L"SnapnabPopupGrid", nullptr, WS_POPUP, x, y, size.cx, size.cy, root, nullptr, hi, nullptr);
    DwmSetWindowAttribute(h, 33, &corner, sizeof corner);
    if (!frame) SetWindowLongPtrW(h, GWLP_USERDATA, 1);      // without the border line (DWM draws the rounded border)
    ShowWindow(h, SW_SHOWNOACTIVATE);
    SetCapture(h);
    MSG msg;
    while (!st.done && GetMessageW(&msg, nullptr, 0, 0)) {
        if (msg.message == WM_KEYDOWN && msg.wParam == VK_ESCAPE) { st.done = true; break; }
        if (msg.message == WM_KEYDOWN || msg.message == WM_CHAR) continue;
        TranslateMessage(&msg); DispatchMessageW(&msg);
    }
    if (GetCapture() == h) ReleaseCapture();
    DestroyWindow(h);
    g_pop = nullptr;
    return st.result;
}

void Swatch(Gdiplus::Graphics& g, RECT r, uint32_t c, bool hot, bool sel) {
    Gdiplus::RectF rf((float)r.left + 2, (float)r.top + 2, (float)(r.right - r.left) - 4, (float)(r.bottom - r.top) - 4);
    Gdiplus::SolidBrush b(Gdiplus::Color(255, (BYTE)((c >> 16) & 255), (BYTE)((c >> 8) & 255), (BYTE)(c & 255)));
    g.FillRectangle(&b, rf);
    Gdiplus::Pen p(Gdiplus::Color(60, 0, 0, 0), 1);
    g.DrawRectangle(&p, rf);
    if (hot || sel) {
        Gdiplus::Pen hp(sel ? Gdiplus::Color(255, 0, 100, 200) : Gdiplus::Color(255, 40, 40, 44), 2);
        g.DrawRectangle(&hp, Gdiplus::RectF(rf.X - 1, rf.Y - 1, rf.Width + 2, rf.Height + 2));
    }
}
} // namespace

int GridPopup(HWND owner, POINT anchor, int cols, int count, SIZE cell, int selected, GridDraw draw) {
    PopupState st;
    int pad = ui::Scale(6, DpiForWindow(owner));
    int rows = (count + cols - 1) / cols;
    for (int i = 0; i < count; i++)
        st.cells.push_back({ { pad + (i % cols) * cell.cx, pad + (i / cols) * cell.cy, pad + (i % cols + 1) * cell.cx, pad + (i / cols + 1) * cell.cy }, i });
    st.drawCell = [&](Gdiplus::Graphics& g, HDC dc, const Cell& c, bool hot) {
        bool sel = c.id == selected;
        if (hot || sel) {
            Gdiplus::SolidBrush hb(sel ? Gdiplus::Color(255, 220, 234, 250) : Gdiplus::Color(255, 238, 240, 244));
            g.FillRectangle(&hb, Gdiplus::Rect(c.r.left + 1, c.r.top + 1, c.r.right - c.r.left - 2, c.r.bottom - c.r.top - 2));
        }
        draw(g, dc, c.id, c.r, hot, sel);
    };
    return RunPopup(owner, anchor, { cols * cell.cx + 2 * pad, rows * cell.cy + 2 * pad }, st);
}

int ListPopup(HWND owner, POINT anchor, const std::vector<PopupItem>& items, int minWidth) {
    int dpi = DpiForWindow(owner);
    auto S = [dpi](int v) { return ui::Scale(v, dpi); };
    const auto& th = EdTheme();
    int pad = S(6), rowIcon = S(40), rowText = S(34), sepH = S(11), iconSz = S(22);
    // width from the longest name + shortcut
    HDC dc = GetDC(nullptr);
    HGDIOBJ of = SelectObject(dc, ui::UiFont(dpi, 10));
    int textW = 0, keyW = 0;
    for (auto& it : items) {
        if (it.separator) continue;
        RECT r{}; DrawTextW(dc, it.label.c_str(), -1, &r, DT_CALCRECT | DT_SINGLELINE); textW = std::max(textW, (int)r.right);
        if (!it.shortcut.empty()) { RECT k{}; DrawTextW(dc, it.shortcut.c_str(), -1, &k, DT_CALCRECT | DT_SINGLELINE); keyW = std::max(keyW, (int)k.right); }
    }
    SelectObject(dc, of);
    ReleaseDC(nullptr, dc);
    int W = std::max(minWidth, pad * 2 + S(14) + iconSz + S(14) + textW + (keyW ? S(28) + keyW : S(14)) + S(10));
    PopupState st;
    std::vector<std::pair<RECT, const PopupItem*>> rows;
    int y = pad;
    for (auto& it : items) {
        if (it.separator) { rows.push_back({ { 0, y, W, y + sepH }, &it }); y += sepH; continue; }
        int h = it.hasIcon ? rowIcon : rowText;
        RECT r{ pad, y, W - pad, y + h };
        rows.push_back({ r, &it });
        if (it.enabled) st.cells.push_back({ r, it.id });
        y += h;
    }
    y += pad;
    auto drawItem = [&](Gdiplus::Graphics& g, HDC mdc, const PopupItem& it, RECT r, bool hot) {
        COLORREF fg = !it.enabled ? RGB(168, 170, 176) : it.checked ? th.accentText : RGB(32, 34, 40);
        if ((hot && it.enabled) || it.checked) {
            Gdiplus::GraphicsPath p;
            float rad = (float)S(6), d = rad * 2;
            Gdiplus::RectF rf((float)r.left, (float)r.top + 1, (float)(r.right - r.left), (float)(r.bottom - r.top) - 2);
            p.AddArc(rf.X, rf.Y, d, d, 180, 90); p.AddArc(rf.GetRight() - d, rf.Y, d, d, 270, 90);
            p.AddArc(rf.GetRight() - d, rf.GetBottom() - d, d, d, 0, 90); p.AddArc(rf.X, rf.GetBottom() - d, d, d, 90, 90);
            p.CloseFigure();
            COLORREF bg = it.checked ? ui::Mix(RGB(255, 255, 255), th.control, hot ? 0.22 : 0.15) : ui::Mix(RGB(255, 255, 255), th.control, 0.08);
            Gdiplus::SolidBrush b(Gdiplus::Color(255, GetRValue(bg), GetGValue(bg), GetBValue(bg)));
            g.FillPath(&b, &p);
            if (it.checked) {                    // small highlight bar on the left (current item)
                Gdiplus::SolidBrush ab(Gdiplus::Color(255, GetRValue(th.control), GetGValue(th.control), GetBValue(th.control)));
                g.FillRectangle(&ab, (float)r.left, (float)(r.top + r.bottom) / 2 - S(8), (float)S(3), (float)S(16));
            }
        }
        int x = r.left + S(14);
        if (it.hasIcon) {
            int iy = (r.top + r.bottom - iconSz) / 2;
            icons::Draw(g, it.icon, Gdiplus::RectF((float)x, (float)iy, (float)iconSz, (float)iconSz),
                        Gdiplus::Color(255, GetRValue(fg), GetGValue(fg), GetBValue(fg)));
            x += iconSz + S(14);
        }
        HGDIOBJ f = SelectObject(mdc, ui::UiFont(dpi, 10, it.checked));
        SetBkMode(mdc, TRANSPARENT);
        SetTextColor(mdc, fg);
        RECT tr{ x, r.top, r.right - S(10), r.bottom };
        DrawTextW(mdc, it.label.c_str(), -1, &tr, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
        if (!it.shortcut.empty()) {
            SelectObject(mdc, ui::UiFont(dpi, 9));
            SetTextColor(mdc, it.enabled ? RGB(128, 130, 138) : RGB(180, 182, 188));
            DrawTextW(mdc, it.shortcut.c_str(), -1, &tr, DT_RIGHT | DT_VCENTER | DT_SINGLELINE);
        }
        SelectObject(mdc, f);
    };
    st.drawBack = [&](Gdiplus::Graphics& g, HDC mdc, RECT) {
        for (auto& [r, it] : rows) {
            if (it->separator) {
                Gdiplus::Pen p(Gdiplus::Color(255, 226, 228, 233), 1.0f);
                float yy = (r.top + r.bottom) / 2.0f;
                g.DrawLine(&p, (float)S(12), yy, (float)(W - S(12)), yy);
            } else if (!it->enabled || it->checked) drawItem(g, mdc, *it, r, false);
        }
    };
    st.drawCell = [&](Gdiplus::Graphics& g, HDC mdc, const Cell& c, bool hot) {
        for (auto& [r, it] : rows) if (!it->separator && it->enabled && it->id == c.id) drawItem(g, mdc, *it, r, hot);
    };
    return RunPopup(owner, anchor, { W, y }, st, 2 /*DWMWCP_ROUND*/, false);
}

// gradient presets built from the theme colors
static std::vector<Gradient> GradientPresets(const std::array<uint32_t, 4>& t) {
    auto lin = [](float a, std::vector<GradStop> st) { Gradient g; g.type = 1; g.angle = a; g.stops = std::move(st); return g; };
    auto rad = [](std::vector<GradStop> st) { Gradient g; g.type = 2; g.stops = std::move(st); return g; };
    return { lin(90, { { 0, 0xFFFFFFFF }, { 1, t[0] } }), lin(0, { { 0, t[1] }, { 1, t[2] } }), lin(45, { { 0, t[2] }, { 1, t[3] } }),
             lin(0, { { 0, t[0] }, { 0.5f, t[1] }, { 1, t[3] } }), rad({ { 0, 0xFFFFFFFF }, { 1, t[0] } }), lin(90, { { 0, 0xFF505050 }, { 1, 0xFF000000 } }),
             lin(90, { { 0, 0xFFF4F5F7 }, { 1, 0xFFB0B4BC } }), rad({ { 0, t[3] }, { 1, 0xFF101010 } }) };
}

bool EditGradient(HWND owner, Gradient& gr, uint32_t base) {
    Gradient g = gr;
    if (!g.Active()) { g.type = 1; g.angle = 90; g.stops = { { 0, 0xFFFFFFFF }, { 1, base | 0xFF000000 } }; }
    GradStop mid1{ 0.33f, 0 }, mid2{ 0.66f, 0 };
    bool has1 = g.stops.size() >= 3, has2 = g.stops.size() >= 4;
    if (has1) mid1 = g.stops[1];
    if (has2) mid2 = g.stops[2];
    std::vector<ui::Field> f = {
        ui::Field::MakeCombo(Tr(L"Tipo"), { Tr(L"Linear"), Tr(L"Radial (do centro)") }, g.type == 2 ? 1 : 0),
        ui::Field::MakeInt(Tr(L"Ângulo (graus)"), (int)g.angle, 0, 359),
        ui::Field::MakeColor(Tr(L"Cor inicial"), ToColorref(g.stops.front().color)),
        ui::Field::MakeColor(Tr(L"Cor final"), ToColorref(g.stops.back().color)),
        ui::Field::MakeCheck(Tr(L"Cor intermediária"), has1),
        ui::Field::MakeColor(Tr(L"Cor"), ToColorref(has1 ? mid1.color : 0xFFFFFFFF)),
        ui::Field::MakeInt(Tr(L"Posição (%)"), (int)std::lround(mid1.pos * 100), 1, 99),
        ui::Field::MakeCheck(Tr(L"Segunda cor intermediária"), has2),
        ui::Field::MakeColor(Tr(L"Cor"), ToColorref(has2 ? mid2.color : 0xFF808080)),
        ui::Field::MakeInt(Tr(L"Posição (%)"), (int)std::lround(mid2.pos * 100), 1, 99),
        ui::Field::MakeLabel(Tr(L"O ângulo vale para o gradiente linear: 0° da esquerda para a direita, 90° de cima para baixo.")) };
    if (!ui::RunForm(GetAncestor(owner, GA_ROOT), Tr(L"Gradiente"), f)) return false;
    g.type = f[0].value == 1 ? 2 : 1;
    g.angle = (float)f[1].value;
    g.stops = { { 0, FromColorref(f[2].color) }, { 1, FromColorref(f[3].color) } };
    if (f[4].value) g.stops.push_back({ f[6].value / 100.0f, FromColorref(f[5].color) });
    if (f[7].value) g.stops.push_back({ f[9].value / 100.0f, FromColorref(f[8].color) });
    std::sort(g.stops.begin(), g.stops.end(), [](const GradStop& a, const GradStop& b) { return a.pos < b.pos; });
    gr = g;
    return true;
}

bool ColorPopup(HWND owner, POINT anchor, uint32_t& color, bool allowNone, const std::array<uint32_t, 4>& theme, Gradient* grad) {
    static std::vector<uint32_t> recent;
    int dpi = DpiForWindow(owner);
    int cs = ui::Scale(22, dpi), pad = ui::Scale(10, dpi), head = ui::Scale(20, dpi);
    static const uint32_t standard[] = {
        0xFFFFFFFF, 0xFFE0E0E0, 0xFFBDBDBD, 0xFF9E9E9E, 0xFF757575, 0xFF424242, 0xFF212121, 0xFF000000,
        0xFFFFCDD2, 0xFFFFE0B2, 0xFFFFF9C4, 0xFFC8E6C9, 0xFFB3E5FC, 0xFFC5CAE9, 0xFFE1BEE7, 0xFFF8BBD0,
        0xFFEF5350, 0xFFFFA726, 0xFFFFEE58, 0xFF66BB6A, 0xFF29B6F6, 0xFF5C6BC0, 0xFFAB47BC, 0xFFEC407A,
        0xFFE53935, 0xFFFB8C00, 0xFFFDD835, 0xFF43A047, 0xFF039BE5, 0xFF3949AB, 0xFF8E24AA, 0xFFD81B60,
        0xFFB71C1C, 0xFFE65100, 0xFFF57F17, 0xFF1B5E20, 0xFF01579B, 0xFF1A237E, 0xFF4A148C, 0xFF880E4F,
    };
    PopupState st;
    std::vector<uint32_t> colors;           // id → color (ids 0..), -2 no color, -3 more colors
    int y = pad + head;
    auto rowOf = [&](const std::vector<uint32_t>& list) {
        for (size_t i = 0; i < list.size(); i++) {
            st.cells.push_back({ { pad + (int)i * cs, y, pad + ((int)i + 1) * cs, y + cs }, (int)colors.size() });
            colors.push_back(list[i]);
        }
        y += cs;
    };
    rowOf({ theme[0], theme[1], theme[2], theme[3], 0xFF000000, 0xFF505050, 0xFFC8C8C8, 0xFFFFFFFF });
    int yStd = y + ui::Scale(8, dpi);
    y = yStd + head;
    for (int r = 0; r < 5; r++) rowOf(std::vector<uint32_t>(standard + r * 8, standard + r * 8 + 8));
    int yRecent = -1;
    if (!recent.empty()) {
        yRecent = y + ui::Scale(8, dpi);
        y = yRecent + head;
        rowOf(recent);
    }
    int yGrad = -1;
    std::vector<Gradient> presets;
    if (grad) {                                   // gradient previews (ids -10…)
        presets = GradientPresets(theme);
        yGrad = y + ui::Scale(8, dpi);
        y = yGrad + head;
        for (size_t i = 0; i < presets.size(); i++) st.cells.push_back({ { pad + (int)i * cs, y, pad + ((int)i + 1) * cs, y + cs }, -10 - (int)i });
        y += cs;
    }
    y += ui::Scale(10, dpi);
    int bw = (8 * cs - ui::Scale(8, dpi)) / 2, bh = ui::Scale(28, dpi);
    if (allowNone) st.cells.push_back({ { pad, y, pad + bw, y + bh }, -2 });
    st.cells.push_back({ { pad + 8 * cs - bw, y, pad + 8 * cs, y + bh }, -3 });
    y += bh;
    if (grad) {
        y += ui::Scale(6, dpi);
        st.cells.push_back({ { pad, y, pad + 8 * cs, y + bh }, -4 });
        y += bh;
    }
    y += pad;
    st.drawBack = [&](Gdiplus::Graphics&, HDC dc, RECT cr) {
        HGDIOBJ of = SelectObject(dc, ui::UiFont(dpi, 8, true));
        SetBkMode(dc, TRANSPARENT); SetTextColor(dc, RGB(90, 90, 98));
        RECT t1{ pad, pad, cr.right - pad, pad + head - ui::Scale(4, dpi) };
        DrawTextW(dc, Tr(L"Cores do tema"), -1, &t1, DT_LEFT | DT_BOTTOM | DT_SINGLELINE);
        RECT t2{ pad, yStd, cr.right - pad, yStd + head - ui::Scale(4, dpi) };
        DrawTextW(dc, Tr(L"Cores padrão"), -1, &t2, DT_LEFT | DT_BOTTOM | DT_SINGLELINE);
        if (yRecent >= 0) { RECT t3{ pad, yRecent, cr.right - pad, yRecent + head - ui::Scale(4, dpi) }; DrawTextW(dc, Tr(L"Recentes"), -1, &t3, DT_LEFT | DT_BOTTOM | DT_SINGLELINE); }
        if (yGrad >= 0) { RECT t4{ pad, yGrad, cr.right - pad, yGrad + head - ui::Scale(4, dpi) }; DrawTextW(dc, Tr(L"Gradientes"), -1, &t4, DT_LEFT | DT_BOTTOM | DT_SINGLELINE); }
        SelectObject(dc, of);
    };
    st.drawCell = [&](Gdiplus::Graphics& g, HDC dc, const Cell& c, bool hot) {
        if (c.id >= 0) { uint32_t col = colors[c.id]; Swatch(g, c.r, col, hot, (col & 0xFFFFFF) == (color & 0xFFFFFF) && (color >> 24) && !(grad && grad->Active())); return; }
        if (c.id <= -10) {                        // gradient preview
            const Gradient& gp = presets[-10 - c.id];
            Gdiplus::RectF rf((float)c.r.left + 2, (float)c.r.top + 2, (float)(c.r.right - c.r.left) - 4, (float)(c.r.bottom - c.r.top) - 4);
            auto br = FillBrush(0xFF000000, gp, rf);
            g.FillRectangle(br.get(), rf);
            Gdiplus::Pen p(Gdiplus::Color(60, 0, 0, 0), 1);
            g.DrawRectangle(&p, rf);
            bool sel = grad && *grad == gp;
            if (hot || sel) {
                Gdiplus::Pen hp(sel ? Gdiplus::Color(255, 0, 100, 200) : Gdiplus::Color(255, 40, 40, 44), 2);
                g.DrawRectangle(&hp, Gdiplus::RectF(rf.X - 1, rf.Y - 1, rf.Width + 2, rf.Height + 2));
            }
            return;
        }
        Gdiplus::SolidBrush b(hot ? Gdiplus::Color(255, 232, 234, 238) : Gdiplus::Color(255, 244, 245, 247));
        g.FillRectangle(&b, Gdiplus::Rect(c.r.left, c.r.top, c.r.right - c.r.left, c.r.bottom - c.r.top));
        Gdiplus::Pen p(Gdiplus::Color(255, 210, 212, 218), 1);
        g.DrawRectangle(&p, Gdiplus::Rect(c.r.left, c.r.top, c.r.right - c.r.left - 1, c.r.bottom - c.r.top - 1));
        ui::DrawTextCentered(dc, c.id == -2 ? Tr(L"Sem cor") : c.id == -4 ? Tr(L"Editar gradiente…") : Tr(L"Mais cores…"), c.r, RGB(40, 40, 44), ui::UiFont(dpi, 8));
    };
    int id = RunPopup(owner, anchor, { 8 * cs + 2 * pad, y }, st);
    if (id == -1) return false;
    if (grad && id <= -10) {                      // gradient preset: the solid color becomes its first stop
        *grad = presets[-10 - id];
        color = grad->stops.front().color;
        return true;
    }
    if (grad && id == -4) {
        if (!EditGradient(owner, *grad, color)) return false;
        color = grad->stops.front().color;
        return true;
    }
    if (grad) grad->type = 0;                     // a solid color (or none) turns the gradient off
    uint32_t chosen;
    if (id == -2) chosen = color & 0x00FFFFFF;
    else if (id == -3) {
        COLORREF c = ToColorref(color);
        if (!PickColor(GetAncestor(owner, GA_ROOT), c)) return false;
        chosen = FromColorref(c);
    } else chosen = colors[id];
    if (chosen >> 24) {
        recent.erase(std::remove(recent.begin(), recent.end(), chosen), recent.end());
        recent.insert(recent.begin(), chosen);
        if (recent.size() > 8) recent.resize(8);
    }
    color = chosen;
    return true;
}

} // namespace snap::ed
