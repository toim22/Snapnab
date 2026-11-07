//! Image files through the Windows Imaging Component (role of LEADTOOLS, ds_arch §13.3 → WIC): PNG, JPEG, BMP, GIF,
//! TIFF, and AVIF / HEIC when the Windows codecs are installed. A port of Image.cpp that makes the same WIC calls in the
//! same order, so the files come out byte for byte the same: decoding to 32 bpp BGRA with the photo's metadata (C29) and
//! its EXIF orientation applied; encoding with the resolution, Snapnab's metadata (unless stripped) and JPEG / BMP
//! composed over white. WebP is read and written with libwebp 1.5.0 (libwebp-sys), as Image.cpp did, whether or not the
//! Windows WebP codec is installed.

use std::io::Read;
use std::os::windows::fs::OpenOptionsExt;
use std::path::Path;

use libwebp_sys as webp;
use windows::Win32::Foundation::{FILETIME, GENERIC_READ, HGLOBAL, SYSTEMTIME, WINCODEC_ERR_COMPONENTNOTFOUND};
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE, FileTimeToLocalFileTime};
use windows::Win32::System::Com::StructuredStorage::{
    CreateStreamOnHGlobal, IPropertyBag2, PROPBAG2, PROPVARIANT, PropVariantClear,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemAlloc, CoUninitialize, IStream,
    STATFLAG_NONAME, STATSTG, STREAM_SEEK_SET,
};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::System::Time::FileTimeToSystemTime;
use windows::Win32::System::Variant::{
    VARENUM, VARIANT, VT_BSTR, VT_FILETIME, VT_LPSTR, VT_LPWSTR, VT_R4, VT_R8, VT_UI1, VT_UI2, VT_UI4, VT_UI8, VT_VECTOR,
};
use windows::Win32::UI::Shell::SHCreateMemStream;
use windows::core::{GUID, HSTRING, PCWSTR, PSTR, PWSTR, w};

use crate::image::{Image, Meta};
use crate::reg;

/// The formats of ImageFormat in Image.h, in the same order (the values cross the C ABI).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum Format {
    Png,
    Jpeg,
    Bmp,
    Gif,
    Tiff,
    Webp,
    Avif,
    Heic,
    Unknown,
}

impl Format {
    /// FormatFromExt: by the file extension (".png", ".JPG"…).
    pub fn from_ext(ext: &str) -> Format {
        match ext.to_lowercase().as_str() {
            ".png" => Format::Png,
            ".jpg" | ".jpeg" | ".jpe" => Format::Jpeg,
            ".bmp" => Format::Bmp,
            ".gif" => Format::Gif,
            ".tif" | ".tiff" => Format::Tiff,
            ".webp" => Format::Webp,
            ".avif" => Format::Avif,
            ".heic" | ".heif" | ".hif" => Format::Heic,
            _ => Format::Unknown,
        }
    }

    pub fn from_i32(v: i32) -> Format {
        [Format::Png, Format::Jpeg, Format::Bmp, Format::Gif, Format::Tiff, Format::Webp, Format::Avif, Format::Heic]
            .get(v as usize)
            .copied()
            .unwrap_or(Format::Unknown)
    }
}

/// How a file is written (SaveOptions in Image.h).
#[derive(Clone, Copy, Debug)]
pub struct SaveOptions {
    pub jpeg_quality: i32,
    pub avif_quality: i32,
    pub webp_lossless: bool,
    pub webp_quality: i32,
    pub strip_metadata: bool,
}

impl Default for SaveOptions {
    fn default() -> Self {
        SaveOptions { jpeg_quality: 90, avif_quality: 80, webp_lossless: true, webp_quality: 90, strip_metadata: false }
    }
}

