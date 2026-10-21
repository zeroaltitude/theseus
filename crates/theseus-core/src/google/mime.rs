//! Mail's wire shapes, pure (the Google design §3.1, rows 2 and 3): the RFC
//! 2822 message a draft is made of, with RFC 2047 words for anything not
//! ASCII; the threading headers; and the reverse, a Gmail `payload` read into
//! headers, plain text (HTML turned to text, quoted tails folded) and the
//! attachments it lists. Nothing here touches the network or the disk.

use base64::engine::general_purpose::{STANDARD, URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine;
use serde_json::Value;

/// A draft's headers and body, as the model gave them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Draft {
    pub from: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub body: String,
    /// The `Message-ID` replied to, and the thread's `References` with it last.
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
}

/// A header value must be one line: a CR or LF in an address or a subject is
/// a second header, so it is refused rather than escaped.
pub fn one_line(what: &str, v: &str) -> Result<(), String> {
    if v.chars().any(|c| c == '\r' || c == '\n' || c == '\0') {
        return Err(format!("{what} must be one line"));
    }
    Ok(())
}

/// The bare address of `Name <a@b>` or `a@b`, lowercased; None when it has no
/// `@` or holds a space or a control character.
pub fn address_of(s: &str) -> Option<String> {
    let inner = match (s.rfind('<'), s.rfind('>')) {
        (Some(a), Some(b)) if a < b => &s[a + 1..b],
        _ => s,
    }
    .trim();
    let (local, domain) = inner.split_once('@')?;
    let ok = !local.is_empty()
        && domain.contains('.')
        && !inner
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || "<>,;\"".contains(c));
    ok.then(|| inner.to_ascii_lowercase())
}

/// Every address of an address-list header (`To`, `Cc`), lowercased. A comma
/// inside a quoted name is not a separator.
pub fn addresses(header: &str) -> Vec<String> {
    let (mut out, mut part, mut quoted, mut angle) = (Vec::new(), String::new(), false, false);
    for c in header.chars() {
        match c {
            '"' => quoted = !quoted,
            '<' if !quoted => angle = true,
            '>' if !quoted => angle = false,
            ',' | ';' if !quoted && !angle => {
                out.extend(address_of(&part));
                part.clear();
                continue;
            }
            _ => {}
        }
        part.push(c);
    }
    out.extend(address_of(&part));
    out
}

/// `s` as RFC 2047 encoded words when it is not plain printable ASCII, each
/// word's payload at most 45 bytes (so a word stays under 75 characters) and
/// cut only between characters.
pub fn encode_word(s: &str) -> String {
    if s.bytes().all(|b| (0x20..0x7f).contains(&b)) {
        return s.to_string();
    }
    let mut words = Vec::new();
    let mut chunk = String::new();
    for c in s.chars() {
        if chunk.len() + c.len_utf8() > 45 {
            words.push(std::mem::take(&mut chunk));
        }
        chunk.push(c);
    }
    words.push(chunk);
    words
        .iter()
        .map(|w| format!("=?UTF-8?B?{}?=", STANDARD.encode(w)))
        .collect::<Vec<_>>()
        .join("\r\n ")
}

/// `Name <a@b>` with the name encoded when it needs it; a bare address as is.
fn encode_address(a: &str) -> String {
    match (a.find('<'), a.rfind('>')) {
        (Some(i), Some(_)) if i > 0 => {
            let name = a[..i].trim().trim_matches('"');
            let addr = address_of(a).unwrap_or_default();
            // An encoded word is never inside a quoted string (RFC 2047).
            if !name.is_ascii() {
                format!("{} <{addr}>", encode_word(name))
            } else if name.contains(['"', '\\', ',']) {
                let plain = name.replace(['\\', '"'], "");
                format!("\"{plain}\" <{addr}>")
            } else {
                format!("{name} <{addr}>")
            }
        }
        _ => address_of(a).unwrap_or_else(|| a.trim().to_string()),
    }
}

/// `Re: ` before a subject that does not have it already.
pub fn reply_subject(subject: &str) -> String {
    let s = subject.trim();
    if s.len() >= 3 && s[..3].eq_ignore_ascii_case("re:") {
        s.to_string()
    } else {
        format!("Re: {s}")
    }
}

