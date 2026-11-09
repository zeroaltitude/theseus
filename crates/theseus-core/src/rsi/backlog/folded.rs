//! The starts a note tells once. A build the pipeline starts after the night's
//! read is not announced on its own: it is recorded here, and the next note
//! that goes out anyway (the night's, or the morning's) lists it with the
//! rest, in one place (the owner, 2026-11-09: five near-identical "Self-improvement
//! backlog: 1 started" lines in two and a half hours).
//!
//! The record is one META value, read and written under `Switch::folded`, so a
//! tick that records and a note that lists never cross. A note removes only
//! what it listed, after its post was written: a start that lands between its
//! read and its post waits for the next note, and a post that failed leaves
//! every start where it was.

use serde::{Deserialize, Serialize};
use theseus_store::{kinds, NewRecord};

use super::Started;
use crate::rpc::Core;

/// The META key of the starts not told yet.
pub const KEY: &str = "self.digest.folded";
/// The starts a note lists; older ones are counted, not kept.
pub(crate) const LISTED: usize = 10;

/// A start waiting for a note.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Pending {
    pub item: String,
    pub title: String,
    pub at_ms: u64,
}

/// What no note has told yet.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Folded {
    /// Oldest first, at most [`LISTED`].
    #[serde(default)]
    pub started: Vec<Pending>,
    /// Starts older than those, past the cap.
    #[serde(default)]
    pub more: u64,
}

impl Folded {
    pub(crate) fn is_empty(&self) -> bool {
        self.started.is_empty() && self.more == 0
    }

    /// How many starts it holds.
    pub(crate) fn count(&self) -> usize {
        self.started.len() + self.more as usize
    }

    /// Its starts as a night's own: the night's wording says them.
    pub(crate) fn as_started(&self, but: &[Started]) -> Vec<Started> {
        self.started
            .iter()
            .filter(|p| !but.iter().any(|s| s.item == p.item))
            .map(|p| Started {
                item: p.item.clone(),
                title: p.title.clone(),
                session_id: String::new(),
                objective: String::new(),
                mint: None,
            })
            .collect()
    }

    /// The lines the morning note appends, a line a start, in the night's
    /// words for one (`night_text`) but for the counts, which it has no task
    /// to read.
    pub(crate) fn lines(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .started
            .iter()
            .map(|p| format!("- started work on: {} (item {})", p.title, p.item))
            .collect();
        if self.more > 0 {
            out.push(format!("- and {} more started", self.more));
        }
        out
    }
}

impl Core {
    /// What is waiting for a note, read under the lock.
    pub(crate) fn folded_read(&self) -> Folded {
        let _one = self.rsi.folded.lock().unwrap_or_else(|e| e.into_inner());
        self.folded_load()
    }

    fn folded_load(&self) -> Folded {
        self.store
            .get_meta::<Folded>(KEY)
            .unwrap_or_else(|e| {
                tracing::warn!(error = %format!("{e:#}"), "self backlog: the folded starts were not read");
                None
            })
            .unwrap_or_default()
    }

    fn folded_write(&self, f: &Folded) {
        let written =
            NewRecord::json(kinds::META, Some(KEY), f).and_then(|r| self.store.append(&[r]));
        if let Err(e) = written {
            tracing::warn!(error = %format!("{e:#}"), "self backlog: the folded starts were not written");
        }
    }

    /// A tick's starts, `(item, title)`, recorded for the next note. Nothing
    /// is recorded where there is no DM to tell (as there was no line then).
    pub(crate) fn folded_add(&self, started: &[(String, String)]) {
        if started.is_empty() || !self.runner.place_rule.owner_dm_bound() {
            return;
        }
        let _one = self.rsi.folded.lock().unwrap_or_else(|e| e.into_inner());
        let mut f = self.folded_load();
        let at_ms = theseus_protocol::now_unix_ms();
        for (item, title) in started {
            // A build started again (a rejected review's) is one line.
            f.started.retain(|p| &p.item != item);
            f.started.push(Pending {
                item: item.clone(),
                title: title.clone(),
                at_ms,
            });
        }
        while f.started.len() > LISTED {
            f.started.remove(0);
            f.more += 1;
        }
        self.folded_write(&f);
    }

    /// A note listed `told`: take those starts off, and only those.
    pub(crate) fn folded_told(&self, told: &Folded) {
        let _one = self.rsi.folded.lock().unwrap_or_else(|e| e.into_inner());
        let mut f = self.folded_load();
        let before = f.clone();
        f.started.retain(|p| !told.started.contains(p));
        f.more = f.more.saturating_sub(told.more);
        if f != before {
            self.folded_write(&f);
        }
    }
}
