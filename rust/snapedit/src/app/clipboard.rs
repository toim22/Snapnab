//! Copy and paste (main.cpp CopyToClipboard / Paste): the image goes to the clipboard as before; the selected objects
//! also go in the Editor's own format ("Snapnab.Objects", to paste in another image or in the same place), and a copied
//! area of the image takes its position along ("Snapnab.Origin").

use snapcore::doc::Style;
use snapcore::gdip::pt;
use snapedit_app::canvas::Tool;
use snapedit_app::{clipfmt, native, tr};
use snapui::host::Ctl;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};

use super::Editor;

const CF_UNICODETEXT: u32 = 13;
const CF_HDROP: u32 = 15;

fn format(name: &str) -> u32 {
    let w = native::wide(name);
    unsafe { RegisterClipboardFormatW(windows::core::PCWSTR(w.as_ptr())) }
}

fn objects_format() -> u32 {
    format("Snapnab.Objects")
}

fn origin_format() -> u32 {
    format("Snapnab.Origin")
}

fn available(f: u32) -> bool {
    unsafe { IsClipboardFormatAvailable(f) }.is_ok()
}

pub fn can_paste() -> bool {
    snapcore::clip::has_image() || available(objects_format()) || available(CF_UNICODETEXT) || available(CF_HDROP)
}

/// Puts bytes (a NUL is added) under a format; the clipboard must be open.
fn set_bytes(fmt: u32, data: &[u8]) {
    unsafe {
        let Ok(h) = GlobalAlloc(GMEM_MOVEABLE, data.len() + 1) else { return };
        let p = GlobalLock(h) as *mut u8;
        if !p.is_null() {
            std::ptr::copy_nonoverlapping(data.as_ptr(), p, data.len());
            *p.add(data.len()) = 0;
            let _ = GlobalUnlock(h);
        }
        if SetClipboardData(fmt, Some(HANDLE(h.0))).is_err() {
            let _ = GlobalFree(Some(h));
        }
    }
}

fn get_bytes(owner: isize, fmt: u32) -> Vec<u8> {
    let mut out = Vec::new();
    if !available(fmt) || unsafe { OpenClipboard(Some(HWND(owner as _))) }.is_err() {
        return out;
    }
    unsafe {
        if let Ok(h) = GetClipboardData(fmt) {
            let g = HGLOBAL(h.0);
            let p = GlobalLock(g) as *const u8;
            if !p.is_null() {
                let n = GlobalSize(g);
                let s = std::slice::from_raw_parts(p, n);
                out = s[..s.iter().position(|&c| c == 0).unwrap_or(n)].to_vec();
                let _ = GlobalUnlock(g);
            }
        }
        let _ = CloseClipboard();
    }
    out
}

/// The first file copied in Explorer.
fn clipboard_file(owner: isize) -> Option<String> {
    if !available(CF_HDROP) || unsafe { OpenClipboard(Some(HWND(owner as _))) }.is_err() {
        return None;
    }
    let mut out = None;
    unsafe {
        if let Ok(h) = GetClipboardData(CF_HDROP) {
            let mut buf = vec![0u16; 1024];
            let n = DragQueryFileW(HDROP(h.0), 0, Some(&mut buf)) as usize;
            if n > 0 {
                out = Some(String::from_utf16_lossy(&buf[..n]));
            }
        }
        let _ = CloseClipboard();
    }
    out
}

impl Editor {
    pub fn copy_to_clipboard(&mut self, cut: bool) -> bool {
        if !self.require_image() {
            return false;
        }
        let raster = self.cv.has_raster_selection();
        let objs = if raster { Vec::new() } else { self.cv.selected_objects() };
        let sel = self.cv.raster_selection_bounds();
        if cut && !objs.is_empty() && self.cv.selection_locked() {
            self.status = tr("Objetos bloqueados não são recortados.");
            return false;
        }
        if !self.cv.copy_selection(false) {
            return false;
        }
        if unsafe { OpenClipboard(Some(HWND(self.hwnd as _))) }.is_ok() {
            if !objs.is_empty() {
                let src = self.cv.doc().map(|d| d.path.clone()).unwrap_or_default();
                set_bytes(objects_format(), snapcore::serial::objects_to_clip(&objs, &src).as_bytes());
            }
            if raster {
                set_bytes(origin_format(), format!("{},{}", sel.left, sel.top).as_bytes());
            }
            unsafe {
                let _ = CloseClipboard();
            }
        }
        if cut {
            self.cv.delete_selected();
        }
        true
    }

    /// The source formatting of the copied text.
    fn clipboard_text_format(&self) -> Option<Style> {
        let mut s = self.cv.styles[Tool::Text.index()].clone();
        let rtf = get_bytes(self.hwnd, format("Rich Text Format"));
        if !rtf.is_empty() && clipfmt::parse_rtf(&rtf, &mut s) {
            return Some(s);
        }
        let html = get_bytes(self.hwnd, format("HTML Format"));
        (!html.is_empty() && clipfmt::parse_html(&String::from_utf8_lossy(&html), &mut s)).then_some(s)
    }

    /// Text paste: without formatting by default (Edit › Paste text without formatting); the alternative paste does
    /// the opposite.
    pub fn paste_text_now(&mut self, alternative: bool) {
        let t = snapcore::clip::get_text();
        if t.is_empty() {
            self.status = tr("Nada para colar.");
            return;
        }
        let keep = self.cv.plain_paste == alternative;
        let fmt = if keep { self.clipboard_text_format() } else { None };
        self.cv.paste_text(&t, fmt.as_ref());
        if keep && fmt.is_none() {
            self.status = tr("O texto copiado não tem formatação; colado com o estilo do objeto.");
        }
        self.doc_changed();
    }

    pub fn paste(&mut self, in_place: bool) {
        let image_doc = self.cv.doc().is_some_and(|d| !d.is_video);
        // objects copied in the Editor (also from another image)
        if image_doc && available(objects_format()) {
            let (objs, src) = snapcore::serial::objects_from_clip(&get_bytes(self.hwnd, objects_format()));
            if !objs.is_empty() {
                let same = src.is_some_and(|s| self.cv.doc().is_some_and(|d| s.eq_ignore_ascii_case(&d.path)));
                self.cv.paste_objects(&objs, in_place, same);
                self.status = if in_place { tr("Colado no mesmo lugar.") } else { tr("Colado.") };
                self.doc_changed();
                return;
            }
        }
        let img = snapcore::clip::get_image()
            .or_else(|| clipboard_file(self.hwnd).and_then(|f| snapedit_app::files::load_image(&f)))
            .filter(|i| !i.is_empty());
        let Some(img) = img else {
            if image_doc && available(CF_UNICODETEXT) {
                self.paste_text_now(in_place); // text → a Text object
            } else {
                self.status = tr("Nada para colar.");
            }
            return;
        };
        if !image_doc {
            self.new_doc_from_image(img, &tr("Colado"), &tr("Área de transferência"));
        } else if in_place {
            // the same position as the copied area (or the top left corner)
            let o = String::from_utf8_lossy(&get_bytes(self.hwnd, origin_format())).into_owned();
            let mut v = o.split(',').map(|x| x.trim().parse::<i32>().unwrap_or(0));
            let (x, y) = (v.next().unwrap_or(0), v.next().unwrap_or(0));
            self.cv.paste_image_at(&img, pt(x as f32, y as f32));
            self.doc_changed();
        } else {
            self.cv.paste_image(&img);
            self.doc_changed();
        }
    }
}

#[allow(dead_code)]
fn unused(_: &mut Ctl) {}
