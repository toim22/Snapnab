//! HDR screenshots with the right colors (C02, Hdr.cpp). On HDR (Advanced Color) monitors a GDI copy of the screen
//! comes out washed out or too bright; there the screen is read with DXGI Output Duplication in FP16 (scRGB) and
//! tone-mapped to SDR with the monitor's "SDR content brightness", so the result looks like what the user sees.
//! Setting HdrCorrection (on by default); monitors in SDR are untouched.

use std::ffi::OsString;

use windows::Win32::Devices::Display::*;
use windows::Win32::Foundation::{HMODULE, RECT};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::core::Interface;

use crate::image::Image;
use crate::{crt, log, reg};

pub fn half_to_float(h: u16) -> f32 {
    let (sign, exp, man) = ((h >> 15) & 1, ((h >> 10) & 31) as i32, (h & 1023) as u32);
    let v = if exp == 0 {
        man as f32 * 2f32.powi(-24) // subnormal (ldexp: exact)
    } else if exp == 31 {
        if man != 0 { 0.0 } else { 65504.0 } // NaN → 0, infinity → max
    } else {
        (man | 1024) as f32 * 2f32.powi(exp - 25)
    };
    if sign != 0 { -v } else { v }
}

/// One scRGB value (1.0 = 80 nits) to an 8-bit sRGB channel, for a given SDR white level in nits.
pub fn tone_map(v: f32, sdr_white_nits: f32) -> u8 {
    if !(v > 0.0) {
        return 0;
    }
    let mut x = v * 80.0 / sdr_white_nits.max(1.0); // 1.0 = SDR white
    const KNEE: f32 = 0.85; // highlights above the knee are compressed
    if x > KNEE {
        x = KNEE + (1.0 - KNEE) * (1.0 - (-(x - KNEE) / (1.0 - KNEE)).exp());
    }
    let s = if x <= 0.0031308 { 12.92 * x } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 };
    ((s * 255.0).round() as i64).clamp(0, 255) as u8
}

pub fn enabled() -> bool {
    reg::get_dword(&OsString::from("HdrCorrection"), 1, None) != 0
}

struct Output {
    adapter: IDXGIAdapter1,
    out: IDXGIOutput6,
    desc: DXGI_OUTPUT_DESC1,
}

impl Output {
    fn hdr(&self) -> bool {
        self.desc.ColorSpace == DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020
    }
    fn name(&self) -> String {
        let n = self.desc.DeviceName;
        let len = n.iter().position(|&c| c == 0).unwrap_or(n.len());
        String::from_utf16_lossy(&n[..len])
    }
}

fn outputs() -> Vec<Output> {
    let mut v = Vec::new();
    let Ok(f) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else { return v };
    let mut i = 0;
    while let Ok(a) = unsafe { f.EnumAdapters1(i) } {
        let mut j = 0;
        while let Ok(o) = unsafe { a.EnumOutputs(j) } {
            if let Ok(out) = o.cast::<IDXGIOutput6>() {
                if let Ok(desc) = unsafe { out.GetDesc1() } {
                    if desc.AttachedToDesktop.as_bool() {
                        v.push(Output { adapter: a.clone(), out, desc });
                    }
                }
            }
            j += 1;
        }
        i += 1;
    }
    v
}

fn intersect(a: &RECT, b: &RECT) -> Option<RECT> {
    let r = RECT { left: a.left.max(b.left), top: a.top.max(b.top), right: a.right.min(b.right), bottom: a.bottom.min(b.bottom) };
    (r.left < r.right && r.top < r.bottom).then_some(r)
}

/// "SDR content brightness" of the monitor (Settings › Display › HDR), in nits; 80 if unknown.
fn sdr_white_nits(gdi_name: &[u16; 32]) -> f32 {
    let (mut np, mut nm) = (0u32, 0u32);
    if unsafe { GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut np, &mut nm) }.is_err() {
        return 80.0;
    }
    let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); np as usize];
    let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); nm as usize];
    if unsafe { QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, &mut np, paths.as_mut_ptr(), &mut nm, modes.as_mut_ptr(), None) }.is_err() {
        return 80.0;
    }
    let want = &gdi_name[..gdi_name.iter().position(|&c| c == 0).unwrap_or(32)];
    for p in &paths[..np as usize] {
        let mut src = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
        src.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
        src.header.size = size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
        src.header.adapterId = p.sourceInfo.adapterId;
        src.header.id = p.sourceInfo.id;
        if unsafe { DisplayConfigGetDeviceInfo(&mut src.header) } != 0 {
            continue;
        }
        let n = &src.viewGdiDeviceName;
        if n[..n.iter().position(|&c| c == 0).unwrap_or(32)] != *want {
            continue;
        }
        let mut wl = DISPLAYCONFIG_SDR_WHITE_LEVEL::default();
        wl.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL;
        wl.header.size = size_of::<DISPLAYCONFIG_SDR_WHITE_LEVEL>() as u32;
        wl.header.adapterId = p.targetInfo.adapterId;
        wl.header.id = p.targetInfo.id;
        if unsafe { DisplayConfigGetDeviceInfo(&mut wl.header) } == 0 && wl.SDRWhiteLevel != 0 {
            return wl.SDRWhiteLevel as f32 / 1000.0 * 80.0;
        }
    }
    80.0
}

