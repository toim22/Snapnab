//! The capture presets (Presets.cpp, comp_7 §2): what a capture takes (image or video, the selection mode), its
//! effects and where it goes. They live in %LOCALAPPDATA%\Snapnab\Presets1.xml as <SavedPresets>, one <CaptureSettings>
//! per preset, with the same elements and attributes the C++ capture app wrote (tests/data/presets-cpp.txt).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::beautify::BeautifyOptions;
use crate::color::{color_to_hex, hex_to_color, rgb};
use crate::fx::CaptureEffects;
use crate::xml::{self, Node};
use crate::{crt, i18n, paths, reg};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CaptureKind {
    #[default]
    Image,
    Video,
}

/// The selection modes, numbered as SelectionMode in the file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum Selection {
    #[default]
    Region = 0,
    Window,
    FullScreen,
    Monitor,
    FixedRegion,
    Freehand,
    MultiArea,
    Object,
    Scrolling,
    ExtendedWindow,
    Menu,
    Clipboard,
    File,
    Panoramic,
}

impl Selection {
    pub const COUNT: i32 = 14;

    pub fn from_index(i: i32) -> Selection {
        use Selection::*;
        const ALL: [Selection; 14] = [Region, Window, FullScreen, Monitor, FixedRegion, Freehand, MultiArea, Object, Scrolling,
                                      ExtendedWindow, Menu, Clipboard, File, Panoramic];
        ALL[i.clamp(0, Self::COUNT - 1) as usize]
    }

    /// The name in the interface language.
    pub fn name(self) -> String {
        use Selection::*;
        i18n::tr_str(match self {
            Region => "Região",
            Window => "Janela",
            FullScreen => "Tela inteira (todos os monitores)",
            Monitor => "Monitor atual",
            FixedRegion => "Região com tamanho fixo",
            Freehand => "Mão livre",
            MultiArea => "Várias áreas",
            Object => "Objeto (controle sob o cursor)",
            Scrolling => "Rolagem automática (janela)",
            Panoramic => "Rolagem panorâmica",
            ExtendedWindow => "Janela estendida",
            Menu => "Menu",
            Clipboard => "Área de transferência",
            File => "Arquivo gráfico",
        })
    }

    /// The modes offered for a kind of capture (the fixed region is an option of Region, not an item of the list).
    pub fn for_kind(k: CaptureKind) -> &'static [Selection] {
        use Selection::*;
        match k {
            CaptureKind::Video => &[Region, Window, FullScreen, Monitor],
            CaptureKind::Image => &[Region, Window, FullScreen, Monitor, Freehand, MultiArea, Object, Panoramic, Scrolling,
                                    ExtendedWindow, Menu, Clipboard, File],
        }
    }
}

/// The file formats, numbered as OutputFileType (new ones at the end).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum FileFormat {
    #[default]
    Png = 0,
    Jpeg,
    Bmp,
    Gif,
    Tiff,
    Webp,
    Avif,
}

impl FileFormat {
    pub fn from_index(i: i32) -> FileFormat {
        use FileFormat::*;
        [Png, Jpeg, Bmp, Gif, Tiff, Webp, Avif][i.clamp(0, 6) as usize]
    }

