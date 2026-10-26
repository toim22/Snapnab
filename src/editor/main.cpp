// SnapnabEditor.exe — image and video editor (≙ SnagitEditor.exe, ds_arch §19 / ed_comp_1..15)
#include "Common.h"
#include "Canvas.h"
#include "Panels.h"
#include "QuickStyles.h"
#include "Popups.h"
#include "Video.h"
#include "Effects.h"
#include "Ipc.h"
#include "Library.h"
#include "Ocr.h"
#include "CaptionBar.h"
#include "Theme.h"
#include "Ui.h"
#include "Widgets.h"
#include <commdlg.h>
#include <dwmapi.h>
#include <mapi.h>
#include <cmath>

using namespace snap;
using namespace snap::ed;

namespace {

enum : int {
    // bar
    T_CAPTURE = 1, T_RECORD, T_COPYALL, T_SAVE, T_SHARE, T_TOOL_BASE = 100,
    // menu
    M_NEW_CAPTURE = 1000, M_NEW_VIDEO, M_NEW_CLIPBOARD, M_NEW_IMAGE, M_OPEN, M_SAVE, M_SAVEAS, M_DUPLICATE, M_DELETE, M_PRINT,
    M_EMAIL, M_OPEN_PROGRAM, M_LIBRARY, M_CLOSE, M_EXIT,
    M_UNDO, M_REDO, M_UNDOALL, M_CUT, M_COPY, M_COPYALL, M_PASTE, M_DELETE_SEL, M_SELECTALL, M_FLATTEN, M_FRONT, M_BACK, M_OCR,
    M_CROP_SEL, M_ROT_CW, M_ROT_CCW, M_ROT_180, M_ROT_FREE, M_FLIP_H, M_FLIP_V, M_RESIZE, M_CANVAS, M_CANVAS_COLOR, M_TRIM,
    M_FX_BORDER, M_FX_SHADOW, M_FX_EDGE, M_FX_GRAY, M_FX_INVERT, M_FX_SEPIA, M_FX_BC, M_FX_HS, M_FX_REPLACE, M_FX_WATERMARK, M_FX_CAPTION,
    M_FX_MONO,
    M_ZOOM_IN, M_ZOOM_OUT, M_ZOOM_100, M_ZOOM_FIT, M_BG_COLOR, M_SHOW_TRAY, M_ABOUT, M_SHORTCUTS, M_SHOW_EFFECTS,
    // panel effects that are not in the menu (Editor-only CEffectSettings*, ed_comp_5 §4.3)
    M_FX_PERSPECTIVE, M_FX_PAGECURL, M_FX_FILTER, M_FX_ADJUST, M_FX_SPOTLIGHT, M_SHOW_LIBRARY,
    // Image menu as in the reference: Arrange (order, align, flip), group, flatten, template, snapping, tags
    M_BRING_FORWARD, M_SEND_BACKWARD, M_ALIGN_LEFT, M_ALIGN_CENTER, M_ALIGN_RIGHT, M_ALIGN_TOP, M_ALIGN_MIDDLE, M_ALIGN_BOTTOM,
    M_GROUP, M_UNGROUP, M_FLATTEN_SEL, M_APPLY_TEMPLATE, M_SNAPPING, M_TAG,
    // distribute, paste in place, lock, lifted piece, rulers, edge snapping, plain-text paste, layers
    M_DIST_H, M_DIST_V, M_PASTE_INPLACE, M_LOCK, M_UNLOCK, M_LIFT, M_DROP, M_RULERS, M_EDGESNAP, M_PLAINPASTE, M_RENAME_OBJ,
    M_SHOW_LAYERS, M_CLEAR_GUIDES, M_OPEN_LINK, M_MANUAL,
    M_TB_CUSTOMIZE = 1500, M_TB_RESET, M_MORE_TOOL_BASE = 1600,
};

// quick effect (CInteropQuickEffect, ed_comp_5 §4.4): border + edge effect + shadow + page curl in a single step
struct QuickEffect {
    std::wstring name;
    bool border = false; int borderW = 4; COLORREF borderColor = RGB(45, 45, 48);
    bool edge = false; int edgeStyle = 0, edgeSize = 14, edgeSides = fx::Bottom | fx::Right;
    bool shadow = false; int shadowOffset = 6, shadowBlur = 6, shadowOpacity = 50;
    bool curl = false; int curlSize = 18 /*% of the shorter side*/, curlCorner = 2;
};

constexpr UINT WM_LIB_CHANGED = WM_APP + 0x60;
constexpr UINT_PTR kFxPreviewTimer = 0x61;      // rebuilds the effect previews after a pause in the edits

struct App {
    HINSTANCE hi = nullptr;
    HWND frame = nullptr;
    HANDLE mutex = nullptr;
    ipc::Notify ipc;
    Toolbar toolbar;
    PropsPanel props;
    EffectsPanel effects;
    LayersPanel layers;
    Canvas canvas;
    VideoView video;
    LibraryTray tray;              // recent captures (at the bottom)
    LibraryTray library;           // view of the whole library (in place of the image)
    bool libraryOpen = false;
    ui::CaptionBar caption;        // custom title bar (icon + menus), the same as the capture window's
    ui::ThemeColors th;
    HMENU menuBar = nullptr;
    int rightTab = 0;              // right panel: 0 Properties, 1 Effects, 2 Layers
    // toolbar: automatic (fixed + 4 most recently used + Magnifier) or custom (any item)
    bool tbCustom = false;
    std::vector<ToolbarEntry> tbCustomList;
    std::vector<Tool> tbRecent;
    int tbToolCount = 10;          // tools on the automatic bar (6 to 16): 5 fixed + recent + Magnifier
    std::vector<QuickEffect> quick;
    std::vector<std::unique_ptr<Document>> docs;
    Document* cur = nullptr;
    std::wstring status;
    std::wstring coords;           // cursor position in image pixels (with the rulers on)
    bool showTray = true;
    HACCEL accel = nullptr;
    RECT footRecent{}, footSize{}, footZoom{};   // footer: recent captures, image size and zoom (clickable)
    HANDLE watchStop = nullptr;
    std::thread watcher;
    int dpi = 96;
} g;

int S(int v) { return ui::Scale(v, g.dpi); }

// ---------------------------------------------------------------- title / status
void UpdateTitle() {
    std::wstring t = L"Snapnab Editor";
    if (g.cur) t = (g.cur->modified ? L"• " : L"") + g.cur->title + Tr(L" — Snapnab Editor");
    SetWindowTextW(g.frame, t.c_str());                                          // taskbar
    g.caption.SetTitle(g.cur ? (g.cur->modified ? L"• " : L"") + g.cur->title : L"");   // on the title bar, only the document
}

void ScheduleEffectPreviews() { if (g.frame) SetTimer(g.frame, kFxPreviewTimer, 350, nullptr); }

void InvalidateStatus() {
    RECT cr; GetClientRect(g.frame, &cr);
    RECT sr{ 0, cr.bottom - S(26), cr.right, cr.bottom };
    InvalidateRect(g.frame, &sr, FALSE);
}

void SetStatus(const std::wstring& s) { g.status = s; InvalidateStatus(); }

void UpdateDocInfo() {
    if (!g.cur || g.cur->isVideo) { g.props.SetDocInfo(g.cur ? Tr(L"Vídeo: ") + FileNameOf(g.cur->path) : L""); return; }
    std::wstring info = Format(Tr(L"%d × %d px"), g.cur->w(), g.cur->h());
    if (!g.cur->captureMode.empty()) info += Tr(L"\nCaptura: ") + g.cur->captureMode;
    if (!g.cur->application.empty()) info += Tr(L"\nAplicativo: ") + g.cur->application;
    if (!g.cur->windowTitle.empty()) info += Tr(L"\nJanela: ") + g.cur->windowTitle;
    info += Format(Tr(L"\nObjetos: %zu"), g.cur->st.objs.size());
    if (!g.cur->tags.empty()) info += Tr(L"\nEtiquetas: ") + g.cur->tags;
    g.props.SetDocInfo(info);
}

// ---------------------------------------------------------------- layout
// right column: "Properties | Effects" tabs above the chosen panel
int PropW() { return S(296); }
int TabsH() { return S(44); }

RECT TabsRect() {
    RECT cr; GetClientRect(g.frame, &cr);
    int y0 = g.caption.Height() + g.toolbar.Height();
    return { cr.right - PropW(), y0, cr.right, y0 + TabsH() };
}

RECT TabRect(int i) {
    RECT t = TabsRect();
    int pad = S(10), w = (t.right - t.left - 2 * pad) / 3;
    return { t.left + pad + i * w, t.top + S(8), t.left + pad + (i + 1) * w, t.bottom - S(6) };
}

void Layout() {
    if (!g.frame) return;
    RECT cr; GetClientRect(g.frame, &cr);
    g.caption.Layout(cr.right, g.dpi);
    int capH = g.caption.Height();
    int tbH = g.toolbar.Height(), stH = S(26), trayH = g.showTray ? g.tray.Height() : 0, propW = PropW();
    MoveWindow(g.toolbar.hwnd, 0, capH, cr.right, tbH, TRUE);
    int y0 = capH + tbH, y1 = cr.bottom - stH - trayH;
    bool video = g.cur && g.cur->isVideo;
    int py = y0 + TabsH();
    MoveWindow(g.props.hwnd, cr.right - propW, py, propW, std::max(0, y1 - py), TRUE);
    MoveWindow(g.effects.hwnd, cr.right - propW, py, propW, std::max(0, y1 - py), TRUE);
    MoveWindow(g.layers.hwnd, cr.right - propW, py, propW, std::max(0, y1 - py), TRUE);
    MoveWindow(g.canvas.hwnd, 0, y0, cr.right - (video ? 0 : propW), std::max(0, y1 - y0), TRUE);
    MoveWindow(g.video.hwnd, 0, y0, cr.right, std::max(0, y1 - y0), TRUE);
    MoveWindow(g.library.hwnd, 0, y0, cr.right, std::max(0, y1 - y0), TRUE);
    bool lib = g.libraryOpen;        // the library takes the place of the image and the panels
    ShowWindow(g.library.hwnd, lib ? SW_SHOW : SW_HIDE);
    ShowWindow(g.canvas.hwnd, video || lib ? SW_HIDE : SW_SHOW);
    ShowWindow(g.props.hwnd, video || lib || g.rightTab != 0 ? SW_HIDE : SW_SHOW);
    ShowWindow(g.effects.hwnd, video || lib || g.rightTab != 1 ? SW_HIDE : SW_SHOW);
    ShowWindow(g.layers.hwnd, video || lib || g.rightTab != 2 ? SW_HIDE : SW_SHOW);
    ShowWindow(g.video.hwnd, video && !lib ? SW_SHOW : SW_HIDE);
    ShowWindow(g.tray.hwnd, g.showTray ? SW_SHOW : SW_HIDE);
    if (g.showTray) MoveWindow(g.tray.hwnd, 0, y1, cr.right, trayH, TRUE);
    InvalidateRect(g.frame, nullptr, FALSE);
}

void SetRightTab(int t) {
    if (g.rightTab == t) return;
    if (g.rightTab == 1 && g.effects.IsOpen()) g.effects.Collapse();     // leaving the effects cancels the preview
    g.rightTab = t;
    RegSetDword(L"RightPanel", t, L"Editor");
    if (t == 1) ScheduleEffectPreviews();
    if (t == 2) g.layers.Refresh();
    Layout();
}

// View › Library (CLibraryView): all captures in place of the image
void ShowLibrary(bool on) {
    if (g.libraryOpen == on) return;
    g.libraryOpen = on;
    if (on) { g.library.Reload(); g.library.SetCurrent(g.library.current); SetFocus(g.library.hwnd); }
    Layout();
}

// ---------------------------------------------------------------- documents
void Autosave(Document* d) {
    if (!d || d->isVideo || !d->modified || d->path.empty()) return;
    if (d->Save()) {
        WriteThumbnail(d->path, d->Render());
        g.tray.InvalidateThumb(d->path);
        g.library.InvalidateThumb(d->path);
        Log(L"Autosave: %s", d->path.c_str());
    }
}

static bool GuidFromLibraryPath(const std::wstring& path, GUID& out) {
    std::wstring name = StripExt(FileNameOf(path));
    return SUCCEEDED(CLSIDFromString((L"{" + name + L"}").c_str(), &out));
}

void Activate(Document* d) {
    if (g.cur == d) { if (d) ShowLibrary(false); return; }
    g.canvas.CommitText();
    if (g.effects.IsOpen()) g.effects.Collapse();            // the effect preview belonged to the previous document
    if (g.cur) {
        Autosave(g.cur);
        GUID gd; if (GuidFromLibraryPath(g.cur->path, gd)) g.ipc.Send(ipc::kCapture, ipc::LibraryDocReleased, ipc::kBlob, &gd, 16);
    }
    g.cur = d;
    if (d && d->isVideo) { g.canvas.SetDocument(nullptr); g.video.Open(d->path); }
    else { g.video.Close(); g.canvas.SetDocument(d); }
    if (d) {
        GUID gd; if (GuidFromLibraryPath(d->path, gd)) g.ipc.Send(ipc::kCapture, ipc::LibraryDocOpened, ipc::kBlob, &gd, 16);
        g.tray.SetCurrent(d->path);
        g.library.current = d->path;
        g.libraryOpen = false;                               // opening a document goes back from the library to the image
    }
    Layout();
    UpdateTitle();
    UpdateDocInfo();
    g.props.Load();
    g.layers.Refresh();
    ScheduleEffectPreviews();
}

Document* FindDoc(const std::wstring& path) {
    for (auto& d : g.docs) if (!_wcsicmp(d->path.c_str(), path.c_str())) return d.get();
    return nullptr;
}

Document* AddDoc(std::unique_ptr<Document> d) {
    // keeps up to 12 documents in memory (the others stay saved in the library)
    if (g.docs.size() >= 12) {
        for (auto it = g.docs.begin(); it != g.docs.end(); ++it)
            if (it->get() != g.cur) { Autosave(it->get()); g.docs.erase(it); break; }
    }
    g.docs.push_back(std::move(d));
    return g.docs.back().get();
}

// a new image becomes a .snap document in the library (as the original imports into the DataStore)
Document* NewDocFromImage(Image img, const std::wstring& title, const std::wstring& mode) {
    auto d = Document::FromImage(std::move(img), title);
    d->captureMode = mode;
    d->path = LibraryDir() + L"\\" + NewGuidString() + L".snap";
    d->Save();
    WriteThumbnail(d->path, *d->st.base);
    Document* p = AddDoc(std::move(d));
    g.tray.Reload();
    Activate(p);
    return p;
}

bool OpenPath(const std::wstring& pathIn) {
    std::wstring path = pathIn;
    wchar_t full[MAX_PATH * 2];
    if (GetFullPathNameW(pathIn.c_str(), MAX_PATH * 2, full, nullptr)) path = full;
    if (Document* d = FindDoc(path)) { Activate(d); return true; }
    if (!FileExists(path)) { MessageBoxW(g.frame, (Tr(L"Arquivo não encontrado:\n") + path).c_str(), L"Snapnab Editor", MB_ICONWARNING); return false; }
    std::wstring ext = FileExt(path);
    auto d = Document::Load(path);
    if (!d) { MessageBoxW(g.frame, (Tr(L"Não foi possível abrir:\n") + path).c_str(), L"Snapnab Editor", MB_ICONWARNING); return false; }
    if (ext != L".snap" && ext != L".mp4") {
        // regular image: imported into the library
        Image img = *d->st.base;
        NewDocFromImage(std::move(img), FileNameOf(path), Tr(L"Arquivo importado"));
        return true;
    }
    Activate(AddDoc(std::move(d)));
    return true;
}

void CloseCurrent() {
    if (!g.cur) return;
    g.canvas.CommitText();
    Autosave(g.cur);
    Document* old = g.cur;
    Activate(nullptr);
    g.docs.erase(std::remove_if(g.docs.begin(), g.docs.end(), [&](auto& d) { return d.get() == old; }), g.docs.end());
    if (!g.docs.empty()) Activate(g.docs.back().get());
}

void DocChanged() {
    UpdateTitle();
    UpdateDocInfo();
    ScheduleEffectPreviews();
    if (g.rightTab == 2) g.layers.Refresh();
    if (g.cur) SetStatus(Format(Tr(L"%d × %d px  •  %zu objeto(s)  •  %zu passo(s) para desfazer"), g.cur->w(), g.cur->h(),
                                g.cur->st.objs.size(), g.cur->UndoDepth()));
}

// ---------------------------------------------------------------- image operations
void TransformObjects(std::function<Gdiplus::PointF(Gdiplus::PointF)> map) {
    for (size_t i = 0; i < g.cur->st.objs.size(); i++) {
        DrawObj* o = g.cur->Mutable(i);
        o->MapPoints(map);                         // includes the arrows' curve and the objects inside groups
        o->UpdateGroupRect();
    }
}

bool RequireImageDoc() {
    if (!g.cur || g.cur->isVideo) { MessageBeep(MB_ICONWARNING); return false; }
    g.canvas.CommitText();
    return true;
}

void Geometry(int cmd) {
    if (!RequireImageDoc()) return;
    Document* d = g.cur;
    const Image& b = *d->st.base;
    float W = (float)b.w, H = (float)b.h;
    d->Checkpoint();
    switch (cmd) {
    case M_ROT_CW: d->SetBase(fx::Rotate90(b, true)); TransformObjects([H](Gdiplus::PointF p) { return Gdiplus::PointF(H - p.Y, p.X); }); break;
    case M_ROT_CCW: d->SetBase(fx::Rotate90(b, false)); TransformObjects([W](Gdiplus::PointF p) { return Gdiplus::PointF(p.Y, W - p.X); }); break;
    case M_ROT_180: d->SetBase(fx::Rotate180(b)); TransformObjects([W, H](Gdiplus::PointF p) { return Gdiplus::PointF(W - p.X, H - p.Y); }); break;
    case M_FLIP_H: d->SetBase(fx::FlipH(b)); TransformObjects([W](Gdiplus::PointF p) { return Gdiplus::PointF(W - p.X, p.Y); }); break;
    case M_FLIP_V: d->SetBase(fx::FlipV(b)); TransformObjects([H](Gdiplus::PointF p) { return Gdiplus::PointF(p.X, H - p.Y); }); break;
    }
    g.canvas.FitToWindow();
    g.canvas.Refresh();
    DocChanged();
}

void FlattenIfNeeded() {
    if (!g.cur->st.objs.empty()) { g.cur->SetBase(g.cur->Render()); g.cur->st.objs.clear(); g.canvas.selected = -1; }
}

// Image › Effects and the toolbar effect items: open the effect settings in the panel, with live preview
void OpenEffect(int cmd) {
    if (!RequireImageDoc()) return;
    int target = cmd, filter = -1;
    switch (cmd) {
    case M_FX_GRAY: target = M_FX_FILTER; filter = 0; break;
    case M_FX_SEPIA: target = M_FX_FILTER; filter = 1; break;
    case M_FX_INVERT: target = M_FX_FILTER; filter = 2; break;
    case M_FX_MONO: target = M_FX_FILTER; filter = 3; break;
    case M_FX_BC: case M_FX_HS: target = M_FX_ADJUST; break;
    }
    if (filter >= 0)
        for (auto& e : g.effects.effects) if (e.cmd == M_FX_FILTER && !e.params.empty()) e.params[0].value = filter;
    SetRightTab(1);
    g.effects.Expand(target);
}

void ImageCommand(int cmd) {
    if (!RequireImageDoc()) return;
    Document* d = g.cur;
    switch (cmd) {
    case M_ROT_CW: case M_ROT_CCW: case M_ROT_180: case M_FLIP_H: case M_FLIP_V: Geometry(cmd); return;
    case M_CROP_SEL:
        if (g.canvas.HasRasterSelection() || g.canvas.HasCrop()) g.canvas.CropToSelection();
        else MessageBoxW(g.frame, Tr(L"Faça uma seleção (ferramenta Seleção) ou um recorte primeiro."), L"Snapnab Editor", MB_ICONINFORMATION);
        return;
    case M_ROT_FREE: {
        std::vector<ui::Field> f = { ui::Field::MakeInt(Tr(L"Ângulo (graus, horário)"), 15, -359, 359), ui::Field::MakeColor(Tr(L"Cor de fundo"), RGB(255, 255, 255)),
                                     ui::Field::MakeCheck(Tr(L"Fundo transparente"), true),
                                     ui::Field::MakeLabel(Tr(L"Os objetos serão mesclados à imagem.")) };
        if (!ui::RunForm(g.frame, Tr(L"Girar"), f)) return;
        d->Checkpoint(); FlattenIfNeeded();
        d->SetBase(fx::RotateAngle(*d->st.base, f[0].value, f[2].value ? 0 : FromColorref(f[1].color)));
        break;
    }
    case M_RESIZE: {
        std::vector<ui::Field> f = { ui::Field::MakeInt(Tr(L"Largura (px)"), d->w(), 1, 20000), ui::Field::MakeInt(Tr(L"Altura (px)"), d->h(), 1, 20000),
                                     ui::Field::MakeInt(Tr(L"ou escala (%)"), 100, 1, 1000), ui::Field::MakeCheck(Tr(L"Manter proporção"), true) };
        if (!ui::RunForm(g.frame, Tr(L"Redimensionar imagem"), f)) return;
        int nw = f[0].value, nh = f[1].value;
        if (f[2].value != 100) { nw = std::max(1, d->w() * f[2].value / 100); nh = std::max(1, d->h() * f[2].value / 100); }
        else if (f[3].value) {
            if (nw != d->w()) nh = std::max(1, (int)std::lround((double)d->h() * nw / d->w()));
            else if (nh != d->h()) nw = std::max(1, (int)std::lround((double)d->w() * nh / d->h()));
        }
        float sx = (float)nw / d->w(), sy = (float)nh / d->h();
        d->Checkpoint();
        d->SetBase(ScaleImage(*d->st.base, nw, nh));
        TransformObjects([sx, sy](Gdiplus::PointF p) { return Gdiplus::PointF(p.X * sx, p.Y * sy); });
        float s = std::sqrt(sx * sy);
        for (size_t i = 0; i < d->st.objs.size(); i++) { DrawObj* o = d->Mutable(i); o->style.width *= s; o->style.fontSize = std::max(6.0f, o->style.fontSize * s); }
        break;
    }
    case M_CANVAS: {
        std::vector<ui::Field> f = { ui::Field::MakeInt(Tr(L"Largura (px)"), d->w(), 1, 20000), ui::Field::MakeInt(Tr(L"Altura (px)"), d->h(), 1, 20000),
                                     ui::Field::MakeCombo(Tr(L"Âncora"), { Tr(L"Superior esquerda"), Tr(L"Superior"), Tr(L"Superior direita"), Tr(L"Esquerda"), Tr(L"Centro"),
                                                                       Tr(L"Direita"), Tr(L"Inferior esquerda"), Tr(L"Inferior"), Tr(L"Inferior direita") }, 4),
                                     ui::Field::MakeColor(Tr(L"Cor do canvas"), RGB(255, 255, 255)), ui::Field::MakeCheck(Tr(L"Transparente"), false) };
        if (!ui::RunForm(g.frame, Tr(L"Tamanho do canvas"), f)) return;
        int nw = f[0].value, nh = f[1].value, a = f[2].value;
        int ox = a % 3 == 0 ? 0 : a % 3 == 1 ? (nw - d->w()) / 2 : nw - d->w();
        int oy = a / 3 == 0 ? 0 : a / 3 == 1 ? (nh - d->h()) / 2 : nh - d->h();
        d->Checkpoint();
        d->SetBase(fx::CanvasResize(*d->st.base, nw, nh, a, f[4].value ? 0 : FromColorref(f[3].color)));
        TransformObjects([ox, oy](Gdiplus::PointF p) { return Gdiplus::PointF(p.X + ox, p.Y + oy); });
        break;
    }
    case M_CANVAS_COLOR: {                           // color or gradient behind the transparent areas of the image
        static uint32_t col = 0xFFFFFFFF;
        static Gradient grad;
        POINT at; GetCursorPos(&at);
        if (!ColorPopup(g.frame, at, col, false, QS().Theme().colors, &grad)) return;
        d->Checkpoint();
        Image& b = d->MutableBase();
        Image out(b.w, b.h, grad.Active() ? 0u : (col | 0xFF000000));
        if (grad.Active()) FillGradientMask(out, {}, { 0, 0, out.w, out.h }, grad, 100);
        out.Blit(b, 0, 0, true);
        d->SetBase(std::move(out));
        break;
    }
    case M_TRIM: {
        Image t = fx::AutoTrim(*d->st.base);
        if (t.w == d->w() && t.h == d->h()) { SetStatus(Tr(L"Nada a cortar: as bordas não são uniformes.")); return; }
        // finds the crop offset to move the objects
        int ox = 0, oy = 0;
        const Image& b = *d->st.base;
        uint32_t ref = b.at(0, 0);
        auto rowU = [&](int y) { for (int x = 0; x < b.w; x++) { uint32_t p = b.at(x, y); if (std::abs(CR(p) - CR(ref)) > 8 || std::abs(CG(p) - CG(ref)) > 8 || std::abs(CB(p) - CB(ref)) > 8 || std::abs(CA(p) - CA(ref)) > 8) return false; } return true; };
        while (oy < b.h - 1 && rowU(oy)) oy++;
        auto colU = [&](int x) { for (int y = oy; y < oy + t.h; y++) { uint32_t p = b.at(x, y); if (std::abs(CR(p) - CR(ref)) > 8 || std::abs(CG(p) - CG(ref)) > 8 || std::abs(CB(p) - CB(ref)) > 8 || std::abs(CA(p) - CA(ref)) > 8) return false; } return true; };
        while (ox < b.w - 1 && colU(ox)) ox++;
        d->Checkpoint();
        d->SetBase(std::move(t));
        TransformObjects([ox, oy](Gdiplus::PointF p) { return Gdiplus::PointF(p.X - ox, p.Y - oy); });
        break;
    }
    case M_FX_BORDER: case M_FX_SHADOW: case M_FX_EDGE: case M_FX_GRAY: case M_FX_INVERT: case M_FX_SEPIA: case M_FX_MONO:
    case M_FX_BC: case M_FX_HS: case M_FX_REPLACE: case M_FX_WATERMARK: case M_FX_CAPTION: case M_FX_PERSPECTIVE:
    case M_FX_PAGECURL: case M_FX_FILTER: case M_FX_ADJUST: case M_FX_SPOTLIGHT:
        OpenEffect(cmd);
        return;
    default: return;
    }
    g.canvas.selected = std::min(g.canvas.selected, (int)d->st.objs.size() - 1);
    g.canvas.FitToWindow();
    g.canvas.Refresh();
    DocChanged();
    g.props.Load();
}

// ---------------------------------------------------------------- file
bool SaveAs() {
    if (!RequireImageDoc()) return false;
    std::wstring base = g.cur->title;
    for (auto& c : base) if (wcschr(L"\\/:*?\"<>|", c)) c = L'_';
    if (FileExt(base).size() > 1 && FileExt(base).size() <= 5) base = StripExt(base);
    std::wstring path = DefaultSaveDir() + L"\\" + (base.empty() ? Tr(L"Captura") : base);
    static int fi = 1;
    if (!ui::SaveFileDialog(g.frame, ImageSaveFilter(), path, fi, L"png", Tr(L"Salvar como"))) return false;
    static const wchar_t* exts[] = { L".png", L".jpg", L".bmp", L".gif", L".tif", L".pdf", L".snap" };
    std::wstring ext = FileExt(path);
    bool known = false;
    for (auto e : exts) if (ext == e || (ext == L".jpeg" && !wcscmp(e, L".jpg")) || (ext == L".tiff" && !wcscmp(e, L".tif"))) known = true;
    if (!known && fi >= 1 && fi <= 7) path += exts[fi - 1];
    int q = 90;
    if (FileExt(path) == L".jpg" || FileExt(path) == L".jpeg") {
        std::vector<ui::Field> f = { ui::Field::MakeInt(Tr(L"Qualidade JPEG (1–100)"), 90, 1, 100) };
        if (!ui::RunForm(g.frame, L"JPEG", f)) return false;
        q = f[0].value;
    }
    bool ok;
    if (FileExt(path) == L".snap") {
        // .snap copy outside the library: writes it without replacing the current document
        std::wstring old = g.cur->path; bool mod = g.cur->modified;
        g.cur->path = path; ok = g.cur->Save();
        g.cur->path = old; g.cur->modified = mod;
    } else ok = g.cur->SaveAs(path, q);
    if (!ok) { MessageBoxW(g.frame, (Tr(L"Não foi possível salvar:\n") + path).c_str(), L"Snapnab Editor", MB_ICONWARNING); return false; }
    SetStatus(Tr(L"Salvo em ") + path);
    // a library capture without a name takes the name used in "Save as" (shown in the tray and library)
    if (IsInLibrary(g.cur->path) && CaptureName(g.cur->path).empty()) {
        std::wstring name = StripExt(FileNameOf(path));
        SetCaptureName(g.cur->path, name);
        g.cur->title = name;
        UpdateTitle();
        g.tray.Reload();
        if (g.libraryOpen) g.library.Reload();
    }
    return true;
}

void SaveCurrent() {
    if (!RequireImageDoc()) return;
    if (g.cur->path.empty()) { SaveAs(); return; }
    if (g.cur->Save()) {
        WriteThumbnail(g.cur->path, g.cur->Render());
        g.tray.InvalidateThumb(g.cur->path);
        g.library.InvalidateThumb(g.cur->path);
        SetStatus(Tr(L"Salvo na biblioteca: ") + FileNameOf(g.cur->path));
        UpdateTitle();
    }
}

void NewFromClipboard() {
    Image img;
    if (!GetClipboardImage(img)) { SetStatus(Tr(L"A área de transferência não contém imagem.")); return; }
    NewDocFromImage(std::move(img), Tr(L"Da área de transferência"), Tr(L"Área de transferência"));
    SetStatus(Tr(L"Imagem nova criada a partir da área de transferência."));
}

void NewBlankImage() {
    std::vector<ui::Field> f = { ui::Field::MakeInt(Tr(L"Largura (px)"), 1280, 1, 20000), ui::Field::MakeInt(Tr(L"Altura (px)"), 720, 1, 20000),
                                 ui::Field::MakeColor(Tr(L"Cor de fundo"), RGB(255, 255, 255)), ui::Field::MakeCheck(Tr(L"Transparente"), false) };
    if (!ui::RunForm(g.frame, Tr(L"Nova imagem"), f)) return;
    NewDocFromImage(Image(f[0].value, f[1].value, f[3].value ? 0 : FromColorref(f[2].color)), Tr(L"Imagem nova"), Tr(L"Nova imagem"));
}

void RequestCapture(int kind) {
    // the Editor does not capture: it asks the capture process (IPC 0x12, ed_comp_14 §3.3)
    if (IsProcessRunning(kCaptureMutex)) g.ipc.SendInt(ipc::kCapture, ipc::EditorStartCapture, kind);
    else LaunchSibling(kCaptureExe, kind ? L"/video" : L"/capture");
}

void DuplicateCurrent() {
    if (!RequireImageDoc()) return;
    auto d = std::make_unique<Document>(*g.cur);
    d->ClearHistory();
    d->path = LibraryDir() + L"\\" + NewGuidString() + L".snap";
    d->title = g.cur->title + Tr(L" (cópia)");
    d->modified = true;
    Autosave(d.get());
    Document* p = AddDoc(std::move(d));
    g.tray.Reload();
    Activate(p);
}

// deletes one or more library items (they go to the Recycle Bin), with a single confirmation
void DeleteDocs(const std::vector<std::wstring>& paths) {
    if (paths.empty()) return;
    std::wstring q = paths.size() == 1 ? Tr(L"Enviar para a Lixeira?\n") + FileNameOf(paths[0])
                                       : Format(Tr(L"Enviar %zu itens da biblioteca para a Lixeira?"), paths.size());
    if (MessageBoxW(g.frame, q.c_str(), L"Snapnab Editor", MB_YESNO | MB_ICONQUESTION) != IDYES) return;
    for (auto& path : paths) {
        if (Document* d = FindDoc(path)) {
            d->modified = false;
            if (d == g.cur) Activate(nullptr);
            g.docs.erase(std::remove_if(g.docs.begin(), g.docs.end(), [&](auto& x) { return x.get() == d; }), g.docs.end());
        }
        DeleteLibraryItem(path, g.frame);
    }
    g.tray.Reload();
    if (g.libraryOpen) g.library.Reload();
    if (!g.cur && !g.docs.empty()) Activate(g.docs.back().get());
    SetStatus(paths.size() == 1 ? Tr(L"Item enviado para a Lixeira.") : Format(Tr(L"%zu itens enviados para a Lixeira."), paths.size()));
}

void DeleteDoc(const std::wstring& path) { DeleteDocs({ path }); }

bool PickFolder(std::wstring& out) {
    IFileOpenDialog* dlg = nullptr;
    if (FAILED(CoCreateInstance(CLSID_FileOpenDialog, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&dlg)))) return false;
    DWORD opt = 0; dlg->GetOptions(&opt);
    dlg->SetOptions(opt | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM);
    dlg->SetTitle(Tr(L"Exportar para a pasta"));
    bool ok = false;
    if (SUCCEEDED(dlg->Show(g.frame))) {
        IShellItem* it = nullptr;
        if (SUCCEEDED(dlg->GetResult(&it))) {
            PWSTR p = nullptr;
            if (SUCCEEDED(it->GetDisplayName(SIGDN_FILESYSPATH, &p))) { out = p; CoTaskMemFree(p); ok = true; }
            it->Release();
        }
    }
    dlg->Release();
    return ok;
}

