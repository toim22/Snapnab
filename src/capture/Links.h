// Links of the captured area (F32): UI Automation finds the hyperlinks of web pages and documents under the
// capture (browsers expose the URL as the element value) so they are stored with the image as clickable areas
#pragma once
#include "Common.h"

namespace snap {

struct CapturedLink { RECT r; std::wstring url; };    // screen coordinates

// links visible in 'area' (screen), searched in the windows of browsers and document viewers under it; gives up
// after 'budgetMs' (the search runs on a worker thread and what it found until then is used)
std::vector<CapturedLink> FindLinks(RECT area, DWORD budgetMs = 900);

} // namespace snap
