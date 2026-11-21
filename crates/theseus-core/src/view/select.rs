//! The selectors and the admission table (trim item 2: the order is a TABLE, and the tests iterate it). A row
//! is one band of rule 6's admission order: its slot, whether it is a floor (never cut), and the one
//! [`Selector`] that fills it. A row's priority is its index in [`ADMISSION`]: over the ceiling, cut from the
//! end. In company, the bands of slots C and F are absent ([`Band::in_company`]).

use serde::{Deserialize, Serialize};

use super::{Piece, PointOfView, Slot, Snapshot};

mod beside;
mod elsewhere;
mod here;
mod make;
mod past;
pub mod run;
pub mod sources;
#[cfg(test)]
mod tests;

pub use beside::{Beside, IN_PLAY_EXCHANGES};
pub use elsewhere::{Elsewhere, LATELY_MS, PLACES_ALWAYS};
pub use here::{Here, FLOOR_EXCHANGES};
pub use make::UNSUMMARISED;
pub use past::Past;
pub use run::{assemble, table, Fixed, Part};

/// One band of the admission order; each has exactly one selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Band {
    /// A, fixed, counted first.
    Frame,
    /// I: the clock, the branch line, the ticker, his message.
    Now,
    /// G: the words of the newest three exchanges in the window.
    Floor,
    /// G: the rest of the window's words.
    WindowWords,
    /// F: his own words elsewhere in the last 48 hours.
    ElsewhereWords,
    /// C.
    Waiting,
    /// E: his words up to 7 days.
    Past7,
    /// B.
    WhoAndWhere,
    /// G: the ladder, newest nodes first.
    Ladder,
    /// G: the window's tool results (thinned by today's clearing pass, trim item 14).
    WindowTools,
    /// H: a reached exchange.
    Reached,
    /// H: a joined task's card.
    TaskCard,
    /// H: named things.
    Named,
    /// D.
    InPlay,
    /// F: Theseus's replies elsewhere.
    ElsewhereReplies,
    /// E: 7 to 30 days.
    Past30,
    /// E: beyond 30 days.
    PastOlder,
    /// H: recall.
    Recall,
}

impl Band {
    /// Whether the band is laid when the place is in company (a channel, or an agent's session).
    pub fn in_company(self) -> bool {
        !matches!(self.row().slot, Slot::Waiting | Slot::Elsewhere)
    }

    /// This band's row of [`ADMISSION`].
    pub fn row(self) -> &'static Row {
        ADMISSION
            .iter()
            .find(|r| r.band == self)
            .expect("every band has a row; tests_view holds it")
    }

    /// Its priority: the index of its row (0 is admitted first and cut last).
    pub fn priority(self) -> usize {
        ADMISSION
            .iter()
            .position(|r| r.band == self)
            .unwrap_or(usize::MAX)
    }
}

/// One row of the admission table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    pub band: Band,
    pub slot: Slot,
    /// A floor is never cut (rule 8): slot I and the window's floor.
    pub floor: bool,
}

const fn row(band: Band, slot: Slot, floor: bool) -> Row {
    Row { band, slot, floor }
}

/// Rule 6's admission order, as data. Within one row: newest first, then by id. Frame is counted first and is
/// never cut either (it is the compiler's, as built); it is not a floor because it is not chosen.
pub const ADMISSION: &[Row] = &[
    row(Band::Frame, Slot::Frame, false),
    row(Band::Now, Slot::Now, true),
    row(Band::Floor, Slot::Here, true),
    row(Band::WindowWords, Slot::Here, false),
    row(Band::ElsewhereWords, Slot::Elsewhere, false),
    row(Band::Waiting, Slot::Waiting, false),
    row(Band::Past7, Slot::Past, false),
    row(Band::WhoAndWhere, Slot::WhoAndWhere, false),
    row(Band::Ladder, Slot::Here, false),
    row(Band::WindowTools, Slot::Here, false),
    row(Band::Reached, Slot::Called, false),
    row(Band::TaskCard, Slot::Called, false),
    row(Band::Named, Slot::Called, false),
    row(Band::InPlay, Slot::InPlay, false),
    row(Band::ElsewhereReplies, Slot::Elsewhere, false),
    row(Band::Past30, Slot::Past, false),
    row(Band::PastOlder, Slot::Past, false),
    row(Band::Recall, Slot::Called, false),
];

/// A selection rule: `(point of view, snapshot) -> pieces`. It names the source it wants, why (each piece's
/// reasons) and in which form; it does not render and knows no other selector. Runs once per turn, on the
/// first call, from the turn's one snapshot. Implemented by the assembler, one per [`Band`].
pub trait Selector {
    /// The row it fills.
    fn band(&self) -> Band;
    /// Its pieces, before the gate.
    fn select(&self, pov: &PointOfView, snap: &dyn Snapshot) -> anyhow::Result<Vec<Piece>>;
}
