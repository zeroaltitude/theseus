//! The router's question, and `/move`, on Discord (theseus-rha2).
//!
//! - **The question.** When the core cannot tell which thread a message is
//!   about, it sends `thread.asked`; the place's actor posts it in one line
//!   with a button for each choice (`1`, `2`, `New thread «…»`, `home`). A
//!   press is `thread.answer` as the presser, with the place it came from, so
//!   the core judges it as it judges an approval's: the owner, from a private
//!   place. The message waits in the core; the card is best effort (what must
//!   be seen is the turn's reply, an outbox post), so a card that never lands
//!   leaves the message to run at home when `[threads] ask_wait_secs` ends.
//! - **`/move thread:`** is `thread.move` for the last message the core placed
//!   in this place: the thread's slug, or `home`. The answer is ephemeral and
//!   comes when the message has run again in the right thread.
//!
//! A child of `runtime`, apart from it for the shape budget's file ceiling.

use theseus_protocol::thread_route::{
    ThreadAnswerParams, ThreadAnswerResult, ThreadAsk, ThreadMoveParams, ThreadMoveResult,
};
use theseus_protocol::{method, DiscordOrigin, LedgerKind};
use twilight_model::application::command::{Command, CommandType};
use twilight_model::application::interaction::application_command::CommandOptionValue;
use twilight_model::application::interaction::{Interaction, InteractionData};
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle, Component};
use twilight_model::channel::message::AllowedMentions;
use twilight_model::http::interaction::{
    InteractionResponse, InteractionResponseData, InteractionResponseType,
};
use twilight_model::id::Id;
use twilight_util::builder::command::{CommandBuilder, StringBuilder};

use super::Shared;

/// A card button's custom id: `thr:<ask id>:<option key>`.
const PREFIX: &str = "thr";

/// A button's label: Discord's cap is 80 characters.
fn label(words: &str) -> String {
    match words.chars().count() > 80 {
        true => words.chars().take(79).collect::<String>() + "\u{2026}",
        false => words.to_string(),
    }
}

/// `/move thread:`.
pub(super) fn command() -> Command {
    CommandBuilder::new(
        "move",
        "Move my last message to another thread, and answer it there",
        CommandType::ChatInput,
    )
    .option(StringBuilder::new("thread", "A thread's slug, or home").required(true))
    .build()
}

/// The question as it is posted: the line, and the start of the message that
/// waits for it.
pub(crate) fn text(ask: &ThreadAsk) -> String {
    format!("\u{2753} **{}**\n> {}", ask.question, ask.message)
}

/// The question's buttons: one row, a button for each choice (five at most).
pub(crate) fn buttons(ask: &ThreadAsk) -> Vec<Component> {
    let row = ask
        .options
        .iter()
        .take(5)
        .map(|o| {
            let style = match o.key.as_str() {
                "home" => ButtonStyle::Secondary,
                "new" => ButtonStyle::Success,
                _ => ButtonStyle::Primary,
            };
            Component::Button(Button {
                id: None,
                custom_id: Some(format!("{PREFIX}:{}:{}", ask.ask_id, o.key)),
                disabled: false,
                emoji: None,
                label: Some(label(&o.label)),
                style,
                url: None,
                sku_id: None,
            })
        })
        .collect();
    vec![Component::ActionRow(ActionRow {
        id: None,
        components: row,
    })]
}

/// A press: the question's id and the option's key.
pub(crate) fn parse(custom_id: &str) -> Option<(String, String)> {
    let rest = custom_id.strip_prefix(PREFIX)?.strip_prefix(':')?;
    let (ask, key) = rest.split_once(':')?;
    (ask.starts_with("ask_") && !key.is_empty()).then(|| (ask.to_string(), key.to_string()))
}

/// The place an interaction came from, as the place's record names it.
fn place_key(i: &Interaction) -> Option<String> {
    match i.guild_id {
        None => Some(format!("dm:{}", i.author()?.id)),
        Some(_) => Some(format!("channel:{}", i.channel.as_ref()?.id)),
    }
}

impl Shared {
    /// A question the core asks, posted in the place's channel with its
    /// buttons. Best effort: a failure is the board's error, and the message
    /// runs at home when its wait ends.
    pub(super) async fn post_thread_ask(&self, channel: u64, ask: &ThreadAsk) {
        let none = AllowedMentions::default();
        let comps = buttons(ask);
        let content = text(ask);
        let sent = self
            .http
            .create_message(Id::new(channel))
            .content(&content)
            .components(&comps)
            .allowed_mentions(Some(&none))
            .await;
        if let Err(e) = sent {
            self.board
                .error("thread ask card", Some(&ask.session_id), e);
        }
    }

