// Documento do Editor (CPaintDocument / IPaintOperation, comp_5 §3 / ed_comp_2)
//   imagem base (raster) + lista de objetos vetoriais editáveis + pilha de desfazer
#pragma once
#include "Image.h"
#include "SnapFile.h"

namespace snap::ed {

enum class ObjType { Arrow, Line, Rect, Ellipse, RoundRect, Pen, Highlighter, Text, Callout, Step, Stamp,
                     Blur, Pixelate, Spotlight, Magnify, Picture, Shape, Group, Count };

// pontas de linha/seta (LineStartCap/LineEndCap do tema, ed_comp_8 §5.1); 0 e 1 valem os antigos "sem seta"/"seta"
enum Cap { CapFlat, CapArrow, CapArrowWide, CapArrowOpen, CapRound, CapDot, CapSquare, CapDiamond, CapCount };
// tracejado (DashType)
enum Dash { DashSolid, DashDash, DashDot, DashDashDot, DashDashDotDot, DashLong, DashCount };
// formas da ferramenta Forma (ToolShape)
enum ShapeKind { ShapeRect, ShapeRoundRect, ShapeEllipse, ShapeTriangle, ShapeDiamond, ShapeParallelogram, ShapeHexagon,
                 ShapeStar, ShapeCount };
constexpr int kCalloutShapes = 24;     // CalloutShape: os 24 formatos da galeria de balões
// tipos de numeração do Passo (StepSequenceType)
enum StepType { StepNumbers, StepUpper, StepLower, StepRoman, StepRomanLower, StepTypeCount };

struct Style {
    uint32_t stroke = 0xFFE5322D;        // cor principal (linha, contorno, preenchimento do passo)
    uint32_t fill = 0x00000000;          // preenchimento (alfa 0 = sem)
    uint32_t textColor = 0xFF000000;     // texto: cor das letras
    float width = 4;                     // espessura / contorno
    int opacity = 100;                   // %
    bool shadow = true;
    // sombra (ShadowPropertiesControl): direção na grade 3×3 (4 = em volta), cor, opacidade, desfoque e distância
    int shadowDir = 8;
    uint32_t shadowColor = 0xFF000000;
    int shadowOpacity = 45, shadowBlur = 6, shadowDist = 5;
    std::wstring font = L"Segoe UI";
    float fontSize = 28;
    bool bold = true, italic = false, underline = false;
    int alignH = 1, alignV = 1;          // 0 esquerda/topo, 1 centro/meio, 2 direita/base
    float padding = 5;
    int arrowStart = CapFlat, arrowEnd = CapArrow;    // pontas (Cap)
    float startSize = 3, endSize = 3;    // tamanho das pontas
    int dash = DashSolid;
    bool bezier = false;                 // linha/seta curva (alça no meio)
    int calloutShape = 13;               // 0..23
    bool shaded = false;                 // balão com degradê
    uint32_t tailColor = 0xFFE5322D;     // seta do balão com linha (cor e espessura)
    float tailWidth = 6;
    int shapeKind = ShapeRect;
    int stepShape = 0, stepType = StepNumbers;        // passo: 0 círculo, 1 gota, 2 quadrado, 3 quadrado arredondado
    int magnifyShape = 0, magnifyScale = 200;         // lupa: 0 círculo, 1 retângulo, 2 arredondado; ampliação em %
    bool magnifyLink = false;            // lupa afastada da área ampliada, ligada a ela por um fio
    int hideKind = 0;                    // ocultar: 0 desfoque, 1 pixelado
    bool antialias = true;               // borracha
    int amount = 10;                     // intensidade do desfoque/pixelização/destaque
};

struct DrawObj {
    ObjType type = ObjType::Rect;
    Gdiplus::PointF p1{}, p2{};          // reta: extremos; demais: cantos do retângulo
    std::vector<Gdiplus::PointF> pts;    // caneta (e marcadores antigos à mão livre)
    Gdiplus::PointF tail{};              // ponta do balão / centro da área ampliada pela lupa ligada
    Gdiplus::PointF ctrl{};              // ponto de controle da linha curva (Bézier)
    std::wstring text;                   // texto / balão / carimbo (emoji ou "file:caminho")
    int stepValue = 1;
    Style style;
    std::shared_ptr<Image> picture;      // imagem embutida (colar, Smart Move)
    std::vector<DrawObj> children;       // Group (CPaintOpGroup, <Group>…</Group>): os objetos agrupados, cópia por valor

