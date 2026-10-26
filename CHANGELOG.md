# Changelog

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/).

## [0.3.0] — 2026-10-26

### Added
- **User manual** in English (`help/Snapnab-Manual.html`, installed next to the programs): quick-start lessons and a
  complete reference of every capture, recording, preset, Editor, library and settings feature, with keyboard shortcut
  tables, troubleshooting and a glossary; contents sidebar, search, light/dark theme and print layout. It opens from
  *Help › Snapnab Manual* and with **F1** in the capture window and the Editor; in the Editor, F1 opens the chapter of
  the current tool (or of the library or the video editor). The installer adds a *Snapnab Manual* Start menu shortcut.
- Editor: **Shift draws perfect geometry in every shape-based tool** — rectangle → square, ellipse → circle, rounded
  rectangle → rounded square, triangle → equilateral, hexagon → regular, lines and polygon sides in 45° steps, stamps in
  the proportion of their image — in Shape, Highlighter, Spotlight, Hide, Magnifier, Callout and Stamp, and a square
  (or circle) raster selection and crop area. Shift while
  dragging an object that already exists locks the movement to the axis the mouse moved along first (arrow-key nudging
  is unchanged); Shift while dragging a line end keeps 45° steps.
- Editor: Highlighter, Spotlight and Hide (blur/pixelate) in more shapes: rectangle, rounded rectangle, ellipse/circle,
  triangle, diamond, parallelogram, hexagon, star, **polygon** (click the corners; click the first one, double-click or
  press Enter to close; Backspace removes the last corner; Esc cancels) and **freehand**. The shape of an existing area
  can be changed (a polygon or freehand outline starts from its box). New quick styles with ellipses.
- Editor: **Layers panel** (third tab on the right, *View › Layers panel*), Photoshop-like: every object from front to
  back with a thumbnail, eye (hidden objects are not drawn nor exported), lock, name (double-click or F2 renames),
  groups that expand to show their children, click/Ctrl/Shift selection shared with the canvas, drag to reorder and a
  context menu; the image is the *Background* row. Visibility, lock and names are stored in the `.snap`.
- Editor: **lock objects** (*Image › Lock object*, Ctrl+L; Unlock, Ctrl+Shift+K; context menu, properties panel and
  layers panel): a locked object shows a lock badge and cannot be moved, resized, cut or deleted.
- Editor: right-click **context menu** on the canvas (cut, copy, paste, paste in place, delete, lock, rename, order,
  align, distribute, group, drop piece, open link).
- Editor: **exact position and size** — X, Y, width and height fields (image pixels, with an aspect-ratio lock) for the
  selected object, the raster selection and the crop area (for example, crop to exactly 800×600); changes apply live.
- Editor: **rulers, guides and cursor coordinates** (*View › Rulers and guides*, Ctrl+Shift+R; off by default and
  remembered): rulers that follow the zoom and scrolling, guides dragged out of the rulers (moved with the Move tool,
  removed by dragging them back onto a ruler, *View › Remove the guides*, stored in the `.snap`), X/Y of the cursor in
  the status bar; with *Snap to canvas* on, objects snap to the guides.
- Editor: **selection and crop snap to contrast edges** (window borders, buttons, table lines) — *Edit › Selection snaps
  to image edges*, on by default; Alt turns it off on the fly.
- Editor: **distribute** 3 or more objects with equal gaps, horizontally or vertically (*Image › Arrange › Align*,
  context menu and toolbar customization, which also gets the six Align commands, Lock and Rulers).
- Editor: **paste in place** (*Edit › Paste in place*, Ctrl+Shift+V): objects copied in one image are pasted at the same
  position in another one (with Ctrl+V, in the same image, the copy is shifted a little); a copied area of the image
  keeps its position too. Copying objects now also puts them on the clipboard in the Editor's own format, with
  their embedded images.
- Editor: **paste text without formatting** by default (*Edit › Paste text without formatting*): text pasted into a text
  or callout takes the object's style; Ctrl+Shift+V while typing keeps the source font, size, color and bold/italic/
  underline (read from *Rich Text Format* or *HTML Format*). Text pasted on the canvas becomes a Text object.
