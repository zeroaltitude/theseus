//! A thread on Discord (theseus-sjzm), the core's half: the surfaces a thread
//! is shown on, `thread.changed` told to every connection, the first message
//! that opens a thread's session announced once, a place that runs as a
//! thread (private only), and a thread's reply posted where its speaker spoke
//! (their DM) and nowhere else.

use std::sync::Arc;

use theseus_protocol::threads::{
    channel_surface, surface_channel, Speaking, ThreadChanged, ThreadEditParams, ThreadNewParams,
};
use theseus_protocol::{error_code, DiscordOrigin, Id, Message, Request, TurnSubmitParams};

use super::tests::test_core;
use super::Core;
use crate::approval::{Answerer, Client, Surface};
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

fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    }
}

fn make(c: &Core, slug: &str) {
    c.thread_new(
        &ThreadNewParams {
            slug: slug.into(),
            title: Some(format!("Title of {slug}")),
            ..Default::default()
        },
        cli(),
    )
    .unwrap();
}

/// The `thread.changed` notifications the bus carried so far.
fn changes(rx: &mut tokio::sync::mpsc::UnboundedReceiver<Message>) -> Vec<ThreadChanged> {
    let mut out = Vec::new();
    while let Ok(m) = rx.try_recv() {
        if let Message::Notification(n) = m {
            if n.method == "thread.changed" {
                out.push(serde_json::from_value(n.params).unwrap());
            }
        }
    }
    out
}

fn tap(c: &Core) -> tokio::sync::mpsc::UnboundedReceiver<Message> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    *c.bus.tap.lock().unwrap() = Some(tx);
    rx
}

/// A thread's surfaces are edited whole by a person and one at a time by the
/// binding; a bad one is refused, and each change is told once.
#[test]
fn a_thread_is_shown_on_surfaces_and_each_change_is_told_once() {
    let c = core();
    make(&c, "tides");
    let mut rx = tap(&c);
    let surface = channel_surface(LAB);
    let edit = |s: Vec<String>| {
        c.thread_edit(
            &ThreadEditParams {
                slug: "tides".into(),
                surfaces: Some(s),
                ..Default::default()
            },
            cli(),
        )
    };
    let r = edit(vec![surface.clone(), surface.clone(), "cli".into()]).unwrap();
    assert_eq!(r.thread.surfaces, [surface.clone(), "cli".to_string()]);
    let told = changes(&mut rx);
    assert_eq!(told.len(), 1, "{told:?}");
    assert_eq!(
        (told[0].change.as_str(), told[0].slug.as_str()),
        ("edited", "tides")
    );
    assert_eq!(told[0].surfaces, r.thread.surfaces);
    // The same again is no change, and says nothing.
    edit(vec![surface.clone(), "cli".into()]).unwrap();
    assert!(changes(&mut rx).is_empty());
    // A surface that is neither `cli` nor a Discord channel is refused.
    let bad = edit(vec!["discord:dm:1".into()]).unwrap_err();
    assert_eq!(bad.code, error_code::INVALID_PARAMS);

    // The binding's own: it adds and removes one, idempotently, as a system act.
    let other = channel_surface(SHARED);
    let t = c
        .thread_surfaces("tides", Some(&other), Some("cli"), "discord")
        .unwrap();
    assert_eq!(t.surfaces, [surface, other.clone()]);
    assert_eq!(surface_channel(&other), Some(SHARED));
    assert_eq!(changes(&mut rx).len(), 1);
    let t = c
        .thread_surfaces("tides", Some(&other), None, "discord")
        .unwrap();
    assert_eq!(t.surfaces.len(), 2);
    assert!(
        changes(&mut rx).is_empty(),
        "adding what is there changes nothing"
    );
    assert!(c
        .thread_surfaces("nowhere", Some(&other), None, "discord")
        .is_err());
}

/// Every move of a thread is told with its state, and a finished or paused
/// one says so; sealed shows as a flag.
#[test]
fn each_move_is_told_with_the_state_it_ends_in() {
    let c = core();
    make(&c, "tides");
    let mut rx = tap(&c);
    c.thread_pause("tides", cli()).unwrap();
    c.thread_resume("tides", cli()).unwrap();
    c.thread_done("tides", cli()).unwrap();
    c.thread_reopen("tides", cli()).unwrap();
    let told: Vec<(String, String)> = changes(&mut rx)
        .into_iter()
        .map(|t| (t.change, t.state.as_str().to_string()))
        .collect();
    let want = [
        ("paused", "paused"),
        ("resumed", "active"),
        ("done", "done"),
        ("reopened", "active"),
    ];
    assert_eq!(
        told,
        want.map(|(a, b)| (a.to_string(), b.to_string())).to_vec()
    );
}

