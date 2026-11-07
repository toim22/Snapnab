//! Upload › Configure (common/Upload.cpp ConfigureDialog): the destination, then its own fields; the secrets go to the
//! Windows Credential Manager, and the OAuth destinations sign in through the browser. Runs on its own thread.

use std::ffi::OsStr;

use snapcore::upload::{Dest, Settings, SignIn};
use snapcore::{net, reg};
use windows::Win32::Foundation::{HWND, LPARAM, S_OK, WPARAM};
use windows::Win32::UI::Controls::{
    TASKDIALOG_NOTIFICATIONS, TASKDIALOGCONFIG, TDCBF_CANCEL_BUTTON, TDF_ALLOW_DIALOG_CANCELLATION, TDF_CALLBACK_TIMER,
    TDF_SHOW_MARQUEE_PROGRESS_BAR, TDM_CLICK_BUTTON, TDM_SET_PROGRESS_BAR_MARQUEE, TDN_CREATED, TDN_TIMER, TaskDialogIndirect,
};
use windows::Win32::UI::WindowsAndMessaging::{IDOK, IDYES, MB_ICONINFORMATION, MB_ICONQUESTION, MB_ICONWARNING, MB_YESNO, SendMessageW};
use windows::core::{HRESULT, PCWSTR};

use crate::form::{self, Field};

fn tr(s: &str) -> String {
    snapcore::i18n::tr_str(s)
}

fn message(owner: isize, text: &str, title: &str, flags: windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE) -> windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_RESULT {
    let (t, c) = (snapcore::wide::to_wide(text), snapcore::wide::to_wide(title));
    unsafe { windows::Win32::UI::WindowsAndMessaging::MessageBoxW((owner != 0).then(|| HWND(owner as _)), PCWSTR(t.as_ptr()), PCWSTR(c.as_ptr()), flags) }
}

const MASK: &str = "********";

fn text(label: &str, t: &str) -> Field {
    Field::Text { label: label.into(), text: t.into() }
}

fn check(label: &str, v: bool) -> Field {
    Field::Check { label: label.into(), value: v }
}

fn masked(secret: &str) -> &'static str {
    if net::load_secret(secret).is_empty() { "" } else { MASK }
}

/// A secret field: unchanged while it shows the mask; empty removes it.
fn secret(name: &str, v: &str) {
    if v != MASK {
        if v.is_empty() {
            net::delete_secret(name);
        } else {
            net::save_secret(name, v);
        }
    }
}

pub fn configure(owner: isize) {
    std::thread::spawn(move || {
        configure_now(owner);
    });
}

