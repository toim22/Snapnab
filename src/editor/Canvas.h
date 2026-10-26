// Painting area (CEditorPaintControl, comp_5 §3.5): zoom, scrolling, tools, handles and raster selection
#pragma once
#include "Document.h"
#include "Cv.h"

namespace snap::ed {

// Favorites and MagicWand are toolbar items: Favorites shows the favorite styles of every tool and
// the Magic wand is the Selection tool in wand mode
enum class Tool { Select, Arrow, Line, Shape, Pen, Highlighter, Text, Callout, Step, Stamp, Blur, Spotlight,
                  Magnify, Fill, Eraser, Crop, Selection, Cutout, Eyedropper, Favorites, MagicWand, Count };
const wchar_t* ToolName(Tool t);

class Canvas {
public:
    HWND hwnd = nullptr;
    Document* doc = nullptr;
    Tool tool = Tool::Arrow;
    std::map<Tool, Style> styles;
    std::wstring stampId = L"👍";   // stamp chosen in the gallery (emoji or "file:path")
    bool pickColorOnce = false; // Fill eyedropper: the next click picks the fill color
    bool snapping = true;       // snap to canvas: the object's edges and center stick to the image's while moving
    bool smartMove = false;     // Move with Smart Move: dragging on the background selects an area and lifts it (inpainting)
    int selMode = 0;            // 0 rectangle, 1 ellipse, 2 lasso, 3 wand
    int selFill = 2;            // 0 transparent, 1 canvas color, 2 smart (inpainting)
    int cutDir = 0;             // 0 removes a horizontal band, 1 vertical
    float tolerance = 15;
    bool wandGlobal = false;
    COLORREF background = RGB(222, 224, 228);
    double zoom = 1.0;
    int selected = -1;          // selected object (the primary one, when there are several)
    bool rulers = false;        // View › Rulers: rulers, guides and cursor coordinates (off by default)
    bool edgeSnap = true;       // selection and crop edges snap to contrast lines (Alt turns it off on the fly)
    bool plainPaste = true;     // pasting text ignores the source formatting (Ctrl+Shift+V does the opposite)

    std::function<void()> onDocChanged;         // document changed (undo, thumbnail, title)
    std::function<void()> onSelectionChanged;   // selected object changed (properties panel)
    std::function<void()> onAreaChanged;        // raster selection or crop area changed (exact geometry fields)
    std::function<void(uint32_t)> onColorPicked;
    std::function<void()> onZoomChanged;
    std::function<void(bool inside, Gdiplus::PointF p)> onCursorMoved;   // cursor position in image pixels (rulers on)
    std::function<void(const std::wstring&)> onHint;                     // short message for the status bar
    std::function<void(POINT screen)> onContextMenu;                     // right click (after selecting the object below)

    void Create(HWND parent);
    void SetDocument(Document* d);
    void SetTool(Tool t);
    void Refresh(bool docChanged = true);
    Style& CurrentStyle();

    void ZoomTo(double z, POINT* anchorClient = nullptr);
    void ZoomIn() { ZoomTo(zoom * 1.25); }
    void ZoomOut() { ZoomTo(zoom / 1.25); }
    void FitToWindow();

    // commands
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
    void BringForward();                       // one step forward / backward in the order
    void SendBackward();
    void AlignSelected(int mode);              // 0 left, 1 center, 2 right, 3 top, 4 middle, 5 bottom (relative to the image)
    void DistributeSelected(bool horizontal);  // equal gaps between 3 or more objects
    // object management (layers panel, lock): indices in z-order, 0 = back
    void SetLocked(const std::vector<int>& idx, bool lock);
    void SetHidden(const std::vector<int>& idx, bool hidden);
    void RenameObject(int idx, const std::wstring& name);
    void MoveObjects(const std::vector<int>& idx, int before);   // reorder: puts the objects before 'before' (size = top)
    bool SelectionLocked() const;              // some selected object is locked
    // lifted piece: the raster selection floats as an image until it is dropped back and merged
    bool LiftSelection();
    bool DropFloating();
    bool HasFloating() const;
    // exact geometry (properties panel): 0 nothing, 1 object, 2 raster selection, 3 crop area
    int GeometryKind() const;
    Gdiplus::RectF Geometry() const;
    void SetGeometry(Gdiplus::RectF r, bool newStep);
    // paste: objects copied in the Editor (same position with inPlace) and images at a position
    void PasteObjects(const std::vector<DrawObj>& objs, bool inPlace, bool sameDoc);
    void PasteImageAt(const Image& img, Gdiplus::PointF topLeft);
    std::vector<DrawObj> SelectedObjects() const;
    void PasteText(const std::wstring& text, const Style* format);   // new text object, or into the text being edited
    bool WantsTyping() const;                  // just drew an arrow (still selected, no label): typed keys start its label
    void ResetPolygon();                       // cancels a polygon being clicked
    bool FlipSelected(bool horizontal);        // flips the selected object; false if there is no object
    void FlattenSelected();                    // flattens only the selected objects into the image
    // multiple object selection (Move: Ctrl/Shift+click or selection rectangle) and groups (CPaintOpGroup)
    std::vector<int> SelectedList() const;     // indices in ascending order (the primary one included)
    int SelectionCount() const { return (int)SelectedList().size(); }
    bool IsSelected(int i) const;
    void SelectObjects(const std::vector<int>& idx);   // the last one becomes the primary
    void SelectAllObjects();
    void GroupSelected();
    void UngroupSelected();
    bool CanUngroup() const;
    void FlattenAll();
    void ApplyStyleToSelected(const Style& s, bool newStep = true);   // newStep=false merges with the previous edit
    void SetSelectedStamp(const std::wstring& id);
    void CommitText();
    bool EditingText() const { return edit_ != nullptr; }
    Image SelectionImage() const;              // image of the raster selection (or of the object)
    RECT RasterSelectionBounds() const { return selBounds_; }
    // live preview of an effect (PreviewEffect, ed_comp_5 §4.2): shows the image in place of the document until
    // it is applied or canceled; nullptr goes back to the document
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
    void LayoutTextEdit();                    // the edit box follows the text: grows downward and follows the alignment
    void FinishCreate();
    void StartSmartMove(Gdiplus::PointF p);
    void FillHole(Image& base, const cv::Mask& m);
    void SetCursorForPoint(POINT p);
    void EraseAt(Gdiplus::PointF p);
    void EraseLine(Gdiplus::PointF from, Gdiplus::PointF to);   // dabs along the segment, so fast strokes leave no gaps
    Gdiplus::PointF eraseLast_{};
    bool eraseShift_ = false;           // Shift held during the stroke: straight line from eraseAnchor_ (axis in axisLock_)
    Gdiplus::PointF eraseAnchor_{};

