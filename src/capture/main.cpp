// Snapnab.exe — capture process (≙ Snagit32.exe): tray, capture window, global hotkeys,
// capture engine, video recording, native outputs, One Click and the IPC channel with the Editor (ds_pseudocode §1–§4)
#include "Common.h"
#include "CaptureEngine.h"
#include "Hotkeys.h"
#include "Icons.h"
#include "Ipc.h"
#include "Library.h"
#include "OneClick.h"
#include "Presets.h"
#include "Settings.h"
#include "CaptionBar.h"
#include "Ui.h"
#include "Webcam.h"
#include "Widgets.h"
#include <dwmapi.h>

using namespace snap;

namespace {

enum : int {
    IDC_PRESETS = 100, IDC_TAB_IMAGE, IDC_TAB_VIDEO, IDC_SEL_LABEL, IDC_SELECTION, IDC_EFFECTS_TITLE, IDC_EFFECTS,
    IDC_SHARE_LABEL, IDC_SHARE, IDC_QUALITY_LABEL, IDC_QUALITY, IDC_WEBCAM_LABEL, IDC_WEBCAM,
    IDC_PREVIEW, IDC_CLIPBOARD, IDC_CURSOR, IDC_DELAY, IDC_MIC, IDC_SYS, IDC_CAPTURE,
    IDC_PRESETS_LABEL, IDC_PRESET_ADD, IDC_OPEN_EDITOR,
    // menu
    IDM_EXIT = 1000, IDM_OPEN_EDITOR, IDM_CAPTURE_IMAGE, IDM_CAPTURE_VIDEO, IDM_CAPTURE_CLIPBOARD, IDM_CAPTURE_FILE,
    IDM_REPEAT, IDM_OPTIONS, IDM_PRESET_IMPORT, IDM_PRESET_EXPORT, IDM_ABOUT, IDM_LIBRARY_FOLDER, IDM_LOGS,
    IDM_SHOW, IDM_ONECLICK, IDM_SHOW_PRESETS, IDM_PRESET_FROM_CURRENT, IDM_PRESET_NEW, IDM_PRESET_HELP,
    IDM_PRESET_RUN, IDM_PRESET_RENAME, IDM_PRESET_DUP, IDM_PRESET_HOTKEY, IDM_PRESET_DEL,
    IDM_SETTINGS_EXPORT, IDM_SETTINGS_IMPORT, IDM_SETTINGS_BACKUPS, IDM_MANUAL,
    IDM_PRESET_BASE = 2000, IDM_DELAY_BASE = 3000, IDM_WEBCAM_OFF = 3100, IDM_WEBCAM_BASE = 3101,
    IDM_WEBCAM_SHAPE = 3200, IDM_WEBCAM_CHROMA = 3300,
};
constexpr UINT WM_FX_ADD = WM_APP + 0x60;       // reopen the effects list on "add another"
constexpr UINT WM_PRESET_ZONE = WM_APP + 0x62;  // click on the hotkey/gear of a preset row
enum : int { HK_IMAGE = 1, HK_VIDEO = 2, HK_VIDEO_PAUSE = 4, HK_VIDEO_STOP = 5, HK_PRESET_BASE = 100 };
enum Share { ShareNone, ShareFile, ShareProgram, ShareText };

struct App {
    HINSTANCE hi = nullptr;
    HWND frame = nullptr, tip = nullptr;
    HICON iconBig = nullptr, iconSmall = nullptr;
    NOTIFYICONDATAW nid{};
    UINT taskbarCreated = 0;
    PresetStore store;
    CaptureEngine engine;
    ipc::Notify ipc;
    HotkeyManager hk;
    OneClick oneClick;
    HANDLE mutex = nullptr;
    int dpi = 96;
    std::map<int, HWND> ctl;
    bool loading = false;
    bool hoverCapture = false;
    DWORD lastClipSeq = 0;
    Preset lastPreset;
    bool haveLast = false;
    bool presetsOpen = false;      // preset list expanded below the bottom bar
    int fxCurrent = -1;            // effect shown in the "Effects" box (target of "Remove this effect")
    bool fxAddMode = false;        // the open effects list is the "Add another effect" one
    HMENU menuBar = nullptr;       // title bar menus (File, Capture, View, Help)
    ui::CaptionBar caption;        // custom title bar (icon + menus), the same as the Editor's
    int presetHot = -1, presetZone = 0;
    std::vector<CameraInfo> cams;  // webcams listed in the last menu
    bool trayIgnoreUp = false;     // second click of a double click on the tray icon
} g;
constexpr UINT_PTR kTrayClickTimer = 8;

// measurements of the compact window (computed from the controls' text, in pixels at the current DPI)
struct Metrics {
    int capH, pad, rowP, ch, tabW, tabH;
    int ax, labW, fieldX, fieldW;                 // selection/effects/share column
    int bx, bw;                                   // options column (switches)
    int capX, capW, capD;                         // capture button
    int contentH, barH, panelH, clientW;
} lay;

// colors derived from the Windows accent color (Personalization > Colors), the same as the Editor's (common/Theme)
struct Theme : ui::ThemeColors {
    HBRUSH brMain = nullptr, brSide = nullptr, brBar = nullptr;
} th;

using ui::Mix;
using ui::Luma;
using ui::kCaptureRed;

HWND C(int id) { return g.ctl[id]; }
Preset& Cur() { return g.store.presets[g.store.selected]; }
int S(int v) { return ui::Scale(v, g.dpi); }

// ---------------------------------------------------------------- theme
void LoadTheme() {
    static_cast<ui::ThemeColors&>(th) = ui::LoadThemeColors();
    for (HBRUSH* b : { &th.brMain, &th.brSide, &th.brBar }) if (*b) DeleteObject(*b);
    th.brMain = CreateSolidBrush(th.bgMain);
    th.brSide = CreateSolidBrush(th.bgSide);
    th.brBar = CreateSolidBrush(th.bgBar);
    ui::ApplyThemeToWidgets(th);
    g.caption.SetTheme(th);
    OneClickTheme ot;
    ot.accent = th.control; ot.bg = th.bgMain; ot.line = th.line; ot.capture = kCaptureRed;
    g.oneClick.SetTheme(ot);
}

void ApplyFrameTheme() {
    COLORREF border = th.captionAccent ? 0xFFFFFFFF /*DWMWA_COLOR_DEFAULT*/ : th.line;
    DwmSetWindowAttribute(g.frame, 34 /*DWMWA_BORDER_COLOR*/, &border, sizeof border);
    RedrawWindow(g.frame, nullptr, nullptr, RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN | RDW_FRAME);
}

// ---------------------------------------------------------------- tray
void TrayAdd() {
    g.nid = {};
    g.nid.cbSize = sizeof(g.nid);
    g.nid.hWnd = g.frame;
    g.nid.uID = 1;
    g.nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    g.nid.uCallbackMessage = WM_SNAP_TRAY;
    g.nid.hIcon = g.iconSmall;
    wcscpy_s(g.nid.szTip, Tr(L"Snapnab — clique para capturar"));
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

// ---------------------------------------------------------------- hotkeys
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
            TrayNotify(Tr(L"Conflito de atalho"), Format(Tr(L"O atalho do preset \"%s\" já está em uso por outra ação."), Tr(p.name).c_str()));
    }
}

void SetVideoHotkeys(bool on) {
    if (on) {
        UINT m, v;
        UnpackHotkey(RegGetDword(L"VideoStartPauseResumeKey", PackHotkey(MOD_SHIFT, VK_F9)), m, v);
        // registered only while the HUD is open; if another program owns the key, the hook serves it meanwhile
        g.hk.Set(HK_VIDEO_PAUSE, m, v);
        UnpackHotkey(RegGetDword(L"VideoStopKey", PackHotkey(MOD_SHIFT, VK_F10)), m, v);
        g.hk.Set(HK_VIDEO_STOP, m, v);
    } else {
        g.hk.Remove(HK_VIDEO_PAUSE);
        g.hk.Remove(HK_VIDEO_STOP);
    }
}

std::wstring HotkeyFor(bool video) {
    UINT m, v;
    if (video) UnpackHotkey(RegGetDword(L"VideoHotkey", PackHotkey(MOD_SHIFT, VK_SNAPSHOT)), m, v);
    else UnpackHotkey(RegGetDword(L"ImageHotkey", PackHotkey(0, VK_SNAPSHOT)), m, v);
    return HotkeyToText(m, v);
}

// ---------------------------------------------------------------- capture
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

// ---------------------------------------------------------------- effects ("Effects" list of the capture window)
enum Fx { FxBorder, FxFilter, FxShadow, FxCaption, FxReplace, FxScale, FxEdge, FxTrim, FxAdjust, FxRotate, FxWatermark, FxResolution, FxCount };
enum : LPARAM { FxNone = -1, FxSep = -2, FxAdd = -3, FxRemove = -4 };   // action items of the list
const wchar_t* const kFxNames[FxCount] = {
    Tr(L"Borda"), Tr(L"Filtros"), Tr(L"Sombra"), Tr(L"Informações da captura"), Tr(L"Substituição de cor"), Tr(L"Redimensionar imagem"),
    Tr(L"Bordas especiais"), Tr(L"Cortar bordas"), Tr(L"Ajuste de cor"), Tr(L"Girar imagem"), Tr(L"Marca d'água"), Tr(L"Resolução da imagem"),
};

bool& FxOn(fx::CaptureEffects& e, int k) {
    bool* on[FxCount] = { &e.border, &e.filter, &e.shadow, &e.caption, &e.replaceColor, &e.scale,
                          &e.edge, &e.trim, &e.adjust, &e.rotate, &e.watermark, &e.resolution };
    return *on[k];
}

int FxCountOn(fx::CaptureEffects& e) { int n = 0; for (int k = 0; k < FxCount; k++) n += FxOn(e, k); return n; }

int FxFirstOn(fx::CaptureEffects& e) { for (int k = 0; k < FxCount; k++) if (FxOn(e, k)) return k; return -1; }

