#include "Image.h"
#include <wincodec.h>
#include <wrl/client.h>

using Microsoft::WRL::ComPtr;

namespace snap {

struct ComScope {
    HRESULT hr;
    ComScope() { hr = CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED); }
    ~ComScope() { if (SUCCEEDED(hr)) CoUninitialize(); }
};

uint32_t BlendOver(uint32_t d, uint32_t s) {
    int sa = CA(s);
    if (sa == 255) return s;
    if (sa == 0) return d;
    int da = CA(d);
    int oa = sa + da * (255 - sa) / 255;
    if (oa == 0) return 0;
    auto ch = [&](int sc, int dc) { return (sc * sa + dc * da * (255 - sa) / 255) / oa; };
    return ARGB(oa, ch(CR(s), CR(d)), ch(CG(s), CG(d)), ch(CB(s), CB(d)));
}

Image Image::Crop(RECT r) const {
    r.left = std::max<LONG>(0, r.left); r.top = std::max<LONG>(0, r.top);
    r.right = std::min<LONG>(w, r.right); r.bottom = std::min<LONG>(h, r.bottom);
    if (r.right <= r.left || r.bottom <= r.top) return {};
    Image out(r.right - r.left, r.bottom - r.top);
    for (int y = 0; y < out.h; y++)
        memcpy(out.row(y), row(r.top + y) + r.left, out.w * 4);
    return out;
}

void Image::Blit(const Image& src, int dx, int dy, bool blend) {
    for (int y = 0; y < src.h; y++) {
        int ty = dy + y;
        if (ty < 0 || ty >= h) continue;
        for (int x = 0; x < src.w; x++) {
            int tx = dx + x;
            if (tx < 0 || tx >= w) continue;
            uint32_t s = src.at(x, y);
            at(tx, ty) = blend ? BlendOver(at(tx, ty), s) : s;
        }
    }
}

void Image::FillRect(RECT r, uint32_t color) {
    r.left = std::max<LONG>(0, r.left); r.top = std::max<LONG>(0, r.top);
    r.right = std::min<LONG>(w, r.right); r.bottom = std::min<LONG>(h, r.bottom);
    for (int y = r.top; y < r.bottom; y++)
        std::fill(row(y) + r.left, row(y) + r.right, color);
}

bool Image::HasTransparency() const {
    for (uint32_t p : px) if (CA(p) != 255) return true;
    return false;
}

void Image::MakeOpaque() { for (auto& p : px) p |= 0xFF000000u; }

HBITMAP Image::ToHBITMAP() const {
    BITMAPINFO bi{};
    bi.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
    bi.bmiHeader.biWidth = w; bi.bmiHeader.biHeight = -h;
    bi.bmiHeader.biPlanes = 1; bi.bmiHeader.biBitCount = 32; bi.bmiHeader.biCompression = BI_RGB;
    void* bits = nullptr;
    HBITMAP hbm = CreateDIBSection(nullptr, &bi, DIB_RGB_COLORS, &bits, nullptr, 0);
    if (hbm && bits) memcpy(bits, px.data(), px.size() * 4);
    return hbm;
}

Image Image::FromHBITMAP(HBITMAP hbm) {
    BITMAP bm{};
    if (!GetObjectW(hbm, sizeof(bm), &bm)) return {};
    Image img(bm.bmWidth, std::abs(bm.bmHeight));
    BITMAPINFO bi{};
    bi.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
    bi.bmiHeader.biWidth = img.w; bi.bmiHeader.biHeight = -img.h;
    bi.bmiHeader.biPlanes = 1; bi.bmiHeader.biBitCount = 32; bi.bmiHeader.biCompression = BI_RGB;
    HDC dc = GetDC(nullptr);
    GetDIBits(dc, hbm, 0, img.h, img.px.data(), &bi, DIB_RGB_COLORS);
    ReleaseDC(nullptr, dc);
    if (bm.bmBitsPixel < 32) img.MakeOpaque();
    else {
        bool allZero = true;
        for (uint32_t p : img.px) if (CA(p)) { allZero = false; break; }
        if (allZero) img.MakeOpaque();
    }
    return img;
}

