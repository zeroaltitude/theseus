//! The CLI's renderers (theseus-0g4, finding 11): what a terminal shows of a
//! session's live events (`Printer`), of its history (`print_node`), of a
//! turn's trace (`print_span`), and every line `theseus health` and the other
//! commands print. Each writes to the writer it is given, or returns its
//! line, so tests read them as text. The golden tests in `tests/golden.rs`
//! hold the bytes.

use std::io::{self, Stderr, Stdout, Write};

use serde_json::Value;
use theseus_protocol::{
    method, ApprovalRefused, Attention, ConfirmRequest, Event, Level, NodeInfo, SessionInfo,
    ToolEnded, ToolListResult, TurnSubmitResult,
};

/// What a `Printer` shows of a session's events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The reply streamed on stdout as it comes, and each event on stderr:
    /// `ask`, and the turn `confirm` follows.
    Text,
    /// Each event on stderr, and none of the reply, which the command prints
    /// once at the end: `ask --no-stream`.
    Quiet,
    /// `Text`, plus each turn's start and end and every context decision:
    /// `watch`.
    Watch,
    /// Nothing: the command prints its result as JSON (`--json`).
    Json,
}

/// Renders live session events for a terminal: the model's text on `out`
/// (stdout), everything else (tools, confirmations, context decisions) on
/// `err` (stderr).
pub struct Printer<O: Write = Stdout, E: Write = Stderr> {
    mode: Mode,
    /// The model's thinking summaries too, on `err`, as they stream.
    thinking: bool,
    out: O,
    err: E,
    stdout_mid_line: bool,
    thinking_open: bool,
}

impl Printer {
    /// A printer to the terminal.
    pub fn new(mode: Mode, thinking: bool) -> Self {
        Self::to(mode, thinking, io::stdout(), io::stderr())
    }
}

impl<O: Write, E: Write> Printer<O, E> {
    pub fn to(mode: Mode, thinking: bool, out: O, err: E) -> Self {
        Self {
            mode,
            thinking,
            out,
            err,
            stdout_mid_line: false,
            thinking_open: false,
        }
    }

    /// What it wrote to `out` and to `err`.
    #[cfg(test)]
    pub fn into_parts(self) -> (O, E) {
        (self.out, self.err)
    }

    /// Finish a partial stdout line or thinking run before an event line.
    pub fn settle(&mut self) {
        if self.thinking_open {
            let _ = writeln!(self.err);
            self.thinking_open = false;
        }
        if self.stdout_mid_line {
            let _ = writeln!(self.out);
            let _ = self.out.flush();
            self.stdout_mid_line = false;
        }
    }

    /// One notification, as the connection hands it over.
    pub fn on(&mut self, m: &str, p: &Value) {
        if let Ok(Some(e)) = Event::from_notification(m, p) {
            self.on_event(&e);
        }
    }

    pub fn on_event(&mut self, e: &Event) {
        let (text, verbose) = match self.mode {
            Mode::Json => return,
            Mode::Text => (true, false),
            Mode::Quiet => (false, false),
            Mode::Watch => (true, true),
        };
        match e {
            Event::ModelDelta(d) if text => {
                if self.thinking_open {
                    let _ = writeln!(self.err);
                    self.thinking_open = false;
                }
                let _ = self.out.write_all(d.text.as_bytes());
                let _ = self.out.flush();
                if !d.text.is_empty() {
                    self.stdout_mid_line = !d.text.ends_with('\n');
                }
            }
            Event::ModelThinking(d) if self.thinking => {
                if !self.thinking_open {
                    self.settle();
                    let _ = write!(self.err, "  (thinking) ");
                    self.thinking_open = true;
                }
                let _ = write!(self.err, "{}", d.text.replace('\n', "\n             "));
            }
            Event::ToolStarted(s) => {
                self.settle();
                let argv = s
                    .argv
                    .as_ref()
                    .map(|a| {
                        format!(
                            " [{}]{}",
                            a.join(" "),
                            s.pid.map(|p| format!(" pid {p}")).unwrap_or_default()
                        )
                    })
                    .unwrap_or_default();
                let _ = writeln!(self.err, "  → {}{argv}", s.tool);
            }
            Event::ToolEnded(t) => {
                self.settle();
                let _ = writeln!(self.err, "{}", tool_ended_line(t));
            }
            Event::ConfirmRequested(c) => {
                self.settle();
                if c.budget.is_some() {
                    let _ = writeln!(
                        self.err,
                        "  $ {}\n      reset and continue: theseus confirm {}\n      keep waiting: theseus confirm --decline {}",
                        c.reason, c.correlation_id, c.correlation_id
                    );
                    return;
                }
                let _ = writeln!(
                    self.err,
                    "  ? {} needs your confirmation{}: {}\n      input: {}\n      approve: theseus confirm {}{}\n      decline: theseus confirm --decline {}",
                    c.tool,
                    if c.floor { " (FLOOR)" } else { "" },
                    c.reason,
                    clip(&c.input.to_string(), 200),
                    c.correlation_id,
                    // It waits because its session read external text
                    // (theseus-9bp).
                    if c.external_text.is_some() {
                        format!(
                            "\n      approve, and trust the session again: theseus confirm --trust {}",
                            c.correlation_id
                        )
                    } else {
                        String::new()
                    },
                    c.correlation_id
                );
            }
            Event::PolicyNotified(n) => {
                self.settle();
                let _ = writeln!(
                    self.err,
                    "  ! notified: {}: {}{}\n      ({}) · should have asked: theseus policy tighten {}{}",
                    n.tool,
                    n.summary,
                    n.granted
                        .as_deref()
                        .map(|g| format!(" · 🔑 {g}"))
                        .unwrap_or_default(),
                    n.notice.setting,
                    n.tool,
                    if n.correlation_id.is_empty() {
                        String::new()
                    } else {
                        format!(" --call {}", n.correlation_id)
                    }
                );
            }
            Event::PolicyTightened(r) | Event::PolicyUntightened(r) => {
                self.settle();
                let tightened = matches!(e, Event::PolicyTightened(_));
                let _ = writeln!(self.err, "  🔒 {}", tightened_line(r, tightened));
            }
            Event::ApprovalRefused(r) => {
                self.settle();
                let _ = writeln!(self.err, "  🚨 {}", job_refusal_line(r));
            }
            Event::ConfirmResolved(r) => {
                self.settle();
                let by = r.by.as_deref().unwrap_or("");
                if r.superseded {
                    let _ = writeln!(
                        self.err,
                        "  ✗ superseded {} (a new message arrived before an answer)",
                        r.correlation_id
                    );
                } else if r.cancelled {
                    // Its execution was cancelled before an answer (theseus-w98).
                    let _ = writeln!(
                        self.err,
                        "  ✗ cancelled {} (its execution was cancelled by {by})",
                        r.correlation_id
                    );
                } else {
                    let _ = writeln!(
                        self.err,
                        "  {} {} (by {by})",
                        if r.approved {
                            "✓ approved"
                        } else {
                            "✗ declined"
                        },
                        r.correlation_id
                    );
                }
            }
            Event::ContextCompiled(c) if c.decision == "recompile" || verbose => {
                self.settle();
                let _ = writeln!(
                    self.err,
                    "  ⟳ context {}{} · {} message(s) · ~{} tokens{}",
                    c.decision,
                    c.trigger
                        .as_deref()
                        .map(|t| format!(" ({t}, {})", c.strategy))
                        .unwrap_or_default(),
                    c.messages,
                    c.est_tokens,
                    if c.repairs.is_empty() {
                        String::new()
                    } else {
                        format!(" · {} repaired", c.repairs.len())
                    }
                );
            }
            Event::TurnStarted(t) if verbose => {
                self.settle();
                let _ = writeln!(
                    self.err,
                    "── turn {}{}",
                    t.turn_id,
                    if t.continuation {
                        " (continuation)"
                    } else {
                        ""
                    }
                );
            }
            Event::TurnEnded(r) if verbose => {
                self.settle();
                let _ = writeln!(self.err, "{}", status_line(r));
            }
            Event::TurnFailed(f) => {
                self.settle();
                // What follows it (theseus-ljr).
                let then = match f.then.as_deref() {
                    Some("backoff") => " [retrying with backoff]",
                    Some("retry") => " [retrying once]",
                    Some("park") => " [not retried: the next message retries]",
                    _ => "",
                };
                let _ = writeln!(
                    self.err,
                    "  ✗ turn failed{}: {}{then}",
                    f.class
                        .as_deref()
                        .map(|c| format!(" ({c})"))
                        .unwrap_or_default(),
                    f.error
                );
            }
            _ => {}
        }
    }
}

pub fn status_line(r: &TurnSubmitResult) -> String {
    let cache = if r.usage.cache_read_input_tokens + r.usage.cache_creation_input_tokens > 0 {
        format!(
            " cache r{} w{}",
            r.usage.cache_read_input_tokens, r.usage.cache_creation_input_tokens
        )
    } else {
        String::new()
    };
    format!(
        "[{} → {}/{} · {} loop(s){}{} · {} · tokens in {} out {}{}{} · {} ms{} · session {}]",
        r.profile,
        r.provider,
        r.model,
        r.loops,
        if r.tool_calls > 0 {
            format!(" · {} tool call(s)", r.tool_calls)
        } else {
            String::new()
        },
        if r.continuation {
            " · continuation"
        } else {
            ""
        },
        r.stop_reason,
        r.usage.input_tokens,
        r.usage.output_tokens,
        cache,
        r.cost_usd
            .map(|c| format!(" · ${c:.4}"))
            .unwrap_or_default(),
        r.elapsed_ms,
        r.first_token_ms
            .map(|t| format!(" (first token {t} ms)"))
            .unwrap_or_default(),
        r.session_id
    )
}

