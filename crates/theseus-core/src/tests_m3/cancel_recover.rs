//! A cancel the daemon died in the middle of (issue 6; theseus-fvxh, theseus-1og8, theseus-vn4d), through
//! the whole core. The rule they are built to: the state machine always leaves a task in a recoverable
//! state; a call or run that supports it may fail utterly, for any reason; resuming it at worst repeats
//! work. A child of `tests_m3` for its rig and helpers, split out under that file's ceiling.
//!
//! A kill at each of three points:
//! - **after the cancel's frame and before its sweep** (fvxh): the calls the cancel stopped are left
//!   without a result in the transcript;
//! - **after a stopped job's wrapper wrote its verdict and before the daemon read it** (1og8): a restart
//!   marked the call unknown without reading the verdict;
//! - **a job reached before it had started** (vn4d): its action kept `unsupported` though its launch's
//!   own stop verified it.

use theseus_kernel::{ActionState, CancelState, ExecState, Verdict, VerifiedBy};

use super::*;

/// A job started in the background (its action stays dispatched, no pid in the spool), and the
/// session it runs in. The turn is over; the model said "Started."
async fn background_job() -> (Rig, TurnSubmitResult, theseus_kernel::Action) {
    let r = rig_parts(
        vec![
            Scripted::tools(
                "",
                &[(
                    "t1",
                    "proc_run",
                    json!({"argv": ["bash", "-c", "echo printed; sleep 30"]}),
                )],
            ),
            Scripted::text("Started."),
        ],
        |cfg| {
            cfg.policy.tools.insert("proc.run".into(), Posture::Open);
            cfg.tools.proc_sync_secs = 1;
        },
        |p| p.launcher = Arc::new(PrintedOnly("printed\n")),
    );
    let first = turn(&r.core, None, "start it").await;
    assert_eq!(first.output, "Started.");
    let job = the_job(&r.core);
    assert_eq!(job.state, ActionState::Dispatched);
    (r, first, job)
}

/// The cancel's own frame and nothing after it: what a daemon that died between the cancel and its sweep
/// wrote. The execution is cancelled, the job's action is told to stop (`requested`), and nothing
/// answered the transcript.
fn cancel_then_die(core: &Core, execution_id: &str) {
    let c = core
        .kernel
        .cancel_execution_with(execution_id, "test", |_| Ok(vec![]))
        .unwrap();
    assert_eq!(c.to_kill.len(), 1, "the job is told to stop");
}

/// A second core over the same store and state directory: the daemon after a restart, whose start has
/// settled what its predecessor left (`Core::build` runs the kernel's startup).
fn restart(r: &Rig) -> Arc<Core> {
    let parts =
        crate::rpc::Parts::for_tests((*r.core.cfg).clone(), r.fake.clone(), r.core.store.clone());
    Core::build(parts).unwrap()
}

/// How many tool calls of the session no result answers.
fn unanswered_calls(core: &Core, sid: &str) -> usize {
    let nodes = core.store.session_nodes(sid).unwrap();
    let answered: std::collections::HashSet<String> = nodes
        .iter()
        .filter_map(|(_, n)| match &n.body {
            Body::ToolResult {
                tool_use_id,
                status,
                late: false,
                ..
            } if *status != ResultStatus::Background => Some(tool_use_id.clone()),
            Body::ToolResult {
                tool_use_id,
                late: true,
                ..
            } => Some(tool_use_id.clone()),
            _ => None,
        })
        .collect();
    nodes
        .iter()
        .filter(|(_, n)| match &n.body {
            Body::ToolCall { tool_use_id, .. } => !answered.contains(tool_use_id),
            _ => false,
        })
        .count()
}

fn late_results(core: &Core, sid: &str) -> Vec<String> {
    core.store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .filter_map(|(_, n)| match n.body {
            Body::ToolResult {
                late: true,
                content,
                ..
            } => Some(content),
            _ => None,
        })
        .collect()
}

/// KEY (fvxh): killed after the cancel's frame and before its sweep, the calls it stopped are answered in
/// the transcript once the restarted daemon serves. Before, nothing did: the execution was cancelled,
/// so no turn would, and the session's next request carried a call with no result.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_kill_between_a_cancel_and_its_sweep_leaves_the_calls_answered_after_the_restart() {
    let (r, first, job) = background_job().await;
    cancel_then_die(&r.core, &job.execution_id);
    assert!(
        late_results(&r.core, &first.session_id).is_empty(),
        "the sweep never ran"
    );
    let again = restart(&r);
    // The restart's start settled the call its predecessor was stopping, one way or the other.
    let a = again.kernel.action(&job.correlation_id).unwrap().unwrap();
    assert!(
        a.state != ActionState::Dispatched,
        "the restart leaves the stopped call settled: {a:?}"
    );
    again.answer_earlier_cancels();
    let late = late_results(&again, &first.session_id);
    assert_eq!(late.len(), 1, "one late result for the one job: {late:?}");
    assert_eq!(unanswered_calls(&again, &first.session_id), 0);
    // Said once: a second look writes nothing more.
    again.answer_earlier_cancels();
    assert_eq!(late_results(&again, &first.session_id).len(), 1);
    let e = again.kernel.execution(&job.execution_id).unwrap().unwrap();
    assert_eq!(e.state, ExecState::Cancelled);
}

