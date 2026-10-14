//! Capture (theseus-slvl.5; `crate::capture`, `rpc/capture.rs`), against the
//! fake Jev: `directive.v1` rides the inbound request with classify and role
//! (no call of its own), and its verdict, by band, keeps a record with a
//! notice and an undo, asks, or does nothing; an order is always asked; a
//! correction made twice is proposed; a message that says a record again is
//! counted; and the owner's words alone are heard (a shared place, a job's
//! message and `capture = false` hear nothing).

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_judge::fake::{FakeJev, Scripted as Jev};
use theseus_protocol::{
    error_code, Response, StandingAnswerResult, StandingChanged, StandingKind, StandingListResult,
    StandingScope, TurnSubmitResult,
};

use crate::approval::{Client, Surface};
use crate::rpc::Core;
use crate::tests_judge::kinds;
use crate::tests_route::{mode, rig, Rig};

/// A guild channel nobody bound private: a shared place.
const LAB: &str = "900000000000000123";

async fn call_on(core: &Arc<Core>, surface: Surface, method: &str, params: Value) -> Response {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let (client, server) = tokio::io::duplex(64 * 1024);
    let (sr, sw) = tokio::io::split(server);
    let label = format!("{}#1", surface.as_str());
    let srv = tokio::spawn(
        core.clone()
            .serve_connection(sr, sw, Client::new(label, surface)),
    );
    let (cr, mut cw) = tokio::io::split(client);
    let req = theseus_protocol::Request::new(theseus_protocol::Id::Num(1), method, params);
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let mut lines = tokio::io::BufReader::new(cr).lines();
    let r = loop {
        let l = lines.next_line().await.unwrap().unwrap();
        if let theseus_protocol::Message::Response(r) = serde_json::from_str(&l).unwrap() {
            break r;
        }
    };
    cw.shutdown().await.unwrap();
    drop(lines);
    let _ = srv.await;
    r
}

async fn ask(core: &Arc<Core>, session: Option<&str>, input: &str) -> TurnSubmitResult {
    ask_with(core, json!({"session_id": session, "input": input})).await
}

async fn ask_with(core: &Arc<Core>, params: Value) -> TurnSubmitResult {
    let r = call_on(core, Surface::Cli, "turn.submit", params).await;
    assert!(r.error.is_none(), "{:?}", r.error);
    serde_json::from_value(r.result.unwrap()).unwrap()
}

/// Script `directive.v1`: the kind it hears and how sure it is, and the scope.
fn hears(jev: &FakeJev, kind: &str, confidence: f64, scope: (&str, f64)) {
    jev.script(
        "directive.v1/kind",
        Jev::Choice {
            option: kind.into(),
            confidence,
        },
    );
    jev.script(
        "directive.v1/scope",
        Jev::Choice {
            option: scope.0.into(),
            confidence: scope.1,
        },
    );
}

fn rows(core: &Core, kind: &str) -> Vec<Value> {
    kinds(&core.store, kind)
        .into_iter()
        .map(|r| r.data)
        .collect()
}

