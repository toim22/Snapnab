// Custom title bar for Snapnab windows: icon + menus on the same strip, optional title and the
// minimize/maximize/close buttons. The window drops the system frame (WM_NCCALCSIZE → 0) and forwards its messages here.
// Menus open with a click, Alt+letter or F10, and switch to the neighbor with the mouse or the arrow keys.
#pragma once
#include "Theme.h"

namespace snap::ui {

class CaptionBar {
public:
    struct Menu { std::wstring name; wchar_t key; HMENU menu; };   // key = Alt letter (uppercase)

    void Init(HWND frame, std::vector<Menu> menus, bool resizable, bool editorIcon);
    void SetTheme(const ThemeColors& t) { th_ = t; Invalidate(); }
    void SetTitle(const std::wstring& t) { title_ = t; Invalidate(); }
    void Layout(int width, int dpi);               // recomputes the rectangles (call on WM_SIZE / WM_DPICHANGED)
    int Height() const { return h_; }
    void Paint(HDC dc);                            // paints the whole strip (0, 0, width, Height())
    std::function<void(HMENU, int index)> onInitMenu;   // before a menu opens (check/disable items)

    // window messages: return true if the bar handled them
    bool NcCalcSize(WPARAM w, LPARAM l, LRESULT& result);
    bool NcHitTest(LPARAM l, LRESULT& result);
    bool MouseMove(POINT client);
    void MouseLeave();
    bool LButtonDown(POINT client);
    bool LButtonUp(POINT client);
    bool RButtonUp(WPARAM w, LPARAM l);            // WM_NCRBUTTONUP: system menu
    bool PreTranslate(const MSG& msg);             // Alt+letter / F10 in the message loop
    void OpenMenu(int index);

private:
    static LRESULT CALLBACK MenuFilter(int code, WPARAM w, LPARAM l);
    int Hit(POINT p) const;                        // 0..n-1 menus, 100 min, 101 max, 102 close, -1 none
    void Invalidate();

    HWND frame_ = nullptr;
    std::vector<Menu> menus_;
    std::vector<RECT> menuRects_;
    RECT btn_[3]{};                                // minimize, maximize, close
    bool resizable_ = false, tracking_ = false, editorIcon_ = false;
    int h_ = 32, dpi_ = 96, width_ = 0, titleX_ = 0;
    int hot_ = -1, pressed_ = -1, open_ = -1, next_ = -1;
    ThemeColors th_;
    std::wstring title_;
    HICON icon_ = nullptr;
    int iconDpi_ = 0;
    static HHOOK hook_;
    static CaptionBar* active_;
};

} // namespace snap::ui
