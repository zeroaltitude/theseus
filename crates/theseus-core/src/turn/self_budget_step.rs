//! The self budget at the model call (theseus-pw1q.3, seam S2). A turn of a
//! session whose task has `origin.principal == "self"` reserves each model
//! call's worst case on the self budget (`crate::rsi::budget`: $10 a branch,
//! $30 a day), beside the kernel's own reservation, and settles it at what
//! the call cost once the call is over, whatever its outcome. At a line the
//! call is not planned: the turn fails with class `self_budget` and the
//! budget's words, which is the reply the work was serving, and the budget
//! has written its `self.budget.stopped` row. A session with no task of its
//! own counts when its execution's principal is `"self"` (the backlog starts
//! an item so). Any session's branch, or none, is found once and kept: a
//! call pays one map lookup after its session's first.
//!
//! A turn's other model calls (a compaction's summary, in the compile before
//! the call) join its cost before the call is reserved: the account keeps the
//! turn's cost it last settled at, so each reservation also holds what the
//! turn spent since, and each settle counts it (a join fix: they were not
//! counted).

use theseus_kernel::{micros_to_usd, Micros};

use super::*;
use crate::rsi::budget::{Reservation, Reserved, Stopped};

/// The failure's class when a self line stopped a call.
pub const SELF_BUDGET_CLASS: &str = "self_budget";

/// A reservation held across one model call, and the turn's cost it counts
/// from.
pub(super) struct Hold {
    reservation: Reservation,
    cost_before: Option<f64>,
}

/// A turn's self account: the hold on the call in flight, and the turn's cost
/// as of the last settle (`None` once a cost is unknown).
pub(super) struct Acct {
    hold: Option<Hold>,
    counted: Option<f64>,
}

impl Default for Acct {
    fn default() -> Self {
        Self {
            hold: None,
            counted: Some(0.0),
        }
    }
}

impl TurnRunner {
    /// Before the call is planned: reserve its worst case when the turn is a
    /// self task's. `Err` is the failure that ends the turn: the call was
    /// not made.
    pub(super) fn self_reserved(
        &self,
        t: &mut Turn<'_>,
        reserve: Micros,
    ) -> std::result::Result<(), Failure> {
        let branch = match t.tc.task {
            Some(_) => self.self_budget.branch_of(t.tc.session_id),
            // A session with no task of its own, opened under the "self"
            // principal: the backlog's start (seam S3, a join fix).
            None => self.self_budget.branch_of_principal(t.tc.session_id, || {
                crate::task_graph::principal_of(t.tc.kernel, t.tc.execution_id)
            }),
        };
        let Some(branch) = branch else {
            return Ok(());
        };
        // What the turn spent since the last settle (a compaction's call) is
        // held with the call's worst case, and counted at its settle.
        let since = match (t.cost, t.self_acct.counted) {
            (Some(now), Some(counted)) => (now - counted).max(0.0),
            _ => 0.0,
        };
        let usd = micros_to_usd(reserve) + since;
        match self.self_budget.reserve("turn", Some(&branch), usd) {
            Ok(Reserved::Granted(reservation)) => {
                t.self_acct.hold = Some(Hold {
                    reservation,
                    cost_before: t.self_acct.counted,
                });
                Ok(())
            }
            Ok(Reserved::Stopped(s)) => Err(Self::self_stopped(t, &s)),
            Err(e) => Err(Failure {
                class: "kernel".into(),
                transient: false,
                usage_unknown: false,
                reason: format!("the self budget could not reserve: {e:#}"),
                source: e,
            }),
        }
    }

    fn self_stopped(t: &mut Turn<'_>, s: &Stopped) -> Failure {
        t.record(&fact::turn::LoopCut {
            decision: SELF_BUDGET_CLASS,
        });
        Failure {
            class: SELF_BUDGET_CLASS.into(),
            transient: false,
            usage_unknown: false,
            reason: SELF_BUDGET_CLASS.into(),
            source: anyhow::anyhow!("{s}"),
        }
    }

    /// After the call, whatever came of it: settle the hold at what the call
    /// cost. A call whose cost is unknown, and a fault, settle at the worst
    /// case, never at zero.
    pub(super) fn self_settled(&self, t: &mut Turn<'_>, called: &Result<Called>) {
        let Some(hold) = t.self_acct.hold.take() else {
            return;
        };
        t.self_acct.counted = t.cost;
        let worst = hold.reservation.usd();
        let spent = match called {
            Ok(Called::Failed(f)) if f.usage_unknown => worst,
            Ok(_) => match (t.cost, hold.cost_before) {
                (Some(now), Some(before)) => (now - before).max(0.0),
                _ => worst,
            },
            Err(_) => worst,
        };
        if let Err(e) = self.self_budget.settle(hold.reservation, spent) {
            // Left open at its worst case: a restart settles it.
            tracing::warn!(error = %format!("{e:#}"), "the self budget could not settle a call");
        }
    }
}