/// A span's bar is drawn on this stretch of the turn: its start, its length,
/// and whether it is a `tools` span's own time rather than the whole turn's.
pub struct Frame {
    from_us: u64,
    total_us: u64,
    grouped: bool,
}

impl Frame {
    pub fn turn(root: &theseus_protocol::Span) -> Self {
        Self {
            from_us: 0,
            total_us: root.duration_us().max(1),
            grouped: false,
        }
    }
}

/// Indented tree with a 24-column bar: where in the turn each span sat. The
/// calls a `tools` span ran together are drawn on its own time, with `▓`, so
/// their overlap shows however short the group is beside the turn.
pub fn print_span(
    w: &mut impl Write,
    s: &theseus_protocol::Span,
    depth: usize,
    f: &Frame,
) -> io::Result<()> {
    let width = 24usize;
    let at = |us: u64| us.saturating_sub(f.from_us) as f64 / f.total_us as f64 * width as f64;
    let a = at(s.start_us).floor() as usize;
    let b = at(s.end_us.unwrap_or(s.start_us)).ceil() as usize;
    let (a, b) = (a.min(width), b.clamp(a.min(width), width));
    let fill = if f.grouped { '▓' } else { '█' };
    let mut bar = String::new();
    for i in 0..width {
        bar.push(if i >= a && (i < b || (i == a && a == b)) {
            fill
        } else {
            '·'
        });
    }
    let dur = if s.end_us == Some(s.start_us) {
        format!("@{}", fmt_us(s.start_us))
    } else {
        fmt_us(s.duration_us())
    };
    let attrs = match &s.attrs {
        serde_json::Value::Null => String::new(),
        v => {
            let t = serde_json::to_string(v).unwrap_or_default();
            let t: String = t.chars().take(90).collect();
            format!("  {t}")
        }
    };
    writeln!(
        w,
        "{bar} {:>9}  {}{} [{}]{}",
        dur,
        "  ".repeat(depth),
        s.name,
        s.kind,
        attrs
    )?;
    let own = Frame {
        from_us: s.start_us,
        total_us: s.duration_us().max(1),
        grouped: true,
    };
    let f = if s.kind == "tools" { &own } else { f };
    for c in &s.children {
        print_span(w, c, depth + 1, f)?;
    }
    Ok(())
}

pub fn session_header(s: &SessionInfo) -> String {
    format!(
        "── {} · {} · {} turn(s) · {} tool call(s) · ${:.4} · {}{}",
        s.session_id,
        s.title
            .as_deref()
            .or(s.label.as_deref())
            .map(|t| format!("\"{t}\""))
            .unwrap_or_else(|| "(untitled)".into()),
        s.turns,
        s.tool_calls,
        s.cost_usd,
        s.model.as_deref().unwrap_or("-"),
        s.execution_state
            .as_deref()
            .map(|e| format!(" · {e}"))
            .unwrap_or_default()
    )
}

/// A result status as the operator reads it: `declined` is a call that never
/// ran (declined, or superseded by a new message). A daemon from before
/// theseus-8az says `denied`.
pub fn status_word(status: &str) -> &str {
    match status {
        "declined" | "denied" => "not run",
        s => s,
    }
}

/// A result's word, and who stopped it when a `/stop` ended it: `stopped by
/// the CLI`, as the stop's own card says, never `cancelled` (theseus-4uw).
pub fn result_word(status: &str, stopped_by: Option<&str>) -> String {
    match stopped_by {
        Some(by) if status == "cancelled" => format!("stopped by {by}"),
        _ => status_word(status).to_string(),
    }
}

/// `theseus watch`'s line for a `tool.ended`: the tool, how it ended, and its
/// exit code, time, size, and marks.
pub fn tool_ended_line(t: &ToolEnded) -> String {
    let mut extra = Vec::new();
    if let Some(c) = t.exit_code {
        extra.push(format!("exit {c}"));
    }
    if let Some(ms) = t.duration_ms {
        extra.push(format!("{ms} ms"));
    }
    extra.push(fmt_bytes(t.bytes));
    if t.truncated {
        extra.push("truncated".into());
    }
    if t.late {
        extra.push("late".into());
    }
    format!(
        "  ← {} {}{}",
        t.tool,
        result_word(&t.status, t.stopped_by.as_deref()),
        if extra.is_empty() {
            String::new()
        } else {
            format!(" · {}", extra.join(" · "))
        }
    )
}

pub fn clip(s: &str, max: usize) -> String {
    let one = s.replace('\n', " ⏎ ");
    if one.chars().count() <= max {
        one
    } else {
        format!("{}…", one.chars().take(max).collect::<String>())
    }
}

