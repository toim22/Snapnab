//! The Windows pieces the Editor keeps native: the menu bar and the popup menus, message boxes, the common file,
//! folder and color dialogs. The modal ones run their own message loop, so the app calls them on worker threads
//! (the egui windows keep drawing) and gets the answer back through its waker.

use std::sync::Mutex;

use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree};
use windows::Win32::UI::Controls::Dialogs::{
    CC_FULLOPEN, CC_RGBINIT, CHOOSECOLORW, ChooseColorW, GetOpenFileNameW, GetSaveFileNameW, OFN_ALLOWMULTISELECT, OFN_EXPLORER,
    OFN_FILEMUSTEXIST, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW,
};
use windows::Win32::UI::Shell::{FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CheckMenuItem, CreateMenu, CreatePopupMenu, DestroyMenu, EnableMenuItem, GetCursorPos, HMENU, MB_ICONINFORMATION,
    MESSAGEBOX_STYLE, MF_BYCOMMAND, MF_CHECKED, MF_ENABLED, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, MF_UNCHECKED, MessageBoxW,
    PostMessageW, SetForegroundWindow, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WM_COMMAND,
};
use windows::core::PCWSTR;

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

fn hwnd(h: isize) -> HWND {
    HWND(h as _)
}

/// MessageBoxW; returns IDOK, IDYES… as an i32.
pub fn message(owner: isize, text: &str, title: &str, flags: MESSAGEBOX_STYLE) -> i32 {
    let (t, c) = (wide(text), wide(title));
    unsafe { MessageBoxW((owner != 0).then(|| hwnd(owner)), PCWSTR(t.as_ptr()), PCWSTR(c.as_ptr()), flags).0 }
}

/// An information box on its own thread.
pub fn inform(owner: isize, text: String, title: String) {
    std::thread::spawn(move || {
        message(owner, &text, &title, MB_ICONINFORMATION);
    });
}

/// Posts a command to a window (the menus, the panels' buttons).
pub fn post_command(owner: isize, id: u32) {
    unsafe {
        let _ = PostMessageW(Some(hwnd(owner)), WM_COMMAND, WPARAM(id as usize), LPARAM(0));
    }
}

pub fn cursor_pos() -> (i32, i32) {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    (p.x, p.y)
}

// ---------------------------------------------------------------- menus
pub enum Item {
    Cmd(u32, String),
    /// a command and whether it is enabled
    Cmd2(u32, String, bool),
    Checked(u32, String, bool),
    Sub(String, Menu),
    Separator,
}

/// A native menu under construction (popup, or the window's menu bar).
pub struct Menu(pub HMENU);

// SAFETY: a menu handle is a plain USER handle; it is built on one thread and handed to the window's thread.
unsafe impl Send for Menu {}

impl Menu {
    fn fill(m: HMENU, items: Vec<Item>) {
        for it in items {
            unsafe {
                let _ = match it {
                    Item::Cmd(id, t) => AppendMenuW(m, MF_STRING, id as usize, PCWSTR(wide(&t).as_ptr())),
                    Item::Cmd2(id, t, on) => {
                        AppendMenuW(m, MF_STRING | if on { MF_ENABLED } else { MF_GRAYED }, id as usize, PCWSTR(wide(&t).as_ptr()))
                    }
                    Item::Checked(id, t, on) => {
                        AppendMenuW(m, MF_STRING | if on { MF_CHECKED } else { MF_UNCHECKED }, id as usize, PCWSTR(wide(&t).as_ptr()))
                    }
                    Item::Sub(t, sub) => {
                        let h = sub.0;
                        std::mem::forget(sub); // owned by the parent from now on
                        AppendMenuW(m, MF_POPUP, h.0 as usize, PCWSTR(wide(&t).as_ptr()))
                    }
                    Item::Separator => AppendMenuW(m, MF_SEPARATOR, 0, PCWSTR::null()),
                };
            }
        }
    }

    pub fn popup(items: Vec<Item>) -> Menu {
        let m = unsafe { CreatePopupMenu() }.unwrap_or_default();
        Self::fill(m, items);
        Menu(m)
    }

