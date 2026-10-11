//! A job's handle (theseus-n8gk), through the whole core: `proc.run
//! {background: true}` answers at once with a short id and the job's output
//! path; `job.read` shows its newest lines and the session's running jobs;
//! `job.wait` gives its result inside its window and says it runs past it;
//! a job that ends while its turn runs reaches the model before the next
//! request; another session's job is refused by name; and the gate judges
//! `job.read` as a read and `job.stop` as a run of the job's own program.
//! Jobs run in process (`InlineLauncher`), so none is stopped here (its
//! "wrapper" is this process): `theseusd/tests/job_handles.rs` stops real
//! ones. A task stands in for the daemon's drain, which takes a background
//! job's completion.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_protocol::{SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::node::{Body, ResultStatus};
use crate::policy::Posture;
use crate::provider::{FakeProvider, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

struct Rig {
    core: Arc<Core>,
    fake: Arc<FakeProvider>,
    drain: tokio::task::JoinHandle<()>,
    _dir: tempfile::TempDir,
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.drain.abort();
    }
}

/// A deployment whose `proc.run` runs with a notice, in process, waiting
/// `sync` seconds, with a drain every 20 ms as the daemon's harness drains
/// on each wrapper's word.
fn rig(script: Vec<Scripted>, sync: u64, edit: impl FnOnce(&mut Config)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().join("state").to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.tools.proc_sync_secs = sync;
    cfg.policy.enforcement = Posture::Notify;
    cfg.policy.tools.remove("proc.run");
    edit(&mut cfg);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake.clone(), store)).unwrap();
    let c = core.clone();
    let drain = tokio::spawn(async move {
        loop {
            c.drain_spool();
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });
    Rig {
        core,
        fake,
        drain,
        _dir: dir,
    }
}

