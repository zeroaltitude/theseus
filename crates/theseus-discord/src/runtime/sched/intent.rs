//! A card made at most once per item, across a kill at any point. A child of
//! `sched`; the same order as a forum post's (`forum/intent.rs`).
//!
//! The card's create carries the item's nonce, so a retry in the same process
//! or soon after returns the first message; Discord's own nonce window closes
//! (the stand-in's never does), so a restart after it, with the card made and
//! its entry lost to a kill, made the card twice. The intent comes first:
//! `discord.sched_intents` (META) holds `(item, channel, when)` before the
//! create and is settled once the entry is saved. The next time the item needs a
//! card, an intent still on the list is reconciled (`Worker::settle_intent`):
//! the channel's newest messages are read for one of the bot's Components V2
//! cards that carries the item's short id (`…ab12cd`, in every card's words),
//! and that message is the item's card, edited to the truth; none is a card
//! never made, and the first is made. A read that fails is no answer: no card is
//! made until it goes through. Not covered: a card made, unrecorded, and more
//! than 100 messages up the task home's channel by the restart.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use twilight_http::request::Request;
use twilight_http::routing::Route as HttpRoute;

use super::{Home, Rec, Worker};
use crate::render::board_v2::short;
use crate::render::sched::Face;

/// META key of the cards being made.
pub(super) const KEY: &str = "discord.sched_intents";

/// How many of the channel's newest messages are read to find a card.
const READ: u16 = 100;

/// Components V2: the flag of a card.
const V2: u64 = 1 << 15;

/// A card about to be made, or made and not yet recorded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct Intent {
    pub id: String,
    pub channel: u64,
    pub at_ms: u64,
}

/// What every card's words carry of its item.
pub(super) fn marker(id: &str) -> String {
    format!("`…{}`", short(id))
}

impl Worker {
    pub(super) fn save_intents(&self) {
        if let Err(e) = self.shared.core.store.put_meta(KEY, &self.intents) {
            tracing::warn!(error = %format!("{e:#}"), "the cards being made were not saved");
        }
    }

    /// The card of `id` is about to be made.
    pub(super) fn intend(&mut self, id: &str, channel: u64) {
        self.intents.retain(|i| i.id != id);
        self.intents.push(Intent {
            id: id.to_string(),
            channel,
            at_ms: theseus_protocol::now_unix_ms(),
        });
        self.save_intents();
    }

    /// The card is recorded, or was never made.
    pub(super) fn settle(&mut self, id: &str) {
        let before = self.intents.len();
        self.intents.retain(|i| i.id != id);
        if self.intents.len() != before {
            self.save_intents();
        }
    }

    /// An intent left by a card a kill may have cut off, resolved before any
    /// card is made for the item. True when the item has its card (found again
    /// and entered in the list: the caller makes none); an error is a read that
    /// did not go through.
    pub(super) async fn settle_intent(
        &mut self,
        f: &Face,
        home: &Home,
        now: u64,
    ) -> anyhow::Result<bool> {
        let Some(intent) = self.intents.iter().find(|i| i.id == f.id).cloned() else {
            return Ok(false);
        };
        let found = self.find_card(&intent).await?;
        self.settle(&f.id);
        let Some(message) = found else {
            tracing::info!(place = %home.label, "a card meant before a stop was never made: the first is made now");
            return Ok(false);
        };
        tracing::info!(place = %home.label, message, "a card made before a stop was recorded, found again: no second one");
        // Its digest is nothing, so the pass edits it to the item's truth.
        self.recs.push(Rec {
            id: f.id.clone(),
            kind: f.kind.noun().to_string(),
            title: f.title.clone(),
            from: f.from.clone(),
            every: f.every.clone(),
            channel: intent.channel,
            guild: home.guild.clone(),
            card: message,
            created_ms: now,
            due_ms: f.due_ms,
            snoozed: false,
            ended_ms: None,
            by: None,
            digest: 0,
        });
        self.save();
        Ok(true)
    }

    /// The newest of the channel's messages that is one of the bot's cards
    /// carrying the item's marker.
    async fn find_card(&self, intent: &Intent) -> anyhow::Result<Option<u64>> {
        let req = Request::builder(&HttpRoute::GetMessages {
            after: None,
            around: None,
            before: None,
            channel_id: intent.channel,
            limit: Some(READ),
        })
        .build()?;
        let messages: Value = self
            .shared
            .http
            .request::<Value>(req)
            .await?
            .model()
            .await?;
        let bot = self.shared.bot_id();
        let marker = marker(&intent.id);
        Ok(messages
            .as_array()
            .into_iter()
            .flatten()
            .filter(|m| m["flags"].as_u64().unwrap_or(0) & V2 != 0)
            .filter(|m| {
                bot == 0
                    || m["author"]["id"]
                        .as_str()
                        .and_then(|a| a.parse::<u64>().ok())
                        == Some(bot)
            })
            .filter(|m| m.to_string().contains(&marker))
            .filter_map(|m| m["id"].as_str()?.parse::<u64>().ok())
            .max())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_marker_is_what_every_cards_words_carry_of_its_item() {
        let id = "wak_01J0aaaaaaaaaaaaaaaaaaaaab12cd";
        assert_eq!(marker(id), "`…ab12cd`");
        use crate::render::sched::{card, Face, Kind, Life};
        let mut face = Face {
            id: id.into(),
            kind: Kind::Reminder,
            title: "stretch".into(),
            due_ms: Some(1_790_000_300_000),
            life: Life::Scheduled { snoozed: false },
            from: None,
            every: None,
            paused: false,
            routine: None,
        };
        for kind in [Kind::Reminder, Kind::CheckIn, Kind::Task] {
            face.kind = kind;
            let words = serde_json::to_string(&card(&face)).unwrap();
            assert!(words.contains(&marker(id)), "{kind:?}: {words}");
        }
    }
}
