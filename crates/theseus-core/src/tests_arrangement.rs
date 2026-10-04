//! The arrangement on `task.create` through the whole core (M5 step 27,
//! theseus-vug.2): the refusal without one, quotes resolved in the calling
//! session's transcript (exact, unique, at least 20 characters; no match;
//! ambiguous), the fidelity check and its ack, and the child's compilation:
//! the brief, then each piece verbatim with its author, time, and session, a
//! superseded piece by reference only, and the same prefix on the task's
//! later turns. The pure rules' own tests are `arrangement::tests`.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_kernel::{ExecState, Execution};
use theseus_protocol::{SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::ledger::LedgerRow;
use crate::node::{Body, Node, ResultStatus};
use crate::policy::Posture;
use crate::provider::{
    DeltaSink, FakeProvider, Provider, ProviderFuture, ProviderRequest, Scripted,
};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

const PLACE: &str = "dm:42";
const TARGET: &str = "discord:dm:42";

/// A stand-in model that answers each request from the request itself, and
/// keeps every request.
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
        Box::pin(async move {
            self.requests.lock().unwrap().push(req.clone());
            let f = FakeProvider::scripted(vec![(self.script)(req)]);
            f.stream_message(req, on_delta).await
        })
    }
}

/// A user message's text blocks.
fn blocks(m: &Value) -> Vec<String> {
    match &m["content"] {
        Value::String(s) => vec![s.clone()],
        Value::Array(b) => b
            .iter()
            .filter(|b| !crate::task_graph::view::is_view(b))
            .filter_map(|b| b["text"].as_str().map(str::to_string))
            .collect(),
        _ => vec![],
    }
}

/// The first text block of the first user message: a task's brief.
fn first_user(req: &ProviderRequest) -> String {
    req.messages
        .iter()
        .find(|m| m["role"] == "user")
        .and_then(|m| blocks(m).into_iter().next())
        .unwrap_or_default()
}

fn last_user(req: &ProviderRequest) -> String {
    req.messages
        .iter()
        .rev()
        .find(|m| m["role"] == "user")
        .map(|m| blocks(m).join("\n"))
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
    model: Arc<Model>,
    _dir: tempfile::TempDir,
}

fn rig(script: impl Fn(&ProviderRequest) -> Scripted + Send + Sync + 'static) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = Posture::Notify;
    let store = Store::open(&dir.path().join("store")).unwrap();
    let model = Arc::new(Model {
        script: Box::new(script),
        requests: Mutex::default(),
    });
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, model.clone(), store)).unwrap();
    core.runner.place_rule.bind_one(crate::places::BoundPlace {
        target: TARGET.into(),
        name: "DM".into(),
        private: false,
        ..Default::default()
    });
    tokio::spawn(crate::harness::drive(core.clone()));
    Rig {
        core,
        model,
        _dir: dir,
    }
}

fn parent_session(core: &Arc<Core>) -> String {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    core.outbox.bind_place(PLACE, &rec.session_id).unwrap();
    rec.session_id
}

async fn turn(core: &Arc<Core>, sid: &str, input: &str) -> TurnSubmitResult {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = EventSink::new(core.bus.clone(), sid, None);
    core.runner
        .run(TurnRequest {
            prompt: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
        })
        .await
        .unwrap()
}

