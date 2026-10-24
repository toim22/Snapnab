#include "Common.h"
#include <cstdarg>
#include <cstdio>
#include <ctime>
#include <rpc.h>
#include <commdlg.h>
#include <lmcons.h>
#include <shellscalingapi.h>

namespace snap {

std::wstring Utf8ToW(const std::string& s) {
    if (s.empty()) return {};
    int n = MultiByteToWideChar(CP_UTF8, 0, s.data(), (int)s.size(), nullptr, 0);
    std::wstring w(n, L'\0');
    MultiByteToWideChar(CP_UTF8, 0, s.data(), (int)s.size(), w.data(), n);
    return w;
}

std::string WToUtf8(const std::wstring& w) {
    if (w.empty()) return {};
    int n = WideCharToMultiByte(CP_UTF8, 0, w.data(), (int)w.size(), nullptr, 0, nullptr, nullptr);
    std::string s(n, '\0');
    WideCharToMultiByte(CP_UTF8, 0, w.data(), (int)w.size(), s.data(), n, nullptr, nullptr);
    return s;
}

std::wstring Format(const wchar_t* fmt, ...) {
    va_list ap; va_start(ap, fmt);
    va_list ap2; va_copy(ap2, ap);
    int n = _vscwprintf(fmt, ap);
    va_end(ap);
    std::wstring s(n > 0 ? n : 0, L'\0');
    if (n > 0) vswprintf(s.data(), n + 1, fmt, ap2);
    va_end(ap2);
    return s;
}

std::string FormatA(const char* fmt, ...) {
    va_list ap; va_start(ap, fmt);
    va_list ap2; va_copy(ap2, ap);
    int n = _vscprintf(fmt, ap);
    va_end(ap);
    std::string s(n > 0 ? n : 0, '\0');
    if (n > 0) vsnprintf(s.data(), n + 1, fmt, ap2);
    va_end(ap2);
    return s;
}

std::wstring Trim(const std::wstring& s) {
    size_t a = s.find_first_not_of(L" \t\r\n\"");
    if (a == std::wstring::npos) return {};
    size_t b = s.find_last_not_of(L" \t\r\n\"");
    return s.substr(a, b - a + 1);
}

std::wstring ToLower(std::wstring s) {
    CharLowerBuffW(s.data(), (DWORD)s.size());
    return s;
}

std::wstring FileExt(const std::wstring& path) {
    const wchar_t* e = PathFindExtensionW(path.c_str());
    return ToLower(e ? e : L"");
}

std::wstring FileNameOf(const std::wstring& path) { return PathFindFileNameW(path.c_str()); }

std::wstring DirOf(const std::wstring& path) {
    size_t p = path.find_last_of(L"\\/");
    return p == std::wstring::npos ? L"" : path.substr(0, p);
}

std::wstring StripExt(const std::wstring& path) {
    std::wstring s = path;
    PathRemoveExtensionW(s.data());
    s.resize(wcslen(s.c_str()));
    return s;
}

std::wstring NewGuidString() {
    GUID g; CoCreateGuid(&g);
    wchar_t buf[64];
    StringFromGUID2(g, buf, 64);
    std::wstring s = buf;
    return s.substr(1, s.size() - 2);
}

std::wstring TimeStamp(const wchar_t* fmt) {
    SYSTEMTIME st; GetLocalTime(&st);
    return Format(fmt, st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond);
}

std::wstring ExeDir() {
    wchar_t buf[MAX_PATH]; GetModuleFileNameW(nullptr, buf, MAX_PATH);
    return DirOf(buf);
}

static std::wstring KnownFolder(REFKNOWNFOLDERID id) {
    PWSTR p = nullptr; std::wstring r;
    if (SUCCEEDED(SHGetKnownFolderPath(id, KF_FLAG_CREATE, nullptr, &p))) r = p;
    CoTaskMemFree(p);
    return r;
}

bool EnsureDir(const std::wstring& dir) {
    if (dir.empty()) return false;
    int r = SHCreateDirectoryExW(nullptr, dir.c_str(), nullptr);
    return r == ERROR_SUCCESS || r == ERROR_ALREADY_EXISTS || r == ERROR_FILE_EXISTS;
}

std::wstring AppDataDir() { auto d = KnownFolder(FOLDERID_LocalAppData) + L"\\Snapnab"; EnsureDir(d); return d; }
std::wstring LibraryDir() {
    std::wstring custom = RegGetString(L"LibraryFolder", L"");
    auto d = custom.empty() ? AppDataDir() + L"\\Library" : custom;
    EnsureDir(d); return d;
}
std::wstring ThumbDir() { auto d = LibraryDir() + L"\\Thumbnails"; EnsureDir(d); return d; }
std::wstring LogsDir() { auto d = AppDataDir() + L"\\Logs"; EnsureDir(d); return d; }
std::wstring DefaultSaveDir() {
    auto d = RegGetString(L"OutputFolder", KnownFolder(FOLDERID_Pictures) + L"\\Snapnab");
    EnsureDir(d); return d;
}

bool FileExists(const std::wstring& path) {
    DWORD a = GetFileAttributesW(path.c_str());
    return a != INVALID_FILE_ATTRIBUTES && !(a & FILE_ATTRIBUTE_DIRECTORY);
}

uint64_t FileSize(const std::wstring& path) {
    WIN32_FILE_ATTRIBUTE_DATA d;
    if (!GetFileAttributesExW(path.c_str(), GetFileExInfoStandard, &d)) return 0;
    return ((uint64_t)d.nFileSizeHigh << 32) | d.nFileSizeLow;
}

// ---------------------------------------------------------------- log
static std::mutex g_logMutex;
static std::wstring g_logFile;
static std::wstring g_logComponent;

void LogInit(const wchar_t* component) {
    g_logComponent = component;
    g_logFile = LogsDir() + L"\\" + component + L".log";
    if (FileSize(g_logFile) > 2 * 1024 * 1024) {
        MoveFileExW(g_logFile.c_str(), (g_logFile + L".old").c_str(), MOVEFILE_REPLACE_EXISTING);
    }
    Log(L"==== %s %s iniciado (pid %u) ====", component, kAppVersion, GetCurrentProcessId());
}

void Log(const wchar_t* fmt, ...) {
    va_list ap; va_start(ap, fmt);
    wchar_t msg[2048];
    _vsnwprintf_s(msg, _TRUNCATE, fmt, ap);
    va_end(ap);
    SYSTEMTIME st; GetLocalTime(&st);
    std::wstring line = Format(L"%04d-%02d-%02d %02d:%02d:%02d.%03d [%u] %s\r\n", st.wYear, st.wMonth, st.wDay,
                               st.wHour, st.wMinute, st.wSecond, st.wMilliseconds, GetCurrentThreadId(), msg);
    OutputDebugStringW(line.c_str());
    if (g_logFile.empty()) return;
    std::lock_guard<std::mutex> lk(g_logMutex);
    HANDLE h = CreateFileW(g_logFile.c_str(), FILE_APPEND_DATA, FILE_SHARE_READ | FILE_SHARE_WRITE, nullptr,
                           OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (h == INVALID_HANDLE_VALUE) return;
    DWORD wr0;
    if (GetLastError() != ERROR_ALREADY_EXISTS) WriteFile(h, "\xEF\xBB\xBF", 3, &wr0, nullptr);   // BOM UTF-8 (Bloco de Notas)
    std::string u = WToUtf8(line);
    DWORD wr; WriteFile(h, u.data(), (DWORD)u.size(), &wr, nullptr);
    CloseHandle(h);
}

// ---------------------------------------------------------------- registro
static std::wstring RegPath(const wchar_t* sub) {
    std::wstring p = kRegRoot;
    if (sub && *sub) { p += L"\\"; p += sub; }
    return p;
}

DWORD RegGetDword(const wchar_t* name, DWORD def, const wchar_t* sub) {
    DWORD v = 0, cb = sizeof(v);
    if (RegGetValueW(HKEY_CURRENT_USER, RegPath(sub).c_str(), name, RRF_RT_REG_DWORD, nullptr, &v, &cb) == ERROR_SUCCESS)
        return v;
    return def;
}

void RegSetDword(const wchar_t* name, DWORD v, const wchar_t* sub) {
    RegSetKeyValueW(HKEY_CURRENT_USER, RegPath(sub).c_str(), name, REG_DWORD, &v, sizeof(v));
}

std::wstring RegGetString(const wchar_t* name, const std::wstring& def, const wchar_t* sub) {
    DWORD cb = 0;
    if (RegGetValueW(HKEY_CURRENT_USER, RegPath(sub).c_str(), name, RRF_RT_REG_SZ, nullptr, nullptr, &cb) != ERROR_SUCCESS)
        return def;
    std::wstring s(cb / sizeof(wchar_t) + 1, L'\0');
    if (RegGetValueW(HKEY_CURRENT_USER, RegPath(sub).c_str(), name, RRF_RT_REG_SZ, nullptr, s.data(), &cb) != ERROR_SUCCESS)
        return def;
    s.resize(wcslen(s.c_str()));
    return s;
}

void RegSetString(const wchar_t* name, const std::wstring& v, const wchar_t* sub) {
    RegSetKeyValueW(HKEY_CURRENT_USER, RegPath(sub).c_str(), name, REG_SZ, v.c_str(),
                    (DWORD)((v.size() + 1) * sizeof(wchar_t)));
}

// ---------------------------------------------------------------- instância única
struct CopyDataHeader { GUID guid; DWORD cb; };

bool SendCommandLine(HWND target, const std::wstring& cmdLine) {
    std::wstring text = cmdLine.substr(0, 4999);         // até 10 000 bytes de texto
    std::vector<BYTE> buf(sizeof(CopyDataHeader) + (text.size() + 1) * sizeof(wchar_t));
    auto* h = reinterpret_cast<CopyDataHeader*>(buf.data());
    h->guid = kCopyDataGuid;
    h->cb = (DWORD)((text.size() + 1) * sizeof(wchar_t));
    memcpy(buf.data() + sizeof(CopyDataHeader), text.c_str(), h->cb);
    COPYDATASTRUCT cds{ kCopyDataCmdLine, (DWORD)buf.size(), buf.data() };
    DWORD_PTR res = 0;
    return SendMessageTimeoutW(target, WM_COPYDATA, 0, (LPARAM)&cds, SMTO_ABORTIFHUNG, 10000, &res) != 0;
}

bool ParseCopyData(const COPYDATASTRUCT* cds, std::wstring& out) {
    if (!cds || cds->dwData != kCopyDataCmdLine || cds->cbData < sizeof(CopyDataHeader)) return false;
    auto* h = reinterpret_cast<const CopyDataHeader*>(cds->lpData);
    if (memcmp(&h->guid, &kCopyDataGuid, sizeof(GUID)) != 0) return false;
    if (h->cb > cds->cbData - sizeof(CopyDataHeader) || h->cb > 10000) return false;
    const wchar_t* t = reinterpret_cast<const wchar_t*>((const BYTE*)cds->lpData + sizeof(CopyDataHeader));
    out.assign(t, wcsnlen(t, h->cb / sizeof(wchar_t)));
    return true;
}

bool ForwardToExistingInstance(const wchar_t* mutexName, const wchar_t* wndClass, HANDLE* outMutex) {
    HANDLE m = CreateMutexW(nullptr, TRUE, mutexName);
    bool exists = GetLastError() == ERROR_ALREADY_EXISTS;
    if (!exists) { *outMutex = m; return false; }
    if (m) CloseHandle(m);
    HWND w = nullptr;
    for (int i = 0; i < 25 && !w; i++) { w = FindWindowW(wndClass, nullptr); if (!w) Sleep(200); }
    if (w) {
        AllowSetForegroundWindow(ASFW_ANY);
        std::wstring cmd = GetCommandLineW();
        SendCommandLine(w, cmd);
    }
    return true;
}

std::vector<std::wstring> SplitCommandLine(const std::wstring& cmdLine) {
    std::vector<std::wstring> r;
    int argc = 0;
    LPWSTR* argv = CommandLineToArgvW(cmdLine.c_str(), &argc);
    for (int i = 1; i < argc; i++) r.push_back(argv[i]);       // ignora o executável
    LocalFree(argv);
    return r;
}

bool IsProcessRunning(const wchar_t* mutexName) {
    HANDLE m = OpenMutexW(SYNCHRONIZE, FALSE, mutexName);
    if (m) { CloseHandle(m); return true; }
    return false;
}

bool LaunchSibling(const wchar_t* exeName, const std::wstring& args, bool lowPriority) {
    std::wstring exe = ExeDir() + L"\\" + exeName;
    std::wstring cmd = L"\"" + exe + L"\" " + args;
    STARTUPINFOW si{ sizeof(si) };
    PROCESS_INFORMATION pi{};
    if (!CreateProcessW(exe.c_str(), cmd.data(), nullptr, nullptr, FALSE,
                        lowPriority ? BELOW_NORMAL_PRIORITY_CLASS : 0, nullptr, ExeDir().c_str(), &si, &pi)) {
        Log(L"LaunchSibling falhou: %s (%u)", exe.c_str(), GetLastError());
        return false;
    }
    AllowSetForegroundWindow(pi.dwProcessId);
    CloseHandle(pi.hThread); CloseHandle(pi.hProcess);
    return true;
}

// ---------------------------------------------------------------- GDI+
GdiplusSession::GdiplusSession() {
    Gdiplus::GdiplusStartupInput in;
    Gdiplus::GdiplusStartup(&token_, &in, nullptr);
}
GdiplusSession::~GdiplusSession() { Gdiplus::GdiplusShutdown(token_); }

// ---------------------------------------------------------------- diversos
std::wstring ComputerName() {
    wchar_t b[MAX_COMPUTERNAME_LENGTH + 1]; DWORD n = MAX_COMPUTERNAME_LENGTH + 1;
    return GetComputerNameW(b, &n) ? b : L"";
}
std::wstring UserName() {
    wchar_t b[UNLEN + 1]; DWORD n = UNLEN + 1;
    return GetUserNameW(b, &n) ? b : L"";
}

RECT VirtualScreenRect() {
    RECT r;
    r.left = GetSystemMetrics(SM_XVIRTUALSCREEN);
    r.top = GetSystemMetrics(SM_YVIRTUALSCREEN);
    r.right = r.left + GetSystemMetrics(SM_CXVIRTUALSCREEN);
    r.bottom = r.top + GetSystemMetrics(SM_CYVIRTUALSCREEN);
    return r;
}

RECT MonitorRectFromPoint(POINT pt, bool work) {
    MONITORINFO mi{ sizeof(mi) };
    GetMonitorInfoW(MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST), &mi);
    return work ? mi.rcWork : mi.rcMonitor;
}

int DpiForWindow(HWND hwnd) {
    UINT d = hwnd ? GetDpiForWindow(hwnd) : GetDpiForSystem();
    return d ? (int)d : 96;
}

std::wstring ColorToHex(COLORREF c) {
    return Format(L"#%02X%02X%02X", GetRValue(c), GetGValue(c), GetBValue(c));
}

COLORREF HexToColor(const std::wstring& s, COLORREF def) {
    if (s.size() < 7 || s[0] != L'#') return def;
    unsigned v = wcstoul(s.c_str() + 1, nullptr, 16);
    return RGB((v >> 16) & 255, (v >> 8) & 255, v & 255);
}

bool PickColor(HWND owner, COLORREF& c) {
    static COLORREF custom[16] = { RGB(242,38,19), RGB(255,214,0), RGB(46,204,64), RGB(0,116,217),
                                   RGB(177,13,201), RGB(255,133,27), RGB(0,0,0), RGB(255,255,255) };
    CHOOSECOLORW cc{ sizeof(cc) };
    cc.hwndOwner = owner; cc.rgbResult = c; cc.lpCustColors = custom;
    cc.Flags = CC_FULLOPEN | CC_RGBINIT;
    if (!ChooseColorW(&cc)) return false;
    c = cc.rgbResult;
    return true;
}

std::wstring HotkeyToText(UINT mods, UINT vk) {
    if (!vk) return L"(nenhum)";
    std::wstring s;
    if (mods & MOD_CONTROL) s += L"Ctrl+";
    if (mods & MOD_SHIFT) s += L"Shift+";
    if (mods & MOD_ALT) s += L"Alt+";
    if (mods & MOD_WIN) s += L"Win+";
    UINT sc = MapVirtualKeyW(vk, MAPVK_VK_TO_VSC);
    LONG lp = sc << 16;
    switch (vk) {
    case VK_SNAPSHOT: return s + L"Print Screen";
    case VK_INSERT: case VK_DELETE: case VK_HOME: case VK_END: case VK_PRIOR: case VK_NEXT:
    case VK_LEFT: case VK_RIGHT: case VK_UP: case VK_DOWN: lp |= 1 << 24; break;
    }
    wchar_t name[64] = L"";
    if (GetKeyNameTextW(lp, name, 64) > 0) return s + name;
    return s + Format(L"VK%u", vk);
}

} // namespace snap
