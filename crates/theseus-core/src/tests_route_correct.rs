//! The owner's corrections of routing (theseus-q31l; `crate::correction`,
//! `turn::route_step::correct`, `rpc/route_correct.rs`), against the fake Jev:
//! words in a private place label the corrected turn's route judgment as the
//! owner's, move the session, and steer a later close message; the same words
//! in a shared place, or from a job, do nothing; a reaction's `route.correct`
//! is the owner's alone; the layer comes back after a restart; and the
//! nightly report counts the labels.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_judge::fake::FakeJev;
use theseus_protocol::{error_code, Response, TurnSubmitResult};

use crate::approval::{Client, Surface};
use crate::rpc::Core;
use crate::tests_judge::kinds;
use crate::tests_route::{decided, mode, rig, rig_on, session, until_route_rows};

/// The owner's Discord id, in the rigs' `[places] owner`.
const OWNER: u64 = 100_000_000_000_000_001;
/// A guild channel nobody bound private: a shared place.
const LAB: &str = "900000000000000123";

fn owned(c: &mut crate::Config) {
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
async fn ask(core: &Arc<Core>, session: Option<&str>, input: &str) -> TurnSubmitResult {
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

/// The user message of `turn` in `session`: the correction's provenance.
fn message_of(core: &Core, session: &str, turn: &str) -> String {
    core.store
        .transcript(session)
        .unwrap()
        .into_iter()
        .map(|(_, n)| n)
        .find(|n| n.turn_id.as_deref() == Some(turn) && n.kind == crate::stub::Kind::UserMessage)
        .map(|n| n.id)
        .unwrap()
}

fn layer(core: &Core) -> theseus_protocol::route::RouteCorrectionsResult {
    core.route_corrections()
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

/// The brief's first test: "that should have been on fable", in a private
/// place, labels the corrected turn's route judgment `not:chat` as the
/// owner's (its provenance the message, by id) and runs on Fable at once;
/// the session stays there; a close message in another session runs there
/// too, ahead of the verdict, as `source: correction`; a message not close
/// to it does not.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn words_in_a_private_place_label_switch_and_steer_a_close_message() {
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
    let two = ask(&r.core, Some(&sid), "hey, that should have been on fable").await;
    assert_eq!(
        (two.profile.as_str(), two.model.as_str()),
        ("fable", "claude-fable-5-1")
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
            Some("mode"),
            &json!({"not": "chat"}),
            Some("operator"),
            Some(1.0),
            Some("message")
        )
    );
    let corrected = rows(&r.core, "route.corrected");
    assert_eq!(corrected.len(), 1);
    let c = &corrected[0];
    assert_eq!(c["turn"].as_str(), Some(one.turn_id.as_str()));
    assert_eq!(c["label"], l["id"]);
    assert_eq!(
        c["provenance"].as_str(),
        Some(message_of(&r.core, &sid, &two.turn_id).as_str()),
        "the owner's own message, by id"
    );
    assert_eq!(c["profile"].as_str(), Some("fable"));
    assert!(l["note"]
        .as_str()
        .unwrap()
        .contains(c["id"].as_str().unwrap()));
    // Its rows rode the turn's frame: the corrected row and the label are
    // scoped for the layer and the learning loop.
    let scoped = r
        .core
        .store
        .scope_after(crate::correction::SCOPE, 0)
        .unwrap();
    assert_eq!(scoped.len(), 1);
    // The session stays on Fable.
    let s = session(&r.core, &sid);
    assert_eq!(s.routed.unwrap().profile.as_deref(), Some("fable"));
    let three = ask(&r.core, Some(&sid), "and what about the tests for it?").await;
    assert_eq!(three.profile, "fable");
    // A close message in another session, ahead of Jev's `chat`.
    let four = ask(&r.core, None, CLOSE).await;
    assert_eq!(four.profile, "fable", "the layer steers it");
    let d = decided(&r.core.store);
    let last = d.last().unwrap();
    assert_eq!(
        (last["source"].as_str(), last["follows"].as_str()),
        (Some("correction"), l["id"].as_str())
    );
    // Not close: Jev's verdict.
    let five = ask(&r.core, None, "what time is low tide at the harbour today").await;
    assert_eq!(five.profile, "sonnet");
    assert_eq!(five.route.unwrap().source, None);
    let listed = layer(&r.core);
    assert_eq!(listed.entries.len(), 1);
    assert_eq!(
        (
            listed.entries[0].to.as_str(),
            listed.entries[0].turn_id.as_str()
        ),
        ("fable", one.turn_id.as_str())
    );
    nightly_counts(&r.core, &judgment).await;
}

/// The same words in a shared place do nothing: no label, no row, no move.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_same_words_in_a_shared_place_do_nothing() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    let r = rig(Some(&jev), 3, owned);
    let sid = crate::tests_places::session(&r.core, Some(&format!("channel:{LAB}")));
    let one = ask(&r.core, Some(&sid), PARSER).await;
    assert_eq!(one.profile, "sonnet");
    let two = ask(&r.core, Some(&sid), "that should have been on fable").await;
    assert_eq!(two.profile, "sonnet");
    assert_eq!(two.route.unwrap().source, None);
    assert!(rows(&r.core, "judge.label").is_empty());
    assert!(rows(&r.core, "route.corrected").is_empty());
    assert!(session(&r.core, &sid).routed.is_none());
    assert!(layer(&r.core).entries.is_empty());
}

