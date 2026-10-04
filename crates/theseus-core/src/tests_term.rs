//! Terminals through the whole core (theseus-n88g.4): the gate's decisions
//! for `term.open` and `term.send` (judged as their program's run), a
//! listed program's screen holding its session, and a terminal closed, with
//! its row and no process left, at its session's end, a cancel, a `/stop`,
//! and the daemon's stop.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_protocol::{SessionKind, TurnSubmitResult};
use theseus_tools::{Tool, ToolCtx};

use crate::bus::EventSink;
use crate::policy::{Posture, ToolPolicy};
use crate::provider::{FakeProvider, Provider, ProviderRequest, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::term::{self, tools, Terms};
use crate::turn::TurnRequest;
use crate::{Config, Core};

const PLACE: &str = "dm:77";

/// A stand-in model that answers each request from the request itself.
struct Model(Box<dyn Fn(&ProviderRequest) -> Scripted + Send + Sync>);

impl Provider for Model {
    fn name(&self) -> &str {
        "fake"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: crate::provider::DeltaSink<'a>,
    ) -> crate::provider::ProviderFuture<'a> {
        Box::pin(async move {
            let answer = (self.0)(req);
            FakeProvider::scripted(vec![answer])
                .stream_message(req, on_delta)
                .await
        })
    }
}

fn text_of(m: &Value) -> String {
    match &m["content"] {
        Value::String(s) => s.clone(),
        Value::Array(b) => b
            .iter()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// The first user text (a task's brief), the last, and how many tool
/// results came after the last.
fn asked(req: &ProviderRequest) -> (String, String, usize) {
    let users: Vec<(usize, String)> = req
        .messages
        .iter()
        .enumerate()
        .filter(|(_, m)| m["role"] == "user")
        .map(|(i, m)| (i, text_of(m)))
        .filter(|(_, t)| !t.is_empty())
        .collect();
    let first = users.first().map(|(_, t)| t.clone()).unwrap_or_default();
    let (at, last) = users.last().cloned().unwrap_or_default();
    let results = req.messages[at..]
        .iter()
        .filter_map(|m| m["content"].as_array())
        .flatten()
        .filter(|b| b["type"] == "tool_result")
        .count();
    (first, last, results)
}

fn call(id: &str, tool: &str, input: Value) -> Scripted {
    Scripted::tools("", &[(id, &theseus_tools::wire_name(tool), input)])
}

struct Rig {
    core: Arc<Core>,
    _dir: tempfile::TempDir,
}

fn rig(
    external: &[&str],
    driver: bool,
    script: impl Fn(&ProviderRequest) -> Scripted + Send + Sync + 'static,
) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = Posture::Notify;
    cfg.policy.external_programs = external.iter().map(|s| s.to_string()).collect();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let core = Core::build(crate::rpc::Parts::for_tests(
        cfg,
        Arc::new(Model(Box::new(script))),
        store,
    ))
    .unwrap();
    if driver {
        tokio::spawn(crate::harness::drive(core.clone()));
    }
    Rig { core, _dir: dir }
}

fn session(core: &Arc<Core>) -> String {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    rec.session_id
}

async fn turn(core: &Arc<Core>, sid: &str, input: &str) -> TurnSubmitResult {
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
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
        })
        .await
        .unwrap()
}

fn rows(core: &Core, kind: &str) -> Vec<(Option<String>, Value)> {
    core.store
        .ledger_tail::<crate::ledger::LedgerRow>(100_000)
        .unwrap()
        .into_iter()
        .filter(|(_, r)| r.kind == kind)
        .map(|(_, r)| (r.session_id.clone(), r.data))
        .collect()
}

