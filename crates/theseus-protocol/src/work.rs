//! The work view (theseus-753z, the substrate design): one task model for
//! every surface. Conversations, tasks, plan steps, jobs, questions, and wakes
//! are each a `WorkView`, in one tree (`parent`, `root`), with the same four
//! attention levels and labels for every kind (`attention_of`), and a
//! question's answers worded once (`answer_options`). Types and pure
//! functions only, like `push.rs`: no clock, no I/O.
//!
//! The daemon's work board (a later row) fills these from frames. Until it
//! does, a reader builds a conversation's or a task's view from today's push
//! (`WorkView::from_execution`), so every surface can adopt the notification
//! policy (`crate::notices`) now. Code says `work`; every surface says
//! "tasks".

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{usd, Attention, ExecutionView, Level, PendingConfirm, SessionKind, WaitingOn};

/// What a piece of work is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum WorkKind {
    /// A session the owner talks in: a root.
    Conversation,
    /// A task session, under the conversation or task that opened it.
    Task,
    /// A step of a task's plan.
    Step,
    /// A dispatched call that runs on its own (`proc.run`, a build).
    Job,
    /// A question for the owner.
    Question,
    /// A wake: a reminder or a sleeping task's due time.
    Wake,
    /// The daemon's own work, under the `system` root.
    System,
}

impl WorkKind {
    pub fn as_str(self) -> &'static str {
        match self {
            WorkKind::Conversation => "conversation",
            WorkKind::Task => "task",
            WorkKind::Step => "step",
            WorkKind::Job => "job",
            WorkKind::Question => "question",
            WorkKind::Wake => "wake",
            WorkKind::System => "system",
        }
    }
}

/// Where a piece of work stands. A newer daemon's state reads as `unknown`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum WorkState {
    Queued,
    Running,
    /// On its calls, a task, or a time.
    Waiting,
    /// On a question, or blocked.
    NeedsYou,
    /// A conversation between exchanges.
    Ready,
    Done,
    Failed,
    Cancelled,
    /// A question no one answered in time.
    Expired,
    #[serde(other)]
    Unknown,
}

impl WorkState {
    pub fn as_str(self) -> &'static str {
        match self {
            WorkState::Queued => "queued",
            WorkState::Running => "running",
            WorkState::Waiting => "waiting",
            WorkState::NeedsYou => "needs_you",
            WorkState::Ready => "ready",
            WorkState::Done => "done",
            WorkState::Failed => "failed",
            WorkState::Cancelled => "cancelled",
            WorkState::Expired => "expired",
            WorkState::Unknown => "unknown",
        }
    }

    /// Whether the work is under way: queued, running, or waiting on work.
    pub fn working(self) -> bool {
        matches!(
            self,
            WorkState::Queued | WorkState::Running | WorkState::Waiting
        )
    }

    /// Whether the work has ended.
    pub fn ended(self) -> bool {
        matches!(
            self,
            WorkState::Done | WorkState::Failed | WorkState::Cancelled | WorkState::Expired
        )
    }
}

/// One piece of work as every surface shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WorkView {
    /// `exe_…` (a conversation, a task), `act_…` (a job, a question),
    /// `wak_…` (a wake), `tsk_…` (a step).
    pub id: String,
    pub kind: WorkKind,
    /// The WAL position of the frame this view comes from, and when it was
    /// committed: a client applies a view only if its position is greater
    /// than the last it applied for the same id (the position rule).
    pub position: u64,
    pub at_ms: u64,
    /// What it is under: the tree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub parent: Option<String>,
    /// The conversation at the top of its tree, or `system`.
    pub root: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_id: Option<String>,
    /// The session's or the task's title, a job's command (`cargo test -p
    /// core`), a wake's note, or the question. Empty while unknown.
    pub title: String,
    /// The place rule's name for where it lives: `cli`, `web`, `discord dm`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub place: Option<String>,
    pub state: WorkState,
    /// The state before this frame; absent for work first seen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub previous: Option<WorkState>,
    /// The same four levels and labels for every kind.
    pub attention: Attention,
    /// When it was opened, dispatched, asked, or set; 0 when the source does
    /// not say.
    pub started_at_ms: u64,
    pub state_since_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub ended_at_ms: Option<u64>,
    /// A conversation's or a task's doing now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub doing: Option<Doing>,
    /// Plan steps, and the model's last word.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub progress: Option<Progress>,
    /// A question's whole self; on an execution, its open one in brief.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub question: Option<Question>,
    /// Spend and limit since the last reset, and this turn's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub cost: Option<Cost>,
    /// A job: the usual time of the same program on this machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub eta: Option<Eta>,
    /// Its subtree, counted by level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub below: Option<Rollup>,
    /// Why it was queued (`wake`, `report`, `task`, `input`), kept while it
    /// is queued.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub why: Option<String>,
    /// A task's ending: its report, so a terminal hears it at once rather
    /// than at the parent's next turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub report: Option<WorkReport>,
    /// The wake whose turn just ended: a client says `⏰ ops · the tide
    /// turns` even when the level did not change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub woke: Option<Woke>,
    /// The owner asked to hear when it is done ("tell me when it's done").
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flagged: bool,
}

