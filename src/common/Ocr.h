// Text recognition (Grab Text) with the native Windows OCR
#pragma once
#include "Image.h"

namespace snap {

bool RunOcr(const Image& img, std::wstring& text, std::wstring& err);

} // namespace snap
