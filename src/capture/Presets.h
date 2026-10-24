// Presets de captura (CaptureSettings / Presets4.xml, comp_7 §2) — captura, efeitos e destinos no mesmo objeto
#pragma once
#include "Common.h"
#include "Effects.h"
#include "Xml.h"

namespace snap {

enum class CaptureKind { Image = 0, Video = 1 };

enum class Selection {
    Region = 0, Window, FullScreen, Monitor, FixedRegion, Freehand, MultiArea, Object, Scrolling,
    ExtendedWindow, Menu, Clipboard, File, Count
};
const wchar_t* SelectionName(Selection s);
std::vector<Selection> SelectionsFor(CaptureKind k);

enum class FileFormat { PNG = 0, JPEG, BMP, GIF, TIFF };
const wchar_t* FileFormatExt(FileFormat f);

struct Preset {
    std::wstring name = L"Novo preset";
    CaptureKind kind = CaptureKind::Image;
    Selection selection = Selection::Region;
    int fixedW = 640, fixedH = 480;
    bool captureCursor = false;
    int delaySeconds = 0;
    // pós-captura / compartilhamento
    bool previewInEditor = true;
    bool copyToClipboard = false;
    bool saveToFile = false;
    FileFormat fileFormat = FileFormat::PNG;
    std::wstring filePrefix = L"Captura";
    std::wstring folder;                  // vazio = Imagens\Snapnab
    bool openInProgram = false;
    std::wstring programPath = L"mspaint.exe";
    fx::CaptureEffects effects;
    // vídeo
    bool microphone = true;
    bool systemAudio = true;
    int fps = 30;
    int quality = 2;                      // 0 muito baixa … 3 alta
    bool downscale1080 = true;
    bool hardwareEncode = true;
    // atalho do preset
    UINT hotkeyMods = 0, hotkeyVk = 0;

    XmlNode ToXml() const;
    static Preset FromXml(const XmlNode& n);
};

struct PresetStore {
    std::vector<Preset> presets;
    int selected = 0;
    bool Load();
    bool Save() const;
    bool Export(const std::wstring& path) const;
    bool Import(const std::wstring& path);
    std::wstring FilePath() const;
    void CreateDefaults();
};

} // namespace snap