// exports library items to a folder: images in the chosen format (with the objects applied), videos as MP4
void ExportItems(const std::vector<std::wstring>& paths) {
    if (paths.empty()) return;
    static int fmt = 0;
    std::vector<ui::Field> f = { ui::Field::MakeCombo(Tr(L"Formato das imagens"), { L"PNG", L"JPEG", L"BMP", L"GIF", L"TIFF" }, fmt),
                                 ui::Field::MakeLabel(Tr(L"Vídeos são copiados em MP4.")) };
    if (!ui::RunForm(g.frame, Format(Tr(L"Exportar %zu item(ns)"), paths.size()), f)) return;
    fmt = f[0].value;
    std::wstring dir;
    if (!PickFolder(dir)) return;
    static const wchar_t* exts[] = { L".png", L".jpg", L".bmp", L".gif", L".tif" };
    HCURSOR old = SetCursor(LoadCursor(nullptr, IDC_WAIT));
    int ok = 0, fail = 0;
    for (auto& p : paths) {
        WIN32_FILE_ATTRIBUTE_DATA fa{};
        GetFileAttributesExW(p.c_str(), GetFileExInfoStandard, &fa);
        SYSTEMTIME st, lt; FileTimeToSystemTime(&fa.ftLastWriteTime, &st); SystemTimeToTzSpecificLocalTime(nullptr, &st, &lt);
        std::wstring base = Format(L"Captura_%04d-%02d-%02d_%02d-%02d-%02d", lt.wYear, lt.wMonth, lt.wDay, lt.wHour, lt.wMinute, lt.wSecond);
        bool video = FileExt(p) == L".mp4";
        std::wstring dst = dir + L"\\" + base + (video ? L".mp4" : exts[fmt]);
        for (int n = 2; FileExists(dst); n++) dst = dir + L"\\" + base + Format(L"_%d", n) + (video ? L".mp4" : exts[fmt]);
        bool done = false;
        if (video) done = CopyFileW(p.c_str(), dst.c_str(), TRUE) != 0;
        else if (Document* d = FindDoc(p)) done = SaveImageFile(dst, d->Render());
        else if (auto doc = Document::Load(p)) done = SaveImageFile(dst, doc->Render());
        done ? ok++ : fail++;
    }
    SetCursor(old);
    SetStatus(Format(Tr(L"%d item(ns) exportado(s) para %s"), ok, dir.c_str()) + (fail ? Format(Tr(L" — %d falha(s)"), fail) : L""));
    ShellExecuteW(nullptr, L"open", dir.c_str(), nullptr, nullptr, SW_SHOWNORMAL);
}

void PrintCurrent() {
    if (!RequireImageDoc()) return;
    PRINTDLGW pd{ sizeof(pd) };
    pd.hwndOwner = g.frame;
    pd.Flags = PD_RETURNDC | PD_NOSELECTION | PD_NOPAGENUMS;
    if (!PrintDlgW(&pd)) return;
    Image img(g.cur->w(), g.cur->h(), 0xFFFFFFFF);
    img.Blit(g.cur->Render(), 0, 0, true);
    DOCINFOW di{ sizeof(di) }; di.lpszDocName = g.cur->title.c_str();
    if (StartDocW(pd.hDC, &di) > 0 && StartPage(pd.hDC) > 0) {
        int pw = GetDeviceCaps(pd.hDC, HORZRES), ph = GetDeviceCaps(pd.hDC, VERTRES);
        double s = std::min((double)pw / img.w, (double)ph / img.h);
        int dpiX = GetDeviceCaps(pd.hDC, LOGPIXELSX);
        s = std::min(s, dpiX / 96.0);                    // does not enlarge beyond the actual size
        int w = (int)(img.w * s), h = (int)(img.h * s);
        img.DrawTo(pd.hDC, (pw - w) / 2, (ph - h) / 2, w, h, true);
        EndPage(pd.hDC);
        EndDoc(pd.hDC);
    }
    DeleteDC(pd.hDC);
    if (pd.hDevMode) GlobalFree(pd.hDevMode);
    if (pd.hDevNames) GlobalFree(pd.hDevNames);
}

std::wstring TempExport() {
    wchar_t tmp[MAX_PATH]; GetTempPathW(MAX_PATH, tmp);
    std::wstring base = g.cur->title;
    for (auto& c : base) if (wcschr(L"\\/:*?\"<>|. ", c)) c = L'_';
    std::wstring p = std::wstring(tmp) + (base.empty() ? L"Snapnab" : base.substr(0, 40)) + L".png";
    SaveImageFile(p, g.cur->Render());
    return p;
}

void SendEmail() {
    // E-mail through Simple MAPI (MAPI32, like the original): PNG attachment
    if (!RequireImageDoc()) return;
    std::wstring file = TempExport();
    HMODULE mapi = LoadLibraryW(L"MAPI32.DLL");
    auto send = mapi ? (ULONG(WINAPI*)(LHANDLE, ULONG_PTR, lpMapiMessageW, FLAGS, ULONG))GetProcAddress(mapi, "MAPISendMailW") : nullptr;
    if (!send) {
        if (mapi) FreeLibrary(mapi);
        std::wstring msg = Tr(L"Nenhum cliente de e-mail MAPI está configurado. A imagem foi salva em:\n") + file;
        MessageBoxW(g.frame, msg.c_str(), L"Snapnab Editor", MB_ICONINFORMATION);
        return;
    }
    MapiFileDescW fd{}; fd.nPosition = (ULONG)-1; fd.lpszPathName = (PWSTR)file.c_str();
    std::wstring fname = FileNameOf(file); fd.lpszFileName = (PWSTR)fname.c_str();
    MapiMessageW msg{}; std::wstring subj = Tr(L"Captura: ") + g.cur->title;
    msg.lpszSubject = (PWSTR)subj.c_str(); msg.nFileCount = 1; msg.lpFiles = &fd;
    ULONG r = send(0, (ULONG_PTR)g.frame, &msg, MAPI_LOGON_UI | MAPI_DIALOG, 0);
    FreeLibrary(mapi);
    if (r != SUCCESS_SUCCESS && r != MAPI_USER_ABORT) MessageBoxW(g.frame, Format(Tr(L"Falha ao enviar e-mail (MAPI %u)."), r).c_str(), L"Snapnab Editor", MB_ICONWARNING);
}

void OpenInProgram() {
    if (!RequireImageDoc()) return;
    std::wstring prog = RegGetString(L"ExternalProgram", L"mspaint.exe", L"Editor");
    std::vector<ui::Field> f = { ui::Field::MakeText(Tr(L"Programa"), prog) };
    if (!ui::RunForm(g.frame, Tr(L"Abrir em programa externo"), f)) return;
    RegSetString(L"ExternalProgram", f[0].text, L"Editor");
    std::wstring file = TempExport();
    ShellExecuteW(nullptr, L"open", f[0].text.c_str(), (L"\"" + file + L"\"").c_str(), nullptr, SW_SHOWNORMAL);
}

void GrabText() {
    if (!RequireImageDoc()) return;
    Image img = g.canvas.HasRasterSelection() ? g.canvas.SelectionImage() : g.cur->Render();
    HCURSOR old = SetCursor(LoadCursor(nullptr, IDC_WAIT));
    std::wstring text, err;
    bool ok = RunOcr(img, text, err);
    SetCursor(old);
    if (!ok) { MessageBoxW(g.frame, (Tr(L"OCR indisponível: ") + err).c_str(), Tr(L"Grab Text"), MB_ICONWARNING); return; }
    if (Trim(text).empty()) text = Tr(L"(nenhum texto reconhecido)");
    ui::ShowTextWindow(g.frame, Tr(L"Grab Text — texto reconhecido"), text);
}

// Editor clipboard formats: the copied objects (XML) and the origin of a copied area of the image (paste in place)
UINT ObjectsFormat() { static UINT f = RegisterClipboardFormatW(L"Snapnab.Objects"); return f; }
UINT OriginFormat() { static UINT f = RegisterClipboardFormatW(L"Snapnab.Origin"); return f; }

void SetClipboardText(UINT fmt, const std::string& text) {
    HGLOBAL hg = GlobalAlloc(GMEM_MOVEABLE, text.size() + 1);
    if (!hg) return;
    memcpy(GlobalLock(hg), text.c_str(), text.size() + 1);
    GlobalUnlock(hg);
    if (!SetClipboardData(fmt, hg)) GlobalFree(hg);
}

std::string GetClipboardText(UINT fmt) {
    std::string out;
    if (!IsClipboardFormatAvailable(fmt) || !OpenClipboard(g.frame)) return out;
    if (HANDLE h = GetClipboardData(fmt)) {
        if (const char* p = (const char*)GlobalLock(h)) { out.assign(p, strnlen(p, GlobalSize(h))); GlobalUnlock(h); }
    }
    CloseClipboard();
    return out;
}

std::wstring GetClipboardUnicode() {
    std::wstring out;
    if (!IsClipboardFormatAvailable(CF_UNICODETEXT) || !OpenClipboard(g.frame)) return out;
    if (HANDLE h = GetClipboardData(CF_UNICODETEXT)) {
        if (const wchar_t* p = (const wchar_t*)GlobalLock(h)) { out.assign(p, wcsnlen(p, GlobalSize(h) / 2)); GlobalUnlock(h); }
    }
    CloseClipboard();
    return out;
}

// copy: the image goes to the clipboard as before; the selected objects also go in the Editor format (to be pasted
// in another image, in the same place with Paste in place) and a copied area of the image takes its position along
bool CopyToClipboard(bool cut) {
    if (!RequireImageDoc()) return false;
    std::vector<DrawObj> objs = g.canvas.HasRasterSelection() ? std::vector<DrawObj>{} : g.canvas.SelectedObjects();
    RECT sel = g.canvas.RasterSelectionBounds();
    bool raster = g.canvas.HasRasterSelection();
    if (cut && !objs.empty() && g.canvas.SelectionLocked()) { SetStatus(Tr(L"Objetos bloqueados não são recortados.")); return false; }
    if (!g.canvas.CopySelection(false)) return false;
    if (OpenClipboard(g.frame)) {
        if (!objs.empty()) SetClipboardText(ObjectsFormat(), ObjectsToClip(objs, g.cur->path));
        if (raster) SetClipboardText(OriginFormat(), FormatA("%d,%d", (int)sel.left, (int)sel.top));
        CloseClipboard();
    }
    if (cut) g.canvas.DeleteSelected();
    return true;
}

// source formatting of the clipboard text (when it is kept)
bool ClipboardTextFormat(Style& out) {
    static UINT rtf = RegisterClipboardFormatW(L"Rich Text Format"), html = RegisterClipboardFormatW(L"HTML Format");
    out = g.canvas.styles[Tool::Text];
    std::string r = GetClipboardText(rtf);
    if (!r.empty() && ParseRtfFormat(r, out)) return true;
    std::string h = GetClipboardText(html);
    return !h.empty() && ParseHtmlFormat(h, out);
}