    /// A press on the question's card: `thread.answer` as the presser, and
    /// the card settles into the answer.
    pub(super) async fn thread_ask_press(
        &self,
        i: &Interaction,
        (ask_id, key): (String, String),
        who: &str,
        discord: Option<DiscordOrigin>,
    ) {
        let r = self
            .rpc
            .call::<_, ThreadAnswerResult>(
                method::THREAD_ANSWER,
                ThreadAnswerParams {
                    ask_id: ask_id.clone(),
                    choice: key.clone(),
                    author: Some(who.to_string()),
                    discord,
                },
            )
            .await;
        self.core.binding_ledger(
            LedgerKind::DiscordLabel,
            None,
            serde_json::json!({"ask": ask_id, "choice": key, "via": "thread ask", "by": who,
                               "ok": r.is_ok(), "error": r.as_ref().err().map(|e| e.message.clone())}),
        );
        // The card becomes the answer; a refused or late press changes
        // nothing on it and tells the presser alone.
        let resp = match r {
            Ok(a) => {
                let was = match (&a.routed, key.as_str()) {
                    (Some(s), _) => format!("\u{2192} {s}"),
                    (None, "new") => "\u{2192} a new thread".to_string(),
                    (None, _) => "\u{2192} home".to_string(),
                };
                InteractionResponse {
                    kind: InteractionResponseType::UpdateMessage,
                    data: Some(InteractionResponseData {
                        content: Some(format!("\u{2705} {was}")),
                        components: Some(Vec::new()),
                        allowed_mentions: Some(AllowedMentions::default()),
                        ..Default::default()
                    }),
                }
            }
            Err(e) => InteractionResponse {
                kind: InteractionResponseType::ChannelMessageWithSource,
                data: Some(InteractionResponseData {
                    content: Some(match e.code == theseus_protocol::error_code::REFUSED {
                        true => format!(
                            "\u{1f510} Your answer did not count: {}.",
                            e.data
                                .get("why")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or(&e.message)
                        ),
                        false => format!("\u{26a0}\u{fe0f} {}", e.message),
                    }),
                    flags: Some(twilight_model::channel::message::MessageFlags::EPHEMERAL),
                    allowed_mentions: Some(AllowedMentions::default()),
                    ..Default::default()
                }),
            },
        };
        if let Err(e) = self
            .http
            .interaction(i.application_id)
            .create_response(i.id, &i.token, &resp)
            .await
        {
            self.board.error("thread ask response", None, e);
        }
    }

    /// `/move thread:`: true when the interaction was one, and answered.
    pub(super) async fn move_interaction(
        &self,
        i: &Interaction,
        who: &str,
        discord: Option<DiscordOrigin>,
    ) -> bool {
        let Some(InteractionData::ApplicationCommand(c)) = &i.data else {
            return false;
        };
        if c.name != "move" {
            return false;
        }
        self.core.binding_ledger(
            LedgerKind::DiscordCommand,
            None,
            serde_json::json!({"command": c.name, "by": who}),
        );
        // The core runs the message again before it answers: acknowledge now.
        self.respond(
            i,
            InteractionResponseType::DeferredChannelMessageWithSource,
            None,
            true,
        )
        .await;
        let to = c
            .options
            .iter()
            .find_map(|o| match &o.value {
                CommandOptionValue::String(s) if o.name == "thread" => Some(s.trim().to_string()),
                _ => None,
            })
            .unwrap_or_default();
        let session = place_key(i).and_then(|k| self.core.outbox.place_session(&k).ok().flatten());
        let said = match session {
            None => {
                "\u{26a0}\u{fe0f} This place has no session to move a message from.".to_string()
            }
            Some(session_id) => {
                let r = self
                    .rpc
                    .call::<_, ThreadMoveResult>(
                        method::THREAD_MOVE,
                        ThreadMoveParams {
                            session_id,
                            to: to.clone(),
                            turn_id: None,
                            author: Some(who.to_string()),
                            discord,
                        },
                    )
                    .await;
                match r {
                    Ok(m) => format!(
                        "\u{21aa} Moved from {} to {}; it ran again there{}.",
                        m.from,
                        m.to,
                        match m.redirected {
                            true => ", and the first thread was told to disregard it",
                            false => "",
                        }
                    ),
                    Err(e) => format!("\u{26a0}\u{fe0f} {}", e.message),
                }
            }
        };
        self.followup(i, &said).await;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::thread_route::AskOption;

    fn ask() -> ThreadAsk {
        ThreadAsk {
            ask_id: "ask_0001".into(),
            session_id: "ses_a".into(),
            place: Some("discord:dm:7".into()),
            question: "Is this about the pr round or the security fix?".into(),
            options: vec![
                AskOption {
                    key: "1".into(),
                    label: "pr round".into(),
                },
                AskOption {
                    key: "2".into(),
                    label: "security fix".into(),
                },
                AskOption {
                    key: "new".into(),
                    label: "New thread \u{ab}tide gauge\u{bb}".into(),
                },
                AskOption {
                    key: "home".into(),
                    label: "home".into(),
                },
            ],
            message: "why is the build red".into(),
            expires_ms: 1,
        }
    }

    /// The question has one button a choice, ids that `parse` reads back, and
    /// the line and the waiting message in its text.
    #[test]
    fn the_card_has_a_button_a_choice_and_each_press_parses() {
        let rows = buttons(&ask());
        let Component::ActionRow(r) = &rows[0] else {
            panic!("{rows:?}")
        };
        let ids: Vec<String> = r
            .components
            .iter()
            .map(|b| match b {
                Component::Button(b) => b.custom_id.clone().unwrap(),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(
            ids,
            [
                "thr:ask_0001:1",
                "thr:ask_0001:2",
                "thr:ask_0001:new",
                "thr:ask_0001:home"
            ]
        );
        assert_eq!(parse(&ids[2]), Some(("ask_0001".into(), "new".into())));
        assert_eq!(parse("thr:other:1"), None);
        assert_eq!(parse("earned:yes:ern_1"), None);
        let t = text(&ask());
        assert!(
            t.contains("Is this about the pr round or the security fix?"),
            "{t}"
        );
        assert!(t.contains("> why is the build red"), "{t}");
    }

    #[test]
    fn move_takes_a_thread_and_nothing_else() {
        let c = command();
        assert_eq!(c.name, "move");
        assert_eq!(c.options.len(), 1);
        assert!(c.options[0].required.unwrap_or(false));
    }
}
