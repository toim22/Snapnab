//! The open documents (main.cpp "documents" and "file"): activate, open, import, autosave, save, save as, duplicate,
//! delete, export, print, e-mail, external program and Grab Text.

use std::sync::Arc;

use snapcore::image::Image;
use snapcore::ipc::{self, Channel};
use snapedit_app::doc::{Document, ext_of, file_name_of};
use snapedit_app::files;
use snapedit_app::native;
use snapedit_app::tr;
use snapui::form::{self, Field};
use snapui::host::Ctl;
use windows::Win32::UI::WindowsAndMessaging::{IDYES, MB_ICONQUESTION, MB_ICONWARNING, MB_YESNO};

use super::{Editor, reg_dw, reg_str, set_reg_dw, set_reg_str};

/// The GUID a library capture is named after, as the 16 bytes of the IPC blob.
fn guid_blob(path: &str) -> Option<[u8; 16]> {
    let stem = std::path::Path::new(path).file_stem()?.to_string_lossy().into_owned();
    let w = native::wide(&format!("{{{stem}}}"));
    let g = unsafe { windows::Win32::System::Com::CLSIDFromString(windows::core::PCWSTR(w.as_ptr())) }.ok()?;
    let mut b = [0u8; 16];
    b[..4].copy_from_slice(&g.data1.to_le_bytes());
    b[4..6].copy_from_slice(&g.data2.to_le_bytes());
    b[6..8].copy_from_slice(&g.data3.to_le_bytes());
    b[8..].copy_from_slice(&g.data4);
    Some(b)
}

