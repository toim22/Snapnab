// Editor panels: toolbar, properties (ToolProperties), effects and tray/library (Tray, CLibraryView).
// They all follow the capture window colors (common/Theme): backgrounds tinted toward the Windows accent color.
#pragma once
#include "Canvas.h"
#include "Icons.h"
#include "Library.h"
#include "Theme.h"
#include "Stamps.h"
#include <set>

namespace snap::ed {

const ui::ThemeColors& EdTheme();
void SetEdTheme(const ui::ThemeColors& t);

// scroll bar drawn by the panel itself: dragging the thumb scrolls, clicking the track moves one page
struct LiteScroll {
    RECT track{};                 // in window coordinates
    int content = 0, view = 0, pos = 0;
    bool dragging = false, hot = false;
    int grabY = 0, grabPos = 0;
    bool Visible() const { return view > 0 && content > view; }
    int MaxPos() const { return std::max(0, content - view); }
    RECT Thumb() const;
    bool Hit(POINT p) const;
    bool Down(POINT p);           // true if the click was on the bar (pos may change)
    bool Move(POINT p);           // while dragging; true if pos changed
    void Up() { dragging = false; }
    void Paint(Gdiplus::Graphics& g) const;
};

// ---- toolbar (TopPanel.ToolBar, ed_comp_3 §3.2) ----
// Items of the ToolBarItemType kinds: tool, effect, output, command and spacer; at the end, "More/Edit" (EditMore)
// with the tools that are not on the bar and the customization. Fixed actions at the ends: Capture/Record and
// Copy all/Save as/Share. Dragging an item moves it elsewhere; while customizing,
// dragging it off the bar removes it (like the BCG toolbars).
struct ToolbarEntry {
    enum Type { Tool, Effect, Output, Command, Spacer } type = Tool;
    int id = 0;                                   // tool (Tool) or menu command
    bool operator==(const ToolbarEntry& o) const { return type == o.type && id == o.id; }
};
struct ToolbarEntryInfo { icons::Id icon = icons::Gear; std::wstring label, hint; };

class Toolbar {
public:
    struct Item { int id; icons::Id icon; std::wstring label; std::wstring hint; bool right; bool primary; };
    HWND hwnd = nullptr;
    std::vector<Item> items;                      // fixed actions
    std::vector<ToolbarEntry> center;             // customizable items, in order
    Tool active = Tool::Arrow;
    bool customizing = false;                     // Customize window open: a click does not run the item, dragging it off removes it
    std::function<ToolbarEntryInfo(const ToolbarEntry&)> describe;
    std::function<void(int id)> onClick;
    std::function<void(const ToolbarEntry&)> onEntry;
    std::function<void(POINT screen)> onMore;     // "More" button: point where the menu opens
    std::function<void(std::vector<ToolbarEntry>)> onReorder;
    std::function<void(const std::wstring&)> onHint;
    void Create(HWND parent);
    void SetActive(Tool t) { active = t; InvalidateRect(hwnd, nullptr, FALSE); }
    void SetCenter(std::vector<ToolbarEntry> v) { center = std::move(v); Layout(); InvalidateRect(hwnd, nullptr, FALSE); }
    int Height() const;
    int DropIndex(POINT screen) const;            // insertion position under the point (-1 = off the bar)
    void ShowDropCaret(int index) { if (index != caret_) { caret_ = index; InvalidateRect(hwnd, nullptr, FALSE); } }
    void InsertAt(int index, const ToolbarEntry& e);   // an item already on the bar (other than a spacer) changes place
private:
    struct Slot { RECT r; int kind; int index; };  // kind: 0 action, 1 customizable item, 2 "More"
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

// "Customize toolbar" window (CBCGPToolbarCustomize): grid with the tools, commands, outputs
// and effects, to drag onto the bar. Done keeps the changes; Cancel returns false (the caller restores the bar).
// toolCount: how many tools the automatic bar shows (onToolCount reports the change, which turns the bar back to automatic)
bool CustomizeToolbarDialog(HWND owner, Toolbar& bar, const std::vector<std::pair<std::wstring, std::vector<ToolbarEntry>>>& catalog,
                            std::function<ToolbarEntryInfo(const ToolbarEntry&)> describe, std::function<std::vector<ToolbarEntry>()> defaults,
                            int toolCount, std::function<void(int)> onToolCount);

// ---- properties (CDotNetPaintProperties / ToolProperties.*, ed_comp_3 §2.3, ed_comp_4 §2) ----
// At the top, the tool's quick styles (theme, gallery, favorites with the star, "+" adds the current style);
// for the Stamp tool, the stamp gallery with search and categories. Below, the tool properties:
// colors, shadow (direction on the 3×3 grid), shape, ends and dash pattern, widths with a slider, advanced text.
class PropsPanel {
public:
    HWND hwnd = nullptr;
    Canvas* canvas = nullptr;
    Tool barTool = Tool::Arrow;                   // tool chosen on the bar (includes Favorites and Wand)
    std::function<void(int)> onCommand;           // action buttons (324..329)
    std::function<void(Tool)> onPickTool;         // favorite chosen: changes the canvas tool
    std::function<void()> onStylesChanged;        // some tool's style changed (remembered between sessions)
    std::function<void(const std::wstring&)> onHint;
    void Create(HWND parent, Canvas* c);
    void SetBarTool(Tool t);
    void Load();                                  // canvas → controls
    void AreaChanged();                           // raster selection / crop area changed: X/Y/W/H fields
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
    Tool PropTool() const;                        // tool whose properties are shown
    Tool StyleTool() const;                       // tool of the quick styles (Favorites = favorites)
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
    void UpdateGeometryFields();
    HWND Child(int id) { return children_[id]; }
    HWND EditFor(int ctlId);
    std::vector<Ctl> ctls_;
    std::vector<Tile> tiles_;
    std::vector<Style> tileStyles_;               // styles of the current gallery (or of the favorites)
    std::vector<std::wstring> stampIds_;
    std::map<int, HWND> children_, edits_;
    std::map<std::wstring, Image> thumbs_;
    RECT qsHeader_{}, addBtn_{}, gridRect_{}, tpHeader_{}, helpBtn_{};
    int gridScroll_ = 0, gridContent_ = 0, scroll_ = 0, content_ = 0;
    int hoverTile_ = -1, hoverCtl_ = -1, dragCtl_ = -1;
    bool hoverStar_ = false, hoverAdd_ = false, advanced_ = true, loading_ = false, dragFirst_ = false;
    int lastEdit_ = -1;
    int stampCat_ = 1;
    bool geomShown_ = false, geomFirst_ = true, aspectLock_ = false;   // X/Y/W/H fields; aspect lock
    int stampPress_ = -1, stampDropAt_ = -1;      // stamp tile pressed / drop position while dragging to reorder
    POINT stampPressPt_{};
    bool stampDragging_ = false;
    std::vector<StampCategory> stampCats_;        // categories shown (refreshed when the order or the pins change)
    float geomAspect_ = 1;
    LiteScroll gridSb_, panelSb_;                 // scrolling of the gallery and of the whole panel
    std::wstring docInfo_, stampSearch_;
    HBRUSH bg_ = nullptr;
};

// ---- layers (F03, Photoshop-like): every object top to bottom in z-order ----
// Eye (hide/show: hidden objects are not rendered nor exported), lock, thumbnail and name of each object; groups
// expand to show their children; click/Ctrl/Shift select (the same selection as the canvas), dragging reorders,
// double click or F2 renames. The image itself is the "Background" row at the bottom.
class LayersPanel {
public:
    HWND hwnd = nullptr;
    Canvas* canvas = nullptr;
    std::function<void(const std::wstring&)> onHint;
    std::function<void(POINT screen)> onContextMenu;  // right click on a row (after selecting it)
    void Create(HWND parent, Canvas* c);
    void Refresh();                               // document or selection changed
    static std::wstring DisplayName(const DrawObj& o);   // given name or an automatic one (type, text…)
private:
    struct Row { int obj = -1; int child = -1; int depth = 0; RECT r{}; bool background = false; };
    static LRESULT CALLBACK Proc(HWND, UINT, WPARAM, LPARAM);
    LRESULT Handle(UINT m, WPARAM w, LPARAM l);
    void Layout();
    void Paint(HDC dc);
    int HitRow(POINT p) const;
    int DropBefore(POINT p) const;                // z-order index where dragged rows go (objs.size() = on top)
    void BeginRename(int row);
    void EndRename(bool commit);
    const Image& Thumb(const ObjPtr& o, int px);
    std::vector<Row> rows_;
    std::set<const DrawObj*> expanded_;
    std::map<const DrawObj*, std::pair<std::weak_ptr<DrawObj>, Image>> thumbs_;
    int scroll_ = 0, content_ = 0, hover_ = -1, press_ = -1, anchor_ = -1, dropAt_ = -1;
    POINT pressPt_{};
    bool dragging_ = false;
    HWND edit_ = nullptr;
    int editObj_ = -1;
    LiteScroll sb_;
};

// ---- effects (SidePanel.Effects / EffectsPanelControls, ed_comp_5 §4) ----
// At the top, the quick effects gallery (QuickEffectGalleryItemVm); below, each effect with a preview on the current image.
// Clicking an effect opens its settings inside the panel: each change shows the live preview on the image
// (PreviewEffect) and "Apply" writes the effect (ApplyEffect); "Cancel" goes back to the document.
struct FxParam {
    enum Kind { Choice, Toggle, Color, Text } kind = Choice;
    std::wstring label;
    std::vector<std::wstring> options;            // Choice: labels
    std::vector<int> values;                      // Choice: value of each option
    int value = 0;                                // Choice: chosen value; Toggle: 0/1
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
    std::vector<std::wstring> quick;              // names of the quick effects
    // thumbnail preview: cmd >= 0 is an effect (with the current settings), cmd < 0 is quick effect -cmd-1
    std::function<Image(int cmd, const std::vector<FxParam>& params, const Image& src)> makePreview;
    std::function<void(int cmd, const std::vector<FxParam>& params)> onPreview;   // live preview on the image
    std::function<void(int cmd, const std::vector<FxParam>& params)> onApply;
    std::function<void()> onCancel;
    std::function<void(int index)> onQuick;
    std::function<void()> onAddQuick;
    std::function<void(int index)> onDeleteQuick;
    void Create(HWND parent);
    void SetSource(const Image& img);             // current image (empty = sample) → rebuilds the previews
    void SetQuick(std::vector<std::wstring> names);
    void Expand(int cmd);                         // opens the settings of an effect (also from the toolbar button)
    void Collapse();                              // closes without applying (notifies onCancel)
    bool IsOpen() const { return open_ >= 0; }
    const Effect* Find(int cmd) const;
private:
    static LRESULT CALLBACK Proc(HWND, UINT, WPARAM, LPARAM);
    LRESULT Handle(UINT m, WPARAM w, LPARAM l);
    void Paint(HDC dc);
    void Relayout();                              // computes the areas and positions the controls of the open effect
    void BuildPreviews();
    void CreateControls();
    void DestroyControls();
    void ParamChanged(int i);
    int HitRow(POINT p) const;
    int HitTile(POINT p) const;                   // quick effect index; quick.size() = "Add"
    int SettingsHeight() const;
    Image src_;
    std::vector<Image> previews_, quickPreviews_;
    std::vector<RECT> rows_, tiles_;              // in content coordinates (without scrolling)
    std::vector<std::pair<int, std::wstring>> heads_;
    RECT settings_{};
    int contentH_ = 0, scroll_ = 0, hover_ = -1, hoverTile_ = -1, open_ = -1;
    std::vector<HWND> ctls_;
    HWND apply_ = nullptr, cancel_ = nullptr;
    HBRUSH bg_ = nullptr;
};

// ---- recent captures tray (Tray) and library view (CLibraryView) ----
// Tray: the recent captures (RecentTrayDocs, default 10), stored in Tray.bin; new captures and opened
// documents go to the front. Library: all captures in a grid. In both: Ctrl/Shift+click selects several,
// Ctrl+A selects all, Del or the thumbnail's X deletes, several selected offer export/delete, and items can be
// dragged to other programs (TrayDragDrop) and files can be dropped from Explorer.
class LibraryTray {
public:
    enum Mode { Tray, Library };
    HWND hwnd = nullptr;
    Mode mode = Tray;
    std::function<void(const std::wstring&)> onOpen;
    std::function<void(const std::vector<std::wstring>&)> onDelete;
    std::function<void(const std::vector<std::wstring>&)> onExport;
    std::function<void(const std::vector<std::wstring>&)> onDropFiles;        // files dropped from Explorer
    std::function<std::vector<std::wstring>(const std::vector<std::wstring>&)> filesForDrag;   // items → files to drag
    std::function<void()> onShowLibrary;          // tray "Library" button / library "Back" button
    std::function<void(const std::wstring& path, const std::wstring& name)> onRenamed;   // capture renamed (F2 / context menu)
    std::wstring current;
    void Create(HWND parent, Mode m);
    void Reload();
    void SetCurrent(const std::wstring& p);       // active document (in the tray, it also goes into the recents)
    void InvalidateThumb(const std::wstring& p);
    void SelectAll();
    void RemoveFromTray(const std::vector<std::wstring>& paths);
    void Rename(const std::wstring& path);        // asks for the capture's name
    static std::wstring ItemLabel(const LibraryItem& it);   // name, or the date and time
    int Height() const;
    static bool DraggingOut() { return s_draggingOut; }   // a drag leaving the tray/library is in progress
private:
    static bool s_draggingOut;
    static LRESULT CALLBACK Proc(HWND, UINT, WPARAM, LPARAM);
    LRESULT Handle(UINT m, WPARAM w, LPARAM l);
    void Paint(HDC dc);
    int Hit(POINT p) const;
    RECT CellRect(int i) const;                   // in window coordinates (with scrolling)
    RECT CloseRect(int i) const;                  // X on the thumbnail corner
    RECT HeaderButton(int which) const;           // 0 export, 1 delete, 2 clear selection, 3 library/back
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
    std::vector<std::wstring> recent_;            // tray: paths in order (Tray.bin)
    std::vector<std::wstring> hidden_;            // removed from the tray by the user (they do not come back when filling free slots)
    std::vector<std::wstring> known_;             // library items already seen (new ones go into the tray)
    mutable std::wstring savedRecent_;            // content last written to Tray.bin
    std::map<std::wstring, Image> thumbs_;
    std::vector<bool> sel_;
    int anchor_ = -1, downItem_ = -1;
    POINT downPt_{};
    int scroll_ = 0, hover_ = -1, hoverClose_ = -1, hoverHeader_ = -1;
    // rectangle selection (as in Explorer): starts on empty space and selects the thumbnails it touches
    bool band_ = false;
    POINT bandStart_{}, bandEnd_{};               // in content coordinates (without scrolling)
    std::vector<bool> bandBase_;                  // previous selection (Ctrl/Shift add to it)
    RECT BandRect() const;                        // in window coordinates
    void UpdateBand(POINT p);
    // search by name (library) and larger preview when resting the mouse on a thumbnail
    HWND search_ = nullptr;
    std::wstring query_;
    HWND preview_ = nullptr;
    int previewItem_ = -1;
    struct PreviewData { Image img; std::wstring title, info; FILETIME stamp{}; };
    std::map<std::wstring, PreviewData> previews_;
    void ShowPreview(int item);
    void HidePreview();
    static LRESULT CALLBACK PreviewProc(HWND, UINT, WPARAM, LPARAM);
};

} // namespace snap::ed
