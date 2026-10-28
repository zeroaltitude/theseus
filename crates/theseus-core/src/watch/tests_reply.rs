//! The `reply` check through the whole core (theseus-rlnbf): a watch that waits
//! for someone to answer, told by the Discord binding of each message and
//! never checking on a timer.
//!
//! The binding's call is `Core::watch_reply_seen`; a test makes it the way
//! the binding does (`say`). A shared channel `#ops` and the owner's DM are
//! bound, the DM private.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::{Message, Request};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::reply::Heard;
use super::tests_watch::{dm_session, life, posts, rows, rpc, until, Life, TARGET};
use crate::approval::{Client, Surface};
use crate::Core;

const OPS: &str = "discord:channel:777";
const HARBOUR: &str = "91";
const STRANGER: &str = "92";

fn bind_ops(core: &Arc<Core>) {
    core.bind_places(vec![
        crate::places::BoundPlace {
            target: TARGET.into(),
            name: "DM @owner".into(),
            private: true,
            ..Default::default()
        },
        crate::places::BoundPlace {
            target: OPS.into(),
            name: "#ops".into(),
            private: false,
            ..Default::default()
        },
    ]);
}

/// The binding hears `text` from `author` in `place`.
fn say(core: &Arc<Core>, place: &str, author_id: &str, author: &str, id: &str, text: &str) {
    core.watch_reply_seen(&Heard {
        place,
        author_id,
        author,
        message_id: id,
        text,
        link: Some(format!("https://discord.com/channels/1/777/{id}")),
    });
}

async fn reply_watch(core: &Arc<Core>, extra: Value) -> Value {
    let mut check = json!({"kind": "reply", "place": "#ops"});
    for (k, v) in extra.as_object().cloned().unwrap_or_default() {
        check[k] = v;
    }
    rpc(
        core,
        "watch.create",
        json!({"title": "harbour master replies", "check": check, "for_at_most": "5d"}),
    )
    .await
    .unwrap()
}

/// `watch.create` as a Discord person, from `channel` (None: their DM).
async fn create_as(core: &Arc<Core>, user: u64, channel: Option<u64>) -> Result<Value, String> {
    let (ours, theirs) = tokio::io::duplex(64 * 1024);
    let (sr, sw) = tokio::io::split(theirs);
    let srv = tokio::spawn(core.clone().serve_connection(
        sr,
        sw,
        Client::new("discord#1", Surface::Discord),
    ));
    let (cr, mut cw) = tokio::io::split(ours);
    let params = json!({
        "title": "harbour master replies",
        "check": {"kind": "reply", "place": "#ops"},
        "discord": {"user_id": user.to_string(), "channel_id": channel.unwrap_or(user + 1).to_string(),
                    "guild_id": channel.map(|_| "900000000000000001")},
    });
    let req = Request::new(theseus_protocol::Id::Num(1), "watch.create", params);
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let mut lines = BufReader::new(cr).lines();
    let out = loop {
        let l = lines.next_line().await.unwrap().unwrap();
        if let Message::Response(r) = serde_json::from_str(&l).unwrap() {
            break r.result.ok_or_else(|| r.error.unwrap().message);
        }
    };
    cw.shutdown().await.unwrap();
    drop(lines);
    let _ = srv.await;
    out
}

async fn settle() {
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
}

fn rig(dir: &std::path::Path, drive: bool) -> Life {
    let l = life(dir, drive, |_| {});
    bind_ops(&l.core);
    l
}

