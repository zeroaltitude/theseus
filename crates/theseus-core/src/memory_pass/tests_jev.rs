//! The memory pass asks the judge nothing (D8a: its shadow judgments, `memory.v1` and
//! `attribution.v1`, are retired): with a judge on the fake Jev and every wired pack off, the
//! pass's own labels are written and Jev hears nothing.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use theseus_judge::fake::FakeJev;

use crate::config::JudgePackConfig;
use crate::config::MemoryMode;
use crate::ledger::LedgerRow;
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
        // A call that is made is not cut short by the clock: the test asserts none is made.
        c.judge.total_secs = 30;
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

async fn until(what: &str, f: impl Fn() -> bool) {
    let t0 = Instant::now();
    while !f() {
        assert!(t0.elapsed() < Duration::from_secs(60), "never: {what}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// With the judge on and every wired pack off, a turn's pass writes its labels and Jev is never
/// called: the pass's labels and attribution are its own, and nothing of it waits on the judge.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_pass_labels_without_asking_the_judge() {
    let jev = FakeJev::start().unwrap();
    let r = rig(&jev, |c| {
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
    assert_eq!(jev.seen().len(), 0, "Jev was never called");
}
