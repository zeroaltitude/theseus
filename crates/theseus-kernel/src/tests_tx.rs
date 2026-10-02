//! The kernel transaction (theseus-0owd, Review 2's C6): several transitions
//! staged and committed as one frame, observed once; nothing written when the
//! closure fails; a nested part that fails takes back only its own; reads that
//! see what was staged; no lock inside, and the guards that keep it so; a turn
//! ended inside freed once its frame is written; and the locks in id order.

use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use theseus_store::kinds;

use crate::kernel::*;
use crate::tests::*;
use crate::types::*;

/// Every frame the kernel commits from now on, as its records' kinds.
fn frames(w: &World) -> Arc<Mutex<Vec<Vec<u16>>>> {
    let seen: Arc<Mutex<Vec<Vec<u16>>>> = Arc::default();
    let into = seen.clone();
    assert!(w.kernel.observe(Arc::new(move |c: Committed<'_>| {
        into.lock()
            .unwrap()
            .push(c.records.iter().map(|r| r.kind).collect());
    })));
    seen
}

/// A conversation's execution, waiting on its first input.
fn waiting(w: &World) -> Execution {
    w.kernel
        .open_execution(
            &new_id("ses"),
            SessionKind::Conversation,
            auth(),
            Some(100_000),
            None,
        )
        .unwrap()
}

fn panic_text(p: Box<dyn std::any::Any + Send>) -> String {
    p.downcast_ref::<String>()
        .cloned()
        .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_default()
}

/// Two transitions, one frame: the input's wake and the turn's admission,
/// the second reading what the first staged, observed once. Nothing reaches
/// the store before the closure returns: a reader outside it sees the
/// execution as it was.
#[test]
fn a_transaction_commits_its_transitions_as_one_observed_frame() {
    let w = world();
    let e = waiting(&w);
    let seen = frames(&w);
    let before = w.kernel.store().last_position();
    let g = w
        .kernel
        .frame(&[&e.id], |k| {
            assert_eq!(k.wake_input(&e.id)?.state, ExecState::Queued);
            assert_eq!(
                k.execution(&e.id)?.unwrap().state,
                ExecState::Queued,
                "the view reads what it staged"
            );
            assert_eq!(
                w.kernel.execution(&e.id)?.unwrap().state,
                ExecState::Waiting,
                "nothing is written yet"
            );
            assert_eq!(w.kernel.store().last_position(), before);
            k.admit(&e.id)
        })
        .unwrap();
    assert_eq!(
        *seen.lock().unwrap(),
        vec![vec![
            kinds::EXECUTION,
            kinds::LEDGER,
            kinds::EXECUTION,
            kinds::LEDGER
        ]]
    );
    assert_eq!(w.kernel.store().last_position(), before + 4);
    assert_eq!(
        w.kernel.execution(&e.id).unwrap().unwrap().state,
        ExecState::Running
    );
    assert!(w.kernel.is_held(&e.id));
    drop(g);
    assert!(!w.kernel.is_held(&e.id));
}

/// A closure that fails writes nothing: what its transitions staged is
/// dropped, the store and the observer see nothing, and a turn it admitted
/// is free again. (The invariant the revert proof plants against.)
#[test]
fn a_transaction_whose_closure_fails_writes_nothing() {
    let w = world();
    let e = waiting(&w);
    let seen = frames(&w);
    let before = w.kernel.store().last_position();
    let err = w
        .kernel
        .frame(&[&e.id], |k| -> anyhow::Result<()> {
            k.wake_input(&e.id)?;
            let _g = k.admit(&e.id)?;
            anyhow::bail!("the closure fails after two transitions")
        })
        .unwrap_err();
    assert!(format!("{err:#}").contains("the closure fails"), "{err:#}");
    assert!(seen.lock().unwrap().is_empty(), "nothing observed");
    assert_eq!(w.kernel.store().last_position(), before, "nothing written");
    let x = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!((x.state, x.wake), (ExecState::Waiting, Some(Wake::Input)));
    assert!(!w.kernel.is_held(&e.id), "the turn it admitted is free");
    assert_eq!(w.kernel.exec_locks().held(), 0);
    // The execution takes the same two transitions as if nothing had run.
    drop(w.kernel.admit_input(&e.id).unwrap());
    assert_eq!(seen.lock().unwrap().len(), 1);
}

/// A transaction inside another joins it: a part that succeeds commits with
/// the outer frame, and one that fails takes back only what it staged.
#[test]
fn a_nested_transaction_joins_and_one_that_fails_takes_back_only_its_part() {
    let w = world();
    let e = waiting(&w);
    let seen = frames(&w);
    let failed = w
        .kernel
        .frame(&[&e.id], |k| {
            k.wake_input(&e.id)?;
            let part = k.frame(&[&e.id], |k| -> anyhow::Result<()> {
                let _g = k.admit(&e.id)?;
                anyhow::bail!("this part fails after its admission")
            });
            Ok(part.is_err())
        })
        .unwrap();
    assert!(failed);
    assert_eq!(
        *seen.lock().unwrap(),
        vec![vec![kinds::EXECUTION, kinds::LEDGER]],
        "the wake alone"
    );
    assert_eq!(
        w.kernel.execution(&e.id).unwrap().unwrap().state,
        ExecState::Queued
    );
    assert!(!w.kernel.is_held(&e.id));
    let g = w
        .kernel
        .frame(&[&e.id], |k| k.frame(&[&e.id], |k| k.admit(&e.id)))
        .unwrap();
    assert_eq!(seen.lock().unwrap().len(), 2, "the joined part, one frame");
    drop(g);
}

