//! The webcam of the recordings (Webcam.cpp, CCameraSource, comp_3 §8): the cameras Media Foundation lists, a reader
//! thread that keeps the latest frame (IMFSourceReader converting any format to RGB32), and the chroma key that takes
//! out a green (or any color) background. The window that shows it lives in the app.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use windows::Win32::Foundation::E_ACCESSDENIED;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize};
use windows::core::{GUID, Interface, PCWSTR, PWSTR};

use crate::image::Image;
use crate::{i18n, log};

const FIRST_VIDEO_STREAM: u32 = 0xFFFF_FFFC; // MF_SOURCE_READER_FIRST_VIDEO_STREAM

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CameraInfo {
    pub name: String,
    pub link: String,
}

fn attr_string(a: &IMFActivate, key: &GUID) -> String {
    let (mut s, mut n) = (PWSTR::null(), 0u32);
    unsafe {
        if a.GetAllocatedString(key, &mut s, &mut n).is_ok() && !s.is_null() {
            let r = s.to_string().unwrap_or_default();
            CoTaskMemFree(Some(s.0 as _));
            return r;
        }
    }
    String::new()
}

fn video_devices() -> windows::core::Result<Vec<IMFActivate>> {
    unsafe {
        let mut attr = None;
        MFCreateAttributes(&mut attr, 1)?;
        let attr = attr.ok_or(windows::core::Error::empty())?;
        attr.SetGUID(&MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID)?;
        let (mut dev, mut n) = (std::ptr::null_mut(), 0u32);
        MFEnumDeviceSources(&attr, &mut dev, &mut n)?;
        let mut out = Vec::new();
        for i in 0..n as usize {
            if let Some(a) = (*dev.add(i)).take() {
                out.push(a);
            }
        }
        CoTaskMemFree(Some(dev as _));
        Ok(out)
    }
}

/// ListCameras: name and symbolic link of each camera.
pub fn list_cameras() -> Vec<CameraInfo> {
    unsafe {
        if MFStartup(MF_VERSION, MFSTARTUP_LITE).is_err() {
            return Vec::new();
        }
    }
    let out = video_devices()
        .unwrap_or_default()
        .iter()
        .map(|a| CameraInfo {
            name: attr_string(a, &MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME),
            link: attr_string(a, &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK),
        })
        .collect();
    unsafe {
        let _ = MFShutdown();
    }
    out
}

/// The webcam shapes (CamShape).
pub const SHAPE_NAMES: [&str; 5] = ["Círculo", "Quadrado arredondado", "Quadrado", "Retângulo 16:9", "Retângulo 4:3"];

/// ChromaAlpha: the chrominance distance (Cb/Cr, BT.601) of a BGRA pixel to the key color; luminance does not count,
/// so shadows and folds in the background are still background. Below t0 it disappears, above t0 + band it stays.
pub fn chroma_alpha(px: u32, key: u32, tolerance: i32) -> u8 {
    let cbcr = |r: f64, g: f64, b: f64| (-0.168736 * r - 0.331264 * g + 0.5 * b, 0.5 * r - 0.418688 * g - 0.081312 * b);
    // key: COLORREF (0x00BBGGRR)
    let (kcb, kcr) = cbcr((key & 255) as f64, ((key >> 8) & 255) as f64, ((key >> 16) & 255) as f64);
    let (cb, cr) = cbcr(((px >> 16) & 255) as f64, ((px >> 8) & 255) as f64, (px & 255) as f64);
    let d = (cb - kcb).hypot(cr - kcr);
    let (t0, band) = (8.0 + tolerance.clamp(0, 100) as f64 * 0.6, 18.0);
    (((d - t0) / band).clamp(0.0, 1.0) * 255.0).round() as u8
}

/// A running camera: its reader thread keeps the latest frame.
pub struct Camera {
    running: Arc<AtomicBool>,
    frame: Arc<Mutex<Image>>,
    thread: Option<std::thread::JoinHandle<()>>,
    /// width / height of the camera image
    pub aspect: f64,
}

