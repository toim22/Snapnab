// Quick styles, tool themes and favorites (QuickStyleList, ToolThemes, FavoritesProvider; ed_comp_8 §5)
// A theme has up to 4 colors; each tool's quick styles are generated from them
// (GenerateQuickStylesForTheme) and the user can add their own (AddCustomQuickStyleToTheme).
// Favorites gather styles of any tool in the Favorites tool.
#pragma once
#include "Canvas.h"
#include <array>

namespace snap::ed {

struct ToolTheme {
    std::wstring name;
    std::array<uint32_t, 4> colors{};
    bool builtin = false;
    std::map<int, std::vector<Style>> custom;      // per tool (int(Tool))
};

struct Favorite { Tool tool = Tool::Arrow; Style style; };

class QuickStyleStore {
public:
    std::vector<ToolTheme> themes;
    int current = 0;
    std::vector<Favorite> favorites;

    void Load();                                   // ToolThemes.xml + Favorites.xml (creates the defaults if missing)
    void Save() const;
    void SaveFavorites() const;
    const ToolTheme& Theme() const { return themes[std::clamp(current, 0, (int)themes.size() - 1)]; }
    bool HasQuickStyles(Tool t) const;
    std::vector<Style> StylesFor(Tool t) const;    // generated from the theme + custom ones (at the end)
    int GeneratedCount(Tool t) const;
    void AddCustom(Tool t, const Style& s);
    void RemoveCustom(Tool t, int customIndex);
    bool IsFavorite(Tool t, const Style& s) const;
    void ToggleFavorite(Tool t, const Style& s);
    // themes (New theme…, Import…, gear: edit, duplicate, export, delete)
    int AddTheme(ToolTheme th);                    // a repeated name gets " (2)" (HandleCollisionAndAddTheme)
    bool ImportTheme(const std::wstring& path, std::wstring& err);
    bool ExportTheme(int index, const std::wstring& path) const;
    void DeleteTheme(int index);
};
QuickStyleStore& QS();

std::vector<Style> GenerateStyles(Tool t, const std::array<uint32_t, 4>& colors);
bool SameStyle(const Style& a, const Style& b);
Image QuickStylePreview(Tool t, const Style& s, int px);       // thumbnail of a style (gallery)
XmlNode StyleToXml(const Style& s, const char* name = "Style");
Style StyleFromXml(const XmlNode& n);
// current tool styles (remembered between sessions)
void SaveToolStyles(const std::map<Tool, Style>& styles);
void LoadToolStyles(std::map<Tool, Style>& styles);

} // namespace snap::ed