pub fn same_path(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

pub fn in_library(path: &str) -> bool {
    let dir = files::library_dir();
    std::path::Path::new(path).parent().is_some_and(|p| same_path(&p.to_string_lossy(), &dir))
}

/// Formats "%d"/"%zu"/"%s" in order (the translated strings keep the C++ printf placeholders).
pub fn fmt(s: &str, args: &[&dyn std::fmt::Display]) -> String {
    let mut out = String::new();
    let mut rest = s;
    let mut i = 0;
    while let Some(p) = rest.find('%') {
        out += &rest[..p];
        let tail = &rest[p..];
        let len = ["%zu", "%d", "%s", "%u", "%%"].iter().find(|t| tail.starts_with(**t)).map(|t| t.len());
        match len {
            Some(2) if tail.starts_with("%%") => {
                out.push('%');
                rest = &tail[2..];
            }
            Some(l) => {
                if let Some(a) = args.get(i) {
                    out += &a.to_string();
                }
                i += 1;
                rest = &tail[l..];
            }
            None => {
                out.push('%');
                rest = &tail[1..];
            }
        }
    }
    out + rest
}

impl Editor {
    pub fn find_doc(&self, path: &str) -> Option<usize> {
        self.cv.docs.iter().position(|d| same_path(&d.path, path))
    }

    /// Autosave: a changed document is written back to its .snap, with its thumbnail.
    pub fn autosave(&mut self, i: usize) {
        let Some(d) = self.cv.docs.get_mut(i) else { return };
        if d.is_video || !d.modified || d.path.is_empty() {
            return;
        }
        if d.save() {
            let (p, img) = (d.path.clone(), d.render(None));
            files::write_thumbnail(&p, &img);
            self.tray.invalidate_thumb(&p);
            self.library.invalidate_thumb(&p);
            snapcore::log::write(&format!("Autosave: {p}"));
        }
    }

    fn ipc_doc(&self, code: u32, path: &str) {
        if let (Some(i), Some(b)) = (&self.ipc, guid_blob(path)) {
            i.send(Channel::Capture, code, ipc::KIND_BLOB, &b, false);
        }
    }

    pub fn activate(&mut self, idx: Option<usize>) {
        if self.cv.cur == idx && idx.is_some() {
            self.show_library(false);
            return;
        }
        self.cv.commit_text();
        if self.fx.is_open() {
            self.fx.collapse();
        }
        if let Some(c) = self.cv.cur {
            self.autosave(c);
            let p = self.cv.docs[c].path.clone();
            self.ipc_doc(ipc::code::LIBRARY_DOC_RELEASED, &p);
        }
        self.cv.set_document(idx);
        match self.cv.doc() {
            Some(d) if d.is_video => {
                let p = d.path.clone();
                self.video.open(&p);
            }
            _ => self.video.close(),
        }
        if let Some(d) = self.cv.doc() {
            let p = d.path.clone();
            self.ipc_doc(ipc::code::LIBRARY_DOC_OPENED, &p);
            self.tray.set_current(&p);
            self.library.current = p;
            self.library_open = false;
        }
        self.update_doc_info();
        self.props.reload();
        self.schedule_effect_previews();
    }

    /// Keeps up to 12 documents in memory (the others stay saved in the library).
    pub fn add_doc(&mut self, d: Document) -> usize {
        if self.cv.docs.len() >= 12
            && let Some(i) = (0..self.cv.docs.len()).find(|&i| Some(i) != self.cv.cur)
        {
            self.autosave(i);
            self.remove_doc(i);
        }
        self.cv.docs.push(d);
        self.cv.docs.len() - 1
    }

    /// Drops document `i` from memory (the current one is deactivated first).
    pub fn remove_doc(&mut self, i: usize) {
        if i >= self.cv.docs.len() {
            return;
        }
        if self.cv.cur == Some(i) {
            self.activate(None);
        }
        self.cv.docs.remove(i);
        if let Some(c) = self.cv.cur
            && c > i
        {
            self.cv.cur = Some(c - 1);
        }
    }

    /// A new image becomes a .snap document in the library.
    pub fn new_doc_from_image(&mut self, img: Image, title: &str, mode: &str) -> usize {
        let mut d = Document::from_image(img, title);
        d.capture_mode = mode.to_owned();
        d.path = files::new_library_path();
        d.save();
        files::write_thumbnail(&d.path, &d.st.base);
        let i = self.add_doc(d);
        self.tray.reload();
        self.activate(Some(i));
        i
    }

    pub fn open_path(&mut self, _ctl: &mut Ctl, path_in: &str) -> bool {
        let path = std::path::absolute(path_in).map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| path_in.to_owned());
        if let Some(i) = self.find_doc(&path) {
            self.activate(Some(i));
            return true;
        }
        if !std::path::Path::new(&path).exists() {
            self.warn(tr("Arquivo não encontrado:\n") + &path);
            return false;
        }
        let ext = ext_of(&path);
        let Some(d) = Document::load(&path) else {
            // e.g. the HEIC/AVIF codec is not installed
            let why = if ext == ".snap" { String::new() } else { files::load_error(&path) };
            self.warn(tr("Não foi possível abrir:\n") + &path + &if why.is_empty() { String::new() } else { format!("\n\n{why}") });
            return false;
        };
        if ext != ".snap" && ext != ".mp4" {
            // a regular image is imported into the library
            let img = (*d.st.base).clone();
            self.new_doc_from_image(img, &file_name_of(&path), &tr("Arquivo importado"));
            return true;
        }
        let i = self.add_doc(d);
        self.activate(Some(i));
        true
    }

    pub fn close_current(&mut self) {
        let Some(c) = self.cv.cur else { return };
        self.cv.commit_text();
        self.autosave(c);
        self.remove_doc(c);
        if !self.cv.docs.is_empty() {
            self.activate(Some(self.cv.docs.len() - 1));
        }
    }

    /// RequireImageDoc: an image document is open (the text being typed is committed).
    pub fn require_image(&mut self) -> bool {
        if self.cv.doc().is_none_or(|d| d.is_video) {
            unsafe {
                let _ = windows::Win32::System::Diagnostics::Debug::MessageBeep(MB_ICONWARNING);
            }
            return false;
        }
        self.cv.commit_text();
        true
    }

    pub fn save_current(&mut self, ctl: &mut Ctl) {
        if !self.require_image() {
            return;
        }
        let Some(c) = self.cv.cur else { return };
        if self.cv.docs[c].path.is_empty() {
            self.save_as(ctl);
            return;
        }
        if self.cv.docs[c].save() {
            let d = &self.cv.docs[c];
            let (p, img) = (d.path.clone(), d.render(None));
            files::write_thumbnail(&p, &img);
            self.tray.invalidate_thumb(&p);
            self.library.invalidate_thumb(&p);
            self.status = tr("Salvo na biblioteca: ") + &file_name_of(&p);
        }
    }

    /// Save as: the file dialog, then the format's own options (JPEG quality, WebP, AVIF).
    pub fn save_as(&mut self, _ctl: &mut Ctl) {
        if !self.require_image() {
            return;
        }
        let Some(d) = self.cv.doc() else { return };
        let mut base: String = d.title.chars().map(|c| if "\\/:*?\"<>|".contains(c) { '_' } else { c }).collect();
        let e = ext_of(&base);
        if e.len() > 1 && e.len() <= 5 {
            base.truncate(base.len() - e.len());
        }
        let initial = format!("{}\\{}", files::default_save_dir(), if base.is_empty() { tr("Captura") } else { base });
        let fi = reg_dw("SaveAsFilter", 1);
        self.modal(
            move |owner| {
                let filter = "PNG (*.png)|*.png|JPEG (*.jpg)|*.jpg;*.jpeg|BMP (*.bmp)|*.bmp|GIF (*.gif)|*.gif|TIFF (*.tif)|*.tif;*.tiff|WebP (*.webp)|*.webp|AVIF (*.avif)|*.avif|PDF (*.pdf)|*.pdf|Snapnab (*.snap)|*.snap|";
                let (mut path, fi) = native::save_file(owner, &tr("Salvar como"), &initial, filter, "png", fi)?;
                const EXTS: [&str; 9] = [".png", ".jpg", ".bmp", ".gif", ".tif", ".webp", ".avif", ".pdf", ".snap"];
                let ext = ext_of(&path);
                let known = EXTS.contains(&ext.as_str()) || ext == ".jpeg" || ext == ".tiff";
                if !known && (1..=EXTS.len() as u32).contains(&fi) {
                    path += EXTS[fi as usize - 1];
                }
                let ext = ext_of(&path);
                let mut q = 90;
                if ext == ".jpg" || ext == ".jpeg" {
                    let mut f = vec![Field::Int { label: tr("Qualidade JPEG (1–100)"), value: 90, min: 1, max: 100 }];
                    if !form::run(owner, "JPEG", &mut f) {
                        return None;
                    }
                    q = f[0].int();
                } else if ext == ".webp" {
                    let mut f = vec![
                        Field::Check { label: tr("Sem perdas (ideal para capturas de tela)"), value: reg_root("WebPLossless", 1) != 0 },
                        Field::Int { label: tr("Qualidade com perdas (1–100)"), value: reg_root("WebPQuality", 90) as i32, min: 1, max: 100 },
                    ];
                    if !form::run(owner, "WebP", &mut f) {
                        return None;
                    }
                    set_reg_root("WebPLossless", f[0].on() as u32);
                    set_reg_root("WebPQuality", f[1].int() as u32);
                } else if ext == ".avif" {
                    if !snapcore::codec::can_encode(snapcore::codec::Format::Avif) {
                        native::message(owner, &snapcore::codec::avif_unavailable_text(), "Snapnab Editor", MB_ICONWARNING);
                        return None;
                    }
                    let mut f = vec![Field::Int { label: tr("Qualidade AVIF (1–100)"), value: reg_root("AvifQuality", 80) as i32, min: 1, max: 100 }];
                    if !form::run(owner, "AVIF", &mut f) {
                        return None;
                    }
                    set_reg_root("AvifQuality", f[0].int() as u32);
                }
                Some((path, q, fi))
            },
            |e, ctl, r| {
                let Some((path, q, fi)) = r else { return };
                set_reg_dw("SaveAsFilter", fi);
                e.save_as_to(ctl, &path, q);
            },
        );
    }

    pub fn save_as_to(&mut self, ctl: &mut Ctl, path: &str, q: i32) {
        let Some(c) = self.cv.cur else { return };
        let d = &mut self.cv.docs[c];
        let ok = if ext_of(path) == ".snap" {
            // a .snap copy outside the library: written without replacing the current document
            let (old, m) = (std::mem::replace(&mut d.path, path.to_owned()), d.modified);
            let ok = d.save();
            d.path = old;
            d.modified = m;
            if ok { Ok(()) } else { Err(String::new()) }
        } else {
            d.save_as(path, q)
        };
        if let Err(why) = ok {
            self.warn(tr("Não foi possível salvar:\n") + path + &if why.is_empty() { String::new() } else { format!("\n\n{why}") });
            return;
        }
        self.status = tr("Salvo em ") + path;
        // a library capture without a name takes the name used in "Save as"
        let p = self.cv.docs[c].path.clone();
        if in_library(&p) && files::capture_name(&p).is_empty() {
            let name = std::path::Path::new(path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            files::set_capture_name(&p, &name);
            self.cv.docs[c].title = name;
            self.tray.reload();
            if self.library_open {
                self.library.reload();
            }
        }
        self.update_title(ctl);
    }

    pub fn new_from_clipboard(&mut self, _ctl: &mut Ctl) {
        let Some(img) = snapcore::clip::get_image() else {
            self.status = tr("A área de transferência não contém imagem.");
            return;
        };
        self.new_doc_from_image(img, &tr("Da área de transferência"), &tr("Área de transferência"));
        self.status = tr("Imagem nova criada a partir da área de transferência.");
    }

    pub fn new_blank_image(&mut self) {
        self.modal(
            |owner| {
                let mut f = vec![
                    Field::Int { label: tr("Largura (px)"), value: 1280, min: 1, max: 20000 },
                    Field::Int { label: tr("Altura (px)"), value: 720, min: 1, max: 20000 },
                    Field::Color { label: tr("Cor de fundo"), color: 0xFFFFFF },
                    Field::Check { label: tr("Transparente"), value: false },
                ];
                form::run(owner, &tr("Nova imagem"), &mut f).then_some(f)
            },
            |e, _, f| {
                let Some(f) = f else { return };
                let fill = if f[3].on() { 0 } else { snapcore::image::from_colorref(f[2].color()) };
                e.new_doc_from_image(Image::sized(f[0].int(), f[1].int(), fill), &tr("Imagem nova"), &tr("Nova imagem"));
            },
        );
    }

    /// The Editor does not capture: it asks the capture process (IPC 0x12).
    pub fn request_capture(&mut self, kind: i32) {
        let running = ipc::mutex_exists(&snapcore::wide::to_wide(ipc::CAPTURE_MUTEX));
        if running && let Some(i) = &self.ipc {
            i.send_int(Channel::Capture, ipc::code::EDITOR_START_CAPTURE, kind, false);
        } else {
            snapcore::instance::launch_sibling("Snapnab.exe", if kind != 0 { "/video" } else { "/capture" }, false);
        }
    }

    pub fn duplicate_current(&mut self) {
        if !self.require_image() {
            return;
        }
        let Some(s) = self.cv.doc() else { return };
        let mut d = Document::default();
        d.title = s.title.clone() + &tr(" (cópia)");
        d.capture_mode = s.capture_mode.clone();
        d.window_title = s.window_title.clone();
        d.application = s.application.clone();
        d.created = s.created.clone();
        d.tags = s.tags.clone();
        d.guides_x = s.guides_x.clone();
        d.guides_y = s.guides_y.clone();
        d.st = s.st.clone();
        d.path = files::new_library_path();
        d.modified = true;
        let i = self.add_doc(d);
        self.autosave(i);
        self.tray.reload();
        self.activate(Some(i));
    }

    /// Deletes library items (to the Recycle Bin) after one confirmation.
    pub fn delete_docs(&mut self, paths: Vec<String>) {
        if paths.is_empty() {
            return;
        }
        let q = if paths.len() == 1 {
            tr("Enviar para a Lixeira?\n") + &file_name_of(&paths[0])
        } else {
            fmt(&tr("Enviar %zu itens da biblioteca para a Lixeira?"), &[&paths.len()])
        };
        self.modal(move |o| native::message(o, &q, "Snapnab Editor", MB_YESNO | MB_ICONQUESTION), move |e, _, r| {
            if r != IDYES.0 {
                return;
            }
            for p in &paths {
                if let Some(i) = e.find_doc(p) {
                    e.cv.docs[i].modified = false;
                    e.remove_doc(i);
                }
                snapcore::library::delete_item(&snapcore::library::library_dir(), &p.encode_utf16().collect::<Vec<u16>>(),
                                               windows::Win32::Foundation::HWND(e.hwnd as _));
            }
            e.tray.reload();
            if e.library_open {
                e.library.reload();
            }
            if e.cv.cur.is_none() && !e.cv.docs.is_empty() {
                e.activate(Some(e.cv.docs.len() - 1));
            }
            e.status = if paths.len() == 1 {
                tr("Item enviado para a Lixeira.")
            } else {
                fmt(&tr("%zu itens enviados para a Lixeira."), &[&paths.len()])
            };
        });
    }

    /// An item of the library (open document or file) as an image with its objects, and its name.
    pub fn load_item_image(&self, p: &str) -> Option<(Image, String)> {
        if ext_of(p) == ".mp4" {
            return None;
        }
        if let Some(i) = self.find_doc(p) {
            let d = &self.cv.docs[i];
            return Some((d.render(None), d.title.clone())).filter(|(im, _)| !im.is_empty());
        }
        let d = Document::load(p)?;
        if d.is_video {
            return None;
        }
        let mut img = d.render(None);
        if ext_of(p) != ".snap" {
            img.meta = d.st.base.meta.clone();
        }
        let name = if d.title.is_empty() { file_name_of(p) } else { d.title.clone() };
        Some((img, name)).filter(|(im, _)| !im.is_empty())
    }

    /// Exports library items to a folder: images in the chosen format (objects applied), videos as MP4.
    pub fn export_items(&mut self, paths: Vec<String>) {
        if paths.is_empty() {
            return;
        }
        // the images are rendered now (the open documents may have unsaved changes)
        let items: Vec<(String, Option<Arc<Image>>)> =
            paths.iter().map(|p| (p.clone(), self.load_item_image(p).map(|(i, _)| Arc::new(i)))).collect();
        let fmt0 = reg_dw("ExportFormat", 0) as i32;
        self.modal(
            move |owner| {
                let mut f = vec![
                    Field::Combo { label: tr("Formato das imagens"), items: ["PNG", "JPEG", "BMP", "GIF", "TIFF"].map(String::from).to_vec(), value: fmt0 },
                    Field::Label(tr("Vídeos são copiados em MP4.")),
                ];
                if !form::run(owner, &fmt(&tr("Exportar %zu item(ns)"), &[&items.len()]), &mut f) {
                    return None;
                }
                let dir = native::pick_folder(owner, &tr("Exportar para a pasta"))?;
                let fi = f[0].int().clamp(0, 4) as usize;
                let ext = [".png", ".jpg", ".bmp", ".gif", ".tif"][fi];
                let (mut ok, mut fail) = (0, 0);
                for (p, img) in &items {
                    let t = std::fs::metadata(p).and_then(|m| m.modified()).ok();
                    let stamp = t.map(local_stamp).unwrap_or_default();
                    let base = format!("{}_{}", tr("Captura"), stamp);
                    let video = ext_of(p) == ".mp4";
                    let e = if video { ".mp4" } else { ext };
                    let mut dst = format!("{dir}\\{base}{e}");
                    let mut n = 2;
                    while std::path::Path::new(&dst).exists() {
                        dst = format!("{dir}\\{base}_{n}{e}");
                        n += 1;
                    }
                    let done = if video {
                        std::fs::copy(p, &dst).is_ok()
                    } else {
                        img.as_ref().is_some_and(|i| files::save_image(&dst, i, 90).is_ok())
                    };
                    if done { ok += 1 } else { fail += 1 }
                }
                snapedit_app::shell_open(&dir);
                Some((fi, dir, ok, fail))
            },
            |e, _, r| {
                let Some((fi, dir, ok, fail)) = r else { return };
                set_reg_dw("ExportFormat", fi as u32);
                e.status = fmt(&tr("%d item(ns) exportado(s) para %s"), &[&ok, &dir])
                    + &if fail > 0 { fmt(&tr(" — %d falha(s)"), &[&fail]) } else { String::new() };
            },
        );
    }

    pub fn temp_export(&self) -> String {
        let Some(d) = self.cv.doc() else { return String::new() };
        files::temp_export(&d.title, &d.render(None))
    }

    pub fn open_in_program(&mut self) {
        if !self.require_image() {
            return;
        }
        let file = self.temp_export();
        let prog = reg_str("ExternalProgram", "mspaint.exe");
        self.modal(
            move |owner| {
                let mut f = vec![Field::Text { label: tr("Programa"), text: prog }];
                form::run(owner, &tr("Abrir em programa externo"), &mut f).then(|| f[0].text().to_owned())
            },
            move |_, _, r| {
                let Some(p) = r else { return };
                set_reg_str("ExternalProgram", &p);
                let (w, a) = (native::wide(&p), native::wide(&format!("\"{file}\"")));
                unsafe {
                    windows::Win32::UI::Shell::ShellExecuteW(None, windows::core::w!("open"), windows::core::PCWSTR(w.as_ptr()),
                                                             windows::core::PCWSTR(a.as_ptr()), windows::core::PCWSTR::null(),
                                                             windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL);
                }
            },
        );
    }

    /// E-mail through Simple MAPI (MAPI32, like the original): a PNG attachment.
    pub fn send_email(&mut self) {
        if !self.require_image() {
            return;
        }
        let file = self.temp_export();
        let subject = tr("Captura: ") + &self.cv.doc().map(|d| d.title.clone()).unwrap_or_default();
        self.modal(move |owner| super::extras::mapi_send(owner, &file, &subject), |e, _, r| {
            if let Err(m) = r {
                e.inform(m, "Snapnab Editor");
            }
        });
    }

    /// Grab Text: OCR of the selection or the whole image, shown in a text window.
    pub fn grab_text(&mut self, _ctl: &mut Ctl) {
        if !self.require_image() {
            return;
        }
        let img = self.image_or_selection();
        self.status = tr("Reconhecendo o texto…");
        self.modal(move |_| snapcore::ocr::run_ocr(&img).map(|t| String::from_utf16_lossy(&t)).map_err(|e| format!("{e:?}")), |e, ctl, r| {
            e.status.clear();
            match r {
                Ok(t) => {
                    let t = if t.trim().is_empty() { tr("(nenhum texto reconhecido)") } else { t };
                    e.show_text(ctl, tr("Grab Text — texto reconhecido"), t);
                }
                Err(err) => e.warn(tr("OCR indisponível: ") + &err),
            }
        });
    }

    pub fn image_or_selection(&mut self) -> Image {
        if self.cv.has_raster_selection() {
            self.cv.selection_image()
        } else {
            self.cv.doc().map(|d| d.render(None)).unwrap_or_default()
        }
    }
}

fn reg_root(name: &str, def: u32) -> u32 {
    snapcore::reg::get_dword(std::ffi::OsStr::new(name), def, None)
}

fn set_reg_root(name: &str, v: u32) {
    snapcore::reg::set_dword(std::ffi::OsStr::new(name), v, None);
}

/// A file time as local "yyyy-MM-dd_HH-mm-ss" (export and drag names).
pub fn local_stamp(t: std::time::SystemTime) -> String {
    use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
    let d = t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let ft = (d.as_nanos() / 100) as u64 + 116_444_736_000_000_000;
    let f = FILETIME { dwLowDateTime: ft as u32, dwHighDateTime: (ft >> 32) as u32 };
    let (mut st, mut lt) = (SYSTEMTIME::default(), SYSTEMTIME::default());
    unsafe {
        let _ = FileTimeToSystemTime(&f, &mut st);
        let _ = SystemTimeToTzSpecificLocalTime(None, &st, &mut lt);
    }
    format!("{:04}-{:02}-{:02}_{:02}-{:02}-{:02}", lt.wYear, lt.wMonth, lt.wDay, lt.wHour, lt.wMinute, lt.wSecond)
}
