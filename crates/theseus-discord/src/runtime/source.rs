//! A reply's cited sources, opened from Discord (theseus-ejdk): the row of
//! buttons under the reply, and the press of a "Show source" button. A link
//! button needs no handler (Discord opens it); a "Show source" press asks
//! the core which message the handle names (`memory.cites`, for the session
//! whose recall gave it) and answers the presser alone, ephemerally, with an
//! excerpt. A shared place's sources are cut by the core before they get here,
//! so a press never shows what that place may not.
//!
//! Citations only show: a press changes nothing and notifies no one.

use theseus_protocol::cite::{is_handle, MemoryCitesParams, MemoryCitesResult};
use twilight_model::application::interaction::{Interaction, InteractionData};
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle};
use twilight_model::channel::message::{AllowedMentions, Component, MessageFlags};
use twilight_model::http::interaction::{
    InteractionResponse, InteractionResponseData, InteractionResponseType,
};

use theseus_core::exits::Exits;
use theseus_protocol::cite::Citation;

use super::Shared;
use crate::render::cite::{excerpt_text, Source};

/// A "Show source" button's `custom_id`: this, the session, `:`, the handle.
const PREFIX: &str = "src:";

/// The row under a reply: a link button where the source's message can be
/// named, else "Show source". At most `MAX_SOURCES`, one row; the offer's
/// "Make this a task" button (`offer`: its turn) joins it, or is a row of its own
/// when the sources fill it (theseus-bh3t.5).
pub(crate) fn source_buttons(
    session: &str,
    sources: &[Source],
    offer: Option<&str>,
) -> Vec<Component> {
    let mut components: Vec<Component> = sources
        .iter()
        .map(|s| {
            let (label, custom_id, url, style) = match &s.link {
                Some(url) => (
                    format!("Open [{}]", s.number),
                    None,
                    Some(url.clone()),
                    ButtonStyle::Link,
                ),
                None => (
                    format!("Show source [{}]", s.number),
                    Some(format!("{PREFIX}{session}:{}", s.handle)),
                    None,
                    ButtonStyle::Secondary,
                ),
            };
            Component::Button(Button {
                id: None,
                custom_id,
                disabled: false,
                emoji: None,
                label: Some(label),
                style,
                url,
                sku_id: None,
            })
        })
        .collect();
    let mut rows = Vec::new();
    if let Some(turn) = offer {
        if components.len() == 5 {
            rows.push(components.split_off(0));
        }
        components.push(super::start_task::offer_button(turn));
    }
    if !components.is_empty() {
        rows.push(components);
    }
    rows.into_iter()
        .map(|components| {
            Component::ActionRow(ActionRow {
                id: None,
                components,
            })
        })
        .collect()
}

/// The session and handle a "Show source" button names.
pub(super) fn parse(custom_id: &str) -> Option<(&str, &str)> {
    let (session, handle) = custom_id.strip_prefix(PREFIX)?.split_once(':')?;
    (!session.is_empty() && is_handle(handle)).then_some((session, handle))
}

/// What a press answers with. It goes out as an interaction response, which
/// does not pass the courier's `Lane::write`, and the excerpt is recalled
/// text, an imported episode among it, stored as it was written: the vault's
/// values are replaced here. The excerpt is masked before `excerpt_text` cuts
/// it, so a value is not cut in two first, and the whole answer after, for
/// the header.
fn shown(exits: &Exits, c: &Citation) -> String {
    let masked = Citation {
        excerpt: exits.text(&c.excerpt).0.into_owned(),
        ..c.clone()
    };
    exits.text(&excerpt_text(&masked)).0.into_owned()
}

