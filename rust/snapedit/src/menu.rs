//! The Editor's menu bar (BuildMenu), its keyboard shortcuts (BuildAccel) and the tool letters (HandleToolKey).
//! The C++ Editor drew the menus in its own title bar; here they are the window's native menu bar (decision
//! recorded in docs/ARCHITECTURE.md), with the same commands and texts.

use egui::{Key, Modifiers};
use snapcore::office::{self, Target};

use crate::canvas::Tool;
use crate::ids::*;
use crate::native::{Item, Menu};
use crate::tr;

fn cmd(id: u32, t: &str) -> Item {
    Item::Cmd(id, tr(t))
}

/// The cursor stamps of Tools › Insert cursor (C25): id and name.
pub fn cursor_stamps() -> Vec<(&'static str, String)> {
    vec![
        ("cursor:arrow", tr("Seta do mouse")),
        ("cursor:arrowdark", tr("Seta do mouse (escura)")),
        ("cursor:hand", tr("Mão (link)")),
        ("cursor:ibeam", tr("Cursor de texto")),
        ("cursor:wait", tr("Ocupado")),
        ("cursor:cross", tr("Mira")),
        ("cursor:move", tr("Mover")),
        ("cursor:resize", tr("Redimensionar")),
        ("cursor:click", tr("Indicador de clique")),
        ("cursor:clickright", tr("Clique direito")),
    ]
}

/// File › Send to: the Office programs (C20).
pub fn office_menu() -> Menu {
    let items = (0..4)
        .filter_map(Target::from_i32)
        .enumerate()
        .map(|(i, t)| Item::Cmd2(M_OFFICE_BASE + i as u32, t.name().to_owned(), office::available(t)))
        .collect();
    Menu::popup(items)
}

/// The tools in the order of the toolbar catalog (kTools), with their hints.
pub const TOOLS: [(Tool, &str); 21] = [
    (Tool::Favorites, "Favoritos: seus estilos favoritos de todas as ferramentas"),
    (Tool::Arrow, "Seta (A)"),
    (Tool::Text, "Texto (T)"),
    (Tool::Callout, "Balão (C)"),
    (Tool::Shape, "Forma: retângulo, elipse… (R)"),
    (Tool::Stamp, "Carimbo (K)"),
    (Tool::Fill, "Preencher (F)"),
    (Tool::Selection, "Seleção e Smart Move (S)"),
    (Tool::MagicWand, "Varinha mágica: seleciona pela cor"),
    (Tool::Crop, "Cortar (X)"),
    (Tool::Cutout, "Recortar faixa (U)"),
    (Tool::Pen, "Caneta (P)"),
    (Tool::Line, "Linha (L)"),
    (Tool::Highlighter, "Marcador: retângulo, elipse, polígono ou mão livre (H)"),
    (Tool::Step, "Passos numerados (N)"),
    (Tool::Eraser, "Borracha (E)"),
    (Tool::Blur, "Ocultar: desfoque/pixelado (B)"),
    (Tool::Select, "Mover e Smart Move (V)"),
    (Tool::Magnify, "Lupa (Z)"),
    (Tool::Spotlight, "Destaque/spotlight (G)"),
    (Tool::Eyedropper, "Conta-gotas (I)"),
];

pub fn tools_menu() -> Menu {
    Menu::popup(TOOLS.iter().map(|(t, _)| Item::Cmd(M_MORE_TOOL_BASE + t.index() as u32, t.name())).collect())
}

