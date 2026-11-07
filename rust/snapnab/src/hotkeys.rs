//! The global hotkeys (Hotkeys.cpp, comp_7 §4.2) and their fallback keyboard hook (CSnagItKeybHook, §4.3). A key
//! another program (or Windows) already owns is served by a WH_KEYBOARD_LL hook, which posts the same WM_HOTKEY to the
//! capture window and swallows the key; so is plain Print Screen when Windows 11 opens the Snipping Tool with it
//! (RegisterHotKey succeeds there, but the shell takes the key first).

use std::collections::BTreeMap;
use std::sync::Mutex;

use snapcore::log;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Diagnostics::Debug::IsDebuggerPresent;
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyNameTextW, MAPVK_VK_TO_VSC, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
    MapVirtualKeyW, RegisterHotKey, UnregisterHotKey, VK_CONTROL, VK_DELETE, VK_DOWN, VK_END, VK_HOME, VK_INSERT, VK_LEFT,
    VK_LWIN, VK_MENU, VK_NEXT, VK_PRIOR, VK_RIGHT, VK_RWIN, VK_SHIFT, VK_SNAPSHOT, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, PostMessageW, SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL,
    WM_HOTKEY, WM_KEYDOWN, WM_SYSKEYDOWN,
};
use windows::core::w;

pub const HK_IMAGE: i32 = 1;
pub const HK_VIDEO: i32 = 2;
pub const HK_VIDEO_PAUSE: i32 = 4;
pub const HK_VIDEO_STOP: i32 = 5;
pub const HK_COLOR: i32 = 6;
pub const HK_CAPTURE_PIN: i32 = 7;
pub const HK_PIN_CLIP: i32 = 8;
pub const HK_FOCUS: i32 = 9;
pub const HK_VIDEO_ZOOM: i32 = 10;
pub const HK_PRESET_BASE: i32 = 100;

const MODS: u32 = MOD_ALT.0 | MOD_CONTROL.0 | MOD_SHIFT.0 | MOD_WIN.0;

pub fn pack(mods: u32, vk: u32) -> u32 {
    (mods << 16) | (vk & 0xFFFF)
}

pub fn unpack(v: u32) -> (u32, u32) {
    (v >> 16, v & 0xFFFF)
}

/// HotkeyToText: "Ctrl+Shift+F11", "Print Screen".
pub fn to_text(mods: u32, vk: u32) -> String {
    if vk == 0 {
        return snapnab_app::tr("(nenhum)");
    }
    let mut s = String::new();
    for (m, t) in [(MOD_CONTROL.0, "Ctrl+"), (MOD_SHIFT.0, "Shift+"), (MOD_ALT.0, "Alt+"), (MOD_WIN.0, "Win+")] {
        if mods & m != 0 {
            s += t;
        }
    }
    if vk == VK_SNAPSHOT.0 as u32 {
        return s + &snapnab_app::tr("Print Screen");
    }
    let sc = unsafe { MapVirtualKeyW(vk, MAPVK_VK_TO_VSC) };
    let mut lp = (sc << 16) as i32;
    let extended = [VK_INSERT, VK_DELETE, VK_HOME, VK_END, VK_PRIOR, VK_NEXT, VK_LEFT, VK_RIGHT, VK_UP, VK_DOWN];
    if extended.iter().any(|k| k.0 as u32 == vk) {
        lp |= 1 << 24;
    }
    let mut name = [0u16; 64];
    let n = unsafe { GetKeyNameTextW(lp, &mut name) };
    if n > 0 {
        return s + &String::from_utf16_lossy(&name[..n as usize]);
    }
    s + &format!("VK{vk}")
}

/// Windows 11 "Use the Print screen key to open screen capture": the value is missing until the user changes the
/// setting, and then it is on by default from build 22000 on.
fn windows_owns_print_screen() -> bool {
    let mut v = 0u32;
    let mut sz = 4u32;
    let r = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, w!("Control Panel\\Keyboard"), w!("PrintScreenKeyForSnippingEnabled"), RRF_RT_REG_DWORD,
                     None, Some(&mut v as *mut _ as *mut _), Some(&mut sz))
    };
    if r.is_ok() {
        return v != 0;
    }
    let mut build = [0u16; 16];
    let mut bs = (build.len() * 2) as u32;
    unsafe {
        let _ = RegGetValueW(HKEY_LOCAL_MACHINE, w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion"), w!("CurrentBuildNumber"),
                             RRF_RT_REG_SZ, None, Some(build.as_mut_ptr() as *mut _), Some(&mut bs));
    }
    let n = build.iter().position(|&c| c == 0).unwrap_or(0);
    String::from_utf16_lossy(&build[..n]).parse::<u32>().unwrap_or(0) >= 22000
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Result {
    Ok,
    Removed,
    InternalConflict,
    HookedFallback,
}

/// The ids the hook serves, read by the hook procedure (it runs on this thread, but outside the app).
struct Hooked {
    hwnd: isize,
    keys: Vec<(i32, u32, u32)>,
}

static HOOKED: Mutex<Hooked> = Mutex::new(Hooked { hwnd: 0, keys: Vec::new() });

