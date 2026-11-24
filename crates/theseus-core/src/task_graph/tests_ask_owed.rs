//! `task.ask`'s reserved call always has an owner that settles it (issue 44): dropped before its send it is
//! settled as costing nothing; cut off once its send may have begun it is held as unknown, once; a failure the
//! provider names costs nothing, and a timeout is held; a second completion is refused. Each reads the
//! conversation's spend book and the day's.

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use serde_json::json;
use theseus_kernel::{Accepted, Action, ActionState, Completion, Outcome, PROVIDER_TOOL};

use super::tests::{ask_of, rig, Rig, QUESTION};
use super::*;
use crate::node::{Body, ResultStatus};
use crate::provider::{ProviderError, Scripted, TimeoutPhase};

/// The conversation's spend book and the day's.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Books {
    spent: u64,
    reserved: u64,
    unknown: u64,
    reservations: BTreeMap<String, u64>,
    day_spent: u64,
    day_held: u64,
}

fn books(core: &Core, exec: &str) -> Books {
    let e = core.kernel.execution(exec).unwrap().expect("the execution");
    let day = core.kernel.day_ceiling().today(core.kernel.now_ms());
    Books {
        spent: e.budget.spent_micros,
        reserved: e.budget.reserved_micros,
        unknown: e.budget.held_unknown_micros,
        reservations: e.budget.reservations,
        day_spent: day.spent,
        day_held: day.held,
    }
}

/// The asking conversation's execution, once it has run a turn.
fn exec_of(r: &Rig) -> String {
    let rec: crate::session::SessionRecord = r.core.store.get_session(&r.parent).unwrap().unwrap();
    rec.execution_id.expect("the conversation's execution")
}

/// The provider calls the kernel holds.
fn provider_calls(core: &Core) -> Vec<Action> {
    (core.kernel.actions().unwrap().into_iter())
        .filter(|a| a.tool == PROVIDER_TOOL)
        .collect()
}

/// The ask's provider call as its request arrives: the one provider call open then (the turn's call that
/// made the ask has settled, and its next is not planned yet). The kernel keeps no proposal on it.
fn open_ask(core: &Core) -> Action {
    let open: Vec<Action> = (core.kernel.open_actions().unwrap().into_iter())
        .filter(|a| a.tool == PROVIDER_TOOL)
        .collect();
    assert_eq!(open.len(), 1, "one provider call open: {open:?}");
    open.into_iter().next().unwrap()
}

/// The call `id` as it is now.
fn call(core: &Core, id: &str) -> Action {
    core.kernel.action(id).unwrap().expect("the call")
}

type Kept = Arc<Mutex<Option<(Books, Action)>>>;
type KeptBooks = Arc<Mutex<Option<Books>>>;

/// The books and the ask's call, kept as the ask's request arrives; then `then` runs.
fn keep_at_ask(r: &Rig, exec: &str, then: impl FnOnce(Arc<Core>, String) + Send + 'static) -> Kept {
    let kept: Kept = Arc::default();
    let (core, exec, k) = (r.core.clone(), exec.to_string(), kept.clone());
    *r.model.on_ask.lock().unwrap() = Some(Box::new(move || {
        let a = open_ask(&core);
        *k.lock().unwrap() = Some((books(&core, &exec), a));
        then(core, exec);
    }));
    kept
}

/// The books, kept as the turn's next request is made, after the ask's result.
fn keep_after_ask(r: &Rig, exec: &str) -> KeptBooks {
    let kept: KeptBooks = Arc::default();
    let (core, exec, k) = (r.core.clone(), exec.to_string(), kept.clone());
    *r.model.after_ask.lock().unwrap() = Some(Box::new(move || {
        *k.lock().unwrap() = Some(books(&core, &exec));
    }));
    kept
}

fn taken(k: &Kept) -> (Books, Action) {
    k.lock()
        .unwrap()
        .clone()
        .expect("the ask's request arrived")
}

