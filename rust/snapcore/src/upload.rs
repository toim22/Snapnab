//! Optional upload and share link (C21; ShareX dossier 01 · C12 uploader framework, C13 destinations, C14 OAuth, C21
//! custom uploader .sxcu), as Upload.cpp had it. Off by default: nothing is sent until the user picks a destination.
//! Destinations: S3-compatible storage (AWS Signature V4), Dropbox, Google Drive and OneDrive (OAuth 2.0 with PKCE, with
//! the user's own app key / client id), FTP (WinINet), SFTP (the OpenSSH client of Windows, key authentication) and a
//! custom HTTP uploader (request template compatible with ShareX .sxcu). Settings in Uploaders.xml; passwords, secrets
//! and tokens in the Windows Credential Manager ("Snapnab/…"), never in files. The dialogs stay with the interface.

use std::collections::BTreeMap;
use std::ffi::{OsStr, c_void};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::Path;

use windows::Win32::Foundation::{GetLastError, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Networking::WinInet::{
    FTP_TRANSFER_TYPE_BINARY, FtpCreateDirectoryW, FtpPutFileW, INTERNET_FLAG_PASSIVE, INTERNET_OPEN_TYPE_PRECONFIG,
    INTERNET_SERVICE_FTP, InternetCloseHandle, InternetConnectW, InternetOpenW,
};
use windows::Win32::System::SystemInformation::{GetSystemDirectoryW, GetSystemTime};
use windows::Win32::System::Threading::{CREATE_NO_WINDOW, WaitForSingleObject};
use windows::core::PCWSTR;

use crate::i18n::fill;
use crate::library::{self, W};
use crate::net::{self, Json, Multipart, Response, json_field, url_encode};
use crate::tr;
use crate::wide::{to_os, to_wide};
use crate::xml::{self, Node};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Dest {
    #[default]
    None = 0,
    S3,
    Dropbox,
    GoogleDrive,
    OneDrive,
    Ftp,
    Sftp,
    Custom,
}

impl Dest {
    pub const COUNT: i32 = 8;

    pub fn from_i32(v: i32) -> Dest {
        match v.clamp(0, Self::COUNT - 1) {
            1 => Dest::S3,
            2 => Dest::Dropbox,
            3 => Dest::GoogleDrive,
            4 => Dest::OneDrive,
            5 => Dest::Ftp,
            6 => Dest::Sftp,
            7 => Dest::Custom,
            _ => Dest::None,
        }
    }
}

pub fn dest_name(d: Dest) -> String {
    match d {
        Dest::S3 => tr!("Armazenamento S3 (AWS e compatíveis)"),
        Dest::Dropbox => "Dropbox".into(),
        Dest::GoogleDrive => "Google Drive".into(),
        Dest::OneDrive => "OneDrive".into(),
        Dest::Ftp => "FTP".into(),
        Dest::Sftp => "SFTP".into(),
        Dest::Custom => tr!("Enviador HTTP personalizado (.sxcu)"),
        Dest::None => tr!("Nenhum (desligado)"),
    }
}

// ---------------------------------------------------------------- settings (Uploaders.xml)

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub dest: Dest,
    // S3 and compatible (Wasabi, Backblaze B2 S3, DigitalOcean Spaces, MinIO, Cloudflare R2…)
    pub s3_endpoint: String,
    pub s3_region: String,
    pub s3_bucket: String,
    pub s3_prefix: String,
    pub s3_access_key: String,
    pub s3_public_url: String,
    pub s3_path_style: bool,
    pub s3_public_read: bool,
    // OAuth apps of the user
    pub dropbox_app_key: String,
    pub google_client_id: String,
    pub ms_client_id: String,
    /// remote folder (Dropbox, Drive, OneDrive, FTP, SFTP)
    pub folder: String,
    /// the file readable by anyone with the link
    pub public_link: bool,
    // FTP / SFTP; http_base: the public web address of the folder (for the link)
    pub host: String,
    pub user: String,
    pub key_file: String,
    pub http_base: String,
    /// 0 = 21 (FTP) / 22 (SFTP)
    pub port: i32,
    /// custom uploader (.sxcu JSON)
    pub custom_json: String,
    /// the link replaces the image on the clipboard
    pub copy_link: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            dest: Dest::None,
            s3_endpoint: "s3.amazonaws.com".into(),
            s3_region: "us-east-1".into(),
            s3_bucket: String::new(),
            s3_prefix: "snapnab/".into(),
            s3_access_key: String::new(),
            s3_public_url: String::new(),
            s3_path_style: false,
            s3_public_read: true,
            dropbox_app_key: String::new(),
            google_client_id: String::new(),
            ms_client_id: String::new(),
            folder: "Snapnab".into(),
            public_link: true,
            host: String::new(),
            user: String::new(),
            key_file: String::new(),
            http_base: String::new(),
            port: 0,
            custom_json: String::new(),
            copy_link: true,
        }
    }
}

/// %LOCALAPPDATA%\Snapnab\Uploaders.xml; SNAPNAB_UPLOADERS_FILE points the end-to-end tests at their own copy, so a
/// test never reads or writes the user's destinations.
fn settings_file() -> std::path::PathBuf {
    match std::env::var_os("SNAPNAB_UPLOADERS_FILE") {
        Some(f) if !f.is_empty() => std::path::PathBuf::from(f),
        _ => crate::paths::app_data_dir().join("Uploaders.xml"),
    }
}

impl Settings {
    pub fn load() -> Settings {
        Self::load_from(&settings_file())
    }

    pub fn load_from(path: &Path) -> Settings {
        let mut s = Settings::default();
        let Some(n) = xml::load_file(path) else { return s };
        let geti = |k: &str, def: i32| n.attr(k).map_or(def, crate::snap::atoi);
        let getb = |k: &str, def: bool| geti(k, def as i32) != 0;
        let get = |k: &str, def: &str| n.attr(k).unwrap_or(def).to_owned();
        s.dest = Dest::from_i32(geti("Destination", 0));
        s.s3_endpoint = get("S3Endpoint", &s.s3_endpoint);
        s.s3_region = get("S3Region", &s.s3_region);
        s.s3_bucket = get("S3Bucket", "");
        s.s3_prefix = get("S3Prefix", &s.s3_prefix);
        s.s3_access_key = get("S3AccessKey", "");
        s.s3_public_url = get("S3PublicUrl", "");
        s.s3_path_style = getb("S3PathStyle", false);
        s.s3_public_read = getb("S3PublicRead", true);
        s.dropbox_app_key = get("DropboxAppKey", "");
        s.google_client_id = get("GoogleClientId", "");
        s.ms_client_id = get("MicrosoftClientId", "");
        s.folder = get("Folder", &s.folder);
        s.public_link = getb("PublicLink", true);
        s.host = get("Host", "");
        s.user = get("User", "");
        s.key_file = get("KeyFile", "");
        s.http_base = get("HttpBase", "");
        s.port = geti("Port", 0);
        if let Some(c) = n.child("Custom") {
            s.custom_json = c.text.clone();
        }
        s.copy_link = getb("CopyLink", true);
        s
    }

    pub fn save(&self) -> bool {
        self.save_to(&settings_file())
    }

