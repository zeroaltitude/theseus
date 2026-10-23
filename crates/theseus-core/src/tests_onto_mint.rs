//! The sweep mints a kind from `other` answers (theseus-81zx7), through a
//! whole core: three sessions' `onto.filed` rows that share a mined name make
//! one kind, one category and three memberships; two make nothing; the undo
//! takes it all back.

use std::sync::Arc;

use serde_json::json;
use theseus_protocol::{now_unix_ms, LedgerKind};

use crate::ledger::LedgerRow;
use crate::provider::{FakeProvider, Scripted};
use crate::store::Store;
use crate::{Config, Core};

fn core() -> (Arc<Core>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let model = Arc::new(FakeProvider::scripted(vec![Scripted::text("Hello.")]));
    (
        Core::build(crate::rpc::Parts::for_tests(cfg, model, store)).unwrap(),
        dir,
    )
}

/// `session`'s turn answered `other` about `names`.
fn said_other(core: &Core, session: &str, names: &[&str], confidence: f64) {
    let data = json!({"judgment": format!("jdg_{session}"), "steps": [
        {"kind": "kind_of_thing", "outcome": "other", "names": names, "confidence": confidence}]});
    let row = LedgerRow::new(LedgerKind::OntoFiled, Some(session), None, data);
    core.store.append_ledger(&row).unwrap();
}

fn snapshot(core: &Core) -> Arc<theseus_ontology::Ontology> {
    core.runner.ontology.snapshot(&core.store).unwrap()
}

#[test]
fn three_other_sessions_mint_a_kind_and_two_do_not() {
    let (core, _dir) = core();
    let now = now_unix_ms();
    said_other(&core, "s1", &["Release Notes", "Lantern"], 0.9);
    said_other(&core, "s2", &["release-notes"], 0.85);
    let swept = core.onto_sweep(now).unwrap();
    assert_eq!((swept.kinds, swept.refused, swept.writes.len()), (0, 0, 0));
    assert!(
        snapshot(&core).kind("release-notes").is_none(),
        "two do not mint"
    );

    said_other(&core, "s3", &["Release Notes"], 0.8);
    let swept = core.onto_sweep(now).unwrap();
    assert_eq!((swept.kinds, swept.failed), (1, 0));
    let write = swept.writes[0].clone();
    assert!(write.ends_with("-kind"), "{write}");
    let o = snapshot(&core);
    let kind = o.kind("release-notes").expect("the kind is minted");
    assert_eq!(kind.added_by, format!("sweep:{write}"));
    assert_eq!(kind.examples, vec!["Release Notes".to_string()]);
    assert!(
        kind.description.contains("3 sessions"),
        "{}",
        kind.description
    );
    assert_eq!(
        kind.precedence,
        o.kinds()
            .iter()
            .filter(|k| k.name != kind.name)
            .map(|k| k.precedence)
            .max()
            .unwrap()
            / 10
            * 10
            + 10
    );
    let cats: Vec<_> = o
        .categories()
        .filter(|c| c.kind() == "release-notes")
        .collect();
    let [cat] = cats.as_slice() else {
        panic!("one category: {cats:?}")
    };
    assert_eq!(cat.name, "Release Notes");
    for s in ["s1", "s2", "s3"] {
        let l = o.member_list(s, "release-notes").expect("a membership");
        assert_eq!(l.members.len(), 1);
        assert_eq!(l.members[0].category, cat.id);
    }

    // A second run writes nothing: the kind lists the name.
    let again = core.onto_sweep(now).unwrap();
    assert_eq!((again.kinds, again.writes.len()), (0, 0));

    // The undo retires the kind, its category and the memberships.
    core.ontology_undo(&write, "test", "test").unwrap();
    let o = snapshot(&core);
    assert!(o.kind("release-notes").unwrap().retired_ms.is_some());
    assert!(o.categories().all(|c| c.kind() != "release-notes"));
    assert!(o
        .member_list("s1", "release-notes")
        .is_none_or(|l| l.members.is_empty()));
    // And an undone kind is not minted again.
    let after = core.onto_sweep(now).unwrap();
    assert_eq!((after.kinds, after.writes.len()), (0, 0));
}

#[test]
fn a_name_that_is_no_kind_name_mints_nothing_and_is_counted() {
    let (core, _dir) = core();
    let now = now_unix_ms();
    // A name that starts with a digit, and the table's own `topic`.
    for s in ["s1", "s2", "s3"] {
        said_other(&core, s, &["2026 plan"], 0.9);
    }
    for s in ["s4", "s5", "s6"] {
        said_other(&core, s, &["topic"], 0.9);
    }
    let swept = core.onto_sweep(now).unwrap();
    assert_eq!((swept.kinds, swept.refused), (0, 2));
    assert!(swept.writes.is_empty());
}

