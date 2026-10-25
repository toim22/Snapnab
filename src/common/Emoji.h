// Carimbos coloridos: emoji desenhado com DirectWrite + Direct2D (fontes coloridas, Segoe UI Emoji).
// O carimbo do Editor (CPaintOpStamp) é rasterizado no tamanho atual do objeto, então continua nítido ao
// ser redimensionado (como os carimbos PDF vetoriais da referência, ed_comp_8 §4.1).
#pragma once
#include "Image.h"

namespace snap {

// emoji/texto centrado num quadrado de px × px, fundo transparente; vazio se o Direct2D não estiver disponível
Image RenderEmoji(const std::wstring& text, int px);

} // namespace snap
