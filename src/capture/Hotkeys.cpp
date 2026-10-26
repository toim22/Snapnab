#include "Hotkeys.h"

namespace snap {

static HotkeyManager* g_mgr = nullptr;

// Windows 11 "Use the Print screen key to open screen capture": the shell takes plain Print Screen before
// RegisterHotKey's owner ever sees it, although the registration succeeds. The value is missing until the user
// changes the setting, and then it is on by default from Windows 11 (build 22000) on.
static bool WindowsOwnsPrintScreen() {
    DWORD v = 0, sz = sizeof(v);
    if (RegGetValueW(HKEY_CURRENT_USER, L"Control Panel\\Keyboard", L"PrintScreenKeyForSnippingEnabled", RRF_RT_REG_DWORD,
                     nullptr, &v, &sz) == ERROR_SUCCESS)
        return v != 0;
    wchar_t build[16] = L""; DWORD bs = sizeof(build);
    RegGetValueW(HKEY_LOCAL_MACHINE, L"SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion", L"CurrentBuildNumber", RRF_RT_REG_SZ,
                 nullptr, build, &bs);
    return _wtoi(build) >= 22000;
}

HotkeyManager::Result HotkeyManager::Set(int id, UINT mods, UINT vk, bool allowOverride) {
    mods &= (MOD_ALT | MOD_CONTROL | MOD_SHIFT | MOD_WIN);
    if (mods == 0 && vk == 0) {
        if (map_.count(id)) { UnregisterHotKey(hwnd_, id); map_.erase(id); }
        hooked_.erase(id);
        UpdateHook();
        return Removed;
    }
    for (auto& [oid, kv] : map_)
        if (oid != id && kv.first == mods && kv.second == vk) {
            Log(L"Hotkey %s: internal conflict with id %d", HotkeyToText(mods, vk).c_str(), oid);
            return InternalConflict;
        }
    auto it = map_.find(id);
    if (it != map_.end() && it->second == std::make_pair(mods, vk)) return hooked_.count(id) ? HookedFallback : Ok;
    if (it != map_.end()) { UnregisterHotKey(hwnd_, id); map_.erase(it); hooked_.erase(id); }
    if (allowOverride && vk == VK_SNAPSHOT && mods == 0 && WindowsOwnsPrintScreen()) {
        Log(L"Hotkey %s: Windows opens the Snipping Tool with this key — using the keyboard hook", HotkeyToText(mods, vk).c_str());
        map_[id] = { mods, vk };
        hooked_[id] = { mods, vk };
        UpdateHook();
        return HookedFallback;
    }
    if (RegisterHotKey(hwnd_, id, mods | MOD_NOREPEAT, vk)) {
        map_[id] = { mods, vk };
        UpdateHook();
        Log(L"Hotkey registered: id %d = %s", id, HotkeyToText(mods, vk).c_str());
        return Ok;
    }
    // external conflict (another program or Windows already owns the combination, e.g. Print Screen)
    Log(L"Hotkey %s: RegisterHotKey failed (%u)%s", HotkeyToText(mods, vk).c_str(), GetLastError(),
        allowOverride ? L" — using the keyboard hook" : L"");
    if (!allowOverride) return Failed;
    map_[id] = { mods, vk };
    hooked_[id] = { mods, vk };
    UpdateHook();
    return HookedFallback;
}

void HotkeyManager::Clear() {
    for (auto& [id, kv] : map_) UnregisterHotKey(hwnd_, id);
    map_.clear(); hooked_.clear();
    UpdateHook();
}

void HotkeyManager::UpdateHook() {
    if (!hooked_.empty() && !hook_) {
        if (IsDebuggerPresent()) return;          // like the original: does not install under a debugger
        g_mgr = this;
        hook_ = SetWindowsHookExW(WH_KEYBOARD_LL, LowLevelProc, GetModuleHandleW(nullptr), 0);
    } else if (hooked_.empty() && hook_) {
        UnhookWindowsHookEx(hook_);
        hook_ = nullptr;
    }
}

LRESULT CALLBACK HotkeyManager::LowLevelProc(int code, WPARAM w, LPARAM l) {
    if (code == HC_ACTION && g_mgr && (w == WM_KEYDOWN || w == WM_SYSKEYDOWN)) {
        auto* k = (KBDLLHOOKSTRUCT*)l;
        // injected keys count too (on-screen keyboard, accessibility and macro tools); Snapnab itself only injects the wheel
        {
            UINT mods = 0;
            if (GetAsyncKeyState(VK_CONTROL) & 0x8000) mods |= MOD_CONTROL;
            if (GetAsyncKeyState(VK_SHIFT) & 0x8000) mods |= MOD_SHIFT;
            if (GetAsyncKeyState(VK_MENU) & 0x8000) mods |= MOD_ALT;
            if ((GetAsyncKeyState(VK_LWIN) | GetAsyncKeyState(VK_RWIN)) & 0x8000) mods |= MOD_WIN;
            for (auto& [id, kv] : g_mgr->hooked_)
                if (kv.second == k->vkCode && kv.first == mods) {
                    PostMessageW(g_mgr->hwnd_, WM_HOTKEY, id, MAKELPARAM(mods, k->vkCode));
                    return 1;          // consumes the key before its original owner
                }
        }
    }
    return CallNextHookEx(nullptr, code, w, l);
}

} // namespace snap
