//! A stand-in for Discord's REST API on 127.0.0.1 (theseus-q4v), for tests
//! and scratch daemons. The binding's `[discord] rest_proxy` sends every
//! request here over plain http (twilight's proxy mode), so no request and no
//! token leaves the machine.
//!
//! It keeps each channel's messages, and a create's nonce is honored as
//! Discord honors it under `enforce_nonce`: a second create with the same
//! nonce in the same channel returns the first message and posts nothing.
//! Unlike Discord's, its nonce window never closes, unless `nonce_window_ms`
//! is set.
//!
//! It can be told to be away:
//! - `down`: every connection is dropped unanswered;
//! - `hang-creates`: a create is posted, then left unanswered (the send landed
//!   and its answer was lost);
//! - `fail`: every request gets a 503.
//!
//! And a test can hold one write in flight (`hold_writes_containing`): a
//! create or edit whose content holds a text is applied, and its answer waits
//! until the hold is lifted (a post in flight while the daemon stops,
//! theseus-pfv).
//!
//! It records each request's method, path, and body, and never a header, so
//! an `Authorization` header is never kept or logged.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};

/// The fake bot's user id, and its application's.
pub const BOT_ID: u64 = 1_553_557_000_000_000_001;
pub const APP_ID: u64 = 1_553_557_000_000_000_002;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    Up,
    Down,
    HangCreates,
    Fail,
}

impl Mode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "up" => Some(Mode::Up),
            "down" => Some(Mode::Down),
            "hang-creates" => Some(Mode::HangCreates),
            "fail" => Some(Mode::Fail),
            _ => None,
        }
    }
}

/// A message as the fake keeps it.
#[derive(Debug, Clone, Serialize)]
pub struct Msg {
    pub id: String,
    pub channel: String,
    pub content: String,
    pub nonce: Option<String>,
    pub components: usize,
    pub reply_to: Option<String>,
    pub edits: u32,
    pub created_ms: u64,
    /// The create's `allowed_mentions`, as sent (theseus-9j9); null when it
    /// sent none, which Discord reads as "parse everything".
    pub allowed_mentions: Value,
    /// The users the create notified, as Discord's `mentions` answers: each
    /// one whose `<@id>` the content carries and `allowed_mentions` allows.
    /// An edit notifies nobody, and the fake leaves this as the create set it.
    pub mentions: Vec<String>,
}

