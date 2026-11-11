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
//!
//! **Lock order: `Switch::backlog`, then `Switch::folded`, never the reverse.**
//! The night holds the backlog lock (`backlog_night`) through `backlog_tell`,
//! which takes the folded lock in `folded_read` and again in `folded_told`. No
//! code under the folded lock takes the backlog lock: `folded_read`,
//! `folded_add` and `folded_told` only load and append the one META record.
//! A caller that holds neither (the tick's `folded_add`) takes the folded lock
//! alone, which is safe. Backlog sites: `backlog.rs` (the night), `start.rs`
//! (two), `word.rs` (five); folded sites: the three functions here.

use serde::{Deserialize, Serialize};
use theseus_store::{kinds, NewRecord};

use super::{Ended, Started};
use crate::rpc::Core;

/// The META key of the starts not told yet.
pub const KEY: &str = "self.digest.folded";
/// The starts a note lists; older ones are counted in the note.
pub(crate) const LISTED: usize = 10;
/// The starts the record keeps by name, so a note can take off exactly the ones
/// it counted; past it the oldest are only counted (`Folded::more`).
const KEPT: usize = 200;

/// A start waiting for a note.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Pending {
    pub item: String,
    pub title: String,
    pub at_ms: u64,
}

/// A gate that ended, waiting for a note (theseus-1eemp): the half of a
/// build's story the owner was never told.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Done {
    pub item: String,
    pub title: String,
    pub verdict: String,
    #[serde(default)]
    pub phase: Option<String>,
    #[serde(default)]
    pub report: Option<String>,
    pub at_ms: u64,
}

/// What no note has told yet.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Folded {
    /// Oldest first. As read (`Core::folded_read`) at most [`LISTED`]; as
    /// stored, up to `KEPT`, so a start a tick added while a note was
    /// posting can never push one the note named into the count.
    #[serde(default)]
    pub started: Vec<Pending>,
    /// Starts older than those: the stored count of the ones past `KEPT`
    /// (and, in a record from before it, past the old cap), and, as read, the
    /// older starts the note counts.
    #[serde(default)]
    pub more: u64,
    /// As read only, never stored: the older starts counted in `more` that
    /// are still named in the record, so [`Core::folded_told`] takes off
    /// exactly them.
    #[serde(skip)]
    pub hidden: Vec<Pending>,
    /// The gates that ended since the last note. Kept whole: a build ends
    /// once, and there are never more of these than there were starts.
    #[serde(default)]
    pub ended: Vec<Done>,
}

impl Folded {
    pub(crate) fn is_empty(&self) -> bool {
        self.started.is_empty() && self.more == 0 && self.ended.is_empty()
    }

    /// What a note sees: the newest [`LISTED`] starts, the rest counted in
    /// `more` and named in `hidden`.
    fn view(mut self) -> Folded {
        if self.started.len() > LISTED {
            let cut = self.started.len() - LISTED;
            self.hidden = self.started.drain(..cut).collect();
            self.more += cut as u64;
        }
        self
    }

    /// How many starts it holds.
    pub(crate) fn count(&self) -> usize {
        self.started.len() + self.more as usize
    }

    /// Its ended gates as a night's own.
    pub(crate) fn as_ended(&self) -> Vec<Ended> {
        self.ended
            .iter()
            .map(|d| Ended {
                item: d.item.clone(),
                title: d.title.clone(),
                verdict: d.verdict.clone(),
                phase: d.phase.clone(),
                report: d.report.clone(),
            })
            .collect()
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
        out.extend(self.as_ended().iter().map(|e| format!("- {}", e.line())));
        out
    }
}

impl Core {
    /// What is waiting for a note, read under the lock.
    pub(crate) fn folded_read(&self) -> Folded {
        #[cfg(test)]
        self.rsi
            .folded_reads
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let _one = self.rsi.folded.lock().unwrap_or_else(|e| e.into_inner());
        self.folded_load().view()
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
        while f.started.len() > KEPT {
            f.started.remove(0);
            f.more += 1;
        }
        self.folded_write(&f);
    }

    /// A build's gate ended, recorded for the next note (theseus-1eemp). The
    /// item's own thread hears it at once (`rsi::pr`); this is the half the
    /// owner reads in the night's or the morning's note, so a red build is
    /// never only a line that said work started.
    pub(crate) fn folded_add_ended(&self, e: &Ended) {
        if !self.runner.place_rule.owner_dm_bound() {
            return;
        }
        let _one = self.rsi.folded.lock().unwrap_or_else(|e| e.into_inner());
        let mut f = self.folded_load();
        // An item built again (a red one's next attempt) is one line: its
        // latest verdict.
        f.ended.retain(|d| d.item != e.item);
        f.ended.push(Done {
            item: e.item.clone(),
            title: e.title.clone(),
            verdict: e.verdict.clone(),
            phase: e.phase.clone(),
            report: e.report.clone(),
            at_ms: theseus_protocol::now_unix_ms(),
        });
        while f.ended.len() > KEPT {
            f.ended.remove(0);
        }
        self.folded_write(&f);
    }

    /// A note listed `told`: take those starts off, and only those.
    pub(crate) fn folded_told(&self, told: &Folded) {
        let _one = self.rsi.folded.lock().unwrap_or_else(|e| e.into_inner());
        let mut f = self.folded_load();
        let before = f.clone();
        // What the note counted is named in `hidden` (its `more` holds those
        // and the record's own count): take each off by identity, and only the
        // record's own count off the number.
        f.started
            .retain(|p| !told.started.contains(p) && !told.hidden.contains(p));
        f.more = f
            .more
            .saturating_sub(told.more.saturating_sub(told.hidden.len() as u64));
        f.ended.retain(|d| !told.ended.contains(d));
        if f != before {
            self.folded_write(&f);
        }
    }
}
