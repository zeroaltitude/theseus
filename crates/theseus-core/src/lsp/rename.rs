//! The rename (L2), in two calls, as the stack tools are (`aws/stack.rs`):
//! a gate's plan is synchronous, and a rename's edit is known only once the
//! server answers.
//!
//! - `lsp.rename.plan {path, line, symbol, new_name}`, a read: the server's
//!   edit, made into each file's new text and shown as a diff, kept by its
//!   digest for this daemon's life. It writes nothing. When the symbol's
//!   definition is elsewhere (TypeScript renames an imported name at its
//!   import), the rename is asked at the definition, and the answer says so.
//! - `lsp.rename {digest}`, a write and not repeatable: exactly that edit.
//!   Its plan's resources are every file it writes, so the gate judges it as
//!   an `fs.patch` of each (the floor, the approve list), and a file outside
//!   the workspace roots waits too ([`outside_roots`]). It writes nothing
//!   unless every file is as the plan read it, then writes each, tells the
//!   servers, and returns the diff.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use theseus_lsp::types::{Position, TextEdit};
use theseus_tools::{
    parse, Access, AsyncRun, Backend, Plan, Resource, Retry, Tool, ToolClass, ToolCtx, ToolFailure,
    ToolOutput,
};

use super::tools::{at_schema, deadline, locate, plan_read, At};
use super::{lock, Board};
use crate::policy::{Decision, Posture};

/// One file a rename writes: as the plan read it, and after.
#[derive(Debug, Clone)]
pub struct FileEdit {
    pub path: PathBuf,
    pub before: String,
    pub after: String,
    pub edits: usize,
}

/// A rename a plan showed, kept by its digest.
#[derive(Debug, Clone)]
pub struct RenameShown {
    pub symbol: String,
    pub new_name: String,
    pub server: String,
    pub files: Vec<FileEdit>,
    pub diff: String,
}

/// The plans kept: the newest few.
#[derive(Default)]
pub(super) struct Shows(Mutex<Vec<(String, RenameShown)>>);

const KEPT: usize = 32;

impl Shows {
    fn put(&self, digest: &str, s: RenameShown) {
        let mut v = lock(&self.0);
        v.retain(|(d, _)| d != digest);
        if v.len() >= KEPT {
            v.remove(0);
        }
        v.push((digest.to_string(), s));
    }

    fn get(&self, digest: &str) -> Option<RenameShown> {
        lock(&self.0)
            .iter()
            .find(|(d, _)| d == digest)
            .map(|(_, s)| s.clone())
    }

    fn take(&self, digest: &str) {
        lock(&self.0).retain(|(d, _)| d != digest);
    }
}

fn sha(text: &str) -> String {
    hex::encode(&Sha256::digest(text.as_bytes())[..12])
}

/// The edit's digest: every file, its text before, and after.
fn digest(files: &[FileEdit]) -> String {
    let mut h = Sha256::new();
    for f in files {
        h.update(f.path.as_os_str().as_encoded_bytes());
        h.update([0]);
        h.update(sha(&f.before));
        h.update([0]);
        h.update(f.after.as_bytes());
        h.update([0]);
    }
    hex::encode(&h.finalize()[..12])
}

/// The byte offset of an LSP position (UTF-16 columns) in `text`; past the
/// end of a line, its end; past the last line, the text's end.
fn offset(text: &str, p: Position) -> usize {
    let mut start = 0;
    for _ in 0..p.line {
        match text[start..].find('\n') {
            Some(i) => start += i + 1,
            None => return text.len(),
        }
    }
    let end = text[start..].find('\n').map_or(text.len(), |i| start + i);
    let row = text[start..end]
        .strip_suffix('\r')
        .unwrap_or(&text[start..end]);
    start + theseus_lsp::position::byte_column(row, p.character).min(row.len())
}

/// `edits` applied to `text`, or why not (two that overlap).
pub(super) fn apply(text: &str, edits: &[TextEdit]) -> Result<String, String> {
    let mut spans: Vec<(usize, usize, &str)> = edits
        .iter()
        .map(|e| {
            (
                offset(text, e.range.start),
                offset(text, e.range.end),
                e.new_text.as_str(),
            )
        })
        .collect();
    spans.sort_by_key(|s| (s.0, s.1));
    for w in spans.windows(2) {
        if w[1].0 < w[0].1 {
            return Err("the server's edits overlap".into());
        }
    }
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for (s, e, new) in spans {
        if s < at || e < s {
            return Err("the server's edits are out of order".into());
        }
        out.push_str(&text[at..s]);
        out.push_str(new);
        at = e;
    }
    out.push_str(&text[at..]);
    Ok(out)
}

