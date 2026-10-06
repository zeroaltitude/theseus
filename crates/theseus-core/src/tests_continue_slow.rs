//! A slow Jev never delays the turn at the compile point (M5 25b;
//! theseus-bhn2): `continue.v1`'s judgment is spawned at a compile, and the
//! turn goes on. Proved by order, never by the turn's duration: once the
//! fake has seen the continue.v1 request, the turn's answer is awaited, and
//! at that moment the judgment has not settled, however fast the machine.
//!
//! The margin: Jev answers after 30 s and the client gives up after 8 s, so
//! the judgment settles 8 s after its request, and the turn's rest (a model
//! stand-in's answer and its frames) has to take less than that: on a
//! machine that starved a turn for 8 s, this test fails and says so.

use std::time::{Duration, Instant};

use theseus_judge::fake::{FakeJev, FakeMode};
use theseus_store::Record;

use crate::config::{PackMode, SignalsConfig};
use crate::ledger::LedgerRow;
use crate::store::Store;
use crate::tests_judge::{off, rig_with, texts, turn};

/// Jev's delay, and what the client waits for it.
const SLOW: Duration = Duration::from_secs(30);
const TOTAL_SECS: u64 = 8;

fn continue_rows(store: &Store) -> Vec<(Record, LedgerRow)> {
    store
        .scope_after("judge:continue", 0)
        .unwrap()
        .into_iter()
        .map(|r| {
            let row: LedgerRow = r.decode().unwrap();
            (r, row)
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_slow_jev_never_delays_the_turn_at_the_compile_point() {
    let jev = FakeJev::start().unwrap();
    let r = rig_with(texts(2), Some(&jev), |c| {
        c.judge.signals = SignalsConfig {
            dormancy_minutes: 0,
            ..SignalsConfig::default()
        };
        // continue.v1 alone judges.
        for (pack, _) in crate::judge::WIRED {
            if *pack != crate::judge::compile::CONTINUE_PACK {
                c.judge.packs.insert((*pack).into(), off());
            }
        }
        c.judge.packs.insert(
            crate::judge::compile::CONTINUE_PACK.into(),
            crate::config::JudgePackConfig {
                mode: Some(PackMode::Shadow),
                ..off()
            },
        );
        c.judge.total_secs = TOTAL_SECS;
    });
    let first = turn(&r.core, None, "Say done.").await;
    jev.set_mode(FakeMode::Slow(SLOW));
    tokio::time::sleep(Duration::from_millis(30)).await;
    let core = r.core.clone();
    let sid = first.session_id.clone();
    let second = tokio::spawn(async move { turn(&core, Some(&sid), "And again.").await });

    // The fake has the continue.v1 request: the judgment is on its way.
    let t0 = Instant::now();
    while !jev
        .seen()
        .iter()
        .any(|s| s.body.to_string().contains("continue.v1"))
    {
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "no continue.v1 request"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    // The turn answers while it is out.
    let res = tokio::time::timeout(Duration::from_secs(20), second)
        .await
        .expect("the turn answered")
        .unwrap();
    assert_eq!(res.session_id, first.session_id);
    let h = r.core.health().judge.unwrap();
    assert_eq!(
        h.calls_today, 0,
        "the judgment had settled before the answer"
    );
    assert!(
        continue_rows(&r.core.store).is_empty(),
        "its row was written before the answer"
    );

    // Then, on events, its row: the client's timeout, usage unknown.
    let t0 = Instant::now();
    let rows = loop {
        let rows = continue_rows(&r.core.store);
        if !rows.is_empty() {
            break rows;
        }
        assert!(t0.elapsed() < Duration::from_secs(60), "no continue.v1 row");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert_eq!(
        rows[0].1.data["outcome"]["outcome"], "failed",
        "{:?}",
        rows[0].1.data
    );
    assert_eq!(rows[0].1.data["outcome"]["class"], "timeout");
    assert_eq!(rows[0].1.turn_id.as_deref(), Some(res.turn_id.as_str()));
}
