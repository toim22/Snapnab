// Barra de título própria das janelas do Snapnab: ícone + menus na mesma faixa, título opcional e botões
// minimizar/maximizar/fechar. A janela tira a moldura do sistema (WM_NCCALCSIZE → 0) e repassa as mensagens daqui.
// Os menus abrem com clique, Alt+letra ou F10, e trocam para o vizinho com o mouse ou com as setas.
#pragma once
#include "Theme.h"

namespace snap::ui {

class CaptionBar {
public:
    struct Menu { std::wstring name; wchar_t key; HMENU menu; };   // key = letra do Alt (maiúscula)

    void Init(HWND frame, std::vector<Menu> menus, bool resizable, bool editorIcon);
    void SetTheme(const ThemeColors& t) { th_ = t; Invalidate(); }
    void SetTitle(const std::wstring& t) { title_ = t; Invalidate(); }
    void Layout(int width, int dpi);               // recalcula os retângulos (chame em WM_SIZE / WM_DPICHANGED)
    int Height() const { return h_; }
    void Paint(HDC dc);                            // desenha a faixa inteira (0, 0, largura, Height())
    std::function<void(HMENU, int index)> onInitMenu;   // antes de abrir um menu (marcar/desabilitar itens)

    // mensagens da janela: devolvem true se a barra tratou
    bool NcCalcSize(WPARAM w, LPARAM l, LRESULT& result);
    bool NcHitTest(LPARAM l, LRESULT& result);
    bool MouseMove(POINT client);
    void MouseLeave();
    bool LButtonDown(POINT client);
    bool LButtonUp(POINT client);
    bool RButtonUp(WPARAM w, LPARAM l);            // WM_NCRBUTTONUP: menu do sistema
    bool PreTranslate(const MSG& msg);             // Alt+letra / F10 no laço de mensagens
    void OpenMenu(int index);

private:
    static LRESULT CALLBACK MenuFilter(int code, WPARAM w, LPARAM l);
    int Hit(POINT p) const;                        // 0..n-1 menus, 100 min, 101 max, 102 fechar, -1 nada
    void Invalidate();

    HWND frame_ = nullptr;
    std::vector<Menu> menus_;
    std::vector<RECT> menuRects_;
    RECT btn_[3]{};                                // minimizar, maximizar, fechar
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
