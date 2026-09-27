//! The gateway loop, the places (a text channel or a DM, each backed by one
//! conversation session), and the executor that turns renderer operations into
//! Discord messages.
//!
//! One actor per place: it serializes that place's turns, coalesces messages
//! that arrive mid-turn into the next turn (authors kept), and owns the map
//! from render keys to Discord message ids. A router hands each notification
//! from the core to the place whose session it names.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use theseus_core::config::DiscordConfig;
use theseus_core::Core;
use theseus_protocol::{
    BindingStatus, Notification, PlaceStatus, SessionInfo, SessionKind, SessionListResult,
    SessionOpenParams, SessionRef, TurnSubmitParams, TurnSubmitResult,
};
use tokio::sync::{mpsc, oneshot};
use twilight_gateway::{Event, EventTypeFlags, Intents, Shard, ShardId, StreamExt as _};
use twilight_http::Client as Http;
use twilight_model::application::command::CommandType;
use twilight_model::application::interaction::{Interaction, InteractionData};
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle, Component};
use twilight_model::channel::message::{AllowedMentions, MessageFlags};
use twilight_model::http::interaction::{
    InteractionResponse, InteractionResponseData, InteractionResponseType,
};
use twilight_model::id::marker::{ApplicationMarker, ChannelMarker, MessageMarker};
use twilight_model::id::Id;
use twilight_util::builder::command::CommandBuilder;

use crate::bindings::{snowflake, Bindings};
use crate::render::{Buttons, Op, Renderer};
use crate::rpc_client::{CallError, RpcClient};

/// The connection label every Discord call carries; authors refine it per message.
const CLIENT: &str = "discord";
const META_PREFIX: &str = "discord.session.";

/// Health's view of the binding, pushed to the core on every change.
#[derive(Clone)]
struct Board {
    core: Arc<Core>,
    st: Arc<Mutex<BindingStatus>>,
}

impl Board {
    fn new(core: Arc<Core>) -> Self {
        let st = BindingStatus {
            kind: "discord".into(),
            state: "starting".into(),
            ..Default::default()
        };
        core.set_binding_status(st.clone());
        Self {
            core,
            st: Arc::new(Mutex::new(st)),
        }
    }

    fn update(&self, f: impl FnOnce(&mut BindingStatus)) {
        let snapshot = {
            let mut g = self.st.lock().unwrap();
            f(&mut g);
            g.clone()
        };
        self.core.set_binding_status(snapshot);
    }

    fn state(&self, state: &str, detail: Option<String>) {
        if let Some(d) = &detail {
            tracing::info!(state, detail = %d, "discord");
        } else {
            tracing::info!(state, "discord");
        }
        self.update(|s| {
            s.state = state.into();
            s.detail = detail;
        });
    }

    fn error(&self, op: &str, session: Option<&str>, e: impl std::fmt::Display) {
        let msg = format!("{op}: {e}");
        tracing::warn!(error = %msg, "discord");
        self.core.binding_ledger(
            "discord.error",
            session,
            json!({"op": op, "error": e.to_string()}),
        );
        self.update(|s| {
            s.errors += 1;
            s.last_error = Some(msg);
        });
    }

    fn place(&self, p: PlaceStatus) {
        self.update(|s| match s.places.iter_mut().find(|x| x.label == p.label) {
            Some(x) => *x = p,
            None => s.places.push(p),
        });
    }
}

/// Run the binding until the process ends. Never fails the daemon: whatever
/// goes wrong is a state in health and a `discord.error` ledger row.
pub async fn run(core: Arc<Core>, cfg: DiscordConfig, path: PathBuf, token: Option<String>) {
    let board = Board::new(core.clone());
    if !cfg.enabled {
        board.state("disabled", Some("[discord].enabled = false".into()));
        return;
    }
    board.update(|s| s.bindings_file = Some(path.display().to_string()));
    let Some(token) = token else {
        board.state(
            "unconfigured",
            Some(format!(
                "no [secrets] entry named {:?} for the bot token",
                cfg.token_secret
            )),
        );
        return;
    };
    if !path.exists() {
        board.state(
            "unconfigured",
            Some(format!(
                "no bindings file at {} (`theseusd example-bindings` prints one)",
                path.display()
            )),
        );
        return;
    }
    let bindings = match Bindings::load(&path) {
        Ok(b) => b,
        Err(e) => {
            board.state("failed", Some(format!("{e:#}")));
            return;
        }
    };
    board.update(|s| {
        s.guild_id = Some(bindings.guild_id.clone());
        s.revision = Some(bindings.revision.clone());
    });
    if let Err(e) = serve(core, cfg, token, bindings, board.clone()).await {
        board.state("failed", Some(format!("{e:#}")));
    }
}

