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
        crate::policy::grants::plan_offer(
            tool,
            self.runner.class_of(&session.session_id),
            decision.posture.as_deref() == Some("approve"),
            decision.class.as_deref() == Some("l1"),
        )
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
