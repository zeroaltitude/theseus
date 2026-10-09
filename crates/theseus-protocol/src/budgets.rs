//! `budget.list` (step 42a, theseus-ext.7; M7 §2.6): where the money is. Each
//! open execution's dollar budget, where its limit comes from, its resets
//! and the waiting budget question, with its tasks under it, and the totals.
//! `theseus budgets` and the cockpit's Budgets tab (42b) read it. It reads
//! records (the open executions, their sessions, their places), and the
//! ledger only by one bounded page per execution that was reset.

use serde::{Deserialize, Serialize};

/// `budget.list`'s answer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BudgetListResult {
    /// The open executions that are no task of another open one, oldest
    /// first, each with its tasks.
    pub executions: Vec<BudgetRow>,
    pub totals: BudgetTotals,
    /// `[kernel] spend_limit_usd`: an execution whose limit is `config`'s
    /// follows it.
    pub config_limit_usd: f64,
    /// The judge's shadow budget for the local day (`[judge]`), a budget of
    /// its own that no session's spends from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub judge: Option<JudgeDayBudget>,
    /// The daemon's day ceiling over every model call (theseus-kp20).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub day_ceiling: Option<DayCeilingBudget>,
}

/// One open execution's money.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BudgetRow {
    pub execution_id: String,
    pub session_id: String,
    /// `conversation` or `task`.
    pub kind: String,
    pub state: String,
    /// The session's title or label, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub title: Option<String>,
    pub limit_usd: f64,
    /// Where the limit comes from: `config` (`[kernel] spend_limit_usd`,
    /// followed when it changes), `place` (a place's ceiling, the lower of
    /// its own and the config's), `carve` (a task's, carved from its
    /// parent), or `pinned` (its own, named when it opened).
    pub limit_from: String,
    /// What names it: the place (`#pier`), or the parent's session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub limit_by: Option<String>,
    /// What reaching the limit does (theseus-usei): `notify` (a notice at
    /// the limit and at each multiple of it, and its calls go on) or `ask`
    /// (it waits on the budget question). Unset from a build before it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub mode: Option<String>,
    /// Settled costs since it opened or was last reset.
    pub spent_usd: f64,
    /// Reserved for calls in flight, its tasks' carves among them.
    pub reserved_usd: f64,
    pub held_unknown_usd: f64,
    pub available_usd: f64,
    /// The session's lifetime cost, which no reset lowers.
    pub lifetime_usd: f64,
    /// Approved resets of the spend to $0.
    pub resets: u32,
    /// The last of them, read from its `budget.reset` row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub last_reset: Option<BudgetResetInfo>,
    /// Why the last reset was not read, when there was one: the ledger's
    /// index is still being built after the start, and a scan of the
    /// history is not this read's to make.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub last_reset_unread: Option<String>,
    /// The budget question waiting for the operator.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub question: Option<BudgetQuestionInfo>,
    /// A task's parent execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub parent: Option<String>,
    /// A task's carve as its parent holds it now: the parent's reservation
    /// for it, which shrinks as the task spends.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub carve_held_usd: Option<f64>,
    /// Its open tasks, oldest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tasks: Vec<BudgetRow>,
}

/// An approved reset, from its `budget.reset` row.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BudgetResetInfo {
    pub at_ms: u64,
    /// Who approved it.
    pub by: String,
    pub spent_before_usd: f64,
}

/// A waiting budget question.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BudgetQuestionInfo {
    /// Its correlation id, which `action.confirm` answers.
    pub correlation_id: String,
    /// What the call waiting on it would reserve.
    pub needs_usd: f64,
}

/// The totals. A task's spend is its parent's too, and its carve is its
/// parent's reservation, so the money figures add the top-level rows only;
/// the lifetime adds every session's, since each session counts its own.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BudgetTotals {
    pub executions: u32,
    pub tasks: u32,
    pub limit_usd: f64,
    pub spent_usd: f64,
    pub reserved_usd: f64,
    pub held_unknown_usd: f64,
    pub available_usd: f64,
    pub lifetime_usd: f64,
    /// Budget questions waiting.
    pub questions: u32,
}

