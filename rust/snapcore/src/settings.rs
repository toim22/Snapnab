//! Backup and transfer of the settings (Settings.cpp, F51): one .snapsettings file with the registry values
//! (HKCU\Software\Snapnab\1 and its subkeys) and the settings files of the data folder (presets, tool themes and quick
//! styles, favorites, tool styles, quick effects, stamp order, Beautify presets, uploaders, AI settings). Import
//! replaces them or merges the file into the current ones.

use std::ffi::c_void;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_BINARY, REG_DWORD, REG_EXPAND_SZ, REG_OPTION_NON_VOLATILE, REG_SZ, REG_VALUE_TYPE,
    RRF_RT_ANY, RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegEnumKeyExW, RegEnumValueW, RegGetValueW, RegOpenKeyExW, RegSetKeyValueW,
};
use windows::core::{PCWSTR, PWSTR};

use crate::reg::ROOT;
use crate::wide::to_wide;
use crate::xml::{self, Node};
use crate::{log, tr};

/// The settings files of the data folder (the library, logs and the tray list are not settings).
pub const FILES: [&str; 9] = ["Presets1.xml", "ToolThemes.xml", "Favorites.xml", "ToolStyles.xml", "QuickEffects.xml", "StampOrder.xml",
                              "BeautifyPresets.xml", "Uploaders.xml", "AiSettings.xml"];

fn full(sub: &str) -> Vec<u16> {
    to_wide(if sub.is_empty() { ROOT.to_owned() } else { format!("{ROOT}\\{sub}") })
}

fn base64_decode(s: &str) -> Vec<u8> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        } as u32)
    };
    let (mut out, mut acc, mut bits) = (Vec::with_capacity(s.len() * 3 / 4), 0u32, 0);
    for c in s.bytes() {
        if c == b'=' {
            break;
        }
        let Some(v) = val(c) else { continue };
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

/// Every value of the key and its subkeys (relative path "" = the root).
fn export_key(sub: &str, out: &mut Node) {
    let path = full(sub);
    let mut k = HKEY::default();
    if unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(path.as_ptr()), None, KEY_READ, &mut k) } != ERROR_SUCCESS {
        return;
    }
    let kn = out.add("Key");
    kn.set("Path", sub);
    let mut i = 0;
    loop {
        let mut name = vec![0u16; 16384];
        let (mut nl, mut ty, mut cb) = (16384u32, 0u32, 0u32);
        let r = unsafe { RegEnumValueW(k, i, Some(PWSTR(name.as_mut_ptr())), &mut nl, None, Some(&mut ty), None, Some(&mut cb)) };
        if r == ERROR_NO_MORE_ITEMS {
            break;
        }
        i += 1;
        if r != ERROR_SUCCESS {
            continue;
        }
        let mut data = vec![0u8; cb as usize + 2];
        nl = 16384;
        if unsafe { RegEnumValueW(k, i - 1, Some(PWSTR(name.as_mut_ptr())), &mut nl, None, Some(&mut ty), Some(data.as_mut_ptr()), Some(&mut cb)) }
            != ERROR_SUCCESS
        {
            continue;
        }
        data.truncate(cb as usize);
        let v = kn.add("Value");
        v.set("Name", String::from_utf16_lossy(&name[..nl as usize]));
        let ty = REG_VALUE_TYPE(ty);
        if ty == REG_DWORD && cb == 4 {
            v.set("Type", "dword").set("Data", u32::from_le_bytes([data[0], data[1], data[2], data[3]]).to_string());
        } else if (ty == REG_SZ || ty == REG_EXPAND_SZ) && cb >= 2 {
            let w: Vec<u16> = data.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&c| c != 0).collect();
            v.set("Type", "string").set("Data", String::from_utf16_lossy(&w));
        } else {
            v.set("RawType", ty.0.to_string()).set("Type", "binary").set("Data", crate::net::base64(&data));
        }
    }
    let mut subs = Vec::new();
    for i in 0.. {
        let mut name = vec![0u16; 256];
        let mut nl = 256u32;
        if unsafe { RegEnumKeyExW(k, i, Some(PWSTR(name.as_mut_ptr())), &mut nl, None, None, None, None) } != ERROR_SUCCESS {
            break;
        }
        subs.push(String::from_utf16_lossy(&name[..nl as usize]));
    }
    unsafe {
        let _ = RegCloseKey(k);
    }
    for s in subs {
        export_key(&if sub.is_empty() { s } else { format!("{sub}\\{s}") }, out);
    }
}

