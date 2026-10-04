//! A check task's facts (M5 28a, theseus-vug.3; `check.rs`): the basis it
//! opened with, and each refused check, so the ledger counts both.

use serde_json::{json, Value};
use theseus_protocol::NarrativePart::Session;
use theseus_protocol::{LedgerKind, TaskCheck};

use super::{Fact, Say};

/// A check task opened (`task.check_opened`): its basis, whole.
pub struct TaskCheckOpened<'a> {
    pub short: &'a str,
    pub session_id: &'a str,
    pub basis: &'a TaskCheck,
}

impl Fact for TaskCheckOpened<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::TaskCheckOpened);

    fn row(&self) -> Value {
        json!({"task": self.session_id, "basis": self.basis})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let flags = match self.basis.overlaps.len() {
            0 => String::new(),
            n => format!(
                " Its words share {} with that task's working: flagged, and it runs anyway.",
                crate::narrative::count(n as u64, "span", "spans")
            ),
        };
        say.line(
            Session,
            format!(
                "Task {} checks task {} by its claim: it reads the claim and {}, and nothing \
                 else of that task's session, on {}.{flags}",
                self.short,
                self.basis.checked_short,
                crate::narrative::count(
                    self.basis.admitted.len().saturating_sub(1) as u64,
                    "piece",
                    "pieces"
                ),
                if self.basis.model.is_empty() {
                    "its parent's model"
                } else {
                    &self.basis.model
                }
            ),
        );
    }
}

/// A `task.create { check_of }` refused (`task.check_refused`): a task
/// this conversation did not start, one with no report, an unknown
/// profile, no objective, a piece the exclusion keeps out, or an
/// arrangement that did not resolve.
pub struct TaskCheckRefused<'a> {
    /// `unknown`, `no_report`, `profile`, `invalid`, `no_objective`,
    /// `excluded`, or a class of 27's refusals.
    pub class: &'a str,
    /// What the call named.
    pub checked: &'a str,
    pub reason: &'a str,
}

impl Fact for TaskCheckRefused<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::TaskCheckRefused);

    fn row(&self) -> Value {
        json!({"class": self.class, "check_of": self.checked, "reason": self.reason})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!(
                "A check of task {} was not started ({}). The model reads why.",
                self.checked,
                self.class.replace('_', " ")
            ),
        );
    }
}
