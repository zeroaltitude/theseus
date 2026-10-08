//! The import's facts (theseus-0lrr.6): a batch of an episode file written
//! (`import.batch`, in the batch's frame), and a tag erased
//! (`import.erased`, the erase's receipt, in its last frame). Each row is
//! scoped `import:<tag>`'s ledger scope, `import.ledger:<tag>`, so a tag's
//! history is one scan.

use serde_json::{json, Value};
use theseus_protocol::{LedgerKind, NarrativePart};

use super::{Fact, Say};

/// A batch written: its tag, what it read and did, and who sent it.
pub struct ImportBatch<'a> {
    pub tag: &'a str,
    pub file: &'a str,
    pub by: &'a str,
    pub imported: u64,
    pub nodes: u64,
}

impl Fact for ImportBatch<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ImportBatch);

    fn row(&self) -> Value {
        json!({"tag": self.tag, "file": self.file, "by": self.by,
               "imported": self.imported, "nodes": self.nodes})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            NarrativePart::Session,
            format!(
                "Imported {} from {} into {} ({}).",
                crate::narrative::count(self.imported, "episode", "episodes"),
                self.file,
                self.tag,
                crate::narrative::count(self.nodes, "node", "nodes"),
            ),
        );
    }
}

/// A tag erased: the receipt (§5.6), who ordered it and why, and what it
/// tombstoned.
pub struct ImportErased<'a> {
    pub tag: &'a str,
    pub by: &'a str,
    pub why: Option<&'a str>,
    pub sessions: u64,
    pub nodes: u64,
}

impl Fact for ImportErased<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ImportErased);

    fn row(&self) -> Value {
        json!({"tag": self.tag, "by": self.by, "why": self.why,
               "sessions": self.sessions, "nodes": self.nodes})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            NarrativePart::Session,
            format!(
                "Erased the import {}: {} and {} tombstoned, by {}.",
                self.tag,
                crate::narrative::count(self.sessions, "session", "sessions"),
                crate::narrative::count(self.nodes, "node", "nodes"),
                self.by,
            ),
        );
    }
}

/// A tag's topics written (`import.topics`, theseus-anh3), a row for each
/// frame: the topics declared and the sessions' lists written; or, by the
/// erase, the sessions' lists emptied and the topics taken away.
pub struct ImportTopics<'a> {
    pub tag: &'a str,
    pub by: &'a str,
    pub made: u64,
    pub joined: u64,
    pub taken: u64,
    pub retired: u64,
}

impl Fact for ImportTopics<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ImportTopics);

    fn row(&self) -> Value {
        json!({"tag": self.tag, "by": self.by, "made": self.made, "joined": self.joined,
               "taken": self.taken, "retired": self.retired})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let line = if self.taken > 0 || self.retired > 0 {
            format!(
                "Took back the import {}'s topics: {} emptied, {} taken away.",
                self.tag,
                crate::narrative::count(
                    self.taken,
                    "session's memberships",
                    "sessions' memberships"
                ),
                crate::narrative::count(self.retired, "topic", "topics"),
            )
        } else {
            format!(
                "Placed {} of the import {} in its topics ({} declared), by {}.",
                crate::narrative::count(self.joined, "session", "sessions"),
                self.tag,
                crate::narrative::count(self.made, "topic", "topics"),
                self.by,
            )
        };
        say.line(NarrativePart::Session, line);
    }
}

/// A tag's rows' scope.
pub fn scope(tag: &str) -> String {
    format!("import.ledger:{tag}")
}
