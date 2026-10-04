//! `budget.list` (step 42a, theseus-ext.7): where the money is. Each open
//! execution's budget as its record holds it, where its limit comes from,
//! its session's lifetime cost, its last reset, and its waiting question,
//! with its tasks under it; the totals; and the judge's shadow day budget.
//!
//! FAST (§9): it reads records, never the history. The open executions come
//! by their state terms (`Kernel::open_executions`), each session's record
//! and place by key, and a reset's time and approver from one ledger page of
//! one row, by the `budget.reset` kind and the session's tag. While the
//! index's shape is built after a start that page is not there, and the row
//! says so instead of scanning.

use std::collections::BTreeMap;

use theseus_kernel::{micros_to_usd as usd, Execution, Micros};
use theseus_protocol::{
    BudgetListResult, BudgetQuestionInfo, BudgetResetInfo, BudgetRow, BudgetTotals, JudgeDayBudget,
    LedgerKind,
};

use super::server::RpcFailure;
use super::Core;
use crate::session::SessionRecord;

/// The newest rows of a session's resets read for its last: its own, past
/// any of an execution the session had before (a session has one at a time).
const PAGE: usize = 8;

/// Why a reset's row is not read while the index's shape is built.
const UNREAD: &str = "the ledger's index is being built after the start; ask again in a moment";

impl Core {
    /// `budget.list`.
    pub fn budget_list(&self) -> Result<BudgetListResult, RpcFailure> {
        let open = self.kernel.open_executions()?;
        let mut rows: BTreeMap<String, BudgetRow> = BTreeMap::new();
        let mut order: Vec<(u64, String)> = Vec::new();
        for e in &open {
            order.push((e.created_at_ms, e.id.clone()));
            rows.insert(e.id.clone(), self.budget_row(e)?);
        }
        order.sort();
        // Each task under its parent, when the parent is open; a task whose
        // parent has ended is listed on its own.
        let mut top: Vec<String> = Vec::new();
        let mut tasks: Vec<(String, String)> = Vec::new();
        for (_, id) in &order {
            match rows[id].parent.clone() {
                Some(p) if rows.contains_key(&p) => tasks.push((p, id.clone())),
                _ => top.push(id.clone()),
            }
        }
        for (parent, id) in tasks {
            let row = rows.remove(&id).expect("listed above");
            rows.get_mut(&parent).expect("open").tasks.push(row);
        }
        let executions: Vec<BudgetRow> = top.iter().filter_map(|id| rows.remove(id)).collect();
        let totals = totals(&open, &executions);
        let j = self.runner.judge.health();
        Ok(BudgetListResult {
            executions,
            totals,
            config_limit_usd: self.cfg.kernel.spend_limit_usd,
            judge: Some(JudgeDayBudget {
                enabled: j.enabled,
                day: j.day,
                limit_usd: j.shadow_limit_usd,
                spent_usd: j.spend_today_usd,
                paused: j.paused,
            }),
        })
    }

    /// One execution's row, from its record, its session's, and its place.
    fn budget_row(&self, e: &Execution) -> Result<BudgetRow, RpcFailure> {
        let b = &e.budget;
        let session = self.store.get_session::<SessionRecord>(&e.session_id)?;
        let (limit_from, limit_by) = self.limit_source(e);
        let carve_held = e.parent.as_deref().map(|p| {
            self.kernel
                .execution(p)
                .ok()
                .flatten()
                .and_then(|parent| {
                    parent
                        .budget
                        .reservations
                        .get(&theseus_kernel::tasks::carve_key(&e.id))
                        .copied()
                })
                .unwrap_or(0)
        });
        let (last_reset, last_reset_unread) = match b.resets {
            0 => (None, None),
            _ => match self.last_reset(e) {
                Ok(r) => (r, None),
                Err(why) => (None, Some(why)),
            },
        };
        Ok(BudgetRow {
            execution_id: e.id.clone(),
            session_id: e.session_id.clone(),
            kind: e.kind.as_str().into(),
            state: e.state.as_str().into(),
            title: session
                .as_ref()
                .and_then(|s| s.title.clone().or_else(|| s.label.clone())),
            limit_usd: usd(b.limit_micros),
            limit_from: limit_from.into(),
            limit_by,
            spent_usd: usd(b.spent_micros),
            reserved_usd: usd(b.reserved_micros),
            held_unknown_usd: usd(b.held_unknown_micros),
            available_usd: usd(b.available()),
            lifetime_usd: session.as_ref().map_or(0.0, |s| s.cost_usd),
            resets: b.resets,
            last_reset,
            last_reset_unread,
            question: b.question.clone().map(|q| BudgetQuestionInfo {
                correlation_id: q,
                needs_usd: usd(b.question_needs_micros),
            }),
            parent: e.parent.clone(),
            carve_held_usd: carve_held.map(usd),
            tasks: Vec::new(),
        })
    }

