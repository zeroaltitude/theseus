//! A job's end as an event (Tier 7.1), through the whole core: the turn
//! waits on its job's wake instead of looking every 50 ms, takes the job's
//! completion with its result's node in one frame, and the drain leaves the
//! completion of a job a turn waits on to that turn. Jobs run in process
//! (`InlineLauncher`), which wakes the turn as the drain does for a real
//! wrapper; the daemon's own test is `theseusd/tests/job_latency.rs`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use theseus_kernel::{Completion, Outcome};
use theseus_protocol::{SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::ledger::LedgerRow;
use crate::node::{Body, ResultStatus};
use crate::policy::Posture;
use crate::provider::{FakeProvider, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

struct Rig {
    core: Arc<Core>,
    _dir: tempfile::TempDir,
}

/// A deployment whose `proc.run` runs with a notice, in process.
fn rig(script: Vec<Scripted>) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().join("state").to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.tools.proc_sync_secs = 10;
    cfg.policy.enforcement = Posture::Notify;
    cfg.policy.tools.remove("proc.run");
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake, store)).unwrap();
    Rig { core, _dir: dir }
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

    fn frames(&self) -> u64 {
        self.core.store.stats().unwrap().frames_appended
    }

    /// The ledger's kinds after position `after`.
    fn rows_after(&self, after: u64) -> Vec<String> {
        self.core
            .store
            .ledger_tail::<LedgerRow>(1000)
            .unwrap()
            .into_iter()
            .filter(|(p, _)| *p > after)
            .map(|(_, row)| row.kind)
            .collect()
    }
}

/// A loop whose one call is a job costs four frames, as an in-process call's
/// does: the provider call's two, the job's plan and dispatch, and the job's
/// completion with its result, which the turn takes itself (five before: the
/// completion, then the result on a frame of its own). The turn looks at its
/// job at the launch and when the job's end wakes it, not every 50 ms, and
/// nothing writes `completion.duplicate`. The turn's own count on its trace
/// (theseus-wz4y) is the store's: no frame of the job's is written by another.
#[tokio::test]
async fn a_loop_with_one_job_costs_four_frames_and_two_looks() {
    let job = json!({"argv": ["sleep", "0.4"]});
    let r = rig(vec![
        Scripted::text("first"),
        Scripted::text("hello"),
        Scripted::tools("Running it.", &[("t1", "proc_run", job)]),
        Scripted::text("It ran."),
    ]);
    let sid = r.turn(None, "warm up").await.session_id;
    let f0 = r.frames();
    r.turn(Some(&sid), "hi").await;
    let plain = r.frames() - f0;
    let (f1, from) = (r.frames(), r.core.store.last_position());
    let looks = r.core.tools.job_waits.looks();
    let res = r.turn(Some(&sid), "run it").await;
    assert_eq!((res.loops, res.tool_calls), (2, 1));
    let job_loop = r.frames() - f1 - plain;
    assert!(job_loop <= 4, "the job's loop wrote {job_loop} frames");
    let traced = res.trace.as_ref().expect("a trace").attrs["frames"].as_u64();
    assert_eq!(traced, Some(r.frames() - f1), "the turn's trace counts");
    let looks = r.core.tools.job_waits.looks() - looks;
    assert!(
        (1..=3).contains(&looks),
        "the turn looked at its 0.4 s job {looks} times"
    );
    let rows = r.rows_after(from);
    assert!(rows.iter().any(|k| k == "action.succeeded"), "{rows:?}");
    assert!(
        !rows.iter().any(|k| k == "completion.duplicate"),
        "{rows:?}"
    );
    let result = r
        .core
        .store
        .session_nodes(&sid)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match n.body {
            Body::ToolResult {
                status, content, ..
            } => Some((status, content)),
            _ => None,
        })
        .expect("the job's result");
    assert_eq!(result.0, ResultStatus::Ok, "{}", result.1);
}

/// The drain leaves a job's spooled completion to the turn waiting on it,
/// and wakes that turn; with no turn waiting, it takes it as before (here a
/// stray, so quarantined).
#[tokio::test]
async fn the_drain_leaves_a_waited_jobs_completion_to_its_turn() {
    let r = rig(vec![]);
    let now = theseus_protocol::now_unix_ms();
    r.core
        .spool
        .write(&Completion {
            correlation_id: "act_waited".into(),
            outcome: Outcome::Succeeded,
            result_ref: None,
            external_op_id: None,
            started_at_ms: now,
            finished_at_ms: now,
            producer: "test".into(),
            signature: None,
            cost_micros: None,
            detail: None,
        })
        .unwrap();
    let waiting = r.core.tools.job_waits.wait("act_waited");
    assert_eq!(r.core.drain_spool(), 0);
    assert!(
        r.core.spool.has_completion("act_waited"),
        "left to the turn"
    );
    let t0 = Instant::now();
    waiting.woken(Duration::from_secs(5)).await;
    assert!(
        t0.elapsed() < Duration::from_secs(1),
        "the drain woke the turn"
    );
    drop(waiting);
    assert_eq!(r.core.drain_spool(), 1);
    assert!(!r.core.spool.has_completion("act_waited"));
    assert_eq!(r.core.kernel.quarantined().unwrap().len(), 1);
}