impl Shared {
    /// A "Show source" press: true when it was one, and answered.
    pub(super) async fn source_interaction(&self, i: &Interaction) -> bool {
        let Some(InteractionData::MessageComponent(c)) = &i.data else {
            return false;
        };
        let Some((session, handle)) = parse(&c.custom_id) else {
            return false;
        };
        let said = match self
            .rpc
            .call::<_, MemoryCitesResult>(
                theseus_protocol::method::MEMORY_CITES,
                MemoryCitesParams {
                    session_id: session.to_string(),
                    handles: vec![handle.to_string()],
                },
            )
            .await
        {
            Ok(r) => match r.cites.first() {
                Some(c) => shown(&self.core.exits, c),
                None => format!("No source for `{handle}` can be shown here."),
            },
            Err(e) => format!("⚠️ The source could not be read: {}", e.message),
        };
        let resp = InteractionResponse {
            kind: InteractionResponseType::ChannelMessageWithSource,
            data: Some(InteractionResponseData {
                content: Some(said),
                flags: Some(MessageFlags::EPHEMERAL),
                allowed_mentions: Some(AllowedMentions::default()),
                ..Default::default()
            }),
        };
        if let Err(e) = self
            .http
            .interaction(i.application_id)
            .create_response(i.id, &i.token, &resp)
            .await
        {
            self.board.error("interaction response", None, e);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The buttons' JSON: a link for a source with a message, "Show source"
    /// (its custom id naming the session and the handle) for one without.
    #[test]
    fn the_row_is_a_link_or_a_show_source_button() {
        let row = source_buttons(
            "ses_1",
            &[
                Source {
                    number: 1,
                    handle: "m:a1b2c3".into(),
                    link: Some("https://discord.com/channels/3/1/2".into()),
                },
                Source {
                    number: 2,
                    handle: "m:d4e5f6".into(),
                    link: None,
                },
            ],
            None,
        );
        let v = serde_json::to_value(&row).unwrap();
        let b = &v[0]["components"];
        assert_eq!(v[0]["type"], 1);
        assert_eq!(b[0]["label"], "Open [1]");
        assert_eq!(b[0]["style"], 5);
        assert_eq!(b[0]["url"], "https://discord.com/channels/3/1/2");
        assert!(b[0].get("custom_id").is_none());
        assert_eq!(b[1]["label"], "Show source [2]");
        assert_eq!(b[1]["style"], 2);
        assert_eq!(b[1]["custom_id"], "src:ses_1:m:d4e5f6");
        assert!(source_buttons("ses_1", &[], None).is_empty());
        assert_eq!(parse("src:ses_1:m:d4e5f6"), Some(("ses_1", "m:d4e5f6")));
        assert_eq!(parse("src:ses_1:d4e5f6"), None);
        assert_eq!(parse("confirm:approve:act_1"), None);
    }

    /// A recalled import holds a vault value as it was written; the press
    /// answers with its name, and a lookalike is left as it is.
    #[test]
    fn a_source_excerpt_leaves_with_the_vaults_values_replaced() {
        use std::time::Instant;
        use theseus_core::secrets::{Secret, SecretBoard};
        const VALUE: &str = "vault-token-4c8e1f7a92b3";
        const LOOKALIKE: &str = "sk-ant-api03-AbCdEfGhIjKlMnOpQrStUvWxYz0123456789";
        let board = SecretBoard::new(["courier_token".to_string()], Instant::now());
        board.publish(
            [("courier_token".to_string(), Ok(Secret::new(VALUE.into())))].into(),
            "fake",
        );
        let exits = Exits::new(board, true);
        let c = Citation {
            handle: "m:a1b2c3".into(),
            header: format!("imported from a harness, {VALUE}"),
            excerpt: format!("the deploy key is {VALUE}\nand {LOOKALIKE}"),
            ..Default::default()
        };
        let said = shown(&exits, &c);
        assert!(!said.contains(VALUE), "{said}");
        assert!(
            said.contains("> the deploy key is [redacted:courier_token]"),
            "{said}"
        );
        assert!(said.contains(LOOKALIKE), "{said}");
    }

    /// The offer's button joins the sources' row, or a row of its own when they fill it.
    #[test]
    fn the_offer_button_joins_the_row_or_makes_its_own() {
        let source = |n| Source {
            number: n,
            handle: format!("m:a1b2c{n}"),
            link: None,
        };
        let one =
            serde_json::to_value(source_buttons("ses_1", &[source(1)], Some("turn_9"))).unwrap();
        assert_eq!(one.as_array().unwrap().len(), 1);
        assert_eq!(one[0]["components"][1]["custom_id"], "tb:offer:turn_9");
        let lone = serde_json::to_value(source_buttons("ses_1", &[], Some("turn_9"))).unwrap();
        assert_eq!(lone[0]["components"].as_array().unwrap().len(), 1);
        let full: Vec<Source> = (1..=5).map(source).collect();
        let two = serde_json::to_value(source_buttons("ses_1", &full, Some("turn_9"))).unwrap();
        assert_eq!(two.as_array().unwrap().len(), 2);
        assert_eq!(two[1]["components"][0]["label"], "➕ Make this a task");
    }
}
