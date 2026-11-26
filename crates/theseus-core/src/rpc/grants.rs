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
use serde_json::Value;
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
    /// or replayed answer changes nothing. The flag's removal is judged first, as `/trust` and the
    /// notice's Remove are judged (`Act::Unlabel`), so a press that may not remove it changes
    /// nothing; then the call is approved; then the flag the offer was made for is removed (one
    /// raised before the question was asked, never a newer one) through `content.unlabel` as the
    /// presser. The approved call's result is the boundary the hold counts from, as if the owner
    /// had just spoken.
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
        let clears_flag = offered.pattern == theseus_protocol::grants::CLEAR_FLAG
            && self.offered_flag(&rec.session_id, a.planned_at_ms)?;
        if clears_flag {
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
        // Already judged. Only the flag the offer was made for is removed: if the session lost it
        // since (a `/trust`) and carries a newer one, that one stays.
        if clears_flag && self.offered_flag(&rec.session_id, a.planned_at_ms)? {
            if let Err(e) = self.unlabel_session(&rec.session_id, "injection", who) {
                tracing::warn!(error = %format!("{e:#}"), "the cleared flag was not removed");
            }
        }
        done
    }
}

impl Core {
    /// Whether the session carries an injection flag raised no later than `asked_ms`: the one a
    /// question asked then offered to clear.
    fn offered_flag(&self, session: &str, asked_ms: u64) -> Result<bool> {
        let rec = self
            .store
            .get_session::<SessionRecord>(session)?
            .ok_or_else(|| anyhow!("no session {session}"))?;
        Ok(rec
            .data
            .as_ref()
            .and_then(|d| d.label(crate::content::Flag::Injection))
            .is_some_and(|l| l.at_ms <= asked_ms))
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
