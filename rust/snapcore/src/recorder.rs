//! Screen recording (Recorder.cpp: TSCRec3 + AVFileWriter + AVStreamEncoder, ds_arch §12, comp_3).
//!   video: BitBlt of the area → RGB32 samples → IMFSinkWriter (H.264 in MP4), hardware encoding when it works
//!   audio: WASAPI (microphone + system loopback) → a mixer with separate mute, volume and automatic gain → AAC
//!   drawn in the video: the cursor highlight, click ripples and the keys pressed (C07), zoom following the pointer
//!   (C15); a crash-safe fragmented MP4 remuxed at the end (C14); audio only, to .m4a or .mp3 (C32).
//!
//! The threads share one `Shared` (atomics and a few mutexes); the recorder itself lives on the app's thread.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU8, AtomicU32, Ordering::Relaxed};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC,
    DeleteObject, GdiFlush, GetDC, HALFTONE, ReleaseDC, SRCCOPY, SelectObject, SetBrushOrgEx, SetStretchBltMode, StretchBlt,
};
use windows::Win32::Graphics::GdiPlus::{
    FontStyleBold, SmoothingModeAntiAlias, StringAlignmentCenter, TextRenderingHintAntiAlias,
};
use windows::Win32::Media::Audio::{
    AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_LOOPBACK,
    AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator,
    WAVEFORMATEX, eCapture, eConsole, eRender,
};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::Media::{timeBeginPeriod, timeEndPeriod};
use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemAlloc, CoTaskMemFree, CoUninitialize};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::System::Power::{ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED, SetThreadExecutionState};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyNameTextW, MAPVK_VK_TO_VSC, MapVirtualKeyW, VK_BACK, VK_CAPITAL, VK_CONTROL, VK_DELETE, VK_DOWN,
    VK_END, VK_ESCAPE, VK_F1, VK_F24, VK_HOME, VK_INSERT, VK_LBUTTON, VK_LEFT, VK_LSHIFT, VK_LWIN, VK_MENU, VK_NEXT,
    VK_NUMLOCK, VK_PRIOR, VK_RBUTTON, VK_RETURN, VK_RIGHT, VK_RMENU, VK_RWIN, VK_SHIFT, VK_SNAPSHOT, VK_SPACE, VK_TAB, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CURSOR_SHOWING, CURSORINFO, CallNextHookEx, DI_NORMAL, DrawIconEx, GetCursorInfo, GetIconInfo, GetMessageW, GetSystemMetrics,
    HC_ACTION, HICON, ICONINFO, KBDLLHOOKSTRUCT, LLKHF_ALTDOWN, MSG, MSLLHOOKSTRUCT, PostThreadMessageW, SM_CXCURSOR,
    SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_LBUTTONDOWN, WM_QUIT,
    WM_RBUTTONDOWN, WM_SYSKEYDOWN,
};
use windows::core::{GUID, PCWSTR};

use crate::gdip::{self, Brush, Font, Graphics, Path, Pen, StringFormat};
use crate::image::{Image, Rect};
use crate::wide::to_wide;
use crate::{library, log, paths};

/// The mixer's and the AAC encoder's rate.
const RATE: i64 = 48000;
const HNS: i64 = 10_000_000;

#[derive(Clone, Debug)]
pub struct RecordSettings {
    /// the screen area
    pub rect: Rect,
    pub cursor: bool,
    pub microphone: bool,
    pub system_audio: bool,
    pub mic_muted: bool,
    pub sys_muted: bool,
    /// automatic microphone gain (otherwise only the manual volume)
    pub auto_gain: bool,
    /// % (0–200)
    pub mic_volume: i32,
    pub sys_volume: i32,
    pub fps: i32,
    /// 0..3
    pub quality: i32,
    pub downscale_1080: bool,
    pub hardware: bool,
    pub path: String,
    // drawn in the video (C07); colors are COLORREFs
    pub highlight_cursor: bool,
    pub highlight_color: u32,
    /// px at the screen size, %
    pub highlight_size: i32,
    pub highlight_opacity: i32,
    pub click_effects: bool,
    pub left_click_color: u32,
    pub right_click_color: u32,
    /// keys on screen: 0 off, 1 shortcuts only (Ctrl/Alt/Win + key, F-keys), 2 every key
    pub key_mode: i32,
    /// zooms in on each click for a moment (C15)
    pub auto_zoom: bool,
    pub zoom_percent: i32,
    /// a fragmented MP4 while recording: a crash leaves a recoverable file (C14)
    pub crash_safe: bool,
    /// C32: no video; .m4a (AAC) or .mp3 by the path's extension
    pub audio_only: bool,
}

const fn rgb(r: u32, g: u32, b: u32) -> u32 {
    r | (g << 8) | (b << 16)
}

impl Default for RecordSettings {
    fn default() -> Self {
        RecordSettings {
            rect: Rect::default(),
            cursor: true,
            microphone: true,
            system_audio: true,
            mic_muted: false,
            sys_muted: false,
            auto_gain: true,
            mic_volume: 100,
            sys_volume: 100,
            fps: 30,
            quality: 2,
            downscale_1080: true,
            hardware: true,
            path: String::new(),
            highlight_cursor: false,
            highlight_color: rgb(255, 214, 0),
            highlight_size: 44,
            highlight_opacity: 35,
            click_effects: false,
            left_click_color: rgb(229, 50, 45),
            right_click_color: rgb(0, 120, 215),
            key_mode: 0,
            auto_zoom: false,
            zoom_percent: 200,
            crash_safe: true,
            audio_only: false,
        }
    }
}

/// The VideoRecording states (comp_3 §4.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Idle,
    Recording,
    Paused,
    Stopping,
}

/// What the recording threads tell the app (disk space, a failed frame).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    DiskLow,
    DiskFull,
    Error,
}

pub type Notify = Arc<dyn Fn(Note) + Send + Sync>;

struct AtomicF32(AtomicU32);

impl AtomicF32 {
    const fn new(v: f32) -> AtomicF32 {
        AtomicF32(AtomicU32::new(v.to_bits()))
    }
    fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Relaxed))
    }
    fn set(&self, v: f32) {
        self.0.store(v.to_bits(), Relaxed)
    }
}

/// The sink writer is used from the video and the audio threads, one at a time (Media Foundation objects are
/// free-threaded).
struct SendWriter(IMFSinkWriter);
unsafe impl Send for SendWriter {}

#[derive(Clone, Copy)]
struct InputEvent {
    key: bool,
    code: u32,
    pt: (i32, i32),
    ctrl: bool,
    alt: bool,
    shift: bool,
    win: bool,
    tick: u64,
}

#[derive(Default)]
struct Clock {
    freq: i64,
    start: i64,
    paused_total: i64,
    pause_start: i64,
}

struct Shared {
    state: AtomicU8,
    running: AtomicBool,
    mic_muted: AtomicBool,
    sys_muted: AtomicBool,
    mic_level: AtomicF32,
    sys_level: AtomicF32,
    mic_vol: AtomicI32,
    sys_vol: AtomicI32,
    auto_gain: AtomicBool,
    /// the current automatic gain (for the meter)
    mic_gain_now: AtomicF32,
    monitoring: AtomicBool,
    highlight: AtomicBool,
    clicks: AtomicBool,
    zoom_on: AtomicBool,
    key_mode: AtomicI32,
    clock: Mutex<Clock>,
    writer: Mutex<Option<SendWriter>>,
    /// clicks and keys seen by the hooks, taken by the video thread (quick taps are not lost between two frames)
    input: Mutex<Vec<InputEvent>>,
    first_frame: Mutex<Image>,
    last_error: Mutex<String>,
}

fn qpc() -> i64 {
    let mut v = 0i64;
    unsafe {
        let _ = QueryPerformanceCounter(&mut v);
    }
    v
}

fn state_from(v: u8) -> State {
    match v {
        1 => State::Recording,
        2 => State::Paused,
        3 => State::Stopping,
        _ => State::Idle,
    }
}

