// Galeria de carimbos (StampGalleryManager, StampSearcher, OrganizeStamps; ed_comp_8 §4)
// Categorias de fábrica com carimbos coloridos (emoji) e "Meus carimbos": imagens na pasta do usuário
// (%LOCALAPPDATA%\Snapnab\Stamps; cada subpasta vira mais uma categoria).
#pragma once
#include "Image.h"

namespace snap::ed {

struct StampItem { std::wstring id, name; };      // id: emoji ou "file:caminho"
struct StampCategory { std::wstring name; std::vector<StampItem> items; bool mine = false; };

std::vector<StampCategory> StampCategories();      // "Meus carimbos" (e subpastas) + as de fábrica
std::vector<StampItem> SearchStamps(const std::vector<StampCategory>& cats, const std::wstring& text);
std::wstring MyStampsDir();
int ImportStamps(HWND owner);                       // escolhe imagens e copia para Meus carimbos; devolve quantas
bool AddImageToMyStamps(const Image& img, std::wstring& savedPath);   // "Adicionar aos meus carimbos"

} // namespace snap::ed
