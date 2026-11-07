//! The capture window's forms and questions (main.cpp: AddPreset, RenamePreset, EditPresetHotkey, PresetHelp, the
//! presets and settings export / import, ShowOptions, the output and fixed-region forms, the effects). Each runs on its
//! own thread (a form has its own message loop); what changes the presets comes back to the app as `UiEv::Store`, a
//! closure run on the app's thread, and the registry settings are written here.

use std::ffi::OsStr;
use std::path::Path;

use snapcore::presets::{CaptureKind, FileFormat, Preset, PresetStore, Selection};
use snapcore::{i18n, instance, reg, settings, sounds};
use snapnab_app::tr;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    IDYES, MB_ICONINFORMATION, MB_ICONQUESTION, MB_ICONWARNING, MB_YESNO, MESSAGEBOX_RESULT, MESSAGEBOX_STYLE, MessageBoxW, PostMessageW,
    WM_COMMAND,
};
use windows::core::PCWSTR;

use crate::form::{self, Field};
use crate::hotkeys::{pack, unpack};
use crate::pins::{UiEv, send};

/// A change of the presets, run on the app's thread (then saved, and the hotkeys registered again).
pub type StoreEdit = Box<dyn FnOnce(&mut PresetStore) + Send>;

fn edit(f: impl FnOnce(&mut PresetStore) + Send + 'static) {
    send(UiEv::Store(Box::new(f)));
}

pub fn message(owner: isize, text: &str, title: &str, flags: MESSAGEBOX_STYLE) -> MESSAGEBOX_RESULT {
    let (t, c) = (snapcore::wide::to_wide(text), snapcore::wide::to_wide(title));
    unsafe { MessageBoxW((owner != 0).then(|| HWND(owner as _)), PCWSTR(t.as_ptr()), PCWSTR(c.as_ptr()), flags) }
}

/// A message box on its own thread (the app keeps running).
pub fn message_async(owner: isize, text: String, title: String, flags: MESSAGEBOX_STYLE) {
    std::thread::spawn(move || {
        message(owner, &text, &title, flags);
    });
}

fn post(owner: isize, cmd: u32) {
    unsafe {
        let _ = PostMessageW(Some(HWND(owner as _)), WM_COMMAND, WPARAM(cmd as usize), LPARAM(0));
    }
}

fn dword(n: &str, d: u32) -> u32 {
    reg::get_dword(OsStr::new(n), d, None)
}

fn set_dword(n: &str, v: u32) {
    reg::set_dword(OsStr::new(n), v, None);
}

