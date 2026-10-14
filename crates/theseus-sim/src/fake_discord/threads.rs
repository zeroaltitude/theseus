//! Threads and webhooks for the stand-in (theseus-sjzm): a thread made in a
//! text channel or a forum (`POST /channels/<id>/threads`), archived,
//! unarchived and renamed (`PATCH /channels/<id>`), read back
//! (`GET /channels/<id>`); a channel's webhooks listed and made, which a bot
//! without Manage Webhooks is refused (`refuse_webhooks`); and a webhook's
//! messages created and edited by their token, the way a persona's reply is
//! signed. A message a webhook wrote keeps the name it was sent under
//! (`Msg::hook`) and the webhook's id as its author.

use std::io::Write as _;
use std::net::TcpStream;

use serde::Serialize;
use serde_json::{json, Value};

use super::{
    message_json, now_ms, query, reply, seen, FakeDiscord, Msg, Seen, APP_ID, BOT_ID, DEFAULT_GUILD,
};

/// A thread the bot made in a channel.
#[derive(Debug, Clone, Serialize)]
pub struct FakeThread {
    pub id: u64,
    pub parent: u64,
    pub guild: u64,
    pub name: String,
    pub archived: bool,
    pub locked: bool,
    /// `POST /channels/<parent>/threads` or the forum's: how it was made.
    pub forum: bool,
}

/// A webhook the bot made in a channel.
#[derive(Debug, Clone, Serialize)]
pub struct FakeHook {
    pub id: u64,
    pub token: String,
    pub channel: u64,
    pub name: String,
}

/// The ids the stand-in gives threads and webhooks: snowflake-sized, apart
/// from the messages' small counter.
const FIRST_ID: u64 = 900_000_000_000_500_000;

pub(super) fn thread_json(t: &FakeThread) -> Value {
    json!({"id": t.id.to_string(), "type": 11, "guild_id": t.guild.to_string(),
        "name": t.name, "parent_id": t.parent.to_string(), "owner_id": BOT_ID.to_string(),
        "message_count": 0, "member_count": 1, "rate_limit_per_user": 0, "flags": 0,
        "thread_metadata": {"archived": t.archived, "auto_archive_duration": 10080,
            "archive_timestamp": "2026-10-14T00:00:00.000000+00:00", "locked": t.locked}})
}

fn hook_json(h: &FakeHook, guild: u64) -> Value {
    json!({"id": h.id.to_string(), "type": 1, "channel_id": h.channel.to_string(),
        "guild_id": guild.to_string(), "name": h.name, "avatar": null,
        "token": h.token, "application_id": APP_ID.to_string()})
}

fn missing_permissions() -> Value {
    json!({"message": "Missing Permissions", "code": 50013})
}

impl FakeDiscord {
    /// The threads the bot made, in order.
    pub fn threads(&self) -> Vec<FakeThread> {
        self.state
            .lock()
            .unwrap()
            .threads
            .values()
            .cloned()
            .collect()
    }

    /// The webhooks the bot made, in order.
    pub fn webhooks(&self) -> Vec<FakeHook> {
        self.state.lock().unwrap().webhooks.clone()
    }

    /// Whether `id` is a webhook the bot made (any other `/webhooks/<id>` is
    /// an interaction's).
    pub(super) fn is_hook(&self, id: &str) -> bool {
        let st = self.state.lock().unwrap();
        st.webhooks.iter().any(|h| h.id.to_string() == id)
    }

    /// `channel` is a forum: a thread made in it needs a first message.
    pub fn set_forum(&self, channel: u64) {
        self.state.lock().unwrap().forums.insert(channel);
    }

    /// The bot lacks Manage Webhooks: listing or making one answers 403.
    pub fn refuse_webhooks(&self, refuse: bool) {
        self.state.lock().unwrap().refuse_webhooks = refuse;
    }

    fn next_big(&self) -> u64 {
        let mut st = self.state.lock().unwrap();
        st.next_big = st.next_big.max(FIRST_ID) + 1;
        st.next_big
    }

