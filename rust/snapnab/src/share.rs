//! The capture's Office and upload outputs (SendToOffice and UploadCapture in the C++ main.cpp), each on its own
//! thread so the app keeps drawing while Word opens or the file goes up; the results come back as notifications.

use snapcore::image::Image;
use snapcore::office::{self, Target};
use snapcore::upload::{self, Settings};
use snapcore::{codec, library};
use snapnab_app::tr;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{IDYES, MB_ICONQUESTION, MB_ICONWARNING, MB_YESNO, MessageBoxW, PostMessageW, WM_COMMAND};
use windows::core::PCWSTR;

use crate::pins::{UiEv, send};

fn message(text: &str, flags: windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE) -> windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_RESULT {
    let (t, c) = (snapcore::wide::to_wide(text), snapcore::wide::to_wide("Snapnab"));
    unsafe { MessageBoxW(None, PCWSTR(t.as_ptr()), PCWSTR(c.as_ptr()), flags) }
}

fn stamp() -> String {
    String::from_utf16_lossy(&library::time_stamp(false))
}

/// TempImageFile: %TEMP%\Snapnab\<name>.png
fn temp_png(img: &Image, name: &str) -> Option<String> {
    let dir = std::env::temp_dir().join("Snapnab");
    let _ = std::fs::create_dir_all(&dir);
    let name: String = name.chars().map(|c| if "\\/:*?\"<>|".contains(c) { '_' } else { c }).collect();
    let path = dir.join(format!("{name}.png"));
    codec::save_file(&path, img, codec::Format::Png, &codec::SaveOptions::from_settings(), crate::VERSION)
        .then(|| path.to_string_lossy().into_owned())
}

/// SendToOffice: Word, PowerPoint, Excel or OneNote (Preset.office 0–3).
pub fn office(img: Image, target: i32) {
    let Some(t) = Target::from_i32(target) else { return };
    std::thread::spawn(move || {
        let Some(file) = temp_png(&img, &format!("Snapnab_{}", stamp())) else { return };
        match office::send(t, &file, img.w as i32, img.h as i32) {
            Ok(()) => send(UiEv::Notify("Snapnab".into(), tr("Captura enviada para o %s.").replace("%s", t.name()))),
            Err(e) => {
                message(&e, MB_ICONWARNING);
            }
        }
    });
}

/// CopyAsMenu (Clipboard.cpp), for the quick access overlay: the image, the file (the video itself or a PNG copy),
/// its path, Base64 and data URLs.
pub fn copy_as_menu(owner: isize, at: (i32, i32), img: &Image, file: &str, title: &str) -> bool {
    use crate::tray::{Item, Menu};
    let video = file.to_lowercase().ends_with(".mp4");
    let t = [tr("Imagem"), tr("Arquivo (para colar no Explorer, e-mail, chat)"), tr("Caminho do arquivo"), tr("Base64 (PNG)"),
             tr("Data URL (PNG)"), tr("Data URL (WebP)")];
    let mut items = Vec::new();
    if !video {
        items.push(Item::Cmd(1, &t[0]));
    }
    items.push(Item::Cmd(2, &t[1]));
    items.push(Item::Cmd(3, &t[2]));
    if !video {
        items.push(Item::Separator);
        items.push(Item::Cmd(4, &t[3]));
        items.push(Item::Cmd(5, &t[4]));
        items.push(Item::Cmd(6, &t[5]));
    }
    let id = Menu::new(items).choose(owner, at.0, at.1);
    // the file for File / File path: the video itself, or a PNG copy of the image (the library keeps .snap)
    let real_file = || -> Option<String> {
        if video || (!file.is_empty() && !file.to_lowercase().ends_with(".snap")) {
            return Some(file.to_string());
        }
        let mut name = if title.is_empty() { format!("Snapnab_{}", stamp()) } else { title.to_string() };
        if let Some(i) = name.rfind('.')
            && codec::Format::from_ext(&name[i..].to_lowercase()) != codec::Format::Unknown
        {
            name.truncate(i); // "shot.png" -> "shot"
        }
        snapcore::dragout::temp_image_file(img, &name, false, crate::VERSION)
    };
    let enc = |f| snapcore::net::base64(&codec::encode(img, f, &codec::SaveOptions::from_settings(), crate::VERSION).unwrap_or_default());
    match id {
        1 => snapcore::clip::copy_image(owner, img, crate::VERSION),
        2 => real_file().is_some_and(|f| snapcore::clip::copy_files(owner, &[f])),
        3 => real_file().is_some_and(|f| snapcore::clip::copy_text(owner, &f)),
        4 => snapcore::clip::copy_text(owner, &enc(codec::Format::Png)),
        5 => snapcore::clip::copy_text(owner, &format!("data:image/png;base64,{}", enc(codec::Format::Png))),
        6 => snapcore::clip::copy_text(owner, &format!("data:image/webp;base64,{}", enc(codec::Format::Webp))),
        _ => false,
    }
}

/// UploadCapture: asks to set the destination up when there is none, then sends; the link goes to the clipboard
/// (CopyLink) and to the library item (Uploads.xml).
pub fn upload(owner: isize, img: Image, lib: String, title: String) {
    std::thread::spawn(move || {
        if !Settings::load().configured() {
            if message(&tr("O upload ainda não está configurado. Configurar agora?"), MB_YESNO | MB_ICONQUESTION) == IDYES {
                unsafe {
                    let _ = PostMessageW(Some(HWND(owner as _)), WM_COMMAND, WPARAM(crate::id::UPLOAD_SETTINGS as usize), LPARAM(0));
                }
            }
            return;
        }
        send(UiEv::Notify("Snapnab".into(), tr("Enviando a captura…")));
        let base: String = if title.is_empty() { "Snapnab".into() } else { title.chars().take(60).collect() };
        let r = upload::upload_image(&img, &format!("{base}_{}", stamp()), crate::VERSION);
        if !r.ok {
            message(&(tr("O upload falhou.\n\n") + &r.error), MB_ICONWARNING);
            return;
        }
        if !lib.is_empty() {
            upload::set_uploaded_url(&lib.encode_utf16().collect::<Vec<_>>(), &r.url);
        }
        if Settings::load().copy_link {
            snapcore::clip::copy_text(owner, &r.url);
        }
        send(UiEv::Notify(tr("Upload concluído — link copiado"), r.url));
    });
}