/// 65 536 entries: half-float bit pattern → 8-bit sRGB.
fn make_lut(white: f32) -> Vec<u8> {
    (0..65536u32).map(|h| tone_map(half_to_float(h as u16), white)).collect()
}

/// One duplicated output: the device, the duplication and a staging texture for reading.
struct Dup {
    ctx: ID3D11DeviceContext,
    dev: ID3D11Device,
    dup: IDXGIOutputDuplication,
    staging: Option<ID3D11Texture2D>,
    bounds: RECT,
    lut: Vec<u8>,
}

impl Dup {
    fn open(o: &Output) -> Option<Dup> {
        if o.desc.Rotation != DXGI_MODE_ROTATION_IDENTITY && o.desc.Rotation != DXGI_MODE_ROTATION_UNSPECIFIED {
            return None;
        }
        let (mut dev, mut ctx) = (None, None);
        unsafe {
            D3D11CreateDevice(&o.adapter, D3D_DRIVER_TYPE_UNKNOWN, HMODULE::default(), D3D11_CREATE_DEVICE_BGRA_SUPPORT, None,
                              D3D11_SDK_VERSION, Some(&mut dev), None, Some(&mut ctx))
        }
        .ok()?;
        let (dev, ctx) = (dev?, ctx?);
        let o5 = o.out.cast::<IDXGIOutput5>().ok()?;
        let dup = unsafe { o5.DuplicateOutput1(&dev, 0, &[DXGI_FORMAT_R16G16B16A16_FLOAT]) }.ok()?;
        Some(Dup { ctx, dev, dup, staging: None, bounds: o.desc.DesktopCoordinates, lut: make_lut(sdr_white_nits(&o.desc.DeviceName)) })
    }

    /// Copies the current desktop image to the staging texture; true when there is an image to read.
    fn acquire(&mut self, wait_ms: u32) -> bool {
        let mut fi = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut res: Option<IDXGIResource> = None;
        match unsafe { self.dup.AcquireNextFrame(wait_ms, &mut fi, &mut res) } {
            Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => return self.staging.is_some(), // the previous copy is still valid
            Err(_) => return false,
            Ok(()) => {}
        }
        let tex = res.and_then(|r| r.cast::<ID3D11Texture2D>().ok());
        let mut ok = tex.is_some();
        if let (Some(t), None) = (&tex, &self.staging) {
            let mut td = D3D11_TEXTURE2D_DESC::default();
            unsafe { t.GetDesc(&mut td) };
            td.Usage = D3D11_USAGE_STAGING;
            td.BindFlags = 0;
            td.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
            td.MiscFlags = 0;
            td.MipLevels = 1;
            td.ArraySize = 1;
            let mut s = None;
            ok = unsafe { self.dev.CreateTexture2D(&td, None, Some(&mut s)) }.is_ok() && s.is_some();
            self.staging = s;
        }
        if ok {
            if let (Some(s), Some(t)) = (&self.staging, &tex) {
                unsafe { self.ctx.CopyResource(s, t) };
            }
        }
        let _ = unsafe { self.dup.ReleaseFrame() };
        ok
    }

    /// The mapped staging texture: rows of 4 half floats (R, G, B, A) per pixel.
    fn read<T>(&self, f: impl FnOnce(*const u8, usize) -> T) -> Option<T> {
        let s = self.staging.as_ref()?;
        let mut m = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe { self.ctx.Map(s, 0, D3D11_MAP_READ, 0, Some(&mut m)) }.ok()?;
        let r = f(m.pData as *const u8, m.RowPitch as usize);
        unsafe { self.ctx.Unmap(s, 0) };
        Some(r)
    }

    fn pixel(&self, row: *const u8, x: usize) -> u32 {
        let p = unsafe { std::slice::from_raw_parts((row as *const u16).add(x * 4), 3) };
        0xFF000000 | (self.lut[p[0] as usize] as u32) << 16 | (self.lut[p[1] as usize] as u32) << 8 | self.lut[p[2] as usize] as u32
    }
}