    pub fn bar(items: Vec<Item>) -> Menu {
        let m = unsafe { CreateMenu() }.unwrap_or_default();
        Self::fill(m, items);
        Menu(m)
    }

    /// Shows the popup at a screen point and returns the chosen command (0: none).
    pub fn choose(&self, owner: isize, x: i32, y: i32) -> u32 {
        unsafe {
            let _ = SetForegroundWindow(hwnd(owner));
            TrackPopupMenu(self.0, TPM_RETURNCMD | TPM_RIGHTBUTTON, x, y, None, hwnd(owner), None).0 as u32
        }
    }

    /// The handle stays alive (a menu bar set on a window is destroyed with it).
    pub fn into_raw(self) -> HMENU {
        let h = self.0;
        std::mem::forget(self);
        h
    }
}

impl Drop for Menu {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyMenu(self.0);
        }
    }
}

/// Enabled and checked states of the menu commands, kept by the app after each frame and applied when a menu opens
/// (WM_INITMENUPOPUP comes inside the window procedure, where the app itself cannot be reached).
#[derive(Clone, Default)]
pub struct MenuStates {
    pub enabled: Vec<(u32, bool)>,
    pub checked: Vec<(u32, bool)>,
}

static STATES: Mutex<Option<MenuStates>> = Mutex::new(None);

pub fn set_menu_states(s: MenuStates) {
    *STATES.lock().unwrap() = Some(s);
}

/// WM_INITMENUPOPUP: applies the states to the menu that opens (commands not in it are ignored).
pub fn apply_menu_states(menu: isize) {
    let g = STATES.lock().unwrap();
    let Some(s) = g.as_ref() else { return };
    let m = HMENU(menu as _);
    unsafe {
        for &(id, on) in &s.enabled {
            let _ = EnableMenuItem(m, id, MF_BYCOMMAND | if on { MF_ENABLED } else { MF_GRAYED });
        }
        for &(id, on) in &s.checked {
            let _ = CheckMenuItem(m, id, (MF_BYCOMMAND | if on { MF_CHECKED } else { MF_UNCHECKED }).0);
        }
    }
}

// ---------------------------------------------------------------- file dialogs
fn filter_w(filter: &str) -> Vec<u16> {
    filter.encode_utf16().map(|c| if c == '|' as u16 { 0 } else { c }).chain([0, 0]).collect()
}

/// Open dialog; `filter` is "Name|*.ext;*.ext2|…".
pub fn open_file(owner: isize, title: &str, filter: &str) -> Option<String> {
    open_files_ex(owner, title, filter, false).and_then(|v| v.into_iter().next())
}

/// Open dialog with several files chosen.
pub fn open_files(owner: isize, title: &str, filter: &str) -> Option<Vec<String>> {
    open_files_ex(owner, title, filter, true)
}

fn open_files_ex(owner: isize, title: &str, filter: &str, multi: bool) -> Option<Vec<String>> {
    let filter = filter_w(filter);
    let t = wide(title);
    let mut buf = vec![0u16; 65536];
    let mut ofn = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: hwnd(owner),
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: windows::core::PWSTR(buf.as_mut_ptr()),
        nMaxFile: buf.len() as u32,
        lpstrTitle: if title.is_empty() { PCWSTR::null() } else { PCWSTR(t.as_ptr()) },
        Flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_EXPLORER | if multi { OFN_ALLOWMULTISELECT } else { Default::default() },
        ..Default::default()
    };
    if !unsafe { GetOpenFileNameW(&mut ofn) }.as_bool() {
        return None;
    }
    // multi-select: the folder, then each name, separated by NULs
    let parts: Vec<String> = buf
        .split(|&c| c == 0)
        .take_while(|s| !s.is_empty())
        .map(String::from_utf16_lossy)
        .collect();
    match parts.len() {
        0 => None,
        1 => Some(parts),
        _ => Some(parts[1..].iter().map(|n| format!("{}\\{}", parts[0], n)).collect()),
    }
}

