// Painéis do Editor: barra de ferramentas, propriedades (ToolProperties), efeitos e bandeja/biblioteca (Tray, CLibraryView).
// Todos seguem as cores da janela de captura (common/Theme): fundos puxados para a cor de destaque do Windows.
#pragma once
#include "Canvas.h"
#include "Icons.h"
#include "Library.h"
#include "Theme.h"

namespace snap::ed {

const ui::ThemeColors& EdTheme();
void SetEdTheme(const ui::ThemeColors& t);

// barra de rolagem desenhada pelo próprio painel: arrastar o polegar rola, clicar no trilho avança uma página
struct LiteScroll {
    RECT track{};                 // em coordenadas da janela
    int content = 0, view = 0, pos = 0;
    bool dragging = false, hot = false;
    int grabY = 0, grabPos = 0;
    bool Visible() const { return view > 0 && content > view; }
    int MaxPos() const { return std::max(0, content - view); }
    RECT Thumb() const;
    bool Hit(POINT p) const;
    bool Down(POINT p);           // true se o clique foi na barra (pode mudar pos)
    bool Move(POINT p);           // durante o arraste; true se pos mudou
    void Up() { dragging = false; }
    void Paint(Gdiplus::Graphics& g) const;
};

// ---- barra de ferramentas (TopPanel.ToolBar, ed_comp_3 §3.2) ----
// Itens dos tipos ToolBarItemType: ferramenta, efeito, saída, comando e espaçador; no fim, o "Mais/Editar" (EditMore)
// com as ferramentas que não estão na barra e a personalização. Ações fixas nas pontas: Capturar/Gravar e
// Copiar tudo/Salvar como/Compartilhar. Arrastar um item o leva para outro lugar; durante a personalização,
// arrastar para fora da barra o remove (como as barras BCG).
struct ToolbarEntry {
    enum Type { Tool, Effect, Output, Command, Spacer } type = Tool;
    int id = 0;                                   // ferramenta (Tool) ou comando do menu
    bool operator==(const ToolbarEntry& o) const { return type == o.type && id == o.id; }
};
struct ToolbarEntryInfo { icons::Id icon = icons::Gear; std::wstring label, hint; };

class Toolbar {
public:
    struct Item { int id; icons::Id icon; std::wstring label; std::wstring hint; bool right; bool primary; };
    HWND hwnd = nullptr;
    std::vector<Item> items;                      // ações fixas
    std::vector<ToolbarEntry> center;             // itens personalizáveis, em ordem
    Tool active = Tool::Arrow;
    bool customizing = false;                     // janela Personalizar aberta: clique não executa, arrastar para fora remove
    std::function<ToolbarEntryInfo(const ToolbarEntry&)> describe;
    std::function<void(int id)> onClick;
    std::function<void(const ToolbarEntry&)> onEntry;
    std::function<void(POINT screen)> onMore;     // botão "Mais": ponto para abrir o menu
    std::function<void(std::vector<ToolbarEntry>)> onReorder;
    std::function<void(const std::wstring&)> onHint;
    void Create(HWND parent);
    void SetActive(Tool t) { active = t; InvalidateRect(hwnd, nullptr, FALSE); }
    void SetCenter(std::vector<ToolbarEntry> v) { center = std::move(v); Layout(); InvalidateRect(hwnd, nullptr, FALSE); }
    int Height() const;
    int DropIndex(POINT screen) const;            // posição de inserção sob o ponto (-1 = fora da barra)
    void ShowDropCaret(int index) { if (index != caret_) { caret_ = index; InvalidateRect(hwnd, nullptr, FALSE); } }
    void InsertAt(int index, const ToolbarEntry& e);   // item já na barra (que não seja espaçador) muda de lugar
private:
    struct Slot { RECT r; int kind; int index; };  // kind: 0 ação, 1 item personalizável, 2 "Mais"
    static LRESULT CALLBACK Proc(HWND, UINT, WPARAM, LPARAM);
    void Paint(HDC dc);
    void Layout();
    int Hit(POINT p) const;
    int InsertIndexAt(POINT client) const;
    std::vector<Slot> slots_;
    int hover_ = -1, pressed_ = -1, dragFrom_ = -1, caret_ = -1;
    POINT downPt_{};
    bool dragging_ = false, dragOut_ = false;
};

// janela "Personalizar a barra de ferramentas" (CBCGPToolbarCustomize): grade com as ferramentas, comandos, destinos
// e efeitos, para arrastar até a barra. Concluído mantém; Cancelar devolve false (quem chamou restaura a barra).
// toolCount: quantas ferramentas a barra automática mostra (onToolCount avisa a troca, que volta a barra para automática)
bool CustomizeToolbarDialog(HWND owner, Toolbar& bar, const std::vector<std::pair<std::wstring, std::vector<ToolbarEntry>>>& catalog,
                            std::function<ToolbarEntryInfo(const ToolbarEntry&)> describe, std::function<std::vector<ToolbarEntry>()> defaults,
                            int toolCount, std::function<void(int)> onToolCount);

// ---- propriedades (CDotNetPaintProperties / ToolProperties.*, ed_comp_3 §2.3, ed_comp_4 §2) ----
// Em cima, os estilos rápidos da ferramenta (tema, galeria, favoritos com a estrela, "+" adiciona o estilo atual);
// na ferramenta Carimbo, a galeria de carimbos com busca e categorias. Embaixo, as propriedades da ferramenta:
// cores, sombra (direção na grade 3×3), forma, pontas e tracejado, larguras com controle deslizante, texto avançado.
class PropsPanel {
public:
    HWND hwnd = nullptr;
    Canvas* canvas = nullptr;
    Tool barTool = Tool::Arrow;                   // ferramenta escolhida na barra (inclui Favoritos e Varinha)
    std::function<void(int)> onCommand;           // botões de ação (324..329)
    std::function<void(Tool)> onPickTool;         // favorito escolhido: troca a ferramenta do canvas
    std::function<void()> onStylesChanged;        // estilo de alguma ferramenta mudou (lembrado entre sessões)
    std::function<void(const std::wstring&)> onHint;
    void Create(HWND parent, Canvas* c);
    void SetBarTool(Tool t);
    void Load();                                  // canvas → controles
    void SetDocInfo(const std::wstring& info) { docInfo_ = info; InvalidateRect(hwnd, nullptr, FALSE); }
    void ThemeChanged();
private:
    struct Ctl { int id; int kind; RECT r; std::wstring label; int minV = 0, maxV = 100; bool enabled = true; };
    struct Tile { RECT r; int index; bool add; };
    static LRESULT CALLBACK Proc(HWND, UINT, WPARAM, LPARAM);
    LRESULT Handle(UINT m, WPARAM w, LPARAM l);
    void Layout();
    void Paint(HDC dc);
    void PaintCtl(Gdiplus::Graphics& g, HDC dc, const Ctl& c);
    Tool PropTool() const;                        // ferramenta cujas propriedades aparecem
    Tool StyleTool() const;                       // ferramenta dos estilos rápidos (Favorites = favoritos)
    bool ObjSelected() const;
    Style Cur() const;
    void SetStyle(const Style& s, bool newStep = true);
    int Value(int id) const;
    void SetValue(int id, int v, bool newStep);
    void ClickCtl(const Ctl& c, POINT p);
    void ClickTile(int i, bool star);
    void TileMenu(int i, POINT screen);
    void ThemeMenu();
    void FillThemeCombo();
    void FillStampCats();
    const Image& Thumb(Tool t, const Style& s, int px);
    const Image& StampThumb(const std::wstring& id, int px);
    HWND Child(int id) { return children_[id]; }
    HWND EditFor(int ctlId);
    std::vector<Ctl> ctls_;
    std::vector<Tile> tiles_;
    std::vector<Style> tileStyles_;               // estilos da galeria atual (ou dos favoritos)
    std::vector<std::wstring> stampIds_;
    std::map<int, HWND> children_, edits_;
    std::map<std::wstring, Image> thumbs_;
    RECT qsHeader_{}, addBtn_{}, gridRect_{}, tpHeader_{}, helpBtn_{};
    int gridScroll_ = 0, gridContent_ = 0, scroll_ = 0, content_ = 0;
    int hoverTile_ = -1, hoverCtl_ = -1, dragCtl_ = -1;
    bool hoverStar_ = false, hoverAdd_ = false, advanced_ = true, loading_ = false, dragFirst_ = false;
    int lastEdit_ = -1;
    int stampCat_ = 1;
    LiteScroll gridSb_, panelSb_;                 // rolagem da galeria e do painel inteiro
    std::wstring docInfo_, stampSearch_;
    HBRUSH bg_ = nullptr;
};

// ---- efeitos (SidePanel.Effects / EffectsPanelControls, ed_comp_5 §4) ----
// No topo, a galeria de efeitos rápidos (QuickEffectGalleryItemVm); embaixo, cada efeito com prévia na imagem atual.
// Clicar num efeito abre os ajustes dentro do painel: cada mudança mostra a prévia ao vivo na imagem
// (PreviewEffect) e "Aplicar" grava o efeito (ApplyEffect); "Cancelar" volta ao documento.
struct FxParam {
    enum Kind { Choice, Toggle, Color, Text } kind = Choice;
    std::wstring label;
    std::vector<std::wstring> options;            // Choice: textos
    std::vector<int> values;                      // Choice: valor de cada opção
    int value = 0;                                // Choice: valor escolhido; Toggle: 0/1
    COLORREF color = 0;
    std::wstring text;
    static FxParam MakeChoice(std::wstring l, std::vector<std::wstring> o, std::vector<int> v, int val) {
        FxParam p; p.kind = Choice; p.label = std::move(l); p.options = std::move(o); p.values = std::move(v); p.value = val; return p;
    }
    static FxParam MakeToggle(std::wstring l, bool on) { FxParam p; p.kind = Toggle; p.label = std::move(l); p.value = on; return p; }
    static FxParam MakeColor(std::wstring l, COLORREF c) { FxParam p; p.kind = Color; p.label = std::move(l); p.color = c; return p; }
    static FxParam MakeText(std::wstring l, std::wstring t) { FxParam p; p.kind = Text; p.label = std::move(l); p.text = std::move(t); return p; }
};

class EffectsPanel {
public:
    struct Effect { int cmd; std::wstring name, desc, group; std::vector<FxParam> params; };
    HWND hwnd = nullptr;
    std::vector<Effect> effects;
    std::vector<std::wstring> quick;              // nomes dos efeitos rápidos
    // prévia em miniatura: cmd >= 0 é um efeito (com os ajustes atuais), cmd < 0 é o efeito rápido -cmd-1
    std::function<Image(int cmd, const std::vector<FxParam>& params, const Image& src)> makePreview;
    std::function<void(int cmd, const std::vector<FxParam>& params)> onPreview;   // prévia ao vivo na imagem
    std::function<void(int cmd, const std::vector<FxParam>& params)> onApply;
    std::function<void()> onCancel;
    std::function<void(int index)> onQuick;
    std::function<void()> onAddQuick;
    std::function<void(int index)> onDeleteQuick;
    void Create(HWND parent);
    void SetSource(const Image& img);             // imagem atual (vazia = exemplo) → refaz as prévias
    void SetQuick(std::vector<std::wstring> names);
    void Expand(int cmd);                         // abre os ajustes de um efeito (também pelo botão na barra)
    void Collapse();                              // fecha sem aplicar (avisa onCancel)
    bool IsOpen() const { return open_ >= 0; }
    const Effect* Find(int cmd) const;
private:
    static LRESULT CALLBACK Proc(HWND, UINT, WPARAM, LPARAM);
    LRESULT Handle(UINT m, WPARAM w, LPARAM l);
    void Paint(HDC dc);
    void Relayout();                              // calcula as áreas e posiciona os controles do efeito aberto
    void BuildPreviews();
    void CreateControls();
    void DestroyControls();
    void ParamChanged(int i);
    int HitRow(POINT p) const;
    int HitTile(POINT p) const;                   // índice do efeito rápido; quick.size() = "Adicionar"
    int SettingsHeight() const;
    Image src_;
    std::vector<Image> previews_, quickPreviews_;
    std::vector<RECT> rows_, tiles_;              // em coordenadas de conteúdo (sem rolagem)
    std::vector<std::pair<int, std::wstring>> heads_;
    RECT settings_{};
    int contentH_ = 0, scroll_ = 0, hover_ = -1, hoverTile_ = -1, open_ = -1;
    std::vector<HWND> ctls_;
    HWND apply_ = nullptr, cancel_ = nullptr;
    HBRUSH bg_ = nullptr;
};

// ---- bandeja de capturas recentes (Tray) e vista da biblioteca (CLibraryView) ----
// Bandeja: as capturas recentes (RecentTrayDocs, padrão 10), guardadas em Tray.bin; novas capturas e documentos
// abertos entram na frente. Biblioteca: todas as capturas em grade. Nas duas: Ctrl/Shift+clique marca várias,
// Ctrl+A marca todas, Del ou o X da miniatura exclui, vários marcados oferecem exportar/excluir, e dá para
// arrastar itens para outros programas (TrayDragDrop) e soltar arquivos do Explorer.
class LibraryTray {
public:
    enum Mode { Tray, Library };
    HWND hwnd = nullptr;
    Mode mode = Tray;
    std::function<void(const std::wstring&)> onOpen;
    std::function<void(const std::vector<std::wstring>&)> onDelete;
    std::function<void(const std::vector<std::wstring>&)> onExport;
    std::function<void(const std::vector<std::wstring>&)> onDropFiles;        // arquivos soltos do Explorer
    std::function<std::vector<std::wstring>(const std::vector<std::wstring>&)> filesForDrag;   // itens → arquivos para arrastar
    std::function<void()> onShowLibrary;          // botão "Biblioteca" da bandeja / "Voltar" da biblioteca
    std::wstring current;
    void Create(HWND parent, Mode m);
    void Reload();
    void SetCurrent(const std::wstring& p);       // documento ativo (na bandeja, também entra nas recentes)
    void InvalidateThumb(const std::wstring& p);
    void SelectAll();
    void RemoveFromTray(const std::vector<std::wstring>& paths);
    int Height() const;
    static bool DraggingOut() { return s_draggingOut; }   // arrasto saindo da bandeja/biblioteca em andamento
private:
    static bool s_draggingOut;
    static LRESULT CALLBACK Proc(HWND, UINT, WPARAM, LPARAM);
    LRESULT Handle(UINT m, WPARAM w, LPARAM l);
    void Paint(HDC dc);
    int Hit(POINT p) const;
    RECT CellRect(int i) const;                   // em coordenadas da janela (com rolagem)
    RECT CloseRect(int i) const;                  // X no canto da miniatura
    RECT HeaderButton(int which) const;           // 0 exportar, 1 excluir, 2 limpar seleção, 3 biblioteca/voltar
    int Columns() const;
    int ContentExtent() const;
    void UpdateScroll();
    std::vector<std::wstring> Selection() const;
    void EnsureVisible();
    void StartDrag(int item);
    void LoadRecent();
    void SaveRecent() const;
    void AddRecent(const std::wstring& path);
    const Image& Thumb(const std::wstring& path, bool video);
    std::vector<LibraryItem> items_;
    std::vector<std::wstring> recent_;            // bandeja: caminhos em ordem (Tray.bin)
    std::vector<std::wstring> hidden_;            // tirados da bandeja pelo usuário (não voltam ao preencher vagas)
    std::vector<std::wstring> known_;             // itens da biblioteca já vistos (os novos entram na bandeja)
    std::map<std::wstring, Image> thumbs_;
    std::vector<bool> sel_;
    int anchor_ = -1, downItem_ = -1;
    POINT downPt_{};
    int scroll_ = 0, hover_ = -1, hoverClose_ = -1, hoverHeader_ = -1;
    // seleção por retângulo (como no Explorer): começa no espaço vazio e marca as miniaturas que ele toca
    bool band_ = false;
    POINT bandStart_{}, bandEnd_{};               // em coordenadas de conteúdo (sem a rolagem)
    std::vector<bool> bandBase_;                  // seleção de antes (Ctrl/Shift somam a ela)
    RECT BandRect() const;                        // em coordenadas da janela
    void UpdateBand(POINT p);
};

} // namespace snap::ed