impl Rig {
    async fn turn(&self, session: Option<&str>, input: &str) -> TurnSubmitResult {
        let rec = match session {
            Some(id) => self
                .core
                .store
                .get_session::<SessionRecord>(id)
                .unwrap()
                .unwrap(),
            None => {
                let r = SessionRecord::new(SessionKind::Conversation, None);
                self.core.store.put_session(&r.session_id, &r).unwrap();
                r
            }
        };
        let (live, _) = self.core.live_profile();
        let target = self
            .core
            .runner
            .resolve_target(&live, None, None, None)
            .unwrap();
        let sink = EventSink::new(self.core.bus.clone(), &rec.session_id, None);
        self.core
            .runner
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

    /// The result of call `id` in `session`: its status, text, meta, and
    /// correlation id.
    fn result(&self, session: &str, id: &str) -> (ResultStatus, String, Value, Option<String>) {
        self.core
            .store
            .session_nodes(session)
            .unwrap()
            .into_iter()
            .find_map(|(_, n)| match n.body {
                Body::ToolResult {
                    tool_use_id,
                    status,
                    content,
                    meta,
                    correlation_id,
                    late: false,
                    ..
                } if tool_use_id == id => Some((status, content, meta, correlation_id)),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no result for {id}"))
    }

    /// The gate's record of call `id`'s tool-call node.
    fn gate(&self, session: &str, id: &str) -> theseus_protocol::GateRecord {
        self.core
            .store
            .session_nodes(session)
            .unwrap()
            .into_iter()
            .find_map(|(_, n)| match n.body {
                Body::ToolCall {
                    tool_use_id, gate, ..
                } if tool_use_id == id => gate.map(|g| *g),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no call {id}"))
    }

    /// Request `i`'s messages, as JSON text.
    fn request(&self, i: usize) -> String {
        let r = self.fake.requests();
        serde_json::to_string(&r[i].messages).unwrap()
    }
}

/// `background: true` answers at once, before the job's end (the job still
/// runs when the turn has ended, by its action, not a stopwatch, which a
/// loaded machine stretches): its short id, its command and directory, and
/// the file its output goes to; the placeholder's meta names the id, and a
/// session's ids are read again from it (a restart's map).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_background_run_answers_at_once_with_its_id_and_output_path() {
    let job = json!({"argv": ["sh", "-c", "sleep 30; echo late"], "background": true});
    let r = rig(
        vec![
            Scripted::tools("Starting it.", &[("t1", "proc_run", job)]),
            Scripted::text("Started."),
        ],
        10,
        |_| {},
    );
    let res = r.turn(None, "start it").await;
    let (status, text, meta, corr) = r.result(&res.session_id, "t1");
    let corr = corr.expect("a job's call has its correlation id");
    let a = r.core.kernel.action(&corr).unwrap().unwrap();
    assert_eq!(
        a.state,
        theseus_kernel::ActionState::Dispatched,
        "it runs on"
    );
    assert_eq!(status, ResultStatus::Background, "{text}");
    assert!(
        text.starts_with("Started job j1 (sh -c sleep 30; echo late) in "),
        "{text}"
    );
    let path = r.core.spool.result_path(&corr);
    assert!(
        text.contains(&format!("; its output goes to {}.\n", path.display())),
        "{text}"
    );
    assert!(
        text.contains(
            "job_read j1 shows its latest output, job_wait j1 waits for it, job_stop j1 stops \
             it. When it ends, you get a notice between your calls."
        ),
        "{text}"
    );
    assert_eq!(meta["job"], "j1");
    let fresh = crate::toolrun::handles::Handles::default();
    let e = fresh
        .resolve(&r.core.store, &res.session_id, "j1")
        .expect("read from the placeholder");
    assert_eq!(e.corr, corr);
}

/// Past its window, a blocking run's answer gains its handle.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_run_past_its_window_answers_with_its_handle() {
    let r = rig(
        vec![
            Scripted::tools(
                "Running it.",
                &[("t1", "proc_run", json!({"argv": ["sleep", "2"]}))],
            ),
            Scripted::text("It goes on."),
        ],
        1,
        |_| {},
    );
    let res = r.turn(None, "run it").await;
    let (status, text, meta, _) = r.result(&res.session_id, "t1");
    assert_eq!(status, ResultStatus::Background, "{text}");
    assert_eq!(
        text,
        "Still running as job j1 after 1 s (timeout 600 s). job_read, job_wait or job_stop it; \
         its result also arrives by itself when it ends."
    );
    assert_eq!(meta["job"], "j1");
}

/// `job.read` shows a running job's newest lines, how many it has printed
/// and how long it has run; with no job it lists the session's running
/// jobs. The job runs 30 s, past a loaded machine's turn.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn job_read_shows_the_newest_lines_and_lists_running_jobs() {
    let job = json!({"argv": ["sh", "-c", "echo one; echo two; echo three; sleep 30"], "background": true});
    let r = rig(
        vec![
            Scripted::tools("Starting it.", &[("t1", "proc_run", job)]),
            Scripted::tools(
                "A moment.",
                &[("t2", "proc_run", json!({"argv": ["sleep", "0.5"]}))],
            ),
            Scripted::tools(
                "Reading it.",
                &[
                    ("t3", "job_read", json!({"job": "j1", "lines": 2})),
                    ("t4", "job_read", json!({})),
                ],
            ),
            Scripted::text("Read."),
        ],
        10,
        |_| {},
    );
    let res = r.turn(None, "start it and look").await;
    let sid = &res.session_id;
    let (status, text, meta, _) = r.result(sid, "t3");
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(
        text.starts_with("Job j1 (sh -c echo one; echo two; echo three; sleep 30), running "),
        "{text}"
    );
    assert!(
        text.ends_with("; 3 lines so far; the last 2 lines:\ntwo\nthree"),
        "{text}"
    );
    assert_eq!(
        (meta["lines"].as_u64(), meta["shown"].as_u64()),
        (Some(3), Some(2))
    );
    let (_, list, _, _) = r.result(sid, "t4");
    assert!(
        list.starts_with(
            "This session's 1 job running:\nj1 (sh -c echo one; echo two; echo three; sleep 30) in "
        ),
        "{list}"
    );
}

/// `job.wait` gives a job's result as a blocking `proc.run` would, once it
/// ends inside the wait, and says it still runs when the wait ends first.
/// The late result the job's end still writes then says only that the model
/// has it, and the turn takes no other for it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn job_wait_gives_the_result_inside_its_window_and_says_still_running_past_it() {
    let quick =
        json!({"argv": ["sh", "-c", "sleep 0.5; printf 'fini%s\\n' shed"], "background": true});
    let slow = json!({"argv": ["sleep", "30"], "background": true});
    let r = rig(
        vec![
            Scripted::tools(
                "Starting them.",
                &[("t1", "proc_run", quick), ("t2", "proc_run", slow)],
            ),
            Scripted::tools(
                "Waiting.",
                &[
                    ("t3", "job_wait", json!({"job": "j1", "timeout_secs": 8})),
                    ("t4", "job_wait", json!({"job": "j2", "timeout_secs": 1})),
                ],
            ),
            Scripted::text("Done waiting."),
        ],
        10,
        |_| {},
    );
    let t0 = Instant::now();
    let res = r.turn(None, "start and wait").await;
    let sid = &res.session_id;
    let (status, text, meta, _) = r.result(sid, "t3");
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert_eq!(text, "[exit code 0]\nfinished\n");
    assert!(meta["delivers"].is_string(), "{meta}");
    let (status, text, _, _) = r.result(sid, "t4");
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(
        text.starts_with("Still running after 1 s. job_read j2 shows"),
        "{text}"
    );
    assert!(t0.elapsed() >= Duration::from_secs(1), "the wait waited");
    let next = r.request(2);
    assert_eq!(next.matches("finished").count(), 1, "given once: {next}");
    assert!(
        next.contains("Its result was given by your job_wait call already."),
        "{next}"
    );
}

/// Where no notice reaches the drain (a daemon on its heartbeat alone, as one
/// whose notify socket's path is too long), `job.wait` still sees the job's
/// end at its next look: it takes the spooled completion itself, as a turn's
/// look at its own job does, and the late result that follows is the short
/// one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn job_wait_takes_the_end_itself_when_no_drain_comes() {
    let quick =
        json!({"argv": ["sh", "-c", "sleep 0.5; printf 'by%s\\n' itself"], "background": true});
    let r = rig(
        vec![
            Scripted::tools("Starting it.", &[("t1", "proc_run", quick)]),
            Scripted::tools(
                "Waiting.",
                &[("t2", "job_wait", json!({"job": "j1", "timeout_secs": 8}))],
            ),
            Scripted::text("Done waiting."),
        ],
        10,
        |_| {},
    );
    r.drain.abort();
    let res = r.turn(None, "start and wait").await;
    let (status, text, meta, _) = r.result(&res.session_id, "t2");
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert_eq!(text, "[exit code 0]\nbyitself\n");
    assert!(meta["delivers"].is_string(), "{meta}");
    let next = r.request(2);
    assert!(
        next.contains("Its result was given by your job_wait call already."),
        "{next}"
    );
}