    pub fn ext(self) -> &'static str {
        match self {
            FileFormat::Jpeg => ".jpg",
            FileFormat::Bmp => ".bmp",
            FileFormat::Gif => ".gif",
            FileFormat::Tiff => ".tif",
            FileFormat::Webp => ".webp",
            FileFormat::Avif => ".avif",
            FileFormat::Png => ".png",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Preset {
    pub name: String,
    pub kind: CaptureKind,
    pub selection: Selection,
    pub fixed_w: i32,
    pub fixed_h: i32,
    pub region_fixed: bool,
    pub region_use_pos: bool,
    pub region_x: i32,
    pub region_y: i32,
    pub capture_cursor: bool,
    pub delay_seconds: i32,
    /// full screen / monitor: each window as a separate object in the Editor
    pub window_objects: bool,
    /// keeps the links of web pages and documents as clickable areas (UI Automation)
    pub capture_links: bool,
    pub preview_in_editor: bool,
    pub copy_to_clipboard: bool,
    pub save_to_file: bool,
    pub file_format: FileFormat,
    pub file_prefix: String,
    /// empty = Pictures\Snapnab
    pub folder: String,
    pub open_in_program: bool,
    pub program_path: String,
    /// OCR of the capture into a text window and the clipboard
    pub grab_text: bool,
    pub pin_to_screen: bool,
    pub read_qr: bool,
    /// Word, PowerPoint, Excel or OneNote (0–3); -1 = none
    pub office: i32,
    pub upload: bool,
    /// command line /output file: the exact file (never saved in the preset)
    pub output_path: String,
    /// command line /output clipboard: no Editor, no overlay (never saved)
    pub output_clipboard_only: bool,
    pub effects: CaptureEffects,
    pub microphone: bool,
    pub system_audio: bool,
    pub mic_volume: i32,
    pub sys_volume: i32,
    pub auto_gain: bool,
    pub fps: i32,
    /// 0 very low … 3 high
    pub quality: i32,
    pub downscale_1080: bool,
    pub hardware_encode: bool,
    pub audio_only: bool,
    /// 0 M4A (AAC), 1 MP3
    pub audio_format: i32,
    pub webcam: bool,
    pub webcam_device: String,
    pub webcam_name: String,
    /// circle, rounded square, square, 16:9, 4:3
    pub webcam_shape: i32,
    pub chroma_key: bool,
    pub chroma_color: u32,
    pub chroma_tolerance: i32,
    /// presets that ship with the program ("panoramic", "grabtext")
    pub builtin: String,
    pub hotkey_mods: u32,
    pub hotkey_vk: u32,
}

impl Default for Preset {
    fn default() -> Self {
        Preset {
            name: "Novo preset".into(),
            kind: CaptureKind::Image,
            selection: Selection::Region,
            fixed_w: 640,
            fixed_h: 480,
            region_fixed: false,
            region_use_pos: false,
            region_x: 0,
            region_y: 0,
            capture_cursor: false,
            delay_seconds: 0,
            window_objects: false,
            capture_links: true,
            preview_in_editor: true,
            copy_to_clipboard: false,
            save_to_file: false,
            file_format: FileFormat::Png,
            file_prefix: "Captura".into(),
            folder: String::new(),
            open_in_program: false,
            program_path: "mspaint.exe".into(),
            grab_text: false,
            pin_to_screen: false,
            read_qr: false,
            office: -1,
            upload: false,
            output_path: String::new(),
            output_clipboard_only: false,
            effects: CaptureEffects::default(),
            microphone: true,
            system_audio: true,
            mic_volume: 100,
            sys_volume: 100,
            auto_gain: true,
            fps: 30,
            quality: 2,
            downscale_1080: true,
            hardware_encode: true,
            audio_only: false,
            audio_format: 0,
            webcam: false,
            webcam_device: String::new(),
            webcam_name: String::new(),
            webcam_shape: 0,
            chroma_key: false,
            chroma_color: rgb(0, 177, 64),
            chroma_tolerance: 40,
            builtin: String::new(),
            hotkey_mods: 0,
            hotkey_vk: 0,
        }
    }
}

// ---------------------------------------------------------------- attribute values as XmlNode reads and writes them
fn geti(n: &Node, k: &str, def: i32) -> i32 {
    n.attr(k).map(crt::atoi).unwrap_or(def)
}
fn getb(n: &Node, k: &str, def: bool) -> bool {
    geti(n, k, def as i32) != 0
}
fn gets(n: &Node, k: &str, def: &str) -> String {
    n.attr(k).unwrap_or(def).to_owned()
}
fn getw(n: &Node, k: &str, def: &[u16]) -> Vec<u16> {
    n.attr(k).map_or_else(|| def.to_vec(), |v| v.encode_utf16().collect())
}
fn getc(n: &Node, k: &str, def: u32) -> u32 {
    let w: Vec<u16> = n.attr(k).unwrap_or("").encode_utf16().collect();
    hex_to_color(&w, def)
}
trait Set {
    fn i(&mut self, k: &str, v: impl Into<i64>) -> &mut Self;
    fn s(&mut self, k: &str, v: &str) -> &mut Self;
    fn w(&mut self, k: &str, v: &[u16]) -> &mut Self;
    fn c(&mut self, k: &str, v: u32) -> &mut Self;
}
impl Set for Node {
    fn i(&mut self, k: &str, v: impl Into<i64>) -> &mut Self {
        self.set(k, v.into().to_string())
    }
    fn s(&mut self, k: &str, v: &str) -> &mut Self {
        self.set(k, v)
    }
    fn w(&mut self, k: &str, v: &[u16]) -> &mut Self {
        self.set(k, String::from_utf16_lossy(v))
    }
    fn c(&mut self, k: &str, v: u32) -> &mut Self {
        self.set(k, color_to_hex(v))
    }
}

/// BeautifyOptions::ToXml.
pub fn beautify_to_xml(o: &BeautifyOptions, name: &str) -> Node {
    let mut n = Node::new(name);
    n.i("Background", o.background).c("Color1", o.color1).c("Color2", o.color2).i("Angle", o.angle).w("Image", &o.image_path)
     .i("Padding", o.padding).i("Radius", o.radius).i("Shadow", o.shadow).i("ShadowSize", o.shadow_size).i("Align", o.align)
     .i("Aspect", o.aspect).i("AutoBalance", o.auto_balance);
    n
}

/// BeautifyOptions::FromXml.
pub fn beautify_from_xml(n: &Node) -> BeautifyOptions {
    let d = BeautifyOptions::default();
    BeautifyOptions {
        background: geti(n, "Background", d.background).clamp(0, 4),
        color1: getc(n, "Color1", d.color1),
        color2: getc(n, "Color2", d.color2),
        angle: geti(n, "Angle", d.angle) % 360,
        image_path: getw(n, "Image", &[]),
        padding: geti(n, "Padding", d.padding).clamp(0, 1000),
        radius: geti(n, "Radius", d.radius).clamp(0, 200),
        shadow: geti(n, "Shadow", d.shadow).clamp(0, 100),
        shadow_size: geti(n, "ShadowSize", d.shadow_size).clamp(0, 200),
        align: geti(n, "Align", d.align).clamp(0, 8),
        aspect: geti(n, "Aspect", d.aspect).clamp(0, 4),
        auto_balance: getb(n, "AutoBalance", d.auto_balance),
    }
}

impl Preset {
    pub fn to_xml(&self) -> Node {
        let mut n = Node::new("CaptureSettings");
        n.s("DisplayName", &self.name).s("CaptureMode", if self.kind == CaptureKind::Video { "Video" } else { "Image" });
        n.i("SelectionMode", self.selection as i32);
        n.i("RegionWidth", self.fixed_w).i("RegionHeight", self.fixed_h);
        n.add("SelectionSettingsRegion").i("RegionSelectionType", self.region_fixed).i("RegionLeft", self.region_x)
         .i("RegionTop", self.region_y).i("SetPosition", self.region_use_pos);
        n.i("CaptureCursor", self.capture_cursor).i("DelayDuration", self.delay_seconds);
        n.i("WindowsAsObjects", self.window_objects).i("CaptureLinks", self.capture_links);
        n.i("PreviewInEditor", self.preview_in_editor).i("CopyToClipboard", self.copy_to_clipboard);
        n.add("ShareSettingsFile").i("Enabled", self.save_to_file).i("OutputFileType", self.file_format as i32)
         .s("Prefix", &self.file_prefix).s("FolderName", &self.folder);
        n.add("ShareSettingsProgram").i("Enabled", self.open_in_program).s("Program", &self.program_path);
        n.add("ShareSettingsGrabText").i("Enabled", self.grab_text);
        n.add("ShareSettingsPin").i("Enabled", self.pin_to_screen);
        n.add("ShareSettingsQRCode").i("Enabled", self.read_qr);
        n.add("ShareSettingsOffice").i("Target", self.office);
        n.add("ShareSettingsUpload").i("Enabled", self.upload);
        if !self.builtin.is_empty() {
            n.s("Builtin", &self.builtin);
        }
        let f = &self.effects;
        let e = n.add("CaptureEffects");
        e.i("Border", f.border).i("BorderWidth", f.border_width).c("BorderColor", f.border_color);
        e.i("Filter", f.filter).i("FilterKind", f.filter_kind);
        e.i("Shadow", f.shadow).i("ShadowOffset", f.shadow_offset).i("ShadowBlur", f.shadow_blur)
         .i("ShadowOpacity", f.shadow_opacity);
        e.i("Caption", f.caption).w("CaptionText", &f.caption_text).i("CaptionTop", f.caption_top);
        e.i("ReplaceColor", f.replace_color).c("ReplaceFrom", f.replace_from).c("ReplaceTo", f.replace_to)
         .i("ReplaceTolerance", f.replace_tolerance);
        e.i("ScaleEnabled", f.scale).i("Scale", f.scale_percent);
        e.i("Edge", f.edge).i("EdgeStyle", f.edge_style).i("EdgeSize", f.edge_size).i("EdgeSides", f.edge_sides);
        e.i("Trim", f.trim).i("TrimTolerance", f.trim_tolerance);
        e.i("Adjust", f.adjust).i("Brightness", f.brightness).i("Contrast", f.contrast).i("Hue", f.hue)
         .i("Saturation", f.saturation);
        e.i("Rotate", f.rotate).i("RotateKind", f.rotate_kind);
        e.i("Watermark", f.watermark).w("WatermarkText", &f.watermark_text).i("WatermarkOpacity", f.watermark_opacity)
         .i("WatermarkTiled", f.watermark_tiled);
        e.i("Resolution", f.resolution).i("Dpi", f.dpi);
        e.i("Beautify", f.beautify);
        e.children.push(beautify_to_xml(&f.beautify_opt, "Beautify"));
        n.add("Video").i("EnableMicrophone", self.microphone).i("EnableSystemAudio", self.system_audio).i("FrameRate", self.fps)
         .i("MicrophoneVolume", self.mic_volume).i("SystemAudioVolume", self.sys_volume).i("AutomaticGain", self.auto_gain)
         .i("Quality", self.quality).i("Downscale1080", self.downscale_1080).i("HardwareEncoding", self.hardware_encode)
         .i("EnableWebcam", self.webcam).s("SelectedWebcam", &self.webcam_device).s("SelectedWebcamName", &self.webcam_name)
         .i("WebcamShape", self.webcam_shape).i("ChromaKey", self.chroma_key).c("ChromaColor", self.chroma_color)
         .i("ChromaTolerance", self.chroma_tolerance).i("AudioOnly", self.audio_only).i("AudioFormat", self.audio_format);
        n.i("HotKey", self.hotkey_vk).i("HotKeyModifiers", self.hotkey_mods);
        n
    }

    pub fn from_xml(n: &Node) -> Preset {
        let mut p = Preset::default();
        p.name = gets(n, "DisplayName", &p.name);
        p.kind = if n.attr("CaptureMode") == Some("Video") { CaptureKind::Video } else { CaptureKind::Image };
        p.selection = Selection::from_index(geti(n, "SelectionMode", 0));
        p.fixed_w = geti(n, "RegionWidth", 640);
        p.fixed_h = geti(n, "RegionHeight", 480);
        if let Some(rs) = n.child("SelectionSettingsRegion") {
            p.region_fixed = getb(rs, "RegionSelectionType", false);
            p.region_use_pos = getb(rs, "SetPosition", false);
            p.region_x = geti(rs, "RegionLeft", 0);
            p.region_y = geti(rs, "RegionTop", 0);
        }
        if p.selection == Selection::FixedRegion {
            // old presets: the fixed region was a selection mode of its own
            p.selection = Selection::Region;
            p.region_fixed = true;
        }
        p.capture_cursor = getb(n, "CaptureCursor", false);
        p.delay_seconds = geti(n, "DelayDuration", 0);
        p.window_objects = getb(n, "WindowsAsObjects", false);
        p.capture_links = getb(n, "CaptureLinks", true);
        p.preview_in_editor = getb(n, "PreviewInEditor", true);
        p.copy_to_clipboard = getb(n, "CopyToClipboard", false);
        if let Some(sh) = n.child("ShareSettingsFile") {
            p.save_to_file = getb(sh, "Enabled", false);
            p.file_format = FileFormat::from_index(geti(sh, "OutputFileType", 0));
            p.file_prefix = gets(sh, "Prefix", &p.file_prefix);
            p.folder = gets(sh, "FolderName", "");
        }
        if let Some(pr) = n.child("ShareSettingsProgram") {
            p.open_in_program = getb(pr, "Enabled", false);
            p.program_path = gets(pr, "Program", &p.program_path);
        }
        if let Some(x) = n.child("ShareSettingsGrabText") {
            p.grab_text = getb(x, "Enabled", false);
        }
        if let Some(x) = n.child("ShareSettingsPin") {
            p.pin_to_screen = getb(x, "Enabled", false);
        }
        if let Some(x) = n.child("ShareSettingsQRCode") {
            p.read_qr = getb(x, "Enabled", false);
        }
        if let Some(x) = n.child("ShareSettingsOffice") {
            p.office = geti(x, "Target", -1).clamp(-1, 3);
        }
        if let Some(x) = n.child("ShareSettingsUpload") {
            p.upload = getb(x, "Enabled", false);
        }
        p.builtin = gets(n, "Builtin", "");
        if let Some(e) = n.child("CaptureEffects") {
            let f = &mut p.effects;
            f.border = getb(e, "Border", false);
            f.border_width = geti(e, "BorderWidth", 4);
            f.border_color = getc(e, "BorderColor", f.border_color);
            f.filter = getb(e, "Filter", getb(e, "Grayscale", false)); // "Grayscale" = old presets
            f.filter_kind = geti(e, "FilterKind", 0).clamp(0, 3);
            f.shadow = getb(e, "Shadow", false);
            f.shadow_offset = geti(e, "ShadowOffset", 6).clamp(0, 50);
            f.shadow_blur = geti(e, "ShadowBlur", 6).clamp(0, 50);
            f.shadow_opacity = geti(e, "ShadowOpacity", 50).clamp(0, 100);
            f.caption = getb(e, "Caption", false);
            f.caption_text = getw(e, "CaptionText", &f.caption_text);
            f.caption_top = getb(e, "CaptionTop", false);
            f.replace_color = getb(e, "ReplaceColor", false);
            f.replace_from = getc(e, "ReplaceFrom", f.replace_from);
            f.replace_to = getc(e, "ReplaceTo", f.replace_to);
            f.replace_tolerance = geti(e, "ReplaceTolerance", 10).clamp(0, 100);
            f.scale_percent = geti(e, "Scale", 100).clamp(10, 400);
            f.scale = getb(e, "ScaleEnabled", f.scale_percent != 100);
            f.edge = getb(e, "Edge", false);
            f.edge_style = geti(e, "EdgeStyle", 0).clamp(0, 5);
            f.edge_size = geti(e, "EdgeSize", 14).clamp(4, 60);
            f.edge_sides = geti(e, "EdgeSides", 6) & 15;
            f.trim = getb(e, "Trim", false);
            f.trim_tolerance = geti(e, "TrimTolerance", 8).clamp(0, 100);
            f.adjust = getb(e, "Adjust", false);
            f.brightness = geti(e, "Brightness", 0).clamp(-100, 100);
            f.contrast = geti(e, "Contrast", 0).clamp(-100, 100);
            f.hue = geti(e, "Hue", 0).clamp(-180, 180);
            f.saturation = geti(e, "Saturation", 0).clamp(-100, 100);
            f.rotate = getb(e, "Rotate", false);
            f.rotate_kind = geti(e, "RotateKind", 0).clamp(0, 4);
            f.watermark = getb(e, "Watermark", false);
            f.watermark_text = getw(e, "WatermarkText", &f.watermark_text);
            f.watermark_opacity = geti(e, "WatermarkOpacity", 35).clamp(5, 100);
            f.watermark_tiled = getb(e, "WatermarkTiled", true);
            f.resolution = getb(e, "Resolution", false);
            f.dpi = geti(e, "Dpi", 300).clamp(36, 2400);
            f.beautify = getb(e, "Beautify", false);
            if let Some(b) = e.child("Beautify") {
                f.beautify_opt = beautify_from_xml(b);
            }
        }
        if let Some(v) = n.child("Video") {
            p.microphone = getb(v, "EnableMicrophone", true);
            p.system_audio = getb(v, "EnableSystemAudio", true);
            p.mic_volume = geti(v, "MicrophoneVolume", 100).clamp(0, 200);
            p.sys_volume = geti(v, "SystemAudioVolume", 100).clamp(0, 200);
            p.auto_gain = getb(v, "AutomaticGain", true);
            p.fps = geti(v, "FrameRate", 30).clamp(5, 60);
            p.quality = geti(v, "Quality", 2).clamp(0, 3);
            p.downscale_1080 = getb(v, "Downscale1080", true);
            p.hardware_encode = getb(v, "HardwareEncoding", true);
            p.audio_only = getb(v, "AudioOnly", false);
            p.audio_format = geti(v, "AudioFormat", 0).clamp(0, 1);
            p.webcam = getb(v, "EnableWebcam", false);
            p.webcam_device = gets(v, "SelectedWebcam", "");
            p.webcam_name = gets(v, "SelectedWebcamName", "");
            p.webcam_shape = geti(v, "WebcamShape", 0).clamp(0, 4);
            p.chroma_key = getb(v, "ChromaKey", false);
            p.chroma_color = getc(v, "ChromaColor", p.chroma_color);
            p.chroma_tolerance = geti(v, "ChromaTolerance", 40).clamp(0, 100);
        }
        p.hotkey_vk = geti(n, "HotKey", 0) as u32;
        p.hotkey_mods = geti(n, "HotKeyModifiers", 0) as u32;
        p
    }
}

// ---------------------------------------------------------------- the presets file

/// The presets of the capture app and the one selected in its list.
#[derive(Clone, Debug, Default)]
pub struct PresetStore {
    pub presets: Vec<Preset>,
    pub selected: i32,
}

fn load_from(path: &Path) -> Option<(Vec<Preset>, i32)> {
    let root = xml::load_file(path)?;
    if root.name != "SavedPresets" {
        return None;
    }
    let list = root.child("Presets").map_or_else(Vec::new, |l| {
        l.children.iter().filter(|c| c.name == "CaptureSettings").map(Preset::from_xml).collect()
    });
    Some((list, geti(&root, "SelectedMode", 0)))
}

fn make_root(presets: &[Preset], selected: i32) -> Node {
    let mut root = Node::new("SavedPresets");
    root.i("Version", 1).i("SelectedMode", selected);
    let list = root.add("Presets");
    list.children.extend(presets.iter().map(Preset::to_xml));
    root
}

const BUILTIN_PRESETS: &str = "BuiltinPresets";

impl PresetStore {
    pub fn file_path() -> PathBuf {
        paths::app_data_dir().join("Presets1.xml")
    }

    /// The presets file, or the defaults (written back) when it is missing, unreadable or empty.
    pub fn load() -> PresetStore {
        let mut s = PresetStore::default();
        match load_from(&Self::file_path()) {
            Some((list, sel)) if !list.is_empty() => {
                s.presets = list;
                s.selected = sel;
                if reg::get_dword(OsStr::new(BUILTIN_PRESETS), 0, None) < 1 {
                    s.add_builtins();
                    s.save();
                }
            }
            _ => {
                s.create_defaults();
                s.save();
            }
        }
        s.selected = s.selected.clamp(0, s.presets.len() as i32 - 1);
        s
    }

    pub fn save(&self) -> bool {
        xml::save_file(&Self::file_path(), &make_root(&self.presets, self.selected))
    }

    pub fn export(&self, path: &Path) -> bool {
        xml::save_file(path, &make_root(&self.presets, self.selected))
    }

    /// Adds the presets of a file at the end (without their hotkeys, which could conflict) and saves.
    pub fn import(&mut self, path: &Path) -> bool {
        let Some((list, _)) = load_from(path) else { return false };
        if list.is_empty() {
            return false;
        }
        self.presets.extend(list.into_iter().map(|mut p| {
            p.hotkey_mods = 0;
            p.hotkey_vk = 0;
            p
        }));
        self.save()
    }

    /// The presets of a new installation. Their names stay in Portuguese in the file and are translated on screen.
    pub fn create_defaults(&mut self) {
        self.presets = default_presets();
        self.selected = 0;
        reg::set_dword(OsStr::new(BUILTIN_PRESETS), 0, None);
        self.add_builtins();
    }

    /// "Panoramic scrolling capture" and "Capture text" at the top of the list, once: the user can delete them later
    /// without them coming back.
    fn add_builtins(&mut self) {
        if reg::get_dword(OsStr::new(BUILTIN_PRESETS), 0, None) >= 1 {
            return;
        }
        self.presets.splice(0..0, builtin_presets());
        self.selected += 2;
        reg::set_dword(OsStr::new(BUILTIN_PRESETS), 1, None);
    }

    /// The first preset of a kind (the image and video hotkeys run it), or a default one.
    pub fn first_of_kind(&self, k: CaptureKind) -> Preset {
        self.presets.iter().find(|p| p.kind == k && p.builtin.is_empty()).cloned().unwrap_or_else(|| Preset {
            kind: k,
            ..Default::default()
        })
    }
}

pub fn default_presets() -> Vec<Preset> {
    let named = |n: &str| Preset { name: n.into(), ..Default::default() };
    let mut b = named("Imagem — Janela");
    b.selection = Selection::Window;
    b.effects.shadow = true;
    let mut c = named("Tela inteira → Área de transferência");
    c.selection = Selection::FullScreen;
    c.preview_in_editor = false;
    c.copy_to_clipboard = true;
    let mut d = named("Rolagem (página inteira)");
    d.selection = Selection::Scrolling;
    let mut e = named("Vídeo — Região");
    e.kind = CaptureKind::Video;
    e.capture_cursor = true;
    let mut f = named("Vídeo — Tela inteira");
    f.kind = CaptureKind::Video;
    f.selection = Selection::Monitor;
    f.capture_cursor = true;
    vec![named("Imagem — Região"), b, c, d, e, f]
}

fn builtin_presets() -> Vec<Preset> {
    let mut pano = Preset { name: "Captura com rolagem panorâmica".into(), builtin: "panoramic".into(), ..Default::default() };
    pano.selection = Selection::Panoramic;
    let mut text = Preset { name: "Capturar texto".into(), builtin: "grabtext".into(), ..Default::default() };
    text.preview_in_editor = false;
    text.grab_text = true;
    vec![pano, text]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// tests/data/presets-cpp.txt: what the C++ capture app wrote for the same presets and read from the same XML.
    #[test]
    fn writes_and_reads_like_the_cpp_app() {
        let golden = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/data/presets-cpp.txt")).unwrap();
        let golden: Vec<&str> = golden.lines().collect();
        let mut out: Vec<String> = Vec::new();
        let mut put = |label: &str, n: &Node| {
            out.push(format!("== {label}"));
            out.extend(xml::serialize(n, false).lines().map(str::to_owned));
        };
        put("default", &Preset::default().to_xml());
        let w = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
        let mut p = Preset {
            name: "Região ç → 中文 😀 \"<&>\"".into(),
            kind: CaptureKind::Video,
            selection: Selection::Monitor,
            fixed_w: 1280,
            fixed_h: 720,
            region_fixed: true,
            region_use_pos: true,
            region_x: -50,
            region_y: 30,
            capture_cursor: true,
            delay_seconds: 5,
            window_objects: true,
            capture_links: false,
            preview_in_editor: false,
            copy_to_clipboard: true,
            save_to_file: true,
            file_format: FileFormat::Webp,
            file_prefix: "Shot".into(),
            folder: "C:\\Caps\\ü".into(),
            open_in_program: true,
            program_path: "C:\\Tools\\x.exe".into(),
            grab_text: true,
            pin_to_screen: true,
            read_qr: true,
            office: 2,
            upload: true,
            microphone: false,
            system_audio: false,
            mic_volume: 150,
            sys_volume: 20,
            auto_gain: false,
            fps: 60,
            quality: 3,
            downscale_1080: false,
            hardware_encode: false,
            audio_only: true,
            audio_format: 1,
            webcam: true,
            webcam_device: "usb#vid&pid".into(),
            webcam_name: "Cam".into(),
            webcam_shape: 3,
            chroma_key: true,
            chroma_color: rgb(0, 255, 0),
            chroma_tolerance: 70,
            builtin: "grabtext".into(),
            hotkey_mods: 2 | 4,
            hotkey_vk: 'P' as u32,
            ..Default::default()
        };
        let f = &mut p.effects;
        (f.border, f.border_width, f.border_color, f.filter, f.filter_kind) = (true, 9, rgb(1, 2, 3), true, 2);
        (f.shadow, f.shadow_offset, f.shadow_blur, f.shadow_opacity) = (true, 11, 12, 77);
        (f.caption, f.caption_text, f.caption_top) = (true, w("{date} ✓"), true);
        (f.replace_color, f.replace_from, f.replace_to, f.replace_tolerance) = (true, rgb(10, 20, 30), rgb(200, 100, 0), 33);
        (f.scale, f.scale_percent, f.edge, f.edge_style, f.edge_size, f.edge_sides) = (true, 150, true, 3, 20, 9);
        (f.trim, f.trim_tolerance, f.adjust, f.brightness, f.contrast, f.hue, f.saturation) = (true, 4, true, -20, 15, -90, 40);
        (f.rotate, f.rotate_kind, f.watermark, f.watermark_text) = (true, 4, true, w("© Me"));
        (f.watermark_opacity, f.watermark_tiled, f.resolution, f.dpi, f.beautify) = (60, false, true, 600, true);
        let b = &mut f.beautify_opt;
        (b.background, b.color1, b.image_path, b.padding, b.aspect, b.auto_balance) =
            (3, rgb(5, 6, 7), w("C:\\bg.png"), 100, 2, false);
        put("custom", &p.to_xml());
        put("custom read", &Preset::from_xml(&p.to_xml()).to_xml());
        let reads = [
            r#"<CaptureSettings DisplayName="Old" SelectionMode="4" RegionWidth="300"/>"#,
            r#"<CaptureSettings SelectionMode="99" CaptureMode="Video"><CaptureEffects Grayscale="1" Scale="150" ShadowOffset="-5" EdgeSides="31" Dpi="5"/></CaptureSettings>"#,
            r#"<CaptureSettings SelectionMode="-3" DelayDuration=" 12x" HotKey="abc"><ShareSettingsFile Enabled="1" OutputFileType="99"/><ShareSettingsOffice Target="7"/><Video FrameRate="1000" Quality="-1" MicrophoneVolume="999" WebcamShape="9" AudioFormat="5"/></CaptureSettings>"#,
            r##"<CaptureSettings><CaptureEffects Beautify="1"><Beautify Background="9" Angle="400" Padding="-5" Color1="#ZZ" Color2="#00FF00"/></CaptureEffects></CaptureSettings>"##,
            r#"<CaptureSettings/>"#,
            r##"<CaptureSettings><CaptureEffects Scale="100" BorderColor="#123456" Filter="0" Grayscale="1"/><ShareSettingsProgram Enabled="1"/><ShareSettingsFile Prefix="" FolderName="D:\x"/></CaptureSettings>"##,
            r#"<CaptureSettings SelectionMode="13" CaptureMode="image"><Video EnableMicrophone="0"/></CaptureSettings>"#,
            r#"<CaptureSettings DisplayName="x" RegionWidth="0x10" RegionHeight="-0" CaptureCursor="2" PreviewInEditor=""/>"#,
        ];
        for (i, r) in reads.iter().enumerate() {
            let n = xml::parse(r.as_bytes());
            let ok = n.is_some() as i32;
            put(&format!("read {i} {ok}"), &Preset::from_xml(&n.unwrap_or_default()).to_xml());
        }
        let file = std::env::temp_dir().join(format!("snapcore-presetgolden-{}.xml", std::process::id()));
        let s = PresetStore { presets: vec![Preset::default(), p], selected: 1 };
        let saved = s.export(&file) as i32;
        let root = xml::load_file(&file);
        let loaded = root.is_some() as i32;
        let _ = std::fs::remove_file(&file);
        put(&format!("file {saved} {loaded}"), &root.unwrap_or_default());
        let mut differ = 0;
        for (i, (a, b)) in out.iter().zip(&golden).enumerate() {
            if a != b {
                differ += 1;
                if differ <= 5 {
                    eprintln!("line {}:\n  rust: {a}\n  c++:  {b}", i + 1);
                }
            }
        }
        assert_eq!(out.len(), golden.len(), "line count");
        assert_eq!(differ, 0);
    }

    #[test]
    fn selections_and_formats() {
        assert_eq!(Selection::from_index(99), Selection::Panoramic);
        assert_eq!(Selection::from_index(-1), Selection::Region);
        assert_eq!(Selection::for_kind(CaptureKind::Video).len(), 4);
        assert!(!Selection::for_kind(CaptureKind::Image).contains(&Selection::FixedRegion));
        assert_eq!(FileFormat::from_index(5).ext(), ".webp");
        assert_eq!(default_presets().len(), 6);
    }
}
