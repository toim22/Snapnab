// Shared Win32 UI utilities (modal forms built in code, fonts, drawing)
#pragma once
#include "Common.h"

namespace snap::ui {

HFONT UiFont(int dpi = 96, int pt = 9, bool bold = false);
HFONT IconFont(int dpi, int px);            // Segoe Fluent Icons / Segoe MDL2 Assets
int   Scale(int v, int dpi);

struct Field {
    enum Type { Label, Text, Int, Check, Combo, Color, Hotkey, Separator } type = Text;
    std::wstring label;
    std::wstring text;                       // Text
    int value = 0;                           // Int / Check / Combo (index)
    int minV = 0, maxV = 100000;
    std::vector<std::wstring> options;       // Combo
    COLORREF color = 0;                      // Color
    UINT hkMods = 0, hkVk = 0;               // Hotkey (MOD_*)
    HWND hwnd = nullptr;                     // filled in at run time

    static Field MakeLabel(const std::wstring& l) { Field f; f.type = Label; f.label = l; return f; }
    static Field MakeText(const std::wstring& l, const std::wstring& t) { Field f; f.type = Text; f.label = l; f.text = t; return f; }
    static Field MakeInt(const std::wstring& l, int v, int mn, int mx) { Field f; f.type = Int; f.label = l; f.value = v; f.minV = mn; f.maxV = mx; return f; }
    static Field MakeCheck(const std::wstring& l, bool v) { Field f; f.type = Check; f.label = l; f.value = v; return f; }
    static Field MakeCombo(const std::wstring& l, std::vector<std::wstring> o, int sel) { Field f; f.type = Combo; f.label = l; f.options = std::move(o); f.value = sel; return f; }
    static Field MakeColor(const std::wstring& l, COLORREF c) { Field f; f.type = Color; f.label = l; f.color = c; return f; }
    static Field MakeHotkey(const std::wstring& l, UINT mods, UINT vk) { Field f; f.type = Hotkey; f.label = l; f.hkMods = mods; f.hkVk = vk; return f; }
    static Field MakeSeparator(const std::wstring& l) { Field f; f.type = Separator; f.label = l; return f; }
};

// Modal form; returns true on OK. 'validate' can reject (by returning an error message).
bool RunForm(HWND owner, const std::wstring& title, std::vector<Field>& fields,
             std::function<std::wstring(std::vector<Field>&)> validate = nullptr, int width = 420);

// Shows long copyable text (OCR result, help)
void ShowTextWindow(HWND owner, const std::wstring& title, const std::wstring& text);

// File dialogs
bool OpenFileDialog(HWND owner, const std::wstring& filter, std::wstring& path, const std::wstring& title = L"");
bool SaveFileDialog(HWND owner, const std::wstring& filter, std::wstring& path, int& filterIndex,
                    const std::wstring& defExt, const std::wstring& title = L"");

void FillRoundRect(HDC dc, RECT r, int radius, COLORREF fill, COLORREF border);
void DrawTextCentered(HDC dc, const std::wstring& t, RECT r, COLORREF color, HFONT font);

} // namespace snap::ui
