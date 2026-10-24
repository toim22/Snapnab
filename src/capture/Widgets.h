// Controles com visual moderno (Windows 11 / material) para a janela de captura: chave liga/desliga,
// botão (normal, plano ou só ícone) e caixa de combinação com lista suspensa própria.
// Falam as mensagens dos controles padrão — BM_GETCHECK/BM_SETCHECK, CB_ADDSTRING/CB_SETCURSEL/CB_GETCURSEL/
// CB_SETITEMDATA…, e avisam o pai com WM_COMMAND (BN_CLICKED, CBN_DROPDOWN, CBN_SELCHANGE, CBN_CLOSEUP).
// O fundo de cada controle vem do pai por WM_CTLCOLORBTN.
#pragma once
#include "Common.h"
#include "Icons.h"

namespace snap::mui {

inline constexpr wchar_t kToggle[] = L"SnapnabToggle";
inline constexpr wchar_t kButton[] = L"SnapnabButton";
inline constexpr wchar_t kCombo[] = L"SnapnabCombo";

struct Palette {
    COLORREF accent = RGB(0, 120, 215);       // trilho da chave ligada, indicador do item selecionado
    COLORREF accentText = RGB(0, 84, 150);    // marcas, ícones de botões planos, anel de foco
    COLORREF line = RGB(222, 224, 228);       // contornos
};

void Register(HINSTANCE hi);
void SetPalette(const Palette& p);

enum : UINT {
    MUI_SETICON = WM_APP + 300,   // wParam = icons::Id + 1 (0 = sem ícone)
    MUI_SETSTYLE,                 // wParam = combinação de Style
    MUI_SETITEMFLAGS,             // combo: wParam = índice, lParam = ItemFlags
    MUI_SETDISPLAYTEXT,           // combo: lParam = texto mostrado no lugar do item selecionado (nullptr = volta ao item)
};
enum Style { ButtonFlat = 1, ButtonIconOnly = 2, ComboMenu = 4 /* lista de ações: as setas não trocam a seleção */ };
enum ItemFlags { ItemSeparator = 1, ItemChecked = 2 };

} // namespace snap::mui
