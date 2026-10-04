//! The task graph's facts (M7 39a, theseus-ext.6; §2.4's ledger rows but
//! the lease's): one per verb, each a ledger row, a `task.changed`
//! notification with the record after the change, and a narrative line
//! ("task tsk_x split into 3 (v4 → v5)").

use serde_json::{json, Value};
use theseus_protocol::NarrativePart::Session;
use theseus_protocol::{notify, Event, LedgerKind};

use super::{Fact, Rec, Say};
use crate::task_graph::TaskRecord;

/// A change to one task: the record after it, the version before it, and
/// what the verb adds.
#[derive(Debug, Clone)]
pub struct Change {
    /// The session whose call or answer made it.
    pub session_id: String,
    pub task: TaskRecord,
    /// The version it had before; none for a new task.
    pub from: Option<u64>,
    pub detail: Value,
}

impl Change {
    fn row(&self) -> Value {
        let mut row = json!({
            "task": self.task.id,
            "title": self.task.title,
            "version": self.task.version,
            "state": self.task.state.as_str(),
        });
        if let Some(f) = self.from {
            row["from"] = json!(f);
        }
        if let (Some(r), Value::Object(d)) = (row.as_object_mut(), &self.detail) {
            for (k, v) in d {
                r.insert(k.clone(), v.clone());
            }
        }
        row
    }

    fn event(&self, verb: &str) -> Event {
        Event::TaskChanged(theseus_protocol::tasks::TaskChanged {
            session_id: self.session_id.clone(),
            verb: verb.into(),
            task: self.task.clone(),
        })
    }

    fn versions(&self) -> String {
        match self.from {
            Some(f) => format!("v{f} → v{}", self.task.version),
            None => format!("v{}", self.task.version),
        }
    }

    fn say(&self, verb: &str) -> String {
        let t = &self.task;
        let count = |k: &str| self.detail.get(k).and_then(Value::as_u64).unwrap_or(0);
        let what = match verb {
            "created" => match &t.session {
                Some(_) => format!("task {} created with its session: \"{}\"", t.id, t.title),
                None => format!("task {} created as a plan item: \"{}\"", t.id, t.title),
            },
            "updated" => format!("task {} updated", t.id),
            "split" => format!("task {} split into {}", t.id, count("children")),
            "closed" => format!(
                "task {} closed {}, with {}",
                t.id,
                t.state.as_str(),
                crate::narrative::count(count("added"), "piece of evidence", "pieces of evidence")
            ),
            "change_proposed" => format!(
                "a change to task {}'s {} waits for the operator",
                t.id,
                self.detail
                    .get("fields")
                    .and_then(Value::as_str)
                    .unwrap_or("objective")
            ),
            "change_accepted" => format!("the operator accepted the change to task {}", t.id),
            "change_declined" => format!(
                "the operator declined the change to task {}; it stays as it was",
                t.id
            ),
            "stale_refused" => format!(
                "an edit of task {} named v{}, and it is at v{}: refused, to be read again",
                t.id,
                count("named"),
                t.version
            ),
            other => format!("task {} {other}", t.id),
        };
        format!("{what} ({}).", self.versions())
    }
}

/// One fact type per verb: the row's kind is the verb's.
macro_rules! changes {
    ($($t:ident = $kind:path, $verb:literal;)*) => {
        $(
            pub struct $t<'a>(pub &'a Change);

            impl Fact for $t<'_> {
                const KIND: Option<LedgerKind> = Some($kind);
                const METHOD: Option<&'static str> = Some(notify::TASK_CHANGED);

                fn row(&self) -> Value {
                    self.0.row()
                }

                fn event(&self) -> Option<Event> {
                    Some(self.0.event($verb))
                }

                fn narrate(&self, say: &mut Say<'_>) {
                    say.line(Session, self.0.say($verb));
                }
            }
        )*

        /// The verbs, as `task.changed` names them.
        pub const VERBS: &[&str] = &[$($verb,)*];

        /// A change's row, for a frame the caller builds.
        pub fn row_of(rec: &Rec<'_>, verb: &str, c: &Change) -> anyhow::Result<theseus_store::NewRecord> {
            match verb {
                $($verb => rec.row(&$t(c)),)*
                other => anyhow::bail!("no task verb `{other}`"),
            }
        }

        /// A change's notification and line, once its frame is written.
        pub fn announce(rec: &Rec<'_>, verb: &str, c: &Change) {
            match verb {
                $($verb => rec.announce(&$t(c)),)*
                _ => {}
            }
        }

        /// A change recorded on every channel, its row in the recorder's
        /// next frame (or one now).
        pub fn record(rec: &Rec<'_>, verb: &str, c: &Change) {
            match verb {
                $($verb => rec.record(&$t(c)),)*
                _ => {}
            }
        }
    };
}

changes! {
    TaskCreated = LedgerKind::TaskCreated, "created";
    TaskUpdated = LedgerKind::TaskUpdated, "updated";
    TaskSplit = LedgerKind::TaskSplit, "split";
    TaskClosed = LedgerKind::TaskClosed, "closed";
    TaskChangeProposed = LedgerKind::TaskChangeProposed, "change_proposed";
    TaskChangeAccepted = LedgerKind::TaskChangeAccepted, "change_accepted";
    TaskChangeDeclined = LedgerKind::TaskChangeDeclined, "change_declined";
    TaskStaleRefused = LedgerKind::TaskStaleRefused, "stale_refused";
}
