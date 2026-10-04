//! `extend.propose` through the whole core (M7 43a), with the fake MCP
//! server in this process standing in for the frozen copy in L1: the board's
//! `Connect` reads the copy it is given (`frozen`) and serves the fake whose
//! mode its `mode` file names, so what runs is what that directory holds.
//! - a proposal is frozen under its digest, tried, and put to the operator:
//!   its manifest records its tools and each test's pass or failure, with
//!   `extend.proposed` and `extend.tested`, and its question waits;
//! - the digest is stable, and an edit in the workspace after the freeze
//!   changes nothing that runs;
//! - no `mcp__ext-` tool is offered before the ack (after it, 43b's
//!   `tests_load.rs`);
//! - an ack from a shared place is refused, and the question keeps waiting;
//!   the owner's ack from a private place writes `extend.acked`;
//! - a decline, and a question nobody answered, load nothing and wake
//!   nothing.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::future::BoxFuture;
use serde_json::{json, Value};
use theseus_mcp::fake::{self, Fake, Mode};
use theseus_mcp::{Client, Transport};
use theseus_protocol::SessionKind;

use crate::bus::EventSink;
use crate::config::McpServerConfig;
use crate::mcp::{Connect, Connected, McpCatalog};
use crate::node::{Body, ResultStatus};
use crate::policy::Posture;
use crate::provider::{
    DeltaSink, FakeProvider, Provider, ProviderFuture, ProviderRequest, Scripted,
};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

const OWNER: u64 = 4_100_000_000_000_000_001;
const LAB: u64 = 4_100_000_000_000_000_777;

/// The frozen copy, as the board would start it: the fake its `mode` file
/// names, from the directory the board is given. Each start is kept, with
/// what the catalog offered at that moment; `edit` is written into the
/// workspace just before a start, as an edit after the freeze would be.
pub(super) struct FromDir {
    pub(super) catalog: Mutex<Option<Arc<McpCatalog>>>,
    pub(super) starts: Mutex<Vec<(String, McpServerConfig, Vec<String>)>>,
    pub(super) edit: Mutex<Option<(PathBuf, String)>>,
    /// Each fake's serving task: finished once its connection closes.
    pub(super) served: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>,
}

impl FromDir {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            catalog: Mutex::default(),
            starts: Mutex::default(),
            edit: Mutex::default(),
            served: Arc::default(),
        })
    }
}

impl Connect for FromDir {
    fn connect(
        &self,
        server: &str,
        cfg: &McpServerConfig,
        _env: Vec<(String, String)>,
        _bearer: Option<String>,
    ) -> BoxFuture<'static, Result<Connected, String>> {
        if let Some((p, text)) = self.edit.lock().unwrap().take() {
            std::fs::write(p, text).unwrap();
        }
        let offered = self
            .catalog
            .lock()
            .unwrap()
            .as_ref()
            .map(|c| c.all().iter().map(|t| t.wire.clone()).collect())
            .unwrap_or_default();
        self.starts
            .lock()
            .unwrap()
            .push((server.to_string(), cfg.clone(), offered));
        let mode = cfg
            .frozen
            .as_ref()
            .and_then(|d| std::fs::read_to_string(d.join("mode")).ok())
            .and_then(|m| Mode::parse(&m));
        let name = server.to_string();
        let served = self.served.clone();
        let hangs = cfg
            .frozen
            .as_ref()
            .and_then(|d| std::fs::read_to_string(d.join("mode")).ok())
            .is_some_and(|m| m == "hang");
        Box::pin(async move {
            if hangs {
                // A server that never answers its handshake.
                std::future::pending::<()>().await;
            }
            let mode = mode.ok_or("the server exited with status 2")?;
            let fake = Fake::new(fake::Config {
                mode,
                name,
                ..Default::default()
            });
            let (ours, theirs) = tokio::io::duplex(1 << 16);
            let (r, w) = tokio::io::split(theirs);
            served.lock().unwrap().push(tokio::spawn(async move {
                let _ = fake.serve_pipes(r, w).await;
            }));
            let (r, w) = tokio::io::split(ours);
            let (client, events) = Client::connect(Transport::pipes(r, w), Default::default())
                .await
                .map_err(|e| e.to_string())?;
            Ok(Connected { client, events })
        })
    }
}

