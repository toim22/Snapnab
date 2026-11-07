//! The stamp gallery (src/editor/Stamps.cpp; StampGalleryManager, StampSearcher, OrganizeStamps, ed_comp_8 §4):
//! built-in categories of color stamps (emoji), label badges, cursors and "My stamps" (images in
//! %LOCALAPPDATA%\Snapnab\Stamps; each subfolder is one more category). The order the user drags and the pinned
//! stamps are kept in StampOrder.xml.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use snapcore::xml::{self, Node};

use crate::stamp_data::{LABELS, STOCK};
use crate::tr;

#[derive(Clone, Debug, PartialEq)]
pub struct StampItem {
    /// emoji, "file:path", "label:RRGGBB:Text" or "cursor:…"
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Default)]
pub struct StampCategory {
    pub name: String,
    pub items: Vec<StampItem>,
    pub mine: bool,
    /// identifies the category in any language (order and pins are saved by it)
    pub key: String,
    /// the "Pinned" category
    pub pinned: bool,
}

pub fn my_stamps_dir() -> String {
    let d = snapcore::paths::app_data_dir().join("Stamps");
    let _ = std::fs::create_dir_all(&d);
    d.to_string_lossy().into_owned()
}

fn label_stamps() -> Vec<StampItem> {
    LABELS
        .iter()
        .map(|(c, t)| {
            let t = tr(t);
            StampItem { id: format!("label:{c:06X}:{t}"), name: t }
        })
        .collect()
}

// ---------------------------------------------------------------- order and pins (StampOrder.xml)
#[derive(Default)]
struct Prefs {
    order: BTreeMap<String, Vec<String>>,
    pinned: Vec<String>,
}

static PREFS: Mutex<Option<Prefs>> = Mutex::new(None);

fn order_file() -> PathBuf {
    snapcore::paths::app_data_dir().join("StampOrder.xml")
}

fn with_prefs<R>(f: impl FnOnce(&mut Prefs) -> R) -> R {
    let mut g = PREFS.lock().unwrap();
    let p = g.get_or_insert_with(|| {
        let mut p = Prefs::default();
        if let Some(root) = xml::load_file(&order_file()) {
            for c in &root.children {
                let ids = c.children.iter().map(|it| it.attr("Id").unwrap_or("").to_owned());
                if c.name == "Category" {
                    p.order.entry(c.attr("Key").unwrap_or("").to_owned()).or_default().extend(ids);
                } else if c.name == "Pinned" {
                    p.pinned.extend(ids);
                }
            }
        }
        p
    });
    f(p)
}

fn save_prefs(p: &Prefs) {
    let mut root = Node::new("StampOrder");
    for (key, list) in &p.order {
        let c = root.add("Category");
        c.set("Key", key.clone());
        for id in list {
            c.add("Item").set("Id", id.clone());
        }
    }
    let pn = root.add("Pinned");
    for id in &p.pinned {
        pn.add("Item").set("Id", id.clone());
    }
    xml::save_file(&order_file(), &root);
}

pub fn is_pinned(id: &str) -> bool {
    with_prefs(|p| p.pinned.iter().any(|x| x == id))
}

pub fn set_pinned(id: &str, pin: bool) {
    with_prefs(|p| {
        p.pinned.retain(|x| x != id);
        if pin {
            p.pinned.push(id.to_owned());
        }
        save_prefs(p);
    });
}

/// The saved order first (items that no longer exist are skipped), the new ones after; then the pinned ones go first.
fn apply_order(c: &mut StampCategory) {
    with_prefs(|p| {
        if let Some(order) = p.order.get(&c.key) {
            let mut sorted: Vec<StampItem> = order.iter().filter_map(|id| c.items.iter().find(|i| &i.id == id).cloned()).collect();
            sorted.extend(c.items.iter().filter(|i| !order.contains(&i.id)).cloned());
            c.items = sorted;
        }
        let (mut a, b): (Vec<_>, Vec<_>) = c.items.drain(..).partition(|s| p.pinned.contains(&s.id));
        a.extend(b);
        c.items = a;
    });
}

/// A drag in the gallery: saves the category's new order.
pub fn move_stamp(cat: &StampCategory, from: usize, to: usize) {
    if cat.key.is_empty() || from >= cat.items.len() || to >= cat.items.len() || from == to {
        return;
    }
    let mut ids: Vec<String> = cat.items.iter().map(|i| i.id.clone()).collect();
    let moved = ids.remove(from);
    ids.insert(to, moved);
    with_prefs(|p| {
        if cat.pinned {
            p.pinned = ids;
        } else {
            p.order.insert(cat.key.clone(), ids);
        }
        save_prefs(p);
    });
}

