// Biblioteca de capturas (DataStore, ds_arch §16 / comp_7 §5 / ed_comp_10)
// O original usa SQL CE (SnagIt900.sdf); aqui a pasta da biblioteca é o próprio índice:
//   <Library>\<GUID>.snap, <Library>\Video_*.mp4 e <Library>\Thumbnails\<nome>.png
#pragma once
#include "SnapFile.h"

namespace snap {

struct LibraryItem {
    std::wstring path;
    FILETIME     modified{};
    uint64_t     size = 0;
    bool         video = false;
};

struct CaptureInfo {
    std::wstring mode;          // CaptureMode: "Região", "Janela", "Rolagem"…
    std::wstring windowTitle;
    std::wstring application;
};

std::vector<LibraryItem> ListLibrary(size_t maxItems = 300);
std::wstring AddImageToLibrary(const Image& img, const CaptureInfo& info);   // devolve o caminho .snap
std::wstring NewVideoPath();
std::wstring ThumbPathFor(const std::wstring& itemPath);
bool  WriteThumbnail(const std::wstring& itemPath, const Image& img);
Image LoadThumbnail(const std::wstring& itemPath);
bool  DeleteLibraryItem(const std::wstring& itemPath, HWND owner);
bool  IsInLibrary(const std::wstring& path);

// Nome automático (<Prefix><AutoNum>, ed_comp_2 §7)
std::wstring NextAutoName(const std::wstring& folder, const std::wstring& prefix, const std::wstring& ext, int digits = 3);

} // namespace snap
