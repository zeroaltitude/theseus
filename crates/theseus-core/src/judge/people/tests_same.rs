//! People whose names are one key are one person, and a recorded "not the
//! same" stops it (theseus-jrffh). Every name here is invented.

use theseus_ontology::{Category, CategoryId, Origin, Record};
use theseus_protocol::{
    OntologyCategoryAddParams, OntologyNeverListParams, OntologyNeverUndoParams,
    OntologyPersonNotSameListParams, OntologyPersonNotSameParams,
};

use crate::tests_categorize::{rig, Rig};

fn add(r: &Rig, name: &str) -> String {
    let p = OntologyCategoryAddParams {
        kind: Some("person".into()),
        name: name.into(),
        ..Default::default()
    };
    r.core.ontology_category_add(&p, "the CLI").unwrap().id
}

/// A person written past `ontology.category.add`, whose own merge would have
/// taken it in at once: how two people of one key come to exist.
fn plant(r: &Rig, name: &str) -> CategoryId {
    let o = r.core.runner.ontology.snapshot(&r.core.store).unwrap();
    let c = Category {
        id: o.mint_id(theseus_ontology::person::KIND, name).unwrap(),
        name: name.into(),
        parent: None,
        description: String::new(),
        added_by: "operator".into(),
        retired_ms: None,
        handles: Vec::new(),
        merged_into: None,
        persona: None,
    };
    let id = c.id.clone();
    r.core
        .runner
        .ontology
        .write(
            &r.core.store,
            vec![Record::Category(c)],
            Origin::Operator,
            |_| Ok(vec![]),
        )
        .unwrap();
    id
}

/// The live people whose name has `key`.
fn live(r: &Rig, key: &str) -> Vec<String> {
    let o = r.core.runner.ontology.snapshot(&r.core.store).unwrap();
    o.categories()
        .filter(|c| c.kind() == theseus_ontology::person::KIND)
        .filter(|c| c.merged_into.is_none() && c.retired_ms.is_none())
        .filter(|c| super::same::name_key(&c.name) == key)
        .map(|c| c.id.to_string())
        .collect()
}

fn not_same(a: &str, b: &str, undo: bool) -> OntologyPersonNotSameParams {
    OntologyPersonNotSameParams {
        a: a.into(),
        b: b.into(),
        undo,
        ..Default::default()
    }
}

/// The four Brams are one person, however they were added; a bare first
/// name stays its own person.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_four_brams_are_one_person_and_a_bare_first_name_is_not() {
    let r = rig(None, 0, |_| {});
    for n in ["bramteller", "bram-teller", "bram", "bram teller"] {
        add(&r, n);
    }
    // A display name alone makes a person of its own; the sweep joins them.
    assert_eq!(live(&r, "bramteller").len(), 3);
    assert_eq!(r.core.people_same_merge().unwrap().len(), 2);
    assert_eq!(
        live(&r, "bramteller").len(),
        1,
        "{:?}",
        live(&r, "bramteller")
    );
    assert_eq!(live(&r, "bram").len(), 1);
    // The merge carries the absorbed names across as handles.
    let o = r.core.runner.ontology.snapshot(&r.core.store).unwrap();
    let kept = o
        .category(&CategoryId::parse(&live(&r, "bramteller")[0]).unwrap())
        .unwrap();
    assert!(
        kept.handles.iter().any(|h| h.starts_with("name:")),
        "{:?}",
        kept.handles
    );
}

/// Two people that do not key together stay two.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn people_whose_keys_differ_are_not_merged() {
    let r = rig(None, 0, |_| {});
    add(&r, "Wren Halloway");
    add(&r, "Wren Hallowell");
    assert!(r.core.people_same_merge().unwrap().is_empty());
    assert_eq!(live(&r, "wrenhalloway").len(), 1);
    assert_eq!(live(&r, "wrenhallowell").len(), 1);
}

