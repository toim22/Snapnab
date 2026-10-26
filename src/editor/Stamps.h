// Stamp gallery (StampGalleryManager, StampSearcher, OrganizeStamps; ed_comp_8 §4)
// Built-in categories with color stamps (emoji) and "My stamps": images in the user's folder
// (%LOCALAPPDATA%\Snapnab\Stamps; each subfolder becomes one more category).
#pragma once
#include "Image.h"

namespace snap::ed {

struct StampItem { std::wstring id, name; };      // id: emoji, "file:path" or "label:RRGGBB:Text"
// key: identifies the category in any language (order and pins are saved by it); pinned: the "Pinned" category
struct StampCategory { std::wstring name; std::vector<StampItem> items; bool mine = false; std::wstring key; bool pinned = false; };

// "Pinned" (when there are pinned stamps) + "My stamps" (and subfolders) + the built-in ones, each in the order chosen
// by the user (stamps dragged in the gallery) with the pinned stamps first
std::vector<StampCategory> StampCategories();
void MoveStamp(const StampCategory& cat, int from, int to);       // drag in the gallery: saves the category's new order
bool IsStampPinned(const std::wstring& id);
void SetStampPinned(const std::wstring& id, bool pin);
std::vector<StampItem> SearchStamps(const std::vector<StampCategory>& cats, const std::wstring& text);
std::wstring MyStampsDir();
int ImportStamps(HWND owner);                       // picks images and copies them to My stamps; returns how many
bool AddImageToMyStamps(const Image& img, std::wstring& savedPath);   // "Add to my stamps"

} // namespace snap::ed
