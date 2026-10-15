//! A routine's trust through the whole core (theseus-52tut; the horizon
//! report's §5.4 and the owner's D-H6): the tools its grant names run without
//! asking even after the run reads a page, and everything else still waits;
//! a question nobody answers by the run's deadline ends the run `blocked`,
//! naming the tool, in one Interrupt; "Try it now" is run 0; a failing routine
//! pings once a local day with its streak; and the answers on its notice go
//! through `work.answer`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_protocol::{Message, Request};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::approval::{Client, Surface};
use crate::config::WebToolsConfig;
use crate::provider::{
    DeltaSink, FakeProvider, Provider, ProviderError, ProviderFuture, ProviderRequest, Scripted,
};
use crate::session::SessionRecord;
use crate::web::tests::{serve, web};
use crate::{Config, Core};

const TARGET: &str = "discord:dm:42";

type Script = Box<dyn Fn(&ProviderRequest) -> Scripted + Send + Sync>;

/// A stand-in model that answers each request from the request, or fails
/// every one while `fail` is set.
struct Model {
    script: Script,
    fail: AtomicBool,
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
            if self.fail.load(Ordering::SeqCst) {
                return Err(ProviderError::Auth {
                    status: 401,
                    message: "Gmail needs a login".into(),
                }
                .into());
            }
            FakeProvider::scripted(vec![(self.script)(req)])
                .stream_message(req, on_delta)
                .await
        })
    }
}

/// How many tool results the request's conversation holds: how far along a
/// scripted run is.
fn results(req: &ProviderRequest) -> usize {
    req.messages
        .iter()
        .filter_map(|m| m["content"].as_array())
        .flatten()
        .filter(|b| b["type"] == "tool_result")
        .count()
}

struct Rig {
    core: Arc<Core>,
    model: Arc<Model>,
    port: u16,
    dir: tempfile::TempDir,
    driver: tokio::task::JoinHandle<()>,
    _server: crate::web::tests::Server,
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.driver.abort();
    }
}

impl Rig {
    fn note(&self) -> String {
        self.dir.path().join("work/note.txt").display().to_string()
    }
}

/// The script a digest run follows: read the page, write the note, edit it,
/// and say it is done. `edit` is the call that is outside a short grant.
fn digest(port: u16, note: String) -> Script {
    Box::new(move |req| match results(req) {
        0 => Scripted::tools(
            "",
            &[(
                "f1",
                "http_fetch",
                json!({"url": format!("http://site.test:{port}/page.html")}),
            )],
        ),
        1 => Scripted::tools(
            "",
            &[(
                "w1",
                "fs_write",
                json!({"path": note, "content": "tide at 06:12\n"}),
            )],
        ),
        2 => Scripted::tools(
            "",
            &[(
                "e1",
                "fs_edit",
                json!({"path": note, "old_string": "06:12", "new_string": "06:13"}),
            )],
        ),
        _ => Scripted::text("Digest written."),
    })
}

