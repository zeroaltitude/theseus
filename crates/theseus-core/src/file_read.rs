//! `file.read` (theseus-c9l6): a file someone gave this session, read on
//! request, by its name. A message's files are kept whole in the store's
//! blobs (`attach`), and the model reads most of them in its context as they
//! arrive; this is how it reads the rest:
//! - a PDF's other pages (`pages`), as `fs.read` reads a PDF by path;
//! - a document's sections (`pages`): a sheet, a slide, a chapter, a cell;
//! - an archive's member (`member`), saved under the tools' working
//!   directory, in `.theseus-files/<session>/`, and read there;
//! - a recording's transcript, through Deepgram, as voice's speech to text
//!   is: spend, so it runs only when the model asks, is capped at
//!   `[tools] transcribe_max_minutes`, and is booked to the session's
//!   execution like a voice call (`speech.transcribed`); a transcript is kept
//!   by the file's digest, so a file is heard once;
//! - a video's transcript and a strip of its frames (ffmpeg), when ffmpeg is
//!   on the machine;
//! - the text in a picture (tesseract), for a model without vision, when
//!   tesseract is on the machine;
//! - any file, saved (`save`) where `proc.run` can use it.
//!
//! The runtime runs it (`ToolRuntime`'s async calls), since it reads the
//! session: the file is found on the turn's task, and the reading runs in the
//! call's own task, its blocking parts on blocking threads. In a shared
//! place, what it reads is outside text (§3.9's hold), as a fetch's is.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_files::kind::Kind;
use theseus_tools::{
    parse, Access, AsyncMediaRun, Backend, External, ImageData, Media, Plan, Resource, Retry, Tool,
    ToolClass, ToolCtx, ToolFailure, ToolOutput,
};

use crate::blobs::Blobs;
use crate::node::{Attachment, AttachmentContent, Body};

pub const READ: &str = "file.read";
pub const FAMILY: &str = "file";

/// The directory under the tools' working directory where a session's files
/// are saved and an archive's members read out.
pub const WORK_DIR: &str = ".theseus-files";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    name: String,
    #[serde(default)]
    pages: Option<String>,
    #[serde(default)]
    member: Option<String>,
    #[serde(default)]
    save: bool,
}

/// The registry's `file.read`: its name, schema, and plan, for the gate. The
/// runtime runs it ([`Reader::run`]).
pub struct FileRead;

impl Tool for FileRead {
    fn name(&self) -> &'static str {
        READ
    }
    fn description(&self) -> &'static str {
        "Read a file someone gave this session (a message's attachment, or a file a tool read), by its name as its line shows it. A PDF: its pages (pages, \"3-5\", up to 20 a read), natively for a model that reads PDFs. A document (Word, Excel, PowerPoint, OpenDocument, EPUB, RTF, a Jupyter notebook): its sections (pages counts sections: sheets, slides, chapters, cells). An archive (zip, tar, tar.gz): its list, or one member (member), saved under the working directory's .theseus-files/ and read. A recording (mp3, wav, m4a, ogg, webm): its transcript, by Deepgram, which costs money (about $0.26 an hour of audio), so read one only when it is wanted. A video: its transcript and a strip of its frames. A picture: the text in it, by OCR, for a model without vision. Any file: save to put it where proc_run can use it."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "description": "The file's name, as its line shows it (or the start of its digest)."},
                "pages": {"type": "string", "description": "A PDF's pages, or a document's sections, counted from 1: \"3\", \"3-5\", or \"21-\"."},
                "member": {"type": "string", "description": "An archive's member to read out, by its path in the archive's list."},
                "save": {"type": "boolean", "description": "Save the file under .theseus-files/ in the working directory, for proc_run."}
            },
            "required": ["name"],
            "additionalProperties": false
        })
    }
    fn class(&self) -> ToolClass {
        ToolClass::Read
    }
    fn backend(&self) -> Backend {
        Backend::Async
    }
    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }
    fn plan(&self, input: &Value, ctx: &ToolCtx) -> Result<Plan, String> {
        let a: Args = parse(input)?;
        if a.name.trim().is_empty() {
            return Err("name is empty: give the file's name as its line shows it".into());
        }
        // Saving writes under the working directory: the gate judges that.
        let resources = match a.save || a.member.is_some() {
            true => vec![Resource {
                path: ctx.cwd.join(WORK_DIR),
                access: Access::Write,
            }],
            false => vec![],
        };
        Ok(Plan {
            summary: format!("read the file {}", a.name.trim()),
            resources,
            ..Default::default()
        })
    }
    fn run_async(&self, _input: &Value, _ctx: &ToolCtx) -> theseus_tools::AsyncRun {
        Box::pin(std::future::ready(Err(ToolFailure::new(
            "file.read runs through the runtime, which finds the session's files",
        ))))
    }
}

