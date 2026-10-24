// SnapnabEditor.exe — editor de imagens e vídeos (≙ SnagitEditor.exe, ds_arch §19 / ed_comp_1..15)
#include "Common.h"
#include "Canvas.h"
#include "Panels.h"
#include "Video.h"
#include "Effects.h"
#include "Ipc.h"
#include "Library.h"
#include "Ui.h"
#include <commdlg.h>
#include <mapi.h>
#include <cmath>

using namespace snap;
using namespace snap::ed;

namespace {

enum : int {
    // barra
    T_CAPTURE = 1, T_RECORD, T_COPYALL, T_SAVE, T_SHARE, T_TOOL_BASE = 100,
    // menu
    M_NEW_CAPTURE = 1000, M_NEW_VIDEO, M_NEW_CLIPBOARD, M_NEW_IMAGE, M_OPEN, M_SAVE, M_SAVEAS, M_DUPLICATE, M_DELETE, M_PRINT,
    M_EMAIL, M_OPEN_PROGRAM, M_LIBRARY, M_CLOSE, M_EXIT,
    M_UNDO, M_REDO, M_UNDOALL, M_CUT, M_COPY, M_COPYALL, M_PASTE, M_DELETE_SEL, M_SELECTALL, M_FLATTEN, M_FRONT, M_BACK, M_OCR,
    M_CROP_SEL, M_ROT_CW, M_ROT_CCW, M_ROT_180, M_ROT_FREE, M_FLIP_H, M_FLIP_V, M_RESIZE, M_CANVAS, M_CANVAS_COLOR, M_TRIM,
    M_FX_BORDER, M_FX_SHADOW, M_FX_EDGE, M_FX_GRAY, M_FX_INVERT, M_FX_SEPIA, M_FX_BC, M_FX_HS, M_FX_REPLACE, M_FX_WATERMARK, M_FX_CAPTION,
    M_FX_MONO,
    M_ZOOM_IN, M_ZOOM_OUT, M_ZOOM_100, M_ZOOM_FIT, M_BG_COLOR, M_SHOW_TRAY, M_ABOUT, M_SHORTCUTS,
};

constexpr UINT WM_LIB_CHANGED = WM_APP + 0x60;

struct App {
    HINSTANCE hi = nullptr;
    HWND frame = nullptr;
    HANDLE mutex = nullptr;
    ipc::Notify ipc;
    Toolbar toolbar;
    PropsPanel props;
    Canvas canvas;
    VideoView video;
    LibraryTray tray;
    std::vector<std::unique_ptr<Document>> docs;
    Document* cur = nullptr;
    std::wstring status;
    bool showTray = true;
    HACCEL accel = nullptr;
    HANDLE watchStop = nullptr;
    std::thread watcher;
    int dpi = 96;
} g;

int S(int v) { return ui::Scale(v, g.dpi); }

// ---------------------------------------------------------------- título / status
void UpdateTitle() {
    std::wstring t = L"Snapnab Editor";
    if (g.cur) t = (g.cur->modified ? L"• " : L"") + g.cur->title + L" — Snapnab Editor";
    SetWindowTextW(g.frame, t.c_str());
}

void InvalidateStatus() {
    RECT cr; GetClientRect(g.frame, &cr);
    RECT sr{ 0, cr.bottom - S(24), cr.right, cr.bottom };
    InvalidateRect(g.frame, &sr, FALSE);
}

void SetStatus(const std::wstring& s) { g.status = s; InvalidateStatus(); }

void UpdateDocInfo() {
    if (!g.cur || g.cur->isVideo) { g.props.SetDocInfo(g.cur ? L"Vídeo: " + FileNameOf(g.cur->path) : L""); return; }
    std::wstring info = Format(L"%d × %d px", g.cur->w(), g.cur->h());
    if (!g.cur->captureMode.empty()) info += L"\nCaptura: " + g.cur->captureMode;
    if (!g.cur->application.empty()) info += L"\nAplicativo: " + g.cur->application;
    if (!g.cur->windowTitle.empty()) info += L"\nJanela: " + g.cur->windowTitle;
    info += Format(L"\nObjetos: %zu", g.cur->st.objs.size());
    g.props.SetDocInfo(info);
}

// ---------------------------------------------------------------- layout
void Layout() {
    if (!g.frame) return;
    RECT cr; GetClientRect(g.frame, &cr);
    int tbH = g.toolbar.Height(), stH = S(24), trayH = g.showTray ? g.tray.Height() : 0, propW = S(250);
    MoveWindow(g.toolbar.hwnd, 0, 0, cr.right, tbH, TRUE);
    int y0 = tbH, y1 = cr.bottom - stH - trayH;
    bool video = g.cur && g.cur->isVideo;
    MoveWindow(g.props.hwnd, cr.right - propW, y0, propW, std::max(0, y1 - y0), TRUE);
    MoveWindow(g.canvas.hwnd, 0, y0, cr.right - (video ? 0 : propW), std::max(0, y1 - y0), TRUE);
    MoveWindow(g.video.hwnd, 0, y0, cr.right, std::max(0, y1 - y0), TRUE);
    ShowWindow(g.canvas.hwnd, video ? SW_HIDE : SW_SHOW);
    ShowWindow(g.props.hwnd, video ? SW_HIDE : SW_SHOW);
    ShowWindow(g.video.hwnd, video ? SW_SHOW : SW_HIDE);
    ShowWindow(g.tray.hwnd, g.showTray ? SW_SHOW : SW_HIDE);
    if (g.showTray) MoveWindow(g.tray.hwnd, 0, y1, cr.right, trayH, TRUE);
    InvalidateStatus();
}

// ---------------------------------------------------------------- documentos
void Autosave(Document* d) {
    if (!d || d->isVideo || !d->modified || d->path.empty()) return;
    if (d->Save()) {
        WriteThumbnail(d->path, d->Render());
        g.tray.InvalidateThumb(d->path);
        Log(L"Autosave: %s", d->path.c_str());
    }
}

static bool GuidFromLibraryPath(const std::wstring& path, GUID& out) {
    std::wstring name = StripExt(FileNameOf(path));
    return SUCCEEDED(CLSIDFromString((L"{" + name + L"}").c_str(), &out));
}

void Activate(Document* d) {
    if (g.cur == d) return;
    g.canvas.CommitText();
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
    }
    Layout();
    UpdateTitle();
    UpdateDocInfo();
    g.props.Load();
}

Document* FindDoc(const std::wstring& path) {
    for (auto& d : g.docs) if (!_wcsicmp(d->path.c_str(), path.c_str())) return d.get();
    return nullptr;
}

