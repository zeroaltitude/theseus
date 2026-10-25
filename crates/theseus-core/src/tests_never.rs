//! "Never propose this" and small talk (theseus-648f8), against the fake Jev:
//! a stored never hides a topic's proposals and an undo brings them back; the
//! config's `not_topics` is never offered; a new topic with no name is
//! never'd by its session; and a window of small talk is not judged, nor
//! judged again at the next exchange end.

use std::time::Duration;

use theseus_judge::fake::FakeJev;
use theseus_protocol::{
    OntologyNeverListParams, OntologyNeverUndoParams, OntologyProposalNeverParams,
    OntologyProposalsParams,
};

use crate::judge::categorize::EVERY;
use crate::tests_categorize::{harbor_at, moorings, rig, session, turn, until_judged};

fn proposals(r: &crate::tests_categorize::Rig) -> theseus_protocol::OntologyProposalsResult {
    r.core
        .ontology_proposals(&OntologyProposalsParams::default())
        .unwrap()
}

fn never(
    r: &crate::tests_categorize::Rig,
    judgments: Vec<String>,
) -> theseus_protocol::OntologyProposalNeverResult {
    r.core
        .ontology_proposal_never(
            &OntologyProposalNeverParams {
                judgments,
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap()
}

/// Two sessions are judged `harbor`; a never on the first rejects it and
/// hides the second (counted in `hidden`); the list shows it; an undo brings
/// the second back.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stored_never_hides_a_topic_and_an_undo_brings_it_back() {
    let jev = FakeJev::start().unwrap();
    harbor_at(&jev, "harbor", 0.93);
    let r = rig(Some(&jev), 30, |_| {});
    let (a, b) = (session(&r.core, None), session(&r.core, None));
    moorings(&r.core, &a, EVERY).await;
    until_judged(&r.core.store, 1).await;
    moorings(&r.core, &b, EVERY).await;
    until_judged(&r.core.store, 2).await;
    let listed = proposals(&r);
    assert_eq!((listed.proposals.len(), listed.hidden), (2, 0));
    let first = listed
        .proposals
        .iter()
        .find(|p| p.session_id == a)
        .unwrap()
        .judgment
        .clone();

    let done = never(&r, vec![first]);
    assert_eq!((done.rejected.len(), done.left.len()), (1, 0), "{done:?}");
    assert_eq!(done.nevers.len(), 1);
    assert_eq!(done.nevers[0].key, "topic:harbor");
    let listed = proposals(&r);
    assert!(listed.proposals.is_empty(), "{:?}", listed.proposals);
    assert_eq!(
        listed.hidden, 1,
        "the second is hidden by the never, counted"
    );
    let nevers = r
        .core
        .ontology_never_list(&OntologyNeverListParams {})
        .unwrap();
    assert_eq!(nevers.nevers.len(), 1);
    assert_eq!(nevers.nevers[0].name, "harbor");

    r.core
        .ontology_never_undo(
            &OntologyNeverUndoParams {
                key: "topic:harbor".into(),
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    let listed = proposals(&r);
    assert_eq!((listed.proposals.len(), listed.hidden), (1, 0));
    assert_eq!(listed.proposals[0].session_id, b);
    assert!(r
        .core
        .ontology_never_list(&OntologyNeverListParams {})
        .unwrap()
        .nevers
        .is_empty());
    // An undo of what is not in force is refused.
    assert!(r
        .core
        .ontology_never_undo(
            &OntologyNeverUndoParams {
                key: "topic:harbor".into(),
                ..Default::default()
            },
            "the CLI"
        )
        .is_err());
}

/// A topic in `[ontology] not_topics` (case and spacing ignored) is not
/// offered to Jev, the other topics are.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_configured_never_topic_is_not_offered() {
    let jev = FakeJev::start().unwrap();
    harbor_at(&jev, "harbor", 0.93);
    let r = rig(Some(&jev), 11, |c| {
        c.ontology.not_topics = vec!["  GARDEN ".into()]
    });
    let sid = session(&r.core, None);
    moorings(&r.core, &sid, EVERY).await;
    until_judged(&r.core.store, 1).await;
    let body = serde_json::to_string(&jev.seen()[0].body).unwrap();
    assert!(body.contains("\"harbor\""), "{body}");
    assert!(!body.contains("\"garden\""), "never offered: {body}");
}

/// A new topic has no name: its never is its session's. A later new-topic
/// judgment in that session is hidden; another session's is not.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_new_topic_never_is_the_sessions() {
    let jev = FakeJev::start().unwrap();
    harbor_at(&jev, "new_topic", 0.9);
    let r = rig(Some(&jev), 40, |_| {});
    let (a, b) = (session(&r.core, None), session(&r.core, None));
    moorings(&r.core, &a, EVERY).await;
    until_judged(&r.core.store, 1).await;
    let j = proposals(&r).proposals[0].clone();
    assert!(j.new_topic);
    let done = never(&r, vec![j.judgment]);
    assert_eq!(done.nevers[0].key, format!("session:{a}"));
    moorings(&r.core, &a, EVERY).await;
    until_judged(&r.core.store, 2).await;
    moorings(&r.core, &b, EVERY).await;
    until_judged(&r.core.store, 3).await;
    let listed = proposals(&r);
    assert_eq!(listed.hidden, 1, "the session's second new topic");
    assert_eq!(listed.proposals.len(), 1);
    assert_eq!(listed.proposals[0].session_id, b);
}

/// A window of greetings and status checks makes no judgment and no Jev
/// call, and the skip moves the mark, so the next exchange end reads only its
/// own records; one substantive message among them is judged, and only that
/// window. (A Jev call takes a second or more here, so the absence of calls
/// is checked at the end, after the window that is judged.)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn small_talk_is_not_judged_and_does_not_retrigger() {
    let jev = FakeJev::start().unwrap();
    harbor_at(&jev, "harbor", 0.93);
    let r = rig(Some(&jev), 40, |_| {});
    let sid = session(&r.core, None);
    let judge = r.core.runner.judge.clone();
    let talk = ["How is it going?", "thanks", "ok", "status?", "Any update?"];
    for i in 0..EVERY {
        turn(&r.core, &sid, talk[i % talk.len()]).await;
    }
    // The skip moved the mark in memory: wait for it.
    let t0 = std::time::Instant::now();
    while !judge.small_talk_skipped(&sid) {
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "the window was skipped"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let after_skip = judge.categorize_records_read();
    turn(&r.core, &sid, "how's it going").await;
    tokio::time::sleep(Duration::from_millis(400)).await;
    let delta = judge.categorize_records_read() - after_skip;
    // One turn writes about twenty records; the skipped window's turns, about
    // two hundred and fifty. A mark left behind would read them all again.
    assert!(
        delta < 100,
        "the mark moved past the skipped window: {delta} records read after {after_skip}"
    );

    // A window with one substantive message is judged as before.
    for i in 0..EVERY - 2 {
        turn(&r.core, &sid, talk[i % talk.len()]).await;
    }
    turn(&r.core, &sid, "Mooring line 4: is it chafing at low tide?").await;
    until_judged(&r.core.store, 1).await;
    // Only that window: no judgment, no Jev call, for the greetings.
    tokio::time::sleep(Duration::from_millis(3000)).await;
    assert_eq!(until_judged(&r.core.store, 1).await.len(), 1);
    assert_eq!(jev.seen().len(), 1);
}
