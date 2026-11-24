//! Picture labels per exchange (issue 46): the onto pack asks two questions
//! of the pictures attached to a person's message, what the picture is mostly
//! of (`image_subject`, the first picture's when there are several) and
//! whether a picture shows a screen or a document someone could read
//! (`shows_screen_or_document`). This reads those answers where the sink
//! writes the judgment's frame, as [`super::topics`] reads the categories,
//! and writes each one at the act band as a touch of the message's exchange:
//! `picture:<subject>`, the pack's option id (`picture:document`), and
//! `picture:shows_screen_or_document` for a yes. A no writes nothing; it stays
//! in the judgment's row, as every answer does.
//!
//! The labels describe the message's pictures, not each picture: the subject
//! is the first one's, the screen answer any one's, and a picture has no
//! record of its own. No judge call is added and the request is not changed,
//! and nothing that decides an act reads these roots: they are found by
//! name, as any thing's timeline is.

use std::collections::BTreeSet;

use theseus_judge::client::Answer;
use theseus_judge::Judgment;
use theseus_store::NewRecord;

use crate::exchange::{touch, Root};
use crate::judge::onto::decide;
use crate::store::Store;

/// What every picture label's root starts with; the sweep mints no kind of
/// this name (`judge::onto::mint`), so no category shares these roots.
pub const PREFIX: &str = "picture:";
/// The question whose answer is the subject's label.
pub const SUBJECT: &str = "image_subject";
/// The question whose yes is a label of its own, under its own id.
pub const SCREEN: &str = "shows_screen_or_document";

/// The picture labels of `j` at the act band `act`: its subject's, and the
/// screen question's when the answer is yes. None for a judgment that asked
/// neither (a message with no picture).
pub fn roots(j: &Judgment, act: f64) -> Vec<String> {
    let mut out = Vec::new();
    if let Some((subject, confidence)) = decide::choice(j, SUBJECT) {
        if confidence >= act && !subject.is_empty() {
            out.push(format!("{PREFIX}{subject}"));
        }
    }
    let yes = j.answer(SCREEN).and_then(|a| match a.answer {
        Answer::Noul { noul } => Some(noul),
        _ => None,
    });
    if yes.is_some_and(|p| p >= act) {
        out.push(format!("{PREFIX}{SCREEN}"));
    }
    out
}

/// The touches the judgments of `batch` come to: each label of [`roots`],
/// once, to its message's exchange (`topics::landing`). The caller gives only
/// judgments the ontology may act on ([`acting`]).
pub fn touches(store: &Store, batch: &[&Judgment], act: f64) -> Vec<NewRecord> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for j in batch {
        let roots = roots(j, act);
        if roots.is_empty() {
            continue;
        }
        let Some((turn, session, at)) = super::topics::landing(store, j) else {
            continue;
        };
        for root in roots {
            if seen.insert((turn.clone(), root.clone())) {
                out.extend(touch(Root::Thing(&root), &turn, &session, at));
            }
        }
    }
    out
}

/// The judgments of `batch` whose answers may be written: the wired onto
/// pack's, answered by the model it pins, in a mode that acts. A learned or
/// shadow version's never are; it is the gate topics use.
pub(crate) fn acting(batch: &[Judgment]) -> Vec<&Judgment> {
    batch
        .iter()
        .filter(|j| crate::judge::onto::stage::acts(j))
        .collect()
}

/// [`touches`] for the sink's frame, at `[ontology] act`.
pub(crate) fn of_batch(core: &crate::rpc::Core, batch: &[Judgment]) -> Vec<NewRecord> {
    touches(&core.store, &acting(batch), core.cfg.ontology.act)
}

#[cfg(test)]
#[path = "tests_pictures.rs"]
mod tests;
