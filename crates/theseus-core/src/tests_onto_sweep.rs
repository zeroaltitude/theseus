//! The ontology's nightly sweep, through a whole core (theseus-63tfk): a
//! planted duplicate pair merges and the undo restores it, and decay drops a
//! machine's old membership and keeps the owner's.

use std::sync::Arc;

use serde_json::Value;
use theseus_ontology::{Category, CategoryId, MemberList, Membership, Origin, Record};

use crate::provider::{FakeProvider, Scripted};
use crate::store::Store;
use crate::{Config, Core};

const DAY: u64 = 24 * 3_600_000;
const NOW: u64 = 100 * DAY;

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

fn entry(category: &str, origin: Origin, as_of_ms: u64) -> Membership {
    Membership {
        category: id(category),
        origin,
        confidence: (origin != Origin::Operator).then_some(0.9),
        as_of_ms,
    }
}

fn topics(session: &str, members: Vec<Membership>) -> Record {
    Record::Members(MemberList {
        session: session.into(),
        kind: "topic".into(),
        members,
    })
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

fn snapshot(core: &Core) -> Arc<theseus_ontology::Ontology> {
    core.runner.ontology.snapshot(&core.store).unwrap()
}

fn list_bytes(core: &Core, session: &str) -> Vec<u8> {
    let o = snapshot(core);
    let l = o.member_list(session, "topic").unwrap().clone();
    serde_json::to_vec(&Record::Members(l).to_value()).unwrap()
}

/// Two topics Jev minted under two spellings of one name, one session each.
fn plant_twins(core: &Core) {
    core.ontology_machine_write(
        Origin::Jev,
        "j-0001",
        vec![
            Record::Category(Category::new(
                id("topic:rust-harness"),
                "Rust Harness",
                "jev:j-0001",
            )),
            Record::Category(Category::new(
                id("topic:rust-harness-2"),
                "rust-harness",
                "jev:j-0001",
            )),
            topics("s1", vec![entry("topic:rust-harness", Origin::Jev, NOW)]),
            topics("s2", vec![entry("topic:rust-harness-2", Origin::Jev, NOW)]),
        ],
    )
    .unwrap();
}

#[test]
fn a_planted_duplicate_pair_merges_and_the_undo_restores_it() {
    let (core, _dir) = core();
    plant_twins(&core);
    let before = snapshot(&core);
    let s2_was = list_bytes(&core, "s2");

    let mut swept = None;
    let frames = frames_in(|| swept = Some(core.onto_sweep(NOW).unwrap()));
    let swept = swept.unwrap();
    assert_eq!(frames, 1, "one merge, one frame");
    assert_eq!((swept.merged, swept.decayed, swept.failed), (1, 0, 0));
    assert_eq!(swept.writes.len(), 1);

    let o = snapshot(&core);
    assert!(o.category(&id("topic:rust-harness-2")).is_none());
    let moved = o.member_list("s2", "topic").unwrap();
    assert_eq!(moved.members[0].category, id("topic:rust-harness"));
    assert_eq!(moved.members[0].origin, Origin::Sweep);
    // The row says what was merged and carries what undoes it.
    let cats = rows_of(&core, "ontology.category");
    let merged = cats
        .iter()
        .find(|r| r["id"] == "topic:rust-harness-2" && r["merged_into"] == "topic:rust-harness")
        .unwrap();
    assert_eq!(merged["origin"], "sweep");
    assert_eq!(merged["undo"]["write"], swept.writes[0].as_str());
    assert_eq!(merged["undo"]["before"]["added_by"], "jev:j-0001");

    // A second run finds nothing, and writes nothing.
    assert_eq!(
        frames_in(|| assert_eq!(core.onto_sweep(NOW).unwrap(), Default::default())),
        0
    );

    let mut keys = vec![];
    let frames = frames_in(|| {
        keys = core
            .ontology_undo(&swept.writes[0], "the CLI", "cli")
            .unwrap()
    });
    assert_eq!(frames, 1);
    assert_eq!(keys.len(), 2);
    let back = snapshot(&core);
    assert_eq!(*back, *before, "the ontology is as it was before the sweep");
    assert_eq!(list_bytes(&core, "s2"), s2_was, "the list, byte for byte");
    // A restart reads the same snapshot.
    core.runner.ontology.forget();
    assert_eq!(*snapshot(&core), *before);
}

#[test]
fn decay_drops_an_old_machine_membership_and_keeps_an_owners() {
    let (core, _dir) = core();
    let old = NOW - 31 * DAY;
    core.runner
        .ontology
        .write(
            &core.store,
            vec![
                Record::Category(Category::new(id("topic:web"), "web", "ada")),
                Record::Category(Category::new(id("topic:ops"), "ops", "ada")),
                Record::Category(Category::new(id("topic:sec"), "sec", "ada")),
                topics(
                    "s1",
                    vec![
                        entry("topic:web", Origin::Operator, old),
                        entry("topic:ops", Origin::Jev, old),
                        entry("topic:sec", Origin::Jev, NOW - DAY),
                    ],
                ),
            ],
            Origin::Operator,
            |_| Ok(vec![]),
        )
        .unwrap();
    let was = list_bytes(&core, "s1");

    let swept = core.onto_sweep(NOW).unwrap();
    assert_eq!((swept.decayed, swept.merged, swept.failed), (1, 0, 0));
    let o = snapshot(&core);
    let kept: Vec<&str> = o
        .member_list("s1", "topic")
        .unwrap()
        .members
        .iter()
        .map(|m| m.category.as_str())
        .collect();
    assert_eq!(kept, ["topic:web", "topic:sec"]);

    core.ontology_undo(&swept.writes[0], "the CLI", "cli")
        .unwrap();
    assert_eq!(list_bytes(&core, "s1"), was);
}

#[test]
fn the_dreams_first_guidance_is_undone_to_an_empty_one() {
    let (core, _dir) = core();
    core.runner
        .ontology
        .write(
            &core.store,
            vec![Record::Category(Category::new(
                id("topic:web"),
                "web",
                "ada",
            ))],
            Origin::Operator,
            |_| Ok(vec![]),
        )
        .unwrap();
    let g = theseus_ontology::Guidance::new(id("topic:web"), "Keep replies short.", 1, "dream:d-1");
    core.ontology_machine_write(Origin::Dream, "d-0001", vec![Record::Guidance(g)])
        .unwrap();
    assert!(!snapshot(&core)
        .guidance(&id("topic:web"))
        .unwrap()
        .is_empty());
    let rows = rows_of(&core, "ontology.guidance");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["undo"]["before"], Value::Null);

    core.ontology_undo("d-0001", "the CLI", "cli").unwrap();
    let o = snapshot(&core);
    let now = o.guidance(&id("topic:web")).unwrap();
    assert!(now.is_empty());
    assert_eq!(now.version, 2);
}