impl Shared {
    fn state(&self) -> State {
        state_from(self.state.load(Relaxed))
    }
    fn set_state(&self, s: State) {
        self.state.store(s as u8, Relaxed);
    }
    fn elapsed_raw(c: &Clock) -> i64 {
        ((qpc() - c.start) as i128 * HNS as i128 / c.freq.max(1) as i128) as i64
    }
    /// The recording clock in 100 ns units, without the pauses.
    fn now_100ns(&self) -> i64 {
        let c = self.clock.lock().unwrap();
        let t = Self::elapsed_raw(&c);
        let mut paused = c.paused_total;
        if self.state() == State::Paused {
            paused += t - c.pause_start;
        }
        (t - paused).max(0)
    }
    fn write_sample(&self, stream: u32, data: &[u8], time: i64, dur: i64) -> bool {
        let r = (|| -> windows::core::Result<()> {
            unsafe {
                let buf = MFCreateMemoryBuffer(data.len() as u32)?;
                let mut p = std::ptr::null_mut();
                buf.Lock(&mut p, None, None)?;
                std::ptr::copy_nonoverlapping(data.as_ptr(), p, data.len());
                buf.Unlock()?;
                buf.SetCurrentLength(data.len() as u32)?;
                let smp = MFCreateSample()?;
                smp.AddBuffer(&buf)?;
                smp.SetSampleTime(time)?;
                smp.SetSampleDuration(dur)?;
                let w = self.writer.lock().unwrap();
                match w.as_ref() {
                    Some(w) => w.0.WriteSample(stream, &smp),
                    None => Ok(()),
                }
            }
        })();
        if let Err(e) = &r {
            log::write(&format!("Recorder: WriteSample({stream}) 0x{:08X}", e.code().0 as u32));
        }
        r.is_ok()
    }
}

// ---------------------------------------------------------------- WASAPI audio source
type Buf = Arc<Mutex<VecDeque<f32>>>;

/// Interleaved stereo at 48 kHz, filled by one capture thread.
struct AudioSource {
    run: Arc<AtomicBool>,
    buf: Buf,
    th: Option<JoinHandle<()>>,
}

fn clear(b: &Buf) {
    b.lock().unwrap().clear();
}

/// Takes up to `n` frames into `out`; returns how many came.
fn pop(b: &Buf, out: &mut [f32], n: usize) -> usize {
    let mut buf = b.lock().unwrap();
    let mut avail = buf.len() / 2;
    // drift: if the buffer grew too much (> n + 200 ms), drops the oldest excess
    if avail > n + RATE as usize / 5 {
        let drop = avail - n - RATE as usize / 20;
        buf.drain(..drop * 2);
        avail -= drop;
    }
    let take = n.min(avail);
    for (i, v) in buf.drain(..take * 2).enumerate() {
        out[i] = v;
    }
    take
}

impl AudioSource {
    fn start(loopback: bool, sh: Arc<Shared>) -> Option<AudioSource> {
        let run = Arc::new(AtomicBool::new(true));
        let buf: Buf = Arc::new(Mutex::new(VecDeque::new()));
        let (tx, rx) = mpsc::channel();
        let th = {
            let (run, buf) = (run.clone(), buf.clone());
            std::thread::spawn(move || audio_loop(loopback, &sh, &run, &buf, tx))
        };
        if rx.recv().unwrap_or(false) {
            Some(AudioSource { run, buf, th: Some(th) })
        } else {
            let _ = th.join();
            None
        }
    }

    fn stop(&mut self) {
        self.run.store(false, Relaxed);
        if let Some(t) = self.th.take() {
            let _ = t.join();
        }
    }
}

impl Drop for AudioSource {
    fn drop(&mut self) {
        self.stop();
    }
}

fn audio_loop(loopback: bool, sh: &Shared, run: &AtomicBool, buf: &Buf, ready: mpsc::Sender<bool>) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let who = if loopback { "system" } else { "microphone" };
    let level = if loopback { &sh.sys_level } else { &sh.mic_level };
    let fail = |what: &str, e: windows::core::Error| {
        log::write(&format!("Audio ({who}): {what} failed 0x{:08X}", e.code().0 as u32));
    };
    let result = (|| -> Option<()> {
        let en: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.map_err(|e| fail("MMDeviceEnumerator", e)).ok()?;
        let dev = unsafe { en.GetDefaultAudioEndpoint(if loopback { eRender } else { eCapture }, eConsole) }
            .map_err(|e| fail("GetDefaultAudioEndpoint", e))
            .ok()?;
        let mut client: IAudioClient = unsafe { dev.Activate(CLSCTX_ALL, None) }.map_err(|e| fail("Activate", e)).ok()?;
        let mut wf = unsafe { client.GetMixFormat() }.map_err(|e| fail("GetMixFormat", e)).ok()?;
        let lb = if loopback { AUDCLNT_STREAMFLAGS_LOOPBACK } else { 0 };
        if let Err(e) = unsafe { client.Initialize(AUDCLNT_SHAREMODE_SHARED, lb, 2_000_000, 0, wf, None) } {
            // some drivers reject their own mix format: stereo float 48 kHz with the WASAPI conversion
            log::write(&format!("Audio ({who}): mix format rejected (0x{:08X}), trying automatic conversion", e.code().0 as u32));
            unsafe { CoTaskMemFree(Some(wf as *const _)) };
            client = unsafe { dev.Activate(CLSCTX_ALL, None) }.map_err(|e| fail("Activate", e)).ok()?;
            unsafe {
                let std = CoTaskMemAlloc(std::mem::size_of::<WAVEFORMATEX>()) as *mut WAVEFORMATEX;
                std.write(WAVEFORMATEX {
                    wFormatTag: 3, // WAVE_FORMAT_IEEE_FLOAT
                    nChannels: 2,
                    nSamplesPerSec: 48000,
                    nAvgBytesPerSec: 48000 * 8,
                    nBlockAlign: 8,
                    wBitsPerSample: 32,
                    cbSize: 0,
                });
                wf = std;
            }
            let flags = lb | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
            if let Err(e) = unsafe { client.Initialize(AUDCLNT_SHAREMODE_SHARED, flags, 2_000_000, 0, wf, None) } {
                unsafe { CoTaskMemFree(Some(wf as *const _)) };
                fail("Initialize", e);
                return None;
            }
        }
        let cap: IAudioCaptureClient = match unsafe { client.GetService() } {
            Ok(c) => c,
            Err(e) => {
                unsafe { CoTaskMemFree(Some(wf as *const _)) };
                fail("GetService", e);
                return None;
            }
        };
        let f = unsafe { wf.read_unaligned() };
        let tag = f.wFormatTag;
        let mut is_float = tag == 3;
        if tag == 0xFFFE {
            // WAVE_FORMAT_EXTENSIBLE: the SubFormat GUID after Samples and dwChannelMask
            const IEEE_FLOAT: GUID = GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);
            let sub = unsafe { (wf as *const u8).add(24).cast::<GUID>().read_unaligned() };
            is_float = sub == IEEE_FLOAT;
        }
        let (ch, bits, rate, blk) = (f.nChannels as usize, f.wBitsPerSample as usize, f.nSamplesPerSec as i64, f.nBlockAlign as usize);
        log::write(&format!("Audio ({who}): {rate} Hz, {ch} channels, {bits} bits{}", if is_float { " float" } else { "" }));
        unsafe { CoTaskMemFree(Some(wf as *const _)) };
        unsafe { client.Start() }.map_err(|e| fail("Start", e)).ok()?;
        let _ = ready.send(true);

        let mut st: Vec<f32> = Vec::new(); // stereo frames at the original rate
        let (mut pos, mut prev) = (0.0f64, (0.0f32, 0.0f32)); // linear resampling
        while run.load(Relaxed) {
            std::thread::sleep(Duration::from_millis(10));
            while let Ok(pkt) = unsafe { cap.GetNextPacketSize() } {
                if pkt == 0 {
                    break;
                }
                let (mut data, mut frames, mut flags) = (std::ptr::null_mut(), 0u32, 0u32);
                if unsafe { cap.GetBuffer(&mut data, &mut frames, &mut flags, None, None) }.is_err() {
                    break;
                }
                let n = frames as usize;
                st.clear();
                st.resize(n * 2, 0.0);
                let mut peak = 0.0f32;
                if flags & 2 == 0 {
                    // not AUDCLNT_BUFFERFLAGS_SILENT
                    for fr in 0..n {
                        let mut s = [0.0f32; 2];
                        for (c, v) in s.iter_mut().enumerate().take(ch.min(2)) {
                            let p = unsafe { data.add(fr * blk + c * (bits / 8)) };
                            *v = unsafe {
                                if is_float && bits == 32 {
                                    p.cast::<f32>().read_unaligned()
                                } else if bits == 16 {
                                    p.cast::<i16>().read_unaligned() as f32 / 32768.0
                                } else if bits == 24 {
                                    (((*p as i32) << 8 | (*p.add(1) as i32) << 16 | (*p.add(2) as i32) << 24) >> 8) as f32 / 8388608.0
                                } else if bits == 32 {
                                    p.cast::<i32>().read_unaligned() as f32 / 2147483648.0
                                } else {
                                    0.0
                                }
                            };
                        }
                        if ch == 1 {
                            s[1] = s[0];
                        }
                        st[fr * 2] = s[0];
                        st[fr * 2 + 1] = s[1];
                        peak = peak.max(s[0].abs()).max(s[1].abs());
                    }
                }
                unsafe {
                    let _ = cap.ReleaseBuffer(frames);
                }
                level.set(peak.max(level.get() * 0.85));
                // push, resampled to 48 kHz
                let mut b = buf.lock().unwrap();
                if rate == RATE {
                    b.extend(st.iter().copied());
                } else {
                    let ratio = rate as f64 / RATE as f64;
                    while pos <= n as f64 - 1.0 {
                        let i = pos.floor() as isize;
                        let fr = (pos - i as f64) as f32;
                        let (al, ar) = if i < 0 { prev } else { (st[i as usize * 2], st[i as usize * 2 + 1]) };
                        let j = (i + 1) as usize;
                        let (bl, br) = (st[j * 2], st[j * 2 + 1]);
                        b.push_back(al + (bl - al) * fr);
                        b.push_back(ar + (br - ar) * fr);
                        pos += ratio;
                    }
                    pos -= n as f64;
                    if n > 0 {
                        prev = (st[(n - 1) * 2], st[(n - 1) * 2 + 1]);
                    }
                }
                let max = RATE as usize * 5 * 2; // never more than 5 s
                if b.len() > max {
                    let ex = b.len() - max;
                    b.drain(..ex);
                }
            }
            level.set(level.get() * 0.9);
        }
        unsafe {
            let _ = client.Stop();
        }
        Some(())
    })();
    if result.is_none() {
        let _ = ready.send(false);
    }
    unsafe { CoUninitialize() };
}

