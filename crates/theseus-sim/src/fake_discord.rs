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
//!
//! With a gateway (`serve_gateway`, theseus-6g62) it stands in for all of
//! Discord that the binding talks to, so a check can do what only a person
//! can on Discord: type a message (`say`) and press a card's button
//! (`press`), each sent through the gateway as Discord sends it. It answers
//! an interaction's callback, its original response's edit, and its
//! follow-ups, and keeps each answer (`replies`), with the interaction's
//! token out of its log.
//!
//! What the binding posted can be read back (theseus-qifw): each message
//! keeps every content it had (`versions`, the create's and each edit's) and
//! its buttons. A message a user typed is kept too, with its author.
//!
//! A guild (`set_guild`, theseus-ck0k) answers the reads of the binding's
//! viewer check: the application's flags (with the Server Members intent),
//! the channel with its overwrites, the guild with its owner and roles, and
//! its members. Without one, the application has no such intent, as before.
//!
//! A check in another process drives it over its own port, under `/_fake/`,
//! a path Discord's API never uses: `say`, `press`, `guild`, and the reads
//! `messages`, `replies`, and `gateway`.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::fake_gateway::{self, FakeGateway, GatewayState, Where};

/// The fake bot's user id, and its application's.
pub const BOT_ID: u64 = 1_553_557_000_000_000_001;
pub const APP_ID: u64 = 1_553_557_000_000_000_002;

/// The guild a typed message names when no guild is set.
pub const DEFAULT_GUILD: u64 = 900_000_000_000_000_001;

/// Discord's View Channel permission bit.
pub const VIEW_CHANNEL: u64 = 1 << 10;

/// The application flag for the Server Members intent of a bot in fewer than
/// a hundred servers.
const GATEWAY_GUILD_MEMBERS_LIMITED: u64 = 1 << 15;

/// An interaction's token, as the fake mints it: its id follows.
const TOKEN_PREFIX: &str = "fake-interaction-token-";

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
    /// Who wrote it: the bot, or the user who typed it (`say`).
    pub author: String,
    /// Every content it has had, in order: the create's, then each edit's
    /// that changed it (theseus-qifw).
    pub versions: Vec<String>,
    /// Its buttons now, as a person sees them.
    pub buttons: Vec<Button>,
    /// Its components as the binding sent them, for a press's payload.
    #[serde(skip)]
    pub components_json: Value,
}

/// A button on a message.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Button {
    pub label: String,
    pub custom_id: String,
    pub disabled: bool,
}

/// Every button in `components` (action rows of buttons).
fn buttons(components: &Value) -> Vec<Button> {
    components
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|row| row["components"].as_array().cloned().unwrap_or_default())
        .filter(|c| c["type"] == 2)
        .map(|c| Button {
            label: c["label"].as_str().unwrap_or("").to_string(),
            custom_id: c["custom_id"].as_str().unwrap_or("").to_string(),
            disabled: c["disabled"].as_bool().unwrap_or(false),
        })
        .collect()
}

/// What the binding answered an interaction (theseus-6g62).
#[derive(Debug, Clone, Serialize)]
pub struct Reply {
    pub at_ms: u64,
    /// The interaction's id, from the path or the token the fake minted.
    pub interaction: Option<String>,
    /// `callback` (the first answer), `original` (an edit of the first
    /// answer), or `followup`.
    pub kind: String,
    /// A callback's type: 4 a message, 5 a deferred message, 6 a deferred
    /// update of the pressed message, 7 an update of it.
    pub response_type: Option<u64>,
    pub content: Option<String>,
    pub ephemeral: bool,
}

/// A guild, as the binding's viewer check reads it (theseus-ck0k): its
/// owner, its roles (`@everyone`'s id is the guild's), its members, and its
/// channels with their overwrites. Ids are numbers; permissions are bits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Guild {
    pub id: u64,
    pub name: String,
    pub owner: u64,
    /// `@everyone`'s permissions in the guild.
    pub everyone: u64,
    #[serde(default)]
    pub roles: Vec<Role>,
    #[serde(default)]
    pub members: Vec<Member>,
    #[serde(default)]
    pub channels: Vec<Channel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Role {
    pub id: u64,
    pub name: String,
    pub permissions: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Member {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub roles: Vec<u64>,
    #[serde(default)]
    pub bot: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub overwrites: Vec<Overwrite>,
}

/// A channel's permission overwrite for a role (`kind` 0) or a member (1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Overwrite {
    pub id: u64,
    pub kind: u8,
    #[serde(default)]
    pub allow: u64,
    #[serde(default)]
    pub deny: u64,
}

impl Guild {
    /// A guild whose `@everyone` can view its channels, as a new guild's
    /// can, with `owner` and the fake bot as members.
    pub fn new(id: u64, owner: (u64, &str)) -> Self {
        Self {
            id,
            name: "lighthouse-test".into(),
            owner: owner.0,
            everyone: VIEW_CHANNEL | (1 << 11) | (1 << 16),
            roles: vec![],
            members: vec![],
            channels: vec![],
        }
        .member(owner.0, owner.1)
        .member_bot()
    }

    pub fn member(mut self, id: u64, name: &str) -> Self {
        self.members.push(Member {
            id,
            name: name.into(),
            roles: vec![],
            bot: false,
        });
        self
    }

    fn member_bot(mut self) -> Self {
        self.members.push(Member {
            id: BOT_ID,
            name: "Theseus (fake)".into(),
            roles: vec![],
            bot: true,
        });
        self
    }

    /// A channel every member can view.
    pub fn channel(mut self, id: u64, name: &str) -> Self {
        self.channels.push(Channel {
            id,
            name: name.into(),
            overwrites: vec![],
        });
        self
    }

