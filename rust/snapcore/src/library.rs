//! The capture library (DataStore, ds_arch §16 / comp_7 §5 / ed_comp_10). The original keeps an SQL CE database; here
//! the library folder itself is the index:
//!
//! - `<Library>\<GUID>.snap` and `<Library>\Video_*.mp4`: the captures
//! - `<Library>\Thumbnails\<name>.png`: their thumbnails
//! - `<Library>\Names.xml`: the names given to captures, by file name
//!
//! A port of Library.cpp. Paths are UTF-16 (`W`), joined as the C++ code joined them, and the file name functions are
//! the Windows ones it used (PathFindExtensionW, CharLowerBuffW…), so the results are the same strings. Every function
//! takes the library folder (`library_dir()` is the program's), which lets the tests use a folder of their own.
//! The thumbnail is scaled by the caller (GDI+ stays in C++ for now) and written here.

use std::collections::BTreeMap;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::Mutex;

use windows::Win32::Foundation::{FILETIME, HWND};
use windows::Win32::Storage::FileSystem::{
    DeleteFileW, FILE_ATTRIBUTE_DIRECTORY, FindClose, FindFirstFileW, FindNextFileW, GetFileAttributesExW,
    GetFileAttributesW, GetFileExInfoStandard, INVALID_FILE_ATTRIBUTES, WIN32_FILE_ATTRIBUTE_DATA, WIN32_FIND_DATAW,
};
use windows::Win32::System::Com::{CoCreateGuid, CoTaskMemFree};
use windows::Win32::System::SystemInformation::{GetLocalTime, GetSystemTimeAsFileTime};
use windows::Win32::UI::Shell::{
    FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_SILENT, FOLDERID_Music, KNOWN_FOLDER_FLAG, PathFindExtensionW,
    PathFindFileNameW, SHFILEOPSTRUCTW, SHFileOperationW, SHGetKnownFolderPath,
};
use windows::Win32::UI::WindowsAndMessaging::CharLowerBuffW;
use windows::core::PCWSTR;

use crate::codec::{self, Format, SaveOptions};
use crate::image::Image;
use crate::wide::{to_os, to_string_lossy};
use crate::xml::{self, Node};
use crate::{log, reg, snap};

/// A UTF-16 string without the terminating nul.
pub type W = Vec<u16>;

// ---------------------------------------------------------------- strings and files as Util.cpp handles them

pub fn w(s: &str) -> W {
    s.encode_utf16().collect()
}

fn cat(parts: &[&[u16]]) -> W {
    parts.concat()
}

fn nul(s: &[u16]) -> W {
    let mut v = s.to_vec();
    v.push(0);
    v
}

fn path(s: &[u16]) -> PathBuf {
    PathBuf::from(to_os(s))
}

fn text(s: &[u16]) -> String {
    to_string_lossy(s)
}

/// ToLower: CharLowerBuffW.
pub fn lower(s: &[u16]) -> W {
    let mut v = s.to_vec();
    if !v.is_empty() {
        unsafe { CharLowerBuffW(&mut v) };
    }
    v
}

/// Trim: spaces, tabs, line breaks and quotes at both ends.
pub fn trim(s: &[u16]) -> &[u16] {
    let strip = |c: &u16| matches!(*c, 0x20 | 0x09 | 0x0D | 0x0A | 0x22);
    let a = s.iter().position(|c| !strip(c));
    let Some(a) = a else { return &[] };
    let b = s.iter().rposition(|c| !strip(c)).unwrap_or(a);
    &s[a..=b]
}

/// The part of `s` from where the Windows function points to (a suffix of the nul-terminated copy).
fn suffix(s: &[u16], f: unsafe fn(PCWSTR) -> *const u16) -> W {
    let z = nul(s);
    let p = unsafe { f(PCWSTR(z.as_ptr())) };
    if p.is_null() {
        return W::new();
    }
    let off = (p as usize - z.as_ptr() as usize) / 2;
    z[off.min(s.len())..s.len()].to_vec()
}

unsafe fn find_ext(p: PCWSTR) -> *const u16 {
    unsafe { PathFindExtensionW(p) }.0
}

unsafe fn find_name(p: PCWSTR) -> *const u16 {
    unsafe { PathFindFileNameW(p) }.0
}

