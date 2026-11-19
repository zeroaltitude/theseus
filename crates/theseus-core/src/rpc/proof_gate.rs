//! The owner's second proof at the answer path (issue 22): where a question
//! marked as needing it is approved only on a good code, or on the open window
//! of the answer's own place, and on no other footing.
//!
//! The gate is in [`Core::confirm_action_with`], **the one place an answer
//! becomes a decision**, so every surface that approves (the CLI, the web UI,
//! Discord's card and its form, the chat's answer tool) meets it. A question
//! is marked by the daemon's own record (`owner_proof::mark`), never by
//! anything an answer says. A decline is never gated: it tightens.
//!
//! **A code is never a message.** The chat's answer tool refuses a marked
//! question outright (`chat_refusal`): what a model reads, or relays, is
//! never a valid proof, and a window never lets a model's tool approve in the
//! owner's name.

use theseus_kernel::Action;
use theseus_protocol::owner_proof::ProofCode;
use theseus_protocol::ConfirmRequest;

use super::confirms::Act;
use super::Core;
use crate::approval::{Answerer, Refusal};
use crate::owner_proof::{marked, Answer, Footing, Proof};

impl Core {
    /// `r` with the proof's marks: whether the asker marked it, and fresh.
    pub(crate) fn with_proof_marks(&self, mut r: ConfirmRequest) -> ConfirmRequest {
        if let Some(fresh) = marked(&self.store, &r.correlation_id) {
            r.needs_proof = true;
            r.needs_fresh_proof = fresh;
        }
        r
    }

    /// What an answer from `who`'s place would be asked for, for each
    /// question in `list` that needs the proof: the card's own line.
    pub(crate) fn with_proof_standing(&self, list: &mut [ConfirmRequest], who: &Answerer) {
        let place = who.via();
        for r in list.iter_mut().filter(|r| r.needs_proof) {
            r.proof = Some(self.proofs.standing(&place, r.needs_fresh_proof));
        }
    }

    /// The gate: an approval of `a` by `who`, with `code` if one came.
    /// The footing when the question is marked and the proof was given (a
    /// code, or an open window of this place), kept for the settled card and
    /// the echo; `None` when it asks for none. A refusal changes nothing and is
    /// recorded.
    pub(crate) fn proof_gate(
        &self,
        a: &Action,
        who: &Answerer,
        code: Option<&ProofCode>,
        typed: bool,
    ) -> Result<Option<Footing>, Refusal> {
        let Some(fresh) = marked(&self.store, &a.correlation_id) else {
            return Ok(None);
        };
        let place = who.via();
        // An agent's process is never the owner: the CLI's socket and the web
        // port are open to every process of the owner's user, so a window
        // opened by the owner's code must never cover one, and one can neither
        // carry a code nor spend the owner's tries on it.
        if let Some(agent) = &who.agent {
            self.proofs.note_agent(&place);
            return Err(self.proof_refused(
                a,
                who,
                format!(
                    "this needs the owner's second proof, and an answer from an agent's process \
                     ({agent}) cannot carry it: the owner approves it from their own terminal \
                     or on its card"
                ),
            ));
        }
        // Words are never a proof, and never a card's yes: no code is read
        // from them and no window covers them.
        if typed {
            return Err(self.proof_refused(
                a,
                who,
                format!(
                    "it needs your second proof, and an answer typed or spoken as words cannot \
                     carry it: press Approve on its card and type the code into its form, or \
                     answer with `theseus answer {} approve --code <the six digits>`",
                    a.correlation_id
                ),
            ));
        }
        let answer = Answer {
            code: code.map(ProofCode::expose),
            place: &place,
        };
        let proof = self.proofs.judge(&a.correlation_id, true, fresh, &answer);
        let why = match proof {
            Proof::NotRequired => return Ok(None),
            Proof::Given | Proof::GivenEarlier { .. } => {
                let footing = self.proofs.footing_of(&proof);
                if let Some(f) = &footing {
                    self.proofs.keep_footing(&a.correlation_id, f.clone());
                }
                return Ok(footing);
            }
            // A secret to keep is not approved (a yes carries no value): the code is asked with the
            // value, by the form or the terminal.
            Proof::Missing if crate::config_change::vault_keep::is_keep(a) => {
                crate::config_change::vault_keep::proof_missing_words(
                    who.surface == crate::approval::Surface::Discord
                        && self.cfg.vault.keep_by_discord,
                )
            }
            Proof::Missing => format!(
                "it needs your second proof: type the code your authenticator app shows into \
                 the card's form, or answer with `theseus answer {} approve --code <the six \
                 digits>`",
                a.correlation_id
            ),
            Proof::Refused(why) => why.to_string(),
        };
        Err(self.proof_refused(a, who, why))
    }

    /// A refusal for the proof, with the row any answer that does not count
    /// leaves (`approval.refused`).
    fn proof_refused(&self, a: &Action, who: &Answerer, why: String) -> Refusal {
        let r = Refusal {
            who: who.who(),
            via: who.via(),
            why,
        };
        let _ = self.refused(
            Act::Answer {
                action: a,
                approve: true,
            },
            who,
            &r,
        );
        r
    }

    /// The words an answer that went through gives back about the proof:
    /// which footing it went through on, as the gate kept it. `None` for a
    /// question that asks for none.
    pub(crate) fn proof_detail(&self, a: &Action, who: &Answerer) -> Option<String> {
        let fresh = marked(&self.store, &a.correlation_id)?;
        if let Some(footing) = self.proofs.take_footing(&a.correlation_id) {
            return Some(footing.detail(fresh));
        }
        let standing = self.proofs.standing(&who.via(), fresh);
        Some(match (fresh, standing.code_needed) {
            (true, _) => "second proof: a fresh code was needed and given".to_string(),
            (false, true) => "second proof: given".to_string(),
            (false, false) => standing.line,
        })
    }

    /// What an answer from `place` would be asked for, in the card's words:
    /// for the Discord binding, which draws a card for the place it posts to.
    pub fn proof_standing(
        &self,
        place: &str,
        fresh: bool,
    ) -> theseus_protocol::owner_proof::ProofStanding {
        self.proofs.standing(place, fresh)
    }

    /// Why the chat's answer tool may not approve `correlation_id`: it asks
    /// for the owner's second proof, which is typed into a card's own form or
    /// the CLI, never said in chat. `None` for a question that asks for none.
    pub(crate) fn chat_refusal(&self, correlation_id: &str) -> Option<String> {
        marked(&self.store, correlation_id).map(|_| {
            format!(
                "this one needs the owner's second proof, a code from their authenticator app, \
                 and a code typed in chat counts for nothing: tell them to approve it on its \
                 card, or with `theseus answer {correlation_id} approve --code <the six digits>`. \
                 Nothing changed"
            )
        })
    }
}
