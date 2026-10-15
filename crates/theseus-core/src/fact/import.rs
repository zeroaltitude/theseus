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
    /// Episodes whose messages grew: only the new ones were written.
    pub appended: u64,
    /// The client's batch number and its run's total, when it said
    /// (`ImportEpisodesParams`), and the tag's source: the work board's
    /// `batch 41 of 120` and its title read them (theseus-jn17m).
    pub batch: Option<u32>,
    pub batches: Option<u32>,
    pub source: Option<&'a str>,
}

impl Fact for ImportBatch<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ImportBatch);

    fn row(&self) -> Value {
        let mut row = json!({"tag": self.tag, "file": self.file, "by": self.by,
               "imported": self.imported, "nodes": self.nodes});
        if self.appended > 0 {
            row["appended"] = json!(self.appended);
        }
        if let Some(b) = self.batch {
            row["batch"] = json!(b);
        }
        if let Some(b) = self.batches {
            row["batches"] = json!(b);
        }
        if let Some(s) = self.source {
            row["source"] = json!(s);
        }
        row
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            NarrativePart::Session,
            format!(
                "Imported {}{} from {} into {} ({}).",
                crate::narrative::count(self.imported, "episode", "episodes"),
                if self.appended > 0 {
                    format!(
                        " and appended to {}",
                        crate::narrative::count(self.appended, "episode", "episodes")
                    )
                } else {
                    String::new()
                },
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

/// An erase's frame before the last (theseus-jn17m): the sessions erased so
/// far this run and the run's total, which the work board shows as progress
/// (`import.erasing`; the receipt, `import.erased`, is the last frame's alone,
/// so what counts receipts counts erases).
pub struct ImportErasing<'a> {
    pub tag: &'a str,
    pub by: &'a str,
    pub progress: Progress,
}

impl Fact for ImportErasing<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ImportErasing);

    fn row(&self) -> Value {
        let mut row = json!({"tag": self.tag, "by": self.by});
        self.progress.put(&mut row);
        row
    }
}

/// How far a run of frames has come: records written, of the records it has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub done: u64,
    pub of: u64,
}

impl Progress {
    fn put(self, row: &mut Value) {
        row["done"] = json!(self.done);
        row["of"] = json!(self.of);
    }
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
    /// The records written so far this run, of its total (theseus-jn17m).
    pub progress: Option<Progress>,
}

impl Fact for ImportTopics<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ImportTopics);

    fn row(&self) -> Value {
        let mut row = json!({"tag": self.tag, "by": self.by, "made": self.made, "joined": self.joined,
               "taken": self.taken, "retired": self.retired});
        if let Some(p) = self.progress {
            p.put(&mut row);
        }
        row
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

/// A tag's people written (`import.people`, theseus-wy7y), a row for each
/// frame: the people declared and the sessions' lists written; or, by the
/// erase, the people taken away.
pub struct ImportPeople<'a> {
    pub tag: &'a str,
    pub by: &'a str,
    pub made: u64,
    pub joined: u64,
    pub retired: u64,
    /// The records written so far this run, of its total (theseus-jn17m).
    pub progress: Option<Progress>,
}

impl Fact for ImportPeople<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ImportPeople);

    fn row(&self) -> Value {
        let mut row = json!({"tag": self.tag, "by": self.by, "made": self.made, "joined": self.joined,
               "retired": self.retired});
        if let Some(p) = self.progress {
            p.put(&mut row);
        }
        row
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let line = if self.retired > 0 {
            format!(
                "Took back the import {}'s people: {} taken away.",
                self.tag,
                crate::narrative::count(self.retired, "person", "people"),
            )
        } else {
            format!(
                "Placed {} of the import {} with its people ({} declared), by {}.",
                crate::narrative::count(self.joined, "session", "sessions"),
                self.tag,
                crate::narrative::count(self.made, "person", "people"),
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

/// The OpenClaw sync's rows' scope: one a run.
pub fn sync_scope() -> String {
    "import.sync".to_string()
}

/// A sync run's end (theseus-pjmj7, `import.sync.record`): what it read,
/// wrote and was refused, and why it failed when it did. In the frame with
/// the cursor and the counts health shows.
pub struct ImportSynced<'a> {
    pub by: &'a str,
    pub started_ms: u64,
    pub episodes: u64,
    pub messages: u64,
    pub imported: u64,
    pub appended: u64,
    pub skipped: u64,
    pub rejected: u64,
    pub error: Option<&'a str>,
}

impl Fact for ImportSynced<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ImportSynced);

    fn row(&self) -> Value {
        json!({"by": self.by, "started_ms": self.started_ms, "episodes": self.episodes,
               "messages": self.messages, "imported": self.imported, "appended": self.appended,
               "skipped": self.skipped, "rejected": self.rejected, "error": self.error})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let line = match self.error {
            Some(e) => format!("The OpenClaw sync failed: {e}."),
            None => format!(
                "The OpenClaw sync read {} ({}): {} new, {} appended to, {} already held, {} refused.",
                crate::narrative::count(self.episodes, "session", "sessions"),
                crate::narrative::count(self.messages, "message", "messages"),
                self.imported,
                self.appended,
                self.skipped,
                self.rejected,
            ),
        };
        say.line(NarrativePart::Session, line);
    }
}
