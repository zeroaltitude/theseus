//! Accepting a new person's proposal by name joins the person holding that
//! name as a `name:` handle (theseus-7mow): any of a person's name handles,
//! not only the display name. A person holding three name handles takes a
//! proposal under each, one at a time and in a row's bulk yes, and no
//! duplicate is made; a name two live people hold is refused with both ids,
//! the single accept and the bulk yes alike, and nothing is written. Every
//! name here is invented.

use serde_json::json;
use theseus_ontology::CategoryId;
use theseus_protocol::{
    OntologyCategoryAddParams, OntologyProposalAcceptAllParams, OntologyProposalAcceptParams,
    OntologyProposalsParams,
};

use super::tests::until_rows;
use super::tests_groups::{plant, Plant};
use super::tests_sweep::shut_gate;
use crate::ledger::LedgerRow;
use crate::tests_categorize::{session, Rig};

fn add(r: &Rig, name: &str, handles: &[&str]) {
    let p = OntologyCategoryAddParams {
        kind: Some("person".into()),
        name: name.into(),
        handles: handles.iter().map(|h| h.to_string()).collect(),
        ..Default::default()
    };
    r.core.ontology_category_add(&p, "the CLI").unwrap();
}

fn people_named(r: &Rig, word: &str) -> Vec<String> {
    let onto = r.core.runner.ontology.snapshot(&r.core.store).unwrap();
    let mut out: Vec<String> = onto
        .categories()
        .filter(|c| c.kind() == theseus_ontology::person::KIND && c.name.contains(word))
        .map(|c| c.id.to_string())
        .collect();
    out.sort();
    out
}

/// A real people.v1 judgment (the sweep's) to plant the others from.
async fn template(r: &Rig) -> LedgerRow {
    r.fake
        .script
        .lock()
        .unwrap()
        .push_back(super::tests::answer(json!([
            {"name": "Wren Halloway", "handles": [], "role_line": "", "evidence": ["L1"]}
        ])));
    r.core.people_sweep("nightly").await.unwrap();
    until_rows(&r.core.store, "judge.call", 1).await.remove(0)
}

/// A session and its new-person proposal for each name, planted from `template`.
fn proposed(r: &Rig, template: &LedgerRow, names: &[&str]) -> Vec<(String, String)> {
    let ps: Vec<Plant> = names
        .iter()
        .map(|n| Plant {
            sid: session(&r.core, None),
            name: n.to_string(),
            confidence: 0.95,
            whom: "new_person",
        })
        .collect();
    plant(r, template, &ps);
    let listed = r
        .core
        .ontology_proposals(&OntologyProposalsParams {
            limit: Some(u32::MAX),
            ..Default::default()
        })
        .unwrap();
    ps.iter()
        .map(|p| {
            let l = listed
                .proposals
                .iter()
                .find(|l| l.session_id == p.sid)
                .expect("listed");
            (p.sid.clone(), l.judgment.clone())
        })
        .collect()
}

fn holds(r: &Rig, sid: &str, id: &CategoryId) -> bool {
    let onto = r.core.runner.ontology.snapshot(&r.core.store).unwrap();
    onto.memberships(sid).iter().any(|m| &m.category == id)
}

fn accept_one(r: &Rig, judgment: &str) -> anyhow::Result<()> {
    let p = OntologyProposalAcceptParams {
        judgment: judgment.into(),
        ..Default::default()
    };
    r.core.ontology_proposal_accept(&p, "the CLI").map(|_| ())
}

fn accept_all(
    r: &Rig,
    judgment: &str,
    as_person: Option<&str>,
) -> theseus_protocol::OntologyProposalAcceptAllResult {
    let p = OntologyProposalAcceptAllParams {
        judgments: vec![judgment.into()],
        as_person: as_person.map(str::to_string),
        ..Default::default()
    };
    r.core.ontology_proposal_accept_all(&p, "the CLI").unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_name_handle_of_a_held_person_takes_the_proposal() {
    let (_jev, r, _private, _shared) = shut_gate(2.0).await;
    add(
        &r,
        "Ada Quill",
        &["name:Addie Quill", "name:A. Quill", "name:Ada Q"],
    );
    let ada = CategoryId::new("person", "ada-quill").unwrap();
    let t = template(&r).await;
    let ps = proposed(
        &r,
        &t,
        &["Addie Quill", "a.  quill", "Ada Q", "Ada Quill", "Ada Q"],
    );
    // Each name handle, and the display name, joins Ada one at a time.
    for (sid, judgment) in &ps[..4] {
        accept_one(&r, judgment).unwrap();
        assert!(holds(&r, sid, &ada), "{judgment} joined Ada");
    }
    assert_eq!(
        people_named(&r, "Quill"),
        ["person:ada-quill"],
        "no duplicate"
    );
    assert_eq!(people_named(&r, "Ada Q"), ["person:ada-quill"]);
    // The grouped accept, `as_person` a name handle's holder.
    let by = accept_all(&r, &ps[4].1, Some("Addie Quill"));
    assert_eq!(by.accepted.len(), 1, "{by:?}");
    assert!(
        holds(&r, &ps[4].0, &ada),
        "accepted as a name handle's holder"
    );
    assert_eq!(people_named(&r, "Quill"), ["person:ada-quill"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_name_two_live_people_hold_is_refused_with_both_ids() {
    let (_jev, r, _private, _shared) = shut_gate(2.0).await;
    add(&r, "Lark Penn", &["name:Larkin"]);
    add(&r, "Larkin Moss", &["name:Larkin"]);
    let t = template(&r).await;
    let ps = proposed(&r, &t, &["Larkin", "Larkin", "Larkin"]);
    let both = |s: &str| s.contains("person:lark-penn") && s.contains("person:larkin-moss");
    let e = accept_one(&r, &ps[0].1).unwrap_err();
    assert!(both(&format!("{e:#}")), "{e:#}");
    // The same in the grouped accept, by the judgment and by `as_person`.
    for as_person in [None, Some("Larkin")] {
        let bulk = accept_all(&r, &ps[1].1, as_person);
        assert!(bulk.accepted.is_empty(), "{bulk:?}");
        assert!(bulk.left.iter().any(|l| both(l)), "{bulk:?}");
    }
    // Never picked, never declared: still the two people, and no membership.
    assert_eq!(
        people_named(&r, "Larkin"),
        ["person:larkin-moss"],
        "no third person named Larkin"
    );
    for (sid, _) in &ps {
        for id in ["lark-penn", "larkin-moss"] {
            assert!(!holds(&r, sid, &CategoryId::new("person", id).unwrap()));
        }
    }
}