fn string(n: &str) -> String {
    reg::get_string(OsStr::new(n), None).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn text(label: &str, t: &str) -> Field {
    Field::Text { label: label.into(), text: t.into() }
}

fn check(label: &str, v: bool) -> Field {
    Field::Check { label: label.into(), value: v }
}

fn int(label: &str, v: i32, min: i32, max: i32) -> Field {
    Field::Int { label: label.into(), value: v.clamp(min, max), min, max }
}

fn combo(label: &str, items: &[String], v: i32) -> Field {
    Field::Combo { label: label.into(), items: items.to_vec(), value: v.clamp(0, items.len().saturating_sub(1) as i32) }
}

fn hotkey(label: &str, packed: u32) -> Field {
    let (mods, vk) = unpack(packed);
    Field::Hotkey { label: label.into(), mods, vk }
}

// ---------------------------------------------------------------- presets

/// AskPresetName: the trimmed name, or None (Cancel or empty).
fn ask_name(owner: isize, title: &str, name: &str) -> Option<String> {
    let mut f = [text(&tr("Nome do preset"), name)];
    if !form::run(owner, title, &mut f) {
        return None;
    }
    let n = f[0].text().trim().to_owned();
    (!n.is_empty()).then_some(n)
}

/// AddPreset: the name asked, then the preset added at the end and selected; the list opens.
fn add_preset(owner: isize, mut p: Preset, title: String) {
    std::thread::spawn(move || {
        let Some(n) = ask_name(owner, &title, &p.name) else { return };
        p.name = n;
        (p.hotkey_mods, p.hotkey_vk) = (0, 0);
        p.builtin.clear();
        edit(move |s| {
            s.presets.push(p);
            s.selected = s.presets.len() as i32 - 1;
        });
        set_dword("CapturePresetsOpen", 1);
    });
}

pub fn preset_new(owner: isize, store: &PresetStore, cur: &Preset) {
    let mut p = Preset::default();
    p.name = tr("Preset %zu").replace("%zu", &(store.presets.len() + 1).to_string());
    p.kind = cur.kind;
    if p.kind == CaptureKind::Video {
        p.capture_cursor = true;
    }
    add_preset(owner, p, tr("Novo preset"));
}

pub fn preset_from_current(owner: isize, cur: &Preset) {
    let mut p = cur.clone();
    p.name = i18n::tr_str(&cur.name) + &tr(" (cópia)");
    add_preset(owner, p, tr("Novo preset com as configurações atuais"));
}

pub fn preset_rename(owner: isize, index: usize, name: &str) {
    let name = name.to_owned();
    std::thread::spawn(move || {
        let Some(n) = ask_name(owner, &tr("Renomear preset"), &name) else { return };
        edit(move |s| {
            if let Some(p) = s.presets.get_mut(index) {
                p.name = n;
            }
        });
    });
}

/// Duplicate: the copy at the end, selected, without the hotkey.
pub fn preset_duplicate(s: &mut PresetStore) {
    let Some(mut p) = s.presets.get(s.selected as usize).cloned() else { return };
    p.name = i18n::tr_str(&p.name) + &tr(" (cópia)");
    (p.hotkey_mods, p.hotkey_vk) = (0, 0);
    p.builtin.clear();
    s.presets.push(p);
    s.selected = s.presets.len() as i32 - 1;
}

pub fn preset_delete(owner: isize, store: &PresetStore) {
    let (n, index) = (store.presets.len(), store.selected.max(0) as usize);
    let name = store.presets.get(index).map(|p| i18n::tr_str(&p.name)).unwrap_or_default();
    std::thread::spawn(move || {
        if n <= 1 {
            message(owner, &tr("É preciso manter pelo menos um preset."), "Snapnab", MB_ICONINFORMATION);
            return;
        }
        if message(owner, &tr("Excluir o preset \"%s\"?").replace("%s", &name), "Snapnab", MB_YESNO | MB_ICONQUESTION) != IDYES {
            return;
        }
        edit(move |s| {
            if index < s.presets.len() && s.presets.len() > 1 {
                s.presets.remove(index);
                s.selected = (index as i32 - 1).max(0);
            }
        });
    });
}

/// EditPresetHotkey: the form; the app sets the key (`UiEv::PresetHotkey`) and says when it is taken.
pub fn preset_hotkey(owner: isize, index: usize, p: &Preset) {
    let (name, mods, vk) = (i18n::tr_str(&p.name), p.hotkey_mods, p.hotkey_vk);
    std::thread::spawn(move || {
        let mut f = [Field::Hotkey { label: tr("Atalho do preset"), mods, vk }, Field::Label(tr("Deixe vazio (Backspace) para remover."))];
        if !form::run(owner, &(tr("Atalho — ") + &name), &mut f) {
            return;
        }
        let (m, v) = f[0].hotkey();
        send(UiEv::PresetHotkey(index, m, v));
    });
}

pub fn preset_help(owner: isize) {
    message_async(owner,
                  tr("Um preset guarda tudo o que a janela de captura mostra: tipo (imagem ou vídeo), seleção, efeitos, destino e opções de gravação.\n\n•  Clique em um preset para carregá-lo na janela; clique duas vezes para capturar com ele.\n•  \"Adicionar atalho\" dá a ele uma tecla de atalho global.\n•  A engrenagem da linha renomeia, duplica ou exclui o preset.\n•  \"Novo preset com as configurações atuais\" salva o que está na janela agora."),
                  tr("Presets"), MB_ICONINFORMATION);
}

pub fn open_dialog(owner: isize, title: &str, filter: &str) -> Option<String> {
    use windows::Win32::UI::Controls::Dialogs::{GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_PATHMUSTEXIST, OPENFILENAMEW};
    let filter: Vec<u16> = filter.encode_utf16().map(|c| if c == '|' as u16 { 0 } else { c }).chain([0]).collect();
    let t = snapcore::wide::to_wide(title);
    let mut buf = vec![0u16; 4096];
    let mut ofn = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: HWND(owner as _),
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: windows::core::PWSTR(buf.as_mut_ptr()),
        nMaxFile: buf.len() as u32,
        lpstrTitle: if title.is_empty() { PCWSTR::null() } else { PCWSTR(t.as_ptr()) },
        Flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST,
        ..Default::default()
    };
    if !unsafe { GetOpenFileNameW(&mut ofn) }.as_bool() {
        return None;
    }
    let n = buf.iter().position(|&c| c == 0).unwrap_or(0);
    Some(String::from_utf16_lossy(&buf[..n]))
}

pub fn presets_export(owner: isize, store: &PresetStore) {
    let store = store.clone();
    std::thread::spawn(move || {
        let Some(path) = crate::pins::save_file_dialog(owner, "", "Presets.snappresets", &tr("Presets do Snapnab (*.snappresets)|*.snappresets|"),
                                                       "snappresets")
        else {
            return;
        };
        if !store.export(Path::new(&path)) {
            message(owner, &tr("Não foi possível exportar."), "Snapnab", MB_ICONWARNING);
        }
    });
}

pub fn presets_import(owner: isize) {
    std::thread::spawn(move || {
        let Some(path) = open_dialog(owner, "", &tr("Presets do Snapnab (*.snappresets;*.xml)|*.snappresets;*.xml|")) else { return };
        edit(move |s| {
            if s.import(Path::new(&path)) {
                send(UiEv::Notify("Snapnab".into(), tr("Presets importados.")));
            } else {
                message_async(owner, tr("Arquivo de presets inválido."), "Snapnab".into(), MB_ICONWARNING);
            }
        });
    });
}

// ---------------------------------------------------------------- settings (F51)

fn settings_filter() -> String {
    tr("Configurações do Snapnab (*.snapsettings)|*.snapsettings|")
}

fn stamp() -> String {
    String::from_utf16_lossy(&snapcore::library::time_stamp(false))
}

