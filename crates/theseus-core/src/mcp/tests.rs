//! The board's tests (M7 36b), with the fake MCP server in this process
//! (`theseus_mcp::fake`, over pipes): a stand-in model calls
//! `mcp__fake__echo` and gets a result node, a notice, and T1's hold; a
//! shared place's turn is offered no MCP tool and its call is refused; a
//! stored list is offered before its server is up, and a call waits for its
//! own server alone; a crash restarts after 1 s, then 5 s, then fails until
//! restarted; `list_changed` applies at the next turn, with its row; and a
//! server's hints never loosen a tool.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::future::BoxFuture;
use serde_json::{json, Value};
use theseus_mcp::fake::{self, Fake, Mode};
use theseus_mcp::{Client, Transport};
use theseus_protocol::{SessionKind, TurnSubmitResult};
use theseus_tools::{Tool, ToolClass, ToolCtx};

use super::{Connect, Connected, McpBoard, McpCatalog, State, StoredList};
use crate::bus::EventSink;
use crate::config::McpServerConfig;
use crate::node::{Body, ResultStatus};
use crate::policy::Posture;
use crate::provider::{
    DeltaSink, FakeProvider, Provider, ProviderFuture, ProviderRequest, Scripted,
};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

/// Each server's fake, served over a fresh pair of pipes at each connect;
/// a server named in `down` refuses to start, as a crashing process does.
#[derive(Default)]
struct InProcess {
    fakes: Mutex<BTreeMap<String, Arc<Fake>>>,
    down: Mutex<Vec<String>>,
    connects: AtomicU64,
}

impl InProcess {
    fn with(servers: &[(&str, Mode)]) -> Arc<Self> {
        let me = Self::default();
        for (name, mode) in servers {
            me.fakes.lock().unwrap().insert(
                name.to_string(),
                Fake::new(fake::Config {
                    mode: *mode,
                    name: name.to_string(),
                    ..Default::default()
                }),
            );
        }
        Arc::new(me)
    }

    fn set_down(&self, name: &str, down: bool) {
        let mut d = self.down.lock().unwrap();
        d.retain(|n| n != name);
        if down {
            d.push(name.into());
        }
    }
}

impl Connect for InProcess {
    fn connect(
        &self,
        server: &str,
        _cfg: &McpServerConfig,
        _env: Vec<(String, String)>,
        _bearer: Option<String>,
    ) -> BoxFuture<'static, Result<Connected, String>> {
        self.connects.fetch_add(1, Ordering::SeqCst);
        let down = self.down.lock().unwrap().iter().any(|n| n == server);
        let fake = self.fakes.lock().unwrap().get(server).cloned();
        Box::pin(async move {
            if down {
                return Err("the server exited with status 3".into());
            }
            let fake = fake.ok_or("no such fake")?;
            let (ours, theirs) = tokio::io::duplex(1 << 16);
            let (r, w) = tokio::io::split(theirs);
            tokio::spawn(fake.serve_pipes(r, w));
            let (r, w) = tokio::io::split(ours);
            let (client, events) = Client::connect(Transport::pipes(r, w), Default::default())
                .await
                .map_err(|e| e.to_string())?;
            Ok(Connected { client, events })
        })
    }
}

fn server_cfg(read: &[&str]) -> McpServerConfig {
    McpServerConfig {
        command: vec!["theseus-sim".into(), "fake-mcp".into()],
        env: Default::default(),
        url: None,
        auth_secret: None,
        read: read.iter().map(|s| s.to_string()).collect(),
        sandbox: Default::default(),
        external: true,
        enabled: true,
        start_timeout_secs: 30,
        call_timeout_secs: 110,
    }
}

