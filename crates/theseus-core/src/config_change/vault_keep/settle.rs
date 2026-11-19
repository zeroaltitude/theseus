//! A keep card settles (issue 26): the owner's value was given and the daemon
//! tried to write it. One frame holds the answer's fact, the question's
//! settlement and the ledger row beside it, as a settings change's does; the
//! card closes with the words the owner was told, by name. None of it holds the
//! value, which was gone from the daemon's hands before any of this began.

use anyhow::Result;
use theseus_kernel::Action;

use super::done::{self, Outcome};
use super::target::Target;
use crate::fact::answer::CallAnswered;
use crate::outbox::Closed;
use crate::rpc::Core;
use crate::turn::OPERATOR;

impl Core {
    /// Settle `a`, the keep card, on `outcome`. A yes that was acted on (the
    /// vault took a value, checked or not) is approved; nothing written is a
    /// refusal, with its reason. The ledger row `vault.keep` is the daemon's
    /// own, written beside.
    pub(crate) fn settle_keep(
        &self,
        a: &Action,
        t: &Target,
        outcome: Outcome,
        by: &str,
        via: &str,
    ) -> Result<()> {
        let words = done::words(t, outcome);
        let approve = !matches!(outcome, Outcome::NotWritten(_) | Outcome::Changed);
        let session = Some(a.session_id.as_str());
        let corr = a.correlation_id.as_str();
        let answered = CallAnswered {
            action: a,
            approve,
            note: Some(&words),
            by,
            via,
            option: Some(if approve { "approve" } else { "decline" }),
        };
        let rows = vec![crate::fact::row(&answered, session, None)?];
        let proposal = a.proposal.clone().unwrap_or_default();
        self.kernel.frame(&[&a.execution_id], |k| {
            match approve {
                true => k.bind_confirm(corr, OPERATOR, &proposal).map(drop)?,
                false => k.decline_action(corr, by, &words).map(drop)?,
            }
            k.stage(&rows)
        })?;
        self.session_rec(&a.session_id).announce(&answered);
        let how = if approve { "approved" } else { "declined" };
        self.card_closed(
            corr,
            Closed {
                note: Some(words),
                ..Closed::new(how, Some(by))
            },
        );
        Ok(())
    }
}
