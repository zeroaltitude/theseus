//! Diagnostics in edit results (L3, theseus-n88g.9): when `fs.write`,
//! `fs.edit`, `fs.patch`, or `lsp.rename` changes a file, its result carries
//! the errors the file's server now reports, so the model fixes them in its
//! next call instead of after a failed build.
//!
//! - **After the toollet**, in `toolrun`'s `run_inproc`, once its result is
//!   in hand and before its node is built: the wait is async, on the turn's
//!   task, never inside the toollet or on its core.
//! - **Each file written is announced** to the server for its root
//!   (`file_changed`, after a sync of the other open documents), and the call
//!   waits up to the edit bound for that file's diagnostics. Only where a
//!   server for that root is up already.
//! - **The block** follows the result's text: "Errors after this edit:",
//!   each file's errors first, then its warnings, at most [`MAX_LINES`]
//!   lines with a count of the rest, and the count of new errors the server
//!   pushed for other files during the wait. A file the bound beat is said
//!   to be pending.
//! - **The record**: the block rides in the result node, in its
//!   completion's frame, so no frame is added; the result's `meta.lsp` says
//!   what was attached, for L5 to count from the record.
//! - **The place rule**: a server reads the whole project, so a shared
//!   place's edit gets no block (`toolrun` asks only in a private place).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use theseus_lsp::types::Diagnostic;
use theseus_lsp::{FileDiagnostics, Freshness};
use theseus_tools::ToolCtx;

use super::tools::{lines_of, place};
use super::{lock, Board, Key, Live};

/// The tools whose success writes files, and so carries the block.
pub const EDITS: [&str; 4] = ["fs.write", "fs.edit", "fs.patch", "lsp.rename"];

/// The longest the block is, in diagnostic lines.
pub const MAX_LINES: usize = 20;

/// How long an edit waits for its files' diagnostics.
pub const EDIT_WAIT: Duration = Duration::from_millis(1500);

impl crate::toolrun::ToolRuntime {
    /// `run_inproc`'s hook: the block onto a result's text and `meta.lsp`,
    /// in a private place only (the place rule). `status` is `None` for a
    /// call a cancel settled, which gets none.
    pub(crate) async fn lsp_onto(
        &self,
        tc: &crate::toolrun::TurnCtx<'_>,
        tool_use_id: &str,
        tool: &str,
        status: Option<crate::node::ResultStatus>,
        text: &mut String,
        meta: &mut Value,
    ) {
        let (Some(board), Some(status)) = (&self.lsp, status) else {
            return;
        };
        if tc.class != crate::places::PlaceClass::Private {
            return;
        }
        let ok = status == crate::node::ResultStatus::Ok;
        if let Some(a) = board.attach(tool_use_id, tool, ok, meta, &self.ctx).await {
            text.push_str(&a.text);
            meta["lsp"] = a.meta;
        }
    }
}