/// A board alone, with no core: its facts go nowhere.
fn board(
    servers: &[(&str, McpServerConfig)],
    connect: Arc<InProcess>,
    stored: impl Fn(&str) -> Option<StoredList>,
) -> (Arc<McpBoard>, Arc<McpCatalog>) {
    let cfg = crate::config::McpConfig {
        servers: servers
            .iter()
            .map(|(n, c)| (n.to_string(), c.clone()))
            .collect(),
    };
    let catalog = Arc::new(McpCatalog::default());
    let board = McpBoard::new(
        &cfg,
        catalog.clone(),
        Arc::new(crate::broker::Broker::empty()),
        crate::secrets::SecretBoard::empty(),
        connect,
        stored,
    );
    (board, catalog)
}

async fn settle() {
    for _ in 0..50 {
        tokio::task::yield_now().await;
    }
}

/// The board's first state test, on tokio's paused clock: a server that
/// keeps crashing at its start restarts after 1 s, then 5 s, and its third
/// crash in 10 minutes leaves it failed, with its reason, until restarted;
/// a restart starts it at once, its crashes forgotten.
#[tokio::test(start_paused = true)]
async fn a_crash_restarts_after_one_then_five_seconds_then_fails_until_restarted() {
    let connect = InProcess::with(&[("fake", Mode::Ok)]);
    connect.set_down("fake", true);
    let (board, _) = board(&[("fake", server_cfg(&[]))], connect.clone(), |_| None);
    let s = board.server("fake").unwrap().clone();
    assert_eq!(s.state(), State::Stopped, "nothing starts before serving");
    board.start();
    settle().await;
    assert_eq!(connect.connects.load(Ordering::SeqCst), 1);
    assert_eq!(s.state(), State::Restarting);
    tokio::time::sleep(Duration::from_millis(990)).await;
    settle().await;
    assert_eq!(connect.connects.load(Ordering::SeqCst), 1, "1 s first");
    tokio::time::sleep(Duration::from_millis(20)).await;
    settle().await;
    assert_eq!(connect.connects.load(Ordering::SeqCst), 2);
    // The second crash was at 1 s, and this is 1.01 s.
    tokio::time::sleep(Duration::from_millis(4_900)).await;
    settle().await;
    assert_eq!(connect.connects.load(Ordering::SeqCst), 2, "then 5 s");
    tokio::time::sleep(Duration::from_millis(200)).await;
    settle().await;
    assert_eq!(connect.connects.load(Ordering::SeqCst), 3);
    assert_eq!(s.state(), State::Failed, "a third crash in 10 minutes");
    let st = s.status();
    assert_eq!((st.state.as_str(), st.crashes), ("failed", 3));
    assert!(st.last_error.unwrap().contains("status 3"));
    // Failed stays failed.
    tokio::time::sleep(Duration::from_secs(3_600)).await;
    settle().await;
    assert_eq!(connect.connects.load(Ordering::SeqCst), 3);
    // A call to it says so at once, and how to start it again.
    let Err(why) = s.client(Duration::from_secs(30)).await else {
        panic!("a failed server answers no call")
    };
    assert!(why.contains("theseus mcp restart fake"), "{why}");
    // `theseus mcp restart fake`.
    connect.set_down("fake", false);
    assert_eq!(board.restart("fake"), Some(State::Failed));
    settle().await;
    tokio::time::sleep(Duration::from_millis(10)).await;
    settle().await;
    assert_eq!(s.state(), State::Ready);
    assert_eq!(s.status().crashes, 0);
    assert!(board.restart("nobody").is_none());
}

