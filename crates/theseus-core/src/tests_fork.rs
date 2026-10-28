//! `session.fork` (theseus-2qzx, R19) through the whole core: a fork's first
//! request holds the source's messages through the fork point and nothing
//! after it; a turn in the fork writes nothing to the source; `node.reach`
//! lists the fork, and a fork of a fork; a restart keeps both; the refusals;
//! and a fork costs the same at 1,000 nodes as at 100,000.

use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use theseus_protocol::{SessionForkParams, SessionForkResult, SessionKind};

use crate::approval::Surface;
use crate::bus::EventSink;
use crate::places::BoundPlace;
use crate::provider::{
    DeltaSink, FakeProvider, Provider, ProviderFuture, ProviderRequest, Scripted,
};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

const OWNER: u64 = 271_828_182_845_904_523;
const LAB: u64 = 314_159_265_358_979_323;

/// A model that answers each request with its own turn's words ("answer to
/// <last user text>") and keeps every request.
#[derive(Default)]
struct Model {
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
        Box::pin(async move {
            self.requests.lock().unwrap().push(req.clone());
            let said = last_user_text(req);
            let f = FakeProvider::scripted(vec![Scripted::text(&format!("answer to {said}"))]);
            f.stream_message(req, on_delta).await
        })
    }
}

fn last_user_text(req: &ProviderRequest) -> String {
    req.messages
        .iter()
        .rev()
        .find(|m| m["role"] == "user")
        .and_then(|m| m["content"].as_array())
        .and_then(|b| b.iter().find_map(|b| b["text"].as_str()))
        .unwrap_or_default()
        .to_string()
}

struct Rig {
    core: Arc<Core>,
    model: Arc<Model>,
    _dir: tempfile::TempDir,
}

fn path(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn rig() -> Rig {
    rig_in(None)
}

/// A rig whose model's window is `window` tokens (and its answers short).
fn rig_in(window: Option<u64>) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = path(dir.path());
    cfg.tools.projects_dir = Some(path(&root));
    cfg.tools.roots = vec![];
    cfg.places.owner = Some(vec![format!("discord:{OWNER}")]);
    // A fork starts with the daemon's defaults: keep memory out of what a
    // request holds, so the messages are the conversation's alone.
    cfg.memory.mode = crate::config::MemoryMode::Off;
    if let Some(w) = window {
        cfg.catalog.insert(
            "claude-sonnet-5-5".into(),
            crate::catalog::CatalogRow {
                context_window: Some(w),
                max_output_tokens: Some(1_024),
                ..Default::default()
            },
        );
    }
    let store = Store::open(&dir.path().join("store")).unwrap();
    let model = Arc::new(Model::default());
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, model.clone(), store)).unwrap();
    core.bind_places(vec![BoundPlace {
        target: format!("discord:channel:{LAB}"),
        name: "#lab".into(),
        private: false,
        ..Default::default()
    }]);
    Rig {
        core,
        model,
        _dir: dir,
    }
}

fn new_session(core: &Core, place: Option<&str>) -> String {
    let r = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&r.session_id, &r).unwrap();
    if let Some(p) = place {
        core.outbox.bind_place(p, &r.session_id).unwrap();
    }
    r.session_id
}