/// Deepgram, as `[voice]` names it, for a recording's transcript.
#[derive(Clone)]
pub struct Hearing {
    pub key_secret: String,
    pub model: String,
    pub language: String,
    pub api_base: String,
    /// `[tools] transcribe_max_minutes`: the most of one recording heard.
    pub max_minutes: u64,
}

/// What `file.read` needs that the call's context does not hold.
pub struct Reader {
    pub secrets: Arc<crate::secrets::SecretBoard>,
    pub hearing: Hearing,
    /// `[tools] max_attachment_bytes`: the most an archive's member is read out to.
    pub max_file: u64,
    pub http: reqwest::Client,
}

/// A file of the session, found by its name.
#[derive(Debug, Clone)]
struct Found {
    file: Attachment,
    digest: String,
    /// The text blob a kept file names.
    text: Option<String>,
}

/// The session's files, newest first: each message's attachments, then each
/// file a tool read.
fn files_of(nodes: &[(u64, crate::node::Node)]) -> Vec<Attachment> {
    let mut out = Vec::new();
    for (_, n) in nodes.iter().rev() {
        match &n.body {
            Body::UserMessage { attachments, .. } => out.extend(attachments.iter().cloned()),
            Body::ToolResult { image: Some(a), .. } => out.push(a.clone()),
            _ => {}
        }
    }
    out
}

/// The file `name` names: its exact name, else its name in any case, else
/// the start of its digest (six characters at least); the newest when
/// several share it.
fn find(files: &[Attachment], name: &str) -> Result<Found, String> {
    let name = name.trim();
    let digest_of = |a: &Attachment| match &a.content {
        AttachmentContent::File { digest, .. } | AttachmentContent::Image { digest, .. } => {
            Some(digest.clone())
        }
        _ => None,
    };
    let kept: Vec<&Attachment> = files.iter().filter(|a| digest_of(a).is_some()).collect();
    let hit = kept
        .iter()
        .find(|a| a.name == name)
        .or_else(|| kept.iter().find(|a| a.name.eq_ignore_ascii_case(name)))
        .or_else(|| {
            (name.len() >= 6)
                .then(|| {
                    kept.iter()
                        .find(|a| digest_of(a).is_some_and(|d| d.starts_with(name)))
                })
                .flatten()
        });
    match hit {
        Some(a) => Ok(Found {
            file: (*a).clone(),
            digest: digest_of(a).unwrap_or_default(),
            text: match &a.content {
                AttachmentContent::File { text, .. } => text.clone(),
                _ => None,
            },
        }),
        None if kept.is_empty() => Err(format!(
            "this session has no file named {name}: no file has been given to it yet"
        )),
        None => {
            let mut names: Vec<&str> = kept.iter().map(|a| a.name.as_str()).collect();
            names.dedup();
            names.truncate(20);
            Err(format!(
                "this session has no file named {name}; its files: {}",
                names.join(", ")
            ))
        }
    }
}

/// Where this session's files are saved: `<cwd>/.theseus-files/<session's
/// last 8 characters>`.
pub fn work_dir(cwd: &Path, session_id: &str) -> PathBuf {
    let short: String = session_id
        .chars()
        .rev()
        .take(8)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    cwd.join(WORK_DIR).join(short)
}

impl Reader {
    /// A reader for a runtime with no tools: it hears nothing.
    pub fn disabled() -> Self {
        Self {
            secrets: crate::secrets::SecretBoard::empty(),
            hearing: Hearing {
                key_secret: String::new(),
                model: String::new(),
                language: String::new(),
                api_base: String::new(),
                max_minutes: 0,
            },
            max_file: 0,
            http: reqwest::Client::new(),
        }
    }