/// The message, as RFC 2822 with CRLF lines: the body is UTF-8 in base64, so
/// no line is too long and no byte is lost on the way.
pub fn build(d: &Draft) -> Result<String, String> {
    one_line("a subject", &d.subject)?;
    let mut lines = vec![format!("From: {}", encode_address(&d.from))];
    for (name, list) in [("To", &d.to), ("Cc", &d.cc), ("Bcc", &d.bcc)] {
        for a in list {
            one_line("an address", a)?;
            if address_of(a).is_none() {
                return Err(format!("`{a}` is not an email address"));
            }
        }
        if !list.is_empty() {
            let all: Vec<String> = list.iter().map(|a| encode_address(a)).collect();
            lines.push(format!("{name}: {}", all.join(", ")));
        }
    }
    lines.push(format!("Subject: {}", encode_word(&d.subject)));
    if let Some(id) = &d.in_reply_to {
        one_line("a message id", id)?;
        lines.push(format!("In-Reply-To: {id}"));
        let mut refs = d.references.clone();
        if !refs.contains(id) {
            refs.push(id.clone());
        }
        for r in &refs {
            one_line("a reference", r)?;
        }
        lines.push(format!("References: {}", refs.join(" ")));
    }
    lines.push("MIME-Version: 1.0".into());
    lines.push("Content-Type: text/plain; charset=UTF-8".into());
    lines.push("Content-Transfer-Encoding: base64".into());
    let body = STANDARD.encode(d.body.replace("\r\n", "\n").replace('\n', "\r\n"));
    let wrapped: Vec<&str> = body
        .as_bytes()
        .chunks(76)
        .map(|c| std::str::from_utf8(c).unwrap_or_default())
        .collect();
    Ok(format!(
        "{}\r\n\r\n{}\r\n",
        lines.join("\r\n"),
        wrapped.join("\r\n")
    ))
}

/// The base64url form Gmail's `raw` takes.
pub fn raw(message: &str) -> String {
    URL_SAFE_NO_PAD.encode(message)
}

/// Bytes from the base64url Gmail uses, padded or not.
pub fn unbase64(s: &str) -> Vec<u8> {
    URL_SAFE_NO_PAD
        .decode(s.trim_end_matches('='))
        .or_else(|_| URL_SAFE.decode(s))
        .unwrap_or_default()
}

/// An RFC 2047 header read back to text (`=?UTF-8?B?…?=` and `?Q?`).
pub fn decode_words(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    let mut after_word = false;
    while let Some(start) = rest.find("=?") {
        let (before, tail) = rest.split_at(start);
        let Some((word, after)) = encoded_word(tail) else {
            out.push_str(&rest[..start + 2]);
            rest = &rest[start + 2..];
            after_word = false;
            continue;
        };
        // Whitespace between two encoded words is not text.
        if !(after_word && before.trim().is_empty()) {
            out.push_str(before);
        }
        out.push_str(&word);
        rest = after;
        after_word = true;
    }
    out.push_str(rest);
    out
}

fn encoded_word(tail: &str) -> Option<(String, &str)> {
    let body = tail.strip_prefix("=?")?;
    let end = body.find("?=")?;
    let mut parts = body[..end].splitn(3, '?');
    let (charset, enc, text) = (parts.next()?, parts.next()?, parts.next()?);
    let bytes = match enc.to_ascii_uppercase().as_str() {
        "B" => STANDARD.decode(text).ok()?,
        "Q" => q_decode(text),
        _ => return None,
    };
    let word = if charset.eq_ignore_ascii_case("utf-8") {
        String::from_utf8_lossy(&bytes).into_owned()
    } else {
        bytes.iter().map(|b| *b as char).collect()
    };
    Some((word, &body[end + 2..]))
}

fn q_decode(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'_' => out.push(b' '),
            b'=' if b.len() >= i + 3 => {
                let hex = std::str::from_utf8(&b[i + 1..i + 3])
                    .ok()
                    .and_then(|h| u8::from_str_radix(h, 16).ok());
                out.push(hex.unwrap_or(b'='));
                i += if hex.is_some() { 2 } else { 0 };
            }
            c => out.push(c),
        }
        i += 1;
    }
    out
}

