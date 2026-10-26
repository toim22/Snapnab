// Editor document (CPaintDocument / IPaintOperation, comp_5 §3 / ed_comp_2)
//   base image (raster) + list of editable vector objects + undo stack
#pragma once
#include "Image.h"
#include "SnapFile.h"

namespace snap::ed {

enum class ObjType { Arrow, Line, Rect, Ellipse, RoundRect, Pen, Highlighter, Text, Callout, Step, Stamp,
                     Blur, Pixelate, Spotlight, Magnify, Picture, Shape, Group, Link, Count };
// Link: clickable area found by UI Automation when capturing (URL in 'text'); not drawn, kept in PDF exports

// line/arrow ends (theme LineStartCap/LineEndCap, ed_comp_8 §5.1); 0 and 1 are the old "no arrow"/"arrow"
enum Cap { CapFlat, CapArrow, CapArrowWide, CapArrowOpen, CapRound, CapDot, CapSquare, CapDiamond, CapCount };
// dash pattern (DashType)
enum Dash { DashSolid, DashDash, DashDot, DashDashDot, DashDashDotDot, DashLong, DashCount };
// shapes of the Shape tool (ToolShape)
enum ShapeKind { ShapeRect, ShapeRoundRect, ShapeEllipse, ShapeTriangle, ShapeDiamond, ShapeParallelogram, ShapeHexagon,
                 ShapeStar, ShapeCount };
constexpr int kCalloutShapes = 24;     // CalloutShape: the 24 shapes of the callout gallery
// Step numbering types (StepSequenceType)
enum StepType { StepNumbers, StepUpper, StepLower, StepRoman, StepRomanLower, StepTypeCount };
// area shapes of the Highlighter, Spotlight and Hide tools: the ShapeKind outlines plus a clicked polygon
// (CPaintOpPolygon, comp_5 §5.1) and a freehand outline; the last two keep their outline in DrawObj::pts
enum AreaShape { AreaPolygon = ShapeCount, AreaFreehand, AreaShapeCount };
inline bool IsPointArea(int areaShape) { return areaShape == AreaPolygon || areaShape == AreaFreehand; }
// magnifier shapes (MagnifyShape): the first three are the original ones
enum MagnifyKind { MagCircle, MagRect, MagRoundRect, MagHexagon, MagDiamond, MagStar, MagnifyKindCount };

// gradient fill (shape fill, canvas color and Fill tool): linear at an angle or radial, two to four color stops
struct GradStop { float pos = 0; uint32_t color = 0xFFFFFFFF; };
struct Gradient {
    int type = 0;                        // 0 none (solid color), 1 linear, 2 radial
    float angle = 90;                    // linear: direction in degrees (0 = left to right, 90 = top to bottom)
    std::vector<GradStop> stops;         // sorted by position (0..1)
    bool Active() const { return type > 0 && stops.size() >= 2; }
    bool operator==(const Gradient& o) const;
};

struct Style {
    uint32_t stroke = 0xFFE5322D;        // main color (line, outline, step fill)
    uint32_t fill = 0x00000000;          // fill (alpha 0 = none)
    uint32_t textColor = 0xFF000000;     // text: letter color
    float width = 4;                     // thickness / outline
    int opacity = 100;                   // %
    bool shadow = true;
    // shadow (ShadowPropertiesControl): direction on the 3×3 grid (4 = all around), color, opacity, blur and distance
    int shadowDir = 8;
    uint32_t shadowColor = 0xFF000000;
    int shadowOpacity = 45, shadowBlur = 6, shadowDist = 5;
    std::wstring font = L"Segoe UI";
    float fontSize = 28;
    bool bold = true, italic = false, underline = false;
    int alignH = 1, alignV = 1;          // 0 left/top, 1 center/middle, 2 right/bottom
    float padding = 5;
    int arrowStart = CapFlat, arrowEnd = CapArrow;    // ends (Cap)
    float startSize = 3, endSize = 3;    // size of the ends
    int dash = DashSolid;
    bool bezier = false;                 // curved line/arrow (handle in the middle)
    int calloutShape = 13;               // 0..23
    bool shaded = false;                 // callout with a gradient
    uint32_t tailColor = 0xFFE5322D;     // callout arrow with a line (color and thickness)
    float tailWidth = 6;
    int shapeKind = ShapeRect;
    int stepShape = 0, stepType = StepNumbers;        // step: 0 circle, 1 teardrop, 2 square, 3 rounded square
    int magnifyShape = 0, magnifyScale = 200;         // magnifier: 0 circle, 1 rectangle, 2 rounded; magnification in %
    bool magnifyLink = false;            // magnifier away from the magnified area, connected to it by a line
    int hideKind = 0;                    // hide: 0 blur, 1 pixelate
    bool antialias = true;               // eraser
    int amount = 10;                     // blur/pixelate/highlight intensity
    float lineSpacing = 1.0f;            // text and callout: line spacing multiplier, 0.8–3 (LineSpacing)
    uint32_t stepOutline = 0xFFFFFFFF;   // step: outline color and width around the shape (0 = no outline)
    float stepOutlineWidth = 0;
    int areaShape = ShapeRect;           // highlighter, spotlight and hide: ShapeKind or AreaShape
    Gradient grad;                       // shape fill / Fill tool color as a gradient
    bool bgBlur = false;                 // callout: frosted glass — blurs what is behind the body
    int bgBlurAmount = 8;
    bool loupe = false;                  // magnifier: real lens look (thick rim, reflection)
    bool loupeHandle = true;             // loupe: handle
};

struct DrawObj {
    ObjType type = ObjType::Rect;
    Gdiplus::PointF p1{}, p2{};          // line: end points; others: rectangle corners
    std::vector<Gdiplus::PointF> pts;    // pen (and old freehand highlighters)
    Gdiplus::PointF tail{};              // callout tip / center of the area magnified by the connected magnifier
    Gdiplus::PointF ctrl{};              // control point of the curved line (Bézier)
    std::wstring text;                   // text / callout / stamp (emoji or "file:path")
    int stepValue = 1;
    Style style;
    std::shared_ptr<Image> picture;      // embedded image (paste, Smart Move)
    std::vector<DrawObj> children;       // Group (CPaintOpGroup, <Group>…</Group>): the grouped objects, copied by value
    std::wstring name;                   // name given in the layers panel (empty = automatic)
    bool hidden = false;                 // hidden in the layers panel: not rendered nor exported
    bool locked = false;                 // cannot be moved, resized or deleted
    bool floating = false;               // lifted piece of the image (Lift out), merged back with "Drop piece"

