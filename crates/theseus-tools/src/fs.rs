//! `fs.*` (spec §3.24): read, write, edit, patch, glob, grep, list. Native:
//! the walker and gitignore handling are ripgrep's own crates (`ignore`,
//! `globset`, `grep-searcher`); writes are atomic (temp file, then rename).

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::SystemTime;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::fs_lines::{is_all_crlf, lf_regions, splice_lf_regions, to_crlf};
use crate::fs_walk::{not_walked, walk_builder, walker};
use crate::{
    image, parse, Access, Cores, ImageData, Media, Plan, Resource, Retry, Tool, ToolClass, ToolCtx,
    ToolFailure, ToolOutput,
};

pub(crate) const MAX_LINE_CHARS: usize = 2000;
const DEFAULT_READ_LINES: usize = 2000;
pub(crate) const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;

fn is_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8192).any(|b| *b == 0)
}

/// What `fs_read` says of a binary it does not show: its kind when the first
/// bytes tell it (theseus-53ujg.2), else that it is binary.
fn binary_said(path: &Path, bytes: &[u8]) -> ToolOutput {
    let Some(k) = crate::magic::kind(bytes) else {
        return ToolOutput {
            text: format!(
                "{} is a binary file ({} bytes); fs_read returns text only.",
                path.display(),
                bytes.len()
            ),
            meta: json!({"path": path, "bytes": bytes.len(), "binary": true}),
        };
    };
    let shows = if k.image {
        " (fs_read shows PNG, JPEG, GIF and WebP images)"
    } else {
        ""
    };
    ToolOutput {
        text: format!(
            "{} is {}, {} bytes{shows}",
            path.display(),
            k.with_article(),
            crate::magic::grouped(bytes.len() as u64)
        ),
        meta: json!({"path": path, "bytes": bytes.len(), "binary": true, "kind": k.desc}),
    }
}

/// Why a file was not read (review 2's R9).
#[derive(Debug)]
pub(crate) enum Unread {
    Io(std::io::Error),
    /// Not a regular file: what it is instead.
    Kind(&'static str),
    /// Longer than the cap: the bytes read before the cut.
    Over(u64),
}

impl Unread {
    /// In words, after the path: `/x/p is a FIFO, not a regular file; …`.
    pub(crate) fn say(&self, path: &Path, tool: &str) -> String {
        match self {
            Self::Io(e) => format!("cannot read {}: {e}", path.display()),
            Self::Kind(k) => format!(
                "{} is {k}, not a regular file; {tool} reads only regular files (a read of a \
                 FIFO, a socket, or a device can wait forever for a writer)",
                path.display()
            ),
            Self::Over(n) => format!(
                "{} grew while it was read, to {n} bytes or more; files over {MAX_FILE_BYTES} \
                 bytes are not read whole: give offset and limit to read a window of it, or use fs_grep",
                path.display()
            ),
        }
    }
}

impl From<std::io::Error> for Unread {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// What a file type is, when it is not a regular file.
fn not_regular(t: fs::FileType) -> Option<&'static str> {
    use std::os::unix::fs::FileTypeExt;
    if t.is_file() {
        None
    } else if t.is_dir() {
        Some("a directory")
    } else if t.is_fifo() {
        Some("a FIFO")
    } else if t.is_socket() {
        Some("a socket")
    } else if t.is_char_device() {
        Some("a character device")
    } else if t.is_block_device() {
        Some("a block device")
    } else {
        Some("not a file")
    }
}

/// A regular file, open, with its metadata (review 2's R9). A FIFO, a socket,
/// or a device is refused by name: a read of one waits for a writer, and
/// holds a core from the pool and a thread for as long as it waits. The file
/// is opened without blocking and checked again once open, so one swapped in
/// after the first check is refused too. Without `follow`, a symlink is
/// refused as well, even one swapped in after the caller looked.
pub(crate) fn open_regular(path: &Path, follow: bool) -> Result<(fs::File, fs::Metadata), Unread> {
    use std::os::unix::fs::OpenOptionsExt;
    let meta = if follow {
        fs::metadata(path)
    } else {
        fs::symlink_metadata(path)
    }?;
    if let Some(k) = not_regular(meta.file_type()) {
        return Err(Unread::Kind(k));
    }
    let nofollow = if follow { 0 } else { libc::O_NOFOLLOW };
    let f = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | nofollow)
        .open(path)?;
    let meta = f.metadata()?;
    if let Some(k) = not_regular(meta.file_type()) {
        return Err(Unread::Kind(k));
    }
    Ok((f, meta))
}

/// A regular file's bytes, at most `cap` of them (`open_regular`). It is
/// read through the cap, so a file that grows after its size was checked is
/// cut there, never read whole.
pub(crate) fn read_regular(path: &Path, cap: u64, follow: bool) -> Result<Vec<u8>, Unread> {
    use std::io::Read as _;
    let (f, meta) = open_regular(path, follow)?;
    let mut bytes = Vec::with_capacity(meta.len().min(cap) as usize);
    f.take(cap.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > cap {
        return Err(Unread::Over(bytes.len() as u64));
    }
    Ok(bytes)
}

/// A regular file's text, whole: `read_regular` without a cap, for the tools
/// that read a file to change it or compare it.
pub(crate) fn read_regular_text(path: &Path, tool: &str) -> Result<String, ToolFailure> {
    let bytes =
        read_regular(path, u64::MAX, true).map_err(|e| ToolFailure::new(e.say(path, tool)))?;
    String::from_utf8(bytes).map_err(|_| {
        ToolFailure::new(format!(
            "cannot read {}: stream did not contain valid UTF-8",
            path.display()
        ))
    })
}

/// Atomic write: temp file beside the target, fsync, rename; keeps the mode.
/// A new file, and any directory made for it, get the mode `umask` gives
/// (theseus-wz2): the operator's, where the daemon's own is 077. `None`: the
/// process's umask, as it is.
pub fn write_atomic(path: &Path, content: &[u8], umask: Option<u32>) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let dir = path.parent().unwrap_or(Path::new("."));
    create_dirs(dir, umask)?;
    let tmp = dir.join(format!(
        ".{}.theseus-tmp-{}",
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        std::process::id()
    ));
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(content)?;
        f.sync_all()?;
    }
    if let Ok(meta) = fs::metadata(path) {
        let _ = fs::set_permissions(&tmp, meta.permissions());
    } else if let Some(u) = umask {
        let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o666 & !u));
    }
    fs::rename(&tmp, path)
}

