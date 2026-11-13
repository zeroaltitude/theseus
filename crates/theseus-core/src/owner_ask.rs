//! `owner.ask`: a read-only answer about the owner, from what Theseus already
//! holds (row B4 `theseus-da9hv`, card `theseus-mqret`, base `75f19f3c`).
//!
//! **Retrieval with citation, never a model-written paragraph.** The decision
//! that governs this card: the answer is a selection of lines Theseus already
//! holds, each named by the record it came from, so every line is checkable
//! against its source. Nothing here calls a model, writes a frame, or reaches
//! anything its caller did not already read: an answer holds exactly the lines
//! its caller handed in, and a caller that hands in none gets no items and the
//! count it searched.
//!
//! **What counts as "about the owner"**, and nothing else:
//!
//! - his live standing records of kind `fact` and `preference` (the ones
//!   `standing::Board` holds; `standing.list` gives their text to the owner in
//!   a private place alone). An `order` is what to *do*, not a fact about him,
//!   so [`Held::standing`] is filtered of them by [`answer`] itself rather than
//!   by each caller;
//! - the card observer's open observations (`card::observe`, META
//!   `card.obs.<id>`), which are **unconfirmed and on no card**: every item one
//!   makes says so ([`Cited::unconfirmed`]), because the owner has not answered
//!   their question yet and a surface that stated them as settled fact would be
//!   putting words in his mouth.
//!
//! Nothing else is read here. In particular no node, no index and no recall: a
//! question answered from this module costs no retrieval and no spend, which is
//! what makes it a floor to bench the memory exam's `owner.ask` class against
//! rather than a second recall pipeline.
//!
//! **Pure, and no clock.** `now_ms` is passed in, so no answer depends on the
//! hour of day (`scripts/AGENTS.md`, "A test of the hour or the local day takes
//! a test clock"); the ranking is a total order over (score, time, id), so one
//! question over one set of records gives one answer, byte for byte, whenever
//! it runs.
//!
//! **The wall is the caller's**, and it is `card::tool::refusal`'s, clause for
//! clause: the owner alone in the place, never a task's session, never a model
//! of restricted origin. This module reads slices and not a turn, so it cannot
//! enforce one — and a reader that skipped it would show the owner's own facts
//! in a place he does not read alone. A reader lands with that refusal or it
//! does not land.
//!
//! **Its reader** is the `owner.ask` protocol method and its CLI, which row B4
//! brings. It is deliberately **not** built in this card: a protocol method
//! regenerates `cockpit/src/protocol.gen/`, which this card must not touch, so
//! the wire shapes are declared for the row that wires them instead. The model
//! tool `owner_ask` is a different thing in the other direction: it *asks* the
//! owner a question. Nothing here asks him anything.

use std::collections::BTreeSet;

use theseus_protocol::card::{CardObservation, ObservationState};
use theseus_protocol::standing::{StandingKind, StandingRecord};

use crate::capture::words::content_words;

/// The share of the question's own content words a held line must hold before
/// it is an answer at all. A third: "what is the owner's favourite animal"
/// carries four content words, and a line that shares two of them ("favourite"
/// and "animal") is worth showing, while one that shares only "owner" is not.
/// It is a size for a similarity and no reading of what a sentence means, as
/// `capture::words`'s own bars are.
pub const MIN_COVER: f64 = 0.34;

/// At most this many lines in one answer. A read-only answer is meant to be
/// checked line by line, so it is short; a caller that wants the rest asks a
/// narrower question.
pub const MAX_ITEMS: usize = 8;

/// What Theseus holds about the owner, as its caller read it. Both slices may
/// be empty; neither is read for anything but its own lines.
#[derive(Debug, Clone, Copy, Default)]
pub struct Held<'a> {
    /// Live standing records. [`answer`] keeps the facts and preferences and
    /// drops the orders and the empty text of a revoke, so a caller may hand
    /// in a board's whole live set.
    pub standing: &'a [StandingRecord],
    /// The card observer's observations. [`answer`] keeps the open ones:
    /// a used or dropped observation has had its question answered already and
    /// is no longer what Theseus holds unconfirmed.
    pub observed: &'a [CardObservation],
}