// text paste: without formatting by default (Edit › Paste text without formatting); the alternative paste
// (Ctrl+Shift+V) does the opposite
void PasteTextNow(bool alternative) {
    std::wstring t = GetClipboardUnicode();
    if (t.empty()) { SetStatus(Tr(L"Nada para colar.")); return; }
    bool keep = g.canvas.plainPaste == alternative;
    Style fmt;
    bool has = keep && ClipboardTextFormat(fmt);
    g.canvas.PasteText(t, has ? &fmt : nullptr);
    if (has) g.props.Load();                  // the object took the copied font, size and color
    if (keep && !has) SetStatus(Tr(L"O texto copiado não tem formatação; colado com o estilo do objeto."));
}

void Paste(bool inPlace = false) {
    // objects copied in the Editor (also from another image)
    if (g.cur && !g.cur->isVideo && IsClipboardFormatAvailable(ObjectsFormat())) {
        std::wstring src;
        auto objs = ObjectsFromClip(GetClipboardText(ObjectsFormat()), &src);
        if (!objs.empty()) {
            g.canvas.PasteObjects(objs, inPlace, !_wcsicmp(src.c_str(), g.cur->path.c_str()));
            SetStatus(inPlace ? Tr(L"Colado no mesmo lugar.") : Tr(L"Colado."));
            DocChanged();
            return;
        }
    }
    Image img;
    if (!GetClipboardImage(img)) {
        // file copied in Explorer
        if (OpenClipboard(g.frame)) {
            if (HDROP drop = (HDROP)GetClipboardData(CF_HDROP)) {
                wchar_t f[MAX_PATH];
                if (DragQueryFileW(drop, 0, f, MAX_PATH)) LoadImageFile(f, img);
            }
            CloseClipboard();
        }
    }
    if (img.empty()) {
        if (g.cur && !g.cur->isVideo && IsClipboardFormatAvailable(CF_UNICODETEXT)) { PasteTextNow(inPlace); return; }   // text → Text object
        SetStatus(Tr(L"Nada para colar."));
        return;
    }
    if (!g.cur || g.cur->isVideo) NewDocFromImage(std::move(img), Tr(L"Colado"), Tr(L"Área de transferência"));
    else if (inPlace) {                       // same position as the copied area (or the top left corner)
        int x = 0, y = 0;
        std::string o = GetClipboardText(OriginFormat());
        if (!o.empty()) sscanf_s(o.c_str(), "%d,%d", &x, &y);
        g.canvas.PasteImageAt(img, Gdiplus::PointF((float)x, (float)y));
        DocChanged();
    } else { g.canvas.PasteImage(img); DocChanged(); }
}

// ---------------------------------------------------------------- menus
// 32 bpp icon (premultiplied alpha) for menu items, drawn with the program's vector icons
HBITMAP MenuIcon(icons::Id id) {
    int px = ui::Scale(16, (int)GetDpiForSystem());
    Image img(px, px, 0);
    {
        auto bmp = img.WrapGdip();
        Gdiplus::Graphics gr(bmp.get());
        gr.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
        icons::Draw(gr, id, Gdiplus::RectF(0, 0, (float)px, (float)px), Gdiplus::Color(255, 52, 56, 64));
        gr.Flush(Gdiplus::FlushIntentionSync);
    }
    for (auto& p : img.px) { int a = CA(p); p = ARGB(a, CR(p) * a / 255, CG(p) * a / 255, CB(p) * a / 255); }
    return img.ToHBITMAP();
}

void SetItemIcon(HMENU m, UINT id, icons::Id icon) {
    MENUITEMINFOW mi{ sizeof(mi) };
    mi.fMask = MIIM_BITMAP;
    mi.hbmpItem = MenuIcon(icon);
    SetMenuItemInfoW(m, id, FALSE, &mi);
}

HMENU ToolsMenu();               // Image › Tools (list of the tools, defined along with the toolbar)

HMENU BuildMenu() {
    HMENU bar = CreateMenu();
    auto pop = [](std::initializer_list<std::pair<int, const wchar_t*>> items) {
        HMENU m = CreatePopupMenu();
        for (auto& [id, t] : items) { if (id == 0) AppendMenuW(m, MF_SEPARATOR, 0, nullptr); else AppendMenuW(m, MF_STRING, id, t); }
        return m;
    };
    HMENU file = pop({ { M_NEW_CAPTURE, Tr(L"Nova &captura\tCtrl+Shift+N") }, { M_NEW_VIDEO, Tr(L"Nova &gravação de vídeo") },
                       { M_NEW_CLIPBOARD, Tr(L"Novo da área de &transferência") }, { M_NEW_IMAGE, Tr(L"Nova &imagem…") }, { 0, nullptr },
                       { M_OPEN, Tr(L"&Abrir…\tCtrl+O") }, { M_SAVE, Tr(L"&Salvar\tCtrl+S") }, { M_SAVEAS, Tr(L"Salvar &como…\tCtrl+Shift+S") },
                       { M_DUPLICATE, Tr(L"&Duplicar") }, { M_DELETE, Tr(L"&Excluir (Lixeira)") }, { 0, nullptr },
                       { M_PRINT, Tr(L"&Imprimir…\tCtrl+P") }, { M_EMAIL, Tr(L"Enviar por &e-mail…") }, { M_OPEN_PROGRAM, Tr(L"Abrir em &programa externo…") }, { 0, nullptr },
                       { M_LIBRARY, Tr(L"Abrir a pasta da &biblioteca") }, { M_CLOSE, Tr(L"&Fechar documento\tCtrl+W") }, { M_EXIT, Tr(L"Sai&r") } });
    HMENU edit = pop({ { M_UNDO, Tr(L"&Desfazer\tCtrl+Z") }, { M_REDO, Tr(L"&Refazer\tCtrl+Y") }, { M_UNDOALL, Tr(L"Desfazer &tudo") }, { 0, nullptr },
                       { M_CUT, Tr(L"Recor&tar\tCtrl+X") }, { M_COPY, Tr(L"&Copiar\tCtrl+C") }, { M_COPYALL, Tr(L"Copiar t&udo\tCtrl+Shift+C") },
                       { M_PASTE, Tr(L"C&olar\tCtrl+V") }, { M_PASTE_INPLACE, Tr(L"Colar no &mesmo lugar\tCtrl+Shift+V") },
                       { M_DELETE_SEL, Tr(L"E&xcluir\tDel") }, { M_SELECTALL, Tr(L"Selecionar tu&do\tCtrl+A") },
                       { M_CROP_SEL, Tr(L"Cortar para a &seleção") }, { M_LIFT, Tr(L"&Levantar a seleção (peça solta)\tCtrl+Shift+L") },
                       { M_DROP, Tr(L"Soltar a &peça na imagem\tEnter") }, { 0, nullptr },
                       { M_PLAINPASTE, Tr(L"Colar texto sem &formatação") }, { M_EDGESNAP, Tr(L"Seleção encaixa nas &bordas da imagem") }, { 0, nullptr },
                       { M_OCR, Tr(L"&Grab Text (OCR)\tCtrl+Shift+O") } });
    // Image › Effects, with a sample of each effect (as in the reference)
    HMENU fxm = pop({ { M_FX_BORDER, Tr(L"&Borda") }, { M_FX_EDGE, Tr(L"Bor&das") }, { M_FX_CAPTION, Tr(L"&Informações da captura") },
                      { M_FX_PERSPECTIVE, Tr(L"&Perspectiva") }, { M_FX_PAGECURL, Tr(L"D&obra de página") }, { M_FX_SHADOW, Tr(L"&Sombra") },
                      { M_FX_FILTER, Tr(L"&Filtros") }, { M_FX_ADJUST, Tr(L"&Ajuste de cores") }, { M_FX_REPLACE, Tr(L"Substituição de co&r") },
                      { M_FX_SPOTLIGHT, Tr(L"Spotlight e &lupa") }, { M_FX_WATERMARK, Tr(L"&Marca d'água") } });
    const std::pair<int, icons::Id> fxIcons[] = { { M_FX_BORDER, icons::FxBorder }, { M_FX_EDGE, icons::FxEdges }, { M_FX_CAPTION, icons::FxCaption },
        { M_FX_PERSPECTIVE, icons::FxPerspective }, { M_FX_PAGECURL, icons::FxPageCurl }, { M_FX_SHADOW, icons::FxShadow },
        { M_FX_FILTER, icons::FxFilters }, { M_FX_ADJUST, icons::FxColorAdjust }, { M_FX_REPLACE, icons::FxColorReplace },
        { M_FX_SPOTLIGHT, icons::FxSpotlight }, { M_FX_WATERMARK, icons::FxWatermark } };
    for (auto& [id, ic] : fxIcons) SetItemIcon(fxm, id, ic);
    HMENU align = pop({ { M_ALIGN_LEFT, Tr(L"À &esquerda") }, { M_ALIGN_CENTER, Tr(L"Ao &centro") }, { M_ALIGN_RIGHT, Tr(L"À &direita") }, { 0, nullptr },
                        { M_ALIGN_TOP, Tr(L"Em &cima") }, { M_ALIGN_MIDDLE, Tr(L"No &meio") }, { M_ALIGN_BOTTOM, Tr(L"Em&baixo") }, { 0, nullptr },
                        { M_DIST_H, Tr(L"Distribuir na &horizontal") }, { M_DIST_V, Tr(L"Distribuir na &vertical") } });
    const std::pair<int, icons::Id> alignIcons[] = { { M_ALIGN_LEFT, icons::AlignLeft }, { M_ALIGN_CENTER, icons::AlignCenter },
        { M_ALIGN_RIGHT, icons::AlignRight }, { M_ALIGN_TOP, icons::AlignTop }, { M_ALIGN_MIDDLE, icons::AlignMiddle },
        { M_ALIGN_BOTTOM, icons::AlignBottom }, { M_DIST_H, icons::DistributeH }, { M_DIST_V, icons::DistributeV } };
    for (auto& [id, ic] : alignIcons) SetItemIcon(align, id, ic);
    HMENU arrange = pop({ { M_BRING_FORWARD, Tr(L"Trazer para a &frente\tCtrl+Shift+F") }, { M_FRONT, Tr(L"Trazer para o &primeiro plano") },
                          { M_SEND_BACKWARD, Tr(L"Enviar para &trás\tCtrl+Shift+B") }, { M_BACK, Tr(L"Enviar para o plano de &fundo") } });
    AppendMenuW(arrange, MF_SEPARATOR, 0, nullptr);
    AppendMenuW(arrange, MF_POPUP, (UINT_PTR)align, Tr(L"&Alinhar"));
    AppendMenuW(arrange, MF_SEPARATOR, 0, nullptr);
    AppendMenuW(arrange, MF_STRING, M_FLIP_V, Tr(L"Espelhar na &vertical"));
    AppendMenuW(arrange, MF_STRING, M_FLIP_H, Tr(L"Espelhar na &horizontal"));
    HMENU rotate = pop({ { M_ROT_CW, Tr(L"Sentido &horário\tCtrl+R") }, { M_ROT_CCW, Tr(L"Sentido &anti-horário") }, { M_ROT_180, Tr(L"&180 graus") },
                         { M_ROT_FREE, Tr(L"Ângulo &personalizado…") } });
    HMENU img = CreatePopupMenu();
    AppendMenuW(img, MF_POPUP, (UINT_PTR)ToolsMenu(), Tr(L"Ferramen&tas"));
    AppendMenuW(img, MF_POPUP, (UINT_PTR)fxm, Tr(L"&Efeitos"));
    AppendMenuW(img, MF_SEPARATOR, 0, nullptr);
    AppendMenuW(img, MF_POPUP, (UINT_PTR)arrange, Tr(L"&Organizar"));
    AppendMenuW(img, MF_STRING, M_GROUP, Tr(L"A&grupar\tCtrl+Shift+G"));
    AppendMenuW(img, MF_STRING, M_UNGROUP, Tr(L"Desagr&upar\tCtrl+Shift+U"));
    AppendMenuW(img, MF_STRING, M_LOCK, Tr(L"&Bloquear objeto\tCtrl+L"));
    AppendMenuW(img, MF_STRING, M_UNLOCK, Tr(L"Desblo&quear objeto\tCtrl+Shift+K"));
    SetItemIcon(img, M_LOCK, icons::Lock);
    SetItemIcon(img, M_UNLOCK, icons::Unlock);
    AppendMenuW(img, MF_SEPARATOR, 0, nullptr);
    AppendMenuW(img, MF_STRING, M_FLATTEN_SEL, Tr(L"&Mesclar\tCtrl+T"));
    AppendMenuW(img, MF_STRING, M_FLATTEN, Tr(L"Mesclar t&udo\tCtrl+Shift+T"));
    AppendMenuW(img, MF_SEPARATOR, 0, nullptr);
    AppendMenuW(img, MF_STRING, M_TRIM, Tr(L"A&parar"));
    AppendMenuW(img, MF_STRING, M_RESIZE, Tr(L"&Redimensionar imagem…"));
    AppendMenuW(img, MF_STRING, M_CANVAS, Tr(L"Redimensionar &canvas…"));
    AppendMenuW(img, MF_STRING, M_APPLY_TEMPLATE, Tr(L"Aplicar mode&lo…\tCtrl+Alt+C"));
    AppendMenuW(img, MF_SEPARATOR, 0, nullptr);
    AppendMenuW(img, MF_POPUP, (UINT_PTR)rotate, Tr(L"&Girar"));
    AppendMenuW(img, MF_STRING, M_SNAPPING, Tr(L"Encai&xe no canvas\tCtrl+;"));
    AppendMenuW(img, MF_STRING, M_CANVAS_COLOR, Tr(L"Cor do canva&s…"));
    AppendMenuW(img, MF_SEPARATOR, 0, nullptr);
    AppendMenuW(img, MF_STRING, M_TAG, Tr(L"Etiquetar &imagem…"));
    HMENU view = pop({ { M_ZOOM_IN, Tr(L"&Ampliar\tCtrl++") }, { M_ZOOM_OUT, Tr(L"&Reduzir\tCtrl+-") }, { M_ZOOM_100, Tr(L"&Tamanho real\tCtrl+1") },
                       { M_ZOOM_FIT, Tr(L"A&justar à janela\tCtrl+0") }, { 0, nullptr }, { M_BG_COLOR, Tr(L"&Cor de fundo da área…") },
                       { M_RULERS, Tr(L"Ré&guas e guias\tCtrl+Shift+R") }, { M_CLEAR_GUIDES, Tr(L"Remover as gu&ias") }, { 0, nullptr },
                       { M_SHOW_EFFECTS, Tr(L"Painel de &efeitos") }, { M_SHOW_LAYERS, Tr(L"Painel de ca&madas") }, { M_SHOW_LIBRARY, Tr(L"&Biblioteca") },
                       { M_SHOW_TRAY, Tr(L"Capturas &recentes (bandeja)") }, { 0, nullptr },
                       { M_TB_CUSTOMIZE, Tr(L"&Personalizar a barra de ferramentas…") } });
    HMENU help = pop({ { M_MANUAL, Tr(L"&Manual do Snapnab\tF1") }, { M_SHORTCUTS, Tr(L"&Atalhos de teclado") },
                       { M_ABOUT, Tr(L"&Sobre o Snapnab Editor") } });
    AppendMenuW(bar, MF_POPUP, (UINT_PTR)file, Tr(L"&Arquivo"));
    AppendMenuW(bar, MF_POPUP, (UINT_PTR)edit, Tr(L"&Editar"));
    AppendMenuW(bar, MF_POPUP, (UINT_PTR)img, Tr(L"&Imagem"));
    AppendMenuW(bar, MF_POPUP, (UINT_PTR)view, Tr(L"E&xibir"));
    AppendMenuW(bar, MF_POPUP, (UINT_PTR)help, Tr(L"A&juda"));
    return bar;
}

HACCEL BuildAccel() {
    ACCEL a[] = {
        { FCONTROL | FVIRTKEY, 'O', M_OPEN }, { FCONTROL | FVIRTKEY, 'S', M_SAVE }, { FCONTROL | FSHIFT | FVIRTKEY, 'S', M_SAVEAS },
        { FCONTROL | FSHIFT | FVIRTKEY, 'N', M_NEW_CAPTURE }, { FCONTROL | FVIRTKEY, 'P', M_PRINT }, { FCONTROL | FVIRTKEY, 'W', M_CLOSE },
        { FCONTROL | FVIRTKEY, 'Z', M_UNDO }, { FCONTROL | FVIRTKEY, 'Y', M_REDO }, { FCONTROL | FSHIFT | FVIRTKEY, 'Z', M_REDO },
        { FCONTROL | FVIRTKEY, 'X', M_CUT }, { FCONTROL | FVIRTKEY, 'C', M_COPY }, { FCONTROL | FSHIFT | FVIRTKEY, 'C', M_COPYALL },
        { FCONTROL | FVIRTKEY, 'V', M_PASTE }, { FCONTROL | FSHIFT | FVIRTKEY, 'V', M_PASTE_INPLACE },
        { FCONTROL | FVIRTKEY, 'A', M_SELECTALL }, { FCONTROL | FSHIFT | FVIRTKEY, 'O', M_OCR },
        { FCONTROL | FVIRTKEY, 'L', M_LOCK }, { FCONTROL | FSHIFT | FVIRTKEY, 'K', M_UNLOCK }, { FCONTROL | FSHIFT | FVIRTKEY, 'L', M_LIFT },
        { FCONTROL | FSHIFT | FVIRTKEY, 'R', M_RULERS },
        { FCONTROL | FSHIFT | FVIRTKEY, 'F', M_BRING_FORWARD }, { FCONTROL | FSHIFT | FVIRTKEY, 'B', M_SEND_BACKWARD },
        { FCONTROL | FSHIFT | FVIRTKEY, 'G', M_GROUP }, { FCONTROL | FSHIFT | FVIRTKEY, 'U', M_UNGROUP },
        { FCONTROL | FVIRTKEY, 'T', M_FLATTEN_SEL }, { FCONTROL | FSHIFT | FVIRTKEY, 'T', M_FLATTEN },
        { FCONTROL | FALT | FVIRTKEY, 'C', M_APPLY_TEMPLATE }, { FCONTROL | FVIRTKEY, VK_OEM_1, M_SNAPPING },
        { FCONTROL | FVIRTKEY, 'R', M_ROT_CW }, { FCONTROL | FVIRTKEY, VK_OEM_PLUS, M_ZOOM_IN }, { FCONTROL | FVIRTKEY, VK_ADD, M_ZOOM_IN },
        { FCONTROL | FVIRTKEY, VK_OEM_MINUS, M_ZOOM_OUT }, { FCONTROL | FVIRTKEY, VK_SUBTRACT, M_ZOOM_OUT },
        { FCONTROL | FVIRTKEY, '1', M_ZOOM_100 }, { FCONTROL | FVIRTKEY, '0', M_ZOOM_FIT }, { FVIRTKEY, VK_F1, M_MANUAL },
    };
    return CreateAcceleratorTableW(a, (int)(sizeof a / sizeof *a));
}

// ---------------------------------------------------------------- toolbar (TopPanel.ToolBar)
// Itens de cinco tipos (ToolBarItemType: Tool, Effect, Output, Command, Spacer) e o "Mais/Editar" no fim.
// Default (automatic): Move, Selection, Text, Highlighter and Shape in the first 5; Magnifier last; positions 6 to 9
// show the most recently used tools. Custom: any item, in the order the user chooses.
struct ToolInfo { Tool t; icons::Id icon; const wchar_t* hint; };
const ToolInfo kTools[] = {
    { Tool::Favorites, icons::Star, TR_NOOP(L"Favoritos: seus estilos favoritos de todas as ferramentas") },
    { Tool::Arrow, icons::Arrow, TR_NOOP(L"Seta (A)") }, { Tool::Text, icons::Text, TR_NOOP(L"Texto (T)") }, { Tool::Callout, icons::Callout, TR_NOOP(L"Balão (C)") },
    { Tool::Shape, icons::Rect, TR_NOOP(L"Forma: retângulo, elipse… (R)") }, { Tool::Stamp, icons::Stamp, TR_NOOP(L"Carimbo (K)") },
    { Tool::Fill, icons::Fill, TR_NOOP(L"Preencher (F)") }, { Tool::Selection, icons::Selection, TR_NOOP(L"Seleção e Smart Move (S)") },
    { Tool::MagicWand, icons::Wand, TR_NOOP(L"Varinha mágica: seleciona pela cor") }, { Tool::Crop, icons::Crop, TR_NOOP(L"Cortar (X)") },
    { Tool::Cutout, icons::Cutout, TR_NOOP(L"Recortar faixa (U)") }, { Tool::Pen, icons::Pen, TR_NOOP(L"Caneta (P)") }, { Tool::Line, icons::Line, TR_NOOP(L"Linha (L)") },
    { Tool::Highlighter, icons::Highlighter, TR_NOOP(L"Marcador: retângulo, elipse, polígono ou mão livre (H)") },
    { Tool::Step, icons::Step, TR_NOOP(L"Passos numerados (N)") }, { Tool::Eraser, icons::Eraser, TR_NOOP(L"Borracha (E)") },
    { Tool::Blur, icons::Blur, TR_NOOP(L"Ocultar: desfoque/pixelado (B)") }, { Tool::Select, icons::Select, TR_NOOP(L"Mover e Smart Move (V)") },
    { Tool::Magnify, icons::Magnify, TR_NOOP(L"Lupa (Z)") }, { Tool::Spotlight, icons::Spotlight, TR_NOOP(L"Destaque/spotlight (G)") },
    { Tool::Eyedropper, icons::Eyedropper, TR_NOOP(L"Conta-gotas (I)") } };

// outputs and commands that can go on the toolbar (ToolBarCommandType: Undo, Redo, NewImage, NewFromClipboard, Open, Save,
// SaveAs, Duplicate, DeleteObject, DeleteImage, FlattenAll, RotateImage, AutoTrim, Print, Ocr)
struct CmdInfo { int id; icons::Id icon; const wchar_t* label; const wchar_t* hint; };
const CmdInfo kOutputs[] = {
    { T_SAVE, icons::Save, TR_NOOP(L"Arquivo"), TR_NOOP(L"Salvar como arquivo (PNG, JPG, PDF…)") }, { M_EMAIL, icons::Mail, TR_NOOP(L"E-mail"), TR_NOOP(L"Enviar por e-mail") },
    { M_OPEN_PROGRAM, icons::Program, TR_NOOP(L"Programa"), TR_NOOP(L"Abrir em um programa externo") },
    { T_COPYALL, icons::Clipboard, TR_NOOP(L"Área de transferência"), TR_NOOP(L"Copiar a imagem inteira") }, { M_PRINT, icons::Print, TR_NOOP(L"Impressora"), TR_NOOP(L"Imprimir a imagem") } };