    Gdiplus::RectF Rect() const;         // normalized rectangle (p1..p2)
    Gdiplus::RectF Bounds() const;       // includes stroke, ends, callout tip, shadow
    bool IsLine() const { return type == ObjType::Arrow || type == ObjType::Line; }
    bool IsPath() const { return type == ObjType::Pen || (type == ObjType::Highlighter && !pts.empty() && !IsPointArea(style.areaShape)); }
    // highlighter, spotlight or hide whose outline is the clicked polygon / freehand line in 'pts'
    bool IsPointShape() const { return IsAreaTool() && IsPointArea(style.areaShape) && pts.size() >= 2; }
    bool IsAreaTool() const { return type == ObjType::Highlighter || type == ObjType::Spotlight || type == ObjType::Blur || type == ObjType::Pixelate; }
    bool HasText() const { return type == ObjType::Text || type == ObjType::Callout; }
    bool HasLabel() const { return type == ObjType::Arrow; }   // arrow with a text label at its tail (Skitch style)
    bool IsShape() const { return type == ObjType::Shape || type == ObjType::Rect || type == ObjType::Ellipse || type == ObjType::RoundRect; }
    int Kind() const;                    // effective shape (ShapeKind), also for old Rect/Ellipse/RoundRect
    bool CalloutHasTail() const;         // callout shape with a draggable tip
    bool HitTest(Gdiplus::PointF p, float tol) const;
    void Move(float dx, float dy);
    void Scale(Gdiplus::RectF from, Gdiplus::RectF to);
    void MapPoints(const std::function<Gdiplus::PointF(Gdiplus::PointF)>& f);   // all points (rotate/resize the image)
    void UpdateGroupRect();              // group: p1..p2 = area covered by the children
};
std::wstring StepLabel(int value, int type);
float CapLength(float width, float size);          // arrowhead length for the thickness and size
using ObjPtr = std::shared_ptr<DrawObj>;

struct DocState {
    std::shared_ptr<const Image> base;
    std::vector<ObjPtr> objs;
    int nextStep = 1;
};

class Document {
public:
    std::wstring path;                   // .snap (library or external) or .mp4
    std::wstring title;
    bool isVideo = false;
    bool modified = false;
    std::wstring captureMode, windowTitle, application, created;
    std::wstring tags;                   // tags (Tag image), comma-separated
    std::vector<float> guidesX, guidesY; // ruler guides (image pixels), stored in the .snap
    DocState st;

