//! An input's turn is admitted in its input's frame (theseus-2uby): the
//! frame that writes `execution.running` writes `turn.started` and the
//! user's node too, so a plain turn pays two syncs before its model's first
//! byte (that frame, and its call's dispatch), counted on its trace
//! (`syncs_before_call`). A cancel or a stop that lands while the turn holds
//! its execution, before that frame, is read there: no call goes out. A
//! child of `tests_m3` for its rig and helpers.

use super::*;
use crate::turn::admit_hook;

/// The turn's syncs before its first call, from its trace.
fn syncs(res: &TurnSubmitResult) -> u64 {
    res.trace.as_ref().expect("a trace").attrs["syncs_before_call"]
        .as_u64()
        .expect("a count of syncs")
}

/// Every turn frame from now on, each as its records' ledger kinds, and
/// `node` and `execution` for those records (the fault seam's check, which
/// sees each turn frame and fails none).
fn spy(core: &Core) -> Arc<std::sync::Mutex<Vec<Vec<String>>>> {
    let seen: Arc<std::sync::Mutex<Vec<Vec<String>>>> = Arc::default();
    let into = seen.clone();
    core.store.fail_turn_frame(move |records| {
        let frame = records
            .iter()
            .map(|rec| match rec.kind {
                theseus_store::kinds::LEDGER => serde_json::from_slice::<Value>(&rec.payload)
                    .map(|v| v["kind"].as_str().unwrap_or("?").to_string())
                    .unwrap_or_default(),
                theseus_store::kinds::NODE => "node".to_string(),
                theseus_store::kinds::EXECUTION => "execution".to_string(),
                k => format!("kind {k}"),
            })
            .collect();
        into.lock().unwrap().push(frame);
        false
    });
    seen
}

/// A resumed session's plain turn: the admission, `turn.started` and the
/// input's node are one frame, the call's plan and dispatch the next, and its
/// trace counts 2 syncs before the call. A new session's first turn (opened
/// as `session.open` opens it, its execution in that frame) counts one more:
/// its first compilation's record, written before the call (a recompile's
/// frame, which this step leaves as it was).
#[tokio::test]
async fn an_inputs_admission_rides_its_inputs_frame() {
    let r = rig(vec![Scripted::text("first"), Scripted::text("hello")]);
    let rec = r
        .core
        .open_session(theseus_protocol::SessionOpenParams {
            kind: None,
            label: None,
            opened_from: None,
        })
        .unwrap();
    let seen = spy(&r.core);
    let first = turn(&r.core, Some(&rec.session_id), "warm up").await;
    let frames = std::mem::take(&mut *seen.lock().unwrap());
    assert_eq!(syncs(&first), 3, "a new session's first turn: {frames:?}");
    assert_eq!(
        frames[0][..5],
        [
            "execution.queued",
            "execution",
            "execution.running",
            "turn.started",
            "node"
        ],
        "{frames:?}"
    );
    assert!(
        frames[1].iter().any(|k| k == "context.recompiled"),
        "its first compilation: {frames:?}"
    );
    let seen = spy(&r.core);
    let res = turn(&r.core, Some(&rec.session_id), "hi").await;
    assert_eq!(syncs(&res), 2, "a resumed session's plain turn");
    let frames = seen.lock().unwrap().clone();
    assert_eq!(
        frames.iter().filter(|f| !f.is_empty()).count(),
        4,
        "{frames:?}"
    );
    assert_eq!(
        frames[0],
        [
            "execution.queued",
            "execution",
            "execution.running",
            "turn.started",
            "node"
        ],
        "{frames:?}"
    );
    assert!(
        frames[1].iter().any(|k| k == "action.dispatched"),
        "the call's dispatch is the second: {frames:?}"
    );
}

/// The turn of an input submitted with `f` landing just before its input's
/// frame, and the execution's id.
async fn held_with(
    r: &Rig,
    f: impl FnOnce(&Arc<Core>, &str) + Send + 'static,
) -> (anyhow::Result<TurnSubmitResult>, String, String) {
    let first = turn(&r.core, None, "warm up").await;
    let sid = first.session_id.clone();
    let exec = first.execution_id.clone().expect("an execution");
    let (core, id) = (r.core.clone(), exec.clone());
    admit_hook(&sid, Box::new(move || f(&core, &id)));
    let rec = r
        .core
        .store
        .get_session::<SessionRecord>(&sid)
        .unwrap()
        .unwrap();
    let (live, _) = r.core.live_profile();
    let target = r
        .core
        .runner
        .resolve_target(&live, None, None, None)
        .unwrap();
    let sink = EventSink::new(r.core.bus.clone(), &sid, None);
    let res = r
        .core
        .runner
        .run(TurnRequest {
            prompt: None,
            session: rec,
            input: Some("do the thing".into()),
            target,
            sink,
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
        })
        .await;
    (res, sid, exec)
}

/// The provider calls an execution dispatched.
fn dispatched(core: &Core, exec: &str) -> usize {
    core.kernel
        .actions()
        .unwrap()
        .into_iter()
        .filter(|a| a.execution_id == exec && a.tool == theseus_kernel::PROVIDER_TOOL)
        .filter(|a| a.dispatched_at_ms.is_some())
        .count()
}

