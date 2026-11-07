//! SnapnabEditor.exe's window (src/editor/main.cpp: CMainFrame): the toolbar, the canvas, the right column with the
//! Properties, Effects and Layers tabs, the footer and the recent captures tray, the native menu bar, the commands
//! (the same WM_COMMAND numbers as the C++ editor), the single instance and the IPC with Snapnab.exe.
//!
//! Every modal piece (the forms, message boxes, file and color dialogs) runs on a worker thread; its answer comes
//! back through the waker as a closure that runs on the UI thread with the editor (`Ev::Do`).

mod clipboard;
mod commands;
mod docs;
mod extras;
mod video;
mod windows_aux;

use std::collections::HashMap;
use std::ffi::OsStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use egui::{Color32, RichText};
use snapcore::ipc::{self, Channel, Notify};
use snapcore::{instance, log, reg};
use snapedit_app::canvas::{Canvas, Note, Tool, TOOL_COUNT};
use snapedit_app::canvas_ui::CanvasView;
use snapedit_app::effects::{EffectsPanel, FxAction};
use snapedit_app::ids::*;
use snapedit_app::native::{self, MenuStates};
use snapedit_app::panels::{self, BarAction, Entry, EntryKind, LayerAction};
use snapedit_app::props::{PropAction, PropsPanel};
use snapedit_app::styles::QuickStyleStore;
use snapedit_app::tray::{LibraryTray, Mode};
use snapedit_app::{menu, tr};
use snapui::host::{App, Caught, Ctl, RawMsg, WinKey, WindowSpec};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::UI::Shell::{DragAcceptFiles, DragFinish, DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::{WM_COMMAND, WM_COPYDATA, WM_DROPFILES, WM_INITMENUPOPUP};

pub const VERSION: &str = snapedit_app::VERSION;

/// A piece of work that comes back to the UI thread (the answer of a form, a dialog, a worker).
pub type Job = Box<dyn FnOnce(&mut Editor, &mut Ctl) + Send>;

pub enum Ev {
    Ipc(ipc::Message),
    Do(Job),
    LibChanged,
}

/// Set while a text field has the keyboard (the window procedure cannot reach the app).
static TEXT_FOCUS: AtomicBool = AtomicBool::new(false);

fn intercept(hwnd: isize, msg: u32, wp: usize, lp: isize) -> Caught {
    let raw = |text| RawMsg { hwnd, msg, wparam: wp, lparam: lp, text };
    match msg {
        WM_COPYDATA => match unsafe { instance::parse_copy_data(lp as *const COPYDATASTRUCT) } {
            Some(t) => Caught::Queue(raw(Some(t)), 1),
            None => Caught::Handled(0),
        },
        WM_COMMAND => Caught::Queue(raw(None), 0),
        WM_INITMENUPOPUP => {
            native::apply_menu_states(wp as isize);
            Caught::Pass
        }
        WM_DROPFILES => {
            // the names are read here: the HDROP lives only during the call
            let d = HDROP(wp as _);
            let n = unsafe { DragQueryFileW(d, 0xFFFF_FFFF, None) };
            let mut files = Vec::new();
            for i in 0..n {
                let mut buf = vec![0u16; 1024];
                let len = unsafe { DragQueryFileW(d, i, Some(&mut buf)) } as usize;
                files.push(String::from_utf16_lossy(&buf[..len]));
            }
            unsafe { DragFinish(d) };
            Caught::Queue(raw(Some(files.join("\n"))), 0)
        }
        _ => Caught::Pass,
    }
}

pub struct Editor {
    pub hwnd: isize,
    pub main: Option<WinKey>,
    pub cv: Canvas,
    pub view: CanvasView,
    pub props: PropsPanel,
    pub qs: QuickStyleStore,
    pub fx: EffectsPanel,
    pub tray: LibraryTray,
    pub library: LibraryTray,
    pub library_open: bool,
    /// right column: 0 Properties, 1 Effects, 2 Layers
    pub right_tab: usize,
    // toolbar: automatic (fixed + most recently used + Magnifier) or custom (any item)
    pub tb_custom: bool,
    pub tb_custom_list: Vec<Entry>,
    pub tb_recent: Vec<Tool>,
    pub tb_tool_count: usize,
    pub tb_active: Tool,
    last_use: HashMap<usize, Instant>,
    pub status: String,
    pub coords: String,
    pub show_tray: bool,
    pub ipc: Option<Notify>,
    pub waker: Option<snapui::host::Waker>,
    /// Add capture (C09): the next capture is appended to this document (for two minutes)
    pub add_capture_to: Option<(String, Instant)>,
    fx_preview_at: Option<Instant>,
    lib_reload_at: Option<Instant>,
    clip_new_at: Option<Instant>,
    /// the text windows (Grab Text, shortcuts) and the other auxiliary windows
    pub aux: windows_aux::Aux,
    pub video: video::VideoView,
    watch_stop: Arc<AtomicBool>,
    last_title: String,
    fx_key: u64,
    zoom_menu: bool,
}

impl Editor {
    fn new() -> Editor {
        let mut cv = Canvas::default();
        let qs = QuickStyleStore::load();
        for t in 0..TOOL_COUNT {
            if let Some(tool) = Tool::from_index(t)
                && let Some(s) = qs.styles_for(tool).into_iter().next()
            {
                cv.styles[t] = s;
            }
        }
        snapedit_app::styles::load_tool_styles(&mut cv.styles);
        let ed = |n: &str, d: u32| reg::get_dword(OsStr::new(n), d, Some(OsStr::new("Editor")));
        cv.smart_move = ed("SmartMove", 0) != 0;
        cv.snapping = ed("CanvasSnapping", 1) != 0;
        cv.rulers = ed("Rulers", 0) != 0;
        cv.edge_snap = ed("EdgeSnap", 1) != 0;
        cv.plain_paste = ed("PlainTextPaste", 1) != 0;
        cv.draw_hold = ed("DrawHold", 1) != 0;
        cv.background = ed("CanvasBackground", 0x00E4E0DE);
        let mut e = Editor {
            hwnd: 0,
            main: None,
            cv,
            view: CanvasView::default(),
            props: PropsPanel::new(),
            qs,
            fx: EffectsPanel::new(),
            tray: LibraryTray::new(Mode::Tray),
            library: LibraryTray::new(Mode::Library),
            library_open: false,
            right_tab: (ed("RightPanel", 0) as usize).min(2),
            tb_custom: false,
            tb_custom_list: Vec::new(),
            tb_recent: Vec::new(),
            tb_tool_count: 10,
            tb_active: Tool::Arrow,
            last_use: HashMap::new(),
            status: String::new(),
            coords: String::new(),
            show_tray: ed("ShowTray", 1) != 0,
            ipc: None,
            waker: None,
            add_capture_to: None,
            fx_preview_at: None,
            lib_reload_at: None,
            clip_new_at: None,
            aux: Default::default(),
            video: Default::default(),
            watch_stop: Arc::new(AtomicBool::new(false)),
            last_title: String::new(),
            fx_key: 1,
            zoom_menu: false,
        };
        e.load_toolbar_config();
        e
    }

    /// Runs `work` on a worker thread (a form, a dialog) and `then` with its answer on the UI thread.
    pub fn modal<R: Send + 'static>(&self, work: impl FnOnce(isize) -> R + Send + 'static,
                                    then: impl FnOnce(&mut Editor, &mut Ctl, R) + Send + 'static) {
        let owner = self.hwnd;
        let Some(w) = self.waker.clone() else { return };
        std::thread::spawn(move || {
            let r = work(owner);
            w.send(Ev::Do(Box::new(move |e: &mut Editor, c: &mut Ctl| then(e, c, r))));
        });
    }

    /// A message box on a worker thread (the window keeps drawing).
    pub fn inform(&self, text: impl Into<String>, title: impl Into<String>) {
        native::inform(self.hwnd, text.into(), title.into());
    }

    pub fn warn(&self, text: impl Into<String>) {
        let (owner, text) = (self.hwnd, text.into());
        std::thread::spawn(move || {
            native::message(owner, &text, "Snapnab Editor", windows::Win32::UI::WindowsAndMessaging::MB_ICONWARNING);
        });
    }

    pub fn schedule_effect_previews(&mut self) {
        self.fx_preview_at = Some(Instant::now() + Duration::from_millis(350));
    }

    pub fn update_title(&mut self, ctl: &Ctl) {
        let t = match self.cv.doc() {
            Some(d) => format!("{}{}{}", if d.modified { "• " } else { "" }, d.title, tr(" — Snapnab Editor")),
            None => "Snapnab Editor".to_owned(),
        };
        if t != self.last_title
            && let Some(w) = self.main.and_then(|k| ctl.window(k))
        {
            w.set_title(&t);
            self.last_title = t;
        }
    }

    pub fn update_doc_info(&mut self) {
        let info = match self.cv.doc() {
            None => String::new(),
            Some(d) if d.is_video => format!("{}{}", tr("Vídeo: "), snapedit_app::doc::file_name_of(&d.path)),
            Some(d) => {
                let mut s = tr("%d × %d px").replacen("%d", &d.w().to_string(), 1).replacen("%d", &d.h().to_string(), 1);
                if !d.capture_mode.is_empty() {
                    s += &(tr("\nCaptura: ") + &d.capture_mode);
                }
                if !d.application.is_empty() {
                    s += &(tr("\nAplicativo: ") + &d.application);
                }
                if !d.window_title.is_empty() {
                    s += &(tr("\nJanela: ") + &d.window_title);
                }
                s += &tr("\nObjetos: %zu").replace("%zu", &d.st.objs.len().to_string());
                if !d.tags.is_empty() {
                    s += &(tr("\nEtiquetas: ") + &d.tags);
                }
                s
            }
        };
        self.props.doc_info = info;
    }

    /// DocChanged: the title, the document information, the effect previews and the status.
    pub fn doc_changed(&mut self) {
        self.update_doc_info();
        self.schedule_effect_previews();
        if let Some(d) = self.cv.doc()
            && !d.is_video
        {
            self.status = tr("%d × %d px  •  %zu objeto(s)  •  %zu passo(s) para desfazer")
                .replacen("%d", &d.w().to_string(), 1)
                .replacen("%d", &d.h().to_string(), 1)
                .replacen("%zu", &d.st.objs.len().to_string(), 1)
                .replacen("%zu", &d.undo_depth().to_string(), 1);
        }
    }

    pub fn set_right_tab(&mut self, t: usize) {
        if self.right_tab == t {
            return;
        }
        if self.right_tab == 1 && self.fx.is_open() {
            self.fx.collapse(); // leaving the effects cancels the preview
            self.cv.set_effect_preview(None);
        }
        self.right_tab = t;
        reg::set_dword(OsStr::new("RightPanel"), t as u32, Some(OsStr::new("Editor")));
        if t == 1 {
            self.schedule_effect_previews();
        }
    }

    pub fn show_library(&mut self, on: bool) {
        if self.library_open == on {
            return;
        }
        self.library_open = on;
        if on {
            self.library.reload();
            let c = self.library.current.clone();
            self.library.set_current(&c);
        }
    }

    fn is_video(&self) -> bool {
        self.cv.doc().is_some_and(|d| d.is_video)
    }

    // ---------------------------------------------------------------- toolbar configuration
    fn recent_slots(&self) -> usize {
        self.tb_tool_count.saturating_sub(panels::FIXED_TOOLS.len() + 1)
    }

    pub fn load_toolbar_config(&mut self) {
        let sub = Some(OsStr::new("Editor"));
        self.tb_custom = reg::get_dword(OsStr::new("ToolbarCustom"), 0, sub) != 0;
        self.tb_custom_list = panels::entries_from_string(&reg_str("ToolbarItems", ""));
        self.tb_tool_count = (reg::get_dword(OsStr::new("ToolbarToolCount"), 10, sub) as usize).clamp(6, 16);
        self.tb_recent = panels::entries_from_string(&reg_str("ToolbarRecentTools", ""))
            .into_iter()
            .filter(|e| e.kind == EntryKind::Tool)
            .filter_map(|e| Tool::from_index(e.id as usize))
            .filter(|t| !panels::is_fixed_tool(*t))
            .collect();
        self.fit_recent_tools();
    }

    pub fn fit_recent_tools(&mut self) {
        let n = self.recent_slots();
        self.tb_recent.truncate(n);
        for t in [Tool::Arrow, Tool::Callout, Tool::Step, Tool::Blur, Tool::Stamp, Tool::Pen, Tool::Line, Tool::Fill, Tool::Crop,
                  Tool::Spotlight, Tool::Eraser]
        {
            if self.tb_recent.len() < n && !self.tb_recent.contains(&t) {
                self.tb_recent.push(t);
            }
        }
    }

    pub fn save_toolbar_config(&self) {
        let sub = Some(OsStr::new("Editor"));
        reg::set_dword(OsStr::new("ToolbarCustom"), self.tb_custom as u32, sub);
        reg::set_dword(OsStr::new("ToolbarToolCount"), self.tb_tool_count as u32, sub);
        set_reg_str("ToolbarItems", &panels::entries_to_string(&self.tb_custom_list));
        let rec: Vec<Entry> = self.tb_recent.iter().map(|t| Entry { kind: EntryKind::Tool, id: t.index() as u32 }).collect();
        set_reg_str("ToolbarRecentTools", &panels::entries_to_string(&rec));
    }

    pub fn default_entries(&self) -> Vec<Entry> {
        let tool = |t: Tool| Entry { kind: EntryKind::Tool, id: t.index() as u32 };
        let mut v: Vec<Entry> = panels::FIXED_TOOLS.iter().map(|t| tool(*t)).collect();
        v.extend(self.tb_recent.iter().map(|t| tool(*t)));
        v.push(tool(Tool::Magnify));
        v
    }

    pub fn center_entries(&self) -> Vec<Entry> {
        if self.tb_custom && !self.tb_custom_list.is_empty() { self.tb_custom_list.clone() } else { self.default_entries() }
    }

    /// A tool used: in automatic mode it takes one of the recent places, replacing the least recently used.
    fn note_tool_used(&mut self, t: Tool) {
        self.last_use.insert(t.index(), Instant::now());
        if self.tb_custom || panels::is_fixed_tool(t) || self.recent_slots() == 0 || self.tb_recent.contains(&t) {
            return;
        }
        let lu = |x: &Tool| self.last_use.get(&x.index()).copied();
        if let Some(i) = (0..self.tb_recent.len()).min_by_key(|&i| lu(&self.tb_recent[i])) {
            self.tb_recent[i] = t;
        }
        self.save_toolbar_config();
    }

    pub fn set_tool(&mut self, t: Tool) {
        if t == Tool::Favorites {
            // the drawing tool becomes that of the last used favorite (or of the first favorite)
            let ct = self.cv.tool;
            if !self.qs.favorites.iter().any(|f| f.tool == ct)
                && let Some(f) = self.qs.favorites.first()
            {
                self.cv.styles[f.tool.index()] = f.style.clone();
                self.cv.set_tool(f.tool);
            } else {
                self.cv.set_tool(ct);
            }
        } else {
            if t == Tool::Selection && self.cv.sel_mode == 3 {
                self.cv.sel_mode = 0;
            }
            self.cv.set_tool(t);
        }
        self.tb_active = t;
        self.note_tool_used(t);
        if self.right_tab != 0 && t != Tool::Select {
            self.set_right_tab(0);
        }
        self.props.bar_tool = t;
        self.status = format!("{}{}", tr("Ferramenta: "), t.name());
        if t == Tool::Magnify {
            self.show_tool_intro();
        }
    }

    // ---------------------------------------------------------------- the frame
    fn main_ui(&mut self, ctl: &mut Ctl, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.timers(ctl, &ctx);
        self.keys(ctl, &ctx);
        let video = self.is_video();
        // toolbar
        let center = self.center_entries();
        let fx_names: HashMap<u32, String> = self.fx.effects.iter().map(|e| (e.cmd, e.name.clone())).collect();
        let mut bar = Vec::new();
        egui::Panel::top("SnapnabToolbar").exact_size(46.0).resizable(false).show(ui, |ui| {
            bar = panels::toolbar(ui, &center, self.tb_active, &|c| fx_names.get(&c).cloned());
        });
        // footer: status, coordinates, image size and zoom
        let mut foot_cmd = None;
        egui::Panel::bottom("SnapnabStatus").exact_size(26.0).resizable(false).show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                let r = ui.selectable_label(self.show_tray, tr("Capturas recentes"));
                panels::tag(ui, &r, "SnapnabStatus.Tray");
                if r.clicked() {
                    foot_cmd = Some(M_SHOW_TRAY);
                }
                ui.label(RichText::new(&self.status).color(Color32::from_gray(70)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(d) = self.cv.doc().filter(|d| !d.is_video) {
                        let z = (self.cv.zoom * 100.0).round() as i32;
                        let r = ui.button(format!("{} ▾", tr("Zoom %d%%").replace("%d", &z.to_string()).replace("%%", "%")));
                        panels::tag(ui, &r, "SnapnabStatus.Zoom");
                        if r.clicked() {
                            self.zoom_menu = !self.zoom_menu;
                        }
                        let size = format!("{} × {} px  ▾", d.w(), d.h());
                        let r = ui.button(size);
                        panels::tag(ui, &r, "SnapnabStatus.Size");
                        if r.clicked() {
                            foot_cmd = Some(M_RESIZE);
                        }
                        ui.label(RichText::new(&self.coords).color(Color32::from_gray(90)));
                    }
                });
            });
        });
        if self.zoom_menu {
            self.zoom_popup(&ctx);
        }
        // recent captures
        let mut tray_acts = Vec::new();
        if self.show_tray {
            egui::Panel::bottom("SnapnabTray").exact_size(128.0).resizable(false).show(ui, |ui| {
                tray_acts = self.tray.ui(ui);
            });
        }
        let mut fx_acts = Vec::new();
        let mut prop_acts = Vec::new();
        let mut layer_acts = Vec::new();
        let mut lib_acts = Vec::new();
        if self.library_open {
            egui::CentralPanel::default().show(ui, |ui| lib_acts = self.library.ui(ui));
        } else if video {
            egui::CentralPanel::default().show(ui, |ui| self.video_ui(ctl, ui));
        } else {
            egui::Panel::right("SnapnabRight").exact_size(296.0).resizable(false).show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (i, name) in ["Propriedades", "Efeitos", "Camadas"].iter().enumerate() {
                        let r = ui.selectable_label(self.right_tab == i, tr(name));
                        panels::tag(ui, &r, &format!("SnapnabTabs.{i}"));
                        if r.clicked() {
                            self.set_right_tab(i);
                        }
                    }
                });
                ui.separator();
                match self.right_tab {
                    1 => {
                        let (w, h) = self.cv.doc().map_or((0, 0), |d| (d.w(), d.h()));
                        let sel = self.cv.has_raster_selection().then(|| self.cv.raster_selection_bounds());
                        let enabled = self.cv.doc().is_some();
                        let ui = &mut ui.new_child(egui::UiBuilder::new().max_rect(ui.available_rect_before_wrap()));
                        let bg = ui.allocate_rect(ui.max_rect(), egui::Sense::hover());
                        panels::tag(ui, &bg, "SnapnabEffects");
                        let ui = &mut ui.new_child(egui::UiBuilder::new().max_rect(bg.rect));
                        egui::ScrollArea::vertical().show(ui, |ui| fx_acts = self.fx.ui(ui, w, h, sel, enabled));
                    }
                    2 => layer_acts = panels::layers(ui, &self.cv),
                    _ => {
                        egui::ScrollArea::vertical().show(ui, |ui| prop_acts = self.props.ui(ui, &mut self.cv, &mut self.qs));
                    }
                }
            });
            egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
                let rect = ui.max_rect();
                self.view.show(ui, rect, &mut self.cv);
            });
        }
        // what the panels asked for
        if let Some(c) = foot_cmd {
            self.command(ctl, c);
        }
        for a in bar {
            match a {
                BarAction::Command(c) => self.command(ctl, c),
                BarAction::Entry(e) => match e.kind {
                    EntryKind::Tool => {
                        if let Some(t) = Tool::from_index(e.id as usize) {
                            self.set_tool(t);
                        }
                    }
                    EntryKind::Effect => self.open_effect(e.id),
                    _ => self.command(ctl, e.id),
                },
                BarAction::More(_) => native::post_command(self.hwnd, commands::MORE_TOOLS),
                BarAction::Hint(h) => self.status = h,
            }
        }
        for a in prop_acts {
            self.prop_action(ctl, a);
        }
        for a in fx_acts {
            self.fx_action(ctl, a);
        }
        for a in layer_acts {
            self.layer_action(ctl, a);
        }
        for a in tray_acts.into_iter().chain(lib_acts) {
            self.tray_action(ctl, a);
        }
        self.handle_notes(ctl);
        self.update_title(ctl);
        TEXT_FOCUS.store(ctx.egui_wants_keyboard_input(), Ordering::Relaxed);
        self.publish_menu_states();
    }

    fn zoom_popup(&mut self, ctx: &egui::Context) {
        let mut close = false;
        egui::Window::new("zoom").title_bar(false).resizable(false).anchor(egui::Align2::RIGHT_BOTTOM, [-8.0, -30.0]).show(ctx, |ui| {
            for z in [25, 50, 75, 100, 150, 200, 300, 400, 800] {
                let on = (self.cv.zoom * 100.0).round() as i32 == z;
                if ui.selectable_label(on, format!("{z}%")).clicked() {
                    self.cv.zoom_to(z as f64 / 100.0, None);
                    close = true;
                }
            }
            ui.separator();
            if ui.button(tr("Ajustar à janela\tCtrl+0").replace('\t', "   ")).clicked() {
                self.cv.fit_to_window();
                close = true;
            }
        });
        if close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.zoom_menu = false;
        }
    }

    /// The timers of the C++ frame: the effect previews after a pause, the library reload after a burst of changes,
    /// the new image from the clipboard once it settles.
    fn timers(&mut self, ctl: &mut Ctl, ctx: &egui::Context) {
        let now = Instant::now();
        let mut next: Option<Instant> = None;
        let mut due = |t: &mut Option<Instant>| -> bool {
            match *t {
                Some(at) if at <= now => {
                    *t = None;
                    true
                }
                Some(at) => {
                    next = Some(next.map_or(at, |n: Instant| n.min(at)));
                    false
                }
                None => false,
            }
        };
        let fx = due(&mut self.fx_preview_at);
        let lib = due(&mut self.lib_reload_at);
        let clip = due(&mut self.clip_new_at);
        if let Some(n) = next {
            ctx.request_repaint_after(n.saturating_duration_since(now));
        }
        if fx {
            let src = self.cv.doc().filter(|d| !d.is_video).map(|d| {
                let r = d.render(None);
                snapcore::gdip::make_thumbnail(&r, 240, 180)
            });
            self.fx_key += 1;
            self.fx.set_source(src, self.fx_key);
        }
        if lib {
            self.tray.reload();
            if self.library_open {
                self.library.reload();
            }
        }
        if clip {
            self.new_from_clipboard(ctl);
            self.show_editor(ctl);
        }
    }

    /// The keyboard: the menu shortcuts (TranslateAccelerator), the tool letters with the canvas in focus, and the
    /// canvas keys (Esc, Enter, Delete, arrows) and typed labels.
    fn keys(&mut self, ctl: &mut Ctl, ctx: &egui::Context) {
        let events = ctx.input(|i| i.events.clone());
        let field = ctx.egui_wants_keyboard_input();
        let editing = self.cv.editing_text();
        let canvas_view = !self.library_open && !self.is_video() && self.cv.doc().is_some();
        for e in events {
            match e {
                egui::Event::Key { key, pressed: true, modifiers, .. } => {
                    if editing {
                        if key == egui::Key::Escape {
                            self.cv.commit_text();
                        }
                        continue;
                    }
                    if self.is_video() && !modifiers.ctrl && self.video_key(ctl, key) {
                        continue;
                    }
                    // in a text field the field keeps its own keys (Ctrl+Z/Y/X/C/V/A); the other shortcuts still work
                    let field_key = !modifiers.ctrl
                        || matches!(key, egui::Key::Z | egui::Key::Y | egui::Key::X | egui::Key::C | egui::Key::V | egui::Key::A);
                    if !(field && field_key)
                        && let Some(c) = menu::accelerator(key, modifiers)
                    {
                        self.command(ctl, c);
                        continue;
                    }
                    if field || !canvas_view {
                        continue;
                    }
                    let plain = !modifiers.ctrl && !modifiers.alt;
                    let step = if modifiers.shift { 10 } else { 1 };
                    match key {
                        egui::Key::Escape if plain => self.cv.escape(),
                        egui::Key::Enter if plain => self.cv.enter(),
                        egui::Key::Delete | egui::Key::Backspace if plain => self.cv.delete_key(),
                        egui::Key::ArrowLeft if plain => self.cv.nudge(-step, 0),
                        egui::Key::ArrowRight if plain => self.cv.nudge(step, 0),
                        egui::Key::ArrowUp if plain => self.cv.nudge(0, -step),
                        egui::Key::ArrowDown if plain => self.cv.nudge(0, step),
                        _ if plain && !modifiers.shift && !self.cv.wants_typing() => {
                            if let Some(t) = menu::tool_key(key) {
                                self.set_tool(t);
                            }
                        }
                        _ => {}
                    }
                }
                egui::Event::Text(t) if !field && !editing && canvas_view && self.cv.wants_typing() => self.cv.type_label(&t),
                egui::Event::Copy if !field && !editing => self.command(ctl, M_COPY),
                egui::Event::Cut if !field && !editing => self.command(ctl, M_CUT),
                egui::Event::Paste(_) | egui::Event::PasteImage(_) if !field && !editing => self.command(ctl, M_PASTE),
                _ => {}
            }
        }
    }

    fn handle_notes(&mut self, ctl: &mut Ctl) {
        let notes = std::mem::take(&mut self.cv.notes);
        for n in notes {
            match n {
                Note::DocChanged => self.doc_changed(),
                Note::SelectionChanged => {
                    let n = self.cv.selection_count();
                    if n > 1 {
                        self.status = tr("%d objetos selecionados  •  Ctrl+Shift+G agrupa  •  Organizar › Alinhar alinha entre si")
                            .replace("%d", &n.to_string());
                    }
                }
                Note::AreaChanged | Note::ZoomChanged => {}
                Note::ColorPicked(c) => {
                    // the eyedropper: the color goes to the drawing tools
                    for t in [Tool::Arrow, Tool::Line, Tool::Shape, Tool::Pen, Tool::Step, Tool::Stamp, Tool::Fill, Tool::Eyedropper] {
                        self.cv.styles[t.index()].stroke = c;
                    }
                    self.cv.styles[Tool::Text.index()].text_color = c;
                    let cr = snapedit_app::props::to_colorref(c);
                    let hex = format!("#{:02X}{:02X}{:02X}", cr & 255, cr >> 8 & 255, cr >> 16 & 255);
                    self.status = format!("{}{}{}", tr("Cor capturada: "), hex, tr(" (aplicada às ferramentas de desenho)"));
                }
                Note::CursorMoved(inside, x, y) => {
                    self.coords = if inside && self.cv.rulers { format!("X: {}   Y: {}", x.floor() as i32, y.floor() as i32) } else { String::new() };
                }
                Note::Hint(h) => self.status = h,
                Note::ContextMenu(_, _) => native::post_command(self.hwnd, commands::CONTEXT_MENU),
            }
        }
        let _ = ctl;
    }

    fn prop_action(&mut self, ctl: &mut Ctl, a: PropAction) {
        match a {
            PropAction::Command(id) => match id {
                324 => {
                    if self.cv.tool == Tool::Crop {
                        self.cv.apply_crop();
                    } else {
                        self.command(ctl, M_CROP_SEL);
                    }
                }
                325 => self.command(ctl, if self.cv.tool == Tool::Crop { M_TRIM } else { M_COPY }),
                326 | 329 => self.cv.delete_selected(),
                327 => self.cv.bring_to_front(),
                328 => self.cv.send_to_back(),
                330 => self.command(ctl, M_GROUP),
                331 => self.command(ctl, M_UNGROUP),
                other => self.command(ctl, other),
            },
            PropAction::Hint(h) => self.status = h,
            PropAction::PickTool(t) => self.cv.set_tool(t),
            PropAction::StylesChanged => {}
            PropAction::DocChanged => self.doc_changed(),
            PropAction::Modal(job) => self.modal(job, |e, c, apply| {
                if let Some(apply) = apply {
                    let more = apply(&mut e.props, &mut e.cv, &mut e.qs);
                    for a in more {
                        e.prop_action(c, a);
                    }
                }
            }),
        }
    }

    fn fx_action(&mut self, ctl: &mut Ctl, a: FxAction) {
        match a {
            FxAction::Preview(cmd) => self.preview_effect(cmd),
            FxAction::Apply(cmd) => self.apply_effect(ctl, cmd),
            FxAction::Cancel => {
                self.fx.collapse();
                self.cv.set_effect_preview(None);
            }
            FxAction::Quick(i) => self.apply_quick_effect(ctl, i),
            FxAction::AddQuick => self.add_quick_effect(),
            FxAction::DeleteQuick(i) => {
                let Some(q) = self.fx.quick.get(i) else { return };
                let text = tr("Excluir o efeito rápido \"%s\"?").replace("%s", &tr(&q.name));
                self.modal(move |o| native::message(o, &text, "Snapnab Editor", windows::Win32::UI::WindowsAndMessaging::MB_YESNO
                    | windows::Win32::UI::WindowsAndMessaging::MB_ICONQUESTION), move |e, _, r| {
                    if r == windows::Win32::UI::WindowsAndMessaging::IDYES.0 && i < e.fx.quick.len() {
                        e.fx.quick.remove(i);
                        snapedit_app::effects::save_quick_effects(&e.fx.quick);
                        e.fx.invalidate_thumbs();
                    }
                });
            }
            FxAction::PickColor(cmd, p) => {
                let Some(cur) = self.fx.find(cmd).and_then(|e| e.params.get(p)).map(|x| x.color()) else { return };
                self.modal(move |o| native::choose_color(o, cur), move |e, _, c| {
                    if let Some(c) = c
                        && let Some(par) = e.fx.find_mut(cmd).and_then(|x| x.params.get_mut(p))
                    {
                        if let snapedit_app::effects::Param::Color { color, .. } = par {
                            *color = c;
                        }
                        e.preview_effect(cmd);
                    }
                });
            }
        }
    }

    fn layer_action(&mut self, ctl: &mut Ctl, a: LayerAction) {
        match a {
            LayerAction::Select(i, add) => {
                if add {
                    let mut v = self.cv.selected_list();
                    if let Some(p) = v.iter().position(|&x| x == i) {
                        v.remove(p);
                    } else {
                        v.push(i);
                    }
                    self.cv.select_objects(&v);
                } else {
                    self.cv.select_objects(&[i]);
                }
            }
            LayerAction::Hidden(i, h) => self.cv.set_hidden(&[i], h),
            LayerAction::Locked(i, l) => self.cv.set_locked(&[i], l),
            LayerAction::Move(i, before) => self.cv.move_objects(&[i], before),
            LayerAction::Rename(i) => {
                self.cv.select_objects(&[i]);
                self.command(ctl, M_RENAME_OBJ);
            }
            LayerAction::ContextMenu => native::post_command(self.hwnd, commands::CONTEXT_MENU),
        }
    }

    pub fn show_editor(&self, ctl: &Ctl) {
        if let Some(w) = self.main.and_then(|k| ctl.window(k)) {
            w.set_visible(true);
            w.set_minimized(false);
            w.focus_window();
        }
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(HWND(self.hwnd as _));
        }
    }

    /// Enabled and checked menu states (WM_INITMENUPOPUP).
    fn publish_menu_states(&self) {
        let img = self.cv.doc().is_some_and(|d| !d.is_video);
        let doc = self.cv.doc();
        let obj = img && self.cv.selected >= 0;
        let n = self.cv.selection_count();
        let mut s = MenuStates::default();
        let mut en = |id: u32, on: bool| s.enabled.push((id, on));
        en(M_UNDO, img && doc.is_some_and(|d| d.can_undo()));
        en(M_REDO, img && doc.is_some_and(|d| d.can_redo()));
        en(M_UNDOALL, img && doc.is_some_and(|d| d.can_undo()));
        for id in [M_SAVE, M_SAVEAS, M_DUPLICATE, M_PRINT, M_EMAIL, M_OPEN_PROGRAM, M_COPYALL, M_SELECTALL, M_OCR, M_COPY, M_APPLY_TEMPLATE,
                   M_READ_QR, M_BEAUTIFY, M_REDACT, M_REMOVE_BG, M_SPLIT, M_ADD_CAPTURE, M_DATESTAMP, M_ASSISTANT, M_PIN, M_TRIM, M_RESIZE,
                   M_CANVAS, M_CANVAS_COLOR, M_TAG, M_FLIP_H, M_FLIP_V, M_ROT_CW, M_ROT_CCW, M_ROT_180, M_ROT_FREE, M_CROP_SEL]
        {
            en(id, img);
        }
        en(M_CUT, img && !self.cv.selection_locked());
        for id in [M_FRONT, M_BACK, M_BRING_FORWARD, M_SEND_BACKWARD, M_FLATTEN_SEL, M_ALIGN_LEFT, M_ALIGN_CENTER, M_ALIGN_RIGHT, M_ALIGN_TOP,
                   M_ALIGN_MIDDLE, M_ALIGN_BOTTOM, M_LOCK, M_RENAME_OBJ]
        {
            en(id, obj);
        }
        en(M_UNLOCK, obj && self.cv.selection_locked());
        en(M_DIST_H, img && n >= 3);
        en(M_DIST_V, img && n >= 3);
        en(M_LIFT, img && self.cv.has_raster_selection());
        en(M_DROP, img && self.cv.has_floating());
        en(M_CLEAR_GUIDES, img && doc.is_some_and(|d| !d.guides_x.is_empty() || !d.guides_y.is_empty()));
        en(M_FLATTEN, img && doc.is_some_and(|d| !d.st.objs.is_empty()));
        en(M_GROUP, img && n >= 2);
        en(M_UNGROUP, img && self.cv.can_ungroup());
        for t in 0..4 {
            en(M_OFFICE_BASE + t, img);
        }
        for i in 0..16 {
            en(M_CURSOR_BASE + i, img);
        }
        en(M_UPLOAD, doc.is_some());
        en(M_COPYAS, doc.is_some());
        en(M_DELETE, doc.is_some());
        en(M_CLOSE, doc.is_some());
        for id in EFFECTS {
            en(id, img);
        }
        let mut ck = |id: u32, on: bool| s.checked.push((id, on));
        ck(M_RULERS, self.cv.rulers);
        ck(M_EDGESNAP, self.cv.edge_snap);
        ck(M_PLAINPASTE, self.cv.plain_paste);
        ck(M_DRAWHOLD, self.cv.draw_hold);
        ck(M_SNAPPING, self.cv.snapping);
        ck(M_SHOW_TRAY, self.show_tray);
        ck(M_SHOW_EFFECTS, self.right_tab == 1 && !self.library_open);
        ck(M_SHOW_LAYERS, self.right_tab == 2 && !self.library_open);
        ck(M_SHOW_LIBRARY, self.library_open);
        for t in 0..TOOL_COUNT as u32 {
            ck(M_MORE_TOOL_BASE + t, t as usize == self.tb_active.index());
        }
        native::set_menu_states(s);
    }

    // ---------------------------------------------------------------- IPC and the command line
    fn on_ipc(&mut self, ctl: &mut Ctl, m: ipc::Message) {
        use ipc::code;
        match m.code {
            code::SHOW_EDITOR | code::SHOW_EDITOR_WITH_PARAM => self.show_editor(ctl),
            code::CLOSE_EDITOR_IF_HIDDEN => {
                if self.main.and_then(|k| ctl.window(k)).is_some_and(|w| w.is_visible() == Some(false)) {
                    self.close(ctl);
                }
            }
            code::RELOAD_CONFIG => self.cv.refresh(false),
            code::NEW_IMAGE_FROM_CLIPBOARD => {
                log::write("Snip detected. New Image From Clipboard commencing.");
                self.clip_new_at = Some(Instant::now() + Duration::from_millis(500));
            }
            code::OPEN_FILE => {
                if m.text.is_empty() {
                    return;
                }
                if let Some((target, at)) = self.add_capture_to.take()
                    && at.elapsed() < Duration::from_secs(120)
                    && snapedit_app::doc::ext_of(&m.text) == ".snap"
                    && self.append_capture(ctl, &target, &m.text)
                {
                    self.show_editor(ctl);
                    return;
                }
                self.open_path(ctl, &m.text);
                self.show_editor(ctl);
            }
            code::LIBRARY_CHANGED => {
                self.tray.reload();
                if self.library_open {
                    self.library.reload();
                }
            }
            code::RESTART_EDITOR => {
                if instance::launch_sibling("SnapnabEditor.exe", &instance::restart_args(), false) {
                    self.close(ctl);
                }
            }
            _ => {}
        }
    }

    pub fn command_line(&mut self, ctl: &mut Ctl, cmd: &str, startup: bool) {
        let args = instance::split_command_line(cmd);
        let mut opened = false;
        let mut i = 0;
        while i < args.len() {
            let a = args[i].to_lowercase();
            if a == "/open" && i + 1 < args.len() {
                i += 1;
                opened |= self.open_path(ctl, &args[i]);
            } else if a == "/newfromclipboard" {
                self.new_from_clipboard(ctl);
                opened = true;
            } else if a == "/restartwait" {
                if args.get(i + 1).is_some_and(|n| n.starts_with(|c: char| c.is_ascii_digit())) {
                    i += 1;
                }
            } else if a == "/help" || a == "/?" {
                self.inform(tr("SnapnabEditor.exe [opções] [arquivo…]\n\n/Open \"arquivo\"     abre um arquivo (.snap, imagem, .mp4)\n/NewFromClipboard   cria imagem a partir da área de transferência\n/restartwait        espera a instância anterior fechar"),
                            "Snapnab Editor");
            } else if !a.starts_with('/') {
                opened |= self.open_path(ctl, &args[i]);
            }
            i += 1;
        }
        if startup && !opened {
            // the most recent capture in the library
            let items = snapcore::library::list(&snapcore::library::library_dir(), 1);
            if let Some(it) = items.first() {
                let p = String::from_utf16_lossy(&it.path);
                self.open_path(ctl, &p);
            }
        }
        if !startup {
            self.show_editor(ctl);
        }
    }

    fn start_library_watcher(&self) {
        let Some(w) = self.waker.clone() else { return };
        let stop = self.watch_stop.clone();
        std::thread::spawn(move || {
            use windows::Win32::Foundation::WAIT_OBJECT_0;
            use windows::Win32::Storage::FileSystem::{
                FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FindCloseChangeNotification, FindFirstChangeNotificationW,
                FindNextChangeNotification,
            };
            use windows::Win32::System::Threading::WaitForSingleObject;
            let dir = native::wide(&snapedit_app::files::library_dir());
            let Ok(h) = (unsafe {
                FindFirstChangeNotificationW(windows::core::PCWSTR(dir.as_ptr()), false, FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE)
            }) else {
                return;
            };
            while !stop.load(Ordering::Relaxed) {
                if unsafe { WaitForSingleObject(h, 500) } == WAIT_OBJECT_0 {
                    w.send(Ev::LibChanged);
                    if unsafe { FindNextChangeNotification(h) }.is_err() {
                        break;
                    }
                }
            }
            unsafe {
                let _ = FindCloseChangeNotification(h);
            }
        });
    }

    /// WM_CLOSE: the documents are saved, then the window goes and the process ends.
    pub fn close(&mut self, ctl: &mut Ctl) {
        self.cv.commit_text();
        for i in 0..self.cv.docs.len() {
            self.autosave(i);
        }
        snapedit_app::styles::save_tool_styles(&self.cv.styles);
        let sub = Some(OsStr::new("Editor"));
        reg::set_dword(OsStr::new("SmartMove"), self.cv.smart_move as u32, sub);
        let h = HWND(self.hwnd as _);
        let mut wp = windows::Win32::UI::WindowsAndMessaging::WINDOWPLACEMENT {
            length: std::mem::size_of::<windows::Win32::UI::WindowsAndMessaging::WINDOWPLACEMENT>() as u32,
            ..Default::default()
        };
        if unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowPlacement(h, &mut wp) }.is_ok() {
            let r = wp.rcNormalPosition;
            let max = wp.showCmd == windows::Win32::UI::WindowsAndMessaging::SW_SHOWMAXIMIZED.0 as u32;
            set_reg_str("WindowPlacement", &format!("{} {} {} {} {}", r.left, r.top, r.right, r.bottom, max as i32));
        }
        self.watch_stop.store(true, Ordering::Relaxed);
        self.video.close();
        if let Some(i) = &self.ipc {
            i.send_int(Channel::Capture, ipc::code::EDITOR_CLOSED, 0, false);
        }
        ctl.exit();
    }
}