/// A reply from the named person fires; another person's does not. The notice
/// goes to the owner's DM (private) and never into #ops, quotes the reply
/// (as it came, the first 200 characters), and the row is marked outside text.
#[tokio::test(flavor = "multi_thread")]
async fn a_reply_from_the_named_person_fires_and_another_persons_does_not() {
    let dir = tempfile::tempdir().unwrap();
    let l = rig(dir.path(), false);
    dm_session(&l.core);
    let made = reply_watch(&l.core, json!({"from": "harbourmaster"})).await;
    assert_eq!(made["watch"]["kind"], "reply", "{made}");
    assert_eq!(made["watch"]["every"], "on arrival");
    assert!(made["watch"]["target"].as_str().unwrap().contains("#ops"));
    let id = made["watch"]["wake_id"].as_str().unwrap().to_string();
    say(
        &l.core,
        OPS,
        STRANGER,
        "deckhand",
        "1001",
        "when do we sail?",
    );
    say(
        &l.core,
        "discord:channel:555",
        HARBOUR,
        "harbourmaster",
        "1002",
        "wrong channel",
    );
    settle().await;
    assert!(rows(&l.core, "watch.met").is_empty(), "not them, not there");
    assert!(l.core.watches.get(&id).is_some());
    let token = format!("{}{}", "ghp_", "a1B2c3D4e5".repeat(4));
    let long = format!("Berth four is yours at dawn {token} {}", "x".repeat(400));
    say(&l.core, OPS, HARBOUR, "harbourmaster", "1003", &long);
    until("the met row", 10, || !rows(&l.core, "watch.met").is_empty()).await;
    let met = rows(&l.core, "watch.met").remove(0);
    assert_eq!(met["message_id"], "1003");
    assert_eq!(met["external"], true);
    let value = met["value"].as_str().unwrap();
    assert!(value.starts_with("Berth four is yours at dawn"), "{value}");
    assert!(value.contains("ghp_"), "the record is raw: {value}");
    assert!(value.chars().count() <= 201, "the first 200 characters");
    let all = posts(&l.core);
    assert_eq!(all.len(), 1, "{all:?}");
    assert_eq!(all[0].0, TARGET, "the owner's private place");
    assert!(
        all[0]
            .1
            .starts_with("👁 harbourmaster replied in #ops: \"Berth four"),
        "{}",
        all[0].1
    );
    assert!(all[0].1.contains("https://discord.com/channels/1/777/1003"));
    assert!(
        all[0].1.contains("ghp_"),
        "the post is the courier's to substitute: {}",
        all[0].1
    );
    assert!(
        l.core.watches.get(&id).is_none(),
        "met: gone from the board"
    );
    assert!(l.core.kernel.open_watches().unwrap().is_empty());
    // A second message is nothing: the watch is gone.
    say(&l.core, OPS, HARBOUR, "harbourmaster", "1004", "and again");
    settle().await;
    assert_eq!(rows(&l.core, "watch.met").len(), 1);
    assert!(l.model.requests().is_empty(), "no model call");
}

/// A met watch leaves the board before its row can be read (theseus-1b2zm): looked
/// at the instant the frame lands, on the thread that writes it. A reader that finds `watch.met` and then asks the
/// board never finds the watch still waiting.
#[tokio::test(flavor = "multi_thread")]
async fn a_met_reply_watch_is_off_the_board_when_its_row_is_written() {
    let dir = tempfile::tempdir().unwrap();
    let l = rig(dir.path(), false);
    dm_session(&l.core);
    let made = reply_watch(&l.core, json!({"words": "berth"})).await;
    let id = made["watch"]["wake_id"].as_str().unwrap().to_string();
    let (core, looked) = (Arc::downgrade(&l.core), Arc::new(AtomicUsize::new(0)));
    super::run::tests_hook::add({
        let looked = looked.clone();
        move |wake| {
            if wake != id {
                return;
            }
            let core = core.upgrade().expect("the core");
            assert!(
                core.watches.get(wake).is_none(),
                "the row is written and the watch still waits"
            );
            looked.fetch_add(1, Ordering::Relaxed);
        }
    });
    say(
        &l.core,
        OPS,
        STRANGER,
        "deckhand",
        "1",
        "berth four is free",
    );
    until("it was met", 10, || rows(&l.core, "watch.met").len() == 1).await;
    until("the frame was looked at", 10, || {
        looked.load(Ordering::Relaxed) == 1
    })
    .await;
}

