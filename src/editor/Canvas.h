// Área de pintura (CEditorPaintControl, comp_5 §3.5): zoom, rolagem, ferramentas, alças e seleção raster
#pragma once
#include "Document.h"
#include "Cv.h"

namespace snap::ed {

// Favorites e MagicWand são itens da barra: Favoritos mostra os estilos favoritos de todas as ferramentas e a
// Varinha mágica é a Seleção no modo varinha
enum class Tool { Select, Arrow, Line, Shape, Pen, Highlighter, Text, Callout, Step, Stamp, Blur, Spotlight,
                  Magnify, Fill, Eraser, Crop, Selection, Cutout, Eyedropper, Favorites, MagicWand, Count };
const wchar_t* ToolName(Tool t);

class Canvas {
public:
    HWND hwnd = nullptr;
    Document* doc = nullptr;
    Tool tool = Tool::Arrow;
    std::map<Tool, Style> styles;
    std::wstring stampId = L"👍";   // carimbo escolhido na galeria (emoji ou "file:caminho")
    bool pickColorOnce = false; // conta-gotas do Preencher: o próximo clique pega a cor de preenchimento
    bool snapping = true;       // encaixe no canvas: bordas e centro do objeto grudam nas da imagem ao mover
    bool smartMove = false;     // Mover com Smart Move: arraste no fundo seleciona uma área e a levanta (inpainting)
    int selMode = 0;            // 0 retângulo, 1 elipse, 2 laço, 3 varinha
    int selFill = 2;            // 0 transparente, 1 cor do canvas, 2 inteligente (inpainting)
    int cutDir = 0;             // 0 remove faixa horizontal, 1 vertical
    float tolerance = 15;
    bool wandGlobal = false;
    COLORREF background = RGB(222, 224, 228);
    double zoom = 1.0;
    int selected = -1;          // objeto selecionado (o principal, quando há vários)

    std::function<void()> onDocChanged;         // documento mudou (desfazer, miniatura, título)
    std::function<void()> onSelectionChanged;   // objeto selecionado mudou (painel de propriedades)
    std::function<void(uint32_t)> onColorPicked;
    std::function<void()> onZoomChanged;

    void Create(HWND parent);
    void SetDocument(Document* d);
    void SetTool(Tool t);
    void Refresh(bool docChanged = true);
    Style& CurrentStyle();

    void ZoomTo(double z, POINT* anchorClient = nullptr);
    void ZoomIn() { ZoomTo(zoom * 1.25); }
    void ZoomOut() { ZoomTo(zoom / 1.25); }
    void FitToWindow();

    // comandos
    bool HasRasterSelection() const { return hasSel_; }
    bool HasCrop() const { return hasCrop_; }
    void ClearRasterSelection();
    void SelectAll();
    void DeleteSelected();
    bool CopySelection(bool cut);
    void CropToSelection();
    void ApplyCrop();
    void PasteImage(const Image& img);
    void BringToFront();
    void SendToBack();
    void BringForward();                       // um passo para a frente / para trás na ordem
    void SendBackward();
    void AlignSelected(int mode);              // 0 esquerda, 1 centro, 2 direita, 3 topo, 4 meio, 5 base (em relação à imagem)
    bool FlipSelected(bool horizontal);        // espelha o objeto selecionado; false se não há objeto
    void FlattenSelected();                    // mescla só os objetos selecionados na imagem
    // seleção de vários objetos (Mover: Ctrl/Shift+clique ou retângulo de seleção) e grupos (CPaintOpGroup)
    std::vector<int> SelectedList() const;     // índices em ordem crescente (o principal incluído)
    int SelectionCount() const { return (int)SelectedList().size(); }
    bool IsSelected(int i) const;
    void SelectObjects(const std::vector<int>& idx);   // o último vira o principal
    void SelectAllObjects();
    void GroupSelected();
    void UngroupSelected();
    bool CanUngroup() const;
    void FlattenAll();
    void ApplyStyleToSelected(const Style& s, bool newStep = true);   // newStep=false funde com a edição anterior
    void SetSelectedStamp(const std::wstring& id);
    void CommitText();
    bool EditingText() const { return edit_ != nullptr; }
    Image SelectionImage() const;              // imagem da seleção raster (ou do objeto)
    RECT RasterSelectionBounds() const { return selBounds_; }
    // prévia ao vivo de um efeito (PreviewEffect, ed_comp_5 §4.2): mostra a imagem no lugar do documento até
    // aplicar ou cancelar; nullptr volta ao documento
    void SetEffectPreview(std::shared_ptr<Image> img) { fxPreview_ = std::move(img); InvalidateRect(hwnd, nullptr, FALSE); }
    bool HasEffectPreview() const { return fxPreview_ != nullptr; }
    void Nudge(int dx, int dy);

private:
    static LRESULT CALLBACK Proc(HWND, UINT, WPARAM, LPARAM);
    LRESULT Handle(UINT m, WPARAM w, LPARAM l);
    void Paint(HDC dc);
    void UpdateScrollBars();
    void Recompose();
    POINT Origin() const;
    Gdiplus::PointF ToDoc(POINT p) const;
    POINT ToScreen(Gdiplus::PointF p) const;
    RECT ToScreenRect(Gdiplus::RectF r) const;
    std::vector<POINT> HandlePoints(const DrawObj& o) const;
    int HitHandle(POINT p) const;
    int HitObject(Gdiplus::PointF p) const;
    void OnDown(POINT p, bool dbl);
    void OnMove(POINT p, WPARAM keys);
    void OnUp(POINT p);
    void BeginTextEdit(int idx, bool isNew);
    void LayoutTextEdit();                    // caixa de edição acompanha o texto: cresce para baixo e segue o alinhamento
    void FinishCreate();
    void StartSmartMove(Gdiplus::PointF p);
    void FillHole(Image& base, const cv::Mask& m);
    void SetCursorForPoint(POINT p);
    void EraseAt(Gdiplus::PointF p);

    Image composed_, display_;
    std::shared_ptr<Image> fxPreview_;
    bool dirty_ = true;
    POINT scroll_{};
    bool fit_ = true;
    // arraste
    enum class Drag { None, Create, Move, Resize, Pan, Crop, Select, Lasso, Erase, Cut, Marquee } drag_ = Drag::None;
    POINT downScreen_{};
    Gdiplus::PointF downDoc_{};
    POINT panStart_{};
    std::shared_ptr<DrawObj> preview_;
    DrawObj dragOrig_;
    std::vector<std::pair<int, DrawObj>> dragMulti_;   // originais de todos os objetos sendo movidos
    std::vector<int> extra_;    // outros objetos selecionados além do principal
    void BeginMove();
    int handle_ = -1;
    bool changed_ = false;
    // seleção raster
    bool hasSel_ = false;
    cv::Mask selMask_;
    RECT selBounds_{};
    Gdiplus::RectF selRect_{};
    std::vector<Gdiplus::PointF> lasso_;
    Image selOverlay_;
    // recorte
    bool hasCrop_ = false;
    Gdiplus::RectF crop_{};
    // texto
    HWND edit_ = nullptr;
    int editIdx_ = -1;
    bool editNew_ = false;
    HFONT editFont_ = nullptr;
};

} // namespace snap::ed
