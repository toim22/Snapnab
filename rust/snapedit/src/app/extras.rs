//! The Editor's extra tools (main.cpp, "competitor feature request"): Office, cursor stamps, Add capture, Combine,
//! QR, Beautify, Redact, Remove background, Split, date stamp, contrast, AI assistant, Compare, Batch, Pin, Upload,
//! Copy as, e-mail (Simple MAPI), printing and the manual.
//!
//! The forms that had a live preview in C++ (RunFormPreview) are plain forms here: the result shows on the canvas
//! and Ctrl+Z undoes it (docs/ARCHITECTURE.md, "Rust rewrite: decisions to review").

use std::collections::HashMap;
use std::path::Path;

use snapcore::beautify::{self, BeautifyOptions};
use snapcore::codec::{self, Format, SaveOptions};
use snapcore::doc::{DrawObj, ObjType, PointF, SHAPE_RECT};
use snapcore::gdip::scale_image;
use snapcore::image::Image;
use snapcore::ipc::{self, Channel};
use snapcore::office::{self, Target};
use snapcore::upload::{self, Settings};
use snapcore::xml::Node;
use snapcore::{color, fx, instance, library, log, paths, presets, reg};
use snapedit_app::canvas::Tool;
use snapedit_app::doc::{Document, ext_of, file_name_of};
use snapedit_app::edtools::{self, CombineOptions, DateTime, RedactItem};
use snapedit_app::effects::apply_quick;
use snapedit_app::native::{self, Item, Menu};
use snapedit_app::{menu, tr};
use snapui::form::{self, Field};
use snapui::host::Ctl;

use super::docs::fmt;
use super::{Editor, VERSION, reg_dw, set_reg_dw};

/// Internal command: the Copy as menu (opened from the window procedure, outside the frame drawing).
pub const COPY_AS_MENU: u32 = 0xF004;

const IMAGE_FILTER: &str = "Imagens|*.png;*.jpg;*.jpeg;*.bmp;*.gif;*.tif;*.tiff;*.webp;*.avif;*.heic;*.heif;*.snap|";

