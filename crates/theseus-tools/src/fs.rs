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

use crate::{
    image, parse, Access, Cores, ImageData, Plan, Resource, Retry, Tool, ToolClass, ToolCtx,
    ToolFailure, ToolOutput,
};

const MAX_LINE_CHARS: usize = 2000;
const DEFAULT_READ_LINES: usize = 2000;
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;

fn is_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8192).any(|b| *b == 0)
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
                 bytes are not read whole (use fs_grep to find the part you need)",
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
fn open_regular(path: &Path, follow: bool) -> Result<(fs::File, fs::Metadata), Unread> {
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

fn walker(base: &Path, hidden: bool, max_depth: Option<usize>) -> ignore::Walk {
    let mut b = ignore::WalkBuilder::new(base);
    b.hidden(!hidden)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(true)
        .parents(true)
        .require_git(false);
    if let Some(d) = max_depth {
        b.max_depth(Some(d));
    }
    // Never descend into VCS internals even when hidden files are shown.
    b.filter_entry(|e| e.file_name() != ".git");
    b.build()
}

/// What `fs.glob` and `fs.grep` never walk, which their results state as part
/// of their scope (Appendix F's rule, theseus-8ye): an empty or narrow result
/// must not read as "there is none".
const NOT_WALKED: &str = "hidden files and .gitignore'd paths are not searched";

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
    path: String,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
}

impl Tool for Read {
    fn name(&self) -> &'static str {
        "fs.read"
    }
    fn description(&self) -> &'static str {
        "Read a text file, returned with line numbers (`   12\\tline`). Use it before editing a file and whenever you need a file's current contents. Pass offset/limit to page through long files. Binary files are reported, not returned; an image (PNG, JPEG, GIF, WebP) is returned as an image."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File path, absolute or relative to the working directory."},
                "offset": {"type": "integer", "minimum": 1, "description": "First line to return (1-based). Default 1."},
                "limit": {"type": "integer", "minimum": 1, "description": "Number of lines to return. Default 2000."}
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
        self.run_with_image(input, ctx).map(|(o, _)| o)
    }
    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
    fn run_with_image(
        &self,
        input: &Value,
        ctx: &ToolCtx,
    ) -> Result<(ToolOutput, Option<ImageData>), ToolFailure> {
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
        if meta.len() > MAX_FILE_BYTES {
            return Err(ToolFailure::new(format!(
                "{} is {} bytes; files over {} bytes are not read whole (use fs_grep to find the part you need)",
                path.display(),
                meta.len(),
                MAX_FILE_BYTES
            )));
        }
        let bytes = read_regular(&path, MAX_FILE_BYTES, true)
            .map_err(|e| ToolFailure::new(e.say(&path, "fs_read")))?;
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
                Some(ImageData { name, info, bytes }),
            ));
        }
        if is_binary(&bytes) {
            return Ok((
                ToolOutput {
                    text: format!(
                        "{} is a binary file ({} bytes); fs_read returns text only.",
                        path.display(),
                        bytes.len()
                    ),
                    meta: json!({"path": path, "bytes": bytes.len(), "binary": true}),
                },
                None,
            ));
        }
        let text = String::from_utf8_lossy(&bytes);
        let lines: Vec<&str> = text.lines().collect();
        let total = lines.len();
        let from = a.offset.unwrap_or(1).max(1);
        let limit = a.limit.unwrap_or(DEFAULT_READ_LINES);
        let mut out = String::new();
        let mut shown_bytes = 0usize;
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
            if shown_bytes + row.len() > ctx.max_read_bytes {
                break;
            }
            shown_bytes += row.len();
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
    path: String,
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
        write_atomic(&path, a.content.as_bytes(), ctx.umask)?;
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
            meta: json!({"path": path, "bytes": a.content.len(), "created": before.is_none(), "bytes_before": before}),
        })
    }
}

// ---------------------------------------------------------------- fs.edit

pub struct Edit;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditArgs {
    path: String,
    old_string: String,
    new_string: String,
    #[serde(default)]
    replace_all: bool,
}

