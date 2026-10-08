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
    /// Once sent, wait until the program is back in front (`until_idle`).
    #[serde(default)]
    until_idle: bool,
    #[serde(default)]
    timeout_ms: Option<u64>,
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
    until_idle: bool,
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
         may run 4 at once; each closes when its session ends. Programs it starts in the \
         background keep running after the session ends, as with proc_run; term_close stops \
         everything it started."
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
         PageDown, F1-F12, Ctrl-C and any Ctrl-<letter>). It answers once the keys are sent, or \
         with until_idle once the program is back in front (as term_read's) with its screen. \
         Sending to a terminal counts as running its program."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "required": ["terminal"], "additionalProperties": false,
        "properties": {
            "terminal": {"type": "string", "description": "the id term_open gave"},
            "text": {"type": "string"},
            "keys": {"type": "array", "items": {"type": "string"}},
            "until_idle": {"type": "boolean"},
            "timeout_ms": {"type": "integer", "minimum": 0}
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
         since the last read, and which rows changed. To wait for a command you typed to finish, \
         give until_idle: true (the program, a shell or a REPL, is back in front, waiting for \
         input). Or wait for quiet_ms (the screen unchanged that long) or until (a text that \
         appears on the screen); whichever comes first, bounded by timeout_ms (default 10000, \
         at most 60000; until_idle alone may wait as long as proc_run's sync wait)."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "required": ["terminal"], "additionalProperties": false,
        "properties": {
            "terminal": {"type": "string"},
            "quiet_ms": {"type": "integer", "minimum": 0},
            "until": {"type": "string", "description": "plain text, not a pattern"},
            "until_idle": {"type": "boolean"},
            "timeout_ms": {"type": "integer", "minimum": 0}
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
        "Close a terminal: its program and everything it started, background jobs too, get a \
         hang-up, then are killed if they linger. Says how the program ended."
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

/// What a read waits for: the screen quiet for `quiet`, a text it shows
/// (`until`), the program back in front (`idle`); whichever comes first,
/// bounded by `bound`, or the program's end. As before `idle`, a wait for a
/// text is not ended by quiet.
#[derive(Default)]
struct Wait<'a> {
    quiet: Option<Duration>,
    until: Option<&'a str>,
    idle: bool,
}

/// Wait on a terminal for `w`, or until `bound` passes, or its program ends.
/// What the wait found. A wait `idle` looks at the pty's front every
/// `IDLE_LOOK`, a timer's wake, and at each change of the screen.
async fn wait(t: &Terminal, w: Wait<'_>, bound: Duration) -> String {
    let Wait { quiet, until, idle } = w;
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
    // Quiet is counted from the last change, as a timer reset at each.
    let mut quiet_from = Instant::now();
    loop {
        if let Some(u) = until {
            if shows(u) {
                return format!("{u:?} appeared after {} ms", t0.elapsed().as_millis());
            }
        }
        if t.pty.shared.hung_up.load(Ordering::Relaxed) || (idle && t.ended()) {
            return format!("its program ended after {} ms", t0.elapsed().as_millis());
        }
        if idle && t.idle() {
            return format!(
                "its program is idle, back in front, after {} ms",
                t0.elapsed().as_millis()
            );
        }
        if let Some(q) = quiet.filter(|_| idle && until.is_none()) {
            if quiet_from.elapsed() >= q {
                return format!(
                    "quiet for {} ms after {} ms",
                    q.as_millis(),
                    t0.elapsed().as_millis()
                );
            }
        }
        let left = bound.saturating_sub(t0.elapsed());
        if left.is_zero() {
            let busy = || {
                t.in_front()
                    .map(|p| format!(" ({p} is in front)"))
                    .unwrap_or_default()
            };
            return match (until, idle) {
                (Some(u), _) => format!("{u:?} did not appear in {} ms", bound.as_millis()),
                (None, true) => format!(
                    "its program was not idle in {} ms{}",
                    bound.as_millis(),
                    busy()
                ),
                (None, false) => format!(
                    "the screen was not quiet for {} ms in {} ms",
                    quiet.unwrap_or_default().as_millis(),
                    bound.as_millis()
                ),
            };
        }
        // The next change, the quiet that ends the wait, or the next look.
        let mut step = match quiet {
            Some(q) if idle => q.saturating_sub(quiet_from.elapsed()).min(left),
            Some(q) => q.min(left),
            None => left,
        };
        if idle {
            step = step.min(super::IDLE_LOOK);
        }
        rx.mark_unchanged();
        match tokio::time::timeout(step, rx.changed()).await {
            Ok(Ok(())) => quiet_from = Instant::now(),
            Ok(Err(_)) => return "its terminal closed".into(),
            Err(_) if quiet.is_some() && until.is_none() && !idle => {
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

/// A read's bound: `timeout_ms`, or the default, at most `MAX_WAIT_MS`, or
/// for `until_idle` alone at most `[tools] proc_sync_secs`.
fn bound_of(terms: &Terms, timeout_ms: Option<u64>, idle_alone: bool) -> Duration {
    let max = match idle_alone {
        true => terms.idle_max_ms,
        false => MAX_WAIT_MS,
    };
    Duration::from_millis(timeout_ms.unwrap_or(READ_WAIT_MS).min(max))
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
            let (t, reclaimed) = terms
                .open(session, &a.argv, cwd, size, ctx.umask)
                .map_err(ToolFailure::new)?;
            let quiet =
                Duration::from_millis(a.quiet_ms.unwrap_or(OPEN_QUIET_MS).min(OPEN_WAIT_MS));
            let w = Wait {
                quiet: Some(quiet),
                ..Wait::default()
            };
            let waited = wait(&t, w, Duration::from_millis(OPEN_WAIT_MS)).await;
            let mut opened = format!("Opened; {waited}");
            if let Some(r) = &reclaimed {
                opened = format!(
                    "Opened in the slot of terminal {} ({}), whose program had ended ({}): it is \
                     closed; {waited}",
                    r.id,
                    program_of(&r.argv),
                    r.how()
                );
            }
            let (mut out, ext) = super::snapshot(&t, Some(opened))?;
            out.meta["opened"] = json!({"argv": t.argv, "cwd": t.cwd, "pid": t.pty.pid(),
                "rows": size.0, "cols": size.1});
            if let Some(r) = &reclaimed {
                out.meta["reclaimed"] = r.meta();
            }
            Ok((out, ext))
        }
        SEND => send(terms, session, input).await,
        READ => {
            let a: ReadArgs = parse(input).map_err(ToolFailure::new)?;
            let t = terms.of(session, &a.terminal).map_err(ToolFailure::new)?;
            let until = a.until.as_deref().filter(|u| !u.is_empty());
            let waits = a.quiet_ms.is_some() || until.is_some() || a.until_idle;
            let waited = match waits {
                true => {
                    let idle_alone = a.until_idle && a.quiet_ms.is_none() && until.is_none();
                    let w = Wait {
                        quiet: a.quiet_ms.map(Duration::from_millis),
                        until,
                        idle: a.until_idle,
                    };
                    Some(wait(&t, w, bound_of(terms, a.timeout_ms, idle_alone)).await)
                }
                false => None,
            };
            super::snapshot(&t, waited.map(|w| format!("Waited: {w}")))
        }
        CLOSE => close(terms, session, input).await,
        other => Err(ToolFailure::new(format!("{other} is not a terminal tool"))),
    }
}

/// `term.send`: the keys typed, and what was sent, in words; with
/// `until_idle`, the screen once the program is back in front.
async fn send(terms: &Arc<Terms>, session: &str, input: &Value) -> AsyncResult {
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
    if a.until_idle {
        let w = Wait {
            idle: true,
            ..Wait::default()
        };
        let waited = wait(&t, w, bound_of(terms, a.timeout_ms, true)).await;
        let (mut out, ext) = super::snapshot(
            &t,
            Some(format!("Sent {}; waited: {waited}", what.join(", then "))),
        )?;
        out.meta["bytes"] = json!(bytes.len());
        out.meta["keys"] = json!(a.keys);
        return Ok((out, ext));
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