    /// Where an execution's limit comes from: a task's carve; the config's,
    /// which it follows (theseus-3pj); a place's ceiling, which pins it
    /// while it caps it (38a); or its own, pinned when it opened.
    fn limit_source(&self, e: &Execution) -> (&'static str, Option<String>) {
        if let Some(p) = &e.parent {
            let parent = self
                .kernel
                .execution(p)
                .ok()
                .flatten()
                .map_or_else(|| p.clone(), |x| x.session_id);
            return ("carve", Some(parent));
        }
        if !e.budget.pinned {
            return ("config", None);
        }
        match self.runner.view_of(&e.session_id).ceiling {
            Some(c) if c.spend_limit_micros.is_some() => ("place", Some(c.place.clone())),
            _ => ("pinned", None),
        }
    }

    /// The execution's last `budget.reset` row: one ledger page of the
    /// session's newest few, by kind and session. `Err` while the index's shape is built.
    fn last_reset(&self, e: &Execution) -> Result<Option<BudgetResetInfo>, String> {
        let page = theseus_store::Page {
            kind: theseus_store::kinds::LEDGER,
            tags: super::methods::ledger_tags(
                Some(LedgerKind::BudgetReset.as_str()),
                Some(&e.session_id),
            ),
            after: None,
            before: None,
            since_ms: None,
            until_ms: None,
            limit: PAGE,
        };
        let out = match self.store.ledger_page(&page) {
            Ok(Some(out)) => out,
            Ok(None) => return Err(UNREAD.into()),
            Err(err) => return Err(format!("its row was not read: {err:#}")),
        };
        for r in out.records.iter().rev() {
            let row: crate::ledger::LedgerRow = r
                .decode()
                .map_err(|err| format!("its row did not decode: {err:#}"))?;
            // A session's rows of an execution before this one are not its.
            if row.data["execution_id"].as_str().is_some_and(|x| x != e.id) {
                continue;
            }
            return Ok(Some(BudgetResetInfo {
                at_ms: row.at_unix_ms,
                by: row.data["by"].as_str().unwrap_or("?").into(),
                spent_before_usd: row.data["spent_before_usd"].as_f64().unwrap_or(0.0),
            }));
        }
        Ok(None)
    }
}

/// The totals: the money of the top-level rows (a task's spend is its
/// parent's, and its carve its parent's reservation), each session's
/// lifetime, and the questions waiting. Added in micro-dollars.
fn totals(open: &[Execution], top: &[BudgetRow]) -> BudgetTotals {
    let by_id: BTreeMap<&str, &Execution> = open.iter().map(|e| (e.id.as_str(), e)).collect();
    let mut t = BudgetTotals::default();
    let (mut limit, mut spent, mut reserved, mut held, mut available): (
        Micros,
        Micros,
        Micros,
        Micros,
        Micros,
    ) = (0, 0, 0, 0, 0);
    let mut lifetime = 0.0;
    for row in top {
        t.executions += 1;
        if let Some(e) = by_id.get(row.execution_id.as_str()) {
            let b = &e.budget;
            limit += b.limit_micros;
            spent += b.spent_micros;
            reserved += b.reserved_micros;
            held += b.held_unknown_micros;
            available += b.available();
        }
        for r in std::iter::once(row).chain(&row.tasks) {
            lifetime += r.lifetime_usd;
            t.questions += u32::from(r.question.is_some());
        }
        t.tasks += row.tasks.len() as u32;
    }
    t.limit_usd = usd(limit);
    t.spent_usd = usd(spent);
    t.reserved_usd = usd(reserved);
    t.held_unknown_usd = usd(held);
    t.available_usd = usd(available);
    t.lifetime_usd = lifetime;
    t
}