async fn until(what: &str, secs: u64, mut f: impl FnMut() -> bool) {
    let t0 = Instant::now();
    while !f() {
        assert!(
            t0.elapsed() < Duration::from_secs(secs),
            "no {what} in {secs} s"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn nodes(core: &Core, sid: &str) -> Vec<Node> {
    core.store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .map(|(_, n)| n)
        .collect()
}

/// Each `task.create` result in a session, in order: its status, its
/// content, and its meta.
fn creates(core: &Core, sid: &str) -> Vec<(ResultStatus, String, Value)> {
    nodes(core, sid)
        .into_iter()
        .filter_map(|n| match n.body {
            Body::ToolResult {
                tool,
                status,
                content,
                meta,
                ..
            } if tool == crate::task::CREATE => Some((status, content, meta)),
            _ => None,
        })
        .collect()
}

fn tasks(core: &Core) -> Vec<Execution> {
    core.kernel.tasks(None).unwrap()
}

fn rows(core: &Core, kind: &str) -> Vec<LedgerRow> {
    core.store
        .ledger_tail::<LedgerRow>(100_000)
        .unwrap()
        .into_iter()
        .map(|(_, r)| r)
        .filter(|r| r.kind == kind)
        .collect()
}

/// The requests a task's turns made: those whose first user message is a
/// brief.
fn task_requests(model: &Model) -> Vec<ProviderRequest> {
    model
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter(|q| first_user(q).starts_with("[Task "))
        .cloned()
        .collect()
}

/// A `task.create` call with `input`.
fn create(input: Value) -> Scripted {
    Scripted::tools("Starting it.", &[("t_task", "task_create", input)])
}

/// The id of the operator's message whose text is `text`.
fn said(core: &Core, sid: &str, text: &str) -> Node {
    nodes(core, sid)
        .into_iter()
        .find(|n| matches!(&n.body, Body::UserMessage { text: t, .. } if t == text))
        .unwrap()
}

/// The refusal without an arrangement, and without a piece that defines the
/// work: the call fails with the reason, opens nothing, and is ledgered.
#[tokio::test]
async fn a_task_without_an_arrangement_is_refused_and_says_why() {
    let r = rig(|req| {
        if answers_a_call(req) {
            return Scripted::text("Noted.");
        }
        let last = last_user(req);
        if last.starts_with("BARE") {
            return create(json!({"brief": "Count the gulls on the pier."}));
        }
        if last.starts_with("CONTEXT") {
            return create(
                json!({"brief": "Count the gulls on the pier.", "arrangement": {
                "pieces": [{"quote": "count the gulls on the pier today", "role": "context"}]}}),
            );
        }
        if last.starts_with("SUPERSEDED") {
            return create(
                json!({"brief": "Count the gulls on the pier.", "arrangement": {
                "pieces": [
                    {"quote": "count the gulls on the pier today", "role": "objective"},
                    {"quote": "SUPERSEDED: count the gulls", "role": "context"}],
                "supersedes": [[0, 1]]}}),
            );
        }
        Scripted::text("Noted.")
    });
    let sid = parent_session(&r.core);
    for ask in [
        "BARE: please count the gulls on the pier today",
        "CONTEXT: please count the gulls on the pier today",
        "SUPERSEDED: count the gulls, as I said",
    ] {
        turn(&r.core, &sid, ask).await;
    }
    let got = creates(&r.core, &sid);
    assert_eq!(got.len(), 3, "{got:?}");
    for (status, content, _) in &got {
        assert_eq!(*status, ResultStatus::Error, "{content}");
        assert!(
            content.starts_with(crate::arrangement::REFUSAL),
            "{content}"
        );
    }
    assert!(got[0].1.contains("`arrangement.pieces`"), "{}", got[0].1);
    assert!(got[1].1.contains("`objective` or `design`"), "{}", got[1].1);
    assert!(tasks(&r.core).is_empty(), "no task opened");
    let refused = rows(&r.core, "task.arrangement_refused");
    let classes: Vec<&str> = refused
        .iter()
        .map(|r| r.data["class"].as_str().unwrap())
        .collect();
    assert_eq!(classes, ["missing", "no_objective", "no_objective"]);
    assert!(rows(&r.core, "task.arranged").is_empty());
}

/// Quotes resolve in the calling session's transcript: exactly, uniquely,
/// at least 20 characters. No match, an ambiguous quote (with each
/// candidate's id, author, and time), a short one, and a quote of another
/// session's message all fail and say why; a unique one starts the task, and
/// the result lists what resolved.
#[tokio::test]
async fn quotes_resolve_to_one_node_of_this_session_or_fail_with_the_reason() {
    let r = rig(|req| {
        if answers_a_call(req) {
            return Scripted::text("Noted.");
        }
        let quote = match last_user(req).as_str() {
            "AMBIGUOUS, please" => "harbour wall needs new granite blocks",
            "NO MATCH, please" => "harbour wall needs old granite blocks",
            "SHORT, please" => "granite",
            "ELSEWHERE, please" => "the tide tables for the outer buoy",
            "UNIQUE, please" => "new  granite blocks\non the north side",
            _ => return Scripted::text("Noted."),
        };
        create(json!({"brief": "Order the granite.", "arrangement": {
            "pieces": [{"quote": quote, "role": "objective"}]}}))
    });
    let other = parent_session(&r.core);
    turn(&r.core, &other, "Fetch the tide tables for the outer buoy.").await;
    let sid = parent_session(&r.core);
    let first = "The harbour wall needs new granite blocks on the north side.";
    turn(&r.core, &sid, first).await;
    turn(
        &r.core,
        &sid,
        "Again: the harbour wall needs new granite blocks, said twice.",
    )
    .await;
    for ask in [
        "AMBIGUOUS, please",
        "NO MATCH, please",
        "SHORT, please",
        "ELSEWHERE, please",
    ] {
        turn(&r.core, &sid, ask).await;
    }
    let got = creates(&r.core, &sid);
    assert_eq!(got.len(), 4);
    assert!(got.iter().all(|(s, _, _)| *s == ResultStatus::Error));
    assert!(tasks(&r.core).is_empty(), "nothing opened");
    // Ambiguous: both candidates, with who wrote each and when.
    let one = said(&r.core, &sid, first);
    let two = said(
        &r.core,
        &sid,
        "Again: the harbour wall needs new granite blocks, said twice.",
    );
    let ambiguous = &got[0].1;
    assert!(ambiguous.contains("matches 2 messages"), "{ambiguous}");
    for n in [&one, &two] {
        assert!(ambiguous.contains(&n.id), "{ambiguous}");
        assert!(
            ambiguous.contains(&crate::arrangement::utc(n.created_at_ms)),
            "{ambiguous}"
        );
    }
    assert!(ambiguous.contains("the operator (test)"), "{ambiguous}");
    assert!(got[1].1.contains("matches no message"), "{}", got[1].1);
    assert!(got[2].1.contains("under the 20"), "{}", got[2].1);
    // Another session's words are not this one's: the place rule.
    assert!(got[3].1.contains("matches no message"), "{}", got[3].1);
    let classes: Vec<String> = rows(&r.core, "task.arrangement_refused")
        .iter()
        .map(|r| r.data["class"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(classes, ["ambiguous", "no_match", "short", "no_match"]);

    // Unique, with its whitespace read as one space: started, and the
    // result lists the node, its author, its time, and its first line.
    turn(&r.core, &sid, "UNIQUE, please").await;
    let (status, content, meta) = creates(&r.core, &sid).pop().unwrap();
    assert_eq!(status, ResultStatus::Ok, "{content}");
    assert!(content.contains(&one.id), "{content}");
    assert!(content.contains("the operator (test)"), "{content}");
    assert!(
        content.contains(&crate::arrangement::utc(one.created_at_ms)),
        "{content}"
    );
    assert!(content.contains(first), "{content}");
    assert_eq!(meta["arrangement"][0]["node"], one.id.as_str());
    assert_eq!(tasks(&r.core).len(), 1);
}

/// The fidelity check: a brief under 200 characters, from a session with
/// more than ten of the person's messages since its last task, with one
/// piece, fails, asking for the design or the ack; with the ack it starts,
/// and the ack is ledgered and shown on the task.
#[tokio::test]
async fn a_one_line_brief_from_a_long_discussion_is_flagged_until_acknowledged() {
    let r = rig(|req| {
        if answers_a_call(req) || first_user(req).starts_with("[Task ") {
            return Scripted::text("Noted.");
        }
        let one = json!([{"quote": "the lamp room needs a new lens", "role": "objective"}]);
        match last_user(req).as_str() {
            "GO, please" => create(json!({"brief": "Fix the lens.",
                "arrangement": {"pieces": one}})),
            "ACK, please" => create(json!({"brief": "Fix the lens.", "fidelity_ack": true,
                "arrangement": {"pieces": one}})),
            _ => Scripted::text("Noted."),
        }
    });
    let sid = parent_session(&r.core);
    turn(&r.core, &sid, "Point one: the lamp room needs a new lens.").await;
    for i in 2..=10 {
        turn(&r.core, &sid, &format!("Point {i} about the lamp.")).await;
    }
    turn(&r.core, &sid, "GO, please").await;
    let got = creates(&r.core, &sid);
    assert_eq!(got[0].0, ResultStatus::Error);
    assert!(got[0].1.contains("the fidelity check"), "{}", got[0].1);
    assert!(got[0].1.contains("`fidelity_ack: true`"), "{}", got[0].1);
    assert!(got[0].1.contains("11 messages"), "{}", got[0].1);
    assert!(tasks(&r.core).is_empty());
    turn(&r.core, &sid, "ACK, please").await;
    let (status, content, meta) = creates(&r.core, &sid).pop().unwrap();
    assert_eq!(status, ResultStatus::Ok, "{content}");
    assert!(
        content.contains("The fidelity check was acknowledged"),
        "{content}"
    );
    assert_eq!(meta["fidelity_ack"], true);
    let arranged = rows(&r.core, "task.arranged");
    assert_eq!(arranged.len(), 1);
    assert_eq!(arranged[0].data["fidelity_ack"], true);
    assert_eq!(arranged[0].data["human_messages"], 12);
    let refused = rows(&r.core, "task.arrangement_refused");
    assert_eq!(refused.len(), 1);
    assert_eq!(refused[0].data["class"], "fidelity");
    let shown = r.core.tasks(Some(&sid), None).unwrap();
    let a = shown[0].arrangement.as_ref().expect("its arrangement");
    assert!(a.fidelity_ack);
    assert_eq!(a.pieces.len(), 1);
    // The count starts again after a task: a short brief with one piece
    // passes now.
    turn(&r.core, &sid, "GO, please").await;
    assert_eq!(creates(&r.core, &sid).pop().unwrap().0, ResultStatus::Ok);
}

/// The child's first compilation renders the brief, then each admitted
/// piece's full text with its author, time, and origin session (a trusted
/// one marked), and a superseded piece by reference only, never its text.
/// The arrangement node has a `derived_from` edge to each piece's node, the
/// task's surfaces list the pieces, and the prefix is the same on the task's
/// later loop and its later turn (a wake's).
#[tokio::test]
async fn the_childs_compilation_renders_each_piece_verbatim_after_the_brief() {
    const RED: &str = "OBJECTIVE: repaint the lighthouse lamp room in signal red.";
    const COATS: &str = "DESIGN: use two coats of marine enamel, sanding between them.";
    const WHITE: &str = "CHANGE: actually, make it signal white instead of red.";
    let r = rig(|req| {
        let (first, last) = (first_user(req), last_user(req));
        if first.starts_with("[Task ") {
            if last.contains("⏰ wake") {
                return Scripted::text("Repainted in white, two coats.");
            }
            if answers_a_call(req) {
                return Scripted::text("Waiting for the paint to dry.");
            }
            return Scripted::tools(
                "Painting the first coat.",
                &[(
                    "w1",
                    "wake_at",
                    json!({"after": "1s", "note": "second coat"}),
                )],
            );
        }
        if answers_a_call(req) {
            return Scripted::text("Started.");
        }
        if last.starts_with("OBJECTIVE") {
            return Scripted::text(COATS);
        }
        if last.starts_with("START") {
            return create(json!({"brief": "Repaint the lamp room.", "arrangement": {
                "pieces": [
                    {"quote": "repaint the lighthouse lamp room in signal red", "role": "objective"},
                    {"quote": "two coats of marine enamel, sanding between them", "role": "design"},
                    {"quote": "make it signal white instead of red", "role": "objective"}],
                "trust": [1],
                "supersedes": [[0, 2]]}}));
        }
        Scripted::text("Noted.")
    });
    let sid = parent_session(&r.core);
    turn(&r.core, &sid, RED).await;
    turn(&r.core, &sid, WHITE).await;
    turn(&r.core, &sid, "START: do it as a task").await;
    let (status, content, _) = creates(&r.core, &sid).pop().unwrap();
    assert_eq!(status, ResultStatus::Ok, "{content}");
    let task = tasks(&r.core).pop().unwrap();
    until("the task complete", 20, || {
        r.core.kernel.execution(&task.id).unwrap().unwrap().state == ExecState::Complete
    })
    .await;

    let pnodes = nodes(&r.core, &sid);
    let red = said(&r.core, &sid, RED);
    let white = said(&r.core, &sid, WHITE);
    let coats = pnodes
        .iter()
        .find(|n| crate::arrangement::text_of(n).as_deref() == Some(COATS))
        .unwrap()
        .clone();
    let reqs = task_requests(&r.model);
    assert_eq!(reqs.len(), 3, "a loop, its follow-up, and the wake's turn");
    let opening = &reqs[0].messages[0];
    let b = blocks(opening);
    assert_eq!(b.len(), 2, "the brief, then the arrangement: {b:?}");
    assert!(b[0].ends_with("Repaint the lamp room."), "{}", b[0]);
    let arr = &b[1];
    let short = crate::narrative::short(&sid);
    // Each admitted piece whole and verbatim, in order, with its origin.
    let coats_at = arr.find(COATS).expect("the design, verbatim");
    let white_at = arr.find(WHITE).expect("the change, verbatim");
    assert!(coats_at < white_at);
    assert!(
        arr.contains(&format!(
            "Piece 2 of 3 (design, trusted testimony): from the model in session {short}, {}",
            crate::arrangement::utc(coats.created_at_ms)
        )),
        "{arr}"
    );
    assert!(
        arr.contains(&format!(
            "Piece 3 of 3 (objective): from the operator (test) in session {short}, {}",
            crate::arrangement::utc(white.created_at_ms)
        )),
        "{arr}"
    );
    // The superseded piece by reference only: its node, never its text.
    assert!(
        arr.contains(&format!(
            "Piece 1 of 3 (objective): superseded by piece 3. It was node {}",
            red.id
        )),
        "{arr}"
    );
    let whole = serde_json::to_string(&reqs[0].messages).unwrap();
    assert!(
        !whole.contains("signal red."),
        "the superseded text: {whole}"
    );
    // The prefix is the same on the task's later loop and its later turn.
    for later in &reqs[1..] {
        assert_eq!(bare(&later.messages[0]), bare(opening), "the prefix moved");
    }

    arranged_as_shown(&r.core, &sid, &task, [&red, &coats, &white]);
}

/// A request's message without the task graph's view and the breakpoint
/// before it, which ride last in a request's last message (39a).
fn bare(m: &Value) -> Value {
    let mut m = m.clone();
    if let Some(b) = m["content"].as_array_mut() {
        b.retain(|b| !crate::task_graph::view::is_view(b));
        for b in b.iter_mut() {
            b.as_object_mut().map(|o| o.remove("cache_control"));
        }
    }
    m
}

/// The child's arrangement node and its edges, one into each piece's node,
/// and the task's surfaces: the pieces, by reference.
fn arranged_as_shown(
    core: &Arc<Core>,
    sid: &str,
    task: &Execution,
    [red, coats, white]: [&Node; 3],
) {
    // The node and its edges, one into each piece's node.
    let tnodes = nodes(core, &task.session_id);
    assert!(crate::task::is_brief(&tnodes[0]));
    let arrangement = &tnodes[1];
    let Body::Arrangement {
        pieces,
        fidelity_ack,
    } = &arrangement.body
    else {
        panic!("the arrangement follows the brief: {arrangement:?}")
    };
    assert!(!fidelity_ack);
    assert_eq!(pieces[0].text, None, "the superseded piece keeps no text");
    for n in [red, coats, white] {
        let edges = core.store.scope_after(&format!("in:{}", n.id), 0).unwrap();
        let e: Vec<crate::graph::Edge> = edges.iter().map(|e| e.decode().unwrap()).collect();
        assert_eq!(e.len(), 1, "{e:?}");
        assert_eq!(
            (e[0].from.as_str(), e[0].via.as_str()),
            (arrangement.id.as_str(), crate::graph::VIA_ARRANGEMENT)
        );
    }
    // The task's surfaces: the pieces, by reference.
    let shown = core.tasks(Some(sid), None).unwrap();
    let a = shown[0].arrangement.as_ref().expect("its arrangement");
    assert_eq!(a.node_id, arrangement.id);
    let roles: Vec<(&str, bool, Option<u32>)> = a
        .pieces
        .iter()
        .map(|p| (p.role.as_str(), p.trusted, p.superseded_by))
        .collect();
    assert_eq!(
        roles,
        [
            ("objective", false, Some(2)),
            ("design", true, None),
            ("objective", false, None)
        ]
    );
    assert_eq!(a.pieces[1].origin, "agent");
    assert_eq!(a.pieces[2].author.as_deref(), Some("test"));
    assert_eq!(rows(core, "task.arranged").len(), 1);
}
