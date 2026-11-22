//! Notes written into a session between its turns (issue 44, issue 46): a card's outcome told to the
//! conversation that asked, a thread's move, join, seal or home note, a brief, a text the owner published
//! there. Such a node has no turn, so no exchange holds it and the window never lays it; the transcript
//! sent it, so a view turn sends the ones since the session's previous turn, in their places before its
//! own nodes. Imported history (its own kinds) and a range moved to a thread stay out.

use std::collections::HashSet;

use crate::stub::{Kind, Stub};

/// The ids of the notes of `nodes` (a session's, in order) that `turn` sends: a person's or the harness's
/// message with no turn, after the session's last node of another turn, not moved away.
pub(super) fn notes(nodes: &[(u64, Stub)], turn: &str) -> HashSet<String> {
    let since = nodes
        .iter()
        .rposition(|(_, n)| n.turn_id.as_deref().is_some_and(|t| t != turn))
        .map_or(0, |i| i + 1);
    let moves = crate::moved::read(nodes);
    nodes[since..]
        .iter()
        .filter(|(_, n)| n.turn_id.is_none() && n.kind == Kind::UserMessage && moves.shows(n))
        .map(|(_, n)| n.id.clone())
        .collect()
}
