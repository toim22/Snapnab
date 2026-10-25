#include "Presets.h"

namespace snap {

const wchar_t* SelectionName(Selection s) {
    switch (s) {
    case Selection::Region: return Tr(L"Região");
    case Selection::Window: return Tr(L"Janela");
    case Selection::FullScreen: return Tr(L"Tela inteira (todos os monitores)");
    case Selection::Monitor: return Tr(L"Monitor atual");
    case Selection::FixedRegion: return Tr(L"Região com tamanho fixo");
    case Selection::Freehand: return Tr(L"Mão livre");
    case Selection::MultiArea: return Tr(L"Várias áreas");
    case Selection::Object: return Tr(L"Objeto (controle sob o cursor)");
    case Selection::Scrolling: return Tr(L"Rolagem automática (janela)");
    case Selection::Panoramic: return Tr(L"Rolagem panorâmica");
    case Selection::ExtendedWindow: return Tr(L"Janela estendida");
    case Selection::Menu: return Tr(L"Menu");
    case Selection::Clipboard: return Tr(L"Área de transferência");
    case Selection::File: return Tr(L"Arquivo gráfico");
    default: return L"?";
    }
}

std::vector<Selection> SelectionsFor(CaptureKind k) {
    // a região fixa é uma opção da própria Região (engrenagem), não um item da lista
    if (k == CaptureKind::Video) return { Selection::Region, Selection::Window, Selection::FullScreen, Selection::Monitor };
    using S = Selection;
    return { S::Region, S::Window, S::FullScreen, S::Monitor, S::Freehand, S::MultiArea, S::Object,
             S::Panoramic, S::Scrolling, S::ExtendedWindow, S::Menu, S::Clipboard, S::File };
}

const wchar_t* FileFormatExt(FileFormat f) {
    switch (f) {
    case FileFormat::JPEG: return L".jpg";
    case FileFormat::BMP: return L".bmp";
    case FileFormat::GIF: return L".gif";
    case FileFormat::TIFF: return L".tif";
    default: return L".png";
    }
}

XmlNode Preset::ToXml() const {
    XmlNode n("CaptureSettings");
    n.setW("DisplayName", name);
    n.set("CaptureMode", kind == CaptureKind::Video ? "Video" : "Image");
    n.seti("SelectionMode", (int)selection);
    n.seti("RegionWidth", fixedW).seti("RegionHeight", fixedH);
    n.add("SelectionSettingsRegion").seti("RegionSelectionType", regionFixed).seti("RegionLeft", regionX).seti("RegionTop", regionY)
     .seti("SetPosition", regionUsePos);
    n.seti("CaptureCursor", captureCursor).seti("DelayDuration", delaySeconds);
    n.seti("PreviewInEditor", previewInEditor).seti("CopyToClipboard", copyToClipboard);
    XmlNode& sh = n.add("ShareSettingsFile");
    sh.seti("Enabled", saveToFile).seti("OutputFileType", (int)fileFormat).setW("Prefix", filePrefix).setW("FolderName", folder);
    XmlNode& pr = n.add("ShareSettingsProgram");
    pr.seti("Enabled", openInProgram).setW("Program", programPath);
    n.add("ShareSettingsGrabText").seti("Enabled", grabText);
    if (!builtin.empty()) n.setW("Builtin", builtin);
    XmlNode& e = n.add("CaptureEffects");
    const auto& f = effects;
    e.seti("Border", f.border).seti("BorderWidth", f.borderWidth).setW("BorderColor", ColorToHex(f.borderColor));
    e.seti("Filter", f.filter).seti("FilterKind", f.filterKind);
    e.seti("Shadow", f.shadow).seti("ShadowOffset", f.shadowOffset).seti("ShadowBlur", f.shadowBlur).seti("ShadowOpacity", f.shadowOpacity);
    e.seti("Caption", f.caption).setW("CaptionText", f.captionText).seti("CaptionTop", f.captionTop);
    e.seti("ReplaceColor", f.replaceColor).setW("ReplaceFrom", ColorToHex(f.replaceFrom)).setW("ReplaceTo", ColorToHex(f.replaceTo))
     .seti("ReplaceTolerance", f.replaceTolerance);
    e.seti("ScaleEnabled", f.scale).seti("Scale", f.scalePercent);
    e.seti("Edge", f.edge).seti("EdgeStyle", f.edgeStyle).seti("EdgeSize", f.edgeSize).seti("EdgeSides", f.edgeSides);
    e.seti("Trim", f.trim).seti("TrimTolerance", f.trimTolerance);
    e.seti("Adjust", f.adjust).seti("Brightness", f.brightness).seti("Contrast", f.contrast).seti("Hue", f.hue).seti("Saturation", f.saturation);
    e.seti("Rotate", f.rotate).seti("RotateKind", f.rotateKind);
    e.seti("Watermark", f.watermark).setW("WatermarkText", f.watermarkText).seti("WatermarkOpacity", f.watermarkOpacity)
     .seti("WatermarkTiled", f.watermarkTiled);
    e.seti("Resolution", f.resolution).seti("Dpi", f.dpi);
    XmlNode& v = n.add("Video");
    v.seti("EnableMicrophone", microphone).seti("EnableSystemAudio", systemAudio).seti("FrameRate", fps)
     .seti("Quality", quality).seti("Downscale1080", downscale1080).seti("HardwareEncoding", hardwareEncode)
     .seti("EnableWebcam", webcam).setW("SelectedWebcam", webcamDevice).setW("SelectedWebcamName", webcamName)
     .seti("WebcamShape", webcamShape).seti("ChromaKey", chromaKey).setW("ChromaColor", ColorToHex(chromaColor))
     .seti("ChromaTolerance", chromaTolerance);
    n.seti("HotKey", hotkeyVk).seti("HotKeyModifiers", hotkeyMods);
    return n;
}

Preset Preset::FromXml(const XmlNode& n) {
    Preset p;
    p.name = n.getW("DisplayName", p.name);
    p.kind = n.get("CaptureMode") == "Video" ? CaptureKind::Video : CaptureKind::Image;
    p.selection = (Selection)std::clamp(n.geti("SelectionMode", 0), 0, (int)Selection::Count - 1);
    p.fixedW = n.geti("RegionWidth", 640); p.fixedH = n.geti("RegionHeight", 480);
    if (auto* rs = n.child("SelectionSettingsRegion")) {
        p.regionFixed = rs->getb("RegionSelectionType"); p.regionUsePos = rs->getb("SetPosition");
        p.regionX = rs->geti("RegionLeft"); p.regionY = rs->geti("RegionTop");
    }
    if (p.selection == Selection::FixedRegion) { p.selection = Selection::Region; p.regionFixed = true; }   // presets antigos
    p.captureCursor = n.getb("CaptureCursor"); p.delaySeconds = n.geti("DelayDuration");
    p.previewInEditor = n.getb("PreviewInEditor", true); p.copyToClipboard = n.getb("CopyToClipboard");
    if (auto* sh = n.child("ShareSettingsFile")) {
        p.saveToFile = sh->getb("Enabled"); p.fileFormat = (FileFormat)std::clamp(sh->geti("OutputFileType"), 0, 4);
        p.filePrefix = sh->getW("Prefix", p.filePrefix); p.folder = sh->getW("FolderName");
    }
    if (auto* pr = n.child("ShareSettingsProgram")) { p.openInProgram = pr->getb("Enabled"); p.programPath = pr->getW("Program", p.programPath); }
    if (auto* gt = n.child("ShareSettingsGrabText")) p.grabText = gt->getb("Enabled");
    p.builtin = n.getW("Builtin");
    if (auto* e = n.child("CaptureEffects")) {
        auto& f = p.effects;
        f.border = e->getb("Border"); f.borderWidth = e->geti("BorderWidth", 4);
        f.borderColor = HexToColor(e->getW("BorderColor"), f.borderColor);
        f.filter = e->getb("Filter", e->getb("Grayscale"));     // "Grayscale" = presets antigos
        f.filterKind = std::clamp(e->geti("FilterKind", 0), 0, 3);
        f.shadow = e->getb("Shadow");
        f.shadowOffset = std::clamp(e->geti("ShadowOffset", 6), 0, 50);
        f.shadowBlur = std::clamp(e->geti("ShadowBlur", 6), 0, 50);
        f.shadowOpacity = std::clamp(e->geti("ShadowOpacity", 50), 0, 100);
        f.caption = e->getb("Caption"); f.captionText = e->getW("CaptionText", f.captionText); f.captionTop = e->getb("CaptionTop");
        f.replaceColor = e->getb("ReplaceColor");
        f.replaceFrom = HexToColor(e->getW("ReplaceFrom"), f.replaceFrom);
        f.replaceTo = HexToColor(e->getW("ReplaceTo"), f.replaceTo);
        f.replaceTolerance = std::clamp(e->geti("ReplaceTolerance", 10), 0, 100);
        f.scalePercent = std::clamp(e->geti("Scale", 100), 10, 400);
        f.scale = e->getb("ScaleEnabled", f.scalePercent != 100);
        f.edge = e->getb("Edge"); f.edgeStyle = std::clamp(e->geti("EdgeStyle"), 0, 5);
        f.edgeSize = std::clamp(e->geti("EdgeSize", 14), 4, 60); f.edgeSides = e->geti("EdgeSides", 6) & 15;
        f.trim = e->getb("Trim"); f.trimTolerance = std::clamp(e->geti("TrimTolerance", 8), 0, 100);
        f.adjust = e->getb("Adjust");
        f.brightness = std::clamp(e->geti("Brightness"), -100, 100); f.contrast = std::clamp(e->geti("Contrast"), -100, 100);
        f.hue = std::clamp(e->geti("Hue"), -180, 180); f.saturation = std::clamp(e->geti("Saturation"), -100, 100);
        f.rotate = e->getb("Rotate"); f.rotateKind = std::clamp(e->geti("RotateKind"), 0, 4);
        f.watermark = e->getb("Watermark"); f.watermarkText = e->getW("WatermarkText", f.watermarkText);
        f.watermarkOpacity = std::clamp(e->geti("WatermarkOpacity", 35), 5, 100); f.watermarkTiled = e->getb("WatermarkTiled", true);
        f.resolution = e->getb("Resolution"); f.dpi = std::clamp(e->geti("Dpi", 300), 36, 2400);
    }
    if (auto* v = n.child("Video")) {
        p.microphone = v->getb("EnableMicrophone", true); p.systemAudio = v->getb("EnableSystemAudio", true);
        p.fps = std::clamp(v->geti("FrameRate", 30), 5, 60); p.quality = std::clamp(v->geti("Quality", 2), 0, 3);
        p.downscale1080 = v->getb("Downscale1080", true); p.hardwareEncode = v->getb("HardwareEncoding", true);
        p.webcam = v->getb("EnableWebcam"); p.webcamDevice = v->getW("SelectedWebcam"); p.webcamName = v->getW("SelectedWebcamName");
        p.webcamShape = std::clamp(v->geti("WebcamShape"), 0, 4);
        p.chromaKey = v->getb("ChromaKey"); p.chromaColor = HexToColor(v->getW("ChromaColor"), p.chromaColor);
        p.chromaTolerance = std::clamp(v->geti("ChromaTolerance", 40), 0, 100);
    }
    p.hotkeyVk = n.geti("HotKey"); p.hotkeyMods = n.geti("HotKeyModifiers");
    return p;
}

std::wstring PresetStore::FilePath() const { return AppDataDir() + L"\\Presets1.xml"; }

void PresetStore::CreateDefaults() {
    presets.clear();
    Preset a; a.name = TR_NOOP(L"Imagem — Região"); presets.push_back(a);
    Preset b; b.name = TR_NOOP(L"Imagem — Janela"); b.selection = Selection::Window; b.effects.shadow = true; presets.push_back(b);
    Preset c; c.name = TR_NOOP(L"Tela inteira → Área de transferência"); c.selection = Selection::FullScreen; c.previewInEditor = false;
    c.copyToClipboard = true; presets.push_back(c);
    Preset d; d.name = TR_NOOP(L"Rolagem (página inteira)"); d.selection = Selection::Scrolling; presets.push_back(d);
    Preset e; e.name = TR_NOOP(L"Vídeo — Região"); e.kind = CaptureKind::Video; e.captureCursor = true; presets.push_back(e);
    Preset f; f.name = TR_NOOP(L"Vídeo — Tela inteira"); f.kind = CaptureKind::Video; f.selection = Selection::Monitor; f.captureCursor = true; presets.push_back(f);
    selected = 0;
    RegSetDword(L"BuiltinPresets", 0);
    AddBuiltins();
}

void PresetStore::AddBuiltins() {
    if (RegGetDword(L"BuiltinPresets", 0) >= 1) return;      // o usuário pode excluí-los depois sem que voltem
    Preset pano; pano.name = TR_NOOP(L"Captura com rolagem panorâmica"); pano.builtin = L"panoramic";
    pano.selection = Selection::Panoramic;
    Preset text; text.name = TR_NOOP(L"Capturar texto"); text.builtin = L"grabtext";
    text.previewInEditor = false; text.grabText = true;
    presets.insert(presets.begin(), { pano, text });
    selected += 2;
    RegSetDword(L"BuiltinPresets", 1);
}

static bool LoadFrom(const std::wstring& path, std::vector<Preset>& out, int* selected) {
    XmlNode root;
    if (!XmlLoadFile(path, root) || root.name != "SavedPresets") return false;
    out.clear();
    if (auto* list = root.child("Presets"))
        for (auto& c : list->children) if (c.name == "CaptureSettings") out.push_back(Preset::FromXml(c));
    if (selected) *selected = root.geti("SelectedMode", 0);
    return true;
}

bool PresetStore::Load() {
    if (!LoadFrom(FilePath(), presets, &selected) || presets.empty()) { CreateDefaults(); Save(); }
    else if (RegGetDword(L"BuiltinPresets", 0) < 1) { AddBuiltins(); Save(); }
    selected = std::clamp(selected, 0, (int)presets.size() - 1);
    return true;
}

static XmlNode MakeRoot(const std::vector<Preset>& presets, int selected) {
    XmlNode root("SavedPresets");
    root.seti("Version", 1).seti("SelectedMode", selected);
    XmlNode& list = root.add("Presets");
    for (auto& p : presets) list.children.push_back(p.ToXml());
    return root;
}

bool PresetStore::Save() const { return XmlSaveFile(FilePath(), MakeRoot(presets, selected)); }
bool PresetStore::Export(const std::wstring& path) const { return XmlSaveFile(path, MakeRoot(presets, selected)); }

bool PresetStore::Import(const std::wstring& path) {
    std::vector<Preset> in;
    if (!LoadFrom(path, in, nullptr) || in.empty()) return false;
    for (auto& p : in) { p.hotkeyMods = p.hotkeyVk = 0; presets.push_back(p); }   // atalhos não são importados (evita conflito)
    return Save();
}

} // namespace snap
