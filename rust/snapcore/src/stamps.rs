//! Stamp images (Render.cpp, Emoji.cpp, Cursors.cpp): emoji drawn by Direct2D with the color font, label badges
//! ("label:RRGGBB:Text") and the cursor stamps (C25) drawn as vectors at any size.

use windows::Win32::Graphics::Direct2D::Common::{D2D_RECT_F, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT};
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::GdiPlus::*;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::core::w;

use crate::doc::{bottom, right};
use crate::gdip::{Bitmap, Brush, FontFamily, Graphics, Matrix, Path, Pen, StringFormat, pt, rect};
use crate::image::{Image, argb, ca, cb, cg, cr};

fn add_round_rect(gp: &Path, r: RectF, rad: f32) {
    let mut rad = rad;
    if r.Width / 2.0 < rad {
        rad = r.Width / 2.0;
    }
    if r.Height / 2.0 < rad {
        rad = r.Height / 2.0;
    }
    if rad < 1.0 {
        gp.rectangle(r);
        return;
    }
    let d = rad * 2.0;
    gp.arc(r.X, r.Y, d, d, 180.0, 90.0);
    gp.arc(right(&r) - d, r.Y, d, d, 270.0, 90.0);
    gp.arc(right(&r) - d, bottom(&r) - d, d, d, 0.0, 90.0);
    gp.arc(r.X, bottom(&r) - d, d, d, 90.0, 90.0);
    gp.close_figure();
}

/// The font with the characters of the text (GDI+ paths do not fall back to other fonts).
fn font_for_text(t: &[u16]) -> &'static str {
    for &c in t {
        if (0x2E80..=0x9FFF).contains(&c) || (0xAC00..=0xD7AF).contains(&c) || (0xF900..=0xFAFF).contains(&c) || (0xFF00..=0xFFEF).contains(&c)
        {
            return "Microsoft YaHei UI";
        }
        if (0x0900..=0x0DFF).contains(&c) {
            return "Nirmala UI";
        }
    }
    "Segoe UI"
}

/// wcstoul(s, nullptr, 16)
fn hex_prefix(s: &str) -> u32 {
    let t = s.trim_start();
    let (neg, t) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let t = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).filter(|r| r.starts_with(|c: char| c.is_ascii_hexdigit())).unwrap_or(t);
    let mut v: u64 = 0;
    for c in t.chars() {
        let Some(d) = c.to_digit(16) else { break };
        v = v * 16 + d as u64;
        if v > u32::MAX as u64 {
            return u32::MAX;
        }
    }
    if neg { (v as u32).wrapping_neg() } else { v as u32 }
}

/// A label stamp ("label:RRGGBB:Text", Before/After, Wrong/Right…): a rounded badge in the color with the text in white.
/// The text is stored in the id, so the document looks the same in any language.
pub fn label(id: &str, px: i32) -> Image {
    let w16: Vec<u16> = id.encode_utf16().collect();
    let Some(c2) = w16.iter().skip(6).position(|&c| c == b':' as u16).map(|i| i + 6) else { return Image::default() };
    let col = 0xFF000000 | hex_prefix(&String::from_utf16_lossy(&w16[6..c2]));
    let text = &w16[c2 + 1..];
    let h = 16.0f32.max(px as f32 / 3.0);
    let em = h * 0.52;
    let ff = FontFamily::new(font_for_text(text));
    let fam = if ff.available() { ff } else { FontFamily::new("Segoe UI") };
    let tp = Path::new();
    let sf = StringFormat::typographic();
    tp.string(text, -1, &fam, FontStyleBold.0, em, rect(0.0, 0.0, 0.0, 0.0), &sf);
    let tb = tp.bounds();
    let pad_x = h * 0.42;
    let w = (h * 1.6).max(tb.Width + 2.0 * pad_x).ceil() as i32 + 2;
    let ih = h.ceil() as i32 + 2;
    let mut img = Image::sized(w, ih, 0);
    {
        let bmp = Bitmap::wrap(&mut img);
        let g = Graphics::new(&bmp);
        g.smoothing(SmoothingModeAntiAlias);
        g.pixel_offset(PixelOffsetModeHalf);
        let bx = rect(1.0, 1.0, w as f32 - 2.0, h);
        let bp = Path::new();
        add_round_rect(&bp, bx, h * 0.28);
        let l = |v: i32| v + (255 - v) * 2 / 5;
        let lb = Brush::linear(pt(0.0, bx.Y), pt(0.0, bottom(&bx)), argb(255, l(cr(col)), l(cg(col)), l(cb(col))), col);
        g.fill_path(&lb, &bp);
        g.draw_path(&Pen::new(argb(70, 0, 0, 0), 1.0f32.max(h * 0.04)), &bp);
        let m = Matrix::new();
        m.translate(bx.X + (bx.Width - tb.Width) / 2.0 - tb.X, bx.Y + (bx.Height - tb.Height) / 2.0 - tb.Y);
        tp.transform(&m);
        g.fill_path(&Brush::solid(0xFFFFFFFF), &tp);
        g.flush();
    }
    img
}

