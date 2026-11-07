//! The recent captures tray (at the bottom of the Editor) and the whole library view (in place of the image): the
//! LibraryTray of src/editor/Panels.cpp. Tray.bin in the library keeps the tray's order and the items the user
//! removed from it (one path per line, UTF-16; "-path" = removed).

use std::collections::HashMap;

use egui::{Color32, RichText, Sense, TextureHandle, vec2};
use snapcore::library::{self, Item};

use crate::tr;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Tray,
    Library,
}

/// What the user did on a tray or library item.
#[derive(Clone, Debug)]
pub enum TrayAction {
    Open(String),
    ShowInFolder(String),
    Delete(Vec<String>),
    Export(Vec<String>),
    Rename(String),
    CopyAs(String),
    Pin(String),
    Upload(String),
    Combine(Vec<String>),
    Compare(Vec<String>),
    Batch(Vec<String>),
    ShowLibrary(bool),
    /// dragging items to another program (TrayDragDrop): the paths and the item whose thumbnail follows the cursor
    DragOut(Vec<String>, String),
}

pub struct LibraryTray {
    pub mode: Mode,
    pub items: Vec<Item>,
    /// selected paths
    pub selection: Vec<String>,
    pub current: String,
    recent: Vec<String>,
    hidden: Vec<String>,
    known: Vec<String>,
    saved: String,
    pub query: String,
    thumbs: HashMap<String, (u64, Option<TextureHandle>)>,
}

fn p(it: &Item) -> String {
    String::from_utf16_lossy(&it.path)
}