Image Image::FromDC(HDC src, RECT r) {
    int cw = r.right - r.left, ch = r.bottom - r.top;
    if (cw <= 0 || ch <= 0) return {};
    Image img(cw, ch);
    BITMAPINFO bi{};
    bi.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
    bi.bmiHeader.biWidth = cw; bi.bmiHeader.biHeight = -ch;
    bi.bmiHeader.biPlanes = 1; bi.bmiHeader.biBitCount = 32; bi.bmiHeader.biCompression = BI_RGB;
    void* bits = nullptr;
    HBITMAP hbm = CreateDIBSection(src, &bi, DIB_RGB_COLORS, &bits, nullptr, 0);
    HDC mem = CreateCompatibleDC(src);
    HGDIOBJ old = SelectObject(mem, hbm);
    BitBlt(mem, 0, 0, cw, ch, src, r.left, r.top, SRCCOPY | CAPTUREBLT);
    GdiFlush();
    memcpy(img.px.data(), bits, img.px.size() * 4);
    SelectObject(mem, old);
    DeleteDC(mem); DeleteObject(hbm);
    img.MakeOpaque();
    return img;
}

std::unique_ptr<Gdiplus::Bitmap> Image::WrapGdip() {
    return std::make_unique<Gdiplus::Bitmap>(w, h, w * 4, PixelFormat32bppARGB, (BYTE*)px.data());
}

Image Image::FromGdip(Gdiplus::Bitmap& bmp) {
    Image img(bmp.GetWidth(), bmp.GetHeight());
    Gdiplus::BitmapData bd{};
    bd.Width = img.w; bd.Height = img.h; bd.Stride = img.w * 4;
    bd.PixelFormat = PixelFormat32bppARGB; bd.Scan0 = img.px.data();
    Gdiplus::Rect rc(0, 0, img.w, img.h);
    bmp.LockBits(&rc, Gdiplus::ImageLockModeRead | Gdiplus::ImageLockModeUserInputBuf, PixelFormat32bppARGB, &bd);
    bmp.UnlockBits(&bd);
    return img;
}

void Image::DrawTo(HDC hdc, int x, int y, int dw, int dh, bool smooth) const {
    if (empty()) return;
    BITMAPINFO bi{};
    bi.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
    bi.bmiHeader.biWidth = w; bi.bmiHeader.biHeight = -h;
    bi.bmiHeader.biPlanes = 1; bi.bmiHeader.biBitCount = 32; bi.bmiHeader.biCompression = BI_RGB;
    int oldMode = SetStretchBltMode(hdc, smooth ? HALFTONE : COLORONCOLOR);
    if (smooth) SetBrushOrgEx(hdc, 0, 0, nullptr);
    StretchDIBits(hdc, x, y, dw, dh, 0, 0, w, h, px.data(), &bi, DIB_RGB_COLORS, SRCCOPY);
    SetStretchBltMode(hdc, oldMode);
}

// ---------------------------------------------------------------- WIC
ImageFormat FormatFromExt(const std::wstring& extIn) {
    std::wstring e = ToLower(extIn);
    if (e == L".png") return ImageFormat::PNG;
    if (e == L".jpg" || e == L".jpeg" || e == L".jpe") return ImageFormat::JPEG;
    if (e == L".bmp") return ImageFormat::BMP;
    if (e == L".gif") return ImageFormat::GIF;
    if (e == L".tif" || e == L".tiff") return ImageFormat::TIFF;
    return ImageFormat::Unknown;
}

static ComPtr<IWICImagingFactory> Factory() {
    ComPtr<IWICImagingFactory> f;
    CoCreateInstance(CLSID_WICImagingFactory, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&f));
    return f;
}

static bool DecodeWith(IWICImagingFactory* f, IWICBitmapDecoder* dec, Image& out) {
    ComPtr<IWICBitmapFrameDecode> frame;
    if (FAILED(dec->GetFrame(0, &frame))) return false;
    ComPtr<IWICFormatConverter> conv;
    if (FAILED(f->CreateFormatConverter(&conv))) return false;
    if (FAILED(conv->Initialize(frame.Get(), GUID_WICPixelFormat32bppBGRA, WICBitmapDitherTypeNone, nullptr, 0,
                                WICBitmapPaletteTypeCustom))) return false;
    UINT w, h; conv->GetSize(&w, &h);
    out = Image(w, h);
    return SUCCEEDED(conv->CopyPixels(nullptr, w * 4, (UINT)out.px.size() * 4, (BYTE*)out.px.data()));
}