/// A cancel that lands while the turn is held (its compile's place in the
/// old order) is read under the input frame's lock: the turn fails
/// `execution_cancelled`, its input unwritten, and no call goes out.
#[tokio::test]
async fn a_cancel_while_the_turn_is_held_ends_it_with_no_dispatch() {
    let r = rig(vec![Scripted::text("first"), Scripted::text("never sent")]);
    let (res, sid, exec) = held_with(&r, |core, id| {
        core.kernel.cancel_execution(id, "operator").unwrap();
    })
    .await;
    let e = res.expect_err("a cancelled turn fails");
    let te = e
        .downcast_ref::<crate::turn::TurnError>()
        .expect("a turn's failure");
    assert_eq!(te.class, "execution_cancelled", "{e:#}");
    assert_eq!(dispatched(&r.core, &exec), 1, "only the first turn's call");
    assert_eq!(r.fake.requests().len(), 1);
    let texts: Vec<String> = r
        .core
        .store
        .session_nodes(&sid)
        .unwrap()
        .into_iter()
        .filter_map(|(_, n)| match n.body {
            Body::UserMessage { text, .. } => Some(text),
            _ => None,
        })
        .collect();
    assert_eq!(texts, ["warm up"], "the input was not written");
    let e = r.core.kernel.execution(&exec).unwrap().unwrap();
    assert_eq!(e.state, theseus_kernel::ExecState::Cancelled);
    assert!(!r.core.kernel.holds_turn(&exec), "the turn let it go");
}

/// A stop that lands while the turn is held, by the kernel alone (no
/// `stop_landed`), marks the held turn, and the admission keeps the mark:
/// the turn plans nothing and parks on input. Before the kernel counted a
/// held turn as running, the stop parked the execution and the admission's
/// wake undid it, and the call went out.
#[tokio::test]
async fn a_stop_while_the_turn_is_held_ends_it_with_no_dispatch() {
    let r = rig(vec![Scripted::text("first"), Scripted::text("never sent")]);
    let (res, _sid, exec) = held_with(&r, |core, id| {
        let stop = core.kernel.stop_execution(id, "operator").unwrap().unwrap();
        assert!(stop.turn_running, "{stop:?}");
    })
    .await;
    let res = res.expect("a stopped turn ends");
    assert_eq!(res.stop_reason, "stopped", "{res:?}");
    assert_eq!(dispatched(&r.core, &exec), 1, "only the first turn's call");
    assert_eq!(r.fake.requests().len(), 1);
    let e = r.core.kernel.execution(&exec).unwrap().unwrap();
    assert_eq!(e.state, theseus_kernel::ExecState::Waiting);
    assert!(e.stopped.is_none(), "the end clears the mark");
}

/// The input's frame is the turn's start whole or not at all: when it fails
/// (as a full disk fails it), nothing of it is written, and the turn's one
/// exit (a fault) admits and ends the held turn in its end's frame, as the
/// end of a turn admitted before it was, so the execution is waiting again,
/// never left running; no call went out, the input is not stored, and the
/// next input runs a turn. Written, the frame holds the admission and the
/// input together (`an_inputs_admission_rides_its_inputs_frame`).
#[tokio::test]
async fn a_failed_input_frame_writes_neither_half_and_the_turn_ends_waiting() {
    let r = rig(vec![Scripted::text("first"), Scripted::text("after it")]);
    let first = turn(&r.core, None, "warm up").await;
    let sid = first.session_id.clone();
    let exec = first.execution_id.clone().expect("an execution");
    let before = r.core.kernel.execution(&exec).unwrap().unwrap();
    r.core.store.fail_turn_frame(|records| {
        records.iter().any(|rec| {
            rec.kind == theseus_store::kinds::NODE
                && rec.payload.windows(9).any(|w| w == b"lost line")
        })
    });
    let rec = r
        .core
        .store
        .get_session::<SessionRecord>(&sid)
        .unwrap()
        .unwrap();
    let (live, _) = r.core.live_profile();
    let target = r
        .core
        .runner
        .resolve_target(&live, None, None, None)
        .unwrap();
    let failed = r
        .core
        .runner
        .run(TurnRequest {
            prompt: None,
            session: rec,
            input: Some("a lost line".into()),
            target,
            sink: EventSink::new(r.core.bus.clone(), &sid, None),
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
        })
        .await;
    assert!(
        failed.is_err(),
        "the turn whose start was not written failed"
    );
    let after = r.core.kernel.execution(&exec).unwrap().unwrap();
    assert_eq!(
        (after.state, after.turns),
        (theseus_kernel::ExecState::Waiting, before.turns + 1),
        "the fault's end admits and ends the held turn"
    );
    assert_eq!(dispatched(&r.core, &exec), 1, "only the first turn's call");
    let inputs = |core: &Core| -> Vec<String> {
        core.store
            .session_nodes(&sid)
            .unwrap()
            .into_iter()
            .filter_map(|(_, n)| match n.body {
                Body::UserMessage { text, .. } => Some(text),
                _ => None,
            })
            .collect()
    };
    assert_eq!(inputs(&r.core), ["warm up"]);
    let next = turn(&r.core, Some(&sid), "the next input").await;
    assert_eq!(next.output, "after it");
    assert_eq!(inputs(&r.core), ["warm up", "the next input"]);
}
