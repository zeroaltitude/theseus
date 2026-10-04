//! The `lsp.*` read tools (L2): definition, references, hover, symbols, and
//! diagnostics, each a read, async, addressed by a path, a 1-based line, and
//! the symbol's text on it (with `occurrence` when it is there more than
//! once), never a column (`theseus_lsp::locate`).
//!
//! Each runs on its file's server ([`Board::server_for`]), started at the
//! first call for its root; the gate judges that start as `proc.run`
//! ([`super::gate`]). Paths in answers are absolute, and lines 1-based, as
//! `fs.read` numbers them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_lsp::types::{Location, Range};
use theseus_tools::{
    parse, Access, AsyncRun, Backend, Plan, Resource, Retry, Tool, ToolClass, ToolCtx, ToolFailure,
    ToolOutput,
};

use super::{Board, Live, Spec};

pub(super) fn all(board: &Arc<Board>) -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(Nav {
            board: board.clone(),
            kind: Kind::Definition,
        }),
        Arc::new(Nav {
            board: board.clone(),
            kind: Kind::References,
        }),
        Arc::new(Nav {
            board: board.clone(),
            kind: Kind::Hover,
        }),
        Arc::new(Symbols(board.clone())),
        Arc::new(Diagnostics(board.clone())),
        Arc::new(super::rename::RenamePlan(board.clone())),
        Arc::new(super::rename::Rename(board.clone())),
    ]
}

/// Where a symbol is: the tools' addressing.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct At {
    pub path: String,
    pub line: u32,
    pub symbol: String,
    #[serde(default)]
    pub occurrence: Option<u32>,
    /// `lsp.references`: count the declaration as one.
    #[serde(default = "yes")]
    pub include_declaration: bool,
}

fn yes() -> bool {
    true
}

pub(super) fn at_schema(extra: Value) -> Value {
    let mut s = json!({
        "type": "object",
        "properties": {
            "path": {"type": "string", "description": "The file, absolute or relative to the working directory."},
            "line": {"type": "integer", "minimum": 1, "description": "The 1-based line the symbol is on, as fs_read numbers it."},
            "symbol": {"type": "string", "description": "The symbol's text on that line, exactly (a name, not a column)."},
            "occurrence": {"type": "integer", "minimum": 1, "description": "Which match on the line, from 1, when the symbol is on it more than once."}
        },
        "required": ["path", "line", "symbol"]
    });
    if let (Some(p), Value::Object(e)) = (s["properties"].as_object_mut(), extra) {
        p.extend(e);
    }
    s
}

/// The deadline of a call that may start its server: the start, the wait
/// for readiness, and the request, each within the request timeout.
pub(super) fn deadline(board: &Board) -> std::time::Duration {
    board.request_timeout() * 3 + std::time::Duration::from_secs(30)
}

/// A read of `path`, whose server must exist.
pub(super) fn plan_read(board: &Board, path: PathBuf, summary: String) -> Result<Plan, String> {
    board.server_for(&path)?;
    Ok(Plan {
        resources: vec![Resource {
            path,
            access: Access::Read,
        }],
        summary,
        ..Default::default()
    })
}

/// The server and position for `a`: its file read, the symbol located, the
/// server started if it is not up.
pub(super) async fn locate(
    board: &Arc<Board>,
    ctx: &ToolCtx,
    a: &At,
) -> Result<(PathBuf, theseus_lsp::types::Position, Spec, Arc<Live>), ToolFailure> {
    let path = ctx.resolve(&a.path);
    let (spec, root) = board.server_for(&path)?;
    let text = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let pos = theseus_lsp::locate(&text, a.line, &a.symbol, a.occurrence)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let live = board.live(&spec, &root).await?;
    live.opened(&path);
    Ok((path, pos, spec, live))
}

/// The lines of a file, read now; empty when it cannot be read.
pub(super) async fn lines_of(path: &Path) -> Vec<String> {
    tokio::fs::read_to_string(path)
        .await
        .map(|t| t.lines().map(String::from).collect())
        .unwrap_or_default()
}