// ---------------------------------------------------------------- input hooks (C07): every click and key, even quick ones
static HOOKED: Mutex<Option<Arc<Shared>>> = Mutex::new(None);

fn push_input(e: InputEvent) {
    if let Ok(h) = HOOKED.lock()
        && let Some(sh) = h.as_ref()
        && let Ok(mut v) = sh.input.lock()
        && v.len() < 256
    {
        v.push(e);
    }
}

fn down(vk: u16) -> bool {
    unsafe { GetAsyncKeyState(vk as i32) < 0 }
}

unsafe extern "system" fn key_hook(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && (w.0 == WM_KEYDOWN as usize || w.0 == WM_SYSKEYDOWN as usize) {
        let k = unsafe { &*(l.0 as *const KBDLLHOOKSTRUCT) };
        push_input(InputEvent {
            key: true,
            code: k.vkCode,
            pt: (0, 0),
            ctrl: down(VK_CONTROL.0),
            alt: down(VK_MENU.0) || k.flags.0 & LLKHF_ALTDOWN.0 != 0,
            shift: down(VK_SHIFT.0),
            win: down(VK_LWIN.0) || down(VK_RWIN.0),
            tick: unsafe { GetTickCount64() },
        });
    }
    unsafe { CallNextHookEx(None, code, w, l) }
}

unsafe extern "system" fn mouse_hook(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && (w.0 == WM_LBUTTONDOWN as usize || w.0 == WM_RBUTTONDOWN as usize) {
        let m = unsafe { &*(l.0 as *const MSLLHOOKSTRUCT) };
        let right = w.0 == WM_RBUTTONDOWN as usize;
        push_input(InputEvent {
            key: false,
            code: if right { VK_RBUTTON.0 as u32 } else { VK_LBUTTON.0 as u32 },
            pt: (m.pt.x, m.pt.y),
            ctrl: false,
            alt: false,
            shift: false,
            win: false,
            tick: unsafe { GetTickCount64() },
        });
    }
    unsafe { CallNextHookEx(None, code, w, l) }
}

fn input_thread(tid: &AtomicU32) {
    tid.store(unsafe { GetCurrentThreadId() }, Relaxed);
    let kh = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(key_hook), None, 0) };
    let mh = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), None, 0) };
    let mut msg = MSG::default();
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 > 0 {}
    unsafe {
        if let Ok(h) = kh {
            let _ = UnhookWindowsHookEx(h);
        }
        if let Ok(h) = mh {
            let _ = UnhookWindowsHookEx(h);
        }
    }
}

// ---------------------------------------------------------------- the writer
fn set_size(t: &IMFMediaType, key: &GUID, a: u32, b: u32) -> windows::core::Result<()> {
    unsafe { t.SetUINT64(key, ((a as u64) << 32) | b as u64) }
}

fn audio_types(mp3: bool) -> windows::core::Result<(IMFMediaType, IMFMediaType)> {
    unsafe {
        let aout = MFCreateMediaType()?;
        aout.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
        aout.SetGUID(&MF_MT_SUBTYPE, if mp3 { &MFAudioFormat_MP3 } else { &MFAudioFormat_AAC })?;
        aout.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, RATE as u32)?;
        aout.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 2)?;
        aout.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, 24000)?;
        if !mp3 {
            aout.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
            aout.SetUINT32(&MF_MT_AAC_PAYLOAD_TYPE, 0)?;
        }
        let ain = MFCreateMediaType()?;
        ain.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
        ain.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)?;
        ain.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
        ain.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, RATE as u32)?;
        ain.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 2)?;
        ain.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, 4)?;
        ain.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, RATE as u32 * 4)?;
        Ok((aout, ain))
    }
}

/// CreateWriter: H.264 (+ AAC) in MP4 or fragmented MP4; AAC (.m4a) or MP3 alone for audio only.
#[allow(clippy::too_many_arguments)]
fn create_writer(s: &RecordSettings, path: &str, fragmented: bool, w: i32, h: i32, hw: bool, audio: bool)
                 -> windows::core::Result<(IMFSinkWriter, u32, u32)> {
    unsafe {
        let mut attr = None;
        MFCreateAttributes(&mut attr, 3)?;
        let attr = attr.unwrap();
        attr.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, hw as u32)?;
        let mp3 = s.audio_only && path.to_lowercase().ends_with(".mp3");
        let container = if mp3 {
            &MFTranscodeContainerType_MP3
        } else if fragmented {
            &MFTranscodeContainerType_FMPEG4
        } else {
            &MFTranscodeContainerType_MPEG4
        };
        attr.SetGUID(&MF_TRANSCODE_CONTAINERTYPE, container)?;
        let wp = to_wide(path);
        let wr = MFCreateSinkWriterFromURL(PCWSTR(wp.as_ptr()), None, &attr)?;
        if s.audio_only {
            let (aout, ain) = audio_types(mp3)?;
            let a = wr.AddStream(&aout)?;
            wr.SetInputMediaType(a, &ain, None)?;
            wr.BeginWriting()?;
            return Ok((wr, 0, a));
        }
        const BPP: [f64; 4] = [0.04, 0.07, 0.10, 0.16];
        let bitrate = (BPP[s.quality.clamp(0, 3) as usize] * w as f64 * h as f64 * s.fps as f64).clamp(400_000.0, 40_000_000.0) as u32;
        let vout = MFCreateMediaType()?;
        vout.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        vout.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
        vout.SetUINT32(&MF_MT_AVG_BITRATE, bitrate)?;
        vout.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        set_size(&vout, &MF_MT_FRAME_SIZE, w as u32, h as u32)?;
        set_size(&vout, &MF_MT_FRAME_RATE, s.fps as u32, 1)?;
        set_size(&vout, &MF_MT_PIXEL_ASPECT_RATIO, 1, 1)?;
        let v = wr.AddStream(&vout)?;
        let vin = MFCreateMediaType()?;
        vin.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        vin.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
        vin.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        set_size(&vin, &MF_MT_FRAME_SIZE, w as u32, h as u32)?;
        set_size(&vin, &MF_MT_FRAME_RATE, s.fps as u32, 1)?;
        set_size(&vin, &MF_MT_PIXEL_ASPECT_RATIO, 1, 1)?;
        wr.SetInputMediaType(v, &vin, None)?;
        let mut a = 0;
        if audio {
            let (aout, ain) = audio_types(false)?;
            a = wr.AddStream(&aout)?;
            wr.SetInputMediaType(a, &ain, None)?;
        }
        wr.BeginWriting()?;
        Ok((wr, v, a))
    }
}

fn delete(p: &str) {
    let _ = std::fs::remove_file(p);
}

// ---------------------------------------------------------------- Recorder
pub struct Recorder {
    sh: Arc<Shared>,
    settings: RecordSettings,
    mic: Option<AudioSource>,
    sys: Option<AudioSource>,
    has_mic: bool,
    has_sys: bool,
    monitor: Option<JoinHandle<()>>,
    video: Option<JoinHandle<()>>,
    audio: Option<JoinHandle<()>>,
    input: Option<JoinHandle<()>>,
    input_tid: Arc<AtomicU32>,
    /// the fragmented recording being written (crash-safe mode); empty = straight to the path
    temp_path: String,
}

