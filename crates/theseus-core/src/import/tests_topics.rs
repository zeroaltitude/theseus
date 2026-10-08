//! A tag's topics (theseus-anh3): a store of a few hundred imported
//! sessions with made-up topic labels gives the tree (a topic for each
//! label and each prefix) and each session's memberships, origin `import`,
//! at most three, an implied ancestor left out first and then the first in
//! the pipeline's order; a second run writes nothing; the erase empties the
//! erased sessions' lists and takes away the topics nothing else uses (one
//! with guidance, one another tag's sessions hold, and one the operator
//! declared are kept), and a restart reads the same; the operator's own
//! membership is kept and counts first; a label no topic is made of stays
//! a label; a stop ends a run between frames and a rerun finishes it; a
//! kinds row that does not name the import is refused with its fix; and
//! `import.topics` is the owner's, from a private place.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::json;
use theseus_ontology::{
    Category, CategoryId, Guidance, Kind, MemberList, Membership, Ontology, Origin, Record,
};
use theseus_protocol::import::{
    ImportEpisodesParams, ImportEraseResult, ImportLine, ImportTopicsResult,
};
use theseus_protocol::{method, OntologyListParams};
use theseus_store::Store as _;

use super::tests::{call, call_as};
use super::topics::{self, plan, Plan};
use super::{episode, session_id_of, write};
use crate::approval::{Client, Surface};
use crate::config::MemoryMode;
use crate::tests_recall::{rig_with, Rig};
use crate::Core;

const TAG: &str = "tern-2026-02";
const OTHER: &str = "tern-2026-03";

/// The made-up labels, by a session's number mod 6: one topic; two; an
/// ancestor beside its descendant; five (capped); none; and one written
/// twice in two cases beside another.
fn labels_of(i: usize) -> Vec<&'static str> {
    match i % 6 {
        0 => vec!["garden/beds"],
        1 => vec!["harbor/tides", "orchard"],
        2 => vec!["garden", "garden/beds/soil"],
        3 => vec![
            "orchard/apples",
            "kiln/glaze",
            "harbor/boats/sails",
            "garden/sheds",
            "orchard/pears",
        ],
        4 => vec![],
        _ => vec!["garden/beds", "Garden/Beds", "orchard/pears"],
    }
}

/// What a session of number `i` should hold, by the rule, written out.
fn expected(i: usize) -> Vec<&'static str> {
    match i % 6 {
        0 => vec!["garden/beds"],
        1 => vec!["harbor/tides", "orchard"],
        2 => vec!["garden/beds/soil"],
        3 => vec!["orchard/apples", "kiln/glaze", "harbor/boats/sails"],
        4 => vec![],
        _ => vec!["garden/beds", "orchard/pears"],
    }
}

/// Every topic the labels make: each label and each prefix.
const TREE: [&str; 13] = [
    "garden",
    "garden/beds",
    "garden/beds/soil",
    "garden/sheds",
    "harbor",
    "harbor/boats",
    "harbor/boats/sails",
    "harbor/tides",
    "kiln",
    "kiln/glaze",
    "orchard",
    "orchard/apples",
    "orchard/pears",
];

fn episode_line(tag: &str, i: usize, topics: &[&str]) -> String {
    let mut v = json!({
        "format": 1, "import_tag": tag, "episode_id": format!("ep_{:064x}", 0x7e44_0000 + i),
        "source": episode::SOURCES[i % episode::SOURCES.len()], "agent": null,
        "place": {"kind": "dm", "name": "wren"},
        "as_of": {"start": "2026-02-03T09:00:00Z", "end": "2026-02-03T09:20:00Z"},
        "labels": {"sensitivity": "personal", "topic": topics},
        "summary": null,
        "messages": [{"idx": 0, "time": "2026-02-03T09:00:00Z", "author": "wren",
            "integrity": "operator", "text": format!("Tern count {i}: nine on the spit."),
            "unit": format!("unit-{i}"), "sha256": "cd".repeat(32)}],
    });
    v["hash"] = json!(episode::hash_of(&v));
    serde_json::to_string(&v).unwrap()
}