    /// Find the file on the turn's task, then read it in the call's own
    /// future. `shared`: the session's place is shared, so what it reads is
    /// outside text. `available`: what the session may still spend, in
    /// micro-dollars, which a transcript must fit.
    pub fn run(
        &self,
        nodes: &[(u64, crate::node::Node)],
        blobs: Arc<Blobs>,
        session_id: &str,
        ctx: &ToolCtx,
        input: &Value,
        (shared, available): (bool, u64),
    ) -> AsyncMediaRun {
        let a: Args = match parse(input) {
            Ok(a) => a,
            Err(e) => return Box::pin(std::future::ready(Err(ToolFailure::new(e)))),
        };
        let found = match find(&files_of(nodes), &a.name) {
            Ok(f) => f,
            Err(e) => return Box::pin(std::future::ready(Err(ToolFailure::new(e)))),
        };
        let job = Job {
            found,
            args: a,
            blobs,
            dir: work_dir(&ctx.cwd, session_id),
            max_read: ctx.max_read_bytes,
            max_file: self.max_file,
            hearing: self.hearing.clone(),
            key: self
                .secrets
                .get(&self.hearing.key_secret)
                .map(|s| s.expose().to_string()),
            http: self.http.clone(),
            available,
        };
        Box::pin(async move {
            let (out, media) = job.read().await?;
            let external = shared.then(|| External {
                url: format!("attachment:{}", out.meta["file"].as_str().unwrap_or("")),
            });
            Ok((out, external, media))
        })
    }
}

/// One `file.read`'s work, owned by its future.
struct Job {
    found: Found,
    args: Args,
    blobs: Arc<Blobs>,
    dir: PathBuf,
    max_read: usize,
    max_file: u64,
    hearing: Hearing,
    key: Option<String>,
    http: reqwest::Client,
    available: u64,
}

type Read = (ToolOutput, Option<Media>);

/// A blocking step on a blocking thread.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, ToolFailure> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| ToolFailure::new(format!("the reading stopped: {e}")))
}

impl Job {
    fn name(&self) -> &str {
        &self.found.file.name
    }

    fn meta(&self, more: Value) -> Value {
        let mut m = json!({"file": self.name(), "digest": self.found.digest,
                           "media_type": self.found.file.media_type, "bytes": self.found.file.size});
        if let (Value::Object(m), Value::Object(more)) = (&mut m, more) {
            m.extend(more);
        }
        m
    }

    fn bytes(&self) -> Result<Vec<u8>, ToolFailure> {
        self.blobs
            .read(&self.found.digest)
            .ok_or_else(|| ToolFailure::new(format!("{}'s stored bytes are missing", self.name())))
    }

    async fn read(self) -> Result<Read, ToolFailure> {
        if self.args.save {
            return self.save().await;
        }
        let kind = match &self.found.file.content {
            AttachmentContent::Image { .. } => return self.ocr().await,
            _ => Kind::of_media_type(&self.found.file.media_type),
        };
        match kind {
            Kind::Pdf => self.pdf().await,
            Kind::Zip | Kind::Tar | Kind::TarGz if self.args.member.is_some() => {
                self.member(kind).await
            }
            k if k.has_text() => self.sections(),
            Kind::Audio => self.hear(false).await,
            Kind::Video => self.hear(true).await,
            _ => self.save().await,
        }
    }

    async fn pdf(self) -> Result<Read, ToolFailure> {
        let bytes = self.bytes()?;
        let name = self.name().to_string();
        let pages = self.args.pages.clone();
        let (mut out, media) = blocking(move || {
            theseus_tools::docs::pdf_pages(
                &name,
                name.clone(),
                bytes,
                pages.as_deref(),
                "file_read",
            )
        })
        .await??;
        out.meta = self.meta(out.meta.clone());
        Ok((out, media))
    }

