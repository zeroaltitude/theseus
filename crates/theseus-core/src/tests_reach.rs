//! `node.reach` through the whole core (theseus-n4m, step 12a; design stage2
//! §2.11 and §3.2's row 12a): the report route's `derived_from` edge, the
//! brief's, and the walk over them.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_kernel::{ExecState, Execution};
use theseus_protocol::{
    error_code, method, NodeReachResult, ReachExposure, ReachTotals, SessionKind, TurnSubmitResult,
};
use theseus_store::{kinds, Store as _};

use crate::bus::EventSink;
use crate::compiler::Recompile;
use crate::graph::Edge;
use crate::node::{Body, Node};
use crate::provider::{
    DeltaSink, FakeProvider, Provider, ProviderFuture, ProviderRequest, Scripted,
};
use crate::reach::reach;
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

/// A stand-in model that answers each request from the request itself.
struct Model(Box<dyn Fn(&ProviderRequest) -> Scripted + Send + Sync>);

impl Provider for Model {
    fn name(&self) -> &str {
        "fake"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            let f = FakeProvider::scripted(vec![(self.0)(req)]);
            f.stream_message(req, on_delta).await
        })
    }
}

fn text_of(m: &Value) -> String {
    match &m["content"] {
        Value::String(s) => s.clone(),
        Value::Array(b) => b
            .iter()
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn first_user(req: &ProviderRequest) -> String {
    req.messages
        .iter()
        .find(|m| m["role"] == "user")
        .map(text_of)
        .unwrap_or_default()
}

fn last_user(req: &ProviderRequest) -> String {
    req.messages
        .iter()
        .rev()
        .find(|m| m["role"] == "user")
        .map(text_of)
        .unwrap_or_default()
}

fn answers_a_call(req: &ProviderRequest) -> bool {
    req.messages
        .last()
        .and_then(|m| m["content"].as_array())
        .is_some_and(|c| c.iter().any(|b| b["type"] == "tool_result"))
}

struct Rig {
    core: Arc<Core>,
    dir: tempfile::TempDir,
}

fn rig(script: impl Fn(&ProviderRequest) -> Scripted + Send + Sync + 'static) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = crate::policy::Posture::Notify;
    let store = Store::open(&dir.path().join("store")).unwrap();
    let model: Arc<dyn Provider> = Arc::new(Model(Box::new(script)));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, model, store)).unwrap();
    // The driver runs the tasks' turns, as the daemon's does.
    tokio::spawn(crate::harness::drive(core.clone()));
    Rig { core, dir }
}

/// A conversation, with no place.
fn conversation(core: &Core) -> String {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    rec.session_id
}

async fn turn(
    core: &Arc<Core>,
    sid: &str,
    input: &str,
    recompile: Option<Recompile>,
) -> TurnSubmitResult {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = EventSink::new(core.bus.clone(), sid, None);
    core.runner
        .run(TurnRequest {
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "test".into(),
            recompile,
            attachments: vec![],
            arrived: None,
            config_wait_us: 0,
            reply_to: None,
        })
        .await
        .unwrap()
}

/// The model of the scenarios: a parent that starts a task on `START`, and
/// a task that names a colour.
fn colours(req: &ProviderRequest) -> Scripted {
    if first_user(req).contains("CHILD: name a colour") {
        return Scripted::text("Teal, the colour of the sea at dusk.");
    }
    if answers_a_call(req) {
        return Scripted::text("Started it.");
    }
    if last_user(req).starts_with("START") {
        return Scripted::tools(
            "Starting it.",
            &[(
                "t_task",
                "task_create",
                json!({"brief": "CHILD: name a colour", "budget_usd": 1.5}),
            )],
        );
    }
    Scripted::text("Noted.")
}