/// Save dialog: returns the path and the chosen filter (1-based).
pub fn save_file(owner: isize, title: &str, initial: &str, filter: &str, def_ext: &str, filter_index: u32) -> Option<(String, u32)> {
    let filter = filter_w(filter);
    let t = wide(title);
    let ext = wide(def_ext);
    let mut buf = vec![0u16; 4096];
    for (d, s) in buf.iter_mut().zip(initial.encode_utf16()) {
        *d = s;
    }
    let mut ofn = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: hwnd(owner),
        lpstrFilter: PCWSTR(filter.as_ptr()),
        nFilterIndex: filter_index.max(1),
        lpstrFile: windows::core::PWSTR(buf.as_mut_ptr()),
        nMaxFile: buf.len() as u32,
        lpstrTitle: PCWSTR(t.as_ptr()),
        lpstrDefExt: PCWSTR(ext.as_ptr()),
        Flags: OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST | OFN_EXPLORER,
        ..Default::default()
    };
    if !unsafe { GetSaveFileNameW(&mut ofn) }.as_bool() {
        return None;
    }
    let n = buf.iter().position(|&c| c == 0).unwrap_or(0);
    Some((String::from_utf16_lossy(&buf[..n]), ofn.nFilterIndex))
}

/// Folder picker (IFileOpenDialog with FOS_PICKFOLDERS).
pub fn pick_folder(owner: isize, title: &str) -> Option<String> {
    unsafe {
        let dlg: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let opt = dlg.GetOptions().ok()?;
        dlg.SetOptions(opt | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM).ok()?;
        let t = wide(title);
        let _ = dlg.SetTitle(PCWSTR(t.as_ptr()));
        dlg.Show(Some(hwnd(owner))).ok()?;
        let item = dlg.GetResult().ok()?;
        let p = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as _));
        s
    }
}

/// The Windows color dialog; COLORREF in and out.
pub fn choose_color(owner: isize, initial: u32) -> Option<u32> {
    static CUSTOM: Mutex<[u32; 16]> = Mutex::new([0x00FFFFFF; 16]);
    let mut custom = *CUSTOM.lock().unwrap();
    let mut cc = CHOOSECOLORW {
        lStructSize: std::mem::size_of::<CHOOSECOLORW>() as u32,
        hwndOwner: hwnd(owner),
        rgbResult: windows::Win32::Foundation::COLORREF(initial & 0xFFFFFF),
        lpCustColors: custom.as_mut_ptr() as *mut _,
        Flags: CC_FULLOPEN | CC_RGBINIT,
        ..Default::default()
    };
    let ok = unsafe { ChooseColorW(&mut cc) }.as_bool();
    *CUSTOM.lock().unwrap() = custom;
    ok.then_some(cc.rgbResult.0)
}

pub fn info_flags() -> MESSAGEBOX_STYLE {
    MB_ICONINFORMATION
}

// ---------------------------------------------------------------- progress
/// The shell's progress dialog (IProgressDialog), for long work on a worker thread; closed when dropped.
pub struct Progress(windows::Win32::UI::Shell::IProgressDialog);

impl Progress {
    pub fn open(owner: isize, title: &str) -> Option<Progress> {
        use windows::Win32::UI::Shell::{CLSID_ProgressDialog, IProgressDialog, PROGDLG_AUTOTIME, PROGDLG_NORMAL};
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
            let d: IProgressDialog = CoCreateInstance(&CLSID_ProgressDialog, None, CLSCTX_INPROC_SERVER).ok()?;
            let t = wide(title);
            let _ = d.SetTitle(PCWSTR(t.as_ptr()));
            d.StartProgressDialog(Some(hwnd(owner)), None, PROGDLG_NORMAL | PROGDLG_AUTOTIME, None).ok()?;
            Some(Progress(d))
        }
    }

    /// The line under the bar and the progress.
    pub fn set(&self, line: &str, done: usize, total: usize) {
        let l = wide(line);
        unsafe {
            let _ = self.0.SetLine(1, PCWSTR(l.as_ptr()), true, None);
            let _ = self.0.SetProgress(done as u32, total.max(1) as u32);
        }
    }

    pub fn canceled(&self) -> bool {
        unsafe { self.0.HasUserCancelled().as_bool() }
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        unsafe {
            let _ = self.0.StopProgressDialog();
        }
    }
}