/// Reads inside a transaction, by key and by kind: each key's newest record,
/// staged or stored, once each, in the store's key order, an execution opened
/// inside included.
#[test]
fn a_transaction_reads_what_it_staged_by_key_and_by_kind() {
    let w = world();
    let es: Vec<Execution> = (0..5).map(|_| waiting(&w)).collect();
    let ids: Vec<&str> = es.iter().map(|e| e.id.as_str()).collect();
    let opened = w
        .kernel
        .frame(&ids, |k| {
            k.wake_input(&es[3].id)?;
            k.wake_input(&es[0].id)?;
            k.wake_input(&es[3].id)?;
            let n = waiting_in(k)?;
            assert_eq!(k.execution(&n.id)?.unwrap().state, ExecState::Waiting);
            let all = k.executions()?;
            let got: Vec<&str> = all.iter().map(|e| e.id.as_str()).collect();
            let mut want: Vec<&str> = ids.iter().copied().chain([n.id.as_str()]).collect();
            want.sort_unstable();
            assert_eq!(got, want, "each once, in key order");
            for e in &all {
                let queued = e.id == es[0].id || e.id == es[3].id;
                assert_eq!(e.state == ExecState::Queued, queued, "{}", e.id);
            }
            Ok(n)
        })
        .unwrap();
    assert_eq!(w.kernel.executions().unwrap().len(), 6);
    assert_eq!(
        w.kernel.execution(&opened.id).unwrap().unwrap().state,
        ExecState::Waiting
    );
}

fn waiting_in(k: &Kernel) -> anyhow::Result<Execution> {
    k.open_execution(
        &new_id("ses"),
        SessionKind::Conversation,
        auth(),
        None,
        None,
    )
}

/// No lock inside: a transition on the view takes none. One called on the
/// kernel itself, which locks, panics as a second lock of the execution on
/// this thread does, and one of an execution the transaction did not name
/// panics too. Either way nothing is written, and the locks are free again.
#[test]
fn inside_a_transaction_only_its_view_transitions_and_only_what_it_named() {
    let w = world();
    let e = waiting(&w);
    let other = waiting(&w);
    let before = w.kernel.store().last_position();
    let twice = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        w.kernel.frame(&[&e.id], |_| w.kernel.wake_input(&e.id))
    }));
    let msg = panic_text(twice.expect_err("the kernel's own transition locks again"));
    assert!(msg.contains("locked twice on one thread"), "{msg}");
    let unnamed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        w.kernel.frame(&[&e.id], |k| k.wake_input(&other.id))
    }));
    let msg = panic_text(unnamed.expect_err("an execution it did not name"));
    assert!(msg.contains("is not locked by this transaction"), "{msg}");
    assert_eq!(w.kernel.store().last_position(), before, "nothing written");
    assert_eq!(w.kernel.exec_locks().held(), 0);
    w.kernel.wake_input(&e.id).unwrap();
    w.kernel.wake_input(&other.id).unwrap();
}

/// A turn ended inside a transaction is freed once the frame is written:
/// until then it is still held, so no second turn can start on it.
#[test]
fn a_turn_ended_inside_is_freed_after_its_frame() {
    let w = world();
    let (_, e, g) = running(&w);
    w.kernel
        .frame(&[&e.id], |k| {
            k.end_turn(g, TurnEnd::Requeue)?;
            assert!(w.kernel.is_held(&e.id), "held until the frame is written");
            Ok(())
        })
        .unwrap();
    assert!(!w.kernel.is_held(&e.id));
    assert_eq!(
        w.kernel.execution(&e.id).unwrap().unwrap().state,
        ExecState::Queued
    );
    drop(w.kernel.admit(&e.id).unwrap());
}

/// Two transactions that name the same two executions in opposite orders,
/// on two threads, 2,000 times each: K1's id order, so neither deadlocks.
#[test]
fn two_transactions_naming_two_executions_in_opposite_orders_never_deadlock() {
    let w = world();
    let (a, b) = (waiting(&w), waiting(&w));
    let k = &w.kernel;
    let (tx, rx) = mpsc::channel();
    std::thread::scope(|s| {
        for (x, y) in [(&a.id, &b.id), (&b.id, &a.id)] {
            let tx = tx.clone();
            s.spawn(move || {
                for _ in 0..2_000 {
                    k.frame(&[x, y], |k| {
                        k.execution(x)?;
                        k.execution(y)?;
                        Ok(())
                    })
                    .unwrap();
                }
                tx.send(()).unwrap();
            });
        }
        for _ in 0..2 {
            rx.recv_timeout(Duration::from_secs(20))
                .expect("deadlock: two transactions named the same two in opposite orders");
        }
    });
    assert_eq!(k.exec_locks().held(), 0);
}
