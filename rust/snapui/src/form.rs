//! The small modal forms built in code (Ui.cpp, RunForm), with the same window class and control IDs (field i is
//! control 1000 + i, OK is 1 and Cancel 2), which the e2e suite fills by ID. Labels, section titles, texts, numbers,
//! lists, check boxes, colors and hotkeys; the label column grows with the longest label, a form taller than the
//! screen is split into two columns at the section nearest the middle, and the check of OK keeps the form open with
//! its message. A form runs its own message loop: call it from its own thread.

use std::cell::{Cell, RefCell};
use std::sync::Once;

use snapcore::screen;
use snapcore::wide::to_wide;
fn tr(s: &str) -> String {
    snapcore::i18n::tr_str(s)
}
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, COLOR_WINDOW, CreateFontW, DEFAULT_CHARSET, DeleteObject, FF_DONTCARE, FW_BOLD, FW_NORMAL,
    GetDC, GetSysColorBrush, GetTextExtentPoint32W, HFONT, OUT_DEFAULT_PRECIS, ReleaseDC, SelectObject,
};
use windows::Win32::UI::Controls::Dialogs::{CC_FULLOPEN, CC_RGBINIT, CHOOSECOLORW, ChooseColorW};
use windows::Win32::UI::Controls::{
    HKM_GETHOTKEY, HKM_SETHOTKEY, HOTKEY_CLASSW, UDM_SETBUDDY, UDM_SETPOS32, UDM_SETRANGE32, UDS_ALIGNRIGHT, UDS_ARROWKEYS,
    UDS_NOTHOUSANDS, UDS_SETBUDDYINT, UPDOWN_CLASSW,
};
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForSystem, GetDpiForWindow};
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, MOD_ALT, MOD_CONTROL, MOD_SHIFT, SetFocus, VK_ESCAPE};
use windows::Win32::UI::WindowsAndMessaging::{
    BS_DEFPUSHBUTTON, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, ES_AUTOHSCROLL, ES_NUMBER, GetDlgItem,
    GetMessageW, GetWindowRect, GetWindowTextW, HMENU, IDC_ARROW, IsChild, IsDialogMessageW, LoadCursorW, MB_ICONWARNING, MSG,
    MessageBoxW, RegisterClassW, SW_SHOW, SendMessageW, SetForegroundWindow, SetWindowTextW, ShowWindow, TranslateMessage,
    WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_KEYDOWN, WM_SETFONT, WNDCLASSW, WS_BORDER, WS_CAPTION, WS_CHILD, WS_CLIPCHILDREN,
    WS_EX_CLIENTEDGE, WS_EX_DLGMODALFRAME, WS_EX_TOPMOST, WS_POPUP, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
};
use windows::core::{PCWSTR, w};

#[derive(Clone, Debug)]
pub enum Field {
    Label(String),
    /// a section title in bold
    Separator(String),
    Text { label: String, text: String },
    Int { label: String, value: i32, min: i32, max: i32 },
    /// a drop-down list; `value` is the chosen index
    Combo { label: String, items: Vec<String>, value: i32 },
    Check { label: String, value: bool },
    /// a COLORREF (0x00BBGGRR) chosen with the Windows color dialog
    Color { label: String, color: u32 },
    /// MOD_* flags and a virtual key (0 = none)
    Hotkey { label: String, mods: u32, vk: u32 },
}

impl Field {
    pub fn int(&self) -> i32 {
        match self {
            Field::Int { value, .. } | Field::Combo { value, .. } => *value,
            Field::Check { value, .. } => *value as i32,
            _ => 0,
        }
    }

    pub fn on(&self) -> bool {
        self.int() != 0
    }

    pub fn text(&self) -> &str {
        match self {
            Field::Text { text, .. } => text,
            _ => "",
        }
    }

    pub fn color(&self) -> u32 {
        if let Field::Color { color, .. } = self { *color } else { 0 }
    }

