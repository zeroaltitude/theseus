//! The terminal tools (theseus-n88g.4): `term.open`, `term.send`,
//! `term.read`, and `term.close`. Each plans as a toollet does, and runs as
//! an async tool through `Terms::run`, which knows its session.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_tools::{
    parse, Access, AsyncResult, AsyncRun, Backend, Plan, Resource, Retry, Tool, ToolClass, ToolCtx,
    ToolFailure, ToolOutput,
};

use super::{keys, Terminal, Terms, CLOSE, MAX_WAIT_MS, OPEN, READ, SEND};

/// The four tools, over one registry of terminals.
pub fn all(terms: &Arc<Terms>) -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(Open),
        Arc::new(Send(terms.clone())),
        Arc::new(Read),
        Arc::new(Close),
    ]
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenArgs {
    argv: Vec<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    rows: Option<u16>,
    #[serde(default)]
    cols: Option<u16>,
    /// How long to wait for the first screen to settle.
    #[serde(default)]
    quiet_ms: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SendArgs {
    terminal: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    keys: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadArgs {
    terminal: String,
    #[serde(default)]
    quiet_ms: Option<u64>,
    #[serde(default)]
    until: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CloseArgs {
    terminal: String,
}

const DEFAULT_SIZE: (u16, u16) = (24, 80);
/// An open's first screen waits for this much quiet, at most `OPEN_WAIT_MS`.
const OPEN_QUIET_MS: u64 = 300;
const OPEN_WAIT_MS: u64 = 3_000;
/// A read that waits and names no bound waits this long at most.
const READ_WAIT_MS: u64 = 10_000;

fn cwd_of(cwd: Option<&str>, ctx: &ToolCtx) -> std::path::PathBuf {
    cwd.map(|p| ctx.resolve(p))
        .unwrap_or_else(|| ctx.cwd.clone())
}

fn size_of(a: &OpenArgs) -> Result<(u16, u16), String> {
    let rows = a.rows.unwrap_or(DEFAULT_SIZE.0);
    let cols = a.cols.unwrap_or(DEFAULT_SIZE.1);
    if !(2..=200).contains(&rows) || !(10..=500).contains(&cols) {
        return Err("rows must be 2 to 200, and cols 10 to 500".into());
    }
    Ok((rows, cols))
}

/// The bytes `term.send` types, and its text alone: the text first, then
/// each named key.
fn send_bytes(a: &SendArgs) -> Result<(Vec<u8>, String), String> {
    let text = a.text.clone().unwrap_or_default();
    let mut bytes = keys::text(&text);
    for k in &a.keys {
        bytes.extend(keys::bytes(k).ok_or_else(|| {
            format!(
                "unknown key {k:?}: name one of Enter, Tab, Escape, Backspace, Delete, Space, Up, \
                 Down, Left, Right, Home, End, PageUp, PageDown, F1-F12, or Ctrl-<letter>"
            )
        })?);
    }
    if bytes.is_empty() {
        return Err("give text, keys, or both".into());
    }
    Ok((bytes, text))
}

pub struct Open;

impl Tool for Open {
    fn name(&self) -> &'static str {
        OPEN
    }
    fn description(&self) -> &'static str {
        "Start a program on a new terminal (a pty) and get its id and first screen: for a REPL, \
         an editor, a pager, or anything that wants a terminal or asks for input. argv is the \
         program and its arguments, run directly with no shell (give [\"bash\"] for a shell). \
         Drive it with term_send, read it with term_read, and end it with term_close. A session \
         may hold 4 terminals; each closes when its session ends."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "required": ["argv"], "additionalProperties": false,
        "properties": {
            "argv": {"type": "array", "items": {"type": "string"}, "minItems": 1},
            "cwd": {"type": "string", "description": "working directory; default the workspace's"},
            "rows": {"type": "integer", "minimum": 2, "maximum": 200, "description": "default 24"},
            "cols": {"type": "integer", "minimum": 10, "maximum": 500, "description": "default 80"},
            "quiet_ms": {"type": "integer", "minimum": 0, "description": "wait for the first screen to be quiet this long (default 300, at most 3000 in all)"}
        }})
    }
    fn class(&self) -> ToolClass {
        ToolClass::Run
    }
    fn backend(&self) -> Backend {
        Backend::Async
    }
    fn retry(&self) -> Retry {
        Retry::NonRepeatable
    }
    fn plan(&self, input: &Value, ctx: &ToolCtx) -> Result<Plan, String> {
        let a: OpenArgs = parse(input)?;
        if a.argv.is_empty() || a.argv[0].trim().is_empty() {
            return Err("argv must name a program".into());
        }
        size_of(&a)?;
        let cwd = cwd_of(a.cwd.as_deref(), ctx);
        Ok(Plan {
            summary: format!(
                "run `{}` on a terminal in {}",
                a.argv.join(" "),
                cwd.display()
            ),
            resources: vec![Resource {
                path: cwd,
                access: Access::Exec,
            }],
            argv: Some(a.argv),
            ..Default::default()
        })
    }
    fn run_async(&self, _: &Value, _: &ToolCtx) -> AsyncRun {
        unreachable_run()
    }
}