/// FileExt: ".png" in lowercase (PathFindExtensionW).
pub fn file_ext(p: &[u16]) -> W {
    lower(&suffix(p, find_ext))
}

/// FileNameOf: PathFindFileNameW.
pub fn file_name(p: &[u16]) -> W {
    suffix(p, find_name)
}

/// DirOf: up to the last \ or / (empty if none).
pub fn dir_of(p: &[u16]) -> W {
    match p.iter().rposition(|&c| c == b'\\' as u16 || c == b'/' as u16) {
        Some(i) => p[..i].to_vec(),
        None => W::new(),
    }
}

/// FileExists: a file, not a folder.
pub fn file_exists(p: &[u16]) -> bool {
    let a = unsafe { GetFileAttributesW(PCWSTR(nul(p).as_ptr())) };
    a != INVALID_FILE_ATTRIBUTES && a & FILE_ATTRIBUTE_DIRECTORY.0 == 0
}

fn ensure_dir(d: &[u16]) {
    if !d.is_empty() {
        let _ = std::fs::create_dir_all(path(d));
    }
}

fn delete_file(p: &[u16]) -> bool {
    unsafe { DeleteFileW(PCWSTR(nul(p).as_ptr())) }.is_ok()
}

fn ft(t: FILETIME) -> u64 {
    (t.dwHighDateTime as u64) << 32 | t.dwLowDateTime as u64
}

/// When the file was last written (FILETIME), if it exists.
fn write_time(p: &[u16]) -> Option<u64> {
    let mut d = WIN32_FILE_ATTRIBUTE_DATA::default();
    unsafe { GetFileAttributesExW(PCWSTR(nul(p).as_ptr()), GetFileExInfoStandard, &mut d as *mut _ as *mut _) }
        .ok()
        .map(|_| ft(d.ftLastWriteTime))
}

/// NewGuidString: "XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX".
pub fn new_guid() -> W {
    let g = unsafe { CoCreateGuid() }.unwrap_or_default();
    let d = g.data4;
    w(&format!(
        "{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        g.data1, g.data2, g.data3, d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]
    ))
}

