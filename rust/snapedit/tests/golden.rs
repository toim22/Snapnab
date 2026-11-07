//! The golden checks: what the code gives for fixed inputs, against what the former C++ code (or the libraries as the
//! C++ build used them) gave, kept in `tests/data`. The C++ test program (`SnapnabTests.exe`) ran these until the C++
//! went away; each generator here builds the same lines, in the same order, with the same printf formatting.
//!
//! A mismatch writes what this build gives to `target/golden/<file>` so it can be compared with the reference.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use snapcore::beautify::{self, BeautifyOptions};
use snapcore::codec::{self, Format, SaveOptions};
use snapcore::crt::{Arg, sprintf};
use snapcore::cv;
use snapcore::doc::*;
use snapcore::fx::{self, CaptureEffects, Edge};
use snapcore::gdip::{self, Bitmap, Graphics, Path as GPath, pt, rect};
use snapcore::image::{Image, Meta, Rect, argb};
use snapcore::serial::{grad_from_xml, grad_stops_to_string, grad_to_xml, objects_from_clip, objects_to_clip};
use snapcore::xml::{self, Node};
use snapcore::{barcode, color, hdr, ocr, pdf, render};
use snapedit_app::doc::Document;
use windows::Win32::Graphics::GdiPlus::{PointF, RectF, SmoothingModeAntiAlias};

const VERSION: &str = "test";

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("tests").join("data")
}

fn compare(file: &str, lines: &[String]) {
    let path = data_dir().join(file);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let want: Vec<&str> = text.lines().map(|l| l.trim_end_matches('\r')).collect();
    let got: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
    if want == got {
        return;
    }
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("target").join("golden");
    let _ = std::fs::create_dir_all(&out);
    let _ = std::fs::write(out.join(file), lines.join("\n") + "\n");
    let mut diffs = Vec::new();
    for i in 0..want.len().max(got.len()) {
        let (w, g) = (want.get(i).copied().unwrap_or("<none>"), got.get(i).copied().unwrap_or("<none>"));
        if w != g {
            diffs.push(format!("line {}:\n  want {w}\n  got  {g}", i + 1));
        }
    }
    panic!("{file}: {} line(s) differ (this build's lines are in target/golden/{file})\n{}", diffs.len(),
           diffs.iter().take(12).cloned().collect::<Vec<_>>().join("\n"));
}

fn fnv1a(d: &[u8]) -> u64 {
    let mut h: u64 = 1469598103934665603;
    for &b in d {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    h
}

fn bytes<T>(v: &[T]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, std::mem::size_of_val(v)) }
}

fn hash_img(img: &Image) -> u64 {
    fnv1a(bytes(&img.px))
}

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn g(v: f64) -> String {
    sprintf("%g", &[Arg::F(v)])
}

fn f3(v: f32) -> String {
    sprintf("%.3f", &[Arg::F(v as f64)])
}

fn pattern(w: i32, h: i32, seed: i32) -> Image {
    let mut img = Image::sized(w, h, 0);
    for y in 0..h {
        for x in 0..w {
            let mut hsh = (x as u32).wrapping_mul(73856093) ^ (y as u32).wrapping_mul(19349663) ^ (seed as u32).wrapping_mul(83492791);
            hsh ^= hsh >> 13;
            hsh = hsh.wrapping_mul(0x5bd1e995);
            hsh ^= hsh >> 15;
            let v = (hsh & 255) as i32;
            // smooth blocks + noise: similar to text/icons on a page
            let blk = ((((x / 24 + (y / 18) * 7 + seed) as u32).wrapping_mul(2654435761)) >> 24) as i32 & 255;
            *img.at_mut(x, y) = argb(255, (blk + v / 4) & 255, (blk * 3 + v / 8) & 255, (255 - blk + v / 6) & 255);
        }
    }
    img
}

fn temp_dir() -> String {
    std::env::temp_dir().to_string_lossy().trim_end_matches('\\').to_string()
}

// ---------------------------------------------------------------- WebP (libwebp 1.5.0 as CMake built it)
#[test]
fn webp() {
    let mut imgs: Vec<(&str, Image)> = vec![("pattern-37x21", pattern(37, 21, 1))];
    let mut alpha = pattern(64, 48, 2);
    for (i, p) in alpha.px.iter_mut().enumerate() {
        *p = (*p & 0xFFFFFF) | (((i * 7 % 256) as u32) << 24);
    }
    imgs.push(("alpha-64x48", alpha));
    imgs.push(("pixel-1x1", Image::sized(1, 1, 0xFF123456)));
    imgs.push(("pattern-160x120", pattern(160, 120, 4)));
    let mut grad = Image::sized(96, 64, 0);
    for y in 0..64 {
        for x in 0..96 {
            *grad.at_mut(x, y) = argb(128 + y, x * 255 / 95, y * 4, 255 - x * 2);
        }
    }
    imgs.push(("gradient-96x64", grad));
    imgs.push(("clear-8x8", Image::sized(8, 8, 0)));
    imgs.push(("pattern-640x480", pattern(640, 480, 5)));
    let mut lines = Vec::new();
    for (name, img) in &imgs {
        for q in [0, 1, 40, 75, 90, 100] {
            let o = SaveOptions { webp_lossless: q == 0, webp_quality: q, ..Default::default() };
            let enc = codec::encode(img, Format::Webp, &o, VERSION).unwrap_or_default();
            let dec = if enc.is_empty() { None } else { codec::decode(&enc).ok() };
            let ok = dec.is_some();
            let dec = dec.unwrap_or_default();
            let qs = if q != 0 { format!("q{q}") } else { "lossless".into() };
            lines.push(format!("{name} {qs} {} {:016x} {}x{} {:016x}", if ok { enc.len() } else { 0 }, fnv1a(&enc), dec.w, dec.h,
                               hash_img(&dec)));
        }
    }
    compare("webp-libwebp-1.5.0.txt", &lines);
}

// ---------------------------------------------------------------- QR codes and barcodes (what ZXing-C++ 2.2.1 read)
/// Each code of a line: "[format|text|left,top,right,bottom|url]" as (format|text|url, box).
fn parse_codes(line: &str) -> Vec<(String, [i32; 4])> {
    let mut v = Vec::new();
    let mut rest = line;
    while let Some(a) = rest.find(" [") {
        let e = rest[a..].find(']').map(|e| a + e).unwrap_or(rest.len());
        let f = &rest[a + 2..e];
        let p1 = f.find('|').unwrap_or(0);
        let p2 = f[p1 + 1..].find('|').map(|x| p1 + 1 + x).unwrap_or(p1);
        let p3 = f.rfind('|').unwrap_or(p2);
        let mut b = [0i32; 4];
        for (k, n) in f[p2 + 1..p3].split(',').take(4).enumerate() {
            b[k] = n.trim().parse().unwrap_or(0);
        }
        v.push((format!("{}{}", &f[..p2], &f[p3..]), b));
        rest = &rest[(e + 1).min(rest.len())..];
    }
    v
}

/// The reader is the `rxing` crate, not ZXing-C++: every code must be read alike (format, text, url), and the box
/// rxing gives (a 1D code: the row it read) must overlap the one ZXing-C++ gave.
#[test]
fn barcodes() {
    let text = std::fs::read_to_string(data_dir().join("barcodes.txt")).unwrap();
    let want: Vec<&str> = text.lines().map(|l| l.trim_end_matches('\r')).filter(|l| !l.is_empty()).collect();
    let mut got = Vec::new();
    for l in &want {
        let Some(c) = l.find(':') else { continue };
        let name = &l[..c];
        let img = codec::load(&data_dir().join("barcodes").join(format!("{name}.png"))).unwrap_or_default();
        let mut line = format!("{name}:");
        for r in barcode::read(&img) {
            line += &format!(" [{}|{}|{},{},{},{}|{}]", r.format, r.text, r.rect.left, r.rect.top, r.rect.right, r.rect.bottom,
                             r.url as i32);
        }
        got.push(line);
    }
    let (mut differ, mut boxes, mut codes) = (Vec::new(), 0, 0);
    for (i, w) in want.iter().enumerate() {
        let (wc, gc) = (parse_codes(w), got.get(i).map(|g| parse_codes(g)).unwrap_or_default());
        let mut same = wc.len() == gc.len();
        for k in 0..wc.len() {
            if !same {
                break;
            }
            same = wc[k].0 == gc[k].0;
            let (a, b) = (wc[k].1, gc[k].1);
            if same && !(a[0].max(b[0]) < a[2].min(b[2]) && a[1].max(b[1]) < a[3].min(b[3])) {
                boxes += 1;
            }
        }
        codes += wc.len();
        if !same {
            differ.push(format!("  {} (was {w})", got.get(i).map(|s| s.as_str()).unwrap_or("missing")));
        }
    }
    assert!(want.len() == 23 && got.len() == want.len() && differ.is_empty() && boxes == 0 && codes == 23,
            "{} images, {codes} codes: {} read differently, {boxes} boxes elsewhere\n{}", want.len(), differ.len(), differ.join("\n"));
}

