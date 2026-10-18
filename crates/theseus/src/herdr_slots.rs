//! What each of herdr's slots says about a Theseus pane (theseus-qb1es): the
//! agent label that herdr's own toast reads ("Write the harbour log needs
//! attention"), the state label worded once (`needs you · fs.write log.md`),
//! and the ten tokens the sidebar and the other herdr tools read. One
//! function over a [`WorkView`] and a few facts the view does not carry
//! (`slots`), so a richer view from the work board drops in with no change
//! here. Pure: no clock, no I/O; the caller passes the time.

use serde_json::{json, Map, Value};
use theseus_protocol::work::{DoingKind, QuestionKind, WorkState, WorkView};
use theseus_protocol::{usd, Level, SessionKind};

/// herdr's cap on a title, a display name, and a token's value.
pub const TEXT_CHARS: usize = 80;

/// The longest agent label herdr's toast and sidebar row carry whole.
pub const AGENT_CHARS: usize = 22;

/// herdr's cap on the tokens a pane reports.
pub const MAX_TOKENS: usize = 16;

/// The tokens a pane reports, in the order the sidebar can use them.
pub const TOKENS: [&str; 10] = [
    "task", "kind", "session", "confirm", "cost", "parent", "since", "doing", "model", "report",
];

/// How often a running call's elapsed time is said again: the one timer a
/// quiet pane keeps while a tool runs.
pub const TOOL_TICK_MS: u64 = 15_000;

/// How often a job's elapsed time is said again.
pub const JOB_TICK_MS: u64 = 60_000;

/// What the view does not carry, or carries in another shape.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub kind: Option<SessionKind>,
    pub session_id: String,
    /// The session's name, whole (`theseus_client::names`).
    pub name: String,
    pub model: Option<String>,
    /// The parent's name, when the session was opened under one.
    pub parent: Option<String>,
    /// When the session was created, for `since`.
    pub created_ms: u64,
    /// The call running now, as words, and since when.
    pub tool: Option<(String, u64)>,
    /// The first line of what a finished task said.
    pub report: Option<String>,
    /// The daemon is away: the label says so.
    pub reconnecting: bool,
    /// A message was just sent from this terminal.
    pub sending: bool,
}

/// One report's worth of slots.
#[derive(Debug, Clone, PartialEq)]
pub struct Slots {
    /// herdr's state: `blocked`, `working`, or `idle`.
    pub state: &'static str,
    /// The attention label, as the daemon words it.
    pub message: String,
    /// The label for `state`, worded once.
    pub label: String,
    /// The short name: herdr's `agent` and `display_agent`.
    pub agent: String,
    /// The whole title.
    pub title: String,
    /// Every token, `None` clearing it.
    pub tokens: Vec<(&'static str, Option<String>)>,
}

impl Slots {
    /// `state_labels`: the label for this state.
    pub fn state_labels(&self) -> Value {
        json!({ self.state: fit(&self.label) })
    }

    /// The tokens as `pane.report_metadata` carries them: a string sets one,
    /// null clears it.
    pub fn token_map(&self) -> Map<String, Value> {
        self.tokens
            .iter()
            .map(|(k, v)| ((*k).to_string(), json!(v.as_deref().map(fit))))
            .collect()
    }
}

/// herdr's state for a level: needs you is `blocked`; working is `working`;
/// ready and idle are `idle`. herdr makes `done` itself, of an idle pane not
/// yet seen, so a finished turn shows as done until looked at. A question, a
/// budget question, a failure and a block are all needs you
/// (`attention_of`), so all `blocked`.
pub fn state_of(level: Level) -> &'static str {
    match level {
        Level::NeedsYou => "blocked",
        Level::Working => "working",
        Level::Ready | Level::Idle => "idle",
    }
}