// settings of an effect; OK turns the effect on
bool EditFx(int k) {
    auto& e = Cur().effects;
    using F = ui::Field;
    std::vector<F> f;
    switch (k) {
    case FxBorder: f = { F::MakeInt(Tr(L"Largura (px)"), e.borderWidth, 1, 100), F::MakeColor(Tr(L"Cor"), e.borderColor) }; break;
    case FxFilter: f = { F::MakeCombo(Tr(L"Filtro"), { Tr(L"Tons de cinza"), Tr(L"Sépia"), Tr(L"Inverter cores"), Tr(L"Preto e branco") }, e.filterKind) }; break;
    case FxShadow:
        f = { F::MakeInt(Tr(L"Deslocamento (px)"), e.shadowOffset, 0, 50), F::MakeInt(Tr(L"Desfoque (px)"), e.shadowBlur, 0, 50),
              F::MakeInt(Tr(L"Opacidade (%)"), e.shadowOpacity, 0, 100) };
        break;
    case FxCaption:
        f = { F::MakeText(Tr(L"Texto"), e.captionText), F::MakeLabel(Tr(L"Campos: {date} {time} {computer} {user}")),
              F::MakeCombo(Tr(L"Posição"), { Tr(L"Embaixo"), Tr(L"Em cima") }, e.captionTop ? 1 : 0) };
        break;
    case FxReplace:
        f = { F::MakeColor(Tr(L"Cor a substituir"), e.replaceFrom), F::MakeColor(Tr(L"Nova cor"), e.replaceTo),
              F::MakeInt(Tr(L"Tolerância"), e.replaceTolerance, 0, 100) };
        break;
    case FxScale: f = { F::MakeInt(Tr(L"Escala (%)"), e.scalePercent, 10, 400) }; break;
    case FxEdge:
        f = { F::MakeCombo(Tr(L"Estilo"), { Tr(L"Rasgada"), Tr(L"Onda"), Tr(L"Dente de serra"), Tr(L"Dente de tubarão"), Tr(L"Esmaecer"), Tr(L"Chanfrada") }, e.edgeStyle),
              F::MakeInt(Tr(L"Tamanho (px)"), e.edgeSize, 4, 60),
              F::MakeCheck(Tr(L"Em cima"), (e.edgeSides & fx::Top) != 0), F::MakeCheck(Tr(L"Direita"), (e.edgeSides & fx::Right) != 0),
              F::MakeCheck(Tr(L"Embaixo"), (e.edgeSides & fx::Bottom) != 0), F::MakeCheck(Tr(L"Esquerda"), (e.edgeSides & fx::Left) != 0) };
        break;
    case FxTrim:
        f = { F::MakeLabel(Tr(L"Remove as faixas de cor uniforme em volta da captura.")), F::MakeInt(Tr(L"Tolerância"), e.trimTolerance, 0, 100) };
        break;
    case FxAdjust:
        f = { F::MakeInt(Tr(L"Brilho"), e.brightness, -100, 100), F::MakeInt(Tr(L"Contraste"), e.contrast, -100, 100),
              F::MakeInt(Tr(L"Matiz (graus)"), e.hue, -180, 180), F::MakeInt(Tr(L"Saturação"), e.saturation, -100, 100) };
        break;
    case FxRotate:
        f = { F::MakeCombo(Tr(L"Girar"), { Tr(L"90° à direita"), Tr(L"90° à esquerda"), L"180°", Tr(L"Espelhar na horizontal"), Tr(L"Espelhar na vertical") },
                           e.rotateKind) };
        break;
    case FxWatermark:
        f = { F::MakeText(Tr(L"Texto"), e.watermarkText), F::MakeInt(Tr(L"Opacidade (%)"), e.watermarkOpacity, 5, 100),
              F::MakeCheck(Tr(L"Repetir por toda a imagem"), e.watermarkTiled) };
        break;
    case FxResolution:
        f = { F::MakeInt(Tr(L"Resolução (DPI)"), e.dpi, 36, 2400), F::MakeLabel(Tr(L"Gravada nos arquivos salvos (PNG, JPEG, TIFF, BMP).")) };
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

// box text: the current effect and how many others are on
void UpdateEffectsText() {
    auto& e = Cur().effects;
    if (g.fxCurrent < 0 || !FxOn(e, g.fxCurrent)) g.fxCurrent = FxFirstOn(e);
    std::wstring t = Tr(L"Nenhum");
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
    if (g.fxAddMode) {          // "Add another effect": only the ones not on yet
        for (int k = 0; k < FxCount; k++) if (!FxOn(e, k)) add(kFxNames[k], k);
        return;
    }
    add(Tr(L"Nenhum"), FxNone);
    for (int k = 0; k < FxCount; k++) add(kFxNames[k], k, FxOn(e, k) ? mui::ItemChecked : 0);
    add(L"", FxSep, mui::ItemSeparator);
    add(Tr(L"Adicionar outro efeito"), FxAdd);
    add(Tr(L"Remover este efeito"), FxRemove);
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
        PostMessageW(g.frame, WM_FX_ADD, 0, 0);     // reopens the list with only the effects that are off
    } else if (item == FxRemove && g.fxCurrent >= 0) {
        FxOn(e, g.fxCurrent) = false;
        g.fxCurrent = -1;
        g.store.Save();
    }
    UpdateEffectsText();
}

// ---------------------------------------------------------------- share (capture output)
Share ShareOf(const Preset& p) {
    if (p.grabText) return ShareText;
    if (p.saveToFile) return ShareFile;
    if (p.openInProgram) return ShareProgram;
    return ShareNone;
}

void SetShare(Preset& p, Share s) {
    p.saveToFile = s == ShareFile;
    p.openInProgram = s == ShareProgram;
    p.grabText = s == ShareText;
}

std::wstring DelayText(int s) { return s <= 0 ? Tr(L"Desligado") : Format(Tr(L"%d s"), s); }

void FillPresetList() {
    HWND lb = C(IDC_PRESETS);
    SendMessageW(lb, WM_SETREDRAW, FALSE, 0);
    ListBox_ResetContent(lb);
    for (auto& p : g.store.presets) ListBox_AddString(lb, p.name.c_str());     // drawn in DrawPresetItem
    ListBox_SetCurSel(lb, g.store.selected);
    SendMessageW(lb, WM_SETREDRAW, TRUE, 0);
    InvalidateRect(lb, nullptr, TRUE);
}

void Layout();
void ComputeMetrics();
void ResizeToContent();

// the Selection gear: Region (choose during capture or fixed region) and, in image presets, the links of the page
// and (full screen / monitor) each window as an object; the box shows "Fixed region"
bool SelectionHasGear(const Preset& p) {
    if (p.selection == Selection::Region) return true;
    if (p.kind != CaptureKind::Image) return false;
    switch (p.selection) {
    case Selection::Window: case Selection::FullScreen: case Selection::Monitor: case Selection::Object: case Selection::Freehand:
    case Selection::MultiArea: return true;
    default: return false;
    }
}

void UpdateSelectionGear(const Preset& p) {
    bool region = p.selection == Selection::Region;
    SendMessageW(C(IDC_SELECTION), mui::MUI_SETGEAR, SelectionHasGear(p), 0);
    SendMessageW(C(IDC_SELECTION), mui::MUI_SETDISPLAYTEXT, 0, (LPARAM)(region && p.regionFixed ? Tr(L"Região fixa") : nullptr));
}

void LoadControls() {
    g.loading = true;
    Preset& p = Cur();
    bool video = p.kind == CaptureKind::Video;
    HWND sel = C(IDC_SELECTION);
    ComboBox_ResetContent(sel);
    auto list = SelectionsFor(p.kind);
    if (std::find(list.begin(), list.end(), p.selection) == list.end()) p.selection = list[0];
    for (size_t i = 0; i < list.size(); i++) {
        ComboBox_AddString(sel, SelectionName(list[i]));
        if (list[i] == p.selection) ComboBox_SetCurSel(sel, (int)i);
    }
    UpdateSelectionGear(p);
    g.fxCurrent = -1;
    UpdateEffectsText();
    Share sh = ShareOf(p);
    ComboBox_SetCurSel(C(IDC_SHARE), sh);
    SendMessageW(C(IDC_SHARE), mui::MUI_SETGEAR, sh == ShareFile || sh == ShareProgram, 0);
    Button_SetCheck(C(IDC_PREVIEW), p.previewInEditor);
    Button_SetCheck(C(IDC_CLIPBOARD), p.copyToClipboard);
    Button_SetCheck(C(IDC_CURSOR), p.captureCursor);
    SendMessageW(C(IDC_DELAY), mui::MUI_SETDISPLAYTEXT, 0, (LPARAM)DelayText(p.delaySeconds).c_str());
    SendMessageW(C(IDC_WEBCAM), mui::MUI_SETDISPLAYTEXT, 0, (LPARAM)(p.webcam ? Tr(L"Ligada") : Tr(L"Desligada")));
    Button_SetCheck(C(IDC_MIC), p.microphone);
    Button_SetCheck(C(IDC_SYS), p.systemAudio);
    ComboBox_SetCurSel(C(IDC_QUALITY), p.quality);
    // visibility per mode
    for (int id : { IDC_EFFECTS, IDC_EFFECTS_TITLE, IDC_SHARE, IDC_SHARE_LABEL, IDC_CLIPBOARD, IDC_DELAY })
        ShowWindow(C(id), video ? SW_HIDE : SW_SHOW);
    for (int id : { IDC_QUALITY, IDC_QUALITY_LABEL, IDC_WEBCAM, IDC_WEBCAM_LABEL, IDC_MIC, IDC_SYS })
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
    UpdateSelectionGear(p);
    p.previewInEditor = Button_GetCheck(C(IDC_PREVIEW)) == BST_CHECKED;
    p.copyToClipboard = Button_GetCheck(C(IDC_CLIPBOARD)) == BST_CHECKED;
    p.captureCursor = Button_GetCheck(C(IDC_CURSOR)) == BST_CHECKED;
    p.microphone = Button_GetCheck(C(IDC_MIC)) == BST_CHECKED;
    p.systemAudio = Button_GetCheck(C(IDC_SYS)) == BST_CHECKED;
    int qi = ComboBox_GetCurSel(C(IDC_QUALITY));
    if (qi >= 0) p.quality = qi;
    g.store.Save();
}

// ---------------------------------------------------------------- dialogs
void EditFileOutput() {
    auto& p = Cur();
    std::vector<ui::Field> f = {
        ui::Field::MakeCombo(Tr(L"Formato"), { L"PNG", L"JPEG", L"BMP", L"GIF", L"TIFF" }, (int)p.fileFormat),
        ui::Field::MakeText(Tr(L"Prefixo do nome"), p.filePrefix),
        ui::Field::MakeText(Tr(L"Pasta"), p.folder.empty() ? DefaultSaveDir() : p.folder),
        ui::Field::MakeLabel(Tr(L"Nome automático: <Prefixo><Número>, ex.: Captura001.png")),
    };
    auto validate = [](std::vector<ui::Field>& f) -> std::wstring {
        const std::wstring& pre = f[1].text;
        if (pre.empty() || pre.find_first_of(L"\\/:*?\"<>|") != std::wstring::npos) return Tr(L"O prefixo contém caracteres inválidos.");
        return L"";
    };
    if (!ui::RunForm(g.frame, Tr(L"Salvar em arquivo"), f, validate)) return;
    p.fileFormat = (FileFormat)f[0].value; p.filePrefix = f[1].text; p.folder = f[2].text;
    g.store.Save();
}

void EditProgramOutput() {
    auto& p = Cur();
    std::vector<ui::Field> f = { ui::Field::MakeText(Tr(L"Programa"), p.programPath),
                                 ui::Field::MakeLabel(Tr(L"A captura é aberta nesse programa (ex.: mspaint.exe).")) };
    if (!ui::RunForm(g.frame, Tr(L"Abrir em um programa externo"), f) || Trim(f[0].text).empty()) return;
    p.programPath = Trim(f[0].text);
    g.store.Save();
}

// fixed region: size and, optionally, the position on the screen (without a position, the rectangle is placed during capture)
bool EditFixedRegion() {
    auto& p = Cur();
    RECT vs = VirtualScreenRect();
    std::vector<ui::Field> f = {
        ui::Field::MakeInt(Tr(L"Largura (px)"), p.fixedW, 16, 20000),
        ui::Field::MakeInt(Tr(L"Altura (px)"), p.fixedH, 16, 20000),
        ui::Field::MakeCheck(Tr(L"Capturar sempre na mesma posição"), p.regionUsePos),
        ui::Field::MakeInt(Tr(L"Esquerda (X)"), p.regionX, vs.left, vs.right - 16),
        ui::Field::MakeInt(Tr(L"Topo (Y)"), p.regionY, vs.top, vs.bottom - 16),
        ui::Field::MakeLabel(Tr(L"Sem posição fixa, você arrasta o retângulo até o lugar na hora da captura.")),
    };
    if (!ui::RunForm(g.frame, Tr(L"Região fixa"), f)) return false;
    p.fixedW = f[0].value; p.fixedH = f[1].value;
    p.regionUsePos = f[2].value != 0;
    p.regionX = f[3].value; p.regionY = f[4].value;
    return true;
}

enum : int { IDM_REGION_INTERACTIVE = 3400, IDM_REGION_FIXED, IDM_WINDOW_OBJECTS, IDM_CAPTURE_LINKS };

void RegionMenu() {
    Preset& p = Cur();
    HMENU m = CreatePopupMenu();
    if (p.selection == Selection::Region) {
        AppendMenuW(m, MF_STRING, IDM_REGION_INTERACTIVE, Tr(L"Selecionar a região na captura"));
        AppendMenuW(m, MF_STRING, IDM_REGION_FIXED, Tr(L"Região fixa…"));
        CheckMenuRadioItem(m, IDM_REGION_INTERACTIVE, IDM_REGION_FIXED, p.regionFixed ? IDM_REGION_FIXED : IDM_REGION_INTERACTIVE, MF_BYCOMMAND);
    }
    if (p.kind == CaptureKind::Image) {
        if (GetMenuItemCount(m)) AppendMenuW(m, MF_SEPARATOR, 0, nullptr);
        if (p.selection == Selection::FullScreen || p.selection == Selection::Monitor)
            AppendMenuW(m, MF_STRING | (p.windowObjects ? MF_CHECKED : 0), IDM_WINDOW_OBJECTS, Tr(L"Cada janela como um objeto separado"));
        AppendMenuW(m, MF_STRING | (p.captureLinks ? MF_CHECKED : 0), IDM_CAPTURE_LINKS, Tr(L"Guardar os links da página (clicáveis, também no PDF)"));
    }
    // opens below the gear, aligned to its left edge (as in the reference)
    RECT r; GetWindowRect(C(IDC_SELECTION), &r);
    RECT ex = r;
    TPMPARAMS tp{ sizeof tp, ex };
    int id = (int)TrackPopupMenuEx(m, TPM_RETURNCMD | TPM_LEFTALIGN | TPM_TOPALIGN | TPM_VERTICAL, r.right - S(30), r.bottom, g.frame, &tp);
    DestroyMenu(m);
    if (id == IDM_REGION_INTERACTIVE) p.regionFixed = false;
    else if (id == IDM_REGION_FIXED) { if (!EditFixedRegion()) return; p.regionFixed = true; }
    else if (id == IDM_WINDOW_OBJECTS) p.windowObjects = !p.windowObjects;
    else if (id == IDM_CAPTURE_LINKS) p.captureLinks = !p.captureLinks;
    else return;
    g.store.Save();
    UpdateSelectionGear(p);
}

void EditVideoOptions() {
    auto& p = Cur();
    static const int fpsList[] = { 10, 15, 24, 30, 60 };
    int fi = 3;
    for (int i = 0; i < 5; i++) if (fpsList[i] == p.fps) fi = i;
    std::vector<ui::Field> f = {
        ui::Field::MakeCombo(Tr(L"Quadros por segundo"), { L"10", L"15", L"24", L"30", L"60" }, fi),
        ui::Field::MakeCheck(Tr(L"Reduzir para 1080p"), p.downscale1080),
        ui::Field::MakeCheck(Tr(L"Codificação por hardware"), p.hardwareEncode),
        ui::Field::MakeSeparator(Tr(L"Áudio")),
        ui::Field::MakeInt(Tr(L"Volume do microfone (%)"), p.micVolume, 0, 200),
        ui::Field::MakeInt(Tr(L"Volume do áudio do sistema (%)"), p.sysVolume, 0, 200),
        ui::Field::MakeCheck(Tr(L"Ganho automático do microfone"), p.autoGain),
        ui::Field::MakeLabel(Tr(L"Também ajustáveis no painel da gravação.")),
    };
    if (!ui::RunForm(g.frame, Tr(L"Opções de vídeo"), f)) return;
    p.fps = fpsList[std::clamp(f[0].value, 0, 4)];
    p.downscale1080 = f[1].value != 0;
    p.hardwareEncode = f[2].value != 0;
    p.micVolume = f[4].value; p.sysVolume = f[5].value; p.autoGain = f[6].value != 0;
    g.store.Save();
}

void EditPresetHotkey() {
    auto& p = Cur();
    std::vector<ui::Field> f = { ui::Field::MakeHotkey(Tr(L"Atalho do preset"), p.hotkeyMods, p.hotkeyVk),
                                 ui::Field::MakeLabel(Tr(L"Deixe vazio (Backspace) para remover.")) };
    if (!ui::RunForm(g.frame, Tr(L"Atalho — ") + Tr(p.name), f)) return;
    UINT oldM = p.hotkeyMods, oldV = p.hotkeyVk;
    p.hotkeyMods = f[0].hkMods; p.hotkeyVk = f[0].hkVk;
    int id = HK_PRESET_BASE + g.store.selected;
    auto r = g.hk.Set(id, p.hotkeyMods, p.hotkeyVk);
    if (r == HotkeyManager::InternalConflict) {
        MessageBoxW(g.frame, Tr(L"Esse atalho já está em uso por outra ação do Snapnab."), Tr(L"Conflito de atalho"), MB_ICONWARNING);
        p.hotkeyMods = oldM; p.hotkeyVk = oldV;
        return;
    }
    if (r == HotkeyManager::HookedFallback)
        MessageBoxW(g.frame, Tr(L"Outro programa já usa esse atalho. O Snapnab vai interceptá-lo por um gancho de teclado."),
                    Tr(L"Atalho"), MB_ICONINFORMATION);
    g.store.Save();
    FillPresetList();
}

bool AskPresetName(std::wstring& name, const wchar_t* title) {
    std::vector<ui::Field> f = { ui::Field::MakeText(Tr(L"Nome do preset"), name) };
    if (!ui::RunForm(g.frame, title, f) || Trim(f[0].text).empty()) return false;
    name = Trim(f[0].text);
    return true;
}

void RenamePreset() {
    std::wstring n = Cur().name;
    if (!AskPresetName(n, Tr(L"Renomear preset"))) return;
    Cur().name = n;
    g.store.Save();
    FillPresetList();
}

void ShowPresetList(bool open);

void AddPreset(Preset p, const wchar_t* title) {
    if (!AskPresetName(p.name, title)) return;
    p.hotkeyMods = p.hotkeyVk = 0;
    p.builtin.clear();
    g.store.presets.push_back(p);
    g.store.selected = (int)g.store.presets.size() - 1;
    g.store.Save(); FillPresetList(); LoadControls(); Layout();
    ShowPresetList(true);
}

void PresetHelp() {
    MessageBoxW(g.frame,
        Tr(L"Um preset guarda tudo o que a janela de captura mostra: tipo (imagem ou vídeo), seleção, efeitos, "
        L"destino e opções de gravação.\n\n"
        L"•  Clique em um preset para carregá-lo na janela; clique duas vezes para capturar com ele.\n"
        L"•  \"Adicionar atalho\" dá a ele uma tecla de atalho global.\n"
        L"•  A engrenagem da linha renomeia, duplica ou exclui o preset.\n"
        L"•  \"Novo preset com as configurações atuais\" salva o que está na janela agora."),
        Tr(L"Presets"), MB_ICONINFORMATION);
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

// restarts the capturer and the Editor (language change): the new instances wait for the current ones to close (/restartwait)
// settings in one file (F51). The import closes the Editor first (so it does not write its settings over the
// imported ones when it exits), keeps a backup of the current settings and restarts both programs with the new ones
void ExportSettings() {
    std::wstring f = Tr(L"Configurações do Snapnab (*.snapsettings)|*.snapsettings|");
    std::replace(f.begin(), f.end(), L'|', L'\0');
    std::wstring path = L"Snapnab.snapsettings"; int fi = 1;
    if (!ui::SaveFileDialog(g.frame, f, path, fi, L"snapsettings", Tr(L"Exportar configurações"))) return;
    g.store.Save();
    std::wstring err;
    if (settings::Export(path, err)) TrayNotify(L"Snapnab", Tr(L"Configurações exportadas: presets, temas, estilos rápidos, favoritos, efeitos rápidos, ordem dos carimbos e barra de ferramentas."));
    else MessageBoxW(g.frame, err.c_str(), L"Snapnab", MB_ICONWARNING);
}

void ImportSettings() {
    std::wstring f = Tr(L"Configurações do Snapnab (*.snapsettings)|*.snapsettings|");
    std::replace(f.begin(), f.end(), L'|', L'\0');
    std::wstring path;
    if (!ui::OpenFileDialog(g.frame, f, path, Tr(L"Importar configurações"))) return;
    if (!settings::Validate(path)) { MessageBoxW(g.frame, Tr(L"Arquivo de configurações inválido."), L"Snapnab", MB_ICONWARNING); return; }
    std::wstring replace = Tr(L"Substituir\nAs configurações atuais dão lugar às do arquivo.");
    std::wstring merge = Tr(L"Mesclar\nMantém as configurações atuais e acrescenta o que faltar (presets, temas, favoritos, efeitos rápidos…).");
    TASKDIALOG_BUTTON btns[] = { { 100, replace.c_str() }, { 101, merge.c_str() } };
    TASKDIALOGCONFIG tc{ sizeof tc };
    tc.hwndParent = g.frame;
    tc.dwFlags = TDF_USE_COMMAND_LINKS | TDF_ALLOW_DIALOG_CANCELLATION;
    tc.pszWindowTitle = L"Snapnab";
    tc.pszMainInstruction = Tr(L"Importar configurações");
    tc.pszContent = Tr(L"O Snapnab e o Editor reiniciam com as configurações importadas. Uma cópia das configurações atuais fica na pasta de backups.");
    tc.cButtons = 2; tc.pButtons = btns;
    tc.dwCommonButtons = TDCBF_CANCEL_BUTTON;
    int btn = 0;
    if (FAILED(TaskDialogIndirect(&tc, &btn, nullptr, nullptr)) || (btn != 100 && btn != 101)) return;
    bool editorOpen = false;
    if (HWND ed = FindWindowW(kEditorWndClass, nullptr)) {
        editorOpen = true;
        DWORD pid = 0; GetWindowThreadProcessId(ed, &pid);
        HANDLE ph = OpenProcess(SYNCHRONIZE, FALSE, pid);
        PostMessageW(ed, WM_CLOSE, 0, 0);
        if (ph) { WaitForSingleObject(ph, 15000); CloseHandle(ph); }
    }
    std::wstring err;
    g.store.Save();
    settings::Export(settings::BackupDir() + L"\\Before-import-" + TimeStamp() + L".snapsettings", err);
    if (!settings::Import(path, btn == 101, err)) {
        MessageBoxW(g.frame, err.c_str(), L"Snapnab", MB_ICONWARNING);
        if (editorOpen) LaunchSibling(kEditorExe, L"");
        return;
    }
    g.store.Load();                              // the imported presets (in case they are saved on the way out)
    if (editorOpen) LaunchSibling(kEditorExe, L"");
    if (LaunchSibling(kCaptureExe, RestartArgs())) SendMessageW(g.frame, WM_COMMAND, IDM_EXIT, 0);
}

void RestartApps() {
    if (FindWindowW(kEditorWndClass, nullptr)) g.ipc.SendInt(ipc::kEditor, ipc::RestartEditor, 0, true);
    if (LaunchSibling(kCaptureExe, RestartArgs())) SendMessageW(g.frame, WM_COMMAND, IDM_EXIT, 0);
}

void ShowOptions() {
    UINT im, iv, vm, vv, pm, pv, sm, sv;
    UnpackHotkey(RegGetDword(L"ImageHotkey", PackHotkey(0, VK_SNAPSHOT)), im, iv);
    UnpackHotkey(RegGetDword(L"VideoHotkey", PackHotkey(MOD_SHIFT, VK_SNAPSHOT)), vm, vv);
    UnpackHotkey(RegGetDword(L"VideoStartPauseResumeKey", PackHotkey(MOD_SHIFT, VK_F9)), pm, pv);
    UnpackHotkey(RegGetDword(L"VideoStopKey", PackHotkey(MOD_SHIFT, VK_F10)), sm, sv);
    // language: "Automatic" follows Windows (English if its language has no translation); the others by their own name
    std::vector<std::wstring> langCodes = { L"" }, langNames = { Tr(L"Automático (idioma do Windows)") };
    for (auto& l : i18n::Languages())
        if (i18n::Available(l.code)) { langCodes.push_back(l.code); langNames.push_back(l.name); }
    int langIndex = (int)(std::find(langCodes.begin(), langCodes.end(), i18n::Configured()) - langCodes.begin());
    std::vector<ui::Field> f = {
        ui::Field::MakeSeparator(Tr(L"Atalhos globais")),
        ui::Field::MakeHotkey(Tr(L"Capturar imagem"), im, iv),
        ui::Field::MakeHotkey(Tr(L"Gravar vídeo"), vm, vv),
        ui::Field::MakeHotkey(Tr(L"Pausar/retomar vídeo"), pm, pv),
        ui::Field::MakeHotkey(Tr(L"Parar vídeo"), sm, sv),
        ui::Field::MakeSeparator(Tr(L"Geral")),
        ui::Field::MakeText(Tr(L"Pasta de saída"), DefaultSaveDir()),
        ui::Field::MakeCombo(Tr(L"Monitorar área de transferência"), { Tr(L"Desligado"), Tr(L"Só a Ferramenta de Captura do Windows"), Tr(L"Qualquer aplicativo") },
                             (int)RegGetDword(L"ClipboardMonitor", 1)),
        ui::Field::MakeCheck(Tr(L"Iniciar com o Windows"), StartupEnabled()),
        ui::Field::MakeCheck(Tr(L"Mostrar a janela de captura ao iniciar"), RegGetDword(L"ShowCaptureWindow", 1) != 0),
        ui::Field::MakeCombo(Tr(L"Idioma"), langNames, langIndex < (int)langCodes.size() ? langIndex : 0),
        ui::Field::MakeLabel(Tr(L"Biblioteca: ") + LibraryDir()),
    };
    auto validate = [](std::vector<ui::Field>& f) -> std::wstring {
        std::vector<std::pair<UINT, UINT>> keys;
        for (int i = 1; i <= 4; i++) {
            if (!f[i].hkVk) continue;
            for (auto& k : keys) if (k == std::make_pair(f[i].hkMods, f[i].hkVk)) return Tr(L"Dois atalhos iguais: ") + HotkeyToText(f[i].hkMods, f[i].hkVk);
            keys.push_back({ f[i].hkMods, f[i].hkVk });
        }
        return L"";
    };
    if (!ui::RunForm(g.frame, Tr(L"Opções do Snapnab"), f, validate, 480)) return;
    RegSetDword(L"ImageHotkey", PackHotkey(f[1].hkMods, f[1].hkVk));
    RegSetDword(L"VideoHotkey", PackHotkey(f[2].hkMods, f[2].hkVk));
    RegSetDword(L"VideoStartPauseResumeKey", PackHotkey(f[3].hkMods, f[3].hkVk));
    RegSetDword(L"VideoStopKey", PackHotkey(f[4].hkMods, f[4].hkVk));
    if (!Trim(f[6].text).empty()) RegSetString(L"OutputFolder", Trim(f[6].text));
    RegSetDword(L"ClipboardMonitor", f[7].value);
    SetStartup(f[8].value != 0);
    RegSetDword(L"ShowCaptureWindow", f[9].value);
    RegisterHotkeys();
    g.ipc.SendInt(ipc::kEditor, ipc::ReloadConfig, 0);
    std::wstring lang = langCodes[std::clamp(f[10].value, 0, (int)langCodes.size() - 1)];
    if (lang != i18n::Configured()) {
        i18n::SetConfigured(lang);
        // the strings are loaded when each program starts (like the reference's resource DLL)
        if (i18n::Resolve() != i18n::Current() &&
            MessageBoxW(g.frame, Tr(L"O novo idioma vale depois que o Snapnab e o Editor reiniciarem. Reiniciar agora?"), L"Snapnab",
                        MB_YESNO | MB_ICONQUESTION) == IDYES)
            RestartApps();
    }
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
    std::wstring text = L"Snapnab " + std::wstring(kAppVersion) + Tr(L" — captura de tela e gravação de vídeo") +
        L"\n" SNAPNAB_COPYRIGHT_W L"\n" + Tr(L"Software livre sob a GNU General Public License versão 3.") + L"\n\n" +
        Tr(L"Processo de captura (Snapnab.exe) + Editor (SnapnabEditor.exe), comunicando-se por memória\n"
           L"compartilhada (canal IPC) e WM_COPYDATA. Capturas ficam na biblioteca:\n") + LibraryDir();
    MessageBoxW(g.frame, text.c_str(), Tr(L"Sobre o Snapnab"), MB_ICONINFORMATION);
}

// ---------------------------------------------------------------- One Click
void SetOneClick(bool on) {
    RegSetDword(L"OneClickEnabled", on);
    g.oneClick.Show(on);
}

void PresetsSubmenu(HMENU m) {
    for (size_t i = 0; i < g.store.presets.size(); i++)
        AppendMenuW(m, MF_STRING, IDM_PRESET_BASE + i, Tr(g.store.presets[i].name).c_str());   // the built-in ones, translated
}

void OneClickMenu(POINT pt) {
    HMENU m = CreatePopupMenu();
    AppendMenuW(m, MF_STRING, IDM_CAPTURE_IMAGE, Tr(L"Capturar imagem"));
    AppendMenuW(m, MF_STRING, IDM_CAPTURE_VIDEO, Tr(L"Gravar vídeo"));
    HMENU pre = CreatePopupMenu();
    PresetsSubmenu(pre);
    AppendMenuW(m, MF_POPUP, (UINT_PTR)pre, Tr(L"Presets"));
    AppendMenuW(m, MF_SEPARATOR, 0, nullptr);
    AppendMenuW(m, MF_STRING, IDM_SHOW, Tr(L"Abrir a janela de captura"));
    AppendMenuW(m, MF_STRING, IDM_ONECLICK, Tr(L"Ocultar o One Click"));
    SetForegroundWindow(g.frame);
    TrackPopupMenu(m, TPM_RIGHTBUTTON | TPM_CENTERALIGN, pt.x, pt.y, 0, g.frame, nullptr);
    PostMessageW(g.frame, WM_NULL, 0, 0);
    DestroyMenu(m);
}

// ---------------------------------------------------------------- clipboard monitor (A13)
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
    Log(L"Clipboard: image detected from '%s'", proc.c_str());
    // the Editor creates the new image from the clipboard (IPC 0x1E, ed_comp_14 §3.4)
    if (IsProcessRunning(kEditorMutex)) g.ipc.SendInt(ipc::kEditor, ipc::NewImageFromClipboard, 0);
    else LaunchSibling(kEditorExe, L"/NewFromClipboard");
}

// ---------------------------------------------------------------- IPC received from the Editor
void OnIpc(const ipc::Message& m) {
    switch (m.code) {
    case ipc::EditorStartCapture: StartCapture(FirstOfKind(m.ivalue == 1 ? CaptureKind::Video : CaptureKind::Image)); break;
    case ipc::EditorClosed: Log(L"Editor closed"); break;
    case ipc::ReloadConfig: g.store.Load(); FillPresetList(); LoadControls(); break;
    default: break;
    }
}

// ---------------------------------------------------------------- command line (G3)
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
            MessageBoxW(nullptr, Tr(L"Snapnab.exe [opções]\n\n"
                L"/hide            inicia só na bandeja\n"
                L"/capture         captura imagem com o preset de imagem atual\n"
                L"/video           grava vídeo com o preset de vídeo atual\n"
                L"/preset \"nome\"   executa um preset pelo nome\n"
                L"/clipboard       captura a imagem da área de transferência\n"
                L"/exit            encerra o Snapnab em execução"), Tr(L"Snapnab — linha de comando"), MB_ICONINFORMATION);
            handled = true;
        }
    }
    if (!handled && (!startup || (!hide && RegGetDword(L"ShowCaptureWindow", 1)))) ShowFrame();
}

