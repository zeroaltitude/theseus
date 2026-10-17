//! Reactions for the stand-in (theseus-bh3t.2): the bot's own, which a test
//! reads (`PUT` and `DELETE /channels/<c>/messages/<m>/reactions/<emoji>/@me`,
//! kept per message in the order they were added), and a person's, which a
//! test sends (`react`: the gateway's `MESSAGE_REACTION_ADD`).

use std::net::TcpStream;

use serde_json::json;

use super::{reply_empty, seen, FakeDiscord, DEFAULT_GUILD};

/// The emoji a route names, its percent escapes undone.
fn decoded(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = (b[i] == b'%' && i + 2 < b.len())
            .then(|| {
                s.get(i + 1..i + 3)
                    .and_then(|h| u8::from_str_radix(h, 16).ok())
            })
            .flatten();
        match hex {
            Some(v) => {
                out.push(v);
                i += 3;
            }
            None => {
                out.push(b[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl FakeDiscord {
    /// The bot's reactions on `message`, in the order they were added.
    pub fn reactions(&self, message: &str) -> Vec<String> {
        self.state
            .lock()
            .unwrap()
            .reactions
            .get(message)
            .cloned()
            .unwrap_or_default()
    }

    /// `user` reacts to `message` in `channel` with `emoji`: the gateway's
    /// `MESSAGE_REACTION_ADD`, in a guild channel or, for a DM, with no guild.
    pub fn react(&self, user: u64, channel: u64, message: &str, emoji: &str) -> Result<(), String> {
        let gw = self.gateway.get().ok_or("the fake serves no gateway")?;
        let guild = {
            let st = self.state.lock().unwrap();
            (!st.dms.contains_key(&channel))
                .then(|| st.guild.as_ref().map_or(DEFAULT_GUILD, |g| g.id))
        };
        let mut d = json!({"user_id": user.to_string(), "channel_id": channel.to_string(),
            "message_id": message, "emoji": {"id": null, "name": emoji}, "burst": false});
        if let Some(g) = guild {
            d["guild_id"] = json!(g.to_string());
        }
        if gw.dispatch("MESSAGE_REACTION_ADD", d) {
            Ok(())
        } else {
            Err("no client is connected to the gateway".into())
        }
    }

    pub(super) fn reaction_put(
        &self,
        stream: TcpStream,
        (channel, message): (&str, &str),
        emoji: &str,
    ) -> std::io::Result<()> {
        let emoji = decoded(emoji);
        let known = self
            .state
            .lock()
            .unwrap()
            .messages
            .iter()
            .any(|m| m.id == message);
        if known {
            let mut st = self.state.lock().unwrap();
            let held = st.reactions.entry(message.to_string()).or_default();
            if !held.contains(&emoji) {
                held.push(emoji.clone());
            }
        }
        let path = format!("/channels/{channel}/messages/{message}/reactions/{emoji}/@me");
        self.record(seen(
            "PUT",
            &path,
            if known { "reacted" } else { "unknown" },
        ));
        reply_empty(stream)
    }

    pub(super) fn reaction_delete(
        &self,
        stream: TcpStream,
        (channel, message): (&str, &str),
        emoji: &str,
    ) -> std::io::Result<()> {
        let emoji = decoded(emoji);
        if let Some(held) = self.state.lock().unwrap().reactions.get_mut(message) {
            held.retain(|e| *e != emoji);
        }
        let path = format!("/channels/{channel}/messages/{message}/reactions/{emoji}/@me");
        self.record(seen("DELETE", &path, "unreacted"));
        reply_empty(stream)
    }
}

#[cfg(test)]
mod tests {
    use super::decoded;

    #[test]
    fn an_emoji_in_a_route_is_decoded() {
        assert_eq!(decoded("%E2%8F%B8%EF%B8%8F"), "⏸\u{fe0f}");
        assert_eq!(decoded("plain"), "plain");
    }
}
