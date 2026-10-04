//! Durable delivery end to end, in process (theseus-q4v): a real core over a
//! scratch store, the binding, and a stand-in for Discord's REST API
//! (`theseus_sim::fake_discord`), with the gateway pointed at a port nothing
//! listens on. Turns come in over a protocol connection of their own, as a
//! CLI's would: the gateway, and so a Discord message, cannot be faked here.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use theseus_core::approval::{Client, Surface};
use theseus_core::policy::Posture;
use theseus_core::provider::{FakeProvider, Provider, Scripted};
use theseus_core::secrets::{Secret, SecretBoard};
use theseus_core::{Config, Core};
use theseus_protocol::{TurnSubmitParams, TurnSubmitResult};
use theseus_sim::fake_discord::{FakeDiscord, Mode, Msg};

use crate::rpc_client::RpcClient;

const USER: u64 = 271_828_182_845_904_523;
/// The fake's DM channel with `USER`.
const DM: u64 = USER + 1;
const CHANNEL: u64 = 900_000_000_000_000_001;
const GUILD: &str = "314159265358979323";

fn dm_only() -> String {
    format!("guild_id = \"{GUILD}\"\n[[dm]]\nuser = \"{USER}\"\nname = \"eddie\"\n")
}

/// A core over `dir`, whose binding talks to `fake` and never to Discord.
fn core_at(
    dir: &Path,
    fake: &FakeDiscord,
    script: Vec<Scripted>,
    tweak: impl FnOnce(&mut Config),
) -> Arc<Core> {
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.to_string_lossy().into_owned();
    let work = dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    cfg.tools.projects_dir = Some(work.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = Posture::Approve;
    cfg.discord.rest_proxy = Some(fake.addr.clone());
    cfg.discord.gateway_proxy = Some("ws://127.0.0.1:9".into());
    cfg.discord.edit_interval_ms = 250;
    tweak(&mut cfg);
    let name = cfg.discord.token_secret.clone();
    let secrets = SecretBoard::new([name.clone()], Instant::now());
    secrets.publish(
        [(name, Ok(Secret::new("fake-token-not-a-secret".into())))].into(),
        "test",
    );
    let store = theseus_core::store::Store::open(&dir.join("store")).unwrap();
    let model: Arc<dyn Provider> = Arc::new(FakeProvider::scripted(script));
    let providers = [(cfg.model.provider.clone(), model)].into_iter().collect();
    Core::build(theseus_core::rpc::Parts {
        cfg,
        providers,
        store,
        secrets,
        startup_log: Arc::default(),
        telemetry: Some(theseus_core::telemetry::Telemetry::disabled()),
        scrubber: Arc::new(theseus_core::scrub::Scrubber::default()),
        launcher: Arc::new(theseus_core::toolrun::InlineLauncher),
        config_gate: theseus_core::config_gate::ConfigGate::file("test"),
        toollets: vec![],
        cpu_cores: None,
    })
    .unwrap()
}

/// Start the binding on `core` with `bindings`, and wait for its places to
/// be bound. Returns a protocol connection of the test's own.
async fn bind(core: &Arc<Core>, dir: &Path, bindings: &str) -> Arc<RpcClient> {
    let path = dir.join("bindings.toml");
    std::fs::write(&path, bindings).unwrap();
    tokio::spawn(crate::run(core.clone(), core.cfg.discord.clone(), path));
    let t0 = Instant::now();
    while core
        .outbox
        .place_session(&format!("dm:{USER}"))
        .unwrap()
        .is_none()
    {
        assert!(
            t0.elapsed() < Duration::from_secs(10),
            "the DM place was never bound: {:?}",
            core.bindings.all()
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    RpcClient::connect(core.clone(), Client::new("test", Surface::Cli)).0
}

async fn until(what: &str, secs: u64, f: impl Fn() -> bool) {
    let t0 = Instant::now();
    while !f() {
        assert!(
            t0.elapsed() < Duration::from_secs(secs),
            "timed out: {what}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn session(core: &Core) -> String {
    core.outbox
        .place_session(&format!("dm:{USER}"))
        .unwrap()
        .unwrap()
}

async fn ask(rpc: &RpcClient, sid: &str, input: &str) -> TurnSubmitResult {
    rpc.call(
        theseus_protocol::method::TURN_SUBMIT,
        TurnSubmitParams {
            prompt: None,
            session_id: Some(sid.into()),
            input: input.into(),
            profile: None,
            provider: None,
            model: None,
            author: Some("test".into()),
            attachments: vec![],
            reply_to: None,
            opened_from: None,
        },
    )
    .await
    .unwrap()
}

fn pending(core: &Core) -> u64 {
    core.outbox.status("discord").pending
}

/// The DM's messages that are not the bind notice.
fn replies(fake: &FakeDiscord) -> Vec<Msg> {
    fake.messages(DM)
        .into_iter()
        .filter(|m| !m.content.starts_with("🔗 Theseus is bound here"))
        .collect()
}

/// Discord away during a turn: its reply waits in the outbox, and is posted
/// once, whole, when Discord is back; the stream's live edits were dropped,
/// never replayed.
#[tokio::test]
async fn a_reply_that_ends_while_discord_is_away_is_posted_once_when_it_is_back() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let core = core_at(
        d.path(),
        &fake,
        vec![Scripted::text("The answer is 42.")],
        |_| {},
    );
    let rpc = bind(&core, d.path(), &dm_only()).await;
    let f = fake.clone();
    until("the bind notice", 10, move || f.messages(DM).len() == 1).await;
    fake.set_mode(Mode::Down);
    let r = ask(&rpc, &session(&core), "what is the answer?").await;
    assert_eq!(r.output, "The answer is 42.");
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert!(
        replies(&fake).is_empty(),
        "nothing reached Discord while it was away"
    );
    assert_eq!(pending(&core), 1, "the reply waits in the outbox");
    fake.set_mode(Mode::Up);
    let c = core.clone();
    until("the reply is delivered", 20, move || pending(&c) == 0).await;
    let got = replies(&fake);
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(
        got[0].content.starts_with("The answer is 42.\n-# "),
        "{}",
        got[0].content
    );
    // One create for it, and its nonce is its key's.
    let key = format!("{}:L0:p0", r.turn_id);
    let creates: Vec<_> = fake
        .seen()
        .into_iter()
        .filter(|s| {
            s.outcome == "created" && s.nonce.as_deref() == Some(crate::nonce(&key).as_str())
        })
        .collect();
    assert_eq!(creates.len(), 1, "{creates:?}");
    let st = core.outbox.status("discord");
    assert!(st.sent >= 2 && st.last_error.is_some(), "{st:?}");
}

/// Three replies written while Discord is away go out in the order they
/// were written.
#[tokio::test]
async fn replies_queued_while_away_are_posted_in_order() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let script = vec![
        Scripted::text("one"),
        Scripted::text("two"),
        Scripted::text("three"),
    ];
    let core = core_at(d.path(), &fake, script, |_| {});
    let rpc = bind(&core, d.path(), &dm_only()).await;
    let f = fake.clone();
    until("the bind notice", 10, move || f.messages(DM).len() == 1).await;
    fake.set_mode(Mode::Down);
    let sid = session(&core);
    for q in ["first?", "second?", "third?"] {
        ask(&rpc, &sid, q).await;
    }
    assert_eq!(pending(&core), 3);
    fake.set_mode(Mode::Up);
    let c = core.clone();
    until("three replies delivered", 20, move || pending(&c) == 0).await;
    let got: Vec<String> = replies(&fake)
        .iter()
        .map(|m| m.content.lines().next().unwrap_or("").to_string())
        .collect();
    assert_eq!(got, ["one", "two", "three"]);
}

/// A crash between the send and the settle: the create reached Discord and
/// its answer was lost with the process. After a restart the post goes again
/// with the same nonce, and Discord (the fake enforcing it) returns the first
/// message: the channel holds one.
#[test]
fn a_crash_between_send_and_settle_leaves_one_message() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    // The first life: the reply's create lands and hangs; the process dies.
    let first = {
        let (dir, fake) = (d.path().to_path_buf(), fake.clone());
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            let turn_id = rt.block_on(async {
                let core = core_at(&dir, &fake, vec![Scripted::text("Done, once.")], |_| {});
                let rpc = bind(&core, &dir, &dm_only()).await;
                let f = fake.clone();
                until("the bind notice", 10, move || f.messages(DM).len() == 1).await;
                fake.set_mode(Mode::HangCreates);
                let r = ask(&rpc, &session(&core), "go").await;
                let f = fake.clone();
                until("the create reached Discord", 10, move || {
                    f.seen().iter().any(|s| s.outcome == "hung")
                })
                .await;
                r.turn_id
            });
            // Every task, and with them the core and its store, dies here.
            rt.shutdown_timeout(Duration::from_secs(2));
            turn_id
        })
        .join()
        .unwrap()
    };
    fake.set_mode(Mode::Up);
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let core = core_at(d.path(), &fake, vec![], |_| {});
        // As the daemon does once it serves.
        core.outbox.warm();
        assert_eq!(pending(&core), 1, "the post is still dispatched");
        let _rpc = bind(&core, d.path(), &dm_only()).await;
        let c = core.clone();
        until("the retry settles", 20, move || pending(&c) == 0).await;
    });
    let got = replies(&fake);
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(
        got[0].content.starts_with("Done, once."),
        "{}",
        got[0].content
    );
    let nonce = crate::nonce(&format!("{first}:L0:p0"));
    let tries: Vec<String> = fake
        .seen()
        .into_iter()
        .filter(|s| s.nonce.as_deref() == Some(nonce.as_str()))
        .map(|s| s.outcome)
        .collect();
    assert_eq!(tries, ["hung", "deduped"]);
}

