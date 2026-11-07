//! The capture sounds (Sounds.cpp, C31): generated in memory as 16-bit mono WAV (a camera shutter, a click, a chime)
//! or a WAV file the user picked, played without waiting.

use std::ffi::OsStr;
use std::sync::OnceLock;

use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME, SND_MEMORY, SND_NODEFAULT};
use windows::core::PCWSTR;

use crate::reg;
use crate::wide::to_wide;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Off = 0,
    Shutter,
    Click,
    Chime,
    Custom,
}

impl Kind {
    pub const COUNT: u32 = 5;

    pub fn from_index(i: u32) -> Kind {
        [Kind::Off, Kind::Shutter, Kind::Click, Kind::Chime, Kind::Custom][i.min(Self::COUNT - 1) as usize]
    }

    pub fn name(self) -> String {
        crate::i18n::tr_str(match self {
            Kind::Off => "Desligado",
            Kind::Shutter => "Obturador de câmera",
            Kind::Click => "Clique",
            Kind::Chime => "Toque suave",
            Kind::Custom => "Arquivo WAV…",
        })
    }
}

const RATE: usize = 44100;

/// MakeWav: `gen(t)` in seconds → sample in -1..1.
fn make_wav(seconds: f64, mut gen_: impl FnMut(f64) -> f64) -> Vec<u8> {
    let n = (seconds * RATE as f64) as usize;
    let mut w = Vec::with_capacity(44 + n * 2);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&((44 + n * 2 - 8) as u32).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&(RATE as u32).to_le_bytes());
    w.extend_from_slice(&(RATE as u32 * 2).to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&((n * 2) as u32).to_le_bytes());
    for i in 0..n {
        let v = gen_(i as f64 / RATE as f64).clamp(-1.0, 1.0);
        w.extend_from_slice(&((v * 30000.0).round() as i16).to_le_bytes());
    }
    w
}

struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        ((self.0 >> 8) & 0xFFFF) as f64 / 32768.0 - 1.0
    }
}

fn sample(k: Kind) -> &'static [u8] {
    static CACHE: [OnceLock<Vec<u8>>; 4] = [OnceLock::new(), OnceLock::new(), OnceLock::new(), OnceLock::new()];
    let pi = std::f64::consts::PI;
    CACHE[k as usize].get_or_init(|| match k {
        // the curtain opening and closing: two crisp clicks 55 ms apart; each a metallic ring (4.2 and 2.6 kHz), a
        // burst of noise band-passed around 3.5 kHz and a short body at 190 Hz
        Kind::Shutter => {
            let w0 = 2.0 * pi * 3500.0 / RATE as f64;
            let alpha = w0.sin() / (2.0 * 0.8);
            let a0 = 1.0 + alpha;
            let (b0, b2, a1, a2) = (alpha / a0, -alpha / a0, -2.0 * w0.cos() / a0, (1.0 - alpha) / a0);
            let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
            let mut noise = Noise(12345);
            make_wav(0.17, |t| {
                let x = noise.next();
                let bp = b0 * x + b2 * x2 - a1 * y1 - a2 * y2; // band-pass biquad (RBJ)
                x2 = x1;
                x1 = x;
                y2 = y1;
                y1 = bp;
                let mut v = 0.0;
                for (at, amp) in [(0.0, 1.0), (0.055, 0.72)] {
                    let d: f64 = t - at;
                    if d < 0.0 {
                        continue;
                    }
                    let ring = (2.0 * pi * 4200.0 * d).sin() * (-d * 900.0).exp() * 0.55
                        + (2.0 * pi * 2600.0 * d).sin() * (-d * 600.0).exp() * 0.35;
                    let body = (2.0 * pi * 190.0 * d).sin() * (-d * 140.0).exp() * 0.22;
                    v += amp * (ring + bp * (-d * 150.0).exp() * 1.1 + body);
                }
                v * 0.42
            })
        }
        Kind::Click => {
            let mut noise = Noise(12345);
            make_wav(0.05, |t| {
                (2.0 * pi * 2200.0 * t).sin() * (-t * 160.0).exp() * 0.8 + noise.next() * (-t * 300.0).exp() * 0.3
            })
        }
        Kind::Chime => make_wav(0.45, |t| {
            let a = (2.0 * pi * 880.0 * t).sin() * (-t * 9.0).exp();
            let b = if t > 0.09 { (2.0 * pi * 1318.5 * (t - 0.09)).sin() * (-(t - 0.09) * 8.0).exp() } else { 0.0 };
            (a + b) * 0.35
        }),
        _ => Vec::new(),
    })
}

pub fn play(k: Kind, custom_file: &str) {
    match k {
        Kind::Off => {}
        Kind::Custom => {
            if !custom_file.is_empty() && std::path::Path::new(custom_file).is_file() {
                let f = to_wide(custom_file);
                unsafe {
                    let _ = PlaySoundW(PCWSTR(f.as_ptr()), None, SND_FILENAME | SND_ASYNC | SND_NODEFAULT);
                }
            }
        }
        _ => {
            let w = sample(k); // static: stays valid while it plays
            if !w.is_empty() {
                unsafe {
                    let _ = PlaySoundW(PCWSTR(w.as_ptr() as *const u16), None, SND_MEMORY | SND_ASYNC | SND_NODEFAULT);
                }
            }
        }
    }
}

/// PlayCaptureSound: the sound chosen in Options (off by default).
pub fn play_capture_sound() {
    let k = Kind::from_index(reg::get_dword(OsStr::new("CaptureSound"), 0, None));
    if k != Kind::Off {
        let f = reg::get_string(OsStr::new("CaptureSoundFile"), None).map(|s| s.to_string_lossy().into_owned());
        play(k, &f.unwrap_or_default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sounds_are_wav_files() {
        for (k, secs) in [(Kind::Shutter, 0.17), (Kind::Click, 0.05), (Kind::Chime, 0.45)] {
            let w = sample(k);
            let n = (secs * RATE as f64) as usize;
            assert_eq!(&w[..4], b"RIFF");
            assert_eq!(w.len(), 44 + n * 2);
            assert!(w[44..].chunks(2).any(|c| c != [0, 0]), "{k:?} is silent");
        }
        assert!(sample(Kind::Off).is_empty());
    }
}
