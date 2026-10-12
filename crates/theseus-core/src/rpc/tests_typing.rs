//! A person's first keystroke (theseus-tnky), through the core: one
//! `session.typing` opens the session's provider connection (a stand-in that
//! counts warm-ups and model requests apart) and sends the index tender
//! `index.warm` (a stand-in socket that counts them), once per session per
//! idle spell, nothing on a turn's path waits for either, and a shared
//! place's typing by someone else warms nothing.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use theseus_protocol::index::IndexWarmResult;
use theseus_protocol::warm::SessionTypingResult;
use theseus_protocol::{Message, Request, Response};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::approval::{Client, Surface};
use crate::provider::{
    DeltaSink, FakeProvider, Provider, ProviderFuture, ProviderRequest, WarmFuture,
};
use crate::tests_recall::{session, turn, ALICE, DEN, OWNER, PIER};
use crate::{Config, Core};

/// A core over a stand-in provider `model` (the default provider's), with
/// the owner on Discord, in a temporary state directory.
struct Rig {
    core: Arc<Core>,
    model: Arc<FakeProvider>,
    _dir: tempfile::TempDir,
}

fn rig_with(
    tweak: impl FnOnce(&mut crate::rpc::Parts),
    model: Arc<dyn Provider>,
    fake: Arc<FakeProvider>,
) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.roots = vec![];
    cfg.places.owner = Some(vec![format!("discord:{OWNER}")]);
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let mut p = crate::rpc::Parts::for_tests(cfg, model, store);
    tweak(&mut p);
    let core = Core::build(p).unwrap();
    core.bind_places(vec![crate::places::BoundPlace {
        target: format!("discord:channel:{DEN}"),
        name: "#den".into(),
        private: true,
        ..Default::default()
    }]);
    Rig {
        core,
        model: fake,
        _dir: dir,
    }
}

fn rig() -> Rig {
    let fake = Arc::new(FakeProvider::default());
    rig_with(|_| {}, fake.clone(), fake)
}

/// What the stand-in tender saw.
#[derive(Default)]
struct Tender {
    warms: AtomicUsize,
}

/// A stand-in tender on the core's own tender socket: every request is an
/// `index.warm`, counted and answered `loading`. The core's supervisor is
/// told it runs.
fn tender(core: &Core) -> Arc<Tender> {
    std::fs::create_dir_all(core.index.dir()).unwrap();
    let listener = tokio::net::UnixListener::bind(core.index.socket()).unwrap();
    core.index.mark_running();
    let seen = Arc::new(Tender::default());
    let counted = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((conn, _)) = listener.accept().await else {
                return;
            };
            let seen = counted.clone();
            tokio::spawn(async move {
                let (r, mut w) = conn.into_split();
                let mut r = BufReader::new(r);
                let mut line = String::new();
                if r.read_line(&mut line).await.unwrap_or(0) == 0 {
                    return;
                }
                let req: Request = serde_json::from_str(&line).unwrap();
                assert_eq!(req.method, "index.warm", "the tender is asked nothing else");
                seen.warms.fetch_add(1, Ordering::SeqCst);
                let answer = IndexWarmResult {
                    model: "loading".into(),
                    mode: "hybrid".into(),
                };
                let mut out = serde_json::to_vec(&Response::ok(req.id, answer)).unwrap();
                out.push(b'\n');
                let _ = w.write_all(&out).await;
            });
        }
    });
    seen
}

/// One request on a connection of `client`'s surface, and its result.
async fn rpc_as(core: &Arc<Core>, client: Client, method: &str, params: Value) -> Value {
    let (ours, theirs) = tokio::io::duplex(64 * 1024);
    let (sr, sw) = tokio::io::split(theirs);
    let srv = tokio::spawn(core.clone().serve_connection(sr, sw, client));
    let (cr, mut cw) = tokio::io::split(ours);
    let req = Request::new(theseus_protocol::Id::Num(1), method, params);
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let mut lines = BufReader::new(cr).lines();
    let out = loop {
        let l = lines.next_line().await.unwrap().unwrap();
        if let Message::Response(r) = serde_json::from_str(&l).unwrap() {
            break r
                .result
                .unwrap_or_else(|| panic!("{method}: {:?}", r.error));
        }
    };
    cw.shutdown().await.unwrap();
    drop(lines);
    let _ = srv.await;
    out
}