impl Default for Recorder {
    fn default() -> Self {
        Self::new()
    }
}

impl Recorder {
    pub fn new() -> Recorder {
        Recorder {
            sh: Arc::new(Shared {
                state: AtomicU8::new(0),
                running: AtomicBool::new(false),
                mic_muted: AtomicBool::new(false),
                sys_muted: AtomicBool::new(false),
                mic_level: AtomicF32::new(0.0),
                sys_level: AtomicF32::new(0.0),
                mic_vol: AtomicI32::new(100),
                sys_vol: AtomicI32::new(100),
                auto_gain: AtomicBool::new(true),
                mic_gain_now: AtomicF32::new(1.0),
                monitoring: AtomicBool::new(false),
                highlight: AtomicBool::new(false),
                clicks: AtomicBool::new(false),
                zoom_on: AtomicBool::new(false),
                key_mode: AtomicI32::new(0),
                clock: Mutex::new(Clock::default()),
                writer: Mutex::new(None),
                input: Mutex::new(Vec::new()),
                first_frame: Mutex::new(Image::default()),
                last_error: Mutex::new(String::new()),
            }),
            settings: RecordSettings::default(),
            mic: None,
            sys: None,
            has_mic: false,
            has_sys: false,
            monitor: None,
            video: None,
            audio: None,
            input: None,
            input_tid: Arc::new(AtomicU32::new(0)),
            temp_path: String::new(),
        }
    }

    pub fn state(&self) -> State {
        self.sh.state()
    }
    /// Seconds recorded (without the pauses).
    pub fn elapsed(&self) -> f64 {
        if self.state() == State::Idle { 0.0 } else { self.sh.now_100ns() as f64 / HNS as f64 }
    }
    pub fn set_mic_muted(&self, m: bool) {
        self.sh.mic_muted.store(m, Relaxed)
    }
    pub fn set_sys_muted(&self, m: bool) {
        self.sh.sys_muted.store(m, Relaxed)
    }
    pub fn mic_muted(&self) -> bool {
        self.sh.mic_muted.load(Relaxed)
    }
    pub fn sys_muted(&self) -> bool {
        self.sh.sys_muted.load(Relaxed)
    }
    pub fn has_mic(&self) -> bool {
        self.has_mic
    }
    pub fn has_sys(&self) -> bool {
        self.has_sys
    }
    /// The levels for the meters, after the volume (and the automatic gain).
    pub fn mic_level(&self) -> f32 {
        (self.sh.mic_level.get() * self.sh.mic_gain_now.get() * self.sh.mic_vol.load(Relaxed) as f32 / 100.0).min(1.0)
    }
    pub fn sys_level(&self) -> f32 {
        (self.sh.sys_level.get() * self.sh.sys_vol.load(Relaxed) as f32 / 100.0).min(1.0)
    }
    /// The volume of each source in % (0–200) and the automatic microphone gain: they change during the recording too.
    pub fn set_mic_volume(&self, v: i32) {
        self.sh.mic_vol.store(v.clamp(0, 200), Relaxed)
    }
    pub fn set_sys_volume(&self, v: i32) {
        self.sh.sys_vol.store(v.clamp(0, 200), Relaxed)
    }
    pub fn set_auto_gain(&self, on: bool) {
        self.sh.auto_gain.store(on, Relaxed)
    }
    pub fn path(&self) -> &str {
        &self.settings.path
    }
    pub fn last_error(&self) -> String {
        self.sh.last_error.lock().unwrap().clone()
    }
    pub fn first_frame(&self) -> Image {
        self.sh.first_frame.lock().unwrap().clone()
    }
    /// Live from the HUD (C07, C15).
    pub fn set_overlay(&self, highlight: bool, clicks: bool, key_mode: i32) {
        self.sh.highlight.store(highlight, Relaxed);
        self.sh.clicks.store(clicks, Relaxed);
        self.sh.key_mode.store(key_mode, Relaxed);
    }
    pub fn highlight_on(&self) -> bool {
        self.sh.highlight.load(Relaxed)
    }
    pub fn clicks_on(&self) -> bool {
        self.sh.clicks.load(Relaxed)
    }
    pub fn key_mode(&self) -> i32 {
        self.sh.key_mode.load(Relaxed)
    }
    pub fn toggle_zoom(&self) {
        self.sh.zoom_on.fetch_xor(true, Relaxed);
    }
    pub fn zoomed(&self) -> bool {
        self.sh.zoom_on.load(Relaxed)
    }
    pub fn monitoring(&self) -> bool {
        self.sh.monitoring.load(Relaxed)
    }

    fn set_error(&self, e: String) {
        *self.sh.last_error.lock().unwrap() = e;
    }

    fn open_sources(&mut self, mic: bool, sys: bool) {
        if mic {
            self.mic = AudioSource::start(false, self.sh.clone());
            self.has_mic = self.mic.is_some();
        }
        if sys {
            self.sys = AudioSource::start(true, self.sh.clone());
            self.has_sys = self.sys.is_some();
        }
    }

    fn close_sources(&mut self) {
        self.mic = None;
        self.sys = None;
    }

    /// Before the recording: the sources capture only for the meters (nothing is written); `start` closes them.
    pub fn start_monitor(&mut self, mic: bool, sys: bool) {
        if self.state() != State::Idle || self.monitoring() {
            return;
        }
        self.open_sources(mic, sys);
        if self.mic.is_none() && self.sys.is_none() {
            return;
        }
        self.sh.monitoring.store(true, Relaxed);
        let sh = self.sh.clone();
        let (mb, sb) = (self.mic.as_ref().map(|s| s.buf.clone()), self.sys.as_ref().map(|s| s.buf.clone()));
        self.monitor = Some(std::thread::spawn(move || {
            // the automatic gain is estimated from the peak, as the mixer does
            let mut env = 0.0f32;
            while sh.monitoring.load(Relaxed) {
                std::thread::sleep(Duration::from_millis(50));
                if let Some(b) = &mb {
                    clear(b);
                }
                if let Some(b) = &sb {
                    clear(b);
                }
                env = sh.mic_level.get().max(env * 0.97);
                sh.mic_gain_now.set(if sh.auto_gain.load(Relaxed) { (0.5 / env.max(1e-4)).clamp(1.0, 8.0) } else { 1.0 });
            }
        }));
    }

    pub fn stop_monitor(&mut self) {
        if !self.monitoring() && self.monitor.is_none() {
            return;
        }
        self.sh.monitoring.store(false, Relaxed);
        if let Some(t) = self.monitor.take() {
            let _ = t.join();
        }
        self.close_sources();
        self.has_mic = false;
        self.has_sys = false;
        self.sh.mic_level.set(0.0);
        self.sh.sys_level.set(0.0);
        self.sh.mic_gain_now.set(1.0);
    }

    pub fn start(&mut self, s: &RecordSettings, notify: Notify) -> bool {
        if self.state() != State::Idle {
            return false;
        }
        self.stop_monitor();
        self.settings = s.clone();
        let sh = self.sh.clone();
        sh.mic_vol.store(s.mic_volume.clamp(0, 200), Relaxed);
        sh.sys_vol.store(s.sys_volume.clamp(0, 200), Relaxed);
        sh.auto_gain.store(s.auto_gain, Relaxed);
        sh.mic_gain_now.set(1.0);
        sh.mic_muted.store(s.mic_muted, Relaxed);
        sh.sys_muted.store(s.sys_muted, Relaxed);
        self.set_error(String::new());
        *sh.first_frame.lock().unwrap() = Image::default();

        // the output size: even, ≤ 1080 lines (optional) and within the H.264 encoder's limit (4096×2304)
        let (mut w, mut h) = (s.rect.width(), s.rect.height());
        if s.audio_only {
            w = 32;
            h = 32;
        }
        if w < 16 || h < 16 {
            self.set_error(crate::tr!("Área de gravação muito pequena."));
            return false;
        }
        self.set_overlay(s.highlight_cursor, s.click_effects, s.key_mode.clamp(0, 2));
        sh.zoom_on.store(false, Relaxed);
        let mut sc = 1.0f64;
        if s.downscale_1080 && h > 1080 {
            sc = 1080.0 / h as f64;
        }
        sc = sc.min(4096.0 / w as f64).min(2304.0 / h as f64);
        let out_w = 32.max(((w as f64 * sc) as i32) & !1);
        let out_h = 32.max(((h as f64 * sc) as i32) & !1);

        if unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }.is_err() {
            self.set_error(crate::tr!("Media Foundation indisponível."));
            return false;
        }
        self.open_sources(s.microphone, s.system_audio);
        let audio = self.has_mic || self.has_sys;
        if s.audio_only && !audio {
            self.set_error(crate::tr!("Nenhuma fonte de áudio disponível (ligue o microfone ou o áudio do sistema)."));
            unsafe {
                let _ = MFShutdown();
            }
            return false;
        }