fn open_reader(link: &str) -> Result<(IMFSourceReader, IMFMediaSource, f64), String> {
    let err = |hr: windows::core::Error| {
        if hr.code() == E_ACCESSDENIED {
            i18n::tr_str("O Windows bloqueou o acesso à câmera. Libere em Configurações › Privacidade e segurança › Câmera.")
        } else {
            i18n::tr_str("Não foi possível abrir a webcam (0x%08X).").replace("%08X", &format!("{:08X}", hr.code().0 as u32))
        }
    };
    unsafe {
        let mut attr = None;
        MFCreateAttributes(&mut attr, 2).map_err(err)?;
        let attr = attr.ok_or_else(|| err(windows::core::Error::empty()))?;
        attr.SetGUID(&MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID).map_err(err)?;
        let wl = crate::wide::to_wide(link);
        attr.SetString(&MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK, PCWSTR(wl.as_ptr())).map_err(err)?;
        let src = MFCreateDeviceSource(&attr).map_err(err)?;
        let fmt_err = |hr: windows::core::Error| {
            i18n::tr_str("A webcam não oferece um formato compatível (0x%08X).").replace("%08X", &format!("{:08X}", hr.code().0 as u32))
        };
        let inner = || -> windows::core::Result<(IMFSourceReader, f64)> {
            let mut ra = None;
            MFCreateAttributes(&mut ra, 1)?;
            let ra = ra.ok_or(windows::core::Error::empty())?;
            ra.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)?; // converts any format to RGB32
            let reader = MFCreateSourceReaderFromMediaSource(&src, &ra)?;
            // the native format closest to 640 px wide (light and enough for the bubble)
            let (mut best, mut bw, mut bh): (Option<IMFMediaType>, u32, u32) = (None, 0, 0);
            for i in 0.. {
                let Ok(t) = reader.GetNativeMediaType(FIRST_VIDEO_STREAM, i) else { break };
                let size = t.GetUINT64(&MF_MT_FRAME_SIZE).unwrap_or(0);
                let (w, h) = ((size >> 32) as u32, size as u32);
                if best.is_none() || (w as i32 - 640).abs() < (bw as i32 - 640).abs() {
                    (best, bw, bh) = (Some(t), w, h);
                }
            }
            if let Some(b) = &best {
                reader.SetCurrentMediaType(FIRST_VIDEO_STREAM, None, b)?;
            }
            let out = MFCreateMediaType()?;
            out.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            out.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
            if bw > 0 {
                out.SetUINT64(&MF_MT_FRAME_SIZE, ((bw as u64) << 32) | bh as u64)?;
            }
            reader.SetCurrentMediaType(FIRST_VIDEO_STREAM, None, &out)?;
            Ok((reader, if bw > 0 && bh > 0 { bw as f64 / bh as f64 } else { 4.0 / 3.0 }))
        };
        match inner() {
            Ok((r, a)) => Ok((r, src, a)),
            Err(e) => {
                let _ = src.Shutdown();
                Err(fmt_err(e))
            }
        }
    }
}