async fn typing(core: &Arc<Core>, client: Client, params: Value) -> SessionTypingResult {
    let v = rpc_as(core, client, "session.typing", params).await;
    serde_json::from_value(v).unwrap()
}

fn cli() -> Client {
    Client::new("sock#1", Surface::Cli)
}

fn discord() -> Client {
    Client::new("discord", Surface::Discord)
}

fn origin(user: u64, channel: u64, guild: bool) -> Value {
    json!({"user_id": user.to_string(), "channel_id": channel.to_string(),
           "guild_id": guild.then_some("7")})
}

/// Wait, on the real clock, until `done` holds (the warm-ups run beside the
/// answer): false at the bound.
async fn until(done: impl Fn() -> bool) -> bool {
    for _ in 0..200 {
        if done() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    false
}

/// A quiet stretch for what must not happen to have happened.
async fn settle() {
    tokio::time::sleep(Duration::from_millis(150)).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_first_keystroke_opens_the_connection_and_warms_the_tender_once() {
    let r = rig();
    let t = tender(&r.core);
    let sid = session(&r.core, None, &["the weir is quiet"]);
    let said = typing(&r.core, cli(), json!({"session_id": sid})).await;
    assert!(said.started, "{said:?}");
    assert!(
        until(|| r.model.warms.load(Ordering::SeqCst) == 1 && t.warms.load(Ordering::SeqCst) == 1)
            .await,
        "one warm-up of each: provider {}, tender {}",
        r.model.warms.load(Ordering::SeqCst),
        t.warms.load(Ordering::SeqCst)
    );
    assert!(
        r.model.requests.lock().unwrap().is_empty(),
        "a warm-up is never a model call"
    );
    // The rest of the spell: the notice is taken, and nothing more is sent.
    for _ in 0..3 {
        let again = typing(&r.core, cli(), json!({"session_id": sid})).await;
        assert!(!again.started, "{again:?}");
        assert!(again.why.unwrap().contains("idle spell"));
    }
    settle().await;
    assert_eq!(r.model.warms.load(Ordering::SeqCst), 1, "no second warm-up");
    assert_eq!(t.warms.load(Ordering::SeqCst), 1, "no second index.warm");
    // Health says when each was warmed.
    let h = r.core.warmth.health();
    assert_eq!((h.started, h.dropped), (1, 3));
    assert_eq!(h.tender.unwrap().outcome, "loading");
    assert_eq!(h.provider.unwrap().outcome, "opened");
    let health = r.core.health_now().await;
    assert_eq!(health.warm.unwrap().started, 1, "health carries the block");
}

#[tokio::test(flavor = "multi_thread")]
async fn another_session_has_its_own_spell() {
    let r = rig();
    let (a, b) = (session(&r.core, None, &[]), session(&r.core, None, &[]));
    assert!(
        typing(&r.core, cli(), json!({"session_id": a}))
            .await
            .started
    );
    assert!(
        typing(&r.core, cli(), json!({"session_id": b}))
            .await
            .started
    );
    assert!(until(|| r.model.warms.load(Ordering::SeqCst) == 2).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tender_that_is_off_leaves_the_provider_warmed_and_says_nothing() {
    let r = rig();
    // Never marked running: pending, as a daemon's first seconds.
    assert!(typing(&r.core, cli(), json!({})).await.started);
    assert!(until(|| r.model.warms.load(Ordering::SeqCst) == 1).await);
    settle().await;
    let h = r.core.warmth.health();
    assert!(h.provider.is_some());
    assert!(h.tender.is_none(), "nothing was warmed there");
}

/// The provider a session's last turn ran on is the one warmed, not the
/// default.
#[tokio::test(flavor = "multi_thread")]
async fn the_provider_warmed_is_the_one_the_sessions_last_turn_ran_on() {
    let other = Arc::new(FakeProvider::default());
    let fake = Arc::new(FakeProvider::default());
    let r = rig_with(
        |p| {
            p.providers.insert("other".into(), other.clone());
        },
        fake.clone(),
        fake,
    );
    let sid = session(&r.core, None, &[]);
    let mut rec: crate::session::SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
    rec.last_target = Some(crate::session::TargetRef {
        profile: "p".into(),
        provider: "other".into(),
        model: "m".into(),
    });
    r.core.store.put_session(&sid, &rec).unwrap();
    assert!(
        typing(&r.core, cli(), json!({"session_id": sid}))
            .await
            .started
    );
    assert!(until(|| other.warms.load(Ordering::SeqCst) == 1).await);
    settle().await;
    assert_eq!(r.model.warms.load(Ordering::SeqCst), 0, "not the default's");
}

/// The Discord binding's notice: an owner's typing warms, anyone else's in a
/// shared place warms nothing, and a notice that names no one is refused.
#[tokio::test(flavor = "multi_thread")]
async fn a_shared_places_typing_by_someone_else_warms_nothing() {
    let r = rig();
    let t = tender(&r.core);
    let sid = session(&r.core, Some(&format!("channel:{PIER}")), &[]);
    let from =
        |user, channel, guild| json!({"session_id": sid, "discord": origin(user, channel, guild)});
    for (what, p) in [
        ("alice in a shared channel", from(ALICE, PIER, true)),
        ("alice in the private den", from(ALICE, DEN, true)),
        ("a notice naming no one", json!({"session_id": sid})),
    ] {
        let said = typing(&r.core, discord(), p).await;
        assert!(!said.started, "{what}: {said:?}");
        assert!(said.why.is_some(), "{what}");
    }
    assert!(
        !crate::mcp_server::allowed(theseus_protocol::method::SESSION_TYPING),
        "an MCP client types nothing"
    );
    settle().await;
    assert_eq!(
        r.model.warms.load(Ordering::SeqCst),
        0,
        "no connection opened"
    );
    assert_eq!(t.warms.load(Ordering::SeqCst), 0, "no index.warm");
    assert_eq!(r.core.warmth.health().dropped, 3);
    // The owner's own typing, in the same shared channel, is theirs to warm.
    let own = typing(&r.core, discord(), from(OWNER, PIER, true)).await;
    assert!(own.started, "{own:?}");
    assert!(
        until(|| r.model.warms.load(Ordering::SeqCst) == 1 && t.warms.load(Ordering::SeqCst) == 1)
            .await
    );
}

/// A provider whose warm-up never ends: the notice's answer, and a turn, wait
/// for none of it.
struct Hangs(FakeProvider);

impl Provider for Hangs {
    fn name(&self) -> &str {
        "hangs"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        self.0.stream_message(req, on_delta)
    }
    fn warm(&self) -> WarmFuture<'_> {
        Box::pin(std::future::pending())
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn nothing_on_the_turns_path_waits_for_a_warm_up() {
    let fake = Arc::new(FakeProvider::default());
    let r = rig_with(|_| {}, Arc::new(Hangs(FakeProvider::default())), fake);
    let sid = session(&r.core, None, &[]);
    let said = tokio::time::timeout(
        Duration::from_secs(2),
        typing(&r.core, cli(), json!({"session_id": sid})),
    )
    .await
    .expect("the notice answers at once");
    assert!(said.started);
    // Its warm-up still hangs, and the turn runs to its end.
    let done = tokio::time::timeout(Duration::from_secs(10), turn(&r.core, &sid, "hello"))
        .await
        .expect("the turn does not wait for the warm-up");
    assert_eq!(done.output, "fake reply");
}