impl Tool for Edit {
    fn name(&self) -> &'static str {
        "fs.edit"
    }
    fn description(&self) -> &'static str {
        "Replace an exact string in a file. old_string must match the file byte for byte, including indentation, and must be unique unless replace_all is true. Read the file first. Returns a diff of the change."
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
        if a.old_string == a.new_string {
            return Err("old_string and new_string are identical; nothing would change".into());
        }
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
        let before = read_regular_text(&path, "fs_edit")?;
        let n = before.matches(a.old_string.as_str()).count();
        if n == 0 {
            return Err(ToolFailure::new(format!(
                "old_string was not found in {}. Read the file again and copy the exact text, including whitespace and indentation.",
                path.display()
            )));
        }
        if n > 1 && !a.replace_all {
            return Err(ToolFailure::new(format!(
                "old_string occurs {n} times in {}. Add surrounding lines to make it unique, or set replace_all.",
                path.display()
            )));
        }
        let after = if a.replace_all {
            before.replace(a.old_string.as_str(), &a.new_string)
        } else {
            before.replacen(a.old_string.as_str(), &a.new_string, 1)
        };
        write_atomic(&path, after.as_bytes(), ctx.umask)?;
        let name = path.display().to_string();
        let diff = unified(&before, &after, &name, &name, 3);
        Ok(ToolOutput {
            text: format!(
                "Replaced {n} occurrence{} in {name}.\n{diff}",
                if n == 1 { "" } else { "s" }
            ),
            meta: json!({"path": path, "replacements": n, "bytes_before": before.len(), "bytes_after": after.len()}),
        })
    }
}

// ---------------------------------------------------------------- fs.patch

pub struct Patch;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PatchArgs {
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
        for f in &files {
            let target = f
                .new
                .as_ref()
                .or(f.old.as_ref())
                .ok_or_else(|| ToolFailure::new("a section has /dev/null on both sides"))?;
            let path = ctx.resolve(target);
            let original = match &f.old {
                None => String::new(),
                Some(o) => read_regular_text(&ctx.resolve(o), "fs_patch")?,
            };
            let p = diffy::Patch::from_str(&f.text).map_err(|e| {
                ToolFailure::new(format!("cannot parse the section for {target}: {e}"))
            })?;
            let applied = diffy::apply(&original, &p).map_err(|e| ToolFailure::new(format!("the patch does not apply to {target}: {e}. Read the file and regenerate the hunk against its current contents.")))?;
            // A deletion carries the whole file, so the call records what it
            // removed and stays reversible.
            if f.new.is_none() && !applied.is_empty() {
                return Err(ToolFailure::new(format!(
                    "the section deleting {target} leaves {} line(s) the patch does not show: a deletion must remove every line. Read the file and include all of it.",
                    applied.lines().count()
                )));
            }
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
                Some(c) => write_atomic(path, c.as_bytes(), ctx.umask)?,
                None => fs::remove_file(path)?,
            }
            summary.push(format!(
                "{} {stat}{}",
                path.display(),
                if content.is_none() { " (deleted)" } else { "" }
            ));
        }
        Ok(ToolOutput {
            text: format!(
                "Applied to {} file{}:\n{}",
                results.len(),
                if results.len() == 1 { "" } else { "s" },
                summary.join("\n")
            ),
            meta: json!({"files": results.iter().map(|(p, c, s)| json!({"path": p, "stat": s, "deleted": c.is_none()})).collect::<Vec<_>>()}),
        })
    }
}

// ---------------------------------------------------------------- fs.glob

pub struct Glob;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GlobArgs {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
}

