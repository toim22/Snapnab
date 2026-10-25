// Estilos rápidos, temas de ferramentas e favoritos (QuickStyleList, ToolThemes, FavoritesProvider; ed_comp_8 §5)
// Um tema tem até 4 cores; os estilos rápidos de cada ferramenta são gerados a partir delas
// (GenerateQuickStylesForTheme) e o usuário pode acrescentar os seus (AddCustomQuickStyleToTheme).
// Os favoritos juntam estilos de qualquer ferramenta na ferramenta Favoritos.
#pragma once
#include "Canvas.h"
#include <array>

namespace snap::ed {

struct ToolTheme {
    std::wstring name;
    std::array<uint32_t, 4> colors{};
    bool builtin = false;
    std::map<int, std::vector<Style>> custom;      // por ferramenta (int(Tool))
};

struct Favorite { Tool tool = Tool::Arrow; Style style; };

class QuickStyleStore {
public:
    std::vector<ToolTheme> themes;
    int current = 0;
    std::vector<Favorite> favorites;

    void Load();                                   // ToolThemes.xml + Favorites.xml (cria os padrões se faltarem)
    void Save() const;
    void SaveFavorites() const;
    const ToolTheme& Theme() const { return themes[std::clamp(current, 0, (int)themes.size() - 1)]; }
    bool HasQuickStyles(Tool t) const;
    std::vector<Style> StylesFor(Tool t) const;    // gerados pelo tema + personalizados (no fim)
    int GeneratedCount(Tool t) const;
    void AddCustom(Tool t, const Style& s);
    void RemoveCustom(Tool t, int customIndex);
    bool IsFavorite(Tool t, const Style& s) const;
    void ToggleFavorite(Tool t, const Style& s);
    // temas (Novo tema…, Importar…, engrenagem: editar, duplicar, exportar, excluir)
    int AddTheme(ToolTheme th);                    // nome repetido ganha " (2)" (HandleCollisionAndAddTheme)
    bool ImportTheme(const std::wstring& path, std::wstring& err);
    bool ExportTheme(int index, const std::wstring& path) const;
    void DeleteTheme(int index);
};
QuickStyleStore& QS();

std::vector<Style> GenerateStyles(Tool t, const std::array<uint32_t, 4>& colors);
bool SameStyle(const Style& a, const Style& b);
Image QuickStylePreview(Tool t, const Style& s, int px);       // miniatura de um estilo (galeria)
XmlNode StyleToXml(const Style& s, const char* name = "Style");
Style StyleFromXml(const XmlNode& n);
// estilos atuais das ferramentas (lembrados entre as sessões)
void SaveToolStyles(const std::map<Tool, Style>& styles);
void LoadToolStyles(std::map<Tool, Style>& styles);

} // namespace snap::ed
