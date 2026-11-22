//! What waits (slot C; design §5.1), from the records that already exist: the questions waiting for the
//! owner's answer (the kernel's pending confirmations: an approval, a budget question, an `owner_ask`) and
//! the tasks waiting on him (a task waiting on a person, or proposed and waiting for his yes). Each is its
//! id and one line. Nothing new is recorded, and an answered question or a task that moved on is simply no
//! longer read.
//!
//! The selector lays slot C only where the owner is alone; each line is still a piece with its own
//! session's marks, so the gate decides it like any other (a sealed thread's question is not shown).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use theseus_kernel::Kernel;
use theseus_protocol::tasks::TaskState;

use super::select::sources::{Ref, Waiting};
use super::text::tokens;
use super::{Form, Handle, Pos, Sizes, SourceRange, Space};
use crate::exchange::OWNER;
use crate::store::Store;
use crate::task_graph::joined::Homes;

/// The view's [`Waiting`] over the store and the kernel.
pub struct StoreWaiting<'a> {
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
    /// One waiting record: its id, when it began, the session it belongs to, and its line.
    fn item(&self, id: &str, at_ms: u64, session: &str, line: String) -> Ref {
        let size = tokens(&line);
        let space = Space::Waiting { id: id.to_string() };
        self.texts.borrow_mut().insert(space.clone(), line);
        self.depends.borrow_mut().insert(session.to_string());
        let pos = Pos {
            at_ms,
            id: id.to_string(),
        };
        Ref {
            range: SourceRange {
                space,
                first: pos.clone(),
                last: pos,
            },
            marks: self.homes.marks(session),
            sizes: Sizes {
                whole: size,
                spoken: size,
                line: size,
            },
            handle: Handle(id.to_string()),
            form: Form::Line,
            unit: None,
        }
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
            let about = a
                .resource
                .as_deref()
                .map_or(String::new(), |r| format!(" on {r}"));
            let line = format!(
                "{}: waiting for your answer: {}{about}",
                a.correlation_id, a.tool
            );
            out.push(self.item(&a.correlation_id, a.planned_at_ms, &a.session_id, line));
        }
        let tasks = crate::task_graph::all(self.store)?;
        let waits = |s: TaskState| matches!(s, TaskState::WaitingHuman | TaskState::Proposed);
        for t in tasks.iter().filter(|t| waits(t.state)) {
            let line = format!("{}: task {}: {}", t.id, t.state.as_str(), t.title);
            let home = crate::task_graph::home(&tasks, t);
            out.push(self.item(&t.id, t.updated_at_ms, &home, line));
        }
        Ok(out)
    }
}