impl SaveOptions {
    /// SaveOptions::FromSettings: the user's choices in the registry (the JPEG quality is not one of them).
    pub fn from_settings() -> Self {
        let dword = |name: &str, def: u32| reg::get_dword(std::ffi::OsStr::new(name), def, None);
        SaveOptions {
            webp_lossless: dword("WebPLossless", 1) != 0,
            webp_quality: dword("WebPQuality", 90).clamp(1, 100) as i32,
            avif_quality: dword("AvifQuality", 80).clamp(1, 100) as i32,
            strip_metadata: dword("StripMetadata", 0) != 0,
            ..Default::default()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    Failed,
    /// Windows has no decoder for the file (the C++ side then says "unsupported format")
    NoDecoder,
    /// libwebp does not take the file as WebP ("Invalid WebP file")
    WebpInvalid,
    /// libwebp failed while decoding it ("Invalid WebP file", and the C++ side clears the image as before)
    WebpCorrupt,
    /// the WebP file could not be read whole (no message)
    WebpUnreadable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodeError {
    Failed,
    /// AVIF could not be written: the codec hint applies (AV1 / HEIF extensions)
    Codec,
    /// libwebp could not encode the image ("WebP encoding failed")
    Webp,
}

// ---------------------------------------------------------------- COM

/// COM for the length of a call, as ComScope in Image.cpp (released last: declare it first).
struct ComScope(bool);

impl ComScope {
    fn new() -> Self {
        ComScope(unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok())
    }
}

impl Drop for ComScope {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

fn factory() -> Option<IWICImagingFactory> {
    unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }.ok()
}

// ---------------------------------------------------------------- property values

fn vt(v: &PROPVARIANT) -> VARENUM {
    unsafe { v.Anonymous.Anonymous.vt }
}

/// A string value (UTF-16, UTF-8 or BSTR), or "".
fn prop_string(v: &PROPVARIANT) -> String {
    unsafe {
        let a = &v.Anonymous.Anonymous.Anonymous;
        match vt(v) {
            VT_LPWSTR if !a.pwszVal.is_null() => String::from_utf16_lossy(a.pwszVal.as_wide()),
            VT_LPSTR if !a.pszVal.is_null() => String::from_utf8_lossy(a.pszVal.as_bytes()).into_owned(),
            VT_BSTR => a.bstrVal.to_string(),
            _ => String::new(),
        }
    }
}

/// Degrees from the EXIF rational triple (degrees, minutes, seconds), or from the doubles the photo policy gives.
fn prop_degrees(v: &PROPVARIANT) -> Option<f64> {
    // EXIF rationals as ULARGE_INTEGER: numerator in the low half, denominator in the high half
    let rational = |x: u64| {
        let (num, den) = (x as u32, (x >> 32) as u32);
        if den != 0 { num as f64 / den as f64 } else { 0.0 }
    };
    let part: [f64; 3] = unsafe {
        let a = &v.Anonymous.Anonymous.Anonymous;
        if vt(v).0 == VT_VECTOR.0 | VT_UI8.0 && a.cauh.cElems >= 3 {
            let e = std::slice::from_raw_parts(a.cauh.pElems, 3);
            [rational(e[0]), rational(e[1]), rational(e[2])]
        } else if vt(v).0 == VT_VECTOR.0 | VT_R8.0 && a.cadbl.cElems >= 3 {
            let e = std::slice::from_raw_parts(a.cadbl.pElems, 3);
            [e[0], e[1], e[2]]
        } else {
            return None;
        }
    };
    Some(part[0] + part[1] / 60.0 + part[2] / 3600.0)
}

/// A metadata value by its query name; cleared on drop.
struct Prop(PROPVARIANT);

impl Drop for Prop {
    fn drop(&mut self) {
        unsafe {
            let _ = PropVariantClear(&mut self.0);
        }
    }
}

fn get(q: &IWICMetadataQueryReader, name: &str) -> Option<Prop> {
    let mut p = Prop(PROPVARIANT::default());
    unsafe { q.GetMetadataByName(&HSTRING::from(name), &mut p.0) }.ok().map(|_| p)
}

fn exif_time(t: &SYSTEMTIME) -> String {
    format!("{:04}:{:02}:{:02} {:02}:{:02}:{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}

// ---------------------------------------------------------------- decoding

fn read_meta(frame: &IWICBitmapFrameDecode) -> Option<Meta> {
    let q = unsafe { frame.GetMetadataQueryReader() }.ok()?;
    let mut m = Meta::default();
    if let Some(v) = get(&q, "System.Photo.DateTaken") {
        if vt(&v.0) == VT_FILETIME {
            let ft = unsafe { v.0.Anonymous.Anonymous.Anonymous.filetime };
            let (mut local, mut st) = (FILETIME::default(), SYSTEMTIME::default());
            unsafe {
                let _ = FileTimeToLocalFileTime(&ft, &mut local); // EXIF dates are local time
                let _ = FileTimeToSystemTime(&local, &mut st);
            }
            m.date_taken = exif_time(&st);
        } else {
            m.date_taken = prop_string(&v.0);
        }
    }
    if let Some(v) = get(&q, "System.Photo.CameraManufacturer") {
        m.make = prop_string(&v.0);
    }
    if let Some(v) = get(&q, "System.Photo.CameraModel") {
        m.model = prop_string(&v.0);
    }
    if let Some(v) = get(&q, "System.ApplicationName") {
        m.software = prop_string(&v.0);
    }
    // GPS: the photo policy, or the EXIF GPS directory of JPEG / TIFF
    for base in ["System.GPS.", "/app1/ifd/gps/", "/ifd/gps/"] {
        let policy = base.starts_with('S');
        let name = |pol: &str, tag: u16| if policy { format!("{base}{pol}") } else { format!("{base}{{ushort={tag}}}") };
        let lat = get(&q, &name("Latitude", 2)).and_then(|v| prop_degrees(&v.0));
        let lon = get(&q, &name("Longitude", 4)).and_then(|v| prop_degrees(&v.0));
        let (Some(mut lat), Some(mut lon)) = (lat, lon) else { continue };
        if get(&q, &name("LatitudeRef", 1)).is_some_and(|v| prop_string(&v.0).starts_with('S')) {
            lat = -lat;
        }
        if get(&q, &name("LongitudeRef", 3)).is_some_and(|v| prop_string(&v.0).starts_with('W')) {
            lon = -lon;
        }
        m.gps = true;
        m.lat = lat;
        m.lon = lon;
        break;
    }
    (!m.is_empty()).then_some(m)
}

/// Photos from phones and cameras are stored sideways with an EXIF orientation: the image is turned upright.
fn apply_orientation(frame: &IWICBitmapFrameDecode, img: &mut Image) {
    let Ok(q) = (unsafe { frame.GetMetadataQueryReader() }) else { return };
    let Some(v) = get(&q, "System.Photo.Orientation") else { return };
    let o = unsafe {
        match vt(&v.0) {
            VT_UI2 => v.0.Anonymous.Anonymous.Anonymous.uiVal as u32,
            VT_UI4 => v.0.Anonymous.Anonymous.Anonymous.ulVal,
            _ => 1,
        }
    };
    if !(2..=8).contains(&o) {
        return;
    }
    let (w, h) = (img.w as usize, img.h as usize);
    let swap = o >= 5;
    let (rw, rh) = if swap { (h, w) } else { (w, h) };
    let mut px = vec![0u32; rw * rh];
    for y in 0..h {
        for x in 0..w {
            let (nx, ny) = match o {
                2 => (w - 1 - x, y),         // mirrored
                3 => (w - 1 - x, h - 1 - y), // 180°
                4 => (x, h - 1 - y),
                5 => (y, x),
                6 => (h - 1 - y, x), // 90° clockwise
                7 => (h - 1 - y, w - 1 - x),
                _ => (y, w - 1 - x), // 8: 90° counterclockwise
            };
            px[ny * rw + nx] = img.px[y * w + x];
        }
    }
    img.w = rw as u32;
    img.h = rh as u32;
    img.px = px;
}

fn decode_with(f: &IWICImagingFactory, dec: &IWICBitmapDecoder) -> Option<Image> {
    unsafe {
        let frame = dec.GetFrame(0).ok()?;
        let conv = f.CreateFormatConverter().ok()?;
        conv.Initialize(&frame, &GUID_WICPixelFormat32bppBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)
            .ok()?;
        let (mut w, mut h) = (0u32, 0u32);
        conv.GetSize(&mut w, &mut h).ok()?;
        let mut img = Image::new(w, h, 0);
        conv.CopyPixels(std::ptr::null(), w * 4, img.bytes_mut()).ok()?;
        img.meta = read_meta(&frame);
        apply_orientation(&frame, &mut img);
        Some(img)
    }
}

// ---------------------------------------------------------------- WebP (libwebp)

/// RIFF....WEBP
pub fn is_webp(d: &[u8]) -> bool {
    d.len() >= 12 && &d[..4] == b"RIFF" && &d[8..12] == b"WEBP"
}

fn decode_webp(d: &[u8]) -> Result<Image, DecodeError> {
    let (mut w, mut h) = (0, 0);
    if unsafe { webp::WebPGetInfo(d.as_ptr(), d.len(), &mut w, &mut h) } == 0 || w <= 0 || h <= 0 {
        return Err(DecodeError::WebpInvalid);
    }
    let mut img = Image::new(w as u32, h as u32, 0);
    let out = img.bytes_mut();
    if unsafe { webp::WebPDecodeBGRAInto(d.as_ptr(), d.len(), out.as_mut_ptr(), out.len(), w * 4) }.is_null() {
        return Err(DecodeError::WebpCorrupt);
    }
    Ok(img)
}

fn encode_webp(img: &Image, opt: &SaveOptions) -> Result<Vec<u8>, EncodeError> {
    let (w, h) = (img.w as i32, img.h as i32);
    let px = img.bytes().as_ptr();
    let mut data: *mut u8 = std::ptr::null_mut();
    let n = unsafe {
        if opt.webp_lossless {
            webp::WebPEncodeLosslessBGRA(px, w, h, w * 4, &mut data)
        } else {
            webp::WebPEncodeBGRA(px, w, h, w * 4, opt.webp_quality as f32, &mut data)
        }
    };
    let out = (n != 0 && !data.is_null()).then(|| unsafe { std::slice::from_raw_parts(data, n) }.to_vec());
    if !data.is_null() {
        unsafe { webp::WebPFree(data as *mut _) };
    }
    out.ok_or(EncodeError::Webp)
}

/// Opened as Image.cpp did: shared for reading and writing.
fn open_shared(path: &Path) -> Option<std::fs::File> {
    std::fs::OpenOptions::new().read(true).share_mode((FILE_SHARE_READ | FILE_SHARE_WRITE).0).open(path).ok()
}

/// The first 12 bytes say WebP.
fn file_is_webp(path: &Path) -> bool {
    let mut head = [0u8; 12];
    open_shared(path).and_then(|mut f| f.read(&mut head).ok()) == Some(12) && is_webp(&head)
}

/// The whole file, if it has 1 byte to 512 MB and reads whole; empty otherwise.
fn read_whole(path: &Path) -> Vec<u8> {
    let Some(mut f) = open_shared(path) else { return Vec::new() };
    let size = f.metadata().map(|m| m.len()).unwrap_or(0);
    if size == 0 || size >= 512 * 1024 * 1024 {
        return Vec::new();
    }
    let mut d = vec![0u8; size as usize];
    if f.read_exact(&mut d).is_err() {
        d.clear();
    }
    d
}

// ---------------------------------------------------------------- decoding

/// Decodes an image file (WebP with libwebp, the Windows WebP codec being optional).
pub fn load(path: &Path) -> Result<Image, DecodeError> {
    if file_is_webp(path) {
        let d = read_whole(path);
        return if d.is_empty() { Err(DecodeError::WebpUnreadable) } else { decode_webp(&d) };
    }
    let _com = ComScope::new();
    let f = factory().ok_or(DecodeError::Failed)?;
    let dec = unsafe {
        f.CreateDecoderFromFilename(&HSTRING::from(path), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand)
    };
    match dec {
        Ok(dec) => decode_with(&f, &dec).ok_or(DecodeError::Failed),
        Err(e) if e.code() == WINCODEC_ERR_COMPONENTNOTFOUND => Err(DecodeError::NoDecoder),
        Err(_) => Err(DecodeError::Failed),
    }
}

/// Decodes an image held in memory.
pub fn decode(data: &[u8]) -> Result<Image, DecodeError> {
    if is_webp(data) {
        return decode_webp(data);
    }
    let _com = ComScope::new();
    let f = factory().ok_or(DecodeError::Failed)?;
    let stream = unsafe { SHCreateMemStream(Some(data)) }.ok_or(DecodeError::Failed)?;
    let dec = unsafe { f.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand) }
        .map_err(|_| DecodeError::Failed)?;
    decode_with(&f, &dec).ok_or(DecodeError::Failed)
}

// ---------------------------------------------------------------- encoding

/// Metadata written by Snapnab (unless stripped): the program, the date and what the source photo had.
fn write_meta(frame: &IWICBitmapFrameEncode, fmt: Format, img: &Image, version: &str) {
    let Ok(q) = (unsafe { frame.GetMetadataQueryWriter() }) else { return };
    // the values are allocated with the COM allocator and cleared after the call, as COM wants (WIC copies them)
    let put_str = |name: &str, s: &str| {
        if s.is_empty() {
            return;
        }
        let mut v = Prop(PROPVARIANT::default());
        unsafe {
            let p = CoTaskMemAlloc(s.len() + 1) as *mut u8;
            if p.is_null() {
                return;
            }
            std::ptr::copy_nonoverlapping(s.as_ptr(), p, s.len());
            *p.add(s.len()) = 0;
            (*v.0.Anonymous.Anonymous).vt = VT_LPSTR;
            (*v.0.Anonymous.Anonymous).Anonymous.pszVal = PSTR(p);
            let _ = q.SetMetadataByName(&HSTRING::from(name), &v.0);
        }
    };
    let now = exif_time(&unsafe { GetLocalTime() });
    let software = format!("Snapnab {version}");
    let m = img.meta.as_ref();
    let date = m.map(|m| m.date_taken.as_str()).filter(|d| !d.is_empty()).unwrap_or(&now);
    if fmt == Format::Png {
        put_str("/tEXt/{str=Software}", &software);
        put_str("/tEXt/{str=Creation Time}", date);
        return;
    }
    if fmt != Format::Jpeg && fmt != Format::Tiff {
        return;
    }
    let ifd = if fmt == Format::Jpeg { "/app1/ifd" } else { "/ifd" };
    put_str(&format!("{ifd}/{{ushort=305}}"), &software);
    put_str(&format!("{ifd}/{{ushort=306}}"), &now);
    put_str(&format!("{ifd}/exif/{{ushort=36867}}"), date);
    let Some(m) = m else { return };
    put_str(&format!("{ifd}/{{ushort=271}}"), &m.make);
    put_str(&format!("{ifd}/{{ushort=272}}"), &m.model);
    if m.gps {
        let put_deg = |tag: &str, deg: f64| {
            let deg = deg.abs();
            let mins = (deg - deg.floor()) * 60.0;
            let rational = |num: u32, den: u32| (den as u64) << 32 | num as u64;
            let r = [
                rational(deg.floor() as u32, 1),
                rational(mins.floor() as u32, 1),
                rational(((mins - mins.floor()) * 60.0 * 1000.0).round() as i64 as u32, 1000),
            ];
            let mut v = Prop(PROPVARIANT::default());
            unsafe {
                let p = CoTaskMemAlloc(std::mem::size_of_val(&r)) as *mut u64;
                if p.is_null() {
                    return;
                }
                std::ptr::copy_nonoverlapping(r.as_ptr(), p, 3);
                (*v.0.Anonymous.Anonymous).vt = VARENUM(VT_VECTOR.0 | VT_UI8.0);
                (*v.0.Anonymous.Anonymous).Anonymous.cauh.cElems = 3;
                (*v.0.Anonymous.Anonymous).Anonymous.cauh.pElems = p;
                let _ = q.SetMetadataByName(&HSTRING::from(format!("{ifd}/gps/{tag}")), &v.0);
            }
        };
        put_str(&format!("{ifd}/gps/{{ushort=1}}"), if m.lat < 0.0 { "S" } else { "N" });
        put_deg("{ushort=2}", m.lat);
        put_str(&format!("{ifd}/gps/{{ushort=3}}"), if m.lon < 0.0 { "W" } else { "E" });
        put_deg("{ushort=4}", m.lon);
    }
}

fn write_prop(props: &IPropertyBag2, name: PCWSTR, vt: VARENUM, set: impl FnOnce(&mut VARIANT)) -> bool {
    let bag = PROPBAG2 { pstrName: PWSTR(name.0 as *mut u16), ..Default::default() };
    let mut v = VARIANT::default();
    unsafe { (*v.Anonymous.Anonymous).vt = vt };
    set(&mut v);
    unsafe { props.Write(1, &bag, &v) }.is_ok()
}

fn container(fmt: Format) -> Option<GUID> {
    Some(match fmt {
        Format::Png => GUID_ContainerFormatPng,
        Format::Jpeg => GUID_ContainerFormatJpeg,
        Format::Bmp => GUID_ContainerFormatBmp,
        Format::Gif => GUID_ContainerFormatGif,
        Format::Tiff => GUID_ContainerFormatTiff,
        Format::Avif => GUID_ContainerFormatHeif,
        _ => return None,
    })
}

fn encode_into(
    f: &IWICImagingFactory, stream: &IStream, img_in: &Image, fmt: Format, opt: &SaveOptions, version: &str,
) -> Result<(), EncodeError> {
    let container = container(fmt).ok_or(EncodeError::Failed)?;
    // JPEG and BMP have no alpha: composed over white
    let flat;
    let img = if (fmt == Format::Jpeg || fmt == Format::Bmp) && img_in.has_transparency() {
        flat = img_in.flattened(0xFFFFFFFF);
        &flat
    } else {
        img_in
    };
    let avif = fmt == Format::Avif;
    let codec = if avif { EncodeError::Codec } else { EncodeError::Failed };
    unsafe {
        let enc = f.CreateEncoder(&container, std::ptr::null()).map_err(|_| codec)?;
        enc.Initialize(stream, WICBitmapEncoderNoCache).map_err(|_| EncodeError::Failed)?;
        let (mut frame, mut props) = (None, None);
        enc.CreateNewFrame(&mut frame, &mut props).map_err(|_| EncodeError::Failed)?;
        let frame = frame.ok_or(EncodeError::Failed)?;
        if let Some(props) = props.as_ref() {
            if fmt == Format::Jpeg || avif {
                let q = if avif { opt.avif_quality } else { opt.jpeg_quality }.clamp(1, 100) as f32 / 100.0;
                write_prop(props, w!("ImageQuality"), VT_R4, |v| (*v.Anonymous.Anonymous).Anonymous.fltVal = q);
            }
            if avif && !write_prop(props, w!("HeifCompressionMethod"), VT_UI1, |v| {
                (*v.Anonymous.Anonymous).Anonymous.bVal = 3; // WICHeifCompressionAV1
            }) {
                return Err(codec);
            }
        }
        frame.Initialize(props.as_ref()).map_err(|_| codec)?;
        let _ = frame.SetSize(img.w, img.h);
        if img_in.dpi > 0.0 {
            let _ = frame.SetResolution(img_in.dpi as f64, img_in.dpi as f64);
        }
        if !opt.strip_metadata {
            write_meta(&frame, fmt, img_in, version);
        }
        let mut pf = GUID_WICPixelFormat32bppBGRA;
        let _ = frame.SetPixelFormat(&mut pf);
        let written = if pf == GUID_WICPixelFormat32bppBGRA {
            frame.WritePixels(img.h, img.w * 4, img.bytes())
        } else {
            // the encoder asked for another format (JPEG: 24 bpp; GIF: indexed) → WIC converter
            (|| {
                let src = f.CreateBitmapFromMemory(img.w, img.h, &GUID_WICPixelFormat32bppBGRA, img.w * 4, img.bytes())?;
                let conv = f.CreateFormatConverter()?;
                let pal = if fmt == Format::Gif {
                    let pal = f.CreatePalette()?;
                    let _ = pal.InitializeFromBitmap(&src, 256, img.has_transparency());
                    Some(pal)
                } else {
                    None
                };
                let (dither, translate) = if pal.is_some() {
                    (WICBitmapDitherTypeErrorDiffusion, WICBitmapPaletteTypeCustom)
                } else {
                    (WICBitmapDitherTypeNone, WICBitmapPaletteTypeMedianCut)
                };
                conv.Initialize(&src, &pf, dither, pal.as_ref(), 0.0, translate)?;
                if let Some(pal) = pal.as_ref() {
                    let _ = frame.SetPalette(pal);
                }
                frame.WriteSource(&conv, std::ptr::null())
            })()
        };
        written.map_err(|_| codec)?;
        frame.Commit().map_err(|_| codec)?;
        enc.Commit().map_err(|_| codec)?;
    }
    Ok(())
}

/// Encodes the image as `fmt`. `version` goes into the metadata ("Snapnab <version>"; WebP gets none).
pub fn encode(img: &Image, fmt: Format, opt: &SaveOptions, version: &str) -> Result<Vec<u8>, EncodeError> {
    if img.is_empty() || img.px.len() != img.w as usize * img.h as usize {
        return Err(EncodeError::Failed);
    }
    if fmt == Format::Webp {
        return encode_webp(img, opt);
    }
    let _com = ComScope::new();
    let f = factory().ok_or(EncodeError::Failed)?;
    let stream = unsafe { CreateStreamOnHGlobal(HGLOBAL::default(), true) }.map_err(|_| EncodeError::Failed)?;
    encode_into(&f, &stream, img, fmt, opt, version)?;
    unsafe {
        let mut st = STATSTG::default();
        stream.Stat(&mut st, STATFLAG_NONAME).map_err(|_| EncodeError::Failed)?;
        let mut out = vec![0u8; st.cbSize as usize];
        stream.Seek(0, STREAM_SEEK_SET, None).map_err(|_| EncodeError::Failed)?;
        let mut read = 0u32;
        stream.Read(out.as_mut_ptr() as *mut _, out.len() as u32, Some(&mut read)).ok().map_err(|_| EncodeError::Failed)?;
        if read as usize != out.len() {
            return Err(EncodeError::Failed);
        }
        Ok(out)
    }
}

/// Writes the image to a file (SaveImageFileEx): unknown formats and HEIC (only opened) as PNG, written whole
/// to `<path>.tmp` and then put in place.
pub fn save_file(path: &Path, img: &Image, fmt: Format, opt: &SaveOptions, version: &str) -> bool {
    save_file_why(path, img, fmt, opt, version).is_ok()
}

/// `save_file` with the reason it failed, as ImageIoError gave it ("" when there is nothing more to say).
pub fn save_file_why(path: &Path, img: &Image, fmt: Format, opt: &SaveOptions, version: &str) -> Result<(), String> {
    let fmt = if matches!(fmt, Format::Unknown | Format::Heic) { Format::Png } else { fmt };
    let data = encode(img, fmt, opt, version).map_err(|e| e.text(fmt))?;
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    if std::fs::write(&tmp, &data).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return Err(String::new());
    }
    std::fs::rename(&tmp, path).map_err(|_| String::new())
}

/// CanEncode: AVIF needs an AV1 encoder (tried once on a tiny image); HEIC is only read.
pub fn can_encode(fmt: Format) -> bool {
    static AVIF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    match fmt {
        Format::Avif => *AVIF.get_or_init(|| {
            let o = SaveOptions { strip_metadata: true, ..Default::default() };
            encode(&Image::new(16, 16, 0xFF808080), Format::Avif, &o, "").is_ok_and(|d| !d.is_empty())
        }),
        Format::Heic | Format::Unknown => false,
        _ => true,
    }
}

/// CodecHint: what to install for HEIC and AVIF ("" for the other formats).
pub fn codec_hint(fmt: Format) -> String {
    match fmt {
        Format::Heic => crate::tr!(
            "Para abrir fotos HEIC, instale da Microsoft Store as extensões \"Extensões de Imagem HEIF\" e \"Extensões de Vídeo HEVC\"."
        ),
        Format::Avif => crate::tr!(
            "Para usar AVIF, instale da Microsoft Store a \"Extensão de Vídeo AV1\" e as \"Extensões de Imagem HEIF\". \
             Salvar em AVIF também precisa de um codificador AV1 (placas de vídeo recentes)."
        ),
        _ => String::new(),
    }
}

/// The message when AVIF cannot be written here (before asking for the quality).
pub fn avif_unavailable_text() -> String {
    crate::tr!("Este computador não consegue salvar em AVIF.\n\n") + &codec_hint(Format::Avif)
}

impl DecodeError {
    /// What ImageIoError said after a failed read of a file of format `fmt`.
    pub fn text(self, fmt: Format) -> String {
        match self {
            DecodeError::WebpInvalid | DecodeError::WebpCorrupt => crate::tr!("Arquivo WebP inválido."),
            DecodeError::WebpUnreadable => String::new(),
            DecodeError::NoDecoder | DecodeError::Failed => {
                let hint = codec_hint(fmt);
                if hint.is_empty() && self == DecodeError::NoDecoder { crate::tr!("Formato de imagem não suportado.") } else { hint }
            }
        }
    }
}

impl EncodeError {
    /// What ImageIoError said after a failed write as `fmt`.
    pub fn text(self, fmt: Format) -> String {
        match self {
            EncodeError::Webp => crate::tr!("Falha ao codificar WebP."),
            EncodeError::Codec => codec_hint(fmt),
            EncodeError::Failed => String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pattern(w: u32, h: u32, alpha: bool) -> Image {
        let mut img = Image::new(w, h, 0);
        for y in 0..h {
            for x in 0..w {
                let a = if alpha { (x * 255 / w.max(1)) | 1 } else { 255 };
                img.px[(y * w + x) as usize] = a << 24 | (x * 7 % 256) << 16 | (y * 13 % 256) << 8 | (x ^ y) % 256;
            }
        }
        img
    }

    #[test]
    fn lossless_formats_round_trip() {
        let img = pattern(37, 21, true);
        let opaque = pattern(37, 21, false);
        let strip = SaveOptions { strip_metadata: true, ..Default::default() };
        for (fmt, src) in [(Format::Png, &img), (Format::Tiff, &img), (Format::Bmp, &opaque)] {
            let data = encode(src, fmt, &strip, "test").unwrap();
            let back = decode(&data).unwrap();
            assert_eq!((back.w, back.h), (37, 21), "{fmt:?}");
            assert_eq!(back.px, src.px, "{fmt:?}");
            assert_eq!(back.meta, None, "{fmt:?}");
        }
        // BMP of an image with transparency: composed over white
        let back = decode(&encode(&img, Format::Bmp, &strip, "test").unwrap()).unwrap();
        assert_eq!(back.px, img.flattened(0xFFFFFFFF).px);
    }

    #[test]
    fn jpeg_and_gif() {
        let img = pattern(64, 48, false);
        let jpeg = encode(&img, Format::Jpeg, &SaveOptions::default(), "test").unwrap();
        assert_eq!(&jpeg[..2], &[0xFF, 0xD8]);
        let back = decode(&jpeg).unwrap();
        assert_eq!((back.w, back.h), (64, 48));
        let gif = encode(&img, Format::Gif, &SaveOptions::default(), "test").unwrap();
        assert_eq!(&gif[..4], b"GIF8");
        assert_eq!(decode(&gif).unwrap().w, 64);
    }

    #[test]
    fn metadata_is_written_and_read() {
        let mut img = pattern(16, 16, false);
        img.meta = Some(Meta {
            date_taken: "2024:05:06 07:08:09".into(),
            make: "Maker".into(),
            model: "Model X".into(),
            software: String::new(),
            gps: true,
            lat: -23.5505,
            lon: -46.6333,
        });
        let jpeg = encode(&img, Format::Jpeg, &SaveOptions::default(), "1.2.3").unwrap();
        let m = decode(&jpeg).unwrap().meta.unwrap();
        assert_eq!(m.date_taken, "2024:05:06 07:08:09");
        assert_eq!((m.make.as_str(), m.model.as_str(), m.software.as_str()), ("Maker", "Model X", "Snapnab 1.2.3"));
        assert!(m.gps && (m.lat + 23.5505).abs() < 1e-4 && (m.lon + 46.6333).abs() < 1e-4);
        let png = encode(&img, Format::Png, &SaveOptions::default(), "1.2.3").unwrap();
        // WIC keeps one tEXt chunk: the second query (Creation Time) replaces the first (Software), as in the C++
        let text = b"tEXtCreation Time\x002024:05:06 07:08:09";
        assert!(png.windows(text.len()).any(|w| w == text));
        assert!(!png.windows(8).any(|w| w == b"Software"));
        let stripped = encode(&img, Format::Jpeg, &SaveOptions { strip_metadata: true, ..Default::default() }, "x").unwrap();
        assert_eq!(decode(&stripped).unwrap().meta, None);
    }

    #[test]
    fn resolution_and_failures() {
        let mut img = pattern(8, 8, false);
        img.dpi = 300.0;
        let png = encode(&img, Format::Png, &SaveOptions::default(), "t").unwrap();
        assert!(png.windows(4).any(|w| w == b"pHYs"));
        assert_eq!(decode(b"not an image"), Err(DecodeError::Failed));
        assert_eq!(encode(&Image::default(), Format::Png, &SaveOptions::default(), "t"), Err(EncodeError::Failed));
        let p = std::env::temp_dir().join(format!("snapcore-codec-{}.xyz", std::process::id()));
        std::fs::write(&p, b"plain text, no image format").unwrap();
        assert_eq!(load(&p), Err(DecodeError::NoDecoder));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn webp_round_trip() {
        let img = pattern(37, 21, true);
        let data = encode(&img, Format::Webp, &SaveOptions::default(), "t").unwrap();
        assert!(is_webp(&data));
        let back = decode(&data).unwrap();
        assert_eq!((back.w, back.h, back.meta.is_none()), (37, 21, true));
        assert_eq!(back.px, img.px, "lossless");
        let lossy = SaveOptions { webp_lossless: false, webp_quality: 40, ..Default::default() };
        let small = encode(&pattern(160, 120, false), Format::Webp, &lossy, "t").unwrap();
        assert_eq!(decode(&small).map(|i| (i.w, i.h)), Ok((160, 120)));
        // from a file, and the failures
        let p = std::env::temp_dir().join(format!("snapcore-codec-{}.webp", std::process::id()));
        assert!(save_file(&p, &img, Format::Webp, &SaveOptions::default(), "t"));
        assert_eq!(load(&p).unwrap().px, img.px);
        let mut bad = data.clone();
        bad.truncate(40);
        assert_eq!(decode(&bad).map(|_| ()), Err(DecodeError::WebpCorrupt));
        assert_eq!(decode(b"RIFF\0\0\0\0WEBPVP8 nothing").map(|_| ()), Err(DecodeError::WebpInvalid));
        std::fs::write(&p, b"RIFF\0\0\0\0WEBPVP8 nothing").unwrap();
        assert_eq!(load(&p).map(|_| ()), Err(DecodeError::WebpInvalid));
        let _ = std::fs::remove_file(&p);
        assert_eq!(encode(&Image::default(), Format::Webp, &SaveOptions::default(), "t"), Err(EncodeError::Failed));
    }
}
