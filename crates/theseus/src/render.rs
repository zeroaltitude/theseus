//! What a terminal shows of the daemon's answers and events (theseus-0g4,
//! finding 11; a library since theseus-7yx, step 10a): a session's live
//! events (`event`), its history (`node_lines`), its questions
//! (`confirm_lines`), a turn's trace (`span_lines`), and every line
//! `theseus health` and the other commands print.
//!
//! Nothing here prints. What a client shows comes back as [`Line`]s, each a
//! text with a [`Tag`]: the CLI prints the text (`print.rs` in its binary),
//! and the TUI styles it by its tag. Lines come back for a session's events,
//! nodes, and questions, for the rows that carry an attention pill (tagged
//! with its level), and for everything that once wrote to a writer. The
//! parts of a line, and the one-line answers only the CLI prints, are
//! `String`s. The golden tests in `tests/golden.rs` hold the bytes.

use serde_json::Value;
use theseus_protocol::{
    Attention, ConfirmRequest, Event, Level, NodeInfo, SessionInfo, ToolEnded, ToolListResult,
    TurnSubmitResult,
};

mod ask;
mod aws;
mod cache;
mod cancel;
mod catalog;
pub mod census;
pub mod cite;
pub mod code;
mod connections;
pub mod content;
pub mod day;
pub mod defer;
pub mod diffview;
pub mod external;
mod failed;
mod features;
mod flight;
pub mod fork;
mod google;
#[cfg(test)]
mod held_tests;
pub mod history;
pub mod horizon;
mod imported;
mod index;
mod judge;
mod judge_runs;
pub mod later;
mod learning;
mod ledger_gap;
mod lsp;
mod mcp;
mod mcp_server;
pub mod md;
mod memory;
pub mod modes;
mod ontology;
pub mod own_bench;
mod parked;
mod places;
pub mod postmortem;
pub mod postmortem_facts;
mod resident;
pub mod rewind;
#[cfg(test)]
mod route_data;
mod rsi;
mod sandbox;
pub mod session_dir;
pub mod sessions;
mod store;
pub mod style;
mod task_graph;
pub mod task_history;
pub mod task_tree;
mod tasks;
pub mod thread;
mod threads_cache;
pub mod time;
mod wakes;
mod warm;
mod watches;
mod web;
pub use aws::{aws_call_line, aws_lines, bootstrap_lines};
pub use cancel::{cancels_line, verdict_lines};
pub use catalog::*;
pub use flight::Flight;
pub use google::{google_lines, status_lines as google_status_lines};
pub use index::{index_hits_lines, index_line, index_status_lines, tender_words};
pub use judge::{judge_line, judge_log_footer, judge_log_lines, judge_show_lines};
pub use judge_runs::{judge_audit_lines, judge_backfill_lines, judge_replay_lines};
pub use learning::{judge_label_line, learning_report_lines};
pub use lsp::lsp_line;
pub use mcp::{mcp_line, mcp_lines};
pub use mcp_server::mcp_server_line;
pub use memory::{consolidated_lines, recall_lines, recalls_lines};
pub use ontology::{
    ontology_categories_lines, ontology_kinds_lines, ontology_memberships_lines,
    ontology_proposals_lines,
};
pub use parked::parked_lines;
pub use places::{places_health_line, places_lines};
pub use rsi::{budget_lines, self_state_line};
pub use sandbox::sandbox_line;
pub use store::{crash_line, node_cache_line, store_lines, store_reads_line};
pub use task_graph::{task_tree_lines, tree_line};
pub use tasks::{task_check, task_pieces};
pub use time::{fmt_date, fmt_time};
pub use wakes::{wake_line, wakes_line};
pub use watches::watch_line;

/// What a line is, as the CLI's marks have always told one from another. The
/// CLI prints a line's text alone, so a tag changes nothing it prints; the
/// TUI styles a line by it. A row that carries no other tag is `Plain`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Tag {
    /// Anything no other tag names: a command's rows and lines, the
    /// operator's message in a history, a span of a turn's trace, health.
    #[default]
    Plain,
    /// The model's reply: its text as it streams, and a reply in a history.
    Reply,
    /// The model's thinking summary.
    Thinking,
    /// A tool call: its start (`→`) and its end (`←`), and a call and its
    /// result in a history (`⚙`, `←`).
    Tool,
    /// A question that waits for you: a call to confirm (`?`), or a session
    /// at its spend limit (`$`).
    Ask,
    /// An answer that approved (`✓`).
    Ok,
    /// What to look at that is no failure: a call that ran with a notice
    /// (`!`), a posture's change (`🔒`), and an answer that did not approve
    /// (`✗`: declined, superseded, or cancelled).
    Warn,
    /// A failure or a refusal: a turn that failed (`✗`), or a job's answer
    /// that was refused (`🚨`).
    Bad,
    /// A line a diff removed (`-`) or added (`+`); the changed words of a
    /// paired line sit between `diffview::MARK_ON` and `MARK_OFF`
    /// (theseus-z6au).
    Del,
    Add,
    /// What goes with the line before it, and a turn's frame: a question's
    /// input and how to answer it, a notice's setting and its undo, a context
    /// decision (`⟳`), a turn's start (`──`) and its status line (`[…]`), and
    /// a reply's usage in a history (`↳`).
    Dim,
    /// A row that carries an attention pill (a session, an execution, a task,
    /// a watched view, a wait's end): the pill's level.
    Level(Level),
}

/// One line: its text, with no newline at its end, and its tag. A renderer
/// that returns several splits its text at its newlines, so each is one row
/// on a screen; one that returns a single row (`session_row`, `task_line`)
/// keeps any newline the daemon's text holds. From `event`, a `Reply` or
/// `Thinking` line is a piece of its stream instead (see `event`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub tag: Tag,
    pub text: String,
}

