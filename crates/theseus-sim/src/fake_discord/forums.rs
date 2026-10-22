//! Forum channels for the stand-in (theseus-bh3t.6): a guild's channels
//! listed (`GET /guilds/<g>/channels`), a forum made (`POST /guilds/<g>/channels`
//! with `type: 15`, its name, overwrites and tags), renamed (`PATCH`), and
//! the rule that Discord refuses a tag edit on an archived post unless the
//! same body unarchives it. A bot without Manage Channels is refused
//! (`refuse_forum`).

use std::net::TcpStream;

use serde_json::{json, Value};

use super::threads::{forum_json, FakeTag};
use super::{channel_json, reply, seen, FakeDiscord, Overwrite};

/// What the stand-in keeps of forums beyond its channels: the switch, and each
/// create's body as it was sent.
#[derive(Default)]
pub(super) struct Forums {
    refuse: bool,
    made: Vec<Value>,
}

/// An overwrite's bits, sent as a string (Discord's way) or a number.
fn bits(v: &Value) -> u64 {
    v.as_str()
        .and_then(|s| s.parse().ok())
        .or_else(|| v.as_u64())
        .unwrap_or(0)
}

fn missing_access() -> Value {
    json!({"message": "Missing Permissions", "code": 50013})
}

impl FakeDiscord {
    /// The bot lacks Manage Channels: listing the guild's channels or making
    /// one answers 403.
    pub fn refuse_forum(&self, refuse: bool) {
        self.state.lock().unwrap().forum_rules.refuse = refuse;
    }

    /// The bodies of the forums made by `POST /guilds/<g>/channels`, in order.
    pub fn forums_made(&self) -> Vec<Value> {
        self.state.lock().unwrap().forum_rules.made.clone()
    }

    /// A forum renamed from outside, as its owner does in Discord.
    pub fn rename_forum(&self, forum: u64, name: &str) {
        let mut st = self.state.lock().unwrap();
        if let Some(c) = st
            .guild
            .as_mut()
            .and_then(|g| g.channels.iter_mut().find(|c| c.id == forum))
        {
            c.name = name.to_string();
        }
    }

    /// A post archived by Discord's clock (a day of quiet), as a test brings it on.
    pub fn archive_thread(&self, thread: u64) {
        if let Some(t) = self.state.lock().unwrap().threads.get_mut(&thread) {
            t.archived = true;
        }
    }

    /// The guild as its file says now, with the forums the bot made in it
    /// kept: the file does not know them.
    pub(super) fn keep_forums_set_guild(&self, mut g: super::Guild) {
        let mut st = self.state.lock().unwrap();
        let old = st.guild.take().map(|o| o.channels).unwrap_or_default();
        for c in old {
            if st.forums.contains(&c.id) && !g.channels.iter().any(|x| x.id == c.id) {
                g.channels.push(c);
            }
        }
        st.guild = Some(g);
    }

    /// `GET /guilds/<g>/channels`: the guild's channels, a forum as type 15.
    pub(super) fn guild_channels(&self, stream: TcpStream, guild: &str) -> std::io::Result<()> {
        let path = format!("/guilds/{guild}/channels");
        let st = self.state.lock().unwrap();
        if st.forum_rules.refuse {
            drop(st);
            self.record(seen("GET", &path, "refused"));
            return reply(stream, 403, &missing_access());
        }
        let Some(g) = st.guild.as_ref().filter(|g| g.id.to_string() == guild) else {
            drop(st);
            self.record(seen("GET", &path, "unknown"));
            return reply(
                stream,
                404,
                &json!({"message": "Unknown Guild", "code": 10004}),
            );
        };
        let mut list: Vec<Value> = g
            .channels
            .iter()
            .map(|c| {
                let mut j = channel_json(g.id, c);
                if st.forums.contains(&c.id) {
                    j["type"] = json!(15);
                }
                j
            })
            .collect();
        // The text channels the bot made outside a category (the task home,
        // theseus-xlxp4); a task channel sits under its category and is not
        // listed.
        list.extend(
            st.channels
                .values()
                .filter(|c| c.kind == 0 && c.parent.is_none() && !c.deleted)
                .map(super::channels::channel_json),
        );
        drop(st);
        self.record(seen("GET", &path, "forums_listed"));
        reply(stream, 200, &json!(list))
    }

