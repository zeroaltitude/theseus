//! A note written into a conversation between its turns (issue 44, issue 46): a card's outcome, a thread's
//! note, a published text. It belongs to no turn, so no exchange holds it; the view's next turn of that
//! session sends it before its own message, as the transcript did.

use crate::node::{Node, Origin};
use crate::tests_places::{rig_with, session, turn, Rig, OWNER};

const NOTE: &str = "Your keep card is settled: the harbour key is kept as asked.";

fn view_rig() -> Rig {
    rig_with(|_| {})
}

/// The texts of a request's messages, as one string.
fn sent(r: &Rig, at: usize) -> String {
    serde_json::to_string(&r.requests()[at].messages).unwrap()
}

#[tokio::test]
async fn a_note_written_between_turns_reaches_the_sessions_next_turn() {
    let r = view_rig();
    let c = &r.core;
    let sid = session(c, Some(&format!("dm:{OWNER}")));
    turn(c, &sid, "keep the harbour key").await;
    let note = Node::relayed(&sid, None, Origin::Harness, "vault", NOTE);
    c.store.append(&[note.record().unwrap()]).unwrap();
    let before = r.requests().len();
    turn(c, &sid, "is it kept?").await;
    let next = sent(&r, before);
    let (at_note, at_message) = (next.find(NOTE), next.find("is it kept?"));
    assert!(at_note.is_some(), "the note is sent: {next}");
    assert!(at_note < at_message, "before the new message: {next}");
    assert_eq!(next.matches(NOTE).count(), 1, "once: {next}");
}