    pub fn save_to(&self, path: &Path) -> bool {
        let mut n = Node::new("Uploaders");
        let b = |v: bool| if v { "1" } else { "0" };
        n.set("Destination", (self.dest as i32).to_string());
        n.set("S3Endpoint", &*self.s3_endpoint).set("S3Region", &*self.s3_region).set("S3Bucket", &*self.s3_bucket)
            .set("S3Prefix", &*self.s3_prefix).set("S3AccessKey", &*self.s3_access_key).set("S3PublicUrl", &*self.s3_public_url)
            .set("S3PathStyle", b(self.s3_path_style)).set("S3PublicRead", b(self.s3_public_read));
        n.set("DropboxAppKey", &*self.dropbox_app_key).set("GoogleClientId", &*self.google_client_id)
            .set("MicrosoftClientId", &*self.ms_client_id);
        n.set("Folder", &*self.folder).set("PublicLink", b(self.public_link));
        n.set("Host", &*self.host).set("User", &*self.user).set("KeyFile", &*self.key_file).set("HttpBase", &*self.http_base)
            .set("Port", self.port.to_string());
        n.set("CopyLink", b(self.copy_link));
        if !self.custom_json.is_empty() {
            n.add("Custom").text = self.custom_json.clone();
        }
        xml::save_file(path, &n)
    }

    /// The destination has what it needs to send (its fields, and the secret or the sign-in).
    pub fn configured(&self) -> bool {
        match self.dest {
            Dest::S3 => !self.s3_bucket.is_empty() && !self.s3_access_key.is_empty() && !net::load_secret("S3SecretKey").is_empty(),
            Dest::Dropbox => !self.dropbox_app_key.is_empty() && !net::load_secret("DropboxRefresh").is_empty(),
            Dest::GoogleDrive => !self.google_client_id.is_empty() && !net::load_secret("GoogleRefresh").is_empty(),
            Dest::OneDrive => !self.ms_client_id.is_empty() && !net::load_secret("MicrosoftRefresh").is_empty(),
            Dest::Ftp | Dest::Sftp => !self.host.is_empty() && !self.user.is_empty(),
            Dest::Custom => !self.custom_json.is_empty(),
            Dest::None => false,
        }
    }
}

/// What an upload gave: the link (and the custom uploader's deletion and thumbnail links) or the error.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outcome {
    pub ok: bool,
    pub url: String,
    pub delete_url: String,
    pub thumbnail_url: String,
    pub error: String,
}

impl Outcome {
    fn fail(error: String) -> Outcome {
        Outcome { error, ..Default::default() }
    }

    fn link(url: String) -> Outcome {
        Outcome { ok: true, url, ..Default::default() }
    }
}

// ---------------------------------------------------------------- helpers

fn wide(s: &str) -> W {
    s.encode_utf16().collect()
}

fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

pub fn mime_of(name: &str) -> &'static str {
    let e = String::from_utf16_lossy(&library::file_ext(&wide(name)));
    match e.as_str() {
        ".png" => "image/png",
        ".jpg" | ".jpeg" => "image/jpeg",
        ".gif" => "image/gif",
        ".webp" => "image/webp",
        ".mp4" => "video/mp4",
        ".pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

/// "<what>: <network error>" or "<what>: HTTP <status> <the start of the answer>".
pub fn err(r: &Response, what: &str) -> String {
    if r.status == 0 {
        return format!("{what}: {}", r.error);
    }
    let b = if r.body.len() > 300 { lossy(&r.body[..300]) + "…" } else { lossy(&r.body) };
    format!("{what}: HTTP {} {b}", r.status)
}

/// JSON with only ASCII (HTTP headers such as Dropbox-API-Arg cannot carry UTF-8).
pub fn ascii_json(s: &str) -> String {
    let mut o = String::new();
    for c in s.encode_utf16() {
        match c {
            0x22 => o.push_str("\\\""),
            0x5C => o.push_str("\\\\"),
            c if !(0x20..=0x7E).contains(&c) => o.push_str(&format!("\\u{c:04x}")),
            c => o.push(c as u8 as char),
        }
    }
    o
}

/// application/x-www-form-urlencoded
pub fn form(kv: &[(&str, &str)]) -> String {
    kv.iter()
        .map(|(k, v)| format!("{}={}", url_encode(k.as_bytes(), false), url_encode(v.as_bytes(), false)))
        .collect::<Vec<_>>()
        .join("&")
}

fn post_form(url: &str, body: &str) -> Response {
    net::request("POST", url, &["Content-Type: application/x-www-form-urlencoded".into()], body.as_bytes(), 60000)
}

/// PKCE (RFC 7636, S256)
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    pub fn new() -> Pkce {
        let verifier = net::base64_url(&net::random_bytes(32));
        let challenge = net::base64_url(&net::sha256(verifier.as_bytes()));
        Pkce { verifier, challenge }
    }
}

impl Default for Pkce {
    fn default() -> Self {
        Self::new()
    }
}

fn find(h: &[u8], n: &[u8], from: usize) -> Option<usize> {
    if from > h.len() {
        return None;
    }
    h[from..].windows(n.len().max(1)).position(|w| w == n).map(|p| p + from)
}

/// A parameter of the query string in the first line of an HTTP request, %-decoded ("" if missing).
fn query_param(req: &[u8], k: &str) -> String {
    let key = format!("{k}=");
    let Some(q) = find(req, b"?", 0) else { return String::new() };
    let Some(e) = find(req, b" ", q) else { return String::new() };
    let qs = [b"&", &req[q + 1..e]].concat();
    let Some(mut p) = find(&qs, format!("&{key}").as_bytes(), 0) else { return String::new() };
    p += key.len() + 1;
    let v = &qs[p..find(&qs, b"&", p).unwrap_or(qs.len())];
    let mut o = Vec::new();
    let mut i = 0;
    while i < v.len() {
        if v[i] == b'%' && i + 2 < v.len() {
            let mut n = 0u32;
            for &c in &v[i + 1..i + 3] {
                match (c as char).to_digit(16) {
                    Some(d) => n = n * 16 + d,
                    None => break,
                }
            }
            o.push(n as u8);
            i += 3;
        } else {
            o.push(if v[i] == b'+' { b' ' } else { v[i] });
            i += 1;
        }
    }
    lossy(&o)
}

/// The loopback redirect (RFC 8252): 127.0.0.1 on a free port, one request, its "code" parameter.
pub struct Loopback {
    listener: TcpListener,
    pub port: u16,
    pub code: String,
    pub error: String,
    pub state: String,
}

impl Loopback {
    pub fn open() -> Option<Loopback> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).ok()?;
        let port = listener.local_addr().ok()?.port();
        listener.set_nonblocking(true).ok()?;
        Some(Loopback { listener, port, code: String::new(), error: String::new(), state: String::new() })
    }

    /// True when the browser came back (with the code or an error).
    pub fn poll(&mut self) -> bool {
        let Ok((mut c, _)) = self.listener.accept() else { return false };
        let _ = c.set_nonblocking(false);
        let mut buf = [0u8; 8191];
        let n = c.read(&mut buf).unwrap_or(0);
        let req = &buf[..n];
        self.code = query_param(req, "code");
        self.error = query_param(req, "error");
        self.state = query_param(req, "state");
        let page = tr!("Snapnab: login concluído. Você já pode fechar esta janela.");
        let html = format!("<html><head><meta charset=\"utf-8\"></head><body style=\"font-family:Segoe UI;margin:40px\"><h2>{page}</h2></body></html>");
        let resp = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{html}",
                           html.len());
        let _ = c.write_all(resp.as_bytes());
        !self.code.is_empty() || !self.error.is_empty()
    }
}

/// A new access token from the stored refresh token (Google keeps the old refresh token: dossier C14; Microsoft and
/// Dropbox may rotate it, and the new one is stored).
fn access_token(token_url: &str, body: &str, refresh_secret: &str) -> Result<String, String> {
    let r = post_form(token_url, body);
    let at = json_field(&r.body, "access_token");
    if !r.ok() || at.is_empty() {
        return Err(err(&r, &tr!("Falha ao renovar o acesso (faça login de novo nas opções de upload)")));
    }
    let rt = json_field(&r.body, "refresh_token");
    if !rt.is_empty() {
        net::save_secret(refresh_secret, &rt);
    }
    Ok(at)
}