/// An image file or .snap (rendered with its objects), off the UI thread.
pub fn load_file_image(p: &str) -> Option<(Image, String)> {
    if ext_of(p) == ".mp4" {
        return None;
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

fn pick_images(owner: isize) -> Vec<String> {
    native::open_files(owner, &tr("Escolha as imagens"), &IMAGE_FILTER.replacen("Imagens", &tr("Imagens"), 1)).unwrap_or_default()
}

// ---------------------------------------------------------------- Beautify presets
pub struct BeautifyPreset {
    pub name: String,
    pub opt: BeautifyOptions,
    pub builtin: bool,
}

fn presets_file() -> std::path::PathBuf {
    paths::app_data_dir().join("BeautifyPresets.xml")
}

pub fn beautify_presets() -> Vec<BeautifyPreset> {
    let mut v = Vec::new();
    let mut add = |name: &str, bg: i32, a: u32, b: u32, angle: i32, pad: i32, rad: i32| {
        let opt = BeautifyOptions { background: bg, color1: a, color2: b, angle, padding: pad, radius: rad, ..Default::default() };
        v.push(BeautifyPreset { name: name.into(), opt, builtin: true });
    };
    let rgb = color::rgb;
    add("Oceano", 1, rgb(102, 126, 234), rgb(118, 75, 162), 135, 64, 12);
    add("Pôr do sol", 1, rgb(255, 126, 95), rgb(254, 180, 123), 135, 64, 12);
    add("Menta", 1, rgb(67, 206, 162), rgb(24, 90, 157), 135, 64, 12);
    add("Grafite", 1, rgb(68, 68, 80), rgb(20, 20, 26), 160, 64, 12);
    add("Papel de parede", 2, rgb(40, 40, 46), rgb(40, 40, 46), 0, 72, 12);
    add("Branco simples", 0, rgb(255, 255, 255), rgb(255, 255, 255), 0, 48, 8);
    if let Some(root) = snapcore::xml::load_file(&presets_file()) {
        for c in root.children.iter().filter(|c| c.name == "Preset") {
            let name = c.attr("Name").unwrap_or("").to_string();
            if !name.is_empty() {
                v.push(BeautifyPreset { name, opt: presets::beautify_from_xml(c), builtin: false });
            }
        }
    }
    v
}

fn save_beautify_presets(all: &[BeautifyPreset]) {
    let mut root = Node::new("BeautifyPresets");
    for p in all.iter().filter(|p| !p.builtin) {
        let mut n = presets::beautify_to_xml(&p.opt, "Preset");
        n.set("Name", p.name.clone());
        root.children.push(n);
    }
    snapcore::xml::save_file(&presets_file(), &root);
}

/// BeautifyDialog (without the preview).
fn beautify_dialog(owner: isize, o: &BeautifyOptions) -> Option<BeautifyOptions> {
    let presets = beautify_presets();
    let mut names = vec![tr("(ajustes abaixo)")];
    names.extend(presets.iter().map(|p| if p.builtin { tr(&p.name) } else { p.name.clone() }));
    let s = |v: &[&str]| v.iter().map(|x| tr(x)).collect::<Vec<_>>();
    let mut f = vec![
        Field::Combo { label: tr("Predefinição"), items: names, value: 0 },
        Field::Combo { label: tr("Fundo"), items: s(&["Cor sólida", "Degradê", "Papel de parede", "Imagem", "Transparente"]), value: o.background },
        Field::Color { label: tr("Cor 1"), color: o.color1 },
        Field::Color { label: tr("Cor 2 (degradê)"), color: o.color2 },
        Field::Int { label: tr("Ângulo do degradê"), value: o.angle, min: 0, max: 359 },
        Field::Text { label: tr("Imagem de fundo"), text: String::from_utf16_lossy(&o.image_path) },
        Field::Int { label: tr("Margem (px)"), value: o.padding, min: 0, max: 1000 },
        Field::Int { label: tr("Cantos arredondados (px)"), value: o.radius, min: 0, max: 200 },
        Field::Int { label: tr("Sombra (%)"), value: o.shadow, min: 0, max: 100 },
        Field::Int { label: tr("Tamanho da sombra (px)"), value: o.shadow_size, min: 0, max: 200 },
        Field::Combo {
            label: tr("Proporção"),
            items: vec![tr("Livre"), "16:9".into(), "4:3".into(), "1:1".into(), "9:16".into()],
            value: o.aspect,
        },
        Field::Combo {
            label: tr("Alinhamento"),
            items: s(&["Superior esquerdo", "Superior", "Superior direito", "Esquerda", "Centro", "Direita", "Inferior esquerdo",
                       "Inferior", "Inferior direito"]),
            value: o.align,
        },
        Field::Check { label: tr("Equilibrar as margens (corta bordas lisas da captura)"), value: o.auto_balance },
        Field::Text { label: tr("Salvar como predefinição (nome)"), text: String::new() },
    ];
    if !form::run(owner, &tr("Embelezar"), &mut f) {
        return None;
    }
    let p = f[0].int();
    let r = if p > 0 && p as usize <= presets.len() {
        presets[p as usize - 1].opt.clone()
    } else {
        BeautifyOptions {
            background: f[1].int(),
            color1: f[2].color(),
            color2: f[3].color(),
            angle: f[4].int(),
            image_path: f[5].text().trim().encode_utf16().collect(),
            padding: f[6].int(),
            radius: f[7].int(),
            shadow: f[8].int(),
            shadow_size: f[9].int(),
            aspect: f[10].int(),
            align: f[11].int(),
            auto_balance: f[12].on(),
        }
    };
    let name = f[13].text().trim().to_string();
    if !name.is_empty() {
        let mut all = presets;
        match all.iter_mut().find(|p| !p.builtin && p.name == name) {
            Some(p) => p.opt = r.clone(),
            None => all.push(BeautifyPreset { name, opt: r.clone(), builtin: false }),
        }
        save_beautify_presets(&all);
    }
    Some(r)
}

// ---------------------------------------------------------------- e-mail and printing

/// Simple MAPI (MAPI32.DLL, as the original): the file as an attachment.
pub fn mapi_send(owner: isize, file: &str, subject: &str) -> Result<(), String> {
    use windows::Win32::Foundation::FreeLibrary;
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    #[repr(C)]
    struct FileDesc {
        reserved: u32,
        flags: u32,
        position: u32,
        path_name: *const u16,
        file_name: *const u16,
        file_type: *const std::ffi::c_void,
    }
    #[repr(C)]
    struct Message {
        reserved: u32,
        subject: *const u16,
        note_text: *const u16,
        message_type: *const u16,
        date_received: *const u16,
        conversation_id: *const u16,
        flags: u32,
        originator: *const std::ffi::c_void,
        recip_count: u32,
        recips: *const std::ffi::c_void,
        file_count: u32,
        files: *const FileDesc,
    }
    type SendMailW = unsafe extern "system" fn(usize, usize, *const Message, u32, u32) -> u32;
    const MAPI_LOGON_UI: u32 = 0x1;
    const MAPI_DIALOG: u32 = 0x8;
    const MAPI_USER_ABORT: u32 = 1;
    let no_client = || tr("Nenhum cliente de e-mail MAPI está configurado. A imagem foi salva em:\n") + file;
    unsafe {
        let Ok(m) = LoadLibraryW(windows::core::w!("MAPI32.DLL")) else { return Err(no_client()) };
        let Some(p) = GetProcAddress(m, windows::core::s!("MAPISendMailW")) else {
            let _ = FreeLibrary(m);
            return Err(no_client());
        };
        let send: SendMailW = std::mem::transmute(p);
        let (wp, wn, ws) = (native::wide(file), native::wide(&file_name_of(file)), native::wide(subject));
        let fd = FileDesc { reserved: 0, flags: 0, position: u32::MAX, path_name: wp.as_ptr(), file_name: wn.as_ptr(), file_type: std::ptr::null() };
        let msg = Message {
            reserved: 0,
            subject: ws.as_ptr(),
            note_text: std::ptr::null(),
            message_type: std::ptr::null(),
            date_received: std::ptr::null(),
            conversation_id: std::ptr::null(),
            flags: 0,
            originator: std::ptr::null(),
            recip_count: 0,
            recips: std::ptr::null(),
            file_count: 1,
            files: &fd,
        };
        let r = send(0, owner as usize, &msg, MAPI_LOGON_UI | MAPI_DIALOG, 0);
        let _ = FreeLibrary(m);
        if r != 0 && r != MAPI_USER_ABORT {
            return Err(tr("Falha ao enviar e-mail (MAPI %u).").replace("%u", &r.to_string()));
        }
    }
    Ok(())
}

/// Draws an image on a printer DC (StretchDIBits, top-down 32 bpp).
unsafe fn draw_to_dc(dc: windows::Win32::Graphics::Gdi::HDC, img: &Image, x: i32, y: i32, w: i32, h: i32) {
    use windows::Win32::Graphics::Gdi::*;
    let bi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: img.wi(),
            biHeight: -img.hi(),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    unsafe {
        SetStretchBltMode(dc, HALFTONE);
        StretchDIBits(dc, x, y, w, h, 0, 0, img.wi(), img.hi(), Some(img.px.as_ptr() as *const _), &bi, DIB_RGB_COLORS, SRCCOPY);
    }
}

/// PrintCurrent: the print dialog, then the image at its real size at most (a long image across pages, broken
/// between text lines).
fn print_image(owner: isize, img: Image, title: String) -> Result<(), String> {
    use windows::Win32::Foundation::{GlobalFree, HWND};
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::Storage::Xps::{DOCINFOW, EndDoc, EndPage, StartDocW, StartPage};
    use windows::Win32::UI::Controls::Dialogs::*;
    let mut pd = PRINTDLGW {
        lStructSize: std::mem::size_of::<PRINTDLGW>() as u32,
        hwndOwner: HWND(owner as _),
        Flags: PD_RETURNDC | PD_NOSELECTION | PD_NOPAGENUMS,
        ..Default::default()
    };
    unsafe {
        if !PrintDlgW(&mut pd).as_bool() {
            let e = CommDlgExtendedError().0;
            if e != 0 {
                return Err(fmt(
                    &tr("Não foi possível abrir a impressão: não há impressora instalada ou o serviço de impressão do Windows (Spooler) está parado. (código 0x%04X)"),
                    &[&format!("{e:04X}")],
                ));
            }
            return Ok(());
        }
        let dc = pd.hDC;
        let wt = native::wide(&title);
        let di = DOCINFOW { cbSize: std::mem::size_of::<DOCINFOW>() as i32, lpszDocName: windows::core::PCWSTR(wt.as_ptr()), ..Default::default() };
        let mut r = Ok(());
        if StartDocW(dc, &di) > 0 {
            let (pw, ph) = (GetDeviceCaps(Some(dc), HORZRES), GetDeviceCaps(Some(dc), VERTRES));
            let dpi_x = GetDeviceCaps(Some(dc), LOGPIXELSX);
            let fit_w = (pw as f64 / img.w as f64).min(dpi_x as f64 / 96.0);
            if snapcore::pdf::split_long_pages() && img.h as f64 * fit_w > ph as f64 * 1.25 {
                let cuts = snapcore::pdf::smart_breaks(&img, ((ph as f64 / fit_w) as i32).max(64), -1);
                for part in edtools::split_image(&img, &cuts, false) {
                    if StartPage(dc) <= 0 {
                        break;
                    }
                    let (w, h) = ((part.w as f64 * fit_w) as i32, (part.h as f64 * fit_w) as i32);
                    draw_to_dc(dc, &part, (pw - w) / 2, 0, w, h);
                    EndPage(dc);
                }
            } else if StartPage(dc) > 0 {
                let s = fit_w.min(ph as f64 / img.h as f64);
                let (w, h) = ((img.w as f64 * s) as i32, (img.h as f64 * s) as i32);
                draw_to_dc(dc, &img, (pw - w) / 2, (ph - h) / 2, w, h);
                EndPage(dc);
            }
            EndDoc(dc);
        } else {
            r = Err(tr("A impressora não aceitou o documento (verifique se ela está ligada e se o serviço de impressão do Windows está rodando)."));
        }
        let _ = DeleteDC(dc);
        if !pd.hDevMode.is_invalid() {
            let _ = GlobalFree(Some(pd.hDevMode));
        }
        if !pd.hDevNames.is_invalid() {
            let _ = GlobalFree(Some(pd.hDevNames));
        }
        r
    }
}

/// OpenManual: help\Snapnab-Manual.html at `topic`, in the default browser (opening the .html through its association
/// would drop the #fragment).
pub fn open_manual(owner: isize, topic: &str) {
    use windows::Win32::UI::Shell::{ASSOCF_IS_PROTOCOL, ASSOCSTR_EXECUTABLE, AssocQueryStringW, ShellExecuteW};
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{PCWSTR, w};
    let path = paths::exe_dir().join("help").join("Snapnab-Manual.html");
    if !path.exists() {
        native::inform(owner, tr("O manual não foi encontrado:\n") + &path.to_string_lossy(), "Snapnab Editor".into());
        return;
    }
    let url = format!("file:///{}#{topic}", path.to_string_lossy().replace('\\', "/").replace(' ', "%20"));
    let mut buf = vec![0u16; 520];
    let mut n = buf.len() as u32;
    let browser = unsafe {
        AssocQueryStringW(ASSOCF_IS_PROTOCOL, ASSOCSTR_EXECUTABLE, w!("http"), w!("open"), Some(windows::core::PWSTR(buf.as_mut_ptr())), &mut n)
    }
    .is_ok()
    .then(|| String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(0)]))
    .filter(|b| !b.is_empty());
    if let Some(b) = browser {
        let (bw, arg) = (native::wide(&b), native::wide(&format!("\"{url}\"")));
        let r = unsafe { ShellExecuteW(None, PCWSTR::null(), PCWSTR(bw.as_ptr()), PCWSTR(arg.as_ptr()), PCWSTR::null(), SW_SHOWNORMAL) };
        if r.0 as isize > 32 {
            log::write(&format!("Manual: opened #{topic} in {b}"));
            return;
        }
    }
    shell_open(&path.to_string_lossy());
}

