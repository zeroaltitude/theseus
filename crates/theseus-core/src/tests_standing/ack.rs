//! A model that thanks the owner for his own standing record is caught in the
//! loop (theseus-ectzg, ask 3): the leading "Noted:" paragraph that restates a
//! record he did not just send is stripped from the answer and recorded as a
//! `turn.standing_ack` row, with the record's id and the words it removed.
//! `standing_ack::tests` holds the comparison; these go through a whole turn.

use super::framing::turn_via;
use super::*;

const PREF: &str = "In group chats, speak as yourself (Theseus); when something needs his voice \
                    or decision, say so and leave it to him";
const NOTED: &str = "Noted: in group chats I speak as myself, and anything that needs your \
                     voice or decision goes back to you. This is a DM, so nothing changes here.";

/// The text of the session's last answer, as the node keeps it.
fn answer(r: &Rig, sid: &str) -> String {
    let nodes = r.core.store.transcript(sid).unwrap();
    let (_, n) = nodes
        .iter()
        .rev()
        .find(|(_, n)| matches!(n.body, crate::node::Body::AssistantMessage { .. }))
        .expect("an answer");
    match &n.body {
        crate::node::Body::AssistantMessage { blocks, .. } => blocks
            .iter()
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join(""),
        _ => unreachable!(),
    }
}

async fn said(input: &str, reply: &str) -> (Rig, String) {
    let r = rig();
    r.add(
        StandingKind::Preference,
        StandingScope::Surface("discord".into()),
        PREF,
    );
    *r.model.reply.lock().unwrap() = Some(reply.into());
    let sid = session(&r.core, None);
    turn_via(&r.core, &sid, input, None).await;
    (r, sid)
}

/// The owner's complaint: the answer opened by thanking him for his record.
#[tokio::test]
async fn a_leading_noted_that_restates_a_record_is_stripped_and_recorded() {
    let (r, sid) = said(
        "How full is the disk?",
        &format!("{NOTED}\n\nThe disk is at 41%."),
    )
    .await;
    assert_eq!(answer(&r, &sid), "The disk is at 41%.");
    let rows = rows_of(&r.core, "turn.standing_ack");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["action"], "stripped");
    assert_eq!(rows[0]["records"].as_array().unwrap().len(), 1);
    assert!(rows[0]["paragraph"]
        .as_str()
        .unwrap()
        .starts_with("Noted: in group chats"));
}

/// Flagged, not emptied: an answer that is only the acknowledgement stays.
#[tokio::test]
async fn an_answer_that_is_only_the_acknowledgement_is_recorded_and_kept() {
    let (r, sid) = said("hi", NOTED).await;
    assert_eq!(answer(&r, &sid), NOTED);
    let rows = rows_of(&r.core, "turn.standing_ack");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["action"], "flagged");
}

/// The check is not a blunt instrument: "Noted: 8082." after he said it.
#[tokio::test]
async fn a_noted_that_answers_the_owner_is_untouched() {
    let (r, sid) = said(
        "The staging port is 8082.",
        "Noted: 8082.\n\nI will use it.",
    )
    .await;
    assert_eq!(answer(&r, &sid), "Noted: 8082.\n\nI will use it.");
    assert!(rows_of(&r.core, "turn.standing_ack").is_empty());
    // And a record he just restated: "Noted" is his answer.
    let (r, sid) = said(
        "In group chats, speak as yourself; if it needs my voice or decision, say so.",
        &format!("{NOTED}\n\nDone."),
    )
    .await;
    assert_eq!(answer(&r, &sid), format!("{NOTED}\n\nDone."));
    assert!(rows_of(&r.core, "turn.standing_ack").is_empty());
}

/// A "Noted:" in the middle, and an answer with no marker: untouched.
#[tokio::test]
async fn a_noted_in_the_middle_and_a_plain_answer_are_untouched() {
    let mid = format!("The disk is at 41%.\n\n{NOTED}");
    let (r, sid) = said("How full is the disk?", &mid).await;
    assert_eq!(answer(&r, &sid), mid);
    let (r, sid) = said("How full is the disk?", "The disk is at 41%.").await;
    assert_eq!(answer(&r, &sid), "The disk is at 41%.");
    assert!(rows_of(&r.core, "turn.standing_ack").is_empty());
}

/// The step's pre-filter and `check` read one list (review F2): a literal in
/// the step would reject a marker added to `MARKERS` before `check` ran, with
/// every test of `check` still green.
#[test]
fn the_step_reads_the_marker_list_and_not_a_copy_of_it() {
    use crate::standing_ack::{opens_with_marker, MARKERS};
    assert!(MARKERS.len() >= 2);
    for m in MARKERS {
        assert!(opens_with_marker(&format!("  {m}: x")), "{m}");
    }
    assert!(!opens_with_marker("The disk is at 41%."));
    let step = include_str!("../turn/standing_step.rs");
    for m in MARKERS {
        assert!(
            !step.contains(&format!("\"{m}\"")),
            "turn/standing_step.rs hard-codes the marker {m}: iterate MARKERS"
        );
    }
    assert!(step.contains("opens_with_marker"));
}
