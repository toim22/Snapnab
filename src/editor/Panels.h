// Painéis do Editor: barra de ferramentas, propriedades (ToolProperties), bandeja da biblioteca (Tray)
#pragma once
#include "Canvas.h"
#include "Icons.h"
#include "Library.h"

namespace snap::ed {

// ---- barra de ferramentas ----
class Toolbar {
public:
    struct Item { int id; icons::Id icon; std::wstring label; std::wstring hint; bool tool; Tool t; bool right; };
    HWND hwnd = nullptr;
    std::vector<Item> items;
    Tool active = Tool::Arrow;
    std::function<void(int id)> onClick;
    std::function<void(const std::wstring&)> onHint;
    void Create(HWND parent);
    void SetActive(Tool t) { active = t; InvalidateRect(hwnd, nullptr, FALSE); }
    int Height() const;
private:
    static LRESULT CALLBACK Proc(HWND, UINT, WPARAM, LPARAM);
    void Paint(HDC dc);
    void Layout();
    int Hit(POINT p) const;
    std::vector<RECT> rects_;
    int hover_ = -1, pressed_ = -1;
    bool compact_ = false;
};

// ---- propriedades ----
class PropsPanel {
public:
    HWND hwnd = nullptr;
    Canvas* canvas = nullptr;
    std::function<void()> onAction;               // botões de ação (cortar, aplicar…) → main
    std::function<void(int)> onCommand;
    void Create(HWND parent, Canvas* c);
    void Load();                                  // canvas → controles
    void SetDocInfo(const std::wstring& info) { docInfo_ = info; InvalidateRect(hwnd, nullptr, FALSE); }
private:
    static LRESULT CALLBACK Proc(HWND, UINT, WPARAM, LPARAM);
    LRESULT Handle(UINT m, WPARAM w, LPARAM l);
    void Save(int changedId);                     // controles → estilo
    void Layout();
    Tool ShownTool() const;
    Style ShownStyle() const;
    HWND Ctl(int id) { return ctl_[id]; }
    std::map<int, HWND> ctl_, lbl_;
    std::vector<int> order_;
    bool loading_ = false;
    std::wstring docInfo_;
    int contentBottom_ = 0;
    Style cur_;
};

// ---- bandeja da biblioteca (CTrayThread / Tray) ----
class LibraryTray {
public:
    HWND hwnd = nullptr;
    std::function<void(const std::wstring&)> onOpen;
    std::function<void(const std::wstring&)> onDelete;
    std::wstring current;
    void Create(HWND parent);
    void Reload();
    void SetCurrent(const std::wstring& p) { current = p; EnsureVisible(); InvalidateRect(hwnd, nullptr, FALSE); }
    void InvalidateThumb(const std::wstring& p);
    int Height() const;
private:
    static LRESULT CALLBACK Proc(HWND, UINT, WPARAM, LPARAM);
    LRESULT Handle(UINT m, WPARAM w, LPARAM l);
    void Paint(HDC dc);
    int Hit(POINT p) const;
    void EnsureVisible();
    const Image& Thumb(const std::wstring& path, bool video);
    std::vector<LibraryItem> items_;
    std::map<std::wstring, Image> thumbs_;
    int scroll_ = 0, hover_ = -1;
};

} // namespace snap::ed
