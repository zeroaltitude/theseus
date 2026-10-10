//! The owner's gate on `people.v1`'s live point (theseus-u5n8, "combine,
//! gated by Jev"), with a fake model and a fake Jev: under the gate no
//! model is called, the mark still moves, and a held person Jev finds
//! involved is a proposal the owner accepts (the owner's own person never
//! listed); over it, exactly one extraction runs. Every name is invented.

use std::time::{Duration, Instant};

use serde_json::json;
use theseus_judge::fake::{FakeJev, Scripted as Jev};
use theseus_protocol::{
    OntologyCategoryAddParams, OntologyProposalAcceptParams, OntologyProposalRejectParams,
    OntologyProposalsParams,
};

use super::live::MARK_PREFIX;
use super::seen::SEEN_SCOPE;
use super::tests::{answer, people_on};
use crate::judge::categorize::{Mark, EVERY};
use crate::ledger::LedgerRow;
use crate::store::Store;
use crate::tests_categorize::{moorings, rig, session, turn, OWNER};
use crate::Core;

fn hold(core: &Core, name: &str, handle: &str) {
    core.ontology_category_add(
        &OntologyCategoryAddParams {
            kind: Some("person".into()),
            name: name.into(),
            handles: vec![handle.into()],
            ..Default::default()
        },
        "the CLI",
    )
    .unwrap();
}

fn rows(store: &Store, scope: &str, kind: &str) -> Vec<LedgerRow> {
    store
        .scope_after(scope, 0)
        .unwrap()
        .into_iter()
        .filter_map(|r| r.decode::<LedgerRow>().ok())
        .filter(|r| r.kind == kind)
        .collect()
}

async fn until<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let t0 = Instant::now();
    loop {
        if let Some(t) = f() {
            return t;
        }
        assert!(t0.elapsed() < Duration::from_secs(20), "waited for {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn mark(core: &Core, sid: &str) -> Option<Mark> {
    core.store.get_meta(&format!("{MARK_PREFIX}{sid}")).unwrap()
}

/// Under the gate: one Jev call and no model call; the mark moves; Orrin,
/// held and named, is proposed for the session and accepted; the owner's
/// own person, named too, is never listed, nor a held person not named.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn under_the_gate_no_model_is_called_and_a_held_person_is_proposed() {
    let jev = FakeJev::start().unwrap();
    jev.script("unlisted", Jev::Noul(0.2));
    jev.script("seen", Jev::Noul(0.93));
    let r = rig(Some(&jev), EVERY, people_on);
    hold(&r.core, "Orrin Vale", "slack:U0ORRIN1");
    hold(&r.core, "Sable Thorn", &format!("discord:{OWNER}"));
    hold(&r.core, "Marisol Tern", "slack:U0TIDE11");
    let sid = session(&r.core, None);
    for i in 0..EVERY {
        turn(
            &r.core,
            &sid,
            &format!("Sable Thorn here: ask Orrin about mooring line {i}."),
        )
        .await;
    }
    let calls = until("the gate's judgment", || {
        let c = rows(&r.core.store, SEEN_SCOPE, "judge.call");
        (!c.is_empty()).then_some(c)
    })
    .await;
    assert_eq!(calls.len(), 1, "one Jev call at the due point");
    until("the mark", || mark(&r.core, &sid)).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        r.fake.requests().len(),
        EVERY,
        "under the gate no model call: the turns' alone"
    );
    assert!(rows(&r.core.store, super::SCOPE, "people.extracted").is_empty());
    let asked: Vec<String> = jev
        .seen()
        .iter()
        .filter(|s| s.body.to_string().contains("people_seen.v1/"))
        .map(|s| s.body["questions"].to_string())
        .collect();
    assert_eq!(asked.len(), 1);
    assert!(asked[0].contains("Orrin Vale"), "{}", asked[0]);
    assert!(
        !asked[0].contains("Sable Thorn"),
        "the owner's own person is never listed: {}",
        asked[0]
    );
    assert!(!asked[0].contains("Marisol Tern"), "not named: not listed");

    let all = r
        .core
        .ontology_proposals(&OntologyProposalsParams::default())
        .unwrap()
        .proposals;
    assert_eq!(all.len(), 1, "{all:?}");
    let p = &all[0];
    assert_eq!(p.session_id, sid);
    assert_eq!(p.topic.as_deref(), Some("person:orrin-vale"));
    assert_eq!(
        p.judgment,
        format!("{}/orrin-vale", calls[0].data["id"].as_str().unwrap())
    );
    let person = p.person.as_ref().unwrap();
    assert_eq!((person.name.as_str(), person.new), ("Orrin Vale", false));
    assert_eq!(p.band, "act");
    r.core
        .ontology_proposal_accept(
            &OntologyProposalAcceptParams {
                judgment: p.judgment.clone(),
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    let o = r.core.runner.ontology.snapshot(&r.core.store).unwrap();
    assert!(o
        .memberships(&sid)
        .iter()
        .any(|m| m.category.to_string() == "person:orrin-vale"));
    let left = r
        .core
        .ontology_proposals(&OntologyProposalsParams::default())
        .unwrap()
        .proposals;
    assert!(left.is_empty(), "answered: {left:?}");
}

/// Over the gate: exactly one extraction for the exchange, and people.v1
/// judges what it found; its role line is kept only when Jev cleared it
/// (an evaluative one, or one Jev did not answer of, is dropped).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn over_the_gate_exactly_one_extraction_runs() {
    let jev = FakeJev::start().unwrap();
    jev.script("unlisted", Jev::Noul(0.9));
    jev.script("real", Jev::Noul(0.95));
    jev.script("involved", Jev::Noul(0.93));
    jev.script("evaluative", Jev::Noul(0.05));
    jev.script(
        "match",
        Jev::Choice {
            option: "new_person".into(),
            confidence: 0.92,
        },
    );
    let r = rig(Some(&jev), EVERY, people_on);
    r.fake.script.lock().unwrap().push_back(answer(json!([
        {"name": "Wren Halloway", "handles": [], "role_line": "Takes the north gauge readings.",
         "evidence": ["L1"]}
    ])));
    let sid = session(&r.core, None);
    moorings(&r.core, &sid, EVERY).await;
    until("the extraction's row", || {
        let x = rows(&r.core.store, super::SCOPE, "people.extracted");
        (!x.is_empty()).then_some(x)
    })
    .await;
    let calls = until("people.v1's judgment", || {
        let j = rows(&r.core.store, super::SCOPE, "judge.call");
        (!j.is_empty()).then_some(j)
    })
    .await;
    let mut j: theseus_judge::Judgment = serde_json::from_value(calls[0].data.clone()).unwrap();
    let line = |j: &theseus_judge::Judgment| super::decide(j, 0.9, 0.6).unwrap().role_line;
    assert_eq!(line(&j).as_deref(), Some("Takes the north gauge readings."));
    for a in j.answers.iter_mut().filter(|a| a.question == "evaluative") {
        a.answer = theseus_judge::Answer::Noul { noul: 0.91 };
    }
    assert_eq!(line(&j), None, "an evaluative line is dropped");
    j.answers.retain(|a| a.question != "evaluative");
    assert_eq!(line(&j), None, "a line Jev did not answer of is dropped");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        r.fake.requests().len(),
        EVERY + 1,
        "the turns and exactly one extraction"
    );
    assert_eq!(rows(&r.core.store, SEEN_SCOPE, "judge.call").len(), 1);
    assert!(mark(&r.core, &sid).is_some());
}

