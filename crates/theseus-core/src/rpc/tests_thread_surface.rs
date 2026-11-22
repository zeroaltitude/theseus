//! What a place tells the model of its routes (once a thread on Discord's core half, theseus-sjzm):
//! the route hints by place, on plain sessions, and an old client's `thread` field, which runs in the
//! session it names. The thread surfaces, state moves and thread sessions this file also tested were
//! deleted with their writers (threads were retired on 2026-11-22, issue 44).

use std::sync::Arc;

use theseus_protocol::threads::Speaking;
use theseus_protocol::{DiscordOrigin, Id, Message, Request, TurnSubmitParams};

use super::tests::test_core;
use super::Core;
use crate::approval::{Client, Surface};
use crate::places::BoundPlace;

const OWNER: u64 = 271_828_182_845_904_523;
const DM_CHANNEL: u64 = 271_828_182_845_904_524;
const LAB: u64 = 314_159_265_358_979_323;
const SHARED: u64 = 314_159_265_358_979_999;

fn core() -> Arc<Core> {
    let c = test_core("the tide is out");
    // The owner is a person of Discord; a DM with them and the lab channel
    // are private, the other channel is shared.
    let mut cfg = (*c.cfg).clone();
    cfg.places.owner = Some(vec![format!("discord:{OWNER}")]);
    let model = Arc::new(crate::provider::FakeProvider {
        reply: "the tide is out".into(),
        ..Default::default()
    });
    let c = Core::build(super::Parts::for_tests(cfg, model, c.store.clone())).unwrap();
    c.bind_places(vec![
        BoundPlace {
            target: format!("discord:dm:{OWNER}"),
            name: "DM".into(),
            ..Default::default()
        },
        BoundPlace {
            target: format!("discord:channel:{LAB}"),
            name: "#lab".into(),
            private: true,
            guild: Some("7".into()),
            ..Default::default()
        },
        BoundPlace {
            target: format!("discord:channel:{SHARED}"),
            name: "#town".into(),
            private: false,
            guild: Some("7".into()),
            ..Default::default()
        },
    ]);
    c
}

/// One `turn.submit` to a session or a thread, over `client`.
async fn submit(
    core: Arc<Core>,
    (session, slug): (Option<&str>, Option<&str>),
    client: Client,
    speaking: Option<Speaking>,
) -> serde_json::Value {
    use tokio::io::{duplex, AsyncBufReadExt, AsyncWriteExt, BufReader};
    let (client_end, server) = duplex(64 * 1024);
    let (sr, sw) = tokio::io::split(server);
    let srv = tokio::spawn(core.serve_connection(sr, sw, client));
    let (cr, mut cw) = tokio::io::split(client_end);
    let req = Request::new(
        Id::Num(1),
        theseus_protocol::method::TURN_SUBMIT,
        TurnSubmitParams {
            dir: None,
            carried: false,
            prompt: None,
            session_id: session.map(str::to_string),
            input: "what does the gauge say".into(),
            profile: None,
            provider: None,
            model: None,
            author: Some("discord:owner".into()),
            attachments: vec![],
            reply_to: Some("4242".into()),
            opened_from: None,
            thread: slug.map(str::to_string),
            in_reply_to: None,
            route_at_ms: None,
            deadline_secs: None,
            speaking,
            via: None,
            surface: None,
        },
    );
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let mut lines = BufReader::new(cr).lines();
    let out = loop {
        let l = lines.next_line().await.unwrap().unwrap();
        if let Message::Response(r) = serde_json::from_str::<Message>(&l).unwrap() {
            break serde_json::to_value(r).unwrap();
        }
    };
    cw.shutdown().await.unwrap();
    drop(lines);
    let _ = srv.await;
    out
}

// ------------------------------------------- what a thread's place tells the model

/// A core whose model keeps its requests, with voice on and bound, and the
/// owner's DM and the lab channel private.
fn core_keeping_requests() -> (Arc<Core>, Arc<crate::provider::FakeProvider>) {
    let c = core();
    let mut cfg = (*c.cfg).clone();
    cfg.voice.enabled = true;
    // The turn's tail carries them (the date and git block's own switch).
    cfg.project.tail_facts = Some(true);
    let model = Arc::new(crate::provider::FakeProvider {
        reply: "the tide is out".into(),
        ..Default::default()
    });
    let c = Core::build(super::Parts::for_tests(cfg, model.clone(), c.store.clone())).unwrap();
    c.bind_places(vec![
        BoundPlace {
            target: format!("discord:dm:{OWNER}"),
            name: "DM".into(),
            ..Default::default()
        },
        BoundPlace {
            target: format!("discord:channel:{LAB}"),
            name: "#lab".into(),
            private: true,
            guild: Some("7".into()),
            ..Default::default()
        },
    ]);
    c.runner.place_rule.bind_voice(true);
    (c, model)
}

/// What the last request's tail tells the model about the places it speaks
/// in, and that its system blocks (the header every surface shares) say none
/// of it.
fn told(model: &crate::provider::FakeProvider) -> (bool, bool) {
    let req = model.requests().pop().expect("a request");
    let system = crate::tests_places::system_text(&req);
    assert!(
        !system.contains("slash commands") && !system.contains("/join"),
        "the shared header says it: {system}"
    );
    let tail = serde_json::to_string(&req.messages).unwrap();
    (tail.contains("slash commands"), tail.contains("/join"))
}