/// `create_dir_all`, each directory it makes given the mode `umask` gives.
fn create_dirs(dir: &Path, umask: Option<u32>) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let Some(u) = umask else {
        return fs::create_dir_all(dir);
    };
    let missing: Vec<&Path> = dir
        .ancestors()
        .take_while(|d| !d.as_os_str().is_empty() && !d.exists())
        .collect();
    for d in missing.into_iter().rev() {
        match fs::create_dir(d) {
            Ok(()) => fs::set_permissions(d, fs::Permissions::from_mode(0o777 & !u))?,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// `n` and its noun: `1 file`, `12 files`.
fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn unified(a: &str, b: &str, a_name: &str, b_name: &str, context: usize) -> String {
    similar::TextDiff::from_lines(a, b)
        .unified_diff()
        .context_radius(context)
        .header(a_name, b_name)
        .to_string()
}

// ---------------------------------------------------------------- fs.read

pub struct Read;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadArgs {
    #[serde(alias = "file", alias = "filename", alias = "file_path")]
    path: String,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
    /// A PDF's pages (theseus-c9l6): `3`, `3-5`, or `3-`.
    #[serde(default)]
    pages: Option<String>,
}

impl Tool for Read {
    fn name(&self) -> &'static str {
        "fs.read"
    }
    fn description(&self) -> &'static str {
        "Read a file. A text file comes back with line numbers (`   12\\tline`): use it before editing a file and whenever you need a file's current contents, and pass offset/limit to page through a long one (a file over 16 MiB is read only by window: give offset or limit). An image (PNG, JPEG, GIF, WebP) comes back as an image. A PDF comes back as its pages, up to 20 at a time: pass pages (\"3\", \"3-5\", \"21-\") to choose them; a model that reads PDFs sees each page whole, scans and charts included, and any other reads their text. A document (Word .docx, Excel .xlsx, PowerPoint .pptx, OpenDocument .odt/.ods/.odp, EPUB, RTF, a Jupyter notebook .ipynb) comes back as its text by section (sheets as tables, slides with titles and notes, cells with outputs), and an archive (zip, tar, tar.gz) as its list; pages counts sections. Other binary files are reported, not returned."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File path, absolute or relative to the working directory."},
                "offset": {"type": "integer", "minimum": 1, "description": "First line to return (1-based). Default 1."},
                "limit": {"type": "integer", "minimum": 1, "description": "Number of lines to return. Default 2000."},
                "pages": {"type": "string", "description": "A PDF's pages, counted from 1: \"3\", \"3-5\", or \"21-\" (to the end). At most 20 a read. Default: the first 20."}
            },
            "required": ["path"],
            "additionalProperties": false
        })
    }
    fn class(&self) -> ToolClass {
        ToolClass::Read
    }
    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }
    fn plan(&self, input: &Value, ctx: &ToolCtx) -> Result<Plan, String> {
        let a: ReadArgs = parse(input)?;
        let path = ctx.resolve(&a.path);
        Ok(Plan {
            summary: format!("read {}", path.display()),
            resources: vec![Resource {
                path,
                access: Access::Read,
            }],
            argv: None,
            url: None,
            ..Default::default()
        })
    }
    fn run(&self, input: &Value, ctx: &ToolCtx) -> Result<ToolOutput, ToolFailure> {
        self.run_with_media(input, ctx).map(|(o, _)| o)
    }
    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
    fn run_with_media(
        &self,
        input: &Value,
        ctx: &ToolCtx,
    ) -> Result<(ToolOutput, Option<Media>), ToolFailure> {
        let a: ReadArgs = parse(input).map_err(ToolFailure::new)?;
        let path = ctx.resolve(&a.path);
        let meta = fs::metadata(&path)
            .map_err(|e| ToolFailure::new(format!("cannot read {}: {e}", path.display())))?;
        if meta.is_dir() {
            return Err(ToolFailure::new(format!(
                "{} is a directory; use fs_list",
                path.display()
            )));
        }
        // A PDF may be as large as an attached one (theseus-c9l6).
        let cap = match meta.len() > MAX_FILE_BYTES && crate::docs::starts_as_pdf(&path) {
            true => theseus_files::MAX_FILE_BYTES,
            false => MAX_FILE_BYTES,
        };
        if meta.len() > cap {
            let asked = a.offset.is_some() || a.limit.is_some();
            if cap == MAX_FILE_BYTES && asked {
                let (from, limit) = (
                    a.offset.unwrap_or(1).max(1),
                    a.limit.unwrap_or(DEFAULT_READ_LINES),
                );
                if let Some(out) = crate::fs_window::read(&path, from, limit, ctx.max_read_bytes)? {
                    crate::fs_stale::noted(ctx, &path);
                    return Ok((out, None));
                }
            }
            return Err(ToolFailure::new(crate::fs_window::refusal(
                &path,
                meta.len(),
                asked,
            )));
        }
        let bytes = read_regular(&path, cap, true)
            .map_err(|e| ToolFailure::new(e.say(&path, "fs_read")))?;
        // The file's bytes are read: this session has been handed it, and
        // notes the state it was in (theseus-w2cs sub-item 2). Everything
        // after this is how they are rendered, and a read that failed before
        // it notes nothing, so it brings no path under the guard.
        crate::fs_stale::noted(ctx, &path);
        // An image the models read comes back as an image (theseus-9g2),
        // capped as an attached one is.
        if let Some(info) = image::sniff(&bytes) {
            let size = bytes.len() as u64;
            let what = format!(
                "{} is a {} image, {}×{}, {} bytes",
                path.display(),
                info.kind(),
                info.width,
                info.height,
                size
            );
            let mut meta = json!({"path": path, "bytes": size, "image": info.media_type, "width": info.width, "height": info.height});
            if let Some(why) = image::refusal(size, &info) {
                meta["not_shown"] = json!(why);
                return Ok((
                    ToolOutput {
                        text: format!("{what}: not shown, {why}."),
                        meta,
                    },
                    None,
                ));
            }
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());
            return Ok((
                ToolOutput {
                    text: format!("{what}."),
                    meta,
                },
                Some(Media::Image(ImageData { name, info, bytes })),
            ));
        }
        if theseus_files::pdf::is_pdf(&bytes) {
            return crate::docs::read_pdf(&path, bytes, a.pages.as_deref());
        }
        if let Some(out) =
            crate::docs::read_doc(&path, &bytes, a.pages.as_deref(), ctx.max_read_bytes)?
        {
            return Ok((out, None));
        }
        if a.pages.is_some() {
            return Err(ToolFailure::new(format!(
                "{} is not a PDF or a document; pages is for those (use offset/limit for lines)",
                path.display()
            )));
        }
        if is_binary(&bytes) {
            return Ok((binary_said(&path, &bytes), None));
        }
        let text = String::from_utf8_lossy(&bytes);
        let lines: Vec<&str> = text.lines().collect();
        let total = lines.len();
        let from = a.offset.unwrap_or(1).max(1);
        // A read that names neither offset nor limit takes the turn's own
        // default window (theseus-tdkcn); the schema's "Default 2000" is unchanged.
        let default = match a.offset.is_none() && ctx.default_read_lines > 0 {
            true => ctx.default_read_lines,
            false => DEFAULT_READ_LINES,
        };
        let limit = a.limit.unwrap_or(default);
        let mut out = String::new();
        let mut room = crate::fs_read_cap::Room::new(ctx.max_read_bytes);
        let mut last = from.saturating_sub(1);
        for (i, line) in lines.iter().enumerate().skip(from - 1).take(limit) {
            let l = if line.chars().count() > MAX_LINE_CHARS {
                format!(
                    "{}… [line truncated]",
                    line.chars().take(MAX_LINE_CHARS).collect::<String>()
                )
            } else {
                line.to_string()
            };
            let row = format!("{:>6}\t{}\n", i + 1, l);
            if !room.takes(&row) {
                break;
            }
            out.push_str(&row);
            last = i + 1;
        }
        if total == 0 {
            out.push_str("(empty file)\n");
        } else if from > total {
            out = format!("(offset {from} is past the end: the file has {total} lines)\n");
        } else if last < total {
            out.push_str(&format!(
                "[showing lines {from}-{last} of {total}; pass offset={} to read on]\n",
                last + 1
            ));
        }
        Ok((
            ToolOutput {
                text: out,
                meta: json!({"path": path, "lines_total": total, "from": from, "to": last, "bytes": bytes.len()}),
            },
            None,
        ))
    }
    fn result_max_chars(&self) -> Option<usize> {
        Some(crate::fs_read_cap::FS_READ_MAX_CHARS)
    }
    /// The rows a cut left out, by their numbers (theseus-46v): an `fs_read`
    /// with that offset and limit returns them.
    fn rest(&self, left_out: &str) -> String {
        let rows: Vec<Option<usize>> = left_out.lines().map(row_number).collect();
        let (Some(i), Some(j)) = (
            rows.iter().position(Option::is_some),
            rows.iter().rposition(Option::is_some),
        ) else {
            return "fs_read with a smaller limit returns them".into();
        };
        // A row cut part-way at either end belongs to the row before the
        // first whole one, or after the last.
        let first = rows[i]
            .unwrap_or(1)
            .saturating_sub(usize::from(i > 0))
            .max(1);
        let last = rows[j].unwrap_or(first) + usize::from(j + 1 < rows.len());
        format!(
            "lines {first}-{last}; fs_read with offset={first} and limit={} returns them",
            last + 1 - first
        )
    }
}