/// A text as herdr keeps it: one line, at most `TEXT_CHARS` characters.
pub fn fit(s: &str) -> String {
    let line = s.lines().next().unwrap_or_default().trim();
    if line.chars().count() <= TEXT_CHARS {
        return line.to_string();
    }
    let cut: String = line.chars().take(TEXT_CHARS - 1).collect();
    format!("{}…", cut.trim_end())
}

/// A name as the toast and the sidebar row say it: at most `AGENT_CHARS`
/// characters, cut at a word.
pub fn short_name(name: &str) -> String {
    let line: String = name
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    let line = line.trim();
    if line.chars().count() <= AGENT_CHARS {
        return line.to_string();
    }
    let head: Vec<char> = line.chars().take(AGENT_CHARS).collect();
    let boundary = line
        .chars()
        .nth(AGENT_CHARS)
        .is_some_and(char::is_whitespace);
    let cut: String = if boundary {
        head.into_iter().collect()
    } else {
        match head.iter().rposition(|c| c.is_whitespace()) {
            Some(i) if i > 0 => head[..i].iter().collect(),
            _ => head.into_iter().collect(),
        }
    };
    cut.trim_end_matches(|c: char| c.is_whitespace() || ",;:-–—".contains(c))
        .to_string()
}

/// A span as a label says it: `41 s`, `4 m`, `1 h 5 m`.
pub fn span(ms: u64) -> String {
    let s = ms / 1000;
    match s {
        0..=59 => format!("{s} s"),
        60..=3599 => format!("{} m", s / 60),
        _ if s % 3600 / 60 == 0 => format!("{} h", s / 3600),
        _ => format!("{} h {} m", s / 3600, s % 3600 / 60),
    }
}

/// A call in words, for the `doing` token and the working label: the tool,
/// then what it acts on: `proc.run cargo nextest`, `fs.write log.md`. A job's
/// argv is what it acts on; any other call's input is searched for a path, a
/// command, a query or an address.
pub fn call_words(tool: &str, argv: Option<&[String]>, input: Option<&Value>) -> String {
    let mut out = tool.to_string();
    let target = match argv {
        Some(a) if !a.is_empty() => Some(a.join(" ")),
        _ => input.and_then(|i| {
            let argv = i.get("argv").and_then(Value::as_array).map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" ")
            });
            argv.filter(|a| !a.is_empty()).or_else(|| {
                ["path", "file", "command", "query", "url", "name", "pattern"]
                    .iter()
                    .find_map(|k| i.get(k).and_then(Value::as_str))
                    .map(str::to_string)
            })
        }),
    };
    if let Some(t) = target {
        let t: String = t
            .lines()
            .next()
            .unwrap_or_default()
            .chars()
            .take(48)
            .collect();
        if !t.trim().is_empty() {
            out.push(' ');
            out.push_str(t.trim());
        }
    }
    out
}

/// The slots for a view (none: a session with no execution yet) and the
/// facts beside it, at `now_ms`.
pub fn slots(w: Option<&WorkView>, f: &Facts, now_ms: u64) -> Slots {
    let (state, message, mut label) = match (w, f.sending) {
        (_, true) => (
            "working",
            "sending your message".into(),
            "sending your message".into(),
        ),
        (None, false) => ("idle", "ready".to_string(), "ready".to_string()),
        (Some(w), false) => (
            state_of(w.attention.level),
            w.attention.label.clone(),
            label_of(w, f, now_ms),
        ),
    };
    let mut message = message;
    if f.reconnecting {
        label.push_str(" · reconnecting");
        message.push_str(" · reconnecting");
    }
    let name = if f.name.is_empty() {
        &f.session_id
    } else {
        &f.name
    };
    let done = w.is_some_and(|w| w.state == WorkState::Done);
    let report = done
        .then(|| {
            w.and_then(|w| w.report.as_ref())
                .and_then(|r| r.first_line.clone())
                .or_else(|| f.report.clone())
        })
        .flatten()
        .map(|r| first_line(&r))
        .filter(|r| !r.is_empty());
    let doing = f
        .tool
        .as_ref()
        .map(|(words, since)| format!("{words} · {}", span(now_ms.saturating_sub(*since))));
    let tokens = vec![
        ("task", Some(name.clone())),
        ("kind", f.kind.map(|k| kind_token(k).to_string())),
        ("session", Some(f.session_id.clone())),
        (
            "confirm",
            w.and_then(|w| w.question.as_ref())
                .filter(|_| w.is_some_and(|w| !w.state.ended()))
                .map(|q| q.id.clone()),
        ),
        (
            "cost",
            Some(usd(w.and_then(|w| w.cost).map_or(0.0, |c| c.spent_usd))),
        ),
        ("parent", f.parent.as_ref().map(|p| format!("↳ {p}"))),
        (
            "since",
            (f.created_ms > 0).then(|| theseus_client::render::time::fmt_hm(f.created_ms)),
        ),
        ("doing", doing),
        ("model", f.model.clone()),
        ("report", report),
    ];
    debug_assert!(tokens.len() <= MAX_TOKENS);
    debug_assert!(tokens.iter().map(|t| t.0).eq(TOKENS));
    Slots {
        state,
        message,
        label,
        agent: short_name(name),
        title: fit(name),
        tokens,
    }
}