fn taken_books(k: &KeptBooks) -> Books {
    k.lock()
        .unwrap()
        .clone()
        .expect("the turn asked again after the ask")
}

/// What the conversation's other calls hold: every open reservation but the ask's.
fn others(b: &Books, ask: &str) -> u64 {
    (b.reservations.iter())
        .filter(|(id, _)| id.as_str() != ask)
        .map(|(_, m)| m)
        .sum()
}

/// A rig whose conversation has run a turn, so its execution is there to read.
async fn warmed() -> (Rig, String) {
    let r = rig().await;
    crate::test_support::turn(&r.core, &r.parent, "hello").await;
    let exec = exec_of(&r);
    (r, exec)
}

/// The text of the ask's one result.
fn result_text(r: &Rig) -> (ResultStatus, String) {
    let answers = r.answers();
    assert_eq!(answers.len(), 1, "{answers:?}");
    match answers[0].body.clone() {
        Body::ToolResult {
            status, content, ..
        } => (status, content),
        _ => unreachable!(),
    }
}

/// Planned and reserved, then dropped before it was ever polled, so before anything could be sent: the call
/// settles as costing nothing, and both books are as they were before it was planned.
#[tokio::test]
async fn an_ask_dropped_before_its_send_holds_nothing() {
    let (r, exec) = warmed().await;
    let guard = r.core.kernel.admit_input(&exec).unwrap();
    let before = books(&r.core, &exec);
    let sink = crate::bus::EventSink::new(r.core.bus.clone(), &r.parent, None);
    let posts = Mutex::new(Vec::new());
    let commitments = crate::commitments::Pending::default();
    let tc = TurnCtx {
        kernel: &r.core.kernel,
        store: &r.core.store,
        guard: &guard,
        session_id: &r.parent,
        execution_id: &exec,
        turn_id: "turn_dropped",
        loop_index: Some(0),
        sink: &sink,
        confirm_ttl_ms: 60_000,
        narrator: &r.core.narrator,
        outbox: &r.core.outbox,
        posts: &posts,
        target: None,
        task: None,
        class: crate::places::PlaceClass::Private,
        ceiling: None,
        places: crate::places::Places {
            rule: &r.core.runner.place_rule,
            cfg: &r.core.cfg,
        },
        dir: None,
        commitments: &commitments,
    };
    let earlier: Vec<String> = (provider_calls(&r.core).into_iter())
        .map(|a| a.correlation_id)
        .collect();
    let run = (r.core.tools.task_ask).run(&tc, &json!({"id": r.task, "question": QUESTION}));
    let a = (provider_calls(&r.core).into_iter())
        .find(|a| !earlier.contains(&a.correlation_id))
        .expect("planned");
    assert_eq!(a.state, ActionState::Dispatched, "reserved and dispatched");
    assert!(a.reserved_micros > 0);
    let rid = a.reservation_id.clone().expect("its reservation");
    let held = books(&r.core, &exec);
    assert_eq!(held.reservations.get(&rid), Some(&a.reserved_micros));
    assert_eq!(held.day_held, before.day_held + a.reserved_micros);
    drop(run);
    let a = call(&r.core, &a.correlation_id);
    assert_eq!(a.state, ActionState::Failed, "settled as never sent: {a:?}");
    assert_eq!(books(&r.core, &exec), before, "nothing held, nothing spent");
    assert!(r.model.asks().is_empty(), "nothing was sent");
    drop(guard);
}

