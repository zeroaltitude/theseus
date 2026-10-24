//! The progress check and the turn's clock (theseus-2uy56, theseus-yct32),
//! through whole cores on the fake model with the judge off: a turn whose
//! commands keep failing past half its deadline is told to replan, past 0.85
//! of it is told to stop and report and ends after one more call; a turn
//! whose commands pass past the same time is told nothing; the time line is
//! appended history, so the cached prefix stays byte for byte between loops.
//!
//! The deadline is set the way `turn.submit` sets it, with its start in the
//! past, so the turn begins at the share the test names: no clock waits.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use theseus_protocol::{SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::ledger::LedgerRow;
use crate::policy::Posture;
use crate::provider::{FakeProvider, ProviderRequest, Scripted};
use crate::rpc::{Core, Parts};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::clock_step::set_deadline;
use crate::turn::progress_step::PROGRESS_STOP;
use crate::turn::TurnRequest;
use crate::Config;

struct Rig {
    core: Arc<Core>,
    fake: Arc<FakeProvider>,
    _dir: tempfile::TempDir,
}

fn rig(script: impl FnOnce(&Path) -> Vec<Scripted>) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root: PathBuf = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().join("state").to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = Posture::Notify;
    cfg.policy.tools.remove("proc.run");
    cfg.judge.enabled = false;
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script(&root)));
    let core = Core::build(Parts::for_tests(cfg, fake.clone(), store)).unwrap();
    Rig {
        core,
        fake,
        _dir: dir,
    }
}

impl Rig {
    /// One turn in a fresh session whose deadline is `secs` long and began
    /// `ago` before the turn: `None` for no deadline.
    async fn turn(&self, deadline: Option<(u64, u64)>) -> (String, TurnSubmitResult) {
        let rec = SessionRecord::new(SessionKind::Conversation, None);
        self.core.store.put_session(&rec.session_id, &rec).unwrap();
        let sid = rec.session_id.clone();
        // The start `turn.submit` gives both the deadline and the request.
        let start = deadline.map(|(secs, ago)| {
            let start = Instant::now() - Duration::from_secs(ago);
            set_deadline(&sid, start, Some(secs));
            start
        });
        let res = self.run(rec, start).await;
        (sid, res)
    }

    /// One turn of `rec`'s session whose request arrived at `arrived`.
    async fn run(&self, rec: SessionRecord, arrived: Option<Instant>) -> TurnSubmitResult {
        let sid = rec.session_id.clone();
        let (live, _) = self.core.live_profile();
        let target = self
            .core
            .runner
            .resolve_target(&live, None, None, None)
            .unwrap();
        self.core
            .runner
            .run(TurnRequest {
                prompt: None,
                surface: None,
                session: rec,
                input: Some("Build the tide tables and make the check pass.".into()),
                target,
                sink: EventSink::new(self.core.bus.clone(), &sid, None),
                author: "test".into(),
                recompile: None,
                attachments: vec![],
                arrived,
                reply_to: None,
                answer_at: None,
            })
            .await
            .unwrap()
    }

    fn rows(&self, sid: &str) -> Vec<serde_json::Value> {
        let rows: Vec<(u64, LedgerRow)> = self.core.store.ledger_tail(5000).unwrap();
        rows.into_iter()
            .filter(|(_, r)| r.kind == "turn.progress" && r.session_id.as_deref() == Some(sid))
            .map(|(_, r)| r.data)
            .collect()
    }

    fn requests(&self) -> Vec<ProviderRequest> {
        self.fake.requests()
    }
}

/// A request's messages as text.
fn messages(r: &ProviderRequest) -> String {
    serde_json::to_string(&r.messages).unwrap()
}

/// A command in `root` that exits with `code`, as loop `id`'s one call.
fn command(root: &Path, id: &str, code: u8) -> Scripted {
    let input =
        json!({"argv": ["sh", "-c", format!("exit {code}")], "cwd": root.display().to_string()});
    Scripted::tools("Running the check.", &[(id, "proc_run", input)])
}

const STEER: &str = "Replan or narrow";
const STOP: &str = "Stop here: in your next answer make no tool calls";

