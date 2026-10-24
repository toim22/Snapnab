#include "Hotkeys.h"

namespace snap {

static HotkeyManager* g_mgr = nullptr;

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
            Log(L"Atalho %s: conflito interno com id %d", HotkeyToText(mods, vk).c_str(), oid);
            return InternalConflict;
        }
    auto it = map_.find(id);
    if (it != map_.end() && it->second == std::make_pair(mods, vk)) return hooked_.count(id) ? HookedFallback : Ok;
    if (it != map_.end()) { UnregisterHotKey(hwnd_, id); map_.erase(it); hooked_.erase(id); }
    if (RegisterHotKey(hwnd_, id, mods | MOD_NOREPEAT, vk)) {
        map_[id] = { mods, vk };
        UpdateHook();
        Log(L"Atalho registrado: id %d = %s", id, HotkeyToText(mods, vk).c_str());
        return Ok;
    }
    // conflito externo (outro programa ou o Windows já possui a combinação, ex.: Print Screen)
    Log(L"Atalho %s: RegisterHotKey falhou (%u)%s", HotkeyToText(mods, vk).c_str(), GetLastError(),
        allowOverride ? L" — usando gancho de teclado" : L"");
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
        if (IsDebuggerPresent()) return;          // como o original: não instala sob depurador
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
        if (!(k->flags & LLKHF_INJECTED)) {
            UINT mods = 0;
            if (GetAsyncKeyState(VK_CONTROL) & 0x8000) mods |= MOD_CONTROL;
            if (GetAsyncKeyState(VK_SHIFT) & 0x8000) mods |= MOD_SHIFT;
            if (GetAsyncKeyState(VK_MENU) & 0x8000) mods |= MOD_ALT;
            if ((GetAsyncKeyState(VK_LWIN) | GetAsyncKeyState(VK_RWIN)) & 0x8000) mods |= MOD_WIN;
            for (auto& [id, kv] : g_mgr->hooked_)
                if (kv.second == k->vkCode && kv.first == mods) {
                    PostMessageW(g_mgr->hwnd_, WM_HOTKEY, id, MAKELPARAM(mods, k->vkCode));
                    return 1;          // consome a tecla antes do dono original
                }
        }
    }
    return CallNextHookEx(nullptr, code, w, l);
}

} // namespace snap
