//! `execution.reopen` and `action.settle` through the core (issue 6; theseus-x3m9, theseus-cv3v): the owner's
//! words are judged as every owner's act is: a stranger's is refused and ledgered with nothing changed, the
//! owner's reopens an execution that ended at its limit.

use std::sync::Arc;

use theseus_kernel::{ExecState, Execution};
use theseus_protocol::money::{ActionSettleParams, ExecutionReopenParams};

use super::tests::test_core;
use super::tests_self_backlog::{cli, stranger};
use super::Core;
use crate::approval::Refusal;
use crate::test_support::rows;

/// An execution that ended at its dollar limit (spent $5 of $5).
fn exhausted(core: &Core) -> Execution {
    let e = core
        .kernel
        .open_execution(
            &format!("ses_{}", theseus_kernel::new_id("x")),
            theseus_kernel::SessionKind::Conversation,
            theseus_kernel::Authority {
                principal: crate::turn::OPERATOR.into(),
                ..Default::default()
            },
            None,
            None,
        )
        .unwrap();
    let mut e = core.kernel.execution(&e.id).unwrap().unwrap();
    e.state = ExecState::BudgetExhausted;
    e.wake = None;
    e.ended_reason = Some("the budget is exhausted".into());
    e.budget.limit_micros = theseus_kernel::usd_to_micros(5.0);
    e.budget.spent_micros = theseus_kernel::usd_to_micros(5.0);
    core.store
        .append(&[
            theseus_store::NewRecord::json(theseus_store::kinds::EXECUTION, Some(&e.id), &e)
                .unwrap()
                .scoped(&e.session_id),
        ])
        .unwrap();
    core.kernel.execution(&e.id).unwrap().unwrap()
}

#[tokio::test]
async fn the_owner_reopens_an_execution_at_its_limit_and_a_stranger_cannot() {
    let core: Arc<Core> = test_core("ok");
    let e = exhausted(&core);
    let ask = |limit_usd: f64| ExecutionReopenParams {
        execution_id: e.id.clone(),
        limit_usd,
    };
    let refused = core.execution_reopen(&ask(10.0), &stranger()).unwrap_err();
    assert!(refused.downcast_ref::<Refusal>().is_some(), "{refused:#}");
    assert_eq!(
        core.kernel.execution(&e.id).unwrap().unwrap().state,
        ExecState::BudgetExhausted,
        "nothing changed"
    );
    assert_eq!(
        rows(&core, &e.session_id, "approval.refused")
            .iter()
            .filter(|r| r["act"] == "execution.reopen")
            .count(),
        1
    );
    // The same limit is no room: refused in words.
    let err = core.execution_reopen(&ask(5.0), &cli()).unwrap_err();
    assert!(err.to_string().contains("must be above what"), "{err:#}");
    let r = core.execution_reopen(&ask(10.0), &cli()).unwrap();
    assert_eq!(
        (r.spent_usd, r.was_limit_usd, r.limit_usd),
        (5.0, 5.0, 10.0)
    );
    let now = core.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(now.state, ExecState::Waiting);
    assert!(now.budget.pinned);
    // No default changed: another execution still has the config's limit.
    let other = core
        .kernel
        .open_execution(
            "ses_other",
            theseus_kernel::SessionKind::Conversation,
            theseus_kernel::Authority {
                principal: crate::turn::OPERATOR.into(),
                ..Default::default()
            },
            None,
            None,
        )
        .unwrap();
    assert_eq!(
        other.budget.limit_micros,
        core.kernel.config().spend_limit_micros
    );
}

#[tokio::test]
async fn a_strangers_settle_is_refused_before_the_call_is_looked_at() {
    let core: Arc<Core> = test_core("ok");
    for usd in [Some(1.0), None] {
        let e = core
            .action_settle(
                &ActionSettleParams {
                    correlation_id: "corr_nothing".into(),
                    usd,
                },
                &stranger(),
            )
            .unwrap_err();
        assert!(e.downcast_ref::<Refusal>().is_some(), "{e:#}");
    }
    // The owner's, for a call that is not there, is the params' error, in words.
    let e = core
        .action_settle(
            &ActionSettleParams {
                correlation_id: "corr_nothing".into(),
                usd: None,
            },
            &cli(),
        )
        .unwrap_err();
    assert!(e.downcast_ref::<Refusal>().is_none(), "{e:#}");
}