/// Whom a create notifies, as Discord decides it: the users the content
/// mentions (`<@id>` or `<@!id>`) that `allowed` allows, by `users`, or by
/// `parse` naming "users"; with no `allowed_mentions`, all of them.
pub fn notified(content: &str, allowed: &Value) -> Vec<String> {
    let mut named = Vec::new();
    let mut rest = content;
    while let Some(at) = rest.find("<@") {
        rest = &rest[at + 2..];
        let id: String = rest
            .trim_start_matches('!')
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        let close = rest.trim_start_matches('!').get(id.len()..id.len() + 1) == Some(">");
        if !id.is_empty() && close && !named.contains(&id) {
            named.push(id);
        }
    }
    if allowed.is_null() {
        return named;
    }
    let parse_users = allowed["parse"]
        .as_array()
        .is_some_and(|p| p.iter().any(|v| v == "users"));
    let users: Vec<&str> = allowed["users"]
        .as_array()
        .map(|u| u.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    named
        .into_iter()
        .filter(|id| parse_users || users.contains(&id.as_str()))
        .collect()
}

/// One request, as recorded: never its headers.
#[derive(Debug, Clone, Serialize)]
pub struct Seen {
    pub at_ms: u64,
    pub method: String,
    pub path: String,
    /// `created`, `deduped` (a nonce seen before), `edited`, `typing`,
    /// `dropped` (down), `hung`, `held` (a write whose answer waits for its
    /// hold), `failed`, `other`, or `unknown`.
    pub outcome: String,
    pub message_id: Option<String>,
    pub nonce: Option<String>,
    pub chars: Option<usize>,
}

#[derive(Default)]
struct State {
    mode: Option<Mode>,
    messages: Vec<Msg>,
    seen: Vec<Seen>,
    next_id: u64,
    nonce_window_ms: Option<u64>,
    /// Every answer waits this long: a slow Discord.
    delay_ms: u64,
    /// A write whose content holds this text waits for its answer until the
    /// hold is lifted.
    hold: Option<String>,
}

pub struct FakeDiscord {
    /// `127.0.0.1:<port>`, for `[discord] rest_proxy`.
    pub addr: String,
    state: Arc<Mutex<State>>,
    /// A file whose first line is the mode, read at every request (the CLI).
    control: Option<PathBuf>,
    /// Where each request and the messages are written (the CLI).
    log: Option<PathBuf>,
}

impl FakeDiscord {
    /// Listen on an ephemeral port; answer until the process ends.
    pub fn start() -> Arc<Self> {
        Self::start_on("127.0.0.1:0", None, None).expect("a local port")
    }

    /// Listen on `addr`. `control` names a file holding the mode; `log`, a
    /// JSONL file of requests, beside which `<log>.messages.json` keeps the
    /// messages.
    pub fn start_on(
        addr: &str,
        control: Option<PathBuf>,
        log: Option<PathBuf>,
    ) -> std::io::Result<Arc<Self>> {
        let listener = TcpListener::bind(addr)?;
        let me = Arc::new(Self {
            addr: listener.local_addr()?.to_string(),
            state: Arc::new(Mutex::new(State {
                next_id: 1_600_000_000_000_000_000,
                ..State::default()
            })),
            control,
            log,
        });
        let fake = me.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let fake = fake.clone();
                std::thread::spawn(move || {
                    let _ = fake.answer(stream);
                });
            }
        });
        Ok(me)
    }

    pub fn set_mode(&self, mode: Mode) {
        self.state.lock().unwrap().mode = Some(mode);
    }

    /// Discord keeps a nonce for a few minutes; the fake, forever unless told.
    pub fn set_nonce_window_ms(&self, ms: u64) {
        self.state.lock().unwrap().nonce_window_ms = Some(ms);
    }

    /// Answer every request this much later.
    pub fn set_delay_ms(&self, ms: u64) {
        self.state.lock().unwrap().delay_ms = ms;
    }

    /// Hold the answer to each create or edit whose content holds `text`:
    /// the write is applied and recorded as `held`, and answered once the
    /// hold is lifted (`None`), or after a minute (theseus-pfv).
    pub fn hold_writes_containing(&self, text: Option<&str>) {
        self.state.lock().unwrap().hold = text.map(str::to_string);
    }

    /// Whether a write of `content` is held now.
    fn holding(&self, content: &str) -> bool {
        let st = self.state.lock().unwrap();
        st.hold.as_deref().is_some_and(|h| content.contains(h))
    }

    /// A held write's wait: until its hold is lifted, or a minute passes.
    fn wait_released(&self, content: &str) {
        for _ in 0..60_000 {
            if !self.holding(content) {
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn mode(&self) -> Mode {
        if let Some(m) = self
            .control
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| s.lines().next().and_then(Mode::parse))
        {
            return m;
        }
        self.state.lock().unwrap().mode.unwrap_or(Mode::Up)
    }

    /// Every message in `channel`, in the order they were created.
    pub fn messages(&self, channel: u64) -> Vec<Msg> {
        let c = channel.to_string();
        self.state
            .lock()
            .unwrap()
            .messages
            .iter()
            .filter(|m| m.channel == c)
            .cloned()
            .collect()
    }

    pub fn all_messages(&self) -> Vec<Msg> {
        self.state.lock().unwrap().messages.clone()
    }

    pub fn seen(&self) -> Vec<Seen> {
        self.state.lock().unwrap().seen.clone()
    }

    fn record(&self, s: Seen) {
        let mut st = self.state.lock().unwrap();
        if let Some(log) = &self.log {
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(log)
            {
                let _ = writeln!(f, "{}", serde_json::to_string(&s).unwrap_or_default());
            }
            let snapshot = log.with_extension("messages.json");
            let _ = std::fs::write(
                snapshot,
                serde_json::to_string_pretty(&st.messages).unwrap_or_default(),
            );
        }
        st.seen.push(s);
    }

    fn answer(&self, stream: TcpStream) -> std::io::Result<()> {
        let mode = self.mode();
        if mode == Mode::Down {
            // Away: the connection closes before any answer.
            self.record(seen("?", "?", "dropped"));
            drop(stream);
            return Ok(());
        }
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
        let mut r = BufReader::new(stream.try_clone()?);
        let mut line = String::new();
        if r.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let mut parts = line.split_whitespace();
        let method = parts.next().unwrap_or("").to_string();
        let full = parts.next().unwrap_or("").to_string();
        let path = full.split('?').next().unwrap_or("").to_string();
        let mut len = 0usize;
        loop {
            let mut h = String::new();
            if r.read_line(&mut h)? == 0 {
                break;
            }
            let h = h.trim_end();
            if h.is_empty() {
                break;
            }
            // Only the body's length is read; every other header, the
            // token's among them, is dropped unread.
            if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                len = v.trim().parse().unwrap_or(0);
            }
        }
        let mut body = vec![0; len];
        r.read_exact(&mut body)?;
        let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        let delay = self.state.lock().unwrap().delay_ms;
        if delay > 0 {
            std::thread::sleep(Duration::from_millis(delay));
        }
        if mode == Mode::Fail {
            self.record(seen(&method, &path, "failed"));
            return reply(
                stream,
                503,
                &json!({"message": "Service Unavailable (fake)", "code": 0}),
            );
        }
        let route = path.trim_start_matches("/api/v10");
        let segs: Vec<&str> = route.trim_matches('/').split('/').collect();
        match (method.as_str(), segs.as_slice()) {
            ("GET", ["users", "@me"]) => {
                self.record(seen(&method, route, "other"));
                reply(
                    stream,
                    200,
                    &json!({"id": BOT_ID.to_string(), "username": "Theseus (fake)",
                    "discriminator": "0000", "bot": true, "mfa_enabled": false}),
                )
            }
            ("GET", ["oauth2", "applications", "@me"] | ["applications", "@me"]) => {
                self.record(seen(&method, route, "other"));
                reply(
                    stream,
                    200,
                    &json!({"id": APP_ID.to_string(), "name": "theseus-fake", "description": "",
                    "bot_public": false, "bot_require_code_grant": false, "verify_key": "", "flags": 0}),
                )
            }
            ("GET", ["users", "@me", "guilds"]) => {
                self.record(seen(&method, route, "other"));
                reply(stream, 200, &json!([]))
            }
            ("PUT", ["applications", _, "commands"]) => {
                self.record(seen(&method, route, "other"));
                reply(stream, 200, &json!([]))
            }
            ("POST", ["users", "@me", "channels"]) => {
                self.record(seen(&method, route, "other"));
                // A DM's channel: the user's id plus one, stable per user.
                let user: u64 = body["recipient_id"]
                    .as_str()
                    .and_then(|s| s.parse().ok())
                    .or_else(|| body["recipient_id"].as_u64())
                    .unwrap_or(0);
                reply(
                    stream,
                    200,
                    &json!({"id": (user + 1).to_string(), "type": 1}),
                )
            }
            ("POST", ["channels", c, "typing"]) => {
                self.record(Seen {
                    message_id: None,
                    ..seen(&method, &format!("/channels/{c}/typing"), "typing")
                });
                reply_empty(stream)
            }
            ("POST", ["channels", c, "messages"]) => self.create(stream, c, &body, mode),
            ("PATCH", ["channels", c, "messages", m]) => self.edit(stream, c, m, &body),
            _ => {
                self.record(seen(&method, route, "unknown"));
                reply(
                    stream,
                    404,
                    &json!({"message": "Unknown (fake)", "code": 10000}),
                )
            }
        }
    }

    fn create(
        &self,
        stream: TcpStream,
        channel: &str,
        body: &Value,
        mode: Mode,
    ) -> std::io::Result<()> {
        let now = now_ms();
        let nonce = match &body["nonce"] {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        };
        let enforce = body["enforce_nonce"].as_bool().unwrap_or(false);
        let (msg, outcome) = {
            let mut st = self.state.lock().unwrap();
            let window = st.nonce_window_ms;
            let first = nonce.as_ref().filter(|_| enforce).and_then(|n| {
                st.messages.iter().find(|m| {
                    m.channel == channel
                        && m.nonce.as_ref() == Some(n)
                        && window.is_none_or(|w| now.saturating_sub(m.created_ms) <= w)
                })
            });
            match first {
                Some(m) => (m.clone(), "deduped"),
                None => {
                    st.next_id += 1;
                    let content = body["content"].as_str().unwrap_or("").to_string();
                    let allowed = body["allowed_mentions"].clone();
                    let m = Msg {
                        id: st.next_id.to_string(),
                        channel: channel.to_string(),
                        mentions: notified(&content, &allowed),
                        content,
                        nonce: nonce.clone(),
                        components: body["components"].as_array().map_or(0, Vec::len),
                        reply_to: body["message_reference"]["message_id"]
                            .as_str()
                            .map(str::to_string),
                        edits: 0,
                        created_ms: now,
                        allowed_mentions: allowed,
                    };
                    st.messages.push(m.clone());
                    (m, "created")
                }
            }
        };
        let hang = mode == Mode::HangCreates;
        let hold = !hang && self.holding(&msg.content);
        self.record(Seen {
            at_ms: now,
            method: "POST".into(),
            path: format!("/channels/{channel}/messages"),
            outcome: match (hang, hold) {
                (true, _) => "hung".into(),
                (_, true) => "held".into(),
                _ => outcome.into(),
            },
            message_id: Some(msg.id.clone()),
            nonce,
            chars: Some(msg.content.chars().count()),
        });
        if hold {
            // The create landed, and its answer waits for the hold.
            self.wait_released(&msg.content);
        }
        if hang {
            // The create landed; its answer is lost. Hold the connection
            // until told otherwise or a minute passes, then close it unanswered.
            for _ in 0..600 {
                if self.mode() != Mode::HangCreates {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            drop(stream);
            return Ok(());
        }
        reply(stream, 200, &message_json(&msg))
    }

    fn edit(
        &self,
        stream: TcpStream,
        channel: &str,
        id: &str,
        body: &Value,
    ) -> std::io::Result<()> {
        let found = {
            let mut st = self.state.lock().unwrap();
            st.messages
                .iter_mut()
                .find(|m| m.channel == channel && m.id == id)
                .map(|m| {
                    if let Some(c) = body["content"].as_str() {
                        m.content = c.to_string();
                    }
                    if let Some(c) = body["components"].as_array() {
                        m.components = c.len();
                    }
                    m.edits += 1;
                    m.clone()
                })
        };
        let path = format!("/channels/{channel}/messages/{id}");
        match found {
            Some(m) => {
                let hold = self.holding(&m.content);
                self.record(Seen {
                    message_id: Some(m.id.clone()),
                    chars: Some(m.content.chars().count()),
                    ..seen("PATCH", &path, if hold { "held" } else { "edited" })
                });
                if hold {
                    // The edit landed, and its answer waits for the hold.
                    self.wait_released(&m.content);
                }
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

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

fn seen(method: &str, path: &str, outcome: &str) -> Seen {
    Seen {
        at_ms: now_ms(),
        method: method.into(),
        path: path.into(),
        outcome: outcome.into(),
        message_id: None,
        nonce: None,
        chars: None,
    }
}

/// A message as Discord answers it: the fields twilight's model requires,
/// and the users it notified as its `mentions` (theseus-9j9).
fn message_json(m: &Msg) -> Value {
    let mentions: Vec<Value> = m
        .mentions
        .iter()
        .map(|id| json!({"id": id, "username": format!("user-{id}"), "discriminator": "0000", "avatar": null, "bot": false, "public_flags": 0}))
        .collect();
    json!({
        "id": m.id, "channel_id": m.channel, "content": m.content,
        "author": {"id": BOT_ID.to_string(), "username": "Theseus (fake)", "discriminator": "0000", "bot": true},
        "timestamp": "2026-09-30T00:00:00.000000+00:00", "edited_timestamp": null, "tts": false,
        "mention_everyone": false, "mentions": mentions, "mention_roles": [], "attachments": [], "embeds": [],
        "pinned": false, "type": 0, "nonce": m.nonce,
    })
}

fn reply(mut stream: TcpStream, status: u16, body: &Value) -> std::io::Result<()> {
    let text = body.to_string();
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        503 => "Service Unavailable",
        _ => "Status",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{text}",
        text.len()
    )?;
    stream.flush()
}

fn reply_empty(mut stream: TcpStream) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 204 No Content\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
    )?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Discord notifies a mentioned user only when `allowed_mentions` allows
    /// it, and every mentioned user when a create sends none (theseus-9j9).
    #[test]
    fn a_create_notifies_only_the_mentions_it_allows() {
        let text = "<@101> <@!202> look, and <@303 and <@101> again";
        assert_eq!(notified(text, &Value::Null), ["101", "202"]);
        assert!(notified(text, &json!({"parse": [], "replied_user": false})).is_empty());
        assert_eq!(
            notified(text, &json!({"parse": [], "users": ["202", "404"]})),
            ["202"]
        );
        assert_eq!(notified(text, &json!({"parse": ["users"]})), ["101", "202"]);
    }
}