impl App for Editor {
    fn start(&mut self, ctl: &mut Ctl) {
        // the saved window place (when it is still on a monitor)
        let wp = reg_str("WindowPlacement", "");
        let v: Vec<i32> = wp.split_whitespace().filter_map(|x| x.parse().ok()).collect();
        let mut position = None;
        let mut size = (1280.0, 820.0);
        let mut physical = false;
        let mut maximize = false;
        if v.len() == 5 && v[2] - v[0] > 300 && v[3] - v[1] > 300 {
            let (cx, cy) = ((v[0] + v[2]) / 2, (v[1] + v[3]) / 2);
            if snapcore::screen::monitor_rect(cx, cy, false).contains(cx, cy) {
                position = Some((v[0], v[1]));
                size = ((v[2] - v[0]) as f32, (v[3] - v[1]) as f32);
                physical = true;
                maximize = v[4] != 0;
            }
        }
        let spec = WindowSpec {
            class: ipc::EDITOR_WND_CLASS,
            title: "Snapnab Editor".into(),
            size,
            physical_size: physical,
            position,
            visible: true,
            intercept: Some(intercept),
            clear_color: [0.95, 0.95, 0.96, 1.0],
            ..Default::default()
        };
        let Some(k) = ctl.create(&spec) else {
            log::write("Editor window: could not be created");
            ctl.exit();
            return;
        };
        self.main = Some(k);
        self.hwnd = ctl.hwnd(k);
        self.cv.hwnd = self.hwnd;
        if let Some(w) = ctl.window(k) {
            w.set_min_inner_size(Some(winit::dpi::LogicalSize::new(900.0, 560.0)));
            if maximize {
                w.set_maximized(true);
            }
        }
        unsafe {
            let m = menu::build_menu().into_raw();
            let _ = windows::Win32::UI::WindowsAndMessaging::SetMenu(HWND(self.hwnd as _), Some(m));
            DragAcceptFiles(HWND(self.hwnd as _), true);
        }
        self.waker = Some(ctl.waker());
        let waker = ctl.waker();
        self.ipc = Notify::start(Channel::Editor, move |m| waker.send(Ev::Ipc(m)));
        self.start_library_watcher();
        self.tray.reload();
        self.set_tool(Tool::Arrow);
        self.command_line(ctl, &instance::command_line(), true);
        self.show_editor(ctl);
    }