/// Wait (on the runtime's timer) until `f` holds.
async fn until(what: &str, mut f: impl FnMut() -> bool) {
    let t0 = Instant::now();
    while !f() {
        assert!(t0.elapsed() < Duration::from_secs(20), "waiting for {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Every standing record, ended ones included, as the CLI lists them.
async fn list(core: &Arc<Core>) -> StandingListResult {
    let r = call_on(core, Surface::Cli, "standing.list", json!({"all": true})).await;
    assert!(r.error.is_none(), "{:?}", r.error);
    serde_json::from_value(r.result.unwrap()).unwrap()
}

/// The records `directive.v1` kept, ended ones too.
fn captured(core: &Core) -> Vec<theseus_protocol::StandingRecord> {
    let all = core.runner.standing.all(&core.store).unwrap();
    all.iter()
        .filter(|r| r.source.starts_with("capture:"))
        .cloned()
        .collect()
}

fn live_of(l: &StandingListResult) -> Vec<&theseus_protocol::StandingInfo> {
    l.records.iter().filter(|r| r.state == "live").collect()
}

fn rig_capture(jev: &FakeJev, turns: usize) -> Rig {
    let r = rig(Some(jev), turns, |_| {});
    // The route pack answers; its mode is no matter here.
    mode(jev, "chat", 0.95);
    r
}

/// The batch's request: the one that holds `directive.v1`.
fn asked_directive(jev: &FakeJev) -> Vec<Value> {
    jev.seen()
        .into_iter()
        .map(|s| s.body)
        .filter(|b| {
            b["questions"]
                .as_object()
                .is_some_and(|q| q.keys().any(|k| k.starts_with("directive.v1/")))
        })
        .collect()
}

/// A sure preference is kept, in the request the other inbound packs ride (no
/// call is added), with its notice and its rows; and an undo revokes it and
/// labels the judgment `wrong capture`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sure_preference_is_kept_with_a_notice_and_an_undo_revokes_it() {
    let jev = FakeJev::start().unwrap();
    hears(&jev, "preference", 0.93, ("everywhere", 0.9));
    let r = rig_capture(&jev, 2);
    let one = ask(&r.core, None, "no tables with me").await;
    until("the record", || captured(&r.core).len() == 1).await;
    let l = list(&r.core).await;
    let live = live_of(&l);
    assert_eq!(live.len(), 1, "{l:?}");
    let rec = &live[0].record;
    assert_eq!(rec.kind, StandingKind::Preference);
    assert_eq!(
        rec.text, "no tables with me",
        "the owner's words, as one line"
    );
    assert_eq!(rec.scope, StandingScope::Everywhere, "Jev was sure of it");
    assert!(rec.source.starts_with("capture:msg_"), "{}", rec.source);
    assert_eq!(live[0].captured_by, "capture");
    // One request for the message beside route's: the directive rode the batch.
    let with = asked_directive(&jev);
    assert_eq!(with.len(), 1);
    let ids: Vec<&String> = with[0]["questions"].as_object().unwrap().keys().collect();
    assert!(ids.iter().any(|i| *i == "classify.v1/kind"), "{ids:?}");
    assert_eq!(
        jev.connections(),
        2,
        "route's request and the batch: no call was added"
    );
    // The rows: the record's, the capture's, with the judgment that said so.
    let cap = rows(&r.core, "standing.captured");
    assert_eq!(cap.len(), 1);
    assert_eq!(cap[0]["record"], rec.id);
    assert_eq!(cap[0]["kind"], "preference");
    assert!(cap[0]["judgment"].as_str().unwrap().starts_with("jdg_"));
    assert_eq!(rows(&r.core, "standing.added").len(), 1);
    // Undo: the record ends and the judgment is labeled.
    let undo = call_on(
        &r.core,
        Surface::Cli,
        "standing.revoke",
        json!({"id": rec.id}),
    )
    .await;
    assert!(undo.error.is_none(), "{:?}", undo.error);
    let changed: StandingChanged = serde_json::from_value(undo.result.unwrap()).unwrap();
    assert_eq!(changed.ended.as_deref(), Some(rec.id.as_str()));
    assert!(live_of(&list(&r.core).await).is_empty());
    let labels = rows(&r.core, "judge.label");
    assert_eq!(labels.len(), 1, "{labels:?}");
    assert_eq!(labels[0]["label"], "wrong capture");
    assert_eq!(labels[0]["judgment"], cap[0]["judgment"]);
    let _ = one;
}

/// Between a half and four fifths it asks: the ask waits (nothing is kept),
/// `keep` writes it where it was judged to hold, `everywhere` widens it, and
/// `not` drops it. Each answer is a row; an ask answered is gone.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unsure_directive_is_asked_and_the_answer_keeps_it_or_not() {
    let jev = FakeJev::start().unwrap();
    hears(&jev, "preference", 0.65, ("session", 0.9));
    let r = rig_capture(&jev, 3);
    let one = ask(&r.core, None, "keep answers short").await;
    until("the ask", || !rows(&r.core, "standing.asked").is_empty()).await;
    let l = list(&r.core).await;
    assert!(l.records.is_empty(), "nothing is kept unasked: {l:?}");
    assert_eq!(l.asks.len(), 1);
    let a = &l.asks[0];
    assert_eq!(
        (a.kind, a.why.as_str()),
        (StandingKind::Preference, "unsure")
    );
    assert_eq!(a.text, "keep answers short");
    assert_eq!(a.scope, StandingScope::Session(one.session_id.clone()));
    // keep: where it was judged to hold.
    let kept = call_on(
        &r.core,
        Surface::Cli,
        "standing.answer",
        json!({"id": a.id, "answer": "keep"}),
    )
    .await;
    assert!(kept.error.is_none(), "{:?}", kept.error);
    let res: StandingAnswerResult = serde_json::from_value(kept.result.unwrap()).unwrap();
    let rec = res.record.unwrap();
    assert_eq!(rec.scope, StandingScope::Session(one.session_id.clone()));
    assert_eq!(rec.text, "keep answers short");
    assert!(rec.source.starts_with("capture:msg_"));
    assert!(
        list(&r.core).await.asks.is_empty(),
        "an answered ask is gone"
    );
    assert_eq!(rows(&r.core, "standing.answered")[0]["answer"], "keep");
    // The same ask again, answered everywhere and then not at all.
    ask(&r.core, Some(&one.session_id), "reply briefly please").await;
    until("the second ask", || {
        rows(&r.core, "standing.asked").len() == 2
    })
    .await;
    let id = list(&r.core).await.asks[0].id.clone();
    let wide = call_on(
        &r.core,
        Surface::Cli,
        "standing.answer",
        json!({"id": &id[..8], "answer": "everywhere"}),
    )
    .await;
    let wide: StandingAnswerResult = serde_json::from_value(wide.result.unwrap()).unwrap();
    assert_eq!(wide.record.unwrap().scope, StandingScope::Everywhere);
    hears(&jev, "fact", 0.6, ("everywhere", 0.9));
    ask(&r.core, Some(&one.session_id), "I work nights").await;
    until("the third ask", || {
        rows(&r.core, "standing.asked").len() == 3
    })
    .await;
    let id = list(&r.core).await.asks[0].id.clone();
    let no = call_on(
        &r.core,
        Surface::Cli,
        "standing.answer",
        json!({"id": id, "answer": "not"}),
    )
    .await;
    let no: StandingAnswerResult = serde_json::from_value(no.result.unwrap()).unwrap();
    assert!(no.record.is_none());
    let l = list(&r.core).await;
    assert!(l.asks.is_empty());
    assert_eq!(live_of(&l).len(), 2, "{l:?}");
    // A word that is no answer, and an ask that is not open, are refused.
    for (id, answer) in [("ask_nope", "keep"), ("ask_x", "maybe")] {
        let bad = call_on(
            &r.core,
            Surface::Cli,
            "standing.answer",
            json!({"id": id, "answer": answer}),
        )
        .await;
        assert!(bad.error.is_some(), "{id} {answer}");
    }
}

/// An order is asked at every confidence, even a sure one: nothing is kept
/// until the owner says so.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_order_is_always_asked_never_kept_on_confidence_alone() {
    let jev = FakeJev::start().unwrap();
    hears(&jev, "order", 0.99, ("everywhere", 0.95));
    let r = rig_capture(&jev, 1);
    ask(&r.core, None, "never push to main").await;
    until("the ask", || !rows(&r.core, "standing.asked").is_empty()).await;
    let l = list(&r.core).await;
    assert!(l.records.is_empty(), "{l:?}");
    assert_eq!(l.asks.len(), 1);
    assert_eq!(
        (l.asks[0].kind, l.asks[0].why.as_str()),
        (StandingKind::Order, "order")
    );
    assert!(rows(&r.core, "standing.captured").is_empty());
    let kept = call_on(
        &r.core,
        Surface::Cli,
        "standing.answer",
        json!({"id": l.asks[0].id, "answer": "keep"}),
    )
    .await;
    let res: StandingAnswerResult = serde_json::from_value(kept.result.unwrap()).unwrap();
    assert_eq!(res.record.unwrap().kind, StandingKind::Order);
}