Document* AddDoc(std::unique_ptr<Document> d) {
    // mantém até 12 documentos em memória (os outros ficam salvos na biblioteca)
    if (g.docs.size() >= 12) {
        for (auto it = g.docs.begin(); it != g.docs.end(); ++it)
            if (it->get() != g.cur) { Autosave(it->get()); g.docs.erase(it); break; }
    }
    g.docs.push_back(std::move(d));
    return g.docs.back().get();
}

// imagem nova vira documento .snap na biblioteca (como o original importa para a DataStore)
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
    if (!FileExists(path)) { MessageBoxW(g.frame, (L"Arquivo não encontrado:\n" + path).c_str(), L"Snapnab Editor", MB_ICONWARNING); return false; }
    std::wstring ext = FileExt(path);
    auto d = Document::Load(path);
    if (!d) { MessageBoxW(g.frame, (L"Não foi possível abrir:\n" + path).c_str(), L"Snapnab Editor", MB_ICONWARNING); return false; }
    if (ext != L".snap" && ext != L".mp4") {
        // imagem comum: importada para a biblioteca
        Image img = *d->st.base;
        NewDocFromImage(std::move(img), FileNameOf(path), L"Arquivo importado");
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
    if (g.cur) SetStatus(Format(L"%d × %d px  •  %zu objeto(s)  •  %zu passo(s) para desfazer", g.cur->w(), g.cur->h(),
                                g.cur->st.objs.size(), g.cur->UndoDepth()));
}

