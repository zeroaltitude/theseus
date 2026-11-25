//! A task's turns in the view (issue 44; the design's §7, "a task's own view"; `task_graph::place`): a task
//! is its own place, `task:<task id>`, and its turns write exchanges there. What its view lays beyond any
//! place's is its opening: the nodes `task.create` wrote before its first turn (the brief, then the
//! arrangement's pieces), which no turn's id carries, so no exchange holds them. They head the window on
//! every turn, as they head the task's session; on its first turn the brief is the message the turn
//! answers; pieces kept by reference are laid with their words read (`arrangement::laid`). The task's card
//! is laid beside the message (`task_graph::joined::StoreCards`).

use crate::node::Body;
use crate::store::{Store, Transcript};
use crate::stub::{Kind, Stub};
use crate::view::{Anchor, PointOfView};

/// The opening of a task's session: its nodes before its first turn's.
pub(super) fn opening(nodes: &[(u64, Stub)]) -> Transcript {
    (nodes.iter())
        .take_while(|(_, n)| n.turn_id.is_none())
        .cloned()
        .collect()
}

/// The brief of a task's session, by node and text, on its first turn (`turn`): no node of an earlier
/// turn is there yet. None on any later turn, or with no brief.
pub(in crate::turn) fn brief(nodes: &[(u64, Stub)], turn: &str) -> Option<(String, String)> {
    let first = (nodes.iter()).all(|(_, n)| n.turn_id.as_deref().is_none_or(|id| id == turn));
    if !first {
        return None;
    }
    opening(nodes).iter().find_map(|(_, n)| match &n.body {
        Body::UserMessage { text, .. } => Some((n.id.clone(), text.clone())),
        _ => None,
    })
}

/// Whether `t` is a task's first turn, which its brief opened (a message written before the turn).
pub(in crate::turn) fn brief_opens(t: &crate::turn::Turn<'_>) -> bool {
    t.tc.task.is_some()
        && (t.tc.store.transcript(t.tc.session_id))
            .is_ok_and(|nodes| brief(&nodes, t.tc.turn_id).is_some())
}

/// The brief a task's first turn answers, read when `session` is a task's (none for any other session).
pub(super) fn brief_of(
    store: &Store,
    session: &str,
    turn: &str,
) -> anyhow::Result<Option<(String, String)>> {
    if crate::task_graph::place::of_task_session(store, session).is_none() {
        return Ok(None);
    }
    Ok(brief(&store.transcript(session)?, turn))
}

/// The opening a view anchored at a task's place lays at the head of its window: the task's session's
/// opening; none for a view anchored anywhere else, or a task with no session. The asking session `sid`
/// is read first: a task's own turn lays its own opening whether or not the task graph holds a record of
/// it (a routine's run, a project's weekly review: their runs are tasks with no task record).
pub(super) fn window_head(
    store: &Store,
    sid: &str,
    pov: &PointOfView,
) -> anyhow::Result<Transcript> {
    let Anchor::Place(place) = &pov.anchor else {
        return Ok(Vec::new());
    };
    let Some(id) = crate::task_graph::place::task_of(place) else {
        return Ok(Vec::new());
    };
    if crate::task_graph::place::of_task_session(store, sid).as_deref() == Some(place.as_str()) {
        return Ok(laid(store, opening(&store.transcript(sid)?)));
    }
    let session = crate::task_graph::get(store, id)?.and_then(|r| r.session);
    match session {
        Some(s) => Ok(laid(store, opening(&store.transcript(&s)?))),
        None => Ok(Vec::new()),
    }
}

/// An opening as a compile lays it: its arrangement's pieces kept by reference with their words read.
fn laid(store: &Store, nodes: Transcript) -> Transcript {
    (nodes.into_iter())
        .map(|(p, n)| match n.kind == Kind::Arrangement {
            true => (p, Stub::from(crate::arrangement::laid(store, &n))),
            false => (p, n),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::node::{Node, Origin};
    use crate::store::Transcript;
    use crate::stub::Stub;

    fn of(nodes: Vec<Node>) -> Transcript {
        (nodes.into_iter().enumerate())
            .map(|(i, n)| (i as u64 + 1, Stub::from(Arc::new(n))))
            .collect()
    }

    fn said(turn: Option<&str>, text: &str) -> Node {
        Node::relayed("ses_t", turn, Origin::Agent, "session:ses_p", text)
    }

    #[test]
    fn the_brief_is_the_first_turns_message_and_no_later_ones() {
        let nodes = of(vec![said(None, "BRIEF: count the boats")]);
        let (_, text) = super::brief(&nodes, "trn_1").expect("the first turn's brief");
        assert_eq!(text, "BRIEF: count the boats");
        let later = of(vec![
            said(None, "BRIEF: count the boats"),
            said(Some("trn_1"), "a wake"),
        ]);
        assert!(
            super::brief(&later, "trn_1").is_some(),
            "its own turn's nodes"
        );
        assert!(super::brief(&later, "trn_2").is_none(), "a later turn");
        assert_eq!(super::opening(&later).len(), 1);
    }
}
