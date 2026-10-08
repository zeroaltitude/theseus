//! The push (theseus-in3, design `stage2` §2): what every surface shows of an
//! execution, computed once by the server. `ExecutionView` is an execution as
//! a sidebar, a queue, or a waiting script needs it, and `attention()` maps it
//! to one of four levels and a label, so the web UI, the CLI, the cockpit, and
//! the TUI never disagree about what needs the operator.
//!
//! The server puts `attention` on the wire (`session.list`, `execution.list`,
//! `task.list`); a Rust client may call the same function on a view it holds.
//! Presentation (what a client has seen, focus, debouncing, sound) stays in
//! each client.

use serde::{Deserialize, Serialize};

use crate::{ConfirmRequest, SessionKind};

/// How much an execution needs people, the most first: a question or a
/// failure (`needs_you`), work under way (`working`), a conversation between
/// exchanges (`ready`), or nothing more to do (`idle`). A client's own "done
/// until seen" sits between `needs_you` and `working` in its order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Level {
    NeedsYou,
    Working,
    Ready,
    Idle,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::NeedsYou => "needs_you",
            Level::Working => "working",
            Level::Ready => "ready",
            Level::Idle => "idle",
        }
    }

    /// Its place in a queue or a rollup: needs you 0, working 2, ready 3,
    /// idle 4. A client's "done until seen" is 1.
    pub fn rank(self) -> u8 {
        match self {
            Level::NeedsYou => 0,
            Level::Working => 2,
            Level::Ready => 3,
            Level::Idle => 4,
        }
    }
}

/// What an execution needs from people, and since when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Attention {
    pub level: Level,
    /// For people: `confirm proc.run: run cargo test`, `turn 4`, `ready`.
    pub label: String,
    /// When the level last changed: within needs you, the longest waiting
    /// comes first.
    pub since_ms: u64,
}

/// What a waiting execution waits on: the kernel's wake, typed (spec §3.15).
/// The tag is `on`, as the kernel stores it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "on", rename_all = "snake_case")]
pub enum WaitingOn {
    /// A due time: a retry's backoff, a task sleeping until then.
    DueAt { at_ms: u64 },
    /// Dispatched calls (a job); any completion wakes it.
    Actions { correlation_ids: Vec<String> },
    /// Another execution reaching its end (a child task).
    Execution { execution_id: String },
    /// The operator's answer to a tool call.
    Confirm { confirm_id: String },
    /// Input on the session: a conversation between exchanges.
    Input,
    /// The operator's answer to a budget question.
    Budget { correlation_id: String },
    /// A wake this build does not know: a newer daemon's.
    #[serde(other)]
    Unknown,
}

/// A question waiting for the operator, in brief: the whole question, with its
/// input, is `confirm.requested`'s or `confirm.list`'s.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PendingConfirm {
    pub correlation_id: String,
    /// The tool that asks (`proc.run`), or `budget.reset`.
    pub tool: String,
    /// Why policy asks (`run cargo test`); empty when the server could not
    /// read it.
    #[serde(default)]
    pub reason: String,
    /// The floor asked: the call touches Theseus's own binary or state.
    #[serde(default)]
    pub floor: bool,
    /// A budget question: the session reached its spend limit.
    #[serde(default)]
    pub budget: bool,
    /// When the question stops holding; 0 for a budget question.
    #[serde(default)]
    pub expires_at_ms: u64,
}

