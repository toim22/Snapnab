//! Interface language — equivalent of the SnagItEditorRes.dll chosen by the language folder (ed_comp_1 §3, step 5): at
//! startup each program loads lang\<code>.po (gettext format) from the executable's folder; without the catalog of the
//! requested language it falls back to English and, without that, to the Portuguese in the code itself.
//!
//! The strings in the code are Brazilian Portuguese; `tr` returns the translation for the current language.
//! scripts/i18n.py extracts the strings to lang/snapnab.pot and checks the catalogs.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::sync::atomic::{AtomicPtr, Ordering};

use windows::Win32::Globalization::{
    GetUserDefaultUILanguage, GetUserPreferredUILanguages, LCIDToLocaleName, MUI_LANGUAGE_NAME,
};
use windows::core::PWSTR;

/// The 12 most spoken languages (total speakers), each with its own name; Portuguese first.
pub const LANGUAGES: [(&str, &str); 12] = [
    ("pt", "Português"), ("en", "English"), ("zh", "中文（简体）"), ("hi", "हिन्दी"),
    ("es", "Español"), ("ar", "العربية"), ("fr", "Français"), ("bn", "বাংলা"),
    ("ru", "Русский"), ("id", "Bahasa Indonesia"), ("ur", "اردو"), ("de", "Deutsch"),
];

/// LANGUAGES as nul-terminated UTF-16, for the C++ side (the pointers live as long as the process).
pub static LANGUAGES_WIDE: LazyLock<Vec<(Vec<u16>, Vec<u16>)>> = LazyLock::new(|| {
    LANGUAGES.iter().map(|(c, n)| (crate::wide::to_wide(c), crate::wide::to_wide(n))).collect()
});

pub fn known(code: &str) -> bool {
    LANGUAGES.iter().any(|(c, _)| *c == code)
}

fn catalog_path(code: &str) -> PathBuf {
    crate::paths::exe_dir().join("lang").join(format!("{code}.po"))
}

/// Portuguese always; the others if lang\<code>.po exists.
pub fn available(code: &str) -> bool {
    code == "pt" || (known(code) && catalog_path(code).is_file())
}

/// Chosen in Options; "" = the Windows language.
pub fn configured() -> &'static str {
    let c = crate::reg::get_string(OsStr::new("Language"), None).unwrap_or_default();
    LANGUAGES.iter().map(|(c, _)| *c).find(|k| OsStr::new(k) == c).unwrap_or("")
}

pub fn set_configured(code: &str) {
    crate::reg::set_string(OsStr::new("Language"), OsStr::new(code), None);
}

/// Windows display languages in order of preference ("pt-BR", "en-US"…).
fn windows_languages() -> Vec<String> {
    let mut out = Vec::new();
    let (mut n, mut cch) = (0u32, 0u32);
    unsafe {
        if GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut n, None, &mut cch).is_ok() && cch > 0 {
            let mut buf = vec![0u16; cch as usize];
            if GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut n, Some(PWSTR(buf.as_mut_ptr())), &mut cch).is_ok() {
                out = buf.split(|&c| c == 0).filter(|s| !s.is_empty()).map(String::from_utf16_lossy).collect();
            }
        }
        if out.is_empty() {
            let mut name = [0u16; 85];                                          // LOCALE_NAME_MAX_LENGTH
            let lcid = GetUserDefaultUILanguage() as u32;                       // MAKELCID(lang, SORT_DEFAULT)
            let n = LCIDToLocaleName(lcid, Some(&mut name), 0);
            if n > 1 {
                out.push(String::from_utf16_lossy(&name[..n as usize - 1]));
            }
        }
    }
    out
}

/// What `init` will use: the chosen language or the Windows one, if there is a catalog.
pub fn resolve() -> &'static str {
    let c = configured();
    if !c.is_empty() && available(c) {
        return c;
    }
    for tag in windows_languages() {
        let primary = tag.split('-').next().unwrap_or("").to_lowercase();
        if let Some(&(code, _)) = LANGUAGES.iter().find(|(c, _)| *c == primary) {
            if available(code) {
                return code;
            }
        }
    }
    if available("en") { "en" } else { "pt" }
}

// ---------------------------------------------------------------- the catalog in use

struct Catalog {
    code: &'static str,
    code_wide: Vec<u16>,
    /// Portuguese → language in use; the values end with a nul so `tr` can hand out C strings
    map: HashMap<Box<[u16]>, Box<[u16]>>,
}

