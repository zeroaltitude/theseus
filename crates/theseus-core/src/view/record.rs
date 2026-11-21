//! The view record: what one turn was shown and why: every piece by reference with its slot, reasons, form
//! and tokens, and what was dropped and why. A record of its own, keyed by today's compilation record (a META
//! key, so no field on an existing record and no store-format step). Read by the renderer, the replay and
//! `theseus view show <turn>` (the record work's).
//!
//! - **The one call the compiler makes**, once per view it builds: [`keep`]`(store, turn, compilation,
//!   &view)`. It writes nothing at once: the record, its turn's index entry and the view's counting row
//!   ([`Tally`], kind `context.view`) ride the turn's next frame, as every row a turn defers does.
//! - **The reads**: [`read`] one view by its compilation, [`of_turn`] a turn's views in the order built.
//! - **What a reader is shown** ([`show`]): ids, kinds, forms, tokens, reasons and counts; text only through
//!   a [`show::PieceText`] the caller passes, which only the owner at his own terminal is given; and, for a
//!   reader in company, only that place's pieces (the rest are counted, not listed).
//!
//! The design note is `docs/design/view-record.md`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use theseus_protocol::LedgerKind;
use theseus_store::{kinds, NewRecord, Store as _};

use super::budget::{Dropped, Residue, Shown, View};
use super::{PointOfView, Reason, Slot};
use crate::config::assembly::Assembly;
use crate::ledger::LedgerRow;
use crate::store::Store;

pub mod show;
#[cfg(test)]
mod tests;
mod text;

pub use text::StoreText;

/// `view.<compilation id>`.
pub const VIEW_PREFIX: &str = "view.";
/// `view_turn.<turn id>\u{1}<20-digit ms>\u{1}<compilation id>`: a turn's views, in the order they were built.
pub const VIEW_TURN_PREFIX: &str = "view_turn.";

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

/// The index entry for `turn`'s view of `compilation`, built at `at_ms`.
pub fn turn_record(turn: &str, compilation: &str, at_ms: u64) -> anyhow::Result<NewRecord> {
    let key = format!("{VIEW_TURN_PREFIX}{turn}\u{1}{at_ms:020}\u{1}{compilation}");
    let of = ViewOfTurn {
        compilation: compilation.into(),
    };
    NewRecord::json(kinds::META, Some(&key), &of)
}

/// Keep what a turn was shown: the view record, its turn's entry and its counting row, deferred into the
/// turn's next frame. The compiler calls it once for each view it builds.
pub fn keep(store: &Store, turn: &str, compilation: &str, view: &View) -> anyhow::Result<()> {
    let record = ViewRecord::new(compilation, view.clone());
    let tally = Tally::of(&record);
    store.defer(view_record(&record)?)?;
    store.defer(turn_record(turn, compilation, view.pov.now_ms)?)?;
    let row = LedgerRow::new(
        LedgerKind::ContextView,
        None,
        Some(turn),
        serde_json::to_value(&tally)?,
    );
    store.defer(NewRecord::json(kinds::LEDGER, None, &row)?)
}

/// One view, by its compilation.
pub fn read(store: &Store, compilation: &str) -> anyhow::Result<Option<ViewRecord>> {
    store.get_meta(&format!("{VIEW_PREFIX}{compilation}"))
}

/// A turn's views, in the order they were built. A view whose record cannot be read is left out, with a
/// warning: one bad record does not hide the turn's others.
pub fn of_turn(store: &Store, turn: &str) -> anyhow::Result<Vec<ViewRecord>> {
    let prefix = format!("{VIEW_TURN_PREFIX}{turn}\u{1}");
    let mut out = Vec::new();
    for r in store.inner().latest_with_prefix(kinds::META, &prefix)? {
        let of: ViewOfTurn = r.decode()?;
        match read(store, &of.compilation) {
            Ok(Some(v)) => out.push(v),
            Ok(None) => {}
            Err(e) => {
                tracing::warn!(turn, compilation = %of.compilation, error = %format!("{e:#}"),
                "a view record cannot be read; the turn's other views are shown")
            }
        }
    }
    Ok(out)
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

/// A view counted by slot, by reason and by why a piece was dropped: the `context.view` row, so which
/// selector earns its tokens can be summed over turns. A piece with two reasons counts under each; a slot's
/// count is each piece once. Ids and counts only.
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