/// Some monitor is in HDR mode now.
pub fn any_hdr_monitor() -> bool {
    outputs().iter().any(|o| o.hdr())
}

/// The area touches an HDR monitor.
pub fn is_hdr_area(screen: RECT) -> bool {
    outputs().iter().any(|o| o.hdr() && intersect(&o.desc.DesktopCoordinates, &screen).is_some())
}

/// Replaces the parts of `img` (a GDI copy of `screen`) that lie on HDR monitors with tone-mapped pixels; false when
/// nothing was replaced (no HDR monitor, duplication unavailable).
pub fn fix_capture(screen: RECT, img: &mut Image) -> bool {
    if img.is_empty() {
        return false;
    }
    let mut any = false;
    for o in outputs() {
        if !o.hdr() {
            continue;
        }
        let Some(part) = intersect(&o.desc.DesktopCoordinates, &screen) else { continue };
        let Some(mut d) = Dup::open(&o) else {
            log::write(&format!("HDR: duplication unavailable on {}", o.name()));
            continue;
        };
        let mut got = false;
        for _ in 0..6 {
            got = d.acquire(150) && d.staging.is_some(); // the first frame can take a moment
            if got {
                break;
            }
        }
        if !got {
            log::write(&format!("HDR: no frame from {}", o.name()));
            continue;
        }
        let w = img.w as usize;
        let done = d.read(|data, pitch| {
            for y in part.top..part.bottom {
                let src = unsafe { data.add((y - d.bounds.top) as usize * pitch) };
                let row = (y - screen.top) as usize * w;
                for x in part.left..part.right {
                    img.px[row + (x - screen.left) as usize] = d.pixel(src, (x - d.bounds.left) as usize);
                }
            }
        });
        if done.is_none() {
            continue;
        }
        any = true;
        let nits = crt::sprintf("%.0f", &[crt::Arg::F(sdr_white_nits(&o.desc.DeviceName) as f64)]);
        log::write(&format!("HDR: {} tone-mapped (SDR white {nits} nits)", o.name()));
    }
    any
}

/// Continuous capture for recordings: one HDR monitor area, frames scaled to the output size (bottom-up BGRA rows).
pub struct Session {
    d: Dup,
    out_w: usize,
    out_h: usize,
    xs: Vec<usize>, // source column / row of each output pixel
    ys: Vec<usize>,
}

impl Session {
    /// None: the area is not on (only) an HDR monitor, or there is no duplication.
    pub fn begin(screen: RECT, out_w: i32, out_h: i32) -> Option<Session> {
        for o in outputs() {
            if !o.hdr() || intersect(&o.desc.DesktopCoordinates, &screen) != Some(screen) {
                continue;
            }
            let d = Dup::open(&o)?;
            let (sw, sh) = (screen.right - screen.left, screen.bottom - screen.top);
            let xs = (0..out_w)
                .map(|i| (screen.left - d.bounds.left + (sw - 1).min(((i as f64 + 0.5) * sw as f64 / out_w as f64) as i32)) as usize)
                .collect();
            let ys = (0..out_h)
                .map(|i| (screen.top - d.bounds.top + (sh - 1).min(((i as f64 + 0.5) * sh as f64 / out_h as f64) as i32)) as usize)
                .collect();
            log::write(&format!("HDR: recording from {} with tone mapping", o.name()));
            return Some(Session { d, out_w: out_w as usize, out_h: out_h as usize, xs, ys });
        }
        None
    }

    /// The latest desktop frame into `bottom_up` (out_w × out_h); false on a lost duplication.
    pub fn grab(&mut self, bottom_up: &mut [u32]) -> bool {
        if !self.d.acquire(0) || self.d.staging.is_none() || bottom_up.len() < self.out_w * self.out_h {
            return false;
        }
        let s = &*self;
        s.d.read(|data, pitch| {
            for y in 0..s.out_h {
                let src = unsafe { data.add(s.ys[y] * pitch) };
                let dst = &mut bottom_up[(s.out_h - 1 - y) * s.out_w..][..s.out_w];
                for (x, d) in dst.iter_mut().enumerate() {
                    *d = s.d.pixel(src, s.xs[x]);
                }
            }
        })
        .is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_and_tone() {
        assert_eq!(half_to_float(0x3C00), 1.0);
        assert_eq!(half_to_float(0xC000), -2.0);
        assert_eq!(half_to_float(0x7C00), 65504.0);
        assert_eq!(half_to_float(0x7C01), 0.0);
        assert_eq!(half_to_float(0x0001), 2f32.powi(-24));
        assert_eq!((tone_map(-1.0, 80.0), tone_map(1.0, 80.0), tone_map(1e9, 80.0)), (0, 249, 255));
    }
}
