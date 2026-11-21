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
use crate::config::assembly::Assembly;
use crate::store::Store;

pub mod show;
#[cfg(test)]
pub(crate) mod tests;
mod text;

pub use text::StoreText;

/// `view.<compilation id>`.
pub const VIEW_PREFIX: &str = "view.";
/// `view_turn.<turn id>\u{1}<20-digit u64::MAX - ms>\u{1}<compilation id>`: a turn's views, newest first.
/// Within one millisecond the order is by compilation id (descending), not by the order of the calls.
pub const VIEW_TURN_PREFIX: &str = "view_turn.";
/// `view_tally.<compilation id>`: a view's counts ([`Tally`]).
pub const VIEW_TALLY_PREFIX: &str = "view_tally.";
/// Between a turn index key's parts; no id may hold it ([`keep`] refuses one that does).
pub const SEP: char = '\u{1}';
/// The most views of one turn [`of_turn`] reads: the newest.
pub const VIEWS_READ: usize = 32;

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

/// The index entry for `turn`'s view of `compilation`, built at `at_ms`: time inverted, so the newest is
/// the first a prefix read meets.
pub fn turn_record(turn: &str, compilation: &str, at_ms: u64) -> anyhow::Result<NewRecord> {
    let key = format!(
        "{VIEW_TURN_PREFIX}{turn}{SEP}{:020}{SEP}{compilation}",
        u64::MAX - at_ms
    );
    let of = ViewOfTurn {
        compilation: compilation.into(),
    };
    NewRecord::json(kinds::META, Some(&key), &of)
}

/// Keep what a turn was shown: the view record, its turn's entry and its counts, deferred into the turn's
/// next frame. The compiler calls it once for each view it builds, with the turn's own store handle. A turn
/// or compilation id holding the key's separator is refused, and nothing is kept.
pub fn keep(store: &Store, turn: &str, compilation: &str, view: &View) -> anyhow::Result<()> {
    if turn.contains(SEP) || compilation.contains(SEP) {
        anyhow::bail!("a view is not kept under an id holding the key's separator");
    }
    let record = ViewRecord::new(compilation, view.clone());
    let tally = Tally::of(&record);
    store.defer(view_record(&record)?)?;
    store.defer(turn_record(turn, compilation, view.pov.now_ms)?)?;
    store.defer(NewRecord::json(
        kinds::META,
        Some(&format!("{VIEW_TALLY_PREFIX}{compilation}")),
        &tally,
    )?)
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

/// The view, if it reads; one that cannot is left out with a warning.
fn readable(store: &Store, turn: &str, compilation: &str) -> Option<ViewRecord> {
    match read(store, compilation) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(turn, compilation, error = %format!("{e:#}"),
                "a view record cannot be read; it is left out");
            None
        }
    }
}

/// A turn's newest view: the one its last compile was given.
pub fn newest(store: &Store, turn: &str) -> anyhow::Result<Option<ViewRecord>> {
    Ok(newest_of(store, turn, 1)?
        .first()
        .and_then(|c| readable(store, turn, c)))
}

/// A turn's newest [`VIEWS_READ`] views, in the order they were built.
pub fn of_turn(store: &Store, turn: &str) -> anyhow::Result<Vec<ViewRecord>> {
    let mut out: Vec<ViewRecord> = newest_of(store, turn, VIEWS_READ)?
        .iter()
        .filter_map(|c| readable(store, turn, c))
        .collect();
    out.reverse();
    Ok(out)
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