pub fn settings_export(owner: isize) {
    std::thread::spawn(move || {
        let Some(path) = crate::pins::save_file_dialog(owner, &tr("Exportar configurações"), "Snapnab.snapsettings", &settings_filter(), "snapsettings")
        else {
            return;
        };
        match settings::export(Path::new(&path)) {
            Ok(()) => send(UiEv::Notify("Snapnab".into(),
                                        tr("Configurações exportadas: presets, temas, estilos rápidos, favoritos, efeitos rápidos, ordem dos carimbos e barra de ferramentas."))),
            Err(e) => {
                message(owner, &e, "Snapnab", MB_ICONWARNING);
            }
        }
    });
}

/// The Replace / Merge question (TaskDialog with command links); Some(merge) or None.
fn ask_replace_or_merge(owner: isize) -> Option<bool> {
    use windows::Win32::UI::Controls::{
        TASKDIALOG_BUTTON, TASKDIALOGCONFIG, TDCBF_CANCEL_BUTTON, TDF_ALLOW_DIALOG_CANCELLATION, TDF_USE_COMMAND_LINKS, TaskDialogIndirect,
    };
    let w = |s: &str| snapcore::wide::to_wide(s);
    let (replace, merge) = (w(&tr("Substituir\nAs configurações atuais dão lugar às do arquivo.")),
                            w(&tr("Mesclar\nMantém as configurações atuais e acrescenta o que faltar (presets, temas, favoritos, efeitos rápidos…).")));
    let (title, main, content) = (w("Snapnab"), w(&tr("Importar configurações")),
                                  w(&tr("O Snapnab e o Editor reiniciam com as configurações importadas. Uma cópia das configurações atuais fica na pasta de backups.")));
    let btns = [TASKDIALOG_BUTTON { nButtonID: 100, pszButtonText: PCWSTR(replace.as_ptr()) },
                TASKDIALOG_BUTTON { nButtonID: 101, pszButtonText: PCWSTR(merge.as_ptr()) }];
    let tc = TASKDIALOGCONFIG {
        cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
        hwndParent: HWND(owner as _),
        dwFlags: TDF_USE_COMMAND_LINKS | TDF_ALLOW_DIALOG_CANCELLATION,
        pszWindowTitle: PCWSTR(title.as_ptr()),
        pszMainInstruction: PCWSTR(main.as_ptr()),
        pszContent: PCWSTR(content.as_ptr()),
        cButtons: 2,
        pButtons: btns.as_ptr(),
        dwCommonButtons: TDCBF_CANCEL_BUTTON,
        ..Default::default()
    };
    let mut btn = 0;
    unsafe { TaskDialogIndirect(&tc, Some(&mut btn), None, None) }.ok()?;
    match btn {
        100 => Some(false),
        101 => Some(true),
        _ => None,
    }
}

/// ImportSettings: the Editor is closed first (so it does not write its settings over the imported ones when it
/// exits), the current settings go to Backups\Before-import-<time>, then both programs restart with the new ones.
pub fn settings_import(owner: isize, store: &PresetStore) {
    let store = store.clone();
    std::thread::spawn(move || {
        let Some(path) = open_dialog(owner, &tr("Importar configurações"), &settings_filter()) else { return };
        if !settings::validate(Path::new(&path)) {
            message(owner, &tr("Arquivo de configurações inválido."), "Snapnab", MB_ICONWARNING);
            return;
        }
        let Some(merge) = ask_replace_or_merge(owner) else { return };
        let editor_open = close_editor_and_wait();
        store.save();
        let _ = settings::export(&settings::backup_dir().join(format!("Before-import-{}.snapsettings", stamp())));
        if let Err(e) = settings::import(Path::new(&path), merge) {
            message(owner, &e, "Snapnab", MB_ICONWARNING);
            if editor_open {
                instance::launch_sibling("SnapnabEditor.exe", "", false);
            }
            return;
        }
        send(UiEv::Store(Box::new(|s| *s = PresetStore::load()))); // the imported presets, in case they are saved on the way out
        if editor_open {
            instance::launch_sibling("SnapnabEditor.exe", "", false);
        }
        if instance::launch_sibling("Snapnab.exe", &instance::restart_args(), false) {
            post(owner, crate::id::EXIT);
        }
    });
}

/// The Editor asked to close; true if it was open (waits up to 15 s for it to end).
fn close_editor_and_wait() -> bool {
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject};
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindowThreadProcessId, WM_CLOSE};
    let Some(ed) = unsafe { FindWindowW(windows::core::w!("SnapnabEditor"), PCWSTR::null()) }.ok().filter(|h| !h.0.is_null()) else {
        return false;
    };
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(ed, Some(&mut pid));
        let ph = OpenProcess(PROCESS_SYNCHRONIZE, false, pid);
        let _ = PostMessageW(Some(ed), WM_CLOSE, WPARAM(0), LPARAM(0));
        if let Ok(ph) = ph {
            WaitForSingleObject(ph, 15000);
            let _ = windows::Win32::Foundation::CloseHandle(ph);
        }
    }
    true
}

// ---------------------------------------------------------------- Options

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

fn startup_enabled() -> bool {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
    let (k, n) = (snapcore::wide::to_wide(RUN_KEY), snapcore::wide::to_wide("Snapnab"));
    unsafe { RegGetValueW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()), PCWSTR(n.as_ptr()), RRF_RT_REG_SZ, None, None, None) }.is_ok()
}

