//! Who retitled a task before its retitle left a mark (`task.title_by:<id>`, written in the edit's own
//! frame since issue 28's fourth pass). A retitle made before then is in the ledger and nowhere else:
//! `task.update` writes a `task.updated` row, or a `task.change_accepted` one when the same call also
//! changed layer 1, whose data carries `task` and `fields` (the comma-joined fields it touched) and whose
//! `session_id` is the retitling session. The same two kinds carry every other task edit, and `plan_step`
//! writes a `task.updated` row with no `fields` each time a step starts: those rows count toward the cap
//! and never match.
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
//! **What it answers when it finds nothing** depends on whether the walk could see everything. A walk
//! that ran to the end of the rows, over a ledger that reaches back past the record's creation, saw every
//! retitle there could be: there was none, and the record's own word stands (a model's if it says a model
//! made it, else the owner's). A walk that was **cut short** (the cap reached with rows left, or the
//! shape-building window after a start, which reads only the newest rows of every kind) or a ledger that
//! begins after the record was created (rows of its life are gone, and a row never written looks the same
//! as one that is gone) cannot say: the title is **unproven** (`Hand::Unproven`), a text nothing proves the
//! owner's, so a model's by the rule on `Hand`, and held from a restricted turn.

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

/// What a walk found, and whether it was cut short.
#[derive(Debug, Default)]
pub struct Walk {
    /// The session that last retitled each task that has a row in what was read.
    pub found: BTreeMap<String, String>,
    /// Whether the walk stopped before the end of the rows it could have read: the cap reached with
    /// rows left, or the shape-building window's tail read, which holds only the newest rows. A title
    /// with no row in a walk that was cut short is unproven; in one that ran to the end it was never
    /// retitled, or its row is gone from the ledger (`ledger_begins`).
    pub cut: bool,
}

/// The session that last retitled each of `wanted`, as the newest rows say, reading at most `cap` rows;
/// a task with no such row in them is left out, and `cut` says whether the rows ran out or the walk did.
pub fn retitlers(store: &Store, wanted: &BTreeSet<&str>, cap: usize) -> Result<Walk> {
    let mut walk = Walk::default();
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
                        .is_ok_and(|row| take(&mut walk.found, wanted, &row))
                    {
                        return Ok(walk);
                    }
                }
                match (out.more, out.first) {
                    (true, Some(first)) => before = Some(first),
                    _ => return Ok(walk),
                }
            }
            // The index's shape is built after serving: the newest rows of every kind, the same cap.
            // Fewer rows than the cap is the whole ledger; as many may leave older ones unread.
            None => {
                let rows = store.ledger_tail::<LedgerRow>(cap)?;
                #[cfg(test)]
                ROWS_READ.with(|c| c.set(c.get() + rows.len()));
                walk.cut = rows.len() >= cap;
                for (_, row) in rows.iter().rev() {
                    if take(&mut walk.found, wanted, row) {
                        break;
                    }
                }
                return Ok(walk);
            }
        }
    }
    // The cap was reached with rows left (the last page said so).
    walk.cut = true;
    Ok(walk)
}

/// When the ledger's oldest row was written, unix ms: none for an empty ledger. A record created before
/// it has a stretch of its life the ledger does not hold, so a retitle in it would leave no row to find.
pub fn ledger_begins(store: &Store) -> Result<Option<u64>> {
    Ok(store
        .ledger_after::<LedgerRow>(0, 1)?
        .first()
        .map(|(_, row)| row.at_unix_ms))
}
