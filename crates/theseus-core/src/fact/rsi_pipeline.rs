//! The self pipeline's facts (theseus-pw1q.16): one row per transition of
//! an item between the pipeline's queues. Each carries its `SELF_ROWS`
//! fields (`item` and its own, in `fields`) beside every `self.*` row's
//! `what`, `why`, `numbers` and `undo`, so `self.log` shows it as written,
//! and `theseus self pipeline --week` reads its numbers.

use serde_json::{json, Map, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Approval;

use super::{Fact, Rec, Say};

macro_rules! transitions {
    ($($(#[$doc:meta])* $name:ident => $kind:path),* $(,)?) => {
        $(
            $(#[$doc])*
            #[derive(Debug, Clone, PartialEq)]
            pub struct $name(pub Moved);

            impl Fact for $name {
                const KIND: Option<LedgerKind> = Some($kind);

                fn row(&self) -> Value {
                    self.0.row()
                }

                fn narrate(&self, say: &mut Say<'_>) {
                    say.line(Approval, self.0.sentence());
                }
            }
        )*

        /// One transition, of whichever kind.
        #[derive(Debug, Clone, PartialEq)]
        pub enum Transition {
            $($name($name),)*
        }

        impl Transition {
            /// What it says, whatever its kind.
            pub fn moved(&self) -> &Moved {
                match self {
                    $(Transition::$name(f) => &f.0,)*
                }
            }

            /// Its ledger row, for a frame the caller builds.
            pub fn row(&self) -> anyhow::Result<theseus_store::NewRecord> {
                match self {
                    $(Transition::$name(f) => super::row(f, None, None),)*
                }
            }

            /// Its sentences, once its frame is written.
            pub fn announce(&self, rec: &Rec<'_>) {
                match self {
                    $(Transition::$name(f) => rec.announce(f),)*
                }
            }
        }
    };
}

transitions! {
    /// A build started: the item left *ready* for *building*.
    Started => LedgerKind::SelfPipelineStarted,
    /// A start was held: the capacity guard, the budget or the slots said
    /// no. Written when the reason changes, never a row a tick.
    Held => LedgerKind::SelfPipelineHeld,
    /// A rejected review sent the item back to *ready*, once, with the
    /// reviewer's findings.
    Returned => LedgerKind::SelfPipelineReturned,
    /// A second rejection: the item leaves the pipeline for the owner's
    /// view (`waiting_human`).
    Parked => LedgerKind::SelfPipelineParked,
    /// A passed review: the branch joins the *joining* queue.
    Queued => LedgerKind::SelfPipelineQueued,
    /// A build's trees were deleted: its branch joined, or it was dropped or
    /// ended (a regenerable cache, never moved to a trash).
    Cleared => LedgerKind::SelfPipelineCleared,
    /// A build ended with no green branch: its turn ended, or a halt or the
    /// owner's drop stopped it.
    Ended => LedgerKind::SelfPipelineEnded,
}

/// What every transition says: the item, its own fields, and the four every
/// `self.*` row carries.
#[derive(Debug, Clone, PartialEq)]
pub struct Moved {
    /// The backlog item; empty for a hold that names none.
    pub item: String,
    /// The row's own fields (`branch`, `session`, `hold`, …).
    pub fields: Value,
    pub what: String,
    pub why: Option<String>,
    pub numbers: Value,
    pub undo: Option<String>,
}

impl Moved {
    pub fn new(item: &str, what: impl Into<String>) -> Self {
        Self {
            item: item.into(),
            fields: json!({}),
            what: what.into(),
            why: None,
            numbers: Value::Null,
            undo: None,
        }
    }

    pub fn with(mut self, key: &str, v: impl Into<Value>) -> Self {
        self.fields[key] = v.into();
        self
    }

    pub fn why(mut self, why: impl Into<String>) -> Self {
        self.why = Some(why.into());
        self
    }

    pub fn numbers(mut self, n: Value) -> Self {
        self.numbers = n;
        self
    }

    pub fn undo(mut self, u: impl Into<String>) -> Self {
        self.undo = Some(u.into());
        self
    }

    fn row(&self) -> Value {
        let mut m = match &self.fields {
            Value::Object(m) => m.clone(),
            _ => Map::new(),
        };
        m.insert("item".into(), json!(self.item));
        m.insert("what".into(), json!(self.what));
        m.insert("why".into(), json!(self.why));
        m.insert("numbers".into(), self.numbers.clone());
        m.insert("undo".into(), json!(self.undo));
        Value::Object(m)
    }

    fn sentence(&self) -> String {
        let mut c = self.what.chars();
        let what: String = c
            .next()
            .map(|f| f.to_uppercase().chain(c).collect())
            .unwrap_or_default();
        match &self.why {
            Some(w) => format!("Self pipeline: {what} ({w})."),
            None => format!("Self pipeline: {what}."),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A row carries the item, its own fields and the four every self row
    /// has; its sentence starts in upper case.
    #[test]
    fn a_transitions_row_carries_its_fields_and_the_four() {
        let m = Moved::new("tsk_a", "started tsk_a on the local builder")
            .with("builder", "local")
            .why("a slot was free")
            .numbers(json!({"building": 1, "slots": 2}));
        let t = Transition::Started(Started(m));
        assert_eq!(
            t.moved().row(),
            json!({"item": "tsk_a", "builder": "local",
                   "what": "started tsk_a on the local builder", "why": "a slot was free",
                   "numbers": {"building": 1, "slots": 2}, "undo": null})
        );
        assert_eq!(
            t.moved().sentence(),
            "Self pipeline: Started tsk_a on the local builder (a slot was free)."
        );
    }
}
