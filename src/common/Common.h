// Snapnab — camada comum (equivalente funcional de SnagitCommonInterop/SnagitCommonDotNet, ds_arch §8.3)
#pragma once
#ifndef NOMINMAX
#define NOMINMAX
#endif
#ifndef UNICODE
#define UNICODE
#endif
#ifndef _UNICODE
#define _UNICODE
#endif
#include <windows.h>
#include <windowsx.h>
#include <shellapi.h>
#include <shlobj.h>
#include <shlwapi.h>
#include <commctrl.h>
#include <objidl.h>
#include <algorithm>
namespace Gdiplus { using std::min; using std::max; }
#include <gdiplus.h>
#include <string>
#include <vector>
#include <memory>
#include <functional>
#include <cstdint>
#include <mutex>
#include <thread>
#include <atomic>
#include <map>
#include "Version.h"            // gerado pelo CMake (src/common/Version.h.in)

namespace snap {

// ---- Identidade dos processos (ds_pseudocode §1/§4; nomes próprios do Snapnab) ----
constexpr wchar_t kCaptureWndClass[] = L"SnapnabUI";            // ≙ "SnagIt5UI"
constexpr wchar_t kEditorWndClass[]  = L"SnapnabEditor";        // ≙ "SnagIt9Editor"
constexpr wchar_t kCaptureMutex[]    = L"Snapnab-Capture-{41F1BF9C-5A4E-4B6F-9C33-1D2E3F405A61}";
constexpr wchar_t kEditorMutex[]     = L"Snapnab-Editor-{800F2EB3-7C1D-4E2A-A3B4-C5D6E7F80912}";
constexpr wchar_t kCaptureExe[]      = L"Snapnab.exe";
constexpr wchar_t kEditorExe[]       = L"SnapnabEditor.exe";
constexpr wchar_t kRegRoot[]         = L"Software\\Snapnab\\1";
constexpr wchar_t kAppVersion[]      = SNAPNAB_VERSION_WSTR;   // de project(VERSION) no CMakeLists.txt
constexpr DWORD   kCopyDataCmdLine   = 0x8010;                  // WM_COPYDATA dwData (ed_comp_1)
// GUID que abre o bloco WM_COPYDATA (cabeçalho + tamanho + texto UTF-16, até 10 000 bytes)
constexpr GUID kCopyDataGuid = { 0x61e85d7d, 0x94d3, 0x4123, { 0x84, 0x10, 0xfa, 0x58, 0xd2, 0xd0, 0x9d, 0xf2 } };

// ---- Strings e formatação ----
std::wstring Utf8ToW(const std::string& s);
std::string  WToUtf8(const std::wstring& s);
std::wstring Format(const wchar_t* fmt, ...);
std::string  FormatA(const char* fmt, ...);
std::wstring Trim(const std::wstring& s);
std::wstring ToLower(std::wstring s);
std::wstring FileExt(const std::wstring& path);       // ".png" em minúsculas
std::wstring FileNameOf(const std::wstring& path);
std::wstring DirOf(const std::wstring& path);
std::wstring StripExt(const std::wstring& path);
std::wstring NewGuidString();
std::wstring TimeStamp(const wchar_t* fmt = L"%04d-%02d-%02d_%02d-%02d-%02d");

// ---- Pastas (ds_arch §16.2: estado local ao perfil do usuário) ----
std::wstring ExeDir();
std::wstring AppDataDir();      // %LOCALAPPDATA%\Snapnab
std::wstring LibraryDir();      // %LOCALAPPDATA%\Snapnab\Library   (DataStore)
std::wstring ThumbDir();        // %LOCALAPPDATA%\Snapnab\Library\Thumbnails
std::wstring LogsDir();         // %LOCALAPPDATA%\Snapnab\Logs
std::wstring DefaultSaveDir();  // Imagens\Snapnab
bool EnsureDir(const std::wstring& dir);
bool FileExists(const std::wstring& path);
uint64_t FileSize(const std::wstring& path);

// ---- Log interno (CSnagitLogger, comp_10) ----
void LogInit(const wchar_t* component);
void Log(const wchar_t* fmt, ...);

// ---- Configuração no registro (RegistryConfig, ds_arch K8) ----
DWORD        RegGetDword(const wchar_t* name, DWORD def, const wchar_t* sub = nullptr);
void         RegSetDword(const wchar_t* name, DWORD v, const wchar_t* sub = nullptr);
std::wstring RegGetString(const wchar_t* name, const std::wstring& def, const wchar_t* sub = nullptr);
void         RegSetString(const wchar_t* name, const std::wstring& v, const wchar_t* sub = nullptr);

// ---- Instância única + linha de comando por WM_COPYDATA (ds_pseudocode §2, ed_comp_1) ----
// Retorna true se já havia instância (e a linha de comando foi repassada a ela).
bool ForwardToExistingInstance(const wchar_t* mutexName, const wchar_t* wndClass, HANDLE* outMutex);
bool SendCommandLine(HWND target, const std::wstring& cmdLine);
bool ParseCopyData(const COPYDATASTRUCT* cds, std::wstring& outCmdLine);
std::vector<std::wstring> SplitCommandLine(const std::wstring& cmdLine);
bool IsProcessRunning(const wchar_t* mutexName);
bool LaunchSibling(const wchar_t* exeName, const std::wstring& args, bool lowPriority = false);

// ---- Mensagens de janela privadas ----
constexpr UINT WM_SNAP_IPC      = WM_APP + 0x40;   // IPC recebido (lParam = IpcMessage*)
constexpr UINT WM_SNAP_TRAY     = WM_APP + 0x41;
constexpr UINT WM_SNAP_CMDLINE  = WM_APP + 0x42;   // lParam = std::wstring*
constexpr UINT WM_SNAP_DONE     = WM_APP + 0x43;   // trabalho em segundo plano concluído

// ---- GDI+ ----
class GdiplusSession {
public:
    GdiplusSession();
    ~GdiplusSession();
private:
    ULONG_PTR token_ = 0;
};

// ---- Diversos ----
std::wstring ComputerName();
std::wstring UserName();
RECT VirtualScreenRect();
RECT MonitorRectFromPoint(POINT pt, bool work = false);
int  DpiForWindow(HWND hwnd);
std::wstring ColorToHex(COLORREF c);
COLORREF HexToColor(const std::wstring& s, COLORREF def);
bool PickColor(HWND owner, COLORREF& c);
std::wstring HotkeyToText(UINT mods, UINT vk);

} // namespace snap
