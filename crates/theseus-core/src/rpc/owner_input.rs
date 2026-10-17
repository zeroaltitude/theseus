//! Whose input a task takes as a steer (theseus-slmj1).

use theseus_protocol::TurnSubmitParams;

use crate::approval::Answerer;
use crate::Core;

impl Core {
    /// Whether this `turn.submit`, made by `who`, is the owner's own typed
    /// input: the owner, from a private place (the judgement `work.answer`
    /// makes; on Discord, `who` carries the origin the binding names in
    /// `speaking`), and neither a job's (`opened_from`) nor a prompt's. A
    /// task's turn takes only this as a steer; anyone else's queues.
    pub(crate) fn owner_input(&self, p: &TurnSubmitParams, who: &Answerer) -> bool {
        p.opened_from.is_none()
            && p.prompt.is_none()
            && crate::places::owner_in_private(who, &self.runner.place_rule, &self.cfg).is_ok()
    }
}

#[cfg(test)]
#[path = "tests_owner_input.rs"]
mod owner_tests;