impl Line {
    pub fn new(tag: Tag, text: impl Into<String>) -> Self {
        Self {
            tag,
            text: text.into(),
        }
    }
}

/// `text` as lines with `tag`, split at its newlines, so each line is one
/// row. Joined again by newlines, they are the text: a reason or an error
/// that holds a newline prints as it always did.
fn push(out: &mut Vec<Line>, tag: Tag, text: &str) {
    out.extend(text.split('\n').map(|t| Line::new(tag, t)));
}

/// Which of a session's events `event` shows. `ask` shows the reply and the
/// calls, and `watch` each turn's start and end too.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Show {
    /// The reply as it streams (`ask --no-stream` prints it once, at the
    /// end, instead).
    pub reply: bool,
    /// The model's thinking summaries.
    pub thinking: bool,
    /// Each turn's start and end, and every context decision rather than
    /// only a recompile (`watch`).
    pub turns: bool,
}

/// A capture's notice and its undo, or its ask and the answers (theseus-slvl.5).
fn standing_noticed(out: &mut Vec<Line>, n: &theseus_protocol::StandingNoticed) {
    for (i, l) in n.lines().iter().enumerate() {
        push(out, if i == 0 { Tag::Ask } else { Tag::Dim }, l);
    }
}

/// What one of a session's events shows, as lines: none for one it does not
/// show. A `Reply` or `Thinking` line is a piece of its stream, as it came:
/// it may hold several lines, or end inside one, and the next piece goes on
/// where it stopped. A line of any other tag is whole, and a client ends a
/// stream's open line before it.
#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
pub fn event(e: &Event, show: Show) -> Vec<Line> {
    let mut out = Vec::new();
    match e {
        Event::ModelDelta(d) if show.reply => out.push(Line::new(Tag::Reply, d.text.as_str())),
        Event::ModelThinking(d) if show.thinking => {
            out.push(Line::new(Tag::Thinking, d.text.as_str()));
        }
        Event::ToolStarted(s) => {
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
            // An L1 job says so (M4 17b), and what it may reach (18c).
            let l1 = match (s.class.as_deref(), &s.egress) {
                (Some("l1"), e) => {
                    let reach = theseus_protocol::sandbox::reach(e.as_deref().unwrap_or(&[]));
                    format!(" 🛡️ L1 · {reach}")
                }
                _ => String::new(),
            };
            push(&mut out, Tag::Tool, &format!("  → {}{argv}{l1}", s.tool));
        }
        Event::ToolEnded(t) => push(&mut out, Tag::Tool, &tool_ended_line(t)),
        // An AWS call's line (row 29, C1): its operation, region, and
        // account, which its `→` line cannot say.
        Event::ToolProposed(p) => {
            if let Some(a) = p.gate.plan.as_ref().and_then(|pl| pl.aws.as_ref()) {
                push(
                    &mut out,
                    Tag::Tool,
                    &format!("  ☁ {} {}", p.tool, aws_call_line(a)),
                );
            }
            // A terminal's call (theseus-n88g.4): which terminal, and the
            // keys it types, which its `→` line cannot say.
            if let Some(line) = theseus_protocol::term::summary(&p.tool, &p.input) {
                push(&mut out, Tag::Tool, &format!("  ⌨ {} {line}", p.tool));
            }
        }
        Event::ConfirmRequested(c) if c.budget.is_some() => {
            push(&mut out, Tag::Ask, &format!("  $ {}", c.reason));
            budget_answers(&mut out, c);
        }
        // A layer-1 task change (39b): the question in the card's words.
        Event::ConfirmRequested(c) if c.change.is_some() => {
            change_lines(&mut out, c);
        }
        Event::ConfirmRequested(c) if c.tool == theseus_protocol::work::ask::OWNER_ASK => {
            ask::lines(&mut out, c);
        }
        Event::ConfirmRequested(c) => {
            push(
                &mut out,
                Tag::Ask,
                &format!(
                    "  ? {} needs your confirmation{}: {}",
                    c.tool,
                    if c.floor { " (FLOOR)" } else { "" },
                    c.reason
                ),
            );
            answers(&mut out, c);
        }
        Event::PolicyNotified(n) => {
            push(
                &mut out,
                Tag::Warn,
                &format!(
                    "  ! notified: {}: {}{}",
                    n.tool,
                    n.summary,
                    n.granted
                        .as_deref()
                        .map(|g| format!(" · 🔑 {g}"))
                        .unwrap_or_default()
                ),
            );
            push(
                &mut out,
                Tag::Dim,
                &format!(
                    "      ({}) · should have asked: theseus policy tighten {}{}",
                    n.notice.setting,
                    n.tool,
                    if n.correlation_id.is_empty() {
                        String::new()
                    } else {
                        format!(" --call {}", n.correlation_id)
                    }
                ),
            );
        }
        // A notified call's score, after its notice (M5 step 24).
        Event::JudgeScored(j) => push(&mut out, Tag::Warn, &judge::scored_line(j)),
        Event::JudgeNoticed(n) => push(&mut out, Tag::Warn, &judge::noticed_line(n)),
        // What capture did with the owner's words (theseus-slvl.5): the notice, and its undo or its answers.
        Event::StandingNoticed(n) => standing_noticed(&mut out, n),
        // A conversation's mode or plan moved (theseus-lyxy).
        Event::SessionMode(m) => out.extend(modes::event_lines(m)),
        // A reminder fired (theseus-8ux6x): its note, and its four answers.
        Event::WakeNoticed(n) => {
            for (i, l) in wakes::noticed_lines(n).iter().enumerate() {
                push(&mut out, if i == 0 { Tag::Ask } else { Tag::Dim }, l);
            }
        }
        Event::PolicyTightened(r) | Event::PolicyUntightened(r) => {
            let tightened = matches!(e, Event::PolicyTightened(_));
            push(
                &mut out,
                Tag::Warn,
                &format!("  🔒 {}", tightened_line(r, tightened)),
            );
        }
        Event::ConfirmResolved(r) => {
            let by = r.by.as_deref().unwrap_or("");
            let (tag, text) = if r.superseded {
                (
                    Tag::Warn,
                    format!(
                        "  ✗ superseded {} (a new message arrived before an answer)",
                        r.correlation_id
                    ),
                )
            } else if r.cancelled {
                // Its execution was cancelled before an answer (theseus-w98).
                (
                    Tag::Warn,
                    format!(
                        "  ✗ cancelled {} (its execution was cancelled by {by})",
                        r.correlation_id
                    ),
                )
            } else if r.expired {
                // Nobody answered in time (theseus-830).
                (
                    Tag::Warn,
                    format!(
                        "  ✗ expired {} (nobody answered in time; it did not run)",
                        r.correlation_id
                    ),
                )
            } else if r.approved {
                (
                    Tag::Ok,
                    format!("  ✓ approved {} (by {by})", r.correlation_id),
                )
            } else {
                (
                    Tag::Warn,
                    format!("  ✗ declined {} (by {by})", r.correlation_id),
                )
            };
            push(&mut out, tag, &text);
        }
        Event::ContextCompiled(c) if c.decision == "recompile" || show.turns => {
            push(
                &mut out,
                Tag::Dim,
                &format!(
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
                ),
            );
        }
        Event::TurnStarted(t) if show.turns => {
            push(
                &mut out,
                Tag::Dim,
                &format!(
                    "── turn {}{}",
                    t.turn_id,
                    if t.continuation {
                        " (continuation)"
                    } else {
                        ""
                    }
                ),
            );
        }
        Event::TurnEnded(r) if show.turns => {
            for line in r.harness_lines() {
                push(&mut out, Tag::Warn, &line);
            }
            push(&mut out, Tag::Dim, &status_line(r))
        }
        Event::TurnFailed(f) => {
            let then = failed::then_words(f.class.as_deref(), f.then.as_deref());
            push(
                &mut out,
                Tag::Bad,
                &format!(
                    "  ✗ turn failed{}: {}{then}",
                    f.class
                        .as_deref()
                        .map(|c| format!(" ({c})"))
                        .unwrap_or_default(),
                    f.error
                ),
            );
        }
        _ => {}
    }
    out
}