async fn until(what: &str, mut f: impl FnMut() -> bool) {
    let t0 = Instant::now();
    while !f() {
        assert!(t0.elapsed() < Duration::from_secs(20), "no {what} in 20 s");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn lives(pid: u32) -> bool {
    theseus_kernel::tree::stat(pid).is_some_and(|s| !matches!(s.state, 'Z' | 'X'))
}

/// The terminal a session holds, with its program's pid.
fn only_terminal(core: &Core, sid: &str) -> (String, u32) {
    let ts = core.tools.terms.of_session(sid);
    assert_eq!(ts.len(), 1, "one terminal open");
    (ts[0].id.clone(), ts[0].pty.pid())
}

/// A policy whose approve list names `python3` and whose allow list `cat`.
fn policy_in(root: &Path) -> ToolPolicy {
    ToolPolicy {
        roots: vec![root.to_path_buf()],
        approve_paths: vec![],
        allow_argv: vec![vec!["cat".into()]],
        approve_argv: vec![vec!["python3".into()]],
        enforcement: Posture::Notify,
        tools: Default::default(),
        mcp: Default::default(),
        aws: Default::default(),
        confirmer: "operator".into(),
        floor_paths: vec![],
        floor_argv: crate::policy::floor_argv(),
    }
}

/// `term.open` is judged as `proc.run` is, by its argv: the approve list
/// waits, the allow list runs, the floor asks; `term.send` is judged by the
/// argv of its terminal's program, whatever keys it types; `term.read` and
/// `term.close` take their own postures.
#[test]
fn the_gate_judges_a_terminal_as_its_programs_run() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().canonicalize().unwrap();
    let ctx = ToolCtx::for_tests(&root);
    let policy = policy_in(&root);
    let terms = Arc::new(Terms::new(
        vec![("PATH".into(), std::env::var("PATH").unwrap())],
        vec![],
    ));
    let decide = |tool: &dyn Tool, input: Value| {
        let plan = tool.plan(&input, &ctx).unwrap();
        let d = policy.decide(tool, &plan);
        (d.posture, d.reason, d.floor)
    };
    let open = tools::Open;
    let (p, why, _) = decide(&open, json!({"argv": ["python3", "-q"]}));
    assert_eq!(p, Posture::Approve, "{why}");
    assert!(
        why.starts_with("run `python3 -q` on a terminal in "),
        "{why}"
    );
    assert!(
        why.ends_with(
            "term.open — approve (`python3 -q` matches the approve list entry `python3`)"
        ),
        "{why}"
    );
    let (p, why, _) = decide(&open, json!({"argv": ["cat"]}));
    assert_eq!(
        (p, why.as_str()),
        (
            Posture::Open,
            "term.open — open (`cat` matches the allow list entry `cat`)"
        )
    );
    let (p, why, floor) = decide(&open, json!({"argv": ["op", "read", "op://v/i/f"]}));
    assert!(p == Posture::Approve && floor, "{why}");
    let (p, why, _) = decide(&open, json!({"argv": ["sh"]}));
    assert_eq!(
        (p, why.as_str()),
        (Posture::Notify, "term.open — notify (enforcement = notify)")
    );
    // Each terminal's keys are its program's run.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let t_py = terms
        .open(
            "s1",
            &["python3".into(), "-q".into()],
            root.clone(),
            (24, 80),
            None,
        )
        .unwrap();
    let t_cat = terms
        .open("s1", &["cat".into()], root, (24, 80), None)
        .unwrap();
    let send = tools::Send(terms.clone());
    let (p, why, _) = decide(&send, json!({"terminal": t_py.id, "text": "print(1)\n"}));
    assert_eq!(p, Posture::Approve, "{why}");
    assert!(
        why.starts_with(&format!(
            "type into terminal {}, which runs `python3 -q` in ",
            t_py.id
        )),
        "{why}"
    );
    assert!(
        why.ends_with(
            "term.send — approve (`python3 -q` matches the approve list entry `python3`)"
        ),
        "{why}"
    );
    let (p, why, _) = decide(&send, json!({"terminal": t_cat.id, "keys": ["Ctrl-C"]}));
    assert_eq!(
        (p, why.as_str()),
        (
            Posture::Open,
            "term.send — open (`cat` matches the allow list entry `cat`)"
        )
    );
    let e = send
        .plan(&json!({"terminal": "t999", "text": "x"}), &ctx)
        .unwrap_err();
    assert!(e.contains("there is no terminal t999"), "{e}");
    // A read and a close: their own postures, and their class is a read.
    let (p, _, _) = decide(&tools::Read, json!({"terminal": t_py.id}));
    assert_eq!(p, Posture::Notify);
    assert_eq!(tools::Read.class(), theseus_tools::ToolClass::Read);
    assert_eq!(tools::Close.class(), theseus_tools::ToolClass::Read);
    assert_eq!(open.class(), theseus_tools::ToolClass::Run);
    assert_eq!(send.class(), theseus_tools::ToolClass::Run);
    rt.block_on(async { assert_eq!(terms.close_session("s1", term::BY_SESSION_END).len(), 2) });
}

/// A terminal whose program `[policy] external_programs` lists holds its
/// session from its first screen (`via: program`): the next `term.send`
/// waits for the operator, and a `term.read` keeps its posture. Its open and
/// its health line are recorded.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_listed_programs_screen_holds_its_session() {
    let r = rig(&["cat"], false, |req| {
        let (_, last, n) = asked(req);
        match (last.as_str(), n) {
            ("open cat", 0) => call("o1", term::OPEN, json!({"argv": ["cat"]})),
            ("open cat", 1) => call("s1", term::SEND, json!({"terminal": "t1", "text": "hi\n"})),
            ("read it", 0) => call("r1", term::READ, json!({"terminal": "t1"})),
            _ => Scripted::text("Done."),
        }
    });
    let core = &r.core;
    let sid = session(core);
    let res = turn(core, &sid, "open cat").await;
    let corr = res
        .awaiting_confirm
        .clone()
        .expect("the send waits after the screen");
    let pending = core.pending_confirms(&sid).unwrap();
    assert_eq!(pending[0].tool, term::SEND);
    let held = core
        .store
        .get_session::<SessionRecord>(&sid)
        .unwrap()
        .unwrap()
        .external
        .expect("held");
    assert_eq!(
        (held.tool.as_str(), held.via.as_deref()),
        (term::OPEN, Some(crate::external::VIA_PROGRAM))
    );
    let opened = rows(core, "term.opened");
    assert_eq!(opened.len(), 1);
    assert_eq!(opened[0].0.as_deref(), Some(sid.as_str()));
    assert_eq!(
        (
            opened[0].1["terminal"].as_str(),
            opened[0].1["argv"][0].as_str()
        ),
        (Some("t1"), Some("cat"))
    );
    let health = core.health();
    assert_eq!(health.terminals.len(), 1);
    assert_eq!(
        (
            health.terminals[0].id.as_str(),
            health.terminals[0].external.as_deref()
        ),
        ("t1", Some("cat"))
    );
    core.confirm_action(&corr, false, Some("not now"), "test")
        .unwrap();
    core.continue_execution(res.execution_id.as_deref().unwrap())
        .await
        .unwrap();
    let res = turn(core, &sid, "read it").await;
    assert!(res.awaiting_confirm.is_none(), "a read keeps its posture");
    core.finish_stop().await;
}