/// Failing loops past half the deadline: the steer once, in the request
/// after the third failing loop, with its row; then the work goes on.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failing_turn_past_half_its_deadline_is_told_to_replan_once() {
    let r = rig(|root| {
        vec![
            command(root, "c1", 1),
            command(root, "c2", 1),
            command(root, "c3", 1),
            command(root, "c4", 1),
            command(root, "c5", 1),
            Scripted::text("It still fails; here is what is left."),
        ]
    });
    // 600 s, 360 used: 60%.
    let (sid, res) = r.turn(Some((600, 360))).await;
    let reqs = r.requests();
    assert_eq!(reqs.len(), 6, "the steer stops nothing");
    assert_eq!(res.loops, 6);
    for (i, req) in reqs.iter().enumerate() {
        let steered = messages(req).contains(STEER);
        assert_eq!(
            steered,
            i >= 3,
            "request {i}: the steer follows the third failing loop"
        );
    }
    assert_eq!(messages(&reqs[5]).matches(STEER).count(), 1, "once a turn");
    assert!(
        messages(&reqs[3]).contains("no check has passed in its last 3 loops (3 failed)"),
        "{}",
        messages(&reqs[3])
    );
    let rows = r.rows(&sid);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["decision"], "steer");
    assert_eq!(rows[0]["loop"], 2);
    assert_eq!(rows[0]["since_pass"], 3);
    assert_eq!(rows[0]["failing"], 3);
    assert!(rows[0]["time_share"].as_f64().unwrap() >= 0.5);
    assert_eq!(rows[0]["deadline_ms"], 600_000);
}

/// Failing loops past 0.85 of the deadline: the stop, and the next request is
/// the turn's last, whatever it answers.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failing_turn_near_its_deadline_is_told_to_stop_and_its_next_call_is_its_last() {
    let r = rig(|root| {
        vec![
            command(root, "c1", 1),
            command(root, "c2", 1),
            command(root, "c3", 1),
            // It calls a tool anyway: the call runs, and the turn ends.
            command(root, "c4", 1),
            Scripted::text("A fifth answer no turn asks for."),
        ]
    });
    // 600 s, 540 used: 90%.
    let (sid, res) = r.turn(Some((600, 540))).await;
    let reqs = r.requests();
    assert_eq!(reqs.len(), 4, "the stop's request is the last");
    assert_eq!((res.loops, res.stop_reason.as_str()), (4, PROGRESS_STOP));
    assert!(messages(&reqs[3]).contains(STOP), "{}", messages(&reqs[3]));
    assert!(!messages(&reqs[3]).contains(STEER), "a stop, not a steer");
    let rows = r.rows(&sid);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["decision"], "stop");
}

/// A stop answered with the report ends the turn there, and the stop nudge
/// and the end note never add a loop to it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stopped_turn_that_reports_ends_with_its_report() {
    let r = rig(|root| {
        vec![
            command(root, "c1", 1),
            command(root, "c2", 1),
            command(root, "c3", 1),
            Scripted::text("Done: the parser. Not done: the check, which still fails."),
            Scripted::text("An answer no turn asks for."),
        ]
    });
    let (_, res) = r.turn(Some((600, 540))).await;
    assert_eq!(r.requests().len(), 4);
    assert_eq!((res.loops, res.stop_reason.as_str()), (4, PROGRESS_STOP));
    assert!(res.output.contains("Not done: the check"), "{}", res.output);
}

/// Commands that pass, past the same time: no note and no row, and the turn
/// runs every loop it asks for.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_whose_checks_pass_is_told_nothing_at_the_same_time() {
    let r = rig(|root| {
        vec![
            command(root, "c1", 0),
            command(root, "c2", 0),
            command(root, "c3", 0),
            command(root, "c4", 0),
            command(root, "c5", 0),
            Scripted::text("Built, and the check passes."),
        ]
    });
    let (sid, res) = r.turn(Some((600, 540))).await;
    let reqs = r.requests();
    assert_eq!((reqs.len(), res.loops), (6, 6));
    assert_eq!(res.stop_reason, "no_tool_calls");
    for req in &reqs {
        let m = messages(req);
        assert!(!m.contains(STEER) && !m.contains(STOP), "{m}");
    }
    assert!(r.rows(&sid).is_empty());
}