/// How to answer a tool's question, after its first line: its input, then
/// the commands that approve it and decline it.
fn answers(out: &mut Vec<Line>, c: &ConfirmRequest) {
    // What the call would do (an edit's hunk, a command with its directory)
    // stands in for its input, which holds the same text as JSON.
    let shown = diffview::preview_lines(c);
    if shown.is_empty() {
        push(
            out,
            Tag::Dim,
            &format!("      input: {}", clip(&c.input.to_string(), 200)),
        );
    }
    out.extend(shown);
    push(
        out,
        Tag::Dim,
        &format!("      approve: theseus answer {} approve", c.correlation_id),
    );
    push(
        out,
        Tag::Dim,
        &format!("      decline: theseus answer {} decline", c.correlation_id),
    );
}

/// A layer-1 task change's question (39b), as the card words it, and how to
/// accept or decline it.
fn change_lines(out: &mut Vec<Line>, c: &ConfirmRequest) {
    let q = c
        .change
        .as_ref()
        .map(|ch| ch.question())
        .unwrap_or_default();
    push(out, Tag::Ask, &format!("  ? {q}"));
    push(
        out,
        Tag::Dim,
        &format!("      accept: theseus answer {} accept", c.correlation_id),
    );
    push(
        out,
        Tag::Dim,
        &format!("      decline: theseus answer {} decline", c.correlation_id),
    );
}

/// How to answer a budget question, after its first line.
fn budget_answers(out: &mut Vec<Line>, c: &ConfirmRequest) {
    push(
        out,
        Tag::Dim,
        &format!(
            "      reset and continue: theseus answer {} reset",
            c.correlation_id
        ),
    );
    push(
        out,
        Tag::Dim,
        &format!(
            "      keep waiting: theseus answer {} decline",
            c.correlation_id
        ),
    );
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
    // How routing placed the turn (25e): its mode and why, and the effort
    // Jev set when it applied (route.v3, theseus-qe3v).
    let route = r.route.as_ref().map_or(String::new(), |ro| {
        let effort = ro
            .effort_applied
            .as_ref()
            .map_or(String::new(), |e| format!(" · effort {e}"));
        // Who answered and why, when two or more personas could (route.v5).
        let who = ro
            .persona
            .as_ref()
            .map_or(String::new(), |p| format!(" · {}", p.words()));
        let data = ro.data.as_ref().and_then(|d| d.footer());
        let data = data.map_or(String::new(), |d| format!(" · {d}"));
        match (&ro.mode, ro.reason.is_empty()) {
            (Some(m), _) => format!("{who} · {m} ({}){effort}{data}", ro.reason),
            (None, true) => who + &data,
            (None, false) => format!("{who} · route {}{effort}{data}", ro.reason),
        }
    });
    // A held turn made no model call (theseus-2rvwv).
    let who = if r.stop_reason == "held" {
        "held, no model ran".to_string()
    } else {
        format!("{}/{}", r.provider, r.model)
    };
    format!(
        "[{} → {}{} · {} loop(s){}{} · {} · tokens in {} out {}{}{} · {} ms{} · session {}{}{}]",
        r.profile,
        who,
        route,
        r.loops,
        if r.tool_calls > 0 {
            format!(" · {} tool call(s)", r.tool_calls)
        } else {
            String::new()
        },
        ["", " · continuation"][usize::from(r.continuation)],
        r.stop_reason,
        r.usage.input_tokens,
        r.usage.output_tokens,
        cache,
        [r.cost_words(), r.context_words()]
            .into_iter()
            .flatten()
            .fold(String::new(), |a, w| a + " · " + &w),
        r.elapsed_ms,
        r.first_token_ms
            .map(|t| format!(" (first token {t} ms)"))
            .unwrap_or_default(),
        r.session_id,
        inspect_words(r),
        later::left(r)
    )
}

