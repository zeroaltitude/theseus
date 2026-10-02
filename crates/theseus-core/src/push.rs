//! The push (theseus-in3, design `stage2` §2): every execution as every
//! surface shows it. `view` builds an `ExecutionView` from the kernel's
//! records, with its `attention` from theseus-protocol's one function, so the
//! web UI, the CLI, the cockpit, and the TUI show the same words.

use theseus_kernel::{Action, ExecState, Execution, Wake, BUDGET_TOOL};
use theseus_protocol::{
    attention, Attention, ExecutionView, GateDecision, PendingConfirm, WaitingOn,
};

/// A time of day on the daemon's clock, `14:00`: how a label says a due time.
pub fn hm(unix_ms: u64) -> String {
    crate::wake::local(unix_ms).hm()
}

/// The kernel's wake, typed for the wire.
pub fn waiting_on(w: &Wake) -> WaitingOn {
    match w {
        Wake::DueAt { at_ms } => WaitingOn::DueAt { at_ms: *at_ms },
        Wake::Actions { correlation_ids } => WaitingOn::Actions {
            correlation_ids: correlation_ids.clone(),
        },
        Wake::Execution { execution_id } => WaitingOn::Execution {
            execution_id: execution_id.clone(),
        },
        Wake::Confirm { confirm_id } => WaitingOn::Confirm {
            confirm_id: confirm_id.clone(),
        },
        Wake::Input => WaitingOn::Input,
        Wake::Budget { correlation_id } => WaitingOn::Budget {
            correlation_id: correlation_id.clone(),
        },
    }
}

/// A question waiting for the operator, in brief: its action, and for a tool
/// call the gate's decision on its node, which holds the reason and the
/// floor. A tool call's question holds `confirm_ttl_ms` from its plan, as
/// `confirm.list` says; a budget question holds until it is answered.
pub fn pending_of(
    a: &Action,
    decision: Option<&GateDecision>,
    confirm_ttl_ms: u64,
) -> PendingConfirm {
    let budget = a.tool == BUDGET_TOOL;
    PendingConfirm {
        correlation_id: a.correlation_id.clone(),
        tool: a
            .proposal
            .as_ref()
            .map(|p| p.tool.clone())
            .unwrap_or_else(|| a.tool.clone()),
        reason: decision.map(|d| d.reason.clone()).unwrap_or_default(),
        floor: decision.is_some_and(|d| d.floor),
        budget,
        expires_at_ms: if budget {
            0
        } else {
            a.planned_at_ms + confirm_ttl_ms
        },
    }
}

/// An execution as every surface shows it, from its record and its
/// questions (a budget question first). `position` and `at_ms` are the
/// frame's it comes from; a list's view has the record's own time.
pub fn view(
    e: &Execution,
    pending: Vec<PendingConfirm>,
    parent_session_id: Option<String>,
    position: u64,
    at_ms: u64,
) -> ExecutionView {
    use theseus_kernel::micros_to_usd as usd;
    let mut v = ExecutionView {
        position,
        at_ms,
        execution_id: e.id.clone(),
        session_id: e.session_id.clone(),
        kind: e.kind,
        parent_session_id,
        state: e.state.as_str().into(),
        previous: None,
        waiting_on: (e.state == ExecState::Waiting)
            .then(|| e.wake.as_ref().map(waiting_on))
            .flatten(),
        pending,
        turns: e.turns,
        spent_usd: usd(e.budget.spent_micros),
        limit_usd: usd(e.budget.limit_micros),
        ended_reason: e.ended_reason.clone(),
        why: None,
        wake_at_ms: e.wakes.iter().map(|w| w.due_at_ms).min(),
        attention: Attention {
            level: theseus_protocol::Level::Idle,
            label: String::new(),
            since_ms: at_ms,
        },
    };
    v.attention = attention(&v, &hm);
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_kernel::{ActionState, Authority, Budget, RetryClass, SessionKind};

    fn execution(state: ExecState, wake: Option<Wake>) -> Execution {
        Execution {
            id: "exe_0000a1b2c3".into(),
            schema: 2,
            session_id: "ses_0000d4e5f6".into(),
            kind: SessionKind::Task,
            state,
            authority: Authority::default(),
            budget: Budget::new(2_000_000),
            wake,
            outstanding: vec![],
            queued_results: vec![],
            parent: Some("exe_parent".into()),
            reports_to: None,
            reports: vec![],
            wakes: vec![],
            wake_parent: false,
            report_wakes: vec![],
            stopped: None,
            turns: 3,
            interrupted: 0,
            resume_pending: false,
            cancel: None,
            ended_reason: None,
            created_at_ms: 1,
            updated_at_ms: 2,
        }
    }

    fn action(tool: &str) -> Action {
        Action {
            correlation_id: format!("cor_{tool}"),
            schema: 2,
            execution_id: "exe_0000a1b2c3".into(),
            session_id: "ses_0000d4e5f6".into(),
            tool: tool.into(),
            args_digest: String::new(),
            proposal: None,
            resource: None,
            retry_class: RetryClass::NonRepeatable,
            state: ActionState::Planned,
            deadline_at_ms: 0,
            planned_at_ms: 1_000,
            authorized_at_ms: None,
            dispatched_at_ms: None,
            settled_at_ms: None,
            external_op_id: None,
            result_ref: None,
            confirm: None,
            cancel: None,
            reservation_id: None,
            reserved_micros: 0,
            resolution: None,
            completions_seen: 0,
            detail: None,
        }
    }

    /// A task parked on a tool call's question: its view says so, with the
    /// gate's reason, the expiry `confirm.list` gives, and its parent.
    #[test]
    fn a_task_parked_on_a_question_needs_you_with_the_gates_reason() {
        let e = execution(
            ExecState::Waiting,
            Some(Wake::Confirm {
                confirm_id: "cor_proc.run".into(),
            }),
        );
        let decision = GateDecision {
            reason: "run cargo test".into(),
            floor: true,
            ..Default::default()
        };
        let p = pending_of(&action("proc.run"), Some(&decision), 300_000);
        assert_eq!(p.expires_at_ms, 301_000);
        let v = view(&e, vec![p], Some("ses_parent".into()), 48, 9);
        assert_eq!(v.attention.level, theseus_protocol::Level::NeedsYou);
        assert_eq!(
            v.attention.label,
            "confirm proc.run: run cargo test · floor"
        );
        assert_eq!(v.attention.since_ms, 9);
        assert_eq!(v.parent_session_id.as_deref(), Some("ses_parent"));
        assert_eq!(
            v.waiting_on,
            Some(WaitingOn::Confirm {
                confirm_id: "cor_proc.run".into()
            })
        );
        assert_eq!((v.spent_usd, v.limit_usd), (0.0, 2.0));
    }

    /// A budget question holds until it is answered; a running execution
    /// keeps no `waiting_on`, though its record may still hold a wake.
    #[test]
    fn a_budget_question_has_no_expiry_and_only_waiting_has_a_wake() {
        let q = pending_of(&action(BUDGET_TOOL), None, 300_000);
        assert!(q.budget && q.expires_at_ms == 0 && q.reason.is_empty());
        let v = view(
            &execution(ExecState::Running, Some(Wake::Input)),
            vec![],
            None,
            1,
            1,
        );
        assert_eq!(v.waiting_on, None);
        assert_eq!(v.attention.label, "turn 3");
    }
}
