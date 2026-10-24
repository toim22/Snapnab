#include "Library.h"

namespace snap {

std::vector<LibraryItem> ListLibrary(size_t maxItems) {
    std::vector<LibraryItem> items;
    std::wstring dir = LibraryDir();
    WIN32_FIND_DATAW fd;
    HANDLE h = FindFirstFileW((dir + L"\\*").c_str(), &fd);
    if (h == INVALID_HANDLE_VALUE) return items;
    do {
        if (fd.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) continue;
        std::wstring ext = FileExt(fd.cFileName);
        if (ext != L".snap" && ext != L".mp4") continue;
        LibraryItem it;
        it.path = dir + L"\\" + fd.cFileName;
        it.modified = fd.ftLastWriteTime;
        it.size = ((uint64_t)fd.nFileSizeHigh << 32) | fd.nFileSizeLow;
        it.video = ext == L".mp4";
        items.push_back(it);
    } while (FindNextFileW(h, &fd));
    FindClose(h);
    std::sort(items.begin(), items.end(), [](const LibraryItem& a, const LibraryItem& b) {
        return CompareFileTime(&a.modified, &b.modified) > 0;
    });
    if (items.size() > maxItems) items.resize(maxItems);
    return items;
}

std::wstring ThumbPathFor(const std::wstring& itemPath) {
    return ThumbDir() + L"\\" + FileNameOf(itemPath) + L".png";
}

bool WriteThumbnail(const std::wstring& itemPath, const Image& img) {
    Image t = MakeThumbnail(img, 160, 120);
    return !t.empty() && SaveImageFile(ThumbPathFor(itemPath), t);
}

Image LoadThumbnail(const std::wstring& itemPath) {
    Image t;
    std::wstring tp = ThumbPathFor(itemPath);
    if (FileExists(tp)) LoadImageFile(tp, t);
    return t;
}

std::wstring AddImageToLibrary(const Image& img, const CaptureInfo& info) {
    std::wstring path = LibraryDir() + L"\\" + NewGuidString() + L".snap";
    SnapFrame f;
    f.base = img;
    f.doc.setW("CaptureMode", info.mode);
    f.doc.setW("WindowTitle", info.windowTitle);
    f.doc.setW("Application", info.application);
    f.doc.setW("Created", TimeStamp(L"%04d-%02d-%02dT%02d:%02d:%02d"));
    if (!SaveSnapFile(path, { f })) return L"";
    WriteThumbnail(path, img);
    Log(L"Biblioteca: adicionado %s (%dx%d, %s)", path.c_str(), img.w, img.h, info.mode.c_str());
    return path;
}

std::wstring NewVideoPath() {
    return LibraryDir() + L"\\Video_" + TimeStamp() + L".mp4";
}

bool DeleteLibraryItem(const std::wstring& itemPath, HWND owner) {
    std::wstring from = itemPath; from.push_back(L'\0');
    SHFILEOPSTRUCTW op{};
    op.hwnd = owner; op.wFunc = FO_DELETE; op.pFrom = from.c_str();
    op.fFlags = FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT;      // vai para a Lixeira
    bool ok = SHFileOperationW(&op) == 0;
    DeleteFileW(ThumbPathFor(itemPath).c_str());
    return ok;
}

bool IsInLibrary(const std::wstring& path) {
    std::wstring lib = ToLower(LibraryDir());
    return ToLower(DirOf(path)) == lib;
}

std::wstring NextAutoName(const std::wstring& folder, const std::wstring& prefix, const std::wstring& ext, int digits) {
    for (int n = 1; n < 1000000; n++) {
        std::wstring p = folder + L"\\" + prefix + Format(L"%0*d", digits, n) + ext;
        if (!FileExists(p)) return p;
    }
    return folder + L"\\" + prefix + NewGuidString() + ext;
}

} // namespace snap