/// The turn in the cockpit, as the status line's last words (theseus-x9swu):
/// the bare address, which terminals link and the line's wrap keeps whole.
fn inspect_words(r: &TurnSubmitResult) -> String {
    r.inspect_url
        .as_ref()
        .map_or(String::new(), |u| format!(" · inspect {u}"))
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
/// their overlap shows however short the group is beside the turn. A line a
/// span, `Plain`.
pub fn span_lines(s: &theseus_protocol::Span, depth: usize, f: &Frame) -> Vec<Line> {
    let mut out = Vec::new();
    span_into(&mut out, s, depth, f);
    out
}

fn span_into(out: &mut Vec<Line>, s: &theseus_protocol::Span, depth: usize, f: &Frame) {
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
        // A judgment's mark names its id whole (M5 23b).
        _ if judge::is_mark(s) => judge::mark_note(s),
        serde_json::Value::Null => String::new(),
        v => {
            let t = serde_json::to_string(v).unwrap_or_default();
            let t: String = t.chars().take(90).collect();
            format!("  {t}")
        }
    };
    push(
        out,
        Tag::Plain,
        &format!(
            "{bar} {:>9}  {}{} [{}]{}",
            dur,
            "  ".repeat(depth),
            s.name,
            s.kind,
            attrs
        ),
    );
    let own = Frame {
        from_us: s.start_us,
        total_us: s.duration_us().max(1),
        grouped: true,
    };
    let f = if s.kind == "tools" { &own } else { f };
    for c in &s.children {
        span_into(out, c, depth + 1, f);
    }
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
    extra.extend(t.verified.clone());
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

/// One node of a session's history, as `theseus history` prints it: the
/// operator's message (`Plain`), a reply (`Reply`) with its thinking
/// (`Thinking`, in full) and its usage (`Dim`), and a call and its result
/// (`Tool`).
#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
pub fn node_lines(n: &NodeInfo, full: bool) -> Vec<Line> {
    let mut out = Vec::new();
    let t = fmt_time(n.at_unix_ms);
    let d = &n.detail;
    let s = |k: &str| d.get(k).and_then(Value::as_str).unwrap_or("");
    match n.kind.as_str() {
        "user_message" => {
            let who = match n.author.as_deref() {
                // The harness's own line (a stop nudge, theseus-fh0u).
                Some(a) if a.starts_with("harness:") => "harness".to_string(),
                Some(a) => format!("operator ({a})"),
                None => "operator".to_string(),
            };
            let text = if full {
                format!("[{t}] {who}:\n{}", indent(&n.text, "    "))
            } else {
                format!("[{t}] {who}: {}", clip(&n.text, 300))
            };
            push(&mut out, Tag::Plain, &text);
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
            push(
                &mut out,
                Tag::Reply,
                &format!(
                    "[{t}] {}:{body}{}",
                    theseus_protocol::surface::reply_label(s("model")),
                    if calls > 0 && n.text.is_empty() {
                        format!(" ({calls} tool call(s))")
                    } else {
                        String::new()
                    }
                ),
            );
            if !n.thinking.is_empty() && full {
                push(
                    &mut out,
                    Tag::Thinking,
                    &format!("      (thinking) {}", clip(&n.thinking, 600)),
                );
            }
            push(
                &mut out,
                Tag::Dim,
                &format!(
                    "      ↳ {} · in {} out {}{cost}",
                    s("stop_reason"),
                    d.pointer("/usage/input_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                    d.pointer("/usage/output_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                ),
            );
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
            push(
                &mut out,
                Tag::Tool,
                &format!(
                    "      ⚙ {} {} [{}]",
                    s("tool"),
                    if full { input } else { clip(&input, 160) },
                    gate
                ),
            );
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
            let text = if full {
                format!(
                    "      ← {} {word}{late}{ms} · {}\n{}",
                    s("tool"),
                    fmt_bytes(n.bytes),
                    indent(&n.text, "        ")
                )
            } else {
                format!(
                    "      ← {} {word}{late}{ms} · {}: {}",
                    s("tool"),
                    fmt_bytes(n.bytes),
                    clip(&n.text, 160)
                )
            };
            push(&mut out, Tag::Tool, &text);
        }
        other => push(&mut out, Tag::Plain, &format!("[{t}] {other}")),
    }
    out
}

/// A question that waits, as a list of them prints it (`answer`, `history`,
/// `sessions explain`, `wait`): its first line (`Ask`), then how to answer
/// it (`Dim`).
pub fn confirm_lines(c: &ConfirmRequest) -> Vec<Line> {
    let mut out = Vec::new();
    if c.budget.is_some() {
        push(
            &mut out,
            Tag::Ask,
            &format!("  $ {} waits for you: {}", c.session_id, c.reason),
        );
        budget_answers(&mut out, c);
        return out;
    }
    if c.change.is_some() {
        change_lines(&mut out, c);
        return out;
    }
    if c.tool == theseus_protocol::work::ask::OWNER_ASK {
        ask::lines(&mut out, c);
        return out;
    }
    if !ask::plain_lines(&mut out, c) {
        push(
            &mut out,
            Tag::Ask,
            &format!(
                "  ? {} waits for you in {}: {}",
                c.tool, c.session_id, c.reason
            ),
        );
    }
    answers(&mut out, c);
    out
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
/// (theseus-qa0), then the secrets from outside the vault with their sources
/// (theseus-n88g.1). A daemon older than that reports only the ready names.
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
    if let Some(words) = s.outside_vault_words() {
        line.push_str(&format!("\n  {words}"));
    }
    line
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
    for t in &c.tenders {
        line.push_str(&format!(" · {}", tender_words(t)));
    }
    if !c.subreaper {
        line.push_str(
            " · not a subreaper: a job that kills its wrapper leaves its orphans to init",
        );
    }
    Some(line)
}

/// Said only when this daemon's jobs can write the binary it runs, or that
/// could not be read (review 2's consideration 3); a daemon older than that
/// says nothing.
pub fn binary_line(b: &theseus_protocol::BinaryStatus) -> Option<String> {
    match b.state.as_str() {
        "jobs_can_write" => Some(format!(
            "binary: JOBS CAN WRITE {}: {} (run the builder as its own user: theseusd install \
             --separate)",
            b.path, b.detail
        )),
        "unknown" => Some(format!("binary: unknown: {}", b.detail)),
        _ => None,
    }
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
    let mut line = format!(
        "context: {} at the system level · {persona}",
        files(c.system_files.len())
    );
    for w in &c.warnings {
        line.push_str(&format!(" · warning: {w}"));
    }
    Some(line)
}

/// `config: vault (confirmed in 1034 ms)`, `config: confirming …`, or
/// `config: held: <why>` (theseus-2fo): where the config came from, and what
/// the vault said of the copy this start served from, which acts either way
/// (theseus-zmgb). A daemon older than that says nothing.
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
            "config: confirming · acting on the copy of {}, which the daemon wrote; the vault \
             is being read",
            c.reference
        ),
        (_, state) => format!(
            "config: {state}: {}",
            c.detail.as_deref().unwrap_or("(no reason given)")
        ),
    };
    if c.state == "held" {
        line.push_str(
            "\n  acting on the copy this start served from; the next start reads the vault again",
        );
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
             --repair from a copy, or ask for help)",
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
    for clause in stop_reach(r) {
        out.push_str(" · ");
        out.push_str(&clause);
    }
    out
}