/// What a conversation or a task is doing now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum DoingKind {
    Thinking,
    Tool,
    Job,
    Asking,
    WaitingOnTasks,
    Sleeping,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Doing {
    pub what: DoingKind,
    /// `turn 4`, `fs.write`, `waiting on 2 calls`.
    pub label: String,
    pub since_ms: u64,
    pub turn: u64,
    pub loop_index: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Progress {
    pub steps_done: u32,
    pub steps_total: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub current: Option<String>,
    /// The model's last word.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub said: Option<String>,
    pub at_ms: u64,
}

/// What a question asks about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum QuestionKind {
    /// A tool call waits for the owner's approval.
    Approval,
    /// The session reached its spend limit.
    Budget,
    /// A layer-1 task change.
    Change,
    /// An extension asks.
    Extension,
    /// The model asks the owner, with its own options.
    Ask,
}

/// A reference to a piece of work, as a notice or a question names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WorkRef {
    pub id: String,
    pub kind: WorkKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_id: Option<String>,
    /// Empty while unknown.
    #[serde(default)]
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Question {
    /// The correlation id an answer names.
    pub id: String,
    pub kind: QuestionKind,
    pub asked_by: WorkRef,
    /// `fs.write create projects/beta-summary.txt (40 B)`.
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "unknown"))]
    pub input: Option<Value>,
    pub options: Vec<AnswerOption>,
    /// What happens if no one answers: `declined`, `not run`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub on_expiry: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub expires_at_ms: Option<u64>,
    /// The floor asks: the call touches Theseus's own binary or state.
    pub floor: bool,
    /// This connection may answer it (the place rule).
    pub may_answer: bool,
    /// The tool that asks (`fs.write`, `budget.reset`), when one does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub tool: Option<String>,
    /// When it was asked; on an execution's brief, when the execution came
    /// to need you.
    #[serde(default)]
    pub asked_at_ms: u64,
    /// The call waits because its session read external text: an approval
    /// may trust the session again (`action.confirm`'s `trust`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub external_text: bool,
    /// An `ask`'s own choices, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
}

impl Question {
    /// How people name it: `Beta summary's fs.write`.
    pub fn subject(&self, who: &str) -> String {
        match self.tool.as_deref() {
            Some(t) if !t.is_empty() => format!("{who}'s {t}"),
            _ => format!("{who}'s question"),
        }
    }
}

/// How an answer looks on a button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Style {
    Primary,
    Plain,
    Danger,
}

/// One answer to a question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AnswerOption {
    /// `approve`, `approve_trust`, `decline`, `reset`, `accept`, `ack`, or
    /// `choice:N`.
    pub id: String,
    pub label: String,
    pub style: Style,
    /// The answer takes a note in words.
    pub takes_note: bool,
    /// What it does, in a phrase.
    pub effect: String,
}

/// A subtree counted by level.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Rollup {
    pub needs_you: u32,
    pub working: u32,
    pub done: u32,
    pub failed: u32,
    pub total: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Eta {
    pub usual_ms: u64,
    pub runs: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Cost {
    pub spent_usd: f64,
    pub limit_usd: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub turn_usd: Option<f64>,
}

/// A task's report, on its ending view: `{"outcome": "failed", "reason":
/// "the provider refused the request (400)"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WorkReport {
    /// `done`, `failed`, `cancelled`; a question's `approved`, `declined`.
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub first_line: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub reason: Option<String>,
}