// ---------------------------------------------------------------- cursor stamps (C25)
fn poly(gp: &Path, pts: &[(f32, f32)], k: f32) {
    let v: Vec<PointF> = pts.iter().map(|&(x, y)| pt(x * k, y * k)).collect();
    gp.polygon(&v);
}

fn round_box(gp: &Path, r: RectF, rad: f32) {
    let mut rad = rad;
    if r.Width / 2.0 < rad {
        rad = r.Width / 2.0;
    }
    if r.Height / 2.0 < rad {
        rad = r.Height / 2.0;
    }
    gp.start_figure();
    gp.arc(r.X, r.Y, 2.0 * rad, 2.0 * rad, 180.0, 90.0);
    gp.arc(right(&r) - 2.0 * rad, r.Y, 2.0 * rad, 2.0 * rad, 270.0, 90.0);
    gp.arc(right(&r) - 2.0 * rad, bottom(&r) - 2.0 * rad, 2.0 * rad, 2.0 * rad, 0.0, 90.0);
    gp.arc(r.X, bottom(&r) - 2.0 * rad, 2.0 * rad, 2.0 * rad, 90.0, 90.0);
    gp.close_figure();
}

/// A filled shape with an outline around the union (the outline drawn first, wider; the fill covers the inner lines).
fn outlined(g: &Graphics, gp: &Path, fill: u32, line: u32, w: f32) {
    let p = Pen::new(line, 2.0 * w);
    p.line_join(LineJoinRound);
    g.draw_path(&p, gp);
    g.fill_path(&Brush::solid(fill), gp);
}

/// A cursor stamp ("cursor:arrow", "cursor:hand"…): drawn on a 24-unit grid at the size.
pub fn cursor(id: &str, px: i32) -> Image {
    let kind = if id.chars().count() > 7 { &id[7..] } else { "arrow" };
    let px = 16.max(px);
    let k = px as f32 / 24.0;
    let w = 1.0f32.max(k * 0.9);
    let (mut iw, ih) = (px, px);
    if kind == "arrow" || kind == "arrowdark" {
        iw = (px as f32 * 15.0 / 24.0).ceil() as i32;
    }
    let mut img = Image::sized(iw, ih, 0);
    {
        let bmp = Bitmap::wrap(&mut img);
        let g = Graphics::new(&bmp);
        g.smoothing(SmoothingModeAntiAlias);
        g.pixel_offset(PixelOffsetModeHalf);
        let (white, black) = (0xFFFFFFFFu32, 0xFF000000u32);
        match kind {
            "arrow" | "arrowdark" => {
                let gp = Path::new();
                poly(&gp, &[(1.5, 1.0), (1.5, 19.5), (6.0, 15.5), (9.0, 22.5), (12.0, 21.2), (9.1, 14.5), (14.5, 14.5)], k);
                let dark = kind == "arrowdark";
                outlined(&g, &gp, if dark { black } else { white }, if dark { white } else { black }, w);
            }
            "hand" => {
                let gp = Path::new();
                gp.fill_mode(FillModeWinding);
                round_box(&gp, rect(7.0 * k, 1.0 * k, 3.6 * k, 13.0 * k), 1.8 * k); // index finger
                round_box(&gp, rect(10.2 * k, 7.5 * k, 3.4 * k, 7.0 * k), 1.7 * k); // fingers
                round_box(&gp, rect(13.2 * k, 8.5 * k, 3.4 * k, 6.5 * k), 1.7 * k);
                round_box(&gp, rect(16.2 * k, 9.8 * k, 3.2 * k, 5.6 * k), 1.6 * k);
                round_box(&gp, rect(7.0 * k, 11.0 * k, 12.4 * k, 11.5 * k), 3.5 * k); // palm
                let m = Matrix::new();
                m.rotate_at(-35.0, pt(5.0 * k, 15.0 * k));
                let thumb = Path::new();
                round_box(&thumb, rect(1.5 * k, 13.5 * k, 8.0 * k, 3.4 * k), 1.7 * k);
                thumb.transform(&m);
                gp.add_path(&thumb, false);
                outlined(&g, &gp, white, black, w);
                let crease = Pen::new(black, w * 0.8);
                g.draw_line(&crease, 10.4 * k, 11.0 * k, 10.4 * k, 14.0 * k);
                g.draw_line(&crease, 13.4 * k, 11.5 * k, 13.4 * k, 14.2 * k);
                g.draw_line(&crease, 16.4 * k, 12.2 * k, 16.4 * k, 14.6 * k);
            }
            "ibeam" => {
                let gp = Path::new();
                poly(&gp, &[(7.0, 2.0), (11.0, 2.0), (12.0, 3.0), (13.0, 2.0), (17.0, 2.0), (17.0, 4.0), (13.0, 4.0), (13.0, 20.0),
                            (17.0, 20.0), (17.0, 22.0), (13.0, 22.0), (12.0, 21.0), (11.0, 22.0), (7.0, 22.0), (7.0, 20.0), (11.0, 20.0),
                            (11.0, 4.0), (7.0, 4.0)], k);
                outlined(&g, &gp, black, white, w);
            }
            "wait" => {
                let r = rect(3.0 * k, 3.0 * k, 18.0 * k, 18.0 * k);
                g.draw_ellipse(&Pen::new(argb(255, 220, 224, 230), 3.2 * k), r.X, r.Y, r.Width, r.Height);
                let arc = Pen::new(argb(255, 0, 120, 215), 3.2 * k);
                arc.start_cap(LineCapRound);
                arc.end_cap(LineCapRound);
                g.draw_arc(&arc, r, -90.0, 110.0);
            }
            "cross" => {
                let gp = Path::new();
                gp.rectangle(rect(11.0 * k, 1.0 * k, 2.0 * k, 22.0 * k));
                gp.rectangle(rect(1.0 * k, 11.0 * k, 22.0 * k, 2.0 * k));
                gp.fill_mode(FillModeWinding);
                outlined(&g, &gp, black, white, w);
            }
            "move" | "resize" => {
                // double arrow (resize) or four arrows (move)
                let gp = Path::new();
                gp.fill_mode(FillModeWinding);
                poly(&gp, &[(1.0, 12.0), (6.0, 7.0), (6.0, 10.5), (18.0, 10.5), (18.0, 7.0), (23.0, 12.0), (18.0, 17.0), (18.0, 13.5),
                            (6.0, 13.5), (6.0, 17.0)], k);
                if kind == "move" {
                    poly(&gp, &[(12.0, 1.0), (17.0, 6.0), (13.5, 6.0), (13.5, 18.0), (17.0, 18.0), (12.0, 23.0), (7.0, 18.0), (10.5, 18.0),
                                (10.5, 6.0), (7.0, 6.0)], k);
                }
                outlined(&g, &gp, black, white, w);
            }
            _ => {
                // click / clickright: concentric rings with a dot (where the click happened)
                let c = if kind == "clickright" { argb(255, 0, 120, 215) } else { argb(255, 229, 50, 45) };
                for i in 0..3 {
                    let rr = (4.5 + i as f32 * 3.6) * k;
                    let p = Pen::new(argb(230 - i * 70, cr(c), cg(c), cb(c)), 1.8 * k);
                    g.draw_ellipse(&p, 12.0 * k - rr, 12.0 * k - rr, 2.0 * rr, 2.0 * rr);
                }
                g.fill_ellipse(&Brush::solid(c), 12.0 * k - 2.4 * k, 12.0 * k - 2.4 * k, 4.8 * k, 4.8 * k);
            }
        }
        g.flush();
    }
    img
}

