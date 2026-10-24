//! The owner card's live questions in the owner's DM (theseus-6f8t0, the
//! owner's call O1). The core's `card_question` post goes to the owner's DM
//! alone (`jev_post`), one message a question with one row of buttons:
//! **Keep**, **Edit**, **Not this** for an add or a pattern, **Keep**,
//! **Edit**, **End it** for a stale one (`card:<keep|edit|not|end>:<cdp id>`).
//! A press is `card.answer` as the presser, with the place it came from, so
//! the core judges it as it judges the CLI's: the owner, from a private place,
//! and anyone else's is refused and told alone. **Edit** opens a modal with one
//! text input holding the proposal's words (at most 160 characters), whose
//! submit is `card.answer { option: edit, text }`. A counted answer removes the
//! card's buttons and the presser hears how it went, alone.
//!
//! A child of `runtime`, apart from it for the shape budget's file ceiling.
//! It is not the locked DM task flow and touches none of it.

use serde_json::{json, Value};
use theseus_protocol::{
    method, CardAnswerParams, CardAnswerResult, CardListParams, CardListResult, DiscordOrigin,
    CARD_OBS_MAX_TEXT,
};
use twilight_model::application::interaction::modal::ModalInteractionData;
use twilight_model::application::interaction::Interaction;
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle, Component};
use twilight_model::http::interaction::InteractionResponseType;

use super::{origin_of, Shared};

/// A button's custom id: `card:<keep|edit|not|end>:<cdp id>`.
const PREFIX: &str = "card";
/// The modal's: `card:m:<cdp id>`.
pub(crate) const MODAL: &str = "card:m:";
/// The modal's one field.
const FIELD: &str = "words";

/// A press of the card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Press {
    pub verb: String,
    pub id: String,
}

/// A proposal id as a custom id may carry it.
fn id_ok(id: &str) -> bool {
    id.starts_with("cdp_")
        && id.len() <= 40
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub(crate) fn parse(custom_id: &str) -> Option<Press> {
    let rest = custom_id.strip_prefix(PREFIX)?.strip_prefix(':')?;
    let (verb, id) = rest.split_once(':')?;
    (matches!(verb, "keep" | "edit" | "not" | "end") && id_ok(id)).then(|| Press {
        verb: verb.into(),
        id: id.into(),
    })
}

/// What the core's post is about: the proposal's id and what it asks.
pub(crate) fn card_of(body: &Value) -> (String, String) {
    let c = &body["card"];
    (
        c["ask"].as_str().unwrap_or("add").to_string(),
        c["id"].as_str().unwrap_or("").to_string(),
    )
}

/// The question's buttons, in one row. A stale question's third is End it.
pub(crate) fn buttons(ask: &str, id: &str) -> Vec<Component> {
    if !id_ok(id) {
        return vec![];
    }
    let b = |verb: &str, label: &str, style: ButtonStyle| {
        Component::Button(Button {
            id: None,
            custom_id: Some(format!("{PREFIX}:{verb}:{id}")),
            disabled: false,
            emoji: None,
            label: Some(label.to_string()),
            style,
            url: None,
            sku_id: None,
        })
    };
    let last = match ask {
        "stale" => b("end", "End it", ButtonStyle::Danger),
        _ => b("not", "Not this", ButtonStyle::Secondary),
    };
    vec![Component::ActionRow(ActionRow {
        id: None,
        components: vec![
            b("keep", "Keep", ButtonStyle::Success),
            b("edit", "Edit", ButtonStyle::Primary),
            last,
        ],
    })]
}

/// The modal Edit opens: one input, holding the proposal's words.
pub(crate) fn modal(id: &str, words: &str) -> Value {
    json!({"type": 9, "data": {
        "custom_id": format!("{MODAL}{id}"),
        "title": "Your words for the card",
        "components": [{
            "type": 18,
            "label": "The card says",
            "component": {"type": 4, "custom_id": FIELD, "style": 1,
                          "required": true, "max_length": CARD_OBS_MAX_TEXT,
                          "value": words.chars().take(CARD_OBS_MAX_TEXT).collect::<String>()},
        }],
    }})
}

/// The `card.answer` a press sends: `keep`, `decline` or `end` as it stands.
/// Edit is a modal first.
pub(crate) fn option_of(verb: &str) -> &'static str {
    match verb {
        "keep" => "keep",
        "edit" => "edit",
        "not" => "decline",
        _ => "end",
    }
}

/// What the presser hears of a counted answer.
fn told(option: &str, r: &CardAnswerResult) -> String {
    let words = r.proposal.kept_text.as_deref().unwrap_or(&r.proposal.text);
    match option {
        "keep" => format!("📌 Kept on your card: {words}"),
        "edit" => format!("📌 Kept in your words: {words}"),
        "decline" => "Not this: it will not be asked again.".to_string(),
        _ => format!("↩️ Ended: {words} no longer holds."),
    }
}