/// The first message to a thread opens its session and says so, once; a place
/// that runs as the thread is private or refused; a paused thread is refused.
#[test]
fn a_place_runs_as_a_thread_when_it_is_private_and_the_thread_active() {
    let c = core();
    make(&c, "tides");
    let mut rx = tap(&c);
    let lab = format!("discord:channel:{LAB}");
    let first = c.thread_place_session("tides", &lab).unwrap();
    let opened = changes(&mut rx);
    assert_eq!(opened.len(), 1, "{opened:?}");
    assert_eq!(opened[0].change, "opened");
    assert_eq!(
        c.threads().get("tides").unwrap().session_id.as_deref(),
        Some(first.session_id.as_str())
    );
    let again = c.thread_place_session("tides", &lab).unwrap();
    assert_eq!(again.session_id, first.session_id);
    assert!(
        changes(&mut rx).is_empty(),
        "the second message opens nothing"
    );
    // The session posts nowhere until the binding names its place.
    assert!(c.outbox.target(&first.session_id).is_none());

    let town = format!("discord:channel:{SHARED}");
    let e = c.thread_place_session("tides", &town).unwrap_err();
    assert_eq!(e.code, error_code::REFUSED);
    assert!(e.message.contains("private"), "{}", e.message);

    c.thread_pause("tides", cli()).unwrap();
    let e = c.thread_place_session("tides", &lab).unwrap_err();
    assert!(e.message.contains("thread resume"), "{}", e.message);
}

/// One `turn.submit` over a Discord connection, as the binding sends a
/// message the owner spoke in their DM to a thread.
async fn say_in_dm(core: Arc<Core>, slug: &str, answer_at: &str, user: u64) -> serde_json::Value {
    use tokio::io::{duplex, AsyncBufReadExt, AsyncWriteExt, BufReader};
    let (client, server) = duplex(64 * 1024);
    let (sr, sw) = tokio::io::split(server);
    let srv = tokio::spawn(core.serve_connection(sr, sw, Client::new("discord", Surface::Discord)));
    let (cr, mut cw) = tokio::io::split(client);
    let req = Request::new(
        Id::Num(1),
        theseus_protocol::method::TURN_SUBMIT,
        TurnSubmitParams {
            dir: None,
            carried: false,
            prompt: None,
            session_id: None,
            input: "what does the gauge say".into(),
            profile: None,
            provider: None,
            model: None,
            author: Some("discord:owner".into()),
            attachments: vec![],
            reply_to: Some("4242".into()),
            opened_from: None,
            thread: Some(slug.into()),
            in_reply_to: None,
            speaking: Some(Speaking {
                discord: DiscordOrigin {
                    user_id: user.to_string(),
                    channel_id: DM_CHANNEL.to_string(),
                    guild_id: None,
                },
                answer_at: Some(answer_at.into()),
            }),
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

/// A thread's turn spoken in the owner's DM is answered in that DM, and the
/// thread's own session has no place of its own that the reply goes to; a
/// stranger's, or a place that is not the speaker's own, is refused.
#[tokio::test]
async fn a_threads_reply_is_posted_where_the_owner_spoke() {
    let c = core();
    make(&c, "tides");
    let dm = format!("discord:dm:{OWNER}");
    let r = say_in_dm(c.clone(), "tides", &dm, OWNER).await;
    let sid = r["result"]["session_id"].as_str().unwrap().to_string();
    assert_eq!(
        c.threads().of_session(&sid).map(|t| t.slug.as_str()),
        Some("tides")
    );
    assert!(c.outbox.target(&sid).is_none());
    let posts = c.outbox.open_for(&dm);
    let replies: Vec<_> = posts
        .iter()
        .filter(|a| crate::outbox::kind_of(a) == "reply")
        .collect();
    assert_eq!(replies.len(), 1, "one reply, in the DM");
    let body = crate::outbox::body_of(replies[0]);
    assert_eq!(body["reply_to"], "4242");
    assert_eq!(body["result"]["session_id"], sid.as_str());

    // Another place than the speaker's own is refused, and so is a stranger.
    let lab = format!("discord:channel:{LAB}");
    let r = say_in_dm(c.clone(), "tides", &lab, OWNER).await;
    assert_eq!(r["error"]["code"], error_code::INVALID_PARAMS, "{r}");
    let r = say_in_dm(c.clone(), "tides", "discord:dm:555", 555).await;
    assert_eq!(r["error"]["code"], error_code::REFUSED, "{r}");
    assert_eq!(
        c.outbox.open_for(&dm).len(),
        posts.len(),
        "nothing more posted"
    );
}