        // crash-safe (C14): a fragmented MP4 in the recovery folder, remuxed into the path when the recording ends
        self.temp_path.clear();
        if s.crash_safe && !s.audio_only {
            let dir = recovery_dir();
            let guid = String::from_utf16_lossy(&library::new_guid());
            let tp = format!("{dir}\\{guid}.mp4");
            if std::fs::write(format!("{tp}.target"), s.path.as_bytes()).is_ok() {
                self.temp_path = tp;
            }
        }
        let create = |hw: bool, temp: &mut String| -> windows::core::Result<(IMFSinkWriter, u32, u32)> {
            if !temp.is_empty() {
                match create_writer(s, temp, true, out_w, out_h, hw, audio) {
                    Ok(r) => return Ok(r),
                    Err(e) => {
                        log::write(&format!("Recorder: fragmented MP4 rejected (0x{:08X}), recording straight to MP4", e.code().0 as u32));
                        delete(temp);
                        delete(&format!("{temp}.target"));
                        temp.clear();
                    }
                }
            }
            create_writer(s, &s.path, false, out_w, out_h, hw, audio)
        };
        let mut temp = std::mem::take(&mut self.temp_path);
        let mut r = create(s.hardware, &mut temp);
        if let Err(e) = &r
            && s.hardware
            && !s.audio_only
        {
            log::write(&format!("Recorder: hardware encoder rejected (0x{:08X}), trying software", e.code().0 as u32));
            delete(&s.path);
            r = create(false, &mut temp);
        }
        self.temp_path = temp;
        let (wr, vstream, astream) = match r {
            Ok(x) => x,
            Err(e) => {
                let msg = crate::i18n::fill(&crate::tr!("Não foi possível iniciar o codificador MP4 (0x%08X)."),
                                            &[&format!("{:08X}", e.code().0 as u32)])
                    .replace("0x0x", "0x");
                log::write(&format!("Recorder: {msg}"));
                self.set_error(msg);
                self.close_sources();
                unsafe {
                    let _ = MFShutdown();
                }
                return false;
            }
        };
        *sh.writer.lock().unwrap() = Some(SendWriter(wr));
        {
            let mut c = sh.clock.lock().unwrap();
            unsafe {
                let _ = QueryPerformanceFrequency(&mut c.freq);
            }
            c.start = qpc();
            c.paused_total = 0;
            c.pause_start = 0;
        }
        sh.set_state(State::Recording);
        sh.running.store(true, Relaxed);
        unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED) }; // no sleep meanwhile
        if !s.audio_only {
            sh.input.lock().unwrap().clear();
            *HOOKED.lock().unwrap() = Some(sh.clone());
            let tid = self.input_tid.clone();
            self.input = Some(std::thread::spawn(move || input_thread(&tid)));
            let (sh2, s2, n2) = (sh.clone(), s.clone(), notify.clone());
            self.video = Some(std::thread::spawn(move || video_thread(&sh2, &s2, out_w, out_h, vstream, &n2)));
        }
        if audio {
            let sh2 = sh.clone();
            let (mb, sb) = (self.mic.as_ref().map(|x| x.buf.clone()), self.sys.as_ref().map(|x| x.buf.clone()));
            self.audio = Some(std::thread::spawn(move || audio_mix_thread(&sh2, mb, sb, astream)));
        }
        log::write(&format!("Recorder: started {w}x{h} → {out_w}x{out_h} @{} fps, audio mic={} sys={}, {}", s.fps,
                            self.has_mic as i32, self.has_sys as i32, s.path));
        true
    }

    pub fn pause(&self) {
        if self.state() != State::Recording {
            return;
        }
        let mut c = self.sh.clock.lock().unwrap();
        c.pause_start = Shared::elapsed_raw(&c);
        self.sh.set_state(State::Paused);
    }

    pub fn resume(&self) {
        if self.state() != State::Paused {
            return;
        }
        {
            let mut c = self.sh.clock.lock().unwrap();
            let t = Shared::elapsed_raw(&c);
            c.paused_total += t - c.pause_start;
            self.sh.set_state(State::Recording);
        }
        for s in [&self.mic, &self.sys].into_iter().flatten() {
            clear(&s.buf);
        }
    }

    /// An image of the area during the recording.
    pub fn take_snapshot(&self) -> Image {
        crate::screen::grab(self.settings.rect)
    }

    /// Stops and finalizes the MP4; false (with `last_error`) when it failed.
    pub fn stop(&mut self) -> bool {
        if self.state() == State::Idle {
            return false;
        }
        self.sh.set_state(State::Stopping);
        self.sh.running.store(false, Relaxed);
        if let Some(t) = self.video.take() {
            let _ = t.join();
        }
        if let Some(t) = self.audio.take() {
            let _ = t.join();
        }
        if let Some(t) = self.input.take() {
            loop {
                let tid = self.input_tid.load(Relaxed);
                if tid != 0 && unsafe { PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0)) }.is_ok() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            let _ = t.join();
            self.input_tid.store(0, Relaxed);
        }
        *HOOKED.lock().unwrap() = None;
        self.close_sources();
        let mut ok = false;
        if let Some(w) = self.sh.writer.lock().unwrap().take() {
            match unsafe { w.0.Finalize() } {
                Ok(()) => ok = true,
                Err(e) => {
                    let c = e.code().0 as u32;
                    log::write(&format!("Recorder: Finalize 0x{c:08X}"));
                    self.set_error(crate::i18n::fill(&crate::tr!("Falha ao finalizar o MP4 (0x%08X)."), &[&format!("{c:08X}")])
                        .replace("0x0x", "0x"));
                }
            }
        }
        // crash-safe recording (C14): the fragmented file becomes a regular MP4 (no re-encoding)
        if !self.temp_path.is_empty() {
            let tp = std::mem::take(&mut self.temp_path);
            if ok {
                if let Err(err) = remux_to_mp4(&tp, &self.settings.path) {
                    log::write(&format!("Recorder: remux failed ({err}), keeping the fragmented MP4"));
                    delete(&self.settings.path);
                    ok = std::fs::rename(&tp, &self.settings.path).is_ok()
                        || std::fs::copy(&tp, &self.settings.path).is_ok();
                }
            }
            delete(&tp);
            delete(&format!("{tp}.target"));
        }
        unsafe {
            let _ = MFShutdown();
            SetThreadExecutionState(ES_CONTINUOUS);
        }
        self.sh.set_state(State::Idle);
        self.sh.mic_level.set(0.0);
        self.sh.sys_level.set(0.0);
        self.has_mic = false;
        self.has_sys = false;
        log::write(&format!("Recorder: stopped ({})", if ok { "ok" } else { "error" }));
        ok
    }

    /// Stops and deletes what was recorded.
    pub fn cancel(&mut self) {
        let temp = std::mem::take(&mut self.temp_path); // nothing to remux: the recording is thrown away
        self.stop();
        if !temp.is_empty() {
            delete(&temp);
            delete(&format!("{temp}.target"));
        }
        delete(&self.settings.path);
        log::write("Recorder: canceled");
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        if self.sh.running.load(Relaxed) {
            self.cancel();
        }
        self.stop_monitor();
    }
}

fn key_name(vk: u32) -> String {
    let named = [
        (VK_RETURN, "Enter"), (VK_ESCAPE, "Esc"), (VK_TAB, "Tab"), (VK_BACK, "⌫"), (VK_DELETE, "Del"), (VK_LEFT, "←"),
        (VK_RIGHT, "→"), (VK_UP, "↑"), (VK_DOWN, "↓"), (VK_HOME, "Home"), (VK_END, "End"), (VK_PRIOR, "PgUp"),
        (VK_NEXT, "PgDn"), (VK_INSERT, "Ins"), (VK_SNAPSHOT, "PrtSc"),
    ];
    if vk == VK_SPACE.0 as u32 {
        return crate::tr!("Espaço");
    }
    if let Some((_, n)) = named.iter().find(|(k, _)| k.0 as u32 == vk) {
        return n.to_string();
    }
    if (VK_F1.0 as u32..=VK_F24.0 as u32).contains(&vk) {
        return format!("F{}", vk - VK_F1.0 as u32 + 1);
    }
    if (b'0' as u32..=b'9' as u32).contains(&vk) || (b'A' as u32..=b'Z' as u32).contains(&vk) {
        return char::from_u32(vk).unwrap().to_string();
    }
    let sc = unsafe { MapVirtualKeyW(vk, MAPVK_VK_TO_VSC) };
    let mut b = [0u16; 64];
    if sc != 0 {
        let n = unsafe { GetKeyNameTextW((sc << 16) as i32, &mut b) };
        if n > 0 {
            return String::from_utf16_lossy(&b[..n as usize]);
        }
    }
    format!("#{vk}")
}