/// The wake whose turn just ended: its note and when it fired.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Woke {
    pub note: String,
    pub at_ms: u64,
}

fn option(id: &str, label: &str, style: Style, takes_note: bool, effect: &str) -> AnswerOption {
    AnswerOption {
        id: id.into(),
        label: label.into(),
        style,
        takes_note,
        effect: effect.into(),
    }
}

/// A question's answers, worded once for every surface: an approval gives
/// approve, approve and trust (when its session read external text), and
/// decline with a note, and the floor marks approve as dangerous; a budget
/// question gives reset and decline; a task change accept and decline; an
/// extension ack and decline; an `ask` its choices, then something else.
pub fn answer_options(q: &Question) -> Vec<AnswerOption> {
    let decline = |effect: &str| option("decline", "Decline", Style::Plain, true, effect);
    match q.kind {
        QuestionKind::Approval => {
            let style = if q.floor {
                Style::Danger
            } else {
                Style::Primary
            };
            let mut out = vec![option("approve", "Approve", style, false, "the call runs")];
            if q.external_text {
                out.push(option(
                    "approve_trust",
                    "Approve and trust",
                    style,
                    false,
                    "the call runs, and the session is trusted again",
                ));
            }
            out.push(decline("the call does not run"));
            out
        }
        QuestionKind::Budget => vec![
            option(
                "reset",
                "Reset to $0",
                Style::Primary,
                false,
                "spend goes back to $0 and the call runs",
            ),
            decline("the session keeps waiting"),
        ],
        QuestionKind::Change => vec![
            option(
                "accept",
                "Accept",
                Style::Primary,
                false,
                "the task changes",
            ),
            decline("the task stays as it is"),
        ],
        QuestionKind::Extension => vec![
            option(
                "ack",
                "Acknowledge",
                Style::Primary,
                false,
                "the extension goes on",
            ),
            decline("the extension stops"),
        ],
        QuestionKind::Ask => {
            let mut out: Vec<AnswerOption> = q
                .choices
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    option(
                        &format!("choice:{i}"),
                        c,
                        Style::Plain,
                        false,
                        "answers with it",
                    )
                })
                .collect();
            out.push(option(
                "choice:other",
                "Something else",
                Style::Plain,
                true,
                "answers in your words",
            ));
            out
        }
    }
}

/// The longest reason a label carries before it is cut with `…`, as
/// `attention()`'s.
const LABEL_CHARS: usize = 60;

fn clip(s: &str) -> String {
    let line = s.lines().next().unwrap_or_default().trim();
    if line.chars().count() <= LABEL_CHARS {
        return line.to_string();
    }
    let cut: String = line.chars().take(LABEL_CHARS - 1).collect();
    format!("{}…", cut.trim_end())
}

/// What a piece of work needs from people: the same four levels and the same
/// label rules as `attention()`, for every kind. First match wins:
///
/// | # | When | Level | Label |
/// |---|---|---|---|
/// | 1 | a budget question | needs you | `budget: $10.02 of $10` |
/// | 2 | any other question | needs you | `confirm proc.run: run cargo test` |
/// | 3 | `failed` | needs you | `failed: <reason>` |
/// | 4 | `needs_you`, no question | needs you | `blocked: <reason>`, `waiting on you` |
/// | 5 | `running` | working | its doing's label, `turn 4`, `running` |
/// | 6 | `queued` | working | `queued`, `queued · wake` |
/// | 7 | `waiting` | working | its doing's label, `waiting` |
/// | 8 | `ready` | ready | `ready` |
/// | 9 | `done`, `cancelled`, `expired` | idle | `complete`, `done`, `cancelled`, `expired` |
/// | 10 | a state this build doesn't know | working | `working` |
///
/// A failed task stays needs you until someone looks (rule 3). `since_ms`
/// is the view's `state_since_ms`.
pub fn attention_of(v: &WorkView) -> Attention {
    let (level, label) = work_level_and_label(v);
    Attention {
        level,
        label,
        since_ms: v.state_since_ms,
    }
}