    /// `POST /channels/<id>/threads`: a thread, with a first message in a forum.
    pub(super) fn thread_create(
        &self,
        stream: TcpStream,
        parent: &str,
        body: &Value,
    ) -> std::io::Result<()> {
        let path = format!("/channels/{parent}/threads");
        let p: u64 = parent.parse().unwrap_or(0);
        let (forum, guild) = {
            let st = self.state.lock().unwrap();
            let guild = st.guild.as_ref().map_or(DEFAULT_GUILD, |g| g.id);
            (st.forums.contains(&p), guild)
        };
        let name = body["name"].as_str().unwrap_or("").to_string();
        if name.is_empty() || (forum && body["message"].is_null()) {
            self.record(seen("POST", &path, "refused"));
            return reply(
                stream,
                400,
                &json!({"message": "Invalid Form Body", "code": 50035}),
            );
        }
        let t = FakeThread {
            id: self.next_big(),
            parent: p,
            guild,
            name,
            archived: false,
            locked: false,
            forum,
        };
        let mut out = thread_json(&t);
        self.state.lock().unwrap().threads.insert(t.id, t.clone());
        if forum {
            let content = body["message"]["content"].as_str().unwrap_or("");
            let m = self.push_bot_message(&t.id.to_string(), content);
            out["message"] = message_json(&m);
        }
        self.record(Seen {
            message_id: None,
            ..seen("POST", &path, "thread")
        });
        reply(stream, 200, &out)
    }

    /// A message the bot wrote, kept in `channel`.
    fn push_bot_message(&self, channel: &str, content: &str) -> Msg {
        let mut st = self.state.lock().unwrap();
        st.next_id += 1;
        let m = Msg {
            id: st.next_id.to_string(),
            channel: channel.to_string(),
            content: content.to_string(),
            nonce: None,
            components: 0,
            reply_to: None,
            edits: 0,
            created_ms: now_ms(),
            allowed_mentions: Value::Null,
            mentions: vec![],
            author: BOT_ID.to_string(),
            hook: None,
            versions: vec![content.to_string()],
            buttons: vec![],
            components_json: json!([]),
            pinned: false,
            flags: 0,
        };
        st.messages.push(m.clone());
        m
    }

    /// `PATCH /channels/<id>` of a thread: archive, lock, rename.
    pub(super) fn thread_update(
        &self,
        stream: TcpStream,
        id: &str,
        body: &Value,
    ) -> std::io::Result<()> {
        let path = format!("/channels/{id}");
        let found = {
            let mut st = self.state.lock().unwrap();
            st.threads.get_mut(&id.parse().unwrap_or(0)).map(|t| {
                if let Some(a) = body["archived"].as_bool() {
                    t.archived = a;
                }
                if let Some(l) = body["locked"].as_bool() {
                    t.locked = l;
                }
                if let Some(n) = body["name"].as_str() {
                    t.name = n.to_string();
                }
                t.clone()
            })
        };
        match found {
            Some(t) => {
                self.record(seen("PATCH", &path, "thread_updated"));
                reply(stream, 200, &thread_json(&t))
            }
            None => {
                self.record(seen("PATCH", &path, "unknown"));
                reply(
                    stream,
                    404,
                    &json!({"message": "Unknown Channel", "code": 10003}),
                )
            }
        }
    }

    /// `GET /channels/<id>/webhooks`.
    pub(super) fn hooks_read(&self, stream: TcpStream, channel: &str) -> std::io::Result<()> {
        let path = format!("/channels/{channel}/webhooks");
        let st = self.state.lock().unwrap();
        if st.refuse_webhooks {
            drop(st);
            self.record(seen("GET", &path, "refused"));
            return reply(stream, 403, &missing_permissions());
        }
        let c: u64 = channel.parse().unwrap_or(0);
        let guild = st.guild.as_ref().map_or(DEFAULT_GUILD, |g| g.id);
        let list: Vec<Value> = st
            .webhooks
            .iter()
            .filter(|h| h.channel == c)
            .map(|h| hook_json(h, guild))
            .collect();
        drop(st);
        self.record(seen("GET", &path, "read"));
        reply(stream, 200, &json!(list))
    }

