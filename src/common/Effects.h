// Effects and filters (IImageFilters / CEffectSettings* / CPaintOpRaster, comp_4 + ds_arch §17.B)
#pragma once
#include "Image.h"

namespace snap::fx {

// geometry
Image Rotate90(const Image& s, bool clockwise);
Image Rotate180(const Image& s);
Image FlipH(const Image& s);
Image FlipV(const Image& s);
Image RotateAngle(const Image& s, double degrees, uint32_t background);
Image CanvasResize(const Image& s, int nw, int nh, int anchor /*0..8, 4=center*/, uint32_t background);
Image AutoTrim(const Image& s, int tolerance = 8);
Image Scale(const Image& s, int percent);

// color
void Grayscale(Image& img);
void Invert(Image& img);
void Sepia(Image& img);
void Monochrome(Image& img, int threshold = 128);
void BrightnessContrast(Image& img, int brightness /*-100..100*/, int contrast /*-100..100*/);
void HueSaturation(Image& img, int hueShift /*degrees*/, int saturation /*-100..100*/);
void Gamma(Image& img, double gamma);
void ReplaceColor(Image& img, uint32_t from, uint32_t to, int tolerance);

// hide
void BoxBlur(Image& img, RECT r, int radius);
void Pixelate(Image& img, RECT r, int block);

// border and finish
Image Border(const Image& s, int width, uint32_t color);
Image Shadow(const Image& s, int offset, int blur, int opacityPercent);
enum class Edge { Torn = 0, Wave, Sawtooth, SharkTooth, Fade, Beveled };    // EdgeEffectPreset* presets (comp_4 §6)
enum Sides { Top = 1, Right = 2, Bottom = 4, Left = 8, All = 15 };
Image EdgeEffect(const Image& s, Edge style, int size, int sides, bool shadow);

// effects only the Editor has (CEffectSettingsPerspectiveShear / PageCurl / SpotlightMagnify, ed_comp_5 §4.3)
Image Perspective(const Image& s, int horizontal /*-100..100*/, int vertical /*-100..100*/, int shear /*-100..100*/);
Image PageCurl(const Image& s, int size, int corner /*0 top left, 1 top right, 2 bottom right, 3 bottom left*/);
Image Spotlight(const Image& s, RECT focus, int dimPercent, int blur, int zoomPercent, bool shadow);
// filters (CEffectSettingsFilter: Emboss, Mosaic, Sharpen, AverageFilter…)
void Emboss(Image& img, int depth /*1..10*/);
void Sharpen(Image& img, int amount /*0..100*/);

// text
void Watermark(Image& img, const std::wstring& text, int opacityPercent, bool tiled, bool emboss);
Image Caption(const Image& s, const std::wstring& text, bool top, int fontPx, uint32_t fg, uint32_t bg);
std::wstring ExpandCaptionTokens(const std::wstring& text);   // {computer} {user} {date} {time}

// Preset capture effects (CaptureEffects, comp_7 §2.1)
// These are the effects of the capture window's "Effects" menu; each has an "on" switch and its own settings.
struct CaptureEffects {
    bool border = false;  int borderWidth = 4;  COLORREF borderColor = RGB(40, 40, 40);
    bool filter = false;  int filterKind = 0;                 // 0 grayscale, 1 sepia, 2 invert, 3 black and white
    bool shadow = false;  int shadowOffset = 6, shadowBlur = 6, shadowOpacity = 50;
    bool caption = false; std::wstring captionText = L"{date} {time} — {computer}"; bool captionTop = false;
    bool replaceColor = false; COLORREF replaceFrom = RGB(255, 255, 255), replaceTo = RGB(255, 255, 0); int replaceTolerance = 10;
    bool scale = false;   int scalePercent = 100;
    bool edge = false;    int edgeStyle = 0, edgeSize = 14, edgeSides = 6 /*fx::Sides*/;
    bool trim = false;    int trimTolerance = 8;
    bool adjust = false;  int brightness = 0, contrast = 0, hue = 0, saturation = 0;
    bool rotate = false;  int rotateKind = 0;                 // 0 90° clockwise, 1 90° counterclockwise, 2 180°, 3 flip ↔, 4 flip ↕
    bool watermark = false; std::wstring watermarkText = L"Snapnab"; int watermarkOpacity = 35; bool watermarkTiled = true;
    bool resolution = false; int dpi = 300;                   // written to the file metadata
    bool Any() const {
        return border || filter || shadow || caption || replaceColor || scale || edge || trim || adjust || rotate || watermark || resolution;
    }
};
Image ApplyCaptureEffects(const Image& s, const CaptureEffects& e);

} // namespace snap::fx