async fn turn(core: &Arc<Core>, sid: &str, input: &str) {
    let rec = core
        .store
        .get_session::<SessionRecord>(sid)
        .unwrap()
        .unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = EventSink::new(core.bus.clone(), sid, None);
    core.runner
        .run(TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "the CLI".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
        .unwrap();
}

fn fork(core: &Core, session: &str, at: &str) -> SessionForkResult {
    core.session_fork(
        SessionForkParams {
            session: session.into(),
            at: at.into(),
            ..Default::default()
        },
        Surface::Cli,
    )
    .unwrap()
}

/// The ids of a session's own nodes, in order, with their user text.
fn nodes_of(core: &Core, sid: &str) -> Vec<(String, String)> {
    core.store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .map(|(_, n)| {
            let text = match &n.body {
                crate::node::Body::UserMessage { text, .. } => text.clone(),
                crate::node::Body::AssistantMessage { blocks, .. } => blocks
                    .iter()
                    .filter_map(|b| b["text"].as_str())
                    .collect::<Vec<_>>()
                    .join(""),
                _ => String::new(),
            };
            (n.id, text)
        })
        .collect()
}

/// A request's messages without the cache markers, which sit on the last
/// block only.
fn plain(req: &ProviderRequest) -> Vec<Value> {
    fn strip(v: &mut Value) {
        match v {
            Value::Object(m) => {
                m.remove("cache_control");
                m.values_mut().for_each(strip);
            }
            Value::Array(a) => a.iter_mut().for_each(strip),
            _ => {}
        }
    }
    let mut m = req.messages.clone();
    m.iter_mut().for_each(strip);
    m
}

fn requests(r: &Rig) -> Vec<ProviderRequest> {
    r.model.requests.lock().unwrap().clone()
}

/// Three turns in a source session; the ids of its nodes.
async fn three_turns(r: &Rig) -> (String, Vec<(String, String)>) {
    let s = new_session(&r.core, None);
    for t in ["one", "two", "three"] {
        turn(&r.core, &s, t).await;
    }
    let nodes = nodes_of(&r.core, &s);
    assert_eq!(nodes.len(), 6, "{nodes:?}");
    (s, nodes)
}

/// A fork at an answer compiles the source's prefix exactly: its first
/// request's messages equal the source's through that answer, and then its
/// own message; the source's later turns never appear.
#[tokio::test]
async fn a_fork_compiles_the_prefix_exactly_and_never_the_sources_later_nodes() {
    let r = rig();
    let (s, nodes) = three_turns(&r).await;
    let at = &nodes[3].0; // the answer to "two"
    let f = fork(&r.core, &s, at);
    assert_eq!(f.at_node, *at);
    assert_eq!(f.title, "one (fork)", "the first prompt titles the source");

    turn(&r.core, &f.session_id, "other road").await;
    let reqs = requests(&r);
    let source_third = plain(&reqs[2]); // one, a1, two, a2, three
    let fork_first = plain(reqs.last().unwrap()); // one, a1, two, a2, other road
    assert_eq!(fork_first.len(), 5, "{fork_first:?}");
    assert_eq!(
        fork_first[..4],
        source_third[..4],
        "the prefix is the source's"
    );
    let text = serde_json::to_string(&fork_first).unwrap();
    assert!(text.contains("other road"), "{text}");
    assert!(
        !text.contains("three") && !text.contains("answer to three"),
        "the source's later nodes leaked: {text}"
    );
    // Its own turn: the fork's nodes are its own two, and the inherited
    // prefix is read through its transcript, not copied.
    assert_eq!(nodes_of(&r.core, &f.session_id).len(), 2);
    let transcript = r.core.store.transcript(&f.session_id).unwrap();
    assert_eq!(transcript.len(), 6, "four inherited and two of its own");
}

/// A fork at a person's message ends its prefix there.
#[tokio::test]
async fn a_fork_at_a_message_of_the_person_ends_there() {
    let r = rig();
    let (s, nodes) = three_turns(&r).await;
    let f = fork(&r.core, &s, &nodes[2].0); // "two"
    turn(&r.core, &f.session_id, "instead").await;
    let reqs = requests(&r);
    let first = plain(reqs.last().unwrap());
    let text = serde_json::to_string(&first).unwrap();
    assert!(text.contains("two") && text.contains("instead"), "{text}");
    assert!(!text.contains("answer to two"), "{text}");
}

/// A turn in the fork writes nothing to the source: its scope, its record,
/// its execution.
#[tokio::test]
async fn a_turn_in_the_fork_writes_nothing_to_the_source() {
    let r = rig();
    let (s, nodes) = three_turns(&r).await;
    let scope_before = r.core.store.scope_after(&s, 0).unwrap().len();
    let rec_before = serde_json::to_string(
        &r.core
            .store
            .get_session::<SessionRecord>(&s)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    let exec_id = serde_json::from_str::<SessionRecord>(&rec_before)
        .unwrap()
        .execution_id
        .unwrap();
    let exec_before = serde_json::to_string(&r.core.kernel.execution(&exec_id).unwrap()).unwrap();

    let f = fork(&r.core, &s, &nodes[1].0);
    turn(&r.core, &f.session_id, "a different second question").await;
    turn(&r.core, &f.session_id, "and a third").await;

    assert_eq!(r.core.store.scope_after(&s, 0).unwrap().len(), scope_before);
    let rec_after = serde_json::to_string(
        &r.core
            .store
            .get_session::<SessionRecord>(&s)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(rec_after, rec_before);
    let exec_after = serde_json::to_string(&r.core.kernel.execution(&exec_id).unwrap()).unwrap();
    assert_eq!(exec_after, exec_before);
}

/// The fork is one frame: its execution, session, compilation, both edges
/// and its row land together, a few hundred bytes.
#[tokio::test]
async fn a_fork_is_one_frame_of_a_few_hundred_bytes_by_reference() {
    let r = rig();
    let (s, nodes) = three_turns(&r).await;
    let before = theseus_store::frames_written_here();
    let f = fork(&r.core, &s, &nodes[3].0);
    assert_eq!(theseus_store::frames_written_here() - before, 1);
    let rec: SessionRecord = r.core.store.get_session(&f.session_id).unwrap().unwrap();
    let origin = rec.forked_from.as_deref().unwrap();
    assert_eq!(origin.session, s);
    assert_eq!(origin.node, nodes[3].0);
    let c = r
        .core
        .store
        .get_compilation(rec.compilation_id.as_deref().unwrap())
        .unwrap()
        .unwrap();
    assert!(
        c.includes.is_empty(),
        "a run by reference, not a list of ids"
    );
    assert_eq!(c.inherits.as_ref().unwrap().through_node, nodes[3].0);
    assert_eq!(c.trigger, "fork");
    assert_eq!(c.derived_from, f.derived_from);
    assert!(c.derived_from.is_some());
    assert!(f.bytes > 0 && f.bytes < 4096, "{} bytes", f.bytes);
    // The fork edge: derived_from the source's compilation, via fork.
    let edges: Vec<crate::graph::Edge> = r
        .core
        .store
        .scope_after(
            &crate::graph::Edge::scope_into(c.derived_from.as_deref().unwrap()),
            0,
        )
        .unwrap()
        .iter()
        .map(|x| x.decode().unwrap())
        .collect();
    assert!(edges
        .iter()
        .any(|e| e.kind == "derived_from" && e.from == c.id && e.via == "fork"));
}

/// `node.reach` of a source node lists the fork as an exposure, and the
/// loops of the fork that held it; a node after the fork point is not
/// reached by it.
#[tokio::test]
async fn reach_on_a_source_node_lists_the_fork() {
    let r = rig();
    let (s, nodes) = three_turns(&r).await;
    let f = fork(&r.core, &s, &nodes[3].0);
    turn(&r.core, &f.session_id, "other road").await;

    let reach = crate::reach::reach(&r.core.store, &nodes[0].0, None)
        .unwrap()
        .unwrap();
    assert_eq!(reach.forks.len(), 1, "{:?}", reach.forks);
    let fk = &reach.forks[0];
    assert_eq!(
        (fk.session_id.as_str(), fk.generation),
        (f.session_id.as_str(), 1)
    );
    assert_eq!(fk.from, s);
    assert_eq!(fk.at_position, f.at_position);
    assert_eq!(
        fk.exposure.compilations.len(),
        1,
        "the fork's first compilation"
    );
    assert_eq!(fk.exposure.loops, 1, "the fork's one loop");
    assert!(reach.totals.sessions >= 2);

    // A node after the fork point is not in the fork.
    let later = crate::reach::reach(&r.core.store, &nodes[5].0, None)
        .unwrap()
        .unwrap();
    assert!(later.forks.is_empty(), "{:?}", later.forks);
    // The fork's own node reaches no fork.
    let own = nodes_of(&r.core, &f.session_id);
    let mine = crate::reach::reach(&r.core.store, &own[0].0, None)
        .unwrap()
        .unwrap();
    assert!(mine.forks.is_empty());
}

/// A fork of a fork: its prefix is the first fork's (the source's through
/// the first point, then the first fork's own), and `node.reach` of a node
/// of the original lists both generations.
#[tokio::test]
async fn a_fork_of_a_fork_works_and_reach_lists_both_generations() {
    let r = rig();
    let (s, nodes) = three_turns(&r).await;
    let f1 = fork(&r.core, &s, &nodes[3].0);
    turn(&r.core, &f1.session_id, "road one").await;
    let own = nodes_of(&r.core, &f1.session_id);
    let f2 = fork(&r.core, &f1.session_id, &own[1].0); // its answer to "road one"
    turn(&r.core, &f2.session_id, "road two").await;

    let reqs = requests(&r);
    let text = serde_json::to_string(&plain(reqs.last().unwrap())).unwrap();
    for want in ["one", "two", "road one", "answer to road one", "road two"] {
        assert!(text.contains(want), "{want} missing: {text}");
    }
    assert!(!text.contains("three"), "{text}");
    assert_eq!(
        r.core.store.transcript(&f2.session_id).unwrap().len(),
        8,
        "four of the original, two of the first fork, two of its own"
    );

    let reach = crate::reach::reach(&r.core.store, &nodes[0].0, None)
        .unwrap()
        .unwrap();
    let got: Vec<(&str, u32)> = reach
        .forks
        .iter()
        .map(|f| (f.session_id.as_str(), f.generation))
        .collect();
    assert_eq!(
        got,
        vec![(f1.session_id.as_str(), 1), (f2.session_id.as_str(), 2)]
    );
    // The first fork's own node reaches the second alone.
    let own_reach = crate::reach::reach(&r.core.store, &own[0].0, None)
        .unwrap()
        .unwrap();
    assert_eq!(own_reach.forks.len(), 1);
    assert_eq!(own_reach.forks[0].session_id, f2.session_id);
}

/// A restart keeps both sessions, and the fork compiles and answers.
#[tokio::test]
async fn a_restart_keeps_the_fork_and_it_still_compiles() {
    let r = rig();
    let (s, nodes) = three_turns(&r).await;
    let f = fork(&r.core, &s, &nodes[3].0);
    turn(&r.core, &f.session_id, "before the restart").await;

    let model = Arc::new(Model::default());
    let core = Core::build(crate::rpc::Parts::for_tests(
        (*r.core.cfg).clone(),
        model.clone(),
        r.core.store.clone(),
    ))
    .unwrap();
    turn(&core, &f.session_id, "after the restart").await;
    let req = model.requests.lock().unwrap().last().unwrap().clone();
    let text = serde_json::to_string(&plain(&req)).unwrap();
    for want in ["one", "two", "before the restart", "after the restart"] {
        assert!(text.contains(want), "{want} missing: {text}");
    }
    assert!(!text.contains("three"));
    // One compilation still: the restart recompiled nothing.
    assert_eq!(
        core.store
            .session_compilations(&f.session_id)
            .unwrap()
            .len(),
        1
    );
    let reach = crate::reach::reach(&core.store, &nodes[0].0, None)
        .unwrap()
        .unwrap();
    assert_eq!(reach.forks.len(), 1);
}

/// A name, a whole id, and a WAL position all name the fork point; a tool
/// call or an unknown node is refused.
#[tokio::test]
async fn the_fork_point_is_a_node_by_name_or_a_position_and_must_close_its_calls() {
    let r = rig();
    let (s, nodes) = three_turns(&r).await;
    let named = format!("msg·{}", &nodes[1].0[nodes[1].0.len() - 6..]);
    assert_eq!(fork(&r.core, &s, &named).at_node, nodes[1].0);
    let (pos, _) = r.core.store.get_node(&nodes[1].0).unwrap().unwrap();
    // A position between two nodes names the last one at or before it.
    assert_eq!(
        fork(&r.core, &s, &format!("@{}", pos + 1)).at_node,
        nodes[1].0
    );
    assert_eq!(fork(&r.core, &s, &nodes[1].0).at_node, nodes[1].0);

    let e = r
        .core
        .session_fork(
            SessionForkParams {
                session: s.clone(),
                at: "msg·ffffff".into(),
                ..Default::default()
            },
            Surface::Cli,
        )
        .unwrap_err();
    assert!(e.message.contains("no node"), "{}", e.message);
    // An answer that made a tool call leaves the call without its result.
    let call = crate::node::Node::assistant(
        &s,
        "turn_open_call",
        0,
        crate::node::Body::AssistantMessage {
            blocks: vec![
                json!({"type": "tool_use", "id": "toolu_1", "name": "fs.read", "input": {}}),
            ],
            model: "stand-in".into(),
            provider: "stand-in".into(),
            stop_reason: Some("tool_use".into()),
            usage: theseus_protocol::Usage::default(),
            cost_usd: None,
            catalog_version: None,
            request_id: None,
            correlation_id: None,
            compilation_id: None,
            request_digest: None,
            effort: None,
        },
    );
    r.core.store.append(&[call.record().unwrap()]).unwrap();
    let e = r
        .core
        .session_fork(
            SessionForkParams {
                session: s.clone(),
                at: call.id.clone(),
                ..Default::default()
            },
            Surface::Cli,
        )
        .unwrap_err();
    assert!(e.message.contains("not a place to fork"), "{}", e.message);
    // Another session's node is not in this transcript.
    let other = new_session(&r.core, None);
    turn(&r.core, &other, "elsewhere").await;
    let theirs = nodes_of(&r.core, &other);
    let e = r
        .core
        .session_fork(
            SessionForkParams {
                session: s,
                at: theirs[0].0.clone(),
                ..Default::default()
            },
            Surface::Cli,
        )
        .unwrap_err();
    assert!(e.message.contains("not in the transcript"), "{}", e.message);
}

/// The title, the profile and the budget: the source's title plus " (fork)"
/// when none is given; a budget is the fork's own, pinned limit.
#[tokio::test]
async fn a_forks_title_profile_and_budget_are_its_own() {
    let r = rig();
    let (s, nodes) = three_turns(&r).await;
    let mut src: SessionRecord = r.core.store.get_session(&s).unwrap().unwrap();
    src.title = Some("refactor the writer".into());
    r.core.store.put_session(&s, &src).unwrap();
    let f = r
        .core
        .session_fork(
            SessionForkParams {
                session: s.clone(),
                at: nodes[3].0.clone(),
                budget_usd: Some(1.5),
                ..Default::default()
            },
            Surface::Cli,
        )
        .unwrap();
    assert_eq!(f.title, "refactor the writer (fork)");
    let rec: SessionRecord = r.core.store.get_session(&f.session_id).unwrap().unwrap();
    let exec = r
        .core
        .kernel
        .execution(rec.execution_id.as_deref().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(exec.budget.limit_micros, theseus_kernel::usd_to_micros(1.5));
    assert!(exec.budget.pinned);
    assert_eq!(rec.last_target, src.last_target);

    let named = r
        .core
        .session_fork(
            SessionForkParams {
                session: s.clone(),
                at: nodes[3].0.clone(),
                title: Some("the other road".into()),
                profile: Some("no-such-profile".into()),
                ..Default::default()
            },
            Surface::Cli,
        )
        .unwrap_err();
    assert!(
        named.message.contains("no-such-profile"),
        "{}",
        named.message
    );
    let bad = r
        .core
        .session_fork(
            SessionForkParams {
                session: s,
                at: nodes[3].0.clone(),
                budget_usd: Some(-1.0),
                ..Default::default()
            },
            Surface::Cli,
        )
        .unwrap_err();
    assert!(bad.message.contains("budget_usd"), "{}", bad.message);
}

/// Authority: a surface that reads no private text may not fork a private
/// place's session; an MCP client forks only its own; an imported session
/// is not forked.
#[tokio::test]
async fn a_fork_of_a_session_the_caller_cannot_read_is_refused() {
    let r = rig();
    let (s, nodes) = three_turns(&r).await;
    for surface in [Surface::Discord, Surface::Unnamed] {
        let e = r
            .core
            .session_fork(
                SessionForkParams {
                    session: s.clone(),
                    at: nodes[1].0.clone(),
                    ..Default::default()
                },
                surface,
            )
            .unwrap_err();
        assert_eq!(e.code, theseus_protocol::error_code::REFUSED, "{surface:?}");
        assert!(e.message.contains("private place"), "{}", e.message);
    }
    // An MCP client may fork only a conversation it opened.
    let e = r
        .core
        .session_fork(
            SessionForkParams {
                session: s.clone(),
                at: nodes[1].0.clone(),
                ..Default::default()
            },
            Surface::Mcp,
        )
        .unwrap_err();
    assert!(e.message.contains("forks only its own"), "{}", e.message);
    assert!(crate::mcp_server::allowed(
        theseus_protocol::method::SESSION_FORK
    ));
    // Nothing was written by a refusal.
    assert_eq!(
        r.core
            .store
            .scope_after(&crate::graph::Edge::scope_into(&s), 0)
            .unwrap()
            .len(),
        0
    );

    // An imported session is the owner's history: closed.
    let mut imp = SessionRecord::new(SessionKind::Conversation, None);
    imp.imported = Some(Box::new(
        serde_json::from_value(json!({
            "tag": "tern-2026", "episode_id": "ep1", "hash": "h", "source": "wiki",
            "place": {"kind": "cli"}, "labels": {"sensitivity": "personal"},
            "as_of": {"start_ms": 1, "end_ms": 2}, "messages": 1, "summary": false,
            "file": "f", "line": 1, "imported_at_ms": 3
        }))
        .unwrap(),
    ));
    r.core.store.put_session(&imp.session_id, &imp).unwrap();
    let e = r
        .core
        .session_fork(
            SessionForkParams {
                session: imp.session_id,
                at: "msg·abcdef".into(),
                ..Default::default()
            },
            Surface::Cli,
        )
        .unwrap_err();
    assert!(e.message.contains("imported"), "{}", e.message);
}

/// An MCP client's conversation forks under its own authority, and the fork
/// carries it: never wider than the source's.
#[tokio::test]
async fn a_fork_keeps_the_sources_authority() {
    let r = rig();
    let info = r
        .core
        .mcp_open(theseus_protocol::SessionOpenParams {
            label: Some("mcp lab".into()),
            ..Default::default()
        })
        .unwrap();
    let s = info.session_id;
    turn(&r.core, &s, "one").await;
    let rec: SessionRecord = r.core.store.get_session(&s).unwrap().unwrap();
    let nodes = nodes_of(&r.core, &s);
    let f = r
        .core
        .session_fork(
            SessionForkParams {
                session: s.clone(),
                at: nodes[1].0.clone(),
                ..Default::default()
            },
            Surface::Mcp,
        )
        .unwrap();
    let frec: SessionRecord = r.core.store.get_session(&f.session_id).unwrap().unwrap();
    let sexec = r
        .core
        .kernel
        .execution(rec.execution_id.as_deref().unwrap())
        .unwrap()
        .unwrap();
    let fexec = r
        .core
        .kernel
        .execution(frec.execution_id.as_deref().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(fexec.authority, sexec.authority);
    assert!(crate::mcp_server::own(&fexec.authority));
}

/// A shared place's session forks into a private place: the fork speaks in
/// no place, and nothing of it reaches the shared one.
#[tokio::test]
async fn a_fork_of_a_shared_places_session_speaks_in_no_shared_place() {
    let r = rig();
    let s = new_session(&r.core, Some(&format!("channel:{LAB}")));
    for t in ["one", "two"] {
        turn(&r.core, &s, t).await;
    }
    assert_eq!(
        r.core.runner.class_of(&s),
        crate::places::PlaceClass::Shared
    );
    let nodes = nodes_of(&r.core, &s);
    // A shared-place caller may fork it (its words are the place's).
    let f = r
        .core
        .session_fork(
            SessionForkParams {
                session: s.clone(),
                at: nodes[1].0.clone(),
                ..Default::default()
            },
            Surface::Discord,
        )
        .unwrap();
    assert_eq!(
        r.core.runner.class_of(&f.session_id),
        crate::places::PlaceClass::Private,
        "the fork is private"
    );
    let lab = format!("discord:channel:{LAB}");
    let posts_before = r.core.outbox.open_for(&lab).len();
    turn(&r.core, &f.session_id, "the private road").await;
    assert_eq!(
        r.core.outbox.target(&f.session_id),
        None,
        "the fork is bound to no place"
    );
    assert_eq!(
        r.core.outbox.open_for(&lab).len(),
        posts_before,
        "nothing of the fork was posted to the shared place"
    );
}

/// What a fork costs does not grow with the source: the same records read
/// and the same bytes written at 1,000 nodes as at 100,000, and no more
/// than twice the time.
#[tokio::test]
async fn a_fork_costs_the_same_at_1_000_nodes_as_at_100_000() {
    let r = rig();
    let measure = |r: &Rig, sid: &str, at: &str, n: usize| -> (u64, u64, std::time::Duration) {
        let mut times = Vec::new();
        let (mut reads, mut bytes) = (0, 0);
        for _ in 0..n {
            let before = theseus_store::records_read_here();
            let t = std::time::Instant::now();
            let f = fork(&r.core, sid, at);
            times.push(t.elapsed());
            reads = theseus_store::records_read_here() - before;
            bytes = f.bytes;
        }
        times.sort();
        eprintln!(
            "{n} forks: p50 {:?}, p95 {:?}",
            times[n / 2],
            times[n * 95 / 100]
        );
        (reads, bytes, times[times.len() / 2])
    };
    // One real turn gives the session its compilation and execution; the
    // rest of the nodes are written in frames, as a long conversation's are.
    let grow = |r: &Rig, sid: &str, upto: usize| {
        let have = r.core.store.session_nodes(sid).unwrap().len();
        let mut at = have;
        while at < upto {
            let n = 5_000.min(upto - at);
            let records: Vec<theseus_store::NewRecord> = (0..n)
                .map(|i| {
                    let node =
                        crate::node::Node::user(sid, None, "the CLI", &format!("note {}", at + i));
                    node.record().unwrap()
                })
                .collect();
            r.core.store.append(&records).unwrap();
            at += n;
        }
    };
    let s = new_session(&r.core, None);
    turn(&r.core, &s, "start").await;
    grow(&r, &s, 1_000);
    let small_at = r
        .core
        .store
        .session_nodes(&s)
        .unwrap()
        .last()
        .unwrap()
        .1
        .id
        .clone();
    let (reads_small, bytes_small, t_small) = measure(&r, &s, &small_at, 40);
    grow(&r, &s, 100_000);
    let big_at = r
        .core
        .store
        .session_nodes(&s)
        .unwrap()
        .last()
        .unwrap()
        .1
        .id
        .clone();
    let (reads_big, bytes_big, t_big) = measure(&r, &s, &big_at, 40);
    eprintln!(
        "fork at 1,000 nodes: {t_small:?}, {reads_small} reads, {bytes_small} B; at 100,000: \
         {t_big:?}, {reads_big} reads, {bytes_big} B"
    );
    assert_eq!(
        reads_big, reads_small,
        "records read must not grow with the source"
    );
    assert!(
        bytes_big.abs_diff(bytes_small) < 64,
        "{bytes_small} B then {bytes_big} B"
    );
    assert!(
        t_big <= t_small * 2 + std::time::Duration::from_millis(2),
        "{t_small:?} at 1,000 nodes, {t_big:?} at 100,000"
    );
    assert!(bytes_big < 4096, "{bytes_big} B");
    let _ = json!(null);
}

/// A fork's history is its inherited prefix and then its own nodes, one list
/// by position, paged as any session's is: the newest page reads back into
/// the prefix only as far as it must, and every inherited node keeps its
/// source's session.
#[tokio::test]
async fn a_forks_history_is_the_prefix_then_its_own_turns_and_pages_across_the_join() {
    let r = rig();
    let (s, nodes) = three_turns(&r).await;
    let f = fork(&r.core, &s, &nodes[3].0);
    turn(&r.core, &f.session_id, "other road").await;
    let own = nodes_of(&r.core, &f.session_id);
    let page = |n: Option<usize>, after: Option<u64>, before: Option<u64>| {
        r.core
            .session_history(theseus_protocol::SessionHistoryParams {
                session_id: f.session_id.clone(),
                n,
                after,
                before,
            })
            .unwrap()
    };
    let ids = |h: &theseus_protocol::SessionHistoryResult| -> Vec<String> {
        h.nodes.iter().map(|n| n.node_id.clone()).collect()
    };

    let whole = page(None, None, None);
    let mut want: Vec<String> = nodes[..4].iter().map(|(id, _)| id.clone()).collect();
    want.extend(own.iter().map(|(id, _)| id.clone()));
    assert_eq!(ids(&whole), want);
    assert!(whole.nodes[..4].iter().all(|n| n.session_id == s));
    assert!(whole.nodes[4..]
        .iter()
        .all(|n| n.session_id == f.session_id));

    // The newest page: its own turn alone, then back across the join.
    assert_eq!(ids(&page(Some(2), None, None)), want[4..]);
    assert_eq!(ids(&page(Some(3), None, None)), want[3..]);
    assert_eq!(ids(&page(Some(100), None, None)), want);

    // A page back from its own first node ends in the prefix, and says
    // where the next goes.
    let first_own = whole.nodes[4].position;
    let back = page(Some(2), None, Some(first_own));
    assert_eq!(ids(&back), want[2..4]);
    assert_eq!(back.older, Some(whole.nodes[2].position));
    // A page forward from inside the prefix crosses into the fork's own.
    let from = whole.nodes[1].position;
    let fwd = page(Some(3), Some(from), None);
    assert_eq!(ids(&fwd), want[2..5]);
    assert_eq!(fwd.next, Some(whole.nodes[4].position));
}

/// A recompile in the fork lists every node it renders by id, the inherited
/// ones too, and the request reads the same conversation as the append did.
#[tokio::test]
async fn a_recompile_in_the_fork_lists_the_inherited_nodes_by_id() {
    let r = rig();
    let (s, nodes) = three_turns(&r).await;
    let f = fork(&r.core, &s, &nodes[3].0);
    turn(&r.core, &f.session_id, "first in the fork").await;
    let mut rec: SessionRecord = r.core.store.get_session(&f.session_id).unwrap().unwrap();
    rec.pending_recompile = Some(crate::compiler::Recompile::Transcript);
    r.core.store.put_session(&f.session_id, &rec).unwrap();
    turn(&r.core, &f.session_id, "second in the fork").await;

    let compilations = r.core.store.session_compilations(&f.session_id).unwrap();
    assert_eq!(compilations.len(), 2, "the fork's first, and the recompile");
    let (first, again) = (&compilations[0], &compilations[1]);
    assert!(first.inherits.is_some() && first.includes.is_empty());
    assert!(again.inherits.is_none(), "a recompile names its nodes");
    for (id, _) in &nodes[..4] {
        assert!(
            again.includes.contains(id),
            "{id} missing from {:?}",
            again.includes
        );
    }
    assert!(
        !again.includes.contains(&nodes[4].0),
        "the source's later nodes"
    );
    let reqs = requests(&r);
    let text = serde_json::to_string(&plain(reqs.last().unwrap())).unwrap();
    for want in ["one", "two", "first in the fork", "second in the fork"] {
        assert!(text.contains(want), "{want} missing: {text}");
    }
    assert!(!text.contains("three"), "{text}");
}

/// The ring works over the inherited prefix and the fork's own nodes: a
/// fork whose context passes its model's window drops its leading turns as
/// any session does, and names what it keeps by id.
#[tokio::test]
async fn a_fork_past_its_window_rings_over_the_prefix_and_its_own_turns() {
    let r = rig_in(Some(40_000));
    let s = new_session(&r.core, None);
    let long = |w: &str| format!("{w} {}", "tide ".repeat(800));
    for w in ["alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta"] {
        turn(&r.core, &s, &long(w)).await;
    }
    let nodes = nodes_of(&r.core, &s);
    let f = fork(&r.core, &s, &nodes[11].0); // through the answer to "zeta"
    turn(&r.core, &f.session_id, &long("theta")).await;
    turn(&r.core, &f.session_id, "short").await;

    let compilations = r.core.store.session_compilations(&f.session_id).unwrap();
    let last = compilations.last().unwrap();
    assert!(
        compilations.len() >= 2,
        "{} compilation(s)",
        compilations.len()
    );
    assert!(last.inherits.is_none(), "{}", last.strategy);
    assert!(
        matches!(last.strategy.as_str(), "ring" | "compaction"),
        "{}",
        last.strategy
    );
    assert!(
        !last.includes.contains(&nodes[0].0),
        "the oldest turn is dropped"
    );
    let req = requests(&r).pop().unwrap();
    let text = serde_json::to_string(&plain(&req)).unwrap();
    assert!(text.contains("short"));
    let said: Vec<String> = req
        .messages
        .iter()
        .filter_map(|m| m["content"][0]["text"].as_str().map(str::to_string))
        .collect();
    assert!(
        !said.iter().any(|t| t.starts_with("eta ")),
        "the source's later turn leaked"
    );
    assert!(said.iter().any(|t| t.starts_with("theta ")));
}

/// A fork's content labels (theseus-9ymz0): the labels whose item the fork
/// carries are copied, one raised by a node after the fork point is not, and
/// the fork is judged as far as its source was, through the point at most.
#[tokio::test]
async fn fork_copies_content_labels_through_the_fork_point() {
    use crate::content::{ContentFlags, Flag, ItemKind, Label};
    use theseus_judge::band::Band;
    let r = rig();
    let (s, nodes) = three_turns(&r).await;
    let label = |flag: Flag, node: &str| Label {
        flag,
        node: Some(node.into()),
        kind: ItemKind::ToolResult,
        source: "fs.read /srv/notes.txt".into(),
        confidence: 0.8,
        band: Band::Confirm,
        at_ms: 9,
        judgment: String::new(),
    };
    let set = |judged_through: &str| {
        let mut rec: SessionRecord = r.core.store.get_session(&s).unwrap().unwrap();
        let mut flags = ContentFlags::default();
        flags.raise(label(Flag::Confidential, &nodes[1].0));
        flags.raise(label(Flag::Secret, &nodes[4].0));
        flags.judged_through = Some(judged_through.into());
        rec.data = Some(Box::new(flags));
        r.core.store.put_session(&s, &rec).unwrap();
    };
    let data_of = |sid: &str| {
        let rec: SessionRecord = r.core.store.get_session(sid).unwrap().unwrap();
        rec.data.map(|d| *d)
    };
    // Judged past the point: the fork is judged through the point, and holds
    // only the label its own nodes raised.
    set(&nodes[5].0);
    let before = fork(&r.core, &s, &nodes[3].0);
    let got = data_of(&before.session_id).expect("the fork carries labels");
    assert!(got.has(Flag::Confidential), "its item is in the fork");
    assert!(!got.has(Flag::Secret), "its item came after the point");
    assert_eq!(got.judged_through.as_deref(), Some(nodes[3].0.as_str()));
    // Judged only to an earlier node: the fork is judged no further.
    set(&nodes[1].0);
    let short = fork(&r.core, &s, &nodes[3].0);
    let got = data_of(&short.session_id).unwrap();
    assert_eq!(got.judged_through.as_deref(), Some(nodes[1].0.as_str()));
    assert_eq!(
        data_of(&s).unwrap().judged_through.as_deref(),
        Some(nodes[1].0.as_str()),
        "the source is read only"
    );
    assert!(data_of(&s).unwrap().has(Flag::Secret));
}
