//! The owner's corrections of routing (theseus-q31l; `crate::correction`,
//! `turn::route_step::correct`, `rpc/route_correct.rs`), against the fake Jev:
//! a typed correction (`route.correct`: a press, a reaction, the CLI) labels
//! the corrected turn's route judgment as the owner's and moves the session's
//! next turn, and steers no other message; it is the owner's alone; the
//! owner's words in a message are only a message, in every place; and the
//! nightly report counts the labels.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_judge::fake::FakeJev;
use theseus_protocol::{error_code, Response, TurnSubmitResult};

use crate::approval::{Client, Surface};
use crate::rpc::Core;
use crate::tests_judge::kinds;
use crate::tests_route::{decided, mode, rig, session};

/// The owner's Discord id, in the rigs' `[places] owner`.
const OWNER: u64 = 100_000_000_000_000_001;
/// A guild channel nobody bound private: a shared place.
const LAB: &str = "900000000000000123";

pub(crate) fn owned(c: &mut crate::Config) {
    c.places.owner = Some(vec![format!("discord:{OWNER}")]);
}

/// One request through the protocol server on `surface`, as a client sends
/// it: its response, errors included.
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

/// A message from the CLI, as `theseus ask` sends it.
pub(crate) async fn ask(core: &Arc<Core>, session: Option<&str>, input: &str) -> TurnSubmitResult {
    ask_with(core, json!({"session_id": session, "input": input})).await
}

async fn ask_with(core: &Arc<Core>, params: Value) -> TurnSubmitResult {
    let r = call_on(core, Surface::Cli, "turn.submit", params).await;
    assert!(r.error.is_none(), "{:?}", r.error);
    serde_json::from_value(r.result.unwrap()).unwrap()
}

fn rows(core: &Core, kind: &str) -> Vec<Value> {
    kinds(&core.store, kind)
        .into_iter()
        .map(|r| r.data)
        .collect()
}