/// Set once by `init`. A replaced catalog is leaked on purpose: strings handed out by `tr` stay valid.
static CATALOG: AtomicPtr<Catalog> = AtomicPtr::new(std::ptr::null_mut());

fn catalog() -> Option<&'static Catalog> {
    unsafe { CATALOG.load(Ordering::Acquire).as_ref() }
}

/// Loads the catalog of `resolve()`. Call at the start of WinMain, after the log is open.
pub fn init() {
    let code = resolve();
    let mut map = HashMap::new();
    if code == "pt" {
        crate::log::write("Language: pt (the program's built-in strings)");
    } else {
        let mut entries = Vec::new();
        if code != "en" {
            load_po_file(&catalog_path("en"), &mut entries);                    // whatever is missing stays in English
        }
        let ok = load_po_file(&catalog_path(code), &mut entries);
        let rtl = is_rtl(code);
        for (pt, text) in entries {
            let text = if rtl { rtl_lines(&text) } else { text };
            let key: Box<[u16]> = pt.encode_utf16().collect();
            map.insert(key, crate::wide::to_wide(&text).into_boxed_slice());
        }
        crate::log::write(&format!("Language: {code} ({}, {} strings)",
                                   if ok { "catalog loaded" } else { "catalog did not open" }, map.len()));
    }
    let c = Box::new(Catalog { code, code_wide: crate::wide::to_wide(code), map });
    CATALOG.store(Box::into_raw(c), Ordering::Release);
}

/// Each line of a right-to-left translation between RLE and PDF, so mixed Latin text keeps the right order; file
/// dialog filters ("Images|*.png") stay intact.
fn rtl_lines(text: &str) -> String {
    if text.contains('|') {
        return text.to_string();
    }
    text.split('\n').map(|l| if l.is_empty() { String::new() } else { format!("\u{202B}{l}\u{202C}") })
        .collect::<Vec<_>>().join("\n")
}

/// The translation of `pt` (UTF-16, without the nul) with its nul, or None to keep the Portuguese.
pub fn tr(pt: &[u16]) -> Option<&'static [u16]> {
    catalog()?.map.get(pt).map(|v| &v[..])
}

/// The translation of `pt` as text, or `pt` itself when there is none. Called through `tr!("…")`, which
/// scripts/i18n.py finds in the Rust sources as it finds `Tr(L"…")` in the C++ ones.
pub fn tr_str(pt: &str) -> String {
    let w: Vec<u16> = pt.encode_utf16().collect();
    match tr(&w) {
        Some(t) => String::from_utf16_lossy(t.strip_suffix(&[0]).unwrap_or(t)),
        None => pt.to_owned(),
    }
}

/// A translated text with its printf markers (%s, %ls, %d, %i, %u) filled in order and %% as %, as Format did.
pub fn fill(t: &str, args: &[&dyn std::fmt::Display]) -> String {
    let mut out = String::new();
    let mut next = args.iter();
    let mut it = t.chars().peekable();
    while let Some(c) = it.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match it.peek().copied() {
            Some('%') => {
                it.next();
                out.push('%');
            }
            Some('l') => {
                it.next();
                if it.peek() == Some(&'s') {
                    it.next();
                }
                if let Some(a) = next.next() {
                    out.push_str(&a.to_string());
                }
            }
            Some('s' | 'd' | 'i' | 'u') => {
                it.next();
                if let Some(a) = next.next() {
                    out.push_str(&a.to_string());
                }
            }
            _ => out.push('%'),
        }
    }
    out
}

/// `tr!("Texto")`: the translation of a Portuguese string of the program (see `tr_str`).
#[macro_export]
macro_rules! tr {
    ($pt:literal) => {
        $crate::i18n::tr_str($pt)
    };
}

/// Language in use ("pt" before `init`).
pub fn current() -> &'static str {
    catalog().map_or("pt", |c| c.code)
}

pub fn current_wide() -> &'static [u16] {
    static PT: [u16; 3] = [b'p' as u16, b't' as u16, 0];
    catalog().map_or(&PT[..], |c| &c.code_wide[..])
}

fn is_rtl(code: &str) -> bool {
    code == "ar" || code == "ur"
}

/// Arabic and Urdu: right-to-left reading.
pub fn rtl() -> bool {
    is_rtl(current())
}