    /// A document's sections from its text blob, paged by `pages`, cut at
    /// `[tools] max_read_bytes` with words that say how to read on.
    fn sections(self) -> Result<Read, ToolFailure> {
        let texts = self
            .found
            .text
            .as_deref()
            .and_then(|d| self.blobs.texts(d))
            .ok_or_else(|| {
                ToolFailure::new(format!("{}'s text was not read when it came", self.name()))
            })?;
        let total = texts.len() as u32;
        let range = match &self.args.pages {
            Some(p) => theseus_files::pdf::Pages::parse(p).map_err(ToolFailure::new)?,
            None => theseus_files::pdf::Pages::new(1, total.max(1)),
        };
        if range.first > total.max(1) {
            return Err(ToolFailure::new(format!(
                "{} has {total} sections, so section {} is past its end",
                self.name(),
                range.first
            )));
        }
        let mut text = format!(
            "{} ({}), {total} sections",
            self.name(),
            self.found.file.media_type
        );
        let mut shown = range.first - 1;
        for s in texts
            .iter()
            .skip(range.first as usize - 1)
            .take(range.count() as usize)
        {
            let piece = format!("\n--- {} ---\n{}", s.label, s.text);
            if text.len() + piece.len() > self.max_read && shown >= range.first {
                break;
            }
            let (kept, _) = crate::attach::cut_to(&piece, self.max_read.saturating_sub(text.len()));
            text.push_str(kept);
            shown += 1;
            if !s.images.is_empty() {
                text.push_str(&format!(
                    "\n[{} images in its outputs, shown with the message it came in]",
                    s.images.len()
                ));
            }
        }
        let last = range.last.min(total);
        if shown < last {
            text.push_str(&format!(
                "\n[shown: sections {}–{shown}; file_read with pages=\"{}-{last}\" reads on]",
                range.first,
                shown + 1
            ));
        }
        let meta = self.meta(json!({"sections": total, "from": range.first, "to": shown}));
        Ok((ToolOutput { text, meta }, None))
    }

    /// Save the file's bytes under the session's work dir, by its name.
    async fn save(self) -> Result<Read, ToolFailure> {
        let bytes = self.bytes()?;
        let file = theseus_files::archive::safe_path(&self.found.file.name)
            .and_then(|p| p.file_name().map(PathBuf::from))
            .unwrap_or_else(|| {
                PathBuf::from(&self.found.digest[..12.min(self.found.digest.len())])
            });
        let path = self.dir.join(file);
        let n = bytes.len();
        let p = path.clone();
        blocking(move || {
            theseus_tools::fs::write_atomic(&p, &bytes, theseus_kernel::umask::operator())
        })
        .await?
        .map_err(|e| ToolFailure::new(format!("{} could not be saved: {e}", path.display())))?;
        let text = format!(
            "Saved {} at {} ({n} bytes): proc_run and the fs tools can use it there.",
            self.name(),
            path.display()
        );
        Ok((
            ToolOutput {
                text,
                meta: self.meta(json!({"saved": path})),
            },
            None,
        ))
    }

    /// An archive's member, read out under the work dir and read there.
    async fn member(self, kind: Kind) -> Result<Read, ToolFailure> {
        let member = self.args.member.clone().unwrap_or_default();
        let rel = theseus_files::archive::safe_path(&member).ok_or_else(|| {
            ToolFailure::new(format!(
                "`{member}` is not a path inside the archive that can be read out"
            ))
        })?;
        let bytes = self.bytes()?;
        let max = self.max_file;
        let m = member.clone();
        let (got, _) =
            blocking(move || theseus_files::convert::extract(&bytes, kind, &m, max)).await?;
        let data =
            got.map_err(|why| ToolFailure::new(format!("{member} was not read out: {why}")))?;
        let path = self
            .dir
            .join(self.found.file.name.replace('/', "_") + ".d")
            .join(rel);
        let (p, d) = (path.clone(), data.clone());
        blocking(move || {
            theseus_tools::fs::write_atomic(&p, &d, theseus_kernel::umask::operator())
        })
        .await?
        .map_err(|e| ToolFailure::new(format!("{} could not be saved: {e}", path.display())))?;
        let mut text = format!(
            "{member} from {}, {} bytes, saved at {}.",
            self.name(),
            data.len(),
            path.display()
        );
        let mut media = None;
        if theseus_files::pdf::is_pdf(&data) {
            let name = member.clone();
            let (out, m) = blocking(move || {
                theseus_tools::docs::pdf_pages("It", name.clone(), data, None, "file_read")
            })
            .await??;
            text.push('\n');
            text.push_str(&out.text);
            media = m;
        } else if let Some(info) = theseus_tools::image::sniff(&data) {
            if theseus_tools::image::refusal(data.len() as u64, &info).is_none() {
                media = Some(Media::Image(ImageData {
                    name: member.clone(),
                    info,
                    bytes: data,
                }));
            }
        } else if !data.iter().take(8192).any(|b| *b == 0) {
            let s = String::from_utf8_lossy(&data);
            let (kept, cut) = crate::attach::cut_to(&s, self.max_read);
            text.push('\n');
            text.push_str(kept);
            if cut {
                text.push_str(&format!(
                    "\n[cut at {} bytes: fs_read the saved file to read on]",
                    kept.len()
                ));
            }
        }
        Ok((
            ToolOutput {
                text,
                meta: self.meta(json!({"member": member, "saved": path})),
            },
            media,
        ))
    }

