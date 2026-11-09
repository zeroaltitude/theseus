//! Discord attachments (theseus-9g2). Discord turns a long paste into a
//! `message.txt` attachment, so a binding that only listed attachments
//! silently missed every long paste. Text files are downloaded as text, and
//! every other file as its bytes, up to `[tools] max_attachment_bytes`
//! (theseus-c9l6): the core keeps an image as an image and any other file
//! whole, and reads a PDF for the model. A file over its cap is listed.
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

/// A whole download, connection included: a 32 MiB file at 1 MB/s.
const FETCH_TIMEOUT: Duration = Duration::from_secs(60);
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

/// What a message's files may be (theseus-c9l6): a text file's bytes
/// (`[tools] max_read_bytes`), and any other file's
/// (`[tools] max_attachment_bytes`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Caps {
    pub max_text: u64,
    pub max_file: u64,
}

/// What to do with one attachment.
#[derive(Debug, Clone, PartialEq)]
pub enum Plan {
    /// Download it and pass its text.
    Text,
    /// Download it and pass its bytes; the core checks them and keeps them:
    /// an image as an image, any other file whole, a PDF read.
    Bytes,
    /// List it without reading it, for this reason.
    Skip(String),
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

/// A text file the core reads as a document (theseus-c9l6): a Jupyter
/// notebook or RTF, by its name or its type.
fn read_as_document(name: &str, t: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let ext = lower.rsplit_once('.').map_or("", |(_, e)| e);
    matches!(ext, "ipynb" | "rtf")
        || matches!(
            t,
            "application/x-ipynb+json" | "application/rtf" | "text/rtf"
        )
}

fn text_extension(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let ext = lower.rsplit_once('.').map_or(lower.as_str(), |(_, e)| e);
    TEXT_EXTENSIONS.contains(&ext)
}

/// Decide, from the metadata alone, whether to download a file. Its bytes
/// say what it is; the core reads them.
pub fn plan(f: &FileMeta, caps: Caps) -> Plan {
    let t = f.media_type();
    // A notebook or an RTF file is text, but the core reads it as a
    // document, whole: it comes as its bytes, up to the cap for files.
    if read_as_document(&f.name, &t) {
        if f.size > caps.max_file {
            return Plan::Skip(format!(
                "over the {} limit for files ([tools] max_attachment_bytes)",
                theseus_core::attach::size_words(caps.max_file)
            ));
        }
        return Plan::Bytes;
    }
    if text_media_type(&t) || text_extension(&f.name) {
        if f.size > caps.max_text {
            return Plan::Skip(format!(
                "over the limit for text ({})",
                theseus_core::attach::size_words(caps.max_text)
            ));
        }
        return Plan::Text;
    }
    if f.size > caps.max_file {
        return Plan::Skip(format!(
            "over the {} limit for files ([tools] max_attachment_bytes)",
            theseus_core::attach::size_words(caps.max_file)
        ));
    }
    Plan::Bytes
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
        (Plan::Bytes, Some(Ok(bytes))) => a.data = Some(theseus_core::blobs::encode(&bytes)),
        (Plan::Text | Plan::Bytes, Some(Err(e))) => {
            a.not_read = Some(format!("the download failed ({e})"))
        }
        (Plan::Text | Plan::Bytes, None) => a.not_read = Some("the download did not run".into()),
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
///
/// An `Err` is its text and whether it was a timeout, which is not asked again.
async fn fetch(http: &reqwest::Client, url: &str, cap: u64) -> Result<Vec<u8>, (String, bool)> {
    let failed = |e: reqwest::Error| (e.to_string(), e.is_timeout());
    let mut resp = http.get(url).send().await.map_err(failed)?;
    if !resp.status().is_success() {
        return Err((format!("HTTP {}", resp.status().as_u16()), false));
    }
    let mut out = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(failed)? {
        out.extend_from_slice(&chunk);
        if out.len() as u64 > cap {
            out.truncate(cap as usize + 1);
            break;
        }
    }
    Ok(out)
}

/// How many times one file is asked for, and the waits between: a CDN link
/// that was only just made, or a hiccup, answers the second time (theseus-uck8).
const ATTEMPTS: usize = 3;
const RETRY_WAIT: [Duration; 2] = [Duration::from_millis(250), Duration::from_millis(1000)];

/// `fetch`, asked again after a status or connection failure up to `ATTEMPTS`
/// times; the last failure is the answer. A timeout is answered once: a link
/// that hangs for `FETCH_TIMEOUT` is slow, not "not yet made", and three asks
/// would hold the message's turn for three of them.
async fn fetch_retrying(http: &reqwest::Client, url: &str, cap: u64) -> Result<Vec<u8>, String> {
    let mut last = fetch(http, url, cap).await;
    for wait in RETRY_WAIT.iter().take(ATTEMPTS - 1) {
        if !matches!(last, Err((_, false))) {
            break;
        }
        tokio::time::sleep(*wait).await;
        last = fetch(http, url, cap).await;
    }
    last.map_err(|(text, _)| text)
}

/// Every file of one message, in order: downloaded when its plan says so,
/// listed with the reason when not. Never fails.
pub async fn fetch_all(http: reqwest::Client, files: Vec<FileMeta>, caps: Caps) -> Vec<Attachment> {
    let mut out = Vec::with_capacity(files.len());
    for f in &files {
        let p = plan(f, caps);
        let fetched = match p {
            Plan::Text => Some(fetch_retrying(&http, &f.url, caps.max_text).await),
            Plan::Bytes => Some(fetch_retrying(&http, &f.url, caps.max_file).await),
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

    const MAX: Caps = Caps {
        max_text: 262_144,
        max_file: 32 * 1024 * 1024,
    };

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
        let big = meta("dump.log", Some("text/plain"), MAX.max_text + 1);
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
        // Any other file is downloaded whole up to its cap (theseus-c9l6),
        // and listed past it.
        let zip = meta("src.zip", Some("application/zip"), 20 * 1024 * 1024);
        assert_eq!(plan(&zip, MAX), Plan::Bytes);
        let huge = meta("dump.zip", Some("application/zip"), 40 * 1024 * 1024);
        let a = entry(&huge, &plan(&huge, MAX), None);
        assert_eq!(
            a.not_read.as_deref(),
            Some("over the 32.0 MB limit for files ([tools] max_attachment_bytes)")
        );
        assert_eq!(
            (a.size, a.media_type.as_str()),
            (41_943_040, "application/zip")
        );
    }

    /// A notebook and an RTF file are text, but the core reads them as
    /// documents, so they come as their bytes, past the text cap.
    #[test]
    fn a_notebook_and_an_rtf_file_come_as_their_bytes() {
        let nb = meta("survey.ipynb", Some("application/json"), 300_000);
        assert_eq!(plan(&nb, MAX), Plan::Bytes);
        assert_eq!(
            plan(&meta("letter.rtf", Some("text/rtf"), 10), MAX),
            Plan::Bytes
        );
        assert_eq!(
            plan(&meta("data.json", Some("application/json"), 10), MAX),
            Plan::Text
        );
    }

    /// The owner's case (theseus-c9l6): a PDF in a DM is downloaded and passed as
    /// its bytes, for the core to read.
    #[test]
    fn a_pdf_is_downloaded_and_passed_as_its_bytes() {
        let f = meta("charter.pdf", Some("application/pdf"), 1_200_000);
        assert_eq!(plan(&f, MAX), Plan::Bytes);
        let a = entry(&f, &Plan::Bytes, Some(Ok(b"%PDF-1.7".to_vec())));
        assert_eq!(a.data.as_deref(), Some("JVBERi0xLjc="));
        assert!(a.text.is_none() && a.not_read.is_none());
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
        assert_eq!(plan(&shot, MAX), Plan::Bytes);
        let a = entry(&shot, &Plan::Bytes, Some(Ok(b"\x89PNG".to_vec())));
        assert_eq!(a.data.as_deref(), Some("iVBORw=="));
        assert!(a.text.is_none() && a.not_read.is_none());
        assert_eq!(plan(&meta("photo.JPG", None, 10), MAX), Plan::Bytes);
        // An image the models would refuse is kept whole too: the core says
        // why it is not shown (theseus-c9l6).
        let big = meta("huge.png", Some("image/png"), 6 * 1024 * 1024);
        assert_eq!(plan(&big, MAX), Plan::Bytes);
        let failed = entry(&shot, &Plan::Bytes, Some(Err("timed out".into())));
        assert_eq!(
            failed.not_read.as_deref(),
            Some("the download failed (timed out)")
        );
    }

    /// A one-file CDN on a local port: each connection is answered from
    /// `answers` in turn (the last repeats); returns the url and the count of
    /// requests served.
    fn cdn(
        answers: Vec<(u16, Vec<u8>)>,
    ) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use std::io::{Read, Write};
        use std::sync::atomic::{AtomicUsize, Ordering};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://{}/attachments/1/2/orders.pdf",
            listener.local_addr().unwrap()
        );
        let served = std::sync::Arc::new(AtomicUsize::new(0));
        let count = served.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf);
                let n = count.fetch_add(1, Ordering::SeqCst);
                let (code, body) = &answers[n.min(answers.len() - 1)];
                let head = format!(
                    "HTTP/1.1 {code} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(body);
            }
        });
        (url, served)
    }

    /// Ends `silent_cdn`'s thread when dropped: it sets the flag and connects
    /// once, so the blocked `accept` returns, the thread ends and the held
    /// connections close.
    struct Stop(std::sync::Arc<std::sync::atomic::AtomicBool>, std::net::SocketAddr);

    impl Drop for Stop {
        fn drop(&mut self) {
            self.0.store(true, std::sync::atomic::Ordering::SeqCst);
            let _ = std::net::TcpStream::connect(self.1);
        }
    }

    /// A CDN that accepts and never answers: counts each request and holds its
    /// connection open until the returned `Stop` is dropped.
    fn silent_cdn() -> (
        String,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
        Stop,
    ) {
        use std::io::Read;
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let url = format!("http://{addr}/attachments/1/2/orders.pdf");
        let served = std::sync::Arc::new(AtomicUsize::new(0));
        let count = served.clone();
        let done = std::sync::Arc::new(AtomicBool::new(false));
        let stop = done.clone();
        std::thread::spawn(move || {
            let mut held = Vec::new();
            for stream in listener.incoming() {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                let mut stream = stream.unwrap();
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf);
                count.fetch_add(1, Ordering::SeqCst);
                held.push(stream);
            }
        });
        (url, served, Stop(done, addr))
    }

    /// A timed-out download is asked once, not `ATTEMPTS` times: a link that
    /// hangs is slow, and each ask would cost `FETCH_TIMEOUT`.
    #[tokio::test]
    async fn a_download_that_times_out_is_not_asked_again() {
        // Real time by construction: the server never answers, so the client's
        // own 300 ms timeout is what ends the ask.
        let (url, served, _stop) = silent_cdn();
        let http = reqwest::Client::builder()
            .timeout(Duration::from_millis(300))
            .build()
            .unwrap();
        let f = FileMeta {
            name: "orders.pdf".into(),
            content_type: Some("application/pdf".into()),
            size: 1_200,
            url,
        };
        let got = fetch_all(http, vec![f], MAX).await;
        assert!(got[0].not_read.is_some(), "{:?}", got[0]);
        assert!(got[0].data.is_none());
        // The server thread counts after the client has timed out, so wait (at
        // most 5 s) for the one ask to be counted rather than read it at once.
        for _ in 0..500 {
            if served.load(std::sync::atomic::Ordering::SeqCst) >= 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let asked = served.load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(asked, 1, "asked once");
        assert!(asked < ATTEMPTS, "not asked {ATTEMPTS} times");
    }

    /// theseus-uck8: the first ask of a CDN link fails (it was only just
    /// made), the second answers. The model must get the file whole, not a
    /// name and a size with "the download failed".
    #[tokio::test]
    async fn a_pdf_whose_first_download_fails_still_arrives_whole() {
        let pdf = b"%PDF-1.7 the harbour master is Odile Varnack".repeat(100);
        let (url, served) = cdn(vec![(404, b"not yet".to_vec()), (200, pdf.clone())]);
        let f = FileMeta {
            name: "orders.pdf".into(),
            content_type: Some("application/pdf".into()),
            size: pdf.len() as u64,
            url,
        };
        let got = fetch_all(client(), vec![f], MAX).await;
        assert_eq!(got.len(), 1);
        assert!(got[0].not_read.is_none(), "{:?}", got[0].not_read);
        let data = got[0].data.as_deref().expect("its bytes");
        assert_eq!(theseus_core::blobs::decode(data).unwrap().len(), pdf.len());
        assert_eq!(served.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    /// A download that never succeeds is still listed with its failure, after
    /// `ATTEMPTS` asks, and the message is not lost.
    #[tokio::test]
    async fn a_download_that_keeps_failing_is_listed_with_the_failure() {
        let (url, served) = cdn(vec![(404, b"gone".to_vec())]);
        let f = FileMeta {
            name: "orders.pdf".into(),
            content_type: Some("application/pdf".into()),
            size: 1_200,
            url,
        };
        let got = fetch_all(client(), vec![f], MAX).await;
        assert_eq!(
            got[0].not_read.as_deref(),
            Some("the download failed (HTTP 404)")
        );
        assert!(got[0].data.is_none());
        assert_eq!(served.load(std::sync::atomic::Ordering::SeqCst), ATTEMPTS);
    }
}