fn value_exists(sub: &str, name: &str) -> bool {
    let (k, n) = (full(sub), to_wide(name));
    unsafe { RegGetValueW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()), PCWSTR(n.as_ptr()), RRF_RT_ANY, None, None, None) == ERROR_SUCCESS }
}

fn import_value(sub: &str, v: &Node, merge: bool) {
    let name = v.attr("Name").unwrap_or("");
    if merge && value_exists(sub, name) {
        return; // merge: the current setting stays
    }
    let (k, n) = (full(sub), to_wide(name));
    let set = |ty: REG_VALUE_TYPE, data: &[u8]| unsafe {
        let _ = RegSetKeyValueW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()), PCWSTR(n.as_ptr()), ty.0, Some(data.as_ptr() as *const c_void),
                                data.len() as u32);
    };
    let data = v.attr("Data").unwrap_or("");
    match v.attr("Type").unwrap_or("") {
        "dword" => set(REG_DWORD, &crate::crt::strtoul(data, 10).to_le_bytes()),
        "string" => {
            let w = to_wide(data);
            set(REG_SZ, &w.iter().flat_map(|c| c.to_le_bytes()).collect::<Vec<u8>>());
        }
        "binary" => set(REG_VALUE_TYPE(v.attr("RawType").map_or(REG_BINARY.0, |t| crate::crt::atoi(t) as u32)), &base64_decode(data)),
        _ => {}
    }
}

/// The identity of an item of a settings list (preset, theme, quick effect, stamp category, pinned stamp) for the merge.
fn item_key(n: &Node) -> String {
    for k in ["DisplayName", "Name", "Key", "Id"] {
        if let Some(v) = n.attr(k) {
            return format!("{}|{k}={v}", n.name);
        }
    }
    String::new()
}

/// Imported items the current node does not have are added: named items by name; a list without attributes (the
/// <Presets> of Presets1.xml, the pinned stamps) is merged inside the current one; other unnamed items (a favorite)
/// when no identical one exists.
fn merge_node(cur: &mut Node, imp: &Node) {
    for c in &imp.children {
        let key = item_key(c);
        if !key.is_empty() {
            if !cur.children.iter().any(|x| item_key(x) == key) {
                cur.children.push(c.clone());
            }
            continue;
        }
        if c.attrs.is_empty() && !c.children.is_empty()
            && let Some(x) = cur.children.iter_mut().find(|x| x.name == c.name && x.attrs.is_empty())
        {
            merge_node(x, c);
            continue;
        }
        let s = xml::serialize(c, false);
        if !cur.children.iter().any(|x| xml::serialize(x, false) == s) {
            cur.children.push(c.clone());
        }
    }
}

/// The merge of one settings file (XML): the current items stay and the imported ones that are missing are added.
pub fn merge_xml(current: &[u8], imported: &[u8]) -> Vec<u8> {
    let Some(imp) = xml::parse(imported) else { return current.to_vec() };
    match xml::parse(current) {
        Some(mut cur) if cur.name == imp.name => {
            merge_node(&mut cur, &imp);
            xml::serialize(&cur, true).into_bytes()
        }
        _ => imported.to_vec(),
    }
}

fn data_dir() -> PathBuf {
    crate::paths::app_data_dir()
}

pub fn export(path: &Path) -> Result<(), String> {
    let mut root = Node::new("SnapnabSettings");
    let stamp = String::from_utf16_lossy(&crate::library::time_stamp(true));
    root.set("Version", "1").set("Created", stamp).set("Program", crate::log::app_version());
    export_key("", root.add("Registry"));
    let files = root.add("Files");
    for f in FILES {
        let data = std::fs::read(data_dir().join(f)).unwrap_or_default();
        if data.is_empty() || data.len() >= 64 << 20 {
            continue;
        }
        files.add("File").set("Name", f).set("Data", crate::net::base64(&data));
    }
    if !xml::save_file(path, &root) {
        return Err(tr!("Não foi possível gravar o arquivo."));
    }
    log::write(&format!("Settings exported: {}", path.display()));
    Ok(())
}

