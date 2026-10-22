//! Jev judges the near-miss cross candidates (theseus-98bz.10), through whole
//! turns against the fake Jev and a stand-in index: a yes is admitted with
//! the penalty and labelled with its thread; a no, a late answer, and the
//! switch off leave the drop; at most `cross_recall_judge_k` notes are asked
//! about; a sealed thread's note is never sent; a turn with the place digest
//! asks nothing.

use std::sync::Arc;
use std::time::Duration;

use theseus_judge::fake::{FakeJev, Scripted as Jev};
use theseus_protocol::threads::ThreadSealParams;

use super::tests_thread_recall::{cli, new_thread, say, scored, thread_session};
use super::Core;
use crate::config::{Config, MemoryMode, PackMode};
use crate::judge::cross::CROSS_PACK;
use crate::tests_rerank::{pack_mode, recalls, rig, sent, turn, Rig};

pub(super) const CUTOFF: &str = "the flagging cutoff for the datatidy report is 0.55 from now on";
const PORT: &str = "the homesvc staging copy listens on port 8453";
const ZIRCON: &str = "the zircon embargo lifts after the ledgerweb disclosure";

/// Recall live, rerank off and this pack live; the lens alone would drop
/// every other stream's note here (`own_top_admissible` off, so the
/// thread's own note in context outranks it).
pub(super) fn rigged(jev: &FakeJev, tweak: impl FnOnce(&mut Config)) -> Rig {
    rig(Some(jev), |c| {
        c.memory.mode = MemoryMode::Live;
        c.memory.recall_max_items = 5;
        c.threads.own_top_admissible = false;
        pack_mode(c, crate::judge::rerank::RERANK_PACK, PackMode::Off);
        pack_mode(c, CROSS_PACK, PackMode::Live);
        tweak(c);
    })
}

/// The pipeline thread asks; `stats` holds the cutoff, `ops` the port, and
/// `embargo`, sealed, its own fact. Returns the asking session.
pub(super) fn world(c: &Arc<Core>, extra: usize) -> String {
    for (slug, title) in [
        ("pipeline", "datatidy pipeline"),
        ("stats", "datatidy stats"),
        ("ops", "homesvc maintenance"),
        ("embargo", "ledgerweb fix"),
    ] {
        new_thread(c, slug, title);
    }
    let pipe = thread_session(c, "pipeline");
    say(c, &pipe, "the datatidy report groups rows by region");
    let stats = thread_session(c, "stats");
    say(c, &stats, CUTOFF);
    for k in 0..extra {
        say(
            c,
            &stats,
            &format!("the datatidy report's cutoff draft number {k}"),
        );
    }
    let ops = thread_session(c, "ops");
    say(c, &ops, PORT);
    let embargo = thread_session(c, "embargo");
    say(c, &embargo, ZIRCON);
    c.thread_seal(
        &ThreadSealParams {
            slug: "embargo".into(),
            reason: Some("before disclosure".into()),
            ..Default::default()
        },
        cli(),
    )
    .unwrap();
    let mut picks = vec![
        (pipe.clone(), 0, 0.104),
        (stats.clone(), 0, 0.097),
        (ops, 0, 0.096),
        (embargo, 0, 0.099),
    ];
    for k in 0..extra {
        picks.push((stats.clone(), k + 1, 0.095 - k as f64 * 0.001));
    }
    c.runner.memory.set_ask(scored(c, picks));
    pipe
}

pub(super) fn cross_requests(jev: &FakeJev) -> Vec<String> {
    jev.seen()
        .iter()
        .map(|s| serde_json::to_string(&s.body).unwrap())
        .filter(|b| b.contains("recall_cross.v1/"))
        .collect()
}

pub(super) fn admitted_texts(r: &Rig) -> String {
    sent(&r.model).last().cloned().unwrap_or_default()
}