/// A terminal of a program no list names holds nothing: its send runs at
/// its posture.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_terminal_of_an_unlisted_program_holds_nothing() {
    let r = rig(&["gh"], false, |req| {
        let (_, last, n) = asked(req);
        match (last.as_str(), n) {
            ("open cat", 0) => call("o1", term::OPEN, json!({"argv": ["cat"]})),
            ("open cat", 1) => call("s1", term::SEND, json!({"terminal": "t1", "text": "hi\n"})),
            _ => Scripted::text("Done."),
        }
    });
    let sid = session(&r.core);
    let res = turn(&r.core, &sid, "open cat").await;
    assert!(res.awaiting_confirm.is_none());
    assert!(r
        .core
        .store
        .get_session::<SessionRecord>(&sid)
        .unwrap()
        .unwrap()
        .external
        .is_none());
    r.core.finish_stop().await;
}

/// A cancel of its execution closes a session's terminal: its row says so,
/// and its program is gone.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cancel_closes_the_sessions_terminals() {
    let r = rig(&[], false, opener);
    let sid = session(&r.core);
    let res = turn(&r.core, &sid, "open one").await;
    let (id, pid) = only_terminal(&r.core, &sid);
    assert!(lives(pid));
    r.core
        .cancel_execution(res.execution_id.as_deref().unwrap(), "test")
        .await
        .unwrap();
    assert_closed(&r.core, &sid, &id, pid, term::BY_CANCEL);
}

