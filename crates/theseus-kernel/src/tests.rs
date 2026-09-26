//! Kernel scenarios (§8 standing set, the ones the kernel alone can express).
//! Every test runs against a real `WalStore` in a temp dir under a virtual
//! clock; "crash" means drop the kernel and reopen the store.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::json;
use tempfile::TempDir;
use theseus_store::{Engine, Store, WalConfig, WalStore};

use crate::clock::{Clock, VirtualClock};
use crate::gate::{run_gate, AllowAll, GateResult, Policy, PolicyDecision, Proposal};
use crate::kernel::*;
use crate::spool::Spool;
use crate::types::*;

struct World {
    dir: TempDir,
    clock: Arc<VirtualClock>,
    kernel: Kernel,
    spool: Spool,
}

fn open_store(dir: &std::path::Path) -> Arc<dyn Store> {
    Arc::new(
        WalStore::open(&dir.join("store"), Engine::Redb, WalConfig::default())
            .unwrap()
            .with_checkpoint_every(0),
    )
}

fn world() -> World {
    world_with(KernelConfig::default())
}

fn world_with(cfg: KernelConfig) -> World {
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
fn crash(w: World, cfg: KernelConfig) -> (World, StartupReport) {
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

fn auth() -> Authority {
    Authority {
        principal: "eddie".into(),
        delegated_by: None,
        ceilings: BTreeMap::from([
            ("shell".into(), "l1".into()),
            ("tools".into(), "fs,text".into()),
        ]),
    }
}

fn proposal(tool: &str) -> Proposal {
    Proposal {
        tool: tool.into(),
        args: json!({"a": 1}),
        resource: None,
        policy_context: json!({"binding": 1}),
    }
}

fn completion(id: &str, outcome: Outcome, usage: Option<u64>) -> Completion {
    Completion {
        correlation_id: id.into(),
        outcome,
        result_ref: Some("node_x".into()),
        external_op_id: None,
        started_at_ms: 1,
        finished_at_ms: 2,
        producer: "test".into(),
        signature: None,
        usage_units: usage,
    }
}

/// Open a conversation, wake it with input, take a turn: the common prelude.
fn running(w: &World) -> (Session, Execution, TurnGuard) {
    let (s, e) = w
        .kernel
        .open_session(
            SessionKind::Conversation,
            vec!["chan_1".into()],
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
    (s, e, g)
}

/// plan → gate → authorize → dispatch, returning the dispatched action.
fn dispatched(w: &World, g: &TurnGuard, tool: &str, reserve: u64) -> Action {
    let mut p = proposal(tool);
    let (r, _) = run_gate(&AllowAll, &mut p, &auth());
    assert_eq!(r, GateResult::Allow);
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
    assert_eq!(e2.budget.reserved, 100);

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
    assert_eq!(e4.queued_results, vec![a.correlation_id.clone()]);
    assert_eq!(e4.budget.spent, 40);
    assert_eq!(e4.budget.reserved, 0);

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
    let tail = w.kernel.session_tail(&s.id, 0, 100).unwrap();
    assert!(tail.len() >= 12, "{}", tail.len());
    assert!(tail.windows(2).all(|p| p[0].position < p[1].position));
    let stats = w.kernel.stats().unwrap();
    assert_eq!(stats.executions_by_state["complete"], 1);
    assert_eq!(stats.actions_by_state["succeeded"], 1);
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
    assert_eq!(e4.budget.reserved, 0);
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
    assert_eq!(e2.budget.held_unknown, 200);
    assert_eq!(e2.budget.reserved, 0);
    assert_eq!(e2.budget.spent, 0);
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
    assert_eq!(e3.budget.held_unknown, 0);
    assert_eq!(e3.budget.spent, 120);
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

struct ConfirmProc;
impl Policy for ConfirmProc {
    fn decide(&self, p: &Proposal, _: &Authority) -> PolicyDecision {
        if p.tool.starts_with("proc.") {
            PolicyDecision::Confirm { by: "eddie".into() }
        } else {
            PolicyDecision::Allow
        }
    }
}

#[test]
fn confirm_binds_the_final_action_and_any_change_after_it_invalidates() {
    let w = world();
    let (_, _e, g) = running(&w);
    let mut p = proposal("proc.run");
    let (r, _) = run_gate(&ConfirmProc, &mut p, &auth());
    assert_eq!(r, GateResult::NeedsConfirm { by: "eddie".into() });
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
    // A hook mutation after the confirm: the digest no longer matches.
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

#[test]
fn budget_is_a_hard_limit_and_exhaustion_is_terminal() {
    let w = world();
    let (_, e, g) = running(&w);
    // limit 100_000, control reserve 10_000 => 90_000 available
    let e1 = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(e1.budget.available(), 90_000);
    let _a = dispatched(&w, &g, "fs.read", 80_000);
    let err = w
        .kernel
        .plan_action(
            &g,
            &proposal("fs.read"),
            RetryClass::SafeToRepeat,
            None,
            20_000,
        )
        .unwrap_err();
    assert!(matches!(
        err.downcast_ref::<KernelError>(),
        Some(KernelError::BudgetExhausted {
            needed: 20_000,
            available: 10_000,
            ..
        })
    ));
    let e2 = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(e2.state, ExecState::BudgetExhausted);
    assert!(e2.ended_reason.is_some());
    // Nothing further runs against it.
    assert!(w
        .kernel
        .plan_action(&g, &proposal("fs.read"), RetryClass::SafeToRepeat, None, 1)
        .is_err());
    drop(g);
    assert!(w.kernel.admit(&e.id).is_err());
}

#[test]
fn admission_ceiling_holds_and_cancel_is_honored_immediately() {
    let w = world_with(KernelConfig {
        admission_ceiling: 2,
        ..Default::default()
    });
    let mut execs = Vec::new();
    for _ in 0..3 {
        let (_, e) = w
            .kernel
            .open_session(SessionKind::Task, vec!["task".into()], auth(), None, None)
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
fn promotion_forks_with_derived_authority_and_carved_budget_and_parent_wakes_on_child_end() {
    let w = world();
    let (s, e, g) = running(&w);
    // Widening is refused.
    let widen = BTreeMap::from([("shell".into(), "l0".into())]);
    assert!(w
        .kernel
        .promote(&e.id, vec!["task_1".into()], &widen, 1000, None)
        .is_err());
    let unknown = BTreeMap::from([("network".into(), "all".into())]);
    assert!(w
        .kernel
        .promote(&e.id, vec!["task_1".into()], &unknown, 1000, None)
        .is_err());
    // Same-or-tighter is fine.
    let same = BTreeMap::from([("shell".into(), "l1".into())]);
    let (cs, child) = w
        .kernel
        .promote(
            &e.id,
            vec!["task_1".into()],
            &same,
            20_000,
            Some("child".into()),
        )
        .unwrap();
    assert_eq!(child.kind, SessionKind::Task);
    assert_eq!(child.state, ExecState::Queued);
    assert_eq!(child.parent.as_deref(), Some(e.id.as_str()));
    assert_eq!(child.reports_to.as_deref(), Some(e.id.as_str()));
    assert_eq!(child.authority.delegated_by.as_deref(), Some("eddie"));
    assert_eq!(child.authority.ceilings, auth().ceilings);
    assert_eq!(child.budget.limit, 30_000);
    let parent = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(parent.budget.limit, 70_000);
    // derived_from edge lives in the child session's scope.
    let tail = w.kernel.session_tail(&cs.id, 0, 100).unwrap();
    let edge = tail
        .iter()
        .find(|r| r.kind == theseus_store::kinds::EDGE)
        .expect("edge");
    let v: serde_json::Value = edge.decode().unwrap();
    assert_eq!(v["type"], "derived_from");
    assert_eq!(v["to"], json!(s.id));
    // The conversation continues concurrently and parks waiting on the child.
    w.kernel
        .end_turn(
            g,
            TurnEnd::Wait {
                wake: Wake::Execution {
                    execution_id: child.id.clone(),
                },
            },
        )
        .unwrap();
    let cg = w.kernel.admit(&child.id).unwrap();
    assert_eq!(w.kernel.stats().unwrap().turns_held, 1);
    w.kernel
        .end_turn(
            cg,
            TurnEnd::Complete {
                reason: "child done".into(),
            },
        )
        .unwrap();
    let parent = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(
        parent.state,
        ExecState::Queued,
        "parent woke on child terminal"
    );
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
        .open_session(SessionKind::Task, vec![], auth(), None, None)
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
        let (_, e) = w
            .kernel
            .open_session(SessionKind::Task, vec![format!("t{i}")], auth(), None, None)
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