/// The cutoff note answers "Regenerate the report with the new cutoff.":
/// Jev's yes admits it, labelled `from thread «datatidy stats»`, with the
/// penalty in its rank; the port note, a no, stays dropped.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_yes_is_admitted_with_its_thread_and_a_no_stays_out() {
    let jev = FakeJev::start().unwrap();
    jev.script("same_subject.1", Jev::Noul(0.95));
    jev.script("same_subject.2", Jev::Noul(0.05));
    let r = rigged(&jev, |_| {});
    let c = &r.core;
    let pipe = world(c, 0);
    turn(c, &pipe, "Regenerate the report with the new cutoff.").await;
    let asked = cross_requests(&jev);
    assert_eq!(asked.len(), 1, "one request");
    assert!(
        asked[0].contains("0.55") && asked[0].contains("8453"),
        "{}",
        asked[0]
    );
    let m = recalls(c, &pipe).pop().unwrap();
    let reason = |text: &str| {
        m.dropped
            .iter()
            .find(|d| sent_text(c, &d.node_id).contains(text))
            .map(|d| d.reason.clone())
    };
    assert!(
        m.admitted
            .iter()
            .any(|a| sent_text(c, &a.node_id).contains("0.55")),
        "{:?}",
        m.dropped
    );
    assert!(
        reason("8453").is_some_and(|r| r.starts_with("thread_")),
        "{:?}",
        reason("8453")
    );
    let req = admitted_texts(&r);
    assert!(
        req.contains("from thread \u{ab}datatidy stats\u{bb}"),
        "{req}"
    );
    assert!(req.contains("0.55"));
    assert!(!req.contains("8453"));
}

/// A node's text, read back.
pub(super) fn sent_text(c: &Core, node: &str) -> String {
    c.store
        .get_node(node)
        .ok()
        .flatten()
        .map(|(_, n)| crate::recall::text_of(&n))
        .unwrap_or_default()
}

/// Late: Jev answers after `route_wait_ms`, and the drops stand.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_late_answer_leaves_the_drops() {
    let jev = FakeJev::start().unwrap();
    jev.script("same_subject", Jev::Noul(0.95));
    jev.set_latency(Duration::from_millis(2_000), Duration::from_millis(2_000));
    let r = rigged(&jev, |_| {});
    let c = &r.core;
    let pipe = world(c, 0);
    turn(c, &pipe, "Regenerate the report with the new cutoff.").await;
    let m = recalls(c, &pipe).pop().unwrap();
    assert!(!m
        .admitted
        .iter()
        .any(|a| sent_text(c, &a.node_id).contains("0.55")));
    assert!(!admitted_texts(&r).contains("0.55"));
}

/// Five near misses: three are asked about, best first; the sealed thread's
/// note is in no request, though its score is above the bar.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn k_caps_the_request_and_a_sealed_note_is_never_sent() {
    let jev = FakeJev::start().unwrap();
    let r = rigged(&jev, |_| {});
    let c = &r.core;
    let pipe = world(c, 3);
    turn(c, &pipe, "Regenerate the report with the new cutoff.").await;
    let asked = cross_requests(&jev);
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].matches("recall_cross.v1/same_subject.").count(), 3);
    assert!(asked[0].contains("0.55"), "the best first");
    for seen in jev.seen() {
        let body = serde_json::to_string(&seen.body).unwrap();
        assert!(!body.contains("zircon"), "a sealed note reached Jev");
    }
}

/// `cross_recall_judge = false`: no request, and the drops are the lens's.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_the_switch_off_nothing_is_asked() {
    let jev = FakeJev::start().unwrap();
    jev.script("same_subject", Jev::Noul(0.95));
    let r = rigged(&jev, |c| c.threads.cross_recall_judge = false);
    let c = &r.core;
    let pipe = world(c, 0);
    turn(c, &pipe, "Regenerate the report with the new cutoff.").await;
    assert!(cross_requests(&jev).is_empty());
    assert!(!admitted_texts(&r).contains("0.55"));
}