fn diff_of(files: &[FileEdit]) -> String {
    files
        .iter()
        .map(|f| {
            let name = f.path.display().to_string();
            similar::TextDiff::from_lines(&f.before, &f.after)
                .unified_diff()
                .context_radius(1)
                .header(&name, &name)
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("")
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanArgs {
    path: String,
    line: u32,
    symbol: String,
    #[serde(default)]
    occurrence: Option<u32>,
    new_name: String,
}

impl PlanArgs {
    fn at(&self) -> At {
        At {
            path: self.path.clone(),
            line: self.line,
            symbol: self.symbol.clone(),
            occurrence: self.occurrence,
            include_declaration: true,
        }
    }
}

struct Plain {
    at: At,
    new_name: String,
}

pub(super) struct RenamePlan(pub Arc<Board>);

impl Tool for RenamePlan {
    fn name(&self) -> &'static str {
        "lsp.rename.plan"
    }

    fn description(&self) -> &'static str {
        "Plan a rename with the file's language server: every edit it would make, in every file, \
         as a diff, and the plan's digest. It changes nothing: lsp_rename with the digest applies \
         exactly this edit. Name the symbol by its file, its 1-based line, and its text on that line."
    }

    fn input_schema(&self) -> Value {
        let mut s = at_schema(json!({
            "new_name": {"type": "string", "description": "The symbol's new name."}
        }));
        s["required"] = json!(["path", "line", "symbol", "new_name"]);
        s
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

    fn deadline(&self) -> Option<std::time::Duration> {
        Some(deadline(&self.0))
    }

    fn plan(&self, input: &Value, ctx: &ToolCtx) -> Result<Plan, String> {
        let p: PlanArgs = parse(input)?;
        let a = Plain {
            at: p.at(),
            new_name: p.new_name,
        };
        if a.new_name.trim().is_empty() {
            return Err("invalid input: new_name is empty".into());
        }
        let path = ctx.resolve(&a.at.path);
        let summary = format!(
            "plan renaming `{}` to `{}` at {}:{}",
            a.at.symbol,
            a.new_name,
            path.display(),
            a.at.line
        );
        plan_read(&self.0, path, summary)
    }

    fn run_async(&self, input: &Value, ctx: &ToolCtx) -> AsyncRun {
        let (board, input, ctx) = (self.0.clone(), input.clone(), ctx.clone());
        Box::pin(async move {
            let p: PlanArgs = parse(&input).map_err(ToolFailure::new)?;
            let a = Plain {
                at: p.at(),
                new_name: p.new_name,
            };
            let (path, pos, spec, live) = locate(&board, &ctx, &a.at).await?;
            let c = &live.client;
            // Rename where the symbol is defined, when that is one place in
            // the workspace: TypeScript renames an imported name at its
            // import site, and the definition's rename reaches every use.
            let mut at = (path.clone(), pos);
            let mut moved = None;
            if c.capabilities().offers("definitionProvider") {
                if let Ok(defs) = board
                    .call(&live, "textDocument/definition", c.definition(&path, pos))
                    .await
                {
                    if let [d] = defs.as_slice() {
                        let dp = theseus_lsp::uri::to_path(&d.uri);
                        let inside = dp
                            .as_ref()
                            .is_some_and(|p| ctx.roots.iter().any(|r| p.starts_with(r)));
                        if let (Some(dp), true) = (dp, inside) {
                            let here = dp == path && d.range.start.line == pos.line;
                            if !here {
                                moved =
                                    Some(format!("{}:{}", dp.display(), d.range.start.line + 1));
                                at = (dp, d.range.start);
                            }
                        }
                    }
                }
            }
            let edit = board
                .call(
                    &live,
                    "textDocument/rename",
                    c.rename(&at.0, at.1, &a.new_name),
                )
                .await?
                .ok_or_else(|| format!("{} will not rename `{}` there", spec.name, a.at.symbol))?;
            if !edit.operations().is_empty() {
                return Err(ToolFailure::new(format!(
                    "{}'s rename would also create, rename, or delete files, which lsp.rename does \
                     not do: rename them with fs and proc tools, then the symbol",
                    spec.name
                )));
            }
            let mut files = files_of(&edit).await?;
            files.sort_by(|x, y| x.path.cmp(&y.path));
            if files.is_empty() {
                return Err(ToolFailure::new(format!(
                    "{}'s rename of `{}` changes nothing",
                    spec.name, a.at.symbol
                )));
            }
            let d = digest(&files);
            let diff = diff_of(&files);
            let edits: usize = files.iter().map(|f| f.edits).sum();
            let names: Vec<String> = files.iter().map(|f| f.path.display().to_string()).collect();
            let mut text = format!(
                "Rename `{}` to `{}` ({}): {} in {}.\n",
                a.at.symbol,
                a.new_name,
                spec.name,
                crate::narrative::count(edits as u64, "edit", "edits"),
                crate::narrative::count(files.len() as u64, "file", "files"),
            );
            if let Some(m) = &moved {
                text.push_str(&format!("Asked at its definition, {m}.\n"));
            }
            text.push_str(&diff);
            text.push_str(&format!(
                "\nNothing is written yet. Apply exactly this with lsp_rename {{\"digest\": \"{d}\"}}.\n"
            ));
            board.renames.put(
                &d,
                RenameShown {
                    symbol: a.at.symbol.clone(),
                    new_name: a.new_name.clone(),
                    server: spec.name.clone(),
                    files,
                    diff,
                },
            );
            let meta = json!({"server": spec.name, "root": live.root(), "digest": d,
                "files": names, "edits": edits, "definition": moved});
            Ok((ToolOutput { text, meta }, None))
        })
    }
}

/// Each file the server's edit changes: as it is now, and after.
async fn files_of(edit: &theseus_lsp::types::WorkspaceEdit) -> Result<Vec<FileEdit>, ToolFailure> {
    let mut by_file: HashMap<PathBuf, Vec<TextEdit>> = HashMap::new();
    for (uri, _, edits) in edit.text_edits() {
        let p = theseus_lsp::uri::to_path(&uri)
            .ok_or_else(|| format!("the rename names {uri}, which is not a file"))?;
        by_file.entry(p).or_default().extend(edits);
    }
    let mut files = Vec::new();
    for (p, edits) in by_file {
        let before = tokio::fs::read_to_string(&p)
            .await
            .map_err(|e| format!("{}: {e}", p.display()))?;
        let after = apply(&before, &edits).map_err(|e| format!("{}: {e}", p.display()))?;
        if after != before {
            files.push(FileEdit {
                path: p,
                before,
                after,
                edits: edits.len(),
            });
        }
    }
    Ok(files)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplyArgs {
    digest: String,
}

pub(super) struct Rename(pub Arc<Board>);

impl Rename {
    fn shown(&self, input: &Value) -> Result<(RenameShown, String), String> {
        let a: ApplyArgs = parse(input)?;
        let s = self.0.renames.get(&a.digest).ok_or_else(|| {
            format!(
                "no rename plan in this daemon's life has digest {}: run lsp_rename_plan, and \
                 apply the digest it returns",
                a.digest
            )
        })?;
        Ok((s, a.digest))
    }
}

impl Tool for Rename {
    fn name(&self) -> &'static str {
        "lsp.rename"
    }

    fn description(&self) -> &'static str {
        "Apply the rename an lsp_rename_plan showed, by its digest: exactly that edit, to every \
         file it named, and only if none changed since. Returns the diff it wrote."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "digest": {"type": "string", "description": "The plan's digest, as lsp_rename_plan returned it."}
            },
            "required": ["digest"]
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Write
    }

    fn backend(&self) -> Backend {
        Backend::Async
    }

    fn retry(&self) -> Retry {
        Retry::NonRepeatable
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let (s, digest) = self.shown(input)?;
        let names: Vec<String> = s
            .files
            .iter()
            .map(|f| f.path.display().to_string())
            .collect();
        Ok(Plan {
            resources: s
                .files
                .iter()
                .map(|f| Resource {
                    path: f.path.clone(),
                    access: Access::Write,
                })
                .collect(),
            summary: format!(
                "rename `{}` to `{}` ({}, plan {digest}): write {}",
                s.symbol,
                s.new_name,
                s.server,
                names.join(", ")
            ),
            ..Default::default()
        })
    }

    fn run_async(&self, input: &Value, ctx: &ToolCtx) -> AsyncRun {
        let shown = self.shown(input);
        let (board, umask) = (self.0.clone(), ctx.umask);
        Box::pin(async move {
            let (s, digest) = shown.map_err(ToolFailure::new)?;
            for f in &s.files {
                let now = tokio::fs::read_to_string(&f.path).await.unwrap_or_default();
                if sha(&now) != sha(&f.before) {
                    return Err(ToolFailure::new(format!(
                        "{} changed since plan {digest} read it: nothing was written. Plan the \
                         rename again.",
                        f.path.display()
                    )));
                }
            }
            let files = s.files.clone();
            let wrote = tokio::task::spawn_blocking(move || {
                let mut wrote = Vec::new();
                for f in &files {
                    theseus_tools::fs::write_atomic(&f.path, f.after.as_bytes(), umask)
                        .map_err(|e| (wrote.clone(), format!("{}: {e}", f.path.display())))?;
                    wrote.push(f.path.clone());
                }
                Ok::<_, (Vec<PathBuf>, String)>(wrote)
            })
            .await
            .map_err(|e| ToolFailure::new(e.to_string()))?;
            board.renames.take(&digest);
            let wrote = match wrote {
                Ok(w) => w,
                Err((w, why)) => {
                    told(&board, &w).await;
                    let names: Vec<String> = w.iter().map(|p| p.display().to_string()).collect();
                    return Err(ToolFailure::new(format!(
                        "the rename stopped at {why}; it had written {}",
                        if names.is_empty() {
                            "nothing".into()
                        } else {
                            names.join(", ")
                        }
                    )));
                }
            };
            told(&board, &wrote).await;
            let names: Vec<String> = wrote.iter().map(|p| p.display().to_string()).collect();
            let text = format!(
                "Renamed `{}` to `{}`: wrote {}.\n{}",
                s.symbol,
                s.new_name,
                names.join(", "),
                s.diff
            );
            Ok((
                ToolOutput {
                    text,
                    meta: json!({"digest": digest, "wrote": names}),
                },
                None,
            ))
        })
    }
}