// ---------------------------------------------------------------- window
HWND MakeCtl(int id, const wchar_t* cls, const wchar_t* text, DWORD style, DWORD ex = 0) {
    HWND h = CreateWindowExW(ex, cls, text, WS_CHILD | WS_VISIBLE | style, 0, 0, 10, 10, g.frame, (HMENU)(INT_PTR)id, g.hi, nullptr);
    SendMessageW(h, WM_SETFONT, (WPARAM)ui::UiFont(g.dpi), TRUE);
    g.ctl[id] = h;
    return h;
}

LRESULT CALLBACK PresetListProc(HWND, UINT, WPARAM, LPARAM, UINT_PTR, DWORD_PTR);

void SetFonts() {
    for (auto& [id, hw] : g.ctl) SendMessageW(hw, WM_SETFONT, (WPARAM)ui::UiFont(g.dpi), TRUE);
    SendMessageW(C(IDC_PRESET_ADD), WM_SETFONT, (WPARAM)ui::UiFont(g.dpi, 16), TRUE);    // bigger "+"
}

void CreateControls() {
    using namespace mui;
    auto style = [](int id, UINT st, int icon = -1) {
        SendMessageW(C(id), MUI_SETSTYLE, st, 0);
        if (icon >= 0) SendMessageW(C(id), MUI_SETICON, icon + 1, 0);
    };
    // created in Tab order
    MakeCtl(IDC_TAB_IMAGE, L"BUTTON", Tr(L"Imagem"), BS_OWNERDRAW | WS_TABSTOP);
    MakeCtl(IDC_TAB_VIDEO, L"BUTTON", Tr(L"Vídeo"), BS_OWNERDRAW | WS_TABSTOP);
    MakeCtl(IDC_SEL_LABEL, L"STATIC", Tr(L"Seleção:"), SS_LEFT | SS_CENTERIMAGE);
    MakeCtl(IDC_SELECTION, kCombo, nullptr, WS_TABSTOP);
    style(IDC_SELECTION, ComboGear);
    MakeCtl(IDC_EFFECTS_TITLE, L"STATIC", Tr(L"Efeitos:"), SS_LEFT | SS_CENTERIMAGE);
    MakeCtl(IDC_EFFECTS, kCombo, nullptr, WS_TABSTOP);
    style(IDC_EFFECTS, ComboMenu);
    MakeCtl(IDC_SHARE_LABEL, L"STATIC", Tr(L"Compartilhar:"), SS_LEFT | SS_CENTERIMAGE);
    HWND sh = MakeCtl(IDC_SHARE, kCombo, nullptr, WS_TABSTOP);
    style(IDC_SHARE, ComboGear);
    for (auto s : { Tr(L"Nenhum"), Tr(L"Arquivo"), Tr(L"Programa externo"), Tr(L"Texto (OCR)") }) ComboBox_AddString(sh, s);
    MakeCtl(IDC_QUALITY_LABEL, L"STATIC", Tr(L"Qualidade:"), SS_LEFT | SS_CENTERIMAGE);
    HWND q = MakeCtl(IDC_QUALITY, kCombo, nullptr, WS_TABSTOP);
    style(IDC_QUALITY, ComboGear);
    for (auto s : { Tr(L"Muito baixa"), Tr(L"Baixa"), Tr(L"Média"), Tr(L"Alta") }) ComboBox_AddString(q, s);
    MakeCtl(IDC_WEBCAM_LABEL, L"STATIC", Tr(L"Webcam:"), SS_LEFT | SS_CENTERIMAGE);
    MakeCtl(IDC_WEBCAM, kButton, L"", WS_TABSTOP);
    style(IDC_WEBCAM, ButtonLink);
    MakeCtl(IDC_PREVIEW, kToggle, Tr(L"Visualizar no Editor"), WS_TABSTOP);
    MakeCtl(IDC_CLIPBOARD, kToggle, Tr(L"Copiar para a área de transferência"), WS_TABSTOP);
    MakeCtl(IDC_CURSOR, kToggle, Tr(L"Capturar o cursor"), WS_TABSTOP);
    MakeCtl(IDC_DELAY, kButton, Tr(L"Atraso"), WS_TABSTOP);
    style(IDC_DELAY, ButtonLink);
    MakeCtl(IDC_MIC, kToggle, Tr(L"Gravar microfone"), WS_TABSTOP);
    MakeCtl(IDC_SYS, kToggle, Tr(L"Gravar áudio do sistema"), WS_TABSTOP);
    MakeCtl(IDC_CAPTURE, L"BUTTON", Tr(L"Capturar"), BS_OWNERDRAW | WS_TABSTOP);
    // bottom bar
    MakeCtl(IDC_PRESETS_LABEL, kButton, Tr(L"Presets"), WS_TABSTOP);
    style(IDC_PRESETS_LABEL, ButtonFlat | ButtonAccent, icons::Gear);
    MakeCtl(IDC_PRESET_ADD, kButton, L"+", WS_TABSTOP);
    style(IDC_PRESET_ADD, ButtonFlat | ButtonMenu | ButtonAccent);
    MakeCtl(IDC_OPEN_EDITOR, kButton, Tr(L"Abrir Editor"), WS_TABSTOP);
    style(IDC_OPEN_EDITOR, ButtonFlat, icons::Editor);
    // preset list (opens below the bar)
    HWND lb = MakeCtl(IDC_PRESETS, L"LISTBOX", nullptr, WS_TABSTOP | LBS_NOTIFY | WS_VSCROLL | LBS_NOINTEGRALHEIGHT | LBS_OWNERDRAWFIXED | LBS_HASSTRINGS);
    SetWindowSubclass(lb, PresetListProc, 1, 0);
    SetFonts();
    // capture button tooltip (the hotkey goes here, not below the button)
    g.tip = CreateWindowExW(WS_EX_TOPMOST, TOOLTIPS_CLASSW, nullptr, WS_POPUP | TTS_ALWAYSTIP | TTS_NOPREFIX, 0, 0, 0, 0,
                            g.frame, nullptr, g.hi, nullptr);
    TOOLINFOW ti{ sizeof ti };
    ti.uFlags = TTF_IDISHWND | TTF_SUBCLASS;
    ti.hwnd = g.frame; ti.uId = (UINT_PTR)C(IDC_CAPTURE);
    ti.lpszText = LPSTR_TEXTCALLBACKW;
    SendMessageW(g.tip, TTM_ADDTOOLW, 0, (LPARAM)&ti);
}

