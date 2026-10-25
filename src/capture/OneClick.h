// One Click (OneClickWindow / OneClickViewModel, comp_7 §3): painel flutuante preso a uma borda da tela.
// Com o mouse em cima, mostra o botão de captura e a engrenagem; fora dele, recolhe e deixa só uma faixa fina
// na borda. Pode ser arrastado livremente e gruda na borda mais próxima ao soltar.
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
    std::function<void(POINT)> onMenu;        // engrenagem: ponto de tela para abrir o menu
    void Show(bool on);
    bool enabled() const { return enabled_; }
    void SetTheme(const OneClickTheme& t);
    void SetBusy(bool busy);                  // some durante uma captura (DisableOneClickUI)
    void SetTip(const std::wstring& tip) { tip_ = tip; }
private:
    static LRESULT CALLBACK Proc(HWND, UINT, WPARAM, LPARAM);
    void Create();
    void Render();                            // desenha e posiciona (UpdateLayeredWindow)
    void Snap(POINT cursor);                  // gruda na borda mais próxima depois de arrastar
    void Tick();
    void ShowTip(bool on);
    int Hit(POINT screen) const;              // 1 = captura, 2 = engrenagem, 0 = fundo, -1 = fora
    SIZE PanelSize(int dpi) const;
    RECT MonitorRect() const;
    int Dpi() const;

    HWND hwnd_ = nullptr, tipWnd_ = nullptr;
    OneClickTheme th_;
    std::wstring tip_ = TR_NOOP(L"Iniciar uma captura");
    bool enabled_ = false, busy_ = false;
    int edge_ = 0;                            // 0 em cima, 1 à direita, 2 embaixo, 3 à esquerda
    double pos_ = 0.82;                       // posição ao longo da borda (0..1)
    POINT monPt_{ 0, 0 };                     // um ponto do monitor escolhido
    double open_ = 0;                         // 0 recolhido … 1 aberto (animado)
    bool wantOpen_ = false, dragging_ = false, down_ = false, menu_ = false, tipOn_ = false;
    int hot_ = 0, pressed_ = 0;
    POINT dragStart_{}, dragWin_{};
    RECT win_{};                              // retângulo atual da janela
    POINT panelAt_{};                         // canto do painel dentro da janela
    ULONGLONG leftAt_ = 0;
};

} // namespace snap
