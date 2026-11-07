"""Writes Presets1.xml with the presets a test needs (same format the program saves, rust/snapcore/src/presets.rs)."""
from xml.sax.saxutils import quoteattr

from .config import DATA_DIR

# Selection (rust/snapcore/src/presets.rs)
SEL = {n: i for i, n in enumerate(["Region", "Window", "FullScreen", "Monitor", "FixedRegion", "Freehand", "MultiArea",
                                   "Object", "Scrolling", "ExtendedWindow", "Menu", "Clipboard", "File", "Panoramic"])}
FILE_TYPES = {"png": 0, "jpg": 1, "bmp": 2, "gif": 3, "tif": 4}

EFFECT_DEFAULTS = dict(Border=0, BorderWidth=4, BorderColor="#282828", Filter=0, FilterKind=0, Shadow=0, ShadowOffset=6,
                       ShadowBlur=6, ShadowOpacity=50, Caption=0, CaptionText="{date} {time} — {computer}", CaptionTop=0,
                       ReplaceColor=0, ReplaceFrom="#FFFFFF", ReplaceTo="#FFFF00", ReplaceTolerance=10, ScaleEnabled=0,
                       Scale=100, Edge=0, EdgeStyle=0, EdgeSize=14, EdgeSides=6, Trim=0, TrimTolerance=8, Adjust=0,
                       Brightness=0, Contrast=0, Hue=0, Saturation=0, Rotate=0, RotateKind=0, Watermark=0,
                       WatermarkText="Snapnab", WatermarkOpacity=35, WatermarkTiled=1, Resolution=0, Dpi=300)


def _attrs(d):
    return " ".join(f"{k}={quoteattr(str(v))}" for k, v in d.items())


def preset(name, selection="FullScreen", kind="Image", *, editor=True, clipboard=False, save_to=None, file_type="png",
           prefix="E2E", program=None, grab_text=False, delay=0, cursor=False, fixed=(640, 480), region_fixed=False,
           region_pos=None, window_objects=False, links=False, effects=None, mic=False, sys_audio=False, fps=15,
           quality=1, hw=False, builtin="", office=-1, upload=False):
    cap = dict(DisplayName=name, CaptureMode=kind, SelectionMode=SEL[selection], RegionWidth=fixed[0],
               RegionHeight=fixed[1], CaptureCursor=int(cursor), DelayDuration=delay,
               WindowsAsObjects=int(window_objects), CaptureLinks=int(links), PreviewInEditor=int(editor),
               CopyToClipboard=int(clipboard), Builtin=builtin, HotKey=0, HotKeyModifiers=0)
    region = dict(RegionSelectionType=int(region_fixed), RegionLeft=(region_pos or (0, 0))[0],
                  RegionTop=(region_pos or (0, 0))[1], SetPosition=int(region_pos is not None))
    file = dict(Enabled=int(save_to is not None), OutputFileType=FILE_TYPES[file_type], Prefix=prefix,
                FolderName=str(save_to or ""))
    prog = dict(Enabled=int(program is not None), Program=program or "mspaint.exe")
    fx = dict(EFFECT_DEFAULTS, **(effects or {}))
    video = dict(EnableMicrophone=int(mic), EnableSystemAudio=int(sys_audio), FrameRate=fps, MicrophoneVolume=100,
                 SystemAudioVolume=100, AutomaticGain=1, Quality=quality, Downscale1080=1, HardwareEncoding=int(hw),
                 EnableWebcam=0, SelectedWebcam="", SelectedWebcamName="", WebcamShape=0, ChromaKey=0,
                 ChromaColor="#00B140", ChromaTolerance=40)
    return (f"    <CaptureSettings {_attrs(cap)}>\n"
            f"      <SelectionSettingsRegion {_attrs(region)}/>\n"
            f"      <ShareSettingsFile {_attrs(file)}/>\n"
            f"      <ShareSettingsProgram {_attrs(prog)}/>\n"
            f"      <ShareSettingsGrabText Enabled=\"{int(grab_text)}\"/>\n"
            f"      <ShareSettingsOffice Target=\"{office}\"/>\n"
            f"      <ShareSettingsUpload Enabled=\"{int(upload)}\"/>\n"
            f"      <CaptureEffects {_attrs(fx)}/>\n"
            f"      <Video {_attrs(video)}/>\n"
            f"    </CaptureSettings>\n")


def write(*presets_xml, selected=0):
    DATA_DIR.mkdir(parents=True, exist_ok=True)
    body = "".join(presets_xml)
    (DATA_DIR / "Presets1.xml").write_text(
        f'<?xml version="1.0" encoding="UTF-8"?>\n<SavedPresets Version="1" SelectedMode="{selected}">\n'
        f"  <Presets>\n{body}  </Presets>\n</SavedPresets>\n", encoding="utf-8")


def names_in_file():
    import xml.etree.ElementTree as ET
    root = ET.parse(DATA_DIR / "Presets1.xml").getroot()
    return [c.get("DisplayName") for c in root.iter("CaptureSettings")]