fn work_level_and_label(v: &WorkView) -> (Level, String) {
    let open = v.question.as_ref().filter(|_| !v.state.ended());
    if let Some(q) = open {
        if q.kind == QuestionKind::Budget {
            let (spent, limit) = v.cost.map_or((0.0, 0.0), |c| (c.spent_usd, c.limit_usd));
            return (
                Level::NeedsYou,
                format!("budget: {} of {}", usd(spent), usd(limit)),
            );
        }
        let mut label = format!("confirm {}", clip(&question_words(q)));
        if q.floor {
            label.push_str(" · floor");
        }
        return (Level::NeedsYou, label);
    }
    let reason = |word: &str| match v.report.as_ref().and_then(|r| r.reason.as_deref()) {
        Some(r) if !r.is_empty() => format!("{word}: {}", clip(r)),
        _ => word.to_string(),
    };
    let doing = || {
        v.doing
            .as_ref()
            .map(|d| d.label.clone())
            .filter(|l| !l.is_empty())
    };
    match v.state {
        WorkState::Failed => (Level::NeedsYou, reason("failed")),
        WorkState::NeedsYou if v.report.is_some() => (Level::NeedsYou, reason("blocked")),
        WorkState::NeedsYou => (Level::NeedsYou, "waiting on you".into()),
        WorkState::Running => (
            Level::Working,
            doing().unwrap_or_else(|| match v.kind {
                WorkKind::Conversation | WorkKind::Task => "turn 1".into(),
                _ => "running".into(),
            }),
        ),
        WorkState::Queued => (
            Level::Working,
            match v.why.as_deref() {
                Some(w) if !w.is_empty() && w != "input" => format!("queued · {w}"),
                _ => "queued".into(),
            },
        ),
        WorkState::Waiting => (Level::Working, doing().unwrap_or_else(|| "waiting".into())),
        WorkState::Ready => (Level::Ready, "ready".into()),
        WorkState::Done => (
            Level::Idle,
            match v.kind {
                WorkKind::Conversation | WorkKind::Task => "complete".into(),
                _ => "done".into(),
            },
        ),
        WorkState::Cancelled => (Level::Idle, "cancelled".into()),
        WorkState::Expired => (Level::Idle, "expired".into()),
        WorkState::Unknown => (Level::Working, "working".into()),
    }
}

/// A question's words for a label: `proc.run: run cargo test`.
fn question_words(q: &Question) -> String {
    match q.tool.as_deref() {
        Some(t) if !t.is_empty() => match q.prompt.strip_prefix(t) {
            Some(rest) if rest.trim().is_empty() => t.to_string(),
            Some(rest) => format!("{t}: {}", rest.trim()),
            None => format!("{t}: {}", q.prompt),
        },
        _ => q.prompt.clone(),
    }
}

/// An execution's state, as a work state. A wait says which: on input it is
/// ready (or waiting, with a job its turn left running), on its calls, a
/// task, or a time it waits, and on a question it needs you.
fn state_of(state: &str, waiting_on: Option<&WaitingOn>, outstanding: u32) -> WorkState {
    match state {
        "queued" => WorkState::Queued,
        "running" => WorkState::Running,
        "waiting" => match waiting_on {
            Some(WaitingOn::Input) if outstanding == 0 => WorkState::Ready,
            Some(WaitingOn::Confirm { .. } | WaitingOn::Budget { .. }) => WorkState::NeedsYou,
            _ => WorkState::Waiting,
        },
        "blocked" | "budget_exhausted" => WorkState::NeedsYou,
        "failed" => WorkState::Failed,
        "complete" => WorkState::Done,
        "cancelled" => WorkState::Cancelled,
        _ => WorkState::Unknown,
    }
}

impl WorkView {
    /// A reference to this work, as a notice names it.
    pub fn work_ref(&self) -> WorkRef {
        WorkRef {
            id: self.id.clone(),
            kind: self.kind,
            session_id: self.session_id.clone(),
            title: self.title.clone(),
        }
    }

