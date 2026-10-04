//! A task's arrangement's facts (M5 27, theseus-vug.2): the pieces a task
//! started from, the fidelity check's ack, and each refused arrangement, so
//! the ledger counts how often quotes fail to resolve (M5 §5's Q7).

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Session;

use super::{Fact, Say};
use crate::arrangement::Piece;
use crate::narrative;

/// A task started from its arrangement (`task.arranged`): each piece by
/// reference, and whether the call acknowledged the fidelity check.
pub struct TaskArranged<'a> {
    pub short: &'a str,
    pub session_id: &'a str,
    pub pieces: &'a [Piece],
    pub fidelity_ack: bool,
    /// The operator's messages in the parent since its last task.
    pub humans: usize,
    pub brief_chars: usize,
}

impl Fact for TaskArranged<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::TaskArranged);

    fn row(&self) -> Value {
        json!({
            "task": self.session_id,
            "pieces": crate::arrangement::meta(self.pieces),
            "fidelity_ack": self.fidelity_ack,
            "human_messages": self.humans,
            "brief_chars": self.brief_chars,
        })
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let roles: Vec<String> = self
            .pieces
            .iter()
            .map(|p| match p.superseded_by {
                Some(_) => format!("{} (superseded)", p.role.as_str()),
                None => p.role.as_str().to_string(),
            })
            .collect();
        let ack = if self.fidelity_ack {
            "; the fidelity check was acknowledged: a short brief from a long discussion, with \
             one piece"
        } else {
            ""
        };
        say.line(
            Session,
            format!(
                "Task {}'s arrangement: {} quoted from this session ({}), which it reads \
                 verbatim after its brief{ack}.",
                self.short,
                narrative::count(self.pieces.len() as u64, "piece", "pieces"),
                roles.join(", ")
            ),
        );
    }
}

/// A `task.create` refused for its arrangement
/// (`task.arrangement_refused`): no arrangement, none that defines the work,
/// a quote that did not resolve, or the fidelity check. The model reads why
/// and tries again.
pub struct TaskArrangementRefused<'a> {
    /// `missing`, `no_objective`, `invalid`, `no_match`, `ambiguous`,
    /// `short`, `unknown_node`, `same_node`, or `fidelity`.
    pub class: &'a str,
    pub pieces: usize,
    pub reason: &'a str,
}

impl Fact for TaskArrangementRefused<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::TaskArrangementRefused);

    fn row(&self) -> Value {
        json!({"class": self.class, "pieces": self.pieces, "reason": self.reason})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!(
                "A task was not started: its arrangement was refused ({}). The model reads \
                 why and can try again.",
                self.class.replace('_', " ")
            ),
        );
    }
}