    fn ui(&mut self, ctl: &mut Ctl, win: WinKey, ui: &mut egui::Ui) {
        if Some(win) == self.main {
            self.main_ui(ctl, ui);
        } else {
            self.aux_ui(ctl, win, ui);
        }
    }

    fn message(&mut self, ctl: &mut Ctl, m: RawMsg) {
        match m.msg {
            WM_COPYDATA => {
                if let Some(t) = m.text {
                    self.command_line(ctl, &t, false);
                }
            }
            WM_COMMAND => {
                let id = (m.wparam & 0xFFFF) as u32;
                if m.hwnd == self.hwnd {
                    self.command(ctl, id);
                }
            }
            WM_DROPFILES => {
                for f in m.text.unwrap_or_default().lines() {
                    self.open_path(ctl, f);
                }
            }
            _ => {}
        }
        if let Some(k) = self.main {
            ctl.repaint(k);
        }
    }

    fn close_requested(&mut self, ctl: &mut Ctl, win: WinKey) {
        if Some(win) == self.main {
            self.close(ctl);
        } else {
            self.aux_close(ctl, win);
        }
    }

    fn event(&mut self, ctl: &mut Ctl, ev: Box<dyn std::any::Any + Send>) {
        let Ok(ev) = ev.downcast::<Ev>() else { return };
        match *ev {
            Ev::Ipc(m) => self.on_ipc(ctl, m),
            Ev::Do(job) => job(self, ctl),
            Ev::LibChanged => self.lib_reload_at = Some(Instant::now() + Duration::from_millis(400)),
        }
        if let Some(k) = self.main {
            ctl.repaint(k);
        }
    }
}