/// A restart offers the stored list before its server is up (FAST, and the
/// cache), and the first call waits for that server alone: another server
/// that never comes up delays nothing.
#[tokio::test]
async fn a_stored_list_is_offered_at_once_and_a_call_waits_for_its_own_server_alone() {
    let connect = InProcess::with(&[("fake", Mode::Ok), ("other", Mode::Ok)]);
    connect.set_down("other", true);
    let stored = StoredList {
        digest: "stored".into(),
        tools: vec![serde_json::from_value(json!({
            "name": "echo", "description": "Echoes its text.",
            "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}}
        }))
        .unwrap()],
    };
    let (board, catalog) = board(
        &[("fake", server_cfg(&[])), ("other", server_cfg(&[]))],
        connect.clone(),
        move |n| (n == "fake").then(|| stored.clone()),
    );
    // Offered at once, with no server started.
    let tool = catalog.by_wire("mcp__fake__echo").expect("the stored tool");
    assert_eq!(tool.name(), "mcp:fake/echo");
    assert!(board.server("fake").unwrap().status().stored);
    assert_eq!(connect.connects.load(Ordering::SeqCst), 0);
    // A call before the start waits for its server, then runs.
    let ctx = ToolCtx::for_tests(Path::new("/tmp"));
    let call = tokio::spawn(tool.run_async(&json!({"text": "hello"}), &ctx));
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!call.is_finished(), "it waits for its server");
    board.start();
    let (out, external) = tokio::time::timeout(Duration::from_secs(10), call)
        .await
        .expect("the call waited for fake alone")
        .unwrap()
        .unwrap();
    assert!(out.text.contains("hello"), "{}", out.text);
    assert_eq!(external.unwrap().url, "mcp:fake/echo");
    // The live list replaced the stored one; the other server is still down.
    assert!(!board.server("fake").unwrap().status().stored);
    assert!(catalog.by_wire("mcp__fake__add").is_some());
    assert_ne!(board.server("other").unwrap().state(), State::Ready);
    board.stop();
}

/// The gate's class: a tool is `Run` and not repeatable unless the operator
/// lists it in `read`, whatever the server's hints say; the hints are shown.
#[tokio::test]
async fn a_servers_hints_never_loosen_a_tool_and_read_makes_it_read() {
    let connect = InProcess::with(&[("fake", Mode::Ok)]);
    let (board, catalog) = board(&[("fake", server_cfg(&["add"]))], connect, |_| None);
    board.start();
    let s = board.server("fake").unwrap().clone();
    assert!(s.client(Duration::from_secs(10)).await.is_ok());
    let all = catalog.all();
    let echo = all.iter().find(|t| t.listed.name == "echo").unwrap();
    let add = all.iter().find(|t| t.listed.name == "add").unwrap();
    assert_eq!(echo.hints(), ["read-only"], "shown");
    assert_eq!(Tool::class(echo.as_ref()), ToolClass::Run, "never loosened");
    assert_eq!(echo.retry(), theseus_tools::Retry::NonRepeatable);
    assert_eq!(
        Tool::class(add.as_ref()),
        ToolClass::Read,
        "the operator's read"
    );
    assert_eq!(add.retry(), theseus_tools::Retry::SafeToRepeat);
    assert_eq!(echo.family(), "mcp");
    // Sorted by canonical name, every wire name unique and the provider's.
    let names: Vec<&str> = all.iter().map(|t| t.canonical.as_str()).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(names, sorted);
    assert!(all.iter().all(|t| t.wire.len() <= 64));
    // An argument that is not an object is the model's mistake.
    let ctx = ToolCtx::for_tests(Path::new("/tmp"));
    assert!(echo.plan(&json!("text"), &ctx).is_err());
    assert_eq!(
        echo.plan(&json!({"text": "x"}), &ctx).unwrap().summary,
        "mcp:fake/echo {\"text\":\"x\"}"
    );
    board.stop();
}

/// A description is capped at 2,000 characters, saying so.
#[test]
fn a_description_is_capped() {
    let mut d = "x".repeat(5_000);
    super::tool::cap_description(&mut d);
    assert!(d.starts_with(&"x".repeat(2_000)) && d.ends_with("[cut at 2,000 characters]"));
    let mut short = "short".to_string();
    super::tool::cap_description(&mut short);
    assert_eq!(short, "short");
}

// ---- through the whole core ----

struct Model {
    script: Box<dyn Fn(&ProviderRequest) -> Scripted + Send + Sync>,
    requests: Mutex<Vec<ProviderRequest>>,
}

