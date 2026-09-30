//! External text through the whole core (T1, theseus-9bp): after a session
//! reads a page, a call that acts waits for approval, with the reason, in the
//! same turn and the next, until the operator trusts the session again; a
//! read keeps its posture; a job's process cannot trust it; the hold survives
//! a restart; a task that a holding session starts holds it, a report from a
//! holding task gives it to its parent, and a wake's turn keeps it, while
//! setting the wake keeps its posture (T1b); and a session that read nothing
//! is unchanged.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_protocol::{ExternalText, SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::config::WebToolsConfig;
use crate::node::{Body, ResultStatus};
use crate::policy::Posture;
use crate::provider::{
    DeltaSink, FakeProvider, Provider, ProviderFuture, ProviderRequest, Scripted,
};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::web::tests::{serve, web};
use crate::{Config, Core};

/// A stand-in model that answers each request from the request itself.
struct Model {
    script: Box<dyn Fn(&ProviderRequest) -> Scripted + Send + Sync>,
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
            let answer = (self.script)(req);
            FakeProvider::scripted(vec![answer])
                .stream_message(req, on_delta)
                .await
        })
    }
}

/// The text of the request's last user message that has any, and how many
/// tool results come after it: "what was asked, and how far along it is".
fn asked(req: &ProviderRequest) -> (String, usize) {
    let texts = |m: &Value| -> String {
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
    };
    let at = req
        .messages
        .iter()
        .rposition(|m| m["role"] == "user" && !texts(m).is_empty())
        .unwrap_or(0);
    let results = req.messages[at..]
        .iter()
        .filter_map(|m| m["content"].as_array())
        .flatten()
        .filter(|b| b["type"] == "tool_result")
        .count();
    (texts(&req.messages[at]), results)
}

struct Rig {
    core: Arc<Core>,
    port: u16,
    _server: crate::web::tests::Server,
    dir: tempfile::TempDir,
}

fn config(root: &Path, state: &Path) -> Config {
    let mut cfg = Config::example();
    cfg.server.state_dir = state.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.tools.proc_sync_secs = 10;
    // Eddie's posture: the writers and proc.run inherit notify.
    cfg.policy.enforcement = Posture::Notify;
    cfg
}

/// A core whose web tools reach a test's server as `site.test`, over the
/// store in `dir`. With `driver`, the driver takes queued turns (tasks,
/// wakes, reports), as the daemon's does.
fn core_in(
    dir: &Path,
    port: u16,
    script: impl Fn(&ProviderRequest) -> Scripted + Send + Sync + 'static,
    driver: bool,
) -> Arc<Core> {
    let root = dir.join("work");
    std::fs::create_dir_all(&root).unwrap();
    let cfg = config(&root.canonicalize().unwrap(), dir);
    let store = Store::open(&dir.join("store")).unwrap();
    let model = Arc::new(Model {
        script: Box::new(script),
    });
    let core = Core::build(crate::rpc::Parts {
        toollets: web(port, WebToolsConfig::default(), true).tools(),
        ..crate::rpc::Parts::for_tests(cfg, model, store)
    })
    .unwrap();
    if driver {
        tokio::spawn(crate::harness::drive(core.clone()));
    }
    core
}

async fn rig(
    script: impl Fn(&ProviderRequest) -> Scripted + Send + Sync + 'static,
    driver: bool,
) -> Rig {
    let server = serve().await;
    let dir = tempfile::tempdir().unwrap();
    let core = core_in(dir.path(), server.port, script, driver);
    Rig {
        core,
        port: server.port,
        _server: server,
        dir,
    }
}

fn page(port: u16) -> String {
    format!("http://site.test:{port}/page.html")
}

fn fetch(id: &str, url: &str) -> Scripted {
    Scripted::tools("", &[(id, "http_fetch", json!({ "url": url }))])
}

fn run(id: &str, word: &str) -> Scripted {
    Scripted::tools("", &[(id, "proc_run", json!({"argv": ["echo", word]}))])
}

