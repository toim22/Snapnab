#include "Panels.h"
#include "Ui.h"
#include "Video.h"
#include <cmath>

namespace snap::ed {

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

int Toolbar::Height() const { return ui::Scale(66, DpiForWindow(hwnd)); }

void Toolbar::Layout() {
    RECT cr; GetClientRect(hwnd, &cr);
    int dpi = DpiForWindow(hwnd);
    int full = ui::Scale(50, dpi), narrow = ui::Scale(40, dpi), pad = ui::Scale(6, dpi), sep = ui::Scale(10, dpi);
    int need = 0;
    for (auto& it : items) need += full + (it.tool ? 0 : sep / 2);
    compact_ = need + 2 * pad > cr.right;
    int bw = compact_ ? narrow : full;
    rects_.assign(items.size(), RECT{});
    int x = pad;
    for (size_t i = 0; i < items.size(); i++) {
        if (items[i].right) continue;
        if (i > 0 && items[i].tool != items[i - 1].tool) x += sep;
        rects_[i] = { x, pad, x + bw, cr.bottom - pad };
        x += bw + 2;
    }
    int xr = cr.right - pad;
    for (int i = (int)items.size() - 1; i >= 0; i--) {
        if (!items[i].right) continue;
        rects_[i] = { xr - bw, pad, xr, cr.bottom - pad };
        xr -= bw + 2;
    }
}

int Toolbar::Hit(POINT p) const {
    for (size_t i = 0; i < rects_.size(); i++) if (PtInRect(&rects_[i], p)) return (int)i;
    return -1;
}

void Toolbar::Paint(HDC hdc) {
    RECT cr; GetClientRect(hwnd, &cr);
    HDC mem = CreateCompatibleDC(hdc);
    HBITMAP bb = CreateCompatibleBitmap(hdc, std::max(1L, cr.right), std::max(1L, cr.bottom));
    HGDIOBJ ob = SelectObject(mem, bb);
    HBRUSH bg = CreateSolidBrush(RGB(248, 248, 250)); FillRect(mem, &cr, bg); DeleteObject(bg);
    RECT line{ 0, cr.bottom - 1, cr.right, cr.bottom };
    HBRUSH lb = CreateSolidBrush(RGB(220, 222, 226)); FillRect(mem, &line, lb); DeleteObject(lb);
    int dpi = DpiForWindow(hwnd);
    Gdiplus::Graphics g(mem);
    g.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
    for (size_t i = 0; i < items.size(); i++) {
        const Item& it = items[i];
        RECT r = rects_[i];
        bool act = it.tool && it.t == active;
        bool capture = it.id == 1;
        COLORREF fill = capture ? RGB(30, 120, 230) : act ? RGB(214, 232, 255) : (int)i == hover_ ? RGB(232, 234, 238) : RGB(248, 248, 250);
        if ((int)i == pressed_) fill = capture ? RGB(20, 95, 190) : RGB(200, 220, 250);
        if (capture && (int)i == hover_) fill = RGB(50, 140, 245);
        if (fill != RGB(248, 248, 250)) ui::FillRoundRect(mem, r, ui::Scale(10, dpi), fill, act ? RGB(120, 170, 240) : fill);
        COLORREF ic = capture ? RGB(255, 255, 255) : act ? RGB(20, 90, 200) : RGB(50, 50, 55);
        int isz = ui::Scale(compact_ ? 22 : 24, dpi);
        int iy = compact_ ? (r.top + r.bottom - isz) / 2 : r.top + ui::Scale(5, dpi);
        int ix = (r.left + r.right - isz) / 2;
        icons::Draw(g, it.icon, Gdiplus::RectF((float)ix, (float)iy, (float)isz, (float)isz),
                    Gdiplus::Color(255, GetRValue(ic), GetGValue(ic), GetBValue(ic)));
        if (!compact_) {
            RECT tr{ r.left - 4, r.bottom - ui::Scale(20, dpi), r.right + 4, r.bottom - ui::Scale(2, dpi) };
            ui::DrawTextCentered(mem, it.label, tr, ic, ui::UiFont(dpi, 7));
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
        int hv = self->Hit({ GET_X_LPARAM(l), GET_Y_LPARAM(l) });
        if (hv != self->hover_) {
            self->hover_ = hv; InvalidateRect(h, nullptr, FALSE);
            if (self->onHint) self->onHint(hv >= 0 ? self->items[hv].hint : L"");
        }
        TRACKMOUSEEVENT t{ sizeof(t), TME_LEAVE, h, 0 }; TrackMouseEvent(&t);
        return 0;
    }
    case WM_MOUSELEAVE: self->hover_ = -1; InvalidateRect(h, nullptr, FALSE); if (self->onHint) self->onHint(L""); return 0;
    case WM_LBUTTONDOWN: self->pressed_ = self->Hit({ GET_X_LPARAM(l), GET_Y_LPARAM(l) }); SetCapture(h); InvalidateRect(h, nullptr, FALSE); return 0;
    case WM_LBUTTONUP: {
        ReleaseCapture();
        int hit = self->Hit({ GET_X_LPARAM(l), GET_Y_LPARAM(l) });
        int p = self->pressed_;
        self->pressed_ = -1;
        InvalidateRect(h, nullptr, FALSE);
        if (hit >= 0 && hit == p && self->onClick) self->onClick(self->items[hit].id);
        return 0;
    }
    }
    return DefWindowProcW(h, m, w, l);
}

// ================================================================= propriedades
enum {
    P_TITLE = 300, P_COLOR, P_FILL, P_NOFILL, P_TEXTCOLOR, P_WIDTH, P_OPACITY, P_SHADOW, P_FONT, P_SIZE, P_BOLD,
    P_ARROW, P_SHAPE, P_CALLOUT, P_HIDE, P_AMOUNT, P_STAMP, P_STEP, P_STEPRESET, P_SELMODE, P_TOL, P_GLOBAL, P_SELFILL,
    P_CUTDIR, P_BTN1, P_BTN2, P_BTN3, P_FRONT, P_BACK, P_DELETE, P_HINT
};

static const int kWidths[] = { 1, 2, 3, 4, 5, 6, 8, 10, 12, 16, 20, 24, 32, 48 };
static const int kSizes[] = { 10, 12, 14, 16, 18, 20, 22, 24, 28, 32, 36, 42, 48, 64, 72, 96 };
static const wchar_t* kFonts[] = { L"Segoe UI", L"Arial", L"Calibri", L"Verdana", L"Georgia", L"Times New Roman", L"Consolas", L"Comic Sans MS", L"Impact", L"Tahoma" };

static Tool ToolForObj(ObjType t) {
    switch (t) {
    case ObjType::Arrow: return Tool::Arrow;
    case ObjType::Line: return Tool::Line;
    case ObjType::Rect: case ObjType::Ellipse: case ObjType::RoundRect: return Tool::Shape;
    case ObjType::Pen: return Tool::Pen;
    case ObjType::Highlighter: return Tool::Highlighter;
    case ObjType::Text: return Tool::Text;
    case ObjType::Callout: return Tool::Callout;
    case ObjType::Step: return Tool::Step;
    case ObjType::Stamp: return Tool::Stamp;
    case ObjType::Blur: case ObjType::Pixelate: return Tool::Blur;
    case ObjType::Spotlight: return Tool::Spotlight;
    case ObjType::Magnify: return Tool::Magnify;
    case ObjType::Picture: return Tool::Select;
    }
    return Tool::Select;
}

Tool PropsPanel::ShownTool() const {
    if (canvas->doc && canvas->selected >= 0 && canvas->selected < (int)canvas->doc->st.objs.size())
        return ToolForObj(canvas->doc->st.objs[canvas->selected]->type);
    return canvas->tool;
}

Style PropsPanel::ShownStyle() const {
    if (canvas->doc && canvas->selected >= 0 && canvas->selected < (int)canvas->doc->st.objs.size())
        return canvas->doc->st.objs[canvas->selected]->style;
    return canvas->styles[canvas->tool];
}

void PropsPanel::Create(HWND parent, Canvas* c) {
    canvas = c;
    static bool reg = false;
    if (!reg) {
        WNDCLASSW wc{}; wc.lpfnWndProc = Proc; wc.hInstance = GetModuleHandleW(nullptr); wc.hCursor = LoadCursor(nullptr, IDC_ARROW);
        wc.lpszClassName = L"SnapnabProps";
        RegisterClassW(&wc); reg = true;
    }
    hwnd = CreateWindowExW(0, L"SnapnabProps", nullptr, WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN | WS_VSCROLL, 0, 0, 10, 10, parent, nullptr,
                           GetModuleHandleW(nullptr), this);
    int dpi = DpiForWindow(parent);
    HINSTANCE hi = GetModuleHandleW(nullptr);
    auto add = [&](int id, const wchar_t* label, const wchar_t* cls, const wchar_t* text, DWORD style) {
        if (label) {
            HWND l = CreateWindowExW(0, L"STATIC", label, WS_CHILD | SS_LEFT, 0, 0, 10, 10, hwnd, nullptr, hi, nullptr);
            SendMessageW(l, WM_SETFONT, (WPARAM)ui::UiFont(dpi, 8), TRUE);
            lbl_[id] = l;
        }
        HWND h = CreateWindowExW(0, cls, text, WS_CHILD | WS_TABSTOP | style, 0, 0, 10, 10, hwnd, (HMENU)(INT_PTR)id, hi, nullptr);
        SendMessageW(h, WM_SETFONT, (WPARAM)(id == P_TITLE ? ui::UiFont(dpi, 11, true) : ui::UiFont(dpi)), TRUE);
        ctl_[id] = h;
        order_.push_back(id);
        return h;
    };
    add(P_TITLE, nullptr, L"STATIC", L"", SS_LEFT);
    add(P_HINT, nullptr, L"STATIC", L"", SS_LEFT);
    add(P_SHAPE, L"Forma", L"COMBOBOX", nullptr, CBS_DROPDOWNLIST);
    add(P_CALLOUT, L"Formato do balão", L"COMBOBOX", nullptr, CBS_DROPDOWNLIST);
    add(P_HIDE, L"Tipo", L"COMBOBOX", nullptr, CBS_DROPDOWNLIST);
    add(P_STAMP, L"Carimbo", L"COMBOBOX", nullptr, CBS_DROPDOWNLIST | WS_VSCROLL);
    add(P_SELMODE, L"Modo de seleção", L"COMBOBOX", nullptr, CBS_DROPDOWNLIST);
    add(P_COLOR, L"Cor", L"BUTTON", nullptr, BS_OWNERDRAW);
    add(P_FILL, L"Preenchimento", L"BUTTON", nullptr, BS_OWNERDRAW);
    add(P_NOFILL, nullptr, L"BUTTON", L"Sem preenchimento", BS_AUTOCHECKBOX);
    add(P_TEXTCOLOR, L"Cor do texto", L"BUTTON", nullptr, BS_OWNERDRAW);
    add(P_FONT, L"Fonte", L"COMBOBOX", nullptr, CBS_DROPDOWNLIST | WS_VSCROLL);
    add(P_SIZE, L"Tamanho", L"COMBOBOX", nullptr, CBS_DROPDOWNLIST | WS_VSCROLL);
    add(P_BOLD, nullptr, L"BUTTON", L"Negrito", BS_AUTOCHECKBOX);
    add(P_WIDTH, L"Espessura", L"COMBOBOX", nullptr, CBS_DROPDOWNLIST | WS_VSCROLL);
    add(P_ARROW, L"Pontas", L"COMBOBOX", nullptr, CBS_DROPDOWNLIST);
    add(P_AMOUNT, L"Intensidade", L"COMBOBOX", nullptr, CBS_DROPDOWNLIST | WS_VSCROLL);
    add(P_STEP, L"Próximo número", L"EDIT", L"1", ES_NUMBER | WS_BORDER);
    add(P_STEPRESET, nullptr, L"BUTTON", L"Reiniciar sequência", 0);
    add(P_TOL, L"Tolerância (0–100)", L"EDIT", L"15", ES_NUMBER | WS_BORDER);
    add(P_GLOBAL, nullptr, L"BUTTON", L"Seleção global (mesma cor em toda a imagem)", BS_AUTOCHECKBOX | BS_MULTILINE);
    add(P_SELFILL, L"Ao mover ou excluir, preencher com", L"COMBOBOX", nullptr, CBS_DROPDOWNLIST);
    add(P_CUTDIR, L"Direção", L"COMBOBOX", nullptr, CBS_DROPDOWNLIST);
    add(P_OPACITY, L"Opacidade", L"COMBOBOX", nullptr, CBS_DROPDOWNLIST | WS_VSCROLL);
    add(P_SHADOW, nullptr, L"BUTTON", L"Sombra", BS_AUTOCHECKBOX);
    add(P_BTN1, nullptr, L"BUTTON", L"", 0);
    add(P_BTN2, nullptr, L"BUTTON", L"", 0);
    add(P_BTN3, nullptr, L"BUTTON", L"", 0);
    add(P_FRONT, nullptr, L"BUTTON", L"Trazer para a frente", 0);
    add(P_BACK, nullptr, L"BUTTON", L"Enviar para trás", 0);
    add(P_DELETE, nullptr, L"BUTTON", L"Excluir objeto", 0);

    for (auto s : { L"Retângulo", L"Elipse", L"Retângulo arredondado" }) ComboBox_AddString(Ctl(P_SHAPE), s);
    for (auto s : { L"Arredondado", L"Retangular", L"Elipse", L"Pensamento" }) ComboBox_AddString(Ctl(P_CALLOUT), s);
    for (auto s : { L"Desfoque", L"Pixelado" }) ComboBox_AddString(Ctl(P_HIDE), s);
    for (auto& g : StampGlyphs()) ComboBox_AddString(Ctl(P_STAMP), g.c_str());
    SendMessageW(Ctl(P_STAMP), WM_SETFONT, (WPARAM)[] { LOGFONTW lf{}; lf.lfHeight = -18; wcscpy_s(lf.lfFaceName, L"Segoe UI Symbol"); return CreateFontIndirectW(&lf); }(), TRUE);
    for (auto s : { L"Retângulo", L"Elipse", L"Laço (mão livre)", L"Varinha mágica" }) ComboBox_AddString(Ctl(P_SELMODE), s);
    for (auto s : { L"Transparência", L"Cor do canvas (branco)", L"Preenchimento inteligente" }) ComboBox_AddString(Ctl(P_SELFILL), s);
    for (auto s : { L"Horizontal (remove linhas)", L"Vertical (remove colunas)" }) ComboBox_AddString(Ctl(P_CUTDIR), s);
    for (auto f : kFonts) ComboBox_AddString(Ctl(P_FONT), f);
    for (int s : kSizes) ComboBox_AddString(Ctl(P_SIZE), std::to_wstring(s).c_str());
    for (int s : kWidths) ComboBox_AddString(Ctl(P_WIDTH), (std::to_wstring(s) + L" px").c_str());
    for (auto s : { L"Seta no fim", L"Seta no início", L"Setas nas duas pontas", L"Sem seta" }) ComboBox_AddString(Ctl(P_ARROW), s);
    for (int a = 2; a <= 30; a += 2) ComboBox_AddString(Ctl(P_AMOUNT), std::to_wstring(a).c_str());
    for (int o = 100; o >= 10; o -= 10) ComboBox_AddString(Ctl(P_OPACITY), (std::to_wstring(o) + L"%").c_str());
}

static bool Vis(Tool t, int id, bool objSelected) {
    switch (id) {
    case P_TITLE: case P_HINT: return true;
    case P_FRONT: case P_BACK: case P_DELETE: return objSelected;
    }
    switch (t) {
    case Tool::Arrow: return id == P_COLOR || id == P_WIDTH || id == P_OPACITY || id == P_SHADOW || id == P_ARROW;
    case Tool::Line: case Tool::Pen: return id == P_COLOR || id == P_WIDTH || id == P_OPACITY || id == P_SHADOW;
    case Tool::Shape: return id == P_SHAPE || id == P_COLOR || id == P_FILL || id == P_NOFILL || id == P_WIDTH || id == P_OPACITY || id == P_SHADOW;
    case Tool::Highlighter: return id == P_COLOR || id == P_WIDTH || id == P_OPACITY;
    case Tool::Text: return id == P_TEXTCOLOR || id == P_FONT || id == P_SIZE || id == P_BOLD || id == P_FILL || id == P_NOFILL || id == P_OPACITY || id == P_SHADOW;
    case Tool::Callout: return id == P_CALLOUT || id == P_FILL || id == P_NOFILL || id == P_COLOR || id == P_WIDTH || id == P_TEXTCOLOR || id == P_FONT ||
                               id == P_SIZE || id == P_BOLD || id == P_OPACITY || id == P_SHADOW;
    case Tool::Step: return id == P_COLOR || id == P_SIZE || id == P_STEP || id == P_STEPRESET || id == P_OPACITY || id == P_SHADOW;
    case Tool::Stamp: return id == P_STAMP || id == P_COLOR || id == P_OPACITY || id == P_SHADOW;
    case Tool::Blur: return id == P_HIDE || id == P_AMOUNT;
    case Tool::Spotlight: return id == P_AMOUNT;
    case Tool::Magnify: return id == P_AMOUNT || id == P_COLOR || id == P_WIDTH || id == P_SHADOW;
    case Tool::Fill: return id == P_COLOR || id == P_OPACITY || id == P_TOL || id == P_GLOBAL;
    case Tool::Eraser: return id == P_WIDTH;
    case Tool::Crop: return id == P_BTN1 || id == P_BTN2;
    case Tool::Selection: return id == P_SELMODE || id == P_TOL || id == P_GLOBAL || id == P_SELFILL || id == P_BTN1 || id == P_BTN2 || id == P_BTN3;
    case Tool::Cutout: return id == P_CUTDIR;
    case Tool::Eyedropper: return id == P_COLOR;
    default: return false;
    }
}

static int IndexOf(const int* arr, int n, int v) {
    int best = 0;
    for (int i = 0; i < n; i++) if (std::abs(arr[i] - v) < std::abs(arr[best] - v)) best = i;
    return best;
}

void PropsPanel::Load() {
    if (!canvas) return;
    loading_ = true;
    Tool t = ShownTool();
    bool objSel = canvas->doc && canvas->selected >= 0 && canvas->selected < (int)canvas->doc->st.objs.size();
    cur_ = ShownStyle();
    std::wstring title = objSel ? std::wstring(L"Objeto: ") + (t == Tool::Select ? L"Imagem" : ToolName(t)) : ToolName(canvas->tool);
    SetWindowTextW(Ctl(P_TITLE), title.c_str());
    std::wstring hint;
    switch (canvas->tool) {
    case Tool::Select: hint = objSel ? L"Arraste para mover; use as alças para redimensionar." : L"Clique num objeto para selecioná-lo. Arraste o fundo para rolar."; break;
    case Tool::Crop: hint = L"Arraste a área a manter; Enter ou duplo clique aplica."; break;
    case Tool::Selection: hint = L"Selecione e arraste para mover (Smart Move); Delete apaga; Ctrl+C copia."; break;
    case Tool::Cutout: hint = L"Arraste sobre a faixa a remover; as partes são unidas."; break;
    case Tool::Eyedropper: hint = L"Clique na imagem para pegar a cor."; break;
    case Tool::Fill: hint = L"Clique para preencher a área de cor semelhante."; break;
    case Tool::Eraser: hint = L"Arraste para apagar pixels da imagem (transparente)."; break;
    case Tool::Step: hint = L"Clique para numerar passos em sequência."; break;
    default: hint = L"Shift restringe proporção/ângulo. Ctrl+clique desenha sobre um objeto."; break;
    }
    SetWindowTextW(Ctl(P_HINT), hint.c_str());
    ComboBox_SetCurSel(Ctl(P_SHAPE), canvas->shapeKind);
    if (objSel) {
        ObjType ot = canvas->doc->st.objs[canvas->selected]->type;
        if (ot == ObjType::Ellipse) ComboBox_SetCurSel(Ctl(P_SHAPE), 1);
        else if (ot == ObjType::RoundRect) ComboBox_SetCurSel(Ctl(P_SHAPE), 2);
        else if (ot == ObjType::Rect) ComboBox_SetCurSel(Ctl(P_SHAPE), 0);
        ComboBox_SetCurSel(Ctl(P_HIDE), ot == ObjType::Pixelate ? 1 : 0);
        if (ot == ObjType::Stamp) {
            auto g = StampGlyphs();
            for (size_t i = 0; i < g.size(); i++) if (g[i] == canvas->doc->st.objs[canvas->selected]->text) ComboBox_SetCurSel(Ctl(P_STAMP), (int)i);
        }
    } else {
        ComboBox_SetCurSel(Ctl(P_HIDE), canvas->hideKind);
        ComboBox_SetCurSel(Ctl(P_STAMP), canvas->stampIndex);
    }
    ComboBox_SetCurSel(Ctl(P_CALLOUT), cur_.calloutShape);
    ComboBox_SetCurSel(Ctl(P_SELMODE), canvas->selMode);
    ComboBox_SetCurSel(Ctl(P_SELFILL), canvas->selFill);
    ComboBox_SetCurSel(Ctl(P_CUTDIR), canvas->cutDir);
    Button_SetCheck(Ctl(P_NOFILL), (cur_.fill >> 24) == 0);
    Button_SetCheck(Ctl(P_SHADOW), cur_.shadow);
    Button_SetCheck(Ctl(P_BOLD), cur_.bold);
    Button_SetCheck(Ctl(P_GLOBAL), canvas->wandGlobal);
    int fi = 0;
    for (int i = 0; i < (int)(sizeof kFonts / sizeof *kFonts); i++) if (cur_.font == kFonts[i]) fi = i;
    ComboBox_SetCurSel(Ctl(P_FONT), fi);
    ComboBox_SetCurSel(Ctl(P_SIZE), IndexOf(kSizes, sizeof kSizes / sizeof *kSizes, (int)cur_.fontSize));
    ComboBox_SetCurSel(Ctl(P_WIDTH), IndexOf(kWidths, sizeof kWidths / sizeof *kWidths, (int)std::lround(cur_.width)));
    int arrow = cur_.arrowStart && cur_.arrowEnd ? 2 : cur_.arrowStart ? 1 : cur_.arrowEnd ? 0 : 3;
    ComboBox_SetCurSel(Ctl(P_ARROW), arrow);
    ComboBox_SetCurSel(Ctl(P_AMOUNT), std::clamp((cur_.amount - 2) / 2, 0, 14));
    ComboBox_SetCurSel(Ctl(P_OPACITY), std::clamp((100 - cur_.opacity) / 10, 0, 9));
    SetWindowTextW(Ctl(P_STEP), std::to_wstring(canvas->doc ? canvas->doc->st.nextStep : 1).c_str());
    SetWindowTextW(Ctl(P_TOL), std::to_wstring((int)canvas->tolerance).c_str());
    if (canvas->tool == Tool::Crop) { SetWindowTextW(Ctl(P_BTN1), L"Aplicar recorte"); SetWindowTextW(Ctl(P_BTN2), L"Corte automático"); }
    else { SetWindowTextW(Ctl(P_BTN1), L"Cortar para a seleção"); SetWindowTextW(Ctl(P_BTN2), L"Copiar seleção"); SetWindowTextW(Ctl(P_BTN3), L"Excluir seleção"); }
    for (int id : order_) {
        bool v = Vis(t, id, objSel);
        if (canvas->tool == Tool::Select && !objSel && id != P_TITLE && id != P_HINT) v = false;
        ShowWindow(Ctl(id), v ? SW_SHOW : SW_HIDE);
        if (lbl_.count(id)) ShowWindow(lbl_[id], v ? SW_SHOW : SW_HIDE);
    }
    for (int id : { P_COLOR, P_FILL, P_TEXTCOLOR }) InvalidateRect(Ctl(id), nullptr, TRUE);
    loading_ = false;
    Layout();
}

void PropsPanel::Layout() {
    RECT cr; GetClientRect(hwnd, &cr);
    int dpi = DpiForWindow(hwnd);
    int pad = ui::Scale(12, dpi), w = cr.right - 2 * pad, y = pad;
    for (int id : order_) {
        if (!IsWindowVisible(Ctl(id))) continue;
        if (lbl_.count(id)) { MoveWindow(lbl_[id], pad, y, w, ui::Scale(16, dpi), TRUE); y += ui::Scale(17, dpi); }
        int h = ui::Scale(26, dpi);
        if (id == P_TITLE) h = ui::Scale(26, dpi);
        if (id == P_HINT) h = ui::Scale(48, dpi);
        if (id == P_GLOBAL) h = ui::Scale(36, dpi);
        wchar_t cls[16] = L""; GetClassNameW(Ctl(id), cls, 16);
        bool combo = !_wcsicmp(cls, L"ComboBox");
        MoveWindow(Ctl(id), pad, y, w, combo ? ui::Scale(260, dpi) : h, TRUE);
        y += h + ui::Scale(8, dpi);
    }
    contentBottom_ = y;
    InvalidateRect(hwnd, nullptr, TRUE);
}

void PropsPanel::Save(int id) {
    if (loading_ || !canvas) return;
    Style s = ShownStyle();
    bool objSel = canvas->doc && canvas->selected >= 0 && canvas->selected < (int)canvas->doc->st.objs.size();
    Tool t = ShownTool();
    wchar_t buf[32];
    switch (id) {
    case P_NOFILL: if (Button_GetCheck(Ctl(P_NOFILL)) == BST_CHECKED) s.fill &= 0x00FFFFFF; else s.fill |= 0xFF000000; break;
    case P_SHADOW: s.shadow = Button_GetCheck(Ctl(P_SHADOW)) == BST_CHECKED; break;
    case P_BOLD: s.bold = Button_GetCheck(Ctl(P_BOLD)) == BST_CHECKED; break;
    case P_FONT: { int i = ComboBox_GetCurSel(Ctl(P_FONT)); if (i >= 0) s.font = kFonts[i]; break; }
    case P_SIZE: { int i = ComboBox_GetCurSel(Ctl(P_SIZE)); if (i >= 0) s.fontSize = (float)kSizes[i]; break; }
    case P_WIDTH: { int i = ComboBox_GetCurSel(Ctl(P_WIDTH)); if (i >= 0) s.width = (float)kWidths[i]; break; }
    case P_ARROW: { int i = ComboBox_GetCurSel(Ctl(P_ARROW)); s.arrowEnd = i == 0 || i == 2; s.arrowStart = i == 1 || i == 2; break; }
    case P_AMOUNT: { int i = ComboBox_GetCurSel(Ctl(P_AMOUNT)); if (i >= 0) s.amount = 2 + i * 2; break; }
    case P_OPACITY: { int i = ComboBox_GetCurSel(Ctl(P_OPACITY)); if (i >= 0) s.opacity = 100 - i * 10; break; }
    case P_CALLOUT: s.calloutShape = std::max(0, ComboBox_GetCurSel(Ctl(P_CALLOUT))); break;
    case P_SHAPE: canvas->shapeKind = std::max(0, ComboBox_GetCurSel(Ctl(P_SHAPE))); if (objSel) canvas->ChangeSelectedKind(); return;
    case P_HIDE: canvas->hideKind = std::max(0, ComboBox_GetCurSel(Ctl(P_HIDE))); if (objSel) canvas->ChangeSelectedKind(); return;
    case P_STAMP: canvas->stampIndex = std::max(0, ComboBox_GetCurSel(Ctl(P_STAMP))); if (objSel) canvas->ChangeSelectedKind(); return;
    case P_SELMODE: canvas->selMode = std::max(0, ComboBox_GetCurSel(Ctl(P_SELMODE))); canvas->ClearRasterSelection(); return;
    case P_SELFILL: canvas->selFill = std::max(0, ComboBox_GetCurSel(Ctl(P_SELFILL))); return;
    case P_CUTDIR: canvas->cutDir = std::max(0, ComboBox_GetCurSel(Ctl(P_CUTDIR))); return;
    case P_GLOBAL: canvas->wandGlobal = Button_GetCheck(Ctl(P_GLOBAL)) == BST_CHECKED; return;
    case P_TOL: GetWindowTextW(Ctl(P_TOL), buf, 32); canvas->tolerance = (float)std::clamp((int)wcstol(buf, nullptr, 10), 0, 100); return;
    case P_STEP: GetWindowTextW(Ctl(P_STEP), buf, 32); if (canvas->doc) canvas->doc->st.nextStep = std::max(1, (int)wcstol(buf, nullptr, 10)); return;
    default: break;
    }
    canvas->styles[t == Tool::Select ? canvas->tool : t] = s;
    if (objSel) canvas->ApplyStyleToSelected(s);
}

LRESULT CALLBACK PropsPanel::Proc(HWND h, UINT m, WPARAM w, LPARAM l) {
    PropsPanel* self = (PropsPanel*)GetWindowLongPtrW(h, GWLP_USERDATA);
    if (m == WM_NCCREATE) { self = (PropsPanel*)((CREATESTRUCTW*)l)->lpCreateParams; SetWindowLongPtrW(h, GWLP_USERDATA, (LONG_PTR)self); self->hwnd = h; }
    return self ? self->Handle(m, w, l) : DefWindowProcW(h, m, w, l);
}

LRESULT PropsPanel::Handle(UINT m, WPARAM w, LPARAM l) {
    switch (m) {
    case WM_SIZE: Layout(); return 0;
    case WM_ERASEBKGND: {
        RECT cr; GetClientRect(hwnd, &cr);
        HBRUSH b = CreateSolidBrush(RGB(246, 247, 249)); FillRect((HDC)w, &cr, b); DeleteObject(b);
        RECT line{ 0, 0, 1, cr.bottom }; HBRUSH lb = CreateSolidBrush(RGB(220, 222, 226)); FillRect((HDC)w, &line, lb); DeleteObject(lb);
        if (!docInfo_.empty() && cr.bottom - ui::Scale(110, DpiForWindow(hwnd)) > contentBottom_) {
            int dpi = DpiForWindow(hwnd);
            RECT tr{ ui::Scale(12, dpi), cr.bottom - ui::Scale(110, dpi), cr.right - ui::Scale(10, dpi), cr.bottom - ui::Scale(8, dpi) };
            HGDIOBJ of = SelectObject((HDC)w, ui::UiFont(dpi, 8));
            SetBkMode((HDC)w, TRANSPARENT); SetTextColor((HDC)w, RGB(110, 110, 115));
            DrawTextW((HDC)w, docInfo_.c_str(), -1, &tr, DT_LEFT | DT_BOTTOM | DT_WORDBREAK | DT_END_ELLIPSIS);
            SelectObject((HDC)w, of);
        }
        return 1;
    }
    case WM_CTLCOLORSTATIC: case WM_CTLCOLORBTN: {
        static HBRUSH b = CreateSolidBrush(RGB(246, 247, 249));
        SetBkMode((HDC)w, TRANSPARENT);
        if ((HWND)l == ctl_[P_HINT]) SetTextColor((HDC)w, RGB(100, 100, 105));
        return (LRESULT)b;
    }
    case WM_DRAWITEM: {
        auto* di = (DRAWITEMSTRUCT*)l;
        Style s = ShownStyle();
        uint32_t c = di->CtlID == P_FILL ? s.fill : di->CtlID == P_TEXTCOLOR ? s.textColor : s.stroke;
        HBRUSH bg = CreateSolidBrush(RGB(246, 247, 249)); FillRect(di->hDC, &di->rcItem, bg); DeleteObject(bg);
        RECT r = di->rcItem; InflateRect(&r, -1, -1);
        ui::FillRoundRect(di->hDC, r, 6, RGB(255, 255, 255), RGB(190, 192, 196));
        int dpi = DpiForWindow(hwnd);
        RECT sw{ r.left + 4, r.top + 4, r.left + 4 + (r.bottom - r.top - 8) * 2, r.bottom - 4 };
        if ((c >> 24) == 0) {
            HBRUSH wb = CreateSolidBrush(RGB(255, 255, 255)); FillRect(di->hDC, &sw, wb); DeleteObject(wb);
            HPEN rp = CreatePen(PS_SOLID, 2, RGB(220, 40, 40)); HGDIOBJ op = SelectObject(di->hDC, rp);
            MoveToEx(di->hDC, sw.left, sw.bottom, nullptr); LineTo(di->hDC, sw.right, sw.top);
            SelectObject(di->hDC, op); DeleteObject(rp);
        } else ui::FillRoundRect(di->hDC, sw, 4, ToColorref(c), RGB(120, 120, 120));
        RECT tr = r; tr.left = sw.right + 8;
        HGDIOBJ of = SelectObject(di->hDC, ui::UiFont(dpi));
        SetBkMode(di->hDC, TRANSPARENT); SetTextColor(di->hDC, RGB(40, 40, 40));
        std::wstring t = (c >> 24) ? ColorToHex(ToColorref(c)) : L"nenhum";
        DrawTextW(di->hDC, t.c_str(), -1, &tr, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
        SelectObject(di->hDC, of);
        return TRUE;
    }
    case WM_COMMAND: {
        int id = LOWORD(w), code = HIWORD(w);
        if (id == P_COLOR || id == P_FILL || id == P_TEXTCOLOR) {
            if (code != BN_CLICKED) return 0;
            Style s = ShownStyle();
            uint32_t& c = id == P_FILL ? s.fill : id == P_TEXTCOLOR ? s.textColor : s.stroke;
            COLORREF cr = ToColorref(c);
            if (!PickColor(GetAncestor(hwnd, GA_ROOT), cr)) return 0;
            c = FromColorref(cr);
            Tool t = ShownTool();
            canvas->styles[t == Tool::Select ? canvas->tool : t] = s;
            if (canvas->doc && canvas->selected >= 0) canvas->ApplyStyleToSelected(s);
            Load();
            return 0;
        }
        if (id == P_STEPRESET) { if (canvas->doc) canvas->doc->st.nextStep = 1; Load(); return 0; }
        if (id == P_BTN1 || id == P_BTN2 || id == P_BTN3 || id == P_FRONT || id == P_BACK || id == P_DELETE) {
            if (onCommand) onCommand(id);
            return 0;
        }
        bool isCombo = code == CBN_SELCHANGE, isCheck = code == BN_CLICKED, isEdit = code == EN_CHANGE;
        if (isCombo || isCheck || isEdit) Save(id);
        return 0;
    }
    }
    return DefWindowProcW(hwnd, m, w, l);
}

// ================================================================= bandeja da biblioteca
void LibraryTray::Create(HWND parent) {
    static bool reg = false;
    if (!reg) {
        WNDCLASSW wc{}; wc.lpfnWndProc = Proc; wc.hInstance = GetModuleHandleW(nullptr); wc.hCursor = LoadCursor(nullptr, IDC_ARROW);
        wc.lpszClassName = L"SnapnabTray";
        RegisterClassW(&wc); reg = true;
    }
    hwnd = CreateWindowExW(0, L"SnapnabTray", nullptr, WS_CHILD | WS_VISIBLE | WS_HSCROLL, 0, 0, 10, 10, parent, nullptr,
                           GetModuleHandleW(nullptr), this);
}

int LibraryTray::Height() const { return ui::Scale(130, DpiForWindow(hwnd)); }

void LibraryTray::Reload() {
    items_ = ListLibrary();
    // miniaturas vazias (arquivo ainda sendo gravado) são tentadas de novo
    for (auto it = thumbs_.begin(); it != thumbs_.end();) it = it->second.empty() ? thumbs_.erase(it) : std::next(it);
    InvalidateRect(hwnd, nullptr, FALSE);
    SendMessageW(hwnd, WM_SIZE, 0, 0);
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

static void CellMetrics(HWND h, int& cw, int& ch, int& pad) {
    int dpi = DpiForWindow(h);
    cw = ui::Scale(128, dpi); ch = ui::Scale(100, dpi); pad = ui::Scale(8, dpi);
}

int LibraryTray::Hit(POINT p) const {
    int cw, ch, pad; CellMetrics(hwnd, cw, ch, pad);
    int dpi = DpiForWindow(hwnd);
    int top = ui::Scale(22, dpi);
    if (p.y < top || p.y > top + ch) return -1;
    int i = (p.x + scroll_ - pad) / (cw + pad);
    int x0 = pad + i * (cw + pad) - scroll_;
    if (i < 0 || i >= (int)items_.size() || p.x < x0 || p.x > x0 + cw) return -1;
    return i;
}

void LibraryTray::EnsureVisible() {
    RECT cr; GetClientRect(hwnd, &cr);
    int cw, ch, pad; CellMetrics(hwnd, cw, ch, pad);
    for (size_t i = 0; i < items_.size(); i++) {
        if (_wcsicmp(items_[i].path.c_str(), current.c_str())) continue;
        int x0 = pad + (int)i * (cw + pad);
        if (x0 < scroll_) scroll_ = x0 - pad;
        else if (x0 + cw > scroll_ + cr.right) scroll_ = x0 + cw + pad - cr.right;
    }
    SendMessageW(hwnd, WM_SIZE, 0, 0);
}

void LibraryTray::Paint(HDC hdc) {
    RECT cr; GetClientRect(hwnd, &cr);
    HDC mem = CreateCompatibleDC(hdc);
    HBITMAP bb = CreateCompatibleBitmap(hdc, std::max(1L, cr.right), std::max(1L, cr.bottom));
    HGDIOBJ ob = SelectObject(mem, bb);
    HBRUSH bg = CreateSolidBrush(RGB(236, 237, 240)); FillRect(mem, &cr, bg); DeleteObject(bg);
    RECT line{ 0, 0, cr.right, 1 }; HBRUSH lb = CreateSolidBrush(RGB(214, 216, 220)); FillRect(mem, &line, lb); DeleteObject(lb);
    int dpi = DpiForWindow(hwnd);
    RECT hr{ ui::Scale(10, dpi), ui::Scale(3, dpi), cr.right, ui::Scale(21, dpi) };
    HGDIOBJ of = SelectObject(mem, ui::UiFont(dpi, 8, true));
    SetBkMode(mem, TRANSPARENT); SetTextColor(mem, RGB(90, 90, 95));
    std::wstring head = Format(L"Biblioteca — capturas recentes (%zu)", items_.size());
    DrawTextW(mem, head.c_str(), -1, &hr, DT_LEFT | DT_SINGLELINE | DT_VCENTER);
    int cw, ch, pad; CellMetrics(hwnd, cw, ch, pad);
    int top = ui::Scale(22, dpi);
    SelectObject(mem, ui::UiFont(dpi, 8));
    for (size_t i = 0; i < items_.size(); i++) {
        int x0 = pad + (int)i * (cw + pad) - scroll_;
        if (x0 + cw < 0) continue;
        if (x0 > cr.right) break;
        RECT cell{ x0, top, x0 + cw, top + ch };
        bool cur = !_wcsicmp(items_[i].path.c_str(), current.c_str());
        ui::FillRoundRect(mem, cell, ui::Scale(8, dpi), cur ? RGB(214, 232, 255) : (int)i == hover_ ? RGB(250, 250, 252) : RGB(244, 245, 247),
                          cur ? RGB(60, 140, 240) : RGB(214, 216, 220));
        const Image& t = Thumb(items_[i].path, items_[i].video);
        int th = ch - ui::Scale(26, dpi), tw = cw - ui::Scale(12, dpi);
        if (!t.empty()) {
            double s = std::min((double)tw / t.w, (double)th / t.h);
            int w = std::max(1, (int)(t.w * s)), h = std::max(1, (int)(t.h * s));
            int ix = x0 + (cw - w) / 2, iy = top + ui::Scale(6, dpi) + (th - h) / 2;
            if (t.HasTransparency()) { Image o(t.w, t.h, 0xFFFFFFFF); o.Blit(t, 0, 0); o.DrawTo(mem, ix, iy, w, h, true); }
            else t.DrawTo(mem, ix, iy, w, h, true);
            if (items_[i].video) {
                RECT badge{ ix + 4, iy + h - ui::Scale(20, dpi), ix + ui::Scale(28, dpi), iy + h - 4 };
                ui::FillRoundRect(mem, badge, 6, RGB(20, 20, 20), RGB(20, 20, 20));
                icons::DrawOnDC(mem, icons::Video, { badge.left + 3, badge.top + 1, badge.right - 3, badge.bottom - 1 }, RGB(255, 255, 255));
            }
        }
        SYSTEMTIME st, lt; FileTimeToSystemTime(&items_[i].modified, &st); SystemTimeToTzSpecificLocalTime(nullptr, &st, &lt);
        std::wstring label = Format(L"%02d/%02d %02d:%02d", lt.wDay, lt.wMonth, lt.wHour, lt.wMinute);
        RECT lr{ x0 + 4, cell.bottom - ui::Scale(20, dpi), cell.right - 4, cell.bottom - 2 };
        SetTextColor(mem, RGB(80, 80, 85));
        DrawTextW(mem, label.c_str(), -1, &lr, DT_CENTER | DT_SINGLELINE | DT_VCENTER);
    }
    if (items_.empty()) {
        RECT er{ 0, top, cr.right, top + ch };
        ui::DrawTextCentered(mem, L"A biblioteca está vazia. Capture algo com o Snapnab (Print Screen).", er, RGB(120, 120, 125), ui::UiFont(dpi, 9));
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
    case WM_SIZE: {
        RECT cr; GetClientRect(hwnd, &cr);
        int cw, ch, pad; CellMetrics(hwnd, cw, ch, pad);
        int total = pad + (int)items_.size() * (cw + pad);
        scroll_ = std::clamp(scroll_, 0, std::max(0, total - (int)cr.right));
        SCROLLINFO si{ sizeof(si), SIF_ALL };
        si.nMax = total; si.nPage = cr.right; si.nPos = scroll_;
        SetScrollInfo(hwnd, SB_HORZ, &si, TRUE);
        InvalidateRect(hwnd, nullptr, FALSE);
        return 0;
    }
    case WM_ERASEBKGND: return 1;
    case WM_PAINT: { PAINTSTRUCT ps; HDC dc = BeginPaint(hwnd, &ps); Paint(dc); EndPaint(hwnd, &ps); return 0; }
    case WM_HSCROLL: {
        SCROLLINFO si{ sizeof(si), SIF_ALL }; GetScrollInfo(hwnd, SB_HORZ, &si);
        switch (LOWORD(w)) {
        case SB_LINELEFT: scroll_ -= 60; break; case SB_LINERIGHT: scroll_ += 60; break;
        case SB_PAGELEFT: scroll_ -= si.nPage; break; case SB_PAGERIGHT: scroll_ += si.nPage; break;
        case SB_THUMBTRACK: case SB_THUMBPOSITION: scroll_ = si.nTrackPos; break;
        }
        SendMessageW(hwnd, WM_SIZE, 0, 0);
        return 0;
    }
    case WM_MOUSEWHEEL: scroll_ -= GET_WHEEL_DELTA_WPARAM(w); SendMessageW(hwnd, WM_SIZE, 0, 0); return 0;
    case WM_MOUSEMOVE: {
        int hv = Hit({ GET_X_LPARAM(l), GET_Y_LPARAM(l) });
        if (hv != hover_) { hover_ = hv; InvalidateRect(hwnd, nullptr, FALSE); }
        TRACKMOUSEEVENT t{ sizeof(t), TME_LEAVE, hwnd, 0 }; TrackMouseEvent(&t);
        return 0;
    }
    case WM_MOUSELEAVE: hover_ = -1; InvalidateRect(hwnd, nullptr, FALSE); return 0;
    case WM_LBUTTONUP: {
        int i = Hit({ GET_X_LPARAM(l), GET_Y_LPARAM(l) });
        if (i >= 0 && onOpen) onOpen(items_[i].path);
        return 0;
    }
    case WM_RBUTTONUP: {
        int i = Hit({ GET_X_LPARAM(l), GET_Y_LPARAM(l) });
        if (i < 0) return 0;
        std::wstring path = items_[i].path;
        HMENU mm = CreatePopupMenu();
        AppendMenuW(mm, MF_STRING, 1, L"Abrir");
        AppendMenuW(mm, MF_STRING, 2, L"Mostrar na pasta");
        AppendMenuW(mm, MF_SEPARATOR, 0, nullptr);
        AppendMenuW(mm, MF_STRING, 3, L"Excluir (Lixeira)");
        POINT p; GetCursorPos(&p);
        int cmd = TrackPopupMenu(mm, TPM_RETURNCMD | TPM_RIGHTBUTTON, p.x, p.y, 0, hwnd, nullptr);
        DestroyMenu(mm);
        if (cmd == 1 && onOpen) onOpen(path);
        else if (cmd == 2) ShellExecuteW(nullptr, L"open", L"explorer.exe", (L"/select,\"" + path + L"\"").c_str(), nullptr, SW_SHOWNORMAL);
        else if (cmd == 3 && onDelete) onDelete(path);
        return 0;
    }
    }
    return DefWindowProcW(hwnd, m, w, l);
}

} // namespace snap::ed
