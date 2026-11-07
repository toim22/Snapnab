//! The .snap container: a 24-byte header followed by an OLE compound file (see src/common/SnapFile.h), read and written
//! with the `cfb` crate instead of the Windows structured storage API (StgOpenStorageOnILockBytes), so a damaged or
//! hostile file is parsed by memory-safe code. What goes inside the streams (the XML documents and the PNG images) is
//! still handled by the C++ side. Stream paths are UTF-8 with '/' between storages, e.g. "Frame0/Document" or
//! "Frame0/ImageStorage/Image_0.png".

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

use cfb::{CompoundFile, Version};

/// 0xCB + "SNAPNAB" + 16-byte format identifier (same bytes as kHeader in SnapFile.cpp)
pub const HEADER: [u8; 24] = [
    0xCB, b'S', b'N', b'A', b'P', b'N', b'A', b'B', 0x5A, 0x3C, 0x91, 0x0E, 0x6B, 0x2D, 0x4F, 0x18, 0xA7, 0x44, 0xC2, 0x9B,
    0x10, 0x7E, 0xD3, 0x55,
];

/// Largest stream read into memory: a bigger one is treated as a damaged file (a capture of 16k × 16k pixels as PNG is
/// far below this).
const MAX_STREAM: u64 = 1 << 30;

/// The file seen from after the header, which is what the compound file reader and writer expect.
struct Offset<F> {
    inner: F,
}

impl<F: Seek> Offset<F> {
    fn new(mut inner: F) -> io::Result<Self> {
        inner.seek(SeekFrom::Start(HEADER.len() as u64))?;
        Ok(Offset { inner })
    }
}

impl<F: Read> Read for Offset<F> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}

impl<F: Write> Write for Offset<F> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl<F: Seek> Seek for Offset<F> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let off = HEADER.len() as u64;
        let pos = match pos {
            SeekFrom::Start(p) => SeekFrom::Start(p.checked_add(off).ok_or(io::ErrorKind::InvalidInput)?),
            other => other,
        };
        let p = self.inner.seek(pos)?;
        p.checked_sub(off).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek before the .snap header"))
    }
}

// ---------------------------------------------------------------- safe API (also used by the Rust tests)

pub struct Reader {
    cf: CompoundFile<Offset<File>>,
}

impl Reader {
    pub fn open(path: &std::path::Path) -> io::Result<Reader> {
        let mut f = File::open(path)?;
        let mut h = [0u8; 24];
        f.read_exact(&mut h)?;
        if h != HEADER {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "not a .snap file"));
        }
        Ok(Reader { cf: CompoundFile::open(Offset::new(f)?)? })
    }

    pub fn read(&mut self, stream: &str) -> io::Result<Vec<u8>> {
        let mut s = self.cf.open_stream(stream_path(stream))?;
        let len = s.len();
        if len > MAX_STREAM {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "stream too large"));
        }
        let mut out = Vec::with_capacity(len as usize);
        s.read_to_end(&mut out)?;
        Ok(out)
    }
}

pub struct Writer {
    cf: CompoundFile<Offset<File>>,
}

impl Writer {
    /// Creates (or truncates) the file. V3 (512-byte sectors), like the Windows API writes them, so older Snapnab
    /// versions open the result too.
    pub fn create(path: &std::path::Path) -> io::Result<Writer> {
        let mut f = File::create(path)?;
        f.write_all(&HEADER)?;
        Ok(Writer { cf: CompoundFile::create_with_version(Version::V3, Offset::new(f)?)? })
    }

    /// Writes one stream, creating the storages on its path as needed.
    pub fn write(&mut self, stream: &str, data: &[u8]) -> io::Result<()> {
        let path = stream_path(stream);
        let mut dir = PathBuf::from("/");
        if let Some(parent) = path.parent() {
            for c in parent.components().skip(1) {
                dir.push(c);
                if !self.cf.exists(&dir) {
                    self.cf.create_storage(&dir)?;
                }
            }
        }
        let mut s = self.cf.create_stream(&path)?;
        s.write_all(data)?;
        s.flush()
    }

    /// Writes everything out and closes the file.
    pub fn finish(mut self) -> io::Result<()> {
        self.cf.flush()?;
        let f = self.cf.into_inner().inner;
        f.sync_data()
    }
}

fn stream_path(stream: &str) -> PathBuf {
    let mut p = PathBuf::from("/");
    for part in stream.split('/').filter(|s| !s.is_empty()) {
        p.push(part);
    }
    p
}

pub fn is_snap(path: &std::path::Path) -> bool {
    let mut h = [0u8; 24];
    File::open(path).and_then(|mut f| f.read_exact(&mut h)).is_ok() && h == HEADER
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("snapcore-test-{}-{name}", std::process::id()))
    }

    #[test]
    fn round_trip() {
        let p = tmp("rt.snap");
        let mut w = Writer::create(&p).unwrap();
        w.write("Index", b"<Index/>").unwrap();
        w.write("Frame0/Document", b"<Document/>").unwrap();
        w.write("Frame0/ImageStorage/Image_0.png", &vec![7u8; 100_000]).unwrap();
        w.write("Frame0/ImageStorage/Image_1.png", b"").unwrap();
        w.finish().unwrap();
        assert!(is_snap(&p));
        let mut r = Reader::open(&p).unwrap();
        assert_eq!(r.read("Index").unwrap(), b"<Index/>");
        assert_eq!(r.read("/Frame0/Document").unwrap(), b"<Document/>");
        assert_eq!(r.read("Frame0/ImageStorage/Image_0.png").unwrap(), vec![7u8; 100_000]);
        assert!(r.read("Frame0/ImageStorage/Image_1.png").unwrap().is_empty());
        assert!(r.read("Frame1/Document").is_err());
        drop(r);
        std::fs::remove_file(&p).unwrap();
    }

    #[test]
    fn rejects_damaged_files() {
        let p = tmp("bad.snap");
        std::fs::write(&p, b"not a snap file at all").unwrap();
        assert!(!is_snap(&p) && Reader::open(&p).is_err());
        let mut junk = HEADER.to_vec();
        junk.extend((0..4096u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8));
        std::fs::write(&p, &junk).unwrap();
        assert!(is_snap(&p) && Reader::open(&p).is_err());       // header right, container garbage: an error, no crash
        std::fs::remove_file(&p).unwrap();
    }
}