const CmdInfo kCommands[] = {
    { M_UNDO, icons::Undo, TR_NOOP(L"Desfazer"), TR_NOOP(L"Desfazer (Ctrl+Z)") }, { M_REDO, icons::Redo, TR_NOOP(L"Refazer"), TR_NOOP(L"Refazer (Ctrl+Y)") },
    { M_NEW_IMAGE, icons::NewDoc, TR_NOOP(L"Nova imagem"), TR_NOOP(L"Nova imagem em branco") }, { M_NEW_CLIPBOARD, icons::NewClipboard, TR_NOOP(L"Da área de transf."), TR_NOOP(L"Nova imagem da área de transferência") },
    { M_OPEN, icons::Folder, TR_NOOP(L"Abrir"), TR_NOOP(L"Abrir arquivo (Ctrl+O)") }, { M_SAVE, icons::Save, TR_NOOP(L"Salvar"), TR_NOOP(L"Salvar na biblioteca (Ctrl+S)") },
    { M_SAVEAS, icons::SaveAs, TR_NOOP(L"Salvar como"), TR_NOOP(L"Salvar como arquivo (Ctrl+Shift+S)") }, { M_DUPLICATE, icons::Duplicate, TR_NOOP(L"Duplicar"), TR_NOOP(L"Duplicar a imagem") },
    { M_DELETE_SEL, icons::DeleteObject, TR_NOOP(L"Excluir objeto"), TR_NOOP(L"Excluir o objeto selecionado (Del)") }, { M_OCR, icons::Ocr, TR_NOOP(L"Grab Text"), TR_NOOP(L"Reconhecer o texto da imagem (Ctrl+Shift+T)") },
    { M_DELETE, icons::Trash, TR_NOOP(L"Excluir imagem"), TR_NOOP(L"Enviar a imagem para a Lixeira") },
    { M_FLATTEN, icons::Layers, TR_NOOP(L"Mesclar tudo"), TR_NOOP(L"Mesclar todos os objetos na imagem") }, { M_ROT_CW, icons::Rotate, TR_NOOP(L"Girar"), TR_NOOP(L"Girar 90° no sentido horário (Ctrl+R)") },
    { M_TRIM, icons::AutoTrim, TR_NOOP(L"Corte automático"), TR_NOOP(L"Cortar as bordas de cor uniforme") },
    { M_ALIGN_LEFT, icons::AlignLeft, TR_NOOP(L"Alinhar à esquerda"), TR_NOOP(L"Alinhar os objetos à esquerda") },
    { M_ALIGN_CENTER, icons::AlignCenter, TR_NOOP(L"Alinhar ao centro"), TR_NOOP(L"Alinhar os objetos ao centro") },
    { M_ALIGN_RIGHT, icons::AlignRight, TR_NOOP(L"Alinhar à direita"), TR_NOOP(L"Alinhar os objetos à direita") },
    { M_ALIGN_TOP, icons::AlignTop, TR_NOOP(L"Alinhar em cima"), TR_NOOP(L"Alinhar os objetos em cima") },
    { M_ALIGN_MIDDLE, icons::AlignMiddle, TR_NOOP(L"Alinhar no meio"), TR_NOOP(L"Alinhar os objetos no meio") },
    { M_ALIGN_BOTTOM, icons::AlignBottom, TR_NOOP(L"Alinhar embaixo"), TR_NOOP(L"Alinhar os objetos embaixo") },
    { M_DIST_H, icons::DistributeH, TR_NOOP(L"Distribuir na horizontal"), TR_NOOP(L"Espaçar 3 ou mais objetos igualmente na horizontal") },
    { M_DIST_V, icons::DistributeV, TR_NOOP(L"Distribuir na vertical"), TR_NOOP(L"Espaçar 3 ou mais objetos igualmente na vertical") },
    { M_LOCK, icons::Lock, TR_NOOP(L"Bloquear"), TR_NOOP(L"Bloquear o objeto selecionado (Ctrl+L)") },
    { M_RULERS, icons::Ruler, TR_NOOP(L"Réguas"), TR_NOOP(L"Mostrar réguas, guias e coordenadas (Ctrl+Shift+R)") } };

const Tool kFixedTools[] = { Tool::Select, Tool::Selection, Tool::Text, Tool::Highlighter, Tool::Shape };
int RecentSlots() { return std::clamp(g.tbToolCount, 6, 16) - 6; }

// fills up (or trims) the recent ones to the chosen number of tools
void FitRecentTools();

bool IsFixedTool(Tool t) { return std::find(std::begin(kFixedTools), std::end(kFixedTools), t) != std::end(kFixedTools) || t == Tool::Magnify; }

ToolbarEntryInfo DescribeEntry(const ToolbarEntry& e) {
    switch (e.type) {
    case ToolbarEntry::Tool:
        for (auto& t : kTools) if ((int)t.t == e.id) return { t.icon, ToolName(t.t), Tr(t.hint) };
        break;
    case ToolbarEntry::Effect:
        if (auto* fxe = g.effects.Find(e.id)) {
            icons::Id ic = icons::Effects;
            switch (e.id) {
            case M_FX_BORDER: ic = icons::FxBorder; break;
            case M_FX_EDGE: ic = icons::FxEdges; break;
            case M_FX_CAPTION: ic = icons::FxCaption; break;
            case M_FX_PERSPECTIVE: ic = icons::FxPerspective; break;
            case M_FX_PAGECURL: ic = icons::FxPageCurl; break;
            case M_FX_SHADOW: ic = icons::FxShadow; break;
            case M_FX_FILTER: ic = icons::FxFilters; break;
            case M_FX_ADJUST: ic = icons::FxColorAdjust; break;
            case M_FX_REPLACE: ic = icons::FxColorReplace; break;
            case M_FX_SPOTLIGHT: ic = icons::FxSpotlight; break;
            case M_FX_WATERMARK: ic = icons::FxWatermark; break;
            }
            return { ic, fxe->name, Tr(L"Efeito: ") + fxe->name + Tr(L" — abre os ajustes no painel de efeitos") };
        }
        break;
    case ToolbarEntry::Output:
        for (auto& c : kOutputs) if (c.id == e.id) return { c.icon, Tr(c.label), Tr(c.hint) };
        break;
    case ToolbarEntry::Command:
        for (auto& c : kCommands) if (c.id == e.id) return { c.icon, Tr(c.label), Tr(c.hint) };
        break;
    default: break;
    }
    return { icons::Gear, L"", L"" };
}

std::vector<ToolbarEntry> CenterEntries() {
    if (g.tbCustom && !g.tbCustomList.empty()) return g.tbCustomList;
    std::vector<ToolbarEntry> v;
    for (Tool t : kFixedTools) v.push_back({ ToolbarEntry::Tool, (int)t });
    for (Tool t : g.tbRecent) v.push_back({ ToolbarEntry::Tool, (int)t });
    v.push_back({ ToolbarEntry::Tool, (int)Tool::Magnify });
    return v;
}

// registry text: "t:5,e:1039,o:6,c:1015,s:0" (type:id)
std::wstring EntriesToString(const std::vector<ToolbarEntry>& v) {
    static const wchar_t kType[] = { L't', L'e', L'o', L'c', L's' };
    std::wstring s;
    for (auto& e : v) s += (s.empty() ? L"" : L",") + std::wstring(1, kType[e.type]) + L":" + std::to_wstring(e.id);
    return s;
}

std::vector<ToolbarEntry> EntriesFromString(const std::wstring& s) {
    std::vector<ToolbarEntry> v;
    for (size_t i = 0; i < s.size();) {
        size_t j = s.find(L',', i);
        std::wstring item = s.substr(i, j == std::wstring::npos ? std::wstring::npos : j - i);
        if (item.size() > 2 && item[1] == L':') {
            static const std::wstring kType = L"teocs";
            size_t t = kType.find(item[0]);
            if (t != std::wstring::npos) v.push_back({ (ToolbarEntry::Type)t, _wtoi(item.c_str() + 2) });
        }
        if (j == std::wstring::npos) break;
        i = j + 1;
    }
    return v;
}

void LoadToolbarConfig() {
    g.tbCustom = RegGetDword(L"ToolbarCustom", 0, L"Editor") != 0;
    g.tbCustomList = EntriesFromString(RegGetString(L"ToolbarItems", L"", L"Editor"));
    g.tbToolCount = std::clamp((int)RegGetDword(L"ToolbarToolCount", 10, L"Editor"), 6, 16);
    g.tbRecent.clear();
    for (auto& e : EntriesFromString(RegGetString(L"ToolbarRecentTools", L"", L"Editor")))
        if (e.type == ToolbarEntry::Tool && !IsFixedTool((Tool)e.id) && e.id < (int)Tool::Count) g.tbRecent.push_back((Tool)e.id);
    FitRecentTools();
}

void FitRecentTools() {
    if ((int)g.tbRecent.size() > RecentSlots()) g.tbRecent.resize(RecentSlots());
    for (Tool t : { Tool::Arrow, Tool::Callout, Tool::Step, Tool::Blur, Tool::Stamp, Tool::Pen, Tool::Line, Tool::Fill, Tool::Crop,
                    Tool::Spotlight, Tool::Eraser })                          // fills up with the most common ones
        if ((int)g.tbRecent.size() < RecentSlots() && std::find(g.tbRecent.begin(), g.tbRecent.end(), t) == g.tbRecent.end()) g.tbRecent.push_back(t);
}

void SaveToolbarConfig() {
    RegSetDword(L"ToolbarCustom", g.tbCustom, L"Editor");
    RegSetDword(L"ToolbarToolCount", g.tbToolCount, L"Editor");
    RegSetString(L"ToolbarItems", EntriesToString(g.tbCustomList), L"Editor");
    std::vector<ToolbarEntry> rec;
    for (Tool t : g.tbRecent) rec.push_back({ ToolbarEntry::Tool, (int)t });
    RegSetString(L"ToolbarRecentTools", EntriesToString(rec), L"Editor");
}

// tool used: in automatic mode, it takes one of positions 6 to 9, replacing the least recently used
void NoteToolUsed(Tool t) {
    static std::map<Tool, ULONGLONG> lastUse;
    lastUse[t] = GetTickCount64();
    if (g.tbCustom || IsFixedTool(t) || RecentSlots() == 0) return;
    if (std::find(g.tbRecent.begin(), g.tbRecent.end(), t) != g.tbRecent.end()) return;
    auto oldest = std::min_element(g.tbRecent.begin(), g.tbRecent.end(), [](Tool a, Tool b) { return lastUse[a] < lastUse[b]; });
    if (oldest != g.tbRecent.end()) *oldest = t;
    SaveToolbarConfig();
    g.toolbar.SetCenter(CenterEntries());
}

void ShowToolIntro(Tool t);

void SetTool(Tool t) {
    if (t == Tool::Favorites) {
        // the drawing tool becomes that of the last used favorite (or of the first favorite)
        Tool ct = g.canvas.tool;
        bool ok = false;
        for (auto& f : QS().favorites) if (f.tool == ct) ok = true;
        if (!ok && !QS().favorites.empty()) { ct = QS().favorites[0].tool; g.canvas.styles[ct] = QS().favorites[0].style; }
        g.canvas.SetTool(ct);
    } else {
        if (t == Tool::Selection && g.canvas.selMode == 3) g.canvas.selMode = 0;
        g.canvas.SetTool(t);
    }
    g.toolbar.SetActive(t);
    NoteToolUsed(t);
    if (g.rightTab != 0 && t != Tool::Select) SetRightTab(0);      // choosing a tool shows its properties
    g.props.SetBarTool(t);
    SetStatus(std::wstring(Tr(L"Ferramenta: ")) + ToolName(t));
    if (t == Tool::Magnify) ShowToolIntro(t);
}

// tool introduction window (onboarding), shown once: "Magnify part of the image"
void ShowToolIntro(Tool t) {
    if (t != Tool::Magnify || RegGetDword(L"MagnifyIntroShown", 0, L"Editor")) return;
    RegSetDword(L"MagnifyIntroShown", 1, L"Editor");
    struct Intro { bool done = false; RECT okR{}; bool hot = false; };
    static Intro* cur = nullptr;
    Intro st;
    cur = &st;
    static bool reg = false;
    if (!reg) {
        WNDCLASSW wc{}; wc.hInstance = g.hi; wc.hCursor = LoadCursor(nullptr, IDC_ARROW); wc.lpszClassName = L"SnapnabToolIntro";
        wc.lpfnWndProc = [](HWND h, UINT m, WPARAM w, LPARAM l) -> LRESULT {
            if (!cur) return DefWindowProcW(h, m, w, l);
            int dpi = DpiForWindow(h);
            auto S = [dpi](int v) { return ui::Scale(v, dpi); };
            switch (m) {
            case WM_ERASEBKGND: return 1;
            case WM_PAINT: {
                PAINTSTRUCT ps; HDC dc = BeginPaint(h, &ps);
                RECT cr; GetClientRect(h, &cr);
                HDC mem = CreateCompatibleDC(dc);
                HBITMAP bb = CreateCompatibleBitmap(dc, cr.right, cr.bottom);
                HGDIOBJ ob = SelectObject(mem, bb);
                HBRUSH wb = CreateSolidBrush(RGB(255, 255, 255)); FillRect(mem, &cr, wb); DeleteObject(wb);
                RECT art{ S(12), S(12), cr.right - S(12), S(12) + S(330) };
                HBRUSH ab = CreateSolidBrush(RGB(42, 46, 58)); FillRect(mem, &art, ab); DeleteObject(ab);
                {
                    Gdiplus::Graphics g2(mem);
                    g2.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
                    float cx = (art.left + art.right) / 2.0f, top = (float)art.top + S(56);
                    Gdiplus::RectF win(cx - S(115), top, (float)S(230), (float)S(245));
                    Gdiplus::SolidBrush paper(Gdiplus::Color(255, 228, 230, 234)); g2.FillRectangle(&paper, win);
                    Gdiplus::SolidBrush bar(Gdiplus::Color(255, 250, 250, 252)); g2.FillRectangle(&bar, win.X, win.Y, win.Width, (float)S(46));
                    Gdiplus::SolidBrush gray(Gdiplus::Color(255, 150, 160, 176));
                    g2.FillRectangle(&gray, win.X + S(46), win.Y + S(18), (float)S(84), (float)S(12));
                    Gdiplus::Pen x(Gdiplus::Color(255, 150, 160, 176), (float)S(3));
                    g2.DrawLine(&x, win.GetRight() - S(36), win.Y + S(16), win.GetRight() - S(22), win.Y + S(30));
                    g2.DrawLine(&x, win.GetRight() - S(22), win.Y + S(16), win.GetRight() - S(36), win.Y + S(30));
                    for (int i = 0; i < 4; i++) g2.FillRectangle(&gray, win.X + S(80), win.Y + S(80) + i * S(26), (float)(i % 2 ? S(70) : S(90)), (float)S(10));
                    Gdiplus::SolidBrush blue(Gdiplus::Color(255, 90, 160, 230)); g2.FillRectangle(&blue, win.X + S(120), win.GetBottom() - S(46), (float)S(70), (float)S(24));
                    Gdiplus::RectF lens(win.X - S(20), win.Y + S(70), (float)S(140), (float)S(140));
                    Gdiplus::SolidBrush lw(Gdiplus::Color(255, 236, 238, 242)); g2.FillEllipse(&lw, lens);
                    Gdiplus::SolidBrush dot(Gdiplus::Color(255, 120, 124, 132)); g2.FillEllipse(&dot, lens.X + S(56), lens.Y + S(56), (float)S(28), (float)S(28));
                    Gdiplus::SolidBrush ww(Gdiplus::Color(255, 255, 255, 255));
                    g2.FillEllipse(&ww, lens.X + S(54), lens.Y + S(8), (float)S(30), (float)S(30)); g2.FillEllipse(&ww, lens.X + S(54), lens.Y + S(102), (float)S(30), (float)S(30));
                    Gdiplus::Pen ring(Gdiplus::Color(255, 160, 90, 200), (float)S(2)); g2.DrawEllipse(&ring, lens);
                    Gdiplus::Pen sel(Gdiplus::Color(255, 120, 120, 126), 1.0f); g2.DrawRectangle(&sel, lens.X + S(36), lens.Y + S(36), (float)S(68), (float)S(68));
                    Gdiplus::SolidBrush cur2(Gdiplus::Color(255, 20, 20, 20));
                    Gdiplus::PointF arrow[] = { { lens.X + S(76), lens.Y + S(70) }, { lens.X + S(76), lens.Y + S(104) }, { lens.X + S(84), lens.Y + S(96) },
                                                { lens.X + S(90), lens.Y + S(108) }, { lens.X + S(95), lens.Y + S(105) }, { lens.X + S(89), lens.Y + S(94) }, { lens.X + S(100), lens.Y + S(94) } };
                    g2.FillPolygon(&cur2, arrow, 7);
                }
                RECT t1{ S(32), art.bottom + S(24), cr.right - S(32), art.bottom + S(54) };
                SetBkMode(mem, TRANSPARENT);
                HGDIOBJ of = SelectObject(mem, ui::UiFont(dpi, 14, true));
                SetTextColor(mem, RGB(52, 62, 84));
                DrawTextW(mem, Tr(L"Ampliar parte da imagem"), -1, &t1, DT_LEFT | DT_SINGLELINE | DT_VCENTER);
                RECT t2{ S(32), t1.bottom + S(2), cr.right - S(32), t1.bottom + S(54) };
                SelectObject(mem, ui::UiFont(dpi, 11));
                SetTextColor(mem, RGB(80, 86, 100));
                DrawTextW(mem, Tr(L"Amplie partes da imagem para destacar os detalhes importantes. Arraste para criar a lupa; "
                               L"com a lente separada, a alça amarela escolhe a área ampliada."), -1, &t2, DT_LEFT | DT_WORDBREAK);
                SelectObject(mem, of);
                {
                    Gdiplus::Graphics g2(mem);
                    g2.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
                    Gdiplus::GraphicsPath gp;
                    RECT o = cur->okR;
                    float rad = (float)S(6), d = rad * 2;
                    gp.AddArc((float)o.left, (float)o.top, d, d, 180, 90); gp.AddArc((float)o.right - d, (float)o.top, d, d, 270, 90);
                    gp.AddArc((float)o.right - d, (float)o.bottom - d, d, d, 0, 90); gp.AddArc((float)o.left, (float)o.bottom - d, d, d, 90, 90);
                    COLORREF c = cur->hot ? ui::Mix(g.th.control, 0, 0.12) : g.th.control;
                    Gdiplus::SolidBrush ob2(Gdiplus::Color(255, GetRValue(c), GetGValue(c), GetBValue(c)));
                    g2.FillPath(&ob2, &gp);
                }
                ui::DrawTextCentered(mem, Tr(L"Entendi!"), cur->okR, RGB(255, 255, 255), ui::UiFont(dpi, 11));
                BitBlt(dc, 0, 0, cr.right, cr.bottom, mem, 0, 0, SRCCOPY);
                SelectObject(mem, ob); DeleteObject(bb); DeleteDC(mem);
                EndPaint(h, &ps);
                return 0;
            }
            case WM_MOUSEMOVE: { bool hv = PtInRect(&cur->okR, { GET_X_LPARAM(l), GET_Y_LPARAM(l) }) != 0; if (hv != cur->hot) { cur->hot = hv; InvalidateRect(h, nullptr, FALSE); } return 0; }
            case WM_LBUTTONUP: if (PtInRect(&cur->okR, { GET_X_LPARAM(l), GET_Y_LPARAM(l) })) cur->done = true; return 0;
            case WM_KEYDOWN: if (w == VK_RETURN || w == VK_ESCAPE || w == VK_SPACE) cur->done = true; return 0;
            case WM_CLOSE: cur->done = true; return 0;
            }
            return DefWindowProcW(h, m, w, l);
        };
        RegisterClassW(&wc); reg = true;
    }
    int dpi = g.dpi;
    int W = ui::Scale(660, dpi), H = ui::Scale(560, dpi);
    RECT wr{ 0, 0, W, H };
    AdjustWindowRectExForDpi(&wr, WS_CAPTION | WS_SYSMENU, FALSE, WS_EX_DLGMODALFRAME, dpi);
    RECT fr; GetWindowRect(g.frame, &fr);
    int ww = wr.right - wr.left, wh = wr.bottom - wr.top;
    st.okR = { W - ui::Scale(32, dpi) - ui::Scale(130, dpi), H - ui::Scale(68, dpi), W - ui::Scale(32, dpi), H - ui::Scale(30, dpi) };
    HWND h = CreateWindowExW(WS_EX_DLGMODALFRAME, L"SnapnabToolIntro", Tr(L"Ampliar parte da imagem"), WS_CAPTION | WS_SYSMENU | WS_POPUP,
                             (fr.left + fr.right - ww) / 2, (fr.top + fr.bottom - wh) / 2, ww, wh, g.frame, nullptr, g.hi, nullptr);
    EnableWindow(g.frame, FALSE);
    ShowWindow(h, SW_SHOW);
    SetFocus(h);
    MSG msg;
    while (!st.done && GetMessageW(&msg, nullptr, 0, 0)) { TranslateMessage(&msg); DispatchMessageW(&msg); }
    EnableWindow(g.frame, TRUE);
    DestroyWindow(h);
    SetForegroundWindow(g.frame);
    cur = nullptr;
}

HMENU ToolsMenu() {
    HMENU m = CreatePopupMenu();
    for (auto& t : kTools) {
        AppendMenuW(m, MF_STRING, M_MORE_TOOL_BASE + (int)t.t, ToolName(t.t));
        SetItemIcon(m, M_MORE_TOOL_BASE + (int)t.t, t.icon);
    }
    return m;
}

