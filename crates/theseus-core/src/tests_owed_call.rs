//! The reserved call's owner settles it in one step (issue 44): a cancel that marks the call while a drop is
//! deciding cannot make the day count it twice; an unknown settlement that lands after a cancel marked the call
//! is left to the cancel; a known cost still lands. A drop settles on the dropping thread, before it returns; a
//! drop inside a frame (another execution's, or the call's own) writes nothing, returns at once, and leaves the
//! call to the reconcile. Each reads the conversation's spend book and the day's, on a bare kernel whose clock
//! can stop a dropping thread between its start and its write, and notes every thread that reads it.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

use serde_json::json;
use theseus_kernel::{
    Action, ActionState, Authority, Clock, Completion, Kernel, KernelConfig, NoEvidence, Outcome,
    Proposal, RetryClass, SessionKind, TurnGuard, PROVIDER_TOOL,
};

use super::Owed;
use crate::store::Store;

const NOW: u64 = 1_800_000_000_000;
const RESERVE: u64 = 40_000;

/// A clock that stands still unless a test moves it on (`ahead`), notes the thread of every reading, and stops one
/// thread at its next reading until the test lets it go.
#[derive(Default)]
struct Hooked {
    stop: Mutex<Option<(ThreadId, Arc<Barrier>)>>,
    ahead: AtomicU64,
    readers: Mutex<Vec<(ThreadId, Option<String>)>>,
}

impl Clock for Hooked {
    fn now_ms(&self) -> u64 {
        let me = std::thread::current();
        (self.readers.lock().unwrap()).push((me.id(), me.name().map(str::to_string)));
        let me = me.id();
        let mut stop = self.stop.lock().unwrap();
        if stop.as_ref().is_some_and(|(t, _)| *t == me) {
            let (_, b) = stop.take().unwrap();
            drop(stop);
            b.wait(); // stopped: the test may act
            b.wait(); // let go
        }
        NOW + self.ahead.load(Ordering::SeqCst)
    }
}

/// The conversation's spend book and the day's.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Books {
    spent: u64,
    unknown: u64,
    reservations: BTreeMap<String, u64>,
    day_spent: u64,
    day_held: u64,
}

fn books(k: &Kernel, exec: &str) -> Books {
    let e = k.execution(exec).unwrap().expect("the execution");
    let day = k.day_ceiling().today(NOW);
    Books {
        spent: e.budget.spent_micros,
        unknown: e.budget.held_unknown_micros,
        reservations: e.budget.reservations,
        day_spent: day.spent,
        day_held: day.held,
    }
}

struct World {
    _dir: tempfile::TempDir,
    clock: Arc<Hooked>,
    kernel: Arc<Kernel>,
    exec: String,
    call: String,
    _turn: TurnGuard,
}

/// A conversation whose turn has reserved and dispatched one provider call, and its owner.
fn world() -> (World, Owed) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let clock = Arc::new(Hooked::default());
    let kernel = Arc::new(Kernel::new(
        store.shared(),
        clock.clone(),
        KernelConfig::default(),
    ));
    kernel.startup(None, &NoEvidence).unwrap();
    let auth = Authority {
        principal: "owner".into(),
        delegated_by: None,
        ceilings: BTreeMap::new(),
    };
    let e = (kernel.open_execution(
        "ses_owed",
        SessionKind::Conversation,
        auth,
        Some(10_000_000),
        None,
    ))
    .unwrap();
    let turn = kernel.admit_input(&e.id).unwrap();
    let proposal = Proposal {
        tool: PROVIDER_TOOL.into(),
        args: json!({"provider": "fake", "model": "fake", "purpose": "test"}),
        resource: Some("fake".into()),
        policy_context: json!({}),
    };
    let a = (kernel.plan_and_dispatch(
        &turn,
        &proposal,
        RetryClass::SafeToRepeat,
        Some(100_000),
        RESERVE,
        |_| Ok(Vec::new()),
    ))
    .unwrap();
    let owed = Owed::new(
        kernel.clone(),
        &a,
        "provider:fake".into(),
        json!({"purpose": "test"}),
    );
    let w = World {
        _dir: dir,
        clock,
        kernel,
        exec: e.id,
        call: a.correlation_id,
        _turn: turn,
    };
    (w, owed)
}

/// The cancel's own step for a call it marked that has no task of its own, as `terminate_all` takes it: a call
/// still open is held as unknown (`cancel_unsupported`); a settled one is left alone.
fn cancel_step(k: &Kernel, to_kill: &[String]) {
    for corr in to_kill {
        let open = matches!(k.action(corr), Ok(Some(a)) if !a.state.is_settled());
        if open {
            k.cancel_unsupported(corr, "no backend stop").unwrap();
        }
    }
}

