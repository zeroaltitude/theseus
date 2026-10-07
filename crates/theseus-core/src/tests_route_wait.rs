//! `route.v1`'s verdict without a wait on every message (theseus-ddbi),
//! against the fake Jev with the latencies it is given:
//!
//! - `route.v1` asks in a request of its own beside the batch of
//!   `classify.v1` and `role.v1`, so its verdict comes when its one question
//!   is answered, never after the batch's; each judgment is recorded once,
//!   with its own request's cost.
//! - Jev's connections open once the socket serves (`Core::warm_judge`),
//!   never on the start path, and are kept warm, so a fresh daemon's first
//!   message pays no connection setup.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use serde_json::Value;
use theseus_judge::fake::FakeJev;
use theseus_judge::price::JevPrice;
use theseus_judge::Usage;

use crate::tests_judge::kinds;
use crate::tests_route::{decided, mode, rig, turn, until_calls};

/// Wait (on the runtime's timer) until the store holds `n` `judge.call`
/// rows, then give the sink a moment more to write any extra.
async fn until_judged(store: &crate::store::Store, n: usize) -> Vec<Value> {
    let t0 = Instant::now();
    loop {
        let rows: Vec<Value> = kinds(store, "judge.call")
            .into_iter()
            .map(|r| r.data)
            .collect();
        if rows.len() >= n {
            tokio::time::sleep(Duration::from_millis(300)).await;
            return kinds(store, "judge.call")
                .into_iter()
                .map(|r| r.data)
                .collect();
        }
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "{} of {n} judgments",
            rows.len()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// A request's question ids.
fn asked(body: &Value) -> BTreeSet<String> {
    body["questions"]
        .as_object()
        .map(|q| q.keys().cloned().collect())
        .unwrap_or_default()
}

/// What the fake bills a request: per question, its state's tokens and 40
/// in, and 30 out (`fake::answers`).
fn billed(body: &Value) -> Usage {
    let n = asked(body).len() as u64;
    let state = body["state"].to_string().len().div_ceil(4) as u64;
    Usage {
        input_tokens: n * (state + 40),
        output_tokens: n * 30,
    }
}

/// (b): `route.v1` alone in one request, `classify.v1` and `role.v1` in
/// the other; three judgments, each recorded once; `route.v1`'s cost is its
/// whole request's, and the batch's two share theirs by question count.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn route_asks_alone_beside_the_batch_and_each_judgment_is_recorded_once() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "sophisticated", 0.95);
    let r = rig(Some(&jev), 1, |_| {});
    let res = turn(&r.core, None, "Weigh two designs for the log.", None).await;
    assert_eq!(res.route.as_ref().unwrap().reason, "verdict");
    until_calls(&jev, 2).await;
    let rows = until_judged(&r.core.store, 3).await;
    let by_pack: BTreeMap<String, usize> = rows.iter().fold(BTreeMap::new(), |mut m, j| {
        *m.entry(j["pack"].as_str().unwrap().to_string())
            .or_default() += 1;
        m
    });
    assert_eq!(
        by_pack,
        BTreeMap::from([
            ("classify.v1".to_string(), 1),
            ("role.v1".to_string(), 1),
            ("route.v1".to_string(), 1)
        ]),
        "each judgment once"
    );
    let seen = jev.seen();
    assert_eq!(seen.len(), 2);
    let route_req = seen
        .iter()
        .find(|s| asked(&s.body).contains("route.v1/mode"))
        .unwrap();
    let batch_req = seen
        .iter()
        .find(|s| !asked(&s.body).contains("route.v1/mode"))
        .unwrap();
    assert_eq!(asked(&route_req.body).len(), 1, "route.v1's one question");
    assert_eq!(
        route_req.body["state"], batch_req.body["state"],
        "one state"
    );
    let price = JevPrice::jev_1_13_0();
    let route = rows.iter().find(|j| j["pack"] == "route.v1").unwrap();
    let batch: Vec<&Value> = rows.iter().filter(|j| j["pack"] != "route.v1").collect();
    assert_eq!(
        (
            route["call"]["packs"].as_u64(),
            route["call"]["questions"].as_u64()
        ),
        (Some(1), Some(1))
    );
    assert_ne!(route["call"]["id"], batch[0]["call"]["id"], "two calls");
    assert_eq!(batch[0]["call"]["id"], batch[1]["call"]["id"]);
    assert_eq!(batch[0]["call"]["packs"], 2);
    let cost = |j: &Value| j["cost_micros"].as_u64().unwrap();
    let tokens = |j: &Value, k: &str| j["usage"][k].as_u64().unwrap();
    let u = billed(&route_req.body);
    assert_eq!(
        (
            tokens(route, "input_tokens"),
            tokens(route, "output_tokens")
        ),
        (u.input_tokens, u.output_tokens)
    );
    assert_eq!(cost(route), price.cost_micros(&u), "route.v1's whole call");
    let u = billed(&batch_req.body);
    let sum = |k: &str| batch.iter().map(|j| tokens(j, k)).sum::<u64>();
    assert_eq!(
        (sum("input_tokens"), sum("output_tokens")),
        (u.input_tokens, u.output_tokens)
    );
    assert_eq!(
        batch.iter().map(|j| cost(j)).sum::<u64>(),
        price.cost_micros(&u),
        "the batch's call, split between its two"
    );
}

/// With (b), `route.v1`'s verdict reaches the turn before the batch's
/// answer: the batch takes 3 s, `route.v1`'s request none, and the turn
/// routes on its verdict without waiting for the batch.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_verdict_reaches_the_turn_before_the_batch_answers() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "sophisticated", 0.95);
    jev.set_latency(Duration::ZERO, Duration::from_secs(3));
    let r = rig(Some(&jev), 1, |c| c.judge.total_secs = 30);
    let t0 = Instant::now();
    let res = turn(&r.core, None, "Weigh two designs for the log.", None).await;
    let took = t0.elapsed();
    assert_eq!(res.route.as_ref().unwrap().reason, "verdict");
    assert_eq!(res.profile, "opus");
    let d = &decided(&r.core.store)[0];
    assert!(
        d["wait_ms"].as_u64().unwrap() < 1500,
        "no wait for the batch: {d}"
    );
    assert!(took < Duration::from_millis(2500), "{took:?}: {d}");
    assert!(
        kinds(&r.core.store, "judge.call")
            .iter()
            .all(|j| j.data["pack"] == "route.v1"),
        "the batch had not answered"
    );
}

