//! The budgeter: one function, [`fit`], and these nine rules are all of them (design §5.1; trim items 1, 13).
//!
//! 1. Each selector names the source it wants, why, and in which form; selectors do not render.
//! 2. The gate runs first: [`fit`] takes only [`Admitted`] pieces; what was denied is not counted, ranked or
//!    mentioned.
//! 3. Identical or overlapping ranges ([`super::SourceRange::overlaps`]) merge, keeping every reason and the
//!    fullest form asked for, shown once: in slot G if the window or ladder asked, else in the first row of
//!    [`super::select::ADMISSION`] that asked.
//! 4. A stale fact and its correction (one [`super::Piece::unit`]) are admitted together or not at all: a
//!    unit with a member the gate denies is left out whole before the budgeter
//!    ([`super::select::assemble`]), and a unit is cut whole here. A window exchange is never a unit.
//! 5. One budget, [`Budget::total`]: the smaller of the ceiling plus slot G's allowance and the provider's
//!    window, less the reserve for the reply and the turn's own calls. Everything is charged to it, headings
//!    and residue lines included. Sizes are today's estimate. Only never-cut text may stand over its own
//!    pool (see [`fit`]). Inside the ceiling, the topic block (slot H) is held to its own cap,
//!    [`Budget::topic`]: over it, the block is cut from its own end, recall first.
//! 6. Admission follows [`super::select::ADMISSION`]; within a row, newest first, then by id.
//! 7. Over budget, cut from the end of that order; a piece steps down a form (`Whole → Spoken → Line`)
//!    before it is dropped, and what is dropped is counted in its band's [`Residue`].
//! 8. A floor row ([`super::select::Row::floor`]) is never cut. A cut elsewhere is marked and carries a
//!    handle; it never breaks a tool call from its result, and never rewrites the owner's words.
//! 9. If the floor alone cannot fit the provider's window, the turn does not answer from a shortened view:
//!    [`Overflow::Floor`] names what did not fit and its handle.
//!
//! Rules 3 and 4 are [`merge`]'s; rules 5, 7 and 8 are [`cut`]'s. Nothing here depends on the order the
//! pieces arrive in: they are put in one canonical order first.

use serde::{Deserialize, Serialize};

use super::gate::Admitted;
use super::select::Band;
use super::{Form, Handle, Piece, PointOfView, Reasons, Slot, SourceRange};

mod cut;
mod merge;
#[cfg(test)]
mod tests;

/// A count of the budgeter's inner operations, for the test that holds it linear; nothing outside tests.
pub(super) mod ops {
    #[cfg(test)]
    thread_local!(static OPS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) });

    #[cfg(test)]
    pub fn tick(n: u64) {
        OPS.with(|c| c.set(c.get() + n));
    }

    #[cfg(not(test))]
    #[inline(always)]
    pub fn tick(_: u64) {}

    /// The count so far on this thread, and reset it.
    #[cfg(test)]
    pub fn take() -> u64 {
        OPS.with(|c| c.replace(0))
    }
}

/// The one ceiling, in tokens, over everything but slot G (trim item 1).
pub const CEILING: u64 = 60_000;
/// Slot G's own allowance (the window and the ladder), outside the ceiling.
pub const HERE_ALLOWANCE: u64 = 150_000;
/// The topic block's own cap (slot H: the task's card, the things named and in play, then recall), inside the
/// ceiling; `[context] topic_max_tokens` sets it.
pub const TOPIC_MAX_TOKENS: u64 = 8_000;
/// What a shown piece's heading and handle cost when rendered, charged to every shown piece (a conservative
/// bound; the renderer's own headings decide the real number). At least [`RESIDUE_LINE_TOKENS`], so a drop
/// never adds tokens.
pub const HEADING_TOKENS: u64 = 40;
/// What a band's residue line (its count of what was left out, and a handle) costs when rendered, charged
/// once for each band that leaves one.
pub const RESIDUE_LINE_TOKENS: u64 = 40;

