//! The Editor's bars and side panels (src/editor/Panels.cpp, LayersPanel.cpp and the toolbar of main.cpp): the toolbar
//! with its automatic or customized items, the layers list and the UI Automation names the e2e suite finds them by.

use egui::{Color32, RichText};
use snapcore::doc::{DrawObj, ObjType};

use crate::canvas::{Canvas, Tool};
use crate::ids::*;
use crate::menu::TOOLS;
use crate::tr;

/// Tags a widget with a UI Automation id (AccessKit author id).
pub fn tag(ui: &egui::Ui, r: &egui::Response, name: &str) {
    ui.ctx().accesskit_node_builder(r.id, |n| n.set_author_id(name.to_owned()));
}

// ---------------------------------------------------------------- toolbar
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    Tool,
    Effect,
    Output,
    Command,
    Spacer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub kind: EntryKind,
    pub id: u32,
}

const KINDS: &str = "teocs";

/// Registry text: "t:5,e:1039,o:6,c:1015,s:0" (type:id).
pub fn entries_to_string(v: &[Entry]) -> String {
    v.iter()
        .map(|e| format!("{}:{}", KINDS.as_bytes()[e.kind as usize] as char, e.id))
        .collect::<Vec<_>>()
        .join(",")
}

pub fn entries_from_string(s: &str) -> Vec<Entry> {
    let kinds = [EntryKind::Tool, EntryKind::Effect, EntryKind::Output, EntryKind::Command, EntryKind::Spacer];
    s.split(',')
        .filter_map(|it| {
            let (k, id) = it.split_once(':')?;
            let k = KINDS.find(k.trim())?;
            Some(Entry { kind: kinds[k], id: id.trim().parse().ok()? })
        })
        .collect()
}

pub struct CmdInfo {
    pub id: u32,
    pub label: &'static str,
    pub hint: &'static str,
}

pub const OUTPUTS: [CmdInfo; 5] = [
    CmdInfo { id: T_SAVE, label: "Arquivo", hint: "Salvar como arquivo (PNG, JPG, PDF…)" },
    CmdInfo { id: M_EMAIL, label: "E-mail", hint: "Enviar por e-mail" },
    CmdInfo { id: M_OPEN_PROGRAM, label: "Programa", hint: "Abrir em um programa externo" },
    CmdInfo { id: T_COPYALL, label: "Área de transferência", hint: "Copiar a imagem inteira" },
    CmdInfo { id: M_PRINT, label: "Impressora", hint: "Imprimir a imagem" },
];