fn set_startup(on: bool) {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RegDeleteKeyValueW, RegSetKeyValueW};
    let (k, n) = (snapcore::wide::to_wide(RUN_KEY), snapcore::wide::to_wide("Snapnab"));
    unsafe {
        if on {
            let v = snapcore::wide::to_wide(format!("\"{}\" /hide", snapcore::paths::exe_dir().join("Snapnab.exe").display()));
            let _ = RegSetKeyValueW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()), PCWSTR(n.as_ptr()), REG_SZ.0, Some(v.as_ptr() as *const _),
                                    (v.len() * 2) as u32);
        } else {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()), PCWSTR(n.as_ptr()));
        }
    }
}

/// ShowOptions: the global hotkeys, general settings, after capturing, capture, privacy and library.
pub fn options(owner: isize) {
    std::thread::spawn(move || {
        use windows::Win32::UI::Input::KeyboardAndMouse::MOD_SHIFT;
        let shift = MOD_SHIFT.0;
        let mut lang_codes = vec![String::new()];
        let mut lang_names = vec![tr("Automático (idioma do Windows)")];
        for (c, n) in i18n::LANGUAGES {
            if i18n::available(c) {
                lang_codes.push(c.into());
                lang_names.push(n.into());
            }
        }
        let lang_index = lang_codes.iter().position(|c| c == i18n::configured()).unwrap_or(0) as i32;
        let sound_names: Vec<String> = (0..sounds::Kind::COUNT).map(|k| sounds::Kind::from_index(k).name()).collect();
        let startup = startup_enabled();
        let mut f = vec![
            Field::Separator(tr("Atalhos globais")),
            hotkey(&tr("Capturar imagem"), dword("ImageHotkey", pack(0, 0x2C))),
            hotkey(&tr("Gravar vídeo"), dword("VideoHotkey", pack(shift, 0x2C))),
            hotkey(&tr("Pausar/retomar vídeo"), dword("VideoStartPauseResumeKey", pack(shift, 0x78))),
            hotkey(&tr("Parar vídeo"), dword("VideoStopKey", pack(shift, 0x79))),
            hotkey(&tr("Conta-gotas de tela"), dword("ColorPickerHotkey", 0)), // 5
            hotkey(&tr("Capturar e fixar na tela"), dword("CapturePinHotkey", 0)),
            hotkey(&tr("Fixar da área de transferência"), dword("PinClipboardHotkey", 0)),
            hotkey(&tr("Foco de tela (apresentações)"), dword("FocusHotkey", 0)),
            Field::Separator(tr("Geral")), // 9
            text(&tr("Pasta de saída"), &crate::engine::default_save_dir()),
            combo(&tr("Monitorar área de transferência"),
                  &[tr("Desligado"), tr("Só a Ferramenta de Captura do Windows"), tr("Qualquer aplicativo")], dword("ClipboardMonitor", 1) as i32),
            check(&tr("Iniciar com o Windows"), startup),
            check(&tr("Mostrar a janela de captura ao iniciar"), dword("ShowCaptureWindow", 1) != 0),
            combo(&tr("Idioma"), &lang_names, lang_index),
            Field::Separator(tr("Depois da captura")), // 15
            check(&tr("Copiar toda captura para a área de transferência"), dword("CopyEveryCapture", 1) != 0),
            combo(&tr("Ao terminar"),
                  &[tr("Abrir no Editor (como o preset diz)"), tr("Mostrar a miniatura rápida em vez do Editor"),
                    tr("Mostrar a miniatura e abrir o Editor"), tr("Só copiar para a área de transferência")],
                  crate::engine::after_capture_mode() as i32),
            int(&tr("A miniatura fecha após (s, 0 = não fecha)"), dword("OverlaySeconds", 8) as i32, 0, 600),
            combo(&tr("Canto da miniatura"),
                  &[tr("Inferior direito"), tr("Inferior esquerdo"), tr("Superior direito"), tr("Superior esquerdo")],
                  dword("OverlayCorner", 0).min(3) as i32),
            combo(&tr("Som da captura"), &sound_names, dword("CaptureSound", 0).min(sounds::Kind::COUNT - 1) as i32),
            text(&tr("Arquivo WAV do som"), &string("CaptureSoundFile")),
            Field::Separator(tr("Captura")), // 22
            check(&tr("Capturar a região assim que soltar o mouse"), dword("QuickCrop", 1) != 0),
            check(&tr("Corrigir as cores em monitores HDR"), dword("HdrCorrection", 1) != 0),
            check(&tr("Cantos arredondados transparentes nas janelas"), dword("WindowRoundCorners", 1) != 0),
            check(&tr("Sombra nas capturas de janela"), dword("WindowShadow", 0) != 0),
            check(&tr("Ocultar os ícones da área de trabalho ao capturar"), dword("HideDesktopIcons", 0) != 0),
            check(&tr("Não perturbe (sem notificações) ao gravar"), dword("DndWhileRecording", 0) != 0),
            check(&tr("Manter a tela acesa durante a captura"), dword("KeepAwake", 1) != 0),
            Field::Separator(tr("Privacidade e biblioteca")), // 30
            check(&tr("Remover metadados (câmera, GPS, datas) ao salvar"), dword("StripMetadata", 0) != 0),
            check(&tr("Limpar a biblioteca automaticamente"), dword("CleanupEnabled", 0) != 0),
            int(&tr("Apagar capturas com mais de (dias)"), dword("CleanupDays", 30) as i32, 1, 3650),
            check(&tr("Manter as capturas que têm nome"), dword("CleanupKeepNamed", 1) != 0),
            check(&tr("Manter as capturas com etiquetas"), dword("CleanupKeepTagged", 1) != 0),
            check(&tr("Enviar para a Lixeira em vez de apagar"), dword("CleanupRecycle", 1) != 0),
            check(&tr("Cursor capturado como objeto no Editor"), dword("CursorAsObject", 0) != 0), // 37 (C25)
            check(&tr("Dividir imagens longas em páginas (impressão e PDF)"), dword("SplitLongPages", 1) != 0), // 38 (C34)
            Field::Label(tr("Biblioteca: ") + &String::from_utf16_lossy(&snapcore::library::library_dir())),
        ];
        let validate = |f: &[Field]| -> String {
            let mut keys: Vec<(u32, u32)> = Vec::new();
            for x in &f[1..=8] {
                let k = x.hotkey();
                if k.1 == 0 {
                    continue;
                }
                if keys.contains(&k) {
                    return tr("Dois atalhos iguais: ") + &crate::hotkeys::to_text(k.0, k.1);
                }
                keys.push(k);
            }
            String::new()
        };
        if !form::run_ex(owner, &tr("Opções do Snapnab"), &mut f, 480, Some(&validate)) {
            return;
        }
        for (i, name) in [(1, "ImageHotkey"), (2, "VideoHotkey"), (3, "VideoStartPauseResumeKey"), (4, "VideoStopKey"), (5, "ColorPickerHotkey"),
                          (6, "CapturePinHotkey"), (7, "PinClipboardHotkey"), (8, "FocusHotkey")] {
            let (m, v) = f[i].hotkey();
            set_dword(name, pack(m, v));
        }
        let out = f[10].text().trim();
        if !out.is_empty() {
            reg::set_string(OsStr::new("OutputFolder"), OsStr::new(out), None);
        }
        set_dword("ClipboardMonitor", f[11].int() as u32);
        if f[12].on() != startup {
            set_startup(f[12].on()); // written only when it changes (it names this copy of Snapnab.exe)
        }
        for (i, name) in [(13, "ShowCaptureWindow"), (16, "CopyEveryCapture"), (17, "AfterCaptureMode"), (18, "OverlaySeconds"), (19, "OverlayCorner"),
                          (20, "CaptureSound"), (23, "QuickCrop"), (24, "HdrCorrection"), (25, "WindowRoundCorners"), (26, "WindowShadow"),
                          (27, "HideDesktopIcons"), (28, "DndWhileRecording"), (29, "KeepAwake"), (31, "StripMetadata"), (32, "CleanupEnabled"),
                          (33, "CleanupDays"), (34, "CleanupKeepNamed"), (35, "CleanupKeepTagged"), (36, "CleanupRecycle"), (37, "CursorAsObject"),
                          (38, "SplitLongPages")] {
            set_dword(name, f[i].int() as u32);
        }
        let wav = f[21].text().trim().to_owned();
        reg::set_string(OsStr::new("CaptureSoundFile"), OsStr::new(&wav), None);
        let sound = sounds::Kind::from_index(f[20].int() as u32);
        if sound == sounds::Kind::Custom && wav.is_empty() {
            message_async(owner, tr("Escolha o arquivo WAV do som da captura (caminho completo)."), "Snapnab".into(), MB_ICONINFORMATION);
        } else if sound != sounds::Kind::Off {
            sounds::play(sound, &wav); // a sample of the sound
        }
        send(UiEv::Reload);
        let lang = lang_codes[f[14].int().clamp(0, lang_codes.len() as i32 - 1) as usize].clone();
        if lang != i18n::configured() {
            i18n::set_configured(&lang);
            // the strings are loaded when each program starts (like the reference's resource DLL)
            if i18n::resolve() != i18n::current()
                && message(owner, &tr("O novo idioma vale depois que o Snapnab e o Editor reiniciarem. Reiniciar agora?"), "Snapnab",
                           MB_YESNO | MB_ICONQUESTION) == IDYES
            {
                send(UiEv::RestartApps);
            }
        }
    });
}