/// The ledger's `action.failed` rows for posts, by post.
fn refused_rows(core: &Core) -> Vec<serde_json::Value> {
    let rows: Vec<(u64, theseus_core::ledger::LedgerRow)> = core.store.ledger_tail(500).unwrap();
    rows.into_iter()
        .filter(|(_, r)| r.kind == "action.failed" && r.data["outbox"].is_string())
        .map(|(_, r)| r.data)
        .collect()
}

/// theseus-l3m: a place taken out of the bindings file while a post for it
/// waits. At the next start the post is settled as refused, with the
/// reason, ledgered and counted in health's refused, no longer pending, and
/// nothing reaches the place. A post written for it later is refused too.
#[test]
fn a_post_for_a_place_no_longer_bound_is_refused_at_the_next_start() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let place = format!("channel:{CHANNEL}");
    let with_channel = format!(
        "{}[[channel]]\nid = \"{CHANNEL}\"\nname = \"harbor\"\nusers = [\"{USER}\"]\nmention_only = false\n",
        dm_only()
    );
    // The first life: the channel is bound, and its reply waits while
    // Discord is away.
    let sid = {
        let (dir, fake, place) = (d.path().to_path_buf(), fake.clone(), place.clone());
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            let sid = rt.block_on(async {
                let script = vec![Scripted::text("The tide turns at six.")];
                let core = core_at(&dir, &fake, script, |_| {});
                let rpc = bind(&core, &dir, &with_channel).await;
                let (c, p) = (core.clone(), place.clone());
                until("the channel is bound", 10, move || {
                    c.outbox.place_session(&p).unwrap().is_some()
                })
                .await;
                let f = fake.clone();
                until("both bind notices", 10, move || {
                    f.messages(DM).len() == 1 && f.messages(CHANNEL).len() == 1
                })
                .await;
                fake.set_mode(Mode::Down);
                let sid = core.outbox.place_session(&place).unwrap().unwrap();
                ask(&rpc, &sid, "when does the tide turn?").await;
                assert_eq!(pending(&core), 1, "the reply waits");
                sid
            });
            rt.shutdown_timeout(Duration::from_secs(2));
            sid
        })
        .join()
        .unwrap()
    };
    // The second life: the channel is gone from the bindings file.
    fake.set_mode(Mode::Up);
    let before = fake.seen().len();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let core = core_at(d.path(), &fake, vec![Scripted::text("Still six.")], |_| {});
        core.outbox.warm();
        assert_eq!(pending(&core), 1, "still waiting before the binding starts");
        let rpc = bind(&core, d.path(), &dm_only()).await;
        let c = core.clone();
        until("the post is refused", 10, move || pending(&c) == 0).await;
        let why = format!("not bound here any more ({place})");
        let st = core.outbox.status("discord");
        assert_eq!(
            (st.failed, st.oldest_pending_ms, st.last_error.as_deref()),
            (1, 0, Some(why.as_str())),
            "{st:?}"
        );
        let rows = refused_rows(&core);
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0]["outbox"], format!("discord:{place}"));
        assert_eq!(rows[0]["detail"]["error"], why.as_str());
        // A post written for that place later is refused at once.
        ask(&rpc, &sid, "and tomorrow?").await;
        let c = core.clone();
        until("the later post is refused", 10, move || {
            c.outbox.status("discord").failed == 2
        })
        .await;
        assert_eq!(pending(&core), 0);
    });
    // Nothing went to the channel after the restart: its one message is the
    // first life's bind notice.
    assert_eq!(fake.messages(CHANNEL).len(), 1);
    let to_channel: Vec<_> = fake.seen()[before..]
        .iter()
        .filter(|s| s.path.contains(&CHANNEL.to_string()))
        .map(|s| (s.method.clone(), s.path.clone()))
        .collect();
    assert!(to_channel.is_empty(), "{to_channel:?}");
}

