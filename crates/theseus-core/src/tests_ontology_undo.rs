//! A machine origin's write and its undo, through a whole core (onto-core,
//! theseus-e3mjc).

use std::sync::Arc;

use serde_json::Value;
use theseus_ontology::{Category, CategoryId, MemberList, Membership, Origin, Record, Refusal};

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

fn id(s: &str) -> CategoryId {
    CategoryId::parse(s).unwrap()
}

fn rows_of(core: &Core, kind: &str) -> Vec<Value> {
    core.store
        .ledger_tail::<crate::ledger::LedgerRow>(100_000)
        .unwrap()
        .into_iter()
        .filter(|(_, r)| r.is_kind(kind))
        .map(|(_, r)| r.data)
        .collect()
}

fn frames_in(f: impl FnOnce()) -> u64 {
    let before = theseus_store::frames_written_here();
    f();
    theseus_store::frames_written_here() - before
}

/// An operator's write straight to the board, as a past session's.
fn operator_writes(core: &Core, records: Vec<Record>) {
    core.runner
        .ontology
        .write(&core.store, records, Origin::Operator, |_| Ok(vec![]))
        .unwrap();
}

fn entry(category: &str, origin: Origin) -> Membership {
    Membership {
        category: id(category),
        origin,
        confidence: (origin != Origin::Operator).then_some(0.9),
        as_of_ms: 1_000,
    }
}

fn topics(session: &str, members: Vec<Membership>) -> Record {
    Record::Members(MemberList {
        session: session.into(),
        kind: "topic".into(),
        members,
    })
}

fn bytes(core: &Core, session: &str) -> Vec<u8> {
    let o = core.runner.ontology.snapshot(&core.store).unwrap();
    serde_json::to_vec(
        &Record::Members(o.member_list(session, "topic").unwrap().clone()).to_value(),
    )
    .unwrap()
}

/// The operator's topic list for `s1`, and a topic the operator made.
fn earlier(core: &Core) {
    operator_writes(
        core,
        vec![
            Record::Category(Category::new(id("topic:web"), "web", "ada")),
            topics("s1", vec![entry("topic:web", Origin::Operator)]),
        ],
    );
}

fn jev_write(core: &Core) -> Result<(), anyhow::Error> {
    core.ontology_machine_write(
        Origin::Jev,
        "j-0001",
        vec![
            Record::Category(Category::new(id("topic:billing"), "billing", "jev:j-0001")),
            topics(
                "s1",
                vec![
                    entry("topic:web", Origin::Operator),
                    entry("topic:billing", Origin::Jev),
                ],
            ),
        ],
    )
}

#[test]
fn a_machine_write_is_undone_byte_for_byte_in_one_frame() {
    let (core, _dir) = core();
    earlier(&core);
    let was = bytes(&core, "s1");
    assert_eq!(frames_in(|| jev_write(&core).unwrap()), 1);
    let o = core.runner.ontology.snapshot(&core.store).unwrap();
    assert!(o.category(&id("topic:billing")).is_some());
    assert_eq!(o.member_list("s1", "topic").unwrap().members.len(), 2);
    // Its rows carry what undoes it: the earlier list, and "none" for the mint.
    let cats = rows_of(&core, "ontology.category");
    let minted = cats.iter().find(|r| r["id"] == "topic:billing").unwrap();
    assert_eq!(minted["undo"]["before"], Value::Null);
    assert_eq!(minted["undo"]["write"], "j-0001");
    assert_eq!(minted["origin"], "jev");

    let mut keys = vec![];
    let frames = frames_in(|| keys = core.ontology_undo("j-0001", "the CLI", "cli").unwrap());
    assert_eq!(
        frames, 1,
        "the inverse records and the undo row are one frame"
    );
    assert_eq!(keys.len(), 2);
    assert_eq!(bytes(&core, "s1"), was, "the earlier list, byte for byte");
    let o = core.runner.ontology.snapshot(&core.store).unwrap();
    assert!(
        o.category(&id("topic:billing")).is_none(),
        "the mint is retired"
    );
    let undone = rows_of(&core, "ontology.undo");
    assert_eq!(undone.len(), 1);
    assert_eq!(undone[0]["label"], "wrong");
    assert_eq!(undone[0]["write"], "j-0001");
    // The undo is the operator's, and a restart reads the same snapshot.
    core.runner.ontology.forget();
    let again = core.runner.ontology.snapshot(&core.store).unwrap();
    assert_eq!(*again, *o);
    // A write is undone once.
    let e = core.ontology_undo("j-0001", "the CLI", "cli").unwrap_err();
    assert!(e.to_string().contains("undone already"), "{e:#}");
}

#[test]
fn an_undo_of_a_mint_another_session_now_uses_is_refused_and_writes_nothing() {
    let (core, _dir) = core();
    earlier(&core);
    jev_write(&core).unwrap();
    core.ontology_machine_write(
        Origin::Jev,
        "j-0002",
        vec![topics("s2", vec![entry("topic:billing", Origin::Jev)])],
    )
    .unwrap();
    let frames = frames_in(|| {
        let e = core.ontology_undo("j-0001", "the CLI", "cli").unwrap_err();
        let refusal = e.downcast_ref::<Refusal>().unwrap();
        assert!(matches!(refusal, Refusal::InUse { .. }), "{e:#}");
    });
    assert_eq!(frames, 0);
    assert!(rows_of(&core, "ontology.undo").is_empty());
    // The later write undoes cleanly, and then the first can go.
    core.ontology_undo("j-0002", "the CLI", "cli").unwrap();
    core.ontology_undo("j-0001", "the CLI", "cli").unwrap();
}

#[test]
fn a_machine_write_that_drops_the_operators_entry_is_refused_and_writes_nothing() {
    let (core, _dir) = core();
    earlier(&core);
    let frames = frames_in(|| {
        let e = core
            .ontology_machine_write(Origin::Jev, "j-0003", vec![topics("s1", vec![])])
            .unwrap_err();
        assert!(e.to_string().contains("topic:web"), "{e:#}");
    });
    assert_eq!(frames, 0);
    assert!(core
        .ontology_machine_write(Origin::Operator, "x", vec![])
        .is_err());
}