pub fn build_menu() -> Menu {
    let file = Menu::popup(vec![
        cmd(M_NEW_CAPTURE, "Nova &captura\tCtrl+Shift+N"),
        cmd(M_NEW_VIDEO, "Nova &gravação de vídeo"),
        cmd(M_NEW_CLIPBOARD, "Novo da área de &transferência"),
        cmd(M_NEW_IMAGE, "Nova &imagem…"),
        Item::Separator,
        cmd(M_OPEN, "&Abrir…\tCtrl+O"),
        cmd(M_SAVE, "&Salvar\tCtrl+S"),
        cmd(M_SAVEAS, "Salvar &como…\tCtrl+Shift+S"),
        cmd(M_DUPLICATE, "&Duplicar"),
        cmd(M_DELETE, "&Excluir (Lixeira)"),
        Item::Separator,
        cmd(M_PRINT, "&Imprimir…\tCtrl+P"),
        cmd(M_EMAIL, "Enviar por &e-mail…"),
        cmd(M_OPEN_PROGRAM, "Abrir em &programa externo…"),
        cmd(M_PIN, "Fi&xar na tela\tCtrl+Shift+P"),
        cmd(M_UPLOAD, "Enviar (&upload)"),
        Item::Sub(tr("Enviar &para"), office_menu()),
        Item::Separator,
        cmd(M_COMBINE, "Com&binar imagens…"),
        cmd(M_COMPARE, "Comparar ima&gens…"),
        cmd(M_BATCH, "Processamento em &lote…"),
        Item::Separator,
        cmd(M_LIBRARY, "Abrir a pasta da &biblioteca"),
        cmd(M_CLOSE, "&Fechar documento\tCtrl+W"),
        cmd(M_EXIT, "Sai&r"),
    ]);
    let edit = Menu::popup(vec![
        cmd(M_UNDO, "&Desfazer\tCtrl+Z"),
        cmd(M_REDO, "&Refazer\tCtrl+Y"),
        cmd(M_UNDOALL, "Desfazer &tudo"),
        Item::Separator,
        cmd(M_CUT, "Recor&tar\tCtrl+X"),
        cmd(M_COPY, "&Copiar\tCtrl+C"),
        cmd(M_COPYALL, "Copiar t&udo\tCtrl+Shift+C"),
        cmd(M_COPYAS, "Copiar co&mo…"),
        cmd(M_PASTE, "C&olar\tCtrl+V"),
        cmd(M_PASTE_INPLACE, "Colar no &mesmo lugar\tCtrl+Shift+V"),
        cmd(M_DELETE_SEL, "E&xcluir\tDel"),
        cmd(M_SELECTALL, "Selecionar tu&do\tCtrl+A"),
        cmd(M_CROP_SEL, "Cortar para a &seleção"),
        cmd(M_LIFT, "&Levantar a seleção (peça solta)\tCtrl+Shift+L"),
        cmd(M_DROP, "Soltar a &peça na imagem\tEnter"),
        Item::Separator,
        cmd(M_PLAINPASTE, "Colar texto sem &formatação"),
        cmd(M_EDGESNAP, "Seleção encaixa nas &bordas da imagem"),
        cmd(M_DRAWHOLD, "Desen&har e segurar vira forma (Caneta)"),
        Item::Separator,
        cmd(M_OCR, "&Grab Text (OCR)\tCtrl+Shift+O"),
    ]);
    let fxm = Menu::popup(vec![
        cmd(M_FX_BORDER, "&Borda"),
        cmd(M_FX_EDGE, "Bor&das"),
        cmd(M_FX_CAPTION, "&Informações da captura"),
        cmd(M_FX_PERSPECTIVE, "&Perspectiva"),
        cmd(M_FX_PAGECURL, "D&obra de página"),
        cmd(M_FX_SHADOW, "&Sombra"),
        cmd(M_FX_FILTER, "&Filtros"),
        cmd(M_FX_ADJUST, "&Ajuste de cores"),
        cmd(M_FX_REPLACE, "Substituição de co&r"),
        cmd(M_FX_SPOTLIGHT, "Spotlight e &lupa"),
        cmd(M_FX_WATERMARK, "&Marca d'água"),
        Item::Separator,
        cmd(M_BEAUTIFY, "&Embelezar…"),
    ]);
    let align = Menu::popup(vec![
        cmd(M_ALIGN_LEFT, "À &esquerda"),
        cmd(M_ALIGN_CENTER, "Ao &centro"),
        cmd(M_ALIGN_RIGHT, "À &direita"),
        Item::Separator,
        cmd(M_ALIGN_TOP, "Em &cima"),
        cmd(M_ALIGN_MIDDLE, "No &meio"),
        cmd(M_ALIGN_BOTTOM, "Em&baixo"),
        Item::Separator,
        cmd(M_DIST_H, "Distribuir na &horizontal"),
        cmd(M_DIST_V, "Distribuir na &vertical"),
    ]);
    let arrange = Menu::popup(vec![
        cmd(M_BRING_FORWARD, "Trazer para a &frente\tCtrl+Shift+F"),
        cmd(M_FRONT, "Trazer para o &primeiro plano"),
        cmd(M_SEND_BACKWARD, "Enviar para &trás\tCtrl+Shift+B"),
        cmd(M_BACK, "Enviar para o plano de &fundo"),
        Item::Separator,
        Item::Sub(tr("&Alinhar"), align),
        Item::Separator,
        cmd(M_FLIP_V, "Espelhar na &vertical"),
        cmd(M_FLIP_H, "Espelhar na &horizontal"),
    ]);
    let rotate = Menu::popup(vec![
        cmd(M_ROT_CW, "Sentido &horário\tCtrl+R"),
        cmd(M_ROT_CCW, "Sentido &anti-horário"),
        cmd(M_ROT_180, "&180 graus"),
        cmd(M_ROT_FREE, "Ângulo &personalizado…"),
    ]);
    let img = Menu::popup(vec![
        Item::Sub(tr("Ferramen&tas"), tools_menu()),
        Item::Sub(tr("&Efeitos"), fxm),
        Item::Separator,
        Item::Sub(tr("&Organizar"), arrange),
        cmd(M_GROUP, "A&grupar\tCtrl+Shift+G"),
        cmd(M_UNGROUP, "Desagr&upar\tCtrl+Shift+U"),
        cmd(M_LOCK, "&Bloquear objeto\tCtrl+L"),
        cmd(M_UNLOCK, "Desblo&quear objeto\tCtrl+Shift+K"),
        Item::Separator,
        cmd(M_FLATTEN_SEL, "&Mesclar\tCtrl+T"),
        cmd(M_FLATTEN, "Mesclar t&udo\tCtrl+Shift+T"),
        Item::Separator,
        cmd(M_TRIM, "A&parar"),
        cmd(M_RESIZE, "&Redimensionar imagem…"),
        cmd(M_CANVAS, "Redimensionar &canvas…"),
        cmd(M_APPLY_TEMPLATE, "Aplicar mode&lo…\tCtrl+Alt+C"),
        Item::Separator,
        Item::Sub(tr("&Girar"), rotate),
        cmd(M_SNAPPING, "Encai&xe no canvas\tCtrl+;"),
        cmd(M_CANVAS_COLOR, "Cor do canva&s…"),
        Item::Separator,
        cmd(M_TAG, "Etiquetar &imagem…"),
    ]);
    let view = Menu::popup(vec![
        cmd(M_ZOOM_IN, "&Ampliar\tCtrl++"),
        cmd(M_ZOOM_OUT, "&Reduzir\tCtrl+-"),
        cmd(M_ZOOM_100, "&Tamanho real\tCtrl+1"),
        cmd(M_ZOOM_FIT, "A&justar à janela\tCtrl+0"),
        Item::Separator,
        cmd(M_BG_COLOR, "&Cor de fundo da área…"),
        cmd(M_RULERS, "Ré&guas e guias\tCtrl+Shift+R"),
        cmd(M_CLEAR_GUIDES, "Remover as gu&ias"),
        Item::Separator,
        cmd(M_SHOW_EFFECTS, "Painel de &efeitos"),
        cmd(M_SHOW_LAYERS, "Painel de ca&madas"),
        cmd(M_SHOW_LIBRARY, "&Biblioteca"),
        cmd(M_SHOW_TRAY, "Capturas &recentes (bandeja)"),
        Item::Separator,
        cmd(M_TB_CUSTOMIZE, "&Personalizar a barra de ferramentas…"),
    ]);
    let help = Menu::popup(vec![
        cmd(M_MANUAL, "&Manual do Snapnab\tF1"),
        cmd(M_SHORTCUTS, "&Atalhos de teclado"),
        cmd(M_ABOUT, "&Sobre o Snapnab Editor"),
    ]);
    let cursors =
        Menu::popup(cursor_stamps().into_iter().enumerate().map(|(i, (_, n))| Item::Cmd(M_CURSOR_BASE + i as u32, n)).collect());
    let tools = Menu::popup(vec![
        cmd(M_READ_QR, "Ler código &QR\tCtrl+Shift+Q"),
        cmd(M_REDACT, "&Ocultar dados sensíveis…\tCtrl+Shift+E"),
        cmd(M_REMOVE_BG, "Remover o &fundo…"),
        cmd(M_SPLIT, "&Dividir imagem…"),
        cmd(M_ADD_CAPTURE, "Adicionar ca&ptura"),
        Item::Separator,
        cmd(M_DATESTAMP, "Inserir data e &hora\tCtrl+Shift+D"),
        Item::Sub(tr("Inserir c&ursor"), cursors),
        cmd(M_DATESTAMP_FMT, "Formato da data e hora…"),
        Item::Separator,
        cmd(M_CONTRAST, "Verificador de &contraste…"),
        cmd(M_ASSISTANT, "&Assistente de IA…"),
        Item::Separator,
        cmd(M_AI_SETTINGS, "Configurar o assistente de IA…"),
        cmd(M_UPLOAD_SETTINGS, "Configurar o &upload…"),
    ]);
    Menu::bar(vec![
        Item::Sub(tr("&Arquivo"), file),
        Item::Sub(tr("&Editar"), edit),
        Item::Sub(tr("&Imagem"), img),
        Item::Sub(tr("&Ferramentas"), tools),
        Item::Sub(tr("E&xibir"), view),
        Item::Sub(tr("A&juda"), help),
    ])
}