/// The model cannot label or correct: no tool it is offered reaches either,
/// a turn a job sent (`opened_from`) is a message whatever it says, and
/// `route.correct` through the MCP server or an unnamed surface is refused,
/// writing nothing.
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
    let two = ask_with(
        &r.core,
        json!({"session_id": sid, "input": "that should have been on fable", "opened_from": sid}),
    )
    .await;
    assert_eq!(two.profile, "sonnet", "a job's words are a message");
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
    assert_eq!(l["label"], json!({"not": "chat"}));
    // The session's next turn runs there, whatever Jev says of it.
    let two = ask(&r.core, Some(&sid), "go on").await;
    assert_eq!(two.profile, to);
    assert_eq!(two.route.unwrap().source.as_deref(), Some("correction"));
    // A press of a footer's control from the cockpit: the CLI's surface
    // and the web's are private, and a mode names its first profile.
    let res = call_on(
        &r.core,
        Surface::Web,
        "route.correct",
        json!({"session_id": sid, "to": "deep_coding", "via": "cockpit", "provenance": "press"}),
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

/// The layer comes back after a restart, from its rows, once the socket
/// answers (`warm_corrections`), and lists the same entry.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_layer_is_rebuilt_from_its_rows_after_a_restart() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    let r = rig(Some(&jev), 4, owned);
    let one = ask(&r.core, None, PARSER).await;
    ask(&r.core, Some(&one.session_id), "use fable for that").await;
    until_route_rows(&r.core.store, 2).await;
    assert_eq!(layer(&r.core).entries.len(), 1);
    let dir = r.dir.clone();
    drop(r);
    let again = rig_on(dir, Some(&jev), 2, owned, crate::tests_judge::board());
    assert!(
        layer(&again.core).entries.is_empty(),
        "nothing read on the start path"
    );
    again.core.warm_corrections();
    let t0 = Instant::now();
    while layer(&again.core).entries.is_empty() {
        assert!(
            t0.elapsed() < Duration::from_secs(10),
            "the layer was not rebuilt"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let l = layer(&again.core);
    assert_eq!(
        (l.entries[0].turn_id.as_str(), l.entries[0].to.as_str()),
        (one.turn_id.as_str(), "fable")
    );
    let close = ask(&again.core, None, CLOSE).await;
    assert_eq!(close.profile, "fable");
}

/// Words in a guild channel bound private are a message (the review's join
/// fix): the place is private, but anyone the channel lets in may write
/// there, and a label is the owner's act (`judge.label`'s rule, which a
/// reaction there keeps: it is judged by its reactor). The owner's DM, and
/// the CLI and the web UI, still count.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn words_in_a_private_guild_channel_are_a_message_and_a_dms_count() {
    use crate::places::BoundPlace;
    use theseus_protocol::PlaceClass;
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    let r = rig(Some(&jev), 4, owned);
    r.core.bind_places(vec![BoundPlace {
        target: format!("discord:channel:{LAB}"),
        name: "#lab".into(),
        private: true,
        ..Default::default()
    }]);
    let lab = crate::tests_places::session(&r.core, Some(&format!("channel:{LAB}")));
    assert_eq!(r.core.runner.class_of(&lab), PlaceClass::Private);
    let one = ask(&r.core, Some(&lab), PARSER).await;
    assert_eq!(one.profile, "sonnet");
    let two = ask_with(
        &r.core,
        json!({"session_id": lab, "input": "that should have been on fable",
               "author": "discord:collaborator"}),
    )
    .await;
    assert_eq!(two.profile, "sonnet", "a message, not a correction");
    assert!(rows(&r.core, "judge.label").is_empty());
    assert!(rows(&r.core, "route.corrected").is_empty());
    // The owner's DM: a correction.
    let dm = crate::tests_places::session(&r.core, Some(&format!("dm:{OWNER}")));
    assert_eq!(r.core.runner.class_of(&dm), PlaceClass::Private);
    ask(&r.core, Some(&dm), PARSER).await;
    let fixed = ask(&r.core, Some(&dm), "that should have been on fable").await;
    assert_eq!(fixed.profile, "fable");
    assert_eq!(rows(&r.core, "route.corrected").len(), 1);
}

/// No Jev request on the turn path: a correcting turn asks Jev what a plain
/// message's turn asks, and nothing more (the review's check of the claim).
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
    let two = ask(&r.core, Some(&sid), "that should have been on fable").await;
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