/// Answers each turn's first request with the calls its input names, and a
/// call's result with a line of text.
struct Model {
    calls: Mutex<Vec<(String, Value)>>,
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
        let answers_a_call = req
            .messages
            .last()
            .and_then(|m| m["content"].as_array())
            .is_some_and(|c| c.iter().any(|b| b["type"] == "tool_result"));
        let answer = match self.calls.lock().unwrap().pop() {
            Some((tool, input)) if !answers_a_call => {
                Scripted::tools("", &[("p1", tool.as_str(), input)])
            }
            _ => Scripted::text("Done."),
        };
        Box::pin(async move {
            FakeProvider::scripted(vec![answer])
                .stream_message(req, on_delta)
                .await
        })
    }
}

struct Rig {
    core: Arc<Core>,
    model: Arc<Model>,
    connect: Arc<FromDir>,
    root: PathBuf,
    _dir: tempfile::TempDir,
}

fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = Posture::Notify;
    cfg.places.owner = Some(vec![format!("discord:{OWNER}")]);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let model = Arc::new(Model {
        calls: Mutex::default(),
        requests: Mutex::default(),
    });
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, model.clone(), store)).unwrap();
    let connect = FromDir::new();
    *connect.catalog.lock().unwrap() = Some(core.tools.mcp.clone());
    core.mcp.set_connect(connect.clone());
    Rig {
        core,
        model,
        connect,
        root,
        _dir: dir,
    }
}

impl Drop for Rig {
    /// The frozen copies are read-only: writable again, so the temp dir goes.
    fn drop(&mut self) {
        let _ = super::freeze::make_writable(&self.core.tools.extend.dir);
    }
}

impl Rig {
    /// A word counter's tree in the workspace, serving in `mode`.
    fn server(&self, at: &str, mode: &str) -> PathBuf {
        let d = self.root.join(at);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("mode"), mode).unwrap();
        std::fs::write(d.join("server.py"), "# counts words\n").unwrap();
        d
    }

    fn session(&self) -> String {
        let r = SessionRecord::new(SessionKind::Conversation, None);
        self.core.store.put_session(&r.session_id, &r).unwrap();
        r.session_id
    }

    async fn turn(&self, sid: &str, input: &str, call: Option<Value>) {
        if let Some(c) = call {
            self.model
                .calls
                .lock()
                .unwrap()
                .push(("extend_propose".into(), c));
        }
        let rec: SessionRecord = self.core.store.get_session(sid).unwrap().unwrap();
        let (live, _) = self.core.live_profile();
        let target = self
            .core
            .runner
            .resolve_target(&live, None, None, None)
            .unwrap();
        self.core
            .runner
            .run(TurnRequest {
                prompt: None,
                session: rec,
                input: Some(input.into()),
                target,
                sink: EventSink::new(self.core.bus.clone(), sid, None),
                author: "test".into(),
                recompile: None,
                attachments: vec![],
                arrived: None,
                reply_to: None,
            })
            .await
            .unwrap();
    }

    fn rows(&self, kind: &str) -> Vec<Value> {
        self.core
            .store
            .ledger_tail::<crate::ledger::LedgerRow>(100_000)
            .unwrap()
            .into_iter()
            .filter(|(_, r)| r.kind == kind)
            .map(|(_, r)| r.data)
            .collect()
    }

    fn result(&self, sid: &str) -> (ResultStatus, String) {
        self.core
            .store
            .session_nodes(sid)
            .unwrap()
            .into_iter()
            .find_map(|(_, n)| match &n.body {
                Body::ToolResult {
                    tool_use_id,
                    status,
                    content,
                    ..
                } if tool_use_id == "p1" => Some((*status, content.clone())),
                _ => None,
            })
            .expect("the proposal's result")
    }

    fn manifests(&self) -> Vec<super::Manifest> {
        super::manifests(&self.core.store).unwrap()
    }

    /// Every tool name offered across every request so far.
    fn offered(&self) -> Vec<String> {
        self.model
            .requests
            .lock()
            .unwrap()
            .iter()
            .flat_map(|r| {
                r.tools
                    .iter()
                    .filter_map(|t| t["name"].as_str().map(str::to_string))
            })
            .collect()
    }
}

pub(super) fn proposal(dir: &Path) -> Value {
    json!({
        "name": "wordcount",
        "dir": dir.display().to_string(),
        "command": ["python3", "server.py"],
        "description": "Counts the words of a text.",
        "tests": [
            {"tool": "echo", "arguments": {"text": "two words"}, "expect": {"contains": "two words"}},
            {"tool": "add", "arguments": {"a": 2, "b": 3}, "expect": {"contains": "5"}},
            {"tool": "echo", "arguments": {"text": "three"}, "expect": {"equals": "four"}}
        ]
    })
}

