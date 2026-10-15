//! A failure's dismissal and a failed task's retry (theseus-7zgsm). The
//! dismissal's row is the kernel's (`execution.dismissed`, written in the
//! frame that records it); this module says it. The retry's row,
//! `task.retried`, rides the frame that opens the new task.

use serde_json::{json, Value};
use theseus_protocol::{LedgerKind, NarrativePart::Session};

use super::{Fact, Say};
use crate::narrative;

/// The owner dismissed a failed, blocked or budget-exhausted execution: it
/// asks for no one now. Its row is the kernel's.
pub struct FailureDismissed<'a> {
    pub execution_id: &'a str,
    pub session_id: &'a str,
    pub state: &'a str,
    pub by: &'a str,
}

impl Fact for FailureDismissed<'_> {
    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!(
                "The {} of {} was dismissed by {}: it stops needing anyone, and its record stays.",
                self.state,
                narrative::short(self.session_id),
                self.by
            ),
        );
    }
}

/// A failed task was tried again as a new, linked task.
pub struct TaskRetried<'a> {
    pub failed_execution: &'a str,
    pub failed_session: &'a str,
    pub execution_id: &'a str,
    pub session_id: &'a str,
    pub limit: theseus_kernel::Micros,
    pub failed_limit: theseus_kernel::Micros,
    pub note: Option<&'a str>,
    pub by: &'a str,
}

impl Fact for TaskRetried<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::TaskRetried);

    fn row(&self) -> Value {
        json!({"retry_of": self.failed_execution, "failed_session": self.failed_session,
               "execution_id": self.execution_id, "task": self.session_id,
               "limit_usd": theseus_kernel::micros_to_usd(self.limit),
               "failed_limit_usd": theseus_kernel::micros_to_usd(self.failed_limit),
               "note": self.note, "by": self.by})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!(
                "Task {} was tried again by {} as {}, with {} carved (the failed one had {}); the \
                 failed one is dismissed.{}",
                narrative::short(self.failed_session),
                self.by,
                narrative::short(self.session_id),
                narrative::dollars(self.limit),
                narrative::dollars(self.failed_limit),
                self.note.map(|n| format!(" Note: {n}")).unwrap_or_default()
            ),
        );
    }
}