- Editor: **line spacing** (80% to 300%) for Text and Callout, also in the quick styles.
- Editor: **text attached to an arrow** (Skitch style): after drawing an arrow, just type — the text appears beyond the
  tail and follows the arrow when either end moves; double-click edits it.
- Editor: **frosted-glass callouts**: callout option that blurs what is behind the body (on/off and strength), with a
  fill opacity slider.
- Editor: **outline on numbered steps** (color and width) for contrast on light and dark backgrounds; new step quick
  styles with an outline.
- Editor: **realistic magnifier**: thick shaded rim, inner shadow and reflection, optional handle; new magnifier shapes
  (hexagon, diamond, star) and loupe quick styles.
- Editor: **gradients** (linear at an angle or radial, 2 to 4 color stops) in the Shape fill, the Fill tool and
  *Image › Canvas color*; the color palette shows gradient presets and *Edit gradient…*.
- Editor: **label stamps** in the new *Labels* category: Before/After, Wrong/Right, Incorrect/Correct, New, Updated,
  Yes/No, Do/Don't, Important, Tip, Note, Example, Draft, Beta, Deprecated and Coming soon, in the interface language.
- Editor: **reorder stamps** by dragging them in the gallery and **pin favorites** (right-click › *Pin to the top*): pinned
  stamps come first in their category and in the *Pinned* category; the order is remembered (`StampOrder.xml`).
- Editor: **lift a piece, edit the rest, drop it back**: *Edit › Lift out the selection* (Ctrl+Shift+L, also in the
  Selection tool) turns the selection into a floating piece; crop or cut out the rest of the image, move the piece (it
  can be dragged with the Selection tool too) and press Enter (or double-click it, or *Drop the piece into the image*) to merge it.
- Editor: **names on captures**: rename from the tray/library context menu or with F2 (*Save as* also names an unnamed
  capture); the name replaces the date in the tray and the library, is the document title and can be searched in the
  library's new search box.
- Editor: **larger preview on hover** in the tray and the library: resting the mouse on a thumbnail shows the capture
  with its annotations, size and date; videos show their first frame and duration.
- Capture: **each window as a separate object** (Selection gear of Full screen/Monitor): the capture opens in the Editor
  with the desktop wallpaper and the taskbar as the background and every visible window as an image object, named
  after its title, which can be moved, hidden or deleted.
- Capture: **links of web pages and documents** (Selection gear, on by default): links found with UI Automation in
  browsers and document viewers under the captured area are stored with the image as clickable areas (outlined with
  the address when the mouse is over them, double-click opens) and stay clickable in PDF exports.
- Video: **manual volume** of the microphone and the system audio (0–200%) and the choice of automatic microphone gain,
  in *Video options* and in a volume panel of the recording HUD, before and during the recording; the meters show the
  adjusted level (they already work before the recording starts) and the preset remembers the values.
- **Backup of the settings**: *File › Export all settings…* / *Import all settings…* (one `.snapsettings` file with the
  registry settings, presets, themes and quick styles, favorites, tool styles, quick effects, stamp order and toolbar
  layout); importing asks whether to replace or merge, keeps a copy of the current settings and restarts both
  programs. A backup is also written automatically once a week to `%LOCALAPPDATA%\Snapnab\Backups` (the last 5 are kept).

### Fixed
- Editor: the properties panel could keep the old pixels of a number field over a button after another object was
  selected.
- Editor: the recent captures tray reloaded itself every 400 ms (it rewrote `Tray.bin`, which is in the watched library
  folder), which kept dropping the hover highlight.
- Editor: zooming with Ctrl+wheel on a touchpad (small wheel steps) jumped a whole zoom step on each event; the zoom now
  follows the size of the wheel step and stays centered on the mouse.

### Changed
- Code comments, internal log messages, test output, build scripts and documentation are now in English. The packaged
  readme is now `README.txt` (it was `LEIA-ME.txt`).
