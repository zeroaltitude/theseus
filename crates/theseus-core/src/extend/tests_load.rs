//! An acked extension loaded, restarted, and revoked (M7 43b), through the
//! whole core, with 43a's stand-in for the frozen copy in L1 (`FromDir`:
//! the fake whose mode the directory it is given names):
//! - its tools are offered from the next turn's start, never mid-turn;
//! - a restart offers its stored list at once and starts it after serving,
//!   from the frozen copy, never the workspace;
//! - a revoke drops its tools and stops its server, and only the owner, from
//!   a private place, revokes;
//! - a second version replaces the first only once acked;
//! - a shared place's turn is never offered its tools;
//! - its posture is notify unless `[policy.mcp]` says otherwise, and the
//!   floor of the ceiling it was loaded under holds its calls.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use serde_json::{json, Value};
use theseus_protocol::extend::ExtensionRevokeParams;
use theseus_protocol::SessionKind;

use super::tests::{proposal, FromDir};
use crate::bus::EventSink;
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
const TOOL: &str = "mcp__ext-wordcount__echo";

type Hook = Box<dyn FnOnce(&Core) + Send>;

/// One request's answer: a hook run first (with the core), then a call or,
/// with none, "Done.".
struct Step {
    hook: Option<Hook>,
    call: Option<(String, Value)>,
}

/// Answers each request with its next step, and keeps every request.
#[derive(Default)]
struct Model {
    steps: Mutex<VecDeque<Step>>,
    requests: Mutex<Vec<ProviderRequest>>,
    core: OnceLock<Weak<Core>>,
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
        let step = self.steps.lock().unwrap().pop_front();
        let answer = match step {
            Some(Step { hook, call }) => {
                if let (Some(h), Some(core)) = (hook, self.core.get().and_then(Weak::upgrade)) {
                    h(&core);
                }
                match call {
                    Some((tool, input)) => Scripted::tools("", &[("c1", tool.as_str(), input)]),
                    None => Scripted::text("Done."),
                }
            }
            None => Scripted::text("Done."),
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
    writable: Writable,
    dir: tempfile::TempDir,
}

/// The frozen copies are read-only: writable again, so the temp dir goes.
struct Writable(PathBuf);

impl Drop for Writable {
    fn drop(&mut self) {
        let _ = super::freeze::make_writable(&self.0);
    }
}

fn config(dir: &Path, root: &Path) -> Config {
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = Posture::Open;
    cfg.places.owner = Some(vec![format!("discord:{OWNER}")]);
    cfg
}

/// A core on `dir`'s store, its board reaching servers through `connect`.
fn core_on(
    dir: &Path,
    root: &Path,
    cfg: Config,
    connect: &Arc<FromDir>,
) -> (Arc<Core>, Arc<Model>) {
    let store = Store::open(&dir.join("store")).unwrap();
    let model = Arc::new(Model::default());
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, model.clone(), store)).unwrap();
    let _ = model.core.set(Arc::downgrade(&core));
    *connect.catalog.lock().unwrap() = Some(core.tools.mcp.clone());
    core.mcp.set_connect(connect.clone());
    let _ = root;
    (core, model)
}

fn rig_with(f: impl FnOnce(&mut Config)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let mut cfg = config(dir.path(), &root);
    f(&mut cfg);
    let connect = FromDir::new();
    let (core, model) = core_on(dir.path(), &root, cfg, &connect);
    Rig {
        writable: Writable(core.tools.extend.dir.clone()),
        core,
        model,
        connect,
        root,
        dir,
    }
}

fn rig() -> Rig {
    rig_with(|_| {})
}