/// TimeStamp: the local time as "2026-11-06_14-03-59" (or with 'T' and ':' for the documents' Created).
pub fn time_stamp(iso: bool) -> W {
    let t = unsafe { GetLocalTime() };
    let (a, b) = if iso { ('T', ':') } else { ('_', '-') };
    w(&format!("{:04}-{:02}-{:02}{a}{:02}{b}{:02}{b}{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond))
}

fn reg_dword(name: &str, def: u32) -> u32 {
    reg::get_dword(std::ffi::OsStr::new(name), def, None)
}

fn reg_string(name: &str) -> W {
    reg::get_string(std::ffi::OsStr::new(name), None).map(|s| s.encode_wide().collect()).unwrap_or_default()
}

fn app_data_dir() -> W {
    crate::paths::app_data_dir().as_os_str().encode_wide().collect()
}

// ---------------------------------------------------------------- folders

/// LibraryDir: LibraryFolder from the settings, otherwise %LOCALAPPDATA%\Snapnab\Library (created if missing).
pub fn library_dir() -> W {
    let custom = reg_string("LibraryFolder");
    let d = if custom.is_empty() {
        cat(&[&app_data_dir(), &w("\\Library")])
    } else {
        custom
    };
    ensure_dir(&d);
    d
}

/// The thumbnails' folder of the library (created if missing).
pub fn thumb_dir(dir: &[u16]) -> W {
    let d = cat(&[dir, &w("\\Thumbnails")]);
    ensure_dir(&d);
    d
}

pub fn thumb_path_for(dir: &[u16], item: &[u16]) -> W {
    cat(&[&thumb_dir(dir), &w("\\"), &file_name(item), &w(".png")])
}

/// Audio-only recordings (C32): AudioFolder from the settings, otherwise Music\Snapnab (created if missing).
pub fn audio_dir() -> W {
    let mut d = reg_string("AudioFolder");
    if d.is_empty() {
        if let Ok(p) = unsafe { SHGetKnownFolderPath(&FOLDERID_Music, KNOWN_FOLDER_FLAG(0), None) } {
            d = cat(&[unsafe { p.as_wide() }, &w("\\Snapnab")]);
            unsafe { CoTaskMemFree(Some(p.0 as *const _)) };
        }
        if d.is_empty() {
            d = cat(&[&app_data_dir(), &w("\\Audio")]);
        }
    }
    ensure_dir(&d);
    d
}

pub fn new_audio_path(mp3: bool) -> W {
    let name = w(&crate::tr!("Audio"));
    cat(&[&audio_dir(), &w("\\"), &name, &w("_"), &time_stamp(false), &w(if mp3 { ".mp3" } else { ".m4a" })])
}

pub fn new_video_path(dir: &[u16]) -> W {
    cat(&[dir, &w("\\Video_"), &time_stamp(false), &w(".mp4")])
}

/// IsInLibrary: the file is directly in the library folder.
pub fn contains(dir: &[u16], p: &[u16]) -> bool {
    lower(&dir_of(p)) == lower(dir)
}

/// NextAutoName: <folder>\<prefix><number><ext>, the first number whose file does not exist (ed_comp_2 §7).
pub fn next_auto_name(folder: &[u16], prefix: &[u16], ext: &[u16], digits: i32) -> W {
    for n in 1..1_000_000 {
        // "%0*d": zeros up to `digits`; a negative width aligns to the left with spaces
        let num = if digits < 0 {
            format!("{:<width$}", n, width = digits.unsigned_abs() as usize)
        } else {
            format!("{:0width$}", n, width = digits as usize)
        };
        let p = cat(&[folder, &w("\\"), prefix, &w(&num), ext]);
        if !file_exists(&p) {
            return p;
        }
    }
    cat(&[folder, &w("\\"), prefix, &new_guid(), ext])
}

// ---------------------------------------------------------------- the captures

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub path: W,
    pub modified: u64, // FILETIME
    pub size: u64,
    pub video: bool,
}

/// The .snap and .mp4 files of the library, the most recent first, at most `max`.
pub fn list(dir: &[u16], max: usize) -> Vec<Item> {
    let mut items = Vec::new();
    let mut fd = WIN32_FIND_DATAW::default();
    let Ok(h) = (unsafe { FindFirstFileW(PCWSTR(nul(&cat(&[dir, &w("\\*")])).as_ptr()), &mut fd) }) else {
        return items;
    };
    loop {
        if fd.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 == 0 {
            let name: W = fd.cFileName.iter().copied().take_while(|&c| c != 0).collect();
            let ext = file_ext(&name);
            if ext == w(".snap") || ext == w(".mp4") {
                items.push(Item {
                    path: cat(&[dir, &w("\\"), &name]),
                    modified: ft(fd.ftLastWriteTime),
                    size: (fd.nFileSizeHigh as u64) << 32 | fd.nFileSizeLow as u64,
                    video: ext == w(".mp4"),
                });
            }
        }
        if unsafe { FindNextFileW(h, &mut fd) }.is_err() {
            break;
        }
    }
    let _ = unsafe { FindClose(h) };
    items.sort_by(|a, b| b.modified.cmp(&a.modified));
    items.truncate(max);
    items
}

/// Writes PNG, JPEG, WebP… with the settings' options, through `<path>.tmp` (SaveImageFile).
fn save_image_file(p: &[u16], img: &Image, version: &str) -> bool {
    codec::save_file(&path(p), img, Format::from_ext(&text(&file_ext(p))), &SaveOptions::from_settings(), version)
}

/// Writes the thumbnail of an item (already scaled by the caller); false if it is empty or cannot be written.
pub fn write_thumbnail(dir: &[u16], item: &[u16], thumb: &Image, version: &str) -> bool {
    !thumb.is_empty() && save_image_file(&thumb_path_for(dir, item), thumb, version)
}

/// What the capture was (CaptureMode "Região", "Janela"…, stored in Portuguese; window title; program).
#[derive(Clone, Debug, Default)]
pub struct CaptureInfo {
    pub mode: W,
    pub window_title: W,
    pub application: W,
}

/// Adds a capture as a new .snap (AddImageToLibrary, or AddLayeredToLibrary with `objects`, the <Objects> element as
/// the Editor writes it, and the images it embeds); `thumb` is its thumbnail. Returns the path.
pub fn add(
    dir: &[u16],
    base: Image,
    thumb: &Image,
    info: &CaptureInfo,
    objects: Option<Node>,
    embedded: BTreeMap<String, Image>,
    version: &str,
) -> Option<W> {
    let p = cat(&[dir, &w("\\"), &new_guid(), &w(".snap")]);
    let (bw, bh) = (base.w, base.h);
    let mut f = snap::Frame::new(base);
    f.doc
        .set("CaptureMode", text(&info.mode))
        .set("WindowTitle", text(&info.window_title))
        .set("Application", text(&info.application))
        .set("Created", text(&time_stamp(true)));
    let n = objects.as_ref().map(|o| o.children.len());
    if let Some(o) = objects {
        f.doc.children.push(o);
    }
    f.embedded = embedded;
    if !snap::save(&path(&p), &[f], version) {
        return None;
    }
    write_thumbnail(dir, &p, thumb, version);
    match n {
        Some(n) => log::write(&format!("Library: added {} ({bw}x{bh}, {}, {n} object(s))", text(&p), text(&info.mode))),
        None => log::write(&format!("Library: added {} ({bw}x{bh}, {})", text(&p), text(&info.mode))),
    }
    Some(p)
}

/// Moves an item to the Recycle Bin and deletes its thumbnail; its name goes with it.
pub fn delete_item(dir: &[u16], item: &[u16], owner: HWND) -> bool {
    let mut from = nul(item);
    from.push(0);
    let mut op = SHFILEOPSTRUCTW {
        hwnd: owner,
        wFunc: FO_DELETE,
        pFrom: PCWSTR(from.as_ptr()),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT).0 as u16,
        ..Default::default()
    };
    // the e2e suite deletes for real (SNAPNAB_NO_RECYCLE=1): its files must not fill the user's Recycle Bin
    let ok = if std::env::var_os("SNAPNAB_NO_RECYCLE").is_some_and(|v| v == "1") {
        delete_file(&nul(item))
    } else {
        unsafe { SHFileOperationW(&mut op) == 0 }
    };
    delete_file(&thumb_path_for(dir, item));
    if ok && !name(dir, item).is_empty() {
        set_name(dir, item, &[]);
    }
    ok
}