    /// A channel only `viewers` (and the bot) can view: `@everyone` is denied
    /// View Channel there, and each viewer allowed it.
    pub fn private_channel(mut self, id: u64, name: &str, viewers: &[u64]) -> Self {
        let mut overwrites = vec![Overwrite {
            id: self.id,
            kind: 0,
            allow: 0,
            deny: VIEW_CHANNEL,
        }];
        for &v in viewers.iter().chain([&BOT_ID]) {
            overwrites.push(Overwrite {
                id: v,
                kind: 1,
                allow: VIEW_CHANNEL,
                deny: 0,
            });
        }
        self.channels.push(Channel {
            id,
            name: name.into(),
            overwrites,
        });
        self
    }
}

/// A message a user types (`say`): in a guild channel, or with `channel`
/// None, in their DM with the bot.
pub struct Typed<'a> {
    pub user: u64,
    pub name: &'a str,
    pub channel: Option<u64>,
    pub content: &'a str,
    /// A file attached to it (theseus-c9l6): the fake reads it and serves
    /// its bytes at the attachment's URL, as Discord's CDN does.
    pub file: Option<&'a std::path::Path>,
}

/// A press of a button on a posted message, found by its label or its
/// custom id, by `user`.
pub struct Pressed<'a> {
    pub message: &'a str,
    pub button: &'a str,
    pub user: u64,
    pub name: &'a str,
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
    /// The guild the viewer check reads, when one is set.
    guild: Option<Guild>,
    /// Each DM channel the bot opened, and its user.
    dms: BTreeMap<u64, u64>,
    /// What the binding answered each interaction, in order.
    replies: Vec<Reply>,
    /// Each interaction `press` sent, by id, and its channel.
    pressed: BTreeMap<String, u64>,
    /// The guild's member list answers 403 (M4 19c): who can view a channel
    /// cannot be read.
    refuse_members: bool,
    /// The file the guild is read from again at every request (M4 19c), so a
    /// live check can change who can view a channel while a turn runs.
    guild_file: Option<PathBuf>,
    /// Each typed message's attached file, by its URL's path
    /// (`attachments/<id>/<name>`): its type and bytes (theseus-c9l6).
    files: BTreeMap<String, (String, Vec<u8>)>,
}

pub struct FakeDiscord {
    /// `127.0.0.1:<port>`, for `[discord] rest_proxy`.
    pub addr: String,
    state: Arc<Mutex<State>>,
    /// A file whose first line is the mode, read at every request (the CLI).
    control: Option<PathBuf>,
    /// Where each request and the messages are written (the CLI).
    log: Option<PathBuf>,
    /// The gateway, once `serve_gateway` starts it.
    gateway: OnceLock<Arc<FakeGateway>>,
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
            gateway: OnceLock::new(),
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

    /// Read the guild from `path` now, and again at every request (M4 19c).
    pub fn watch_guild_file(&self, path: &Path) -> anyhow::Result<()> {
        let text = std::fs::read_to_string(path)?;
        self.set_guild(serde_json::from_str(&text)?);
        self.state.lock().unwrap().guild_file = Some(path.to_path_buf());
        Ok(())
    }

