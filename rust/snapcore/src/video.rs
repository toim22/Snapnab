//! Video in the Editor (Video.cpp — CVideoDoc / MFMP4Encoder / GifCreator, ds_pseudocode §11): a Media Foundation
//! frame reader, MP4 export without the cut ranges, an animated GIF from a range, and the preview audio (waveOut).

use std::collections::VecDeque;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::{
    CALLBACK_EVENT, HWAVEOUT, WAVE_FORMAT_PCM, WAVE_MAPPER, WAVEFORMATEX, WAVEHDR, WHDR_DONE, waveOutClose, waveOutOpen,
    waveOutPrepareHeader, waveOutReset, waveOutUnprepareHeader, waveOutWrite,
};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::core::{GUID, PCWSTR};

use crate::gif::GifWriter;
use crate::i18n::tr_str as tr;
use crate::image::Image;
use crate::wide::to_wide;

/// 100 ns units per second.
pub const HNS: i64 = 10_000_000;
const FIRST_VIDEO: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
const FIRST_AUDIO: u32 = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32;
const ALL_STREAMS: u32 = MF_SOURCE_READER_ALL_STREAMS.0 as u32;
const MEDIASOURCE: u32 = MF_SOURCE_READER_MEDIASOURCE.0 as u32;
const EOS: u32 = MF_SOURCE_READERF_ENDOFSTREAM.0 as u32;

/// A REMOVED range (start inclusive, end exclusive), in 100 ns.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Interval {
    pub start: i64,
    pub end: i64,
}

pub fn is_inside_any_cut(cuts: &[Interval], t: i64) -> bool {
    cuts.iter().any(|c| t >= c.start && t < c.end)
}

pub fn preceding_cuts_duration(cuts: &[Interval], t: i64) -> i64 {
    cuts.iter().filter(|c| c.end <= t).map(|c| c.end - c.start).sum()
}

/// Sorted, without empty ranges, overlapping ones merged.
pub fn normalize(cuts: &[Interval]) -> Vec<Interval> {
    let mut v = cuts.to_vec();
    v.sort_by_key(|c| c.start);
    let mut out: Vec<Interval> = Vec::new();
    for c in v {
        if c.end <= c.start {
            continue;
        }
        match out.last_mut() {
            Some(l) if c.start <= l.end => l.end = l.end.max(c.end),
            _ => out.push(c),
        }
    }
    out
}

fn attrs(n: u32) -> windows::core::Result<IMFAttributes> {
    let mut a = None;
    unsafe { MFCreateAttributes(&mut a, n)? };
    a.ok_or_else(|| windows::core::Error::from_hresult(windows::core::HRESULT(-1)))
}

fn size_of(t: &IMFMediaType, key: &GUID) -> (u32, u32) {
    let v = unsafe { t.GetUINT64(key) }.unwrap_or(0);
    ((v >> 32) as u32, v as u32)
}

fn set_pair(t: &IMFMediaType, key: &GUID, a: u32, b: u32) -> windows::core::Result<()> {
    unsafe { t.SetUINT64(key, ((a as u64) << 32) | b as u64) }
}

fn duration_of(rd: &IMFSourceReader) -> i64 {
    unsafe {
        match rd.GetPresentationAttribute(MEDIASOURCE, &MF_PD_DURATION) {
            Ok(pv) => pv.Anonymous.Anonymous.Anonymous.uhVal as i64,
            Err(_) => 0,
        }
    }
}

fn seek(rd: &IMFSourceReader, t: i64) -> bool {
    let mut pv = PROPVARIANT::default();
    unsafe {
        let a = &mut *pv.Anonymous.Anonymous;
        a.vt = windows::Win32::System::Variant::VT_I8;
        a.Anonymous.hVal = t.max(0);
        rd.SetCurrentPosition(&GUID::zeroed(), &pv).is_ok()
    }
}

