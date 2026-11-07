//! OnCommand and ImageCommand (main.cpp): every menu, toolbar and shortcut command, with the same IDs.

use snapcore::doc::{ObjType, PointF};
use snapcore::fx;
use snapcore::image::{Image, cr, cg, cb, ca};
use snapedit_app::canvas::{TOOL_COUNT, Tool};
use snapedit_app::effects::{apply_fx, apply_quick, keeps_objects};
use snapedit_app::ids::*;
use snapedit_app::menu::TOOLS;
use snapedit_app::native::{self, Item, Menu};
use snapedit_app::{panels, tr};
use snapui::form::{self, Field};
use snapui::host::Ctl;
use std::sync::Arc;

use super::docs::fmt;
use super::{Editor, set_reg_dw};

/// Internal commands (posted to the window so their popup menus open outside the frame drawing).
pub const MORE_TOOLS: u32 = 0xF001;
pub const CONTEXT_MENU: u32 = 0xF002;
const SHARE_MENU: u32 = 0xF003;

fn yes(v: bool) -> u32 {
    v as u32
}

impl Editor {
    pub fn command(&mut self, ctl: &mut Ctl, id: u32) {
        if (T_TOOL_BASE..T_TOOL_BASE + TOOL_COUNT as u32).contains(&id) {
            if let Some(t) = Tool::from_index((id - T_TOOL_BASE) as usize) {
                self.set_tool(t);
            }
            return;
        }
        if (M_OFFICE_BASE..M_OFFICE_BASE + 4).contains(&id) {
            self.send_to_office((id - M_OFFICE_BASE) as i32);
            return;
        }
        if (M_CURSOR_BASE..M_CURSOR_BASE + 16).contains(&id) {
            self.insert_cursor((id - M_CURSOR_BASE) as usize);
            return;
        }
        if (M_MORE_TOOL_BASE..M_MORE_TOOL_BASE + TOOL_COUNT as u32).contains(&id) {
            if let Some(t) = Tool::from_index((id - M_MORE_TOOL_BASE) as usize) {
                self.set_tool(t);
            }
            return;
        }
        if (B_PLAY..=B_PLAYER).contains(&id) {
            self.video_command(ctl, id);
            return;
        }
        match id {
            MORE_TOOLS => self.more_tools_menu(ctl),
            CONTEXT_MENU => self.context_menu(ctl),
            SHARE_MENU => self.share_menu(ctl),
            super::extras::COPY_AS_MENU => self.copy_as_menu(),
            M_TB_CUSTOMIZE => self.customize_toolbar(ctl),
            M_TB_RESET => {
                self.tb_custom = false;
                self.save_toolbar_config();
            }
            M_SHOW_EFFECTS => {
                self.show_library(false);
                self.set_right_tab(if self.right_tab == 1 { 0 } else { 1 });
            }
            M_SHOW_LAYERS => {
                self.show_library(false);
                self.set_right_tab(if self.right_tab == 2 { 0 } else { 2 });
            }
            M_SHOW_LIBRARY => self.show_library(!self.library_open),
            T_CAPTURE | M_NEW_CAPTURE => self.request_capture(0),
            T_RECORD | M_NEW_VIDEO => self.request_capture(1),
            T_COPYALL | M_COPYALL => {
                if self.require_image()
                    && let Some(d) = self.cv.doc()
                    && snapcore::clip::copy_image(self.hwnd, &d.render(None), super::VERSION)
                {
                    self.status = tr("Imagem inteira copiada para a área de transferência.");
                }
            }
            T_SAVE | M_SAVEAS => self.save_as(ctl),
            T_SHARE => native::post_command(self.hwnd, SHARE_MENU),
            M_NEW_CLIPBOARD => self.new_from_clipboard(ctl),
            M_NEW_IMAGE => self.new_blank_image(),
            M_OPEN => self.modal(
                |o| native::open_file(o, &tr("Abrir"), &(tr("Imagens e capturas") + "|*.snap;*.png;*.jpg;*.jpeg;*.bmp;*.gif;*.tif;*.tiff;*.webp;*.avif;*.heic;*.mp4|" + &tr("Todos os arquivos") + "|*.*|")),
                |e, c, p| {
                    if let Some(p) = p {
                        e.open_path(c, &p);
                    }
                },
            ),
            M_SAVE => self.save_current(ctl),
            M_DUPLICATE => self.duplicate_current(),
            M_DELETE => {
                if let Some(d) = self.cv.doc() {
                    let p = d.path.clone();
                    self.delete_docs(vec![p]);
                }
            }
            M_PRINT => self.print_current(),
            M_EMAIL => self.send_email(),
            M_OPEN_PROGRAM => self.open_in_program(),
            M_LIBRARY => snapedit_app::shell_open(&snapedit_app::files::library_dir()),
            M_CLOSE => self.close_current(),
            M_EXIT => self.close(ctl),
            M_UNDO | M_REDO | M_UNDOALL => {
                if self.cv.doc().is_none_or(|d| d.is_video) {
                    return;
                }
                self.cv.commit_text();
                if let Some(d) = self.cv.doc_mut() {
                    match id {
                        M_UNDO => d.undo(),
                        M_REDO => d.redo(),
                        _ => d.undo_all(),
                    }
                }
                self.cv.selected = -1;
                self.cv.refresh(true);
                if id != M_REDO {
                    self.cv.fit_to_window();
                }
                self.doc_changed();
                self.props.reload();
            }
            M_CUT => {
                self.copy_to_clipboard(true);
            }
            M_COPY => {
                if self.copy_to_clipboard(false) {
                    self.status = tr("Copiado.");
                }
            }
            M_PASTE => self.paste(false),
            M_PASTE_INPLACE => self.paste(true),
            M_DIST_H | M_DIST_V => {
                if self.require_image() {
                    self.cv.distribute_selected(id == M_DIST_H);
                }
            }
            M_LOCK | M_UNLOCK => {
                if !self.require_image() {
                    return;
                }
                let v = self.cv.selected_list();
                if v.is_empty() {
                    self.status = tr("Selecione um objeto para bloquear ou desbloquear.");
                    return;
                }
                let all = self.cv.doc().is_some_and(|d| v.iter().all(|&i| d.st.objs[i].locked));
                let lock = id == M_LOCK && !all;
                self.cv.set_locked(&v, lock);
                self.status = if lock {
                    tr("Objeto bloqueado: não pode ser movido, redimensionado nem excluído.")
                } else {
                    tr("Objeto desbloqueado.")
                };
            }
            M_LIFT => {
                if !self.require_image() {
                    return;
                }
                if !self.cv.has_raster_selection() {
                    self.status = tr("Selecione uma área com a ferramenta Seleção para levantá-la.");
                } else if self.cv.lift_selection() {
                    self.status = tr("Peça solta: ajuste o resto da imagem (cortar, recortar faixa…), posicione a peça e tecle Enter para soltá-la.");
                }
            }
            M_DROP => {
                if self.require_image() && self.cv.drop_floating() {
                    self.status = tr("Peça solta na imagem.");
                }
            }
            M_RULERS => {
                self.cv.rulers = !self.cv.rulers;
                set_reg_dw("Rulers", yes(self.cv.rulers));
                if !self.cv.rulers {
                    self.coords.clear();
                }
                self.cv.refresh(false);
                if self.cv.zoom > 0.0 && self.cv.doc().is_some_and(|d| !d.is_video) {
                    let z = self.cv.zoom;
                    self.cv.zoom_to(z, None);
                }
                self.status = if self.cv.rulers {
                    tr("Réguas ligadas: arraste de uma régua para criar uma guia; arraste a guia de volta para removê-la.")
                } else {
                    tr("Réguas desligadas.")
                };
            }
            M_CLEAR_GUIDES => {
                if let Some(d) = self.cv.doc_mut()
                    && !d.is_video
                    && (!d.guides_x.is_empty() || !d.guides_y.is_empty())
                {
                    d.guides_x.clear();
                    d.guides_y.clear();
                    d.modified = true;
                    self.cv.refresh(false);
                }
            }
            M_EDGESNAP => {
                self.cv.edge_snap = !self.cv.edge_snap;
                set_reg_dw("EdgeSnap", yes(self.cv.edge_snap));
                self.status = if self.cv.edge_snap {
                    tr("A seleção e o corte encaixam nas bordas da imagem (Alt desliga na hora).")
                } else {
                    tr("Encaixe nas bordas desligado.")
                };
            }
            M_DRAWHOLD => {
                self.cv.draw_hold = !self.cv.draw_hold;
                set_reg_dw("DrawHold", yes(self.cv.draw_hold));
                self.status = if self.cv.draw_hold {
                    tr("Caneta: desenhe uma linha, seta, retângulo, elipse ou triângulo e segure parado para virar a forma.")
                } else {
                    tr("Desenhar e segurar desligado.")
                };
            }
            M_PLAINPASTE => {
                self.cv.plain_paste = !self.cv.plain_paste;
                set_reg_dw("PlainTextPaste", yes(self.cv.plain_paste));
                self.status = if self.cv.plain_paste {
                    tr("Texto colado sem formatação (Ctrl+Shift+V mantém a formatação).")
                } else {
                    tr("Texto colado com a formatação de origem (Ctrl+Shift+V cola sem formatação).")
                };
            }
            M_RENAME_OBJ => {
                if !self.require_image() || self.cv.selected < 0 {
                    return;
                }
                let i = self.cv.selected as usize;
                let name = self.cv.doc().map(|d| d.st.objs[i].name.clone()).unwrap_or_default();
                self.modal(
                    move |o| {
                        let mut f = vec![Field::Text { label: tr("Nome"), text: name }];
                        form::run(o, &tr("Renomear objeto"), &mut f).then(|| f[0].text().trim().to_owned())
                    },
                    move |e, _, r| {
                        if let Some(n) = r {
                            e.cv.rename_object(i, &n);
                        }
                    },
                );
            }
            M_OPEN_LINK => {
                if let Some(d) = self.cv.doc()
                    && self.cv.selected >= 0
                    && let Some(o) = d.st.objs.get(self.cv.selected as usize)
                    && o.kind == ObjType::Link
                {
                    let u = String::from_utf16_lossy(&o.text);
                    if u.starts_with("http://") || u.starts_with("https://") || u.starts_with("mailto:") {
                        snapedit_app::shell_open(&u);
                    }
                }
            }
            M_DELETE_SEL => {
                if self.require_image() {
                    self.cv.delete_selected();
                }
            }
            M_SELECTALL => {
                if self.library_open {
                    self.library.select_all();
                } else if self.cv.doc().is_some_and(|d| !d.is_video && !d.st.objs.is_empty())
                    && self.cv.tool == Tool::Select
                    && !self.cv.smart_move
                {
                    self.cv.select_all_objects(); // with Move, Ctrl+A selects all the objects
                } else if self.require_image() {
                    self.tb_active = Tool::Selection;
                    self.cv.select_all();
                }
            }
            M_FLATTEN => {
                if self.require_image() {
                    self.cv.flatten_all();
                }
            }
            M_FLATTEN_SEL => {
                if self.require_image() {
                    self.cv.flatten_selected();
                }
            }
            M_FRONT => self.cv.bring_to_front(),
            M_BACK => self.cv.send_to_back(),
            M_BRING_FORWARD => self.cv.bring_forward(),
            M_SEND_BACKWARD => self.cv.send_backward(),
            M_ALIGN_LEFT..=M_ALIGN_BOTTOM => {
                if self.require_image() {
                    self.cv.align_selected((id - M_ALIGN_LEFT) as i32);
                }
            }
            M_FLIP_H | M_FLIP_V => {
                // with an object selected, flips the object; without one, the whole image
                if self.require_image() && !self.cv.flip_selected(id == M_FLIP_H) {
                    self.image_command(ctl, id);
                }
            }
            M_GROUP => {
                if !self.require_image() {
                    return;
                }
                if self.cv.selection_count() < 2 {
                    self.status = tr("Selecione dois ou mais objetos (Ctrl+clique ou arraste um retângulo com o Mover) para agrupar.");
                    return;
                }
                self.cv.group_selected();
                self.status = tr("Objetos agrupados. Imagem › Desagrupar (Ctrl+Shift+U) separa.");
            }
            M_UNGROUP => {
                if self.require_image() && self.cv.can_ungroup() {
                    self.cv.ungroup_selected();
                    self.status = tr("Grupo desfeito.");
                }
            }
            M_APPLY_TEMPLATE => self.combine_now(Vec::new(), true),
            M_COMBINE => {
                let s = self.library_selection();
                let enough = s.len() >= 2;
                self.combine_now(if enough { s } else { Vec::new() }, !enough);
            }
            M_ADD_CAPTURE => {
                if !self.require_image() {
                    return;
                }
                self.add_capture_to = self.cv.doc().map(|d| (d.path.clone(), std::time::Instant::now()));
                self.status = tr("Faça a captura: ela será adicionada abaixo desta imagem.");
                self.request_capture(0);
            }
            M_READ_QR => self.read_qr(),
            M_BEAUTIFY => self.beautify_current(),
            M_REDACT => self.redact_sensitive(),
            M_REMOVE_BG => self.remove_background_now(),
            M_SPLIT => self.split_current(),
            M_DATESTAMP => self.insert_date_stamp(),
            M_DATESTAMP_FMT => self.date_stamp_format(),
            M_CONTRAST => self.contrast_now(ctl),
            M_ASSISTANT => self.assistant(ctl),
            M_AI_SETTINGS => self.assistant_settings(),
            M_UPLOAD_SETTINGS => {
                std::thread::spawn({
                    let o = self.hwnd;
                    move || {
                        snapui::upload_ui::configure_now(o);
                    }
                });
            }
            M_COMPARE => {
                let s = self.library_selection();
                self.compare_now(ctl, s);
            }
            M_BATCH => {
                let s = self.library_selection();
                let enough = s.len() >= 2;
                self.run_batch(if enough { s } else { Vec::new() });
            }
            M_PIN => self.pin_current(),
            M_UPLOAD => self.upload_current(),
            M_COPYAS => {
                if let Some(d) = self.cv.doc() {
                    let (p, t) = (d.path.clone(), d.title.clone());
                    let img = if d.is_video { Image::default() } else { d.render(None) };
                    self.cv.commit_text();
                    self.copy_as(p, img, t);
                }
            }
            M_SNAPPING => {
                self.cv.snapping = !self.cv.snapping;
                set_reg_dw("CanvasSnapping", yes(self.cv.snapping));
                self.status = if self.cv.snapping {
                    tr("Encaixe no canvas ligado (Alt ao arrastar desliga na hora).")
                } else {
                    tr("Encaixe no canvas desligado.")
                };
            }
            M_TAG => {
                if !self.require_image() {
                    return;
                }
                let tags = self.cv.doc().map(|d| d.tags.clone()).unwrap_or_default();
                self.modal(
                    move |o| {
                        let mut f = vec![
                            Field::Text { label: tr("Etiquetas"), text: tags },
                            Field::Label(tr("Separe as etiquetas por vírgula. Elas ficam gravadas na captura.")),
                        ];
                        form::run(o, &tr("Etiquetar imagem"), &mut f).then(|| f[0].text().trim().to_owned())
                    },
                    |e, _, r| {
                        let Some(t) = r else { return };
                        if let Some(c) = e.cv.cur
                            && e.cv.docs[c].tags != t
                        {
                            e.cv.docs[c].tags = t.clone();
                            e.cv.docs[c].modified = true;
                            e.autosave(c);
                            e.update_doc_info();
                        }
                        e.status = if t.is_empty() { tr("Etiquetas removidas.") } else { tr("Etiquetas: ") + &t };
                    },
                );
            }
            M_OCR => self.grab_text(ctl),
            M_ZOOM_IN => self.cv.zoom_in(),
            M_ZOOM_OUT => self.cv.zoom_out(),
            M_ZOOM_100 => self.cv.zoom_to(1.0, None),
            M_ZOOM_FIT => self.cv.fit_to_window(),
            M_BG_COLOR => {
                let cur = self.cv.background;
                self.modal(move |o| native::choose_color(o, cur), |e, _, c| {
                    if let Some(c) = c {
                        e.cv.background = c;
                        set_reg_dw("CanvasBackground", c);
                        e.cv.refresh(false);
                    }
                });
            }
            M_SHOW_TRAY => {
                self.show_tray = !self.show_tray;
                set_reg_dw("ShowTray", yes(self.show_tray));
            }
            M_SHORTCUTS => self.show_shortcuts(ctl),
            M_MANUAL => self.show_manual(),
            M_ABOUT => {
                let text = format!(
                    "Snapnab Editor {}\nCopyright © 2025–2026 Snapnab\n\n{}\n\n{}\n\n{}{}",
                    super::VERSION,
                    tr("Edição de capturas com objetos vetoriais, efeitos, Smart Move,\nOCR, exportação PDF e edição de vídeo (cortes e GIF)."),
                    tr("Software livre sob a GNU General Public License versão 3."),
                    tr("Biblioteca: "),
                    snapedit_app::files::library_dir()
                );
                self.inform(text, tr("Sobre"));
            }
            _ => self.image_command(ctl, id),
        }
    }