/// The one task the session started, once it has finished.
async fn finished_task(core: &Core, parent: &str) -> Execution {
    let rec: SessionRecord = core.store.get_session(parent).unwrap().unwrap();
    let tasks = core.kernel.tasks(rec.execution_id.as_deref()).unwrap();
    assert_eq!(tasks.len(), 1, "{tasks:?}");
    let id = tasks[0].id.clone();
    let t0 = Instant::now();
    loop {
        let e = core.kernel.execution(&id).unwrap().unwrap();
        if e.state == ExecState::Complete {
            return e;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "no task end in 20 s: {e:?}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn nodes(core: &Core, sid: &str) -> Vec<(u64, Node)> {
    core.store.session_nodes(sid).unwrap()
}

/// The parent's node that relays the task's report.
fn relayed(core: &Core, parent: &str) -> (u64, Node) {
    let mut r: Vec<(u64, Node)> = nodes(core, parent)
        .into_iter()
        .filter(|(_, n)| n.author.as_deref().is_some_and(|a| a.starts_with("task:")))
        .collect();
    assert_eq!(r.len(), 1);
    r.remove(0)
}

fn ids(e: &ReachExposure) -> Vec<String> {
    e.compilations
        .iter()
        .map(|c| c.compilation_id.clone())
        .collect()
}

/// The prove (§3.2, row 12a). A task's last node is relayed to its parent,
/// and the parent's next turn compiles it. `node.reach` on the task's node
/// names both sessions (generations 0 and 1), the exact compilations, and
/// the loops. The edge is keyed and scoped as §6.1 and §2.11 say, and its
/// first write marks no manifest.
#[tokio::test]
async fn reach_follows_a_report_into_the_parent_and_names_its_compilations() {
    let r = rig(colours);
    let manifest = std::fs::read(r.dir.path().join("store/MANIFEST.json")).unwrap();
    let parent = conversation(&r.core);
    turn(&r.core, &parent, "START the colour task", None).await;
    let task = finished_task(&r.core, &parent).await;
    let tnodes = nodes(&r.core, &task.session_id);
    let (last, text) = crate::task::last_message(tnodes.iter().map(|(_, n)| n)).unwrap();
    assert_eq!(text, "Teal, the colour of the sea at dusk.");

    // The parent's next turn reads the report and recompiles, so its
    // compilation holds the relayed node; the turn after reads it again.
    turn(
        &r.core,
        &parent,
        "What did it find?",
        Some(Recompile::Transcript),
    )
    .await;
    turn(&r.core, &parent, "And then?", None).await;
    let (at, copy) = relayed(&r.core, &parent);

    // The edge: in the report's frame, right after the node it writes.
    let key = format!("derived_from|{}|{last}", copy.id);
    let rec = r
        .core
        .store
        .inner()
        .latest_by_key(kinds::EDGE, &key)
        .unwrap()
        .expect("the report's edge");
    assert_eq!(rec.position, at + 1, "in the frame that writes the copy");
    assert_eq!(rec.scope.as_deref(), Some(format!("in:{last}").as_str()));
    let edge: Edge = rec.decode().unwrap();
    assert_eq!(
        (
            edge.kind.as_str(),
            edge.from.as_str(),
            edge.to.as_str(),
            edge.via.as_str()
        ),
        ("derived_from", copy.id.as_str(), last.as_str(), "report")
    );
    assert_eq!(
        std::fs::read(r.dir.path().join("store/MANIFEST.json")).unwrap(),
        manifest,
        "EDGE is marked at schema 1 already: the first edge marks nothing"
    );

    let got = reach(&r.core.store, &last, None).unwrap().unwrap();
    // Generation 0: the task's last message, which nothing in its own
    // session read after it.
    assert_eq!(got.node_id, last);
    assert_eq!(got.session_id, task.session_id);
    assert_eq!(got.direct, ReachExposure::default());
    // Generation 1: the copy in the parent.
    assert_eq!(got.descendants.len(), 1, "{got:?}");
    let d = &got.descendants[0];
    assert_eq!(
        (
            d.node_id.as_str(),
            d.session_id.as_str(),
            d.position,
            d.generation,
            d.via.as_str(),
            d.route.as_str(),
            d.from.as_str()
        ),
        (
            copy.id.as_str(),
            parent.as_str(),
            at,
            1,
            "derived_from",
            "report",
            last.as_str()
        )
    );
    // The exact compilations: the recompile's, which is the parent's current
    // one, and no other of the parent's holds it.
    let current = r
        .core
        .store
        .get_session::<SessionRecord>(&parent)
        .unwrap()
        .unwrap()
        .compilation_id
        .unwrap();
    let holding: Vec<String> = r
        .core
        .store
        .session_compilations(&parent)
        .unwrap()
        .into_iter()
        .filter(|c| c.includes.contains(&copy.id))
        .map(|c| c.id)
        .collect();
    assert_eq!(holding, vec![current]);
    assert_eq!(ids(&d.exposure), holding);
    assert_eq!(d.exposure.compilations[0].strategy, "transcript");
    // Two loops held it: the recompiled turn's, and the next turn's.
    assert_eq!(d.exposure.loops, 2);
    let replies: Vec<u64> = nodes(&r.core, &parent)
        .iter()
        .filter(|(p, n)| *p > at && matches!(n.body, Body::AssistantMessage { .. }))
        .map(|(_, n)| n.created_at_ms)
        .collect();
    assert_eq!(replies.len(), 2);
    assert_eq!(d.exposure.last_ms, replies.last().copied());
    assert!(d.exposure.first_ms <= Some(replies[0]));
    assert_eq!(
        got.totals,
        ReachTotals {
            contexts: 3,
            sessions: 2
        }
    );
    assert!(!got.partial);

    // The copy's own reach: the same exposure, and no copies of its own.
    let own = reach(&r.core.store, &copy.id, None).unwrap().unwrap();
    assert_eq!(own.direct, d.exposure);
    assert!(own.descendants.is_empty() && !own.partial);
    assert_eq!(own.totals.sessions, 1);

    // At the generation cap the walk stops short, and says so.
    let capped = reach(&r.core.store, &last, Some(0)).unwrap().unwrap();
    assert!(capped.descendants.is_empty() && capped.partial);
    assert_eq!(capped.totals.sessions, 1);
}

/// The brief's edge (§2.11's part 2): the task's first node is
/// `derived_from` the parent's reply that holds the `task.create` call, so
/// that reply's reach names the task's session, with the task's compilation
/// and loop.
#[tokio::test]
async fn reach_follows_a_brief_into_its_task() {
    let r = rig(colours);
    let parent = conversation(&r.core);
    turn(&r.core, &parent, "START the colour task", None).await;
    let task = finished_task(&r.core, &parent).await;
    let pnodes = nodes(&r.core, &parent);
    let (holder, call) = pnodes
        .iter()
        .find_map(|(_, n)| match &n.body {
            Body::ToolCall {
                tool,
                assistant_node,
                ..
            } if tool == crate::task::CREATE => Some((assistant_node.clone(), n.id.clone())),
            _ => None,
        })
        .unwrap();
    let tnodes = nodes(&r.core, &task.session_id);
    let (bp, brief) = &tnodes[0];
    let Body::UserMessage { text, .. } = &brief.body else {
        panic!("the brief: {brief:?}")
    };
    assert!(text.contains("CHILD: name a colour"));

    let got = reach(&r.core.store, &holder, None).unwrap().unwrap();
    assert_eq!(got.session_id, parent);
    assert_eq!(got.descendants.len(), 1, "{got:?}");
    let d = &got.descendants[0];
    assert_eq!(
        (
            d.node_id.as_str(),
            d.session_id.as_str(),
            d.position,
            d.generation,
            d.route.as_str(),
            d.from.as_str()
        ),
        (
            brief.id.as_str(),
            task.session_id.as_str(),
            *bp,
            1,
            "brief",
            holder.as_str()
        )
    );
    // The task compiled its brief once and read it in its one loop.
    let tcomps = r.core.store.session_compilations(&task.session_id).unwrap();
    assert_eq!(tcomps.len(), 1);
    assert_eq!(ids(&d.exposure), vec![tcomps[0].id.clone()]);
    assert_eq!(d.exposure.loops, 1);
    // In the parent, the reply was read by the loop that answered the call.
    assert_eq!(got.direct.loops, 1);
    assert!(got.direct.compilations.is_empty());
    assert_eq!(
        got.totals,
        ReachTotals {
            contexts: 3,
            sessions: 2
        }
    );
    // The call node renders into no request: nothing held it, and nothing
    // copies it.
    let c = reach(&r.core.store, &call, None).unwrap().unwrap();
    assert_eq!(c.direct, ReachExposure::default());
    assert!(c.descendants.is_empty());
}

/// A store with no edges answers direct exposure only, from the session's
/// own compilation and loops.
#[tokio::test]
async fn a_session_with_no_edges_answers_its_own_exposure_only() {
    let r = rig(|_| Scripted::text("Noted."));
    let sid = conversation(&r.core);
    turn(&r.core, &sid, "the first message", None).await;
    turn(&r.core, &sid, "the second message", None).await;
    assert_eq!(r.core.store.inner().count_of_kind(kinds::EDGE).unwrap(), 0);
    let all = nodes(&r.core, &sid);
    let first = &all[0].1;
    let got = reach(&r.core.store, &first.id, None).unwrap().unwrap();
    let comps = r.core.store.session_compilations(&sid).unwrap();
    assert_eq!(comps.len(), 1);
    assert_eq!(ids(&got.direct), vec![comps[0].id.clone()]);
    assert_eq!(got.direct.loops, 2, "each turn's loop held it");
    assert!(got.descendants.is_empty() && !got.partial);
    assert_eq!(
        got.totals,
        ReachTotals {
            contexts: 3,
            sessions: 1
        }
    );
    // The last reply: written after every loop, read by none.
    let last = &all.last().unwrap().1;
    let got = reach(&r.core.store, &last.id, None).unwrap().unwrap();
    assert_eq!(got.direct, ReachExposure::default());
    assert_eq!(got.totals.contexts, 0);
    assert!(reach(&r.core.store, "msg_nowhere", None).unwrap().is_none());
}

/// A turn that reads a report writes no extra frame: the edge rides in the
/// report's frame. A plain turn in another session is the measure.
#[tokio::test]
async fn a_turn_that_reads_a_report_writes_no_extra_frame_for_its_edge() {
    let r = rig(colours);
    let frames = || r.core.store.stats().unwrap().frames_appended;
    let parent = conversation(&r.core);
    let other = conversation(&r.core);
    turn(&r.core, &parent, "START the colour task", None).await;
    finished_task(&r.core, &parent).await;
    turn(&r.core, &other, "warm up", None).await;
    let f0 = frames();
    turn(&r.core, &other, "a plain turn", None).await;
    let plain = frames() - f0;
    let f0 = frames();
    turn(&r.core, &parent, "a turn that reads the report", None).await;
    let reading = frames() - f0;
    assert_eq!(
        reading,
        plain + 1,
        "the report's frame, and nothing more for its edge"
    );
    let (at, _) = relayed(&r.core, &parent);
    let edges = r
        .core
        .store
        .inner()
        .tail_of_kind(kinds::EDGE, 10)
        .unwrap()
        .into_iter()
        .filter(|e| e.position > at)
        .count();
    assert_eq!(edges, 1);
}

/// One request over a connection to the core: its result, or its error's
/// code.
async fn call(core: &Arc<Core>, m: &str, params: Value) -> Result<Value, i64> {
    use tokio::io::{duplex, AsyncBufReadExt, AsyncWriteExt, BufReader};
    let (client, server) = duplex(64 * 1024);
    let (sr, sw) = tokio::io::split(server);
    let serving = tokio::spawn(core.clone().serve_connection(sr, sw, "test".into()));
    let (cr, mut cw) = tokio::io::split(client);
    let req = theseus_protocol::Request::new(theseus_protocol::Id::Num(1), m, params);
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let mut lines = BufReader::new(cr).lines();
    let answer = loop {
        let l = lines.next_line().await.unwrap().expect("an answer");
        if let theseus_protocol::Message::Response(r) = serde_json::from_str(&l).unwrap() {
            break r;
        }
    };
    cw.shutdown().await.unwrap();
    drop(lines);
    let _ = serving.await;
    match answer.error {
        Some(e) => Err(e.code),
        None => Ok(answer.result.unwrap_or(Value::Null)),
    }
}

/// `node.reach` over the protocol: the method's arm, its answer, an unknown
/// node, and no params, which fail at once.
#[tokio::test]
async fn node_reach_answers_over_the_protocol() {
    let r = rig(|_| Scripted::text("Noted."));
    let sid = conversation(&r.core);
    turn(&r.core, &sid, "a message", None).await;
    let first = nodes(&r.core, &sid)[0].1.id.clone();
    let ok = call(&r.core, method::NODE_REACH, json!({"node_id": first})).await;
    let got: NodeReachResult = serde_json::from_value(ok.unwrap()).unwrap();
    assert_eq!(got.session_id, sid);
    assert_eq!(got.totals.contexts, 2, "its compilation and its loop");
    let missing = call(&r.core, method::NODE_REACH, json!({"node_id": "msg_x"})).await;
    assert_eq!(missing.unwrap_err(), error_code::NOT_FOUND);
    let t0 = Instant::now();
    let none = call(&r.core, method::NODE_REACH, Value::Null).await;
    assert_eq!(none.unwrap_err(), error_code::INVALID_PARAMS);
    assert!(t0.elapsed() < Duration::from_secs(1));
}