// ---------------------------------------------------------------- the outputs and the fixed region

pub fn file_output(owner: isize, index: usize, p: &Preset) {
    let (fmt, prefix, folder) = (p.file_format as i32, p.file_prefix.clone(),
                                 if p.folder.is_empty() { crate::engine::default_save_dir() } else { p.folder.clone() });
    std::thread::spawn(move || {
        let formats: Vec<String> = ["PNG", "JPEG", "BMP", "GIF", "TIFF", "WebP", "AVIF"].iter().map(|s| s.to_string()).collect();
        let mut f = [combo(&tr("Formato"), &formats, fmt), text(&tr("Prefixo do nome"), &prefix), text(&tr("Pasta"), &folder),
                     Field::Label(tr("Nome automático: <Prefixo><Número>, ex.: Captura001.png"))];
        let validate = |f: &[Field]| -> String {
            let pre = f[1].text();
            if pre.is_empty() || pre.contains(['\\', '/', ':', '*', '?', '"', '<', '>', '|']) { tr("O prefixo contém caracteres inválidos.") } else { String::new() }
        };
        if !form::run_ex(owner, &tr("Salvar em arquivo"), &mut f, 420, Some(&validate)) {
            return;
        }
        let (fmt, prefix, folder) = (FileFormat::from_index(f[0].int()), f[1].text().to_owned(), f[2].text().to_owned());
        if fmt == FileFormat::Avif && !snapcore::codec::can_encode(snapcore::codec::Format::Avif) {
            crate::engine::message_box(snapcore::codec::avif_unavailable_text(), "Snapnab".into());
            return;
        }
        edit(move |s| {
            if let Some(p) = s.presets.get_mut(index) {
                (p.file_format, p.file_prefix, p.folder) = (fmt, prefix, folder);
            }
        });
    });
}