impl Provider for Model {
    fn name(&self) -> &str {
        "fake"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        self.requests.lock().unwrap().push(req.clone());
        Box::pin(async move {
            let answer = (self.script)(req);
            FakeProvider::scripted(vec![answer])
                .stream_message(req, on_delta)
                .await
        })
    }
}

fn answers_a_call(req: &ProviderRequest) -> bool {
    req.messages
        .last()
        .and_then(|m| m["content"].as_array())
        .is_some_and(|c| c.iter().any(|b| b["type"] == "tool_result"))
}

/// Calls `mcp__fake__echo` once a turn, then says it is done.
fn script(req: &ProviderRequest) -> Scripted {
    if answers_a_call(req) {
        return Scripted::text("Done.");
    }
    Scripted::tools(
        "",
        &[("m1", "mcp__fake__echo", json!({"text": "from the fake"}))],
    )
}

struct Rig {
    core: Arc<Core>,
    model: Arc<Model>,
    _dir: tempfile::TempDir,
}

fn rig(mode: Mode) -> Rig {
    rig_with(mode, server_cfg(&[]), None)
}

/// `rig`, with this server's table, and this list stored before the core
/// is built.
fn rig_with(mode: Mode, server: McpServerConfig, stored: Option<StoredList>) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = Posture::Notify;
    cfg.mcp.servers.insert("fake".into(), server);
    let store = Store::open(&dir.path().join("store")).unwrap();
    if let Some(list) = stored {
        store
            .put_meta(&format!("{}fake", super::STORED_PREFIX), &list)
            .unwrap();
    }
    let model = Arc::new(Model {
        script: Box::new(script),
        requests: Mutex::default(),
    });
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, model.clone(), store)).unwrap();
    core.mcp.set_connect(InProcess::with(&[("fake", mode)]));
    Rig {
        core,
        model,
        _dir: dir,
    }
}

fn session(core: &Core, place: Option<&str>) -> String {
    let r = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&r.session_id, &r).unwrap();
    if let Some(p) = place {
        core.outbox.bind_place(p, &r.session_id).unwrap();
    }
    r.session_id
}

async fn turn(core: &Arc<Core>, sid: &str, input: &str) -> TurnSubmitResult {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    core.runner
        .run(TurnRequest {
            session: rec,
            input: Some(input.into()),
            target,
            sink: EventSink::new(core.bus.clone(), sid, None),
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
        })
        .await
        .unwrap()
}

fn offered(req: &ProviderRequest) -> Vec<String> {
    req.tools
        .iter()
        .filter_map(|t| t["name"].as_str().map(str::to_string))
        .collect()
}

fn ledgered(core: &Core, kind: &str) -> Vec<Value> {
    core.store
        .ledger_tail::<crate::ledger::LedgerRow>(100_000)
        .unwrap()
        .into_iter()
        .filter(|(_, r)| r.kind == kind)
        .map(|(_, r)| r.data)
        .collect()
}

/// The result node of the call `id` in a session: its status, its text,
/// and whether it is outside text.
fn result_of(core: &Core, sid: &str, id: &str) -> (ResultStatus, String, bool) {
    core.store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match &n.body {
            Body::ToolResult {
                tool_use_id,
                status,
                content,
                external,
                ..
            } if tool_use_id == id => Some((*status, content.clone(), external.is_some())),
            _ => None,
        })
        .expect("a result")
}