- Editor: Ctrl+click or Shift+click on a selected object removes it from the selection when the mouse is released
  without moving (before, on the press), so Shift+drag can move the selection.
- Editor: right after drawing an arrow, letter keys type its label instead of switching tools (click elsewhere or press
  Esc first to use the tool shortcuts).

## [0.2.0] — 2026-10-25

### Added
- Interface in 12 languages, the most widely spoken: Portuguese, English, Chinese (Simplified), Hindi, Spanish, Arabic,
  French, Bengali, Russian, Indonesian, Urdu and German. The default is the Windows language and, if it has no
  translation, English; *File › Options… › Language* picks another one and restarts Snapnab and the Editor to apply it.
  Chinese, Hindi and Bengali use a font with the language's characters; in Arabic and Urdu the strings read right to
  left (the window layout stays the same).
- Translations in gettext catalogs (`lang/<language>.po`, template `lang/snapnab.pot`), editable in Poedit;
  `scripts/i18n.py` extracts the strings from the code and checks the catalogs (missing strings and `%d`/`%s` markers).
- `Snapnab-<version>-setup.exe` installer, built by `build.bat` along with the portable zip (when Inno Setup 6 is
  installed). It installs for the current user only, without asking for administrator rights, or for all users;
  creates the Start menu shortcuts, an optional desktop shortcut and "start with Windows"; closes a running Snapnab
  before updating; uninstalls from *Settings › Apps* keeping captures and settings. The wizard comes in the same 12
  languages as the program (chosen from the Windows language), follows the Windows light/dark theme and uses the
  Snapnab icon.
- Webcam in video recordings: pick the camera under *Webcam*; it appears inside the recorded area, with a button on the
  recording panel to show or hide it. Shapes: circle, rounded square, square, 16:9 and 4:3 rectangles, plus background
  removal by color (chroma key, with adjustable color and tolerance), which leaves the webcam without a frame.
- Gear on the "Region" Selection (image and video): *Select the region during capture* or *Fixed region* (width, height
  and, optionally, a fixed position on the screen, captured without a selector). Replaces the "Fixed-size region" item
  of the list; old presets are converted.
- Editor: centered toolbar with 10 tools and a "More" button with the rest. By default Move, Selection, Text,
  Highlighter and Shape take the first 5 slots, the Magnifier the last one, and slots 6 to 9 show the most recently
  used tools; dragging one item over another swaps them.
- Editor: besides tools, the toolbar accepts effects, outputs (file, copy all, email, print, external program),
  commands (undo, redo, new image, open, save, duplicate, delete, flatten, rotate, auto trim, Grab Text) and spacers,
  chosen in the *Customize toolbar* window (from "More").
