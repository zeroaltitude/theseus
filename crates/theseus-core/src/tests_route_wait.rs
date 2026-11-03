//! `route.v1`'s verdict without a wait on every message (theseus-ddbi),
//! against the fake Jev with the latencies it is given:
//!
//! - `route.v1` asks in a request of its own beside the batch of
//!   `classify.v4` and `role.v1`, so its verdict comes when its questions
//!   (route.v10's two: the model and the personal one) are
//!   answered, never after the batch's; each judgment is recorded once,
//!   with its own request's cost; `effort.v1`, `work.v1` and `split.v1` each ask
//!   in a request of their own beside it.
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

/// (b): route.v10 alone in one request (the model and the personal question), `effort.v1`, `work.v1`
/// and `split.v1` each alone in theirs, `classify.v4` and `role.v1` in the batch; six judgments, each
/// recorded once, and each request's cost is its own packs' (the batch's two share theirs by question
/// count).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn route_asks_alone_beside_the_batch_and_each_judgment_is_recorded_once() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "sophisticated", 0.95);
    let r = rig(Some(&jev), 1, |_| {});
    let res = turn(&r.core, None, "Weigh two designs for the log.", None).await;
    assert_eq!(res.route.as_ref().unwrap().reason, "verdict");
    until_calls(&jev, 5).await;
    let rows = until_judged(&r.core.store, 6).await;
    let by_pack: BTreeMap<String, usize> = rows.iter().fold(BTreeMap::new(), |mut m, j| {
        *m.entry(j["pack"].as_str().unwrap().to_string())
            .or_default() += 1;
        m
    });
    assert_eq!(
        by_pack,
        BTreeMap::from([
            ("classify.v4".to_string(), 1),
            ("effort.v1".to_string(), 1),
            ("role.v1".to_string(), 1),
            ("route.v10".to_string(), 1),
            ("split.v1".to_string(), 1),
            ("work.v1".to_string(), 1)
        ]),
        "each judgment once"
    );
    let seen = jev.seen();
    assert_eq!(seen.len(), 5, "route, effort, work, split and the batch");
    let route_req = seen
        .iter()
        .find(|s| asked(&s.body).contains("route.v10/model"))
        .unwrap();
    // route.v10's two questions, the model and the personal one, in its one request
    // (theseus-qe3v, theseus-gasov; the effort and the context are not its any more).
    assert_eq!(
        asked(&route_req.body),
        BTreeSet::from([
            "route.v10/model".to_string(),
            "route.v10/personal".to_string()
        ]),
        "route.v10's two questions, one request"
    );
    // Every request's judgments: one call each, the call's own pack and question counts, and the
    // request's own billed usage and price.
    let price = JevPrice::jev_1_13_0();
    for req in &seen {
        let questions = asked(&req.body);
        let packs: BTreeSet<&str> = questions
            .iter()
            .map(|q| q.split('/').next().unwrap())
            .collect();
        let mine: Vec<&Value> = rows
            .iter()
            .filter(|j| packs.contains(j["pack"].as_str().unwrap()))
            .collect();
        assert_eq!(mine.len(), packs.len(), "{packs:?}");
        assert!(
            mine.iter()
                .all(|j| j["call"]["id"] == mine[0]["call"]["id"]),
            "one call for {packs:?}"
        );
        assert_eq!(
            (
                mine[0]["call"]["packs"].as_u64(),
                mine[0]["call"]["questions"].as_u64()
            ),
            (Some(packs.len() as u64), Some(questions.len() as u64)),
            "{packs:?}"
        );
        let u = billed(&req.body);
        let tokens = |k: &str| {
            mine.iter()
                .map(|j| j["usage"][k].as_u64().unwrap())
                .sum::<u64>()
        };
        assert_eq!(
            (tokens("input_tokens"), tokens("output_tokens")),
            (u.input_tokens, u.output_tokens),
            "{packs:?}"
        );
        assert_eq!(
            mine.iter()
                .map(|j| j["cost_micros"].as_u64().unwrap())
                .sum::<u64>(),
            price.cost_micros(&u),
            "the call of {packs:?}"
        );
    }
    let calls: BTreeSet<String> = rows.iter().map(|j| j["call"]["id"].to_string()).collect();
    assert_eq!(calls.len(), 5, "five calls");
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
            .all(|j| j.data["pack"] == "route.v10"),
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
/// so its verdict pays none: under a 1.5 s set-up, it waits far less past
/// its compile.
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
    // The wait past the compile, not the turn's wall time, which a starved
    // debug build stretches whatever Jev does.
    let res = turn(&r.core, None, "Weigh two designs for the log.", None).await;
    assert_eq!(res.route.as_ref().unwrap().reason, "verdict");
    let d = &decided(&r.core.store)[0];
    assert!(d["wait_ms"].as_u64().unwrap() < 1000, "no set-up paid: {d}");
    until_calls(&jev, 5).await;
    assert_eq!(
        jev.opened(),
        opened,
        "all five requests rode the kept connections"
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
        "the same ones"
    );
    assert_eq!(jev.connections(), 0, "nothing billed");
}
