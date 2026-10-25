#include "Emoji.h"
#include <d2d1_1.h>
#include <dwrite.h>
#include <wincodec.h>
#include <wrl/client.h>

namespace snap {

using Microsoft::WRL::ComPtr;

namespace {
struct Factories {
    ComPtr<ID2D1Factory1> d2d;
    ComPtr<IDWriteFactory> dw;
    ComPtr<IWICImagingFactory> wic;
    bool ok = false;
    Factories() {
        ok = SUCCEEDED(D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, __uuidof(ID2D1Factory1), nullptr, (void**)d2d.GetAddressOf())) &&
             SUCCEEDED(DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED, __uuidof(IDWriteFactory), (IUnknown**)dw.GetAddressOf())) &&
             SUCCEEDED(CoCreateInstance(CLSID_WICImagingFactory, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&wic)));
    }
};

Factories& F() { static Factories f; return f; }
}

Image RenderEmoji(const std::wstring& text, int px) {
    px = std::clamp(px, 8, 2048);
    Factories& f = F();
    if (!f.ok || text.empty()) return {};
    ComPtr<IWICBitmap> bmp;
    if (FAILED(f.wic->CreateBitmap(px, px, GUID_WICPixelFormat32bppPBGRA, WICBitmapCacheOnDemand, &bmp))) return {};
    D2D1_RENDER_TARGET_PROPERTIES rp = D2D1::RenderTargetProperties(D2D1_RENDER_TARGET_TYPE_DEFAULT,
        D2D1::PixelFormat(DXGI_FORMAT_B8G8R8A8_UNORM, D2D1_ALPHA_MODE_PREMULTIPLIED));
    ComPtr<ID2D1RenderTarget> rt;
    if (FAILED(f.d2d->CreateWicBitmapRenderTarget(bmp.Get(), rp, &rt))) return {};
    ComPtr<IDWriteTextFormat> fmt;
    float size = px * 0.80f;
    if (FAILED(f.dw->CreateTextFormat(L"Segoe UI Emoji", nullptr, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STYLE_NORMAL,
                                      DWRITE_FONT_STRETCH_NORMAL, size, L"pt-br", &fmt))) return {};
    fmt->SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
    fmt->SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
    fmt->SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
    ComPtr<ID2D1SolidColorBrush> brush;
    rt->CreateSolidColorBrush(D2D1::ColorF(0.2f, 0.2f, 0.22f), &brush);
    rt->SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
    rt->BeginDraw();
    rt->Clear(D2D1::ColorF(0, 0, 0, 0));
    // um pouco maior que o quadrado na vertical: a caixa da fonte tem respiro acima e abaixo do desenho
    rt->DrawText(text.c_str(), (UINT32)text.size(), fmt.Get(), D2D1::RectF(-px * 0.1f, -px * 0.12f, px * 1.1f, px * 1.06f), brush.Get(),
                 D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT);
    if (FAILED(rt->EndDraw())) return {};
    Image out(px, px);
    WICRect wr{ 0, 0, px, px };
    if (FAILED(bmp->CopyPixels(&wr, px * 4, (UINT)(out.px.size() * 4), (BYTE*)out.px.data()))) return {};
    for (auto& p : out.px) {           // pré-multiplicado → alfa direto
        int a = CA(p);
        if (a == 0) { p = 0; continue; }
        if (a < 255) p = ARGB(a, std::min(255, CR(p) * 255 / a), std::min(255, CG(p) * 255 / a), std::min(255, CB(p) * 255 / a));
    }
    return out;
}

} // namespace snap
