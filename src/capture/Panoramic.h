// Panoramic scrolling capture (PanoramicCapture, comp_3/comp_1): after choosing the region, the user scrolls the
// content (page, spreadsheet, chat…) any way they like; the region is copied several times per second and each
// new frame is fitted onto the previous one by the detected offset. The result is a single, long image.
#pragma once
#include "Image.h"

namespace snap {

// Stitches successive frames of the same scrolled region (vertical or horizontal). Also used by the tests.
class PanoramaStitcher {
public:
    bool Add(const Image& frame);        // true if the frame added content
    const Image& result() const { return canvas_; }
    int axis() const { return axis_; }   // 0 = still idle, 1 = vertical, 2 = horizontal
    static constexpr int kMaxSize = 32000;
private:
    bool Offset(const Image& a, const Image& b, bool vertical, int& d) const;
    Image canvas_, prev_;
    int axis_ = 0;
    int posX_ = 0, posY_ = 0;            // position of the last frame in the result
};

class PanoramicSession {
public:
    // done receives the final image (empty if the user cancels)
    void Start(RECT area, std::function<void(Image)> done);
    bool active() const { return bar_ != nullptr; }
private:
    static LRESULT CALLBACK BarProc(HWND, UINT, WPARAM, LPARAM);
    void Grab();
    void Finish(bool ok);
    void PaintBar(HDC dc);
    int HitButton(POINT p) const;

    RECT area_{};
    HWND bar_ = nullptr, border_ = nullptr;
    std::function<void(Image)> done_;
    PanoramaStitcher stitch_;
    int hover_ = 0;
    RECT btnStop_{}, btnCancel_{};
};

} // namespace snap