impl Rig {
    fn server(&self, at: &str, mode: &str, text: &str) -> PathBuf {
        let d = self.root.join(at);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("mode"), mode).unwrap();
        std::fs::write(d.join("server.py"), text).unwrap();
        d
    }

    fn session(&self, place: Option<&str>) -> String {
        let r = SessionRecord::new(SessionKind::Conversation, None);
        self.core.store.put_session(&r.session_id, &r).unwrap();
        if let Some(p) = place {
            self.core.outbox.bind_place(p, &r.session_id).unwrap();
        }
        r.session_id
    }

    fn script(&self, steps: Vec<Step>) {
        self.model.steps.lock().unwrap().extend(steps);
    }

    async fn turn(&self, sid: &str, input: &str) {
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

    /// Propose `dir` as `wordcount` in a new session: the question's id.
    async fn propose(&self, dir: &Path) -> String {
        let sid = self.session(None);
        self.script(vec![call("extend_propose", proposal(dir)), text()]);
        self.turn(&sid, "propose the counter").await;
        let q = self.core.confirm_list().unwrap();
        assert_eq!(q.len(), 1, "{q:?}");
        q[0].correlation_id.clone()
    }

    fn ack(&self, q: &str) {
        let done = self.core.confirm_action(q, true, None, "cli").unwrap();
        assert!(done.approved && !done.resumes);
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

    /// The tool names each request of the turns since `from` offered.
    fn offered_since(&self, from: usize) -> Vec<Vec<String>> {
        self.model.requests.lock().unwrap()[from..]
            .iter()
            .map(|r| {
                r.tools
                    .iter()
                    .filter_map(|t| t["name"].as_str().map(str::to_string))
                    .collect()
            })
            .collect()
    }

    fn requests(&self) -> usize {
        self.model.requests.lock().unwrap().len()
    }

    fn catalog(&self) -> Vec<String> {
        self.core
            .tools
            .mcp
            .all()
            .iter()
            .map(|t| t.wire.clone())
            .collect()
    }

    /// The result of the last call `c1` in `sid`.
    fn result(&self, sid: &str) -> (ResultStatus, String) {
        self.core
            .store
            .session_nodes(sid)
            .unwrap()
            .into_iter()
            .rev()
            .find_map(|(_, n)| match &n.body {
                Body::ToolResult {
                    tool_use_id,
                    status,
                    content,
                    ..
                } if tool_use_id == "c1" => Some((*status, content.clone())),
                _ => None,
            })
            .expect("the call's result")
    }

    fn state(&self, server: &str) -> Option<String> {
        self.core
            .mcp
            .status()
            .into_iter()
            .find(|s| s.name == server)
            .map(|s| s.state)
    }

    async fn until_ready(&self) {
        let t0 = std::time::Instant::now();
        while self.state("ext-wordcount").as_deref() != Some("ready") {
            assert!(
                t0.elapsed() < Duration::from_secs(10),
                "not ready: {:?}",
                self.core.mcp.status()
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    /// Each start of the loaded `ext-wordcount` so far, its trials left
    /// out: the directory it ran from.
    fn ext_starts(&self) -> Vec<Option<PathBuf>> {
        self.connect
            .starts
            .lock()
            .unwrap()
            .iter()
            // A trial's calls have its shorter timeout.
            .filter(|(s, c, _)| {
                s == "ext-wordcount" && c.call_timeout_secs != super::CALL_TIMEOUT_SECS
            })
            .map(|(_, c, _)| c.frozen.clone())
            .collect()
    }
}

fn call(tool: &str, input: Value) -> Step {
    Step {
        hook: None,
        call: Some((tool.into(), input)),
    }
}

fn text() -> Step {
    Step {
        hook: None,
        call: None,
    }
}

const WC: &str = "# counts words\n";

/// The brief's first test: acked while a turn runs, its tools are in the
/// catalog at once, but that turn's later loops are not offered them; the
/// next turn is, and its call runs on the frozen copy in L1, at notify, its
/// result not outside text (no network).
#[tokio::test]
async fn an_acked_extension_is_offered_from_the_next_turn_never_mid_turn() {
    let r = rig();
    r.core.mcp.start();
    let src = r.server("tools/wc", "ok", WC);
    let q = r.propose(&src).await;
    // A turn whose first model call is answered after the ack lands, and
    // whose second loop comes after the load.
    let sid = r.session(None);
    let from = r.requests();
    let q2 = q.clone();
    r.script(vec![
        Step {
            hook: Some(Box::new(move |core: &Core| {
                core.confirm_action(&q2, true, None, "cli").unwrap();
            })),
            call: Some(("fs_glob".into(), json!({"pattern": "*"}))),
        },
        text(),
    ]);
    r.turn(&sid, "look around").await;
    assert!(r.catalog().iter().any(|t| t == TOOL), "{:?}", r.catalog());
    let mid = r.offered_since(from);
    assert_eq!(mid.len(), 2, "two loops");
    assert!(
        mid.iter().flatten().all(|t| !t.starts_with("mcp__")),
        "offered mid-turn: {mid:?}"
    );
    // The rows: acked and loaded, in one frame with the record.
    let loaded = &r.rows("extend.loaded")[0];
    assert_eq!(
        (loaded["name"].as_str(), loaded["server"].as_str()),
        (Some("wordcount"), Some("ext-wordcount"))
    );
    assert!(loaded["replaced"].is_null(), "{loaded}");
    let set = super::load::LoadedSet::read(&r.core.store).unwrap();
    let l = &set.loaded["wordcount"];
    assert_eq!(
        (l.acked_by.as_str(), l.question.as_str()),
        ("cli", q.as_str())
    );
    assert_eq!(l.tools.len(), 5);
    // The next turn is offered it, and its call runs.
    let from = r.requests();
    r.script(vec![
        call(TOOL, json!({"text": "three words here"})),
        text(),
    ]);
    r.turn(&sid, "count them").await;
    let next = r.offered_since(from);
    assert!(next[0].iter().any(|t| t == TOOL), "{:?}", next[0]);
    let (status, said) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{said}");
    assert!(said.contains("three words here"), "{said}");
    // From the frozen copy, in L1, no network, no secret, not external.
    let m = &super::manifests(&r.core.store).unwrap()[0];
    let starts = r.connect.starts.lock().unwrap().clone();
    let (_, cfg, _) = starts
        .iter()
        .rfind(|(s, _, _)| s == "ext-wordcount")
        .unwrap();
    assert_eq!(cfg.frozen.as_deref(), Some(Path::new(&m.frozen)));
    assert_eq!(cfg.sandbox, crate::config::mcp::McpSandbox::L1);
    assert!(cfg.egress.is_empty() && cfg.env.is_empty() && !cfg.external);
    // At notify, the extension's default, though the config runs tools open.
    let now = r.core.tools.posture_now("mcp:ext-wordcount/echo");
    assert_eq!(now.posture, Posture::Notify, "{}", now.setting);
    assert!(
        now.setting.contains("an extension's default"),
        "{}",
        now.setting
    );
    assert!(
        crate::external::held(&r.core.store, &sid)
            .unwrap()
            .is_none(),
        "no network: not outside text"
    );
    // The surfaces.
    let list = r.core.extend_list().unwrap();
    assert_eq!(list.loaded.len(), 1);
    assert!(list.loaded[0]
        .tools
        .contains(&"mcp:ext-wordcount/add".to_string()));
    assert_eq!(list.loaded[0].state, "ready");
    assert_eq!(list.loaded[0].calls, 1);
    assert_eq!(r.core.health().extensions.unwrap().loaded, 1);
    assert!(r
        .core
        .health()
        .mcp
        .iter()
        .any(|s| s.name == "ext-wordcount"));
}

/// The brief's second test: after a restart the stored list is offered at
/// once, nothing starts before serving, and it starts after serving from the
/// frozen copy, though the workspace's tree was edited since.
#[tokio::test]
async fn a_restart_starts_it_after_serving_from_the_frozen_copy() {
    let r = rig();
    let src = r.server("tools/wc", "ok", WC);
    let q = r.propose(&src).await;
    r.ack(&q);
    let frozen = PathBuf::from(&super::manifests(&r.core.store).unwrap()[0].frozen);
    // Not started: the board had not begun.
    assert!(r.ext_starts().is_empty());
    // The workspace's tree changes: nothing that runs may see it.
    std::fs::write(src.join("mode"), "error").unwrap();
    // A restart: the same store, a new core.
    let Rig {
        core,
        model,
        connect,
        root,
        writable,
        dir,
    } = r;
    let cfg = (*core.cfg).clone();
    drop(core);
    drop(model);
    let (core, model) = core_on(dir.path(), &root, cfg, &connect);
    let r = Rig {
        core,
        model,
        connect,
        root,
        writable,
        dir,
    };
    assert!(r.catalog().iter().any(|t| t == TOOL), "{:?}", r.catalog());
    assert!(r.ext_starts().is_empty(), "nothing starts before serving");
    assert_eq!(r.state("ext-wordcount").as_deref(), Some("stopped"));
    r.core.mcp.start();
    r.until_ready().await;
    assert_eq!(r.ext_starts(), [Some(frozen.clone())]);
    let sid = r.session(None);
    r.script(vec![call(TOOL, json!({"text": "as frozen"})), text()]);
    r.turn(&sid, "count").await;
    let (status, said) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "it ran as frozen: {said}");
    assert!(said.contains("as frozen"), "{said}");
}

/// The brief's third test: a revoke from a shared place is refused and
/// changes nothing; the owner's drops its tools from the catalog and the
/// next turn, stops its server (its connection closes), and keeps the
/// frozen copy.
#[tokio::test]
async fn a_revoke_drops_its_tools_and_stops_its_server() {
    let r = rig();
    r.core.mcp.start();
    let src = r.server("tools/wc", "ok", WC);
    let q = r.propose(&src).await;
    r.ack(&q);
    r.until_ready().await;
    let served = r.connect.served.lock().unwrap().len();
    assert!(served >= 2, "its trial and its load");
    let p = ExtensionRevokeParams {
        name: "wordcount".into(),
        ..Default::default()
    };
    // From a shared place: refused, and it stays loaded.
    let shared = crate::approval::Answerer {
        label: format!("discord:{OWNER}"),
        surface: crate::approval::Surface::Discord,
        discord: Some(theseus_protocol::DiscordOrigin {
            user_id: OWNER.to_string(),
            channel_id: LAB.to_string(),
            guild_id: Some("900000000000000001".into()),
        }),
    };
    let e = r.core.extension_revoke(&p, shared).unwrap_err();
    assert!(e.to_string().contains("shared"), "{e:#}");
    let refused = r.rows("approval.refused");
    assert_eq!(refused.len(), 1);
    assert_eq!(refused[0]["act"], "extension.revoke");
    assert!(r.rows("extend.revoked").is_empty());
    assert!(r.catalog().iter().any(|t| t == TOOL));
    assert_eq!(r.state("ext-wordcount").as_deref(), Some("ready"));
    // The owner's, from the CLI.
    let done = r.core.extension_revoke(&p, "cli").unwrap();
    assert_eq!(done.tools.len(), 5);
    let row = &r.rows("extend.revoked")[0];
    assert_eq!(
        (row["name"].as_str(), row["by"].as_str()),
        (Some("wordcount"), Some("cli"))
    );
    assert!(r.catalog().is_empty(), "{:?}", r.catalog());
    assert_eq!(r.state("ext-wordcount"), None, "off the board");
    // Its connection closed: the fake that served its load has ended.
    let last = r.connect.served.lock().unwrap().pop().unwrap();
    tokio::time::timeout(Duration::from_secs(5), last)
        .await
        .expect("its server stopped")
        .unwrap();
    assert!(Path::new(&done.frozen).is_dir(), "the frozen copy stays");
    let m = &super::manifests(&r.core.store).unwrap()[0];
    assert_eq!(m.state, "revoked");
    assert!(r.core.extend_list().unwrap().loaded.is_empty());
    assert_eq!(r.core.health().extensions.unwrap().loaded, 0);
    // The next turn is not offered it, and a second revoke says why not.
    let sid = r.session(None);
    let from = r.requests();
    r.turn(&sid, "anything?").await;
    assert!(r.offered_since(from)[0]
        .iter()
        .all(|t| !t.starts_with("mcp__")));
    let e = r.core.extension_revoke(&p, "cli").unwrap_err();
    assert!(e.to_string().contains("no extension named"), "{e:#}");
}

/// The brief's fourth test: a second version of a loaded name is a new
/// proposal; until its ack, the first keeps running; its ack loads it in
/// the first's place, and `extend.loaded` names the digest it replaced.
#[tokio::test]
async fn a_second_version_replaces_the_first_only_once_acked() {
    let r = rig();
    r.core.mcp.start();
    let v1 = r.server("tools/wc", "ok", WC);
    let q1 = r.propose(&v1).await;
    r.ack(&q1);
    r.until_ready().await;
    let d1 = super::manifests(&r.core.store).unwrap()[0].digest.clone();
    let v2 = r.server("tools/wc2", "ok", "# counts words, faster\n");
    let q2 = r.propose(&v2).await;
    let d2 = super::manifests(&r.core.store).unwrap()[0].digest.clone();
    assert_ne!(d1, d2);
    // Proposed, not acked: the first still runs.
    let l = r.core.extend_list().unwrap().loaded;
    assert_eq!((l.len(), l[0].digest.as_str()), (1, d1.as_str()));
    let starts = r.ext_starts();
    assert_eq!(starts.len(), 1, "{starts:?}");
    assert!(starts[0].as_ref().unwrap().ends_with(&d1));
    // Acked: the second in its place.
    r.ack(&q2);
    r.until_ready().await;
    let rows = r.rows("extend.loaded");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1]["replaced"], d1.as_str());
    assert_eq!(rows[1]["digest"], d2.as_str());
    let l = r.core.extend_list().unwrap().loaded;
    assert_eq!((l.len(), l[0].digest.as_str()), (1, d2.as_str()));
    assert_eq!(l[0].replaced.as_deref(), Some(d1.as_str()));
    let starts = r.ext_starts();
    assert!(starts.last().unwrap().as_ref().unwrap().ends_with(&d2));
    let states: Vec<(String, String)> = super::manifests(&r.core.store)
        .unwrap()
        .into_iter()
        .map(|m| (m.digest, m.state))
        .collect();
    assert!(
        states.contains(&(d1.clone(), "replaced".into())),
        "{states:?}"
    );
    assert!(states.contains(&(d2.clone(), "acked".into())), "{states:?}");
    let statuses: Vec<_> = r.core.mcp.status().into_iter().map(|s| s.name).collect();
    assert_eq!(statuses, ["ext-wordcount"], "one server of the name");
}

/// A shared place's turn is never offered an extension's tool, and the
/// gate refuses a call of one there; a private session's turn is offered it.
#[tokio::test]
async fn a_shared_places_turn_is_never_offered_an_extensions_tool() {
    let r = rig();
    r.core.mcp.start();
    let src = r.server("tools/wc", "ok", WC);
    let q = r.propose(&src).await;
    r.ack(&q);
    r.core.bind_places(vec![crate::places::BoundPlace {
        target: format!("discord:channel:{LAB}"),
        name: "#lab".into(),
        private: false,
        ..Default::default()
    }]);
    let lab = r.session(Some(&format!("channel:{LAB}")));
    let from = r.requests();
    r.script(vec![call(TOOL, json!({"text": "leak"})), text()]);
    r.turn(&lab, "count").await;
    let offered = r.offered_since(from);
    assert!(
        offered.iter().flatten().all(|t| !t.starts_with("mcp__")),
        "{offered:?}"
    );
    let (status, said) = r.result(&lab);
    assert_ne!(status, ResultStatus::Ok, "{said}");
    let mine = r.session(None);
    let from = r.requests();
    r.turn(&mine, "anything?").await;
    assert!(r.offered_since(from)[0].iter().any(|t| t == TOOL));
}

/// `[policy.mcp] "ext-<name>"` sets its posture; and the ceiling of the
/// place it was proposed in, recorded with the load, floors its calls
/// wherever they come from.
#[tokio::test]
async fn its_posture_is_the_policy_lines_and_its_loads_ceiling_floors_it() {
    let r = rig_with(|c| {
        c.policy.mcp.insert("ext-wordcount".into(), Posture::Open);
    });
    let now = r.core.tools.posture_now("mcp:ext-wordcount/echo");
    assert_eq!(now.posture, Posture::Open, "{}", now.setting);
    let l = super::load::Loaded {
        name: "wordcount".into(),
        digest: "3f2a1c".into(),
        description: String::new(),
        command: vec![],
        frozen: "/frozen".into(),
        source: "/work".into(),
        tools: vec!["echo".into()],
        capabilities: Default::default(),
        acked_by: "cli".into(),
        acked_via: "cli".into(),
        acked_at_ms: 0,
        question: "act_q".into(),
        proposed_by: Default::default(),
        place_name: Some("#pier".into()),
        ceiling: Some(theseus_protocol::PlaceCeiling {
            posture_floor: Some("approve".into()),
            ..Default::default()
        }),
        replaced: None,
    };
    let floors = super::load::Floors::default();
    floors.set(&l);
    let d = crate::policy::Decision {
        posture: Posture::Open,
        reason: "open".into(),
        notify: None,
        floor: false,
        granted: None,
        external: None,
    };
    let held = floors.floor("mcp:ext-wordcount/echo", d.clone(), "echo");
    assert_eq!(held.posture, Posture::Approve);
    assert!(held.reason.contains("#pier"), "{}", held.reason);
    assert_eq!(
        floors.floor("mcp:other/echo", d.clone(), "echo").posture,
        d.posture
    );
    floors.clear("ext-wordcount");
    assert_eq!(
        floors.floor("mcp:ext-wordcount/echo", d, "echo").posture,
        Posture::Open
    );
}