fn refresh(s: &Settings, d: Dest) -> Result<String, String> {
    match d {
        Dest::Dropbox => access_token(
            "https://api.dropboxapi.com/oauth2/token",
            &form(&[("grant_type", "refresh_token"), ("refresh_token", &net::load_secret("DropboxRefresh")),
                    ("client_id", &s.dropbox_app_key)]),
            "DropboxRefresh",
        ),
        Dest::GoogleDrive => access_token(
            "https://oauth2.googleapis.com/token",
            &form(&[("grant_type", "refresh_token"), ("refresh_token", &net::load_secret("GoogleRefresh")),
                    ("client_id", &s.google_client_id), ("client_secret", &net::load_secret("GoogleClientSecret"))]),
            "GoogleRefresh",
        ),
        Dest::OneDrive => access_token(
            "https://login.microsoftonline.com/common/oauth2/v2.0/token",
            &form(&[("grant_type", "refresh_token"), ("refresh_token", &net::load_secret("MicrosoftRefresh")),
                    ("client_id", &s.ms_client_id), ("scope", "offline_access Files.ReadWrite")]),
            "MicrosoftRefresh",
        ),
        _ => Err(String::new()),
    }
}

fn trim_slashes(s: &str) -> &str {
    s.trim_end_matches('/')
}

fn reg_string(name: &str) -> String {
    crate::reg::get_string(OsStr::new(name), None).map(|v| v.to_string_lossy().into_owned()).unwrap_or_default()
}

fn set_reg_string(name: &str, v: &str) {
    crate::reg::set_string(OsStr::new(name), OsStr::new(v), None)
}

// ---------------------------------------------------------------- destinations

fn upload_s3(s: &Settings, data: &[u8], name: &str) -> Outcome {
    let secret = net::load_secret("S3SecretKey");
    if s.s3_bucket.is_empty() || s.s3_access_key.is_empty() || secret.is_empty() {
        return Outcome::fail(tr!("Configure o bucket e as chaves do S3."));
    }
    let bucket = &s.s3_bucket;
    let region = if s.s3_region.is_empty() { "us-east-1" } else { &s.s3_region };
    let endpoint = trim_slashes(s.s3_endpoint.strip_prefix("https://").unwrap_or(&s.s3_endpoint));
    let key = format!("{}{name}", s.s3_prefix);
    let host = if s.s3_path_style { endpoint.to_owned() } else { format!("{bucket}.{endpoint}") };
    let uri = format!("/{}{}", if s.s3_path_style { format!("{bucket}/") } else { String::new() }, url_encode(key.as_bytes(), true));
    let st = unsafe { GetSystemTime() };
    let amz_date = format!("{:04}{:02}{:02}T{:02}{:02}{:02}Z", st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond);
    let hash = net::hex(&net::sha256(data));
    let mut h = BTreeMap::from([
        ("host".to_owned(), host.clone()),
        ("x-amz-content-sha256".to_owned(), hash.clone()),
        ("x-amz-date".to_owned(), amz_date.clone()),
        ("content-type".to_owned(), mime_of(name).to_owned()),
    ]);
    if s.s3_public_read {
        h.insert("x-amz-acl".into(), "public-read".into());
    }
    let auth = sigv4_authorization("PUT", &uri, "", &h, &hash, &amz_date, region, "s3", &s.s3_access_key, &secret);
    let mut hdr = vec![format!("Authorization: {auth}")];
    hdr.extend(h.iter().filter(|(k, _)| *k != "host").map(|(k, v)| format!("{k}: {v}")));
    let resp = net::request("PUT", &format!("https://{host}{uri}"), &hdr, data, 300000);
    if !resp.ok() {
        return Outcome::fail(err(&resp, &tr!("Falha no upload para o S3")));
    }
    let base = if s.s3_public_url.is_empty() {
        format!("https://{host}{}", if s.s3_path_style { format!("/{bucket}") } else { String::new() })
    } else {
        s.s3_public_url.clone()
    };
    Outcome::link(format!("{}/{}", trim_slashes(&base), url_encode(key.as_bytes(), true)))
}

fn upload_dropbox(s: &Settings, data: &[u8], name: &str) -> Outcome {
    if data.len() >= 150_000_000 {
        return Outcome::fail(tr!("O Dropbox aceita até 150 MB por envio pela API."));
    }
    let at = match refresh(s, Dest::Dropbox) {
        Ok(at) => at,
        Err(e) => return Outcome::fail(e),
    };
    let path = format!("/{}{name}", if s.folder.is_empty() { String::new() } else { format!("{}/", s.folder) });
    let bearer = format!("Authorization: Bearer {at}");
    let up = net::request(
        "POST",
        "https://content.dropboxapi.com/2/files/upload",
        &[bearer.clone(), "Content-Type: application/octet-stream".into(),
          format!("Dropbox-API-Arg: {{\"path\":\"{}\",\"mode\":\"add\",\"autorename\":true}}", ascii_json(&path))],
        data,
        300000,
    );
    if !up.ok() {
        return Outcome::fail(err(&up, &tr!("Falha no upload para o Dropbox")));
    }
    let mut saved = json_field(&up.body, "path_display");
    if saved.is_empty() {
        saved = path;
    }
    let json_hdr = [bearer.clone(), "Content-Type: application/json".into()];
    let ln = net::request("POST", "https://api.dropboxapi.com/2/sharing/create_shared_link_with_settings", &json_hdr,
                          format!("{{\"path\":\"{}\"}}", net::json_escape(&saved)).as_bytes(), 60000);
    let mut url = json_field(&ln.body, "url");
    if url.is_empty() && find(&ln.body, b"shared_link_already_exists", 0).is_some() {
        let ls = net::request("POST", "https://api.dropboxapi.com/2/sharing/list_shared_links", &json_hdr,
                              format!("{{\"path\":\"{}\",\"direct_only\":true}}", net::json_escape(&saved)).as_bytes(), 60000);
        url = json_field(&ls.body, "links[0].url");
    }
    if url.is_empty() {
        return Outcome::fail(err(&ln, &tr!("O arquivo foi enviado, mas o link não pôde ser criado")));
    }
    Outcome::link(url)
}

