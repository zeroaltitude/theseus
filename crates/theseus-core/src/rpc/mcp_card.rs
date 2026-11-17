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

impl Core {
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
}