/// Automatic cleanup (C41): captures older than `days` are removed (to the Recycle Bin or deleted), except, if asked,
/// the ones with a name or with tags. Returns how many were removed.
pub fn cleanup(dir: &[u16], days: i32, keep_named: bool, keep_tagged: bool, recycle: bool) -> i32 {
    if days <= 0 {
        return 0;
    }
    let now = ft(unsafe { GetSystemTimeAsFileTime() });
    let limit = days as u64 * 24 * 3600 * 10_000_000;
    let mut removed = 0;
    for it in list(dir, 1_000_000) {
        if now < it.modified || now - it.modified < limit {
            continue;
        }
        if keep_named && !name(dir, &it.path).is_empty() {
            continue;
        }
        if keep_tagged && !it.video {
            if let Some((doc, true)) = snap::load_document(&path(&it.path)) {
                if !trim(&w(doc.attr("Tags").unwrap_or(""))).is_empty() {
                    continue;
                }
            }
        }
        let ok = if recycle {
            delete_item(dir, &it.path, HWND::default())
        } else {
            let ok = delete_file(&it.path);
            delete_file(&thumb_path_for(dir, &it.path));
            if ok && !name(dir, &it.path).is_empty() {
                set_name(dir, &it.path, &[]);
            }
            ok
        };
        if ok {
            removed += 1;
            log::write(&format!("Cleanup: removed {}", text(&it.path)));
        }
    }
    removed
}

/// The cleanup with the settings (CleanupEnabled, CleanupDays, CleanupKeepNamed, CleanupKeepTagged, CleanupRecycle),
/// at most once a day (CleanupLast / CleanupLastHigh, in hours).
pub fn cleanup_now() -> i32 {
    if reg_dword("CleanupEnabled", 0) == 0 {
        return 0;
    }
    let now = ft(unsafe { GetSystemTimeAsFileTime() }) / 10_000_000 / 3600;
    let last = (reg_dword("CleanupLastHigh", 0) as u64) << 32 | reg_dword("CleanupLast", 0) as u64;
    if last != 0 && now.wrapping_sub(last) < 24 {
        return 0;
    }
    reg::set_dword(std::ffi::OsStr::new("CleanupLast"), (now & 0xFFFF_FFFF) as u32, None);
    reg::set_dword(std::ffi::OsStr::new("CleanupLastHigh"), (now >> 32) as u32, None);
    let n = cleanup(
        &library_dir(),
        reg_dword("CleanupDays", 30) as i32,
        reg_dword("CleanupKeepNamed", 1) != 0,
        reg_dword("CleanupKeepTagged", 1) != 0,
        reg_dword("CleanupRecycle", 1) != 0,
    );
    log::write(&format!("Cleanup: {n} capture(s) older than {} day(s) removed", reg_dword("CleanupDays", 30)));
    n
}