    // ---------------------------------------------------------------- image operations
    /// Moves every point of every object (rotation, flip, resize, canvas, trim).
    pub fn transform_objects(&mut self, f: &dyn Fn(PointF) -> PointF) {
        let Some(d) = self.cv.doc_mut() else { return };
        for i in 0..d.st.objs.len() {
            if let Some(o) = d.obj_mut(i) {
                o.map_points(f);
                o.update_group_rect();
            }
        }
    }

    pub fn flatten_if_needed(&mut self) {
        let Some(d) = self.cv.doc_mut() else { return };
        if !d.st.objs.is_empty() {
            let r = d.render(None);
            d.set_base(r);
            d.st.objs.clear();
            self.cv.selected = -1;
        }
    }

    /// After an image command: the selection kept valid, fit, redraw, the panels told.
    pub fn after_image_change(&mut self) {
        let n = self.cv.doc().map_or(0, |d| d.st.objs.len() as i32);
        self.cv.selected = self.cv.selected.min(n - 1);
        self.cv.fit_to_window();
        self.cv.refresh(true);
        self.doc_changed();
        self.props.reload();
    }

    fn geometry(&mut self, cmd: u32) {
        let Some(d) = self.cv.doc_mut() else { return };
        let b = d.st.base.clone();
        let (w, h) = (b.w as f32, b.h as f32);
        d.checkpoint();
        let (img, f): (Image, Box<dyn Fn(PointF) -> PointF>) = match cmd {
            M_ROT_CW => (fx::rotate90(&b, true), Box::new(move |p: PointF| PointF { X: h - p.Y, Y: p.X })),
            M_ROT_CCW => (fx::rotate90(&b, false), Box::new(move |p: PointF| PointF { X: p.Y, Y: w - p.X })),
            M_ROT_180 => (fx::rotate180(&b), Box::new(move |p: PointF| PointF { X: w - p.X, Y: h - p.Y })),
            M_FLIP_H => (fx::flip_h(&b), Box::new(move |p: PointF| PointF { X: w - p.X, Y: p.Y })),
            _ => (fx::flip_v(&b), Box::new(move |p: PointF| PointF { X: p.X, Y: h - p.Y })),
        };
        d.set_base(img);
        self.transform_objects(&*f);
        self.cv.fit_to_window();
        self.cv.refresh(true);
        self.doc_changed();
    }

