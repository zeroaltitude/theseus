//! A place with one live thread offers a new one (theseus-98bz.7), through
//! whole cores: a message the thread does not hold that names a new stream's
//! branch and ticket is asked about (the thread, a new one, home); a message
//! the thread holds routes as before; a spoken turn runs at home unasked; and
//! with `[threads] single_offers_new = false` nothing is asked.

use std::time::{Duration, Instant};

use theseus_protocol::thread_route::RouteHow;
use theseus_protocol::threads::{ThreadEditParams, WorkingSet};
use theseus_protocol::TurnSubmitParams;

use super::tests_thread_grow::{cli, rig, send, send_as, thread, Rig};
use crate::approval::Surface;
use crate::config::Config;
use crate::provider::Scripted;

const NEW_STREAM: &str = "homesvc maintenance: branch chore/bump-1-8, ticket OPS-4214";

/// One live thread holding the root `tarnlib`; questions expire in a second.
fn lone(tweak: impl FnOnce(&mut Config)) -> Rig {
    let r = rig(
        |_| (0..3).map(|_| Scripted::text("On it.")).collect(),
        |cfg| {
            cfg.threads.ask_wait_secs = 1;
            tweak(cfg);
        },
    );
    thread(&r.core, "tarnlib-work");
    r.core
        .thread_edit(
            &ThreadEditParams {
                slug: "tarnlib-work".into(),
                working_set: Some(WorkingSet {
                    roots: vec!["tarnlib".into()],
                    ..Default::default()
                }),
                ..Default::default()
            },
            cli(),
        )
        .unwrap();
    r
}

fn said(session: &str, input: &str) -> TurnSubmitParams {
    TurnSubmitParams {
        session_id: Some(session.into()),
        input: input.into(),
        ..TurnSubmitParams::default()
    }
}

fn asked(r: &Rig) -> Vec<crate::ledger::LedgerRow> {
    crate::test_support::ledger_rows_2k(&r.core, "thread.asked")
}

/// The new stream's first message is asked about, offering `new`; unanswered,
/// it runs at home. A message about the thread's own root routes there.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_lone_thread_asks_and_offers_a_new_one() {
    let r = lone(|_| {});
    let home = r.home();
    let v = send(&r.core, said(&home, NEW_STREAM)).await;
    let rows = asked(&r);
    assert_eq!(rows.len(), 1, "one question");
    let labels: Vec<&str> = rows[0].data["options"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|o| o.as_str())
        .collect();
    assert_eq!(labels.len(), 3, "{labels:?}");
    assert_eq!(labels[0], "tarnlib work");
    assert!(labels[1].starts_with("New thread"), "{labels:?}");
    assert_eq!(labels[2], "home");
    assert_eq!(v["result"]["session_id"], home.as_str(), "unanswered: home");
    let v = send(&r.core, said(&home, "retry the flaky test in tarnlib")).await;
    assert_eq!(v["result"]["thread"]["slug"], "tarnlib-work", "{v}");
    assert_eq!(
        v["result"]["thread"]["how"],
        serde_json::to_value(RouteHow::Visible).unwrap()
    );
    assert_eq!(
        asked(&r).len(),
        1,
        "a message the thread holds asks nothing"
    );
}

/// A spoken turn is never held for a question: it runs at home at once.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_spoken_turn_beside_a_lone_thread_runs_at_home() {
    let r = lone(|cfg| cfg.threads.ask_wait_secs = 60);
    let home = r.home();
    let mut p = said(&home, NEW_STREAM);
    p.surface = Some("voice".into());
    let t0 = Instant::now();
    let v = send_as(&r.core, Surface::Discord, p).await;
    assert!(t0.elapsed() < Duration::from_secs(30), "no wait");
    assert_eq!(v["result"]["session_id"], home.as_str(), "{v}");
    assert!(asked(&r).is_empty());
    let routed = crate::test_support::ledger_rows_2k(&r.core, "thread.routed");
    assert_eq!(routed.len(), 1, "the router saw it");
}

/// `single_offers_new = false`: the same message runs at home unasked, as
/// before.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_the_switch_off_a_lone_thread_asks_nothing() {
    let r = lone(|cfg| cfg.threads.single_offers_new = false);
    let home = r.home();
    let v = send(&r.core, said(&home, NEW_STREAM)).await;
    assert!(asked(&r).is_empty());
    assert_eq!(v["result"]["session_id"], home.as_str());
    assert_eq!(
        v["result"]["thread"]["how"],
        serde_json::to_value(RouteHow::Home).unwrap()
    );
}

/// **The lone-thread offer is a guess of HOME too** (`ttlze-offer-new-home`).
///
/// `offer_beside_one` fires only when the thread's score is **below** the
/// confirm bar, and under `unsure = "guess"` the new arm handed that `Ask` to
/// `thread_route::guessed`, which takes the best-ranked existing choice — the
/// same below-bar thread. The W11 report's claim that the lone-thread offer
/// "is also a home guess" was measured false by its reviewer: it is a guess of
/// the lone thread whenever its score exceeds 0.
///
/// So this message, which the thread does not hold and which names a new
/// stream's branch and ticket, must run at **home**, as a guess and movable.
/// The positive counts beside it: the router did see the message
/// (`thread.routed`), and nothing was asked, which is what `guess` is for.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn under_guess_a_lone_threads_new_offer_guesses_home() {
    let r = lone(|cfg| cfg.threads.unsure = crate::config::Unsure::Guess);
    let home = r.home();
    let v = send(&r.core, said(&home, NEW_STREAM)).await;
    assert!(asked(&r).is_empty(), "a guess asks nothing");
    assert_eq!(
        v["result"]["thread"]["slug"],
        serde_json::Value::Null,
        "home, not the below-bar lone thread: {v}"
    );
    assert_eq!(v["result"]["session_id"], home.as_str(), "{v}");
    assert_eq!(
        v["result"]["thread"]["how"],
        serde_json::to_value(RouteHow::Guess).unwrap(),
        "{v}"
    );
    assert_eq!(v["result"]["thread"]["movable"], true, "{v}");
    let routed = crate::test_support::ledger_rows_2k(&r.core, "thread.routed");
    assert_eq!(routed.len(), 1, "the router saw it: {routed:?}");
}