int TextW(const std::wstring& t, HFONT f = nullptr) {
    HDC dc = GetDC(g.frame);
    HGDIOBJ o = SelectObject(dc, f ? f : ui::UiFont(g.dpi));
    RECT r{};
    DrawTextW(dc, t.c_str(), -1, &r, DT_CALCRECT | DT_NOPREFIX);
    SelectObject(dc, o); ReleaseDC(g.frame, dc);
    return r.right;
}

int TextW(int id) { wchar_t b[256]; GetWindowTextW(C(id), b, 256); return TextW(b); }

int MaxTextW(std::initializer_list<int> ids) { int w = 0; for (int id : ids) w = std::max(w, TextW(id)); return w; }

const wchar_t* const kMenuNames[] = { TR_NOOP(L"Arquivo"), TR_NOOP(L"Captura"), TR_NOOP(L"Exibir"), TR_NOOP(L"Ajuda") };   // Tr() when used

// Layout (like the reference capture window):
//   title bar: icon + menus | minimize, close
//   vertical tabs | labels + fields | switches and options | round capture button
//   bottom bar: ⚙ Presets  + ……… Open Editor; the preset list opens below it
void ComputeMetrics() {
    lay.pad = S(14); lay.rowP = S(34); lay.ch = S(28);
    lay.tabW = S(70); lay.tabH = S(66);
    lay.ax = lay.tabW + S(16);
    lay.labW = MaxTextW({ IDC_SEL_LABEL, IDC_EFFECTS_TITLE, IDC_SHARE_LABEL, IDC_QUALITY_LABEL, IDC_WEBCAM_LABEL }) + S(10);
    lay.fieldX = lay.ax + lay.labW;
    lay.fieldW = S(160);
    lay.bx = lay.fieldX + lay.fieldW + S(24);
    lay.bw = std::max(MaxTextW({ IDC_PREVIEW, IDC_CLIPBOARD, IDC_CURSOR, IDC_MIC, IDC_SYS }) + S(12) + S(44),
                      TextW(Tr(L"Atraso")) + TextW(Tr(L"Desligado")) + S(48));
    lay.capD = S(84);
    lay.capW = lay.capD + S(8);
    lay.capX = lay.bx + lay.bw + S(18);
    lay.clientW = lay.capX + lay.capW + S(12);
    lay.contentH = lay.pad * 2 + 3 * lay.rowP + lay.ch;               // 4 rows
    lay.barH = S(40);
    g.caption.Layout(lay.clientW, g.dpi);
    lay.capH = g.caption.Height();
}