    Gdiplus::RectF Rect() const;         // retângulo normalizado (p1..p2)
    Gdiplus::RectF Bounds() const;       // inclui traço, pontas, ponta do balão, sombra
    bool IsLine() const { return type == ObjType::Arrow || type == ObjType::Line; }
    bool IsPath() const { return type == ObjType::Pen || (type == ObjType::Highlighter && !pts.empty()); }
    bool HasText() const { return type == ObjType::Text || type == ObjType::Callout; }
    bool IsShape() const { return type == ObjType::Shape || type == ObjType::Rect || type == ObjType::Ellipse || type == ObjType::RoundRect; }
    int Kind() const;                    // forma efetiva (ShapeKind), também para Rect/Ellipse/RoundRect antigos
    bool CalloutHasTail() const;         // formato de balão com ponta arrastável
    bool HitTest(Gdiplus::PointF p, float tol) const;
    void Move(float dx, float dy);
    void Scale(Gdiplus::RectF from, Gdiplus::RectF to);
    void MapPoints(const std::function<Gdiplus::PointF(Gdiplus::PointF)>& f);   // todos os pontos (girar/redimensionar a imagem)
    void UpdateGroupRect();              // grupo: p1..p2 = área ocupada pelos filhos
};
std::wstring StepLabel(int value, int type);
float CapLength(float width, float size);          // comprimento da ponta de seta para a espessura e o tamanho
using ObjPtr = std::shared_ptr<DrawObj>;

struct DocState {
    std::shared_ptr<const Image> base;
    std::vector<ObjPtr> objs;
    int nextStep = 1;
};

class Document {
public:
    std::wstring path;                   // .snap (biblioteca ou externo) ou .mp4
    std::wstring title;
    bool isVideo = false;
    bool modified = false;
    std::wstring captureMode, windowTitle, application, created;
    std::wstring tags;                   // etiquetas (Etiquetar imagem), separadas por vírgula
    DocState st;

    int w() const { return st.base ? st.base->w : 0; }
    int h() const { return st.base ? st.base->h : 0; }

    void Checkpoint();                   // empilha o estado atual para desfazer
    bool CanUndo() const { return !undo_.empty(); }
    bool CanRedo() const { return !redo_.empty(); }
    void Undo();
    void Redo();
    void UndoAll();
    void ClearHistory() { undo_.clear(); redo_.clear(); }
    void DiscardCheckpoint() { if (!undo_.empty()) undo_.pop_back(); }   // operação acabou sem mudança
    size_t UndoDepth() const { return undo_.size(); }

    DrawObj* Mutable(size_t idx);        // clona o objeto (copy-on-write) e devolve ponteiro editável
    Image& MutableBase();                // clona a imagem base
    void SetBase(Image img);

    Image Render(const DrawObj* hideText = nullptr) const;   // compõe imagem final
    Image Flattened() const { return Render(); }

    bool Save();                         // grava .snap em 'path'
    bool SaveAs(const std::wstring& file, int jpegQuality = 90);
    static std::unique_ptr<Document> Load(const std::wstring& file);
    static std::unique_ptr<Document> FromImage(Image img, const std::wstring& title);
private:
    std::vector<DocState> undo_, redo_;
};

// renderização (Render.cpp)
void RenderObject(Gdiplus::Graphics& g, Image& canvas, const DrawObj& o, bool hideText = false);
void ShapePath(Gdiplus::GraphicsPath& gp, int kind, Gdiplus::RectF r);                      // contorno de uma ShapeKind
void CalloutBodyPath(Gdiplus::GraphicsPath& gp, int shape, Gdiplus::RectF r);              // corpo de um balão (sem a ponta)
Gdiplus::RectF CalloutTextRect(const DrawObj& o);                                          // área do texto dentro do balão
float TextBlockHeight(const DrawObj& o, float width);                                    // altura do texto quebrado na largura
void DrawCapGlyph(Gdiplus::Graphics& g, int cap, Gdiplus::RectF r, Gdiplus::Color c, bool atStart);   // amostra para menus
void ApplyDash(Gdiplus::Pen& p, int dash);
void ApplyOpacityLayer(Image& canvas, const Image& layer, int opacity);
Gdiplus::Color Gc(uint32_t argb, int opacityPercent = 100);

// serialização XML de objetos (Serialize.cpp)
XmlNode ObjToXml(const DrawObj& o, SnapFrame& frame, int& imgCounter);
ObjPtr ObjFromXml(const XmlNode& n, const SnapFrame& frame);

// PDF (Pdf.cpp); o OCR (RunOcr) fica em common/Ocr.h, compartilhado com a captura
bool SavePdf(const std::wstring& path, const Image& img, std::wstring& err);

} // namespace snap::ed