/// `term.send`: it holds the terminals, since its plan names the program
/// its terminal runs.
pub struct Send(pub Arc<Terms>);

impl Tool for Send {
    fn name(&self) -> &'static str {
        SEND
    }
    fn description(&self) -> &'static str {
        "Type into a terminal: text (each newline is Enter), then named keys in order (Enter, \
         Tab, Escape, Backspace, Delete, Space, Up, Down, Left, Right, Home, End, PageUp, \
         PageDown, F1-F12, Ctrl-C and any Ctrl-<letter>). It answers once the keys are sent; \
         term_read shows what they did. Sending to a terminal counts as running its program."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "required": ["terminal"], "additionalProperties": false,
        "properties": {
            "terminal": {"type": "string", "description": "the id term_open gave"},
            "text": {"type": "string"},
            "keys": {"type": "array", "items": {"type": "string"}}
        }})
    }
    fn class(&self) -> ToolClass {
        ToolClass::Run
    }
    fn backend(&self) -> Backend {
        Backend::Async
    }
    fn retry(&self) -> Retry {
        Retry::NonRepeatable
    }
    fn plan(&self, input: &Value, _: &ToolCtx) -> Result<Plan, String> {
        let a: SendArgs = parse(input)?;
        send_bytes(&a)?;
        // It runs the terminal's program, and is judged as that program's
        // run: its argv and its working directory.
        let t = self
            .0
            .get(&a.terminal)
            .ok_or_else(|| format!("there is no terminal {}; term_open starts one", a.terminal))?;
        Ok(Plan {
            summary: format!(
                "type into terminal {}, which runs `{}` in {}",
                t.id,
                t.argv.join(" "),
                t.cwd.display()
            ),
            resources: vec![Resource {
                path: t.cwd.clone(),
                access: Access::Exec,
            }],
            argv: Some(t.argv.clone()),
            ..Default::default()
        })
    }
    fn run_async(&self, _: &Value, _: &ToolCtx) -> AsyncRun {
        unreachable_run()
    }
}

pub struct Read;

impl Tool for Read {
    fn name(&self) -> &'static str {
        READ
    }
    fn description(&self) -> &'static str {
        "Read a terminal's screen: its rows as text, the cursor, the lines that scrolled off \
         since the last read, and which rows changed. To wait for a program, give quiet_ms (the \
         screen unchanged that long) or until (a text that appears on the screen), bounded by \
         timeout_ms (default 10000, at most 60000)."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "required": ["terminal"], "additionalProperties": false,
        "properties": {
            "terminal": {"type": "string"},
            "quiet_ms": {"type": "integer", "minimum": 0},
            "until": {"type": "string", "description": "plain text, not a pattern"},
            "timeout_ms": {"type": "integer", "minimum": 0, "maximum": MAX_WAIT_MS}
        }})
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
        let a: ReadArgs = parse(input)?;
        Ok(Plan {
            summary: format!("read terminal {}", a.terminal),
            resources: vec![Resource {
                path: ctx.cwd.clone(),
                access: Access::Read,
            }],
            ..Default::default()
        })
    }
    fn run_async(&self, _: &Value, _: &ToolCtx) -> AsyncRun {
        unreachable_run()
    }
}