/// A correction too faint to ask about is noted; the same one a second time
/// in the week is proposed, though each was below a half.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_correction_made_twice_is_proposed_though_each_was_below_the_band() {
    let jev = FakeJev::start().unwrap();
    hears(&jev, "correction", 0.4, ("session", 0.5));
    let r = rig_capture(&jev, 2);
    let one = ask(&r.core, None, "stop using tables for lists").await;
    until("the note", || {
        !rows(&r.core, "standing.candidate").is_empty()
    })
    .await;
    let l = list(&r.core).await;
    assert!(
        l.records.is_empty() && l.asks.is_empty(),
        "once is nothing: {l:?}"
    );
    ask(
        &r.core,
        Some(&one.session_id),
        "again, no tables for lists please",
    )
    .await;
    until("the proposal", || {
        !rows(&r.core, "standing.asked").is_empty()
    })
    .await;
    let l = list(&r.core).await;
    assert_eq!(l.asks.len(), 1, "{l:?}");
    assert_eq!(l.asks[0].why, "twice");
    assert_eq!(l.asks[0].text, "again, no tables for lists please");
    assert!(l.records.is_empty(), "proposed, not kept");
}

/// A message that says a live record again writes `standing.repeated` and no
/// record; the list shows the week's count against the record.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn said_again_is_counted_and_never_duplicated() {
    let jev = FakeJev::start().unwrap();
    hears(&jev, "preference", 0.93, ("everywhere", 0.9));
    let r = rig_capture(&jev, 3);
    let one = ask(&r.core, None, "no tables with me").await;
    until("the record", || captured(&r.core).len() == 1).await;
    ask(&r.core, Some(&one.session_id), "I said no tables with me").await;
    until("the repeat", || {
        !rows(&r.core, "standing.repeated").is_empty()
    })
    .await;
    ask(&r.core, Some(&one.session_id), "no tables with me, again").await;
    until("the second repeat", || {
        rows(&r.core, "standing.repeated").len() == 2
    })
    .await;
    assert_eq!(captured(&r.core).len(), 1, "no second record");
    let l = list(&r.core).await;
    let live = live_of(&l);
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].repeats, 2, "the week's count: {l:?}");
    let rep = rows(&r.core, "standing.repeated");
    assert_eq!(rep[1]["record"], live[0].record.id);
    assert_eq!(rep[1]["week"], 2);
}

