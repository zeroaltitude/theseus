//! Kernel scenarios (§8 standing set, the ones the kernel alone can express).
//! Every test runs against a real `WalStore` in a temp dir under a virtual
//! clock; "crash" means drop the kernel and reopen the store.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::json;
use tempfile::TempDir;
use theseus_store::{kinds, NewRecord, Store, WalConfig, WalStore};

use crate::clock::{Clock, VirtualClock};
use crate::gate::Proposal;
use crate::kernel::*;
use crate::spool::Spool;
use crate::types::*;

pub(super) struct World {
    pub(super) dir: TempDir,
    pub(super) clock: Arc<VirtualClock>,
    pub(super) kernel: Kernel,
    pub(super) spool: Spool,
}

fn open_store(dir: &std::path::Path) -> Arc<dyn Store> {
    Arc::new(
        WalStore::open(&dir.join("store"), WalConfig::default())
            .unwrap()
            .with_checkpoint_every(0),
    )
}

pub(super) fn world() -> World {
    world_with(KernelConfig::default())
}

pub(super) fn world_with(cfg: KernelConfig) -> World {
    let dir = tempfile::tempdir().unwrap();
    let clock = VirtualClock::new(1_000_000);
    let spool = Spool::open(&dir.path().join("spool")).unwrap();
    let kernel = Kernel::new(open_store(dir.path()), clock.clone(), cfg);
    kernel.startup(Some(&spool), &NoEvidence).unwrap();
    World {
        dir,
        clock,
        kernel,
        spool,
    }
}

/// Crash: drop the kernel, reopen the store, run startup with spool evidence.
pub(super) fn crash(w: World, cfg: KernelConfig) -> (World, StartupReport) {
    let World {
        dir,
        clock,
        spool,
        kernel,
    } = w;
    drop(kernel); // the old process is gone; its store handle with it
    let kernel = Kernel::new(open_store(dir.path()), clock.clone(), cfg);
    let ev = crate::job::WrapperEvidence {
        spool: spool.clone(),
    };
    let rep = kernel.startup(Some(&spool), &ev).unwrap();
    (
        World {
            dir,
            clock,
            kernel,
            spool,
        },
        rep,
    )
}

pub(super) fn auth() -> Authority {
    Authority {
        principal: "eddie".into(),
        delegated_by: None,
        ceilings: BTreeMap::from([
            ("shell".into(), "l1".into()),
            ("tools".into(), "fs,text".into()),
        ]),
    }
}

pub(super) fn proposal(tool: &str) -> Proposal {
    Proposal {
        tool: tool.into(),
        args: json!({"a": 1}),
        resource: None,
        policy_context: json!({"binding": 1}),
    }
}

pub(super) fn completion(id: &str, outcome: Outcome, usage: Option<u64>) -> Completion {
    Completion {
        correlation_id: id.into(),
        outcome,
        result_ref: Some("node_x".into()),
        external_op_id: None,
        started_at_ms: 1,
        finished_at_ms: 2,
        producer: "test".into(),
        signature: None,
        cost_micros: usage,
        detail: None,
    }
}

/// Open a conversation's execution, wake it with input, take a turn: the
/// common prelude. Returns the session id with the execution and its guard.
pub(super) fn running(w: &World) -> (SessionId, Execution, TurnGuard) {
    let e = w
        .kernel
        .open_execution(
            &new_id("ses"),
            SessionKind::Conversation,
            auth(),
            Some(100_000),
            None,
        )
        .unwrap();
    assert_eq!(e.state, ExecState::Waiting);
    assert_eq!(e.wake, Some(Wake::Input));
    let e = w.kernel.wake_input(&e.id).unwrap();
    assert_eq!(e.state, ExecState::Queued);
    let g = w.kernel.admit(&e.id).unwrap();
    let e = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(e.state, ExecState::Running);
    (e.session_id.clone(), e, g)
}

/// plan → authorize → dispatch, returning the dispatched action.
pub(super) fn dispatched(w: &World, g: &TurnGuard, tool: &str, reserve: u64) -> Action {
    let p = proposal(tool);
    let a = w
        .kernel
        .plan_action(g, &p, RetryClass::SafeToRepeat, Some(60_000), reserve)
        .unwrap();
    assert_eq!(a.state, ActionState::Planned);
    let a = w.kernel.authorize(&a.correlation_id, &p, None).unwrap();
    assert_eq!(a.state, ActionState::Authorized);
    let a = w.kernel.dispatch(&a.correlation_id, None).unwrap();
    assert_eq!(a.state, ActionState::Dispatched);
    a
}

#[test]
fn full_lifecycle_one_action_one_turn() {
    let w = world();
    let (s, e, g) = running(&w);
    let a = dispatched(&w, &g, "fs.read", 100);
    let e2 = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(e2.outstanding, vec![a.correlation_id.clone()]);
    assert_eq!(e2.budget.reserved_micros, 100);

    // The turn parks on the action.
    let e3 = w
        .kernel
        .end_turn(
            g,
            TurnEnd::Wait {
                wake: Wake::Actions {
                    correlation_ids: vec![a.correlation_id.clone()],
                },
            },
        )
        .unwrap();
    assert_eq!(e3.state, ExecState::Waiting);

    // Completion settles and continues in one frame.
    let before = w.kernel.store().last_position();
    let acc = w
        .kernel
        .accept_completion(&completion(&a.correlation_id, Outcome::Succeeded, Some(40)))
        .unwrap();
    assert!(matches!(
        acc,
        Accepted::Settled {
            execution_state: ExecState::Queued,
            ..
        }
    ));
    let frame_len = w.kernel.store().last_position() - before;
    assert_eq!(
        frame_len, 4,
        "completion + action + execution + ledger, one frame"
    );
    let e4 = w.kernel.execution(&e.id).unwrap().unwrap();
    assert!(e4.outstanding.is_empty());
    assert_eq!(e4.queued_results, vec![a.correlation_id]);
    assert_eq!(e4.budget.spent_micros, 40);
    assert_eq!(e4.budget.reserved_micros, 0);

    // Next turn consumes the result and completes.
    let g = w.kernel.admit(&e.id).unwrap();
    let results = w.kernel.take_results(&g).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].state, ActionState::Succeeded);
    assert_eq!(results[0].result_ref.as_deref(), Some("node_x"));
    let e5 = w
        .kernel
        .end_turn(
            g,
            TurnEnd::Complete {
                reason: "done".into(),
            },
        )
        .unwrap();
    assert_eq!(e5.state, ExecState::Complete);
    assert_eq!(e5.turns, 2);

    // The session tail holds every record in scope, in order.
    let tail = w.kernel.store().scan_scope(&s, 0, 100).unwrap();
    assert!(tail.len() >= 12, "{}", tail.len());
    assert!(tail.windows(2).all(|p| p[0].position < p[1].position));
    let stats = w.kernel.stats().unwrap();
    assert_eq!(stats.executions_by_state["complete"], 1);
    assert_eq!(stats.actions_by_state["succeeded"], 1);
}

/// `plan_and_dispatch` (theseus-qa0): planned, authorized, and dispatched in
/// one frame, each transition with its record and row, in that order, and the
/// caller's records after the plan's; the end state `dispatched` leaves; over
/// budget, nothing written. A view commits through its own handle and shares
/// the turn locks.
#[test]
fn plan_and_dispatch_is_three_transitions_in_one_frame() {
    let w = world();
    let (_, e, g) = running(&w);
    let frames = || w.kernel.store().stats().unwrap().frames_appended;
    let (f0, p0) = (frames(), w.kernel.store().last_position());
    let p = proposal("provider.messages");
    let view = w.kernel.view(w.kernel.store().clone());
    let a = view
        .plan_and_dispatch(&g, &p, RetryClass::SafeToRepeat, Some(60_000), 100, |a| {
            Ok(vec![NewRecord::json(
                kinds::META,
                Some(&format!("extra:{}", a.correlation_id)),
                &json!({"n": 1}),
            )?])
        })
        .unwrap();
    assert_eq!(frames() - f0, 1, "one frame");
    let labels: Vec<String> = w
        .kernel
        .store()
        .scan(p0 + 1, None, 100)
        .unwrap()
        .iter()
        .map(|r| match r.kind {
            kinds::LEDGER => r.decode::<LedgerRow>().unwrap().kind,
            k => kinds::name(k).to_string(),
        })
        .collect();
    assert_eq!(
        labels,
        [
            "execution",
            "action",
            "action.planned",
            "meta",
            "action",
            "action.authorized",
            "action",
            "execution",
            "action.dispatched"
        ]
    );
    assert_eq!(a.state, ActionState::Dispatched);
    assert!(a.authorized_at_ms.is_some() && a.dispatched_at_ms.is_some());
    assert_eq!(w.kernel.action(&a.correlation_id).unwrap(), Some(a.clone()));
    let e2 = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(e2.outstanding, vec![a.correlation_id]);
    assert_eq!(e2.budget.reserved_micros, 100);
    assert!(view.is_held(&e.id), "the view shares the turn locks");
    assert!(view.is_accepting(), "and the startup phase");
    // Over budget: the refusal writes nothing.
    let (f1, p1) = (frames(), w.kernel.store().last_position());
    let err = view
        .plan_and_dispatch(&g, &p, RetryClass::SafeToRepeat, None, 1_000_000, |_| {
            Ok(vec![])
        })
        .unwrap_err();
    assert!(matches!(
        err.downcast_ref::<KernelError>(),
        Some(KernelError::OverBudget { .. })
    ));
    assert_eq!((frames(), w.kernel.store().last_position()), (f1, p1));
    drop(g);
}

#[test]
fn duplicate_completion_is_a_logged_noop_and_stray_is_quarantined() {
    let w = world();
    let (_, e, g) = running(&w);
    let a = dispatched(&w, &g, "fs.read", 0);
    let c = completion(&a.correlation_id, Outcome::Succeeded, None);
    assert!(matches!(
        w.kernel.accept_completion(&c).unwrap(),
        Accepted::Settled { .. }
    ));
    assert!(matches!(
        w.kernel.accept_completion(&c).unwrap(),
        Accepted::DuplicateNoop { .. }
    ));
    let a2 = w.kernel.action(&a.correlation_id).unwrap().unwrap();
    assert_eq!(a2.completions_seen, 2);
    let e2 = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(e2.queued_results.len(), 1, "delivered once");

    let stray = completion("act_nobody", Outcome::Succeeded, None);
    assert!(matches!(
        w.kernel.accept_completion(&stray).unwrap(),
        Accepted::Quarantined { .. }
    ));
    assert_eq!(w.kernel.quarantined().unwrap().len(), 1);
    // Still one execution, untouched.
    assert_eq!(w.kernel.executions().unwrap().len(), 1);
    drop(g);
}