/// A card and how its question closed, both written while Discord is away:
/// when it is back, the card is posted, then edited to say how it closed,
/// at the id its create returned, its buttons gone.
#[tokio::test]
async fn a_cards_settle_waits_for_its_create_and_edits_it_by_id() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let script = vec![Scripted::tools(
        "",
        &[(
            "t1",
            "fs_write",
            serde_json::json!({"path": "a.txt", "content": "x"}),
        )],
    )];
    let core = core_at(d.path(), &fake, script, |_| {});
    let rpc = bind(&core, d.path(), &dm_only()).await;
    let f = fake.clone();
    until("the bind notice", 10, move || f.messages(DM).len() == 1).await;
    fake.set_mode(Mode::Down);
    let r = ask(&rpc, &session(&core), "write a").await;
    let q = r.awaiting_confirm.clone().expect("the write waits");
    // Answered from the CLI while Discord is away.
    let cli = theseus_core::approval::Answerer {
        label: "cli#1".into(),
        surface: Surface::Cli,
        discord: None,
    };
    core.confirm_action(&q, false, Some("not now"), cli)
        .unwrap();
    // The reply (its footer), the card, and the card's settle wait.
    assert_eq!(pending(&core), 3);
    fake.set_mode(Mode::Up);
    let c = core.clone();
    until("all three delivered", 20, move || pending(&c) == 0).await;
    let card = fake
        .messages(DM)
        .into_iter()
        .find(|m| m.content.contains("fs.write"))
        .expect("the card");
    assert!(
        card.content
            .starts_with("❎ **Declined** by cli#1 · `fs.write` a.txt"),
        "{}",
        card.content
    );
    assert_eq!((card.edits, card.components), (1, 0), "{card:?}");
    let seen = fake.seen();
    let create = seen
        .iter()
        .position(|s| s.outcome == "created" && s.message_id.as_deref() == Some(card.id.as_str()))
        .unwrap();
    let edit = seen
        .iter()
        .position(|s| s.outcome == "edited" && s.message_id.as_deref() == Some(card.id.as_str()))
        .unwrap();
    assert!(create < edit, "the edit went after its create, to its id");
}