/// Interface font with the language's characters (Segoe UI maps ideographs to Meiryo, Japanese glyphs, and has no
/// Devanagari or Bengali); None = the default font.
pub fn ui_font_face() -> Option<&'static str> {
    match current() {
        "zh" => Some("Microsoft YaHei UI"),
        "hi" | "bn" => Some("Nirmala UI"),
        _ => None,
    }
}

/// Alt letter of a menu: the Latin initial of the translated name. The title bar compares the virtual key (A–Z); names
/// in other alphabets keep the Portuguese letter.
pub fn alt_key(name: &[u16], fallback: u16) -> u16 {
    let first = name.first().and_then(|&c| char::from_u32(c as u32));
    let mut up = first.map(|c| c.to_uppercase()).into_iter().flatten();
    match (up.next(), up.next()) {
        (Some(c @ 'A'..='Z'), None) => c as u16,
        _ => fallback,
    }
}

// ---------------------------------------------------------------- reading the .po

/// The text between the first and the last quote, with the C escapes resolved (an unknown escape stays as it is).
fn parse_quoted(line: &[u8], out: &mut Vec<u8>) {
    let (Some(a), Some(b)) = (line.iter().position(|&c| c == b'"'), line.iter().rposition(|&c| c == b'"')) else {
        return;
    };
    let mut i = a + 1;
    while i < b {
        let ch = line[i];
        if ch != b'\\' || i + 1 >= b {
            out.push(ch);
            i += 1;
            continue;
        }
        i += 1;
        match line[i] {
            b'n' => out.push(b'\n'),
            b't' => out.push(b'\t'),
            b'r' => out.push(b'\r'),
            b'"' => out.push(b'"'),
            b'\\' => out.push(b'\\'),
            other => out.extend_from_slice(&[b'\\', other]),
        }
        i += 1;
    }
}

#[derive(PartialEq, Clone, Copy)]
enum Field {
    None,
    Ctxt,
    Id,
    Str,
    Skip,
}

/// Translated, non-"fuzzy" entries of a .po catalog as (msgid, msgstr); a msgctxt goes before the msgid, joined by
/// U+0004 as gettext does. Plural forms keep only msgstr[0].
pub fn parse_po(text: &[u8], out: &mut Vec<(String, String)>) {
    let text = text.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(text);
    let (mut ctxt, mut id, mut msg) = (Vec::new(), Vec::new(), Vec::new());
    let (mut field, mut fuzzy) = (Field::None, false);
    let mut flush = |ctxt: &mut Vec<u8>, id: &mut Vec<u8>, msg: &mut Vec<u8>, fuzzy: &mut bool, field: &mut Field| {
        if !id.is_empty() && !msg.is_empty() && !*fuzzy {
            let id = String::from_utf8_lossy(id);
            let key = if ctxt.is_empty() { id.into_owned() } else { format!("{}\u{4}{id}", String::from_utf8_lossy(ctxt)) };
            out.push((key, String::from_utf8_lossy(msg).into_owned()));
        }
        ctxt.clear();
        id.clear();
        msg.clear();
        *fuzzy = false;
        *field = Field::None;
    };
    for raw in text.split(|&c| c == b'\n') {
        let l = raw.trim_ascii_end();
        let l = l.trim_ascii_start();
        if l.is_empty() {
            flush(&mut ctxt, &mut id, &mut msg, &mut fuzzy, &mut field);
            continue;
        }
        let starts = l[0] != b'"';
        if starts && (field == Field::Str || field == Field::Skip) && !l.starts_with(b"msgstr") {
            flush(&mut ctxt, &mut id, &mut msg, &mut fuzzy, &mut field);   // next entry without a blank line
        }
        if l[0] == b'#' {
            if l.starts_with(b"#,") && l.windows(5).any(|w| w == b"fuzzy") {
                fuzzy = true;
            }
            continue;
        }
        if l.starts_with(b"msgctxt") {
            field = Field::Ctxt;
            parse_quoted(l, &mut ctxt);
        } else if l.starts_with(b"msgid_plural") {
            field = Field::Skip;
        } else if l.starts_with(b"msgid") {
            field = Field::Id;
            parse_quoted(l, &mut id);
        } else if l.starts_with(b"msgstr[0]") || (l.starts_with(b"msgstr") && !l.starts_with(b"msgstr[")) {
            field = Field::Str;
            parse_quoted(l, &mut msg);
        } else if l.starts_with(b"msgstr[") {
            field = Field::Skip;
        } else if l[0] == b'"' {
            match field {
                Field::Ctxt => parse_quoted(l, &mut ctxt),
                Field::Id => parse_quoted(l, &mut id),
                Field::Str => parse_quoted(l, &mut msg),
                _ => {}
            }
        }
    }
    flush(&mut ctxt, &mut id, &mut msg, &mut fuzzy, &mut field);
}