/// Copies a frame (RGB32, any pitch) to an image. The decoder often delivers rows wider than the video, so the stride
/// is never assumed to be w * 4.
fn sample_to_image(s: &IMFSample, w: i32, h: i32) -> Option<Image> {
    let mut out = Image::sized(w, h, 0);
    let wu = w as usize;
    let copy2d = |b: &IMFMediaBuffer, out: &mut Image| -> bool {
        let Ok(b2) = windows::core::Interface::cast::<IMF2DBuffer>(b) else { return false };
        let (mut scan0, mut pitch) = (std::ptr::null_mut::<u8>(), 0i32);
        unsafe {
            if b2.Lock2D(&mut scan0, &mut pitch).is_err() {
                return false;
            }
            let ok = pitch.unsigned_abs() as usize >= wu * 4;
            if ok {
                for y in 0..h as usize {
                    let src = std::slice::from_raw_parts(scan0.offset(pitch as isize * y as isize) as *const u32, wu);
                    out.px[y * wu..(y + 1) * wu].copy_from_slice(src);
                }
            }
            let _ = b2.Unlock2D();
            ok
        }
    };
    unsafe {
        if s.GetBufferCount().unwrap_or(0) == 1
            && let Ok(b) = s.GetBufferByIndex(0)
            && copy2d(&b, &mut out)
        {
            out.make_opaque();
            return Some(out);
        }
        let b = s.ConvertToContiguousBuffer().ok()?;
        if !copy2d(&b, &mut out) {
            let (mut p, mut len) = (std::ptr::null_mut::<u8>(), 0u32);
            b.Lock(&mut p, None, Some(&mut len)).ok()?;
            // linear buffer: the stride comes from the size (the height may be aligned too)
            let mut pitch = 0usize;
            for a in [1usize, 2, 8, 16, 32, 64] {
                let hh = (h as usize).div_ceil(a) * a;
                let len = len as usize;
                if len % hh == 0 && len / hh >= wu * 4 && len / hh < wu * 4 + 512 {
                    pitch = len / hh;
                    break;
                }
            }
            if pitch > 0 {
                for y in 0..h as usize {
                    let src = std::slice::from_raw_parts(p.add(y * pitch) as *const u32, wu);
                    out.px[y * wu..(y + 1) * wu].copy_from_slice(src);
                }
            }
            let _ = b.Unlock();
            if pitch == 0 {
                return None;
            }
        }
    }
    out.make_opaque();
    Some(out)
}

// ---------------------------------------------------------------- reader

/// Reads the frames of a video file as images.
pub struct VideoReader {
    reader: Option<IMFSourceReader>,
    mf: bool,
    pub width: i32,
    pub height: i32,
    /// 100 ns
    pub duration: i64,
    pub fps: f64,
}

impl Default for VideoReader {
    fn default() -> Self {
        VideoReader { reader: None, mf: false, width: 0, height: 0, duration: 0, fps: 30.0 }
    }
}

impl VideoReader {
    pub fn open(path: &str) -> Option<VideoReader> {
        let mut v = VideoReader::default();
        unsafe {
            MFStartup(MF_VERSION, MFSTARTUP_FULL).ok()?;
            v.mf = true;
            let a = attrs(1).ok()?;
            a.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1).ok()?;
            let wp = to_wide(path);
            let rd = MFCreateSourceReaderFromURL(PCWSTR(wp.as_ptr()), &a).ok()?;
            let _ = rd.SetStreamSelection(ALL_STREAMS, false);
            let _ = rd.SetStreamSelection(FIRST_VIDEO, true);
            let mt = MFCreateMediaType().ok()?;
            mt.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).ok()?;
            mt.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32).ok()?;
            rd.SetCurrentMediaType(FIRST_VIDEO, None, &mt).ok()?;
            let cur = rd.GetCurrentMediaType(FIRST_VIDEO).ok()?;
            let (w, h) = size_of(&cur, &MF_MT_FRAME_SIZE);
            let (num, den) = size_of(&cur, &MF_MT_FRAME_RATE);
            v.width = w as i32;
            v.height = h as i32;
            v.fps = if den > 0 && num > 0 { num as f64 / den as f64 } else { 30.0 };
            v.duration = duration_of(&rd);
            v.reader = Some(rd);
        }
        (v.width > 0 && v.height > 0).then_some(v)
    }

    pub fn seek(&mut self, t: i64) -> bool {
        self.reader.as_ref().is_some_and(|r| seek(r, t))
    }

    /// The next frame and its time stamp.
    pub fn read_frame(&mut self) -> Option<(Image, i64)> {
        let rd = self.reader.as_ref()?;
        for _ in 0..1000 {
            let (mut flags, mut ts, mut s) = (0u32, 0i64, None);
            unsafe { rd.ReadSample(FIRST_VIDEO, 0, None, Some(&mut flags), Some(&mut ts), Some(&mut s)) }.ok()?;
            if flags & EOS != 0 {
                return None;
            }
            let Some(s) = s else { continue };
            return sample_to_image(&s, self.width, self.height).map(|i| (i, ts));
        }
        None
    }
}