/// A call that asks, as a shared place may make one: a wake (public by
/// nature), set to ask first.
fn wake_call(text: &str) -> Scripted {
    Scripted::tools(
        text,
        &[(
            "t1",
            "wake_at",
            serde_json::json!({"after": "10m", "note": "check the build"}),
        )],
    )
}

/// How a card names `wake_call`'s call.
const WAKE: &str = "`wake.at` {\"after\":\"10m\",\"note\":\"check the build\"}";

/// 2b's approval route through the lanes, under the place rule
/// (theseus-zmgb): a card for a shared channel goes to the owner's DM, with
/// a note in the channel; how its question closed edits both, the card's
/// buttons gone.
#[tokio::test]
async fn a_card_for_a_shared_channel_goes_to_the_dm_and_its_settle_edits_both() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let core = core_at(d.path(), &fake, vec![wake_call("")], |c| {
        c.policy.tools.insert("wake.at".into(), Posture::Approve);
    });
    let bindings = format!(
        "{}[[channel]]\nid = \"{CHANNEL}\"\nname = \"general\"\nusers = [\"{USER}\"]\nmention_only = false\n",
        dm_only()
    );
    let rpc = bind(&core, d.path(), &bindings).await;
    let c = core.clone();
    until("the channel is bound", 10, move || {
        c.outbox
            .place_session(&format!("channel:{CHANNEL}"))
            .unwrap()
            .is_some()
    })
    .await;
    let sid = core
        .outbox
        .place_session(&format!("channel:{CHANNEL}"))
        .unwrap()
        .unwrap();
    let q = ask(&rpc, &sid, "remind me")
        .await
        .awaiting_confirm
        .expect("the wake waits");
    let c = core.clone();
    until("the card and the reply delivered", 10, move || {
        pending(&c) == 0
    })
    .await;
    let card = fake
        .messages(DM)
        .into_iter()
        .find(|m| m.content.starts_with(&format!("**Approve?** {WAKE}")))
        .expect("the card in the DM");
    assert!(
        card.content.contains("-# for #general · expires"),
        "{}",
        card.content
    );
    assert_eq!(card.components, 1, "its buttons");
    let note = fake
        .messages(CHANNEL)
        .into_iter()
        .find(|m| m.content.starts_with("🔐"))
        .expect("the note in the channel");
    assert_eq!(
        note.content,
        format!(
            "🔐 Approval for {WAKE} was asked in DM @eddie: this channel is shared, and an \
             answer counts only from a private place."
        )
    );
    // Closed with no event that says so; the reconcile finds it.
    core.kernel
        .decline_action(&q, "operator", "closed elsewhere")
        .unwrap();
    assert_eq!(core.outbox.reconcile_cards().unwrap(), 1);
    let c = core.clone();
    until("the settle delivered", 10, move || pending(&c) == 0).await;
    let card = fake
        .messages(DM)
        .into_iter()
        .find(|m| m.id == card.id)
        .unwrap();
    assert_eq!(
        card.content,
        format!("❎ **Declined** by operator · {WAKE}")
    );
    assert_eq!(card.components, 0, "its buttons are gone");
    let note = fake
        .messages(CHANNEL)
        .into_iter()
        .find(|m| m.id == note.id)
        .unwrap();
    assert_eq!(
        note.content,
        format!("🔐 ❎ **Declined** by operator · {WAKE} (in DM @eddie)")
    );
}