async fn rig_with(script: impl FnOnce(u16, String) -> Script, deadline_secs: u64) -> Rig {
    let server = serve().await;
    let port = server.port;
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    let note = work
        .canonicalize()
        .unwrap()
        .join("note.txt")
        .display()
        .to_string();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(work.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = crate::policy::Posture::Notify;
    cfg.kernel.min_repeat_minutes = 1;
    cfg.routines.run_deadline_secs = deadline_secs;
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let model = Arc::new(Model {
        script: script(port, note),
        fail: AtomicBool::new(false),
    });
    let core = Core::build(crate::rpc::Parts {
        toollets: web(port, WebToolsConfig::default(), true).tools(),
        ..crate::rpc::Parts::for_tests(cfg, model.clone(), store)
    })
    .unwrap();
    core.bind_places(vec![crate::places::BoundPlace {
        target: TARGET.into(),
        name: "DM @owner".into(),
        private: true,
        ..Default::default()
    }]);
    let driver = tokio::spawn(crate::harness::drive(core.clone()));
    Rig {
        core,
        model,
        port,
        dir,
        driver,
        _server: server,
    }
}

async fn rig(deadline_secs: u64) -> Rig {
    rig_with(digest, deadline_secs).await
}

async fn rpc(core: &Arc<Core>, method: &str, params: Value) -> Result<Value, String> {
    let (ours, theirs) = tokio::io::duplex(64 * 1024);
    let (sr, sw) = tokio::io::split(theirs);
    let srv = tokio::spawn(core.clone().serve_connection(
        sr,
        sw,
        Client::new("sock#1", Surface::Cli),
    ));
    let (cr, mut cw) = tokio::io::split(ours);
    let req = Request::new(theseus_protocol::Id::Num(1), method, params);
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let mut lines = BufReader::new(cr).lines();
    let out = loop {
        let l = lines.next_line().await.unwrap().unwrap();
        if let Message::Response(r) = serde_json::from_str(&l).unwrap() {
            break r.result.ok_or_else(|| r.error.unwrap().message);
        }
    };
    cw.shutdown().await.unwrap();
    drop(lines);
    let _ = srv.await;
    out
}

async fn until(what: &str, secs: u64, mut f: impl FnMut() -> bool) {
    let t0 = Instant::now();
    while !f() {
        assert!(
            t0.elapsed() < Duration::from_secs(secs),
            "no {what} in {secs} s"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn rows(core: &Core, kind: &str) -> Vec<Value> {
    core.store
        .ledger_tail::<crate::ledger::LedgerRow>(100_000)
        .unwrap()
        .into_iter()
        .map(|(_, r)| r)
        .filter(|r| r.kind == kind)
        .map(|r| r.data)
        .collect()
}

/// The routine's notices posted to its owner's DM: `(purpose, text, answers)`.
fn notices(core: &Core) -> Vec<(String, String, Vec<String>)> {
    core.kernel
        .outbox_actions()
        .unwrap()
        .into_iter()
        .filter(|a| crate::outbox::kind_of(a) == "reminder")
        .map(|a| crate::outbox::body_of(&a).clone())
        .filter(|b| {
            b["purpose"]
                .as_str()
                .is_some_and(|p| p.starts_with("routine_"))
        })
        .map(|b| {
            (
                b["purpose"].as_str().unwrap().to_string(),
                b["text"].as_str().unwrap().to_string(),
                b["answers"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|a| a["id"].as_str().unwrap().to_string())
                    .collect(),
            )
        })
        .collect()
}

async fn make(core: &Arc<Core>, extra: Value) -> Result<Value, String> {
    let mut p = json!({"title": "Tide check", "schedule": "every 1m",
                       "brief": "Read the tide page and keep a note."});
    for (k, v) in extra.as_object().unwrap() {
        p[k] = v.clone();
    }
    rpc(core, "routine.create", p)
        .await
        .map(|v| v["routine"].clone())
}

async fn runs(core: &Arc<Core>, id: &str) -> Vec<Value> {
    rpc(core, "routine.runs", json!({"id": id})).await.unwrap()["runs"]
        .as_array()
        .unwrap()
        .clone()
}

async fn info(core: &Arc<Core>, id: &str) -> Value {
    rpc(core, "routine.runs", json!({"id": id})).await.unwrap()["routine"].clone()
}

fn external_of(core: &Core, task: &str) -> bool {
    core.store
        .get_session::<SessionRecord>(task)
        .unwrap()
        .unwrap()
        .external
        .is_some()
}

/// The tools a grant names run without asking after the run read a page: the
/// run reads, writes the note (granted), and edits it (granted too here), and
/// finishes, with no question asked; its notices name the grant. It is "Try
/// it now": run 0, not counted among the routine's runs.
#[tokio::test(flavor = "multi_thread")]
async fn a_granted_tool_runs_unasked_after_the_run_reads_a_page() {
    let r = rig(900).await;
    let made = make(&r.core, json!({"grant": ["fs.write", "fs.edit"]}))
        .await
        .unwrap();
    let id = made["id"].as_str().unwrap().to_string();
    assert_eq!(made["grant"], json!(["fs.write", "fs.edit"]));
    let tried = rpc(&r.core, "routine.try", json!({"id": id}))
        .await
        .unwrap();
    let task = tried["task"].as_str().unwrap().to_string();
    until("the trial to complete", 30, || {
        std::fs::read_to_string(r.note()).is_ok_and(|t| t.contains("06:13"))
    })
    .await;
    let listed = runs(&r.core, &id).await;
    until("run 0 to end", 30, || {
        r.core
            .kernel
            .tasks(None)
            .unwrap()
            .iter()
            .any(|t| t.session_id == task && t.state.is_terminal())
    })
    .await;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["run"], 0, "Try it now is run 0 in the log");
    assert!(external_of(&r.core, &task), "the run did read outside text");
    assert!(
        r.core.pending_confirms(&task).unwrap().is_empty(),
        "nothing asked"
    );
    assert!(rows(&r.core, "tool.confirm_requested").is_empty());
    let notified = serde_json::to_string(&rows(&r.core, "tool.notified")).unwrap();
    assert!(
        notified.contains("ran under the routine's grant: fs.write"),
        "{notified}"
    );
    // The trial is no run of the series: the first scheduled one is run 1.
    assert_eq!(info(&r.core, &id).await["runs"], 0);
}

/// The control of the first: with no grant the same run asks for its write
/// after the page, once, and a conversation's session with the same hold asks
/// too (the grant is the routine's run's alone).
#[tokio::test(flavor = "multi_thread")]
async fn without_a_grant_the_same_call_asks_and_so_does_any_other_session() {
    let r = rig(900).await;
    let made = make(&r.core, json!({})).await.unwrap();
    let id = made["id"].as_str().unwrap().to_string();
    let tried = rpc(&r.core, "routine.try", json!({"id": id}))
        .await
        .unwrap();
    let task = tried["task"].as_str().unwrap().to_string();
    until("the write to ask", 30, || {
        !r.core.pending_confirms(&task).unwrap().is_empty()
    })
    .await;
    let asked = r.core.pending_confirms(&task).unwrap();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].tool, "fs.write");
    assert!(!std::path::Path::new(&r.note()).exists());

    // An ordinary conversation that read the page asks too, for a routine
    // with a grant exists in the same core.
    make(&r.core, json!({"title": "Other", "grant": ["fs.write"]}))
        .await
        .unwrap();
    let rec = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    let conversation = rec.session_id.clone();
    let (live, _) = r.core.live_profile();
    let target = r
        .core
        .runner
        .resolve_target(&live, None, None, None)
        .unwrap();
    let sink = crate::bus::EventSink::new(r.core.bus.clone(), &rec.session_id, None);
    let res = r
        .core
        .runner
        .run(crate::turn::TurnRequest {
            prompt: None,
            session: rec,
            input: Some("read then write".into()),
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
    assert!(
        res.awaiting_confirm.is_some(),
        "a conversation asks: {res:?}"
    );
    // For the write itself: the routine's grant is not this session's.
    let asked = r.core.pending_confirms(&conversation).unwrap();
    assert_eq!(asked[0].tool, "fs.write", "{asked:?}");
}

/// A call outside the grant asks once, and with nobody there the run ends
/// `blocked` at the run's deadline, naming the tool, in one Interrupt with its
/// answers; never a silent expiry. "Add to grant" through `work.answer`
/// grants it, and the next run completes.
#[tokio::test(flavor = "multi_thread")]
async fn an_ungranted_tool_asks_once_and_the_run_ends_blocked_naming_it() {
    let r = rig(1).await;
    let made = make(&r.core, json!({"grant": ["fs.write"]})).await.unwrap();
    let id = made["id"].as_str().unwrap().to_string();
    rpc(&r.core, "routine.run_now", json!({"id": id}))
        .await
        .unwrap();
    until("the run to end blocked", 30, || {
        notices(&r.core).iter().any(|n| n.0 == "routine_blocked")
    })
    .await;
    let listed = runs(&r.core, &id).await;
    assert_eq!(listed[0]["state"], "blocked", "{listed:?}");
    assert_eq!(listed[0]["blocked"]["tool"], "fs.edit");
    assert_eq!(
        listed[0]["blocked"]["reason"],
        "fs.edit needs approval (not in its grant)"
    );
    // It asked once, and nobody was silently expired.
    assert_eq!(rows(&r.core, "tool.confirm_requested").len(), 1);
    assert!(rows(&r.core, "question.expired").is_empty());
    let n = notices(&r.core);
    assert_eq!(n.len(), 1, "{n:?}");
    assert_eq!(
        n[0].1,
        "✗ Tide check blocked: fs.edit needs approval (not in its grant)"
    );
    assert_eq!(n[0].2, ["add_to_grant", "run_now", "pause", "details"]);
    // The run's own cancel report is not posted: the routine tells it.
    let reports = r
        .core
        .kernel
        .outbox_actions()
        .unwrap()
        .into_iter()
        .filter(|a| crate::outbox::kind_of(a) == "report")
        .count();
    assert_eq!(reports, 0);
    assert_eq!(info(&r.core, &id).await["streak"], 1);

    // Add to grant: the question is the routine's notice (its wake).
    let wake = {
        let spec = r
            .core
            .runner
            .tools
            .routines
            .find(&r.core.store, &id)
            .unwrap();
        spec.wake_id.clone()
    };
    let answer = rpc(
        &r.core,
        "work.answer",
        json!({"question_id": wake, "option": "add_to_grant"}),
    )
    .await
    .unwrap();
    assert_eq!(answer["option"], "add_to_grant");
    assert_eq!(
        info(&r.core, &id).await["grant"],
        json!(["fs.write", "fs.edit"])
    );
    // Details are words.
    let details = rpc(
        &r.core,
        "work.answer",
        json!({"question_id": wake, "option": "details"}),
    )
    .await
    .unwrap();
    assert!(
        details["detail"].as_str().unwrap().contains("blocked"),
        "{details}"
    );

    rpc(&r.core, "routine.run_now", json!({"id": id}))
        .await
        .unwrap();
    until("the second run to complete", 30, || {
        std::fs::read_to_string(r.note()).is_ok_and(|t| t.contains("06:13"))
    })
    .await;
    until("its end", 30, || {
        r.core
            .kernel
            .tasks(None)
            .unwrap()
            .iter()
            .filter(|t| t.state.is_terminal())
            .count()
            == 2
    })
    .await;
    assert_eq!(runs(&r.core, &id).await[0]["state"], "complete");
    assert_eq!(notices(&r.core).len(), 1, "a good run tells nothing new");
    let _ = r.port;
}

/// A grant that names a tool no grant may, a tool that is not there, or a
/// post without its place is refused at creation, naming it; nothing is made.
#[tokio::test(flavor = "multi_thread")]
async fn a_grant_naming_a_denied_tool_is_refused_at_creation() {
    let r = rig(900).await;
    for (grant, says) in [
        (json!(["proc.run"]), "`proc.run` can't be in a grant"),
        (
            json!(["fs.write", "no.such"]),
            "`no.such` is not a tool you have",
        ),
        (json!(["channel.post"]), "granted to a place"),
    ] {
        let e = make(&r.core, json!({ "grant": grant })).await.unwrap_err();
        assert!(e.contains(says), "{e}");
    }
    let list = rpc(&r.core, "routine.list", json!({})).await.unwrap();
    assert!(list["routines"].as_array().unwrap().is_empty());
    // An update that names one is refused too, and the grant stays.
    let made = make(&r.core, json!({"grant": ["fs.write"]})).await.unwrap();
    let id = made["id"].as_str().unwrap();
    let e = rpc(
        &r.core,
        "routine.update",
        json!({"id": id, "add_grant": ["proc.run"]}),
    )
    .await
    .unwrap_err();
    assert!(e.contains("proc.run"), "{e}");
    assert_eq!(info(&r.core, id).await["grant"], json!(["fs.write"]));
}

/// The grant is the routine's run's alone: a task the run starts has the run
/// as its parent, a task of another conversation has none, and a routine with
/// no grant gives none.
#[tokio::test(flavor = "multi_thread")]
async fn the_grant_applies_to_the_routines_own_runs_and_nothing_else() {
    let r = rig(900).await;
    let made = make(&r.core, json!({"grant": ["fs.write"]})).await.unwrap();
    let id = made["id"].as_str().unwrap();
    let spec = r
        .core
        .runner
        .tools
        .routines
        .find(&r.core.store, id)
        .unwrap();
    let book = &r.core.runner.tools.routines;
    let task_of = |parent: &str, exec: &str| crate::session::TaskOf {
        parent_session: parent.into(),
        parent_execution: exec.into(),
        by: "wake".into(),
        target: None,
        arrangement: None,
        check: None,
    };
    let of = |t: crate::session::TaskOf| book.grant_for(&r.core.kernel, &r.core.store, &t);
    let own = of(task_of(&spec.session_id, &spec.execution_id)).expect("the run's own grant");
    assert!(own.covers("fs.write", &[]));
    assert!(!own.covers("fs.edit", &[]));
    // A task a run starts: its parent is the run's task session.
    assert!(of(task_of("ses_the_run_itself", "exe_the_run_itself")).is_none());
    // A task of some other conversation, and a forged execution.
    assert!(of(task_of("ses_other", &spec.execution_id)).is_none());
    assert!(of(task_of(&spec.session_id, "exe_other")).is_none());
    // A routine with no grant gives none.
    let bare = make(&r.core, json!({"title": "Bare"})).await.unwrap();
    let bare = r
        .core
        .runner
        .tools
        .routines
        .find(&r.core.store, bare["id"].as_str().unwrap())
        .unwrap();
    assert!(of(task_of(&bare.session_id, &bare.execution_id)).is_none());
}

/// A routine that fails pings once a local day, with the streak: the first
/// failure tells it, the second the same day tells nothing and extends the
/// streak, and after midnight the next one tells it again, "(3 in a row)".
/// The runs' own failure reports are not posted. (The day turns by rewinding
/// the routine's ping book: the core's clock is the real one.)
#[tokio::test(flavor = "multi_thread")]
async fn a_failing_routine_pings_once_a_day_with_its_streak() {
    let r = rig(900).await;
    r.model.fail.store(true, Ordering::SeqCst);
    let made = make(&r.core, json!({})).await.unwrap();
    let id = made["id"].as_str().unwrap().to_string();
    let spec = r
        .core
        .runner
        .tools
        .routines
        .find(&r.core.store, &id)
        .unwrap();

    rpc(&r.core, "routine.run_now", json!({"id": id}))
        .await
        .unwrap();
    until("the first failure's notice", 30, || {
        notices(&r.core).len() == 1
    })
    .await;
    let first = &notices(&r.core)[0];
    assert_eq!(first.0, "routine_failed");
    assert!(first.1.starts_with("✗ Tide check failed: "), "{}", first.1);
    assert!(!first.1.contains("in a row"), "{}", first.1);
    assert_eq!(first.2, ["run_now", "pause", "details"]);

    rpc(&r.core, "routine.run_now", json!({"id": id}))
        .await
        .unwrap();
    until("the second failure to be settled", 30, || {
        rows(&r.core, "routine.changed")
            .iter()
            .any(|c| c["what"].as_str().is_some_and(|w| w.contains("told today")))
    })
    .await;
    assert_eq!(notices(&r.core).len(), 1, "the same day tells it once");
    assert_eq!(info(&r.core, &id).await["streak"], 2);

    // Midnight passes.
    let ping = crate::routine::watch::ping(&r.core.store, &spec.session_id);
    let back = theseus_protocol::routines::RoutinePing {
        day: "2000-01-01".into(),
        ..ping
    };
    r.core
        .store
        .append(&[crate::routine::watch::ping_record(&spec.session_id, &back).unwrap()])
        .unwrap();
    rpc(&r.core, "routine.run_now", json!({"id": id}))
        .await
        .unwrap();
    until("the next day's notice", 30, || notices(&r.core).len() == 2).await;
    assert!(
        notices(&r.core)[1].1.contains("(3 in a row)"),
        "{:?}",
        notices(&r.core)
    );
    // No failure report of the runs' own.
    let reports = r
        .core
        .kernel
        .outbox_actions()
        .unwrap()
        .into_iter()
        .filter(|a| crate::outbox::kind_of(a) == "report")
        .count();
    assert_eq!(reports, 0);
    // A good run ends the streak and tells nothing.
    r.model.fail.store(false, Ordering::SeqCst);
    let listed = runs(&r.core, &id).await;
    assert!(listed.iter().all(|x| x["state"] == "failed"), "{listed:?}");
}