impl Drop for VideoReader {
    fn drop(&mut self) {
        self.reader = None;
        if self.mf {
            unsafe {
                let _ = MFShutdown();
            }
        }
    }
}

/// Returns false to cancel.
pub type Progress<'a> = &'a mut dyn FnMut(i32) -> bool;

/// COM on this thread for the guard's life (only undone when this call did it: a thread already in another apartment
/// keeps it).
struct Com(bool);
impl Com {
    fn mta() -> Com {
        Com(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok())
    }
}
impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

// ---------------------------------------------------------------- MP4 without the cut ranges

/// Re-encodes `src` to H.264/AAC in `dst` without the cut ranges (MFMP4Encoder::Transcode::ToFileWithCuts).
pub fn transcode_with_cuts(src: &str, dst: &str, cuts: &[Interval], progress: Progress) -> Result<(), String> {
    let cuts = normalize(cuts);
    let _com = Com::mta();
    unsafe {
        if MFStartup(MF_VERSION, MFSTARTUP_FULL).is_err() {
            return Err("MFStartup".into());
        }
    }
    let r = transcode(src, dst, &cuts, progress);
    unsafe {
        let _ = MFShutdown();
    }
    if r.is_err() {
        let _ = std::fs::remove_file(dst);
    }
    r
}