impl Shared {
    /// A press of a card question's button.
    pub(super) async fn card_question_press(
        &self,
        i: &Interaction,
        press: Press,
        who: &str,
        discord: Option<DiscordOrigin>,
    ) {
        if press.verb == "edit" {
            // The proposal's words, as the presser may read them; none when
            // the core refuses them, and the answer will be refused too.
            let words = match self
                .rpc
                .call::<_, CardListResult>(
                    method::CARD_LIST,
                    CardListParams {
                        author: Some(who.to_string()),
                        discord,
                    },
                )
                .await
            {
                Ok(l) => l
                    .open
                    .iter()
                    .find(|p| p.id == press.id)
                    .map(|p| p.text.clone())
                    .unwrap_or_default(),
                Err(_) => String::new(),
            };
            self.respond_modal(i, &modal(&press.id, &words)).await;
            return;
        }
        self.card_answer_as(i, &press.id, option_of(&press.verb), None, who, discord)
            .await;
    }

    /// A submitted edit.
    pub(super) async fn card_question_modal(
        &self,
        i: &Interaction,
        m: &ModalInteractionData,
        who: &str,
    ) {
        let Some(id) = m.custom_id.strip_prefix(MODAL).filter(|id| id_ok(id)) else {
            return;
        };
        let text = super::persona_soul::field_of(m, FIELD);
        let discord = origin_of(
            i.author().map(|u| u.id.get()),
            i.channel.as_ref().map(|c| c.id.get()),
            i.guild_id,
        );
        self.card_answer_as(i, id, "edit", Some(text), who, discord)
            .await;
    }

    /// `card.answer` as the presser. A counted answer removes the card's
    /// buttons and is told to the presser alone; a refused or failed one
    /// changes nothing on the card and is told alone too.
    async fn card_answer_as(
        &self,
        i: &Interaction,
        id: &str,
        option: &str,
        text: Option<String>,
        who: &str,
        discord: Option<DiscordOrigin>,
    ) {
        // Acknowledge first: `card.answer` writes between turns (`pass.writing()`),
        // which can outlast Discord's three seconds while a turn runs.
        self.respond(
            i,
            InteractionResponseType::DeferredUpdateMessage,
            None,
            false,
        )
        .await;
        let r = self
            .rpc
            .call::<_, CardAnswerResult>(
                method::CARD_ANSWER,
                CardAnswerParams {
                    id: id.to_string(),
                    option: option.to_string(),
                    text,
                    author: Some(who.to_string()),
                    discord,
                    ..Default::default()
                },
            )
            .await;
        let said = match r {
            Ok(done) => {
                // A counted answer: the card's buttons go.
                if let Err(e) = self
                    .interact
                    .interaction(i.application_id)
                    .update_response(&i.token)
                    .components(Some(&[]))
                    .await
                {
                    self.board.error("card question", None, e);
                }
                told(option, &done)
            }
            Err(e) => super::capture_card::failed(&e),
        };
        self.followup(i, &said).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(ask: &str) -> Vec<(String, String)> {
        buttons(ask, "cdp_00000001")
            .iter()
            .flat_map(|r| match r {
                Component::ActionRow(r) => r.components.clone(),
                _ => vec![],
            })
            .filter_map(|c| match c {
                Component::Button(b) => Some((b.label?, b.custom_id?)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn add_and_pattern_show_not_this_and_stale_shows_end_it() {
        for ask in ["add", "pattern"] {
            let labels: Vec<String> = row(ask).into_iter().map(|(l, _)| l).collect();
            assert_eq!(labels, ["Keep", "Edit", "Not this"], "{ask}");
        }
        let stale = row("stale");
        let labels: Vec<&str> = stale.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(labels, ["Keep", "Edit", "End it"]);
        let ids: Vec<&str> = stale.iter().map(|(_, c)| c.as_str()).collect();
        assert_eq!(
            ids,
            [
                "card:keep:cdp_00000001",
                "card:edit:cdp_00000001",
                "card:end:cdp_00000001"
            ]
        );
        assert!(row("add")[2].1.starts_with("card:not:"));
    }

    #[test]
    fn each_id_parses_back_and_a_stranger_is_none() {
        for (verb, option) in [
            ("keep", "keep"),
            ("edit", "edit"),
            ("not", "decline"),
            ("end", "end"),
        ] {
            let p = parse(&format!("card:{verb}:cdp_00000001")).unwrap();
            assert_eq!((p.verb.as_str(), p.id.as_str()), (verb, "cdp_00000001"));
            assert_eq!(option_of(verb), option);
        }
        for bad in [
            "card:maybe:cdp_1",
            "card:keep:std_1",
            "card:keep:",
            "card:keep:cdp_a b",
            "standing:keep:ask_1",
            "card",
        ] {
            assert!(parse(bad).is_none(), "{bad}");
        }
        assert!(buttons("add", "std_1").is_empty());
    }

    #[test]
    fn the_modal_holds_the_words_cut_at_160_and_the_post_names_its_card() {
        let long = "x".repeat(300);
        let m = modal("cdp_00000001", &long);
        let input = &m["data"]["components"][0]["component"];
        assert_eq!(input["value"].as_str().unwrap().chars().count(), 160);
        assert_eq!(input["max_length"], 160);
        assert_eq!(m["data"]["custom_id"], "card:m:cdp_00000001");
        let body = json!({"kind": "card_question", "text": "t",
            "card": {"id": "cdp_00000001", "ask": "stale"}});
        assert_eq!(card_of(&body), ("stale".into(), "cdp_00000001".into()));
        assert_eq!(card_of(&json!({})), ("add".into(), String::new()));
    }
}