/// The gate asks at every due point: a person the owner rejected for a
/// session is not proposed there again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_person_rejected_for_a_session_is_not_proposed_there_again() {
    let jev = FakeJev::start().unwrap();
    jev.script("unlisted", Jev::Noul(0.2));
    jev.script("seen", Jev::Noul(0.93));
    let r = rig(Some(&jev), 2 * EVERY, people_on);
    hold(&r.core, "Orrin Vale", "slack:U0ORRIN1");
    let sid = session(&r.core, None);
    let proposals = || {
        r.core
            .ontology_proposals(&OntologyProposalsParams::default())
            .unwrap()
            .proposals
    };
    for round in 1..=2 {
        for i in 0..EVERY {
            turn(&r.core, &sid, &format!("Ask Orrin about line {round}.{i}.")).await;
        }
        until("the gate's judgment", || {
            (rows(&r.core.store, SEEN_SCOPE, "judge.call").len() == round).then_some(())
        })
        .await;
        if round == 1 {
            let all = proposals();
            assert_eq!(all.len(), 1, "{all:?}");
            r.core
                .ontology_proposal_reject(
                    &OntologyProposalRejectParams {
                        judgment: all[0].judgment.clone(),
                        note: Some("not this session's".into()),
                        ..Default::default()
                    },
                    "the CLI",
                )
                .unwrap();
        }
    }
    assert!(proposals().is_empty(), "rejected once: {:?}", proposals());
    assert_eq!(r.fake.requests().len(), 2 * EVERY, "no model call");
}

/// The owner is never a candidate by name: the held person holding the
/// owner's handle lends its name to the exclusions, whatever the lines say.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_owners_held_name_is_never_a_candidate() {
    let r = rig(None, 0, people_on);
    hold(&r.core, "Sable Thorn", &format!("discord:{OWNER}"));
    hold(&r.core, "Orrin Vale", "slack:U0ORRIN1");
    let o = r.core.runner.ontology.snapshot(&r.core.store).unwrap();
    let not = super::NotPeople::of(&r.core.runner.cfg, &[]).with_held(&o);
    let cand = |name: &str| theseus_judge::builders::PersonCandidate {
        name: name.into(),
        handles: vec![],
        role_line: String::new(),
        evidence: vec![],
    };
    assert!(not.excludes(&cand("sable  THORN")), "the owner's held name");
    assert!(!not.excludes(&cand("Orrin Vale")));
}