fn transcode(src: &str, dst: &str, cuts: &[Interval], progress: Progress) -> Result<(), String> {
    fn e(s: &'static str) -> impl Fn(windows::core::Error) -> String {
        move |_| tr(s)
    }
    unsafe {
        let ra = attrs(1).map_err(e("Não foi possível abrir o vídeo."))?;
        let _ = ra.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1);
        let ws = to_wide(src);
        // separate readers for video and audio (one reader with both fails now and then with video processing on);
        // the samples are interleaved by time stamp when writing
        let rd = MFCreateSourceReaderFromURL(PCWSTR(ws.as_ptr()), &ra).map_err(e("Não foi possível abrir o vídeo."))?;
        let _ = rd.SetStreamSelection(ALL_STREAMS, false);
        let _ = rd.SetStreamSelection(FIRST_VIDEO, true);
        let vt = MFCreateMediaType().map_err(e("Formato de vídeo não suportado."))?;
        let _ = vt.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video);
        let _ = vt.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32);
        rd.SetCurrentMediaType(FIRST_VIDEO, None, &vt).map_err(e("Formato de vídeo não suportado."))?;
        let vcur = rd.GetCurrentMediaType(FIRST_VIDEO).map_err(e("Formato de vídeo não suportado."))?;
        let (w, h) = size_of(&vcur, &MF_MT_FRAME_SIZE);
        let (mut fnum, mut fden) = size_of(&vcur, &MF_MT_FRAME_RATE);
        if fnum == 0 || fden == 0 {
            (fnum, fden) = (30, 1);
        }
        // audio → 16-bit PCM, if there is any
        let (mut rate, mut ch) = (48000u32, 2u32);
        let mut rda = None;
        if let Ok(r) = MFCreateSourceReaderFromURL(PCWSTR(ws.as_ptr()), None) {
            let _ = r.SetStreamSelection(ALL_STREAMS, false);
            let _ = r.SetStreamSelection(FIRST_AUDIO, true);
            if let Ok(at) = MFCreateMediaType() {
                let _ = at.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio);
                let _ = at.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM);
                let _ = at.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16);
                if r.SetCurrentMediaType(FIRST_AUDIO, None, &at).is_ok()
                    && let Ok(acur) = r.GetCurrentMediaType(FIRST_AUDIO)
                {
                    rate = acur.GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND).unwrap_or(48000);
                    ch = acur.GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS).unwrap_or(2);
                    if (rate == 44100 || rate == 48000) && (ch == 1 || ch == 2) {
                        rda = Some(r);
                    }
                }
            }
        }
        let duration = duration_of(&rd);

        let _ = std::fs::remove_file(dst);
        let wa = attrs(1).map_err(e("Não foi possível criar o arquivo de saída."))?;
        let _ = wa.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 0);
        let wd = to_wide(dst);
        let wr = MFCreateSinkWriterFromURL(PCWSTR(wd.as_ptr()), None, &wa).map_err(e("Não foi possível criar o arquivo de saída."))?;
        let (ow, oh) = (w & !1, h & !1);
        let fps = fnum as f64 / fden as f64;
        let vo = MFCreateMediaType().map_err(e("Codificador H.264 indisponível."))?;
        let _ = vo.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video);
        let _ = vo.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264);
        let _ = vo.SetUINT32(&MF_MT_AVG_BITRATE, (0.12 * ow as f64 * oh as f64 * fps).clamp(500_000.0, 40_000_000.0) as u32);
        let _ = vo.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32);
        let _ = set_pair(&vo, &MF_MT_FRAME_SIZE, ow, oh);
        let _ = set_pair(&vo, &MF_MT_FRAME_RATE, fnum, fden);
        let _ = set_pair(&vo, &MF_MT_PIXEL_ASPECT_RATIO, 1, 1);
        let vs = wr.AddStream(&vo).map_err(e("Codificador H.264 indisponível."))?;
        let vi = MFCreateMediaType().map_err(e("Entrada de vídeo recusada."))?;
        let _ = vi.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video);
        let _ = vi.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32);
        let _ = vi.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32);
        let _ = set_pair(&vi, &MF_MT_FRAME_SIZE, ow, oh);
        let _ = set_pair(&vi, &MF_MT_FRAME_RATE, fnum, fden);
        let _ = set_pair(&vi, &MF_MT_PIXEL_ASPECT_RATIO, 1, 1);
        wr.SetInputMediaType(vs, &vi, None).map_err(e("Entrada de vídeo recusada."))?;
        let mut as_ = 0;
        if rda.is_some() {
            let mk = |sub: &GUID| -> windows::core::Result<IMFMediaType> {
                let t = MFCreateMediaType()?;
                t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
                t.SetGUID(&MF_MT_SUBTYPE, sub)?;
                t.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
                t.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, rate)?;
                t.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, ch)?;
                Ok(t)
            };
            let ok = (|| -> windows::core::Result<u32> {
                let ao = mk(&MFAudioFormat_AAC)?;
                ao.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, 24000)?;
                let ai = mk(&MFAudioFormat_PCM)?;
                ai.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, ch * 2)?;
                ai.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, rate * ch * 2)?;
                let s = wr.AddStream(&ao)?;
                wr.SetInputMediaType(s, &ai, None)?;
                Ok(s)
            })();
            as_ = ok.map_err(e("Codificador AAC indisponível."))?;
        }
        wr.BeginWriting().map_err(e("Falha ao iniciar a gravação."))?;

        struct Pending {
            s: Option<IMFSample>,
            ts: i64,
        }
        let fetch = |r: &IMFSourceReader, stream: u32, p: &mut Pending, done: &mut bool| -> Result<(), String> {
            while !*done && p.s.is_none() {
                let (mut flags, mut ts, mut s) = (0u32, 0i64, None);
                if let Err(x) = r.ReadSample(stream, 0, None, Some(&mut flags), Some(&mut ts), Some(&mut s)) {
                    return Err(crate::i18n::fill(&tr("Falha ao ler o vídeo (0x%08X)."), &[&format!("{:08X}", x.code().0 as u32)]));
                }
                if flags & EOS != 0 {
                    *done = true;
                    break;
                }
                if s.is_some() {
                    p.s = s;
                    p.ts = ts;
                }
            }
            Ok(())
        };
        let (mut video_done, mut audio_done) = (false, rda.is_none());
        let (mut pv, mut pa) = (Pending { s: None, ts: 0 }, Pending { s: None, ts: 0 });
        let mut flip = vec![0u8; ow as usize * oh as usize * 4];
        let mut last_pct = -1;
        while !(video_done && audio_done) {
            fetch(&rd, FIRST_VIDEO, &mut pv, &mut video_done)?;
            if let Some(ra) = rda.as_ref()
                && let Err(x) = fetch(ra, FIRST_AUDIO, &mut pa, &mut audio_done)
            {
                crate::log::write(&format!("Cut: unreadable audio ({x}), continuing without audio"));
                audio_done = true;
                pa.s = None;
            }
            if pv.s.is_none() && pa.s.is_none() {
                continue;
            }
            let is_video = pv.s.is_some() && (pa.s.is_none() || pv.ts <= pa.ts);
            let cur = if is_video { &mut pv } else { &mut pa };
            let (s, ts) = (cur.s.take().unwrap(), cur.ts);
            if is_inside_any_cut(cuts, ts) {
                continue;
            }
            let removed = preceding_cuts_duration(cuts, ts);
            let dur = s.GetSampleDuration().unwrap_or(0);
            if is_video {
                let Some(img) = sample_to_image(&s, w as i32, h as i32) else { continue };
                // the sink writer's RGB32 input is bottom-up
                let row = ow as usize * 4;
                for y in 0..oh as usize {
                    let src = &img.px[y * w as usize..y * w as usize + ow as usize];
                    let d = &mut flip[(oh as usize - 1 - y) * row..(oh as usize - y) * row];
                    for (k, p) in src.iter().enumerate() {
                        d[k * 4..k * 4 + 4].copy_from_slice(&p.to_le_bytes());
                    }
                }
                let b = MFCreateMemoryBuffer(flip.len() as u32).map_err(e("Falha ao codificar quadro."))?;
                let mut p = std::ptr::null_mut();
                if b.Lock(&mut p, None, None).is_ok() {
                    std::ptr::copy_nonoverlapping(flip.as_ptr(), p, flip.len());
                    let _ = b.Unlock();
                }
                let _ = b.SetCurrentLength(flip.len() as u32);
                let o = MFCreateSample().map_err(e("Falha ao codificar quadro."))?;
                let _ = o.AddBuffer(&b);
                let _ = o.SetSampleTime(ts - removed);
                let _ = o.SetSampleDuration(dur);
                wr.WriteSample(vs, &o).map_err(e("Falha ao codificar quadro."))?;
                if duration > 0 {
                    let pct = (ts * 100 / duration) as i32;
                    if pct != last_pct {
                        last_pct = pct;
                        if !progress(pct) {
                            return Err(tr("Cancelado."));
                        }
                    }
                }
            } else {
                let _ = s.SetSampleTime(ts - removed);
                wr.WriteSample(as_, &s).map_err(e("Falha ao codificar áudio."))?;
            }
        }
        wr.Finalize().map_err(e("Falha ao finalizar o MP4."))?;
    }
    Ok(())
}

