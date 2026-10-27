//! `task.not_this`: the owner's "not this" on a task. Three things, in order:
//!
//! - **Cancels the task** with `task.cancel`'s own path (`task_cancel_by`); a
//!   task that has ended is left as it is, and the rest goes on.
//! - **Writes the one rolling preference**, a standing preference for
//!   everywhere whose `source` is `task.not_this`: it replaces the last one
//!   that is still live (`standing.add`'s `replaces`), so there is one, never
//!   a pile. It is keyed on the task's *shape*, what the model said it
//!   started the task for (`because`), else the task's title: that is the
//!   line the next self-started task will say of itself, so the model can
//!   tell the two apart.
//! - **Labels the offer** that produced the task wrong, when the origin
//!   session's ledger holds an acting `offer_task` judgment (the newest one).
//!   A task carries no link to its offer yet, so this is best effort.
//!
//! The owner's, judged as `standing.add` is (`Act::Standing`) before anything
//! is cancelled, so a refusal leaves the task running.

use anyhow::anyhow;
use serde_json::json;
use theseus_protocol::task_not_this::{TaskNotThisParams, TaskNotThisResult, SOURCE};
use theseus_protocol::{error_code, method, StandingAddParams, StandingKind, StandingScope};

use super::confirms::Act;
use super::server::{parse, reply, Conn, RpcFailure};
use super::standing::failure;
use super::Core;
use crate::approval::Answerer;
use crate::ledger::LedgerRow;

/// How many of the ledger's newest rows are read for the task's offer.
const OFFER_SCAN: usize = 2_000;

impl Core {
    /// `task.cancel` and `task.not_this`: the two that stop a task.
    pub(super) async fn task_stop(
        &self,
        m: &str,
        params: serde_json::Value,
        conn: Conn<'_>,
    ) -> Result<serde_json::Value, RpcFailure> {
        match m {
            method::TASK_NOT_THIS => reply(self.rpc_task_not_this(parse(params)?, conn).await?),
            _ => reply(self.task_cancel(parse(params)?, conn).await?),
        }
    }

    async fn rpc_task_not_this(
        &self,
        p: TaskNotThisParams,
        conn: Conn<'_>,
    ) -> Result<TaskNotThisResult, RpcFailure> {
        let who = conn.answerer(p.author.clone(), p.discord.clone());
        self.task_not_this(&p, who).await
    }

    /// Cancel, record the rolling preference, label the offer.
    pub async fn task_not_this(
        &self,
        p: &TaskNotThisParams,
        who: Answerer,
    ) -> Result<TaskNotThisResult, RpcFailure> {
        let what = format!("not this, of the task {}", p.task);
        self.judge_act(
            &who,
            Act::Standing {
                method: method::TASK_NOT_THIS,
                what: &what,
            },
        )
        .map_err(failure)?;
        let cancelled = self
            .task_cancel_by(&p.task, &who.label)
            .await
            .map_err(|e| match e.downcast::<crate::task::NoSuchTask>() {
                Ok(n) => RpcFailure::new(error_code::NOT_FOUND, n.0),
                Err(e) => RpcFailure::invalid(e),
            })?;
        let (shape, offer_session) = self.shape_of(&cancelled.task);
        let text = preference_text(&shape, p.reason.as_deref());
        let (record, replaced) = self.roll_preference(&text, &who)?;
        let labeled = self.label_offer(offer_session.as_deref(), p.reason.as_deref(), &who);
        Ok(TaskNotThisResult {
            task: cancelled.task,
            preference: record,
            replaced,
            labeled,
        })
    }

