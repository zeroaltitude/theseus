//! The operator's bulk yes to Jev's proposals (theseus-wy7y):
//! `ontology.proposal.accept_all` takes every unanswered proposal of a kind
//! at a confidence, or exactly the judgments named (the cockpit's
//! selection), each as the single accept takes it (the operator's
//! membership and the judgment's label); a new topic's proposal is left for
//! one at a time, with why; and a refused answer accepts nothing.

use theseus_ontology::Origin;
use theseus_protocol::{OntologyProposalAcceptAllParams, OntologyProposalsParams};

use crate::approval::{Answerer, Surface};
use crate::judge::categorize::EVERY;
use crate::tests_categorize::{harbor_at, moorings, rig, session, until_judged};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn proposals_are_accepted_in_bulk_by_kind_confidence_or_selection() {
    let jev = theseus_judge::fake::FakeJev::start().unwrap();
    let r = rig(Some(&jev), 50, |_| {});
    let mut sessions = Vec::new();
    for (i, (option, confidence)) in [
        ("harbor", 0.93),
        ("garden", 0.62),
        ("new_topic", 0.97),
        ("harbor", 0.71),
    ]
    .into_iter()
    .enumerate()
    {
        harbor_at(&jev, option, confidence);
        let s = session(&r.core, None);
        moorings(&r.core, &s, EVERY).await;
        until_judged(&r.core.store, i + 1).await;
        sessions.push(s);
    }
    let all = r
        .core
        .ontology_proposals(&OntologyProposalsParams::default())
        .unwrap()
        .proposals;
    let of = |i: usize| {
        all.iter()
            .find(|p| p.session_id == sessions[i])
            .unwrap()
            .judgment
            .clone()
    };

    // People: none proposed, none accepted.
    let none = r
        .core
        .ontology_proposal_accept_all(
            &OntologyProposalAcceptAllParams {
                kind: Some("person".into()),
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    assert!(none.accepted.is_empty() && none.left.is_empty(), "{none:?}");

    // Someone else's answer, from a shared place, accepts nothing.
    let shared = Answerer {
        label: "discord:31415".into(),
        surface: Surface::Discord,
        discord: Some(theseus_protocol::DiscordOrigin {
            user_id: "31415".into(),
            channel_id: "92653".into(),
            guild_id: Some("27182818".into()),
        }),
    };
    assert!(r
        .core
        .ontology_proposal_accept_all(&OntologyProposalAcceptAllParams::default(), shared)
        .is_err());

    // Topics at 0.9 or more: the first; the new topic's is left with why.
    let done = r
        .core
        .ontology_proposal_accept_all(
            &OntologyProposalAcceptAllParams {
                kind: Some("topic".into()),
                min_confidence: 0.9,
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    assert_eq!(done.accepted, [of(0)]);
    assert_eq!(done.left.len(), 1, "{:?}", done.left);
    assert!(done.left[0].starts_with(&of(2)), "{:?}", done.left);
    assert!(done.left[0].contains("needs a name"), "{:?}", done.left);
    let o = r.core.runner.ontology.snapshot(&r.core.store).unwrap();
    let held = o.memberships(&sessions[0]);
    assert_eq!(
        (held[0].category.as_str(), held[0].origin),
        ("topic:harbor", Origin::Operator)
    );

    // Exactly the selection: the fourth, not the second.
    let done = r
        .core
        .ontology_proposal_accept_all(
            &OntologyProposalAcceptAllParams {
                judgments: vec![of(3)],
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    assert_eq!(done.accepted, [of(3)]);
    let left: Vec<String> = r
        .core
        .ontology_proposals(&OntologyProposalsParams::default())
        .unwrap()
        .proposals
        .into_iter()
        .map(|p| p.judgment)
        .collect();
    assert_eq!(left.len(), 2);
    assert!(left.contains(&of(1)) && left.contains(&of(2)));
}
