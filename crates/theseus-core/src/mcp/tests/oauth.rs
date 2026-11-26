//! A server that signs in (issue 55), through a whole core: the real board
//! and the real HTTP client, the fake authorization server and the fake MCP
//! server on loopback. The owner's `theseus mcp login` (the protocol calls the
//! CLI makes, from the CLI's surface) takes it from `needs sign-in` to a tool
//! a turn calls; what it gave is kept in a private file a new start reads;
//! it is renewed before it runs out and on a 401, rotating; a refresh refused
//! for good leaves it `needs sign-in` with one note and no loop; a logout
//! forgets and revokes; and no value is anywhere it must not be.

use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use theseus_mcp::fake::{self, Fake};
use theseus_mcp::oauth::fake::{AuthConfig, FakeAuth};

use super::{answers_a_call, result_of, session, turn, until, Model};
use crate::approval::{Client, Surface};
use crate::config::mcp::McpOauth;
use crate::config::McpServerConfig;
use crate::mcp::attached::{Attached, AttachedSet};
use crate::node::{Body, ResultStatus};
use crate::policy::Posture;
use crate::provider::{ProviderRequest, Scripted};
use crate::store::Store;
use crate::{Config, Core};

/// Calls `mcp__docs__echo` once a turn, then says it is done.
fn script(req: &ProviderRequest) -> Scripted {
    if answers_a_call(req) {
        return Scripted::text("Done.");
    }
    Scripted::tools(
        "",
        &[("o1", "mcp__docs__echo", json!({"text": "signed in"}))],
    )
}

pub(super) struct Rig {
    pub(super) core: Arc<Core>,
    pub(super) auth: Arc<FakeAuth>,
    pub(super) fake: Arc<Fake>,
    pub(super) dir: tempfile::TempDir,
}

/// A core whose server `docs` signs in at a fake authorization server.
/// `setup` runs on the store directory before the core is built.
pub(super) async fn rig_on(auth: Arc<FakeAuth>, setup: impl FnOnce(&std::path::Path)) -> Rig {
    rig_with(auth, McpOauth::default(), setup).await
}

/// [`rig_on`], with the server's `oauth` settings.
pub(super) async fn rig_with(
    auth: Arc<FakeAuth>,
    oauth: McpOauth,
    setup: impl FnOnce(&std::path::Path),
) -> Rig {
    let fake = Fake::new(fake::Config {
        oauth: Some(auth.clone()),
        name: "docs".into(),
        ..Default::default()
    });
    let addr = fake.clone().serve_http(0).await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = Posture::Notify;
    cfg.mcp.servers.insert(
        "docs".into(),
        McpServerConfig {
            command: vec![],
            env: Default::default(),
            url: Some(format!("http://{addr}/mcp")),
            auth_secret: None,
            oauth: Some(oauth),
            read: vec![],
            sandbox: Default::default(),
            egress: vec![],
            frozen: None,
            external: true,
            enabled: true,
            start_timeout_secs: 10,
            call_timeout_secs: 30,
        },
    );
    let store_dir = dir.path().join("store");
    setup(&store_dir);
    let store = Store::open(&store_dir).unwrap();
    let model = Arc::new(Model {
        script: Box::new(script),
        requests: Mutex::default(),
    });
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, model, store)).unwrap();
    Rig {
        core,
        auth,
        fake,
        dir,
    }
}

pub(super) async fn rig() -> Rig {
    rig_on(FakeAuth::start(AuthConfig::default()).await, |_| {}).await
}

pub(super) fn kept_file(dir: &std::path::Path) -> std::path::PathBuf {
    crate::mcp::oauth::file(&crate::mcp::oauth::dir(&dir.join("store")), "docs")
}

/// One protocol call over a connection of `surface`: its result, or its error
/// as `{"error": {code, message}}`.
pub(super) async fn call(core: &Arc<Core>, surface: Surface, method: &str, params: Value) -> Value {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let (client, server) = tokio::io::duplex(64 * 1024);
    let (sr, sw) = tokio::io::split(server);
    let srv = tokio::spawn(
        core.clone()
            .serve_connection(sr, sw, Client::new("sock#1", surface)),
    );
    let (cr, mut cw) = tokio::io::split(client);
    let req = theseus_protocol::Request::new(theseus_protocol::Id::Num(1), method, params);
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let mut lines = BufReader::new(cr).lines();
    let out = loop {
        let l = lines.next_line().await.unwrap().unwrap();
        if let theseus_protocol::Message::Response(r) = serde_json::from_str(&l).unwrap() {
            break match (r.result, r.error) {
                (Some(v), _) => v,
                (None, e) => json!({"error": e}),
            };
        }
    };
    cw.shutdown().await.unwrap();
    drop(lines);
    let _ = srv.await;
    out
}