int PresetRows() { return std::clamp((int)g.store.presets.size(), 2, 6); }

SIZE ClientSizeWanted() {
    lay.panelH = PresetRows() * S(30) + S(10);
    return { lay.clientW, lay.capH + lay.contentH + lay.barH + (g.presetsOpen ? lay.panelH : 0) };
}

void ResizeToContent() {
    SIZE c = ClientSizeWanted();       // no system frame: window = client area
    SetWindowPos(g.frame, nullptr, 0, 0, c.cx, c.cy, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
}

void Layout() {
    auto mv = [](int id, int x, int y, int w, int h) { MoveWindow(C(id), x, y, w, h, TRUE); };
    bool video = Cur().kind == CaptureKind::Video;
    int ch = lay.ch, lw = lay.labW, fx = lay.fieldX, T = lay.capH;
    auto row = [T](int i) { return T + lay.pad + i * lay.rowP; };
    // vertical tabs
    mv(IDC_TAB_IMAGE, 0, T, lay.tabW, lay.tabH);
    mv(IDC_TAB_VIDEO, 0, T + lay.tabH, lay.tabW, lay.tabH);
    // fields column
    mv(IDC_SEL_LABEL, lay.ax, row(0), lw, ch);
    mv(IDC_SELECTION, fx, row(0), lay.fieldW, ch);
    SendMessageW(C(IDC_SELECTION), CB_SETDROPPEDWIDTH, S(230), 0);
    // image
    mv(IDC_EFFECTS_TITLE, lay.ax, row(1), lw, ch);
    mv(IDC_EFFECTS, fx, row(1), lay.fieldW, ch);
    mv(IDC_SHARE_LABEL, lay.ax, row(2), lw, ch);
    mv(IDC_SHARE, fx, row(2), lay.fieldW, ch);
    // video
    mv(IDC_QUALITY_LABEL, lay.ax, row(1), lw, ch);
    mv(IDC_QUALITY, fx, row(1), lay.fieldW, ch);
    mv(IDC_WEBCAM_LABEL, lay.ax, row(2), lw, ch);
    mv(IDC_WEBCAM, fx, row(2), TextW(Cur().webcam ? Tr(L"Ligada") : Tr(L"Desligada")) + S(26), ch);
    // options column
    mv(IDC_PREVIEW, lay.bx, row(0), lay.bw, ch);
    if (video) {
        mv(IDC_CURSOR, lay.bx, row(1), lay.bw, ch);
        mv(IDC_MIC, lay.bx, row(2), lay.bw, ch);
        mv(IDC_SYS, lay.bx, row(3), lay.bw, ch);
    } else {
        mv(IDC_CLIPBOARD, lay.bx, row(1), lay.bw, ch);
        mv(IDC_CURSOR, lay.bx, row(2), lay.bw, ch);
        mv(IDC_DELAY, lay.bx, row(3), lay.bw, ch);
    }
    // round capture button, centered in the content strip
    mv(IDC_CAPTURE, lay.capX, T + (lay.contentH - lay.capW) / 2, lay.capW, lay.capW);
    // bottom bar: ⚙ Presets  +   …   Open Editor
    int by = T + lay.contentH + (lay.barH - ch) / 2;
    int pw = TextW(IDC_PRESETS_LABEL) + S(16) + S(6) + S(16);
    mv(IDC_PRESETS_LABEL, S(8), by, pw, ch);
    mv(IDC_PRESET_ADD, S(8) + pw, by, TextW(L"+", ui::UiFont(g.dpi, 16)) + S(24), ch);
    int ew = TextW(IDC_OPEN_EDITOR) + S(16) + S(6) + S(16);
    mv(IDC_OPEN_EDITOR, lay.clientW - S(8) - ew, by, ew, ch);
    // preset list (expanded): full-width rows
    int py = T + lay.contentH + lay.barH + S(5);
    ClientSizeWanted();
    mv(IDC_PRESETS, 0, py, lay.clientW, lay.panelH - S(10));
    SendMessageW(C(IDC_PRESETS), LB_SETITEMHEIGHT, 0, S(30));
    ShowWindow(C(IDC_PRESETS), g.presetsOpen ? SW_SHOW : SW_HIDE);
}

void ShowPresetList(bool open) {
    if (g.presetsOpen == open) return;
    g.presetsOpen = open;
    RegSetDword(L"CapturePresetsOpen", g.presetsOpen);
    bool l = g.loading; g.loading = true;
    Layout();
    ResizeToContent();
    g.loading = l;
    if (g.presetsOpen) ListBox_SetCurSel(C(IDC_PRESETS), g.store.selected);     // scrolls to the current preset
}

HMENU BuildMenu() {
    HMENU bar = CreateMenu();
    HMENU file = CreatePopupMenu();
    AppendMenuW(file, MF_STRING, IDM_OPEN_EDITOR, Tr(L"Abrir o &Editor"));
    AppendMenuW(file, MF_STRING, IDM_LIBRARY_FOLDER, Tr(L"Abrir a pasta da &biblioteca"));
    AppendMenuW(file, MF_STRING, IDM_LOGS, Tr(L"Abrir a pasta de &logs"));
    AppendMenuW(file, MF_SEPARATOR, 0, nullptr);
    AppendMenuW(file, MF_STRING, IDM_PRESET_IMPORT, Tr(L"&Importar presets…"));
    AppendMenuW(file, MF_STRING, IDM_PRESET_EXPORT, Tr(L"E&xportar presets…"));
    AppendMenuW(file, MF_SEPARATOR, 0, nullptr);
    AppendMenuW(file, MF_STRING, IDM_SETTINGS_EXPORT, Tr(L"Exportar &todas as configurações…"));
    AppendMenuW(file, MF_STRING, IDM_SETTINGS_IMPORT, Tr(L"Importar t&odas as configurações…"));
    AppendMenuW(file, MF_STRING, IDM_SETTINGS_BACKUPS, Tr(L"Abrir a pasta de &backups"));
    AppendMenuW(file, MF_SEPARATOR, 0, nullptr);
    AppendMenuW(file, MF_STRING, IDM_OPTIONS, Tr(L"&Opções…"));
    AppendMenuW(file, MF_SEPARATOR, 0, nullptr);
    AppendMenuW(file, MF_STRING, IDM_EXIT, Tr(L"&Sair do Snapnab"));
    HMENU cap = CreatePopupMenu();
    AppendMenuW(cap, MF_STRING, IDM_CAPTURE_IMAGE, Tr(L"Capturar &imagem"));
    AppendMenuW(cap, MF_STRING, IDM_CAPTURE_VIDEO, Tr(L"Gravar &vídeo"));
    AppendMenuW(cap, MF_STRING, IDM_REPEAT, Tr(L"&Repetir a última captura"));
    AppendMenuW(cap, MF_SEPARATOR, 0, nullptr);
    AppendMenuW(cap, MF_STRING, IDM_CAPTURE_CLIPBOARD, Tr(L"Da área de &transferência"));
    AppendMenuW(cap, MF_STRING, IDM_CAPTURE_FILE, Tr(L"De um &arquivo gráfico…"));
    HMENU view = CreatePopupMenu();
    AppendMenuW(view, MF_STRING, IDM_ONECLICK, Tr(L"Mostrar o &One Click"));
    AppendMenuW(view, MF_STRING, IDM_SHOW_PRESETS, Tr(L"Lista de &presets"));
    HMENU help = CreatePopupMenu();
    AppendMenuW(help, MF_STRING, IDM_MANUAL, Tr(L"&Manual do Snapnab\tF1"));
    AppendMenuW(help, MF_STRING, IDM_PRESET_HELP, Tr(L"Como usar os &presets"));
    AppendMenuW(help, MF_SEPARATOR, 0, nullptr);
    AppendMenuW(help, MF_STRING, IDM_ABOUT, Tr(L"&Sobre o Snapnab"));
    AppendMenuW(bar, MF_POPUP, (UINT_PTR)file, Tr(kMenuNames[0]));
    AppendMenuW(bar, MF_POPUP, (UINT_PTR)cap, Tr(kMenuNames[1]));
    AppendMenuW(bar, MF_POPUP, (UINT_PTR)view, Tr(kMenuNames[2]));
    AppendMenuW(bar, MF_POPUP, (UINT_PTR)help, Tr(kMenuNames[3]));
    return bar;
}

void TrayMenu() {
    HMENU m = CreatePopupMenu();
    AppendMenuW(m, MF_STRING, IDM_CAPTURE_IMAGE, Tr(L"Capturar imagem"));
    AppendMenuW(m, MF_STRING, IDM_CAPTURE_VIDEO, Tr(L"Gravar vídeo"));
    HMENU pre = CreatePopupMenu();
    PresetsSubmenu(pre);
    AppendMenuW(m, MF_POPUP, (UINT_PTR)pre, Tr(L"Presets"));
    AppendMenuW(m, MF_SEPARATOR, 0, nullptr);
    AppendMenuW(m, MF_STRING, IDM_SHOW, Tr(L"Abrir a janela de captura"));
    AppendMenuW(m, MF_STRING | (g.oneClick.enabled() ? MF_CHECKED : 0), IDM_ONECLICK, Tr(L"Mostrar o One Click"));
    AppendMenuW(m, MF_STRING, IDM_OPEN_EDITOR, Tr(L"Abrir o Editor"));
    AppendMenuW(m, MF_STRING, IDM_OPTIONS, Tr(L"Opções…"));
    AppendMenuW(m, MF_SEPARATOR, 0, nullptr);
    AppendMenuW(m, MF_STRING, IDM_EXIT, Tr(L"Sair"));
    SetMenuDefaultItem(m, IDM_SHOW, FALSE);
    POINT p; GetCursorPos(&p);
    SetForegroundWindow(g.frame);
    TrackPopupMenu(m, TPM_RIGHTBUTTON, p.x, p.y, 0, g.frame, nullptr);
    PostMessageW(g.frame, WM_NULL, 0, 0);
    DestroyMenu(m);
}

// drop-down menu of a "Label  Value ›" control (opens below it, aligned right or left)
int PopupBelow(HWND ctl, HMENU m, bool rightAlign = true) {
    RECT r; GetWindowRect(ctl, &r);
    TPMPARAMS tp{ sizeof tp, r };
    return (int)TrackPopupMenuEx(m, TPM_RETURNCMD | (rightAlign ? TPM_RIGHTALIGN : TPM_LEFTALIGN) | TPM_TOPALIGN | TPM_VERTICAL,
                                 rightAlign ? r.right : r.left, r.bottom, g.frame, &tp);
}

void DelayMenu() {
    HMENU m = CreatePopupMenu();
    int cur = Cur().delaySeconds;
    for (int s : { 0, 1, 2, 3, 5, 10, 15, 30, 60 })
        AppendMenuW(m, MF_STRING | (s == cur ? MF_CHECKED : 0), IDM_DELAY_BASE + s,
                    s == 0 ? Tr(L"Desligado") : s == 1 ? Tr(L"1 segundo") : Format(Tr(L"%d segundos"), s).c_str());
    int id = PopupBelow(C(IDC_DELAY), m);
    DestroyMenu(m);
    if (id < IDM_DELAY_BASE) return;
    Cur().delaySeconds = id - IDM_DELAY_BASE;
    g.store.Save();
    LoadControls();
}

void EditChromaKey() {
    auto& p = Cur();
    std::vector<ui::Field> f = {
        ui::Field::MakeCheck(Tr(L"Remover o fundo da webcam"), p.chromaKey),
        ui::Field::MakeColor(Tr(L"Cor do fundo"), p.chromaColor),
        ui::Field::MakeInt(Tr(L"Tolerância (0–100)"), p.chromaTolerance, 0, 100),
        ui::Field::MakeLabel(Tr(L"Use um fundo liso e bem iluminado (tela verde é o ideal).")),
        ui::Field::MakeLabel(Tr(L"Com o fundo removido, a webcam aparece sem moldura.")),
    };
    if (!ui::RunForm(g.frame, Tr(L"Remover o fundo (chroma key)"), f)) return;
    p.chromaKey = f[0].value != 0;
    p.chromaColor = f[1].color;
    p.chromaTolerance = f[2].value;
    if (p.chromaKey) p.webcam = true;
    g.store.Save();
    LoadControls();
    Layout();
}

void WebcamMenu() {
    HCURSOR old = SetCursor(LoadCursor(nullptr, IDC_WAIT));
    g.cams = ListCameras();
    SetCursor(old);
    const Preset& p = Cur();
    HMENU m = CreatePopupMenu();
    AppendMenuW(m, MF_STRING | (!p.webcam ? MF_CHECKED : 0), IDM_WEBCAM_OFF, Tr(L"Desligada"));
    AppendMenuW(m, MF_SEPARATOR, 0, nullptr);
    if (g.cams.empty()) AppendMenuW(m, MF_STRING | MF_GRAYED, 0, Tr(L"Nenhuma webcam encontrada"));
    for (size_t i = 0; i < g.cams.size(); i++) {
        bool on = p.webcam && (p.webcamDevice == g.cams[i].link || (p.webcamDevice.empty() && i == 0));
        AppendMenuW(m, MF_STRING | (on ? MF_CHECKED : 0), IDM_WEBCAM_BASE + i, g.cams[i].name.c_str());
    }
    // webcam shape and background removal
    AppendMenuW(m, MF_SEPARATOR, 0, nullptr);
    HMENU shapes = CreatePopupMenu();
    for (int s = 0; s < (int)CamShape::Count; s++)
        AppendMenuW(shapes, MF_STRING | (s == p.webcamShape ? MF_CHECKED : 0) | (p.chromaKey ? MF_GRAYED : 0), IDM_WEBCAM_SHAPE + s,
                    CamShapeName((CamShape)s));
    AppendMenuW(m, MF_POPUP | (p.chromaKey ? MF_GRAYED : 0), (UINT_PTR)shapes, Tr(L"Formato"));
    AppendMenuW(m, MF_STRING | (p.chromaKey ? MF_CHECKED : 0), IDM_WEBCAM_CHROMA, Tr(L"Remover o fundo (chroma key)…"));
    int id = PopupBelow(C(IDC_WEBCAM), m, false);
    DestroyMenu(m);
    if (id >= IDM_WEBCAM_SHAPE && id < IDM_WEBCAM_SHAPE + (int)CamShape::Count) {
        Cur().webcamShape = id - IDM_WEBCAM_SHAPE;
        g.store.Save();
        return;
    }
    if (id == IDM_WEBCAM_CHROMA) { EditChromaKey(); return; }
    if (id == IDM_WEBCAM_OFF) Cur().webcam = false;
    else if (id >= IDM_WEBCAM_BASE && id < IDM_WEBCAM_BASE + (int)g.cams.size()) {
        auto& c = g.cams[id - IDM_WEBCAM_BASE];
        Cur().webcam = true; Cur().webcamDevice = c.link; Cur().webcamName = c.name;
    } else return;
    g.store.Save();
    LoadControls();
    Layout();
}

void AddPresetMenu() {
    HMENU m = CreatePopupMenu();
    AppendMenuW(m, MF_STRING, IDM_PRESET_FROM_CURRENT, Tr(L"Novo preset com as configurações atuais"));
    AppendMenuW(m, MF_STRING, IDM_PRESET_NEW, Tr(L"Novo preset…"));
    AppendMenuW(m, MF_SEPARATOR, 0, nullptr);
    AppendMenuW(m, MF_STRING, IDM_PRESET_HELP, Tr(L"Ajuda"));
    RECT r; GetWindowRect(C(IDC_PRESET_ADD), &r);
    TPMPARAMS tp{ sizeof tp, r };
    TrackPopupMenuEx(m, TPM_LEFTALIGN | TPM_TOPALIGN | TPM_VERTICAL, r.left, r.bottom, g.frame, &tp);
    DestroyMenu(m);
}

void PresetRowMenu(int item, POINT pt) {
    HMENU m = CreatePopupMenu();
    AppendMenuW(m, MF_STRING, IDM_PRESET_RUN, Tr(L"Capturar com este preset"));
    AppendMenuW(m, MF_SEPARATOR, 0, nullptr);
    AppendMenuW(m, MF_STRING, IDM_PRESET_RENAME, Tr(L"Renomear…"));
    AppendMenuW(m, MF_STRING, IDM_PRESET_HOTKEY, Tr(L"Atalho…"));
    AppendMenuW(m, MF_STRING, IDM_PRESET_DUP, Tr(L"Duplicar"));
    AppendMenuW(m, MF_SEPARATOR, 0, nullptr);
    AppendMenuW(m, MF_STRING, IDM_PRESET_DEL, Tr(L"Excluir"));
    (void)item;
    TrackPopupMenu(m, TPM_RIGHTALIGN | TPM_TOPALIGN, pt.x, pt.y, 0, g.frame, nullptr);
    DestroyMenu(m);
}

// ---------------------------------------------------------------- preset list
// row: type icon, name, "Add hotkey" (or the hotkey) and gear — like the reference preset list
std::wstring HotkeyLabel(const Preset& p) { return p.hotkeyVk ? HotkeyToText(p.hotkeyMods, p.hotkeyVk) : Tr(L"Adicionar atalho"); }

int PresetZoneAt(int item, int x) {
    if (item < 0 || item >= (int)g.store.presets.size()) return 0;
    RECT r; ListBox_GetItemRect(C(IDC_PRESETS), item, &r);
    if (x >= r.right - S(40)) return 2;
    int tw = TextW(HotkeyLabel(g.store.presets[item]));
    if (x >= r.right - S(44) - tw - S(6)) return 1;
    return 0;
}

void DrawPresetItem(DRAWITEMSTRUCT* di) {
    if ((int)di->itemID < 0 || di->itemID >= g.store.presets.size()) return;
    HDC dc = di->hDC;
    RECT r = di->rcItem;
    FillRect(dc, &r, th.brSide);
    const Preset& p = g.store.presets[di->itemID];
    bool sel = (di->itemState & ODS_SELECTED) != 0;
    bool hotRow = (int)di->itemID == g.presetHot;
    Gdiplus::Graphics gr(dc);
    gr.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
    if (sel || hotRow) {
        COLORREF fill = Mix(th.bgSide, th.accent, sel ? 0.16 : 0.07);
        Gdiplus::SolidBrush b(Gdiplus::Color(255, GetRValue(fill), GetGValue(fill), GetBValue(fill)));
        gr.FillRectangle(&b, (INT)r.left, (INT)r.top, (INT)(r.right - r.left), (INT)(r.bottom - r.top));
    }
    if (sel) {
        COLORREF a = Luma(th.accent) > 0.55 ? th.accentDark : th.accent;
        Gdiplus::SolidBrush ab(Gdiplus::Color(255, GetRValue(a), GetGValue(a), GetBValue(a)));
        float ih = (float)S(16);
        gr.FillRectangle(&ab, (float)r.left, r.top + ((r.bottom - r.top) - ih) / 2, (float)S(3), ih);
    }
    int is = S(16), cy = (r.top + r.bottom) / 2;
    RECT ir{ r.left + S(16), cy - is / 2, r.left + S(16) + is, cy - is / 2 + is };
    icons::DrawOnDC(dc, p.kind == CaptureKind::Video ? icons::VideoFill : icons::CameraFill, ir, sel ? th.accentText : RGB(84, 84, 92));
    // gear and hotkey on the right
    int zone = hotRow ? g.presetZone : 0;
    RECT gearR{ r.right - S(40), r.top, r.right - S(10), r.bottom };
    if (zone == 2) {
        RECT hr = gearR; InflateRect(&hr, -S(2), -S(3));
        ui::FillRoundRect(dc, hr, S(6), Mix(th.bgSide, th.accent, 0.20), Mix(th.bgSide, th.accent, 0.20));
    }
    RECT gi{ (gearR.left + gearR.right - is) / 2, cy - is / 2, (gearR.left + gearR.right - is) / 2 + is, cy - is / 2 + is };
    icons::DrawOnDC(dc, icons::Gear, gi, zone == 2 ? th.accentText : RGB(96, 96, 104));
    std::wstring hk = HotkeyLabel(p);
    HGDIOBJ of = SelectObject(dc, ui::UiFont(g.dpi));
    SetBkMode(dc, TRANSPARENT);
    RECT hr{ r.left, r.top, r.right - S(44), r.bottom };
    SetTextColor(dc, zone == 1 ? th.accentText : p.hotkeyVk ? RGB(70, 70, 78) : RGB(128, 128, 136));
    DrawTextW(dc, hk.c_str(), -1, &hr, DT_RIGHT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
    hr.right -= TextW(hk) + S(14);
    RECT tr{ ir.right + S(12), r.top, hr.right, r.bottom };
    SetTextColor(dc, RGB(28, 28, 32));
    DrawTextW(dc, Tr(p.name).c_str(), -1, &tr, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS);
    SelectObject(dc, of);
    if ((di->itemState & ODS_FOCUS) && !(di->itemState & ODS_NOFOCUSRECT)) { RECT f = r; InflateRect(&f, -S(2), -S(1)); DrawFocusRect(dc, &f); }
}

LRESULT CALLBACK PresetListProc(HWND h, UINT m, WPARAM w, LPARAM l, UINT_PTR, DWORD_PTR) {
    switch (m) {
    case WM_MOUSEMOVE: {
        DWORD r = (DWORD)SendMessageW(h, LB_ITEMFROMPOINT, 0, l);
        int item = HIWORD(r) ? -1 : (int)LOWORD(r);
        int zone = PresetZoneAt(item, GET_X_LPARAM(l));
        if (item != g.presetHot || zone != g.presetZone) {
            int old = g.presetHot;
            g.presetHot = item; g.presetZone = zone;
            RECT ir;
            if (old >= 0 && ListBox_GetItemRect(h, old, &ir) != LB_ERR) InvalidateRect(h, &ir, FALSE);
            if (item >= 0 && ListBox_GetItemRect(h, item, &ir) != LB_ERR) InvalidateRect(h, &ir, FALSE);
        }
        TRACKMOUSEEVENT t{ sizeof t, TME_LEAVE, h, 0 };
        TrackMouseEvent(&t);
        break;
    }
    case WM_MOUSELEAVE:
        g.presetHot = -1; g.presetZone = 0;
        InvalidateRect(h, nullptr, FALSE);
        break;
    case WM_SETCURSOR:
        if (g.presetZone) { SetCursor(LoadCursor(nullptr, IDC_HAND)); return TRUE; }
        break;
    case WM_LBUTTONUP: {
        LRESULT res = DefSubclassProc(h, m, w, l);
        DWORD r = (DWORD)SendMessageW(h, LB_ITEMFROMPOINT, 0, l);
        int item = HIWORD(r) ? -1 : (int)LOWORD(r);
        int zone = PresetZoneAt(item, GET_X_LPARAM(l));
        if (item >= 0 && zone) PostMessageW(g.frame, WM_PRESET_ZONE, item, zone);
        return res;
    }
    case WM_LBUTTONDBLCLK: {
        DWORD r = (DWORD)SendMessageW(h, LB_ITEMFROMPOINT, 0, l);
        int item = HIWORD(r) ? -1 : (int)LOWORD(r);
        if (PresetZoneAt(item, GET_X_LPARAM(l))) return 0;   // a double click on the hotkey/gear does not capture
        break;
    }
    }
    return DefSubclassProc(h, m, w, l);
}

void SelectPreset(int i) {
    if (i < 0 || i >= (int)g.store.presets.size()) return;
    g.store.selected = i;
    g.store.Save();
    ListBox_SetCurSel(C(IDC_PRESETS), i);
    LoadControls();
    bool l = g.loading; g.loading = true; Layout(); g.loading = l;
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
        if (active) {   // active tab indicator, as in Windows 11 navigation
            int ih = S(22);
            RECT ind{ r.left + S(3), (r.top + r.bottom - ih) / 2, r.left + S(6), (r.top + r.bottom + ih) / 2 };
            ui::FillRoundRect(dc, ind, S(3), th.accent, th.accent);
        }
        COLORREF fg = active ? th.accentText : RGB(92, 92, 100);
        int is = S(22), cx = (r.left + r.right) / 2;
        RECT ir{ cx - is / 2, r.top + S(11), cx + is / 2, r.top + S(11) + is };
        icons::DrawOnDC(dc, di->CtlID == IDC_TAB_VIDEO ? icons::VideoFill : icons::CameraFill, ir, fg);
        RECT tr{ r.left, ir.bottom + S(3), r.right, r.bottom - S(6) };
        ui::DrawTextCentered(dc, di->CtlID == IDC_TAB_VIDEO ? Tr(L"Vídeo") : Tr(L"Imagem"), tr, active ? th.accentText : RGB(70, 70, 78),
                             ui::UiFont(g.dpi, 9, active));
        if (di->itemState & ODS_FOCUS) { RECT f = r; InflateRect(&f, -2, -2); DrawFocusRect(dc, &f); }
        return;
    }
    FillRect(dc, &r, th.brMain);
    if (di->CtlID == IDC_CAPTURE) {
        int d = lay.capD;
        RECT c{ r.left + (r.right - r.left - d) / 2, r.top + (r.bottom - r.top - d) / 2, 0, 0 };
        c.right = c.left + d; c.bottom = c.top + d;
        bool pressed = (di->itemState & ODS_SELECTED) != 0;
        COLORREF col = kCaptureRed;
        if (g.hoverCapture) col = Mix(col, RGB(255, 255, 255), 0.10);
        if (pressed) col = Mix(kCaptureRed, 0, 0.16);
        Gdiplus::Graphics gr(dc);
        gr.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
        gr.SetPixelOffsetMode(Gdiplus::PixelOffsetModeHalf);
        for (int i = 3; i >= 1; i--) {                 // soft shadow
            Gdiplus::SolidBrush sh(Gdiplus::Color((BYTE)(16 * (4 - i)), 0, 0, 0));
            gr.FillEllipse(&sh, (float)c.left - i + 1, (float)c.top - i + 2.5f + i * 0.4f, (float)d + 2 * i - 2, (float)d + 2 * i - 2);
        }
        Gdiplus::SolidBrush b(Gdiplus::Color(255, GetRValue(col), GetGValue(col), GetBValue(col)));
        gr.FillEllipse(&b, (float)c.left, (float)c.top, (float)d, (float)d);
        ui::DrawTextCentered(dc, video ? Tr(L"Gravar") : Tr(L"Capturar"), c, RGB(255, 255, 255), ui::UiFont(g.dpi, 10, true));
        if (di->itemState & ODS_FOCUS) { RECT f = r; InflateRect(&f, -1, -1); DrawFocusRect(dc, &f); }
    }
}

void OnCommand(int id, int code) {
    if (id >= IDM_PRESET_BASE && id < IDM_PRESET_BASE + (int)g.store.presets.size()) {
        StartCapture(g.store.presets[id - IDM_PRESET_BASE]); return;
    }
    switch (id) {
    case IDC_PRESETS:
        if (code == LBN_SELCHANGE) SelectPreset(ListBox_GetCurSel(C(IDC_PRESETS)));
        else if (code == LBN_DBLCLK) StartCapture(Cur());
        break;
    case IDC_PRESET_ADD: AddPresetMenu(); break;
    case IDM_PRESET_FROM_CURRENT: { Preset p = Cur(); p.name = Cur().name + Tr(L" (cópia)"); AddPreset(p, Tr(L"Novo preset com as configurações atuais")); break; }
    case IDM_PRESET_NEW: {
        Preset p; p.name = Format(Tr(L"Preset %zu"), g.store.presets.size() + 1);
        p.kind = Cur().kind; if (p.kind == CaptureKind::Video) p.captureCursor = true;
        AddPreset(p, Tr(L"Novo preset"));
        break;
    }
    case IDM_PRESET_HELP: PresetHelp(); break;
    case IDM_PRESET_RUN: StartCapture(Cur()); break;
    case IDM_PRESET_DUP: {
        Preset p = Cur(); p.name += Tr(L" (cópia)"); p.hotkeyMods = p.hotkeyVk = 0; p.builtin.clear();
        g.store.presets.push_back(p);
        g.store.selected = (int)g.store.presets.size() - 1;
        g.store.Save(); FillPresetList(); LoadControls(); Layout(); ResizeToContent();
        break;
    }
    case IDM_PRESET_DEL:
        if (g.store.presets.size() <= 1) { MessageBoxW(g.frame, Tr(L"É preciso manter pelo menos um preset."), L"Snapnab", MB_ICONINFORMATION); break; }
        if (MessageBoxW(g.frame, Format(Tr(L"Excluir o preset \"%s\"?"), Tr(Cur().name).c_str()).c_str(), L"Snapnab", MB_YESNO | MB_ICONQUESTION) != IDYES) break;
        g.store.presets.erase(g.store.presets.begin() + g.store.selected);
        g.store.selected = std::max(0, g.store.selected - 1);
        g.store.Save(); RegisterHotkeys(); FillPresetList(); LoadControls(); Layout(); ResizeToContent();
        break;
    case IDC_PRESETS_LABEL: case IDM_SHOW_PRESETS: ShowPresetList(!g.presetsOpen); break;
    case IDM_PRESET_RENAME: RenamePreset(); break;
    case IDM_PRESET_HOTKEY: EditPresetHotkey(); break;
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
    case IDC_SELECTION:
        if (code == CBN_SELCHANGE) SaveControls();
        else if (code == mui::MUI_GEARCLICK) RegionMenu();
        break;
    case IDC_EFFECTS:
        if (code == CBN_DROPDOWN) FillEffectsList();
        else if (code == CBN_SELCHANGE) {
            int i = ComboBox_GetCurSel(C(IDC_EFFECTS));
            if (i >= 0) OnEffectPicked(ComboBox_GetItemData(C(IDC_EFFECTS), i));
        } else if (code == CBN_CLOSEUP) g.fxAddMode = false;
        break;
    case IDC_SHARE:
        if (code == CBN_SELCHANGE) {
            SetShare(Cur(), (Share)std::max(0, ComboBox_GetCurSel(C(IDC_SHARE))));
            g.store.Save();
            LoadControls();
        } else if (code == mui::MUI_GEARCLICK) {
            Share s = ShareOf(Cur());
            if (s == ShareFile) EditFileOutput();
            else if (s == ShareProgram) EditProgramOutput();
        }
        break;
    case IDC_QUALITY:
        if (code == CBN_SELCHANGE) SaveControls();
        else if (code == mui::MUI_GEARCLICK) EditVideoOptions();
        break;
    case IDC_DELAY: DelayMenu(); break;
    case IDC_WEBCAM: WebcamMenu(); break;
    case IDC_PREVIEW: case IDC_CLIPBOARD: case IDC_CURSOR: case IDC_MIC: case IDC_SYS:
        if (code == BN_CLICKED) SaveControls();
        break;
    case IDC_CAPTURE: StartCapture(Cur()); break;
    case IDC_OPEN_EDITOR: case IDM_OPEN_EDITOR: OpenEditor(); break;
    case IDM_OPTIONS: ShowOptions(); break;
    case IDM_ONECLICK: SetOneClick(!g.oneClick.enabled()); break;
    case IDM_CAPTURE_IMAGE: StartCapture(FirstOfKind(CaptureKind::Image)); break;
    case IDM_CAPTURE_VIDEO: StartCapture(FirstOfKind(CaptureKind::Video)); break;
    case IDM_REPEAT: if (g.haveLast) StartCapture(g.lastPreset); else StartCapture(Cur()); break;
    case IDM_CAPTURE_CLIPBOARD: { Preset p = FirstOfKind(CaptureKind::Image); p.selection = Selection::Clipboard; p.delaySeconds = 0; StartCapture(p); break; }
    case IDM_CAPTURE_FILE: { Preset p = FirstOfKind(CaptureKind::Image); p.selection = Selection::File; p.delaySeconds = 0; StartCapture(p); break; }
    case IDM_PRESET_IMPORT: {
        std::wstring f = Tr(L"Presets do Snapnab (*.snappresets;*.xml)|*.snappresets;*.xml|");
        std::replace(f.begin(), f.end(), L'|', L'\0');
        std::wstring path;
        if (ui::OpenFileDialog(g.frame, f, path)) {
            if (g.store.Import(path)) { FillPresetList(); Layout(); ResizeToContent(); TrayNotify(L"Snapnab", Tr(L"Presets importados.")); }
            else MessageBoxW(g.frame, Tr(L"Arquivo de presets inválido."), L"Snapnab", MB_ICONWARNING);
        }
        break;
    }
    case IDM_PRESET_EXPORT: {
        std::wstring f = Tr(L"Presets do Snapnab (*.snappresets)|*.snappresets|");
        std::replace(f.begin(), f.end(), L'|', L'\0');
        std::wstring path = L"Presets.snappresets"; int fi = 1;
        if (ui::SaveFileDialog(g.frame, f, path, fi, L"snappresets") && !g.store.Export(path))
            MessageBoxW(g.frame, Tr(L"Não foi possível exportar."), L"Snapnab", MB_ICONWARNING);
        break;
    }
    case IDM_LIBRARY_FOLDER: ShellExecuteW(nullptr, L"open", LibraryDir().c_str(), nullptr, nullptr, SW_SHOWNORMAL); break;
    case IDM_SETTINGS_EXPORT: ExportSettings(); break;
    case IDM_SETTINGS_IMPORT: ImportSettings(); break;
    case IDM_SETTINGS_BACKUPS: ShellExecuteW(nullptr, L"open", settings::BackupDir().c_str(), nullptr, nullptr, SW_SHOWNORMAL); break;
    case IDM_LOGS: ShellExecuteW(nullptr, L"open", LogsDir().c_str(), nullptr, nullptr, SW_SHOWNORMAL); break;
    case IDM_ABOUT: About(); break;
    case IDM_MANUAL: OpenManual(g.frame, L"capture-window"); break;
    case IDM_SHOW: ShowFrame(); break;
    case IDM_EXIT:
        if (g.engine.recorder.state() != Recorder::Idle) {
            if (MessageBoxW(g.frame, Tr(L"Há uma gravação em andamento. Parar e sair?"), L"Snapnab", MB_YESNO | MB_ICONQUESTION) != IDYES) break;
            g.engine.VideoStop();
        }
        DestroyWindow(g.frame);
        break;
    }
}

void PaintBands(HDC dc) {
    RECT cr; GetClientRect(g.frame, &cr);
    int T = lay.capH;
    g.caption.Paint(dc);
    RECT main{ 0, T, cr.right, cr.bottom };
    FillRect(dc, &main, th.brMain);
    RECT tabs{ 0, T, lay.tabW, T + lay.contentH };
    FillRect(dc, &tabs, th.brSide);
    RECT bar{ 0, T + lay.contentH, cr.right, T + lay.contentH + lay.barH };
    FillRect(dc, &bar, th.brBar);
    RECT panel{ 0, T + lay.contentH + lay.barH, cr.right, cr.bottom };
    if (g.presetsOpen) FillRect(dc, &panel, th.brSide);
    HBRUSH ln = CreateSolidBrush(th.line);
    RECT l1{ 0, T + lay.contentH, cr.right, T + lay.contentH + 1 };
    FillRect(dc, &l1, ln);
    if (g.presetsOpen) { RECT l2{ 0, panel.top, cr.right, panel.top + 1 }; FillRect(dc, &l2, ln); }
    DeleteObject(ln);
}

LRESULT CALLBACK FrameProc(HWND h, UINT m, WPARAM w, LPARAM l) {
    if (m == g.taskbarCreated && m) { TrayAdd(); return 0; }
    switch (m) {
    case WM_CREATE: {
        g.frame = h;
        g.dpi = DpiForWindow(h);
        g.presetsOpen = RegGetDword(L"CapturePresetsOpen", 0) != 0;
        g.caption.Init(h, { { Tr(kMenuNames[0]), i18n::AltKey(Tr(kMenuNames[0]), L'A'), GetSubMenu(g.menuBar, 0) },
                            { Tr(kMenuNames[1]), i18n::AltKey(Tr(kMenuNames[1]), L'C'), GetSubMenu(g.menuBar, 1) },
                            { Tr(kMenuNames[2]), i18n::AltKey(Tr(kMenuNames[2]), L'E'), GetSubMenu(g.menuBar, 2) },
                            { Tr(kMenuNames[3]), i18n::AltKey(Tr(kMenuNames[3]), L'J'), GetSubMenu(g.menuBar, 3) } }, false, false);
        g.caption.onInitMenu = [](HMENU mm, int) {
            CheckMenuItem(mm, IDM_ONECLICK, g.oneClick.enabled() ? MF_CHECKED : MF_UNCHECKED);
            CheckMenuItem(mm, IDM_SHOW_PRESETS, g.presetsOpen ? MF_CHECKED : MF_UNCHECKED);
        };
        LoadTheme();
        g.loading = true;
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
    case WM_NCCALCSIZE: { LRESULT r; if (g.caption.NcCalcSize(w, l, r)) return r; break; }   // the title bar is ours
    case WM_NCACTIVATE: return DefWindowProcW(h, m, w, -1);
    case WM_NCHITTEST: { LRESULT r; if (g.caption.NcHitTest(l, r)) return r; break; }
    case WM_NCRBUTTONUP: if (g.caption.RButtonUp(w, l)) return 0; break;
    case WM_MOUSEMOVE: g.caption.MouseMove({ GET_X_LPARAM(l), GET_Y_LPARAM(l) }); return 0;
    case WM_MOUSELEAVE: g.caption.MouseLeave(); return 0;
    case WM_LBUTTONDOWN: g.caption.LButtonDown({ GET_X_LPARAM(l), GET_Y_LPARAM(l) }); return 0;
    case WM_LBUTTONUP: g.caption.LButtonUp({ GET_X_LPARAM(l), GET_Y_LPARAM(l) }); return 0;
    case WM_SIZE: { bool l2 = g.loading; g.loading = true; Layout(); g.loading = l2; InvalidateRect(h, nullptr, TRUE); return 0; }
    case WM_DPICHANGED: {
        g.dpi = HIWORD(w);
        RECT* r = (RECT*)l;
        SetFonts();
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
    case WM_ERASEBKGND: PaintBands((HDC)w); return 1;
    case WM_CTLCOLORSTATIC: case WM_CTLCOLORBTN: {
        // background according to the window strip the control is in (content, bottom bar or preset list)
        HWND c = (HWND)l;
        SetBkMode((HDC)w, TRANSPARENT);
        RECT rc; GetWindowRect(c, &rc);
        MapWindowPoints(nullptr, h, (POINT*)&rc, 2);
        SetTextColor((HDC)w, RGB(28, 28, 32));
        int T = lay.capH;
        if (rc.top >= T + lay.contentH + lay.barH) return (LRESULT)th.brSide;
        if (rc.top >= T + lay.contentH) return (LRESULT)th.brBar;
        return (LRESULT)th.brMain;
    }
    case WM_DRAWITEM: DrawOwner((DRAWITEMSTRUCT*)l); return TRUE;
    case WM_MEASUREITEM: ((MEASUREITEMSTRUCT*)l)->itemHeight = S(30); return TRUE;
    case WM_CTLCOLORLISTBOX: return (LRESULT)th.brSide;
    case WM_NOTIFY: {
        auto* nm = (NMTTDISPINFOW*)l;
        if (nm->hdr.hwndFrom == g.tip && nm->hdr.code == TTN_GETDISPINFOW) {
            static std::wstring t;
            bool video = Cur().kind == CaptureKind::Video;
            t = (video ? Tr(L"Gravar vídeo  (") : Tr(L"Capturar imagem  (")) + HotkeyFor(video) + L")";
            nm->lpszText = t.data();
            return 0;
        }
        break;
    }
    case WM_PRESET_ZONE: {
        int item = (int)w;
        if (item != g.store.selected) SelectPreset(item);
        if (l == 1) EditPresetHotkey();
        else {
            RECT r; ListBox_GetItemRect(C(IDC_PRESETS), item, &r);
            POINT pt{ r.right - S(12), r.bottom };
            ClientToScreen(C(IDC_PRESETS), &pt);
            PresetRowMenu(item, pt);
        }
        return 0;
    }
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
        if (w == kTrayClickTimer) { KillTimer(h, kTrayClickTimer); StartCapture(Cur()); return 0; }
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
        // a click captures, a double click opens the window: the single click waits for the double-click time before
        // capturing, and the second click of a double click is ignored (before, both fired, alternating the result)
        switch (LOWORD(l)) {
        case WM_LBUTTONUP:
            if (g.trayIgnoreUp) { g.trayIgnoreUp = false; break; }
            SetTimer(h, kTrayClickTimer, GetDoubleClickTime(), nullptr);
            break;
        case WM_LBUTTONDBLCLK:
            KillTimer(h, kTrayClickTimer);
            g.trayIgnoreUp = true;
            ShowFrame();
            break;
        case NIN_KEYSELECT: StartCapture(Cur()); break;
        case WM_RBUTTONUP: case WM_CONTEXTMENU: KillTimer(h, kTrayClickTimer); TrayMenu(); break;
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
    case WM_CLOSE: ShowWindow(h, SW_HIDE); return 0;     // stays in the tray
    case WM_ENDSESSION: if (w) DestroyWindow(h); return 0;
    case WM_DESTROY:
        RemoveClipboardFormatListener(h);
        Shell_NotifyIconW(NIM_DELETE, &g.nid);
        g.hk.Clear();
        g.oneClick.Show(false);
        g.ipc.SendInt(ipc::kEditor, ipc::CloseEditorIfHidden, 0);
        PostQuitMessage(0);
        return 0;
    }
    return DefWindowProcW(h, m, w, l);
}

} // namespace

int WINAPI wWinMain(HINSTANCE hi, HINSTANCE, LPWSTR, int) {
    SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    // single instance: mutex + "SnapnabUI" window; the second instance forwards the command line (WM_COPYDATA)
    WaitForPreviousInstance(kCaptureWndClass);
    if (ForwardToExistingInstance(kCaptureMutex, kCaptureWndClass, &g.mutex)) return 0;
    CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
    INITCOMMONCONTROLSEX icc{ sizeof(icc), ICC_STANDARD_CLASSES | ICC_UPDOWN_CLASS | ICC_HOTKEY_CLASS | ICC_LISTVIEW_CLASSES };
    InitCommonControlsEx(&icc);
    GdiplusSession gdip;
    mui::Register(hi);
    LogInit(L"Snapnab");
    i18n::Init();
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
    g.menuBar = BuildMenu();
    // fixed size, computed from the content in WM_CREATE (ResizeToContent); the window draws its own title bar
    HWND frame = CreateWindowExW(0, kCaptureWndClass, L"Snapnab",
                                 WS_POPUP | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN,
                                 CW_USEDEFAULT, CW_USEDEFAULT, 700, 260, nullptr, nullptr, hi, nullptr);
    BOOL dark = FALSE;
    DwmSetWindowAttribute(frame, 20 /*DWMWA_USE_IMMERSIVE_DARK_MODE*/, &dark, sizeof dark);
    DWORD corner = 2;   // DWMWCP_ROUND
    DwmSetWindowAttribute(frame, 33 /*DWMWA_WINDOW_CORNER_PREFERENCE*/, &corner, sizeof corner);
    // initial position: center of the primary screen (CW_USEDEFAULT does not work for WS_POPUP)
    {
        RECT wr; GetWindowRect(frame, &wr);
        RECT wa = MonitorRectFromPoint({ 0, 0 }, true);
        SetWindowPos(frame, nullptr, wa.left + (wa.right - wa.left - (wr.right - wr.left)) / 2,
                     wa.top + (wa.bottom - wa.top - (wr.bottom - wr.top)) / 3, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
    }
    TrayAdd();
    g.hk.Init(frame);
    RegisterHotkeys();
    AddClipboardFormatListener(frame);
    g.lastClipSeq = GetClipboardSequenceNumber();

    // automatic backup of the settings, once a week (the last 5 are kept)
    std::wstring backup = settings::AutoBackup();
    if (!backup.empty()) Log(L"Settings backup: %s", backup.c_str());
    g.engine.Init(frame, &g.ipc);
    g.engine.notify = TrayNotify;
    g.engine.onPresetVolumes = [](const Preset& used) {       // volumes changed in the recording panel
        for (auto& p : g.store.presets)
            if (p.name == used.name && p.kind == used.kind) { p.micVolume = used.micVolume; p.sysVolume = used.sysVolume; p.autoGain = used.autoGain; }
        g.store.Save();
    };
    g.engine.onBusyChanged = [](bool busy) {
        SetVideoHotkeys(busy && g.lastPreset.kind == CaptureKind::Video);
        g.oneClick.SetBusy(busy);
    };
    g.oneClick.onCapture = [] { StartCapture(Cur()); };
    g.oneClick.onMenu = OneClickMenu;
    if (RegGetDword(L"OneClickEnabled", 0)) g.oneClick.Show(true);
    // IPC channel: this process is channel 0
    g.ipc.Start(ipc::kCapture, [](const ipc::Message& m) {
        PostMessageW(g.frame, WM_SNAP_IPC, 0, (LPARAM)new ipc::Message(m));
    });
    ProcessCommandLine(GetCommandLineW(), true);

    MSG msg;
    while (GetMessageW(&msg, nullptr, 0, 0)) {
        if (g.caption.PreTranslate(msg)) continue;
        // F1 in the capture window opens the manual at its chapter
        if (msg.message == WM_KEYDOWN && msg.wParam == VK_F1 && g.frame && (msg.hwnd == g.frame || IsChild(g.frame, msg.hwnd))) {
            OpenManual(g.frame, L"capture-window");
            continue;
        }
        if (g.frame && IsDialogMessageW(g.frame, &msg)) continue;
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
    g.ipc.Stop();
    Log(L"Snapnab exited");
    if (g.mutex) { ReleaseMutex(g.mutex); CloseHandle(g.mutex); }
    CoUninitialize();
    return 0;
}
