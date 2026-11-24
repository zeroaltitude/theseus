//! The view record: what one turn was shown and why: every piece by reference with its slot, reasons, form
//! and tokens, and what was dropped and why. A record of its own, keyed by today's compilation record (a META
//! key, so no field on an existing record and no store-format step). Read by the renderer, the replay and
//! `theseus view show <turn>` (the record work's).
//!
//! - **The one call the compiler makes**, once per view it builds: [`keep`]`(store, turn, compilation,
//!   &view)`. It writes nothing at once: the record, its turn's index entry and the view's counts
//!   ([`Tally`], META `view_tally.<compilation>`) ride the turn's next frame, as every row a turn defers does.
//! - **The reads**: [`read`] one view by its compilation, [`newest`] a turn's newest view, [`of_turn`] a
//!   turn's newest views in the order built, [`tally`] a view's counts. Each is bounded.
//! - **What a reader is shown** ([`show`]): a stored record is a map of what WAS admitted, never a
//!   permission: every piece's source is checked again today ([`show::Source`], `source.rs`) before its
//!   reference, and then its text, is shown. No count of what is left out.
//!
//! The design note is `docs/design/view-record.md`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use theseus_store::{kinds, NewRecord, Store as _};

use super::budget::{Dropped, Residue, Shown, View};
use super::{PointOfView, Reason, Slot};
use crate::store::Store;

pub mod show;
pub mod source;
#[cfg(test)]
pub(crate) mod tests;
#[cfg(test)]
mod tests_kinds;
mod text;

pub use text::StoreText;

/// `view.<compilation id>`.
pub const VIEW_PREFIX: &str = "view.";
/// `view_turn.<turn id>\u{1}<20-digit u64::MAX - kept at>\u{1}<compilation id>`: a turn's views, newest
/// first. "Kept at" is the order the views were kept in ([`kept_at`], microseconds, never the same twice in
/// one process): a view kept again for a new chain of the turn reuses its assembly's time, which cannot
/// order the two. A record written before it holds the assembly's milliseconds, and sorts as older.
pub const VIEW_TURN_PREFIX: &str = "view_turn.";
/// `view_tally.<compilation id>`: a view's counts ([`Tally`]).
pub const VIEW_TALLY_PREFIX: &str = "view_tally.";
/// Between a turn index key's parts; no id may hold it ([`keep`] refuses one that does).
pub const SEP: char = '\u{1}';
/// The most views of one turn [`of_turn`] reads: the newest.
pub const VIEWS_READ: usize = 32;

/// Which way a stored view's request was built. A record is only ever written by the view; the name is
/// kept so every record reads as it was written (`[context] assembly`, which once chose, is retired).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Assembly {
    Transcript,
    #[default]
    View,
}

/// One turn's view, as stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewRecord {
    /// The compilation record this view was built for.
    pub compilation: String,
    /// Which strategy built the request (always `View` for a record written; kept so a reader can say).
    pub assembly: Assembly,
    pub pov: PointOfView,
    pub shown: Vec<Shown>,
    pub dropped: Vec<Dropped>,
    pub residue: Vec<Residue>,
    pub tokens: u64,
    /// The room the turn made when its request passed the budget (`turn::view_step::room`); none when it
    /// made none. A record written before it reads as none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room: Option<Room>,
    /// What a replay needs beyond the session, by blob digest (`compiler::view::kept`); none for a view
    /// built before views kept it, and for a view no request was sent from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kept: Option<crate::compiler::view::kept::Kept>,
}

