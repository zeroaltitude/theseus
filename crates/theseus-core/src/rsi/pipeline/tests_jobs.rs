//! A build whose turn ended on a job it started (a test run past the call's time limit goes on as a
//! background job whose end wakes the turn) is still building. On 2026-11-17 such a build's
//! execution waited on its job, the pipeline read "not running, queued or blocked" as a turn that
//! ended with no branch, failed the item (a try against its cause's cap) and removed its working
//! copy while the builder was still working: its next `proc.run` found no directory, and its
//! `self.propose_change` was refused because the item had used all its tries.

use std::time::Duration;

use serde_json::json;
use theseus_kernel::{Proposal, RetryClass, TurnEnd, Wake};
use theseus_protocol::tasks::TaskState;

use super::tests::{rig_with, Rig};
use crate::rsi::tree;

/// The build of `item`, its turn ended waiting on a dispatched job: the job's id.
fn wait_on_job(r: &Rig, item: &str) -> (String, String) {
    let t = crate::task_graph::get(&r.core.store, item)
        .unwrap()
        .unwrap();
    let exec = t
        .evidence
        .iter()
        .find_map(|e| e.identity.strip_prefix("execution:"))
        .expect("a started item has an execution")
        .to_string();
    let k = &r.core.kernel;
    k.wake_input(&exec).unwrap();
    let guard = k.admit(&exec).unwrap();
    let p = Proposal {
        tool: "proc.run".into(),
        args: json!({"argv": ["cargo", "test"]}),
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
    (exec, a.correlation_id)
}

/// KEY: a build waiting on its running job keeps its slot, its working copy and its record; the
/// pipeline fails nothing and charges no try.
#[tokio::test]
async fn a_build_waiting_on_its_job_is_still_building() {
    let mut r = rig_with(1, 30.0);
    r.grace = Duration::ZERO;
    r.tick().await;
    let item = r.fake.starts.lock().unwrap()[0].0.clone();
    wait_on_job(&r, &item);
    for _ in 0..2 {
        r.tick().await;
    }
    assert_eq!(r.state(&item), TaskState::InProgress);
    assert!(tree::find(&r.f.place, &item).is_some(), "its working copy");
    assert!(r.rows("self.pipeline.ended").is_empty());
    let t = crate::task_graph::get(&r.core.store, &item)
        .unwrap()
        .unwrap();
    assert!(
        crate::rsi::backlog::attempts::tries(&t, 0).is_empty(),
        "no try is charged to a build that still runs"
    );
}

/// A build whose turn is over for good (it waits for input, nothing will wake it) is ended as
/// before, so the wait on a job is the only thing that changed.
#[tokio::test]
async fn a_build_waiting_on_nothing_still_ends() {
    let mut r = rig_with(1, 30.0);
    r.grace = Duration::ZERO;
    r.tick().await;
    let item = r.fake.starts.lock().unwrap()[0].0.clone();
    r.tick().await;
    assert_eq!(r.state(&item), TaskState::Failed);
    assert_eq!(r.rows("self.pipeline.ended").len(), 1);
}
