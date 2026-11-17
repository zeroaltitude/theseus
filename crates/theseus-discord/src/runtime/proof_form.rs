//! The owner's second proof on a card (issue 22). A child of `runtime`.
//!
//! A card's Approve for a question that needs the proof opens **a form**, a
//! modal with one field for the code from the owner's authenticator app,
//! instead of answering. The form's title line says which footing the answer
//! goes through, from the daemon's own words for this place ("needed, enter the
//! code" or "given here 3 hours ago, good for 21 hours more": leave it empty to
//! use it). The submit is `work.answer` as the presser with the code, checked
//! by the daemon.
//!
//! **The code goes from the form to the daemon and nowhere else**: it is not a
//! message in any channel, not a content item, not a line of a transcript, and
//! not in the binding's own ledger rows (`binding_ledger` is given the outcome
//! and never the field). The binding never echoes it, and an answer typed or
//! spoken as words carries `typed`, which the daemon never takes a proof from.

use serde_json::{json, Value};
use theseus_protocol::answer::{ConfirmListParams, WorkAnswerParams, WorkAnswerResult};
use theseus_protocol::owner_proof::ProofCode;
use theseus_protocol::{method, ConfirmListResult, DiscordOrigin, LedgerKind};
use twilight_model::application::interaction::modal::ModalInteractionData;
use twilight_model::application::interaction::Interaction;

use super::{origin_of, Shared};
use crate::render::board_v2::short;
use crate::render::{card_v2, clip};

/// A proof form's `custom_id` prefix: `pf:<question id>`.
pub(super) const MODAL: &str = "pf:";
/// The form's one input.
const CODE_FIELD: &str = "code";

const TEXT_INPUT: u64 = 4;
const LABEL: u64 = 18;

/// The form: one short input for the code. `line` is the daemon's words for
/// this place; `required` is false when an earlier proof from here still
/// covers the answer, so an empty field uses it.
pub(crate) fn modal(question: &str, line: &str, required: bool) -> Value {
    json!({"type": 9, "data": {
        "custom_id": format!("{MODAL}{question}"),
        "title": clip("Second proof", 45),
        "components": [{
            "type": LABEL,
            "label": "Code from your authenticator app",
            "description": clip(line, 100),
            "component": {"type": TEXT_INPUT, "custom_id": CODE_FIELD, "style": 1,
                          "required": required, "min_length": if required { 6 } else { 0 },
                          "max_length": 7, "placeholder": "123456"},
        }],
    }})
}

/// The question a form is for, from its `custom_id`.
pub(super) fn question_of(custom_id: &str) -> Option<&str> {
    custom_id.strip_prefix(MODAL).filter(|q| !q.is_empty())
}

impl Shared {
    /// A press that approves `question`: when it needs the owner's second
    /// proof, the form opens instead and this is true; the press is then
    /// answered by the form's submit. Asked of the daemon as the presser, so
    /// the words are for this place. Any failure to ask leaves the press to
    /// answer as it always did, and the daemon's gate then refuses an
    /// approval that carries no proof.
    pub(super) async fn proof_form(
        &self,
        i: &Interaction,
        question: &str,
        discord: Option<DiscordOrigin>,
    ) -> bool {
        let listed: Result<ConfirmListResult, _> = self
            .rpc
            .call(method::CONFIRM_LIST, ConfirmListParams { discord })
            .await;
        let Ok(listed) = listed else {
            return false;
        };
        let Some(req) = listed
            .confirms
            .into_iter()
            .find(|c| c.correlation_id == question || c.correlation_id.ends_with(question))
        else {
            return false;
        };
        if !req.needs_proof {
            return false;
        }
        let (line, required) = match &req.proof {
            Some(p) => (p.line.clone(), p.code_needed),
            None => ("second proof: needed".to_string(), true),
        };
        self.respond_modal(i, &modal(&req.correlation_id, &line, required))
            .await;
        true
    }

    /// A submitted form: `work.answer` as the presser, approving, with the
    /// code if one was typed. The core judges it; a refusal is told to the
    /// presser alone and the card keeps its buttons.
    pub(super) async fn proof_form_submit(
        &self,
        i: &Interaction,
        m: &ModalInteractionData,
        who: &str,
    ) {
        let Some(question) = question_of(&m.custom_id) else {
            return;
        };
        let typed = super::persona_soul::field_of(m, CODE_FIELD);
        let code = Some(typed.trim())
            .filter(|c| !c.is_empty())
            .map(ProofCode::new);
        let discord = origin_of(
            i.author().map(|u| u.id.get()),
            i.channel.as_ref().map(|c| c.id.get()),
            i.guild_id,
        );
        let r = self
            .rpc
            .call::<_, WorkAnswerResult>(
                method::WORK_ANSWER,
                WorkAnswerParams {
                    question_id: question.to_string(),
                    option: "approve".to_string(),
                    author: Some(who.to_string()),
                    discord,
                    code,
                    ..Default::default()
                },
            )
            .await;
        // The outcome, and never the field.
        self.core.binding_ledger(
            LedgerKind::DiscordConfirm,
            None,
            json!({"correlation_id": question, "approve": true, "option": "approve", "by": who,
                   "ok": r.is_ok(), "modal": true, "proof": true,
                   "error": r.as_ref().err().map(|e| e.message.clone())}),
        );
        match r {
            Ok(done) => {
                let mut line =
                    format!("✅ **Approved** by {who} · `…{}`", short(&done.question_id));
                if let Some(detail) = &done.detail {
                    line.push_str(&format!("\n-# {detail}"));
                }
                if i.message.is_some() {
                    self.respond_v2(i, 7, &card_v2::closed(&line), true).await;
                } else {
                    self.tell(i, line).await;
                }
            }
            Err(e) if e.code == theseus_protocol::error_code::REFUSED => {
                let why = e.data.get("why").and_then(Value::as_str);
                self.tell(
                    i,
                    format!(
                        "🔐 Your answer did not count: {}. It keeps waiting for an answer that does.",
                        why.unwrap_or(&e.message)
                    ),
                )
                .await;
            }
            Err(e) => self.tell(i, format!("⚠️ Could not answer: {e}")).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The form is one input whose words are the daemon's, required when a
    /// code is needed and optional when an earlier proof covers it.
    #[test]
    fn the_form_has_one_code_input_and_says_which_footing_it_is() {
        let needed = modal("act_0000reef", "second proof: needed, enter the code", true);
        let input = &needed["data"]["components"][0]["component"];
        assert_eq!(input["custom_id"], "code");
        assert_eq!(input["required"], true);
        assert_eq!(input["min_length"], 6);
        assert_eq!(needed["data"]["custom_id"], "pf:act_0000reef");
        let given = modal(
            "act_0000reef",
            "second proof: given here 3 hours ago",
            false,
        );
        let input = &given["data"]["components"][0]["component"];
        assert_eq!(input["required"], false);
        assert_eq!(input["min_length"], 0);
        assert!(given["data"]["components"][0]["description"]
            .as_str()
            .unwrap()
            .contains("given here 3 hours ago"));
    }

    #[test]
    fn a_forms_question_is_read_from_its_id() {
        assert_eq!(question_of("pf:act_0000reef"), Some("act_0000reef"));
        assert_eq!(question_of("pf:"), None);
        assert_eq!(question_of("wm:abc:approve"), None);
    }
}