/// A recorded pair is not merged by the automatic path even when the keys
/// match, and it is merged once the record is undone.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_recorded_pair_is_not_auto_merged() {
    let r = rig(None, 0, |_| {});
    let a = plant(&r, "Pell Garrow");
    let b = plant(&r, "pell-garrow");
    let done = r
        .core
        .ontology_person_not_same(&not_same(&b.to_string(), &a.to_string(), false), "the CLI")
        .unwrap();
    assert_eq!(
        done.key,
        format!("pair:{}", super::same::pair_key(a.as_str(), b.as_str()))
    );
    assert!(r.core.people_same_merge().unwrap().is_empty());
    assert_eq!(live(&r, "pellgarrow").len(), 2);
    let list = r
        .core
        .ontology_person_not_same_list(&OntologyPersonNotSameListParams {})
        .unwrap();
    assert_eq!(list.pairs.len(), 1);
    // The same list is the nevers' (one record kind, one undo).
    assert_eq!(
        r.core
            .ontology_never_list(&OntologyNeverListParams {})
            .unwrap()
            .nevers
            .len(),
        1
    );
    r.core
        .ontology_never_undo(
            &OntologyNeverUndoParams {
                key: done.key,
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    assert_eq!(r.core.people_same_merge().unwrap().len(), 1);
    assert_eq!(live(&r, "pellgarrow").len(), 1);
}

/// A "not the same" of one person with itself, or of a name no one holds, is
/// refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pair_needs_two_held_people() {
    let r = rig(None, 0, |_| {});
    let a = add(&r, "Ivo Marsh");
    assert!(r
        .core
        .ontology_person_not_same(&not_same(&a, &a, false), "the CLI")
        .is_err());
    assert!(r
        .core
        .ontology_person_not_same(&not_same(&a, "person:nobody", false), "the CLI")
        .is_err());
}

/// The record is the store's: a store opened again reads the pair.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_recorded_pair_survives_a_reopen() {
    let r = rig(None, 0, |_| {});
    let a = plant(&r, "Odo Finch");
    let b = plant(&r, "odo finch");
    r.core
        .ontology_person_not_same(&not_same(a.as_str(), b.as_str(), false), "the CLI")
        .unwrap();
    // Reopen: the store's files are copied while the core is down to a
    // stopped state, and a new core on the copy reads the pair.
    let state = std::path::PathBuf::from(&r.core.runner.cfg.server.state_dir);
    let cfg = (*r.core.runner.cfg).clone();
    let copy = tempfile::tempdir().unwrap();
    let out = std::process::Command::new("cp")
        .arg("-r")
        .arg(state.join("store"))
        .arg(copy.path())
        .status()
        .unwrap();
    assert!(out.success());
    drop(r);
    let store = crate::store::Store::open(&copy.path().join("store")).unwrap();
    let again = reopened(cfg, store);
    assert_eq!(again.not_same_pairs().len(), 1);
}

/// A core on a store opened again.
fn reopened(
    cfg: crate::config::Config,
    store: crate::store::Store,
) -> std::sync::Arc<crate::rpc::Core> {
    crate::rpc::Core::build(crate::rpc::Parts::for_tests(
        cfg,
        std::sync::Arc::new(crate::provider::FakeProvider::scripted(vec![])),
        store,
    ))
    .unwrap()
}

/// The candidates read: a first name heading one full name is offered, a
/// recorded pair is left out and counted, and nothing is merged.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_candidates_offer_a_first_name_and_leave_a_recorded_pair_out() {
    use theseus_protocol::OntologyPersonCandidatesParams;
    let r = rig(None, 0, |_| {});
    let short = add(&r, "Juno");
    let long = add(&r, "Juno Pike");
    add(&r, "Quill Marsh");
    let ask = || {
        r.core
            .ontology_person_candidates(&OntologyPersonCandidatesParams::default())
            .unwrap()
    };
    let got = ask();
    assert_eq!(got.candidates.len(), 1, "{got:?}");
    assert_eq!(
        (
            got.candidates[0].a.as_str(),
            got.candidates[0].kind.as_str()
        ),
        (short.as_str(), "first_name")
    );
    assert_eq!(got.candidates[0].b_name, "Juno Pike");
    assert_eq!(live(&r, "juno").len() + live(&r, "junopike").len(), 2);
    r.core
        .ontology_person_not_same(&not_same(&short, &long, false), "the CLI")
        .unwrap();
    let got = ask();
    assert_eq!((got.candidates.len(), got.left_out), (0, 1));
}