/// The nightly loop reads the owner's label as it reads any operator label,
/// once the judge's sink has written the judgment it labels.
async fn nightly_counts(core: &Arc<Core>, judgment: &str) {
    let t0 = Instant::now();
    while core.store.ledger_by_key(judgment).unwrap().is_none() {
        assert!(t0.elapsed() < Duration::from_secs(20), "the judgment's row");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let report = core
        .run_learning(theseus_protocol::now_unix_ms(), "on_demand", |_| {})
        .unwrap();
    assert!(report.labels.operator >= 1, "{:?}", report.labels);
    let route = report
        .packs
        .iter()
        .find(|p| p.pack == crate::judge::inbound::ROUTE_PACK)
        .unwrap();
    assert!(route.labeled >= 1, "{route:?}");
}

const PARSER: &str = "Write a parser for the lighthouse log format in Rust";
const CLOSE: &str = "write a parser for the harbour log format in rust";

/// The typed correction a footer's press makes: the CLI's `route.correct`.
async fn correct(core: &Arc<Core>, session: &str, to: &str) -> Response {
    call_on(
        core,
        Surface::Cli,
        "route.correct",
        json!({"session_id": session, "to": to, "via": "cli", "provenance": "test"}),
    )
    .await
}

/// A typed correction (`route.correct`, as a footer's press or `theseus judge
/// correct` sends it) labels the corrected turn's route judgment `not:sonnet`
/// as the owner's (its provenance the press), moves the session's next turn
/// to Fable as `source: correction`, and steers nothing else: the turn after
/// is Jev's again, and a message close to the corrected one, in another
/// session, is Jev's too.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_typed_correction_labels_and_switches_one_turn_and_steers_no_other() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    let r = rig(Some(&jev), 6, owned);
    let one = ask(&r.core, None, PARSER).await;
    let sid = one.session_id.clone();
    assert_eq!(one.profile, "sonnet");
    let judgment = decided(&r.core.store)[0]["judgment"]
        .as_str()
        .unwrap()
        .to_string();
    let res = correct(&r.core, &sid, "fable").await;
    assert!(res.error.is_none(), "{:?}", res.error);
    let two = ask(&r.core, Some(&sid), "and what about the tests for it?").await;
    assert_eq!(
        (two.profile.as_str(), two.model.as_str()),
        ("fable", "claude-fable-5-2")
    );
    let route = two.route.clone().unwrap();
    assert_eq!(
        (route.reason.as_str(), route.source.as_deref()),
        ("correction", Some("correction"))
    );
    // The owner's label, on the first turn's judgment.
    let labels = rows(&r.core, "judge.label");
    assert_eq!(labels.len(), 1, "{labels:?}");
    let l = &labels[0];
    assert_eq!(
        (
            l["judgment"].as_str(),
            l["question"].as_str(),
            &l["label"],
            l["source"].as_str(),
            l["weight"].as_f64(),
            l["via"].as_str()
        ),
        (
            Some(judgment.as_str()),
            Some("model"),
            &json!({"not": "sonnet"}),
            Some("operator"),
            Some(1.0),
            Some("cli")
        )
    );
    let corrected = rows(&r.core, "route.corrected");
    assert_eq!(corrected.len(), 1);
    let c = &corrected[0];
    assert_eq!(c["turn"].as_str(), Some(one.turn_id.as_str()));
    assert_eq!(c["label"], l["id"]);
    assert_eq!(c["provenance"].as_str(), Some("test"));
    assert_eq!(c["profile"].as_str(), Some("fable"));
    assert!(l["note"]
        .as_str()
        .unwrap()
        .contains(c["id"].as_str().unwrap()));
    // Its rows are scoped for the learning loop.
    let scoped = r
        .core
        .store
        .scope_after(crate::correction::SCOPE, 0)
        .unwrap();
    assert_eq!(scoped.len(), 1);
    // The session stays on Fable.
    let s = session(&r.core, &sid);
    assert_eq!(s.routed.unwrap().profile.as_deref(), Some("fable"));
    // Nothing holds it there afterwards: no step down is left, so the next turn goes where Jev says.
    let three = ask(&r.core, Some(&sid), "and the docs?").await;
    assert_eq!(three.profile, "sonnet", "Jev chooses again, at once");
    // A message close to the corrected one, in another session: Jev's.
    let four = ask(&r.core, None, CLOSE).await;
    assert_eq!(four.profile, "sonnet", "no layer steers it");
    assert_eq!(four.route.unwrap().source, None);
    nightly_counts(&r.core, &judgment).await;
}

/// The owner's words are a message, in every place: "that should have been on
/// fable" and "use opus for that" write no label and no row, move no session,
/// and run where Jev says, from the CLI, a DM, a shared channel, and a guild
/// channel bound private.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_owners_words_are_a_message_in_every_place() {
    use crate::places::BoundPlace;
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    let r = rig(Some(&jev), 12, owned);
    r.core.bind_places(vec![BoundPlace {
        target: format!("discord:channel:{LAB}"),
        name: "#lab".into(),
        private: true,
        ..Default::default()
    }]);
    let shared = crate::tests_places::session(&r.core, Some("channel:900000000000000456"));
    let lab = crate::tests_places::session(&r.core, Some(&format!("channel:{LAB}")));
    let dm = crate::tests_places::session(&r.core, Some(&format!("dm:{OWNER}")));
    for place in [None, Some(shared), Some(lab), Some(dm)] {
        let one = ask(&r.core, place.as_deref(), PARSER).await;
        for words in ["that should have been on fable", "hey, use opus for that"] {
            let two = ask(&r.core, Some(&one.session_id), words).await;
            assert_eq!(two.profile, "sonnet", "{words}: a message");
            assert_eq!(two.route.unwrap().source, None);
        }
        assert!(session(&r.core, &one.session_id).routed.is_none());
    }
    assert!(rows(&r.core, "judge.label").is_empty());
    assert!(rows(&r.core, "route.corrected").is_empty());
}

