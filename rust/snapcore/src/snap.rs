//! The .snap document (ed_comp_2 §3): the 24-byte header and compound file of `container`, holding
//!
//! - `Index`: XML `<Index SoftwareVersion=…><FrameList><Frame Name="Frame0"/>…</FrameList></Index>`
//! - `Frame%d/Document`: XML of the frame (vector objects, properties, `BaseImage`, `Width`, `Height`, `Metadata`)
//! - `Frame%d/ImageStorage/…`: PNG streams, the base image (`Image_0.png`) and the images the objects embed
//!   (by their `StreamName`)
//!
//! A port of SaveSnapFile / LoadSnapFile / LoadSnapDocument in SnapFile.cpp, writing the streams in the same order.

use std::collections::BTreeMap;
use std::path::Path;

use crate::codec::{self, Format, SaveOptions};
use crate::container::{Reader, Writer, is_snap};
use crate::image::{Image, Meta};
use crate::xml::{self, Node};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Frame {
    pub base: Image,                         // background image (Image_0.png)
    pub doc: Node,                           // objects and metadata (the <Document> element)
    pub embedded: BTreeMap<String, Image>,   // images referenced by the objects (StreamName → image)
}

impl Frame {
    pub fn new(base: Image) -> Self {
        Frame { base, doc: Node::new("Document"), embedded: BTreeMap::new() }
    }
}

// ---------------------------------------------------------------- attribute values as XmlNode reads them

/// atoi: leading white space, a sign, digits (saturated).
pub fn atoi(s: &str) -> i32 {
    let t = s.trim_start_matches([' ', '\t', '\n', '\r', '\x0B', '\x0C']);
    let (neg, digits) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let mut v: i64 = 0;
    for c in digits.bytes().take_while(u8::is_ascii_digit) {
        v = (v * 10 + (c - b'0') as i64).min(i32::MAX as i64 + 1);
    }
    let v = if neg { -v } else { v };
    v.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

/// atof: the longest prefix that reads as a decimal number (0 if none).
pub fn atof(s: &str) -> f64 {
    let t = s.trim_start_matches([' ', '\t', '\n', '\r', '\x0B', '\x0C']).as_bytes();
    let mut i = 0;
    if i < t.len() && (t[i] == b'+' || t[i] == b'-') {
        i += 1;
    }
    let int_start = i;
    while i < t.len() && t[i].is_ascii_digit() {
        i += 1;
    }
    let mut digits = i - int_start;
    if i < t.len() && t[i] == b'.' {
        let frac = i + 1;
        let mut j = frac;
        while j < t.len() && t[j].is_ascii_digit() {
            j += 1;
        }
        digits += j - frac;
        if digits > 0 {
            i = j;
        }
    }
    if digits == 0 {
        return 0.0;
    }
    if i < t.len() && (t[i] | 0x20) == b'e' {
        let mut j = i + 1;
        if j < t.len() && (t[j] == b'+' || t[j] == b'-') {
            j += 1;
        }
        let exp_start = j;
        while j < t.len() && t[j].is_ascii_digit() {
            j += 1;
        }
        if j > exp_start {
            i = j;
        }
    }
    std::str::from_utf8(&t[..i]).ok().and_then(|n| n.parse().ok()).unwrap_or(0.0)
}

/// setd of XmlNode: "%.4f".
pub fn fixed4(v: f64) -> String {
    crate::crt::fixed(v, 4)
}

// ---------------------------------------------------------------- writing

fn png(img: &Image) -> Option<Vec<u8>> {
    // internal copies carry no metadata (EncodeToMemory)
    codec::encode(img, Format::Png, &SaveOptions { strip_metadata: true, ..Default::default() }, "").ok()
}

fn write_all(w: &mut Writer, frames: &[Frame], version: &str) -> bool {
    let mut index = Node::new("Index");
    index.add("FrameList");
    index.set("SoftwareVersion", version);
    for (i, f) in frames.iter().enumerate() {
        let fname = format!("Frame{i}");
        index.child_mut("FrameList").unwrap().add("Frame").set("Name", fname.as_str());
        let mut doc = f.doc.clone();
        doc.set("BaseImage", "Image_0.png");
        // metadata of an imported photo (C29), written only when there is some
        if let Some(m) = f.base.meta.as_ref().filter(|_| doc.child("Metadata").is_none()) {
            let md = doc.add("Metadata");
            md.set("DateTaken", m.date_taken.as_str())
                .set("Make", m.make.as_str())
                .set("Model", m.model.as_str())
                .set("Software", m.software.as_str());
            if m.gps {
                md.set("Gps", "1").set("Latitude", fixed4(m.lat)).set("Longitude", fixed4(m.lon));
            }
        }
        doc.set("Width", f.base.w.to_string()).set("Height", f.base.h.to_string());
        let ok = w.write(&format!("{fname}/Document"), xml::serialize(&doc, true).as_bytes()).is_ok()
            && png(&f.base).is_some_and(|p| w.write(&format!("{fname}/ImageStorage/Image_0.png"), &p).is_ok())
            && f.embedded.iter().all(|(name, img)| {
                png(img).is_some_and(|p| w.write(&format!("{fname}/ImageStorage/{name}"), &p).is_ok())
            });
        if !ok {
            return false;
        }
    }
    w.write("Index", xml::serialize(&index, true).as_bytes()).is_ok()
}

/// Writes the frames to `path` (through `<path>.saving`, put in place at the end). `version` is the program's.
pub fn save(path: &Path, frames: &[Frame], version: &str) -> bool {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".saving");
    let tmp = std::path::PathBuf::from(tmp);
    let ok = {
        let mut w = match Writer::create(&tmp) {
            Ok(w) => w,
            Err(_) => {
                crate::log::write(&format!("SaveSnapFile: could not open {}", tmp.display()));
                return false;
            }
        };
        write_all(&mut w, frames, version) && w.finish().is_ok()
    }; // the file is closed here, an unfinished one too
    if !ok {
        let _ = std::fs::remove_file(&tmp);
        crate::log::write(&format!("SaveSnapFile failed: {}", path.display()));
        return false;
    }
    std::fs::rename(&tmp, path).is_ok()
}