pub fn program_output(owner: isize, index: usize, p: &Preset) {
    let prog = p.program_path.clone();
    std::thread::spawn(move || {
        let mut f = [text(&tr("Programa"), &prog), Field::Label(tr("A captura é aberta nesse programa (ex.: mspaint.exe)."))];
        if !form::run(owner, &tr("Abrir em um programa externo"), &mut f) || f[0].text().trim().is_empty() {
            return;
        }
        let prog = f[0].text().trim().to_owned();
        edit(move |s| {
            if let Some(p) = s.presets.get_mut(index) {
                p.program_path = prog;
            }
        });
    });
}

pub fn powerpoint_options(owner: isize) {
    std::thread::spawn(move || {
        let mut f = [check(&tr("Cada captura num slide novo (desligado: no slide atual)"), dword("OfficeNewSlide", 1) != 0)];
        if form::run(owner, "PowerPoint", &mut f) {
            set_dword("OfficeNewSlide", f[0].int() as u32);
        }
    });
}

/// EditFixedRegion: the size and, optionally, the position on the screen; OK makes the region fixed.
pub fn fixed_region(owner: isize, index: usize, p: &Preset) {
    let p = p.clone();
    std::thread::spawn(move || {
        let vs = snapcore::screen::virtual_screen();
        let mut f = [int(&tr("Largura (px)"), p.fixed_w, 16, 20000), int(&tr("Altura (px)"), p.fixed_h, 16, 20000),
                     check(&tr("Capturar sempre na mesma posição"), p.region_use_pos),
                     int(&tr("Esquerda (X)"), p.region_x, vs.left, vs.right - 16), int(&tr("Topo (Y)"), p.region_y, vs.top, vs.bottom - 16),
                     Field::Label(tr("Sem posição fixa, você arrasta o retângulo até o lugar na hora da captura."))];
        if !form::run(owner, &tr("Região fixa"), &mut f) {
            return;
        }
        let v: Vec<i32> = f.iter().map(Field::int).collect();
        edit(move |s| {
            if let Some(p) = s.presets.get_mut(index) {
                (p.fixed_w, p.fixed_h, p.region_use_pos, p.region_x, p.region_y) = (v[0], v[1], v[2] != 0, v[3], v[4]);
                p.region_fixed = true;
                p.selection = Selection::Region;
            }
        });
    });
}