/// What a turn did to make room in its request (`turn::view_step::room`): numbers, and counts of its own
/// results, never what a result held.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Room {
    /// Times the turn made room.
    pub made: u32,
    /// The results its newest chain's clearing pass turned into their one-line stand-ins.
    pub cleared: u64,
    /// The newest results kept whole, in tokens, by the clearing pass every new chain of the turn runs
    /// whatever its line; none: the line holds.
    pub keep: Option<u64>,
    /// Slot G's allowance (the window and the ladder), in tokens, when it was lowered.
    pub here: Option<u64>,
    /// What slot G held when the view was last assembled, in tokens.
    pub held: u64,
    /// The window a provider stated when it refused a request of the turn for its size, read at Theseus's
    /// estimate.
    pub window: Option<u64>,
    /// This assembly reserves what the turn's requests carry beyond the view (slot G is being lowered, after
    /// clearing); the turn's own working state, not recorded.
    #[serde(skip)]
    pub reserve: bool,
    /// This assembly is the floor alone (the last before an overage); not recorded.
    #[serde(skip)]
    pub floor_only: bool,
    /// This floor is bare: its window exchanges are cut too (after a provider's size refusal); not recorded.
    #[serde(skip)]
    pub bare: bool,
}

impl Room {
    /// The room as a record says it, with the results the newest pass cleared; none when none was made.
    pub fn made(self, cleared: u64) -> Option<Self> {
        (self.made > 0).then_some(Self { cleared, ..self })
    }
}

impl ViewRecord {
    pub fn new(compilation: &str, view: View) -> Self {
        Self {
            compilation: compilation.into(),
            assembly: Assembly::View,
            pov: view.pov,
            shown: view.shown,
            dropped: view.dropped,
            residue: view.residue,
            tokens: view.tokens,
            room: None,
            kept: None,
        }
    }

    /// Its store key.
    pub fn key(&self) -> String {
        format!("{VIEW_PREFIX}{}", self.compilation)
    }
}

/// The record to write (written into a frame the turn already writes).
pub fn view_record(v: &ViewRecord) -> anyhow::Result<NewRecord> {
    NewRecord::json(kinds::META, Some(&v.key()), v)
}

/// The entry that finds a turn's views: its value names the compilation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewOfTurn {
    pub compilation: String,
}

/// The index entry for `turn`'s view of `compilation`, kept at `at` ([`kept_at`]): inverted, so the newest
/// is the first a prefix read meets.
pub fn turn_record(turn: &str, compilation: &str, at: u64) -> anyhow::Result<NewRecord> {
    let key = format!(
        "{VIEW_TURN_PREFIX}{turn}{SEP}{:020}{SEP}{compilation}",
        u64::MAX - at
    );
    let of = ViewOfTurn {
        compilation: compilation.into(),
    };
    NewRecord::json(kinds::META, Some(&key), &of)
}

/// Keep what a turn was shown: the view record, its turn's entry and its counts, deferred into the turn's
/// next frame. A turn or compilation id holding the key's separator is refused, and nothing is kept.
pub fn keep(store: &Store, turn: &str, compilation: &str, view: &View) -> anyhow::Result<()> {
    keep_made(store, turn, compilation, view, (None, None))
}

/// [`keep`], with the room the turn made and what a replay needs, if any: the compiler calls it once for
/// each view it builds, with the turn's own store handle.
pub fn keep_made(
    store: &Store,
    turn: &str,
    compilation: &str,
    view: &View,
    (room, kept): (Option<Room>, Option<crate::compiler::view::kept::Kept>),
) -> anyhow::Result<()> {
    if turn.contains(SEP) || compilation.contains(SEP) {
        anyhow::bail!("a view is not kept under an id holding the key's separator");
    }
    let record = ViewRecord {
        room,
        kept,
        ..ViewRecord::new(compilation, view.clone())
    };
    let tally = Tally::of(&record);
    store.defer(view_record(&record)?)?;
    store.defer(turn_record(turn, compilation, kept_at())?)?;
    store.defer(NewRecord::json(
        kinds::META,
        Some(&format!("{VIEW_TALLY_PREFIX}{compilation}")),
        &tally,
    )?)
}

/// The order a view is kept in, as a time in microseconds: later for a later keep, never the same twice in
/// one process.
fn kept_at() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = theseus_protocol::now_unix_ms().saturating_mul(1_000);
    let mut last = LAST.load(Ordering::Relaxed);
    loop {
        let next = now.max(last + 1);
        match LAST.compare_exchange_weak(last, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return next,
            Err(seen) => last = seen,
        }
    }
}

/// One view, by its compilation.
pub fn read(store: &Store, compilation: &str) -> anyhow::Result<Option<ViewRecord>> {
    store.get_meta(&format!("{VIEW_PREFIX}{compilation}"))
}

