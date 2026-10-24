// Gerenciador de atalhos globais (comp_7 §4.2) + gancho de teclado de reserva (CSnagItKeybHook, §4.3)
#pragma once
#include "Common.h"

namespace snap {

class HotkeyManager {
public:
    enum Result { Ok, Removed, InternalConflict, HookedFallback, Failed };
    void Init(HWND hwnd) { hwnd_ = hwnd; }
    // (0,0) remove; conflito interno não registra; conflito externo cai para o gancho WH_KEYBOARD_LL
    Result Set(int id, UINT mods, UINT vk, bool allowOverride = true);
    void Remove(int id) { Set(id, 0, 0); }
    void Clear();
    bool IsHooked(int id) const { return hooked_.count(id) != 0; }
    ~HotkeyManager() { Clear(); }
private:
    static LRESULT CALLBACK LowLevelProc(int code, WPARAM w, LPARAM l);
    void UpdateHook();
    HWND hwnd_ = nullptr;
    std::map<int, std::pair<UINT, UINT>> map_;      // id → (mods, vk) aceitos
    std::map<int, std::pair<UINT, UINT>> hooked_;   // ids servidos pelo gancho
    HHOOK hook_ = nullptr;
};

inline DWORD PackHotkey(UINT mods, UINT vk) { return (mods << 16) | (vk & 0xFFFF); }
inline void UnpackHotkey(DWORD v, UINT& mods, UINT& vk) { mods = v >> 16; vk = v & 0xFFFF; }

} // namespace snap
