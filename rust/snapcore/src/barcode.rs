//! QR code and barcode reading (C04, Barcode.cpp) with rxing, a Rust port of ZXing: tryHarder, inverted codes and
//! several codes in one picture. The results dialog stays with the interface.

use std::collections::HashSet;

use rxing::{BarcodeFormat, DecodeHints, RXingResult};

use crate::gdip::scale_image;
use crate::image::{Image, Rect, blend_over, ca, cb, cg, cr};

#[derive(Debug, Clone, PartialEq)]
pub struct Code {
    pub text: String,
    pub format: &'static str, // the symbology as ZXing-C++ named it ("QRCode", "EAN-13"…)
    pub rect: Rect,           // where it is in the image (around the points the reader found)
    pub url: bool,            // http(s) link: can be opened
}

fn format_name(f: &BarcodeFormat) -> &'static str {
    match f {
        BarcodeFormat::AZTEC => "Aztec",
        BarcodeFormat::CODABAR => "Codabar",
        BarcodeFormat::CODE_39 => "Code39",
        BarcodeFormat::CODE_93 => "Code93",
        BarcodeFormat::CODE_128 => "Code128",
        BarcodeFormat::DATA_MATRIX => "DataMatrix",
        BarcodeFormat::EAN_8 => "EAN-8",
        BarcodeFormat::EAN_13 => "EAN-13",
        BarcodeFormat::ITF => "ITF",
        BarcodeFormat::MAXICODE => "MaxiCode",
        BarcodeFormat::PDF_417 => "PDF417",
        BarcodeFormat::QR_CODE => "QRCode",
        BarcodeFormat::MICRO_QR_CODE => "MicroQRCode",
        BarcodeFormat::RECTANGULAR_MICRO_QR_CODE => "rMQRCode",
        BarcodeFormat::RSS_14 => "DataBar",
        BarcodeFormat::RSS_EXPANDED => "DataBarExpanded",
        BarcodeFormat::UPC_A => "UPC-A",
        BarcodeFormat::UPC_E => "UPC-E",
        _ => "Unknown",
    }
}

/// ZXing's luminance of a BGRX pixel.
fn luma(p: u32) -> u8 {
    ((306 * cr(p) + 601 * cg(p) + 117 * cb(p) + 0x200) >> 10) as u8
}

fn decode(img: &Image) -> Vec<RXingResult> {
    if img.is_empty() {
        return Vec::new();
    }
    let lum: Vec<u8> = img.px.iter().map(|&p| luma(p)).collect();
    let mut hints = DecodeHints { TryHarder: Some(true), AlsoInverted: Some(true), ..Default::default() };
    rxing::helpers::detect_multiple_in_luma_with_hints(lum, img.w, img.h, &mut hints).unwrap_or_default()
}

pub fn read(src: &Image) -> Vec<Code> {
    let mut out: Vec<Code> = Vec::new();
    if src.is_empty() {
        return out;
    }
    // transparent pixels read as white (codes are dark on light)
    let mut img = src.clone();
    for p in img.px.iter_mut() {
        if ca(*p) < 255 {
            *p = blend_over(0xFFFFFFFF, *p);
        }
    }
    let mut seen = HashSet::new();
    let mut take = |im: &Image, k: f64, out: &mut Vec<Code>| {
        for r in decode(im) {
            let pts = r.getPoints();
            let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for p in pts {
                x0 = x0.min(p.x);
                y0 = y0.min(p.y);
                x1 = x1.max(p.x);
                y1 = y1.max(p.y);
            }
            let rect = if pts.is_empty() {
                Rect::default()
            } else {
                Rect::new((x0 as f64 / k) as i32, (y0 as f64 / k) as i32, (x1 as f64 / k) as i32 + 1, (y1 as f64 / k) as i32 + 1)
            };
            let text = r.getText().to_string();
            let format = format_name(r.getBarcodeFormat());
            let low = text.to_lowercase();
            let url = low.starts_with("http://") || low.starts_with("https://");
            if seen.insert((text.clone(), format)) {
                out.push(Code { text, format, rect, url });
            }
        }
    };
    take(&img, 1.0, &mut out);
    // small codes on a screen capture (a few pixels per module) read better enlarged
    if out.is_empty() && (img.w as u64 * img.h as u64) < 1500 * 1500 {
        let big = scale_image(&img, img.wi() * 2, img.hi() * 2, false);
        take(&big, 2.0, &mut out);
    }
    // in reading order: top to bottom, then left to right
    out.sort_by_key(|c| (c.rect.top, c.rect.left));
    out
}
