// Snapnab.exe — processo de captura (≙ Snagit32.exe): bandeja, janela de captura, atalhos globais,
// motor de captura, gravação de vídeo, saídas nativas e canal IPC com o Editor (ds_pseudocode §1–§4)
#include "Common.h"
#include "CaptureEngine.h"
#include "Hotkeys.h"
#include "Icons.h"
#include "Ipc.h"
#include "Library.h"
#include "Presets.h"
#include "Ui.h"
#include "Widgets.h"
#include <dwmapi.h>

using namespace snap;

namespace {

enum : int {
    IDC_PRESETS = 100, IDC_PRESET_NEW, IDC_PRESET_DUP, IDC_PRESET_DEL, IDC_PRESET_RENAME, IDC_PRESET_HOTKEY,
    IDC_TAB_IMAGE, IDC_TAB_VIDEO, IDC_SELECTION, IDC_SEL_CFG, IDC_EFFECTS, IDC_EFFECTS_LABEL,
    IDC_PREVIEW, IDC_CLIPBOARD, IDC_SAVEFILE, IDC_SAVEFILE_CFG, IDC_CURSOR, IDC_DELAY, IDC_DELAY_UD, IDC_DELAY_LABEL,
    IDC_MIC, IDC_SYS, IDC_FPS, IDC_FPS_LABEL, IDC_QUALITY, IDC_QUALITY_LABEL, IDC_DOWNSCALE, IDC_HW,
    IDC_CAPTURE, IDC_HINT, IDC_OPEN_EDITOR, IDC_OPTIONS, IDC_SEL_LABEL, IDC_SHARE_LABEL, IDC_PRESETS_LABEL,
    IDC_EFFECTS_TITLE, IDC_FILE_SUMMARY,
    // menu
    IDM_EXIT = 1000, IDM_OPEN_EDITOR, IDM_CAPTURE_IMAGE, IDM_CAPTURE_VIDEO, IDM_CAPTURE_CLIPBOARD, IDM_CAPTURE_FILE,
    IDM_REPEAT, IDM_OPTIONS, IDM_PRESET_IMPORT, IDM_PRESET_EXPORT, IDM_ABOUT, IDM_LIBRARY_FOLDER, IDM_LOGS,
    IDM_SHOW, IDM_PRESET_BASE = 2000,
};
constexpr UINT WM_FX_ADD = WM_APP + 0x60;     // reabrir a lista de efeitos em "adicionar outro"
enum : int { HK_IMAGE = 1, HK_VIDEO = 2, HK_VIDEO_PAUSE = 4, HK_VIDEO_STOP = 5, HK_PRESET_BASE = 100 };

struct App {
    HINSTANCE hi = nullptr;
    HWND frame = nullptr;
    HICON iconBig = nullptr, iconSmall = nullptr;
    NOTIFYICONDATAW nid{};
    UINT taskbarCreated = 0;
    PresetStore store;
    CaptureEngine engine;
    ipc::Notify ipc;
    HotkeyManager hk;
    HANDLE mutex = nullptr;
    int dpi = 96;
    std::map<int, HWND> ctl;
    bool loading = false;
    bool hoverCapture = false;
    DWORD lastClipSeq = 0;
    Preset lastPreset;
    bool haveLast = false;
    bool presetsOpen = false;      // lista de presets expandida abaixo da barra inferior
    int fxCurrent = -1;            // efeito mostrado na caixa "Efeitos" (alvo de "Remover este efeito")
    bool fxAddMode = false;        // a lista de efeitos aberta é a de "Adicionar outro efeito"
} g;

// medidas da janela compacta (calculadas pelo texto dos controles, em pixels do DPI atual)
struct Metrics {
    int pad, rowP, ch, tabW, tabH;
    int ax, labW, fieldX, fieldW, cfgX, cfgW;     // coluna de seleção/efeitos/arquivo
    int bx, bw;                                   // coluna de opções (caixas de seleção)
    int capX, capW;                               // botão de captura
    int contentH, barH, panelH, clientW;
} lay;

// cores derivadas da cor de destaque do Windows (Personalização > Cores)
struct Theme {
    COLORREF accent = RGB(0, 120, 215), accentDark = RGB(0, 90, 158), accentText = RGB(0, 84, 150);
    COLORREF bgMain = RGB(250, 250, 252), bgSide = RGB(240, 241, 244), bgBar = RGB(245, 246, 248);
    COLORREF bgCaption = RGB(243, 243, 245), line = RGB(222, 224, 228);
    HBRUSH brMain = nullptr, brSide = nullptr, brBar = nullptr, brCaption = nullptr;
} th;

HWND C(int id) { return g.ctl[id]; }
Preset& Cur() { return g.store.presets[g.store.selected]; }
int S(int v) { return ui::Scale(v, g.dpi); }

// ---------------------------------------------------------------- tema
COLORREF Mix(COLORREF a, COLORREF b, double t) {
    auto ch = [t](int x, int y) { return (BYTE)std::lround(x + (y - x) * t); };
    return RGB(ch(GetRValue(a), GetRValue(b)), ch(GetGValue(a), GetGValue(b)), ch(GetBValue(a), GetBValue(b)));
}

double Luma(COLORREF c) { return (0.299 * GetRValue(c) + 0.587 * GetGValue(c) + 0.114 * GetBValue(c)) / 255.0; }

bool ReadHkcuDword(const wchar_t* key, const wchar_t* name, DWORD& v) {
    DWORD cb = sizeof v;
    return RegGetValueW(HKEY_CURRENT_USER, key, name, RRF_RT_REG_DWORD, nullptr, &v, &cb) == ERROR_SUCCESS;
}

void LoadTheme() {
    // AccentPalette = 8 cores RGBA (Light3, Light2, Light1, Accent, Dark1, Dark2, Dark3, …)
    BYTE pal[32]; DWORD cb = sizeof pal;
    DWORD v;
    if (RegGetValueW(HKEY_CURRENT_USER, L"Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Accent", L"AccentPalette",
                     RRF_RT_REG_BINARY, nullptr, pal, &cb) == ERROR_SUCCESS && cb >= 24) {
        th.accent = RGB(pal[12], pal[13], pal[14]);
        th.accentDark = RGB(pal[16], pal[17], pal[18]);
        th.accentText = RGB(pal[20], pal[21], pal[22]);
    } else if (ReadHkcuDword(L"Software\\Microsoft\\Windows\\DWM", L"AccentColor", v)) {   // 0xAABBGGRR
        th.accent = v & 0xFFFFFF;
        th.accentDark = Mix(th.accent, 0, 0.25);
        th.accentText = Mix(th.accent, 0, 0.4);
    } else {
        BOOL opaque; DWORD argb;
        if (SUCCEEDED(DwmGetColorizationColor(&argb, &opaque))) th.accent = RGB((argb >> 16) & 255, (argb >> 8) & 255, argb & 255);
        th.accentDark = Mix(th.accent, 0, 0.25);
        th.accentText = Mix(th.accent, 0, 0.4);
    }
    while (Luma(th.accentText) > 0.42) th.accentText = Mix(th.accentText, 0, 0.15);   // legível sobre fundo claro
    // fundos neutros puxados de leve para a cor de destaque
    th.bgMain = Mix(RGB(251, 251, 252), th.accent, 0.05);
    th.bgBar = Mix(RGB(246, 246, 248), th.accent, 0.09);
    th.bgSide = Mix(RGB(238, 239, 241), th.accent, 0.13);
    th.bgCaption = Mix(RGB(243, 243, 245), th.accent, 0.11);
    th.line = Mix(RGB(220, 221, 225), th.accent, 0.18);
    for (HBRUSH* b : { &th.brMain, &th.brSide, &th.brBar, &th.brCaption }) if (*b) DeleteObject(*b);
    th.brMain = CreateSolidBrush(th.bgMain);
    th.brSide = CreateSolidBrush(th.bgSide);
    th.brBar = CreateSolidBrush(th.bgBar);
    th.brCaption = CreateSolidBrush(th.bgCaption);
    mui::Palette p;
    p.accent = Luma(th.accent) > 0.55 ? th.accentDark : th.accent;
    p.accentText = th.accentText;
    p.line = th.line;
    mui::SetPalette(p);
}

COLORREF CaptureColor() {   // botão de captura de imagem na cor de destaque (escurecida se for clara demais)
    return Luma(th.accent) > 0.55 ? th.accentDark : th.accent;
}

void ApplyFrameTheme() {
    // barra de título: se o usuário pediu a cor de destaque nas barras de título, o Windows já pinta;
    // senão usa o mesmo tom suave do menu (DWMWA_CAPTION_COLOR, Windows 11)
    DWORD prevalence = 0;
    ReadHkcuDword(L"Software\\Microsoft\\Windows\\DWM", L"ColorPrevalence", prevalence);
    COLORREF cap = prevalence ? 0xFFFFFFFF /*DWMWA_COLOR_DEFAULT*/ : th.bgCaption;
    DwmSetWindowAttribute(g.frame, 35 /*DWMWA_CAPTION_COLOR*/, &cap, sizeof cap);
    COLORREF border = prevalence ? 0xFFFFFFFF : th.line;
    DwmSetWindowAttribute(g.frame, 34 /*DWMWA_BORDER_COLOR*/, &border, sizeof border);
    MENUINFO mi{ sizeof mi };
    mi.fMask = MIM_BACKGROUND;
    mi.hbrBack = th.brCaption;
    SetMenuInfo(GetMenu(g.frame), &mi);
    DrawMenuBar(g.frame);
    RedrawWindow(g.frame, nullptr, nullptr, RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN | RDW_FRAME);
}

// ---------------------------------------------------------------- bandeja
void TrayAdd() {
    g.nid = {};
    g.nid.cbSize = sizeof(g.nid);
    g.nid.hWnd = g.frame;
    g.nid.uID = 1;
    g.nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    g.nid.uCallbackMessage = WM_SNAP_TRAY;
    g.nid.hIcon = g.iconSmall;
    wcscpy_s(g.nid.szTip, L"Snapnab — clique para capturar");
    Shell_NotifyIconW(NIM_ADD, &g.nid);
    g.nid.uVersion = NOTIFYICON_VERSION_4;
    Shell_NotifyIconW(NIM_SETVERSION, &g.nid);
}

void TrayNotify(const std::wstring& title, const std::wstring& text) {
    NOTIFYICONDATAW n = g.nid;
    n.uFlags = NIF_INFO;
    wcsncpy_s(n.szInfoTitle, title.c_str(), _TRUNCATE);
    wcsncpy_s(n.szInfo, text.c_str(), _TRUNCATE);
    n.dwInfoFlags = NIIF_INFO | NIIF_NOSOUND;
    Shell_NotifyIconW(NIM_MODIFY, &n);
}

// ---------------------------------------------------------------- atalhos
void RegisterHotkeys() {
    g.hk.Clear();
    UINT m, v;
    UnpackHotkey(RegGetDword(L"ImageHotkey", PackHotkey(0, VK_SNAPSHOT)), m, v);
    g.hk.Set(HK_IMAGE, m, v);
    UnpackHotkey(RegGetDword(L"VideoHotkey", PackHotkey(MOD_SHIFT, VK_SNAPSHOT)), m, v);
    g.hk.Set(HK_VIDEO, m, v);
    for (size_t i = 0; i < g.store.presets.size(); i++) {
        auto& p = g.store.presets[i];
        if (!p.hotkeyVk) continue;
        if (g.hk.Set(HK_PRESET_BASE + (int)i, p.hotkeyMods, p.hotkeyVk) == HotkeyManager::InternalConflict)
            TrayNotify(L"Conflito de atalho", L"O atalho do preset \"" + p.name + L"\" já está em uso por outra ação.");
    }
}

void SetVideoHotkeys(bool on) {
    if (on) {
        UINT m, v;
        UnpackHotkey(RegGetDword(L"VideoStartPauseResumeKey", PackHotkey(MOD_SHIFT, VK_F9)), m, v);
        g.hk.Set(HK_VIDEO_PAUSE, m, v, false);
        UnpackHotkey(RegGetDword(L"VideoStopKey", PackHotkey(MOD_SHIFT, VK_F10)), m, v);
        g.hk.Set(HK_VIDEO_STOP, m, v, false);
    } else {
        g.hk.Remove(HK_VIDEO_PAUSE);
        g.hk.Remove(HK_VIDEO_STOP);
    }
}

std::wstring HotkeyHint() {
    UINT m, v;
    UnpackHotkey(RegGetDword(L"ImageHotkey", PackHotkey(0, VK_SNAPSHOT)), m, v);
    std::wstring s = L"Imagem: " + HotkeyToText(m, v);
    UnpackHotkey(RegGetDword(L"VideoHotkey", PackHotkey(MOD_SHIFT, VK_SNAPSHOT)), m, v);
    s += L"\nVídeo: " + HotkeyToText(m, v);
    return s;
}

// ---------------------------------------------------------------- captura
void StartCapture(const Preset& p) {
    g.lastPreset = p; g.haveLast = true;
    g.engine.Activate(p);
}

Preset FirstOfKind(CaptureKind k) {
    if (Cur().kind == k) return Cur();
    for (auto& p : g.store.presets) if (p.kind == k) return p;
    Preset p; p.kind = k; if (k == CaptureKind::Video) p.captureCursor = true;
    return p;
}

// ---------------------------------------------------------------- UI: preset ↔ controles
// ---------------------------------------------------------------- efeitos (lista "Efeitos" da janela de captura)
enum Fx { FxBorder, FxFilter, FxShadow, FxCaption, FxReplace, FxScale, FxEdge, FxTrim, FxAdjust, FxRotate, FxWatermark, FxResolution, FxCount };
enum : LPARAM { FxNone = -1, FxSep = -2, FxAdd = -3, FxRemove = -4 };   // itens de ação da lista
const wchar_t* const kFxNames[FxCount] = {
    L"Borda", L"Filtros", L"Sombra", L"Informações da captura", L"Substituição de cor", L"Redimensionar imagem",
    L"Bordas especiais", L"Cortar bordas", L"Ajuste de cor", L"Girar imagem", L"Marca d'água", L"Resolução da imagem",
};

bool& FxOn(fx::CaptureEffects& e, int k) {
    bool* on[FxCount] = { &e.border, &e.filter, &e.shadow, &e.caption, &e.replaceColor, &e.scale,
                          &e.edge, &e.trim, &e.adjust, &e.rotate, &e.watermark, &e.resolution };
    return *on[k];
}

int FxCountOn(fx::CaptureEffects& e) { int n = 0; for (int k = 0; k < FxCount; k++) n += FxOn(e, k); return n; }

int FxFirstOn(fx::CaptureEffects& e) { for (int k = 0; k < FxCount; k++) if (FxOn(e, k)) return k; return -1; }

// ajustes de um efeito; OK liga o efeito
bool EditFx(int k) {
    auto& e = Cur().effects;
    using F = ui::Field;
    std::vector<F> f;
    switch (k) {
    case FxBorder: f = { F::MakeInt(L"Largura (px)", e.borderWidth, 1, 100), F::MakeColor(L"Cor", e.borderColor) }; break;
    case FxFilter: f = { F::MakeCombo(L"Filtro", { L"Tons de cinza", L"Sépia", L"Inverter cores", L"Preto e branco" }, e.filterKind) }; break;
    case FxShadow:
        f = { F::MakeInt(L"Deslocamento (px)", e.shadowOffset, 0, 50), F::MakeInt(L"Desfoque (px)", e.shadowBlur, 0, 50),
              F::MakeInt(L"Opacidade (%)", e.shadowOpacity, 0, 100) };
        break;
    case FxCaption:
        f = { F::MakeText(L"Texto", e.captionText), F::MakeLabel(L"Campos: {date} {time} {computer} {user}"),
              F::MakeCombo(L"Posição", { L"Embaixo", L"Em cima" }, e.captionTop ? 1 : 0) };
        break;
    case FxReplace:
        f = { F::MakeColor(L"Cor a substituir", e.replaceFrom), F::MakeColor(L"Nova cor", e.replaceTo),
              F::MakeInt(L"Tolerância", e.replaceTolerance, 0, 100) };
        break;
    case FxScale: f = { F::MakeInt(L"Escala (%)", e.scalePercent, 10, 400) }; break;
    case FxEdge:
        f = { F::MakeCombo(L"Estilo", { L"Rasgada", L"Onda", L"Dente de serra", L"Dente de tubarão", L"Esmaecer" }, e.edgeStyle),
              F::MakeInt(L"Tamanho (px)", e.edgeSize, 4, 60),
              F::MakeCheck(L"Em cima", (e.edgeSides & fx::Top) != 0), F::MakeCheck(L"Direita", (e.edgeSides & fx::Right) != 0),
              F::MakeCheck(L"Embaixo", (e.edgeSides & fx::Bottom) != 0), F::MakeCheck(L"Esquerda", (e.edgeSides & fx::Left) != 0) };
        break;
    case FxTrim:
        f = { F::MakeLabel(L"Remove as faixas de cor uniforme em volta da captura."), F::MakeInt(L"Tolerância", e.trimTolerance, 0, 100) };
        break;
    case FxAdjust:
        f = { F::MakeInt(L"Brilho", e.brightness, -100, 100), F::MakeInt(L"Contraste", e.contrast, -100, 100),
              F::MakeInt(L"Matiz (graus)", e.hue, -180, 180), F::MakeInt(L"Saturação", e.saturation, -100, 100) };
        break;
    case FxRotate:
        f = { F::MakeCombo(L"Girar", { L"90° à direita", L"90° à esquerda", L"180°", L"Espelhar na horizontal", L"Espelhar na vertical" },
                           e.rotateKind) };
        break;
    case FxWatermark:
        f = { F::MakeText(L"Texto", e.watermarkText), F::MakeInt(L"Opacidade (%)", e.watermarkOpacity, 5, 100),
              F::MakeCheck(L"Repetir por toda a imagem", e.watermarkTiled) };
        break;
    case FxResolution:
        f = { F::MakeInt(L"Resolução (DPI)", e.dpi, 36, 2400), F::MakeLabel(L"Gravada nos arquivos salvos (PNG, JPEG, TIFF, BMP).") };
        break;
    default: return false;
    }
    if (!ui::RunForm(g.frame, kFxNames[k], f)) return false;
    switch (k) {
    case FxBorder: e.borderWidth = f[0].value; e.borderColor = f[1].color; break;
    case FxFilter: e.filterKind = f[0].value; break;
    case FxShadow: e.shadowOffset = f[0].value; e.shadowBlur = f[1].value; e.shadowOpacity = f[2].value; break;
    case FxCaption: e.captionText = f[0].text; e.captionTop = f[2].value == 1; break;
    case FxReplace: e.replaceFrom = f[0].color; e.replaceTo = f[1].color; e.replaceTolerance = f[2].value; break;
    case FxScale: e.scalePercent = f[0].value; break;
    case FxEdge:
        e.edgeStyle = f[0].value; e.edgeSize = f[1].value;
        e.edgeSides = (f[2].value ? fx::Top : 0) | (f[3].value ? fx::Right : 0) | (f[4].value ? fx::Bottom : 0) | (f[5].value ? fx::Left : 0);
        if (!e.edgeSides) e.edgeSides = fx::Bottom | fx::Right;
        break;
    case FxTrim: e.trimTolerance = f[1].value; break;
    case FxAdjust: e.brightness = f[0].value; e.contrast = f[1].value; e.hue = f[2].value; e.saturation = f[3].value; break;
    case FxRotate: e.rotateKind = f[0].value; break;
    case FxWatermark: e.watermarkText = f[0].text; e.watermarkOpacity = f[1].value; e.watermarkTiled = f[2].value != 0; break;
    case FxResolution: e.dpi = f[0].value; break;
    }
    FxOn(e, k) = true;
    g.fxCurrent = k;
    g.store.Save();
    return true;
}

// texto da caixa: o efeito atual e quantos outros estão ligados
void UpdateEffectsText() {
    auto& e = Cur().effects;
    if (g.fxCurrent < 0 || !FxOn(e, g.fxCurrent)) g.fxCurrent = FxFirstOn(e);
    std::wstring t = L"Nenhum";
    if (g.fxCurrent >= 0) {
        t = kFxNames[g.fxCurrent];
        int others = FxCountOn(e) - 1;
        if (others > 0) t += Format(L"  +%d", others);
    }
    SendMessageW(C(IDC_EFFECTS), mui::MUI_SETDISPLAYTEXT, 0, (LPARAM)t.c_str());
}

void FillEffectsList() {
    HWND cb = C(IDC_EFFECTS);
    auto& e = Cur().effects;
    ComboBox_ResetContent(cb);
    auto add = [cb](const wchar_t* text, LPARAM data, UINT flags = 0) {
        int i = ComboBox_AddString(cb, text);
        ComboBox_SetItemData(cb, i, data);
        SendMessageW(cb, mui::MUI_SETITEMFLAGS, i, flags);
    };
    if (g.fxAddMode) {          // "Adicionar outro efeito": só os que ainda não estão ligados
        for (int k = 0; k < FxCount; k++) if (!FxOn(e, k)) add(kFxNames[k], k);
        return;
    }
    add(L"Nenhum", FxNone);
    for (int k = 0; k < FxCount; k++) add(kFxNames[k], k, FxOn(e, k) ? mui::ItemChecked : 0);
    add(L"", FxSep, mui::ItemSeparator);
    add(L"Adicionar outro efeito", FxAdd);
    add(L"Remover este efeito", FxRemove);
}

void OnEffectPicked(LPARAM item) {
    auto& e = Cur().effects;
    if (item == FxNone) {
        for (int k = 0; k < FxCount; k++) FxOn(e, k) = false;
        g.fxCurrent = -1;
        g.store.Save();
    } else if (item >= 0 && item < FxCount) {
        EditFx((int)item);
    } else if (item == FxAdd) {
        PostMessageW(g.frame, WM_FX_ADD, 0, 0);     // reabre a lista só com os efeitos desligados
    } else if (item == FxRemove && g.fxCurrent >= 0) {
        FxOn(e, g.fxCurrent) = false;
        g.fxCurrent = -1;
        g.store.Save();
    }
    UpdateEffectsText();
}

std::wstring FileSummary(const Preset& p) {
    static const wchar_t* fmt[] = { L"PNG", L"JPEG", L"BMP", L"GIF", L"TIFF" };
    std::wstring dir = p.folder.empty() ? DefaultSaveDir() : p.folder;
    return Format(L"%s em %s", fmt[std::clamp((int)p.fileFormat, 0, 4)], PathFindFileNameW(dir.c_str()));
}

void FillPresetList() {
    HWND lb = C(IDC_PRESETS);
    SendMessageW(lb, WM_SETREDRAW, FALSE, 0);
    ListBox_ResetContent(lb);
    for (auto& p : g.store.presets) ListBox_AddString(lb, p.name.c_str());     // desenhado em DrawPresetItem
    ListBox_SetCurSel(lb, g.store.selected);
    SendMessageW(lb, WM_SETREDRAW, TRUE, 0);
    InvalidateRect(lb, nullptr, TRUE);
}

void Layout();

void LoadControls() {
    g.loading = true;
    Preset& p = Cur();
    bool video = p.kind == CaptureKind::Video;
    HWND sel = C(IDC_SELECTION);
    ComboBox_ResetContent(sel);
    auto list = SelectionsFor(p.kind);
    int idx = 0;
    for (size_t i = 0; i < list.size(); i++) {
        ComboBox_AddString(sel, SelectionName(list[i]));
        if (list[i] == p.selection) idx = (int)i;
    }
    if (std::find(list.begin(), list.end(), p.selection) == list.end()) p.selection = list[0];
    ComboBox_SetCurSel(sel, idx);
    EnableWindow(C(IDC_SEL_CFG), p.selection == Selection::FixedRegion);
    g.fxCurrent = -1;
    UpdateEffectsText();
    SetWindowTextW(C(IDC_FILE_SUMMARY), FileSummary(p).c_str());
    Button_SetCheck(C(IDC_PREVIEW), p.previewInEditor);
    Button_SetCheck(C(IDC_CLIPBOARD), p.copyToClipboard);
    Button_SetCheck(C(IDC_SAVEFILE), p.saveToFile);
    Button_SetCheck(C(IDC_CURSOR), p.captureCursor);
    {
        HWND d = C(IDC_DELAY);
        ComboBox_ResetContent(d);
        std::vector<int> secs = { 0, 1, 2, 3, 5, 10, 15, 30, 60 };
        if (std::find(secs.begin(), secs.end(), p.delaySeconds) == secs.end()) {
            secs.push_back(p.delaySeconds);
            std::sort(secs.begin(), secs.end());
        }
        for (int s : secs) {
            int i = ComboBox_AddString(d, s == 0 ? L"Desligado" : s == 1 ? L"1 segundo" : Format(L"%d segundos", s).c_str());
            ComboBox_SetItemData(d, i, s);
            if (s == p.delaySeconds) ComboBox_SetCurSel(d, i);
        }
    }
    Button_SetCheck(C(IDC_MIC), p.microphone);
    Button_SetCheck(C(IDC_SYS), p.systemAudio);
    static const int fpsList[] = { 10, 15, 24, 30, 60 };
    int fi = 3;
    for (int i = 0; i < 5; i++) if (fpsList[i] == p.fps) fi = i;
    ComboBox_SetCurSel(C(IDC_FPS), fi);
    ComboBox_SetCurSel(C(IDC_QUALITY), p.quality);
    Button_SetCheck(C(IDC_DOWNSCALE), p.downscale1080);
    Button_SetCheck(C(IDC_HW), p.hardwareEncode);
    // visibilidade por modo
    for (int id : { IDC_EFFECTS, IDC_EFFECTS_TITLE, IDC_CLIPBOARD, IDC_SAVEFILE, IDC_SAVEFILE_CFG, IDC_SHARE_LABEL,
                    IDC_FILE_SUMMARY, IDC_DELAY, IDC_DELAY_LABEL })
        ShowWindow(C(id), video ? SW_HIDE : SW_SHOW);
    for (int id : { IDC_MIC, IDC_SYS, IDC_FPS, IDC_FPS_LABEL, IDC_QUALITY, IDC_QUALITY_LABEL, IDC_DOWNSCALE, IDC_HW })
        ShowWindow(C(id), video ? SW_SHOW : SW_HIDE);
    InvalidateRect(C(IDC_TAB_IMAGE), nullptr, TRUE);
    InvalidateRect(C(IDC_TAB_VIDEO), nullptr, TRUE);
    InvalidateRect(C(IDC_CAPTURE), nullptr, TRUE);
    g.loading = false;
}

void SaveControls() {
    if (g.loading) return;
    Preset& p = Cur();
    auto list = SelectionsFor(p.kind);
    int si = ComboBox_GetCurSel(C(IDC_SELECTION));
    if (si >= 0 && si < (int)list.size()) p.selection = list[si];
    EnableWindow(C(IDC_SEL_CFG), p.selection == Selection::FixedRegion);
    p.previewInEditor = Button_GetCheck(C(IDC_PREVIEW)) == BST_CHECKED;
    p.copyToClipboard = Button_GetCheck(C(IDC_CLIPBOARD)) == BST_CHECKED;
    p.saveToFile = Button_GetCheck(C(IDC_SAVEFILE)) == BST_CHECKED;
    p.captureCursor = Button_GetCheck(C(IDC_CURSOR)) == BST_CHECKED;
    int di = ComboBox_GetCurSel(C(IDC_DELAY));
    if (di >= 0) p.delaySeconds = std::clamp((int)ComboBox_GetItemData(C(IDC_DELAY), di), 0, 60);
    p.microphone = Button_GetCheck(C(IDC_MIC)) == BST_CHECKED;
    p.systemAudio = Button_GetCheck(C(IDC_SYS)) == BST_CHECKED;
    static const int fpsList[] = { 10, 15, 24, 30, 60 };
    int fi = ComboBox_GetCurSel(C(IDC_FPS));
    if (fi >= 0) p.fps = fpsList[fi];
    int qi = ComboBox_GetCurSel(C(IDC_QUALITY));
    if (qi >= 0) p.quality = qi;
    p.downscale1080 = Button_GetCheck(C(IDC_DOWNSCALE)) == BST_CHECKED;
    p.hardwareEncode = Button_GetCheck(C(IDC_HW)) == BST_CHECKED;
    g.store.Save();
}

// ---------------------------------------------------------------- diálogos
void EditFileOutput() {
    auto& p = Cur();
    std::vector<ui::Field> f = {
        ui::Field::MakeCombo(L"Formato", { L"PNG", L"JPEG", L"BMP", L"GIF", L"TIFF" }, (int)p.fileFormat),
        ui::Field::MakeText(L"Prefixo do nome", p.filePrefix),
        ui::Field::MakeText(L"Pasta", p.folder.empty() ? DefaultSaveDir() : p.folder),
        ui::Field::MakeLabel(L"Nome automático: <Prefixo><Número>, ex.: Captura001.png"),
        ui::Field::MakeCheck(L"Abrir também em um programa externo", p.openInProgram),
        ui::Field::MakeText(L"Programa", p.programPath),
    };
    auto validate = [](std::vector<ui::Field>& f) -> std::wstring {
        const std::wstring& pre = f[1].text;
        if (pre.empty() || pre.find_first_of(L"\\/:*?\"<>|") != std::wstring::npos) return L"O prefixo contém caracteres inválidos.";
        return L"";
    };
    if (!ui::RunForm(g.frame, L"Salvar em arquivo", f, validate)) return;
    p.fileFormat = (FileFormat)f[0].value; p.filePrefix = f[1].text; p.folder = f[2].text;
    p.openInProgram = f[4].value; p.programPath = f[5].text;
    g.store.Save();
    SetWindowTextW(C(IDC_FILE_SUMMARY), FileSummary(p).c_str());
}

void EditFixedSize() {
    auto& p = Cur();
    std::vector<ui::Field> f = { ui::Field::MakeInt(L"Largura", p.fixedW, 16, 10000), ui::Field::MakeInt(L"Altura", p.fixedH, 16, 10000) };
    if (!ui::RunForm(g.frame, L"Região com tamanho fixo", f)) return;
    p.fixedW = f[0].value; p.fixedH = f[1].value;
    g.store.Save();
}

void EditPresetHotkey() {
    auto& p = Cur();
    std::vector<ui::Field> f = { ui::Field::MakeHotkey(L"Atalho do preset", p.hotkeyMods, p.hotkeyVk),
                                 ui::Field::MakeLabel(L"Deixe vazio (Backspace) para remover.") };
    if (!ui::RunForm(g.frame, L"Atalho — " + p.name, f)) return;
    UINT oldM = p.hotkeyMods, oldV = p.hotkeyVk;
    p.hotkeyMods = f[0].hkMods; p.hotkeyVk = f[0].hkVk;
    int id = HK_PRESET_BASE + g.store.selected;
    auto r = g.hk.Set(id, p.hotkeyMods, p.hotkeyVk);
    if (r == HotkeyManager::InternalConflict) {
        MessageBoxW(g.frame, L"Esse atalho já está em uso por outra ação do Snapnab.", L"Conflito de atalho", MB_ICONWARNING);
        p.hotkeyMods = oldM; p.hotkeyVk = oldV;
        return;
    }
    if (r == HotkeyManager::HookedFallback)
        MessageBoxW(g.frame, L"Outro programa já usa esse atalho. O Snapnab vai interceptá-lo por um gancho de teclado.",
                    L"Atalho", MB_ICONINFORMATION);
    g.store.Save();
    FillPresetList();
}

void RenamePreset() {
    std::vector<ui::Field> f = { ui::Field::MakeText(L"Nome do preset", Cur().name) };
    if (!ui::RunForm(g.frame, L"Renomear preset", f) || Trim(f[0].text).empty()) return;
    Cur().name = Trim(f[0].text);
    g.store.Save();
    FillPresetList();
}

bool StartupEnabled() {
    wchar_t b[MAX_PATH * 2]; DWORD cb = sizeof b;
    return RegGetValueW(HKEY_CURRENT_USER, L"Software\\Microsoft\\Windows\\CurrentVersion\\Run", L"Snapnab", RRF_RT_REG_SZ, nullptr, b, &cb) == ERROR_SUCCESS;
}

void SetStartup(bool on) {
    HKEY k;
    if (RegOpenKeyExW(HKEY_CURRENT_USER, L"Software\\Microsoft\\Windows\\CurrentVersion\\Run", 0, KEY_SET_VALUE, &k) != ERROR_SUCCESS) return;
    if (on) {
        std::wstring v = L"\"" + ExeDir() + L"\\" + kCaptureExe + L"\" /hide";
        RegSetValueExW(k, L"Snapnab", 0, REG_SZ, (const BYTE*)v.c_str(), (DWORD)((v.size() + 1) * 2));
    } else RegDeleteValueW(k, L"Snapnab");
    RegCloseKey(k);
}

void ShowOptions() {
    UINT im, iv, vm, vv, pm, pv, sm, sv;
    UnpackHotkey(RegGetDword(L"ImageHotkey", PackHotkey(0, VK_SNAPSHOT)), im, iv);
    UnpackHotkey(RegGetDword(L"VideoHotkey", PackHotkey(MOD_SHIFT, VK_SNAPSHOT)), vm, vv);
    UnpackHotkey(RegGetDword(L"VideoStartPauseResumeKey", PackHotkey(MOD_SHIFT, VK_F9)), pm, pv);
    UnpackHotkey(RegGetDword(L"VideoStopKey", PackHotkey(MOD_SHIFT, VK_F10)), sm, sv);
    std::vector<ui::Field> f = {
        ui::Field::MakeSeparator(L"Atalhos globais"),
        ui::Field::MakeHotkey(L"Capturar imagem", im, iv),
        ui::Field::MakeHotkey(L"Gravar vídeo", vm, vv),
        ui::Field::MakeHotkey(L"Pausar/retomar vídeo", pm, pv),
        ui::Field::MakeHotkey(L"Parar vídeo", sm, sv),
        ui::Field::MakeSeparator(L"Geral"),
        ui::Field::MakeText(L"Pasta de saída", DefaultSaveDir()),
        ui::Field::MakeCombo(L"Monitorar área de transferência", { L"Desligado", L"Só a Ferramenta de Captura do Windows", L"Qualquer aplicativo" },
                             (int)RegGetDword(L"ClipboardMonitor", 1)),
        ui::Field::MakeCheck(L"Iniciar com o Windows", StartupEnabled()),
        ui::Field::MakeCheck(L"Mostrar a janela de captura ao iniciar", RegGetDword(L"ShowCaptureWindow", 1) != 0),
        ui::Field::MakeLabel(L"Biblioteca: " + LibraryDir()),
    };
    auto validate = [](std::vector<ui::Field>& f) -> std::wstring {
        std::vector<std::pair<UINT, UINT>> keys;
        for (int i = 1; i <= 4; i++) {
            if (!f[i].hkVk) continue;
            for (auto& k : keys) if (k == std::make_pair(f[i].hkMods, f[i].hkVk)) return L"Dois atalhos iguais: " + HotkeyToText(f[i].hkMods, f[i].hkVk);
            keys.push_back({ f[i].hkMods, f[i].hkVk });
        }
        return L"";
    };
    if (!ui::RunForm(g.frame, L"Opções do Snapnab", f, validate, 480)) return;
    RegSetDword(L"ImageHotkey", PackHotkey(f[1].hkMods, f[1].hkVk));
    RegSetDword(L"VideoHotkey", PackHotkey(f[2].hkMods, f[2].hkVk));
    RegSetDword(L"VideoStartPauseResumeKey", PackHotkey(f[3].hkMods, f[3].hkVk));
    RegSetDword(L"VideoStopKey", PackHotkey(f[4].hkMods, f[4].hkVk));
    if (!Trim(f[6].text).empty()) RegSetString(L"OutputFolder", Trim(f[6].text));
    RegSetDword(L"ClipboardMonitor", f[7].value);
    SetStartup(f[8].value != 0);
    RegSetDword(L"ShowCaptureWindow", f[9].value);
    RegisterHotkeys();
    SetWindowTextW(C(IDC_HINT), HotkeyHint().c_str());
    g.ipc.SendInt(ipc::kEditor, ipc::ReloadConfig, 0);
}

void OpenEditor() {
    HWND ed = FindWindowW(kEditorWndClass, nullptr);
    if (ed) {
        AllowSetForegroundWindow(ASFW_ANY);
        g.ipc.SendInt(ipc::kEditor, ipc::ShowEditor, 1);
        if (IsIconic(ed)) ShowWindow(ed, SW_RESTORE);
        SetForegroundWindow(ed);
    } else LaunchSibling(kEditorExe, L"");
}

void ShowFrame() {
    ShowWindow(g.frame, SW_SHOW);
    if (IsIconic(g.frame)) ShowWindow(g.frame, SW_RESTORE);
    SetForegroundWindow(g.frame);
}

void About() {
    std::wstring text = L"Snapnab " + std::wstring(kAppVersion) + L" — captura de tela e gravação de vídeo\n" SNAPNAB_COPYRIGHT_W L"\n"
        L"Software livre sob a GNU General Public License versão 3.\n\n" + std::wstring(
        L"Processo de captura (Snapnab.exe) + Editor (SnapnabEditor.exe), comunicando-se por memória\n"
        L"compartilhada (canal IPC) e WM_COPYDATA. Capturas ficam na biblioteca:\n") + LibraryDir();
    MessageBoxW(g.frame, text.c_str(), L"Sobre o Snapnab", MB_ICONINFORMATION);
}

// ---------------------------------------------------------------- monitor da área de transferência (A13)
void OnClipboardUpdate() {
    DWORD mode = RegGetDword(L"ClipboardMonitor", 1);
    DWORD seq = GetClipboardSequenceNumber();
    if (!mode || seq == g.lastClipSeq || g.engine.busy()) return;
    g.lastClipSeq = seq;
    if (!ClipboardHasImage()) return;
    HWND owner = GetClipboardOwner();
    DWORD pid = 0;
    if (owner) GetWindowThreadProcessId(owner, &pid);
    if (pid == GetCurrentProcessId()) return;
    std::wstring proc = owner ? ToLower(WindowProcessName(owner)) : L"";
    if (proc == ToLower(kEditorExe)) return;
    bool snip = proc == L"snippingtool.exe" || proc == L"screenclippinghost.exe" || proc == L"screensketch.exe";
    if (mode == 1 && !snip) return;
    Log(L"Clipboard: imagem detectada de '%s'", proc.c_str());
    // o Editor cria a imagem nova a partir do clipboard (IPC 0x1E, ed_comp_14 §3.4)
    if (IsProcessRunning(kEditorMutex)) g.ipc.SendInt(ipc::kEditor, ipc::NewImageFromClipboard, 0);
    else LaunchSibling(kEditorExe, L"/NewFromClipboard");
}

// ---------------------------------------------------------------- IPC recebido do Editor
void OnIpc(const ipc::Message& m) {
    switch (m.code) {
    case ipc::EditorStartCapture: StartCapture(FirstOfKind(m.ivalue == 1 ? CaptureKind::Video : CaptureKind::Image)); break;
    case ipc::EditorClosed: Log(L"Editor fechado"); break;
    case ipc::ReloadConfig: g.store.Load(); FillPresetList(); LoadControls(); break;
    default: break;
    }
}

// ---------------------------------------------------------------- linha de comando (G3)
void ProcessCommandLine(const std::wstring& cmdLine, bool startup) {
    auto args = SplitCommandLine(cmdLine);
    bool hide = false, handled = false;
    for (size_t i = 0; i < args.size(); i++) {
        std::wstring a = ToLower(args[i]);
        if (a == L"/hide" || a == L"/tray") hide = true;
        else if (a == L"/capture") { StartCapture(FirstOfKind(CaptureKind::Image)); handled = true; }
        else if (a == L"/video") { StartCapture(FirstOfKind(CaptureKind::Video)); handled = true; }
        else if (a == L"/preset" && i + 1 < args.size()) {
            std::wstring name = args[++i];
            for (auto& p : g.store.presets) if (_wcsicmp(p.name.c_str(), name.c_str()) == 0) { StartCapture(p); handled = true; break; }
        } else if (a == L"/clipboard") { Preset p = Cur(); p.kind = CaptureKind::Image; p.selection = Selection::Clipboard; StartCapture(p); handled = true; }
        else if (a == L"/exit") { PostMessageW(g.frame, WM_COMMAND, IDM_EXIT, 0); handled = true; }
        else if (a == L"/help" || a == L"/?") {
            MessageBoxW(nullptr, L"Snapnab.exe [opções]\n\n"
                L"/hide            inicia só na bandeja\n"
                L"/capture         captura imagem com o preset de imagem atual\n"
                L"/video           grava vídeo com o preset de vídeo atual\n"
                L"/preset \"nome\"   executa um preset pelo nome\n"
                L"/clipboard       captura a imagem da área de transferência\n"
                L"/exit            encerra o Snapnab em execução", L"Snapnab — linha de comando", MB_ICONINFORMATION);
            handled = true;
        }
    }
    if (!handled && (!startup || (!hide && RegGetDword(L"ShowCaptureWindow", 1)))) ShowFrame();
}

// ---------------------------------------------------------------- janela
HWND MakeCtl(int id, const wchar_t* cls, const wchar_t* text, DWORD style, DWORD ex = 0) {
    HWND h = CreateWindowExW(ex, cls, text, WS_CHILD | WS_VISIBLE | style, 0, 0, 10, 10, g.frame, (HMENU)(INT_PTR)id, g.hi, nullptr);
    SendMessageW(h, WM_SETFONT, (WPARAM)ui::UiFont(g.dpi), TRUE);
    g.ctl[id] = h;
    return h;
}

void CreateControls() {
    using namespace mui;
    auto style = [](int id, UINT st, int icon = -1) {
        SendMessageW(C(id), MUI_SETSTYLE, st, 0);
        if (icon >= 0) SendMessageW(C(id), MUI_SETICON, icon + 1, 0);
    };
    // criados na ordem do Tab
    MakeCtl(IDC_TAB_IMAGE, L"BUTTON", L"Imagem", BS_OWNERDRAW | WS_TABSTOP);
    MakeCtl(IDC_TAB_VIDEO, L"BUTTON", L"Vídeo", BS_OWNERDRAW | WS_TABSTOP);
    MakeCtl(IDC_SEL_LABEL, L"STATIC", L"Seleção:", SS_LEFT | SS_CENTERIMAGE);
    MakeCtl(IDC_SELECTION, kCombo, nullptr, WS_TABSTOP);
    MakeCtl(IDC_SEL_CFG, kButton, L"Tamanho da região…", WS_TABSTOP);
    style(IDC_SEL_CFG, ButtonIconOnly, icons::Gear);
    MakeCtl(IDC_EFFECTS_TITLE, L"STATIC", L"Efeitos:", SS_LEFT | SS_CENTERIMAGE);
    MakeCtl(IDC_EFFECTS, kCombo, nullptr, WS_TABSTOP);
    style(IDC_EFFECTS, ComboMenu);
    MakeCtl(IDC_SHARE_LABEL, L"STATIC", L"Arquivo:", SS_LEFT | SS_CENTERIMAGE);
    MakeCtl(IDC_FILE_SUMMARY, L"STATIC", L"", SS_LEFT | SS_ENDELLIPSIS | SS_CENTERIMAGE);
    MakeCtl(IDC_SAVEFILE_CFG, kButton, L"Configurar o arquivo…", WS_TABSTOP);
    style(IDC_SAVEFILE_CFG, ButtonIconOnly, icons::Gear);
    MakeCtl(IDC_DELAY_LABEL, L"STATIC", L"Atraso:", SS_LEFT | SS_CENTERIMAGE);
    MakeCtl(IDC_DELAY, kCombo, nullptr, WS_TABSTOP);
    MakeCtl(IDC_FPS_LABEL, L"STATIC", L"Quadros/s:", SS_LEFT | SS_CENTERIMAGE);
    HWND fps = MakeCtl(IDC_FPS, kCombo, nullptr, WS_TABSTOP);
    for (auto s : { L"10", L"15", L"24", L"30", L"60" }) ComboBox_AddString(fps, s);
    MakeCtl(IDC_QUALITY_LABEL, L"STATIC", L"Qualidade:", SS_LEFT | SS_CENTERIMAGE);
    HWND q = MakeCtl(IDC_QUALITY, kCombo, nullptr, WS_TABSTOP);
    for (auto s : { L"Muito baixa", L"Baixa", L"Média", L"Alta" }) ComboBox_AddString(q, s);
    MakeCtl(IDC_DOWNSCALE, kToggle, L"Reduzir para 1080p", WS_TABSTOP);
    MakeCtl(IDC_HW, kToggle, L"Codificação por hardware", WS_TABSTOP);
    MakeCtl(IDC_PREVIEW, kToggle, L"Visualizar no Editor", WS_TABSTOP);
    MakeCtl(IDC_CLIPBOARD, kToggle, L"Copiar para a área de transferência", WS_TABSTOP);
    MakeCtl(IDC_SAVEFILE, kToggle, L"Salvar em arquivo (nome automático)", WS_TABSTOP);
    MakeCtl(IDC_CURSOR, kToggle, L"Capturar o cursor", WS_TABSTOP);
    MakeCtl(IDC_MIC, kToggle, L"Gravar microfone", WS_TABSTOP);
    MakeCtl(IDC_SYS, kToggle, L"Gravar áudio do sistema", WS_TABSTOP);
    MakeCtl(IDC_CAPTURE, L"BUTTON", L"Capturar", BS_OWNERDRAW | WS_TABSTOP);
    MakeCtl(IDC_HINT, L"STATIC", HotkeyHint().c_str(), SS_CENTER);
    // barra inferior
    MakeCtl(IDC_PRESETS_LABEL, kButton, L"Presets", WS_TABSTOP);
    style(IDC_PRESETS_LABEL, ButtonFlat);
    MakeCtl(IDC_PRESET_NEW, kButton, L"+  Novo", WS_TABSTOP);
    style(IDC_PRESET_NEW, ButtonFlat);
    MakeCtl(IDC_OPTIONS, kButton, L"Opções", WS_TABSTOP);
    style(IDC_OPTIONS, ButtonFlat, icons::Gear);
    MakeCtl(IDC_OPEN_EDITOR, kButton, L"Abrir Editor", WS_TABSTOP);
    style(IDC_OPEN_EDITOR, ButtonFlat, icons::Editor);
    // lista de presets (abre abaixo da barra)
    MakeCtl(IDC_PRESETS, L"LISTBOX", nullptr, WS_TABSTOP | LBS_NOTIFY | WS_VSCROLL | LBS_NOINTEGRALHEIGHT | LBS_OWNERDRAWFIXED | LBS_HASSTRINGS);
    MakeCtl(IDC_PRESET_DUP, kButton, L"Duplicar", WS_TABSTOP);
    MakeCtl(IDC_PRESET_RENAME, kButton, L"Renomear", WS_TABSTOP);
    MakeCtl(IDC_PRESET_HOTKEY, kButton, L"Atalho…", WS_TABSTOP);
    MakeCtl(IDC_PRESET_DEL, kButton, L"Excluir", WS_TABSTOP);
}

int TextW(int id) {   // largura do texto do controle (a linha mais longa, se tiver várias)
    wchar_t b[256]; GetWindowTextW(C(id), b, 256);
    HDC dc = GetDC(g.frame);
    HGDIOBJ o = SelectObject(dc, ui::UiFont(g.dpi));
    RECT r{};
    DrawTextW(dc, b, -1, &r, DT_CALCRECT | DT_NOPREFIX);
    SelectObject(dc, o); ReleaseDC(g.frame, dc);
    return r.right;
}

int MaxTextW(std::initializer_list<int> ids) { int w = 0; for (int id : ids) w = std::max(w, TextW(id)); return w; }

// Disposição (como a janela de captura de referência):
//   abas verticais | rótulos + campos (seleção, efeitos, arquivo, atraso) | opções | botão de captura
//   barra inferior: Presets, Novo … Opções, Abrir Editor; a lista de presets abre abaixo dela
void ComputeMetrics() {
    lay.pad = S(12); lay.rowP = S(30); lay.ch = S(26);
    lay.tabW = S(78); lay.tabH = S(62);
    lay.ax = lay.tabW + S(16);
    lay.labW = MaxTextW({ IDC_SEL_LABEL, IDC_EFFECTS_TITLE, IDC_SHARE_LABEL, IDC_DELAY_LABEL, IDC_FPS_LABEL, IDC_QUALITY_LABEL }) + S(10);
    lay.fieldX = lay.ax + lay.labW;
    lay.fieldW = S(170);
    lay.cfgW = S(28);                                                   // botão de engrenagem
    lay.cfgX = lay.fieldX + lay.fieldW + S(4);
    lay.bx = lay.cfgX + lay.cfgW + S(26);
    lay.bw = MaxTextW({ IDC_PREVIEW, IDC_CLIPBOARD, IDC_SAVEFILE, IDC_CURSOR, IDC_MIC, IDC_SYS }) + S(16) + S(46);   // texto + chave
    lay.capW = std::max(S(112), TextW(IDC_HINT) + S(8));
    lay.capX = lay.bx + lay.bw + S(12);
    lay.clientW = lay.capX + lay.capW + lay.pad;
    lay.contentH = lay.pad * 2 + 4 * lay.rowP + lay.ch;                // 5 linhas (o modo vídeo usa todas)
    lay.barH = S(40);
    lay.panelH = lay.pad * 2 + 4 * S(31) - S(5);
}

SIZE ClientSizeWanted() { return { lay.clientW, lay.contentH + lay.barH + (g.presetsOpen ? lay.panelH : 0) }; }

void ResizeToContent() {
    SIZE c = ClientSizeWanted();
    RECT wr{ 0, 0, c.cx, c.cy };
    AdjustWindowRectExForDpi(&wr, GetWindowLongW(g.frame, GWL_STYLE), TRUE, GetWindowLongW(g.frame, GWL_EXSTYLE), g.dpi);
    SetWindowPos(g.frame, nullptr, 0, 0, wr.right - wr.left, wr.bottom - wr.top, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
    // a barra de menu pode quebrar em duas linhas e roubar altura: corrige pelo cliente real
    RECT cr; GetClientRect(g.frame, &cr);
    if (cr.bottom != c.cy || cr.right != c.cx) {
        RECT w; GetWindowRect(g.frame, &w);
        SetWindowPos(g.frame, nullptr, 0, 0, w.right - w.left + c.cx - cr.right, w.bottom - w.top + c.cy - cr.bottom,
                     SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

void Layout() {
    auto mv = [](int id, int x, int y, int w, int h) { MoveWindow(C(id), x, y, w, h, TRUE); };
    bool video = Cur().kind == CaptureKind::Video;
    SetWindowTextW(C(IDC_PRESETS_LABEL), g.presetsOpen ? L"Presets  ▴" : L"Presets  ▾");
    int ch = lay.ch, lw = lay.labW, fx = lay.fieldX;
    int colAEnd = lay.cfgX + lay.cfgW;
    auto row = [](int i) { return lay.pad + i * lay.rowP; };
    // abas verticais
    mv(IDC_TAB_IMAGE, 0, 0, lay.tabW, lay.tabH);
    mv(IDC_TAB_VIDEO, 0, lay.tabH, lay.tabW, lay.tabH);
    // coluna de campos
    mv(IDC_SEL_LABEL, lay.ax, row(0), lw, ch);
    mv(IDC_SELECTION, fx, row(0), lay.fieldW, ch);
    SendMessageW(C(IDC_SELECTION), CB_SETDROPPEDWIDTH, S(240), 0);
    mv(IDC_SEL_CFG, lay.cfgX, row(0), lay.cfgW, ch);
    // imagem
    mv(IDC_EFFECTS_TITLE, lay.ax, row(1), lw, ch);
    mv(IDC_EFFECTS, fx, row(1), lay.fieldW, ch);
    mv(IDC_SHARE_LABEL, lay.ax, row(2), lw, ch);
    mv(IDC_FILE_SUMMARY, fx + S(10), row(2), lay.fieldW - S(10), ch);
    mv(IDC_SAVEFILE_CFG, lay.cfgX, row(2), lay.cfgW, ch);
    mv(IDC_DELAY_LABEL, lay.ax, row(3), lw, ch);
    mv(IDC_DELAY, fx, row(3), S(130), ch);
    // vídeo
    mv(IDC_FPS_LABEL, lay.ax, row(1), lw, ch);
    mv(IDC_FPS, fx, row(1), S(90), ch);
    mv(IDC_QUALITY_LABEL, lay.ax, row(2), lw, ch);
    mv(IDC_QUALITY, fx, row(2), lay.fieldW, ch);
    mv(IDC_DOWNSCALE, lay.ax, row(3), colAEnd - lay.ax, ch);
    mv(IDC_HW, lay.ax, row(4), colAEnd - lay.ax, ch);
    // coluna de opções
    mv(IDC_PREVIEW, lay.bx, row(0), lay.bw, ch);
    if (video) {
        mv(IDC_CURSOR, lay.bx, row(1), lay.bw, ch);
        mv(IDC_MIC, lay.bx, row(2), lay.bw, ch);
        mv(IDC_SYS, lay.bx, row(3), lay.bw, ch);
    } else {
        mv(IDC_CLIPBOARD, lay.bx, row(1), lay.bw, ch);
        mv(IDC_SAVEFILE, lay.bx, row(2), lay.bw, ch);
        mv(IDC_CURSOR, lay.bx, row(3), lay.bw, ch);
    }
    // botão de captura + atalhos
    int capH = S(104);
    mv(IDC_CAPTURE, lay.capX, lay.pad - S(2), lay.capW, capH);
    mv(IDC_HINT, lay.capX, lay.pad - S(2) + capH + S(2), lay.capW, S(34));
    // barra inferior
    int by = lay.contentH + (lay.barH - ch) / 2;
    int pw = TextW(IDC_PRESETS_LABEL) + S(24);
    mv(IDC_PRESETS_LABEL, lay.pad - S(4), by, pw, ch);
    mv(IDC_PRESET_NEW, lay.pad - S(4) + pw + S(4), by, TextW(IDC_PRESET_NEW) + S(24), ch);
    int ew = TextW(IDC_OPEN_EDITOR) + S(46), ow = TextW(IDC_OPTIONS) + S(46);    // texto + ícone
    mv(IDC_OPEN_EDITOR, lay.clientW - lay.pad + S(4) - ew, by, ew, ch);
    mv(IDC_OPTIONS, lay.clientW - lay.pad + S(4) - ew - S(4) - ow, by, ow, ch);
    // lista de presets (expandida)
    int py = lay.contentH + lay.barH + lay.pad;
    int bw = MaxTextW({ IDC_PRESET_DUP, IDC_PRESET_DEL, IDC_PRESET_RENAME, IDC_PRESET_HOTKEY }) + S(36);
    int bx = lay.clientW - lay.pad - bw;
    mv(IDC_PRESETS, lay.pad, py, bx - S(10) - lay.pad, lay.panelH - lay.pad * 2);
    SendMessageW(C(IDC_PRESETS), LB_SETITEMHEIGHT, 0, S(30));
    int i = 0;
    for (int id : { IDC_PRESET_DUP, IDC_PRESET_RENAME, IDC_PRESET_HOTKEY, IDC_PRESET_DEL }) mv(id, bx, py + S(31) * i++, bw, ch);
    for (int id : { IDC_PRESETS, IDC_PRESET_DUP, IDC_PRESET_RENAME, IDC_PRESET_HOTKEY, IDC_PRESET_DEL })
        ShowWindow(C(id), g.presetsOpen ? SW_SHOW : SW_HIDE);
}

void TogglePresets() {
    g.presetsOpen = !g.presetsOpen;
    RegSetDword(L"CapturePresetsOpen", g.presetsOpen);
    bool l = g.loading; g.loading = true;
    Layout();
    ResizeToContent();
    g.loading = l;
    if (g.presetsOpen) {
        ListBox_SetCurSel(C(IDC_PRESETS), g.store.selected);     // rola até o preset atual
        SetFocus(C(IDC_PRESETS));
    }
}

HMENU BuildMenu() {
    HMENU bar = CreateMenu();
    HMENU file = CreatePopupMenu();
    AppendMenuW(file, MF_STRING, IDM_OPEN_EDITOR, L"Abrir o &Editor");
    AppendMenuW(file, MF_STRING, IDM_LIBRARY_FOLDER, L"Abrir a pasta da &biblioteca");
    AppendMenuW(file, MF_STRING, IDM_LOGS, L"Abrir a pasta de &logs");
    AppendMenuW(file, MF_SEPARATOR, 0, nullptr);
    AppendMenuW(file, MF_STRING, IDM_OPTIONS, L"&Opções…");
    AppendMenuW(file, MF_SEPARATOR, 0, nullptr);
    AppendMenuW(file, MF_STRING, IDM_EXIT, L"&Sair do Snapnab");
    HMENU cap = CreatePopupMenu();
    AppendMenuW(cap, MF_STRING, IDM_CAPTURE_IMAGE, L"Capturar &imagem");
    AppendMenuW(cap, MF_STRING, IDM_CAPTURE_VIDEO, L"Gravar &vídeo");
    AppendMenuW(cap, MF_STRING, IDM_REPEAT, L"&Repetir a última captura");
    AppendMenuW(cap, MF_SEPARATOR, 0, nullptr);
    AppendMenuW(cap, MF_STRING, IDM_CAPTURE_CLIPBOARD, L"Da área de &transferência");
    AppendMenuW(cap, MF_STRING, IDM_CAPTURE_FILE, L"De um &arquivo gráfico…");
    HMENU pre = CreatePopupMenu();
    AppendMenuW(pre, MF_STRING, IDM_PRESET_IMPORT, L"&Importar presets…");
    AppendMenuW(pre, MF_STRING, IDM_PRESET_EXPORT, L"&Exportar presets…");
    HMENU help = CreatePopupMenu();
    AppendMenuW(help, MF_STRING, IDM_ABOUT, L"&Sobre o Snapnab");
    AppendMenuW(bar, MF_POPUP, (UINT_PTR)file, L"&Arquivo");
    AppendMenuW(bar, MF_POPUP, (UINT_PTR)cap, L"&Captura");
    AppendMenuW(bar, MF_POPUP, (UINT_PTR)pre, L"&Presets");
    AppendMenuW(bar, MF_POPUP, (UINT_PTR)help, L"A&juda");
    return bar;
}

void TrayMenu() {
    HMENU m = CreatePopupMenu();
    AppendMenuW(m, MF_STRING, IDM_CAPTURE_IMAGE, L"Capturar imagem");
    AppendMenuW(m, MF_STRING, IDM_CAPTURE_VIDEO, L"Gravar vídeo");
    HMENU pre = CreatePopupMenu();
    for (size_t i = 0; i < g.store.presets.size(); i++)
        AppendMenuW(pre, MF_STRING, IDM_PRESET_BASE + i, g.store.presets[i].name.c_str());
    AppendMenuW(m, MF_POPUP, (UINT_PTR)pre, L"Presets");
    AppendMenuW(m, MF_SEPARATOR, 0, nullptr);
    AppendMenuW(m, MF_STRING, IDM_SHOW, L"Abrir a janela de captura");
    AppendMenuW(m, MF_STRING, IDM_OPEN_EDITOR, L"Abrir o Editor");
    AppendMenuW(m, MF_STRING, IDM_OPTIONS, L"Opções…");
    AppendMenuW(m, MF_SEPARATOR, 0, nullptr);
    AppendMenuW(m, MF_STRING, IDM_EXIT, L"Sair");
    SetMenuDefaultItem(m, IDM_SHOW, FALSE);
    POINT p; GetCursorPos(&p);
    SetForegroundWindow(g.frame);
    TrackPopupMenu(m, TPM_RIGHTBUTTON, p.x, p.y, 0, g.frame, nullptr);
    PostMessageW(g.frame, WM_NULL, 0, 0);
    DestroyMenu(m);
}

// linha da lista de presets: ícone do tipo, nome e atalho (como a lista de presets de referência)
void DrawPresetItem(DRAWITEMSTRUCT* di) {
    if ((int)di->itemID < 0 || di->itemID >= g.store.presets.size()) return;
    HDC dc = di->hDC;
    RECT r = di->rcItem;
    FillRect(dc, &r, th.brSide);
    const Preset& p = g.store.presets[di->itemID];
    bool sel = (di->itemState & ODS_SELECTED) != 0;
    if (sel) {
        RECT f = r; InflateRect(&f, -S(2), -S(1));
        Gdiplus::Graphics gr(dc);
        gr.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
        COLORREF fill = Mix(th.bgSide, th.accent, 0.16);
        Gdiplus::GraphicsPath path;
        float rad = (float)S(4), d = rad * 2;
        Gdiplus::RectF rf((float)f.left, (float)f.top, (float)(f.right - f.left), (float)(f.bottom - f.top));
        path.AddArc(rf.X, rf.Y, d, d, 180, 90); path.AddArc(rf.GetRight() - d, rf.Y, d, d, 270, 90);
        path.AddArc(rf.GetRight() - d, rf.GetBottom() - d, d, d, 0, 90); path.AddArc(rf.X, rf.GetBottom() - d, d, d, 90, 90);
        path.CloseFigure();
        Gdiplus::SolidBrush b(Gdiplus::Color(255, GetRValue(fill), GetGValue(fill), GetBValue(fill)));
        gr.FillPath(&b, &path);
        COLORREF a = CaptureColor();
        Gdiplus::SolidBrush ab(Gdiplus::Color(255, GetRValue(a), GetGValue(a), GetBValue(a)));
        float ih = (float)S(14);
        gr.FillRectangle(&ab, rf.X + S(2), rf.Y + (rf.Height - ih) / 2, (float)S(3), ih);
    }
    int is = S(16), cy = (r.top + r.bottom) / 2;
    RECT ir{ r.left + S(12), cy - is / 2, r.left + S(12) + is, cy - is / 2 + is };
    icons::DrawOnDC(dc, p.kind == CaptureKind::Video ? icons::Video : icons::Camera, ir, sel ? th.accentText : RGB(80, 80, 86));
    std::wstring hk = p.hotkeyVk ? HotkeyToText(p.hotkeyMods, p.hotkeyVk) : L"";
    HGDIOBJ of = SelectObject(dc, ui::UiFont(g.dpi));
    SetBkMode(dc, TRANSPARENT);
    RECT hr{ r.left, r.top, r.right - S(10), r.bottom };
    if (!hk.empty()) {
        SetTextColor(dc, RGB(110, 110, 116));
        DrawTextW(dc, hk.c_str(), -1, &hr, DT_RIGHT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
        RECT m{}; DrawTextW(dc, hk.c_str(), -1, &m, DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX);
        hr.right -= m.right + S(12);
    }
    RECT tr{ ir.right + S(10), r.top, hr.right, r.bottom };
    SetTextColor(dc, RGB(28, 28, 32));
    DrawTextW(dc, p.name.c_str(), -1, &tr, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS);
    SelectObject(dc, of);
    if ((di->itemState & ODS_FOCUS) && !(di->itemState & ODS_NOFOCUSRECT)) { RECT f = r; InflateRect(&f, -S(2), -S(1)); DrawFocusRect(dc, &f); }
}

void DrawOwner(DRAWITEMSTRUCT* di) {
    if (di->CtlID == IDC_PRESETS) { DrawPresetItem(di); return; }
    HDC dc = di->hDC;
    RECT r = di->rcItem;
    bool video = Cur().kind == CaptureKind::Video;
    if (di->CtlID == IDC_TAB_IMAGE || di->CtlID == IDC_TAB_VIDEO) {
        bool active = (di->CtlID == IDC_TAB_VIDEO) == video;
        HBRUSH bg = CreateSolidBrush(active ? th.bgMain : th.bgSide);
        FillRect(dc, &r, bg); DeleteObject(bg);
        if (active) {   // indicador da aba ativa, como na navegação do Windows 11
            int ih = S(20);
            RECT ind{ r.left + S(3), (r.top + r.bottom - ih) / 2, r.left + S(6), (r.top + r.bottom + ih) / 2 };
            ui::FillRoundRect(dc, ind, S(3), th.accent, th.accent);
        }
        COLORREF fg = active ? th.accentText : RGB(70, 70, 74);
        int is = S(22), cx = (r.left + r.right) / 2;
        RECT ir{ cx - is / 2, r.top + S(9), cx + is / 2, r.top + S(9) + is };
        icons::DrawOnDC(dc, di->CtlID == IDC_TAB_VIDEO ? icons::Video : icons::Image, ir, fg);
        RECT tr{ r.left, ir.bottom + S(2), r.right, r.bottom - S(6) };
        ui::DrawTextCentered(dc, di->CtlID == IDC_TAB_VIDEO ? L"Vídeo" : L"Imagem", tr, fg, ui::UiFont(g.dpi, 9, active));
        if (di->itemState & ODS_FOCUS) { RECT f = r; InflateRect(&f, -2, -2); DrawFocusRect(dc, &f); }
        return;
    }
    FillRect(dc, &r, th.brMain);
    if (di->CtlID == IDC_CAPTURE) {
        int d = std::min(r.right - r.left, r.bottom - r.top - S(26));
        RECT c{ r.left + (r.right - r.left - d) / 2, r.top, 0, 0 };
        c.right = c.left + d; c.bottom = c.top + d;
        bool pressed = (di->itemState & ODS_SELECTED) != 0;
        COLORREF base = video ? RGB(226, 52, 52) : CaptureColor();
        COLORREF col = base;
        if (g.hoverCapture) col = Mix(base, RGB(255, 255, 255), 0.12);
        if (pressed) col = video ? RGB(190, 40, 40) : Mix(base, 0, 0.2);
        Gdiplus::Graphics gr(dc);
        gr.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
        Gdiplus::SolidBrush sh(Gdiplus::Color(40, 0, 0, 0));
        gr.FillEllipse(&sh, (INT)c.left + 2, (INT)c.top + 4, (INT)d, (INT)d);
        Gdiplus::SolidBrush b(Gdiplus::Color(255, GetRValue(col), GetGValue(col), GetBValue(col)));
        gr.FillEllipse(&b, (INT)c.left, (INT)c.top, (INT)d, (INT)d);
        float in = d * 0.27f;
        icons::Draw(gr, video ? icons::Video : icons::Capture,
                    Gdiplus::RectF(c.left + in, c.top + in, d - 2 * in, d - 2 * in), Gdiplus::Color(255, 255, 255, 255));
        RECT t{ r.left, c.bottom + S(4), r.right, r.bottom };
        ui::DrawTextCentered(dc, video ? L"Gravar" : L"Capturar", t, RGB(40, 40, 40), ui::UiFont(g.dpi, 11, true));
        if (di->itemState & ODS_FOCUS) { RECT f = r; InflateRect(&f, -1, -1); DrawFocusRect(dc, &f); }
    }
}

void OnCommand(int id, int code) {
    if (id >= IDM_PRESET_BASE && id < IDM_PRESET_BASE + (int)g.store.presets.size()) {
        StartCapture(g.store.presets[id - IDM_PRESET_BASE]); return;
    }
    switch (id) {
    case IDC_PRESETS:
        if (code == LBN_SELCHANGE) {
            int s = ListBox_GetCurSel(C(IDC_PRESETS));
            if (s >= 0) { g.store.selected = s; g.store.Save(); LoadControls(); g.loading = true; Layout(); g.loading = false; }
        } else if (code == LBN_DBLCLK) StartCapture(Cur());
        break;
    case IDC_PRESET_NEW: {
        Preset p; p.name = Format(L"Preset %zu", g.store.presets.size() + 1);
        p.kind = Cur().kind; if (p.kind == CaptureKind::Video) p.captureCursor = true;
        g.store.presets.push_back(p);
        g.store.selected = (int)g.store.presets.size() - 1;
        g.store.Save(); FillPresetList(); LoadControls(); Layout();
        if (!g.presetsOpen) TogglePresets();
        RenamePreset();
        break;
    }
    case IDC_PRESET_DUP: {
        Preset p = Cur(); p.name += L" (cópia)"; p.hotkeyMods = p.hotkeyVk = 0;
        g.store.presets.push_back(p);
        g.store.selected = (int)g.store.presets.size() - 1;
        g.store.Save(); FillPresetList(); LoadControls();
        break;
    }
    case IDC_PRESET_DEL:
        if (g.store.presets.size() <= 1) { MessageBoxW(g.frame, L"É preciso manter pelo menos um preset.", L"Snapnab", MB_ICONINFORMATION); break; }
        if (MessageBoxW(g.frame, (L"Excluir o preset \"" + Cur().name + L"\"?").c_str(), L"Snapnab", MB_YESNO | MB_ICONQUESTION) != IDYES) break;
        g.store.presets.erase(g.store.presets.begin() + g.store.selected);
        g.store.selected = std::max(0, g.store.selected - 1);
        g.store.Save(); RegisterHotkeys(); FillPresetList(); LoadControls(); Layout();
        break;
    case IDC_PRESETS_LABEL: TogglePresets(); break;
    case IDC_PRESET_RENAME: RenamePreset(); break;
    case IDC_PRESET_HOTKEY: EditPresetHotkey(); break;
    case IDC_TAB_IMAGE: case IDC_TAB_VIDEO: {
        CaptureKind k = id == IDC_TAB_VIDEO ? CaptureKind::Video : CaptureKind::Image;
        if (Cur().kind != k) {
            Cur().kind = k;
            auto list = SelectionsFor(k);
            if (std::find(list.begin(), list.end(), Cur().selection) == list.end()) Cur().selection = Selection::Region;
            g.store.Save(); FillPresetList(); LoadControls(); Layout();
        }
        break;
    }
    case IDC_SELECTION: if (code == CBN_SELCHANGE) SaveControls(); break;
    case IDC_SEL_CFG: EditFixedSize(); break;
    case IDC_EFFECTS:
        if (code == CBN_DROPDOWN) FillEffectsList();
        else if (code == CBN_SELCHANGE) {
            int i = ComboBox_GetCurSel(C(IDC_EFFECTS));
            if (i >= 0) OnEffectPicked(ComboBox_GetItemData(C(IDC_EFFECTS), i));
        } else if (code == CBN_CLOSEUP) g.fxAddMode = false;
        break;
    case IDC_SAVEFILE_CFG: EditFileOutput(); break;
    case IDC_PREVIEW: case IDC_CLIPBOARD: case IDC_SAVEFILE: case IDC_CURSOR: case IDC_MIC: case IDC_SYS:
    case IDC_DOWNSCALE: case IDC_HW:
        if (code == BN_CLICKED) SaveControls();
        break;
    case IDC_FPS: case IDC_QUALITY: if (code == CBN_SELCHANGE) SaveControls(); break;
    case IDC_DELAY: if (code == CBN_SELCHANGE) SaveControls(); break;
    case IDC_CAPTURE: StartCapture(Cur()); break;
    case IDC_OPEN_EDITOR: case IDM_OPEN_EDITOR: OpenEditor(); break;
    case IDC_OPTIONS: case IDM_OPTIONS: ShowOptions(); break;
    case IDM_CAPTURE_IMAGE: StartCapture(FirstOfKind(CaptureKind::Image)); break;
    case IDM_CAPTURE_VIDEO: StartCapture(FirstOfKind(CaptureKind::Video)); break;
    case IDM_REPEAT: if (g.haveLast) StartCapture(g.lastPreset); else StartCapture(Cur()); break;
    case IDM_CAPTURE_CLIPBOARD: { Preset p = FirstOfKind(CaptureKind::Image); p.selection = Selection::Clipboard; p.delaySeconds = 0; StartCapture(p); break; }
    case IDM_CAPTURE_FILE: { Preset p = FirstOfKind(CaptureKind::Image); p.selection = Selection::File; p.delaySeconds = 0; StartCapture(p); break; }
    case IDM_PRESET_IMPORT: {
        std::wstring f = L"Presets do Snapnab (*.snappresets;*.xml)|*.snappresets;*.xml|";
        std::replace(f.begin(), f.end(), L'|', L'\0');
        std::wstring path;
        if (ui::OpenFileDialog(g.frame, f, path)) {
            if (g.store.Import(path)) { FillPresetList(); TrayNotify(L"Snapnab", L"Presets importados."); }
            else MessageBoxW(g.frame, L"Arquivo de presets inválido.", L"Snapnab", MB_ICONWARNING);
        }
        break;
    }
    case IDM_PRESET_EXPORT: {
        std::wstring f = L"Presets do Snapnab (*.snappresets)|*.snappresets|";
        std::replace(f.begin(), f.end(), L'|', L'\0');
        std::wstring path = L"Presets.snappresets"; int fi = 1;
        if (ui::SaveFileDialog(g.frame, f, path, fi, L"snappresets") && !g.store.Export(path))
            MessageBoxW(g.frame, L"Não foi possível exportar.", L"Snapnab", MB_ICONWARNING);
        break;
    }
    case IDM_LIBRARY_FOLDER: ShellExecuteW(nullptr, L"open", LibraryDir().c_str(), nullptr, nullptr, SW_SHOWNORMAL); break;
    case IDM_LOGS: ShellExecuteW(nullptr, L"open", LogsDir().c_str(), nullptr, nullptr, SW_SHOWNORMAL); break;
    case IDM_ABOUT: About(); break;
    case IDM_SHOW: ShowFrame(); break;
    case IDM_EXIT:
        if (g.engine.recorder.state() != Recorder::Idle) {
            if (MessageBoxW(g.frame, L"Há uma gravação em andamento. Parar e sair?", L"Snapnab", MB_YESNO | MB_ICONQUESTION) != IDYES) break;
            g.engine.VideoStop();
        }
        DestroyWindow(g.frame);
        break;
    }
}

LRESULT CALLBACK FrameProc(HWND h, UINT m, WPARAM w, LPARAM l) {
    if (m == g.taskbarCreated && m) { TrayAdd(); return 0; }
    switch (m) {
    case WM_CREATE: {
        g.frame = h;
        g.dpi = DpiForWindow(h);
        g.presetsOpen = RegGetDword(L"CapturePresetsOpen", 0) != 0;
        LoadTheme();
        g.loading = true;          // UDM_SETBUDDY/SetWindowText disparam EN_CHANGE antes dos valores carregados
        CreateControls();
        FillPresetList();
        LoadControls();
        g.loading = true;
        ComputeMetrics();
        Layout();
        ResizeToContent();
        ApplyFrameTheme();
        g.loading = false;
        return 0;
    }
    case WM_SIZE: { bool l = g.loading; g.loading = true; Layout(); g.loading = l; InvalidateRect(h, nullptr, TRUE); return 0; }
    case WM_DPICHANGED: {
        g.dpi = HIWORD(w);
        RECT* r = (RECT*)l;
        for (auto& [id, hw] : g.ctl) SendMessageW(hw, WM_SETFONT, (WPARAM)ui::UiFont(g.dpi), TRUE);
        SetWindowPos(h, nullptr, r->left, r->top, r->right - r->left, r->bottom - r->top, SWP_NOZORDER | SWP_NOACTIVATE);
        bool ld = g.loading; g.loading = true;
        ComputeMetrics();
        Layout();
        ResizeToContent();
        g.loading = ld;
        return 0;
    }
    case WM_SETTINGCHANGE:
        if (l && lstrcmpiW((const wchar_t*)l, L"ImmersiveColorSet") == 0) { LoadTheme(); ApplyFrameTheme(); }
        break;
    case WM_DWMCOLORIZATIONCOLORCHANGED: LoadTheme(); ApplyFrameTheme(); break;
    case WM_ERASEBKGND: {
        HDC dc = (HDC)w;
        RECT cr; GetClientRect(h, &cr);
        FillRect(dc, &cr, th.brMain);
        RECT tabs{ 0, 0, lay.tabW, lay.contentH };
        FillRect(dc, &tabs, th.brSide);
        RECT bar{ 0, lay.contentH, cr.right, lay.contentH + lay.barH };
        FillRect(dc, &bar, th.brBar);
        RECT panel{ 0, lay.contentH + lay.barH, cr.right, cr.bottom };
        if (g.presetsOpen) FillRect(dc, &panel, th.brSide);
        HBRUSH ln = CreateSolidBrush(th.line);
        RECT l1{ 0, lay.contentH, cr.right, lay.contentH + 1 };
        FillRect(dc, &l1, ln);
        if (g.presetsOpen) { RECT l2{ 0, panel.top, cr.right, panel.top + 1 }; FillRect(dc, &l2, ln); }
        DeleteObject(ln);
        return 1;
    }
    case WM_CTLCOLORSTATIC: case WM_CTLCOLORBTN: {
        // fundo conforme a faixa da janela onde o controle está (conteúdo, barra inferior ou lista de presets)
        HWND c = (HWND)l;
        SetBkMode((HDC)w, TRANSPARENT);
        RECT rc; GetWindowRect(c, &rc);
        MapWindowPoints(nullptr, h, (POINT*)&rc, 2);
        int id = GetDlgCtrlID(c);
        bool dim = id == IDC_HINT || id == IDC_FILE_SUMMARY;
        SetTextColor((HDC)w, dim ? RGB(100, 100, 106) : GetSysColor(COLOR_WINDOWTEXT));
        if (rc.top >= lay.contentH + lay.barH) return (LRESULT)th.brSide;
        if (rc.top >= lay.contentH) return (LRESULT)th.brBar;
        return (LRESULT)th.brMain;
    }
    case WM_DRAWITEM: DrawOwner((DRAWITEMSTRUCT*)l); return TRUE;
    case WM_MEASUREITEM: ((MEASUREITEMSTRUCT*)l)->itemHeight = S(30); return TRUE;
    case WM_CTLCOLORLISTBOX: return (LRESULT)th.brSide;
    case WM_FX_ADD:
        g.fxAddMode = true;
        SendMessageW(C(IDC_EFFECTS), CB_SHOWDROPDOWN, TRUE, 0);
        return 0;
    case WM_COMMAND: OnCommand(LOWORD(w), HIWORD(w)); return 0;
    case WM_SETCURSOR:
        if ((HWND)w == C(IDC_CAPTURE)) {
            if (!g.hoverCapture) { g.hoverCapture = true; InvalidateRect(C(IDC_CAPTURE), nullptr, FALSE); SetTimer(h, 7, 100, nullptr); }
            SetCursor(LoadCursor(nullptr, IDC_HAND));
            return TRUE;
        }
        break;
    case WM_TIMER:
        if (w == 7) {
            POINT p; GetCursorPos(&p);
            if (WindowFromPoint(p) != C(IDC_CAPTURE)) { g.hoverCapture = false; KillTimer(h, 7); InvalidateRect(C(IDC_CAPTURE), nullptr, FALSE); }
        }
        return 0;
    case WM_HOTKEY: {
        int id = (int)w;
        if (id == HK_IMAGE) StartCapture(FirstOfKind(CaptureKind::Image));
        else if (id == HK_VIDEO) {
            if (g.engine.recorder.state() != Recorder::Idle) g.engine.VideoStop();
            else StartCapture(FirstOfKind(CaptureKind::Video));
        }
        else if (id == HK_VIDEO_PAUSE) g.engine.VideoStartPauseResume();
        else if (id == HK_VIDEO_STOP) g.engine.VideoStop();
        else if (id >= HK_PRESET_BASE && id - HK_PRESET_BASE < (int)g.store.presets.size()) StartCapture(g.store.presets[id - HK_PRESET_BASE]);
        return 0;
    }
    case WM_SNAP_TRAY:
        switch (LOWORD(l)) {
        case WM_LBUTTONUP: case NIN_SELECT: StartCapture(Cur()); break;     // "One Click"
        case WM_LBUTTONDBLCLK: ShowFrame(); break;
        case WM_RBUTTONUP: case WM_CONTEXTMENU: TrayMenu(); break;
        }
        return 0;
    case WM_SNAP_IPC: {
        std::unique_ptr<ipc::Message> msg((ipc::Message*)l);
        OnIpc(*msg);
        return 0;
    }
    case WM_SNAP_DONE: g.engine.OnRecorderNote(w); return 0;
    case WM_COPYDATA: {
        std::wstring cmd;
        if (!ParseCopyData((COPYDATASTRUCT*)l, cmd)) return FALSE;
        auto* p = new std::wstring(cmd);
        PostMessageW(h, WM_SNAP_CMDLINE, 0, (LPARAM)p);
        return TRUE;
    }
    case WM_SNAP_CMDLINE: {
        std::unique_ptr<std::wstring> cmd((std::wstring*)l);
        ProcessCommandLine(*cmd, false);
        return 0;
    }
    case WM_CLIPBOARDUPDATE: OnClipboardUpdate(); return 0;
    case WM_CLOSE: ShowWindow(h, SW_HIDE); return 0;     // continua na bandeja
    case WM_ENDSESSION: if (w) DestroyWindow(h); return 0;
    case WM_DESTROY:
        RemoveClipboardFormatListener(h);
        Shell_NotifyIconW(NIM_DELETE, &g.nid);
        g.hk.Clear();
        g.ipc.SendInt(ipc::kEditor, ipc::CloseEditorIfHidden, 0);
        PostQuitMessage(0);
        return 0;
    }
    return DefWindowProcW(h, m, w, l);
}

} // namespace

int WINAPI wWinMain(HINSTANCE hi, HINSTANCE, LPWSTR, int) {
    SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    // instância única: mutex + janela "SnapnabUI"; a segunda instância repassa a linha de comando (WM_COPYDATA)
    if (ForwardToExistingInstance(kCaptureMutex, kCaptureWndClass, &g.mutex)) return 0;
    CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
    INITCOMMONCONTROLSEX icc{ sizeof(icc), ICC_STANDARD_CLASSES | ICC_UPDOWN_CLASS | ICC_HOTKEY_CLASS | ICC_LISTVIEW_CLASSES };
    InitCommonControlsEx(&icc);
    GdiplusSession gdip;
    mui::Register(hi);
    LogInit(L"Snapnab");
    g.hi = hi;
    g.store.Load();

    g.iconBig = icons::MakeAppIcon(32, false);
    g.iconSmall = icons::MakeAppIcon(16, false);
    WNDCLASSEXW wc{ sizeof(wc) };
    wc.lpfnWndProc = FrameProc; wc.hInstance = hi; wc.hCursor = LoadCursor(nullptr, IDC_ARROW);
    wc.hIcon = LoadIconW(hi, MAKEINTRESOURCEW(1)); if (!wc.hIcon) wc.hIcon = g.iconBig;
    wc.hIconSm = g.iconSmall;
    wc.lpszClassName = kCaptureWndClass;
    RegisterClassExW(&wc);
    g.taskbarCreated = RegisterWindowMessageW(L"TaskbarCreated");
    // tamanho fixo, calculado pelo conteúdo em WM_CREATE (ResizeToContent)
    HWND frame = CreateWindowExW(0, kCaptureWndClass, L"Snapnab — Captura",
                                 WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN,
                                 CW_USEDEFAULT, CW_USEDEFAULT, 800, 300, nullptr, BuildMenu(), hi, nullptr);
    BOOL dark = FALSE;
    DwmSetWindowAttribute(frame, 20 /*DWMWA_USE_IMMERSIVE_DARK_MODE*/, &dark, sizeof dark);
    TrayAdd();
    g.hk.Init(frame);
    RegisterHotkeys();
    AddClipboardFormatListener(frame);
    g.lastClipSeq = GetClipboardSequenceNumber();

    g.engine.Init(frame, &g.ipc);
    g.engine.notify = TrayNotify;
    g.engine.onBusyChanged = [](bool busy) {
        SetVideoHotkeys(busy && g.lastPreset.kind == CaptureKind::Video);
    };
    // canal IPC: este processo é o canal 0
    g.ipc.Start(ipc::kCapture, [](const ipc::Message& m) {
        PostMessageW(g.frame, WM_SNAP_IPC, 0, (LPARAM)new ipc::Message(m));
    });
    ProcessCommandLine(GetCommandLineW(), true);

    MSG msg;
    while (GetMessageW(&msg, nullptr, 0, 0)) {
        if (g.frame && IsDialogMessageW(g.frame, &msg)) continue;
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
    g.ipc.Stop();
    Log(L"Snapnab encerrado");
    if (g.mutex) { ReleaseMutex(g.mutex); CloseHandle(g.mutex); }
    CoUninitialize();
    return 0;
}
