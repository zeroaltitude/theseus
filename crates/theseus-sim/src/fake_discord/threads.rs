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
    buttons, message_json, now_ms, query, reply, rows, seen, v2_refusal, v2_text, FakeDiscord, Msg,
    Seen, APP_ID, BOT_ID, DEFAULT_GUILD, IS_COMPONENTS_V2,
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
    /// The create's `type`, as sent (0 when it sent none: Discord then makes
    /// a private thread in a text channel, theseus-i3do6) and its
    /// `auto_archive_duration` in minutes (0 when it sent none: Discord's
    /// default is 4,320).
    pub kind: u64,
    pub auto_archive: u64,
    /// Every name it has had, the create's first (a rename is tightly
    /// limited, so the tests count them).
    pub names: Vec<String>,
    /// A forum post's tags now (ids), as the last create or edit set them,
    /// and every set of them in order.
    pub tags: Vec<String>,
    pub tag_sets: Vec<Vec<String>>,
    /// When the create and each edit of its tags landed (ms), for the pace.
    pub tag_times: Vec<u64>,
}

/// A tag of a forum, as it holds them.
#[derive(Debug, Clone, Serialize)]
pub struct FakeTag {
    pub id: String,
    pub name: String,
}

/// A webhook the bot made in a channel.
#[derive(Debug, Clone, Serialize)]
pub struct FakeHook {
    pub id: u64,
    pub token: String,
    pub channel: u64,
    pub name: String,
}

/// A forum channel as Discord answers it, with its available tags.
pub(super) fn forum_json(id: &str, tags: &[FakeTag]) -> Value {
    let tags: Vec<Value> = tags
        .iter()
        .map(|t| json!({"id": t.id, "name": t.name, "moderated": false, "emoji_id": null, "emoji_name": null}))
        .collect();
    json!({"id": id, "type": 15, "available_tags": tags, "name": "forum"})
}

/// The ids in a `applied_tags` array.
fn tag_ids(v: &Value) -> Vec<String> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| match t {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
        .collect()
}

/// The ids the stand-in gives threads and webhooks: snowflake-sized, apart
/// from the messages' small counter.
pub(super) const FIRST_ID: u64 = 900_000_000_000_500_000;