/// theseus-94a6 through the binding: a fetch of a private address, asked in a
/// shared channel, posts a card whose reason says that the page would join a
/// conversation others can read. The card goes to the owner's DM, as any
/// card for a shared place does (theseus-zmgb). Nothing connects: the
/// question is never answered.
#[tokio::test]
async fn a_private_address_card_from_a_shared_channel_says_where_the_page_goes() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let url = "http://127.0.0.1:7455/notes";
    let script = vec![Scripted::tools(
        "",
        &[("t1", "http_fetch", serde_json::json!({ "url": url }))],
    )];
    let core = core_at(d.path(), &fake, script, |_| {});
    let bindings = format!(
        "{}[[channel]]\nid = \"{CHANNEL}\"\nname = \"general\"\nusers = [\"{USER}\"]\nmention_only = false\nprivate = false\n",
        dm_only()
    );
    let rpc = bind(&core, d.path(), &bindings).await;
    let c = core.clone();
    until("the channel is bound", 10, move || {
        c.outbox
            .place_session(&format!("channel:{CHANNEL}"))
            .unwrap()
            .is_some()
    })
    .await;
    let sid = core
        .outbox
        .place_session(&format!("channel:{CHANNEL}"))
        .unwrap()
        .unwrap();
    ask(&rpc, &sid, "fetch the notes")
        .await
        .awaiting_confirm
        .expect("the fetch waits");
    let c = core.clone();
    until("the card delivered", 10, move || pending(&c) == 0).await;
    let card = fake
        .messages(DM)
        .into_iter()
        .find(|m| m.content.starts_with("**Approve?** `http.fetch`"))
        .expect("the card in the DM");
    assert!(
        card.content.contains(&format!(
            "fetch {url}: http.fetch — approve (127.0.0.1 is a loopback address, and a private \
             address waits for approval; this is a shared place, so the page joins a \
             conversation others can read)"
        )),
        "{}",
        card.content
    );
}

