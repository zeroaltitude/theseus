//! `drive.find`, `drive.read`, `drive.write` and `drive.share` (the Google
//! design §3.1, rows 8 to 11): the owner's files, found, read, written, and
//! shared only with a yes.
//!
//! - **find** compiles typed fields to Drive's `q` (`text` is `fullText
//!   contains`, every quoted value escaped), one row a file.
//! - **read** gives a Doc as markdown (Drive's own export), a Sheet's tabs and
//!   its first 200 rows (or `range`) as a table, Slides' text by slide with
//!   object ids and layouts, and a PDF or any other file to `save_to`
//!   (required past 1 MB).
//! - **write** creates, replaces or moves a file; markdown becomes a Doc and
//!   CSV a Sheet by Drive's own import conversion. Drive keeps revisions, so a
//!   write is undoable: notify.
//! - **share** changes who can see a file, or trashes it. It reaches other
//!   people or removes things, so its plan is **authority**: the gate asks at
//!   every posture, and no line, grant or trust loosens it.
//!
//! A result is outside text unless the file is the account's own and not
//! shared (`Source::File`); a listing is, unless every row is.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_google::{Body, Repeat, Request, Service};
use theseus_tools::{
    parse, Access, AsyncRun, Backend, Plan, Resource, Retry, Tool, ToolClass, ToolCtx, ToolFailure,
};

use super::external::{finish, Finished, Source};
use super::{CallFailed, Called, Google, Meta};

pub const FIND: &str = "drive.find";
pub const READ: &str = "drive.read";
pub const WRITE: &str = "drive.write";
pub const SHARE: &str = "drive.share";

pub const DOC: &str = "application/vnd.google-apps.document";
pub const SHEET: &str = "application/vnd.google-apps.spreadsheet";
pub const SLIDES: &str = "application/vnd.google-apps.presentation";
pub const FOLDER: &str = "application/vnd.google-apps.folder";
const PPTX: &str = "application/vnd.openxmlformats-officedocument.presentationml.presentation";

/// The most a file or an export is shown whole: past it, `save_to`.
const INLINE_MAX: u64 = 1 << 20;
/// The most `drive.write` sends (Drive's multipart upload).
const UPLOAD_MAX: u64 = 5 << 20;
/// A Sheet's rows shown when `range` names none.
const SHEET_ROWS: u32 = 200;
const LIST_MAX: u32 = 100;
const LIST_DEFAULT: u32 = 20;
/// What a row of metadata names.
const FIELDS: &str =
    "id,name,mimeType,size,owners(emailAddress,me),shared,webViewLink,modifiedTime";

pub fn tools(g: &Arc<Google>) -> Vec<Arc<dyn Tool>> {
    [Op::Find, Op::Read, Op::Write, Op::Share]
        .map(|op| Arc::new(DriveTool(g.clone(), op)) as Arc<dyn Tool>)
        .into()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Op {
    Find,
    Read,
    Write,
    Share,
}

pub struct DriveTool(Arc<Google>, Op);

// ------------------------------------------------------------------ inputs

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FindArgs {
    q: Option<String>,
    text: Option<String>,
    name: Option<String>,
    folder: Option<String>,
    r#type: Option<String>,
    #[serde(default)]
    recent: bool,
    max: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadArgs {
    id: String,
    range: Option<String>,
    save_to: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteArgs {
    id: Option<String>,
    name: Option<String>,
    folder: Option<String>,
    content: Option<String>,
    from_path: Option<String>,
    r#as: Option<String>,
    move_to: Option<String>,
    rename: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShareArgs {
    id: String,
    email: Option<String>,
    role: Option<String>,
    #[serde(default)]
    anyone_with_link: bool,
    #[serde(default)]
    remove: bool,
    #[serde(default)]
    notify: bool,
    trash: Option<bool>,
}

// ------------------------------------------------------------------ pieces

/// A Drive id: letters, digits, `_` and `-` (and `root`), so it can ride a
/// path unescaped and never name another one.
fn id_of(id: &str) -> Result<&str, String> {
    let ok = !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    ok.then_some(id)
        .ok_or_else(|| format!("{id:?} is not a Drive file id: letters, digits, - and _ only"))
}

/// A string as Drive's `q` quotes it: single quotes, with `\` and `'` escaped.
pub fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}

fn mime_of_type(t: &str) -> Result<&'static str, String> {
    Ok(match t {
        "doc" => DOC,
        "sheet" => SHEET,
        "slides" => SLIDES,
        "pdf" => "application/pdf",
        "folder" => FOLDER,
        _ => {
            return Err(format!(
                "type {t:?} is not doc, sheet, slides, pdf or folder"
            ))
        }
    })
}

/// A mime type as one short word.
fn type_word(mime: &str) -> &str {
    match mime {
        DOC => "doc",
        SHEET => "sheet",
        SLIDES => "slides",
        FOLDER => "folder",
        "application/pdf" => "pdf",
        m => m,
    }
}

/// `find`'s `q`: the raw `q` and each typed field, all of them and not trashed.
pub fn compile_q(
    q: Option<&str>,
    text: Option<&str>,
    name: Option<&str>,
    folder: Option<&str>,
    r#type: Option<&str>,
) -> Result<String, String> {
    let mut parts = Vec::new();
    if let Some(q) = q.filter(|q| !q.trim().is_empty()) {
        parts.push(format!("({q})"));
    }
    if let Some(t) = text {
        parts.push(format!("fullText contains {}", quote(t)));
    }
    if let Some(n) = name {
        parts.push(format!("name contains {}", quote(n)));
    }
    if let Some(f) = folder {
        parts.push(format!("{} in parents", quote(id_of(f)?)));
    }
    if let Some(t) = r#type {
        parts.push(format!("mimeType = {}", quote(mime_of_type(t)?)));
    }
    parts.push("trashed = false".into());
    Ok(parts.join(" and "))
}

/// What a file's metadata says, as the read and the External rule use it.
struct Info {
    id: String,
    name: String,
    mime: String,
    size: u64,
    /// Every owner is the account.
    own: bool,
    shared: bool,
    owner: String,
    link: String,
    modified: String,
}

impl Info {
    fn of(v: &Value) -> Info {
        let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
        let owners = v["owners"].as_array().cloned().unwrap_or_default();
        let owner = match owners.first() {
            Some(o) if o["me"].as_bool() == Some(true) => "you".to_string(),
            Some(o) => o["emailAddress"].as_str().unwrap_or("?").to_string(),
            None => "?".into(),
        };
        Info {
            id: s("id"),
            name: s("name"),
            mime: s("mimeType"),
            size: v["size"].as_str().and_then(|n| n.parse().ok()).unwrap_or(0),
            own: !owners.is_empty() && owners.iter().all(|o| o["me"].as_bool() == Some(true)),
            shared: v["shared"].as_bool().unwrap_or(false),
            owner,
            link: s("webViewLink"),
            modified: s("modifiedTime"),
        }
    }

    fn source(&self) -> Source<'_> {
        Source::File {
            id: &self.id,
            owned_by_account: self.own,
            shared: self.shared,
        }
    }

    fn line(&self) -> String {
        format!(
            "{} | {} | {} | {} | {} | {}shared | {}",
            self.id,
            self.name.replace(['\n', '|'], " "),
            type_word(&self.mime),
            self.owner,
            self.modified,
            if self.shared { "" } else { "not " },
            self.link
        )
    }
}