async fn serve(
    core: Arc<Core>,
    cfg: DiscordConfig,
    token: String,
    bindings: Bindings,
    board: Board,
) -> anyhow::Result<()> {
    // Both rustls providers are compiled into this workspace; pick one for the process.
    let _ = rustls::crypto::ring::default_provider().install_default();
    board.state("connecting", None);
    let http = Arc::new(Http::new(token.clone()));

    let me = http.current_user().await?.model().await?;
    board.update(|s| s.bot_user = Some(format!("{} ({})", me.name, me.id)));
    let app_id = http.current_user_application().await?.model().await?.id;
    let guild = Id::new(snowflake("guild_id", &bindings.guild_id)?);
    let in_guild = http
        .current_user_guilds()
        .await?
        .models()
        .await?
        .iter()
        .any(|g| g.id == guild);
    if !in_guild {
        board.update(|s| {
            s.detail = Some(format!(
                "the bot is not in guild {guild} yet; invite it: https://discord.com/oauth2/authorize?client_id={app_id}&scope=bot+applications.commands&permissions=117824"
            ))
        });
    }
    register_commands(&http, app_id, &board).await;

    let (rpc, notes) = RpcClient::connect(core.clone(), CLIENT);
    let shared = Arc::new(Shared {
        core: core.clone(),
        rpc: rpc.clone(),
        http: http.clone(),
        board: board.clone(),
        app_id,
        bot_id: me.id.get(),
        edit_interval: Duration::from_millis(cfg.edit_interval_ms.max(250)),
        routes: Mutex::new(Routes::default()),
    });

    shared.refresh_bot_roles(guild).await;
    // Places: every [[channel]] and every [[dm]], each with its session.
    for c in &bindings.channel {
        let channel = Id::new(snowflake("channel id", &c.id)?);
        let users = c
            .users
            .iter()
            .map(|u| snowflake("user id", u))
            .collect::<anyhow::Result<Vec<_>>>()?;
        shared
            .clone()
            .start_place(
                format!("channel:{}", c.id),
                "channel",
                c.label(),
                Some(channel),
                users,
                c.mention_only,
            )
            .await?;
    }
    for d in &bindings.dm {
        let user = snowflake("dm user", &d.user)?;
        let channel = match http.create_private_channel(Id::new(user)).await {
            Ok(r) => r.model().await.ok().map(|c| c.id),
            Err(e) => {
                board.error("open DM channel", None, &e);
                None
            }
        };
        shared
            .clone()
            .start_place(
                format!("dm:{}", d.user),
                "dm",
                d.label(),
                channel,
                vec![user],
                false,
            )
            .await?;
    }
    tokio::spawn(route(shared.clone(), notes));

    let intents = Intents::GUILDS
        | Intents::GUILD_MESSAGES
        | Intents::DIRECT_MESSAGES
        | Intents::MESSAGE_CONTENT;
    let mut shard = Shard::new(ShardId::ONE, token, intents);
    let mut last_latency = std::time::Instant::now();
    while let Some(item) = shard.next_event(EventTypeFlags::all()).await {
        if last_latency.elapsed() > Duration::from_secs(15) {
            last_latency = std::time::Instant::now();
            let ms = shard.latency().average().map(|d| d.as_millis() as u64);
            board.update(|s| s.latency_ms = ms);
        }
        let event = match item {
            Ok(e) => e,
            Err(e) => {
                board.error("gateway receive", None, &e);
                continue;
            }
        };
        match event {
            Event::Ready(r) => {
                board.update(|s| {
                    s.state = "ready".into();
                    s.connected_at_ms = theseus_protocol::now_unix_ms();
                    if r.guilds.iter().any(|g| g.id == guild) {
                        s.detail = None;
                    }
                });
                core.binding_ledger(
                    "discord.ready",
                    None,
                    json!({"bot": me.name, "guilds": r.guilds.len(), "revision": bindings.revision}),
                );
                tracing::info!(bot = %me.name, "discord gateway ready");
            }
            Event::Resumed => board.state("ready", None),
            Event::GuildCreate(g) if g.id() == guild => {
                board.update(|s| s.detail = None);
                shared.refresh_bot_roles(guild).await;
            }
            Event::GatewayClose(frame) => {
                let why = frame
                    .map(|f| format!("close {} {}", f.code, f.reason))
                    .unwrap_or_else(|| "closed".into());
                core.binding_ledger("discord.disconnected", None, json!({"why": why}));
                board.state("resuming", Some(why));
            }
            Event::MessageCreate(m) => shared.clone().on_message(&m.0).await,
            Event::InteractionCreate(i) => {
                let s = shared.clone();
                let i = i.0;
                tokio::spawn(async move { s.on_interaction(i).await });
            }
            _ => {}
        }
    }
    board.state("disconnected", Some("gateway stream ended".into()));
    Ok(())
}

async fn register_commands(http: &Http, app: Id<ApplicationMarker>, board: &Board) {
    let cmds = [
        (
            "stop",
            "Stop what Theseus is doing here (cancels this session's work) and start fresh",
        ),
        (
            "cancel",
            "Same as /stop until tasks arrive: cancel this session's work",
        ),
        (
            "new",
            "Start a new session here; the old one stays in the web UI",
        ),
        (
            "status",
            "This place's session: state, turns, dollars, waiting confirms",
        ),
    ]
    .map(|(n, d)| CommandBuilder::new(n, d, CommandType::ChatInput).build());
    match http.interaction(app).set_global_commands(&cmds).await {
        Ok(_) => tracing::info!(commands = cmds.len(), "discord slash commands registered"),
        Err(e) => board.error("register slash commands", None, e),
    }
}

