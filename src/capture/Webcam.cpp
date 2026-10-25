#include "Webcam.h"
#include "Ui.h"
#include <cmath>
#include <future>
#include <mfapi.h>
#include <mfidl.h>
#include <mfreadwrite.h>
#include <wrl/client.h>

namespace snap {

using Microsoft::WRL::ComPtr;

namespace {
constexpr UINT WM_CAM_FRAME = WM_APP + 1;
constexpr wchar_t kClass[] = L"SnapnabWebcam";
WebcamOverlay* g_cam = nullptr;
int Sc(int v, int dpi) { return MulDiv(v, dpi, 96); }

std::wstring AttrString(IMFActivate* a, const GUID& key) {
    wchar_t* s = nullptr; UINT32 n = 0;
    std::wstring r;
    if (SUCCEEDED(a->GetAllocatedString(key, &s, &n))) { r = s; CoTaskMemFree(s); }
    return r;
}
} // namespace

std::vector<CameraInfo> ListCameras() {
    std::vector<CameraInfo> out;
    if (FAILED(MFStartup(MF_VERSION, MFSTARTUP_LITE))) return out;
    ComPtr<IMFAttributes> attr;
    if (SUCCEEDED(MFCreateAttributes(&attr, 1)) &&
        SUCCEEDED(attr->SetGUID(MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID))) {
        IMFActivate** dev = nullptr; UINT32 n = 0;
        if (SUCCEEDED(MFEnumDeviceSources(attr.Get(), &dev, &n))) {
            for (UINT32 i = 0; i < n; i++) {
                out.push_back({ AttrString(dev[i], MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME),
                                AttrString(dev[i], MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK) });
                dev[i]->Release();
            }
            CoTaskMemFree(dev);
        }
    }
    MFShutdown();
    return out;
}

const wchar_t* CamShapeName(CamShape s) {
    switch (s) {
    case CamShape::Circle: return Tr(L"Círculo");
    case CamShape::RoundedSquare: return Tr(L"Quadrado arredondado");
    case CamShape::Square: return Tr(L"Quadrado");
    case CamShape::Wide: return Tr(L"Retângulo 16:9");
    case CamShape::Classic: return Tr(L"Retângulo 4:3");
    default: return L"?";
    }
}

// distância de crominância (Cb/Cr, BT.601) até a cor-chave: a luminância não conta, então sombras e dobras do
// fundo continuam sendo fundo. Abaixo de t0 some, acima de t0 + faixa fica, no meio é borda suave.
uint8_t ChromaAlpha(uint32_t px, COLORREF key, int tolerance) {
    auto cbcr = [](double r, double g, double b, double& cb, double& cr) {
        cb = -0.168736 * r - 0.331264 * g + 0.5 * b;
        cr = 0.5 * r - 0.418688 * g - 0.081312 * b;
    };
    double kcb, kcr, cb, cr;
    cbcr(GetRValue(key), GetGValue(key), GetBValue(key), kcb, kcr);
    cbcr((px >> 16) & 255, (px >> 8) & 255, px & 255, cb, cr);
    double d = std::hypot(cb - kcb, cr - kcr);
    double t0 = 8 + std::clamp(tolerance, 0, 100) * 0.6, band = 18;
    double a = std::clamp((d - t0) / band, 0.0, 1.0);
    return (uint8_t)std::lround(a * 255);
}

bool WebcamOverlay::Start(const std::wstring& linkIn, RECT area, const WebcamStyle& style, std::wstring& err) {
    Stop();
    style_ = style;
    std::wstring link = linkIn;
    auto cams = ListCameras();
    if (cams.empty()) { err = Tr(L"Nenhuma webcam encontrada."); return false; }
    if (link.empty() || std::none_of(cams.begin(), cams.end(), [&](auto& c) { return c.link == link; })) link = cams[0].link;

    static bool reg = false;
    HINSTANCE hi = GetModuleHandleW(nullptr);
    if (!reg) {
        WNDCLASSW wc{}; wc.lpfnWndProc = Proc; wc.hInstance = hi; wc.hCursor = LoadCursor(nullptr, IDC_SIZEALL); wc.lpszClassName = kClass;
        RegisterClassW(&wc);
        reg = true;
    }
    g_cam = this;
    hwnd_ = CreateWindowExW(WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE, kClass, Tr(L"Snapnab — webcam"),
                            WS_POPUP, 0, 0, 10, 10, nullptr, nullptr, hi, nullptr);

    std::promise<std::wstring> ready;
    auto fut = ready.get_future();
    running_ = true;
    thread_ = std::thread([this, link, p = std::move(ready)]() mutable {
        CoInitializeEx(nullptr, COINIT_MULTITHREADED);
        MFStartup(MF_VERSION, MFSTARTUP_LITE);
        ComPtr<IMFAttributes> attr;
        ComPtr<IMFMediaSource> src;
        ComPtr<IMFSourceReader> reader;
        HRESULT hr = MFCreateAttributes(&attr, 2);
        if (SUCCEEDED(hr)) hr = attr->SetGUID(MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID);
        if (SUCCEEDED(hr)) hr = attr->SetString(MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK, link.c_str());
        if (SUCCEEDED(hr)) hr = MFCreateDeviceSource(attr.Get(), &src);
        if (FAILED(hr)) {
            p.set_value(hr == E_ACCESSDENIED
                ? Tr(L"O Windows bloqueou o acesso à câmera. Libere em Configurações › Privacidade e segurança › Câmera.")
                : Format(Tr(L"Não foi possível abrir a webcam (0x%08X)."), (unsigned)hr));
        } else {
            ComPtr<IMFAttributes> ra;
            MFCreateAttributes(&ra, 1);
            ra->SetUINT32(MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, TRUE);       // converte qualquer formato para RGB32
            hr = MFCreateSourceReaderFromMediaSource(src.Get(), ra.Get(), &reader);
            // formato nativo mais próximo de 640 px de largura (leve e suficiente para a bolha)
            ComPtr<IMFMediaType> best;
            UINT32 bestW = 0, bestH = 0;
            for (DWORD i = 0; SUCCEEDED(hr); i++) {
                ComPtr<IMFMediaType> t;
                if (FAILED(reader->GetNativeMediaType((DWORD)MF_SOURCE_READER_FIRST_VIDEO_STREAM, i, &t))) break;
                UINT32 w = 0, h = 0;
                MFGetAttributeSize(t.Get(), MF_MT_FRAME_SIZE, &w, &h);
                if (!best || std::abs((int)w - 640) < std::abs((int)bestW - 640)) { best = t; bestW = w; bestH = h; }
            }
            if (SUCCEEDED(hr) && best) hr = reader->SetCurrentMediaType((DWORD)MF_SOURCE_READER_FIRST_VIDEO_STREAM, nullptr, best.Get());
            if (bestW && bestH) aspect_ = (double)bestW / bestH;
            ComPtr<IMFMediaType> out;
            if (SUCCEEDED(hr)) hr = MFCreateMediaType(&out);
            if (SUCCEEDED(hr)) {
                out->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Video);
                out->SetGUID(MF_MT_SUBTYPE, MFVideoFormat_RGB32);
                if (bestW) MFSetAttributeSize(out.Get(), MF_MT_FRAME_SIZE, bestW, bestH);
                hr = reader->SetCurrentMediaType((DWORD)MF_SOURCE_READER_FIRST_VIDEO_STREAM, nullptr, out.Get());
            }
            p.set_value(SUCCEEDED(hr) ? L"" : Format(Tr(L"A webcam não oferece um formato compatível (0x%08X)."), (unsigned)hr));
            if (SUCCEEDED(hr)) ReadLoop(reader.Get());
            reader.Reset();
            src->Shutdown();
        }
        MFShutdown();
        CoUninitialize();
    });
    err = fut.get();
    if (!err.empty()) { Stop(); return false; }
    // tamanho pelo formato (com chroma key, a imagem inteira da câmera, sem moldura); canto inferior direito da área
    int dpi = DpiForWindow(hwnd_);
    int h = Sc(style_.chroma ? 220 : 176, dpi), w = h;
    if (style_.chroma) w = (int)std::lround(h * aspect_);
    else if (style_.shape == CamShape::Wide) w = h * 16 / 9;
    else if (style_.shape == CamShape::Classic) w = h * 4 / 3;
    int sh = style_.chroma ? 0 : Sc(8, dpi);
    int W = w + 2 * sh, H = h + 2 * sh, m = Sc(12, dpi);
    int x = std::max<int>(area.left, area.right - W - m), y = std::max<int>(area.top, area.bottom - H - m);
    SetWindowPos(hwnd_, HWND_TOPMOST, x, y, W, H, SWP_NOACTIVATE);
    Render();
    ShowWindow(hwnd_, SW_SHOWNOACTIVATE);
    Log(L"Webcam: iniciada (%s)", link.c_str());
    return true;
}

