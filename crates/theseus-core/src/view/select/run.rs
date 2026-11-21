//! A turn's selection, end to end: one selector per row of [`ADMISSION`], each run once from the turn's
//! one snapshot, every piece through the gate (or, for code's own text, [`Admitted::code`]), then the
//! budgeter. Slots A and I are laid by code: [`Fixed`] carries their parts and sizes as the compiler has
//! them.

use std::collections::BTreeSet;

use super::super::budget::{fit, Budget, Overflow, View};
use super::super::gate::{Admitted, Gate};
use super::super::{
    Audience, Author, Form, Handle, Marks, Piece, PointOfView, Pos, Present, Reason, Sizes,
    SourceRange, Space,
};
use super::beside::Beside;
use super::elsewhere::Elsewhere;
use super::here::Here;
use super::past::Past;
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

/// One selector per row of [`ADMISSION`], in its order. `frame` and `now` are slots A and I's parts.
pub fn table<'a>(
    src: Sources<'a>,
    frame: Vec<Part>,
    now: Vec<Part>,
) -> Vec<Box<dyn Selector + 'a>> {
    let (mut frame, mut now) = (Some(frame), Some(now));
    ADMISSION
        .iter()
        .map(|row| -> Box<dyn Selector + 'a> {
            match row.band {
                Band::Frame => Box::new(Fixed::new(Band::Frame, frame.take().unwrap_or_default())),
                Band::Now => Box::new(Fixed::new(Band::Now, now.take().unwrap_or_default())),
                b @ (Band::Floor | Band::WindowWords | Band::WindowTools | Band::Ladder) => {
                    Box::new(Here::new(b, src))
                }
                b @ (Band::ElsewhereWords | Band::ElsewhereReplies) => {
                    Box::new(Elsewhere::new(b, src))
                }
                b @ (Band::Past7 | Band::Past30 | Band::PastOlder) => Box::new(Past::new(b, src)),
                b @ (Band::Waiting
                | Band::WhoAndWhere
                | Band::Reached
                | Band::TaskCard
                | Band::Named
                | Band::InPlay
                | Band::Recall) => Box::new(Beside::new(b, src)),
            }
        })
        .collect()
}

/// Run every selector once, gate every piece, and fit. A band absent in company (slots C and F) is not run
/// unless the owner is alone. The outer error is a source's read; the inner one is rule 9's overflow.
pub fn assemble(
    pov: &PointOfView,
    snap: &dyn Snapshot,
    selectors: &[Box<dyn Selector + '_>],
    gate: &dyn Gate,
    budget: &Budget,
) -> anyhow::Result<Result<View, Overflow>> {
    let mut admitted = Vec::new();
    for s in selectors {
        if !s.band().in_company() && pov.present != Present::OwnerAlone {
            continue;
        }
        for piece in s.select(pov, snap)? {
            let piece = match piece.range.space {
                Space::Code { .. } => Admitted::code(piece),
                _ => gate.admit(pov, piece),
            };
            admitted.extend(piece);
        }
    }
    Ok(fit(pov, admitted, budget))
}
