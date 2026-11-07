//! The notification channel between the capture app and the Editor (Ipc.cpp; ds_pseudocode §4, ds_arch §19.8,
//! ed_comp_14 §3): one 4 KiB shared mapping guarded by a named mutex, an event per channel that wakes its receiver,
//! and an acknowledgement event per channel the sender can wait on. A message is `[u32 code][u32 kind][payload]` with
//! kind 0 = i32, 1 = 16 bytes, 2 = NUL-terminated UTF-16. Every kernel object keeps the name the C++ programs use, so
//! a Rust program talks to a C++ one while the move goes on.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

use windows::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0};
use windows::Win32::System::Memory::{
    CreateFileMappingW, FILE_MAP_READ, FILE_MAP_WRITE, FlushViewOfFile, MapViewOfFile, PAGE_READWRITE, UnmapViewOfFile,
};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, INFINITE, OpenMutexW, ReleaseMutex, ResetEvent, SYNCHRONIZATION_SYNCHRONIZE, SetEvent,
    WaitForMultipleObjects, WaitForSingleObject,
};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, IsHungAppWindow};
use windows::core::PCWSTR;

use crate::log;
use crate::wide::to_wide;

pub const CAPTURE_MUTEX: &str = "Snapnab-Capture-{41F1BF9C-5A4E-4B6F-9C33-1D2E3F405A61}";
pub const EDITOR_MUTEX: &str = "Snapnab-Editor-{800F2EB3-7C1D-4E2A-A3B4-C5D6E7F80912}";
pub const CAPTURE_WND_CLASS: &str = "SnapnabUI";
pub const EDITOR_WND_CLASS: &str = "SnapnabEditor";

const MAP_SIZE: u32 = 0x1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Channel {
    Capture = 0,
    Editor = 1,
}

pub const KIND_INT: u32 = 0;
pub const KIND_BLOB: u32 = 1;
pub const KIND_STRING: u32 = 2;

/// The message codes (the original's numbering where its meaning is known).
pub mod code {
    // capture app → Editor
    pub const SHOW_EDITOR: u32 = 0x06;
    pub const CLOSE_EDITOR_IF_HIDDEN: u32 = 0x0A;
    pub const RELOAD_CONFIG: u32 = 0x0C;
    pub const SHOW_EDITOR_WITH_PARAM: u32 = 0x0E;
    pub const NEW_IMAGE_FROM_CLIPBOARD: u32 = 0x1E;
    pub const OPEN_FILE: u32 = 0x30;
    pub const LIBRARY_CHANGED: u32 = 0x31;
    pub const RESTART_EDITOR: u32 = 0x32;
    // Editor → capture app
    pub const LIBRARY_DOC_OPENED: u32 = 0x02;
    pub const LIBRARY_DOC_RELEASED: u32 = 0x03;
    pub const EDITOR_START_CAPTURE: u32 = 0x12;
    pub const EDITOR_CLOSED: u32 = 0x13;
    pub const PIN_FILE: u32 = 0x40;
    pub const EDITOR_OPTION_CHANGED: u32 = 0x17;
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Message {
    pub code: u32,
    pub kind: u32,
    pub ivalue: i32,
    pub blob: [u8; 16],
    pub text: String,
}

/// The names of the kernel objects. `prefix` is empty for the real programs; the tests use their own so they never
/// reach a running Snapnab.
#[derive(Clone, Debug, Default)]
pub struct Names {
    pub prefix: String,
}

impl Names {
    fn n(&self, s: &str) -> Vec<u16> {
        to_wide(format!("{}{s}", self.prefix))
    }
    fn map(&self) -> Vec<u16> {
        self.n("SnapnabIPCNotifyMemMapFile")
    }
    fn mutex(&self) -> Vec<u16> {
        self.n("SnapnabIPCNotifyMutex")
    }
    fn event(&self, ch: Channel) -> Vec<u16> {
        self.n(&format!("SnapnabIPCNotifyEvent{}", ch as u32))
    }
    fn ack(&self, ch: Channel) -> Vec<u16> {
        self.n(&format!("SnapnabIPCNotifyWaitUntilMessageSentEvent{}", ch as u32))
    }
    fn alive(&self, ch: Channel) -> Vec<u16> {
        self.n(&format!("SnapnabIPCNotifyThreadStartedMutex{}", ch as u32))
    }
    /// The single-instance mutex of the program behind a channel.
    pub fn program_mutex(&self, ch: Channel) -> Vec<u16> {
        self.n(if ch == Channel::Editor { EDITOR_MUTEX } else { CAPTURE_MUTEX })
    }
}

/// A handle that crosses to the listener thread (the kernel objects are thread-agnostic).
#[derive(Clone, Copy)]
struct H(HANDLE);
unsafe impl Send for H {}
unsafe impl Sync for H {}

fn close(h: HANDLE) {
    if !h.is_invalid() && !h.0.is_null() {
        unsafe {
            let _ = CloseHandle(h);
        }
    }
}

/// True while a named mutex exists (the single-instance mutex of a running program).
pub fn mutex_exists(name: &[u16]) -> bool {
    match unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, PCWSTR(name.as_ptr())) } {
        Ok(h) => {
            close(h);
            true
        }
        Err(_) => false,
    }
}

