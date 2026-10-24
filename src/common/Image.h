// Imagem em memória (BGRA 32 bpp, alfa direto) + E/S de formatos (papel do LEADTOOLS, ds_arch §13.3 → WIC)
#pragma once
#include "Common.h"

namespace snap {

struct Image {
    int w = 0, h = 0;
    std::vector<uint32_t> px;           // 0xAARRGGBB por pixel (BGRA na memória), linha a linha de cima para baixo
    float dpi = 0;                      // resolução gravada no arquivo (0 = padrão do codificador)

    Image() = default;
    Image(int width, int height, uint32_t fill = 0) : w(width), h(height), px((size_t)width * height, fill) {}

    bool empty() const { return w <= 0 || h <= 0; }
    uint32_t* row(int y) { return px.data() + (size_t)y * w; }
    const uint32_t* row(int y) const { return px.data() + (size_t)y * w; }
    uint32_t& at(int x, int y) { return px[(size_t)y * w + x]; }
    uint32_t at(int x, int y) const { return px[(size_t)y * w + x]; }

    Image Crop(RECT r) const;
    void Blit(const Image& src, int dx, int dy, bool blend = true);
    void FillRect(RECT r, uint32_t color);
    bool HasTransparency() const;
    void MakeOpaque();

    // pontes GDI / GDI+
    HBITMAP ToHBITMAP() const;                                     // DIB section 32 bpp top-down
    static Image FromHBITMAP(HBITMAP hbm);
    static Image FromDC(HDC hdc, RECT r);                          // BitBlt de uma área do DC
    std::unique_ptr<Gdiplus::Bitmap> WrapGdip();                   // compartilha px (sem cópia)
    static Image FromGdip(Gdiplus::Bitmap& bmp);
    void DrawTo(HDC hdc, int x, int y, int dw, int dh, bool smooth) const;  // StretchDIBits
};

inline uint32_t ARGB(int a, int r, int g, int b) { return ((uint32_t)a << 24) | ((uint32_t)r << 16) | ((uint32_t)g << 8) | (uint32_t)b; }
inline uint32_t FromColorref(COLORREF c, int a = 255) { return ARGB(a, GetRValue(c), GetGValue(c), GetBValue(c)); }
inline COLORREF ToColorref(uint32_t p) { return RGB((p >> 16) & 255, (p >> 8) & 255, p & 255); }
inline int CA(uint32_t p) { return (p >> 24) & 255; }
inline int CR(uint32_t p) { return (p >> 16) & 255; }
inline int CG(uint32_t p) { return (p >> 8) & 255; }
inline int CB(uint32_t p) { return p & 255; }
uint32_t BlendOver(uint32_t dst, uint32_t src);

// ---- Formatos (E1/K5) ----
enum class ImageFormat { PNG, JPEG, BMP, GIF, TIFF, Unknown };
ImageFormat FormatFromExt(const std::wstring& ext);
bool LoadImageFile(const std::wstring& path, Image& out);
bool SaveImageFile(const std::wstring& path, const Image& img, int jpegQuality = 90);
bool EncodeImage(IStream* stream, const Image& img, ImageFormat fmt, int jpegQuality = 90);
bool DecodeImage(IStream* stream, Image& out);
bool EncodeToMemory(const Image& img, ImageFormat fmt, std::vector<BYTE>& out, int jpegQuality = 90);
bool DecodeFromMemory(const BYTE* data, size_t len, Image& out);
std::wstring ImageOpenFilter();        // filtro para GetOpenFileName
std::wstring ImageSaveFilter();

// ---- Área de transferência (E2) ----
bool CopyImageToClipboard(HWND owner, const Image& img);
bool ClipboardHasImage();
bool GetClipboardImage(Image& out);

// ---- Miniatura ----
Image MakeThumbnail(const Image& img, int maxW, int maxH);
Image ScaleImage(const Image& img, int nw, int nh, bool highQuality = true);

} // namespace snap
