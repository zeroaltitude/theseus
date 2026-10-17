//! The owner held a task at its next loop, or let it go on (`task.pause`,
//! `task.resume`). Each rides the frame that writes the META key
//! `task.paused.<session>`; the work board reads the rows to mark the task.

use serde_json::{json, Value};
use theseus_protocol::{LedgerKind, NarrativePart::Session};

use super::{Fact, Say};
use crate::narrative;

/// The task was held: it parks at its next loop and does not report.
pub struct TaskPaused<'a> {
    pub task: &'a str,
    pub session_id: &'a str,
    pub by: &'a str,
}

impl Fact for TaskPaused<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::TaskPaused);

    fn row(&self) -> Value {
        json!({"task": self.task, "session_id": self.session_id, "by": self.by})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!(
                "Task {} was paused by {}: it parks at its next loop and reports when resumed and done.",
                narrative::short(self.session_id),
                self.by
            ),
        );
    }
}

/// The task was let go on.
pub struct TaskResumed<'a> {
    pub task: &'a str,
    pub session_id: &'a str,
    pub by: &'a str,
}

impl Fact for TaskResumed<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::TaskResumed);

    fn row(&self) -> Value {
        json!({"task": self.task, "session_id": self.session_id, "by": self.by})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!(
                "Task {} was resumed by {}.",
                narrative::short(self.session_id),
                self.by
            ),
        );
    }
}