// ---------------------------------------------------------------- color texts, HDR tone mapping, OCR
#[test]
fn misc() {
    let mut l = Vec::new();
    let rgb = color::rgb;
    for c in [rgb(0, 0, 0), rgb(255, 255, 255), rgb(255, 128, 0), rgb(18, 171, 63), rgb(128, 128, 128), rgb(1, 2, 3),
              rgb(250, 5, 130), rgb(0, 0, 255), rgb(127, 255, 212), rgb(64, 0, 64)]
    {
        let mut line = format!("color {c:06x}:");
        for f in 0..color::FORMATS.len() as i32 {
            line += &format!(" [{}]", color::to_text(c, f));
        }
        let (h, s, li) = color::rgb_to_hsl(c);
        line += &sprintf(" hsl %.17g %.17g %.17g lum %.17g contrast %.17g",
                         &[Arg::F(h), Arg::F(s), Arg::F(li), Arg::F(color::luminance(c)), Arg::F(color::contrast_ratio(c, rgb(40, 90, 200)))]);
        l.push(line);
    }
    for f in color::FORMATS {
        l.push(format!("format {f}"));
    }
    for t in ["#12AB3F", "#12ab3fzz", "12AB3F", "#FFF", "# 12AB3F", "#0x12AB3F", "#-1", "#123456789AB"] {
        l.push(format!("hex {t} -> {:06x}", color::hex_to_color(&w(t), 0x777777)));
    }
    let mut h = 0u32;
    while h < 65536 {
        l.push(format!("half {h:04x} {}", sprintf("%.9g", &[Arg::F(hdr::half_to_float(h as u16) as f64)])));
        h += 997;
    }
    for white in [80.0f32, 200.0, 480.0, 0.5] {
        let lut: Vec<u8> = (0..65536u32).map(|h| hdr::tone_map(hdr::half_to_float(h as u16), white)).collect();
        l.push(format!("tonemap {} {:016x}", g(white as f64), fnv1a(&lut)));
    }
    for v in [-1.0f32, 0.0, 0.001, 0.5, 1.0, 2.0, 12.5, 1e9] {
        l.push(format!("tone {}: {} {}", g(v as f64), hdr::tone_map(v, 80.0), hdr::tone_map(v, 240.0)));
    }
    for (n, t) in ["Hello Snapnab 2026", "4111 1111 1111 1111", "Grab text from the screen", "ABC-123 xyz"].iter().enumerate() {
        let img = fx::caption(&Image::sized(560, 4, 0xFFFFFFFF), &w(t), true, 28, 0xFF000000, 0xFFFFFFFF);
        let r = ocr::run_ocr(&img);
        let r2 = ocr::run_ocr_lines(&img);
        let text = r.as_ref().map(|t| String::from_utf16_lossy(t)).unwrap_or_default();
        let mut line = format!("ocr {n}: {} [{text}] {}", r.is_ok() as i32, r2.is_ok() as i32);
        for ln in r2.unwrap_or_default() {
            let b = ln.rect;
            line += &format!(" {{{} {},{},{},{}", String::from_utf16_lossy(&ln.text), b.left, b.top, b.right, b.bottom);
            for wd in &ln.words {
                let b = wd.rect;
                line += &format!(" <{} {},{},{},{}>", String::from_utf16_lossy(&wd.text), b.left, b.top, b.right, b.bottom);
            }
            line += "}";
        }
        l.push(line.replace('\n', "|"));
    }
    let faces = ocr::detect_faces(&pattern(200, 150, 9));
    l.push(format!("faces {} {}", faces.is_ok() as i32, faces.map(|f| f.len()).unwrap_or(0)));
    compare("misc-cpp.txt", &l);
}

// ---------------------------------------------------------------- effects, Beautify, computer vision, scaling, PDF
fn fp(name: &str, img: &Image) -> String {
    format!("{name} {}x{} {:016x} dpi={} meta={}", img.w, img.h, hash_img(img), g(img.dpi as f64), img.meta.is_some() as i32)
}

fn fp_mask(name: &str, m: &[u8]) -> String {
    format!("{name} {} {:016x}", m.len(), fnv1a(m))
}