/// The numbers rule 5 takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Budget {
    pub ceiling: u64,
    pub here_allowance: u64,
    /// The answering model's window (or `[context] working_window_tokens`, lower).
    pub provider_window: u64,
    /// Room kept for the reply and the turn's own tool calls.
    pub reserve: u64,
    /// The floor alone (`turn::view_step::room`'s last render before an overage): with the ceiling and slot
    /// G's allowance at none, every piece that may be cut is; the whole is the window less the reserve.
    #[serde(default)]
    pub floor_only: bool,
    /// The floor's window exchanges are cut like the rest of slot G (`turn::view_step::room`'s barest render,
    /// after a provider refused a request of the turn for its size and the floor was no smaller): only the
    /// turn's message and the fixed blocks stand.
    #[serde(default)]
    pub bare: bool,
    /// The topic block's own cap, inside the ceiling (the view's slot H).
    #[serde(default = "topic_max_tokens")]
    pub topic: u64,
}

fn topic_max_tokens() -> u64 {
    TOPIC_MAX_TOKENS
}

impl Budget {
    /// The design's numbers for a provider window and reserve.
    pub fn new(provider_window: u64, reserve: u64) -> Self {
        Self {
            ceiling: CEILING,
            here_allowance: HERE_ALLOWANCE,
            provider_window,
            reserve,
            floor_only: false,
            bare: false,
            topic: TOPIC_MAX_TOKENS,
        }
    }