fn sid(i: usize) -> String {
    session_id_of(&format!("ep_{:064x}", 0x7e44_0000 + i))
}

/// Import `n` sessions of `tag` from number `from`, each labelled by `labels`.
fn import(
    core: &Core,
    tag: &str,
    from: usize,
    n: usize,
    labels: impl Fn(usize) -> Vec<&'static str>,
) {
    let lines: Vec<ImportLine> = (from..from + n)
        .map(|i| ImportLine {
            line: i as u64 + 1,
            text: episode_line(tag, i, &labels(i)),
        })
        .collect();
    let r = write::import_batch(
        &core.store,
        &ImportEpisodesParams {
            file: "terns.jsonl".into(),
            lines,
        },
        "test",
    )
    .unwrap();
    assert!(r.rejected.is_empty(), "{:?}", r.rejected);
}

const N: usize = 300;

/// The rig: kept whole by its test, since it holds the store's directory.
fn rig() -> Rig {
    rig_with(MemoryMode::Canary, |c| c.memory.canary_fraction = 1.0)
}

fn snapshot(c: &Core) -> Arc<Ontology> {
    c.runner.ontology.snapshot(&c.store).unwrap()
}

/// Each topic's path, by its id, read up the tree.
fn paths(o: &Ontology) -> BTreeMap<CategoryId, String> {
    o.categories()
        .filter(|c| c.kind() == "topic")
        .map(|c| {
            let p: Vec<String> = o
                .path(&c.id)
                .iter()
                .map(|c| c.name.to_lowercase())
                .collect();
            (c.id.clone(), p.join("/"))
        })
        .collect()
}

/// A session's topic memberships as paths, with their origins.
fn held(o: &Ontology, session: &str) -> Vec<(String, Origin)> {
    let p = paths(o);
    o.memberships(session)
        .iter()
        .map(|m| (p[&m.category].clone(), m.origin))
        .collect()
}

fn id_of(o: &Ontology, path: &str) -> CategoryId {
    paths(o)
        .into_iter()
        .find(|(_, p)| p == path)
        .unwrap_or_else(|| panic!("no topic {path}"))
        .0
}

async fn run(c: &Arc<Core>, tag: &str) -> ImportTopicsResult {
    serde_json::from_value(
        call(c, method::IMPORT_TOPICS, json!({"tag": tag}))
            .await
            .unwrap(),
    )
    .unwrap()
}

fn operator_write(c: &Core, r: Record) {
    c.runner
        .ontology
        .write(&c.store, vec![r], Origin::Operator, |_| Ok(vec![]))
        .unwrap();
}

/// The labels make the tree, each label's topic under its prefix's; each
/// session holds what the rule says; the counts add up, and `ontology.list`
/// counts each topic's sessions, without its memberships when asked so.
#[tokio::test]
async fn a_tags_labels_make_a_tree_and_each_session_holds_at_most_three() {
    let rig = rig();
    let c = &rig.core;
    import(c, TAG, 0, N, labels_of);
    let r = run(c, TAG).await;
    let o = snapshot(c);
    let mut got: Vec<String> = paths(&o).into_values().collect();
    let mut want: Vec<String> = TREE.iter().map(|s| s.to_string()).collect();
    want.sort();
    got.sort();
    assert_eq!(got, want, "a topic for each label and each prefix");
    for id in paths(&o).keys() {
        let cat = o.category(id).unwrap();
        assert_eq!(cat.added_by, topics::made_by(TAG));
        assert_eq!(o.path(id).len(), paths(&o)[id].split('/').count());
    }
    let mut memberships = 0;
    for i in 0..N {
        let want: Vec<(String, Origin)> = expected(i)
            .iter()
            .map(|p| (p.to_string(), Origin::Import))
            .collect();
        assert_eq!(held(&o, &sid(i)), want, "session {i}");
        memberships += want.len() as u64;
    }
    assert_eq!(
        (
            r.sessions,
            r.labels,
            r.topics,
            r.made,
            r.joined,
            r.memberships,
            r.capped,
            r.unplaced
        ),
        (
            N as u64,
            10,
            13,
            13,
            (N - N / 6) as u64,
            memberships,
            (N / 6) as u64,
            0
        ),
        "{r:?}"
    );
    assert!(r.frames >= 1);

    let list = c
        .ontology_list(&OntologyListParams {
            session_id: None,
            memberships: Some(false),
        })
        .unwrap();
    assert!(list.memberships.is_empty(), "the tree alone");
    let full = c.ontology_list(&OntologyListParams::default()).unwrap();
    assert_eq!(full.memberships.len() as u64, memberships);
    let counted: u64 = list.categories.iter().map(|c| c.members).sum();
    assert_eq!(counted, memberships, "each topic counts its sessions");
    let beds = list
        .categories
        .iter()
        .find(|x| x.id == id_of(&o, "garden/beds").as_str())
        .unwrap();
    assert_eq!(beds.members, (N / 6 * 2) as u64, "classes 0 and 5");
    assert!(full.memberships.iter().all(|m| m.origin == "import"));
}

