//! Discord attachments (theseus-9g2). Discord turns a long paste into a
//! `message.txt` attachment, so a binding that only listed attachments
//! silently missed every long paste. Text files and images (PNG, JPEG, GIF,
//! WebP, up to 5 MiB) are downloaded; anything else is listed.
//!
//! The gateway loop awaits each message's handler inline, so nothing here
//! runs on it: `on_message` plans each file (`plan`, pure), and the downloads
//! run in a spawned task whose handle rides with the message. The place's
//! submit task awaits it before `turn.submit`, which keeps the order and
//! never blocks the loop. `entry` turns metadata plus what a download
//! returned into the `attachments` entry, and it is the unit-tested part. A
//! failed download never loses the message: the turn runs, and the entry
//! says the download failed.

use std::time::Duration;

use theseus_protocol::Attachment;

/// A whole download, connection included.
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// What the binding knows about one attachment before it downloads anything.
#[derive(Debug, Clone, PartialEq)]
pub struct FileMeta {
    pub name: String,
    pub content_type: Option<String>,
    pub size: u64,
    pub url: String,
}

impl FileMeta {
    pub fn of(a: &twilight_model::channel::Attachment) -> Self {
        Self {
            name: a.filename.clone(),
            content_type: a.content_type.clone(),
            size: a.size,
            url: a.url.clone(),
        }
    }

    /// The media type without its parameters, lowercased (`text/plain`).
    fn media_type(&self) -> String {
        self.content_type
            .as_deref()
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase()
    }
}

/// What to do with one attachment.
#[derive(Debug, Clone, PartialEq)]
pub enum Plan {
    /// Download it and pass its text.
    Text,
    /// Download it and pass its bytes; the core checks them and stores them.
    Image,
    /// List it without reading it, for this reason.
    Skip(String),
}

/// The image types the models read (the core checks the bytes themselves).
fn image_type(t: &str, name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let ext = lower.rsplit_once('.').map_or("", |(_, e)| e);
    matches!(t, "image/png" | "image/jpeg" | "image/gif" | "image/webp")
        || (t.is_empty() && matches!(ext, "png" | "jpg" | "jpeg" | "gif" | "webp"))
}

/// Extensions read as text whatever type Discord reports (it reports none
/// for many source files).
const TEXT_EXTENSIONS: &[&str] = &[
    "txt",
    "md",
    "markdown",
    "rst",
    "json",
    "jsonl",
    "ndjson",
    "yaml",
    "yml",
    "toml",
    "ini",
    "cfg",
    "conf",
    "csv",
    "tsv",
    "log",
    "xml",
    "html",
    "htm",
    "css",
    "svg",
    "sh",
    "bash",
    "zsh",
    "fish",
    "py",
    "rs",
    "go",
    "js",
    "mjs",
    "cjs",
    "ts",
    "tsx",
    "jsx",
    "java",
    "kt",
    "swift",
    "c",
    "h",
    "cc",
    "cpp",
    "hpp",
    "rb",
    "php",
    "pl",
    "lua",
    "sql",
    "diff",
    "patch",
    "lock",
    "tex",
    "proto",
    "graphql",
    "dockerfile",
    "makefile",
    "gitignore",
];

/// Media types read as text besides `text/*`.
fn text_media_type(t: &str) -> bool {
    t.starts_with("text/")
        || t.ends_with("+json")
        || t.ends_with("+xml")
        || t.ends_with("+yaml")
        || matches!(
            t,
            "application/json"
                | "application/x-ndjson"
                | "application/yaml"
                | "application/x-yaml"
                | "application/toml"
                | "application/x-toml"
                | "application/xml"
                | "application/javascript"
                | "application/x-javascript"
                | "application/typescript"
                | "application/x-sh"
                | "application/x-shellscript"
                | "application/sql"
                | "application/x-diff"
                | "application/x-patch"
        )
}

fn text_extension(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let ext = lower.rsplit_once('.').map_or(lower.as_str(), |(_, e)| e);
    TEXT_EXTENSIONS.contains(&ext)
}