/// The model cannot label or correct: no tool it is offered reaches either,
/// and `route.correct` through the MCP server or an unnamed surface is
/// refused, writing nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_model_cannot_label_or_correct() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    let r = rig(Some(&jev), 3, owned);
    for d in r.core.runner.tools.definitions() {
        let name = d["name"].as_str().unwrap_or_default();
        for w in ["label", "correct", "judge", "route"] {
            assert!(!name.contains(w), "the model is offered {name}");
        }
    }
    let one = ask(&r.core, None, PARSER).await;
    let sid = one.session_id.clone();
    for surface in [Surface::Mcp, Surface::Unnamed] {
        let res = call_on(
            &r.core,
            surface,
            "route.correct",
            json!({"session_id": sid, "to": "fable"}),
        )
        .await;
        let e = res.error.expect("refused");
        assert_eq!(e.code, error_code::REFUSED, "{e:?}");
    }
    assert!(rows(&r.core, "judge.label").is_empty());
    assert!(rows(&r.core, "route.corrected").is_empty());
    assert!(session(&r.core, &sid).routed.is_none());
}

/// A Discord origin: the owner (or another person) in their DM, or in a
/// guild channel.
fn discord(user: u64, guild_channel: Option<&str>) -> Value {
    json!({
        "user_id": user.to_string(),
        "channel_id": guild_channel.map_or_else(|| (user + 1).to_string(), str::to_string),
        "guild_id": guild_channel.map(|_| "900000000000000001"),
    })
}

/// A reaction (`route.correct` from the binding): the owner's, in their DM,
/// labels the turn and moves the session's next turn to the next stronger
/// profile; the same from a shared channel, or from anyone else, is refused
/// and writes nothing. A footer's press is the same call.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reaction_and_a_press_are_the_owners_alone() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    let r = rig(Some(&jev), 4, owned);
    let one = ask(&r.core, None, PARSER).await;
    let sid = one.session_id.clone();
    for (who, place) in [(OWNER, Some(LAB)), (OWNER + 7, None)] {
        let res = call_on(
            &r.core,
            Surface::Discord,
            "route.correct",
            json!({"session_id": sid, "turn_id": one.turn_id, "to": "stronger",
                   "via": "reaction", "provenance": "msg_1:up", "discord": discord(who, place)}),
        )
        .await;
        assert_eq!(res.error.expect("refused").code, error_code::REFUSED);
    }
    assert!(rows(&r.core, "route.corrected").is_empty());
    let res = call_on(
        &r.core,
        Surface::Discord,
        "route.correct",
        json!({"session_id": sid, "turn_id": one.turn_id, "to": "stronger",
               "via": "reaction", "provenance": "1234567890:⬆️", "discord": discord(OWNER, None)}),
    )
    .await;
    assert!(res.error.is_none(), "{:?}", res.error);
    let done: theseus_protocol::route::RouteCorrectResult =
        serde_json::from_value(res.result.unwrap()).unwrap();
    let to = done.profile.clone().unwrap();
    assert_ne!(to, "sonnet");
    assert!(done.label.is_some());
    let c = &rows(&r.core, "route.corrected")[0];
    assert_eq!(
        (c["via"].as_str(), c["provenance"].as_str()),
        (Some("reaction"), Some("1234567890:⬆️"))
    );
    let l = &rows(&r.core, "judge.label")[0];
    assert_eq!(l["label"], json!({"not": "sonnet"}));
    // The session's next turn runs there, whatever Jev says of it.
    let two = ask(&r.core, Some(&sid), "go on").await;
    assert_eq!(two.profile, to);
    assert_eq!(two.route.unwrap().source.as_deref(), Some("correction"));
    // A press of a footer's control from the cockpit: the CLI's surface
    // and the web's are private, and the press names a profile.
    let res = call_on(
        &r.core,
        Surface::Web,
        "route.correct",
        json!({"session_id": sid, "to": "opus", "via": "cockpit", "provenance": "press"}),
    )
    .await;
    assert!(res.error.is_none(), "{:?}", res.error);
    let res = call_on(
        &r.core,
        Surface::Cli,
        "route.correct",
        json!({"session_id": sid, "to": "mars"}),
    )
    .await;
    let e = res.error.expect("an unknown name");
    assert!(e.message.contains("names no profile"), "{}", e.message);
}

