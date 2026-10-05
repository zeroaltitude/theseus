//! `memory.v1` and `attribution.v1` in shadow (M6 step 31a), against the
//! fake Jev: the pass hands each node it labels to `memory.v1` and a
//! recall whose turn has its reply to `attribution.v1`; each judgment is a
//! `judge.call` row of the sink's, scoped to its pack, its state in a blob
//! holding only what the session's model saw. With `[judge]` off, or a
//! pack's sample at 0, nothing is asked, and the labels are written all the
//! same. Health lists both packs among the wired.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use theseus_judge::fake::{FakeJev, Scripted as Jev};

use super::tests::Fixed;
use crate::config::JudgePackConfig;
use crate::config::MemoryMode;
use crate::ledger::LedgerRow;
use crate::provider::Scripted;
use crate::secrets::{Secret, SecretBoard};
use crate::tests_recall::{index_of, session, turn};
use crate::Core;

/// A canary core (every session on `baseline`) with the judge on the fake
/// Jev, its key ready, and a stand-in index for the pass.
fn rig(jev: &FakeJev, tweak: impl FnOnce(&mut crate::Config)) -> crate::tests_recall::Rig {
    let board = SecretBoard::new(["jev_api_key".to_string()], Instant::now());
    board.publish(
        BTreeMap::from([(
            "jev_api_key".to_string(),
            Ok(Secret::new("jev-test-key-0123456789".into())),
        )]),
        "test",
    );
    let base = jev.base();
    crate::tests_recall::rig_with_secrets(MemoryMode::Live, board, |c| {
        c.judge.enabled = true;
        c.judge.api_base = base;
        c.judge.connect_secs = 1;
        c.judge.total_secs = 1;
        c.memory.recall_deadline_ms = 2000;
        tweak(c);
    })
}

/// The rows of `scope`, decoded.
fn rows(c: &Core, scope: &str) -> Vec<LedgerRow> {
    c.store
        .scope_after(scope, 0)
        .unwrap()
        .into_iter()
        .filter(|r| r.kind == theseus_store::kinds::LEDGER)
        .map(|r| r.decode::<LedgerRow>().unwrap())
        .collect()
}

/// A judgment's state, from its blob.
fn state_of(c: &Core, row: &LedgerRow) -> Value {
    let digest = row.data["state"]["sha256"].as_str().unwrap();
    serde_json::from_slice(&std::fs::read(c.store.blobs().path(digest)).unwrap()).unwrap()
}

