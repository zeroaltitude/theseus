//! The card Jev's filing sends (theseus-xzxqv): when `onto.v1` mints a new
//! category and files the session under it, the owner's DM gets one card
//! saying so, with an **Undo** button. The press is `ontology.undo` as the
//! presser, from where they pressed, so the core judges it as it judges any
//! ontology write: the owner, from a private place. A card that only asks (the
//! confirm band) has no button: it names the command that files. A child of
//! `runtime`, apart from it for the shape budget's file ceiling.

use serde_json::Value;
use theseus_protocol::{DiscordOrigin, OntologyUndoParams, OntologyUndoResult};
use twilight_model::application::interaction::Interaction;
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle, Component};
use twilight_model::http::interaction::InteractionResponseType;

use super::Shared;

/// A card button's custom id: `onto:undo:<write>`.
const PREFIX: &str = "onto";

/// The card's one button: Undo.
pub(crate) fn buttons(write: &str) -> Vec<Component> {
    vec![Component::ActionRow(ActionRow {
        id: None,
        components: vec![Component::Button(Button {
            id: None,
            custom_id: Some(format!("{PREFIX}:undo:{write}")),
            disabled: false,
            emoji: None,
            label: Some("Undo".to_string()),
            style: ButtonStyle::Secondary,
            url: None,
            sku_id: None,
        })],
    })]
}

/// A press: the write it undoes.
pub(crate) fn parse(custom_id: &str) -> Option<String> {
    let write = custom_id.strip_prefix(PREFIX)?.strip_prefix(":undo:")?;
    (!write.is_empty()
        && write
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
    .then(|| write.to_string())
}

/// The card's write, and whether it has an undo, from the core's `onto_card`
/// post.
pub(crate) fn write_of(body: &Value) -> Option<String> {
    (body["undo"] == true)
        .then(|| body["write"].as_str().map(str::to_string))
        .flatten()
}

impl Shared {
    /// An Undo press: the write goes back as the presser's act. The reply is
    /// the presser's alone.
    pub(super) async fn onto_press(
        &self,
        i: &Interaction,
        write: String,
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
        let r = self
            .rpc
            .call::<_, OntologyUndoResult>(
                theseus_protocol::method::ONTOLOGY_UNDO,
                OntologyUndoParams {
                    write: write.clone(),
                    author: Some(who.to_string()),
                    discord,
                },
            )
            .await;
        let text = match r {
            Ok(_) => "↩️ Undone: the earlier filing is back, and Jev counts that one wrong. It \
                      applies at the session's next recompile."
                .to_string(),
            Err(e) if e.code == theseus_protocol::error_code::REFUSED => format!(
                "🔐 Your undo did not count: {}.",
                e.data
                    .get("why")
                    .and_then(Value::as_str)
                    .unwrap_or(&e.message)
            ),
            Err(e) => format!("⚠️ The undo was not made: {}", e.message),
        };
        self.followup(i, &text).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The card has one Undo button, its press parses back to the write, and
    /// a card that asks has none.
    #[test]
    fn a_mint_card_has_an_undo_and_its_press_parses() {
        let c = buttons("jdg_0123abc");
        let Component::ActionRow(row) = &c[0] else {
            panic!("a row")
        };
        let Component::Button(b) = &row.components[0] else {
            panic!("a button")
        };
        let id = b.custom_id.as_deref().unwrap();
        assert_eq!(id, "onto:undo:jdg_0123abc");
        assert_eq!(parse(id).as_deref(), Some("jdg_0123abc"));
        for bad in ["onto:undo:", "onto:undo:a b", "seal:yes:x", "onto:redo:j"] {
            assert!(parse(bad).is_none(), "{bad}");
        }
        assert_eq!(
            write_of(&json!({"kind": "onto_card", "undo": true, "write": "jdg_1"})).as_deref(),
            Some("jdg_1")
        );
        assert!(write_of(&json!({"kind": "onto_card", "undo": false, "write": "jdg_1"})).is_none());
    }
}
