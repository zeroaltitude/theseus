//! What a reader of a view record is shown (`view.show`, `theseus view show`, and the model's own look): the
//! point of view, each slot with its pieces, what was dropped and why, and the totals of what is shown. By
//! default ids, kinds, forms, tokens, reasons and labels.
//!
//! A stored record is a map of what WAS admitted, never a permission. Every piece, shown or dropped, passes
//! its [`Source`] again before its reference is shown: today's reading of where it came from (its session's
//! place and seal, through the gate's own functions; `source.rs`), failing closed when that cannot be read.
//! A piece's text is read only after that check, and only through a [`PieceText`] the caller passes (the
//! owner's, at his own terminal).
//!
//! **Nothing counts what is left out.** Totals are of what is shown; a restricted reader is told one fixed
//! sentence ([`RESTRICTED`]) whatever was left out, and is never shown the residue or the record's at-hand
//! reference, so two records that differ only in pieces it may not see read the same, byte for byte.

use serde::Serialize;
use theseus_protocol::view::{
    ViewDrop, ViewPiece, ViewPov, ViewResidue, ViewShown, ViewSlot, ViewTotals,
};

use super::super::{Anchor, AtHand, Handle, Label, Marks, Piece, PointOfView, SourceRange, Space};
use super::ViewRecord;

/// What a restricted reader is told, whatever the record holds.
pub const RESTRICTED: &str = "only what this conversation may read today is listed";

/// The text of a piece, from its source; none when its source holds no text this build reads. Called only
/// for a piece its [`Source`] allowed.
pub trait PieceText {
    fn text(&self, piece: &Piece) -> Option<String>;
}

/// Whether a piece's source may be shown to this reader today.
pub trait Source {
    /// `marks` are the piece's stored marks (a dropped piece has none); a source that cannot be read is no.
    fn may_show(&self, range: &SourceRange, marks: Option<&Marks>) -> bool;
    /// [`Self::may_show`] for a piece found by `handle`: a fold is proven by its node, which its range alone
    /// does not name.
    fn may_show_piece(&self, range: &SourceRange, _handle: &Handle, marks: Option<&Marks>) -> bool {
        self.may_show(range, marks)
    }
    /// Whether the record's at-hand reference may be shown.
    fn at_hand(&self, at_hand: &AtHand) -> bool;
}

/// Who reads a record.
#[derive(Clone, Copy)]
pub struct Reader<'a> {
    pub source: &'a dyn Source,
    /// Each piece's text: given only for the owner at his own terminal.
    pub text: Option<&'a dyn PieceText>,
    /// Not the owner alone: no residue, no at-hand reference, and the [`RESTRICTED`] sentence.
    pub restricted: bool,
}

/// An enumerated value's name, as it is stored.
pub fn name<T: Serialize>(v: &T) -> String {
    match serde_json::to_value(v) {
        Ok(serde_json::Value::String(s)) => s,
        Ok(other) => other.to_string(),
        Err(_) => String::new(),
    }
}

/// The kind of a space, by name.
fn kind_of(space: &Space) -> &'static str {
    match space {
        Space::Exchanges { .. } => "exchanges",
        Space::Folds { .. } => "folds",
        Space::Facts { .. } => "facts",
        Space::Card { .. } => "card",
        Space::Task { .. } => "task",
        Space::Waiting { .. } => "waiting",
        Space::Recalled { .. } => "recalled",
        Space::Code { .. } => "code",
    }
}