/// The brief's core test: the tree is frozen under its SHA-256, read-only;
/// the frozen copy, and only it, is started in L1 with no network and no
/// secret; its tools and each test's pass or failure are recorded in the
/// manifest, with the rows; and the operator is asked.
#[tokio::test]
async fn a_proposal_is_frozen_tried_in_l1_and_put_to_the_operator() {
    let r = rig();
    let src = r.server("tools/wc", "ok");
    let sid = r.session();
    r.turn(&sid, "propose the counter", Some(proposal(&src)))
        .await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    let ms = r.manifests();
    assert_eq!(ms.len(), 1);
    let m = &ms[0];
    assert_eq!(m.digest, super::freeze::digest(&src).unwrap());
    let frozen = r.core.tools.extend.dir.join("wordcount").join(&m.digest);
    assert_eq!(m.frozen, frozen.display().to_string());
    assert_eq!(super::freeze::digest(&frozen).unwrap(), m.digest);
    assert_eq!((m.files, m.state.as_str()), (2, "proposed"));
    assert_eq!(m.tools.len(), 5, "{:?}", m.tools);
    assert!(m
        .tools
        .iter()
        .any(|t| t.name == "echo" && t.input_schema.is_object()));
    let passed: Vec<bool> = m.tests.iter().map(|t| t.passed).collect();
    assert_eq!(passed, [true, true, false], "{:?}", m.tests);
    assert!(m.tests[2]
        .why
        .as_deref()
        .unwrap()
        .contains("is not \"four\""));
    assert_eq!(m.capabilities.network, Vec::<String>::new());
    assert_eq!(m.proposed_by.session_id, sid);
    assert_eq!(m.proposed_by.principal, "operator");
    // The frozen copy, in L1, with no network and no secret.
    let starts = r.connect.starts.lock().unwrap().clone();
    assert_eq!(starts.len(), 1);
    let (server, cfg, _) = &starts[0];
    assert_eq!(server, "ext-wordcount");
    assert_eq!(cfg.sandbox, crate::config::mcp::McpSandbox::L1);
    assert_eq!(cfg.frozen.as_deref(), Some(frozen.as_path()));
    assert!(cfg.egress.is_empty() && cfg.env.is_empty());
    // Its result says what it found, and what happens next.
    assert!(text.contains("2 of 3 tests passed"), "{text}");
    assert!(text.contains("- echo: FAILED"), "{text}");
    // The rows, and the question.
    let p = &r.rows("extend.proposed")[0];
    assert_eq!(
        (p["name"].as_str(), p["files"].as_u64()),
        (Some("wordcount"), Some(2))
    );
    let t = &r.rows("extend.tested")[0];
    assert_eq!(
        (t["passed"].as_u64(), t["tests"].as_u64()),
        (Some(2), Some(3)),
        "{t}"
    );
    let asks = r.core.confirm_list().unwrap();
    assert_eq!(asks.len(), 1, "{asks:?}");
    let q = &asks[0];
    assert_eq!(q.tool, super::ACK);
    assert_eq!(Some(&q.correlation_id), m.question.as_ref());
    assert_eq!(
        q.reason,
        format!(
            "Load wordcount {}: 5 tools, 2 of 3 tests passed, no network?",
            &m.digest[..6]
        )
    );
    assert_eq!(r.rows("tool.confirm_requested").len(), 1);
    // Nothing of it runs once its trial is over.
    assert!(r.core.mcp.status().is_empty(), "{:?}", r.core.mcp.status());
    // `extend.list` and health's count.
    let l = r.core.extend_list().unwrap();
    assert_eq!(l.extensions.len(), 1);
    let e = &l.extensions[0];
    assert_eq!((e.state.as_str(), e.passed, e.tests), ("proposed", 2, 3));
    assert_eq!(e.question, m.question);
    assert_eq!(e.tools.len(), 5);
    let h = r.core.health().extensions.expect("health counts proposals");
    assert_eq!((h.proposals, h.waiting, h.acked), (1, 1, 0));
}