/// A call's failure as the tool's: the words, and the row in its meta.
fn failed(f: CallFailed) -> ToolFailure {
    ToolFailure {
        message: f.message,
        meta: json!({"private": true, "google_called": f.row, "outcome_unknown": f.unknown}),
    }
}

fn get(path: String, query: &[(&str, &str)]) -> Request {
    sent(Service::Drive, "GET", path, query, Body::None, Repeat::Safe)
}

fn sent(
    service: Service,
    verb: &str,
    path: String,
    query: &[(&str, &str)],
    body: Body,
    repeat: Repeat,
) -> Request {
    Request {
        service,
        verb: verb.into(),
        path,
        query: query
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect(),
        body,
        repeat,
    }
}

/// One file's bytes from a workspace path, at most `cap`.
fn read_capped(path: &Path, cap: u64) -> Result<Vec<u8>, String> {
    use std::io::Read as _;
    let meta =
        std::fs::metadata(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if !meta.is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    let f =
        std::fs::File::open(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut bytes = Vec::new();
    f.take(cap + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if bytes.len() as u64 > cap {
        return Err(format!(
            "{} is over {} MB: Drive's simple upload takes less; `google.call` can send it in parts",
            path.display(),
            cap >> 20
        ));
    }
    Ok(bytes)
}

fn ext_of(name: &str) -> String {
    Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// The media type of a file by its extension.
fn media_of_ext(ext: &str) -> &'static str {
    match ext {
        "md" | "markdown" => "text/markdown",
        "csv" => "text/csv",
        "txt" => "text/plain",
        "html" => "text/html",
        "json" => "application/json",
        "pdf" => "application/pdf",
        "pptx" => PPTX,
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        _ => "application/octet-stream",
    }
}

// ------------------------------------------------------------------ the tool

impl Op {
    fn name(self) -> &'static str {
        match self {
            Op::Find => FIND,
            Op::Read => READ,
            Op::Write => WRITE,
            Op::Share => SHARE,
        }
    }
}

impl Tool for DriveTool {
    fn name(&self) -> &str {
        self.1.name()
    }

    fn description(&self) -> &str {
        match self.1 {
            Op::Find => {
                "Find files in the owner's Drive. `text` searches inside files; `q` is Drive's \
                 query syntax. Rows: id, name, type, owner, modified, shared, link. Private; \
                 others' files are outside text."
            }
            Op::Read => {
                "Read a Drive file: a Doc as markdown, a Sheet as tabs and its first 200 rows \
                 (or `range`), Slides as text by slide. A PDF, other binary or file over 1 MB \
                 needs `save_to`. Private; outside text unless the owner's own and unshared."
            }
            Op::Write => {
                "Write to Drive. No `id`: create `name` from `content` or workspace `from_path` (.md \
                 is a Doc, .csv a Sheet; `as` sets it). With `id`: replace content, \
                 `move_to`, or `rename`."
            }
            Op::Share => {
                "Share a Drive file with `email` or `anyone_with_link` as `role`, `remove` that \
                 access, or `trash` the file. Asks the owner every time."
            }
        }
    }

    fn input_schema(&self) -> Value {
        match self.1 {
            Op::Find => json!({
                "type": "object",
                "properties": {
                    "q": {"type": "string"},
                    "text": {"type": "string"},
                    "name": {"type": "string"},
                    "folder": {"type": "string"},
                    "type": {"type": "string", "enum": ["doc", "sheet", "slides", "pdf", "folder"]},
                    "recent": {"type": "boolean"},
                    "max": {"type": "integer"}
                },
                "additionalProperties": false
            }),
            Op::Read => json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "range": {"type": "string"},
                    "save_to": {"type": "string"}
                },
                "required": ["id"],
                "additionalProperties": false
            }),
            Op::Write => json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "name": {"type": "string"},
                    "folder": {"type": "string"},
                    "content": {"type": "string"},
                    "from_path": {"type": "string"},
                    "as": {"type": "string", "enum": ["doc", "sheet", "raw"]},
                    "move_to": {"type": "string"},
                    "rename": {"type": "string"}
                },
                "additionalProperties": false
            }),
            Op::Share => json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "email": {"type": "string"},
                    "role": {"type": "string", "enum": ["reader", "commenter", "writer"]},
                    "anyone_with_link": {"type": "boolean"},
                    "remove": {"type": "boolean"},
                    "notify": {"type": "boolean"},
                    "trash": {"type": "boolean"}
                },
                "required": ["id"],
                "additionalProperties": false
            }),
        }
    }

    fn class(&self) -> ToolClass {
        match self.1 {
            Op::Find | Op::Read => ToolClass::Read,
            Op::Write | Op::Share => ToolClass::Write,
        }
    }

    fn backend(&self) -> Backend {
        Backend::Async
    }

    fn retry(&self) -> Retry {
        match self.1 {
            Op::Find | Op::Read => Retry::SafeToRepeat,
            // A write may have run when a crash cut it: unknown, never run again.
            Op::Write | Op::Share => Retry::NonRepeatable,
        }
    }

    fn plan(&self, input: &Value, ctx: &ToolCtx) -> Result<Plan, String> {
        if !self.0.services().contains(&Service::Drive) {
            return Err("drive is not in `[google] services`, so nothing of it is called".into());
        }
        match self.1 {
            Op::Find => {
                let q = Find::of(input)?.q;
                Ok(Plan {
                    summary: format!("find in Drive: {q}"),
                    ..Default::default()
                })
            }
            Op::Read => {
                let r = Reading::of(input, ctx)?;
                Ok(Plan {
                    summary: format!("read Drive file {}", r.id),
                    // The file it saves is the call's own: the floor and the roots apply.
                    resources: r
                        .save_to
                        .iter()
                        .map(|p| resource(p, Access::Write))
                        .collect(),
                    ..Default::default()
                })
            }
            Op::Write => {
                let w = Writing::of(input, ctx)?;
                Ok(Plan {
                    summary: w.summary(),
                    resources: w.from.iter().map(|p| resource(p, Access::Read)).collect(),
                    ..Default::default()
                })
            }
            Op::Share => {
                let s = Sharing::of(input)?;
                Ok(Plan {
                    summary: s.summary(),
                    class: Some(ToolClass::Write),
                    authority: Some(format!(
                        "{}, which reaches other people or removes a file, so the owner decides \
                         it, every time, at every posture",
                        s.summary()
                    )),
                    ..Default::default()
                })
            }
        }
    }

    fn run_async(&self, input: &Value, ctx: &ToolCtx) -> AsyncRun {
        let g = self.0.clone();
        let umask = ctx.umask;
        let work: Result<Work, String> = match self.1 {
            Op::Find => Find::of(input).map(Work::Find),
            Op::Read => Reading::of(input, ctx).map(Work::Read),
            Op::Write => Writing::of(input, ctx).map(Work::Write),
            Op::Share => Sharing::of(input).map(Work::Share),
        };
        Box::pin(async move {
            match work.map_err(ToolFailure::new)? {
                Work::Find(f) => f.run(&g).await,
                Work::Read(r) => r.run(&g, umask).await,
                Work::Write(w) => w.run(&g).await,
                Work::Share(s) => s.run(&g).await,
            }
        })
    }

    fn rest(&self, _left_out: &str) -> String {
        match self.1 {
            Op::Read => "`save_to` writes the whole file to a workspace path, and a Sheet's `range` reads another part".into(),
            _ => "a narrower call returns them".into(),
        }
    }
}