void WebcamOverlay::ReadLoop(void* r) {
    auto* reader = (IMFSourceReader*)r;
    UINT32 w = 0, h = 0;
    INT32 defStride = 0;
    {
        ComPtr<IMFMediaType> cur;
        if (SUCCEEDED(reader->GetCurrentMediaType((DWORD)MF_SOURCE_READER_FIRST_VIDEO_STREAM, &cur))) {
            MFGetAttributeSize(cur.Get(), MF_MT_FRAME_SIZE, &w, &h);
            defStride = (INT32)MFGetAttributeUINT32(cur.Get(), MF_MT_DEFAULT_STRIDE, 0);
        }
    }
    if (!w || !h) return;
    if (!defStride) defStride = -(INT32)(w * 4);      // RGB32 sem indicação: de baixo para cima
    while (running_) {
        DWORD idx = 0, flags = 0; LONGLONG ts = 0;
        ComPtr<IMFSample> sample;
        if (FAILED(reader->ReadSample((DWORD)MF_SOURCE_READER_FIRST_VIDEO_STREAM, 0, &idx, &flags, &ts, &sample))) break;
        if (flags & (MF_SOURCE_READERF_ERROR | MF_SOURCE_READERF_ENDOFSTREAM)) break;
        if (!sample) continue;
        ComPtr<IMFMediaBuffer> buf;
        if (FAILED(sample->ConvertToContiguousBuffer(&buf))) continue;
        BYTE* scan0 = nullptr; LONG pitch = 0;
        ComPtr<IMF2DBuffer> b2;
        BYTE* raw = nullptr; DWORD len = 0;
        bool locked2d = SUCCEEDED(buf.As(&b2)) && SUCCEEDED(b2->Lock2D(&scan0, &pitch));
        if (!locked2d) {
            if (FAILED(buf->Lock(&raw, nullptr, &len))) continue;
            pitch = defStride;
            scan0 = pitch < 0 ? raw + (size_t)(h - 1) * (size_t)(-pitch) : raw;
        }
        Image img((int)w, (int)h);
        for (UINT32 y = 0; y < h; y++) {
            const uint32_t* s = (const uint32_t*)(scan0 + (LONG_PTR)y * pitch);
            uint32_t* d = &img.px[(size_t)y * w];
            for (UINT32 x = 0; x < w; x++) d[x] = s[x] | 0xFF000000;
        }
        if (locked2d) b2->Unlock2D(); else buf->Unlock();
        {
            std::lock_guard<std::mutex> lk(frameMutex_);
            frame_ = std::move(img);
        }
        if (hwnd_) PostMessageW(hwnd_, WM_CAM_FRAME, 0, 0);
    }
}