bool LoadImageFile(const std::wstring& path, Image& out) {
    ComScope cs;
    auto f = Factory();
    if (!f) return false;
    ComPtr<IWICBitmapDecoder> dec;
    if (FAILED(f->CreateDecoderFromFilename(path.c_str(), nullptr, GENERIC_READ, WICDecodeMetadataCacheOnDemand, &dec)))
        return false;
    return DecodeWith(f.Get(), dec.Get(), out);
}

bool DecodeImage(IStream* stream, Image& out) {
    ComScope cs;
    auto f = Factory();
    if (!f) return false;
    ComPtr<IWICBitmapDecoder> dec;
    if (FAILED(f->CreateDecoderFromStream(stream, nullptr, WICDecodeMetadataCacheOnDemand, &dec))) return false;
    return DecodeWith(f.Get(), dec.Get(), out);
}

bool EncodeImage(IStream* stream, const Image& imgIn, ImageFormat fmt, int quality) {
    ComScope cs;
    auto f = Factory();
    if (!f || imgIn.empty()) return false;
    GUID container;
    switch (fmt) {
    case ImageFormat::PNG: container = GUID_ContainerFormatPng; break;
    case ImageFormat::JPEG: container = GUID_ContainerFormatJpeg; break;
    case ImageFormat::BMP: container = GUID_ContainerFormatBmp; break;
    case ImageFormat::GIF: container = GUID_ContainerFormatGif; break;
    case ImageFormat::TIFF: container = GUID_ContainerFormatTiff; break;
    default: return false;
    }
    // JPEG/BMP não têm alfa: compõe sobre branco
    const Image* img = &imgIn;
    Image flat;
    if ((fmt == ImageFormat::JPEG || fmt == ImageFormat::BMP) && imgIn.HasTransparency()) {
        flat = Image(imgIn.w, imgIn.h, 0xFFFFFFFF);
        flat.Blit(imgIn, 0, 0, true);
        img = &flat;
    }
    ComPtr<IWICBitmapEncoder> enc;
    if (FAILED(f->CreateEncoder(container, nullptr, &enc))) return false;
    if (FAILED(enc->Initialize(stream, WICBitmapEncoderNoCache))) return false;
    ComPtr<IWICBitmapFrameEncode> frame;
    ComPtr<IPropertyBag2> props;
    if (FAILED(enc->CreateNewFrame(&frame, &props))) return false;
    if (fmt == ImageFormat::JPEG && props) {
        PROPBAG2 opt{}; opt.pstrName = (LPOLESTR)L"ImageQuality";
        VARIANT v; VariantInit(&v); v.vt = VT_R4; v.fltVal = std::clamp(quality, 1, 100) / 100.0f;
        props->Write(1, &opt, &v);
    }
    if (FAILED(frame->Initialize(props.Get()))) return false;
    frame->SetSize(img->w, img->h);
    if (imgIn.dpi > 0) frame->SetResolution(imgIn.dpi, imgIn.dpi);
    WICPixelFormatGUID pf = GUID_WICPixelFormat32bppBGRA;
    frame->SetPixelFormat(&pf);
    HRESULT hr;
    if (pf == GUID_WICPixelFormat32bppBGRA) {
        hr = frame->WritePixels(img->h, img->w * 4, (UINT)img->px.size() * 4, (BYTE*)img->px.data());
    } else {
        // o codificador pediu outro formato (JPEG: 24bpp; GIF: indexado) → conversor WIC
        ComPtr<IWICBitmap> src;
        f->CreateBitmapFromMemory(img->w, img->h, GUID_WICPixelFormat32bppBGRA, img->w * 4,
                                  (UINT)img->px.size() * 4, (BYTE*)img->px.data(), &src);
        ComPtr<IWICFormatConverter> conv;
        f->CreateFormatConverter(&conv);
        ComPtr<IWICPalette> pal;
        if (fmt == ImageFormat::GIF) {
            f->CreatePalette(&pal);
            pal->InitializeFromBitmap(src.Get(), 256, img->HasTransparency());
        }
        hr = conv->Initialize(src.Get(), pf, pal ? WICBitmapDitherTypeErrorDiffusion : WICBitmapDitherTypeNone,
                              pal.Get(), 0, pal ? WICBitmapPaletteTypeCustom : WICBitmapPaletteTypeMedianCut);
        if (SUCCEEDED(hr)) {
            if (pal) frame->SetPalette(pal.Get());
            hr = frame->WriteSource(conv.Get(), nullptr);
        }
    }
    if (FAILED(hr)) return false;
    if (FAILED(frame->Commit())) return false;
    return SUCCEEDED(enc->Commit());
}