// ---------------------------------------------------------------- reading

fn read_png(r: &mut Reader, stream: &str) -> Option<Image> {
    codec::decode(&r.read(stream).ok()?).ok()
}

/// Embedded images referenced by StreamName at any level of the document.
fn read_embedded(r: &mut Reader, n: &Node, images: &str, out: &mut BTreeMap<String, Image>) {
    if let Some(sn) = n.attr("StreamName").filter(|s| !s.is_empty() && !out.contains_key(*s)) {
        if let Some(img) = read_png(r, &format!("{images}{sn}")) {
            out.insert(sn.to_owned(), img);
        }
    }
    for c in &n.children {
        read_embedded(r, c, images, out);
    }
}

fn read_all(path: &Path) -> Option<Vec<Frame>> {
    let mut r = Reader::open(path).ok()?;
    let index = xml::parse(&r.read("Index").ok()?)?;
    let mut frames = Vec::new();
    if let Some(list) = index.child("FrameList") {
        for fr in &list.children {
            let name = fr.attr("Name").unwrap_or("");
            let data = r.read(&format!("{name}/Document")).ok()?; // a frame without its document
            let doc = xml::parse_partial(&data).0;
            let images = format!("{name}/ImageStorage/");
            let mut base = read_png(&mut r, &format!("{images}{}", doc.attr("BaseImage").unwrap_or("Image_0.png")))
                .unwrap_or_default();
            if let Some(md) = doc.child("Metadata") {
                let text = |k: &str| md.attr(k).unwrap_or("").to_owned();
                let m = Meta {
                    date_taken: text("DateTaken"),
                    make: text("Make"),
                    model: text("Model"),
                    software: text("Software"),
                    gps: atoi(md.attr("Gps").unwrap_or("0")) != 0,
                    lat: md.attr("Latitude").map_or(0.0, atof),
                    lon: md.attr("Longitude").map_or(0.0, atof),
                };
                if !m.is_empty() {
                    base.meta = Some(m);
                }
            }
            let mut embedded = BTreeMap::new();
            read_embedded(&mut r, &doc, &images, &mut embedded);
            frames.push(Frame { base, doc, embedded });
        }
    }
    (!frames.is_empty() && !frames[0].base.is_empty()).then_some(frames)
}

/// Reads every frame; None if the file is not a .snap (not logged), is damaged or has no base image.
pub fn load(path: &Path) -> Option<Vec<Frame>> {
    if !is_snap(path) {
        return None;
    }
    let frames = read_all(path);
    if frames.is_none() {
        crate::log::write(&format!("LoadSnapFile failed: {}", path.display()));
    }
    frames
}

