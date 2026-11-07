//! snapcore — Snapnab's shared Rust layer, used by both programs (Snapnab.exe and SnapnabEditor.exe) and by
//! SnapnabTool.exe (docs/ARCHITECTURE.md).
//!
//! - `container`: the compound file inside every .snap
//! - `log`: %LOCALAPPDATA%\Snapnab\Logs\<program>.log
//! - `reg`: the settings in HKCU\Software\Snapnab\1
//! - `i18n`: the interface language and the lang\<code>.po catalogs
//! - `xml`: the XML reader and writer (presets, .snap documents, Names.xml)
//! - `image`, `codec`: images in memory and image files through the Windows codecs (WIC)
//! - `snap`: the .snap document (index, frame documents, base and embedded images)
//! - `library`: the capture library (its folder, thumbnails, capture names, cleanup)
//! - `gif`: the animated GIF writer
//! - `crt`: printf-style number formatting through the C runtime (the same rounding the C++ had)
//! - `gdip`: the GDI+ flat API (scaling, text, polygons) and image scaling
//! - `fx`: the image effects and the capture preset effects
//! - `beautify`: Beautify (background, padding, rounded corners, shadow)
//! - `cv`: computer vision (magic wand, masks, inpainting, translation detection)
//! - `pdf`: the PDF writer and the page breaks of long captures
//! - `color`: color values as text and the recent colors
//! - `hdr`: HDR screenshots and recordings (DXGI duplication, tone mapping)
//! - `ocr`: text recognition and face detection (Windows OCR and FaceAnalysis)
//! - `barcode`: QR codes and barcodes (rxing)
//! - `net`: HTTPS (WinHTTP), SHA-256/HMAC, JSON and the secrets in the Credential Manager
//! - `upload`: the upload destinations, their settings, the sign-in and the custom uploader syntax
//! - `ai`: the AI assistant's request
//! - `doc`, `render`, `stamps`: the editor's objects, their painters and the stamp images (emoji, labels, cursors)
//! - `serial`: the editor's objects as XML, in the .snap and on the clipboard
//! - `presets`: the capture presets (Presets1.xml)
//! - `ipc`, `instance`: the channel between the two programs, one instance per program, forwarded command lines
//! - `screen`, `clip`, `sounds`: screen copies, the cursor and the windows; the clipboard; the capture sounds

pub mod ai;
pub mod barcode;
pub mod beautify;
pub mod clip;
pub mod codec;
pub mod color;
pub mod container;
pub mod crt;
pub mod cv;
pub mod doc;
pub mod dragout;
pub mod fx;
pub mod gdip;
pub mod gif;
pub mod hdr;
pub mod i18n;
pub mod image;
pub mod instance;
pub mod ipc;
pub mod library;
pub mod links;
pub mod log;
pub mod net;
pub mod ocr;
pub mod office;
pub mod paths;
pub mod pdf;
pub mod presets;
pub mod panorama;
pub mod recorder;
pub mod webcam;
pub mod reg;
pub mod render;
pub mod screen;
pub mod serial;
pub mod settings;
pub mod snap;
pub mod sounds;
pub mod stamps;
pub mod upload;
pub mod video;
pub mod wide;
pub mod xml;