/// No Jev request on the turn path: a turn a typed correction steers asks Jev
/// what a plain message's turn asks, and nothing more (the review's check of
/// the claim).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_correction_asks_jev_nothing_a_plain_turn_does_not() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    let r = rig(Some(&jev), 4, owned);
    // Every request settled: the count still for 400 ms.
    let settled = || async {
        let mut n = jev.seen().len();
        let mut still = Instant::now();
        let t0 = Instant::now();
        while still.elapsed() < Duration::from_millis(400) {
            assert!(
                t0.elapsed() < Duration::from_secs(20),
                "Jev's requests never settled"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
            let m = jev.seen().len();
            if m != n {
                (n, still) = (m, Instant::now());
            }
        }
        n
    };
    let one = ask(&r.core, None, PARSER).await;
    let sid = one.session_id.clone();
    let before = settled().await;
    assert!(correct(&r.core, &sid, "fable").await.error.is_none());
    let two = ask(&r.core, Some(&sid), "and the tests for it?").await;
    assert_eq!(two.route.unwrap().source.as_deref(), Some("correction"));
    let corrected = settled().await;
    ask(&r.core, Some(&sid), "and what about the tests for it?").await;
    let plain = settled().await;
    assert_eq!(rows(&r.core, "route.corrected").len(), 1);
    assert_eq!(
        corrected - before,
        plain - corrected,
        "the correcting turn asked Jev {} requests, a plain one {}",
        corrected - before,
        plain - corrected
    );
}

/// The exam harvester still makes a routing-correction candidate from a typed
/// correction's ACTUAL stored row, which no longer carries the message's
/// words: it reads the corrected message back from the row's own session and
/// turn. A row of the old shape (with `words`) still works, and a row whose
/// message cannot be read makes none.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_typed_correction_makes_one_exam_candidate_and_an_old_row_still_does() {
    use crate::ledger::LedgerRow;
    use crate::rsi::exams::{harvest, Family};
    use theseus_protocol::LedgerKind;
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    let r = rig(Some(&jev), 3, owned);
    let one = ask(&r.core, None, PARSER).await;
    let res = correct(&r.core, &one.session_id, "fable").await;
    assert!(res.error.is_none(), "{:?}", res.error);
    let now = theseus_protocol::now_unix_ms();
    let raws = r.core.exam_raws(0, now + 60_000, now).unwrap();
    let made: Vec<_> = raws
        .iter()
        .filter(|x| x.family == Family::Correction)
        .collect();
    assert_eq!(made.len(), 1, "{raws:?}");
    assert_eq!(made[0].check, "reply has word \"fable\"");
    assert!(
        made[0].task.contains("lighthouse log format"),
        "{:?}",
        made[0].task
    );
    assert!(
        rows(&r.core, "route.corrected")[0].get("words").is_none(),
        "the stored row carries no words"
    );
    // The old shape: `words`, and no message to read.
    let mut old = LedgerRow::new(
        LedgerKind::RouteCorrected,
        Some("ses_old"),
        None,
        json!({"id": "rcx_old", "turn": "turn_old", "session": "ses_old", "ran": "haiku",
               "to": "fable", "words": ["harbor", "tide", "gauge"]}),
    );
    old.at_unix_ms = 5;
    let raws = harvest::from_rows(&[(1, old)], &|_| false);
    assert_eq!(raws.len(), 1);
    assert!(
        raws[0].task.contains("harbor tide gauge"),
        "{:?}",
        raws[0].task
    );
    // A new-shape row whose message is gone makes none.
    let mut gone = LedgerRow::new(
        LedgerKind::RouteCorrected,
        Some("ses_gone"),
        None,
        json!({"id": "rcx_g", "turn": "turn_g", "session": "ses_gone", "ran": "haiku", "to": "fable"}),
    );
    gone.at_unix_ms = 5;
    let mut rows_gone = vec![(2, gone)];
    r.core.read_corrected_messages(&mut rows_gone);
    assert!(harvest::from_rows(&rows_gone, &|_| false).is_empty());
}