/// What a stop reached past the turn, one clause each, in the order every
/// surface says it: the tasks it cancelled, the jobs that had left something
/// running and were ended (theseus-7kha), and each job it kept, on whose
/// word. The CLI's `stop_line` and the chat pane's answer both take it from
/// here, so a stop is never silent about it on one and spoken on the other.
pub fn stop_reach(r: &theseus_protocol::ExecutionStopResult) -> Vec<String> {
    let mut out = Vec::new();
    if !r.cancelled.is_empty() {
        let shorts: Vec<&str> = r.cancelled.iter().map(|id| id_tail(id)).collect();
        out.push(format!(
            "cancelled {} task(s): {}",
            r.cancelled.len(),
            shorts.join(" ")
        ));
    }
    if !r.left_ended.is_empty() {
        out.push(format!(
            "ended {} job(s) that had left something running: {}",
            r.left_ended.len(),
            r.left_ended.join(" ")
        ));
    }
    for k in &r.left_kept {
        out.push(format!(
            "kept job {} running, approved by {}",
            k.job, k.approved_by
        ));
    }
    out
}

/// The last six characters of an id, how people name a task.
fn id_tail(id: &str) -> &str {
    id.get(id.len().saturating_sub(6)..).unwrap_or(id)
}

/// `in 2h 46m`, or `due 3m ago` (a wake waiting for its busy session): the shared rule, theseus-hnm37.
pub fn until_due(due_ms: u64, now_ms: u64) -> String {
    theseus_protocol::human_time::until(due_ms, now_ms)
}

/// Where a node went, as `theseus reach` prints it (theseus-n4m, step 12a):
/// its reach in a line, `seen by 3 contexts in 2 sessions`, with the first
/// and last exposure of the node and its copies; then the node and each
/// copy, a generation to a line, with the compilations that hold it under
/// it; and a line that says so when the walk stopped short.
pub fn reach_lines(r: &theseus_protocol::NodeReachResult) -> Vec<Line> {
    let all = || {
        std::iter::once(&r.direct)
            .chain(r.descendants.iter().map(|d| &d.exposure))
            .chain(r.forks.iter().map(|f| &f.exposure))
    };
    let first = all().filter_map(|e| e.first_ms).min();
    let last = all().filter_map(|e| e.last_ms).max();
    let mut lines = vec![Line::new(
        Tag::Plain,
        format!(
            "{}: seen by {} in {}{}",
            r.node_id,
            plural(r.totals.contexts, "context", "contexts"),
            plural(r.totals.sessions.into(), "session", "sessions"),
            exposed(first, last)
        ),
    )];
    reach_generation(
        &mut lines,
        format!(
            "generation 0 · {} · {} at {}",
            r.session_id, r.node_id, r.position
        ),
        &r.direct,
    );
    for d in &r.descendants {
        reach_generation(
            &mut lines,
            format!(
                "generation {} · {} · {} at {}, {} {} by the {}",
                d.generation, d.session_id, d.node_id, d.position, d.via, d.from, d.route
            ),
            &d.exposure,
        );
    }
    lines.extend(fork::reach_fork_lines(r));
    if r.partial {
        lines.push(Line::new(
            Tag::Warn,
            "partial: its copies go further than this walk followed (a generation cap, or 256 \
             copies); --generations N follows more, up to 16",
        ));
    }
    lines
}