pub(super) fn status(r: &Rig) -> theseus_protocol::mcp::McpServerStatus {
    r.core
        .mcp
        .status()
        .into_iter()
        .find(|s| s.name == "docs")
        .expect("docs on the board")
}

/// `theseus mcp login docs`, as the CLI makes it: begin, the owner's browser,
/// finish. Returns what finish said, and every word the CLI was shown.
pub(super) async fn login(r: &Rig) -> (Value, Vec<String>) {
    let begun = call(
        &r.core,
        Surface::Cli,
        "mcp.login.begin",
        json!({"name": "docs"}),
    )
    .await;
    let url = begun["url"]
        .as_str()
        .unwrap_or_else(|| panic!("{begun}"))
        .to_string();
    r.auth.browse(&url, true).await;
    let done = call(
        &r.core,
        Surface::Cli,
        "mcp.login.finish",
        json!({"login_id": begun["login_id"], "wait_secs": 10}),
    )
    .await;
    (done.clone(), vec![begun.to_string(), done.to_string()])
}

pub(super) fn notes(r: &Rig, sid: &str) -> Vec<String> {
    r.core
        .store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .filter(|(_, n)| n.author.as_deref() == Some(super::super::attached::NOTE_AUTHOR))
        .filter_map(|(_, n)| match n.body {
            Body::UserMessage { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

/// The conversation `sid` attached `docs`, as the attach's yes records it.
pub(super) fn attached_by(r: &Rig, sid: &str) {
    let mut set = AttachedSet::new();
    set.insert(
        "docs".into(),
        Attached {
            by: "the owner".into(),
            via: "the CLI".into(),
            at_ms: 1,
            session_id: sid.into(),
        },
    );
    r.core.mcp.note_attached(set);
}

/// The owner's case, whole: never signed in, the server is `needs sign-in`
/// (not a crash, told to no one); the login from the CLI prints the host and
/// an address, the browser's redirect comes back, and the server comes up
/// with its tools; a turn calls one. The sign-in is in a private file. A
/// login from another surface is refused.
#[tokio::test]
async fn the_owner_signs_in_once_and_a_turn_calls_the_servers_tool() {
    let r = rig().await;
    let sid = session(&r.core, None);
    attached_by(&r, &sid);
    r.core.mcp.start();
    until("needs sign-in", || status(&r).state == "needs sign-in").await;
    let s = status(&r);
    assert_eq!(s.crashes, 0, "{s:?}");
    assert_eq!(s.last_error.as_deref(), Some("it has not been signed in"));
    assert!(
        notes(&r, &sid).is_empty(),
        "never signed in: told to no one"
    );
    let web = call(
        &r.core,
        Surface::Web,
        "mcp.login.begin",
        json!({"name": "docs"}),
    )
    .await;
    assert!(
        web["error"]["message"]
            .as_str()
            .unwrap()
            .contains("from the CLI"),
        "{web}"
    );
    let begun = call(
        &r.core,
        Surface::Cli,
        "mcp.login.begin",
        json!({"name": "docs"}),
    )
    .await;
    assert_eq!(begun["host"], "127.0.0.1", "{begun}");
    assert!(begun["redirect"]
        .as_str()
        .unwrap()
        .starts_with("http://127.0.0.1:"));
    r.auth.browse(begun["url"].as_str().unwrap(), true).await;
    let done = call(
        &r.core,
        Surface::Cli,
        "mcp.login.finish",
        json!({"login_id": begun["login_id"], "wait_secs": 10}),
    )
    .await;
    assert_eq!(done["state"], "done", "{done}");
    assert_eq!(done["connected"], true, "{done}");
    assert!(done["tools"].as_u64().unwrap() >= 1, "{done}");
    assert_eq!(status(&r).state, "ready");
    let mode = std::fs::metadata(kept_file(r.dir.path()))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
    turn(&r.core, &sid, "use the docs").await;
    let (st, text, external) = result_of(&r.core, &sid, "o1");
    assert_eq!(
        (st, text.as_str(), external),
        (ResultStatus::Ok, "signed in", true)
    );
    assert_eq!(r.auth.seen().registrations, 1);
}

/// The paste in place of the redirect: one with another sign-in's state is
/// refused and ends that sign-in; the right one finishes a new one.
#[tokio::test]
async fn the_paste_takes_the_redirects_place_and_a_wrong_state_is_refused() {
    let r = rig().await;
    r.core.mcp.start();
    let begun = call(
        &r.core,
        Surface::Cli,
        "mcp.login.begin",
        json!({"name": "docs", "manual": true}),
    )
    .await;
    assert_eq!(begun["redirect"], "", "{begun}");
    assert_eq!(begun["paste_after_secs"], 0);
    let sent_to = r.auth.browse(begun["url"].as_str().unwrap(), false).await;
    let wrong = {
        let mut u = reqwest::Url::parse(&sent_to).unwrap();
        let pairs: Vec<(String, String)> = u
            .query_pairs()
            .map(|(k, v)| {
                let v = if k == "state" {
                    "another".into()
                } else {
                    v.into_owned()
                };
                (k.into_owned(), v)
            })
            .collect();
        u.query_pairs_mut().clear().extend_pairs(pairs);
        u.to_string()
    };
    let e = call(
        &r.core,
        Surface::Cli,
        "mcp.login.finish",
        json!({"login_id": begun["login_id"], "pasted": wrong}),
    )
    .await;
    assert!(
        e["error"]["message"]
            .as_str()
            .unwrap()
            .contains("state is not this sign-in's"),
        "{e}"
    );
    // A paste that is not its redirect ends the sign-in (round 3): the right
    // one is too late, and a new sign-in takes it.
    let late = call(
        &r.core,
        Surface::Cli,
        "mcp.login.finish",
        json!({"login_id": begun["login_id"], "pasted": sent_to}),
    )
    .await;
    assert!(late["error"].is_object(), "{late}");
    let begun = call(
        &r.core,
        Surface::Cli,
        "mcp.login.begin",
        json!({"name": "docs", "manual": true}),
    )
    .await;
    let sent_to = r.auth.browse(begun["url"].as_str().unwrap(), false).await;
    let done = call(
        &r.core,
        Surface::Cli,
        "mcp.login.finish",
        json!({"login_id": begun["login_id"], "pasted": sent_to}),
    )
    .await;
    assert_eq!(done["state"], "done", "{done}");
    assert_eq!(done["connected"], true, "{done}");
}

/// A new start reads what was kept: no login, and the server comes up.
#[tokio::test]
async fn a_restart_reads_what_was_kept() {
    let first = rig().await;
    first.core.mcp.start();
    let (done, _) = login(&first).await;
    assert_eq!(done["connected"], true, "{done}");
    let kept = std::fs::read(kept_file(first.dir.path())).unwrap();
    first.core.mcp.stop();
    // A second daemon on another store with the kept file, and the same
    // authorization server, which still takes the token.
    let second = rig_on(first.auth.clone(), |store| {
        let p = crate::mcp::oauth::file(&crate::mcp::oauth::dir(store), "docs");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, &kept).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
    })
    .await;
    // Its server's address is its own fake's: the kept sign-in names the first.
    let s = second
        .core
        .mcp
        .server("docs")
        .unwrap()
        .cfg
        .url
        .clone()
        .unwrap();
    // The kept sign-in is bound to its server: its address and its resource
    // move with it.
    let mut k: crate::mcp::oauth::Kept = serde_json::from_slice(&kept).unwrap();
    k.url = s.clone();
    k.resource = s;
    crate::mcp::oauth::write(&kept_file(second.dir.path()), &k).unwrap();
    second.core.mcp.start();
    until("ready from the kept sign-in", || {
        status(&second).state == "ready"
    })
    .await;
    assert_eq!(second.auth.seen().redeemed, 1, "no second sign-in");
}

/// Renewed before it runs out (a token that runs out within the minute is
/// renewed before it is sent), and once on a 401; a rotated refresh token is
/// written back whole each time.
#[tokio::test]
async fn it_is_renewed_before_it_runs_out_and_on_a_401_with_rotation() {
    // Every token the server gives runs out in 30 s: each is renewed before
    // it is sent.
    let auth = FakeAuth::start(AuthConfig {
        access_ttl: 30,
        ..AuthConfig::default()
    })
    .await;
    let r = rig_on(auth, |_| {}).await;
    r.core.mcp.start();
    let (done, _) = login(&r).await;
    assert_eq!(done["connected"], true, "{done}");
    // What the connection's own requests renewed settles first.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(r.auth.seen().refreshes >= 1, "renewed before it ran out");
    let refresh_of = || {
        crate::mcp::oauth::read(&kept_file(r.dir.path()))
            .unwrap()
            .unwrap()
            .refresh_token
            .clone()
            .unwrap()
    };
    // Now the server gives an hour: one renewal more, then none.
    r.auth.set(|c| c.access_ttl = 3600);
    let renewed = r.auth.seen().refreshes;
    let first = refresh_of();
    let client = r
        .core
        .mcp
        .server("docs")
        .unwrap()
        .client(Duration::from_secs(5))
        .await
        .unwrap();
    client
        .call_tool("echo", json!({"text": "a"}))
        .await
        .unwrap();
    assert_eq!(r.auth.seen().refreshes, renewed + 1);
    let second = refresh_of();
    assert_ne!(first, second, "rotated and written back");
    client
        .call_tool("echo", json!({"text": "a"}))
        .await
        .unwrap();
    assert_eq!(
        r.auth.seen().refreshes,
        renewed + 1,
        "an hour left: not renewed"
    );
    // On a 401.
    r.auth.expire_access();
    let out = client
        .call_tool("echo", json!({"text": "b"}))
        .await
        .unwrap();
    assert_eq!(out.text_for_model(), "b");
    assert_eq!(r.auth.seen().refreshes, renewed + 2);
    assert_ne!(refresh_of(), second);
}

/// A refresh refused for good: `needs sign-in`, one note to the conversation
/// that attached it naming the one command, no crash counted, and no loop:
/// an hour later the authorization server has seen one refused refresh.
#[tokio::test]
async fn a_refused_refresh_gives_needs_sign_in_one_note_and_no_loop() {
    let r = rig().await;
    let sid = session(&r.core, None);
    attached_by(&r, &sid);
    r.core.mcp.start();
    let (done, _) = login(&r).await;
    assert_eq!(done["connected"], true, "{done}");
    r.auth.revoke_all();
    let client = r
        .core
        .mcp
        .server("docs")
        .unwrap()
        .client(Duration::from_secs(5))
        .await
        .unwrap();
    assert!(client
        .call_tool("echo", json!({"text": "x"}))
        .await
        .is_err());
    until("needs sign-in", || status(&r).state == "needs sign-in").await;
    until("the note", || !notes(&r, &sid).is_empty()).await;
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(3600)).await;
    tokio::time::resume();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let n = notes(&r, &sid);
    assert_eq!(n.len(), 1, "{n:?}");
    assert!(n[0].contains("`docs`"), "{}", n[0]);
    assert!(n[0].contains("`theseus mcp login docs`"), "{}", n[0]);
    assert!(n[0].contains("invalid_grant"), "{}", n[0]);
    assert!(
        !n[0].contains("the fake says no"),
        "the server's words: {}",
        n[0]
    );
    let s = status(&r);
    assert_eq!((s.state.as_str(), s.crashes), ("needs sign-in", 0), "{s:?}");
    assert_eq!(r.auth.seen().refused_refreshes, 1, "one refresh, no loop");
    assert!(s.last_error.unwrap().contains("refused when renewed"));
    // A call says the command.
    let Err(e) = r
        .core
        .mcp
        .server("docs")
        .unwrap()
        .client(Duration::from_millis(50))
        .await
    else {
        panic!("a server that needs a sign-in gave a client");
    };
    assert!(e.contains("`theseus mcp login docs`"), "{e}");
}

/// Logout forgets the kept sign-in, revokes both tokens at the server, and
/// leaves the server `needs sign-in`.
#[tokio::test]
async fn logout_forgets_and_revokes() {
    let r = rig().await;
    r.core.mcp.start();
    let (done, _) = login(&r).await;
    assert_eq!(done["connected"], true, "{done}");
    let out = call(&r.core, Surface::Cli, "mcp.logout", json!({"name": "docs"})).await;
    assert_eq!(
        (out["forgot"].clone(), out["revoked"].clone()),
        (json!(true), json!(true)),
        "{out}"
    );
    assert!(!kept_file(r.dir.path()).exists());
    assert_eq!(r.auth.seen().revoked, ["refresh_token", "access_token"]);
    until("needs sign-in", || status(&r).state == "needs sign-in").await;
    let web = call(&r.core, Surface::Web, "mcp.logout", json!({"name": "docs"})).await;
    assert!(web["error"].is_object(), "{web}");
}

/// After a whole sign-in, a tool call, a renewal and a refused refresh, no
/// token, code or verifier is in any node, ledger record, note, captured log
/// line, status, or what the CLI was given.
#[tokio::test]
async fn no_value_is_anywhere_it_must_not_be() {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let sub = tracing_subscriber::fmt()
        .with_writer(Sink(buf.clone()))
        .with_env_filter("theseus_core=trace,theseus_mcp=trace,theseus_store=trace")
        .with_ansi(false)
        .finish();
    let _guard = tracing::subscriber::set_default(sub);
    let r = rig().await;
    let sid = session(&r.core, None);
    attached_by(&r, &sid);
    r.core.mcp.start();
    let (done, mut said) = login(&r).await;
    assert_eq!(done["connected"], true, "{done}");
    turn(&r.core, &sid, "use the docs").await;
    r.auth.expire_access();
    turn(&r.core, &sid, "again").await;
    r.auth.revoke_all();
    turn(&r.core, &sid, "and again").await;
    until("needs sign-in", || status(&r).state == "needs sign-in").await;
    said.push(format!("{:?}", r.core.mcp.status()));
    said.push(
        call(&r.core, Surface::Cli, "mcp.list", Value::Null)
            .await
            .to_string(),
    );
    said.push(
        call(&r.core, Surface::Cli, "health", Value::Null)
            .await
            .to_string(),
    );
    let nodes: Vec<String> = r
        .core
        .store
        .session_nodes(&sid)
        .unwrap()
        .into_iter()
        .map(|(_, n)| format!("{n:?}"))
        .collect();
    let ledger: Vec<String> = r
        .core
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(100_000)
        .unwrap()
        .into_iter()
        .map(|(_, row)| format!("{row:?}"))
        .collect();
    let log = String::from_utf8_lossy(&buf.lock().unwrap()).into_owned();
    let secrets = r.auth.seen().secrets;
    assert!(
        secrets.len() >= 5,
        "codes, tokens and the verifier: {}",
        secrets.len()
    );
    assert!(r.fake.seen().unauthorized >= 1);
    for value in &secrets {
        for (what, text) in [
            ("a node", nodes.join("\n")),
            ("the ledger", ledger.join("\n")),
            ("a log line", log.clone()),
            ("what was said", said.join("\n")),
        ] {
            assert!(!text.contains(value.as_str()), "a value is in {what}");
        }
    }
    assert!(!log.contains("harbour portal"), "a body in a log line");
    // Each value is known to the scrubber.
    for value in &secrets {
        if value.starts_with("fake-at-") || value.starts_with("fake-rt-") {
            let (cut, _) = crate::scrub::Scrubber::from_board(r.core.secrets.clone()).scrub(value);
            assert!(
                !cut.contains(value.as_str()),
                "the scrubber does not know a value"
            );
        }
    }
}

#[derive(Clone)]
pub(super) struct Sink(pub(super) Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Sink {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Sink {
    type Writer = Sink;
    fn make_writer(&'a self) -> Sink {
        self.clone()
    }
}

/// The failure note of a server that does not sign in and is refused with a
/// 401 names the sign-in's command (issue 55).
#[test]
fn a_401_note_names_the_login_command() {
    let n = crate::mcp::attached::failed_note("docs", r#"HTTP 401: {"error":"invalid_token"}"#);
    assert!(n.contains("`theseus mcp login docs`"), "{n}");
    assert!(n.contains("`theseus mcp restart docs`"), "{n}");
    let n = crate::mcp::attached::failed_note("docs", "HTTP 503: down");
    assert!(!n.contains("login"), "{n}");
}