unsafe extern "system" fn low_level_proc(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && (w.0 == WM_KEYDOWN as usize || w.0 == WM_SYSKEYDOWN as usize) {
        let k = unsafe { &*(l.0 as *const KBDLLHOOKSTRUCT) };
        // injected keys count too (on-screen keyboard, accessibility and macro tools)
        let down = |vk: u16| unsafe { GetAsyncKeyState(vk as i32) } as u16 & 0x8000 != 0;
        let mut mods = 0;
        if down(VK_CONTROL.0) {
            mods |= MOD_CONTROL.0;
        }
        if down(VK_SHIFT.0) {
            mods |= MOD_SHIFT.0;
        }
        if down(VK_MENU.0) {
            mods |= MOD_ALT.0;
        }
        if down(VK_LWIN.0) || down(VK_RWIN.0) {
            mods |= MOD_WIN.0;
        }
        if let Ok(h) = HOOKED.lock()
            && let Some(&(id, _, _)) = h.keys.iter().find(|&&(_, m, vk)| vk == k.vkCode && m == mods)
        {
            unsafe {
                let _ = PostMessageW(Some(HWND(h.hwnd as _)), WM_HOTKEY, WPARAM(id as usize),
                                     LPARAM(((k.vkCode << 16) | mods) as isize));
            }
            return LRESULT(1); // consumes the key before its original owner
        }
    }
    unsafe { CallNextHookEx(None, code, w, l) }
}

pub struct Manager {
    hwnd: isize,
    /// id → accepted (mods, vk)
    map: BTreeMap<i32, (u32, u32)>,
    /// ids served by the hook
    hooked: BTreeMap<i32, (u32, u32)>,
    hook: Option<HHOOK>,
}

impl Manager {
    pub fn new(hwnd: isize) -> Manager {
        Manager { hwnd, map: BTreeMap::new(), hooked: BTreeMap::new(), hook: None }
    }

    fn h(&self) -> Option<HWND> {
        Some(HWND(self.hwnd as _))
    }

    /// (0, 0) removes; an internal conflict is not registered; an external one falls back to the hook.
    pub fn set(&mut self, id: i32, mods: u32, vk: u32) -> Result {
        let mods = mods & MODS;
        if mods == 0 && vk == 0 {
            if self.map.remove(&id).is_some() {
                unsafe {
                    let _ = UnregisterHotKey(self.h(), id);
                }
            }
            self.hooked.remove(&id);
            self.update_hook();
            return Result::Removed;
        }
        if let Some((oid, _)) = self.map.iter().find(|&(&oid, &kv)| oid != id && kv == (mods, vk)) {
            log::write(&format!("Hotkey {}: internal conflict with id {oid}", to_text(mods, vk)));
            return Result::InternalConflict;
        }
        if let Some(&kv) = self.map.get(&id) {
            if kv == (mods, vk) {
                return if self.hooked.contains_key(&id) { Result::HookedFallback } else { Result::Ok };
            }
            unsafe {
                let _ = UnregisterHotKey(self.h(), id);
            }
            self.map.remove(&id);
            self.hooked.remove(&id);
        }
        if vk == VK_SNAPSHOT.0 as u32 && mods == 0 && windows_owns_print_screen() {
            log::write(&format!("Hotkey {}: Windows opens the Snipping Tool with this key — using the keyboard hook",
                                to_text(mods, vk)));
            return self.hook_it(id, mods, vk);
        }
        let ok = unsafe { RegisterHotKey(self.h(), id, windows::Win32::UI::Input::KeyboardAndMouse::HOT_KEY_MODIFIERS(mods | MOD_NOREPEAT.0), vk) };
        match ok {
            Ok(()) => {
                self.map.insert(id, (mods, vk));
                self.update_hook();
                log::write(&format!("Hotkey registered: id {id} = {}", to_text(mods, vk)));
                Result::Ok
            }
            Err(e) => {
                // another program or Windows already owns the combination
                log::write(&format!("Hotkey {}: RegisterHotKey failed ({}) — using the keyboard hook", to_text(mods, vk),
                                    e.code().0 as u32 & 0xFFFF));
                self.hook_it(id, mods, vk)
            }
        }
    }

    fn hook_it(&mut self, id: i32, mods: u32, vk: u32) -> Result {
        self.map.insert(id, (mods, vk));
        self.hooked.insert(id, (mods, vk));
        self.update_hook();
        Result::HookedFallback
    }

    pub fn clear(&mut self) {
        for &id in self.map.keys() {
            unsafe {
                let _ = UnregisterHotKey(self.h(), id);
            }
        }
        self.map.clear();
        self.hooked.clear();
        self.update_hook();
    }

    fn update_hook(&mut self) {
        if let Ok(mut h) = HOOKED.lock() {
            h.hwnd = self.hwnd;
            h.keys = self.hooked.iter().map(|(&id, &(m, v))| (id, m, v)).collect();
        }
        if !self.hooked.is_empty() && self.hook.is_none() {
            if unsafe { IsDebuggerPresent() }.as_bool() {
                return; // like the original: not under a debugger
            }
            self.hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(low_level_proc), None, 0) }.ok();
        } else if self.hooked.is_empty()
            && let Some(h) = self.hook.take()
        {
            unsafe {
                let _ = UnhookWindowsHookEx(h);
            }
        }
    }
}

impl Drop for Manager {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_and_names_keys() {
        assert_eq!(unpack(pack(MOD_SHIFT.0, VK_SNAPSHOT.0 as u32)), (MOD_SHIFT.0, 0x2C));
        assert_eq!(to_text(MOD_CONTROL.0 | MOD_ALT.0, 0x7A), "Ctrl+Alt+F11");
        assert_eq!(to_text(0, VK_SNAPSHOT.0 as u32), "Print Screen");
    }
}