// ---------------------------------------------------------------- emoji (Emoji.cpp)
/// An emoji drawn in color (Segoe UI Emoji through Direct2D) on a transparent square of `px` pixels.
pub fn emoji(text: &str, px: i32) -> Image {
    let px = px.clamp(8, 2048);
    let t: Vec<u16> = text.encode_utf16().collect();
    if t.is_empty() {
        return Image::default();
    }
    emoji_d2d(&t, px).unwrap_or_default()
}

fn emoji_d2d(text: &[u16], px: i32) -> windows::core::Result<Image> {
    unsafe {
        let d2d: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
        let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let bmp = wic.CreateBitmap(px as u32, px as u32, &GUID_WICPixelFormat32bppPBGRA, WICBitmapCacheOnDemand)?;
        let rp = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: 0.0,
            dpiY: 0.0,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };
        let rt = d2d.CreateWicBitmapRenderTarget(&bmp, &rp)?;
        let size = px as f32 * 0.80;
        let fmt = dw.CreateTextFormat(w!("Segoe UI Emoji"), None, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STYLE_NORMAL,
                                      DWRITE_FONT_STRETCH_NORMAL, size, w!("pt-br"))?;
        fmt.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
        fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        fmt.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
        let brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.2, g: 0.2, b: 0.22, a: 1.0 }, None)?;
        rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        rt.BeginDraw();
        rt.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
        // a little taller than the square: the font box has breathing room above and below the glyph
        let p = px as f32;
        let layout = D2D_RECT_F { left: -p * 0.1, top: -p * 0.12, right: p * 1.1, bottom: p * 1.06 };
        rt.DrawText(text, &fmt, &layout, &brush, D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT, DWRITE_MEASURING_MODE_NATURAL);
        rt.EndDraw(None, None)?;
        let mut out = Image::sized(px, px, 0);
        let wr = WICRect { X: 0, Y: 0, Width: px, Height: px };
        bmp.CopyPixels(&wr, px as u32 * 4, out.bytes_mut())?;
        for p in out.px.iter_mut() {
            // premultiplied → straight alpha
            let a = ca(*p);
            if a == 0 {
                *p = 0;
                continue;
            }
            if a < 255 {
                *p = argb(a, 255.min(cr(*p) * 255 / a), 255.min(cg(*p) * 255 / a), 255.min(cb(*p) * 255 / a));
            }
        }
        std::result::Result::Ok(out)
    }
}