    pub fn hotkey(&self) -> (u32, u32) {
        if let Field::Hotkey { mods, vk, .. } = self { (*mods, *vk) } else { (0, 0) }
    }

    fn label(&self) -> &str {
        match self {
            Field::Label(t) | Field::Separator(t) => t,
            Field::Text { label, .. }
            | Field::Int { label, .. }
            | Field::Combo { label, .. }
            | Field::Check { label, .. }
            | Field::Color { label, .. }
            | Field::Hotkey { label, .. } => label,
        }
    }
}

const CB_ADDSTRING: u32 = 0x0143;
const CB_GETCURSEL: u32 = 0x0147;
const CB_SETCURSEL: u32 = 0x014E;
const CBS_DROPDOWNLIST: u32 = 3;
const BM_GETCHECK: u32 = 0x00F0;
const BM_SETCHECK: u32 = 0x00F1;
const BS_AUTOCHECKBOX: u32 = 3;
const BN_CLICKED: usize = 0;
const HOTKEYF_SHIFT: u32 = 1;
const HOTKEYF_CONTROL: u32 = 2;
const HOTKEYF_ALT: u32 = 4;

const ID_FIELD: i32 = 1000;
// static control styles (SS_LEFT, SS_CENTERIMAGE)
const SS_LEFT: u32 = 0;
const SS_CENTERIMAGE: u32 = 0x200;
const ID_OK: usize = 1;
const ID_CANCEL: usize = 2;

thread_local! {
    /// the answer of the form running on this thread (None: still open)
    static DONE: Cell<Option<bool>> = const { Cell::new(None) };
    /// the color fields of the form running on this thread: (control id, COLORREF)
    static COLORS: RefCell<Vec<(i32, u32)>> = const { RefCell::new(Vec::new()) };
    static CUSTOM: RefCell<[COLORREF; 16]> = RefCell::new([COLORREF(0xFFFFFF); 16]);
}

fn color_text(c: u32) -> String {
    format!("#{:02X}{:02X}{:02X}", c & 0xFF, (c >> 8) & 0xFF, (c >> 16) & 0xFF)
}

fn hkf_to_mod(f: u32) -> u32 {
    (if f & HOTKEYF_SHIFT != 0 { MOD_SHIFT.0 } else { 0 }) | (if f & HOTKEYF_CONTROL != 0 { MOD_CONTROL.0 } else { 0 })
        | (if f & HOTKEYF_ALT != 0 { MOD_ALT.0 } else { 0 })
}

fn mod_to_hkf(m: u32) -> u32 {
    (if m & MOD_SHIFT.0 != 0 { HOTKEYF_SHIFT } else { 0 }) | (if m & MOD_CONTROL.0 != 0 { HOTKEYF_CONTROL } else { 0 })
        | (if m & MOD_ALT.0 != 0 { HOTKEYF_ALT } else { 0 })
}

unsafe extern "system" fn form_proc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match m {
        WM_COMMAND if wp.0 & 0xFFFF == ID_OK || wp.0 & 0xFFFF == ID_CANCEL => {
            DONE.with(|d| d.set(Some(wp.0 & 0xFFFF == ID_OK)));
            LRESULT(0)
        }
        WM_COMMAND if (wp.0 >> 16) == BN_CLICKED => {
            let id = (wp.0 & 0xFFFF) as i32;
            let cur = COLORS.with(|c| c.borrow().iter().find(|(i, _)| *i == id).map(|(_, c)| *c));
            if let Some(c) = cur {
                let mut cc = CHOOSECOLORW {
                    lStructSize: std::mem::size_of::<CHOOSECOLORW>() as u32,
                    hwndOwner: h,
                    rgbResult: COLORREF(c),
                    Flags: CC_FULLOPEN | CC_RGBINIT,
                    ..Default::default()
                };
                let picked = CUSTOM.with(|cu| {
                    let mut cu = cu.borrow_mut();
                    cc.lpCustColors = cu.as_mut_ptr();
                    unsafe { ChooseColorW(&mut cc) }.as_bool()
                });
                if picked {
                    let n = cc.rgbResult.0;
                    COLORS.with(|cs| {
                        if let Some(e) = cs.borrow_mut().iter_mut().find(|(i, _)| *i == id) {
                            e.1 = n;
                        }
                    });
                    let t = to_wide(color_text(n));
                    unsafe {
                        if let Ok(b) = GetDlgItem(Some(h), id) {
                            let _ = SetWindowTextW(b, PCWSTR(t.as_ptr()));
                        }
                    }
                }
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            DONE.with(|d| d.set(Some(false)));
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(h, m, wp, lp) },
    }
}