/// One generation of a reach: its head, what held it, and the compilations
/// that hold it.
fn reach_generation(lines: &mut Vec<Line>, head: String, e: &theseus_protocol::ReachExposure) {
    let held = if e.compilations.is_empty() && e.loops == 0 {
        "no context held it".to_string()
    } else {
        format!(
            "{}, {}{}",
            plural(e.compilations.len() as u64, "compilation", "compilations"),
            plural(e.loops, "loop", "loops"),
            exposed(e.first_ms, e.last_ms)
        )
    };
    lines.push(Line::new(Tag::Plain, format!("  {head}: {held}")));
    for c in &e.compilations {
        lines.push(Line::new(
            Tag::Dim,
            format!(
                "      compilation {} ({}, {})",
                c.compilation_id,
                c.strategy,
                fmt_time(c.created_at_ms)
            ),
        ));
    }
}

/// ` · first 20:41:07.123 · last 20:42:10.456`, or nothing when nothing
/// held it.
fn exposed(first: Option<u64>, last: Option<u64>) -> String {
    match (first, last) {
        (Some(f), Some(l)) => format!(" · first {} · last {}", fmt_time(f), fmt_time(l)),
        _ => String::new(),
    }
}

/// `1 loop`, `2 loops`.
fn plural(n: u64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// One task, as `theseus tasks` lists it (DD7): its short id, its state and
/// what it waits on, its spend of its carved limit, its age, its title, and
/// the session that started it. Tagged with its pill's level, when it has
/// one.
pub fn task_line(t: &theseus_protocol::TaskInfo, now_ms: u64) -> Line {
    let age = now_ms.saturating_sub(t.created_at_ms) / 1000;
    let age = match age {
        0..=59 => format!("{age}s"),
        60..=3599 => format!("{}m", age / 60),
        3600..=86_399 => format!("{}h", age / 3600),
        _ => format!("{}d", age / 86_400),
    };
    let state = tasks::state_word(t, now_ms);
    let mut asks = match t.pending_confirms {
        0 => String::new(),
        n => format!(" · {n} waiting for you (theseus answer)"),
    };
    // Its report starts its parent's next turn (W1).
    if t.wake_parent {
        asks.push_str(" · wakes its parent");
    }
    if let Some(a) = &t.arrangement {
        asks.push_str(&tasks::clip(a));
    }
    // A task of a task says how deep it sits (theseus-tecj5).
    if t.depth > 1 {
        asks.push_str(&format!(" · depth {}", t.depth));
    }
    Line::new(
        level_tag(t.attention.as_ref()),
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
        ),
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
        s.push_str(&format!(" · at its limit: theseus answer {q} reset"));
    }
    if !b.units_before.is_null() {
        s.push_str(&format!(
            " · before dollars: {} of {} units",
            b.units_before["spent"], b.units_before["limit"]
        ));
    }
    s
}

