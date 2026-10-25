//! One `ontology.proposal.accept_all` is one frame (theseus-6o8q8): the
//! ontology is cloned, appended and synced once for all the proposals it
//! takes, and each still has its label, its membership and its effect.

use theseus_ontology::CategoryId;
use theseus_protocol::{OntologyProposalAcceptAllParams, OntologyProposalsParams};

use crate::judge::categorize::EVERY;
use crate::tests_categorize::{harbor_at, moorings, rig, session, until_judged};

const SESSIONS: usize = 12;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_accept_all_of_many_topics_is_one_frame_and_each_still_lands() {
    let jev = theseus_judge::fake::FakeJev::start().unwrap();
    harbor_at(&jev, "harbor", 0.93);
    let r = rig(Some(&jev), 50, |_| {});
    let mut sessions = Vec::new();
    for i in 0..SESSIONS {
        let s = session(&r.core, None);
        moorings(&r.core, &s, EVERY).await;
        until_judged(&r.core.store, i + 1).await;
        sessions.push(s);
    }
    let judgments: Vec<String> = r
        .core
        .ontology_proposals(&OntologyProposalsParams::default())
        .unwrap()
        .proposals
        .into_iter()
        .map(|p| p.judgment)
        .collect();
    assert_eq!(judgments.len(), SESSIONS);

    let before = r.core.store.stats().unwrap().frames_appended;
    let done = r
        .core
        .ontology_proposal_accept_all(
            &OntologyProposalAcceptAllParams {
                judgments: judgments.clone(),
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    let frames = r.core.store.stats().unwrap().frames_appended - before;
    assert_eq!(done.accepted.len(), SESSIONS, "{done:?}");
    assert!(done.left.is_empty(), "{done:?}");
    assert_eq!(frames, 1, "one frame for {SESSIONS} accepts");

    // Each one's effect: the session holds the topic.
    let o = r.core.runner.ontology.snapshot(&r.core.store).unwrap();
    let harbor = CategoryId::new("topic", "harbor").unwrap();
    for s in &sessions {
        assert!(
            o.memberships(s).iter().any(|m| m.category == harbor),
            "{s} joined harbor"
        );
    }
    // Each one's record: answered, so nothing is left to propose.
    assert!(r
        .core
        .ontology_proposals(&OntologyProposalsParams::default())
        .unwrap()
        .proposals
        .is_empty());
    // And a repeat takes none and writes no ontology frame of its own.
    let again = r
        .core
        .ontology_proposal_accept_all(
            &OntologyProposalAcceptAllParams {
                judgments,
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    assert!(again.accepted.is_empty() && again.left.len() == SESSIONS);
}