/// The token `kind` marks a pane as ours by: `task` or `conversation`.
pub fn kind_token(k: SessionKind) -> &'static str {
    match k {
        SessionKind::Task => "task",
        SessionKind::Conversation => "conversation",
    }
}

fn first_line(s: &str) -> String {
    s.lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// The state label, worded once for every state. First match wins, as
/// `attention_of`'s table does.
fn label_of(w: &WorkView, f: &Facts, now_ms: u64) -> String {
    let said = |word: &str, rest: Option<&str>| match rest.filter(|r| !r.trim().is_empty()) {
        Some(r) => format!("{word} · {}", first_line(r)),
        None => word.to_string(),
    };
    let open = w.question.as_ref().filter(|_| !w.state.ended());
    if let Some(q) = open {
        if q.kind == QuestionKind::Budget {
            let (spent, limit) = w.cost.map_or((0.0, 0.0), |c| (c.spent_usd, c.limit_usd));
            return format!("budget · {} of {}", usd(spent), usd(limit));
        }
        let words = match q.tool.as_deref() {
            Some(t) if !t.is_empty() && !q.prompt.starts_with(t) => format!("{t} {}", q.prompt),
            _ => q.prompt.clone(),
        };
        return said("needs you", Some(&words));
    }
    let reason = w.report.as_ref().and_then(|r| r.reason.as_deref());
    match w.state {
        WorkState::Failed => said("failed", reason),
        WorkState::NeedsYou => match w.report.as_ref().map(|r| r.outcome.as_str()) {
            Some("budget_exhausted") => "budget · exhausted".into(),
            Some(_) => said("blocked", reason),
            None => "needs you".into(),
        },
        WorkState::Running => {
            let working = match &f.tool {
                Some((words, _)) => format!("{} · {words}", w.attention.label),
                None => w.attention.label.clone(),
            };
            // A paused task says so first, so a cut label keeps it.
            if w.paused {
                format!("paused · {working}")
            } else {
                working
            }
        }
        WorkState::Waiting => match w.doing.as_ref().map(|d| d.what) {
            Some(DoingKind::Job) if w.state_since_ms > 0 => {
                format!("job · {}", span(now_ms.saturating_sub(w.state_since_ms)))
            }
            Some(DoingKind::Job) => "job".into(),
            _ => w.attention.label.clone(),
        },
        WorkState::Ready => "ready".into(),
        WorkState::Done => said(
            "done",
            f.report
                .as_deref()
                .or(w.report.as_ref().and_then(|r| r.first_line.as_deref())),
        ),
        WorkState::Cancelled => "cancelled".into(),
        WorkState::Expired => "expired".into(),
        WorkState::Queued => w.attention.label.clone(),
        WorkState::Unknown => "working".into(),
    }
}

/// When the label next says a different elapsed time, if one is running:
/// the tick the pane wakes for. None when nothing is timed.
pub fn next_tick(w: Option<&WorkView>, f: &Facts, now_ms: u64) -> Option<u64> {
    let every = |since: u64, step: u64| {
        let n = now_ms.saturating_sub(since) / step + 1;
        since + n * step
    };
    if let Some((_, since)) = &f.tool {
        return Some(every(*since, TOOL_TICK_MS));
    }
    let w = w?;
    let job = w.state == WorkState::Waiting
        && w.doing.as_ref().is_some_and(|d| d.what == DoingKind::Job)
        && w.state_since_ms > 0;
    job.then(|| every(w.state_since_ms, JOB_TICK_MS))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The agent label: at most 22 characters, cut at a word.
    #[test]
    fn the_agent_label_is_cut_at_a_word() {
        assert_eq!(short_name("Write the harbour log"), "Write the harbour log");
        assert_eq!(
            short_name("Write the harbour log needs attention"),
            "Write the harbour log"
        );
        assert_eq!(short_name("Survey the north quay"), "Survey the north quay");
        assert_eq!(
            short_name("Survey the north quay again"),
            "Survey the north quay"
        );
        assert_eq!(
            short_name("Reconcile the tidewater ledger"),
            "Reconcile the"
        );
        assert_eq!(
            short_name("Supercalifragilisticexpialidocious"),
            "Supercalifragilisticex"
        );
        assert_eq!(short_name("a\nb"), "a");
    }

    fn running(label: &str) -> WorkView {
        let x: theseus_protocol::ExecutionView = serde_json::from_value(json!({
            "position": 5, "execution_id": "exe_q7f3k2", "session_id": "ses_q7f3k2",
            "kind": "task", "state": "running", "turns": 3, "spent_usd": 0.42,
            "attention": {"level": "working", "label": label, "since_ms": 0}}))
        .unwrap();
        WorkView::from_execution(&x)
    }

    /// A paused task's pane label says so first (theseus-ypnxt.2).
    #[test]
    fn a_paused_task_says_paused_in_its_label() {
        let mut w = running("turn 3");
        w.paused = true;
        let f = Facts {
            kind: Some(SessionKind::Task),
            session_id: "ses_q7f3k2".into(),
            ..Facts::default()
        };
        assert_eq!(slots(Some(&w), &f, 1_000).label, "paused · turn 3");
    }

    /// A running call is said as it runs and with its elapsed time, and the
    /// pane wakes for the next whole tick, never sooner.
    #[test]
    fn a_running_call_is_timed_and_ticks_on_the_quarter_minute() {
        let w = running("turn 3");
        let f = Facts {
            kind: Some(SessionKind::Task),
            session_id: "ses_q7f3k2".into(),
            name: "Write the harbour log needs attention".into(),
            tool: Some(("proc.run cargo test".into(), 1_000_000)),
            ..Facts::default()
        };
        let s = slots(Some(&w), &f, 1_041_000);
        assert_eq!(s.label, "turn 3 · proc.run cargo test");
        let doing = s.tokens.iter().find(|t| t.0 == "doing").unwrap();
        assert_eq!(doing.1.as_deref(), Some("proc.run cargo test · 41 s"));
        assert_eq!(s.agent, "Write the harbour log");
        assert_eq!(next_tick(Some(&w), &f, 1_041_000), Some(1_045_000));
        let idle = Facts { tool: None, ..f };
        assert_eq!(
            next_tick(Some(&w), &idle, 1_041_000),
            None,
            "nothing timed, no timer"
        );
    }

    #[test]
    fn spans_read_in_the_biggest_unit() {
        assert_eq!(span(41_000), "41 s");
        assert_eq!(span(4 * 60_000 + 5_000), "4 m");
        assert_eq!(span(3_600_000), "1 h");
        assert_eq!(span(3_900_000), "1 h 5 m");
    }
}