impl Tool for Glob {
    fn name(&self) -> &'static str {
        "fs.glob"
    }
    fn description(&self) -> &'static str {
        "Find files by glob pattern (e.g. `**/*.rs`, `src/**/test_*.py`), newest first. Respects .gitignore; skips hidden files and .git."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string", "description": "Glob relative to path, e.g. **/*.rs"},
                "path": {"type": "string", "description": "Directory to search. Default: the working directory."}
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
        for e in walker(&base, false, None).flatten() {
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
                "No files match {} under {} ({} files looked at); {NOT_WALKED}.",
                a.pattern,
                base.display(),
                scanned
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
                "{}\n[{listed} under {}, newest first{past}; {NOT_WALKED}]",
                shown.join("\n"),
                base.display()
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
    #[serde(default)]
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
        "Search file contents with a regular expression (Rust regex syntax), ripgrep-style: respects .gitignore, skips binary files. output_mode: content (path:line:text, default), files (paths only), or count."
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
                "max_results": {"type": "integer", "minimum": 1, "description": "Cap on matching lines (content) or files. Default 200."}
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
            walker(&base, false, None)
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
            format!("; {NOT_WALKED}")
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
const INLINE_FILES: usize = 256;
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
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    depth: Option<usize>,
    #[serde(default)]
    all: bool,
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
        let mut entries: Vec<(PathBuf, bool, u64)> = Vec::new();
        let mut b = ignore::WalkBuilder::new(&base);
        b.max_depth(Some(depth))
            .hidden(!a.all)
            .git_ignore(!a.all)
            .ignore(!a.all)
            .parents(!a.all)
            .require_git(false);
        b.filter_entry(|e| e.file_name() != ".git");
        b.sort_by_file_path(|x, y| x.cmp(y));
        let mut total = 0usize;
        // What the listing leaves out (theseus-8ye): the directories at the
        // last level, whose contents it never reads, and the first entry past
        // its length, in path order.
        let mut undescended = 0usize;
        let mut first_left: Option<PathBuf> = None;
        for e in b.build().flatten() {
            if e.path() == base {
                continue;
            }
            total += 1;
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if is_dir && e.depth() == depth {
                undescended += 1;
            }
            if entries.len() < ctx.max_entries {
                let size = if is_dir {
                    0
                } else {
                    e.metadata().map(|m| m.len()).unwrap_or(0)
                };
                entries.push((e.path().to_path_buf(), is_dir, size));
            } else if first_left.is_none() {
                first_left = Some(e.path().to_path_buf());
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(d: &tempfile::TempDir) -> ToolCtx {
        ToolCtx::for_tests(d.path())
    }

    /// A PNG header for the given size, then `pad` zero bytes.
    fn png(width: u32, height: u32, pad: usize) -> Vec<u8> {
        let mut v = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
        v.extend_from_slice(&width.to_be_bytes());
        v.extend_from_slice(&height.to_be_bytes());
        v.extend_from_slice(&[8, 2, 0, 0, 0, 0, 0, 0, 0]);
        v.resize(v.len() + pad, 0);
        v
    }

    /// A named pipe at `path`.
    fn mkfifo(path: &Path) {
        use std::os::unix::ffi::OsStrExt;
        let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0, "mkfifo");
    }

    /// `call` on another thread, given five seconds. A call that waits on a
    /// FIFO is let go, by opening the FIFO's other end, before the test fails,
    /// so a revert of the fix fails here rather than hanging the suite.
    fn within_5s<T: Send + 'static>(fifo: &Path, call: impl FnOnce() -> T + Send + 'static) -> T {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(call());
        });
        match rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(r) => r,
            Err(_) => {
                drop(fs::OpenOptions::new().write(true).open(fifo));
                panic!("the call waited on {} for 5 s", fifo.display());
            }
        }
    }

    /// Review 2's R9: a FIFO in the roots is refused at once, by name, by
    /// every tool that reads a file, never read (a read waits for a writer,
    /// holding a core and a thread). So are a socket and a device.
    #[test]
    fn a_fifo_a_socket_or_a_device_is_refused_never_read() {
        let d = tempfile::tempdir().unwrap();
        let fifo = d.path().join("pipe");
        mkfifo(&fifo);
        let base = d.path().to_path_buf();
        let read = within_5s(&fifo, move || {
            Read.run(&json!({"path": "pipe"}), &ToolCtx::for_tests(&base))
        });
        let e = read.unwrap_err().message;
        assert!(
            e.ends_with("pipe is a FIFO, not a regular file; fs_read reads only regular files (a read of a FIFO, a socket, or a device can wait forever for a writer)"),
            "{e}"
        );
        let base = d.path().to_path_buf();
        let edit = within_5s(&fifo, move || {
            Edit.run(
                &json!({"path": "pipe", "old_string": "a", "new_string": "b"}),
                &ToolCtx::for_tests(&base),
            )
        });
        assert!(edit
            .unwrap_err()
            .message
            .contains("pipe is a FIFO, not a regular file; fs_edit"));
        let base = d.path().to_path_buf();
        let diff = within_5s(&fifo, move || {
            crate::text::Diff.run(
                &json!({"a_path": "pipe", "b": "x\n"}),
                &ToolCtx::for_tests(&base),
            )
        });
        assert!(diff
            .unwrap_err()
            .message
            .contains("is a FIFO, not a regular file; text_diff"));
        // A search passes it by, named or walked past.
        for path in ["pipe", "."] {
            let base = d.path().to_path_buf();
            let grep = within_5s(&fifo, move || {
                Grep.run(
                    &json!({"pattern": "a", "path": path}),
                    &ToolCtx::for_tests(&base),
                )
            });
            assert!(grep.is_ok(), "{path}");
        }
        let _sock = std::os::unix::net::UnixListener::bind(d.path().join("sock")).unwrap();
        let e = Read
            .run(&json!({"path": "sock"}), &ctx(&d))
            .unwrap_err()
            .message;
        assert!(e.contains("sock is a socket, not a regular file"), "{e}");
        let e = Read
            .run(&json!({"path": "/dev/null"}), &ctx(&d))
            .unwrap_err()
            .message;
        assert!(
            e.contains("/dev/null is a character device, not a regular file"),
            "{e}"
        );
    }

    /// The read goes through the cap, so a file that grew after its size was
    /// checked is cut there, never read whole (review 2's R9).
    #[test]
    fn a_read_stops_at_its_cap() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("grows.txt");
        fs::write(&p, "0123456789").unwrap();
        assert_eq!(read_regular(&p, 10, true).unwrap(), b"0123456789");
        match read_regular(&p, 4, true) {
            Err(Unread::Over(n)) => assert_eq!(n, 5, "it read one byte past the cap, no more"),
            other => panic!("{other:?}"),
        }
        let link = d.path().join("link.txt");
        std::os::unix::fs::symlink(&p, &link).unwrap();
        assert!(read_regular(&link, 10, true).is_ok());
        assert!(
            matches!(
                read_regular(&link, 10, false),
                Err(Unread::Kind(_) | Unread::Io(_))
            ),
            "without follow, a link is not read"
        );
    }

    /// `fs.read` of an image returns it for the model (theseus-9g2); one
    /// over the 5 MiB limit is described and refused with the reason.
    #[test]
    fn reading_an_image_returns_it_and_a_huge_one_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let c = ctx(&d);
        let bytes = png(640, 480, 500);
        std::fs::write(d.path().join("shot.png"), &bytes).unwrap();
        let (out, img) = Read
            .run_with_image(&json!({"path": "shot.png"}), &c)
            .unwrap();
        assert!(
            out.text
                .ends_with("shot.png is a PNG image, 640×480, 533 bytes."),
            "{}",
            out.text
        );
        assert_eq!(out.meta["image"], "image/png");
        let img = img.expect("an image");
        assert_eq!((img.name.as_str(), img.info.width), ("shot.png", 640));
        assert_eq!(img.bytes, bytes);
        // `run` (what a caller without images gets) says the same, without the bytes.
        let plain = Read.run(&json!({"path": "shot.png"}), &c).unwrap();
        assert_eq!(plain.text, out.text);

        std::fs::write(d.path().join("huge.png"), png(4000, 3000, 6 * 1024 * 1024)).unwrap();
        let (out, img) = Read
            .run_with_image(&json!({"path": "huge.png"}), &c)
            .unwrap();
        assert!(img.is_none());
        assert!(
            out.text
                .ends_with(": not shown, an image over the 5 MiB limit."),
            "{}",
            out.text
        );
        assert_eq!(out.meta["not_shown"], "an image over the 5 MiB limit");
    }

    /// What a cut left out of a read is rows, by number (theseus-46v): the
    /// call `rest` names returns exactly them, a row cut part-way included.
    #[test]
    fn a_reads_rest_names_the_rows_left_out_and_the_call_returns_them() {
        let d = tempfile::tempdir().unwrap();
        let c = ctx(&d);
        let body: String = (1..=40).map(|i| format!("ledger row {i}\n")).collect();
        std::fs::write(d.path().join("rows.txt"), body).unwrap();
        let out = Read.run(&json!({"path": "rows.txt"}), &c).unwrap().text;
        let rows: Vec<&str> = out.lines().collect();

        // Whole rows 12 to 30.
        let left = rows[11..30].join("\n") + "\n";
        let rest = Read.rest(&left);
        assert_eq!(
            rest,
            "lines 12-30; fs_read with offset=12 and limit=19 returns them"
        );
        let again = Read
            .run(&json!({"path": "rows.txt", "offset": 12, "limit": 19}), &c)
            .unwrap()
            .text;
        // Those rows, then the read's own line on where the file goes on.
        assert!(again.starts_with(&left), "the call returns them: {again}");
        assert!(again[left.len()..].starts_with("[showing lines 12-30 of 40;"));

        // Cut part-way: the end of row 11 and the start of row 31.
        let left = format!(" row 11\n{}\n    31\tledg", rows[11..30].join("\n"));
        assert_eq!(
            Read.rest(&left),
            "lines 11-31; fs_read with offset=11 and limit=21 returns them"
        );
        // Nothing that reads as a row: a smaller read.
        assert_eq!(
            Read.rest("no rows here"),
            "fs_read with a smaller limit returns them"
        );
    }

    #[test]
    fn read_write_edit_roundtrip_with_errors_the_model_can_act_on() {
        let d = tempfile::tempdir().unwrap();
        let c = ctx(&d);
        let out = WriteFile
            .run(
                &json!({"path": "src/a.rs", "content": "fn a() {}\nfn b() {}\n"}),
                &c,
            )
            .unwrap();
        assert!(out.text.starts_with("Created"), "{}", out.text);
        let r = Read.run(&json!({"path": "src/a.rs"}), &c).unwrap();
        assert_eq!(r.text, "     1\tfn a() {}\n     2\tfn b() {}\n");
        let e = Edit.run(&json!({"path": "src/a.rs", "old_string": "fn b() {}", "new_string": "fn b() { a() }"}), &c).unwrap();
        assert!(
            e.text.contains("+fn b() { a() }") && e.text.contains("-fn b() {}"),
            "{}",
            e.text
        );
        let miss = Edit
            .run(
                &json!({"path": "src/a.rs", "old_string": "nope", "new_string": "x"}),
                &c,
            )
            .unwrap_err();
        assert!(miss.message.contains("not found"));
        WriteFile
            .run(&json!({"path": "dup.txt", "content": "x x x"}), &c)
            .unwrap();
        let dup = Edit
            .run(
                &json!({"path": "dup.txt", "old_string": "x", "new_string": "y"}),
                &c,
            )
            .unwrap_err();
        assert!(dup.message.contains("occurs 3 times"));
        Edit.run(
            &json!({"path": "dup.txt", "old_string": "x", "new_string": "y", "replace_all": true}),
            &c,
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(d.path().join("dup.txt")).unwrap(),
            "y y y"
        );
        assert!(Edit
            .plan(
                &json!({"path": "a", "old_string": "", "new_string": "x"}),
                &c
            )
            .is_err());
        assert!(
            Read.plan(&json!({"path": "a", "bogus": 1}), &c).is_err(),
            "unknown fields are rejected"
        );
        // Paging.
        WriteFile.run(&json!({"path": "long.txt", "content": (1..=10).map(|i| format!("l{i}\n")).collect::<String>()}), &c).unwrap();
        let p = Read
            .run(&json!({"path": "long.txt", "offset": 4, "limit": 2}), &c)
            .unwrap();
        assert!(
            p.text.starts_with("     4\tl4\n     5\tl5\n") && p.text.contains("offset=6"),
            "{}",
            p.text
        );
    }

    /// The daemon runs under umask 077 (theseus-wz2), and what a tool makes in
    /// the workspace gets the operator's mode instead: a new file, and each
    /// directory made for it. A file that exists keeps its own mode.
    #[test]
    fn new_files_and_directories_get_the_operators_mode() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let d = tempfile::tempdir().unwrap();
        let c = ToolCtx {
            umask: Some(0o027),
            ..ctx(&d)
        };
        WriteFile
            .run(&json!({"path": "deep/er/new.txt", "content": "x"}), &c)
            .unwrap();
        assert_eq!(mode(&d.path().join("deep")), 0o750);
        assert_eq!(mode(&d.path().join("deep/er")), 0o750);
        assert_eq!(mode(&d.path().join("deep/er/new.txt")), 0o640);
        let kept = d.path().join("kept.txt");
        fs::write(&kept, "a").unwrap();
        fs::set_permissions(&kept, fs::Permissions::from_mode(0o604)).unwrap();
        WriteFile
            .run(&json!({"path": "kept.txt", "content": "b"}), &c)
            .unwrap();
        assert_eq!(mode(&kept), 0o604);
        let private = ToolCtx {
            umask: Some(0o077),
            ..ctx(&d)
        };
        let patch = "--- /dev/null\n+++ b/made/by-patch.txt\n@@ -0,0 +1 @@\n+hello\n";
        Patch.run(&json!({"patch": patch}), &private).unwrap();
        assert_eq!(mode(&d.path().join("made")), 0o700);
        assert_eq!(mode(&d.path().join("made/by-patch.txt")), 0o600);
    }

    #[test]
    fn patch_applies_atomically_across_files_or_not_at_all() {
        let d = tempfile::tempdir().unwrap();
        let c = ctx(&d);
        fs::write(d.path().join("x.txt"), "one\ntwo\nthree\n").unwrap();
        let patch = "--- a/x.txt\n+++ b/x.txt\n@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n three\n--- /dev/null\n+++ b/new.txt\n@@ -0,0 +1 @@\n+hello\n";
        let plan = Patch.plan(&json!({"patch": patch}), &c).unwrap();
        assert_eq!(plan.resources.len(), 2);
        let out = Patch.run(&json!({"patch": patch}), &c).unwrap();
        assert!(out.text.contains("+1 -1"), "{}", out.text);
        assert_eq!(
            fs::read_to_string(d.path().join("x.txt")).unwrap(),
            "one\nTWO\nthree\n"
        );
        assert_eq!(
            fs::read_to_string(d.path().join("new.txt")).unwrap(),
            "hello\n"
        );
        // A second section that does not apply leaves the first untouched.
        let bad = "--- a/x.txt\n+++ b/x.txt\n@@ -1,3 +1,3 @@\n one\n-TWO\n+2\n three\n--- a/new.txt\n+++ b/new.txt\n@@ -1 +1 @@\n-nothere\n+x\n";
        assert!(Patch.run(&json!({"patch": bad}), &c).is_err());
        assert_eq!(
            fs::read_to_string(d.path().join("x.txt")).unwrap(),
            "one\nTWO\nthree\n"
        );
        // A deletion must show every line it removes; a partial one deletes nothing.
        let partial = "--- a/x.txt\n+++ /dev/null\n@@ -1,1 +0,0 @@\n-one\n";
        let e = Patch.run(&json!({"patch": partial}), &c).unwrap_err();
        assert!(
            e.message.contains("must remove every line"),
            "{}",
            e.message
        );
        assert!(d.path().join("x.txt").exists());
        let whole = "--- a/x.txt\n+++ /dev/null\n@@ -1,3 +0,0 @@\n-one\n-TWO\n-three\n";
        Patch.run(&json!({"patch": whole}), &c).unwrap();
        assert!(!d.path().join("x.txt").exists());
    }

    /// Cores for the tests: a thread per job, at most `n` at once.
    #[derive(Debug)]
    struct Threads {
        free: Arc<AtomicUsize>,
        lent: AtomicUsize,
    }

    impl Cores for Threads {
        fn try_spawn(&self, job: Box<dyn FnOnce() + Send>) -> bool {
            let took = self
                .free
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1));
            if took.is_err() {
                return false;
            }
            self.lent.fetch_add(1, Ordering::SeqCst);
            let free = self.free.clone();
            std::thread::spawn(move || {
                job();
                free.fetch_add(1, Ordering::SeqCst);
            });
            true
        }
    }

    /// `fs.grep` searches a big tree's files ahead on borrowed cores
    /// (theseus-a60), and says exactly what it says on its own thread: every
    /// mode, with context, a glob, no match, and a cap that falls inside a
    /// file searched ahead.
    #[test]
    fn grep_says_the_same_with_borrowed_cores() {
        let d = tempfile::tempdir().unwrap();
        for i in 0..1000 {
            let dir = d.path().join(format!("d{}", i % 10));
            fs::create_dir_all(&dir).unwrap();
            let text: String = (0..20)
                .map(|l| match i % 7 == 0 && l % (1 + i % 5) == 0 {
                    true => format!("needle {i} {l}\n"),
                    false => format!("hay {i} {l}\n"),
                })
                .collect();
            fs::write(dir.join(format!("f{i}.txt")), text).unwrap();
        }
        let threads = Arc::new(Threads {
            free: Arc::new(AtomicUsize::new(3)),
            lent: AtomicUsize::new(0),
        });
        let lent = ToolCtx {
            cores: Some(threads.clone()),
            ..ctx(&d)
        };
        let mut crossed = false;
        for input in [
            json!({"pattern": "needle"}),
            json!({"pattern": "needle", "max_results": 1000}),
            json!({"pattern": "needle", "context": 2, "max_results": 777}),
            json!({"pattern": "needle", "max_results": 100000}),
            json!({"pattern": "needle", "output_mode": "files", "max_results": 100}),
            json!({"pattern": "needle", "output_mode": "count", "max_results": 1000}),
            json!({"pattern": "needle 99[0-9]", "glob": "f99*.txt"}),
            json!({"pattern": "nowhere"}),
        ] {
            let alone = Grep.run(&input, &ctx(&d)).unwrap();
            let helped = Grep.run(&input, &lent).unwrap();
            assert_eq!(alone.text, helped.text, "{input}");
            assert_eq!(alone.meta, helped.meta, "{input}");
            let scanned = alone.meta["files_scanned"].as_u64().unwrap() as usize;
            crossed |= alone.meta["capped"] == true
                && input.get("output_mode").is_none()
                && scanned > INLINE_FILES;
        }
        assert!(crossed, "a cap fell among the files searched ahead");
        assert!(threads.lent.load(Ordering::SeqCst) > 0, "it borrowed cores");
        // A helper that found no chunk left may still be on its way out.
        let t0 = std::time::Instant::now();
        while threads.free.load(Ordering::SeqCst) < 3 {
            assert!(t0.elapsed().as_secs() < 5, "every core came back");
            std::thread::yield_now();
        }
    }

    #[test]
    fn glob_grep_list_respect_gitignore_and_report_counts() {
        let d = tempfile::tempdir().unwrap();
        let c = ctx(&d);
        fs::create_dir_all(d.path().join("src/sub")).unwrap();
        fs::create_dir_all(d.path().join("target")).unwrap();
        fs::write(d.path().join(".gitignore"), "target/\n").unwrap();
        fs::write(
            d.path().join("src/lib.rs"),
            "pub fn alpha() {}\n// TODO: beta\n",
        )
        .unwrap();
        fs::write(
            d.path().join("src/sub/m.rs"),
            "fn gamma() {}\nfn alpha_two() {}\n",
        )
        .unwrap();
        fs::write(d.path().join("target/junk.rs"), "fn alpha() {}\n").unwrap();
        let g = Glob.run(&json!({"pattern": "**/*.rs"}), &c).unwrap();
        assert_eq!(g.meta["matches"], 2, "{}", g.text);
        assert!(!g.text.contains("target"));
        let hits = Grep.run(&json!({"pattern": "fn alpha"}), &c).unwrap();
        assert_eq!(hits.meta["matching_lines"], 2, "{}", hits.text);
        assert!(hits.text.contains("lib.rs:1:pub fn alpha() {}"));
        let files = Grep
            .run(&json!({"pattern": "TODO", "output_mode": "files"}), &c)
            .unwrap();
        assert!(
            files.text.lines().next().unwrap().ends_with("lib.rs"),
            "{}",
            files.text
        );
        let ci = Grep
            .run(
                &json!({"pattern": "todo", "case_insensitive": true, "output_mode": "count"}),
                &c,
            )
            .unwrap();
        assert!(
            ci.text.lines().next().unwrap().ends_with("lib.rs:1"),
            "{}",
            ci.text
        );
        let ctx_hit = Grep
            .run(&json!({"pattern": "gamma", "context": 1}), &c)
            .unwrap();
        assert!(
            ctx_hit.text.contains("m.rs-2-fn alpha_two() {}"),
            "{}",
            ctx_hit.text
        );
        assert!(Grep.plan(&json!({"pattern": "("}), &c).is_err());
        let l = List.run(&json!({}), &c).unwrap();
        assert!(
            l.text.contains("src/") && l.text.contains("lib.rs (") && !l.text.contains("target/"),
            "{}",
            l.text
        );
    }

    /// Five lantern logs, written a minute apart, oldest first, and a hidden
    /// one: the tree the listing tests read.
    fn lantern_logs(d: &tempfile::TempDir) {
        let t0 = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_790_000_000);
        for i in 1..=5 {
            let p = d.path().join(format!("lantern-{i}.txt"));
            fs::write(&p, "wick\nwick\nwick\n").unwrap();
            fs::File::options()
                .write(true)
                .open(&p)
                .unwrap()
                .set_modified(t0 + std::time::Duration::from_secs(60 * i))
                .unwrap();
        }
        fs::write(d.path().join(".lantern-key.txt"), "wick\n").unwrap();
    }

    /// Appendix F's rule (theseus-8ye): a glob's result states its scope (the
    /// pattern, where, newest first, what the walk never sees) and exactly
    /// what it left out (the oldest, by count, and the call that returns
    /// them); an empty result says what was looked at and what was not.
    #[test]
    fn a_globs_result_says_its_scope_and_the_oldest_it_left_out() {
        let d = tempfile::tempdir().unwrap();
        lantern_logs(&d);
        let base = d.path().display().to_string();
        let narrow = ToolCtx {
            max_entries: 3,
            ..ctx(&d)
        };
        let g = Glob.run(&json!({"pattern": "*.txt"}), &narrow).unwrap();
        let lines: Vec<&str> = g.text.lines().collect();
        assert_eq!(lines.len(), 4, "{}", g.text);
        assert!(
            lines[0].ends_with("lantern-5.txt"),
            "newest first: {}",
            g.text
        );
        assert_eq!(
            lines[3],
            format!(
                "[3 of 5 files matching *.txt under {base}, newest first; the 2 oldest not shown: a \
                 narrower pattern or path returns them; hidden files and .gitignore'd paths are not \
                 searched]"
            )
        );
        assert_eq!(
            (g.meta["matches"].clone(), g.meta["shown"].clone()),
            (json!(5), json!(3))
        );
        let all = Glob.run(&json!({"pattern": "*.txt"}), &ctx(&d)).unwrap();
        assert!(
            all.text.ends_with(&format!(
                "\n[5 files matching *.txt under {base}, newest first; hidden files and \
                 .gitignore'd paths are not searched]"
            )),
            "{}",
            all.text
        );
        let none = Glob
            .run(&json!({"pattern": ".lantern*"}), &ctx(&d))
            .unwrap();
        assert_eq!(
            none.text,
            format!(
                "No files match .lantern* under {base} (5 files looked at); hidden files and \
                 .gitignore'd paths are not searched."
            )
        );
    }

    /// A grep's result states how many lines or files it found, in how many
    /// of how many files searched, where; capped, it says it stopped, where,
    /// how many files it never opened, that more may match, and the call
    /// that returns them (theseus-8ye).
    #[test]
    fn a_greps_result_says_its_scope_and_where_the_cap_stopped_it() {
        let d = tempfile::tempdir().unwrap();
        lantern_logs(&d);
        let base = d.path().display().to_string();
        let c = ctx(&d);
        let walked = "hidden files and .gitignore'd paths are not searched";
        let grep = |input: Value| Grep.run(&input, &c).unwrap();
        let last = |t: &str| t.lines().last().unwrap().to_string();
        let full = grep(json!({"pattern": "wick"}));
        assert_eq!(full.text.lines().count(), 16, "{}", full.text);
        assert_eq!(
            last(&full.text),
            format!("[15 matching lines in 5 files; 5 files under {base} searched; {walked}]")
        );
        let capped = grep(json!({"pattern": "wick", "max_results": 4}));
        assert_eq!(capped.text.lines().count(), 5, "{}", capped.text);
        let stopped_in = capped
            .text
            .lines()
            .nth(3)
            .unwrap()
            .split(':')
            .next()
            .unwrap();
        assert_eq!(
            last(&capped.text),
            format!(
                "[stopped at max_results=4: the first 4 matching lines, from 2 files, the last in \
                 {stopped_in}; 3 more of 5 files under {base} not searched, so more may match: a \
                 narrower search, or a larger max_results, returns them; {walked}]"
            )
        );
        assert_eq!(
            (
                capped.meta["capped"].clone(),
                capped.meta["files_searched"].clone(),
                capped.meta["files_total"].clone()
            ),
            (json!(true), json!(2), json!(5))
        );
        let files = grep(json!({"pattern": "wick", "output_mode": "files", "max_results": 2}));
        assert_eq!(
            last(&files.text),
            format!(
                "[stopped at max_results=2: the first 2 files with matches; 3 more of 5 files under \
                 {base} not searched, so more may match: a narrower search, or a larger \
                 max_results, returns them; {walked}]"
            )
        );
        let counted = grep(json!({"pattern": "wick", "output_mode": "count"}));
        assert_eq!(
            last(&counted.text),
            format!(
                "[5 files with matches (15 matching lines); 5 files under {base} searched; {walked}]"
            )
        );
        let only = grep(json!({"pattern": "wick", "glob": "lantern-1*"}));
        assert_eq!(
            last(&only.text),
            format!(
                "[3 matching lines in 1 file; 1 file matching lantern-1* under {base} searched; \
                 {walked}]"
            )
        );
        let none = grep(json!({"pattern": "tallow"}));
        assert_eq!(
            none.text,
            format!("No matches for /tallow/; 5 files under {base} searched; {walked}.")
        );
        let one = d.path().join("lantern-2.txt").display().to_string();
        let in_one = grep(json!({"pattern": "wick", "path": one}));
        assert_eq!(last(&in_one.text), format!("[3 matching lines in {one}]"));
        let cut_one = grep(json!({"pattern": "wick", "path": one, "max_results": 2}));
        assert_eq!(
            last(&cut_one.text),
            format!(
                "[stopped at max_results=2: the first 2 matching lines in {one}; its later lines \
                 were not searched, so more may match: a larger max_results returns them]"
            )
        );
    }

    /// A listing states its scope (where, to what depth) and what it left
    /// out: the entries past its length, from where in path order; the
    /// directories whose contents lie below its depth; the hidden and ignored
    /// entries unless `all` shows them (theseus-8ye).
    #[test]
    fn a_listings_result_says_its_scope_and_what_it_left_out() {
        let d = tempfile::tempdir().unwrap();
        lantern_logs(&d);
        fs::create_dir_all(d.path().join("harbor/dock/crates")).unwrap();
        fs::write(d.path().join("harbor/dock/crates/rope.txt"), "coil\n").unwrap();
        let base = d.path().display().to_string();
        let l = List.run(&json!({}), &ctx(&d)).unwrap();
        assert_eq!(
            l.text.lines().last().unwrap(),
            format!(
                "[7 entries under {base} to depth 2; the contents of 1 directory at depth 2 not \
                 listed: fs_list of one, or a greater depth, lists them; hidden and .gitignore'd \
                 entries not shown: all=true shows them]"
            )
        );
        let narrow = ToolCtx {
            max_entries: 3,
            ..ctx(&d)
        };
        let cut = List.run(&json!({"depth": 3}), &narrow).unwrap();
        assert_eq!(
            cut.text.lines().last().unwrap(),
            format!(
                "[3 of 8 entries under {base} to depth 3, in path order; the rest, from \
                 lantern-1.txt on, not shown: fs_list of a subdirectory, or with a lower depth, \
                 returns them; the contents of 1 directory at depth 3 not listed: fs_list of one, \
                 or a greater depth, lists them; hidden and .gitignore'd entries not shown: \
                 all=true shows them]"
            )
        );
        let every = List
            .run(&json!({"depth": 6, "all": true}), &ctx(&d))
            .unwrap();
        assert_eq!(
            every.text.lines().last().unwrap(),
            format!("[10 entries under {base} to depth 6]")
        );
    }
}