/// The accelerator table: (ctrl, shift, alt, key) → command.
const ACCEL: &[(bool, bool, bool, Key, u32)] = &[
    (true, false, false, Key::O, M_OPEN),
    (true, false, false, Key::S, M_SAVE),
    (true, true, false, Key::S, M_SAVEAS),
    (true, true, false, Key::N, M_NEW_CAPTURE),
    (true, false, false, Key::P, M_PRINT),
    (true, false, false, Key::W, M_CLOSE),
    (true, false, false, Key::Z, M_UNDO),
    (true, false, false, Key::Y, M_REDO),
    (true, true, false, Key::Z, M_REDO),
    (true, false, false, Key::X, M_CUT),
    (true, false, false, Key::C, M_COPY),
    (true, true, false, Key::C, M_COPYALL),
    (true, false, false, Key::V, M_PASTE),
    (true, true, false, Key::V, M_PASTE_INPLACE),
    (true, false, false, Key::A, M_SELECTALL),
    (true, true, false, Key::O, M_OCR),
    (true, false, false, Key::L, M_LOCK),
    (true, true, false, Key::K, M_UNLOCK),
    (true, true, false, Key::L, M_LIFT),
    (true, true, false, Key::R, M_RULERS),
    (true, true, false, Key::F, M_BRING_FORWARD),
    (true, true, false, Key::B, M_SEND_BACKWARD),
    (true, true, false, Key::G, M_GROUP),
    (true, true, false, Key::U, M_UNGROUP),
    (true, false, false, Key::T, M_FLATTEN_SEL),
    (true, true, false, Key::T, M_FLATTEN),
    (true, false, true, Key::C, M_APPLY_TEMPLATE),
    (true, false, false, Key::Semicolon, M_SNAPPING),
    (true, false, false, Key::R, M_ROT_CW),
    (true, false, false, Key::Plus, M_ZOOM_IN),
    (true, false, false, Key::Equals, M_ZOOM_IN),
    (true, false, false, Key::Minus, M_ZOOM_OUT),
    (true, false, false, Key::Num1, M_ZOOM_100),
    (true, false, false, Key::Num0, M_ZOOM_FIT),
    (false, false, false, Key::F1, M_MANUAL),
    (true, true, false, Key::P, M_PIN),
    (true, true, false, Key::Q, M_READ_QR),
    (true, true, false, Key::E, M_REDACT),
    (true, true, false, Key::D, M_DATESTAMP),
];