    /// Image › Effects and the toolbar effect items: the effect's settings open in the panel, with a live preview.
    pub fn open_effect(&mut self, cmd: u32) {
        if !self.require_image() {
            return;
        }
        let (target, filter) = match cmd {
            M_FX_GRAY => (M_FX_FILTER, 0),
            M_FX_SEPIA => (M_FX_FILTER, 1),
            M_FX_INVERT => (M_FX_FILTER, 2),
            M_FX_MONO => (M_FX_FILTER, 3),
            M_FX_BC | M_FX_HS => (M_FX_ADJUST, -1),
            c => (c, -1),
        };
        if filter >= 0
            && let Some(e) = self.fx.find_mut(M_FX_FILTER)
            && let Some(p) = e.params.first_mut()
        {
            p.set_value(filter);
        }
        self.set_right_tab(1);
        self.fx.expand(target);
        self.preview_effect(target);
    }

    pub fn preview_effect(&mut self, cmd: u32) {
        let Some(d) = self.cv.doc().filter(|d| !d.is_video) else { return };
        let Some(e) = self.fx.find(cmd) else { return };
        let src = d.render(None);
        let sel = self.cv.has_raster_selection().then(|| self.cv.raster_selection_bounds());
        let (img, _) = apply_fx(cmd, &e.params, &src, 1.0, sel, d.w(), d.h());
        self.cv.set_effect_preview(Some(Arc::new(img)));
        if cmd == M_FX_SPOTLIGHT && sel.is_none() {
            self.status = tr("Spotlight: sem seleção, o foco é o meio da imagem. Use a ferramenta Seleção para escolher a área.");
        }
    }

