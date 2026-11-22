//! The threadbench's clock (theseus-98bz.3), through whole cores: a submit
//! carrying `route_at_ms` is refused unless `[threads] bench_clock` is on;
//! with it on, messages `ask -s` sends are placed by their address, at the
//! bench's gaps (the place tail reads them).

use theseus_protocol::TurnSubmitParams;

use super::tests_thread_grow::{rig, send, thread};
use crate::provider::Scripted;

/// A bench minute.
const MIN: u64 = 60_000;
/// The plan's first message, on the bench's clock.
const BASE: u64 = 1_700_000_000_000;

fn asked(session: &str, input: &str, at: u64) -> TurnSubmitParams {
    TurnSubmitParams {
        session_id: Some(session.into()),
        input: input.into(),
        via: Some("ask".into()),
        route_at_ms: Some(at),
        deadline_secs: None,
        ..TurnSubmitParams::default()
    }
}

/// Off (every real config), a submit that carries the bench's clock is
/// refused, naming the key.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_bench_clock_is_refused_unless_the_key_is_on() {
    let r = rig(|_| vec![Scripted::text("Hi.")], |_| {});
    let c = &r.core;
    let home = r.home();
    let v = send(c, asked(&home, "hello", BASE)).await;
    let msg = v["error"]["message"].as_str().unwrap_or_default();
    assert!(msg.contains("[threads] bench_clock"), "{v}");
    assert!(v.get("result").is_none() || v["result"].is_null());
}

/// On, messages `ask -s` sends are placed, at the bench's clock: one that
/// addresses a thread runs there, and one 23 bench-minutes later that names
/// none runs at home. Both are routed rows.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_bench_clock_places_ask_by_its_address() {
    let r = rig(
        |_| vec![Scripted::text("Looking."), Scripted::text("Sure.")],
        |cfg| cfg.threads.bench_clock = true,
    );
    let c = &r.core;
    thread(c, "pr-round");
    let home = r.home();
    let first = send(
        c,
        asked(&home, "#pr-round what is failing in PR 412?", BASE),
    )
    .await;
    assert_eq!(first["result"]["thread"]["slug"], "pr-round", "{first}");
    let second = send(c, asked(&home, "and the tests too", BASE + 23 * MIN)).await;
    assert_eq!(
        second["result"]["session_id"].as_str(),
        Some(home.as_str()),
        "{second}"
    );
    let routed = crate::test_support::ledger_rows_2k(c, "thread.routed");
    assert_eq!(routed.len(), 2, "both were routed");
    assert_eq!(routed[1].data["to"], "home");
}