/// A row's number in `fs.read`'s output (`    12\tline`), if `line` is a
/// whole row.
fn row_number(line: &str) -> Option<usize> {
    let (n, _) = line.split_once('\t')?;
    n.trim_start().parse().ok()
}

// ---------------------------------------------------------------- fs.write

pub struct WriteFile;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteArgs {
    #[serde(alias = "file", alias = "filename", alias = "file_path")]
    path: String,
    #[serde(alias = "file_text", alias = "contents")]
    content: String,
}

impl Tool for WriteFile {
    fn name(&self) -> &'static str {
        "fs.write"
    }
    fn description(&self) -> &'static str {
        "Create a file or replace its whole contents. Prefer fs_edit for changes to an existing file; use this for new files or complete rewrites. Parent directories are created."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File path, absolute or relative to the working directory."},
                "content": {"type": "string", "description": "The complete new contents."}
            },
            "required": ["path", "content"],
            "additionalProperties": false
        })
    }
    fn class(&self) -> ToolClass {
        ToolClass::Write
    }
    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }
    fn plan(&self, input: &Value, ctx: &ToolCtx) -> Result<Plan, String> {
        let a: WriteArgs = parse(input)?;
        let path = ctx.resolve(&a.path);
        let verb = if path.exists() { "replace" } else { "create" };
        Ok(Plan {
            summary: format!("{verb} {} ({} bytes)", path.display(), a.content.len()),
            resources: vec![Resource {
                path,
                access: Access::Write,
            }],
            argv: None,
            url: None,
            ..Default::default()
        })
    }
    fn run(&self, input: &Value, ctx: &ToolCtx) -> Result<ToolOutput, ToolFailure> {
        let a: WriteArgs = parse(input).map_err(ToolFailure::new)?;
        let path = ctx.resolve(&a.path);
        if path.is_dir() {
            return Err(ToolFailure::new(format!(
                "{} is a directory",
                path.display()
            )));
        }
        let before = fs::metadata(&path).ok().map(|m| m.len());
        let old = crate::change::old_text(&path);
        write_atomic(&path, a.content.as_bytes(), ctx.umask)?;
        crate::fs_stale::wrote(ctx, &path);
        let name = crate::change::shown(&path, &ctx.cwd);
        let diff = (before.is_none() || old.is_some()).then(|| {
            let d = crate::change::file_diff(&name, old.as_deref(), &a.content);
            crate::change::meta(&crate::change::tool_diff(name.clone(), &d))
        });
        let text = match before {
            Some(b) => format!(
                "Replaced {} ({b} bytes → {} bytes).",
                path.display(),
                a.content.len()
            ),
            None => format!("Created {} ({} bytes).", path.display(), a.content.len()),
        };
        Ok(ToolOutput {
            text,
            meta: json!({"path": path, "bytes": a.content.len(), "created": before.is_none(), "bytes_before": before, "diff": diff}),
        })
    }
}

// ---------------------------------------------------------------- fs.edit

pub struct Edit;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditArgs {
    #[serde(alias = "file", alias = "filename", alias = "file_path")]
    path: String,
    #[serde(alias = "old_str", alias = "old_text")]
    old_string: String,
    #[serde(alias = "new_str", alias = "new_text")]
    new_string: String,
    #[serde(default)]
    replace_all: bool,
}