/// The digest is stable: the same tree proposed again is the same copy and
/// digest. An edit in the workspace after the freeze, made before the
/// server starts, changes nothing that runs: the tests pass as frozen.
#[tokio::test]
async fn an_edit_after_proposing_changes_nothing_that_runs() {
    let r = rig();
    let src = r.server("wc", "ok");
    let sid = r.session();
    // The edit lands after the freeze, as the server starts.
    *r.connect.edit.lock().unwrap() = Some((src.join("mode"), "error".into()));
    r.turn(&sid, "propose", Some(proposal(&src))).await;
    let m = r.manifests().remove(0);
    let passed: Vec<bool> = m.tests.iter().map(|t| t.passed).collect();
    assert_eq!(
        passed,
        [true, true, false],
        "it ran as frozen: {:?}",
        m.tests
    );
    let frozen = PathBuf::from(&m.frozen);
    assert_eq!(std::fs::read_to_string(frozen.join("mode")).unwrap(), "ok");
    assert_eq!(super::freeze::digest(&frozen).unwrap(), m.digest);
    // The edited tree is another digest; put back, the same one again.
    assert_ne!(super::freeze::digest(&src).unwrap(), m.digest);
    std::fs::write(src.join("mode"), "ok").unwrap();
    let sid2 = r.session();
    r.turn(&sid2, "again", Some(proposal(&src))).await;
    let again: Vec<String> = r.manifests().iter().map(|m| m.digest.clone()).collect();
    assert_eq!(
        again,
        std::slice::from_ref(&m.digest),
        "the same tree, the same digest and copy"
    );
}

/// No `mcp__ext-` tool is offered before the ack: not while it is tried,
/// not at the next turn. Once acked, it loads (43b, `tests_load.rs`).
#[tokio::test]
async fn no_tool_is_offered_before_the_ack() {
    let r = rig();
    let src = r.server("wc", "ok");
    let sid = r.session();
    r.turn(&sid, "propose", Some(proposal(&src))).await;
    let starts = r.connect.starts.lock().unwrap().clone();
    assert!(
        starts[0].2.iter().all(|w| !w.starts_with("mcp__")),
        "offered while it was tried: {:?}",
        starts[0].2
    );
    r.turn(&sid, "what now?", None).await;
    let offered = r.offered();
    assert!(offered.iter().any(|t| t == "extend_propose"), "{offered:?}");
    assert!(
        offered.iter().all(|t| !t.starts_with("mcp__")),
        "an extension's tool was offered: {offered:?}"
    );
    let q = r.core.confirm_list().unwrap().remove(0);
    let done = r
        .core
        .confirm_action(&q.correlation_id, true, None, "cli")
        .unwrap();
    assert!(done.approved && !done.resumes);
    let acked = &r.rows("extend.acked")[0];
    assert_eq!(
        (acked["name"].as_str(), acked["by"].as_str()),
        (Some("wordcount"), Some("cli"))
    );
    assert_eq!(r.manifests()[0].state, "acked");
    let h = r.core.health().extensions.unwrap();
    assert_eq!((h.proposals, h.waiting, h.acked), (1, 0, 1));
    assert!(r.core.confirm_list().unwrap().is_empty());
}

/// An ack from a shared place is refused, ledgered, and the question keeps
/// waiting; the owner's ack from their DM counts.
#[tokio::test]
async fn an_ack_from_a_shared_place_is_refused() {
    let r = rig();
    let src = r.server("wc", "ok");
    let sid = r.session();
    r.turn(&sid, "propose", Some(proposal(&src))).await;
    let q = r.core.confirm_list().unwrap().remove(0);
    let from = |channel: Option<u64>| crate::approval::Answerer {
        label: format!("discord:{OWNER}"),
        surface: crate::approval::Surface::Discord,
        discord: Some(theseus_protocol::DiscordOrigin {
            user_id: OWNER.to_string(),
            channel_id: channel.unwrap_or(OWNER + 1).to_string(),
            guild_id: channel.map(|_| "900000000000000001".to_string()),
        }),
    };
    let e = r
        .core
        .confirm_action(&q.correlation_id, true, None, from(Some(LAB)))
        .unwrap_err();
    assert!(e.to_string().contains("shared"), "{e:#}");
    assert_eq!(r.rows("approval.refused").len(), 1);
    assert!(r.rows("extend.acked").is_empty());
    assert_eq!(r.core.confirm_list().unwrap().len(), 1, "it still waits");
    assert_eq!(r.manifests()[0].state, "proposed");
    // The owner's DM is private.
    r.core
        .confirm_action(&q.correlation_id, true, None, from(None))
        .unwrap();
    assert_eq!(r.rows("extend.acked")[0]["via"], "discord:dm");
}