/// A second run reads the same labels and writes nothing: no topic, no
/// list, no frame, and the store's last position stays where it was.
#[tokio::test]
async fn a_second_run_changes_nothing() {
    let rig = rig();
    let c = &rig.core;
    import(c, TAG, 0, N, labels_of);
    run(c, TAG).await;
    let before = (snapshot(c), c.store.inner().last_position());
    let r = run(c, TAG).await;
    assert_eq!((r.made, r.joined, r.frames), (0, 0, 0), "{r:?}");
    assert_eq!(r.topics, 13);
    assert_eq!(c.store.inner().last_position(), before.1, "nothing written");
    assert_eq!(*snapshot(c), *before.0);
    // A restart reads what the first run wrote, and a run after it still
    // writes nothing.
    c.runner.ontology.forget();
    assert_eq!(*snapshot(c), *before.0, "a restart reads the same");
    assert_eq!(run(c, TAG).await.frames, 0);
}

/// The erase empties the erased sessions' lists and takes away the topics
/// the import made that nothing uses. Kept: one with guidance and its
/// parent; one another tag's sessions hold, and its parent; and the
/// operator's own topic the import used. A restart reads the same.
#[tokio::test]
async fn the_erase_takes_back_the_memberships_and_the_topics_nothing_uses() {
    let rig = rig();
    let c = &rig.core;
    // The operator declared `harbor` before the import: the import uses it.
    let harbor = Category {
        id: CategoryId::new("topic", "harbor").unwrap(),
        name: "harbor".into(),
        parent: None,
        description: String::new(),
        added_by: "the operator".into(),
        retired_ms: None,
    };
    operator_write(c, Record::Category(harbor.clone()));
    import(c, TAG, 0, N, labels_of);
    import(c, OTHER, N, 12, |_| vec!["kiln/glaze"]);
    run(c, TAG).await;
    let r = run(c, OTHER).await;
    assert_eq!(
        (r.made, r.joined),
        (0, 12),
        "the other tag's topic was made"
    );
    let o = snapshot(c);
    assert_eq!(
        id_of(&o, "harbor"),
        harbor.id,
        "the operator's topic is used"
    );
    let sheds = id_of(&o, "garden/sheds");
    operator_write(
        c,
        Record::Guidance(Guidance::new(
            sheds.clone(),
            "Sheds hold the tools.",
            1,
            "the operator",
        )),
    );

    let e: ImportEraseResult = serde_json::from_value(
        call(c, method::IMPORT_ERASE, json!({"tag": TAG}))
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(e.sessions, N as u64);
    assert_eq!(e.memberships, (N - N / 6) as u64, "{e:?}");
    let o = snapshot(c);
    for i in 0..N {
        assert!(o.memberships(&sid(i)).is_empty(), "erased session {i}");
    }
    for i in N..N + 12 {
        assert_eq!(
            held(&o, &sid(i)),
            vec![("kiln/glaze".into(), Origin::Import)]
        );
    }
    let mut kept: Vec<String> = paths(&o).into_values().collect();
    kept.sort();
    assert_eq!(
        kept,
        ["garden", "garden/sheds", "harbor", "kiln", "kiln/glaze"],
        "only what something uses"
    );
    assert_eq!(e.topics, 8, "{e:?}");
    c.runner.ontology.forget();
    assert_eq!(*snapshot(c), *o, "a restart reads the same");

    // The erase again takes nothing more; the other tag's erase takes the
    // rest it made, and the operator's topic stays.
    let again: ImportEraseResult = serde_json::from_value(
        call(c, method::IMPORT_ERASE, json!({"tag": TAG}))
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!((again.memberships, again.topics), (0, 0));
    call(c, method::IMPORT_ERASE, json!({"tag": OTHER}))
        .await
        .unwrap();
    let mut left: Vec<String> = paths(&snapshot(c)).into_values().collect();
    left.sort();
    assert_eq!(left, ["garden", "garden/sheds", "harbor"]);
}

/// A membership the operator gave an imported session is kept, and counts
/// toward the three first; the import fills the rest, and the session
/// counts as capped.
#[tokio::test]
async fn the_operators_membership_is_kept_and_counts_first() {
    let rig = rig();
    let c = &rig.core;
    let pier = Category {
        id: CategoryId::new("topic", "pier").unwrap(),
        name: "pier".into(),
        parent: None,
        description: String::new(),
        added_by: "the operator".into(),
        retired_ms: None,
    };
    operator_write(c, Record::Category(pier.clone()));
    import(c, TAG, 3, 1, labels_of);
    operator_write(
        c,
        Record::Members(MemberList {
            session: sid(3),
            kind: "topic".into(),
            members: vec![Membership::operator(pier.id.clone(), 1)],
        }),
    );
    let r = run(c, TAG).await;
    assert_eq!((r.joined, r.capped, r.memberships), (1, 1, 2), "{r:?}");
    let o = snapshot(c);
    assert_eq!(
        held(&o, &sid(3)),
        vec![
            ("pier".into(), Origin::Operator),
            ("orchard/apples".into(), Origin::Import),
            ("kiln/glaze".into(), Origin::Import),
        ]
    );
    assert_eq!(run(c, TAG).await.frames, 0, "and again nothing");
}

/// A membership the operator appends after the import leaves the list as it
/// is at the next run: its order is kept, so nothing is written again.
#[tokio::test]
async fn a_membership_appended_after_the_run_is_kept_in_its_place() {
    let rig = rig();
    let c = &rig.core;
    import(c, TAG, 0, 1, labels_of);
    run(c, TAG).await;
    let pier = Category {
        id: CategoryId::new("topic", "pier").unwrap(),
        name: "pier".into(),
        parent: None,
        description: String::new(),
        added_by: "the operator".into(),
        retired_ms: None,
    };
    operator_write(c, Record::Category(pier.clone()));
    let mut l = snapshot(c).member_list(&sid(0), "topic").unwrap().clone();
    l.members.push(Membership::operator(pier.id.clone(), 2));
    operator_write(c, Record::Members(l));
    let last = c.store.inner().last_position();
    let r = run(c, TAG).await;
    assert_eq!((r.joined, r.frames), (0, 0), "{r:?}");
    assert_eq!(c.store.inner().last_position(), last);
    assert_eq!(
        held(&snapshot(c), &sid(0)),
        vec![
            ("garden/beds".into(), Origin::Import),
            ("pier".into(), Origin::Operator)
        ]
    );
}

/// The rule, pure: once each in any case, an implied ancestor out, then
/// the first `max`; a label with an empty part, or deeper than the tree
/// nests, is unplaced.
#[test]
fn a_sessions_plan_follows_the_rule() {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<String>>();
    let p = |v: &[&str]| {
        v.iter()
            .map(|x| s(&x.split('/').collect::<Vec<_>>()))
            .collect()
    };
    assert_eq!(
        plan(&s(&["a/b", "a", "c", "A/B", "d", "e"]), 3),
        Plan {
            taken: p(&["a/b", "c", "d"]),
            capped: true,
            unplaced: 0
        }
    );
    assert_eq!(
        plan(&s(&["a//b", "", "a/b/c/d/e/f/g/h/i", " x / y "]), 3),
        Plan {
            taken: p(&["x/y"]),
            capped: false,
            unplaced: 3
        }
    );
    assert_eq!(
        plan(&s(&["k", "k/l", "k/l/m"]), 1),
        Plan {
            taken: p(&["k/l/m"]),
            capped: false,
            unplaced: 0
        }
    );
}

/// A label no topic is made of stays a label: counted, and no topic made.
#[tokio::test]
async fn a_label_no_topic_is_made_of_stays_a_label() {
    let rig = rig();
    let c = &rig.core;
    import(c, TAG, 0, 2, |_| vec!["dune//grass", "dune/grass"]);
    let r = run(c, TAG).await;
    assert_eq!((r.topics, r.unplaced, r.memberships), (2, 2, 2), "{r:?}");
    let rec = c
        .store
        .get_session::<crate::session::SessionRecord>(&sid(0))
        .unwrap()
        .unwrap();
    assert_eq!(
        rec.imported.unwrap().labels.topic.len(),
        2,
        "the labels stay"
    );
}

/// A stop ends a run between two frames, with what it wrote held; the rerun
/// finishes it, and the store then holds what one run writes.
#[tokio::test]
async fn a_stop_ends_a_run_between_frames_and_a_rerun_finishes_it() {
    let rig = rig();
    let c = &rig.core;
    import(c, TAG, 0, 60, labels_of);
    let last = c.store.inner().last_position();
    // Past the first frame's write, the stop is seen.
    let stopping = || c.store.inner().last_position() > last;
    let (r, stopped) =
        topics::assign_in(&c.store, &c.runner.ontology, TAG, "test", stopping, 7).unwrap();
    assert!(stopped, "{r:?}");
    assert_eq!(r.frames, 1);
    let mid = snapshot(c);
    assert!(mid.categories().any(|c| c.kind() == "topic"));
    let (r2, stopped) =
        topics::assign_in(&c.store, &c.runner.ontology, TAG, "test", || false, 7).unwrap();
    assert!(!stopped);
    assert_eq!(r2.made, 6, "the first frame's seven were held: {r2:?}");
    let o = snapshot(c);
    for i in 0..60 {
        let want: Vec<(String, Origin)> = expected(i)
            .iter()
            .map(|p| (p.to_string(), Origin::Import))
            .collect();
        assert_eq!(held(&o, &sid(i)), want, "session {i}");
    }
    assert_eq!(run(c, TAG).await.frames, 0);
}

/// A kinds row the operator wrote that does not name the import refuses
/// the run with its fix, and nothing is written.
#[tokio::test]
async fn a_kind_row_without_the_import_refuses_the_run_with_its_fix() {
    let rig = rig();
    let c = &rig.core;
    import(c, TAG, 0, 6, labels_of);
    let mut k: Kind = snapshot(c).kind("topic").unwrap().clone();
    k.assigned_by = vec![Origin::Operator];
    k.version += 1;
    k.added_by = "the operator".into();
    operator_write(c, Record::Kind(k));
    let last = c.store.inner().last_position();
    let e = call(c, method::IMPORT_TOPICS, json!({"tag": TAG}))
        .await
        .unwrap_err();
    assert!(e.1.contains("add `import` to its assigned_by"), "{e:?}");
    assert_eq!(c.store.inner().last_position(), last);
}

/// `import.topics` is the owner's, from a private place: a connection no
/// listener named is refused, and no topic or membership is written (the
/// refusal's own ledger row is).
#[tokio::test]
async fn topics_from_no_private_place_are_refused() {
    let rig = rig();
    let c = &rig.core;
    import(c, TAG, 0, 6, labels_of);
    let e = call_as(
        c,
        Client::new("test", Surface::Unnamed),
        method::IMPORT_TOPICS,
        json!({"tag": TAG}),
    )
    .await
    .unwrap_err();
    assert_eq!(e.0, theseus_protocol::error_code::REFUSED, "{e:?}");
    assert!(!snapshot(c).categories().any(|c| c.kind() == "topic"));
}
