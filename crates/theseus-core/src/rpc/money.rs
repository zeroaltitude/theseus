//! `execution.reopen` and `action.settle` (issue 6; theseus-x3m9, theseus-cv3v): the owner's two words on
//! money the kernel holds or has stopped (`theseus_kernel::owner_money`). Each is judged as every owner's
//! act is (the owner, from a private place or their own shell; never an agent's process), refused by the
//! CLI inside a job, and changes no default. Routed with the session words (`sessions::OWN`).

use serde_json::Value;
use theseus_kernel::usd_to_micros;
use theseus_protocol::money::{
    ActionSettleParams, ActionSettleResult, ExecutionReopenParams, ExecutionReopenResult,
};
use theseus_protocol::{error_code, method};

use super::server::{route, Conn, RpcFailure};
use super::Core;
use crate::approval::Refusal;
use crate::rpc::Act;

impl Core {
    pub(super) fn rpc_money(
        &self,
        m: &str,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        let who = conn.answerer(None, None);
        match m {
            method::EXECUTION_REOPEN => route(params, |p: ExecutionReopenParams| {
                self.execution_reopen(&p, &who).map_err(money_refused)
            }),
            _ => route(params, |p: ActionSettleParams| {
                self.action_settle(&p, &who).map_err(money_refused)
            }),
        }
    }

    /// Reopen an execution that ended at its dollar limit, with a new limit above its spend.
    pub fn execution_reopen(
        &self,
        p: &ExecutionReopenParams,
        who: &crate::approval::Answerer,
    ) -> anyhow::Result<ExecutionReopenResult> {
        let what = format!(
            "Reopening {} with a limit of ${:.2}",
            p.execution_id, p.limit_usd
        );
        self.judge_act(
            who,
            Act::Session {
                method: method::EXECUTION_REOPEN,
                what: &what,
            },
        )?;
        if !(p.limit_usd.is_finite() && p.limit_usd > 0.0) {
            anyhow::bail!("the new limit must be a dollar amount above zero");
        }
        let was = self
            .kernel
            .execution(&p.execution_id)?
            .ok_or_else(|| anyhow::anyhow!("no execution {}", p.execution_id))?
            .budget
            .limit_micros;
        let e = self.kernel.reopen_exhausted(
            &p.execution_id,
            usd_to_micros(p.limit_usd),
            &who.who(),
        )?;
        self.admission.notify_waiters();
        Ok(ExecutionReopenResult {
            execution_id: e.id,
            spent_usd: theseus_kernel::micros_to_usd(e.budget.spent_micros),
            was_limit_usd: theseus_kernel::micros_to_usd(was),
            limit_usd: theseus_kernel::micros_to_usd(e.budget.limit_micros),
        })
    }

    /// Settle a call held as unknown spend at what it cost, or forgive it (no `usd`).
    pub fn action_settle(
        &self,
        p: &ActionSettleParams,
        who: &crate::approval::Answerer,
    ) -> anyhow::Result<ActionSettleResult> {
        let what = match p.usd {
            Some(u) => format!("Settling {} at ${u:.2}", p.correlation_id),
            None => format!("Forgiving {}", p.correlation_id),
        };
        self.judge_act(
            who,
            Act::Session {
                method: method::ACTION_SETTLE,
                what: &what,
            },
        )?;
        if p.usd.is_some_and(|u| !(u.is_finite() && u >= 0.0)) {
            anyhow::bail!("what the call cost must be a dollar amount, zero or more");
        }
        let cost = p.usd.map_or(0, usd_to_micros);
        let (a, held) = self
            .kernel
            .settle_held_spend(&p.correlation_id, cost, &who.who())?;
        self.admission.notify_waiters();
        self.step_after_owner_settled(&a);
        Ok(ActionSettleResult {
            correlation_id: a.correlation_id,
            execution_id: a.execution_id,
            usd: theseus_kernel::micros_to_usd(cost),
            held_usd: theseus_kernel::micros_to_usd(held),
            forgiven: p.usd.is_none(),
        })
    }
}

/// A refusal as the owner's words answer one; any other error is the params'.
fn money_refused(e: anyhow::Error) -> RpcFailure {
    match e.downcast::<Refusal>() {
        Ok(r) => RpcFailure {
            code: error_code::REFUSED,
            message: format!(
                "a word on money from {} does not count: {}. Nothing changed.",
                r.who, r.why
            ),
            data: serde_json::json!({"who": r.who, "via": r.via, "why": r.why}),
        },
        Err(e) => RpcFailure::invalid(e),
    }
}
