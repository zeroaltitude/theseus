//! What waits (slot C; design §5.1), from the records that already exist: the questions waiting for the
//! owner's answer (the kernel's pending confirmations: an approval, a budget question, an `owner_ask`) and
//! the tasks waiting on him (a task waiting on a person, or proposed and waiting for his yes). Each is its
//! id and one line. Nothing new is recorded, and an answered question or a task that moved on is simply no
//! longer read.
//!
//! The selector lays slot C only where the owner is alone; each line is asked of the gate with its own
//! session's marks BEFORE it is read into the turn (a sealed thread's question is not shown, and the view
//! does not depend on a session it showed nothing of), and again as any piece is.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use theseus_kernel::Kernel;
use theseus_protocol::tasks::TaskState;

use super::gate::Gate;
use super::select::sources::{Ref, Waiting};
use super::sources_names::admitted;
use super::text::tokens;
use super::{Form, Handle, Marks, PointOfView, Pos, Provenance, Sizes, SourceRange, Space};
use crate::exchange::OWNER;
use crate::store::Store;
use crate::task_graph::joined::Homes;

/// The view's [`Waiting`] over the store and the kernel.
pub struct StoreWaiting<'a> {
    pub gate: &'a dyn Gate,
    pub pov: &'a PointOfView,
    pub store: &'a Store,
    pub kernel: &'a Kernel,
    /// A session's marks as the gate reads them.
    pub homes: &'a dyn Homes,
    /// Each text read, by its space; the renderer lays it (`wire_text`).
    pub texts: &'a RefCell<BTreeMap<Space, String>>,
    /// The sessions read from (`Sessions::depends`).
    pub depends: &'a RefCell<BTreeSet<String>>,
}

impl StoreWaiting<'_> {
    /// One waiting record: its id, when it began, the session it belongs to, and its line, made only once
    /// the gate admits it; none when it does not, and then nothing of it is read into the turn.
    fn item(
        &self,
        (id, at_ms, session): (&str, u64, &str),
        line: impl FnOnce() -> String,
    ) -> Option<Ref> {
        let marks = self.homes.marks(session);
        let line = admitted_line(self.gate, self.pov, &marks, at_ms, line)?;
        let size = tokens(&line);
        let space = Space::Waiting {
            id: id.to_string(),
            from: Some(Provenance {
                sessions: vec![session.to_string()],
                exchanges: Vec::new(),
            }),
        };
        self.texts.borrow_mut().insert(space.clone(), line);
        self.depends.borrow_mut().insert(session.to_string());
        let pos = Pos {
            at_ms,
            id: id.to_string(),
        };
        Some(Ref {
            range: SourceRange {
                space,
                first: pos.clone(),
                last: pos,
            },
            marks,
            sizes: Sizes {
                whole: size,
                spoken: size,
                line: size,
            },
            handle: Handle(id.to_string()),
            form: Form::Line,
            unit: None,
        })
    }
}

impl Waiting for StoreWaiting<'_> {
    /// What waits between Theseus and the owner; another person has none of his.
    fn waiting(&self, person: &str) -> anyhow::Result<Vec<Ref>> {
        if person != OWNER {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for a in self.kernel.pending_confirms()? {
            let line = || {
                let about = (a.resource.as_deref()).map_or(String::new(), |r| format!(" on {r}"));
                format!(
                    "{}: waiting for your answer: {}{about}",
                    a.correlation_id, a.tool
                )
            };
            out.extend(self.item((&a.correlation_id, a.planned_at_ms, &a.session_id), line));
        }
        let tasks = crate::task_graph::all(self.store)?;
        let waits = |s: TaskState| matches!(s, TaskState::WaitingHuman | TaskState::Proposed);
        for t in tasks.iter().filter(|t| waits(t.state)) {
            let line = || format!("{}: task {}: {}", t.id, t.state.as_str(), t.title);
            let home = crate::task_graph::home(&tasks, t);
            out.extend(self.item((&t.id, t.updated_at_ms, &home), line));
        }
        Ok(out)
    }
}

/// A waiting record's line, made by `line` only when the gate admits a record of `marks`: a refused record's
/// content is never read.
fn admitted_line(
    gate: &dyn Gate,
    pov: &PointOfView,
    marks: &Marks,
    at_ms: u64,
    line: impl FnOnce() -> String,
) -> Option<String> {
    admitted(gate, pov, marks.clone(), at_ms).then(line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::gate::Admitted;
    use crate::view::{Anchor, Audience, Author, Piece, Present};

    /// A gate that admits nothing.
    struct Shut;

    impl Gate for Shut {
        fn admit(&self, _: &PointOfView, _: Piece) -> Option<Admitted<Piece>> {
            None
        }
    }

    /// A refused waiting record's line is never made: its content closure is not called.
    #[test]
    fn a_refused_waiting_records_line_is_never_made() {
        let pov = PointOfView {
            anchor: Anchor::Place("local".into()),
            person: Some(OWNER.into()),
            present: Present::OwnerAlone,
            at_hand: None,
            now_ms: 9_000,
        };
        let marks = Marks {
            place: Some("discord:dm:42".into()),
            audience: Audience::Private,
            author: Author::Theseus,
            outside: false,
            sealed: true,
            veiled: false,
        };
        let made = std::cell::Cell::new(false);
        let line = || {
            made.set(true);
            "a line".to_string()
        };
        assert_eq!(admitted_line(&Shut, &pov, &marks, 1, line), None);
        assert!(!made.get(), "the refused record's content was read");
    }
}