/// One line of the answer, with what to look up to check it.
#[derive(Debug, Clone, PartialEq)]
pub struct Cited {
    /// The line Theseus holds, verbatim. Nothing here rewrites the owner's
    /// words or an observer's phrase.
    pub text: String,
    /// What to look up: `standing:<id>` or `card.obs:<id>`.
    pub citation: String,
    /// How Theseus holds it: `fact`, `preference`, or `observation`.
    pub held_as: &'static str,
    /// Where the line came from: a standing record's `source` (the node it was
    /// captured from, or the surface that made it), an observation's session.
    pub source: String,
    /// When it was written, Unix ms, as its own record says.
    pub when_ms: u64,
    /// The share of the question's content words this line holds, 0 to 1.
    pub covers: f64,
    /// True for an observation: unconfirmed, on no card. A surface shows it as
    /// something Theseus noticed and the owner has not answered.
    pub unconfirmed: bool,
}

/// A question's read-only answer.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Answer {
    /// The lines, best first, at most [`MAX_ITEMS`].
    pub items: Vec<Cited>,
    /// Every line considered, whether it became an item or not. An answer with
    /// no items that searched nothing is a different thing from one that
    /// searched two hundred lines and matched none, and a reader that printed
    /// "Theseus holds nothing about that" for the first would be lying: a bar
    /// of the form "no items" is met by an empty input, so the positive count
    /// rides beside it.
    pub searched: usize,
    /// The question's own content words, sorted, so a surface can say what was
    /// looked for and a test can see why a line did or did not match.
    pub words: Vec<String>,
}

/// The read-only answer to `question` from what `held` holds. It takes no
/// clock: the answer is the same at any hour, and a rule that needed one (a
/// stale fact said as a current one) would take it as an argument rather than
/// read it here.
pub fn answer(question: &str, held: Held<'_>) -> Answer {
    let asked = content_words(question);
    let lines = candidates(held);
    let searched = lines.len();
    let words = asked.iter().cloned().collect();
    if asked.is_empty() {
        return Answer {
            items: Vec::new(),
            searched,
            words,
        };
    }
    let mut items: Vec<Cited> = lines
        .into_iter()
        .filter_map(|c| cover(&asked, &c.text).map(|covers| Cited { covers, ..c }))
        .collect();
    // A total order, so one question over one set gives one answer: the best
    // cover first, then the newest, then the citation, which is unique.
    items.sort_by(|a, b| {
        b.covers
            .total_cmp(&a.covers)
            .then(b.when_ms.cmp(&a.when_ms))
            .then(a.citation.cmp(&b.citation))
    });
    items.truncate(MAX_ITEMS);
    Answer {
        items,
        searched,
        words,
    }
}

/// The share of `asked` that `text` holds, or none when it is under
/// [`MIN_COVER`]. A question word is held when the line holds that word; this
/// is the same content-word rule `capture::words::same` reads, so one notion of
/// a content word serves both.
fn cover(asked: &BTreeSet<String>, text: &str) -> Option<f64> {
    let held = content_words(text);
    let shared = asked.intersection(&held).count();
    if shared == 0 {
        return None;
    }
    let covers = shared as f64 / asked.len() as f64;
    (covers >= MIN_COVER).then_some(covers)
}

/// Every line that may answer, with its citation: the facts and preferences of
/// `held.standing` and the open observations of `held.observed`. `covers` is
/// filled by [`answer`]; here it is zero.
fn candidates(held: Held<'_>) -> Vec<Cited> {
    let standing = held
        .standing
        .iter()
        .filter(|r| !r.text.trim().is_empty())
        .filter_map(|r| {
            let held_as = match r.kind {
                StandingKind::Fact => "fact",
                StandingKind::Preference => "preference",
                // An order is what to do, not a fact about him.
                StandingKind::Order => return None,
            };
            Some(Cited {
                text: r.text.clone(),
                citation: format!("standing:{}", r.id),
                held_as,
                source: r.source.clone(),
                when_ms: r.since_ms,
                covers: 0.0,
                unconfirmed: false,
            })
        });
    let observed = held
        .observed
        .iter()
        .filter(|o| o.state == ObservationState::Open)
        .filter(|o| !o.text.trim().is_empty())
        .map(|o| Cited {
            text: o.text.clone(),
            citation: format!("card.obs:{}", o.id),
            held_as: "observation",
            source: o.session.clone(),
            when_ms: o.at_ms,
            covers: 0.0,
            unconfirmed: true,
        });
    standing.chain(observed).collect()
}

#[cfg(test)]
#[path = "owner_ask/tests.rs"]
mod tests;
