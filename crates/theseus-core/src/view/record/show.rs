//! What a reader of a view record is shown (`view.show`, `theseus view show`, and the model's own look):
//! the point of view, each slot with its pieces, what was dropped and why, and the totals. By default ids,
//! kinds, forms, tokens, reasons and counts. A piece's text only through a [`PieceText`] the caller passes,
//! which is the owner's at his own terminal alone. A reader in company ([`Reader::place`]) is shown that
//! place's pieces only: the others are counted in `held_out`, never listed.

use serde::Serialize;
use theseus_protocol::view::{
    ViewDrop, ViewPiece, ViewPov, ViewResidue, ViewShown, ViewSlot, ViewTotals,
};

use super::super::budget::Dropped;
use super::super::{Anchor, AtHand, Label, Piece, PointOfView, Space};
use super::ViewRecord;

/// The text of a piece, from its source; none when its source holds no text this build reads.
pub trait PieceText {
    fn text(&self, piece: &Piece) -> Option<String>;
}

/// Who reads a record.
#[derive(Clone, Copy, Default)]
pub struct Reader<'a> {
    /// `None`: every piece (the owner alone, or at his own terminal). `Some(place)`: a reader in company,
    /// shown only the pieces of that place.
    pub place: Option<&'a str>,
    /// Each piece's text: given only for the owner at his own terminal.
    pub text: Option<&'a dyn PieceText>,
}

/// An enumerated value's name, as it is stored.
pub fn name<T: Serialize>(v: &T) -> String {
    match serde_json::to_value(v) {
        Ok(serde_json::Value::String(s)) => s,
        Ok(other) => other.to_string(),
        Err(_) => String::new(),
    }
}

/// The place a range lies in, when its space has one.
fn place_of_space(space: &Space) -> Option<&str> {
    match space {
        Space::Exchanges { place } | Space::Folds { place, .. } => Some(place),
        _ => None,
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
        Space::Code { .. } => "code",
    }
}

impl Reader<'_> {
    /// Whether a piece is this reader's to see: every piece, or in company the place's own. Code's own text
    /// (the frame, the clock) is every reader's.
    fn sees(&self, piece: &Piece) -> bool {
        match self.place {
            None => true,
            Some(_) if matches!(piece.range.space, Space::Code { .. }) => true,
            Some(p) => piece.marks.place.as_deref() == Some(p),
        }
    }

    /// The same for a dropped piece, which carries no marks: by the place its range lies in.
    fn sees_dropped(&self, d: &Dropped) -> bool {
        match self.place {
            None => true,
            Some(_) if matches!(d.range.space, Space::Code { .. }) => true,
            Some(p) => place_of_space(&d.range.space) == Some(p),
        }
    }

    /// One record, as this reader is shown it.
    pub fn show(&self, rec: &ViewRecord) -> ViewShown {
        let mut totals = ViewTotals::default();
        let mut slots: Vec<ViewSlot> = Vec::new();
        let mut shown: Vec<_> = rec.shown.iter().collect();
        shown.sort_by_key(|s| s.piece.slot);
        for s in shown {
            if !self.sees(&s.piece) {
                totals.held_out += 1;
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
        for d in &rec.dropped {
            if !self.sees_dropped(d) {
                totals.held_out += 1;
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
            pov: pov_of(&rec.pov),
            slots,
            dropped,
            // A band's residue is a count with no place: in company it is not shown.
            residue: match self.place {
                None => rec.residue.iter().map(residue_of).collect(),
                Some(_) => Vec::new(),
            },
            totals,
        }
    }
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
        text: None,
    }
}

fn residue_of(r: &super::super::budget::Residue) -> ViewResidue {
    ViewResidue {
        band: name(&r.band),
        pieces: r.pieces,
        tokens: r.tokens,
    }
}

/// The point of view, with no name in it: whether a person was named, not who.
fn pov_of(p: &PointOfView) -> ViewPov {
    let (anchor_kind, anchor) = match &p.anchor {
        Anchor::Place(a) => ("place", a.clone()),
        Anchor::Task(t) => ("task", t.clone()),
    };
    ViewPov {
        anchor_kind: anchor_kind.into(),
        anchor,
        present: name(&p.present),
        person: p.person.is_some(),
        at_hand: p.at_hand.as_ref().map(|a| match a {
            AtHand::LineOfWork(l) => format!("line_of_work {l}"),
            AtHand::Task(t) => format!("task {t}"),
        }),
        now_ms: p.now_ms,
    }
}
