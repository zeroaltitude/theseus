//! The harness loop's bind (theseus-74lt). A restart's kernel startup drains
//! the spool before the daemon serves, and the loop binds the wrappers'
//! notify socket only after serving: a job's result spooled between the two
//! had its one notify refused, since nothing listened. The loop takes it at
//! its bind, not a `heartbeat_ms` (60 s) later. The clock is tokio's paused
//! one, so the test reads the loop's own waits, never the machine's load.

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use theseus_kernel::{
    ActionState, Authority, Completion, ExecState, Outcome, Proposal, RetryClass, TurnEnd, Wake,
};
use theseus_protocol::SessionKind;

use crate::provider::FakeProvider;
use crate::session::SessionRecord;
use crate::store::Store;
use crate::{Config, Core};

/// A core over `state`, its startup done: the spool was empty for its drain.
fn core(state: &std::path::Path) -> Arc<Core> {
    let mut cfg = Config::example();
    cfg.server.state_dir = state.to_string_lossy().into_owned();
    cfg.tools.roots = vec![];
    let store = Store::open(&state.join("store")).unwrap();
    let provider = Arc::new(FakeProvider::scripted(vec![]));
    Core::build(crate::rpc::Parts::for_tests(cfg, provider, store)).unwrap()
}

/// A session's execution that waits on one job, sent: the execution's id and
/// the job's correlation id.
fn waiting_on_a_job(core: &Core) -> (String, String) {
    let k = &core.kernel;
    let mut rec = SessionRecord::new(SessionKind::Conversation, None);
    let operator = Authority {
        principal: crate::turn::OPERATOR.into(),
        ..Default::default()
    };
    let e = k
        .open_execution(
            &rec.session_id,
            SessionKind::Conversation,
            operator,
            None,
            None,
        )
        .unwrap();
    rec.execution_id = Some(e.id.clone());
    core.store.put_session(&rec.session_id, &rec).unwrap();
    k.wake_input(&e.id).unwrap();
    let guard = k.admit(&e.id).unwrap();
    let p = Proposal {
        tool: "proc.run".into(),
        args: json!({"argv": ["true"]}),
        resource: None,
        policy_context: serde_json::Value::Null,
    };
    let a = k
        .plan_action(&guard, &p, RetryClass::SafeToRepeat, Some(600_000), 0)
        .unwrap();
    k.authorize(&a.correlation_id, &p, None).unwrap();
    let a = k.dispatch(&a.correlation_id, None).unwrap();
    let wake = Wake::Actions {
        correlation_ids: vec![a.correlation_id.clone()],
    };
    k.end_turn(guard, TurnEnd::Wait { wake }).unwrap();
    (e.id, a.correlation_id)
}

/// The job's wrapper ends after the startup's drain: its result goes into the
/// spool, and its one notify finds no socket.
fn the_wrapper_ends(core: &Core, job: &str) {
    let out = core.spool.result_path(job);
    std::fs::write(&out, "done\n").unwrap();
    let now = theseus_protocol::now_unix_ms();
    core.spool
        .write(&Completion {
            correlation_id: job.into(),
            outcome: Outcome::Succeeded,
            result_ref: Some(out.to_string_lossy().into_owned()),
            external_op_id: None,
            started_at_ms: now,
            finished_at_ms: now,
            producer: "test".into(),
            signature: None,
            cost_micros: None,
            detail: Some(json!({"exit_code": 0})),
        })
        .unwrap();
    theseus_kernel::job::notify(&crate::harness::notify_socket_path(core), job);
}

/// A result spooled between the startup's drain and the loop's bind is taken
/// at the bind: the job settles and its execution is queued for the driver
/// within a second, with the heartbeat at its 60 s default.
#[tokio::test(start_paused = true)]
async fn a_result_spooled_before_the_loop_binds_is_taken_at_the_bind() {
    let dir = tempfile::tempdir().unwrap();
    let core = core(dir.path());
    assert_eq!(core.kernel.config().heartbeat_ms, 60_000, "the default");
    let (exec, job) = waiting_on_a_job(&core);
    the_wrapper_ends(&core, &job);
    let t0 = tokio::time::Instant::now();
    let harness = tokio::spawn(crate::harness::run(core.clone()));
    while core.spool.has_completion(&job) {
        let waited = t0.elapsed();
        assert!(
            waited < Duration::from_secs(1),
            "the result still waits in the spool after {waited:?}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let a = core.kernel.action(&job).unwrap().unwrap();
    assert_eq!(a.state, ActionState::Succeeded);
    let e = core.kernel.execution(&exec).unwrap().unwrap();
    assert_eq!((e.state, e.queued_results), (ExecState::Queued, vec![job]));
    core.shutdown.notify_waiters();
    tokio::time::timeout(Duration::from_secs(5), harness)
        .await
        .expect("the loop stops at shutdown")
        .unwrap();
}