#[test]
fn crash_mid_turn_requeues_as_interrupted_and_nothing_else_changes() {
    let w = world();
    let (_, e, g) = running(&w);
    let a = dispatched(&w, &g, "proc.run", 10);
    std::mem::forget(g); // the process dies holding the turn
    let (w, rep) = crash(w, KernelConfig::default());
    assert_eq!(rep.requeued_interrupted, vec![e.id.clone()]);
    assert_eq!(rep.steps.len(), 5);
    let e2 = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(e2.state, ExecState::Queued);
    assert_eq!(e2.interrupted, 1);
    assert_eq!(e2.outstanding, vec![a.correlation_id.clone()]);
    let a2 = w.kernel.action(&a.correlation_id).unwrap().unwrap();
    assert_eq!(
        a2.state,
        ActionState::Dispatched,
        "dispatched survives; the reconciler owns it"
    );
    // Second startup is a no-op.
    let (w, rep2) = crash(w, KernelConfig::default());
    assert!(rep2.requeued_interrupted.is_empty());
    assert_eq!(w.kernel.execution(&e.id).unwrap().unwrap().interrupted, 1);
}

#[test]
fn completion_during_restart_lands_in_spool_and_startup_drains_it() {
    let w = world();
    let (_, e, g) = running(&w);
    let a = dispatched(&w, &g, "proc.run", 0);
    w.kernel
        .end_turn(
            g,
            TurnEnd::Wait {
                wake: Wake::Actions {
                    correlation_ids: vec![a.correlation_id.clone()],
                },
            },
        )
        .unwrap();
    // Harness is down; the wrapper finishes and spools.
    w.spool
        .write(&completion(&a.correlation_id, Outcome::Succeeded, None))
        .unwrap();
    let (w, rep) = crash(w, KernelConfig::default());
    assert_eq!(rep.spool_drained, 1);
    assert!(
        !w.spool.has_completion(&a.correlation_id),
        "removed after the frame"
    );
    let e2 = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(e2.state, ExecState::Queued);
    assert_eq!(e2.queued_results, vec![a.correlation_id.clone()]);
    // A second copy still in the spool would be a duplicate no-op.
    w.spool
        .write(&completion(&a.correlation_id, Outcome::Succeeded, None))
        .unwrap();
    let (w, rep) = crash(w, KernelConfig::default());
    assert_eq!(rep.spool_drained, 1);
    assert_eq!(
        w.kernel
            .execution(&e.id)
            .unwrap()
            .unwrap()
            .queued_results
            .len(),
        1
    );
}

#[test]
fn crash_inside_each_startup_step_recovers_on_the_next_startup() {
    for fault in 1..=5u8 {
        let w = world();
        let (_, e, g) = running(&w);
        let a = dispatched(&w, &g, "proc.run", 0);
        std::mem::forget(g);
        w.spool
            .write(&completion(&a.correlation_id, Outcome::Succeeded, None))
            .unwrap();
        // Crash, then a startup that dies after `fault`.
        let World {
            dir,
            clock,
            spool,
            kernel,
        } = w;
        drop(kernel);
        let k = Kernel::new(
            open_store(dir.path()),
            clock.clone(),
            KernelConfig {
                fault_after_startup_step: Some(fault),
                ..Default::default()
            },
        );
        let ev = crate::job::WrapperEvidence {
            spool: spool.clone(),
        };
        let err = k.startup(Some(&spool), &ev).expect_err("injected");
        assert!(err.to_string().contains(&format!("step {fault}")), "{err}");
        assert!(!k.is_accepting() || fault == 5);
        drop(k);
        // A clean startup finishes the job regardless of where the last one died.
        let (w, _rep) = {
            let kernel = Kernel::new(
                open_store(dir.path()),
                clock.clone(),
                KernelConfig::default(),
            );
            let ev = crate::job::WrapperEvidence {
                spool: spool.clone(),
            };
            let rep = kernel.startup(Some(&spool), &ev).unwrap();
            (
                World {
                    dir,
                    clock,
                    kernel,
                    spool,
                },
                rep,
            )
        };
        let e2 = w.kernel.execution(&e.id).unwrap().unwrap();
        assert_eq!(e2.state, ExecState::Queued, "fault after step {fault}");
        assert_eq!(
            e2.interrupted, 1,
            "requeue counted exactly once (fault {fault})"
        );
        assert_eq!(
            e2.queued_results,
            vec![a.correlation_id.clone()],
            "fault {fault}"
        );
        assert!(e2.outstanding.is_empty());
        assert_eq!(
            w.kernel.action(&a.correlation_id).unwrap().unwrap().state,
            ActionState::Succeeded
        );
        assert!(!w.spool.has_completion(&a.correlation_id));
        assert!(w.kernel.is_accepting());
    }
}

#[test]
fn cancel_of_dispatched_job_then_late_completion_does_not_revive() {
    let w = world();
    let (_, e, g) = running(&w);
    let a = dispatched(&w, &g, "proc.run", 50);
    let to_kill = w.kernel.cancel_execution(&e.id, "eddie").unwrap();
    assert_eq!(to_kill, vec![a.correlation_id.clone()]);
    let e2 = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(e2.state, ExecState::Cancelled);
    // The turn holder learns of it when it ends the turn: cancel wins.
    let e3 = w
        .kernel
        .end_turn(
            g,
            TurnEnd::Complete {
                reason: "nope".into(),
            },
        )
        .unwrap();
    assert_eq!(e3.state, ExecState::Cancelled);
    let a2 = w.kernel.cancel_acknowledged(&a.correlation_id).unwrap();
    assert_eq!(a2.cancel, Some(CancelState::Acknowledged));
    assert_eq!(a2.state, ActionState::Dispatched);
    let a3 = w.kernel.cancel_verified(&a.correlation_id).unwrap();
    assert_eq!(a3.state, ActionState::Cancelled);
    let e4 = w.kernel.execution(&e.id).unwrap().unwrap();
    assert!(e4.outstanding.is_empty());
    assert_eq!(e4.budget.reserved_micros, 0);
    // The job finished anyway, late.
    let acc = w
        .kernel
        .accept_completion(&completion(&a.correlation_id, Outcome::Succeeded, None))
        .unwrap();
    assert!(matches!(acc, Accepted::LateAfterCancel { .. }));
    let e5 = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(e5.state, ExecState::Cancelled);
    assert!(e5.queued_results.is_empty());
    // Cancel is idempotent and admission never touches a cancelled execution.
    assert!(w
        .kernel
        .cancel_execution(&e.id, "eddie")
        .unwrap()
        .is_empty());
    assert!(w.kernel.admit(&e.id).is_err());
}

/// A cancel ends what its execution planned and never sent, in its own
/// frame (theseus-w98). A call waiting for the operator, a call planned, and
/// one authorized but not dispatched each settle `Cancelled`, their
/// resolution "the execution was cancelled by eddie", their reservations
/// released, with an `action.cancelled` row each. The caller gets them, told
/// that a turn held the execution, and only the dispatched job is left to
/// kill. Nothing waits for the operator now. The turn that planned them hears
/// the cancel when it goes on, as before: its authorize and its dispatch are
/// refused as `NotRunnable`, and a confirm finds nothing waiting.
#[test]
fn a_cancel_ends_what_its_execution_planned_and_never_sent() {
    let w = world();
    let (sid, e, g) = running(&w);
    let ask = proposal("fs.write");
    let waiting = w
        .kernel
        .plan_confirm_with(&g, &ask, RetryClass::NonRepeatable, None, |_| Ok(vec![]))
        .unwrap();
    let read = proposal("fs.read");
    let planned = w
        .kernel
        .plan_action(&g, &read, RetryClass::SafeToRepeat, None, 300)
        .unwrap();
    let run = proposal("proc.run");
    let authorized = w
        .kernel
        .plan_action(&g, &run, RetryClass::SafeToRepeat, None, 200)
        .unwrap();
    w.kernel
        .authorize(&authorized.correlation_id, &run, None)
        .unwrap();
    let job = dispatched(&w, &g, "proc.run", 50);
    // The call between its plan and its authorization counts as waiting
    // too: `awaits_confirm` cannot tell it from one the operator was asked.
    // The product never leaves a call there (`plan_and_dispatch` is one
    // frame); this one is planned through the kernel directly.
    assert_eq!(w.kernel.pending_confirms().unwrap().len(), 2);
    let held = |w: &World| {
        w.kernel
            .execution(&e.id)
            .unwrap()
            .unwrap()
            .budget
            .reserved_micros
    };
    assert_eq!(held(&w), 550);

    let mut seen = None;
    let cancel = w
        .kernel
        .cancel_execution_with(&e.id, "eddie", |end| {
            seen = Some((end.turn_running, end.not_run.len(), end.execution.state));
            Ok(vec![])
        })
        .unwrap();
    assert_eq!(seen, Some((true, 3, ExecState::Cancelled)));
    assert_eq!(cancel.to_kill, vec![job.correlation_id.clone()]);
    let mut ended: Vec<_> = cancel
        .not_run
        .iter()
        .map(|a| a.correlation_id.clone())
        .collect();
    let mut want = vec![
        waiting.correlation_id.clone(),
        planned.correlation_id.clone(),
        authorized.correlation_id.clone(),
    ];
    ended.sort();
    want.sort();
    assert_eq!(ended, want);
    for c in &want {
        let a = w.kernel.action(c).unwrap().unwrap();
        assert_eq!(a.state, ActionState::Cancelled, "{c}");
        assert_eq!(
            a.resolution.as_deref(),
            Some("the execution was cancelled by eddie")
        );
    }
    assert!(w.kernel.pending_confirms().unwrap().is_empty());
    assert_eq!(
        held(&w),
        50,
        "only the dispatched job's reservation is held"
    );
    assert_eq!(rows(&w, &sid, "action.cancelled").len(), 3);
    let row = &rows(&w, &sid, "execution.cancelled")[0];
    assert_eq!(row["not_run"].as_array().unwrap().len(), 3);
    assert_eq!(row["outstanding"], json!([job.correlation_id]));

    // The turn goes on, and hears the cancel as it did before.
    let refused = |r: anyhow::Result<Action>| {
        matches!(
            r.unwrap_err().downcast_ref::<KernelError>(),
            Some(KernelError::NotRunnable {
                state: "cancelled",
                ..
            })
        )
    };
    assert!(refused(w.kernel.authorize(
        &planned.correlation_id,
        &read,
        None
    )));
    assert!(refused(w.kernel.dispatch(&authorized.correlation_id, None)));
    assert!(matches!(
        w.kernel
            .authorize_and_dispatch(&waiting.correlation_id, &ask, None, None)
            .unwrap_err()
            .downcast_ref::<KernelError>(),
        Some(KernelError::NotRunnable { .. })
    ));
    assert!(w
        .kernel
        .bind_confirm(&waiting.correlation_id, "eddie", &ask)
        .is_err());
    // A second cancel finds nothing more to end.
    let again = w
        .kernel
        .cancel_execution_with(&e.id, "eddie", |_| panic!("cancelled once"))
        .unwrap();
    assert!(again.to_kill.is_empty() && again.not_run.is_empty());
    drop(g);
}

