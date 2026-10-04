//! The parked-task invariant (M5 step 28b; design §2.12; theseus-vug):
//! health's `tasks.parked`, each task in progress that cannot progress by
//! itself, with its blocker named.
//!
//! A task can progress when it has a running turn, a queue place, a job (or
//! any dispatched call) whose completion wakes it, a due time, a wake of its
//! own (37b: a task parked on input with a pending wake continues at that
//! wake), a report or a crash's resume the driver takes, or a question to the
//! operator younger than [`QUESTION_FRESH_MS`]. Anything else is parked: a
//! task waiting on input (a task gets none), one a `/stop` parked, an
//! approval or budget question unanswered for a day, a blocked execution, or
//! one waiting with no wake at all.
//!
//! Read from the open executions alone (`Kernel::open_executions`, by the
//! store's terms) and one action per waiting question, never a history scan.

use theseus_kernel::{ExecState, Execution, SessionKind, Wake};
use theseus_protocol::ParkedTask;

/// A question younger than this can still be answered: its task is not
/// parked yet.
pub const QUESTION_FRESH_MS: u64 = 24 * 3_600_000;

/// What holds a task that cannot progress by itself, or `None` when it
/// can. `asked_at` is when a question (an action, by its correlation id)
/// was asked, when that can be read.
pub fn blocker(
    e: &Execution,
    asked_at: impl Fn(&str) -> Option<u64>,
    now_ms: u64,
) -> Option<(&'static str, String, u64)> {
    if e.kind != SessionKind::Task || e.state.is_terminal() {
        return None;
    }
    if e.resume_pending || !e.report_wakes.is_empty() {
        return None;
    }
    let since = e.updated_at_ms;
    match e.state {
        ExecState::Queued | ExecState::Running => None,
        ExecState::Blocked => Some((
            "blocked",
            match &e.ended_reason {
                Some(r) => format!("blocked: {r}"),
                None => "blocked, and nothing says why".into(),
            },
            since,
        )),
        ExecState::Waiting => match &e.wake {
            Some(Wake::DueAt { .. } | Wake::Actions { .. } | Wake::Execution { .. }) => None,
            Some(Wake::Input) if !e.wakes.is_empty() => None,
            Some(Wake::Input) => Some(match &e.stopped {
                Some(s) => (
                    "stopped",
                    format!("stopped by {}; it waits for input", s.by),
                    since,
                ),
                None => (
                    "input",
                    "waits for input, and a task gets none".into(),
                    since,
                ),
            }),
            Some(Wake::Confirm { confirm_id }) => question(
                "approval",
                "an approval",
                confirm_id,
                &asked_at,
                since,
                now_ms,
            ),
            Some(Wake::Budget { correlation_id }) => question(
                "budget",
                "a budget question",
                correlation_id,
                &asked_at,
                since,
                now_ms,
            ),
            None => Some(("nothing", "waits, and nothing wakes it".into(), since)),
        },
        _ => None,
    }
}

/// A question's blocker once it is a day old.
fn question(
    blocker: &'static str,
    what: &str,
    id: &str,
    asked_at: &impl Fn(&str) -> Option<u64>,
    since: u64,
    now_ms: u64,
) -> Option<(&'static str, String, u64)> {
    let at = asked_at(id).unwrap_or(since);
    let age = now_ms.saturating_sub(at);
    (age >= QUESTION_FRESH_MS).then(|| {
        (
            blocker,
            format!("{what} unanswered for {} h", age / 3_600_000),
            at,
        )
    })
}

/// The parked tasks among `open`, oldest first, with their titles.
pub fn parked(
    open: &[Execution],
    asked_at: impl Fn(&str) -> Option<u64>,
    title: impl Fn(&str) -> Option<String>,
    now_ms: u64,
) -> Vec<ParkedTask> {
    let mut out: Vec<ParkedTask> = open
        .iter()
        .filter_map(|e| {
            let (blocker, detail, since_ms) = blocker(e, &asked_at, now_ms)?;
            Some(ParkedTask {
                task_id: e.session_id.clone(),
                short: crate::task::short(&e.session_id),
                execution_id: e.id.clone(),
                title: title(&e.session_id),
                state: e.state.as_str().into(),
                blocker: blocker.into(),
                detail,
                since_ms,
            })
        })
        .collect();
    out.sort_by_key(|p| p.since_ms);
    out
}

impl crate::rpc::Core {
    /// Health's `tasks` block: the parked tasks, from the open executions.
    pub fn tasks_health(&self) -> theseus_protocol::TasksHealth {
        let open = match self.kernel.open_executions() {
            Ok(o) => o,
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "health: the open executions were not read");
                return Default::default();
            }
        };
        let asked_at = |id: &str| {
            self.kernel
                .action(id)
                .ok()
                .flatten()
                .map(|a| a.planned_at_ms)
        };
        let title = |sid: &str| {
            self.store
                .get_session::<crate::session::SessionRecord>(sid)
                .ok()
                .flatten()
                .and_then(|s| s.title)
        };
        theseus_protocol::TasksHealth {
            parked: parked(&open, asked_at, title, theseus_protocol::now_unix_ms()),
        }
    }
}
