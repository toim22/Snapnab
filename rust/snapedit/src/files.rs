//! Files of the Editor: images in and out, PDF, the library's names and thumbnails.

use std::path::Path;

use snapcore::codec::{self, Format, SaveOptions};
use snapcore::image::Image;
use snapcore::library;

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

pub fn library_dir() -> String {
    String::from_utf16_lossy(&library::library_dir())
}

pub fn load_image(path: &str) -> Option<Image> {
    codec::load(Path::new(path)).ok()
}

/// Why an image file could not be opened (the codec to install, an invalid WebP…), or "".
pub fn load_error(path: &str) -> String {
    match codec::load(Path::new(path)) {
        Ok(_) => String::new(),
        Err(e) => e.text(Format::from_ext(&crate::doc::ext_of(path))),
    }
}

/// Writes an image file; the error is why it failed ("" when there is nothing more to say).
pub fn save_image(path: &str, img: &Image, jpeg_quality: i32) -> Result<(), String> {
    let ext = crate::doc::ext_of(path);
    let opt = SaveOptions { jpeg_quality, ..SaveOptions::from_settings() };
    codec::save_file_why(Path::new(path), img, Format::from_ext(&ext), &opt, crate::VERSION)
}

pub fn save_pdf(path: &str, img: &Image, links: &[(snapcore::doc::RectF, String)]) -> Result<(), String> {
    let links: Vec<snapcore::pdf::Link> = links
        .iter()
        .map(|(r, u)| snapcore::pdf::Link { x: r.X, y: r.Y, w: r.Width, h: r.Height, url: w(u) })
        .collect();
    snapcore::pdf::save_pdf(&w(path), img, &links).map_err(|e| format!("{e:?}"))
}

/// The name the capture was given in the library (Names.xml), or "".
pub fn capture_name(path: &str) -> String {
    let p = w(path);
    String::from_utf16_lossy(&library::name(&library::dir_of(&p), &p))
}

pub fn set_capture_name(path: &str, name: &str) -> bool {
    let p = w(path);
    library::set_name(&library::dir_of(&p), &p, &w(name))
}

/// The library thumbnail of an item (160 × 120 at most).
pub fn write_thumbnail(path: &str, img: &Image) -> bool {
    let p = w(path);
    let t = snapcore::gdip::make_thumbnail(img, 160, 120);
    library::write_thumbnail(&library::dir_of(&p), &p, &t, crate::VERSION)
}

/// A new .snap path in the library.
pub fn new_library_path() -> String {
    let dir = library_dir();
    format!("{dir}\\{}.snap", String::from_utf16_lossy(&library::new_guid()))
}

/// The library thumbnail of an item, if it was written.
pub fn load_thumbnail(path: &str) -> Option<Image> {
    let p = w(path);
    let t = String::from_utf16_lossy(&library::thumb_path_for(&library::dir_of(&p), &p));
    load_image(&t)
}

/// Pictures\\Snapnab (or the OutputFolder setting), created if missing.
pub fn default_save_dir() -> String {
    use windows::Win32::UI::Shell::{FOLDERID_Pictures, KNOWN_FOLDER_FLAG, SHGetKnownFolderPath};
    let set = snapcore::reg::get_string(std::ffi::OsStr::new("OutputFolder"), None).map(|s| s.to_string_lossy().into_owned());
    let d = match set.filter(|s| !s.trim().is_empty()) {
        Some(s) => s,
        None => {
            let pics = unsafe { SHGetKnownFolderPath(&FOLDERID_Pictures, KNOWN_FOLDER_FLAG(0), None) }
                .map(|p| unsafe {
                    let s = p.to_string().unwrap_or_default();
                    windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *const _));
                    s
                })
                .unwrap_or_default();
            format!("{pics}\\Snapnab")
        }
    };
    let _ = std::fs::create_dir_all(&d);
    d
}

/// The file name of the document's title, without the characters Windows refuses.
pub fn safe_name(title: &str, extra: &str) -> String {
    title.chars().map(|c| if "\\/:*?\"<>|".contains(c) || extra.contains(c) { '_' } else { c }).collect()
}

/// Writes the composed image to a temporary PNG (e-mail, external program, Office, pin).
pub fn temp_export(title: &str, img: &Image) -> String {
    let base = safe_name(title, ". ");
    let base: String = if base.is_empty() { "Snapnab".into() } else { base.chars().take(40).collect() };
    let p = std::env::temp_dir().join(format!("{base}.png")).to_string_lossy().into_owned();
    let _ = save_image(&p, img, 90);
    p
}
