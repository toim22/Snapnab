//! The AI assistant's request (C22, Assistant.cpp): one question about a capture to the provider the user picked
//! (OpenAI, Google Gemini, Anthropic, an OpenAI-compatible endpoint or a local model through Ollama's OpenAI-compatible
//! API), with the user's own key. The window, the settings and the key's storage stay with the interface.

use crate::i18n::fill;
use crate::net::{self, Json};
use crate::tr;

pub const OPENAI: i32 = 0;
pub const GEMINI: i32 = 1;
pub const ANTHROPIC: i32 = 2;

/// The request: its URL, headers and JSON body. `png_b64` is the image as base64; `question` already carries the
/// "(Answer in …)" line.
pub fn request_parts(provider: i32, endpoint: &str, model: &str, key: &str, png_b64: &str, question: &str) -> (String, Vec<String>, Json) {
    let base = endpoint.trim_end_matches('/');
    let mut headers = vec!["Content-Type: application/json".to_owned()];
    let q = || Json::str(question);
    let (url, body) = match provider {
        GEMINI => {
            headers.push(format!("x-goog-api-key: {key}"));
            let parts = Json::arr()
                .push(Json::obj().set("text", q()))
                .push(Json::obj().set("inline_data", Json::obj().set("mime_type", Json::str("image/png")).set("data", Json::str(png_b64))));
            (format!("{base}/models/{model}:generateContent"),
             Json::obj().set("contents", Json::arr().push(Json::obj().set("parts", parts))))
        }
        ANTHROPIC => {
            headers.push(format!("x-api-key: {key}"));
            headers.push("anthropic-version: 2023-06-01".into());
            let content = Json::arr()
                .push(Json::obj().set("type", Json::str("image")).set(
                    "source",
                    Json::obj().set("type", Json::str("base64")).set("media_type", Json::str("image/png")).set("data", Json::str(png_b64)),
                ))
                .push(Json::obj().set("type", Json::str("text")).set("text", q()));
            (format!("{base}/messages"),
             Json::obj().set("model", Json::str(model)).set("max_tokens", Json::Number(2048.0))
                 .set("messages", Json::arr().push(Json::obj().set("role", Json::str("user")).set("content", content))))
        }
        _ => {
            // OpenAI and compatible (also Ollama's /v1)
            if !key.is_empty() {
                headers.push(format!("Authorization: Bearer {key}"));
            }
            let content = Json::arr()
                .push(Json::obj().set("type", Json::str("text")).set("text", q()))
                .push(Json::obj().set("type", Json::str("image_url"))
                          .set("image_url", Json::obj().set("url", Json::str(format!("data:image/png;base64,{png_b64}")))));
            (format!("{base}/chat/completions"),
             Json::obj().set("model", Json::str(model))
                 .set("messages", Json::arr().push(Json::obj().set("role", Json::str("user")).set("content", content))))
        }
    };
    (url, headers, body)
}

/// The answer in an HTTP reply (line breaks as CR LF), or the error to show.
pub fn read_answer(provider: i32, r: &net::Response) -> Result<String, String> {
    if r.status == 0 {
        return Err(tr!("Sem resposta do provedor: ") + &r.error);
    }
    let j = Json::parse(&r.body);
    if !r.ok() {
        let msg = j.as_ref().and_then(|j| {
            ["error.message", "error.error.message", "message", "error"]
                .iter()
                .filter_map(|p| j.path(p).map(Json::text))
                .find(|t| !t.is_empty())
        });
        let msg = msg.unwrap_or_else(|| String::from_utf8_lossy(&r.body[..r.body.len().min(300)]).into_owned());
        return Err(fill(&tr!("O provedor respondeu %d: "), &[&r.status]) + &msg);
    }
    let Some(j) = j else { return Err(tr!("Resposta inválida do provedor.")) };
    let path = match provider {
        GEMINI => "candidates[0].content.parts[0].text",
        ANTHROPIC => "content[0].text",
        _ => "choices[0].message.content",
    };
    let text = j.path(path).map(Json::text).unwrap_or_default();
    if text.is_empty() {
        return Err(tr!("O provedor não devolveu texto."));
    }
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c == '\n' && !out.ends_with('\r') {
            out.push('\r');
        }
        out.push(c);
    }
    Ok(out)
}

/// One question about a PNG image (blocking: call from a worker thread).
pub fn ask(provider: i32, endpoint: &str, model: &str, key: &str, png: &[u8], question: &str) -> Result<String, String> {
    let (url, headers, body) = request_parts(provider, endpoint, model, key, &net::base64(png), question);
    read_answer(provider, &net::request("POST", &url, &headers, body.dump().as_bytes(), 120000))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers() {
        let r = net::Response { status: 200, body: br#"{"content":[{"text":"a\nb"}]}"#.to_vec(), ..Default::default() };
        assert_eq!(read_answer(ANTHROPIC, &r).unwrap(), "a\r\nb");
        let r = net::Response { status: 401, body: br#"{"error":{"message":"bad key"}}"#.to_vec(), ..Default::default() };
        assert!(read_answer(OPENAI, &r).unwrap_err().ends_with("bad key"));
        let r = net::Response { status: 200, body: b"{}".to_vec(), ..Default::default() };
        assert!(read_answer(GEMINI, &r).is_err());
        let (url, h, _) = request_parts(GEMINI, "https://g/v1beta/", "m", "k", "", "q");
        assert_eq!(url, "https://g/v1beta/models/m:generateContent");
        assert_eq!(h[1], "x-goog-api-key: k");
    }
}