impl Reader<'_> {
    /// One record, as this reader is shown it.
    pub fn show(&self, rec: &ViewRecord) -> ViewShown {
        let mut totals = ViewTotals::default();
        let mut slots: Vec<ViewSlot> = Vec::new();
        let mut shown: Vec<_> = rec.shown.iter().collect();
        shown.sort_by_key(|s| s.piece.slot);
        for s in shown {
            let (range, marks) = (&s.piece.range, Some(&s.piece.marks));
            if !self.source.may_show_piece(range, &s.piece.handle, marks) {
                continue;
            }
            let slot = name(&s.piece.slot);
            if slots.last().is_none_or(|l| l.slot != slot) {
                slots.push(ViewSlot {
                    slot,
                    ..ViewSlot::default()
                });
            }
            let last = slots.last_mut().expect("pushed");
            last.tokens += s.tokens;
            totals.shown += 1;
            totals.tokens += s.tokens;
            last.pieces.push(ViewPiece {
                form: name(&s.form),
                tokens: s.tokens,
                label: name(&Label::of(&s.piece, &rec.pov)),
                text: self.text.and_then(|t| t.text(&s.piece)),
                ..piece_ref(&s.piece)
            });
        }
        let mut dropped = Vec::new();
        let mut held: Vec<&super::super::budget::Dropped> = Vec::new();
        for d in &rec.dropped {
            if !self.source.may_show_piece(&d.range, &d.handle, None) {
                held.push(d);
                continue;
            }
            totals.dropped += 1;
            totals.dropped_tokens += d.tokens;
            dropped.push(ViewDrop {
                handle: d.handle.0.clone(),
                kind: kind_of(&d.range.space).into(),
                slot: name(&d.slot),
                band: name(&d.band),
                reasons: d.reasons.iter().map(name).collect(),
                tokens: d.tokens,
                why: name(&d.why),
            });
        }
        ViewShown {
            compilation: rec.compilation.clone(),
            assembly: name(&rec.assembly),
            pov: self.pov_of(&rec.pov),
            slots,
            dropped,
            // A band's residue counts pieces with no reference to check: a restricted reader is not shown it,
            // and no reader the residue of the bands of cards, what waits and the person's facts, whose
            // sources a count cannot re-check.
            residue: match self.restricted {
                false => {
                    let checked: Vec<_> = (rec.residue.iter())
                        .filter(|r| !unchecked(r.band))
                        .cloned()
                        .collect();
                    residue_without(&checked, &held)
                }
                true => Vec::new(),
            },
            totals,
            restricted: self.restricted.then(|| RESTRICTED.into()),
        }
    }

    /// The point of view, with no name in it (whether a person was named, not who), and its at-hand
    /// reference only for a reader who is not restricted and whose source allows it today.
    fn pov_of(&self, p: &PointOfView) -> ViewPov {
        let (anchor_kind, anchor) = match &p.anchor {
            Anchor::Place(a) => ("place", a.clone()),
            Anchor::Task(t) => ("task", t.clone()),
        };
        ViewPov {
            anchor_kind: anchor_kind.into(),
            anchor,
            present: name(&p.present),
            person: p.person.is_some(),
            at_hand: p
                .at_hand
                .as_ref()
                .filter(|a| !self.restricted && self.source.at_hand(a))
                .map(|a| match a {
                    AtHand::LineOfWork(l) => format!("line_of_work {l}"),
                    AtHand::Task(t) => format!("task {t}"),
                }),
            now_ms: p.now_ms,
        }
    }
}

/// A band whose residue is not shown: its pieces' sources are re-checked one by one, never by a count.
fn unchecked(band: super::super::select::Band) -> bool {
    use super::super::select::Band;
    matches!(
        band,
        Band::Named | Band::InPlay | Band::Waiting | Band::WhoAndWhere
    )
}

/// What a piece is, by reference: no text.
fn piece_ref(p: &Piece) -> ViewPiece {
    ViewPiece {
        handle: p.handle.0.clone(),
        kind: kind_of(&p.range.space).into(),
        first: p.range.first.id.clone(),
        last: p.range.last.id.clone(),
        band: name(&p.band),
        form: name(&p.form),
        tokens: p.tokens(),
        reasons: p.reasons.iter().map(name).collect(),
        label: String::new(),
        labels: Vec::new(),
        text: None,
    }
}

/// The record's residue less what `held` (drops this reader may not see) added to it, as the budgeter
/// counted them (a merged piece was never counted): a held piece's count and size are not shown either. A
/// band left with nothing is not shown.
fn residue_without(
    residue: &[super::super::budget::Residue],
    held: &[&super::super::budget::Dropped],
) -> Vec<ViewResidue> {
    use super::super::budget::DropWhy;
    (residue.iter())
        .map(|r| {
            let counted = |d: &&&super::super::budget::Dropped| {
                d.band == r.band && !matches!(d.why, DropWhy::Merged | DropWhy::InWindow)
            };
            let gone = held.iter().filter(counted);
            let (n, t) = gone.fold((0u32, 0u64), |(n, t), d| (n + 1, t + d.tokens));
            super::super::budget::Residue {
                band: r.band,
                pieces: r.pieces.saturating_sub(n),
                tokens: r.tokens.saturating_sub(t),
            }
        })
        .filter(|r| r.pieces > 0)
        .map(|r| residue_of(&r))
        .collect()
}

fn residue_of(r: &super::super::budget::Residue) -> ViewResidue {
    ViewResidue {
        band: name(&r.band),
        pieces: r.pieces,
        tokens: r.tokens,
    }
}