/// The files an edit's result says it wrote (`meta.path`; `fs.patch`'s
/// `meta.files[].path`; `lsp.rename`'s `meta.wrote`), resolved as its call's.
pub fn written(tool: &str, meta: &Value, ctx: &ToolCtx) -> Vec<PathBuf> {
    let strs: Vec<&str> = match tool {
        "fs.write" | "fs.edit" => meta
            .get("path")
            .and_then(Value::as_str)
            .into_iter()
            .collect(),
        "fs.patch" => meta["files"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|f| f.get("path").and_then(Value::as_str))
            .collect(),
        "lsp.rename" => meta["wrote"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect(),
        _ => Vec::new(),
    };
    let mut seen = BTreeSet::new();
    strs.into_iter()
        .map(|s| ctx.resolve(s))
        .filter(|p| seen.insert(p.clone()))
        .collect()
}

/// What the hook adds to a result: its text, and `meta.lsp`.
#[derive(Debug, Clone)]
pub struct Attached {
    pub text: String,
    pub meta: Value,
}

/// What became of one file's wait.
#[derive(Debug)]
enum Got {
    Diagnostics(FileDiagnostics),
    /// The bound ran out first.
    Pending,
    Failed(String),
}

struct File {
    path: PathBuf,
    server: String,
    got: Got,
}

impl Board {
    /// The block for a call's result, when there is one: for a successful
    /// edit, its files' diagnostics. `toolrun` asks only in a private place.
    pub async fn attach(
        self: &Arc<Self>,
        tool_use_id: &str,
        tool: &str,
        ok: bool,
        meta: &Value,
        ctx: &ToolCtx,
    ) -> Option<Attached> {
        if !ok || !EDITS.contains(&tool) {
            return None;
        }
        let paths = written(tool, meta, ctx);
        self.after_edit(tool_use_id, &paths).await
    }

    /// The server up for `key`, if one is.
    fn up_for(&self, key: &Key) -> Option<Arc<Live>> {
        lock(&self.up)
            .get(key)
            .filter(|l| l.client.closed().is_none())
            .cloned()
    }

    /// Announce each file to its root's server, wait within the edit bound
    /// for its diagnostics, and render them.
    pub async fn after_edit(
        self: &Arc<Self>,
        tool_use_id: &str,
        paths: &[PathBuf],
    ) -> Option<Attached> {
        let start = tokio::time::Instant::now();
        let deadline = start + EDIT_WAIT;
        // Each file's server, where one is up for its root.
        let mut groups: BTreeMap<Key, (Arc<Live>, Vec<PathBuf>)> = BTreeMap::new();
        for p in paths {
            let Ok((spec, root)) = self.server_for(p) else {
                continue;
            };
            let key: Key = (spec.name.clone(), root);
            let Some(live) = self.up_for(&key) else {
                continue;
            };
            groups
                .entry(key)
                .or_insert((live, Vec::new()))
                .1
                .push(p.clone());
        }
        if groups.is_empty() {
            return None;
        }
        let mut waits = Vec::new();
        let mut before = Vec::new();
        for (live, files) in groups.values() {
            before.push((live.clone(), live.client.pushed_errors(), files.clone()));
            // The other open documents first, so what the server says of
            // them during the wait is counted.
            let _ = live.client.sync_disk().await;
            for file in files {
                let server = live.server().to_string();
                let (board, live, path) = (self.clone(), live.clone(), file.clone());
                let task = tokio::spawn(async move {
                    let _ = live.client.file_changed(&path).await;
                    if tokio::fs::metadata(&path).await.is_err() {
                        return None;
                    }
                    let timeout = board.request_timeout();
                    Some(
                        board
                            .call(
                                &live,
                                "textDocument/diagnostic",
                                live.client.diagnostics(&path, timeout),
                            )
                            .await,
                    )
                });
                waits.push((server, file.clone(), task));
            }
        }
        let mut files = Vec::new();
        for (server, path, mut task) in waits {
            let got = match tokio::time::timeout_at(deadline, &mut task).await {
                Ok(Ok(None)) => continue,
                Ok(Ok(Some(Ok(d)))) => Got::Diagnostics(d),
                Ok(Ok(Some(Err(why)))) => Got::Failed(why),
                Ok(Err(join)) => Got::Failed(join.to_string()),
                Err(_) => {
                    task.abort();
                    Got::Pending
                }
            };
            self.bind(task.id(), tool_use_id);
            files.push(File { path, server, got });
        }
        let mut others = 0;
        for (live, was, edited) in &before {
            others += new_errors(was, &live.client.pushed_errors(), &live.client, edited);
        }
        if files.is_empty() {
            return None;
        }
        Some(render(&files, others, start.elapsed()).await)
    }
}

/// The errors pushed for documents other than `edited` that were not there
/// before: each document's rise in its count.
fn new_errors(
    before: &BTreeMap<String, usize>,
    after: &BTreeMap<String, usize>,
    client: &theseus_lsp::Client,
    edited: &[PathBuf],
) -> usize {
    let edited: BTreeSet<String> = edited.iter().filter_map(|p| client.uri(p).ok()).collect();
    after
        .iter()
        .filter(|(u, _)| !edited.contains(*u))
        .map(|(u, n)| n.saturating_sub(before.get(u).copied().unwrap_or(0)))
        .sum()
}

fn freshness(got: &Got) -> &'static str {
    match got {
        Got::Diagnostics(d) => match d.freshness {
            Freshness::Pulled => "pulled",
            Freshness::Pushed => "pushed",
            Freshness::Stale => "stale",
        },
        Got::Pending => "pending",
        Got::Failed(_) => "failed",
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// A file's errors, then its warnings: what the block shows of it.
fn shown(items: &[Diagnostic]) -> Vec<&Diagnostic> {
    let warning = |d: &&Diagnostic| d.severity == Some(theseus_lsp::types::Severity::WARNING);
    let errors = items.iter().filter(|d| d.is_error());
    errors.chain(items.iter().filter(warning)).collect()
}

/// The block, and `meta.lsp`.
async fn render(files: &[File], others: usize, waited: Duration) -> Attached {
    let mut out = vec![String::new(), "Errors after this edit:".to_string()];
    let (mut lines, mut errors_all, mut errors_shown, mut warnings_all) = (0, 0, 0, 0);
    let mut hidden = 0;
    let mut each = Vec::new();
    for f in files {
        let (errors, warnings) = match &f.got {
            Got::Diagnostics(d) => (
                d.errors().count(),
                d.items
                    .iter()
                    .filter(|x| x.severity == Some(theseus_lsp::types::Severity::WARNING))
                    .count(),
            ),
            _ => (0, 0),
        };
        each.push(json!({
            "path": f.path, "server": f.server, "freshness": freshness(&f.got),
            "errors": errors, "warnings": warnings,
        }));
        let name = f.path.display();
        match &f.got {
            Got::Pending => out.push(format!(
                "{name}: pending: {} had not answered for it within {} ms",
                f.server,
                EDIT_WAIT.as_millis()
            )),
            Got::Failed(why) => out.push(format!("{name}: {} failed: {why}", f.server)),
            Got::Diagnostics(d) => {
                errors_all += errors;
                warnings_all += warnings;
                let stale = match d.freshness {
                    Freshness::Stale => " (stale: the server had not answered for this text)",
                    _ => "",
                };
                let mut head = match errors {
                    0 => "no errors".to_string(),
                    n => plural(n, "error", "errors"),
                };
                if warnings > 0 {
                    head.push_str(&format!(", {}", plural(warnings, "warning", "warnings")));
                }
                out.push(format!("{name} ({}): {head}{stale}", f.server));
                let text = lines_of(&f.path).await;
                for item in shown(&d.items) {
                    if lines >= MAX_LINES {
                        hidden += 1;
                        continue;
                    }
                    lines += 1;
                    if item.is_error() {
                        errors_shown += 1;
                    }
                    out.push(line(&f.path, item, &text));
                }
            }
        }
    }
    if hidden > 0 {
        out.push(format!(
            "…[{hidden} more not shown: lsp_diagnostics with a path lists a file's]"
        ));
    }
    if others > 0 {
        out.push(format!(
            "Other files: {others} new {} during this edit (lsp_diagnostics lists the open files').",
            if others == 1 { "error" } else { "errors" }
        ));
    }
    let first = files.first().map(|f| f.server.clone()).unwrap_or_default();
    let fresh: BTreeSet<&str> = files.iter().map(|f| freshness(&f.got)).collect();
    let meta = json!({
        "server": first,
        "freshness": match fresh.len() { 1 => fresh.first().copied().unwrap_or(""), _ => "mixed" },
        "errors": errors_all,
        "errors_shown": errors_shown,
        "warnings": warnings_all,
        "other_errors": others,
        "waited_ms": waited.as_millis() as u64,
        "files": each,
    });
    Attached {
        text: out.join("\n"),
        meta,
    }
}

/// `  path:line:col error [code]: message (source)`.
fn line(path: &Path, item: &Diagnostic, text: &[String]) -> String {
    let sev = item.severity.map_or("error", |s| s.name());
    let code = item
        .code_text()
        .map(|c| format!(" [{c}]"))
        .unwrap_or_default();
    let src = item
        .source
        .as_deref()
        .map(|s| format!(" ({s})"))
        .unwrap_or_default();
    format!(
        "  {} {sev}{code}: {}{src}",
        place(path, &item.range, text),
        item.message
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diag(line: u32, severity: u8, message: &str) -> Diagnostic {
        serde_json::from_value(json!({
            "range": {"start": {"line": line, "character": 0}, "end": {"line": line, "character": 1}},
            "severity": severity, "message": message,
        }))
        .unwrap()
    }

    /// Errors first, then warnings; hints and notes are left out; past
    /// twenty lines, a count of the rest.
    #[tokio::test]
    async fn the_block_shows_errors_first_and_is_capped() {
        let mut items = vec![diag(0, 2, "a warning"), diag(1, 4, "a hint")];
        items.extend((0..25).map(|i| diag(i, 1, "an error")));
        let d = FileDiagnostics {
            uri: "file:///w/a.py".into(),
            version: 2,
            items,
            freshness: Freshness::Pushed,
            waited: Duration::from_millis(3),
        };
        let files = [File {
            path: PathBuf::from("/w/a.py"),
            server: "ty".into(),
            got: Got::Diagnostics(d),
        }];
        let a = render(&files, 2, Duration::from_millis(3)).await;
        let lines: Vec<&str> = a.text.lines().collect();
        assert_eq!(lines[1], "Errors after this edit:");
        assert_eq!(lines[2], "/w/a.py (ty): 25 errors, 1 warning");
        let shown: Vec<&&str> = lines.iter().filter(|l| l.starts_with("  ")).collect();
        assert_eq!(shown.len(), MAX_LINES);
        assert!(
            shown.iter().all(|l| l.contains(" error: an error")),
            "{}",
            a.text
        );
        assert!(!a.text.contains("hint"));
        assert!(a.text.contains("…[6 more not shown"), "{}", a.text);
        assert!(a.text.contains("Other files: 2 new errors"), "{}", a.text);
        assert_eq!(a.meta["errors"], 25);
        assert_eq!(a.meta["errors_shown"], 20);
        assert_eq!(a.meta["other_errors"], 2);
        assert_eq!(a.meta["freshness"], "pushed");
    }

    /// The files an edit wrote, from its meta, each once.
    #[test]
    fn an_edits_files_are_read_from_its_meta() {
        let ctx = ToolCtx::for_tests(Path::new("/w"));
        assert_eq!(
            written("fs.edit", &json!({"path": "/w/a.py"}), &ctx),
            [PathBuf::from("/w/a.py")]
        );
        let patch = json!({"files": [{"path": "/w/a.py"}, {"path": "/w/b.py", "deleted": true}, {"path": "/w/a.py"}]});
        assert_eq!(
            written("fs.patch", &patch, &ctx),
            [PathBuf::from("/w/a.py"), PathBuf::from("/w/b.py")]
        );
        assert_eq!(
            written("lsp.rename", &json!({"wrote": ["/w/c.rs"]}), &ctx),
            [PathBuf::from("/w/c.rs")]
        );
        assert!(written("fs.read", &json!({"path": "/w/a.py"}), &ctx).is_empty());
    }
}
