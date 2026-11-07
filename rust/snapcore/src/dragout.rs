//! Dragging files out to other programs (Explorer, e-mail, chat, Office): the shell's own data object for the files
//! (CF_HDROP and the Shell ID list, which packaged apps such as Paint and Photos use), a drag image and
//! "Preferred DropEffect" = copy. Used by the Editor's tray and library, the quick access overlay (C08) and pinned
//! images (C01) — the overlay's CF_HDROP drag of ShareX dossier 01 · F08.

use std::path::Path;

use windows::Win32::Foundation::{POINT, SIZE};
use windows::Win32::Graphics::Gdi::{DeleteObject, HGDIOBJ};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree, DVASPECT_CONTENT, FORMATETC, IDataObject, STGMEDIUM, STGMEDIUM_0,
    TYMED_HGLOBAL,
};
use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
use windows::Win32::System::Memory::{GHND, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::{DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE, IDropSource};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    BHID_DataObject, CLSID_DragDropHelper, IDragSourceHelper, SHCreateShellItemArrayFromIDLists, SHDRAGIMAGE, SHDoDragDrop,
    SHParseDisplayName,
};
use windows::core::{HSTRING, w};

use crate::codec::{self, Format, SaveOptions};
use crate::image::{Image, argb, ca, cb, cg, cr};
use crate::{gdip, log, screen};

/// The shell data object of `files` (all must exist).
fn data_object(files: &[String]) -> Option<IDataObject> {
    let mut pidls: Vec<*mut ITEMIDLIST> = Vec::new();
    for f in files {
        let mut id: *mut ITEMIDLIST = std::ptr::null_mut();
        if unsafe { SHParseDisplayName(&HSTRING::from(f.as_str()), None, &mut id, 0, None) }.is_ok() && !id.is_null() {
            pidls.push(id);
        }
    }
    let list: Vec<*const ITEMIDLIST> = pidls.iter().map(|&p| p as *const _).collect();
    let obj = if list.is_empty() {
        None
    } else {
        unsafe { SHCreateShellItemArrayFromIDLists(&list) }
            .and_then(|arr| unsafe { arr.BindToHandler::<_, IDataObject>(None, &BHID_DataObject) })
            .ok()
    };
    for p in pidls {
        unsafe { CoTaskMemFree(Some(p as *const _)) };
    }
    obj
}

/// DragFilesOut: drags `files` from `source` (modal until the drop); `thumb` (optional) follows the cursor. Move is
/// only allowed when the files are temporary copies. Returns the drop effect (0 = canceled).
pub fn drag_files_out(source: isize, files: &[String], thumb: Option<&Image>, allow_move: bool) -> u32 {
    if files.is_empty() {
        return 0;
    }
    let Some(obj) = data_object(files) else {
        log::write(&format!("DragOut: no data object for {} file(s)", files.len()));
        return 0;
    };
    if let Some(t) = thumb.filter(|t| !t.is_empty())
        && let Ok(helper) = unsafe { CoCreateInstance::<_, IDragSourceHelper>(&CLSID_DragDropHelper, None, CLSCTX_INPROC_SERVER) }
    {
        let max_s = (96 * unsafe { GetDpiForWindow(screen::hwnd(source)) }.max(96) / 96) as f64;
        let k = 1.0f64.min(max_s / t.w as f64).min(max_s / t.h as f64);
        let mut img = gdip::scale_image(t, ((t.w as f64 * k) as i32).max(1), ((t.h as f64 * k) as i32).max(1), true);
        for px in img.px.iter_mut() {
            let a = ca(*px) * 220 / 255;
            *px = argb(a, cr(*px) * a / 255, cg(*px) * a / 255, cb(*px) * a / 255);
        }
        if let Some(hb) = screen::to_hbitmap(&img) {
            let di = SHDRAGIMAGE {
                sizeDragImage: SIZE { cx: img.wi(), cy: img.hi() },
                ptOffset: POINT { x: img.wi() / 2, y: img.hi() / 2 },
                hbmpDragImage: hb,
                crColorKey: windows::Win32::Foundation::COLORREF(0xFFFF_FFFF), // CLR_NONE
            };
            if unsafe { helper.InitializeFromBitmap(&di, &obj) }.is_err() {
                unsafe {
                    let _ = DeleteObject(HGDIOBJ(hb.0));
                }
            }
        }
    }
    // Explorer follows the preferred effect (copy); some apps, like Paint, only accept it when "move" is also allowed.
    // Move only applies to temporary copies: an original file (a video of the library) cannot leave its folder
    unsafe {
        let cf = RegisterClipboardFormatW(w!("Preferred DropEffect"));
        if let Ok(h) = GlobalAlloc(GHND, 4) {
            *(GlobalLock(h) as *mut u32) = DROPEFFECT_COPY.0;
            let _ = GlobalUnlock(h);
            let fe = FORMATETC { cfFormat: cf as u16, ptd: std::ptr::null_mut(), dwAspect: DVASPECT_CONTENT.0, lindex: -1, tymed: TYMED_HGLOBAL.0 as u32 };
            let m = STGMEDIUM { tymed: TYMED_HGLOBAL.0 as u32, u: STGMEDIUM_0 { hGlobal: h }, pUnkForRelease: std::mem::ManuallyDrop::new(None) };
            let _ = obj.SetData(&fe, &m, true);
        }
    }
    let mut allowed = DROPEFFECT_COPY | DROPEFFECT_LINK;
    if allow_move {
        allowed |= DROPEFFECT_MOVE;
    }
    match unsafe { SHDoDragDrop(Some(screen::hwnd(source)), &obj, None::<&IDropSource>, allowed) } {
        Ok(DROPEFFECT(e)) => e,
        Err(_) => 0,
    }
}

/// TempImageFile: a temporary copy of an image for dragging or copying as a file, %TEMP%\Snapnab\<name>.png (C29:
/// metadata stripped when StripMetadata or `strip` says so).
pub fn temp_image_file(img: &Image, name: &str, strip: bool, version: &str) -> Option<String> {
    let dir = std::env::temp_dir().join("Snapnab");
    let _ = std::fs::create_dir_all(&dir);
    let name = if name.is_empty() { format!("Snapnab_{}", String::from_utf16_lossy(&crate::library::time_stamp(false))) } else { name.to_string() };
    let name: String = name.chars().map(|c| if "\\/:*?\"<>|".contains(c) { '_' } else { c }).collect();
    let path = dir.join(format!("{name}.png"));
    let mut o = SaveOptions::from_settings();
    if strip {
        o.strip_metadata = true;
    }
    codec::save_file(Path::new(&path), img, Format::Png, &o, version).then(|| path.to_string_lossy().into_owned())
}
