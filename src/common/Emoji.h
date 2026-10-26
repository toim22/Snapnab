// Color stamps: emoji drawn with DirectWrite + Direct2D (color fonts, Segoe UI Emoji).
// The Editor stamp (CPaintOpStamp) is rasterized at the object's current size, so it stays sharp when
// resized (like the reference's vector PDF stamps, ed_comp_8 §4.1).
#pragma once
#include "Image.h"

namespace snap {

// emoji/text centered in a px × px square, transparent background; empty if Direct2D is not available
Image RenderEmoji(const std::wstring& text, int px);

} // namespace snap
