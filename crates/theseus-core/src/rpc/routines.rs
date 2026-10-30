//! The routines' verbs and their firing (theseus-pvm5c): `routine.create`,
//! `.list`, `.runs`, `.pause`, `.resume`, `.skip`, `.run_now`, `.update` and `.delete`.
//! The verbs are the owner's, acted at once (`judge_act(Act::Routine)`: the
//! owner, from a private place; the CLI refuses them inside a job), and each is
//! one kernel frame with its row. The model's `routine.create` tool
//! (`routine_tool.rs`) raises one approval card first and makes the same
//! routine. `fire_routines_in` is the due scan's routine pass, in the
//! harness's 500 ms tick beside the notices: no new timer.

use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde_json::Value;
use theseus_kernel::{Execution, Kernel, Reschedule, TaskRun};
use theseus_protocol::routines::{
    CatchUp, RoutineCreateParams, RoutineEditResult, RoutineListResult, RoutineNameParams,
    RoutinePauseParams, RoutineRunsParams, RoutineRunsResult, RoutineSpec, RoutineUpdateParams,
    Speak,
};
use theseus_protocol::{error_code, method};

use super::confirms::Act;
use super::server::{reply, route, Conn, RpcFailure};
use super::Core;
use crate::approval::{Answerer, Refusal};
use crate::routine::{self, Env, NewRoutine};
use crate::wake_when::parse_when;

/// The routines' methods, routed by `rpc_prefixed`.
pub(super) const OWN: [&str; 10] = [
    method::ROUTINE_CREATE,
    method::ROUTINE_LIST,
    method::ROUTINE_RUNS,
    method::ROUTINE_PAUSE,
    method::ROUTINE_RESUME,
    method::ROUTINE_SKIP,
    method::ROUTINE_RUN_NOW,
    method::ROUTINE_TRY,
    method::ROUTINE_UPDATE,
    method::ROUTINE_DELETE,
];

/// The newest runs `routine.runs` shows when it is asked for none.
const DEFAULT_RUNS: u32 = 20;

fn invalid(e: impl std::fmt::Display) -> RpcFailure {
    RpcFailure::new(error_code::INVALID_PARAMS, e.to_string())
}

fn failure(e: anyhow::Error) -> RpcFailure {
    match e.downcast::<Refusal>() {
        Ok(r) => RpcFailure {
            code: error_code::REFUSED,
            message: format!(
                "a routine request from {} does not count: {}. Routines are the owner's, from a \
                 private place.",
                r.who, r.why
            ),
            data: serde_json::json!({"who": r.who, "via": r.via, "why": r.why}),
        },
        Err(e) => RpcFailure::invalid(e),
    }
}

impl Core {
    /// The routines' methods, by name.
    pub(super) async fn rpc_routines(
        self: &Arc<Self>,
        name: &str,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        match name {
            method::ROUTINE_CREATE => route(params, |p: RoutineCreateParams| {
                let who = conn.answerer(p.author.clone(), p.discord.clone());
                self.routine_create(&p, who)
            }),
            method::ROUTINE_LIST => reply(self.routine_list()?),
            method::ROUTINE_RUNS => route(params, |p| self.routine_runs(&p)),
            method::ROUTINE_PAUSE => route(params, |p: RoutinePauseParams| {
                let who = conn.answerer(p.author.clone(), p.discord.clone());
                self.routine_pause(&p, who)
            }),
            method::ROUTINE_RESUME => route(params, |p: RoutineNameParams| {
                let who = conn.answerer(p.author.clone(), p.discord.clone());
                self.routine_resume(&p, who)
            }),
            method::ROUTINE_SKIP => route(params, |p: RoutineNameParams| {
                let who = conn.answerer(p.author.clone(), p.discord.clone());
                self.routine_skip(&p, who)
            }),
            method::ROUTINE_RUN_NOW => route(params, |p: RoutineNameParams| {
                let who = conn.answerer(p.author.clone(), p.discord.clone());
                self.routine_run_now(&p, who)
            }),
            method::ROUTINE_TRY => route(params, |p: RoutineNameParams| {
                let who = conn.answerer(p.author.clone(), p.discord.clone());
                self.routine_try(&p, who)
            }),
            method::ROUTINE_UPDATE => route(params, |p: RoutineUpdateParams| {
                let who = conn.answerer(p.author.clone(), p.discord.clone());
                self.routine_update(&p, who)
            }),
            _ => {
                let p: RoutineNameParams = super::server::parse(params)?;
                let who = conn.answerer(p.author.clone(), p.discord.clone());
                reply(self.routine_delete(&p, who).await?)
            }
        }
    }

