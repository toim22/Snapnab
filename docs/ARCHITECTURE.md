# Architecture

Snapnab is a Rust/Win32 (x64) implementation built from reference architecture and pseudocode documents that are
not part of this repository (the `§N`, `comp_N` and `ed_comp_N` citations in the code point to them). It consists of
**two cooperating executables** plus a DLL:

| Binary | Equivalent of | Role |
|---|---|---|
| `Snapnab.exe` | `Snagit32.exe` | tray, capture window, presets, global hotkeys, capture engine, video recording, outputs |
| `SnapnabEditor.exe` | `SnagitEditor.exe` | image editing (vector objects + raster), effects, OCR, PDF, video and GIF editing, library |
| `SnapEx64.dll` | `SnagEx64.dll` | extended window capture (`WH_CALLWNDPROC` hook + `WM_GETMINMAXINFO` 19500×19500) |

## Source code

Everything is Rust, in the Cargo workspace `rust/` (until version 0.4 it was C++20; the move went one phase at a
time behind the unchanged e2e suite, branch `rust-rewrite`). The interface is egui 0.37 (immediate mode, drawn by
wgpu, accessible through AccessKit); the Windows APIs come from the `windows` crate.

| Folder | Contents |
|---|---|
| `rust/snapcore/` | library shared by both programs, no interface: the `.snap` documents (`snap.rs`, over the compound file container of `container.rs`, the `cfb` crate), the capture library (`library.rs`), the animated GIF writer (`gif.rs`), the log, the registry settings (`reg.rs`), the interface language (`i18n.rs`: catalog and `.po` reader), the XML reader and writer, the image files (`codec.rs`: PNG, JPEG, BMP, GIF, TIFF, AVIF, HEIC through WIC, with metadata and EXIF orientation, and WebP with libwebp 1.5.0 from the `libwebp-sys` crate), the image effects and capture preset effects (`fx.rs`), Beautify (`beautify.rs`), image scaling and the GDI+ flat API (`gdip.rs`), computer vision (`cv.rs`), the PDF writer and page breaks (`pdf.rs`), the color texts (`color.rs`), HDR capture (`hdr.rs`: DXGI duplication and tone mapping), OCR and face detection (`ocr.rs`, Windows.Media.Ocr and FaceAnalysis through WinRT), QR codes and barcodes (`barcode.rs`, the `rxing` crate), HTTPS, SHA-256/HMAC, JSON and the Credential Manager secrets (`net.rs`), the upload destinations with their settings, sign-in and custom uploader syntax (`upload.rs`), the AI assistant's request (`ai.rs`), Office through IDispatch late binding (`office.rs`), the Editor's objects, their painters and hit-testing (`doc.rs`, `render.rs`) with the stamp images (`stamps.rs`: emoji through Direct2D, labels and cursors) and their XML in the `.snap` and on the clipboard (`serial.rs`), printf, atoi, atof, strtoul and sscanf through the C runtime (`crt.rs`, so numbers come out and read back as the C++ did them), single instance and the forwarded command line (`instance.rs`), the IPC channel (`ipc.rs`), the presets (`presets.rs`), the screen, window and cursor grabs (`screen.rs`), the links of the captured area (`links.rs`, UI Automation), the clipboard formats (`clip.rs`), dragging files out to other programs (`dragout.rs`), the capture sounds (`sounds.rs`), the panoramic stitching (`panorama.rs`), the recorder (`recorder.rs`: Media Foundation H.264/AAC in a fragmented MP4 remuxed at the end, WASAPI microphone and loopback mixed at 48 kHz with automatic gain, the cursor highlight, clicks, keys and zoom drawn into the frames, the recovery of interrupted recordings), video reading, cutting and GIF export (`video.rs`), the webcam (`webcam.rs`: Media Foundation source reader, RGB32 frames on their own thread, the chroma key) and the settings files (`settings.rs`: export, validation, replace or merge, the backups folder) |
| `rust/snapui/` | the interface shared by both programs: `host.rs` runs the windows itself on winit + egui-winit + egui-wgpu (one GPU device, a surface, renderer and egui context per window) instead of eframe, so each window keeps its Win32 class and a window subclass hands raw messages (WM_COPYDATA, WM_COMMAND, tray clicks, hotkeys) to the app; the code-built forms (`form.rs`: text, number, list, check, color and hotkey fields, a validation that keeps the form open, two columns when tall), the vector icons (`icons.rs`), the QR results dialog (`qr.rs`) and the upload destinations with their sign-in (`upload_ui.rs`) |
| `rust/snapnab/` | `Snapnab.exe`: the tray icon and menu (`tray.rs`), the command line (`cmdline.rs`, combined into one capture in `main.rs` as `ProcessCommandLine` did), the capture window (`window.rs`, in the library so `tests/window.rs` drives it with kittest; each control tagged with the C++ control's name as its AccessKit `author_id`, which UI Automation shows as the AutomationId), Options and the preset, effect, output, fixed region, video and webcam forms and the settings import and export (`dialogs.rs`, on their own threads), the global hotkeys with their low-level keyboard hook fallback (`hotkeys.rs`), the capture engine (`engine.rs`: delay, region, window, object, fixed, freehand and multi-area selection, full screen, monitor, clipboard and file inputs, windows as objects, the capture effects and the outputs) with the selection overlay in egui (`select.rs`, tested with kittest in `tests/select.rs`), video recording (`engine/video.rs` with the HUD in egui, `hud.rs`; the red frame, the countdown and the button tooltips as plain Win32 windows, `rawwin.rs`), the scrolling window, extended window and panoramic captures (`engine/scroll.rs`), the webcam bubble (`camwin.rs`), pinned images (`pins.rs`: one layered Win32 window per pin drawn with GDI+), the quick access overlay (`quick.rs`), One Click (`oneclick.rs`), the color picker, the screen focus and the clean desktop (`desk.rs`), the interval capture (`interval.rs`) and the upload and Office outputs on their own threads (`share.rs`). The raw Win32 windows send their requests to the app as `UiEv` through the event loop (`pins::send`) |
| `rust/snapedit/` | `SnapnabEditor.exe`: the library part has the document and its undo history (`doc.rs`), the canvas and the tools (`canvas.rs`, `tools.rs`, `edtools.rs`), the panels (`panels.rs`: properties in `props.rs` with the quick styles of `styles.rs`, effects in `effects.rs`, layers), the stamps gallery (`stamps.rs`, `stamp_data.rs`), the tray and the library view (`tray.rs`), the clipboard formats (`clipfmt.rs`), files and thumbnails (`files.rs`), the menus (`menu.rs`) and the WM_COMMAND numbers (`ids.rs`); the program (`src/app/`) has the main window, the commands, the documents, the clipboard, the extra tools (combine, batch, split, background removal, contrast, compare, assistant), the auxiliary windows and the video view |
| `rust/snapex/` | `SnapEx64.dll`: the WH_CALLWNDPROC hook that lets a window grow beyond the screen, with its messages in a section shared by every process |
| `rust/snaptool/` | `SnapnabTool.exe`, a helper for the tests and for debugging: `dump file.snap` prints the document XML, `audio file.mp4` reads the audio track |
| `rust/resources/` | icons, manifest (DPI PerMonitorV2, Common Controls 6) and `app.rc.in`, the resource script each program's `build.rs` fills in (`build_rc.rs`, compiled by `embed-resource`) |
| `tests/` | the end-to-end UI suite (`tests/e2e`, pytest + pywinauto) and the data the Rust tests check against (`tests/data`) |
| `help/` | the manual and its screenshots (`help/images`, regenerated by `scripts/manual-screenshots.ps1`) |
| `lang/` | interface translations: `<language>.po` (gettext) and the `snapnab.pot` template, generated by `scripts/i18n.py` |

The product version lives only in `[workspace.package] version` in `rust/Cargo.toml`; the programs read it as
`CARGO_PKG_VERSION` and the `VERSIONINFO` resources, `build.bat` and the installer take it from there. The C runtime is
linked statically (`rust/.cargo/config.toml`).

## How the reference architecture was followed

| Document | Implementation |
|---|---|
| Process model (pseudocode §1–§2) | single instance through a mutex + window (`SnapnabUI` / `SnapnabEditor`); the 2nd instance forwards its command line through `WM_COPYDATA` (`dwData 0x8010`, GUID + size + UTF-16 ≤ 10 000 bytes); the Editor starts the capturer if it is not running |
| `CCaptureEngine` state machine (§3) | `rust/snapnab/src/engine.rs`: `Idle → Activated → InputSelecting → InputSelected → Capturing → HaveInput → Outputting`, `Suspended` while the video is paused (all logged) |
| IPC (§4, §19.8, ed_comp_14) | `rust/snapcore/src/ipc.rs`: 4 KiB mapping + mutex + `…Event%d` + ack `…WaitUntilMessageSentEvent%d`; message `[code][kind][payload]` (int / 16 B / UTF-16); waits for the peer 75×200 ms; 10 s ack only if the peer is not hung (`IsHungAppWindow`). Codes: `0x06` show, `0x0A` close if hidden, `0x0C` reload, `0x1E` new image from clipboard, `0x02/0x03` document opened/released, `0x12` capture requested by the Editor, `0x13` Editor closed; Snapnab's own: `0x30` open file, `0x31` library changed, `0x32` restart the Editor (language change) |
| Extended window (§6) | `rust/snapex/src/lib.rs` (`Snap5000`/`Snap5001`) |
| Automatic scrolling (§3.2, §9.5) | mouse wheel scrolling + an in-house `PhaseCorrelationTranslationDetector` (2D FFT, `conj(B)`, `DBL_EPSILON`, Hann window, negated peak) with SAD verification and stitching |
| Panoramic scrolling (comp_1/comp_3) | `rust/snapnab/src/engine/scroll.rs` and `rust/snapcore/src/panorama.rs`: the chosen region is copied ~11 times per second while the user scrolls; the offset between frames comes from comparing row (or column) signatures, and only the new strip is pasted, so fixed headers are not repeated |
| One Click (comp_7 §3) | `rust/snapnab/src/oneclick.rs`: layered window docked to an edge of the screen; collapses to an accent-colored strip and opens with the mouse over it; dragging detaches the panel and it snaps to the nearest edge (`OneClickEdge`, `OneClickPos`, `OneClickMonX/Y`, `OneClickEnabled` in the registry); hides during a capture and stays out of captures (`WDA_EXCLUDEFROMCAPTURE`) |
| Webcam (comp_3 §8) | `rust/snapcore/src/webcam.rs` and `rust/snapnab/src/camwin.rs`: `MFEnumDeviceSources` + `IMFSourceReader` (RGB32 through the video processor) in a mirrored layered window inside the recorded area (circle, rounded square, square, 16:9 or 4:3); since the recording copies the screen, the webcam ends up in the video; chroma key by chrominance distance (Cb/Cr) with a soft edge and halo reduction; the HUD has the show/hide button (`ToggleWebcamCommand`) |
| Grab Text | "Capture text" preset: the capture goes through OCR (`rust/snapcore/src/ocr.rs`), the text goes to the clipboard and opens in a window |
| Computer vision (§9) | `rust/snapcore/src/cv.rs`: cost-ordered wand, global selection, `DoWand` (clamp 0.01–100, shortcut 100), Dingo7 (dominant color of the dilated neighborhood), diffusion (role of Navier-Stokes), Smart Move automatic tolerance (formula from §9.6) |
| `.snap` format (ed_comp_2 §3) | `rust/snapcore/src/snap.rs`: 24-byte header + OLE compound file; streams `Index` (XML), `Frame%d/Document`, `Frame%d/ImageStorage/Image_N.png`. The whole document (index, frame documents, base and embedded images, photo metadata) is read and written over the compound file of `container.rs` (the `cfb` crate, version 3 files like the Windows API writes) |
| Library (DataStore) | `%LOCALAPPDATA%\Snapnab\Library` (`<GUID>.snap`, `Video_*.mp4`, `Thumbnails\`); the folder is watched by the Editor. Capture names (the reference's DataStore keeps them in its database) are in `Names.xml` in the library, by file name. Listing, adding, thumbnails, names, cleanup and automatic names are in `rust/snapcore/src/library.rs`; the thumbnail is scaled (`MakeThumbnail`, GDI+ in `gdip.rs`) |
| Interactive or fixed region (comp_1 §6, comp_7 §2) | Region gear: `RegionSelectionType` Interactive/Fixed, `Region{Left,Top,Width,Height}` and `SetPosition`; with a position, the input is ready without a selector (`INPUT_REGION_FIXED`); without one, the crosshair shows a fixed-size rectangle to place |
| Presets (comp_7) | `Presets1.xml` with capture, effects, outputs, audio/video and hotkey in the same object; `.snappresets` import/export |
| Hotkeys (comp_7 §4) | `RegisterHotKey` with internal conflict detection; an external conflict (e.g. Windows' Print Screen) falls back to a `WH_KEYBOARD_LL` hook; Esc registered as hotkey id 3 while the crosshair is active |
| Video (§11, comp_3) | `rust/snapcore/src/recorder.rs`: BitBlt → `IMFSinkWriter` H.264/MP4 (optional hardware, falls back to software), WASAPI microphone + system loopback, mixer with separate mute and volume (0–200%, automatic microphone gain optional) → AAC, pause without a time gap, downscaling to 1080p, cursor, disk limits, prevents sleep. Before the recording the HUD opens the sources only for the meters (`StartMonitor`) |
| MP4 cutting (§11.4) | `TranscodeWithCuts` with `IsInsideAnyCut` / `PrecedingCutsDuration` (PTS rewriting) |
| GIF (§11.5) | `rust/snapcore/src/gif.rs`: in-house LZW, 4 bpp (16 colors) or 8 bpp, median cut palette, Floyd–Steinberg dithering, delta frames; the palette sorts with a port of the MSVC `std::sort`, so the files stay byte for byte those of the former C++ writer; frames of another size are scaled with GDI+ (`ScaleImage`, `gdip.rs`) |
| PDF (§10.1) | A4 or Letter page according to the measurement system, orientation from the image, margin 0; captured links become `/Link` annotations with a `/URI` action |
| Polygon objects (comp_5 §5.1 `CPaintOpPolygon`) | the Highlighter, Spotlight and Hide areas take an `AreaShape` (the `ShapeKind` outlines, a clicked polygon or a freehand outline) and keep the corners in `Points`, as the reference's `<Polygon>` |
| Layers and z-order (comp_5 §4 `CPaintOpReorderComponentSelection`) | `rust/snapedit/src/panels.rs`: hide, lock, rename and reorder are document operations with an undo step; `Hidden`, `Locked` and `Name` attributes of each object in the `.snap` |
| Interface language (ed_comp_1 §3, step 5) | `rust/snapcore/src/i18n.rs`: like the `SnagItEditorRes.dll` in the language folder, each program loads `lang\<code>.po` from the executable's folder at startup; the code stays in Portuguese and `tr("…")` / `tr!("…")` returns the translation (static tables translate their strings when shown). Language = the one chosen in *Options* (`Language` in `HKCU\Software\Snapnab\1`) or the first Windows display language with a catalog, otherwise English; anything missing from a catalog falls back to English. Changing the language restarts both programs (`/restartwait` waits for the previous instance to close). Built-in names stored as data (presets, themes, quick effects) stay in Portuguese in the file and are translated when shown. the formatting (`crt.rs`, `_vswprintf_p`) lets translations reorder the values (`%1$s`) |

## Capture window

Compact and fixed-size, with a custom title bar (icon and the File, Capture, View and Help menus on the same strip,
also opened with Alt+letter and F10). Image/Video tabs are vertical; Selection, Effects and Share fields (image) or
Selection, Quality and Webcam (video); option switches and Delay; a round red capture button (the hotkey is in its tooltip).
Bottom bar: ⚙ Presets (opens the list), + (new preset from the current settings, new preset, help) and Open Editor.
Each row of the list has "Add hotkey" and a gear (capture, rename, hotkey, duplicate, delete).
The colors are slightly tinted toward the Windows accent color (Personalization > Colors) and follow changes immediately.

The controls are egui widgets (`rust/snapnab/src/window.rs`) with the C++ control IDs as their AutomationIds: toggle switches, flat or icon-only buttons, "Label Value ›" rows that open a menu and combo boxes with
a drop-down list, which can have the settings gear inside (`MUI_GEARCLICK` notification). The **Effects** box lists
Border, Filters, Shadow, Capture info, Color replacement, Resize image, Edge effects, Trim edges,
Color adjustment, Rotate image, Watermark and Image resolution; choosing one opens its settings and turns it on, with
"Add another effect" and "Remove this effect" to combine several.

## Editor

Same title bar, colors and controls as the capture window. At the top, Capture and Record on the left, Copy all,
Save as and Share on the right and, in the middle, the customizable toolbar (TopPanel.ToolBar, ed_comp_3 §3.2): items of the
`ToolBarItemType` kinds — tool, effect, output, command and spacer — and "More/Edit" at the end, with the tools
that are not on the bar and the customization. Automatic: Move, Selection, Text, Highlighter and Shape, the 4 most
recently used tools and the Magnifier; custom: any item, through the *Customize toolbar* window (available × on the bar)
or by dragging one item over another. Registry: `ToolbarCustom`, `ToolbarItems` (`t:5,e:1039,o:4,c:1015,s:0`) and
`ToolbarRecentTools`.

On the right, the Properties and Effects tabs. The effects panel (EffectsPanelControls, ed_comp_5 §4) has the quick
effects gallery at the top (`%LOCALAPPDATA%\Snapnab\QuickEffects.xml`; a quick effect combines border, edge effect, page
curl and shadow, and "Add" saves the panel's current settings as a new one) and, below, Border, Shadow,
Perspective, Page curl, Edge effects, Filters, Color adjustment, Color replacement, Spotlight, Watermark and
Caption. Clicking an effect opens its settings inside the panel; each change shows the live preview in place of the image
(PreviewEffect) and *Apply* writes the effect with one undo step (ApplyEffect). Border, shadow and caption keep the
objects editable (only shifted); the others flatten the objects first, so the preview and the result are the same. The
*Image › Effects* menu and the toolbar's effect items open the same panel. The same function produces the thumbnails (with
pixel values scaled to the thumbnail size, rebuilt 350 ms after the last edit).

The Highlighter creates `Highlighter` objects (`CPaintOpHighlight`, comp_5 §3.2) in the chosen color and opacity, in
any area shape (rectangle by default); old freehand highlighters still open with their color multiplied over the image.

New object properties of this version are written to the `.snap` only when they are used and read with defaults, so
older files open unchanged: `Name`, `Hidden`, `Locked`, `Floating` (lifted piece), `AreaShape` + `Points`,
`LineSpacing`, `StepOutlineColor`/`StepOutlineWidth`, `FillGradient`/`GradientAngle`/`GradientStops`
(`pos:#AARRGGBB;…`), `BackgroundBlur`/`BackgroundBlurAmount`, `Loupe`/`LoupeHandle`, the arrow label (`Text` + font
attributes on `<Arrow>`), `<Link … Url>` objects and the document's `<Guides>`. The quick styles (`StyleToXml`) carry
the same style fields.

Canvas extras: rulers and guides (`Rulers` in the registry, guides per document), edge snapping by contrast for the
selection and crop (`EdgeSnap`; the strongest column/row difference within 6 screen pixels along the edge), axis lock
with Shift while moving, the polygon tool, the context menu, exact geometry (`GeometryKind`/`SetGeometry`) and the lifted
piece (a `floating` picture object merged by *Drop piece*). Copying objects puts them on the clipboard as
`Snapnab.Objects` (the objects' XML with the embedded images in base64) and an area of the image adds `Snapnab.Origin`;
*Paste in place* uses them. *Paste text without formatting* (`PlainTextPaste`) takes, when the formatting is kept, the
first run of *Rich Text Format* or *HTML Format* (`rust/snapedit/src/clipfmt.rs`).

Properties tab (`rust/snapedit/src/props.rs` + Quick Styles, ed_comp_3 §2.3 and ed_comp_8 §5): at the top, the
tool's quick styles (`styles.rs`): each theme stores up to 4 colors and the styles of every tool are
generated from them (GenerateQuickStylesForTheme), plus the ones the user adds with "+"
(`%LOCALAPPDATA%\Snapnab\ToolThemes.xml`; `.snaptheme` export). The star marks a style as a favorite
(`Favorites.xml`); the Favorites tool shows all of them and, when one is chosen, switches the drawing tool while keeping
the bar on Favorites. For the Stamp tool, the gallery (`rust/snapedit/src/stamps.rs`): categories of color emoji (drawn with
Direct2D in `rust/snapcore/src/stamps.rs`, at the object's size) and My stamps (`%LOCALAPPDATA%\Snapnab\Stamps`).
Below, the tool properties, drawn by the panel itself: colors (palette), shadow
(ShadowPropertiesControl: direction on the 3×3 grid, color, opacity, blur, distance), shape, ends and dash pattern,
sliders with a value box, advanced text. Objects are drawn on their own layer (`rust/snapcore/src/render.rs`):
the shadow comes from the layer's alpha and the opacity applies to the whole object. The current styles of each tool are
stored in `ToolStyles.xml`.

Customize toolbar (CBCGPToolbarCustomize): grid window with Drawing tools, Menu items, Share outputs, Effects and
Miscellaneous; dragging an item onto the bar inserts it where the bar indicates; while the window is open the
bar is in customization mode and dragging an item off it removes it. Cancel restores the previous bar. The
same window chooses how many tools the automatic bar shows (6 to 16, `ToolbarToolCount`).

Image menu (as in the reference): Tools, Effects (items with icons, 32 bpp `hbmpItem`), Arrange (order one step
or all the way, Align — one object relative to the image, several among themselves —, Flip the selected objects or the image),
Group/Ungroup, Flatten selected / Flatten all, Trim, Resize image/canvas, Apply template
(disabled: templates not implemented), Rotate, Snap to canvas (`CanvasSnapping`), Canvas color and Tag
image (`Tags` attribute of the `.snap` document).

Multiple object selection (`Canvas`): the primary object (`selected`, the last one clicked, whose properties appear in the
panel) plus the other selected ones; Ctrl/Shift+click adds or removes, and the Move tool's selection rectangle picks the
objects it touches. Group (`CPaintOpGroup`, comp_5 §3.2) replaces the selected objects with a `DrawObj` of type Group that
stores the children by value (copying the group copies the children, which keeps undo correct); in the `.snap` it is
`<Group X1… >` with the children's elements inside.

At the bottom, the recent captures tray (Tray, ed_comp_3 §5): `Tray.bin` in the library stores the order, up to
`RecentTrayDocs` items (default 10); new captures and opened documents go to the front, a deleted item frees a slot
for the next capture and "Remove from tray" does not come back by itself. Dragging items to another program
(TrayDragDrop) delivers PNGs with the annotations (written to `%TEMP%\Snapnab`) or the MP4 in a custom `IDataObject` with
`CF_HDROP` and `Shell IDList Array`, preferred effect "copy" and "move" allowed only for the temporary copies (Paint
only accepts "move"; the MP4 is the library file); the `WM_DROPFILES` that arrives when the item is dropped back on the
Editor itself is discarded at the end of the drag. Files dropped from Explorer are imported. Holding the button on empty
space draws the selection rectangle (Ctrl/Shift add to the selection). *Library* (or *View › Library*) shows all
captures in a grid in place of the image, with multiple selection, batch export and delete, and a search by name.
Resting the mouse on a thumbnail shows a larger preview (the composed image, or the first frame and the duration of a
video). `Tray.bin` is only rewritten when the tray changes (the library folder is watched).

Stamps: the *Labels* category draws rounded badges at run time (`label:RRGGBB:Text`, the text in the interface language
when the stamp is placed, stored in the object); the gallery order and the pinned stamps are in `StampOrder.xml`, by
category key (language independent).

Settings backup (`rust/snapcore/src/settings.rs`): one XML file (`.snapsettings`) with every value of `HKCU\Software\Snapnab\1`
and its subkeys and the settings files of the data folder (base64). Importing closes the Editor first (it writes its
settings when it closes), saves the current settings to `Backups\Before-import-*.snapsettings`, replaces or merges
(registry values that already exist, list items with the same name and the tool styles stay; the missing presets,
themes, favorites, quick effects and stamp categories are added) and restarts both programs. The capture
program writes `Backups\Settings-YYYY-MM-DD.snapsettings` once a week and keeps the last 5.

Capture of each window as an object (full screen / monitor preset option): `PaintDesktop` draws the wallpaper, the
taskbars come from the screen capture and each visible window (`EnumWindows` z-order, without cloaked, transparent or
own windows) is captured with `PrintWindow(PW_RENDERFULLCONTENT)`, cut to `DWMWA_EXTENDED_FRAME_BOUNDS`, with the
Windows 11 rounded corners; programs that do not draw this way fall back to their visible part. The library receives a
`.snap` with the background and one `<Image Name="title">` per window. Links (`rust/snapcore/src/links.rs`): UI Automation searches the
`Hyperlink` elements of browser and document viewer windows under the area (URL from the Value or LegacyIAccessible
value), on a worker thread limited to 900 ms; they become `<Link>` objects (they follow scale and border effects; other
effects that change the geometry drop them).

## Competitor feature batch (C01–C44)

The features of the competitor feature request (local notes in `docs/pesquisa/`, not part of the repository) follow the patterns above (presets,
`snapui::form`, objects added through the canvas, settings in `HKCU\Software\Snapnab\1` or the data folder,
exported with the settings) and, where it has the feature, the ShareX reverse-engineering dossier
(`docs/pesquisa/01-ANALISE.md`, `02-EDITOR_ANALISE.md`). Map:

| Feature | Module |
|---|---|
| Auto-copy, quick thumbnail (C08), capture sound (C31), clean desktop (C16), HDR (C02), rounded corners (C06), cursor as object (C25) | `snapnab/src/engine.rs`, `snapnab/src/quick.rs`, `snapcore/src/sounds.rs`, `snapnab/src/desk.rs`, `snapcore/src/hdr.rs` (DXGI FP16 duplication + tone mapping) |
| Pins (C01) | `snapnab/src/pins.rs` (renders `.snap` objects with `snapcore/src/render.rs`) |
| Region history, keyboard selection, color in the crosshair (C18, C19, C10) | `snapnab/src/select.rs`, `snapnab/src/desk.rs` (color picker), `snapcore/src/color.rs` |
| Interval capture (C24), screen focus (C39), library cleanup (C41), command line (C44) | `snapnab/src/interval.rs`, `snapnab/src/desk.rs`, `snapcore/src/library.rs`, `snapnab/src/cmdline.rs` |
| WebP/AVIF/HEIC and metadata (C03, C29) | `snapcore/src/codec.rs` (WebP with libwebp 1.5.0 through `libwebp-sys`; AVIF/HEIC through WIC, HEIF encoder with AV1) |
| QR (C04) | `snapcore/src/barcode.rs` (the `rxing` crate, a Rust port of ZXing); `snapui/src/qr.rs` (results dialog) |
| Beautify (C05) | `snapcore/src/beautify.rs` (the render); `snapnab/src/dialogs.rs` (capture effect) and `snapedit/src/app/extras.rs` (Editor dialog and presets) |
| Copy as (C30), drag out | `snapedit/src/app/extras.rs` (Copy as), `snapcore/src/dragout.rs` |
| Office (C20), upload (C21) | `snapcore/src/office.rs` (IDispatch late binding), `snapcore/src/upload.rs` + `net.rs` (WinHTTP, SigV4, OAuth PKCE loopback, Credential Manager); `snapui/src/upload_ui.rs` (dialogs) |
| Combine, redaction, cursors, date stamp, background removal, split, contrast (C09, C11, C25–C27, C34, C36) | `snapedit/src/edtools.rs`, `snapedit/src/app/extras.rs`, `snapcore/src/stamps.rs` (cursors) and `snapcore/src/pdf.rs` (PDF writer, smart page breaks) |
| Compare (C17), AI assistant (C22), batch (C28) | `snapedit/src/app/windows_aux.rs` (Compare and the assistant window; the request in `snapcore/src/ai.rs`), `snapedit/src/app/extras.rs` (batch) |
| Smart highlighter, text only (C12), draw & hold (C23), touch/pen (C43) | `snapedit/src/canvas.rs` (OCR word boxes from `snapcore/src/ocr.rs`), `recognize_stroke` in `snapedit/src/tools.rs` |
| Hand-drawn style (C35), vertical text and text on a path (C37) | `snapcore/src/render.rs`, `snapcore/src/doc.rs` (new `Style` fields, written to the `.snap` only when set) |
| Clicks/keys overlay (C07), zoom (C15), crash-safe recording (C14), audio only (C32), restart confirmation (C33) | `snapcore/src/recorder.rs` (low-level hooks feed the overlays; fragmented MP4 remuxed at the end; interrupted recordings recovered at startup), `snapnab/src/hud.rs` and `snapnab/src/engine/video.rs` |

## Deliberate differences from the reference

- **Not implemented (out of scope or dependent on third-party services):** licensing, telemetry, cloud crash
  reporting, updater, account/login, JWT feature switches, cloud plugins (`.tscplugin`), COM automation of Snapnab
  itself (Office is driven as a COM client), virtual printer, helper process with `uiAccess`, phone integration, drop
  zones (templates are the *Combine images* layouts), Simplify, spell checker,
  XLIFF translation, video from images, full-screen webcam (the *switcher* that swaps the screen for the camera) and
  All-in-One mode.
- **Replacements for third-party components:** LEADTOOLS → WIC (plus libwebp for WebP); ABBYY → Windows OCR
  (`Windows.Media.Ocr`, depends on the installed languages); barcode reading → rxing (Rust port of ZXing); SQL CE → the library folder itself; OpenCV → in-house algorithms in `cv.rs`; Cairo/Pango → GDI+.
- Help: the reference opens its help online (`IRedirect.LaunchHelpRedirect`, `HELP_*` topics); Snapnab ships an
  offline manual, `help/Snapnab-Manual.html`, opened by *Help › Snapnab Manual* and F1 (`open_manual` in
  `rust/snapnab/src/main.rs` and `rust/snapedit/src/app/extras.rs`). A topic is a `#section` anchor; since a file association drops the fragment, the default
  browser is started with the `file://` URL. In the Editor, F1 opens the anchor of the current tool, the library or the
  video editor.
- `SnapEx64` installs the hook only on the target window's thread (the reference uses a global hook), so the DLL is not
  injected into the whole desktop.
- Dingo7 counts only the ring around the selection (the pseudocode includes the object's own pixels, which would pull
  its color in).
- The "Image resolution" effect only applies to files saved by the capture; the image sent to the Editor does not keep the DPI.
- Inno Setup installer (`Snapnab-<version>-setup.exe`, per user without administrator rights or for all users) instead
  of the reference's MSI installed with `msiexec` (comp_9); there is no automatic updater.
- Translations in gettext catalogs instead of per-language resource DLLs (same choice at program startup, but
  editable without compiling). In Arabic and Urdu each string reads right to left (RLE/PDF embedding), but the
  window layout is not mirrored (egui draws left to right).

## Rust rewrite: decisions to review

Where the Rust version could not do a thing the way the C++ did, it was decided and noted here (branch `rust-rewrite`):

- The capture window's controls are egui widgets, not Win32 child windows. The C++ control IDs survive as the
  AutomationId of each widget (and as WM_COMMAND ids for the tabs, the presets button, the capture button and every
  menu command), so automation and the e2e suite find them; `tests/e2e/tests/test_05_capture_window.py` clicks them
  with the real mouse through UI Automation (`snapnab_e2e/uia.py`) instead of sending `CB_SETCURSEL` / `BN_CLICKED`.
- The capture window keeps the standard Windows title bar for now (the C++ draws its own, with the menus in it); the
  menus are an egui menu bar below it. The look (light theme, accent colors, the tab and capture button drawing) waits
  for the design pass.
- The Beautify effect of a capture preset is edited in a plain form: the named Beautify presets (the picker and
  "save as preset") come with the Editor's Beautify dialog in phase 6.
- The Editor (phase 6) is one egui window with the C++ class names and WM_COMMAND numbers. The canvas, the right
  column (Properties, Effects, Layers), the toolbar, the footer and the trays are egui widgets with AutomationIds
  (`SnapnabCanvas`, `SnapnabEffects`, `SnapnabEffects.Apply`, `SnapnabLayers`, `SnapnabToolbar.<id>`, …), not child
  windows; the e2e suite finds them through UI Automation.
- The forms that showed a live preview in the C++ (Combine, Hide sensitive data, Remove background, Beautify, Split
  image, Batch) are plain forms (`snapui::form`): the result shows on the canvas after OK and Undo takes it back.
- The canvas color uses the Windows color dialog; the contrast checker has no screen eyedropper (the color dialog and
  Copy HEX only).
- The read-QR and upload dialogs moved to `snapui`, so the capture app and the Editor share them.
- The text windows (Grab Text, shortcuts), Compare, the contrast checker, the AI assistant, Customize toolbar and the
  Magnify intro are egui windows of their own with the C++ class names (`SnapnabTextWnd`, `SnapnabCompare`, …). They do
  not disable the Editor while open (the C++ ran them as modal loops). Grab Text's text is the UI Automation value of
  `SnapnabTextWnd.Text`.
- Customize toolbar is a list (add from the catalog, move up/down, remove) instead of drag and drop on the bar.
- The video view is drawn inside the main window (the C++ was a child window, `SnapnabVideoView`, now the
  AutomationId of its area). Its buttons post `WM_COMMAND` 2001–2011 (the C++ B_* + 2000) to the main window, and the
  long saves (MP4, GIF) show the shell's progress dialog.
- `SNAPNAB_NO_RECYCLE=1` deletes instead of sending to the Recycle Bin (only for the test runs, which must not fill the
  user's Recycle Bin).
- Where the Editor's egui layout differs from the C++ one (the tray and the library view, Customize toolbar, the layers,
  the zoom and size boxes of the footer), some texts changed: the new ones are translated in every catalog, and the
  C++ texts no longer shown stay in the `.po` files as obsolete entries (`#~`), ready if they come back with the
  design pass.
- The checks against the former C++ output, which the C++ test program ran, are a Rust test now
  (`rust/snapedit/tests/golden.rs`, part of `cargo test`). The barcode reader is rxing, so its boxes are compared with
  ZXing-C++'s by overlap (a 1D code's box is the row rxing read), as the C++ test already did.

## Tests

`cargo test --workspace` in `rust/` (or `build.bat test`) runs the Rust tests: the compound file container, the
`.snap` documents (write and read back, damaged files: garbage after the header, a file cut in half), XML, image
files, the `.po` reader and the shipped catalogs (all complete and with the same `%d`/`%s` markers as the original),
the registry round trip, phase correlation, wand and inpainting, rendering of every object, PDF, 4/8 bpp GIF, effects,
IPC between sender and receiver, MP4 recording, cutting and GIF from video, OCR and the panoramic stitching. The
interface is tested with `egui_kittest` (`snapui/tests`, `snapnab/tests`), against images in `tests/snapshots`.

The parts moved from C++ are checked against what the former C++ code gave, kept as data in `tests/data`
(`rust/snapedit/tests/golden.rs` builds the same lines; a mismatch writes this build's to `rust/target/golden`): WebP
against `webp-libwebp-1.5.0.txt` (the size and hash of the files that build wrote, lossless and five qualities, and
of the pixels it read back); the effects, Beautify, scaling, computer vision and PDF export against `pixels-cpp.txt`
(the size, hash, resolution and metadata of what it made of the same generated images); the barcode reader against
`barcodes.txt`, what ZXing-C++ 2.2.1 read in `barcodes/*.png` (the same codes, and boxes that overlap); the color texts, the HDR tone mapping and OCR against
`misc-cpp.txt`; SHA-256/HMAC, URL encoding, JSON, multipart bodies, the assistant's request bodies, AWS Signature V4
and the custom uploader syntax against `net-cpp.txt`; the Editor's painters against `render-cpp.txt` (every object
type and style variation, with the bounds, text and label boxes, outlines and stamp images they measured); the
objects' XML, older and hand-written files, the clipboard text, a `.snap` saved and opened and hit-testing on a grid
against `doc-cpp.txt`; the presets against `presets-cpp.txt`. The GIF writer writes the same bytes as the C++ one.

The end-to-end suite (`tests/e2e`, see its README) drives the real programs through UI Automation and the mouse:
lifecycle and IPC, every capture input and effect, video, the capture window, the Editor's tools, commands, files and
video editing, pins and the overlay, the tools and the outputs (upload against a local fake server, Office).
`SnapnabTool.exe dump file.snap` prints a document's XML and `SnapnabTool.exe audio file.mp4` reads a recording's
audio track; the e2e suite uses both.
