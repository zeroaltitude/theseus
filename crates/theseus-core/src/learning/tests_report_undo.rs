//! The stored learning report carries the ontology pack's undo line
//! (theseus-2tqqs): `onto_undo::undo_rates` over the 7 days to the run, the one
//! definition (theseus-ukmbn), on `onto.v1`'s `PackReport` alone.

use serde_json::json;
use theseus_ontology::{Category, CategoryId, MemberList, Membership, Origin, Record};
use theseus_protocol::judge_runs::UndoRate;
use theseus_protocol::now_unix_ms;

use crate::tests_judge::{off, rig_on, texts};
use crate::tests_learning::{call_row, judgment};
use crate::Core;

fn id(s: &str) -> CategoryId {
    CategoryId::parse(s).unwrap()
}

/// Jev's write `write`: a topic and one session's membership of it.
fn jev_files(core: &Core, write: &str, topic: &str, session: &str) {
    let cat = id(&format!("topic:{topic}"));
    core.ontology_machine_write(
        Origin::Jev,
        write,
        vec![
            Record::Category(Category::new(cat.clone(), topic, format!("jev:{write}"))),
            Record::Members(MemberList {
                session: session.into(),
                kind: "topic".into(),
                members: vec![Membership {
                    category: cat,
                    origin: Origin::Jev,
                    confidence: Some(0.9),
                    as_of_ms: now_unix_ms(),
                }],
            }),
        ],
    )
    .unwrap();
}

/// Two of Jev's writes, one undone: `onto.v1`'s report says topic 1 of 2
/// (0.5), and a pack that is not the ontology's has no `undo`, nor the key in
/// its JSON.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_learning_report_carries_the_onto_undo_line() {
    let r = rig_on(texts(0), None, |c| {
        c.judge.enabled = true;
        c.judge.api_base = "http://127.0.0.1:9".into();
        for (p, _) in crate::judge::WIRED {
            c.judge.packs.insert((*p).into(), off());
        }
        c.secrets
            .entry("jev_api_key".into())
            .or_insert_with(|| "env:THESEUS_TEST_JEV".into());
    });
    let c = &r.core;
    let now = now_unix_ms();
    let calls = [
        judgment("jdg_a", "onto.v1", vec![], json!({}), 10),
        judgment("jdg_l", "loop.v1", vec![], json!({}), 10),
    ];
    let rows: Vec<_> = calls.iter().map(|j| call_row(j, now - 1_000)).collect();
    c.store.append(&rows).unwrap();
    jev_files(c, "jdg_a", "kelp", "ses_a");
    jev_files(c, "jdg_b", "tides", "ses_b");
    c.ontology_undo("jdg_b", "test", "test").unwrap();
    let rep = c
        .run_learning(now_unix_ms() + 1_000, "on_demand", |_| {})
        .unwrap();
    let pack = |name: &str| rep.packs.iter().find(|p| p.pack == name).unwrap();
    assert_eq!(
        pack("onto.v1").undo,
        [UndoRate {
            kind: "topic".into(),
            writes: 2,
            undone: 1,
            rate: Some(0.5),
        }]
    );
    assert_eq!(pack("onto.v1").undo_days, 7, "the window rides the report");
    let other = pack("loop.v1");
    assert!(other.undo.is_empty());
    assert!(serde_json::to_value(other).unwrap().get("undo").is_none());
}

/// A store that cannot read the undo rows leaves the line out and the rest
/// of the report stands (theseus-efb8i).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_undo_read_leaves_the_rest_of_the_learning_report() {
    let r = rig_on(texts(0), None, |c| {
        c.judge.enabled = true;
        c.judge.api_base = "http://127.0.0.1:9".into();
        for (p, _) in crate::judge::WIRED {
            c.judge.packs.insert((*p).into(), off());
        }
        c.secrets
            .entry("jev_api_key".into())
            .or_insert_with(|| "env:THESEUS_TEST_JEV".into());
    });
    let c = &r.core;
    let now = now_unix_ms();
    let j = judgment("jdg_a", "onto.v1", vec![], json!({}), 10);
    c.store.append(&[call_row(&j, now - 1_000)]).unwrap();
    jev_files(c, "jdg_a", "kelp", "ses_a");
    c.store.fail_ledger_pages();
    let rep = c
        .run_learning(now_unix_ms() + 1_000, "on_demand", |_| {})
        .unwrap();
    let p = rep.packs.iter().find(|p| p.pack == "onto.v1").unwrap();
    assert_eq!(p.calls, 1, "the pack's own numbers stand");
    assert!(p.undo.is_empty());
}
