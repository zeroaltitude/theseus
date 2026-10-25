//! The owner as a word, and a person "never" (theseus-648f8). Every name
//! here is invented.

use theseus_judge::builders::PersonCandidate;
use theseus_protocol::{
    OntologyNeverListParams, OntologyNeverUndoParams, OntologyProposalNeverParams,
    OntologyProposalsParams,
};

use super::tests::until_rows;
use super::tests_groups::{plant, Plant};
use super::tests_sweep::shut_gate;
use super::NotPeople;
use crate::tests_categorize::session;

fn cand(name: &str) -> PersonCandidate {
    PersonCandidate {
        name: name.into(),
        handles: Vec::new(),
        role_line: String::new(),
        evidence: Vec::new(),
    }
}

/// A transcript's author line says `owner`; as a candidate's name it is the
/// owner, not a person to propose.
#[test]
fn the_word_owner_is_not_a_candidate() {
    let cfg = crate::config::Config::example();
    let not = NotPeople::of(&cfg, &[]);
    assert!(not.excludes(&cand("owner")));
    assert!(not.excludes(&cand("The  Owner")));
    assert!(!not.excludes(&cand("Wren Halloway")));
}

/// A never on a person's row rejects it and keeps the name out of later
/// proposals (counted in `hidden`); an undo lets it back.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_person_never_rejects_the_row_and_keeps_the_name_out() {
    let (_jev, r, _p, _s) = shut_gate(2.0).await;
    r.fake
        .script
        .lock()
        .unwrap()
        .push_back(super::tests::answer(serde_json::json!([
            {"name": "Wren Halloway", "handles": [], "role_line": "", "evidence": ["L1"]}
        ])));
    r.core.people_sweep("nightly").await.unwrap();
    let template = until_rows(&r.core.store, "judge.call", 1).await.remove(0);
    let plant_one = |name: &str| {
        plant(
            &r,
            &template,
            &[Plant {
                sid: session(&r.core, None),
                name: name.into(),
                confidence: 0.95,
                whom: "new_person",
            }],
        )
    };
    let listed = || {
        r.core
            .ontology_proposals(&OntologyProposalsParams::default())
            .unwrap()
    };
    plant_one("Pell Garrow");
    let row = listed()
        .proposals
        .into_iter()
        .find(|p| p.person.as_ref().is_some_and(|p| p.name == "Pell Garrow"))
        .unwrap();
    let done = r
        .core
        .ontology_proposal_never(
            &OntologyProposalNeverParams {
                judgments: vec![row.judgment],
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    assert_eq!((done.rejected.len(), done.nevers.len()), (1, 1), "{done:?}");
    assert_eq!(done.nevers[0].key, "person:pell garrow");

    let before = listed().hidden;
    plant_one("pell  GARROW");
    let l = listed();
    assert!(l.proposals.iter().all(|p| p
        .person
        .as_ref()
        .is_none_or(|p| !p.name.to_lowercase().contains("pell"))));
    assert_eq!(
        l.hidden,
        before + 1,
        "the later proposal is hidden, counted"
    );

    r.core
        .ontology_never_undo(
            &OntologyNeverUndoParams {
                key: "person:pell garrow".into(),
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    assert!(listed().proposals.iter().any(|p| p
        .person
        .as_ref()
        .is_some_and(|p| p.name.to_lowercase().contains("pell"))));
    assert!(r
        .core
        .ontology_never_list(&OntologyNeverListParams {})
        .unwrap()
        .nevers
        .is_empty());
}
