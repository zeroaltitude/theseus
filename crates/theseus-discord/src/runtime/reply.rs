//! A message told to the core's reply watches (theseus-rlnbf): "tell me when
//! the harbour master replies in #ops". The call is first in `on_message`,
//! before the message is routed to a place, and reads only: with no reply
//! watch open it is one atomic load in the core, with none for this channel
//! one map lookup. It sees every message the bot hears, in a place of ours or
//! not, so a watch can wait on a channel nobody bound as a place.

use theseus_core::watch::reply::Heard;

use super::Shared;

impl Shared {
    /// Tell the core of a message (a bot's own never reaches here).
    pub(super) fn tell_replies(&self, m: &twilight_model::channel::Message) {
        let author_id = m.author.id.to_string();
        let place = match m.guild_id {
            Some(_) => format!("discord:channel:{}", m.channel_id),
            None => format!("discord:dm:{author_id}"),
        };
        let message_id = m.id.to_string();
        let link = format!(
            "https://discord.com/channels/{}/{}/{}",
            m.guild_id
                .map_or_else(|| "@me".to_string(), |g| g.to_string()),
            m.channel_id,
            m.id
        );
        self.core.watch_reply_seen(&Heard {
            place: &place,
            author_id: &author_id,
            author: &m.author.name,
            message_id: &message_id,
            text: &m.content,
            link: Some(link),
        });
    }
}