// ---------------------------------------------------------------- GIF

/// An animated GIF of [from, to] (to <= from = until the end) at `fps`, `width` px wide (V2AGNative::GifCreator).
#[allow(clippy::too_many_arguments)]
pub fn create_gif_from_video(src: &str, dst: &str, from: i64, to: i64, fps: i32, width: i32, bpp: i32, dither: bool,
                             delta: bool, progress: Progress) -> Result<(), String> {
    let _com = Com::mta();
    let mut rd = VideoReader::open(src).ok_or_else(|| tr("Não foi possível abrir o vídeo."))?;
    let to = if to <= from { rd.duration } else { to };
    let fps = fps.max(1);
    let ow = width.clamp(16, rd.width.max(16));
    let oh = ((rd.height as f64 * ow as f64 / rd.width as f64).round() as i32).max(16);
    let mut gw = GifWriter::open(Path::new(dst), ow, oh, fps as f64, bpp, dither, delta, b"")
        .ok_or_else(|| tr("Não foi possível criar o GIF."))?;
    rd.seek(from);
    let step = HNS / fps as i64;
    let mut next = from;
    let mut frames = 0;
    let mut canceled = false;
    while let Some((img, ts)) = rd.read_frame() {
        if ts < next - step / 2 {
            continue;
        }
        if ts > to {
            break;
        }
        // repeated frames lengthen the delay instead of being added again
        let mut reps = 1;
        next += step;
        while ts >= next + step / 2 {
            next += step;
            reps += 1;
        }
        gw.add_frame(&crate::gdip::scale_image(&img, ow, oh, true), ((100.0 * reps as f64 / fps as f64).round() as i32).max(2));
        frames += 1;
        if to > from && !progress(((ts - from) * 100 / (to - from)) as i32) {
            canceled = true;
            break;
        }
    }
    let ok = gw.close();
    if canceled {
        let _ = std::fs::remove_file(dst);
        return Err(tr("Cancelado."));
    }
    if frames == 0 {
        return Err(tr("Nenhum quadro no intervalo."));
    }
    if ok { Ok(()) } else { Err(tr("Não foi possível criar o GIF.")) }
}

// ---------------------------------------------------------------- preview audio

