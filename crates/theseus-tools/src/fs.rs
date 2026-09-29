//! `fs.*` (spec §3.24): read, write, edit, patch, glob, grep, list. Native:
//! the walker and gitignore handling are ripgrep's own crates (`ignore`,
//! `globset`, `grep-searcher`); writes are atomic (temp file, then rename).

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    image, parse, Access, ImageData, Plan, Resource, Retry, Tool, ToolClass, ToolCtx, ToolFailure,
    ToolOutput,
};

const MAX_LINE_CHARS: usize = 2000;
const DEFAULT_READ_LINES: usize = 2000;
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;

fn is_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8192).any(|b| *b == 0)
}

/// Atomic write: temp file beside the target, fsync, rename; keeps the mode.
pub fn write_atomic(path: &Path, content: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
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
    }
    fs::rename(&tmp, path)
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
        })
    }
    fn run(&self, input: &Value, ctx: &ToolCtx) -> Result<ToolOutput, ToolFailure> {
        self.run_with_image(input, ctx).map(|(o, _)| o)
    }
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
        let bytes = fs::read(&path)?;
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
        write_atomic(&path, a.content.as_bytes())?;
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
        })
    }
    fn run(&self, input: &Value, ctx: &ToolCtx) -> Result<ToolOutput, ToolFailure> {
        let a: EditArgs = parse(input).map_err(ToolFailure::new)?;
        let path = ctx.resolve(&a.path);
        let before = fs::read_to_string(&path)
            .map_err(|e| ToolFailure::new(format!("cannot read {}: {e}", path.display())))?;
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
        write_atomic(&path, after.as_bytes())?;
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
                Some(o) => fs::read_to_string(ctx.resolve(o))
                    .map_err(|e| ToolFailure::new(format!("cannot read {o}: {e}")))?,
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
                Some(c) => write_atomic(path, c.as_bytes())?,
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
        let mut text = if shown.is_empty() {
            format!("No files match {} under {}.", a.pattern, base.display())
        } else {
            shown.join("\n")
        };
        if total > shown.len() {
            text.push_str(&format!(
                "\n[{} of {total} matches shown; narrow the pattern]",
                shown.len()
            ));
        }
        Ok(ToolOutput {
            text,
            meta: json!({"base": base, "matches": total, "scanned": scanned}),
        })
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
        })
    }
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
        let mut searcher = grep_searcher::SearcherBuilder::new()
            .line_number(true)
            .binary_detection(grep_searcher::BinaryDetection::quit(0))
            .before_context(if mode == "content" { ctxl } else { 0 })
            .after_context(if mode == "content" { ctxl } else { 0 })
            .build();
        let files: Vec<PathBuf> = if base.is_file() {
            vec![base.clone()]
        } else {
            walker(&base, false, None)
                .flatten()
                .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
                .map(|e| e.into_path())
                .collect()
        };
        let mut out: Vec<String> = Vec::new();
        let mut hits = 0usize;
        let mut files_hit: Vec<(String, usize)> = Vec::new();
        let mut scanned = 0usize;
        for f in &files {
            if let Some(g) = &glob {
                let name = f.file_name().map(PathBuf::from).unwrap_or_default();
                let rel = f.strip_prefix(&base).unwrap_or(f);
                if !(g.is_match(&name) || g.is_match(rel)) {
                    continue;
                }
            }
            scanned += 1;
            let shown = f.display().to_string();
            let mut local: Vec<String> = Vec::new();
            let mut local_hits = 0usize;
            let cap = if mode == "content" {
                max.saturating_sub(hits)
            } else {
                usize::MAX
            };
            if cap == 0 {
                break;
            }
            let mut sink = Collect {
                path: &shown,
                out: &mut local,
                hits: &mut local_hits,
                max: cap,
                last_line: None,
            };
            if searcher.search_path(&matcher, f, &mut sink).is_err() {
                continue;
            }
            if local_hits > 0 {
                hits += local_hits;
                files_hit.push((shown.clone(), local_hits));
                if mode == "content" {
                    if !out.is_empty() && ctxl > 0 {
                        out.push("--".into());
                    }
                    out.extend(local);
                }
            }
            if mode != "content" && files_hit.len() >= max {
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
        let text = if files_hit.is_empty() {
            format!("No matches for /{}/ under {}.", a.pattern, base.display())
        } else {
            text
        };
        Ok(ToolOutput {
            text,
            meta: json!({"base": base, "files_with_matches": files_hit.len(), "matching_lines": hits, "files_scanned": scanned, "capped": mode == "content" && hits >= max}),
        })
    }
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
        for e in b.build().flatten() {
            if e.path() == base {
                continue;
            }
            total += 1;
            if entries.len() < ctx.max_entries {
                let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                let size = if is_dir {
                    0
                } else {
                    e.metadata().map(|m| m.len()).unwrap_or(0)
                };
                entries.push((e.path().to_path_buf(), is_dir, size));
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
        if total > entries.len() {
            text.push_str(&format!(
                "[{} of {total} entries shown; list a subdirectory or lower the depth]\n",
                entries.len()
            ));
        }
        Ok(ToolOutput {
            text,
            meta: json!({"base": base, "entries": total, "shown": entries.len(), "depth": depth}),
        })
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
        assert!(files.text.ends_with("lib.rs"));
        let ci = Grep
            .run(
                &json!({"pattern": "todo", "case_insensitive": true, "output_mode": "count"}),
                &c,
            )
            .unwrap();
        assert!(ci.text.ends_with("lib.rs:1"), "{}", ci.text);
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
}