fn parse_list(list: &str) -> Vec<StampItem> {
    list.split('|')
        .filter_map(|item| item.split_once(' ').map(|(id, name)| StampItem { id: id.to_owned(), name: tr(name) }))
        .collect()
}

fn list_images(dir: &str, out: &mut Vec<StampItem>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            continue;
        }
        let ext = crate::doc::ext_of(&p.to_string_lossy());
        if ![".png", ".jpg", ".jpeg", ".gif", ".bmp", ".tif", ".tiff"].contains(&ext.as_str()) {
            continue;
        }
        let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        out.push(StampItem { id: format!("file:{}", p.to_string_lossy()), name });
    }
    out.sort_by_key(|a| a.name.to_lowercase());
}

/// "Pinned" (when there are pinned stamps) + "My stamps" (and subfolders) + the built-in ones, each in the order the
/// user chose, with the pinned stamps first.
pub fn categories() -> Vec<StampCategory> {
    let dir = my_stamps_dir();
    let mut cats = Vec::new();
    let mut mine = StampCategory { name: tr("Meus carimbos"), mine: true, key: "mine".into(), ..Default::default() };
    list_images(&dir, &mut mine.items);
    cats.push(mine);
    if let Ok(rd) = std::fs::read_dir(&dir) {
        let mut subs: Vec<_> = rd.flatten().filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        subs.sort();
        for s in subs.into_iter().filter(|s| !s.starts_with('.')) {
            let mut c = StampCategory { name: format!("{}{s}", tr("Meus carimbos › ")), mine: true, key: format!("mine:{s}"), ..Default::default() };
            list_images(&format!("{dir}\\{s}"), &mut c.items);
            cats.push(c);
        }
    }
    cats.push(StampCategory { name: tr("Rótulos"), items: label_stamps(), key: "labels".into(), ..Default::default() });
    // mouse cursors and click indicators (C25)
    let cursors = crate::menu::cursor_stamps().into_iter().map(|(id, name)| StampItem { id: id.to_owned(), name }).collect();
    cats.push(StampCategory { name: tr("Cursores"), items: cursors, key: "cursors".into(), ..Default::default() });
    for (cat, list) in STOCK {
        cats.push(StampCategory { name: tr(cat), items: parse_list(list), key: cat.to_owned(), ..Default::default() });
    }
    let pinned = with_prefs(|p| p.pinned.clone());
    let pins: Vec<StampItem> = pinned.iter().filter_map(|id| cats.iter().find_map(|c| c.items.iter().find(|s| &s.id == id).cloned())).collect();
    for c in &mut cats {
        apply_order(c);
    }
    if !pins.is_empty() {
        cats.insert(0, StampCategory { name: tr("Fixados"), items: pins, key: "pinned".into(), pinned: true, ..Default::default() });
    }
    cats
}

pub fn search(cats: &[StampCategory], text: &str) -> Vec<StampItem> {
    let q = text.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let q = q.as_str();
    cats.iter()
        .filter(|c| !c.pinned)
        .flat_map(|c| c.items.iter().filter(move |it| it.name.to_lowercase().contains(&q) || c.name.to_lowercase().contains(&q)))
        .cloned()
        .collect()
}

/// Copies the chosen images to My stamps; returns how many.
pub fn import(owner: isize) -> usize {
    let filter = format!("{}|*.png;*.jpg;*.jpeg;*.gif;*.bmp;*.tif;*.tiff|", tr("Imagens"));
    let Some(files) = crate::native::open_files(owner, &tr("Importar carimbos"), &filter) else { return 0 };
    let dir = my_stamps_dir();
    files
        .iter()
        .filter(|f| {
            let name = std::path::Path::new(f).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            std::fs::copy(f, format!("{dir}\\{name}")).is_ok()
        })
        .count()
}

/// "Add to my stamps".
pub fn add_image(img: &snapcore::image::Image) -> Option<String> {
    if img.is_empty() {
        return None;
    }
    let ts = String::from_utf16_lossy(&snapcore::library::time_stamp(false)).replace(':', "-");
    let saved = format!("{}\\{} {ts}.png", my_stamps_dir(), tr("Carimbo"));
    crate::files::save_image(&saved, img, 90).ok().map(|_| saved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalog_has_every_category() {
        assert_eq!(STOCK.len(), 9);
        assert!(parse_list(STOCK[0].1).iter().any(|s| s.id == "📎" && s.name == "clipe anexo"));
        assert_eq!(label_stamps().len(), 20);
        assert!(label_stamps()[0].id.starts_with("label:5A6B7D:"));
    }
}
