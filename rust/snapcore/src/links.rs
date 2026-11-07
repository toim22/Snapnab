//! Links of the captured area (F32): UI Automation finds the hyperlinks of web pages and documents under the capture
//! (browsers expose the URL as the element's value), so they are stored with the image as clickable `<Link>` areas
//! (and become `/Link` annotations in the PDF).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use windows::Win32::Foundation::{HWND, LPARAM, POINT};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize};
use windows::Win32::System::Variant::{VARIANT, VT_BOOL, VT_BSTR};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, TreeScope_Descendants, UIA_BoundingRectanglePropertyId, UIA_ControlTypePropertyId,
    UIA_HyperlinkControlTypeId, UIA_IsOffscreenPropertyId, UIA_LegacyIAccessibleValuePropertyId, UIA_ValueValuePropertyId,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GA_ROOT, GetAncestor, GetWindowRect, IsIconic, IsWindowVisible, WindowFromPoint,
};
use windows::core::{BOOL, BSTR};

use crate::image::Rect;
use crate::{log, screen};

/// A link of the captured area, in screen coordinates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturedLink {
    pub rect: Rect,
    pub url: String,
}

/// Programs whose windows show web pages and documents with links.
fn is_link_host(h: isize) -> bool {
    const APPS: [&str; 17] = [
        "chrome.exe", "msedge.exe", "firefox.exe", "brave.exe", "opera.exe", "vivaldi.exe", "iexplore.exe", "arc.exe",
        "winword.exe", "powerpnt.exe", "excel.exe", "outlook.exe", "onenote.exe", "acrord32.exe", "acrobat.exe",
        "sumatrapdf.exe", "thunderbird.exe",
    ];
    let exe = screen::window_process_name(h).to_lowercase();
    APPS.contains(&exe.as_str())
}

fn variant_text(v: &VARIANT) -> String {
    if v.vt() == VT_BSTR { BSTR::try_from(v).map(|b| b.to_string()).unwrap_or_default() } else { String::new() }
}

#[derive(Default)]
struct Shared {
    links: Mutex<Vec<CapturedLink>>,
    cancel: AtomicBool,
}

fn search(sh: &Shared, wins: &[isize], area: Rect) {
    let Ok(uia) = (unsafe { CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }) else {
        return;
    };
    let Ok(cond) = (unsafe { uia.CreatePropertyCondition(UIA_ControlTypePropertyId, &VARIANT::from(UIA_HyperlinkControlTypeId.0)) })
    else {
        return;
    };
    let Ok(cache) = (unsafe { uia.CreateCacheRequest() }) else { return };
    unsafe {
        let _ = cache.AddProperty(UIA_BoundingRectanglePropertyId);
        let _ = cache.AddProperty(UIA_ValueValuePropertyId);
        let _ = cache.AddProperty(UIA_LegacyIAccessibleValuePropertyId);
        let _ = cache.AddProperty(UIA_IsOffscreenPropertyId);
    }
    for &w in wins {
        for attempt in 0..2 {
            if sh.cancel.load(Ordering::Relaxed) {
                break;
            }
            let Ok(root) = (unsafe { uia.ElementFromHandle(HWND(w as _)) }) else { break };
            let Ok(arr) = (unsafe { root.FindAllBuildCache(TreeScope_Descendants, &cond, &cache) }) else { break };
            let n = unsafe { arr.Length() }.unwrap_or(0);
            let mut found = 0;
            for i in 0..n {
                if sh.cancel.load(Ordering::Relaxed) {
                    break;
                }
                let Ok(el) = (unsafe { arr.GetElement(i) }) else { continue };
                let Ok(rc) = (unsafe { el.CachedBoundingRectangle() }) else { continue };
                if rc.right <= rc.left || rc.bottom <= rc.top {
                    continue;
                }
                if let Ok(off) = unsafe { el.GetCachedPropertyValue(UIA_IsOffscreenPropertyId) }
                    && off.vt() == VT_BOOL
                    && bool::try_from(&off).unwrap_or(false)
                {
                    continue;
                }
                let mut url = unsafe { el.GetCachedPropertyValue(UIA_ValueValuePropertyId) }.map(|v| variant_text(&v)).unwrap_or_default();
                if url.is_empty() {
                    url = unsafe { el.GetCachedPropertyValue(UIA_LegacyIAccessibleValuePropertyId) }
                        .map(|v| variant_text(&v))
                        .unwrap_or_default();
                }
                if !(url.starts_with("http://") || url.starts_with("https://") || url.starts_with("mailto:")) {
                    continue;
                }
                let vis = Rect::new(rc.left, rc.top, rc.right, rc.bottom).intersect(&area);
                if vis.is_empty() {
                    continue;
                }
                // only where the window is really visible (not under another window)
                let c = POINT { x: (vis.left + vis.right) / 2, y: (vis.top + vis.bottom) / 2 };
                if unsafe { GetAncestor(WindowFromPoint(c), GA_ROOT) }.0 as isize != w {
                    continue;
                }
                found += 1;
                let mut links = sh.links.lock().unwrap();
                if !links.iter().any(|l| l.rect == vis && l.url == url) {
                    links.push(CapturedLink { rect: vis, url });
                }
            }
            if found > 0 || attempt > 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(250)); // browsers build their accessibility tree when first asked
        }
    }
}

/// FindLinks: the links visible in `area` (screen), searched in the windows of browsers and document viewers under
/// it. Gives up after `budget_ms` (the search runs on a worker thread, and what it found until then is used).
pub fn find_links(area: Rect, budget_ms: u64) -> Vec<CapturedLink> {
    struct Ctx {
        area: Rect,
        wins: Vec<isize>,
    }
    unsafe extern "system" fn each(h: HWND, p: LPARAM) -> BOOL {
        let c = unsafe { &mut *(p.0 as *mut Ctx) };
        let hi = h.0 as isize;
        if !unsafe { IsWindowVisible(h) }.as_bool() || unsafe { IsIconic(h) }.as_bool() || screen::is_cloaked(hi) {
            return true.into();
        }
        if screen::window_pid(hi) == std::process::id() {
            return true.into();
        }
        let mut r = Default::default();
        if unsafe { GetWindowRect(h, &mut r) }.is_err() {
            return true.into();
        }
        if Rect::new(r.left, r.top, r.right, r.bottom).intersect(&c.area).is_empty() {
            return true.into();
        }
        if is_link_host(hi) {
            c.wins.push(hi);
        }
        true.into()
    }
    let mut ctx = Ctx { area, wins: Vec::new() };
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut ctx as *mut Ctx as isize));
    }
    if ctx.wins.is_empty() {
        return Vec::new();
    }
    let sh = Arc::new(Shared::default());
    let (tx, rx) = mpsc::channel();
    let (sh2, wins) = (sh.clone(), ctx.wins);
    std::thread::spawn(move || {
        let init = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        search(&sh2, &wins, area);
        if init.is_ok() {
            unsafe { CoUninitialize() };
        }
        let _ = tx.send(());
    });
    if rx.recv_timeout(Duration::from_millis(budget_ms)).is_err() {
        sh.cancel.store(true, Ordering::Relaxed);
        log::write(&format!("Links: search stopped after {budget_ms} ms"));
    }
    let links = sh.links.lock().unwrap().clone();
    log::write(&format!("Links: {} link(s) in the captured area", links.len()));
    links
}