/// A turn that ends its execution ends what it planned and never sent
/// (theseus-w98), as a cancel does: a call a crash left planned, and one
/// waiting for the operator, settle `Cancelled` with "the execution ended
/// (complete)", and their reservations are released.
#[test]
fn an_execution_that_ends_ends_what_it_planned_and_never_sent() {
    let w = world();
    let (sid, _e, g) = running(&w);
    let left = w
        .kernel
        .plan_action(
            &g,
            &proposal("fs.read"),
            RetryClass::SafeToRepeat,
            None,
            100,
        )
        .unwrap();
    let asked = w
        .kernel
        .plan_confirm_with(
            &g,
            &proposal("fs.write"),
            RetryClass::NonRepeatable,
            None,
            |_| Ok(vec![]),
        )
        .unwrap();
    let e = w
        .kernel
        .end_turn(
            g,
            TurnEnd::Complete {
                reason: "done".into(),
            },
        )
        .unwrap();
    assert_eq!(e.state, ExecState::Complete);
    for c in [&left.correlation_id, &asked.correlation_id] {
        let a = w.kernel.action(c).unwrap().unwrap();
        assert_eq!(a.state, ActionState::Cancelled, "{c}");
        assert_eq!(
            a.resolution.as_deref(),
            Some("the execution ended (complete)")
        );
    }
    assert_eq!(e.budget.reserved_micros, 0);
    assert!(w.kernel.pending_confirms().unwrap().is_empty());
    assert_eq!(rows(&w, &sid, "action.cancelled").len(), 2);
}

#[test]
fn unknown_then_genuine_success_resolves_and_budget_moves_held_to_spent() {
    let w = world();
    let (_, e, g) = running(&w);
    let a = dispatched(&w, &g, "http.post", 200);
    w.kernel
        .end_turn(
            g,
            TurnEnd::Wait {
                wake: Wake::Actions {
                    correlation_ids: vec![a.correlation_id.clone()],
                },
            },
        )
        .unwrap();
    // Past deadline, no evidence: the reconciler marks it unknown.
    w.clock.advance(61_000);
    let rep = w.kernel.reconcile(&NoEvidence).unwrap();
    assert_eq!(rep.marked_unknown, vec![a.correlation_id.clone()]);
    let a2 = w.kernel.action(&a.correlation_id).unwrap().unwrap();
    assert_eq!(a2.state, ActionState::OutcomeUnknown);
    let e2 = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(
        e2.state,
        ExecState::Queued,
        "unknown is a result the model gets"
    );
    assert_eq!(e2.queued_results, vec![a.correlation_id.clone()]);
    assert_eq!(e2.budget.held_unknown_micros, 200);
    assert_eq!(e2.budget.reserved_micros, 0);
    assert_eq!(e2.budget.spent_micros, 0);
    // Later, authoritative evidence.
    let acc = w
        .kernel
        .accept_completion(&completion(
            &a.correlation_id,
            Outcome::Succeeded,
            Some(120),
        ))
        .unwrap();
    assert_eq!(
        acc,
        Accepted::ResolvedUnknown {
            correlation_id: a.correlation_id.clone(),
            outcome: Outcome::Succeeded
        }
    );
    let a3 = w.kernel.action(&a.correlation_id).unwrap().unwrap();
    assert_eq!(a3.state, ActionState::Succeeded);
    assert!(a3.resolution.as_deref().unwrap().contains("resolved"));
    let e3 = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(e3.budget.held_unknown_micros, 0);
    assert_eq!(e3.budget.spent_micros, 120);
    // Resolution never revives a cancelled execution: cancel, then resolve another unknown.
}

#[test]
fn overdue_action_with_spooled_evidence_settles_from_the_reconciler() {
    let w = world();
    let (_, e, g) = running(&w);
    let a = dispatched(&w, &g, "proc.run", 0);
    w.kernel
        .end_turn(
            g,
            TurnEnd::Wait {
                wake: Wake::Actions {
                    correlation_ids: vec![a.correlation_id.clone()],
                },
            },
        )
        .unwrap();
    // The notify was lost; only the spool knows.
    w.spool
        .write(&completion(&a.correlation_id, Outcome::Failed, None))
        .unwrap();
    // Not overdue yet: event-first, the reconciler does not poll it.
    let rep = w
        .kernel
        .reconcile(&crate::job::WrapperEvidence {
            spool: w.spool.clone(),
        })
        .unwrap();
    assert!(rep.settled_from_evidence.is_empty());
    w.clock.advance(61_000);
    let rep = w
        .kernel
        .reconcile(&crate::job::WrapperEvidence {
            spool: w.spool.clone(),
        })
        .unwrap();
    assert_eq!(rep.settled_from_evidence, vec![a.correlation_id.clone()]);
    let e2 = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(e2.state, ExecState::Queued);
    assert_eq!(
        w.kernel.action(&a.correlation_id).unwrap().unwrap().state,
        ActionState::Failed
    );
}

#[test]
fn due_wake_fires_from_the_reconciler_under_the_virtual_clock() {
    let w = world();
    let (_, e, g) = running(&w);
    let due = w.clock.now_ms() + 7 * 24 * 3600 * 1000; // a task waits a week
    w.kernel
        .end_turn(
            g,
            TurnEnd::Wait {
                wake: Wake::DueAt { at_ms: due },
            },
        )
        .unwrap();
    assert!(w.kernel.reconcile(&NoEvidence).unwrap().woke_due.is_empty());
    w.clock.advance(7 * 24 * 3600 * 1000 - 1);
    assert!(w.kernel.reconcile(&NoEvidence).unwrap().woke_due.is_empty());
    w.clock.advance(1);
    assert_eq!(
        w.kernel.reconcile(&NoEvidence).unwrap().woke_due,
        vec![e.id.clone()]
    );
    assert_eq!(
        w.kernel.execution(&e.id).unwrap().unwrap().state,
        ExecState::Queued
    );
}

#[test]
fn confirm_binds_the_final_action_and_any_change_after_it_invalidates() {
    let w = world();
    let (_, _e, g) = running(&w);
    // The core's policy decided this call waits for eddie.
    let p = proposal("proc.run");
    let a = w
        .kernel
        .plan_action(&g, &p, RetryClass::NonRepeatable, None, 0)
        .unwrap();
    // No confirm yet: authorize refuses.
    let err = w
        .kernel
        .authorize(&a.correlation_id, &p, Some("eddie"))
        .unwrap_err();
    assert!(matches!(
        err.downcast_ref::<KernelError>(),
        Some(KernelError::ConfirmRequired { .. })
    ));
    // Confirm for different args: refused.
    let mut other = p.clone();
    other.args["a"] = json!(2);
    assert!(w
        .kernel
        .bind_confirm(&a.correlation_id, "eddie", &other)
        .is_err());
    // Confirm for the real args, by the right person.
    w.kernel
        .bind_confirm(&a.correlation_id, "eddie", &p)
        .unwrap();
    // Arguments changed after the confirm: the digest no longer matches.
    let err = w
        .kernel
        .authorize(&a.correlation_id, &other, Some("eddie"))
        .unwrap_err();
    assert!(matches!(
        err.downcast_ref::<KernelError>(),
        Some(KernelError::ConfirmInvalidated { .. })
    ));
    // Wrong principal.
    let err = w
        .kernel
        .authorize(&a.correlation_id, &p, Some("tank"))
        .unwrap_err();
    assert!(matches!(
        err.downcast_ref::<KernelError>(),
        Some(KernelError::ConfirmInvalidated { .. })
    ));
    // Expired.
    w.clock.advance(KernelConfig::default().confirm_ttl_ms + 1);
    let err = w
        .kernel
        .authorize(&a.correlation_id, &p, Some("eddie"))
        .unwrap_err();
    assert!(matches!(
        err.downcast_ref::<KernelError>(),
        Some(KernelError::ConfirmInvalidated { .. })
    ));
    // Re-confirm and it dispatches.
    w.kernel
        .bind_confirm(&a.correlation_id, "eddie", &p)
        .unwrap();
    let a2 = w
        .kernel
        .authorize(&a.correlation_id, &p, Some("eddie"))
        .unwrap();
    assert_eq!(a2.state, ActionState::Authorized);
    let a3 = w.kernel.dispatch(&a.correlation_id, Some("ext_1")).unwrap();
    assert_eq!(a3.state, ActionState::Dispatched);
    assert_eq!(a3.external_op_id.as_deref(), Some("ext_1"));
    drop(g);
}

/// An action that waits for the operator keeps the proposal its confirm binds,
/// and nothing else does (theseus-0g4). `pending_confirms` is the one list of
/// what waits: a budget question first, never a provider call or a confirmed
/// action. An action stored before theseus-0g4, with no `proposal` key,
/// decodes and still waits.
#[test]
fn an_action_that_waits_keeps_its_proposal_and_one_stored_without_still_waits() {
    let w = world();
    let (_, _e, g) = running(&w);
    let ask = proposal("fs.write");
    let a = w
        .kernel
        .plan_confirm_with(&g, &ask, RetryClass::NonRepeatable, None, |_| Ok(vec![]))
        .unwrap();
    let stored = w.kernel.action(&a.correlation_id).unwrap().unwrap();
    assert_eq!(stored.proposal.as_ref(), Some(&ask));
    assert_eq!(stored.args_digest, crate::gate::digest_proposal(&ask));
    assert!(stored.awaits_confirm());
    let ran = dispatched(&w, &g, "fs.read", 0);
    assert!(ran.proposal.is_none(), "a call that never waits keeps none");
    let model = w
        .kernel
        .plan_action(
            &g,
            &proposal(PROVIDER_TOOL),
            RetryClass::SafeToRepeat,
            None,
            0,
        )
        .unwrap();
    assert!(model.proposal.is_none() && !model.awaits_confirm());
    let ids = |v: Vec<Action>| v.into_iter().map(|a| a.correlation_id).collect::<Vec<_>>();
    assert_eq!(
        ids(w.kernel.pending_confirms().unwrap()),
        vec![a.correlation_id.clone()]
    );
    // A budget question keeps its proposal too, and is listed first.
    let q = w.kernel.ask_budget(&g, 1_000).unwrap();
    let qp = q.proposal.clone().expect("the question keeps its proposal");
    assert_eq!(crate::gate::digest_proposal(&qp), q.args_digest);
    assert_eq!(
        ids(w.kernel.pending_confirms().unwrap()),
        vec![q.correlation_id.clone(), a.correlation_id.clone()]
    );
    // As a binary from before theseus-0g4 stored it: no proposal, still waiting.
    let mut v = serde_json::to_value(&stored).unwrap();
    assert!(v.as_object_mut().unwrap().remove("proposal").is_some());
    let old: Action = serde_json::from_value(v.clone()).unwrap();
    assert!(old.proposal.is_none() && old.awaits_confirm());
    w.kernel
        .store()
        .append(&[NewRecord::json(kinds::ACTION, Some(&a.correlation_id), &v).unwrap()])
        .unwrap();
    assert_eq!(ids(w.kernel.pending_confirms().unwrap()).len(), 2);
    // Answered: it waits no longer.
    w.kernel
        .bind_confirm(&a.correlation_id, "eddie", &ask)
        .unwrap();
    assert_eq!(
        ids(w.kernel.pending_confirms().unwrap()),
        vec![q.correlation_id]
    );
    drop(g);
}

