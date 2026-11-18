//! Who answers the card of an attached or removed MCP server (issue 22): the
//! owner in person. An agent's process (Claude Code, Codex, anything
//! `peer_proc` names) is refused whatever its surface, a Discord channel is
//! refused as a place, and the card keeps waiting. It is the narrower rule
//! `places::owner_in_person_of` is, which a repository's trust takes too, asked
//! of a question the place rule alone would let an agent on the CLI answer: it
//! changes what Theseus runs and reaches.
//!
//! This is a check made after `judge_act`, which stays as it was: it can only
//! refuse what `judge_act` let through.

use anyhow::Result;
use theseus_kernel::Action;

use super::confirms::Act;
use super::Core;
use crate::approval::{Answerer, Refusal};

/// The META key that keeps the agent whose process the session's last input
/// came through, so a turn that continues after an approval or a wait knows it.
pub(crate) fn agent_input_key(session_id: &str) -> String {
    format!("mcp.agent_input.{session_id}")
}

impl Core {
    /// Keep (or forget) the agent behind a session's input. A write only when
    /// the answer changes, so an ordinary turn writes nothing.
    pub(super) fn note_agent_input(&self, session_id: &str, agent: Option<&str>) {
        let key = agent_input_key(session_id);
        let kept = self.store.get_meta::<String>(&key).ok().flatten();
        let done = match (agent, kept.as_deref()) {
            (Some(a), k) if k != Some(a) => self.store.put_meta(&key, &a.to_string()),
            (None, Some(_)) => self.store.delete_meta(&key),
            _ => Ok(()),
        };
        if let Err(e) = done {
            tracing::warn!(error = %e, "the agent behind a session's input was not kept");
        }
    }

    /// `Err` when `who` may not answer `a`, a card of this kind; the refusal
    /// is ledgered and narrated as any refused answer is.
    pub(super) fn judge_mcp_card(&self, who: &Answerer, a: &Action, approve: bool) -> Result<()> {
        if !crate::config_change::mcp_attach::is_card(a) {
            return Ok(());
        }
        let verdict = crate::places::owner_in_person_of(
            who,
            &self.runner.place_rule,
            &self.cfg,
            "attaching or removing an MCP server changes what Theseus runs and reaches, so only \
             the owner answers it",
            "an MCP server is attached or removed",
        );
        let Err(why) = verdict else {
            return Ok(());
        };
        let r = Refusal {
            who: who.who(),
            via: who.via(),
            why,
        };
        self.refused(Act::Answer { action: a, approve }, who, &r)?;
        Err(r.into())
    }
    /// `Err` when a yes typed or spoken as words (what the model's answer
    /// tool carries, `Rides::typed`) answers a question that is only the
    /// owner's own: a settings question (a change, an undo, an attached or
    /// removed server, a restart, an install) or a call the floor holds (it
    /// reaches a protected file or program, held for the moment a turn may have
    /// been steered). Those are answered by the owner's own press, or their own
    /// terminal, never by a conversation in their name, whoever's message
    /// started the turn. A no from words changes nothing and is allowed. Held
    /// here, in the one place an answer becomes a decision, so it holds for any
    /// future caller.
    pub(super) fn judge_typed_settings(
        &self,
        who: &Answerer,
        a: &Action,
        approve: bool,
        typed: bool,
    ) -> Result<()> {
        if !(approve && typed) {
            return Ok(());
        }
        let why = if crate::config_change::words::is_settings_action(a) {
            crate::config_change::words::BY_PRESS
        } else if self.is_floor_hold(a) {
            crate::config_change::words::FLOOR_BY_PRESS
        } else {
            return Ok(());
        };
        let r = Refusal {
            who: who.who(),
            via: who.via(),
            why: why.to_string(),
        };
        self.refused(Act::Answer { action: a, approve }, who, &r)?;
        Err(r.into())
    }

    /// Whether `a` is a call the floor holds: the gate's decision on its node.
    fn is_floor_hold(&self, a: &Action) -> bool {
        let nodes = self.store.session_nodes(&a.session_id).unwrap_or_default();
        super::confirms::call_of(&nodes, &a.correlation_id)
            .0
            .is_some_and(|d| d.floor)
    }
}
