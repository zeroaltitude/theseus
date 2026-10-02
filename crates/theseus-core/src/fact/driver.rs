//! The driver's facts: the heartbeat, a job's spooled completion, a cancel,
//! a stop, a lost wrapper, and a continuation (`rpc/driver.rs`).

use serde_json::{json, Value};
use theseus_kernel::{Action, Completion, Execution, Outcome};
use theseus_protocol::NarrativePart::{Job, Session};
use theseus_protocol::{notify, ConfirmResolved, Event, LedgerKind};

use super::{Fact, Say};
use crate::narrative;

/// The heartbeat acted (`why`: the timer, a wrapper's notice): executions a
/// wait came due for, actions settled from a wrapper's evidence, and actions
/// marked unknown.
pub struct HeartbeatActed<'a> {
    pub why: &'a str,
    pub due: u64,
    pub evidence: u64,
    pub unknown: u64,
}

impl Fact for HeartbeatActed<'_> {
    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Job,
            format!(
                "Heartbeat ({}): {} woke because a wait came due, {} settled from a job \
                 wrapper's evidence, {} marked unknown.",
                self.why,
                narrative::count(self.due, "execution", "executions"),
                narrative::count(self.evidence, "action", "actions"),
                self.unknown
            ),
        );
    }
}

/// A job's completion came from the spool.
pub struct SpooledCompletion<'a> {
    pub completion: &'a Completion,
    pub action: &'a Action,
}

impl Fact for SpooledCompletion<'_> {
    fn narrate(&self, say: &mut Say<'_>) {
        let c = self.completion;
        let outcome = match c.outcome {
            Outcome::Succeeded => "succeeded",
            Outcome::Failed => "failed",
            Outcome::Unknown => "an unknown outcome",
        };
        let exit = c
            .detail
            .as_ref()
            .and_then(|d| d.get("exit_code"))
            .and_then(Value::as_i64)
            .map(|x| format!(", exit code {x}"))
            .unwrap_or_default();
        say.line(
            Job,
            format!(
                "Job {} ({}) finished: {outcome}{exit}; its completion came from the spool.",
                narrative::short(&c.correlation_id),
                self.action.tool
            ),
        );
    }
}

/// A question the operator was asked closed with its execution's cancel
/// (`confirm.resolved`, cancelled).
pub struct QuestionCancelled<'a> {
    pub question: &'a Action,
    pub by: &'a str,
}

impl Fact for QuestionCancelled<'_> {
    const METHOD: Option<&'static str> = Some(notify::CONFIRM_RESOLVED);

    fn event(&self) -> Option<Event> {
        Some(Event::ConfirmResolved(ConfirmResolved {
            session_id: self.question.session_id.clone(),
            correlation_id: self.question.correlation_id.clone(),
            by: Some(self.by.into()),
            cancelled: true,
            ..Default::default()
        }))
    }
}

/// An execution was cancelled: its actions were told to stop.
pub struct ExecutionCancelled<'a> {
    pub execution: &'a Execution,
    pub by: &'a str,
    pub stopped: usize,
}

impl Fact for ExecutionCancelled<'_> {
    fn narrate(&self, say: &mut Say<'_>) {
        let e = self.execution;
        say.line(
            Session,
            format!(
                "Execution {} cancelled by {}: {} stopped; it is {} now.",
                narrative::short(&e.id),
                self.by,
                narrative::count(self.stopped as u64, "action", "actions"),
                e.state.as_str()
            ),
        );
    }
}

/// A question a `/stop` declined (W1; `confirm.resolved`, stopped).
pub struct QuestionStopped<'a> {
    pub question: &'a Action,
    pub by: &'a str,
}

impl Fact for QuestionStopped<'_> {
    const METHOD: Option<&'static str> = Some(notify::CONFIRM_RESOLVED);

    fn event(&self) -> Option<Event> {
        Some(Event::ConfirmResolved(ConfirmResolved {
            session_id: self.question.session_id.clone(),
            correlation_id: self.question.correlation_id.clone(),
            by: Some(self.by.into()),
            stopped: true,
            ..Default::default()
        }))
    }
}

/// A `/stop` (W1): what it told to stop and declined, and that the session
/// goes on, waiting on its next input.
pub struct ExecutionStopped<'a> {
    pub execution: &'a Execution,
    pub by: &'a str,
    pub to_kill: usize,
    pub declined: usize,
    pub turn_running: bool,
    pub tasks_running: u32,
}

impl Fact for ExecutionStopped<'_> {
    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!(
                "Stopped by {}: {} told to stop, {} declined{}; the session goes on, waiting on \
                 its next input{}.",
                self.by,
                narrative::count(self.to_kill as u64, "action", "actions"),
                narrative::count(self.declined as u64, "question", "questions"),
                if self.turn_running {
                    ", and the running turn ends at its next step"
                } else {
                    ""
                },
                match (self.tasks_running, self.execution.wakes.len()) {
                    (0, 0) => String::new(),
                    (t, w) => format!(
                        " ({} and {} go on)",
                        narrative::count(t as u64, "task", "tasks"),
                        narrative::count(w as u64, "wake", "wakes")
                    ),
                }
            ),
        );
    }
}

/// A job's wrapper was killed before it reported (theseus-6uo): its outcome
/// is unknown (`job.wrapper_lost`), a security event.
pub struct WrapperLost<'a> {
    pub action: &'a Action,
    pub pid: u32,
    pub signal: i32,
}

impl Fact for WrapperLost<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::JobWrapperLost);

    fn row(&self) -> Value {
        let a = self.action;
        json!({"correlation_id": a.correlation_id, "pid": self.pid, "signal": self.signal,
            "tool": a.tool, "execution_id": a.execution_id})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Job,
            format!(
                "Job {} ({}) lost its wrapper (pid {}, killed by signal {}) before it reported: \
                 the job, or something beside it, killed it. Its outcome is unknown.",
                narrative::short(&self.action.correlation_id),
                self.action.tool,
                self.pid,
                self.signal
            ),
        );
    }
}

/// The driver takes a continuation turn for an execution that can run
/// without the operator.
pub struct DriverResumes<'a> {
    pub execution: &'a Execution,
}

impl Fact for DriverResumes<'_> {
    fn narrate(&self, say: &mut Say<'_>) {
        let e = self.execution;
        say.line(
            Session,
            format!(
                "The driver resumes execution {}: {}.",
                narrative::short(&e.id),
                match e.queued_results.len() {
                    0 if e.resume_pending => "it was woken".to_string(),
                    0 => "it is queued".to_string(),
                    n => format!(
                        "{} arrived",
                        narrative::count(n as u64, "result", "results")
                    ),
                }
            ),
        );
    }
}
