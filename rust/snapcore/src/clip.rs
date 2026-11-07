//! The clipboard (Image.cpp CopyImageToClipboard/GetClipboardImage, Clipboard.cpp): images as PNG (with alpha) and as
//! a DIB over white for older programs, text, and files (as Explorer's copy does).

use std::ffi::c_void;

use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::Graphics::Gdi::{BI_BITFIELDS, BI_RGB, BITMAPINFOHEADER, HBITMAP};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GMEM_ZEROINIT, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::System::Ole::{CF_BITMAP, CF_DIB, CF_DIBV5, CF_HDROP, CF_UNICODETEXT, DROPEFFECT_COPY};
use windows::Win32::UI::Shell::DROPFILES;
use windows::core::w;

use crate::codec::{self, Format, SaveOptions};
use crate::image::{Image, argb, blend_over, ca};
use crate::screen;

fn png_format() -> u32 {
    unsafe { RegisterClipboardFormatW(w!("PNG")) }
}

fn open(owner: isize, tries: u32, wait_ms: u64) -> bool {
    for i in 0..tries {
        if i > 0 {
            std::thread::sleep(std::time::Duration::from_millis(wait_ms));
        }
        let o = (owner != 0).then(|| HWND(owner as *mut c_void));
        if unsafe { OpenClipboard(o) }.is_ok() {
            return true;
        }
    }
    false
}

/// A movable global block with the bytes (for SetClipboardData, which takes it over).
fn global(data: &[u8], zero_extra: usize) -> Option<HGLOBAL> {
    unsafe {
        let flags = if zero_extra > 0 { GMEM_MOVEABLE | GMEM_ZEROINIT } else { GMEM_MOVEABLE };
        let h = GlobalAlloc(flags, data.len() + zero_extra).ok()?;
        let p = GlobalLock(h) as *mut u8;
        if p.is_null() {
            let _ = GlobalFree(Some(h));
            return None;
        }
        std::ptr::copy_nonoverlapping(data.as_ptr(), p, data.len());
        let _ = GlobalUnlock(h);
        Some(h)
    }
}

fn set(format: u32, h: HGLOBAL) -> bool {
    let ok = unsafe { SetClipboardData(format, Some(HANDLE(h.0))) }.is_ok();
    if !ok {
        unsafe {
            let _ = GlobalFree(Some(h));
        }
    }
    ok
}

/// CopyImageToClipboard: PNG (alpha kept) and CF_DIB (32 bpp bottom-up, composited over white).
pub fn copy_image(owner: isize, img: &Image, version: &str) -> bool {
    if img.is_empty() {
        return false;
    }
    let opt = SaveOptions { strip_metadata: true, ..SaveOptions::from_settings() };
    let png = codec::encode(img, Format::Png, &opt, version).unwrap_or_default();
    let bih = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: img.wi(),
        biHeight: img.hi(),
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        biSizeImage: (img.px.len() * 4) as u32,
        ..Default::default()
    };
    let mut dib = Vec::with_capacity(bih.biSize as usize + img.px.len() * 4);
    dib.extend_from_slice(unsafe {
        std::slice::from_raw_parts(&bih as *const _ as *const u8, std::mem::size_of::<BITMAPINFOHEADER>())
    });
    for y in (0..img.hi()).rev() {
        for &p in img.row(y) {
            dib.extend_from_slice(&blend_over(0xFFFF_FFFF, p).to_le_bytes());
        }
    }
    let Some(hdib) = global(&dib, 0) else { return false };
    if !open(owner, 1, 0) {
        unsafe {
            let _ = GlobalFree(Some(hdib));
        }
        return false;
    }
    unsafe {
        let _ = EmptyClipboard();
    }
    set(CF_DIB.0 as u32, hdib);
    if !png.is_empty()
        && let Some(h) = global(&png, 0)
    {
        set(png_format(), h);
    }
    unsafe {
        let _ = CloseClipboard();
    }
    true
}

/// ClipboardHasImage
pub fn has_image() -> bool {
    unsafe {
        [CF_DIB.0 as u32, CF_DIBV5.0 as u32, png_format(), CF_BITMAP.0 as u32]
            .iter()
            .any(|&f| IsClipboardFormatAvailable(f).is_ok())
    }
}

/// The bytes of a clipboard format (the clipboard is open).
unsafe fn data(format: u32) -> Option<Vec<u8>> {
    let h = unsafe { GetClipboardData(format) }.ok()?;
    let g = HGLOBAL(h.0);
    let p = unsafe { GlobalLock(g) } as *const u8;
    if p.is_null() {
        return None;
    }
    let v = unsafe { std::slice::from_raw_parts(p, GlobalSize(g)) }.to_vec();
    unsafe {
        let _ = GlobalUnlock(g);
    }
    Some(v)
}

/// A packed DIB (BITMAPINFOHEADER or V5 and the pixels), 24 or 32 bits.
fn decode_dib(d: &[u8]) -> Option<Image> {
    if d.len() < std::mem::size_of::<BITMAPINFOHEADER>() {
        return None;
    }
    let bih: BITMAPINFOHEADER = unsafe { std::ptr::read_unaligned(d.as_ptr() as *const BITMAPINFOHEADER) };
    if bih.biBitCount != 32 && bih.biBitCount != 24 {
        return None;
    }
    let (w, h) = (bih.biWidth, bih.biHeight.abs());
    let bottom_up = bih.biHeight > 0;
    let mut off = bih.biSize as usize;
    if bih.biCompression == BI_BITFIELDS.0 && bih.biSize as usize == std::mem::size_of::<BITMAPINFOHEADER>() {
        off += 12;
    }
    let stride = ((w * bih.biBitCount as i32 + 31) / 32 * 4) as usize;
    if w <= 0 || h <= 0 || d.len() < off + stride * h as usize {
        return None;
    }
    let mut out = Image::sized(w, h, 0);
    let mut any_alpha = false;
    for y in 0..h {
        let src = &d[off + (if bottom_up { h - 1 - y } else { y }) as usize * stride..];
        for x in 0..w as usize {
            let v = if bih.biBitCount == 32 {
                let v = u32::from_le_bytes(src[x * 4..x * 4 + 4].try_into().unwrap());
                any_alpha |= ca(v) != 0;
                v
            } else {
                argb(255, src[x * 3 + 2] as i32, src[x * 3 + 1] as i32, src[x * 3] as i32)
            };
            *out.at_mut(x as i32, y) = v;
        }
    }
    if !any_alpha {
        out.make_opaque();
    }
    Some(out)
}