// ---------------------------------------------------------------- capture names (Names.xml)

#[derive(Default)]
struct NameIndex {
    file: W,
    names: BTreeMap<W, W>, // lowercase file name → name, in UTF-16 order like the std::map of wstring
    stamp: u64,
    loaded: bool,
}

static NAMES: Mutex<Option<NameIndex>> = Mutex::new(None);

fn names_file(dir: &[u16]) -> W {
    cat(&[dir, &w("\\Names.xml")])
}

/// Reads Names.xml again when another program (or the other library folder) changed it.
fn load_names(n: &mut NameIndex, dir: &[u16]) {
    let file = names_file(dir);
    let stamp = write_time(&file);
    if n.loaded && n.file == file && stamp.is_none_or(|s| s == n.stamp) {
        return;
    }
    n.loaded = true;
    n.file = file.clone();
    n.names.clear();
    n.stamp = stamp.unwrap_or(0);
    if stamp.is_none() {
        return;
    }
    let Some(root) = xml::load_file(&path(&file)) else { return };
    for c in root.children.iter().filter(|c| c.name == "Item") {
        n.names.insert(lower(&w(c.attr("File").unwrap_or(""))), w(c.attr("Name").unwrap_or("")));
    }
}

/// The name given to a capture ("" if none).
pub fn name(dir: &[u16], item: &[u16]) -> W {
    let mut g = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    let n = g.get_or_insert_with(NameIndex::default);
    load_names(n, dir);
    n.names.get(&lower(&file_name(item))).cloned().unwrap_or_default()
}

