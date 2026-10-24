// Documento do Editor (CPaintDocument / IPaintOperation, comp_5 §3 / ed_comp_2)
//   imagem base (raster) + lista de objetos vetoriais editáveis + pilha de desfazer
#pragma once
#include "Image.h"
#include "SnapFile.h"

namespace snap::ed {

enum class ObjType { Arrow, Line, Rect, Ellipse, RoundRect, Pen, Highlighter, Text, Callout, Step, Stamp,
                     Blur, Pixelate, Spotlight, Magnify, Picture };

struct Style {
    uint32_t stroke = 0xFFE5322D;        // cor principal
    uint32_t fill = 0x00000000;          // preenchimento (alfa 0 = sem)
    uint32_t textColor = 0xFF000000;
    float width = 4;
    int opacity = 100;                   // %
    bool shadow = true;
    std::wstring font = L"Segoe UI";
    float fontSize = 28;
    bool bold = true;
    int arrowStart = 0, arrowEnd = 1;    // 0 nenhuma, 1 seta
    int calloutShape = 0;                // 0 arredondado, 1 retângulo, 2 elipse, 3 pensamento
    int amount = 10;                     // intensidade do desfoque/pixelização/lupa
};

struct DrawObj {
    ObjType type = ObjType::Rect;
    Gdiplus::PointF p1{}, p2{};          // reta: extremos; demais: cantos do retângulo
    std::vector<Gdiplus::PointF> pts;    // caneta / marcador
    Gdiplus::PointF tail{};              // ponta do balão
    std::wstring text;                   // texto / balão / carimbo
    int stepValue = 1;
    Style style;
    std::shared_ptr<Image> picture;      // imagem embutida (colar, Smart Move)

    Gdiplus::RectF Rect() const;         // retângulo normalizado (p1..p2)
    Gdiplus::RectF Bounds() const;       // inclui traço, ponta do balão, etc.
    bool IsLine() const { return type == ObjType::Arrow || type == ObjType::Line; }
    bool IsPath() const { return type == ObjType::Pen || type == ObjType::Highlighter; }
    bool HasText() const { return type == ObjType::Text || type == ObjType::Callout; }
    bool HitTest(Gdiplus::PointF p, float tol) const;
    void Move(float dx, float dy);
    void Scale(Gdiplus::RectF from, Gdiplus::RectF to);
};
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
void ApplyOpacityLayer(Image& canvas, const Image& layer, int opacity);
Gdiplus::Color Gc(uint32_t argb, int opacityPercent = 100);
std::vector<std::wstring> StampGlyphs();

// serialização XML de objetos (Serialize.cpp)
XmlNode ObjToXml(const DrawObj& o, SnapFrame& frame, int& imgCounter);
ObjPtr ObjFromXml(const XmlNode& n, const SnapFrame& frame);

// PDF (Pdf.cpp) e OCR (Ocr.cpp)
bool SavePdf(const std::wstring& path, const Image& img, std::wstring& err);
bool RunOcr(const Image& img, std::wstring& text, std::wstring& err);

} // namespace snap::ed