/// A shared place's messages are never heard: the pack is not asked, nothing
/// is kept, asked or noted, whatever Jev would have said.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_shared_places_message_is_never_captured() {
    let jev = FakeJev::start().unwrap();
    hears(&jev, "preference", 0.97, ("everywhere", 0.97));
    let r = rig_capture(&jev, 2);
    let sid = crate::tests_places::session(&r.core, Some(&format!("channel:{LAB}")));
    ask(&r.core, Some(&sid), "no tables with me").await;
    // The batch has gone and settled when the fake has read both requests.
    until("the requests", || jev.connections() >= 2).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(asked_directive(&jev).is_empty(), "the pack was asked");
    assert!(captured(&r.core).is_empty());
    let l = list(&r.core).await;
    assert!(l.records.is_empty() && l.asks.is_empty());
    for kind in [
        "standing.captured",
        "standing.asked",
        "standing.candidate",
        "standing.repeated",
    ] {
        assert!(rows(&r.core, kind).is_empty(), "{kind}");
    }
    // Reached directly with a verdict, the belt holds as well.
    let m = crate::capture::act::Message {
        session: sid.clone(),
        node: "msg_x".into(),
        text: "no tables with me".into(),
        place_kind: "discord_channel (shared)".into(),
    };
    let v = crate::capture::Verdict {
        heard: crate::capture::Heard::Preference,
        band: theseus_judge::band::Band::Act,
        confidence: 0.99,
        p_correction: 0.0,
        said: crate::capture::Said::Everywhere,
        sure_of_scope: true,
        judgment: "jdg_x".into(),
        pack: "directive.v1".into(),
    };
    assert_eq!(
        r.core.capture_now(&m, &v).unwrap(),
        crate::capture::act::Done::Nothing
    );
    assert!(captured(&r.core).is_empty());
}