    /// The text in a picture, by OCR.
    async fn ocr(self) -> Result<Read, ToolFailure> {
        let bytes = self.bytes()?;
        let got = blocking(move || theseus_files::media::ocr(&bytes)).await?;
        let text = match got {
            Ok(t) if t.trim().is_empty() => format!("{}: OCR found no text in it.", self.name()),
            Ok(t) => format!(
                "The text in {}, by OCR (tesseract; layout and pictures are not in it):\n{t}",
                self.name()
            ),
            Err(why) => format!("{}'s text could not be read by OCR: {why}.", self.name()),
        };
        Ok((
            ToolOutput {
                text,
                meta: self.meta(json!({"ocr": true})),
            },
            None,
        ))
    }

    /// A strip of a video's frames (ffmpeg), as the image its result shows;
    /// `None` when ffmpeg could not make one.
    async fn frames(
        &self,
        path: PathBuf,
        seconds: Option<f64>,
    ) -> Result<Option<Media>, ToolFailure> {
        let s = seconds.unwrap_or(30.0);
        let png = blocking(move || theseus_files::media::frames(&path, 4, s))
            .await?
            .ok();
        Ok(png.and_then(|png| {
            let info = theseus_tools::image::sniff(&png)?;
            Some(Media::Image(ImageData {
                name: format!("{} (frames)", self.name()),
                info,
                bytes: png,
            }))
        }))
    }

    /// A recording's transcript (and, for a video, a strip of its frames),
    /// kept by the file's digest so it is heard once.
    async fn hear(self, video: bool) -> Result<Read, ToolFailure> {
        let path = self.blobs.path(&self.found.digest);
        let cached = self.blobs.derived(&self.found.digest, "transcript.json");
        let seconds = {
            let p = path.clone();
            blocking(move || theseus_files::media::duration(&p)).await?
        };
        let media = match video {
            true => self.frames(path.clone(), seconds).await?,
            false => None,
        };
        if let Some(t) = cached
            .as_deref()
            .and_then(|b| serde_json::from_slice::<Value>(b).ok())
        {
            let text = format!(
                "Transcript of {}{} (heard before; no new charge):\n{}",
                self.name(),
                length_words(t["seconds"].as_f64()),
                t["text"].as_str().unwrap_or("")
            );
            return Ok((
                ToolOutput {
                    text,
                    meta: self.meta(json!({"transcript": "cached"})),
                },
                media,
            ));
        }
        let Some(key) = self.key.clone() else {
            return Err(ToolFailure::new(format!(
                "{} is not transcribed: the secret {} (Deepgram's key) is not set",
                self.name(),
                self.hearing.key_secret
            )));
        };
        let cap = self.hearing.max_minutes as f64 * 60.0;
        // What Deepgram is sent: the file, or ffmpeg's audio of it, cut at
        // the cap when it is longer.
        let (audio, media_type, heard_for) = {
            let long = seconds.is_some_and(|s| s > cap);
            if video || long {
                let p = path.clone();
                let limit = long.then_some(cap);
                let wav = blocking(move || cut_audio(&p, limit))
                    .await?
                    .map_err(|why| {
                        ToolFailure::new(format!("{} is not transcribed: {why}", self.name()))
                    })?;
                (wav, "audio/wav".to_string(), seconds.map(|s| s.min(cap)))
            } else {
                (self.bytes()?, self.found.file.media_type.clone(), seconds)
            }
        };
        if let Some(price) = crate::catalog::speech_price("deepgram", &self.hearing.model) {
            let est = price.cost_micros(Duration::from_secs_f64(heard_for.unwrap_or(cap)), 0);
            if est > self.available {
                return Err(ToolFailure::new(format!(
                    "{} is not transcribed: its transcript would cost about {}, past what this session may still spend",
                    self.name(),
                    crate::narrative::dollars(est)
                )));
            }
        }
        let heard = listen(&self.http, &self.hearing, &key, audio, &media_type)
            .await
            .map_err(|why| {
                ToolFailure::new(format!("{} is not transcribed: {why}", self.name()))
            })?;
        let record =
            json!({"text": heard.text, "seconds": heard.seconds, "model": self.hearing.model});
        let _ = self.blobs.put_derived(
            &self.found.digest,
            "transcript.json",
            record.to_string().as_bytes(),
        );
        let cut = match seconds {
            Some(s) if s > cap => format!(
                " [only its first {} minutes were heard: [tools] transcribe_max_minutes]",
                self.hearing.max_minutes
            ),
            _ => String::new(),
        };
        let text = format!(
            "Transcript of {}{}, by Deepgram {}{cut}:\n{}",
            self.name(),
            length_words(Some(heard.seconds)),
            self.hearing.model,
            heard.text
        );
        let meta = self.meta(json!({"speech": {"provider": "deepgram", "model": self.hearing.model,
                                               "seconds": heard.seconds, "chars": heard.text.chars().count()}}));
        Ok((ToolOutput { text, meta }, media))
    }
}

