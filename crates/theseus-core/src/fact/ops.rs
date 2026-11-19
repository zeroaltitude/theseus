//! The Operations page's verbs (theseus-jid7t.2): each is the owner's act and
//! writes one row, in the frame that changes the state it names. None speaks
//! to a place: a card learns of a task change through the kernel's own
//! records, as it does of any other surface's.

use serde_json::{json, Value};
use theseus_protocol::{LedgerKind, NarrativePart::Session};

use super::{Fact, Say};
use crate::narrative;

/// The owner chose, or let go of, the model a session's turns run on.
pub struct SessionProfileSet<'a> {
    pub session_id: &'a str,
    pub from: Option<&'a str>,
    pub to: Option<&'a str>,
    pub by: &'a str,
}

impl Fact for SessionProfileSet<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SessionProfileSet);

    fn row(&self) -> Value {
        json!({"session_id": self.session_id, "from": self.from, "to": self.to, "by": self.by})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let line = match self.to {
            Some(to) => format!(
                "{} set the model of session {} to {to} from its next turn.",
                self.by,
                narrative::short(self.session_id)
            ),
            None => format!(
                "{} let go of the model chosen for session {}.",
                self.by,
                narrative::short(self.session_id)
            ),
        };
        say.line(Session, line);
    }
}

/// A turn asked another profile one question (`profile.ask`, issue 28). Counts and names only: the
/// question and the answer are in the turn's own nodes, never in a row. The call's money is its
/// action's, which the day ceiling reads; this row carries it for a reader of the books.
pub struct ProfileAsked<'a> {
    pub profile: &'a str,
    pub provider: &'a str,
    pub model: &'a str,
    pub correlation_id: &'a str,
    /// `answered`, `cut` (it hit its cap) or `failed`.
    pub outcome: &'a str,
    /// A failure's class.
    pub class: Option<&'a str>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
}

impl Fact for ProfileAsked<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ProfileAsked);

    fn row(&self) -> Value {
        json!({"profile": self.profile, "provider": self.provider, "model": self.model,
            "correlation_id": self.correlation_id, "outcome": self.outcome, "class": self.class,
            "input_tokens": self.input_tokens, "output_tokens": self.output_tokens,
            "cost_usd": self.cost_usd})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let line = match self.class {
            Some(class) => format!(
                "Asked {} (model {}) one question; it could not answer ({class}).",
                self.profile, self.model
            ),
            None => format!(
                "Asked {} (model {}) one question; it {} ({} tokens out, {}).",
                self.profile,
                self.model,
                self.outcome,
                self.output_tokens,
                format_args!("${:.4}", self.cost_usd)
            ),
        };
        say.line(Session, line);
    }
}

/// A task's report was given another home.
pub struct TaskRehomed<'a> {
    pub task: &'a str,
    pub session_id: &'a str,
    pub from: Option<&'a str>,
    pub to: Option<&'a str>,
    pub by: &'a str,
}

impl Fact for TaskRehomed<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::TaskRehomed);

    fn row(&self) -> Value {
        json!({"task": self.task, "session_id": self.session_id, "from": self.from, "to": self.to, "by": self.by})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!(
                "Task {} will report to another home, by {}.",
                narrative::short(self.session_id),
                self.by
            ),
        );
    }
}

/// A deferred task's start was moved.
pub struct TaskRescheduled<'a> {
    pub task: &'a str,
    pub session_id: &'a str,
    pub from: u64,
    pub to: u64,
    pub by: &'a str,
}

impl Fact for TaskRescheduled<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::TaskRescheduled);

    fn row(&self) -> Value {
        json!({"task": self.task, "session_id": self.session_id, "from": self.from, "to": self.to, "by": self.by})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!(
                "Task {} will start at {}, moved by {}.",
                narrative::short(self.session_id),
                crate::task_start::day_hm(self.to),
                self.by
            ),
        );
    }
}
