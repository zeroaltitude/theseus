//! Scheduled events for the stand-in (theseus-4zc4d): a guild's Events tab as
//! the calendar mirror writes it. `POST /guilds/<g>/scheduled-events` makes an
//! external event, `GET` lists them, `PATCH .../<id>` edits one or moves its
//! status, `DELETE` removes it. It keeps Discord's rules that the mirror must
//! honour: an external event needs a location and an end time, a name is at
//! most 100 characters and a description 1,000, a status moves only
//! scheduled → active → completed (or scheduled → canceled) and a finished
//! event can no longer be edited, and a guild holds at most `event_limit`
//! active or scheduled events (Discord's answer is 400, code 30038).

use std::net::TcpStream;

use serde::Serialize;
use serde_json::{json, Value};

use super::{reply, reply_empty, seen, FakeDiscord, BOT_ID, DEFAULT_GUILD};

/// Discord's published limit on a guild's active and scheduled events.
pub const GUILD_EVENT_LIMIT: usize = 100;

/// A scheduled event the stand-in holds.
#[derive(Debug, Clone, Serialize)]
pub struct FakeEvent {
    pub id: u64,
    pub guild: u64,
    pub name: String,
    pub description: String,
    pub location: String,
    pub starts: String,
    pub ends: String,
    /// 1 scheduled, 2 active, 3 completed, 4 canceled.
    pub status: u8,
    /// The writes that touched it, in order: `create`, `edit`, `status:<n>`.
    pub history: Vec<String>,
}

impl FakeEvent {
    fn json(&self) -> Value {
        json!({"id": self.id.to_string(), "guild_id": self.guild.to_string(),
            "creator_id": BOT_ID.to_string(), "name": self.name,
            "description": self.description, "scheduled_start_time": self.starts,
            "scheduled_end_time": self.ends, "privacy_level": 2, "status": self.status,
            "entity_type": 3, "entity_id": null,
            "entity_metadata": {"location": self.location}})
    }

    fn open(&self) -> bool {
        self.status < 3
    }
}

fn bad(code: u32, message: &str) -> Value {
    json!({"message": message, "code": code})
}

impl FakeDiscord {
    /// The scheduled events the bot made, in order (settled ones too).
    pub fn events(&self) -> Vec<FakeEvent> {
        self.state.lock().unwrap().events.clone()
    }

    /// Every request that reached a guild's scheduled-events routes.
    pub fn event_requests(&self) -> Vec<super::Seen> {
        self.seen()
            .into_iter()
            .filter(|s| s.path.contains("/scheduled-events"))
            .collect()
    }

    /// A guild holds this many active or scheduled events at most; a create
    /// past it is refused with code 30038.
    pub fn set_event_limit(&self, limit: usize) {
        self.state.lock().unwrap().event_limit = Some(limit);
    }

    /// The bot lacks Manage Events: every scheduled-events route answers 403.
    pub fn refuse_events(&self, refuse: bool) {
        self.state.lock().unwrap().refuse_events = refuse;
    }

    /// Put an event the bot did not make in the guild (a person's own), so
    /// the mirror's room is counted against it.
    pub fn add_foreign_event(&self, name: &str) -> u64 {
        let mut st = self.state.lock().unwrap();
        st.next_event += 1;
        let id = st.next_event;
        let guild = st.guild.as_ref().map_or(DEFAULT_GUILD, |g| g.id);
        st.events.push(FakeEvent {
            id,
            guild,
            name: name.into(),
            description: String::new(),
            location: "elsewhere".into(),
            starts: "2099-01-01T00:00:00+00:00".into(),
            ends: "2099-01-01T01:00:00+00:00".into(),
            status: 1,
            history: vec!["create".into()],
        });
        id
    }

