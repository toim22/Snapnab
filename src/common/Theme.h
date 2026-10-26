// Colors of the Snapnab windows (capture and Editor), derived from the Windows accent color (Personalization > Colors):
// neutral backgrounds slightly tinted toward the accent, title strip and control color.
#pragma once
#include "Common.h"

namespace snap::ui {

struct ThemeColors {
    COLORREF accent = RGB(0, 120, 215), accentDark = RGB(0, 90, 158), accentText = RGB(0, 84, 150);
    COLORREF control = RGB(0, 120, 215);       // switch track, indicators (accent darkened if it is too light)
    COLORREF bgMain = RGB(250, 250, 252), bgSide = RGB(240, 241, 244), bgBar = RGB(245, 246, 248);
    COLORREF bgCaption = RGB(243, 243, 245), captionText = RGB(28, 28, 32), line = RGB(222, 224, 228);
    bool captionAccent = false;                // "Show accent color on title bars" turned on
};

constexpr COLORREF kCaptureRed = RGB(226, 56, 44);    // round capture/record button

ThemeColors LoadThemeColors();
COLORREF Mix(COLORREF a, COLORREF b, double t);
double Luma(COLORREF c);
void ApplyThemeToWidgets(const ThemeColors& t);       // palette of the modern controls (Widgets)

} // namespace snap::ui