pub const COMMANDS: [CmdInfo; 23] = [
    CmdInfo { id: M_UNDO, label: "Desfazer", hint: "Desfazer (Ctrl+Z)" },
    CmdInfo { id: M_REDO, label: "Refazer", hint: "Refazer (Ctrl+Y)" },
    CmdInfo { id: M_NEW_IMAGE, label: "Nova imagem", hint: "Nova imagem em branco" },
    CmdInfo { id: M_NEW_CLIPBOARD, label: "Da área de transf.", hint: "Nova imagem da área de transferência" },
    CmdInfo { id: M_OPEN, label: "Abrir", hint: "Abrir arquivo (Ctrl+O)" },
    CmdInfo { id: M_SAVE, label: "Salvar", hint: "Salvar na biblioteca (Ctrl+S)" },
    CmdInfo { id: M_SAVEAS, label: "Salvar como", hint: "Salvar como arquivo (Ctrl+Shift+S)" },
    CmdInfo { id: M_DUPLICATE, label: "Duplicar", hint: "Duplicar a imagem" },
    CmdInfo { id: M_DELETE_SEL, label: "Excluir objeto", hint: "Excluir o objeto selecionado (Del)" },
    CmdInfo { id: M_OCR, label: "Grab Text", hint: "Reconhecer o texto da imagem (Ctrl+Shift+T)" },
    CmdInfo { id: M_DELETE, label: "Excluir imagem", hint: "Enviar a imagem para a Lixeira" },
    CmdInfo { id: M_FLATTEN, label: "Mesclar tudo", hint: "Mesclar todos os objetos na imagem" },
    CmdInfo { id: M_ROT_CW, label: "Girar", hint: "Girar 90° no sentido horário (Ctrl+R)" },
    CmdInfo { id: M_TRIM, label: "Corte automático", hint: "Cortar as bordas de cor uniforme" },
    CmdInfo { id: M_ALIGN_LEFT, label: "Alinhar à esquerda", hint: "Alinhar os objetos à esquerda" },
    CmdInfo { id: M_ALIGN_CENTER, label: "Alinhar ao centro", hint: "Alinhar os objetos ao centro" },
    CmdInfo { id: M_ALIGN_RIGHT, label: "Alinhar à direita", hint: "Alinhar os objetos à direita" },
    CmdInfo { id: M_ALIGN_TOP, label: "Alinhar em cima", hint: "Alinhar os objetos em cima" },
    CmdInfo { id: M_ALIGN_MIDDLE, label: "Alinhar no meio", hint: "Alinhar os objetos no meio" },
    CmdInfo { id: M_ALIGN_BOTTOM, label: "Alinhar embaixo", hint: "Alinhar os objetos embaixo" },
    CmdInfo { id: M_DIST_H, label: "Distribuir na horizontal", hint: "Espaçar 3 ou mais objetos igualmente na horizontal" },
    CmdInfo { id: M_DIST_V, label: "Distribuir na vertical", hint: "Espaçar 3 ou mais objetos igualmente na vertical" },
    CmdInfo { id: M_LOCK, label: "Bloquear", hint: "Bloquear o objeto selecionado (Ctrl+L)" },
];

pub const RULERS_CMD: CmdInfo = CmdInfo { id: M_RULERS, label: "Réguas", hint: "Mostrar réguas, guias e coordenadas (Ctrl+Shift+R)" };

pub const FIXED_TOOLS: [Tool; 5] = [Tool::Select, Tool::Selection, Tool::Text, Tool::Highlighter, Tool::Shape];

pub fn is_fixed_tool(t: Tool) -> bool {
    FIXED_TOOLS.contains(&t) || t == Tool::Magnify
}

/// The label and hint of a toolbar entry.
pub fn describe(e: &Entry, effect_name: &dyn Fn(u32) -> Option<String>) -> (String, String) {
    match e.kind {
        EntryKind::Tool => TOOLS
            .iter()
            .find(|(t, _)| t.index() as u32 == e.id)
            .map(|(t, h)| (t.name(), tr(h)))
            .unwrap_or_default(),
        EntryKind::Effect => effect_name(e.id)
            .map(|n| (n.clone(), format!("{}{}{}", tr("Efeito: "), n, tr(" — abre os ajustes no painel de efeitos"))))
            .unwrap_or_default(),
        EntryKind::Output => OUTPUTS.iter().find(|c| c.id == e.id).map(|c| (tr(c.label), tr(c.hint))).unwrap_or_default(),
        EntryKind::Command => COMMANDS
            .iter()
            .chain(std::iter::once(&RULERS_CMD))
            .find(|c| c.id == e.id)
            .map(|c| (tr(c.label), tr(c.hint)))
            .unwrap_or_default(),
        EntryKind::Spacer => (String::new(), String::new()),
    }
}

/// What the toolbar asks for.
#[derive(Clone, Debug)]
pub enum BarAction {
    Command(u32),
    Entry(Entry),
    More(egui::Pos2),
    Hint(String),
}