    /// `POST /guilds/<g>/channels` with `type: 15`: a forum with the sent
    /// overwrites and tags. (Task channels' own types are made elsewhere.)
    pub(super) fn forum_create(
        &self,
        stream: TcpStream,
        guild: &str,
        body: &Value,
    ) -> std::io::Result<()> {
        let path = format!("/guilds/{guild}/channels");
        if self.state.lock().unwrap().forum_rules.refuse {
            self.record(seen("POST", &path, "refused"));
            return reply(stream, 403, &missing_access());
        }
        let name = body["name"].as_str().unwrap_or("").to_string();
        let tags = body["available_tags"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if body["type"].as_u64() != Some(15) || name.is_empty() || tags.len() > 20 {
            self.record(seen("POST", &path, "refused"));
            return reply(
                stream,
                400,
                &json!({"message": "Invalid Form Body", "code": 50035}),
            );
        }
        let id = self.next_big();
        let tags: Vec<FakeTag> = tags
            .iter()
            .map(|t| FakeTag {
                id: self.next_big().to_string(),
                name: t["name"].as_str().unwrap_or("").to_string(),
            })
            .collect();
        let overwrites = body["permission_overwrites"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|o| Overwrite {
                id: o["id"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0),
                kind: u8::try_from(o["type"].as_u64().unwrap_or(0)).unwrap_or(0),
                allow: bits(&o["allow"]),
                deny: bits(&o["deny"]),
            })
            .collect();
        let made = {
            let mut st = self.state.lock().unwrap();
            let Some(g) = st.guild.as_mut().filter(|g| g.id.to_string() == guild) else {
                drop(st);
                self.record(seen("POST", &path, "unknown"));
                return reply(
                    stream,
                    404,
                    &json!({"message": "Unknown Guild", "code": 10004}),
                );
            };
            let c = super::Channel {
                id,
                name,
                overwrites,
            };
            let mut j = channel_json(g.id, &c);
            g.channels.push(c);
            st.forums.insert(id);
            st.forum_tags.insert(id, tags.clone());
            st.forum_rules.made.push(body.clone());
            j["type"] = json!(15);
            j["available_tags"] = forum_json(&id.to_string(), &tags)["available_tags"].clone();
            j
        };
        self.record(seen("POST", &path, "forum_made"));
        reply(stream, 200, &made)
    }

    /// The rules of a `PATCH /channels/<id>` that are forums': a forum's
    /// rename, and a tag edit on an archived post, which Discord refuses
    /// unless the body unarchives it. The status and body to answer, when one
    /// of them applies.
    pub(super) fn forum_patch(&self, id: &str, body: &Value) -> Option<(u16, Value)> {
        let c: u64 = id.parse().ok()?;
        let mut st = self.state.lock().unwrap();
        if st.forums.contains(&c) && body["name"].is_string() {
            let g = st.guild.as_mut()?;
            let ch = g.channels.iter_mut().find(|ch| ch.id == c)?;
            ch.name = body["name"].as_str()?.to_string();
            return Some((200, channel_json(g.id, ch)));
        }
        let archived = st.threads.get(&c).is_some_and(|t| t.archived);
        if archived && body["applied_tags"].is_array() && body["archived"].as_bool() != Some(false)
        {
            drop(st);
            self.record(seen("PATCH", &format!("/channels/{id}"), "archived"));
            return Some((400, json!({"message": "Thread is archived", "code": 50083})));
        }
        None
    }
}