#[test]
fn pixels() {
    gdip::startup();
    let tmp = temp_dir();
    let mut l: Vec<String> = Vec::new();
    let mut p = pattern(97, 61, 3);
    p.dpi = 144.0;
    p.meta = Some(Meta { make: "Maker".into(), ..Default::default() });
    let mut a = pattern(80, 50, 4);
    for (i, px) in a.px.iter_mut().enumerate() {
        *px = (*px & 0xFFFFFF) | (((i * 13 % 256) as u32) << 24);
    }
    let mut framed = Image::sized(60, 40, 0xFFF0F0F0);
    framed.blit(&pattern(30, 20, 5), 12, 9, false);
    let tiny = Image::sized(1, 1, 0xFF336699);
    let srcs: [(&str, &Image); 4] = [("P", &p), ("A", &a), ("framed", &framed), ("tiny", &tiny)];
    for (n, im) in srcs {
        let k = |what: &str| format!("{n} {what}");
        l.push(fp(&k("rot90cw"), &fx::rotate90(im, true)));
        l.push(fp(&k("rot90ccw"), &fx::rotate90(im, false)));
        l.push(fp(&k("rot180"), &fx::rotate180(im)));
        l.push(fp(&k("flipH"), &fx::flip_h(im)));
        l.push(fp(&k("flipV"), &fx::flip_v(im)));
        l.push(fp(&k("rotate15"), &fx::rotate_angle(im, 15.0, 0xFFFFFFFF)));
        l.push(fp(&k("rotate-37.5"), &fx::rotate_angle(im, -37.5, 0)));
        for anchor in 0..9 {
            l.push(fp(&format!("{n} canvas+{anchor}"), &fx::canvas_resize(im, im.wi() + 17, im.hi() + 8, anchor, 0xFF102030)));
            l.push(fp(&format!("{n} canvas-{anchor}"), &fx::canvas_resize(im, im.wi() / 2, im.hi() - 3, anchor, 0)));
        }
        l.push(fp(&k("trim8"), &fx::auto_trim(im, 8)));
        l.push(fp(&k("trim0"), &fx::auto_trim(im, 0)));
        for pc in [100, 50, 150, 7] {
            l.push(fp(&format!("{n} scale{pc}"), &fx::scale(im, pc)));
        }
        let mut inplace = |what: &str, f: &dyn Fn(&mut Image)| {
            let mut t = (*im).clone();
            f(&mut t);
            l.push(fp(&k(what), &t));
        };
        inplace("gray", &|t| fx::grayscale(t));
        inplace("invert", &|t| fx::invert(t));
        inplace("sepia", &|t| fx::sepia(t));
        inplace("mono128", &|t| fx::monochrome(t, 128));
        inplace("mono60", &|t| fx::monochrome(t, 60));
        inplace("bc30-20", &|t| fx::brightness_contrast(t, 30, -20));
        inplace("bc-50+70", &|t| fx::brightness_contrast(t, -50, 70));
        inplace("hs45+30", &|t| fx::hue_saturation(t, 45, 30));
        inplace("hs-120-60", &|t| fx::hue_saturation(t, -120, -60));
        inplace("gamma0.5", &|t| fx::gamma(t, 0.5));
        inplace("gamma2.2", &|t| fx::gamma(t, 2.2));
        let first = im.px[0];
        inplace("replace", &|t| fx::replace_color(t, first, 0xFFFF00FF, 10));
        inplace("blur", &|t| fx::box_blur(t, Rect::new(3, 2, 40, 30), 4));
        inplace("blurAll", &|t| fx::box_blur(t, Rect::new(-5, -5, 500, 500), 9));
        inplace("pixelate", &|t| fx::pixelate(t, Rect::new(5, 4, 70, 45), 7));
        inplace("pixelate1", &|t| fx::pixelate(t, Rect::new(0, 0, 500, 500), 1));
        inplace("emboss3", &|t| fx::emboss(t, 3));
        inplace("emboss10", &|t| fx::emboss(t, 12));
        inplace("sharpen40", &|t| fx::sharpen(t, 40));
        inplace("sharpen100", &|t| fx::sharpen(t, 100));
        inplace("watermark", &|t| fx::watermark(t, &w("Snapnab"), 35, false, false));
        inplace("watermarkTiled", &|t| fx::watermark(t, &w("Confidencial ação"), 60, true, true));
        l.push(fp(&k("border"), &fx::border(im, 5, 0xFF00FF00)));
        l.push(fp(&k("shadow"), &fx::shadow(im, 6, 6, 50)));
        l.push(fp(&k("shadow0"), &fx::shadow(im, 0, 1, 100)));
        for st in 0..6 {
            for sides in [fx::TOP | fx::RIGHT | fx::BOTTOM | fx::LEFT, fx::BOTTOM | fx::RIGHT, fx::TOP | fx::LEFT] {
                for sh in [false, true] {
                    l.push(fp(&format!("{n} edge{st}-{sides}-{}", sh as i32), &fx::edge_effect(im, Edge::from_i32(st), 9, sides, sh)));
                }
            }
        }
        l.push(fp(&k("persp"), &fx::perspective(im, 40, -25, 30)));
        l.push(fp(&k("persp2"), &fx::perspective(im, -100, 100, -100)));
        for c in 0..4 {
            l.push(fp(&format!("{n} curl{c}"), &fx::page_curl(im, 20, c)));
        }
        l.push(fp(&k("spot"), &fx::spotlight(im, Rect::new(10, 8, 40, 30), 50, 3, 100, false)));
        l.push(fp(&k("spotZoom"), &fx::spotlight(im, Rect::new(10, 8, 40, 30), 70, 0, 150, true)));
        l.push(fp(&k("spotOut"), &fx::spotlight(im, Rect::new(500, 500, 600, 600), 70, 0, 150, true)));
        l.push(fp(&k("caption"), &fx::caption(im, &w("Snapnab — teste"), false, 12, 0xFFFFFFFF, 0xFF202020)));
        l.push(fp(&k("captionTop"), &fx::caption(im, &w("Um texto bem comprido que não cabe na largura"), true, 14, 0xFF000000, 0x80FFFFFF)));
        l.push(fp(&k("scaleHQ"), &gdip::scale_image(im, 45, 31, true)));
        l.push(fp(&k("scaleNN"), &gdip::scale_image(im, 150, 20, false)));
        l.push(fp(&k("thumb"), &gdip::make_thumbnail(im, 40, 40)));
    }
    // the capture preset effects, alone and together (no {date}/{time} tokens: they change)
    {
        let mut e = CaptureEffects::default();
        let mut run = |name: &str, e: &CaptureEffects| {
            l.push(fp(name, &fx::apply_capture_effects(&p, e)));
            l.push(fp(&format!("{name}-A"), &fx::apply_capture_effects(&a, e)));
        };
        run("ce none", &e);
        e.trim = true;
        run("ce trim", &e);
        e.rotate = true;
        for r in 0..5 {
            e.rotate_kind = r;
            run(&format!("ce rotate{r}"), &e);
        }
        e.scale = true;
        e.scale_percent = 80;
        run("ce scale", &e);
        e.adjust = true;
        e.brightness = 10;
        e.contrast = 20;
        e.hue = 30;
        e.saturation = -10;
        run("ce adjust", &e);
        e.filter = true;
        for f in 0..4 {
            e.filter_kind = f;
            run(&format!("ce filter{f}"), &e);
        }
        e.replace_color = true;
        run("ce replace", &e);
        e.watermark = true;
        run("ce watermark", &e);
        e.caption = true;
        e.caption_text = w("Snapnab");
        e.caption_top = true;
        run("ce caption", &e);
        e.border = true;
        run("ce border", &e);
        e.edge = true;
        e.edge_sides = 0;
        run("ce edge", &e);
        e.shadow = true;
        run("ce shadow", &e);
        e.beautify = true;
        run("ce beautify", &e);
        e.resolution = true;
        e.dpi = 200;
        run("ce resolution", &e);
    }
    // Beautify: backgrounds, aspect ratios, alignment, corners, shadow, auto balance
    {
        let bg_file = format!("{tmp}\\beautify_bg.png");
        codec::save_file(Path::new(&bg_file), &pattern(50, 90, 9), Format::Png, &SaveOptions::default(), VERSION);
        let mut i = 0;
        for bg in [0, 1, 3, 4] {
            for aspect in 0..5 {
                for align in [0, 4, 8] {
                    let o = BeautifyOptions {
                        background: bg,
                        aspect,
                        align,
                        image_path: w(&bg_file),
                        angle: (i * 47) % 360,
                        padding: 10 + i % 30,
                        radius: i % 25,
                        shadow: (i * 7) % 101,
                        shadow_size: i % 30,
                        auto_balance: i % 2 == 0,
                        ..Default::default()
                    };
                    for (n, im) in srcs {
                        let (r, org) = beautify::beautify(im, &o);
                        l.push(fp(&format!("beautify{i} {n}"), &r) + &format!(" at {},{}", org.0, org.1));
                    }
                    i += 1;
                }
            }
        }
        let _ = std::fs::remove_file(&bg_file);
    }
    // computer vision
    {
        let mut wi = pattern(64, 48, 7);
        for y in 10..30 {
            for x in 5..40 {
                *wi.at_mut(x, y) = 0xFF2040F0 + ((x * y) % 5) as u32;
            }
        }
        for (sx, sy) in [(6, 11), (0, 0), (63, 47), (-1, 3)] {
            for tol in [0.0f32, 1.0, 5.0, 30.0, 100.0, 150.0] {
                for global in [false, true] {
                    l.push(fp_mask(&format!("wand {sx},{sy} {} {}", g(tol as f64), global as i32), &cv::wand_select(&wi, (sx, sy), tol, global)));
                    l.push(fp_mask(&format!("dowand {sx},{sy} {} {}", g(tol as f64), global as i32), &cv::do_wand(&wi, (sx, sy), tol, global)));
                }
            }
        }
        l.push(fp_mask("rect", &cv::rect_mask(64, 48, Rect::new(-3, 5, 30, 70))));
        l.push(fp_mask("ellipse", &cv::ellipse_mask(64, 48, Rect::new(4, 3, 50, 40))));
        l.push(fp_mask("ellipseThin", &cv::ellipse_mask(64, 48, Rect::new(10, 10, 10, 30))));
        l.push(fp_mask("polygon", &cv::polygon_mask(64, 48, &[(5, 5), (60, 10), (30, 45), (2, 30)])));
        l.push(fp_mask("polygonStar", &cv::polygon_mask(64, 48, &[(32, 0), (40, 47), (0, 15), (63, 15), (24, 47)])));
        l.push(fp_mask("polygon2", &cv::polygon_mask(64, 48, &[(5, 5), (60, 10)])));
        let m = cv::ellipse_mask(64, 48, Rect::new(20, 12, 44, 36));
        let b = cv::mask_bounds(&m, 64, 48);
        l.push(format!("bounds {} {} {} {} empty {} {}", b.left, b.top, b.right, b.bottom, cv::mask_empty(&m) as i32,
                       cv::mask_empty(&vec![0u8; 10]) as i32));
        let b = cv::mask_bounds(&vec![0u8; 64 * 48], 64, 48);
        l.push(format!("bounds0 {} {} {} {}", b.left, b.top, b.right, b.bottom));
        for k in [1, 3, 7, 12] {
            l.push(fp_mask(&format!("dilate{k}"), &cv::dilate(&m, 64, 48, k)));
        }
        for k in [3, 7] {
            let mut t = wi.clone();
            let r = cv::dingo7_inpaint(&mut t, &m, k);
            let (dom, share) = r.unwrap_or((0, 0.0));
            l.push(fp(&format!("dingo{k}"), &t) + &format!(" ok={} dom={dom:08x} share={}", r.is_some() as i32, sprintf("%.17g", &[Arg::F(share)])));
        }
        {
            let mut t = wi.clone();
            let ok = cv::dingo7_inpaint(&mut t, &vec![255u8; 64 * 48], 3).is_some();
            l.push(fp("dingoAll", &t) + &format!(" ok={}", ok as i32));
        }
        {
            let mut t = wi.clone();
            cv::diffusion_inpaint(&mut t, &m, 0);
            l.push(fp("diffusion", &t));
        }
        {
            let mut t = wi.clone();
            cv::diffusion_inpaint(&mut t, &m, 5);
            l.push(fp("diffusion5", &t));
        }
        {
            let mut t = a.clone();
            cv::diffusion_inpaint(&mut t, &cv::rect_mask(80, 50, Rect::new(0, 0, 80, 10)), 0);
            l.push(fp("diffusionEdge", &t));
        }
        {
            let mut t = wi.clone();
            cv::smart_fill(&mut t, &m);
            l.push(fp("smartfill", &t));
        }
        {
            let mut t = Image::sized(64, 48, 0xFFFFFFFF);
            t.fill_rect(Rect::new(20, 20, 30, 30), 0xFF000000);
            cv::smart_fill(&mut t, &cv::rect_mask(64, 48, Rect::new(18, 18, 32, 32)));
            l.push(fp("smartfillFlat", &t));
        }
        for im in [&p, &a, &tiny] {
            l.push(format!("autotol {}", sprintf("%.9g", &[Arg::F(cv::smart_move_auto_tolerance(im) as f64)])));
        }
        l.push(format!("autotol0 {}", sprintf("%.9g", &[Arg::F(cv::smart_move_auto_tolerance(&Image::default()) as f64)])));
        l.push(sprintf("tolscale %.9g %.9g", &[Arg::F(cv::smart_move_tolerance_scaling(12.0, 2.0, 75.0, 0.0, 100.0) as f64),
                                                Arg::F(cv::smart_move_tolerance_scaling(5.0, 3.0, 3.0, 0.0, 1.0) as f64)]));
        // translations: shifted parts of a larger image
        let mut big = pattern(700, 900, 11);
        let mut y = 0;
        while y < 900 {
            for x in 20..600 {
                *big.at_mut(x, y) = 0xFF000000;
            }
            y += 37;
        }
        for (dx, dy, ww, hh) in [(0, 40, 300, 400), (0, -90, 640, 500), (13, 7, 200, 160), (0, 0, 120, 90), (0, 300, 600, 560)] {
            let (ax, ay) = ((-dx).max(0) + 10, (-dy).max(0) + 100);
            let ia = big.crop(Rect::new(ax, ay, ax + ww, ay + hh));
            let ib = big.crop(Rect::new(ax + dx, ay + dy, ax + dx + ww, ay + dy + hh));
            let (d, conf) = cv::detect_translation(&ia, &ib);
            l.push(format!("translation {dx},{dy} {ww}x{hh} -> {},{} conf {}", d.0, d.1, sprintf("%.17g", &[Arg::F(conf)])));
        }
        let (none, _) = cv::detect_translation(&p, &a);
        l.push(format!("translation mismatch {},{}", none.0, none.1));
    }
    // long images across pages, and the PDF writer
    {
        let mut page = Image::sized(300, 2600, 0xFFFFFFFF);
        for line in 0..120 {
            let y0 = 12 + line * 21 + (line % 7);
            for y in y0..(y0 + 11).min(page.hi()) {
                for x in 10..280 - (line % 5) * 30 {
                    if (x * 7 + y * 3 + line) % 5 < 3 {
                        *page.at_mut(x, y) = 0xFF202020;
                    }
                }
            }
        }
        let list = |v: &[i32]| v.iter().map(|c| format!(" {c}")).collect::<String>();
        for ph in [400, 777, 3000, 10] {
            l.push(format!("breaks {ph}:{}", list(&pdf::smart_breaks(&page, ph, -1))));
        }
        l.push(format!("breaks s40:{}", list(&pdf::smart_breaks(&page, 500, 40))));
        for parts in [1, 2, 5, 9] {
            l.push(format!("cuts {parts}:{}", list(&pdf::smart_cuts(&page, parts))));
        }
        let mut k = 0;
        for part in snapedit_app::edtools::split_image(&page, &[900, 100, 5000, -4, 1300], false) {
            l.push(fp(&format!("split{k}"), &part));
            k += 1;
        }
        for part in snapedit_app::edtools::split_image(&p, &[30, 60], true) {
            l.push(fp(&format!("splitV{k}"), &part));
            k += 1;
        }
        let mut pdf_fp = |name: &str, img: &Image, links: &[pdf::Link]| {
            let f = format!("{tmp}\\fingerprint (a).pdf");
            let r = pdf::save_pdf(&w(&f), img, links);
            let d = if r.is_ok() { std::fs::read(&f).unwrap_or_default() } else { Vec::new() };
            let _ = std::fs::remove_file(&f);
            let err = match r {
                Ok(()) => "",
                Err(pdf::PdfError::Empty) => "Imagem vazia.",
                Err(pdf::PdfError::Create) => "Não foi possível criar o arquivo.",
                Err(pdf::PdfError::Write) => "Falha ao gravar o arquivo.",
                Err(pdf::PdfError::Encode) => "Falha ao codificar a imagem.",
            };
            l.push(format!("pdf {name} ok={} {} {:016x} err={err}", r.is_ok() as i32, d.len(), fnv1a(&d)));
        };
        let link = |x: f32, y: f32, ww: f32, hh: f32, url: &str| pdf::Link { x, y, w: ww, h: hh, url: w(url) };
        let links = [link(10.0, 20.0, 80.0, 15.0, "https://example.com/a(b)?q=ação"), link(5.0, 1290.0, 100.0, 30.0, "mailto:x@y.z"),
                     link(5.0, 5.0, 10.0, 10.0, "file:///C:/x"), link(0.0, 2590.0, 300.0, 40.0, "http://edge")];
        pdf_fp("page", &page, &links);
        pdf_fp("P", &p, &[]);
        pdf_fp("wide", &pattern(400, 100, 2), &[link(0.0, 0.0, 400.0, 100.0, "https://w")]);
        pdf_fp("A", &a, &[]);
        pdf_fp("empty", &Image::default(), &[]);
    }
    compare("pixels-cpp.txt", &l);
}