fn path_of(uri: &str) -> PathBuf {
    theseus_lsp::uri::to_path(uri).unwrap_or_else(|| PathBuf::from(uri))
}

/// `path:line:column`, 1-based.
pub(super) fn place(path: &Path, r: &Range, lines: &[String]) -> String {
    let row = lines.get(r.start.line as usize).map_or("", String::as_str);
    let col = theseus_lsp::position::byte_column(row, r.start.character);
    let col = row[..col.min(row.len())].chars().count() + 1;
    format!("{}:{}:{col}", path.display(), r.start.line + 1)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Definition,
    References,
    Hover,
}

/// The lines around a definition: before and after.
const AROUND: (usize, usize) = (2, 4);
/// The most locations a definition shows.
const MAX_DEFINITIONS: usize = 10;
/// The most references shown.
const MAX_REFERENCES: usize = 200;

struct Nav {
    board: Arc<Board>,
    kind: Kind,
}

impl Tool for Nav {
    fn name(&self) -> &'static str {
        match self.kind {
            Kind::Definition => "lsp.definition",
            Kind::References => "lsp.references",
            Kind::Hover => "lsp.hover",
        }
    }

    fn description(&self) -> &'static str {
        match self.kind {
            Kind::Definition => {
                "Where a symbol is defined, from the file's language server: each location with a \
                 few lines around it. Name the symbol by its file, its 1-based line, and its text \
                 on that line."
            }
            Kind::References => {
                "Every reference to a symbol, from the file's language server, grouped by file, \
                 each with its line. Name the symbol by its file, its 1-based line, and its text \
                 on that line."
            }
            Kind::Hover => {
                "What the file's language server says of a symbol: its type, signature, and \
                 documentation. Name the symbol by its file, its 1-based line, and its text on \
                 that line."
            }
        }
    }

    fn input_schema(&self) -> Value {
        match self.kind {
            Kind::References => at_schema(json!({
                "include_declaration": {"type": "boolean", "description": "Count the declaration as a reference (default true)."}
            })),
            _ => {
                let mut s = at_schema(json!({}));
                s["properties"]
                    .as_object_mut()
                    .map(|p| p.remove("include_declaration"));
                s
            }
        }
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
        Some(deadline(&self.board))
    }

    fn plan(&self, input: &Value, ctx: &ToolCtx) -> Result<Plan, String> {
        let a: At = parse(input)?;
        if self.kind != Kind::References && input.get("include_declaration").is_some() {
            return Err("invalid input: include_declaration is lsp.references' alone".into());
        }
        let verb = match self.kind {
            Kind::Definition => "definition of",
            Kind::References => "references to",
            Kind::Hover => "hover on",
        };
        let path = ctx.resolve(&a.path);
        let summary = format!("{verb} `{}` at {}:{}", a.symbol, path.display(), a.line);
        plan_read(&self.board, path, summary)
    }

    fn run_async(&self, input: &Value, ctx: &ToolCtx) -> AsyncRun {
        let (board, input, ctx, kind) = (self.board.clone(), input.clone(), ctx.clone(), self.kind);
        Box::pin(async move {
            let a: At = parse(&input).map_err(ToolFailure::new)?;
            let (path, pos, spec, live) = locate(&board, &ctx, &a).await?;
            let c = &live.client;
            let meta = |n: usize| json!({"server": spec.name, "root": live.root(), "count": n});
            let text = match kind {
                Kind::Definition => {
                    let locs = board
                        .call(&live, "textDocument/definition", c.definition(&path, pos))
                        .await?;
                    let text = definitions(&a, &locs).await;
                    return Ok((
                        ToolOutput {
                            text,
                            meta: meta(locs.len()),
                        },
                        None,
                    ));
                }
                Kind::References => {
                    let locs = board
                        .call(
                            &live,
                            "textDocument/references",
                            c.references(&path, pos, a.include_declaration),
                        )
                        .await?;
                    let text = references(&a, &locs).await;
                    return Ok((
                        ToolOutput {
                            text,
                            meta: meta(locs.len()),
                        },
                        None,
                    ));
                }
                Kind::Hover => board
                    .call(&live, "textDocument/hover", c.hover(&path, pos))
                    .await?
                    .map(|h| h.text())
                    .filter(|t| !t.trim().is_empty())
                    .unwrap_or_else(|| {
                        format!("{} says nothing of `{}` there.", spec.name, a.symbol)
                    }),
            };
            Ok((
                ToolOutput {
                    text,
                    meta: meta(1),
                },
                None,
            ))
        })
    }
}

