// Drop-down lists of the properties panel: grid gallery (shapes, callouts, ends, dash patterns) and the color
// palette (side panel ColorPalette). They open below the control and close on a click outside or with Esc.
#pragma once
#include "Common.h"
#include "Icons.h"
#include <array>

namespace snap::ed {

struct Gradient;

using GridDraw = std::function<void(Gdiplus::Graphics& g, HDC dc, int index, RECT cell, bool hot, bool selected)>;

// returns the chosen index or -1
int GridPopup(HWND owner, POINT anchorScreen, int cols, int count, SIZE cell, int selected, GridDraw draw);

// list with icon, name and shortcut (toolbar "More" button): material, with the current tool highlighted
struct PopupItem {
    int id = 0;
    icons::Id icon = icons::Gear;
    bool hasIcon = true;
    std::wstring label, shortcut;
    bool enabled = true, checked = false, separator = false;
};
int ListPopup(HWND owner, POINT anchorScreen, const std::vector<PopupItem>& items, int minWidth = 0);

// palette: theme colors, standard colors, recent ones, "No color" (allowNone) and "More colors…" (Windows picker);
// with 'grad', also gradient presets and "Gradient…" (a solid color turns the gradient off)
bool ColorPopup(HWND owner, POINT anchorScreen, uint32_t& color, bool allowNone, const std::array<uint32_t, 4>& themeColors,
                Gradient* grad = nullptr);
// gradient editor: type, angle and two to four color stops
bool EditGradient(HWND owner, Gradient& g, uint32_t baseColor);

} // namespace snap::ed
