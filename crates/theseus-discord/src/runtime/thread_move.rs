//! `/move` on Discord (theseus-rha2).
//!
//! - **`/move thread:`** is `thread.move` for the last message the core placed
//!   in this place: the thread's slug, or `home`. The answer is ephemeral and
//!   comes when the message has run again in the right thread.
//! - **"Typing…"** shows while the place's turn runs ([`typing`]).
//! - **A reply's turn** ([`Shared::reply_turn_of`]): a reply to one of this
//!   process's replies names the turn, so the core can send it to that turn's
//!   thread (an address). A reply to one sent before a restart, which this
//!   process does not hold, finds its turn by the message's durable
//!   `discord.message.out` row ([`sent_turn_of`], issue 44).
//!
//! The router's question ("is this about …?") and its card are gone: a message
//! goes to a thread by its address only (the owner, 2026-11-21).
//!
//! A child of `runtime`, apart from it for the shape budget's file ceiling.

use theseus_protocol::thread_route::{ThreadMoveParams, ThreadMoveResult};
use theseus_protocol::{method, DiscordOrigin, LedgerKind, LedgerTailParams, LedgerTailResult};
use twilight_model::application::command::{Command, CommandType};
use twilight_model::application::interaction::application_command::CommandOptionValue;
use twilight_model::application::interaction::{Interaction, InteractionData};
use twilight_model::http::interaction::InteractionResponseType;
use twilight_util::builder::command::{CommandBuilder, StringBuilder};

use super::Shared;
use crate::rpc_client::RpcClient;
use crate::runtime::route::turn_of_key;

/// The rows of `discord.message.out` read back for a reply's message, at
/// most, newest first.
const SENT_READ: usize = 5000;

/// The turn whose reply `message` was, by the message's `discord.message.out`
/// row, the durable record a restart keeps: its key names the turn
/// (`turn_of_key`). None when no row of the newest `SENT_READ` names it, its
/// key is no turn's, or the rows cannot be read: the message then carries no
/// reply's turn, as before.
pub(super) async fn sent_turn_of(rpc: &RpcClient, message: u64) -> Option<String> {
    let id = message.to_string();
    let (mut before, mut read) = (None, 0);
    while read < SENT_READ {
        let page = rpc
            .call::<_, LedgerTailResult>(
                method::LEDGER_TAIL,
                LedgerTailParams {
                    n: Some(1000),
                    kind: Some(LedgerKind::DiscordMessageOut.as_str().into()),
                    before,
                    ..Default::default()
                },
            )
            .await;
        let page = match page {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(error = %e.message, "a reply's message was not read back");
                return None;
            }
        };
        read += page.rows.len();
        if let Some(r) = page
            .rows
            .iter()
            .find(|r| r.data["message_id"].as_str() == Some(id.as_str()))
        {
            return turn_of_key(r.data["part"].as_str()?).map(str::to_string);
        }
        before = Some(page.older?);
    }
    None
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

/// The place an interaction came from, as the place's record names it.
fn place_key(i: &Interaction) -> Option<String> {
    match i.guild_id {
        None => Some(format!("dm:{}", i.author()?.id)),
        Some(_) => Some(format!("channel:{}", i.channel.as_ref()?.id)),
    }
}

/// Whether the place shows "typing…" now: while its turn runs, or while its
/// submit is outstanding and not stopping.
pub(super) fn typing(inflight: bool, stopping: bool, busy: bool) -> bool {
    busy || (inflight && !stopping)
}

impl super::Place {
    /// [`typing`] for this place.
    pub(super) fn typing_due(&self) -> bool {
        typing(self.turning(), self.stopping, self.renderer.busy())
    }
}

impl Shared {
    /// The turn a message replies to, when it replies to one of this
    /// process's replies (theseus-rha2).
    pub(super) fn reply_turn_of(&self, m: &twilight_model::channel::Message) -> Option<String> {
        m.referenced_message
            .as_ref()
            .and_then(|r| self.replies.turn(r.id.get()))
    }

    /// The message a reply answers when it is one of this bot's whose turn
    /// this process does not hold: [`sent_turn_of`] reads it back.
    pub(super) fn reply_unheld_of(&self, m: &twilight_model::channel::Message) -> Option<u64> {
        let r = m.referenced_message.as_ref()?;
        (r.author.id.get() == self.bot_id() && self.replies.turn(r.id.get()).is_none())
            .then_some(r.id.get())
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

    /// "Typing…" shows while a turn runs, and while a submit is outstanding;
    /// not once it is stopping or done.
    #[test]
    fn typing_shows_while_a_turn_runs() {
        assert!(typing(true, false, false), "submitted");
        assert!(typing(false, false, true), "running");
        assert!(!typing(true, true, false), "stopping");
        assert!(!typing(false, false, false), "done");
    }

    #[test]
    fn move_takes_a_thread_and_nothing_else() {
        let c = command();
        assert_eq!(c.name, "move");
        assert_eq!(c.options.len(), 1);
        assert!(c.options[0].required.unwrap_or(false));
    }

    /// A reply to a message sent before a restart finds the turn it answered
    /// by the message's durable row (issue 44); a message no row names, or
    /// whose key is no turn's, finds none.
    #[tokio::test]
    async fn a_message_sent_before_a_restart_names_its_turn_by_its_row() {
        let d = tempfile::tempdir().unwrap();
        let secrets = theseus_core::secrets::SecretBoard::new([], std::time::Instant::now());
        let core = crate::runtime::tests::core_with(d.path(), secrets, |_| {});
        let shared = crate::runtime::shared_for_tests(&core);
        for (id, part) in [("4242", "turn_old:0"), ("4343", "board")] {
            core.binding_ledger(
                LedgerKind::DiscordMessageOut,
                None,
                serde_json::json!({"place": "#harbor", "target": "discord:channel:7",
                                   "message_id": id, "channel": "7", "part": part}),
            );
        }
        let found = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                if let Some(t) = sent_turn_of(&shared.rpc, 4242).await {
                    break t;
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("the row is read back");
        assert_eq!(found, "turn_old");
        assert_eq!(sent_turn_of(&shared.rpc, 4343).await, None, "no turn's key");
        assert_eq!(sent_turn_of(&shared.rpc, 4444).await, None, "no row");
    }
}