fn scale(v: i32, dpi: u32) -> i32 {
    (v as i64 * dpi as i64 / 96) as i32
}

/// UiFont: Segoe UI 9 pt at the window's DPI.
fn ui_font(dpi: u32, bold: bool) -> HFONT {
    unsafe {
        CreateFontW(-(9 * dpi as i32 / 72), 0, 0, 0, if bold { FW_BOLD.0 } else { FW_NORMAL.0 } as i32, 0, 0, 0, DEFAULT_CHARSET,
                    OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, FF_DONTCARE.0 as u32, w!("Segoe UI"))
    }
}

fn text_width(font: HFONT, s: &str) -> i32 {
    let t: Vec<u16> = s.encode_utf16().collect();
    let mut sz = SIZE::default();
    unsafe {
        let dc = GetDC(None);
        let old = SelectObject(dc, font.into());
        let _ = GetTextExtentPoint32W(dc, &t, &mut sz);
        SelectObject(dc, old);
        ReleaseDC(None, dc);
    }
    sz.cx
}

/// Collect: the values of the controls into `fields`.
fn collect(h: HWND, fields: &mut [Field]) {
    for (i, f) in fields.iter_mut().enumerate() {
        let id = ID_FIELD + i as i32;
        let e = unsafe { GetDlgItem(Some(h), id) }.unwrap_or_default();
        let mut buf = vec![0u16; 4096];
        let n = unsafe { GetWindowTextW(e, &mut buf) } as usize;
        let got = String::from_utf16_lossy(&buf[..n]);
        unsafe {
            match f {
                Field::Int { value, min, max, .. } => {
                    let s: String = got.chars().filter(|c| c.is_ascii_digit() || *c == '-').collect();
                    *value = s.parse::<i32>().unwrap_or(*value).clamp(*min, *max);
                }
                Field::Text { text, .. } => *text = got,
                Field::Combo { value, .. } => *value = SendMessageW(e, CB_GETCURSEL, None, None).0.max(0) as i32,
                Field::Check { value, .. } => *value = SendMessageW(e, BM_GETCHECK, None, None).0 == 1,
                Field::Color { color, .. } => {
                    if let Some(c) = COLORS.with(|cs| cs.borrow().iter().find(|(i, _)| *i == id).map(|(_, c)| *c)) {
                        *color = c;
                    }
                }
                Field::Hotkey { mods, vk, .. } => {
                    let v = SendMessageW(e, HKM_GETHOTKEY, None, None).0 as u32;
                    (*vk, *mods) = (v & 0xFF, hkf_to_mod((v >> 8) & 0xFF));
                }
                Field::Label(_) | Field::Separator(_) => {}
            }
        }
    }
}

/// RunForm: true on OK, with the values written back into `fields`.
pub fn run(owner: isize, title: &str, fields: &mut [Field]) -> bool {
    run_ex(owner, title, fields, 420, None)
}