fn open_map(names: &Names) -> Option<HANDLE> {
    let n = names.map();
    unsafe { CreateFileMappingW(INVALID_HANDLE_VALUE, None, PAGE_READWRITE, 0, MAP_SIZE, PCWSTR(n.as_ptr())) }.ok()
}

/// The receiving end of a channel plus the sending of messages to the other one.
pub struct Notify {
    me: Channel,
    names: Names,
    running: Arc<AtomicBool>,
    stop: H,
    keep: Vec<H>,
    thread: Option<JoinHandle<()>>,
}

impl Notify {
    /// Starts listening on channel `me`; `listener` runs on the listener thread for every message.
    pub fn start(me: Channel, listener: impl Fn(Message) + Send + 'static) -> Option<Notify> {
        Self::start_named(me, Names::default(), listener)
    }

    pub fn start_named(me: Channel, names: Names, listener: impl Fn(Message) + Send + 'static) -> Option<Notify> {
        let (ev, ack, mx) = (names.event(me), names.ack(me), names.mutex());
        let event = unsafe { CreateEventW(None, true, false, PCWSTR(ev.as_ptr())) }.ok();
        let ackh = unsafe { CreateEventW(None, true, false, PCWSTR(ack.as_ptr())) }.ok();
        let stop = unsafe { CreateEventW(None, true, false, PCWSTR::null()) }.ok();
        let mutex = unsafe { CreateMutexW(None, false, PCWSTR(mx.as_ptr())) }.ok();
        let map = open_map(&names);
        let (Some(event), Some(ackh), Some(stop), Some(mutex), Some(map)) = (event, ackh, stop, mutex, map) else {
            log::write(&format!("IPC: failed to create objects ({})", std::io::Error::last_os_error()));
            for h in [event, ackh, stop, mutex, map].into_iter().flatten() {
                close(h);
            }
            return None;
        };
        let running = Arc::new(AtomicBool::new(true));
        let (r, alive) = (running.clone(), names.alive(me));
        let (e, a, s, m, mp) = (H(event), H(ackh), H(stop), H(mutex), H(map));
        let thread = std::thread::Builder::new()
            .name("ipc".into())
            .spawn(move || listen(alive, e, a, s, m, mp, r, listener))
            .ok()?;
        Some(Notify { me, names, running, stop: s, keep: vec![e, a, s, m, mp], thread: Some(thread) })
    }

    pub fn send_int(&self, dest: Channel, code: u32, v: i32, wait: bool) -> bool {
        self.send(dest, code, KIND_INT, &v.to_le_bytes(), wait)
    }

    pub fn send_string(&self, dest: Channel, code: u32, s: &str, wait: bool) -> bool {
        let mut b: Vec<u8> = s.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
        b.truncate((MAP_SIZE - 8) as usize);
        self.send(dest, code, KIND_STRING, &b, wait)
    }

    /// Writes the message and wakes the peer; with `wait`, waits (up to 10 s, and only if its window is not hung)
    /// for the peer to read it. False when the peer program is not running or its listener never started.
    pub fn send(&self, dest: Channel, code: u32, kind: u32, payload: &[u8], wait: bool) -> bool {
        if dest == self.me {
            return false;
        }
        send_named(&self.names, dest, code, kind, payload, wait)
    }
}