/// `words` match as text in any case, or as a /pattern/; both given with
/// `from`, both must hold.
#[tokio::test(flavor = "multi_thread")]
async fn words_match_as_text_or_a_pattern_and_with_from_both_must_hold() {
    let dir = tempfile::tempdir().unwrap();
    let l = rig(dir.path(), false);
    dm_session(&l.core);
    let text = reply_watch(&l.core, json!({"words": "Berth Four"})).await;
    let re = reply_watch(&l.core, json!({"words": "/sail(s|ing)? (at|by) \\d+/"})).await;
    let both = reply_watch(
        &l.core,
        json!({"words": "dawn", "from": format!("discord:{HARBOUR}")}),
    )
    .await;
    say(&l.core, OPS, STRANGER, "deckhand", "2001", "nothing to see");
    settle().await;
    assert!(rows(&l.core, "watch.met").is_empty());
    say(
        &l.core,
        OPS,
        STRANGER,
        "deckhand",
        "2002",
        "we sail at 6 and berth FOUR is free",
    );
    until("two met", 10, || rows(&l.core, "watch.met").len() == 2).await;
    for w in [&text, &re] {
        let id = w["watch"]["wake_id"].as_str().unwrap();
        assert!(l.core.watches.get(id).is_none());
    }
    let both_id = both["watch"]["wake_id"].as_str().unwrap();
    assert!(
        l.core.watches.get(both_id).is_some(),
        "right words, wrong person"
    );
    say(&l.core, OPS, HARBOUR, "anyone", "2003", "at dawn then");
    until("three met", 10, || rows(&l.core, "watch.met").len() == 3).await;
    // A pattern that does not compile is refused at the owner's verb.
    let bad = rpc(
        &l.core,
        "watch.create",
        json!({"title": "x", "check": {"kind": "reply", "place": "#ops", "words": "/(/"}}),
    )
    .await
    .unwrap_err();
    assert!(bad.contains("pattern"), "{bad}");
    let turn = rpc(
        &l.core,
        "watch.create",
        json!({"title": "x", "check": {"kind": "reply", "place": "#ops"}, "then": {"kind": "turn"}}),
    )
    .await
    .unwrap_err();
    assert!(turn.contains("outside text"), "{turn}");
    let nowhere = rpc(
        &l.core,
        "watch.create",
        json!({"title": "x", "check": {"kind": "reply", "place": "#nowhere"}}),
    )
    .await
    .unwrap_err();
    assert!(nowhere.contains("no place"), "{nowhere}");
}

/// A restart rebuilds the index with the watches, and a reply after it fires.
#[tokio::test(flavor = "multi_thread")]
async fn a_restart_rebuilds_the_index_and_a_reply_after_it_fires() {
    let dir = tempfile::tempdir().unwrap();
    let first = rig(dir.path(), false);
    dm_session(&first.core);
    let made = reply_watch(&first.core, json!({"from": "harbourmaster"})).await;
    let id = made["watch"]["wake_id"].as_str().unwrap().to_string();
    drop(first.stop().await);
    let second = rig(dir.path(), false);
    assert!(
        second
            .core
            .watches
            .reply_matches(OPS, "1", "harbourmaster", "hi")
            .is_empty(),
        "nothing is indexed before the load"
    );
    second.core.watch_load();
    assert_eq!(
        second
            .core
            .watches
            .reply_matches(OPS, "1", "harbourmaster", "hi"),
        vec![id.clone()],
        "the load rebuilt the index"
    );
    assert_eq!(
        second.core.watches.get(&id).unwrap().next_ms,
        second.core.watches.get(&id).unwrap().spec.expires_ms,
        "never due on the tick"
    );
    say(
        &second.core,
        OPS,
        STRANGER,
        "harbourmaster",
        "3001",
        "back at last",
    );
    until("the met row", 10, || {
        !rows(&second.core, "watch.met").is_empty()
    })
    .await;
    assert_eq!(rows(&second.core, "watch.met")[0]["message_id"], "3001");
}