fn session(core: &Core) -> String {
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
            config_wait_us: 0,
            reply_to: None,
        })
        .await
        .unwrap()
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

fn hold(core: &Core, sid: &str) -> Option<ExternalText> {
    core.store
        .get_session::<SessionRecord>(sid)
        .unwrap()
        .unwrap()
        .external
}

/// The session's one fetch result node.
fn fetched_node(core: &Core, sid: &str) -> String {
    core.store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match &n.body {
            Body::ToolResult {
                tool,
                status: ResultStatus::Ok,
                ..
            } if tool == "http.fetch" => Some(n.id.clone()),
            _ => None,
        })
        .expect("a fetch result")
}

/// The reason the brief gives, for a session that read `url`.
fn waits_because(url: &str) -> (String, &'static str) {
    (
        format!("proc.run — approve (this session read external text (http.fetch {url}, at "),
        "), and a call that acts waits for approval after that (§3.9))",
    )
}

fn assert_waits_for(reason: &str, url: &str) {
    let (head, tail) = waits_because(url);
    assert!(reason.contains(&head), "{reason}");
    assert!(reason.ends_with(tail), "{reason}");
}

async fn until(what: &str, mut f: impl FnMut() -> bool) {
    let t0 = Instant::now();
    while !f() {
        assert!(t0.elapsed() < Duration::from_secs(20), "no {what} in 20 s");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// The brief's first two tests, and the hold's records: a fetch, then
/// `proc.run` in the same turn, waits with the reason; the next turn waits
/// too; the operator's trust clears it, and `proc.run` is back at `notify`.
/// The read is recorded once: the hold on the session record, and one
/// `session.external_read` row naming the node, the tool, and the URL.
#[tokio::test]
async fn a_fetch_then_a_run_waits_in_this_turn_and_the_next_until_the_operator_trusts_it() {
    let r = rig(
        |req| {
            let (said, results) = asked(req);
            let url = said.split_whitespace().last().unwrap_or("").to_string();
            match (said.as_str(), results) {
                (s, 0) if s.starts_with("read") => fetch("f1", &url),
                (s, 1) if s.starts_with("read") => run("r1", "hi"),
                (s, 0) if s.starts_with("again") => run("r2", "again"),
                (s, 0) if s.starts_with("now") => run("r3", "notice"),
                _ => Scripted::text("Done."),
            }
        },
        false,
    )
    .await;
    let (core, url) = (&r.core, page(r.port));
    let sid = session(core);
    let res = turn(core, &sid, &format!("read then run {url}")).await;
    let corr = res.awaiting_confirm.clone().expect("the run waits");
    let asked = ledgered(core, "tool.confirm_requested");
    assert_eq!(asked.len(), 1);
    assert_waits_for(asked[0]["reason"].as_str().unwrap(), &url);
    assert!(asked[0]["reason"]
        .as_str()
        .unwrap()
        .starts_with("run `echo hi` in "));
    // The confirm names what the session read, so a card can offer trust.
    let pending = core.pending_confirms(&sid).unwrap();
    assert_eq!(pending[0].external_text.as_ref().unwrap().url, url);
    // Recorded once, on the session record and in the ledger.
    let h = hold(core, &sid).expect("the session holds external text");
    assert_eq!(
        (h.tool.as_str(), h.url.as_str(), h.via.as_deref()),
        ("http.fetch", url.as_str(), None)
    );
    assert_eq!(h.node_id, fetched_node(core, &sid));
    let read = ledgered(core, "session.external_read");
    assert_eq!(read.len(), 1);
    assert_eq!(
        (read[0]["node_id"].as_str(), read[0]["url"].as_str()),
        (Some(h.node_id.as_str()), Some(url.as_str()))
    );
    let health = core.health();
    assert_eq!(health.external_text.len(), 1);
    assert_eq!(health.external_text[0].session_id, sid);
    assert_eq!(health.external_text[0].held, h);

    // Declined, and the next turn waits too.
    core.confirm_action(&corr, false, Some("not now"), "test")
        .unwrap();
    core.continue_execution(res.execution_id.as_deref().unwrap())
        .await
        .unwrap();
    let res = turn(core, &sid, "again, please").await;
    let corr = res
        .awaiting_confirm
        .clone()
        .expect("the next turn waits too");
    let asked = ledgered(core, "tool.confirm_requested");
    assert_eq!(asked.len(), 2);
    assert_waits_for(asked[1]["reason"].as_str().unwrap(), &url);
    assert_eq!(ledgered(core, "session.external_read").len(), 1);

    // The operator trusts it; the waiting call was asked before, so it is
    // answered as it stands, and the next proc.run is back at notify.
    let t = core.trust_session(&sid, "test").unwrap();
    assert_eq!(
        (t.how.as_str(), t.held.url.as_str()),
        ("policy.trust", url.as_str())
    );
    assert!(hold(core, &sid).is_none());
    assert!(core.health().external_text.is_empty());
    let trusted = ledgered(core, "session.trusted");
    assert_eq!(trusted.len(), 1);
    assert_eq!(trusted[0]["held"]["url"], url.as_str());
    let again = core.trust_session(&sid, "test").unwrap_err();
    assert!(
        again.to_string().contains("holds no external text"),
        "{again}"
    );
    core.confirm_action(&corr, false, None, "test").unwrap();
    core.continue_execution(res.execution_id.as_deref().unwrap())
        .await
        .unwrap();
    let res = turn(core, &sid, "now run it").await;
    assert!(res.awaiting_confirm.is_none(), "{res:?}");
    // The fetch was a notice; of the runs, only the last.
    let runs: Vec<Value> = ledgered(core, "tool.notified")
        .into_iter()
        .filter(|n| n["tool"] == "proc.run")
        .collect();
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert_eq!(
        (
            runs[0]["input"]["argv"][1].as_str(),
            runs[0]["setting"].as_str()
        ),
        (Some("notice"), Some("enforcement = notify"))
    );
}

/// A `Read` call after the fetch keeps its posture: `fs.read` stays open and
/// a second fetch stays at notify, and neither writes a second read row.
#[tokio::test]
async fn a_read_after_the_fetch_keeps_its_posture() {
    let r = rig(
        |req| {
            let (said, results) = asked(req);
            let url = said.split_whitespace().last().unwrap_or("").to_string();
            match results {
                0 => fetch("f1", &url),
                1 => Scripted::tools(
                    "",
                    &[
                        ("g1", "fs_read", json!({"path": "notes.txt"})),
                        (
                            "f2",
                            "http_fetch",
                            json!({"url": url.replace("page.html", "plain.txt")}),
                        ),
                    ],
                ),
                _ => Scripted::text("Read both."),
            }
        },
        false,
    )
    .await;
    std::fs::write(r.dir.path().join("work/notes.txt"), "a note\n").unwrap();
    let sid = session(&r.core);
    let res = turn(&r.core, &sid, &format!("read {}", page(r.port))).await;
    assert!(res.awaiting_confirm.is_none(), "{res:?}");
    assert_eq!(res.tool_calls, 3);
    assert!(ledgered(&r.core, "tool.confirm_requested").is_empty());
    let notified: Vec<String> = ledgered(&r.core, "tool.notified")
        .iter()
        .map(|n| n["tool"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(notified, ["http.fetch", "http.fetch"], "fs.read stays open");
    assert!(hold(&r.core, &sid).is_some());
    assert_eq!(ledgered(&r.core, "session.external_read").len(), 1);
}

/// A job's process cannot trust a session again (J1's rule, theseus-6qy):
/// `policy.trust` from a process under a live job wrapper is refused with the
/// job, ledgered as `approval.refused`, and the session keeps its hold. The
/// operator's own trust, from outside every job, counts.
#[tokio::test]
async fn a_jobs_process_cannot_trust_a_session_again() {
    use crate::approval::Surface::Cli;
    let job = crate::peer::Standin::start("act_truster");
    let r = rig(
        |req| match asked(req) {
            (said, 0) => fetch("f1", said.split_whitespace().last().unwrap()),
            _ => Scripted::text("Read."),
        },
        false,
    )
    .await;
    let sid = session(&r.core);
    turn(&r.core, &sid, &format!("read {}", page(r.port))).await;
    assert!(hold(&r.core, &sid).is_some());
    let params = json!({"session_id": sid});
    let client = crate::approval::Client::new("sock#7", Cli)
        .with_peer(crate::peer::Peer::process(job.child));
    let e = rpc_as(
        &r.core,
        client,
        theseus_protocol::method::POLICY_TRUST,
        params.clone(),
    )
    .await
    .expect_err("refused");
    assert_eq!(e.code, theseus_protocol::error_code::REFUSED, "{e:?}");
    let why = format!(
        "from a Theseus job's process (job act_truster, pid {}, sleep)",
        job.child
    );
    assert!(e.message.contains(&why), "{}", e.message);
    let refused = ledgered(&r.core, "approval.refused");
    assert_eq!(refused.len(), 1);
    assert_eq!(
        (
            refused[0]["act"].as_str(),
            refused[0]["session_id"].as_str()
        ),
        (Some("policy.trust"), Some(sid.as_str()))
    );
    assert_eq!(refused[0]["from_job"], true);
    assert!(
        hold(&r.core, &sid).is_some(),
        "it still holds external text"
    );
    assert!(ledgered(&r.core, "session.trusted").is_empty());

    if crate::peer::tests_support::inside_a_job() {
        return;
    }
    let client = crate::approval::Client::new("sock#1", Cli)
        .with_peer(crate::peer::Peer::process(std::process::id()));
    let ok = rpc_as(
        &r.core,
        client,
        theseus_protocol::method::POLICY_TRUST,
        params,
    )
    .await
    .unwrap();
    assert_eq!(
        (ok["by"].as_str(), ok["via"].as_str()),
        (Some("the CLI"), Some("cli"))
    );
    assert!(hold(&r.core, &sid).is_none());
    let trusted = ledgered(&r.core, "session.trusted");
    assert_eq!(trusted[0]["asker"]["pid"], std::process::id());
}

/// The hold is the session's own record, so a restart keeps it: after the
/// daemon's core is rebuilt over the same store, the session still holds
/// external text, health lists it, and its next `proc.run` still waits.
#[tokio::test]
async fn the_hold_survives_a_restart() {
    let server = serve().await;
    let url = page(server.port);
    let dir = tempfile::tempdir().unwrap();
    let script = |req: &ProviderRequest| match asked(req) {
        (said, 0) if said.starts_with("read") => {
            fetch("f1", said.split_whitespace().last().unwrap())
        }
        (said, 0) if said.starts_with("run") => run("r1", "after"),
        _ => Scripted::text("Done."),
    };
    let sid = {
        let core = core_in(dir.path(), server.port, script, false);
        let sid = session(&core);
        turn(&core, &sid, &format!("read {url}")).await;
        assert!(hold(&core, &sid).is_some());
        sid
    };
    let core = core_in(dir.path(), server.port, script, false);
    assert_eq!(core.health().external_text.len(), 1);
    let res = turn(&core, &sid, "run it now").await;
    assert!(
        res.awaiting_confirm.is_some(),
        "still held after the restart"
    );
    let p = core.pending_confirms(&sid).unwrap();
    assert_waits_for(&p[0].reason, &url);
}

/// A session that read no external text is unchanged: its `proc.run` runs at
/// notify, and nothing about external text is written or listed.
#[tokio::test]
async fn a_session_with_no_external_text_is_unchanged() {
    let r = rig(
        |req| match asked(req) {
            (_, 0) => run("r1", "plain"),
            _ => Scripted::text("Ran."),
        },
        false,
    )
    .await;
    let sid = session(&r.core);
    let res = turn(&r.core, &sid, "run it").await;
    assert!(res.awaiting_confirm.is_none(), "{res:?}");
    assert_eq!(ledgered(&r.core, "tool.notified").len(), 1);
    assert!(ledgered(&r.core, "session.external_read").is_empty());
    assert!(hold(&r.core, &sid).is_none());
    assert!(r.core.health().external_text.is_empty());
    let e = r.core.trust_session(&sid, "test").unwrap_err();
    assert!(e.to_string().contains("holds no external text"), "{e}");
}

/// "Approve + trust session": the approval runs the waiting call and trusts
/// the session again in one answer, so the next call that acts in the same
/// turn runs at notify. `session.trusted` names the approval.
#[tokio::test]
async fn an_approval_with_trust_runs_the_call_and_trusts_the_session() {
    let r = rig(
        |req| {
            let (said, results) = asked(req);
            match results {
                0 => fetch("f1", said.split_whitespace().last().unwrap()),
                1 => run("r1", "approved"),
                2 => run("r2", "after"),
                _ => Scripted::text("Both ran."),
            }
        },
        false,
    )
    .await;
    let sid = session(&r.core);
    let res = turn(&r.core, &sid, &format!("read {}", page(r.port))).await;
    let corr = res.awaiting_confirm.clone().expect("the run waits");
    let e = r
        .core
        .confirm_action_with(&corr, false, None, "test", true)
        .unwrap_err();
    assert!(e.to_string().contains("trust goes with an approval"), "{e}");
    let ok = r
        .core
        .confirm_action_with(&corr, true, None, "test", true)
        .unwrap();
    assert!(ok.approved);
    assert!(hold(&r.core, &sid).is_none());
    let trusted = ledgered(&r.core, "session.trusted");
    assert_eq!(
        (
            trusted[0]["how"].as_str(),
            trusted[0]["correlation_id"].as_str()
        ),
        (Some("action.confirm"), Some(corr.as_str()))
    );
    let cont = r
        .core
        .continue_execution(res.execution_id.as_deref().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert!(cont.awaiting_confirm.is_none(), "{cont:?}");
    assert_eq!(cont.output, "Both ran.");
    let notified: Vec<String> = ledgered(&r.core, "tool.notified")
        .iter()
        .map(|n| n["input"]["argv"][1].as_str().unwrap_or("").to_string())
        .collect();
    assert!(notified.contains(&"after".to_string()), "{notified:?}");
}

/// A task that a holding session starts holds the external text too, from
/// its brief: `task.create` itself waits (it is a run), and once approved the
/// child's record holds it, taken from the parent, and the child's own
/// `proc.run` waits, saying the session that started it had read it.
#[tokio::test]
async fn a_task_that_a_holding_session_starts_holds_it_too() {
    let r = rig(
        |req| {
            let (said, results) = asked(req);
            if said.starts_with("[Task ") {
                return match results {
                    0 => run("c1", "child"),
                    _ => Scripted::text("The child's run was declined."),
                };
            }
            match results {
                0 => fetch("f1", said.split_whitespace().last().unwrap()),
                1 => Scripted::tools(
                    "",
                    &[("t1", "task_create", json!({"brief": "Run echo child"}))],
                ),
                _ => Scripted::text("Started it."),
            }
        },
        true,
    )
    .await;
    let (core, url) = (&r.core, page(r.port));
    let sid = session(core);
    let res = turn(core, &sid, &format!("read then delegate {url}")).await;
    let corr = res.awaiting_confirm.clone().expect("task.create waits");
    let p = core.pending_confirms(&sid).unwrap();
    assert!(
        p[0].reason
            .contains("task.create — approve (this session read external text"),
        "{}",
        p[0].reason
    );
    core.confirm_action(&corr, true, None, "test").unwrap();
    let parent_exec = res.execution_id.clone().unwrap();
    let child = {
        let mut child = None;
        until("the task", || {
            child = core
                .kernel
                .tasks(Some(parent_exec.as_str()))
                .unwrap()
                .into_iter()
                .next()
                .map(|t| t.session_id);
            child.is_some()
        })
        .await;
        child.unwrap()
    };
    let h = hold(core, &child).expect("the task holds it from its start");
    assert_eq!(
        (h.via.as_deref(), h.from_session.as_deref(), h.url.as_str()),
        (Some("task.create"), Some(sid.as_str()), url.as_str())
    );
    until("the child's call to wait", || {
        core.confirm_list()
            .unwrap()
            .iter()
            .any(|c| c.session_id == child)
    })
    .await;
    let c = core
        .confirm_list()
        .unwrap()
        .into_iter()
        .find(|c| c.session_id == child)
        .unwrap();
    assert_eq!(c.tool, "proc.run");
    assert!(
        c.reason.contains(&format!(
            "(http.fetch {url}, which session {} had read before it started this task, at ",
            crate::task::short(&sid)
        )),
        "{}",
        c.reason
    );
    assert_eq!(
        c.external_text.as_ref().unwrap().via.as_deref(),
        Some("task.create")
    );
    let reads: Vec<Value> = ledgered(core, "session.external_read");
    assert_eq!(
        reads.len(),
        2,
        "the parent's read, and the task's: {reads:?}"
    );
    assert_eq!(reads[1]["via"], "task.create");
}

/// A report from a task that read external text gives it to its parent, in
/// the frame that writes the report: a clean parent starts a task with
/// `wake_parent` (at notify); the task fetches a page; its report starts the
/// parent's next turn (W1), and that turn's `proc.run` waits, saying the
/// text came in the task's report.
#[tokio::test]
async fn a_report_from_a_task_that_read_external_text_makes_its_parent_hold_it() {
    let r = rig(
        |req| {
            let (said, results) = asked(req);
            if said.starts_with("[Task ") {
                let url = said.split_whitespace().last().unwrap_or("").to_string();
                return match results {
                    0 => fetch("c1", &url),
                    _ => Scripted::text("The page says hello."),
                };
            }
            if said.starts_with("[Report from task") {
                return match results {
                    0 => run("p2", "parent"),
                    _ => Scripted::text("Declined."),
                };
            }
            match results {
                0 => {
                    let url = said.split_whitespace().last().unwrap_or("").to_string();
                    Scripted::tools(
                        "",
                        &[(
                            "t1",
                            "task_create",
                            json!({"brief": format!("Fetch the page {url}"), "wake_parent": true}),
                        )],
                    )
                }
                _ => Scripted::text("Started it."),
            }
        },
        true,
    )
    .await;
    let (core, url) = (&r.core, page(r.port));
    let sid = session(core);
    let res = turn(core, &sid, &format!("delegate the reading of {url}")).await;
    assert!(
        res.awaiting_confirm.is_none(),
        "a clean session starts it at notify"
    );
    assert!(hold(core, &sid).is_none());
    until("the report's turn to wait", || {
        core.confirm_list()
            .unwrap()
            .iter()
            .any(|c| c.session_id == sid)
    })
    .await;
    let c = core
        .confirm_list()
        .unwrap()
        .into_iter()
        .find(|c| c.session_id == sid)
        .unwrap();
    let h = hold(core, &sid).expect("the parent holds it now");
    let task = h.from_session.clone().unwrap();
    assert_eq!(
        (h.via.as_deref(), h.url.as_str()),
        (Some("task.report"), url.as_str())
    );
    assert!(
        c.reason.contains(&format!(
            "(http.fetch {url}, in task {}'s report, at ",
            crate::task::short(&task)
        )),
        "{}",
        c.reason
    );
    // The hold names the report node, which the same frame wrote.
    let report = core
        .store
        .session_nodes(&sid)
        .unwrap()
        .into_iter()
        .find(|(_, n)| n.author.as_deref().is_some_and(|a| a.starts_with("task:")))
        .map(|(_, n)| n.id)
        .unwrap();
    assert_eq!(h.node_id, report);
    assert!(hold(core, &task).is_some(), "the task keeps its own");
}

/// `wake.at` keeps its posture in a holding session (T1b), and a wake's turn
/// is the session's own turn, so the hold covers it: a turn reads a page,
/// then sets a wake, which runs at notify with no wait; the wake's turn
/// proposes `proc.run`, and it waits with the reason.
#[tokio::test]
async fn a_wakes_turn_follows_the_rule() {
    let r = rig(
        |req| {
            let (said, results) = asked(req);
            if said.contains("WAKE-NOTE") {
                return match results {
                    0 => run("w1", "woke"),
                    _ => Scripted::text("Declined."),
                };
            }
            let url = said.split_whitespace().last().unwrap_or("").to_string();
            match results {
                0 => fetch("f1", &url),
                1 => Scripted::tools(
                    "",
                    &[(
                        "k1",
                        "wake_at",
                        json!({"after": "1s", "note": "WAKE-NOTE run it"}),
                    )],
                ),
                _ => Scripted::text("Read, and set."),
            }
        },
        true,
    )
    .await;
    let (core, url) = (&r.core, page(r.port));
    let sid = session(core);
    let res = turn(core, &sid, &format!("read, then remind me {url}")).await;
    assert!(
        res.awaiting_confirm.is_none(),
        "the wake is set with no wait, after the page: {res:?}"
    );
    assert!(hold(core, &sid).is_some());
    let set: Vec<Value> = ledgered(core, "tool.notified")
        .into_iter()
        .filter(|n| n["tool"] == "wake.at")
        .collect();
    assert_eq!(set.len(), 1, "{set:?}");
    assert_eq!(set[0]["setting"], "enforcement = notify", "its own posture");
    until("the wake's turn to wait", || {
        core.confirm_list()
            .unwrap()
            .iter()
            .any(|c| c.session_id == sid)
    })
    .await;
    assert_eq!(ledgered(core, "wake.fired").len(), 1);
    let c = core
        .confirm_list()
        .unwrap()
        .into_iter()
        .find(|c| c.session_id == sid)
        .unwrap();
    assert_eq!(c.tool, "proc.run");
    assert_waits_for(&c.reason, &url);
    let asked: Vec<Value> = ledgered(core, "tool.confirm_requested");
    assert_eq!(asked.len(), 1, "only the wake's run waited: {asked:?}");
    assert_eq!(asked[0]["tool"], "proc.run");
}

/// A request over a real protocol connection accepted as `client`.
async fn rpc_as(
    core: &Arc<Core>,
    client: crate::approval::Client,
    method: &str,
    params: Value,
) -> Result<Value, theseus_protocol::RpcError> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let (ours, theirs) = tokio::io::duplex(64 * 1024);
    let (sr, sw) = tokio::io::split(theirs);
    let srv = tokio::spawn(core.clone().serve_connection(sr, sw, client));
    let (cr, mut cw) = tokio::io::split(ours);
    let req = theseus_protocol::Request::new(theseus_protocol::Id::Num(1), method, params);
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let mut lines = BufReader::new(cr).lines();
    let out = loop {
        let l = lines.next_line().await.unwrap().unwrap();
        if let theseus_protocol::Message::Response(r) = serde_json::from_str(&l).unwrap() {
            break match (r.result, r.error) {
                (Some(v), _) => Ok(v),
                (None, e) => Err(e.unwrap()),
            };
        }
    };
    cw.shutdown().await.unwrap();
    drop(lines);
    let _ = srv.await;
    out
}
