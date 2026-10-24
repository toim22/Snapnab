#include "SnapFile.h"
#include <wrl/client.h>

using Microsoft::WRL::ComPtr;

namespace snap {

// 0xCB + "SNAPNAB" + identificador de formato de 16 bytes  (≙ 0xCB + "TSCSNAG" + 16 B no original)
static const BYTE kHeader[24] = { 0xCB, 'S', 'N', 'A', 'P', 'N', 'A', 'B',
                                  0x5A, 0x3C, 0x91, 0x0E, 0x6B, 0x2D, 0x4F, 0x18,
                                  0xA7, 0x44, 0xC2, 0x9B, 0x10, 0x7E, 0xD3, 0x55 };

// ILockBytes sobre o arquivo somando o deslocamento do cabeçalho (≙ CHeaderedILockBytes)
class HeaderedLockBytes : public ILockBytes {
public:
    HeaderedLockBytes(HANDLE f, ULONGLONG off) : file_(f), off_(off) {}
    ~HeaderedLockBytes() { if (file_ != INVALID_HANDLE_VALUE) CloseHandle(file_); }
    STDMETHODIMP QueryInterface(REFIID riid, void** ppv) override {
        if (riid == IID_IUnknown || riid == IID_ILockBytes) { *ppv = this; AddRef(); return S_OK; }
        *ppv = nullptr; return E_NOINTERFACE;
    }
    STDMETHODIMP_(ULONG) AddRef() override { return ++ref_; }
    STDMETHODIMP_(ULONG) Release() override { ULONG r = --ref_; if (!r) delete this; return r; }
    STDMETHODIMP ReadAt(ULARGE_INTEGER pos, void* pv, ULONG cb, ULONG* read) override {
        LARGE_INTEGER p; p.QuadPart = pos.QuadPart + off_;
        SetFilePointerEx(file_, p, nullptr, FILE_BEGIN);
        DWORD rd = 0;
        BOOL ok = ReadFile(file_, pv, cb, &rd, nullptr);
        if (read) *read = rd;
        return ok ? S_OK : STG_E_READFAULT;
    }
    STDMETHODIMP WriteAt(ULARGE_INTEGER pos, const void* pv, ULONG cb, ULONG* written) override {
        LARGE_INTEGER p; p.QuadPart = pos.QuadPart + off_;
        SetFilePointerEx(file_, p, nullptr, FILE_BEGIN);
        DWORD wr = 0;
        BOOL ok = WriteFile(file_, pv, cb, &wr, nullptr);
        if (written) *written = wr;
        return ok ? S_OK : STG_E_WRITEFAULT;
    }
    STDMETHODIMP Flush() override { return FlushFileBuffers(file_) ? S_OK : STG_E_WRITEFAULT; }
    STDMETHODIMP SetSize(ULARGE_INTEGER cb) override {
        LARGE_INTEGER p; p.QuadPart = cb.QuadPart + off_;
        SetFilePointerEx(file_, p, nullptr, FILE_BEGIN);
        return SetEndOfFile(file_) ? S_OK : STG_E_MEDIUMFULL;
    }
    STDMETHODIMP LockRegion(ULARGE_INTEGER, ULARGE_INTEGER, DWORD) override { return STG_E_INVALIDFUNCTION; }
    STDMETHODIMP UnlockRegion(ULARGE_INTEGER, ULARGE_INTEGER, DWORD) override { return STG_E_INVALIDFUNCTION; }
    STDMETHODIMP Stat(STATSTG* st, DWORD) override {
        memset(st, 0, sizeof(*st));
        st->type = STGTY_LOCKBYTES;
        LARGE_INTEGER sz; GetFileSizeEx(file_, &sz);
        st->cbSize.QuadPart = sz.QuadPart > (LONGLONG)off_ ? sz.QuadPart - off_ : 0;
        return S_OK;
    }
private:
    HANDLE file_;
    ULONGLONG off_;
    ULONG ref_ = 1;
};

static bool WriteStream(IStorage* stg, const wchar_t* name, const void* data, size_t len) {
    ComPtr<IStream> st;
    if (FAILED(stg->CreateStream(name, STGM_CREATE | STGM_WRITE | STGM_SHARE_EXCLUSIVE, 0, 0, &st))) return false;
    ULONG wr = 0;
    return SUCCEEDED(st->Write(data, (ULONG)len, &wr)) && wr == len;
}

static bool ReadStream(IStorage* stg, const wchar_t* name, std::string& out) {
    ComPtr<IStream> st;
    if (FAILED(stg->OpenStream(name, nullptr, STGM_READ | STGM_SHARE_EXCLUSIVE, 0, &st))) return false;
    STATSTG ss{}; st->Stat(&ss, STATFLAG_NONAME);
    out.resize((size_t)ss.cbSize.QuadPart);
    ULONG rd = 0;
    return SUCCEEDED(st->Read(out.data(), (ULONG)out.size(), &rd)) && rd == out.size();
}

static bool WritePng(IStorage* stg, const std::string& name, const Image& img) {
    std::vector<BYTE> png;
    if (!EncodeToMemory(img, ImageFormat::PNG, png)) return false;
    return WriteStream(stg, Utf8ToW(name).c_str(), png.data(), png.size());
}

static bool ReadPng(IStorage* stg, const std::string& name, Image& img) {
    std::string data;
    if (!ReadStream(stg, Utf8ToW(name).c_str(), data)) return false;
    return DecodeFromMemory((const BYTE*)data.data(), data.size(), img);
}

bool SaveSnapFile(const std::wstring& path, const std::vector<SnapFrame>& frames) {
    std::wstring tmp = path + L".saving";
    HANDLE f = CreateFileW(tmp.c_str(), GENERIC_READ | GENERIC_WRITE, 0, nullptr, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (f == INVALID_HANDLE_VALUE) { Log(L"SaveSnapFile: não abriu %s", tmp.c_str()); return false; }
    DWORD wr; WriteFile(f, kHeader, sizeof kHeader, &wr, nullptr);
    bool ok = false;
    {
        auto* lb = new HeaderedLockBytes(f, sizeof kHeader);
        ComPtr<IStorage> root;
        if (SUCCEEDED(StgCreateDocfileOnILockBytes(lb, STGM_CREATE | STGM_READWRITE | STGM_SHARE_EXCLUSIVE, 0, &root))) {
            XmlNode index("Index");
            XmlNode& list = index.add("FrameList");
            index.set("SoftwareVersion", WToUtf8(kAppVersion));
            ok = true;
            for (size_t i = 0; i < frames.size() && ok; i++) {
                std::wstring fname = Format(L"Frame%zu", i);
                list.add("Frame").setW("Name", fname);
                ComPtr<IStorage> fs, is;
                ok = SUCCEEDED(root->CreateStorage(fname.c_str(), STGM_CREATE | STGM_READWRITE | STGM_SHARE_EXCLUSIVE, 0, 0, &fs)) &&
                     SUCCEEDED(fs->CreateStorage(L"ImageStorage", STGM_CREATE | STGM_READWRITE | STGM_SHARE_EXCLUSIVE, 0, 0, &is));
                if (!ok) break;
                XmlNode doc = frames[i].doc;
                doc.set("BaseImage", "Image_0.png");
                doc.seti("Width", frames[i].base.w).seti("Height", frames[i].base.h);
                std::string xml = XmlSerialize(doc);
                ok = WriteStream(fs.Get(), L"Document", xml.data(), xml.size()) && WritePng(is.Get(), "Image_0.png", frames[i].base);
                for (auto& e : frames[i].embedded) if (ok) ok = WritePng(is.Get(), e.first, e.second);
                if (ok) ok = SUCCEEDED(is->Commit(STGC_DEFAULT)) && SUCCEEDED(fs->Commit(STGC_DEFAULT));
            }
            if (ok) {
                std::string xml = XmlSerialize(index);
                ok = WriteStream(root.Get(), L"Index", xml.data(), xml.size()) && SUCCEEDED(root->Commit(STGC_DEFAULT));
            }
        }
        lb->Release();          // fecha o arquivo quando o IStorage também soltar
    }
    if (!ok) { DeleteFileW(tmp.c_str()); Log(L"SaveSnapFile falhou: %s", path.c_str()); return false; }
    return MoveFileExW(tmp.c_str(), path.c_str(), MOVEFILE_REPLACE_EXISTING) != 0;
}

bool IsSnapFile(const std::wstring& path) {
    HANDLE f = CreateFileW(path.c_str(), GENERIC_READ, FILE_SHARE_READ, nullptr, OPEN_EXISTING, 0, nullptr);
    if (f == INVALID_HANDLE_VALUE) return false;
    BYTE h[24]{}; DWORD rd = 0;
    ReadFile(f, h, 24, &rd, nullptr);
    CloseHandle(f);
    return rd == 24 && memcmp(h, kHeader, 24) == 0;
}

bool LoadSnapFile(const std::wstring& path, std::vector<SnapFrame>& frames) {
    if (!IsSnapFile(path)) return false;
    HANDLE f = CreateFileW(path.c_str(), GENERIC_READ, FILE_SHARE_READ, nullptr, OPEN_EXISTING, 0, nullptr);
    if (f == INVALID_HANDLE_VALUE) return false;
    auto* lb = new HeaderedLockBytes(f, sizeof kHeader);
    bool ok = false;
    {
        ComPtr<IStorage> root;
        if (SUCCEEDED(StgOpenStorageOnILockBytes(lb, nullptr, STGM_READ | STGM_SHARE_EXCLUSIVE, nullptr, 0, &root))) {
            std::string idx;
            XmlNode index;
            if (ReadStream(root.Get(), L"Index", idx) && XmlParse(idx, index)) {
                frames.clear();
                ok = true;
                if (const XmlNode* list = index.child("FrameList")) {
                    for (auto& fr : list->children) {
                        std::wstring name = fr.getW("Name");
                        ComPtr<IStorage> fs, is;
                        if (FAILED(root->OpenStorage(name.c_str(), nullptr, STGM_READ | STGM_SHARE_EXCLUSIVE, nullptr, 0, &fs))) { ok = false; break; }
                        fs->OpenStorage(L"ImageStorage", nullptr, STGM_READ | STGM_SHARE_EXCLUSIVE, nullptr, 0, &is);
                        SnapFrame frame;
                        std::string xml;
                        if (ReadStream(fs.Get(), L"Document", xml)) XmlParse(xml, frame.doc);
                        if (is) {
                            ReadPng(is.Get(), frame.doc.get("BaseImage", "Image_0.png"), frame.base);
                            // imagens embutidas referenciadas por StreamName em qualquer nível
                            std::function<void(const XmlNode&)> walk = [&](const XmlNode& n) {
                                std::string sn = n.get("StreamName");
                                if (!sn.empty() && !frame.embedded.count(sn)) {
                                    Image im; if (ReadPng(is.Get(), sn, im)) frame.embedded[sn] = std::move(im);
                                }
                                for (auto& c : n.children) walk(c);
                            };
                            walk(frame.doc);
                        }
                        frames.push_back(std::move(frame));
                    }
                }
                ok = ok && !frames.empty() && !frames[0].base.empty();
            }
        }
    }
    lb->Release();
    if (!ok) Log(L"LoadSnapFile falhou: %s", path.c_str());
    return ok;
}

} // namespace snap
