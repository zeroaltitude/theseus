//! Check tasks through the whole core (M5 step 28a, theseus-vug.3): a check's
//! compilation holds nothing of the checked task's session but its report,
//! rendered as a claim, after the checked task's objective and acceptance;
//! a copied span of twelve words raises the overlap flag and eleven do not;
//! the basis is recorded on the check's session and shown on its surfaces;
//! a check of a task with no report is refused; and the exclusion holds on
//! the check's own pieces. The pure rules' own tests are `check::tests`.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_kernel::{ExecState, Execution};
use theseus_protocol::{SessionKind, TaskCheck};

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

const OBJECTIVE: &str = "OBJECTIVE: count the herring gulls on the north pier at low tide.";
const ACCEPT: &str = "ACCEPT: the count names how many gulls, and where the number came from.";
/// What the maker read while it worked: its tool result's text.
const WORKING: &str = "Harbour log, Tuesday: the north pier holds forty two herring gulls at low \
    tide, counted from the lamp gallery at dawn by the keeper.";
const MAKER_BRIEF: &str = "Count the gulls by reading the harbour log in tide.txt.";
const REPORT: &str = "REPORT: there are 42 herring gulls on the north pier.";
const CHECK_BRIEF: &str = "CHECKBRIEF: establish independently how many gulls the pier holds.";
/// Twelve words of `WORKING`, and eleven.
const COPIED_12: &str = "CHECKBRIEF: confirm that the north pier holds forty two herring gulls \
    at low tide counted, by your own count.";
const COPIED_11: &str = "CHECKBRIEF: confirm that the north pier holds forty two herring gulls \
    at low tide, by your own count.";

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

/// A user message's text blocks, the task graph's view left out.
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

/// The parent: the operator's messages, then `START` opens the maker and
/// `CHECK <input>` a check with that input. The maker reads the harbour log
/// and reports; a check answers. `PARK` starts a maker that waits on a wake.
fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let work = root.canonicalize().unwrap();
    std::fs::write(work.join("tide.txt"), WORKING).unwrap();
    let log = work.join("tide.txt").to_string_lossy().into_owned();
    let script = move |req: &ProviderRequest| {
        let (first, last) = (first_user(req), last_user(req));
        if first.starts_with("[Task ") && first.contains("CHECKBRIEF") {
            return Scripted::text("CHECKED: the claim holds; I counted 42 myself.");
        }
        if first.starts_with("[Task ") && first.contains("Wait on the tide") {
            if answers_a_call(req) {
                return Scripted::text("Waiting for the tide.");
            }
            return Scripted::tools(
                "Waiting.",
                &[("w1", "wake_at", json!({"after": "1h", "note": "the tide"}))],
            );
        }
        if first.starts_with("[Task ") {
            if answers_a_call(req) {
                return Scripted::text(REPORT);
            }
            return Scripted::tools(
                "Reading the log.",
                &[("r1", "fs_read", json!({"path": log}))],
            );
        }
        if answers_a_call(req) {
            return Scripted::text("Started.");
        }
        let arranged = json!({"pieces": [
            {"quote": "count the herring gulls on the north pier at low tide", "role": "objective"},
            {"quote": "the count names how many gulls, and where the number came from", "role": "acceptance"}]});
        if last.ends_with("START") {
            return create(json!({"brief": MAKER_BRIEF, "arrangement": arranged}));
        }
        if last.ends_with("PARK") {
            return create(
                json!({"brief": "Wait on the tide, then count.", "arrangement": arranged}),
            );
        }
        if let Some(input) = last.rsplit_once("CHECK ").map(|(_, i)| i) {
            return create(serde_json::from_str(input).unwrap());
        }
        Scripted::text("Noted.")
    };
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(work.to_string_lossy().into_owned());
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
        private: true,
        ..Default::default()
    });
    tokio::spawn(crate::harness::drive(core.clone()));
    Rig {
        core,
        model,
        _dir: dir,
    }
}

fn create(input: Value) -> Scripted {
    Scripted::tools("Starting it.", &[("t_task", "task_create", input)])
}

fn parent_session(core: &Arc<Core>) -> String {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    core.outbox.bind_place(PLACE, &rec.session_id).unwrap();
    rec.session_id
}

