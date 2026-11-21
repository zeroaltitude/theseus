//! `look.theseus` asked for `view` (issue 44): Theseus reads its own view record for this turn: what it was
//! given, slot by slot, what was left out, and why; by id, kind, form, tokens and reason, never text (a piece
//! opens by its handle, through the history tool). Who is present is the gate's own reading of the turn
//! ([`crate::view_gate::present`]), taken from the turn's place, where its input arrived and that input's
//! owner's mark; no argument of the call can claim it. The owner alone is shown every piece; anyone else
//! present, only the pieces of this view's own place, the rest counted; presence that cannot be read is
//! shown nothing.

use theseus_memory::gate::Present;

use crate::node::Body;
use crate::view::record::{self, show::Reader, ViewRecord};
use crate::view::Anchor;
use crate::view_gate::{self, Input};
use crate::Core;

/// What the look says when the turn has no view.
const NONE: &str =
    "this turn has no view record: its context was compiled as a transcript (`[context] \
     assembly`), or its view is not written yet";

/// The id of the message that opened `turn` in `session`: the first person's message of the turn.
fn opener(core: &Core, session: &str, turn: &str) -> Option<String> {
    let nodes = core.store.session_nodes(session).ok()?;
    nodes
        .into_iter()
        .map(|(_, n)| n)
        .find(|n| n.turn_id.as_deref() == Some(turn) && matches!(n.body, Body::UserMessage { .. }))
        .map(|n| n.id)
}

/// Who is present for this turn, by the gate's rule.
fn presence(core: &Core, session: &str, turn: &str) -> Present {
    let place = core.runner.place_of(session);
    let target = core.runner.outbox.try_target(session).ok().flatten();
    let node = opener(core, session, turn);
    let input = match &node {
        Some(n) => Input::Message(n),
        None => Input::SetUp(None),
    };
    view_gate::present(&place, target.as_deref(), input, &core.store)
}

/// The look's words: this turn's views, as the present may read them.
pub(super) fn words(core: &Core, session: &str, turn: &str) -> String {
    let present = presence(core, session, turn);
    if present == Present::Unknown {
        return "who is present in this turn cannot be read, so its view is not shown\n".into();
    }
    match record::of_turn(&core.store, turn) {
        Ok(views) => shown(present, &views),
        Err(e) => format!("this turn's view cannot be read: {e:#}\n"),
    }
}

/// `views` as `present` may read them: every piece for the owner alone; for anyone else, each view's own
/// place's pieces, the rest counted. `Unknown` is never passed here.
fn shown(present: Present, views: &[ViewRecord]) -> String {
    if views.is_empty() {
        return format!("{NONE}\n");
    }
    let alone = present == Present::OwnerAlone;
    let mut out = String::new();
    for v in views {
        let here = match &v.pov.anchor {
            Anchor::Place(p) => p.as_str(),
            Anchor::Task(_) => "",
        };
        let reader = Reader {
            place: (!alone).then_some(here),
            text: None,
        };
        out.push_str(&theseus_protocol::view::lines(&reader.show(v)));
    }
    if !alone {
        out.push_str("(not the owner alone here: only this place's pieces are listed)\n");
    }
    out
}

#[cfg(test)]
mod tests;