pub(super) fn thread_json(t: &FakeThread) -> Value {
    // Without `type`, Discord makes a private thread in a text channel.
    let kind = if t.forum || t.kind != 0 { 11 } else { 12 };
    json!({"id": t.id.to_string(), "type": kind, "guild_id": t.guild.to_string(),
        "name": t.name, "parent_id": t.parent.to_string(), "owner_id": BOT_ID.to_string(),
        "message_count": 0, "member_count": 1, "rate_limit_per_user": 0, "flags": 0,
        "applied_tags": t.tags,
        "thread_metadata": {"archived": t.archived, "auto_archive_duration": t.auto_archive.max(60),
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

    /// `channel` was set a forum.
    fn is_forum(&self, channel: &str) -> bool {
        let c: u64 = channel.parse().unwrap_or(0);
        self.state.lock().unwrap().forums.contains(&c)
    }

    /// A forum that already has these tags (one made before a build added
    /// more), without a request.
    pub fn seed_forum_tags(&self, forum: u64, names: &[&str]) {
        let tags: Vec<FakeTag> = names
            .iter()
            .map(|n| FakeTag {
                id: self.next_big().to_string(),
                name: (*n).to_string(),
            })
            .collect();
        self.state.lock().unwrap().forum_tags.insert(forum, tags);
    }

    /// A forum's tags now, in order.
    pub fn forum_tags(&self, forum: u64) -> Vec<FakeTag> {
        self.state
            .lock()
            .unwrap()
            .forum_tags
            .get(&forum)
            .cloned()
            .unwrap_or_default()
    }

    /// `PATCH /channels/<forum>` with `available_tags`: Discord keeps at most
    /// 20, and a tag without an id is new.
    fn forum_tags_set(&self, stream: TcpStream, id: &str, body: &Value) -> std::io::Result<()> {
        let path = format!("/channels/{id}");
        let sent = body["available_tags"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if sent.len() > 20 {
            self.record(seen("PATCH", &path, "invalid"));
            return reply(
                stream,
                400,
                &json!({"message": "Invalid Form Body", "code": 50035,
                "errors": {"available_tags": "at most 20 tags"}}),
            );
        }
        let tags: Vec<FakeTag> = sent
            .iter()
            .map(|t| FakeTag {
                id: t["id"]
                    .as_str()
                    .map_or_else(|| self.next_big().to_string(), str::to_string),
                name: t["name"].as_str().unwrap_or("").to_string(),
            })
            .collect();
        let c: u64 = id.parse().unwrap_or(0);
        self.state
            .lock()
            .unwrap()
            .forum_tags
            .insert(c, tags.clone());
        self.record(seen("PATCH", &path, "forum_tags"));
        reply(stream, 200, &forum_json(id, &tags))
    }

    pub(super) fn next_big(&self) -> u64 {
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
        let tags = tag_ids(&body["applied_tags"]);
        let known: Vec<String> = self.forum_tags(p).into_iter().map(|t| t.id).collect();
        let archive = body["auto_archive_duration"].as_u64();
        let bad = name.is_empty()
            || (forum && body["message"].is_null())
            || name.chars().count() > 100
            || archive.is_some_and(|a| ![60, 1440, 4320, 10080].contains(&a))
            || tags.len() > 5
            || tags.iter().any(|t| !known.contains(t));
        if bad {
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
            names: vec![name.clone()],
            name,
            archived: false,
            locked: false,
            forum,
            kind: body["type"].as_u64().unwrap_or(0),
            auto_archive: body["auto_archive_duration"].as_u64().unwrap_or(0),
            tag_sets: vec![tags.clone()],
            tag_times: vec![now_ms()],
            tags,
        };
        let mut out = thread_json(&t);
        if forum {
            // A forum post's first message has the post's own id, and may be
            // a Components V2 card, held to the same rules as any create.
            match self.starter_message(&t, &body["message"]) {
                Ok(m) => out["message"] = message_json(&m),
                Err(why) => {
                    self.record(seen("POST", &path, "invalid"));
                    return reply(
                        stream,
                        400,
                        &json!({"message": "Invalid Form Body", "code": 50035,
                        "errors": {"message": why}}),
                    );
                }
            }
        }
        self.state.lock().unwrap().threads.insert(t.id, t);
        self.record(Seen {
            message_id: None,
            ..seen("POST", &path, "thread")
        });
        reply(stream, 200, &out)
    }

    /// A forum post's first message: its id is the post's, its content the
    /// text or the V2 card's text, kept as a create keeps it. It is pushed in
    /// one hold of the state: a message another request pushed between a push
    /// and a read of "the last message" took this one's id, flag and card
    /// (theseus-g8bxk).
    fn starter_message(&self, t: &FakeThread, body: &Value) -> Result<Msg, String> {
        let flags = body["flags"].as_u64().unwrap_or(0);
        if let Some(why) = v2_refusal(flags, &body["content"], &body["components"]) {
            return Err(why);
        }
        let v2 = flags & IS_COMPONENTS_V2 != 0;
        let content = if v2 {
            v2_text(&body["components"])
        } else {
            body["content"].as_str().unwrap_or("").to_string()
        };
        let mut st = self.state.lock().unwrap();
        // The id the message would have had; the next create's is the same as before.
        st.next_id += 1;
        let m = Msg {
            id: t.id.to_string(),
            channel: t.id.to_string(),
            versions: vec![content.clone()],
            content,
            nonce: None,
            components: rows(&body["components"]),
            reply_to: None,
            edits: 0,
            created_ms: now_ms(),
            allowed_mentions: Value::Null,
            mentions: vec![],
            author: BOT_ID.to_string(),
            hook: None,
            buttons: buttons(&body["components"]),
            components_json: body["components"].clone(),
            pinned: false,
            flags,
        };
        st.messages.push(m.clone());
        Ok(m)
    }

    /// `PATCH /channels/<id>` of a thread: archive, lock, rename.
    pub(super) fn thread_update(
        &self,
        stream: TcpStream,
        id: &str,
        body: &Value,
    ) -> std::io::Result<()> {
        let path = format!("/channels/{id}");
        if let Some((status, out)) = self.forum_patch(id, body) {
            return reply(stream, status, &out);
        }
        // A forum itself: its available tags, whole, as Discord takes them
        // (a tag without an id is new).
        if body["applied_tags"].is_array()
            && body["applied_tags"].as_array().is_some_and(|a| a.len() > 5)
        {
            self.record(seen("PATCH", &path, "invalid"));
            return reply(
                stream,
                400,
                &json!({"message": "Invalid Form Body", "code": 50035,
                "errors": {"applied_tags": "at most 5 tags"}}),
            );
        }
        if body["available_tags"].is_array() && self.is_forum(id) {
            return self.forum_tags_set(stream, id, body);
        }
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
                    t.names.push(n.to_string());
                }
                if body["applied_tags"].is_array() {
                    t.tags = tag_ids(&body["applied_tags"]);
                    t.tag_sets.push(t.tags.clone());
                    t.tag_times.push(now_ms());
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

#[cfg(test)]
mod tests {
    use super::*;

    fn thread(id: u64) -> FakeThread {
        FakeThread {
            id,
            parent: 1,
            guild: DEFAULT_GUILD,
            name: "post".into(),
            archived: false,
            locked: false,
            forum: true,
            kind: 0,
            auto_archive: 0,
            names: vec!["post".into()],
            tags: vec![],
            tag_sets: vec![vec![]],
            tag_times: vec![now_ms()],
        }
    }

    /// A message the bot's reply create would push into `channel`: one hold
    /// of the state, as `create` takes.
    fn push_reply(fake: &FakeDiscord, channel: &str) {
        let mut st = fake.state.lock().unwrap();
        st.next_id += 1;
        let m = Msg {
            id: st.next_id.to_string(),
            channel: channel.to_string(),
            content: "Started both.".into(),
            nonce: None,
            components: 0,
            reply_to: None,
            edits: 0,
            created_ms: now_ms(),
            allowed_mentions: Value::Null,
            mentions: vec![],
            author: BOT_ID.to_string(),
            hook: None,
            versions: vec!["Started both.".into()],
            buttons: vec![],
            components_json: json!([]),
            pinned: false,
            flags: 0,
        };
        st.messages.push(m);
    }

    /// A forum post's first message is pushed whole (theseus-g8bxk). It was
    /// pushed, and then "the last message" was read back to be given the
    /// post's id, flag and card, so a create that landed between the two
    /// steps (a reply in the owner's DM, say) was given them instead: the DM
    /// then held a V2 message with the reply's words and a task's card, in
    /// 1 to 7 runs in 100 of the task home's test, more as notices added
    /// creates. Creates are pushed from two threads while 3,000 posts are
    /// made; no DM message may change, and each post keeps its own message.
    #[test]
    fn a_post_made_beside_other_creates_never_takes_their_message() {
        const DM: &str = "7";
        // A fixed count per pusher, not a spin until the main thread is done:
        // the test's size is then a property of the test and not of the host's
        // core count, allocator and mutex handoff. 20,000 creates against
        // 3,000 posts is far more pressure than the window this row measured.
        const PUSHES: usize = 20_000;
        let fake = FakeDiscord::start();
        let pushers: Vec<_> = (0..2)
            .map(|_| {
                let fake = fake.clone();
                std::thread::spawn(move || {
                    for _ in 0..PUSHES {
                        push_reply(&fake, DM);
                    }
                })
            })
            .collect();
        let card = json!([{"type": 17, "components": [{"type": 10, "content": "the card"}]}]);
        let body = json!({"flags": IS_COMPONENTS_V2, "components": card});
        let posts: Vec<u64> = (0..3000).map(|_| fake.next_big()).collect();
        for id in &posts {
            fake.starter_message(&thread(*id), &body).unwrap();
        }
        for p in pushers {
            p.join().unwrap();
        }
        let dm = fake.messages(7);
        assert!(!dm.is_empty());
        // Hoisted: `p.to_string()` inside the inner loop allocated once per
        // (message, post) pair, which is `dm.len() * 3000` allocations.
        let post_ids: std::collections::HashSet<String> =
            posts.iter().map(|p| p.to_string()).collect();
        for m in &dm {
            assert!(
                m.flags == 0 && m.components_json == json!([]) && !post_ids.contains(&m.id),
                "a DM message was taken for a post's: {m:#?}"
            );
        }
        for id in &posts {
            let own = fake.messages(*id);
            assert!(
                own.len() == 1 && own[0].id == id.to_string() && own[0].flags == IS_COMPONENTS_V2,
                "post {id}: {own:#?}"
            );
        }
    }
}