fn resource(path: &Path, access: Access) -> Resource {
    Resource {
        path: path.to_path_buf(),
        access,
    }
}

enum Work {
    Find(Find),
    Read(Reading),
    Write(Writing),
    Share(Sharing),
}

/// A call of this tool's: the label and id its row names, the class word.
fn meta(tool: &'static str, method: &str, id: Option<&str>, class: &'static str) -> Meta {
    Meta {
        tool,
        method: method.into(),
        resource: id.map(String::from),
        class,
        recipients: None,
    }
}

async fn call(g: &Google, m: Meta, req: Request) -> Result<Called, ToolFailure> {
    g.call(m, req).await.map_err(failed)
}

// -------------------------------------------------------------------- find

struct Find {
    q: String,
    max: u32,
    recent: bool,
}

impl Find {
    fn of(input: &Value) -> Result<Find, String> {
        let a: FindArgs = parse(input)?;
        Ok(Find {
            q: compile_q(
                a.q.as_deref(),
                a.text.as_deref(),
                a.name.as_deref(),
                a.folder.as_deref(),
                a.r#type.as_deref(),
            )?,
            max: a.max.unwrap_or(LIST_DEFAULT).clamp(1, LIST_MAX),
            recent: a.recent,
        })
    }

    async fn run(self, g: &Google) -> Result<Finished, ToolFailure> {
        let fields = format!("files({FIELDS})");
        let max = self.max.to_string();
        let mut query = vec![
            ("q", self.q.as_str()),
            ("pageSize", max.as_str()),
            ("fields", fields.as_str()),
            ("supportsAllDrives", "true"),
            ("includeItemsFromAllDrives", "true"),
        ];
        if self.recent {
            query.push(("orderBy", "modifiedTime desc"));
        }
        let done = call(
            g,
            meta(FIND, "drive.files.list", None, "read"),
            get("/drive/v3/files".into(), &query),
        )
        .await?;
        let files: Vec<Info> = done.response.json()["files"]
            .as_array()
            .map(|a| a.iter().map(Info::of).collect())
            .unwrap_or_default();
        let mut text = format!("{} file(s) for {}\n", files.len(), self.q);
        if !files.is_empty() {
            text.push_str("id | name | type | owner | modified | shared | link\n");
        }
        for f in &files {
            text.push_str(&f.line());
            text.push('\n');
        }
        // A listing is stranger text unless every row is the account's own and unshared.
        let source = if files.iter().all(|f| f.own && !f.shared) {
            Source::None
        } else {
            Source::Outside {
                label: "drive:find",
            }
        };
        Ok(finish(
            g.account(),
            text,
            json!({"files": files.len(), "q": self.q}),
            done.row,
            source,
        ))
    }
}

