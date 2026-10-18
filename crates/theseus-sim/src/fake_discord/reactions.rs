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

    /// `/_fake/react`: `react` for a check in another process. The body names
    /// the `user`, the `message`, the `emoji` and, for a guild channel, the
    /// `channel`; without one the message is in the user's DM with the bot.
    pub(super) fn control_react(&self, body: &serde_json::Value) -> (u16, serde_json::Value) {
        let n = |k: &str| {
            body[k]
                .as_u64()
                .or_else(|| body[k].as_str().and_then(|v| v.parse().ok()))
        };
        let s = |k: &str| body[k].as_str().unwrap_or("");
        let user = n("user").unwrap_or(0);
        let channel = n("channel").unwrap_or(user + 1);
        match self.react(user, channel, s("message"), s("emoji")) {
            Ok(()) => (200, json!({"id": s("message")})),
            Err(e) => (409, json!({"error": e})),
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
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    use serde_json::{json, Value};

    use super::{decoded, FakeDiscord};
    use crate::fake_discord::BOT_ID;

    #[test]
    fn an_emoji_in_a_route_is_decoded() {
        assert_eq!(decoded("%E2%8F%B8%EF%B8%8F"), "⏸\u{fe0f}");
        assert_eq!(decoded("plain"), "plain");
    }

    fn post(fake: &FakeDiscord, route: &str, body: &Value) -> (u16, Value) {
        let mut s = TcpStream::connect_timeout(&fake.addr.parse().unwrap(), Duration::from_secs(5))
            .unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let b = body.to_string();
        write!(
            s,
            "POST {route} HTTP/1.1\r\nhost: fake\r\ncontent-length: {}\r\n\r\n{b}",
            b.len()
        )
        .unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        let (head, body) = out.split_once("\r\n\r\n").unwrap();
        let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
        (status, serde_json::from_str(body).unwrap_or(Value::Null))
    }

    /// `theseus-sim discord react`: the control route sends a person's
    /// reaction as the gateway's `MESSAGE_REACTION_ADD`, from the user named
    /// (never the bot) and in the channel named, or the user's DM.
    #[test]
    fn a_react_on_a_bot_message_reaches_a_binding_as_a_reaction_add() {
        let fake = FakeDiscord::start_with_gateway();
        let (s, e) = post(&fake, "/_fake/react", &json!({"user": 7, "message": "1"}));
        assert_eq!(s, 409, "no client yet: {e}");
        let gw = fake.gateway().unwrap();
        let stream =
            TcpStream::connect_timeout(&gw.addr.parse().unwrap(), Duration::from_secs(5)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let (mut ws, _) = tungstenite::client::client(gw.url(), stream).unwrap();
        let frame = |ws: &mut tungstenite::WebSocket<TcpStream>| loop {
            if let tungstenite::Message::Text(t) = ws.read().unwrap() {
                return serde_json::from_str::<Value>(t.as_str()).unwrap();
            }
        };
        frame(&mut ws);
        ws.send(tungstenite::Message::text(
            json!({"op": 2, "d": {}}).to_string(),
        ))
        .unwrap();
        frame(&mut ws);
        let (_, card) = post(
            &fake,
            "/api/v10/channels/55/messages",
            &json!({"content": "task card"}),
        );
        let id = card["id"].as_str().unwrap().to_string();
        let (s, e) = post(
            &fake,
            "/_fake/react",
            &json!({"user": 7, "channel": 55, "message": id, "emoji": "\u{23f8}\u{fe0f}"}),
        );
        assert_eq!(s, 200, "{e}");
        let d = frame(&mut ws);
        assert_eq!(d["t"], "MESSAGE_REACTION_ADD");
        assert_eq!(d["d"]["user_id"], "7");
        assert_ne!(d["d"]["user_id"], BOT_ID.to_string(), "never the bot's own");
        assert_eq!(d["d"]["channel_id"], "55");
        assert_eq!(d["d"]["message_id"], id);
        assert_eq!(d["d"]["emoji"]["name"], "\u{23f8}\u{fe0f}");
    }
}