/// A turn whose request carries the place digest asks nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_call_when_the_digest_fired() {
    let jev = FakeJev::start().unwrap();
    jev.script("same_subject", Jev::Noul(0.95));
    jev.script(
        "scope",
        Jev::Choice {
            option: "place".into(),
            confidence: 0.95,
        },
    );
    let r = rigged(&jev, |c| {
        pack_mode(c, crate::judge::inbound::CLASSIFY_PACK, PackMode::Live);
    });
    let c = &r.core;
    let pipe = world(c, 0);
    // No time words: a span named would keep recall inside it (theseus-w9qv).
    turn(c, &pipe, "What else is going on across the other work?").await;
    assert!(
        admitted_texts(&r).contains("[Place digest"),
        "the digest fired"
    );
    assert!(cross_requests(&jev).is_empty());
    let m = recalls(c, &pipe).pop().unwrap();
    assert!(
        m.dropped.iter().any(|d| d.reason.starts_with("thread_")),
        "near misses Jev would have read: {:?}",
        m.dropped
    );
    // One of them above the cross judge's bar, as `cross_step` reads it, or
    // the check above guards nothing (theseus-1uiiu).
    let bar = c.cfg.threads.cross_recall_bar;
    assert!(
        m.dropped
            .iter()
            .any(|d| d.reason.starts_with("thread_") && d.share.is_some_and(|s| s > bar)),
        "no thread drop above the bar {bar}, so no call was owed: {:?}",
        m.dropped
    );
}

/// The cross judge's failures move only its own breaker (theseus-pt7lt):
/// five cross calls that never answer open `recall_cross` (a `judge.circuit`
/// row naming it), the shared breaker stays closed and `classify` still
/// judges, and the next turn's near misses are not sent at all.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failing_cross_judge_opens_only_its_own_breaker() {
    let jev = FakeJev::start().unwrap();
    jev.script(
        "scope",
        Jev::Choice {
            option: "thread".into(),
            confidence: 0.95,
        },
    );
    // Only a cross request holds the port note: it waits past its deadline.
    jev.hold_when("8453");
    let r = rigged(&jev, |c| {
        pack_mode(c, crate::judge::inbound::CLASSIFY_PACK, PackMode::Live);
    });
    let c = &r.core;
    let pipe = world(c, 0);
    let circuits = || {
        c.store
            .ledger_tail::<crate::ledger::LedgerRow>(5000)
            .unwrap()
            .into_iter()
            .map(|(_, r)| r)
            .filter(|r| r.kind == "judge.circuit")
            .collect::<Vec<_>>()
    };
    for i in 1..=5 {
        turn(c, &pipe, "Regenerate the report with the new cutoff.").await;
        assert_eq!(cross_requests(&jev).len(), i, "asked on turn {i}");
    }
    let t0 = std::time::Instant::now();
    while circuits().is_empty() {
        assert!(
            t0.elapsed() < Duration::from_secs(60),
            "the breaker never moved"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let opened = circuits();
    assert_eq!(opened.len(), 1, "{opened:?}");
    assert_eq!(opened[0].data["breaker"], "recall_cross");
    assert_eq!(opened[0].data["transition"]["circuit"], "opened");
    let h = c.health().judge.unwrap();
    assert_eq!(h.breaker, "closed", "the shared breaker");
    assert!(
        h.breakers
            .iter()
            .any(|b| b.starts_with("recall_cross: open")),
        "{:?}",
        h.breakers
    );
    // The sixth turn sends no cross request, and classify still answers.
    let classified = || {
        jev.seen()
            .iter()
            .filter(|s| {
                serde_json::to_string(&s.body)
                    .unwrap()
                    .contains("classify.v4/")
            })
            .count()
    };
    let before = classified();
    turn(c, &pipe, "Regenerate the report with the new cutoff.").await;
    assert_eq!(cross_requests(&jev).len(), 5, "skipped with no request");
    assert!(
        classified() > before,
        "classify judged on the shared breaker"
    );
    assert_eq!(circuits().len(), 1);
    jev.release();
}
