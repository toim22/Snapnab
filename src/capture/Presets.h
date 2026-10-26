// Capture presets (CaptureSettings / Presets4.xml, comp_7 §2) — capture, effects and outputs in the same object
#pragma once
#include "Common.h"
#include "Effects.h"
#include "Xml.h"

namespace snap {

enum class CaptureKind { Image = 0, Video = 1 };

enum class Selection {
    Region = 0, Window, FullScreen, Monitor, FixedRegion, Freehand, MultiArea, Object, Scrolling,
    ExtendedWindow, Menu, Clipboard, File, Panoramic, Count
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
    // Region: chosen during the capture (Interactive) or fixed (Fixed), with an optional start position
    // (SelectionSettingsRegion: RegionSelectionType, Region{Left,Top,Width,Height}, SetPosition — comp_7 §2, comp_1 §6)
    bool regionFixed = false, regionUsePos = false;
    int regionX = 0, regionY = 0;
    bool captureCursor = false;
    int delaySeconds = 0;
    bool windowObjects = false;           // full screen / monitor: each window as a separate object in the Editor
    bool captureLinks = true;             // keeps the links of web pages and documents as clickable areas (UI Automation)
    // after capture / sharing
    bool previewInEditor = true;
    bool copyToClipboard = false;
    bool saveToFile = false;
    FileFormat fileFormat = FileFormat::PNG;
    std::wstring filePrefix = L"Captura";
    std::wstring folder;                  // empty = Pictures\Snapnab
    bool openInProgram = false;
    std::wstring programPath = L"mspaint.exe";
    bool grabText = false;                // Grab Text: OCR of the capture → text window + clipboard
    fx::CaptureEffects effects;
    // video
    bool microphone = true;
    bool systemAudio = true;
    int micVolume = 100, sysVolume = 100; // manual volume of each source (%), before and during the recording
    bool autoGain = true;                 // automatic microphone gain (on top of the volume)
    int fps = 30;
    int quality = 2;                      // 0 very low … 3 high
    bool downscale1080 = true;
    bool hardwareEncode = true;
    bool webcam = false;                  // webcam picture-in-picture (EnableWebcam / SelectedWebcam, comp_7 §2)
    std::wstring webcamDevice, webcamName;    // device symbolic link and friendly name
    int webcamShape = 0;                  // CamShape: circle, rounded square, square, 16:9, 4:3
    bool chromaKey = false;               // removes the webcam background by color
    COLORREF chromaColor = RGB(0, 177, 64);
    int chromaTolerance = 40;
    std::wstring builtin;                 // presets that ship with the program ("panoramic", "grabtext")
    // preset hotkey
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
private:
    void AddBuiltins();       // "Panoramic scrolling capture" and "Capture text" at the top of the list (once)
};

} // namespace snap
