//! Guild channels for the stand-in (theseus-bh3t.1): a text channel or a
//! category the bot makes (`POST /guilds/<g>/channels`), moved, renamed and
//! re-permissioned (`PATCH /channels/<id>`), and deleted (`DELETE
//! /channels/<id>`). A bot without Manage Channels is refused with 403
//! (`refuse_channels`), as Discord does. Discord's limits are kept: a name of
//! at most 100 characters, 50 channels to a category, and a parent that is a
//! category that exists.

use std::net::TcpStream;

use serde::Serialize;
use serde_json::{json, Value};

use super::{reply, seen, FakeDiscord, Overwrite, DEFAULT_GUILD};

/// A channel the bot made.
#[derive(Debug, Clone, Serialize)]
pub struct FakeChannel {
    pub id: u64,
    pub guild: u64,
    /// 0 for a text channel, 4 for a category.
    pub kind: u8,
    pub name: String,
    pub parent: Option<u64>,
    pub topic: String,
    pub overwrites: Vec<Overwrite>,
    /// Every name it has had, the create's first.
    pub names: Vec<String>,
    pub deleted: bool,
}

/// Discord's most channels in a category.
const CATEGORY_MAX: usize = 50;

fn refused() -> Value {
    json!({"message": "Missing Permissions", "code": 50013})
}

fn invalid(why: &str) -> Value {
    json!({"message": "Invalid Form Body", "code": 50035, "errors": {"_errors": [why]}})
}

/// A bitfield as Discord sends it: a string, or a number.
fn bits(v: &Value) -> u64 {
    match v {
        Value::String(s) => s.parse().unwrap_or(0),
        other => other.as_u64().unwrap_or(0),
    }
}

fn id_of(v: &Value) -> u64 {
    bits(v)
}

fn overwrites(v: &Value) -> Vec<Overwrite> {
    v.as_array()
        .into_iter()
        .flatten()
        .map(|o| Overwrite {
            id: id_of(&o["id"]),
            kind: o["type"].as_u64().unwrap_or(0) as u8,
            allow: bits(&o["allow"]),
            deny: bits(&o["deny"]),
        })
        .collect()
}

pub(super) fn channel_json(c: &FakeChannel) -> Value {
    let over: Vec<Value> = c
        .overwrites
        .iter()
        .map(|o| json!({"id": o.id.to_string(), "type": o.kind, "allow": o.allow.to_string(), "deny": o.deny.to_string()}))
        .collect();
    json!({"id": c.id.to_string(), "type": c.kind, "guild_id": c.guild.to_string(),
        "name": c.name, "position": 0, "topic": c.topic, "permission_overwrites": over,
        "parent_id": c.parent.map(|p| p.to_string())})
}

impl FakeDiscord {
    /// The channels and categories the bot made, in order, deleted ones too.
    pub fn made_channels(&self) -> Vec<FakeChannel> {
        self.state
            .lock()
            .unwrap()
            .channels
            .values()
            .cloned()
            .collect()
    }

    /// The bot lacks Manage Channels: making, changing or deleting a channel
    /// answers 403.
    pub fn refuse_channels(&self, refuse: bool) {
        self.state.lock().unwrap().refuse_channels = refuse;
    }

    /// Whether `id` is a channel or category the bot made.
    fn is_made_channel(&self, id: &str) -> bool {
        let c: u64 = id.parse().unwrap_or(0);
        self.state.lock().unwrap().channels.contains_key(&c)
    }

    /// `POST /guilds/<guild>/channels`.
    pub(super) fn channel_create(
        &self,
        stream: TcpStream,
        guild: &str,
        body: &Value,
    ) -> std::io::Result<()> {
        // A forum is the default forum's (`forums.rs`, theseus-bh3t.6).
        if body["type"].as_u64() == Some(15) {
            return self.forum_create(stream, guild, body);
        }
        let path = format!("/guilds/{guild}/channels");
        let (code, out) = {
            let mut st = self.state.lock().unwrap();
            let guild_id = st.guild.as_ref().map_or(DEFAULT_GUILD, |g| g.id);
            let kind = body["type"].as_u64().unwrap_or(0) as u8;
            let name = body["name"].as_str().unwrap_or("").to_string();
            let parent = body.get("parent_id").filter(|p| !p.is_null()).map(id_of);
            let siblings = parent.map_or(0, |p| {
                st.channels
                    .values()
                    .filter(|c| !c.deleted && c.parent == Some(p))
                    .count()
            });
            let parent_ok = parent.is_none_or(|p| {
                st.channels
                    .get(&p)
                    .is_some_and(|c| c.kind == 4 && !c.deleted)
            });
            if st.refuse_channels {
                (403, refused())
            } else if guild.parse::<u64>().ok() != Some(guild_id)
                || name.is_empty()
                || name.chars().count() > 100
                || !matches!(kind, 0 | 4)
                || (kind == 4 && parent.is_some())
                || !parent_ok
            {
                (400, invalid("name, type or parent_id"))
            } else if siblings >= CATEGORY_MAX {
                (
                    400,
                    json!({"message": "Maximum number of channels in category reached (50)", "code": 30013}),
                )
            } else {
                st.next_big = st.next_big.max(super::threads::FIRST_ID) + 1;
                let c = FakeChannel {
                    id: st.next_big,
                    guild: guild_id,
                    kind,
                    names: vec![name.clone()],
                    name: if kind == 0 { name.to_lowercase() } else { name },
                    parent,
                    topic: body["topic"].as_str().unwrap_or("").to_string(),
                    overwrites: overwrites(&body["permission_overwrites"]),
                    deleted: false,
                };
                let out = channel_json(&c);
                st.channels.insert(c.id, c);
                (200, out)
            }
        };
        self.record(seen(
            "POST",
            &path,
            if code == 200 { "channel" } else { "refused" },
        ));
        reply(stream, code, &out)
    }

