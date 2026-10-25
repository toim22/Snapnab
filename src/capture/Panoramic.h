// Captura com rolagem panorâmica (PanoramicCapture, comp_3/comp_1): depois de escolher a região, o usuário rola o
// conteúdo (página, planilha, conversa…) do jeito que quiser; a região é copiada várias vezes por segundo e cada
// quadro novo é encaixado no anterior pelo deslocamento detectado. O resultado é uma imagem única, comprida.
#pragma once
#include "Image.h"

namespace snap {

// Junta quadros sucessivos da mesma região rolada (vertical ou horizontal). Usado também pelos testes.
class PanoramaStitcher {
public:
    bool Add(const Image& frame);        // true se o quadro acrescentou conteúdo
    const Image& result() const { return canvas_; }
    int axis() const { return axis_; }   // 0 = ainda parado, 1 = vertical, 2 = horizontal
    static constexpr int kMaxSize = 32000;
private:
    bool Offset(const Image& a, const Image& b, bool vertical, int& d) const;
    Image canvas_, prev_;
    int axis_ = 0;
    int posX_ = 0, posY_ = 0;            // posição do último quadro no resultado
};

class PanoramicSession {
public:
    // done recebe a imagem final (vazia se o usuário cancelar)
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