fn colorref_argb(a: u32, c: u32) -> u32 {
    (a << 24) | ((c & 0xFF) << 16) | (c & 0xFF00) | ((c >> 16) & 0xFF)
}

struct Ripple {
    p: (i32, i32),
    t0: u64,
    right: bool,
}

fn video_thread(sh: &Shared, s: &RecordSettings, out_w: i32, out_h: i32, stream: u32, notify: &Notify) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        timeBeginPeriod(1);
    }
    let r = s.rect;
    let (sw, sh_) = (r.width(), r.height());
    let screen = unsafe { GetDC(None) };
    let mem = unsafe { CreateCompatibleDC(Some(screen)) };
    let bi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: out_w,
            biHeight: out_h, // bottom-up: Media Foundation's RGB32
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = std::ptr::null_mut();
    let Ok(dib) = (unsafe { CreateDIBSection(Some(screen), &bi, DIB_RGB_COLORS, &mut bits, None, 0) }) else {
        unsafe {
            let _ = DeleteDC(mem);
            ReleaseDC(None, screen);
            timeEndPeriod(1);
            CoUninitialize();
        }
        *sh.last_error.lock().unwrap() = crate::tr!("Falha ao gravar quadro de vídeo.");
        notify(Note::Error);
        return;
    };
    let old = unsafe { SelectObject(mem, dib.into()) };
    let scaled = out_w != sw || out_h != sh_;
    unsafe {
        SetStretchBltMode(mem, HALFTONE);
        let _ = SetBrushOrgEx(mem, 0, 0, None);
    }
    let frame_dur = HNS / s.fps.max(1) as i64;
    let bytes = (out_w * out_h * 4) as usize;
    let mut last_ts = -1i64;
    let mut last_disk = 0u64;
    let mut warned_disk = false;
    let mut ripples: Vec<Ripple> = Vec::new();
    let mut key_text = String::new();
    let mut key_time = 0u64;
    let (mut zoom_now, mut zcx, mut zcy) = (1.0f64, (r.left + r.right) as f64 / 2.0, (r.top + r.bottom) as f64 / 2.0);
    let mut auto_zoom_until = 0u64;
    let dir: Vec<u16> = to_wide(std::path::Path::new(&s.path).parent().map(|p| p.as_os_str()).unwrap_or_default());

    while sh.running.load(Relaxed) {
        if sh.state() == State::Paused {
            std::thread::sleep(Duration::from_millis(10));
            continue;
        }
        let t = sh.now_100ns();
        let tick = unsafe { GetTickCount64() };
        let mut ci = CURSORINFO { cbSize: std::mem::size_of::<CURSORINFO>() as u32, ..Default::default() };
        let have_cursor = unsafe { GetCursorInfo(&mut ci) }.is_ok();
        let showing = have_cursor && ci.flags.0 & CURSOR_SHOWING.0 != 0;
        // the clicks and keys the hooks saw since the last frame (C07, and the C15 auto zoom)
        let events: Vec<InputEvent> = std::mem::take(&mut *sh.input.lock().unwrap());
        let clicks = sh.clicks.load(Relaxed);
        for e in events.iter().filter(|e| !e.key) {
            // (the hook's time may be a little after this frame's clock: the age must not go below zero)
            if clicks {
                ripples.push(Ripple { p: e.pt, t0: e.tick.min(tick), right: e.code == VK_RBUTTON.0 as u32 });
            }
            if s.auto_zoom {
                auto_zoom_until = e.tick + 2500;
            }
        }
        // zoom (C15): eases toward the target and follows the pointer
        let target = if sh.zoom_on.load(Relaxed) || tick < auto_zoom_until { s.zoom_percent.clamp(120, 400) as f64 / 100.0 } else { 1.0 };
        zoom_now += (target - zoom_now) * 0.18;
        if (target - zoom_now).abs() < 0.004 {
            zoom_now = target;
        }
        if have_cursor {
            zcx += (ci.ptScreenPos.x as f64 - zcx) * 0.2;
            zcy += (ci.ptScreenPos.y as f64 - zcy) * 0.2;
        }
        let (src_w, src_h) = (sw as f64 / zoom_now, sh_ as f64 / zoom_now);
        let (mut src_l, mut src_t) = ((zcx - src_w / 2.0).clamp(r.left as f64, (r.right as f64 - src_w).max(r.left as f64)),
                                      (zcy - src_h / 2.0).clamp(r.top as f64, (r.bottom as f64 - src_h).max(r.top as f64)));
        if zoom_now == 1.0 {
            src_l = r.left as f64;
            src_t = r.top as f64;
        }
        let (kx, ky) = (out_w as f64 / src_w, out_h as f64 / src_h); // screen → output
        let to_out = |p: (i32, i32)| (((p.0 as f64 - src_l) * kx) as f32, ((p.1 as f64 - src_t) * ky) as f32);
        unsafe {
            let rop = SRCCOPY | CAPTUREBLT;
            if zoom_now != 1.0 {
                let _ = StretchBlt(mem, 0, 0, out_w, out_h, Some(screen), src_l.round() as i32, src_t.round() as i32,
                                   src_w.round() as i32, src_h.round() as i32, rop);
            } else if scaled {
                let _ = StretchBlt(mem, 0, 0, out_w, out_h, Some(screen), r.left, r.top, sw, sh_, rop);
            } else {
                let _ = BitBlt(mem, 0, 0, out_w, out_h, Some(screen), r.left, r.top, rop);
            }
        }
        let highlight = sh.highlight.load(Relaxed);
        let key_mode = sh.key_mode.load(Relaxed);
        let overlays = highlight || !ripples.is_empty() || key_mode > 0;
        let px = kx.min(ky) as f32; // a screen pixel in output pixels
        let mut g = overlays.then(|| {
            let g = Graphics::from_hdc(mem.0 as isize);
            g.smoothing(SmoothingModeAntiAlias);
            g
        });
        if highlight && showing
            && let Some(g) = &g
        {
            // a halo under the cursor (C07)
            let c = to_out((ci.ptScreenPos.x, ci.ptScreenPos.y));
            let rad = s.highlight_size as f32 / 2.0 * px;
            let a = (s.highlight_opacity.clamp(5, 100) * 255 / 100) as u32;
            g.fill_ellipse(&Brush::solid(colorref_argb(a, s.highlight_color)), c.0 - rad, c.1 - rad, 2.0 * rad, 2.0 * rad);
        }
        if s.cursor && showing {
            let mut ii = ICONINFO::default();
            let icon = HICON(ci.hCursor.0);
            if unsafe { GetIconInfo(icon, &mut ii) }.is_ok() {
                if let Some(g) = &g {
                    g.flush();
                }
                // the cursor follows the scale of the picture (smaller when downscaled, bigger when zoomed in)
                let c = to_out((ci.ptScreenPos.x, ci.ptScreenPos.y));
                let custom = (px - 1.0).abs() > 0.01;
                let cs = (unsafe { GetSystemMetrics(SM_CXCURSOR) } as f32 * px).round() as i32;
                let (cx, cy) = ((c.0 - ii.xHotspot as f32 * px).round() as i32, (c.1 - ii.yHotspot as f32 * px).round() as i32);
                unsafe {
                    let _ = DrawIconEx(mem, cx, cy, icon, if custom { cs } else { 0 }, if custom { cs } else { 0 }, 0, None, DI_NORMAL);
                    if !ii.hbmMask.is_invalid() {
                        let _ = DeleteObject(ii.hbmMask.into());
                    }
                    if !ii.hbmColor.is_invalid() {
                        let _ = DeleteObject(ii.hbmColor.into());
                    }
                }
            }
        }
        // click ripples (C07): an expanding ring over a fading disc for 700 ms, red for the left button, blue for the right
        if !ripples.is_empty()
            && let Some(g) = &g
        {
            const LIFE: f32 = 700.0;
            ripples.retain(|rp| tick.saturating_sub(rp.t0) as f32 <= LIFE);
            for rp in &ripples {
                let prog = (tick - rp.t0) as f32 / LIFE;
                let ease = 1.0 - (1.0 - prog) * (1.0 - prog);
                let cc = if rp.right { s.right_click_color } else { s.left_click_color };
                let c = to_out(rp.p);
                let rad = (10.0 + 38.0 * ease) * px;
                g.fill_ellipse(&Brush::solid(colorref_argb((90.0 * (1.0 - prog)) as u32, cc)), c.0 - rad, c.1 - rad, 2.0 * rad, 2.0 * rad);
                let pen = Pen::new(colorref_argb((240.0 * (1.0 - prog)) as u32, cc), (5.0 * px).max(3.0));
                g.draw_ellipse(&pen, c.0 - rad, c.1 - rad, 2.0 * rad, 2.0 * rad);
                g.fill_ellipse(&Brush::solid(colorref_argb((200.0 * (1.0 - prog)) as u32, cc)), c.0 - 6.0 * px, c.1 - 6.0 * px,
                               12.0 * px, 12.0 * px);
            }
        }
        // keys on screen (C07): shortcuts only by default — typed text (passwords) stays out unless "every key"
        if key_mode > 0
            && let Some(g) = &g
        {
            for e in events.iter().filter(|e| e.key) {
                let vk = e.code;
                let modifier = [VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN, VK_CAPITAL, VK_NUMLOCK].iter().any(|k| k.0 as u32 == vk)
                    || (VK_LSHIFT.0 as u32..=VK_RMENU.0 as u32).contains(&vk)
                    || vk >= 256;
                if modifier {
                    continue;
                }
                let fkey = (VK_F1.0 as u32..=VK_F24.0 as u32).contains(&vk);
                let shortcut = e.ctrl || e.alt || e.win || fkey;
                if key_mode == 2 || shortcut {
                    let combo = format!("{}{}{}{}{}", if e.ctrl { "Ctrl + " } else { "" }, if e.alt { "Alt + " } else { "" },
                                        if e.win { "Win + " } else { "" }, if e.shift && shortcut { "Shift + " } else { "" }, key_name(vk));
                    let typing = key_mode == 2 && !shortcut && combo.chars().count() == 1;
                    let lower = combo.to_lowercase();
                    if typing && tick - key_time < 1500 && key_text.chars().count() < 28 && !key_text.is_empty() && !key_text.contains('+') {
                        key_text += if e.shift { &combo } else { &lower };
                    } else {
                        key_text = if typing && !e.shift { lower } else { combo };
                    }
                    key_time = tick;
                }
            }
            if !key_text.is_empty() && tick - key_time < 1800 {
                let fade = if tick - key_time > 1400 { 1.0 - (tick - key_time - 1400) as f32 / 400.0 } else { 1.0 };
                let em = (out_h as f32 / 20.0).max(14.0);
                let font = Font::new("Segoe UI", em, FontStyleBold);
                let kt: Vec<u16> = key_text.encode_utf16().collect();
                let fmt = StringFormat::new();
                let bx = g.measure_at(&kt, &font, gdip::pt(0.0, 0.0), &fmt);
                let (bw, bh) = (bx.Width + em * 1.2, bx.Height + em * 0.5);
                let bg = gdip::rect((out_w as f32 - bw) / 2.0, out_h as f32 - bh - out_h as f32 * 0.06, bw, bh);
                let path = Path::new();
                let rr = bh * 0.3;
                path.arc(bg.X, bg.Y, 2.0 * rr, 2.0 * rr, 180.0, 90.0);
                path.arc(bg.X + bg.Width - 2.0 * rr, bg.Y, 2.0 * rr, 2.0 * rr, 270.0, 90.0);
                path.arc(bg.X + bg.Width - 2.0 * rr, bg.Y + bg.Height - 2.0 * rr, 2.0 * rr, 2.0 * rr, 0.0, 90.0);
                path.arc(bg.X, bg.Y + bg.Height - 2.0 * rr, 2.0 * rr, 2.0 * rr, 90.0, 90.0);
                path.close_figure();
                g.fill_path(&Brush::solid(((190.0 * fade) as u32) << 24 | 0x14161A), &path);
                let center = StringFormat::new();
                center.align(StringAlignmentCenter);
                center.line_align(StringAlignmentCenter);
                g.text_hint(TextRenderingHintAntiAlias);
                g.draw_string(&kt, &font, bg, &center, &Brush::solid(((255.0 * fade) as u32) << 24 | 0xFFFFFF));
            }
        }
        g = None;
        drop(g);
        unsafe {
            let _ = GdiFlush();
        }
        let frame = unsafe { std::slice::from_raw_parts(bits as *const u8, bytes) };
        // the BitBlt alpha is undefined: the encoder ignores it, the thumbnail needs it opaque
        {
            let mut ff = sh.first_frame.lock().unwrap();
            if ff.is_empty() {
                let mut f = Image::sized(out_w, out_h, 0);
                let src = frame.as_ptr() as *const u32;
                for y in 0..out_h as usize {
                    let row = unsafe { std::slice::from_raw_parts(src.add((out_h as usize - 1 - y) * out_w as usize), out_w as usize) };
                    f.px[y * out_w as usize..(y + 1) * out_w as usize].copy_from_slice(row);
                }
                f.make_opaque();
                *ff = f;
            }
        }
        let ts = t.max(last_ts + 1);
        if !sh.write_sample(stream, frame, ts, frame_dur) {
            *sh.last_error.lock().unwrap() = crate::tr!("Falha ao gravar quadro de vídeo.");
            notify(Note::Error);
            break;
        }
        last_ts = ts;
        // the disk space limits (warn / stop)
        let now = unsafe { GetTickCount64() };
        if now - last_disk > 2000 {
            last_disk = now;
            let mut free = 0u64;
            if unsafe { GetDiskFreeSpaceExW(PCWSTR(dir.as_ptr()), Some(&mut free), None, None) }.is_ok() {
                if free < 200 * 1024 * 1024 {
                    notify(Note::DiskFull);
                } else if !warned_disk && free < 1024 * 1024 * 1024 {
                    warned_disk = true;
                    notify(Note::DiskLow);
                }
            }
        }
        // waits for the next frame on the time grid
        let next = (t / frame_dur + 1) * frame_dur;
        loop {
            if !sh.running.load(Relaxed) || sh.state() == State::Paused {
                break;
            }
            let now = sh.now_100ns();
            if now >= next {
                break;
            }
            let ms = ((next - now) / 10000).clamp(1, 50);
            std::thread::sleep(Duration::from_millis(ms as u64));
        }
    }
    unsafe {
        SelectObject(mem, old);
        let _ = DeleteObject(dib.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        timeEndPeriod(1);
        CoUninitialize();
    }
}