/// One execution as every surface shows it: `execution.changed`'s params, and
/// a row of `executions.watch`'s snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ExecutionView {
    /// The WAL position of the last record of the frame this view comes from:
    /// it only grows, per execution. A client applies a view only if its
    /// position is greater than the last it applied.
    #[serde(default)]
    pub position: u64,
    /// When that frame was committed.
    #[serde(default)]
    pub at_ms: u64,
    pub execution_id: String,
    pub session_id: String,
    pub kind: SessionKind,
    /// A task's parent session: the tree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub parent_session_id: Option<String>,
    /// `queued`, `running`, `waiting`, `blocked`, `cancelled`, `failed`,
    /// `budget_exhausted`, or `complete`; a newer daemon may add one.
    pub state: String,
    /// The state before this frame; absent for an execution first seen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub previous: Option<String>,
    /// What it waits on, while it waits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub waiting_on: Option<WaitingOn>,
    /// Its questions for the operator: a budget question first.
    #[serde(default)]
    pub pending: Vec<PendingConfirm>,
    /// Turns taken.
    #[serde(default)]
    pub turns: u64,
    /// Its calls dispatched and not settled: a conversation waiting on input
    /// with one is still working (a job its turn left running).
    #[serde(default)]
    pub outstanding: u32,
    /// Its spend since the last reset, and its limit, in US dollars.
    #[serde(default)]
    pub spent_usd: f64,
    #[serde(default)]
    pub limit_usd: f64,
    /// Why it ended, failed, or is blocked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub ended_reason: Option<String>,
    /// Why it was queued, when the frame says (`wake`, `report`, `task`,
    /// `input`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub why: Option<String>,
    /// Its soonest pending wake (`wake.at`), while it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub wake_at_ms: Option<u64>,
    pub attention: Attention,
}

/// `executions.watch`: its snapshot's size. It replaces any earlier watch on
/// the same connection.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ExecutionsWatchParams {
    /// How many of the executions that need no one and do nothing (ready or
    /// idle) the snapshot carries, the most recently active first: 200 by
    /// default. Every execution that needs you or works is always there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub limit: Option<u32>,
}

/// `executions.watch`'s answer: the board as it stood when the watch began.
/// The subscription comes first, so no change falls between the two: apply a
/// snapshot row, as an event, only if its `position` is greater than the
/// last one applied for its execution.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ExecutionsWatchResult {
    /// The board's position when the snapshot was taken: the last frame it
    /// applied.
    pub position: u64,
    /// Every execution that needs you or works, then the `limit` most
    /// recently active of the rest.
    pub executions: Vec<ExecutionView>,
    /// Every question waiting for the operator, as `confirm.list` gives them.
    pub confirms: Vec<ConfirmRequest>,
    /// How many executions the board holds.
    pub total: u64,
}

/// `session.list`: every session, or only these (theseus-in3): a client that
/// meets a new session in `execution.changed` asks for its title.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SessionListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub ids: Option<Vec<String>>,
    /// The newest `n` sessions by when each was opened, newest first, in
    /// place of every session (theseus-96w2): a page read through the
    /// store's index, which costs the page, not every session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub n: Option<usize>,
    /// With `n`: only sessions opened before this cursor, an answer's
    /// `older`, to page back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub before: Option<u64>,
    /// Only sessions in this state (theseus-emqx). Absent: every session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub state: Option<crate::SessionState>,
}

/// What `session.wait` waits for (design `stage2` §2.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum WaitUntil {
    /// The session needs someone: a question, a block, a failure.
    Blocked,
    /// Nothing runs or is queued for it: it needs you, is ready, or is idle.
    /// A job, a child task, or a due time still counts as working.
    Settled,
    /// It ended. A conversation never does, so this is refused for one.
    Terminal,
}

impl WaitUntil {
    pub fn as_str(self) -> &'static str {
        match self {
            WaitUntil::Blocked => "blocked",
            WaitUntil::Settled => "settled",
            WaitUntil::Terminal => "terminal",
        }
    }

    /// Whether `view` is what this waits for.
    pub fn reached_by(self, view: &ExecutionView) -> bool {
        match self {
            WaitUntil::Blocked => view.attention.level == Level::NeedsYou,
            WaitUntil::Settled => view.attention.level != Level::Working,
            WaitUntil::Terminal => matches!(
                view.state.as_str(),
                "complete" | "cancelled" | "failed" | "budget_exhausted"
            ),
        }
    }
}