pub struct Close;

impl Tool for Close {
    fn name(&self) -> &'static str {
        CLOSE
    }
    fn description(&self) -> &'static str {
        "Close a terminal: its program and everything it started get a hang-up, then are \
         killed if they linger. Says how the program ended."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "required": ["terminal"], "additionalProperties": false,
            "properties": {"terminal": {"type": "string"}}})
    }
    /// A read: it stops only what this session started, as a cancel does,
    /// so it never waits on a hold.
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
        let a: CloseArgs = parse(input)?;
        Ok(Plan {
            summary: format!("close terminal {}", a.terminal),
            resources: vec![Resource {
                path: ctx.cwd.clone(),
                access: Access::Read,
            }],
            ..Default::default()
        })
    }
    fn run_async(&self, _: &Value, _: &ToolCtx) -> AsyncRun {
        unreachable_run()
    }
}

/// A `term.*` tool runs through `Terms::run`, which knows its session.
fn unreachable_run() -> AsyncRun {
    Box::pin(std::future::ready(Err(ToolFailure::new(
        "a terminal tool runs through its session's terminals",
    ))))
}

/// Wait on a terminal: until its screen is quiet for `quiet`, or shows
/// `until`, or `bound` passes, or its program ends. What the wait found.
async fn wait(
    t: &Terminal,
    quiet: Option<Duration>,
    until: Option<&str>,
    bound: Duration,
) -> String {
    let t0 = Instant::now();
    let mut rx = t.pty.shared.version.subscribe();
    // The screen's rows as one text, so a text may span rows.
    let shows = |u: &str| {
        t.pty
            .shared
            .screen
            .lock()
            .unwrap()
            .rows()
            .join("\n")
            .contains(u)
    };
    // A row's trailing blanks are cut, so a text's are too: `>>> ` is the
    // prompt `>>>` at a row's end.
    let until = until.map(|u| match u.trim_end_matches(' ') {
        "" => u,
        cut => cut,
    });
    loop {
        if let Some(u) = until {
            if shows(u) {
                return format!("{u:?} appeared after {} ms", t0.elapsed().as_millis());
            }
        }
        if t.pty.shared.hung_up.load(Ordering::Relaxed) {
            return format!("its program ended after {} ms", t0.elapsed().as_millis());
        }
        let left = bound.saturating_sub(t0.elapsed());
        if left.is_zero() {
            return match until {
                Some(u) => format!("{u:?} did not appear in {} ms", bound.as_millis()),
                None => format!(
                    "the screen was not quiet for {} ms in {} ms",
                    quiet.unwrap_or_default().as_millis(),
                    bound.as_millis()
                ),
            };
        }
        // The next change, or the quiet that ends the wait.
        let step = match (quiet, until) {
            (Some(q), _) => q.min(left),
            (None, _) => left,
        };
        rx.mark_unchanged();
        match tokio::time::timeout(step, rx.changed()).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) => return "its terminal closed".into(),
            Err(_) if quiet.is_some() && until.is_none() => {
                return format!(
                    "quiet for {} ms after {} ms",
                    step.as_millis(),
                    t0.elapsed().as_millis()
                );
            }
            Err(_) => {}
        }
    }
}