/// A stop while the ask's call is in flight (the fake never answers): the turn ends with the call outstanding,
/// and its owner settles it as unknown. Its whole reservation is held on the conversation, not released and
/// not booked as spent, and the day counts it once, at its reservation. No dispatched call is left.
#[tokio::test]
async fn an_ask_stopped_in_flight_is_held_as_unknown_and_left_dispatched_by_none() {
    let (r, exec) = warmed().await;
    r.model.hang.store(true, Ordering::SeqCst);
    let at = keep_at_ask(&r, &exec, |core, exec| {
        tokio::spawn(async move {
            let _ = core.stop_execution(&exec, "tester").await;
        });
    });
    r.calls(vec![vec![ask_of(&r.task)]]);
    let _ = crate::test_support::try_turn(&r.core, &r.parent, "ask the task why").await;
    let id = taken(&at).1.correlation_id;
    crate::test_support::until("the ask's call settled", || {
        call(&r.core, &id).state != ActionState::Dispatched
    })
    .await;
    let a = call(&r.core, &id);
    assert_eq!(a.state, ActionState::OutcomeUnknown, "{a:?}");
    let (at, _) = taken(&at);
    let now = books(&r.core, &exec);
    let (rid, res) = (a.reservation_id.clone().unwrap(), a.reserved_micros);
    assert_eq!(
        at.reservations.get(&rid),
        Some(&res),
        "reserved as it was sent"
    );
    assert!(!now.reservations.contains_key(&rid), "no longer open");
    assert_eq!(now.unknown, at.unknown + res, "held as unknown, whole");
    assert_eq!(now.spent, at.spent, "not booked as spent");
    assert_eq!(now.reserved, at.reserved - res);
    assert_eq!(now.day_spent, at.day_spent + res, "the day counts it once");
    assert_eq!(now.day_held, at.day_held - res);
    let open = r.core.kernel.open_actions().unwrap();
    assert!(
        !open
            .iter()
            .any(|x| x.correlation_id == a.correlation_id && x.state == ActionState::Dispatched),
        "{open:?}"
    );
}

/// A cancel of the conversation while the ask's call is in flight: held as unknown once, on both books, never
/// released and never counted twice.
#[tokio::test]
async fn an_ask_cancelled_in_flight_is_held_once() {
    let (r, exec) = warmed().await;
    r.model.hang.store(true, Ordering::SeqCst);
    let at = keep_at_ask(&r, &exec, |core, exec| {
        tokio::spawn(async move {
            let _ = core.cancel_execution(&exec, "tester").await;
        });
    });
    r.calls(vec![vec![ask_of(&r.task)]]);
    let _ = crate::test_support::try_turn(&r.core, &r.parent, "ask the task why").await;
    let id = taken(&at).1.correlation_id;
    // The cancel's own step and its owner's drop are both done.
    crate::test_support::until("the ask's call settled", || {
        call(&r.core, &id).state != ActionState::Dispatched
            && r.core.kernel.open_actions().unwrap().is_empty()
    })
    .await;
    let a = call(&r.core, &id);
    assert!(
        matches!(
            a.state,
            ActionState::Cancelled | ActionState::OutcomeUnknown
        ),
        "{a:?}"
    );
    let (at, _) = taken(&at);
    let now = books(&r.core, &exec);
    let (rid, res) = (a.reservation_id.clone().unwrap(), a.reserved_micros);
    assert!(!now.reservations.contains_key(&rid), "no longer open");
    assert_eq!(now.unknown, at.unknown + res, "held as unknown, whole");
    assert_eq!(now.spent, at.spent, "not released as spent");
    assert_eq!(now.day_spent, at.day_spent + res, "the day counts it once");
    assert_eq!(now.day_held, at.day_held - res);
}

