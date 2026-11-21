//! `look.theseus` asked for `view` (issue 44): Theseus reads what its own turn was given at its newest
//! compile, slot by slot, and what was left out and why; by id, kind, form, tokens and reason, never text (a
//! piece opens by its handle, through the history tool).
//!
//! - **Who is present** is the gate's own reading of this turn (`view_gate::Verified`, the presence the turn
//!   computed from its input's owner's mark), for this turn alone: a presence kept for another turn of the
//!   session, or none, is unknown, and unknown is shown nothing. No argument of the call can claim it, and no
//!   node of the session is read to find it.
//! - **Every piece is checked again today** through the gate (`view::record::source`), with the turn's point
//!   of view narrowed by the record's: a piece this turn may not read now is not listed, and nothing counts it.
//! - **One view, bounded**: the turn's newest, at most [`PIECES`] pieces listed; past that it says how many
//!   pieces it may read were not listed.

use theseus_memory::gate::Present;

use crate::view::permit::point_of_view;
use crate::view::record::show::Reader;
use crate::view::record::source::GateSource;
use crate::view::record::{self, ViewRecord};
use crate::view_gate::Verified;
use crate::Core;

/// The most pieces (shown and dropped) the look lists.
pub const PIECES: usize = 60;

/// What the look says when the turn has no view.
const NONE: &str =
    "this turn has no view record: its context was compiled as a transcript (`[context] \
     assembly`), or its view is not written yet";

/// What it says when who is present is not known.
const UNKNOWN: &str = "who is present in this turn cannot be read, so its view is not shown";

/// The look's words: this turn's newest view, as the present may read it.
pub(crate) fn words(core: &Core, session: &str, turn: &str) -> String {
    let this_turn = core
        .runner
        .presence
        .get(session)
        .is_some_and(|a| a.turn_id == turn);
    let verified = Verified::of_session(&core.runner, session);
    if !this_turn || verified.pov().present == Present::Unknown {
        return format!("{UNKNOWN}\n");
    }
    match record::newest(&core.store, turn) {
        Ok(Some(v)) => shown(core, session, &verified, &v),
        Ok(None) => format!("{NONE}\n"),
        Err(e) => format!("this turn's view cannot be read: {e:#}\n"),
    }
}

/// `v` as the turn may read it today.
fn shown(core: &Core, session: &str, verified: &Verified, v: &ViewRecord) -> String {
    let pov = point_of_view(verified, &v.pov);
    let restricted = pov.present != Present::OwnerAlone;
    let source = GateSource {
        runner: &core.runner,
        pov,
        asker: Some(session),
    };
    let reader = Reader {
        source: &source,
        text: None,
        restricted,
    };
    bounded(reader.show(v))
}

/// At most [`PIECES`] pieces listed; the rest of what may be read is counted.
fn bounded(mut v: theseus_protocol::view::ViewShown) -> String {
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
    out
}

#[cfg(test)]
mod tests;
