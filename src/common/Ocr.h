// Reconhecimento de texto (Grab Text) com o OCR nativo do Windows
#pragma once
#include "Image.h"

namespace snap {

bool RunOcr(const Image& img, std::wstring& text, std::wstring& err);

} // namespace snap