pub fn shell_open(path: &str) {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::PCWSTR;
    let p = native::wide(path);
    unsafe {
        ShellExecuteW(None, windows::core::w!("open"), PCWSTR(p.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

// ---------------------------------------------------------------- Copy as
fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

fn encoded(img: &Image, fmt: Format) -> Vec<u8> {
    codec::encode(img, fmt, &SaveOptions::from_settings(), VERSION).unwrap_or_default()
}

/// A PNG copy in %TEMP%\Snapnab (File / File path of an image whose library item is a .snap).
fn temp_image_file(img: &Image, name: &str) -> Option<String> {
    let dir = std::env::temp_dir().join("Snapnab");
    let _ = std::fs::create_dir_all(&dir);
    let name: String = name.chars().map(|c| if "\\/:*?\"<>|".contains(c) { '_' } else { c }).collect();
    let path = dir.join(format!("{name}.png"));
    codec::save_file(&path, img, Format::Png, &SaveOptions::from_settings(), VERSION).then(|| path.to_string_lossy().into_owned())
}

/// CopyAsMenu (Clipboard.cpp).
fn copy_as_menu(owner: isize, img: &Image, file: &str, title: &str, url: &str) -> bool {
    let video = ext_of(file) == ".mp4";
    let mut items = Vec::new();
    if !video {
        items.push(Item::Cmd(1, tr("Imagem")));
    }
    items.push(Item::Cmd(2, tr("Arquivo (para colar no Explorer, e-mail, chat)")));
    items.push(Item::Cmd(3, tr("Caminho do arquivo")));
    if !video {
        items.push(Item::Separator);
        items.push(Item::Cmd(4, tr("Base64 (PNG)")));
        items.push(Item::Cmd(5, tr("Data URL (PNG)")));
        items.push(Item::Cmd(6, tr("Data URL (WebP)")));
    }
    if !url.is_empty() {
        items.push(Item::Separator);
        items.push(Item::Cmd(8, tr("Link do upload")));
        if !video {
            items.push(Item::Cmd(7, tr("Imagem em Markdown (link do upload)")));
        }
    }
    let (x, y) = native::cursor_pos();
    let id = Menu::popup(items).choose(owner, x, y);
    let real_file = || -> Option<String> {
        if video || (!file.is_empty() && ext_of(file) != ".snap") {
            return Some(file.to_string());
        }
        let mut name = if title.is_empty() { format!("Snapnab_{}", String::from_utf16_lossy(&library::time_stamp(false))) } else { title.into() };
        let e = ext_of(&name);
        if Format::from_ext(&e) != Format::Unknown {
            name.truncate(name.len() - e.len());
        }
        temp_image_file(img, &name)
    };
    match id {
        1 => snapcore::clip::copy_image(owner, img, VERSION),
        2 => real_file().is_some_and(|f| snapcore::clip::copy_files(owner, &[f])),
        3 => real_file().is_some_and(|f| snapcore::clip::copy_text(owner, &f)),
        4 => snapcore::clip::copy_text(owner, &base64(&encoded(img, Format::Png))),
        5 => snapcore::clip::copy_text(owner, &format!("data:image/png;base64,{}", base64(&encoded(img, Format::Png)))),
        6 => snapcore::clip::copy_text(owner, &format!("data:image/webp;base64,{}", base64(&encoded(img, Format::Webp)))),
        8 => snapcore::clip::copy_text(owner, url),
        7 => snapcore::clip::copy_text(owner, &format!("![{}]({url})", if title.is_empty() { "Snapnab" } else { title })),
        _ => false,
    }
}

// ---------------------------------------------------------------- the tools
impl Editor {
    pub fn send_to_office(&mut self, t: i32) {
        if !self.require_image() {
            return;
        }
        let Some(target) = Target::from_i32(t) else { return };
        let file = self.temp_export();
        let (w, h) = self.cv.doc().map_or((0, 0), |d| (d.w(), d.h()));
        self.status = tr("Enviando…");
        self.modal(move |_| office::send(target, &file, w, h), move |e, _, r| match r {
            Ok(()) => e.status = tr("Enviado para ") + target.name() + ".",
            Err(err) => {
                e.status.clear();
                e.inform(err, target.name());
            }
        });
    }

    pub fn insert_cursor(&mut self, i: usize) {
        let cs = menu::cursor_stamps();
        if i >= cs.len() || !self.require_image() {
            return;
        }
        let (dw, dh) = self.cv.doc().map_or((0.0, 0.0), |d| (d.w() as f32, d.h() as f32));
        let mut o = DrawObj { kind: ObjType::Stamp, text: cs[i].0.encode_utf16().collect(), name: cs[i].1.clone(), ..Default::default() };
        o.style = self.cv.styles[Tool::Stamp.index()].clone();
        let h = (dw.min(dh) / 8.0).clamp(24.0, 64.0);
        let w = h * snapcore::render::stamp_aspect(cs[i].0);
        o.p1 = PointF { X: dw / 2.0 - w / 2.0, Y: dh / 2.0 - h / 2.0 };
        o.p2 = PointF { X: dw / 2.0 + w / 2.0, Y: dh / 2.0 + h / 2.0 };
        self.cv.paste_objects(&[o], true, true);
        self.doc_changed();
    }

    /// Add capture (C09): the capture goes below the target image as a Picture object.
    pub fn append_capture(&mut self, _ctl: &mut Ctl, target: &str, capture: &str) -> bool {
        let Some(i) = self.find_doc(target) else { return false };
        if self.cv.docs[i].is_video {
            return false;
        }
        let Some(nd) = Document::load(capture).filter(|d| !d.is_video) else { return false };
        let add = nd.render(None);
        if add.is_empty() {
            return false;
        }
        self.activate(Some(i));
        self.cv.commit_text();
        let Some(d) = self.cv.doc_mut() else { return false };
        let (w, h) = (d.w().max(add.wi()), d.h() + 16 + add.hi());
        d.checkpoint();
        let bg = if d.st.base.has_transparency() { 0 } else { 0xFFFF_FFFF };
        let base = fx::canvas_resize(&d.st.base, w, h, 0, bg);
        d.set_base(base);
        self.cv.refresh(true);
        self.cv.paste_image_at(&add, PointF { X: 0.0, Y: (h - add.hi()) as f32 });
        self.cv.fit_to_window();
        self.doc_changed();
        self.status = tr("Captura adicionada abaixo da imagem (é um objeto: pode ser movida).");
        true
    }

    /// Combine (C09): the library selection, or the current image plus files; the result is a new image.
    pub fn combine_now(&mut self, paths: Vec<String>, with_current: bool) {
        let mut imgs = Vec::new();
        if with_current && self.cv.doc().is_some_and(|d| !d.is_video) {
            self.cv.commit_text();
            imgs.push(self.cv.doc().unwrap().render(None));
        }
        // open documents are rendered here; files are read on the worker
        let open: HashMap<String, Image> =
            paths.iter().filter_map(|p| self.find_doc(p).map(|i| (p.clone(), self.cv.docs[i].render(None)))).collect();
        self.modal(
            move |owner| {
                let paths = if paths.is_empty() && imgs.len() < 2 { pick_images(owner) } else { paths };
                for p in &paths {
                    match open.get(p) {
                        Some(i) => imgs.push(i.clone()),
                        None => {
                            if let Some((i, _)) = load_file_image(p) {
                                imgs.push(i)
                            }
                        }
                    }
                }
                if imgs.len() < 2 {
                    return Err(tr("Escolha duas ou mais imagens para combinar."));
                }
                let o = CombineOptions::load();
                let s = |v: &[&str]| v.iter().map(|x| tr(x)).collect::<Vec<_>>();
                let mut f = vec![
                    Field::Label(fmt(&tr("%zu imagens"), &[&imgs.len()])),
                    Field::Combo { label: tr("Disposição"), items: s(&["Lado a lado", "Empilhadas", "Grade"]), value: o.layout },
                    Field::Int { label: tr("Colunas (grade)"), value: o.columns, min: 1, max: 20 },
                    Field::Int { label: tr("Espaço entre as imagens (px)"), value: o.gap, min: 0, max: 500 },
                    Field::Int { label: tr("Margem (px)"), value: o.margin, min: 0, max: 500 },
                    Field::Combo {
                        label: tr("Alinhamento"),
                        items: s(&["Início (topo/esquerda)", "Centro", "Fim (base/direita)"]),
                        value: o.align,
                    },
                    Field::Check { label: tr("Mesmo tamanho (ajusta altura ou largura)"), value: o.same_size },
                    Field::Check { label: tr("Fundo transparente"), value: o.transparent },
                    Field::Color { label: tr("Cor de fundo"), color: o.background },
                ];
                if !form::run(owner, &tr("Combinar imagens"), &mut f) {
                    return Ok(None);
                }
                let o = CombineOptions {
                    layout: f[1].int(),
                    columns: f[2].int(),
                    gap: f[3].int(),
                    margin: f[4].int(),
                    align: f[5].int(),
                    same_size: f[6].on(),
                    transparent: f[7].on(),
                    background: f[8].color(),
                };
                o.save();
                let n = imgs.len();
                Ok(Some((edtools::combine_images(&imgs, &o, None), n)))
            },
            |e, _, r| match r {
                Err(m) => e.status = m,
                Ok(None) => {}
                Ok(Some((out, _))) if out.is_empty() => e.warn(tr("A imagem combinada ficaria grande demais.")),
                Ok(Some((out, n))) => {
                    e.new_doc_from_image(out, &tr("Imagens combinadas"), &tr("Combinar imagens"));
                    e.status = fmt(&tr("%zu imagens combinadas numa imagem nova."), &[&n]);
                }
            },
        );
    }

    pub fn read_qr(&mut self) {
        if !self.require_image() {
            return;
        }
        snapui::qr::show(self.image_or_selection());
    }

    /// Beautify (C05): the last settings are remembered (Editor\BeautifyLast, as XML).
    pub fn beautify_current(&mut self) {
        if !self.require_image() {
            return;
        }
        let last = super::reg_str("BeautifyLast", "");
        let o = snapcore::xml::parse(last.as_bytes()).map(|n| presets::beautify_from_xml(&n)).unwrap_or_default();
        self.modal(move |owner| beautify_dialog(owner, &o), |e, _, r| {
            let Some(o) = r else { return };
            super::set_reg_str("BeautifyLast", &snapcore::xml::serialize(&presets::beautify_to_xml(&o, "Beautify"), false));
            e.commit_effect(&tr("Embelezar"), false, |base| beautify::beautify(base, &o));
        });
    }

    /// Redact sensitive data (C11): OCR words + patterns and faces, each one a Blur/Pixelate/black object.
    pub fn redact_sensitive(&mut self) {
        if !self.require_image() {
            return;
        }
        let img = self.cv.doc().map(|d| d.render(None)).unwrap_or_default();
        self.status = tr("Procurando dados sensíveis…");
        let faces = reg_dw("RedactFaces", 1) != 0;
        let style0 = (reg_dw("RedactStyle", 0) as i32).clamp(0, 2);
        self.modal(
            move |owner| {
                let (lines, err) = match snapcore::ocr::run_ocr_lines(&img) {
                    Ok(l) => (l, String::new()),
                    Err(e) => (Vec::new(), e.message()),
                };
                let mut items = edtools::find_sensitive(&lines);
                if faces && let Ok(f) = snapcore::ocr::detect_faces(&img) {
                    items.extend(f.into_iter().map(|r| RedactItem { kind: tr("Rosto"), text: String::new(), rect: r, on: true }));
                }
                if items.is_empty() {
                    let m = if err.is_empty() {
                        tr("Nenhum dado sensível encontrado (e-mail, telefone, cartão, documentos, links com token, chaves, rostos).")
                    } else {
                        tr("OCR indisponível: ") + &err
                    };
                    native::message(owner, &m, &tr("Ocultar dados sensíveis"), windows::Win32::UI::WindowsAndMessaging::MB_ICONINFORMATION);
                    return None;
                }
                let mut f = vec![
                    Field::Combo { label: tr("Ocultar com"), items: vec![tr("Desfoque"), tr("Pixelização"), tr("Tarja preta")], value: style0 },
                    Field::Label(fmt(&tr("%zu itens encontrados — desmarque o que deve ficar visível:"), &[&items.len()])),
                ];
                let per_item = items.len().min(40);
                for it in &items[..per_item] {
                    let t: String = if it.text.chars().count() > 34 { it.text.chars().take(32).collect::<String>() + "…" } else { it.text.clone() };
                    f.push(Field::Check { label: format!("{}: {t}", it.kind), value: it.on });
                }
                let mut rest: Vec<String> = Vec::new();
                for it in &items[per_item..] {
                    if !rest.contains(&it.kind) {
                        rest.push(it.kind.clone());
                    }
                }
                for k in &rest {
                    f.push(Field::Check { label: fmt(&tr("Outros: %s"), &[k]), value: true });
                }
                if !form::run(owner, &tr("Ocultar dados sensíveis"), &mut f) {
                    return None;
                }
                for (i, it) in items.iter_mut().enumerate() {
                    it.on = if i < per_item {
                        f[2 + i].on()
                    } else {
                        f[2 + per_item + rest.iter().position(|k| *k == it.kind).unwrap_or(0)].on()
                    };
                }
                Some((items, f[0].int()))
            },
            |e, _, r| {
                e.status.clear();
                let Some((items, style)) = r else { return };
                set_reg_dw("RedactStyle", style as u32);
                let mut objs = Vec::new();
                for it in items.iter().filter(|i| i.on) {
                    let mut o = DrawObj {
                        p1: PointF { X: it.rect.left as f32, Y: it.rect.top as f32 },
                        p2: PointF { X: it.rect.right as f32, Y: it.rect.bottom as f32 },
                        name: it.kind.clone(),
                        ..Default::default()
                    };
                    if style == 2 {
                        o.kind = ObjType::Shape;
                        o.style = e.cv.styles[Tool::Shape.index()].clone();
                        o.style.shape_kind = SHAPE_RECT;
                        o.style.fill = 0xFF00_0000;
                        o.style.stroke = 0xFF00_0000;
                        o.style.width = 1.0;
                        o.style.shadow = false;
                        o.style.opacity = 100;
                        o.style.grad = Default::default();
                    } else {
                        o.kind = ObjType::Blur;
                        o.style = e.cv.styles[Tool::Blur.index()].clone();
                        o.style.hide_kind = style;
                        o.style.area_shape = SHAPE_RECT;
                        o.style.opacity = 100;
                        o.style.amount = o.style.amount.max(12);
                    }
                    objs.push(o);
                }
                if objs.is_empty() {
                    return;
                }
                e.cv.paste_objects(&objs, true, true);
                e.doc_changed();
                e.status = fmt(
                    &tr("%zu áreas ocultadas — cada uma é um objeto (pode ser movida, redimensionada ou excluída)."),
                    &[&objs.len()],
                );
            },
        );
    }

    /// Remove background (C27): the selection (cropped) or the whole image.
    pub fn remove_background_now(&mut self) {
        if !self.require_image() {
            return;
        }
        let sel = self.cv.has_raster_selection();
        let full = self.cv.doc().map(|d| d.render(None)).unwrap_or_default();
        let src = if sel { full.crop(self.cv.raster_selection_bounds()) } else { full };
        if src.is_empty() {
            return;
        }
        let (tol, feather) = (reg_dw("RemoveBgTolerance", 30) as i32, reg_dw("RemoveBgFeather", 1) as i32);
        self.modal(
            move |owner| {
                let mut f = vec![
                    Field::Int { label: tr("Tolerância (1–100)"), value: tol, min: 1, max: 100 },
                    Field::Int { label: tr("Borda suave (px)"), value: feather, min: 0, max: 20 },
                    Field::Label(if sel {
                        tr("A imagem será cortada para a seleção.")
                    } else {
                        tr("O fundo é o que toca as bordas da imagem e tem as cores delas.")
                    }),
                ];
                if !form::run(owner, &tr("Remover o fundo"), &mut f) {
                    return None;
                }
                set_reg_dw("RemoveBgTolerance", f[0].int() as u32);
                set_reg_dw("RemoveBgFeather", f[1].int() as u32);
                Some(edtools::remove_background(&src, f[0].int(), f[1].int()))
            },
            |e, _, r| {
                let Some(out) = r else { return };
                e.cv.clear_raster_selection();
                let Some(d) = e.cv.doc_mut() else { return };
                d.checkpoint();
                d.st.objs.clear(); // the objects are part of the processed image
                d.set_base(out);
                e.cv.selected = -1;
                e.after_image_change();
                e.status = tr("Fundo removido (Ctrl+Z desfaz). Salve como PNG ou WebP para manter a transparência.");
            },
        );
    }

    /// Split (C34): asks how, then each part becomes a new image.
    pub fn split_current(&mut self) {
        if !self.require_image() {
            return;
        }
        let Some(d) = self.cv.doc() else { return };
        let (img, base) = (d.render(None), d.title.clone());
        self.modal(
            move |owner| {
                let tall = img.h >= img.w;
                let len = if tall { img.hi() } else { img.wi() };
                let mut f = vec![
                    Field::Label(fmt(&tr("Imagem de %d × %d px"), &[&img.w, &img.h])),
                    Field::Combo {
                        label: tr("Cortar"),
                        items: vec![tr("Na horizontal (partes empilhadas)"), tr("Na vertical (partes lado a lado)")],
                        value: if tall { 0 } else { 1 },
                    },
                    Field::Combo { label: tr("Dividir por"), items: vec![tr("Tamanho de cada parte (px)"), tr("Número de partes")], value: 1 },
                    Field::Int { label: tr("Tamanho (px)"), value: (len / 2).max(100), min: 16, max: 100000 },
                    Field::Int { label: tr("Número de partes"), value: 2, min: 2, max: 100 },
                    Field::Check { label: tr("Não cortar linhas de texto (quebras inteligentes)"), value: true },
                ];
                if !form::run(owner, &tr("Dividir imagem"), &mut f) {
                    return None;
                }
                let vertical = f[1].int() == 1;
                let cuts = edtools::split_cuts(&img, vertical, f[2].int() == 1, f[3].int(), f[4].int(), f[5].on());
                (!cuts.is_empty()).then(|| edtools::split_image(&img, &cuts, vertical))
            },
            move |e, _, r| {
                let Some(parts) = r else { return };
                let n = parts.len();
                for (i, p) in parts.into_iter().enumerate() {
                    e.new_doc_from_image(p, &fmt(&tr("%s — parte %zu"), &[&base, &(i + 1)]), &tr("Imagem dividida"));
                }
                e.status = fmt(&tr("Imagem dividida em %zu partes (imagens novas na bandeja)."), &[&n]);
            },
        );
    }

    pub fn insert_date_stamp(&mut self) {
        if !self.require_image() {
            return;
        }
        let t = edtools::date_stamp_text(&edtools::date_stamp_format(), &DateTime::now());
        self.cv.paste_text(&t, None);
        self.doc_changed();
    }

    pub fn date_stamp_format(&mut self) {
        self.modal(
            |owner| {
                let now = DateTime::now();
                let mut f = vec![
                    Field::Text { label: tr("Formato"), text: edtools::date_stamp_format() },
                    Field::Label(tr("Vazio = data e hora curtas. Códigos: yyyy ano · MM mês · MMM/MMMM nome do mês · dd dia · ddd/dddd dia da semana · HH hora (24 h) · hh hora (12 h) · mm minutos · ss segundos · tt AM/PM · '…' texto")),
                    Field::Label(tr("Exemplo: ") + &edtools::date_stamp_text("dddd, dd/MM/yyyy HH:mm", &now)),
                ];
                if !form::run(owner, &tr("Formato da data e hora"), &mut f) {
                    return None;
                }
                edtools::set_date_stamp_format(f[0].text().trim());
                Some(edtools::date_stamp_text(&edtools::date_stamp_format(), &now))
            },
            |e, _, r| {
                if let Some(t) = r {
                    e.status = tr("Data e hora: ") + &t;
                }
            },
        );
    }

    pub fn contrast_now(&mut self, ctl: &mut Ctl) {
        let mut cols = (0, 0x00FF_FFFF);
        if self.cv.doc().is_some_and(|d| !d.is_video) && self.cv.has_raster_selection()
            && let Some(c) = edtools::dominant_colors(&self.cv.selection_image())
        {
            cols = c;
        }
        self.show_contrast(ctl, cols.0, cols.1);
    }

    pub fn assistant(&mut self, ctl: &mut Ctl) {
        if !self.require_image() {
            return;
        }
        let img = self.cv.doc().map(|d| d.render(None)).unwrap_or_default();
        if super::windows_aux::AiSettings::load().enabled {
            self.show_assistant(ctl, img);
            return;
        }
        self.modal(
            |owner| {
                let r = native::message(
                    owner,
                    &tr("O assistente de IA está desligado. Ele envia a imagem para um provedor de IA escolhido por você, com a sua chave.\n\nConfigurar agora?"),
                    &tr("Assistente de IA"),
                    windows::Win32::UI::WindowsAndMessaging::MB_YESNO | windows::Win32::UI::WindowsAndMessaging::MB_ICONINFORMATION,
                );
                r == windows::Win32::UI::WindowsAndMessaging::IDYES.0
                    && super::windows_aux::assistant_settings_dialog(owner)
                    && super::windows_aux::AiSettings::load().enabled
            },
            move |e, ctl, ok| {
                if ok {
                    e.show_assistant(ctl, img);
                }
            },
        );
    }

    pub fn assistant_settings(&mut self) {
        self.modal(super::windows_aux::assistant_settings_dialog, |_, _, _| {});
    }

    /// Compare (C17): two library items, or the current image and a file, or two files.
    pub fn compare_now(&mut self, ctl: &mut Ctl, paths: Vec<String>) {
        let paths: Vec<String> = paths.into_iter().filter(|p| ext_of(p) != ".mp4").collect();
        if paths.len() >= 2 {
            match (self.load_item_image(&paths[0]), self.load_item_image(&paths[1])) {
                (Some(a), Some(b)) => self.show_compare(ctl, a, b),
                _ => self.status = tr("Não foi possível abrir as imagens para comparar."),
            }
            return;
        }
        let cur = if self.cv.doc().is_some_and(|d| !d.is_video) {
            self.cv.commit_text();
            self.cv.doc().map(|d| (d.render(None), d.title.clone(), d.path.clone()))
        } else {
            None
        };
        self.modal(
            move |owner| {
                let (a, b) = match cur {
                    Some((img, title, path)) => {
                        let p = paths.first().filter(|p| **p != path).cloned().or_else(|| {
                            native::open_file(owner, &tr("Comparar com…"), &IMAGE_FILTER.replacen("Imagens", &tr("Imagens"), 1))
                        })?;
                        (Some((img, title)), load_file_image(&p))
                    }
                    None => {
                        let files = pick_images(owner);
                        if files.len() < 2 {
                            return Some(Err(tr("Escolha duas imagens para comparar.")));
                        }
                        (load_file_image(&files[0]), load_file_image(&files[1]))
                    }
                };
                Some(match (a, b) {
                    (Some(a), Some(b)) => Ok((a, b)),
                    _ => Err(tr("Não foi possível abrir as imagens para comparar.")),
                })
            },
            |e, ctl, r| match r {
                Some(Ok((a, b))) => e.show_compare(ctl, a, b),
                Some(Err(m)) => e.status = m,
                None => {}
            },
        );
    }

    /// Batch processing (C28): quick effects and Beautify presets as the effects.
    pub fn run_batch(&mut self, files: Vec<String>) {
        let files: Vec<String> = files.into_iter().filter(|f| ext_of(f) != ".mp4").collect();
        let open: HashMap<String, Image> =
            files.iter().filter_map(|p| self.find_doc(p).map(|i| (p.clone(), self.cv.docs[i].render(None)))).collect();
        let quick = self.fx.quick.clone();
        self.modal(
            move |owner| {
                let files = if files.is_empty() { pick_images(owner) } else { files };
                if files.is_empty() {
                    return;
                }
                let mut effects: Vec<(String, Box<dyn Fn(&Image) -> Image>)> = Vec::new();
                for q in quick {
                    effects.push((tr(&q.name), Box::new(move |i: &Image| apply_quick(&q, i, 1.0).0)));
                }
                for p in beautify_presets() {
                    let o = p.opt;
                    let n = if p.builtin { tr(&p.name) } else { p.name };
                    effects.push((tr("Embelezar: ") + &n, Box::new(move |i: &Image| beautify::beautify(i, &o).0)));
                }
                let load = |p: &str| open.get(p).cloned().or_else(|| load_file_image(p).map(|x| x.0));
                batch_dialog(owner, files, &load, &effects);
            },
            |_, _, _| {},
        );
    }

    /// Pin (C01): the capture process pins the .snap (with its objects) or a PNG copy.
    pub fn pin_path(&mut self, p: &str) {
        let sent = ipc::mutex_exists(&snapcore::wide::to_wide(ipc::CAPTURE_MUTEX))
            && self.ipc.as_ref().is_some_and(|i| i.send_string(Channel::Capture, ipc::code::PIN_FILE, p, false));
        if !sent {
            instance::launch_sibling("Snapnab.exe", &format!("/hide /pin \"{p}\""), false);
        }
        self.status = tr("Imagem fixada na tela.");
    }

    pub fn pin_current(&mut self) {
        if !self.require_image() {
            return;
        }
        if let Some(i) = self.cv.cur {
            self.autosave(i);
        }
        let p = self.cv.doc().map(|d| d.path.clone()).unwrap_or_default();
        let p = if ext_of(&p) == ".snap" && Path::new(&p).exists() { p } else { self.temp_export() };
        self.pin_path(&p);
    }

    /// Upload (C21): off until a destination is configured; the link replaces the image on the clipboard (option).
    pub fn upload_item(&mut self, item: String, img: Option<Image>, name: String) {
        self.status = tr("Enviando…");
        self.modal(
            move |owner| {
                if !Settings::load().configured() && (!snapui::upload_ui::configure_now(owner) || !Settings::load().configured()) {
                    return None;
                }
                let r = match &img {
                    Some(i) => upload::upload_image(i, &name, VERSION),
                    None => upload::upload_file(&Settings::load(), &item.encode_utf16().collect::<Vec<_>>(), &file_name_of(&item)),
                };
                if r.ok {
                    if !item.is_empty() {
                        upload::set_uploaded_url(&item.encode_utf16().collect::<Vec<_>>(), &r.url);
                    }
                    if Settings::load().copy_link {
                        snapcore::clip::copy_text(owner, &r.url);
                    }
                }
                Some(r)
            },
            |e, _, r| {
                let Some(r) = r else {
                    e.status.clear();
                    return;
                };
                if !r.ok {
                    e.status = tr("Falha no envio.");
                    e.warn(tr("Falha no envio: ") + &r.error);
                    return;
                }
                let copied = Settings::load().copy_link;
                e.status = tr("Enviado: ") + &r.url + &if copied { tr(" (link copiado)") } else { String::new() };
            },
        );
    }

    pub fn upload_current(&mut self) {
        let Some(d) = self.cv.doc() else { return };
        if d.is_video {
            let (p, t) = (d.path.clone(), d.title.clone());
            self.upload_item(p, None, t);
            return;
        }
        self.cv.commit_text();
        let Some(d) = self.cv.doc() else { return };
        let (p, img, t) = (d.path.clone(), d.render(None), d.title.clone());
        self.upload_item(p, Some(img), t);
    }

    /// Copy as (C30): the menu opens from the window procedure (COPY_AS_MENU).
    pub fn copy_as(&mut self, path: String, img: Image, title: String) {
        self.aux.pending_copy_as = Some((path, img, title));
        native::post_command(self.hwnd, COPY_AS_MENU);
    }

    pub fn copy_as_menu(&mut self) {
        let Some((path, img, title)) = self.aux.pending_copy_as.take() else { return };
        let url = upload::uploaded_url(&path.encode_utf16().collect::<Vec<_>>());
        if copy_as_menu(self.hwnd, &img, &path, &title, &url) {
            self.status = tr("Copiado.");
        }
    }

    pub fn print_current(&mut self) {
        if !self.require_image() {
            return;
        }
        let Some(d) = self.cv.doc() else { return };
        let mut img = Image::sized(d.w(), d.h(), 0xFFFF_FFFF);
        img.blit(&d.render(None), 0, 0, true);
        let title = d.title.clone();
        self.modal(move |owner| print_image(owner, img, title), |e, _, r| {
            if let Err(m) = r {
                e.inform(m, tr("Imprimir"));
            }
        });
    }

    /// F1 / Help › Manual: the chapter of what is on screen (library, video or the current tool).
    pub fn show_manual(&mut self) {
        const TOOL_TOPIC: [&str; 21] = [
            "tool-move", "tool-arrow", "tool-line", "tool-shape", "tool-pen", "tool-highlighter", "tool-text", "tool-callout",
            "tool-steps", "tool-stamp", "tool-blur", "tool-spotlight", "tool-magnify", "tool-fill", "tool-eraser", "tool-crop",
            "tool-selection", "tool-cutout", "tool-eyedropper", "tool-favorites", "tool-magicwand",
        ];
        let topic = if self.library_open {
            "tray-library"
        } else if self.cv.doc().is_some_and(|d| d.is_video) {
            "video-editing"
        } else if self.cv.doc().is_some() {
            TOOL_TOPIC.get(self.cv.tool.index()).copied().unwrap_or("editor-tour")
        } else {
            "editor-tour"
        };
        let (owner, topic) = (self.hwnd, topic.to_string());
        std::thread::spawn(move || open_manual(owner, &topic));
    }
}

// ---------------------------------------------------------------- batch

const BATCH_EXT: [&str; 8] = ["", ".png", ".jpg", ".webp", ".avif", ".bmp", ".gif", ".tif"];

fn default_batch_folder() -> String {
    use windows::Win32::UI::Shell::{FOLDERID_Pictures, KNOWN_FOLDER_FLAG, SHGetKnownFolderPath};
    unsafe {
        SHGetKnownFolderPath(&FOLDERID_Pictures, KNOWN_FOLDER_FLAG(0), None)
            .map(|p| {
                let s = p.to_string().unwrap_or_default();
                windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as _));
                format!("{s}\\Snapnab\\{}", tr("Lote"))
            })
            .unwrap_or_default()
    }
}

fn batch_reg_str(name: &str, def: &str) -> String {
    reg::get_string(name.as_ref(), Some("Editor".as_ref())).map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| def.to_string())
}

/// The output name: {name} {n} {date}, without characters Windows refuses.
pub fn batch_output_name(pattern: &str, src: &str, n: usize, ext: &str, date: &str) -> String {
    let mut name = file_name_of(src);
    if let Some(d) = name.rfind('.') {
        name.truncate(d);
    }
    let p = if pattern.trim().is_empty() { "{name}" } else { pattern.trim() };
    let out = p.replace("{name}", &name).replace("{n}", &format!("{n:03}")).replace("{date}", date);
    out.chars().map(|c| if "\\/:*?\"<>|".contains(c) { '_' } else { c }).collect::<String>() + ext
}

/// BatchDialog (Batch.cpp): resize, an effect, watermark, format and quality, metadata, names; progress with cancel.
fn batch_dialog(owner: isize, files: Vec<String>, load: &dyn Fn(&str) -> Option<Image>, effects: &[(String, Box<dyn Fn(&Image) -> Image>)]) {
    let s = |v: &[&str]| v.iter().map(|x| tr(x)).collect::<Vec<_>>();
    let mut fx_names = vec![tr("(nenhum)")];
    fx_names.extend(effects.iter().map(|e| e.0.clone()));
    let mut formats = vec![tr("O mesmo do original")];
    formats.extend(["PNG", "JPEG", "WebP", "AVIF", "BMP", "GIF", "TIFF"].map(String::from));
    let mut f = vec![
        Field::Label(fmt(&tr("%zu imagens — os originais não são alterados."), &[&files.len()])),
        Field::Combo { label: tr("Redimensionar"), items: s(&["Não", "Porcentagem", "Largura/altura máxima"]), value: (reg_dw("BatchResize", 0) as i32).clamp(0, 2) },
        Field::Int { label: tr("Porcentagem"), value: (reg_dw("BatchPercent", 50) as i32).clamp(1, 1000), min: 1, max: 1000 },
        Field::Int { label: tr("Largura máxima (0 = livre)"), value: reg_dw("BatchMaxWidth", 1280) as i32, min: 0, max: 100000 },
        Field::Int { label: tr("Altura máxima (0 = livre)"), value: reg_dw("BatchMaxHeight", 0) as i32, min: 0, max: 100000 },
        Field::Combo { label: tr("Efeito"), items: fx_names, value: 0 },
        Field::Text { label: tr("Marca d'água (texto)"), text: String::new() },
        Field::Int { label: tr("Opacidade da marca d'água (%)"), value: 30, min: 5, max: 100 },
        Field::Combo { label: tr("Formato"), items: formats, value: (reg_dw("BatchFormat", 0) as i32).clamp(0, 7) },
        Field::Int { label: tr("Qualidade (JPEG, WebP, AVIF)"), value: (reg_dw("BatchQuality", 90) as i32).clamp(1, 100), min: 1, max: 100 },
        Field::Check { label: tr("Remover metadados (EXIF, GPS…)"), value: reg_dw("BatchStripMetadata", 1) != 0 },
        Field::Text { label: tr("Nome dos arquivos"), text: batch_reg_str("BatchPattern", "{name}") },
        Field::Label(tr("{name} nome original · {n} número · {date} data de hoje")),
        Field::Text { label: tr("Pasta de destino"), text: batch_reg_str("BatchFolder", &default_batch_folder()) },
    ];
    let folder = loop {
        if !form::run(owner, &tr("Processamento em lote"), &mut f) {
            return;
        }
        let dir = f[13].text().trim().to_string();
        let problem = if dir.is_empty() {
            tr("Escolha a pasta de destino.")
        } else if std::fs::create_dir_all(&dir).is_err() {
            tr("Não foi possível criar a pasta de destino.")
        } else if f[8].int() == 4 && !snapcore::codec::can_encode(snapcore::codec::Format::Avif) {
            snapcore::codec::codec_hint(snapcore::codec::Format::Avif)
        } else {
            break dir;
        };
        native::message(owner, &problem, &tr("Processamento em lote"), windows::Win32::UI::WindowsAndMessaging::MB_ICONWARNING);
    };
    let (resize, percent, max_w, max_h, effect) = (f[1].int(), f[2].int(), f[3].int(), f[4].int(), f[5].int());
    let (watermark, wm_opacity, format, quality, strip) = (f[6].text().trim().to_string(), f[7].int(), f[8].int(), f[9].int(), f[10].on());
    let pattern = f[11].text().to_string();
    set_reg_dw("BatchResize", resize as u32);
    set_reg_dw("BatchPercent", percent as u32);
    set_reg_dw("BatchMaxWidth", max_w as u32);
    set_reg_dw("BatchMaxHeight", max_h as u32);
    set_reg_dw("BatchFormat", format as u32);
    set_reg_dw("BatchQuality", quality as u32);
    set_reg_dw("BatchStripMetadata", strip as u32);
    reg::set_string("BatchPattern".as_ref(), pattern.as_ref(), Some("Editor".as_ref()));
    reg::set_string("BatchFolder".as_ref(), folder.as_ref(), Some("Editor".as_ref()));

    let process = |src: &Image| -> Image {
        let mut img = src.clone();
        if resize == 1 && percent != 100 {
            img = fx::scale(&img, percent);
        } else if resize == 2 && (max_w > 0 || max_h > 0) {
            let mut k: f64 = 1.0;
            if max_w > 0 && img.wi() > max_w {
                k = k.min(max_w as f64 / img.w as f64);
            }
            if max_h > 0 && img.hi() > max_h {
                k = k.min(max_h as f64 / img.h as f64);
            }
            if k < 1.0 {
                img = scale_image(&img, ((img.w as f64 * k).round() as i32).max(1), ((img.h as f64 * k).round() as i32).max(1), true);
            }
        }
        if effect > 0 && let Some(e) = effects.get(effect as usize - 1) {
            img = e.1(&img);
        }
        if !watermark.is_empty() {
            fx::watermark(&mut img, &watermark.encode_utf16().collect::<Vec<_>>(), wm_opacity, true, false);
        }
        img.meta = src.meta.clone();
        img
    };
    let date = {
        let t = DateTime::now();
        format!("{:04}-{:02}-{:02}", t.year, t.month, t.day)
    };
    let (mut ok, mut failed, mut first_error) = (0, 0, String::new());
    let total = files.len();
    let progress = native::Progress::open(owner, &tr("Processamento em lote"));
    for (i, file) in files.iter().enumerate() {
        if progress.as_ref().is_some_and(|p| p.canceled()) {
            break;
        }
        if let Some(p) = &progress {
            p.set(&file_name_of(file), i, total);
        }
        let Some(src) = load(file).filter(|s| !s.is_empty()) else {
            failed += 1;
            if first_error.is_empty() {
                first_error = file_name_of(file);
            }
            continue;
        };
        let mut ext = BATCH_EXT[format.clamp(0, 7) as usize].to_string();
        let mut opt = SaveOptions { jpeg_quality: quality, webp_quality: quality, avif_quality: quality, webp_lossless: false, strip_metadata: strip, ..SaveOptions::from_settings() };
        if ext.is_empty() {
            let f = Format::from_ext(&ext_of(file));
            ext = match f {
                Format::Unknown | Format::Heic => ".png".into(),
                _ => ext_of(file),
            };
            if f == Format::Webp {
                opt.webp_lossless = true;
            }
        }
        let out = process(&src);
        let name = batch_output_name(&pattern, file, i + 1, &ext, &date);
        let mut path = format!("{folder}\\{name}");
        let mut n = 2;
        while Path::new(&path).exists() {
            let dot = name.rfind('.').unwrap_or(name.len());
            path = format!("{folder}\\{}_{n}{}", &name[..dot], &name[dot..]);
            n += 1;
        }
        if codec::save_file(Path::new(&path), &out, Format::from_ext(&ext), &opt, VERSION) {
            ok += 1;
        } else {
            failed += 1;
            if first_error.is_empty() {
                first_error = file_name_of(file);
            }
        }
    }
    drop(progress);
    let mut msg = fmt(&tr("%d imagens processadas em:\n%s"), &[&ok, &folder]);
    if failed > 0 {
        msg += &fmt(&tr("\n\n%d falharam (primeira: %s)."), &[&failed, &first_error]);
    }
    msg += &tr("\n\nAbrir a pasta?");
    use windows::Win32::UI::WindowsAndMessaging::{IDYES, MB_ICONINFORMATION, MB_ICONWARNING, MB_YESNO};
    if native::message(owner, &msg, &tr("Processamento em lote"), MB_YESNO | if failed > 0 { MB_ICONWARNING } else { MB_ICONINFORMATION }) == IDYES.0 {
        shell_open(&folder);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_and_names() {
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
        assert_eq!(batch_output_name("{name}_{n}", "C:\\a\\shot.final.png", 7, ".jpg", "2026-01-02"), "shot.final_007.jpg");
        assert_eq!(batch_output_name("", "x.png", 1, ".png", ""), "x.png");
        assert_eq!(batch_output_name("{date}: {name}", "x.png", 1, ".png", "2026-01-02"), "2026-01-02_ x.png");
    }
}