/// The compilations of `turn`'s newest `n` views, newest first: one bounded read.
fn newest_of(store: &Store, turn: &str, n: usize) -> anyhow::Result<Vec<String>> {
    let prefix = format!("{VIEW_TURN_PREFIX}{turn}{SEP}");
    store
        .inner()
        .first_with_prefix(kinds::META, &prefix, n)?
        .into_iter()
        .map(|r| r.decode::<ViewOfTurn>().map(|o| o.compilation))
        .collect()
}

/// The view, if it reads; one that cannot is left out with a warning (the error).
fn readable(store: &Store, turn: &str, compilation: &str) -> Result<Option<ViewRecord>, ()> {
    read(store, compilation).map_err(|e| {
        tracing::warn!(turn, compilation, error = %format!("{e:#}"),
            "a view record cannot be read; it is left out");
    })
}

/// A turn's newest view: the one its last compile was given.
pub fn newest(store: &Store, turn: &str) -> anyhow::Result<Option<ViewRecord>> {
    Ok(newest_of(store, turn, 1)?
        .first()
        .and_then(|c| readable(store, turn, c).ok().flatten()))
}

/// A turn's newest [`VIEWS_READ`] views, in the order they were built.
pub fn of_turn(store: &Store, turn: &str) -> anyhow::Result<Vec<ViewRecord>> {
    Ok(of_turn_unread(store, turn)?.0)
}

/// [`of_turn`], and the compilations of those of the views this build cannot read (a later build's names),
/// each left out with a warning: both in the order built.
pub fn of_turn_unread(store: &Store, turn: &str) -> anyhow::Result<(Vec<ViewRecord>, Vec<String>)> {
    let (mut out, mut unread) = (Vec::new(), Vec::new());
    for c in newest_of(store, turn, VIEWS_READ)? {
        match readable(store, turn, &c) {
            Ok(v) => out.extend(v),
            Err(()) => unread.push(c),
        }
    }
    out.reverse();
    unread.reverse();
    Ok((out, unread))
}

/// A view's counts.
pub fn tally(store: &Store, compilation: &str) -> anyhow::Result<Option<Tally>> {
    store.get_meta(&format!("{VIEW_TALLY_PREFIX}{compilation}"))
}

/// How many pieces, and how many tokens.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Count {
    pub pieces: u64,
    pub tokens: u64,
}

impl Count {
    fn add(&mut self, tokens: u64) {
        self.pieces += 1;
        self.tokens += tokens;
    }
}

/// A view counted by slot, by reason and by why a piece was dropped, so which selector earns its tokens can
/// be summed over turns. A piece with two reasons counts under each; a slot's count is each piece once. Ids
/// and counts only; read by the owner in person alone (`theseus view show TURN --tally`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tally {
    pub compilation: String,
    pub present: super::Present,
    pub slots: BTreeMap<Slot, Count>,
    pub reasons: BTreeMap<Reason, Count>,
    /// By why, as its name (`merged`, `over_budget`, `unit_cut`).
    pub dropped: BTreeMap<String, Count>,
    pub dropped_reasons: BTreeMap<Reason, Count>,
    pub tokens: u64,
}

impl Tally {
    pub fn of(v: &ViewRecord) -> Self {
        let mut t = Self {
            compilation: v.compilation.clone(),
            present: v.pov.present,
            slots: BTreeMap::new(),
            reasons: BTreeMap::new(),
            dropped: BTreeMap::new(),
            dropped_reasons: BTreeMap::new(),
            tokens: v.tokens,
        };
        for s in &v.shown {
            t.slots.entry(s.piece.slot).or_default().add(s.tokens);
            for r in &s.piece.reasons {
                t.reasons.entry(*r).or_default().add(s.tokens);
            }
        }
        for d in &v.dropped {
            t.dropped
                .entry(show::name(&d.why))
                .or_default()
                .add(d.tokens);
            for r in &d.reasons {
                t.dropped_reasons.entry(*r).or_default().add(d.tokens);
            }
        }
        t
    }
}
