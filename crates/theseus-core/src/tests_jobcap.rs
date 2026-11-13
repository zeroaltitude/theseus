//! The job cap, through the whole core (theseus-v6bd): with `[tools]
//! max_jobs` set, a `proc.run` that would pass it is **refused with a named
//! reason, never queued** (the owner, 2026-11-12), and with the key off
//! (`jobcap::OFF`) the same two calls both run, which is every daemon's
//! behaviour before the key existed.
//!
//! The count the cap reads is the spool's pid files
//! (`toolrun::jobcap`'s module doc says why, and what that count does not
//! see). A pid file is written by the real spawn (`job::spawn_detached`), and
//! these tests run jobs in process (`InlineLauncher`), which does not spawn
//! one — so a job in flight is stood in for by writing its pid file, exactly
//! as theseus-kernel's `tests_gone.rs` stands in for a wrapper. What the
//! tests drive is the production decision in `job::launch`, over the
//! production count. `theseusd/tests/` is where real wrappers run.
//!
//! What is NOT proved here, and is named rather than left implied: the row's
//! other half, two hundred jobs ending at once with the daemon answering
//! throughout. That is a daemon test with real wrappers, not a core one.

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
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
    drain: tokio::task::JoinHandle<()>,
    _dir: tempfile::TempDir,
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.drain.abort();
    }
}

/// A deployment whose `proc.run` runs with a notice, in process, with
/// `max_jobs` as given and a drain every 20 ms, as the daemon's harness
/// drains on each wrapper's word.
fn rig(script: Vec<Scripted>, max_jobs: usize) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().join("state").to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.tools.proc_sync_secs = 10;
    cfg.tools.max_jobs = max_jobs;
    cfg.policy.enforcement = Posture::Notify;
    cfg.policy.tools.remove("proc.run");
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake, store)).unwrap();
    let c = core.clone();
    let drain = tokio::spawn(async move {
        loop {
            c.drain_spool();
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });
    Rig {
        core,
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
                surface: None,
                session: rec,
                input: Some(input.into()),
                target,
                sink,
                author: "test".into(),
                recompile: None,
                attachments: vec![],
                arrived: None,
                reply_to: None,
                answer_at: None,
            })
            .await
            .unwrap()
    }

    /// Call `id`'s result in `session`: its status and its text.
    fn result(&self, session: &str, id: &str) -> (ResultStatus, String) {
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
                    late: false,
                    ..
                } if tool_use_id == id => Some((status, content)),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no result for {id}"))
    }
}

/// One `proc.run`, which the cap either refuses or runs.
fn script() -> Vec<Scripted> {
    let run = json!({"argv": ["sh", "-c", "echo second"]});
    vec![
        Scripted::tools("Running it.", &[("t1", "proc_run", run)]),
        Scripted::text("Done."),
    ]
}

/// A job in flight: its pid file in the spool, which is what
/// `Spool::running` counts and what a real wrapper writes as it starts.
fn a_job_is_running(r: &Rig, id: &str) {
    r.core.spool.write_pid(id, std::process::id()).unwrap();
    assert!(
        r.core.spool.running().iter().any(|(j, _)| j == id),
        "the stand-in job is not counted as running"
    );
}

/// With `[tools] max_jobs = 1` and one job in flight, the next `proc.run`
/// is refused, and the refusal names the cap, the count and the key, in the
/// words `jobcap::refusal` makes once. **It is not queued**: nothing is
/// waiting to start it, and the words say so, which is the owner's own
/// decision for this row.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_job_over_the_cap_is_refused_by_name_and_never_queued() {
    let r = rig(script(), 1);
    a_job_is_running(&r, "job-already-running");
    let res = r.turn(None, "run it").await;
    let (status, text) = r.result(&res.session_id, "t1");
    assert_eq!(status, ResultStatus::Error, "{text}");
    for want in [
        "1 jobs are already running",
        "`[tools] max_jobs` is 1",
        "refused rather than queued",
        "did not run",
    ] {
        assert!(text.contains(want), "{want} missing from {text}");
    }
    assert!(
        !text.contains("try again later"),
        "a refusal must not read as a queue: {text}"
    );
    // And nothing was started: the command never ran, so its output is in
    // no result. A refusal that quietly ran the job would pass the words
    // above and fail here.
    assert!(!text.contains("second"), "the command ran: {text}");
}

/// The control, which is what makes the test above a measurement of the cap
/// and not of the rig: with the identical job in flight and `[tools]
/// max_jobs = 0` (`jobcap::OFF`, every daemon's behaviour before this key)
/// the same call runs, and its result is its output.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_the_cap_off_the_same_call_runs() {
    let r = rig(script(), crate::toolrun::jobcap::OFF);
    a_job_is_running(&r, "job-already-running");
    let res = r.turn(None, "run it").await;
    let (status, text) = r.result(&res.session_id, "t1");
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(text.contains("second"), "{text}");
    assert!(!text.contains("max_jobs"), "{text}");
}
