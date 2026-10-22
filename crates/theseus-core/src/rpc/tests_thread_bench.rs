//! The threadbench's clock (theseus-98bz.3), through whole cores: a submit
//! carrying `route_at_ms` is refused unless `[threads] bench_clock` is on;
//! with it on, messages `ask -s` sends are placed, at the bench's gaps, so
//! stickiness fades as it would between real messages.

use serde_json::Value;
use theseus_protocol::threads::{ThreadEditParams, WorkingSet};
use theseus_protocol::TurnSubmitParams;

use super::tests_thread_grow::{cli, rig, send, thread};
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

/// On, two messages `ask -s` sends 23 bench-minutes apart are both routed;
/// the second's sticky thread scores what 23 minutes leave of stickiness,
/// under 0.15, not the 0.88 of a message sent right after.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_bench_clock_routes_ask_at_the_plans_gaps() {
    let r = rig(
        |_| vec![Scripted::text("Looking."), Scripted::text("Sure.")],
        |cfg| cfg.threads.bench_clock = true,
    );
    let c = &r.core;
    thread(c, "pr-round");
    c.thread_edit(
        &ThreadEditParams {
            slug: "pr-round".into(),
            working_set: Some(WorkingSet {
                refs: vec!["#412".into()],
                ..Default::default()
            }),
            ..Default::default()
        },
        cli(),
    )
    .unwrap();
    let home = r.home();
    let first = send(c, asked(&home, "what is failing in PR 412?", BASE)).await;
    assert_eq!(first["result"]["thread"]["slug"], "pr-round", "{first}");
    send(c, asked(&home, "and the tests too", BASE + 23 * MIN)).await;
    let routed = crate::test_support::ledger_rows_2k(c, "thread.routed");
    assert_eq!(routed.len(), 2, "both were routed");
    let pre: &Value = routed[1].data["pre"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["thread"] == "pr-round")
        .unwrap();
    let score = pre["score"].as_f64().unwrap();
    assert!(score < 0.15, "23 minutes leave little stickiness: {score}");
}
