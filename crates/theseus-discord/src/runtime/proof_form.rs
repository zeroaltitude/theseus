//! The owner's second proof on a card (issue 22). A child of `runtime`.
//!
//! A card's Approve for a question that needs a code opens **a form**, a modal
//! with one field for the code from the owner's authenticator app, instead of
//! answering. When the daemon says no code is needed for this place (an open
//! window of the place covers the question, never one that takes a fresh code
//! every time), the press answers and no form opens: asking again would make
//! the window worthless. The press decides by the daemon's answer at press
//! time: a window that has run out since the card was drawn opens the form
//! then, or, when it ran out between the list and the answer, the answer is
//! refused with the daemon's words and the next press opens it. The submit is
//! `work.answer` as the presser with the code, checked by the daemon.
//!
//! **The code goes from the form to the daemon and nowhere else**: it is not a
//! message in any channel, not a content item, not a line of a transcript, and
//! not in the binding's own ledger rows (`binding_ledger` is given the outcome
//! and never the field). The binding never echoes it, and an answer typed or
//! spoken as words carries `typed`, which the daemon never takes a proof from.

use serde_json::{json, Value};
use theseus_protocol::answer::{ConfirmListParams, WorkAnswerParams, WorkAnswerResult};
use theseus_protocol::owner_proof::ProofCode;
use theseus_protocol::{method, ConfirmListResult, ConfirmRequest, DiscordOrigin, LedgerKind};
use twilight_model::application::interaction::modal::ModalInteractionData;
use twilight_model::application::interaction::Interaction;
use twilight_model::http::interaction::InteractionResponseType;

use super::{origin_of, Shared};
use crate::render::board_v2::short;
use crate::render::{card_v2, clip};

/// A proof form's `custom_id` prefix: `pf:<question id>`.
pub(super) const MODAL: &str = "pf:";
/// The form's one input.
const CODE_FIELD: &str = "code";

const TEXT_INPUT: u64 = 4;
const LABEL: u64 = 18;

/// The most characters a form field's description holds (Discord's limit).
const DESCRIPTION: usize = 100;

/// What the form says when the daemon's line does not fit a description: never
/// a line cut in the middle of a sentence.
const NEEDED: &str = "second proof: needed, enter the code from your authenticator app";

/// The form: one short input for the code. `line` is the daemon's words for
/// this place, said whole or, when they are too long for a description, as
/// [`NEEDED`]; `required` is false for a form that an earlier proof would
/// cover (the press answers instead of opening one: `covered`).
pub(crate) fn modal(question: &str, line: &str, required: bool) -> Value {
    let said = if line.chars().count() <= DESCRIPTION {
        line
    } else {
        NEEDED
    };
    json!({"type": 9, "data": {
        "custom_id": format!("{MODAL}{question}"),
        "title": clip("Second proof", 45),
        "components": [{
            "type": LABEL,
            "label": "Code from your authenticator app",
            "description": said,
            "component": {"type": TEXT_INPUT, "custom_id": CODE_FIELD, "style": 1,
                          "required": required, "min_length": if required { 6 } else { 0 },
                          "max_length": 7, "placeholder": "123456"},
        }],
    }})
}

/// Whether an open window of the presser's place covers this approval, so the
/// press answers and no form opens: the daemon says no code is needed for the
/// place, and the question does not take a fresh code every time.
pub(super) fn covered(req: &ConfirmRequest) -> bool {
    req.needs_proof && !req.needs_fresh_proof && req.proof.as_ref().is_some_and(|p| !p.code_needed)
}

/// What a press on a secret to keep is told: where its value is typed.
const KEEP_HERE: &str = "A secret's value is not typed in Discord. Give it where only you see it: \
     run `theseus vault keep` in your own terminal (nothing is shown as you type), or use the \
     card's field in the cockpit. Never paste it into chat.";

/// Whether the question is a secret to keep (`vault.keep`): a settings card
/// whose arguments carry the target.
pub(super) fn keeps(req: &ConfirmRequest) -> bool {
    req.tool == "config.ack" && req.input.get("keep").is_some_and(Value::is_object)
}

/// The question a form is for, from its `custom_id`.
pub(super) fn question_of(custom_id: &str) -> Option<&str> {
    custom_id.strip_prefix(MODAL).filter(|q| !q.is_empty())
}

