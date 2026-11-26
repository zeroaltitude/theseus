//! A plan question's other yes (theseus-lyxy); the offer and its words are
//! `policy::grants::plan_offer` and `theseus_protocol::grants`.
//!
//! - The offer on a question: `grant_offer_of`, computed from the call's
//!   stored gate record the way the turn computed it when it asked, so
//!   `confirm.list`, a card and the push say the same. Only `plan.present`
//!   offers one: "yes, and move this conversation to edits mode".
//! - The answer: `action.confirm { grant }` (`confirm_granting`). The daemon
//!   recomputes its own offer and takes the answer only when it echoes that
//!   offer's pattern, so a client cannot widen it; the plan's own tool moves
//!   the conversation once the call runs.
//!
//! A scoped grant ("yes, and don't ask again" for a command's prefix or for
//! edits in the project) used to ride the same answer; it is gone, and its
//! `policy.granted` and `policy.grant_ended` rows stay in old ledgers.

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use theseus_protocol::grants::{GrantAnswer, GrantOffer};
use theseus_protocol::modes::SessionMode;
use theseus_protocol::GateRecord;

use super::Core;
use crate::approval::Answerer;
use crate::node::{Body, Node};
use crate::session::SessionRecord;

impl Core {
    /// The offer of a question for a tool call, from its stored gate record.
    pub(super) fn grant_offer_of(
        &self,
        session: &SessionRecord,
        tool: &str,
        _input: &Value,
        gate: Option<&GateRecord>,
    ) -> Option<GrantOffer> {
        let decision = gate?.decision.as_ref()?;
        let class = self.runner.class_of(&session.session_id);
        let approve = decision.posture.as_deref() == Some("approve");
        crate::policy::grants::plan_offer(
            tool,
            class,
            approve,
            decision.class.as_deref() == Some("l1"),
        )
        .or_else(|| {
            crate::policy::grants::hold_offer(
                tool,
                class,
                approve,
                decision.floor,
                &decision.reason,
            )
        })
    }

    /// `action.confirm` with `grant`: approve the call and take its offer.
    pub fn confirm_granting(
        &self,
        correlation_id: &str,
        note: Option<&str>,
        who: Answerer,
        answer: &GrantAnswer,
    ) -> Result<theseus_protocol::ActionConfirmResult> {
        let a = self
            .kernel
            .action(correlation_id)?
            .ok_or_else(|| anyhow!("no action {correlation_id}"))?;
        let rec = self
            .store
            .get_session::<SessionRecord>(&a.session_id)?
            .ok_or_else(|| anyhow!("no session {}", a.session_id))?;
        let nodes = self.store.session_nodes(&a.session_id)?;
        let Some((tool, input, gate)) = call_of(&nodes, correlation_id) else {
            bail!("{correlation_id} is no tool call's question: it offers no grant");
        };
        let Some(offered) = self.grant_offer_of(&rec, &tool, &input, gate.as_deref()) else {
            bail!("{correlation_id} offers no grant: approve it without one. Nothing changed.");
        };
        if answer.pattern != offered.pattern {
            bail!(
                "the grant answered (`{}`) is not the one offered (`{}`). Nothing changed.",
                answer.pattern,
                offered.pattern
            );
        }
        if offered.kind == theseus_protocol::grants::KIND_CLEAR {
            return self.confirm_clearing(&rec, correlation_id, note, who, &offered);
        }
        // A plan's second yes is a mode: the plan's own tool moves the
        // conversation once the call runs (theseus-lyxy).
        let modes = &self.tools.modes;
        modes.pick(correlation_id, SessionMode::Edits);
        let done = self.confirm_action_with(correlation_id, true, note, who, Default::default());
        if done.is_err() {
            modes.unpick(correlation_id);
        }
        done
    }
}

/// One "Approve and clear" at a time, so a check that the question waits and the effects that
/// follow are not split by another answer to it.
static CLEARING: std::sync::Mutex<()> = std::sync::Mutex::new(());