/// ConfigureDialog; true when the settings were saved.
pub fn configure_now(owner: isize) -> bool {
    let mut s = Settings::load();
    let names: Vec<String> = (0..Dest::COUNT).map(|d| snapcore::upload::dest_name(Dest::from_i32(d))).collect();
    let strip = reg::get_dword(OsStr::new("StripMetadataOnUpload"), 1, None) != 0;
    let mut f0 = [Field::Label(tr("Desligado por padrão: nada é enviado até você escolher um destino.")),
                  Field::Combo { label: tr("Destino"), items: names, value: s.dest as i32 },
                  check(&tr("Copiar o link para a área de transferência depois do envio"), s.copy_link),
                  check(&tr("Remover metadados das imagens enviadas"), strip)];
    if !form::run(owner, &tr("Upload — destino"), &mut f0) {
        return false;
    }
    s.dest = Dest::from_i32(f0[1].int());
    s.copy_link = f0[2].on();
    reg::set_dword(OsStr::new("StripMetadataOnUpload"), f0[3].int() as u32, None);
    let mut f: Vec<Field> = match s.dest {
        Dest::S3 => vec![text(&tr("Endpoint (host)"), &s.s3_endpoint), text(&tr("Região"), &s.s3_region), text("Bucket", &s.s3_bucket),
                         text(&tr("Prefixo (pasta)"), &s.s3_prefix), text("Access key ID", &s.s3_access_key),
                         text("Secret access key", masked("S3SecretKey")), text(&tr("Endereço público (opcional)"), &s.s3_public_url),
                         check(&tr("Endereço no estilo de caminho (MinIO e alguns serviços)"), s.s3_path_style),
                         check(&tr("Leitura pública (x-amz-acl: public-read)"), s.s3_public_read),
                         Field::Label(tr("A chave secreta fica no Gerenciador de Credenciais do Windows."))],
        Dest::Dropbox => vec![text(&tr("App key do seu app Dropbox"), &s.dropbox_app_key), text(&tr("Pasta"), &s.folder),
                              Field::Label(tr("Crie um app em dropbox.com/developers (acesso \"App folder\" ou \"Full\", permissões files.content.write e sharing.write)."))],
        Dest::GoogleDrive => vec![text(&tr("Client ID (app para computador)"), &s.google_client_id),
                                  text("Client secret", masked("GoogleClientSecret")), text(&tr("Pasta"), &s.folder),
                                  check(&tr("Qualquer pessoa com o link pode ver"), s.public_link),
                                  Field::Label(tr("Crie credenciais OAuth do tipo \"App para computador\" no Google Cloud Console (API do Drive ativada)."))],
        Dest::OneDrive => vec![text(&tr("Client ID (registro de app do Azure)"), &s.ms_client_id), text(&tr("Pasta"), &s.folder),
                               check(&tr("Link anônimo (qualquer pessoa com o link)"), s.public_link),
                               Field::Label(tr("Registre um app (\"Aplicativos móveis e de desktop\", redirecionamento http://localhost) com a permissão Files.ReadWrite."))],
        Dest::Ftp | Dest::Sftp => {
            let ftp = s.dest == Dest::Ftp;
            vec![text(&tr("Servidor"), &s.host), Field::Int { label: tr("Porta (0 = padrão)"), value: s.port, min: 0, max: 65535 },
                 text(&tr("Usuário"), &s.user),
                 if ftp { text(&tr("Senha"), masked("FtpPassword")) } else { text(&tr("Arquivo da chave privada"), &s.key_file) },
                 text(&tr("Pasta remota"), &s.folder), text(&tr("Endereço web da pasta (para o link)"), &s.http_base),
                 Field::Label(if ftp { tr("FTP sem criptografia: prefira SFTP para dados sensíveis.") }
                              else { tr("Usa o cliente OpenSSH do Windows com autenticação por chave.") })]
        }
        Dest::Custom => {
            let filter = tr("Enviador personalizado (*.sxcu;*.json)|*.sxcu;*.json|");
            if let Some(path) = open_dialog(owner, &tr("Importar enviador (.sxcu)"), &filter) {
                let mut d = std::fs::read(&path).unwrap_or_default();
                if d.starts_with(&[0xEF, 0xBB, 0xBF]) {
                    d.drain(..3);
                }
                if !snapcore::upload::custom_valid(&d) {
                    message(owner, &tr("Arquivo .sxcu inválido."), "Snapnab", MB_ICONWARNING);
                    return false;
                }
                s.custom_json = String::from_utf8_lossy(&d).into_owned();
            } else if s.custom_json.is_empty() {
                return false;
            }
            return s.save();
        }
        Dest::None => return s.save(),
    };
    if !form::run(owner, &(tr("Upload — ") + &snapcore::upload::dest_name(s.dest)), &mut f) {
        return false;
    }
    let t = |i: usize| f[i].text().trim().to_owned();
    let mut need_login = false;
    match s.dest {
        Dest::S3 => {
            (s.s3_endpoint, s.s3_region, s.s3_bucket, s.s3_prefix, s.s3_access_key) = (t(0), t(1), t(2), f[3].text().to_owned(), t(4));
            secret("S3SecretKey", &t(5));
            s.s3_public_url = t(6);
            (s.s3_path_style, s.s3_public_read) = (f[7].on(), f[8].on());
        }
        Dest::Dropbox => {
            need_login = t(0) != s.dropbox_app_key || net::load_secret("DropboxRefresh").is_empty();
            (s.dropbox_app_key, s.folder) = (t(0), t(1));
        }
        Dest::GoogleDrive => {
            need_login = t(0) != s.google_client_id || net::load_secret("GoogleRefresh").is_empty();
            s.google_client_id = t(0);
            secret("GoogleClientSecret", &t(1));
            (s.folder, s.public_link) = (t(2), f[3].on());
        }
        Dest::OneDrive => {
            need_login = t(0) != s.ms_client_id || net::load_secret("MicrosoftRefresh").is_empty();
            (s.ms_client_id, s.folder, s.public_link) = (t(0), t(1), f[2].on());
        }
        Dest::Ftp | Dest::Sftp => {
            (s.host, s.port, s.user) = (t(0), f[1].int(), t(2));
            if s.dest == Dest::Ftp {
                secret("FtpPassword", f[3].text());
            } else {
                s.key_file = t(3);
            }
            (s.folder, s.http_base) = (t(4), t(5));
        }
        _ => {}
    }
    s.save();
    let oauth = matches!(s.dest, Dest::Dropbox | Dest::GoogleDrive | Dest::OneDrive);
    if oauth && (need_login || message(owner, &tr("Fazer login de novo agora?"), "Snapnab", MB_YESNO | MB_ICONQUESTION) == IDYES) {
        match sign_in(owner, &s) {
            Ok(()) => {
                message(owner, &tr("Login concluído. As capturas podem ser enviadas."), "Snapnab", MB_ICONINFORMATION);
            }
            Err(e) if !e.is_empty() => {
                message(owner, &e, "Snapnab", MB_ICONWARNING);
            }
            Err(_) => {}
        }
    }
    true
}