/// The time line: in the first request, after the input, and in the
/// messages, never the system or the tools, which stay byte for byte from
/// loop to loop; a turn with no deadline has none.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_time_line_is_history_and_the_cached_prefix_holds() {
    let r = rig(|root| {
        vec![
            command(root, "c1", 0),
            Scripted::text("Done."),
            command(root, "c2", 0),
            Scripted::text("Done."),
        ]
    });
    let (_, _) = r.turn(Some((900, 0))).await;
    let reqs = r.requests();
    assert_eq!(reqs.len(), 2);
    let line = "[The harness: 14 minutes left of this turn's 15.]";
    assert!(messages(&reqs[0]).contains(line), "{}", messages(&reqs[0]));
    assert_eq!(
        messages(&reqs[1])
            .matches("minutes left of this turn's")
            .count(),
        1,
        "one line a minute, never one a loop"
    );
    for req in &reqs {
        let fixed = serde_json::to_string(&(&req.system, &req.tools)).unwrap();
        assert!(
            !fixed.contains("minutes left"),
            "the line is outside the prefix"
        );
    }
    assert_eq!(
        serde_json::to_string(&reqs[0].system).unwrap(),
        serde_json::to_string(&reqs[1].system).unwrap()
    );
    assert_eq!(
        serde_json::to_string(&reqs[0].tools).unwrap(),
        serde_json::to_string(&reqs[1].tools).unwrap()
    );
    // The first request's messages are the second's first messages, byte
    // for byte: only appended to.
    let (a, b) = (&reqs[0].messages, &reqs[1].messages);
    assert!(a.len() < b.len());
    assert_eq!(
        serde_json::to_string(&a[..a.len() - 1]).unwrap(),
        serde_json::to_string(&b[..a.len() - 1]).unwrap()
    );
    // The CPU fact sits in the session's system block, the same every loop.
    let system = serde_json::to_string(&reqs[0].system).unwrap();
    assert!(system.contains("CPUs available to commands: "), "{system}");
    // No deadline: no line.
    let (_, _) = r.turn(None).await;
    let reqs = r.requests();
    assert_eq!(reqs.len(), 4);
    assert!(!messages(&reqs[2]).contains("left of this turn"));
    assert!(!messages(&reqs[3]).contains("left of this turn"));
}

/// A command that passes beside a read that fails, every loop: the check is
/// the command's, so the turn is a passing one and is told nothing
/// (theseus-rmjqr).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_passing_command_beside_a_failed_read_is_a_pass() {
    let both = |root: &Path, n: u32| {
        let run = json!({"argv": ["sh", "-c", "exit 0"], "cwd": root.display().to_string()});
        let read = json!({"path": root.join("missing.txt").display().to_string()});
        Scripted::tools(
            "Checking, and reading.",
            &[
                (&format!("c{n}"), "proc_run", run),
                (&format!("r{n}"), "fs_read", read),
            ],
        )
    };
    let r = rig(|root| {
        let mut script: Vec<Scripted> = (1..=4).map(|n| both(root, n)).collect();
        script.push(Scripted::text("Done."));
        script
    });
    // 600 s, 360 used: 60%.
    let (sid, res) = r.turn(Some((600, 360))).await;
    assert_eq!(res.loops, 5);
    for req in r.requests() {
        let m = messages(&req);
        assert!(!m.contains(STEER) && !m.contains(STOP), "{m}");
        assert!(!m.contains("harness:progress"), "{m}");
    }
    assert!(r.rows(&sid).is_empty());
}

/// A deadline is the turn's whose submit set it: a later turn of the session,
/// which arrived on its own (a nudge, a wake), has none.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_later_turn_of_the_session_does_not_inherit_its_deadline() {
    let r = rig(|_| vec![Scripted::text("First."), Scripted::text("Second.")]);
    let (sid, _) = r.turn(Some((900, 0))).await;
    assert!(messages(&r.requests()[0]).contains("minutes left of this turn's 15"));
    let rec: SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
    r.run(rec, Some(Instant::now())).await;
    let reqs = r.requests();
    assert_eq!(reqs.len(), 2);
    // The session's history holds the first turn's line, and no second one.
    assert_eq!(
        messages(&reqs[1])
            .matches("minutes left of this turn's")
            .count(),
        1
    );
}