/// The control: the owner's DM itself, a turn of the place's own session. If
/// the live turn did not tell it either, the place rule was never the whole of
/// it.
#[tokio::test]
async fn a_turn_in_the_owners_dm_is_told_the_chat_route_and_the_voice_channel() {
    let (c, model) = core_keeping_requests();
    let rec = c.open_session(Default::default()).unwrap();
    c.outbox
        .bind_place(&format!("dm:{OWNER}"), &rec.session_id)
        .unwrap();
    let speaking = Speaking {
        discord: DiscordOrigin {
            user_id: OWNER.to_string(),
            channel_id: DM_CHANNEL.to_string(),
            guild_id: None,
        },
        answer_at: None,
    };
    let client = Client::new("discord", Surface::Discord);
    let r = submit(
        c.clone(),
        (Some(&rec.session_id), None),
        client,
        Some(speaking),
    )
    .await;
    assert!(r["error"].is_null(), "{r}");
    assert_eq!(told(&model), (true, true));
}

/// A plain session bound to `place` (`dm:<id>`, `channel:<id>`), as a binding's place opens one.
fn place_session(c: &Core, place: &str) -> String {
    let rec = c.open_session(Default::default()).unwrap();
    c.outbox.bind_place(place, &rec.session_id).unwrap();
    rec.session_id
}

/// The owner speaking over the Discord connection, from `channel` (a DM's or a guild channel's).
fn spoken_in(channel: u64, guild: Option<&str>) -> Speaking {
    Speaking {
        discord: DiscordOrigin {
            user_id: OWNER.to_string(),
            channel_id: channel.to_string(),
            guild_id: guild.map(str::to_string),
        },
        answer_at: None,
    }
}

/// The route hints by place, on plain sessions (the authority half of the thread case this file had, issue
/// 44): in the owner's DM and in a private channel of theirs the model is told it has a chat route and a
/// voice channel to join; at the CLI, which has neither, it is told neither; and the DM again tells both.
#[tokio::test]
async fn each_place_is_told_its_own_chat_route_and_voice_channel() {
    let (c, model) = core_keeping_requests();
    let dm = place_session(&c, &format!("dm:{OWNER}"));
    let lab = place_session(&c, &format!("channel:{LAB}"));
    let discord = || Client::new("discord", Surface::Discord);
    let r = submit(
        c.clone(),
        (Some(&dm), None),
        discord(),
        Some(spoken_in(DM_CHANNEL, None)),
    )
    .await;
    assert!(r["error"].is_null(), "{r}");
    assert_eq!(told(&model), (true, true), "the owner's DM");
    let r = submit(
        c.clone(),
        (Some(&lab), None),
        discord(),
        Some(spoken_in(LAB, Some("7"))),
    )
    .await;
    assert!(r["error"].is_null(), "{r}");
    assert_eq!(told(&model), (true, true), "a private channel of theirs");
    let r = submit(
        c.clone(),
        (None, None),
        Client::new("cli", Surface::Cli),
        None,
    )
    .await;
    assert!(r["error"].is_null(), "{r}");
    assert_eq!(told(&model), (false, false), "the CLI has neither");
    let r = submit(
        c.clone(),
        (Some(&dm), None),
        discord(),
        Some(spoken_in(DM_CHANNEL, None)),
    )
    .await;
    assert!(r["error"].is_null(), "{r}");
    assert_eq!(told(&model), (true, true), "and the DM again: back");
}

/// With no voice channel bound, a turn in the owner's DM is told of the chat route and not of `/join`.
#[tokio::test]
async fn a_turn_is_offered_join_only_where_voice_is_bound() {
    let (c, model) = core_keeping_requests();
    c.runner.place_rule.bind_voice(false);
    let dm = place_session(&c, &format!("dm:{OWNER}"));
    let client = Client::new("discord", Surface::Discord);
    let r = submit(
        c.clone(),
        (Some(&dm), None),
        client,
        Some(spoken_in(DM_CHANNEL, None)),
    )
    .await;
    assert!(r["error"].is_null(), "{r}");
    assert_eq!(told(&model), (true, false));
}

/// An old client's turn (issue 44): `{session_id, thread, speaking}` runs in the session it names, as the
/// owner who spoke there (the DM's route hints are the place's and the speaker's), and the thread is
/// ignored; with no `session_id`, the thread opens no thread's session, only a fresh ordinary one.
#[tokio::test]
async fn an_old_clients_thread_field_keeps_the_real_session_and_speaker() {
    let (c, model) = core_keeping_requests();
    let dm = place_session(&c, &format!("dm:{OWNER}"));
    let discord = || Client::new("discord", Surface::Discord);
    let speaking = Some(spoken_in(DM_CHANNEL, None));
    let r = submit(
        c.clone(),
        (Some(&dm), Some("tides")),
        discord(),
        speaking.clone(),
    )
    .await;
    assert!(r["error"].is_null(), "{r}");
    assert_eq!(r["result"]["session_id"], dm.as_str(), "{r}");
    assert_eq!(
        told(&model),
        (true, true),
        "the DM's own place, its speaker the owner"
    );

    let r = submit(c.clone(), (None, Some("tides")), discord(), speaking).await;
    assert!(r["error"].is_null(), "{r}");
    let fresh = r["result"]["session_id"].as_str().unwrap().to_string();
    assert_ne!(fresh, dm);
    let rec = c
        .store
        .get_session::<crate::session::SessionRecord>(&fresh)
        .unwrap()
        .unwrap();
    assert_eq!(rec.kind, theseus_protocol::SessionKind::Conversation);
    assert!(
        c.outbox.target(&fresh).is_none(),
        "bound to no place, a thread's or any other"
    );
}