void WebcamOverlay::Render() {
    if (!hwnd_) return;
    RECT wr; GetWindowRect(hwnd_, &wr);
    int W = wr.right - wr.left, H = wr.bottom - wr.top;
    int dpi = DpiForWindow(hwnd_);
    const bool chroma = style_.chroma;
    int sh = chroma ? 0 : Sc(8, dpi);
    float cw = (float)(W - 2 * sh), ch = (float)(H - 2 * sh);       // área da câmera
    if (cw < 4 || ch < 4) return;
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
        g.SetInterpolationMode(Gdiplus::InterpolationModeHighQualityBilinear);
        g.SetPixelOffsetMode(Gdiplus::PixelOffsetModeHalf);
        g.Clear(Gdiplus::Color(0, 0, 0, 0));
        // contorno do formato escolhido
        auto shapePath = [&](Gdiplus::GraphicsPath& p, Gdiplus::RectF r) {
            float rad = 0;
            switch (style_.shape) {
            case CamShape::Circle: p.AddEllipse(r); return;
            case CamShape::RoundedSquare: rad = r.Height * 0.22f; break;
            case CamShape::Square: rad = (float)Sc(3, dpi); break;
            default: rad = (float)Sc(14, dpi); break;
            }
            float d = rad * 2;
            p.AddArc(r.X, r.Y, d, d, 180, 90); p.AddArc(r.GetRight() - d, r.Y, d, d, 270, 90);
            p.AddArc(r.GetRight() - d, r.GetBottom() - d, d, d, 0, 90); p.AddArc(r.X, r.GetBottom() - d, d, d, 90, 90);
            p.CloseFigure();
        };
        Gdiplus::RectF cam((float)sh, (float)sh, cw, ch);
        Gdiplus::GraphicsPath outline;
        if (!chroma) {
            for (int i = sh; i >= 1; i--) {         // sombra
                double t = 1.0 - (double)i / (sh + 1);
                Gdiplus::RectF s = cam; s.Inflate((float)i, (float)i); s.Offset(0, i * 0.4f);
                Gdiplus::GraphicsPath sp; shapePath(sp, s);
                Gdiplus::SolidBrush sb(Gdiplus::Color((BYTE)std::lround(14 * t * t), 0, 0, 0));
                g.FillPath(&sb, &sp);
            }
            shapePath(outline, cam);
            Gdiplus::SolidBrush back(Gdiplus::Color(255, 40, 40, 44));
            g.FillPath(&back, &outline);
        }
        Image f;
        { std::lock_guard<std::mutex> lk(frameMutex_); f = frame_; }
        if (!f.empty()) {
            // recorte central na proporção da janela, espelhado (como um espelho, igual às chamadas de vídeo)
            double want = cw / ch;
            float sw = (float)f.w, shh = (float)f.h;
            if (sw / shh > want) sw = (float)(shh * want); else shh = (float)(sw / want);
            if (chroma) {
                // fundo removido: alfa por pixel (pré-multiplicado) e cor da borda puxada para o cinza, para não
                // sobrar halo da cor-chave
                for (auto& p : f.px) {
                    uint8_t a = ChromaAlpha(p, style_.key, style_.tolerance);
                    if (a == 255) continue;
                    int r = (p >> 16) & 255, gg = (p >> 8) & 255, b = p & 255;
                    int y = (r * 77 + gg * 150 + b * 29) >> 8;
                    double k = 1.0 - a / 255.0;
                    r = (int)(r + (y - r) * k); gg = (int)(gg + (y - gg) * k); b = (int)(b + (y - b) * k);
                    p = ((uint32_t)a << 24) | ((uint32_t)(r * a / 255) << 16) | ((uint32_t)(gg * a / 255) << 8) | (uint32_t)(b * a / 255);
                }
            }
            Gdiplus::Bitmap src(f.w, f.h, f.w * 4, chroma ? PixelFormat32bppPARGB : PixelFormat32bppRGB, (BYTE*)f.px.data());
            if (!chroma) g.SetClip(&outline);
            Gdiplus::GraphicsState st = g.Save();
            g.TranslateTransform(sh + cw, (float)sh);
            g.ScaleTransform(-1, 1);
            g.DrawImage(&src, Gdiplus::RectF(0, 0, cw, ch), (f.w - sw) / 2, (f.h - shh) / 2, sw, shh, Gdiplus::UnitPixel);
            g.Restore(st);
            g.ResetClip();
        }
        if (!chroma) {
            Gdiplus::RectF rr = cam; rr.Inflate(-1.0f, -1.0f);
            Gdiplus::GraphicsPath ringPath; shapePath(ringPath, rr);
            Gdiplus::Pen ring(Gdiplus::Color(255, 255, 255, 255), (float)Sc(3, dpi));
            g.DrawPath(&ring, &ringPath);
        }
    }
    POINT src{ 0, 0 }, dst{ wr.left, wr.top };
    SIZE sz{ W, H };
    BLENDFUNCTION bf{ AC_SRC_OVER, 0, 255, AC_SRC_ALPHA };
    UpdateLayeredWindow(hwnd_, screen, &dst, &sz, mem, &src, 0, &bf, ULW_ALPHA);
    SelectObject(mem, ob); DeleteObject(bmp); DeleteDC(mem);
    ReleaseDC(nullptr, screen);
}

void WebcamOverlay::SetVisible(bool on) { if (hwnd_) ShowWindow(hwnd_, on ? SW_SHOWNOACTIVATE : SW_HIDE); }

void WebcamOverlay::Stop() {
    running_ = false;
    if (thread_.joinable()) thread_.join();
    if (hwnd_) { DestroyWindow(hwnd_); hwnd_ = nullptr; }
    std::lock_guard<std::mutex> lk(frameMutex_);
    frame_ = Image();
}

LRESULT CALLBACK WebcamOverlay::Proc(HWND h, UINT m, WPARAM w, LPARAM l) {
    WebcamOverlay* s = g_cam;
    switch (m) {
    case WM_CAM_FRAME:
        if (s && s->hwnd_ == h) {
            MSG drop;                                   // descarta quadros acumulados: desenha só o mais recente
            while (PeekMessageW(&drop, h, WM_CAM_FRAME, WM_CAM_FRAME, PM_REMOVE)) {}
            s->Render();
        }
        return 0;
    case WM_NCHITTEST: return HTCAPTION;                 // arrastar a bolha para qualquer lugar
    case WM_MOUSEACTIVATE: return MA_NOACTIVATE;
    }
    return DefWindowProcW(h, m, w, l);
}

} // namespace snap