/// A session id as people name it: `…` and its last six characters.
pub fn short_id(id: &str) -> String {
    let n = id.chars().count();
    format!(
        "…{}",
        id.chars().skip(n.saturating_sub(6)).collect::<String>()
    )
}

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
/// All `Plain`.
#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
pub fn health_lines(h: &theseus_protocol::HealthResult, now_ms: u64) -> Vec<Line> {
    let mut out = Vec::new();
    let o = &mut out;
    day::push_head(o, h);
    push(o, Tag::Plain, &format!(
        "{} {} · protocol {} · up {}s · live profile {} ({}/{}) · providers [{}] · sessions {} · turns {} · provider errors {} · ledger rows {}",
        h.name, h.version, h.protocol, h.uptime_secs, h.profile, h.provider, h.model, h.providers.join(", "), imported::sessions_words(h.sessions, &h.imported), h.turns, h.provider_errors, h.ledger_rows
    ));
    imported::push_sync(o, h.imported.sync.as_ref());
    push(
        o,
        Tag::Plain,
        &format!("telemetry: {}", h.telemetry.summary(now_ms)),
    );
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
    push(o, Tag::Plain, &format!(
        "kernel: {} · turns held {}/{} · executions [{}] · actions [{}] · quarantined completions {}{}{}",
        if k.accepting { "accepting" } else { "starting" },
        k.turns_held,
        k.admission_ceiling,
        external::executions(k),
        fmt_counts(&k.actions_by_state),
        k.quarantined_completions,
        lingering_note(k.lingering_wrappers),
        // What a session's spend limit does when reached (theseus-usei).
        k.spend_limit_mode.as_ref().map_or_else(String::new, |m| {
            format!(" · spend limit ${:.2} a session, {m}", k.spend_limit_usd)
        })
    ));
    if let Some(line) = children_line(&h.children) {
        push(o, Tag::Plain, &line);
    }
    // The index tender's line, then the MCP servers' (M7 36b).
    for line in h
        .index
        .as_ref()
        .map(index_line)
        .into_iter()
        .chain(mcp_line(&h.mcp))
    {
        push(o, Tag::Plain, &line);
    }
    // L1's line, the jobs' cgroup's (theseus-a5nv), and the language
    // servers' (L2).
    let sandbox = h.sandbox.as_ref().map(sandbox_line);
    let cgroup = sandbox::cgroup_line(&h.startup);
    for line in sandbox
        .into_iter()
        .chain(cgroup)
        .chain(h.lsp.as_deref().map(lsp_line))
    {
        push(o, Tag::Plain, &line);
    }
    if let Some(line) = cancels_line(&h.cancels) {
        push(o, Tag::Plain, &line);
    }
    places::push_health(o, h.places.as_ref());
    judge::push_health(o, h.judge.as_ref());
    rsi::push_health(o, h.self_improve.as_ref());
    memory::push_health(o, h.memory.as_ref(), now_ms);
    ledger_gap::push_health(o, h.ledger_gap.as_ref());
    mcp_server::push_health(o, h.mcp_server.as_ref());
    parked::push_health(o, h.tasks.as_ref());
    web::push_health(o, &h.web, &h.tools_not_offered);
    if let Some(line) = disk_line(&h.disk) {
        push(o, Tag::Plain, &line);
    }
    if let Some(line) = binary_line(&h.binary) {
        push(o, Tag::Plain, &line);
    }
    if let Some(line) = spool_line(&h.spool, now_ms) {
        push(o, Tag::Plain, &line);
    }
    if let Some(p) = &h.push {
        push(o, Tag::Plain, &push_line(p));
        connections::push_health(o, &p.connections);
    }
    push(o, Tag::Plain, &broker_line(&h.broker));
    if let Some(k) = &h.harness_only {
        push(o, Tag::Plain, &k.line());
    }
    push(
        o,
        Tag::Plain,
        &format!(
            "tokens total: in {} out {} cache-read {} cache-write {}",
            h.usage_total.input_tokens,
            h.usage_total.output_tokens,
            h.usage_total.cache_read_input_tokens,
            cache_write_words(&h.usage_total),
        ),
    );
    if let Some(line) = config_line(&h.config) {
        push(o, Tag::Plain, &line);
    }
    if let Some(line) = context_line(&h.context) {
        push(o, Tag::Plain, &line);
    }
    push(
        o,
        Tag::Plain,
        &secrets_line(&h.secrets, &h.secrets_resolved),
    );
    for line in aws_lines(h.aws.as_ref(), now_ms)
        .into_iter()
        .chain(google::google_lines(h.google.as_ref()))
    {
        push(o, Tag::Plain, &line);
    }
    if let Some(line) = startup_line(&h.startup) {
        push(o, Tag::Plain, &line);
    }
    if let Some(line) = store_history_line(&h.startup) {
        push(o, Tag::Plain, &line);
    }
    for (tag, line) in store_lines(&h.store)
        .into_iter()
        .chain(resident::resident_lines(h.resident.as_ref()))
        .chain(defer::defer_lines(h.tools_defer.as_ref()))
        .chain(warm::warm_lines(h.warm.as_ref(), now_ms))
        .chain(cache::cache_lines(h.cache.as_ref()))
        .chain(threads_cache::threads_lines(h.threads.as_ref()))
    {
        push(o, tag, &line);
    }
    if let Some(c) = &h.crash {
        let tag = if c.this_start { Tag::Bad } else { Tag::Plain };
        push(o, tag, &crash_line(c));
    }
    for b in &h.bindings {
        push(o, Tag::Plain, &binding_line(b));
        if let Some(outbox) = &b.outbox {
            push(o, Tag::Plain, &outbox_line(&b.kind, outbox));
        }
        if let Some(voice) = &b.voice {
            push(o, Tag::Plain, &voice.line());
        }
    }
    if let Some(line) = wakes_line(&h.wakes, now_ms) {
        push(o, Tag::Plain, &line);
    }
    if !h.tightenings.is_empty() {
        let t: Vec<String> = h
            .tightenings
            .iter()
            .map(|t| format!("{} (by {}, {})", t.tool, t.by, fmt_time(t.at_ms)))
            .collect();
        push(
            o,
            Tag::Plain,
            &format!(
                "tightened (should have asked): {} · undo: theseus policy untighten <tool>",
                t.join(", ")
            ),
        );
    }
    // A line per open terminal (theseus-n88g.4), and what their closes left.
    let terms = theseus_protocol::term::health_lines(&h.terminals, &h.terminals_left, now_ms);
    o.extend(terms.into_iter().map(|l| Line::new(Tag::Plain, l)));
    out
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
    ) + &places::places_by_guild(b)
        .map(|g| format!(" · {g}"))
        .unwrap_or_default()
}

/// The push (theseus-in3), as `theseus health` says it: `push: 2 watchers ·
/// 1 waiting · board 212 · 1 question · 340 events · lost 0 · at position
/// 48213 · seeded in 38 ms`, or that nothing has watched since the start.
/// What a connection's backlog cap dropped counts, seeded or not.
pub fn push_line(p: &theseus_protocol::PushStatus) -> String {
    if !p.seeded {
        return format!(
            "push: not seeded: nothing has watched since the start (the first \
             session.watch, executions.watch, or session.wait seeds it) · lost {}",
            p.lost
        );
    }
    let s = |n: u64| if n == 1 { "" } else { "s" };
    // The work board's watchers (theseus-hkx30), named only while one watches.
    let work = if p.work_watchers == 0 {
        String::new()
    } else {
        format!(
            " · {} task watcher{} ({} interactive)",
            p.work_watchers,
            s(p.work_watchers),
            p.interactive
        )
    };
    format!(
        "push: {} watcher{}{work} · {} waiting · board {} · {} question{} · {} event{} · lost {} · at \
         position {} · seeded in {}",
        p.watchers,
        s(p.watchers),
        p.waiting,
        p.board,
        p.questions,
        s(p.questions),
        p.events,
        s(p.events),
        p.lost,
        p.position,
        fmt_us(p.seed_us)
    )
}

