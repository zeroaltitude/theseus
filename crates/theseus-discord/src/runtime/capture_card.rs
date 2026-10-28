//! The cards of capture (theseus-slvl.5): what `directive.v1` kept from the
//! owner's words, and what it asks about. The core's `standing_card` post goes
//! to the owner's DM alone (as the seal card does), a kept record with
//! **Undo** and a question with **Keep**, **Keep everywhere** (when it would
//! hold narrower) and **Not this**. A press is `standing.revoke` or
//! `standing.answer` as the presser, with the place it came from, so the core
//! judges it as it judges an answer: the owner, from a private place. The
//! presser hears how it went, alone. A child of `runtime`, apart from it for
//! the shape budget's file ceiling.

use serde_json::Value;
use theseus_protocol::{
    method, DiscordOrigin, StandingAnswerParams, StandingAnswerResult, StandingChanged,
    StandingRevokeParams,
};
use twilight_model::application::interaction::Interaction;
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle, Component};
use twilight_model::http::interaction::InteractionResponseType;

use super::Shared;

/// A card button's custom id: `standing:<undo|keep|everywhere|not>:<id>`.
const PREFIX: &str = "standing";

/// What a card is: a kept record's (`undo`), or an ask's (`keep`, with
/// `everywhere` offered when the record would hold narrower).
pub(crate) fn card_of(body: &Value) -> (String, String, bool) {
    let c = &body["card"];
    (
        c["mode"].as_str().unwrap_or("").to_string(),
        c["id"].as_str().unwrap_or("").to_string(),
        c["everywhere"].as_bool().unwrap_or(false),
    )
}

/// A card's buttons, in one row.
pub(crate) fn buttons(mode: &str, id: &str, everywhere: bool) -> Vec<Component> {
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
    let row = match mode {
        "kept" => vec![b("undo", "Undo", ButtonStyle::Secondary)],
        "ask" => {
            let mut row = vec![b("keep", "Keep", ButtonStyle::Success)];
            if everywhere {
                row.push(b("everywhere", "Keep everywhere", ButtonStyle::Primary));
            }
            row.push(b("not", "Not this", ButtonStyle::Secondary));
            row
        }
        _ => return vec![],
    };
    vec![Component::ActionRow(ActionRow {
        id: None,
        components: row,
    })]
}

/// A press: its verb and the record's or the ask's id.
pub(crate) fn parse(custom_id: &str) -> Option<(String, String)> {
    let rest = custom_id.strip_prefix(PREFIX)?.strip_prefix(':')?;
    let (verb, id) = rest.split_once(':')?;
    let ok = match verb {
        "undo" => id.starts_with("std_"),
        "keep" | "everywhere" | "not" => id.starts_with("ask_"),
        _ => false,
    };
    ok.then(|| (verb.to_string(), id.to_string()))
}

/// What the presser hears of a refusal or a failure.
pub(super) fn failed(e: &crate::rpc_client::CallError) -> String {
    match e.code == theseus_protocol::error_code::REFUSED {
        true => format!(
            "🔐 Your answer did not count: {}.",
            e.data
                .get("why")
                .and_then(Value::as_str)
                .unwrap_or(&e.message)
        ),
        false => format!("⚠️ {}", e.message),
    }
}

impl Shared {
    /// A card button: an undo is `standing.revoke`, the rest `standing.answer`,
    /// as the presser from where they pressed. The reply is theirs alone.
    pub(super) async fn standing_press(
        &self,
        i: &Interaction,
        (verb, id): (String, String),
        who: &str,
        discord: Option<DiscordOrigin>,
    ) {
        self.respond(
            i,
            InteractionResponseType::DeferredChannelMessageWithSource,
            None,
            true,
        )
        .await;
        let text = match verb.as_str() {
            "undo" => match self
                .rpc
                .call::<_, StandingChanged>(
                    method::STANDING_REVOKE,
                    StandingRevokeParams {
                        id: id.clone(),
                        author: Some(who.to_string()),
                        discord,
                    },
                )
                .await
            {
                Ok(_) => format!("↩️ Undone: {id} leaves every prompt at its next turn."),
                Err(e) => failed(&e),
            },
            _ => match self
                .rpc
                .call::<_, StandingAnswerResult>(
                    method::STANDING_ANSWER,
                    StandingAnswerParams {
                        id: id.clone(),
                        answer: verb.clone(),
                        author: Some(who.to_string()),
                        discord,
                    },
                )
                .await
            {
                Ok(r) => match r.record {
                    Some(rec) => format!(
                        "📌 Kept as a standing {} for {}: {}",
                        rec.kind.name(),
                        rec.scope.words(),
                        rec.text
                    ),
                    None => "Not kept.".to_string(),
                },
                Err(e) => failed(&e),
            },
        };
        self.followup(i, &text).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ids(rows: &[Component]) -> Vec<(String, String)> {
        rows.iter()
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

    /// A kept record's card has Undo; an ask's has Keep, Keep everywhere
    /// when it would hold narrower, and Not this; each id parses back.
    #[test]
    fn a_kept_card_has_undo_and_an_ask_has_keep_and_not() {
        let kept = ids(&buttons("kept", "std_00000001", false));
        assert_eq!(kept, [("Undo".into(), "standing:undo:std_00000001".into())]);
        assert_eq!(
            parse("standing:undo:std_00000001"),
            Some(("undo".into(), "std_00000001".into()))
        );
        let ask = ids(&buttons("ask", "ask_00000002", true));
        let labels: Vec<&str> = ask.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(labels, ["Keep", "Keep everywhere", "Not this"]);
        for (_, id) in &ask {
            assert!(parse(id).is_some(), "{id}");
        }
        let narrow = ids(&buttons("ask", "ask_00000002", false));
        assert_eq!(narrow.len(), 2, "everywhere is offered only when wider");
        assert!(buttons("other", "x", false).is_empty());
    }

    /// A press is read only for the card's own ids: an undo of an ask, a keep
    /// of a record, and another family's id are nothing.
    #[test]
    fn a_press_names_its_own_kind_of_id() {
        assert_eq!(parse("standing:undo:ask_1"), None);
        assert_eq!(parse("standing:keep:std_1"), None);
        assert_eq!(parse("standing:maybe:ask_1"), None);
        assert_eq!(parse("other:yes:x"), None);
        assert_eq!(parse("standing"), None);
    }

    #[test]
    fn the_card_is_read_from_the_cores_post() {
        let body = json!({"kind": "standing_card", "text": "t",
            "card": {"mode": "ask", "id": "ask_1", "everywhere": true}});
        assert_eq!(card_of(&body), ("ask".into(), "ask_1".into(), true));
        assert_eq!(card_of(&json!({})), (String::new(), String::new(), false));
    }
}