async fn turn(core: &Arc<Core>, sid: &str, input: &str) {
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
        .unwrap();
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

/// The last `task.create` result in a session: its status, content, meta.
fn last_create(core: &Core, sid: &str) -> (ResultStatus, String, Value) {
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
        .next_back()
        .expect("a task.create result")
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

async fn finished(core: &Core, e: &Execution) {
    until("the task complete", 30, || {
        core.kernel.execution(&e.id).unwrap().unwrap().state == ExecState::Complete
    })
    .await;
}

/// The maker, started and finished: its execution.
async fn maker(r: &Rig, sid: &str) -> Execution {
    turn(&r.core, sid, OBJECTIVE).await;
    turn(&r.core, sid, ACCEPT).await;
    turn(&r.core, sid, "START").await;
    let (status, content, _) = last_create(&r.core, sid);
    assert_eq!(status, ResultStatus::Ok, "{content}");
    let m = tasks(&r.core).pop().unwrap();
    finished(&r.core, &m).await;
    m
}

/// Start a check with `input`, and say what the call answered.
async fn check(r: &Rig, sid: &str, input: Value) -> (ResultStatus, String, Value) {
    turn(&r.core, sid, &format!("CHECK {input}")).await;
    last_create(&r.core, sid)
}

/// The check task the last call opened.
fn the_check(r: &Rig, meta: &Value) -> Execution {
    let id = meta["task_id"].as_str().unwrap();
    tasks(&r.core)
        .into_iter()
        .find(|e| e.session_id == id)
        .unwrap()
}

fn basis(core: &Core, e: &Execution) -> TaskCheck {
    let rec: SessionRecord = core.store.get_session(&e.session_id).unwrap().unwrap();
    rec.task.unwrap().check.expect("the check's basis")
}

/// The requests a session's turns made, by the brief that opens them.
fn requests_with(model: &Model, brief: &str) -> Vec<ProviderRequest> {
    model
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter(|q| first_user(q).starts_with("[Task ") && first_user(q).contains(brief))
        .cloned()
        .collect()
}

/// The check's first compilation renders its brief, then the checked task's
/// objective and acceptance pieces and its report as a claim, and holds
/// nothing else of the checked task's session: not its brief, its calls, or
/// what its tools read. Its arrangement node derives from the report and the
/// pieces, and from no other node of that session.
#[tokio::test]
async fn a_checks_compilation_holds_nothing_of_the_checked_session_but_its_claim() {
    let r = rig();
    let sid = parent_session(&r.core);
    let m = maker(&r, &sid).await;
    let short = crate::task::short(&m.session_id);
    let (status, content, meta) =
        check(&r, &sid, json!({"brief": CHECK_BRIEF, "check_of": short})).await;
    assert_eq!(status, ResultStatus::Ok, "{content}");
    let c = the_check(&r, &meta);
    finished(&r.core, &c).await;

    let made = nodes(&r.core, &m.session_id);
    let report = made
        .iter()
        .rev()
        .find(|n| crate::arrangement::text_of(n).as_deref() == Some(REPORT))
        .expect("the maker's report")
        .clone();
    let reqs = requests_with(&r.model, "CHECKBRIEF");
    assert_eq!(reqs.len(), 1, "one loop");
    let b = blocks(&reqs[0].messages[0]);
    assert_eq!(b.len(), 2, "the brief, then the arrangement: {b:?}");
    assert!(b[0].ends_with(CHECK_BRIEF), "{}", b[0]);
    let arr = &b[1];
    let objective_at = arr.find(OBJECTIVE).expect("the checked task's objective");
    let accept_at = arr.find(ACCEPT).expect("its acceptance");
    let claim_at = arr
        .find(&format!(
            "--- Claim: claimed by task {short}, as of {} ---\n{REPORT}",
            crate::arrangement::utc(report.created_at_ms)
        ))
        .expect("the report, as a claim");
    assert!(objective_at < accept_at && accept_at < claim_at, "{arr}");
    // Nothing else of the maker's session reaches it. (The task graph's view
    // names the maker's record, its title among them: the parent's words,
    // not the maker's session.)
    let whole = reqs[0]
        .messages
        .iter()
        .flat_map(blocks)
        .collect::<Vec<_>>()
        .join("\n");
    for kept_out in [
        "forty two herring gulls",
        MAKER_BRIEF,
        "Reading the log.",
        "tide.txt",
    ] {
        assert!(
            !whole.contains(kept_out),
            "{kept_out:?} reached the check: {whole}"
        );
    }
    // The node, its claim, and its edges: one to the report, none to any
    // other node of the maker's session.
    let mine = nodes(&r.core, &c.session_id);
    let Body::Arrangement { claim, pieces, .. } = &mine[1].body else {
        panic!("the arrangement follows the brief: {:?}", mine[1])
    };
    let claim = claim.as_ref().expect("its claim");
    assert_eq!(claim.node, report.id);
    assert_eq!(claim.task, m.session_id);
    assert_eq!(pieces.len(), 2);
    for n in &made {
        let into: Vec<crate::graph::Edge> = r
            .core
            .store
            .scope_after(&crate::graph::Edge::scope_into(&n.id), 0)
            .unwrap()
            .into_iter()
            .map(|r| r.decode().unwrap())
            .collect();
        let from_check = into.iter().filter(|e| mine.iter().any(|x| x.id == e.from));
        if n.id == report.id {
            let e: Vec<_> = from_check.collect();
            assert_eq!(e.len(), 1, "{e:?}");
            assert_eq!(e[0].via, crate::graph::VIA_CLAIM);
        } else {
            assert_eq!(from_check.count(), 0, "an edge into {}", n.id);
        }
    }
}

/// The basis is on the check's session record, in its `task.check_opened`
/// row and its call's result, on `task.list`, and beside its report: its
/// post and the node its parent reads.
#[tokio::test]
async fn the_basis_is_recorded_and_shown() {
    let r = rig();
    let sid = parent_session(&r.core);
    let m = maker(&r, &sid).await;
    let short = crate::task::short(&m.session_id);
    let (status, content, meta) = check(
        &r,
        &sid,
        json!({"brief": CHECK_BRIEF, "check_of": m.session_id}),
    )
    .await;
    assert_eq!(status, ResultStatus::Ok, "{content}");
    let c = the_check(&r, &meta);
    let b = basis(&r.core, &c);
    assert_eq!(b.checked_task, m.session_id);
    assert_eq!(b.checked_short, short);
    assert_eq!(b.excluded_sessions, std::slice::from_ref(&m.session_id));
    let admitted: Vec<(&str, &str)> = b
        .admitted
        .iter()
        .map(|p| (p.role.as_str(), p.from.as_str()))
        .collect();
    assert_eq!(
        admitted,
        [
            ("objective", "checked"),
            ("acceptance", "checked"),
            ("claim", "claim")
        ]
    );
    let (live, _) = r.core.live_profile();
    let target = r
        .core
        .runner
        .resolve_target(&live, None, None, None)
        .unwrap();
    assert_eq!(
        (b.profile.as_str(), b.model.as_str()),
        (target.profile.as_str(), target.model.as_str())
    );
    assert!(b.overlaps.is_empty(), "{:?}", b.overlaps);
    let line = format!(
        "🔍 check of task {short} · independent (excluded ses_…{short}, {})",
        target.model
    );
    assert_eq!(b.line(), line);
    assert!(content.contains(&line), "{content}");
    assert_eq!(meta["check"]["checked_task"], m.session_id.as_str());
    let opened = rows(&r.core, "task.check_opened");
    assert_eq!(opened.len(), 1);
    assert_eq!(
        opened[0].data["basis"]["report_node"],
        b.report_node.as_str()
    );
    let listed = r.core.tasks(Some(&sid), None).unwrap();
    let shown = listed.iter().find(|t| t.task_id == c.session_id).unwrap();
    assert_eq!(shown.check.as_ref(), Some(&b));
    assert!(listed
        .iter()
        .find(|t| t.task_id == m.session_id)
        .unwrap()
        .check
        .is_none());
    finished(&r.core, &c).await;
    let report = crate::task::load_report(&r.core.store, &r.core.kernel, &c.id)
        .unwrap()
        .unwrap();
    assert_eq!(report.check.as_deref(), Some(line.as_str()));
    assert_eq!(report.post_body()["check"], line.as_str());
    assert!(
        report.node_text().contains(&format!("]\n{line}")),
        "{}",
        report.node_text()
    );
    // A task that is no check carries none.
    let made = crate::task::load_report(&r.core.store, &r.core.kernel, &m.id)
        .unwrap()
        .unwrap();
    assert!(made.post_body().get("check").is_none());
}

/// A brief that copies twelve words of the maker's working is flagged, with
/// its span and the node that holds it; eleven are not. Either way the check
/// runs.
#[tokio::test]
async fn a_copied_span_of_twelve_words_raises_the_flag_and_eleven_does_not() {
    let r = rig();
    let sid = parent_session(&r.core);
    let m = maker(&r, &sid).await;
    let short = crate::task::short(&m.session_id);
    let read = nodes(&r.core, &m.session_id)
        .into_iter()
        .find(|n| matches!(&n.body, Body::ToolResult { content, .. } if content.contains("forty two")))
        .expect("the log the maker read");
    let (status, content, meta) =
        check(&r, &sid, json!({"brief": COPIED_12, "check_of": short})).await;
    assert_eq!(status, ResultStatus::Ok, "{content}");
    let b = basis(&r.core, &the_check(&r, &meta));
    assert_eq!(b.overlaps.len(), 1, "{:?}", b.overlaps);
    let o = &b.overlaps[0];
    assert_eq!((o.source.as_str(), o.words), ("brief", 12));
    assert_eq!(o.node_id, read.id);
    assert_eq!(
        o.span,
        "the north pier holds forty two herring gulls at low tide counted"
    );
    assert!(b.line().ends_with(" · overlap: 1 span"), "{}", b.line());
    assert!(
        content.contains("Flagged: the brief shares 12 words"),
        "{content}"
    );
    let (status, content, meta) =
        check(&r, &sid, json!({"brief": COPIED_11, "check_of": short})).await;
    assert_eq!(status, ResultStatus::Ok, "{content}");
    let b = basis(&r.core, &the_check(&r, &meta));
    assert!(b.overlaps.is_empty(), "{:?}", b.overlaps);
}

/// A check of a task with no report is refused with the reason, and opens
/// nothing: one still waiting, one cancelled, one this conversation did not
/// start; and `profile` is a check's alone.
#[tokio::test]
async fn a_check_of_a_task_with_no_report_is_refused() {
    let r = rig();
    let sid = parent_session(&r.core);
    turn(&r.core, &sid, OBJECTIVE).await;
    turn(&r.core, &sid, ACCEPT).await;
    turn(&r.core, &sid, "PARK").await;
    let parked = tasks(&r.core).pop().unwrap();
    until("the task parked on its wake", 30, || {
        r.core
            .kernel
            .execution(&parked.id)
            .unwrap()
            .unwrap()
            .wakes
            .len()
            == 1
    })
    .await;
    let short = crate::task::short(&parked.session_id);
    let (status, content, _) =
        check(&r, &sid, json!({"brief": CHECK_BRIEF, "check_of": short})).await;
    assert_eq!(status, ResultStatus::Error);
    // Parked on its wake, or still finishing the turn that set it.
    let state = r.core.kernel.execution(&parked.id).unwrap().unwrap().state;
    assert!(!state.is_terminal());
    assert!(content.contains(&format!("task {short} is ")), "{content}");
    assert!(content.contains(" and has not reported."), "{content}");
    r.core.task_cancel_by(&short, "test").await.unwrap();
    let (status, content, _) =
        check(&r, &sid, json!({"brief": CHECK_BRIEF, "check_of": short})).await;
    assert_eq!(status, ResultStatus::Error);
    assert!(
        content.contains(&format!("task {short} cancelled")),
        "{content}"
    );
    assert!(content.contains("has no report to check"), "{content}");
    let (status, content, _) = check(
        &r,
        &sid,
        json!({"brief": CHECK_BRIEF, "check_of": "zzzzzz"}),
    )
    .await;
    assert_eq!(status, ResultStatus::Error);
    assert!(
        content.contains("among the tasks this conversation started"),
        "{content}"
    );
    let refused: Vec<String> = rows(&r.core, "task.check_refused")
        .iter()
        .map(|r| r.data["class"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(refused, ["no_report", "no_report", "unknown"]);
    assert_eq!(tasks(&r.core).len(), 1, "no check opened");
    // A profile without `check_of` is invalid input.
    turn(
        &r.core,
        &sid,
        &format!(
            "CHECK {}",
            json!({"brief": "Count again.", "profile": "default",
                "arrangement": {"pieces": [{"quote": "count the herring gulls on the north pier", "role": "objective"}]}})
        ),
    )
    .await;
    let (status, content, _) = last_create(&r.core, &sid);
    assert_eq!(status, ResultStatus::Error);
    assert!(content.contains("`profile` is for a check"), "{content}");
}

/// The check's own pieces: one that quotes the parent's copy of the report
/// reads as the claim; one whose node derives from any other node of the
/// maker's session is refused, naming the exclusion; and a named profile is
/// what the check runs on.
#[tokio::test]
async fn the_exclusion_holds_on_the_checks_own_pieces() {
    let r = rig();
    let sid = parent_session(&r.core);
    let m = maker(&r, &sid).await;
    let short = crate::task::short(&m.session_id);
    // The parent's next turn reads the report into its session.
    turn(&r.core, &sid, "What did it find?").await;
    let relayed = nodes(&r.core, &sid)
        .into_iter()
        .find(|n| {
            crate::arrangement::text_of(n).is_some_and(|t| t.starts_with("[Report from task"))
        })
        .expect("the report, relayed");
    let quoted = json!({"brief": CHECK_BRIEF, "check_of": short, "profile": "default",
        "arrangement": {"pieces": [{"node": relayed.id, "role": "context"}]}});
    let (status, content, meta) = check(&r, &sid, quoted).await;
    assert_eq!(status, ResultStatus::Ok, "{content}");
    let c = the_check(&r, &meta);
    let b = basis(&r.core, &c);
    assert_eq!(b.admitted.len(), 4);
    assert_eq!(
        (b.admitted[2].role.as_str(), b.admitted[2].from.as_str()),
        ("claim", "own")
    );
    assert_eq!(b.admitted[2].node_id, relayed.id);
    assert_eq!(b.profile, "default");
    let rec: SessionRecord = r.core.store.get_session(&c.session_id).unwrap().unwrap();
    assert_eq!(rec.last_target.unwrap().profile, "default");
    let Body::Arrangement { pieces, .. } = &nodes(&r.core, &c.session_id)[1].body else {
        panic!("no arrangement")
    };
    assert_eq!(
        pieces.len(),
        2,
        "the copy of the report reads as the claim, not a piece"
    );

    // A note in the parent that copies the maker's working, as a recall or
    // a publish would: a node with a `derived_from` edge into it.
    let read = nodes(&r.core, &m.session_id)
        .into_iter()
        .find(|n| matches!(n.body, Body::ToolResult { .. }))
        .unwrap();
    let note = Node::relayed(
        &sid,
        None,
        crate::node::Origin::Harness,
        "harness",
        "NOTE: what the maker's harbour log said, copied here for the record.",
    );
    let edge = crate::graph::Edge::new(
        crate::graph::EdgeKind::DerivedFrom,
        &note.id,
        &read.id,
        crate::graph::VIA_PUBLISH,
    );
    r.core
        .store
        .append(&[note.record().unwrap(), edge.record().unwrap()])
        .unwrap();
    let excluded = json!({"brief": CHECK_BRIEF, "check_of": short,
        "arrangement": {"pieces": [{"node": note.id, "role": "context"}]}});
    let (status, content, _) = check(&r, &sid, excluded).await;
    assert_eq!(status, ResultStatus::Error, "{content}");
    assert!(content.contains("(the exclusion)"), "{content}");
    assert!(
        content.contains(&format!(
            "derives from node {} of task {short}'s session",
            read.id
        )),
        "{content}"
    );
    // A profile the config does not name.
    let (status, content, _) = check(
        &r,
        &sid,
        json!({"brief": CHECK_BRIEF, "check_of": short, "profile": "nowhere"}),
    )
    .await;
    assert_eq!(status, ResultStatus::Error);
    assert!(
        content.contains("no profile is named `nowhere`"),
        "{content}"
    );
    let refused: Vec<String> = rows(&r.core, "task.check_refused")
        .iter()
        .map(|r| r.data["class"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(refused, ["excluded", "profile"]);
}

/// A check names its task by the task graph's record id too (39a's `tsk_…`,
/// which the model reads in the view and in `task.create`'s result), as well
/// as by its session's id or the end of it.
#[tokio::test]
async fn a_check_names_its_task_by_its_record_id() {
    let r = rig();
    let sid = parent_session(&r.core);
    let m = maker(&r, &sid).await;
    let record = crate::task_graph::of_session(&m.session_id);
    let (status, content, meta) =
        check(&r, &sid, json!({"brief": CHECK_BRIEF, "check_of": record})).await;
    assert_eq!(status, ResultStatus::Ok, "{content}");
    let c = the_check(&r, &meta);
    assert_eq!(basis(&r.core, &c).checked_task, m.session_id);
    finished(&r.core, &c).await;
}

/// 30c's summary is the model's account of its range, the maker's own
/// working in other words: the overlap flag reads its text, so a brief that
/// copies twelve words of a summary of the maker's turns is flagged.
#[test]
fn the_overlap_flag_reads_a_summarys_text() {
    let summary = Node::summary(
        "ses_maker",
        "trn_1",
        Body::Summary {
            first: 1,
            last: 9,
            nodes: 4,
            text: WORKING.into(),
            profile: "session".into(),
            model: "fake".into(),
            cost_usd: None,
            header: "[Summary of 4 earlier messages]".into(),
        },
    );
    let text = crate::check::working_text(&summary).expect("a summary's words");
    let found = crate::check::overlaps(
        &[("brief".to_string(), COPIED_12.to_string())],
        &[(summary.id.as_str(), text)],
    );
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(
        (found[0].node_id.as_str(), found[0].words),
        (summary.id.as_str(), 12)
    );
}
