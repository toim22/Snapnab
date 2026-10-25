// Cores das janelas do Snapnab (captura e Editor), derivadas da cor de destaque do Windows (Personalização > Cores):
// fundos neutros puxados de leve para o destaque, faixa de título e cor dos controles.
#pragma once
#include "Common.h"

namespace snap::ui {

struct ThemeColors {
    COLORREF accent = RGB(0, 120, 215), accentDark = RGB(0, 90, 158), accentText = RGB(0, 84, 150);
    COLORREF control = RGB(0, 120, 215);       // trilho das chaves, indicadores (destaque escurecido se for claro demais)
    COLORREF bgMain = RGB(250, 250, 252), bgSide = RGB(240, 241, 244), bgBar = RGB(245, 246, 248);
    COLORREF bgCaption = RGB(243, 243, 245), captionText = RGB(28, 28, 32), line = RGB(222, 224, 228);
    bool captionAccent = false;                // "Mostrar a cor de destaque nas barras de título" ligado
};

constexpr COLORREF kCaptureRed = RGB(226, 56, 44);    // botão redondo de captura/gravação

ThemeColors LoadThemeColors();
COLORREF Mix(COLORREF a, COLORREF b, double t);
double Luma(COLORREF c);
void ApplyThemeToWidgets(const ThemeColors& t);       // paleta dos controles modernos (Widgets)

} // namespace snap::ui