// ---------------------------------------------------------------- operações de imagem
void TransformObjects(std::function<Gdiplus::PointF(Gdiplus::PointF)> map) {
    for (size_t i = 0; i < g.cur->st.objs.size(); i++) {
        DrawObj* o = g.cur->Mutable(i);
        o->p1 = map(o->p1); o->p2 = map(o->p2); o->tail = map(o->tail);
        for (auto& p : o->pts) p = map(p);
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

void ImageCommand(int cmd) {
    if (!RequireImageDoc()) return;
    Document* d = g.cur;
    switch (cmd) {
    case M_ROT_CW: case M_ROT_CCW: case M_ROT_180: case M_FLIP_H: case M_FLIP_V: Geometry(cmd); return;
    case M_CROP_SEL:
        if (g.canvas.HasRasterSelection() || g.canvas.HasCrop()) g.canvas.CropToSelection();
        else MessageBoxW(g.frame, L"Faça uma seleção (ferramenta Seleção) ou um recorte primeiro.", L"Snapnab Editor", MB_ICONINFORMATION);
        return;
    case M_ROT_FREE: {
        std::vector<ui::Field> f = { ui::Field::MakeInt(L"Ângulo (graus, horário)", 15, -359, 359), ui::Field::MakeColor(L"Cor de fundo", RGB(255, 255, 255)),
                                     ui::Field::MakeCheck(L"Fundo transparente", true),
                                     ui::Field::MakeLabel(L"Os objetos serão mesclados à imagem.") };
        if (!ui::RunForm(g.frame, L"Girar", f)) return;
        d->Checkpoint(); FlattenIfNeeded();
        d->SetBase(fx::RotateAngle(*d->st.base, f[0].value, f[2].value ? 0 : FromColorref(f[1].color)));
        break;
    }
    case M_RESIZE: {
        std::vector<ui::Field> f = { ui::Field::MakeInt(L"Largura (px)", d->w(), 1, 20000), ui::Field::MakeInt(L"Altura (px)", d->h(), 1, 20000),
                                     ui::Field::MakeInt(L"ou escala (%)", 100, 1, 1000), ui::Field::MakeCheck(L"Manter proporção", true) };
        if (!ui::RunForm(g.frame, L"Redimensionar imagem", f)) return;
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
        std::vector<ui::Field> f = { ui::Field::MakeInt(L"Largura (px)", d->w(), 1, 20000), ui::Field::MakeInt(L"Altura (px)", d->h(), 1, 20000),
                                     ui::Field::MakeCombo(L"Âncora", { L"Superior esquerda", L"Superior", L"Superior direita", L"Esquerda", L"Centro",
                                                                       L"Direita", L"Inferior esquerda", L"Inferior", L"Inferior direita" }, 4),
                                     ui::Field::MakeColor(L"Cor do canvas", RGB(255, 255, 255)), ui::Field::MakeCheck(L"Transparente", false) };
        if (!ui::RunForm(g.frame, L"Tamanho do canvas", f)) return;
        int nw = f[0].value, nh = f[1].value, a = f[2].value;
        int ox = a % 3 == 0 ? 0 : a % 3 == 1 ? (nw - d->w()) / 2 : nw - d->w();
        int oy = a / 3 == 0 ? 0 : a / 3 == 1 ? (nh - d->h()) / 2 : nh - d->h();
        d->Checkpoint();
        d->SetBase(fx::CanvasResize(*d->st.base, nw, nh, a, f[4].value ? 0 : FromColorref(f[3].color)));
        TransformObjects([ox, oy](Gdiplus::PointF p) { return Gdiplus::PointF(p.X + ox, p.Y + oy); });
        break;
    }
    case M_CANVAS_COLOR: {
        COLORREF c = RGB(255, 255, 255);
        if (!PickColor(g.frame, c)) return;
        d->Checkpoint();
        Image& b = d->MutableBase();
        Image out(b.w, b.h, FromColorref(c)); out.Blit(b, 0, 0, true);
        d->SetBase(std::move(out));
        break;
    }
    case M_TRIM: {
        Image t = fx::AutoTrim(*d->st.base);
        if (t.w == d->w() && t.h == d->h()) { SetStatus(L"Nada a cortar: as bordas não são uniformes."); return; }
        // localiza o deslocamento do recorte para mover os objetos
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
    case M_FX_BORDER: {
        std::vector<ui::Field> f = { ui::Field::MakeInt(L"Largura (px)", 6, 1, 200), ui::Field::MakeColor(L"Cor", RGB(40, 40, 40)) };
        if (!ui::RunForm(g.frame, L"Borda", f)) return;
        int w = f[0].value;
        d->Checkpoint();
        d->SetBase(fx::Border(*d->st.base, w, FromColorref(f[1].color)));
        TransformObjects([w](Gdiplus::PointF p) { return Gdiplus::PointF(p.X + w, p.Y + w); });
        break;
    }
    case M_FX_SHADOW: {
        std::vector<ui::Field> f = { ui::Field::MakeInt(L"Deslocamento (px)", 8, 0, 100), ui::Field::MakeInt(L"Desfoque (px)", 8, 1, 60),
                                     ui::Field::MakeInt(L"Opacidade (%)", 50, 5, 100) };
        if (!ui::RunForm(g.frame, L"Sombra", f)) return;
        int pad = f[1].value * 2;
        d->Checkpoint();
        d->SetBase(fx::Shadow(*d->st.base, f[0].value, f[1].value, f[2].value));
        TransformObjects([pad](Gdiplus::PointF p) { return Gdiplus::PointF(p.X + pad, p.Y + pad); });
        break;
    }
    case M_FX_EDGE: {
        std::vector<ui::Field> f = { ui::Field::MakeCombo(L"Estilo", { L"Rasgada", L"Onda", L"Dente de serra", L"Dente de tubarão", L"Esmaecer" }, 0),
                                     ui::Field::MakeInt(L"Tamanho (px)", 16, 2, 200),
                                     ui::Field::MakeCheck(L"Topo", false), ui::Field::MakeCheck(L"Direita", true),
                                     ui::Field::MakeCheck(L"Base", true), ui::Field::MakeCheck(L"Esquerda", false),
                                     ui::Field::MakeCheck(L"Sombra", true) };
        if (!ui::RunForm(g.frame, L"Borda especial", f)) return;
        int sides = (f[2].value ? fx::Top : 0) | (f[3].value ? fx::Right : 0) | (f[4].value ? fx::Bottom : 0) | (f[5].value ? fx::Left : 0);
        d->Checkpoint(); FlattenIfNeeded();
        d->SetBase(fx::EdgeEffect(*d->st.base, (fx::Edge)f[0].value, f[1].value, sides ? sides : fx::All, f[6].value));
        break;
    }
    case M_FX_GRAY: d->Checkpoint(); fx::Grayscale(d->MutableBase()); break;
    case M_FX_INVERT: d->Checkpoint(); fx::Invert(d->MutableBase()); break;
    case M_FX_SEPIA: d->Checkpoint(); fx::Sepia(d->MutableBase()); break;
    case M_FX_MONO: {
        std::vector<ui::Field> f = { ui::Field::MakeInt(L"Limiar (0–255)", 128, 0, 255) };
        if (!ui::RunForm(g.frame, L"Monocromático", f)) return;
        d->Checkpoint(); fx::Monochrome(d->MutableBase(), f[0].value);
        break;
    }
    case M_FX_BC: {
        std::vector<ui::Field> f = { ui::Field::MakeInt(L"Brilho (−100 a 100)", 0, -100, 100), ui::Field::MakeInt(L"Contraste (−100 a 100)", 0, -100, 100),
                                     ui::Field::MakeInt(L"Gama (×100)", 100, 10, 500) };
        if (!ui::RunForm(g.frame, L"Brilho e contraste", f)) return;
        d->Checkpoint();
        Image& b = d->MutableBase();
        fx::BrightnessContrast(b, f[0].value, f[1].value);
        if (f[2].value != 100) fx::Gamma(b, f[2].value / 100.0);
        break;
    }
    case M_FX_HS: {
        std::vector<ui::Field> f = { ui::Field::MakeInt(L"Matiz (graus)", 0, -180, 180), ui::Field::MakeInt(L"Saturação (−100 a 100)", 0, -100, 100) };
        if (!ui::RunForm(g.frame, L"Matiz e saturação", f)) return;
        d->Checkpoint(); fx::HueSaturation(d->MutableBase(), f[0].value, f[1].value);
        break;
    }
    case M_FX_REPLACE: {
        std::vector<ui::Field> f = { ui::Field::MakeColor(L"Substituir a cor", RGB(255, 255, 255)), ui::Field::MakeColor(L"Por", RGB(240, 240, 240)),
                                     ui::Field::MakeCheck(L"Tornar transparente", false), ui::Field::MakeInt(L"Tolerância", 10, 0, 255) };
        if (!ui::RunForm(g.frame, L"Substituir cor", f)) return;
        d->Checkpoint();
        fx::ReplaceColor(d->MutableBase(), FromColorref(f[0].color), f[2].value ? 0 : FromColorref(f[1].color), f[3].value);
        break;
    }
    case M_FX_WATERMARK: {
        std::vector<ui::Field> f = { ui::Field::MakeText(L"Texto", L"Confidencial"), ui::Field::MakeInt(L"Opacidade (%)", 35, 5, 100),
                                     ui::Field::MakeCheck(L"Repetir em mosaico", true), ui::Field::MakeCheck(L"Relevo", false) };
        if (!ui::RunForm(g.frame, L"Marca d'água", f)) return;
        d->Checkpoint(); FlattenIfNeeded();
        fx::Watermark(d->MutableBase(), f[0].text, f[1].value, f[2].value, f[3].value);
        break;
    }
    case M_FX_CAPTION: {
        std::vector<ui::Field> f = { ui::Field::MakeText(L"Texto", L"{date} {time} — {user}@{computer}"),
                                     ui::Field::MakeLabel(L"Tokens: {date} {time} {computer} {user}"),
                                     ui::Field::MakeCombo(L"Posição", { L"Embaixo", L"Em cima" }, 0), ui::Field::MakeInt(L"Tamanho da fonte", 16, 8, 72),
                                     ui::Field::MakeColor(L"Cor do texto", RGB(255, 255, 255)), ui::Field::MakeColor(L"Cor da faixa", RGB(32, 32, 32)) };
        if (!ui::RunForm(g.frame, L"Legenda", f)) return;
        bool top = f[2].value == 1;
        int bar = f[3].value * 2;
        d->Checkpoint();
        d->SetBase(fx::Caption(*d->st.base, f[0].text, top, f[3].value, FromColorref(f[4].color), FromColorref(f[5].color)));
        if (top) TransformObjects([bar](Gdiplus::PointF p) { return Gdiplus::PointF(p.X, p.Y + bar); });
        break;
    }
    default: return;
    }
    g.canvas.selected = std::min(g.canvas.selected, (int)d->st.objs.size() - 1);
    g.canvas.FitToWindow();
    g.canvas.Refresh();
    DocChanged();
    g.props.Load();
}

// ---------------------------------------------------------------- arquivo
bool SaveAs() {
    if (!RequireImageDoc()) return false;
    std::wstring base = g.cur->title;
    for (auto& c : base) if (wcschr(L"\\/:*?\"<>|", c)) c = L'_';
    if (FileExt(base).size() > 1 && FileExt(base).size() <= 5) base = StripExt(base);
    std::wstring path = DefaultSaveDir() + L"\\" + (base.empty() ? L"Captura" : base);
    static int fi = 1;
    if (!ui::SaveFileDialog(g.frame, ImageSaveFilter(), path, fi, L"png", L"Salvar como")) return false;
    static const wchar_t* exts[] = { L".png", L".jpg", L".bmp", L".gif", L".tif", L".pdf", L".snap" };
    std::wstring ext = FileExt(path);
    bool known = false;
    for (auto e : exts) if (ext == e || (ext == L".jpeg" && !wcscmp(e, L".jpg")) || (ext == L".tiff" && !wcscmp(e, L".tif"))) known = true;
    if (!known && fi >= 1 && fi <= 7) path += exts[fi - 1];
    int q = 90;
    if (FileExt(path) == L".jpg" || FileExt(path) == L".jpeg") {
        std::vector<ui::Field> f = { ui::Field::MakeInt(L"Qualidade JPEG (1–100)", 90, 1, 100) };
        if (!ui::RunForm(g.frame, L"JPEG", f)) return false;
        q = f[0].value;
    }
    bool ok;
    if (FileExt(path) == L".snap") {
        // cópia .snap fora da biblioteca: grava sem trocar o documento atual
        std::wstring old = g.cur->path; bool mod = g.cur->modified;
        g.cur->path = path; ok = g.cur->Save();
        g.cur->path = old; g.cur->modified = mod;
    } else ok = g.cur->SaveAs(path, q);
    if (!ok) { MessageBoxW(g.frame, (L"Não foi possível salvar:\n" + path).c_str(), L"Snapnab Editor", MB_ICONWARNING); return false; }
    SetStatus(L"Salvo em " + path);
    return true;
}

void SaveCurrent() {
    if (!RequireImageDoc()) return;
    if (g.cur->path.empty()) { SaveAs(); return; }
    if (g.cur->Save()) {
        WriteThumbnail(g.cur->path, g.cur->Render());
        g.tray.InvalidateThumb(g.cur->path);
        SetStatus(L"Salvo na biblioteca: " + FileNameOf(g.cur->path));
        UpdateTitle();
    }
}

void NewFromClipboard() {
    Image img;
    if (!GetClipboardImage(img)) { SetStatus(L"A área de transferência não contém imagem."); return; }
    NewDocFromImage(std::move(img), L"Da área de transferência", L"Área de transferência");
    SetStatus(L"Imagem nova criada a partir da área de transferência.");
}

void NewBlankImage() {
    std::vector<ui::Field> f = { ui::Field::MakeInt(L"Largura (px)", 1280, 1, 20000), ui::Field::MakeInt(L"Altura (px)", 720, 1, 20000),
                                 ui::Field::MakeColor(L"Cor de fundo", RGB(255, 255, 255)), ui::Field::MakeCheck(L"Transparente", false) };
    if (!ui::RunForm(g.frame, L"Nova imagem", f)) return;
    NewDocFromImage(Image(f[0].value, f[1].value, f[3].value ? 0 : FromColorref(f[2].color)), L"Imagem nova", L"Nova imagem");
}

void RequestCapture(int kind) {
    // o Editor não captura: pede ao processo de captura (IPC 0x12, ed_comp_14 §3.3)
    if (IsProcessRunning(kCaptureMutex)) g.ipc.SendInt(ipc::kCapture, ipc::EditorStartCapture, kind);
    else LaunchSibling(kCaptureExe, kind ? L"/video" : L"/capture");
}

void DuplicateCurrent() {
    if (!RequireImageDoc()) return;
    auto d = std::make_unique<Document>(*g.cur);
    d->ClearHistory();
    d->path = LibraryDir() + L"\\" + NewGuidString() + L".snap";
    d->title = g.cur->title + L" (cópia)";
    d->modified = true;
    Autosave(d.get());
    Document* p = AddDoc(std::move(d));
    g.tray.Reload();
    Activate(p);
}

void DeleteDoc(const std::wstring& path) {
    if (MessageBoxW(g.frame, (L"Enviar para a Lixeira?\n" + FileNameOf(path)).c_str(), L"Snapnab Editor", MB_YESNO | MB_ICONQUESTION) != IDYES) return;
    if (Document* d = FindDoc(path)) {
        d->modified = false;
        if (d == g.cur) { Activate(nullptr); }
        g.docs.erase(std::remove_if(g.docs.begin(), g.docs.end(), [&](auto& x) { return x.get() == d; }), g.docs.end());
    }
    DeleteLibraryItem(path, g.frame);
    g.tray.Reload();
    if (!g.cur && !g.docs.empty()) Activate(g.docs.back().get());
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
        s = std::min(s, dpiX / 96.0);                    // não amplia além do tamanho real
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
    // E-mail por MAPI simples (MAPI32, como o original): anexo PNG
    if (!RequireImageDoc()) return;
    std::wstring file = TempExport();
    HMODULE mapi = LoadLibraryW(L"MAPI32.DLL");
    auto send = mapi ? (ULONG(WINAPI*)(LHANDLE, ULONG_PTR, lpMapiMessageW, FLAGS, ULONG))GetProcAddress(mapi, "MAPISendMailW") : nullptr;
    if (!send) {
        if (mapi) FreeLibrary(mapi);
        std::wstring msg = L"Nenhum cliente de e-mail MAPI está configurado. A imagem foi salva em:\n" + file;
        MessageBoxW(g.frame, msg.c_str(), L"Snapnab Editor", MB_ICONINFORMATION);
        return;
    }
    MapiFileDescW fd{}; fd.nPosition = (ULONG)-1; fd.lpszPathName = (PWSTR)file.c_str();
    std::wstring fname = FileNameOf(file); fd.lpszFileName = (PWSTR)fname.c_str();
    MapiMessageW msg{}; std::wstring subj = L"Captura: " + g.cur->title;
    msg.lpszSubject = (PWSTR)subj.c_str(); msg.nFileCount = 1; msg.lpFiles = &fd;
    ULONG r = send(0, (ULONG_PTR)g.frame, &msg, MAPI_LOGON_UI | MAPI_DIALOG, 0);
    FreeLibrary(mapi);
    if (r != SUCCESS_SUCCESS && r != MAPI_USER_ABORT) MessageBoxW(g.frame, Format(L"Falha ao enviar e-mail (MAPI %u).", r).c_str(), L"Snapnab Editor", MB_ICONWARNING);
}

void OpenInProgram() {
    if (!RequireImageDoc()) return;
    std::wstring prog = RegGetString(L"ExternalProgram", L"mspaint.exe", L"Editor");
    std::vector<ui::Field> f = { ui::Field::MakeText(L"Programa", prog) };
    if (!ui::RunForm(g.frame, L"Abrir em programa externo", f)) return;
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
    if (!ok) { MessageBoxW(g.frame, (L"OCR indisponível: " + err).c_str(), L"Grab Text", MB_ICONWARNING); return; }
    if (Trim(text).empty()) text = L"(nenhum texto reconhecido)";
    ui::ShowTextWindow(g.frame, L"Grab Text — texto reconhecido", text);
}

void Paste() {
    Image img;
    if (!GetClipboardImage(img)) {
        // arquivo copiado no Explorer
        if (OpenClipboard(g.frame)) {
            if (HDROP drop = (HDROP)GetClipboardData(CF_HDROP)) {
                wchar_t f[MAX_PATH];
                if (DragQueryFileW(drop, 0, f, MAX_PATH)) LoadImageFile(f, img);
            }
            CloseClipboard();
        }
    }
    if (img.empty()) { SetStatus(L"Nada para colar."); return; }
    if (!g.cur || g.cur->isVideo) NewDocFromImage(std::move(img), L"Colado", L"Área de transferência");
    else { g.canvas.PasteImage(img); DocChanged(); }
}

// ---------------------------------------------------------------- menus
HMENU BuildMenu() {
    HMENU bar = CreateMenu();
    auto pop = [](std::initializer_list<std::pair<int, const wchar_t*>> items) {
        HMENU m = CreatePopupMenu();
        for (auto& [id, t] : items) { if (id == 0) AppendMenuW(m, MF_SEPARATOR, 0, nullptr); else AppendMenuW(m, MF_STRING, id, t); }
        return m;
    };
    HMENU file = pop({ { M_NEW_CAPTURE, L"Nova &captura\tCtrl+Shift+N" }, { M_NEW_VIDEO, L"Nova &gravação de vídeo" },
                       { M_NEW_CLIPBOARD, L"Novo da área de &transferência" }, { M_NEW_IMAGE, L"Nova &imagem…" }, { 0, nullptr },
                       { M_OPEN, L"&Abrir…\tCtrl+O" }, { M_SAVE, L"&Salvar\tCtrl+S" }, { M_SAVEAS, L"Salvar &como…\tCtrl+Shift+S" },
                       { M_DUPLICATE, L"&Duplicar" }, { M_DELETE, L"&Excluir (Lixeira)" }, { 0, nullptr },
                       { M_PRINT, L"&Imprimir…\tCtrl+P" }, { M_EMAIL, L"Enviar por &e-mail…" }, { M_OPEN_PROGRAM, L"Abrir em &programa externo…" }, { 0, nullptr },
                       { M_LIBRARY, L"Abrir a pasta da &biblioteca" }, { M_CLOSE, L"&Fechar documento\tCtrl+W" }, { M_EXIT, L"Sai&r" } });
    HMENU edit = pop({ { M_UNDO, L"&Desfazer\tCtrl+Z" }, { M_REDO, L"&Refazer\tCtrl+Y" }, { M_UNDOALL, L"Desfazer &tudo" }, { 0, nullptr },
                       { M_CUT, L"Recor&tar\tCtrl+X" }, { M_COPY, L"&Copiar\tCtrl+C" }, { M_COPYALL, L"Copiar t&udo\tCtrl+Shift+C" },
                       { M_PASTE, L"C&olar\tCtrl+V" }, { M_DELETE_SEL, L"E&xcluir\tDel" }, { M_SELECTALL, L"Selecionar tu&do\tCtrl+A" }, { 0, nullptr },
                       { M_FRONT, L"Trazer para a &frente" }, { M_BACK, L"Enviar para &trás" }, { M_FLATTEN, L"&Mesclar tudo" }, { 0, nullptr },
                       { M_OCR, L"&Grab Text (OCR)\tCtrl+Shift+T" } });
    HMENU fxm = pop({ { M_FX_BORDER, L"&Borda…" }, { M_FX_SHADOW, L"&Sombra…" }, { M_FX_EDGE, L"Borda &especial…" }, { 0, nullptr },
                      { M_FX_GRAY, L"Tons de &cinza" }, { M_FX_MONO, L"&Monocromático…" }, { M_FX_SEPIA, L"Sé&pia" }, { M_FX_INVERT, L"&Inverter cores" },
                      { M_FX_BC, L"Brilho e &contraste…" }, { M_FX_HS, L"Matiz e sa&turação…" }, { M_FX_REPLACE, L"Substituir co&r…" }, { 0, nullptr },
                      { M_FX_WATERMARK, L"Marca d'á&gua…" }, { M_FX_CAPTION, L"&Legenda…" } });
    HMENU img = pop({ { M_CROP_SEL, L"&Cortar para a seleção" }, { M_TRIM, L"Corte &automático" }, { 0, nullptr },
                      { M_ROT_CW, L"Girar 90° &horário\tCtrl+R" }, { M_ROT_CCW, L"Girar 90° &anti-horário" }, { M_ROT_180, L"Girar &180°" },
                      { M_ROT_FREE, L"Girar &livre…" }, { M_FLIP_H, L"Espelhar na &horizontal" }, { M_FLIP_V, L"Espelhar na &vertical" }, { 0, nullptr },
                      { M_RESIZE, L"&Redimensionar imagem…" }, { M_CANVAS, L"&Tamanho do canvas…" }, { M_CANVAS_COLOR, L"C&or do canvas…" } });
    AppendMenuW(img, MF_SEPARATOR, 0, nullptr);
    AppendMenuW(img, MF_POPUP, (UINT_PTR)fxm, L"&Efeitos");
    HMENU view = pop({ { M_ZOOM_IN, L"&Ampliar\tCtrl++" }, { M_ZOOM_OUT, L"&Reduzir\tCtrl+-" }, { M_ZOOM_100, L"&Tamanho real\tCtrl+1" },
                       { M_ZOOM_FIT, L"A&justar à janela\tCtrl+0" }, { 0, nullptr }, { M_BG_COLOR, L"&Cor de fundo da área…" },
                       { M_SHOW_TRAY, L"Mostrar a &bandeja da biblioteca" } });
    HMENU help = pop({ { M_SHORTCUTS, L"&Atalhos de teclado" }, { M_ABOUT, L"&Sobre o Snapnab Editor" } });
    AppendMenuW(bar, MF_POPUP, (UINT_PTR)file, L"&Arquivo");
    AppendMenuW(bar, MF_POPUP, (UINT_PTR)edit, L"&Editar");
    AppendMenuW(bar, MF_POPUP, (UINT_PTR)img, L"&Imagem");
    AppendMenuW(bar, MF_POPUP, (UINT_PTR)view, L"E&xibir");
    AppendMenuW(bar, MF_POPUP, (UINT_PTR)help, L"A&juda");
    return bar;
}

HACCEL BuildAccel() {
    ACCEL a[] = {
        { FCONTROL | FVIRTKEY, 'O', M_OPEN }, { FCONTROL | FVIRTKEY, 'S', M_SAVE }, { FCONTROL | FSHIFT | FVIRTKEY, 'S', M_SAVEAS },
        { FCONTROL | FSHIFT | FVIRTKEY, 'N', M_NEW_CAPTURE }, { FCONTROL | FVIRTKEY, 'P', M_PRINT }, { FCONTROL | FVIRTKEY, 'W', M_CLOSE },
        { FCONTROL | FVIRTKEY, 'Z', M_UNDO }, { FCONTROL | FVIRTKEY, 'Y', M_REDO }, { FCONTROL | FSHIFT | FVIRTKEY, 'Z', M_REDO },
        { FCONTROL | FVIRTKEY, 'X', M_CUT }, { FCONTROL | FVIRTKEY, 'C', M_COPY }, { FCONTROL | FSHIFT | FVIRTKEY, 'C', M_COPYALL },
        { FCONTROL | FVIRTKEY, 'V', M_PASTE }, { FCONTROL | FVIRTKEY, 'A', M_SELECTALL }, { FCONTROL | FSHIFT | FVIRTKEY, 'T', M_OCR },
        { FCONTROL | FVIRTKEY, 'R', M_ROT_CW }, { FCONTROL | FVIRTKEY, VK_OEM_PLUS, M_ZOOM_IN }, { FCONTROL | FVIRTKEY, VK_ADD, M_ZOOM_IN },
        { FCONTROL | FVIRTKEY, VK_OEM_MINUS, M_ZOOM_OUT }, { FCONTROL | FVIRTKEY, VK_SUBTRACT, M_ZOOM_OUT },
        { FCONTROL | FVIRTKEY, '1', M_ZOOM_100 }, { FCONTROL | FVIRTKEY, '0', M_ZOOM_FIT },
    };
    return CreateAcceleratorTableW(a, (int)(sizeof a / sizeof *a));
}

void SetTool(Tool t) {
    g.canvas.SetTool(t);
    g.toolbar.SetActive(t);
    g.props.Load();
    SetStatus(std::wstring(L"Ferramenta: ") + ToolName(t));
}

void ShowShortcuts() {
    ui::ShowTextWindow(g.frame, L"Atalhos do Snapnab Editor",
        L"Ferramentas (com a área de pintura em foco):\n"
        L"  V Mover   A Seta   L Linha   R Forma   P Caneta   H Marcador\n"
        L"  T Texto   C Balão   N Passo   K Carimbo   B Ocultar   G Destaque   Z Lupa\n"
        L"  F Preencher   E Borracha   X Cortar   S Seleção   U Recortar faixa   I Conta-gotas\n\n"
        L"Edição:  Ctrl+Z/Ctrl+Y desfazer/refazer • Ctrl+C/X/V • Ctrl+Shift+C copiar tudo • Del excluir\n"
        L"         Setas movem o objeto (Shift = 10 px) • Shift restringe proporção/ângulo\n"
        L"         Duplo clique edita texto • Esc cancela • Enter aplica o recorte\n"
        L"Exibir:  Ctrl+roda = zoom • Ctrl+0 ajustar • Ctrl+1 tamanho real • botão do meio arrasta\n"
        L"Arquivo: Ctrl+S salvar na biblioteca • Ctrl+Shift+S salvar como (PNG/JPG/BMP/GIF/TIFF/PDF/SNAP)\n"
        L"Texto:   Ctrl+Shift+T Grab Text (OCR)\n"
        L"Vídeo:   Espaço reproduz/pausa • ←/→ quadro • I/O marcam início/fim\n");
}

void OnCommand(int id) {
    if (id >= T_TOOL_BASE && id < T_TOOL_BASE + (int)Tool::Count) { SetTool((Tool)(id - T_TOOL_BASE)); return; }
    switch (id) {
    case T_CAPTURE: case M_NEW_CAPTURE: RequestCapture(0); break;
    case T_RECORD: case M_NEW_VIDEO: RequestCapture(1); break;
    case T_COPYALL: case M_COPYALL:
        if (RequireImageDoc() && CopyImageToClipboard(g.frame, g.cur->Render())) SetStatus(L"Imagem inteira copiada para a área de transferência.");
        break;
    case T_SAVE: case M_SAVEAS: SaveAs(); break;
    case T_SHARE: {
        HMENU m = CreatePopupMenu();
        AppendMenuW(m, MF_STRING, M_SAVEAS, L"Arquivo…");
        AppendMenuW(m, MF_STRING, M_COPYALL, L"Área de transferência");
        AppendMenuW(m, MF_STRING, M_EMAIL, L"E-mail…");
        AppendMenuW(m, MF_STRING, M_PRINT, L"Impressora…");
        AppendMenuW(m, MF_STRING, M_OPEN_PROGRAM, L"Programa externo…");
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
    case M_CUT: if (RequireImageDoc()) g.canvas.CopySelection(true); break;
    case M_COPY: if (RequireImageDoc() && g.canvas.CopySelection(false)) SetStatus(L"Copiado."); break;
    case M_PASTE: Paste(); break;
    case M_DELETE_SEL: if (RequireImageDoc()) g.canvas.DeleteSelected(); break;
    case M_SELECTALL: if (RequireImageDoc()) { g.toolbar.SetActive(Tool::Selection); g.canvas.SelectAll(); g.props.Load(); } break;
    case M_FLATTEN: if (RequireImageDoc()) g.canvas.FlattenAll(); break;
    case M_FRONT: g.canvas.BringToFront(); break;
    case M_BACK: g.canvas.SendToBack(); break;
    case M_OCR: GrabText(); break;
    case M_ZOOM_IN: g.canvas.ZoomIn(); break;
    case M_ZOOM_OUT: g.canvas.ZoomOut(); break;
    case M_ZOOM_100: g.canvas.ZoomTo(1.0); break;
    case M_ZOOM_FIT: g.canvas.FitToWindow(); break;
    case M_BG_COLOR: { COLORREF c = g.canvas.background; if (PickColor(g.frame, c)) { g.canvas.background = c; RegSetDword(L"CanvasBackground", c, L"Editor"); g.canvas.Refresh(false); } break; }
    case M_SHOW_TRAY:
        g.showTray = !g.showTray;
        CheckMenuItem(GetMenu(g.frame), M_SHOW_TRAY, g.showTray ? MF_CHECKED : MF_UNCHECKED);
        RegSetDword(L"ShowTray", g.showTray, L"Editor");
        Layout();
        break;
    case M_SHORTCUTS: ShowShortcuts(); break;
    case M_ABOUT:
        MessageBoxW(g.frame, (L"Snapnab Editor " + std::wstring(kAppVersion) + L"\n" SNAPNAB_COPYRIGHT_W L"\n\n"
                              L"Edição de capturas com objetos vetoriais, efeitos, Smart Move,\n"
                              L"OCR, exportação PDF e edição de vídeo (cortes e GIF).\n\n"
                              L"Software livre sob a GNU General Public License versão 3.\n\nBiblioteca: " + LibraryDir()).c_str(),
                    L"Sobre", MB_ICONINFORMATION);
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
    case ipc::LibraryChanged: g.tray.Reload(); break;
    default: break;
    }
}

// ---------------------------------------------------------------- linha de comando (ProcessUserCmdLineCommands)
void ProcessCommandLine(const std::wstring& cmdLine, bool startup) {
    auto args = SplitCommandLine(cmdLine);
    bool opened = false;
    for (size_t i = 0; i < args.size(); i++) {
        std::wstring a = ToLower(args[i]);
        if (a == L"/open" && i + 1 < args.size()) { opened |= OpenPath(args[++i]); }
        else if (a == L"/newfromclipboard") { NewFromClipboard(); opened = true; }
        else if (a == L"/restartwait") {
            // espera a instância anterior fechar
            for (int k = 0; k < 75 && FindWindowW(kEditorWndClass, nullptr) && FindWindowW(kEditorWndClass, nullptr) != g.frame; k++) Sleep(200);
        }
        else if (a == L"/help" || a == L"/?") {
            MessageBoxW(nullptr, L"SnapnabEditor.exe [opções] [arquivo…]\n\n/Open \"arquivo\"     abre um arquivo (.snap, imagem, .mp4)\n"
                                 L"/NewFromClipboard   cria imagem a partir da área de transferência\n/restartwait        espera a instância anterior fechar",
                        L"Snapnab Editor", MB_ICONINFORMATION);
        }
        else if (a[0] != L'/') opened |= OpenPath(args[i]);
    }
    if (startup && !opened) {
        // abre a captura mais recente da biblioteca
        auto items = ListLibrary(1);
        if (!items.empty()) OpenPath(items[0].path);
    }
    if (!startup) ShowEditor();
}

// ---------------------------------------------------------------- vigia da pasta da biblioteca (CDirectoryChangeWatcher)
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

// ---------------------------------------------------------------- janela principal
void PaintStatus(HDC dc) {
    RECT cr; GetClientRect(g.frame, &cr);
    RECT sr{ 0, cr.bottom - S(24), cr.right, cr.bottom };
    HBRUSH b = CreateSolidBrush(RGB(30, 120, 230)); FillRect(dc, &sr, b); DeleteObject(b);
    HGDIOBJ of = SelectObject(dc, ui::UiFont(g.dpi, 8));
    SetBkMode(dc, TRANSPARENT); SetTextColor(dc, RGB(255, 255, 255));
    RECT l = sr; l.left += S(10); l.right -= S(220);
    DrawTextW(dc, g.status.c_str(), -1, &l, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
    std::wstring right;
    if (g.cur && !g.cur->isVideo) right = Format(L"%d × %d px    Zoom %d%%", g.cur->w(), g.cur->h(), (int)std::lround(g.canvas.zoom * 100));
    RECT r = sr; r.right -= S(10);
    DrawTextW(dc, right.c_str(), -1, &r, DT_RIGHT | DT_VCENTER | DT_SINGLELINE);
    SelectObject(dc, of);
}

void CreateChildren() {
    g.toolbar.Create(g.frame);
    using I = icons::Id;
    auto& it = g.toolbar.items;
    it.push_back({ T_CAPTURE, I::Capture, L"Capturar", L"Nova captura de imagem (pede ao Snapnab)", false, Tool::Select, false });
    it.push_back({ T_RECORD, I::Video, L"Gravar", L"Nova gravação de vídeo", false, Tool::Select, false });
    struct TI { Tool t; I icon; const wchar_t* hint; } tools[] = {
        { Tool::Select, I::Select, L"Mover/selecionar objetos (V)" }, { Tool::Arrow, I::Arrow, L"Seta (A)" }, { Tool::Line, I::Line, L"Linha (L)" },
        { Tool::Shape, I::Rect, L"Forma: retângulo, elipse… (R)" }, { Tool::Text, I::Text, L"Texto (T)" }, { Tool::Callout, I::Callout, L"Balão (C)" },
        { Tool::Pen, I::Pen, L"Caneta (P)" }, { Tool::Highlighter, I::Highlighter, L"Marcador (H)" }, { Tool::Step, I::Step, L"Passos numerados (N)" },
        { Tool::Stamp, I::Stamp, L"Carimbo (K)" }, { Tool::Blur, I::Blur, L"Ocultar: desfoque/pixelado (B)" }, { Tool::Spotlight, I::Spotlight, L"Destaque/spotlight (G)" },
        { Tool::Magnify, I::Magnify, L"Lupa (Z)" }, { Tool::Fill, I::Fill, L"Preencher (F)" }, { Tool::Selection, I::Selection, L"Seleção, varinha, Smart Move (S)" },
        { Tool::Crop, I::Crop, L"Cortar (X)" }, { Tool::Cutout, I::Cutout, L"Recortar faixa (U)" }, { Tool::Eraser, I::Eraser, L"Borracha (E)" },
        { Tool::Eyedropper, I::Eyedropper, L"Conta-gotas (I)" } };
    for (auto& t : tools) it.push_back({ T_TOOL_BASE + (int)t.t, t.icon, ToolName(t.t), t.hint, true, t.t, false });
    it.push_back({ T_COPYALL, I::Copy, L"Copiar tudo", L"Copiar a imagem inteira (Ctrl+Shift+C)", false, Tool::Select, true });
    it.push_back({ T_SAVE, I::Save, L"Salvar como", L"Salvar como arquivo (Ctrl+Shift+S)", false, Tool::Select, true });
    it.push_back({ T_SHARE, I::Share, L"Compartilhar", L"Compartilhar: arquivo, área de transferência, e-mail, impressora, programa", false, Tool::Select, true });
    g.toolbar.onClick = [](int id) { OnCommand(id); };
    g.toolbar.onHint = [](const std::wstring& h) { if (!h.empty()) SetStatus(h); };

    g.canvas.Create(g.frame);
    g.canvas.background = RegGetDword(L"CanvasBackground", RGB(222, 224, 228), L"Editor");
    g.canvas.onDocChanged = [] { DocChanged(); };
    g.canvas.onSelectionChanged = [] { g.props.Load(); };
    g.canvas.onZoomChanged = [] { InvalidateStatus(); };
    g.canvas.onColorPicked = [](uint32_t c) {
        // conta-gotas: aplica a cor à ferramenta anterior mais comum (seta) e a todas as ferramentas de traço
        for (Tool t : { Tool::Arrow, Tool::Line, Tool::Shape, Tool::Pen, Tool::Step, Tool::Stamp, Tool::Fill, Tool::Eyedropper }) g.canvas.styles[t].stroke = c;
        g.canvas.styles[Tool::Text].textColor = c;
        SetStatus(L"Cor capturada: " + ColorToHex(ToColorref(c)) + L" (aplicada às ferramentas de desenho)");
        g.props.Load();
    };
    g.props.Create(g.frame, &g.canvas);
    g.props.onCommand = [](int id) {
        switch (id) {
        case 324: /*P_BTN1*/ if (g.canvas.tool == Tool::Crop) g.canvas.ApplyCrop(); else ImageCommand(M_CROP_SEL); break;
        case 325: /*P_BTN2*/ if (g.canvas.tool == Tool::Crop) ImageCommand(M_TRIM); else OnCommand(M_COPY); break;
        case 326: /*P_BTN3*/ g.canvas.DeleteSelected(); break;
        case 327: g.canvas.BringToFront(); break;
        case 328: g.canvas.SendToBack(); break;
        case 329: g.canvas.DeleteSelected(); break;
        }
    };
    g.video.Create(g.frame);
    g.video.onSaveFrame = [](const Image& img) { NewDocFromImage(img, L"Quadro do vídeo", L"Quadro de vídeo"); };
    g.video.onNewVideo = [](const std::wstring& p) { g.tray.Reload(); SetStatus(L"Vídeo salvo: " + p); };
    g.tray.Create(g.frame);
    g.tray.onOpen = [](const std::wstring& p) { OpenPath(p); };
    g.tray.onDelete = [](const std::wstring& p) { DeleteDoc(p); };
    g.tray.Reload();
    SetTool(Tool::Arrow);
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
        CreateChildren();
        DragAcceptFiles(h, TRUE);
        return 0;
    case WM_SIZE: Layout(); return 0;
    case WM_GETMINMAXINFO: ((MINMAXINFO*)l)->ptMinTrackSize = { S(760), S(520) }; return 0;
    case WM_DPICHANGED: {
        g.dpi = HIWORD(w);
        RECT* r = (RECT*)l;
        SetWindowPos(h, nullptr, r->left, r->top, r->right - r->left, r->bottom - r->top, SWP_NOZORDER | SWP_NOACTIVATE);
        Layout();
        return 0;
    }
    case WM_ERASEBKGND: return 1;
    case WM_PAINT: { PAINTSTRUCT ps; HDC dc = BeginPaint(h, &ps); PaintStatus(dc); EndPaint(h, &ps); return 0; }
    case WM_COMMAND: OnCommand(LOWORD(w)); return 0;
    case WM_INITMENUPOPUP: {
        HMENU mm = (HMENU)w;
        bool img = g.cur && !g.cur->isVideo;
        auto en = [&](int id, bool on) { EnableMenuItem(mm, id, MF_BYCOMMAND | (on ? MF_ENABLED : MF_GRAYED)); };
        en(M_UNDO, img && g.cur->CanUndo()); en(M_REDO, img && g.cur->CanRedo()); en(M_UNDOALL, img && g.cur->CanUndo());
        for (int id : { M_SAVE, M_SAVEAS, M_DUPLICATE, M_PRINT, M_EMAIL, M_OPEN_PROGRAM, M_COPYALL, M_SELECTALL, M_FLATTEN, M_OCR, M_CUT, M_COPY })
            en(id, img);
        en(M_FRONT, img && g.canvas.selected >= 0); en(M_BACK, img && g.canvas.selected >= 0);
        en(M_PASTE, ClipboardHasImage() || IsClipboardFormatAvailable(CF_HDROP));
        en(M_DELETE, g.cur != nullptr); en(M_CLOSE, g.cur != nullptr);
        CheckMenuItem(mm, M_SHOW_TRAY, g.showTray ? MF_CHECKED : MF_UNCHECKED);
        return 0;
    }
    case WM_DROPFILES: {
        HDROP d = (HDROP)w;
        UINT n = DragQueryFileW(d, 0xFFFFFFFF, nullptr, 0);
        for (UINT i = 0; i < n; i++) { wchar_t f[MAX_PATH]; DragQueryFileW(d, i, f, MAX_PATH); OpenPath(f); }
        DragFinish(d);
        return 0;
    }
    case WM_TIMER:
        if (w == 0x1E) {        // caso 0x1E do OnIPCNotify: comando "nova imagem do clipboard" e depois mostrar
            KillTimer(h, w);
            NewFromClipboard();
            ShowEditor();
        } else if (w == 0x60) { KillTimer(h, w); g.tray.Reload(); }
        return 0;
    case WM_LIB_CHANGED: SetTimer(h, 0x60, 400, nullptr); return 0;      // agrupa rajadas de mudanças
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
    // instância única: mutex + janela "SnapnabEditor"; a segunda instância repassa a linha de comando (WM_COPYDATA 0x8010)
    if (ForwardToExistingInstance(kEditorMutex, kEditorWndClass, &g.mutex)) return 0;
    SetCurrentDirectoryW(ExeDir().c_str());
    CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
    INITCOMMONCONTROLSEX icc{ sizeof(icc), ICC_STANDARD_CLASSES | ICC_UPDOWN_CLASS | ICC_HOTKEY_CLASS | ICC_PROGRESS_CLASS | ICC_BAR_CLASSES };
    InitCommonControlsEx(&icc);
    GdiplusSession gdip;
    LogInit(L"SnapnabEditor");
    g.hi = hi;
    g.showTray = RegGetDword(L"ShowTray", 1, L"Editor") != 0;

    // partida condicionada ao processo de captura (mutex + janela "SnapnabUI")
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
    HWND frame = CreateWindowExW(0, kEditorWndClass, L"Snapnab Editor", WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN, x, y, W, H,
                                 nullptr, BuildMenu(), hi, nullptr);
    g.accel = BuildAccel();
    g.ipc.Start(ipc::kEditor, [](const ipc::Message& m) { PostMessageW(g.frame, WM_SNAP_IPC, 0, (LPARAM)new ipc::Message(m)); });
    StartLibraryWatcher();
    ShowWindow(frame, maximize ? SW_SHOWMAXIMIZED : nShow);
    UpdateWindow(frame);
    ProcessCommandLine(GetCommandLineW(), true);
    SetForegroundWindow(frame);

    MSG msg;
    while (GetMessageW(&msg, nullptr, 0, 0)) {
        // edição de texto no lugar: Esc confirma; não intercepta atalhos de edição de texto
        if (g.canvas.EditingText()) {
            if (msg.message == WM_KEYDOWN && msg.wParam == VK_ESCAPE) { g.canvas.CommitText(); continue; }
            TranslateMessage(&msg); DispatchMessageW(&msg);
            continue;
        }
        wchar_t cls[32] = L"";
        GetClassNameW(GetFocus(), cls, 32);
        bool inEdit = !_wcsicmp(cls, L"Edit") || !_wcsicmp(cls, L"ComboBox") || !_wcsicmp(cls, L"msctls_hotkey32");
        if (!inEdit && TranslateAcceleratorW(g.frame, g.accel, &msg)) continue;
        if (msg.message == WM_KEYDOWN && GetFocus() == g.canvas.hwnd && GetKeyState(VK_CONTROL) >= 0 && GetKeyState(VK_MENU) >= 0 &&
            HandleToolKey(msg.wParam)) continue;
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
    g.ipc.Stop();
    if (g.watcher.joinable()) g.watcher.join();
    Log(L"SnapnabEditor encerrado");
    if (g.mutex) { ReleaseMutex(g.mutex); CloseHandle(g.mutex); }
    CoUninitialize();
    return 0;
}