/// Only the first frame's document (no images): tags, capture mode… The bool is false if the XML is malformed (the
/// partial tree comes with it); None if the stream cannot be read.
pub fn load_document(path: &Path) -> Option<(Node, bool)> {
    if !is_snap(path) {
        return None;
    }
    let data = Reader::open(path).ok()?.read("Frame0/Document").ok()?;
    Some(xml::parse_partial(&data))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pattern(w: u32, h: u32, seed: u32) -> Image {
        let mut img = Image::new(w, h, 0);
        for (i, p) in img.px.iter_mut().enumerate() {
            let v = (i as u32).wrapping_mul(2654435761).wrapping_add(seed * 977);
            *p = 0xFF00_0000 | (v & 0xFFFFFF);
        }
        img
    }

    fn temp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("snapcore-snap-{}-{name}", std::process::id()))
    }

    #[test]
    fn round_trip() {
        let mut f = Frame::new(pattern(321, 123, 9));
        f.base.dpi = 96.0;
        f.doc.set("CaptureMode", "Região ç → 中文");
        let objs = f.doc.add("Objects");
        objs.add("Arrow").set("X1", "1.5000").set("PenColor", "#FFE5322D");
        objs.add("Image").set("StreamName", "Image_1.png");
        objs.add("Group").add("Image").set("StreamName", "Image_2.png");
        f.embedded.insert("Image_1.png".into(), pattern(10, 10, 1));
        f.embedded.insert("Image_2.png".into(), pattern(7, 3, 2));
        let mut second = Frame::new(pattern(20, 30, 3));
        second.base.meta = Some(Meta {
            date_taken: "2024:05:06 07:08:09".into(),
            make: "Maker".into(),
            gps: true,
            lat: -23.55052,
            lon: 46.6333,
            ..Default::default()
        });
        let p = temp("round.snap");
        assert!(save(&p, &[f.clone(), second.clone()], "9.9.9"));
        assert!(is_snap(&p));
        let mut tmp = p.clone().into_os_string();
        tmp.push(".saving");
        assert!(!Path::new(&tmp).exists());

        let back = load(&p).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].base.px, f.base.px);
        assert_eq!((back[0].base.w, back[0].base.h), (321, 123));
        assert_eq!(back[0].doc.attr("CaptureMode"), Some("Região ç → 中文"));
        assert_eq!(back[0].doc.attr("BaseImage"), Some("Image_0.png"));
        assert_eq!(back[0].doc.attr("Width"), Some("321"));
        assert_eq!(back[0].embedded.keys().collect::<Vec<_>>(), ["Image_1.png", "Image_2.png"]);
        assert_eq!(back[0].embedded["Image_2.png"].px, f.embedded["Image_2.png"].px);
        let md = back[1].doc.child("Metadata").unwrap();
        assert_eq!((md.attr("Latitude"), md.attr("Longitude"), md.attr("Gps")), (Some("-23.5505"), Some("46.6333"), Some("1")));
        let m = back[1].base.meta.as_ref().unwrap();
        assert_eq!((m.date_taken.as_str(), m.make.as_str(), m.gps), ("2024:05:06 07:08:09", "Maker", true));
        assert_eq!((m.lat, m.lon), (-23.5505, 46.6333));

        let (doc, ok) = load_document(&p).unwrap();
        assert!(ok);
        assert_eq!(doc.attr("CaptureMode"), Some("Região ç → 中文"));
        let r = Reader::open(&p).unwrap().read("Index").unwrap();
        let index = xml::parse(&r).unwrap();
        assert_eq!(index.attr("SoftwareVersion"), Some("9.9.9"));
        assert_eq!(index.child("FrameList").unwrap().children.len(), 2);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn refusals() {
        let p = temp("bad.snap");
        std::fs::write(&p, b"not a snap file").unwrap();
        assert!(load(&p).is_none());
        assert!(load_document(&p).is_none());
        assert!(load(&temp("missing.snap")).is_none());
        // a frame list without frames: no base image
        let mut w = Writer::create(&p).unwrap();
        w.write("Index", b"<Index><FrameList/></Index>").unwrap();
        w.finish().unwrap();
        assert!(load(&p).is_none());
        let _ = std::fs::remove_file(&p);
        // an empty image cannot be written
        assert!(!save(&p, &[Frame::new(Image::default())], "1"));
        assert!(!p.exists());
    }

    #[test]
    fn attribute_numbers() {
        assert_eq!((atoi(" 42x"), atoi("-7"), atoi("+3"), atoi("abc"), atoi("99999999999")), (42, -7, 3, 0, i32::MAX));
        assert_eq!((atof("1.5e2xyz"), atof(" -0.25"), atof(".5"), atof("5."), atof("e3"), atof("1e")), (150.0, -0.25, 0.5, 5.0, 0.0, 1.0));
        assert_eq!((fixed4(1.5), fixed4(-23.55052), fixed4(2.0)), ("1.5000".into(), "-23.5505".into(), "2.0000".into()));
    }
}