fn eq(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

fn recent_max() -> usize {
    snapcore::reg::get_dword(std::ffi::OsStr::new("RecentTrayDocs"), 10, Some(std::ffi::OsStr::new("Editor"))).clamp(1, 100) as usize
}

fn tray_file() -> String {
    format!("{}\\Tray.bin", crate::files::library_dir())
}

/// "dd/MM HH:mm" of a FILETIME, in local time.
pub fn item_date(ft: u64) -> String {
    use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
    let f = FILETIME { dwLowDateTime: ft as u32, dwHighDateTime: (ft >> 32) as u32 };
    let (mut st, mut lt) = (SYSTEMTIME::default(), SYSTEMTIME::default());
    unsafe {
        let _ = FileTimeToSystemTime(&f, &mut st);
        let _ = SystemTimeToTzSpecificLocalTime(None, &st, &mut lt);
    }
    format!("{:02}/{:02} {:02}:{:02}", lt.wDay, lt.wMonth, lt.wHour, lt.wMinute)
}

/// "YYYY-MM-DD_HH-MM-SS" of a FILETIME, in local time (the names of the dragged captures).
pub fn file_stamp(ft: u64) -> String {
    use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
    let f = FILETIME { dwLowDateTime: ft as u32, dwHighDateTime: (ft >> 32) as u32 };
    let (mut st, mut lt) = (SYSTEMTIME::default(), SYSTEMTIME::default());
    unsafe {
        let _ = FileTimeToSystemTime(&f, &mut st);
        let _ = SystemTimeToTzSpecificLocalTime(None, &st, &mut lt);
    }
    format!("{:04}-{:02}-{:02}_{:02}-{:02}-{:02}", lt.wYear, lt.wMonth, lt.wDay, lt.wHour, lt.wMinute, lt.wSecond)
}

impl LibraryTray {
    pub fn new(mode: Mode) -> LibraryTray {
        let mut t = LibraryTray {
            mode,
            items: Vec::new(),
            selection: Vec::new(),
            current: String::new(),
            recent: Vec::new(),
            hidden: Vec::new(),
            known: Vec::new(),
            saved: String::new(),
            query: String::new(),
            thumbs: HashMap::new(),
        };
        if mode == Mode::Tray {
            t.load_recent();
        }
        t
    }

    pub fn label(it: &Item) -> String {
        let name = crate::files::capture_name(&p(it));
        if !name.is_empty() { name } else { item_date(it.modified) }
    }

    fn load_recent(&mut self) {
        self.recent.clear();
        self.hidden.clear();
        let Ok(b) = std::fs::read(tray_file()) else { return };
        let w: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        for line in String::from_utf16_lossy(&w).split('\n') {
            let line = line.trim().trim_start_matches('\u{FEFF}');
            if let Some(h) = line.strip_prefix('-').filter(|h| !h.is_empty()) {
                self.hidden.push(h.to_owned());
            } else if !line.is_empty() {
                self.recent.push(line.to_owned());
            }
        }
    }

    fn save_recent(&mut self) {
        if self.mode != Mode::Tray {
            return;
        }
        let mut s = String::new();
        for r in &self.recent {
            s += &format!("{r}\r\n");
        }
        for h in &self.hidden {
            s += &format!("-{h}\r\n");
        }
        // written only when it changes (the library watcher reloads the tray on every write)
        if s == self.saved {
            return;
        }
        self.saved = s.clone();
        let bytes: Vec<u8> = s.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
        let _ = std::fs::write(tray_file(), bytes);
    }

    pub fn reload(&mut self) {
        let dir = library::library_dir();
        let all = library::list(&dir, 100_000);
        let exists = |x: &String| std::path::Path::new(x).exists();
        if self.mode == Mode::Tray {
            let max = recent_max();
            if self.known.is_empty() {
                self.known = all.iter().map(|i| p(i).to_lowercase()).collect();
                if self.recent.is_empty() {
                    self.recent = all.iter().take(max).map(p).collect();
                }
            } else {
                let fresh: Vec<String> = all.iter().map(p).filter(|x| !self.known.contains(&x.to_lowercase())).collect();
                for f in fresh.iter().rev() {
                    self.recent.retain(|r| !eq(r, f));
                    self.recent.insert(0, f.clone());
                }
                self.known = all.iter().map(|i| p(i).to_lowercase()).collect();
            }
            self.recent.retain(exists);
            self.hidden.retain(exists);
            self.recent.truncate(max);
            if self.recent.len() < max {
                let start = match self.recent.last() {
                    Some(last) => all.iter().position(|i| eq(&p(i), last)).map_or(all.len(), |k| k + 1),
                    None => 0,
                };
                for it in all.iter().skip(start) {
                    if self.recent.len() >= max {
                        break;
                    }
                    let x = p(it);
                    if !self.recent.iter().any(|r| eq(r, &x)) && !self.hidden.iter().any(|h| eq(h, &x)) {
                        self.recent.push(x);
                    }
                }
            }
            self.save_recent();
            self.items = self.recent.iter().filter_map(|r| all.iter().find(|i| eq(&p(i), r)).cloned()).collect();
        } else {
            self.items = all;
            let q = self.query.trim().to_lowercase();
            if !q.is_empty() {
                self.items.retain(|it| {
                    Self::label(it).to_lowercase().contains(&q) || crate::doc::file_name_of(&p(it)).to_lowercase().contains(&q)
                });
            }
        }
        let paths: Vec<String> = self.items.iter().map(p).collect();
        self.selection.retain(|s| paths.iter().any(|x| eq(x, s)));
    }

    /// A capture opened or created: it goes to the front of the tray.
    pub fn add_recent(&mut self, path: &str) {
        if self.mode != Mode::Tray || path.is_empty() {
            return;
        }
        let dir = crate::files::library_dir();
        if !crate::doc::file_name_of(path).is_empty() && !std::path::Path::new(path).parent().is_some_and(|d| eq(&d.to_string_lossy(), &dir)) {
            return;
        }
        self.recent.retain(|r| !eq(r, path));
        self.recent.insert(0, path.to_owned());
        self.recent.truncate(recent_max());
        self.hidden.retain(|h| !eq(h, path));
        self.save_recent();
    }

    pub fn remove_from_tray(&mut self, paths: &[String]) {
        for x in paths {
            self.recent.retain(|r| !eq(r, x));
            self.hidden.push(x.clone());
        }
        self.save_recent();
        self.reload();
    }

    pub fn set_current(&mut self, path: &str) {
        self.current = path.to_owned();
        // a capture that is not in the tray yet comes in at the front; one already there keeps its place, so going
        // back to an older capture to copy or edit it does not make it look like the latest (issue #2)
        if self.mode == Mode::Tray && !self.recent.iter().any(|r| eq(r, path)) {
            self.add_recent(path);
        }
        self.reload();
        // opening an item leaves only it selected
        self.selection = self.items.iter().map(p).filter(|x| eq(x, path)).collect();
    }

    pub fn invalidate_thumb(&mut self, path: &str) {
        self.thumbs.remove(&path.to_lowercase());
    }

    pub fn select_all(&mut self) {
        self.selection = self.items.iter().map(p).collect();
    }

    /// The selected paths in the list's order (the newest first); the current item when nothing is selected.
    pub fn selected_paths(&self) -> Vec<String> {
        self.items.iter().map(p).filter(|x| self.selection.iter().any(|s| eq(s, x))).collect()
    }

    fn thumb(&mut self, ctx: &egui::Context, it: &Item) -> Option<TextureHandle> {
        let key = p(it).to_lowercase();
        if let Some((m, t)) = self.thumbs.get(&key)
            && *m == it.modified
        {
            return t.clone();
        }
        let t = crate::files::load_thumbnail(&p(it)).map(|img| crate::canvas_ui::texture(ctx, &format!("thumb-{key}"), &img, true));
        self.thumbs.insert(key, (it.modified, t.clone()));
        t
    }

    /// Draws the tray (one row) or the library (a grid); returns what the user did.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> Vec<TrayAction> {
        let mut acts = Vec::new();
        let ctx = ui.ctx().clone();
        let tray = self.mode == Mode::Tray;
        let name = if tray { "SnapnabTray" } else { "SnapnabLibrary" };
        let bg = ui.allocate_rect(ui.max_rect(), Sense::hover());
        crate::panels::tag(ui, &bg, name);
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(ui.max_rect().shrink(4.0)));
        ui.horizontal(|ui| {
            if tray {
                ui.label(RichText::new(tr("Capturas recentes")).strong());
                let r = ui.small_button(tr("Biblioteca…"));
                crate::panels::tag(ui, &r, "SnapnabTray.Library");
                if r.clicked() {
                    acts.push(TrayAction::ShowLibrary(true));
                }
            } else {
                ui.label(RichText::new(tr("Biblioteca")).strong().size(15.0));
                ui.label(format!("({})", self.items.len()));
                ui.add_space(12.0);
                let r = ui.add(egui::TextEdit::singleline(&mut self.query).hint_text(tr("Pesquisar")).desired_width(200.0));
                crate::panels::tag(ui, &r, "SnapnabLibrary.Search");
                if r.changed() {
                    self.reload();
                }
                let r = ui.button(tr("Voltar à imagem"));
                crate::panels::tag(ui, &r, "SnapnabLibrary.Close");
                if r.clicked() {
                    acts.push(TrayAction::ShowLibrary(false));
                }
            }
        });
        let items = self.items.clone();
        let cell = vec2(170.0, 140.0);
        let mut body = |ui: &mut egui::Ui| {
            for (i, it) in items.iter().enumerate() {
                let path = p(it);
                let sel = self.selection.iter().any(|s| eq(s, &path));
                let cur = eq(&path, &self.current);
                let t = self.thumb(&ctx, it);
                let (r, resp) = ui.allocate_exact_size(cell, Sense::click_and_drag());
                crate::panels::tag(ui, &resp, &format!("{name}.{i}"));
                let fill = if sel { Color32::from_rgb(204, 228, 255) } else if resp.hovered() { Color32::from_gray(235) } else { Color32::TRANSPARENT };
                ui.painter().rect_filled(r, 6.0, fill);
                if cur {
                    ui.painter().rect_stroke(r.shrink(1.0), 6.0, egui::Stroke::new(2.0, Color32::from_rgb(30, 120, 230)), egui::StrokeKind::Inside);
                }
                let img_r = egui::Rect::from_min_size(r.min + vec2(5.0, 5.0), vec2(cell.x - 10.0, cell.y - 32.0));
                if let Some(t) = t {
                    let sz = t.size_vec2();
                    let s = (img_r.width() / sz.x).min(img_r.height() / sz.y).min(1.0);
                    let dst = egui::Rect::from_center_size(img_r.center(), sz * s);
                    ui.painter().image(t.id(), dst, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                } else if it.video {
                    ui.painter().rect_filled(img_r.shrink(10.0), 4.0, Color32::from_gray(60));
                    ui.painter().text(img_r.center(), egui::Align2::CENTER_CENTER, "▶", egui::FontId::proportional(28.0), Color32::WHITE);
                }
                let label = Self::label(it);
                ui.painter().text(egui::pos2(r.center().x, r.bottom() - 14.0), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(12.0),
                                  Color32::from_gray(50));
                if resp.clicked() {
                    let ctrl = ui.input(|i| i.modifiers.ctrl);
                    if ctrl {
                        if sel {
                            self.selection.retain(|s| !eq(s, &path));
                        } else {
                            self.selection.push(path.clone());
                        }
                    } else {
                        self.selection = vec![path.clone()];
                        acts.push(TrayAction::Open(path.clone()));
                    }
                }
                if resp.double_clicked() {
                    acts.push(TrayAction::Open(path.clone()));
                }
                if resp.drag_started() {
                    let paths = if sel { self.selected_paths() } else { vec![path.clone()] };
                    acts.push(TrayAction::DragOut(paths, path.clone()));
                }
                if resp.secondary_clicked() && !sel {
                    self.selection = vec![path.clone()];
                }
                let s = self.selected_paths();
                let video = it.video;
                resp.context_menu(|ui| {
                    if s.len() == 1 {
                        if ui.button(tr("Abrir")).clicked() {
                            acts.push(TrayAction::Open(s[0].clone()));
                            ui.close();
                        }
                        if ui.button(tr("Mostrar na pasta")).clicked() {
                            acts.push(TrayAction::ShowInFolder(s[0].clone()));
                            ui.close();
                        }
                        ui.separator();
                    }
                    let exp = if s.len() > 1 { tr("Exportar %zu itens…").replace("%zu", &s.len().to_string()) } else { tr("Exportar…") };
                    if ui.button(exp).clicked() {
                        acts.push(TrayAction::Export(s.clone()));
                        ui.close();
                    }
                    if s.len() == 1 {
                        if ui.button(tr("Copiar como…")).clicked() {
                            acts.push(TrayAction::CopyAs(s[0].clone()));
                            ui.close();
                        }
                        if !video && ui.button(tr("Fixar na tela")).clicked() {
                            acts.push(TrayAction::Pin(s[0].clone()));
                            ui.close();
                        }
                        if ui.button(tr("Enviar (upload)")).clicked() {
                            acts.push(TrayAction::Upload(s[0].clone()));
                            ui.close();
                        }
                    }
                    if s.len() >= 2 && ui.button(tr("Combinar %zu imagens…").replace("%zu", &s.len().to_string())).clicked() {
                        acts.push(TrayAction::Combine(s.clone()));
                        ui.close();
                    }
                    if s.len() == 2 && ui.button(tr("Comparar as duas…")).clicked() {
                        acts.push(TrayAction::Compare(s.clone()));
                        ui.close();
                    }
                    let batch = if s.len() > 1 { tr("Processar %zu em lote…").replace("%zu", &s.len().to_string()) } else { tr("Processamento em lote…") };
                    if ui.button(batch).clicked() {
                        acts.push(TrayAction::Batch(s.clone()));
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(tr("Selecionar tudo\tCtrl+A").replace('\t', "   ")).clicked() {
                        self.select_all();
                        ui.close();
                    }
                    if s.len() == 1 && ui.button(tr("Renomear…\tF2").replace('\t', "   ")).clicked() {
                        acts.push(TrayAction::Rename(s[0].clone()));
                        ui.close();
                    }
                    if tray {
                        let t = if s.len() > 1 { tr("Remover da bandeja") } else { tr("Remover da bandeja (continua na biblioteca)") };
                        if ui.button(t).clicked() {
                            self.remove_from_tray(&s);
                            ui.close();
                        }
                    }
                    ui.separator();
                    let del = if s.len() > 1 {
                        tr("Excluir %zu itens (Lixeira)\tDel").replace("%zu", &s.len().to_string())
                    } else {
                        tr("Excluir (Lixeira)\tDel")
                    };
                    if ui.button(del.replace('\t', "   ")).clicked() {
                        acts.push(TrayAction::Delete(s.clone()));
                        ui.close();
                    }
                });
            }
        };
        if tray {
            egui::ScrollArea::horizontal().id_salt("tray-scroll").auto_shrink([false, false]).show(&mut ui, |ui| {
                ui.horizontal(|ui| body(ui));
            });
        } else {
            egui::ScrollArea::vertical().id_salt("lib-scroll").auto_shrink([false, false]).show(&mut ui, |ui| {
                ui.horizontal_wrapped(|ui| body(ui));
            });
        }
        acts
    }
}