/// Each definition with the lines around it, numbered.
async fn definitions(a: &At, locs: &[Location]) -> String {
    if locs.is_empty() {
        return format!("No definition found for `{}`.", a.symbol);
    }
    let mut out = Vec::new();
    for l in locs.iter().take(MAX_DEFINITIONS) {
        let path = path_of(&l.uri);
        let lines = lines_of(&path).await;
        let at = l.range.start.line as usize;
        let mut block = vec![place(&path, &l.range, &lines)];
        let from = at.saturating_sub(AROUND.0);
        let to = (at + AROUND.1 + 1).min(lines.len());
        for (i, line) in lines.iter().enumerate().take(to).skip(from) {
            let mark = if i == at { '>' } else { ' ' };
            block.push(format!("{mark}{:>5}  {line}", i + 1));
        }
        out.push(block.join("\n"));
    }
    if locs.len() > MAX_DEFINITIONS {
        out.push(format!(
            "…[{} more definitions not shown]",
            locs.len() - MAX_DEFINITIONS
        ));
    }
    out.join("\n\n")
}

/// References grouped by file, in file order, each with its line's text.
async fn references(a: &At, locs: &[Location]) -> String {
    if locs.is_empty() {
        return format!("No references found to `{}`.", a.symbol);
    }
    let mut by_file: BTreeMap<PathBuf, Vec<&Range>> = BTreeMap::new();
    for l in locs {
        by_file.entry(path_of(&l.uri)).or_default().push(&l.range);
    }
    let mut out = vec![format!(
        "{} to `{}` in {}:",
        crate::narrative::count(locs.len() as u64, "reference", "references"),
        a.symbol,
        crate::narrative::count(by_file.len() as u64, "file", "files"),
    )];
    let mut shown = 0;
    for (path, mut ranges) in by_file {
        if shown >= MAX_REFERENCES {
            break;
        }
        ranges.sort_by_key(|r| (r.start.line, r.start.character));
        let lines = lines_of(&path).await;
        out.push(format!("{} ({})", path.display(), ranges.len()));
        for r in ranges {
            if shown >= MAX_REFERENCES {
                break;
            }
            shown += 1;
            let text = lines.get(r.start.line as usize).map_or("", |l| l.trim());
            out.push(format!("{:>7}: {text}", r.start.line + 1));
        }
    }
    if locs.len() > shown {
        out.push(format!(
            "…[{} more references not shown: lsp_references on a narrower symbol, or fs_grep in one file, shows them]",
            locs.len() - shown
        ));
    }
    out.join("\n")
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SymbolsArgs {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    query: Option<String>,
}

/// The most symbols shown.
const MAX_SYMBOLS: usize = 300;

struct Symbols(Arc<Board>);

impl Tool for Symbols {
    fn name(&self) -> &'static str {
        "lsp.symbols"
    }

    fn description(&self) -> &'static str {
        "Symbols from a language server: {path} for a file's outline (each symbol, its kind, and \
         its line, nested as it is), or {query} for the workspace's symbols whose names match \
         (with a path, in that file's project; without, on the server up)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "A file: its outline, or, with query, the project to search."},
                "query": {"type": "string", "description": "Search the workspace's symbols by name."}
            }
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

    fn deadline(&self) -> Option<std::time::Duration> {
        Some(deadline(&self.0))
    }

    fn plan(&self, input: &Value, ctx: &ToolCtx) -> Result<Plan, String> {
        let a: SymbolsArgs = parse(input)?;
        match (&a.path, &a.query) {
            (None, None) => Err("invalid input: give a path (an outline) or a query".into()),
            (Some(p), q) => {
                let path = ctx.resolve(p);
                let summary = match q {
                    None => format!("outline of {}", path.display()),
                    Some(q) => format!("symbols matching `{q}` in {}'s project", path.display()),
                };
                plan_read(&self.0, path, summary)
            }
            (None, Some(q)) => Ok(Plan {
                summary: format!("workspace symbols matching `{q}`"),
                ..Default::default()
            }),
        }
    }

    fn run_async(&self, input: &Value, ctx: &ToolCtx) -> AsyncRun {
        let (board, input, ctx) = (self.0.clone(), input.clone(), ctx.clone());
        Box::pin(async move {
            let a: SymbolsArgs = parse(&input).map_err(ToolFailure::new)?;
            let live =
                match &a.path {
                    Some(p) => {
                        let path = ctx.resolve(p);
                        let (spec, root) = board.server_for(&path)?;
                        let live = board.live(&spec, &root).await?;
                        if a.query.is_none() {
                            return outline(&board, &live, &path).await;
                        }
                        live
                    }
                    None => {
                        let mut up = board.up();
                        up.sort_by_key(|l| std::cmp::Reverse(*super::lock(&l.last_used)));
                        match up.len() {
                            0 => return Err(ToolFailure::new(
                                "no language server is up: give a path in the project to search",
                            )),
                            _ => up.swap_remove(0),
                        }
                    }
                };
            let q = a.query.unwrap_or_default();
            let found = board
                .call(&live, "workspace/symbol", live.client.workspace_symbols(&q))
                .await?;
            let mut out = vec![format!(
                "{} matching `{q}` in {} ({}):",
                crate::narrative::count(found.len() as u64, "symbol", "symbols"),
                live.root().display(),
                live.server()
            )];
            for s in found.iter().take(MAX_SYMBOLS) {
                let line = s
                    .location
                    .range
                    .map(|r| format!(":{}", r.start.line + 1))
                    .unwrap_or_default();
                let inside = s
                    .container_name
                    .as_deref()
                    .map(|c| format!(" in {c}"))
                    .unwrap_or_default();
                out.push(format!(
                    "{} {}{inside} — {}{line}",
                    s.kind.name(),
                    s.name,
                    path_of(&s.location.uri).display()
                ));
            }
            if found.len() > MAX_SYMBOLS {
                out.push(format!(
                    "…[{} more not shown: a longer query narrows them]",
                    found.len() - MAX_SYMBOLS
                ));
            }
            let meta = json!({"server": live.server(), "root": live.root(), "count": found.len()});
            Ok((
                ToolOutput {
                    text: out.join("\n"),
                    meta,
                },
                None,
            ))
        })
    }
}