/// The ledger rows of one kind in a session, oldest first.
pub(super) fn rows(w: &World, session: &str, kind: &str) -> Vec<serde_json::Value> {
    w.kernel
        .store()
        .scan_scope(session, 0, 10_000)
        .unwrap()
        .iter()
        .filter(|r| r.kind == kinds::LEDGER)
        .filter_map(|r| r.decode::<LedgerRow>().ok())
        .filter(|r| r.kind == kind)
        .map(|r| r.data)
        .collect()
}

/// Plan a reservation that must not fit, and return the refusal's figures.
fn over(w: &World, g: &TurnGuard, reserve: Micros) -> (Micros, Micros, Micros, Micros) {
    let err = w
        .kernel
        .plan_action(
            g,
            &proposal("provider.messages"),
            RetryClass::SafeToRepeat,
            None,
            reserve,
        )
        .unwrap_err();
    match err.downcast_ref::<KernelError>() {
        Some(KernelError::OverBudget {
            needed,
            available,
            spent,
            limit,
        }) => (*needed, *available, *spent, *limit),
        other => panic!("expected OverBudget, got {other:?}: {err}"),
    }
}

/// A budget is a hard limit in micro-dollars (theseus-0sg): a reservation that
/// does not fit is refused and changes nothing, and the execution does not
/// end. It waits with the reason `budget` on a question to the operator; an
/// approval resets its spend to $0, ledgers who approved it with the spend
/// before and the limit, and queues the execution with the question as a
/// result, so its next turn makes the call.
#[test]
fn a_call_over_the_limit_waits_on_the_operator_and_an_approved_reset_continues() {
    let w = world();
    let (s, e, g) = running(&w);
    assert_eq!(e.budget.limit_micros, 100_000, "running() opens $0.10");
    assert_eq!(e.budget.available(), 100_000, "no control reserve");
    let a = dispatched(&w, &g, "provider.messages", 60_000);
    w.kernel
        .accept_completion(&completion(
            &a.correlation_id,
            Outcome::Succeeded,
            Some(45_000),
        ))
        .unwrap();
    let before = w.kernel.store().last_position();
    assert_eq!(over(&w, &g, 60_000), (60_000, 55_000, 45_000, 100_000));
    assert_eq!(
        w.kernel.store().last_position(),
        before,
        "a refusal writes nothing"
    );
    let e1 = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(e1.state, ExecState::Running, "the execution does not end");
    assert_eq!(
        format!(
            "{}",
            KernelError::OverBudget {
                needed: 60_000,
                available: 55_000,
                spent: 45_000,
                limit: 100_000
            }
        ),
        "over budget: the call needs $0.06, and $0.055 of the $0.10 limit is left ($0.045 spent)"
    );

    // The turn consumes the call it settled, as a turn does before it parks.
    assert_eq!(w.kernel.take_results(&g).unwrap().len(), 1);
    let q = w.kernel.ask_budget(&g, 60_000).unwrap();
    assert_eq!(
        (q.tool.as_str(), q.state),
        (BUDGET_TOOL, ActionState::Planned)
    );
    assert_eq!(q.reserved_micros, 0);
    let asked = rows(&w, &s, "budget.asked");
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0]["spent_usd"], 0.045);
    assert_eq!(asked[0]["limit_usd"], 0.1);
    assert_eq!(asked[0]["needed_usd"], 0.06);
    let e2 = w
        .kernel
        .end_turn(
            g,
            TurnEnd::Wait {
                wake: Wake::Budget {
                    correlation_id: q.correlation_id.clone(),
                },
            },
        )
        .unwrap();
    assert_eq!(e2.state, ExecState::Waiting);
    assert_eq!(
        e2.budget.question.as_deref(),
        Some(q.correlation_id.as_str())
    );
    let stored = serde_json::to_value(&e2.wake).unwrap();
    assert_eq!(
        stored["on"], "budget",
        "the waiting reason is budget: {stored}"
    );

    let (e3, spent_before) = w
        .kernel
        .reset_budget(&q.correlation_id, "discord:eddie")
        .unwrap();
    assert_eq!(spent_before, 45_000);
    assert_eq!(e3.state, ExecState::Queued);
    assert!(e3.resume_pending, "the driver takes the next turn");
    assert_eq!(
        (e3.budget.spent_micros, e3.budget.resets),
        (0, 1),
        "spend back to $0, one reset"
    );
    assert!(e3.budget.question.is_none());
    assert_eq!(e3.queued_results, vec![q.correlation_id.clone()]);
    let q2 = w.kernel.action(&q.correlation_id).unwrap().unwrap();
    assert_eq!(q2.state, ActionState::Succeeded);
    assert!(q2.settled_at_ms.is_some());
    assert!(
        q2.resolution
            .as_deref()
            .unwrap()
            .starts_with("approved by discord:eddie"),
        "{:?}",
        q2.resolution
    );
    let reset = rows(&w, &s, "budget.reset");
    assert_eq!(reset.len(), 1);
    assert_eq!(reset[0]["by"], "discord:eddie");
    assert_eq!(reset[0]["spent_before_usd"], 0.045);
    assert_eq!(reset[0]["limit_usd"], 0.1);
    assert_eq!(reset[0]["resets"], 1);
    assert_eq!(reset[0]["correlation_id"], q.correlation_id.as_str());

    // The next turn gets the answer as a result, and the call now fits.
    let g = w.kernel.admit(&e.id).unwrap();
    let results = w.kernel.take_results(&g).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].tool, BUDGET_TOOL);
    let _ = dispatched(&w, &g, "provider.messages", 60_000);
    // A question is answered once.
    assert!(w.kernel.reset_budget(&q.correlation_id, "eddie").is_err());
    drop(g);
}

/// A decline, or no answer, is not a hard no: the execution keeps waiting on
/// its budget. New input wakes it, its next call asks again, and the new
/// question supersedes an unanswered one.
#[test]
fn a_declined_budget_question_keeps_waiting_and_new_input_asks_again() {
    let w = world();
    let (s, e, g) = running(&w);
    let _ = over(&w, &g, 200_000);
    let q1 = w.kernel.ask_budget(&g, 200_000).unwrap();
    let wake = Wake::Budget {
        correlation_id: q1.correlation_id.clone(),
    };
    w.kernel
        .end_turn(g, TurnEnd::Wait { wake: wake.clone() })
        .unwrap();
    let d = w
        .kernel
        .decline_action(&q1.correlation_id, "operator", "not now")
        .unwrap();
    assert_eq!(d.state, ActionState::Cancelled);
    let e1 = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(
        (e1.state, e1.wake.clone()),
        (ExecState::Waiting, Some(wake)),
        "declined: still waiting on the budget"
    );
    assert!(e1.budget.question.is_none());
    assert!(w.kernel.reset_budget(&q1.correlation_id, "eddie").is_err());

    // A new message: the next call asks again.
    w.kernel.wake_input(&e.id).unwrap();
    let g = w.kernel.admit(&e.id).unwrap();
    let _ = over(&w, &g, 200_000);
    let q2 = w.kernel.ask_budget(&g, 200_000).unwrap();
    assert_ne!(q2.correlation_id, q1.correlation_id);
    w.kernel
        .end_turn(
            g,
            TurnEnd::Wait {
                wake: Wake::Budget {
                    correlation_id: q2.correlation_id.clone(),
                },
            },
        )
        .unwrap();
    // Unanswered, and another message: the newer question supersedes it.
    w.kernel.wake_input(&e.id).unwrap();
    let g = w.kernel.admit(&e.id).unwrap();
    let q3 = w.kernel.ask_budget(&g, 200_000).unwrap();
    let q2 = w.kernel.action(&q2.correlation_id).unwrap().unwrap();
    assert_eq!(q2.state, ActionState::Cancelled);
    assert!(
        q2.resolution.as_deref().unwrap().contains("superseded"),
        "{:?}",
        q2.resolution
    );
    let e3 = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(
        e3.budget.question.as_deref(),
        Some(q3.correlation_id.as_str())
    );
    assert_eq!(rows(&w, &s, "budget.asked").len(), 3);
    let open: Vec<_> = w
        .kernel
        .open_actions()
        .unwrap()
        .into_iter()
        .filter(|a| a.tool == BUDGET_TOOL)
        .collect();
    assert_eq!(open.len(), 1, "one open question at a time");
    drop(g);
}

/// An approval that lands before the turn parks (the operator is quick)
/// still continues: the turn ends queued, never waiting on an answered question.
#[test]
fn a_reset_approved_before_the_turn_parks_still_continues() {
    let w = world();
    let (_, _, g) = running(&w);
    let q = w.kernel.ask_budget(&g, 500_000).unwrap();
    let (mid, _) = w.kernel.reset_budget(&q.correlation_id, "eddie").unwrap();
    assert_eq!(mid.state, ExecState::Running);
    let e2 = w
        .kernel
        .end_turn(
            g,
            TurnEnd::Wait {
                wake: Wake::Budget {
                    correlation_id: q.correlation_id.clone(),
                },
            },
        )
        .unwrap();
    assert_eq!(e2.state, ExecState::Queued);
    assert_eq!(e2.queued_results, vec![q.correlation_id]);
    assert!(e2.wake.is_none());
}

/// Cancel is still the operator's way out, and terminal stays terminal: the
/// open question closes with the execution, and a reset cannot revive it.
#[test]
fn cancelling_a_budget_wait_closes_its_question_and_terminal_stays_terminal() {
    let w = world();
    let (_, e, g) = running(&w);
    let q = w.kernel.ask_budget(&g, 500_000).unwrap();
    w.kernel
        .end_turn(
            g,
            TurnEnd::Wait {
                wake: Wake::Budget {
                    correlation_id: q.correlation_id.clone(),
                },
            },
        )
        .unwrap();
    w.kernel.cancel_execution(&e.id, "eddie").unwrap();
    let q2 = w.kernel.action(&q.correlation_id).unwrap().unwrap();
    assert_eq!(q2.state, ActionState::Cancelled);
    assert!(q2.settled_at_ms.is_some());
    assert!(w.kernel.reset_budget(&q.correlation_id, "eddie").is_err());
    let e2 = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(e2.state, ExecState::Cancelled);
    assert!(w.kernel.wake_input(&e.id).is_err());
}

/// A turn that ends its execution while a budget question is open (a later
/// call fitted, and the turn completed) closes the question with it: the
/// simulator found this (seed 8), and nothing may approve a dead question.
#[test]
fn an_execution_that_ends_closes_its_open_budget_question() {
    let w = world();
    let (_, e, g) = running(&w);
    let q = w.kernel.ask_budget(&g, 500_000).unwrap();
    let _ = dispatched(&w, &g, "provider.messages", 1_000);
    w.kernel
        .end_turn(
            g,
            TurnEnd::Complete {
                reason: "done".into(),
            },
        )
        .unwrap();
    let q2 = w.kernel.action(&q.correlation_id).unwrap().unwrap();
    assert_eq!(q2.state, ActionState::Cancelled);
    assert!(q2.resolution.as_deref().unwrap().contains("ended"));
    let e2 = w.kernel.execution(&e.id).unwrap().unwrap();
    assert!(e2.budget.question.is_none());
    assert!(w.kernel.reset_budget(&q.correlation_id, "eddie").is_err());
}