/// Plays the audio track from a position through waveOut, skipping the cut ranges; stops when dropped.
pub struct AudioPlayer {
    run: Arc<AtomicBool>,
    th: Option<JoinHandle<()>>,
}

impl AudioPlayer {
    pub fn start(path: &str, from: i64, cuts: Vec<Interval>) -> AudioPlayer {
        let run = Arc::new(AtomicBool::new(true));
        let r = run.clone();
        let path = path.to_string();
        let th = std::thread::spawn(move || {
            let _com = Com::mta();
            unsafe {
                if MFStartup(MF_VERSION, MFSTARTUP_LITE).is_ok() {
                    play(&path, from, &cuts, &r);
                    let _ = MFShutdown();
                }
            }
        });
        AudioPlayer { run, th: Some(th) }
    }
}

impl Drop for AudioPlayer {
    fn drop(&mut self) {
        self.run.store(false, Ordering::Relaxed);
        if let Some(t) = self.th.take() {
            let _ = t.join();
        }
    }
}

unsafe fn play(path: &str, from: i64, cuts: &[Interval], run: &AtomicBool) {
    unsafe {
        let wp = to_wide(path);
        let Ok(rd) = MFCreateSourceReaderFromURL(PCWSTR(wp.as_ptr()), None) else { return };
        let _ = rd.SetStreamSelection(ALL_STREAMS, false);
        if rd.SetStreamSelection(FIRST_AUDIO, true).is_err() {
            return; // a video without audio
        }
        let Ok(mt) = MFCreateMediaType() else { return };
        let _ = mt.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio);
        let _ = mt.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM);
        let _ = mt.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16);
        if rd.SetCurrentMediaType(FIRST_AUDIO, None, &mt).is_err() {
            return;
        }
        let Ok(cur) = rd.GetCurrentMediaType(FIRST_AUDIO) else { return };
        let ch = cur.GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS).unwrap_or(2) as u16;
        let rate = cur.GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND).unwrap_or(48000);
        let wf = WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_PCM as u16,
            nChannels: ch,
            nSamplesPerSec: rate,
            nAvgBytesPerSec: rate * ch as u32 * 2,
            nBlockAlign: ch * 2,
            wBitsPerSample: 16,
            cbSize: 0,
        };
        seek(&rd, from);
        let Ok(ev) = CreateEventW(None, false, false, None) else { return };
        let mut wo = HWAVEOUT::default();
        if waveOutOpen(Some(&mut wo), WAVE_MAPPER, &wf, Some(ev.0 as usize), None, CALLBACK_EVENT) != 0 {
            let _ = CloseHandle(ev);
            return;
        }
        // each header owns its data; boxed so the addresses stay put while waveOut uses them
        let mut queue: VecDeque<(Box<WAVEHDR>, Vec<u8>)> = VecDeque::new();
        let hdr_size = std::mem::size_of::<WAVEHDR>() as u32;
        let recycle = |queue: &mut VecDeque<(Box<WAVEHDR>, Vec<u8>)>, all: bool| {
            while let Some(f) = queue.front() {
                if !all && f.0.dwFlags & WHDR_DONE == 0 {
                    break;
                }
                let (mut h, _) = queue.pop_front().unwrap();
                waveOutUnprepareHeader(wo, &mut *h, hdr_size);
            }
        };
        let mut eos = false;
        while run.load(Ordering::Relaxed) && !(eos && queue.is_empty()) {
            recycle(&mut queue, false);
            if eos || queue.len() >= 10 {
                let _ = WaitForSingleObject(ev, 30) == WAIT_OBJECT_0;
                continue;
            }
            let (mut flags, mut ts, mut s) = (0u32, 0i64, None);
            if rd.ReadSample(FIRST_AUDIO, 0, None, Some(&mut flags), Some(&mut ts), Some(&mut s)).is_err() || flags & EOS != 0 {
                eos = true;
                continue;
            }
            let Some(s) = s else { continue };
            if ts < from - HNS / 20 || is_inside_any_cut(cuts, ts) {
                continue;
            }
            let Ok(b) = s.ConvertToContiguousBuffer() else { continue };
            let (mut p, mut len) = (std::ptr::null_mut::<u8>(), 0u32);
            if b.Lock(&mut p, None, Some(&mut len)).is_err() {
                continue;
            }
            let mut data = std::slice::from_raw_parts(p, len as usize).to_vec();
            let _ = b.Unlock();
            let mut h = Box::new(WAVEHDR { lpData: windows::core::PSTR(data.as_mut_ptr()), dwBufferLength: len, ..Default::default() });
            waveOutPrepareHeader(wo, &mut *h, hdr_size);
            waveOutWrite(wo, &mut *h, hdr_size);
            queue.push_back((h, data));
        }
        waveOutReset(wo);
        recycle(&mut queue, true);
        waveOutClose(wo);
        let _ = CloseHandle(HANDLE(ev.0));
    }
}

