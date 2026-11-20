//! Who retitled a task before its retitle left a mark (`task.title_by:<id>`, written in the edit's own
//! frame since issue 28's fourth pass). A retitle made before then is in the ledger and nowhere else:
//! `task.update` writes a `task.updated` row, or a `task.change_accepted` one when the same call also
//! changed layer 1, whose data carries `task` and `fields` (the comma-joined fields it touched) and whose
//! `session_id` is the retitling session.
//!
//! **The read is bounded and shared.** `look.work` asks for up to 24 titles at once (12 projects and their
//! next milestones). Only a record whose `version` is above 1 can have been retitled (a record starts at 1
//! and every change moves it), and only one with no mark stands in need; for all of those together this
//! walks the newest rows of the two kinds, newest first, **once**, and stops as soon as every title has
//! its row or [`ROWS_MAX`] rows have been read. So the worst case of a look is [`ROWS_MAX`] rows read and
//! decoded (four pages of [`PAGE`] through the ledger's tag index), however many projects it shows, and a
//! record edited but never retitled (a claim, a close) costs that once, not once each. A store that has
//! never been edited costs nothing.
//!
//! **What it answers when it finds nothing.** The title is unproven: the caller answers as it did before
//! this existed (a model's if the record says a model made it, else the owner's). That is the answer when
//! the row is older than the newest [`ROWS_MAX`], when the ledger no longer has it, and when the index's
//! shape is still being built after a start and the newest [`ROWS_MAX`] rows of every kind do not hold it.
//! Nothing in the record shows a retitle that no row remembers, so nothing here guesses one.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use theseus_protocol::LedgerKind;
use theseus_store::pages::ledger_kind;
use theseus_store::{kinds, Page};

use crate::ledger::LedgerRow;
use crate::store::Store;

/// The most rows one look reads to find retitles.
pub const ROWS_MAX: usize = 2_000;
/// The rows of one page.
const PAGE: usize = 500;

#[cfg(test)]
thread_local! {
    /// The rows this thread's reads have taken: what a test holds the bound to.
    pub(crate) static ROWS_READ: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

const ROW_KINDS: [LedgerKind; 2] = [LedgerKind::TaskUpdated, LedgerKind::TaskChangeAccepted];

/// What one row says: the task and the session, when it retitled.
fn retitle_of(row: &LedgerRow) -> Option<(String, String)> {
    if !ROW_KINDS.iter().any(|k| k.as_str() == row.kind) {
        return None;
    }
    let titled = row.data["fields"]
        .as_str()?
        .split(',')
        .any(|f| f.trim() == "title");
    let task = row.data["task"].as_str()?;
    let session = row.session_id.as_deref()?;
    titled.then(|| (task.to_string(), session.to_string()))
}

/// Note `row`'s retitle if it is one of `wanted`'s (the first seen of a task is its newest); whether every
/// one of them has its row now.
fn take(found: &mut BTreeMap<String, String>, wanted: &BTreeSet<&str>, row: &LedgerRow) -> bool {
    if let Some((task, session)) = retitle_of(row) {
        if wanted.contains(task.as_str()) {
            found.entry(task).or_insert(session);
        }
    }
    found.len() == wanted.len()
}

/// The session that last retitled each of `wanted`, as the newest rows say, reading at most `cap` rows;
/// a task with no such row in them is left out.
pub fn retitlers(
    store: &Store,
    wanted: &BTreeSet<&str>,
    cap: usize,
) -> Result<BTreeMap<String, String>> {
    let mut found: BTreeMap<String, String> = BTreeMap::new();
    let (mut read, mut before) = (0usize, None);
    while read < cap {
        let page = Page {
            kind: kinds::LEDGER,
            tags: ROW_KINDS.iter().map(|k| ledger_kind(k.as_str())).collect(),
            before,
            limit: PAGE.min(cap - read),
            ..Page::default()
        };
        match store.ledger_page(&page)? {
            Some(out) => {
                read += out.records.len();
                #[cfg(test)]
                ROWS_READ.with(|c| c.set(c.get() + out.records.len()));
                // A page is oldest first; the newest row of a task is the one that counts.
                for r in out.records.iter().rev() {
                    if r.decode::<LedgerRow>()
                        .is_ok_and(|row| take(&mut found, wanted, &row))
                    {
                        return Ok(found);
                    }
                }
                match (out.more, out.first) {
                    (true, Some(first)) => before = Some(first),
                    _ => break,
                }
            }
            // The index's shape is built after serving: the newest rows of every kind, the same cap.
            None => {
                for (_, row) in store.ledger_tail::<LedgerRow>(cap)?.iter().rev() {
                    #[cfg(test)]
                    ROWS_READ.with(|c| c.set(c.get() + 1));
                    if take(&mut found, wanted, row) {
                        break;
                    }
                }
                break;
            }
        }
    }
    Ok(found)
}