fn unknown(call: &str) -> Completion {
    Completion {
        correlation_id: call.into(),
        outcome: Outcome::Unknown,
        result_ref: None,
        external_op_id: None,
        started_at_ms: NOW,
        finished_at_ms: NOW,
        producer: "provider:fake".into(),
        signature: None,
        cost_micros: None,
        detail: Some(json!({"class": "timeout"})),
    }
}

/// The held-once books: the call's reservation left the open ones and is held as unknown once; nothing was
/// spent; the day counts the reservation once, and holds it no more.
fn held_once(w: &World, before: &Books, after: &Books) {
    let a = w.kernel.action(&w.call).unwrap().unwrap();
    let r = a.reservation_id.expect("the call reserved");
    assert_eq!(
        before.reservations.get(&r),
        Some(&RESERVE),
        "the call's reservation open: {before:?}"
    );
    assert!(
        after.reservations.is_empty(),
        "no reservation left open: {after:?}"
    );
    assert_eq!(after.spent, before.spent, "nothing spent");
    assert_eq!(
        after.unknown,
        before.unknown + RESERVE,
        "held as unknown, once"
    );
    assert_eq!(
        after.day_spent,
        before.day_spent + RESERVE,
        "the day counts the call once: {before:?} -> {after:?}"
    );
    assert_eq!(
        after.day_held,
        before.day_held - RESERVE,
        "the day's hold let go"
    );
}

/// The review's interleaving: a drop once the send may have begun (a deadline or a `/stop` aborting the tool)
/// starts while no cancel has marked the call; an execution cancel then marks it; the drop's write and the
/// cancel's own step follow. The drop is stopped by the clock at its first reading, after it has begun and
/// before it writes: on code that reads the cancel's mark there and writes later, the day counts the call twice.
#[test]
fn a_cancel_that_marks_the_call_while_a_drop_decides_is_counted_once_on_the_day() {
    let (w, owed) = world();
    owed.sending();
    let before = books(&w.kernel, &w.exec);
    assert_eq!(
        before.day_held, RESERVE,
        "the day holds the call: {before:?}"
    );

    let barrier = Arc::new(Barrier::new(2));
    let (tx, rx) = std::sync::mpsc::channel();
    let gate = barrier.clone();
    let clock = w.clock.clone();
    let dropper = std::thread::spawn(move || {
        *clock.stop.lock().unwrap() = Some((std::thread::current().id(), gate));
        tx.send(()).unwrap();
        drop(owed);
    });
    rx.recv().unwrap();
    barrier.wait(); // the drop has begun, and has not written
    let to_kill = w.kernel.cancel_execution(&w.exec, "test").unwrap();
    assert_eq!(to_kill, vec![w.call.clone()], "the cancel marks the call");
    barrier.wait(); // let the drop go on
    dropper.join().unwrap();
    cancel_step(&w.kernel, &to_kill);

    let a = w.kernel.action(&w.call).unwrap().unwrap();
    assert_eq!(
        a.state,
        ActionState::Cancelled,
        "the cancel settled it: {a:?}"
    );
    held_once(&w, &before, &books(&w.kernel, &w.exec));
}

/// The call's own unknown settlement (a timeout) after a cancel marked the call and before the cancel's step:
/// the cancel's step holds it, so the settlement writes nothing, and the day counts it once.
#[test]
fn an_unknown_settlement_after_a_cancel_marked_the_call_is_left_to_the_cancel() {
    let (w, owed) = world();
    owed.sending();
    let before = books(&w.kernel, &w.exec);
    let to_kill = w.kernel.cancel_execution(&w.exec, "test").unwrap();
    owed.settle(&unknown(&w.call), Vec::new()).unwrap();
    drop(owed);
    cancel_step(&w.kernel, &to_kill);
    held_once(&w, &before, &books(&w.kernel, &w.exec));
}

/// A known cost after a cancel marked the call still lands, at its price, once; the cancel's step then finds
/// the call settled and leaves it.
#[test]
fn a_known_cost_after_a_cancel_marked_the_call_lands_once() {
    let (w, owed) = world();
    owed.sending();
    let before = books(&w.kernel, &w.exec);
    let to_kill = w.kernel.cancel_execution(&w.exec, "test").unwrap();
    let c = Completion {
        outcome: Outcome::Succeeded,
        cost_micros: Some(12_345),
        ..unknown(&w.call)
    };
    owed.settle(&c, Vec::new()).unwrap();
    drop(owed);
    cancel_step(&w.kernel, &to_kill);
    let a = w.kernel.action(&w.call).unwrap().unwrap();
    assert_eq!(a.state, ActionState::Succeeded, "{a:?}");
    let after = books(&w.kernel, &w.exec);
    assert!(after.reservations.is_empty(), "{after:?}");
    assert_eq!(after.spent, before.spent + 12_345, "spent at its price");
    assert_eq!(after.unknown, before.unknown, "nothing held as unknown");
    assert_eq!(
        after.day_spent,
        before.day_spent + 12_345,
        "the day counts its price once"
    );
    assert_eq!(after.day_held, before.day_held - RESERVE);
}