// -------------------------------------------------------------------- read

struct Reading {
    id: String,
    range: Option<String>,
    save_to: Option<PathBuf>,
}

impl Reading {
    fn of(input: &Value, ctx: &ToolCtx) -> Result<Reading, String> {
        let a: ReadArgs = parse(input)?;
        Ok(Reading {
            id: id_of(&a.id)?.into(),
            range: a.range,
            save_to: a.save_to.as_deref().map(|p| ctx.resolve(p)),
        })
    }

    async fn run(self, g: &Google, umask: Option<u32>) -> Result<Finished, ToolFailure> {
        let id = self.id.as_str();
        let file = format!("/drive/v3/files/{id}");
        let info = call(
            g,
            meta(READ, "drive.files.get", Some(id), "read"),
            get(
                file.clone(),
                &[("fields", FIELDS), ("supportsAllDrives", "true")],
            ),
        )
        .await?;
        let i = Info::of(&info.response.json());
        let (bytes, row) = match i.mime.as_str() {
            FOLDER => {
                return Err(ToolFailure::new(format!(
                    "{} is a folder: `drive.find` with folder={id} lists it",
                    i.name
                )))
            }
            DOC => {
                let d = call(
                    g,
                    meta(READ, "drive.files.export", Some(id), "read"),
                    get(format!("{file}/export"), &[("mimeType", "text/markdown")]),
                )
                .await?;
                (d.response.body, d.row)
            }
            SHEET => {
                let (table, row) = self.sheet(g).await?;
                (table.into_bytes(), row)
            }
            SLIDES => {
                let (text, row) = self.slides(g).await?;
                (text.into_bytes(), row)
            }
            m if m.starts_with("application/vnd.google-apps.") => {
                return Err(ToolFailure::new(format!(
                    "{} is a Google {}: it has no text read here; `google.call` with \
                     drive.files.export reads it in a format Drive offers",
                    i.name,
                    m.rsplit('.').next().unwrap_or(m)
                )))
            }
            _ => {
                if i.size > INLINE_MAX && self.save_to.is_none() {
                    return Err(ToolFailure::new(format!(
                        "{} is {} bytes, over 1 MB: `save_to` writes it to a workspace path",
                        i.name, i.size
                    )));
                }
                let d = call(
                    g,
                    meta(READ, "drive.files.get", Some(id), "read"),
                    get(file, &[("alt", "media"), ("supportsAllDrives", "true")]),
                )
                .await?;
                (d.response.body, d.row)
            }
        };
        let head = format!("{} ({}, {})", i.name, type_word(&i.mime), i.id);
        let mut m = json!({"id": i.id, "name": i.name, "mime": i.mime, "bytes": bytes.len()});
        let text = match &self.save_to {
            Some(path) => {
                let (p, b) = (path.clone(), bytes.clone());
                theseus_store::blocking(move || theseus_tools::fs::write_atomic(&p, &b, umask))
                    .map_err(|e| {
                        ToolFailure::new(format!("{head}; writing {}: {e}", path.display()))
                    })?;
                m["saved_to"] = json!(path);
                format!("{head}: {} bytes written to {}\n", bytes.len(), path.display())
            }
            None => match String::from_utf8(bytes) {
                Ok(t) if (t.len() as u64) <= INLINE_MAX => format!("{head}\n\n{t}\n"),
                Ok(t) => {
                    return Err(ToolFailure::new(format!(
                        "{head} is {} bytes of text, over 1 MB: `save_to` writes it to a workspace path",
                        t.len()
                    )))
                }
                Err(_) => {
                    return Err(ToolFailure::new(format!(
                        "{head} is not text: `save_to` writes it to a workspace path"
                    )))
                }
            },
        };
        Ok(finish(g.account(), text, m, row, i.source()))
    }