    /// The guild as its file says now; a file that does not read keeps the
    /// guild as it was.
    fn reread_guild(&self) {
        let Some(path) = self.state.lock().unwrap().guild_file.clone() else {
            return;
        };
        if let Some(g) = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str::<Guild>(&t).ok())
        {
            self.set_guild(g);
        }
    }

    /// Refuse the guild's member list, as Discord does a bot without access
    /// (M4 19c): who can view a channel then cannot be read.
    pub fn refuse_members(&self, refuse: bool) {
        self.state.lock().unwrap().refuse_members = refuse;
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

    /// REST and a gateway on ephemeral ports (theseus-6g62).
    pub fn start_with_gateway() -> Arc<Self> {
        let fake = Self::start();
        fake.serve_gateway("127.0.0.1:0").expect("a local port");
        fake
    }

    /// Also serve a gateway on `addr`; its `ws://` URL is what `[discord]
    /// gateway_proxy` takes. Its READY names the guild the fake holds then.
    pub fn serve_gateway(self: &Arc<Self>, addr: &str) -> std::io::Result<String> {
        let me = Arc::downgrade(self);
        let ready = Arc::new(move || me.upgrade().map_or(Value::Null, |f| f.ready()));
        let gw = FakeGateway::start_on(addr, ready)?;
        let url = gw.url();
        let _ = self.gateway.set(gw);
        Ok(url)
    }

    pub fn gateway(&self) -> Option<&Arc<FakeGateway>> {
        self.gateway.get()
    }

    /// The gateway's counters, or none without a gateway.
    pub fn gateway_state(&self) -> Option<GatewayState> {
        self.gateway.get().map(|g| g.state())
    }

    /// The guild the viewer check reads; the application then reports the
    /// Server Members intent (theseus-ck0k).
    pub fn set_guild(&self, guild: Guild) {
        self.state.lock().unwrap().guild = Some(guild);
    }

    /// What the binding answered each interaction, in order.
    pub fn replies(&self) -> Vec<Reply> {
        self.state.lock().unwrap().replies.clone()
    }

    fn guild_id(&self) -> u64 {
        self.state
            .lock()
            .unwrap()
            .guild
            .as_ref()
            .map_or(DEFAULT_GUILD, |g| g.id)
    }

    /// READY's data: the bot, its guild, and where to resume.
    fn ready(&self) -> Value {
        let resume = self.gateway.get().map(|g| g.url()).unwrap_or_default();
        let mut me = fake_gateway::user_json(BOT_ID, "Theseus (fake)", true);
        me["mfa_enabled"] = json!(false);
        me["verified"] = json!(true);
        me["flags"] = json!(0);
        json!({"v": 10, "user": me,
            "guilds": [{"id": self.guild_id().to_string(), "unavailable": true}],
            "session_id": "fake-session", "resume_gateway_url": resume, "shard": [0, 1],
            "application": {"id": APP_ID.to_string(), "flags": self.app_flags()}})
    }

    fn app_flags(&self) -> u64 {
        if self.state.lock().unwrap().guild.is_some() {
            GATEWAY_GUILD_MEMBERS_LIMITED
        } else {
            0
        }
    }

    /// A message `t.user` types, sent through the gateway as Discord sends
    /// it, and kept with the channel's messages. Its id, or why it could
    /// not be sent.
    pub fn say(&self, t: &Typed<'_>) -> Result<String, String> {
        let gw = self.gateway.get().ok_or("the fake serves no gateway")?;
        let file = match t.file {
            Some(p) => Some((
                p.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "file".into()),
                std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?,
            )),
            None => None,
        };
        let (at, id) = {
            let mut st = self.state.lock().unwrap();
            let at = match t.channel {
                Some(c) => Where {
                    channel: c,
                    guild: Some(st.guild.as_ref().map_or(DEFAULT_GUILD, |g| g.id)),
                },
                None => {
                    let c = t.user + 1;
                    st.dms.insert(c, t.user);
                    Where {
                        channel: c,
                        guild: None,
                    }
                }
            };
            st.next_id += 1;
            let id = st.next_id;
            st.messages.push(Msg {
                id: id.to_string(),
                channel: at.channel.to_string(),
                content: t.content.to_string(),
                nonce: None,
                components: 0,
                reply_to: None,
                edits: 0,
                created_ms: now_ms(),
                allowed_mentions: Value::Null,
                mentions: notified(t.content, &Value::Null),
                author: t.user.to_string(),
                versions: vec![t.content.to_string()],
                buttons: vec![],
                components_json: json!([]),
            });
            (at, id)
        };
        let mentioned: Vec<(u64, bool)> = notified(t.content, &Value::Null)
            .iter()
            .filter_map(|u| u.parse().ok())
            .map(|u| (u, u == BOT_ID))
            .collect();
        let mut d = fake_gateway::message_create(id, at, (t.user, t.name), t.content, &mentioned);
        if let Some((name, bytes)) = file {
            let kind = if name.to_ascii_lowercase().ends_with(".pdf") {
                "application/pdf"
            } else {
                "application/octet-stream"
            };
            let path = format!("attachments/{id}/{name}");
            let url = format!("http://{}/{path}", self.addr);
            d["attachments"] = json!([{"id": (id + 1_000_000).to_string(), "filename": name,
                "size": bytes.len(), "url": url, "proxy_url": url, "content_type": kind}]);
            self.state
                .lock()
                .unwrap()
                .files
                .insert(path, (kind.to_string(), bytes));
        }
        if !gw.dispatch("MESSAGE_CREATE", d) {
            return Err("no client is connected to the gateway".into());
        }
        Ok(id.to_string())
    }

    /// A press of a button on a message the bot posted, by `p.user`, sent
    /// through the gateway as Discord sends it: the interaction carries the
    /// message, buttons and all. The interaction's id, or why it could not
    /// be sent (no such message, or no such button on it).
    pub fn press(&self, p: &Pressed<'_>) -> Result<String, String> {
        let gw = self.gateway.get().ok_or("the fake serves no gateway")?;
        let (msg, at, iid) = {
            let mut st = self.state.lock().unwrap();
            let msg = st
                .messages
                .iter()
                .find(|m| m.id == p.message)
                .cloned()
                .ok_or_else(|| format!("no message {}", p.message))?;
            let channel: u64 = msg.channel.parse().unwrap_or(0);
            let guild = if st.dms.contains_key(&channel) {
                None
            } else {
                Some(st.guild.as_ref().map_or(DEFAULT_GUILD, |g| g.id))
            };
            st.next_id += 1;
            let iid = st.next_id;
            st.pressed.insert(iid.to_string(), channel);
            (msg, Where { channel, guild }, iid)
        };
        let button = msg
            .buttons
            .iter()
            .find(|b| b.label == p.button || b.custom_id == p.button)
            .ok_or_else(|| {
                let labels: Vec<&str> = msg.buttons.iter().map(|b| b.label.as_str()).collect();
                format!(
                    "message {} has no button {:?}; it has {labels:?}",
                    msg.id, p.button
                )
            })?;
        let token = format!("{TOKEN_PREFIX}{iid}");
        let i = fake_gateway::Interaction {
            id: iid,
            app: APP_ID,
            token: &token,
            at,
            user: (p.user, p.name),
        };
        let mut message = message_json(&msg);
        message["components"] = msg.components_json.clone();
        let d = fake_gateway::component_press(&i, message, &button.custom_id);
        if !gw.dispatch("INTERACTION_CREATE", d) {
            return Err("no client is connected to the gateway".into());
        }
        Ok(iid.to_string())
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
        let Some((method, full, body)) = read_request(&stream)? else {
            return Ok(());
        };
        self.reread_guild();
        let path = full.split('?').next().unwrap_or("").to_string();
        let route = path.trim_start_matches("/api/v10");
        let segs: Vec<&str> = route.trim_matches('/').split('/').collect();
        if let ["_fake", rest @ ..] = segs.as_slice() {
            // A check's own request: never slowed, failed, or recorded.
            return self.control(stream, &method, rest, &body);
        }
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
        if let Some(v) = self.bot_route(&method, &segs, &body) {
            self.record(seen(&method, route, "other"));
            return reply(stream, 200, &v);
        }
        match (method.as_str(), segs.as_slice()) {
            ("GET", ["attachments", ..]) => {
                let file = self
                    .state
                    .lock()
                    .unwrap()
                    .files
                    .get(route.trim_matches('/'))
                    .cloned();
                self.record(seen("GET", route, "file"));
                match file {
                    Some((kind, bytes)) => reply_bytes(stream, &kind, &bytes),
                    None => reply(
                        stream,
                        404,
                        &json!({"message": "Unknown attachment", "code": 0}),
                    ),
                }
            }
            ("GET", ["channels", c]) => self.channel_read(stream, route, c),
            ("GET", ["guilds", g, rest @ ..]) => self.guild_read(stream, route, g, rest, &full),
            ("POST" | "PATCH", ["interactions" | "webhooks", ..]) => {
                self.interaction_route(stream, &method, &segs, &body)
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
                        versions: vec![content.clone()],
                        content,
                        nonce: nonce.clone(),
                        components: body["components"].as_array().map_or(0, Vec::len),
                        reply_to: body["message_reference"]["message_id"]
                            .as_str()
                            .map(str::to_string),
                        edits: 0,
                        created_ms: now,
                        allowed_mentions: allowed,
                        author: BOT_ID.to_string(),
                        buttons: buttons(&body["components"]),
                        components_json: body["components"].clone(),
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
                        if c != m.content {
                            m.versions.push(c.to_string());
                        }
                        m.content = c.to_string();
                    }
                    if let Some(c) = body["components"].as_array() {
                        m.components = c.len();
                        m.buttons = buttons(&body["components"]);
                        m.components_json = body["components"].clone();
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

    /// What the bot asks about itself and sets up as it connects: who it is,
    /// its application (with the members intent when a guild is set), its
    /// guilds, its commands, and a DM's channel (the user's id plus one,
    /// stable per user). Answered 200; None for every other route.
    fn bot_route(&self, method: &str, segs: &[&str], body: &Value) -> Option<Value> {
        Some(match (method, segs) {
            ("GET", ["users", "@me"]) => {
                json!({"id": BOT_ID.to_string(), "username": "Theseus (fake)",
                    "discriminator": "0000", "bot": true, "mfa_enabled": false})
            }
            ("GET", ["oauth2", "applications", "@me"] | ["applications", "@me"]) => {
                json!({"id": APP_ID.to_string(), "name": "theseus-fake", "description": "",
                    "bot_public": false, "bot_require_code_grant": false, "verify_key": "",
                    "flags": self.app_flags()})
            }
            ("GET", ["users", "@me", "guilds"]) => {
                let st = self.state.lock().unwrap();
                let guilds: Vec<Value> = st
                    .guild
                    .iter()
                    .map(|g| {
                        json!({"id": g.id.to_string(), "name": g.name, "icon": null,
                            "owner": false, "permissions": "0", "features": []})
                    })
                    .collect();
                json!(guilds)
            }
            ("PUT", ["applications", _, "commands"]) => json!([]),
            ("POST", ["users", "@me", "channels"]) => {
                let user: u64 = body["recipient_id"]
                    .as_str()
                    .and_then(|s| s.parse().ok())
                    .or_else(|| body["recipient_id"].as_u64())
                    .unwrap_or(0);
                self.state.lock().unwrap().dms.insert(user + 1, user);
                json!({"id": (user + 1).to_string(), "type": 1})
            }
            _ => return None,
        })
    }

    /// An answer to an interaction, by its route: the callback, an edit of
    /// the original response, or a follow-up. Anything else there is a 404.
    fn interaction_route(
        &self,
        stream: TcpStream,
        method: &str,
        segs: &[&str],
        body: &Value,
    ) -> std::io::Result<()> {
        let (path, interaction, kind) = match (method, segs) {
            ("POST", ["interactions", id, _, "callback"]) => (
                format!("/interactions/{id}/{{token}}/callback"),
                Some(*id),
                "callback",
            ),
            ("PATCH", ["webhooks", app, token, "messages", "@original"]) => (
                format!("/webhooks/{app}/{{token}}/messages/@original"),
                token.strip_prefix(TOKEN_PREFIX),
                "original",
            ),
            ("POST", ["webhooks", app, token]) => (
                format!("/webhooks/{app}/{{token}}"),
                token.strip_prefix(TOKEN_PREFIX),
                "followup",
            ),
            _ => {
                // The route as asked, with its token (the third segment) cut
                // out, as for every route above.
                let mut cut = segs.to_vec();
                if let Some(token) = cut.get_mut(2) {
                    *token = "{token}";
                }
                self.record(seen(method, &format!("/{}", cut.join("/")), "unknown"));
                return reply(
                    stream,
                    404,
                    &json!({"message": "Unknown (fake)", "code": 10000}),
                );
            }
        };
        self.interaction_reply(stream, method, &path, interaction, kind, body)
    }

    /// A channel, as the viewer check reads it: a guild channel with its
    /// overwrites, or a DM the bot opened.
    fn channel_read(&self, stream: TcpStream, route: &str, id: &str) -> std::io::Result<()> {
        self.record(seen("GET", route, "read"));
        let found = {
            let st = self.state.lock().unwrap();
            let c: u64 = id.parse().unwrap_or(0);
            match (&st.guild, st.dms.contains_key(&c)) {
                (_, true) => Some(json!({"id": id, "type": 1})),
                (Some(g), false) => g
                    .channels
                    .iter()
                    .find(|ch| ch.id == c)
                    .map(|ch| channel_json(g.id, ch)),
                (None, false) => None,
            }
        };
        match found {
            Some(c) => reply(stream, 200, &c),
            None => reply(
                stream,
                404,
                &json!({"message": "Unknown Channel", "code": 10003}),
            ),
        }
    }

    /// The guild, its members (a page after `after`, up to `limit`), or one
    /// member, from the guild set (theseus-ck0k).
    fn guild_read(
        &self,
        stream: TcpStream,
        route: &str,
        id: &str,
        rest: &[&str],
        full: &str,
    ) -> std::io::Result<()> {
        self.record(seen("GET", route, "read"));
        let guild = self.state.lock().unwrap().guild.clone();
        let Some(g) = guild.filter(|g| g.id.to_string() == id) else {
            return reply(
                stream,
                404,
                &json!({"message": "Unknown Guild", "code": 10004}),
            );
        };
        match rest {
            [] => reply(stream, 200, &guild_json(&g)),
            ["members"] if self.state.lock().unwrap().refuse_members => reply(
                stream,
                403,
                &json!({"message": "Missing Access", "code": 50001}),
            ),
            ["members"] => {
                let after: u64 = query(full, "after")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
                let limit: usize = query(full, "limit")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(1);
                let mut members: Vec<&Member> = g.members.iter().filter(|m| m.id > after).collect();
                members.sort_by_key(|m| m.id);
                let page: Vec<Value> = members.iter().take(limit).map(|m| member_json(m)).collect();
                reply(stream, 200, &json!(page))
            }
            ["members", u] => match g.members.iter().find(|m| m.id.to_string() == *u) {
                Some(m) => reply(stream, 200, &member_json(m)),
                None => reply(
                    stream,
                    404,
                    &json!({"message": "Unknown Member", "code": 10007}),
                ),
            },
            _ => reply(
                stream,
                404,
                &json!({"message": "Unknown (fake)", "code": 10000}),
            ),
        }
    }

    /// An answer to an interaction: kept, and answered as Discord does. The
    /// token stays out of the recorded path.
    fn interaction_reply(
        &self,
        stream: TcpStream,
        method: &str,
        path: &str,
        interaction: Option<&str>,
        kind: &str,
        body: &Value,
    ) -> std::io::Result<()> {
        self.record(seen(method, path, kind));
        // A callback's message is in `data`; an edit's and a follow-up's is the body.
        let data = if kind == "callback" {
            &body["data"]
        } else {
            body
        };
        let r = Reply {
            at_ms: now_ms(),
            interaction: interaction.map(str::to_string),
            kind: kind.to_string(),
            response_type: body["type"].as_u64().filter(|_| kind == "callback"),
            content: data["content"].as_str().map(str::to_string),
            ephemeral: data["flags"].as_u64().is_some_and(|f| f & 64 != 0),
        };
        let content = r.content.clone().unwrap_or_default();
        self.state.lock().unwrap().replies.push(r);
        if kind == "callback" {
            return reply_empty(stream);
        }
        let mut st = self.state.lock().unwrap();
        st.next_id += 1;
        // The interaction's channel, when `press` sent it.
        let channel = interaction
            .and_then(|i| st.pressed.get(i).copied())
            .unwrap_or(1);
        let m = json!({"id": st.next_id.to_string(), "channel_id": channel.to_string(), "content": content,
            "author": fake_gateway::user_json(BOT_ID, "Theseus (fake)", true),
            "timestamp": "2026-09-30T00:00:00.000000+00:00", "edited_timestamp": null, "tts": false,
            "mention_everyone": false, "mentions": [], "mention_roles": [], "attachments": [],
            "embeds": [], "pinned": false, "type": 0});
        drop(st);
        reply(stream, 200, &m)
    }

    /// A check's request, under `/_fake/`: send a typed message or a press
    /// through the gateway, set the guild, or read what the fake holds.
    fn control(
        &self,
        stream: TcpStream,
        method: &str,
        rest: &[&str],
        body: &Value,
    ) -> std::io::Result<()> {
        let s = |k: &str| body[k].as_str().unwrap_or("").to_string();
        let n = |k: &str| {
            body[k]
                .as_u64()
                .or_else(|| body[k].as_str().and_then(|v| v.parse().ok()))
        };
        let sent = |r: Result<String, String>| match r {
            Ok(id) => (200, json!({"id": id})),
            Err(e) => (409, json!({"error": e})),
        };
        let (status, out) = match (method, rest) {
            ("POST", ["say"]) => {
                let file = s("file");
                sent(self.say(&Typed {
                    user: n("user").unwrap_or(0),
                    name: &s("name"),
                    channel: n("channel"),
                    content: &s("content"),
                    file: (!file.is_empty()).then(|| std::path::Path::new(&file)),
                }))
            }
            ("POST", ["press"]) => sent(self.press(&Pressed {
                message: &s("message"),
                button: &s("button"),
                user: n("user").unwrap_or(0),
                name: &s("name"),
            })),
            ("POST", ["guild"]) => match serde_json::from_value::<Guild>(body.clone()) {
                Ok(g) => {
                    self.set_guild(g);
                    (200, json!({"ok": true}))
                }
                Err(e) => (400, json!({"error": e.to_string()})),
            },
            ("GET", ["messages"]) => (200, json!(self.all_messages())),
            ("GET", ["replies"]) => (200, json!(self.replies())),
            ("GET", ["gateway"]) => (200, json!(self.gateway_state())),
            _ => (404, json!({"error": "no such control route"})),
        };
        reply(stream, status, &out)
    }
}

/// One request: its method, its target with the query, and its body. Only
/// the body's length is read of the headers; every other header, the
/// token's among them, is dropped unread. None when the client sent nothing.
fn read_request(stream: &TcpStream) -> std::io::Result<Option<(String, String, Value)>> {
    let mut r = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    if r.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let full = parts.next().unwrap_or("").to_string();
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
        if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
            len = v.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0; len];
    r.read_exact(&mut body)?;
    Ok(Some((
        method,
        full,
        serde_json::from_slice(&body).unwrap_or(Value::Null),
    )))
}

/// A query parameter's value.
fn query<'a>(full: &'a str, key: &str) -> Option<&'a str> {
    full.split_once('?')?
        .1
        .split('&')
        .find_map(|kv| kv.strip_prefix(key)?.strip_prefix('='))
}