/// A header's value in a Gmail payload (case-insensitive), decoded.
pub fn header(payload: &Value, name: &str) -> Option<String> {
    payload["headers"]
        .as_array()?
        .iter()
        .find(|h| {
            h["name"]
                .as_str()
                .is_some_and(|n| n.eq_ignore_ascii_case(name))
        })
        .and_then(|h| h["value"].as_str())
        .map(decode_words)
}

/// A file a message lists: its name, type, size and where to fetch it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    pub name: String,
    pub mime: String,
    pub size: u64,
    pub id: String,
}

/// What a message says: its text and its files.
#[derive(Debug, Default)]
pub struct Body {
    /// The text with its quoted tails folded.
    pub text: String,
    /// The text whole.
    pub raw: String,
    /// The HTML part as written, even when a plain part gave the text.
    pub html: String,
    pub files: Vec<Attachment>,
}

/// The message's readable text (a plain part, else an HTML one turned to
/// text), with each quoted tail folded, and the files it lists.
pub fn read(payload: &Value) -> Body {
    let (mut plain, mut html, mut files) = (None, None, Vec::new());
    walk(payload, &mut plain, &mut html, &mut files);
    let text = match (&plain, &html) {
        (Some(p), _) => p.clone(),
        (None, Some(h)) => html_text(h),
        (None, None) => String::new(),
    };
    Body {
        text: fold_quoted(text.trim()),
        raw: text,
        html: html.unwrap_or_default(),
        files,
    }
}

fn walk(
    part: &Value,
    plain: &mut Option<String>,
    html: &mut Option<String>,
    files: &mut Vec<Attachment>,
) {
    let mime = part["mimeType"].as_str().unwrap_or("");
    let name = part["filename"].as_str().unwrap_or("");
    let body = &part["body"];
    if !name.is_empty() {
        if let Some(id) = body["attachmentId"].as_str() {
            files.push(Attachment {
                name: name.to_string(),
                mime: mime.to_string(),
                size: body["size"].as_u64().unwrap_or(0),
                id: id.to_string(),
            });
        }
    } else if let Some(data) = body["data"].as_str() {
        let text = String::from_utf8_lossy(&unbase64(data)).into_owned();
        match mime {
            "text/plain" if plain.is_none() => *plain = Some(text),
            "text/html" if html.is_none() => *html = Some(text),
            _ => {}
        }
    }
    for p in part["parts"].as_array().into_iter().flatten() {
        walk(p, plain, html, files);
    }
}

/// HTML mail as text, its quoted tail (Gmail's `gmail_quote`, a
/// `blockquote`) cut before the conversion so the folding sees plain lines.
fn html_text(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let cut = ["<div class=\"gmail_quote", "<blockquote"]
        .iter()
        .filter_map(|m| lower.find(m))
        .min()
        .unwrap_or(html.len());
    let text = crate::web::html::to_text(&html[..cut], None).text;
    if cut < html.len() {
        format!("{}\n> [quoted text]", text.trim_end())
    } else {
        text
    }
}

/// Quoted lines (`> …`) and the line that introduces them (`On … wrote:`)
/// folded into one marker, so a long thread is not read again at every reply.
pub fn fold_quoted(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let quoted = |l: &str| l.trim_start().starts_with('>');
        let intro =
            lines[i].trim_end().ends_with("wrote:") && lines.get(i + 1).is_some_and(|l| quoted(l));
        if quoted(lines[i]) || intro {
            let start = i;
            while i < lines.len() && (quoted(lines[i]) || i == start) {
                i += 1;
            }
            let n = lines[start..i].iter().filter(|l| quoted(l)).count();
            if !(out.last().is_some_and(|l| l.starts_with("[quoted text"))) {
                out.push(format!("[quoted text folded: {n} line(s)]"));
            }
        } else {
            out.push(lines[i].to_string());
            i += 1;
        }
    }
    out.join("\n")
}

/// A file name that is only a name: no directory, no dot first, no control
/// character, never empty.
pub fn safe_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let clean: String = base
        .chars()
        .map(|c| if c.is_control() { '_' } else { c })
        .collect();
    let clean = clean.trim_start_matches('.').trim();
    if clean.is_empty() {
        "attachment".into()
    } else {
        clean.chars().take(120).collect()
    }
}