    /// CommitEffect: one undo step; the objects are merged unless the effect keeps them (border, shadow, caption).
    pub fn commit_effect(&mut self, name: &str, keep_objects: bool, run: impl FnOnce(&Image) -> (Image, (i32, i32))) {
        if !self.require_image() {
            return;
        }
        if let Some(d) = self.cv.doc_mut() {
            d.checkpoint();
        }
        if !keep_objects {
            self.flatten_if_needed();
        }
        let Some(d) = self.cv.doc_mut() else { return };
        let (img, off) = run(&d.st.base.clone());
        d.set_base(img);
        if off != (0, 0) {
            let (ox, oy) = (off.0 as f32, off.1 as f32);
            self.transform_objects(&move |p: PointF| PointF { X: p.X + ox, Y: p.Y + oy });
        }
        self.cv.set_effect_preview(None);
        self.after_image_change();
        self.status = tr("Efeito aplicado: ") + name + &tr(" (Ctrl+Z desfaz)");
    }

    pub fn apply_effect(&mut self, _ctl: &mut Ctl, cmd: u32) {
        let Some(e) = self.fx.find(cmd) else { return };
        let (name, params) = (e.name.clone(), e.params.clone());
        let (w, h) = self.cv.doc().map_or((0, 0), |d| (d.w(), d.h()));
        let sel = self.cv.has_raster_selection().then(|| self.cv.raster_selection_bounds());
        self.commit_effect(&name, keeps_objects(cmd), |base| apply_fx(cmd, &params, base, 1.0, sel, w, h));
        self.fx.collapse();
    }

