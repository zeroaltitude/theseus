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

/// A tag's rows' scope.
pub fn scope(tag: &str) -> String {
    format!("import.ledger:{tag}")
}
