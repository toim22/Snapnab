//! Grab Text (OCR, Ocr.cpp) with the native Windows OCR (Windows.Media.Ocr), and the faces Windows finds
//! (Windows.Media.FaceAnalysis) for the automatic redaction (C11). Both run on a thread of their own in the
//! multithreaded apartment, as WinRT wants.

use windows::Foundation::Rect as WinRect;
use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap};
use windows::Media::FaceAnalysis::FaceDetector;
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::Buffer;
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::System::WinRT::{IBufferByteAccess, RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize};
use windows::core::Interface;

use crate::gdip::scale_image;
use crate::image::{Image, Rect, cb, cg, cr};

/// Why it failed; `Message` carries the text of a Windows error.
#[derive(Debug, Clone, PartialEq)]
pub enum OcrError {
    Empty,
    NoLanguage,
    Failed,
    FacesUnsupported,
    FacesFailed,
    Message(String),
}

impl OcrError {
    /// The message shown to the user (OcrErrorText in Ocr.cpp).
    pub fn message(&self) -> String {
        match self {
            OcrError::Empty => crate::i18n::tr_str("Imagem vazia."),
            OcrError::NoLanguage => {
                crate::i18n::tr_str("Nenhum idioma de OCR instalado no Windows (Configurações › Hora e idioma › Idioma).")
            }
            OcrError::Failed => crate::i18n::tr_str("Falha desconhecida no OCR."),
            OcrError::FacesUnsupported => crate::i18n::tr_str("A detecção de rostos não está disponível neste Windows."),
            OcrError::FacesFailed => crate::i18n::tr_str("Falha na detecção de rostos."),
            OcrError::Message(m) => m.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Word {
    pub text: Vec<u16>,
    pub rect: Rect,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Line {
    pub text: Vec<u16>,
    pub rect: Rect,
    pub words: Vec<Word>,
}

/// Runs `f` on an MTA thread and waits for it; a Windows error becomes its message, a failure without one `fallback`.
fn on_mta<T: Send>(fallback: OcrError, f: impl FnOnce() -> Result<T, OcrError> + Send) -> Result<T, OcrError> {
    std::thread::scope(|s| {
        s.spawn(|| {
            let init = unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.is_ok();
            let r = f();
            if init {
                unsafe { RoUninitialize() };
            }
            r
        })
        .join()
        .unwrap_or(Err(fallback))
    })
}

fn win(fallback: &OcrError) -> impl Fn(windows::core::Error) -> OcrError + '_ {
    move |e| {
        let m = e.message();
        if m.is_empty() { fallback.clone() } else { OcrError::Message(m) }
    }
}

fn to_software_bitmap(work: &Image) -> windows::core::Result<SoftwareBitmap> {
    let sb = SoftwareBitmap::CreateWithAlpha(BitmapPixelFormat::Bgra8, work.wi(), work.hi(), BitmapAlphaMode::Premultiplied)?;
    let n = (work.px.len() * 4) as u32;
    let buf = Buffer::Create(n)?;
    unsafe {
        let p = buf.cast::<IBufferByteAccess>()?.Buffer()?;
        std::ptr::copy_nonoverlapping(work.bytes().as_ptr(), p, n as usize);
    }
    buf.SetLength(n)?;
    sb.CopyFromBuffer(&buf)?;
    Ok(sb)
}

/// The image on white (opaque).
fn opaque(src: &Image) -> Image {
    let mut img = Image::sized(src.wi(), src.hi(), 0xFFFFFFFF);
    img.blit(src, 0, 0, true);
    img
}

/// Reduced to the engine's largest size; the factor applied.
fn fit_engine(img: Image) -> windows::core::Result<(Image, f64)> {
    let max = OcrEngine::MaxImageDimension()?;
    if img.w > max || img.h > max {
        let s = (max as f64 / img.w as f64).min(max as f64 / img.h as f64);
        let (w, h) = ((img.w as f64 * s) as i32, (img.h as f64 * s) as i32);
        return Ok((scale_image(&img, w, h, true), s));
    }
    Ok((img, 1.0))
}

fn engine() -> Result<OcrEngine, OcrError> {
    OcrEngine::TryCreateFromUserProfileLanguages().map_err(|_| OcrError::NoLanguage)
}

/// The text, line by line ("\n" after each).
pub fn run_ocr(src: &Image) -> Result<Vec<u16>, OcrError> {
    if src.is_empty() {
        return Err(OcrError::Empty);
    }
    // opaque background and scaling: small text is enlarged, huge images are reduced
    let mut img = opaque(src);
    if img.h < 500 && img.w < 3000 {
        img = scale_image(&img, img.wi() * 2, img.hi() * 2, true);
    }
    on_mta(OcrError::Failed, move || {
        let e = win(&OcrError::Failed);
        let (work, _) = fit_engine(img).map_err(&e)?;
        let sb = to_software_bitmap(&work).map_err(&e)?;
        let engine = engine()?;
        let res = engine.RecognizeAsync(&sb).map_err(&e)?.join().map_err(&e)?;
        let mut out = Vec::new();
        for line in res.Lines().map_err(&e)? {
            out.extend_from_slice(&line.Text().map_err(&e)?);
            out.push('\n' as u16);
        }
        Ok(out)
    })
}

fn word_rect(r: &WinRect, f: f64) -> Rect {
    Rect::new((r.X as f64 * f).floor() as i32, (r.Y as f64 * f).floor() as i32, ((r.X + r.Width) as f64 * f).ceil() as i32,
              ((r.Y + r.Height) as f64 * f).ceil() as i32)
}

fn union(words: &[Word]) -> Rect {
    let mut u = Rect::new(i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for w in words {
        u.left = u.left.min(w.rect.left);
        u.top = u.top.min(w.rect.top);
        u.right = u.right.max(w.rect.right);
        u.bottom = u.bottom.max(w.rect.bottom);
    }
    u
}

/// Lines and words with their boxes (image pixels): automatic redaction (C11), smart highlighter and text-only blur.
pub fn run_ocr_lines(src: &Image) -> Result<Vec<Line>, OcrError> {
    if src.is_empty() {
        return Err(OcrError::Empty);
    }
    // the image as the engine wants it (opaque, enlarged when small) and the factor back to the original pixels
    let mut img = opaque(src);
    let mut k = 1.0;
    if img.h < 500 && img.w < 3000 {
        img = scale_image(&img, img.wi() * 2, img.hi() * 2, true);
        k = 2.0;
    }
    on_mta(OcrError::Failed, move || {
        let e = win(&OcrError::Failed);
        let (work, s) = fit_engine(img).map_err(&e)?;
        let sb = to_software_bitmap(&work).map_err(&e)?;
        let engine = engine()?;
        let res = engine.RecognizeAsync(&sb).map_err(&e)?.join().map_err(&e)?;
        let f = 1.0 / (k * s);
        let mut lines = Vec::new();
        for line in res.Lines().map_err(&e)? {
            let mut ol = Line { text: line.Text().map_err(&e)?.to_vec(), ..Default::default() };
            for w in line.Words().map_err(&e)? {
                let r = w.BoundingRect().map_err(&e)?;
                ol.words.push(Word { text: w.Text().map_err(&e)?.to_vec(), rect: word_rect(&r, f) });
            }
            if ol.words.is_empty() {
                continue;
            }
            ol.rect = union(&ol.words);
            lines.push(ol);
        }
        recover_missed_lines(&engine, &work, f, &mut lines).map_err(&e)?;
        Ok(lines)
    })
}

/// Second pass for text the engine leaves out. Windows OCR drops a whole line when it holds a run like "1111" (the test
/// card 4111 1111 1111 1111, for one), whatever the scale. Bands of the image that look like a line of text and that no
/// recognized line covers are cut into words and read again word by word, and a word that still comes back empty is
/// read two characters at a time (the engine reads "11" and "41" on their own). `img` is the prepared image; `f` maps
/// its pixels back to the original. Bounded in bands and time: it runs whenever text is recognized.
fn recover_missed_lines(engine: &OcrEngine, img: &Image, f: f64, lines: &mut Vec<Line>) -> windows::core::Result<()> {
    let (w, h) = (img.wi(), img.hi());
    if w < 16 || h < 16 || w as i64 * h as i64 > 40_000_000 {
        return Ok(());
    }
    let luma = |x: i32, y: i32| {
        let p = img.at(x, y);
        (cr(p) * 30 + cg(p) * 59 + cb(p) * 11) / 100
    };
    let mut hist = [0i32; 256];
    for y in (0..h).step_by(2) {
        for x in (0..w).step_by(2) {
            hist[luma(x, y) as usize] += 1;
        }
    }
    // the most common shade: the background (the first of equals, as std::max_element)
    let mut bg = 0;
    for (i, &c) in hist.iter().enumerate() {
        if c > hist[bg] {
            bg = i;
        }
    }
    let bg = bg as i32;
    let ink = |x: i32, y: i32| (luma(x, y) - bg).abs() > 80;
    let mut covered = vec![false; h as usize];
    for l in lines.iter() {
        let y0 = 0.max((l.rect.top as f64 / f) as i32);
        let y1 = h.min((l.rect.bottom as f64 / f).ceil() as i32);
        for y in y0..y1 {
            covered[y as usize] = true;
        }
    }
    let mut row_ink = vec![0i32; h as usize];
    for y in 0..h {
        if !covered[y as usize] {
            for x in 0..w {
                row_ink[y as usize] += ink(x, y) as i32;
            }
        }
    }
    let start = unsafe { GetTickCount64() };
    let mut bands = 0;
    let read = |piece: &Image| -> windows::core::Result<Vec<u16>> {
        let mut t = Vec::new();
        for line in engine.RecognizeAsync(&to_software_bitmap(piece)?)?.join()?.Lines()? {
            for wd in line.Words()? {
                t.extend_from_slice(&wd.Text()?);
            }
        }
        Ok(t)
    };
    // the piece on a margin of background
    let padded = |x0: i32, y0: i32, x1: i32, y1: i32| -> Image {
        let pad = 8.max((y1 - y0) / 2);
        let mut out = Image::sized(x1 - x0 + 2 * pad, y1 - y0 + 2 * pad, 0xFF000000 | (bg << 16 | bg << 8 | bg) as u32);
        out.blit(&img.crop(Rect::new(x0, y0, x1, y1)), pad, pad, false);
        // a small piece is enlarged: the engine reads two loose characters only when they are about 48 px tall
        if y1 - y0 < 48 {
            let s = 48.0 / (y1 - y0) as f64;
            out = scale_image(&out, (out.w as f64 * s) as i32, (out.h as f64 * s) as i32, true);
        }
        out
    };
    let mut y = 0;
    while y < h && bands < 8 && unsafe { GetTickCount64() } - start < 1500 {
        if row_ink[y as usize] == 0 {
            y += 1;
            continue;
        }
        let y0 = y;
        while y < h && row_ink[y as usize] > 0 {
            y += 1;
        }
        let (y1, lh) = (y, y - y0);
        if !(10..=120).contains(&lh) {
            continue;
        }
        // glyphs: runs of columns with ink; words: glyphs closer than about 0.4 of the line height
        let col_ink = |x: i32| (y0..y1).any(|yy| ink(x, yy));
        let mut glyphs: Vec<(i32, i32)> = Vec::new();
        let mut x = 0;
        while x < w {
            if !col_ink(x) {
                x += 1;
                continue;
            }
            let gx = x;
            while x < w && col_ink(x) {
                x += 1;
            }
            glyphs.push((gx, x));
        }
        if glyphs.len() < 4 || glyphs.len() > 200 {
            continue;
        }
        // glyphs no wider than the line is tall
        if !glyphs.iter().all(|g| (g.1 - g.0) as f64 <= lh as f64 * 1.5) {
            continue;
        }
        bands += 1;
        let mut words: Vec<Vec<(i32, i32)>> = vec![Vec::new()];
        for i in 0..glyphs.len() {
            if i > 0 && (glyphs[i].0 - glyphs[i - 1].1) as f64 > lh as f64 * 0.4 {
                words.push(Vec::new());
            }
            words.last_mut().unwrap().push(glyphs[i]);
        }
        let mut texts: Vec<Vec<u16>> = Vec::new();
        for wg in &words {
            texts.push(read(&padded(wg[0].0, y0, wg.last().unwrap().1, y1))?);
        }
        if texts.iter().all(|t| t.is_empty()) {
            continue; // nothing readable at all: an icon row or a picture, not a line of text
        }
        // consecutive unread words are read together, two glyphs at a time (a narrow "1" between wide gaps can come
        // out as a word of its own, and a single "1" is not read either)
        let mut groups: Vec<Vec<(i32, i32)>> = Vec::new();
        let mut group_text: Vec<Vec<u16>> = Vec::new();
        for n in 0..words.len() {
            if !texts[n].is_empty() || n == 0 || !texts[n - 1].is_empty() {
                groups.push(words[n].clone());
                group_text.push(texts[n].clone());
            } else {
                groups.last_mut().unwrap().extend_from_slice(&words[n]);
            }
        }
        let mut ol = Line::default();
        let (mut prev_usual, mut prev_end) = (0f64, -1i32);
        for (n, wg) in groups.iter().enumerate() {
            let mut t = group_text[n].clone();
            // word break after glyph a: a gap clearly wider than the usual one between glyphs ("1"s stand far apart)
            let mut gaps: Vec<i32> = wg.windows(2).map(|p| p[1].0 - p[0].1).collect();
            gaps.sort_unstable();
            let usual = if gaps.is_empty() { 0.0 } else { gaps[gaps.len() / 2] as f64 };
            let gap = |a: usize| {
                let g = (wg[a + 1].0 - wg[a].1) as f64;
                g > lh as f64 * 0.4 && g > usual * 1.6
            };
            if t.is_empty() && wg.len() <= 40 {
                let mut i = 0;
                while i < wg.len() {
                    let j = (i + 1).min(wg.len() - 1);
                    let mut s = read(&padded(wg[i].0, y0, wg[j].1, y1))?;
                    s.retain(|&c| c != ' ' as u16);
                    if j > i && s.len() == 2 && gap(i) {
                        s.insert(1, ' ' as u16); // the pair spans two words
                    }
                    let some = !s.is_empty();
                    t.extend_from_slice(&s);
                    if some && j + 1 < wg.len() && gap(j) {
                        t.push(' ' as u16);
                    }
                    i += 2;
                }
            }
            while t.last() == Some(&(' ' as u16)) {
                t.pop();
            }
            if t.is_empty() {
                continue;
            }
            let digits = t.iter().filter(|&&c| ('0' as u16..='9' as u16).contains(&c)).count();
            if digits * 2 >= t.len() {
                // digits read as look-alike letters
                for c in t.iter_mut() {
                    if "Il|i!".encode_utf16().any(|x| x == *c) {
                        *c = '1' as u16;
                    } else if "Oo".encode_utf16().any(|x| x == *c) {
                        *c = '0' as u16;
                    }
                }
            }
            let (first, last) = (wg[0].0, wg.last().unwrap().1);
            let rect = Rect::new((first as f64 * f).floor() as i32, (y0 as f64 * f).floor() as i32, (last as f64 * f).ceil() as i32,
                                 (y1 as f64 * f).ceil() as i32);
            // pieces of one word read apart ("41" + "11") are joined without a space
            let same_word = prev_end >= 0 && (first - prev_end) as f64 <= prev_usual.max(usual) * 1.6;
            if !(ol.text.is_empty() || same_word) {
                ol.text.push(' ' as u16);
            }
            ol.text.extend_from_slice(&t);
            ol.words.push(Word { text: t, rect });
            prev_usual = usual;
            prev_end = last;
        }
        if ol.words.is_empty() {
            continue;
        }
        ol.rect = union(&ol.words);
        let at = lines.iter().position(|l| l.rect.top > ol.rect.top).unwrap_or(lines.len());
        lines.insert(at, ol);
    }
    Ok(())
}

/// The faces Windows finds, in image pixels.
pub fn detect_faces(src: &Image) -> Result<Vec<Rect>, OcrError> {
    if src.is_empty() {
        return Err(OcrError::Empty);
    }
    let mut img = opaque(src);
    let mut s = 1.0;
    if img.w > 2000 || img.h > 2000 {
        s = (2000.0 / img.w as f64).min(2000.0 / img.h as f64);
        img = scale_image(&img, (img.w as f64 * s) as i32, (img.h as f64 * s) as i32, true);
    }
    on_mta(OcrError::FacesFailed, move || {
        let e = win(&OcrError::FacesFailed);
        if !FaceDetector::IsSupported().map_err(&e)? {
            return Err(OcrError::FacesUnsupported);
        }
        let sb = to_software_bitmap(&img).map_err(&e)?;
        let gray = SoftwareBitmap::Convert(&sb, BitmapPixelFormat::Gray8).map_err(&e)?;
        let det = FaceDetector::CreateAsync().map_err(&e)?.join().map_err(&e)?;
        let mut faces = Vec::new();
        for face in det.DetectFacesAsync(&gray).map_err(&e)?.join().map_err(&e)? {
            let b = face.FaceBox().map_err(&e)?;
            faces.push(Rect::new((b.X as f64 / s) as i32, (b.Y as f64 / s) as i32, ((b.X + b.Width) as f64 / s) as i32,
                                 ((b.Y + b.Height) as f64 / s) as i32));
        }
        Ok(faces)
    })
}
