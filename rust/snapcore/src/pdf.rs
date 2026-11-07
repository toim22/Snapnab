//! PDF export (Pdf.cpp; CPaintDocument::SaveAs → PDF::PDFPage::SavePDFDocument, ds_pseudocode §10.1): A4 or Letter by
//! the measurement system, orientation from the image shape, the image centered and fitted, written as JPEG
//! (DCTDecode); links captured with the image become /Link annotations; long images (scrolling captures) are split
//! across pages without cutting text lines (C34). The numbers are written by the C runtime's printf, as before, so the
//! files are byte for byte the same.

use std::io::Write;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::fs::OpenOptionsExt;

use windows::Win32::Globalization::{GetLocaleInfoEx, LOCALE_IMEASURE, LOCALE_RETURN_NUMBER};
use windows::core::PCWSTR;

use crate::codec::{self, Format, SaveOptions};
use crate::crt::{Arg, sprintf};
use crate::image::{Image, Rect, cb, cg, cr};
use crate::library::file_name;
use crate::reg;

/// A clickable area (image pixels) captured with the image (PdfLink).
#[derive(Clone, Debug)]
pub struct Link {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub url: Vec<u16>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PdfError {
    Empty,
    Encode,
    Create,
    Write,
}

/// SplitLongPages (on by default).
pub fn split_long_pages() -> bool {
    reg::get_dword(std::ffi::OsStr::new("SplitLongPages"), 1, None) != 0
}

/// The URI as a PDF literal string: ASCII, the rest percent-encoded, ( ) \ escaped.
fn pdf_uri(url: &[u16]) -> Vec<u8> {
    let mut out = Vec::new();
    for c in String::from_utf16_lossy(url).bytes() {
        if !(0x21..=0x7E).contains(&c) {
            out.extend_from_slice(format!("%{c:02X}").as_bytes());
        } else if c == b'(' || c == b')' || c == b'\\' {
            out.push(b'\\');
            out.push(c);
        } else {
            out.push(c);
        }
    }
    out
}

fn uses_us_letter() -> bool {
    let mut measure = [0u16; 2];
    unsafe { GetLocaleInfoEx(PCWSTR::null(), LOCALE_IMEASURE | LOCALE_RETURN_NUMBER, Some(&mut measure)) };
    measure[0] == 1
}

struct Page {
    src: Rect,
    pw: f64,
    ph: f64,
    s: f64,
    ix: f64,
    iy: f64,
    links: Vec<Link>,
}

/// RectF::Intersect, in single precision.
fn intersect(a: &Link, x: f32, y: f32, w: f32, h: f32) -> (f32, f32, f32, f32) {
    let right = (a.x + a.w).min(x + w);
    let bottom = (a.y + a.h).min(y + h);
    let left = a.x.max(x);
    let top = a.y.max(y);
    (left, top, right - left, bottom - top)
}

pub fn save_pdf(path: &[u16], img: &Image, links: &[Link]) -> Result<(), PdfError> {
    if img.is_empty() {
        return Err(PdfError::Empty);
    }
    let us = uses_us_letter();
    let (pw0, ph0) = if us { (612.0, 792.0) } else { (595.28, 841.89) };
    let (iw, ih) = (img.wi(), img.hi());
    let mut pages: Vec<Page> = Vec::new();
    if split_long_pages() && ih as f64 > iw as f64 * (ph0 / pw0) * 1.25 {
        let s = pw0 / iw as f64;
        let mut cuts = smart_breaks(img, 64.max((ph0 / s) as i32), -1);
        cuts.push(ih);
        let mut prev = 0;
        for c in cuts {
            if c <= prev {
                continue;
            }
            pages.push(Page { src: Rect::new(0, prev, iw, c), pw: pw0, ph: ph0, s, ix: 0.0, iy: ph0 - (c - prev) as f64 * s, links: vec![] });
            prev = c;
        }
    } else {
        let (mut pw, mut ph) = (pw0, ph0);
        if iw > ih {
            std::mem::swap(&mut pw, &mut ph);
        }
        let s = (pw / iw as f64).min(ph / ih as f64);
        pages.push(Page {
            src: Rect::new(0, 0, iw, ih),
            pw,
            ph,
            s,
            ix: (pw - iw as f64 * s) / 2.0,
            iy: (ph - ih as f64 * s) / 2.0,
            links: vec![],
        });
    }
    // link annotations: only web and e-mail addresses, cut to each page's part of the image
    let starts = |u: &[u16], p: &str| String::from_utf16_lossy(u).starts_with(p);
    for l in links {
        if !(starts(&l.url, "http://") || starts(&l.url, "https://") || starts(&l.url, "mailto:")) {
            continue;
        }
        for p in pages.iter_mut() {
            let (x, y, w, h) = intersect(l, p.src.left as f32, p.src.top as f32, (p.src.right - p.src.left) as f32,
                                         (p.src.bottom - p.src.top) as f32);
            if w >= 1.0 && h >= 1.0 {
                p.links.push(Link { x, y: y - p.src.top as f32, w, h, url: l.url.clone() });
            }
        }
    }
    // object numbers: 1 catalog, 2 page tree, then page / image / contents for each page, then the annotations, the info
    let n = pages.len();
    let mut annot_no = 3 + 3 * n;
    let mut annot_ids: Vec<Vec<usize>> = vec![vec![]; n];
    for (i, p) in pages.iter().enumerate() {
        for _ in &p.links {
            annot_ids[i].push(annot_no);
            annot_no += 1;
        }
    }
    let z = |v: usize| Arg::I(v as i64);
    let mut out: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offs: Vec<usize> = Vec::new();
    let obj = |out: &mut Vec<u8>, offs: &mut Vec<usize>, body: &[u8]| {
        offs.push(out.len());
        out.extend_from_slice(sprintf("%zu 0 obj\n", &[z(offs.len())]).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };
    obj(&mut out, &mut offs, b"<< /Type /Catalog /Pages 2 0 R >>");
    let mut kids = String::new();
    for i in 0..n {
        kids += &sprintf(if i != 0 { " %zu 0 R" } else { "%zu 0 R" }, &[z(3 + 3 * i)]);
    }
    obj(&mut out, &mut offs, format!("<< /Type /Pages /Kids [{kids}] /Count {n} >>").as_bytes());
    let mut jpeg_opt = SaveOptions::from_settings();
    jpeg_opt.jpeg_quality = 92;
    jpeg_opt.strip_metadata = true; // internal copies carry no metadata (EncodeToMemory)
    for (i, p) in pages.iter().enumerate() {
        let part = if n == 1 { img.clone() } else { img.crop(p.src) };
        let jpg = codec::encode(&part, Format::Jpeg, &jpeg_opt, "").map_err(|_| PdfError::Encode)?;
        let mut annots = String::new();
        for (k, id) in annot_ids[i].iter().enumerate() {
            annots += &sprintf(if k != 0 { " %zu 0 R" } else { "%zu 0 R" }, &[z(*id)]);
        }
        let (im, contents) = (4 + 3 * i, 5 + 3 * i);
        let page = sprintf(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 %.2f %.2f] /Resources << /XObject << /Im0 %zu 0 R >> >> /Contents %zu 0 R",
            &[Arg::F(p.pw), Arg::F(p.ph), z(im), z(contents)],
        ) + &if annots.is_empty() { String::new() } else { format!(" /Annots [{annots}]") }
            + " >>";
        obj(&mut out, &mut offs, page.as_bytes());
        offs.push(out.len());
        out.extend_from_slice(
            sprintf(
                "%zu 0 obj\n<< /Type /XObject /Subtype /Image /Width %d /Height %d /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length %zu >>\nstream\n",
                &[z(im), Arg::I(part.w as i64), Arg::I(part.h as i64), z(jpg.len())],
            )
            .as_bytes(),
        );
        out.extend_from_slice(&jpg);
        out.extend_from_slice(b"\nendstream\nendobj\n");
        let content = sprintf("q\n%.3f 0 0 %.3f %.3f %.3f cm\n/Im0 Do\nQ\n",
                              &[Arg::F(part.w as f64 * p.s), Arg::F(part.h as f64 * p.s), Arg::F(p.ix), Arg::F(p.iy)]);
        let body = sprintf("<< /Length %zu >>\nstream\n", &[z(content.len())]) + &content + "endstream";
        obj(&mut out, &mut offs, body.as_bytes());
    }
    for p in &pages {
        let part_h = p.src.bottom - p.src.top;
        for l in &p.links {
            // image pixels → page points (origin at the bottom left)
            let x0 = p.ix + l.x as f64 * p.s;
            let x1 = p.ix + (l.x + l.w) as f64 * p.s;
            let y1 = p.iy + (part_h as f32 - l.y) as f64 * p.s;
            let y0 = p.iy + (part_h as f32 - (l.y + l.h)) as f64 * p.s;
            let mut body = sprintf("<< /Type /Annot /Subtype /Link /Rect [%.2f %.2f %.2f %.2f] /Border [0 0 0] /A << /S /URI /URI (",
                                   &[Arg::F(x0), Arg::F(y0), Arg::F(x1), Arg::F(y1)])
                .into_bytes();
            body.extend_from_slice(&pdf_uri(&l.url));
            body.extend_from_slice(b") >> >>");
            obj(&mut out, &mut offs, &body);
        }
    }
    let mut title = String::from_utf16_lossy(&file_name(path)).into_bytes();
    for c in title.iter_mut() {
        if *c == b'(' || *c == b')' || *c == b'\\' {
            *c = b'_';
        }
    }
    let mut info = b"<< /Producer (Snapnab) /Title (".to_vec();
    info.extend_from_slice(&title);
    info.extend_from_slice(b") >>");
    obj(&mut out, &mut offs, &info);
    let xref = out.len();
    out.extend_from_slice(sprintf("xref\n0 %zu\n0000000000 65535 f \n", &[z(offs.len() + 1)]).as_bytes());
    for &o in &offs {
        out.extend_from_slice(sprintf("%010zu 00000 n \n", &[z(o)]).as_bytes());
    }
    out.extend_from_slice(
        sprintf("trailer\n<< /Size %zu /Root 1 0 R /Info %zu 0 R >>\nstartxref\n%zu\n%%%%EOF\n", &[z(offs.len() + 1), z(offs.len()), z(xref)])
            .as_bytes(),
    );

    let target = std::path::PathBuf::from(std::ffi::OsString::from_wide(path));
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .share_mode(0)
        .open(target)
        .map_err(|_| PdfError::Create)?;
    f.write_all(&out).map_err(|_| PdfError::Write)
}

// ---------------------------------------------------------------- long images across pages (C34; also Split image)

/// How busy each row is: pixels that differ from the row's first pixel (a text line makes it busy).
fn row_busyness(img: &Image) -> Vec<i32> {
    (0..img.hi())
        .map(|y| {
            let r = img.row(y);
            let rf = r[0];
            r.iter().filter(|&&p| (cr(p) - cr(rf)).abs() + (cg(p) - cg(rf)).abs() + (cb(p) - cb(rf)).abs() > 40).count() as i32
        })
        .collect()
}

/// Where to cut an image into `parts` pieces, moved to quiet rows.
pub fn smart_cuts(img: &Image, parts: i32) -> Vec<i32> {
    let mut cuts = Vec::new();
    if img.is_empty() || parts < 2 {
        return cuts;
    }
    let busy = row_busyness(img);
    let h = img.hi();
    let search = 4.max(h / (parts * 4));
    for i in 1..parts {
        let target = (h as i64 * i as i64 / parts as i64) as i32;
        let (mut best, mut best_score) = (target, i32::MAX);
        for y in 1.max(target - search)..=(h - 2).min(target + search) {
            let s = busy[y as usize] + busy[y as usize - 1] + busy[y as usize + 1];
            let score = s * 64 + (target - y).abs();
            if score < best_score {
                best_score = score;
                best = y;
            }
        }
        if cuts.last().is_none_or(|&l| best > l) {
            cuts.push(best);
        }
    }
    cuts
}

/// Page breaks every `page_h` rows at most, moved up to quiet rows (gaps between lines); `search` < 0 is a quarter page.
pub fn smart_breaks(img: &Image, page_h: i32, search: i32) -> Vec<i32> {
    let mut cuts = Vec::new();
    let h = img.hi();
    if img.is_empty() || page_h < 16 || h <= page_h {
        return cuts;
    }
    let search = if search < 0 { page_h / 4 } else { search };
    let busy = row_busyness(img);
    let mut pos = 0;
    while h - pos > page_h {
        let target = pos + page_h;
        let (mut best, mut best_score) = (target, i32::MAX);
        let mut y = target;
        while y > (pos + 16).max(target - search) {
            // a quiet row with quiet neighbors is a gap between lines; nearer the page end is better
            let s = busy[y as usize]
                + if y > 0 { busy[y as usize - 1] } else { 0 }
                + if y + 1 < h { busy[y as usize + 1] } else { 0 };
            let score = s * 64 + (target - y);
            if score < best_score {
                best_score = score;
                best = y;
            }
            if s == 0 && target - y > 0 && best_score <= target - y {
                break;
            }
            y -= 1;
        }
        cuts.push(best);
        pos = best;
    }
    cuts
}