impl Shared {
    /// A press that approves `question`: when it needs a code the owner has
    /// not given from here, the form opens instead and this is true; the press
    /// is then answered by the form's submit. A question an open window covers
    /// is answered by the press itself (`covered`: this is false). Asked of the
    /// daemon as the presser, so the words are for this place. Any failure to
    /// ask leaves the press to answer as it always did, and the daemon's gate
    /// then refuses an approval that carries no proof.
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
        // A secret to keep takes a value, which Discord's form does not carry
        // (issue 26: the owner decides whether a durable secret may travel
        // through Discord): the press says where it is typed, to the presser
        // alone, and opens no form. The card says the same.
        if keeps(&req) {
            self.respond(
                i,
                InteractionResponseType::ChannelMessageWithSource,
                Some(format!("🔐 {KEEP_HERE}")),
                true,
            )
            .await;
            return true;
        }
        if !req.needs_proof || covered(&req) {
            return false;
        }
        // No authenticator is enrolled: a form could not be answered, so the
        // daemon's words are said whole, to the presser alone.
        if let Some(p) = req.proof.as_ref().filter(|p| p.not_enrolled) {
            self.respond(
                i,
                InteractionResponseType::ChannelMessageWithSource,
                Some(format!("🔐 {}", p.line)),
                true,
            )
            .await;
            return true;
        }
        let line = req.proof.as_ref().map_or(NEEDED, |p| p.line.as_str());
        self.respond_modal(i, &modal(&req.correlation_id, line, true))
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
    use theseus_protocol::owner_proof::ProofStanding;

    fn asked(needs: bool, fresh: bool, standing: Option<ProofStanding>) -> ConfirmRequest {
        ConfirmRequest {
            needs_proof: needs,
            needs_fresh_proof: fresh,
            proof: standing,
            ..Default::default()
        }
    }

    fn standing(code_needed: bool, line: &str) -> ProofStanding {
        ProofStanding {
            code_needed,
            not_enrolled: false,
            given_at_ms: None,
            until_ms: None,
            line: line.into(),
        }
    }

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

    /// A secret to keep opens no form in Discord, whatever its standing: the
    /// press says where the value is typed (issue 26), and any other settings
    /// card is not mistaken for one.
    #[test]
    fn a_secret_to_keep_opens_no_form_and_a_settings_card_is_not_one() {
        let card = |tool: &str, input: serde_json::Value| ConfirmRequest {
            tool: tool.into(),
            input,
            needs_proof: true,
            ..Default::default()
        };
        assert!(keeps(&card("config.ack", json!({"keep": {"vault": "v"}}))));
        assert!(!keeps(&card("config.ack", json!({"paths": []}))));
        assert!(!keeps(&card("config.ack", json!({"keep": null}))));
        assert!(!keeps(&card("proc.run", json!({"keep": {"vault": "v"}}))));
        assert!(KEEP_HERE.contains("theseus vault keep") && KEEP_HERE.contains("cockpit"));
        assert!(!KEEP_HERE.to_lowercase().contains("type it here"));
    }

    /// A line too long for a description is never cut mid-sentence: the
    /// standard whole sentence stands in for it.
    #[test]
    fn a_line_too_long_for_the_description_is_replaced_whole_never_cut() {
        let long = "second proof: ".to_string() + &"word ".repeat(30);
        let m = modal("act_0000reef", &long, true);
        let said = m["data"]["components"][0]["description"].as_str().unwrap();
        assert_eq!(said, NEEDED);
        assert!(said.chars().count() <= DESCRIPTION);
        let whole = modal("act_0000reef", &"x".repeat(DESCRIPTION), true);
        assert_eq!(
            whole["data"]["components"][0]["description"]
                .as_str()
                .unwrap()
                .len(),
            DESCRIPTION
        );
    }

    /// An open window of the place covers an approval that takes no fresh code:
    /// the press answers. Every other state opens the form (or, with no
    /// standing known, leaves the daemon's gate to judge).
    #[test]
    fn a_press_inside_a_window_answers_and_every_other_state_asks() {
        let window = standing(
            false,
            "second proof: given here 3 minutes ago, good for 23 hours more",
        );
        assert!(covered(&asked(true, false, Some(window.clone()))));
        // A question that takes a fresh code every time is never covered.
        assert!(!covered(&asked(true, true, Some(window))));
        // No window: a code is needed.
        assert!(!covered(&asked(
            true,
            false,
            Some(standing(true, "second proof: needed"))
        )));
        // The standing was not given: the form is the safe side.
        assert!(!covered(&asked(true, false, None)));
        // A question that needs no proof is not this module's.
        assert!(!covered(&asked(false, false, Some(standing(false, "")))));
    }

    #[test]
    fn a_forms_question_is_read_from_its_id() {
        assert_eq!(question_of("pf:act_0000reef"), Some("act_0000reef"));
        assert_eq!(question_of("pf:"), None);
        assert_eq!(question_of("wm:abc:approve"), None);
    }
}