fn upload_drive(s: &Settings, data: &[u8], name: &str) -> Outcome {
    let at = match refresh(s, Dest::GoogleDrive) {
        Ok(at) => at,
        Err(e) => return Outcome::fail(e),
    };
    let bearer = format!("Authorization: Bearer {at}");
    // the folder (created once, then remembered by its id)
    let mut folder_id = reg_string("GoogleDriveFolderId");
    if !s.folder.is_empty() && folder_id.is_empty() {
        let meta = format!("{{\"name\":\"{}\",\"mimeType\":\"application/vnd.google-apps.folder\"}}", net::json_escape(&s.folder));
        let f = net::request("POST", "https://www.googleapis.com/drive/v3/files?fields=id",
                             &[bearer.clone(), "Content-Type: application/json".into()], meta.as_bytes(), 60000);
        folder_id = json_field(&f.body, "id");
        if !folder_id.is_empty() {
            set_reg_string("GoogleDriveFolderId", &folder_id);
        }
    }
    let meta = format!("{{\"name\":\"{}\"{}}}", net::json_escape(name),
                       if folder_id.is_empty() { String::new() } else { format!(",\"parents\":[\"{folder_id}\"]") });
    let boundary = format!("snapnab{}", net::hex(&net::random_bytes(8)));
    let mut body = format!("--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{meta}\r\n--{boundary}\r\nContent-Type: {}\r\n\r\n",
                           mime_of(name)).into_bytes();
    body.extend_from_slice(data);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let up = net::request("POST", "https://www.googleapis.com/upload/drive/v3/files?uploadType=multipart&fields=id,webViewLink",
                          &[bearer.clone(), format!("Content-Type: multipart/related; boundary={boundary}")], &body, 300000);
    let id = json_field(&up.body, "id");
    let link = json_field(&up.body, "webViewLink");
    if !up.ok() || id.is_empty() {
        if up.status == 404 && !folder_id.is_empty() {
            set_reg_string("GoogleDriveFolderId", ""); // the folder was deleted
        }
        return Outcome::fail(err(&up, &tr!("Falha no upload para o Google Drive")));
    }
    if s.public_link {
        net::request("POST", &format!("https://www.googleapis.com/drive/v3/files/{id}/permissions"),
                     &[bearer, "Content-Type: application/json".into()], br#"{"role":"reader","type":"anyone"}"#, 60000);
    }
    Outcome::link(if link.is_empty() { format!("https://drive.google.com/file/d/{id}/view") } else { link })
}

fn upload_onedrive(s: &Settings, data: &[u8], name: &str) -> Outcome {
    let at = match refresh(s, Dest::OneDrive) {
        Ok(at) => at,
        Err(e) => return Outcome::fail(e),
    };
    let bearer = format!("Authorization: Bearer {at}");
    let full = format!("{}{name}", if s.folder.is_empty() { String::new() } else { format!("{}/", s.folder) });
    let path = url_encode(full.as_bytes(), true);
    let up = net::request(
        "PUT",
        &format!("https://graph.microsoft.com/v1.0/me/drive/root:/{path}:/content?@microsoft.graph.conflictBehavior=rename"),
        &[bearer.clone(), format!("Content-Type: {}", mime_of(name))],
        data,
        300000,
    );
    let id = json_field(&up.body, "id");
    if !up.ok() || id.is_empty() {
        return Outcome::fail(err(&up, &tr!("Falha no upload para o OneDrive")));
    }
    let kind: &[u8] =
        if s.public_link { br#"{"type":"view","scope":"anonymous"}"# } else { br#"{"type":"view","scope":"organization"}"# };
    let ln = net::request("POST", &format!("https://graph.microsoft.com/v1.0/me/drive/items/{id}/createLink"),
                          &[bearer, "Content-Type: application/json".into()], kind, 60000);
    let mut url = json_field(&ln.body, "link.webUrl");
    if url.is_empty() {
        url = json_field(&up.body, "webUrl");
    }
    if url.is_empty() {
        return Outcome::fail(err(&ln, &tr!("O arquivo foi enviado, mas o link não pôde ser criado")));
    }
    Outcome::link(url)
}

fn public_url(s: &Settings, name: &str, scheme: &str) -> String {
    let base = if s.http_base.is_empty() {
        format!("{scheme}{}{}", s.host, if s.folder.is_empty() { String::new() } else { format!("/{}", s.folder) })
    } else {
        s.http_base.clone()
    };
    format!("{}/{}", trim_slashes(&base), url_encode(name.as_bytes(), false))
}

fn upload_ftp(s: &Settings, local: &[u16], name: &str) -> Outcome {
    let pass = to_wide(net::load_secret("FtpPassword"));
    let (agent, host, user) = (to_wide("Snapnab"), to_wide(&s.host), to_wide(&s.user));
    unsafe {
        let inet = InternetOpenW(PCWSTR(agent.as_ptr()), INTERNET_OPEN_TYPE_PRECONFIG.0, PCWSTR::null(), PCWSTR::null(), 0);
        let port = if s.port != 0 { s.port as u16 } else { 21 };
        let c = if inet.is_null() {
            std::ptr::null_mut()
        } else {
            InternetConnectW(inet, PCWSTR(host.as_ptr()), port, PCWSTR(user.as_ptr()), PCWSTR(pass.as_ptr()), INTERNET_SERVICE_FTP,
                             INTERNET_FLAG_PASSIVE, None)
        };
        if c.is_null() {
            let e = GetLastError().0;
            if !inet.is_null() {
                let _ = InternetCloseHandle(inet);
            }
            return Outcome::fail(fill(&tr!("Não foi possível conectar ao FTP (%u)."), &[&e]));
        }
        let folder = wide(&s.folder);
        if !folder.is_empty() {
            // creates each level of the folder (existing ones fail quietly)
            for i in 0..=folder.len() {
                if i == folder.len() || folder[i] == b'/' as u16 {
                    if i > 0 {
                        let dir: W = folder[..i].iter().copied().chain([0]).collect();
                        let _ = FtpCreateDirectoryW(c as *const c_void, PCWSTR(dir.as_ptr()));
                    }
                }
            }
        }
        let remote = to_wide(format!("{}{name}", if s.folder.is_empty() { String::new() } else { format!("{}/", s.folder) }));
        let local_z: W = local.iter().copied().chain([0]).collect();
        let r = if FtpPutFileW(c as *const c_void, PCWSTR(local_z.as_ptr()), PCWSTR(remote.as_ptr()), FTP_TRANSFER_TYPE_BINARY, None).is_ok() {
            Outcome::link(public_url(s, name, "ftp://"))
        } else {
            Outcome::fail(fill(&tr!("O servidor FTP recusou o arquivo (%u)."), &[&GetLastError().0]))
        };
        let _ = InternetCloseHandle(c);
        let _ = InternetCloseHandle(inet);
        r
    }
}

fn upload_sftp(s: &Settings, local: &[u16], name: &str) -> Outcome {
    let mut sys = [0u16; 260];
    let n = unsafe { GetSystemDirectoryW(Some(&mut sys)) } as usize;
    let exe = format!("{}\\OpenSSH\\sftp.exe", String::from_utf16_lossy(&sys[..n.min(sys.len())]));
    if !Path::new(&exe).is_file() {
        return Outcome::fail(tr!("O cliente OpenSSH do Windows não está instalado (Configurações › Sistema › Recursos opcionais)."));
    }
    let batch = std::env::temp_dir().join(format!("snapnab_sftp_{}.txt", String::from_utf16_lossy(&library::time_stamp(false))));
    let remote = format!("{}{name}", if s.folder.is_empty() { String::new() } else { format!("{}/", s.folder) });
    let local_s = String::from_utf16_lossy(local);
    let cmds = format!("{}put \"{local_s}\" \"{remote}\"\n",
                       if s.folder.is_empty() { String::new() } else { format!("-mkdir \"{}\"\n", s.folder) });
    if std::fs::write(&batch, cmds).is_err() {
        return Outcome::fail("Temp file".into());
    }
    let args = format!("-b \"{}\" -o BatchMode=yes -o StrictHostKeyChecking=accept-new -P {}{} {}@{}", batch.display(),
                       if s.port != 0 { s.port } else { 22 },
                       if s.key_file.is_empty() { String::new() } else { format!(" -i \"{}\"", s.key_file) }, s.user, s.host);
    let child = std::process::Command::new(&exe)
        .raw_arg(args)
        .creation_flags(CREATE_NO_WINDOW.0)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    let Ok(mut child) = child else {
        let _ = std::fs::remove_file(&batch);
        return Outcome::fail(tr!("Não foi possível iniciar o sftp."));
    };
    let mut code = 1u32;
    if unsafe { WaitForSingleObject(HANDLE(child.as_raw_handle()), 300000) } == WAIT_OBJECT_0 {
        if let Ok(Some(st)) = child.try_wait() {
            code = st.code().unwrap_or(1) as u32;
        }
    } else {
        let _ = child.kill();
        let _ = child.wait();
    }
    let _ = std::fs::remove_file(&batch);
    if code != 0 {
        return Outcome::fail(fill(&tr!("O SFTP falhou (código %u). Use autenticação por chave (sem senha) e confira host, usuário e pasta."),
                                  &[&code]));
    }
    Outcome::link(public_url(s, name, "sftp://"))
}

fn upload_custom(s: &Settings, data: &[u8], name: &str) -> Outcome {
    let Some(u) = Json::parse(s.custom_json.as_bytes()) else {
        return Outcome::fail(tr!("O enviador personalizado não é um JSON válido."));
    };
    let str = |k: &str| u.get(k).map(Json::text).unwrap_or_default();
    let fname = name.as_bytes();
    let none = BTreeMap::new();
    let expand = |t: &str| expand_syntax(t.as_bytes(), fname, b"", &none);
    let mut method = str("RequestMethod");
    if method.is_empty() {
        method = "POST".into();
    }
    let mut url = expand(&str("RequestURL"));
    if url.is_empty() {
        return Outcome::fail(tr!("O enviador personalizado não tem RequestURL."));
    }
    if find(&url, b"://", 0).is_none() {
        url = [b"https://".as_slice(), &url].concat();
    }
    if let Some(Json::Object(p)) = u.get("Parameters") {
        for (k, v) in p {
            url.extend_from_slice(if url.contains(&b'?') { b"&" } else { b"?" });
            url.extend_from_slice(format!("{}={}", url_encode(k.as_bytes(), false), url_encode(&expand(&v.text()), false)).as_bytes());
        }
    }
    let mut hdr = Vec::new();
    if let Some(Json::Object(h)) = u.get("Headers") {
        for (k, v) in h {
            hdr.push(format!("{k}: {}", lossy(&expand(&v.text()))));
        }
    }
    let body_kind = str("Body");
    let mut body = Vec::new();
    if body_kind.is_empty() || body_kind == "MultipartFormData" {
        let mut mp = Multipart::new();
        if let Some(Json::Object(a)) = u.get("Arguments") {
            for (k, v) in a {
                mp.field(k.as_bytes(), &expand(&v.text()));
            }
        }
        let mut field = str("FileFormName");
        if field.is_empty() {
            field = "file".into();
        }
        mp.file(field.as_bytes(), fname, mime_of(name), data);
        body = mp.finish();
        hdr.push(mp.content_type());
    } else if body_kind == "Binary" {
        body = data.to_vec();
        hdr.push(format!("Content-Type: {}", mime_of(name)));
    } else if body_kind == "FormURLEncoded" {
        if let Some(Json::Object(a)) = u.get("Arguments") {
            for (k, v) in a {
                if !body.is_empty() {
                    body.push(b'&');
                }
                body.extend_from_slice(format!("{}={}", url_encode(k.as_bytes(), false), url_encode(&expand(&v.text()), false)).as_bytes());
            }
        }
        hdr.push("Content-Type: application/x-www-form-urlencoded".into());
    } else {
        // JSON / XML: the Data template
        body = expand(&str("Data"));
        hdr.push(if body_kind == "XML" { "Content-Type: application/xml" } else { "Content-Type: application/json" }.into());
    }
    let resp = net::request(&method, &lossy(&url), &hdr, &body, 300000);
    let after = |t: &str| lossy(&expand_syntax(t.as_bytes(), fname, &resp.body, &resp.headers));
    if !resp.ok() {
        let em = str("ErrorMessage");
        return Outcome::fail(if em.is_empty() { err(&resp, &tr!("Falha no envio")) } else { after(&em) });
    }
    let link = str("URL");
    let url = if link.is_empty() { resp.text() } else { after(&link) };
    let url = String::from_utf16_lossy(library::trim(&wide(&url)));
    let mut r = Outcome { ok: !url.is_empty(), url, thumbnail_url: after(&str("ThumbnailURL")), delete_url: after(&str("DeletionURL")),
                          error: String::new() };
    if !r.ok {
        r.error = tr!("O servidor respondeu, mas o link não foi encontrado na resposta.");
    }
    r
}

// ---------------------------------------------------------------- AWS Signature V4

/// The Authorization header value for a request whose headers (lowercase name → value) are already final.
#[allow(clippy::too_many_arguments)]
pub fn sigv4_authorization(method: &str, uri: &str, query: &str, headers: &BTreeMap<String, String>, payload_hash: &str,
                           amz_date: &str, region: &str, service: &str, access_key: &str, secret_key: &str) -> String {
    let mut canon_headers = String::new();
    let mut signed = String::new();
    for (k, v) in headers {
        canon_headers += &format!("{k}:{v}\n");
        if !signed.is_empty() {
            signed.push(';');
        }
        signed += k;
    }
    let canonical = format!("{method}\n{uri}\n{query}\n{canon_headers}\n{signed}\n{payload_hash}");
    let date = &amz_date[..amz_date.len().min(8)];
    let scope = format!("{date}/{region}/{service}/aws4_request");
    let to_sign = format!("AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}", net::hex(&net::sha256(canonical.as_bytes())));
    let mut k = net::hmac_sha256(format!("AWS4{secret_key}").as_bytes(), date.as_bytes());
    for part in [region, service, "aws4_request"] {
        k = net::hmac_sha256(&k, part.as_bytes());
    }
    let sig = net::hex(&net::hmac_sha256(&k, to_sign.as_bytes()));
    format!("AWS4-HMAC-SHA256 Credential={access_key}/{scope},SignedHeaders={signed},Signature={sig}")
}

// ---------------------------------------------------------------- the custom uploader's syntax (dossier C21)

/// The {…} syntax of a custom uploader: {response}, {json:path}, {json:text|path}, {regex:pattern|group},
/// {xml:path}, {header:name}, {filename}, {input}, {base64:text}, {random:a|b}; `\` escapes the next character.
/// Bytes in, bytes out, as the C++ did (the parts between the ASCII delimiters are copied as they are).
pub fn expand_syntax(text: &[u8], file_name: &[u8], response: &[u8], headers: &BTreeMap<String, String>) -> Vec<u8> {
    fn parse(t: &[u8], i: &mut usize, depth: i32, f: &dyn Fn(&str, &[Vec<u8>]) -> Option<Vec<u8>>) -> Vec<u8> {
        let mut out = Vec::new();
        while *i < t.len() {
            let c = t[*i];
            if c == b'\\' && *i + 1 < t.len() {
                out.push(t[*i + 1]);
                *i += 2;
                continue;
            }
            if (c == b'}' || c == b'|') && depth > 0 {
                return out;
            }
            if c != b'{' {
                out.push(c);
                *i += 1;
                continue;
            }
            *i += 1;
            // the function name, up to ':' or '}'
            let start = *i;
            while *i < t.len() && t[*i] != b':' && t[*i] != b'}' {
                *i += 1;
            }
            let name = &t[start..*i];
            let mut args = Vec::new();
            if *i < t.len() && t[*i] == b':' {
                *i += 1;
                loop {
                    args.push(parse(t, i, depth + 1, f));
                    if *i < t.len() && t[*i] == b'|' {
                        *i += 1;
                        continue;
                    }
                    break;
                }
            }
            if *i < t.len() && t[*i] == b'}' {
                *i += 1;
            }
            match f(&String::from_utf8_lossy(name).to_ascii_lowercase(), &args) {
                Some(v) => out.extend(v),
                None => {
                    out.push(b'{');
                    out.extend_from_slice(name);
                    out.push(b'}');
                }
            }
        }
        out
    }
    let func = |n: &str, args: &[Vec<u8>]| -> Option<Vec<u8>> {
        let a0 = args.first();
        Some(match n {
            "filename" => file_name.to_vec(),
            "input" => Vec::new(),
            "response" => response.to_vec(),
            "base64" if a0.is_some() => net::base64(a0?).into_bytes(),
            "header" if a0.is_some() => {
                headers.get(&lossy(a0?).to_lowercase()).map(|v| v.clone().into_bytes()).unwrap_or_default()
            }
            "json" if a0.is_some() => {
                let (src, path) = if args.len() > 1 { (&args[0][..], &args[1][..]) } else { (response, &args[0][..]) };
                Json::parse(src).and_then(|j| j.path(&lossy(path)).map(|v| v.text().into_bytes())).unwrap_or_default()
            }
            "regex" if a0.is_some() => {
                let Ok(re) = regex::bytes::Regex::new(&lossy(a0?)) else { return Some(Vec::new()) };
                let Some(m) = re.captures(response) else { return Some(Vec::new()) };
                let g = if args.len() > 1 { crate::snap::atoi(&lossy(&args[1])) } else if m.len() > 1 { 1 } else { 0 };
                if g >= 0 && (g as usize) < m.len() {
                    m.get(g as usize).map(|x| x.as_bytes().to_vec()).unwrap_or_default()
                } else {
                    Vec::new()
                }
            }
            "xml" if a0.is_some() => {
                // a simple element path: the text of the first element with the last name
                let p = a0?;
                let tag = &p[p.iter().rposition(|&c| c == b'/').map_or(0, |k| k + 1)..];
                let open = [b"<".as_slice(), tag].concat();
                let close = [b"</".as_slice(), tag].concat();
                let a = find(response, &open, 0).and_then(|a| find(response, b">", a));
                match a.and_then(|a| find(response, &close, a).map(|b| (a, b))) {
                    Some((a, b)) => response[a + 1..b].to_vec(),
                    None => Vec::new(),
                }
            }
            "random" if a0.is_some() => args[net::random_bytes(1)[0] as usize % args.len()].clone(),
            _ => return None,
        })
    };
    let mut i = 0;
    parse(text, &mut i, 0, &func)
}

// ---------------------------------------------------------------- upload

/// Uploads one file (blocking: call from a worker thread); `remote_name` = the name in the destination ("" = the
/// file's own).
pub fn upload_file(s: &Settings, path: &[u16], remote_name: &str) -> Outcome {
    if s.dest == Dest::None {
        return Outcome::fail(tr!("Nenhum destino de upload configurado."));
    }
    let name: String = if remote_name.is_empty() { String::from_utf16_lossy(&library::file_name(path)) } else { remote_name.to_owned() }
        .chars()
        .map(|c| if "\\/:*?\"<>|#%".contains(c) { '_' } else { c })
        .collect();
    crate::log::write(&format!("Upload: {} → {}", String::from_utf16_lossy(path), dest_name(s.dest)));
    let r = match s.dest {
        Dest::Ftp => upload_ftp(s, path, &name),
        Dest::Sftp => upload_sftp(s, path, &name),
        _ => {
            let data = std::fs::read(to_os(path)).ok().filter(|d| !d.is_empty() && d.len() < 2 << 30);
            match data {
                None => Outcome::fail(tr!("Não foi possível ler o arquivo.")),
                Some(data) => match s.dest {
                    Dest::S3 => upload_s3(s, &data, &name),
                    Dest::Dropbox => upload_dropbox(s, &data, &name),
                    Dest::GoogleDrive => upload_drive(s, &data, &name),
                    Dest::OneDrive => upload_onedrive(s, &data, &name),
                    _ => upload_custom(s, &data, &name),
                },
            }
        }
    };
    crate::log::write(&format!("Upload: {}", if r.ok { &r.url } else { &r.error }));
    r
}

/// UploadImageAsync's work (run it on a thread): the image as a PNG in %TEMP%\Snapnab (metadata stripped unless
/// StripMetadataOnUpload is 0), sent with the saved settings, the file deleted after.
pub fn upload_image(img: &crate::image::Image, name: &str, version: &str) -> Outcome {
    use std::os::windows::ffi::OsStrExt;
    let dir = std::env::temp_dir().join("Snapnab");
    let _ = std::fs::create_dir_all(&dir);
    let name: String = if name.is_empty() { format!("Snapnab_{}", String::from_utf16_lossy(&library::time_stamp(false))) } else { name.to_owned() };
    let name: String = name.chars().map(|c| if "\\/:*?\"<>|".contains(c) { '_' } else { c }).collect();
    let path = dir.join(format!("{name}.png"));
    let mut o = crate::codec::SaveOptions::from_settings();
    o.strip_metadata = crate::reg::get_dword(OsStr::new("StripMetadataOnUpload"), 1, None) != 0; // C29
    if !crate::codec::save_file(&path, img, crate::codec::Format::Png, &o, version) {
        return Outcome::fail(tr!("Não foi possível preparar a imagem."));
    }
    let w: Vec<u16> = path.as_os_str().encode_wide().collect();
    let r = upload_file(&Settings::load(), &w, "");
    let _ = std::fs::remove_file(&path);
    r
}

/// A .sxcu file's text: JSON with a RequestURL.
pub fn custom_valid(text: &[u8]) -> bool {
    Json::parse(text).is_some_and(|j| j.get("RequestURL").is_some())
}

// ---------------------------------------------------------------- uploaded links (Library\Uploads.xml, by file name)

fn uploads_file() -> W {
    [library::library_dir(), wide("\\Uploads.xml")].concat()
}

pub fn uploaded_url(item: &[u16]) -> String {
    uploaded_url_in(&uploads_file(), item)
}

pub fn set_uploaded_url(item: &[u16], url: &str) {
    set_uploaded_url_in(&uploads_file(), item, url)
}

fn uploaded_url_in(file: &[u16], item: &[u16]) -> String {
    let Some(n) = xml::load_file(Path::new(&to_os(file))) else { return String::new() };
    let key = library::lower(&library::file_name(item));
    n.children
        .iter()
        .find(|c| library::lower(&wide(c.attr("File").unwrap_or(""))) == key)
        .map(|c| c.attr("Url").unwrap_or("").to_owned())
        .unwrap_or_default()
}

fn set_uploaded_url_in(file: &[u16], item: &[u16], url: &str) {
    let path = to_os(file);
    let (mut n, _) = std::fs::read(&path).map(|d| xml::parse_partial(&d)).unwrap_or_else(|_| (Node::new("Uploads"), false));
    let key = library::file_name(item);
    let low = library::lower(&key);
    n.children.retain(|c| library::lower(&wide(c.attr("File").unwrap_or(""))) != low);
    if !url.is_empty() {
        n.add("Upload").set("File", String::from_utf16_lossy(&key)).set("Url", url);
    }
    if n.children.len() > 2000 {
        let extra = n.children.len() - 2000;
        n.children.drain(..extra);
    }
    xml::save_file(Path::new(&path), &n);
}

// ---------------------------------------------------------------- sign-in (OAuth 2.0 with PKCE)

/// One sign-in: the page to open in the browser, then the code (Dropbox: pasted by the user; Google and Microsoft:
/// the loopback redirect), then the refresh token stored in the Credential Manager.
pub struct SignIn {
    s: Settings,
    pkce: Pkce,
    lb: Option<Loopback>,
    redirect: String,
    state: String,
    pub url: String,
}

impl SignIn {
    pub fn begin(s: &Settings) -> Result<SignIn, String> {
        let pkce = Pkce::new();
        if s.dest == Dest::Dropbox {
            // Dropbox: no redirect — the page shows a code to paste
            let url = format!("https://www.dropbox.com/oauth2/authorize?response_type=code&token_access_type=offline&code_challenge_method=S256&client_id={}&code_challenge={}",
                              s.dropbox_app_key, pkce.challenge);
            return Ok(SignIn { s: s.clone(), pkce, lb: None, redirect: String::new(), state: String::new(), url });
        }
        let Some(lb) = Loopback::open() else {
            return Err(tr!("Não foi possível abrir a porta local para o retorno do navegador."));
        };
        let google = s.dest == Dest::GoogleDrive;
        let redirect = format!("{}{}", if google { "http://127.0.0.1:" } else { "http://localhost:" }, lb.port);
        let state = net::hex(&net::random_bytes(8));
        let mut url = if google {
            format!("https://accounts.google.com/o/oauth2/v2/auth?response_type=code&access_type=offline&prompt=consent&scope={}&client_id={}",
                    url_encode(b"https://www.googleapis.com/auth/drive.file", false), url_encode(s.google_client_id.as_bytes(), false))
        } else {
            format!("https://login.microsoftonline.com/common/oauth2/v2.0/authorize?response_type=code&scope={}&client_id={}",
                    url_encode(b"offline_access Files.ReadWrite", false), url_encode(s.ms_client_id.as_bytes(), false))
        };
        url += &format!("&redirect_uri={}&code_challenge_method=S256&code_challenge={}&state={state}",
                        url_encode(redirect.as_bytes(), false), pkce.challenge);
        Ok(SignIn { s: s.clone(), pkce, lb: Some(lb), redirect, state, url })
    }

    /// True when the browser came back to the loopback address (always false for Dropbox).
    pub fn poll(&mut self) -> bool {
        self.lb.as_mut().is_some_and(Loopback::poll)
    }

    /// Exchanges the code for the refresh token. Err("") when there is nothing to say (the user gave up).
    pub fn finish(&mut self, dropbox_code: &str) -> Result<(), String> {
        let s = &self.s;
        if s.dest == Dest::Dropbox {
            let code = String::from_utf16_lossy(library::trim(&wide(dropbox_code)));
            let r = post_form("https://api.dropboxapi.com/oauth2/token",
                              &form(&[("code", &code), ("grant_type", "authorization_code"), ("code_verifier", &self.pkce.verifier),
                                      ("client_id", &s.dropbox_app_key)]));
            let rt = json_field(&r.body, "refresh_token");
            if rt.is_empty() {
                return Err(err(&r, &tr!("O Dropbox recusou o código")));
            }
            net::save_secret("DropboxRefresh", &rt);
            return Ok(());
        }
        let Some(lb) = &self.lb else { return Err(String::new()) };
        if lb.code.is_empty() {
            return Err(lb.error.clone());
        }
        if lb.state != self.state {
            return Err(tr!("Resposta de login inválida."));
        }
        let google = s.dest == Dest::GoogleDrive;
        let r = if google {
            post_form("https://oauth2.googleapis.com/token",
                      &form(&[("code", &lb.code), ("grant_type", "authorization_code"), ("redirect_uri", &self.redirect),
                              ("code_verifier", &self.pkce.verifier), ("client_id", &s.google_client_id),
                              ("client_secret", &net::load_secret("GoogleClientSecret"))]))
        } else {
            post_form("https://login.microsoftonline.com/common/oauth2/v2.0/token",
                      &form(&[("code", &lb.code), ("grant_type", "authorization_code"), ("redirect_uri", &self.redirect),
                              ("code_verifier", &self.pkce.verifier), ("client_id", &s.ms_client_id),
                              ("scope", "offline_access Files.ReadWrite")]))
        };
        let rt = json_field(&r.body, "refresh_token");
        if rt.is_empty() {
            return Err(err(&r, &tr!("O login foi recusado")));
        }
        net::save_secret(if google { "GoogleRefresh" } else { "MicrosoftRefresh" }, &rt);
        if google {
            set_reg_string("GoogleDriveFolderId", "");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(ascii_json("a\"é\\\n"), "a\\\"\\u00e9\\\\\\u000a");
        assert_eq!(form(&[("a b", "c&d"), ("e", "")]), "a%20b=c%26d&e=");
        assert_eq!(mime_of("X.JPEG"), "image/jpeg");
        assert_eq!(mime_of("a.bin"), "application/octet-stream");
        assert_eq!(query_param(b"GET /?code=a%2Fb+c&state=xy HTTP/1.1\r\n", "code"), "a/b c");
        assert_eq!(query_param(b"GET /?code=a&state=xy HTTP/1.1\r\n", "state"), "xy");
        assert_eq!(query_param(b"GET / HTTP/1.1\r\n", "code"), "");
        let r = Response { status: 403, body: b"no".to_vec(), ..Default::default() };
        assert_eq!(err(&r, "Falha"), "Falha: HTTP 403 no");
    }

    #[test]
    fn settings_round_trip() {
        let dir = std::env::temp_dir().join(format!("snapcore_upload_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("Uploaders.xml");
        assert_eq!(Settings::load_from(&f), Settings::default());
        let s = Settings { dest: Dest::Custom, s3_bucket: "b".into(), port: 2222, public_link: false, custom_json: "{\"RequestURL\":\"x\"}".into(),
                           folder: "Pasta é".into(), ..Default::default() };
        assert!(s.save_to(&f));
        assert_eq!(Settings::load_from(&f), s);
        let text = std::fs::read_to_string(&f).unwrap();
        assert!(text.contains("Destination=\"7\"") && text.contains("<Custom>"), "{text}");
        let up = wide(&dir.join("Uploads.xml").to_string_lossy());
        set_uploaded_url_in(&up, &wide("C:\\x\\Shot.png"), "https://a");
        set_uploaded_url_in(&up, &wide("C:\\x\\other.png"), "https://b");
        assert_eq!(uploaded_url_in(&up, &wide("D:\\y\\shot.PNG")), "https://a");
        set_uploaded_url_in(&up, &wide("C:\\x\\shot.png"), "");
        assert_eq!(uploaded_url_in(&up, &wide("C:\\x\\Shot.png")), "");
        assert_eq!(uploaded_url_in(&up, &wide("other.png")), "https://b");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn esc(s: &[u8]) -> String {
        let mut o = String::new();
        for &c in s {
            if c == b'\\' {
                o.push_str("\\\\");
            } else if (0x20..0x7F).contains(&c) {
                o.push(c as char);
            } else {
                o.push_str(&format!("\\x{c:02x}"));
            }
        }
        o
    }

    /// The same results as the C++ code (tests/data/net-cpp.txt, written by `SnapnabTests.exe netgolden` before the
    /// move; NetFingerprints in tests.cpp makes the same lines).
    #[test]
    fn golden() {
        let mut l: Vec<String> = Vec::new();
        let ins: [&[u8]; 6] = [b"", b"abc", b"The quick brown fox jumps over the lazy dog", "cé ñ 日 🙂".as_bytes(), &[b'x'; 200],
                               b"\0\x01\x1f\x7f\xff\"\\\n\r\t~-_.*'()"];
        for s in ins {
            let slash = [s, b"/a b/"].concat();
            l.push(format!("sha {} b64u {} url {} urlslash {} esc {}", net::hex(&net::sha256(s)), net::base64_url(s), url_encode(s, false),
                           url_encode(&slash, true), esc(&net::json_escape_bytes(s))));
            l.push(format!("hmac {} {}", net::hex(&net::hmac_sha256(s, b"message")), net::hex(&net::hmac_sha256(&[b'k'; 100], s))));
        }
        let js: &[&str] = &[
            r#"{"a":[1,2,{"b":"c\u00e9"}],"t":true,"f":false,"n":null}"#,
            "  [ 1 , -2.5e3 , 0.1, 1e15, 1e14, 123456789012345678, -0, 3.0000000001, 1.00000000001, 2.5e-7, 1e300, 5e-324 ]  ",
            r#""\ud83d\ude42 \u0041\n\t\"\\\/ \b\f\r \x""#, r#"["\u0000x", "\u00ff", "\u0800", "\uFFFF"]"#,
            r#"{"k":"v",}"#, "[1,]", "{}", "[]", r#"{"a":{}}"#, r#""unterminated"#, "tru", "nul", "01", ".5", "+1", "1.", "-", "1e", "[1 2]",
            r#"{"a" 1}"#, r#"{"dup":1,"dup":2}"#, "[inf, -inf, nan, infinity, INF]", "0x10", "[0x1F, 0X0a]", "  ", "", "[1]x", "\t{\r\n}\n",
            r#"{"a":"b"} "#, "[true,false,null]", "[-1.5e+2, 1E-2, 0.000001, 1234567.125, 999999999999999, 1000000000000001]",
        ];
        for t in js {
            let d = Json::parse(t.as_bytes()).map_or("FAIL".to_owned(), |j| esc(j.dump().as_bytes()));
            l.push(format!("json [{}] -> {d}", esc(t.as_bytes())));
        }
        let deep = |n: usize| "[".repeat(n) + &"]".repeat(n);
        l.push(format!("json depth {} {}", Json::parse(deep(150).as_bytes()).is_some() as i32, Json::parse(deep(250).as_bytes()).is_some() as i32));
        let j = Json::parse(r#"{"data":{"link":"L","list":[{"x":1},{"x":"two"}],"it's":5,"q\"k":6},"n":2.50,"b":false,"s":"\u00e9"}"#.as_bytes()).unwrap();
        for p in ["data.link", "$.data.link", "$", "data.list[1].x", "data['it's']", r#"data["list"][0].x"#, "data.list[5]", "nope", "data..link",
                  "data.list[-1]", "n", "b", "data", "data.list[0", "[0]", "s", "$.n", ".n", "data.list[1]", "data.list.x"] {
            l.push(format!("path {p} -> {}", j.path(p).map_or("null".to_owned(), |v| format!("{} {}", v.kind(), esc(v.text().as_bytes())))));
        }
        let b = Json::obj().set("s", Json::str("x\"y")).set("n", Json::Number(2048.0)).set("f", Json::Number(0.5)).set("t", Json::Bool(true))
            .set("a", Json::arr().push(Json::Null).push(Json::obj()).push(Json::arr()));
        l.push(format!("build {}", esc(b.dump().as_bytes())));
        for (name, p) in [("gemini", crate::ai::GEMINI), ("anthropic", crate::ai::ANTHROPIC), ("openai", crate::ai::OPENAI)] {
            let (_, _, body) = crate::ai::request_parts(p, "", "m-1", "", "QUJD", "Explain\n\"this\" é");
            l.push(format!("ai {name} {}", esc(body.dump().as_bytes())));
        }
        let mut mp = Multipart::new();
        l.push(format!("multipart boundary {} {}", mp.boundary.len(), &mp.boundary[..19]));
        mp.boundary = "B".into();
        mp.field(b"a", b"1");
        mp.field("ü".as_bytes(), b"x\r\ny");
        mp.file(b"file", b"a b.png", "image/png", b"\x89PNG\0\x01");
        l.push(format!("multipart {} {}", esc(&mp.finish()), esc(mp.content_type().as_bytes())));
        let empty = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let map = |kv: &[(&str, &str)]| kv.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<BTreeMap<_, _>>();
        let hd = map(&[("host", "examplebucket.s3.amazonaws.com"), ("range", "bytes=0-9"), ("x-amz-content-sha256", empty),
                       ("x-amz-date", "20130524T000000Z")]);
        l.push(format!("sigv4 {}", sigv4_authorization("GET", "/test.txt", "", &hd, empty, "20130524T000000Z", "us-east-1", "s3",
                                                       "AKIAIOSFODNN7EXAMPLE", "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY")));
        let h2 = map(&[("content-type", "image/png"), ("host", "b.s3.eu-west-2.wasabisys.com"), ("x-amz-acl", "public-read"),
                       ("x-amz-content-sha256", "abc"), ("x-amz-date", "20260102T030405Z")]);
        l.push(format!("sigv4 {}", sigv4_authorization("PUT", "/snapnab/a%20b.png", "x=1&y=2", &h2, "abc", "20260102T030405Z", "eu-west-2",
                                                       "s3", "AK", "cé secret")));
        let resp = br#"{"data":{"link":"https://i.example/abc.png","id":7},"ok":true,"list":["a","b"]}"#;
        let hd = map(&[("location", "https://x/y"), ("x-id", "42")]);
        for t in ["{json:data.link}", "{json:data.id}", "{json:\\{\"a\":1\\}|a}", "name={filename}", "{FileName}", "[{input}]", "{response}",
                  "{base64:user:pass}", "{header:Location}", "[{header:missing}]", "{regex:\"id\":([0-9]+)}", "{regex:ok\"?:(t)(r)|2}",
                  "{regex:abc}", "[{regex:(}]", "{regex:link\":\"([^\"]+)|0}", "{regex:LINK}", "{unknown}", "{unknown:x|y}", "a\\{b\\}c\\\\d",
                  "{json:data.link", "x}y|z", "{base64:{filename}}", "{random:only}", "{json:list[1]}", "{json:$.ok}", "{JSON:data.id} {Json:nope}",
                  "{}", "{:a}", "{json:}", "{filename:ignored}", "tail\\", "{regex:[0-9]|5}", "{xml:a}"] {
            l.push(format!("expand [{}] -> [{}]", esc(t.as_bytes()), esc(&expand_syntax(t.as_bytes(), "shót 1.png".as_bytes(), resp, &hd))));
        }
        let xml = br#"<r><item a="1">val</item><list><item>two</item></list><e/></r>"#;
        for t in ["{xml:r/item}", "{xml:/r/list/item}", "{xml:e}", "{xml:none}", "{xml:list}"] {
            l.push(format!("expand [{}] -> [{}]", esc(t.as_bytes()), esc(&expand_syntax(t.as_bytes(), b"f", xml, &BTreeMap::new()))));
        }
        let want = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/data/net-cpp.txt")).unwrap();
        let want: Vec<&str> = want.lines().map(|x| x.trim_end_matches('\r')).filter(|x| !x.is_empty()).collect();
        let mut differ = 0;
        for i in 0..want.len().max(l.len()) {
            let (g, w) = (l.get(i).map_or("missing", |s| s.as_str()), want.get(i).copied().unwrap_or("missing"));
            if g != w {
                differ += 1;
                eprintln!("got  {g}\nwant {w}");
            }
        }
        assert!(want.len() == 110 && differ == 0, "{differ} of {} lines differ", want.len());
    }

    #[test]
    fn upload_refusals() {
        let none = Settings::default();
        assert!(!upload_file(&none, &wide("C:\\nothing.png"), "").ok);
        let custom = Settings { dest: Dest::Custom, custom_json: "{".into(), ..Default::default() };
        let missing = upload_file(&custom, &wide("C:\\does\\not\\exist.png"), "");
        assert!(!missing.ok && !missing.error.is_empty());
        assert!(custom_valid(br#"{"RequestURL":"https://x"}"#) && !custom_valid(b"{}") && !custom_valid(b"x"));
    }
}