/// `session.wait`: until the session's execution is `until`. Owned by the
/// daemon: a parked task that costs nothing while it waits, and ends with its
/// connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SessionWaitParams {
    pub session_id: String,
    pub until: WaitUntil,
    /// Only a view after this position counts; without it the current view
    /// does, and a wait already satisfied answers at once (`already`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub after_position: Option<u64>,
    /// How long to wait: 10 minutes by default, 24 hours at most.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub timeout_ms: Option<u64>,
}

/// `session.wait`'s answer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SessionWaitResult {
    /// `blocked`, `settled`, `terminal`, or `timeout`.
    pub reached: String,
    /// The wait was satisfied when it began: it never parked.
    pub already: bool,
    /// The session's execution as the wait ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub execution: Option<ExecutionView>,
    /// The session's questions for the operator, whole.
    #[serde(default)]
    pub confirms: Vec<ConfirmRequest>,
}

/// `events.lost` (design `stage2` §2.5): a connection's queue passed the
/// backlog cap, so its notifications were dropped from then until it drained.
/// Responses are never dropped. Re-read each stream: `executions` with
/// `executions.watch`, `session:<id>` with `session.history` then
/// `session.watch`, `narrative` with `narrative.watch`, `policy` with
/// `health`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct EventsLost {
    /// How many notifications were dropped.
    pub dropped: u64,
    /// The streams they belonged to.
    pub streams: Vec<String>,
}

/// The push for health (theseus-in3): `push: 3 watchers · board 212 · 1
/// question · seeded in 38 ms`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PushStatus {
    /// The board was seeded: something watched since the daemon started. A
    /// daemon nothing watches never pays for it.
    pub seeded: bool,
    /// How long the seed took, in microseconds.
    #[serde(default)]
    pub seed_us: u64,
    /// The executions the board holds.
    #[serde(default)]
    pub board: u64,
    /// The questions waiting for the operator, as the board holds them.
    #[serde(default)]
    pub questions: u64,
    /// Connections with an `executions.watch`.
    #[serde(default)]
    pub watchers: u64,
    /// `execution.changed` notifications made since the daemon started.
    #[serde(default)]
    pub events: u64,
    /// `session.wait` calls parked now.
    #[serde(default)]
    pub waiting: u64,
    /// Notifications dropped at a connection's backlog cap since the daemon
    /// started, every stream's.
    #[serde(default)]
    pub lost: u64,
    /// The board's position: the last frame it applied.
    #[serde(default)]
    pub position: u64,
}

/// The longest reason a label carries before it is cut with `…`.
const LABEL_REASON_CHARS: usize = 60;

/// What an execution needs from people (design `stage2` §2.2). Pure: every
/// surface gets the same answer for the same view. The first rule that
/// matches wins:
///
/// | # | When | Level | Label |
/// |---|---|---|---|
/// | 1 | a budget question is pending | needs you | `budget: $10.02 of $10` |
/// | 2 | any other question is pending | needs you | `confirm proc.run: run cargo test` |
/// | 3 | `blocked` | needs you | `blocked: <reason>` |
/// | 4 | `failed` | needs you | `failed: <reason>` |
/// | 5 | `budget_exhausted` | needs you | `budget exhausted` |
/// | 6 | `running` | working | `turn 4` |
/// | 7 | `queued` | working | `queued`, `queued · wake` |
/// | 8 | waiting on calls, or on input with calls still dispatched (a job its turn left running) | working | `waiting on 2 calls` |
/// | 9 | waiting on another execution | working | `waiting on task a1b2c3` |
/// | 10 | waiting on a due time | working | `sleeping until 14:00` |
/// | 11 | waiting on a question with none pending (a race) | needs you | `waiting on you` |
/// | 12 | waiting on input | ready | `ready`, `ready · wake 14:00` |
/// | 13 | `complete`, `cancelled` | idle | `complete`, `cancelled` |
/// | 14 | a state or wake this build doesn't know | working | its name |
///
/// A sleeping task is working, never ready, so its wake is no "finished".
/// `hm` writes a time of day (`14:00`) as the reader's clock reads it: the
/// server passes its own time zone's, and `utc_hm` is UTC's. `since_ms` is
/// the view's `at_ms`; the server's board keeps the time the level last
/// changed instead.
pub fn attention(view: &ExecutionView, hm: &dyn Fn(u64) -> String) -> Attention {
    let (level, label) = level_and_label(view, hm);
    Attention {
        level,
        label,
        since_ms: view.at_ms,
    }
}

