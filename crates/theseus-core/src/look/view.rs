//! `look.theseus` asked for `view` (issue 44): Theseus reads what its own turn was given at its newest
//! compile, slot by slot, and what was left out and why; by id, kind, form, tokens and reason, never text (a
//! piece opens by its handle, through the history tool).
//!
//! - **Who is present** decides nothing (anything goes, 2026-11-22): the look reads the turn's view as the
//!   turn's widest point of view (`view_gate::Verified`).
//! - **Every piece is checked again today** through the gate (`view::record::source`), with the turn's point
//!   of view narrowed by the record's: a piece this turn may not read now is not listed, and nothing counts it.
//! - **One view, bounded**: the turn's newest, at most [`PIECES`] pieces listed; past that it says how many
//!   pieces it may read were not listed.
//! - **Its proof, bounded**: at most [`PROOF`] units of proof of folds and task cards, spent in the order the
//!   look lists pieces; a fold or a card the budget does not reach is not listed, and one line says how many.
//!   The piece bound cannot come first: the totals, the count of pieces past it and the residue are of every
//!   piece the turn may read. It is read on the blocking pool (`look.rs`), as the owner's `view.show` is.

use crate::view::record::show::Reader;
use crate::view::record::source::GateSource;
use crate::view::record::{self, ViewRecord};
use crate::view_gate::Verified;
use crate::Core;

/// The most pieces (shown and dropped) the look lists.
pub const PIECES: usize = 60;

/// The most units of proof one look spends on folds and task cards ([`GateSource::reads`]: a fold's node and
/// each exchange it stands for, the task list once): a settled ladder's rungs 1 and 2 whole, each standing at
/// fewer than 2N nodes (`ladder::read`), of N and N² exchanges, and the task list. 16,459 with N = 20.
pub const PROOF: u64 = {
    let n = crate::ladder::N as u64;
    (2 * n - 1) * ((1 + n) + (1 + n * n)) + 1
};

/// What the look says when the turn has no view.
const NONE: &str = "this turn has no view record: its view is not written yet";

/// The look's words: this turn's newest view.
pub(crate) fn words(core: &Core, session: &str, turn: &str) -> String {
    #[cfg(test)]
    READ_ON
        .lock()
        .unwrap()
        .push((session.to_string(), std::thread::current().id()));
    let verified = Verified::of_session(&core.runner, session);
    match record::newest(&core.store, turn) {
        Ok(Some(v)) => shown(core, session, &verified, &v),
        Ok(None) => format!("{NONE}\n"),
        Err(e) => format!("this turn's view cannot be read: {e:#}\n"),
    }
}

/// `v` as the turn may read it today.
fn shown(core: &Core, session: &str, verified: &Verified, v: &ViewRecord) -> String {
    let source = GateSource::within(&core.runner, verified.pov().clone(), Some(session), PROOF);
    let reader = Reader {
        source: &source,
        text: None,
        restricted: false,
    };
    let shown = reader.show(v);
    bounded(shown, source.unchecked())
}

/// At most [`PIECES`] pieces listed; the rest of what may be read is counted, and so are the `unchecked`
/// folds and cards the proof's budget did not reach.
fn bounded(mut v: theseus_protocol::view::ViewShown, unchecked: u64) -> String {
    let mut room = PIECES;
    let mut over = 0usize;
    for s in &mut v.slots {
        let keep = s.pieces.len().min(room);
        over += s.pieces.len() - keep;
        s.pieces.truncate(keep);
        room -= keep;
    }
    v.slots.retain(|s| !s.pieces.is_empty());
    let keep = v.dropped.len().min(room);
    over += v.dropped.len() - keep;
    v.dropped.truncate(keep);
    let mut out = theseus_protocol::view::lines(&v);
    if over > 0 {
        out.push_str(&format!(
            "  ({over} more pieces you may read are not listed: the look lists {PIECES})\n"
        ));
    }
    if unchecked > 0 {
        out.push_str(&format!(
            "  ({unchecked} folds or task cards were not checked and are not listed: a look proves at most \
             {PROOF} of their sources)\n"
        ));
    }
    out
}

/// The thread each session's look read its view on: a test's witness.
#[cfg(test)]
static READ_ON: std::sync::Mutex<Vec<(String, std::thread::ThreadId)>> =
    std::sync::Mutex::new(Vec::new());

/// The threads `session`'s looks read their view on.
#[cfg(test)]
pub(crate) fn read_on(session: &str) -> Vec<std::thread::ThreadId> {
    (READ_ON.lock().unwrap().iter())
        .filter(|(s, _)| s == session)
        .map(|(_, t)| *t)
        .collect()
}

#[cfg(test)]
mod tests;