// ---------------------------------------------------------------- the Editor's painters
fn obj(t: ObjType, x1: f32, y1: f32, x2: f32, y2: f32) -> DrawObj {
    let mut o = DrawObj::new(t);
    o.p1 = pt(x1, y1);
    o.p2 = pt(x2, y2);
    o.ctrl = pt((x1 + x2) / 2.0, (y1 + y2) / 2.0 - 40.0);
    o
}

fn boxf(r: RectF) -> String {
    format!("{},{},{},{}", f3(r.X), f3(r.Y), f3(r.Width), f3(r.Height))
}

fn path_fp(gp: &GPath) -> String {
    let (pts, types) = gp.data();
    let h = fnv1a(bytes(&pts)).wrapping_mul(31).wrapping_add(fnv1a(&types));
    format!("{} {h:016x}", pts.len())
}

fn grad(kind: i32, angle: f32, stops: &[(f32, u32)]) -> Gradient {
    Gradient { kind, angle, stops: stops.iter().map(|&(pos, color)| GradStop { pos, color }).collect() }
}

#[test]
fn painters() {
    gdip::startup();
    let mut l: Vec<String> = Vec::new();
    let mut base = Image::sized(200, 150, 0);
    for y in 0..150 {
        for x in 0..200 {
            *base.at_mut(x, y) = argb(255, (x * 5 + y) & 255, (y * 7) & 255, if (x / 10 + y / 10) % 2 != 0 { 230 } else { 40 });
        }
    }
    let base = Arc::new(base);
    let render_objs = |l: &mut Vec<String>, name: &str, objs: &[DrawObj], hide_first: bool| {
        let mut d = Document::default();
        d.set_base((*base).clone());
        for o in objs {
            d.st.objs.push(Arc::new(o.clone()));
        }
        let out = d.render(if hide_first { Some(0) } else { None });
        l.push(format!("{name} {:016x} b={}", hash_img(&out), boxf(objs[0].bounds())));
    };
    let add = |l: &mut Vec<String>, name: &str, o: &DrawObj| render_objs(l, name, std::slice::from_ref(o), false);
    let zig: Vec<PointF> = (0..24).map(|i| pt(20.0 + i as f32 * 7.0, 75.0 + ((i as f64 * 0.7).sin() as f32) * 40.0)).collect();

    // lines and arrows: every pair of ends, dashes, curves, hand-drawn strokes and labels
    for a in 0..CAP_COUNT {
        for b in 0..CAP_COUNT {
            let mut o = obj(ObjType::Arrow, 30.0, 110.0, 170.0, 40.0);
            o.style.arrow_start = a;
            o.style.arrow_end = b;
            o.style.start_size = 1.0 + (a % 4) as f32;
            o.style.end_size = 2.0 + (b % 3) as f32;
            o.style.width = 2.0 + ((a + b) % 6) as f32;
            o.style.shadow = (a + b) % 2 == 0;
            o.style.shadow_dir = (a * 3 + b) % 9;
            o.style.bezier = (a + b) % 3 == 0;
            add(&mut l, &format!("arrow {a} {b}"), &o);
        }
    }
    for d in 0..DASH_COUNT {
        for c in 0..2 {
            let mut o = obj(ObjType::Line, 20.0, 20.0, 180.0, 130.0);
            o.style.dash = d;
            o.style.width = 5.0;
            o.style.bezier = c == 1;
            o.style.arrow_start = if c != 0 { CAP_ROUND } else { CAP_FLAT };
            o.style.arrow_end = CAP_FLAT;
            o.style.stroke = 0xFF2060E0;
            o.style.opacity = 80 - d * 5;
            add(&mut l, &format!("line dash {d} {c}"), &o);
        }
    }
    for c in 0..4 {
        let mut o = obj(ObjType::Arrow, 25.0, 120.0, 175.0, 30.0);
        o.style.sketch = true;
        o.style.sketch_seed = 1 + c * 6;
        o.style.bezier = c % 2 == 1;
        o.style.width = 3.0 + c as f32 * 2.0;
        o.style.arrow_start = if c == 3 { CAP_ARROW_OPEN } else { CAP_FLAT };
        add(&mut l, &format!("arrow sketch {c}"), &o);
    }
    {
        let mut o = obj(ObjType::Arrow, 100.0, 75.0, 104.0, 78.0);
        add(&mut l, "arrow short", &o);
        o.p2 = pt(100.2, 75.1);
        add(&mut l, "arrow tiny", &o);
    }
    let labels = ["Look here", "Two\nlines", "A much longer label that wraps around", "Ends\n"];
    for (i, lab) in labels.iter().enumerate() {
        let i = i as i32;
        for c in 0..2 {
            let mut o = obj(ObjType::Arrow, 120.0, 60.0 + i as f32 * 10.0, 40.0, 120.0);
            o.text = w(lab);
            o.style.bezier = c == 1;
            o.style.font_size = 14.0 + i as f32 * 3.0;
            o.style.arrow_start = if i % 2 != 0 { CAP_ARROW } else { CAP_FLAT };
            o.style.italic = i == 2;
            o.style.underline = i == 3;
            o.style.bold = i != 1;
            add(&mut l, &format!("arrow label {i} {c}"), &o);
            l.push(format!("arrow label rect {i} {c} {}", boxf(render::arrow_label_rect(&o))));
        }
    }
    for i in 0..4 {
        let mut o = obj(if i < 2 { ObjType::Line } else { ObjType::Arrow }, if i % 2 != 0 { 180.0 } else { 20.0 }, 100.0,
                        if i % 2 != 0 { 20.0 } else { 180.0 }, 60.0);
        o.text = w("Text along a path");
        o.style.text_on_path = true;
        o.style.bezier = i >= 2;
        o.style.font_size = 16.0;
        o.style.sketch = i == 3;
        add(&mut l, &format!("text on path {i}"), &o);
    }

    // shapes: every outline, fills, gradients, dashes and hand-drawn outlines
    for k in 0..SHAPE_COUNT {
        for v in 0..3 {
            let mut o = obj(ObjType::Shape, 30.0, 25.0, 165.0 - v as f32 * 20.0, 125.0);
            o.style.shape_kind = k;
            o.style.width = if v == 2 { 0.0 } else { 3.0 + v as f32 * 3.0 };
            o.style.fill = if v != 0 { 0x9020C0F0 } else { 0 };
            o.style.dash = (k + v) % DASH_COUNT;
            o.style.sketch = v == 1 && k % 2 == 0;
            o.style.sketch_seed = k + 2;
            add(&mut l, &format!("shape {k} {v}"), &o);
        }
    }
    for (i, t) in [ObjType::Rect, ObjType::Ellipse, ObjType::RoundRect].into_iter().enumerate() {
        let mut o = obj(t, 160.0, 120.0, 40.0, 30.0);
        o.style.fill = 0xFFFFFFFF;
        add(&mut l, &format!("old shape {i}"), &o);
    }
    for gi in 0..6 {
        let mut o = obj(ObjType::Shape, 20.0, 20.0, 180.0, 130.0);
        o.style.shape_kind = if gi % 3 == 2 { SHAPE_ELLIPSE } else { SHAPE_RECT };
        o.style.grad = grad(if gi < 4 { 1 } else { 2 }, gi as f32 * 37.0, &[(0.0, 0xFFFF0000), (1.0, 0x800000FF)]);
        if gi % 2 != 0 {
            o.style.grad.stops = grad(0, 0.0, &[(0.2, 0xFF00FF00), (0.5, 0xFFFFFF00), (0.7, 0x40FF00FF), (0.9, 0xFF000000)]).stops;
        }
        o.style.opacity = 100 - gi * 7;
        add(&mut l, &format!("gradient {gi}"), &o);
        let br = render::fill_brush(o.style.fill, &o.style.grad, o.rect(), 70 + gi * 5);
        let mut im = Image::sized(80, 60, 0);
        {
            let bmp = Bitmap::wrap(&mut im);
            let gg = Graphics::new(&bmp);
            gg.fill_rect(&br, rect(0.0, 0.0, 80.0, 60.0));
        }
        l.push(format!("fill brush {gi} {:016x}", hash_img(&im)));
        let mask: Vec<u8> = (0..base.px.len()).map(|i| (i * 7 % 256) as u8).collect();
        let mut fm = (*base).clone();
        render::fill_gradient_mask(&mut fm, if gi % 2 != 0 { &mask } else { &[] }, Rect::new(10, 5, 190, 140), &o.style.grad, 90);
        l.push(format!("gradient mask {gi} {:016x}", hash_img(&fm)));
    }

    // pen strokes
    {
        let mut o = obj(ObjType::Pen, 0.0, 0.0, 0.0, 0.0);
        o.pts = zig.clone();
        o.style.width = 6.0;
        o.style.stroke = 0xFF10A040;
        add(&mut l, "pen", &o);
        o.style.dash = DASH_DOT;
        add(&mut l, "pen dash", &o);
        o.style.dash = DASH_SOLID;
        o.pressure = (0..o.pts.len()).map(|i| (i % 7) as f32 / 6.0).collect();
        add(&mut l, "pen pressure", &o);
        o.pressure.clear();
        o.pts = vec![pt(100.0, 75.0)];
        o.style.width = 12.0;
        add(&mut l, "pen dot", &o);
    }

    // highlighters, hides and spotlights: rectangles, every area outline, polygons, freehand, words only, old strokes
    let areas = [ObjType::Highlighter, ObjType::Blur, ObjType::Pixelate, ObjType::Spotlight];
    let poly = vec![pt(30.0, 30.0), pt(170.0, 40.0), pt(150.0, 130.0), pt(60.0, 110.0), pt(20.0, 70.0)];
    for (t, &kind) in areas.iter().enumerate() {
        for s in 0..AREA_SHAPE_COUNT + 1 {
            let mut o = obj(kind, 35.0, 30.0, 165.0, 120.0);
            o.style.stroke = 0xFFFFE000;
            o.style.opacity = if t != 0 { 100 } else { 55 };
            o.style.amount = 4 + s * 2;
            o.style.shadow = s % 2 == 1;
            if s < AREA_SHAPE_COUNT {
                o.style.area_shape = s;
                if is_point_area(s) {
                    o.pts = poly.clone();
                }
            } else {
                o.style.text_only = true;
                o.text_boxes = vec![rect(40.0, 40.0, 50.0, 14.0), rect(100.0, 42.0, 60.0, 12.0), rect(40.0, 80.0, 90.0, 16.0)];
            }
            add(&mut l, &format!("area {t} {s}"), &o);
        }
    }
    {
        let mut o = obj(ObjType::Highlighter, 0.0, 0.0, 0.0, 0.0);
        o.pts = zig.clone();
        o.style.width = 14.0;
        o.style.stroke = 0xFFFFE000;
        o.style.opacity = 70;
        add(&mut l, "highlighter freehand", &o);
        o.pts = vec![pt(80.0, 80.0)];
        add(&mut l, "highlighter freehand dot", &o);
    }

    // texts: alignments, line spacing, vertical, hand-drawn, outline and the old background box
    for h in 0..3 {
        for v in 0..3 {
            let mut o = obj(ObjType::Text, 15.0, 10.0, 185.0, 140.0);
            o.text = w("Snapnab text\nsecond line");
            o.style.align_h = h;
            o.style.align_v = v;
            o.style.font_size = 18.0 + h as f32 * 4.0;
            o.style.text_color = 0xFF102030;
            o.style.width = v as f32;
            o.style.stroke = 0xFFFFFFFF;
            add(&mut l, &format!("text {h} {v}"), &o);
        }
    }
    let texts = ["Spacing\nbetween\nlines", "Ends with enter\n\n", "Supercalifragilisticexpialidocious word", ""];
    for (i, t) in texts.iter().enumerate() {
        for v in 0..3 {
            let mut o = obj(ObjType::Text, 20.0, 15.0, 180.0, 135.0);
            o.text = w(t);
            o.style.line_spacing = 0.8 + v as f32 * 0.9;
            o.style.align_v = v;
            o.style.align_h = 2 - v;
            o.style.font_size = 17.0;
            o.style.font = match v {
                1 => "Georgia",
                2 => "No Such Font Here",
                _ => "Segoe UI",
            }
            .into();
            add(&mut l, &format!("text spacing {i} {v}"), &o);
            for wd in [30.0f32, 90.0, 170.0] {
                l.push(sprintf(&format!("text height {i} {v} %.0f %.4f"),
                               &[Arg::F(wd as f64), Arg::F(render::text_block_height(&o, wd) as f64)]));
            }
        }
    }
    {
        let mut o = obj(ObjType::Text, 20.0, 10.0, 180.0, 140.0);
        o.text = w("Vertical text");
        o.style.vertical = true;
        o.style.font_size = 22.0;
        add(&mut l, "text vertical", &o);
        l.push(sprintf("text vertical height %.4f", &[Arg::F(render::text_block_height(&o, 120.0) as f64)]));
        o.style.vertical = false;
        o.style.sketch = true;
        o.style.italic = true;
        o.style.underline = true;
        add(&mut l, "text sketch", &o);
        o.style.sketch = false;
        o.style.fill = 0xC0FFFFFF;
        o.style.padding = 12.0;
        add(&mut l, "text box", &o);
        o.style.opacity = 40;
        o.style.shadow = false;
        add(&mut l, "text faded", &o);
    }

    // callouts: the 24 shapes, tips everywhere, shaded, frosted glass, leaders, text boxes
    for c in 0..CALLOUT_SHAPES {
        for t in 0..4 {
            let mut o = obj(ObjType::Callout, 50.0, 40.0, 150.0, 100.0);
            let tips = [pt(190.0, 140.0), pt(100.0, 70.0), pt(10.0, 70.0), pt(100.0, 5.0)];
            o.tail = tips[t as usize];
            o.text = w("Callout text here");
            o.style.callout_shape = c;
            o.style.font_size = 13.0;
            o.style.fill = 0xFFFFFFF0;
            o.style.stroke = 0xFF303030;
            o.style.width = if t == 3 { 0.0 } else { 2.0 + t as f32 };
            o.style.shaded = t == 1;
            o.style.arrow_end = t;
            o.style.tail_width = 3.0 + t as f32;
            add(&mut l, &format!("callout {c} {t}"), &o);
            l.push(format!("callout text rect {c} {t} {}", boxf(render::callout_text_rect(&o))));
        }
    }
    {
        let mut o = obj(ObjType::Callout, 40.0, 30.0, 160.0, 110.0);
        o.tail = pt(190.0, 140.0);
        o.text = w("Frosted");
        o.style.bg_blur = true;
        o.style.bg_blur_amount = 6;
        o.style.fill = 0x80FFFFFF;
        add(&mut l, "callout frosted", &o);
        o.style.vertical = true;
        o.style.bg_blur = false;
        o.style.line_spacing = 1.4;
        add(&mut l, "callout vertical", &o);
        render_objs(&mut l, "callout hidden text", std::slice::from_ref(&o), true);
    }
    for c in 0..CALLOUT_SHAPES {
        let gp = GPath::new();
        render::callout_body_path(&gp, c, rect(10.0, 20.0, 120.0, 70.0));
        l.push(format!("callout body {c} {}", path_fp(&gp)));
    }
    for k in 0..SHAPE_COUNT {
        let gp = GPath::new();
        render::shape_path(&gp, k, rect(5.0, 10.0, 90.0, 60.0));
        l.push(format!("shape path {k} {}", path_fp(&gp)));
    }
    for s in 0..AREA_SHAPE_COUNT {
        let mut o = obj(ObjType::Spotlight, 10.0, 10.0, 90.0, 70.0);
        o.style.area_shape = s;
        if is_point_area(s) {
            o.pts = poly.clone();
        }
        let gp = GPath::new();
        render::area_path(&gp, &o);
        l.push(format!("area path {s} {}", path_fp(&gp)));
    }

    // steps: shapes, number styles, outlines
    for s in 0..4 {
        for t in 0..STEP_TYPE_COUNT {
            let mut o = obj(ObjType::Step, 100.0, 75.0, 100.0, 75.0);
            let vals = [1, 12, 123, 27, 3999];
            o.step_value = vals[t as usize];
            o.style.step_shape = s;
            o.style.step_type = t;
            o.style.font_size = 20.0 + s as f32 * 6.0;
            o.style.text_color = 0xFFFFFFFF;
            o.style.step_outline_width = if t % 2 != 0 { 3.0 } else { 0.0 };
            add(&mut l, &format!("step {s} {t}"), &o);
        }
    }
    for v in [1, 26, 27, 52, 702, 703, 1994, 3999, 5000] {
        for t in 0..STEP_TYPE_COUNT {
            l.push(format!("step label {v} {t} {}", step_label(v, t)));
        }
    }
    for wd in [1.0f32, 4.0, 9.0] {
        for s in [0.5f32, 3.0, 6.0] {
            l.push(sprintf("cap length %.1f %.1f %.4f", &[Arg::F(wd as f64), Arg::F(s as f64), Arg::F(cap_length(wd, s) as f64)]));
        }
    }

    // stamps: emoji, labels (Latin, CJK, Devanagari) and every cursor
    let mut stamps: Vec<String> = vec![
        "\u{1F600}".into(),
        "\u{2705}".into(),
        "label:2E7D32:Before".into(),
        "label:C62828:Wrong".into(),
        "label:1565C0:\u{4E2D}\u{6587}".into(),
        "label:6A1B9A:\u{0939}\u{093F}".into(),
        "label:bad".into(),
    ];
    for c in ["arrow", "arrowdark", "hand", "ibeam", "wait", "cross", "move", "resize", "click", "clickright"] {
        stamps.push(format!("cursor:{c}"));
    }
    for (i, s) in stamps.iter().enumerate() {
        let mut o = obj(ObjType::Stamp, 40.0, 20.0, 160.0, 130.0);
        o.text = w(s);
        o.style.shadow = i % 2 == 0;
        add(&mut l, &format!("stamp {i}"), &o);
        let si = render::stamp_image(s, 70);
        l.push(format!("stamp image {i} {}x{} {:016x} aspect {}", si.w, si.h, hash_img(&si),
                       sprintf("%.4f", &[Arg::F(render::stamp_aspect(s) as f64)])));
    }

    // magnifiers: shapes, connected, loupes
    for m in 0..MAGNIFY_KIND_COUNT {
        for v in 0..4 {
            let mut o = obj(ObjType::Magnify, 100.0, 50.0, 180.0, 130.0);
            o.tail = pt(50.0, 50.0);
            o.style.magnify_shape = m;
            o.style.magnify_scale = 150 + v * 100;
            o.style.magnify_link = v % 2 == 1;
            o.style.loupe = v >= 2;
            o.style.loupe_handle = v == 2;
            o.style.width = 2.0 + v as f32;
            o.style.stroke = 0xFF404040u32.wrapping_add(v as u32 * 0x200000);
            add(&mut l, &format!("magnify {m} {v}"), &o);
            l.push(format!("magnifier extra {m} {v} {}", boxf(render::magnifier_extra(&o))));
        }
    }

    // embedded pictures, groups, several objects together
    {
        let mut pic = Image::sized(30, 20, 0);
        for y in 0..20 {
            for x in 0..30 {
                *pic.at_mut(x, y) = argb(200 + y, x * 8, y * 12, 128);
            }
        }
        let mut o = obj(ObjType::Picture, 30.0, 30.0, 150.0, 110.0);
        o.picture = Some(pic);
        o.style.width = 0.0;
        add(&mut l, "picture", &o);
        o.style.width = 3.0;
        o.style.stroke = 0xFF000000;
        o.style.opacity = 70;
        add(&mut l, "picture framed", &o);
        let mut gr = obj(ObjType::Group, 0.0, 0.0, 0.0, 0.0);
        gr.children = vec![obj(ObjType::Arrow, 20.0, 20.0, 120.0, 100.0), obj(ObjType::Shape, 90.0, 40.0, 180.0, 140.0)];
        gr.children[1].style.fill = 0x8000FF00;
        gr.update_group_rect();
        add(&mut l, "group", &gr);
        let mut b = obj(ObjType::Blur, 0.0, 0.0, 200.0, 70.0);
        b.style.amount = 8;
        render_objs(&mut l, "stack", &[gr.children[0].clone(), b, gr.children[1].clone(), o], false);
    }

    // the line end samples of the menus
    for c in 0..CAP_COUNT {
        for s in 0..2 {
            let mut im = Image::sized(60, 24, 0xFFFFFFFF);
            {
                let bmp = Bitmap::wrap(&mut im);
                let gg = Graphics::new(&bmp);
                gg.smoothing(SmoothingModeAntiAlias);
                render::draw_cap_glyph(&gg, c, rect(0.0, 0.0, 60.0, 24.0), 0xFF141414, s == 1);
            }
            l.push(format!("cap glyph {c} {s} {:016x}", hash_img(&im)));
        }
    }
    compare("render-cpp.txt", &l);
}

