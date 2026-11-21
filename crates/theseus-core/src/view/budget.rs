//! The budgeter: one function, [`fit`], and these nine rules are all of them (design §5.1; trim items 1, 13).
//!
//! 1. Each selector names the source it wants, why, and in which form; selectors do not render.
//! 2. The gate runs first: [`fit`] takes only [`Admitted`] pieces; what was denied is not counted, ranked or
//!    mentioned.
//! 3. Identical or overlapping ranges ([`super::SourceRange::overlaps`]) merge, keeping every reason and the
//!    fullest form asked for, shown once: in slot G if the window or ladder asked, else in the first row of
//!    [`super::select::ADMISSION`] that asked.
//! 4. A stale fact and its correction (one [`super::Piece::unit`]) are admitted together or not at all.
//! 5. One budget, [`Budget::total`]: the smaller of the ceiling plus slot G's allowance and the provider's
//!    window, less the reserve for the reply and the turn's own calls. Everything is charged to it. Sizes are
//!    today's estimate.
//! 6. Admission follows [`super::select::ADMISSION`]; within a row, newest first, then by id.
//! 7. Over budget, cut from the end of that order; a piece steps down a form (`Whole → Spoken → Line`)
//!    before it is dropped, and what is dropped is counted in its band's [`Residue`].
//! 8. A floor row ([`super::select::Row::floor`]) is never cut. A cut elsewhere is marked and carries a
//!    handle; it never breaks a tool call from its result, and never rewrites the owner's words.
//! 9. If the floor alone cannot fit the provider's window, the turn does not answer from a shortened view:
//!    [`Overflow::Floor`] names what did not fit and its handle.
//!
//! The body of [`fit`] is a SHELL (the contract): it admits every piece in table order with its form as
//! asked, and only rule 9's check. The assembler fills rules 3, 4 and 7; the signature is fixed.

use serde::{Deserialize, Serialize};

use super::gate::Admitted;
use super::select::Band;
use super::{Form, Handle, Piece, PointOfView, Reasons, Slot, SourceRange};

/// The one ceiling, in tokens, over everything but slot G (trim item 1).
pub const CEILING: u64 = 60_000;
/// Slot G's own allowance (the window and the ladder), outside the ceiling.
pub const HERE_ALLOWANCE: u64 = 150_000;

/// The numbers rule 5 takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Budget {
    pub ceiling: u64,
    pub here_allowance: u64,
    /// The answering model's window (or `[context] working_window_tokens`, lower).
    pub provider_window: u64,
    /// Room kept for the reply and the turn's own tool calls.
    pub reserve: u64,
}

impl Budget {
    /// The design's numbers for a provider window and reserve.
    pub fn new(provider_window: u64, reserve: u64) -> Self {
        Self {
            ceiling: CEILING,
            here_allowance: HERE_ALLOWANCE,
            provider_window,
            reserve,
        }
    }

    /// Rule 5: the one budget everything is charged to.
    pub fn total(&self) -> u64 {
        (self.ceiling + self.here_allowance)
            .min(self.provider_window)
            .saturating_sub(self.reserve)
    }
}

/// A piece as the view shows it: the form chosen and the tokens charged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shown {
    pub piece: Piece,
    /// The form shown (at most the form asked for).
    pub form: Form,
    pub tokens: u64,
}

/// Why a piece is not shown. (What the gate denied is not here: rule 2.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DropWhy {
    /// Merged into another piece's range (rule 3).
    Merged,
    /// Cut from the end of the order (rule 7).
    OverBudget,
    /// Its correction's unit was cut (rule 4).
    UnitCut,
}

/// A piece not shown, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dropped {
    pub range: SourceRange,
    pub slot: Slot,
    pub band: Band,
    pub reasons: Reasons,
    pub handle: Handle,
    pub tokens: u64,
    pub why: DropWhy,
}

/// A band's count of what was left out, rendered as its residue line with a handle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Residue {
    pub band: Band,
    pub pieces: u32,
    pub tokens: u64,
}

/// What the budgeter returns: the pieces shown in admission order, and what was left out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct View {
    pub pov: PointOfView,
    pub shown: Vec<Shown>,
    pub dropped: Vec<Dropped>,
    pub residue: Vec<Residue>,
    /// Everything charged, against [`Budget::total`].
    pub tokens: u64,
}

/// The typed overflow (rule 9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Overflow {
    /// The floor's own text does not fit: what did not, its handle, its tokens and the room there was.
    Floor {
        handle: Handle,
        tokens: u64,
        room: u64,
    },
}

/// Fit admitted pieces into one budget. SHELL: see the module comment.
pub fn fit(
    pov: &PointOfView,
    pieces: Vec<Admitted<Piece>>,
    budget: &Budget,
) -> Result<View, Overflow> {
    let mut pieces: Vec<Piece> = pieces.into_iter().map(Admitted::into_inner).collect();
    pieces.sort_by(|a, b| {
        let key = |p: &Piece| (p.band.priority(), std::cmp::Reverse(p.range.last.clone()));
        key(a).cmp(&key(b))
    });
    let room = budget.total();
    let floor: u64 = pieces
        .iter()
        .filter(|p| p.band.row().floor)
        .map(Piece::tokens)
        .sum();
    if floor > room {
        let first = pieces.iter().find(|p| p.band.row().floor);
        return Err(Overflow::Floor {
            handle: first
                .map(|p| p.handle.clone())
                .unwrap_or(Handle(String::new())),
            tokens: floor,
            room,
        });
    }
    let shown: Vec<Shown> = pieces
        .into_iter()
        .map(|piece| Shown {
            form: piece.form,
            tokens: piece.tokens(),
            piece,
        })
        .collect();
    Ok(View {
        pov: pov.clone(),
        tokens: shown.iter().map(|s| s.tokens).sum(),
        shown,
        dropped: Vec::new(),
        residue: Vec::new(),
    })
}