    /// Rule 5: the one budget everything is charged to.
    pub fn total(&self) -> u64 {
        let pools = match self.floor_only {
            true => self.provider_window,
            false => (self.ceiling + self.here_allowance).min(self.provider_window),
        };
        pools.saturating_sub(self.reserve)
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
    /// A recalled passage the request already carries with its words, in an exchange laid whole or spoken
    /// ([`set_aside`], issue 44): it is there, so it is not counted as left out.
    InWindow,
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

/// Fit admitted pieces into one budget: the nine rules of the module comment.
///
/// What is charged is what will be rendered: each shown piece's form plus [`HEADING_TOKENS`], and
/// [`RESIDUE_LINE_TOKENS`] for each band that leaves a residue line; `View.tokens` is that sum.
///
/// The three sums of rule 5 have ONE exception: text that is never cut (the frame and the floor) may
/// stand over its own pool (the ceiling, or slot G's allowance) as long as the whole fits the one budget.
/// Everything else is held to its own pool and to the whole. The typed overflow is for the whole only:
/// when what cannot be cut does not fit [`Budget::total`].
pub fn fit(
    pov: &PointOfView,
    pieces: Vec<Admitted<Piece>>,
    budget: &Budget,
) -> Result<View, Overflow> {
    let bared = |mut p: Piece| {
        if budget.bare && p.band == Band::Floor {
            p.band = Band::WindowWords;
        }
        p
    };
    let items = merge::merge(
        pieces
            .into_iter()
            .map(Admitted::into_inner)
            .map(bared)
            .collect(),
    );
    let (groups, members) = merge::unit_groups(&items);
    let done = cut::cut(&items, &groups, &members, budget);
    let room = budget.total();
    let tokens = match u64::try_from(done.tokens) {
        Ok(t) if t <= room => t,
        _ => return Err(overflow(&items, &done.fate, done.tokens, room)),
    };
    let mut shown = Vec::new();
    let mut dropped = Vec::new();
    for (it, f) in items.into_iter().zip(done.fate) {
        dropped.extend(it.merged.iter().map(|q| drop_of(q, DropWhy::Merged)));
        match f {
            cut::Fate::Shown(form) => shown.push(Shown {
                tokens: it.piece.sizes.at(form).saturating_add(HEADING_TOKENS),
                form,
                piece: it.piece,
            }),
            cut::Fate::OverBudget => dropped.push(drop_of(&it.piece, DropWhy::OverBudget)),
            cut::Fate::UnitCut => dropped.push(drop_of(&it.piece, DropWhy::UnitCut)),
        }
    }
    shown.sort_by_cached_key(|s| order(&s.piece));
    dropped.sort_by_cached_key(|d| {
        (
            d.band.priority(),
            std::cmp::Reverse(d.range.last.clone()),
            d.handle.clone(),
            d.why as u8,
        )
    });
    Ok(View {
        pov: pov.clone(),
        tokens,
        residue: residue(&dropped),
        shown,
        dropped,
    })
}

/// Slot H's backstop (issue 44): a recalled piece whose node is among `carried` (what the shown exchanges
/// lay with their words, `TextRenderer::carried`) is set aside as [`DropWhy::InWindow`]: one copy, the
/// window's. The residue is not changed: nothing was left out.
pub fn set_aside(view: &mut View, carried: &std::collections::BTreeSet<String>) {
    let held = |p: &Piece| matches!(&p.range.space, super::Space::Recalled { node, .. } if carried.contains(node));
    let (aside, shown): (Vec<Shown>, Vec<Shown>) = std::mem::take(&mut view.shown)
        .into_iter()
        .partition(|s| held(&s.piece));
    view.shown = shown;
    view.dropped
        .extend(aside.iter().map(|s| drop_of(&s.piece, DropWhy::InWindow)));
}

/// A piece's unit (rule 4): a current fact tied to its correction. A window exchange is history, never a
/// unit, whatever its source says.
pub(in crate::view) fn unit_of(p: &Piece) -> Option<&str> {
    match p.range.space {
        super::Space::Exchanges { .. } => None,
        _ => p.unit.as_deref(),
    }
}

/// Rule 6's order for what is shown: the row, newest first, then by handle.
fn order(p: &Piece) -> (usize, std::cmp::Reverse<super::Pos>, Handle) {
    (
        p.band.priority(),
        std::cmp::Reverse(p.range.last.clone()),
        p.handle.clone(),
    )
}

fn drop_of(p: &Piece, why: DropWhy) -> Dropped {
    Dropped {
        range: p.range.clone(),
        slot: p.slot,
        band: p.band,
        reasons: p.reasons.clone(),
        handle: p.handle.clone(),
        tokens: p.tokens(),
        why,
    }
}

/// Rule 7's count: per band, what was cut (a merged piece is shown, so it is not counted).
fn residue(dropped: &[Dropped]) -> Vec<Residue> {
    let mut out: Vec<Residue> = Vec::new();
    for d in (dropped.iter()).filter(|d| !matches!(d.why, DropWhy::Merged | DropWhy::InWindow)) {
        match out.iter_mut().find(|r| r.band == d.band) {
            Some(r) => {
                r.pieces = r.pieces.saturating_add(1);
                r.tokens = r.tokens.saturating_add(d.tokens);
            }
            None => out.push(Residue {
                band: d.band,
                pieces: 1,
                tokens: d.tokens,
            }),
        }
    }
    out.sort_by_key(|r| r.band.priority());
    out
}

/// Rule 9: what is left after every permitted cut still does not fit; it is all never-cut text. Name the
/// newest piece of the floor (or of what else could not be cut).
fn overflow(items: &[merge::Item], fate: &[cut::Fate], tokens: u128, room: u64) -> Overflow {
    let left = || {
        items
            .iter()
            .zip(fate)
            .filter(|(_, f)| matches!(f, cut::Fate::Shown(_)))
            .map(|(it, _)| &it.piece)
    };
    let first = left()
        .filter(|p| p.band == Band::Floor)
        .min_by_key(|p| order(p))
        .or_else(|| left().min_by_key(|p| order(p)));
    Overflow::Floor {
        handle: first.map_or(Handle(String::new()), |p| p.handle.clone()),
        tokens: u64::try_from(tokens).unwrap_or(u64::MAX),
        room,
    }
}
