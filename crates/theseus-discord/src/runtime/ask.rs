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
//! - **A needs-you card** (theseus-suvpe) has those buttons, or Approve and
//!   Decline (the classic card's own `confirm:` ids, so a press is the one
//!   `action.confirm` path), and **Details** (`need:details:<question id>`), which shows the presser
//!   alone the full request.

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

/// A Details button's custom id: `need:details:<question id>`.
const DETAILS: &str = "need:details";

/// An expired card's "Ask again" custom id: `again:<question id>`.
const AGAIN: &str = "again";

/// An expired card's one button (theseus-27p6w).
pub(crate) fn again_buttons(question: &str) -> Vec<Component> {
    vec![Component::ActionRow(ActionRow {
        id: None,
        components: vec![Component::Button(Button {
            id: None,
            custom_id: Some(format!("{AGAIN}:{question}")),
            disabled: false,
            emoji: None,
            label: Some("Ask again".into()),
            style: ButtonStyle::Secondary,
            url: None,
            sku_id: None,
        })],
    })]
}

/// The question an "Ask again" press names.
pub(crate) fn parse_again(custom_id: &str) -> Option<String> {
    let q = custom_id.strip_prefix(AGAIN)?.strip_prefix(':')?;
    (!q.is_empty()).then(|| q.to_string())
}

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

/// A needs-you card's buttons: the question's options, or Approve and Decline,
/// and Details last, in the last row that has room.
pub(crate) fn card_buttons(question: &str, choices: &[String]) -> Vec<Component> {
    let mut rows = match choices.is_empty() {
        true => super::confirm_buttons(question),
        false => buttons(question, choices),
    };
    let details = Component::Button(Button {
        id: None,
        custom_id: Some(format!("{DETAILS}:{question}")),
        disabled: false,
        emoji: None,
        label: Some("Details".into()),
        style: ButtonStyle::Secondary,
        url: None,
        sku_id: None,
    });
    match rows.last_mut() {
        Some(Component::ActionRow(row)) if row.components.len() < 5 => row.components.push(details),
        _ => rows.push(Component::ActionRow(ActionRow {
            id: None,
            components: vec![details],
        })),
    }
    rows
}

/// A Details press: the question's id.
pub(crate) fn parse_details(custom_id: &str) -> Option<String> {
    let q = custom_id.strip_prefix(DETAILS)?.strip_prefix(':')?;
    (!q.is_empty()).then(|| q.to_string())
}

/// A press: the option's number from 1, and the question's id.
pub(crate) fn parse(custom_id: &str) -> Option<(usize, String)> {
    let rest = custom_id.strip_prefix(PREFIX)?.strip_prefix(':')?;
    let (n, question) = rest.split_once(':')?;
    let n: usize = n.parse().ok().filter(|n| (1..=6).contains(n))?;
    (!question.is_empty()).then(|| (n, question.to_string()))
}

impl Shared {
    /// A Details press: the whole request, shown to the presser alone. The
    /// core's list is read as the presser, so a surface that may not read the
    /// question sees none of it.
    pub(super) async fn ask_details(
        &self,
        i: &Interaction,
        question: &str,
        discord: Option<DiscordOrigin>,
    ) {
        let listed = self
            .rpc
            .call::<_, theseus_protocol::ConfirmListResult>(
                method::CONFIRM_LIST,
                theseus_protocol::answer::ConfirmListParams { discord },
            )
            .await;
        let text = match listed {
            Ok(l) => l
                .confirms
                .iter()
                .find(|c| c.correlation_id == question)
                .map_or_else(
                    || "That question is not open any more.".to_string(),
                    crate::render::question_details,
                ),
            Err(e) => format!("Could not read it: {}", e.message),
        };
        self.respond(
            i,
            InteractionResponseType::ChannelMessageWithSource,
            Some(text),
            true,
        )
        .await;
    }

    /// "Ask again" on an expired card: `work.answer` with `ask_again` as the
    /// presser, which the core judges as an answer. A counted press clears the
    /// button and says so; a refused one tells the presser alone and the
    /// button stays.
    pub(super) async fn ask_again_press(
        &self,
        i: &Interaction,
        question: String,
        who: &str,
        discord: Option<DiscordOrigin>,
    ) {
        let r = self
            .rpc
            .call::<_, WorkAnswerResult>(
                method::WORK_ANSWER,
                WorkAnswerParams {
                    question_id: question,
                    option: theseus_protocol::answer::ASK_AGAIN.into(),
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
                    content: Some(format!("{was}\n-# 🔁 {who} asked again")),
                    components: Some(vec![]),
                    allowed_mentions: Some(AllowedMentions::default()),
                    ..Default::default()
                },
            ),
            Err(e) => (
                InteractionResponseType::ChannelMessageWithSource,
                InteractionResponseData {
                    content: Some(format!(
                        "🔐 It was not asked again: {}",
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
        assert_eq!(parse_details("need:details:act_1"), Some("act_1".into()));
        assert_eq!(parse_details("ask:1:act_1"), None);
        // Details closes the last row that has room: past five options a
        // second row, else the same one.
        let rows = |n: usize| {
            let c: Vec<String> = (0..n).map(|i| format!("Option {i}")).collect();
            card_buttons("act_1", &c)
        };
        assert_eq!(rows(2).len(), 1);
        assert_eq!(rows(5).len(), 2);
        assert_eq!(rows(6).len(), 2);
        let Component::ActionRow(plain) = &card_buttons("act_1", &[])[0] else {
            panic!("a row")
        };
        let ids: Vec<_> = plain
            .components
            .iter()
            .map(|c| match c {
                Component::Button(b) => b.custom_id.clone().unwrap(),
                _ => panic!(),
            })
            .collect();
        assert_eq!(
            ids,
            [
                "confirm:approve:act_1",
                "confirm:decline:act_1",
                "need:details:act_1"
            ]
        );
    }
}
