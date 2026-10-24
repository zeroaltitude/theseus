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