/// "mm:ss.ss"
/// Reads only the audio track and reports it (the e2e suite checks that a recording has sound; the former
/// `SnapnabTests.exe audio`): the samples, the end of the stream and the peak level.
pub fn audio_report(path: &str) -> String {
    use std::fmt::Write;
    let _com = Com::mta();
    let mut out = String::new();
    unsafe {
        if MFStartup(MF_VERSION, MFSTARTUP_FULL).is_err() {
            return "MFStartup failed\n".into();
        }
        let wp = to_wide(path);
        let rd = match MFCreateSourceReaderFromURL(PCWSTR(wp.as_ptr()), None) {
            Ok(r) => {
                out += "open 0x00000000\n";
                r
            }
            Err(e) => {
                let _ = writeln!(out, "open 0x{:08X}", e.code().0 as u32);
                let _ = MFShutdown();
                return out;
            }
        };
        let _ = rd.SetStreamSelection(ALL_STREAMS, false);
        let _ = rd.SetStreamSelection(FIRST_AUDIO, true);
        if let Ok(mt) = MFCreateMediaType() {
            let _ = mt.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio);
            let _ = mt.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM);
            let _ = mt.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16);
            let hr = rd.SetCurrentMediaType(FIRST_AUDIO, None, &mt).map_or_else(|e| e.code().0 as u32, |_| 0);
            let _ = writeln!(out, "settype 0x{hr:08X}");
        }
        let mut peak = 0i32;
        for i in 0..10000 {
            let (mut flags, mut ts, mut s) = (0u32, 0i64, None);
            if let Err(e) = rd.ReadSample(FIRST_AUDIO, 0, None, Some(&mut flags), Some(&mut ts), Some(&mut s)) {
                let _ = writeln!(out, "sample {i} FAILED 0x{:08X}", e.code().0 as u32);
                break;
            }
            if flags & EOS != 0 {
                let _ = writeln!(out, "EOS after {i} samples, last ts {:.3}", ts as f64 / 1e7);
                break;
            }
            if i < 3 || i % 50 == 0 {
                let _ = writeln!(out, "sample {i} ts {:.3} flags {flags}", ts as f64 / 1e7);
            }
            if let Some(s) = s
                && let Ok(b) = s.ConvertToContiguousBuffer()
            {
                let (mut p, mut len) = (std::ptr::null_mut::<u8>(), 0u32);
                if b.Lock(&mut p, None, Some(&mut len)).is_ok() {
                    let data = std::slice::from_raw_parts(p as *const u8, len as usize);
                    for c in data.chunks_exact(2) {
                        peak = peak.max((i16::from_le_bytes([c[0], c[1]]) as i32).abs());
                    }
                    let _ = b.Unlock();
                }
            }
        }
        let _ = writeln!(out, "audio peak: {peak} of 32767 ({:.1}%)", peak as f64 * 100.0 / 32767.0);
        drop(rd);
        let _ = MFShutdown();
    }
    out
}

pub fn fmt_time(t: i64) -> String {
    let s = t as f64 / HNS as f64;
    let m = (s / 60.0) as i32;
    format!("{m:02}:{:05.2}", s - m as f64 * 60.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cuts_merge_and_shift() {
        let c = normalize(&[Interval { start: 50, end: 60 }, Interval { start: 10, end: 20 }, Interval { start: 15, end: 30 },
                            Interval { start: 70, end: 70 }]);
        assert_eq!(c, vec![Interval { start: 10, end: 30 }, Interval { start: 50, end: 60 }]);
        assert!(is_inside_any_cut(&c, 10) && !is_inside_any_cut(&c, 30));
        assert_eq!(preceding_cuts_duration(&c, 55), 20);
        assert_eq!(preceding_cuts_duration(&c, 60), 30);
        assert_eq!(fmt_time(HNS * 75 + HNS / 2), "01:15.50");
    }
}