fn read_loop(reader: &IMFSourceReader, running: &AtomicBool, frame: &Mutex<Image>, on_frame: &dyn Fn()) {
    let (w, h, def_stride) = unsafe {
        let Ok(cur) = reader.GetCurrentMediaType(FIRST_VIDEO_STREAM) else { return };
        let size = cur.GetUINT64(&MF_MT_FRAME_SIZE).unwrap_or(0);
        ((size >> 32) as u32, size as u32, cur.GetUINT32(&MF_MT_DEFAULT_STRIDE).unwrap_or(0) as i32)
    };
    if w == 0 || h == 0 {
        return;
    }
    let def_stride = if def_stride == 0 { -((w * 4) as i32) } else { def_stride }; // RGB32 without a hint: bottom-up
    while running.load(Ordering::Relaxed) {
        let (mut flags, mut sample) = (0u32, None);
        if unsafe { reader.ReadSample(FIRST_VIDEO_STREAM, 0, None, Some(&mut flags), None, Some(&mut sample)) }.is_err() {
            break;
        }
        if flags & (MF_SOURCE_READERF_ERROR.0 as u32 | MF_SOURCE_READERF_ENDOFSTREAM.0 as u32) != 0 {
            break;
        }
        let Some(sample) = sample else { continue };
        let Ok(buf) = (unsafe { sample.ConvertToContiguousBuffer() }) else { continue };
        let mut img = Image::sized(w as i32, h as i32, 0);
        unsafe {
            let b2: Option<IMF2DBuffer> = buf.cast().ok();
            let (mut scan0, mut pitch) = (std::ptr::null_mut::<u8>(), 0i32);
            let locked2d = b2.as_ref().is_some_and(|b| b.Lock2D(&mut scan0, &mut pitch).is_ok());
            if !locked2d {
                let (mut raw, mut len) = (std::ptr::null_mut::<u8>(), 0u32);
                if buf.Lock(&mut raw, None, Some(&mut len)).is_err() {
                    continue;
                }
                pitch = def_stride;
                scan0 = if pitch < 0 { raw.add((h as usize - 1) * (-pitch) as usize) } else { raw };
            }
            for y in 0..h as usize {
                let s = std::slice::from_raw_parts(scan0.offset(y as isize * pitch as isize) as *const u32, w as usize);
                for (d, &p) in img.px[y * w as usize..(y + 1) * w as usize].iter_mut().zip(s) {
                    *d = p | 0xFF00_0000;
                }
            }
            if locked2d {
                let _ = b2.as_ref().map(|b| b.Unlock2D());
            } else {
                let _ = buf.Unlock();
            }
        }
        if let Ok(mut f) = frame.lock() {
            *f = img;
        }
        on_frame();
    }
}

impl Camera {
    /// Opens the camera with this symbolic link (empty or gone: the first one); `on_frame` runs on the reader thread
    /// after each new frame.
    pub fn start(link: &str, on_frame: impl Fn() + Send + 'static) -> Result<Camera, String> {
        let cams = list_cameras();
        if cams.is_empty() {
            return Err(i18n::tr_str("Nenhuma webcam encontrada."));
        }
        let link = if cams.iter().any(|c| c.link == link) { link.to_string() } else { cams[0].link.clone() };
        let running = Arc::new(AtomicBool::new(true));
        let frame = Arc::new(Mutex::new(Image::default()));
        let (tx, rx) = std::sync::mpsc::channel::<Result<f64, String>>();
        let (r2, f2) = (running.clone(), frame.clone());
        let l2 = link.clone();
        let thread = std::thread::spawn(move || unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let _ = MFStartup(MF_VERSION, MFSTARTUP_LITE);
            match open_reader(&l2) {
                Err(e) => {
                    let _ = tx.send(Err(e));
                }
                Ok((reader, src, aspect)) => {
                    let _ = tx.send(Ok(aspect));
                    read_loop(&reader, &r2, &f2, &on_frame);
                    drop(reader);
                    let _ = src.Shutdown();
                }
            }
            let _ = MFShutdown();
            CoUninitialize();
        });
        let mut cam = Camera { running, frame, thread: Some(thread), aspect: 4.0 / 3.0 };
        match rx.recv() {
            Ok(Ok(a)) => {
                cam.aspect = a;
                log::write(&format!("Webcam: started ({link})"));
                Ok(cam)
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err(i18n::tr_str("Nenhuma webcam encontrada.")),
        }
    }

    /// The latest frame (empty before the first one).
    pub fn frame(&self) -> Image {
        self.frame.lock().map(|f| f.clone()).unwrap_or_default()
    }
}

impl Drop for Camera {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chroma_key_removes_the_green_and_keeps_the_rest() {
        let green = 0x0040B100; // COLORREF of RGB(0, 177, 64)
        assert_eq!(chroma_alpha(0xFF00B140, green, 40), 0, "the key color itself");
        assert_eq!(chroma_alpha(0xFF008C33, green, 40), 0, "a shadow on the green");
        assert_eq!(chroma_alpha(0xFFE0B090, green, 40), 255, "skin");
        assert_eq!(chroma_alpha(0xFF2040C0, green, 40), 255, "a blue shirt");
    }

    #[test]
    fn lists_the_cameras_without_failing() {
        // the machine may have none: only that the call comes back
        let _ = list_cameras();
    }
}
