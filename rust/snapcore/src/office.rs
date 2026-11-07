//! Office.cpp: a capture sent to Word, PowerPoint, Excel or OneNote (C20) through COM automation (late binding,
//! IDispatch). Word: the picture at the cursor of the active document (a new document when none is open). PowerPoint:
//! centered on the current slide or on a new blank slide (OfficeNewSlide). Excel: at the active cell. OneNote: at the
//! end of the page open in OneNote (desktop OneNote 2016/365).

use std::ffi::OsStr;
use std::mem::ManuallyDrop;

use windows::Win32::Foundation::VARIANT_BOOL;
use windows::Win32::System::Com::{
    CLSCTX_LOCAL_SERVER, CLSIDFromProgID, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize, DISPATCH_FLAGS,
    DISPATCH_METHOD, DISPATCH_PROPERTYGET, DISPATCH_PROPERTYPUT, DISPPARAMS, EXCEPINFO, IDispatch,
};
use windows::Win32::System::Ole::{DISPID_PROPERTYPUT, GetActiveObject};
use windows::Win32::System::Variant::{
    VAR_CHANGE_FLAGS, VARENUM, VARIANT, VT_BOOL, VT_BSTR, VT_DISPATCH, VT_I4, VT_R4, VariantChangeType, VariantClear,
};
use windows::core::{BSTR, GUID, HRESULT, Interface, PCWSTR};

use crate::i18n::fill;
use crate::{log, reg, tr};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Word = 0,
    PowerPoint,
    Excel,
    OneNote,
}

impl Target {
    pub fn from_i32(v: i32) -> Option<Target> {
        Some(match v {
            0 => Target::Word,
            1 => Target::PowerPoint,
            2 => Target::Excel,
            3 => Target::OneNote,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Target::Word => "Word",
            Target::PowerPoint => "PowerPoint",
            Target::Excel => "Excel",
            Target::OneNote => "OneNote",
        }
    }

    fn prog_id(self) -> &'static str {
        match self {
            Target::Word => "Word.Application",
            Target::PowerPoint => "PowerPoint.Application",
            Target::Excel => "Excel.Application",
            Target::OneNote => "OneNote.Application",
        }
    }

    fn clsid(self) -> Option<GUID> {
        let p = crate::wide::to_wide(self.prog_id());
        unsafe { CLSIDFromProgID(PCWSTR(p.as_ptr())) }.ok()
    }
}

/// The program is installed (its COM class is registered).
pub fn available(t: Target) -> bool {
    t.clsid().is_some()
}

/// A VARIANT that clears itself.
struct Var(VARIANT);

impl Var {
    fn empty() -> Var {
        Var(VARIANT::default())
    }

    fn make(vt: VARENUM, set: impl FnOnce(&mut windows::Win32::System::Variant::VARIANT_0_0_0)) -> Var {
        let mut v = VARIANT::default();
        unsafe {
            let inner = &mut *v.Anonymous.Anonymous;
            inner.vt = vt;
            set(&mut inner.Anonymous);
        }
        Var(v)
    }

    fn str(s: &str) -> Var {
        Var::make(VT_BSTR, |a| a.bstrVal = ManuallyDrop::new(BSTR::from(s)))
    }

    fn int(i: i32) -> Var {
        Var::make(VT_I4, |a| a.lVal = i)
    }

    fn float(f: f32) -> Var {
        Var::make(VT_R4, |a| a.fltVal = f)
    }

    fn bool(b: bool) -> Var {
        Var::make(VT_BOOL, |a| a.boolVal = VARIANT_BOOL(if b { -1 } else { 0 }))
    }

    fn vt(&self) -> VARENUM {
        unsafe { self.0.Anonymous.Anonymous.vt }
    }

    fn disp(&self) -> Option<IDispatch> {
        if self.vt() == VT_DISPATCH { unsafe { (*self.0.Anonymous.Anonymous.Anonymous.pdispVal).clone() } } else { None }
    }

    fn i4(&self) -> Option<i32> {
        (self.vt() == VT_I4).then(|| unsafe { self.0.Anonymous.Anonymous.Anonymous.lVal })
    }

    fn r4(&self) -> Option<f32> {
        (self.vt() == VT_R4).then(|| unsafe { self.0.Anonymous.Anonymous.Anonymous.fltVal })
    }

    fn text(&self) -> String {
        if self.vt() == VT_BSTR { unsafe { self.0.Anonymous.Anonymous.Anonymous.bstrVal.to_string() } } else { String::new() }
    }

    fn to_r4(&mut self) {
        let p = &mut self.0 as *mut VARIANT;
        unsafe {
            let _ = VariantChangeType(p, p, VAR_CHANGE_FLAGS(0), VT_R4);
        }
    }
}