bool SaveImageFile(const std::wstring& path, const Image& img, int quality) {
    ImageFormat fmt = FormatFromExt(FileExt(path));
    if (fmt == ImageFormat::Unknown) fmt = ImageFormat::PNG;
    ComPtr<IStream> st;
    std::wstring tmp = path + L".tmp";
    if (FAILED(SHCreateStreamOnFileEx(tmp.c_str(), STGM_CREATE | STGM_WRITE | STGM_SHARE_EXCLUSIVE,
                                      FILE_ATTRIBUTE_NORMAL, TRUE, nullptr, &st))) return false;
    bool ok = EncodeImage(st.Get(), img, fmt, quality);
    st.Reset();
    if (!ok) { DeleteFileW(tmp.c_str()); return false; }
    return MoveFileExW(tmp.c_str(), path.c_str(), MOVEFILE_REPLACE_EXISTING) != 0;
}

bool EncodeToMemory(const Image& img, ImageFormat fmt, std::vector<BYTE>& out, int quality) {
    ComPtr<IStream> st;
    if (FAILED(CreateStreamOnHGlobal(nullptr, TRUE, &st))) return false;
    if (!EncodeImage(st.Get(), img, fmt, quality)) return false;
    STATSTG ss{}; st->Stat(&ss, STATFLAG_NONAME);
    out.resize((size_t)ss.cbSize.QuadPart);
    LARGE_INTEGER z{}; st->Seek(z, STREAM_SEEK_SET, nullptr);
    ULONG rd = 0; st->Read(out.data(), (ULONG)out.size(), &rd);
    return rd == out.size();
}

bool DecodeFromMemory(const BYTE* data, size_t len, Image& out) {
    ComPtr<IStream> st;
    st.Attach(SHCreateMemStream(data, (UINT)len));
    return st && DecodeImage(st.Get(), out);
}

std::wstring ImageOpenFilter() {
    std::wstring f = L"Imagens e documentos (*.snap;*.png;*.jpg;*.jpeg;*.bmp;*.gif;*.tif;*.tiff;*.ico;*.mp4)|*.snap;*.png;*.jpg;*.jpeg;*.bmp;*.gif;*.tif;*.tiff;*.ico;*.mp4|"
                     L"Documento Snapnab (*.snap)|*.snap|Vídeo MP4 (*.mp4)|*.mp4|Todos os arquivos (*.*)|*.*|";
    std::replace(f.begin(), f.end(), L'|', L'\0');
    return f;
}

std::wstring ImageSaveFilter() {
    std::wstring f = L"PNG (*.png)|*.png|JPEG (*.jpg)|*.jpg|BMP (*.bmp)|*.bmp|GIF (*.gif)|*.gif|TIFF (*.tif)|*.tif|"
                     L"PDF (*.pdf)|*.pdf|Documento Snapnab (*.snap)|*.snap|";
    std::replace(f.begin(), f.end(), L'|', L'\0');
    return f;
}

// ---------------------------------------------------------------- área de transferência
static UINT PngFormat() { static UINT f = RegisterClipboardFormatW(L"PNG"); return f; }

bool CopyImageToClipboard(HWND owner, const Image& img) {
    if (img.empty()) return false;
    std::vector<BYTE> png;
    EncodeToMemory(img, ImageFormat::PNG, png);
    // CF_DIB: 32 bpp de baixo para cima, composto sobre branco (apps antigos ignoram o alfa)
    size_t sz = sizeof(BITMAPINFOHEADER) + img.px.size() * 4;
    HGLOBAL hDib = GlobalAlloc(GMEM_MOVEABLE, sz);
    if (!hDib) return false;
    auto* bih = (BITMAPINFOHEADER*)GlobalLock(hDib);
    *bih = {};
    bih->biSize = sizeof(BITMAPINFOHEADER); bih->biWidth = img.w; bih->biHeight = img.h;
    bih->biPlanes = 1; bih->biBitCount = 32; bih->biCompression = BI_RGB;
    bih->biSizeImage = (DWORD)(img.px.size() * 4);
    auto* dst = (uint32_t*)(bih + 1);
    for (int y = 0; y < img.h; y++) {
        const uint32_t* s = img.row(img.h - 1 - y);
        for (int x = 0; x < img.w; x++) dst[(size_t)y * img.w + x] = BlendOver(0xFFFFFFFF, s[x]);
    }
    GlobalUnlock(hDib);
    if (!OpenClipboard(owner)) { GlobalFree(hDib); return false; }
    EmptyClipboard();
    SetClipboardData(CF_DIB, hDib);
    if (!png.empty()) {
        HGLOBAL hPng = GlobalAlloc(GMEM_MOVEABLE, png.size());
        if (hPng) {
            memcpy(GlobalLock(hPng), png.data(), png.size());
            GlobalUnlock(hPng);
            SetClipboardData(PngFormat(), hPng);
        }
    }
    CloseClipboard();
    return true;
}