fn level_and_label(view: &ExecutionView, hm: &dyn Fn(u64) -> String) -> (Level, String) {
    if view.pending.iter().any(|p| p.budget) {
        return (
            Level::NeedsYou,
            format!("budget: {} of {}", usd(view.spent_usd), usd(view.limit_usd)),
        );
    }
    if let Some(first) = view.pending.first() {
        return (Level::NeedsYou, confirm_label(first, view.pending.len()));
    }
    let reason = |word: &str| match view.ended_reason.as_deref() {
        Some(r) if !r.is_empty() => format!("{word}: {}", clip(r)),
        _ => word.to_string(),
    };
    match view.state.as_str() {
        "blocked" => (Level::NeedsYou, reason("blocked")),
        "failed" => (Level::NeedsYou, reason("failed")),
        "budget_exhausted" => (Level::NeedsYou, "budget exhausted".into()),
        "running" => (Level::Working, format!("turn {}", view.turns.max(1))),
        "queued" => (
            Level::Working,
            match view.why.as_deref() {
                Some(w) if !w.is_empty() && w != "input" => format!("queued · {w}"),
                _ => "queued".into(),
            },
        ),
        "waiting" => waiting(view, hm),
        "complete" | "cancelled" => (Level::Idle, view.state.clone()),
        other => (Level::Working, other.to_string()),
    }
}

fn waiting(view: &ExecutionView, hm: &dyn Fn(u64) -> String) -> (Level, String) {
    match &view.waiting_on {
        Some(WaitingOn::Actions { correlation_ids }) => {
            (Level::Working, calls(correlation_ids.len()))
        }
        Some(WaitingOn::Execution { execution_id }) => (
            Level::Working,
            format!("waiting on task {}", tail(execution_id)),
        ),
        Some(WaitingOn::DueAt { at_ms }) => {
            (Level::Working, format!("sleeping until {}", hm(*at_ms)))
        }
        Some(WaitingOn::Confirm { .. } | WaitingOn::Budget { .. }) => {
            (Level::NeedsYou, "waiting on you".into())
        }
        // A job the turn left running (answered `background`): still working.
        Some(WaitingOn::Input) if view.outstanding > 0 => {
            (Level::Working, calls(view.outstanding as usize))
        }
        Some(WaitingOn::Input) => (
            Level::Ready,
            match view.wake_at_ms {
                Some(at) => format!("ready · wake {}", hm(at)),
                None => "ready".into(),
            },
        ),
        Some(WaitingOn::Unknown) | None => (Level::Working, "waiting".into()),
    }
}

/// `waiting on 2 calls`.
fn calls(n: usize) -> String {
    format!("waiting on {n} call{}", if n == 1 { "" } else { "s" })
}

fn confirm_label(first: &PendingConfirm, count: usize) -> String {
    let mut label = if first.reason.is_empty() {
        format!("confirm {}", first.tool)
    } else {
        format!("confirm {}: {}", first.tool, clip(&first.reason))
    };
    if first.floor {
        label.push_str(" · floor");
    }
    if count > 1 {
        label.push_str(&format!(" · +{} more", count - 1));
    }
    label
}

/// The end of an id, as people name it (`/cancel a1b2c3`).
fn tail(id: &str) -> &str {
    let n = id.chars().count();
    match id.char_indices().nth(n.saturating_sub(6)) {
        Some((i, _)) => &id[i..],
        None => id,
    }
}

/// One line of a reason, cut at `LABEL_REASON_CHARS`.
fn clip(s: &str) -> String {
    let line = s.lines().next().unwrap_or_default().trim();
    if line.chars().count() <= LABEL_REASON_CHARS {
        return line.to_string();
    }
    let cut: String = line.chars().take(LABEL_REASON_CHARS - 1).collect();
    format!("{}…", cut.trim_end())
}