/// A failure the provider names (a refused request) is known to have cost nothing: settled at zero on both
/// books, and the result says so.
#[tokio::test]
async fn a_provider_failure_with_known_zero_usage_costs_nothing() {
    let (r, exec) = warmed().await;
    *r.model.answer.lock().unwrap() = Some(Scripted::Fail(ProviderError::InvalidRequest {
        status: 400,
        message: "bad request".into(),
    }));
    let at = keep_at_ask(&r, &exec, |_, _| {});
    let next = keep_after_ask(&r, &exec);
    r.calls(vec![vec![ask_of(&r.task)]]);
    crate::test_support::turn(&r.core, &r.parent, "ask the task why").await;
    let ((at, a), next) = (taken(&at), taken_books(&next));
    let a = call(&r.core, &a.correlation_id);
    assert_eq!(a.state, ActionState::Failed, "{a:?}");
    let (rid, res) = (a.reservation_id.clone().unwrap(), a.reserved_micros);
    assert!(!next.reservations.contains_key(&rid));
    assert_eq!(next.spent, at.spent, "nothing spent");
    assert_eq!(next.unknown, at.unknown, "nothing held");
    assert_eq!(next.day_spent, at.day_spent, "nothing spent today");
    assert_eq!(
        next.day_held + res + others(&at, &rid),
        at.day_held + others(&next, &rid),
        "its hold on the day let go"
    );
    let (status, text) = result_text(&r);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(text.contains("The call cost nothing"), "{text}");
}

/// A timeout leaves the cost unknown: the whole reservation is held on the conversation and counted by the
/// day, and the result says, in the harness's words, that its cost is not known and is held.
#[tokio::test]
async fn a_timed_out_ask_is_held_as_unknown() {
    let (r, exec) = warmed().await;
    *r.model.answer.lock().unwrap() = Some(Scripted::Fail(ProviderError::Timeout {
        phase: TimeoutPhase::Total,
        elapsed_ms: 100_000,
    }));
    let at = keep_at_ask(&r, &exec, |_, _| {});
    let next = keep_after_ask(&r, &exec);
    r.calls(vec![vec![ask_of(&r.task)]]);
    crate::test_support::turn(&r.core, &r.parent, "ask the task why").await;
    let ((at, a), next) = (taken(&at), taken_books(&next));
    let a = call(&r.core, &a.correlation_id);
    assert_eq!(a.state, ActionState::OutcomeUnknown, "{a:?}");
    let (rid, res) = (a.reservation_id.clone().unwrap(), a.reserved_micros);
    assert!(!next.reservations.contains_key(&rid));
    assert_eq!(next.unknown, at.unknown + res, "held as unknown, whole");
    assert_eq!(next.spent, at.spent, "not booked as spent");
    assert_eq!(next.day_spent, at.day_spent + res, "the day counts it once");
    let (status, text) = result_text(&r);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(
        text.contains("Its cost is not known") && text.contains("stays held"),
        "{text}"
    );
}

/// An answered ask is settled once, by its own completion: its owner adds none. A second completion, or a
/// late one saying unknown, is refused by the kernel, and neither book moves.
#[tokio::test]
async fn a_second_or_late_completion_of_an_ask_is_refused() {
    let (r, exec) = warmed().await;
    let at = keep_at_ask(&r, &exec, |_, _| {});
    r.calls(vec![vec![ask_of(&r.task)]]);
    crate::test_support::turn(&r.core, &r.parent, "ask the task why").await;
    let a = call(&r.core, &taken(&at).1.correlation_id);
    assert_eq!(a.state, ActionState::Succeeded, "{a:?}");
    assert_eq!(
        a.completions_seen, 1,
        "settled once: its owner made no second"
    );
    let before = books(&r.core, &exec);
    for outcome in [Outcome::Succeeded, Outcome::Unknown] {
        let c = Completion {
            correlation_id: a.correlation_id.clone(),
            outcome,
            result_ref: None,
            external_op_id: None,
            started_at_ms: 0,
            finished_at_ms: 0,
            producer: "provider:fake".into(),
            signature: None,
            cost_micros: Some(a.reserved_micros * 3),
            detail: None,
        };
        let got = r
            .core
            .kernel
            .accept_completion_with(&c, Vec::new())
            .unwrap();
        assert!(matches!(got, Accepted::DuplicateNoop { .. }), "{got:?}");
    }
    assert_eq!(books(&r.core, &exec), before, "neither book moved");
    assert_eq!(
        call(&r.core, &a.correlation_id).state,
        ActionState::Succeeded
    );
}