/// One execution's view, as `theseus watch --all` prints it: its position,
/// its session, its kind, its state (`running → waiting` for a change), its
/// pill, and its spend. Tagged with the pill's level.
pub fn view_line(v: &theseus_protocol::ExecutionView) -> Line {
    let state = match &v.previous {
        Some(p) if *p != v.state => format!("{p} → {}", v.state),
        _ => v.state.clone(),
    };
    Line::new(
        Tag::Level(v.attention.level),
        format!(
            "{}\t{}\t{}\t{state}\t{}\t${:.4}",
            v.position,
            v.session_id,
            v.kind.as_str(),
            pill(&v.attention),
            v.spent_usd
        ),
    )
}

/// What a wait reached, and the view it ended on: `blocked (already) 48213
/// ses_… conversation waiting ● confirm proc.run: …`, tagged with the view's
/// level.
pub fn waited_line(r: &theseus_protocol::SessionWaitResult) -> Line {
    let head = if r.already {
        format!("{} (already)", r.reached)
    } else {
        r.reached.clone()
    };
    match &r.execution {
        Some(v) => {
            let view = view_line(v);
            Line::new(view.tag, format!("{head}\t{}", view.text))
        }
        None => Line::new(Tag::Plain, head),
    }
}

/// A connection that fell behind (theseus-in3): `lost 865 notifications while
/// this terminal was behind (executions); reading them again`.
pub fn lost_line(l: &theseus_protocol::EventsLost) -> String {
    format!(
        "lost {} notification{} while this terminal was behind ({}); reading again",
        l.dropped,
        if l.dropped == 1 { "" } else { "s" },
        l.streams.join(", ")
    )
}

/// One execution in full, as `theseus sessions explain` prints it: its
/// pill, what it waits on, its pending wakes, its turns and calls, and its
/// budget. Its first line is tagged with the pill's level, the rest `Plain`.
pub fn explain_lines(
    e: &theseus_protocol::ExecutionInfo,
    wakes: &[theseus_protocol::WakeInfo],
    now_ms: u64,
) -> Vec<Line> {
    use theseus_protocol::WaitingOn;
    let mut lines = Vec::new();
    push(
        &mut lines,
        level_tag(e.attention.as_ref()),
        &format!(
            "{}\t{}\t{}{}",
            e.execution_id,
            e.kind,
            e.state,
            e.attention
                .as_ref()
                .map(|a| format!("\t{}", pill(a)))
                .unwrap_or_default()
        ),
    );
    // The rest, `Plain`.
    let mut out = vec![format!(
        "session: {}{}",
        e.session_id,
        e.reports_to
            .as_deref()
            .map(|r| format!(" · reports to {r}"))
            .unwrap_or_default()
    )];
    let waits = match (e.state.as_str(), &e.waiting_on) {
        ("waiting", Some(WaitingOn::Input)) if e.outstanding > 0 => format!(
            "input, with {} call{} still running",
            e.outstanding,
            if e.outstanding == 1 { "" } else { "s" }
        ),
        ("waiting", Some(WaitingOn::Input)) => "input: the operator's next message".into(),
        ("waiting", Some(WaitingOn::Confirm { confirm_id })) => {
            format!("your answer to {confirm_id}")
        }
        ("waiting", Some(WaitingOn::Budget { correlation_id })) => {
            format!("your answer to the budget question {correlation_id}")
        }
        ("waiting", Some(WaitingOn::Actions { correlation_ids })) => {
            format!("calls: {}", correlation_ids.join(", "))
        }
        ("waiting", Some(WaitingOn::Execution { execution_id })) => {
            format!("execution {execution_id}")
        }
        ("waiting", Some(WaitingOn::DueAt { at_ms })) => {
            format!("a due time, {}", until_due(*at_ms, now_ms))
        }
        ("waiting", _) => "something this CLI does not know".into(),
        (state, _) => format!("nothing: it is {state}"),
    };
    out.push(format!("waits on: {waits}"));
    if !wakes.is_empty() {
        let w: Vec<String> = wakes.iter().map(|w| wake_line(w, now_ms)).collect();
        out.push(format!("wakes: {}", w.join("; ")));
    }
    out.push(format!(
        "turns {} · interrupted {} · calls running {} · results queued {}",
        e.turns, e.interrupted, e.outstanding, e.queued_results
    ));
    out.push(budget_line(&e.budget));
    if let Some(r) = &e.ended_reason {
        out.push(format!("ended: {r}"));
    }
    for text in &out {
        push(&mut lines, Tag::Plain, text);
    }
    lines
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
            } else if r.expired {
                "expired"
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

/// A row's tag: its pill's level, or `Plain` from a daemon that sends no
/// pill.
fn level_tag(a: Option<&Attention>) -> Tag {
    a.map_or(Tag::Plain, |a| Tag::Level(a.level))
}

/// One session, as `theseus sessions` lists it: its attention where the
/// state goes, from a daemon that sends one (theseus-in3), and tagged with
/// its level.
pub fn session_row(s: &SessionInfo) -> Line {
    Line::new(
        level_tag(s.attention.as_ref()),
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
            } + &sessions::state_column(s),
            s.model.as_deref().unwrap_or("-"),
            sessions::name_column(s) + &session_dir::column(s) + &content::flag_column(s)
        ),
    )
}

/// One execution, as `theseus sessions --state` lists it: its attention after its
/// state, from a daemon that sends one (theseus-in3), and tagged with its
/// level.
pub fn execution_row(e: &theseus_protocol::ExecutionInfo) -> Line {
    Line::new(
        level_tag(e.attention.as_ref()),
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
        ),
    )
}

#[cfg(test)]
mod tests;