/// Pending live edits of one message collapse into the last: a burst of
/// updates to a slow Discord costs a handful of edits, not one per update.
#[tokio::test]
async fn live_edits_of_one_message_coalesce_into_the_last() {
    use crate::courier::{Lane, LaneMsg};
    use crate::render::{Buttons, Op};
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let core = core_at(d.path(), &fake, vec![], |_| {});
    let _rpc = bind(&core, d.path(), &dm_only()).await;
    let shared = crate::runtime::shared_for_tests(&core);
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let lane = Lane::new(
        shared,
        "discord:test".into(),
        "channel",
        "#test".into(),
        Some(CHANNEL),
        None,
    );
    tokio::spawn(lane.run(rx));
    fake.set_delay_ms(150);
    for i in 0..60 {
        let _ = tx.send(LaneMsg::Live(Op::Upsert {
            key: "t1:L0:p0".into(),
            content: format!("update {i}"),
            buttons: Buttons::Keep,
        }));
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let f = fake.clone();
    until("the last state arrives", 10, move || {
        f.messages(CHANNEL)
            .first()
            .is_some_and(|m| m.content == "update 59")
    })
    .await;
    let m = &fake.messages(CHANNEL)[0];
    assert!(m.edits < 10, "60 updates cost {} edits", m.edits);
    assert_eq!(fake.messages(CHANNEL).len(), 1);
}

/// A second user the channel below is bound to (an invented id).
const OTHER: u64 = 300_000_000_000_000_003;

/// The ledger's `discord.message.out` rows: what Discord said each create
/// mentioned, by its message id.
fn mentioned_out(core: &Core) -> Vec<(String, serde_json::Value)> {
    let rows: Vec<(u64, theseus_core::ledger::LedgerRow)> = core.store.ledger_tail(500).unwrap();
    rows.into_iter()
        .filter(|(_, r)| r.kind == "discord.message.out")
        .map(|(_, r)| {
            (
                r.data["message_id"].as_str().unwrap_or("").to_string(),
                r.data["mentions"].clone(),
            )
        })
        .collect()
}

/// theseus-9j9, under the place rule (theseus-zmgb): a shared channel
/// answers nothing, so its card goes to the owner's DM, which needs no
/// mention, and the channel gets the note. No message mentions anyone: the
/// bind notice, the turn's tool line, its reply (whose text names a user),
/// the note, or the card. (A card that stays in a private channel names its
/// answerers: `courier::answerers` and `mentioning`.)
#[tokio::test]
async fn a_shared_channels_card_goes_to_the_dm_and_nothing_mentions_anyone() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let script = vec![wake_call(&format!("Asking <@{USER}> about it."))];
    let core = core_at(d.path(), &fake, script, |c| {
        c.policy.tools.insert("wake.at".into(), Posture::Approve);
    });
    let bindings = format!(
        "{}[[channel]]\nid = \"{CHANNEL}\"\nname = \"lighthouse\"\nusers = [\"{USER}\", \"{OTHER}\"]\nmention_only = false\n",
        dm_only()
    );
    let rpc = bind(&core, d.path(), &bindings).await;
    let c = core.clone();
    until("the channel is bound", 10, move || {
        c.outbox
            .place_session(&format!("channel:{CHANNEL}"))
            .unwrap()
            .is_some()
    })
    .await;
    let sid = core
        .outbox
        .place_session(&format!("channel:{CHANNEL}"))
        .unwrap()
        .unwrap();
    ask(&rpc, &sid, "remind me")
        .await
        .awaiting_confirm
        .expect("the wake waits");
    // Live progress can come after the outbox first drains, so wait for every
    // message the test reads, not only for an empty outbox.
    let c = core.clone();
    let expected = ["🔗 Theseus is bound here", "Asking <@", "`wake.at`", "🔐"];
    until(
        "the card, the tool line, the reply, and the note delivered",
        10,
        || {
            let msgs = fake.messages(CHANNEL);
            pending(&c) == 0
                && fake.messages(DM).iter().any(|m| m.components > 0)
                && expected.iter().all(|w| {
                    msgs.iter()
                        .any(|m| m.components == 0 && m.content.contains(w))
                })
        },
    )
    .await;
    let msgs = fake.messages(CHANNEL);
    assert!(
        msgs.iter().all(|m| m.components == 0),
        "no card in the channel: {msgs:#?}"
    );
    // The channel reads the turn in its order (theseus-50p): its text, the
    // call's tool line, then the note where the card would be.
    let at = |what: &str, is: &dyn Fn(&Msg) -> bool| {
        msgs.iter()
            .position(is)
            .unwrap_or_else(|| panic!("no {what}: {msgs:#?}"))
    };
    let text = at("text", &|m| m.content.contains("Asking <@"));
    let line = at("tool line", &|m| m.content.contains("`wake.at`"));
    let note_at = at("note", &|m| m.content.starts_with("🔐"));
    assert!(
        text < line && line < note_at,
        "text {text}, tool line {line}, note {note_at}: {msgs:#?}"
    );
    let note = &msgs[note_at];
    assert_eq!(
        note.content,
        format!(
            "🔐 Approval for {WAKE} was asked in DM @eddie: this channel is shared, and an \
             answer counts only from a private place."
        )
    );
    let card = fake
        .messages(DM)
        .into_iter()
        .find(|m| m.components > 0)
        .expect("the card in the DM");
    assert!(
        card.content.starts_with(&format!("**Approve?** {WAKE}")),
        "{}",
        card.content
    );
    let out = mentioned_out(&core);
    let others: Vec<Msg> = msgs.into_iter().chain([card]).collect();
    for what in expected {
        assert!(
            others.iter().any(|m| m.content.contains(what)),
            "no message with {what:?}: {others:?}"
        );
    }
    for m in &others {
        assert!(m.mentions.is_empty(), "{m:?}");
        assert_eq!(m.allowed_mentions["parse"], serde_json::json!([]), "{m:?}");
        assert!(m.allowed_mentions.get("users").is_none(), "{m:?}");
        let row = out.iter().find(|(id, _)| *id == m.id).expect("its row");
        assert_eq!(row.1, serde_json::json!([]), "{m:?}");
    }
}