/// GetClipboardImage: PNG first (it keeps the alpha), then the DIB, then the bitmap.
pub fn get_image() -> Option<Image> {
    if !open(0, 10, 50) {
        // the C++ read on even without the clipboard open (GetClipboardData then fails): nothing to read
        return None;
    }
    let mut out = unsafe { data(png_format()) }.and_then(|d| codec::decode(&d).ok());
    if out.is_none() {
        out = unsafe { data(CF_DIBV5.0 as u32).or_else(|| data(CF_DIB.0 as u32)) }.and_then(|d| decode_dib(&d));
    }
    if out.is_none()
        && let Ok(h) = unsafe { GetClipboardData(CF_BITMAP.0 as u32) }
    {
        out = Some(screen::from_hbitmap(HBITMAP(h.0))).filter(|i| !i.is_empty());
    }
    unsafe {
        let _ = CloseClipboard();
    }
    out
}

/// The Unicode text on the clipboard (empty when there is none).
pub fn get_text() -> String {
    if !open(0, 10, 30) {
        return String::new();
    }
    let bytes = unsafe { data(CF_UNICODETEXT.0 as u32) }.unwrap_or_default();
    unsafe {
        let _ = CloseClipboard();
    }
    let w: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&c| c != 0).collect();
    String::from_utf16_lossy(&w)
}

/// CopyText
pub fn copy_text(owner: isize, text: &str) -> bool {
    if !open(owner, 10, 30) {
        return false;
    }
    unsafe {
        let _ = EmptyClipboard();
    }
    let mut w: Vec<u16> = text.encode_utf16().collect();
    w.push(0);
    let bytes = unsafe { std::slice::from_raw_parts(w.as_ptr() as *const u8, w.len() * 2) };
    let ok = global(bytes, 0).is_some_and(|h| set(CF_UNICODETEXT.0 as u32, h));
    unsafe {
        let _ = CloseClipboard();
    }
    ok
}

/// CopyFiles: the files as Explorer's Copy puts them (CF_HDROP and "Preferred DropEffect" = copy).
pub fn copy_files(owner: isize, paths: &[String]) -> bool {
    if paths.is_empty() {
        return false;
    }
    let df = DROPFILES { pFiles: std::mem::size_of::<DROPFILES>() as u32, fWide: true.into(), ..Default::default() };
    let mut b: Vec<u8> =
        unsafe { std::slice::from_raw_parts(&df as *const _ as *const u8, std::mem::size_of::<DROPFILES>()) }.to_vec();
    for p in paths {
        for c in p.encode_utf16().chain([0]) {
            b.extend_from_slice(&c.to_le_bytes());
        }
    }
    let Some(hd) = global(&b, 2) else { return false };
    let he = global(&DROPEFFECT_COPY.0.to_le_bytes(), 0);
    if !open(owner, 10, 30) {
        unsafe {
            let _ = GlobalFree(Some(hd));
            if let Some(h) = he {
                let _ = GlobalFree(Some(h));
            }
        }
        return false;
    }
    unsafe {
        let _ = EmptyClipboard();
    }
    let ok = set(CF_HDROP.0 as u32, hd);
    if let Some(h) = he {
        set(unsafe { RegisterClipboardFormatW(w!("Preferred DropEffect")) }, h);
    }
    unsafe {
        let _ = CloseClipboard();
    }
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_packed_dibs() {
        // 2×2, 24 bits, bottom-up: rows are padded to 4 bytes
        let mut d = vec![0u8; 40];
        let bih = BITMAPINFOHEADER { biSize: 40, biWidth: 2, biHeight: 2, biPlanes: 1, biBitCount: 24, ..Default::default() };
        unsafe { std::ptr::write_unaligned(d.as_mut_ptr() as *mut BITMAPINFOHEADER, bih) };
        d.extend_from_slice(&[255, 0, 0, 0, 255, 0, 0, 0]); // bottom row: blue, green
        d.extend_from_slice(&[0, 0, 255, 9, 9, 9, 0, 0]); // top row: red, gray
        let img = decode_dib(&d).unwrap();
        assert_eq!(img.px, [0xFFFF0000, 0xFF090909, 0xFF0000FF, 0xFF00FF00]);
        // 32 bits without alpha become opaque; with alpha they keep it
        let mut d32 = vec![0u8; 40];
        let bih = BITMAPINFOHEADER { biSize: 40, biWidth: 1, biHeight: -1, biPlanes: 1, biBitCount: 32, ..Default::default() };
        unsafe { std::ptr::write_unaligned(d32.as_mut_ptr() as *mut BITMAPINFOHEADER, bih) };
        d32.extend_from_slice(&[1, 2, 3, 0]);
        assert_eq!(decode_dib(&d32).unwrap().px, [0xFF030201]);
        let n = d32.len();
        d32[n - 1] = 128;
        assert_eq!(decode_dib(&d32).unwrap().px, [0x80030201]);
        assert!(decode_dib(&d32[..41]).is_none(), "too short");
    }
}