/// A file's outline: each symbol, its kind, and its line, nested.
async fn outline(
    board: &Board,
    live: &Arc<Live>,
    path: &Path,
) -> Result<(ToolOutput, Option<theseus_tools::External>), ToolFailure> {
    live.opened(path);
    let s = board
        .call(
            live,
            "textDocument/documentSymbol",
            live.client.document_symbols(path),
        )
        .await?;
    let flat = s.flatten();
    let mut out = vec![format!(
        "{} ({}):",
        path.display(),
        crate::narrative::count(flat.len() as u64, "symbol", "symbols")
    )];
    for (depth, name, kind, r) in flat.iter().take(MAX_SYMBOLS) {
        out.push(format!(
            "{}{} {name} — line {}",
            "  ".repeat(*depth),
            kind.name(),
            r.start.line + 1
        ));
    }
    if flat.len() > MAX_SYMBOLS {
        out.push(format!(
            "…[{} more symbols not shown: lsp_symbols with a query finds one]",
            flat.len() - MAX_SYMBOLS
        ));
    }
    let meta = json!({"server": live.server(), "root": live.root(), "count": flat.len()});
    Ok((
        ToolOutput {
            text: out.join("\n"),
            meta,
        },
        None,
    ))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DiagnosticsArgs {
    #[serde(default)]
    path: Option<String>,
}

/// The most diagnostics shown.
const MAX_DIAGNOSTICS: usize = 200;

struct Diagnostics(Arc<Board>);

impl Tool for Diagnostics {
    fn name(&self) -> &'static str {
        "lsp.diagnostics"
    }

    fn description(&self) -> &'static str {
        "A language server's errors and warnings: {path} for one file's, checked against its text \
         now; without a path, those of every file the lsp tools opened on the servers up."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "The file to check."}
            }
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

    fn deadline(&self) -> Option<std::time::Duration> {
        Some(deadline(&self.0))
    }

    fn plan(&self, input: &Value, ctx: &ToolCtx) -> Result<Plan, String> {
        let a: DiagnosticsArgs = parse(input)?;
        match a.path {
            Some(p) => {
                let path = ctx.resolve(&p);
                let summary = format!("diagnostics of {}", path.display());
                plan_read(&self.0, path, summary)
            }
            None => Ok(Plan {
                summary: "diagnostics of the open files".into(),
                ..Default::default()
            }),
        }
    }

    fn run_async(&self, input: &Value, ctx: &ToolCtx) -> AsyncRun {
        let (board, input, ctx) = (self.0.clone(), input.clone(), ctx.clone());
        Box::pin(async move {
            let a: DiagnosticsArgs = parse(&input).map_err(ToolFailure::new)?;
            let mut files: Vec<(Arc<Live>, PathBuf)> = Vec::new();
            match a.path {
                Some(p) => {
                    let path = ctx.resolve(&p);
                    let (spec, root) = board.server_for(&path)?;
                    let live = board.live(&spec, &root).await?;
                    live.opened(&path);
                    files.push((live, path));
                }
                None => {
                    for l in board.up() {
                        for f in l.open_files() {
                            files.push((l.clone(), f));
                        }
                    }
                }
            }
            if files.is_empty() {
                return Ok((
                    ToolOutput {
                        text: "No file is open on a language server: give a path.".into(),
                        meta: json!({"count": 0}),
                    },
                    None,
                ));
            }
            let mut out = Vec::new();
            let (mut total, mut shown) = (0usize, 0usize);
            for (live, path) in &files {
                let d = board
                    .call(
                        live,
                        "textDocument/diagnostic",
                        live.client.diagnostics(path, board.request_timeout()),
                    )
                    .await?;
                total += d.items.len();
                let fresh = match d.freshness {
                    theseus_lsp::Freshness::Stale => {
                        " (stale: the server had not answered for this text in time)"
                    }
                    _ => "",
                };
                if d.items.is_empty() {
                    out.push(format!("{}: no diagnostics{fresh}", path.display()));
                    continue;
                }
                out.push(format!("{} ({}){fresh}:", path.display(), d.items.len()));
                let lines = lines_of(path).await;
                for item in &d.items {
                    if shown >= MAX_DIAGNOSTICS {
                        break;
                    }
                    shown += 1;
                    let sev = item.severity.map_or("note", |s| s.name());
                    let code = item
                        .code_text()
                        .map(|c| format!(" [{c}]"))
                        .unwrap_or_default();
                    let src = item
                        .source
                        .as_deref()
                        .map(|s| format!(" ({s})"))
                        .unwrap_or_default();
                    out.push(format!(
                        "  {} {sev}{code}: {}{src}",
                        place(path, &item.range, &lines),
                        item.message
                    ));
                }
            }
            if total > shown {
                out.push(format!(
                    "…[{} more not shown: one file's path narrows them]",
                    total - shown
                ));
            }
            Ok((
                ToolOutput {
                    text: out.join("\n"),
                    // The files it lists, which the edit hook leaves out of
                    // what arrived for the session's pending waits.
                    meta: json!({"count": total, "files": files.iter().map(|(_, p)| p).collect::<Vec<_>>()}),
                },
                None,
            ))
        })
    }
}
