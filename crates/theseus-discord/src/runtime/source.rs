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

use super::Shared;
use crate::render::cite::{excerpt_text, Source};

/// A "Show source" button's `custom_id`: this, the session, `:`, the handle.
const PREFIX: &str = "src:";

/// The row under a reply: a link button where the source's message can be
/// named, else "Show source". At most `MAX_SOURCES`, one row.
pub(crate) fn source_buttons(session: &str, sources: &[Source]) -> Vec<Component> {
    let components: Vec<Component> = sources
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
    if components.is_empty() {
        return Vec::new();
    }
    vec![Component::ActionRow(ActionRow {
        id: None,
        components,
    })]
}

/// The session and handle a "Show source" button names.
pub(super) fn parse(custom_id: &str) -> Option<(&str, &str)> {
    let (session, handle) = custom_id.strip_prefix(PREFIX)?.split_once(':')?;
    (!session.is_empty() && is_handle(handle)).then_some((session, handle))
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
                Some(c) => excerpt_text(c),
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
        assert!(source_buttons("ses_1", &[]).is_empty());
        assert_eq!(parse("src:ses_1:m:d4e5f6"), Some(("ses_1", "m:d4e5f6")));
        assert_eq!(parse("src:ses_1:d4e5f6"), None);
        assert_eq!(parse("confirm:approve:act_1"), None);
    }
}