/// Wait (on the runtime's timer) until `cond`, at most 20 s.
async fn until(what: &str, cond: impl Fn() -> bool) {
    let t0 = Instant::now();
    while !cond() {
        assert!(t0.elapsed() < Duration::from_secs(20), "{what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Step 2: the start path (`Core::build`, all of it before the socket
/// serves) opens no connection to Jev; the after-serving warm-up opens two,
/// paying the set-up then; and a first message's two requests ride them,
/// so its verdict pays none: under a 1.5 s set-up, it waits far less.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn jevs_connections_open_after_serving_and_a_first_message_pays_no_setup() {
    let jev = FakeJev::start().unwrap();
    jev.keep_alive(Duration::from_millis(1500));
    mode(&jev, "sophisticated", 0.95);
    let r = rig(Some(&jev), 1, |_| {});
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        (jev.opened(), jev.warmups(), jev.connections()),
        (0, 0, 0),
        "nothing reaches Jev before serving"
    );
    r.core.warm_judge();
    until("the warm-up's answers", || r.core.runner.judge.jev_warm()).await;
    let opened = jev.opened();
    assert_eq!(opened, crate::judge::warm::CONNECTIONS);
    let t0 = Instant::now();
    let res = turn(&r.core, None, "Weigh two designs for the log.", None).await;
    let took = t0.elapsed();
    assert_eq!(res.route.as_ref().unwrap().reason, "verdict");
    let d = &decided(&r.core.store)[0];
    assert!(d["wait_ms"].as_u64().unwrap() < 1000, "no set-up paid: {d}");
    assert!(took < Duration::from_millis(1500), "{took:?}: {d}");
    until_calls(&jev, 2).await;
    assert_eq!(
        jev.opened(),
        opened,
        "both requests rode the kept connections"
    );
}

/// The keeper uses the connections again after each `every` of Jev's
/// silence, on the ones it opened: warm-ups come, connections do not.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_keeper_uses_its_connections_again_while_jev_is_silent() {
    let jev = FakeJev::start().unwrap();
    jev.keep_alive(Duration::ZERO);
    let r = rig(Some(&jev), 1, |_| {});
    r.core.runner.judge.keep_warm(Duration::from_millis(300));
    until("three refreshes", || {
        jev.warmups() >= 3 * crate::judge::warm::CONNECTIONS
    })
    .await;
    assert_eq!(
        jev.opened(),
        crate::judge::warm::CONNECTIONS,
        "the same two"
    );
    assert_eq!(jev.connections(), 0, "nothing billed");
}