    pub fn apply_quick_effect(&mut self, _ctl: &mut Ctl, i: usize) {
        let Some(q) = self.fx.quick.get(i).cloned() else { return };
        self.commit_effect(&tr(&q.name), q.keeps_objects(), |base| apply_quick(&q, base, 1.0));
    }

    /// "Add to quick effects": the current Border, Edge effects, Shadow and Page curl settings.
    pub fn add_quick_effect(&mut self) {
        let n = self.fx.quick.len() + 1;
        self.modal(
            move |o| {
                let mut f = vec![
                    Field::Text { label: tr("Nome"), text: tr("Meu efeito %zu").replace("%zu", &n.to_string()) },
                    Field::Label(tr("Os ajustes vêm dos efeitos do painel (abra cada um para mudar):")),
                    Field::Check { label: tr("Borda"), value: false },
                    Field::Check { label: tr("Bordas especiais"), value: false },
                    Field::Check { label: tr("Sombra"), value: true },
                    Field::Check { label: tr("Dobra de página"), value: false },
                ];
                (form::run(o, &tr("Adicionar aos efeitos rápidos"), &mut f) && !f[0].text().trim().is_empty()).then_some(f)
            },
            |e, _, f| {
                let Some(f) = f else { return };
                let q = snapedit_app::effects::quick_from_panel(&e.fx.effects, f[0].text().trim().to_owned(), f[2].on(), f[3].on(), f[4].on(),
                                                                f[5].on());
                if !(q.border || q.edge || q.shadow || q.curl) {
                    return;
                }
                e.status = tr("Efeito rápido adicionado: ") + &q.name;
                e.fx.quick.push(q);
                snapedit_app::effects::save_quick_effects(&e.fx.quick);
                e.fx.invalidate_thumbs();
            },
        );
    }