/// The Editor's registry values (HKCU\Software\Snapnab\Editor).
pub fn reg_dw(name: &str, def: u32) -> u32 {
    reg::get_dword(OsStr::new(name), def, Some(OsStr::new("Editor")))
}

pub fn set_reg_dw(name: &str, v: u32) {
    reg::set_dword(OsStr::new(name), v, Some(OsStr::new("Editor")));
}

pub fn reg_str(name: &str, def: &str) -> String {
    reg::get_string(OsStr::new(name), Some(OsStr::new("Editor"))).map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| def.to_owned())
}

pub fn set_reg_str(name: &str, v: &str) {
    reg::set_string(OsStr::new(name), OsStr::new(v), Some(OsStr::new("Editor")));
}

pub fn run() {
    instance::wait_for_previous_instance(ipc::EDITOR_WND_CLASS);
    let Some(mutex) = instance::claim_or_forward(ipc::EDITOR_MUTEX, ipc::EDITOR_WND_CLASS) else {
        // forwarded: ends right away (third-party overlays crashed in the DLL unload notifications)
        unsafe {
            let _ = windows::Win32::System::Threading::TerminateProcess(windows::Win32::System::Threading::GetCurrentProcess(), 0);
        }
        return;
    };
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
    }
    log::init("SnapnabEditor", VERSION);
    std::panic::set_hook(Box::new(|info| log::write(&format!("PANIC: {info}"))));
    snapcore::i18n::init();
    snapcore::gdip::startup();
    if !ipc::mutex_exists(&snapcore::wide::to_wide(ipc::CAPTURE_MUTEX)) {
        instance::launch_sibling("Snapnab.exe", "/hide", true);
    }
    if let Err(e) = snapui::host::run(Editor::new(), |_| {}) {
        log::write(&format!("Event loop: {e}"));
    }
    log::write("SnapnabEditor exited");
    drop(mutex);
}
