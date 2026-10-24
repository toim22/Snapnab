// Seleção de entrada (CCapturePreparer, comp_1 §4–§6): mira sobre a tela congelada
#pragma once
#include "Image.h"

namespace snap {

enum class SelectMode { Region, Window, Object, Fixed, Freehand, MultiArea, PickWindow };

struct SelectResult {
    bool ok = false;
    RECT rect{};                    // coordenadas de tela
    HWND window = nullptr;          // janela de nível superior sob o clique
    HWND child = nullptr;           // controle mais interno sob o clique
    std::vector<POINT> path;        // mão livre (tela)
    std::vector<RECT> rects;        // várias áreas (tela)
};

// Abre a janela de seleção; 'done' é chamado uma vez, na thread de UI.
void RunSelectOverlay(const Image& frozen, RECT virtualScreen, SelectMode mode, SIZE fixedSize, bool video,
                      std::function<void(const SelectResult&)> done);

// Utilitários de janela usados pela seleção e pela captura
RECT WindowFrameRect(HWND h);        // DWMWA_EXTENDED_FRAME_BOUNDS (sem a sombra invisível)
std::wstring WindowTitle(HWND h);
std::wstring WindowProcessName(HWND h);

} // namespace snap
