//! `http.request`'s send: the files measured, the placeholders filled, the body built (a
//! multipart form is written by hand, with a boundary the daemon makes), the key put in its one
//! place, one request sent on the checked client, and the answer shaped.
//!
//! The answer is the status line, a closed list of headers and the body. A 3xx is returned, not
//! followed. A 4xx or 5xx is an answer (`meta.failed`). Theseus's own errors carry nothing of the
//! server's; a request that may have reached the server and got no answer is `outcome_unknown`.

use std::path::PathBuf;
use std::time::Duration;

use base64::Engine as _;
use reqwest::header::{self, HeaderName, HeaderValue};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use theseus_tools::{External, ToolFailure, ToolOutput};

use super::{Checked, Input, Place, MAX_BODY};
use crate::web::{body, causes, html, status_line, Web};

/// The response headers the answer shows: what an upload or a paged API needs.
const SHOWN: [&str; 7] = [
    "content-type",
    "content-length",
    "location",
    "etag",
    "retry-after",
    "content-range",
    "range",
];

/// What the answer to a redirect says.
pub const NOT_FOLLOWED: &str = "Theseus follows no redirect with a key. If the new address is \
     the same service, call http.request there; to read it with no key, use http.fetch.";

/// A file the call sends: its part's name (None for a whole body), its bytes and their digest.
#[derive(Debug, Clone)]
pub struct Measured {
    pub name: Option<String>,
    pub bytes: Vec<u8>,
    pub sha256: [u8; 32],
}

impl Measured {
    /// A file not read: for checking the placeholders before anything is read.
    pub fn nothing(name: Option<&str>) -> Self {
        Self {
            name: name.map(str::to_string),
            bytes: Vec::new(),
            sha256: [0; 32],
        }
    }

    fn of(name: Option<String>, bytes: Vec<u8>) -> Self {
        let mut sha256 = [0u8; 32];
        sha256.copy_from_slice(&Sha256::digest(&bytes));
        Self {
            name,
            bytes,
            sha256,
        }
    }

    fn hex(&self) -> String {
        hex::encode(self.sha256)
    }
}

/// What one placeholder names: a file, and which of its facts.
fn placeholder<'a>(
    inner: &str,
    files: &'a [Measured],
) -> Result<(&'a Measured, &'static str), String> {
    let (which, fact) = inner.rsplit_once('.').ok_or_else(|| {
        format!("`{{{{{inner}}}}}` names no fact: bytes, sha256 or sha256_base64")
    })?;
    let fact = match fact {
        "bytes" => "bytes",
        "sha256" => "sha256",
        "sha256_base64" => "sha256_base64",
        _ => {
            return Err(format!(
                "`{{{{{inner}}}}}`: a file's facts are bytes, sha256 and sha256_base64 (no MD5)"
            ))
        }
    };
    let file = match which.strip_prefix("file:") {
        Some(part) => files
            .iter()
            .find(|f| f.name.as_deref() == Some(part))
            .ok_or_else(|| format!("`{{{{{inner}}}}}` names no file part of this call"))?,
        None if which == "file" => match files {
            [one] => one,
            [] => return Err(format!("`{{{{{inner}}}}}`: this call sends no file")),
            _ => {
                return Err(format!(
                    "`{{{{{inner}}}}}`: this call sends more than one file, so name its part, as \
                     `{{{{file:<part>.{fact}}}}}`"
                ))
            }
        },
        None => return Err(format!("`{{{{{inner}}}}}` is not a file's placeholder")),
    };
    Ok((file, fact))
}

fn fact_of(file: &Measured, fact: &str) -> String {
    match fact {
        "bytes" => file.bytes.len().to_string(),
        "sha256" => file.hex(),
        _ => base64::engine::general_purpose::STANDARD.encode(file.sha256),
    }
}