pub fn indent(s: &str, pad: &str) -> String {
    s.lines()
        .map(|l| format!("{pad}{l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn print_node(w: &mut impl Write, n: &NodeInfo, full: bool) -> io::Result<()> {
    let t = fmt_time(n.at_unix_ms);
    let d = &n.detail;
    let s = |k: &str| d.get(k).and_then(Value::as_str).unwrap_or("");
    match n.kind.as_str() {
        "user_message" => {
            let who = match n.author.as_deref() {
                Some(a) => format!("operator ({a})"),
                None => "operator".to_string(),
            };
            if full {
                writeln!(w, "[{t}] {who}:\n{}", indent(&n.text, "    "))?;
            } else {
                writeln!(w, "[{t}] {who}: {}", clip(&n.text, 300))?;
            }
        }
        "assistant_message" => {
            let cost = d
                .get("cost_usd")
                .and_then(Value::as_f64)
                .map(|c| format!(" · ${c:.4}"))
                .unwrap_or_default();
            let calls = d
                .get("tool_calls")
                .and_then(Value::as_array)
                .map(|a| a.len())
                .unwrap_or(0);
            let body = if n.text.is_empty() {
                String::new()
            } else if full {
                format!("\n{}", indent(&n.text, "    "))
            } else {
                format!(" {}", clip(&n.text, 300))
            };
            writeln!(
                w,
                "[{t}] {}:{body}{}",
                s("model"),
                if calls > 0 && n.text.is_empty() {
                    format!(" ({calls} tool call(s))")
                } else {
                    String::new()
                }
            )?;
            if !n.thinking.is_empty() && full {
                writeln!(w, "      (thinking) {}", clip(&n.thinking, 600))?;
            }
            writeln!(
                w,
                "      ↳ {} · in {} out {}{cost}",
                s("stop_reason"),
                d.pointer("/usage/input_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                d.pointer("/usage/output_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
            )?;
        }
        "tool_call" => {
            let input = d.get("input").map(|v| v.to_string()).unwrap_or_default();
            // The policy's verdict (its posture; the band for older rows) with
            // its reason; a call that failed validation never reached policy.
            let gate = match d.get("decision") {
                Some(Value::Object(o)) => format!(
                    "{}: {}",
                    o.get("posture")
                        .or_else(|| o.get("mode"))
                        .and_then(Value::as_str)
                        .unwrap_or("?"),
                    clip(o.get("reason").and_then(Value::as_str).unwrap_or(""), 90)
                ),
                _ => match d.pointer("/result/gate").and_then(Value::as_str) {
                    Some("deny") => "invalid input".into(),
                    g => g.unwrap_or("-").to_string(),
                },
            };
            writeln!(
                w,
                "      ⚙ {} {} [{}]",
                s("tool"),
                if full { input } else { clip(&input, 160) },
                gate
            )?;
        }
        "tool_result" => {
            let late = if d.get("late").and_then(Value::as_bool) == Some(true) {
                " (late)"
            } else {
                ""
            };
            let ms = format!(
                "{}{}",
                d.pointer("/meta/exit_code")
                    .and_then(Value::as_i64)
                    .map(|c| format!(" · exit {c}"))
                    .unwrap_or_default(),
                d.get("duration_ms")
                    .and_then(Value::as_u64)
                    .map(|m| format!(" · {m} ms"))
                    .unwrap_or_default()
            );
            let word = result_word(
                s("status"),
                d.pointer("/meta/stopped_by").and_then(Value::as_str),
            );
            if full {
                writeln!(
                    w,
                    "      ← {} {word}{late}{ms} · {}\n{}",
                    s("tool"),
                    fmt_bytes(n.bytes),
                    indent(&n.text, "        ")
                )?;
            } else {
                writeln!(
                    w,
                    "      ← {} {word}{late}{ms} · {}: {}",
                    s("tool"),
                    fmt_bytes(n.bytes),
                    clip(&n.text, 160)
                )?;
            }
        }
        other => writeln!(w, "[{t}] {other}")?,
    }
    Ok(())
}

pub fn print_confirm(w: &mut impl Write, c: &ConfirmRequest) -> io::Result<()> {
    if c.budget.is_some() {
        writeln!(w,
            "  $ {} waits for you: {}\n      reset and continue: theseus confirm {}\n      keep waiting: theseus confirm --decline {}",
            c.session_id, c.reason, c.correlation_id, c.correlation_id
        )?;
        return Ok(());
    }
    writeln!(w,
        "  ? {} waits for you in {}: {}\n      input: {}\n      approve: theseus confirm {}{}\n      decline: theseus confirm --decline {}",
        c.tool,
        c.session_id,
        c.reason,
        clip(&c.input.to_string(), 200),
        c.correlation_id,
        // It waits because its session read external text (theseus-9bp).
        if c.external_text.is_some() {
            format!(
                "\n      approve, and trust the session again: theseus confirm --trust {}",
                c.correlation_id
            )
        } else {
            String::new()
        },
        c.correlation_id
    )?;
    Ok(())
}

pub fn print_approval(w: &mut impl Write, a: &theseus_protocol::ApprovalStatus) -> io::Result<()> {
    if !a.configured {
        writeln!(
            w,
            "approval: no [approval] section, so the CLI, the web UI, and a place's listed \
             Discord users answer"
        )?;
        return Ok(());
    }
    let users = if a.trusted_users.is_empty() {
        "nobody on Discord".to_string()
    } else {
        a.trusted_users.join(", ")
    };
    let channels: Vec<String> = a
        .channels
        .iter()
        .map(|c| format!("{} {}", c.channel, c.state.replace('_', " ")))
        .collect();
    writeln!(
        w,
        "approval: trusted users {users} · channels: {}",
        if channels.is_empty() {
            "none".to_string()
        } else {
            channels.join(", ")
        }
    )?;
    for c in a.channels.iter().filter(|c| c.state != "trusted") {
        writeln!(w, "  {} is not trusted: {}", c.channel, c.detail)?;
    }
    Ok(())
}

/// A binding's outbox (theseus-q4v): `discord outbox: 2 pending, the oldest
/// 3 min old · 41 sent · 0 refused · last error …`.
pub fn outbox_line(kind: &str, o: &theseus_protocol::OutboxStatus) -> String {
    let now = theseus_protocol::now_unix_ms();
    let mut s = format!("{kind} outbox: {} pending", o.pending);
    if o.pending > 0 && o.oldest_pending_ms > 0 {
        let secs = now.saturating_sub(o.oldest_pending_ms) / 1000;
        s.push_str(&format!(", the oldest {} old", human_secs(secs)));
    }
    s.push_str(&format!(" · {} sent · {} refused", o.sent, o.failed));
    if let Some(e) = &o.last_error {
        let secs = now.saturating_sub(o.last_error_ms) / 1000;
        s.push_str(&format!(" · last error {} ago: {e}", human_secs(secs)));
    }
    s
}

pub fn human_secs(s: u64) -> String {
    match s {
        0..=119 => format!("{s} s"),
        120..=7199 => format!("{} min", s / 60),
        _ => format!("{} h", s / 3600),
    }
}

/// `secrets: resolving | ready | failed <names>`, with the names ready, how
/// the vault was read and how long it took, and each failure's reason
/// (theseus-qa0). A daemon older than that reports only the ready names.
pub fn secrets_line(s: &theseus_protocol::SecretsStatus, ready: &[String]) -> String {
    if s.state.is_empty() {
        return format!("secrets [{}]", ready.join(", "));
    }
    let mut line = format!("secrets: {}", s.summary());
    match s.state.as_str() {
        "resolving" => line.push_str(&format!(
            " · {} of {} ready so far",
            s.ready.len(),
            s.ready.len() + s.resolving.len() + s.failed.len()
        )),
        _ => {
            if let Some(ms) = s.settled_ms {
                line.push_str(&format!(" · {} ready {ms} ms after start", s.ready.len()));
            }
            if let Some(m) = &s.method {
                line.push_str(&format!(" ({m})"));
            }
        }
    }
    if !s.ready.is_empty() {
        line.push_str(&format!(" [{}]", s.ready.join(", ")));
    }
    for f in &s.failed {
        line.push_str(&format!("\n  {} did not resolve: {}", f.name, f.error));
    }
    if let Some(ms) = s.retry_in_ms.filter(|_| !s.failed.is_empty()) {
        line.push_str(&format!("\n  fetched again in {:.0} s", ms as f64 / 1000.0));
    }
    line
}

/// A Theseus job's process tried to answer an approval and was refused
/// (theseus-6qy), as `theseus watch` says it.
pub fn job_refusal_line(r: &ApprovalRefused) -> String {
    let tool = r.tool.as_deref().unwrap_or("?");
    let what = match r.act.as_str() {
        method::POLICY_UNTIGHTEN => format!("the undo of {tool}'s tightening"),
        _ => format!("an answer to {tool}"),
    };
    format!("refused {what} {} through {}", r.why, r.via)
}

/// The kernel line's note of job wrappers that linger for descendants their
/// command left running (theseus-6qy); nothing when none does.
pub fn lingering_note(n: u64) -> String {
    match n {
        0 => String::new(),
        1 => " · 1 job wrapper lingers for what its command left running".into(),
        n => format!(" · {n} job wrappers linger for what their commands left running"),
    }
}

/// `children: 2 job wrappers running · 1 adopted orphan · 0 zombies · reaped
/// 160 wrappers, 1 orphan` (theseus-z4b): what the daemon holds. Zombies stay
/// 0 in steady state, so a count that grows is a leak. A daemon older than
/// that says nothing.
pub fn children_line(c: &theseus_protocol::ChildrenStatus) -> Option<String> {
    if *c == theseus_protocol::ChildrenStatus::default() {
        return None;
    }
    let n = |n: u64, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    let mut line = format!(
        "children: {} running",
        n(c.wrappers_running, "job wrapper", "job wrappers")
    );
    if c.wrappers_lingering > 0 {
        line.push_str(&format!(", {} lingering", c.wrappers_lingering));
    }
    line.push_str(&format!(
        " · {} · {} · reaped {}, {}",
        n(c.orphans, "adopted orphan", "adopted orphans"),
        n(c.zombies, "zombie", "zombies"),
        n(c.reaped_wrappers, "wrapper", "wrappers"),
        n(c.reaped_orphans, "orphan", "orphans")
    ));
    if !c.subreaper {
        line.push_str(
            " · not a subreaper: a job that kills its wrapper leaves its orphans to init",
        );
    }
    Some(line)
}

/// `disk: 81,920 MB free of 1,006,712 MB under /home/x/.theseus · health warns
/// below 5,120 MB, jobs are refused below 1,024 MB` (theseus-102): the
/// filesystem that holds the state dir, as `statvfs` reads it. `LOW` under the
/// warning, `BELOW THE FLOOR` when new jobs are refused. Under WSL it is the
/// virtual disk, a file on the Windows drive, which can fill first (see
/// `DiskStatus`). A daemon older than that says nothing.
pub fn disk_line(d: &theseus_protocol::DiskStatus) -> Option<String> {
    if d.path.is_empty() {
        return None;
    }
    if d.state == "unknown" {
        return Some(format!(
            "disk: unknown under {}: {}",
            d.path,
            d.error.as_deref().unwrap_or("not read")
        ));
    }
    let head = match d.state.as_str() {
        "low" => "disk: LOW, ",
        "below_floor" => "disk: BELOW THE FLOOR, new jobs are refused: ",
        _ => "disk: ",
    };
    let mut limits = Vec::new();
    if d.warn_mb > 0 {
        limits.push(format!("health warns below {} MB", thousands(d.warn_mb)));
    }
    if d.floor_mb > 0 {
        limits.push(format!(
            "jobs are refused below {} MB",
            thousands(d.floor_mb)
        ));
    }
    Some(format!(
        "{head}{} MB free of {} MB under {}{}",
        thousands(d.free_mb),
        thousands(d.total_mb),
        d.path,
        if limits.is_empty() {
            String::new()
        } else {
            format!(" · {}", limits.join(", "))
        }
    ))
}

/// `spool: swept 3 min ago: removed 3 files (24 bytes): 1 absorbed, 1 ended, 1
/// unknown · kept 2: 1 pending, 1 running` (theseus-2ij): the last sweep of the
/// jobs' raw output that no result will absorb. Nothing before the first
/// sweep, or from a daemon older than that.
pub fn spool_line(s: &theseus_protocol::SpoolStatus, now_ms: u64) -> Option<String> {
    let w = s.last_sweep.as_ref()?;
    let secs = now_ms.saturating_sub(w.at_unix_ms) / 1000;
    let ago = if secs < 120 {
        format!("{secs} s ago")
    } else {
        format!("{} min ago", secs / 60)
    };
    let by = |m: &std::collections::BTreeMap<String, u64>| {
        m.iter()
            .map(|(why, n)| format!("{n} {why}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut line = format!(
        "spool: swept {ago}: removed {} {} ({} bytes)",
        w.removed,
        if w.removed == 1 { "file" } else { "files" },
        thousands(w.removed_bytes)
    );
    if !w.removed_by.is_empty() {
        line.push_str(&format!(": {}", by(&w.removed_by)));
    }
    line.push_str(&format!(" · kept {}", w.kept));
    if !w.kept_by.is_empty() {
        line.push_str(&format!(": {}", by(&w.kept_by)));
    }
    Some(line)
}

/// `130,300`.
pub fn thousands(n: u64) -> String {
    let d = n.to_string();
    let mut out = String::with_capacity(d.len() + d.len() / 3);
    for (i, c) in d.chars().enumerate() {
        if i > 0 && (d.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// `broker: gh gets GH_TOKEN (github_token, notify), used 3 times`
/// (theseus-dcy): each grant of the secret broker, by name, with its uses.
/// Never a value.
pub fn broker_line(grants: &[theseus_protocol::GrantStatus]) -> String {
    if grants.is_empty() {
        return "broker: no grants".into();
    }
    let each: Vec<String> = grants
        .iter()
        .map(|g| {
            let what = g.variable.as_deref().unwrap_or(&g.secret);
            let times = match g.uses {
                1 => "once".to_string(),
                n => format!("{n} times"),
            };
            format!(
                "{} gets {what} ({}, {}), used {times}",
                g.to, g.secret, g.posture
            )
        })
        .collect();
    format!("broker: {}", each.join(" · "))
}

/// `context: 2 system files · persona theseus (1 file)` (theseus-c48): the
/// context files every session gets, and the persona in play with its own.
/// A config that names none, or a daemon older than that, says nothing.
pub fn context_line(c: &theseus_protocol::ContextStatus) -> Option<String> {
    if *c == theseus_protocol::ContextStatus::default() {
        return None;
    }
    let files = |n: usize| format!("{n} file{}", if n == 1 { "" } else { "s" });
    let persona = match (&c.persona, c.personas.is_empty()) {
        (Some(p), _) => format!("persona {p} ({})", files(c.persona_files.len())),
        (None, true) => "no persona".to_string(),
        (None, false) => format!(
            "no persona in play (defined: {}; set [context] default_persona)",
            c.personas.join(", ")
        ),
    };
    Some(format!(
        "context: {} at the system level · {persona}",
        files(c.system_files.len())
    ))
}

/// `config: vault (confirmed in 1034 ms)`, `config: confirming …`, or
/// `config: held: <why>` (theseus-2fo): where the config came from, and
/// whether the vault has confirmed the copy this start served from. A daemon
/// older than that says nothing.
pub fn config_line(c: &theseus_protocol::ConfigStatus) -> Option<String> {
    let ms = |v: Option<u64>| v.map(|ms| format!("{ms} ms")).unwrap_or_else(|| "?".into());
    let mut line = match (c.source.as_str(), c.state.as_str()) {
        ("", _) => return None,
        ("file", _) => format!("config: file {}", c.reference),
        (_, "confirmed") if c.started_from == "vault" => format!(
            "config: vault (read before serving, in {}: {})",
            ms(c.confirmed_ms),
            c.detail.as_deref().unwrap_or("there was no copy")
        ),
        (_, "confirmed") => {
            let mut l = format!("config: vault (confirmed in {})", ms(c.confirmed_ms));
            if let Some(d) = c
                .detail
                .as_deref()
                .filter(|d| d.starts_with("only comments"))
            {
                l.push_str(&format!(": {d}"));
            }
            l
        }
        (_, "confirming") => format!(
            "config: confirming · serving from the copy of {}; nothing acts until the vault \
             confirms it",
            c.reference
        ),
        (_, state) => format!(
            "config: {state}: {}",
            c.detail.as_deref().unwrap_or("(no reason given)")
        ),
    };
    if let Some(ms) = c.retry_in_ms.filter(|_| c.state == "held") {
        line.push_str(&format!(
            "\n  the vault is read again in {:.0} s",
            ms as f64 / 1000.0
        ));
    }
    if let Some(r) = &c.restarted {
        line.push_str(&format!(
            "\n  restarted at {} onto the vault's changed note; changed since the copy: {}",
            fmt_time(r.at_unix_ms),
            r.tables.join(", ")
        ));
    }
    Some(line)
}

/// `startup: serving at 14.2 ms (config 1.0 ms · store 2.1 ms · …) · after:
/// secrets 1.03 s · …`: the last start's phases (theseus-qa0), and the time
/// no phase names, when there is some.
pub fn startup_line(phases: &[theseus_protocol::StartupPhase]) -> Option<String> {
    let serving = phases
        .iter()
        .filter(|p| !p.background)
        .filter_map(|p| p.end_us)
        .max()?;
    let span = |p: &theseus_protocol::StartupPhase| match p.end_us {
        Some(end) => format!("{} {}", p.name, fmt_us(end.saturating_sub(p.start_us))),
        None => format!("{} running", p.name),
    };
    let mut on: Vec<String> = phases.iter().filter(|p| !p.background).map(span).collect();
    // A slow start whose time shows here has a cause no phase names.
    let named: u64 = phases
        .iter()
        .filter(|p| !p.background)
        .filter_map(|p| p.end_us.map(|e| e.saturating_sub(p.start_us)))
        .sum();
    let between = serving.saturating_sub(named);
    if between >= 500 {
        on.push(format!("{} between phases", fmt_us(between)));
    }
    let after: Vec<String> = phases.iter().filter(|p| p.background).map(span).collect();
    let mut line = format!(
        "startup: serving at {} ({})",
        fmt_us(serving),
        on.join(" · ")
    );
    if !after.is_empty() {
        line.push_str(&format!(" · after: {}", after.join(" · ")));
    }
    Some(line)
}

/// `store: CORRUPT …` when the history check after serving (theseus-8ni)
/// found a frame that does not check; nothing while it is whole or running.
pub fn store_history_line(phases: &[theseus_protocol::StartupPhase]) -> Option<String> {
    let p = phases.iter().find(|p| p.name == "store.verify")?;
    (p.detail["outcome"] == "corrupt").then(|| {
        format!(
            "store: CORRUPT history, {}; reads from there are refused (theseusd restore \
             from a copy, or ask for help)",
            p.detail["error"].as_str().unwrap_or("?")
        )
    })
}

/// What `theseus stop` prints (W1): what stopped, and what goes on.
pub fn stop_line(r: &theseus_protocol::ExecutionStopResult) -> String {
    if !r.stopped {
        return format!(
            "nothing to stop: execution {} has ended ({})",
            r.execution.execution_id, r.execution.state
        );
    }
    let mut out = format!(
        "stopped session {}'s work · {} action(s) told to stop · {} declined{} · the conversation \
         goes on",
        r.execution.session_id,
        r.stopped_actions.len(),
        r.declined.len(),
        if r.turn_running {
            " · its running turn ends at its next step"
        } else {
            ""
        }
    );
    if r.tasks_running > 0 || r.wakes_pending > 0 {
        out.push_str(&format!(
            " · {} task(s) and {} wake(s) go on (theseus cancel <id>)",
            r.tasks_running, r.wakes_pending
        ));
    }
    out
}

/// `in 9m`, `in 2h`, or `due 3m ago` (a wake waiting for its busy session).
pub fn until_due(due_ms: u64, now_ms: u64) -> String {
    let words = |ms: u64| {
        let s = ms / 1000;
        match s {
            0..=59 => format!("{s}s"),
            60..=3599 => format!("{}m", s / 60),
            3600..=86_399 => format!("{}h", s / 3600),
            _ => format!("{}d", s / 86_400),
        }
    };
    if due_ms >= now_ms {
        format!("in {}", words(due_ms - now_ms))
    } else {
        format!("due {} ago", words(now_ms - due_ms))
    }
}

/// One pending wake, as `theseus wakes` lists it (DD8): its short id, when
/// it is due, its session and its state, and its note's first line.
pub fn wake_line(w: &theseus_protocol::WakeInfo, now_ms: u64) -> String {
    let note = w.note.lines().next().unwrap_or_default();
    let note: String = note.chars().take(100).collect();
    format!(
        "{}\t{} ({})\tsession {} ({}){}\t{note}",
        w.short,
        w.due_local,
        until_due(w.due_at_ms, now_ms),
        w.session_id,
        w.state,
        w.target
            .as_deref()
            .map(|t| format!(" → {t}"))
            .unwrap_or_default()
    )
}

pub fn wakes_line(wakes: &[theseus_protocol::WakeInfo], now_ms: u64) -> Option<String> {
    let next = wakes.iter().min_by_key(|w| w.due_at_ms)?;
    let note: String = next
        .note
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(60)
        .collect();
    Some(format!(
        "wakes: {} pending · next {} {} ({}): {note} · theseus wakes, theseus cancel <id>",
        wakes.len(),
        next.short,
        next.due_local,
        until_due(next.due_at_ms, now_ms)
    ))
}

/// One task, as `theseus tasks` lists it (DD7): its short id, its state and
/// what it waits on, its spend of its carved limit, its age, its title, and
/// the session that started it.
pub fn task_line(t: &theseus_protocol::TaskInfo, now_ms: u64) -> String {
    let age = now_ms.saturating_sub(t.created_at_ms) / 1000;
    let age = match age {
        0..=59 => format!("{age}s"),
        60..=3599 => format!("{}m", age / 60),
        3600..=86_399 => format!("{}h", age / 3600),
        _ => format!("{}d", age / 86_400),
    };
    let state = match (&t.attention, &t.waiting_on) {
        (Some(a), _) => pill(a),
        (None, Some(w)) => format!("{} on {w}", t.state),
        (None, None) => t.state.clone(),
    };
    let mut asks = match t.pending_confirms {
        0 => String::new(),
        n => format!(" · {n} waiting for you (theseus confirm)"),
    };
    // Its report starts its parent's next turn (W1).
    if t.wake_parent {
        asks.push_str(" · wakes its parent");
    }
    format!(
        "{}\t{state}\t${:.4} of ${:.2}\t{} turn{}\t{age}\t{}\tfrom {}{asks}{}",
        t.short,
        t.spent_usd,
        t.limit_usd,
        t.turns,
        if t.turns == 1 { "" } else { "s" },
        t.title.as_deref().unwrap_or("untitled"),
        t.parent_session_id,
        t.ended_reason
            .as_deref()
            .map(|r| format!("\t{r}"))
            .unwrap_or_default()
    )
}

/// An execution's budget in dollars: spend since the last reset, the limit,
/// what is reserved and held, and what came before dollar budgets.
pub fn budget_line(b: &theseus_protocol::BudgetInfo) -> String {
    let mut s = format!(
        "budget ${:.4} of ${:.2} (reserved ${:.4}, held ${:.4})",
        b.spent_usd, b.limit_usd, b.reserved_usd, b.held_unknown_usd
    );
    if b.resets > 0 {
        s.push_str(&format!(
            " · {} reset{}",
            b.resets,
            if b.resets == 1 { "" } else { "s" }
        ));
    }
    if let Some(q) = &b.question {
        s.push_str(&format!(" · at its limit: theseus confirm {q}"));
    }
    if !b.units_before.is_null() {
        s.push_str(&format!(
            " · before dollars: {} of {} units",
            b.units_before["spent"], b.units_before["limit"]
        ));
    }
    s
}

/// `wakes: 2 pending · next 3f9a1c 2026-09-30 13:15:00 -07:00 (in 9m): check
/// the build`, or nothing when none is pending.
/// The sessions that hold external text (theseus-9bp), as health and `policy
/// list` say them: how many, the first three with what each read and since
/// when, and the way to trust one again.
pub fn external_line(held: &[theseus_protocol::ExternalTextInfo]) -> Option<String> {
    if held.is_empty() {
        return None;
    }
    let each: Vec<String> = held
        .iter()
        .take(3)
        .map(|i| {
            let name = match &i.task {
                Some(t) => format!("task {t}"),
                None => short_id(&i.session_id),
            };
            format!(
                "{name} since {} ({}{})",
                held_since(&i.since_local, &i.held),
                held_what(&i.held),
                match i.held.via.as_deref() {
                    Some("task.create") => ", from the session that started it",
                    Some("task.report") => ", from a task's report",
                    _ => "",
                }
            )
        })
        .collect();
    let more = match held.len() {
        n if n > 3 => format!(", and {} more", n - 3),
        _ => String::new(),
    };
    Some(format!(
        "external text: {} session{} read it, so their calls that act wait: {}{more} · trust one \
         again: theseus policy trust <session>",
        held.len(),
        if held.len() == 1 { "" } else { "s" },
        each.join("; ")
    ))
}

/// What a session read, as the hold's reason names it (theseus-qiy): a
/// search by its query, anything else by its URL, cut to 72 characters.
pub fn held_what(h: &theseus_protocol::ExternalText) -> String {
    let what = h.what();
    if what.chars().count() <= 72 {
        return what;
    }
    format!("{}…", what.chars().take(71).collect::<String>())
}

/// When a hold began: the daemon's local time, as the reason says it
/// (theseus-qiy), or UTC from a daemon that sends none.
pub fn held_since(since_local: &str, h: &theseus_protocol::ExternalText) -> String {
    if since_local.is_empty() {
        fmt_time(h.since_ms)
    } else {
        since_local.to_string()
    }
}

/// A session id as people name it: `…` and its last six characters.
pub fn short_id(id: &str) -> String {
    let n = id.chars().count();
    format!(
        "…{}",
        id.chars().skip(n.saturating_sub(6)).collect::<String>()
    )
}

/// What `theseus policy trust` prints: the session, what it had read, and
/// that its calls that act run at their postures again.
pub fn trusted_line(r: &theseus_protocol::TrustResult) -> String {
    format!(
        "trusted session {} again (by {}) · it had read {} since {} · its calls that act run at \
         their postures again, until it reads external text again",
        r.session_id,
        r.by,
        r.held.what(),
        held_since(&r.since_local, &r.held)
    )
}

/// Health's `[approval]` (theseus-sgh): who may answer, and each listed
/// channel's state, with the reason for any that is not trusted.
/// `theseus policy list`: every tool's posture now and what set it, then any
/// tightening of a tool this daemon does not register.
pub fn policy_list(l: &ToolListResult, tightenings: &[theseus_protocol::Tightening]) -> String {
    let mut out = format!("{:<12} {:<8} set by\n", "tool", "posture");
    for t in &l.tools {
        let set_by = match &t.tightened {
            Some(x) if t.policy != t.config_posture => format!(
                "{} at {}{} (the config says {}, {})",
                t.setting,
                fmt_time(x.at_ms),
                x.correlation_id
                    .as_deref()
                    .map(|c| format!(", from {c}"))
                    .unwrap_or_default(),
                t.config_posture,
                t.config_setting
            ),
            Some(x) => format!(
                "{} (also tightened by {} at {})",
                t.setting,
                x.by,
                fmt_time(x.at_ms)
            ),
            None => t.setting.clone(),
        };
        out.push_str(&format!("{:<12} {:<8} {set_by}\n", t.name, t.policy));
    }
    for x in tightenings
        .iter()
        .filter(|x| !l.tools.iter().any(|t| t.name == x.tool))
    {
        out.push_str(&format!(
            "{:<12} {:<8} tightened by {} at {} (not a tool this daemon has)\n",
            x.tool,
            x.posture,
            x.by,
            fmt_time(x.at_ms)
        ));
    }
    let n = tightenings.len();
    out.push_str(&match n {
        0 => "no tightenings: every posture is the config's\n".to_string(),
        _ => format!(
            "{n} tightening{} · undo one with `theseus policy untighten <tool>`\n",
            if n == 1 { "" } else { "s" }
        ),
    });
    out
}

/// One line saying what `policy tighten` (`tightened`) or `policy untighten`
/// did.
pub fn tightened_line(r: &theseus_protocol::TightenResult, tightened: bool) -> String {
    match (tightened, r.already, r.changed) {
        (true, true, _) => format!(
            "{} already asks first: tightened by {} at {}",
            r.tool,
            r.tightening.by,
            fmt_time(r.tightening.at_ms)
        ),
        (true, false, true) => format!(
            "{} now asks first: tightened by {} (the config says {}, {}) · undo: theseus policy untighten {}",
            r.tool, r.by, r.config_posture, r.config_setting, r.tool
        ),
        (true, false, false) => format!(
            "{} already asks ({}); tightened by {} as well, so it keeps asking if the config changes",
            r.tool, r.config_setting, r.by
        ),
        (false, _, true) => format!(
            "{} is back to what the config says: {} ({}); the tightening by {} is undone",
            r.tool, r.posture, r.setting, r.tightening.by
        ),
        (false, _, false) => format!(
            "the tightening of {} by {} is undone; the config still asks ({})",
            r.tool, r.tightening.by, r.setting
        ),
    }
}

/// hh:mm:ss.mmm in local time, without pulling in a date crate.
pub fn fmt_time(unix_ms: u64) -> String {
    let secs = unix_ms / 1000;
    let ms = unix_ms % 1000;
    let s = secs % 86_400;
    format!(
        "{:02}:{:02}:{:02}.{:03}Z",
        s / 3600,
        (s / 60) % 60,
        s % 60,
        ms
    )
}

pub fn fmt_us(us: u64) -> String {
    if us >= 1_000_000 {
        format!("{:.2} s", us as f64 / 1e6)
    } else if us >= 1000 {
        format!("{:.1} ms", us as f64 / 1e3)
    } else {
        format!("{us} µs")
    }
}

pub fn fmt_bytes(b: u64) -> String {
    if b >= 1 << 20 {
        format!("{:.1} MB", b as f64 / (1u64 << 20) as f64)
    } else if b >= 1024 {
        format!("{:.1} KB", b as f64 / 1024.0)
    } else {
        format!("{b} B")
    }
}

pub fn fmt_tokens(n: u64) -> String {
    if n >= 1_000_000 && n.is_multiple_of(1_000_000) {
        format!("{}M", n / 1_000_000)
    } else if n >= 1000 {
        format!("{}K", n / 1000)
    } else {
        n.to_string()
    }
}

pub fn fmt_price(p: f64) -> String {
    if p == 0.0 {
        "-".into()
    } else {
        format!("{p:.3}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

/// `theseus health`: every line, `now_ms` for the ones that say how long ago.
pub fn print_health(
    w: &mut impl Write,
    h: &theseus_protocol::HealthResult,
    now_ms: u64,
) -> io::Result<()> {
    writeln!(
        w,
        "{} {} · protocol {} · up {}s · live profile {} ({}/{}) · providers [{}] · sessions {} · turns {} · provider errors {} · ledger rows {}",
        h.name, h.version, h.protocol, h.uptime_secs, h.profile, h.provider, h.model, h.providers.join(", "), h.sessions, h.turns, h.provider_errors, h.ledger_rows
    )?;
    writeln!(w, "telemetry: {}", h.telemetry.summary(now_ms))?;
    let k = &h.kernel;
    let fmt_counts = |m: &std::collections::BTreeMap<String, u64>| {
        if m.is_empty() {
            "none".to_string()
        } else {
            m.iter()
                .map(|(s, n)| format!("{n} {s}"))
                .collect::<Vec<_>>()
                .join(", ")
        }
    };
    writeln!(
        w,
        "kernel: {} · turns held {}/{} · executions [{}] · actions [{}] · quarantined completions {}{}",
        if k.accepting { "accepting" } else { "starting" },
        k.turns_held,
        k.admission_ceiling,
        fmt_counts(&k.executions_by_state),
        fmt_counts(&k.actions_by_state),
        k.quarantined_completions,
        lingering_note(k.lingering_wrappers)
    )?;
    if let Some(line) = children_line(&h.children) {
        writeln!(w, "{line}")?;
    }
    if let Some(line) = disk_line(&h.disk) {
        writeln!(w, "{line}")?;
    }
    if let Some(line) = spool_line(&h.spool, now_ms) {
        writeln!(w, "{line}")?;
    }
    if let Some(p) = &h.push {
        writeln!(w, "{}", push_line(p))?;
    }
    writeln!(w, "{}", broker_line(&h.broker))?;
    writeln!(
        w,
        "tokens total: in {} out {} cache-read {} cache-write {}",
        h.usage_total.input_tokens,
        h.usage_total.output_tokens,
        h.usage_total.cache_read_input_tokens,
        h.usage_total.cache_creation_input_tokens,
    )?;
    if let Some(line) = config_line(&h.config) {
        writeln!(w, "{line}")?;
    }
    if let Some(line) = context_line(&h.context) {
        writeln!(w, "{line}")?;
    }
    writeln!(w, "{}", secrets_line(&h.secrets, &h.secrets_resolved))?;
    if let Some(line) = startup_line(&h.startup) {
        writeln!(w, "{line}")?;
    }
    if let Some(line) = store_history_line(&h.startup) {
        writeln!(w, "{line}")?;
    }
    for b in &h.bindings {
        writeln!(w, "{}", binding_line(b))?;
        if let Some(o) = &b.outbox {
            writeln!(w, "{}", outbox_line(&b.kind, o))?;
        }
    }
    print_approval(w, &h.approval)?;
    if let Some(line) = wakes_line(&h.wakes, now_ms) {
        writeln!(w, "{line}")?;
    }
    if !h.tightenings.is_empty() {
        let t: Vec<String> = h
            .tightenings
            .iter()
            .map(|t| format!("{} (by {}, {})", t.tool, t.by, fmt_time(t.at_ms)))
            .collect();
        writeln!(
            w,
            "tightened (should have asked): {} · undo: theseus policy untighten <tool>",
            t.join(", ")
        )?;
    }
    if let Some(line) = external_line(&h.external_text) {
        writeln!(w, "{line}")?;
    }
    Ok(())
}

/// A binding's health line: its state, its counts, and each place's session.
fn binding_line(b: &theseus_protocol::BindingStatus) -> String {
    let places: Vec<String> = b
        .places
        .iter()
        .map(|p| {
            format!(
                "{} → {}",
                p.label,
                p.session_id.as_deref().unwrap_or("no session")
            )
        })
        .collect();
    format!(
        "{}: {}{} · {} in · {} sent · {} edits · {} presses · {} ignored · {} errors{}",
        b.kind,
        b.state,
        b.detail
            .as_deref()
            .map(|d| format!(" ({d})"))
            .unwrap_or_default(),
        b.messages_in,
        b.messages_out,
        b.edits,
        b.interactions,
        b.ignored,
        b.errors,
        if places.is_empty() {
            String::new()
        } else {
            format!(" · {}", places.join(", "))
        }
    )
}

/// The push (theseus-in3), as `theseus health` says it: `push: 2 watchers ·
/// board 212 · 1 question · 340 events · seeded in 38 ms`, or that nothing
/// has watched since the start.
pub fn push_line(p: &theseus_protocol::PushStatus) -> String {
    if !p.seeded {
        return "push: not seeded: nothing has watched since the start (the first \
                executions.watch seeds it)"
            .into();
    }
    let s = |n: u64| if n == 1 { "" } else { "s" };
    format!(
        "push: {} watcher{} · board {} · {} question{} · {} event{} · at position {} · seeded in {}",
        p.watchers,
        s(p.watchers),
        p.board,
        p.questions,
        s(p.questions),
        p.events,
        s(p.events),
        p.position,
        fmt_us(p.seed_us)
    )
}

/// One execution's view, as `theseus watch --all` prints it: its position,
/// its session, its kind, its state (`running → waiting` for a change), its
/// pill, and its spend.
pub fn view_line(v: &theseus_protocol::ExecutionView) -> String {
    let state = match &v.previous {
        Some(p) if *p != v.state => format!("{p} → {}", v.state),
        _ => v.state.clone(),
    };
    format!(
        "{}\t{}\t{}\t{state}\t{}\t${:.4}",
        v.position,
        v.session_id,
        v.kind.as_str(),
        pill(&v.attention),
        v.spent_usd
    )
}

/// A question's arrival or end, as `theseus watch --all` prints it.
pub fn question_line(e: &Event) -> Option<String> {
    match e {
        Event::ConfirmRequested(c) => Some(format!(
            "-\t{}\tconfirm.requested\t{}: {}\t{}",
            c.session_id, c.tool, c.reason, c.correlation_id
        )),
        Event::ConfirmResolved(r) => {
            let how = if r.withdrawn {
                "withdrawn"
            } else if r.stopped {
                "stopped"
            } else if r.cancelled {
                "cancelled"
            } else if r.superseded {
                "superseded"
            } else if r.approved {
                "approved"
            } else {
                "declined"
            };
            Some(format!(
                "-\t{}\tconfirm.resolved\t{how}{}\t{}",
                r.session_id,
                r.by.as_deref()
                    .map(|b| format!(" by {b}"))
                    .unwrap_or_default(),
                r.correlation_id
            ))
        }
        _ => None,
    }
}

/// A level's mark, as every surface draws it (design `stage2` §2.9): ●
/// needs you, ◐ working, ○ ready, · idle. A client's "done until seen" is ◆.
pub fn glyph(level: Level) -> char {
    match level {
        Level::NeedsYou => '●',
        Level::Working => '◐',
        Level::Ready => '○',
        Level::Idle => '·',
    }
}

/// What an execution needs from people, as a pill: `● confirm proc.run: run
/// cargo test` (theseus-in3).
pub fn pill(a: &Attention) -> String {
    format!("{} {}", glyph(a.level), a.label)
}

/// One session, as `theseus sessions` lists it: its attention where the
/// state goes, from a daemon that sends one (theseus-in3).
pub fn session_row(s: &SessionInfo) -> String {
    format!(
        "{}\t{}\tturns={}\ttools={}\t${:.4}\tin={}\tout={}\t{}\t{}\t{}",
        s.session_id,
        fmt_time(s.last_active_ms.max(s.created_at_unix_ms)),
        s.turns,
        s.tool_calls,
        s.cost_usd,
        s.usage.input_tokens,
        s.usage.output_tokens,
        match &s.attention {
            Some(a) => pill(a),
            None => s.execution_state.clone().unwrap_or_else(|| "-".into()),
        },
        s.model.as_deref().unwrap_or("-"),
        s.label
            .as_deref()
            .or(s.title.as_deref())
            .unwrap_or_default()
    )
}

/// One execution, as `theseus executions` lists it: its attention after its
/// state, from a daemon that sends one (theseus-in3).
pub fn execution_row(e: &theseus_protocol::ExecutionInfo) -> String {
    format!(
        "{}\t{}\t{}{}\tturns={}\tinterrupted={}\toutstanding={}\tqueued={}\t{}\tsession={}{}",
        e.execution_id,
        e.kind,
        e.state,
        e.attention
            .as_ref()
            .map(|a| format!("\t{}", pill(a)))
            .unwrap_or_default(),
        e.turns,
        e.interrupted,
        e.outstanding,
        e.queued_results,
        budget_line(&e.budget),
        e.session_id,
        e.ended_reason
            .as_deref()
            .map(|r| format!("\t{r}"))
            .unwrap_or_default()
    )
}

/// One ledger row, as `theseus ledger` lists it: its data cut at 160
/// characters.
pub fn ledger_row(r: &theseus_protocol::LedgerEntry) -> serde_json::Result<String> {
    let data = serde_json::to_string(&r.data)?;
    let data: String = data.chars().take(160).collect();
    Ok(format!(
        "{:>6}  {}  {:<16} {:<36} {}",
        r.position,
        fmt_time(r.at_unix_ms),
        r.kind,
        r.turn_id.as_deref().unwrap_or_default(),
        data
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::notify;

    /// `theseus wakes` and health's wakes line (DD8).
    /// A task's pill takes the place of its state and what it waits on
    /// (theseus-in3); a daemon that sends none keeps the old words.
    #[test]
    fn a_tasks_line_shows_its_attention() {
        let mut t = theseus_protocol::TaskInfo {
            short: "a1b2c3".into(),
            state: "waiting".into(),
            waiting_on: Some("a job".into()),
            limit_usd: 2.0,
            turns: 1,
            created_at_ms: 1_000,
            ..Default::default()
        };
        assert!(task_line(&t, 61_000).starts_with("a1b2c3\twaiting on a job\t$0.0000 of $2.00"));
        t.attention = Some(Attention {
            level: Level::Working,
            label: "waiting on 1 call".into(),
            since_ms: 0,
        });
        assert!(
            task_line(&t, 61_000).starts_with("a1b2c3\t◐ waiting on 1 call\t"),
            "{}",
            task_line(&t, 61_000)
        );
        assert_eq!(glyph(Level::NeedsYou), '●');
        assert_eq!(glyph(Level::Idle), '·');
    }

    #[test]
    fn a_wake_lists_its_due_time_session_and_note() {
        let w = theseus_protocol::WakeInfo {
            wake_id: "wak_0199aaaa3f9a1c".into(),
            short: "3f9a1c".into(),
            session_id: "ses_1".into(),
            execution_id: "exe_1".into(),
            session_title: None,
            due_at_ms: 1_000_000 + 540_000,
            due_local: "2026-09-30 13:15:00 -07:00".into(),
            note: "check the build\nand the tests".into(),
            set_at_ms: 1_000_000,
            target: Some("discord:dm:42".into()),
            state: "waiting".into(),
        };
        assert_eq!(
            wake_line(&w, 1_000_000),
            "3f9a1c\t2026-09-30 13:15:00 -07:00 (in 9m)\tsession ses_1 (waiting) → discord:dm:42\t\
             check the build"
        );
        assert_eq!(until_due(1_000, 181_000), "due 3m ago");
        assert_eq!(wakes_line(&[], 0), None);
        let later = theseus_protocol::WakeInfo {
            short: "b4f566".into(),
            due_at_ms: w.due_at_ms + 3_600_000,
            ..w.clone()
        };
        assert_eq!(
            wakes_line(&[later, w], 1_000_000).unwrap(),
            "wakes: 2 pending · next 3f9a1c 2026-09-30 13:15:00 -07:00 (in 9m): check the build · \
             theseus wakes, theseus cancel <id>"
        );
    }

    /// `theseus health`'s context line (theseus-c48).
    #[test]
    fn the_context_line_names_the_levels_and_the_persona_in_play() {
        use theseus_protocol::ContextStatus;
        assert_eq!(context_line(&ContextStatus::default()), None);
        let c = ContextStatus {
            system_files: vec!["~/a.md".into(), "~/b.md".into()],
            persona: Some("theseus".into()),
            persona_files: vec!["~/p.md".into()],
            personas: vec!["theseus".into()],
        };
        assert_eq!(
            context_line(&c).unwrap(),
            "context: 2 files at the system level · persona theseus (1 file)"
        );
        let none = ContextStatus {
            persona: None,
            persona_files: vec![],
            ..c
        };
        assert_eq!(
            context_line(&none).unwrap(),
            "context: 2 files at the system level · no persona in play (defined: theseus; set \
             [context] default_persona)"
        );
    }

    fn tool(name: &str, policy: &str, config: &str, setting: &str) -> theseus_protocol::ToolInfo {
        theseus_protocol::ToolInfo {
            name: name.into(),
            wire_name: name.replace('.', "_"),
            family: String::new(),
            description: String::new(),
            class: "run".into(),
            backend: "job".into(),
            policy: policy.into(),
            setting: setting.into(),
            config_posture: config.into(),
            config_setting: "enforcement = notify".into(),
            tightened: None,
            input_schema: Value::Null,
            calls: 0,
        }
    }

    /// `theseus watch` says what a job's process tried, and why it was
    /// refused (theseus-6qy).
    #[test]
    fn a_jobs_refused_answer_is_one_line() {
        let why = "from a Theseus job's process (job act_j, pid 42, theseus)";
        let refused = |v: Value| serde_json::from_value::<ApprovalRefused>(v).unwrap();
        assert_eq!(
            job_refusal_line(&refused(
                serde_json::json!({"act": "action.confirm", "tool": "fs.write", "via": "cli", "why": why})
            )),
            format!("refused an answer to fs.write {why} through cli")
        );
        assert_eq!(
            job_refusal_line(&refused(
                serde_json::json!({"act": "policy.untighten", "tool": "fs.edit", "via": "web", "why": why})
            )),
            format!("refused the undo of fs.edit's tightening {why} through web")
        );
    }

    /// The kernel line counts the job wrappers that linger, and says nothing
    /// when none does (theseus-6qy).
    #[test]
    fn the_kernel_line_counts_lingering_wrappers() {
        assert_eq!(lingering_note(0), "");
        assert_eq!(
            lingering_note(1),
            " · 1 job wrapper lingers for what its command left running"
        );
        assert_eq!(
            lingering_note(3),
            " · 3 job wrappers linger for what their commands left running"
        );
    }

    /// The broker line names each grant and its uses, never a value
    /// (theseus-dcy), and says when there is none.
    #[test]
    fn the_broker_line_names_each_grant_and_its_uses() {
        use theseus_protocol::GrantStatus;
        assert_eq!(broker_line(&[]), "broker: no grants");
        let gh = GrantStatus {
            kind: "program".into(),
            to: "gh".into(),
            variable: Some("GH_TOKEN".into()),
            secret: "github_token".into(),
            posture: "notify".into(),
            uses: 3,
        };
        let search = GrantStatus {
            kind: "tool".into(),
            to: "web.search".into(),
            variable: None,
            secret: "brave_api_key".into(),
            posture: "notify".into(),
            uses: 1,
        };
        assert_eq!(
            broker_line(&[gh, search]),
            "broker: gh gets GH_TOKEN (github_token, notify), used 3 times · web.search gets \
             brave_api_key (brave_api_key, notify), used once"
        );
    }

    /// The children line says what the daemon holds and has reaped, and
    /// nothing for a daemon older than theseus-z4b.
    #[test]
    fn the_disk_line_says_free_space_and_its_limits() {
        use theseus_protocol::DiskStatus;
        assert_eq!(disk_line(&DiskStatus::default()), None, "an older daemon");
        let mut d = DiskStatus {
            path: "/invented/state".into(),
            state: "ok".into(),
            free_mb: 81_920,
            total_mb: 1_006_712,
            warn_mb: 5120,
            floor_mb: 1024,
            error: None,
        };
        assert_eq!(
            disk_line(&d).unwrap(),
            "disk: 81,920 MB free of 1,006,712 MB under /invented/state · health warns below \
             5,120 MB, jobs are refused below 1,024 MB"
        );
        d.state = "low".into();
        d.free_mb = 4000;
        assert!(disk_line(&d)
            .unwrap()
            .starts_with("disk: LOW, 4,000 MB free"));
        d.state = "below_floor".into();
        d.free_mb = 812;
        d.warn_mb = 0;
        assert_eq!(
            disk_line(&d).unwrap(),
            "disk: BELOW THE FLOOR, new jobs are refused: 812 MB free of 1,006,712 MB under \
             /invented/state · jobs are refused below 1,024 MB"
        );
        d.state = "unknown".into();
        d.error = Some("invented failure".into());
        assert_eq!(
            disk_line(&d).unwrap(),
            "disk: unknown under /invented/state: invented failure"
        );
    }

    #[test]
    fn the_spool_line_says_the_last_sweep() {
        use theseus_protocol::{SpoolStatus, SpoolSweep};
        assert_eq!(spool_line(&SpoolStatus::default(), 0), None);
        let sweep = SpoolSweep {
            at_unix_ms: 1_000_000,
            took_ms: 2,
            removed: 3,
            removed_bytes: 1_024,
            kept: 2,
            kept_bytes: 10,
            removed_by: [("absorbed", 1), ("ended", 1), ("unknown", 1)]
                .map(|(k, v)| (k.to_string(), v))
                .into(),
            kept_by: [("pending", 1), ("running", 1)]
                .map(|(k, v)| (k.to_string(), v))
                .into(),
        };
        let s = SpoolStatus {
            last_sweep: Some(sweep),
        };
        assert_eq!(
            spool_line(&s, 1_000_000 + 180_000).unwrap(),
            "spool: swept 3 min ago: removed 3 files (1,024 bytes): 1 absorbed, 1 ended, 1 unknown \
             · kept 2: 1 pending, 1 running"
        );
        assert!(spool_line(&s, 1_005_000)
            .unwrap()
            .starts_with("spool: swept 5 s ago"));
    }

    #[test]
    fn the_children_line_counts_wrappers_orphans_and_zombies() {
        use theseus_protocol::ChildrenStatus;
        assert_eq!(children_line(&ChildrenStatus::default()), None);
        let c = ChildrenStatus {
            subreaper: true,
            wrappers_running: 2,
            wrappers_lingering: 1,
            orphans: 1,
            zombies: 0,
            owned: 1,
            reaped_wrappers: 160,
            reaped_orphans: 2,
        };
        assert_eq!(
            children_line(&c).unwrap(),
            "children: 2 job wrappers running, 1 lingering · 1 adopted orphan · 0 zombies · \
             reaped 160 wrappers, 2 orphans"
        );
        let stdio = ChildrenStatus {
            reaped_wrappers: 1,
            ..ChildrenStatus::default()
        };
        assert_eq!(
            children_line(&stdio).unwrap(),
            "children: 0 job wrappers running · 0 adopted orphans · 0 zombies · reaped 1 \
             wrapper, 0 orphans · not a subreaper: a job that kills its wrapper leaves its \
             orphans to init"
        );
    }

    /// A call a `/stop` ended reads as stopped, by whom, in `theseus watch`
    /// and the history, never as `cancelled`; a cancel's call still reads
    /// cancelled, and a decline not run (theseus-4uw).
    #[test]
    fn a_call_a_stop_ended_reads_stopped_in_watch_and_history() {
        let ended = |v: Value| serde_json::from_value::<ToolEnded>(v).unwrap();
        let stopped = ended(
            serde_json::json!({"tool": "proc.run", "status": "cancelled",
            "duration_ms": 2100, "bytes": 40, "stopped_by": "the CLI"}),
        );
        assert_eq!(
            tool_ended_line(&stopped),
            "  ← proc.run stopped by the CLI · 2100 ms · 40 B"
        );
        let cancelled = ended(
            serde_json::json!({"tool": "proc.run", "status": "cancelled",
            "duration_ms": 5, "bytes": 0}),
        );
        assert_eq!(
            tool_ended_line(&cancelled),
            "  ← proc.run cancelled · 5 ms · 0 B"
        );
        assert_eq!(
            result_word("ok", Some("the CLI")),
            "ok",
            "it finished first"
        );
        assert_eq!(result_word("declined", None), "not run");
    }

    /// `theseus policy list` says what set each posture, and marks a
    /// tightening with who, when, and the call; `tighten` and `untighten`
    /// say what changed (theseus-sgh).
    #[test]
    fn policy_list_and_its_lines_say_what_set_each_posture() {
        let t = theseus_protocol::Tightening {
            tool: "proc.run".into(),
            posture: "approve".into(),
            by: "discord:eddie".into(),
            at_ms: 3_600_000,
            correlation_id: Some("act_1".into()),
            ..Default::default()
        };
        let mut run = tool(
            "proc.run",
            "approve",
            "notify",
            "tightened by discord:eddie",
        );
        run.tightened = Some(t.clone());
        let l = ToolListResult {
            tools: vec![
                run,
                tool("fs.write", "notify", "notify", "enforcement = notify"),
            ],
            roots: vec![],
            shell_fallback_ratio: 0.0,
            calls_total: 0,
        };
        let gone = theseus_protocol::Tightening {
            tool: "mcp:x/y".into(),
            ..t.clone()
        };
        let out = policy_list(&l, &[t.clone(), gone]);
        assert!(
            out.contains(
                "proc.run     approve  tightened by discord:eddie at 01:00:00.000Z, from act_1 \
                 (the config says notify, enforcement = notify)"
            ),
            "{out}"
        );
        assert!(
            out.contains("fs.write     notify   enforcement = notify"),
            "{out}"
        );
        assert!(
            out.contains("mcp:x/y      approve  tightened by discord:eddie"),
            "{out}"
        );
        assert!(out.ends_with("2 tightenings · undo one with `theseus policy untighten <tool>`\n"));
        let r = theseus_protocol::TightenResult {
            tool: "proc.run".into(),
            by: "sock#3".into(),
            tightening: t,
            posture: "notify".into(),
            setting: "enforcement = notify".into(),
            config_posture: "notify".into(),
            config_setting: "enforcement = notify".into(),
            changed: true,
            already: false,
        };
        assert_eq!(
            tightened_line(&r, false),
            "proc.run is back to what the config says: notify (enforcement = notify); the \
             tightening by discord:eddie is undone"
        );
        assert!(
            tightened_line(&r, true).starts_with("proc.run now asks first: tightened by sock#3")
        );
    }

    /// `theseus health` says where the secrets stand and names a failure
    /// with its reason; the startup line splits the start path from what
    /// follows it (theseus-qa0).
    #[test]
    fn health_says_resolving_ready_or_failed_and_times_the_start() {
        use theseus_protocol::{SecretFailed, SecretsStatus, StartupPhase};
        let resolving = SecretsStatus {
            state: "resolving".into(),
            resolving: vec!["a".into(), "b".into()],
            ..Default::default()
        };
        assert_eq!(
            secrets_line(&resolving, &[]),
            "secrets: resolving · 0 of 2 ready so far"
        );
        let failed = SecretsStatus {
            state: "failed".into(),
            ready: vec!["a".into()],
            failed: vec![SecretFailed {
                name: "b".into(),
                error: "could not find item".into(),
            }],
            method: Some("inject, then read".into()),
            settled_ms: Some(1720),
            retry_in_ms: Some(4000),
            ..Default::default()
        };
        assert_eq!(
            secrets_line(&failed, &[]),
            "secrets: failed b · 1 ready 1720 ms after start (inject, then read) [a]\n  b did not \
             resolve: could not find item\n  fetched again in 4 s"
        );
        assert_eq!(
            secrets_line(&SecretsStatus::default(), &["a".into()]),
            "secrets [a]",
            "an older daemon"
        );
        let phase = |name: &str, bg: bool, start: u64, end: Option<u64>| StartupPhase {
            name: name.into(),
            background: bg,
            start_us: start,
            end_us: end,
            detail: Value::Null,
        };
        let line = startup_line(&[
            phase("config", false, 100, Some(900)),
            phase("socket", false, 11_000, Some(11_400)),
            phase("secrets", true, 1000, None),
        ])
        .unwrap();
        // No phase names the 10.1 ms from config's end to the socket's
        // start, nor the 0.1 ms before config: 10.2 ms in all.
        assert_eq!(
            line,
            "startup: serving at 11.4 ms (config 800 µs · socket 400 µs · 10.2 ms between phases) \
             · after: secrets running"
        );
        assert!(startup_line(&[]).is_none());
    }

    /// `store: CORRUPT …` in `theseus health` when the history check after
    /// serving (theseus-8ni) found a frame that does not check; nothing while
    /// it runs or when the history is whole.
    #[test]
    fn health_is_loud_when_the_store_history_is_corrupt() {
        let verify = |detail: Value, end: Option<u64>| theseus_protocol::StartupPhase {
            name: "store.verify".into(),
            background: true,
            start_us: 1000,
            end_us: end,
            detail,
        };
        assert!(store_history_line(&[verify(Value::Null, None)]).is_none());
        assert!(
            store_history_line(&[verify(serde_json::json!({"outcome": "ok"}), Some(9))]).is_none()
        );
        let line = store_history_line(&[verify(
            serde_json::json!({"outcome": "corrupt", "error": "corrupt frame in segment 1 at offset 0: crc mismatch"}),
            Some(9),
        )])
        .unwrap();
        assert!(
            line.starts_with("store: CORRUPT history, corrupt frame in segment 1 at offset 0"),
            "{line}"
        );
    }

    /// `config:` in `theseus health` (theseus-2fo): a file, a read before
    /// serving, confirming, confirmed from the copy, and held after a restart
    /// onto a changed note, with the next read.
    #[test]
    fn health_says_where_the_config_came_from_and_whether_it_may_act() {
        use theseus_protocol::{ConfigRestart, ConfigStatus};
        let vault = |state: &str, from: &str| ConfigStatus {
            source: "vault".into(),
            reference: "op://V/c/notesPlain".into(),
            state: state.into(),
            started_from: from.into(),
            ..Default::default()
        };
        assert!(
            config_line(&ConfigStatus::default()).is_none(),
            "an older daemon"
        );
        let file = ConfigStatus {
            source: "file".into(),
            reference: "/x/c.toml".into(),
            state: "confirmed".into(),
            started_from: "file".into(),
            ..Default::default()
        };
        assert_eq!(config_line(&file).unwrap(), "config: file /x/c.toml");
        let first = ConfigStatus {
            detail: Some("there was no copy yet; the copy is kept for the next start".into()),
            confirmed_ms: Some(1012),
            ..vault("confirmed", "vault")
        };
        assert_eq!(
            config_line(&first).unwrap(),
            "config: vault (read before serving, in 1012 ms: there was no copy yet; the copy is \
             kept for the next start)"
        );
        assert_eq!(
            config_line(&vault("confirming", "copy")).unwrap(),
            "config: confirming · serving from the copy of op://V/c/notesPlain; nothing acts \
             until the vault confirms it"
        );
        let ok = ConfigStatus {
            confirmed_ms: Some(1034),
            detail: Some("the same text as the copy".into()),
            ..vault("confirmed", "copy")
        };
        assert_eq!(
            config_line(&ok).unwrap(),
            "config: vault (confirmed in 1034 ms)"
        );
        let held = ConfigStatus {
            detail: Some(
                "the vault's note changed again since the restart; restart to apply".into(),
            ),
            retry_in_ms: Some(10_000),
            restarted: Some(ConfigRestart {
                reference: "op://V/c/notesPlain".into(),
                at_unix_ms: 3_600_000,
                tables: vec!["kernel".into()],
                ..Default::default()
            }),
            ..vault("held", "copy")
        };
        assert_eq!(
            config_line(&held).unwrap(),
            "config: held: the vault's note changed again since the restart; restart to apply\n  \
             the vault is read again in 10 s\n  restarted at 01:00:00.000Z onto the vault's \
             changed note; changed since the copy: kernel"
        );
    }

    /// A printer's mode (theseus-0g4): `Text` streams the reply and ends its
    /// partial line before an event line; `Quiet` prints the events and none
    /// of the reply; `Watch` adds each turn's start; `Json` prints nothing.
    #[test]
    fn each_mode_prints_what_it_says() {
        let delta =
            |text: &str| serde_json::json!({"turn_id": "turn_a", "loop_index": 0, "text": text});
        let events = [
            (
                notify::TURN_STARTED,
                serde_json::json!({"session_id": "ses_a", "turn_id": "turn_a", "continuation": true}),
            ),
            (notify::MODEL_THINKING, delta("hm")),
            (notify::MODEL_DELTA, delta("Half a line")),
            (
                notify::TOOL_STARTED,
                serde_json::json!({"session_id": "ses_a", "turn_id": "turn_a", "tool": "fs.read",
                    "tool_use_id": "tu_1", "correlation_id": "act_1", "backend": "inproc"}),
            ),
            (notify::MODEL_DELTA, delta(" and the rest\n")),
        ];
        let run = |mode, thinking| {
            let mut p = Printer::to(mode, thinking, Vec::new(), Vec::new());
            for (m, v) in &events {
                p.on(m, v);
            }
            p.settle();
            let (out, err) = p.into_parts();
            (
                String::from_utf8(out).unwrap(),
                String::from_utf8(err).unwrap(),
            )
        };
        let reply = "Half a line\n and the rest\n".to_string();
        assert_eq!(
            run(Mode::Text, true),
            (reply.clone(), "  (thinking) hm\n  → fs.read\n".to_string())
        );
        assert_eq!(
            run(Mode::Quiet, true),
            (String::new(), "  (thinking) hm\n  → fs.read\n".to_string())
        );
        assert_eq!(
            run(Mode::Watch, false),
            (
                reply,
                "── turn turn_a (continuation)\n  → fs.read\n".to_string()
            )
        );
        assert_eq!(run(Mode::Json, true), (String::new(), String::new()));
    }
}