/// A job that ends while its turn still runs reaches the model at the
/// turn's next loop, before its next request, as the late result a later
/// turn would have read.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_job_that_ends_mid_turn_reaches_the_next_request() {
    let quick =
        json!({"argv": ["sh", "-c", "sleep 0.3; printf 'mid%s\\n' turn"], "background": true});
    let r = rig(
        vec![
            Scripted::tools("Starting it.", &[("t1", "proc_run", quick)]),
            Scripted::tools(
                "Working meanwhile.",
                &[("t2", "proc_run", json!({"argv": ["sleep", "1.5"]}))],
            ),
            Scripted::text("Both done."),
        ],
        10,
        |_| {},
    );
    let res = r.turn(None, "start it and keep working").await;
    assert_eq!(res.loops, 3);
    let next = r.request(2);
    assert!(
        next.contains(
            "[Background result for your earlier proc.run call (t1): status ok, exit code 0]"
        ),
        "the third request: {next}"
    );
    assert!(next.contains("midturn"), "{next}");
    assert!(!r.request(1).contains("midturn"), "not before it ended");
}

/// A job is its session's: another session naming it by its long id is
/// refused by name, and a short id it never gave names nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn another_sessions_job_is_refused_by_name() {
    let job = json!({"argv": ["sleep", "1"], "background": true});
    let r = rig(
        vec![
            Scripted::tools("Starting it.", &[("t1", "proc_run", job)]),
            Scripted::text("Started."),
        ],
        10,
        |_| {},
    );
    let a = r.turn(None, "start it").await;
    let (_, _, _, corr) = r.result(&a.session_id, "t1");
    let corr = corr.unwrap();
    r.fake.script.lock().unwrap().extend([
        Scripted::tools(
            "Looking.",
            &[
                ("u1", "job_read", json!({"job": corr})),
                ("u2", "job_wait", json!({"job": "j1"})),
            ],
        ),
        Scripted::text("Not mine."),
    ]);
    let b = r.turn(None, "look at it").await;
    let (status, text, _, _) = r.result(&b.session_id, "u1");
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert_eq!(
        text,
        format!(
            "Invalid input: job {corr} is another session's: only the session that started a \
             job reads, waits for, or stops it"
        )
    );
    let (status, text, _, _) = r.result(&b.session_id, "u2");
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(
        text.starts_with("Invalid input: this session has no job j1"),
        "{text}"
    );
}

/// The gate's classes: `job.read` is a read, open by the template;
/// `job.stop` is a run of the job's own program, its argv and directory,
/// and here waits for approval, as `[policy.tools]` says (so nothing stops
/// this process's in-process job).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_gate_reads_job_read_as_a_read_and_job_stop_as_the_jobs_run() {
    let job = json!({"argv": ["sleep", "1"], "background": true});
    let r = rig(
        vec![
            Scripted::tools("Starting it.", &[("t1", "proc_run", job)]),
            Scripted::tools(
                "Stopping it.",
                &[
                    ("t2", "job_read", json!({"job": "j1"})),
                    ("t3", "job_stop", json!({"job": "j1"})),
                ],
            ),
            Scripted::text("Asked."),
        ],
        10,
        |c| {
            c.policy.tools.insert("job.stop".into(), Posture::Approve);
        },
    );
    let res = r.turn(None, "start it and stop it").await;
    let sid = &res.session_id;
    let read = r.gate(sid, "t2");
    assert_eq!(read.result.gate, "allow");
    assert_eq!(read.plan.as_ref().unwrap().argv, None);
    let stop = r.gate(sid, "t3");
    assert_eq!(stop.result.gate, "needs_confirm");
    let plan = stop.plan.expect("planned");
    assert_eq!(
        plan.argv.as_deref(),
        Some(&["sleep".to_string(), "1".to_string()][..])
    );
    assert!(
        plan.summary.starts_with("stop job j1 (`sleep 1`) in "),
        "{}",
        plan.summary
    );
    assert_eq!(plan.resources[0].access, theseus_tools::Access::Exec);
}
