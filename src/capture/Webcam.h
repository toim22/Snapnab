// Picture-in-picture webcam (CCameraSource + MFPreviewWindow, comp_3 §8). The camera is read with Media Foundation
// (IMFSourceReader → RGB32) and drawn in an always-on-top round window, placed inside the recorded area:
// since the recording copies the screen, the webcam ends up in the video. The window can be dragged while recording.
#pragma once
#include "Image.h"

namespace snap {

struct CameraInfo { std::wstring name, link; };
std::vector<CameraInfo> ListCameras();

enum class CamShape { Circle = 0, RoundedSquare, Square, Wide, Classic, Count };   // Wide = 16:9, Classic = 4:3
const wchar_t* CamShapeName(CamShape s);

struct WebcamStyle {
    CamShape shape = CamShape::Circle;
    bool chroma = false;                       // removes the background of the chosen color (no frame or shadow)
    COLORREF key = RGB(0, 177, 64);            // green-screen green
    int tolerance = 40;                        // 0..100
};

// chroma key alpha of a BGRA pixel (0 = background removed, 255 = kept); exposed for the tests
uint8_t ChromaAlpha(uint32_t px, COLORREF key, int tolerance);

class WebcamOverlay {
public:
    bool Start(const std::wstring& link, RECT area, const WebcamStyle& style, std::wstring& err);   // empty link = first camera
    void Stop();
    void SetVisible(bool on);
    bool running() const { return running_; }
    bool visible() const { return hwnd_ && IsWindowVisible(hwnd_); }
    ~WebcamOverlay() { Stop(); }
private:
    static LRESULT CALLBACK Proc(HWND, UINT, WPARAM, LPARAM);
    void ReadLoop(void* reader);
    void Render();

    HWND hwnd_ = nullptr;
    std::thread thread_;
    std::atomic<bool> running_{ false };
    std::mutex frameMutex_;
    Image frame_;
    WebcamStyle style_;
    std::atomic<double> aspect_{ 4.0 / 3 };    // width/height of the camera image
    bool dragging_ = false;
    POINT dragOff_{};
};

} // namespace snap
