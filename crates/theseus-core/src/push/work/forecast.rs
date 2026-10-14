//! A task's forecast on the work board (theseus-zz752). The numbers come from
//! the daemon's table of finished tasks (`crate::forecast::Book`, a memory
//! read): said once when the task first shows, kept in its inputs, and carried
//! on its view with whether it has passed its p90 and Jev's latest read of it.
//! A task that finishes adds its row to the table. Nothing here writes a
//! frame, a record or a timer: the board's own frames are the clock, so a
//! task that passes its p90 between two frames is told at the second.

use theseus_protocol::forecast::WorkForecast;
use theseus_protocol::work::{WorkKind, WorkView};

use super::{Dirty, ExecIn, WorkBoard};
use crate::forecast::{Outcome, Row};
use crate::ledger::LedgerRow;
use crate::Core;

/// The states a finished task ends in. A cancelled task did not finish.
fn finished(state: &str) -> bool {
    matches!(state, "complete" | "failed" | "budget_exhausted")
}

impl WorkBoard {
    /// Say the forecast of a task not yet forecast, when the table is read:
    /// a task whose forecast is a number or a count of too few keeps it.
    pub(super) fn ensure_forecast(&mut self, core: &Core, exec: &str) {
        let Some(ei) = self.execs.get(exec) else {
            return;
        };
        if ei.kind != WorkKind::Task
            || matches!(ei.forecast, Some(Outcome::Some(_) | Outcome::Few(_)))
        {
            return;
        }
        // The task's record: the board's, else the graph's (the frame that
        // opened the task may reach the board before its record is read).
        if !self.task_of_exec.contains_key(exec) {
            let session = ei.x.session_id.clone();
            match crate::task_graph::get(&core.store, &crate::task_graph::of_session(&session)) {
                Ok(Some(t)) => {
                    self.task_of_exec.insert(exec.to_string(), t);
                }
                _ => return,
            }
        }
        let outcome = core.tools.forecasts.for_task(&self.task_of_exec[exec]);
        if let Some(ei) = self.execs.get_mut(exec) {
            ei.forecast = Some(outcome);
        }
    }

    /// A task view's forecast, from its inputs: the numbers, Jev's read, and
    /// whether the task has been past its p90 (this run's memory).
    pub(super) fn forecast_of(&self, ei: &ExecIn) -> Option<WorkForecast> {
        let mut w = ei.forecast?.forecast()?.wire();
        w.past = self.past.contains(&ei.x.execution_id);
        if let Some((read, p)) = &ei.read {
            w.read = Some(read.clone());
            w.read_p = Some(*p);
        }
        Some(w)
    }

    /// Whether a task view is past its forecast's p90, of dollars or of
    /// minutes, whichever came first, at `at_ms`: sticky once true, so a
    /// reset budget or a seed after a restart never says it twice.
    pub(super) fn mark_past(&mut self, v: &mut WorkView, at_ms: u64) {
        if v.kind != WorkKind::Task {
            return;
        }
        let Some(fc) = v.forecast.as_mut() else {
            return;
        };
        let ran_from = self.execs.get(&v.id).map_or(0, |e| e.ran_from_ms);
        let mins = if ran_from > 0 {
            at_ms.saturating_sub(ran_from) as f64 / 60_000.0
        } else {
            0.0
        };
        let spent = v.cost.map_or(0.0, |c| c.spent_usd);
        let over = !v.state.ended()
            && ((fc.usd_p90 > 0.0 && spent > fc.usd_p90)
                || (fc.min_p90 > 0.0 && mins > fc.min_p90));
        if over {
            self.past.insert(v.id.clone());
        }
        fc.past = self.past.contains(&v.id);
    }

    /// A task's end: its row joins the table, once.
    pub(super) fn note_ended(&mut self, core: &Core, exec: &str) {
        let Some(ei) = self.execs.get_mut(exec) else {
            return;
        };
        if ei.kind != WorkKind::Task || ei.recorded || !finished(&ei.x.state) {
            return;
        }
        ei.recorded = true;
        let Some(t) = self.task_of_exec.get(exec) else {
            return;
        };
        let started = if ei.ran_from_ms > 0 {
            ei.ran_from_ms
        } else {
            ei.created_at_ms
        };
        let min = ei.x.at_ms.saturating_sub(started) as f64 / 60_000.0;
        if min > crate::forecast::MAX_MINUTES {
            return;
        }
        core.tools.forecasts.ended(Row {
            task: t.id.clone(),
            class: crate::forecast::class_of(t).to_string(),
            terms: crate::forecast::terms_of(&t.title, &t.objective),
            usd: ei.x.spent_usd,
            min,
            loops: Some(ei.loops_total),
            started_ms: started,
            ended_ms: ei.x.at_ms,
        });
    }

    /// A `loop.v1` judgment of a task's session: its latest read of the task.
    pub(super) fn take_read(&mut self, r: &LedgerRow, d: &mut Dirty) {
        let Some(id) = r
            .session_id
            .as_deref()
            .and_then(|s| self.by_session.get(s))
            .cloned()
        else {
            return;
        };
        let pack = r.data["pack"].as_str().unwrap_or("");
        if !pack.starts_with("loop.") || r.data["outcome"]["outcome"].as_str() != Some("answered") {
            return;
        }
        let work = r.data["answers"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|a| a["question"].as_str() == Some("work_state"));
        let Some(a) = work.map(|a| &a["answer"]) else {
            return;
        };
        let (Some(choice), Some(p)) = (a["choice"].as_str(), a["confidence"].as_f64()) else {
            return;
        };
        if let Some(ei) = self.execs.get_mut(&id) {
            if ei.kind == WorkKind::Task {
                ei.read = Some((choice.to_string(), p as f32));
                d.execs.insert(id);
            }
        }
    }

    /// After the table is read: the forecast of each task that is open, in
    /// place and at the time the warm-up ran. A view it changes is sent
    /// again with the next frame (`stale`), like a job's usual time.
    pub(in crate::push) fn forecast_open(&mut self, core: &Core, at_ms: u64) {
        let open: Vec<String> = self
            .execs
            .iter()
            .filter(|(_, e)| {
                e.kind == WorkKind::Task && !finished(&e.x.state) && e.x.state != "cancelled"
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in open {
            self.ensure_forecast(core, &id);
            let Some(mut new) = self.exec_view(&id) else {
                continue;
            };
            self.mark_past(&mut new, at_ms);
            self.refresh_silent(new);
        }
    }
}