fn said(sessions: &[&str], name: &str) -> Vec<crate::judge::onto::mint::Said> {
    sessions
        .iter()
        .map(|s| crate::judge::onto::mint::Said {
            session: s.to_string(),
            names: vec![name.into()],
            confidence: 0.9,
        })
        .collect()
}

/// KEY (theseus-e3pxa): three names one session each are asked pair by pair,
/// and two yeses (`a~b`, `b~c`) chain them into the one kind.
#[test]
fn a_chain_of_singletons_clusters() {
    use crate::judge::onto::mint::{folds, kind_pairs, plan_with, Hints};
    let (core, _dir) = core();
    let o = snapshot(&core);
    let none = Default::default();
    let mut all = said(&["s1"], "Alpha Ledger");
    all.extend(said(&["s2"], "Beta Ledger"));
    all.extend(said(&["s3"], "Gamma Ledger"));
    let pairs = kind_pairs(&o, &folds(&all), &none);
    let pair = |a: &str, b: &str| (a.to_string(), b.to_string());
    assert!(
        pairs.contains(&pair("alphaledger", "betaledger")),
        "{pairs:?}"
    );
    assert!(
        pairs.contains(&pair("betaledger", "gammaledger")),
        "{pairs:?}"
    );
    let now = now_unix_ms();
    let hints = Hints {
        same: vec![
            pair("alphaledger", "betaledger"),
            pair("betaledger", "gammaledger"),
        ],
        ..Hints::default()
    };
    let plan = plan_with(&o, &all, now, |n| format!("sweep-{now}-{n}-kind"), &hints);
    assert_eq!((plan.minted.len(), plan.minted[0].sessions), (1, 3));
    assert!(kind_pairs(&o, &folds(&said(&["s1", "s2"], "Alpha Ledger")), &none).is_empty());
}

/// Two names that are each a cluster are asked whether they are one kind
/// before both mint.
#[test]
fn two_clusters_are_asked_before_both_mint() {
    use crate::judge::onto::mint::{folds, kind_pairs, plan_with, Hints};
    let (core, _dir) = core();
    let o = snapshot(&core);
    let none = Default::default();
    let mut all = said(&["s1", "s2", "s3"], "Lantern Ledger");
    all.extend(said(&["s4", "s5", "s6"], "Ledger App"));
    let pairs = kind_pairs(&o, &folds(&all), &none);
    assert_eq!(
        pairs,
        vec![("lanternledger".to_string(), "ledgerapp".to_string())]
    );
    let now = now_unix_ms();
    let write = |n: usize| format!("sweep-{now}-{n}-kind");
    assert_eq!(
        plan_with(&o, &all, now, write, &Hints::default())
            .minted
            .len(),
        2
    );
    let hints = Hints {
        same: pairs,
        ..Hints::default()
    };
    assert_eq!(plan_with(&o, &all, now, write, &hints).minted.len(), 1);
}

/// One session each under the names `a`, `b`, ...: `Alpha Ledger` is `alphaledger`.
fn singletons(names: &[&str]) -> Vec<crate::judge::onto::mint::Said> {
    let mut all = Vec::new();
    for (n, name) in names.iter().enumerate() {
        all.extend(said(&[&format!("s{n}")], name));
    }
    all
}

fn kept(pairs: &[(&str, &str)]) -> std::collections::BTreeSet<(String, String)> {
    pairs
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

/// O3 (theseus-3327o): two names one session each can never make a cluster,
/// so no pair is asked.
#[test]
fn two_isolated_singletons_ask_nothing() {
    use crate::judge::onto::mint::{folds, kind_pairs};
    let (core, _dir) = core();
    let all = singletons(&["Alpha Ledger", "Beta Ledger"]);
    assert!(kind_pairs(&snapshot(&core), &folds(&all), &kept(&[])).is_empty());
}

/// Four singletons, one kept yes `a~b`: `a~c` (and `b~c`) reach a cluster,
/// `c~d` is two sessions with no kept yes of its own, so it is not asked.
#[test]
fn a_pair_that_cannot_reach_a_cluster_is_not_asked() {
    use crate::judge::onto::mint::{folds, kind_pairs};
    let (core, _dir) = core();
    let all = singletons(&[
        "Alpha Ledger",
        "Beta Ledger",
        "Gamma Ledger",
        "Delta Ledger",
    ]);
    let pairs = kind_pairs(
        &snapshot(&core),
        &folds(&all),
        &kept(&[("alphaledger", "betaledger")]),
    );
    let has = |a: &str, b: &str| pairs.contains(&(a.to_string(), b.to_string()));
    assert!(has("alphaledger", "gammaledger"), "{pairs:?}");
    assert!(has("betaledger", "deltaledger"), "{pairs:?}");
    assert!(!has("deltaledger", "gammaledger"), "{pairs:?}");
    assert!(
        !has("alphaledger", "betaledger"),
        "joined already: {pairs:?}"
    );
}
