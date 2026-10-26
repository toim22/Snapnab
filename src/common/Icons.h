// Vector icons drawn with GDI+ (no resource files)
#pragma once
#include "Common.h"

namespace snap::icons {

enum Id {
    Capture, Image, Video, Record, Pause, Stop, Restart, Cancel, Mic, MicOff, Speaker, SpeakerOff, Camera,
    Select, Arrow, Line, Rect, Ellipse, Pen, Highlighter, Text, Callout, Step, Stamp, Blur, Spotlight,
    Magnify, Fill, Eraser, Crop, Selection, Wand, Cutout, Eyedropper, Undo, Redo, Editor, Folder, Gear,
    Copy, Save, Share, Ocr, Library, CameraFill, VideoFill, Webcam, WebcamOff,
    Mail, Print, NewDoc, Trash, Layers, Rotate, Effects,
    Star, Clipboard, Program, Duplicate, SaveAs, NewClipboard, AutoTrim, DeleteObject,
    FxBorder, FxEdges, FxCaption, FxPerspective, FxPageCurl, FxShadow, FxFilters, FxColorAdjust, FxColorReplace, FxSpotlight, FxWatermark,
    Lock, Unlock, Eye, EyeOff, Pin, AlignLeft, AlignCenter, AlignRight, AlignTop, AlignMiddle, AlignBottom, DistributeH, DistributeV,
    Ruler, Link, Group, Sliders
};

void Draw(Gdiplus::Graphics& g, Id id, Gdiplus::RectF r, Gdiplus::Color c);
void DrawOnDC(HDC dc, Id id, RECT r, COLORREF c);
HICON MakeAppIcon(int size, bool editor);     // app icon drawn at run time

} // namespace snap::icons
