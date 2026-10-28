//! Terminals through the whole core (theseus-n88g.4): the gate's decisions
//! for `term.open` and `term.send` (judged as their program's run), a
//! terminal's send running at its posture, and a terminal closed, with its
//! row and no process left, at its session's end, a cancel, a `/stop`, and
//! the daemon's stop.

use std::path::Path;
use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::SessionKind;
use theseus_tools::{Tool, ToolCtx};

use crate::policy::{Posture, ToolPolicy};
use crate::provider::{FakeProvider, Provider, ProviderRequest, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::term::{self, tools, Terms};
use crate::test_support::{turn, until};
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

fn rig(driver: bool, script: impl Fn(&ProviderRequest) -> Scripted + Send + Sync + 'static) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = Posture::Notify;
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

fn rows(core: &Core, kind: &str) -> Vec<(Option<String>, Value)> {
    core.store
        .ledger_tail::<crate::ledger::LedgerRow>(100_000)
        .unwrap()
        .into_iter()
        .filter(|(_, r)| r.kind == kind)
        .map(|(_, r)| (r.session_id.clone(), r.data))
        .collect()
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
        private_addresses: Default::default(),
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
    let terms = Arc::new(Terms::new(vec![(
        "PATH".into(),
        std::env::var("PATH").unwrap(),
    )]));
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
        .unwrap()
        .0;
    let t_cat = terms
        .open("s1", &["cat".into()], root, (24, 80), None)
        .unwrap()
        .0;
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

/// A terminal of any program runs at its posture: the `term.send` after its
/// open is not held, and a `term.read` keeps its posture. Its open and its
/// health line are recorded.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_terminals_send_runs_at_its_posture_and_its_open_is_recorded() {
    let r = rig(false, |req| {
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
    assert!(res.awaiting_confirm.is_none(), "the send is not held");
    assert!(core.pending_confirms(&sid).unwrap().is_empty());
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
    assert_eq!(health.terminals[0].id.as_str(), "t1");
    let res = turn(core, &sid, "read it").await;
    assert!(res.awaiting_confirm.is_none(), "a read keeps its posture");
    core.finish_stop().await;
}

/// A cancel of its execution closes a session's terminal: its row says so,
/// and its program is gone.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cancel_closes_the_sessions_terminals() {
    let r = rig(false, opener);
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
    let r = rig(false, opener);
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
    let r = rig(false, opener);
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
    let r = rig(true, |req| {
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
            ..Default::default()
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

/// theseus-ggqf: a task's terminal leaves its background job running at the
/// task's end, as `proc.run` would: a `term.left` row names it, health lists
/// it, and its terminal's foreground program is gone. The parent's `/stop`
/// then ends it, with a `term.left` row that says so.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tasks_background_job_outlives_its_end_until_a_stop() {
    background_job_until(term::BY_STOP, "4791", "4792").await;
}

/// A cancel of the parent's execution ends what the task's terminal left, as
/// a `/stop` does (the join's review: no test held the cancel's end).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tasks_background_job_outlives_its_end_until_a_cancel() {
    background_job_until(term::BY_CANCEL, "4789", "4790").await;
}

/// A task's terminal leaves `set -m; sleep BG & exec sleep FG`'s job at the
/// task's end, and the parent's `/stop` or cancel (`by`) ends it.
async fn background_job_until(by: &'static str, bg: &str, fg: &str) {
    const START: &str = "START a task that leaves a job running, please";
    let run = format!("{:07}", std::process::id());
    let (bg, fg) = (format!("{bg}.{run}"), format!("{fg}.{run}"));
    let script = format!("set -m; sleep {bg} & exec sleep {fg}");
    let r = rig(true, move |req| {
        let (first, last, n) = asked(req);
        if first.contains("CHILD") {
            return match n {
                0 => call(
                    "o1",
                    term::OPEN,
                    json!({"argv": ["bash", "-c", script], "quiet_ms": 200}),
                ),
                _ => Scripted::text("Started it, and done."),
            };
        }
        match (last.as_str(), n) {
            (START, 0) => call(
                "t1",
                crate::task::CREATE,
                json!({
                    "brief": "CHILD start a job",
                    "arrangement": {"pieces": [{"quote": START, "role": "objective"}]}
                }),
            ),
            _ => Scripted::text("Started."),
        }
    });
    let sid = session(&r.core);
    r.core.outbox.bind_place(PLACE, &sid).unwrap();
    r.core
        .runner
        .place_rule
        .bind_one(crate::places::BoundPlace {
            target: format!("discord:{PLACE}"),
            name: "a private channel".into(),
            private: true,
            ..Default::default()
        });
    let res = turn(&r.core, &sid, START).await;
    until("the task's terminal left its job", || {
        !rows(&r.core, "term.left").is_empty()
    })
    .await;
    let left = rows(&r.core, "term.left");
    assert_eq!(left[0].1["ended"], false, "{left:?}");
    assert_eq!(left[0].1["by"], term::BY_SESSION_END);
    assert_eq!(left[0].1["left"][0]["program"], "sleep");
    assert_eq!(left[0].1["left"][0]["why"], term::pty::WHY_BACKGROUND);
    assert!(
        lives_marked(&bg),
        "the background job did not outlive its session"
    );
    until("the foreground program's end", || !lives_marked(&fg)).await;
    let health = r.core.health();
    assert_eq!(
        health.terminals_left.len(),
        1,
        "{:?}",
        health.terminals_left
    );
    assert_eq!(health.terminals_left[0].program, "sleep");
    let execution = res.execution_id.as_deref().unwrap();
    match by {
        term::BY_STOP => drop(r.core.stop_execution(execution, "test").await.unwrap()),
        _ => drop(r.core.cancel_execution(execution, "test").await.unwrap()),
    }
    until("the end of what was left", || !lives_marked(&bg)).await;
    let left = rows(&r.core, "term.left");
    assert_eq!(left.len(), 2, "{left:?}");
    let ended = left.iter().find(|(_, r)| r["ended"] == true).unwrap();
    assert_eq!(ended.1["by"], by, "{left:?}");
    assert!(r.core.health().terminals_left.is_empty());
    // The narrative's two lines in the task's session: what was left, and
    // its end (the join's review: no test read them).
    let task = left[0].0.clone().unwrap();
    let said: Vec<String> = r
        .core
        .narrator
        .tail()
        .into_iter()
        .filter(|l| l.session_id.as_deref() == Some(task.as_str()))
        .map(|l| l.text)
        .collect();
    let pid = &left[0].1["left"][0]["pid"];
    for line in [
        format!(
            "Left running as terminal t1 (bash) closed because its session ended: pid {pid} \
             (sleep, a background job)."
        ),
        format!(
            "Ended what terminal t1 (bash) left running, because {by}: pid {pid} (sleep, a \
             background job)."
        ),
    ] {
        assert!(
            said.iter().any(|t| t.contains(&line)),
            "{line:?} in {said:#?}"
        );
    }
}

/// An open that finds its session full of ended terminals reclaims the
/// oldest, and the reclaimed terminal's close is recorded: a `term.closed`
/// row by `term.open reclaimed its slot` (the join's review: no test read a
/// result's `reclaimed` into its row).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_open_that_reclaims_a_slot_records_the_reclaimed_close() {
    let r = rig(false, |req| {
        let (_, last, n) = asked(req);
        match (last.as_str(), n) {
            ("open five", n) if n < 5 => call(
                &format!("o{n}"),
                term::OPEN,
                json!({"argv": ["bash", "-c", format!("echo done-{n}")], "quiet_ms": 200}),
            ),
            _ => Scripted::text("Opened five."),
        }
    });
    let sid = session(&r.core);
    turn(&r.core, &sid, "open five").await;
    let closed = rows(&r.core, "term.closed");
    assert_eq!(closed.len(), 1, "{closed:?}");
    assert_eq!(closed[0].0.as_deref(), Some(sid.as_str()));
    assert_eq!(closed[0].1["terminal"], "t1", "{closed:?}");
    assert_eq!(closed[0].1["by"], term::BY_RECLAIM);
    assert_eq!(closed[0].1["exit"], 0);
    assert_eq!(r.core.tools.terms.of_session(&sid).len(), term::PER_SESSION);
}

/// `[tools.term] keep_background` and `[tools] proc_sync_secs` reach the
/// core's terminals (the join's review: no test read the config's key
/// through to them).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_configs_term_settings_reach_the_terminals() {
    for (keep, sync_secs, idle_max_ms) in [(false, 900, 900_000), (true, 30, 60_000)] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("work");
        std::fs::create_dir_all(&root).unwrap();
        let mut cfg = Config::example();
        cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
        cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
        cfg.tools.roots = vec![];
        cfg.tools.term.keep_background = keep;
        cfg.tools.proc_sync_secs = sync_secs;
        let store = Store::open(&dir.path().join("store")).unwrap();
        let model = Arc::new(Model(Box::new(|_| Scripted::text("."))));
        let core = Core::build(crate::rpc::Parts::for_tests(cfg, model, store)).unwrap();
        assert_eq!(core.tools.terms.keep_background, keep);
        assert_eq!(core.tools.terms.idle_max_ms, idle_max_ms);
    }
}

/// Esc's stop (`keep_jobs`, theseus-wy6v.1) leaves the session's terminals
/// open, their programs running and no close recorded (chat-ask's join
/// review: nothing held the terminals' half of the flag).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stop_keeping_jobs_leaves_the_sessions_terminals_open() {
    let r = rig(false, opener);
    let sid = session(&r.core);
    let res = turn(&r.core, &sid, "open one").await;
    let (id, pid) = only_terminal(&r.core, &sid);
    r.core
        .stop_execution_with(res.execution_id.as_deref().unwrap(), "test", true)
        .await
        .unwrap();
    assert!(r.core.tools.terms.get(&id).is_some(), "{id} was closed");
    assert!(lives(pid), "{id}'s program was ended");
    assert!(rows(&r.core, "term.closed").is_empty());
    r.core.finish_stop().await;
}
