#include "Theme.h"
#include "Widgets.h"
#include <dwmapi.h>

namespace snap::ui {

COLORREF Mix(COLORREF a, COLORREF b, double t) {
    auto ch = [t](int x, int y) { return (BYTE)std::lround(x + (y - x) * t); };
    return RGB(ch(GetRValue(a), GetRValue(b)), ch(GetGValue(a), GetGValue(b)), ch(GetBValue(a), GetBValue(b)));
}

double Luma(COLORREF c) { return (0.299 * GetRValue(c) + 0.587 * GetGValue(c) + 0.114 * GetBValue(c)) / 255.0; }

static bool ReadHkcuDword(const wchar_t* key, const wchar_t* name, DWORD& v) {
    DWORD cb = sizeof v;
    return RegGetValueW(HKEY_CURRENT_USER, key, name, RRF_RT_REG_DWORD, nullptr, &v, &cb) == ERROR_SUCCESS;
}

ThemeColors LoadThemeColors() {
    ThemeColors t;
    // AccentPalette = 8 cores RGBA (Light3, Light2, Light1, Accent, Dark1, Dark2, Dark3, …)
    BYTE pal[32]; DWORD cb = sizeof pal;
    DWORD v;
    if (RegGetValueW(HKEY_CURRENT_USER, L"Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Accent", L"AccentPalette",
                     RRF_RT_REG_BINARY, nullptr, pal, &cb) == ERROR_SUCCESS && cb >= 24) {
        t.accent = RGB(pal[12], pal[13], pal[14]);
        t.accentDark = RGB(pal[16], pal[17], pal[18]);
        t.accentText = RGB(pal[20], pal[21], pal[22]);
    } else if (ReadHkcuDword(L"Software\\Microsoft\\Windows\\DWM", L"AccentColor", v)) {   // 0xAABBGGRR
        t.accent = v & 0xFFFFFF;
        t.accentDark = Mix(t.accent, 0, 0.25);
        t.accentText = Mix(t.accent, 0, 0.4);
    } else {
        BOOL opaque; DWORD argb;
        if (SUCCEEDED(DwmGetColorizationColor(&argb, &opaque))) t.accent = RGB((argb >> 16) & 255, (argb >> 8) & 255, argb & 255);
        t.accentDark = Mix(t.accent, 0, 0.25);
        t.accentText = Mix(t.accent, 0, 0.4);
    }
    while (Luma(t.accentText) > 0.42) t.accentText = Mix(t.accentText, 0, 0.15);   // legível sobre fundo claro
    t.control = Luma(t.accent) > 0.55 ? t.accentDark : t.accent;
    // fundos neutros puxados de leve para a cor de destaque
    t.bgMain = Mix(RGB(251, 251, 252), t.accent, 0.05);
    t.bgBar = Mix(RGB(246, 246, 248), t.accent, 0.09);
    t.bgSide = Mix(RGB(238, 239, 241), t.accent, 0.13);
    t.line = Mix(RGB(220, 221, 225), t.accent, 0.18);
    // barra de título: na cor de destaque se o usuário pediu isso nas barras de título do Windows
    DWORD prevalence = 0;
    ReadHkcuDword(L"Software\\Microsoft\\Windows\\DWM", L"ColorPrevalence", prevalence);
    t.captionAccent = prevalence != 0;
    t.bgCaption = t.captionAccent ? t.control : Mix(RGB(243, 243, 245), t.accent, 0.11);
    t.captionText = t.captionAccent ? RGB(255, 255, 255) : RGB(28, 28, 32);
    return t;
}

void ApplyThemeToWidgets(const ThemeColors& t) {
    mui::Palette p;
    p.accent = t.control;
    p.accentText = t.accentText;
    p.line = t.line;
    mui::SetPalette(p);
}

} // namespace snap::ui