/// Decide, from the metadata alone, whether to download a file.
pub fn plan(f: &FileMeta, max_text: u64) -> Plan {
    let t = f.media_type();
    if text_media_type(&t) || text_extension(&f.name) {
        if f.size > max_text {
            return Plan::Skip(format!(
                "over the limit for text ({})",
                theseus_core::attach::size_words(max_text)
            ));
        }
        return Plan::Text;
    }
    if image_type(&t, &f.name) {
        if f.size > theseus_protocol::MAX_IMAGE_BYTES {
            return Plan::Skip("an image over the 5 MiB limit".into());
        }
        return Plan::Image;
    }
    if t.starts_with("image/") {
        return Plan::Skip("only PNG, JPEG, GIF, and WebP images are read".into());
    }
    Plan::Skip("only text files and images are read".into())
}

/// The `attachments` entry for one file: its metadata, its plan, and what
/// its download returned (`None` when it was not downloaded).
pub fn entry(f: &FileMeta, plan: &Plan, fetched: Option<Result<Vec<u8>, String>>) -> Attachment {
    let mut a = Attachment {
        name: f.name.clone(),
        media_type: f.content_type.clone().unwrap_or_default(),
        size: f.size,
        ..Default::default()
    };
    match (plan, fetched) {
        (Plan::Skip(reason), _) => a.not_read = Some(reason.clone()),
        (Plan::Text, Some(Ok(bytes))) => {
            a.text = Some(match String::from_utf8(bytes) {
                Ok(s) => s,
                Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
            });
        }
        (Plan::Image, Some(Ok(bytes))) => a.data = Some(theseus_core::blobs::encode(&bytes)),
        (Plan::Text | Plan::Image, Some(Err(e))) => {
            a.not_read = Some(format!("the download failed ({e})"))
        }
        (Plan::Text | Plan::Image, None) => a.not_read = Some("the download did not run".into()),
    }
    a
}

/// The client every download uses: bounded in time, like every other call.
pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(format!("theseus/{}", theseus_core::VERSION))
        .timeout(FETCH_TIMEOUT)
        .connect_timeout(CONNECT_TIMEOUT)
        .build()
        .unwrap_or_default()
}

/// Download one file, reading at most `cap` bytes plus one: a file longer
/// than its reported size arrives cut, and the core marks the cut.
async fn fetch(http: &reqwest::Client, url: &str, cap: u64) -> Result<Vec<u8>, String> {
    let mut resp = http.get(url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    let mut out = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
        out.extend_from_slice(&chunk);
        if out.len() as u64 > cap {
            out.truncate(cap as usize + 1);
            break;
        }
    }
    Ok(out)
}

/// Every file of one message, in order: downloaded when its plan says so,
/// listed with the reason when not. Never fails.
pub async fn fetch_all(
    http: reqwest::Client,
    files: Vec<FileMeta>,
    max_text: u64,
) -> Vec<Attachment> {
    let mut out = Vec::with_capacity(files.len());
    for f in &files {
        let p = plan(f, max_text);
        let fetched = match p {
            Plan::Text => Some(fetch(&http, &f.url, max_text).await),
            Plan::Image => Some(fetch(&http, &f.url, theseus_protocol::MAX_IMAGE_BYTES).await),
            Plan::Skip(_) => None,
        };
        out.push(entry(f, &p, fetched));
    }
    out
}

