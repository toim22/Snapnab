// Input selection (CCapturePreparer, comp_1 §4–§6): crosshair over the frozen screen
#pragma once
#include "Image.h"

namespace snap {

enum class SelectMode { Region, Window, Object, Fixed, Freehand, MultiArea, PickWindow };

struct SelectResult {
    bool ok = false;
    RECT rect{};                    // screen coordinates
    HWND window = nullptr;          // top-level window under the click
    HWND child = nullptr;           // innermost control under the click
    std::vector<POINT> path;        // freehand (screen)
    std::vector<RECT> rects;        // multiple areas (screen)
};

// Opens the selection window; 'done' is called once, on the UI thread.
void RunSelectOverlay(const Image& frozen, RECT virtualScreen, SelectMode mode, SIZE fixedSize, bool video,
                      std::function<void(const SelectResult&)> done);

// Window utilities used by the selection and the capture
RECT WindowFrameRect(HWND h);        // DWMWA_EXTENDED_FRAME_BOUNDS (without the invisible shadow)
std::wstring WindowTitle(HWND h);
std::wstring WindowProcessName(HWND h);

} // namespace snap