    /// `POST /channels/<id>/webhooks`.
    pub(super) fn hook_make(
        &self,
        stream: TcpStream,
        channel: &str,
        body: &Value,
    ) -> std::io::Result<()> {
        let path = format!("/channels/{channel}/webhooks");
        if self.state.lock().unwrap().refuse_webhooks {
            self.record(seen("POST", &path, "refused"));
            return reply(stream, 403, &missing_permissions());
        }
        let h = FakeHook {
            id: self.next_big(),
            token: format!("hook-token-{}", now_ms()),
            channel: channel.parse().unwrap_or(0),
            name: body["name"].as_str().unwrap_or("hook").to_string(),
        };
        let guild = {
            let mut st = self.state.lock().unwrap();
            st.webhooks.push(h.clone());
            st.guild.as_ref().map_or(DEFAULT_GUILD, |g| g.id)
        };
        self.record(seen("POST", &path, "hook_made"));
        reply(stream, 200, &hook_json(&h, guild))
    }

    /// `POST /webhooks/<id>/<token>`: a message under the name it is sent
    /// with, in the hook's channel or the thread named by `thread_id`.
    pub(super) fn hook_execute(
        &self,
        stream: TcpStream,
        (id, token): (&str, &str),
        full: &str,
        body: &Value,
    ) -> std::io::Result<()> {
        let path = format!("/webhooks/{id}/<token>");
        let hook = {
            let st = self.state.lock().unwrap();
            st.webhooks
                .iter()
                .find(|h| h.id.to_string() == id && h.token == token)
                .cloned()
        };
        let Some(h) = hook else {
            self.record(seen("POST", &path, "unknown"));
            return reply(
                stream,
                404,
                &json!({"message": "Unknown Webhook", "code": 10015}),
            );
        };
        let channel = query(full, "thread_id")
            .map(str::to_string)
            .unwrap_or_else(|| h.channel.to_string());
        let content = body["content"].as_str().unwrap_or("").to_string();
        let name = body["username"].as_str().unwrap_or(&h.name).to_string();
        let m = {
            let mut st = self.state.lock().unwrap();
            st.next_id += 1;
            let allowed = body["allowed_mentions"].clone();
            let m = Msg {
                id: st.next_id.to_string(),
                channel,
                mentions: super::notified(&content, &allowed),
                versions: vec![content.clone()],
                content,
                nonce: None,
                components: 0,
                reply_to: None,
                edits: 0,
                created_ms: now_ms(),
                allowed_mentions: allowed,
                author: h.id.to_string(),
                hook: Some(name),
                buttons: vec![],
                components_json: json!([]),
                pinned: false,
                flags: body["flags"].as_u64().unwrap_or(0),
            };
            st.messages.push(m.clone());
            m
        };
        self.record(Seen {
            message_id: Some(m.id.clone()),
            chars: Some(m.content.chars().count()),
            ..seen("POST", &path, "hook_created")
        });
        if query(full, "wait") == Some("true") {
            reply(stream, 200, &message_json(&m))
        } else {
            let mut s = stream;
            write!(
                s,
                "HTTP/1.1 204 No Content\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
            )?;
            s.flush()
        }
    }

    /// `PATCH /webhooks/<id>/<token>/messages/<mid>`: a webhook edits only
    /// its own messages.
    pub(super) fn hook_edit(
        &self,
        stream: TcpStream,
        (id, token, mid): (&str, &str, &str),
        body: &Value,
    ) -> std::io::Result<()> {
        let path = format!("/webhooks/{id}/<token>/messages/{mid}");
        let found = {
            let mut st = self.state.lock().unwrap();
            let ok = st
                .webhooks
                .iter()
                .any(|h| h.id.to_string() == id && h.token == token);
            st.messages
                .iter_mut()
                .find(|m| m.id == mid && m.author == id)
                .filter(|_| ok)
                .map(|m| {
                    if let Some(c) = body["content"].as_str() {
                        if c != m.content {
                            m.versions.push(c.to_string());
                        }
                        m.content = c.to_string();
                    }
                    m.edits += 1;
                    m.clone()
                })
        };
        match found {
            Some(m) => {
                self.record(Seen {
                    message_id: Some(m.id.clone()),
                    chars: Some(m.content.chars().count()),
                    ..seen("PATCH", &path, "hook_edited")
                });
                reply(stream, 200, &message_json(&m))
            }
            None => {
                self.record(seen("PATCH", &path, "unknown"));
                reply(
                    stream,
                    404,
                    &json!({"message": "Unknown Message", "code": 10008}),
                )
            }
        }
    }
}
