//! The view record: what one turn was shown and why: every piece by reference with its slot, reasons, form
//! and tokens, and what was dropped and why. A record of its own, keyed by today's compilation record (a META
//! key, so no field on an existing record and no store-format step). Read by the renderer, the replay and
//! `theseus view show <turn>` (the record work's).

use serde::{Deserialize, Serialize};
use theseus_store::{kinds, NewRecord};

use super::budget::{Dropped, Residue, Shown, View};
use super::PointOfView;
use crate::config::assembly::Assembly;

/// `view.<compilation id>`.
pub const VIEW_PREFIX: &str = "view.";

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