/// A `/stop` halts its terminals' programs too.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stop_closes_the_sessions_terminals() {
    let r = rig(&[], false, opener);
    let sid = session(&r.core);
    let res = turn(&r.core, &sid, "open one").await;
    let (id, pid) = only_terminal(&r.core, &sid);
    r.core
        .stop_execution(res.execution_id.as_deref().unwrap(), "test")
        .await
        .unwrap();
    assert_closed(&r.core, &sid, &id, pid, term::BY_STOP);
}

/// The daemon's stop closes every terminal, each recorded in its session.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_daemons_stop_closes_every_terminal() {
    let r = rig(&[], false, opener);
    let (a, b) = (session(&r.core), session(&r.core));
    turn(&r.core, &a, "open one").await;
    turn(&r.core, &b, "open one").await;
    let (ida, pida) = only_terminal(&r.core, &a);
    let (idb, pidb) = only_terminal(&r.core, &b);
    r.core.finish_stop().await;
    assert_closed(&r.core, &a, &ida, pida, term::BY_DAEMON);
    assert_closed(&r.core, &b, &idb, pidb, term::BY_DAEMON);
}

/// A task's session ends when it reports: the terminal it opened closes
/// with it, though nothing asked to close it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tasks_terminal_closes_at_its_sessions_end() {
    // The ask the task's arrangement quotes (M5 27: a task needs one).
    const START: &str = "START a task that opens a terminal, please";
    let r = rig(&[], true, |req| {
        let (first, last, n) = asked(req);
        if first.contains("CHILD") {
            return match n {
                0 => call(
                    "o1",
                    term::OPEN,
                    json!({"argv": ["sleep", "4747"], "quiet_ms": 0}),
                ),
                _ => Scripted::text("Opened it, and done."),
            };
        }
        match (last.as_str(), n) {
            (START, 0) => call(
                "t1",
                crate::task::CREATE,
                json!({
                    "brief": "CHILD open a terminal",
                    "arrangement": {"pieces": [{"quote": START, "role": "objective"}]}
                }),
            ),
            _ => Scripted::text("Started."),
        }
    });
    let sid = session(&r.core);
    r.core.outbox.bind_place(PLACE, &sid).unwrap();
    // A private place: a shared one is offered no terminal (the place rule).
    r.core
        .runner
        .place_rule
        .bind_one(crate::places::BoundPlace {
            target: format!("discord:{PLACE}"),
            name: "a private channel".into(),
            private: true,
        });
    turn(&r.core, &sid, START).await;
    until("the task's terminal closed", || {
        !rows(&r.core, "term.closed").is_empty()
    })
    .await;
    let closed = rows(&r.core, "term.closed");
    let task = closed[0].0.clone().unwrap();
    assert_ne!(task, sid, "the task's own session");
    assert_eq!(closed[0].1["by"], term::BY_SESSION_END);
    assert_eq!(closed[0].1["argv"], json!(["sleep", "4747"]));
    assert!(r.core.tools.terms.all().is_empty());
    assert!(
        !lives_marked("4747"),
        "the task's program outlived its session"
    );
}

fn opener(req: &ProviderRequest) -> Scripted {
    let (_, last, n) = asked(req);
    match (last.as_str(), n) {
        ("open one", 0) => call("o1", term::OPEN, json!({"argv": ["sh"], "quiet_ms": 0})),
        _ => Scripted::text("Opened."),
    }
}

fn assert_closed(core: &Core, sid: &str, id: &str, pid: u32, by: &str) {
    assert!(core.tools.terms.get(id).is_none(), "{id} still open");
    assert!(!lives(pid), "{id}'s program outlived its close");
    let closed: Vec<_> = rows(core, "term.closed")
        .into_iter()
        .filter(|(s, r)| s.as_deref() == Some(sid) && r["terminal"] == id)
        .collect();
    assert_eq!(closed.len(), 1, "one term.closed row for {id}");
    assert_eq!(closed[0].1["by"], by);
}

fn lives_marked(marker: &str) -> bool {
    std::fs::read_dir("/proc").unwrap().flatten().any(|e| {
        let Some(pid) = e.file_name().to_str().and_then(|p| p.parse::<u32>().ok()) else {
            return false;
        };
        lives(pid)
            && std::fs::read(format!("/proc/{pid}/cmdline"))
                .is_ok_and(|c| String::from_utf8_lossy(&c).contains(marker))
    })
}