    Image composed_, display_;
    std::shared_ptr<Image> fxPreview_;
    bool dirty_ = true;
    // display_ = composed_ over the checkerboard, kept while neither the image nor the zoom change;
    // while erasing, only eraseRect_ (image pixels) is refreshed in composed_ and display_
    bool composedTransparent_ = false, displayValid_ = false;
    double displayZoom_ = 0;
    RECT eraseRect_{};
    bool erasePending_ = false;
    void UpdateDisplay(const Image& frame, const RECT* only);
    // the shown image already scaled to the zoom (a DIB the size of the image on screen); while erasing only the
    // erased part is scaled again
    HBITMAP scaled_ = nullptr;
    int scaledW_ = 0, scaledH_ = 0;
    bool scaledValid_ = false;
    RECT scaledDirty_{};
    bool scaledPartial_ = false;
    void DrawComposed(HDC mem, RECT ir);
    POINT scroll_{};
    bool fit_ = true;
    // drag
    enum class Drag { None, Create, Move, Resize, Pan, Crop, Select, Lasso, Erase, Cut, Marquee, Polygon, Guide } drag_ = Drag::None;
    int axisLock_ = 0;          // Shift while moving: 0 free, 1 horizontal, 2 vertical (the axis moved along first)
    int pendingDeselect_ = -1;  // Ctrl/Shift+click on a selected object: leaves the selection if released without moving
    bool labelReady_ = false;   // an arrow was just drawn: typing starts its label (Skitch style)
    int guideAxis_ = 0, guideIdx_ = -1;    // guide being dragged: 0 vertical (X), 1 horizontal (Y)
    int linkHot_ = -1;          // link area under the cursor
    RECT View() const;          // client area minus the rulers
    int RulerSize() const;
    bool HitGuide(POINT p, int& axis, int& idx) const;
    void PaintRulers(HDC dc, Gdiplus::Graphics& g, RECT cr);
    float SnapEdge(bool vertical, float pos, float a0, float a1);    // pulls an edge to the strongest contrast line nearby
    void PolygonClick(POINT p, bool dbl);
    void FinishPolygon();
    Gdiplus::PointF Constrain45(Gdiplus::PointF from, Gdiplus::PointF to) const;
    POINT downScreen_{};
    Gdiplus::PointF downDoc_{};
    POINT panStart_{};
    std::shared_ptr<DrawObj> preview_;
    DrawObj dragOrig_;
    std::vector<std::pair<int, DrawObj>> dragMulti_;   // originals of all objects being moved
    std::vector<int> extra_;    // other selected objects besides the primary
    void BeginMove();
    int handle_ = -1;
    bool changed_ = false;
    // raster selection
    bool hasSel_ = false;
    cv::Mask selMask_;
    RECT selBounds_{};
    Gdiplus::RectF selRect_{};
    std::vector<Gdiplus::PointF> lasso_;
    Image selOverlay_;
    // crop
    bool hasCrop_ = false;
    Gdiplus::RectF crop_{};
    // text
    HWND edit_ = nullptr;
    int editIdx_ = -1;
    bool editNew_ = false;
    HFONT editFont_ = nullptr;
};

} // namespace snap::ed