    /// `/guilds/<g>/scheduled-events[/<id>]`.
    pub(super) fn event_route(
        &self,
        stream: TcpStream,
        method: &str,
        route: &str,
        parts: (&str, Option<&str>),
        body: &Value,
    ) -> std::io::Result<()> {
        let (guild, id) = parts;
        let (known, refuse) = {
            let st = self.state.lock().unwrap();
            let g = st.guild.as_ref().map_or(DEFAULT_GUILD, |g| g.id);
            (g.to_string() == guild, st.refuse_events)
        };
        if !known {
            self.record(seen(method, route, "unknown_guild"));
            return reply(stream, 404, &bad(10004, "Unknown Guild"));
        }
        if refuse {
            self.record(seen(method, route, "refused"));
            return reply(stream, 403, &bad(50013, "Missing Permissions"));
        }
        match (method, id) {
            ("GET", None) => {
                self.record(seen(method, route, "listed"));
                let all: Vec<Value> = {
                    let st = self.state.lock().unwrap();
                    st.events
                        .iter()
                        .filter(|e| e.open())
                        .map(|e| e.json())
                        .collect()
                };
                reply(stream, 200, &json!(all))
            }
            ("POST", None) => self.event_make(stream, route, guild, body),
            ("PATCH", Some(id)) => self.event_edit(stream, route, id, body),
            ("DELETE", Some(id)) => {
                let gone = {
                    let mut st = self.state.lock().unwrap();
                    let n = st.events.len();
                    st.events.retain(|e| e.id.to_string() != id);
                    st.events.len() < n
                };
                if gone {
                    self.record(seen(method, route, "deleted"));
                    reply_empty(stream)
                } else {
                    self.record(seen(method, route, "missing"));
                    reply(stream, 404, &bad(10070, "Unknown Guild Scheduled Event"))
                }
            }
            _ => {
                self.record(seen(method, route, "unknown"));
                reply(stream, 404, &bad(10000, "Unknown (fake)"))
            }
        }
    }

    fn event_make(
        &self,
        stream: TcpStream,
        route: &str,
        guild: &str,
        body: &Value,
    ) -> std::io::Result<()> {
        let text = |k: &str| body[k].as_str().unwrap_or("").to_string();
        let (name, description) = (text("name"), text("description"));
        let (starts, ends) = (text("scheduled_start_time"), text("scheduled_end_time"));
        let location = body["entity_metadata"]["location"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let invalid = name.is_empty()
            || name.chars().count() > 100
            || description.chars().count() > 1000
            || body["entity_type"] != 3
            || starts.is_empty()
            || ends.is_empty()
            || location.is_empty();
        if invalid {
            self.record(seen("POST", route, "invalid"));
            return reply(stream, 400, &bad(50035, "Invalid Form Body"));
        }
        let mut st = self.state.lock().unwrap();
        let limit = st.event_limit.unwrap_or(GUILD_EVENT_LIMIT);
        if st.events.iter().filter(|e| e.open()).count() >= limit {
            drop(st);
            self.record(seen("POST", route, "limit"));
            return reply(
                stream,
                400,
                &bad(30038, "Maximum number of active scheduled events reached"),
            );
        }
        st.next_event += 1;
        let e = FakeEvent {
            id: st.next_event,
            guild: guild.parse().unwrap_or(0),
            name,
            description,
            location,
            starts,
            ends,
            status: 1,
            history: vec!["create".into()],
        };
        let out = e.json();
        st.events.push(e);
        drop(st);
        self.record(seen("POST", route, "event_created"));
        reply(stream, 200, &out)
    }

    fn event_edit(
        &self,
        stream: TcpStream,
        route: &str,
        id: &str,
        body: &Value,
    ) -> std::io::Result<()> {
        let mut st = self.state.lock().unwrap();
        let Some(e) = st.events.iter_mut().find(|e| e.id.to_string() == id) else {
            drop(st);
            self.record(seen("PATCH", route, "missing"));
            return reply(stream, 404, &bad(10070, "Unknown Guild Scheduled Event"));
        };
        let refuse = |this: &Self, stream, why: &str| {
            this.record(seen("PATCH", route, "invalid"));
            reply(stream, 400, &bad(50035, why))
        };
        if !e.open() {
            drop(st);
            return refuse(self, stream, "A finished event can no longer be edited");
        }
        if let Some(to) = body["status"].as_u64() {
            let ok = matches!((e.status, to), (1, 2 | 4) | (2, 3));
            if !ok {
                drop(st);
                return refuse(self, stream, "Invalid status transition");
            }
            e.status = to as u8;
            e.history.push(format!("status:{to}"));
        }
        let mut edited = false;
        for (key, field) in [
            ("name", &mut e.name),
            ("description", &mut e.description),
            ("scheduled_start_time", &mut e.starts),
            ("scheduled_end_time", &mut e.ends),
        ] {
            if let Some(v) = body[key].as_str() {
                *field = v.to_string();
                edited = true;
            }
        }
        if let Some(v) = body["entity_metadata"]["location"].as_str() {
            e.location = v.to_string();
            edited = true;
        }
        if e.name.chars().count() > 100 || e.description.chars().count() > 1000 {
            drop(st);
            return refuse(self, stream, "Invalid Form Body");
        }
        if edited {
            e.history.push("edit".into());
        }
        let out = e.json();
        drop(st);
        self.record(seen("PATCH", route, "event_edited"));
        reply(stream, 200, &out)
    }
}
