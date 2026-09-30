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

const USER: u64 = 159_471_966_640_799_744;
/// The fake's DM channel with `USER`.
const DM: u64 = USER + 1;
const CHANNEL: u64 = 900_000_000_000_000_001;
const GUILD: &str = "712398310421561444";

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
            session_id: Some(sid.into()),
            input: input.into(),
            profile: None,
            provider: None,
            model: None,
            author: Some("test".into()),
            attachments: vec![],
            reply_to: None,
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
    core.confirm_action(&q, false, Some("not now"), "cli#1")
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

/// 2b's approval route through the lanes: a card for a place that is not a
/// trusted channel goes to the trusted user's DM, with a note in the place;
/// how its question closed edits both, the card's buttons gone.
#[tokio::test]
async fn a_card_for_an_untrusted_channel_goes_to_the_dm_and_its_settle_edits_both() {
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
    let core = core_at(d.path(), &fake, script, |c| {
        c.approval = Some(theseus_core::config::ApprovalConfig {
            trusted_users: vec![format!("discord:{USER}")],
            channels: vec!["discord:dm".into(), "cli".into()],
        })
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
    let q = ask(&rpc, &sid, "write a")
        .await
        .awaiting_confirm
        .expect("the write waits");
    let c = core.clone();
    until("the card and the reply delivered", 10, move || {
        pending(&c) == 0
    })
    .await;
    let card = fake
        .messages(DM)
        .into_iter()
        .find(|m| m.content.starts_with("**Approve?** `fs.write` a.txt"))
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
        "🔐 Approval for `fs.write` a.txt was asked in DM @eddie: this channel is not a trusted \
         channel (it is not listed in [approval] channels)."
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
        "❎ **Declined** by operator · `fs.write` a.txt"
    );
    assert_eq!(card.components, 0, "its buttons are gone");
    let note = fake
        .messages(CHANNEL)
        .into_iter()
        .find(|m| m.id == note.id)
        .unwrap();
    assert_eq!(
        note.content,
        "🔐 ❎ **Declined** by operator · `fs.write` a.txt (in DM @eddie)"
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