/// Reads a .po catalog into `out`; false if the file does not open.
pub fn load_po_file(path: &Path, out: &mut Vec<(String, String)>) -> bool {
    match std::fs::read(path) {
        Ok(text) => {
            parse_po(&text, out);
            true
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Vec<(String, String)> {
        let mut v = Vec::new();
        parse_po(s.as_bytes(), &mut v);
        v
    }

    #[test]
    fn entries_escapes_and_continuations() {
        let po = "\u{FEFF}msgid \"\"\r\nmsgstr \"Content-Type: text/plain\\n\"\r\n\r\n\
                  #: src/a.cpp:1\nmsgid \"Abrir\"\nmsgstr \"Open\"\n\n\
                  msgid \"\"\n\"Linha 1\\n\"\n\"Linha 2\"\nmsgstr \"\"\n\"Line 1\\n\"\n\"Line 2\"\n\n\
                  msgid \"Aspas \\\"x\\\" \\\\ \\t \\q\"\nmsgstr \"Quotes \\\"x\\\" \\\\ \\t \\q\"\n\n\
                  msgid \"Vazio\"\nmsgstr \"\"\n\n\
                  #, fuzzy, c-format\nmsgid \"Incerto\"\nmsgstr \"Unsure\"\n\n\
                  msgctxt \"menu\"\nmsgid \"Arquivo\"\nmsgstr \"File\"\n\
                  msgid \"Sem linha em branco\"\nmsgstr \"No blank line\"\n\
                  msgid \"%d item\"\nmsgid_plural \"%d itens\"\nmsgstr[0] \"%d item\"\nmsgstr[1] \"%d items\"\n";
        assert_eq!(parse(po), vec![
            ("Abrir".to_string(), "Open".to_string()),
            ("Linha 1\nLinha 2".to_string(), "Line 1\nLine 2".to_string()),
            ("Aspas \"x\" \\ \t \\q".to_string(), "Quotes \"x\" \\ \t \\q".to_string()),
            ("menu\u{4}Arquivo".to_string(), "File".to_string()),
            ("Sem linha em branco".to_string(), "No blank line".to_string()),
            ("%d item".to_string(), "%d item".to_string()),
        ]);
    }

    #[test]
    fn odd_lines_do_not_panic() {
        for po in ["msgid", "msgid \"", "msgid \"\\", "msgstr \"x\\\"", "\"", "#", "msgid \"a\"\nmsgstr \"b\\", "\0\u{FFFD}"] {
            parse(po);
        }
        assert_eq!(parse("msgid \"a\"\nmsgstr \"b\\\""), vec![("a".to_string(), "b\\".to_string())]);
    }

    #[test]
    fn right_to_left_lines() {
        assert_eq!(rtl_lines("a\n\nb\n"), "\u{202B}a\u{202C}\n\n\u{202B}b\u{202C}\n");
        assert_eq!(rtl_lines("Imagens|*.png"), "Imagens|*.png");
    }

    #[test]
    fn alt_keys() {
        let w = |s: &str| s.encode_utf16().collect::<Vec<_>>();
        assert_eq!(alt_key(&w("file"), b'A' as u16), b'F' as u16);
        assert_eq!(alt_key(&w("Édition"), b'E' as u16), b'E' as u16);
        assert_eq!(alt_key(&w("ملف"), b'A' as u16), b'A' as u16);
        assert_eq!(alt_key(&[], b'J' as u16), b'J' as u16);
    }

    /// Every catalog in lang\ parses, is complete and keeps the %d/%s markers of the original.
    #[test]
    fn shipped_catalogs() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../lang");
        let mut sizes = Vec::new();
        for e in std::fs::read_dir(&dir).unwrap() {
            let p = e.unwrap().path();
            if p.extension().is_some_and(|x| x == "po") {
                let mut v = Vec::new();
                assert!(load_po_file(&p, &mut v));
                sizes.push((p, v.len()));
            }
        }
        assert_eq!(sizes.len(), 11);
        assert!(sizes.iter().all(|(_, n)| *n == sizes[0].1 && *n > 1000), "{sizes:?}");
    }
}