/// The command of a key with its modifiers, if it is a shortcut.
pub fn accelerator(key: Key, m: Modifiers) -> Option<u32> {
    ACCEL
        .iter()
        .find(|(c, s, a, k, _)| *k == key && *c == m.ctrl && *s == m.shift && *a == m.alt)
        .map(|e| e.4)
}

/// The tool letters (with the canvas in focus, no modifiers).
pub fn tool_key(key: Key) -> Option<Tool> {
    Some(match key {
        Key::V => Tool::Select,
        Key::A => Tool::Arrow,
        Key::L => Tool::Line,
        Key::R => Tool::Shape,
        Key::P => Tool::Pen,
        Key::H => Tool::Highlighter,
        Key::T => Tool::Text,
        Key::C => Tool::Callout,
        Key::N => Tool::Step,
        Key::K => Tool::Stamp,
        Key::B => Tool::Blur,
        Key::G => Tool::Spotlight,
        Key::Z => Tool::Magnify,
        Key::F => Tool::Fill,
        Key::E => Tool::Eraser,
        Key::X => Tool::Crop,
        Key::S => Tool::Selection,
        Key::U => Tool::Cutout,
        Key::I => Tool::Eyedropper,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_follow_the_accelerator_table() {
        assert_eq!(accelerator(Key::S, Modifiers::CTRL), Some(M_SAVE));
        assert_eq!(accelerator(Key::S, Modifiers::CTRL | Modifiers::SHIFT), Some(M_SAVEAS));
        assert_eq!(accelerator(Key::C, Modifiers::CTRL | Modifiers::SHIFT), Some(M_COPYALL));
        assert_eq!(accelerator(Key::S, Modifiers::NONE), None);
        assert_eq!(tool_key(Key::K), Some(Tool::Stamp));
    }
}