/// RunForm with its width and a check of OK: a non-empty answer is shown over the form, which stays open.
pub fn run_ex(owner: isize, title: &str, fields: &mut [Field], width: i32, validate: Option<&dyn Fn(&[Field]) -> String>) -> bool {
    static REG: Once = Once::new();
    REG.call_once(|| unsafe {
        let wc = WNDCLASSW {
            lpfnWndProc: Some(form_proc),
            hInstance: windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default().into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: GetSysColorBrush(COLOR_WINDOW),
            lpszClassName: w!("SnapnabForm"),
            ..Default::default()
        };
        RegisterClassW(&wc);
    });
    DONE.with(|d| d.set(None));
    COLORS.with(|c| {
        *c.borrow_mut() = fields
            .iter()
            .enumerate()
            .filter_map(|(i, f)| if let Field::Color { color, .. } = f { Some((ID_FIELD + i as i32, *color)) } else { None })
            .collect()
    });
    let owner_h = (owner != 0).then(|| HWND(owner as _));
    let dpi = match owner_h {
        Some(o) => unsafe { GetDpiForWindow(o) },
        None => unsafe { GetDpiForSystem() },
    }
    .max(96);
    let font = ui_font(dpi, false);
    let bold = ui_font(dpi, true);
    let (mut ww_, pad, row_h, ctl_h) = (scale(width, dpi), scale(14, dpi), scale(30, dpi), scale(24, dpi));
    let mut label_w = scale(150, dpi);
    {
        // the label column grows with the longest label (translations), and the window with it
        let widest = fields
            .iter()
            .filter(|f| !matches!(f, Field::Label(_) | Field::Separator(_) | Field::Check { .. }))
            .map(|f| text_width(font, f.label()))
            .max()
            .unwrap_or(0);
        let need = (widest + scale(14, dpi)).min(scale(320, dpi));
        if need > label_w {
            ww_ += need - label_w;
            label_w = need;
        }
    }
    // long explanatory labels wrap over several rows (up to 4)
    let avail = (ww_ - 2 * pad - scale(4, dpi)).max(1);
    let span: Vec<i32> = fields
        .iter()
        .map(|f| if let Field::Label(t) = f { ((text_width(font, t) * 11 / 10) / avail + 1).clamp(1, 4) } else { 1 })
        .collect();
    let span_rows = |a: usize, b: usize| span[a..b].iter().sum::<i32>();
    // the monitor of the owner (or of the cursor)
    let area = match owner_h {
        Some(o) => {
            let mut r = RECT::default();
            unsafe {
                let _ = GetWindowRect(o, &mut r);
            }
            screen::to_rect(r)
        }
        None => {
            let (cx, cy) = screen::cursor_pos();
            screen::monitor_rect(cx, cy, true)
        }
    };
    let work = screen::monitor_rect((area.left + area.right) / 2, (area.top + area.bottom) / 2, true);
    // a form taller than the screen is split into two columns, at the section (separator) nearest the middle
    let mut rows = span_rows(0, fields.len());
    let mut col_break = fields.len();
    let max_rows = 8.max((work.height() * 85 / 100 - pad * 2 - scale(90, dpi)) / row_h);
    if rows > max_rows {
        let mut best = fields.len();
        let mut best_d = i32::MAX;
        for (i, f) in fields.iter().enumerate().skip(1) {
            if matches!(f, Field::Separator(_)) && (i as i32 - rows / 2).abs() < best_d {
                (best, best_d) = (i, (i as i32 - rows / 2).abs());
            }
        }
        if best == fields.len() {
            best = fields.len().div_ceil(2);
        }
        col_break = best;
        rows = span_rows(0, col_break).max(span_rows(col_break, fields.len()));
    }
    let col_w = ww_;
    let total_w = if col_break < fields.len() { col_w * 2 } else { col_w };
    let client_h = pad * 2 + rows * row_h + scale(46, dpi);
    let style = WS_POPUP | WS_CAPTION | WS_SYSMENU | WS_CLIPCHILDREN;
    let mut wr = RECT { left: 0, top: 0, right: total_w, bottom: client_h };
    let mut done = false;
    unsafe {
        let _ = AdjustWindowRectExForDpi(&mut wr, style, false, WS_EX_DLGMODALFRAME, dpi);
        let (ww, wh) = (wr.right - wr.left, wr.bottom - wr.top);
        let (x, y) = ((area.left + area.right - ww) / 2, ((area.top + area.bottom - wh) / 2).max(work.top));
        let t = to_wide(title);
        let Ok(h) = CreateWindowExW(WS_EX_DLGMODALFRAME | WS_EX_TOPMOST, w!("SnapnabForm"), PCWSTR(t.as_ptr()), style, x, y, ww, wh,
                                    owner_h, None, None, None)
        else {
            return false;
        };
        let child = |ex, class: PCWSTR, text: &str, st: u32, x, y, w, hh, id: i32, f: HFONT| {
            let t = to_wide(text);
            let c = CreateWindowExW(ex, class, PCWSTR(t.as_ptr()), WS_CHILD | WS_VISIBLE | WINDOW_STYLE(st), x, y, w, hh, Some(h),
                                    (id != 0).then(|| HMENU(id as isize as _)), None, None)
                .unwrap_or_default();
            SendMessageW(c, WM_SETFONT, Some(WPARAM(f.0 as usize)), Some(LPARAM(1)));
            c
        };
        let mut yy = pad;
        let mut first = None;
        for (i, f) in fields.iter().enumerate() {
            let id = ID_FIELD + i as i32;
            if i == col_break {
                yy = pad;
            }
            let x0 = if i >= col_break { col_w } else { 0 };
            let (cx, sp) = (x0 + pad + label_w, span[i]);
            let cw = x0 + col_w - pad - cx;
            let label = |text: &str| {
                child(Default::default(), w!("STATIC"), text, SS_LEFT | SS_CENTERIMAGE, x0 + pad, yy, label_w - scale(8, dpi), ctl_h, 0, font);
            };
            let ctl = match f {
                Field::Label(text) => {
                    let st = SS_LEFT | if sp > 1 { 0 } else { SS_CENTERIMAGE };
                    child(Default::default(), w!("STATIC"), text, st, x0 + pad, yy + if sp > 1 { scale(3, dpi) } else { 0 }, col_w - 2 * pad,
                          ctl_h + (sp - 1) * row_h, id, font);
                    None
                }
                Field::Separator(text) => {
                    child(Default::default(), w!("STATIC"), text, SS_LEFT | SS_CENTERIMAGE, x0 + pad, yy, col_w - 2 * pad, ctl_h, id, bold);
                    None
                }
                Field::Text { label: l, text } => {
                    label(l);
                    Some(child(WS_EX_CLIENTEDGE, w!("EDIT"), text, WS_TABSTOP.0 | ES_AUTOHSCROLL as u32, cx, yy, cw, ctl_h, id, font))
                }
                Field::Int { label: l, value, min, max } => {
                    label(l);
                    let st = WS_TABSTOP.0 | ES_AUTOHSCROLL as u32 | if *min >= 0 { ES_NUMBER as u32 } else { 0 };
                    let e = child(WS_EX_CLIENTEDGE, w!("EDIT"), &value.to_string(), st, cx, yy, scale(90, dpi), ctl_h, id, font);
                    let ud = child(Default::default(), UPDOWN_CLASSW, "", (UDS_ALIGNRIGHT | UDS_SETBUDDYINT | UDS_ARROWKEYS | UDS_NOTHOUSANDS) as u32,
                                   0, 0, 0, 0, 0, font);
                    SendMessageW(ud, UDM_SETBUDDY, Some(WPARAM(e.0 as usize)), None);
                    SendMessageW(ud, UDM_SETRANGE32, Some(WPARAM(*min as isize as usize)), Some(LPARAM(*max as isize)));
                    SendMessageW(ud, UDM_SETPOS32, None, Some(LPARAM(*value as isize)));
                    Some(e)
                }
                Field::Combo { label: l, items, value } => {
                    label(l);
                    let c = child(Default::default(), w!("COMBOBOX"), "", WS_TABSTOP.0 | CBS_DROPDOWNLIST | 0x0020_0000 /* WS_VSCROLL */, cx, yy,
                                  cw, scale(300, dpi), id, font);
                    for it in items {
                        let t = to_wide(it);
                        SendMessageW(c, CB_ADDSTRING, None, Some(LPARAM(t.as_ptr() as isize)));
                    }
                    SendMessageW(c, CB_SETCURSEL, Some(WPARAM(*value as usize)), None);
                    Some(c)
                }
                Field::Check { label: l, value } => {
                    let c = child(Default::default(), w!("BUTTON"), l, WS_TABSTOP.0 | BS_AUTOCHECKBOX, x0 + pad, yy, col_w - 2 * pad, ctl_h, id,
                                  font);
                    SendMessageW(c, BM_SETCHECK, Some(WPARAM(*value as usize)), None);
                    Some(c)
                }
                Field::Color { label: l, color } => {
                    label(l);
                    Some(child(Default::default(), w!("BUTTON"), &color_text(*color), WS_TABSTOP.0, cx, yy, scale(110, dpi), ctl_h, id, font))
                }
                Field::Hotkey { label: l, mods, vk } => {
                    label(l);
                    let c = child(Default::default(), HOTKEY_CLASSW, "", WS_TABSTOP.0 | WS_BORDER.0, cx, yy, cw, ctl_h, id, font);
                    SendMessageW(c, HKM_SETHOTKEY, Some(WPARAM(((*vk & 0xFF) | (mod_to_hkf(*mods) << 8)) as usize)), None);
                    Some(c)
                }
            };
            if let Some(c) = ctl {
                first.get_or_insert(c);
            }
            yy += row_h * sp;
        }
        let (bw, bh) = (scale(96, dpi), scale(28, dpi));
        yy = pad + rows * row_h + scale(6, dpi);
        let ok = child(Default::default(), w!("BUTTON"), &tr("OK"), WS_TABSTOP.0 | BS_DEFPUSHBUTTON as u32,
                       total_w - pad - bw * 2 - scale(8, dpi), yy, bw, bh, ID_OK as i32, font);
        child(Default::default(), w!("BUTTON"), &tr("Cancelar"), WS_TABSTOP.0, total_w - pad - bw, yy, bw, bh, ID_CANCEL as i32, font);

        if let Some(o) = owner_h {
            let _ = EnableWindow(o, false);
        }
        let _ = ShowWindow(h, SW_SHOW);
        let _ = SetForegroundWindow(h);
        let _ = SetFocus(Some(first.unwrap_or(ok)));
        let mut msg = MSG::default();
        loop {
            while DONE.with(|d| d.get()).is_none() && GetMessageW(&mut msg, None, 0, 0).as_bool() {
                if msg.message == WM_KEYDOWN && msg.wParam.0 == VK_ESCAPE.0 as usize && (msg.hwnd == h || IsChild(h, msg.hwnd).as_bool()) {
                    DONE.with(|d| d.set(Some(false)));
                    continue;
                }
                if !IsDialogMessageW(h, &msg).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            if DONE.with(|d| d.get()) != Some(true) {
                break;
            }
            collect(h, fields);
            let err = validate.map(|v| v(fields)).unwrap_or_default();
            if err.is_empty() {
                done = true;
                break;
            }
            let (t, c) = (to_wide(&err), to_wide("Snapnab"));
            MessageBoxW(Some(h), PCWSTR(t.as_ptr()), PCWSTR(c.as_ptr()), MB_ICONWARNING);
            DONE.with(|d| d.set(None)); // the form stays open
        }
        if let Some(o) = owner_h {
            let _ = EnableWindow(o, true);
            let _ = SetForegroundWindow(o);
        }
        let _ = DestroyWindow(h);
        let _ = DeleteObject(font.into());
        let _ = DeleteObject(bold.into());
    }
    done
}