async fn until(what: &str, mut f: impl FnMut() -> bool) {
    let t0 = std::time::Instant::now();
    while !f() {
        assert!(t0.elapsed() < Duration::from_secs(20), "no {what} in 20 s");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// The brief's core test: a stand-in model calls `mcp__fake__echo` and gets
/// a result node (outside text), a notice (the call ran at notify), and
/// T1's hold on its session. The board's start and ready are ledgered, and
/// health and `mcp.list` show the server and its tools.
#[tokio::test]
async fn a_model_calls_an_mcp_tool_and_gets_its_result_a_notice_and_the_hold() {
    let r = rig(Mode::Ok);
    r.core.mcp.start();
    let s = r.core.mcp.server("fake").unwrap().clone();
    assert!(s.client(Duration::from_secs(10)).await.is_ok());
    let sid = session(&r.core, None);
    let res = turn(&r.core, &sid, "echo something").await;
    assert!(res.awaiting_confirm.is_none(), "{res:?}");
    // Offered after the built-ins, by wire name.
    let reqs = r.model.requests.lock().unwrap().clone();
    let names = offered(&reqs[0]);
    let first_mcp = names.iter().position(|n| n.starts_with("mcp__")).unwrap();
    assert!(
        names[first_mcp..].iter().all(|n| n.starts_with("mcp__")),
        "{names:?}"
    );
    assert!(names.contains(&"mcp__fake__echo".to_string()));
    let system: String = reqs[0]
        .system
        .iter()
        .filter_map(|b| b["text"].as_str())
        .collect();
    assert!(
        system.contains("MCP servers the operator attached (fake)"),
        "{system}"
    );
    // The result node, outside text.
    let (status, text, external) = result_of(&r.core, &sid, "m1");
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(text.contains("from the fake"), "{text}");
    assert!(external, "an MCP result is outside text");
    // The notice: it ran at notify.
    let notified = ledgered(&r.core, "tool.notified");
    assert_eq!(notified.len(), 1, "{notified:?}");
    assert_eq!(notified[0]["tool"], "mcp:fake/echo");
    // T1's hold.
    let rec: SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
    let held = rec.external.expect("the session holds");
    assert_eq!(
        (held.tool.as_str(), held.url.as_str()),
        ("mcp:fake/echo", "mcp:fake/echo")
    );
    // The board's rows, health, and mcp.list.
    until("the ready row", || {
        !ledgered(&r.core, "mcp.ready").is_empty()
    })
    .await;
    assert_eq!(ledgered(&r.core, "mcp.started")[0]["server"], "fake");
    let h = r.core.health();
    assert_eq!(h.mcp.len(), 1);
    assert_eq!((h.mcp[0].state.as_str(), h.mcp[0].calls), ("ready", 1));
    let list = r.core.mcp.list(&r.core.tools);
    let echo = list.tools.iter().find(|t| t.tool == "echo").unwrap();
    assert_eq!(
        (echo.class.as_str(), echo.posture.as_str(), echo.calls),
        ("run", "notify", 1)
    );
    assert_eq!(echo.hints, ["read-only"]);
    // The stored list, for the next start.
    until("the stored list", || {
        super::read_stored(&r.core.store, "fake").is_some()
    })
    .await;
    r.core.mcp.stop();
}

/// The place rule: a shared place's turn is offered no MCP tool, and a call
/// its model makes anyway is refused at the gate and never reaches the
/// server.
#[tokio::test]
async fn a_shared_places_turn_is_offered_no_mcp_tool() {
    let r = rig(Mode::Ok);
    r.core.mcp.start();
    let s = r.core.mcp.server("fake").unwrap().clone();
    assert!(s.client(Duration::from_secs(10)).await.is_ok());
    let private = session(&r.core, None);
    let shared = session(&r.core, Some("channel:4242"));
    turn(&r.core, &shared, "echo in the lab").await;
    let reqs = r.model.requests.lock().unwrap().clone();
    let names = offered(&reqs[0]);
    assert!(
        !names.iter().any(|n| n.starts_with("mcp__")),
        "a shared place is offered no MCP tool: {names:?}"
    );
    let (status, text, _) = result_of(&r.core, &shared, "m1");
    assert_ne!(status, ResultStatus::Ok);
    assert!(
        text.starts_with("Not run: mcp:fake/echo is not offered in a shared place"),
        "{text}"
    );
    assert_eq!(s.status().calls, 0, "nothing reached the server");
    // A private place, on the same core, is offered them.
    turn(&r.core, &private, "echo here").await;
    let reqs = r.model.requests.lock().unwrap().clone();
    assert!(offered(reqs.last().unwrap()).contains(&"mcp__fake__echo".to_string()));
    r.core.mcp.stop();
}

/// `list_changed` applies at the next turn: the fake changes its list at
/// each call. The turn that called keeps the tools it began with; the next
/// turn is offered the new list; the change is ledgered as
/// `mcp.tools_changed`.
#[tokio::test]
async fn a_changed_list_applies_at_the_next_turn_with_its_row() {
    let r = rig(Mode::ChangeTools);
    r.core.mcp.start();
    r.core
        .mcp
        .server("fake")
        .unwrap()
        .client(Duration::from_secs(10))
        .await
        .map(|_| ())
        .unwrap();
    let sid = session(&r.core, None);
    turn(&r.core, &sid, "echo once").await;
    until("the change", || {
        !ledgered(&r.core, "mcp.tools_changed").is_empty()
    })
    .await;
    let reqs = r.model.requests.lock().unwrap().clone();
    assert_eq!(reqs.len(), 2, "the call's loop and the answer's");
    assert_eq!(
        offered(&reqs[0]),
        offered(&reqs[1]),
        "a turn keeps the tools it began with"
    );
    assert!(!offered(&reqs[1]).contains(&"mcp__fake__tool_v1".to_string()));
    turn(&r.core, &sid, "echo twice").await;
    let reqs = r.model.requests.lock().unwrap().clone();
    assert!(
        offered(&reqs[2]).contains(&"mcp__fake__tool_v1".to_string()),
        "the next turn is offered the new list"
    );
    let changed = ledgered(&r.core, "mcp.tools_changed");
    assert_eq!(changed[0]["added"], json!(["tool_v1"]));
    assert_eq!(changed[0]["changed"], json!(["echo"]));
    // And the operator's notice, in the outbox.
    until("the notice", || {
        r.core
            .outbox
            .open_for(crate::outbox::OPERATOR_TARGET)
            .iter()
            .any(|a| crate::outbox::kind_of(a) == "mcp_changed")
    })
    .await;
    r.core.mcp.stop();
}

/// An error result is a failure the model reads, and still outside text.
#[tokio::test]
async fn an_error_result_is_a_failure_and_still_outside_text() {
    let r = rig(Mode::Error);
    r.core.mcp.start();
    r.core
        .mcp
        .server("fake")
        .unwrap()
        .client(Duration::from_secs(10))
        .await
        .map(|_| ())
        .unwrap();
    let sid = session(&r.core, None);
    turn(&r.core, &sid, "echo an error").await;
    let (status, text, external) = result_of(&r.core, &sid, "m1");
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(text.contains("answered an error"), "{text}");
    assert!(external, "the server's error is its text too");
    r.core.mcp.stop();
}

/// A call to a server that does not come up within its start timeout
/// fails `mcp_unavailable`, saying nothing was sent: not unknown, and not
/// outside text, since the server said nothing.
#[tokio::test]
async fn a_call_to_a_server_that_never_comes_up_is_unavailable() {
    // The stored list offers the tool; the server never answers.
    let stored = StoredList {
        digest: "stored".into(),
        tools: vec![serde_json::from_value(json!({
            "name": "echo", "description": "Echoes its text.",
            "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}}
        }))
        .unwrap()],
    };
    let r = rig_with(
        Mode::Ok,
        McpServerConfig {
            start_timeout_secs: 1,
            ..server_cfg(&[])
        },
        Some(stored),
    );
    let down = InProcess::with(&[("fake", Mode::Ok)]);
    down.set_down("fake", true);
    r.core.mcp.set_connect(down);
    r.core.mcp.start();
    let sid = session(&r.core, None);
    turn(&r.core, &sid, "echo into the void").await;
    let (status, text, external) = result_of(&r.core, &sid, "m1");
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(
        text.starts_with("mcp_unavailable: MCP server fake"),
        "{text}"
    );
    assert!(text.contains("Nothing was sent"), "{text}");
    assert!(!external, "the server said nothing");
    r.core.mcp.stop();
}