// Customize (CBCGPToolbarCustomize): grid to drag onto the toolbar; Cancel restores the previous toolbar
void CustomizeToolbar() {
    std::vector<std::pair<std::wstring, std::vector<ToolbarEntry>>> catalog(5);
    catalog[0].first = Tr(L"Ferramentas de desenho"); for (auto& t : kTools) catalog[0].second.push_back({ ToolbarEntry::Tool, (int)t.t });
    catalog[1].first = Tr(L"Itens de menu"); for (auto& c : kCommands) catalog[1].second.push_back({ ToolbarEntry::Command, c.id });
    catalog[2].first = Tr(L"Destinos de compartilhamento"); for (auto& c : kOutputs) catalog[2].second.push_back({ ToolbarEntry::Output, c.id });
    catalog[3].first = Tr(L"Efeitos");                    // in the order of the Image › Effects menu
    for (int cmd : { M_FX_BORDER, M_FX_EDGE, M_FX_CAPTION, M_FX_PERSPECTIVE, M_FX_PAGECURL, M_FX_SHADOW, M_FX_FILTER, M_FX_ADJUST,
                     M_FX_REPLACE, M_FX_SPOTLIGHT, M_FX_WATERMARK })
        catalog[3].second.push_back({ ToolbarEntry::Effect, cmd });
    catalog[4].first = Tr(L"Diversos"); catalog[4].second.push_back({ ToolbarEntry::Spacer, 0 });
    std::vector<ToolbarEntry> before = g.toolbar.center, beforeList = g.tbCustomList;
    std::vector<Tool> beforeRecent = g.tbRecent;
    bool wasCustom = g.tbCustom;
    int beforeCount = g.tbToolCount;
    auto defaults = [] { bool c = g.tbCustom; g.tbCustom = false; auto v = CenterEntries(); g.tbCustom = c; return v; };
    auto onCount = [](int n) {                       // number of tools: back to the automatic toolbar with that size
        g.tbToolCount = n;
        FitRecentTools();
        g.tbCustom = false;
        g.toolbar.SetCenter(CenterEntries());
    };
    if (!CustomizeToolbarDialog(g.frame, g.toolbar, catalog, DescribeEntry, defaults, g.tbToolCount, onCount)) {
        g.tbCustom = wasCustom; g.tbCustomList = beforeList; g.tbToolCount = beforeCount; g.tbRecent = beforeRecent;
        g.toolbar.SetCenter(before);
        SaveToolbarConfig();
        SetStatus(Tr(L"Personalização cancelada."));
        return;
    }
    g.tbCustom = !(g.toolbar.center == defaults());
    if (g.tbCustom) g.tbCustomList = g.toolbar.center;
    SaveToolbarConfig();
    g.toolbar.SetCenter(CenterEntries());
    SetStatus(g.tbCustom ? Tr(L"Barra de ferramentas personalizada.") : Tr(L"Barra de ferramentas automática."));
}

// "More/Edit" (EditMore): the tools that are not on the toolbar, with icons, and the customization
void MoreToolsMenu(POINT pt) {
    std::vector<PopupItem> items;
    std::vector<ToolbarEntry> vis = CenterEntries();
    for (auto& ti : kTools) {
        if (std::find(vis.begin(), vis.end(), ToolbarEntry{ ToolbarEntry::Tool, (int)ti.t }) != vis.end()) continue;
        PopupItem it;
        it.id = M_MORE_TOOL_BASE + (int)ti.t;
        it.icon = ti.icon;
        it.label = ToolName(ti.t);
        std::wstring h = Tr(ti.hint);                   // shortcut: the letter in parentheses at the end of the hint, e.g. "(K)"
        size_t o = h.rfind(L'('), c = h.rfind(L')');
        if (o != std::wstring::npos && c == o + 2) it.shortcut = h.substr(o + 1, 1);
        it.checked = ti.t == g.toolbar.active;
        items.push_back(it);
    }
    PopupItem sep; sep.separator = true;
    items.push_back(sep);
    PopupItem cust; cust.id = M_TB_CUSTOMIZE; cust.icon = icons::Gear; cust.label = Tr(L"Personalizar a barra…");
    items.push_back(cust);
    PopupItem reset; reset.id = M_TB_RESET; reset.icon = icons::Restart; reset.label = Tr(L"Voltar ao padrão (automática)"); reset.enabled = g.tbCustom;
    items.push_back(reset);
    int cmd = ListPopup(g.frame, pt, items, ui::Scale(220, g.dpi));
    if (cmd > 0) PostMessageW(g.frame, WM_COMMAND, cmd, 0);
}

// F1 / Help › Manual: the manual opens at the chapter of what is on screen (library, video or the current tool)
void ShowManual() {
    static const wchar_t* const kToolTopic[] = {
        L"tool-move", L"tool-arrow", L"tool-line", L"tool-shape", L"tool-pen", L"tool-highlighter", L"tool-text", L"tool-callout",
        L"tool-steps", L"tool-stamp", L"tool-blur", L"tool-spotlight", L"tool-magnify", L"tool-fill", L"tool-eraser", L"tool-crop",
        L"tool-selection", L"tool-cutout", L"tool-eyedropper", L"tool-favorites", L"tool-magicwand" };
    static_assert(std::size(kToolTopic) == (size_t)Tool::Count, "one manual topic per tool");
    const wchar_t* topic = L"editor-tour";
    if (g.libraryOpen) topic = L"tray-library";
    else if (g.cur && g.cur->isVideo) topic = L"video-editing";
    else if (g.cur) topic = kToolTopic[(int)g.canvas.tool];
    OpenManual(g.frame, topic);
}

void ShowShortcuts() {
    ui::ShowTextWindow(g.frame, Tr(L"Atalhos do Snapnab Editor"),
        Tr(L"Ferramentas (com a área de pintura em foco):\n"
        L"  V Mover   A Seta   L Linha   R Forma   P Caneta   H Marcador\n"
        L"  T Texto   C Balão   N Passo   K Carimbo   B Ocultar   G Destaque   Z Lupa\n"
        L"  F Preencher   E Borracha   X Cortar   S Seleção   U Recortar faixa   I Conta-gotas\n\n"
        L"Edição:  Ctrl+Z/Ctrl+Y desfazer/refazer • Ctrl+C/X/V • Ctrl+Shift+C copiar tudo • Del excluir\n"
        L"         Setas movem o objeto (Shift = 10 px) • Shift restringe proporção/ângulo\n"
        L"         Duplo clique edita texto • Esc cancela • Enter aplica o recorte\n"
        L"Exibir:  Ctrl+roda = zoom • Ctrl+0 ajustar • Ctrl+1 tamanho real • botão do meio arrasta\n"
        L"Arquivo: Ctrl+S salvar na biblioteca • Ctrl+Shift+S salvar como (PNG/JPG/BMP/GIF/TIFF/PDF/SNAP)\n"
        L"Texto:   Ctrl+Shift+O Grab Text (OCR)\n"
        L"Imagem:  Ctrl+Shift+F/B um passo à frente/atrás • Ctrl+T mesclar o objeto • Ctrl+Shift+T mesclar tudo • Ctrl+; encaixe\n"
        L"Vídeo:   Espaço reproduz/pausa • ←/→ quadro • I/O marcam início/fim\n"));
}

void OnCommand(int id) {
    if (id >= T_TOOL_BASE && id < T_TOOL_BASE + (int)Tool::Count) { SetTool((Tool)(id - T_TOOL_BASE)); return; }
    if (id >= M_MORE_TOOL_BASE && id < M_MORE_TOOL_BASE + (int)Tool::Count) { SetTool((Tool)(id - M_MORE_TOOL_BASE)); return; }
    switch (id) {
    case M_TB_CUSTOMIZE: CustomizeToolbar(); break;
    case M_TB_RESET: g.tbCustom = false; SaveToolbarConfig(); g.toolbar.SetCenter(CenterEntries()); break;
    case M_SHOW_EFFECTS: ShowLibrary(false); SetRightTab(g.rightTab == 1 ? 0 : 1); break;
    case M_SHOW_LAYERS: ShowLibrary(false); SetRightTab(g.rightTab == 2 ? 0 : 2); break;
    case M_SHOW_LIBRARY: ShowLibrary(!g.libraryOpen); break;
    case T_CAPTURE: case M_NEW_CAPTURE: RequestCapture(0); break;
    case T_RECORD: case M_NEW_VIDEO: RequestCapture(1); break;
    case T_COPYALL: case M_COPYALL:
        if (RequireImageDoc() && CopyImageToClipboard(g.frame, g.cur->Render())) SetStatus(Tr(L"Imagem inteira copiada para a área de transferência."));
        break;
    case T_SAVE: case M_SAVEAS: SaveAs(); break;
    case T_SHARE: {
        HMENU m = CreatePopupMenu();
        AppendMenuW(m, MF_STRING, M_SAVEAS, Tr(L"Arquivo…"));
        AppendMenuW(m, MF_STRING, M_COPYALL, Tr(L"Área de transferência"));
        AppendMenuW(m, MF_STRING, M_EMAIL, Tr(L"E-mail…"));
        AppendMenuW(m, MF_STRING, M_PRINT, Tr(L"Impressora…"));
        AppendMenuW(m, MF_STRING, M_OPEN_PROGRAM, Tr(L"Programa externo…"));
        POINT p; GetCursorPos(&p);
        int cmd = TrackPopupMenu(m, TPM_RETURNCMD | TPM_RIGHTBUTTON, p.x, p.y, 0, g.frame, nullptr);
        DestroyMenu(m);
        if (cmd) OnCommand(cmd);
        break;
    }
    case M_NEW_CLIPBOARD: NewFromClipboard(); break;
    case M_NEW_IMAGE: NewBlankImage(); break;
    case M_OPEN: { std::wstring p; if (ui::OpenFileDialog(g.frame, ImageOpenFilter(), p)) OpenPath(p); break; }
    case M_SAVE: SaveCurrent(); break;
    case M_DUPLICATE: DuplicateCurrent(); break;
    case M_DELETE: if (g.cur) DeleteDoc(g.cur->path); break;
    case M_PRINT: PrintCurrent(); break;
    case M_EMAIL: SendEmail(); break;
    case M_OPEN_PROGRAM: OpenInProgram(); break;
    case M_LIBRARY: ShellExecuteW(nullptr, L"open", LibraryDir().c_str(), nullptr, nullptr, SW_SHOWNORMAL); break;
    case M_CLOSE: CloseCurrent(); break;
    case M_EXIT: SendMessageW(g.frame, WM_CLOSE, 0, 0); break;
    case M_UNDO: if (g.cur && !g.cur->isVideo) { g.canvas.CommitText(); g.cur->Undo(); g.canvas.selected = -1; g.canvas.Refresh(); g.canvas.FitToWindow(); DocChanged(); g.props.Load(); } break;
    case M_REDO: if (g.cur && !g.cur->isVideo) { g.canvas.CommitText(); g.cur->Redo(); g.canvas.selected = -1; g.canvas.Refresh(); DocChanged(); g.props.Load(); } break;
    case M_UNDOALL: if (g.cur && !g.cur->isVideo) { g.cur->UndoAll(); g.canvas.selected = -1; g.canvas.FitToWindow(); g.canvas.Refresh(); DocChanged(); } break;
    case M_CUT: CopyToClipboard(true); break;
    case M_COPY: if (CopyToClipboard(false)) SetStatus(Tr(L"Copiado.")); break;
    case M_PASTE: Paste(); break;
    case M_PASTE_INPLACE: Paste(true); break;
    case M_DIST_H: case M_DIST_V: if (RequireImageDoc()) g.canvas.DistributeSelected(id == M_DIST_H); break;
    case M_LOCK: case M_UNLOCK: {                    // Ctrl+L toggles; Unlock always unlocks
        if (!RequireImageDoc()) break;
        auto v = g.canvas.SelectedList();
        if (v.empty()) { SetStatus(Tr(L"Selecione um objeto para bloquear ou desbloquear.")); break; }
        bool allLocked = std::all_of(v.begin(), v.end(), [](int i) { return g.cur->st.objs[i]->locked; });
        bool lock = id == M_LOCK && !allLocked;
        g.canvas.SetLocked(v, lock);
        SetStatus(lock ? Tr(L"Objeto bloqueado: não pode ser movido, redimensionado nem excluído.") : Tr(L"Objeto desbloqueado."));
        break;
    }
    case M_LIFT:
        if (!RequireImageDoc()) break;
        if (!g.canvas.HasRasterSelection()) { SetStatus(Tr(L"Selecione uma área com a ferramenta Seleção para levantá-la.")); break; }
        if (g.canvas.LiftSelection()) SetStatus(Tr(L"Peça solta: ajuste o resto da imagem (cortar, recortar faixa…), posicione a peça e tecle Enter para soltá-la."));
        break;
    case M_DROP: if (RequireImageDoc() && g.canvas.DropFloating()) SetStatus(Tr(L"Peça solta na imagem.")); break;
    case M_RULERS:
        g.canvas.rulers = !g.canvas.rulers;
        RegSetDword(L"Rulers", g.canvas.rulers, L"Editor");
        if (!g.canvas.rulers) g.coords.clear();
        g.canvas.Refresh(false);
        if (g.canvas.zoom > 0 && g.cur && !g.cur->isVideo) g.canvas.ZoomTo(g.canvas.zoom);
        InvalidateStatus();
        SetStatus(g.canvas.rulers ? Tr(L"Réguas ligadas: arraste de uma régua para criar uma guia; arraste a guia de volta para removê-la.") : Tr(L"Réguas desligadas."));
        break;
    case M_CLEAR_GUIDES:
        if (g.cur && !g.cur->isVideo && (!g.cur->guidesX.empty() || !g.cur->guidesY.empty())) {
            g.cur->guidesX.clear(); g.cur->guidesY.clear(); g.cur->modified = true;
            g.canvas.Refresh(false);
        }
        break;
    case M_EDGESNAP:
        g.canvas.edgeSnap = !g.canvas.edgeSnap;
        RegSetDword(L"EdgeSnap", g.canvas.edgeSnap, L"Editor");
        SetStatus(g.canvas.edgeSnap ? Tr(L"A seleção e o corte encaixam nas bordas da imagem (Alt desliga na hora).") : Tr(L"Encaixe nas bordas desligado."));
        break;
    case M_PLAINPASTE:
        g.canvas.plainPaste = !g.canvas.plainPaste;
        RegSetDword(L"PlainTextPaste", g.canvas.plainPaste, L"Editor");
        SetStatus(g.canvas.plainPaste ? Tr(L"Texto colado sem formatação (Ctrl+Shift+V mantém a formatação).") : Tr(L"Texto colado com a formatação de origem (Ctrl+Shift+V cola sem formatação)."));
        break;
    case M_RENAME_OBJ: {
        if (!RequireImageDoc() || g.canvas.selected < 0) break;
        int i = g.canvas.selected;
        std::vector<ui::Field> f = { ui::Field::MakeText(Tr(L"Nome"), g.cur->st.objs[i]->name) };
        if (ui::RunForm(g.frame, Tr(L"Renomear objeto"), f)) g.canvas.RenameObject(i, Trim(f[0].text));
        break;
    }
    case M_OPEN_LINK:
        if (g.cur && g.canvas.selected >= 0 && g.cur->st.objs[g.canvas.selected]->type == ObjType::Link) {
            const std::wstring& u = g.cur->st.objs[g.canvas.selected]->text;
            if (u.rfind(L"http://", 0) == 0 || u.rfind(L"https://", 0) == 0 || u.rfind(L"mailto:", 0) == 0) ShellExecuteW(nullptr, L"open", u.c_str(), nullptr, nullptr, SW_SHOWNORMAL);
        }
        break;
    case M_DELETE_SEL: if (RequireImageDoc()) g.canvas.DeleteSelected(); break;
    case M_SELECTALL:
        if (GetFocus() == g.tray.hwnd) { g.tray.SelectAll(); break; }     // Ctrl+A na bandeja/biblioteca marca todas as capturas
        if (g.libraryOpen) { g.library.SelectAll(); break; }
        if (g.cur && !g.cur->isVideo && g.canvas.tool == Tool::Select && !g.canvas.smartMove && !g.cur->st.objs.empty()) {
            g.canvas.SelectAllObjects();               // with Move, Ctrl+A selects all objects
            break;
        }
        if (RequireImageDoc()) { g.toolbar.SetActive(Tool::Selection); g.canvas.SelectAll(); g.props.Load(); } break;
    case M_FLATTEN: if (RequireImageDoc()) g.canvas.FlattenAll(); break;
    case M_FLATTEN_SEL: if (RequireImageDoc()) g.canvas.FlattenSelected(); break;
    case M_FRONT: g.canvas.BringToFront(); break;
    case M_BACK: g.canvas.SendToBack(); break;
    case M_BRING_FORWARD: g.canvas.BringForward(); break;
    case M_SEND_BACKWARD: g.canvas.SendBackward(); break;
    case M_ALIGN_LEFT: case M_ALIGN_CENTER: case M_ALIGN_RIGHT: case M_ALIGN_TOP: case M_ALIGN_MIDDLE: case M_ALIGN_BOTTOM:
        if (RequireImageDoc()) g.canvas.AlignSelected(id - M_ALIGN_LEFT);
        break;
    case M_FLIP_H: case M_FLIP_V:                    // with an object selected, flips the object; without one, the whole image
        if (RequireImageDoc() && !g.canvas.FlipSelected(id == M_FLIP_H)) ImageCommand(id);
        break;
    case M_GROUP:
        if (!RequireImageDoc()) break;
        if (g.canvas.SelectionCount() < 2) { SetStatus(Tr(L"Selecione dois ou mais objetos (Ctrl+clique ou arraste um retângulo com o Mover) para agrupar.")); break; }
        g.canvas.GroupSelected();
        SetStatus(Tr(L"Objetos agrupados. Imagem › Desagrupar (Ctrl+Shift+U) separa."));
        break;
    case M_UNGROUP:
        if (!RequireImageDoc() || !g.canvas.CanUngroup()) break;
        g.canvas.UngroupSelected();
        SetStatus(Tr(L"Grupo desfeito."));
        break;
    case M_APPLY_TEMPLATE:
        SetStatus(Tr(L"Modelos (combinar imagens) ainda não estão disponíveis no Snapnab."));
        break;
    case M_SNAPPING:
        g.canvas.snapping = !g.canvas.snapping;
        RegSetDword(L"CanvasSnapping", g.canvas.snapping, L"Editor");
        SetStatus(g.canvas.snapping ? Tr(L"Encaixe no canvas ligado (Alt ao arrastar desliga na hora).") : Tr(L"Encaixe no canvas desligado."));
        break;
    case M_TAG: {
        if (!RequireImageDoc()) break;
        std::vector<ui::Field> f = { ui::Field::MakeText(Tr(L"Etiquetas"), g.cur->tags),
                                     ui::Field::MakeLabel(Tr(L"Separe as etiquetas por vírgula. Elas ficam gravadas na captura.")) };
        if (!ui::RunForm(g.frame, Tr(L"Etiquetar imagem"), f)) break;
        std::wstring t = Trim(f[0].text);
        if (t != g.cur->tags) { g.cur->tags = t; g.cur->modified = true; Autosave(g.cur); UpdateDocInfo(); UpdateTitle(); }
        SetStatus(t.empty() ? Tr(L"Etiquetas removidas.") : Tr(L"Etiquetas: ") + t);
        break;
    }
    case M_OCR: GrabText(); break;
    case M_ZOOM_IN: g.canvas.ZoomIn(); break;
    case M_ZOOM_OUT: g.canvas.ZoomOut(); break;
    case M_ZOOM_100: g.canvas.ZoomTo(1.0); break;
    case M_ZOOM_FIT: g.canvas.FitToWindow(); break;
    case M_BG_COLOR: { COLORREF c = g.canvas.background; if (PickColor(g.frame, c)) { g.canvas.background = c; RegSetDword(L"CanvasBackground", c, L"Editor"); g.canvas.Refresh(false); } break; }
    case M_SHOW_TRAY:
        g.showTray = !g.showTray;
        RegSetDword(L"ShowTray", g.showTray, L"Editor");
        Layout();
        break;
    case M_SHORTCUTS: ShowShortcuts(); break;
    case M_MANUAL: ShowManual(); break;
    case M_ABOUT:
        MessageBoxW(g.frame, (L"Snapnab Editor " + std::wstring(kAppVersion) + L"\n" SNAPNAB_COPYRIGHT_W L"\n\n" +
                              Tr(L"Edição de capturas com objetos vetoriais, efeitos, Smart Move,\n"
                                 L"OCR, exportação PDF e edição de vídeo (cortes e GIF).") + L"\n\n" +
                              Tr(L"Software livre sob a GNU General Public License versão 3.") + L"\n\n" + Tr(L"Biblioteca: ") + LibraryDir()).c_str(),
                    Tr(L"Sobre"), MB_ICONINFORMATION);
        break;
    default: ImageCommand(id); break;
    }
}

// ---------------------------------------------------------------- IPC (CMainFrame::OnIPCNotify, ds_pseudocode §4.3)
void ShowEditor() {
    if (IsIconic(g.frame)) ShowWindow(g.frame, SW_RESTORE);
    ShowWindow(g.frame, SW_SHOW);
    SetForegroundWindow(g.frame);
}

void OnIPCNotify(const ipc::Message& m) {
    switch (m.code) {
    case ipc::ShowEditor: case ipc::ShowEditorWithParam: ShowEditor(); break;
    case ipc::CloseEditorIfHidden: if (!IsWindowVisible(g.frame)) PostMessageW(g.frame, WM_CLOSE, 0, 0); break;
    case ipc::ReloadConfig: g.canvas.Refresh(false); break;
    case ipc::NewImageFromClipboard:
        Log(L"Snip detected. New Image From Clipboard commencing.");
        SetTimer(g.frame, 0x1E, 500, nullptr);         // espera o clipboard assentar (Sleep(500) no original)
        break;
    case ipc::OpenFile: if (!m.text.empty()) { OpenPath(m.text); ShowEditor(); } break;
    case ipc::LibraryChanged: g.tray.Reload(); if (g.libraryOpen) g.library.Reload(); break;
    case ipc::RestartEditor:            // language changed: opens the new instance (which waits for this one to close) and closes
        if (LaunchSibling(kEditorExe, RestartArgs())) PostMessageW(g.frame, WM_CLOSE, 0, 0);
        break;
    default: break;
    }
}