    fn judge_routine(
        &self,
        who: &Answerer,
        method: &'static str,
        what: &str,
    ) -> Result<(), RpcFailure> {
        self.judge_act(who, Act::Routine { method, what })
            .map_err(failure)
    }

    /// Where a routine's reports go: its owner's DM, nowhere, or a place.
    pub(crate) fn routine_target(&self, spec: &RoutineSpec) -> Option<String> {
        match spec.deliver_to.as_str() {
            "none" => None,
            "dm" => self
                .runner
                .place_rule
                .owner_dm_places()
                .first()
                .map(|p| format!("discord:{p}")),
            other => Some(other.to_string()),
        }
    }

    fn routine_env<'a>(&'a self, target: Option<String>) -> Env<'a> {
        Env {
            kernel: &self.kernel,
            store: &self.store,
            outbox: &self.outbox,
            target,
        }
    }

    fn named(&self, name: &str) -> Result<Arc<RoutineSpec>, RpcFailure> {
        self.runner
            .tools
            .routines
            .find(&self.store, name)
            .map_err(|e| RpcFailure::new(error_code::NOT_FOUND, e))
    }

    fn routine_edited(
        &self,
        spec: &RoutineSpec,
        task: Option<String>,
    ) -> Result<RoutineEditResult, RpcFailure> {
        let env = self.routine_env(None);
        Ok(RoutineEditResult {
            routine: routine::info_of(&env, spec)?,
            task,
        })
    }

    /// `routine.create`.
    pub fn routine_create(
        &self,
        p: &RoutineCreateParams,
        who: impl Into<Answerer>,
    ) -> Result<RoutineEditResult, RpcFailure> {
        let who = who.into();
        self.judge_routine(
            &who,
            method::ROUTINE_CREATE,
            &format!("the routine {}", p.title.trim()),
        )?;
        let new = NewRoutine {
            title: p.title.clone(),
            schedule: p.schedule.clone(),
            brief: p.brief.clone(),
            budget_usd: p.budget_usd,
            month_usd: p.month_usd,
            deliver_to: p.deliver_to.clone(),
            catch_up: p.catch_up.clone(),
            speak: p.speak.clone(),
            start: p.start.clone(),
            stop: p.stop.clone(),
            carry: p.carry.clone(),
            grant: self.checked_grant(&p.grant)?,
        };
        let by = who.label;
        let book = &self.runner.tools.routines;
        let prepared =
            routine::prepare(&self.kernel, &book.cfg, &new, "owner", None).map_err(invalid)?;
        let spec = routine::create(&self.kernel, &self.store, book, prepared, &by)
            .map_err(RpcFailure::invalid)?;
        self.routine_edited(&spec, None)
    }

    /// `routine.list`.
    pub fn routine_list(&self) -> Result<RoutineListResult, RpcFailure> {
        let env = self.routine_env(None);
        let mut routines = Vec::new();
        for s in self.runner.tools.routines.all(&self.store)? {
            routines.push(routine::info_of(&env, &s)?);
        }
        Ok(RoutineListResult { routines })
    }

    /// `routine.runs`: its tasks, newest first.
    pub fn routine_runs(&self, p: &RoutineRunsParams) -> Result<RoutineRunsResult, RpcFailure> {
        let spec = self.named(&p.id)?;
        let env = self.routine_env(None);
        let mut tasks = self.kernel.tasks(Some(&spec.execution_id))?;
        tasks.reverse();
        let n = p.limit.unwrap_or(DEFAULT_RUNS).max(1) as usize;
        let mut runs = Vec::new();
        for t in tasks.iter().take(n) {
            if let Some(r) = routine::run_row(&env, &t.id)? {
                runs.push(r);
            }
        }
        Ok(RoutineRunsResult {
            routine: routine::info_of(&env, &spec)?,
            runs,
        })
    }

    /// `routine.pause`: hold the series, or move it past `until`.
    pub fn routine_pause(
        &self,
        p: &RoutinePauseParams,
        who: impl Into<Answerer>,
    ) -> Result<RoutineEditResult, RpcFailure> {
        let who = who.into();
        let spec = self.named(&p.id)?;
        self.judge_routine(
            &who,
            method::ROUTINE_PAUSE,
            &format!("the pause of routine {}", routine::short(&spec.session_id)),
        )?;
        let kc = self.kernel.config();
        let until = match &p.until {
            Some(u) => {
                let now = self.kernel.now_ms();
                let u = bare_day(u);
                Some(parse_when(&u, now, &kc.zone, kc.wakes.max_ahead_ms).map_err(invalid)?)
            }
            None => None,
        };
        self.kernel
            .pause_wake(&spec.execution_id, &spec.wake_id, until, &who.label)
            .map_err(RpcFailure::invalid)?
            .ok_or_else(|| {
                RpcFailure::new(error_code::NOT_FOUND, "the routine has no schedule left")
            })?;
        self.routine_edited(&spec, None)
    }

    /// `routine.resume`.
    pub fn routine_resume(
        &self,
        p: &RoutineNameParams,
        who: impl Into<Answerer>,
    ) -> Result<RoutineEditResult, RpcFailure> {
        let who = who.into();
        let spec = self.named(&p.id)?;
        self.judge_routine(
            &who,
            method::ROUTINE_RESUME,
            &format!("the resume of routine {}", routine::short(&spec.session_id)),
        )?;
        self.kernel
            .resume_wake(&spec.execution_id, &spec.wake_id, &who.label)
            .map_err(RpcFailure::invalid)?
            .ok_or_else(|| {
                RpcFailure::new(error_code::NOT_FOUND, "the routine has no schedule left")
            })?;
        self.routine_edited(&spec, None)
    }

    /// `routine.skip` (theseus-0oldh): the next occurrence is dropped and the
    /// series goes on at the one after it. No run is opened, so a skip spends
    /// none of a run-count stop, and the routine's `runs` does not move.
    pub fn routine_skip(
        &self,
        p: &RoutineNameParams,
        who: impl Into<Answerer>,
    ) -> Result<RoutineEditResult, RpcFailure> {
        let who = who.into();
        let spec = self.named(&p.id)?;
        self.judge_routine(
            &who,
            method::ROUTINE_SKIP,
            &format!("the skip of routine {}", routine::short(&spec.session_id)),
        )?;
        self.kernel
            .skip_wake(&spec.execution_id, &spec.wake_id, &who.label)
            .map_err(RpcFailure::invalid)?
            .ok_or_else(|| {
                RpcFailure::new(error_code::NOT_FOUND, "the routine has no schedule left")
            })?;
        self.routine_edited(&spec, None)
    }

    /// `routine.run_now`: one run, the series untouched, paused or not.
    pub fn routine_run_now(
        &self,
        p: &RoutineNameParams,
        who: impl Into<Answerer>,
    ) -> Result<RoutineEditResult, RpcFailure> {
        let who = who.into();
        let spec = self.named(&p.id)?;
        self.judge_routine(
            &who,
            method::ROUTINE_RUN_NOW,
            &format!("the run of routine {}", routine::short(&spec.session_id)),
        )?;
        let target = self.routine_target(&spec);
        let env = self.routine_env(target.clone());
        let mut updated = None;
        let fired = self
            .kernel
            .run_routine_now(&spec.execution_id, &spec.wake_id, |_, due| {
                let (plan, next) = routine::plan(&env, &spec, due)?;
                updated = Some(next);
                Ok(plan)
            })
            .map_err(RpcFailure::invalid)?;
        let task = fired.task.as_ref().map(|t| t.session_id.clone());
        let after = updated.unwrap_or_else(|| (*spec).clone());
        self.routine_opened(&fired.task, &target, &after);
        self.routine_edited(&after, task)
    }

    /// `routine.try`: "Try it now" (theseus-52tut), occurrence 0 at once, run 0
    /// in the log, the series untouched. Any question it asks goes where a
    /// run's does, and the owner who ran it is watching.
    pub fn routine_try(
        &self,
        p: &RoutineNameParams,
        who: impl Into<Answerer>,
    ) -> Result<RoutineEditResult, RpcFailure> {
        let who = who.into();
        let spec = self.named(&p.id)?;
        self.judge_routine(
            &who,
            method::ROUTINE_TRY,
            &format!("the trial of routine {}", routine::short(&spec.session_id)),
        )?;
        let target = self.routine_target(&spec);
        let env = self.routine_env(target.clone());
        let mut updated = None;
        let fired = self
            .kernel
            .run_routine_now(&spec.execution_id, &spec.wake_id, |_, due| {
                let (plan, next) = routine::plan_trial(&env, &spec, due)?;
                updated = Some(next);
                Ok(plan)
            })
            .map_err(RpcFailure::invalid)?;
        let task = fired.task.as_ref().map(|t| t.session_id.clone());
        let after = updated.unwrap_or_else(|| (*spec).clone());
        self.routine_opened(&fired.task, &target, &after);
        self.routine_edited(&after, task)
    }

    /// The routine's grant once `p` has changed it, and whether it did: the
    /// whole `grant` replaces, `add_grant` joins; all checked.
    fn grant_after(
        &self,
        spec: &RoutineSpec,
        p: &RoutineUpdateParams,
    ) -> Result<(Vec<String>, bool), RpcFailure> {
        let have = self
            .kernel
            .execution(&spec.execution_id)?
            .as_ref()
            .and_then(routine::written_grant)
            .unwrap_or_default();
        let wanted = routine::grant::joined(p.grant.as_ref().unwrap_or(&have), &p.add_grant);
        if wanted == have {
            return Ok((have, false));
        }
        Ok((self.checked_grant(&wanted)?, true))
    }

    /// A grant as written, checked against the tools this daemon has.
    fn checked_grant(&self, written: &[String]) -> Result<Vec<String>, RpcFailure> {
        let rt = &self.runner.tools;
        routine::grant::check(written, &|t| rt.registry.get(t).is_some()).map_err(invalid)
    }

    /// After a run's frame: the outbox learns where its report goes, and the
    /// book keeps the new record.
    fn routine_opened(
        &self,
        task: &Option<Execution>,
        target: &Option<String>,
        spec: &RoutineSpec,
    ) {
        if let (Some(t), Some(to)) = (task, target) {
            self.outbox.task_bound(&t.session_id, to);
        }
        if let Some(t) = task {
            // Its end is told once it comes (`routines_tick`).
            self.runner
                .tools
                .routines
                .open
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(t.id.clone(), spec.session_id.clone());
        }
        self.runner.tools.routines.set(spec);
    }

    /// `routine.update`: what is given changes; the series, its brief and its
    /// limits in one frame with the record.
    pub fn routine_update(
        &self,
        p: &RoutineUpdateParams,
        who: impl Into<Answerer>,
    ) -> Result<RoutineEditResult, RpcFailure> {
        let who = who.into();
        let spec = self.named(&p.id)?;
        self.judge_routine(
            &who,
            method::ROUTINE_UPDATE,
            &format!("the edit of routine {}", routine::short(&spec.session_id)),
        )?;
        let book = &self.runner.tools.routines;
        let kc = self.kernel.config();
        let now = self.kernel.now_ms();
        let mut next = (*spec).clone();
        let mut change = Reschedule::default();
        if let Some(t) = p.title.as_deref().map(str::trim) {
            if t.is_empty() || t.contains('\n') {
                return Err(invalid("a routine's title is one line"));
            }
            next.title = t.to_string();
        }
        if let Some(b) = p.brief.as_deref().map(str::trim) {
            if b.is_empty() {
                return Err(invalid("a routine needs a brief"));
            }
            next.brief = b.to_string();
        }
        if let Some(r) = p.budget_usd {
            next.run_micros = theseus_kernel::usd_to_micros(r);
        }
        if let Some(m) = p.month_usd {
            next.month_micros = theseus_kernel::usd_to_micros(m);
            change.limit_micros = Some(next.month_micros);
        }
        if next.run_micros == 0 || next.month_micros == 0 || next.run_micros > next.month_micros {
            return Err(invalid(
                "a run's budget must be above 0 and no more than the month's",
            ));
        }
        if let Some(c) = &p.catch_up {
            next.catch_up =
                CatchUp::parse(c).ok_or_else(|| invalid("catch_up is auto, one, or none"))?;
        }
        if let Some(s) = &p.speak {
            next.speak =
                Speak::parse(s).ok_or_else(|| invalid("speak is always or when_notable"))?;
        }
        if let Some(d) = &p.deliver_to {
            next.deliver_to = match d.trim() {
                "" | "dm" => "dm".into(),
                "none" => "none".into(),
                p if p.starts_with("discord:") => p.to_string(),
                other => {
                    return Err(invalid(format!(
                        "deliver_to is dm, none, or a place, not `{other}`"
                    )))
                }
            };
        }
        if let Some(s) = &p.schedule {
            let parsed = theseus_kernel::cron::parse_schedule(s, &kc.zone, now, kc.min_repeat_ms)
                .map_err(invalid)?;
            next.schedule = s.trim().to_string();
            next.period_ms = parsed
                .repeat
                .next_after(&kc.zone, parsed.first_ms)
                .map_or(parsed.repeat.every.nominal_ms(), |n| n - parsed.first_ms);
            change.schedule = Some((parsed.first_ms, parsed.repeat));
        }
        let (grant, grant_changed) = self.grant_after(&spec, p)?;
        if p.brief.is_some() || p.budget_usd.is_some() || grant_changed {
            change.task = Some(TaskRun {
                brief: next.brief.clone(),
                limit_micros: next.run_micros,
                tools: grant,
            });
        }
        let row =
            routine::changed_row(&next, "updated", &who.label).map_err(RpcFailure::invalid)?;
        let extra = vec![
            routine::spec_record(&next).map_err(RpcFailure::invalid)?,
            row,
        ];
        if change.schedule.is_some() || change.task.is_some() || change.limit_micros.is_some() {
            self.kernel
                .reschedule_routine(&spec.execution_id, &spec.wake_id, change, &who.label, extra)
                .map_err(RpcFailure::invalid)?;
        } else {
            self.store.append(&extra).map_err(RpcFailure::invalid)?;
        }
        book.set(&next);
        self.routine_edited(&next, None)
    }

    /// `routine.delete`: its schedule and any run going end; its record stays
    /// as a tombstone and its runs stay in the record.
    pub async fn routine_delete(
        &self,
        p: &RoutineNameParams,
        who: impl Into<Answerer>,
    ) -> Result<RoutineEditResult, RpcFailure> {
        let who = who.into();
        let spec = self.named(&p.id)?;
        self.judge_routine(
            &who,
            method::ROUTINE_DELETE,
            &format!(
                "the deletion of routine {}",
                routine::short(&spec.session_id)
            ),
        )?;
        let info = self.routine_edited(&spec, None)?;
        for t in self.kernel.tasks(Some(&spec.execution_id))? {
            if !t.state.is_terminal() {
                self.cancel_execution(&t.id, &who.label)
                    .await
                    .map_err(RpcFailure::invalid)?;
            }
        }
        if self
            .kernel
            .execution(&spec.execution_id)?
            .is_some_and(|e| !e.state.is_terminal())
        {
            self.cancel_execution(&spec.execution_id, &who.label)
                .await
                .map_err(RpcFailure::invalid)?;
        }
        let mut gone = (*spec).clone();
        gone.deleted_ms = Some(self.kernel.now_ms());
        let row =
            routine::changed_row(&gone, "deleted", &who.label).map_err(RpcFailure::invalid)?;
        self.store
            .append(&[
                routine::spec_record(&gone).map_err(RpcFailure::invalid)?,
                row,
            ])
            .map_err(RpcFailure::invalid)?;
        self.runner.tools.routines.set(&gone);
        Ok(info)
    }

    /// The due scan's routine pass over the executions the `nw` term found:
    /// every routine wake that is due, in any state of its conversation, in one
    /// frame each. Returns how many fired or were skipped. A failure is logged
    /// and tried again on the next tick.
    pub(crate) fn fire_routines_in(&self, execs: &[Execution], now: u64) -> usize {
        // The first tick reads the records, for the commitments block.
        self.runner.tools.routines.warm_once(&self.store);
        let mut n = 0;
        for e in execs.iter().filter(|e| Kernel::routine_due(e, now)) {
            match self.fire_routine_of(e) {
                Ok(k) => n += k,
                Err(err) => tracing::warn!(
                    execution_id = %e.id, error = %format!("{err:#}"),
                    "a due routine could not be fired"
                ),
            }
        }
        n
    }

    fn fire_routine_of(&self, e: &Execution) -> Result<usize> {
        let book = &self.runner.tools.routines;
        let Some(spec) = book.by_execution(&self.store, &e.id) else {
            return Err(anyhow!("it has a routine wake and no routine record"));
        };
        let target = self.routine_target(&spec);
        let env = self.routine_env(target.clone());
        let mut updated: Vec<RoutineSpec> = Vec::new();
        let fired = self.kernel.fire_routines(&e.id, |_, due| {
            let (plan, next) = routine::plan(&env, &spec, due)?;
            updated.push(next);
            Ok(plan)
        })?;
        self.routine_skips_told(&spec, &fired);
        for (f, next) in fired.iter().zip(updated) {
            // The stop has tripped: hold the series, so there is one `stopped`
            // skip and not one an occurrence for ever (theseus-97b8c). The
            // record the frame wrote already says when it tripped.
            if f.skipped == Some("stopped") {
                if let Err(err) = self.kernel.pause_wake(
                    &spec.execution_id,
                    &spec.wake_id,
                    None,
                    &format!(
                        "its stop ({})",
                        next.stop.as_ref().map_or("", |s| s.text.as_str())
                    ),
                ) {
                    tracing::warn!(routine = %routine::short(&spec.session_id),
                        error = %format!("{err:#}"), "a stopped routine could not be held");
                }
            }
            self.routine_opened(&f.task, &target, &next);
            if let Some(a) = &f.post {
                self.outbox.posted(a);
            }
            tracing::info!(
                routine = %routine::short(&spec.session_id),
                run = ?f.task.as_ref().map(|t| t.id.as_str()),
                skipped = ?f.skipped, missed = f.wake.missed, "a routine fired"
            );
        }
        Ok(fired.len())
    }
}

/// `mon` alone means that day from its start; `parse_when` reads `mon 0:00`.
fn bare_day(u: &str) -> String {
    match theseus_kernel::Day::parse(u.trim()) {
        Some(d) => format!("{} 0:00", d.as_str()),
        None => u.to_string(),
    }
}