    pub fn image_command(&mut self, ctl: &mut Ctl, cmd: u32) {
        if !self.require_image() {
            return;
        }
        let Some(d) = self.cv.doc() else { return };
        let (dw, dh) = (d.w(), d.h());
        match cmd {
            M_ROT_CW | M_ROT_CCW | M_ROT_180 | M_FLIP_H | M_FLIP_V => {
                self.geometry(cmd);
                return;
            }
            M_CROP_SEL => {
                if self.cv.has_raster_selection() || self.cv.has_crop() {
                    self.cv.crop_to_selection();
                } else {
                    self.inform(tr("Faça uma seleção (ferramenta Seleção) ou um recorte primeiro."), "Snapnab Editor");
                }
                return;
            }
            M_ROT_FREE => {
                self.modal(
                    |o| {
                        let mut f = vec![
                            Field::Int { label: tr("Ângulo (graus, horário)"), value: 15, min: -359, max: 359 },
                            Field::Color { label: tr("Cor de fundo"), color: 0xFFFFFF },
                            Field::Check { label: tr("Fundo transparente"), value: true },
                            Field::Label(tr("Os objetos serão mesclados à imagem.")),
                        ];
                        form::run(o, &tr("Girar"), &mut f).then_some(f)
                    },
                    |e, _, f| {
                        let Some(f) = f else { return };
                        if let Some(d) = e.cv.doc_mut() {
                            d.checkpoint();
                        }
                        e.flatten_if_needed();
                        let bg = if f[2].on() { 0 } else { snapcore::image::from_colorref(f[1].color()) };
                        if let Some(d) = e.cv.doc_mut() {
                            let img = fx::rotate_angle(&d.st.base, f[0].int() as f64, bg);
                            d.set_base(img);
                        }
                        e.after_image_change();
                    },
                );
                return;
            }
            M_RESIZE => {
                self.modal(
                    move |o| {
                        let mut f = vec![
                            Field::Int { label: tr("Largura (px)"), value: dw, min: 1, max: 20000 },
                            Field::Int { label: tr("Altura (px)"), value: dh, min: 1, max: 20000 },
                            Field::Int { label: tr("ou escala (%)"), value: 100, min: 1, max: 1000 },
                            Field::Check { label: tr("Manter proporção"), value: true },
                        ];
                        form::run(o, &tr("Redimensionar imagem"), &mut f).then_some(f)
                    },
                    |e, _, f| {
                        let Some(f) = f else { return };
                        let Some(d) = e.cv.doc_mut() else { return };
                        let (w, h) = (d.w(), d.h());
                        let (mut nw, mut nh) = (f[0].int(), f[1].int());
                        if f[2].int() != 100 {
                            nw = (w * f[2].int() / 100).max(1);
                            nh = (h * f[2].int() / 100).max(1);
                        } else if f[3].on() {
                            if nw != w {
                                nh = ((h as f64 * nw as f64 / w as f64).round() as i32).max(1);
                            } else if nh != h {
                                nw = ((w as f64 * nh as f64 / h as f64).round() as i32).max(1);
                            }
                        }
                        let (sx, sy) = (nw as f32 / w as f32, nh as f32 / h as f32);
                        d.checkpoint();
                        let img = snapcore::gdip::scale_image(&d.st.base, nw, nh, true);
                        d.set_base(img);
                        e.transform_objects(&move |p: PointF| PointF { X: p.X * sx, Y: p.Y * sy });
                        let s = (sx * sy).sqrt();
                        if let Some(d) = e.cv.doc_mut() {
                            for i in 0..d.st.objs.len() {
                                if let Some(o) = d.obj_mut(i) {
                                    o.style.width *= s;
                                    o.style.font_size = (o.style.font_size * s).max(6.0);
                                }
                            }
                        }
                        e.after_image_change();
                    },
                );
                return;
            }
            M_CANVAS => {
                self.modal(
                    move |o| {
                        let anchors = ["Superior esquerda", "Superior", "Superior direita", "Esquerda", "Centro", "Direita", "Inferior esquerda",
                                       "Inferior", "Inferior direita"];
                        let mut f = vec![
                            Field::Int { label: tr("Largura (px)"), value: dw, min: 1, max: 20000 },
                            Field::Int { label: tr("Altura (px)"), value: dh, min: 1, max: 20000 },
                            Field::Combo { label: tr("Âncora"), items: anchors.iter().map(|a| tr(a)).collect(), value: 4 },
                            Field::Color { label: tr("Cor do canvas"), color: 0xFFFFFF },
                            Field::Check { label: tr("Transparente"), value: false },
                        ];
                        form::run(o, &tr("Tamanho do canvas"), &mut f).then_some(f)
                    },
                    |e, _, f| {
                        let Some(f) = f else { return };
                        let Some(d) = e.cv.doc_mut() else { return };
                        let (nw, nh, a) = (f[0].int(), f[1].int(), f[2].int());
                        let ox = match a % 3 { 0 => 0, 1 => (nw - d.w()) / 2, _ => nw - d.w() } as f32;
                        let oy = match a / 3 { 0 => 0, 1 => (nh - d.h()) / 2, _ => nh - d.h() } as f32;
                        d.checkpoint();
                        let bg = if f[4].on() { 0 } else { snapcore::image::from_colorref(f[3].color()) };
                        let img = fx::canvas_resize(&d.st.base, nw, nh, a, bg);
                        d.set_base(img);
                        e.transform_objects(&move |p: PointF| PointF { X: p.X + ox, Y: p.Y + oy });
                        e.after_image_change();
                    },
                );
                return;
            }
            M_CANVAS_COLOR => {
                // the color behind the transparent areas of the image
                self.modal(|o| native::choose_color(o, 0xFFFFFF), |e, _, c| {
                    let Some(c) = c else { return };
                    let Some(d) = e.cv.doc_mut() else { return };
                    d.checkpoint();
                    let b = d.st.base.clone();
                    let mut out = Image::new(b.w, b.h, snapcore::image::from_colorref(c) | 0xFF00_0000);
                    out.blit(&b, 0, 0, true);
                    d.set_base(out);
                    e.after_image_change();
                });
                return;
            }
            M_TRIM => {
                let Some(d) = self.cv.doc_mut() else { return };
                let b = d.st.base.clone();
                let t = fx::auto_trim(&b, 8);
                if t.w == b.w && t.h == b.h {
                    self.status = tr("Nada a cortar: as bordas não são uniformes.");
                    return;
                }
                // the crop offset, to move the objects
                let refc = b.at(0, 0);
                let same = |p: u32| {
                    (cr(p) - cr(refc)).abs() <= 8 && (cg(p) - cg(refc)).abs() <= 8 && (cb(p) - cb(refc)).abs() <= 8 && (ca(p) - ca(refc)).abs() <= 8
                };
                let (bw, bh) = (b.wi(), b.hi());
                let mut oy = 0;
                while oy < bh - 1 && (0..bw).all(|x| same(b.at(x, oy))) {
                    oy += 1;
                }
                let mut ox = 0;
                while ox < bw - 1 && (oy..(oy + t.hi()).min(bh)).all(|y| same(b.at(ox, y))) {
                    ox += 1;
                }
                d.checkpoint();
                d.set_base(t);
                let (ox, oy) = (ox as f32, oy as f32);
                self.transform_objects(&move |p: PointF| PointF { X: p.X - ox, Y: p.Y - oy });
            }
            c if EFFECTS.contains(&c) || matches!(c, M_FX_GRAY | M_FX_INVERT | M_FX_SEPIA | M_FX_MONO | M_FX_BC | M_FX_HS) => {
                self.open_effect(c);
                return;
            }
            _ => return,
        }
        let _ = ctl;
        self.after_image_change();
    }

    // ---------------------------------------------------------------- popup menus
    fn share_menu(&mut self, ctl: &mut Ctl) {
        let m = Menu::popup(vec![
            Item::Cmd(M_SAVEAS, tr("Arquivo…")),
            Item::Cmd(M_COPYALL, tr("Área de transferência")),
            Item::Cmd(M_COPYAS, tr("Copiar como…")),
            Item::Cmd(M_EMAIL, tr("E-mail…")),
            Item::Cmd(M_PRINT, tr("Impressora…")),
            Item::Cmd(M_OPEN_PROGRAM, tr("Programa externo…")),
            Item::Separator,
            Item::Cmd(M_PIN, tr("Fixar na tela")),
            Item::Cmd(M_UPLOAD, tr("Enviar (upload)")),
            Item::Sub(tr("Enviar para"), snapedit_app::menu::office_menu()),
        ]);
        let (x, y) = native::cursor_pos();
        let c = m.choose(self.hwnd, x, y);
        if c != 0 {
            self.command(ctl, c);
        }
    }