/// theseus-9j9: a card in a DM reaches its one user anyway, and names no one.
#[tokio::test]
async fn a_card_in_a_dm_mentions_no_one() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let script = vec![Scripted::tools(
        "",
        &[(
            "t1",
            "fs_write",
            serde_json::json!({"path": "a.txt", "content": "x"}),
        )],
    )];
    let core = core_at(d.path(), &fake, script, |_| {});
    let rpc = bind(&core, d.path(), &dm_only()).await;
    ask(&rpc, &session(&core), "write a")
        .await
        .awaiting_confirm
        .expect("the write waits");
    let c = core.clone();
    until("the card delivered", 10, move || pending(&c) == 0).await;
    let card = fake
        .messages(DM)
        .into_iter()
        .find(|m| m.components > 0)
        .expect("the card");
    assert!(
        card.content.starts_with("**Approve?** `fs.write` a.txt"),
        "{}",
        card.content
    );
    assert!(card.mentions.is_empty(), "{card:?}");
    assert!(card.allowed_mentions.get("users").is_none(), "{card:?}");
}

/// theseus-j7xi: `events.lost` and `execution.changed` reach the binding over
/// its own connection, as the session's bus delivers them to every watcher,
/// and the binding posts nothing for them: no message, edit, or typing at
/// Discord. Its outbox posts still go: the next turn's reply is delivered.
#[tokio::test]
async fn events_lost_and_execution_changed_post_nothing_and_replies_still_go() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let core = core_at(
        d.path(),
        &fake,
        vec![Scripted::text("First."), Scripted::text("Second.")],
        |_| {},
    );
    let rpc = bind(&core, d.path(), &dm_only()).await;
    let sid = session(&core);
    ask(&rpc, &sid, "one").await;
    until("the first reply", 10, || {
        replies(&fake)
            .iter()
            .any(|m| m.content.starts_with("First."))
    })
    .await;
    until("the outbox to drain", 10, || pending(&core) == 0).await;
    // Quiet: no request at Discord for three edit intervals.
    let mut seen = fake.seen().len();
    let mut quiet_since = Instant::now();
    while quiet_since.elapsed() < Duration::from_millis(750) {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let now = fake.seen().len();
        if now != seen {
            (seen, quiet_since) = (now, Instant::now());
        }
    }
    let view: theseus_protocol::ExecutionView = serde_json::from_value(serde_json::json!({
        "position": 1_000_000, "at_ms": 1, "execution_id": "exec_j7xi", "session_id": sid,
        "kind": "conversation", "state": "waiting", "previous": "running",
        "attention": {"level": "ready", "label": "ready", "since_ms": 1}
    }))
    .unwrap();
    let sink = theseus_core::bus::EventSink::new(core.bus.clone(), &sid, None);
    sink.send(theseus_protocol::Event::EventsLost(
        theseus_protocol::EventsLost {
            dropped: 865,
            streams: vec!["executions".into(), format!("session:{sid}")],
        },
    ));
    sink.send(theseus_protocol::Event::ExecutionChanged(view));
    tokio::time::sleep(Duration::from_millis(1_000)).await;
    let after = fake.seen();
    assert_eq!(
        after.len(),
        seen,
        "the binding posted for them: {:?}",
        &after[seen.min(after.len())..]
    );
    // Its posts still go.
    ask(&rpc, &sid, "two").await;
    until("the second reply", 10, || {
        replies(&fake)
            .iter()
            .any(|m| m.content.starts_with("Second."))
    })
    .await;
}