// ---------------------------------------------------------------- command line (ProcessUserCmdLineCommands)
void ProcessCommandLine(const std::wstring& cmdLine, bool startup) {
    auto args = SplitCommandLine(cmdLine);
    bool opened = false;
    for (size_t i = 0; i < args.size(); i++) {
        std::wstring a = ToLower(args[i]);
        if (a == L"/open" && i + 1 < args.size()) { opened |= OpenPath(args[++i]); }
        else if (a == L"/newfromclipboard") { NewFromClipboard(); opened = true; }
        else if (a == L"/restartwait") {         // the wait for the previous instance happened at startup (WaitForPreviousInstance)
            if (i + 1 < args.size() && iswdigit(args[i + 1][0])) i++;
        }
        else if (a == L"/help" || a == L"/?") {
            MessageBoxW(nullptr, Tr(L"SnapnabEditor.exe [opções] [arquivo…]\n\n/Open \"arquivo\"     abre um arquivo (.snap, imagem, .mp4)\n"
                                 L"/NewFromClipboard   cria imagem a partir da área de transferência\n/restartwait        espera a instância anterior fechar"),
                        L"Snapnab Editor", MB_ICONINFORMATION);
        }
        else if (a[0] != L'/') opened |= OpenPath(args[i]);
    }
    if (startup && !opened) {
        // opens the most recent capture in the library
        auto items = ListLibrary(1);
        if (!items.empty()) OpenPath(items[0].path);
    }
    if (!startup) ShowEditor();
}

// ---------------------------------------------------------------- library folder watcher (CDirectoryChangeWatcher)
void StartLibraryWatcher() {
    g.watchStop = CreateEventW(nullptr, TRUE, FALSE, nullptr);
    g.watcher = std::thread([] {
        HANDLE ch = FindFirstChangeNotificationW(LibraryDir().c_str(), FALSE, FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE);
        if (ch == INVALID_HANDLE_VALUE) return;
        HANDLE w[2] = { ch, g.watchStop };
        while (WaitForMultipleObjects(2, w, FALSE, INFINITE) == WAIT_OBJECT_0) {
            PostMessageW(g.frame, WM_LIB_CHANGED, 0, 0);
            FindNextChangeNotification(ch);
        }
        FindCloseChangeNotification(ch);
    });
}