    /// A conversation's or a task's view from today's push, so readers adopt
    /// the notification policy before the work board exists. The view names
    /// sessions, not executions, for its tree: `parent` is the parent
    /// session, and `root` the parent session or its own. Its question is its
    /// first pending one, in brief, asked when it came to need you; its
    /// title stays empty until the view carries one, and `started_at_ms` is
    /// 0 (the push does not say).
    pub fn from_execution(x: &ExecutionView) -> WorkView {
        let kind = match x.kind {
            SessionKind::Conversation => WorkKind::Conversation,
            SessionKind::Task => WorkKind::Task,
        };
        let mut state = state_of(&x.state, x.waiting_on.as_ref(), x.outstanding);
        if !x.pending.is_empty() && !state.ended() && state != WorkState::Failed {
            state = WorkState::NeedsYou;
        }
        let me = WorkRef {
            id: x.execution_id.clone(),
            kind,
            session_id: Some(x.session_id.clone()),
            title: String::new(),
        };
        let question = x
            .pending
            .first()
            .map(|p| brief(p, me.clone(), x.attention.since_ms));
        let cost = (x.spent_usd > 0.0 || x.limit_usd > 0.0).then_some(Cost {
            spent_usd: x.spent_usd,
            limit_usd: x.limit_usd,
            turn_usd: None,
        });
        let report = matches!(x.state.as_str(), "failed" | "blocked")
            .then(|| x.ended_reason.clone())
            .flatten()
            .map(|reason| WorkReport {
                outcome: x.state.clone(),
                first_line: None,
                reason: Some(reason),
            });
        WorkView {
            id: x.execution_id.clone(),
            kind,
            position: x.position,
            at_ms: x.at_ms,
            parent: x.parent_session_id.clone(),
            root: x
                .parent_session_id
                .clone()
                .unwrap_or_else(|| x.session_id.clone()),
            session_id: Some(x.session_id.clone()),
            title: String::new(),
            place: None,
            state,
            previous: x.previous.as_deref().map(|p| state_of(p, None, 0)),
            attention: x.attention.clone(),
            started_at_ms: 0,
            state_since_ms: x.attention.since_ms,
            ended_at_ms: state.ended().then_some(x.at_ms),
            doing: doing(x, state),
            progress: None,
            question,
            cost,
            eta: None,
            below: None,
            why: x.why.clone(),
            report,
            woke: None,
            flagged: false,
        }
    }
}

/// What an execution is doing now, from its state and its wait.
fn doing(x: &ExecutionView, state: WorkState) -> Option<Doing> {
    let what = match (state, &x.waiting_on) {
        (WorkState::Running, _) => DoingKind::Thinking,
        (WorkState::NeedsYou, Some(WaitingOn::Confirm { .. } | WaitingOn::Budget { .. })) => {
            DoingKind::Asking
        }
        (WorkState::NeedsYou, _) if !x.pending.is_empty() => DoingKind::Asking,
        (WorkState::Waiting, Some(WaitingOn::Execution { .. })) => DoingKind::WaitingOnTasks,
        (WorkState::Waiting, Some(WaitingOn::DueAt { .. })) => DoingKind::Sleeping,
        (WorkState::Waiting, _) => DoingKind::Job,
        _ => return None,
    };
    Some(Doing {
        what,
        label: x.attention.label.clone(),
        since_ms: x.attention.since_ms,
        turn: x.turns,
        loop_index: 0,
    })
}

/// A pending question in brief.
fn brief(p: &PendingConfirm, asked_by: WorkRef, asked_at_ms: u64) -> Question {
    let kind = if p.budget {
        QuestionKind::Budget
    } else {
        QuestionKind::Approval
    };
    let prompt = if p.reason.is_empty() {
        p.tool.clone()
    } else {
        format!("{} {}", p.tool, p.reason)
    };
    let mut q = Question {
        id: p.correlation_id.clone(),
        kind,
        asked_by,
        prompt,
        detail: None,
        input: None,
        options: Vec::new(),
        on_expiry: (p.expires_at_ms > 0).then(|| "not run".to_string()),
        expires_at_ms: (p.expires_at_ms > 0).then_some(p.expires_at_ms),
        floor: p.floor,
        may_answer: false,
        tool: Some(p.tool.clone()),
        asked_at_ms,
        external_text: false,
        choices: Vec::new(),
    };
    q.options = answer_options(&q);
    q
}

#[cfg(test)]
#[path = "tests_work.rs"]
mod tests;
