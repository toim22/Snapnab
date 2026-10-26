#include "Ui.h"
#include <commdlg.h>

namespace snap::ui {

int Scale(int v, int dpi) { return MulDiv(v, dpi, 96); }

static bool FontExists(const wchar_t* face);

HFONT UiFont(int dpi, int pt, bool bold) {
    static std::map<int, HFONT> cache;
    int key = dpi * 1000 + pt * 2 + (bold ? 1 : 0);
    auto it = cache.find(key);
    if (it != cache.end()) return it->second;
    // Segoe UI Variable (Windows 11) has a lighter design and optical spacing; on Windows 10 Segoe UI is used
    // Chinese, Hindi and Bengali use the font with the language's characters (i18n::UiFontFace)
    static const wchar_t* face = i18n::UiFontFace() ? i18n::UiFontFace()
                               : FontExists(L"Segoe UI Variable Text") ? L"Segoe UI Variable Text" : L"Segoe UI";
    LOGFONTW lf{};
    lf.lfHeight = -MulDiv(pt, dpi, 72);
    lf.lfWeight = bold ? FW_SEMIBOLD : FW_NORMAL;
    lf.lfQuality = CLEARTYPE_NATURAL_QUALITY;
    wcscpy_s(lf.lfFaceName, face);
    HFONT f = CreateFontIndirectW(&lf);
    cache[key] = f;
    return f;
}

static bool FontExists(const wchar_t* face) {
    HDC dc = GetDC(nullptr);
    LOGFONTW lf{}; lf.lfCharSet = DEFAULT_CHARSET; wcscpy_s(lf.lfFaceName, face);
    bool found = false;
    EnumFontFamiliesExW(dc, &lf, [](const LOGFONTW*, const TEXTMETRICW*, DWORD, LPARAM p) -> int { *(bool*)p = true; return 0; },
                        (LPARAM)&found, 0);
    ReleaseDC(nullptr, dc);
    return found;
}

HFONT IconFont(int dpi, int px) {
    static std::map<int, HFONT> cache;
    int key = dpi * 1000 + px;
    auto it = cache.find(key);
    if (it != cache.end()) return it->second;
    static const wchar_t* face = FontExists(L"Segoe Fluent Icons") ? L"Segoe Fluent Icons" : L"Segoe MDL2 Assets";
    LOGFONTW lf{};
    lf.lfHeight = -Scale(px, dpi);
    lf.lfQuality = CLEARTYPE_QUALITY;
    wcscpy_s(lf.lfFaceName, face);
    HFONT f = CreateFontIndirectW(&lf);
    cache[key] = f;
    return f;
}

void FillRoundRect(HDC dc, RECT r, int radius, COLORREF fill, COLORREF border) {
    HBRUSH b = CreateSolidBrush(fill);
    HPEN p = CreatePen(PS_SOLID, 1, border);
    HGDIOBJ ob = SelectObject(dc, b), op = SelectObject(dc, p);
    RoundRect(dc, r.left, r.top, r.right, r.bottom, radius, radius);
    SelectObject(dc, ob); SelectObject(dc, op);
    DeleteObject(b); DeleteObject(p);
}

void DrawTextCentered(HDC dc, const std::wstring& t, RECT r, COLORREF color, HFONT font) {
    HGDIOBJ of = SelectObject(dc, font);
    SetBkMode(dc, TRANSPARENT);
    SetTextColor(dc, color);
    DrawTextW(dc, t.c_str(), -1, &r, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
    SelectObject(dc, of);
}

// ---------------------------------------------------------------- form
namespace {
struct FormState {
    std::vector<Field>* fields;
    std::function<std::wstring(std::vector<Field>&)> validate;
    bool done = false, ok = false;
    int dpi = 96;
};

constexpr int ID_OK = 1, ID_CANCEL = 2, ID_FIELD = 1000;

UINT HkfToMod(BYTE f) { return ((f & HOTKEYF_SHIFT) ? MOD_SHIFT : 0) | ((f & HOTKEYF_CONTROL) ? MOD_CONTROL : 0) | ((f & HOTKEYF_ALT) ? MOD_ALT : 0); }
BYTE ModToHkf(UINT m) { return (BYTE)(((m & MOD_SHIFT) ? HOTKEYF_SHIFT : 0) | ((m & MOD_CONTROL) ? HOTKEYF_CONTROL : 0) | ((m & MOD_ALT) ? HOTKEYF_ALT : 0)); }

void Collect(std::vector<Field>& fields) {
    for (auto& f : fields) {
        wchar_t buf[4096];
        switch (f.type) {
        case Field::Text: GetWindowTextW(f.hwnd, buf, 4096); f.text = buf; break;
        case Field::Int: GetWindowTextW(f.hwnd, buf, 64); f.value = std::clamp((int)wcstol(buf, nullptr, 10), f.minV, f.maxV); break;
        case Field::Check: f.value = Button_GetCheck(f.hwnd) == BST_CHECKED; break;
        case Field::Combo: f.value = ComboBox_GetCurSel(f.hwnd); break;
        case Field::Hotkey: {
            WORD v = (WORD)SendMessageW(f.hwnd, HKM_GETHOTKEY, 0, 0);
            f.hkVk = LOBYTE(v); f.hkMods = HkfToMod(HIBYTE(v));
            break;
        }
        default: break;
        }
    }
}

LRESULT CALLBACK FormProc(HWND h, UINT m, WPARAM w, LPARAM l) {
    auto* st = (FormState*)GetWindowLongPtrW(h, GWLP_USERDATA);
    switch (m) {
    case WM_COMMAND: {
        int id = LOWORD(w);
        if (id == ID_OK) {
            Collect(*st->fields);
            if (st->validate) {
                std::wstring err = st->validate(*st->fields);
                if (!err.empty()) { MessageBoxW(h, err.c_str(), L"Snapnab", MB_ICONWARNING); return 0; }
            }
            st->ok = true; st->done = true;
        } else if (id == ID_CANCEL) {
            st->done = true;
        } else if (id >= ID_FIELD && id < ID_FIELD + (int)st->fields->size()) {
            Field& f = (*st->fields)[id - ID_FIELD];
            if (f.type == Field::Color && HIWORD(w) == BN_CLICKED) {
                if (PickColor(h, f.color)) InvalidateRect(f.hwnd, nullptr, TRUE);
            }
        }
        return 0;
    }
    case WM_DRAWITEM: {
        auto* di = (DRAWITEMSTRUCT*)l;
        int idx = (int)di->CtlID - ID_FIELD;
        if (idx >= 0 && idx < (int)st->fields->size()) {
            Field& f = (*st->fields)[idx];
            FillRect(di->hDC, &di->rcItem, GetSysColorBrush(COLOR_BTNFACE));
            RECT r = di->rcItem; InflateRect(&r, -2, -2);
            FillRoundRect(di->hDC, r, 6, f.color, RGB(120, 120, 120));
            RECT t = r; t.left += r.bottom - r.top + 6;
            std::wstring hex = ColorToHex(f.color);
            int lum = (GetRValue(f.color) * 299 + GetGValue(f.color) * 587 + GetBValue(f.color) * 114) / 1000;
            DrawTextCentered(di->hDC, hex, r, lum > 128 ? RGB(0, 0, 0) : RGB(255, 255, 255), UiFont(st->dpi));
        }
        return TRUE;
    }
    case WM_CTLCOLORSTATIC: {
        SetBkMode((HDC)w, TRANSPARENT);
        return (LRESULT)GetSysColorBrush(COLOR_WINDOW);
    }
    case WM_CLOSE: st->done = true; return 0;
    }
    return DefWindowProcW(h, m, w, l);
}
}

bool RunForm(HWND owner, const std::wstring& title, std::vector<Field>& fields,
             std::function<std::wstring(std::vector<Field>&)> validate, int width) {
    static bool reg = false;
    if (!reg) {
        WNDCLASSW wc{}; wc.lpfnWndProc = FormProc; wc.hInstance = GetModuleHandleW(nullptr);
        wc.hCursor = LoadCursor(nullptr, IDC_ARROW); wc.hbrBackground = GetSysColorBrush(COLOR_WINDOW);
        wc.lpszClassName = L"SnapnabForm";
        RegisterClassW(&wc); reg = true;
    }
    FormState st{ &fields, validate };
    st.dpi = DpiForWindow(owner);
    int dpi = st.dpi;
    int W = Scale(width, dpi), pad = Scale(14, dpi), rowH = Scale(30, dpi), ctlH = Scale(24, dpi);
    int labelW = Scale(150, dpi);
    {   // the label column grows with the longest label (translations), and the window with it
        HDC dc = GetDC(nullptr);
        HGDIOBJ of = SelectObject(dc, UiFont(dpi));
        int widest = 0;
        for (auto& f : fields) {
            if (f.type == Field::Label || f.type == Field::Separator || f.type == Field::Check) continue;
            SIZE sz{}; GetTextExtentPoint32W(dc, f.label.c_str(), (int)f.label.size(), &sz);
            widest = std::max(widest, (int)sz.cx);
        }
        SelectObject(dc, of); ReleaseDC(nullptr, dc);
        int need = std::min(widest + Scale(14, dpi), Scale(320, dpi));
        if (need > labelW) { W += need - labelW; labelW = need; }
    }
    int rows = 0;
    for (auto& f : fields) rows += f.type == Field::Label ? 1 : 1;
    int clientH = pad * 2 + rows * rowH + Scale(46, dpi);
    RECT wr{ 0, 0, W, clientH };
    DWORD style = WS_POPUP | WS_CAPTION | WS_SYSMENU | WS_CLIPCHILDREN;
    AdjustWindowRectExForDpi(&wr, style, FALSE, WS_EX_DLGMODALFRAME, dpi);
    RECT orc; GetWindowRect(owner ? owner : GetDesktopWindow(), &orc);
    int x = (orc.left + orc.right - (wr.right - wr.left)) / 2, y = (orc.top + orc.bottom - (wr.bottom - wr.top)) / 2;
    HWND h = CreateWindowExW(WS_EX_DLGMODALFRAME | WS_EX_TOPMOST, L"SnapnabForm", title.c_str(), style, x, y,
                             wr.right - wr.left, wr.bottom - wr.top, owner, nullptr, GetModuleHandleW(nullptr), nullptr);
    SetWindowLongPtrW(h, GWLP_USERDATA, (LONG_PTR)&st);
    HFONT font = UiFont(dpi), bold = UiFont(dpi, 9, true);
    HINSTANCE hi = GetModuleHandleW(nullptr);
    int yy = pad;
    HWND first = nullptr;
    for (size_t i = 0; i < fields.size(); i++) {
        Field& f = fields[i];
        HMENU id = (HMENU)(INT_PTR)(ID_FIELD + i);
        int cx = pad + labelW, cw = W - pad - cx;
        auto label = [&]() {
            HWND s = CreateWindowExW(0, L"STATIC", f.label.c_str(), WS_CHILD | WS_VISIBLE | SS_LEFT | SS_CENTERIMAGE,
                                     pad, yy, labelW - Scale(8, dpi), ctlH, h, nullptr, hi, nullptr);
            SendMessageW(s, WM_SETFONT, (WPARAM)font, TRUE);
        };
        switch (f.type) {
        case Field::Label:
            f.hwnd = CreateWindowExW(0, L"STATIC", f.label.c_str(), WS_CHILD | WS_VISIBLE | SS_LEFT | SS_CENTERIMAGE,
                                     pad, yy, W - 2 * pad, ctlH, h, id, hi, nullptr);
            SendMessageW(f.hwnd, WM_SETFONT, (WPARAM)font, TRUE);
            break;
        case Field::Separator:
            f.hwnd = CreateWindowExW(0, L"STATIC", f.label.c_str(), WS_CHILD | WS_VISIBLE | SS_LEFT | SS_CENTERIMAGE,
                                     pad, yy, W - 2 * pad, ctlH, h, id, hi, nullptr);
            SendMessageW(f.hwnd, WM_SETFONT, (WPARAM)bold, TRUE);
            break;
        case Field::Text:
            label();
            f.hwnd = CreateWindowExW(WS_EX_CLIENTEDGE, L"EDIT", f.text.c_str(), WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL,
                                     cx, yy, cw, ctlH, h, id, hi, nullptr);
            break;
        case Field::Int:
            label();
            f.hwnd = CreateWindowExW(WS_EX_CLIENTEDGE, L"EDIT", std::to_wstring(f.value).c_str(),
                                     WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL | (f.minV >= 0 ? ES_NUMBER : 0),
                                     cx, yy, Scale(90, dpi), ctlH, h, id, hi, nullptr);
            {
                HWND ud = CreateWindowExW(0, UPDOWN_CLASSW, nullptr, WS_CHILD | WS_VISIBLE | UDS_ALIGNRIGHT | UDS_SETBUDDYINT |
                                          UDS_ARROWKEYS | UDS_NOTHOUSANDS, 0, 0, 0, 0, h, nullptr, hi, nullptr);
                SendMessageW(ud, UDM_SETBUDDY, (WPARAM)f.hwnd, 0);
                SendMessageW(ud, UDM_SETRANGE32, f.minV, f.maxV);
                SendMessageW(ud, UDM_SETPOS32, 0, f.value);
            }
            break;
        case Field::Check:
            f.hwnd = CreateWindowExW(0, L"BUTTON", f.label.c_str(), WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX,
                                     pad, yy, W - 2 * pad, ctlH, h, id, hi, nullptr);
            Button_SetCheck(f.hwnd, f.value ? BST_CHECKED : BST_UNCHECKED);
            break;
        case Field::Combo:
            label();
            f.hwnd = CreateWindowExW(0, L"COMBOBOX", nullptr, WS_CHILD | WS_VISIBLE | WS_TABSTOP | CBS_DROPDOWNLIST | WS_VSCROLL,
                                     cx, yy, cw, Scale(300, dpi), h, id, hi, nullptr);
            for (auto& o : f.options) ComboBox_AddString(f.hwnd, o.c_str());
            ComboBox_SetCurSel(f.hwnd, f.value);
            break;
        case Field::Color:
            label();
            f.hwnd = CreateWindowExW(0, L"BUTTON", nullptr, WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW,
                                     cx, yy, Scale(110, dpi), ctlH, h, id, hi, nullptr);
            break;
        case Field::Hotkey:
            label();
            f.hwnd = CreateWindowExW(0, HOTKEY_CLASSW, nullptr, WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_BORDER,
                                     cx, yy, cw, ctlH, h, id, hi, nullptr);
            SendMessageW(f.hwnd, HKM_SETHOTKEY, MAKEWORD(f.hkVk, ModToHkf(f.hkMods)), 0);
            break;
        }
        if (f.hwnd) SendMessageW(f.hwnd, WM_SETFONT, (WPARAM)(f.type == Field::Separator ? bold : font), TRUE);
        if (!first && f.hwnd && f.type != Field::Label && f.type != Field::Separator) first = f.hwnd;
        yy += rowH;
    }
    int bw = Scale(96, dpi), bh = Scale(28, dpi);
    yy += Scale(6, dpi);
    HWND ok = CreateWindowExW(0, L"BUTTON", Tr(L"OK"), WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_DEFPUSHBUTTON,
                              W - pad - bw * 2 - Scale(8, dpi), yy, bw, bh, h, (HMENU)(INT_PTR)ID_OK, hi, nullptr);
    HWND cancel = CreateWindowExW(0, L"BUTTON", Tr(L"Cancelar"), WS_CHILD | WS_VISIBLE | WS_TABSTOP,
                                  W - pad - bw, yy, bw, bh, h, (HMENU)(INT_PTR)ID_CANCEL, hi, nullptr);
    SendMessageW(ok, WM_SETFONT, (WPARAM)font, TRUE);
    SendMessageW(cancel, WM_SETFONT, (WPARAM)font, TRUE);

    if (owner) EnableWindow(owner, FALSE);
    ShowWindow(h, SW_SHOW);
    SetForegroundWindow(h);
    SetFocus(first ? first : ok);
    MSG msg;
    while (!st.done && GetMessageW(&msg, nullptr, 0, 0)) {
        if (msg.message == WM_KEYDOWN && msg.wParam == VK_ESCAPE && (msg.hwnd == h || IsChild(h, msg.hwnd))) { st.done = true; continue; }
        if (!IsDialogMessageW(h, &msg)) { TranslateMessage(&msg); DispatchMessageW(&msg); }
    }
    if (owner) { EnableWindow(owner, TRUE); SetForegroundWindow(owner); }
    DestroyWindow(h);
    return st.ok;
}

// ---------------------------------------------------------------- text window
void ShowTextWindow(HWND owner, const std::wstring& title, const std::wstring& text) {
    std::vector<Field> dummy;
    static bool reg = false;
    if (!reg) {
        WNDCLASSW wc{}; wc.hInstance = GetModuleHandleW(nullptr);
        wc.lpfnWndProc = [](HWND h, UINT m, WPARAM w, LPARAM l) -> LRESULT {
            switch (m) {
            case WM_SIZE: {
                HWND e = GetDlgItem(h, 10), b = GetDlgItem(h, 11);
                int dpi = DpiForWindow(h), bh = Scale(30, dpi), pad = Scale(8, dpi);
                MoveWindow(e, pad, pad, LOWORD(l) - 2 * pad, HIWORD(l) - bh - 3 * pad, TRUE);
                MoveWindow(b, LOWORD(l) - pad - Scale(180, dpi), HIWORD(l) - bh - pad, Scale(180, dpi), bh, TRUE);
                return 0;
            }
            case WM_COMMAND:
                if (LOWORD(w) == 11) {
                    HWND e = GetDlgItem(h, 10);
                    int n = GetWindowTextLengthW(e);
                    std::wstring t(n + 1, L'\0'); GetWindowTextW(e, t.data(), n + 1);
                    if (OpenClipboard(h)) {
                        EmptyClipboard();
                        HGLOBAL g = GlobalAlloc(GMEM_MOVEABLE, (n + 1) * 2);
                        memcpy(GlobalLock(g), t.c_str(), (n + 1) * 2); GlobalUnlock(g);
                        SetClipboardData(CF_UNICODETEXT, g);
                        CloseClipboard();
                    }
                }
                return 0;
            case WM_CLOSE: DestroyWindow(h); return 0;
            }
            return DefWindowProcW(h, m, w, l);
        };
        wc.hCursor = LoadCursor(nullptr, IDC_ARROW); wc.hbrBackground = GetSysColorBrush(COLOR_BTNFACE);
        wc.lpszClassName = L"SnapnabTextWnd";
        RegisterClassW(&wc); reg = true;
    }
    int dpi = DpiForWindow(owner);
    HWND h = CreateWindowExW(WS_EX_TOOLWINDOW, L"SnapnabTextWnd", title.c_str(), WS_OVERLAPPEDWINDOW,
                             CW_USEDEFAULT, CW_USEDEFAULT, Scale(620, dpi), Scale(460, dpi), owner, nullptr, GetModuleHandleW(nullptr), nullptr);
    std::wstring t;
    for (wchar_t c : text) { if (c == L'\n') t += L"\r\n"; else if (c != L'\r') t += c; }
    HWND e = CreateWindowExW(WS_EX_CLIENTEDGE, L"EDIT", t.c_str(), WS_CHILD | WS_VISIBLE | WS_VSCROLL | ES_MULTILINE |
                             ES_AUTOVSCROLL | ES_WANTRETURN, 0, 0, 10, 10, h, (HMENU)10, GetModuleHandleW(nullptr), nullptr);
    HWND b = CreateWindowExW(0, L"BUTTON", Tr(L"Copiar tudo"), WS_CHILD | WS_VISIBLE, 0, 0, 10, 10, h, (HMENU)11,
                             GetModuleHandleW(nullptr), nullptr);
    SendMessageW(e, WM_SETFONT, (WPARAM)UiFont(dpi, 10), TRUE);
    SendMessageW(b, WM_SETFONT, (WPARAM)UiFont(dpi), TRUE);
    RECT rc; GetClientRect(h, &rc);
    SendMessageW(h, WM_SIZE, 0, MAKELPARAM(rc.right, rc.bottom));
    ShowWindow(h, SW_SHOW);
}

bool OpenFileDialog(HWND owner, const std::wstring& filter, std::wstring& path, const std::wstring& title) {
    wchar_t buf[MAX_PATH * 4] = L"";
    OPENFILENAMEW of{ sizeof(of) };
    of.hwndOwner = owner; of.lpstrFilter = filter.c_str(); of.lpstrFile = buf; of.nMaxFile = MAX_PATH * 4;
    of.lpstrTitle = title.empty() ? nullptr : title.c_str();
    of.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_EXPLORER;
    if (!GetOpenFileNameW(&of)) return false;
    path = buf;
    return true;
}

bool SaveFileDialog(HWND owner, const std::wstring& filter, std::wstring& path, int& filterIndex,
                    const std::wstring& defExt, const std::wstring& title) {
    wchar_t buf[MAX_PATH * 4];
    wcscpy_s(buf, path.c_str());
    OPENFILENAMEW of{ sizeof(of) };
    of.hwndOwner = owner; of.lpstrFilter = filter.c_str(); of.lpstrFile = buf; of.nMaxFile = MAX_PATH * 4;
    of.nFilterIndex = filterIndex; of.lpstrDefExt = defExt.c_str();
    of.lpstrTitle = title.empty() ? nullptr : title.c_str();
    of.Flags = OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST | OFN_EXPLORER;
    if (!GetSaveFileNameW(&of)) return false;
    path = buf;
    filterIndex = of.nFilterIndex;
    return true;
}

} // namespace snap::ui