/// Sending without a listener of one's own (the Notify above sends through this).
pub fn send_named(names: &Names, dest: Channel, code: u32, kind: u32, payload: &[u8], wait: bool) -> bool {
    if !mutex_exists(&names.program_mutex(dest)) {
        return false; // nobody on the other side
    }
    // waits for the peer's listener thread: up to 75 × 200 ms = 15 s
    let alive = names.alive(dest);
    if !(0..75).any(|i| {
        if i > 0 {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        mutex_exists(&alive)
    }) {
        log::write(&format!("IPC: peer {} did not respond", dest as u32));
        return false;
    }
    let len = payload.len().min((MAP_SIZE - 8) as usize);
    let (ackn, mxn, evn) = (names.ack(dest), names.mutex(), names.event(dest));
    unsafe {
        let ack = CreateEventW(None, true, false, PCWSTR(ackn.as_ptr())).unwrap_or_default();
        let _ = ResetEvent(ack);
        let m = CreateMutexW(None, false, PCWSTR(mxn.as_ptr())).unwrap_or_default();
        WaitForSingleObject(m, 5000);
        let map = open_map(names).unwrap_or_default();
        let view = MapViewOfFile(map, FILE_MAP_WRITE, 0, 0, MAP_SIZE as usize);
        if !view.Value.is_null() {
            let p = view.Value as *mut u8;
            std::ptr::copy_nonoverlapping(code.to_le_bytes().as_ptr(), p, 4);
            std::ptr::copy_nonoverlapping(kind.to_le_bytes().as_ptr(), p.add(4), 4);
            std::ptr::copy_nonoverlapping(payload.as_ptr(), p.add(8), len);
            let _ = FlushViewOfFile(view.Value, 0);
            let _ = UnmapViewOfFile(view);
        }
        let _ = ReleaseMutex(m);
        let ev = CreateEventW(None, true, false, PCWSTR(evn.as_ptr())).unwrap_or_default();
        let _ = SetEvent(ev);
        if wait {
            let class = to_wide(if dest == Channel::Editor { EDITOR_WND_CLASS } else { CAPTURE_WND_CLASS });
            if let Ok(w) = FindWindowW(PCWSTR(class.as_ptr()), PCWSTR::null())
                && !IsHungAppWindow(w).as_bool()
            {
                WaitForSingleObject(ack, 10000); // never blocks on a hung peer
            }
            let _ = ResetEvent(ack);
        }
        for h in [ev, map, m, ack] {
            close(h);
        }
    }
    log::write(&format!("IPC: sent code=0x{code:02X} kind={kind} → {}", dest as u32));
    true
}

#[allow(clippy::too_many_arguments)]
fn listen(alive: Vec<u16>, event: H, ack: H, stop: H, mutex: H, map: H, running: Arc<AtomicBool>,
          listener: impl Fn(Message)) {
    // "receiver thread alive": the sender waits for this named mutex before sending
    let alive = unsafe { CreateMutexW(None, true, PCWSTR(alive.as_ptr())) }.unwrap_or_default();
    while running.load(Ordering::SeqCst) {
        let r = unsafe { WaitForMultipleObjects(&[event.0, stop.0], false, INFINITE) };
        if r != WAIT_OBJECT_0 {
            break;
        }
        let mut m = Message::default();
        unsafe {
            let _ = ResetEvent(event.0);
            WaitForSingleObject(mutex.0, 5000);
            let view = MapViewOfFile(map.0, FILE_MAP_READ, 0, 0, MAP_SIZE as usize);
            if !view.Value.is_null() {
                let b = std::slice::from_raw_parts(view.Value as *const u8, MAP_SIZE as usize);
                let u32_at = |i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
                m.code = u32_at(0);
                m.kind = u32_at(4);
                match m.kind {
                    KIND_INT => m.ivalue = u32_at(8) as i32,
                    KIND_BLOB => m.blob.copy_from_slice(&b[8..24]),
                    KIND_STRING => {
                        let w: Vec<u16> = b[8..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]]))
                                                 .take_while(|&c| c != 0).collect();
                        m.text = String::from_utf16_lossy(&w);
                    }
                    _ => {}
                }
                let _ = UnmapViewOfFile(view);
            }
            let _ = ReleaseMutex(mutex.0);
            let _ = SetEvent(ack.0); // releases the sender
        }
        log::write(&format!("IPC: received code=0x{:02X} kind={}", m.code, m.kind));
        listener(m);
    }
    unsafe {
        let _ = ReleaseMutex(alive);
    }
    close(alive);
}

impl Drop for Notify {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        unsafe {
            let _ = SetEvent(self.stop.0);
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        for h in &self.keep {
            close(h.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn messages_go_both_ways() {
        let names = Names { prefix: format!("snapcore-test-{}-", std::process::id()) };
        // the peer programs "run": their single-instance mutexes exist
        let held: Vec<HANDLE> = [Channel::Capture, Channel::Editor]
            .iter()
            .map(|&c| unsafe { CreateMutexW(None, false, PCWSTR(names.program_mutex(c).as_ptr())) }.unwrap())
            .collect();
        let (tx_c, rx_c) = mpsc::channel();
        let (tx_e, rx_e) = mpsc::channel();
        let cap = Notify::start_named(Channel::Capture, names.clone(), move |m| tx_c.send(m).unwrap()).unwrap();
        let ed = Notify::start_named(Channel::Editor, names.clone(), move |m| tx_e.send(m).unwrap()).unwrap();
        let t = Duration::from_secs(10);

        assert!(cap.send_int(Channel::Editor, code::LIBRARY_CHANGED, -7, true));
        let m = rx_e.recv_timeout(t).unwrap();
        assert_eq!((m.code, m.kind, m.ivalue), (code::LIBRARY_CHANGED, KIND_INT, -7));

        assert!(ed.send_string(Channel::Capture, code::PIN_FILE, "C:\\caps\\ç 中文.snap", false));
        let m = rx_c.recv_timeout(t).unwrap();
        assert_eq!((m.code, m.kind, m.text.as_str()), (code::PIN_FILE, KIND_STRING, "C:\\caps\\ç 中文.snap"));

        let blob: Vec<u8> = (1..=16).collect();
        assert!(ed.send(Channel::Capture, code::LIBRARY_DOC_OPENED, KIND_BLOB, &blob, false));
        assert_eq!(rx_c.recv_timeout(t).unwrap().blob.to_vec(), blob);

        assert!(!cap.send_int(Channel::Capture, code::LIBRARY_CHANGED, 0, false), "never to itself");
        drop(ed);
        for h in held {
            close(h);
        }
        assert!(!cap.send_int(Channel::Editor, code::LIBRARY_CHANGED, 0, false), "the Editor is not running");
    }
}
