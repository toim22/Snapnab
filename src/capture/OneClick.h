// One Click (OneClickWindow / OneClickViewModel, comp_7 §3): floating panel docked to an edge of the screen.
// With the mouse over it, it shows the capture button and the gear; otherwise it collapses to a thin strip
// on the edge. It can be dragged freely and snaps to the nearest edge when dropped.
#pragma once
#include "Common.h"

namespace snap {

struct OneClickTheme {
    COLORREF accent = RGB(0, 120, 215);
    COLORREF bg = RGB(250, 250, 252);
    COLORREF line = RGB(222, 224, 228);
    COLORREF capture = RGB(226, 56, 44);
};

class OneClick {
public:
    std::function<void()> onCapture;
    std::function<void(POINT)> onMenu;        // gear: screen point where the menu opens
    void Show(bool on);
    bool enabled() const { return enabled_; }
    void SetTheme(const OneClickTheme& t);
    void SetBusy(bool busy);                  // hides during a capture (DisableOneClickUI)
    void SetTip(const std::wstring& tip) { tip_ = tip; }
private:
    static LRESULT CALLBACK Proc(HWND, UINT, WPARAM, LPARAM);
    void Create();
    void Render();                            // paints and positions (UpdateLayeredWindow)
    void Snap(POINT cursor);                  // snaps to the nearest edge after dragging
    void Tick();
    void ShowTip(bool on);
    int Hit(POINT screen) const;              // 1 = capture, 2 = gear, 0 = background, -1 = outside
    SIZE PanelSize(int dpi) const;
    RECT MonitorRect() const;
    int Dpi() const;

    HWND hwnd_ = nullptr, tipWnd_ = nullptr;
    OneClickTheme th_;
    std::wstring tip_ = TR_NOOP(L"Iniciar uma captura");
    bool enabled_ = false, busy_ = false;
    int edge_ = 0;                            // 0 top, 1 right, 2 bottom, 3 left
    double pos_ = 0.82;                       // position along the edge (0..1)
    POINT monPt_{ 0, 0 };                     // a point on the chosen monitor
    double open_ = 0;                         // 0 collapsed … 1 open (animated)
    bool wantOpen_ = false, dragging_ = false, down_ = false, menu_ = false, tipOn_ = false;
    int hot_ = 0, pressed_ = 0;
    POINT dragStart_{}, dragWin_{};
    RECT win_{};                              // current window rectangle
    POINT panelAt_{};                         // corner of the panel inside the window
    ULONGLONG leftAt_ = 0;
};

} // namespace snap