bool ClipboardHasImage() {
    return IsClipboardFormatAvailable(CF_DIB) || IsClipboardFormatAvailable(CF_DIBV5) ||
           IsClipboardFormatAvailable(PngFormat()) || IsClipboardFormatAvailable(CF_BITMAP);
}

bool GetClipboardImage(Image& out) {
    bool ok = false;
    for (int tries = 0; tries < 10 && !OpenClipboard(nullptr); tries++) Sleep(50);
    if (HANDLE h = GetClipboardData(PngFormat())) {
        void* p = GlobalLock(h);
        if (p) { ok = DecodeFromMemory((BYTE*)p, GlobalSize(h), out); GlobalUnlock(h); }
    }
    if (!ok) {
        HANDLE h = GetClipboardData(CF_DIBV5);
        if (!h) h = GetClipboardData(CF_DIB);
        if (h) {
            auto* bih = (BITMAPINFOHEADER*)GlobalLock(h);
            if (bih && (bih->biBitCount == 32 || bih->biBitCount == 24)) {
                int w = bih->biWidth, hh = std::abs(bih->biHeight);
                bool bottomUp = bih->biHeight > 0;
                BYTE* bits = (BYTE*)bih + bih->biSize;
                if (bih->biCompression == BI_BITFIELDS && bih->biSize == sizeof(BITMAPINFOHEADER)) bits += 12;
                int stride = ((w * bih->biBitCount + 31) / 32) * 4;
                out = Image(w, hh);
                bool anyAlpha = false;
                for (int y = 0; y < hh; y++) {
                    BYTE* r = bits + (size_t)(bottomUp ? hh - 1 - y : y) * stride;
                    for (int x = 0; x < w; x++) {
                        if (bih->biBitCount == 32) {
                            uint32_t v = ((uint32_t*)r)[x];
                            if (CA(v)) anyAlpha = true;
                            out.at(x, y) = v;
                        } else out.at(x, y) = ARGB(255, r[x * 3 + 2], r[x * 3 + 1], r[x * 3]);
                    }
                }
                if (!anyAlpha) out.MakeOpaque();
                ok = true;
            }
            if (bih) GlobalUnlock(h);
        }
    }
    if (!ok) {
        if (HBITMAP hbm = (HBITMAP)GetClipboardData(CF_BITMAP)) { out = Image::FromHBITMAP(hbm); ok = !out.empty(); }
    }
    CloseClipboard();
    return ok;
}

// ---------------------------------------------------------------- escala
Image ScaleImage(const Image& src, int nw, int nh, bool hq) {
    if (src.empty() || nw <= 0 || nh <= 0) return {};
    Image dst(nw, nh, 0);
    Image& s = const_cast<Image&>(src);
    auto sb = s.WrapGdip();
    auto db = dst.WrapGdip();
    {
        Gdiplus::Graphics g(db.get());
        g.SetInterpolationMode(hq ? Gdiplus::InterpolationModeHighQualityBicubic : Gdiplus::InterpolationModeNearestNeighbor);
        g.SetPixelOffsetMode(Gdiplus::PixelOffsetModeHalf);
        g.SetCompositingMode(Gdiplus::CompositingModeSourceCopy);
        Gdiplus::ImageAttributes ia;
        ia.SetWrapMode(Gdiplus::WrapModeTileFlipXY);
        g.DrawImage(sb.get(), Gdiplus::Rect(0, 0, nw, nh), 0, 0, src.w, src.h, Gdiplus::UnitPixel, &ia);
    }
    return dst;
}

Image MakeThumbnail(const Image& img, int maxW, int maxH) {
    if (img.empty()) return {};
    double s = std::min(1.0, std::min((double)maxW / img.w, (double)maxH / img.h));
    return ScaleImage(img, std::max(1, (int)(img.w * s)), std::max(1, (int)(img.h * s)));
}

} // namespace snap
