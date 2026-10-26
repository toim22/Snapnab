// Modern-looking controls (Windows 11 / material) for the capture window and the Editor: toggle switch,
// button (normal, flat or icon-only) and combo box with its own drop-down list.
// They speak the standard controls' messages — BM_GETCHECK/BM_SETCHECK, CB_ADDSTRING/CB_SETCURSEL/CB_GETCURSEL/
// CB_SETITEMDATA…, and notify the parent with WM_COMMAND (BN_CLICKED, CBN_DROPDOWN, CBN_SELCHANGE, CBN_CLOSEUP).
// Each control's background comes from the parent through WM_CTLCOLORBTN.
#pragma once
#include "Common.h"
#include "Icons.h"

namespace snap::mui {

inline constexpr wchar_t kToggle[] = L"SnapnabToggle";
inline constexpr wchar_t kButton[] = L"SnapnabButton";
inline constexpr wchar_t kCombo[] = L"SnapnabCombo";

struct Palette {
    COLORREF accent = RGB(0, 120, 215);       // track of a switch that is on, indicator of the selected item
    COLORREF accentText = RGB(0, 84, 150);    // check marks, flat button icons, focus ring
    COLORREF line = RGB(222, 224, 228);       // outlines
};

void Register(HINSTANCE hi);
void SetPalette(const Palette& p);

enum : UINT {
    MUI_SETICON = WM_APP + 300,   // wParam = icons::Id + 1 (0 = no icon)
    MUI_SETSTYLE,                 // wParam = combination of Style
    MUI_SETITEMFLAGS,             // combo: wParam = index, lParam = ItemFlags
    MUI_SETDISPLAYTEXT,           // combo: lParam = text shown in place of the selected item (nullptr = back to the item)
                                  // ButtonLink button: lParam = value shown on the right (e.g. "Off")
    MUI_SETGEAR,                  // ComboGear combo: wParam = gear enabled
};
enum Style {
    ButtonFlat = 1, ButtonIconOnly = 2,
    ComboMenu = 4,                // action list: the arrow keys do not change the selection
    ComboGear = 8,                // settings gear inside the box, to the right of the arrow (notifies with MUI_GEARCLICK)
    ButtonLink = 16,              // "Label        Value ›" row that opens a menu (notifies with BN_CLICKED)
    ButtonMenu = 32,              // small arrow after the text (button that opens a menu)
    ButtonAccent = 64,            // text in the accent color
};
enum ItemFlags { ItemSeparator = 1, ItemChecked = 2 };
constexpr WORD MUI_GEARCLICK = 0x0400;   // notification code (HIWORD of WM_COMMAND) of a click on the gear

} // namespace snap::mui