/// Each server up whose root holds a written file hears of it.
async fn told(board: &Board, wrote: &[PathBuf]) {
    for l in board.up() {
        for p in wrote.iter().filter(|p| p.starts_with(l.root())) {
            let _ = l.client.file_changed(p).await;
        }
    }
}

/// The gate's step for `lsp.rename`: a write outside the workspace roots
/// waits, as the brief for the rename asks (an `fs.patch` there takes its
/// posture; a rename reaches files the model never named).
pub(super) fn outside_roots(roots: &[PathBuf], tool: &str, plan: &Plan, d: Decision) -> Decision {
    if tool != "lsp.rename" {
        return d;
    }
    let outside = plan.resources.iter().find(|r| {
        r.access == Access::Write
            && !roots
                .iter()
                .any(|root| theseus_tools::paths::within(&r.path, root))
    });
    match outside {
        Some(r) => d.at_least(
            Posture::Approve,
            &format!("{} is outside the workspace roots", r.path.display()),
            "lsp.rename: a write outside the workspace roots waits",
            tool,
            &plan.summary,
        ),
        None => d,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_lsp::types::Range;

    fn edit(l0: u32, c0: u32, l1: u32, c1: u32, t: &str) -> TextEdit {
        TextEdit {
            range: Range {
                start: Position::new(l0, c0),
                end: Position::new(l1, c1),
            },
            new_text: t.into(),
        }
    }

    /// Edits at UTF-16 columns, in any order, become one text; overlapping
    /// ones are refused.
    #[test]
    fn edits_apply_at_utf16_columns_in_any_order() {
        let text = "fn é_total() {}\nlet x = é_total();\n";
        let out = apply(text, &[edit(1, 8, 1, 15, "sum"), edit(0, 3, 0, 10, "sum")]).unwrap();
        assert_eq!(out, "fn sum() {}\nlet x = sum();\n");
        assert!(apply(text, &[edit(0, 0, 0, 5, "a"), edit(0, 3, 0, 6, "b")]).is_err());
        assert_eq!(offset("ab\ncd", Position::new(9, 0)), 5);
    }
}