/// A guild channel as Discord answers it, with its overwrites.
fn channel_json(guild: u64, c: &Channel) -> Value {
    let overwrites: Vec<Value> = c
        .overwrites
        .iter()
        .map(|o| json!({"id": o.id.to_string(), "type": o.kind, "allow": o.allow.to_string(), "deny": o.deny.to_string()}))
        .collect();
    json!({"id": c.id.to_string(), "type": 0, "guild_id": guild.to_string(), "name": c.name,
        "position": 0, "permission_overwrites": overwrites})
}

fn role_json(id: u64, name: &str, permissions: u64) -> Value {
    json!({"id": id.to_string(), "name": name, "color": 0,
        "colors": {"primary_color": 0, "secondary_color": null, "tertiary_color": null},
        "hoist": false, "managed": false, "mentionable": false,
        "permissions": permissions.to_string(), "position": 0, "flags": 0})
}

/// The guild as Discord answers it: what twilight requires, with its owner
/// and its roles, `@everyone` first.
fn guild_json(g: &Guild) -> Value {
    let mut roles = vec![role_json(g.id, "@everyone", g.everyone)];
    roles.extend(
        g.roles
            .iter()
            .map(|r| role_json(r.id, &r.name, r.permissions)),
    );
    json!({"id": g.id.to_string(), "name": g.name, "owner_id": g.owner.to_string(), "roles": roles,
        "afk_timeout": 300, "default_message_notifications": 0, "explicit_content_filter": 0,
        "features": [], "mfa_level": 0, "nsfw_level": 0, "preferred_locale": "en-US",
        "premium_progress_bar_enabled": false, "system_channel_flags": 0, "verification_level": 0,
        "emojis": [], "stickers": []})
}

