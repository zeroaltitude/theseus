//! A question with options on Discord (theseus-j7cje): its buttons and their
//! presses. A child of `runtime`, apart from it for the shape budget's file
//! ceiling. `discord-board` dresses the card later.
//!
//! - **One button per option** (`ask:<number from 1>:<question id>`), the
//!   first five of six in a row each; a sixth goes to a second row. "Something
//!   else" takes words, so it is answered from the CLI or the cockpit
//!   (`theseus answer <id> other --note …`).
//! - **A press is `work.answer`** as the presser, with the presser's Discord
//!   ids, so the core judges it as it judges every answer (the owner, from a
//!   private place). The message answers itself: a counted press clears its
//!   buttons and says which; a refused one tells the presser alone, and the
//!   buttons stay for an answer that counts.

use theseus_protocol::answer::{WorkAnswerParams, WorkAnswerResult};
use theseus_protocol::{method, DiscordOrigin};
use twilight_model::application::interaction::Interaction;
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle, Component};
use twilight_model::channel::message::{AllowedMentions, MessageFlags};
use twilight_model::http::interaction::{
    InteractionResponse, InteractionResponseData, InteractionResponseType,
};

use super::Shared;

/// A button's custom id: `ask:<number>:<question id>`.
const PREFIX: &str = "ask";

/// The buttons of a question's options: a numbered button each, five to a row.
pub(crate) fn buttons(question: &str, choices: &[String]) -> Vec<Component> {
    choices
        .chunks(5)
        .enumerate()
        .map(|(row, chunk)| {
            let components = chunk
                .iter()
                .enumerate()
                .map(|(i, label)| {
                    let n = row * 5 + i + 1;
                    Component::Button(Button {
                        id: None,
                        custom_id: Some(format!("{PREFIX}:{n}:{question}")),
                        disabled: false,
                        emoji: None,
                        label: Some(format!("{n}. {}", crate::render::clip(label, 60))),
                        style: ButtonStyle::Primary,
                        url: None,
                        sku_id: None,
                    })
                })
                .collect();
            Component::ActionRow(ActionRow {
                id: None,
                components,
            })
        })
        .collect()
}

/// A press: the option's number from 1, and the question's id.
pub(crate) fn parse(custom_id: &str) -> Option<(usize, String)> {
    let rest = custom_id.strip_prefix(PREFIX)?.strip_prefix(':')?;
    let (n, question) = rest.split_once(':')?;
    let n: usize = n.parse().ok().filter(|n| (1..=6).contains(n))?;
    (!question.is_empty()).then(|| (n, question.to_string()))
}

impl Shared {
    /// A press on an option: `work.answer` as the presser.
    pub(super) async fn ask_press(
        &self,
        i: &Interaction,
        (n, question): (usize, String),
        who: &str,
        discord: Option<DiscordOrigin>,
    ) {
        let r = self
            .rpc
            .call::<_, WorkAnswerResult>(
                method::WORK_ANSWER,
                WorkAnswerParams {
                    question_id: question,
                    option: n.to_string(),
                    author: Some(who.to_string()),
                    discord,
                    ..Default::default()
                },
            )
            .await;
        let was = i
            .message
            .as_ref()
            .map(|m| m.content.clone())
            .unwrap_or_default();
        let (kind, data) = match r {
            Ok(_) => (
                InteractionResponseType::UpdateMessage,
                InteractionResponseData {
                    content: Some(format!("{was}\n-# ✅ {who} chose {n}")),
                    components: Some(vec![]),
                    allowed_mentions: Some(AllowedMentions::default()),
                    ..Default::default()
                },
            ),
            // The buttons stay, so the owner may try again; the failure is theirs alone.
            Err(e) => (
                InteractionResponseType::ChannelMessageWithSource,
                InteractionResponseData {
                    content: Some(format!(
                        "🔐 Your answer did not count: {}",
                        e.data
                            .get("why")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or(&e.message)
                    )),
                    flags: Some(MessageFlags::EPHEMERAL),
                    allowed_mentions: Some(AllowedMentions::default()),
                    ..Default::default()
                },
            ),
        };
        let resp = InteractionResponse {
            kind,
            data: Some(data),
        };
        if let Err(e) = self
            .http
            .interaction(i.application_id)
            .create_response(i.id, &i.token, &resp)
            .await
        {
            self.board.error("interaction response", None, e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn six_options_are_two_rows_and_their_ids_parse() {
        let choices: Vec<String> = (1..=6).map(|i| format!("Option {i}")).collect();
        let rows = buttons("act_0000reef", &choices);
        assert_eq!(rows.len(), 2);
        let Component::ActionRow(last) = &rows[1] else {
            panic!("a row")
        };
        let Component::Button(b) = &last.components[0] else {
            panic!("a button")
        };
        assert_eq!(b.custom_id.as_deref(), Some("ask:6:act_0000reef"));
        assert_eq!(
            parse("ask:6:act_0000reef"),
            Some((6, "act_0000reef".into()))
        );
        assert_eq!(parse("ask:7:act_0000reef"), None);
        assert_eq!(parse("ask:0:act_0000reef"), None);
        assert_eq!(parse("confirm:approve:act_1"), None);
    }
}