/// Dollars as a label says them: `$10`, `$10.02`, and a fraction of a cent
/// to four places (`$0.0042`).
pub fn usd(x: f64) -> String {
    if x > 0.0 && x < 0.01 {
        return format!("${x:.4}");
    }
    let cents = (x * 100.0).round();
    if cents % 100.0 == 0.0 {
        format!("${:.0}", cents / 100.0)
    } else {
        format!("${:.2}", cents / 100.0)
    }
}

/// A time of day in UTC, `14:00`: for a reader with no time zone of its own.
pub fn utc_hm(unix_ms: u64) -> String {
    let s = unix_ms / 1000 % 86_400;
    format!("{:02}:{:02}", s / 3600, s / 60 % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A view of `state` waiting on `wake`, with `questions` pending.
    fn view(state: &str, wake: Option<WaitingOn>, questions: Vec<PendingConfirm>) -> ExecutionView {
        ExecutionView {
            position: 7,
            at_ms: 1_790_000_000_000,
            execution_id: "exe_0000a1b2c3".into(),
            session_id: "ses_0000d4e5f6".into(),
            kind: SessionKind::Conversation,
            parent_session_id: None,
            state: state.into(),
            previous: None,
            waiting_on: wake,
            pending: questions,
            turns: 4,
            outstanding: 0,
            spent_usd: 10.02,
            limit_usd: 10.0,
            ended_reason: None,
            why: None,
            wake_at_ms: None,
            attention: Attention {
                level: Level::Idle,
                label: String::new(),
                since_ms: 0,
            },
        }
    }

    fn ask(tool: &str, reason: &str) -> PendingConfirm {
        PendingConfirm {
            correlation_id: format!("cor_{tool}"),
            tool: tool.into(),
            reason: reason.into(),
            floor: false,
            budget: false,
            expires_at_ms: 1_790_000_300_000,
        }
    }

    fn budget_ask() -> PendingConfirm {
        PendingConfirm {
            budget: true,
            expires_at_ms: 0,
            ..ask("budget.reset", "This session reached its spend limit")
        }
    }

    const STATES: [&str; 8] = [
        "queued",
        "running",
        "waiting",
        "blocked",
        "cancelled",
        "failed",
        "budget_exhausted",
        "complete",
    ];

    fn wakes() -> Vec<Option<WaitingOn>> {
        vec![
            None,
            Some(WaitingOn::DueAt {
                at_ms: 1_790_000_000_000 + 3_600_000,
            }),
            Some(WaitingOn::Actions {
                correlation_ids: vec!["cor_1".into(), "cor_2".into()],
            }),
            Some(WaitingOn::Execution {
                execution_id: "exe_0000c4d5e6".into(),
            }),
            Some(WaitingOn::Confirm {
                confirm_id: "cor_proc.run".into(),
            }),
            Some(WaitingOn::Input),
            Some(WaitingOn::Budget {
                correlation_id: "cor_budget.reset".into(),
            }),
            Some(WaitingOn::Unknown),
        ]
    }

    /// What the table in `attention`'s docs says for one view, written out
    /// a second way: by state first, then by wake.
    fn expected(state: &str, wake: &Option<WaitingOn>, questions: &[PendingConfirm]) -> Level {
        if !questions.is_empty() {
            return Level::NeedsYou;
        }
        match (state, wake) {
            ("blocked" | "failed" | "budget_exhausted", _) => Level::NeedsYou,
            ("running" | "queued", _) => Level::Working,
            ("complete" | "cancelled", _) => Level::Idle,
            ("waiting", Some(WaitingOn::Input)) => Level::Ready,
            ("waiting", Some(WaitingOn::Confirm { .. } | WaitingOn::Budget { .. })) => {
                Level::NeedsYou
            }
            _ => Level::Working,
        }
    }

    /// Every state × every wake × {no question, one, a budget question}.
    #[test]
    fn every_state_and_wake_with_and_without_a_question_has_its_level() {
        let questions = [
            vec![],
            vec![ask("proc.run", "run cargo test")],
            vec![budget_ask()],
        ];
        let mut seen = 0;
        for state in STATES {
            for wake in wakes() {
                for q in &questions {
                    let v = view(state, wake.clone(), q.clone());
                    let a = attention(&v, &utc_hm);
                    assert_eq!(
                        a.level,
                        expected(state, &wake, q),
                        "{state} on {wake:?} with {q:?}: {a:?}"
                    );
                    assert!(!a.label.is_empty(), "{state} on {wake:?}: a label");
                    assert_eq!(a.since_ms, v.at_ms);
                    seen += 1;
                }
            }
        }
        assert_eq!(seen, 8 * 8 * 3);
    }

    /// Each label, as the design's table writes it.
    #[test]
    fn each_rule_has_its_label() {
        let label = |v: ExecutionView| attention(&v, &utc_hm).label;
        let waiting = |w: WaitingOn| view("waiting", Some(w), vec![]);
        // 1, 2: questions; a budget question wins over a tool call.
        assert_eq!(
            label(view(
                "waiting",
                None,
                vec![ask("proc.run", "x"), budget_ask()]
            )),
            "budget: $10.02 of $10"
        );
        assert_eq!(
            label(view(
                "waiting",
                Some(WaitingOn::Confirm {
                    confirm_id: "c".into()
                }),
                vec![ask("proc.run", "run cargo test")]
            )),
            "confirm proc.run: run cargo test"
        );
        let mut floor = ask("fs.write", "write under ~/.theseus");
        floor.floor = true;
        assert_eq!(
            label(view("waiting", None, vec![floor, ask("proc.run", "")])),
            "confirm fs.write: write under ~/.theseus · floor · +1 more"
        );
        assert_eq!(
            label(view("running", None, vec![ask("proc.run", "")])),
            "confirm proc.run",
            "a question wins over the state; no reason, no colon"
        );
        // 3, 4, 5: ends that need someone.
        let mut v = view("blocked", None, vec![]);
        v.ended_reason = Some("the provider refused the key\nmore".into());
        assert_eq!(label(v), "blocked: the provider refused the key");
        let mut v = view("failed", None, vec![]);
        v.ended_reason = Some("x".repeat(80));
        let l = label(v);
        assert!(l.starts_with("failed: xxx") && l.ends_with('…'), "{l}");
        assert_eq!(l.chars().count(), "failed: ".len() + LABEL_REASON_CHARS);
        assert_eq!(label(view("failed", None, vec![])), "failed");
        assert_eq!(
            label(view("budget_exhausted", None, vec![])),
            "budget exhausted"
        );
        // 6, 7: work under way.
        assert_eq!(label(view("running", None, vec![])), "turn 4");
        assert_eq!(label(view("queued", None, vec![])), "queued");
        let mut v = view("queued", None, vec![]);
        v.why = Some("report".into());
        assert_eq!(label(v), "queued · report");
        let mut v = view("queued", None, vec![]);
        v.why = Some("input".into());
        assert_eq!(label(v), "queued", "input is what queues it anyway");
        // 8, 9, 10: waiting on work.
        assert_eq!(
            label(waiting(WaitingOn::Actions {
                correlation_ids: vec!["a".into()]
            })),
            "waiting on 1 call"
        );
        assert_eq!(
            label(waiting(WaitingOn::Actions {
                correlation_ids: vec!["a".into(), "b".into()]
            })),
            "waiting on 2 calls"
        );
        assert_eq!(
            label(waiting(WaitingOn::Execution {
                execution_id: "exe_0000c4d5e6".into()
            })),
            "waiting on task c4d5e6"
        );
        // 1_789_999_200_000 is 14:00 UTC.
        let sleeping = waiting(WaitingOn::DueAt {
            at_ms: 1_789_999_200_000,
        });
        assert_eq!(label(sleeping.clone()), "sleeping until 14:00");
        assert_eq!(
            attention(&sleeping, &|_| "2:00 PM".into()).label,
            "sleeping until 2:00 PM",
            "the reader's clock writes the time"
        );
        // 11: a question the race already took.
        assert_eq!(
            label(waiting(WaitingOn::Budget {
                correlation_id: "q".into()
            })),
            "waiting on you"
        );
        // 8 again: a job the turn left running keeps it working.
        let mut v = waiting(WaitingOn::Input);
        v.outstanding = 1;
        let a = attention(&v, &utc_hm);
        assert_eq!(
            (a.level, a.label.as_str()),
            (Level::Working, "waiting on 1 call")
        );
        // 12: between exchanges.
        assert_eq!(label(waiting(WaitingOn::Input)), "ready");
        let mut v = waiting(WaitingOn::Input);
        v.wake_at_ms = Some(1_789_999_200_000);
        assert_eq!(label(v), "ready · wake 14:00");
        // 13: nothing more to do.
        assert_eq!(label(view("complete", None, vec![])), "complete");
        assert_eq!(label(view("cancelled", None, vec![])), "cancelled");
    }

    /// A state or a wake from a newer daemon reads as work under way, by its
    /// name: never as ready, which would fire a "finished".
    #[test]
    fn a_state_or_wake_this_build_does_not_know_reads_as_working() {
        let a = attention(&view("paused_for_jev", None, vec![]), &utc_hm);
        assert_eq!(
            (a.level, a.label.as_str()),
            (Level::Working, "paused_for_jev")
        );
        let w: WaitingOn = serde_json::from_str(r#"{"on":"jev_recovery","attempt":2}"#).unwrap();
        assert_eq!(w, WaitingOn::Unknown);
        let a = attention(&view("waiting", Some(w), vec![]), &utc_hm);
        assert_eq!((a.level, a.label.as_str()), (Level::Working, "waiting"));
    }

    /// The kernel's wake decodes as the typed one, and a view round-trips.
    #[test]
    fn the_kernels_wake_decodes_typed_and_a_view_round_trips() {
        for (json, typed) in [
            (r#"{"on":"input"}"#, WaitingOn::Input),
            (
                r#"{"on":"due_at","at_ms":5}"#,
                WaitingOn::DueAt { at_ms: 5 },
            ),
            (
                r#"{"on":"actions","correlation_ids":["c"]}"#,
                WaitingOn::Actions {
                    correlation_ids: vec!["c".into()],
                },
            ),
            (
                r#"{"on":"budget","correlation_id":"q"}"#,
                WaitingOn::Budget {
                    correlation_id: "q".into(),
                },
            ),
        ] {
            assert_eq!(serde_json::from_str::<WaitingOn>(json).unwrap(), typed);
            assert_eq!(serde_json::to_string(&typed).unwrap(), json);
        }
        let mut v = view(
            "waiting",
            Some(WaitingOn::Input),
            vec![ask("proc.run", "r")],
        );
        v.attention = attention(&v, &utc_hm);
        let back: ExecutionView =
            serde_json::from_value(serde_json::to_value(&v).unwrap()).unwrap();
        assert_eq!(back, v);
        let wire = serde_json::to_value(&v).unwrap();
        assert!(wire.get("previous").is_none(), "absent, not null: {wire}");
        assert_eq!(wire["attention"]["level"], "needs_you");
    }

    #[test]
    fn dollars_read_as_people_write_them() {
        assert_eq!(usd(10.0), "$10");
        assert_eq!(usd(10.02), "$10.02");
        assert_eq!(usd(0.0042), "$0.0042");
        assert_eq!(usd(0.0), "$0");
        assert_eq!(usd(99.999), "$100");
        assert_eq!(utc_hm(1_789_999_200_000), "14:00");
    }

    #[test]
    fn levels_order_a_queue() {
        let mut v = [Level::Idle, Level::Ready, Level::NeedsYou, Level::Working];
        v.sort_by_key(|l| l.rank());
        assert_eq!(
            v,
            [Level::NeedsYou, Level::Working, Level::Ready, Level::Idle]
        );
        assert_eq!(
            serde_json::to_string(&Level::NeedsYou).unwrap(),
            r#""needs_you""#
        );
    }
}