/// A real cost is never hidden: a call that cost more than it reserved books
/// all of it, so spend may pass the limit, and then nothing more is reserved
/// until the operator resets it.
#[test]
fn a_call_that_costs_more_than_it_reserved_books_all_of_it() {
    let w = world();
    let (_, e, g) = running(&w);
    let a = dispatched(&w, &g, "provider.messages", 90_000);
    w.kernel
        .accept_completion(&completion(
            &a.correlation_id,
            Outcome::Succeeded,
            Some(120_000),
        ))
        .unwrap();
    let e1 = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(e1.budget.spent_micros, 120_000);
    assert_eq!(e1.budget.available(), 0);
    let (_, available, spent, limit) = over(&w, &g, 1);
    assert_eq!((available, spent, limit), (0, 120_000, 100_000));
    drop(g);
}

// ------------------------------------------------------------ the limit follows the config (theseus-3pj)

fn limited(spend_limit_micros: Micros) -> KernelConfig {
    KernelConfig {
        spend_limit_micros,
        ..KernelConfig::default()
    }
}

/// A conversation opened with the config's limit, which it follows, woken
/// and admitted.
fn following(w: &World) -> (SessionId, Execution, TurnGuard) {
    let e = w
        .kernel
        .open_execution(
            &new_id("ses"),
            SessionKind::Conversation,
            auth(),
            None,
            None,
        )
        .unwrap();
    assert!(!e.budget.pinned);
    w.kernel.wake_input(&e.id).unwrap();
    let g = w.kernel.admit(&e.id).unwrap();
    let e = w.kernel.execution(&e.id).unwrap().unwrap();
    (e.session_id.clone(), e, g)
}

/// Spend `cost` on one call; then the next call, needing `needs`, does not
/// fit, and the turn parks on its question.
fn parked_at_limit(w: &World, g: TurnGuard, cost: Micros, needs: Micros) -> Action {
    let a = dispatched(w, &g, "provider.messages", cost);
    w.kernel
        .accept_completion(&completion(
            &a.correlation_id,
            Outcome::Succeeded,
            Some(cost),
        ))
        .unwrap();
    w.kernel.take_results(&g).unwrap();
    over(w, &g, needs);
    let q = w.kernel.ask_budget(&g, needs).unwrap();
    w.kernel
        .end_turn(
            g,
            TurnEnd::Wait {
                wake: Wake::Budget {
                    correlation_id: q.correlation_id.clone(),
                },
            },
        )
        .unwrap();
    q
}

/// An open session follows `spend_limit_micros` (theseus-3pj). A raise
/// across a restart gives it the new limit in one ledgered rewrite, withdraws
/// the question it waits on, with the reason, and queues it with the
/// question as a result, as an approved reset does, so its next turn makes
/// the call that did not fit. The spend and the resets are untouched. A
/// second start under the same config writes nothing.
#[test]
fn a_raised_limit_across_a_restart_lets_a_session_waiting_at_its_old_limit_continue() {
    let w = world_with(limited(100_000));
    let (s, e, g) = following(&w);
    assert_eq!(e.budget.limit_micros, 100_000);
    let q = parked_at_limit(&w, g, 60_000, 60_000);

    let (w, rep) = crash(w, limited(250_000));
    assert_eq!(
        rep.limits_followed,
        vec![LimitFollowed {
            execution_id: e.id.clone(),
            session_id: s.clone(),
            from_micros: 100_000,
            to_micros: 250_000,
            withdrew: Some(q.correlation_id.clone()),
            proceeds: true,
        }]
    );
    let e1 = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(
        (
            e1.state,
            e1.budget.limit_micros,
            e1.budget.spent_micros,
            e1.budget.resets
        ),
        (ExecState::Queued, 250_000, 60_000, 0),
        "the config's limit; the spend and the resets as they were"
    );
    assert!(e1.resume_pending, "the driver takes the next turn");
    assert!(e1.wake.is_none() && e1.budget.question.is_none());
    assert_eq!(e1.queued_results, vec![q.correlation_id.clone()]);
    let q1 = w.kernel.action(&q.correlation_id).unwrap().unwrap();
    assert_eq!(q1.state, ActionState::Cancelled);
    assert_eq!(
        q1.resolution.as_deref(),
        Some("withdrawn: the spend limit was raised from $0.10 to $0.25")
    );
    let changed = rows(&w, &s, "budget.limit_changed");
    assert_eq!(changed.len(), 1);
    let c = &changed[0];
    assert_eq!(
        (
            &c["from_usd"],
            &c["to_usd"],
            &c["spent_usd"],
            &c["available_usd"]
        ),
        (&json!(0.1), &json!(0.25), &json!(0.06), &json!(0.19))
    );
    assert_eq!(
        (&c["withdrew"], &c["proceeds"], &c["state"]),
        (&json!(q.correlation_id), &json!(true), &json!("queued"))
    );
    assert_eq!(
        rows(&w, &s, "execution.queued").last().unwrap()["why"],
        "limit_raised"
    );
    assert!(w.kernel.pending_confirms().unwrap().is_empty());

    // The next turn gets the withdrawn question as its result, and the call fits.
    let g = w.kernel.admit(&e.id).unwrap();
    let results = w.kernel.take_results(&g).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].correlation_id, q.correlation_id);
    let _ = dispatched(&w, &g, "provider.messages", 60_000);
    drop(g);

    // Once: a start under the same limit rewrites nothing.
    let (w, rep) = crash(w, limited(250_000));
    assert!(rep.limits_followed.is_empty(), "{:?}", rep.limits_followed);
    assert_eq!(rows(&w, &s, "budget.limit_changed").len(), 1);
}

/// A lower limit changes nothing but the limit, even when the session has
/// already spent more than it. Its next reservation does not fit, and the
/// turn asks, as usual; an approved reset then brings the spend to $0 under
/// the new limit.
#[test]
fn a_lowered_limit_asks_at_the_next_reservation_over_it() {
    let w = world_with(limited(100_000));
    let (s, e, g) = following(&w);
    let a = dispatched(&w, &g, "provider.messages", 50_000);
    w.kernel
        .accept_completion(&completion(
            &a.correlation_id,
            Outcome::Succeeded,
            Some(50_000),
        ))
        .unwrap();
    w.kernel.take_results(&g).unwrap();
    w.kernel
        .end_turn(g, TurnEnd::Wait { wake: Wake::Input })
        .unwrap();

    let (w, rep) = crash(w, limited(40_000));
    assert_eq!(rep.limits_followed.len(), 1);
    let f = &rep.limits_followed[0];
    assert_eq!(
        (
            f.from_micros,
            f.to_micros,
            f.withdrew.as_deref(),
            f.proceeds
        ),
        (100_000, 40_000, None, false)
    );
    let e1 = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(
        (e1.state, e1.wake.clone(), e1.budget.limit_micros),
        (ExecState::Waiting, Some(Wake::Input), 40_000)
    );
    assert_eq!(
        e1.budget.spent_micros, 50_000,
        "spend never moves but by a reset"
    );
    let changed = rows(&w, &s, "budget.limit_changed");
    assert_eq!(
        (&changed[0]["to_usd"], &changed[0]["available_usd"]),
        (&json!(0.04), &json!(0.0))
    );

    w.kernel.wake_input(&e.id).unwrap();
    let g = w.kernel.admit(&e.id).unwrap();
    assert_eq!(over(&w, &g, 1), (1, 0, 50_000, 40_000));
    let q = w.kernel.ask_budget(&g, 30_000).unwrap();
    assert_eq!(rows(&w, &s, "budget.asked")[0]["limit_usd"], 0.04);
    w.kernel
        .end_turn(
            g,
            TurnEnd::Wait {
                wake: Wake::Budget {
                    correlation_id: q.correlation_id.clone(),
                },
            },
        )
        .unwrap();
    let (e2, before) = w.kernel.reset_budget(&q.correlation_id, "eddie").unwrap();
    assert_eq!((before, e2.budget.spent_micros), (50_000, 0));
    let g = w.kernel.admit(&e.id).unwrap();
    w.kernel.take_results(&g).unwrap();
    let _ = dispatched(&w, &g, "provider.messages", 30_000);
    drop(g);
}

/// Under a copy the vault has not confirmed (theseus-2fo), startup writes no
/// limit the copy decides. The vault's word applies it, once
/// (`follow_spend_limit`).
#[test]
fn under_an_unconfirmed_copy_startup_keeps_the_limits_and_the_vaults_word_applies_them() {
    let w = world_with(limited(100_000));
    let (s, e, g) = following(&w);
    let q = parked_at_limit(&w, g, 60_000, 60_000);
    let (w, rep) = crash(
        w,
        KernelConfig {
            unconfirmed_config: true,
            ..limited(250_000)
        },
    );
    assert!(rep.limits_followed.is_empty());
    let e1 = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(
        (e1.state, e1.budget.limit_micros, e1.budget.question),
        (ExecState::Waiting, 100_000, Some(q.correlation_id))
    );
    assert!(rows(&w, &s, "budget.limit_changed").is_empty());

    let followed = w.kernel.follow_spend_limit().unwrap();
    assert_eq!(followed.len(), 1);
    assert!(followed[0].proceeds);
    let e2 = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(
        (e2.state, e2.budget.limit_micros),
        (ExecState::Queued, 250_000)
    );
    assert!(w.kernel.follow_spend_limit().unwrap().is_empty(), "once");
    assert_eq!(rows(&w, &s, "budget.limit_changed").len(), 1);
}

/// A declined question leaves the session waiting on its budget. A raise
/// lets that wait proceed too: the declined question, which stays declined,
/// is queued as the result.
#[test]
fn a_raise_lets_a_declined_budget_wait_proceed() {
    let w = world_with(limited(100_000));
    let (_, e, g) = following(&w);
    let q = parked_at_limit(&w, g, 60_000, 60_000);
    w.kernel
        .decline_action(&q.correlation_id, "eddie", "not now")
        .unwrap();
    let e1 = w.kernel.execution(&e.id).unwrap().unwrap();
    assert!(e1.budget.question.is_none());
    assert!(matches!(e1.wake, Some(Wake::Budget { .. })));

    let (w, rep) = crash(w, limited(200_000));
    let f = &rep.limits_followed[0];
    assert_eq!((f.withdrew.as_deref(), f.proceeds), (None, true));
    let e2 = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(e2.state, ExecState::Queued);
    assert_eq!(e2.queued_results, vec![q.correlation_id.clone()]);
    let q2 = w.kernel.action(&q.correlation_id).unwrap().unwrap();
    assert!(q2.resolution.unwrap().starts_with("declined by eddie"));
}

