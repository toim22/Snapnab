//! Snapnab's user interface in egui (immediate mode: the UI is a function called every frame, which reads the state
//! and draws the widgets, instead of a tree of window objects kept in sync).
//!
//! - `host`: the windows (winit), drawn by egui on the GPU (wgpu), with their Win32 classes and messages
//! - the fonts and the text check of the scripts Snapnab ships

use std::sync::{Arc, OnceLock};

use egui::{FontData, FontDefinitions, FontFamily};

pub mod form;
pub mod host;
pub mod icons;
pub mod qr;
pub mod upload_ui;

/// Windows' own fonts, so every language Snapnab ships has its letters: Segoe UI (Latin, Greek, Cyrillic, Arabic,
/// Hebrew), Nirmala UI (Hindi, Bengali and the other Indic scripts), Segoe UI Symbol and Segoe UI Emoji.
pub fn install_fonts(ctx: &egui::Context) {
    static DEFS: OnceLock<FontDefinitions> = OnceLock::new();
    ctx.set_fonts(DEFS.get_or_init(load_fonts).clone());
}

/// The font files are read once per process; every window's Context shares them.
fn load_fonts() -> FontDefinitions {
    let mut defs = FontDefinitions::default();
    let dir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into()) + r"\Fonts\";
    let mut names = Vec::new();
    for (name, file, index) in [
        ("Segoe UI", "segoeui.ttf", 0),
        ("Nirmala UI", "Nirmala.ttc", 0),
        ("Segoe UI Symbol", "seguisym.ttf", 0),
        ("Segoe UI Emoji", "seguiemj.ttf", 0),
        ("Consolas", "consola.ttf", 0),
    ] {
        let Ok(bytes) = std::fs::read(dir.clone() + file) else { continue };
        let mut data = FontData::from_owned(bytes);
        data.index = index;
        defs.font_data.insert(name.to_owned(), Arc::new(data));
        names.push(name.to_owned());
    }
    // Consolas first for monospace, Segoe UI first for the rest; the other fonts are fallbacks for missing letters
    let (mono, text): (Vec<_>, Vec<_>) = names.into_iter().partition(|n| n == "Consolas");
    let prop = defs.families.entry(FontFamily::Proportional).or_default();
    prop.splice(0..0, text.iter().cloned());
    let monospace = defs.families.entry(FontFamily::Monospace).or_default();
    monospace.splice(0..0, mono.into_iter().chain(text));
    defs
}

/// Sample lines in the scripts that need shaping: Arabic and Urdu (right to left, joined letters) and Hindi and
/// Bengali (conjuncts and vowel signs).
pub const SCRIPT_SAMPLES: [(&str, &str); 5] = [
    ("pt", "Capturar a tela e salvar a imagem"),
    ("ar", "التقاط الشاشة وحفظ الصورة"),
    ("ur", "اسکرین کیپچر کریں"),
    ("hi", "स्क्रीन कैप्चर करें"),
    ("bn", "স্ক্রিন ক্যাপচার করুন"),
];

pub fn script_probe(ui: &mut egui::Ui) {
    for (lang, text) in SCRIPT_SAMPLES {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(lang).monospace());
            ui.label(egui::RichText::new(text).size(44.0));
        });
    }
}