/// `[standing] capture = false` asks nothing and keeps nothing; a message a
/// job sent, or one through the MCP server, is not the owner's words.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn capture_off_and_a_jobs_message_hear_nothing() {
    let jev = FakeJev::start().unwrap();
    hears(&jev, "preference", 0.97, ("everywhere", 0.97));
    let off = rig(Some(&jev), 1, |c| c.standing.capture = false);
    mode(&jev, "chat", 0.95);
    ask(&off.core, None, "no tables with me").await;
    until("the requests", || jev.connections() >= 2).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(asked_directive(&jev).is_empty());
    assert!(captured(&off.core).is_empty());
    // On, a job's turn (`opened_from`) and an MCP client's are messages.
    let jev = FakeJev::start().unwrap();
    hears(&jev, "preference", 0.97, ("everywhere", 0.97));
    let on = rig_capture(&jev, 3);
    let one = ask(&on.core, None, "hello").await;
    until("the first batch", || jev.connections() >= 2).await;
    let before = asked_directive(&jev).len();
    ask_with(
        &on.core,
        json!({"session_id": one.session_id, "input": "no tables with me",
               "opened_from": one.session_id}),
    )
    .await;
    let mcp = call_on(
        &on.core,
        Surface::Mcp,
        "turn.submit",
        json!({"session_id": one.session_id, "input": "no tables with me either"}),
    )
    .await;
    let _ = mcp;
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(
        asked_directive(&jev).len(),
        before,
        "no job's or MCP client's message was asked"
    );
    assert_eq!(
        captured(&on.core).len(),
        1,
        "only the owner's first message"
    );
}

/// An answer, like any standing write, is the owner's alone from a private
/// place: from a shared channel it is refused and the ask stays open.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_answer_from_a_shared_place_is_refused() {
    let jev = FakeJev::start().unwrap();
    hears(&jev, "preference", 0.6, ("session", 0.9));
    let r = rig_capture(&jev, 1);
    ask(&r.core, None, "keep answers short").await;
    until("the ask", || !rows(&r.core, "standing.asked").is_empty()).await;
    let id = list(&r.core).await.asks[0].id.clone();
    let res = call_on(
        &r.core,
        Surface::Discord,
        "standing.answer",
        json!({"id": id, "answer": "keep",
               "author": "someone", "discord": {"user_id": "7", "channel_id": LAB,
                                                  "guild_id": "900000000000000001"}}),
    )
    .await;
    assert_eq!(res.error.unwrap().code, error_code::REFUSED);
    assert_eq!(list(&r.core).await.asks.len(), 1);
    assert!(captured(&r.core).is_empty());
}

/// What capture did reaches the session's watchers as `standing.noticed`, so
/// the terminal (`theseus watch`, the TUI) shows the notice and its undo, or
/// the ask and its answers.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_notice_and_the_ask_reach_the_sessions_watchers() {
    let jev = FakeJev::start().unwrap();
    hears(&jev, "preference", 0.93, ("everywhere", 0.9));
    let r = rig_capture(&jev, 2);
    let rec = crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    r.core.bus.watch(&rec.session_id, "watcher", tx);
    ask(&r.core, Some(&rec.session_id), "no tables with me").await;
    until("the record", || captured(&r.core).len() == 1).await;
    hears(&jev, "order", 0.95, ("everywhere", 0.9));
    ask(&r.core, Some(&rec.session_id), "never push to main").await;
    until("the ask", || !rows(&r.core, "standing.asked").is_empty()).await;
    let mut got: Vec<theseus_protocol::StandingNoticed> = vec![];
    while let Ok(m) = rx.try_recv() {
        if let theseus_protocol::Message::Notification(n) = m {
            if n.method == theseus_protocol::notify::STANDING_NOTICED {
                got.push(serde_json::from_value(n.params).unwrap());
            }
        }
    }
    assert_eq!(got.len(), 2, "{got:?}");
    assert_eq!(
        (got[0].what.as_str(), got[1].what.as_str()),
        ("kept", "asked")
    );
    assert_eq!(got[0].session_id, rec.session_id);
    assert!(got[0]
        .line
        .starts_with("📌 Kept as a standing preference everywhere (terminal)"));
    assert_eq!(got[0].id, captured(&r.core)[0].id);
    assert!(got[0].lines()[1].starts_with("Undo: theseus standing revoke std_"));
    assert!(got[1].lines()[1].contains("theseus standing answer ask_"));
}