/// The watch is never due on the tick and gives up at its expiry like any.
#[tokio::test(flavor = "multi_thread")]
async fn a_reply_watch_is_never_due_and_expires_like_any() {
    let dir = tempfile::tempdir().unwrap();
    let l = rig(dir.path(), false);
    dm_session(&l.core);
    let made = reply_watch(&l.core, json!({})).await;
    let id = made["watch"]["wake_id"].as_str().unwrap().to_string();
    let e = l.core.watches.get(&id).unwrap();
    assert_eq!(e.next_ms, e.spec.expires_ms);
    assert!(
        l.core
            .watches
            .take_due(l.core.kernel.now_ms() + 86_400_000)
            .is_empty(),
        "a day on, still not due"
    );
    assert_eq!(e.checks, 0);
    // The expiry is its one look.
    let past = l.core.kernel.now_ms() - 1;
    l.core.watches.with(&id, |e| {
        e.spec.expires_ms = past;
        e.next_ms = past;
    });
    assert_eq!(l.core.watches.take_due(l.core.kernel.now_ms()).len(), 1);
    super::tests_watch::look(&l.core, &id).await;
    assert_eq!(rows(&l.core, "watch.expired").len(), 1);
    assert!(l.core.kernel.open_watches().unwrap().is_empty());
    assert!(
        l.core.watches.reply_matches(OPS, "1", "x", "y").is_empty(),
        "out of the index"
    );
    assert!(posts(&l.core)[0]
        .1
        .contains("gave up on \"harbour master replies\""));
}

/// A watch that lives in a shared place's session still tells the owner in a
/// private place: never into the channel the reply was written in.
#[tokio::test(flavor = "multi_thread")]
async fn the_notice_never_goes_into_a_shared_place() {
    let dir = tempfile::tempdir().unwrap();
    let l = rig(dir.path(), false);
    dm_session(&l.core);
    let ops = l
        .core
        .open_session(theseus_protocol::SessionOpenParams {
            dir: None,
            kind: None,
            label: None,
            opened_from: None,
            persona: None,
        })
        .unwrap()
        .session_id;
    l.core.outbox.bind_place("channel:777", &ops).unwrap();
    let made = rpc(
        &l.core,
        "watch.create",
        json!({"title": "harbour master replies", "session_id": ops,
               "check": {"kind": "reply", "place": "#ops", "from": "harbourmaster"}}),
    )
    .await
    .unwrap();
    assert_eq!(made["watch"]["session_id"], ops);
    say(&l.core, OPS, HARBOUR, "harbourmaster", "4001", "ahoy");
    until("the notice", 10, || !posts(&l.core).is_empty()).await;
    let all = posts(&l.core);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].0, TARGET, "the owner's DM, not {}", all[0].0);
}

/// A reply-watch is the owner's act from a private place: a person in the
/// shared #ops cannot set one, and the owner can from the DM.
#[tokio::test(flavor = "multi_thread")]
async fn a_shared_place_cannot_set_a_reply_watch() {
    let dir = tempfile::tempdir().unwrap();
    let l = rig(dir.path(), false);
    dm_session(&l.core);
    let refused = create_as(&l.core, 4242, Some(777)).await.unwrap_err();
    assert!(refused.contains("does not count"), "{refused}");
    let also_the_owner_in_it = create_as(&l.core, 42, Some(777)).await.unwrap_err();
    assert!(
        also_the_owner_in_it.contains("does not count"),
        "{also_the_owner_in_it}"
    );
    assert!(l.core.kernel.open_watches().unwrap().is_empty());
    let ok = create_as(&l.core, 42, None).await.unwrap();
    assert_eq!(ok["watch"]["kind"], "reply");
    assert_eq!(l.core.kernel.open_watches().unwrap().len(), 1);
}

/// FAST: the binding's call is cheap. With none open it is one load; with one
/// open for another place it is a map lookup. A burst of 1,000 messages is
/// well under a millisecond of work in total in a debug build, and the work is
/// the same whether the one watch is for this place or another.
#[tokio::test(flavor = "multi_thread")]
async fn a_burst_of_messages_costs_one_lookup_each() {
    let dir = tempfile::tempdir().unwrap();
    let l = rig(dir.path(), false);
    dm_session(&l.core);
    let t = |core: &Arc<Core>| {
        let t0 = std::time::Instant::now();
        for i in 0..1_000 {
            say(
                core,
                "discord:channel:555",
                STRANGER,
                "deckhand",
                &i.to_string(),
                "chatter",
            );
        }
        t0.elapsed()
    };
    let none = t(&l.core);
    reply_watch(&l.core, json!({"from": "harbourmaster"})).await;
    let other = t(&l.core);
    eprintln!(
        "1,000 messages: {none:?} with no reply watch open, {other:?} with one for another place"
    );
    assert!(none < std::time::Duration::from_millis(50), "{none:?}");
    assert!(other < std::time::Duration::from_millis(50), "{other:?}");
}