/// KEY (1og8): killed after the stopped job's wrapper wrote its verdict and before the daemon read it, the
/// restart settles the call from that verdict (verified, as the wrapper said), not as unknown; the verdict
/// is taken; and a verdict no action waits for is swept.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_restart_mid_cancel_reads_the_verdict_the_stopped_wrapper_left() {
    let (r, first, job) = background_job().await;
    cancel_then_die(&r.core, &job.execution_id);
    let spool = &r.core.spool;
    let said = Verdict::verified_as(VerifiedBy::Tree, Some(2));
    spool.write_stop(&job.correlation_id, &said).unwrap();
    // An old verdict, of a call that settled long ago: nothing will read it.
    spool.write_stop("act_long_gone", &said).unwrap();
    let again = restart(&r);
    let a = again.kernel.action(&job.correlation_id).unwrap().unwrap();
    assert_eq!(
        (a.state, a.cancel),
        (
            ActionState::Cancelled,
            Some(CancelState::TerminationVerified)
        ),
        "settled by the verdict, not marked unknown: {a:?}"
    );
    assert_eq!(
        a.verdict.as_ref().map(|v| (v.verified_by, v.killed)),
        Some((VerifiedBy::Tree, Some(2)))
    );
    assert!(
        spool.read_stop(&job.correlation_id).is_none(),
        "its verdict is taken"
    );
    again.answer_earlier_cancels();
    assert!(
        spool.read_stop("act_long_gone").is_none(),
        "an old verdict is swept"
    );
    let late = late_results(&again, &first.session_id);
    assert_eq!(late.len(), 1, "{late:?}");
    assert!(
        late[0].contains("cancelled by test") && late[0].contains("it was stopped"),
        "{late:?}"
    );
    assert_eq!(unanswered_calls(&again, &first.session_id), 0);
}

/// KEY (vn4d): a stop that reaches a job before its pid is written settles it `unsupported`; its launch then
/// stops it, and the stop's verdict is the action's now, so the call reads as stopped and not as a call
/// that "cannot be stopped once started". The session stays open and resumable.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_job_a_stop_reached_before_its_pid_ends_verified_by_its_launchs_stop() {
    let (held, mut there, go) = Held::new();
    let launcher = Arc::new(HeldLaunch {
        held,
        started: Default::default(),
    });
    let l = launcher.clone();
    let r = rig_parts(
        vec![
            Scripted::tools("", &[("t1", "proc_run", json!({"argv": ["sleep", "60"]}))]),
            Scripted::text("Resumed."),
        ],
        |cfg| {
            cfg.policy.tools.insert("proc.run".into(), Posture::Open);
        },
        |p| p.launcher = l,
    );
    let core = r.core.clone();
    let first = tokio::spawn(async move { turn(&core, None, "sleep a minute").await });
    there.recv().await.unwrap();
    let job = the_job(&r.core);
    let corr = job.correlation_id.clone();
    r.core
        .stop_execution(&job.execution_id, "test")
        .await
        .unwrap();
    // The stop found no pid: it reached nothing.
    let a = r.core.kernel.action(&corr).unwrap().unwrap();
    assert_eq!(a.cancel, Some(CancelState::Unsupported));
    go.send(()).unwrap();
    let res = tokio::time::timeout(Duration::from_secs(20), first)
        .await
        .expect("the turn ended")
        .unwrap();
    assert_eq!(res.stop_reason, "stopped");
    // The launch stopped it, and the action says how.
    let a = r.core.kernel.action(&corr).unwrap().unwrap();
    assert_eq!(
        (a.state, a.cancel),
        (
            ActionState::Cancelled,
            Some(CancelState::TerminationVerified)
        ),
        "{a:?}"
    );
    assert!(
        a.verdict.as_ref().is_some_and(Verdict::verified),
        "{:?}",
        a.verdict
    );
    let (status, text) = results(&r.core, &res.session_id).remove(0);
    assert_eq!(status, ResultStatus::Cancelled, "{text}");
    assert!(!text.contains("cannot be stopped"), "{text}");
    // Nothing is held as unknown spend for a job that was verifiably stopped.
    let e = r.core.kernel.execution(&job.execution_id).unwrap().unwrap();
    assert_eq!(e.budget.held_unknown_micros, 0, "{:?}", e.budget);
    // Resumable: the session takes its next message.
    let next = turn(&r.core, Some(&res.session_id), "go on").await;
    assert_eq!(next.output, "Resumed.");
}
