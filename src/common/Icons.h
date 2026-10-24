// Ícones vetoriais desenhados com GDI+ (sem arquivos de recurso)
#pragma once
#include "Common.h"

namespace snap::icons {

enum Id {
    Capture, Image, Video, Record, Pause, Stop, Restart, Cancel, Mic, MicOff, Speaker, SpeakerOff, Camera,
    Select, Arrow, Line, Rect, Ellipse, Pen, Highlighter, Text, Callout, Step, Stamp, Blur, Spotlight,
    Magnify, Fill, Eraser, Crop, Selection, Wand, Cutout, Eyedropper, Undo, Redo, Editor, Folder, Gear,
    Copy, Save, Share, Ocr, Library
};

void Draw(Gdiplus::Graphics& g, Id id, Gdiplus::RectF r, Gdiplus::Color c);
void DrawOnDC(HDC dc, Id id, RECT r, COLORREF c);
HICON MakeAppIcon(int size, bool editor);     // ícone do app desenhado em tempo de execução

} // namespace snap::icons