fn audio_mix_thread(sh: &Shared, mic: Option<Buf>, sys: Option<Buf>, stream: u32) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let mut written = 0i64; // frames already written
    let (mut a, mut b): (Vec<f32>, Vec<f32>) = (Vec::new(), Vec::new());
    let mut out: Vec<i16> = Vec::new();
    // automatic microphone gain: laptop microphones arrive 20–30 dB below ideal. The peak envelope targets -6 dBFS with
    // a gain of 1× to 8×; it drops at once (no clipping) and rises slowly (no noise "pumping").
    let (mut mic_env, mut mic_gain) = (0.0f32, 1.0f32);
    while sh.running.load(Relaxed) {
        std::thread::sleep(Duration::from_millis(20));
        if sh.state() == State::Paused {
            for s in [&mic, &sys].into_iter().flatten() {
                clear(s);
            }
            continue;
        }
        let target = sh.now_100ns() * RATE / HNS;
        let n = (target - written).min(RATE);
        if n <= 0 {
            continue;
        }
        let nu = n as usize;
        a.clear();
        a.resize(nu * 2, 0.0);
        b.clear();
        b.resize(nu * 2, 0.0);
        if let Some(m) = &mic {
            pop(m, &mut a, nu);
        }
        if let Some(s) = &sys {
            pop(s, &mut b, nu);
        }
        let (mm, sm) = (sh.mic_muted.load(Relaxed), sh.sys_muted.load(Relaxed));
        if mic.is_some() {
            let peak = a.iter().fold(0.0f32, |p, s| p.max(s.abs()));
            mic_env = peak.max(mic_env * 0.97);
            if sh.auto_gain.load(Relaxed) {
                let want = (0.5 / mic_env.max(1e-4)).clamp(1.0, 8.0);
                mic_gain = if want < mic_gain { want } else { mic_gain + (want - mic_gain) * 0.02 };
            } else {
                mic_gain = 1.0; // manual: only the volume
            }
            sh.mic_gain_now.set(mic_gain);
            let k = mic_gain * sh.mic_vol.load(Relaxed) as f32 / 100.0;
            a.iter_mut().for_each(|s| *s *= k);
        }
        let sv = sh.sys_vol.load(Relaxed);
        if sys.is_some() && sv != 100 {
            let k = sv as f32 / 100.0;
            b.iter_mut().for_each(|s| *s *= k);
        }
        out.clear();
        out.extend(a.iter().zip(b.iter()).map(|(&x, &y)| {
            let v = if mm { 0.0 } else { x } + if sm { 0.0 } else { y };
            let v = v / (1.0 + v.abs() * 0.15) * 1.15; // a soft limiter: mic and system together do not clip
            (v.clamp(-1.0, 1.0) * 32767.0).round() as i16
        }));
        let (t0, t1) = (written * HNS / RATE, (written + n) * HNS / RATE);
        let bytes = unsafe { std::slice::from_raw_parts(out.as_ptr() as *const u8, out.len() * 2) };
        sh.write_sample(stream, bytes, t0, t1 - t0);
        written += n;
    }
    unsafe { CoUninitialize() };
}