    /// "More" on the toolbar: the tools that are not on it, and the customization.
    fn more_tools_menu(&mut self, ctl: &mut Ctl) {
        let vis = self.center_entries();
        let mut items: Vec<Item> = TOOLS
            .iter()
            .filter(|(t, _)| !vis.iter().any(|e| e.kind == panels::EntryKind::Tool && e.id as usize == t.index()))
            .map(|(t, _)| Item::Checked(M_MORE_TOOL_BASE + t.index() as u32, t.name(), *t == self.tb_active))
            .collect();
        items.push(Item::Separator);
        items.push(Item::Cmd(M_TB_CUSTOMIZE, tr("Personalizar a barra…")));
        items.push(Item::Cmd2(M_TB_RESET, tr("Voltar ao padrão (automática)"), self.tb_custom));
        let (x, y) = native::cursor_pos();
        let c = Menu::popup(items).choose(self.hwnd, x, y);
        if c != 0 {
            self.command(ctl, c);
        }
    }

    /// The canvas context menu: commands for the selected objects, or paste / selection with none.
    fn context_menu(&mut self, ctl: &mut Ctl) {
        let Some(d) = self.cv.doc().filter(|d| !d.is_video) else { return };
        let can_paste = super::clipboard::can_paste();
        let v = self.cv.selected_list();
        let mut m = Vec::new();
        if !v.is_empty() && self.cv.selected >= 0 {
            let o = &d.st.objs[self.cv.selected as usize];
            let locked = self.cv.selection_locked();
            if o.kind == ObjType::Link {
                m.push(Item::Cmd(M_OPEN_LINK, tr("Abrir o link")));
                m.push(Item::Separator);
            }
            if o.floating {
                m.push(Item::Cmd(M_DROP, tr("Soltar a peça na imagem\tEnter")));
                m.push(Item::Separator);
            }
            m.push(Item::Cmd2(M_CUT, tr("Recor&tar\tCtrl+X"), !locked));
            m.push(Item::Cmd(M_COPY, tr("&Copiar\tCtrl+C")));
            m.push(Item::Cmd2(M_PASTE, tr("C&olar\tCtrl+V"), can_paste));
            m.push(Item::Cmd2(M_PASTE_INPLACE, tr("Colar no &mesmo lugar\tCtrl+Shift+V"), can_paste));
            m.push(Item::Cmd2(M_DELETE_SEL, tr("E&xcluir\tDel"), !locked));
            m.push(Item::Separator);
            m.push(if locked { Item::Cmd(M_UNLOCK, tr("Desblo&quear\tCtrl+Shift+K")) } else { Item::Cmd(M_LOCK, tr("&Bloquear\tCtrl+L")) });
            if v.len() == 1 {
                m.push(Item::Cmd(M_RENAME_OBJ, tr("Re&nomear…")));
            }
            m.push(Item::Separator);
            m.push(Item::Cmd(M_FRONT, tr("Trazer para o &primeiro plano")));
            m.push(Item::Cmd(M_BRING_FORWARD, tr("Trazer para a &frente\tCtrl+Shift+F")));
            m.push(Item::Cmd(M_SEND_BACKWARD, tr("Enviar para &trás\tCtrl+Shift+B")));
            m.push(Item::Cmd(M_BACK, tr("Enviar para o plano de &fundo")));
            if v.len() >= 2 {
                m.push(Item::Separator);
                for (id, t) in [(M_ALIGN_LEFT, "Alinhar à esquerda"), (M_ALIGN_CENTER, "Alinhar ao centro"), (M_ALIGN_RIGHT, "Alinhar à direita"),
                                (M_ALIGN_TOP, "Alinhar em cima"), (M_ALIGN_MIDDLE, "Alinhar no meio"), (M_ALIGN_BOTTOM, "Alinhar embaixo")]
                {
                    m.push(Item::Cmd(id, tr(t)));
                }
                m.push(Item::Cmd2(M_DIST_H, tr("Distribuir na horizontal"), v.len() >= 3));
                m.push(Item::Cmd2(M_DIST_V, tr("Distribuir na vertical"), v.len() >= 3));
                m.push(Item::Separator);
                m.push(Item::Cmd(M_GROUP, tr("A&grupar\tCtrl+Shift+G")));
            }
            if self.cv.can_ungroup() {
                m.push(Item::Cmd(M_UNGROUP, tr("Desagr&upar\tCtrl+Shift+U")));
            }
        } else {
            m.push(Item::Cmd2(M_PASTE, tr("C&olar\tCtrl+V"), can_paste));
            m.push(Item::Cmd2(M_PASTE_INPLACE, tr("Colar no &mesmo lugar\tCtrl+Shift+V"), can_paste));
            if self.cv.has_raster_selection() {
                m.push(Item::Separator);
                m.push(Item::Cmd(M_COPY, tr("&Copiar\tCtrl+C")));
                m.push(Item::Cmd(M_CROP_SEL, tr("Cortar para a &seleção")));
                m.push(Item::Cmd(M_LIFT, tr("&Levantar a seleção (peça solta)\tCtrl+Shift+L")));
            }
            if self.cv.has_floating() {
                m.push(Item::Cmd(M_DROP, tr("Soltar a peça na imagem\tEnter")));
            }
            m.push(Item::Separator);
            m.push(Item::Cmd(M_SELECTALL, tr("Selecionar tu&do\tCtrl+A")));
        }
        let (x, y) = native::cursor_pos();
        let c = Menu::popup(m).choose(self.hwnd, x, y);
        if c != 0 {
            self.command(ctl, c);
        }
    }

    /// The tray and the library list the newest first; combine and compare take the oldest first (before → after).
    pub fn library_selection(&self) -> Vec<String> {
        let mut v = if self.library_open { self.library.selected_paths() } else { self.tray.selected_paths() };
        v.reverse();
        v
    }