/// An owner dropped on a thread that holds no lock settles before the drop returns, on that thread: every clock
/// reading of the drop, the settlement's included, is the dropping thread's, so no thread was spawned for it.
#[test]
fn a_drop_on_a_thread_holding_no_lock_settles_on_that_thread_before_it_returns() {
    let (w, owed) = world();
    let before = books(&w.kernel, &w.exec);
    w.clock.readers.lock().unwrap().clear();
    drop(owed);
    let a = w.kernel.action(&w.call).unwrap().unwrap();
    assert_eq!(a.state, ActionState::Failed, "settled as never sent: {a:?}");
    let after = books(&w.kernel, &w.exec);
    assert!(after.reservations.is_empty(), "{after:?}");
    assert_eq!(after.day_held, before.day_held - RESERVE);
    let me = std::thread::current().id();
    let readers = w.clock.readers.lock().unwrap().clone();
    assert!(!readers.is_empty(), "the drop read the clock");
    let elsewhere: Vec<_> = readers.iter().filter(|(t, _)| *t != me).collect();
    assert!(
        elsewhere.is_empty(),
        "the drop read the clock on another thread: {elsewhere:?}"
    );
}

/// What a drop inside a frame must be: back at once (bounded well under a second; nothing here sleeps), with no
/// panic, and nothing written: the call is still dispatched, its books as they were.
fn dropped_in_a_frame_writes_nothing(w: &World, frame_on: &str, owed: Owed) -> (Books, Action) {
    let (before, call) = (
        books(&w.kernel, &w.exec),
        w.kernel.action(&w.call).unwrap().unwrap(),
    );
    let mut took = Duration::ZERO;
    (w.kernel.frame(&[frame_on], |_k| {
        let t0 = Instant::now();
        drop(owed);
        took = t0.elapsed();
        Ok(())
    }))
    .unwrap();
    assert!(took < Duration::from_millis(250), "the drop took {took:?}");
    let now = w.kernel.action(&w.call).unwrap().unwrap();
    assert_eq!(
        now.state,
        ActionState::Dispatched,
        "left as it was: {now:?}"
    );
    assert_eq!(now, call, "nothing written to the call");
    assert_eq!(
        books(&w.kernel, &w.exec),
        before,
        "nothing written to the books"
    );
    (before, call)
}

/// The reconcile, once the call's deadline has passed, holds a call left dispatched as unknown, once.
fn the_reconcile_holds_it(w: &World, before: &Books, call: &Action) {
    let ahead = call.deadline_at_ms.saturating_sub(NOW) + 1;
    w.clock.ahead.store(ahead, Ordering::SeqCst);
    let rep = w.kernel.reconcile(&NoEvidence).unwrap();
    assert_eq!(rep.marked_unknown, vec![w.call.clone()], "{rep:?}");
    let a = w.kernel.action(&w.call).unwrap().unwrap();
    assert_eq!(a.state, ActionState::OutcomeUnknown, "{a:?}");
    held_once(w, before, &books(&w.kernel, &w.exec));
}

/// An owner dropped inside a frame on ANOTHER execution (a thread holding a lock): it does not take a lock
/// inside a lock, writes nothing and returns at once; the reconcile settles the call.
#[test]
fn a_drop_inside_another_executions_frame_is_left_to_the_reconcile() {
    let (w, owed) = world();
    owed.sending();
    let other = (w.kernel.open_execution(
        "ses_other",
        SessionKind::Conversation,
        Authority {
            principal: "owner".into(),
            delegated_by: None,
            ceilings: BTreeMap::new(),
        },
        None,
        None,
    ))
    .unwrap();
    let (before, call) = dropped_in_a_frame_writes_nothing(&w, &other.id, owed);
    the_reconcile_holds_it(&w, &before, &call);
}

/// An owner dropped inside a frame on the call's OWN execution: the same, with no wait for its own lock.
#[test]
fn a_drop_inside_its_own_executions_frame_is_left_to_the_reconcile() {
    let (w, owed) = world();
    let exec = w.exec.clone();
    let (before, call) = dropped_in_a_frame_writes_nothing(&w, &exec, owed);
    the_reconcile_holds_it(&w, &before, &call);
}