/// Draws the toolbar: Capture and Record on the left, the tools and items in the middle, the outputs on the right.
pub fn toolbar(ui: &mut egui::Ui, center: &[Entry], active: Tool, effect_name: &dyn Fn(u32) -> Option<String>) -> Vec<BarAction> {
    let mut acts = Vec::new();
    let hint = |r: &egui::Response, h: &str, acts: &mut Vec<BarAction>| {
        if r.hovered() && !h.is_empty() {
            acts.push(BarAction::Hint(h.to_owned()));
        }
    };
    ui.horizontal_centered(|ui| {
        for (id, label, h) in [
            (T_CAPTURE, "Capturar", "Nova captura de imagem (pede ao Snapnab)"),
            (T_RECORD, "Gravar", "Nova gravação de vídeo"),
        ] {
            let r = ui.add(egui::Button::new(RichText::new(tr(label)).strong()).min_size(egui::vec2(0.0, 30.0)));
            tag(ui, &r, &format!("SnapnabToolbar.{id}"));
            hint(&r, &tr(h), &mut acts);
            if r.clicked() {
                acts.push(BarAction::Command(id));
            }
        }
        ui.separator();
        for e in center {
            if e.kind == EntryKind::Spacer {
                ui.add_space(14.0);
                continue;
            }
            let (label, h) = describe(e, effect_name);
            if label.is_empty() {
                continue;
            }
            let on = e.kind == EntryKind::Tool && e.id == active.index() as u32;
            let r = ui.add(egui::Button::selectable(on, label).min_size(egui::vec2(0.0, 30.0)));
            let id = match e.kind {
                EntryKind::Tool => T_TOOL_BASE + e.id,
                _ => e.id,
            };
            tag(ui, &r, &format!("SnapnabToolbar.{id}"));
            hint(&r, &h, &mut acts);
            if r.clicked() {
                acts.push(BarAction::Entry(*e));
            }
        }
        let r = ui.add(egui::Button::new(tr("Mais ▾")).min_size(egui::vec2(0.0, 30.0)));
        tag(ui, &r, "SnapnabToolbar.More");
        if r.clicked() {
            acts.push(BarAction::More(r.rect.left_bottom()));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            for (id, label, h) in [
                (T_SHARE, "Compartilhar", "Compartilhar: arquivo, área de transferência, e-mail, impressora, programa"),
                (T_SAVE, "Salvar como", "Salvar como arquivo (Ctrl+Shift+S)"),
                (T_COPYALL, "Copiar tudo", "Copiar a imagem inteira (Ctrl+Shift+C)"),
            ] {
                let r = ui.add(egui::Button::new(tr(label)).min_size(egui::vec2(0.0, 30.0)));
                tag(ui, &r, &format!("SnapnabToolbar.{id}"));
                hint(&r, &tr(h), &mut acts);
                if r.clicked() {
                    acts.push(BarAction::Command(id));
                }
            }
        });
    });
    acts
}

// ---------------------------------------------------------------- layers
pub fn display_name(o: &DrawObj) -> String {
    if !o.name.is_empty() {
        return o.name.clone();
    }
    let text = String::from_utf16_lossy(&o.text);
    let first = || {
        let l = text.lines().next().unwrap_or("").to_owned();
        if l.chars().count() > 40 { l.chars().take(40).collect::<String>() + "…" } else { l }
    };
    const SHAPES: [&str; 8] = ["Retângulo", "Retângulo arredondado", "Elipse", "Triângulo", "Losango", "Paralelogramo", "Hexágono", "Estrela"];
    match o.kind {
        ObjType::Arrow => if text.is_empty() { tr("Seta") } else { format!("{}: {}", tr("Seta"), first()) },
        ObjType::Line => tr("Linha"),
        ObjType::Pen => tr("Caneta"),
        ObjType::Highlighter => tr("Marcador"),
        ObjType::Text => if text.is_empty() { tr("Texto") } else { first() },
        ObjType::Callout => if text.is_empty() { tr("Balão") } else { first() },
        ObjType::Step => tr("Passo %s").replace("%s", &snapcore::doc::step_label(o.step_value, o.style.step_type)),
        ObjType::Stamp => {
            if let Some(rest) = text.strip_prefix("label:") {
                rest.split_once(':').map_or(rest.to_owned(), |(_, t)| t.to_owned())
            } else {
                tr("Carimbo")
            }
        }
        ObjType::Blur => tr("Desfoque"),
        ObjType::Pixelate => tr("Pixelado"),
        ObjType::Spotlight => tr("Destaque"),
        ObjType::Magnify => tr("Lupa"),
        ObjType::Picture => if o.floating { tr("Peça solta") } else { tr("Imagem") },
        ObjType::Group => tr("Grupo (%zu)").replace("%zu", &o.children.len().to_string()),
        ObjType::Link => format!("{}{}", tr("Link: "), text),
        _ => tr(SHAPES[o.shape_kind().clamp(0, 7) as usize]),
    }
}

