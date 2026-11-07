//! The capture app's command line (G3, C44): `Snapnab.exe /region 100,100,800,600 /ocr /output clipboard`. The words
//! are read in order into commands; the app runs them (an area, a tool and an output combine into one capture).

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tool {
    Text,
    Qr,
    Upload,
    Pin,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cmd {
    /// x, y, width, height; None when the value is not four numbers (or the area is too small)
    Region(Option<(i32, i32, i32, i32)>),
    Window(String),
    Monitor(i32),
    FullScreen,
    Delay(i32),
    Tool(Tool),
    PinFile(String),
    PinClipboard,
    ColorPicker,
    Focus,
    Interval { stop: bool },
    Output(String),
    Hide,
    CaptureImage,
    CaptureVideo,
    Preset(String),
    Clipboard,
    Exit,
    Help,
}

/// swscanf(v, "%d,%d,%d,%d"): each number may have spaces and a sign before it; the commas must follow right away.
fn four_ints(v: &str) -> Option<(i32, i32, i32, i32)> {
    let mut s = v;
    let mut out = [0i32; 4];
    for (k, o) in out.iter_mut().enumerate() {
        if k > 0 {
            s = s.strip_prefix(',')?;
        }
        let t = s.trim_start();
        let sign = t.starts_with(['+', '-']) as usize;
        let digits = t[sign..].bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        *o = snapcore::crt::atoi(&t[..sign + digits]);
        s = &t[sign + digits..];
    }
    Some((out[0], out[1], out[2], out[3]))
}

pub fn parse(args: &[String]) -> Vec<Cmd> {
    let mut out = Vec::new();
    let mut i = 0;
    let next = |i: &mut usize| -> String {
        if *i + 1 < args.len() {
            *i += 1;
            args[*i].clone()
        } else {
            String::new()
        }
    };
    while i < args.len() {
        let a = args[i].to_lowercase();
        match a.as_str() {
            "/region" => {
                let v = next(&mut i);
                out.push(Cmd::Region(four_ints(&v).filter(|&(_, _, w, h)| w > 1 && h > 1)));
            }
            "/window" => out.push(Cmd::Window(next(&mut i))),
            "/monitor" => out.push(Cmd::Monitor(snapcore::crt::atoi(&next(&mut i)))),
            "/fullscreen" => out.push(Cmd::FullScreen),
            "/delay" => out.push(Cmd::Delay(snapcore::crt::atoi(&next(&mut i)).clamp(0, 600))),
            "/ocr" => out.push(Cmd::Tool(Tool::Text)),
            "/qr" => out.push(Cmd::Tool(Tool::Qr)),
            "/upload" => out.push(Cmd::Tool(Tool::Upload)),
            "/pin" => {
                if i + 1 < args.len() && !args[i + 1].starts_with('/') {
                    i += 1;
                    out.push(Cmd::PinFile(args[i].clone()));
                } else {
                    out.push(Cmd::Tool(Tool::Pin));
                }
            }
            "/pinclipboard" => out.push(Cmd::PinClipboard),
            "/colorpicker" => out.push(Cmd::ColorPicker),
            "/focus" => out.push(Cmd::Focus),
            "/interval" => out.push(Cmd::Interval { stop: next(&mut i).to_lowercase() == "stop" }),
            "/output" => out.push(Cmd::Output(next(&mut i))),
            "/hide" | "/tray" => out.push(Cmd::Hide),
            "/capture" => out.push(Cmd::CaptureImage),
            "/video" => out.push(Cmd::CaptureVideo),
            "/preset" if i + 1 < args.len() => {
                i += 1;
                out.push(Cmd::Preset(args[i].clone()));
            }
            "/clipboard" => out.push(Cmd::Clipboard),
            "/exit" => out.push(Cmd::Exit),
            "/help" | "/?" => out.push(Cmd::Help),
            _ => {}
        }
        i += 1;
    }
    out
}

pub const HELP: &str = "Snapnab.exe [opções]\n\n\
/hide                 inicia só na bandeja\n\
/capture              captura imagem com o preset de imagem atual\n\
/video                grava vídeo com o preset de vídeo atual\n\
/preset \"nome\"        executa um preset pelo nome\n\
/clipboard            captura a imagem da área de transferência\n\
/region x,y,l,a       captura essa área da tela\n\
/window \"título\"      captura a janela cujo título contém o texto\n\
/monitor n            captura o monitor n (1, 2…, da esquerda para a direita)\n\
/fullscreen           captura a tela inteira\n\
/delay s              espera s segundos antes de capturar\n\
/ocr  /qr  /upload    reconhece o texto, lê o código QR ou envia a captura\n\
/pin [arquivo]        fixa o arquivo na tela (sem arquivo: captura e fixa)\n\
/pinclipboard         fixa o conteúdo da área de transferência\n\
/colorpicker          abre o conta-gotas de tela\n\
/focus                liga ou desliga o foco de tela\n\
/interval [stop]      inicia (ou para) a captura por intervalo\n\
/output clipboard|editor|none|\"arquivo\"   para onde vai a captura\n\
/exit                 encerra o Snapnab em execução";

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &[&str]) -> Vec<Cmd> {
        parse(&s.iter().map(|x| x.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn reads_the_switches() {
        assert_eq!(p(&["/REGION", "100, 200,800,600", "/ocr", "/output", "clipboard"]),
                   [Cmd::Region(Some((100, 200, 800, 600))), Cmd::Tool(Tool::Text), Cmd::Output("clipboard".into())]);
        assert_eq!(p(&["/region", "1,2,3"]), [Cmd::Region(None)]);
        assert_eq!(p(&["/region", "1 ,2,3,4"]), [Cmd::Region(None)], "the comma must follow the number");
        assert_eq!(p(&["/region", "0,0,1,50"]), [Cmd::Region(None)], "too narrow");
        assert_eq!(p(&["/region", "-5,+7,10,10x"]), [Cmd::Region(Some((-5, 7, 10, 10)))]);
        assert_eq!(p(&["/pin", "/hide"]), [Cmd::Tool(Tool::Pin), Cmd::Hide]);
        assert_eq!(p(&["/pin", "C:\\a.png"]), [Cmd::PinFile("C:\\a.png".into())]);
        assert_eq!(p(&["/delay", "9999", "/interval", "STOP"]), [Cmd::Delay(600), Cmd::Interval { stop: true }]);
        assert_eq!(p(&["/interval", "/hide"]), [Cmd::Interval { stop: false }], "the next word is taken");
        assert_eq!(p(&["/preset"]), Vec::<Cmd>::new());
        assert_eq!(p(&["/preset", "Imagem — Região", "/?", "/tray", "/unknown"]),
                   [Cmd::Preset("Imagem — Região".into()), Cmd::Help, Cmd::Hide]);
    }
}
