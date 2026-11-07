//! The QR code and barcode results (barcode::ShowResults, C04): a task dialog that copies each content, opens each
//! link or copies them all; a message when nothing was found. It runs on its own thread, the codes read there too.

use snapcore::image::Image;
use snapcore::wide::to_wide;
use snapcore::{barcode, clip};
use windows::Win32::UI::Controls::{
    TASKDIALOG_BUTTON, TASKDIALOG_FLAGS, TASKDIALOGCONFIG, TDCBF_CLOSE_BUTTON, TDF_ALLOW_DIALOG_CANCELLATION,
    TDF_USE_COMMAND_LINKS, TaskDialogIndirect,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{MB_ICONINFORMATION, MessageBoxW, SW_SHOWNORMAL};
use windows::core::{PCWSTR, w};

/// Reads the codes of `img` and shows what was found.
pub fn show(img: Image) {
    std::thread::spawn(move || {
        let codes = barcode::read(&img);
        snapcore::log::write(&format!("QR: {} code(s) found", codes.len()));
        results(&codes);
    });
}

fn results(codes: &[barcode::Code]) {
    let caption = to_wide(tr("Ler código QR"));
    if codes.is_empty() {
        let t = to_wide(tr("Nenhum código QR ou de barras encontrado na imagem."));
        unsafe {
            MessageBoxW(None, PCWSTR(t.as_ptr()), PCWSTR(caption.as_ptr()), MB_ICONINFORMATION);
        }
        return;
    }
    // (0 copy, 1 open, 2 copy all) and the index of each button
    let mut labels: Vec<Vec<u16>> = Vec::new();
    let mut action: Vec<(i32, usize)> = Vec::new();
    let mut content = String::new();
    for (i, c) in codes.iter().take(8).enumerate() {
        let shown: String = if c.text.chars().count() > 120 { c.text.chars().take(117).collect::<String>() + "…" } else { c.text.clone() };
        content += &format!("{}. [{}] {}\n", i + 1, c.format, shown);
        labels.push(to_wide(tr("Copiar %zu\n%s").replace("%zu", &(i + 1).to_string()).replace("%s", &shown)));
        action.push((0, i));
        if c.url {
            labels.push(to_wide(tr("Abrir o link %zu\n%s").replace("%zu", &(i + 1).to_string()).replace("%s", &shown)));
            action.push((1, i));
        }
    }
    if codes.len() > 1 {
        labels.push(to_wide(tr("Copiar tudo")));
        action.push((2, 0));
    }
    let btns: Vec<TASKDIALOG_BUTTON> =
        labels.iter().enumerate().map(|(i, l)| TASKDIALOG_BUTTON { nButtonID: 100 + i as i32, pszButtonText: PCWSTR(l.as_ptr()) }).collect();
    let title = to_wide(if codes.len() == 1 {
        tr("1 código encontrado")
    } else {
        tr("%zu códigos encontrados").replace("%zu", &codes.len().to_string())
    });
    let content = to_wide(content);
    let tc = TASKDIALOGCONFIG {
        cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
        dwFlags: TASKDIALOG_FLAGS(TDF_USE_COMMAND_LINKS.0 | TDF_ALLOW_DIALOG_CANCELLATION.0),
        pszWindowTitle: PCWSTR(caption.as_ptr()),
        pszMainInstruction: PCWSTR(title.as_ptr()),
        pszContent: PCWSTR(content.as_ptr()),
        cButtons: btns.len() as u32,
        pButtons: btns.as_ptr(),
        dwCommonButtons: TDCBF_CLOSE_BUTTON,
        ..Default::default()
    };
    let mut btn = 0i32;
    if unsafe { TaskDialogIndirect(&tc, Some(&mut btn), None, None) }.is_err() || btn < 100 || btn >= 100 + action.len() as i32 {
        return;
    }
    let (kind, idx) = action[(btn - 100) as usize];
    match kind {
        0 => {
            clip::copy_text(0, &codes[idx].text);
        }
        1 => {
            let u = to_wide(&codes[idx].text);
            unsafe {
                ShellExecuteW(None, w!("open"), PCWSTR(u.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
            }
        }
        _ => {
            let all: Vec<&str> = codes.iter().map(|c| c.text.as_str()).collect();
            clip::copy_text(0, &all.join("\r\n\r\n"));
        }
    }
}

fn tr(s: &str) -> String {
    snapcore::i18n::tr_str(s)
}