/// One `term.*` call, for `session`.
pub async fn run(
    terms: &Arc<Terms>,
    tool: &str,
    session: &str,
    input: &Value,
    ctx: &ToolCtx,
) -> AsyncResult {
    match tool {
        OPEN => {
            let a: OpenArgs = parse(input).map_err(ToolFailure::new)?;
            let size = size_of(&a).map_err(ToolFailure::new)?;
            let cwd = cwd_of(a.cwd.as_deref(), ctx);
            let t = terms
                .open(session, &a.argv, cwd, size, ctx.umask)
                .map_err(ToolFailure::new)?;
            let quiet =
                Duration::from_millis(a.quiet_ms.unwrap_or(OPEN_QUIET_MS).min(OPEN_WAIT_MS));
            let waited = wait(&t, Some(quiet), None, Duration::from_millis(OPEN_WAIT_MS)).await;
            let (mut out, ext) = super::snapshot(&t, Some(format!("Opened; {waited}")))?;
            out.meta["opened"] = json!({"argv": t.argv, "cwd": t.cwd, "pid": t.pty.pid(),
                "rows": size.0, "cols": size.1});
            Ok((out, ext))
        }
        SEND => send(terms, session, input),
        READ => {
            let a: ReadArgs = parse(input).map_err(ToolFailure::new)?;
            let t = terms.of(session, &a.terminal).map_err(ToolFailure::new)?;
            let waits = a.quiet_ms.is_some() || a.until.is_some();
            let waited = match waits {
                true => {
                    let bound = Duration::from_millis(
                        a.timeout_ms.unwrap_or(READ_WAIT_MS).min(MAX_WAIT_MS),
                    );
                    let quiet = a.quiet_ms.map(Duration::from_millis);
                    Some(
                        wait(
                            &t,
                            quiet,
                            a.until.as_deref().filter(|u| !u.is_empty()),
                            bound,
                        )
                        .await,
                    )
                }
                false => None,
            };
            super::snapshot(&t, waited.map(|w| format!("Waited: {w}")))
        }
        CLOSE => close(terms, session, input).await,
        other => Err(ToolFailure::new(format!("{other} is not a terminal tool"))),
    }
}

/// `term.send`: the keys typed, and what was sent, in words.
fn send(terms: &Arc<Terms>, session: &str, input: &Value) -> AsyncResult {
    let a: SendArgs = parse(input).map_err(ToolFailure::new)?;
    let (bytes, text) = send_bytes(&a).map_err(ToolFailure::new)?;
    let t = terms
        .send(session, &a.terminal, &bytes, &text)
        .map_err(ToolFailure::new)?;
    let mut what = Vec::new();
    if !text.is_empty() {
        what.push(format!("{} characters of text", text.chars().count()));
    }
    if !a.keys.is_empty() {
        what.push(a.keys.join(", "));
    }
    Ok((
        ToolOutput {
            text: format!(
                "Sent {} to terminal {} ({}); term_read shows what it did.",
                what.join(", then "),
                t.id,
                t.program()
            ),
            meta: json!({"terminal": t.id, "program": t.program(), "bytes": bytes.len(),
                "keys": a.keys}),
        },
        None,
    ))
}

/// `term.close`, off the runtime's workers: how its program ended.
async fn close(terms: &Arc<Terms>, session: &str, input: &Value) -> AsyncResult {
    let a: CloseArgs = parse(input).map_err(ToolFailure::new)?;
    let t = terms.of(session, &a.terminal).map_err(ToolFailure::new)?;
    let (terms2, t2) = (terms.clone(), t.clone());
    let closed = tokio::task::spawn_blocking(move || {
        terms2.close_one(&t2, super::BY_TOOL, super::CLOSE_GRACE)
    })
    .await
    .map_err(|e| ToolFailure::new(format!("the close failed: {e}")))?
    .ok_or_else(|| ToolFailure::new(format!("terminal {} is already closing", t.id)))?;
    let killed = match closed.killed {
        0 => String::new(),
        n => format!(
            "; {n} process{} that lingered killed",
            if n == 1 { "" } else { "es" }
        ),
    };
    Ok((
        ToolOutput {
            text: format!(
                "Closed terminal {} ({}): {}{killed}.",
                closed.id,
                program_of(&closed.argv),
                closed.how()
            ),
            meta: json!({"terminal": closed.id, "closed": closed.meta()}),
        },
        None,
    ))
}

fn program_of(argv: &[String]) -> String {
    super::program(argv)
}