impl Drop for Var {
    fn drop(&mut self) {
        unsafe {
            let _ = VariantClear(&mut self.0);
        }
    }
}

const LOCALE_USER_DEFAULT: u32 = 0x0400;

/// A late-bound call: a method or a property get/put with the arguments in order (IDispatch wants them reversed).
fn call(d: Option<&IDispatch>, flags: DISPATCH_FLAGS, name: &str, args: &[&Var]) -> Result<Var, HRESULT> {
    let Some(d) = d else { return Err(windows::Win32::Foundation::E_POINTER) };
    let n = crate::wide::to_wide(name);
    let names = [PCWSTR(n.as_ptr())];
    let mut id = 0i32;
    unsafe { d.GetIDsOfNames(&GUID::zeroed(), names.as_ptr(), 1, LOCALE_USER_DEFAULT, &mut id) }.map_err(|e| e.code())?;
    // a bitwise copy of each argument: the callee only reads them and `args` keeps ownership
    let mut a: Vec<VARIANT> = args.iter().rev().map(|v| unsafe { std::ptr::read(&v.0) }).collect();
    let mut put = DISPID_PROPERTYPUT;
    let dp = DISPPARAMS {
        rgvarg: if a.is_empty() { std::ptr::null_mut() } else { a.as_mut_ptr() },
        rgdispidNamedArgs: if flags.contains(DISPATCH_PROPERTYPUT) { &mut put } else { std::ptr::null_mut() },
        cArgs: a.len() as u32,
        cNamedArgs: flags.contains(DISPATCH_PROPERTYPUT) as u32,
    };
    let mut out = Var::empty();
    let mut ei = EXCEPINFO::default();
    let r = unsafe { d.Invoke(id, &GUID::zeroed(), LOCALE_USER_DEFAULT, flags, &dp, Some(&mut out.0), Some(&mut ei), None) };
    std::mem::forget(a); // the copies are not ours to clear
    r.map(|_| out).map_err(|e| e.code())
}

fn get(d: Option<&IDispatch>, name: &str) -> Var {
    call(d, DISPATCH_PROPERTYGET | DISPATCH_METHOD, name, &[]).unwrap_or_else(|_| Var::empty())
}

fn get_args(d: Option<&IDispatch>, name: &str, args: &[&Var]) -> Result<Var, HRESULT> {
    call(d, DISPATCH_PROPERTYGET | DISPATCH_METHOD, name, args)
}

fn method(d: Option<&IDispatch>, name: &str, args: &[&Var]) -> Result<Var, HRESULT> {
    call(d, DISPATCH_METHOD, name, args)
}

/// The running instance of the program, or a new one made visible.
fn connect(t: Target) -> Result<IDispatch, String> {
    let Some(clsid) = t.clsid() else {
        return Err(fill(&tr!("O %s não está instalado neste computador (ou é a versão da Microsoft Store, sem automação)."), &[&t.name()]));
    };
    let mut unk = None;
    if unsafe { GetActiveObject(&clsid, None, &mut unk) }.is_ok()
        && let Some(app) = unk.and_then(|u| u.cast::<IDispatch>().ok())
    {
        return Ok(app);
    }
    let app: IDispatch = unsafe { CoCreateInstance(&clsid, None, CLSCTX_LOCAL_SERVER) }
        .map_err(|_| fill(&tr!("Não foi possível abrir o %s."), &[&t.name()]))?;
    if t != Target::OneNote {
        let _ = call(Some(&app), DISPATCH_PROPERTYPUT, "Visible", &[&Var::bool(true)]);
    }
    Ok(app)
}

/// Sends the PNG at `path` (w × h pixels); the error explains a failure (not installed, no page open…).
pub fn send(t: Target, path: &str, w: i32, h: i32) -> Result<(), String> {
    let ci = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    struct Uninit(bool);
    impl Drop for Uninit {
        fn drop(&mut self) {
            if self.0 {
                unsafe { CoUninitialize() };
            }
        }
    }
    let _un = Uninit(ci.is_ok());
    let r = send_in(t, path, w, h);
    match &r {
        Ok(()) => log::write(&format!("Office: picture sent to {}", t.name())),
        Err(e) => log::write(&format!("Office: {} failed: {e}", t.name())),
    }
    r
}

