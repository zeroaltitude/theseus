//! Jev's rerank of recall made live, bounded, and taught by the owner's
//! labels (M6 step 32d; design §2.7, §2.9, §2.12, §2.14), through whole cores
//! against the fake Jev and a stand-in index, as `tests_rerank` does for
//! 32c's shadow arm.

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::future::BoxFuture;
use serde_json::{json, Value};
use theseus_judge::fake::{FakeJev, FakeMode, Scripted as Jev};
use theseus_judge::judge::{AnswerRecord, StateRecord};
use theseus_judge::{DecisionPoint, Judge, Judgment, Outcome};
use theseus_protocol::memory::MemoryLabelParams;
use theseus_protocol::{Span, TurnSubmitResult};
use tokio::sync::{mpsc, oneshot};

use crate::config::{MemoryMode, PackMode};
use crate::ledger::LedgerRow;
use crate::store::Store;
use crate::tests_rerank::{
    heron_rig, index_of, keys_of, pack_mode, recalls, rig, sent, session, turn, until_reranked,
};
use crate::Config;

/// A note the owner labeled `wrong` or `stale` never reaches Jev's rerank
/// request, nor comes back through its repack (theseus-mm4a): its key and
/// its text are in neither the state Jev was sent nor the row's admitted
/// lists, though Jev would rank it first. `recall_end` hands the labels over
/// (`Recalled.labeled`); so does the live path.
async fn a_labeled_note_never_reaches_jev(live: bool) {
    for word in ["wrong", "stale"] {
        let jev = FakeJev::start().unwrap();
        // Were the heron's note asked about, Jev would put it first.
        for k in 1..=3 {
            jev.script(&format!("helps.{k}"), Jev::Noul(0.97));
        }
        let (r, here, order) = heron_rig(&jev, |c| {
            if live {
                c.memory.mode = MemoryMode::Live;
            }
        });
        let c = &r.core;
        let heron = c.store.session_nodes(&order[2]).unwrap()[0].1.id.clone();
        let key = format!("{heron}#0");
        c.memory_label(
            &MemoryLabelParams {
                node_id: heron.clone(),
                label: word.into(),
                recall_id: None,
                note: None,
            },
            "cli",
        )
        .unwrap();
        turn(c, &here, "Where does the grey heron nest?").await;
        let rows = until_reranked(&c.store, 1).await;
        let seen = jev.seen();
        assert_eq!(seen.len(), 1, "{word}: one rerank");
        let state = serde_json::to_string(&seen[0].body).unwrap();
        assert!(!state.contains(&heron), "{word}: the node's id reached Jev");
        assert!(
            !state.contains("grey heron nests"),
            "{word}: the note's text reached Jev"
        );
        let rr = &rows[0].data["context"]["rerank"];
        assert_eq!(rr["eligible"], 2, "{word}: {rr}");
        for list in ["fused_admitted", "reranked_admitted", "top"] {
            assert!(
                !keys_of(&rr[list]).contains(&key),
                "{word}: {list} holds the labeled note: {rr}"
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_labeled_note_never_reaches_a_shadow_rerank() {
    a_labeled_note_never_reaches_jev(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_labeled_note_never_reaches_a_live_rerank() {
    a_labeled_note_never_reaches_jev(true).await;
}

/// Every `judge` span of a kind in a trace.
fn judge_spans(s: &Span, kind: &str, out: &mut Vec<Value>) {
    if s.name == "judge" && s.kind == kind {
        out.push(s.attrs.clone());
    }
    for c in &s.children {
        judge_spans(c, kind, out);
    }
}

fn wait_span(res: &TurnSubmitResult) -> Value {
    let mut found = Vec::new();
    judge_spans(res.trace.as_ref().unwrap(), "wait", &mut found);
    assert_eq!(found.len(), 1, "{found:?}");
    found.remove(0)
}

/// Rerank's failures and timeouts move only its own breaker (32d): five
/// reranks that time out in a row open it (a `judge.circuit` row naming
/// it, health's judge line), the next rerank skips at once, and a
/// `loop.v1` judgment still goes out, on the shared breaker, closed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn five_rerank_timeouts_open_reranks_breaker_alone() {
    let jev = FakeJev::start().unwrap();
    // Slower than rerank's 600 ms, inside loop.v1's 5 s.
    jev.set_mode(FakeMode::Slow(Duration::from_millis(1500)));
    let (r, here, _) = heron_rig(&jev, |c| {
        c.judge.packs.remove("loop.v1");
    });
    let c = &r.core;
    for i in 1..=5 {
        turn(c, &here, "Where does the grey heron nest?").await;
        let rows = until_reranked(&c.store, i).await;
        let rr = &rows[i - 1].data["context"]["rerank"];
        assert_eq!(rr["fallback"], "timeout", "{i}: {rr}");
    }
    let circuits = kinds(&c.store, "judge.circuit");
    assert_eq!(circuits.len(), 1, "{circuits:?}");
    assert_eq!(circuits[0].data["breaker"], "rerank");
    assert_eq!(circuits[0].data["transition"]["circuit"], "opened");
    // Its opening counts on rerank.v1's ladder: one event today (26a's
    // `opens_per_day`).
    let day = c.runner.judge.today();
    let scope = crate::judge::ladder::events_scope("rerank", &day);
    let opened = || {
        c.store
            .scope_after(&scope, 0)
            .unwrap()
            .into_iter()
            .map(|r| r.decode::<LedgerRow>().unwrap().data["event"].clone())
            .collect::<Vec<Value>>()
    };
    let t0 = Instant::now();
    while opened().is_empty() {
        assert!(t0.elapsed() < Duration::from_secs(10), "no event");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(opened(), [json!({"event": "breaker_opened", "day": day})]);
    let h = c.health().judge.unwrap();
    assert_eq!(h.breaker, "closed", "the shared breaker");
    assert_eq!(h.breakers.len(), 1);
    assert!(
        h.breakers[0].starts_with("rerank: open"),
        "{:?}",
        h.breakers
    );
    // The sixth rerank skips at once; loop.v1 still goes out, and answers.
    let loops_before = loop_rows(&c.store).len();
    turn(c, &here, "And the kettle?").await;
    let rows = until_reranked(&c.store, 6).await;
    let rr = &rows[5].data;
    assert_eq!(rr["outcome"]["reason"], "circuit_open", "{rr}");
    let t0 = Instant::now();
    loop {
        let rows = loop_rows(&c.store);
        if rows.len() > loops_before
            && rows[loops_before..]
                .iter()
                .any(|r| r.data["outcome"]["outcome"] == "answered")
        {
            break;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(30),
            "loop.v1 never answered"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(kinds(&c.store, "judge.circuit").len(), 1);
    assert_eq!(c.health().judge.unwrap().breaker, "closed");
}

fn kinds(store: &Store, kind: &str) -> Vec<LedgerRow> {
    store
        .ledger_tail::<LedgerRow>(100_000)
        .unwrap()
        .into_iter()
        .map(|(_, r)| r)
        .filter(|r| r.kind == kind)
        .collect()
}

fn loop_rows(store: &Store) -> Vec<LedgerRow> {
    store
        .scope_after("judge:loop", 0)
        .unwrap()
        .iter()
        .map(|r| r.decode().unwrap())
        .collect()
}

/// Memory live: every session's recall reaches its model.
fn live(c: &mut Config) {
    c.memory.mode = MemoryMode::Live;
}

/// A live recall carries Jev's order into the request: the heron's note,
/// third in the fused order, is the one the pack admits, and the request
/// renders it; the manifest, the row, and the turn's `judge` span say Jev's
/// order was applied.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_live_recall_carries_jevs_order_into_the_request() {
    let jev = FakeJev::start().unwrap();
    jev.script("helps.1", Jev::Noul(0.10));
    jev.script("helps.2", Jev::Noul(0.30));
    jev.script("helps.3", Jev::Noul(0.97));
    let (r, here, order) = heron_rig(&jev, |c| {
        live(c);
        // A loaded machine's local Jev may take more than 200 ms.
        c.memory.rerank_wait_ms = 600;
    });
    let c = &r.core;
    let res = turn(c, &here, "Where does the grey heron nest?").await;
    let req = sent(&r.model).pop().unwrap();
    assert!(
        req.contains("the grey heron nests by the old weir"),
        "{req}"
    );
    assert!(!req.contains("kettle"), "{req}");
    let m = &recalls(c, &here)[0];
    assert_eq!(m.admitted.len(), 1);
    assert_eq!(m.admitted[0].session_id, order[2], "Jev's best");
    let rr = m.rerank.as_ref().expect("the manifest's rerank");
    assert!(rr.applied, "{rr:?}");
    assert_eq!(rr.why, None);
    assert_eq!(rr.wait_ms, 600);
    let span = wait_span(&res);
    assert_eq!(span["applied"], true);
    assert_eq!(span["mode"], "live");
    assert_eq!(span["judgment"].as_str(), rr.judgment.as_deref());
    let rows = until_reranked(&c.store, 1).await;
    let d = &rows[0].data;
    assert_eq!(d["mode"], "live");
    assert_eq!(d["id"].as_str(), rr.judgment.as_deref());
    let x = &d["context"]["rerank"];
    assert_eq!(
        (&x["live"], &x["applied"], &x["late"]),
        (&json!(true), &json!(true), &json!(false))
    );
    assert_eq!(
        keys_of(&x["reranked_admitted"]),
        [format!("{}#0", m.admitted[0].node_id)],
        "the row's repack is the turn's"
    );
    assert!(c
        .health()
        .judge
        .unwrap()
        .packs
        .contains(&"rerank.v1: live (owner: decision of 2026-10-04)".to_string()));
}

/// A Jev slower than the wait leaves the request byte for byte a judge-off
/// turn's, the turn goes on at the wait, and the answer, when it comes, is
/// recorded `late`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_slow_jev_leaves_the_request_as_judge_offs_and_its_late_row_lands() {
    let off = rig(None, live);
    let notes = session(&off.core, None, &["the grey heron nests by the old weir"]);
    let here = session(&off.core, None, &[]);
    off.core
        .runner
        .memory
        .set_ask(index_of(&off.core, vec![notes.clone()]));
    turn(&off.core, &here, "Where does the grey heron nest?").await;
    let base = normalized(&sent(&off.model), &[&notes, &here]);
    assert!(recalls(&off.core, &here)[0].rerank.is_none());
    let jev = FakeJev::start().unwrap();
    // Past the wait, inside the call's 600 ms.
    jev.set_mode(FakeMode::Slow(Duration::from_millis(400)));
    jev.script("helps.1", Jev::Noul(0.05));
    let r = rig(Some(&jev), live);
    let c = &r.core;
    let notes = session(c, None, &["the grey heron nests by the old weir"]);
    let here = session(c, None, &[]);
    c.runner.memory.set_ask(index_of(c, vec![notes.clone()]));
    let res = turn(c, &here, "Where does the grey heron nest?").await;
    assert_eq!(
        normalized(&sent(&r.model), &[&notes, &here]),
        base,
        "the turn's request"
    );
    let m = &recalls(c, &here)[0];
    let rr = m.rerank.as_ref().unwrap();
    assert_eq!((rr.applied, rr.why.as_deref()), (false, Some("timeout")));
    assert!(rr.waited_ms >= 200.0, "{rr:?}");
    assert_eq!(wait_span(&res)["why"], "timeout");
    let rows = until_reranked(&c.store, 1).await;
    let x = &rows[0].data["context"]["rerank"];
    assert_eq!(
        (&x["applied"], &x["late"]),
        (&json!(false), &json!(true)),
        "{x}"
    );
}

/// What differs between two rigs' requests that is not the turn's: the
/// sessions' ids, and the sources' dates in a recall's testimony.
fn normalized(reqs: &[String], ids: &[&str]) -> Vec<String> {
    reqs.iter()
        .map(|r| {
            let mut r = r.clone();
            for (i, id) in ids.iter().enumerate() {
                r = r.replace(id, &format!("<session {i}>"));
            }
            unpositioned(&undated(&r))
        })
        .collect()
}

/// `(as of @12)` as `(as of @<n>)`: a judged rig's warm read writes the
/// adoptions before its first session (theseus-289c), so its records stand
/// three positions on from a judge-off rig's.
fn unpositioned(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find("(as of @") {
        let (head, tail) = rest.split_at(i + "(as of @".len());
        out.push_str(head);
        out.push_str("<n>");
        rest = tail.trim_start_matches(|c: char| c.is_ascii_digit());
    }
    out.push_str(rest);
    out
}

/// `2026-10-04 18:14 UTC` as `<date>`.
fn undated(s: &str) -> String {
    let date = |d: &[u8]| {
        d.len() == 16
            && d.iter().enumerate().all(|(k, b)| match k {
                4 | 7 => *b == b'-',
                10 => *b == b' ',
                13 => *b == b':',
                _ => b.is_ascii_digit(),
            })
    };
    let (mut out, mut rest) = (String::new(), s);
    while let Some(i) = rest.find(" UTC") {
        let at = i.saturating_sub(16);
        if i >= 16 && date(&rest.as_bytes()[at..i]) {
            out.push_str(&rest[..at]);
            out.push_str("<date>");
        } else {
            out.push_str(&rest[..i + 4]);
        }
        rest = &rest[i + 4..];
    }
    out.push_str(rest);
    out
}

/// Rerank's breaker open, the day's budget spent, the judge off, rerank in
/// shadow: a live recall waits for no Jev.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_wait_when_the_breaker_is_open_the_budget_spent_the_judge_off_or_rerank_in_shadow() {
    let wait = |c: &mut Config| {
        live(c);
        c.memory.rerank_wait_ms = 600;
    };
    // The breaker: five timeouts open it, then a recall waits for nothing.
    let jev = FakeJev::start().unwrap();
    jev.set_mode(FakeMode::Slow(Duration::from_secs(3)));
    let (r, _, _) = heron_rig(&jev, wait);
    let c = &r.core;
    // A fresh session each time: a live recall's notes are in its
    // session's context after it, and no longer eligible there.
    for i in 1..=5 {
        let here = session(c, None, &[]);
        turn(c, &here, "Where does the grey heron nest?").await;
        until_reranked(&c.store, i).await;
    }
    // The wait is what is bounded: the turn's own time under load is not.
    let here = session(c, None, &[]);
    turn(c, &here, "Where does the grey heron nest?").await;
    let m = recalls(c, &here).pop().unwrap();
    let rr = m.rerank.unwrap();
    assert_eq!(rr.why.as_deref(), Some("breaker_open"));
    // What it waited is the eligibility pass and the dispatch, never Jev
    // (who takes 3 s here): under load that is tens of ms.
    assert!(rr.waited_ms < 300.0, "{rr:?}");
    // No turn waited for its row: skipped at the breaker, never late.
    let rows = until_reranked(&c.store, 6).await;
    let x = &rows[5].data;
    assert_eq!(x["outcome"]["reason"], "circuit_open", "{x}");
    assert_eq!(x["context"]["rerank"]["late"], false, "{x}");
    // The budget: the first rerank finds the limit, the next knows it.
    let jev = FakeJev::start().unwrap();
    jev.set_mode(FakeMode::Slow(Duration::from_secs(3)));
    let (r, _, _) = heron_rig(&jev, |c| {
        wait(c);
        c.judge.shadow_limit_usd_per_day = 0.0;
    });
    let c = &r.core;
    for i in 0..2 {
        let here = session(c, None, &[]);
        turn(c, &here, "Where does the grey heron nest?").await;
        let rr = recalls(c, &here).pop().unwrap().rerank.unwrap();
        assert_eq!(rr.why.as_deref(), Some("budget"), "{rr:?}");
        // The first waits for its own reservation's answer (never Jev's);
        // the second is told before it dispatches, and waits for nothing.
        let most = if i == 0 { 600.0 } else { 300.0 };
        assert!(rr.waited_ms < most, "{i}: {rr:?}");
    }
    assert_eq!(jev.connections(), 0);
    // The judge off, and rerank in shadow: no rerank waited on.
    for shadow in [false, true] {
        let jev = FakeJev::start().unwrap();
        jev.set_mode(FakeMode::Slow(Duration::from_secs(3)));
        let r = if shadow {
            heron_rig(&jev, |c| {
                wait(c);
                pack_mode(c, "rerank.v1", PackMode::Shadow);
            })
            .0
        } else {
            rig(None, wait)
        };
        let c = &r.core;
        let notes = session(c, None, &["the grey heron nests by the old weir"]);
        let here = session(c, None, &[]);
        c.runner.memory.set_ask(index_of(c, vec![notes]));
        let res = turn(c, &here, "Where does the grey heron nest?").await;
        assert!(recalls(c, &here)[0].rerank.is_none(), "shadow {shadow}");
        let mut waits = Vec::new();
        judge_spans(res.trace.as_ref().unwrap(), "wait", &mut waits);
        assert!(waits.is_empty(), "shadow {shadow}: {waits:?}");
        if shadow {
            let mut found = Vec::new();
            crate::tests_rerank::marks(res.trace.as_ref().unwrap(), &mut found);
            assert_eq!(found[0]["mode"], "shadow");
            let rows = until_reranked(&c.store, 1).await;
            assert_eq!(rows[0].data["mode"], "shadow");
        }
    }
}

/// Jev as a channel: each decision point goes to the test, which answers
/// it when it likes.
pub(crate) struct ChannelJudge(
    pub(crate) mpsc::UnboundedSender<(DecisionPoint, oneshot::Sender<Vec<Judgment>>)>,
);

impl Judge for ChannelJudge {
    fn judge(&self, point: DecisionPoint) -> BoxFuture<'_, Vec<Judgment>> {
        Box::pin(async move {
            let (tx, rx) = oneshot::channel();
            let _ = self.0.send((point, tx));
            rx.await.unwrap_or_default()
        })
    }
}

/// An answered judgment of `point`'s one ask: each note's Noul from `p`, by
/// its number.
pub(crate) fn answered(point: &DecisionPoint, p: impl Fn(usize) -> f64) -> Vec<Judgment> {
    let ask = &point.asks[0];
    let answers = ask
        .asked
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let answer = theseus_judge::Answer::Noul { noul: p(i + 1) };
            AnswerRecord {
                question: a.id.clone(),
                def: a.def.clone(),
                about: a.about.clone(),
                band: theseus_judge::band::band(&answer, a.thresholds),
                answer,
            }
        })
        .collect();
    vec![Judgment {
        id: ask.id.clone().unwrap(),
        pack: ask.pack.name(),
        version: ask.pack.version,
        pack_sha256: ask.pack.sha256.clone(),
        point: ask.pack.point,
        mode: ask.mode,
        model: ask.pack.jev_model.clone(),
        answered_by: Some(ask.pack.jev_model.clone()),
        model_drift: false,
        state: StateRecord::from(ask.state.as_ref()),
        questions: ask.asked.len(),
        answers,
        call: None,
        timing: Default::default(),
        usage: None,
        cost_micros: Some(1),
        reserve_micros: None,
        outcome: Outcome::Answered,
        circuit: None,
        rate_limit: Default::default(),
        context: ask.context.clone(),
    }]
}

/// On tokio's paused clock, with Jev a channel: the turn goes on exactly
/// `rerank_wait_ms` after the rerank starts. An answer 1 ms before the
/// bound is applied; one 50 ms after it is late, and recall's own order
/// stands.
#[tokio::test(start_paused = true)]
async fn the_turn_goes_on_at_the_wait_exactly() {
    for (after_ms, applied) in [(199u64, true), (250, false)] {
        let jev = FakeJev::start().unwrap();
        let (r, here, order) = heron_rig(&jev, live);
        let c = r.core.clone();
        let (tx, mut rx) = mpsc::unbounded_channel();
        c.runner.judge.rerank_with(Arc::new(ChannelJudge(tx)));
        let turned = {
            let (c, here) = (c.clone(), here.clone());
            tokio::spawn(async move { turn(&c, &here, "Where does the grey heron nest?").await })
        };
        let (point, reply) = rx.recv().await.unwrap();
        let asked = tokio::time::Instant::now();
        tokio::time::sleep(Duration::from_millis(after_ms)).await;
        // The heron's note is the third.
        let _ = reply.send(answered(&point, |n| if n == 3 { 0.97 } else { 0.05 }));
        let res = turned.await.unwrap();
        let m = recalls(&c, &here).pop().unwrap();
        let rr = m.rerank.clone().unwrap();
        assert_eq!(rr.applied, applied, "{after_ms} ms: {rr:?}");
        if applied {
            assert_eq!(rr.waited_ms, after_ms as f64, "{rr:?}");
            assert_eq!(m.admitted[0].session_id, order[2], "Jev's order");
        } else {
            assert_eq!(rr.waited_ms, 200.0, "exactly the wait: {rr:?}");
            assert_eq!(rr.why.as_deref(), Some("timeout"));
            assert_eq!(m.admitted[0].session_id, order[0], "recall's own order");
            assert!(asked.elapsed() >= Duration::from_millis(after_ms));
        }
        assert_eq!(wait_span(&res)["applied"], applied);
        let rows = until_reranked(&c.store, 1).await;
        let x = &rows[0].data["context"]["rerank"];
        assert_eq!(
            (&x["applied"], &x["late"]),
            (&json!(applied), &json!(!applied)),
            "{after_ms} ms: {x}"
        );
    }
}

/// A ladder rollback of rerank.v1 stops its live order (batch 5's join,
/// theseus-9j7x): a recall in front of the model keeps recall's own order,
/// with no wait, its rerank judged in shadow, and health says the pack is
/// rolled back.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_ladder_rollback_of_rerank_stops_its_live_order() {
    let jev = FakeJev::start().unwrap();
    jev.script("helps.1", Jev::Noul(0.10));
    jev.script("helps.2", Jev::Noul(0.30));
    jev.script("helps.3", Jev::Noul(0.97));
    let (r, here, _) = heron_rig(&jev, |c| {
        live(c);
        c.memory.rerank_wait_ms = 600;
    });
    let c = &r.core;
    c.pack_rollback(
        &theseus_protocol::packs::PackRollbackParams {
            pack: "rerank.v1".into(),
            why: None,
            off: false,
        },
        "cli",
    )
    .unwrap();
    let res = turn(c, &here, "Where does the grey heron nest?").await;
    let m = &recalls(c, &here)[0];
    assert!(
        m.rerank.as_ref().is_none_or(|rr| !rr.applied),
        "{:?}",
        m.rerank
    );
    let mut waits = Vec::new();
    judge_spans(res.trace.as_ref().unwrap(), "wait", &mut waits);
    assert!(waits.is_empty(), "no wait: {waits:?}");
    let rows = until_reranked(&c.store, 1).await;
    assert_eq!(rows[0].data["mode"], "shadow");
    let h = c.health().judge.unwrap();
    assert!(
        h.packs
            .contains(&"rerank.v1: rolled back (owner: the owner rolled it back)".to_string()),
        "{:?}",
        h.packs
    );
}
