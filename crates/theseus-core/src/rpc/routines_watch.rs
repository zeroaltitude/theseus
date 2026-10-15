//! The ends of a routine's runs (theseus-52tut; the Core's half of
//! `routine/watch.rs`): a question the run asked outside its grant that
//! nobody answered by the run's deadline ends the run **blocked**, naming the
//! tool; a run that fails or is blocked is told as one Interrupt a routine a
//! local day, with the streak; a run skipped, or missed while the daemon was
//! down, informs. The harness's 500 ms tick calls `routines_tick`: it reads
//! only the runs not yet settled and the asks the expiry scan queued, so a
//! daemon with no routine pays two empty-collection tests.
//!
//! `work.answer` on a routine's notice (`answer_routine`) takes Run now,
//! Pause, Details, and Add to grant for a blocked one.

use std::sync::Arc;

use anyhow::Result;
use serde_json::json;
use theseus_kernel::{Action, RoutineFired};
use theseus_protocol::answer::WorkAnswerParams;
use theseus_protocol::answer::WorkAnswerResult;
use theseus_protocol::notices::{routine_answers, routine_line, routine_pings};
use theseus_protocol::routines::{
    RoutineBlocked, RoutineNameParams, RoutinePauseParams, RoutineRunsParams, RoutineSpec,
    RoutineUpdateParams, BLOCKED_PREFIX,
};
use theseus_protocol::{error_code, WakeAnswer};
use theseus_store::{kinds, NewRecord};

use super::server::RpcFailure;
use super::Core;
use crate::approval::Answerer;
use crate::fact::wake::RoutineNoticed;
use crate::routine::watch::{self, Outcome};
use crate::routine::{self, Env};

impl Core {
    /// What a run's question may wait: the run's deadline for a routine's run,
    /// else the confirm lifetime.
    pub(crate) fn ask_ttl_ms(&self, session_id: &str, is_task: bool) -> u64 {
        watch::ask_ttl_ms(
            &self.store,
            self.runner.tools.routines.cfg.run_deadline_secs,
            self.kernel.config().confirm_ttl_ms,
            session_id,
            is_task,
        )
    }

    /// An expired question of a routine's run goes to the tick, which ends the
    /// run blocked; false for any other question.
    pub(crate) fn queue_if_routine(&self, a: &Action) -> bool {
        if !watch::is_run(&self.store, &a.session_id) {
            return false;
        }
        self.runner
            .tools
            .routines
            .blocked
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(a.clone());
        true
    }

    /// The harness's tick: the reminders due, then the routines' runs that
    /// ended or went unanswered. One call so the driver's loop stays short.
    pub(crate) async fn fire_notices_and_routines(&self) {
        self.fire_notices();
        self.routines_tick().await;
    }

