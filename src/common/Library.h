// Capture library (DataStore, ds_arch §16 / comp_7 §5 / ed_comp_10)
// The original uses SQL CE (SnagIt900.sdf); here the library folder itself is the index:
//   <Library>\<GUID>.snap, <Library>\Video_*.mp4 and <Library>\Thumbnails\<name>.png
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
    std::wstring mode;          // CaptureMode: "Região", "Janela", "Rolagem"… (stored values, kept in Portuguese)
    std::wstring windowTitle;
    std::wstring application;
};

std::vector<LibraryItem> ListLibrary(size_t maxItems = 300);
std::wstring AddImageToLibrary(const Image& img, const CaptureInfo& info);   // returns the .snap path
// capture with vector objects (each window as an image, captured links): 'objects' is the <Objects> element as the
// Editor writes it and 'embedded' its images (StreamName); 'flat' is the composed image for the thumbnail
std::wstring AddLayeredToLibrary(const Image& base, const Image& flat, const CaptureInfo& info, const XmlNode& objects,
                                 const std::map<std::string, Image>& embedded);
std::wstring NewVideoPath();
std::wstring ThumbPathFor(const std::wstring& itemPath);
bool  WriteThumbnail(const std::wstring& itemPath, const Image& img);
Image LoadThumbnail(const std::wstring& itemPath);
bool  DeleteLibraryItem(const std::wstring& itemPath, HWND owner);
bool  IsInLibrary(const std::wstring& path);

// names given to captures (shown in the tray and library, searchable): <Library>\Names.xml, by file name
std::wstring CaptureName(const std::wstring& itemPath);
bool SetCaptureName(const std::wstring& itemPath, const std::wstring& name);   // empty removes the name

// Automatic name (<Prefix><AutoNum>, ed_comp_2 §7)
std::wstring NextAutoName(const std::wstring& folder, const std::wstring& prefix, const std::wstring& ext, int digits = 3);

} // namespace snap