    /// A Sheet: its tabs, then the first rows of the first tab, or `range`.
    async fn sheet(&self, g: &Google) -> Result<(String, Value), ToolFailure> {
        let id = self.id.as_str();
        let base = format!("/v4/spreadsheets/{id}");
        let tabs = call(
            g,
            meta(READ, "sheets.spreadsheets.get", Some(id), "read"),
            sent(
                Service::Sheets,
                "GET",
                base.clone(),
                &[("fields", "sheets.properties(title,gridProperties)")],
                Body::None,
                Repeat::Safe,
            ),
        )
        .await?;
        let tabs = tabs.response.json();
        let titles: Vec<(String, u64, u64)> = tabs["sheets"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|s| {
                        let p = &s["properties"];
                        (
                            p["title"].as_str().unwrap_or("").to_string(),
                            p["gridProperties"]["rowCount"].as_u64().unwrap_or(0),
                            p["gridProperties"]["columnCount"].as_u64().unwrap_or(0),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let range = match (&self.range, titles.first()) {
            (Some(r), _) => r.clone(),
            (None, Some((t, ..))) => format!("{}!1:{SHEET_ROWS}", quote_tab(t)),
            (None, None) => return Err(ToolFailure::new("the spreadsheet has no tabs")),
        };
        let vals = call(
            g,
            meta(
                READ,
                "sheets.spreadsheets.values.batchGet",
                Some(id),
                "read",
            ),
            sent(
                Service::Sheets,
                "GET",
                format!("{base}/values:batchGet"),
                &[("ranges", range.as_str())],
                Body::None,
                Repeat::Safe,
            ),
        )
        .await?;
        let v = vals.response.json();
        let rows = v["valueRanges"][0]["values"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let mut out = String::from("Tabs:\n");
        for (t, r, c) in &titles {
            out.push_str(&format!("- {t} ({r} rows x {c} columns)\n"));
        }
        out.push_str(&format!(
            "\n{} row(s) of {}:\n\n{}",
            rows.len(),
            v["valueRanges"][0]["range"].as_str().unwrap_or(&range),
            table(&rows)
        ));
        Ok((out, vals.row))
    }

    /// Slides: each slide's text by object id, with its layout.
    async fn slides(&self, g: &Google) -> Result<(String, Value), ToolFailure> {
        let id = self.id.as_str();
        const MASK: &str = "slides(objectId,slideProperties(layoutObjectId),pageElements(objectId,\
            shape(text(textElements(textRun(content)))))),layouts(objectId,layoutProperties(displayName))";
        let d = call(
            g,
            meta(READ, "slides.presentations.get", Some(id), "read"),
            sent(
                Service::Slides,
                "GET",
                format!("/v1/presentations/{id}"),
                &[("fields", MASK)],
                Body::None,
                Repeat::Safe,
            ),
        )
        .await?;
        let p = d.response.json();
        let layouts = p["layouts"].as_array().cloned().unwrap_or_default();
        let layout_of = |lid: &str| {
            layouts
                .iter()
                .find(|l| l["objectId"] == lid)
                .and_then(|l| l["layoutProperties"]["displayName"].as_str())
                .unwrap_or(lid)
                .to_string()
        };
        let mut out = String::new();
        for (n, s) in p["slides"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            let lid = s["slideProperties"]["layoutObjectId"]
                .as_str()
                .unwrap_or("");
            out.push_str(&format!(
                "## Slide {} ({}, layout {})\n",
                n + 1,
                s["objectId"].as_str().unwrap_or("?"),
                layout_of(lid)
            ));
            for e in s["pageElements"].as_array().cloned().unwrap_or_default() {
                let text: String = e["shape"]["text"]["textElements"]
                    .as_array()
                    .map(|t| {
                        t.iter()
                            .filter_map(|t| t["textRun"]["content"].as_str())
                            .collect()
                    })
                    .unwrap_or_default();
                if !text.trim().is_empty() {
                    out.push_str(&format!(
                        "[{}] {}\n",
                        e["objectId"].as_str().unwrap_or("?"),
                        text.trim_end()
                    ));
                }
            }
            out.push('\n');
        }
        Ok((out, d.row))
    }
}

/// A tab's name as A1 notation quotes it.
fn quote_tab(t: &str) -> String {
    format!("'{}'", t.replace('\'', "''"))
}

/// Rows of cells as a markdown table, the first row its header.
fn table(rows: &[Value]) -> String {
    let cells = |r: &Value| -> Vec<String> {
        r.as_array()
            .map(|c| {
                c.iter()
                    .map(|c| {
                        let s = c
                            .as_str()
                            .map(String::from)
                            .unwrap_or_else(|| c.to_string());
                        s.replace('|', "\\|").replace('\n', " ")
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let width = rows.iter().map(|r| cells(r).len()).max().unwrap_or(0);
    let mut out = String::new();
    for (n, r) in rows.iter().enumerate() {
        let mut c = cells(r);
        c.resize(width, String::new());
        out.push_str(&format!("| {} |\n", c.join(" | ")));
        if n == 0 {
            out.push_str(&format!("|{}\n", " --- |".repeat(width)));
        }
    }
    out
}

// ------------------------------------------------------------------- write

struct Writing {
    id: Option<String>,
    name: Option<String>,
    folder: Option<String>,
    from: Option<PathBuf>,
    content: Option<String>,
    /// What a create converts to (`DOC`, `SHEET`, `SLIDES`), if anything.
    target: Option<&'static str>,
    media: &'static str,
    move_to: Option<String>,
    rename: Option<String>,
}

impl Writing {
    fn of(input: &Value, ctx: &ToolCtx) -> Result<Writing, String> {
        let a: WriteArgs = parse(input)?;
        if a.content.is_some() && a.from_path.is_some() {
            return Err("give `content` or `from_path`, not both".into());
        }
        let from = a.from_path.as_deref().map(|p| ctx.resolve(p));
        let ext = match (&from, &a.name) {
            (Some(p), _) => ext_of(&p.to_string_lossy()),
            (None, Some(n)) => ext_of(n),
            _ => String::new(),
        };
        let kind = match a.r#as.as_deref() {
            Some(k @ ("doc" | "sheet" | "slides" | "raw")) => k,
            Some(k) => return Err(format!("`as` {k:?} is not doc, sheet, slides or raw")),
            None => match ext.as_str() {
                "md" | "markdown" => "doc",
                "csv" => "sheet",
                _ => "raw",
            },
        };
        if kind == "slides" && from.is_none() {
            return Err(
                "slides come from a .pptx `from_path` (`slides.write` makes a deck from text)"
                    .into(),
            );
        }
        let target = match kind {
            "doc" => Some(DOC),
            "sheet" => Some(SHEET),
            "slides" => Some(SLIDES),
            _ => None,
        };
        let media = match (&from, kind) {
            (Some(_), _) => media_of_ext(&ext),
            (None, "doc") => "text/markdown",
            (None, "sheet") => "text/csv",
            _ => "text/plain",
        };
        let id = a.id.as_deref().map(id_of).transpose()?.map(String::from);
        let has_content = a.content.is_some() || from.is_some();
        match &id {
            None if a.name.as_deref().is_none_or(|n| n.trim().is_empty()) => {
                return Err("a new file needs a `name`".into())
            }
            None if a.move_to.is_some() || a.rename.is_some() => {
                return Err("`move_to` and `rename` change an existing file: give its `id`".into())
            }
            Some(_) if a.name.is_some() || a.folder.is_some() || a.r#as.is_some() => {
                return Err("`name`, `folder` and `as` make a new file: with an `id`, `rename` and `move_to` change it".into())
            }
            Some(_) if !has_content && a.move_to.is_none() && a.rename.is_none() => {
                return Err("with an `id`, give `content`, `from_path`, `move_to` or `rename`".into())
            }
            _ => {}
        }
        Ok(Writing {
            id,
            name: a.name,
            folder: a
                .folder
                .as_deref()
                .map(id_of)
                .transpose()?
                .map(String::from),
            from,
            content: a.content,
            target,
            media,
            move_to: a
                .move_to
                .as_deref()
                .map(id_of)
                .transpose()?
                .map(String::from),
            rename: a.rename,
        })
    }

    fn summary(&self) -> String {
        match &self.id {
            None => format!(
                "create Drive file {:?}{}",
                self.name.as_deref().unwrap_or(""),
                self.target
                    .map_or(String::new(), |t| format!(" as a {}", type_word(t)))
            ),
            Some(id) => {
                let mut what = Vec::new();
                if self.from.is_some() || self.content.is_some() {
                    what.push("replace its content".to_string());
                }
                if let Some(m) = &self.move_to {
                    what.push(format!("move it to {m}"));
                }
                if let Some(r) = &self.rename {
                    what.push(format!("rename it to {r:?}"));
                }
                format!("change Drive file {id}: {}", what.join(", "))
            }
        }
    }

    /// The bytes to send, if any: the text, or the file read (off the runtime).
    fn body(&self) -> Result<Option<Vec<u8>>, ToolFailure> {
        if let Some(c) = &self.content {
            if c.len() as u64 > UPLOAD_MAX {
                return Err(ToolFailure::new(
                    "`content` is over 5 MB: write it to a file and use `from_path`",
                ));
            }
            return Ok(Some(c.clone().into_bytes()));
        }
        match &self.from {
            Some(p) => {
                let p = p.clone();
                theseus_store::blocking(move || read_capped(&p, UPLOAD_MAX))
                    .map(Some)
                    .map_err(ToolFailure::new)
            }
            None => Ok(None),
        }
    }

    async fn run(self, g: &Google) -> Result<Finished, ToolFailure> {
        let bytes = self.body()?;
        let (done, what) = match &self.id {
            None => (self.create(g, bytes).await?, "created"),
            Some(id) => (self.change(g, id, bytes).await?, "changed"),
        };
        let i = Info::of(&done.response.json());
        let text = format!(
            "{what} {} ({}, {}) {}\n",
            i.name,
            type_word(&i.mime),
            i.id,
            i.link
        );
        let m = json!({"id": i.id, "name": i.name, "mime": i.mime});
        // The owner's own write: no one else's text came back.
        Ok(finish(g.account(), text, m, done.row, Source::None))
    }

    async fn create(&self, g: &Google, bytes: Option<Vec<u8>>) -> Result<Called, ToolFailure> {
        let mut m = json!({"name": self.name});
        if let Some(t) = self.target {
            m["mimeType"] = t.into();
        }
        if let Some(f) = &self.folder {
            m["parents"] = json!([f]);
        }
        let meta = meta(WRITE, "drive.files.create", None, "write");
        let query = [("fields", FIELDS), ("supportsAllDrives", "true")];
        let req = match bytes {
            None => sent(
                Service::Drive,
                "POST",
                "/drive/v3/files".into(),
                &query,
                Body::Json(m),
                Repeat::NonRepeatable,
            ),
            Some(data) => {
                let b = format!("theseus-{}", uuid::Uuid::new_v4().simple());
                let mut body = format!(
                    "--{b}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{m}\r\n--{b}\r\nContent-Type: {}\r\n\r\n",
                    self.media
                )
                .into_bytes();
                body.extend_from_slice(&data);
                body.extend_from_slice(format!("\r\n--{b}--").as_bytes());
                let mut q = query.to_vec();
                q.push(("uploadType", "multipart"));
                sent(
                    Service::Drive,
                    "POST",
                    "/upload/drive/v3/files".into(),
                    &q,
                    Body::Bytes {
                        content_type: format!("multipart/related; boundary={b}"),
                        data: body,
                    },
                    Repeat::NonRepeatable,
                )
            }
        };
        call(g, meta, req).await
    }

    async fn change(
        &self,
        g: &Google,
        id: &str,
        bytes: Option<Vec<u8>>,
    ) -> Result<Called, ToolFailure> {
        let query = [("fields", FIELDS), ("supportsAllDrives", "true")];
        let mut done = None;
        if let Some(data) = bytes {
            let mut q = query.to_vec();
            q.push(("uploadType", "media"));
            let req = sent(
                Service::Drive,
                "PATCH",
                format!("/upload/drive/v3/files/{id}"),
                &q,
                Body::Bytes {
                    content_type: self.media.into(),
                    data,
                },
                Repeat::Safe,
            );
            done = Some(call(g, meta(WRITE, "drive.files.update", Some(id), "write"), req).await?);
        }
        if self.move_to.is_some() || self.rename.is_some() {
            // A move adds the new parent and removes the ones the file has.
            let parents = match &self.move_to {
                Some(to) => {
                    let cur = call(
                        g,
                        meta(WRITE, "drive.files.get", Some(id), "read"),
                        get(
                            format!("/drive/v3/files/{id}"),
                            &[("fields", "parents"), ("supportsAllDrives", "true")],
                        ),
                    )
                    .await?;
                    let had = cur.response.json()["parents"]
                        .as_array()
                        .map(|p| {
                            p.iter()
                                .filter_map(Value::as_str)
                                .collect::<Vec<_>>()
                                .join(",")
                        })
                        .unwrap_or_default();
                    Some((to.clone(), had))
                }
                None => None,
            };
            let mut q = query.to_vec();
            if let Some((add, remove)) = &parents {
                q.push(("addParents", add.as_str()));
                q.push(("removeParents", remove.as_str()));
            }
            let body = self
                .rename
                .as_ref()
                .map_or(json!({}), |r| json!({"name": r}));
            let req = sent(
                Service::Drive,
                "PATCH",
                format!("/drive/v3/files/{id}"),
                &q,
                Body::Json(body),
                Repeat::Safe,
            );
            done = Some(call(g, meta(WRITE, "drive.files.update", Some(id), "write"), req).await?);
        }
        done.ok_or_else(|| ToolFailure::new("nothing to change"))
    }
}

// ------------------------------------------------------------------- share

enum Change {
    Trash(bool),
    Grant {
        who: Who,
        role: String,
        notify: bool,
    },
    Revoke(Who),
}

enum Who {
    User(String),
    Anyone,
}

struct Sharing {
    id: String,
    change: Change,
}

impl Sharing {
    fn of(input: &Value) -> Result<Sharing, String> {
        let a: ShareArgs = parse(input)?;
        let id = id_of(&a.id)?.to_string();
        if let Some(t) = a.trash {
            if a.email.is_some() || a.anyone_with_link || a.remove || a.role.is_some() || a.notify {
                return Err(
                    "`trash` is a call of its own: it takes no `email`, `role` or the like".into(),
                );
            }
            return Ok(Sharing {
                id,
                change: Change::Trash(t),
            });
        }
        let who = match (&a.email, a.anyone_with_link) {
            (Some(e), false) if e.contains('@') && !e.contains(char::is_whitespace) => {
                Who::User(e.clone())
            }
            (Some(e), false) => return Err(format!("{e:?} is not an email address")),
            (None, true) => Who::Anyone,
            (Some(_), true) => return Err("give `email` or `anyone_with_link`, not both".into()),
            (None, false) => return Err("give `email`, `anyone_with_link` or `trash`".into()),
        };
        if a.remove {
            if a.role.is_some() || a.notify {
                return Err("`remove` takes no `role` or `notify`".into());
            }
            return Ok(Sharing {
                id,
                change: Change::Revoke(who),
            });
        }
        let role = a.role.unwrap_or_else(|| "reader".into());
        if !["reader", "commenter", "writer"].contains(&role.as_str()) {
            return Err(format!(
                "`role` {role:?} is not reader, commenter or writer"
            ));
        }
        Ok(Sharing {
            id,
            change: Change::Grant {
                who,
                role,
                notify: a.notify,
            },
        })
    }

    fn summary(&self) -> String {
        let who = |w: &Who| match w {
            Who::User(e) => e.clone(),
            Who::Anyone => "anyone with the link".into(),
        };
        match &self.change {
            Change::Trash(true) => format!("move Drive file {} to the Trash", self.id),
            Change::Trash(false) => format!("restore Drive file {} from the Trash", self.id),
            Change::Grant {
                who: w,
                role,
                notify,
            } => format!(
                "share Drive file {} with {} as {role}{}",
                self.id,
                who(w),
                if *notify {
                    " and send them the invitation"
                } else {
                    ""
                }
            ),
            Change::Revoke(w) => format!("stop sharing Drive file {} with {}", self.id, who(w)),
        }
    }

    async fn run(self, g: &Google) -> Result<Finished, ToolFailure> {
        let id = self.id.as_str();
        let files = format!("/drive/v3/files/{id}");
        let done = match &self.change {
            Change::Trash(t) => {
                let req = sent(
                    Service::Drive,
                    "PATCH",
                    files,
                    &[("fields", "id,name,trashed"), ("supportsAllDrives", "true")],
                    Body::Json(json!({"trashed": t})),
                    Repeat::Safe,
                );
                call(
                    g,
                    meta(SHARE, "drive.files.update", Some(id), "authority"),
                    req,
                )
                .await?
            }
            Change::Grant { who, role, notify } => {
                let body = match who {
                    Who::User(e) => json!({"type": "user", "role": role, "emailAddress": e}),
                    Who::Anyone => json!({"type": "anyone", "role": role}),
                };
                let mail = notify.to_string();
                let req = sent(
                    Service::Drive,
                    "POST",
                    format!("{files}/permissions"),
                    &[
                        ("sendNotificationEmail", mail.as_str()),
                        ("supportsAllDrives", "true"),
                    ],
                    Body::Json(body),
                    Repeat::NonRepeatable,
                );
                call(
                    g,
                    meta(SHARE, "drive.permissions.create", Some(id), "authority"),
                    req,
                )
                .await?
            }
            Change::Revoke(who) => {
                let list = call(
                    g,
                    meta(SHARE, "drive.permissions.list", Some(id), "read"),
                    get(
                        format!("{files}/permissions"),
                        &[
                            ("fields", "permissions(id,type,emailAddress,role)"),
                            ("supportsAllDrives", "true"),
                        ],
                    ),
                )
                .await?;
                let found = list.response.json()["permissions"]
                    .as_array()
                    .and_then(|p| {
                        p.iter()
                            .find(|p| match who {
                                Who::Anyone => p["type"] == "anyone",
                                Who::User(e) => p["emailAddress"]
                                    .as_str()
                                    .is_some_and(|x| x.eq_ignore_ascii_case(e)),
                            })
                            .and_then(|p| p["id"].as_str().map(String::from))
                    });
                let pid = found.ok_or_else(|| {
                    ToolFailure::new(format!("{}: no such access to remove", self.summary()))
                })?;
                let pid = id_of(&pid).map_err(ToolFailure::new)?;
                let req = sent(
                    Service::Drive,
                    "DELETE",
                    format!("{files}/permissions/{pid}"),
                    &[("supportsAllDrives", "true")],
                    Body::None,
                    Repeat::NonRepeatable,
                );
                call(
                    g,
                    meta(SHARE, "drive.permissions.delete", Some(id), "authority"),
                    req,
                )
                .await?
            }
        };
        let text = format!("done: {}\n", self.summary());
        Ok(finish(
            g.account(),
            text,
            json!({"id": id}),
            done.row,
            Source::None,
        ))
    }
}