/// ` (4:12)`, or nothing when the length is not known.
fn length_words(seconds: Option<f64>) -> String {
    match seconds {
        Some(s) => {
            let s = s.round() as u64;
            format!(" ({}:{:02})", s / 60, s % 60)
        }
        None => String::new(),
    }
}

/// A recording's audio as WAV, cut at `limit` seconds when given (ffmpeg).
fn cut_audio(path: &Path, limit: Option<f64>) -> Result<Vec<u8>, String> {
    match limit {
        None => theseus_files::media::audio_of(path),
        Some(l) => theseus_files::media::audio_of_first(path, l),
    }
}

/// What Deepgram heard.
pub struct Heard {
    pub text: String,
    pub seconds: f64,
}

/// One recording to Deepgram's pre-recorded endpoint, as its bytes with its
/// type, the way theseus-voice's `listen` sends an utterance (smart
/// formatting on, the key in the Authorization header alone, never quoted).
pub async fn listen(
    http: &reqwest::Client,
    h: &Hearing,
    key: &str,
    audio: Vec<u8>,
    media_type: &str,
) -> Result<Heard, String> {
    let url = format!("{}/v1/listen", h.api_base.trim_end_matches('/'));
    let resp = http
        .post(url)
        .query(&[
            ("model", h.model.as_str()),
            ("language", h.language.as_str()),
            ("smart_format", "true"),
        ])
        .header(reqwest::header::AUTHORIZATION, format!("Token {key}"))
        .header(reqwest::header::CONTENT_TYPE, media_type)
        .body(audio)
        .send()
        .await
        .map_err(|e| format!("Deepgram could not be reached ({e})"))?;
    let status = resp.status();
    let body = resp
        .bytes()
        .await
        .map_err(|e| format!("Deepgram's answer could not be read ({e})"))?;
    let text = String::from_utf8_lossy(&body);
    if !status.is_success() {
        let said: String = text.replace(key, "<key>").chars().take(300).collect();
        return Err(format!("Deepgram answered {status}: {}", said.trim()));
    }
    let v: Value = serde_json::from_slice(&body)
        .map_err(|e| format!("Deepgram's answer could not be read ({e})"))?;
    let transcript = v["results"]["channels"][0]["alternatives"][0]["transcript"]
        .as_str()
        .ok_or("Deepgram's answer has no transcript")?
        .trim()
        .to_string();
    Ok(Heard {
        text: transcript,
        seconds: v["metadata"]["duration"].as_f64().unwrap_or(0.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, digest: &str) -> Attachment {
        Attachment {
            name: name.into(),
            media_type: "application/pdf".into(),
            size: 10,
            content: AttachmentContent::File {
                digest: digest.into(),
                pages: Some(1),
                text: None,
                parts: vec![],
                unread: None,
            },
        }
    }

    #[test]
    fn a_file_is_found_by_its_name_its_case_or_its_digest_newest_first() {
        let files = vec![
            file("Orders.pdf", "aaaaaa11"),
            file("orders.pdf", "bbbbbb22"),
            file("tides.pdf", "cccccc33"),
        ];
        assert_eq!(find(&files, "orders.pdf").unwrap().digest, "bbbbbb22");
        assert_eq!(find(&files, "ORDERS.PDF").unwrap().digest, "aaaaaa11");
        assert_eq!(find(&files, "cccccc").unwrap().digest, "cccccc33");
        assert_eq!(
            find(&files, "fees.pdf").unwrap_err(),
            "this session has no file named fees.pdf; its files: Orders.pdf, orders.pdf, tides.pdf"
        );
        assert!(find(&[], "x")
            .unwrap_err()
            .contains("no file has been given to it yet"));
        assert_eq!(
            work_dir(Path::new("/w"), "ses_0123456789abcdef"),
            PathBuf::from("/w/.theseus-files/89abcdef")
        );
    }
}