/// A decline settles the question declined, loads nothing, and wakes
/// nothing: no turn runs after it. A question nobody answers expires the
/// same way.
#[tokio::test]
async fn a_decline_loads_nothing_and_neither_does_an_expiry() {
    let r = rig();
    let src = r.server("wc", "ok");
    let sid = r.session();
    r.turn(&sid, "propose", Some(proposal(&src))).await;
    let requests = r.model.requests.lock().unwrap().len();
    let q = r.core.confirm_list().unwrap().remove(0);
    let done = r
        .core
        .confirm_action(&q.correlation_id, false, Some("not today"), "cli")
        .unwrap();
    assert!(!done.approved && !done.resumes);
    let d = &r.rows("extend.declined")[0];
    assert_eq!(
        (d["by"].as_str(), d["note"].as_str()),
        (Some("cli"), Some("not today"))
    );
    let m = &r.manifests()[0];
    assert_eq!(
        (m.state.as_str(), m.note.as_deref()),
        ("declined", Some("not today"))
    );
    let a = r.core.kernel.action(&q.correlation_id).unwrap().unwrap();
    assert_eq!(a.state, theseus_kernel::ActionState::Cancelled);
    assert!(r.core.tools.mcp.is_empty());
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        r.model.requests.lock().unwrap().len(),
        requests,
        "no turn ran"
    );
    let e = r.core.kernel.execution(&a.execution_id).unwrap().unwrap();
    assert_eq!(e.state, theseus_kernel::ExecState::Waiting);

    // A second proposal, which nobody answers.
    let src2 = r.server("wc2", "ok");
    std::fs::write(src2.join("server.py"), "# counts words, twice\n").unwrap();
    let mut p = proposal(&src2);
    p["name"] = json!("wordcount2");
    r.turn(&sid, "propose again", Some(p)).await;
    let requests = r.model.requests.lock().unwrap().len();
    let q = r.core.confirm_list().unwrap().remove(0);
    let ttl = r.core.kernel.config().confirm_ttl_ms;
    assert_eq!(r.core.expire_questions(q.requested_at_ms + ttl + 1), 1);
    let d = r.rows("extend.declined");
    assert_eq!(d.len(), 2);
    assert_eq!(d[1]["by"], "expiry");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        r.model.requests.lock().unwrap().len(),
        requests,
        "no turn ran"
    );
}

/// A tree outside the workspace, or one holding a link, is refused before
/// anything runs; a server that does not come up is recorded, and nothing
/// is asked.
#[tokio::test]
async fn what_cannot_be_tried_is_said_and_nothing_is_asked() {
    let r = rig();
    assert!(r.core.health().extensions.is_none(), "none proposed yet");
    let sid = r.session();
    let outside = r._dir.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    let mut p = proposal(&outside);
    r.turn(&sid, "propose", Some(p.clone())).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Error);
    assert!(text.contains("outside the workspace roots"), "{text}");
    // A server that exits at its start.
    let src = r.server("broken", "no such mode");
    p["dir"] = json!(src.display().to_string());
    let sid = r.session();
    r.turn(&sid, "propose", Some(p)).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(text.contains("did not come up in L1"), "{text}");
    let m = &r.manifests()[0];
    assert_eq!(m.state, "failed");
    assert!(m.question.is_none());
    assert!(m.tests.iter().all(|t| !t.passed));
    assert!(r.core.confirm_list().unwrap().is_empty());
    assert!(r.rows("extend.tested")[0]["error"].is_string());
}

/// A `/stop` that lands while the server is tried ends the trial: the call
/// answers stopped, nothing is asked, and nothing of it runs on.
#[tokio::test]
async fn a_stop_during_the_trial_ends_it_and_asks_nothing() {
    let r = Arc::new(rig());
    let src = r.server("wc", "hang");
    let sid = r.session();
    let turn = {
        let (r, sid, p) = (r.clone(), sid.clone(), proposal(&src));
        tokio::spawn(async move { r.turn(&sid, "propose", Some(p)).await })
    };
    let t0 = std::time::Instant::now();
    while r.core.mcp.status().iter().all(|s| s.state != "proposed") {
        assert!(t0.elapsed() < Duration::from_secs(10), "no trial");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let rec: SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
    let exec = rec.execution_id.unwrap();
    r.core.stop_execution(&exec, "cli").await.unwrap();
    tokio::time::timeout(Duration::from_secs(10), turn)
        .await
        .expect("the turn ended")
        .unwrap();
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Cancelled, "{text}");
    assert!(text.contains("stopped by cli"), "{text}");
    assert!(r.core.mcp.status().is_empty(), "{:?}", r.core.mcp.status());
    assert!(r.core.confirm_list().unwrap().is_empty());
    assert!(r.rows("extend.tested").is_empty());
}
