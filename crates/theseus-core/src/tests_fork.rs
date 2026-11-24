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
    // The owner typed it in person at the terminal, as `turn.submit` marks it (issue 44): a fork's
    // inherited run is its own history only where the owner is alone (`fork/window.rs`).
    let mark = crate::view_gate::test_mark(&core.runner, sid, true);
    let sink = EventSink::new(core.bus.clone(), sid, None).with_arrival(Some(mark));
    core.runner
        .run(TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "the CLI".into(),
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
    // What it is (issue 44, 2026-11-22): the record of an inherited run, not the old builder's `transcript`.
    assert_eq!(c.strategy, "fork");
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
    // Issue 44: the fork's first compilation, made at the fork, and the one its view turn makes (a view
    // turn is a new compilation every turn).
    assert_eq!(
        fk.exposure.compilations.len(),
        2,
        "the fork's first compilation and its turn's"
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

/// A fork of a fork whose two ancestors both went on after the points they were forked at (issue 44): the
/// youngest fork's request holds its own turns and the run it inherits, and none of either ancestor's later
/// turns.
#[tokio::test]
async fn a_fork_of_a_fork_holds_nothing_either_ancestor_said_after_its_point() {
    let r = rig();
    let (s, nodes) = three_turns(&r).await;
    let f1 = fork(&r.core, &s, &nodes[3].0); // through the answer to "two"
    turn(&r.core, &f1.session_id, "road one").await;
    let own = nodes_of(&r.core, &f1.session_id);
    let f2 = fork(&r.core, &f1.session_id, &own[1].0); // through its answer to "road one"
    turn(&r.core, &s, "four").await;
    turn(&r.core, &f1.session_id, "road one later").await;
    turn(&r.core, &f2.session_id, "road two").await;
    turn(&r.core, &f2.session_id, "road two again").await;

    let text = serde_json::to_string(&plain(requests(&r).last().unwrap())).unwrap();
    for want in [
        "one",
        "two",
        "answer to road one",
        "road two",
        "road two again",
    ] {
        assert!(text.contains(want), "{want} missing: {text}");
    }
    for later in ["three", "four", "road one later"] {
        assert!(!text.contains(later), "{later} leaked: {text}");
    }
}

/// A restart keeps both sessions, and the fork compiles and answers.
#[tokio::test]
async fn a_restart_keeps_the_fork_and_it_still_compiles() {
    let r = rig();
    let (s, nodes) = three_turns(&r).await;
    let f = fork(&r.core, &s, &nodes[3].0);
    turn(&r.core, &f.session_id, "before the restart").await;
    let compiled = |c: &Core| c.store.session_compilations(&f.session_id).unwrap().len();
    let before = compiled(&r.core);

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
    // The restart recompiled nothing: the one compilation since is the new turn's own (issue 44: a view
    // turn is a new compilation every turn).
    assert_eq!(compiled(&core), before + 1);
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

/// A fork's content labels (theseus-9ymz0): the labels whose item the fork
/// carries are copied, and one raised by a node after the fork point is not.
/// The source's removals come too. **The fork carries no mark** (theseus-gyjyo.7):
/// there is none to carry, and a source record that still holds one (a format-53
/// record) does not give the fork one.
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
    let mut rec: SessionRecord = r.core.store.get_session(&s).unwrap().unwrap();
    let mut flags = ContentFlags::default();
    flags.raise(label(Flag::Confidential, &nodes[1].0));
    flags.raise(label(Flag::Secret, &nodes[4].0));
    flags.raise(label(Flag::Personal, &nodes[3].0));
    flags.remove(Flag::Personal, "the owner", "cli", 11);
    rec.data = Some(Box::new(flags));
    r.core.store.put_session(&s, &rec).unwrap();
    let data_of = |sid: &str| {
        let rec: SessionRecord = r.core.store.get_session(sid).unwrap().unwrap();
        rec.data.map(|d| *d)
    };
    let forked = fork(&r.core, &s, &nodes[3].0);
    let got = data_of(&forked.session_id).expect("the fork carries labels");
    assert!(got.has(Flag::Confidential), "its item is in the fork");
    assert!(!got.has(Flag::Secret), "its item came after the point");
    assert_eq!(got.removed.len(), 1, "the source's removal comes with it");
    assert_eq!(
        data_of(&s).unwrap().labels.len(),
        2,
        "the source is read only"
    );
    assert!(data_of(&s).unwrap().has(Flag::Secret));
}

/// A source with no content data forks with none, and one whose only content
/// is after the point forks with none (nothing is written for an empty record).
#[tokio::test]
async fn a_fork_of_a_session_with_no_labels_inside_the_point_carries_no_data() {
    use crate::content::{ContentFlags, Flag, ItemKind, Label};
    use theseus_judge::band::Band;
    let r = rig();
    let (s, nodes) = three_turns(&r).await;
    let plain = fork(&r.core, &s, &nodes[3].0);
    let rec: SessionRecord = r
        .core
        .store
        .get_session(&plain.session_id)
        .unwrap()
        .unwrap();
    assert!(rec.data.is_none(), "no labels, no data");
    let mut src: SessionRecord = r.core.store.get_session(&s).unwrap().unwrap();
    let mut flags = ContentFlags::default();
    flags.raise(Label {
        flag: Flag::Secret,
        node: Some(nodes[4].0.clone()),
        kind: ItemKind::ToolResult,
        source: "fs.read /srv/notes.txt".into(),
        confidence: 0.8,
        band: Band::Confirm,
        at_ms: 9,
        judgment: String::new(),
    });
    src.data = Some(Box::new(flags));
    r.core.store.put_session(&s, &src).unwrap();
    let after = fork(&r.core, &s, &nodes[3].0);
    let rec: SessionRecord = r
        .core
        .store
        .get_session(&after.session_id)
        .unwrap()
        .unwrap();
    assert!(rec.data.is_none(), "the label's item is after the point");
}

/// Every text of each message of a request, joined, in order (a message's blocks beside it included).
fn said_in(req: &ProviderRequest) -> Vec<String> {
    (req.messages.iter())
        .map(|m| {
            (m["content"].as_array().into_iter().flatten())
                .filter_map(|b| b["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .collect()
}

/// The turn of a node, by id.
fn turn_of(core: &Core, node: &str) -> String {
    let (_, n) = core.store.get_node(node).unwrap().unwrap();
    n.turn_id.unwrap()
}

/// A fork made at the person's message, in a place other than its source's (issue 44, round 2): the message
/// at the point is in the fork's request, and neither its answer nor anything later is. Before, the inherited
/// run admitted an exchange only once its reply was inside the run, so the message was lost when no other
/// selector of the fork's own place found it.
#[tokio::test]
async fn a_fork_at_the_persons_message_elsewhere_holds_that_message_and_not_its_answer() {
    let r = rig();
    let s = new_session(&r.core, Some(&format!("channel:{LAB}")));
    for t in ["one", "two", "three"] {
        turn(&r.core, &s, t).await;
    }
    let nodes = nodes_of(&r.core, &s);
    let f = r
        .core
        .session_fork(
            SessionForkParams {
                session: s.clone(),
                at: nodes[2].0.clone(), // "two"
                ..Default::default()
            },
            Surface::Discord,
        )
        .unwrap();
    turn(&r.core, &f.session_id, "instead").await;
    let said = said_in(requests(&r).last().unwrap());
    // The message at the point and the fork's own may be one user message (two person's lines in a row).
    let line = |words: &str| said.iter().any(|t| t.lines().any(|l| l.ends_with(words)));
    assert!(line("two"), "the message at the point: {said:?}");
    assert!(line("instead"), "{said:?}");
    let text = said.join("\n");
    assert!(!text.contains("answer to two"), "its answer: {said:?}");
    assert!(!text.contains("three"), "a later turn: {said:?}");
}

/// A fork of a fork made INSIDE its parent's inherited prefix (issue 44, round 2): the grandparent is cut at
/// the earlier point, and the parent's own turns, all after it, are out.
#[tokio::test]
async fn a_fork_inside_its_parents_inherited_prefix_holds_neither_later_road() {
    let r = rig();
    let (s, nodes) = three_turns(&r).await;
    let f1 = fork(&r.core, &s, &nodes[3].0); // through the answer to "two"
    turn(&r.core, &f1.session_id, "road one").await;
    let f2 = fork(&r.core, &f1.session_id, &nodes[1].0); // the answer to "one", inside f1's prefix
    turn(&r.core, &f2.session_id, "road two").await;
    let said = said_in(requests(&r).last().unwrap());
    let text = said.join("\n");
    for want in ["answer to one", "road two"] {
        assert!(text.contains(want), "{want} missing: {said:?}");
    }
    assert!(
        !said.iter().any(|t| t.ends_with("\ntwo") || t == "two"),
        "the source's later message: {said:?}"
    );
    for later in ["answer to two", "road one", "three"] {
        assert!(!text.contains(later), "{later} leaked: {said:?}");
    }
}

/// A summary never tells a fork what it forked before (issue 44, round 2), at the seam every selector reads
/// (`StoreSources::facts`): the exchange the point falls in is a candidate without its `did` line, and an
/// exchange begun after the point is none. The same store read as no fork is the control.
#[tokio::test]
async fn a_forks_facts_hold_no_summary_of_what_came_after_its_point() {
    use crate::view::select::sources::Exchanges as _;
    let r = rig();
    let (s, nodes) = three_turns(&r).await;
    let f = fork(&r.core, &s, &nodes[2].0); // "two", before its answer
    let (two, three) = (turn_of(&r.core, &nodes[2].0), turn_of(&r.core, &nodes[4].0));
    for (turn, line) in [(&two, "answered two in full"), (&three, "answered three")] {
        let d = crate::ladder::did::DidLine {
            turn_id: turn.clone(),
            session_id: s.clone(),
            line: line.into(),
            model: "m".into(),
            writer: 1,
            marks: crate::view::Marks {
                place: Some("local".into()),
                audience: crate::view::Audience::Private,
                author: crate::view::Author::Theseus,
                outside: false,
                sealed: false,
                veiled: false,
            },
            cost_usd: 0.0,
            at_ms: 0,
        };
        r.core
            .store
            .append(&[crate::ladder::did::did_record(&d).unwrap()])
            .unwrap();
    }
    let facts = |sessions: &crate::view::sources_store::Sessions, turn: &str| {
        crate::view::sources_store::StoreSources {
            store: &r.core.store,
            mark: None,
            recalled: Vec::new(),
            sessions,
            own: None,
        }
        .facts(turn)
        .unwrap()
    };
    let plain = crate::view::sources_store::Sessions::default();
    assert!(
        facts(&plain, &two).unwrap().did.is_some(),
        "the control: no fork, its summary"
    );
    assert!(
        facts(&plain, &three).is_some(),
        "the control: no fork, a candidate"
    );
    let cut = crate::fork::cut::Cut::of(&r.core.store, &f.session_id).unwrap();
    let forked = crate::view::sources_store::Sessions::with_cut(cut);
    let at_point = facts(&forked, &two).expect("the exchange the point falls in is a candidate");
    assert_eq!(at_point.did, None, "never by its summary");
    assert!(
        facts(&forked, &three).is_none(),
        "an exchange begun after the point is none"
    );
}

/// A fork whose point cannot be read fails its turn plainly, before anything is sent (issue 44, round 2): its
/// cut is never switched off.
#[tokio::test]
async fn a_fork_whose_point_cannot_be_read_fails_its_turn_and_sends_nothing() {
    let r = rig();
    let (s, nodes) = three_turns(&r).await;
    let position = r.core.store.get_node(&nodes[3].0).unwrap().unwrap().0;
    let mut rec = SessionRecord::new(SessionKind::Conversation, None);
    rec.forked_from = Some(Box::new(theseus_protocol::ForkOrigin {
        session: s.clone(),
        node: "nod_gone".into(),
        position,
        compilation: None,
        at_ms: 0,
    }));
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    let sent = requests(&r).len();
    let (live, _) = r.core.live_profile();
    let target = r
        .core
        .runner
        .resolve_target(&live, None, None, None)
        .unwrap();
    let mark = crate::view_gate::test_mark(&r.core.runner, &rec.session_id, true);
    let sink = EventSink::new(r.core.bus.clone(), &rec.session_id, None).with_arrival(Some(mark));
    let res = r
        .core
        .runner
        .run(TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some("on the broken road".into()),
            target,
            sink,
            author: "the CLI".into(),
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await;
    assert!(res.is_err(), "{res:?}");
    assert_eq!(requests(&r).len(), sent, "nothing was sent");
}

/// A fold is laid for a fork by the positions of the exchanges it stands for, never by their clock (issue 44,
/// round 2b). A fold over an exchange the source began after the point is not laid, though its times read as
/// before the point (the clock stepped back); a fold over an exchange before the point is laid, though its
/// times read as later; a fold one of whose exchanges cannot be read is not laid. At the ladder seam
/// (`Cut::fold_inside`, which `ForkLadder::read` asks of every fold): the rig has no ladder writer for a
/// request-level case.
#[tokio::test]
async fn a_fold_is_cut_by_its_exchanges_positions_not_by_their_clock() {
    let r = rig();
    let (s, nodes) = three_turns(&r).await;
    let f = fork(&r.core, &s, &nodes[3].0); // through the answer to "two"
    let (one, three) = (turn_of(&r.core, &nodes[0].0), turn_of(&r.core, &nodes[4].0));
    let stored = |id: &str, exchanges: Vec<String>, ms: u64| store_fold(&r.core, id, exchanges, ms);
    stored("fold.later", vec![three], 1);
    stored("fold.before", vec![one], u64::MAX / 2);
    stored("fold.unread", vec!["turn_gone".into()], 1);
    let cut = crate::fork::cut::Cut::of(&r.core.store, &f.session_id).unwrap();
    let laid = |handle: &str| cut.fold_inside(&r.core.store, handle).unwrap();
    assert!(
        !laid("fold.later"),
        "a later exchange's fold, its clock stepped back"
    );
    assert!(
        laid("fold.before"),
        "an earlier exchange's fold, its clock ahead"
    );
    assert!(!laid("fold.unread"), "a fold whose exchange cannot be read");
    let none = crate::fork::cut::Cut::default();
    assert!(
        none.fold_inside(&r.core.store, "fold.later").unwrap(),
        "no fork: every fold"
    );
}

/// A first-level fold of `exchanges` at the place `local`, stored as the ladder stores one, its times `ms`.
fn store_fold(core: &Core, id: &str, exchanges: Vec<String>, ms: u64) {
    let n = crate::ladder::fold::FoldNode {
        id: id.into(),
        place: "local".into(),
        level: 1,
        exchanges,
        first_ms: ms,
        last_ms: ms,
        his: vec![],
        counted: 0,
        children: vec![],
        title: String::new(),
        account: "what was said there".into(),
        marks: crate::view::Marks {
            place: Some("local".into()),
            audience: crate::view::Audience::Private,
            author: crate::view::Author::Theseus,
            outside: false,
            sealed: false,
            veiled: false,
        },
        model: None,
        writer: 1,
        at_ms: ms,
    };
    let records = crate::ladder::fold::records(&n).unwrap();
    core.store.append(&records).unwrap();
}

/// A summary is laid for a fork only on complete proof of position (issue 44, round 2c). An ancestor's
/// exchange whose referenced reply node is missing, one none of whose nodes can be read, and one that names
/// none are not proven before the point, so neither a fold over them nor their `did` line is laid. The same
/// exchange with every node read is.
#[tokio::test]
async fn a_summary_needs_every_node_of_its_exchange_proven_before_the_point() {
    let r = rig();
    let (s, nodes) = three_turns(&r).await;
    let f = fork(&r.core, &s, &nodes[3].0); // through the answer to "two"
    let one = turn_of(&r.core, &nodes[0].0);
    let whole = crate::exchange::exchange(&r.core.store, &one)
        .unwrap()
        .unwrap();
    let mut no_reply = whole.clone();
    no_reply.turn_id = "turn_no_reply".into();
    no_reply.reply = Some("nod_gone".into());
    let mut unread = whole.clone();
    unread.turn_id = "turn_unread".into();
    unread.messages = vec!["nod_gone_too".into()];
    unread.ask = Some("nod_gone_too".into());
    unread.reply = Some("nod_gone_three".into());
    let mut empty = whole.clone();
    empty.turn_id = "turn_empty".into();
    empty.messages = vec![];
    empty.ask = None;
    empty.reply = None;
    for ex in [&no_reply, &unread, &empty] {
        let record = crate::exchange::exchange_record(ex).unwrap();
        r.core.store.append(&[record]).unwrap();
        store_fold(
            &r.core,
            &format!("fold.{}", ex.turn_id),
            vec![ex.turn_id.clone()],
            1,
        );
    }
    store_fold(&r.core, "fold.whole", vec![one.clone()], 1);
    let cut = crate::fork::cut::Cut::of(&r.core.store, &f.session_id).unwrap();
    let store = &r.core.store;
    assert!(
        cut.proven_inside(store, &whole).unwrap(),
        "every node read, before the point"
    );
    assert!(cut.fold_inside(store, "fold.whole").unwrap());
    let read = crate::view::sources_store::Sessions::with_cut(cut.clone());
    assert!(read.summarised(store, &one).unwrap());
    for ex in [&no_reply, &unread, &empty] {
        let t = &ex.turn_id;
        assert!(!cut.proven_inside(store, ex).unwrap(), "{t}: not proven");
        assert!(
            !cut.fold_inside(store, &format!("fold.{t}")).unwrap(),
            "{t}: its fold"
        );
        assert!(!read.summarised(store, t).unwrap(), "{t}: its did line");
    }
}

/// A fork's history tool holds nothing its source said at or after the fork point, by position (issue 44): a
/// page of the fork's place lists the inherited exchange before the point and not the source's later one, and
/// an open of the later one reads as no exchange here. The same lens with no cut lists and opens it.
#[tokio::test]
async fn a_forks_history_tool_holds_nothing_past_its_point() {
    use crate::history_tool::page::Lens;
    let r = rig();
    let (s, nodes) = three_turns(&r).await;
    let f = fork(&r.core, &s, &nodes[3].0); // through the answer to "two"
    turn(&r.core, &f.session_id, "other road").await;
    let (one, three) = (turn_of(&r.core, &nodes[0].0), turn_of(&r.core, &nodes[4].0));
    let lens = |cut| Lens {
        store: &r.core.store,
        here: "local".into(),
        in_view: None,
        words: None,
        span: None,
        cut,
        scope: None,
        cuts: crate::scrub::Cuts::none(),
    };
    let args = crate::view::history::HistoryArgs::default();
    let page = |l: &Lens<'_>| -> Vec<String> {
        let walked = l.walk(&args, None, 0).unwrap();
        walked
            .page
            .lines
            .iter()
            .map(|x| x.handle.0.clone())
            .collect()
    };
    let plain = lens(Default::default());
    let cut = crate::fork::cut::Cut::of(&r.core.store, &f.session_id).unwrap();
    let forked = lens(cut);
    let all = page(&plain);
    assert!(
        all.contains(&one) && all.contains(&three),
        "the control: {all:?}"
    );
    let got = page(&forked);
    assert!(
        got.contains(&one),
        "the inherited exchange before the point: {got:?}"
    );
    assert!(
        !got.contains(&three),
        "the source's later exchange: {got:?}"
    );
    let later = crate::view::Handle(three);
    assert!(crate::history_tool::open::open(&plain, &later).is_ok());
    assert!(
        crate::history_tool::open::open(&forked, &later).is_err(),
        "a fork opens nothing past its point"
    );
}

/// A fork asking a time span past its point gets nothing past it (D6a step 1): the span reads every exchange
/// of today, and the fork's cut, behind the same seam (`Lens::admitted`), still holds the source's later turn.
#[tokio::test]
async fn a_fork_asking_a_span_gets_nothing_past_its_point() {
    let r = rig();
    let (s, nodes) = three_turns(&r).await;
    let f = fork(&r.core, &s, &nodes[3].0); // through the answer to "two"
    turn(&r.core, &f.session_id, "other road").await;
    let (one, three) = (turn_of(&r.core, &nodes[0].0), turn_of(&r.core, &nodes[4].0));
    let pov = crate::view::PointOfView {
        anchor: crate::view::Anchor::Place("local".into()),
        person: None,
        present: crate::view::Present::Open,
        at_hand: None,
        now_ms: theseus_protocol::now_unix_ms(),
    };
    let a: crate::view::history::HistoryArgs =
        serde_json::from_value(json!({"when": "today"})).unwrap();
    let b = crate::history_tool::Board::default();
    let got = crate::history_tool::answer(&r.core, &b, (&f.session_id, "t"), &pov, &a)
        .await
        .unwrap()
        .text;
    assert!(got.contains(&one), "the inherited turn: {got}");
    assert!(!got.contains(&three), "the source's later turn: {got}");
}

/// A fork opening its source session by id reads it as no session here (D6a step 2): the cut names it, and
/// the source's later turn is past the point; the fork's own session opens.
#[tokio::test]
async fn a_fork_opening_its_sources_session_reads_no_session() {
    let r = rig();
    let (s, nodes) = three_turns(&r).await;
    let f = fork(&r.core, &s, &nodes[3].0);
    turn(&r.core, &f.session_id, "other road").await;
    let pov = crate::view::PointOfView {
        anchor: crate::view::Anchor::Place("local".into()),
        person: None,
        present: crate::view::Present::Open,
        at_hand: None,
        now_ms: theseus_protocol::now_unix_ms(),
    };
    let b = crate::history_tool::Board::default();
    let open = |id: &str| -> crate::view::history::HistoryArgs {
        serde_json::from_value(json!({ "open": id })).unwrap()
    };
    let src = crate::history_tool::answer(&r.core, &b, (&f.session_id, "t"), &pov, &open(&s))
        .await
        .unwrap_err();
    assert!(src.contains("no session"), "{src}");
    let own = crate::history_tool::answer(
        &r.core,
        &b,
        (&f.session_id, "t"),
        &pov,
        &open(&f.session_id),
    )
    .await
    .unwrap()
    .text;
    assert!(own.contains("other road"), "{own}");
    let plain = crate::history_tool::answer(&r.core, &b, (&s, "t"), &pov, &open(&s))
        .await
        .unwrap()
        .text;
    assert!(
        plain.contains("three"),
        "the control, from the source: {plain}"
    );
}

/// A fork's history tool admits an ancestor's exchange only on complete proof of position (XF7, issue 44),
/// through its entry point (`history_tool::answer`, as `Board::run` dispatches it): an exchange whose ask is
/// before the point but whose referenced reply is missing is not proven inside, so neither its line nor its
/// stored `did` line (which may summarise that later reply) is paged to the fork, and its handle opens as no
/// exchange here. The plain controls stay: the source sees it and opens it.
#[tokio::test]
async fn a_forks_history_tool_needs_proof_and_a_missing_reply_admits_no_summary() {
    let r = rig();
    let (s, nodes) = three_turns(&r).await;
    let f = fork(&r.core, &s, &nodes[3].0); // through the answer to "two"
    let one = turn_of(&r.core, &nodes[0].0);
    let whole = crate::exchange::exchange(&r.core.store, &one)
        .unwrap()
        .unwrap();
    let mut no_reply = whole.clone();
    no_reply.turn_id = "turn_no_reply".into();
    no_reply.reply = Some("nod_gone".into());
    no_reply.start_ms = whole.start_ms + 1;
    let summary = crate::ladder::did::DidLine {
        turn_id: no_reply.turn_id.clone(),
        session_id: s.clone(),
        line: "THE LATER REPLY'S SUMMARY".into(),
        model: "m".into(),
        writer: 1,
        marks: crate::view::Marks {
            place: Some(whole.place.clone()),
            audience: crate::view::Audience::Private,
            author: crate::view::Author::Theseus,
            outside: false,
            sealed: false,
            veiled: false,
        },
        cost_usd: 0.0,
        at_ms: 0,
    };
    let mut recs = vec![
        crate::exchange::exchange_record(&no_reply).unwrap(),
        crate::ladder::did::did_record(&summary).unwrap(),
    ];
    recs.extend(crate::exchange::touch(
        crate::exchange::Root::Place(&whole.place),
        &no_reply.turn_id,
        &s,
        no_reply.start_ms,
    ));
    r.core.store.append(&recs).unwrap();
    // Asked from another place, naming the exchange's: a page with nothing in view to start before.
    let pov = crate::view::PointOfView {
        anchor: crate::view::Anchor::Place("elsewhere".into()),
        person: None,
        present: crate::view::Present::Open,
        at_hand: None,
        now_ms: theseus_protocol::now_unix_ms(),
    };
    let b = crate::history_tool::Board::default();
    let call = |session: String, v: Value| {
        let a: crate::view::history::HistoryArgs = serde_json::from_value(v).unwrap();
        let (core, b, pov) = (r.core.clone(), &b, &pov);
        async move { crate::history_tool::answer(&core, b, (&session, "t"), pov, &a).await }
    };
    let page = json!({ "place": whole.place });
    let open = json!({ "open": "turn_no_reply" });
    let plain = call(s.clone(), page.clone()).await.unwrap().text;
    assert!(
        plain.contains("turn_no_reply") && plain.contains("THE LATER REPLY'S SUMMARY"),
        "the control, from the source: {plain}"
    );
    assert!(
        call(s.clone(), open.clone()).await.is_ok(),
        "the source opens it"
    );
    let forked = call(f.session_id.clone(), page).await.unwrap().text;
    assert!(
        forked.contains(&one),
        "the proven exchange before the point: {forked}"
    );
    assert!(!forked.contains("turn_no_reply"), "{forked}");
    assert!(!forked.contains("THE LATER REPLY'S SUMMARY"), "{forked}");
    let shut = call(f.session_id.clone(), open).await.unwrap_err();
    assert!(shut.contains("no exchange"), "{shut}");
}

/// A words search in a fork whose bounded hits were filled by the source's words after the point never says
/// "history is exhausted" (XF7, issue 44): the search's top hits (here 120, past its bound of 100) are all
/// past the point, crowding out an earlier match, so the walk shows nothing and says the search was cut at the
/// fork point and may be incomplete. The same hits in the source are shown; the earlier match alone, found, is
/// shown to the fork and ends as exhausted, as nothing was held.
#[tokio::test]
async fn a_forks_words_search_crowded_past_its_point_says_it_may_be_incomplete() {
    use crate::history_tool::page::{render, Lens};
    use crate::view::history::PageEnd;
    let r = rig();
    let (s, nodes) = three_turns(&r).await;
    let f = fork(&r.core, &s, &nodes[3].0); // through the answer to "two"
    let one = turn_of(&r.core, &nodes[0].0);
    let place = crate::exchange::exchange(&r.core.store, &one)
        .unwrap()
        .unwrap()
        .place;
    // The source's later words, written after the fork: every node past the point.
    let mut later = std::collections::HashSet::new();
    for i in 0..120u64 {
        let turn = format!("turn_later_{i:03}");
        let ask = crate::node::Node::user(&s, Some(&turn), "owner", &format!("tide {i}"));
        let at = theseus_protocol::now_unix_ms() + i;
        let x = crate::exchange::Exchange {
            turn_id: turn.clone(),
            session_id: s.clone(),
            place: place.clone(),
            private: true,
            opened_by: crate::exchange::Opener::Message,
            person: Some(crate::exchange::OWNER.into()),
            start_ms: at,
            end_ms: Some(at),
            ask: Some(ask.id.clone()),
            reply: None,
            messages: vec![ask.id.clone()],
            stop: Some("end_turn".into()),
            rebuilt: false,
        };
        let mut recs = vec![
            ask.record().unwrap(),
            crate::exchange::exchange_record(&x).unwrap(),
        ];
        recs.extend(crate::exchange::touch(
            crate::exchange::Root::Place(&place),
            &turn,
            &s,
            at,
        ));
        r.core.store.append(&recs).unwrap();
        later.insert(turn);
    }
    let lens = |cut, words| Lens {
        store: &r.core.store,
        here: place.clone(),
        in_view: None,
        words: Some(words),
        span: None,
        cut,
        scope: None,
        cuts: crate::scrub::Cuts::none(),
    };
    let args = crate::view::history::HistoryArgs::default();
    let cut = crate::fork::cut::Cut::of(&r.core.store, &f.session_id).unwrap();
    let crowded = lens(cut.clone(), later.clone())
        .walk(&args, None, 0)
        .unwrap()
        .page;
    assert!(
        crowded.lines.is_empty(),
        "nothing past the point: {crowded:?}"
    );
    assert_eq!(crowded.end, PageEnd::SearchCut);
    let said = render(
        &crowded,
        (&place, None),
        0,
        None,
        &crate::scrub::Cuts::none(),
    );
    assert!(said.contains("may be incomplete"), "{said}");
    assert!(!said.contains("exhausted"), "{said}");
    let source = lens(Default::default(), later)
        .walk(&args, None, 0)
        .unwrap()
        .page;
    assert_eq!(
        source.lines.len(),
        100,
        "the control: the source sees them, a page at a time"
    );
    let earlier = lens(cut, [one.clone()].into())
        .walk(&args, None, 0)
        .unwrap()
        .page;
    let handles: Vec<_> = earlier.lines.iter().map(|l| l.handle.0.clone()).collect();
    assert_eq!(handles, vec![one]);
    assert_eq!(earlier.end, PageEnd::HistoryExhausted, "nothing held");
}

/// A stand-in index answering every query with `nodes`, ranked in the order given.
fn index_ranked(core: &Arc<Core>, nodes: Vec<String>) -> crate::recall::Ask {
    use theseus_protocol::index::{IndexHit, IndexQueryResult, IndexSourceRank};
    let store = core.store.clone();
    Arc::new(move |_p| -> crate::recall::AskFuture {
        let hits = nodes
            .iter()
            .filter_map(|id| store.get_node(id).unwrap())
            .enumerate()
            .map(|(i, (position, n))| IndexHit {
                text: crate::recall::text_of(&n),
                time_ms: n.created_at_ms,
                node_id: n.id,
                chunk: 0,
                session_id: n.session_id,
                position,
                kind: "user_message".into(),
                origin: "operator".into(),
                author: None,
                place: None,
                tool: None,
                external: false,
                entities_matched: vec![],
                entities: Vec::new(),
                sources: [(
                    "bm25".to_string(),
                    IndexSourceRank {
                        rank: i + 1,
                        score: 1.0,
                    },
                )]
                .into(),
                fused: 1.0 / (61 + i) as f64,
            })
            .collect();
        Box::pin(async move {
            Ok(IndexQueryResult {
                hits,
                indexed_through: 0,
                lag: Default::default(),
                timings: Default::default(),
                skipped: Default::default(),
                weights: Default::default(),
            })
        })
    })
}

/// A fork's words search that starts from a cursor, or from what is in view, is told it may be incomplete when
/// the cut held any of its hits (XF8, issue 44), through the tool's entry point with recall's own ranking and cap:
/// the source's 120 matches after the point outrank its one earlier match, which the cap of 100 leaves out; the
/// fork asks with `before` older than every post-point hit (and once with the ordinary start), so its walk never
/// meets a held hit, and the page must still say "may be incomplete", never "exhausted". The source pages its hits.
#[tokio::test]
async fn a_forks_words_search_from_a_cursor_crowded_past_its_point_says_it_may_be_incomplete() {
    let r = rig();
    let (s, nodes) = three_turns(&r).await;
    let f = fork(&r.core, &s, &nodes[3].0); // through the answer to "two"
    let one = turn_of(&r.core, &nodes[0].0);
    let place = crate::exchange::exchange(&r.core.store, &one)
        .unwrap()
        .unwrap()
        .place;
    let first_later = theseus_protocol::now_unix_ms() + 60_000;
    let mut ranked = Vec::new();
    for i in 0..120u64 {
        let turn = format!("turn_tide_{i:03}");
        let ask = crate::node::Node::user(&s, Some(&turn), "owner", &format!("tide {i}"));
        let at = first_later + i;
        let x = crate::exchange::Exchange {
            turn_id: turn.clone(),
            session_id: s.clone(),
            place: place.clone(),
            private: true,
            opened_by: crate::exchange::Opener::Message,
            person: Some(crate::exchange::OWNER.into()),
            start_ms: at,
            end_ms: Some(at),
            ask: Some(ask.id.clone()),
            reply: None,
            messages: vec![ask.id.clone()],
            stop: Some("end_turn".into()),
            rebuilt: false,
        };
        let mut recs = vec![
            ask.record().unwrap(),
            crate::exchange::exchange_record(&x).unwrap(),
        ];
        recs.extend(crate::exchange::touch(
            crate::exchange::Root::Place(&place),
            &turn,
            &s,
            at,
        ));
        r.core.store.append(&recs).unwrap();
        ranked.push(ask.id.clone());
    }
    // The earlier, eligible match ranks last: the cap leaves it out.
    ranked.push(nodes[0].0.clone());
    r.core.runner.memory.set_ask(index_ranked(&r.core, ranked));
    let pov = crate::view::PointOfView {
        anchor: crate::view::Anchor::Place(place.clone()),
        person: None,
        present: crate::view::Present::Open,
        at_hand: None,
        now_ms: theseus_protocol::now_unix_ms(),
    };
    let b = crate::history_tool::Board::default();
    let call = |session: String, v: Value| {
        let a: crate::view::history::HistoryArgs = serde_json::from_value(v).unwrap();
        let (core, b, pov) = (r.core.clone(), &b, &pov);
        async move { crate::history_tool::answer(&core, b, (&session, "t"), pov, &a).await }
    };
    // A cursor with an empty turn id begins its page WITH its millisecond: one before the first post-point
    // hit's, so the walk meets none of them.
    let before = json!({"words": "tide", "before": {"at_ms": first_later - 1, "turn_id": ""}});
    let cut = call(f.session_id.clone(), before).await.unwrap().text;
    assert!(cut.contains("may be incomplete"), "from a cursor: {cut}");
    assert!(!cut.contains("exhausted"), "from a cursor: {cut}");
    let bare = call(f.session_id.clone(), json!({"words": "tide"}))
        .await
        .unwrap()
        .text;
    assert!(
        bare.contains("may be incomplete"),
        "the ordinary start: {bare}"
    );
    assert!(!bare.contains("exhausted"), "the ordinary start: {bare}");
    assert!(
        !bare.contains("turn_tide_"),
        "nothing past the point: {bare}"
    );
    // The source's ordinary start is below what it has in view, which these later writes are not part of:
    // its control starts above every hit.
    let above = json!({"words": "tide", "before": {"at_ms": first_later + 1_000, "turn_id": ""}});
    let source = call(s.clone(), above).await.unwrap().text;
    // The cap keeps the 100 best-ranked hits, the first 100 written (tide 0 to 99); the source pages them.
    assert!(
        source.contains("turn_tide_099") && source.contains("turn_tide_000"),
        "the control: {source}"
    );
    assert!(!source.contains("turn_tide_119"), "past the cap: {source}");
    assert!(!source.contains("may be incomplete"), "{source}");
}

#[tokio::test]
async fn repeated_item_labels_reach_existing_and_future_forks() {
    use crate::content::{Flag, ItemKind, Label};
    use theseus_judge::band::Band;
    let r = rig();
    let (sid, nodes) = three_turns(&r).await;
    let label = |node: &str| Label {
        flag: Flag::Confidential,
        node: Some(node.into()),
        kind: ItemKind::ToolResult,
        source: "fixture".into(),
        confidence: 1.0,
        band: Band::Act,
        at_ms: 9,
        judgment: String::new(),
    };
    // The source's aggregate points past the fork, but the earlier item later gets the same label.
    r.core
        .runner
        .judge
        .land_deterministic(&sid, vec![(label(&nodes[4].0), 1)]);
    let existing = fork(&r.core, &sid, &nodes[3].0);
    r.core
        .runner
        .judge
        .land_deterministic(&sid, vec![(label(&nodes[1].0), 1)]);
    let future = fork(&r.core, &sid, &nodes[3].0);
    for id in [&existing.session_id, &future.session_id] {
        let rec: SessionRecord = r.core.store.get_session(id).unwrap().unwrap();
        assert!(
            rec.data.as_ref().is_some_and(|d| d.has(Flag::Confidential)),
            "an included labeled item must label its fork"
        );
    }
}

// ------------------------------------------------------------ records under `labels.` that do not read (issue 44)

/// Words in a record that does not read: an error or a log that quoted its payload would show them.
const UNREAD: &str = "UNREAD-WORDS-k4";

/// A label on `node`, as the scenario above raises it.
fn label_of(node: &str, flag: crate::content::Flag) -> crate::content::Label {
    crate::content::Label {
        flag,
        node: Some(node.into()),
        kind: crate::content::ItemKind::ToolResult,
        source: "fixture".into(),
        confidence: 1.0,
        band: theseus_judge::band::Band::Act,
        at_ms: 9,
        judgment: String::new(),
    }
}

/// The same, as stored JSON.
fn label_on(node: &str, flag: crate::content::Flag) -> Value {
    serde_json::to_value(label_of(node, flag)).unwrap()
}

/// The source's aggregate label on a node past the fork (`nodes[4]`), as the scenario above has it, so the
/// fork's flag can come only from the earlier item's record.
fn aggregate_past_the_fork(r: &Rig, sid: &str, nodes: &[(String, String)]) {
    let label = label_of(&nodes[4].0, crate::content::Flag::Confidential);
    (r.core.runner.judge).land_deterministic(sid, vec![(label, 1)]);
}

/// A fork of `sid` at `at` is refused for the record under `key`, in fixed words naming the key alone, and
/// writes no session.
fn refused_for(r: &Rig, sid: &str, at: &str, key: &str) {
    let sessions = || r.core.store.list_sessions::<Value>().unwrap().len();
    let before = sessions();
    let params = SessionForkParams {
        session: sid.into(),
        at: at.into(),
        ..Default::default()
    };
    let err = (r.core.session_fork(params, Surface::Cli)).expect_err("the fork is refused");
    assert_eq!(err.code, theseus_protocol::error_code::REFUSED, "{err:?}");
    assert!(err.message.contains(key), "{err:?}");
    for words in ["missing field", "expected", UNREAD] {
        assert!(!err.message.contains(words), "{words}: {err:?}");
    }
    assert_eq!(sessions(), before, "no fork is written");
}

/// The scenario of `repeated_item_labels_reach_existing_and_future_forks`, with the earlier item's record
/// holding one more raised label, `injection`, that does not read (it has no `source`): the fork would hold
/// that item, so it is refused, and nothing of a fork is written. A fork never holds fewer labels than its
/// source may (issue 44).
#[tokio::test]
async fn a_fork_is_refused_when_an_item_it_holds_has_a_label_that_does_not_read() {
    use crate::content::Flag;
    let r = rig();
    let (sid, nodes) = three_turns(&r).await;
    aggregate_past_the_fork(&r, &sid, &nodes);
    let earlier = label_of(&nodes[1].0, Flag::Confidential);
    (r.core.runner.judge).land_deterministic(&sid, vec![(earlier, 1)]);
    let key = crate::item_labels::key(&nodes[1].0);
    let mut item: Value = r.core.store.get_meta(&key).unwrap().expect("the item's record");
    let mut bad = label_on(&nodes[1].0, Flag::Injection);
    bad.as_object_mut().unwrap().remove("source");
    bad["note"] = json!(UNREAD);
    item["raised"].as_array_mut().unwrap().push(bad);
    r.core.store.put_meta(&key, &item).unwrap();
    refused_for(&r, &sid, &nodes[3].0, &key);
}

/// Beside it (issue 44): the earlier item's record in the older shape (a removal kept on it names no session)
/// reads, and the fork keeps its flag; a record that does not read and has no `node`, whose key names no node
/// in the store, and one whose key names a node of another session, are passed over and do not stop the fork.
#[tokio::test]
async fn a_fork_reads_an_older_items_record_and_passes_over_records_naming_no_item_it_holds() {
    use crate::content::Flag;
    let r = rig();
    let (sid, nodes) = three_turns(&r).await;
    aggregate_past_the_fork(&r, &sid, &nodes);
    let older = json!({"node": nodes[1].0, "kind": "tool_result", "source": "fixture", "entered_ms": 1,
                       "raised": [label_on(&nodes[1].0, Flag::Confidential)],
                       "removed": [{"flag": "secret", "by": "the owner", "via": "cli", "at_ms": 2}]});
    let store = &r.core.store;
    store.put_meta(&crate::item_labels::key(&nodes[1].0), &older).unwrap();
    let other = new_session(&r.core, None);
    turn(&r.core, &other, "elsewhere").await;
    let elsewhere = crate::item_labels::key(&nodes_of(&r.core, &other)[0].0);
    let foreign = json!({"raised": [label_on(&nodes[0].0, Flag::Secret)], "note": UNREAD});
    store.put_meta("labels.no-such-node", &foreign).unwrap();
    store.put_meta(&elsewhere, &json!({"note": UNREAD})).unwrap();
    let f = fork(&r.core, &sid, &nodes[3].0);
    let rec: SessionRecord = store.get_session(&f.session_id).unwrap().unwrap();
    let flags = rec.data.as_deref().cloned().unwrap_or_default();
    assert!(flags.has(Flag::Confidential), "the older record's: {flags:?}");
    assert!(!flags.has(Flag::Secret), "nothing is read from one passed over: {flags:?}");
}

/// A record that does not read, whose key names an item the fork would hold but which has no `node` (an object
/// of the kind a rebuild failed on, placed there), refuses the fork as a damaged item's record does; so does
/// one under another key whose `node` names such an item. With neither, the fork is made (issue 44).
#[tokio::test]
async fn a_fork_is_refused_when_a_record_that_does_not_read_names_an_item_it_holds() {
    let r = rig();
    let (sid, nodes) = three_turns(&r).await;
    let placed = [
        (
            crate::item_labels::key(&nodes[2].0),
            json!({"kind": "operator_message", "source": "cli", "entered_ms": 1, "note": UNREAD}),
        ),
        (
            "labels.no-such-node".to_string(),
            json!({"node": nodes[0].0, "note": UNREAD}),
        ),
    ];
    for (key, v) in &placed {
        r.core.store.put_meta(key, v).unwrap();
        refused_for(&r, &sid, &nodes[3].0, key);
        r.core.store.delete_meta(key).unwrap();
    }
    fork(&r.core, &sid, &nodes[3].0);
}