    /// What kind of work the task was, in one line, and the session its
    /// creating call ran in.
    fn shape_of(&self, task: &theseus_protocol::TaskInfo) -> (String, Option<String>) {
        let id = crate::task_graph::of_session(&task.task_id);
        let rec = crate::task_graph::all(&self.store)
            .ok()
            .and_then(|all| all.into_iter().find(|t| t.id == id));
        let because = rec.as_ref().and_then(|t| t.origin.because.clone());
        let title = rec.as_ref().map(|t| t.title.clone());
        let shape = because
            .or(title.filter(|t| !t.trim().is_empty()))
            .or_else(|| task.title.clone())
            .unwrap_or_else(|| format!("task {}", task.short));
        (shape, rec.map(|t| t.origin.session))
    }

    /// Write `text` as the rolling preference: it ends the live one with the
    /// source `task.not_this`, if any. Two writers that race for the same one
    /// find it gone for the second, which then reads the set again.
    fn roll_preference(
        &self,
        text: &str,
        who: &Answerer,
    ) -> Result<(theseus_protocol::StandingRecord, Option<String>), RpcFailure> {
        let mut last = None;
        for _ in 0..3 {
            let all = self
                .runner
                .standing
                .all(&self.store)
                .map_err(RpcFailure::invalid)?;
            let now = theseus_protocol::now_unix_ms();
            let held = crate::standing::live(&all, None, now)
                .into_iter()
                .find(|r| r.source == SOURCE && r.scope == StandingScope::Everywhere);
            // The same words again leave the one record as it is.
            if let Some(r) = held.filter(|r| r.text == text) {
                return Ok((r.clone(), None));
            }
            let old = held.map(|r| r.id.clone());
            let add = StandingAddParams {
                kind: StandingKind::Preference,
                text: text.to_string(),
                scope: StandingScope::Everywhere,
                source: Some(SOURCE.into()),
                replaces: old.clone(),
                ..Default::default()
            };
            match self.standing_add(&add, who.clone()) {
                Ok(c) => return Ok((c.record, c.ended)),
                Err(e) if old.is_some() => last = Some(e),
                Err(e) => return Err(e),
            }
        }
        Err(last.unwrap_or_else(|| RpcFailure::invalid(anyhow!("the preference was not written"))))
    }

    /// The newest acting `offer_task` judgment of `session`, labelled false
    /// (the owner did not want it). Its id when the label was written.
    fn label_offer(
        &self,
        session: Option<&str>,
        reason: Option<&str>,
        who: &Answerer,
    ) -> Option<String> {
        let session = session?;
        let rows = self.store.ledger_tail::<LedgerRow>(OFFER_SCAN).ok()?;
        let judgment = rows.iter().rev().find_map(|(_, r)| {
            if r.kind != theseus_protocol::LedgerKind::JudgeCall.as_str()
                || r.session_id.as_deref() != Some(session)
            {
                return None;
            }
            let j: theseus_judge::Judgment = serde_json::from_value(r.data.clone()).ok()?;
            crate::judge::inbound::offer_answer(&j).then_some(j.id)
        })?;
        let label = theseus_protocol::learning::JudgeLabelParams {
            judgment: judgment.clone(),
            question: Some(crate::judge::inbound::OFFER_QUESTION.into()),
            label: json!(false),
            note: Some(
                reason
                    .map(|r| format!("not this: {r}"))
                    .unwrap_or_else(|| "not this".into()),
            ),
            discord: None,
        };
        match self.judge_label(&label, who.clone()) {
            Ok(_) => Some(judgment),
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "the offer was not labelled");
                None
            }
        }
    }
}

/// The preference's text: one line, the shape of the work and, when the owner
/// gave one, why. Cut so the whole stays under a record's limit.
fn preference_text(shape: &str, reason: Option<&str>) -> String {
    let one = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut = |s: String, n: usize| s.chars().take(n).collect::<String>();
    let mut text = format!(
        "Not this: the owner turned down a task you started on your own for \"{}\". Do not start \
         another like it.",
        cut(one(shape), 300)
    );
    if let Some(r) = reason.map(one).filter(|r| !r.is_empty()) {
        text.push_str(&format!(" Their reason: {}", cut(r, 400)));
    }
    text
}

#[cfg(test)]
#[path = "tests_task_not_this.rs"]
mod tests;