    int w() const { return st.base ? st.base->w : 0; }
    int h() const { return st.base ? st.base->h : 0; }

    void Checkpoint();                   // pushes the current state for undo
    bool CanUndo() const { return !undo_.empty(); }
    bool CanRedo() const { return !redo_.empty(); }
    void Undo();
    void Redo();
    void UndoAll();
    void ClearHistory() { undo_.clear(); redo_.clear(); }
    void DiscardCheckpoint() { if (!undo_.empty()) undo_.pop_back(); }   // operation ended without changes
    size_t UndoDepth() const { return undo_.size(); }

    DrawObj* Mutable(size_t idx);        // clones the object (copy-on-write) and returns an editable pointer
    Image& MutableBase();                // clones the base image
    void SetBase(Image img);

    Image Render(const DrawObj* hideText = nullptr) const;   // composes the final image
    Image Flattened() const { return Render(); }

    bool Save();                         // writes the .snap to 'path'
    bool SaveAs(const std::wstring& file, int jpegQuality = 90);
    static std::unique_ptr<Document> Load(const std::wstring& file);
    static std::unique_ptr<Document> FromImage(Image img, const std::wstring& title);
private:
    std::vector<DocState> undo_, redo_;
};

// rendering (Render.cpp)
void RenderObject(Gdiplus::Graphics& g, Image& canvas, const DrawObj& o, bool hideText = false);
void ShapePath(Gdiplus::GraphicsPath& gp, int kind, Gdiplus::RectF r);                      // outline of a ShapeKind
void CalloutBodyPath(Gdiplus::GraphicsPath& gp, int shape, Gdiplus::RectF r);              // body of a callout (without the tip)
Gdiplus::RectF CalloutTextRect(const DrawObj& o);                                          // text area inside the callout
Gdiplus::RectF ArrowLabelRect(const DrawObj& o);                                           // arrow label box, beyond the tail
void AreaPath(Gdiplus::GraphicsPath& gp, const DrawObj& o);                                // outline of a highlighter/spotlight/hide area
Gdiplus::RectF MagnifierExtra(const DrawObj& o);                                           // lens rectangle plus the loupe handle
float StampAspect(const std::wstring& id);                                                 // width/height of a stamp's image
Image StampImage(const std::wstring& id, int px);                                          // stamp image (emoji, file or label)
std::unique_ptr<Gdiplus::Brush> FillBrush(uint32_t color, const Gradient& g, Gdiplus::RectF r, int opacity = 100);
void FillGradientMask(Image& img, const std::vector<uint8_t>& mask, RECT bounds, const Gradient& g, int opacity);
float TextBlockHeight(const DrawObj& o, float width);                                    // height of the text wrapped to the width
void DrawCapGlyph(Gdiplus::Graphics& g, int cap, Gdiplus::RectF r, Gdiplus::Color c, bool atStart);   // sample for menus
void ApplyDash(Gdiplus::Pen& p, int dash);
void ApplyOpacityLayer(Image& canvas, const Image& layer, int opacity);
Gdiplus::Color Gc(uint32_t argb, int opacityPercent = 100);

// XML serialization of objects (Serialize.cpp)
std::string GradStopsToString(const Gradient& g);                 // "0:#FF112233;1:#FF445566"
Gradient GradFromXml(const XmlNode& n);                           // FillGradient / GradientAngle / GradientStops
void GradToXml(XmlNode& n, const Gradient& g);
std::string ObjectsToClip(const std::vector<DrawObj>& objs, const std::wstring& source);      // copy (Editor clipboard format)
std::vector<DrawObj> ObjectsFromClip(const std::string& xml, std::wstring* source = nullptr);
// source formatting of the clipboard text (Rich Text Format or HTML): font, size (px), color, bold/italic/underline
bool ParseRtfFormat(const std::string& rtf, Style& out);
bool ParseHtmlFormat(const std::string& html, Style& out);
XmlNode ObjToXml(const DrawObj& o, SnapFrame& frame, int& imgCounter);
ObjPtr ObjFromXml(const XmlNode& n, const SnapFrame& frame);

// PDF (Pdf.cpp); OCR (RunOcr) lives in common/Ocr.h, shared with the capture
struct PdfLink { Gdiplus::RectF r; std::wstring url; };             // clickable area (image pixels) captured with the image
bool SavePdf(const std::wstring& path, const Image& img, std::wstring& err, const std::vector<PdfLink>& links = {});

} // namespace snap::ed
