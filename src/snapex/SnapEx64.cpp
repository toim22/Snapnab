// SnapEx64.dll — "Snapnab Extended Window Capture" (≙ SnagEx64.dll, ds_pseudocode §6 / ds_arch §10.1)
//   Snap5000(msgEnter, msgLeave): installs a global WH_CALLWNDPROC hook; when the target window receives msgEnter,
//   it is subclassed and starts answering WM_GETMINMAXINFO with 19500×19500 (it can grow beyond the screen).
//   Snap5001(): undoes it.
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>

#pragma data_seg(".shared")
static HHOOK    g_hook = nullptr;
static UINT     g_msgEnter = 0;
static UINT     g_msgLeave = 0;
#pragma data_seg()
#pragma comment(linker, "/SECTION:.shared,RWS")

// local state of each process the DLL was mapped into
static HMODULE  g_self = nullptr;
static HWND     g_hwnd = nullptr;
static WNDPROC  g_oldProc = nullptr;
static bool     g_subclassed = false;

static LRESULT CALLBACK SubclassProc(HWND h, UINT m, WPARAM wp, LPARAM lp) {
    if (m == WM_GETMINMAXINFO) {
        auto* mmi = (MINMAXINFO*)lp;
        mmi->ptMaxTrackSize.x = 19500;      // 0x4c2c
        mmi->ptMaxTrackSize.y = 19500;
        mmi->ptMaxSize.x = 19500;
        mmi->ptMaxSize.y = 19500;
        return 1;                            // does not call the original proc
    }
    return IsWindowUnicode(h) ? CallWindowProcW(g_oldProc, h, m, wp, lp) : CallWindowProcA(g_oldProc, h, m, wp, lp);
}

static LRESULT CALLBACK HookProc(int nCode, WPARAM wp, LPARAM lp) {
    if (nCode == HC_ACTION) {
        auto* c = (CWPSTRUCT*)lp;
        if (c->message == g_msgEnter && g_msgEnter && !g_subclassed) {
            g_oldProc = IsWindowUnicode(c->hwnd)
                ? (WNDPROC)SetWindowLongPtrW(c->hwnd, GWLP_WNDPROC, (LONG_PTR)&SubclassProc)
                : (WNDPROC)SetWindowLongPtrA(c->hwnd, GWLP_WNDPROC, (LONG_PTR)&SubclassProc);
            if (g_oldProc) { g_hwnd = c->hwnd; g_subclassed = true; }
        }
        if (c->message == g_msgLeave && g_msgLeave && g_subclassed && c->hwnd == g_hwnd) {
            if (IsWindowUnicode(g_hwnd)) SetWindowLongPtrW(g_hwnd, GWLP_WNDPROC, (LONG_PTR)g_oldProc);
            else SetWindowLongPtrA(g_hwnd, GWLP_WNDPROC, (LONG_PTR)g_oldProc);
            g_subclassed = false; g_hwnd = nullptr;
        }
    }
    return CallNextHookEx(g_hook, nCode, wp, lp);
}

extern "C" BOOL Snap5000(UINT msgEnter, UINT msgLeave, DWORD threadId) {
    g_msgEnter = msgEnter;
    g_msgLeave = msgLeave;
    // threadId = 0 → all threads of the desktop (like the original); the capturer passes the target window's thread
    g_hook = SetWindowsHookExW(WH_CALLWNDPROC, HookProc, g_self, threadId);
    return g_hook != nullptr;
}

extern "C" void Snap5001() {
    if (g_hook) { UnhookWindowsHookEx(g_hook); g_hook = nullptr; }
}

BOOL APIENTRY DllMain(HMODULE h, DWORD reason, LPVOID) {
    if (reason == DLL_PROCESS_ATTACH) { g_self = h; DisableThreadLibraryCalls(h); }
    if (reason == DLL_PROCESS_DETACH && g_subclassed && g_hwnd && IsWindow(g_hwnd)) {
        SetWindowLongPtrW(g_hwnd, GWLP_WNDPROC, (LONG_PTR)g_oldProc);   // never leaves the window with a dangling proc
    }
    return TRUE;
}