/// Names a capture; an empty name removes it. False if Names.xml cannot be written.
pub fn set_name(dir: &[u16], item: &[u16], name: &[u16]) -> bool {
    let mut g = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    let n = g.get_or_insert_with(NameIndex::default);
    load_names(n, dir);
    let (key, v) = (lower(&file_name(item)), trim(name).to_vec());
    if v.is_empty() {
        n.names.remove(&key);
    } else {
        n.names.insert(key, v);
    }
    let mut root = Node::new("Names");
    for (file, nm) in &n.names {
        root.add("Item").set("File", text(file)).set("Name", text(nm));
    }
    let ok = xml::save_file(&path(&n.file), &root);
    if let Some(s) = write_time(&n.file) {
        n.stamp = s;
    }
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> W {
        let d = std::env::temp_dir().join(format!("snapcore_library_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.as_os_str().encode_wide().collect()
    }

    #[test]
    fn file_names() {
        assert_eq!(file_ext(&w(r"C:\a.b\file.SNAP")), w(".snap"));
        assert_eq!(file_ext(&w(r"C:\a.b\file")), w(""));
        assert_eq!(file_ext(&w("a.sn ap")), w(""));
        assert_eq!(file_name(&w(r"C:\x\y/z.png")), w("z.png"));
        assert_eq!(dir_of(&w(r"C:\x\y/z.png")), w(r"C:\x\y"));
        assert_eq!(dir_of(&w("z.png")), w(""));
        assert_eq!(lower(&w("ÁBÇ.SNAP")), w("ábç.snap"));
        assert_eq!(trim(&w(" \"a b\"\r\n")), &w("a b")[..]);
        assert!(trim(&w(" \t ")).is_empty());
        assert_eq!(new_guid().len(), 36);
        assert_eq!(time_stamp(true)[10], b'T' as u16);
    }

    #[test]
    fn listing_names_and_numbers() {
        let dir = temp_dir("list");
        for (i, f) in ["a.snap", "b.MP4", "c.txt", "d e.snap"].iter().enumerate() {
            let p = path(&cat(&[&dir, &w("\\"), &w(f)]));
            std::fs::write(&p, vec![0u8; i + 1]).unwrap();
            let t = std::time::SystemTime::now() - std::time::Duration::from_secs(100 * (i as u64 + 1));
            std::fs::File::options().write(true).open(&p).unwrap().set_modified(t).unwrap();
        }
        std::fs::create_dir(path(&cat(&[&dir, &w("\\f.snap")]))).unwrap();
        let l = list(&dir, 10);
        let names: Vec<String> = l.iter().map(|i| text(&file_name(&i.path))).collect();
        assert_eq!(names, ["a.snap", "b.MP4", "d e.snap"]);
        assert_eq!((l[1].video, l[1].size), (true, 2));
        assert_eq!(list(&dir, 2).len(), 2);
        assert!(contains(&dir, &cat(&[&lower(&dir), &w("\\X.snap")])));
        assert!(!contains(&dir, &cat(&[&dir, &w("\\sub\\X.snap")])));

        let item = cat(&[&dir, &w("\\a.snap")]);
        assert!(name(&dir, &item).is_empty());
        assert!(set_name(&dir, &item, &w("  Relatório \"final\" ")));
        assert_eq!(name(&dir, &cat(&[&dir, &w("\\A.SNAP")])), w("Relatório \"final"));
        assert!(set_name(&dir, &item, &w(" ")));
        assert!(name(&dir, &item).is_empty());

        std::fs::write(path(&cat(&[&dir, &w("\\Snap001.png")])), b"x").unwrap();
        std::fs::create_dir(path(&cat(&[&dir, &w("\\Snap002.png")]))).unwrap();
        assert_eq!(next_auto_name(&dir, &w("Snap"), &w(".png"), 3), cat(&[&dir, &w("\\Snap002.png")]));
        assert_eq!(next_auto_name(&dir, &w("Snap"), &w(".png"), 1), cat(&[&dir, &w("\\Snap1.png")]));
        let _ = std::fs::remove_dir_all(path(&dir));
    }

    #[test]
    fn adding_and_cleaning_up() {
        let dir = temp_dir("add");
        let img = Image::new(40, 30, 0xFF336699);
        let info = CaptureInfo { mode: w("Região"), window_title: w("Título"), application: w("app.exe") };
        let p = add(&dir, img.clone(), &Image::new(16, 12, 0xFF000000), &info, None, BTreeMap::new(), "1.2.3").unwrap();
        assert_eq!(file_ext(&p), w(".snap"));
        let frames = snap::load(&path(&p)).unwrap();
        assert_eq!(frames[0].doc.attr("CaptureMode"), Some("Região"));
        assert_eq!(frames[0].doc.attr("Created").map(str::len), Some(19));
        assert!(file_exists(&thumb_path_for(&dir, &p)));

        let mut objects = Node::new("Objects");
        objects.add("Image").set("StreamName", "Image_1.png");
        let mut emb = BTreeMap::new();
        emb.insert("Image_1.png".to_string(), Image::new(4, 4, 0xFFFF0000));
        let q = add(&dir, img, &Image::default(), &info, Some(objects), emb, "1.2.3").unwrap();
        let frames = snap::load(&path(&q)).unwrap();
        assert!(frames[0].doc.child("Objects").is_some() && frames[0].embedded.contains_key("Image_1.png"));
        assert!(!file_exists(&thumb_path_for(&dir, &q)));       // an empty thumbnail is not written

        // both are old: the named one stays, the other goes (deleted, not recycled)
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(10 * 24 * 3600);
        for f in [&p, &q] {
            std::fs::File::options().write(true).open(path(f)).unwrap().set_modified(old).unwrap();
        }
        set_name(&dir, &q, &w("keep"));
        assert_eq!(cleanup(&dir, 30, true, true, false), 0);
        assert_eq!(cleanup(&dir, 5, true, true, false), 1);
        assert!(!file_exists(&p) && !file_exists(&thumb_path_for(&dir, &p)) && file_exists(&q));
        assert_eq!(cleanup(&dir, 5, false, true, false), 1);
        assert!(!file_exists(&q) && name(&dir, &q).is_empty());
        let _ = std::fs::remove_dir_all(path(&dir));
    }
}