/// A settings file that can be imported.
pub fn validate(path: &Path) -> bool {
    xml::load_file(path).is_some_and(|r| r.name == "SnapnabSettings" && r.child("Registry").is_some())
}

/// Replace: the current settings give way to the file's; merge: the current ones stay and the file adds what is
/// missing (registry values and list items — presets, themes, favorites, quick effects — by name).
pub fn import(path: &Path, merge: bool) -> Result<(), String> {
    let root = xml::load_file(path).filter(|r| r.name == "SnapnabSettings").ok_or_else(|| tr!("Arquivo de configurações inválido."))?;
    if !merge {
        let k = to_wide(ROOT);
        unsafe {
            let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()));
        }
        for f in FILES {
            let _ = std::fs::remove_file(data_dir().join(f));
        }
    }
    if let Some(reg) = root.child("Registry") {
        for k in reg.children.iter().filter(|k| k.name == "Key") {
            let sub = k.attr("Path").unwrap_or("");
            if sub.contains("..") {
                continue;
            }
            let p = full(sub);
            let mut hk = HKEY::default();
            if unsafe {
                RegCreateKeyExW(HKEY_CURRENT_USER, PCWSTR(p.as_ptr()), None, PCWSTR::null(), REG_OPTION_NON_VOLATILE, KEY_WRITE, None, &mut hk,
                                None)
            } == ERROR_SUCCESS
            {
                unsafe {
                    let _ = RegCloseKey(hk);
                }
            }
            for v in k.children.iter().filter(|v| v.name == "Value") {
                import_value(sub, v, merge);
            }
        }
    }
    if let Some(files) = root.child("Files") {
        for f in &files.children {
            let name = f.attr("Name").unwrap_or("");
            let Some(known) = FILES.iter().find(|k| k.eq_ignore_ascii_case(name)) else { continue }; // only known files
            let mut data = base64_decode(f.attr("Data").unwrap_or(""));
            let dst = data_dir().join(known);
            if merge && dst.exists() {
                if known.eq_ignore_ascii_case("ToolStyles.xml") {
                    continue; // one style per tool: the current ones stay
                }
                data = merge_xml(&std::fs::read(&dst).unwrap_or_default(), &data);
            }
            if std::fs::write(&dst, &data).is_err() {
                return Err(tr!("Não foi possível gravar ") + known);
            }
        }
    }
    log::write(&format!("Settings imported ({}): {}", if merge { "merge" } else { "replace" }, path.display()));
    Ok(())
}

/// %LOCALAPPDATA%\Snapnab\Backups (created if missing)
pub fn backup_dir() -> PathBuf {
    let d = data_dir().join("Backups");
    let _ = std::fs::create_dir_all(&d);
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trip() {
        for d in [&b""[..], b"a", b"ab", b"abc", b"\x00\xff\x10 hello"] {
            assert_eq!(base64_decode(&crate::net::base64(d)), d);
        }
    }

    #[test]
    fn merge_adds_what_is_missing() {
        let cur = br#"<SavedPresets Version="1"><Presets><CaptureSettings DisplayName="a" X="1"/></Presets></SavedPresets>"#;
        let imp = br#"<SavedPresets Version="1"><Presets><CaptureSettings DisplayName="a" X="2"/><CaptureSettings DisplayName="b"/></Presets></SavedPresets>"#;
        let out = String::from_utf8(merge_xml(cur, imp)).unwrap();
        assert!(out.contains(r#"DisplayName="a" X="1""#) && !out.contains(r#"X="2""#), "{out}");
        assert!(out.contains(r#"DisplayName="b""#), "{out}");
        let other = br#"<Other/>"#;
        assert_eq!(merge_xml(other, imp), imp.to_vec(), "a different root: the imported file");
    }
}
