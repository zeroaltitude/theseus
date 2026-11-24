//! A turn's selection, end to end: one selector per row of [`ADMISSION`], each run once from the turn's
//! one snapshot, every piece through the gate (or, for code's own text, [`Admitted::code`]), then the
//! budgeter. Slots A and I are laid by code: [`Fixed`] carries their parts and sizes as the compiler has
//! them.

use std::collections::BTreeSet;

use super::super::budget::{fit, unit_of, Budget, Overflow, View};
use super::super::gate::{Admitted, Gate};
use super::super::{
    Audience, Author, Form, Handle, Marks, Piece, PointOfView, Pos, Reason, Sizes, Slot,
    SourceRange, Space,
};
use super::beside::Beside;
use super::elsewhere::Elsewhere;
use super::here::Here;
use super::sources::Sources;
use super::{Band, Selector, Snapshot, ADMISSION};

/// A part of slot A or I, laid by code: its name and its size by today's estimate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    pub name: String,
    pub tokens: u64,
}

/// The selector of slot A (`Frame`) or I (`Now`): code's own text, one piece per part.
pub struct Fixed {
    band: Band,
    parts: Vec<Part>,
}

impl Fixed {
    pub fn new(band: Band, parts: Vec<Part>) -> Self {
        debug_assert!(matches!(band, Band::Frame | Band::Now));
        Self { band, parts }
    }
}

impl Selector for Fixed {
    fn band(&self) -> Band {
        self.band
    }

    fn select(&self, pov: &PointOfView, _: &dyn Snapshot) -> anyhow::Result<Vec<Piece>> {
        Ok(self
            .parts
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let pos = Pos {
                    at_ms: pov.now_ms,
                    id: format!("{i:04}"),
                };
                Piece {
                    range: SourceRange {
                        space: Space::Code {
                            part: p.name.clone(),
                        },
                        first: pos.clone(),
                        last: pos,
                    },
                    band: self.band,
                    slot: self.band.row().slot,
                    reasons: BTreeSet::from([Reason::Fixed]),
                    form: Form::Whole,
                    marks: Marks {
                        place: None,
                        audience: Audience::Private,
                        author: Author::Theseus,
                        outside: false,
                        sealed: false,
                        veiled: false,
                    },
                    sizes: Sizes {
                        whole: p.tokens,
                        spoken: p.tokens,
                        line: p.tokens,
                    },
                    handle: Handle(format!("code:{}", p.name)),
                    unit: None,
                }
            })
            .collect())
    }
}

/// One selector per row of [`ADMISSION`], in its order. `frame` and `now` are slots A and I's parts. A
/// retired band has no row, so it has no selector.
pub fn table<'a>(
    src: Sources<'a>,
    frame: Vec<Part>,
    now: Vec<Part>,
) -> Vec<Box<dyn Selector + 'a>> {
    let (mut frame, mut now) = (Some(frame), Some(now));
    ADMISSION
        .iter()
        .filter_map(|row| -> Option<Box<dyn Selector + 'a>> {
            Some(match row.band {
                Band::Frame => Box::new(Fixed::new(Band::Frame, frame.take().unwrap_or_default())),
                Band::Now => Box::new(Fixed::new(Band::Now, now.take().unwrap_or_default())),
                b @ (Band::Floor | Band::WindowWords | Band::WindowTools | Band::Ladder) => {
                    Box::new(Here::new(b, src))
                }
                b @ (Band::ElsewhereWords | Band::ElsewhereReplies) => {
                    Box::new(Elsewhere::new(b, src))
                }
                b @ (Band::Waiting
                | Band::WhoAndWhere
                | Band::Reached
                | Band::TaskCard
                | Band::Named
                | Band::InPlay
                | Band::Recall) => Box::new(Beside::new(b, src)),
                Band::Retired => return None,
            })
        })
        .collect()
}

/// A turn's assembly, and how many pieces were left out as programming faults: a stored reference that
/// claimed code's own space ([`Space::Code`]) from any selector but slot A's and I's. A fault is never shown
/// and never counted in the view; it is logged, and counted here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assembled {
    pub view: Result<View, Overflow>,
    pub faults: u32,
}

/// Run every selector once, gate every piece, and fit; every band runs, whoever is present (2026-11-22). The
/// outer error is a source's read; the inner one is rule 9's overflow.
pub fn assemble(
    pov: &PointOfView,
    snap: &dyn Snapshot,
    selectors: &[Box<dyn Selector + '_>],
    gate: &dyn Gate,
    budget: &Budget,
) -> anyhow::Result<Result<View, Overflow>> {
    assemble_counted(pov, snap, selectors, gate, budget).map(|a| a.view)
}

/// [`assemble`], with its count of faults.
///
/// - Code's own text ([`Space::Code`]) is admitted without the gate ONLY from the selectors of slots A and
///   I ([`Fixed`]'s bands); from any other selector it is a fault.
/// - Every other piece loses its authority (`make::retrieved`, the one place that does it) and goes
///   through the gate.
/// - A unit (a stale fact and its correction) with any member the gate denies is left out whole, before
///   the budgeter: no residue, no dropped entry.
pub fn assemble_counted(
    pov: &PointOfView,
    snap: &dyn Snapshot,
    selectors: &[Box<dyn Selector + '_>],
    gate: &dyn Gate,
    budget: &Budget,
) -> anyhow::Result<Assembled> {
    let mut admitted = Vec::new();
    let mut denied_units = BTreeSet::new();
    let mut faults = 0u32;
    for s in selectors {
        // A self-loop session's turn lays its own place only (a scope of the self loop, `Gate::own_place_only`):
        // nothing waiting (C), nothing elsewhere (F).
        let scoped_out = matches!(s.band().row().slot, Slot::Waiting | Slot::Elsewhere);
        if gate.own_place_only() && scoped_out {
            continue;
        }
        let laid_by_code = matches!(s.band(), Band::Frame | Band::Now);
        for piece in s.select(pov, snap)? {
            if let Space::Code { part } = &piece.range.space {
                if laid_by_code {
                    admitted.extend(Admitted::code(piece));
                } else {
                    tracing::error!(band = ?s.band(), part = %part, "a stored piece claimed code's own text; left out");
                    faults = faults.saturating_add(1);
                }
                continue;
            }
            let piece = super::make::retrieved(piece);
            let unit = unit_of(&piece).map(str::to_owned);
            match gate.admit(pov, piece) {
                Some(a) => admitted.push(a),
                None => denied_units.extend(unit),
            }
        }
    }
    admitted.retain(|a| unit_of(a.get()).is_none_or(|u| !denied_units.contains(u)));
    Ok(Assembled {
        view: fit(pov, admitted, budget),
        faults,
    })
}