async fn until(what: &str, f: impl Fn() -> bool) {
    let t0 = Instant::now();
    while !f() {
        assert!(t0.elapsed() < Duration::from_secs(20), "never: {what}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// A turn whose live recall admitted a note: the pass's `memory.v1`
/// judgments, one per node it labeled, and one `attribution.v1` judgment
/// with a `relied_on` Noul per note, each in shadow and from the judge's
/// own budget, its state holding the texts the model saw.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_pass_asks_memory_v1_and_attribution_v1_in_shadow() {
    let jev = FakeJev::start().unwrap();
    jev.script(
        "kind",
        Jev::Choice {
            option: "fact".into(),
            confidence: 0.93,
        },
    );
    let r = rig(&jev, |_| {});
    let c = &r.core;
    c.runner.pass.set_index(Arc::new(Fixed {
        mode: Mutex::new("hybrid".into()),
        ..Fixed::default()
    }));
    let a = session(c, None, &["The kestrel survey runs every second Thursday."]);
    let q = session(c, None, &[]);
    c.runner.memory.set_ask(index_of(c, vec![a.clone()]));
    r.model
        .script
        .lock()
        .unwrap()
        .push_back(Scripted::text("It runs every second Thursday."));
    let res = turn(c, &q, "When does the kestrel survey run?").await;
    assert_eq!(res.recalled, 1);
    let memory = || rows(c, "judge:memory");
    let attribution = || rows(c, "judge:attribution");
    until("memory.v1 of the question and the reply", || {
        memory().len() >= 2
    })
    .await;
    until("attribution.v1 of the recall", || !attribution().is_empty()).await;
    let m = memory();
    for row in &m {
        let d = &row.data;
        assert_eq!(d["pack"], "memory.v1");
        assert_eq!(
            (d["mode"].as_str(), d["budget"].as_str()),
            (Some("shadow"), Some("shadow"))
        );
        assert_eq!(d["outcome"]["outcome"], "answered", "{d}");
        assert_eq!(d["context"]["class"], "memory");
        assert!(d["cost_micros"].as_u64().unwrap() > 0);
        assert_eq!(row.session_id.as_deref(), Some(q.as_str()));
    }
    let asked: Vec<&str> = m
        .iter()
        .filter_map(|r| r.data["context"]["node"].as_str())
        .collect();
    let labeled: Vec<String> = rows(c, &crate::fact::memory::scope(&q))
        .iter()
        .filter(|r| r.kind == "memory.labeled")
        .map(|r| r.data["node_id"].as_str().unwrap().to_string())
        .collect();
    for n in &labeled {
        assert!(asked.contains(&n.as_str()), "{n} was labeled but not asked");
    }
    let answers = m[0].data["answers"].as_array().unwrap().clone();
    let kind = answers.iter().find(|x| x["question"] == "kind").unwrap();
    assert_eq!(kind["answer"]["choice"], "fact");
    // Its state: the node's text, and nothing it did not see.
    let state = state_of(c, &m[0]);
    assert!(
        state["text"].as_str().is_some_and(|t| !t.is_empty()),
        "{state}"
    );
    assert!(!state.to_string().contains("jev-test-key"));

    let a1 = &attribution()[0].data;
    assert_eq!(a1["pack"], "attribution.v1");
    assert_eq!(a1["mode"], "shadow");
    let notes: Vec<&str> = a1["context"]["notes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let source = c.store.session_nodes(&a).unwrap()[0].1.id.clone();
    assert_eq!(notes, [source.as_str()]);
    let relied: Vec<&Value> = a1["answers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|x| {
            x["question"]
                .as_str()
                .is_some_and(|q| q.starts_with("relied_on"))
        })
        .collect();
    assert_eq!(relied.len(), 1, "{a1}");
    let state = state_of(c, &attribution()[0]);
    assert_eq!(state["message"], "When does the kestrel survey run?");
    assert_eq!(state["reply"], "It runs every second Thursday.");
    // Health lists the packs this build wires.
    let h = c.health().judge.unwrap();
    assert!(
        h.packs.contains(&"memory.v1: shadow".to_string()),
        "{:?}",
        h.packs
    );
    assert!(h.packs.contains(&"attribution.v1: shadow".to_string()));
}

/// With `[judge]` off, and with a pack's sample at 0, nothing is asked;
/// the deterministic labels are written either way.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_the_judge_the_deterministic_half_runs_alone() {
    let jev = FakeJev::start().unwrap();
    for off in ["judge", "sample"] {
        let r = rig(&jev, |c| match off {
            "judge" => c.judge.enabled = false,
            _ => {
                // Every other wired pack off too, so Jev hears nothing from
                // this turn (main's shadow packs ask at their points).
                for (p, _) in crate::judge::WIRED {
                    c.judge.packs.insert(
                        (*p).into(),
                        JudgePackConfig {
                            mode: Some(crate::config::PackMode::Off),
                            sample: None,
                            notices: None,
                        },
                    );
                }
                for p in ["memory.v1", "attribution.v1"] {
                    c.judge.packs.insert(
                        p.into(),
                        JudgePackConfig {
                            mode: None,
                            sample: Some(0.0),
                            notices: None,
                        },
                    );
                }
            }
        });
        let c = &r.core;
        c.runner.memory.set_ask(index_of(c, vec![]));
        let q = session(c, None, &[]);
        turn(c, &q, "The heron hide opens at dawn.").await;
        let labeled = || {
            rows(c, &crate::fact::memory::scope(&q))
                .iter()
                .filter(|r| r.kind == "memory.labeled")
                .count()
        };
        until("the labels", || labeled() >= 2).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(rows(c, "judge:memory").is_empty(), "{off}");
        assert!(rows(c, "judge:attribution").is_empty(), "{off}");
    }
    assert_eq!(jev.seen().len(), 0, "Jev was never called");
}