/// What every place and the gateway share.
struct Shared {
    core: Arc<Core>,
    rpc: Arc<RpcClient>,
    http: Arc<Http>,
    board: Board,
    app_id: Id<ApplicationMarker>,
    bot_id: u64,
    edit_interval: Duration,
    routes: Mutex<Routes>,
}

#[derive(Default)]
struct Routes {
    /// session id → place mailbox
    by_session: HashMap<String, mpsc::UnboundedSender<PlaceMsg>>,
    /// Discord channel id → place mailbox
    by_channel: HashMap<u64, mpsc::UnboundedSender<PlaceMsg>>,
    /// DM user id → place mailbox (the DM channel may open later)
    by_dm_user: HashMap<u64, mpsc::UnboundedSender<PlaceMsg>>,
    /// channel id → who may drive it
    users: HashMap<u64, Vec<u64>>,
    /// channel ids where only an @mention or a reply to the bot starts a turn
    mention_only: std::collections::HashSet<u64>,
    /// the bot's roles in the guild (an @Theseus can arrive as its managed role)
    bot_roles: Vec<u64>,
    /// turn id → session id (deltas name only the turn)
    turns: HashMap<String, String>,
}

enum PlaceMsg {
    Inbound {
        author: String,
        text: String,
        message: Id<MessageMarker>,
    },
    Event(Notification),
    SubmitDone(Result<(), CallError>),
    Control {
        cmd: Control,
        by: String,
        reply: oneshot::Sender<String>,
    },
    DmChannel(Id<ChannelMarker>),
    /// Something to say that is not part of a turn.
    Notice(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    Stop,
    New,
    Status,
}

fn parse_control(text: &str) -> Option<Control> {
    match text.split_whitespace().next()? {
        "/stop" | "/cancel" => Some(Control::Stop),
        "/new" => Some(Control::New),
        "/status" => Some(Control::Status),
        _ => None,
    }
}

impl Shared {
    /// Resolve (or open) the place's session, watch it, and start its actor.
    async fn start_place(
        self: Arc<Self>,
        key: String,
        kind: &'static str,
        label: String,
        channel: Option<Id<ChannelMarker>>,
        users: Vec<u64>,
        mention_only: bool,
    ) -> anyhow::Result<()> {
        let (session_id, fresh) = self.session_for(&key, &label).await?;
        let (tx, rx) = mpsc::unbounded_channel();
        {
            let mut r = self.routes.lock().unwrap();
            r.by_session.insert(session_id.clone(), tx.clone());
            if let Some(c) = channel {
                r.by_channel.insert(c.get(), tx.clone());
                r.users.insert(c.get(), users.clone());
                if mention_only {
                    r.mention_only.insert(c.get());
                }
            }
            if kind == "dm" {
                r.by_dm_user.insert(users[0], tx.clone());
            }
        }
        let notice_tx = tx.clone();
        let place = Place {
            shared: self.clone(),
            key,
            kind,
            label,
            channel,
            users,
            mention_only,
            session_id,
            renderer: Renderer::default(),
            msgs: HashMap::new(),
            inflight: false,
            queued: Vec::new(),
            anchor: None,
            saw_failure: false,
            last_activity_ms: 0,
            tx,
        };
        place.report();
        if fresh {
            let how = if mention_only {
                "@mention me or reply to one of my messages to talk"
            } else {
                "talk to me in this place"
            };
            let _ = notice_tx.send(PlaceMsg::Notice(format!(
                "🔗 Theseus is bound here (session `{}`). {how}; `/status`, `/new` and `/stop` work too, and everything shows in the web UI.",
                place.session_id
            )));
        }
        tokio::spawn(place.run(rx));
        Ok(())
    }

    /// The session stored for this place, if it still exists and can take
    /// turns; otherwise a new conversation session, stored for next time.
    async fn session_for(&self, key: &str, label: &str) -> anyhow::Result<(String, bool)> {
        let meta = format!("{META_PREFIX}{key}");
        if let Some(sid) = self.core.store.get_meta::<String>(&meta)? {
            if let Some(info) = self.session_info(&sid).await {
                if !terminal(info.execution_state.as_deref()) {
                    self.watch(&sid).await;
                    return Ok((sid, false));
                }
            }
        }
        Ok((self.open_session(key, label).await?, true))
    }

    async fn open_session(&self, key: &str, label: &str) -> anyhow::Result<String> {
        let info: SessionInfo = self
            .rpc
            .call(
                theseus_protocol::method::SESSION_OPEN,
                SessionOpenParams {
                    kind: Some(SessionKind::Conversation),
                    label: Some(format!("discord {label}")),
                },
            )
            .await?;
        self.core
            .store
            .put_meta(&format!("{META_PREFIX}{key}"), &info.session_id)?;
        self.core.binding_ledger(
            "discord.bound",
            Some(&info.session_id),
            json!({"place": key, "label": label}),
        );
        self.watch(&info.session_id).await;
        Ok(info.session_id)
    }