    pub fn tray_action(&mut self, ctl: &mut Ctl, a: snapedit_app::tray::TrayAction) {
        use snapedit_app::tray::TrayAction as T;
        match a {
            T::Open(p) => {
                self.open_path(ctl, &p);
            }
            T::ShowInFolder(p) => {
                let args = format!("/select,\"{p}\"");
                let a = native::wide(&args);
                unsafe {
                    windows::Win32::UI::Shell::ShellExecuteW(None, windows::core::w!("open"), windows::core::w!("explorer.exe"),
                                                             windows::core::PCWSTR(a.as_ptr()), windows::core::PCWSTR::null(),
                                                             windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL);
                }
            }
            T::Delete(v) => self.delete_docs(v),
            T::Export(v) => self.export_items(v),
            T::Rename(p) => {
                // the name shown in the tray and the library, and the document's title
                let name = snapedit_app::files::capture_name(&p);
                self.modal(
                    move |o| {
                        let mut f = vec![Field::Text { label: tr("Nome"), text: name }];
                        form::run(o, &tr("Renomear captura"), &mut f).then(|| (p, f[0].text().trim().to_owned()))
                    },
                    |e, _, r| {
                        let Some((p, name)) = r else { return };
                        snapedit_app::files::set_capture_name(&p, &name);
                        if let Some(i) = e.find_doc(&p) {
                            let d = &mut e.cv.docs[i];
                            d.title = if !name.is_empty() {
                                name
                            } else if !d.window_title.is_empty() {
                                d.window_title.clone()
                            } else if !d.capture_mode.is_empty() {
                                d.capture_mode.clone()
                            } else {
                                snapedit_app::doc::file_name_of(&p)
                            };
                        }
                        e.tray.reload();
                        e.library.reload();
                    },
                );
            }
            T::CopyAs(p) => {
                if snapedit_app::doc::ext_of(&p) == ".mp4" {
                    let n = snapedit_app::doc::file_name_of(&p);
                    self.copy_as(p, Image::default(), n);
                } else if let Some((img, name)) = self.load_item_image(&p) {
                    self.copy_as(p, img, name);
                }
            }
            T::Pin(p) => self.pin_path(&p),
            T::Upload(p) => {
                if snapedit_app::doc::ext_of(&p) == ".mp4" {
                    let n = snapedit_app::doc::file_name_of(&p);
                    self.upload_item(p, None, n);
                } else if let Some((img, name)) = self.load_item_image(&p) {
                    self.upload_item(p, Some(img), name);
                }
            }
            T::Combine(mut v) => {
                v.reverse();
                self.combine_now(v, false);
            }
            T::Compare(mut v) => {
                v.reverse();
                self.compare_now(ctl, v);
            }
            T::Batch(v) => self.run_batch(v),
            T::ShowLibrary(on) => self.show_library(on),
            T::DragOut(paths, item) => self.drag_out(paths, &item),
        }
    }

    /// FilesForDrag: images go as PNG with the annotations (in %TEMP%\Snapnab, named after the capture's time),
    /// videos as the library's MP4.
    fn files_for_drag(&self, paths: &[String]) -> Vec<String> {
        use std::os::windows::fs::MetadataExt;
        let dir = std::env::temp_dir().join("Snapnab");
        let _ = std::fs::create_dir_all(&dir);
        let mut out: Vec<String> = Vec::new();
        for p in paths {
            if snapedit_app::doc::ext_of(p) == ".mp4" {
                out.push(p.clone());
                continue;
            }
            let Some((img, _)) = self.load_item_image(p) else { continue };
            let ft = std::fs::metadata(p).map(|m| m.last_write_time()).unwrap_or(0);
            let base = dir.join(format!("{}_{}", tr("Captura"), snapedit_app::tray::file_stamp(ft))).to_string_lossy().into_owned();
            let mut f = format!("{base}.png");
            let mut n = 2;
            while out.contains(&f) {
                f = format!("{base}_{n}.png");
                n += 1;
            }
            if snapcore::codec::save_file(std::path::Path::new(&f), &img, snapcore::codec::Format::Png,
                                          &snapcore::codec::SaveOptions::from_settings(), super::VERSION)
            {
                out.push(f);
            }
        }
        out
    }

    /// LibraryTray::StartDrag: the items go to another program; a drop back onto the Editor itself is discarded.
    fn drag_out(&mut self, paths: Vec<String>, item: &str) {
        use windows::Win32::UI::Shell::{DragFinish, HDROP};
        use windows::Win32::UI::WindowsAndMessaging::{MSG, PM_REMOVE, PeekMessageW, WM_DROPFILES};
        let files = self.files_for_drag(&paths);
        if files.is_empty() {
            snapcore::log::write(&format!("Tray: nothing to drag ({} items)", paths.len()));
            return;
        }
        let original = files.iter().any(|f| paths.contains(f));
        let thumb = snapedit_app::files::load_thumbnail(item);
        let effect = snapcore::dragout::drag_files_out(self.hwnd, &files, thumb.as_ref(), !original);
        // dropping back onto the Editor's own window arrives as a WM_DROPFILES posted only after the drag ends:
        // discard it here so a copy of the item is not imported
        let mut m = MSG::default();
        while unsafe { PeekMessageW(&mut m, None, WM_DROPFILES, WM_DROPFILES, PM_REMOVE) }.as_bool() {
            unsafe { DragFinish(HDROP(m.wParam.0 as _)) };
        }
        snapcore::log::write(&format!("Tray: dragged {} file(s), effect {effect}", files.len()));
    }
}

/// Used by the status text of the tray ("%d item(ns)").
#[allow(dead_code)]
pub fn count_text(n: usize) -> String {
    fmt(&tr("%zu itens"), &[&n])
}
