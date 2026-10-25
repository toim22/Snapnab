// Webcam picture-in-picture (CCameraSource + MFPreviewWindow, comp_3 §8). A câmera é lida com o Media Foundation
// (IMFSourceReader → RGB32) e desenhada numa janela redonda sempre visível, posicionada dentro da área gravada:
// como a gravação copia a tela, a webcam entra no vídeo. A janela pode ser arrastada durante a gravação.
#pragma once
#include "Image.h"

namespace snap {

struct CameraInfo { std::wstring name, link; };
std::vector<CameraInfo> ListCameras();

enum class CamShape { Circle = 0, RoundedSquare, Square, Wide, Classic, Count };   // Wide = 16:9, Classic = 4:3
const wchar_t* CamShapeName(CamShape s);

struct WebcamStyle {
    CamShape shape = CamShape::Circle;
    bool chroma = false;                       // remove o fundo da cor escolhida (sem moldura nem sombra)
    COLORREF key = RGB(0, 177, 64);            // verde de tela verde
    int tolerance = 40;                        // 0..100
};

// alfa do chroma key de um pixel BGRA (0 = fundo removido, 255 = mantém); exposto para os testes
uint8_t ChromaAlpha(uint32_t px, COLORREF key, int tolerance);

class WebcamOverlay {
public:
    bool Start(const std::wstring& link, RECT area, const WebcamStyle& style, std::wstring& err);   // link vazio = primeira câmera
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
    std::atomic<double> aspect_{ 4.0 / 3 };    // largura/altura da imagem da câmera
    bool dragging_ = false;
    POINT dragOff_{};
};

} // namespace snap