    async fn watch(&self, sid: &str) {
        if let Err(e) = self
            .rpc
            .call::<_, Value>(
                theseus_protocol::method::SESSION_WATCH,
                SessionRef {
                    session_id: sid.to_string(),
                },
            )
            .await
        {
            self.board.error("session.watch", Some(sid), e);
        }
    }

    async fn session_info(&self, sid: &str) -> Option<SessionInfo> {
        let list: SessionListResult = self
            .rpc
            .call(theseus_protocol::method::SESSION_LIST, json!({}))
            .await
            .ok()?;
        list.sessions.into_iter().find(|s| s.session_id == sid)
    }

    /// The bot's roles in the guild, so an @Theseus that resolves to its
    /// managed role still counts as a mention.
    async fn refresh_bot_roles(&self, guild: Id<twilight_model::id::marker::GuildMarker>) {
        let roles = match self.http.guild_member(guild, Id::new(self.bot_id)).await {
            Ok(r) => match r.model().await {
                Ok(m) => m.roles.iter().map(|r| r.get()).collect(),
                Err(_) => vec![],
            },
            Err(_) => vec![], // not in the guild yet
        };
        self.routes.lock().unwrap().bot_roles = roles;
    }

    async fn on_message(self: Arc<Self>, m: &twilight_model::channel::Message) {
        if m.author.bot || m.author.id.get() == self.bot_id {
            return;
        }
        let (tx, allowed, mention_only, bot_roles) = {
            let r = self.routes.lock().unwrap();
            let mention_only = r.mention_only.contains(&m.channel_id.get());
            let roles = r.bot_roles.clone();
            let (tx, allowed) = match m.guild_id {
                Some(_) => (
                    r.by_channel.get(&m.channel_id.get()).cloned(),
                    r.users
                        .get(&m.channel_id.get())
                        .is_some_and(|u| u.contains(&m.author.id.get())),
                ),
                None => {
                    let tx = r.by_dm_user.get(&m.author.id.get()).cloned();
                    (tx.clone(), tx.is_some())
                }
            };
            (tx, allowed, mention_only, roles)
        };
        let Some(tx) = tx else {
            return; // not a place of ours
        };
        if mention_only {
            let mentions: Vec<u64> = m.mentions.iter().map(|u| u.id.get()).collect();
            let roles: Vec<u64> = m.mention_roles.iter().map(|r| r.get()).collect();
            let replied = m.referenced_message.as_ref().map(|r| r.author.id.get());
            if !addressed(
                &m.content,
                &mentions,
                &roles,
                replied,
                self.bot_id,
                &bot_roles,
            ) {
                return; // talk in a shared channel that is not for Theseus
            }
        }
        if !allowed {
            self.board.update(|s| s.ignored += 1);
            self.core.binding_ledger(
                "discord.ignored",
                None,
                json!({"channel": m.channel_id.to_string(), "author": m.author.name, "author_id": m.author.id.to_string(), "reason": "not in this place's users"}),
            );
            return;
        }
        if m.guild_id.is_none() {
            let _ = tx.send(PlaceMsg::DmChannel(m.channel_id));
        }
        let mut text = if mention_only {
            strip_mentions(&m.content, self.bot_id, &bot_roles)
        } else {
            m.content.clone()
        };
        for a in &m.attachments {
            text.push_str(&format!(
                "\n[attachment: {} ({} bytes), not read]",
                a.filename, a.size
            ));
        }
        if text.trim().is_empty() {
            return;
        }
        self.board.update(|s| s.messages_in += 1);
        let _ = tx.send(PlaceMsg::Inbound {
            author: m.author.name.clone(),
            text,
            message: m.id,
        });
    }