    /// The tick: ask the daemon about nothing it has no routine for.
    pub(crate) async fn routines_tick(&self) {
        let book = &self.runner.tools.routines;
        if book.held().is_none() {
            return;
        }
        self.routine_seed_once();
        let asks = std::mem::take(&mut *book.blocked.lock().unwrap_or_else(|e| e.into_inner()));
        for a in asks {
            if let Err(e) = self.routine_block(&a).await {
                tracing::warn!(error = %format!("{e:#}"), correlation_id = %a.correlation_id,
                    "a routine's unanswered question could not end its run; tried again");
                book.blocked
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(a);
            }
        }
        let open: Vec<(String, String)> = book
            .open
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|(a, b)| (a.clone(), b.clone()))
            .collect();
        for (task, routine) in open {
            match self.routine_settle(&task, &routine) {
                Ok(true) => {
                    book.open
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .remove(&task);
                }
                Ok(false) => {}
                Err(e) => tracing::warn!(error = %format!("{e:#}"), task = %task,
                    "a routine's run could not be settled; tried again"),
            }
        }
    }

    /// After a start, the runs that were going or not yet told: each routine's
    /// last run, unless the ping book says it was settled.
    fn routine_seed_once(&self) {
        let book = &self.runner.tools.routines;
        if book.seeded.swap(true, std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        let Some(specs) = book.held() else { return };
        let mut open = book.open.lock().unwrap_or_else(|e| e.into_inner());
        for s in specs.values() {
            if let Some(last) = &s.last_task {
                if watch::ping(&self.store, &s.session_id).settled != *last {
                    open.insert(last.clone(), s.session_id.clone());
                }
            }
        }
    }

    /// A question a routine's run asked outside its grant is past the run's
    /// deadline: the run ends `blocked`, naming the tool, in one record with
    /// its row; then it is cancelled (its question and calls answered not
    /// run), and the next settle tells the owner.
    async fn routine_block(&self, a: &Action) -> Result<()> {
        let tool = blocked_entry(a);
        let why = watch::blocked_reason(&tool);
        let mark = RoutineBlocked {
            tool: tool.clone(),
            reason: why.clone(),
        };
        let mut records = vec![NewRecord::json(
            kinds::META,
            Some(&format!("{BLOCKED_PREFIX}{}", a.session_id)),
            &mark,
        )?];
        if let Some(spec) = self.routine_of_run(&a.session_id) {
            records.push(routine::changed_row(
                &spec,
                &format!("run blocked: {why}"),
                "routine",
            )?);
        }
        self.store.append(&records)?;
        self.cancel_execution(&a.execution_id, "routine:blocked")
            .await?;
        Ok(())
    }

    /// The routine a run's task session belongs to.
    fn routine_of_run(&self, task_session: &str) -> Option<Arc<RoutineSpec>> {
        let mark: theseus_protocol::routines::RunMark = self
            .store
            .get_meta(&format!(
                "{}{task_session}",
                theseus_protocol::routines::RUN_PREFIX
            ))
            .ok()
            .flatten()?;
        self.runner
            .tools
            .routines
            .held()?
            .get(&mark.routine)
            .cloned()
    }

    /// One run that may have ended. `Ok(true)`: it is settled and leaves the
    /// open set; `Ok(false)`: still going.
    fn routine_settle(&self, task_exec: &str, routine: &str) -> Result<bool> {
        let Some(e) = self.kernel.execution(task_exec)? else {
            return Ok(true);
        };
        if !e.state.is_terminal() {
            return Ok(false);
        }
        let book = &self.runner.tools.routines;
        let Some(spec) = book.held().and_then(|m| m.get(routine).cloned()) else {
            return Ok(true);
        };
        let before = watch::ping(&self.store, &spec.session_id);
        if before.settled == task_exec {
            return Ok(true);
        }
        let mut next = before.clone();
        next.settled = task_exec.to_string();
        let outcome = watch::outcome(&self.store, &e);
        let (blocked, why) = match &outcome {
            Outcome::Failed(r) => (false, r.clone()),
            Outcome::Blocked(b) => (true, b.reason.clone()),
            _ => {
                self.store
                    .append(&[watch::ping_record(&spec.session_id, &next)?])?;
                return Ok(true);
            }
        };
        let target = self.routine_target(&spec);
        let env = Env {
            kernel: &self.kernel,
            store: &self.store,
            outbox: &self.outbox,
            target: target.clone(),
        };
        let streak = watch::streak(&env, &spec);
        let now = self.kernel.now_ms();
        let today = theseus_kernel::cron::local(&self.kernel.config().zone, now, "%Y-%m-%d");
        let pings = routine_pings(Some(before.day.as_str()).filter(|d| !d.is_empty()), &today);
        let line = routine_line(&spec.title, blocked, streak, &why);
        let word = if blocked { "blocked" } else { "failed" };
        let mut records = vec![routine::changed_row(
            &spec,
            &format!(
                "run {word} (streak {streak}){}",
                if pings { "" } else { ", told today" }
            ),
            "routine",
        )?];
        let mut post = None;
        if pings {
            next.day = today;
            if let Some(t) = &target {
                let body = notice_body(&spec, &line, blocked, now);
                let (a, more) = self
                    .outbox
                    .stage(&spec.session_id, &spec.execution_id, t, body)?;
                records.extend(more);
                post = Some(a);
            }
        }
        records.push(watch::ping_record(&spec.session_id, &next)?);
        self.store.append(&records)?;
        if pings {
            if let Some(a) = &post {
                self.outbox.posted(a);
            }
            self.routine_announce(
                &spec,
                if blocked {
                    "routine_blocked"
                } else {
                    "routine_failed"
                },
                &line,
                now,
                target.as_deref(),
                routine_answers(blocked),
            );
        }
        Ok(true)
    }

    /// A routine entry's `wake.noticed` to every connection.
    fn routine_announce(
        &self,
        spec: &RoutineSpec,
        purpose: &'static str,
        line: &str,
        at_ms: u64,
        posted_to: Option<&str>,
        answers: Vec<WakeAnswer>,
    ) {
        let fact = RoutineNoticed {
            spec,
            wake_id: &spec.wake_id,
            purpose,
            line,
            at_ms,
            posted_to,
            answers,
        };
        self.session_rec(&spec.session_id).announce(&fact);
    }

    /// Fired routines that were skipped, or caught up after downtime, inform
    /// (the ping rule, H-6 and H-7): a badge, no DM post.
    pub(crate) fn routine_skips_told(&self, spec: &RoutineSpec, fired: &[RoutineFired]) {
        for f in fired {
            let (purpose, text) = match (f.skipped, f.wake.while_down || f.wake.missed > 0) {
                (Some("catch_up"), _) => (
                    "routine_missed",
                    format!(
                        "🔁 {}: missed while the daemon was down; it runs at its next time",
                        spec.title
                    ),
                ),
                (Some(why), _) => (
                    "routine_skipped",
                    format!(
                        "🔁 {}: this run was skipped ({})",
                        spec.title,
                        skip_words(why)
                    ),
                ),
                (None, true) => (
                    "routine_missed",
                    format!(
                        "🔁 {}: {} missed while the daemon was down",
                        spec.title,
                        f.wake.missed.max(1)
                    ),
                ),
                (None, false) => continue,
            };
            self.routine_announce(spec, purpose, &text, self.kernel.now_ms(), None, vec![]);
        }
    }

    /// `work.answer` on a routine's failure or block notice (its id is the
    /// routine's wake): Run now, Pause, Details, and Add to grant when the
    /// last run was blocked.
    pub(crate) fn answer_routine(
        &self,
        p: &WorkAnswerParams,
        who: Answerer,
        spec: &Arc<RoutineSpec>,
    ) -> Result<WorkAnswerResult, RpcFailure> {
        let blocked = self.last_blocked(spec);
        let options = routine_answers(blocked.is_some());
        if !options.iter().any(|o| o.id == p.option) {
            return Err(RpcFailure::new(
                error_code::INVALID_PARAMS,
                format!(
                    "`{}` is not an answer to a routine's notice; the answers are: {}",
                    p.option,
                    options
                        .iter()
                        .map(|o| o.id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ));
        }
        let name = RoutineNameParams {
            id: spec.session_id.clone(),
            author: p.author.clone(),
            discord: p.discord.clone(),
        };
        let mut detail = None;
        match p.option.as_str() {
            "run_now" => {
                self.routine_run_now(&name, who)?;
            }
            "pause" => {
                self.routine_pause(
                    &RoutinePauseParams {
                        id: spec.session_id.clone(),
                        until: None,
                        author: p.author.clone(),
                        discord: p.discord.clone(),
                    },
                    who,
                )?;
            }
            "details" => detail = Some(self.routine_details(spec)?),
            _ => {
                let b = blocked.ok_or_else(|| {
                    RpcFailure::new(error_code::INVALID_PARAMS, "the last run was not blocked")
                })?;
                self.routine_update(
                    &RoutineUpdateParams {
                        id: spec.session_id.clone(),
                        add_grant: vec![b.tool],
                        author: p.author.clone(),
                        discord: p.discord.clone(),
                        ..Default::default()
                    },
                    who,
                )?;
            }
        }
        Ok(WorkAnswerResult {
            question_id: spec.wake_id.clone(),
            option: p.option.clone(),
            resumes: None,
            detail,
        })
    }

    /// The block the routine's last run ended with, when it did.
    fn last_blocked(&self, spec: &RoutineSpec) -> Option<RoutineBlocked> {
        let last = spec.last_task.as_ref()?;
        let e = self.kernel.execution(last).ok().flatten()?;
        match watch::outcome(&self.store, &e) {
            Outcome::Blocked(b) => Some(b),
            _ => None,
        }
    }

    /// Details: the latest runs and why the trouble ones ended as they did.
    fn routine_details(&self, spec: &Arc<RoutineSpec>) -> Result<String, RpcFailure> {
        let r = self.routine_runs(&RoutineRunsParams {
            id: spec.session_id.clone(),
            limit: Some(5),
        })?;
        let mut out = format!(
            "{} · {} in a row failed or blocked",
            spec.title, r.routine.streak
        );
        for run in &r.runs {
            out.push_str(&format!(
                "\n#{} {} {}: {}",
                run.run,
                run.started_local,
                run.state,
                run.report.as_deref().unwrap_or("no report")
            ));
        }
        Ok(out)
    }
}

/// The post the courier draws a routine's notice from: the reminder's shape,
/// with the routine's answers.
fn notice_body(spec: &RoutineSpec, line: &str, blocked: bool, now: u64) -> serde_json::Value {
    json!({
        "kind": "reminder", "text": line, "wake_id": spec.wake_id,
        "short": crate::task::short(&spec.wake_id),
        "purpose": if blocked { "routine_blocked" } else { "routine_failed" },
        "due_at_ms": now, "late_ms": 0,
        "answers": routine_answers(blocked),
    })
}

/// What a skipped occurrence's reason says.
fn skip_words(why: &str) -> &str {
    match why {
        "overlap" => "the last run is still going",
        "budget" => "the month's budget is spent",
        "paused" => "it is paused",
        other => other,
    }
}

/// The grant entry a blocked call is added as: the tool, and for a post the
/// place it names (`channel.post #digest`).
fn blocked_entry(a: &Action) -> String {
    let to = a
        .proposal
        .as_ref()
        .and_then(|p| p.args.get("to"))
        .and_then(|v| v.as_str());
    match (a.tool.as_str(), to) {
        (t, Some(to)) if t == crate::glide::POST => format!("{t} {}", to.trim()),
        (t, _) => t.to_string(),
    }
}