impl Core {
    /// "Approve and clear" (issue 54). Nothing happens unless the question still waits: a stale
    /// or replayed answer changes nothing. The flag the offer was made for (one raised before the
    /// question was asked) is captured; its removal is judged first, as `/trust` and the notice's
    /// Remove are judged (`Act::Unlabel`), so a press that may not remove it changes nothing; then
    /// the call is approved; then that flag, and only that flag, is removed: its identity is
    /// compared inside the session's own mutation (`unlabel_session_if`), so a flag raised since
    /// stays, and the owner is told so. The approved call's result is the boundary the hold counts
    /// from, as if the owner had just spoken.
    fn confirm_clearing(
        &self,
        rec: &SessionRecord,
        correlation_id: &str,
        note: Option<&str>,
        who: Answerer,
        offered: &GrantOffer,
    ) -> Result<theseus_protocol::ActionConfirmResult> {
        let _one = CLEARING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let a = self
            .kernel
            .action(correlation_id)?
            .ok_or_else(|| anyhow!("no action {correlation_id}"))?;
        if a.state != theseus_kernel::ActionState::Planned {
            bail!(
                "action {correlation_id} is {}, not waiting for confirmation: nothing changed",
                a.state.as_str()
            );
        }
        let flag = match offered.pattern == theseus_protocol::grants::CLEAR_FLAG {
            true => self.offered_flag(&rec.session_id, a.planned_at_ms)?,
            false => None,
        };
        if flag.is_some() {
            let what = format!("Removing injection from session {}", rec.session_id);
            self.judge_act(
                &who,
                super::confirms::Act::Unlabel {
                    session: &rec.session_id,
                    what: &what,
                },
            )?;
        }
        let holds = &self.tools.holds;
        holds.clear_on(correlation_id);
        let done =
            self.confirm_action_with(correlation_id, true, note, who.clone(), Default::default());
        if done.is_err() {
            holds.unclear(correlation_id);
            return done;
        }
        let Some(expected) = flag else {
            return done;
        };
        #[cfg(test)]
        between_hook(&rec.session_id);
        let injection = crate::content::Flag::Injection;
        match self.unlabel_session_if(&rec.session_id, injection, &who, &expected) {
            Ok(Some(_)) => {}
            Ok(None) => self.tell_flag_left(&rec.session_id),
            Err(e) => tracing::warn!(error = %format!("{e:#}"), "the cleared flag was not removed"),
        }
        done
    }
}

impl Core {
    /// The injection flag the session carries, when it was raised no later than `asked_ms`: the
    /// one a question asked then offered to clear.
    fn offered_flag(&self, session: &str, asked_ms: u64) -> Result<Option<crate::content::Label>> {
        let rec = self
            .store
            .get_session::<SessionRecord>(session)?
            .ok_or_else(|| anyhow!("no session {session}"))?;
        Ok(rec
            .data
            .as_ref()
            .and_then(|d| d.label(crate::content::Flag::Injection))
            .filter(|l| l.at_ms <= asked_ms)
            .cloned())
    }

    /// The owner is told an "Approve and clear" left a flag: the call was approved, and the flag
    /// the session carries now is not the one the card offered to clear. A note that cannot be
    /// staged is logged.
    fn tell_flag_left(&self, session: &str) {
        let text = crate::owner_note::note(
            crate::owner_note::Kind::Fyi,
            "Approve and clear left a newer flag",
            &format!("In conversation {}", crate::narrative::short(session)),
            "The call was approved. The flag on this conversation now is not the one the card \
             offered to clear, so it stays: /trust or the flag's Remove clears it.",
        );
        match self
            .outbox
            .stage_to_operator(Some(session), json!({"kind": "notice", "text": text}))
        {
            Ok((post, frame)) => match self.store.append(&frame) {
                Ok(_) => self.outbox.posted(&post),
                Err(e) => {
                    tracing::warn!(error = %format!("{e:#}"), "the left flag's note was not kept")
                }
            },
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "the left flag's note was not staged")
            }
        }
    }
}

/// What a test runs between the answer's check and the flag's removal: one per session.
#[cfg(test)]
type Between = Vec<(String, Box<dyn FnOnce() + Send>)>;

#[cfg(test)]
static BETWEEN: std::sync::Mutex<Between> = std::sync::Mutex::new(Vec::new());

#[cfg(test)]
impl Core {
    /// Have `f` run, once, between an "Approve and clear" answer's check and its flag's removal in
    /// `session`: what a test does in that gap.
    pub(crate) fn between_clear_check(&self, session: &str, f: Box<dyn FnOnce() + Send>) {
        BETWEEN.lock().unwrap().push((session.to_string(), f));
    }
}

/// Run what a test set for `session` between the check and the removal, once.
#[cfg(test)]
fn between_hook(session: &str) {
    let hook = {
        let mut all = BETWEEN.lock().unwrap();
        all.iter()
            .position(|(s, _)| s == session)
            .map(|i| all.remove(i).1)
    };
    if let Some(f) = hook {
        f();
    }
}

/// The tool call a question names, from its session's nodes.
fn call_of(
    nodes: &[(u64, Node)],
    correlation_id: &str,
) -> Option<(String, Value, Option<Box<GateRecord>>)> {
    nodes.iter().find_map(|(_, n)| match &n.body {
        Body::ToolCall {
            tool,
            input,
            correlation_id: Some(c),
            gate,
            ..
        } if c == correlation_id => Some((tool.clone(), input.clone(), gate.clone())),
        _ => None,
    })
}