/// EditVideoOptions: the preset's frame rate, encoding and audio; the recording overlays for every video preset.
pub fn video_options(owner: isize, index: usize, p: &Preset) {
    let p = p.clone();
    std::thread::spawn(move || {
        const FPS: [i32; 5] = [10, 15, 24, 30, 60];
        let fi = FPS.iter().position(|&x| x == p.fps).unwrap_or(3) as i32;
        let n = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<String>>();
        use windows::Win32::UI::Input::KeyboardAndMouse::MOD_SHIFT;
        let mut f = vec![
            combo(&tr("Quadros por segundo"), &n(&["10", "15", "24", "30", "60"]), fi),
            check(&tr("Reduzir para 1080p"), p.downscale_1080),
            check(&tr("Codificação por hardware"), p.hardware_encode),
            Field::Separator(tr("Áudio")),
            int(&tr("Volume do microfone (%)"), p.mic_volume, 0, 200),
            int(&tr("Volume do áudio do sistema (%)"), p.sys_volume, 0, 200),
            check(&tr("Ganho automático do microfone"), p.auto_gain),
            Field::Label(tr("Também ajustáveis no painel da gravação.")),
            check(&tr("Gravar somente o áudio (sem vídeo)"), p.audio_only),
            combo(&tr("Formato do áudio"), &n(&["M4A (AAC)", "MP3"]), p.audio_format),
            Field::Separator(tr("Na gravação (todos os presets)")),
            check(&tr("Destacar o cursor"), dword("RecHighlightCursor", 0) != 0),
            Field::Color { label: tr("Cor do destaque"), color: dword("RecHighlightColor", 0x0000D6FF) },
            int(&tr("Tamanho do destaque (px)"), dword("RecHighlightSize", 44) as i32, 16, 200),
            check(&tr("Mostrar os cliques"), dword("RecClickEffects", 0) != 0),
            Field::Color { label: tr("Cor do clique esquerdo"), color: dword("RecLeftClickColor", 0x002D32E5) },
            Field::Color { label: tr("Cor do clique direito"), color: dword("RecRightClickColor", 0x00D77800) },
            combo(&tr("Teclas na tela"), &[tr("Não mostrar"), tr("Só atalhos (Ctrl, Alt, Win, F1…)"), tr("Todas (inclusive o que for digitado)")],
                  dword("RecKeyMode", 0).min(2) as i32),
            check(&tr("Ampliar ao clicar (zoom automático)"), dword("RecAutoZoom", 0) != 0),
            int(&tr("Ampliação do zoom (%)"), dword("RecZoomPercent", 200) as i32, 120, 400),
            hotkey(&tr("Atalho do zoom durante a gravação"), dword("VideoZoomKey", pack(MOD_SHIFT.0, 0x7A))),
            check(&tr("Proteger contra travamentos (recupera a gravação)"), dword("RecCrashSafe", 1) != 0),
            check(&tr("Confirmar antes de recomeçar a gravação"), dword("ConfirmRestart", 1) != 0),
        ];
        if !form::run(owner, &tr("Opções de vídeo"), &mut f) {
            return;
        }
        for (i, name) in [(11, "RecHighlightCursor"), (13, "RecHighlightSize"), (14, "RecClickEffects"), (17, "RecKeyMode"), (18, "RecAutoZoom"),
                          (19, "RecZoomPercent"), (21, "RecCrashSafe"), (22, "ConfirmRestart")] {
            set_dword(name, f[i].int() as u32);
        }
        for (i, name) in [(12, "RecHighlightColor"), (15, "RecLeftClickColor"), (16, "RecRightClickColor")] {
            set_dword(name, f[i].color());
        }
        let (zm, zv) = f[20].hotkey();
        set_dword("VideoZoomKey", pack(zm, zv));
        let v: Vec<i32> = f.iter().map(Field::int).collect();
        edit(move |s| {
            if let Some(p) = s.presets.get_mut(index) {
                p.fps = FPS[v[0].clamp(0, 4) as usize];
                (p.downscale_1080, p.hardware_encode) = (v[1] != 0, v[2] != 0);
                (p.mic_volume, p.sys_volume, p.auto_gain) = (v[4], v[5], v[6] != 0);
                (p.audio_only, p.audio_format) = (v[8] != 0, v[9]);
            }
        });
    });
}

/// EditChromaKey: the webcam's background removal.
pub fn chroma_key(owner: isize, index: usize, p: &Preset) {
    let p = p.clone();
    std::thread::spawn(move || {
        let mut f = [check(&tr("Remover o fundo da webcam"), p.chroma_key), Field::Color { label: tr("Cor do fundo"), color: p.chroma_color },
                     int(&tr("Tolerância (0–100)"), p.chroma_tolerance, 0, 100),
                     Field::Label(tr("Use um fundo liso e bem iluminado (tela verde é o ideal).")),
                     Field::Label(tr("Com o fundo removido, a webcam aparece sem moldura."))];
        if !form::run(owner, &tr("Remover o fundo (chroma key)"), &mut f) {
            return;
        }
        let (on, color, tol) = (f[0].on(), f[1].color(), f[2].int());
        edit(move |s| {
            if let Some(p) = s.presets.get_mut(index) {
                (p.chroma_key, p.chroma_color, p.chroma_tolerance) = (on, color, tol);
            }
        });
    });
}

// ---------------------------------------------------------------- effects (EditFx)