    /// `PATCH /channels/<id>`: a channel the bot made is changed here, any
    /// other (a thread, a forum) goes to the thread routes.
    pub(super) fn channel_patch(
        &self,
        stream: TcpStream,
        id: &str,
        body: &Value,
    ) -> std::io::Result<()> {
        if !self.is_made_channel(id) {
            return self.thread_update(stream, id, body);
        }
        let path = format!("/channels/{id}");
        let (code, out) = {
            let mut st = self.state.lock().unwrap();
            let key: u64 = id.parse().unwrap_or(0);
            let parent = body
                .get("parent_id")
                .map(|p| (!p.is_null()).then(|| id_of(p)));
            let parent_ok = parent.flatten().is_none_or(|p| {
                st.channels
                    .get(&p)
                    .is_some_and(|c| c.kind == 4 && !c.deleted)
            });
            let siblings = parent.flatten().map_or(0, |p| {
                st.channels
                    .values()
                    .filter(|c| !c.deleted && c.parent == Some(p) && c.id != key)
                    .count()
            });
            let name = body["name"].as_str();
            if st.refuse_channels {
                (403, refused())
            } else if st.channels.get(&key).is_none_or(|c| c.deleted) {
                (404, json!({"message": "Unknown Channel", "code": 10003}))
            } else if !parent_ok || name.is_some_and(|n| n.is_empty() || n.chars().count() > 100) {
                (400, invalid("name or parent_id"))
            } else if siblings >= CATEGORY_MAX {
                (
                    400,
                    json!({"message": "Maximum number of channels in category reached (50)", "code": 30013}),
                )
            } else {
                let c = st.channels.get_mut(&key).expect("checked above");
                if let Some(n) = name {
                    c.name = if c.kind == 0 {
                        n.to_lowercase()
                    } else {
                        n.to_string()
                    };
                    c.names.push(n.to_string());
                }
                if let Some(p) = parent {
                    c.parent = p;
                }
                if let Some(t) = body["topic"].as_str() {
                    c.topic = t.to_string();
                }
                if body["permission_overwrites"].is_array() {
                    c.overwrites = overwrites(&body["permission_overwrites"]);
                }
                (200, channel_json(c))
            }
        };
        self.record(seen(
            "PATCH",
            &path,
            if code == 200 {
                "channel_updated"
            } else {
                "refused"
            },
        ));
        reply(stream, code, &out)
    }

    /// `DELETE /channels/<id>`: a channel the bot made goes (it stays listed,
    /// marked deleted); a category's channels stay, with no parent.
    pub(super) fn channel_delete(&self, stream: TcpStream, id: &str) -> std::io::Result<()> {
        let path = format!("/channels/{id}");
        let (code, out) = {
            let mut st = self.state.lock().unwrap();
            let key: u64 = id.parse().unwrap_or(0);
            if st.refuse_channels {
                (403, refused())
            } else if st.channels.get(&key).is_none_or(|c| c.deleted) {
                (404, json!({"message": "Unknown Channel", "code": 10003}))
            } else {
                let c = st.channels.get_mut(&key).expect("checked above");
                c.deleted = true;
                let out = channel_json(c);
                for other in st.channels.values_mut().filter(|c| c.parent == Some(key)) {
                    other.parent = None;
                }
                (200, out)
            }
        };
        self.record(seen(
            "DELETE",
            &path,
            if code == 200 {
                "channel_deleted"
            } else {
                "refused"
            },
        ));
        reply(stream, code, &out)
    }
}