- Editor: Effects panel on the right (tab next to Properties) with the quick effects gallery (Shadow, Border and
  shadow, Torn paper, Page curl, Wave, Sawtooth, Fade, Bevel and the ones you add) and the effects with a preview on
  the current image. Clicking an effect opens its settings in the panel itself, with a live preview on the image, and
  *Apply*/*Cancel*.
- Editor: new effects Perspective, Page curl and Spotlight; Bevel edge effect; Emboss, Mosaic, Sharpen and Blur
  filters; Color adjustment combines brightness, contrast, gamma, hue and saturation.
- Editor: recent captures tray (up to 10, configurable through `RecentTrayDocs`) with drag and drop: drag one or more
  captures to another program (images go as PNG with the annotations, videos as MP4) and drop files from Explorer to
  import them. The *Library* button shows all captures in a grid.
- Editor: library with multiple selection (Ctrl+click, Shift+click, Ctrl+A), export and delete of the selected items,
  an X on the thumbnail to delete, Del deletes the selected items and a highlight outline on hover.
- Editor: rectangle selection in the tray and the library, as in Explorer: hold the left button on empty space (in the
  tray, also on the title strip) and drag over the thumbnails; Ctrl or Shift add to the previous selection and, near
  the edge, the list scrolls by itself.
- Editor: the Highlighter draws a dragged rectangle in the chosen color and opacity (an editable highlight object).
- Editor: quick styles in every tool, as in the reference: a gallery with the theme's styles (Basic, Industrial or
  your own 4-color theme), "+" to save the current style, a star to mark favorites and the Favorites tool with the
  favorites of every tool. Themes: New theme…, Import…, edit, duplicate, export (.snaptheme) and delete.
- Editor: per-tool properties as in the reference — colors with a palette, shadow with direction (3×3 grid), color,
  opacity, blur and distance; arrowheads (8 kinds) and dash patterns; widths with a slider; Bézier curve; text with an
  outline, bold/italic/underline, horizontal and vertical alignment and margin; 24 callout shapes (speech, cloud,
  thought, box with arrow, arrows, note, explosion, whisper) with gradient; shapes (rectangle, rounded, ellipse,
  triangle, diamond, parallelogram, hexagon, star); steps as a circle, teardrop or square, numbered with numbers,
  letters or Roman numerals; round or square magnifier, with magnification and a lens separate from the area; eraser
  with opacity and smoothing; Fill with an eyedropper and tolerance; Smart Move in Move.
- Editor: color stamps in categories (Business, Interface, Activities, Food and drink, Holidays, Smileys and people,
  Stickers, Symbols, Travel and places) with search, and "My stamps" with imported images.
- Editor: *Customize toolbar* window as a grid (drawing tools, menu items, outputs, effects and spacer): drag an item
  onto the toolbar to add it; with the window open, drag an item off the toolbar to remove it. New on the toolbar:
  Favorites and Magic wand.
- Editor: Magnifier introduction on first use; footer with "Recent captures", image size and zoom menu.
- Editor: each tool's styles are remembered between sessions.
- Editor: Image menu as in the reference — Tools, Effects (with a sample of each effect), Arrange (bring forward
  Ctrl+Shift+F, bring to front, send backward Ctrl+Shift+B, send to back, Align, flip vertically and horizontally),
  Group/Ungroup, Flatten (Ctrl+T, only the selected object), Flatten all (Ctrl+Shift+T), Trim, Resize image/canvas,
  Apply template, Rotate (clockwise, counterclockwise, 180°, custom angle), Snap to canvas (Ctrl+;) and Tag image.
- Editor: Align and Flip act on the selected object (with no object, Flip flips the whole image); snap to canvas
  sticks the object's edges and center to the image's while moving (Alt turns it off on the fly).
- Editor: per-image tags (Image › Tag image…), stored in the capture and shown in its information.
- Editor: in the *Customize toolbar* window, choose how many tools the automatic toolbar shows (6 to 16).
- Editor: multiple object selection with Move — Ctrl/Shift+click adds to or removes from the selection, dragging on the
  background draws a selection rectangle and Ctrl+A selects all. The selected objects move, delete, change order,
  align with each other, flip and flatten together; a style change applies to those of the same kind.
- Editor: Group (Ctrl+Shift+G) and Ungroup (Ctrl+Shift+U): the group moves and resizes as a single object and is
  stored in the .snap as `<Group>` with the objects inside.
- "Skip" button on the countdown before recording (the record button and the hotkey also skip the countdown).
- One Click (*View › Show One Click*): a panel docked to the edge of the screen that collapses to a thin strip and
  opens with the mouse.
- "Panoramic scrolling capture" and "Capture text" presets at the top of the list.
- *Panoramic scrolling* selection: the user scrolls the content and the frames are stitched into a single image.
- *Text (OCR)* output under *Share*.
- Presets "+" menu: new preset from the current settings, new preset and help; a gear and "Add hotkey" on each row of
  the list.

### Fixed
- Editor: dragging captures from the tray and the library to other programs did not work. The drag now starts even
  with a quick movement, carries the thumbnail along with the cursor and also delivers the files as Shell items;
  Explorer copies them and apps like Paint (which only accept "move") open the image. Videos go with their library
  path and are never moved. Dropping an item back on the Editor itself no longer imports a copy of it.
- Editor: Text by click and drag: the box is dashed while dragging and while typing, keeps the dragged width and grows
  downward with the text (before, text beyond the dragged height was cut off and nothing showed while dragging); the
  text being edited sits in the same place it ends up in the image. The tool's cursor is the crosshair, like Selection's.
- Editor: the scroll bar of the Customize toolbar window (and of the properties panel) can now be dragged.
- Clicking the tray icon sometimes opened the window and sometimes captured: now a single click captures and a double
  click only opens the window.
- Videos whose width is not a multiple of 16 (e.g. 854 px) looked tilted and scrambled in the Editor.
- The Editor player did not play the video's audio; it now plays in sync with the image and skips the cut ranges.
- Microphone recorded too quietly: the recorder applies automatic gain (up to 8×) and a soft limiter on the mix.
- Installer: choosing *Program Files* (or another protected folder) in the "just for me" install failed with
  "access denied" in the middle of copying. The installer now warns when leaving the folder page and offers to
  restart as administrator, for all users, with the chosen folder and without asking for the license again.

### Changed
- Built-in preset, theme, quick effect and stamp names appear in the chosen language.
- The Editor got the same look as the capture window: custom title bar with the menus, colors from the Windows accent
  color, modern controls in the properties panel and a subdued status bar. The title bar, colors and controls are now
  shared by both programs (`src/common/CaptionBar`, `Theme`, `Widgets`).
- Narrower capture window, with a custom title bar (icon and menus on the same strip), a red capture button with no
  text below it, material-style tab icons and the Segoe UI Variable font.
- The settings gear sits inside the Selection, Share and Quality boxes.
- "Save to file" became the *File* output under *Share*; *Capture cursor* takes its place, as in the reference.
- Delay became a "Delay  Off ›" row with a menu; frames per second, downscaling to 1080p and hardware encoding moved
  to the Quality gear.
- Editor: *Image › Effects* opens the effect in the effects panel (with a live preview) instead of a dialog.
- Editor: a drawing tool only picks objects of its own kind; over the others it draws (so a text can be highlighted or
  pointed at without moving it). Ctrl still forces drawing over an object of the same kind.
- Editor: dragging an item on the toolbar moves it to the target position (before, it swapped the two).
- Editor: Grab Text (OCR) moved to Ctrl+Shift+O (Ctrl+Shift+T is now Flatten all, as in the reference);
  "Crop to selection" moved to the Edit menu.
- Editor: more compact toolbar; new Stamp, Steps, Cut out and Fill icons (a tilted bucket with a falling paint drop);
  "Step" became "Steps".
- Editor: the "More" button opens its own material-style list with each tool's icon, name and shortcut (before, it was
  a plain Windows menu with text only).
- Editor: with Move, dragging on the background now selects objects (scrolling the image still works with the middle
  button, the wheel and the scroll bars).
- Editor: effects renamed as in the reference menu: Borders, Capture info, Color replacement and Spotlight and
  magnify.

## [0.1.0] — 2026-10-24

First pre-release: the main features already work, but the program is still under development.

### Capture (Snapnab.exe)
- Region, window, full screen, monitor, fixed region, freehand, multiple areas, object, scrolling,
  extended window, menu, clipboard and file capture.
- MP4 video recording (H.264 + AAC) with microphone and system audio, pause, and a control panel outside the video.
- Presets with global hotkeys, effects and outputs (Editor, clipboard, file, external program).
- 12 capture effects: border, filters, shadow, capture info, color replacement, resize,
  edge effects, trim edges, color adjustment, rotate, watermark and resolution.
- Compact capture window, with modern controls and colors that follow the Windows accent color.

### Editor (SnapnabEditor.exe)
- Annotation tools (arrow, shapes, text, callouts, steps, stamps, blur, highlight, magnifier…),
  selection with Smart Move, magic wand and smart fill.
- Image effects, OCR (Grab Text), PNG/JPG/BMP/GIF/TIFF/PDF/SNAP export, printing and email.
- Video editing: cuts, save frame, animated GIF and MP4 export.
- Capture library with thumbnails.