fn member_json(m: &Member) -> Value {
    let roles: Vec<String> = m.roles.iter().map(u64::to_string).collect();
    json!({"user": fake_gateway::user_json(m.id, &m.name, m.bot), "roles": roles,
        "joined_at": "2026-09-30T00:00:00.000000+00:00", "deaf": false, "mute": false, "flags": 0,
        "nick": null, "communication_disabled_until": null})
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
    let bot = m.author == BOT_ID.to_string();
    let name = if bot {
        "Theseus (fake)".to_string()
    } else {
        format!("user-{}", m.author)
    };
    json!({
        "id": m.id, "channel_id": m.channel, "content": m.content,
        "author": {"id": m.author, "username": name, "discriminator": "0000", "bot": bot},
        "timestamp": "2026-09-30T00:00:00.000000+00:00", "edited_timestamp": null, "tts": false,
        "mention_everyone": false, "mentions": mentions, "mention_roles": [], "attachments": [], "embeds": [],
        "pinned": false, "type": 0, "nonce": m.nonce,
    })
}

/// A file's bytes, as a CDN answers them (theseus-c9l6).
fn reply_bytes(mut stream: TcpStream, kind: &str, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        bytes.len()
    )?;
    stream.write_all(bytes)?;
    stream.flush()
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

    /// One request to the fake, as a client sends it: the status and the
    /// body.
    fn http(addr: &str, method: &str, path: &str, body: &Value) -> (u16, Value) {
        let addr = addr.parse().unwrap();
        let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(5)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let b = if body.is_null() {
            String::new()
        } else {
            body.to_string()
        };
        write!(
            s,
            "{method} {path} HTTP/1.1\r\nhost: fake\r\ncontent-length: {}\r\n\r\n{b}",
            b.len()
        )
        .unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        let (head, body) = out.split_once("\r\n\r\n").unwrap();
        let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
        (status, serde_json::from_str(body).unwrap_or(Value::Null))
    }

    const ANA: u64 = 900_000_000_000_000_101;
    const CY: u64 = 900_000_000_000_000_303;
    const LAB: u64 = 900_000_000_000_000_010;
    const COMMONS: u64 = 900_000_000_000_000_020;

    fn guild() -> Guild {
        Guild::new(DEFAULT_GUILD, (ANA, "ana"))
            .member(CY, "cy")
            .private_channel(LAB, "lab", &[ANA])
            .channel(COMMONS, "commons")
    }

    /// theseus-ck0k: with a guild set, the reads the binding's viewer check
    /// makes answer as Discord does, in the shapes its model reads: the
    /// application's flags with the members intent, a channel and its
    /// overwrites, the guild with its owner and roles, and its members, a
    /// page at a time. Without one, the application has no such intent.
    #[test]
    fn the_viewer_checks_reads_answer_from_the_guild() {
        use twilight_model::oauth::{Application, ApplicationFlags};
        let fake = FakeDiscord::start();
        let get = |p: &str| http(&fake.addr, "GET", &format!("/api/v10{p}"), &Value::Null);
        let app: Application = serde_json::from_value(get("/applications/@me").1).unwrap();
        assert_eq!(app.flags, Some(ApplicationFlags::empty()));
        assert_eq!(get("/guilds/{DEFAULT_GUILD}").0, 404);
        assert_eq!(get("/users/@me/guilds").1, json!([]));
        fake.set_guild(guild());
        let app: Application = serde_json::from_value(get("/applications/@me").1).unwrap();
        assert!(app
            .flags
            .unwrap()
            .contains(ApplicationFlags::GATEWAY_GUILD_MEMBERS_LIMITED));
        let ch: twilight_model::channel::Channel =
            serde_json::from_value(get(&format!("/channels/{LAB}")).1).unwrap();
        assert_eq!(ch.guild_id.map(|g| g.get()), Some(DEFAULT_GUILD));
        let ow = ch.permission_overwrites.unwrap();
        assert_eq!(
            ow.len(),
            3,
            "@everyone denied, ana and the bot allowed: {ow:?}"
        );
        let g: twilight_model::guild::Guild =
            serde_json::from_value(get(&format!("/guilds/{DEFAULT_GUILD}")).1).unwrap();
        assert_eq!(g.owner_id.get(), ANA);
        assert_eq!(g.roles[0].id.get(), DEFAULT_GUILD, "@everyone first");
        let page = |q: &str| -> Vec<u64> {
            let v = get(&format!("/guilds/{DEFAULT_GUILD}/members?{q}")).1;
            let ms: Vec<twilight_model::guild::Member> = serde_json::from_value(v).unwrap();
            ms.iter().map(|m| m.user.id.get()).collect()
        };
        assert_eq!(page("limit=1000"), [ANA, CY, BOT_ID]);
        assert_eq!(page(&format!("limit=1&after={ANA}")), [CY]);
        assert_eq!(
            get(&format!("/guilds/{DEFAULT_GUILD}/members/{BOT_ID}")).0,
            200
        );
        assert_eq!(get("/channels/12345").0, 404);
        assert_eq!(
            get("/users/@me/guilds").1[0]["id"],
            DEFAULT_GUILD.to_string()
        );
    }

    /// theseus-6g62: an interaction's callback is answered 204, and its
    /// original response's edit and its follow-up a message; every answer
    /// is kept, and no token reaches the record.
    #[test]
    fn interaction_answers_are_kept_and_their_tokens_are_not() {
        let fake = FakeDiscord::start();
        let tok = format!("{TOKEN_PREFIX}77");
        let (s, _) = http(
            &fake.addr,
            "POST",
            &format!("/api/v10/interactions/77/{tok}/callback"),
            &json!({"type": 6}),
        );
        assert_eq!(s, 204);
        let (s, m) = http(
            &fake.addr,
            "POST",
            &format!("/api/v10/webhooks/{APP_ID}/{tok}?wait=true"),
            &json!({"content": "only you see this", "flags": 64}),
        );
        assert_eq!(s, 200);
        let _: twilight_model::channel::Message = serde_json::from_value(m).unwrap();
        let (s, _) = http(
            &fake.addr,
            "PATCH",
            &format!("/api/v10/webhooks/{APP_ID}/{tok}/messages/@original"),
            &json!({"content": "done"}),
        );
        assert_eq!(s, 200);
        let r = fake.replies();
        assert!(
            r.iter().all(|a| a.interaction.as_deref() == Some("77")),
            "{r:?}"
        );
        let kinds: Vec<&str> = r.iter().map(|a| a.kind.as_str()).collect();
        assert_eq!(kinds, ["callback", "followup", "original"]);
        assert_eq!(
            (r[0].response_type, r[0].content.as_deref()),
            (Some(6), None)
        );
        assert_eq!(
            (r[1].content.as_deref(), r[1].ephemeral),
            (Some("only you see this"), true)
        );
        assert_eq!(
            (r[2].content.as_deref(), r[2].ephemeral),
            (Some("done"), false)
        );
        // A route under /webhooks the fake doesn't know is a 404 that
        // answers no interaction, and is recorded as asked, token cut out.
        let (s, _) = http(
            &fake.addr,
            "POST",
            &format!("/api/v10/webhooks/{APP_ID}/{tok}/messages/@sideways"),
            &json!({"content": "?"}),
        );
        assert_eq!(s, 404);
        assert_eq!(fake.replies().len(), 3);
        let unknown = fake.seen().into_iter().find(|s| s.outcome == "unknown");
        assert_eq!(
            unknown.map(|s| s.path),
            Some(format!("/webhooks/{APP_ID}/{{token}}/messages/@sideways"))
        );
        assert!(fake.seen().iter().all(|s| !s.path.contains(&tok)));
    }

    /// theseus-qifw: a message keeps each content it had, in order, and its
    /// buttons until an edit takes them away.
    #[test]
    fn a_message_keeps_every_version_and_its_buttons() {
        let fake = FakeDiscord::start();
        let buttons = json!([{"type": 1, "components": [
            {"type": 2, "style": 3, "label": "Approve", "custom_id": "confirm:approve:act_1"},
            {"type": 2, "style": 4, "label": "Decline", "custom_id": "confirm:decline:act_1"}]}]);
        let (_, m) = http(
            &fake.addr,
            "POST",
            &format!("/api/v10/channels/{LAB}/messages"),
            &json!({"content": "**Approve?**", "components": buttons}),
        );
        let id = m["id"].as_str().unwrap().to_string();
        let msg = &fake.messages(LAB)[0];
        let labels: Vec<&str> = msg.buttons.iter().map(|b| b.label.as_str()).collect();
        assert_eq!(labels, ["Approve", "Decline"]);
        assert_eq!(msg.author, BOT_ID.to_string());
        http(
            &fake.addr,
            "PATCH",
            &format!("/api/v10/channels/{LAB}/messages/{id}"),
            &json!({"content": "✅ **Approved**", "components": []}),
        );
        let msg = &fake.messages(LAB)[0];
        assert_eq!(msg.versions, ["**Approve?**", "✅ **Approved**"]);
        assert!(msg.buttons.is_empty() && msg.components == 0);
    }

    fn read_frame(ws: &mut tungstenite::WebSocket<TcpStream>) -> Value {
        loop {
            if let tungstenite::Message::Text(t) = ws.read().unwrap() {
                return serde_json::from_str(t.as_str()).unwrap();
            }
        }
    }

    /// theseus-6g62: READY, a typed message, and a press go through the
    /// gateway in the shapes the binding's model reads. A press names a
    /// button the message has; the message rides along, buttons and all.
    #[test]
    fn say_and_press_send_what_the_binding_reads() {
        use twilight_model::application::interaction::{Interaction, InteractionData};
        let fake = FakeDiscord::start_with_gateway();
        fake.set_guild(guild());
        let gw = fake.gateway().unwrap();
        let stream =
            TcpStream::connect_timeout(&gw.addr.parse().unwrap(), Duration::from_secs(5)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let (mut ws, _) = tungstenite::client::client(gw.url(), stream).unwrap();
        assert_eq!(read_frame(&mut ws)["op"], 10);
        ws.send(tungstenite::Message::text(
            json!({"op": 2, "d": {}}).to_string(),
        ))
        .unwrap();
        let ready = read_frame(&mut ws);
        let r: twilight_model::gateway::payload::incoming::Ready =
            serde_json::from_value(ready["d"].clone()).unwrap();
        assert_eq!(r.guilds[0].id.get(), DEFAULT_GUILD);
        assert_eq!(r.user.id.get(), BOT_ID);
        let typed = format!("<@{BOT_ID}> hello");
        fake.say(&Typed {
            user: ANA,
            name: "ana",
            channel: Some(LAB),
            content: &typed,
            file: None,
        })
        .unwrap();
        let m: twilight_model::channel::Message =
            serde_json::from_value(read_frame(&mut ws)["d"].clone()).unwrap();
        assert_eq!(m.guild_id.map(|g| g.get()), Some(DEFAULT_GUILD));
        assert_eq!(m.author.id.get(), ANA);
        assert_eq!(m.mentions[0].id.get(), BOT_ID);
        let buttons = json!([{"type": 1, "components": [
            {"type": 2, "style": 3, "label": "Approve", "custom_id": "confirm:approve:act_1"}]}]);
        let (_, card) = http(
            &fake.addr,
            "POST",
            &format!("/api/v10/channels/{LAB}/messages"),
            &json!({"content": "**Approve?**", "components": buttons}),
        );
        let card = card["id"].as_str().unwrap().to_string();
        let press = |button: &str| {
            fake.press(&Pressed {
                message: &card,
                button,
                user: ANA,
                name: "ana",
            })
        };
        let err = press("Nope").unwrap_err();
        assert!(err.contains("\"Approve\""), "{err}");
        press("Approve").unwrap();
        let i: Interaction = serde_json::from_value(read_frame(&mut ws)["d"].clone()).unwrap();
        let Some(InteractionData::MessageComponent(c)) = &i.data else {
            panic!("{i:?}")
        };
        assert_eq!(c.custom_id, "confirm:approve:act_1");
        assert_eq!(i.author().map(|u| u.id.get()), Some(ANA));
        assert_eq!(i.message.as_ref().unwrap().components.len(), 1);
        assert!(i.token.starts_with(TOKEN_PREFIX));
        fake.say(&Typed {
            user: ANA,
            name: "ana",
            channel: None,
            content: "in the DM",
            file: None,
        })
        .unwrap();
        let dm: twilight_model::channel::Message =
            serde_json::from_value(read_frame(&mut ws)["d"].clone()).unwrap();
        assert!(dm.guild_id.is_none());
        assert_eq!(dm.channel_id.get(), ANA + 1);
        let typed: Vec<String> = fake
            .all_messages()
            .iter()
            .map(|m| m.author.clone())
            .collect();
        assert_eq!(typed.iter().filter(|a| **a == ANA.to_string()).count(), 2);
    }

    /// The control routes a check in another process uses (`theseus-sim
    /// discord say|press|read`, `fake-discord --guild`): each answers, a
    /// press of a button the message lacks is refused with why, and none of
    /// them is recorded as a request to Discord.
    #[test]
    fn the_control_routes_drive_and_read_the_fake() {
        let fake = FakeDiscord::start_with_gateway();
        let ctl = |method: &str, route: &str, body: Value| {
            http(&fake.addr, method, &format!("/_fake/{route}"), &body)
        };
        let (s, g) = ctl("GET", "gateway", Value::Null);
        assert_eq!((s, g["connected"].clone()), (200, json!(false)));
        let (s, e) = ctl(
            "POST",
            "say",
            json!({"channel": LAB, "user": ANA, "content": "hi"}),
        );
        assert_eq!(s, 409, "no client yet: {e}");
        let (s, _) = ctl("POST", "guild", serde_json::to_value(guild()).unwrap());
        assert_eq!(s, 200);
        let gw = fake.gateway().unwrap();
        let stream =
            TcpStream::connect_timeout(&gw.addr.parse().unwrap(), Duration::from_secs(5)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let (mut ws, _) = tungstenite::client::client(gw.url(), stream).unwrap();
        read_frame(&mut ws);
        ws.send(tungstenite::Message::text(
            json!({"op": 2, "d": {}}).to_string(),
        ))
        .unwrap();
        assert_eq!(
            read_frame(&mut ws)["d"]["guilds"][0]["id"],
            DEFAULT_GUILD.to_string()
        );
        let (s, said) = ctl(
            "POST",
            "say",
            json!({"channel": LAB.to_string(), "user": ANA, "name": "ana", "content": "hi"}),
        );
        assert_eq!(s, 200, "{said}");
        let m = read_frame(&mut ws);
        assert_eq!(
            (m["t"].clone(), m["d"]["content"].clone()),
            (json!("MESSAGE_CREATE"), json!("hi"))
        );
        assert_eq!(m["d"]["id"], said["id"]);
        let (s, e) = ctl(
            "POST",
            "press",
            json!({"message": said["id"], "button": "Approve", "user": ANA, "name": "ana"}),
        );
        assert_eq!(s, 409);
        assert!(e["error"].as_str().unwrap().contains("no button"), "{e}");
        let (_, msgs) = ctl("GET", "messages", Value::Null);
        assert_eq!(msgs[0]["author"], ANA.to_string());
        assert_eq!(ctl("GET", "replies", Value::Null).1, json!([]));
        assert_eq!(ctl("GET", "nothing", Value::Null).0, 404);
        assert!(fake.seen().is_empty(), "{:?}", fake.seen());
    }
}