/// What does not follow: an execution opened with a limit of its own
/// (`pinned`), and one that has ended. A limit that is the config's leaves
/// no mark in the record, so every record the product has written follows.
#[test]
fn a_pinned_limit_and_an_ended_execution_keep_their_limits() {
    let w = world_with(limited(100_000));
    let (_, pinned, g) = running(&w);
    assert!(pinned.budget.pinned);
    w.kernel
        .end_turn(g, TurnEnd::Wait { wake: Wake::Input })
        .unwrap();
    let (_, done, g) = following(&w);
    let stored = serde_json::to_value(&done.budget).unwrap();
    assert!(stored.get("pinned").is_none(), "{stored}");
    w.kernel
        .end_turn(
            g,
            TurnEnd::Complete {
                reason: "done".into(),
            },
        )
        .unwrap();

    let (w, rep) = crash(w, limited(300_000));
    assert!(rep.limits_followed.is_empty(), "{:?}", rep.limits_followed);
    let limit = |id: &str| w.kernel.execution(id).unwrap().unwrap().budget.limit_micros;
    assert_eq!((limit(&pinned.id), limit(&done.id)), (100_000, 100_000));
}

#[test]
fn admission_ceiling_holds_and_cancel_is_honored_immediately() {
    let w = world_with(KernelConfig {
        admission_ceiling: 2,
        ..Default::default()
    });
    let mut execs = Vec::new();
    for _ in 0..3 {
        let e = w
            .kernel
            .open_execution(&new_id("ses"), SessionKind::Task, auth(), None, None)
            .unwrap();
        w.kernel.wake_input(&e.id).unwrap();
        execs.push(e.id);
    }
    let g0 = w.kernel.admit(&execs[0]).unwrap();
    let g1 = w.kernel.admit(&execs[1]).unwrap();
    let err = w.kernel.admit(&execs[2]).unwrap_err();
    assert!(matches!(
        err.downcast_ref::<KernelError>(),
        Some(KernelError::AdmissionFull { ceiling: 2 })
    ));
    assert_eq!(
        w.kernel.execution(&execs[2]).unwrap().unwrap().state,
        ExecState::Queued
    );
    // /cancel does not queue behind admission.
    w.kernel.cancel_execution(&execs[2], "eddie").unwrap();
    assert_eq!(
        w.kernel.execution(&execs[2]).unwrap().unwrap().state,
        ExecState::Cancelled
    );
    // Double admit on the same execution in one process is refused.
    assert!(matches!(
        w.kernel
            .admit(&execs[0])
            .unwrap_err()
            .downcast_ref::<KernelError>(),
        Some(KernelError::NotRunnable { .. })
    ));
    w.kernel.end_turn(g0, TurnEnd::Requeue).unwrap();
    // Room again.
    let g0b = w.kernel.admit(&execs[0]).unwrap();
    assert_eq!(w.kernel.stats().unwrap().turns_held, 2);
    w.kernel
        .end_turn(
            g0b,
            TurnEnd::Complete {
                reason: "ok".into(),
            },
        )
        .unwrap();
    w.kernel
        .end_turn(
            g1,
            TurnEnd::Complete {
                reason: "ok".into(),
            },
        )
        .unwrap();
    assert_eq!(w.kernel.stats().unwrap().turns_held, 0);
}

#[test]
fn wait_on_actions_already_settled_does_not_park_forever() {
    let w = world();
    let (_, e, g) = running(&w);
    let a = dispatched(&w, &g, "fs.read", 0);
    // In-process tool: completion arrives while the turn is still held.
    w.kernel
        .accept_completion(&completion(&a.correlation_id, Outcome::Succeeded, None))
        .unwrap();
    let e2 = w
        .kernel
        .end_turn(
            g,
            TurnEnd::Wait {
                wake: Wake::Actions {
                    correlation_ids: vec![a.correlation_id.clone()],
                },
            },
        )
        .unwrap();
    assert_eq!(e2.state, ExecState::Queued, "the result is already there");
    assert_eq!(e2.queued_results, vec![a.correlation_id]);
    let _ = e;
}

#[test]
fn kernel_refuses_events_before_startup_and_hundred_sessions_survive_upgrade_mid_turn() {
    let dir = tempfile::tempdir().unwrap();
    let clock = VirtualClock::new(5);
    let k = Kernel::new(
        open_store(dir.path()),
        clock.clone(),
        KernelConfig::default(),
    );
    let err = k
        .open_execution("ses_0", SessionKind::Task, auth(), None, None)
        .unwrap_err();
    assert!(matches!(
        err.downcast_ref::<KernelError>(),
        Some(KernelError::NotAccepting { step: 0 })
    ));
    let spool = Spool::open(&dir.path().join("spool")).unwrap();
    k.startup(Some(&spool), &NoEvidence).unwrap();
    let w = World {
        dir,
        clock,
        kernel: k,
        spool,
    };
    let cfg = KernelConfig {
        admission_ceiling: 100,
        ..Default::default()
    };
    let (w, _) = crash(w, cfg.clone());
    let mut ids = Vec::new();
    let mut guards = Vec::new();
    for i in 0..100 {
        let e = w
            .kernel
            .open_execution(&format!("ses_t{i}"), SessionKind::Task, auth(), None, None)
            .unwrap();
        w.kernel.wake_input(&e.id).unwrap();
        let g = w.kernel.admit(&e.id).unwrap();
        if i % 2 == 0 {
            let _ = dispatched(&w, &g, "proc.run", 10);
        }
        guards.push(g);
        ids.push(e.id);
    }
    for g in guards {
        std::mem::forget(g);
    }
    // Graceful upgrade = new binary opens the same store.
    let (w, rep) = crash(w, cfg);
    assert_eq!(rep.requeued_interrupted.len(), 100);
    for id in &ids {
        let e = w.kernel.execution(id).unwrap().unwrap();
        assert_eq!(e.state, ExecState::Queued);
        assert_eq!(e.interrupted, 1);
    }
    assert_eq!(w.kernel.open_actions().unwrap().len(), 50);
    // And every one takes its next turn.
    let mut n = 0;
    for id in &ids {
        let g = w.kernel.admit(id).unwrap();
        w.kernel
            .end_turn(
                g,
                TurnEnd::Complete {
                    reason: "resumed".into(),
                },
            )
            .unwrap();
        n += 1;
    }
    assert_eq!(n, 100);
}