/// EditFx: the settings of effect k of the preset; OK turns it on.
pub fn edit_fx(owner: isize, index: usize, k: usize, p: &Preset) {
    let e = p.effects.clone();
    std::thread::spawn(move || {
        let w = |v: &[u16]| String::from_utf16_lossy(v);
        let n = |v: &[&str]| v.iter().map(|s| tr(s)).collect::<Vec<String>>();
        let color = |label: &str, c: u32| Field::Color { label: tr(label), color: c };
        let o = &e.beautify_opt;
        let mut f: Vec<Field> = match k {
            0 => vec![int(&tr("Largura (px)"), e.border_width, 1, 100), color("Cor", e.border_color)],
            1 => vec![combo(&tr("Filtro"), &n(&["Tons de cinza", "Sépia", "Inverter cores", "Preto e branco"]), e.filter_kind)],
            2 => vec![int(&tr("Deslocamento (px)"), e.shadow_offset, 0, 50), int(&tr("Desfoque (px)"), e.shadow_blur, 0, 50),
                      int(&tr("Opacidade (%)"), e.shadow_opacity, 0, 100)],
            3 => vec![text(&tr("Texto"), &w(&e.caption_text)), Field::Label(tr("Campos: {date} {time} {computer} {user}")),
                      combo(&tr("Posição"), &n(&["Embaixo", "Em cima"]), e.caption_top as i32)],
            4 => vec![color("Cor a substituir", e.replace_from), color("Nova cor", e.replace_to), int(&tr("Tolerância"), e.replace_tolerance, 0, 100)],
            5 => vec![int(&tr("Escala (%)"), e.scale_percent, 10, 400)],
            6 => {
                use snapcore::fx::{BOTTOM, LEFT, RIGHT, TOP};
                vec![combo(&tr("Estilo"), &n(&["Rasgada", "Onda", "Dente de serra", "Dente de tubarão", "Esmaecer", "Chanfrada"]), e.edge_style),
                     int(&tr("Tamanho (px)"), e.edge_size, 4, 60), check(&tr("Em cima"), e.edge_sides & TOP != 0),
                     check(&tr("Direita"), e.edge_sides & RIGHT != 0), check(&tr("Embaixo"), e.edge_sides & BOTTOM != 0),
                     check(&tr("Esquerda"), e.edge_sides & LEFT != 0)]
            }
            7 => vec![Field::Label(tr("Remove as faixas de cor uniforme em volta da captura.")), int(&tr("Tolerância"), e.trim_tolerance, 0, 100)],
            8 => vec![int(&tr("Brilho"), e.brightness, -100, 100), int(&tr("Contraste"), e.contrast, -100, 100),
                      int(&tr("Matiz (graus)"), e.hue, -180, 180), int(&tr("Saturação"), e.saturation, -100, 100)],
            9 => vec![combo(&tr("Girar"), &[tr("90° à direita"), tr("90° à esquerda"), "180°".into(), tr("Espelhar na horizontal"), tr("Espelhar na vertical")],
                            e.rotate_kind)],
            10 => vec![text(&tr("Texto"), &w(&e.watermark_text)), int(&tr("Opacidade (%)"), e.watermark_opacity, 5, 100),
                       check(&tr("Repetir por toda a imagem"), e.watermark_tiled)],
            11 => vec![int(&tr("Resolução (DPI)"), e.dpi, 36, 2400), Field::Label(tr("Gravada nos arquivos salvos (PNG, JPEG, TIFF, BMP)."))],
            _ => vec![combo(&tr("Fundo"), &n(&["Cor sólida", "Degradê", "Papel de parede", "Imagem", "Transparente"]), o.background),
                      color("Cor 1", o.color1), color("Cor 2 (degradê)", o.color2), int(&tr("Ângulo do degradê"), o.angle, 0, 359),
                      text(&tr("Imagem de fundo"), &w(&o.image_path)), int(&tr("Margem (px)"), o.padding, 0, 1000),
                      int(&tr("Cantos arredondados (px)"), o.radius, 0, 200), int(&tr("Sombra (%)"), o.shadow, 0, 100),
                      int(&tr("Tamanho da sombra (px)"), o.shadow_size, 0, 200),
                      combo(&tr("Proporção"), &[tr("Original"), "16:9".into(), "4:3".into(), "1:1".into(), "9:16".into()], o.aspect),
                      combo(&tr("Alinhamento"),
                            &n(&["Superior esquerdo", "Superior", "Superior direito", "Esquerda", "Centro", "Direita", "Inferior esquerdo",
                                 "Inferior", "Inferior direito"]), o.align),
                      check(&tr("Equilibrar as margens (corta bordas lisas da captura)"), o.auto_balance)],
        };
        let title = if k == 12 { tr("Embelezar") } else { tr(crate::window_fx_name(k)) };
        if !form::run(owner, &title, &mut f) {
            return;
        }
        let v: Vec<i32> = f.iter().map(Field::int).collect();
        let c: Vec<u32> = f.iter().map(Field::color).collect();
        let t: Vec<Vec<u16>> = f.iter().map(|x| x.text().encode_utf16().collect()).collect();
        edit(move |s| {
            let Some(p) = s.presets.get_mut(index) else { return };
            let e = &mut p.effects;
            match k {
                0 => (e.border_width, e.border_color) = (v[0], c[1]),
                1 => e.filter_kind = v[0],
                2 => (e.shadow_offset, e.shadow_blur, e.shadow_opacity) = (v[0], v[1], v[2]),
                3 => (e.caption_text, e.caption_top) = (t[0].clone(), v[2] == 1),
                4 => (e.replace_from, e.replace_to, e.replace_tolerance) = (c[0], c[1], v[2]),
                5 => e.scale_percent = v[0],
                6 => {
                    use snapcore::fx::{BOTTOM, LEFT, RIGHT, TOP};
                    (e.edge_style, e.edge_size) = (v[0], v[1]);
                    let sides = [(2, TOP), (3, RIGHT), (4, BOTTOM), (5, LEFT)].iter().filter(|(i, _)| v[*i] != 0).fold(0, |a, (_, b)| a | b);
                    e.edge_sides = if sides == 0 { BOTTOM | RIGHT } else { sides };
                }
                7 => e.trim_tolerance = v[1],
                8 => (e.brightness, e.contrast, e.hue, e.saturation) = (v[0], v[1], v[2], v[3]),
                9 => e.rotate_kind = v[0],
                10 => (e.watermark_text, e.watermark_opacity, e.watermark_tiled) = (t[0].clone(), v[1], v[2] != 0),
                11 => e.dpi = v[0],
                _ => {
                    let o = &mut e.beautify_opt;
                    (o.background, o.color1, o.color2, o.angle) = (v[0], c[1], c[2], v[3]);
                    o.image_path = t[4].iter().copied().collect::<Vec<u16>>();
                    (o.padding, o.radius, o.shadow, o.shadow_size, o.aspect, o.align, o.auto_balance) = (v[5], v[6], v[7], v[8], v[9], v[10], v[11] != 0);
                }
            }
            *snapnab_app::window::fx_on(e, k) = true;
        });
    });
}