// ---------------------------------------------------------------- the objects' XML, hit-testing, the .snap document
fn one_line(s: &str) -> String {
    s.replace('\n', "\\n").replace('\r', "\\r")
}

/// An object's XML as the clipboard (and the .snap) store it; a picture's PNG by its hash.
fn xml_of(o: &DrawObj) -> String {
    let s = objects_to_clip(std::slice::from_ref(o), "");
    let mut x = match (s.find("<Objects>"), s.find("</Objects>")) {
        (Some(a), Some(b)) => s[a + 9..b].to_string(),
        _ => s.clone(),
    };
    if let Some(e) = s.find("<Embedded") {
        x += &format!(" [{:016x}]", fnv1a(&s.as_bytes()[e..]));
    }
    one_line(&x)
}

fn extra(o: &DrawObj) -> String {
    let f = |v: f32| f3(v);
    format!("t={} hk={} ms={} sv={} aa={} ctrl={},{} tail={},{} p={},{},{},{} kids={} pic={}", o.kind as i32, o.style.hide_kind,
            o.style.magnify_scale, o.step_value, o.style.antialias as i32, f(o.ctrl.X), f(o.ctrl.Y), f(o.tail.X), f(o.tail.Y),
            f(o.p1.X), f(o.p1.Y), f(o.p2.X), f(o.p2.Y), o.children.len(),
            o.picture.as_ref().map(|p| p.w as i64 * 1000 + p.h as i64).unwrap_or(-1))
}