    async fn on_interaction(self: Arc<Self>, i: Interaction) {
        self.board.update(|s| s.interactions += 1);
        let user = i.author().map(|u| (u.id.get(), u.name.clone()));
        let channel = i.channel.as_ref().map(|c| c.id.get());
        let (tx, allowed) = {
            let r = self.routes.lock().unwrap();
            let tx = channel
                .and_then(|c| r.by_channel.get(&c).cloned())
                .or_else(|| {
                    user.as_ref()
                        .and_then(|(u, _)| r.by_dm_user.get(u).cloned())
                });
            let allowed = match (channel, &user) {
                (Some(c), Some((u, _))) => {
                    r.users.get(&c).is_some_and(|v| v.contains(u)) || r.by_dm_user.contains_key(u)
                }
                _ => false,
            };
            (tx, allowed)
        };
        let who = user
            .as_ref()
            .map(|(_, n)| format!("discord:{n}"))
            .unwrap_or_else(|| "discord".into());
        let (Some(tx), true) = (tx, allowed) else {
            self.respond(
                &i,
                InteractionResponseType::ChannelMessageWithSource,
                Some("Only the people this place is bound to can do that.".into()),
                true,
            )
            .await;
            return;
        };
        match &i.data {
            Some(InteractionData::MessageComponent(c)) => {
                let Some((approve, corr)) = parse_confirm_id(&c.custom_id) else {
                    return;
                };
                // Acknowledge now (Discord allows three seconds), answer the kernel, then settle the message.
                self.respond(
                    &i,
                    InteractionResponseType::DeferredUpdateMessage,
                    None,
                    false,
                )
                .await;
                let r = self
                    .rpc
                    .call::<_, Value>(
                        theseus_protocol::method::ACTION_CONFIRM,
                        theseus_protocol::ActionConfirmParams {
                            correlation_id: corr.clone(),
                            approve,
                            note: None,
                            watch: false,
                            author: Some(who.clone()),
                        },
                    )
                    .await;
                let line = i
                    .message
                    .as_ref()
                    .and_then(|m| m.content.lines().next())
                    .map(|l| l.trim_start_matches("**Approve?** ").to_string())
                    .unwrap_or_default();
                let content = match &r {
                    Ok(_) if approve => format!("✅ **Approved** by {who} · {line}"),
                    Ok(_) => format!("❎ **Declined** by {who} · {line}"),
                    Err(e) => format!("⚠️ Could not answer: {e} · {line}"),
                };
                self.core.binding_ledger(
                    "discord.confirm",
                    None,
                    json!({"correlation_id": corr, "approve": approve, "by": who, "ok": r.is_ok(), "error": r.as_ref().err().map(|e| e.message.clone())}),
                );
                if let (Some(ch), Some(m)) = (i.channel.as_ref(), i.message.as_ref()) {
                    let res = self
                        .http
                        .update_message(ch.id, m.id)
                        .content(Some(&content))
                        .components(Some(&[]))
                        .allowed_mentions(Some(&AllowedMentions::default()))
                        .await;
                    if let Err(e) = res {
                        self.board.error("settle confirm message", None, e);
                    }
                }
            }
            Some(InteractionData::ApplicationCommand(c)) => {
                let cmd = match c.name.as_str() {
                    "stop" | "cancel" => Control::Stop,
                    "new" => Control::New,
                    _ => Control::Status,
                };
                self.respond(
                    &i,
                    InteractionResponseType::DeferredChannelMessageWithSource,
                    None,
                    false,
                )
                .await;
                self.core.binding_ledger(
                    "discord.command",
                    None,
                    json!({"command": c.name, "by": who}),
                );
                let (rtx, rrx) = oneshot::channel();
                let _ = tx.send(PlaceMsg::Control {
                    cmd,
                    by: who,
                    reply: rtx,
                });
                let text = rrx
                    .await
                    .unwrap_or_else(|_| "The place did not answer.".into());
                if let Err(e) = self
                    .http
                    .interaction(self.app_id)
                    .update_response(&i.token)
                    .content(Some(&text))
                    .await
                {
                    self.board.error("command reply", None, e);
                }
            }
            _ => {}
        }
    }

