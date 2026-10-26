#include "Settings.h"
#include "Xml.h"

namespace snap::settings {

namespace {
// settings files of the data folder (the library, logs and the tray list are not settings)
const wchar_t* const kFiles[] = { L"Presets1.xml", L"ToolThemes.xml", L"Favorites.xml", L"ToolStyles.xml", L"QuickEffects.xml",
                                  L"StampOrder.xml" };

std::string ReadAll(const std::wstring& path) {
    std::string out;
    HANDLE h = CreateFileW(path.c_str(), GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE, nullptr, OPEN_EXISTING, 0, nullptr);
    if (h == INVALID_HANDLE_VALUE) return out;
    LARGE_INTEGER sz{};
    if (GetFileSizeEx(h, &sz) && sz.QuadPart < 64 * 1024 * 1024) {
        out.resize((size_t)sz.QuadPart);
        DWORD rd = 0;
        if (!out.empty() && !ReadFile(h, out.data(), (DWORD)out.size(), &rd, nullptr)) out.clear();
        out.resize(rd);
    }
    CloseHandle(h);
    return out;
}

bool WriteAll(const std::wstring& path, const std::string& data) {
    HANDLE h = CreateFileW(path.c_str(), GENERIC_WRITE, 0, nullptr, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (h == INVALID_HANDLE_VALUE) return false;
    DWORD wr = 0;
    BOOL ok = data.empty() || WriteFile(h, data.data(), (DWORD)data.size(), &wr, nullptr);
    CloseHandle(h);
    return ok && wr == data.size();
}

// registry: every value of the key and its subkeys (relative path "" = the root)
void ExportKey(HKEY root, const std::wstring& sub, XmlNode& out) {
    HKEY k;
    std::wstring full = std::wstring(kRegRoot) + (sub.empty() ? L"" : L"\\" + sub);
    if (RegOpenKeyExW(root, full.c_str(), 0, KEY_READ, &k) != ERROR_SUCCESS) return;
    XmlNode& kn = out.add("Key");
    kn.setW("Path", sub);
    for (DWORD i = 0;; i++) {
        wchar_t name[16384]; DWORD nl = 16384, type = 0, cb = 0;
        LONG r = RegEnumValueW(k, i, name, &nl, nullptr, &type, nullptr, &cb);
        if (r == ERROR_NO_MORE_ITEMS) break;
        if (r != ERROR_SUCCESS) continue;
        std::vector<BYTE> data(cb + 2);
        nl = 16384;
        if (RegEnumValueW(k, i, name, &nl, nullptr, &type, data.data(), &cb) != ERROR_SUCCESS) continue;
        data.resize(cb);
        XmlNode& v = kn.add("Value");
        v.setW("Name", name);
        if (type == REG_DWORD && cb == 4) v.set("Type", "dword").seti("Data", *(DWORD*)data.data());
        else if ((type == REG_SZ || type == REG_EXPAND_SZ) && cb >= 2) v.set("Type", "string").setW("Data", std::wstring((const wchar_t*)data.data(), wcsnlen((const wchar_t*)data.data(), cb / 2)));
        else v.seti("RawType", type).set("Type", "binary").set("Data", Base64Encode(data));
    }
    std::vector<std::wstring> subs;
    for (DWORD i = 0;; i++) {
        wchar_t name[256]; DWORD nl = 256;
        if (RegEnumKeyExW(k, i, name, &nl, nullptr, nullptr, nullptr, nullptr) != ERROR_SUCCESS) break;
        subs.push_back(name);
    }
    RegCloseKey(k);
    for (auto& s : subs) ExportKey(root, sub.empty() ? s : sub + L"\\" + s, out);
}

bool ValueExists(const std::wstring& sub, const std::wstring& name) {
    std::wstring full = std::wstring(kRegRoot) + (sub.empty() ? L"" : L"\\" + sub);
    return RegGetValueW(HKEY_CURRENT_USER, full.c_str(), name.c_str(), RRF_RT_ANY, nullptr, nullptr, nullptr) == ERROR_SUCCESS;
}

void ImportValue(const std::wstring& sub, const XmlNode& v, bool merge) {
    std::wstring name = v.getW("Name"), full = std::wstring(kRegRoot) + (sub.empty() ? L"" : L"\\" + sub);
    if (merge && ValueExists(sub, name)) return;           // merge: the current setting stays
    std::string type = v.get("Type");
    if (type == "dword") { DWORD d = (DWORD)std::stoul(v.get("Data", "0")); RegSetKeyValueW(HKEY_CURRENT_USER, full.c_str(), name.c_str(), REG_DWORD, &d, 4); }
    else if (type == "string") { std::wstring s = v.getW("Data"); RegSetKeyValueW(HKEY_CURRENT_USER, full.c_str(), name.c_str(), REG_SZ, s.c_str(), (DWORD)((s.size() + 1) * 2)); }
    else if (type == "binary") {
        auto data = Base64Decode(v.get("Data"));
        RegSetKeyValueW(HKEY_CURRENT_USER, full.c_str(), name.c_str(), (DWORD)v.geti("RawType", REG_BINARY), data.data(), (DWORD)data.size());
    }
}

// identity of an item of a settings list (preset, theme, quick effect, stamp category, pinned stamp) for the merge
std::string ItemKey(const XmlNode& n) {
    for (const char* k : { "DisplayName", "Name", "Key", "Id" }) if (n.attr(k)) return n.name + "|" + k + "=" + n.get(k);
    return "";
}

// imported items the current node does not have are added: named items by name; a list without attributes (the
// <Presets> of Presets1.xml, the pinned stamps) is merged inside the current one; other unnamed items (a favorite)
// when no identical one exists
void MergeNode(XmlNode& cur, const XmlNode& imp) {
    for (auto& c : imp.children) {
        std::string key = ItemKey(c);
        if (!key.empty()) {
            if (std::none_of(cur.children.begin(), cur.children.end(), [&](const XmlNode& x) { return ItemKey(x) == key; }))
                cur.children.push_back(c);
            continue;
        }
        if (c.attrs.empty() && !c.children.empty()) {
            auto it = std::find_if(cur.children.begin(), cur.children.end(), [&](const XmlNode& x) { return x.name == c.name && x.attrs.empty(); });
            if (it != cur.children.end()) { MergeNode(*it, c); continue; }
        }
        std::string s = XmlSerialize(c, false);
        if (std::none_of(cur.children.begin(), cur.children.end(), [&](const XmlNode& x) { return XmlSerialize(x, false) == s; }))
            cur.children.push_back(c);
    }
}
}

std::string MergeXml(const std::string& current, const std::string& imported) {
    XmlNode cur, imp;
    if (!XmlParse(imported, imp)) return current;
    if (!XmlParse(current, cur) || cur.name != imp.name) return imported;
    MergeNode(cur, imp);
    return XmlSerialize(cur);
}

bool Export(const std::wstring& path, std::wstring& err) {
    XmlNode root("SnapnabSettings");
    root.seti("Version", 1).setW("Created", TimeStamp(L"%04d-%02d-%02dT%02d:%02d:%02d")).setW("Program", kAppVersion);
    ExportKey(HKEY_CURRENT_USER, L"", root.add("Registry"));
    XmlNode& files = root.add("Files");
    for (auto f : kFiles) {
        std::string data = ReadAll(AppDataDir() + L"\\" + f);
        if (data.empty()) continue;
        files.add("File").setW("Name", f).set("Data", Base64Encode(std::vector<BYTE>(data.begin(), data.end())));
    }
    if (!XmlSaveFile(path, root)) { err = Tr(L"Não foi possível gravar o arquivo."); return false; }
    Log(L"Settings exported: %s", path.c_str());
    return true;
}

bool Validate(const std::wstring& path) {
    XmlNode root;
    return XmlLoadFile(path, root) && root.name == "SnapnabSettings" && root.child("Registry");
}

bool Import(const std::wstring& path, bool merge, std::wstring& err) {
    XmlNode root;
    if (!XmlLoadFile(path, root) || root.name != "SnapnabSettings") { err = Tr(L"Arquivo de configurações inválido."); return false; }
    if (!merge) {                                          // replace: starts from empty settings
        RegDeleteTreeW(HKEY_CURRENT_USER, kRegRoot);
        for (auto f : kFiles) DeleteFileW((AppDataDir() + L"\\" + f).c_str());
    }
    if (const XmlNode* reg = root.child("Registry"))
        for (auto& k : reg->children) {
            if (k.name != "Key") continue;
            std::wstring sub = k.getW("Path");
            if (sub.find(L"..") != std::wstring::npos) continue;
            std::wstring full = std::wstring(kRegRoot) + (sub.empty() ? L"" : L"\\" + sub);
            HKEY hk;
            if (RegCreateKeyExW(HKEY_CURRENT_USER, full.c_str(), 0, nullptr, 0, KEY_WRITE, nullptr, &hk, nullptr) == ERROR_SUCCESS) RegCloseKey(hk);
            for (auto& v : k.children) if (v.name == "Value") ImportValue(sub, v, merge);
        }
    if (const XmlNode* files = root.child("Files"))
        for (auto& f : files->children) {
            std::wstring name = f.getW("Name");
            if (std::none_of(std::begin(kFiles), std::end(kFiles), [&](const wchar_t* k) { return !_wcsicmp(k, name.c_str()); })) continue;   // only known files
            auto bytes = Base64Decode(f.get("Data"));
            std::string data(bytes.begin(), bytes.end());
            std::wstring dst = AppDataDir() + L"\\" + name;
            if (merge && FileExists(dst)) {
                if (!_wcsicmp(name.c_str(), L"ToolStyles.xml")) continue;    // one style per tool: the current ones stay
                data = MergeXml(ReadAll(dst), data);
            }
            if (!WriteAll(dst, data)) { err = Tr(L"Não foi possível gravar ") + name; return false; }
        }
    Log(L"Settings imported (%s): %s", merge ? L"merge" : L"replace", path.c_str());
    return true;
}

std::wstring BackupDir() {
    std::wstring d = AppDataDir() + L"\\Backups";
    EnsureDir(d);
    return d;
}

std::wstring AutoBackup() {
    SYSTEMTIME st; GetLocalTime(&st);
    FILETIME now; SystemTimeToFileTime(&st, &now);
    ULARGE_INTEGER n{ now.dwLowDateTime, now.dwHighDateTime };
    ULONGLONG last = ((ULONGLONG)RegGetDword(L"SettingsBackupHigh", 0) << 32) | RegGetDword(L"SettingsBackupLow", 0);
    const ULONGLONG week = 7ULL * 24 * 3600 * 10000000ULL;
    if (last && n.QuadPart - last < week) return L"";
    std::wstring dir = BackupDir();
    std::wstring path = dir + Format(L"\\Settings-%04d-%02d-%02d.snapsettings", st.wYear, st.wMonth, st.wDay);
    std::wstring err;
    if (!Export(path, err)) return L"";
    RegSetDword(L"SettingsBackupHigh", (DWORD)(n.QuadPart >> 32));
    RegSetDword(L"SettingsBackupLow", (DWORD)(n.QuadPart & 0xFFFFFFFF));
    // keeps the 5 most recent backups
    std::vector<std::wstring> all;
    WIN32_FIND_DATAW fd;
    HANDLE h = FindFirstFileW((dir + L"\\Settings-*.snapsettings").c_str(), &fd);
    if (h != INVALID_HANDLE_VALUE) {
        do all.push_back(fd.cFileName); while (FindNextFileW(h, &fd));
        FindClose(h);
    }
    std::sort(all.begin(), all.end());
    for (size_t i = 0; i + 5 < all.size(); i++) DeleteFileW((dir + L"\\" + all[i]).c_str());
    return path;
}

} // namespace snap::settings
