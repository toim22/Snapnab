# Snapnab

**Screen capture, video recording and image editor for Windows.**

![Snapnab capture window](docs/images/capture-window.png)

Snapnab lives in the notification area: press **Print Screen**, choose what to capture and the image opens in
**Snapnab Editor**, ready to annotate, highlight and share. It also records the screen as MP4, with microphone and system audio.

## Features

**Capture**
- Region, window, full screen, monitor, fixed-size region, freehand, multiple areas, object under the cursor,
  panoramic scrolling (you scroll, it stitches), automatic window scrolling, extended window, menus, clipboard and file
- **Capture text**: recognizes the text in the region (OCR) and copies it to the clipboard
- Magnifier and coordinates on the crosshair, delay with a countdown, cursor capture
- Presets with global hotkeys and outputs: Editor, clipboard, file with an automatic name, external program, text
- **One Click**: a discreet panel docked to the edge of the screen that opens when the mouse is over it (*View › Show One Click*)
- 12 effects applied on capture: border, filters, shadow, capture info, color replacement, resize,
  edge effects, trim edges, color adjustment, rotate, watermark and resolution
- Full-screen captures with **each window as a separate object**, and the **links** of web pages and documents kept as
  clickable areas (also in PDF exports)

**Video recording**
- MP4 (H.264 + AAC), microphone and system audio with separate mute and volume (automatic or manual microphone gain),
  pause without gaps in the video
- Webcam inside the video (circle, rounded square, square or rectangle, with optional background removal), which can
  be dragged during the recording
- Control panel and frame that do not show up in the recording, downscaling to 1080p, hardware encoding

**Editor**
- Arrow (with a text label at the tail), line, shapes, pen, highlighter, text, callouts (with frosted-glass
  background), numbered steps, stamps (including Before/After and other labels), blur, spotlight, magnifier
  (also a realistic loupe), fill, eraser, eyedropper, crop and cut out
- Shift draws perfect squares, circles and 45° lines in every tool; highlighter, spotlight and blur also as an ellipse,
  polygon or freehand outline; gradient fills
- **Layers panel** (hide, lock, rename and reorder objects), rulers and guides, exact X/Y/width/height, align and
  distribute, paste in place, selection and crop that snap to edges
- Selection with Smart Move, magic wand and smart fill; lift a piece, edit the rest and drop it back; undo/redo
- Image effects, text recognition (OCR), save as PNG, JPG, BMP, GIF, TIFF, PDF and `.snap`, print and email
- Video: cut out parts, save a frame, create an animated GIF and export MP4
- Library with all your captures: names, search, a larger preview on hover
- Export and import all settings in one file, with an automatic weekly backup

**Languages**
- English, Portuguese, Chinese (Simplified), Hindi, Spanish, Arabic, French, Bengali, Russian, Indonesian, Urdu and
  German; follows the Windows language (English if there is no translation) and can be changed in *File › Options… › Language*

The capture window follows the accent color chosen in *Settings > Personalization > Colors*.

The complete user manual is [help/Snapnab-Manual.html](help/Snapnab-Manual.html); it opens from *Help › Snapnab Manual*
(**F1**) in both programs.

## Installation

Requires **Windows 10 version 2004 or later, or Windows 11 (64-bit)**. No extra runtime is needed.

- **Portable:** download `Snapnab-<version>-win-x64.zip` from the *Releases* page, extract it to any folder and open `Snapnab.exe`.
- **Installer:** open `Snapnab-<version>-setup.exe` and follow the wizard. It installs just for you (no administrator
  prompt) or for all users, creates the Start menu shortcuts, can start Snapnab with Windows and
  uninstalls from *Settings › Apps*. Your captures and settings are kept when uninstalling.

## Usage

| Shortcut | Action |
|---|---|
| `Print Screen` | capture an image |
| `Shift+Print Screen` | record a video (again to stop) |
| `Shift+F9` | pause / resume the recording |
| `Shift+F10` | stop the recording |
| `Esc` | cancel the selection |

The shortcuts can be changed in *Options*. Command line:

```text
Snapnab.exe        /capture | /video | /preset "name" | /clipboard | /hide | /exit | /help
SnapnabEditor.exe  /Open "file" | /NewFromClipboard | /help
```

### Your data

| What | Where |
|---|---|
| Captures and videos | `%LOCALAPPDATA%\Snapnab\Library` |
| Presets | `%LOCALAPPDATA%\Snapnab\Presets1.xml` |
| Editor themes, quick styles, favorites, quick effects, stamp order | `%LOCALAPPDATA%\Snapnab\*.xml` |
| Settings backups | `%LOCALAPPDATA%\Snapnab\Backups` |
| Logs | `%LOCALAPPDATA%\Snapnab\Logs` |
| Settings | `HKEY_CURRENT_USER\Software\Snapnab` |

Nothing is sent to the internet.

## Building from source

With Visual Studio 2022+ (*Desktop development with C++* workload):

```bat
build.bat
```

The program ends up in `dist\Snapnab\` and the package in `dist\Snapnab-<version>-win-x64.zip`. Details, tests and the
installer are in [docs/BUILDING.md](docs/BUILDING.md).

## Repository layout

```text
src/                    source code (C++20, Win32)
  common/               library shared by both programs
  capture/              Snapnab.exe — capture and recording
  editor/               SnapnabEditor.exe — editor
  snapex/               SnapEx64.dll — extended window capture
  resources/            icons and manifest
tests/                  integration tests
help/                   user manual (Snapnab-Manual.html, opened by Help › Snapnab Manual and F1)
lang/                   interface translations (gettext: <language>.po and the snapnab.pot template)
packaging/              package files and installer script
scripts/                development utilities (i18n.py extracts and checks the translations)
docs/                   architecture, build and images
```

The architecture and its mapping to the reference documents are in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
The version history is in [CHANGELOG.md](CHANGELOG.md).

## License

Copyright © 2026 Antonio Rezende.

Snapnab is free software: you can use, study, modify and redistribute it under the terms of the
[GNU General Public License version 3](LICENSE). Modified versions that are distributed must also have their
source code open under the same license. The program is provided without warranty of any kind.