fn tmp(name: &str) -> String {
    std::env::temp_dir().join(format!("snapnab_test_{name}")).to_string_lossy().into_owned()
}

#[test]
fn documents() {
    gdip::startup();
    let mut l: Vec<String> = Vec::new();
    let read_xml = |x: &str| {
        objects_from_clip(format!("<SnapnabObjects Source=\"\"><Objects>{x}</Objects></SnapnabObjects>").as_bytes()).0.into_iter().next()
    };

    // a set of objects that touches every attribute
    let mut s: Vec<(String, DrawObj)> = Vec::new();
    {
        let mut o = obj(ObjType::Arrow, 30.0, 110.0, 170.0, 40.0);
        o.style.bezier = true;
        o.style.dash = 3;
        o.style.arrow_start = CAP_DIAMOND;
        o.style.start_size = 2.5;
        o.style.width = 6.0;
        o.text = w("Label <&> \"q\" 'a'\nnext\tline");
        o.style.font = "Segoe UI".into();
        o.style.italic = true;
        o.style.font_size = 13.25;
        s.push(("arrow label".into(), o));
        let mut ln = obj(ObjType::Line, -12.5, 0.00005, 1e7, -0.00015);
        ln.style.stroke = 0x80FF0000;
        ln.style.opacity = 35;
        ln.style.shadow = false;
        ln.name = "Línea ✓".into();
        ln.locked = true;
        s.push(("line".into(), ln));
    }
    for k in 0..SHAPE_COUNT {
        let mut o = obj(ObjType::Shape, 30.0, 25.0, 165.0, 125.0);
        o.style.shape_kind = k;
        o.style.fill = if k % 2 != 0 { 0x9020C0F0 } else { 0 };
        o.style.width = 1.0 + k as f32;
        if k == 2 {
            o.style.grad = grad(1, 37.5, &[(0.0, 0xFF112233), (0.4, 0x80445566), (1.0, 0xFFFFFFFF)]);
        }
        if k == 5 {
            o.style.grad = grad(2, 90.0, &[(0.125, 0xFF000000), (0.875, 0x00FFFFFF)]);
        }
        if k == 3 {
            o.style.sketch = true;
            o.style.sketch_seed = 42;
        }
        s.push((format!("shape {k}"), o));
    }
    s.push(("old rect".into(), obj(ObjType::Rect, 160.0, 120.0, 40.0, 30.0)));
    s.push(("old ellipse".into(), obj(ObjType::Ellipse, 10.0, 10.0, 90.0, 60.0)));
    s.push(("old roundrect".into(), obj(ObjType::RoundRect, 50.0, 50.0, 150.0, 100.0)));
    {
        let mut o = obj(ObjType::Pen, 0.0, 0.0, 0.0, 0.0);
        for i in 0..12 {
            o.pts.push(pt(20.0 + i as f32 * 13.3, 75.0 + ((i as f64 * 0.7).sin() as f32) * 40.0));
            o.pressure.push(i as f32 / 11.0);
        }
        o.p1 = o.pts[0];
        o.p2 = *o.pts.last().unwrap();
        o.style.arrow_end = CAP_ARROW;
        s.push(("pen pressure".into(), o.clone()));
        o.pressure.pop();
        s.push(("pen pressure mismatch".into(), o));
    }
    {
        let mut o = obj(ObjType::Highlighter, 20.0, 30.0, 120.0, 60.0);
        o.style.stroke = 0x80FFFF00;
        s.push(("highlighter".into(), o.clone()));
        o.style.area_shape = AREA_POLYGON;
        o.pts = vec![pt(20.0, 30.0), pt(120.0, 40.0), pt(90.0, 90.0), pt(25.0, 70.0)];
        s.push(("highlighter polygon".into(), o));
    }
    {
        let mut o = obj(ObjType::Text, 20.0, 20.0, 180.0, 120.0);
        o.text = w("Héllo wörld — 日本語 😀\r\nline two");
        o.name = "Title".into();
        o.hidden = true;
        o.floating = true;
        o.style.line_spacing = 1.35;
        o.style.align_h = 2;
        o.style.align_v = 1;
        o.style.padding = 7.5;
        o.style.underline = true;
        o.style.text_only = true;
        o.text_boxes = vec![rect(20.0, 20.0, 50.25, 12.0), rect(30.5, 40.0, 60.0, 14.75)];
        o.style.snap_text = true;
        o.style.vertical = true;
        o.style.text_on_path = true;
        s.push(("text".into(), o));
        s.push(("text empty".into(), obj(ObjType::Text, 5.0, 5.0, 6.0, 6.0)));
    }
    for c in [0, 13, 16, 22, 23] {
        let mut o = obj(ObjType::Callout, 40.0, 30.0, 160.0, 90.0);
        o.text = w("Callout");
        o.tail = pt(20.0, 130.0);
        o.style.callout_shape = c;
        o.style.shaded = c == 13;
        o.style.tail_color = 0xFF00AA00;
        o.style.tail_width = 4.5;
        o.style.bg_blur = c == 22;
        o.style.bg_blur_amount = 12;
        o.style.grad = grad(1, 0.0, &[(0.0, 0xFF000000), (1.0, 0xFFFFFFFF)]);
        s.push((format!("callout {c}"), o));
    }
    for i in 0..3 {
        let mut o = obj(ObjType::Step, 60.0, 60.0, 92.0, 92.0);
        o.step_value = 3 + i * 10;
        o.style.step_shape = i;
        o.style.step_type = i + 1;
        if i == 1 {
            o.style.step_outline = 0xFF123456;
            o.style.step_outline_width = 2.5;
        }
        s.push((format!("step {i}"), o));
    }
    {
        let mut o = obj(ObjType::Stamp, 50.0, 50.0, 98.0, 98.0);
        o.text = w("label:FF8800:Before");
        s.push(("stamp".into(), o));
    }
    for (i, &t) in [ObjType::Blur, ObjType::Pixelate, ObjType::Spotlight].iter().enumerate() {
        let i = i as i32;
        let mut o = obj(t, 30.0, 30.0, 150.0, 110.0);
        o.style.amount = 4 + i * 7;
        o.style.area_shape = match i {
            0 => SHAPE_ELLIPSE,
            1 => AREA_FREEHAND,
            _ => SHAPE_RECT,
        };
        if i == 1 {
            o.pts = vec![pt(30.0, 30.0), pt(150.0, 40.0), pt(140.0, 110.0), pt(40.0, 100.0), pt(35.0, 60.0)];
            o.style.hide_kind = 1;
        }
        s.push((format!("hide {i}"), o));
    }
    {
        let mut o = obj(ObjType::Magnify, 100.0, 20.0, 180.0, 100.0);
        o.tail = pt(40.0, 120.0);
        o.style.magnify_shape = 1;
        o.style.magnify_scale = 250;
        o.style.magnify_link = true;
        o.style.amount = 6;
        o.style.loupe = true;
        o.style.loupe_handle = false;
        s.push(("magnify".into(), o.clone()));
        o.style.loupe = false;
        s.push(("magnify plain".into(), o));
    }
    {
        let mut o = obj(ObjType::Picture, 10.0, 10.0, 40.0, 30.0);
        let mut pic = Image::sized(3, 2, 0);
        for i in 0..6u32 {
            pic.px[i as usize] = 0xFF000000u32 | i.wrapping_mul(0x2A1B0C);
        }
        o.picture = Some(pic);
        s.push(("picture".into(), o));
    }
    {
        let mut gr = obj(ObjType::Group, 0.0, 0.0, 0.0, 0.0);
        gr.children.push(s[0].1.clone());
        gr.children.push(s[14].1.clone());
        let mut inner = obj(ObjType::Group, 0.0, 0.0, 0.0, 0.0);
        inner.children.push(obj(ObjType::Line, 5.0, 5.0, 25.0, 45.0));
        inner.update_group_rect();
        gr.children.push(inner);
        gr.update_group_rect();
        gr.name = "Group 1".into();
        s.push(("group".into(), gr));
    }
    {
        let mut o = obj(ObjType::Link, 10.0, 10.0, 90.0, 30.0);
        o.text = w("https://example.com/a?b=1&c=é");
        s.push(("link".into(), o));
    }

    for (name, o) in &s {
        l.push(format!("xml {name} {}", xml_of(o)));
    }

    // read back: the XML of what was read, and what the XML does not show
    for (name, o) in &s {
        let r = objects_from_clip(objects_to_clip(std::slice::from_ref(o), "").as_bytes()).0;
        l.push(format!("read {name} {}", r.len()));
        if let Some(rr) = r.first() {
            l.push(format!("read {name} {} {}", extra(rr), xml_of(rr)));
        }
    }

    // hand-written and older files
    let hand = [
        r#"<Rect X1="10" Y1="20" X2="5" Y2="7"/>"#,
        r##"<Ellipse X1="1" Y1="2" X2="30" Y2="40" PenColor="#123456"/>"##,
        r#"<RoundRect/>"#,
        r#"<Highlight X1="1" Y1="1" X2="50" Y2="9" AreaShape="99"/>"#,
        r#"<Callout CalloutShape="2"/>"#,
        r#"<Callout CalloutShape="9"/>"#,
        r#"<Callout CalloutShape="-1" CalloutKind="30"/>"#,
        r#"<Callout CalloutKind="17" TailX="3.5" TailY="-4"/>"#,
        r#"<Pixelate Amount="5"/>"#,
        r#"<Link X1="0" Url=""/>"#,
        r#"<Link X1="4" Y1="5" X2="6" Y2="7" Url="http://x"/>"#,
        r#"<Group/>"#,
        r#"<Group><Bogus/></Group>"#,
        r#"<Group Name="g"><Arrow X1="1" Y1="2" X2="30" Y2="40"/><Text X1="0" Y1="0" X2="10" Y2="10" Text="t"/></Group>"#,
        r#"<Bogus X1="1"/>"#,
        r#"<Image StreamName="missing.png"/>"#,
        r#"<Image/>"#,
        r##"<Arrow PenColor="#1" FillColor="zz" TextColor="#0x7FABCDEF" ShadowColor="#FFFFFFFFFF" TailColor="#-1"/>"##,
        r##"<Arrow PenColor="#abcdeg" FillColor="#" TextColor="# 12" ShadowColor="#+0000FF"/>"##,
        r#"<Pen Points="1,2 3,4 junk 5,6" Pressure="0.5 1.5 -1 x 0.25"/>"#,
        r#"<Pen Points=" 1,2" Pressure=" 0.5"/>"#,
        r#"<Pen Points="1 ,2 3,4"/>"#,
        "<Pen Points=\"1, 2   3,\t4\"/>",
        r#"<Pen Points="1,2"/>"#,
        r#"<Pen Points="0x10,2 1e2,-3.5e-1 .5,5. +7,-0 "/>"#,
        r#"<Pen Points="1.25e,3 4,5"/>"#,
        r#"<Pen Points="inf,1 2,3"/>"#,
        r#"<Text TextBoxes="1,2,3,4 5,6,7 8" LineSpacing="9" AlignH="-3" AlignV="7" Bold="0" TextOnly="1"/>"#,
        r#"<Text TextBoxes="1,2,3,4 5,6,7,8" LineSpacing="0.1"/>"#,
        r##"<Shape FillGradient="1" GradientAngle="45.5" GradientStops="1:#FF0000FF;0:#00FF00;0.5:bad;2:#FFFFFFFF;x" ToolShape="99"/>"##,
        r##"<Shape FillGradient="2" GradientStops="0:#FF000000"/>"##,
        r##"<Shape FillGradient="7" GradientStops=";;0.3:#FF0000FF;-1:#FF00FF00;"/>"##,
        r##"<Shape FillGradient="-2" GradientAngle="abc" GradientStops="0.25:#FF0000FF;0.25:#FF00FF00;0.1:"/>"##,
        r#"<Magnify Amount="3"/>"#,
        r#"<Magnify Amount="40" MagnifyShape="12" LoupeHandle="0"/>"#,
        r#"<Magnify MagnifyScale="5000"/>"#,
        r#"<Step StepValue="-4" StepStyle="7" StepSequenceType="-1" StepOutlineWidth="99"/>"#,
        r#"<Arrow X1=" 12.5abc" Y1="+7x" X2="1e2" Y2=".5" PenWidth="-.e3" Opacity=" -12 " ShadowDir="12" Shadow="0"/>"#,
        r#"<Arrow X1="0x1A" Y1="inf" X2="1e400" Y2="-0" PenWidth="0x1p3" Opacity="99999999999"/>"#,
        r#"<Arrow ArrowStartCap="20" ArrowEndCap="-5" DashType="9" Bezier="2" StartSize="3.25"/>"#,
        r#"<Arrow X1="10" Y1="20" X2="30" Y2="40" Bezier="1"/>"#,
        r#"<Text FaceName="Consolas" FontSize="0" Italic="1" Underline="1" TextPadding="-2" Text="a&amp;b&#10;c"/>"#,
        r#"<Callout BackgroundBlur="1" BackgroundBlurAmount="99" Shaded="1"/>"#,
        r#"<Callout BackgroundBlur="1" BackgroundBlurAmount="0"/>"#,
        r#"<Blur AreaShape="9" Points="1,1 50,1 25,40 "/>"#,
        r#"<Spotlight Amount="-3"/>"#,
        r#"<Stamp Text="cursor:arrow" Hidden="1" Locked="5" Floating="0" Name="s"/>"#,
        r#"<Text Sketch="1" SketchSeed="77" VerticalText="1" TextOnPath="1" SnapText="1"/>"#,
    ];
    for h in hand {
        l.push(match read_xml(h) {
            Some(r) => format!("hand {h} -> {} {}", extra(&r), xml_of(&r)),
            None => format!("hand {h} -> null"),
        });
    }

    // gradient stops as the quick styles store them
    for gd in [Gradient::default(), grad(1, 0.0, &[(0.0, 0xFF000000)]), grad(1, 0.0, &[(0.0005, 0x01020304), (0.9995, 0xFFFFFFFF), (1.0, 0)]),
               grad(2, -45.0, &[(0.3333333, 0xFFABCDEF), (0.6666667, 0x00000000)])]
    {
        let mut n = Node::new("G");
        grad_to_xml(&mut n, &gd);
        let back = grad_from_xml(&n);
        l.push(format!("grad [{}] {} back={}", grad_stops_to_string(&gd), one_line(&xml::serialize(&n, false)), (back == gd) as i32));
    }

    // hit-testing on a grid, at two tolerances
    for (name, o) in &s {
        for tol in [0.0f32, 4.0] {
            let mut bits = String::new();
            let mut hits = 0;
            for y in (-10..160).step_by(5) {
                for x in (-10..210).step_by(5) {
                    let h = o.hit_test(pt(x as f32 + 0.5, y as f32 + 0.25), tol);
                    bits.push(if h { '1' } else { '0' });
                    hits += h as i32;
                }
            }
            l.push(format!("hit {name} {} {hits} {:016x}", g(tol as f64), fnv1a(bits.as_bytes())));
        }
    }

    // the clipboard: written, read back; and texts that are not objects
    {
        let objs: Vec<DrawObj> = s.iter().map(|(_, o)| o.clone()).collect();
        let clip = objects_to_clip(&objs, "C:\\Docs\\é 1.snap");
        l.push(format!("clip {} {:016x}", clip.len(), fnv1a(clip.as_bytes())));
        let (back, src) = objects_from_clip(clip.as_bytes());
        let all: String = back.iter().map(|o| extra(o) + &xml_of(o) + "\n").collect();
        l.push(format!("clip back {} {} {:016x}", back.len(), src.unwrap_or_default(), fnv1a(all.as_bytes())));
        let bad = [
            "",
            "<Other/>",
            r#"<SnapnabObjects><Objects><Arrow X1="1"/><Nope/></Objects></SnapnabObjects>"#,
            r#"<SnapnabObjects Source="s"><Objects><Image StreamName="Image_1.png"/></Objects><Embedded Name="Image_1.png" Data="!!!"/></SnapnabObjects>"#,
            r#"<SnapnabObjects Source="x">"#,
        ];
        for b in bad {
            let (r, src) = objects_from_clip(b.as_bytes());
            let x: String = r.iter().map(|o| xml_of(o) + ";").collect();
            l.push(format!("clip bad {} [{}] {x}", r.len(), src.unwrap_or_else(|| "unset".into())));
        }
    }

    // a document saved as .snap and read back
    {
        let file = tmp("docgolden.snap");
        let mut d = Document::default();
        let mut base = Image::sized(40, 30, 0);
        for i in 0..40 * 30u32 {
            base.px[i as usize] = 0xFF000000u32 | (i.wrapping_mul(2654435761) >> 8);
        }
        d.set_base(base);
        d.path = file.clone();
        d.capture_mode = "Região".into();
        d.window_title = "Window — “title”".into();
        d.application = "app.exe".into();
        d.created = "2026-01-02T03:04:05".into();
        d.tags = "a,b,ç".into();
        d.st.next_step = 7;
        d.guides_x = vec![10.5, 20.0];
        d.guides_y = vec![0.125];
        for (_, o) in &s {
            d.st.objs.push(Arc::new(o.clone()));
        }
        let saved = d.save();
        let doc = snapcore::snap::load_document(Path::new(&file));
        let dx = doc.as_ref().map(|(n, _)| xml::serialize(n, false)).unwrap_or_default();
        l.push(format!("doc saved {} {} {} {:016x}", saved as i32, doc.is_some() as i32, dx.len(), fnv1a(dx.as_bytes())));
        match Document::load(&file) {
            Some(r) => {
                let all: String = r.st.objs.iter().map(|o| extra(o) + &xml_of(o) + "\n").collect();
                let mut gs = String::new();
                for x in &r.guides_x {
                    gs += &format!("x{} ", f3(*x));
                }
                for y in &r.guides_y {
                    gs += &format!("y{} ", f3(*y));
                }
                l.push(format!("doc loaded {}|{}|{}|{}|{}|{}|{gs}|{}x{} {:016x}|{} {:016x}", r.capture_mode, r.window_title, r.application,
                               r.created, r.tags, r.st.next_step, r.w(), r.h(), hash_img(&r.st.base), r.st.objs.len(),
                               fnv1a(all.as_bytes())));
                l.push(format!("doc title {}", r.title));
            }
            None => l.push("doc loaded null".into()),
        }
        let mut e = Document::default();
        e.set_base(Image::sized(4, 3, 0xFF00FF00));
        e.path = tmp("docgolden2.snap");
        let s2 = e.save();
        let r2 = Document::load(&e.path);
        l.push(format!("doc empty {} {} {} {} {}", s2 as i32, r2.as_ref().map(|r| r.title.clone()).unwrap_or("null".into()),
                       r2.as_ref().map(|r| r.st.next_step).unwrap_or(-1), r2.as_ref().map(|r| r.st.objs.len()).unwrap_or(0),
                       r2.as_ref().map(|r| r.guides_x.len() + r.guides_y.len()).unwrap_or(9)));
        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_file(&e.path);
        l.push(format!("doc missing {}", Document::load(&tmp("docgolden_missing.snap")).is_some() as i32));
    }
    compare("doc-cpp.txt", &l);
}