// ---------------------------------------------------------------- crash recovery (C14)
pub fn recovery_dir() -> String {
    let d = paths::app_data_dir().join("Recovery");
    let _ = std::fs::create_dir_all(&d);
    d.to_string_lossy().into_owned()
}

/// Copies the streams of `inp` into a regular MP4 without re-encoding (Source Reader → Sink Writer passthrough); a file
/// cut short by a crash is copied up to where it can be read.
pub fn remux_to_mp4(inp: &str, out: &str) -> Result<(), String> {
    if unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }.is_err() {
        return Err("MFStartup".into());
    }
    let r = (|| -> windows::core::Result<bool> {
        unsafe {
            let wi = to_wide(inp);
            let rd = MFCreateSourceReaderFromURL(PCWSTR(wi.as_ptr()), None)?;
            let mut attr = None;
            MFCreateAttributes(&mut attr, 1)?;
            let attr = attr.unwrap();
            attr.SetGUID(&MF_TRANSCODE_CONTAINERTYPE, &MFTranscodeContainerType_MPEG4)?;
            let wo = to_wide(out);
            let wr = MFCreateSinkWriterFromURL(PCWSTR(wo.as_ptr()), None, &attr)?;
            let mut map: Vec<(u32, u32)> = Vec::new(); // reader stream → writer stream
            let mut i = 0u32;
            loop {
                let mt = match rd.GetNativeMediaType(i, 0) {
                    Ok(m) => m,
                    Err(e) if e.code() == MF_E_INVALIDSTREAMNUMBER => break,
                    Err(e) => return Err(e),
                };
                let major = mt.GetGUID(&MF_MT_MAJOR_TYPE)?;
                if major != MFMediaType_Video && major != MFMediaType_Audio {
                    rd.SetStreamSelection(i, false)?;
                } else {
                    rd.SetStreamSelection(i, true)?;
                    rd.SetCurrentMediaType(i, None, &mt)?;
                    map.push((i, wr.AddStream(&mt)?));
                }
                i += 1;
            }
            if map.is_empty() {
                return Ok(false);
            }
            wr.BeginWriting()?;
            let mut count = 0u64;
            while !map.is_empty() {
                let (mut idx, mut flags, mut ts) = (0u32, 0u32, 0i64);
                let mut smp: Option<IMFSample> = None;
                const ANY_STREAM: u32 = 0xFFFF_FFFE;
                if rd.ReadSample(ANY_STREAM, 0, Some(&mut idx), Some(&mut flags), Some(&mut ts), Some(&mut smp)).is_err() || flags & 1 != 0 {
                    break; // cut short: what was read is kept
                }
                if flags & 2 != 0 {
                    // end of stream
                    map.retain(|&(r, _)| r != idx);
                    continue;
                }
                let (Some(smp), Some(&(_, ws))) = (smp, map.iter().find(|&&(r, _)| r == idx)) else { continue };
                if wr.WriteSample(ws, &smp).is_err() {
                    break;
                }
                count += 1;
            }
            if count == 0 {
                return Ok(false);
            }
            wr.Finalize()?;
            Ok(true)
        }
    })();
    unsafe {
        let _ = MFShutdown();
    }
    match r {
        Ok(true) => Ok(()),
        Ok(false) => {
            delete(out);
            Err("0x80004005".into())
        }
        Err(e) => {
            delete(out);
            Err(format!("0x{:08X}", e.code().0 as u32))
        }
    }
}

/// Recordings a crash interrupted are left in the recovery folder with a ".target" file naming the video they were
/// meant to become: finishes them (at startup) and returns the recovered videos.
pub fn recover_interrupted() -> Vec<String> {
    let mut done = Vec::new();
    let dir = recovery_dir();
    let Ok(rd) = std::fs::read_dir(&dir) else { return done };
    let files: Vec<std::path::PathBuf> =
        rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("mp4"))).collect();
    for f in files {
        let fs = f.to_string_lossy().into_owned();
        let mut target = std::fs::read(format!("{fs}.target")).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
        if target.is_empty() {
            target = format!("{}\\{}", String::from_utf16_lossy(&library::library_dir()), f.file_name().unwrap().to_string_lossy());
        }
        // never over an existing video
        let mut n = 2;
        while std::path::Path::new(&target).exists() {
            let base = &target[..target.len() - 4];
            let base = match base.rfind("_r") {
                Some(us) if base[us + 2..].chars().next().is_some_and(|c| c.is_ascii_digit()) => &base[..us],
                _ => base,
            };
            target = format!("{base}_r{n}.mp4");
            n += 1;
        }
        if let Some(d) = std::path::Path::new(&target).parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let size = std::fs::metadata(&f).map(|m| m.len()).unwrap_or(0);
        let res = if size > 4096 { remux_to_mp4(&fs, &target) } else { Err(String::new()) };
        let ok = res.is_ok();
        if ok {
            done.push(target.clone());
        }
        log::write(&format!("Recorder: recovery of {fs} → {target}: {}",
                            match &res { Ok(()) => "ok".to_string(), Err(e) => format!("failed {e}") }));
        if ok || size <= 4096 {
            delete(&fs);
            delete(&format!("{fs}.target"));
        }
    }
    done
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pops_and_drops_the_drift() {
        let b: Buf = Arc::new(Mutex::new((0..20).map(|v| v as f32).collect()));
        let mut out = [0.0f32; 8];
        assert_eq!(pop(&b, &mut out, 4), 4);
        assert_eq!(out, [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]);
        let mut out2 = [0.0f32; 20];
        assert_eq!(pop(&b, &mut out2, 10), 6, "only what there is");
        // more than 200 ms beyond what is asked: the oldest excess goes, 50 ms stay ahead
        let b: Buf = Arc::new(Mutex::new(vec![0.0; (RATE as usize / 5 + 101) * 2].into()));
        let mut out = vec![0.0f32; 200];
        pop(&b, &mut out, 100);
        assert_eq!(b.lock().unwrap().len() / 2, RATE as usize / 20);
    }

    #[test]
    fn names_keys() {
        assert_eq!(key_name(VK_F1.0 as u32 + 4), "F5");
        assert_eq!(key_name(b'A' as u32), "A");
        assert_eq!(key_name(VK_RETURN.0 as u32), "Enter");
    }

    #[test]
    fn records_and_remuxes_a_short_video() {
        // a real recording of a small screen area through the crash-safe path, then its remux
        let dir = std::env::temp_dir().join(format!("snapnab-rec-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.mp4").to_string_lossy().into_owned();
        let mut r = Recorder::new();
        let s = RecordSettings {
            rect: Rect::new(0, 0, 320, 240),
            microphone: false,
            system_audio: false,
            fps: 15,
            crash_safe: false,
            path: path.clone(),
            ..Default::default()
        };
        assert!(r.start(&s, Arc::new(|_| {})), "{}", r.last_error());
        std::thread::sleep(Duration::from_millis(700));
        r.pause();
        assert_eq!(r.state(), State::Paused);
        let e = r.elapsed();
        std::thread::sleep(Duration::from_millis(300));
        assert!((r.elapsed() - e).abs() < 0.05, "the clock stops while paused");
        r.resume();
        std::thread::sleep(Duration::from_millis(400));
        assert!(r.stop());
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[4..8], b"ftyp");
        assert_eq!((r.first_frame().w, r.first_frame().h), (320, 240));
        let out = dir.join("r.mp4").to_string_lossy().into_owned();
        remux_to_mp4(&path, &out).unwrap();
        assert!(std::fs::metadata(&out).unwrap().len() > 1000);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