/// Rows stored before theseus-hco still read. The execution and the actions
/// are verbatim from a store an earlier `theseusd` wrote (a scratch daemon,
/// 2026-09-28). The execution's `kind` was the kernel's own `SessionKind`,
/// now the protocol's; the actions carry the only two retry classes any code
/// ever built; and old stores keep `derived_from` edges nothing reads.
#[test]
fn rows_stored_before_the_session_model_cut_still_read() {
    let exec = r#"{"id":"exe_01a0e754c794744aa0f3c8cef99e690b","schema":1,"session_id":"ses_01a0e754c794744aa0f3c8cd0a26857d","kind":"conversation","state":"waiting","authority":{"principal":"operator","ceilings":{}},"budget":{"limit":20000000,"spent":0,"reserved":0,"held_unknown":0,"control_reserve":10000,"reservations":{}},"wake":{"on":"input"},"outstanding":[],"queued_results":[],"turns":0,"interrupted":0,"resume_pending":false,"created_at_ms":1790587488148,"updated_at_ms":1790587488148}"#;
    let safe = r#"{"correlation_id":"act_01a0e754c7ee733bb97884fe2629cac7","schema":1,"execution_id":"exe_01a0e754c794744aa0f3c8cef99e690b","session_id":"ses_01a0e754c794744aa0f3c8cd0a26857d","tool":"provider.messages","args_digest":"d29b78a3f83cb59acf23e90bebd287435a94c213061bca8447156b65d31f72ee","resource":"zai","retry_class":{"class":"safe_to_repeat"},"state":"planned","deadline_at_ms":1790588088238,"planned_at_ms":1790587488238,"reservation_id":"rsv_01a0e754c7ee733bb97884fd65b5d52e","reserved_units":133351,"completions_seen":0}"#;
    let once = r#"{"correlation_id":"act_01a0e7568c0577d7a4a2c9d58fb052dd","schema":1,"execution_id":"exe_01a0e755c3c475aab628168f14e0d0f0","session_id":"ses_01a0e755c3c475aab628168e7d9a2424","tool":"proc.run","args_digest":"8adceac28fe004b3c3980e81c17840a7fdfc7315fba1bd907234bd2f20591aff","resource":"/tmp/theseus-2app/work/scratch","retry_class":{"class":"non_repeatable"},"state":"planned","deadline_at_ms":1790588233973,"planned_at_ms":1790587603973,"reserved_units":0,"completions_seen":0}"#;
    let e = Execution::from_stored(exec.as_bytes(), 100 * MICROS_PER_USD).unwrap();
    assert_eq!(e.kind, SessionKind::Conversation);
    let a: Action = serde_json::from_str(safe).unwrap();
    assert_eq!(a.retry_class, RetryClass::SafeToRepeat);
    assert_eq!(a.reserved_micros, 0, "units never read as dollars");
    let b: Action = serde_json::from_str(once).unwrap();
    assert_eq!(b.retry_class, RetryClass::NonRepeatable);
    for (kind, stored) in [
        (SessionKind::Conversation, r#""conversation""#),
        (SessionKind::Task, r#""task""#),
    ] {
        assert_eq!(serde_json::to_string(&kind).unwrap(), stored);
        assert_eq!(serde_json::from_str::<SessionKind>(stored).unwrap(), kind);
    }

    // A store holding them, and an old edge, starts and reads them back.
    let w = world();
    let raw = |kind, key: &str, json: &str| {
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        NewRecord::json(kind, Some(key), &v)
            .unwrap()
            .scoped(&e.session_id)
    };
    let edge = r#"{"type":"derived_from","from":"cmp_2","to":"cmp_1","at_ms":1}"#;
    w.kernel
        .store()
        .append(&[
            raw(kinds::EXECUTION, &e.id, exec),
            raw(kinds::ACTION, &a.correlation_id, safe),
            raw(kinds::EDGE, "derived_from|cmp_2|cmp_1", edge),
        ])
        .unwrap();
    let (w, _) = crash(w, KernelConfig::default());
    // Startup rewrote the unit budget once, in dollars (theseus-0sg).
    let expected = Execution {
        schema: SCHEMA,
        ..e.clone()
    };
    assert_eq!(w.kernel.execution(&e.id).unwrap().unwrap(), expected);
    assert_eq!(w.kernel.action(&a.correlation_id).unwrap().unwrap(), a);
}

/// Executions stored with unit budgets (before theseus-0sg) serve under this
/// binary: M3.5's rule is that a new on-disk format lands with the reader for
/// the one it replaces. The reader gives each a dollar budget: the configured
/// limit, nothing reserved or held, and the unit figures kept as they were.
/// Startup rewrites each once, taking its spend from the session's recorded
/// cost (a lookup the core installs). Terminal stays terminal, a turn a crash
/// interrupted is requeued, and the next startup rewrites nothing.
#[test]
fn executions_stored_with_unit_budgets_serve_in_dollars() {
    let exhausted = r#"{"id":"exe_old_exhausted","schema":1,"session_id":"ses_old_a","kind":"conversation","state":"budget_exhausted","authority":{"principal":"operator","ceilings":{}},"budget":{"limit":1000000,"spent":877683,"reserved":0,"held_unknown":0,"control_reserve":10000,"reservations":{}},"outstanding":[],"queued_results":[],"turns":15,"interrupted":0,"resume_pending":false,"ended_reason":"action provider.messages needs 172068 units, 112317 available","created_at_ms":1790000000000,"updated_at_ms":1790000500000}"#;
    let waiting = r#"{"id":"exe_old_waiting","schema":1,"session_id":"ses_old_b","kind":"conversation","state":"waiting","authority":{"principal":"operator","ceilings":{}},"budget":{"limit":20000000,"spent":154321,"reserved":0,"held_unknown":0,"control_reserve":10000,"reservations":{}},"wake":{"on":"input"},"outstanding":[],"queued_results":[],"turns":3,"interrupted":0,"resume_pending":false,"created_at_ms":1790000000000,"updated_at_ms":1790000500000}"#;
    let running = r#"{"id":"exe_old_running","schema":1,"session_id":"ses_old_c","kind":"task","state":"running","authority":{"principal":"operator","ceilings":{}},"budget":{"limit":1000000,"spent":5000,"reserved":133351,"held_unknown":2000,"control_reserve":10000,"reservations":{"rsv_old":133351}},"outstanding":[],"queued_results":[],"turns":2,"interrupted":0,"resume_pending":false,"created_at_ms":1790000000000,"updated_at_ms":1790000500000}"#;
    let w = world();
    let raw = |key: &str, session: &str, json: &str| {
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        NewRecord::json(kinds::EXECUTION, Some(key), &v)
            .unwrap()
            .scoped(session)
    };
    w.kernel
        .store()
        .append(&[
            raw("exe_old_exhausted", "ses_old_a", exhausted),
            raw("exe_old_waiting", "ses_old_b", waiting),
            raw("exe_old_running", "ses_old_c", running),
        ])
        .unwrap();
    // Before any rewrite, the reader already serves every one of them.
    let read = w.kernel.execution("exe_old_exhausted").unwrap().unwrap();
    assert_eq!(read.state, ExecState::BudgetExhausted);
    assert_eq!(read.budget.limit_micros, 100 * MICROS_PER_USD);
    assert_eq!(read.budget.units_before.as_ref().unwrap().spent, 877_683);

    let restart = |w: World| {
        let World {
            dir,
            clock,
            spool,
            kernel,
        } = w;
        drop(kernel);
        let spent = |session: &str| match session {
            "ses_old_a" => 450_000,
            "ses_old_b" => 12_000,
            _ => 0,
        };
        let kernel = Kernel::new(
            open_store(dir.path()),
            clock.clone(),
            KernelConfig::default(),
        )
        .with_legacy_spend(Arc::new(spent));
        kernel.startup(Some(&spool), &NoEvidence).unwrap();
        World {
            dir,
            clock,
            spool,
            kernel,
        }
    };
    let w = restart(w);
    let x = w.kernel.execution("exe_old_exhausted").unwrap().unwrap();
    assert_eq!(
        x.state,
        ExecState::BudgetExhausted,
        "terminal stays terminal"
    );
    assert_eq!(x.schema, SCHEMA);
    assert_eq!(
        (x.budget.limit_micros, x.budget.spent_micros),
        (100 * MICROS_PER_USD, 450_000),
        "the configured limit, and the session's recorded $0.45"
    );
    let units = x.budget.units_before.clone().unwrap();
    assert_eq!((units.limit, units.spent), (1_000_000, 877_683));
    assert_eq!(
        x.ended_reason.as_deref().map(|r| r.contains("172068")),
        Some(true)
    );
    let y = w.kernel.execution("exe_old_waiting").unwrap().unwrap();
    assert_eq!(
        (y.state, y.budget.spent_micros),
        (ExecState::Waiting, 12_000)
    );
    let z = w.kernel.execution("exe_old_running").unwrap().unwrap();
    assert_eq!((z.state, z.interrupted), (ExecState::Queued, 1));
    assert_eq!(
        (z.budget.reserved_micros, z.budget.held_unknown_micros),
        (0, 0),
        "units are never read as dollars"
    );
    assert!(z.budget.reservations.is_empty());
    assert_eq!(z.budget.units_before.as_ref().unwrap().held_unknown, 2000);
    for (session, n) in [("ses_old_a", 1), ("ses_old_b", 1), ("ses_old_c", 1)] {
        let migrated = rows(&w, session, "budget.migrated");
        assert_eq!(migrated.len(), n, "{session}");
        assert!(migrated[0]["units_before"].is_object(), "{:?}", migrated[0]);
    }
    assert_eq!(
        rows(&w, "ses_old_a", "budget.migrated")[0]["spent_usd"],
        0.45
    );
    // The next startup finds nothing to rewrite.
    let w = restart(w);
    for session in ["ses_old_a", "ses_old_b", "ses_old_c"] {
        assert_eq!(rows(&w, session, "budget.migrated").len(), 1, "{session}");
    }
    assert_eq!(
        w.kernel.stats().unwrap().executions_by_state["budget_exhausted"],
        1
    );
    // The rewritten budget counts on from the session's recorded spend.
    let e = w.kernel.execution("exe_old_waiting").unwrap().unwrap();
    assert_eq!(e.budget.available(), 100 * MICROS_PER_USD - 12_000);
}

// ------------------------------------------------------------ one writer per execution (theseus-id9)

/// A store that stops one thread at one read, so that a second writer runs
/// between a transition's read and its write.
pub(super) struct Pausing {
    inner: Arc<dyn Store>,
    at: std::sync::Mutex<Option<PauseAt>>,
}

struct PauseAt {
    thread: std::thread::ThreadId,
    kind: theseus_store::RecordKind,
    key: String,
    /// Stop at this read of the record by that thread; 1 is the first.
    nth: usize,
    paused: std::sync::mpsc::Sender<()>,
    resume: std::sync::mpsc::Receiver<()>,
}

impl Store for Pausing {
    fn append(&self, batch: &[NewRecord]) -> anyhow::Result<Vec<u64>> {
        self.inner.append(batch)
    }
    fn get(&self, position: u64) -> anyhow::Result<Option<theseus_store::Record>> {
        self.inner.get(position)
    }
    fn scan(
        &self,
        from: u64,
        to: Option<u64>,
        limit: usize,
    ) -> anyhow::Result<Vec<theseus_store::Record>> {
        self.inner.scan(from, to, limit)
    }
    fn latest_by_key(
        &self,
        kind: theseus_store::RecordKind,
        key: &str,
    ) -> anyhow::Result<Option<theseus_store::Record>> {
        let r = self.inner.latest_by_key(kind, key)?;
        let stop = {
            let mut at = self.at.lock().unwrap();
            let hit = at.as_mut().is_some_and(|p| {
                if p.thread != std::thread::current().id() || p.kind != kind || p.key != key {
                    return false;
                }
                p.nth -= 1;
                p.nth == 0
            });
            if hit {
                at.take()
            } else {
                None
            }
        };
        if let Some(p) = stop {
            p.paused.send(()).unwrap();
            p.resume.recv().unwrap();
        }
        Ok(r)
    }
    fn latest_of_kind(
        &self,
        kind: theseus_store::RecordKind,
    ) -> anyhow::Result<Vec<theseus_store::Record>> {
        self.inner.latest_of_kind(kind)
    }
    fn tail_of_kind(
        &self,
        kind: theseus_store::RecordKind,
        n: usize,
    ) -> anyhow::Result<Vec<theseus_store::Record>> {
        self.inner.tail_of_kind(kind, n)
    }
    fn count_of_kind(&self, kind: theseus_store::RecordKind) -> anyhow::Result<u64> {
        self.inner.count_of_kind(kind)
    }
    fn scan_scope(
        &self,
        scope: &str,
        after: u64,
        limit: usize,
    ) -> anyhow::Result<Vec<theseus_store::Record>> {
        self.inner.scan_scope(scope, after, limit)
    }
    fn count_in_scope(&self, scope: &str) -> anyhow::Result<u64> {
        self.inner.count_in_scope(scope)
    }
    fn last_position(&self) -> u64 {
        self.inner.last_position()
    }
    fn checkpoint(&self) -> anyhow::Result<u64> {
        self.inner.checkpoint()
    }
    fn stats(&self) -> anyhow::Result<theseus_store::StoreStats> {
        self.inner.stats()
    }
}

pub(super) fn pausing_world() -> (World, Arc<Pausing>) {
    let dir = tempfile::tempdir().unwrap();
    let clock = VirtualClock::new(1_000_000);
    let spool = Spool::open(&dir.path().join("spool")).unwrap();
    let p = Arc::new(Pausing {
        inner: open_store(dir.path()),
        at: Default::default(),
    });
    let kernel = Kernel::new(p.clone(), clock.clone(), KernelConfig::default());
    kernel.startup(Some(&spool), &NoEvidence).unwrap();
    (
        World {
            dir,
            clock,
            kernel,
            spool,
        },
        p,
    )
}

/// How a race went: what each writer returned, and whether the second one
/// was waiting for an execution's lock when the first went on.
pub(super) struct Raced<R1, R2> {
    pub(super) first: R1,
    pub(super) second: R2,
    pub(super) second_waited: bool,
}

/// Run `first` on a thread stopped at its `nth` read of (`kind`, `key`), and
/// `second` on another thread while it is stopped. The first goes on once
/// the second has returned, or waits for an execution's lock: so with the
/// locks the second writes after the first, and without them, in between.
pub(super) fn race<R1: Send, R2: Send>(
    k: &Kernel,
    p: &Pausing,
    (kind, key, nth): (theseus_store::RecordKind, &str, usize),
    first: impl FnOnce(&Kernel) -> R1 + Send,
    second: impl FnOnce(&Kernel) -> R2 + Send,
) -> Raced<R1, R2> {
    use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
    use std::time::{Duration, Instant};
    let (paused_tx, paused_rx) = std::sync::mpsc::channel();
    let (resume_tx, resume_rx) = std::sync::mpsc::channel();
    let done = AtomicBool::new(false);
    std::thread::scope(|s| {
        let a = s.spawn(|| {
            *p.at.lock().unwrap() = Some(PauseAt {
                thread: std::thread::current().id(),
                kind,
                key: key.to_string(),
                nth,
                paused: paused_tx,
                resume: resume_rx,
            });
            first(k)
        });
        paused_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("the first writer never made its read");
        let b = s.spawn(|| {
            let r = second(k);
            done.store(true, SeqCst);
            r
        });
        let t0 = Instant::now();
        let second_waited = loop {
            if done.load(SeqCst) {
                break false;
            }
            if k.exec_locks().waiting() > 0 {
                break true;
            }
            assert!(
                t0.elapsed() < Duration::from_secs(10),
                "the second writer neither returned nor waited"
            );
            std::thread::yield_now();
        };
        resume_tx.send(()).unwrap();
        Raced {
            first: a.join().unwrap(),
            second: b.join().unwrap(),
            second_waited,
        }
    })
}

/// No dispatched action of a cancelled execution is left without a cancel
/// request, or out of its `outstanding`: nothing would stop it or wait for it.
fn assert_nothing_runs_unasked(w: &World, e: &Execution) {
    for a in w.kernel.actions().unwrap() {
        if a.execution_id == e.id && a.state == ActionState::Dispatched {
            assert!(
                a.cancel.is_some() && e.outstanding.contains(&a.correlation_id),
                "{} is dispatched for cancelled {} with no cancel request ({:?}) or not \
                 outstanding ({:?}): nothing will stop it",
                a.correlation_id,
                e.id,
                a.cancel,
                e.outstanding
            );
        }
    }
}

/// The lost update theseus-id9 was filed for: a turn's commit, read before a
/// cancel's frame was indexed, put `running` back over `cancelled`. Now the
/// cancel waits for the plan's frame, and then cancels the call it dispatched,
/// and the execution stays cancelled across a restart.
#[test]
fn a_turns_commit_never_puts_running_back_over_a_cancel() {
    let (w, p) = pausing_world();
    let (_, e, g) = running(&w);
    // The plan is a transaction, whose first read finds the family (its
    // parent, if any) before it locks, as `lock_family` does: its second read
    // is the plan's own, under the lock.
    let r = race(
        &w.kernel,
        &p,
        (kinds::EXECUTION, &e.id, 2),
        |k| {
            k.plan_and_dispatch(
                &g,
                &proposal("fs.read"),
                RetryClass::SafeToRepeat,
                None,
                100,
                |_| Ok(vec![]),
            )
        },
        |k| k.cancel_execution(&e.id, "operator"),
    );
    let x = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(x.state, ExecState::Cancelled, "the cancel was lost");
    assert_nothing_runs_unasked(&w, &x);
    assert!(r.second_waited, "the cancel waited for the plan's frame");
    let a = r.first.unwrap();
    assert_eq!(r.second.unwrap(), vec![a.correlation_id]);
    drop((g, p));
    let (w, _) = crash(w, KernelConfig::default());
    let x = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(
        x.state,
        ExecState::Cancelled,
        "the index and the WAL's order agree"
    );
}

/// The same race the other way: a cancel, read before a turn's plan was
/// indexed, must not write back an `outstanding` without the call. Now the
/// plan waits, finds the execution cancelled, and plans nothing.
#[test]
fn a_cancel_never_drops_a_call_the_turn_dispatched() {
    let (w, p) = pausing_world();
    let (_, e, g) = running(&w);
    let r = race(
        &w.kernel,
        &p,
        // The cancel's first read learns whether it has a parent (DD7's
        // `lock_family`); its second is the one under the lock.
        (kinds::EXECUTION, &e.id, 2),
        |k| k.cancel_execution(&e.id, "operator"),
        |k| {
            k.plan_and_dispatch(
                &g,
                &proposal("fs.read"),
                RetryClass::SafeToRepeat,
                None,
                100,
                |_| Ok(vec![]),
            )
        },
    );
    let x = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(x.state, ExecState::Cancelled);
    assert_nothing_runs_unasked(&w, &x);
    assert!(r.second_waited);
    let err = r.second.unwrap_err();
    assert!(
        matches!(
            err.downcast_ref::<KernelError>(),
            Some(KernelError::NoTurn {
                state: "cancelled",
                ..
            })
        ),
        "{err}"
    );
    assert!(r.first.unwrap().is_empty());
    assert!(w.kernel.actions().unwrap().is_empty());
}

/// A completion arriving for a waiting execution, and a cancel: the
/// completion's continuation never puts the execution back in the queue over
/// the cancel.
#[test]
fn a_completion_never_revives_a_cancelled_execution() {
    let (w, p) = pausing_world();
    let (_, e, g) = running(&w);
    let a = dispatched(&w, &g, "proc.run", 100);
    w.kernel
        .end_turn(
            g,
            TurnEnd::Wait {
                wake: Wake::Actions {
                    correlation_ids: vec![a.correlation_id.clone()],
                },
            },
        )
        .unwrap();
    let c = completion(&a.correlation_id, Outcome::Succeeded, Some(40));
    let r = race(
        &w.kernel,
        &p,
        // The first read learns whether the execution has a parent (DD7's
        // `lock_family`); the second is the settlement's, under the lock.
        (kinds::EXECUTION, &e.id, 2),
        |k| k.accept_completion(&c),
        |k| k.cancel_execution(&e.id, "operator"),
    );
    let x = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(x.state, ExecState::Cancelled, "the cancel was lost");
    assert!(x.outstanding.is_empty());
    assert!(r.second_waited);
    assert!(matches!(r.first.unwrap(), Accepted::Settled { .. }));
    assert!(r.second.unwrap().is_empty(), "the call had settled");
    assert_eq!(x.budget.spent_micros, 40, "the settlement was kept too");
}

/// `end_turn` never overwrites a terminal state (A2): the check itself is
/// now inside the lock, so a cancel lands before it or after the frame.
#[test]
fn a_turns_end_never_overwrites_a_cancel_that_landed_during_it() {
    let (w, p) = pausing_world();
    let (_, e, g) = running(&w);
    let r = race(
        &w.kernel,
        &p,
        // Read 1 learns whether it has a parent (DD7); read 2 is under the lock.
        (kinds::EXECUTION, &e.id, 2),
        move |k| k.end_turn(g, TurnEnd::Wait { wake: Wake::Input }),
        |k| k.cancel_execution(&e.id, "operator"),
    );
    let x = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(x.state, ExecState::Cancelled, "the cancel was lost");
    assert_eq!(x.wake, None);
    assert!(r.second_waited);
    assert_eq!(r.first.unwrap().state, ExecState::Waiting);
    r.second.unwrap();
}

/// The reconciler's scan finds a due wake, and may be a frame stale: it
/// decides again under the lock, so a cancel since then stands.
#[test]
fn a_due_wake_never_requeues_a_cancelled_execution() {
    let (w, p) = pausing_world();
    let (_, e, g) = running(&w);
    let at_ms = w.clock.now_ms() + 1_000;
    w.kernel
        .end_turn(
            g,
            TurnEnd::Wait {
                wake: Wake::DueAt { at_ms },
            },
        )
        .unwrap();
    w.clock.advance(2_000);
    let r = race(
        &w.kernel,
        &p,
        (kinds::EXECUTION, &e.id, 1),
        |k| k.reconcile(&NoEvidence),
        |k| k.cancel_execution(&e.id, "operator"),
    );
    let x = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(x.state, ExecState::Cancelled, "the cancel was lost");
    assert!(r.second_waited);
    assert_eq!(r.first.unwrap().woke_due, vec![e.id]);
    r.second.unwrap();
}

/// Actions belong to their execution, and its lock covers them: a confirm
/// bound while the operator declines the same call never brings it back.
#[test]
fn a_confirm_never_revives_a_declined_action() {
    let (w, p) = pausing_world();
    let (_, e, g) = running(&w);
    let prop = proposal("fs.write");
    let a = w
        .kernel
        .plan_confirm_with(&g, &prop, RetryClass::NonRepeatable, None, |_| Ok(vec![]))
        .unwrap();
    let corr = a.correlation_id;
    // The action's second read: the first only finds its execution's lock.
    let r = race(
        &w.kernel,
        &p,
        (kinds::ACTION, &corr, 2),
        |k| k.bind_confirm(&corr, "eddie", &prop),
        |k| k.decline_action(&corr, "eddie", "no"),
    );
    let a = w.kernel.action(&corr).unwrap().unwrap();
    assert_eq!(a.state, ActionState::Cancelled, "the decline was lost");
    assert!(r.second_waited);
    assert!(r.first.unwrap().confirm.is_some());
    r.second.unwrap();
    drop(g);
    assert_eq!(
        w.kernel.execution(&e.id).unwrap().unwrap().state,
        ExecState::Running
    );
}

/// Writers of different executions never wait for each other: a turn's
/// cancel goes through while another execution's plan holds its lock.
#[test]
fn writers_of_different_executions_never_wait_for_each_other() {
    let (w, p) = pausing_world();
    let (_, e1, g1) = running(&w);
    let (_, e2, _g2) = running(&w);
    let r = race(
        &w.kernel,
        &p,
        (kinds::EXECUTION, &e1.id, 1),
        |k| {
            k.plan_and_dispatch(
                &g1,
                &proposal("fs.read"),
                RetryClass::SafeToRepeat,
                None,
                100,
                |_| Ok(vec![]),
            )
        },
        |k| k.cancel_execution(&e2.id, "operator"),
    );
    assert!(
        !r.second_waited,
        "the cancel of {} waited for {}'s plan",
        e2.id, e1.id
    );
    r.first.unwrap();
    r.second.unwrap();
    assert_eq!(
        w.kernel.execution(&e1.id).unwrap().unwrap().state,
        ExecState::Running
    );
    assert_eq!(
        w.kernel.execution(&e2.id).unwrap().unwrap().state,
        ExecState::Cancelled
    );
    assert_eq!(w.kernel.exec_locks().held(), 0);
}

/// The push's observer (theseus-in3): nothing is observed until one is
/// installed, and then every frame the kernel or any view of it commits,
/// each record with its WAL position, after the append. Once only.
#[test]
fn the_observer_sees_each_committed_frame_with_its_positions() {
    let w = world();
    let (_, e, _guard) = running(&w);
    assert!(!w.kernel.observed(), "nothing watches yet");
    type Seen = Vec<(Vec<u16>, Vec<u64>)>;
    let seen: Arc<std::sync::Mutex<Seen>> = Arc::default();
    let into = seen.clone();
    assert!(w.kernel.observe(Arc::new(move |c: Committed<'_>| {
        let kinds = c.records.iter().map(|r| r.kind).collect();
        into.lock().unwrap().push((kinds, c.positions.to_vec()));
    })));
    assert!(w.kernel.observed());
    assert!(!w.kernel.observe(Arc::new(|_| {})), "one observer");
    // A view made before or after the install shares it.
    let view = w.kernel.view(w.kernel.store().clone());
    let other = view
        .open_execution(
            &new_id("ses"),
            SessionKind::Conversation,
            auth(),
            None,
            None,
        )
        .unwrap();
    w.kernel.cancel_execution(&e.id, "operator").unwrap();
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 2, "two frames: {seen:?}");
    for (id, (kinds, positions)) in [&other.id, &e.id].into_iter().zip(&seen) {
        assert_eq!(kinds.len(), positions.len());
        let at = kinds.iter().position(|k| *k == kinds::EXECUTION).unwrap();
        let stored = w
            .kernel
            .store()
            .latest_by_key(kinds::EXECUTION, id)
            .unwrap()
            .unwrap();
        assert_eq!(stored.position, positions[at], "the record's own position");
        assert!(positions.windows(2).all(|p| p[0] < p[1]));
    }
    // The seed's reads: every record with its position.
    let at: BTreeMap<String, u64> = w
        .kernel
        .executions_at()
        .unwrap()
        .into_iter()
        .map(|(p, e)| (e.id, p))
        .collect();
    assert_eq!(at[&other.id], seen[0].1[0]);
}