impl Tool for Edit {
    fn name(&self) -> &'static str {
        "fs.edit"
    }
    fn description(&self) -> &'static str {
        "Replace an exact string in a file. old_string must match the file exactly, including indentation (a quoted newline matches either line ending), and must be unique unless replace_all is true. Read the file first. Returns a diff of the change."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File path, absolute or relative to the working directory."},
                "old_string": {"type": "string", "description": "Exact text to replace (must occur exactly once unless replace_all)."},
                "new_string": {"type": "string", "description": "Replacement text."},
                "replace_all": {"type": "boolean", "description": "Replace every occurrence. Default false."}
            },
            "required": ["path", "old_string", "new_string"],
            "additionalProperties": false
        })
    }
    fn class(&self) -> ToolClass {
        ToolClass::Write
    }
    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }
    fn plan(&self, input: &Value, ctx: &ToolCtx) -> Result<Plan, String> {
        let a: EditArgs = parse(input)?;
        if a.old_string.is_empty() {
            return Err("old_string must not be empty (use fs_write to create a file)".into());
        }
        // An identical pair is not invalid input: it is usually a retry of an
        // edit that already landed. `run` answers it as a no-op.
        let path = ctx.resolve(&a.path);
        Ok(Plan {
            summary: format!(
                "edit {}{}",
                path.display(),
                if a.replace_all {
                    " (all occurrences)"
                } else {
                    ""
                }
            ),
            resources: vec![Resource {
                path,
                access: Access::Write,
            }],
            argv: None,
            url: None,
            ..Default::default()
        })
    }
    fn run(&self, input: &Value, ctx: &ToolCtx) -> Result<ToolOutput, ToolFailure> {
        let a: EditArgs = parse(input).map_err(ToolFailure::new)?;
        let path = ctx.resolve(&a.path);
        crate::fs_stale::fresh(ctx, &path)?;
        let before = read_regular_text(&path, "fs_edit")?;
        // `fs.read` shows a line without its `\r`, so a model quotes bare
        // `\n`. When the text as given is not in a file that has CRLF endings,
        // match the quote with each quoted newline reading either ending
        // (`lf_regions`), and write each replacement with its region's own
        // endings (`splice_lf_regions`): only the edited text changes.
        let regions = match before.contains("\r\n") && !before.contains(a.old_string.as_str()) {
            true => Some(lf_regions(&before, &a.old_string)),
            false => None,
        };
        let n = match &regions {
            Some(r) => r.len(),
            None => before.matches(a.old_string.as_str()).count(),
        };
        if n == 0 {
            return Err(ToolFailure::new(format!(
                "old_string was not found in {}. Read the file again and copy the exact text, including whitespace and indentation.",
                path.display()
            )));
        }
        if a.old_string == a.new_string {
            return Ok(ToolOutput {
                text: format!(
                    "No change made: the replacement is identical to the text already in {}. If you meant to change it, send a new_string that differs from old_string.",
                    path.display()
                ),
                meta: json!({"path": path, "replacements": 0, "bytes_before": before.len(), "bytes_after": before.len()}),
            });
        }
        if n > 1 && !a.replace_all {
            return Err(ToolFailure::new(format!(
                "old_string occurs {n} times in {}. Add surrounding lines to make it unique, or set replace_all.",
                path.display()
            )));
        }
        let after = match regions {
            Some(mut r) => {
                r.truncate(if a.replace_all { n } else { 1 });
                splice_lf_regions(&before, &r, &a.new_string)
            }
            None if a.replace_all => before.replace(a.old_string.as_str(), &a.new_string),
            None => before.replacen(a.old_string.as_str(), &a.new_string, 1),
        };
        write_atomic(&path, after.as_bytes(), ctx.umask)?;
        crate::fs_stale::wrote(ctx, &path);
        let name = path.display().to_string();
        let diff = unified(&before, &after, &name, &name, 3);
        let shown = crate::change::shown(&path, &ctx.cwd);
        Ok(ToolOutput {
            text: format!(
                "Replaced {n} occurrence{} in {name}.\n{diff}",
                if n == 1 { "" } else { "s" }
            ),
            meta: json!({"path": path, "replacements": n, "bytes_before": before.len(), "bytes_after": after.len(), "diff": crate::change::meta(&crate::change::tool_diff(shown.clone(), &crate::change::file_diff(&shown, Some(&before), &after)))}),
        })
    }
}

// ---------------------------------------------------------------- fs.patch

pub struct Patch;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PatchArgs {
    #[serde(alias = "diff")]
    patch: String,
}

/// One file's section of a multi-file unified diff.
struct FilePatch {
    old: Option<String>,
    new: Option<String>,
    text: String,
}

fn strip_prefix(p: &str) -> Option<String> {
    let p = p.split('\t').next().unwrap_or(p).trim();
    if p == "/dev/null" {
        return None;
    }
    let p = p
        .strip_prefix("a/")
        .or_else(|| p.strip_prefix("b/"))
        .unwrap_or(p);
    Some(p.to_string())
}

fn split_patch(patch: &str) -> Result<Vec<FilePatch>, String> {
    let lines: Vec<&str> = patch.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].starts_with("--- ") && i + 1 < lines.len() && lines[i + 1].starts_with("+++ ") {
            let old = strip_prefix(&lines[i][4..]);
            let new = strip_prefix(&lines[i + 1][4..]);
            let start = i;
            i += 2;
            while i < lines.len()
                && !(lines[i].starts_with("--- ")
                    && i + 1 < lines.len()
                    && lines[i + 1].starts_with("+++ "))
            {
                if lines[i].starts_with("diff --git ") {
                    break;
                }
                i += 1;
            }
            let mut text = lines[start..i].join("\n");
            text.push('\n');
            out.push(FilePatch { old, new, text });
        } else {
            i += 1;
        }
    }
    if out.is_empty() {
        return Err("no file sections found: a patch needs `--- a/path` / `+++ b/path` headers and @@ hunks".into());
    }
    Ok(out)
}

impl Tool for Patch {
    fn name(&self) -> &'static str {
        "fs.patch"
    }
    fn description(&self) -> &'static str {
        "Apply a unified diff (one or more files, `--- a/path` / `+++ b/path` headers, @@ hunks). All files apply or none do. Use for multi-hunk or multi-file changes; use fs_edit for a single replacement."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "patch": {"type": "string", "description": "Unified diff text. Paths are relative to the working directory (a/ and b/ prefixes are stripped); /dev/null creates or deletes."}
            },
            "required": ["patch"],
            "additionalProperties": false
        })
    }
    fn class(&self) -> ToolClass {
        ToolClass::Write
    }
    fn retry(&self) -> Retry {
        Retry::NonRepeatable
    }
    fn plan(&self, input: &Value, ctx: &ToolCtx) -> Result<Plan, String> {
        let a: PatchArgs = parse(input)?;
        let files = split_patch(&a.patch)?;
        let mut resources = Vec::new();
        let mut names = Vec::new();
        for f in &files {
            for p in [&f.old, &f.new].into_iter().flatten() {
                let path = ctx.resolve(p);
                if !resources.iter().any(|r: &Resource| r.path == path) {
                    names.push(path.display().to_string());
                    resources.push(Resource {
                        path,
                        access: Access::Write,
                    });
                }
            }
        }
        Ok(Plan {
            summary: format!(
                "patch {} file{}: {}",
                names.len(),
                if names.len() == 1 { "" } else { "s" },
                names.join(", ")
            ),
            resources,
            argv: None,
            url: None,
            ..Default::default()
        })
    }
    fn run(&self, input: &Value, ctx: &ToolCtx) -> Result<ToolOutput, ToolFailure> {
        let a: PatchArgs = parse(input).map_err(ToolFailure::new)?;
        let files = split_patch(&a.patch).map_err(ToolFailure::new)?;
        // Compute every result first; write only if all apply.
        let mut results: Vec<(PathBuf, Option<String>, String)> = Vec::new();
        let mut diffs: Vec<(String, String)> = Vec::new();
        let mut recounted = Vec::new();
        for f in &files {
            let target = f
                .new
                .as_ref()
                .or(f.old.as_ref())
                .ok_or_else(|| ToolFailure::new("a section has /dev/null on both sides"))?;
            let path = ctx.resolve(target);
            crate::fs_stale::fresh(ctx, &path)?;
            let original = match &f.old {
                None => String::new(),
                Some(o) => read_regular_text(&ctx.resolve(o), "fs_patch")?,
            };
            // Each hunk header's lengths from its body (theseus-inw).
            let text = crate::recount::noted(&f.text, target, &mut recounted);
            let p = diffy::Patch::from_str(&text).map_err(|e| {
                ToolFailure::new(format!("cannot parse the section for {target}: {e}"))
            })?;
            // A file whose endings are all CRLF is patched as the read showed
            // it, and written back with CRLF endings.
            let crlf = is_all_crlf(&original);
            let basis = if crlf {
                original.replace("\r\n", "\n")
            } else {
                original.clone()
            };
            let applied = diffy::apply(&basis, &p).map_err(|e| ToolFailure::new(format!("the patch does not apply to {target}: {e}. Read the file and regenerate the hunk against its current contents.")))?;
            let applied = if crlf { to_crlf(&applied) } else { applied };
            // A deletion carries the whole file, so the call records what it
            // removed and stays reversible.
            if f.new.is_none() && !applied.is_empty() {
                return Err(ToolFailure::new(format!(
                    "the section deleting {target} leaves {} line(s) the patch does not show: a deletion must remove every line. Read the file and include all of it.",
                    applied.lines().count()
                )));
            }
            let shown = crate::change::shown(&path, &ctx.cwd);
            let before = (f.old.is_some()).then_some(original.as_str());
            diffs.push((
                shown.clone(),
                crate::change::file_diff(&shown, before, &applied),
            ));
            let stat = {
                let d = similar::TextDiff::from_lines(&original, &applied);
                let (mut add, mut del) = (0, 0);
                for c in d.iter_all_changes() {
                    match c.tag() {
                        similar::ChangeTag::Insert => add += 1,
                        similar::ChangeTag::Delete => del += 1,
                        _ => {}
                    }
                }
                format!("+{add} -{del}")
            };
            results.push((
                path,
                if f.new.is_none() { None } else { Some(applied) },
                stat,
            ));
        }
        let mut summary = Vec::new();
        for (path, content, stat) in &results {
            match content {
                Some(c) => {
                    write_atomic(path, c.as_bytes(), ctx.umask)?;
                    crate::fs_stale::wrote(ctx, path);
                }
                None => fs::remove_file(path)?,
            }
            summary.push(format!(
                "{} {stat}{}",
                path.display(),
                if content.is_none() { " (deleted)" } else { "" }
            ));
        }
        summary.append(&mut recounted);
        Ok(ToolOutput {
            text: format!(
                "Applied to {} file{}:\n{}",
                results.len(),
                if results.len() == 1 { "" } else { "s" },
                summary.join("\n")
            ),
            meta: json!({"files": results.iter().map(|(p, c, s)| json!({"path": p, "stat": s, "deleted": c.is_none()})).collect::<Vec<_>>(), "diff": crate::change::meta(&crate::change::combine(&diffs))}),
        })
    }
}