/// `s` with each file placeholder (`{{file.bytes}}`, `{{file:<part>.sha256}}`) filled. Other
/// braces are left as they are.
pub fn fill(s: &str, files: &[Measured]) -> Result<String, String> {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find("{{file") {
        out.push_str(&rest[..at]);
        let after = &rest[at + 2..];
        let Some(end) = after.find("}}") else {
            out.push_str(&rest[at..]);
            return Ok(out);
        };
        let inner = &after[..end];
        let (file, fact) = placeholder(inner, files)?;
        out.push_str(&fact_of(file, fact));
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

/// A JSON value with its strings filled; a string that is only a `bytes` placeholder becomes a
/// number.
fn fill_json(v: &Value, files: &[Measured]) -> Result<Value, String> {
    Ok(match v {
        Value::String(s) => {
            let t = s.trim();
            let whole =
                t.starts_with("{{") && t.ends_with("}}") && t[2..].find("}}") == Some(t.len() - 4);
            match whole {
                true => match placeholder(&t[2..t.len() - 2], files)? {
                    (f, "bytes") => json!(f.bytes.len()),
                    _ => Value::String(fill(s, files)?),
                },
                false => Value::String(fill(s, files)?),
            }
        }
        Value::Array(a) => Value::Array(
            a.iter()
                .map(|x| fill_json(x, files))
                .collect::<Result<_, _>>()?,
        ),
        Value::Object(o) => Value::Object(
            o.iter()
                .map(|(k, x)| Ok((k.clone(), fill_json(x, files)?)))
                .collect::<Result<_, String>>()?,
        ),
        other => other.clone(),
    })
}

/// The call's headers and its JSON and text, filled.
pub struct Filled {
    pub headers: Vec<(String, String)>,
    pub json: Option<Value>,
    pub text: Option<String>,
    pub parts: Vec<(Option<Value>, Option<String>)>,
}

/// The input's placeholders filled with `files`' facts, or why one names nothing.
pub fn filled(i: &Input, files: &[Measured]) -> Result<Filled, String> {
    let headers = i
        .headers
        .iter()
        .map(|(k, v)| Ok((k.clone(), fill(v, files)?)))
        .collect::<Result<_, String>>()?;
    let json = i.json.as_ref().map(|v| fill_json(v, files)).transpose()?;
    let text = i.text.as_deref().map(|t| fill(t, files)).transpose()?;
    let parts = i
        .form
        .iter()
        .flatten()
        .map(|p| {
            Ok((
                p.json.as_ref().map(|v| fill_json(v, files)).transpose()?,
                p.text.as_deref().map(|t| fill(t, files)).transpose()?,
            ))
        })
        .collect::<Result<_, String>>()?;
    Ok(Filled {
        headers,
        json,
        text,
        parts,
    })
}

/// Read the call's files: each resolved as `fs.read` resolves one, a regular file, together at
/// most the size a file tool reads.
async fn measure(files: &[(Option<String>, PathBuf)]) -> Result<Vec<Measured>, String> {
    let cap = theseus_files::MAX_FILE_BYTES;
    let mut total = 0u64;
    let mut out = Vec::new();
    for (name, path) in files {
        let shown = path.display();
        let meta = tokio::fs::metadata(path)
            .await
            .map_err(|e| format!("Not sent: {shown} could not be read ({})", e.kind()))?;
        if !meta.is_file() {
            return Err(format!("Not sent: {shown} is not a file"));
        }
        total += meta.len();
        if total > cap {
            return Err(format!(
                "Not sent: the files are over {}, the most a request sends",
                crate::narrative::bytes(cap)
            ));
        }
        let bytes = tokio::fs::read(path)
            .await
            .map_err(|e| format!("Not sent: {shown} could not be read ({})", e.kind()))?;
        out.push(Measured::of(name.clone(), bytes));
    }
    Ok(out)
}

/// A multipart form's body and its boundary, written by hand (RFC 7578): each part with its
/// name, a file part with its file name, each with its type.
pub fn form_body(
    i: &Input,
    filled: &Filled,
    files: &[Measured],
    paths: &[(Option<String>, PathBuf)],
) -> Result<(Vec<u8>, String), String> {
    let boundary = format!("theseus-{}", uuid::Uuid::new_v4().simple());
    let mut out: Vec<u8> = Vec::new();
    for (p, (json, text)) in i.form.iter().flatten().zip(&filled.parts) {
        let (data, ctype, filename): (Vec<u8>, String, Option<String>) = if let Some(v) = json {
            let data = serde_json::to_vec(v).map_err(|e| e.to_string())?;
            (data, "application/json".into(), None)
        } else if let Some(t) = text {
            (
                t.clone().into_bytes(),
                "text/plain; charset=utf-8".into(),
                None,
            )
        } else {
            let f = files
                .iter()
                .find(|f| f.name.as_deref() == Some(p.name.as_str()))
                .ok_or("a file part was not read")?;
            let own = paths
                .iter()
                .find(|(n, _)| n.as_deref() == Some(p.name.as_str()))
                .and_then(|(_, path)| path.file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("file")
                .replace(['"', '\r', '\n'], "_");
            (
                f.bytes.clone(),
                "application/octet-stream".into(),
                Some(p.filename.clone().unwrap_or(own)),
            )
        };
        if data.len() > MAX_BODY && filename.is_none() {
            return Err(format!("a part is at most {MAX_BODY} bytes"));
        }
        if data
            .windows(boundary.len())
            .any(|w| w == boundary.as_bytes())
        {
            return Err("a part holds the form's boundary; send it again".into());
        }
        let ctype = p.content_type.clone().unwrap_or(ctype);
        out.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        let disposition = match &filename {
            Some(f) => format!("form-data; name=\"{}\"; filename=\"{f}\"", p.name),
            None => format!("form-data; name=\"{}\"", p.name),
        };
        out.extend_from_slice(
            format!("Content-Disposition: {disposition}\r\nContent-Type: {ctype}\r\n\r\n")
                .as_bytes(),
        );
        out.extend_from_slice(&data);
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    Ok((out, boundary))
}

/// The request's body and its type, if it has one.
fn request_body(
    c: &Checked,
    filled: &Filled,
    files: &[Measured],
    paths: &[(Option<String>, PathBuf)],
) -> Result<Option<(Vec<u8>, String)>, String> {
    let i = &c.input;
    let typed = |default: &str| {
        i.content_type
            .clone()
            .unwrap_or_else(|| default.to_string())
    };
    Ok(if let Some(v) = &filled.json {
        let data = serde_json::to_vec(v).map_err(|e| e.to_string())?;
        Some((data, typed("application/json")))
    } else if let Some(t) = &filled.text {
        Some((t.clone().into_bytes(), typed("text/plain; charset=utf-8")))
    } else if i.form.is_some() {
        let (data, boundary) = form_body(i, filled, files, paths)?;
        Some((data, format!("multipart/form-data; boundary={boundary}")))
    } else if i.file.is_some() {
        let f = files
            .iter()
            .find(|f| f.name.is_none())
            .ok_or("the file was not read")?;
        Some((f.bytes.clone(), typed("application/octet-stream")))
    } else {
        None
    })
}

/// A header value that is the key: never printed by the client's `Debug`.
fn sensitive(s: &str) -> Result<HeaderValue, ToolFailure> {
    let mut v = HeaderValue::from_str(s)
        .map_err(|_| ToolFailure::new("Not sent: the key is not text a header can carry"))?;
    v.set_sensitive(true);
    Ok(v)
}

/// A failure that may have reached the server: whether it acted is not known.
fn unknown(message: String) -> ToolFailure {
    ToolFailure {
        message,
        meta: json!({"outcome_unknown": true}),
    }
}

/// The request, on the checked client: the model's headers, the key in its one place, and the
/// body with its type.
fn build(
    web: &Web,
    c: &Checked,
    key: &str,
    filled: &Filled,
    body: Option<(Vec<u8>, String)>,
) -> Result<reqwest::RequestBuilder, ToolFailure> {
    let mut req = web.client(false)?.request(c.method.clone(), c.url.clone());
    for (k, v) in &filled.headers {
        let name = HeaderName::from_bytes(k.as_bytes())
            .map_err(|_| ToolFailure::new(format!("Not sent: `{k}` is not a header's name")))?;
        let value = HeaderValue::from_str(v).map_err(|_| {
            ToolFailure::new(format!(
                "Not sent: the header `{k}`'s value is not one line"
            ))
        })?;
        req = req.header(name, value);
    }
    req = match &c.input.key.place {
        Place::Bearer {} => req.header(header::AUTHORIZATION, sensitive(&format!("Bearer {key}"))?),
        Place::Header { name, prefix } => {
            let name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
                ToolFailure::new("Not sent: the key's header is not a header's name")
            })?;
            let value = format!("{}{key}", prefix.as_deref().unwrap_or(""));
            req.header(name, sensitive(&value)?)
        }
        Place::Basic { username } => req.basic_auth(username, Some(key)),
    };
    if let Some((data, ctype)) = body {
        req = req.header(header::CONTENT_TYPE, ctype).body(data);
    }
    Ok(req)
}

/// What came back: the status, the shown headers, the type, the length, the body and whether
/// it was cut.
type Answer = (
    reqwest::StatusCode,
    Vec<(String, String)>,
    String,
    Option<u64>,
    Vec<u8>,
    bool,
);

/// Send `req` and read its answer, within `secs`. A 3xx's body is not read.
async fn receive(
    web: &Web,
    req: reqwest::RequestBuilder,
    secs: u64,
    host: &str,
) -> Result<Answer, ToolFailure> {
    let read = async {
        let mut resp = req.send().await.map_err(|e| failed(host, &e))?;
        let status = resp.status();
        let shown: Vec<(String, String)> = SHOWN
            .iter()
            .filter_map(|h| {
                let v = resp.headers().get(*h)?.to_str().ok()?;
                Some((h.to_string(), v.to_string()))
            })
            .collect();
        let mime = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(';').next())
            .map(|v| v.trim().to_ascii_lowercase())
            .unwrap_or_default();
        let length = resp.content_length();
        let (data, truncated) = match status.is_redirection() {
            false => body(&mut resp, web.cfg.max_bytes).await.map_err(|e| {
                unknown(format!(
                    "Sent, and the answer from {host} could not be read ({}): whether it acted on \
                     it is not known",
                    causes(&e)
                ))
            })?,
            true => (Vec::new(), false),
        };
        Ok::<Answer, ToolFailure>((status, shown, mime, length, data, truncated))
    };
    tokio::time::timeout(Duration::from_secs(secs), read)
        .await
        .map_err(|_| {
            unknown(format!(
                "Sent, and no answer came within {secs} s ([tools.web] timeout_secs): whether \
                 {host} acted on it is not known"
            ))
        })?
}

/// Send the checked call with `key` in its place, and shape the answer.
pub async fn send(
    web: &Web,
    c: &Checked,
    key: &str,
    paths: &[(Option<String>, PathBuf)],
) -> Result<(ToolOutput, Option<External>), ToolFailure> {
    let not_sent = |e: String| ToolFailure::new(format!("Not sent: {e}"));
    let files = measure(paths).await.map_err(ToolFailure::new)?;
    let filled = filled(&c.input, &files).map_err(not_sent)?;
    let body = request_body(c, &filled, &files, paths).map_err(not_sent)?;
    let req = build(web, c, key, &filled, body)?;
    let secs = match files.is_empty() {
        true => web.cfg.timeout_secs,
        false => web.cfg.timeout_secs.saturating_mul(4),
    };
    let (status, shown, mime, length, data, truncated) = receive(web, req, secs, &c.host()).await?;
    let text = answer_text(
        c,
        (status, &shown, &mime, length),
        (&data, truncated),
        &files,
    );
    let sent: Vec<Value> = files
        .iter()
        .map(|f| json!({"part": f.name, "bytes": f.bytes.len(), "sha256": f.hex()}))
        .collect();
    let meta = json!({
        "status": status.as_u16(),
        "content_type": mime,
        "bytes": data.len(),
        "truncated": truncated,
        "sent": sent,
        "failed": status.is_client_error() || status.is_server_error(),
    });
    Ok((
        ToolOutput {
            text: web.fit(text),
            meta,
        },
        Some(External { url: c.marker() }),
    ))
}

/// Why a request failed before an answer: a connection that never opened was not sent; anything
/// else may have reached the server.
fn failed(host: &str, e: &reqwest::Error) -> ToolFailure {
    if let Some(r) = crate::web::net::refused_in(e) {
        return ToolFailure::new(format!(
            "Not sent: {r}, so Theseus did not connect. {}",
            super::PUBLIC_ONLY
        ));
    }
    if e.is_connect() || e.is_builder() {
        return ToolFailure::new(format!("Not sent: {}", causes(e)));
    }
    unknown(format!(
        "Sent, and no answer came from {host} ({}): whether it acted on it is not known",
        causes(e)
    ))
}

/// The answer's text: the first line, the shown headers, what was sent, and the body.
fn answer_text(
    c: &Checked,
    (status, shown, mime, length): (reqwest::StatusCode, &[(String, String)], &str, Option<u64>),
    (data, truncated): (&[u8], bool),
    files: &[Measured],
) -> String {
    let size = crate::narrative::bytes;
    let ctype = if mime.is_empty() {
        "no content type"
    } else {
        mime
    };
    let mut text = format!(
        "{} {}{} → {}, {ctype}, {}",
        c.method,
        c.host(),
        c.path(),
        status_line(status),
        size(length.unwrap_or(data.len() as u64))
    );
    if truncated {
        text.push_str(&format!(
            ", the first {} ([tools.web] max_bytes)",
            size(data.len() as u64)
        ));
    }
    text.push_str(".\n");
    for (k, v) in shown {
        text.push_str(&format!("{k}: {v}\n"));
    }
    for f in files {
        let part = f
            .name
            .as_deref()
            .map_or(String::new(), |n| format!(" (part {n})"));
        text.push_str(&format!(
            "Sent a file{part}: {} bytes, sha256 {}.\n",
            f.bytes.len(),
            f.hex()
        ));
    }
    if status.is_redirection() {
        text.push_str(NOT_FOLLOWED);
        text.push('\n');
        return text;
    }
    text.push('\n');
    match crate::web::fetch::kind(mime, data) {
        crate::web::fetch::Kind::Html => {
            let page = html::to_text(&String::from_utf8_lossy(data), Some(&c.url));
            text.push_str(&page.text);
        }
        crate::web::fetch::Kind::Text => text.push_str(&String::from_utf8_lossy(data)),
        _ if data.is_empty() => {}
        _ => text.push_str(&format!(
            "[a body of {} that is not text, not read]\n",
            size(data.len() as u64)
        )),
    }
    text
}
