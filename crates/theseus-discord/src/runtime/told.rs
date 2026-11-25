//! The Undo on a note that tells the owner of an act done unasked (issue 57,
//! `theseus_core::owner_note::told`). The button carries the note's post id
//! alone; a press reads the note's undo back from the core, so it works after
//! a restart. Like the mint's Undo (`onto_card.rs`), the press is the undo's
//! own method as the presser, from where they pressed, and the core judges it
//! by that method's rule. Once undone the note says so and loses its button,
//! and a later press calls nothing. A child of `runtime`, apart from it for
//! the shape budget's file ceiling.

use serde_json::Value;
use theseus_core::owner_note::told;
use theseus_protocol::DiscordOrigin;
use twilight_model::application::interaction::Interaction;
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle, Component};
use twilight_model::http::interaction::InteractionResponseType;

use super::Shared;

/// A note's button's custom id: `told:undo:<post>`.
const PREFIX: &str = "told:undo:";

/// One press at a time, so two quick presses undo once.
static PRESSING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The note's one button: Undo.
pub(crate) fn buttons(post: &str) -> Vec<Component> {
    vec![Component::ActionRow(ActionRow {
        id: None,
        components: vec![Component::Button(Button {
            id: None,
            custom_id: Some(format!("{PREFIX}{post}")),
            disabled: false,
            emoji: None,
            label: Some("Undo".to_string()),
            style: ButtonStyle::Secondary,
            url: None,
            sku_id: None,
        })],
    })]
}

/// A press: the note's post id.
pub(crate) fn parse(custom_id: &str) -> Option<String> {
    let post = custom_id.strip_prefix(PREFIX)?;
    (!post.is_empty()
        && post.len() <= 80
        && post
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
    .then(|| post.to_string())
}

impl Shared {
    /// An Undo press: the note's undo as the presser's act. The reply is the
    /// presser's alone; a counted undo also edits the note.
    pub(super) async fn told_press(
        &self,
        i: &Interaction,
        post: String,
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
        let _one = PRESSING.lock().await;
        let core = &self.core;
        let text = match told::undone(&core.store, &post) {
            Ok(Some(u)) => format!("↩️ Already undone by {}: nothing was done.", u.by),
            Err(e) => format!("⚠️ The undo was not made: {e:#}"),
            Ok(None) => match told::of(&core.kernel, &post) {
                Ok(Some((note, undo))) => {
                    let r = self
                        .rpc
                        .call::<_, Value>(undo.method(), undo.params(who, discord))
                        .await;
                    match r {
                        Ok(_) => {
                            if let Err(e) = told::mark_undone(&core.store, &post, who) {
                                tracing::warn!(error = %format!("{e:#}"), "discord: an undone note was not kept");
                            }
                            self.told_edit(i, &told::undone_text(&note, who)).await;
                            "↩️ Undone.".to_string()
                        }
                        Err(e) if e.code == theseus_protocol::error_code::REFUSED => format!(
                            "🔐 Your undo did not count: {}.",
                            e.data
                                .get("why")
                                .and_then(Value::as_str)
                                .unwrap_or(&e.message)
                        ),
                        Err(e) => format!("⚠️ The undo was not made: {}", e.message),
                    }
                }
                Ok(None) => "⚠️ This note's undo is no longer known.".to_string(),
                Err(e) => format!("⚠️ The undo was not made: {e:#}"),
            },
        };
        self.followup(i, &text).await;
    }

    /// The pressed note, edited to `text` with no button.
    async fn told_edit(&self, i: &Interaction, text: &str) {
        let Some(m) = &i.message else { return };
        let edit = self
            .http
            .update_message(m.channel_id, m.id)
            .content(Some(text))
            .components(Some(&[]));
        if let Err(e) = edit.await {
            self.board.error("undone note", None, e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The id carries the post id alone, fits Discord's 100, and parses back.
    #[test]
    fn a_told_button_carries_the_post_and_parses_back() {
        let post = "a".repeat(80);
        let c = buttons(&post);
        let Component::ActionRow(row) = &c[0] else {
            panic!("a row")
        };
        let Component::Button(b) = &row.components[0] else {
            panic!("a button")
        };
        let id = b.custom_id.as_deref().unwrap();
        assert!(id.len() <= 100, "{id}");
        assert_eq!(b.label.as_deref(), Some("Undo"));
        assert_eq!(parse(id).as_deref(), Some(post.as_str()));
        for bad in ["told:undo:", "told:undo:a b", "onto:undo:x", "told:redo:x"] {
            assert!(parse(bad).is_none(), "{bad}");
        }
    }
}