/// The judge's shadow day budget (`[judge] shadow_limit_usd_per_day`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct JudgeDayBudget {
    pub enabled: bool,
    /// The local day, `YYYY-MM-DD`.
    pub day: String,
    pub limit_usd: f64,
    pub spent_usd: f64,
    /// Today's judgments skipped at the limit.
    pub paused: bool,
}

/// The daemon's day ceiling (`[kernel] daily_spend_ceiling_usd`,
/// theseus-kp20): every model call's spend today, against the ceiling.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DayCeilingBudget {
    /// The local day, `YYYY-MM-DD`.
    pub day: String,
    pub ceiling_usd: f64,
    /// Settled and booked today.
    pub spent_usd: f64,
    /// Held by calls in flight.
    pub held_usd: f64,
    /// Whether no model call is made until the day turns.
    pub reached: bool,
    /// When the first call was refused today (unix ms).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub reached_at_ms: Option<u64>,
    /// When the day turns: the next local midnight (unix ms), and its local
    /// time, `2026-10-09 00:00`.
    pub turns_at_ms: u64,
    pub turns_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape on the wire: optional fields absent when unset, a task
    /// under its parent, and the bytes read back to the same value.
    #[test]
    fn a_budget_list_keeps_its_shape_on_the_wire() {
        let task = BudgetRow {
            execution_id: "exe_b2".into(),
            session_id: "ses_b2".into(),
            kind: "task".into(),
            state: "running".into(),
            limit_usd: 2.0,
            limit_from: "carve".into(),
            limit_by: Some("ses_a1".into()),
            spent_usd: 0.5,
            available_usd: 1.5,
            lifetime_usd: 0.5,
            parent: Some("exe_a1".into()),
            carve_held_usd: Some(1.5),
            ..Default::default()
        };
        let r = BudgetListResult {
            executions: vec![BudgetRow {
                execution_id: "exe_a1".into(),
                session_id: "ses_a1".into(),
                kind: "conversation".into(),
                state: "waiting".into(),
                limit_usd: 100.0,
                limit_from: "config".into(),
                spent_usd: 3.0,
                reserved_usd: 1.5,
                available_usd: 95.5,
                lifetime_usd: 7.0,
                resets: 1,
                last_reset: Some(BudgetResetInfo {
                    at_ms: 1_759_300_000_000,
                    by: "cli".into(),
                    spent_before_usd: 4.0,
                }),
                question: Some(BudgetQuestionInfo {
                    correlation_id: "act_q1".into(),
                    needs_usd: 0.25,
                }),
                tasks: vec![task],
                ..Default::default()
            }],
            totals: BudgetTotals {
                executions: 1,
                tasks: 1,
                limit_usd: 100.0,
                spent_usd: 3.0,
                reserved_usd: 1.5,
                available_usd: 95.5,
                lifetime_usd: 7.5,
                questions: 1,
                ..Default::default()
            },
            config_limit_usd: 100.0,
            judge: None,
            day_ceiling: None,
        };
        let text = serde_json::to_string(&r).unwrap();
        assert_eq!(
            text,
            r#"{"executions":[{"execution_id":"exe_a1","session_id":"ses_a1","kind":"conversation","state":"waiting","limit_usd":100.0,"limit_from":"config","spent_usd":3.0,"reserved_usd":1.5,"held_unknown_usd":0.0,"available_usd":95.5,"lifetime_usd":7.0,"resets":1,"last_reset":{"at_ms":1759300000000,"by":"cli","spent_before_usd":4.0},"question":{"correlation_id":"act_q1","needs_usd":0.25},"tasks":[{"execution_id":"exe_b2","session_id":"ses_b2","kind":"task","state":"running","limit_usd":2.0,"limit_from":"carve","limit_by":"ses_a1","spent_usd":0.5,"reserved_usd":0.0,"held_unknown_usd":0.0,"available_usd":1.5,"lifetime_usd":0.5,"resets":0,"parent":"exe_a1","carve_held_usd":1.5}]}],"totals":{"executions":1,"tasks":1,"limit_usd":100.0,"spent_usd":3.0,"reserved_usd":1.5,"held_unknown_usd":0.0,"available_usd":95.5,"lifetime_usd":7.5,"questions":1},"config_limit_usd":100.0}"#
        );
        let back: BudgetListResult = serde_json::from_str(&text).unwrap();
        assert_eq!(back, r);
    }
}