// ---------------------------------------------------------------- main window
// subdued status bar (same tones as the capture window's bottom bar) and right panel tabs
void PaintFrame(HDC dc) {
    const auto& th = g.th;
    RECT cr; GetClientRect(g.frame, &cr);
    g.caption.Paint(dc);
    RECT sr{ 0, cr.bottom - S(26), cr.right, cr.bottom };
    HBRUSH b = CreateSolidBrush(th.bgBar); FillRect(dc, &sr, b); DeleteObject(b);
    RECT sl{ 0, sr.top, cr.right, sr.top + 1 };
    HBRUSH lb = CreateSolidBrush(th.line); FillRect(dc, &sl, lb);
    HGDIOBJ of = SelectObject(dc, ui::UiFont(g.dpi, 8));
    SetBkMode(dc, TRANSPARENT); SetTextColor(dc, RGB(70, 70, 78));
    // footer (BottomPanel.Footer): "Recent captures" on the left; image size and zoom, with a menu, on the right
    auto textW = [&](const std::wstring& t) { RECT m{}; DrawTextW(dc, t.c_str(), -1, &m, DT_CALCRECT | DT_SINGLELINE); return (int)m.right; };
    {
        Gdiplus::Graphics fg(dc);
        fg.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
        std::wstring rt = Tr(L"Capturas recentes");
        g.footRecent = { S(6), sr.top + S(3), S(6) + S(30) + textW(rt), sr.bottom - S(3) };
        if (g.showTray) {
            Gdiplus::SolidBrush cb(Gdiplus::Color(255, GetRValue(ui::Mix(th.bgBar, th.control, 0.14)), GetGValue(ui::Mix(th.bgBar, th.control, 0.14)), GetBValue(ui::Mix(th.bgBar, th.control, 0.14))));
            fg.FillRectangle(&cb, Gdiplus::Rect(g.footRecent.left, g.footRecent.top, g.footRecent.right - g.footRecent.left, g.footRecent.bottom - g.footRecent.top));
        }
        int is = S(14), iy = (sr.top + sr.bottom) / 2 - is / 2;
        icons::Draw(fg, icons::Restart, Gdiplus::RectF((float)g.footRecent.left + S(6), (float)iy, (float)is, (float)is), Gdiplus::Color(255, 70, 70, 78));
        RECT tr = g.footRecent; tr.left += S(24);
        DrawTextW(dc, rt.c_str(), -1, &tr, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
    }
    g.footSize = g.footZoom = {};
    int rx = sr.right - S(10);
    if (g.cur && !g.cur->isVideo) {
        std::wstring zt = Format(Tr(L"Zoom %d%%  ▾"), (int)std::lround(g.canvas.zoom * 100)), st = Format(Tr(L"%d × %d px  ▾"), g.cur->w(), g.cur->h());
        g.footZoom = { rx - textW(zt) - S(12), sr.top + S(3), rx, sr.bottom - S(3) };
        rx = g.footZoom.left - S(8);
        g.footSize = { rx - textW(st) - S(12), sr.top + S(3), rx, sr.bottom - S(3) };
        DrawTextW(dc, zt.c_str(), -1, &g.footZoom, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        DrawTextW(dc, st.c_str(), -1, &g.footSize, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        rx = g.footSize.left;
    }
    if (!g.coords.empty() && g.canvas.rulers && g.cur && !g.cur->isVideo) {      // cursor position (rulers on)
        RECT cr2{ rx - S(16) - textW(L"X: 00000   Y: 00000"), sr.top, rx - S(6), sr.bottom };
        DrawTextW(dc, g.coords.c_str(), -1, &cr2, DT_RIGHT | DT_VCENTER | DT_SINGLELINE);
        rx = cr2.left;
    }
    RECT l = sr; l.left = g.footRecent.right + S(14); l.right = rx - S(10);
    DrawTextW(dc, g.status.c_str(), -1, &l, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
    // "Properties | Effects" tabs (segmented control)
    if (!(g.cur && g.cur->isVideo) && !g.libraryOpen) {
        RECT t = TabsRect();
        HBRUSH tb = CreateSolidBrush(th.bgMain); FillRect(dc, &t, tb); DeleteObject(tb);
        RECT vl{ t.left, t.top, t.left + 1, cr.bottom - S(26) - (g.showTray ? g.tray.Height() : 0) };
        FillRect(dc, &vl, lb);
        Gdiplus::Graphics gr(dc);
        gr.SetSmoothingMode(Gdiplus::SmoothingModeAntiAlias);
        gr.SetPixelOffsetMode(Gdiplus::PixelOffsetModeHalf);
        RECT a = TabRect(0), z = TabRect(2);
        RECT all{ a.left, a.top, z.right, z.bottom };
        auto round = [&](RECT rc, COLORREF c, int rad) {
            Gdiplus::GraphicsPath p;
            float d = (float)S(rad) * 2, x = (float)rc.left, y = (float)rc.top, w = (float)(rc.right - rc.left), h = (float)(rc.bottom - rc.top);
            p.AddArc(x, y, d, d, 180, 90); p.AddArc(x + w - d, y, d, d, 270, 90);
            p.AddArc(x + w - d, y + h - d, d, d, 0, 90); p.AddArc(x, y + h - d, d, d, 90, 90);
            p.CloseFigure();
            Gdiplus::SolidBrush br(Gdiplus::Color(255, GetRValue(c), GetGValue(c), GetBValue(c)));
            gr.FillPath(&br, &p);
        };
        round(all, th.bgSide, 7);
        RECT on = TabRect(std::clamp(g.rightTab, 0, 2));
        InflateRect(&on, -S(3), -S(3));
        RECT sh = on; OffsetRect(&sh, 0, 1);
        round(sh, ui::Mix(th.bgSide, 0, 0.08), 5);
        round(on, RGB(255, 255, 255), 5);
        for (int i = 0; i < 3; i++) {
            RECT tr = TabRect(i);
            ui::DrawTextCentered(dc, i == 0 ? Tr(L"Propriedades") : i == 1 ? Tr(L"Efeitos") : Tr(L"Camadas"), tr,
                                 g.rightTab == i ? th.accentText : RGB(80, 80, 88), ui::UiFont(g.dpi, 9, g.rightTab == i));
        }
    }
    DeleteObject(lb);
    SelectObject(dc, of);
}

void LoadTheme() {
    g.th = ui::LoadThemeColors();
    SetEdTheme(g.th);
    ui::ApplyThemeToWidgets(g.th);
    g.caption.SetTheme(g.th);
    g.props.ThemeChanged();
    COLORREF border = g.th.captionAccent ? 0xFFFFFFFF /*DWMWA_COLOR_DEFAULT*/ : g.th.line;
    DwmSetWindowAttribute(g.frame, 34 /*DWMWA_BORDER_COLOR*/, &border, sizeof border);
    RedrawWindow(g.frame, nullptr, nullptr, RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN);
}

// ---------------------------------------------------------------- effects panel (CImageEffectHandler, ed_comp_5 §4)
// Each effect has its settings (CEffectSettings*); the same function produces the panel thumbnail, the live preview on
// the image and the applied result. Pixel values are scaled by k (the thumbnail is smaller than the document).
std::vector<EffectsPanel::Effect> MakeEffects() {
    using F = FxParam;
    auto choice = [](std::initializer_list<int> v, const wchar_t* suffix) {
        std::vector<std::wstring> o; for (int x : v) o.push_back(std::to_wstring(x) + suffix); return o;
    };
    auto signedPct = [](std::initializer_list<int> v) {
        std::vector<std::wstring> o; for (int x : v) o.push_back((x > 0 ? L"+" : L"") + std::to_wstring(x) + L"%"); return o;
    };
    std::vector<EffectsPanel::Effect> e = {
        { M_FX_BORDER, Tr(L"Borda"), Tr(L"Contorno sólido em volta da imagem, na largura e cor que você escolher."), Tr(L"Bordas e sombras"),
          { F::MakeChoice(Tr(L"Largura"), choice({ 1, 2, 3, 4, 6, 8, 10, 12, 16, 20, 24, 32 }, L" px"), { 1, 2, 3, 4, 6, 8, 10, 12, 16, 20, 24, 32 }, 4),
            F::MakeColor(Tr(L"Cor"), RGB(45, 45, 48)) } },
        { M_FX_SHADOW, Tr(L"Sombra"), Tr(L"Sombra suave atrás da imagem, que destaca a captura do fundo."), Tr(L"Bordas e sombras"),
          { F::MakeChoice(Tr(L"Deslocamento"), choice({ 0, 2, 4, 6, 8, 12, 16, 20 }, L" px"), { 0, 2, 4, 6, 8, 12, 16, 20 }, 6),
            F::MakeChoice(Tr(L"Desfoque"), choice({ 2, 4, 6, 8, 12, 16, 24 }, L" px"), { 2, 4, 6, 8, 12, 16, 24 }, 8),
            F::MakeChoice(Tr(L"Opacidade"), choice({ 20, 35, 50, 65, 80 }, L"%"), { 20, 35, 50, 65, 80 }, 50) } },
        { M_FX_PERSPECTIVE, Tr(L"Perspectiva"), Tr(L"Inclina a imagem como se estivesse de lado, em pé ou deitada."), Tr(L"Bordas e sombras"),
          { F::MakeChoice(Tr(L"Horizontal"), signedPct({ -60, -45, -30, -15, 0, 15, 30, 45, 60 }), { -60, -45, -30, -15, 0, 15, 30, 45, 60 }, 30),
            F::MakeChoice(Tr(L"Vertical"), signedPct({ -60, -45, -30, -15, 0, 15, 30, 45, 60 }), { -60, -45, -30, -15, 0, 15, 30, 45, 60 }, 0),
            F::MakeChoice(Tr(L"Inclinação"), signedPct({ -40, -20, 0, 20, 40 }), { -40, -20, 0, 20, 40 }, 0) } },
        { M_FX_PAGECURL, Tr(L"Dobra de página"), Tr(L"Dobra um canto da imagem, como a ponta de uma folha de papel."), Tr(L"Bordas e sombras"),
          { F::MakeChoice(Tr(L"Tamanho"), { Tr(L"Pequena"), Tr(L"Média"), Tr(L"Grande") }, { 10, 18, 28 }, 18),
            F::MakeChoice(Tr(L"Canto"), { Tr(L"Superior esquerdo"), Tr(L"Superior direito"), Tr(L"Inferior direito"), Tr(L"Inferior esquerdo") }, { 0, 1, 2, 3 }, 2) } },
        { M_FX_EDGE, Tr(L"Bordas"), Tr(L"Rasgada, onda, serrilhada, tubarão, esmaecida ou chanfrada."), Tr(L"Bordas e sombras"),
          { F::MakeChoice(Tr(L"Estilo"), { Tr(L"Rasgada"), Tr(L"Onda"), Tr(L"Dente de serra"), Tr(L"Dente de tubarão"), Tr(L"Esmaecer"), Tr(L"Chanfrada") }, { 0, 1, 2, 3, 4, 5 }, 0),
            F::MakeChoice(Tr(L"Tamanho"), choice({ 6, 10, 14, 20, 28, 40 }, L" px"), { 6, 10, 14, 20, 28, 40 }, 14),
            F::MakeToggle(Tr(L"Em cima"), false), F::MakeToggle(Tr(L"Direita"), true), F::MakeToggle(Tr(L"Embaixo"), true), F::MakeToggle(Tr(L"Esquerda"), false),
            F::MakeToggle(Tr(L"Sombra"), true) } },
        { M_FX_FILTER, Tr(L"Filtros"), Tr(L"Cinza, sépia, inverter, P&B, relevo, mosaico, nitidez, desfoque."), Tr(L"Cores"),
          { F::MakeChoice(Tr(L"Filtro"), { Tr(L"Tons de cinza"), Tr(L"Sépia"), Tr(L"Inverter cores"), Tr(L"Preto e branco"), Tr(L"Relevo"), Tr(L"Mosaico"), Tr(L"Nitidez"), Tr(L"Desfoque") },
                          { 0, 1, 2, 3, 4, 5, 6, 7 }, 0),
            F::MakeChoice(Tr(L"Intensidade"), { Tr(L"Baixa"), Tr(L"Média"), Tr(L"Alta") }, { 1, 2, 3 }, 2) } },
        { M_FX_ADJUST, Tr(L"Ajuste de cores"), Tr(L"Brilho, contraste, gama, matiz e saturação."), Tr(L"Cores"),
          { F::MakeChoice(Tr(L"Brilho"), signedPct({ -60, -40, -20, -10, 0, 10, 20, 40, 60 }), { -60, -40, -20, -10, 0, 10, 20, 40, 60 }, 0),
            F::MakeChoice(Tr(L"Contraste"), signedPct({ -60, -40, -20, -10, 0, 10, 20, 40, 60 }), { -60, -40, -20, -10, 0, 10, 20, 40, 60 }, 20),
            F::MakeChoice(Tr(L"Gama"), { L"0,5", L"0,7", L"0,85", L"1,0", L"1,2", L"1,5", L"2,0" }, { 50, 70, 85, 100, 120, 150, 200 }, 100),
            F::MakeChoice(Tr(L"Matiz"), { L"−90°", L"−45°", L"0°", L"+45°", L"+90°", L"+180°" }, { -90, -45, 0, 45, 90, 180 }, 0),
            F::MakeChoice(Tr(L"Saturação"), signedPct({ -100, -60, -30, 0, 30, 60, 100 }), { -100, -60, -30, 0, 30, 60, 100 }, 30) } },
        { M_FX_REPLACE, Tr(L"Substituição de cor"), Tr(L"Troca uma cor por outra (ou por transparente)."), Tr(L"Cores"),
          { F::MakeColor(Tr(L"Cor atual"), RGB(255, 255, 255)), F::MakeColor(Tr(L"Nova cor"), RGB(255, 244, 196)), F::MakeToggle(Tr(L"Tornar transparente"), false),
            F::MakeChoice(Tr(L"Tolerância"), { L"0", L"5", L"10", L"20", L"35", L"50" }, { 0, 5, 10, 20, 35, 50 }, 10) } },
        { M_FX_SPOTLIGHT, Tr(L"Spotlight e lupa"), Tr(L"Destaca a seleção (ou o meio da imagem) e escurece o resto."), Tr(L"Destaque"),
          { F::MakeChoice(Tr(L"Escurecer"), choice({ 0, 20, 35, 50, 65, 80 }, L"%"), { 0, 20, 35, 50, 65, 80 }, 50),
            F::MakeChoice(Tr(L"Desfoque"), choice({ 0, 2, 4, 8, 12 }, L" px"), { 0, 2, 4, 8, 12 }, 4),
            F::MakeChoice(Tr(L"Ampliar"), choice({ 100, 125, 150, 200 }, L"%"), { 100, 125, 150, 200 }, 100),
            F::MakeToggle(Tr(L"Sombra na área em foco"), true) } },
        { M_FX_WATERMARK, Tr(L"Marca d'água"), Tr(L"Texto translúcido sobre a imagem, uma vez ou em mosaico."), Tr(L"Texto"),
          { F::MakeText(Tr(L"Texto"), Tr(L"Confidencial")), F::MakeChoice(Tr(L"Opacidade"), choice({ 15, 25, 35, 50, 70 }, L"%"), { 15, 25, 35, 50, 70 }, 35),
            F::MakeToggle(Tr(L"Repetir em mosaico"), true), F::MakeToggle(Tr(L"Relevo"), false) } },
        { M_FX_CAPTION, Tr(L"Informações da captura"), Tr(L"Faixa de texto em cima ou embaixo; aceita {date} e {time}."), Tr(L"Texto"),
          { F::MakeText(Tr(L"Texto"), L"{date} {time} — {user}@{computer}"), F::MakeChoice(Tr(L"Posição"), { Tr(L"Embaixo"), Tr(L"Em cima") }, { 0, 1 }, 0),
            F::MakeChoice(Tr(L"Tamanho"), choice({ 10, 12, 14, 16, 20, 24, 32 }, L" px"), { 10, 12, 14, 16, 20, 24, 32 }, 16),
            F::MakeColor(Tr(L"Cor do texto"), RGB(255, 255, 255)), F::MakeColor(Tr(L"Cor da faixa"), RGB(32, 32, 36)) } },
    };
    return e;
}

int PV(const std::vector<FxParam>& p, size_t i) { return i < p.size() ? p[i].value : 0; }
COLORREF PCol(const std::vector<FxParam>& p, size_t i) { return i < p.size() ? p[i].color : 0; }
std::wstring PTxt(const std::vector<FxParam>& p, size_t i) { return i < p.size() ? p[i].text : L""; }

// Spotlight focus area: the selection (Selection tool) or, without a selection, the middle of the image
RECT SpotlightFocus(int w, int h) {
    if (g.canvas.HasRasterSelection()) return g.canvas.RasterSelectionBounds();
    return { w / 4, h / 4, w * 3 / 4, h * 3 / 4 };
}

// applies a panel effect; off returns how far the objects must move (effects that enlarge the image)
Image ApplyFx(int cmd, const std::vector<FxParam>& p, const Image& src, double k, POINT* off, int docW, int docH) {
    auto px = [k](int v) { return std::max(v > 0 ? 1 : 0, (int)std::lround(v * k)); };
    if (off) *off = { 0, 0 };
    Image img = src;
    switch (cmd) {
    case M_FX_BORDER: { int w = px(PV(p, 0)); if (off) *off = { w, w }; return fx::Border(img, w, FromColorref(PCol(p, 1))); }
    case M_FX_SHADOW: {
        int blur = std::max(1, px(PV(p, 1)));
        if (off) *off = { blur * 2, blur * 2 };
        return fx::Shadow(img, px(PV(p, 0)), blur, PV(p, 2));
    }
    case M_FX_PERSPECTIVE: return fx::Perspective(img, PV(p, 0), PV(p, 1), PV(p, 2));
    case M_FX_PAGECURL: return fx::PageCurl(img, std::min(img.w, img.h) * PV(p, 0) / 100, PV(p, 1));
    case M_FX_EDGE: {
        int sides = (PV(p, 2) ? fx::Top : 0) | (PV(p, 3) ? fx::Right : 0) | (PV(p, 4) ? fx::Bottom : 0) | (PV(p, 5) ? fx::Left : 0);
        return fx::EdgeEffect(img, (fx::Edge)PV(p, 0), std::max(2, px(PV(p, 1))), sides ? sides : fx::Bottom | fx::Right, PV(p, 6) != 0);
    }
    case M_FX_FILTER: {
        int in = std::clamp(PV(p, 1), 1, 3);
        switch (PV(p, 0)) {
        case 0: fx::Grayscale(img); break;
        case 1: fx::Sepia(img); break;
        case 2: fx::Invert(img); break;
        case 3: fx::Monochrome(img, 168 - in * 20); break;
        case 4: fx::Emboss(img, in * 3); break;
        case 5: fx::Pixelate(img, { 0, 0, img.w, img.h }, std::max(2, px(in * 6))); break;
        case 6: fx::Sharpen(img, in * 33); break;
        case 7: fx::BoxBlur(img, { 0, 0, img.w, img.h }, std::max(1, px(in * 2))); break;
        }
        return img;
    }
    case M_FX_ADJUST:
        fx::BrightnessContrast(img, PV(p, 0), PV(p, 1));
        if (PV(p, 2) != 100) fx::Gamma(img, PV(p, 2) / 100.0);
        if (PV(p, 3) || PV(p, 4)) fx::HueSaturation(img, PV(p, 3), PV(p, 4));
        return img;
    case M_FX_REPLACE:
        fx::ReplaceColor(img, FromColorref(PCol(p, 0)), PV(p, 2) ? 0 : FromColorref(PCol(p, 1)), PV(p, 3));
        return img;
    case M_FX_SPOTLIGHT: {
        RECT f = SpotlightFocus(docW, docH);
        RECT fk{ (LONG)std::lround(f.left * k), (LONG)std::lround(f.top * k), (LONG)std::lround(f.right * k), (LONG)std::lround(f.bottom * k) };
        return fx::Spotlight(img, fk, PV(p, 0), px(PV(p, 1)), PV(p, 2), PV(p, 3) != 0);
    }
    case M_FX_WATERMARK: fx::Watermark(img, PTxt(p, 0), PV(p, 1), PV(p, 2) != 0, PV(p, 3) != 0); return img;
    case M_FX_CAPTION: {
        Image r = fx::Caption(img, PTxt(p, 0), PV(p, 1) == 1, std::max(6, px(PV(p, 2))), FromColorref(PCol(p, 3)), FromColorref(PCol(p, 4)));
        if (off && PV(p, 1) == 1) *off = { 0, r.h - img.h };
        return r;
    }
    }
    return img;
}

// quick effect: border → edge effect → page curl → shadow (a "bundle", like CInteropQuickEffect)
Image ApplyQuick(const QuickEffect& q, const Image& src, double k, POINT* off) {
    auto px = [k](int v) { return std::max(v > 0 ? 1 : 0, (int)std::lround(v * k)); };
    Image img = src;
    int shift = 0;
    if (q.border) { int w = px(q.borderW); img = fx::Border(img, w, FromColorref(q.borderColor)); shift += w; }
    if (q.edge) img = fx::EdgeEffect(img, (fx::Edge)q.edgeStyle, std::max(2, px(q.edgeSize)), q.edgeSides ? q.edgeSides : fx::Bottom | fx::Right, false);
    if (q.curl) img = fx::PageCurl(img, std::min(img.w, img.h) * q.curlSize / 100, q.curlCorner);
    if (q.shadow) { int blur = std::max(1, px(q.shadowBlur)); img = fx::Shadow(img, px(q.shadowOffset), blur, q.shadowOpacity); shift += blur * 2; }
    if (off) *off = { shift, shift };
    return img;
}

// QuickEffects.xml in the data folder (AddToQuickEffects saves the current panel state as a new preset)
std::wstring QuickFile() { return AppDataDir() + L"\\QuickEffects.xml"; }

std::vector<QuickEffect> DefaultQuickEffects() {
    std::vector<QuickEffect> v(8);
    v[0].name = TR_NOOP(L"Sombra"); v[0].shadow = true; v[0].shadowOffset = 6; v[0].shadowBlur = 8;
    v[1].name = TR_NOOP(L"Borda e sombra"); v[1].border = true; v[1].borderW = 3; v[1].borderColor = RGB(90, 90, 96); v[1].shadow = true;
    v[2].name = TR_NOOP(L"Papel rasgado"); v[2].edge = true; v[2].edgeStyle = 0; v[2].edgeSize = 14; v[2].shadow = true; v[2].shadowOffset = 4;
    v[3].name = TR_NOOP(L"Dobra de página"); v[3].curl = true; v[3].curlSize = 20; v[3].shadow = true; v[3].shadowOffset = 4; v[3].shadowBlur = 6; v[3].shadowOpacity = 40;
    v[4].name = TR_NOOP(L"Onda"); v[4].edge = true; v[4].edgeStyle = 1; v[4].edgeSize = 10; v[4].edgeSides = fx::All;
    v[5].name = TR_NOOP(L"Serrilhado"); v[5].edge = true; v[5].edgeStyle = 2; v[5].edgeSize = 12; v[5].edgeSides = fx::Bottom; v[5].shadow = true; v[5].shadowOffset = 4;
    v[6].name = TR_NOOP(L"Esmaecer"); v[6].edge = true; v[6].edgeStyle = 4; v[6].edgeSize = 24; v[6].edgeSides = fx::All;
    v[7].name = TR_NOOP(L"Chanfrado"); v[7].edge = true; v[7].edgeStyle = 5; v[7].edgeSize = 10; v[7].edgeSides = fx::All; v[7].border = true; v[7].borderW = 1; v[7].borderColor = RGB(60, 60, 66);
    return v;
}

void LoadQuickEffects() {
    g.quick.clear();
    XmlNode root;
    if (XmlLoadFile(QuickFile(), root) && root.name == "QuickEffects") {
        for (auto& c : root.children) {
            if (c.name != "QuickEffect") continue;
            QuickEffect q;
            q.name = c.getW("Name", Tr(L"Efeito"));
            q.border = c.getb("Border"); q.borderW = std::clamp(c.geti("BorderWidth", 4), 1, 100); q.borderColor = HexToColor(c.getW("BorderColor"), q.borderColor);
            q.edge = c.getb("Edge"); q.edgeStyle = std::clamp(c.geti("EdgeStyle"), 0, 5); q.edgeSize = std::clamp(c.geti("EdgeSize", 14), 2, 100);
            q.edgeSides = c.geti("EdgeSides", q.edgeSides) & 15;
            q.shadow = c.getb("Shadow"); q.shadowOffset = std::clamp(c.geti("ShadowOffset", 6), 0, 60);
            q.shadowBlur = std::clamp(c.geti("ShadowBlur", 6), 1, 60); q.shadowOpacity = std::clamp(c.geti("ShadowOpacity", 50), 5, 100);
            q.curl = c.getb("PageCurl"); q.curlSize = std::clamp(c.geti("PageCurlSize", 18), 5, 45); q.curlCorner = std::clamp(c.geti("PageCurlPos", 2), 0, 3);
            g.quick.push_back(q);
        }
    } else g.quick = DefaultQuickEffects();
}

void SaveQuickEffects() {
    XmlNode root("QuickEffects");
    for (auto& q : g.quick) {
        XmlNode& c = root.add("QuickEffect");
        c.setW("Name", q.name);
        c.seti("Border", q.border).seti("BorderWidth", q.borderW).setW("BorderColor", ColorToHex(q.borderColor));
        c.seti("Edge", q.edge).seti("EdgeStyle", q.edgeStyle).seti("EdgeSize", q.edgeSize).seti("EdgeSides", q.edgeSides);
        c.seti("Shadow", q.shadow).seti("ShadowOffset", q.shadowOffset).seti("ShadowBlur", q.shadowBlur).seti("ShadowOpacity", q.shadowOpacity);
        c.seti("PageCurl", q.curl).seti("PageCurlSize", q.curlSize).seti("PageCurlPos", q.curlCorner);
    }
    XmlSaveFile(QuickFile(), root);
}

void RefreshQuickGallery() {
    std::vector<std::wstring> names;
    for (auto& q : g.quick) names.push_back(Tr(q.name));      // the built-in ones, translated
    g.effects.SetQuick(names);
}

// writes an effect's result to the document: effects that only enlarge the image keep the objects editable
// (moving along); the others flatten the objects first, so the preview and the result are the same
void CommitEffect(const std::wstring& name, bool keepObjects, std::function<Image(const Image&, POINT*)> run) {
    if (!RequireImageDoc()) return;
    Document* d = g.cur;
    d->Checkpoint();
    if (!keepObjects) FlattenIfNeeded();
    POINT off{};
    d->SetBase(run(*d->st.base, &off));
    if (off.x || off.y) TransformObjects([off](Gdiplus::PointF p) { return Gdiplus::PointF(p.X + off.x, p.Y + off.y); });
    g.canvas.SetEffectPreview(nullptr);
    g.canvas.selected = std::min(g.canvas.selected, (int)d->st.objs.size() - 1);
    g.canvas.FitToWindow();
    g.canvas.Refresh();
    DocChanged();
    g.props.Load();
    SetStatus(Tr(L"Efeito aplicado: ") + name + Tr(L" (Ctrl+Z desfaz)"));
}

void PreviewEffect(int cmd, const std::vector<FxParam>& p) {
    if (!g.cur || g.cur->isVideo) return;
    Image src = g.cur->Render();
    g.canvas.SetEffectPreview(std::make_shared<Image>(ApplyFx(cmd, p, src, 1.0, nullptr, g.cur->w(), g.cur->h())));
    if (cmd == M_FX_SPOTLIGHT && !g.canvas.HasRasterSelection())
        SetStatus(Tr(L"Spotlight: sem seleção, o foco é o meio da imagem. Use a ferramenta Seleção para escolher a área."));
}

void ApplyEffect(int cmd, const std::vector<FxParam>& p) {
    const auto* e = g.effects.Find(cmd);
    bool keep = cmd == M_FX_BORDER || cmd == M_FX_SHADOW || cmd == M_FX_CAPTION;
    int w = g.cur ? g.cur->w() : 0, h = g.cur ? g.cur->h() : 0;
    CommitEffect(e ? e->name : L"", keep, [&](const Image& base, POINT* off) { return ApplyFx(cmd, p, base, 1.0, off, w, h); });
}

void ApplyQuickEffect(int i) {
    if (i < 0 || i >= (int)g.quick.size()) return;
    QuickEffect q = g.quick[i];
    CommitEffect(Tr(q.name), !q.edge && !q.curl, [&](const Image& base, POINT* off) { return ApplyQuick(q, base, 1.0, off); });
}

// "Add to quick effects": combines the current Border, Edge effects, Shadow and Page curl settings
void AddQuickEffect() {
    auto par = [](int cmd) { const auto* e = g.effects.Find(cmd); return e ? e->params : std::vector<FxParam>{}; };
    auto b = par(M_FX_BORDER), ed = par(M_FX_EDGE), sh = par(M_FX_SHADOW), cu = par(M_FX_PAGECURL);
    std::vector<ui::Field> f = {
        ui::Field::MakeText(Tr(L"Nome"), Format(Tr(L"Meu efeito %zu"), g.quick.size() + 1)),
        ui::Field::MakeLabel(Tr(L"Os ajustes vêm dos efeitos do painel (abra cada um para mudar):")),
        ui::Field::MakeCheck(Tr(L"Borda"), false), ui::Field::MakeCheck(Tr(L"Bordas especiais"), false),
        ui::Field::MakeCheck(Tr(L"Sombra"), true), ui::Field::MakeCheck(Tr(L"Dobra de página"), false),
    };
    if (!ui::RunForm(g.frame, Tr(L"Adicionar aos efeitos rápidos"), f) || Trim(f[0].text).empty()) return;
    QuickEffect q;
    q.name = Trim(f[0].text);
    q.border = f[2].value != 0; q.borderW = PV(b, 0); q.borderColor = PCol(b, 1);
    q.edge = f[3].value != 0; q.edgeStyle = PV(ed, 0); q.edgeSize = PV(ed, 1);
    q.edgeSides = (PV(ed, 2) ? fx::Top : 0) | (PV(ed, 3) ? fx::Right : 0) | (PV(ed, 4) ? fx::Bottom : 0) | (PV(ed, 5) ? fx::Left : 0);
    q.shadow = f[4].value != 0; q.shadowOffset = PV(sh, 0); q.shadowBlur = PV(sh, 1); q.shadowOpacity = PV(sh, 2);
    q.curl = f[5].value != 0; q.curlSize = PV(cu, 0); q.curlCorner = PV(cu, 1);
    if (!q.border && !q.edge && !q.shadow && !q.curl) return;
    g.quick.push_back(q);
    SaveQuickEffects();
    RefreshQuickGallery();
    SetStatus(Tr(L"Efeito rápido adicionado: ") + q.name);
}

// library items → files to drag: images as temporary PNGs (with the objects), videos as they are
std::vector<std::wstring> FilesForDrag(const std::vector<std::wstring>& paths) {
    std::vector<std::wstring> out;
    wchar_t tmp[MAX_PATH]; GetTempPathW(MAX_PATH, tmp);
    std::wstring dir = std::wstring(tmp) + L"Snapnab";
    EnsureDir(dir);
    for (auto& p : paths) {
        if (FileExt(p) == L".mp4") { out.push_back(p); continue; }
        Image img;
        if (Document* d = FindDoc(p)) img = d->Render();
        else if (auto doc = Document::Load(p)) img = doc->Render();
        if (img.empty()) continue;
        WIN32_FILE_ATTRIBUTE_DATA fa{};
        GetFileAttributesExW(p.c_str(), GetFileExInfoStandard, &fa);
        SYSTEMTIME st, lt; FileTimeToSystemTime(&fa.ftLastWriteTime, &st); SystemTimeToTzSpecificLocalTime(nullptr, &st, &lt);
        std::wstring base = dir + Format(L"\\Captura_%04d-%02d-%02d_%02d-%02d-%02d", lt.wYear, lt.wMonth, lt.wDay, lt.wHour, lt.wMinute, lt.wSecond);
        std::wstring f = base + L".png";
        for (int n = 2; std::find(out.begin(), out.end(), f) != out.end(); n++) f = base + Format(L"_%d.png", n);
        if (SaveImageFile(f, img)) out.push_back(f);
    }
    return out;
}

// canvas context menu (right click): commands for the selected objects, or paste/selection with none
void CanvasContextMenu(POINT pt) {
    if (!g.cur || g.cur->isVideo) return;
    HMENU m = CreatePopupMenu();
    auto add = [&](int id, const std::wstring& t, bool on = true) { AppendMenuW(m, MF_STRING | (on ? 0 : MF_GRAYED), id, t.c_str()); };
    bool canPaste = ClipboardHasImage() || IsClipboardFormatAvailable(ObjectsFormat()) || IsClipboardFormatAvailable(CF_UNICODETEXT) ||
                    IsClipboardFormatAvailable(CF_HDROP);
    auto v = g.canvas.SelectedList();
    if (!v.empty()) {
        const DrawObj& o = *g.cur->st.objs[g.canvas.selected];
        bool locked = g.canvas.SelectionLocked();
        if (o.type == ObjType::Link) { add(M_OPEN_LINK, Tr(L"Abrir o link")); AppendMenuW(m, MF_SEPARATOR, 0, nullptr); }
        if (o.floating) { add(M_DROP, Tr(L"Soltar a peça na imagem\tEnter")); AppendMenuW(m, MF_SEPARATOR, 0, nullptr); }
        add(M_CUT, Tr(L"Recor&tar\tCtrl+X"), !locked); add(M_COPY, Tr(L"&Copiar\tCtrl+C"));
        add(M_PASTE, Tr(L"C&olar\tCtrl+V"), canPaste); add(M_PASTE_INPLACE, Tr(L"Colar no &mesmo lugar\tCtrl+Shift+V"), canPaste);
        add(M_DELETE_SEL, Tr(L"E&xcluir\tDel"), !locked);
        AppendMenuW(m, MF_SEPARATOR, 0, nullptr);
        if (locked) add(M_UNLOCK, Tr(L"Desblo&quear\tCtrl+Shift+K")); else add(M_LOCK, Tr(L"&Bloquear\tCtrl+L"));
        SetItemIcon(m, locked ? M_UNLOCK : M_LOCK, locked ? icons::Unlock : icons::Lock);
        if (v.size() == 1) add(M_RENAME_OBJ, Tr(L"Re&nomear…"));
        AppendMenuW(m, MF_SEPARATOR, 0, nullptr);
        add(M_FRONT, Tr(L"Trazer para o &primeiro plano")); add(M_BRING_FORWARD, Tr(L"Trazer para a &frente\tCtrl+Shift+F"));
        add(M_SEND_BACKWARD, Tr(L"Enviar para &trás\tCtrl+Shift+B")); add(M_BACK, Tr(L"Enviar para o plano de &fundo"));
        if (v.size() >= 2) {
            AppendMenuW(m, MF_SEPARATOR, 0, nullptr);
            add(M_ALIGN_LEFT, Tr(L"Alinhar à esquerda")); add(M_ALIGN_CENTER, Tr(L"Alinhar ao centro")); add(M_ALIGN_RIGHT, Tr(L"Alinhar à direita"));
            add(M_ALIGN_TOP, Tr(L"Alinhar em cima")); add(M_ALIGN_MIDDLE, Tr(L"Alinhar no meio")); add(M_ALIGN_BOTTOM, Tr(L"Alinhar embaixo"));
            add(M_DIST_H, Tr(L"Distribuir na horizontal"), v.size() >= 3); add(M_DIST_V, Tr(L"Distribuir na vertical"), v.size() >= 3);
            AppendMenuW(m, MF_SEPARATOR, 0, nullptr);
            add(M_GROUP, Tr(L"A&grupar\tCtrl+Shift+G"));
        }
        if (g.canvas.CanUngroup()) add(M_UNGROUP, Tr(L"Desagr&upar\tCtrl+Shift+U"));
    } else {
        add(M_PASTE, Tr(L"C&olar\tCtrl+V"), canPaste); add(M_PASTE_INPLACE, Tr(L"Colar no &mesmo lugar\tCtrl+Shift+V"), canPaste);
        if (g.canvas.HasRasterSelection()) {
            AppendMenuW(m, MF_SEPARATOR, 0, nullptr);
            add(M_COPY, Tr(L"&Copiar\tCtrl+C")); add(M_CROP_SEL, Tr(L"Cortar para a &seleção"));
            add(M_LIFT, Tr(L"&Levantar a seleção (peça solta)\tCtrl+Shift+L"));
        }
        if (g.canvas.HasFloating()) add(M_DROP, Tr(L"Soltar a peça na imagem\tEnter"));
        AppendMenuW(m, MF_SEPARATOR, 0, nullptr);
        add(M_SELECTALL, Tr(L"Selecionar tu&do\tCtrl+A"));
    }
    int cmd = TrackPopupMenu(m, TPM_RETURNCMD | TPM_RIGHTBUTTON, pt.x, pt.y, 0, g.frame, nullptr);
    DestroyMenu(m);
    if (cmd) OnCommand(cmd);
}

void CreateChildren() {
    g.toolbar.Create(g.frame);
    using I = icons::Id;
    auto& it = g.toolbar.items;
    it.push_back({ T_CAPTURE, I::Capture, Tr(L"Capturar"), Tr(L"Nova captura de imagem (pede ao Snapnab)"), false, true });
    it.push_back({ T_RECORD, I::VideoFill, Tr(L"Gravar"), Tr(L"Nova gravação de vídeo"), false, false });
    it.push_back({ T_COPYALL, I::Copy, Tr(L"Copiar tudo"), Tr(L"Copiar a imagem inteira (Ctrl+Shift+C)"), true, false });
    it.push_back({ T_SAVE, I::Save, Tr(L"Salvar como"), Tr(L"Salvar como arquivo (Ctrl+Shift+S)"), true, false });
    it.push_back({ T_SHARE, I::Share, Tr(L"Compartilhar"), Tr(L"Compartilhar: arquivo, área de transferência, e-mail, impressora, programa"), true, false });
    // effects panel before the toolbar: the toolbar's effect items use its names
    g.effects.Create(g.frame);
    g.effects.effects = MakeEffects();
    g.effects.makePreview = [](int cmd, const std::vector<FxParam>& p, const Image& src) {
        double k = g.cur && !g.cur->isVideo && g.cur->w() > 0 ? (double)src.w / g.cur->w() : 0.35;
        int w = g.cur ? g.cur->w() : src.w, h = g.cur ? g.cur->h() : src.h;
        if (cmd >= 0) return ApplyFx(cmd, p, src, k, nullptr, w, h);
        int i = -cmd - 1;
        return i < (int)g.quick.size() ? ApplyQuick(g.quick[i], src, k, nullptr) : src;
    };
    g.effects.onPreview = PreviewEffect;
    g.effects.onApply = ApplyEffect;
    g.effects.onCancel = [] { g.canvas.SetEffectPreview(nullptr); };
    g.effects.onQuick = ApplyQuickEffect;
    g.effects.onAddQuick = AddQuickEffect;
    g.effects.onDeleteQuick = [](int i) {
        if (i < 0 || i >= (int)g.quick.size()) return;
        if (MessageBoxW(g.frame, Format(Tr(L"Excluir o efeito rápido \"%s\"?"), Tr(g.quick[i].name).c_str()).c_str(), L"Snapnab Editor",
                        MB_YESNO | MB_ICONQUESTION) != IDYES) return;
        g.quick.erase(g.quick.begin() + i);
        SaveQuickEffects();
        RefreshQuickGallery();
    };
    LoadQuickEffects();
    RefreshQuickGallery();

    g.toolbar.describe = DescribeEntry;
    LoadToolbarConfig();
    g.toolbar.SetCenter(CenterEntries());
    g.toolbar.onClick = [](int id) { OnCommand(id); };
    g.toolbar.onEntry = [](const ToolbarEntry& e) {
        switch (e.type) {
        case ToolbarEntry::Tool: SetTool((Tool)e.id); break;
        case ToolbarEntry::Effect: OpenEffect(e.id); break;    // effect item: opens its settings in the panel
        default: OnCommand(e.id); break;
        }
    };
    g.toolbar.onMore = [](POINT p) { MoreToolsMenu(p); };
    g.toolbar.onReorder = [](std::vector<ToolbarEntry> v) {   // an item was dragged: the bar becomes custom
        g.tbCustom = true; g.tbCustomList = std::move(v);
        SaveToolbarConfig();
        SetStatus(Tr(L"Barra de ferramentas personalizada. Mais › Voltar ao padrão desfaz."));
    };
    g.toolbar.onHint = [](const std::wstring& h) { if (!h.empty()) SetStatus(h); };

    QS().Load();                                   // tool themes and favorites
    g.canvas.Create(g.frame);
    for (int t = 0; t < (int)Tool::Count; t++) {   // built-in: the theme's first quick style
        auto qs = QS().StylesFor((Tool)t);
        if (!qs.empty()) g.canvas.styles[(Tool)t] = qs[0];
    }
    LoadToolStyles(g.canvas.styles);               // each tool's styles from the previous session
    g.canvas.smartMove = RegGetDword(L"SmartMove", 0, L"Editor") != 0;
    g.canvas.snapping = RegGetDword(L"CanvasSnapping", 1, L"Editor") != 0;
    g.canvas.rulers = RegGetDword(L"Rulers", 0, L"Editor") != 0;               // off by default
    g.canvas.edgeSnap = RegGetDword(L"EdgeSnap", 1, L"Editor") != 0;
    g.canvas.plainPaste = RegGetDword(L"PlainTextPaste", 1, L"Editor") != 0;
    g.canvas.onCursorMoved = [](bool inside, Gdiplus::PointF p) {
        std::wstring c = inside ? Format(L"X: %d   Y: %d", (int)std::floor(p.X), (int)std::floor(p.Y)) : L"";
        if (c != g.coords) { g.coords = c; InvalidateStatus(); }
    };
    g.canvas.onHint = [](const std::wstring& h) { SetStatus(h); };
    g.canvas.onContextMenu = [](POINT pt) { CanvasContextMenu(pt); };
    g.canvas.background = RegGetDword(L"CanvasBackground", RGB(222, 224, 228), L"Editor");
    g.canvas.onDocChanged = [] { DocChanged(); };
    g.canvas.onSelectionChanged = [] {
        g.props.Load();
        if (g.rightTab == 2) g.layers.Refresh();
        int n = g.canvas.SelectionCount();
        if (n > 1) SetStatus(Format(Tr(L"%d objetos selecionados  •  Ctrl+Shift+G agrupa  •  Organizar › Alinhar alinha entre si"), n));
    };
    g.canvas.onAreaChanged = [] { g.props.AreaChanged(); };
    g.canvas.onZoomChanged = [] { InvalidateStatus(); };
    g.canvas.onColorPicked = [](uint32_t c) {
        // eyedropper: applies the color to the most common previous tool (arrow) and to all stroke tools
        for (Tool t : { Tool::Arrow, Tool::Line, Tool::Shape, Tool::Pen, Tool::Step, Tool::Stamp, Tool::Fill, Tool::Eyedropper }) g.canvas.styles[t].stroke = c;
        g.canvas.styles[Tool::Text].textColor = c;
        SetStatus(Tr(L"Cor capturada: ") + ColorToHex(ToColorref(c)) + Tr(L" (aplicada às ferramentas de desenho)"));
        g.props.Load();
    };
    g.layers.Create(g.frame, &g.canvas);
    g.layers.onHint = [](const std::wstring& h) { SetStatus(h); };
    g.layers.onContextMenu = [](POINT pt) { CanvasContextMenu(pt); };
    g.props.Create(g.frame, &g.canvas);
    g.props.onCommand = [](int id) {
        switch (id) {
        case 324: /*P_BTN1*/ if (g.canvas.tool == Tool::Crop) g.canvas.ApplyCrop(); else ImageCommand(M_CROP_SEL); break;
        case 325: /*P_BTN2*/ if (g.canvas.tool == Tool::Crop) ImageCommand(M_TRIM); else OnCommand(M_COPY); break;
        case 326: /*P_BTN3*/ g.canvas.DeleteSelected(); break;
        case 327: g.canvas.BringToFront(); break;
        case 328: g.canvas.SendToBack(); break;
        case 329: g.canvas.DeleteSelected(); break;
        case 330: OnCommand(M_GROUP); break;
        case 331: OnCommand(M_UNGROUP); break;
        }
    };
    g.props.onPickTool = [](Tool t) { g.canvas.SetTool(t); };      // favorite: changes the drawing tool (the bar stays on Favorites)
    g.props.onStylesChanged = [] {};
    g.props.onHint = [](const std::wstring& h) { SetStatus(h); };
    g.video.Create(g.frame);
    g.video.onSaveFrame = [](const Image& img) { NewDocFromImage(img, Tr(L"Quadro do vídeo"), Tr(L"Quadro de vídeo")); };
    g.video.onNewVideo = [](const std::wstring& p) { g.tray.Reload(); SetStatus(Tr(L"Vídeo salvo: ") + p); };
    // recent captures tray (at the bottom) and whole library (in place of the image), with the same commands
    for (LibraryTray* lt : { &g.tray, &g.library }) {
        lt->Create(g.frame, lt == &g.tray ? LibraryTray::Tray : LibraryTray::Library);
        lt->onOpen = [](const std::wstring& p) { OpenPath(p); };
        lt->onRenamed = [lt](const std::wstring& p, const std::wstring& name) {
            if (Document* d = FindDoc(p)) {
                d->title = !name.empty() ? name : !d->windowTitle.empty() ? d->windowTitle : !d->captureMode.empty() ? d->captureMode : FileNameOf(p);
                UpdateTitle();
            }
            (lt == &g.tray ? g.library : g.tray).Reload();          // the other view shows the new name too
        };
        lt->onDelete = [](const std::vector<std::wstring>& p) { DeleteDocs(p); };
        lt->onExport = [](const std::vector<std::wstring>& p) { ExportItems(p); };
        lt->onDropFiles = [](const std::vector<std::wstring>& files) { for (auto& f : files) OpenPath(f); };
        lt->filesForDrag = FilesForDrag;
    }
    g.tray.onShowLibrary = [] { ShowLibrary(true); };
    g.library.onShowLibrary = [] { ShowLibrary(false); };
    g.tray.Reload();
    SetTool(Tool::Arrow);
    g.rightTab = std::min(2, (int)RegGetDword(L"RightPanel", 0, L"Editor"));
    g.effects.SetSource(Image());
}

bool HandleToolKey(WPARAM vk) {
    static const std::map<WPARAM, Tool> keys = {
        { 'V', Tool::Select }, { 'A', Tool::Arrow }, { 'L', Tool::Line }, { 'R', Tool::Shape }, { 'P', Tool::Pen }, { 'H', Tool::Highlighter },
        { 'T', Tool::Text }, { 'C', Tool::Callout }, { 'N', Tool::Step }, { 'K', Tool::Stamp }, { 'B', Tool::Blur }, { 'G', Tool::Spotlight },
        { 'Z', Tool::Magnify }, { 'F', Tool::Fill }, { 'E', Tool::Eraser }, { 'X', Tool::Crop }, { 'S', Tool::Selection }, { 'U', Tool::Cutout },
        { 'I', Tool::Eyedropper } };
    auto it = keys.find(vk);
    if (it == keys.end()) return false;
    SetTool(it->second);
    return true;
}

LRESULT CALLBACK FrameProc(HWND h, UINT m, WPARAM w, LPARAM l) {
    switch (m) {
    case WM_CREATE:
        g.frame = h;
        g.dpi = DpiForWindow(h);
        g.caption.Init(h, { { Tr(L"Arquivo"), i18n::AltKey(Tr(L"Arquivo"), L'A'), GetSubMenu(g.menuBar, 0) },
                            { Tr(L"Editar"), i18n::AltKey(Tr(L"Editar"), L'E'), GetSubMenu(g.menuBar, 1) },
                            { Tr(L"Imagem"), i18n::AltKey(Tr(L"Imagem"), L'I'), GetSubMenu(g.menuBar, 2) },
                            { Tr(L"Exibir"), L'X', GetSubMenu(g.menuBar, 3) },
                            { Tr(L"Ajuda"), i18n::AltKey(Tr(L"Ajuda"), L'J'), GetSubMenu(g.menuBar, 4) } }, true, true);
        LoadTheme();
        CreateChildren();
        DragAcceptFiles(h, TRUE);
        return 0;
    // custom title bar (the same as the capture window's): no system frame at the top
    case WM_NCCALCSIZE: { LRESULT r; if (g.caption.NcCalcSize(w, l, r)) return r; break; }
    case WM_NCHITTEST: { LRESULT r; if (g.caption.NcHitTest(l, r)) return r; break; }
    case WM_NCRBUTTONUP: if (g.caption.RButtonUp(w, l)) return 0; break;
    case WM_MOUSEMOVE: g.caption.MouseMove({ GET_X_LPARAM(l), GET_Y_LPARAM(l) }); return 0;
    case WM_MOUSELEAVE: g.caption.MouseLeave(); return 0;
    case WM_LBUTTONDOWN: {
        POINT p{ GET_X_LPARAM(l), GET_Y_LPARAM(l) };
        if (g.caption.LButtonDown(p)) return 0;
        for (int i = 0; i < 3; i++) { RECT t = TabRect(i); if (PtInRect(&t, p) && !(g.cur && g.cur->isVideo) && !g.libraryOpen) SetRightTab(i); }
        if (PtInRect(&g.footRecent, p)) { OnCommand(M_SHOW_TRAY); return 0; }
        if (PtInRect(&g.footSize, p)) { OnCommand(M_RESIZE); return 0; }
        if (PtInRect(&g.footZoom, p)) {                // zoom levels (IZoomLevels)
            HMENU m = CreatePopupMenu();
            int levels[] = { 25, 50, 75, 100, 150, 200, 300, 400, 800 };
            for (int z : levels) AppendMenuW(m, MF_STRING | (std::lround(g.canvas.zoom * 100) == z ? MF_CHECKED : 0), 10 + z, (std::to_wstring(z) + L"%").c_str());
            AppendMenuW(m, MF_SEPARATOR, 0, nullptr);
            AppendMenuW(m, MF_STRING, 1, Tr(L"Ajustar à janela\tCtrl+0"));
            POINT sp{ g.footZoom.left, g.footZoom.top }; ClientToScreen(h, &sp);
            int cmd = TrackPopupMenu(m, TPM_RETURNCMD | TPM_BOTTOMALIGN, sp.x, sp.y, 0, h, nullptr);
            DestroyMenu(m);
            if (cmd == 1) g.canvas.FitToWindow(); else if (cmd > 10) g.canvas.ZoomTo((cmd - 10) / 100.0);
            InvalidateStatus();
            return 0;
        }
        return 0;
    }
    case WM_LBUTTONUP: g.caption.LButtonUp({ GET_X_LPARAM(l), GET_Y_LPARAM(l) }); return 0;
    case WM_SETTINGCHANGE:
        if (l && lstrcmpiW((const wchar_t*)l, L"ImmersiveColorSet") == 0) LoadTheme();
        break;
    case WM_DWMCOLORIZATIONCOLORCHANGED: LoadTheme(); break;
    case WM_SIZE: Layout(); return 0;
    case WM_GETMINMAXINFO: ((MINMAXINFO*)l)->ptMinTrackSize = { S(900), S(560) }; return 0;
    case WM_DPICHANGED: {
        g.dpi = HIWORD(w);
        RECT* r = (RECT*)l;
        SetWindowPos(h, nullptr, r->left, r->top, r->right - r->left, r->bottom - r->top, SWP_NOZORDER | SWP_NOACTIVATE);
        Layout();
        return 0;
    }
    case WM_ERASEBKGND: return 1;
    case WM_PAINT: { PAINTSTRUCT ps; HDC dc = BeginPaint(h, &ps); PaintFrame(dc); EndPaint(h, &ps); return 0; }
    case WM_COMMAND: OnCommand(LOWORD(w)); return 0;
    case WM_INITMENUPOPUP: {
        HMENU mm = (HMENU)w;
        bool img = g.cur && !g.cur->isVideo;
        auto en = [&](int id, bool on) { EnableMenuItem(mm, id, MF_BYCOMMAND | (on ? MF_ENABLED : MF_GRAYED)); };
        en(M_UNDO, img && g.cur->CanUndo()); en(M_REDO, img && g.cur->CanRedo()); en(M_UNDOALL, img && g.cur->CanUndo());
        for (int id : { M_SAVE, M_SAVEAS, M_DUPLICATE, M_PRINT, M_EMAIL, M_OPEN_PROGRAM, M_COPYALL, M_SELECTALL, M_OCR, M_CUT, M_COPY })
            en(id, img);
        en(M_CUT, img && !g.canvas.SelectionLocked());           // locked objects are not cut
        bool obj = img && g.canvas.selected >= 0;
        for (int id : { M_FRONT, M_BACK, M_BRING_FORWARD, M_SEND_BACKWARD, M_FLATTEN_SEL, M_ALIGN_LEFT, M_ALIGN_CENTER, M_ALIGN_RIGHT,
                        M_ALIGN_TOP, M_ALIGN_MIDDLE, M_ALIGN_BOTTOM, M_LOCK, M_RENAME_OBJ })
            en(id, obj);
        en(M_UNLOCK, obj && g.canvas.SelectionLocked());
        en(M_DIST_H, img && g.canvas.SelectionCount() >= 3); en(M_DIST_V, img && g.canvas.SelectionCount() >= 3);
        en(M_LIFT, img && g.canvas.HasRasterSelection()); en(M_DROP, img && g.canvas.HasFloating());
        en(M_PASTE_INPLACE, img && (IsClipboardFormatAvailable(ObjectsFormat()) || ClipboardHasImage() || IsClipboardFormatAvailable(CF_UNICODETEXT)));
        en(M_CLEAR_GUIDES, img && (!g.cur->guidesX.empty() || !g.cur->guidesY.empty()));
        CheckMenuItem(mm, M_RULERS, g.canvas.rulers ? MF_CHECKED : MF_UNCHECKED);
        CheckMenuItem(mm, M_EDGESNAP, g.canvas.edgeSnap ? MF_CHECKED : MF_UNCHECKED);
        CheckMenuItem(mm, M_PLAINPASTE, g.canvas.plainPaste ? MF_CHECKED : MF_UNCHECKED);
        en(M_FLATTEN, img && !g.cur->st.objs.empty());
        en(M_GROUP, img && g.canvas.SelectionCount() >= 2); en(M_UNGROUP, img && g.canvas.CanUngroup()); en(M_APPLY_TEMPLATE, false);
        for (int id : { M_TRIM, M_RESIZE, M_CANVAS, M_CANVAS_COLOR, M_TAG, M_FLIP_H, M_FLIP_V, M_ROT_CW, M_ROT_CCW, M_ROT_180, M_ROT_FREE, M_CROP_SEL })
            en(id, img);
        CheckMenuItem(mm, M_SNAPPING, g.canvas.snapping ? MF_CHECKED : MF_UNCHECKED);
        for (auto& t : kTools) CheckMenuItem(mm, M_MORE_TOOL_BASE + (int)t.t, t.t == g.toolbar.active ? MF_CHECKED : MF_UNCHECKED);
        en(M_PASTE, ClipboardHasImage() || IsClipboardFormatAvailable(CF_HDROP) || IsClipboardFormatAvailable(ObjectsFormat()) ||
                    (img && IsClipboardFormatAvailable(CF_UNICODETEXT)));
        en(M_DELETE, g.cur != nullptr); en(M_CLOSE, g.cur != nullptr);
        CheckMenuItem(mm, M_SHOW_TRAY, g.showTray ? MF_CHECKED : MF_UNCHECKED);
        CheckMenuItem(mm, M_SHOW_EFFECTS, g.rightTab == 1 && !g.libraryOpen ? MF_CHECKED : MF_UNCHECKED);
        CheckMenuItem(mm, M_SHOW_LAYERS, g.rightTab == 2 && !g.libraryOpen ? MF_CHECKED : MF_UNCHECKED);
        CheckMenuItem(mm, M_SHOW_LIBRARY, g.libraryOpen ? MF_CHECKED : MF_UNCHECKED);
        for (int id : { M_FX_BORDER, M_FX_SHADOW, M_FX_PERSPECTIVE, M_FX_PAGECURL, M_FX_EDGE, M_FX_FILTER, M_FX_ADJUST, M_FX_REPLACE,
                        M_FX_SPOTLIGHT, M_FX_WATERMARK, M_FX_CAPTION })
            en(id, img);
        return 0;
    }
    case WM_DROPFILES: {
        HDROP d = (HDROP)w;
        UINT n = DragQueryFileW(d, 0xFFFFFFFF, nullptr, 0);
        if (!LibraryTray::DraggingOut())       // a tray item dropped back on the Editor: does not import a copy
            for (UINT i = 0; i < n; i++) { wchar_t f[MAX_PATH]; DragQueryFileW(d, i, f, MAX_PATH); OpenPath(f); }
        DragFinish(d);
        return 0;
    }
    case WM_TIMER:
        if (w == 0x1E) {        // OnIPCNotify case 0x1E: "new image from clipboard" command and then show
            KillTimer(h, w);
            NewFromClipboard();
            ShowEditor();
        } else if (w == 0x60) { KillTimer(h, w); g.tray.Reload(); if (g.libraryOpen) g.library.Reload(); }
        else if (w == kFxPreviewTimer) {
            KillTimer(h, w);
            g.effects.SetSource(g.cur && !g.cur->isVideo ? g.cur->Render() : Image());
        }
        return 0;
    case WM_LIB_CHANGED: SetTimer(h, 0x60, 400, nullptr); return 0;      // groups bursts of changes
    case WM_SNAP_IPC: { std::unique_ptr<ipc::Message> msg((ipc::Message*)l); OnIPCNotify(*msg); return 0; }
    case WM_COPYDATA: {
        std::wstring cmd;
        if (!ParseCopyData((COPYDATASTRUCT*)l, cmd)) return FALSE;
        PostMessageW(h, WM_SNAP_CMDLINE, 0, (LPARAM)new std::wstring(cmd));
        return TRUE;
    }
    case WM_SNAP_CMDLINE: { std::unique_ptr<std::wstring> c((std::wstring*)l); ProcessCommandLine(*c, false); return 0; }
    case WM_CLOSE: {
        g.canvas.CommitText();
        for (auto& d : g.docs) Autosave(d.get());
        SaveToolStyles(g.canvas.styles);
        RegSetDword(L"SmartMove", g.canvas.smartMove, L"Editor");
        WINDOWPLACEMENT wp{ sizeof(wp) };
        GetWindowPlacement(h, &wp);
        RegSetString(L"WindowPlacement", Format(L"%d %d %d %d %d", wp.rcNormalPosition.left, wp.rcNormalPosition.top,
                                                wp.rcNormalPosition.right, wp.rcNormalPosition.bottom, wp.showCmd == SW_SHOWMAXIMIZED), L"Editor");
        DestroyWindow(h);
        return 0;
    }
    case WM_DESTROY:
        if (g.watchStop) SetEvent(g.watchStop);
        g.ipc.SendInt(ipc::kCapture, ipc::EditorClosed, 0);
        PostQuitMessage(0);
        return 0;
    }
    return DefWindowProcW(h, m, w, l);
}

} // namespace

int WINAPI wWinMain(HINSTANCE hi, HINSTANCE, LPWSTR, int nShow) {
    SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    // single instance: mutex + "SnapnabEditor" window; the second instance forwards the command line (WM_COPYDATA 0x8010)
    WaitForPreviousInstance(kEditorWndClass);
    if (ForwardToExistingInstance(kEditorMutex, kEditorWndClass, &g.mutex)) return 0;
    SetCurrentDirectoryW(ExeDir().c_str());
    OleInitialize(nullptr);          // COM in STA + OLE drag and drop (tray → other programs)
    INITCOMMONCONTROLSEX icc{ sizeof(icc), ICC_STANDARD_CLASSES | ICC_UPDOWN_CLASS | ICC_HOTKEY_CLASS | ICC_PROGRESS_CLASS | ICC_BAR_CLASSES };
    InitCommonControlsEx(&icc);
    GdiplusSession gdip;
    mui::Register(hi);
    LogInit(L"SnapnabEditor");
    i18n::Init();
    g.hi = hi;
    g.showTray = RegGetDword(L"ShowTray", 1, L"Editor") != 0;

    // startup conditioned on the capture process (mutex + "SnapnabUI" window)
    if (!IsProcessRunning(kCaptureMutex)) LaunchSibling(kCaptureExe, L"/hide", true);

    WNDCLASSEXW wc{ sizeof(wc) };
    wc.lpfnWndProc = FrameProc; wc.hInstance = hi; wc.hCursor = LoadCursor(nullptr, IDC_ARROW);
    wc.hIcon = LoadIconW(hi, MAKEINTRESOURCEW(1));
    if (!wc.hIcon) wc.hIcon = icons::MakeAppIcon(32, true);
    wc.hIconSm = icons::MakeAppIcon(16, true);
    wc.lpszClassName = kEditorWndClass;
    RegisterClassExW(&wc);
    int dpi = GetDpiForSystem();
    int x = CW_USEDEFAULT, y = CW_USEDEFAULT, W = ui::Scale(1280, dpi), H = ui::Scale(820, dpi);
    bool maximize = false;
    std::wstring wp = RegGetString(L"WindowPlacement", L"", L"Editor");
    int l, t, r, b, mx;
    if (swscanf_s(wp.c_str(), L"%d %d %d %d %d", &l, &t, &r, &b, &mx) == 5 && r - l > 300 && b - t > 300) {
        RECT test{ l, t, r, b };
        if (MonitorFromRect(&test, MONITOR_DEFAULTTONULL)) { x = l; y = t; W = r - l; H = b - t; maximize = mx != 0; }
    }
    g.menuBar = BuildMenu();       // menus shown by the custom title bar (there is no system menu bar)
    HWND frame = CreateWindowExW(0, kEditorWndClass, L"Snapnab Editor", WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN, x, y, W, H,
                                 nullptr, nullptr, hi, nullptr);
    DWORD corner = 2;   // DWMWCP_ROUND
    DwmSetWindowAttribute(frame, 33 /*DWMWA_WINDOW_CORNER_PREFERENCE*/, &corner, sizeof corner);
    SetWindowPos(frame, nullptr, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED);
    g.accel = BuildAccel();
    g.ipc.Start(ipc::kEditor, [](const ipc::Message& m) { PostMessageW(g.frame, WM_SNAP_IPC, 0, (LPARAM)new ipc::Message(m)); });
    StartLibraryWatcher();
    ShowWindow(frame, maximize ? SW_SHOWMAXIMIZED : nShow);
    UpdateWindow(frame);
    ProcessCommandLine(GetCommandLineW(), true);
    SetForegroundWindow(frame);

    MSG msg;
    while (GetMessageW(&msg, nullptr, 0, 0)) {
        // in-place text editing: Esc commits; text editing shortcuts are not intercepted
        if (g.canvas.EditingText()) {
            if (msg.message == WM_KEYDOWN && msg.wParam == VK_ESCAPE) { g.canvas.CommitText(); continue; }
            // paste into the text: without formatting by default; Ctrl+Shift+V does the opposite
            if (msg.message == WM_KEYDOWN && msg.wParam == 'V' && GetKeyState(VK_CONTROL) < 0 && GetKeyState(VK_MENU) >= 0) {
                PasteTextNow(GetKeyState(VK_SHIFT) < 0);
                continue;
            }
            TranslateMessage(&msg); DispatchMessageW(&msg);
            continue;
        }
        if (g.caption.PreTranslate(msg)) continue;
        wchar_t cls[32] = L"";
        GetClassNameW(GetFocus(), cls, 32);
        bool inEdit = !_wcsicmp(cls, L"Edit") || !_wcsicmp(cls, L"ComboBox") || !_wcsicmp(cls, L"msctls_hotkey32");
        if (!inEdit && TranslateAcceleratorW(g.frame, g.accel, &msg)) continue;
        // tool letters (an arrow just drawn takes the typed letters as its label instead)
        if (msg.message == WM_KEYDOWN && GetFocus() == g.canvas.hwnd && GetKeyState(VK_CONTROL) >= 0 && GetKeyState(VK_MENU) >= 0 &&
            !g.canvas.WantsTyping() && HandleToolKey(msg.wParam)) continue;
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
    g.ipc.Stop();
    if (g.watcher.joinable()) g.watcher.join();
    Log(L"SnapnabEditor exited");
    if (g.mutex) { ReleaseMutex(g.mutex); CloseHandle(g.mutex); }
    OleUninitialize();
    return 0;
}