#[derive(Clone, Debug)]
pub enum LayerAction {
    Select(usize, bool),
    Hidden(usize, bool),
    Locked(usize, bool),
    /// moves object `.0` to just before index `.1` (in document order)
    Move(usize, usize),
    Rename(usize),
    ContextMenu,
}

/// The layers list: the objects from the front (last drawn) to the back, then the image itself.
pub fn layers(ui: &mut egui::Ui, c: &Canvas) -> Vec<LayerAction> {
    let mut acts = Vec::new();
    let bg = ui.allocate_rect(ui.max_rect(), egui::Sense::hover());
    tag(ui, &bg, "SnapnabLayers");
    let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(ui.max_rect().shrink(6.0)));
    ui.label(RichText::new(tr("Camadas")).strong());
    let Some(d) = c.doc().filter(|d| !d.is_video) else {
        ui.label(RichText::new(tr("Nenhuma imagem aberta.")).color(Color32::from_gray(120)));
        return acts;
    };
    let n = d.st.objs.len();
    if n == 0 {
        ui.label(RichText::new(tr("Os objetos desenhados (setas, textos, formas…) aparecem aqui, do mais à frente ao mais atrás."))
            .color(Color32::from_gray(120)));
    }
    egui::ScrollArea::vertical().id_salt("layers-scroll").auto_shrink([false, false]).show(&mut ui, |ui| {
        for i in (0..n).rev() {
            let o = &d.st.objs[i];
            let sel = c.is_selected(i);
            ui.horizontal(|ui| {
                let eye = ui.small_button(if o.hidden { "◌" } else { "👁" }).on_hover_text(tr("Mostrar ou ocultar"));
                tag(ui, &eye, &format!("SnapnabLayers.Eye.{i}"));
                if eye.clicked() {
                    acts.push(LayerAction::Hidden(i, !o.hidden));
                }
                let lock = ui.small_button(if o.locked { "🔒" } else { "🔓" }).on_hover_text(tr("Bloquear ou desbloquear"));
                tag(ui, &lock, &format!("SnapnabLayers.Lock.{i}"));
                if lock.clicked() {
                    acts.push(LayerAction::Locked(i, !o.locked));
                }
                let r = ui.add(egui::Button::selectable(sel, display_name(o)));
                tag(ui, &r, &format!("SnapnabLayers.{i}"));
                if r.clicked() {
                    acts.push(LayerAction::Select(i, ui.input(|x| x.modifiers.ctrl)));
                }
                if r.double_clicked() {
                    acts.push(LayerAction::Rename(i));
                }
                if r.secondary_clicked() {
                    if !sel {
                        acts.push(LayerAction::Select(i, false));
                    }
                    acts.push(LayerAction::ContextMenu);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if i > 0 && ui.small_button("▼").on_hover_text(tr("Enviar para trás")).clicked() {
                        acts.push(LayerAction::Move(i, i - 1));
                    }
                    if i + 1 < n && ui.small_button("▲").on_hover_text(tr("Trazer para a frente")).clicked() {
                        acts.push(LayerAction::Move(i, i + 2));
                    }
                });
            });
        }
        ui.label(RichText::new(tr("Fundo (imagem)")).color(Color32::from_gray(110)));
    });
    acts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toolbar_entries_round_trip() {
        let v = vec![
            Entry { kind: EntryKind::Tool, id: 5 },
            Entry { kind: EntryKind::Effect, id: 1039 },
            Entry { kind: EntryKind::Output, id: 6 },
            Entry { kind: EntryKind::Command, id: 1015 },
            Entry { kind: EntryKind::Spacer, id: 0 },
        ];
        let s = entries_to_string(&v);
        assert_eq!(s, "t:5,e:1039,o:6,c:1015,s:0");
        assert_eq!(entries_from_string(&s), v);
    }
}
