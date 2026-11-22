//! Notes written into a session between its turns (issue 44, issue 46): a card's outcome told to the
//! conversation that asked, a thread's move, join, seal or home note, a brief, a text the owner published
//! there. Such a node has no turn, so no exchange holds it and the window never lays it; the transcript
//! sent it on every later turn, so a view turn sends the ones written since the oldest exchange of this
//! session that its window lays (since its previous turn when the window lays none), before its own
//! nodes. Imported history (its own kinds) and a range moved to a thread stay out.

use std::collections::HashSet;

use crate::stub::{Kind, Stub};

/// The ids of the notes of `nodes` (a session's, in order) that `turn` sends: a person's or the harness's
/// message with no turn, after the first node of an exchange `window` lays (after the session's last node
/// of another turn when it lays none), not moved away. Before a session's
/// first turn there are none: what precedes it is its opening (a task's brief and pieces), which the view
/// lays as the window's head (`turn::view_step::task`).
pub(crate) fn notes(nodes: &[(u64, Stub)], turn: &str, window: &HashSet<&str>) -> HashSet<String> {
    let Some(last) =
        (nodes.iter()).rposition(|(_, n)| n.turn_id.as_deref().is_some_and(|t| t != turn))
    else {
        return HashSet::new();
    };
    let laid = |n: &Stub| n.turn_id.as_deref().is_some_and(|t| window.contains(t));
    let since = (nodes.iter())
        .position(|(_, n)| laid(n))
        .unwrap_or(last + 1);
    let moves = crate::moved::read(nodes);
    nodes[since..]
        .iter()
        .filter(|(_, n)| n.turn_id.is_none() && n.kind == Kind::UserMessage && moves.shows(n))
        .map(|(_, n)| n.id.clone())
        .collect()
}