/// What a message's files become when their task never reported back.
pub fn lost(files: &[FileMeta], why: &str) -> Vec<Attachment> {
    files
        .iter()
        .map(|f| Attachment {
            name: f.name.clone(),
            media_type: f.content_type.clone().unwrap_or_default(),
            size: f.size,
            not_read: Some(format!("the download failed ({why})")),
            ..Default::default()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(name: &str, content_type: Option<&str>, size: u64) -> FileMeta {
        FileMeta {
            name: name.into(),
            content_type: content_type.map(str::to_string),
            size,
            url: format!("https://cdn.discordapp.com/attachments/1/2/{name}"),
        }
    }

    const MAX: u64 = 262_144;

    #[test]
    fn a_long_paste_is_read_and_passed_as_text() {
        let paste = "The fact is 42.\n".repeat(320);
        let f = meta(
            "message.txt",
            Some("text/plain; charset=utf-8"),
            paste.len() as u64,
        );
        let p = plan(&f, MAX);
        assert_eq!(p, Plan::Text);
        let a = entry(&f, &p, Some(Ok(paste.clone().into_bytes())));
        assert_eq!(a.name, "message.txt");
        assert_eq!(a.media_type, "text/plain; charset=utf-8");
        assert_eq!(a.size, 5_120);
        assert_eq!(a.text.as_deref(), Some(paste.as_str()));
        assert!(a.not_read.is_none() && a.data.is_none());
    }

    #[test]
    fn text_is_known_by_type_or_extension_and_everything_else_is_listed() {
        assert_eq!(plan(&meta("notes.yaml", None, 10), MAX), Plan::Text);
        assert_eq!(
            plan(&meta("main.rs", Some("application/octet-stream"), 10), MAX),
            Plan::Text
        );
        assert_eq!(
            plan(&meta("x.json", Some("application/json"), 10), MAX),
            Plan::Text
        );
        let big = meta("dump.log", Some("text/plain"), MAX + 1);
        let p = plan(&big, MAX);
        assert_eq!(
            p,
            Plan::Skip("over the limit for text (262,144 bytes)".into())
        );
        let a = entry(&big, &p, None);
        assert_eq!(
            a.not_read.as_deref(),
            Some("over the limit for text (262,144 bytes)")
        );
        assert!(a.text.is_none());
        let zip = meta("src.zip", Some("application/zip"), 20 * 1024 * 1024);
        let a = entry(&zip, &plan(&zip, MAX), None);
        assert_eq!(
            a.not_read.as_deref(),
            Some("only text files and images are read")
        );
        assert_eq!(
            (a.size, a.media_type.as_str()),
            (20_971_520, "application/zip")
        );
    }

    #[test]
    fn a_failed_download_is_listed_with_the_failure() {
        let f = meta("message.txt", Some("text/plain"), 5_000);
        let a = entry(&f, &Plan::Text, Some(Err("HTTP 404".into())));
        assert_eq!(
            a.not_read.as_deref(),
            Some("the download failed (HTTP 404)")
        );
        assert!(a.text.is_none());
        let lost = lost(&[f], "the task stopped");
        assert_eq!(
            lost[0].not_read.as_deref(),
            Some("the download failed (the task stopped)")
        );
        // Bytes that are not UTF-8 still arrive, as text with replacements.
        let odd = entry(
            &meta("a.txt", None, 3),
            &Plan::Text,
            Some(Ok(vec![b'o', 0xff, b'k'])),
        );
        assert_eq!(odd.text.as_deref(), Some("o\u{fffd}k"));
    }

    #[test]
    fn an_image_is_downloaded_and_passed_as_its_bytes() {
        let shot = meta("image.png", Some("image/png"), 4);
        assert_eq!(plan(&shot, MAX), Plan::Image);
        let a = entry(&shot, &Plan::Image, Some(Ok(b"\x89PNG".to_vec())));
        assert_eq!(a.data.as_deref(), Some("iVBORw=="));
        assert!(a.text.is_none() && a.not_read.is_none());
        assert_eq!(plan(&meta("photo.JPG", None, 10), MAX), Plan::Image);
        let big = meta("huge.png", Some("image/png"), 6 * 1024 * 1024);
        assert_eq!(
            plan(&big, MAX),
            Plan::Skip("an image over the 5 MiB limit".into())
        );
        let heic = meta("x.heic", Some("image/heic"), 10);
        assert_eq!(
            plan(&heic, MAX),
            Plan::Skip("only PNG, JPEG, GIF, and WebP images are read".into())
        );
        let failed = entry(&shot, &Plan::Image, Some(Err("timed out".into())));
        assert_eq!(
            failed.not_read.as_deref(),
            Some("the download failed (timed out)")
        );
    }
}
