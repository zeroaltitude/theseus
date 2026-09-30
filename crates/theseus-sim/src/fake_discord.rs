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
}

/// One request, as recorded: never its headers.
#[derive(Debug, Clone, Serialize)]
pub struct Seen {
    pub at_ms: u64,
    pub method: String,
    pub path: String,
    /// `created`, `deduped` (a nonce seen before), `edited`, `typing`,
    /// `dropped` (down), `hung`, `failed`, `other`, or `unknown`.
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
                    let m = Msg {
                        id: st.next_id.to_string(),
                        channel: channel.to_string(),
                        content: body["content"].as_str().unwrap_or("").to_string(),
                        nonce: nonce.clone(),
                        components: body["components"].as_array().map_or(0, Vec::len),
                        reply_to: body["message_reference"]["message_id"]
                            .as_str()
                            .map(str::to_string),
                        edits: 0,
                        created_ms: now,
                    };
                    st.messages.push(m.clone());
                    (m, "created")
                }
            }
        };
        let hang = mode == Mode::HangCreates;
        self.record(Seen {
            at_ms: now,
            method: "POST".into(),
            path: format!("/channels/{channel}/messages"),
            outcome: if hang { "hung".into() } else { outcome.into() },
            message_id: Some(msg.id.clone()),
            nonce,
            chars: Some(msg.content.chars().count()),
        });
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
                self.record(Seen {
                    message_id: Some(m.id.clone()),
                    chars: Some(m.content.chars().count()),
                    ..seen("PATCH", &path, "edited")
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

/// A message as Discord answers it: the fields twilight's model requires.
fn message_json(m: &Msg) -> Value {
    json!({
        "id": m.id, "channel_id": m.channel, "content": m.content,
        "author": {"id": BOT_ID.to_string(), "username": "Theseus (fake)", "discriminator": "0000", "bot": true},
        "timestamp": "2026-09-30T00:00:00.000000+00:00", "edited_timestamp": null, "tts": false,
        "mention_everyone": false, "mentions": [], "mention_roles": [], "attachments": [], "embeds": [],
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