fn send_in(t: Target, path: &str, w: i32, h: i32) -> Result<(), String> {
    let app = connect(t)?;
    let a = Some(&app);
    let file = Var::str(path);
    let refused = |hr: HRESULT| fill(&tr!("O %s recusou a imagem (0x%08X).").replace("%08X", &format!("{:08X}", hr.0 as u32)), &[&t.name()]);
    let (pw, ph) = (w as f64 * 0.75, h as f64 * 0.75); // pixels at 96 DPI in points
    match t {
        Target::Word => {
            let docs = get(a, "Documents");
            if get(docs.disp().as_ref(), "Count").i4() == Some(0) {
                let _ = method(docs.disp().as_ref(), "Add", &[]);
            }
            let sel = get(a, "Selection");
            let shapes = get(sel.disp().as_ref(), "InlineShapes");
            // FileName, LinkToFile, SaveWithDocument
            method(shapes.disp().as_ref(), "AddPicture", &[&file, &Var::bool(false), &Var::bool(true)]).map_err(refused)?;
        }
        Target::PowerPoint => {
            let pres = get(a, "Presentations");
            let p = if get(pres.disp().as_ref(), "Count").i4() == Some(0) {
                method(pres.disp().as_ref(), "Add", &[]).unwrap_or_else(|_| Var::empty())
            } else {
                get(a, "ActivePresentation")
            };
            let slides = get(p.disp().as_ref(), "Slides");
            let setup = get(p.disp().as_ref(), "PageSetup");
            let sw = get(setup.disp().as_ref(), "SlideWidth").r4().unwrap_or(960.0) as f64;
            let sh = get(setup.disp().as_ref(), "SlideHeight").r4().unwrap_or(540.0) as f64;
            let new_slide = reg::get_dword(OsStr::new("OfficeNewSlide"), 1, None) != 0;
            let n = get(slides.disp().as_ref(), "Count").i4().unwrap_or(0);
            // the slide shown in the window (0 when there is no window or slide yet)
            let view = || {
                let win = get(a, "ActiveWindow");
                get(win.disp().as_ref(), "View")
            };
            let current = {
                let v = view();
                let cur = get(v.disp().as_ref(), "Slide");
                get(cur.disp().as_ref(), "SlideIndex").i4().unwrap_or(0)
            };
            let slide = if new_slide || n == 0 || current == 0 {
                let at = if current > 0 { current + 1 } else { n + 1 };
                let s = method(slides.disp().as_ref(), "Add", &[&Var::int(at), &Var::int(12 /* ppLayoutBlank */)]).map_err(refused)?;
                let v = view();
                let _ = method(v.disp().as_ref(), "GotoSlide", &[&Var::int(at)]);
                s
            } else {
                get_args(slides.disp().as_ref(), "Item", &[&Var::int(current)]).map_err(refused)?
            };
            // the picture fitted inside the slide (with a margin), centered
            let k = 1f64.min(sw * 0.9 / pw).min(sh * 0.9 / ph);
            let (pw, ph) = (pw * k, ph * k);
            let shapes = get(slide.disp().as_ref(), "Shapes");
            let (f, tr_) = (Var::int(0), Var::int(-1));
            let args = [&file, &f, &tr_, &Var::float(((sw - pw) / 2.0) as f32), &Var::float(((sh - ph) / 2.0) as f32),
                        &Var::float(pw as f32), &Var::float(ph as f32)];
            method(shapes.disp().as_ref(), "AddPicture", &args).map_err(refused)?;
        }
        Target::Excel => {
            let books = get(a, "Workbooks");
            if get(books.disp().as_ref(), "Count").i4() == Some(0) {
                let _ = method(books.disp().as_ref(), "Add", &[]);
            }
            let sheet = get(a, "ActiveSheet");
            let cell = get(a, "ActiveCell");
            let mut l = get(cell.disp().as_ref(), "Left");
            let mut tp = get(cell.disp().as_ref(), "Top");
            l.to_r4();
            tp.to_r4();
            let shapes = get(sheet.disp().as_ref(), "Shapes");
            let (f, tr_) = (Var::int(0), Var::int(-1));
            let args = [&file, &f, &tr_, &l, &tp, &Var::float(pw as f32), &Var::float(ph as f32)];
            method(shapes.disp().as_ref(), "AddPicture", &args).map_err(refused)?;
        }
        Target::OneNote => {
            let windows = get(a, "Windows");
            let cur = get(windows.disp().as_ref(), "CurrentWindow");
            let id = get(cur.disp().as_ref(), "CurrentPageId").text();
            if id.is_empty() {
                return Err(tr!("Abra uma página no OneNote antes de enviar a captura."));
            }
            let data = std::fs::read(path).unwrap_or_default();
            let xml = format!(
                "<?xml version=\"1.0\"?><one:Page xmlns:one=\"http://schemas.microsoft.com/office/onenote/2013/onenote\" ID=\"{id}\">\
                 <one:Outline><one:OEChildren><one:OE><one:Image><one:Size width=\"{pw:.6}\" height=\"{ph:.6}\" isSetByUser=\"true\"/>\
                 <one:Data>{}</one:Data></one:Image></one:OE></one:OEChildren></one:Outline></one:Page>",
                crate::net::base64(&data)
            );
            method(a, "UpdatePageContent", &[&Var::str(&xml)]).map_err(refused)?;
        }
    }
    Ok(())
}