    async fn respond(
        &self,
        i: &Interaction,
        kind: InteractionResponseType,
        content: Option<String>,
        ephemeral: bool,
    ) {
        let data = content.map(|c| InteractionResponseData {
            content: Some(c),
            flags: ephemeral.then_some(MessageFlags::EPHEMERAL),
            allowed_mentions: Some(AllowedMentions::default()),
            ..Default::default()
        });
        let resp = InteractionResponse { kind, data };
        if let Err(e) = self
            .http
            .interaction(self.app_id)
            .create_response(i.id, &i.token, &resp)
            .await
        {
            self.board.error("interaction response", None, e);
        }
    }
}

/// A message is for Theseus when it @mentions the bot (as a user or through
/// one of its roles) or replies to one of the bot's messages.
fn addressed(
    content: &str,
    mentions: &[u64],
    mention_roles: &[u64],
    replied_to: Option<u64>,
    bot_id: u64,
    bot_roles: &[u64],
) -> bool {
    mentions.contains(&bot_id)
        || content.contains(&format!("<@{bot_id}>"))
        || content.contains(&format!("<@!{bot_id}>"))
        || mention_roles.iter().any(|r| bot_roles.contains(r))
        || replied_to == Some(bot_id)
}

/// The message without the tokens that addressed the bot.
fn strip_mentions(content: &str, bot_id: u64, bot_roles: &[u64]) -> String {
    let mut s = content
        .replace(&format!("<@{bot_id}>"), "")
        .replace(&format!("<@!{bot_id}>"), "");
    for r in bot_roles {
        s = s.replace(&format!("<@&{r}>"), "");
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn terminal(state: Option<&str>) -> bool {
    matches!(
        state,
        Some("cancelled" | "budget_exhausted" | "failed" | "complete")
    )
}

/// `confirm:approve:<corr>` / `confirm:decline:<corr>`.
fn parse_confirm_id(id: &str) -> Option<(bool, String)> {
    let rest = id.strip_prefix("confirm:")?;
    let (verb, corr) = rest.split_once(':')?;
    match verb {
        "approve" => Some((true, corr.to_string())),
        "decline" => Some((false, corr.to_string())),
        _ => None,
    }
}

fn confirm_buttons(corr: &str) -> Vec<Component> {
    let button = |verb: &str, label: &str, style| {
        Component::Button(Button {
            id: None,
            custom_id: Some(format!("confirm:{verb}:{corr}")),
            disabled: false,
            emoji: None,
            label: Some(label.to_string()),
            style,
            url: None,
            sku_id: None,
        })
    };
    vec![Component::ActionRow(ActionRow {
        id: None,
        components: vec![
            button("approve", "Approve", ButtonStyle::Success),
            button("decline", "Decline", ButtonStyle::Danger),
        ],
    })]
}

/// Hand each notification to the place whose session it names.
async fn route(shared: Arc<Shared>, mut notes: mpsc::UnboundedReceiver<Notification>) {
    while let Some(n) = notes.recv().await {
        let target = {
            let mut r = shared.routes.lock().unwrap();
            let sid = n
                .params
                .get("session_id")
                .and_then(Value::as_str)
                .map(str::to_string);
            let turn = n.params.get("turn_id").and_then(Value::as_str);
            if let (Some(sid), Some(turn)) = (&sid, turn) {
                if n.method == theseus_protocol::notify::TURN_STARTED {
                    r.turns.insert(turn.to_string(), sid.clone());
                }
            }
            let sid = sid.or_else(|| turn.and_then(|t| r.turns.get(t).cloned()));
            if n.method == theseus_protocol::notify::TURN_ENDED {
                if let Some(t) = turn {
                    r.turns.remove(t);
                }
            }
            sid.and_then(|s| r.by_session.get(&s).cloned())
        };
        if let Some(tx) = target {
            let _ = tx.send(PlaceMsg::Event(n));
        }
    }
}

struct Place {
    shared: Arc<Shared>,
    key: String,
    kind: &'static str,
    label: String,
    channel: Option<Id<ChannelMarker>>,
    users: Vec<u64>,
    mention_only: bool,
    session_id: String,
    renderer: Renderer,
    /// render key → Discord message id
    msgs: HashMap<String, Id<MessageMarker>>,
    /// A `turn.submit` of ours is outstanding.
    inflight: bool,
    /// Messages that arrived mid-turn, for the next turn: (author, text, message).
    queued: Vec<(String, String, Id<MessageMarker>)>,
    /// The first message of the next turn replies to this one.
    anchor: Option<Id<MessageMarker>>,
    saw_failure: bool,
    last_activity_ms: u64,
    tx: mpsc::UnboundedSender<PlaceMsg>,
}

impl Place {
    async fn run(mut self, mut rx: mpsc::UnboundedReceiver<PlaceMsg>) {
        let mut tick = tokio::time::interval(self.shared.edit_interval);
        let mut typing = tokio::time::interval(Duration::from_secs(8));
        loop {
            tokio::select! {
                m = rx.recv() => match m {
                    Some(m) => self.handle(m).await,
                    None => break,
                },
                _ = tick.tick() => {
                    let ops = self.renderer.tick();
                    self.apply(ops).await;
                }
                _ = typing.tick() => {
                    if self.inflight || self.renderer.busy() {
                        self.typing().await;
                    }
                }
            }
        }
    }

    async fn handle(&mut self, m: PlaceMsg) {
        match m {
            PlaceMsg::Inbound {
                author,
                text,
                message,
            } => {
                self.last_activity_ms = theseus_protocol::now_unix_ms();
                self.shared.core.binding_ledger(
                    "discord.message.in",
                    Some(&self.session_id),
                    json!({"place": self.label, "author": author, "chars": text.chars().count(), "message_id": message.to_string()}),
                );
                if let Some(cmd) = parse_control(&text) {
                    let reply = self.control(cmd, &format!("discord:{author}")).await;
                    self.say(&reply, Some(message)).await;
                    return;
                }
                if self.inflight {
                    self.queued.push((author, text, message));
                } else {
                    self.submit(vec![(author, text, message)]);
                }
                self.report();
            }
            PlaceMsg::Event(n) => {
                if n.method == theseus_protocol::notify::TURN_FAILED {
                    self.saw_failure = true;
                }
                let ops = self.renderer.on_notification(&n.method, &n.params);
                self.apply(ops).await;
            }
            PlaceMsg::SubmitDone(r) => {
                self.inflight = false;
                if let Err(e) = r {
                    let class = e
                        .data
                        .get("class")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let gone = e.code == theseus_protocol::error_code::NOT_FOUND;
                    if gone || class.starts_with("execution_") || class == "budget_exhausted" {
                        let class = if gone {
                            "session missing".to_string()
                        } else {
                            class
                        };
                        let note = format!(
                            "This place's session can't take turns any more ({class}), so I opened a new one. The old one stays in the web UI."
                        );
                        match self.rebind().await {
                            Ok(()) => {
                                self.say(&note, None).await;
                                let batch = std::mem::take(&mut self.queued);
                                if !batch.is_empty() {
                                    self.submit(batch);
                                }
                                return;
                            }
                            Err(err) => self.shared.board.error("rebind", None, err),
                        }
                    } else if !self.saw_failure {
                        self.say(&format!("⚠️ {}", e.message), None).await;
                    }
                }
                let batch = std::mem::take(&mut self.queued);
                if !batch.is_empty() {
                    self.submit(batch);
                }
                self.report();
            }
            PlaceMsg::Control { cmd, by, reply } => {
                let text = self.control(cmd, &by).await;
                let _ = reply.send(text);
            }
            PlaceMsg::Notice(text) => self.say(&text, None).await,
            PlaceMsg::DmChannel(c) => {
                if self.channel != Some(c) {
                    self.channel = Some(c);
                    let mut r = self.shared.routes.lock().unwrap();
                    r.by_channel.insert(c.get(), self.tx.clone());
                    r.users.insert(c.get(), self.users.clone());
                    drop(r);
                    self.report();
                }
            }
        }
    }

    /// Start a turn with these messages (one, or several coalesced, authors kept).
    fn submit(&mut self, batch: Vec<(String, String, Id<MessageMarker>)>) {
        let one_author = batch.iter().all(|(a, _, _)| *a == batch[0].0);
        let input = if batch.len() == 1 {
            batch[0].1.clone()
        } else if one_author {
            batch
                .iter()
                .map(|(_, t, _)| t.as_str())
                .collect::<Vec<_>>()
                .join("\n\n")
        } else {
            batch
                .iter()
                .map(|(a, t, _)| format!("[{a}] {t}"))
                .collect::<Vec<_>>()
                .join("\n\n")
        };
        let author = if one_author {
            format!("discord:{}", batch[0].0)
        } else {
            "discord".into()
        };
        self.anchor = batch.last().map(|b| b.2);
        self.inflight = true;
        self.saw_failure = false;
        let (rpc, tx, sid) = (
            self.shared.rpc.clone(),
            self.tx.clone(),
            self.session_id.clone(),
        );
        tokio::spawn(async move {
            let r = rpc
                .call::<_, TurnSubmitResult>(
                    theseus_protocol::method::TURN_SUBMIT,
                    TurnSubmitParams {
                        session_id: Some(sid),
                        input,
                        profile: None,
                        provider: None,
                        model: None,
                        author: Some(author),
                    },
                )
                .await
                .map(|_| ());
            let _ = tx.send(PlaceMsg::SubmitDone(r));
        });
    }

    async fn control(&mut self, cmd: Control, by: &str) -> String {
        match cmd {
            Control::Status => {
                let Some(s) = self.shared.session_info(&self.session_id).await else {
                    return format!("Session `{}` is not in the store.", self.session_id);
                };
                format!(
                    "Session `{}` ({}) · execution {} · {} turn(s) · ${:.4} · {} tool call(s) · {} waiting for approval{}",
                    s.session_id,
                    self.label,
                    s.execution_state.as_deref().unwrap_or("?"),
                    s.turns,
                    s.cost_usd,
                    s.tool_calls,
                    s.pending_confirms,
                    s.model.map(|m| format!(" · last model {m}")).unwrap_or_default()
                )
            }
            Control::New => match self.rebind().await {
                Ok(()) => format!(
                    "🆕 New session `{}` here. The previous one stays in the web UI's session list.",
                    self.session_id
                ),
                Err(e) => format!("⚠️ Could not open a new session: {e}"),
            },
            Control::Stop => {
                let exec = self
                    .shared
                    .session_info(&self.session_id)
                    .await
                    .and_then(|s| s.execution_id);
                let mut said = String::new();
                if let Some(eid) = exec {
                    match self
                        .shared
                        .rpc
                        .call::<_, theseus_protocol::ExecutionCancelResult>(
                            theseus_protocol::method::EXECUTION_CANCEL,
                            theseus_protocol::ExecutionCancelParams {
                                execution_id: eid,
                                author: Some(by.to_string()),
                            },
                        )
                        .await
                    {
                        Ok(r) => {
                            said = format!(
                                "⏹️ Stopped: cancelled this session's work ({} running action(s) told to stop).",
                                r.cancelled_actions.len()
                            )
                        }
                        Err(e) => said = format!("⚠️ Could not cancel: {e}."),
                    }
                }
                self.queued.clear();
                match self.rebind().await {
                    Ok(()) => format!(
                        "{said} Your next message starts a new session (`{}`); the old one stays in the web UI.",
                        self.session_id
                    )
                    .trim_start()
                    .to_string(),
                    Err(e) => format!("{said} Could not open a new session: {e}"),
                }
            }
        }
    }

    /// Point this place at a fresh session.
    async fn rebind(&mut self) -> anyhow::Result<()> {
        let old = self.session_id.clone();
        let sid = self.shared.open_session(&self.key, &self.label).await?;
        {
            let mut r = self.shared.routes.lock().unwrap();
            r.by_session.remove(&old);
            r.by_session.insert(sid.clone(), self.tx.clone());
        }
        let _ = self
            .shared
            .rpc
            .call::<_, Value>(
                theseus_protocol::method::SESSION_UNWATCH,
                SessionRef { session_id: old },
            )
            .await;
        self.session_id = sid;
        self.renderer = Renderer::default();
        self.report();
        Ok(())
    }

    async fn apply(&mut self, ops: Vec<Op>) {
        for op in ops {
            match op {
                Op::Typing => self.typing().await,
                Op::Upsert {
                    key,
                    content,
                    buttons,
                } => self.upsert(&key, &content, buttons).await,
            }
        }
    }

    async fn typing(&self) {
        if let Some(c) = self.channel {
            let _ = self.shared.http.create_typing_trigger(c).await;
        }
    }

    async fn say(&mut self, text: &str, reply_to: Option<Id<MessageMarker>>) {
        let key = format!("note:{}", theseus_protocol::now_unix_ms());
        self.anchor = reply_to.or(self.anchor);
        self.upsert(&key, text, Buttons::Keep).await;
    }

    async fn upsert(&mut self, key: &str, content: &str, buttons: Buttons) {
        let Some(channel) = self.channel else {
            return; // a DM whose channel has not opened yet
        };
        if content.trim().is_empty() {
            return;
        }
        let none = AllowedMentions::default();
        let comps = match &buttons {
            Buttons::Confirm(corr) => Some(confirm_buttons(corr)),
            Buttons::Clear => Some(vec![]),
            Buttons::Keep => None,
        };
        let http = &self.shared.http;
        if let Some(mid) = self.msgs.get(key).copied() {
            let mut req = http
                .update_message(channel, mid)
                .content(Some(content))
                .allowed_mentions(Some(&none));
            if let Some(c) = &comps {
                req = req.components(Some(c));
            }
            match req.await {
                Ok(_) => self.shared.board.update(|s| s.edits += 1),
                Err(e) => self
                    .shared
                    .board
                    .error("edit message", Some(&self.session_id), e),
            }
            return;
        }
        let mut req = http
            .create_message(channel)
            .content(content)
            .allowed_mentions(Some(&none));
        if let Some(c) = comps.as_deref().filter(|c| !c.is_empty()) {
            req = req.components(c);
        }
        let anchor = self.anchor.take();
        if let Some(a) = anchor {
            req = req.reply(a).fail_if_not_exists(false);
        }
        match req.await {
            Ok(resp) => match resp.model().await {
                Ok(m) => {
                    self.msgs.insert(key.to_string(), m.id);
                    self.shared.board.update(|s| s.messages_out += 1);
                    self.shared.core.binding_ledger(
                        "discord.message.out",
                        Some(&self.session_id),
                        json!({"place": self.label, "message_id": m.id.to_string(), "part": key, "chars": content.chars().count(), "buttons": matches!(buttons, Buttons::Confirm(_))}),
                    );
                }
                Err(e) => self
                    .shared
                    .board
                    .error("read sent message", Some(&self.session_id), e),
            },
            Err(e) => self
                .shared
                .board
                .error("send message", Some(&self.session_id), e),
        }
    }

    fn report(&self) {
        self.shared.board.place(PlaceStatus {
            kind: self.kind.into(),
            label: self.label.clone(),
            channel_id: self.channel.map(|c| c.to_string()),
            session_id: Some(self.session_id.clone()),
            users: self.users.iter().map(u64::to_string).collect(),
            mention_only: self.mention_only,
            last_activity_ms: self.last_activity_ms,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_and_confirm_ids_parse() {
        assert_eq!(parse_control("/stop"), Some(Control::Stop));
        assert_eq!(parse_control("  /cancel now"), Some(Control::Stop));
        assert_eq!(parse_control("/new"), Some(Control::New));
        assert_eq!(parse_control("/status"), Some(Control::Status));
        assert_eq!(parse_control("please /stop"), None);
        assert_eq!(parse_control("/stopper"), None);
        assert_eq!(
            parse_confirm_id("confirm:approve:act_1"),
            Some((true, "act_1".into()))
        );
        assert_eq!(
            parse_confirm_id("confirm:decline:act_2"),
            Some((false, "act_2".into()))
        );
        assert_eq!(parse_confirm_id("confirm:maybe:x"), None);
        assert_eq!(parse_confirm_id("other"), None);
        assert!(terminal(Some("cancelled")));
        assert!(!terminal(Some("waiting")));
    }

    #[test]
    fn mention_only_channels_answer_mentions_and_replies_only() {
        let (bot, role) = (1553557742759706625u64, 42u64);
        assert!(addressed("hi", &[bot], &[], None, bot, &[role]));
        assert!(addressed(
            "<@1553557742759706625> hi",
            &[],
            &[],
            None,
            bot,
            &[role]
        ));
        assert!(addressed(
            "<@!1553557742759706625> hi",
            &[],
            &[],
            None,
            bot,
            &[role]
        ));
        assert!(addressed("<@&42> hi", &[], &[role], None, bot, &[role]));
        assert!(addressed("thanks", &[], &[], Some(bot), bot, &[role]));
        assert!(!addressed("@Tabitha hi", &[7], &[], None, bot, &[role]));
        assert!(!addressed("hi all", &[], &[9], Some(7), bot, &[role]));
        assert_eq!(
            strip_mentions("<@1553557742759706625>  run the tests", bot, &[role]),
            "run the tests"
        );
        assert_eq!(strip_mentions("hey <@&42> look", bot, &[role]), "hey look");
    }

    #[test]
    fn confirm_buttons_carry_the_correlation_id() {
        let c = confirm_buttons("act_9");
        let Component::ActionRow(row) = &c[0] else {
            panic!()
        };
        let ids: Vec<_> = row
            .components
            .iter()
            .map(|b| match b {
                Component::Button(b) => b.custom_id.clone().unwrap(),
                _ => panic!(),
            })
            .collect();
        assert_eq!(ids, vec!["confirm:approve:act_9", "confirm:decline:act_9"]);
    }
}