// ---------------------------------------------------------------- fs.glob

pub struct Glob;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GlobArgs {
    pattern: String,
    #[serde(default, alias = "file", alias = "filename", alias = "file_path")]
    path: Option<String>,
    #[serde(default)]
    all: bool,
}

impl Tool for Glob {
    fn name(&self) -> &'static str {
        "fs.glob"
    }
    fn description(&self) -> &'static str {
        "Find files by glob pattern (e.g. `**/*.rs`, `src/**/test_*.py`), newest first. Respects .gitignore; skips hidden files and .git unless all is true (then only .git is skipped)."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string", "description": "Glob relative to path, e.g. **/*.rs"},
                "path": {"type": "string", "description": "Directory to search. Default: the working directory."},
                "all": {"type": "boolean", "description": "Include hidden and ignored entries. Default false."}
            },
            "required": ["pattern"],
            "additionalProperties": false
        })
    }
    fn class(&self) -> ToolClass {
        ToolClass::Read
    }
    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }
    fn plan(&self, input: &Value, ctx: &ToolCtx) -> Result<Plan, String> {
        let a: GlobArgs = parse(input)?;
        globset::Glob::new(&a.pattern).map_err(|e| format!("invalid glob: {e}"))?;
        let base = a
            .path
            .as_deref()
            .map(|p| ctx.resolve(p))
            .unwrap_or_else(|| ctx.cwd.clone());
        Ok(Plan {
            summary: format!("glob {} in {}", a.pattern, base.display()),
            resources: vec![Resource {
                path: base,
                access: Access::Read,
            }],
            argv: None,
            url: None,
            ..Default::default()
        })
    }
    fn run(&self, input: &Value, ctx: &ToolCtx) -> Result<ToolOutput, ToolFailure> {
        let a: GlobArgs = parse(input).map_err(ToolFailure::new)?;
        let base = a
            .path
            .as_deref()
            .map(|p| ctx.resolve(p))
            .unwrap_or_else(|| ctx.cwd.clone());
        let matcher = globset::GlobBuilder::new(&a.pattern)
            .literal_separator(true)
            .build()?
            .compile_matcher();
        let mut hits: Vec<(SystemTime, PathBuf)> = Vec::new();
        let mut scanned = 0usize;
        for e in walker(&base, a.all, None).flatten() {
            if !e.file_type().map(|t| t.is_file()).unwrap_or(false) {
                continue;
            }
            scanned += 1;
            let rel = e.path().strip_prefix(&base).unwrap_or(e.path());
            if matcher.is_match(rel) {
                let mt = e
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                hits.push((mt, e.path().to_path_buf()));
            }
        }
        hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        let total = hits.len();
        let shown: Vec<String> = hits
            .iter()
            .take(ctx.max_entries)
            .map(|(_, p)| p.display().to_string())
            .collect();
        // Every result states its scope and what it left out (Appendix F,
        // theseus-8ye): which files, where, in what order, the oldest past
        // the listing's length, and what the walk never sees.
        let left = total - shown.len();
        let text = if shown.is_empty() {
            format!(
                "No files match {} under {} ({} files looked at); {}.",
                a.pattern,
                base.display(),
                scanned,
                not_walked(a.all)
            )
        } else {
            let listed = match left {
                0 => format!("{} matching {}", count(total, "file", "files"), a.pattern),
                _ => format!("{} of {total} files matching {}", shown.len(), a.pattern),
            };
            let past = match left {
                0 => String::new(),
                n => format!("; the {n} oldest not shown: a narrower pattern or path returns them"),
            };
            format!(
                "{}\n[{listed} under {}, newest first{past}; {}]",
                shown.join("\n"),
                base.display(),
                not_walked(a.all)
            )
        };
        Ok(ToolOutput {
            text,
            meta: json!({"base": base, "matches": total, "shown": shown.len(), "scanned": scanned}),
        })
    }
    fn rest(&self, _left_out: &str) -> String {
        "a narrower pattern or path returns them".into()
    }
}

// ---------------------------------------------------------------- fs.grep

pub struct Grep;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GrepArgs {
    pattern: String,
    #[serde(default, alias = "file", alias = "filename", alias = "file_path")]
    path: Option<String>,
    #[serde(default)]
    glob: Option<String>,
    #[serde(default)]
    case_insensitive: bool,
    #[serde(default)]
    output_mode: Option<String>,
    #[serde(default)]
    context: Option<usize>,
    #[serde(default)]
    max_results: Option<usize>,
    #[serde(default)]
    all: bool,
}

struct Collect<'a> {
    path: &'a str,
    out: &'a mut Vec<String>,
    hits: &'a mut usize,
    max: usize,
    last_line: Option<u64>,
}

impl grep_searcher::Sink for Collect<'_> {
    type Error = std::io::Error;
    fn matched(
        &mut self,
        _s: &grep_searcher::Searcher,
        m: &grep_searcher::SinkMatch<'_>,
    ) -> Result<bool, Self::Error> {
        if *self.hits >= self.max {
            return Ok(false);
        }
        let n = m.line_number().unwrap_or(0);
        if let Some(l) = self.last_line {
            if n > l + 1 {
                self.out.push("--".into());
            }
        }
        let line = String::from_utf8_lossy(m.bytes());
        let line: String = line
            .trim_end_matches(['\n', '\r'])
            .chars()
            .take(MAX_LINE_CHARS)
            .collect();
        self.out.push(format!("{}:{}:{}", self.path, n, line));
        self.last_line = Some(n);
        *self.hits += 1;
        Ok(true)
    }
    fn context(
        &mut self,
        _s: &grep_searcher::Searcher,
        c: &grep_searcher::SinkContext<'_>,
    ) -> Result<bool, Self::Error> {
        let n = c.line_number().unwrap_or(0);
        if let Some(l) = self.last_line {
            if n > l + 1 {
                self.out.push("--".into());
            }
        }
        let line = String::from_utf8_lossy(c.bytes());
        let line: String = line
            .trim_end_matches(['\n', '\r'])
            .chars()
            .take(MAX_LINE_CHARS)
            .collect();
        self.out.push(format!("{}-{}-{}", self.path, n, line));
        self.last_line = Some(n);
        Ok(true)
    }
}

impl Tool for Grep {
    fn name(&self) -> &'static str {
        "fs.grep"
    }
    fn description(&self) -> &'static str {
        "Search file contents with a regular expression (Rust regex syntax), ripgrep-style: respects .gitignore and skips hidden files unless all is true (.git is never searched), skips binary files. output_mode: content (path:line:text, default), files (paths only), or count."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string", "description": "Regular expression."},
                "path": {"type": "string", "description": "File or directory to search. Default: the working directory."},
                "glob": {"type": "string", "description": "Only files matching this glob, e.g. *.rs"},
                "case_insensitive": {"type": "boolean"},
                "output_mode": {"type": "string", "enum": ["content", "files", "count"]},
                "context": {"type": "integer", "minimum": 0, "maximum": 10, "description": "Lines of context around each match (content mode)."},
                "max_results": {"type": "integer", "minimum": 1, "description": "Cap on matching lines (content) or files. Default 200."},
                "all": {"type": "boolean", "description": "Include hidden and ignored entries. Default false."}
            },
            "required": ["pattern"],
            "additionalProperties": false
        })
    }
    fn class(&self) -> ToolClass {
        ToolClass::Read
    }
    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }
    fn plan(&self, input: &Value, ctx: &ToolCtx) -> Result<Plan, String> {
        let a: GrepArgs = parse(input)?;
        regex::Regex::new(&a.pattern).map_err(|e| format!("invalid regex: {e}"))?;
        if let Some(g) = &a.glob {
            globset::Glob::new(g).map_err(|e| format!("invalid glob: {e}"))?;
        }
        if let Some(m) = &a.output_mode {
            if !["content", "files", "count"].contains(&m.as_str()) {
                return Err(format!("output_mode {m:?} is not content, files, or count"));
            }
        }
        let base = a
            .path
            .as_deref()
            .map(|p| ctx.resolve(p))
            .unwrap_or_else(|| ctx.cwd.clone());
        Ok(Plan {
            summary: format!("grep /{}/ in {}", a.pattern, base.display()),
            resources: vec![Resource {
                path: base,
                access: Access::Read,
            }],
            argv: None,
            url: None,
            ..Default::default()
        })
    }
    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
    fn run(&self, input: &Value, ctx: &ToolCtx) -> Result<ToolOutput, ToolFailure> {
        let a: GrepArgs = parse(input).map_err(ToolFailure::new)?;
        let base = a
            .path
            .as_deref()
            .map(|p| ctx.resolve(p))
            .unwrap_or_else(|| ctx.cwd.clone());
        let matcher = grep_regex::RegexMatcherBuilder::new()
            .case_insensitive(a.case_insensitive)
            .build(&a.pattern)?;
        let glob = match &a.glob {
            Some(g) => Some(globset::Glob::new(g)?.compile_matcher()),
            None => None,
        };
        let mode = a.output_mode.clone().unwrap_or_else(|| "content".into());
        let max = a
            .max_results
            .unwrap_or(200)
            .min(ctx.max_entries.max(200) * 10);
        let ctxl = a.context.unwrap_or(0).min(10);
        let s = Search {
            matcher,
            content: mode == "content",
            context: ctxl,
        };
        let files: Vec<PathBuf> = if base.is_file() {
            vec![base.clone()]
        } else {
            walker(&base, a.all, None)
                .flatten()
                .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
                .map(|e| e.into_path())
                .collect()
        };
        let files: Vec<PathBuf> = files
            .into_iter()
            .filter(|f| {
                glob.as_ref().is_none_or(|g| {
                    let name = f.file_name().map(PathBuf::from).unwrap_or_default();
                    let rel = f.strip_prefix(&base).unwrap_or(f);
                    g.is_match(&name) || g.is_match(rel)
                })
            })
            .collect();
        let mut searcher = s.searcher();
        let mut ahead: Option<Vec<Option<Found>>> = None;
        let mut out: Vec<String> = Vec::new();
        let mut hits = 0usize;
        let mut files_hit: Vec<(String, usize)> = Vec::new();
        let mut scanned = 0usize;
        let mut searched = 0usize;
        for (i, f) in files.iter().enumerate() {
            scanned += 1;
            let cap = if s.content {
                max.saturating_sub(hits)
            } else {
                usize::MAX
            };
            if cap == 0 {
                break;
            }
            searched += 1;
            // Past the first files, searched ahead on the free cores
            // (theseus-a60), with the whole cap: the same as a search with
            // this one, unless it found more than this one allows.
            let early = match (i.checked_sub(INLINE_FILES), &ctx.cores) {
                (Some(j), Some(cores)) => ahead
                    .get_or_insert_with(|| search_ahead(&files[INLINE_FILES..], &s, max, cores))[j]
                    .take(),
                _ => None,
            };
            let found = match early {
                Some(Some((lines, n))) if n <= cap => Some((lines, n)),
                Some(None) => None,
                _ => s.search(&mut searcher, f, cap),
            };
            let Some((local, local_hits)) = found else {
                continue;
            };
            if local_hits > 0 {
                hits += local_hits;
                files_hit.push((f.display().to_string(), local_hits));
                if s.content {
                    if !out.is_empty() && ctxl > 0 {
                        out.push("--".into());
                    }
                    out.extend(local);
                }
            }
            if !s.content && files_hit.len() >= max {
                break;
            }
        }
        let text = match mode.as_str() {
            "files" => files_hit
                .iter()
                .map(|(p, _)| p.clone())
                .collect::<Vec<_>>()
                .join("\n"),
            "count" => files_hit
                .iter()
                .map(|(p, n)| format!("{p}:{n}"))
                .collect::<Vec<_>>()
                .join("\n"),
            _ => out.join("\n"),
        };
        // Every result states its scope and what it left out (Appendix F,
        // theseus-8ye): how many lines or files, out of how many files
        // searched, where; and, when the cap stopped the search, that more
        // may match, where it stopped, and how many files it never opened.
        let capped = if s.content {
            hits >= max
        } else {
            files_hit.len() >= max && searched < files.len()
        };
        let unsearched = files.len() - searched;
        let only = a
            .glob
            .as_deref()
            .map(|g| format!(" matching {g}"))
            .unwrap_or_default();
        // `n` files of the search's scope: `4 files matching *.rs under /w`.
        let files_in = |n: usize| {
            format!(
                "{}{only} under {}",
                count(n, "file", "files"),
                base.display()
            )
        };
        let one = base.is_file();
        let walked = if one {
            String::new()
        } else {
            format!("; {}", not_walked(a.all))
        };
        let text = if files_hit.is_empty() {
            match one {
                true => format!("No matches for /{}/ in {}.", a.pattern, base.display()),
                false => format!(
                    "No matches for /{}/; {} searched{walked}.",
                    a.pattern,
                    files_in(searched)
                ),
            }
        } else {
            let found = match (s.content, mode.as_str()) {
                (true, _) => count(hits, "matching line", "matching lines"),
                (false, "count") => format!(
                    "{} ({})",
                    count(files_hit.len(), "file with matches", "files with matches"),
                    count(hits, "matching line", "matching lines")
                ),
                _ => count(files_hit.len(), "file with matches", "files with matches"),
            };
            let line = match (capped, s.content, one) {
                (false, _, true) => format!("[{found} in {}]", base.display()),
                (false, true, false) => format!(
                    "[{found} in {}; {} searched{walked}]",
                    count(files_hit.len(), "file", "files"),
                    files_in(searched)
                ),
                (false, false, false) => format!("[{found}; {} searched{walked}]", files_in(searched)),
                (true, _, true) => format!(
                    "[stopped at max_results={max}: the first {max} matching lines in {}; its later \
                     lines were not searched, so more may match: a larger max_results returns them]",
                    base.display()
                ),
                (true, content, false) => {
                    // Where the cap fell: in the last file shown, which may
                    // have more lines past it, or may not (a cap that falls on
                    // a file's last match cannot tell).
                    let first = match (content, files_hit.last()) {
                        (true, Some((p, _))) => format!(
                            "the first {max} matching lines, from {}, the last in {p}",
                            count(files_hit.len(), "file", "files")
                        ),
                        _ => format!("the first {max} files with matches"),
                    };
                    format!(
                        "[stopped at max_results={max}: {first}; {unsearched} more of {} not \
                         searched, so more may match: a narrower search, or a larger max_results, \
                         returns them{walked}]",
                        files_in(files.len())
                    )
                }
            };
            format!("{text}\n{line}")
        };
        Ok(ToolOutput {
            text,
            meta: json!({"base": base, "files_with_matches": files_hit.len(), "matching_lines": hits, "files_scanned": scanned, "files_searched": searched, "files_total": files.len(), "capped": capped}),
        })
    }
    fn rest(&self, _left_out: &str) -> String {
        "a narrower search returns them: a more specific pattern, a path or glob, fewer \
         context lines, or output_mode files or count"
            .into()
    }
}

/// The files `fs.grep` searches on its own thread before it borrows cores,
/// so a search that stops early (200 matches in the first files) does no
/// work it throws away, and a small tree none at all.
pub(crate) const INLINE_FILES: usize = 256;
/// The files a core takes at a time.
const CHUNK_FILES: usize = 64;

/// One file's matching lines and how many matched; `None` when it could not
/// be searched.
type Found = Option<(Vec<String>, usize)>;

/// A search's settings, cloned onto every core it borrows.
#[derive(Clone)]
struct Search {
    matcher: grep_regex::RegexMatcher,
    content: bool,
    context: usize,
}

impl Search {
    fn searcher(&self) -> grep_searcher::Searcher {
        let context = if self.content { self.context } else { 0 };
        grep_searcher::SearcherBuilder::new()
            .line_number(true)
            .binary_detection(grep_searcher::BinaryDetection::quit(0))
            .before_context(context)
            .after_context(context)
            .build()
    }

    /// One file, stopping after `cap` matching lines.
    fn search(&self, searcher: &mut grep_searcher::Searcher, f: &Path, cap: usize) -> Found {
        let shown = f.display().to_string();
        let (mut lines, mut hits) = (Vec::new(), 0usize);
        let mut sink = Collect {
            path: &shown,
            out: &mut lines,
            hits: &mut hits,
            max: cap,
            last_line: None,
        };
        // A regular file only, even one swapped in since the walk looked
        // (review 2's R9).
        let (file, _) = open_regular(f, true).ok()?;
        searcher.search_file(&self.matcher, &file, &mut sink).ok()?;
        Some((lines, hits))
    }
}

/// A search of many files ahead of the merge, shared by this thread and the
/// cores it borrowed.
struct Ahead {
    files: Vec<PathBuf>,
    search: Search,
    max: usize,
    /// The next chunk to hand out: chunks go in walk order.
    next: AtomicUsize,
    /// Matching lines (files, outside content mode) in the chunks searched.
    found: AtomicUsize,
    stop: AtomicBool,
    chunks: Mutex<Vec<Option<Vec<Found>>>>,
    filled: Condvar,
}

impl Ahead {
    fn work(&self) {
        let mut searcher = self.search.searcher();
        let cap = if self.search.content {
            self.max
        } else {
            usize::MAX
        };
        while !self.stop.load(Ordering::Relaxed) {
            let c = self.next.fetch_add(1, Ordering::Relaxed);
            let Some(files) = self.files.chunks(CHUNK_FILES).nth(c) else {
                break;
            };
            let found: Vec<Found> = files
                .iter()
                .map(|f| self.search.search(&mut searcher, f, cap))
                .collect();
            let n: usize = found
                .iter()
                .flatten()
                .map(|(_, hits)| match self.search.content {
                    true => *hits,
                    false => usize::from(*hits > 0),
                })
                .sum();
            // The chunks handed out are a prefix of the walk, so once they
            // hold the cap, the merge stops inside them.
            if self.found.fetch_add(n, Ordering::Relaxed) + n >= self.max {
                self.stop.store(true, Ordering::Relaxed);
            }
            self.chunks.lock().unwrap()[c] = Some(found);
            self.filled.notify_all();
        }
    }
}

/// Search `files` in chunks, on this thread and on every core free now,
/// each file with the whole cap, until the chunks searched hold it. A file
/// searched ahead is `Some`; the merge searches any other itself.
fn search_ahead(
    files: &[PathBuf],
    s: &Search,
    max: usize,
    cores: &Arc<dyn Cores>,
) -> Vec<Option<Found>> {
    let n = files.len().div_ceil(CHUNK_FILES);
    let a = Arc::new(Ahead {
        files: files.to_vec(),
        search: s.clone(),
        max,
        next: AtomicUsize::new(0),
        found: AtomicUsize::new(0),
        stop: AtomicBool::new(false),
        chunks: Mutex::new(vec![None; n]),
        filled: Condvar::new(),
    });
    for _ in 1..n {
        let helper = a.clone();
        if !cores.try_spawn(Box::new(move || helper.work())) {
            break;
        }
    }
    a.work();
    // Every chunk handed out so far is being searched: wait for them.
    let handed = a.next.load(Ordering::Relaxed).min(n);
    let mut chunks = a.chunks.lock().unwrap();
    while chunks[..handed].iter().any(Option::is_none) {
        chunks = a.filled.wait(chunks).unwrap();
    }
    let mut out = Vec::with_capacity(files.len());
    for (c, chunk) in chunks.iter_mut().enumerate() {
        let len = CHUNK_FILES.min(files.len() - c * CHUNK_FILES);
        match chunk.take() {
            Some(found) => out.extend(found.into_iter().map(Some)),
            None => out.extend((0..len).map(|_| None)),
        }
    }
    out
}

// ---------------------------------------------------------------- fs.list

pub struct List;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListArgs {
    #[serde(default, alias = "file", alias = "filename", alias = "file_path")]
    path: Option<String>,
    #[serde(default)]
    depth: Option<usize>,
    #[serde(default)]
    all: bool,
}

/// What `fs.list`'s walk found: the entries kept, how many there were in all,
/// the directories at the last level, whose contents it never reads, and the
/// first entry past its length, in path order (theseus-8ye).
struct ListWalk {
    entries: Vec<(PathBuf, bool, u64)>,
    total: usize,
    undescended: usize,
    first_left: Option<PathBuf>,
}

fn list_walk(base: &Path, all: bool, depth: usize, max_entries: usize) -> ListWalk {
    let mut w = ListWalk {
        entries: Vec::new(),
        total: 0,
        undescended: 0,
        first_left: None,
    };
    let mut b = walk_builder(base, all, Some(depth));
    b.sort_by_file_path(|x, y| x.cmp(y));
    for e in b.build().flatten() {
        if e.path() == base {
            continue;
        }
        w.total += 1;
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir && e.depth() == depth {
            w.undescended += 1;
        }
        if w.entries.len() < max_entries {
            let size = if is_dir {
                0
            } else {
                e.metadata().map(|m| m.len()).unwrap_or(0)
            };
            w.entries.push((e.path().to_path_buf(), is_dir, size));
        } else if w.first_left.is_none() {
            w.first_left = Some(e.path().to_path_buf());
        }
    }
    w
}

impl Tool for List {
    fn name(&self) -> &'static str {
        "fs.list"
    }
    fn description(&self) -> &'static str {
        "List a directory as a tree with sizes (default depth 2). Respects .gitignore and hides dotfiles unless all is true. Use it to orient in an unfamiliar repository."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Directory. Default: the working directory."},
                "depth": {"type": "integer", "minimum": 1, "maximum": 6, "description": "Levels to descend. Default 2."},
                "all": {"type": "boolean", "description": "Include hidden and ignored entries. Default false."}
            },
            "additionalProperties": false
        })
    }
    fn class(&self) -> ToolClass {
        ToolClass::Read
    }
    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }
    fn plan(&self, input: &Value, ctx: &ToolCtx) -> Result<Plan, String> {
        let a: ListArgs = parse(input)?;
        let base = a
            .path
            .as_deref()
            .map(|p| ctx.resolve(p))
            .unwrap_or_else(|| ctx.cwd.clone());
        Ok(Plan {
            summary: format!("list {}", base.display()),
            resources: vec![Resource {
                path: base,
                access: Access::Read,
            }],
            argv: None,
            url: None,
            ..Default::default()
        })
    }
    fn run(&self, input: &Value, ctx: &ToolCtx) -> Result<ToolOutput, ToolFailure> {
        let a: ListArgs = parse(input).map_err(ToolFailure::new)?;
        let base = a
            .path
            .as_deref()
            .map(|p| ctx.resolve(p))
            .unwrap_or_else(|| ctx.cwd.clone());
        if !base.is_dir() {
            return Err(ToolFailure::new(format!(
                "{} is not a directory",
                base.display()
            )));
        }
        let depth = a.depth.unwrap_or(2).clamp(1, 6);
        let ListWalk {
            entries,
            total,
            undescended,
            first_left,
        } = list_walk(&base, a.all, depth, ctx.max_entries);
        let mut text = format!("{}/\n", base.display());
        for (p, is_dir, size) in &entries {
            let rel = p.strip_prefix(&base).unwrap_or(p);
            let level = rel.components().count().saturating_sub(1);
            let name = rel
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if *is_dir {
                text.push_str(&format!("{}{}/\n", "  ".repeat(level + 1), name));
            } else {
                text.push_str(&format!(
                    "{}{} ({})\n",
                    "  ".repeat(level + 1),
                    name,
                    human(*size)
                ));
            }
        }
        // Every result states its scope and what it left out (Appendix F,
        // theseus-8ye): the entries past its length, from where in path
        // order; the directories whose contents are below its depth; and the
        // hidden and ignored entries, unless `all` shows them.
        let listed = match first_left {
            None => format!(
                "{} under {} to depth {depth}",
                count(total, "entry", "entries"),
                base.display()
            ),
            Some(p) => format!(
                "{} of {total} entries under {} to depth {depth}, in path order; the rest, from {} \
                 on, not shown: fs_list of a subdirectory, or with a lower depth, returns them",
                entries.len(),
                base.display(),
                p.strip_prefix(&base).unwrap_or(&p).display()
            ),
        };
        let below =
            match undescended {
                0 => String::new(),
                n => {
                    format!(
                "; the contents of {} at depth {depth} not listed: fs_list of one{} lists them",
                count(n, "directory", "directories"),
                if depth < 6 { ", or a greater depth," } else { "" }
            )
                }
            };
        let hidden = match a.all {
            true => "",
            false => "; hidden and .gitignore'd entries not shown: all=true shows them",
        };
        text.push_str(&format!("[{listed}{below}{hidden}]\n"));
        Ok(ToolOutput {
            text,
            meta: json!({"base": base, "entries": total, "shown": entries.len(), "depth": depth, "undescended": undescended}),
        })
    }
    fn rest(&self, _left_out: &str) -> String {
        "fs_list of a subdirectory, or with a lower depth, returns them".into()
    }
}

fn human(n: u64) -> String {
    if n >= 1 << 20 {
        format!("{:.1} MB", n as f64 / (1 << 20) as f64)
    } else if n >= 1 << 10 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}