/// SignIn: the page opens in the browser; Dropbox shows a code to paste, Google and Microsoft come back to a local
/// address (a task dialog waits for it).
fn sign_in(owner: isize, s: &Settings) -> Result<(), String> {
    let mut si = SignIn::begin(s)?;
    shell_open(&si.url);
    let mut code = String::new();
    if s.dest == Dest::Dropbox {
        let mut f = [Field::Label(tr("Autorize no navegador e cole aqui o código que o Dropbox mostrar.")), text(&tr("Código"), "")];
        if !form::run(owner, &tr("Login no Dropbox"), &mut f) || f[1].text().trim().is_empty() {
            return Err(String::new());
        }
        code = f[1].text().to_owned();
    } else {
        wait_for_browser(owner, &mut si);
    }
    si.finish(&code)
}

/// WaitForBrowser: a task dialog with a marquee that closes itself when the browser comes back.
fn wait_for_browser(owner: isize, si: &mut SignIn) -> bool {
    struct Ctx<'a> {
        si: &'a mut SignIn,
        got: bool,
    }
    unsafe extern "system" fn cb(h: HWND, n: TASKDIALOG_NOTIFICATIONS, _w: WPARAM, _l: LPARAM, data: isize) -> HRESULT {
        let c = unsafe { &mut *(data as *mut Ctx) };
        unsafe {
            if n == TDN_CREATED {
                SendMessageW(h, TDM_SET_PROGRESS_BAR_MARQUEE.0 as u32, Some(WPARAM(1)), Some(LPARAM(0)));
            }
            if n == TDN_TIMER && !c.got && c.si.poll() {
                c.got = true;
                SendMessageW(h, TDM_CLICK_BUTTON.0 as u32, Some(WPARAM(IDOK.0 as usize)), Some(LPARAM(0)));
            }
        }
        S_OK
    }
    let mut ctx = Ctx { si, got: false };
    let w = |s: &str| snapcore::wide::to_wide(s);
    let (title, main, content) = (w("Snapnab"), w(&tr("Conclua o login no navegador")),
                                  w(&tr("Autorize o Snapnab na página que abriu. Esta janela fecha sozinha quando o navegador voltar.")));
    let tc = TASKDIALOGCONFIG {
        cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
        hwndParent: HWND(owner as _),
        dwFlags: TDF_CALLBACK_TIMER | TDF_ALLOW_DIALOG_CANCELLATION | TDF_SHOW_MARQUEE_PROGRESS_BAR,
        pszWindowTitle: PCWSTR(title.as_ptr()),
        pszMainInstruction: PCWSTR(main.as_ptr()),
        pszContent: PCWSTR(content.as_ptr()),
        dwCommonButtons: TDCBF_CANCEL_BUTTON,
        pfCallback: Some(cb),
        lpCallbackData: &mut ctx as *mut Ctx as isize,
        ..Default::default()
    };
    let mut btn = 0;
    let _ = unsafe { TaskDialogIndirect(&tc, Some(&mut btn), None, None) };
    ctx.got
}

fn shell_open(target: &str) {
    let t = snapcore::wide::to_wide(target);
    unsafe {
        windows::Win32::UI::Shell::ShellExecuteW(None, windows::core::w!("open"), PCWSTR(t.as_ptr()), PCWSTR::null(), PCWSTR::null(),
                                                 windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL);
    }
}

fn open_dialog(owner: isize, title: &str, filter: &str) -> Option<String> {
    use windows::Win32::UI::Controls::Dialogs::{GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_PATHMUSTEXIST, OPENFILENAMEW};
    let filter: Vec<u16> = filter.encode_utf16().map(|c| if c == '|' as u16 { 0 } else { c }).chain([0]).collect();
    let t = snapcore::wide::to_wide(title);
    let mut buf = vec![0u16; 4096];
    let mut ofn = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: HWND(owner as _),
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: windows::core::PWSTR(buf.as_mut_ptr()),
        nMaxFile: buf.len() as u32,
        lpstrTitle: if title.is_empty() { PCWSTR::null() } else { PCWSTR(t.as_ptr()) },
        Flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST,
        ..Default::default()
    };
    if !unsafe { GetOpenFileNameW(&mut ofn) }.as_bool() {
        return None;
    }
    let n = buf.iter().position(|&c| c == 0).unwrap_or(0);
    Some(String::from_utf16_lossy(&buf[..n]))
}
