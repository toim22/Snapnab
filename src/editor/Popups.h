// Listas suspensas do painel de propriedades: galeria em grade (formas, balões, pontas, tracejados) e a paleta de
// cores (ColorPalette do painel lateral). Abrem embaixo do controle, fecham ao clicar fora ou com Esc.
#pragma once
#include "Common.h"
#include "Icons.h"
#include <array>

namespace snap::ed {

using GridDraw = std::function<void(Gdiplus::Graphics& g, HDC dc, int index, RECT cell, bool hot, bool selected)>;

// devolve o índice escolhido ou -1
int GridPopup(HWND owner, POINT anchorScreen, int cols, int count, SIZE cell, int selected, GridDraw draw);

// lista com ícone, nome e atalho (botão "Mais" da barra): material, com a ferramenta atual realçada
struct PopupItem {
    int id = 0;
    icons::Id icon = icons::Gear;
    bool hasIcon = true;
    std::wstring label, shortcut;
    bool enabled = true, checked = false, separator = false;
};
int ListPopup(HWND owner, POINT anchorScreen, const std::vector<PopupItem>& items, int minWidth = 0);

// paleta: cores do tema, cores padrão, recentes, "Sem cor" (allowNone) e "Mais cores…" (seletor do Windows)
bool ColorPopup(HWND owner, POINT anchorScreen, uint32_t& color, bool allowNone, const std::array<uint32_t, 4>& themeColors);

} // namespace snap::ed