/// theseus-c3e: with no DM taking approvals, an operator's notice falls back
/// to the place of the session it concerns only when this daemon binds that
/// place. Sessions whose places the bindings file does not name (as a store
/// copied from another daemon's has, or a place since removed) get nothing
/// there: no DM is opened with the user, nothing is posted to the channel,
/// and each notice is refused with the reason. A bound place still gets its
/// notice. Before, the first opened a DM with the stranger and the second
/// posted to the unbound channel.
#[tokio::test]
async fn an_operators_notice_falls_back_only_to_a_place_this_daemon_binds() {
    const STRANGER: u64 = 500_000_000_000_000_505;
    const ELSEWHERE: u64 = 900_000_000_000_000_009;
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let core = core_at(d.path(), &fake, vec![], |_| {});
    // What a copied store names: a DM and a channel this daemon does not bind.
    let placed = |place: &str| {
        let r = theseus_core::session::SessionRecord::new(
            theseus_protocol::SessionKind::Conversation,
            None,
        );
        core.store.put_session(&r.session_id, &r).unwrap();
        core.outbox.bind_place(place, &r.session_id).unwrap();
        r.session_id
    };
    let stranger = placed(&format!("dm:{STRANGER}"));
    let elsewhere = placed(&format!("channel:{ELSEWHERE}"));
    let path = d.path().join("bindings.toml");
    std::fs::write(
        &path,
        format!(
            "guild_id = \"{GUILD}\"\n[[channel]]\nid = \"{CHANNEL}\"\nname = \"harbor\"\n\
             users = [\"{USER}\"]\nmention_only = false\n"
        ),
    )
    .unwrap();
    tokio::spawn(crate::run(core.clone(), core.cfg.discord.clone(), path));
    let f = fake.clone();
    until("the channel's bind notice", 10, move || {
        f.messages(CHANNEL).len() == 1
    })
    .await;
    let before = fake.seen().len();
    let notice = |sid: &str| {
        core.outbox
            .to_operator(
                Some(sid),
                serde_json::json!({"kind": "restarted", "at_unix_ms": 1, "tables": ["model"]}),
            )
            .unwrap()
    };
    notice(&stranger);
    notice(&elsewhere);
    let c = core.clone();
    until("both notices are refused", 10, move || {
        c.outbox.status("discord").failed == 2 && pending(&c) == 0
    })
    .await;
    let mut why: Vec<String> = refused_rows(&core)
        .iter()
        .map(|r| r["detail"]["error"].as_str().unwrap_or("").to_string())
        .collect();
    why.sort();
    assert_eq!(
        why,
        [
            format!(
                "no DM takes approvals, and the notice's place, discord:channel:{ELSEWHERE}, \
                 is not one this daemon's bindings file names"
            ),
            format!(
                "no DM takes approvals, and the notice's place, discord:dm:{STRANGER}, is not \
                 one this daemon's bindings file names"
            ),
        ]
    );
    let reached: Vec<(String, String)> = fake.seen()[before..]
        .iter()
        .map(|s| (s.method.clone(), s.path.clone()))
        .collect();
    assert!(reached.is_empty(), "{reached:?}");
    assert!(fake.messages(ELSEWHERE).is_empty());
    // The bound channel's own session still gets its notice there.
    let bound = core
        .outbox
        .place_session(&format!("channel:{CHANNEL}"))
        .unwrap()
        .unwrap();
    notice(&bound);
    let f = fake.clone();
    until("the bound channel's notice", 10, move || {
        f.messages(CHANNEL).len() == 2
    })
    .await;
}

/// A hands group's one line (step 40 part 2): each `hands` post of a group
/// is under its group's key, so its later states edit the one message,
/// never a line per hand; another group gets a message of its own.
#[tokio::test]
async fn a_hands_groups_line_is_one_message_edited_in_place() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let core = core_at(d.path(), &fake, vec![], |_| {});
    let _rpc = bind(&core, d.path(), &dm_only()).await;
    let sid = session(&core);
    let target = format!("discord:dm:{USER}");
    let post = |group: &str, text: &str| {
        core.outbox
            .post(
                &sid,
                "",
                &target,
                serde_json::json!({"kind": "hands", "group": group, "text": text}),
            )
            .unwrap();
    };
    post(
        "act_example_a",
        "🖐️ 0/3 done, 0 failed, 3 running, $0.00 of $5",
    );
    let c = core.clone();
    until("the first line", 10, move || pending(&c) == 0).await;
    post(
        "act_example_a",
        "🖐️ 2/3 done, 1 failed, 1 running, $0.02 of $5",
    );
    post(
        "act_example_a",
        "🖐️ 3/3 done, 1 failed, $0.03 of $5 · done, until met",
    );
    post(
        "act_example_b",
        "🖐️ 0/1 done, 0 failed, 1 running, $0.00 of $1",
    );
    let c = core.clone();
    until("every line delivered", 10, move || pending(&c) == 0).await;
    let lines: Vec<Msg> = replies(&fake)
        .into_iter()
        .filter(|m| m.content.starts_with("🖐️"))
        .collect();
    assert_eq!(lines.len(), 2, "one message a group: {lines:?}");
    let a = lines.iter().find(|m| m.content.contains("of $5")).unwrap();
    assert_eq!(
        a.content,
        "🖐️ 3/3 done, 1 failed, $0.03 of $5 · done, until met"
    );
    assert!(a.edits >= 1, "{a:?}");
}
