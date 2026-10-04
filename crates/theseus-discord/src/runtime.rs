//! The gateway loop, the places (a text channel or a DM, each backed by one
//! conversation session), and the lanes that write to Discord.
//!
//! One actor per place: it serializes that place's turns, coalesces messages
//! that arrive mid-turn into the next turn (authors kept), and renders the
//! live progress of its session's turns. A router hands each notification
//! from the core to the place whose session it names. One lane per place
//! writes its messages (`courier`): the outbox's posts first, in order, then
//! the live progress, so what must reach Discord does even when a place's
//! actor never saw it (theseus-q4v).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use theseus_protocol::LedgerKind;

use serde_json::{json, Value};
use theseus_core::approval::{Checked, Client, Surface};
use theseus_core::config::DiscordConfig;
use theseus_core::outbox::OPERATOR_TARGET;
use theseus_core::Core;
use theseus_protocol::Event as CoreEvent;
use theseus_protocol::{
    Attachment, BindingStatus, DiscordOrigin, Notification, PlaceStatus, PolicyTightenParams,
    SessionInfo, SessionKind, SessionListResult, SessionOpenParams, SessionRef, TightenResult,
    TrustResult, TurnSubmitParams, TurnSubmitResult,
};
use tokio::sync::{mpsc, oneshot};
use twilight_gateway::{Event, EventTypeFlags, Intents, Shard, ShardId, StreamExt as _};
use twilight_http::Client as Http;
use twilight_model::application::command::CommandType;
use twilight_model::application::interaction::{Interaction, InteractionData};
use twilight_model::channel::message::component::{
    ActionRow, Button, ButtonStyle, Component, SelectMenu, SelectMenuOption, SelectMenuType,
};
use twilight_model::channel::message::{AllowedMentions, MessageFlags};
use twilight_model::http::interaction::{
    InteractionResponse, InteractionResponseData, InteractionResponseType,
};
use twilight_model::id::marker::{ApplicationMarker, ChannelMarker, GuildMarker, MessageMarker};
use twilight_model::id::Id;
use twilight_util::builder::command::CommandBuilder;

use crate::bindings::{snowflake, Bindings};
use crate::courier::{self, Lane, LaneMsg, SendErr};
use crate::files;
use crate::render::{Asked, Renderer};
use crate::rpc_client::{CallError, RpcClient};
use crate::viewers;

mod audience;
mod publish;
mod voice;
use publish::PublishAsk;

/// The connection label every Discord call carries; authors refine it per message.
const CLIENT: &str = "discord";

/// Health's view of the binding, pushed to the core on every change.
#[derive(Clone)]
pub(crate) struct Board {
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
        core.bindings.set(st.clone());
        Self {
            core,
            st: Arc::new(Mutex::new(st)),
        }
    }

    pub(crate) fn update(&self, f: impl FnOnce(&mut BindingStatus)) {
        let snapshot = {
            let mut g = self.st.lock().unwrap();
            f(&mut g);
            g.clone()
        };
        self.core.bindings.set(snapshot);
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

    pub(crate) fn error(&self, op: &str, session: Option<&str>, e: impl std::fmt::Display) {
        let msg = format!("{op}: {e}");
        tracing::warn!(error = %msg, "discord");
        self.core.binding_ledger(
            LedgerKind::DiscordError,
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
/// goes wrong is a state in health and a `discord.error` ledger row. The bot
/// token comes from the secret board once it resolves (theseus-qa0). Nothing
/// waits for the binding (theseus-q4v): what must reach its places is in the
/// outbox, and it delivers that when it can.
pub async fn run(core: Arc<Core>, cfg: DiscordConfig, path: PathBuf) {
    let board = Board::new(core.clone());
    if !cfg.enabled {
        board.state("disabled", Some("[discord].enabled = false".into()));
        return;
    }
    // Nothing talks to Discord on a config copy's word (theseus-2fo).
    if !core.config_gate.is_open() {
        board.state(
            "waiting",
            Some("waiting for the vault to confirm the config this daemon started from".into()),
        );
        if !core.config_gate.opened().await {
            return;
        }
    }
    board.update(|s| s.bindings_file = Some(path.display().to_string()));
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
    // Each place's class follows from the file (the place rule): told now,
    // before any message from them is read.
    core.bind_places(bound_places(&bindings));
    let Some(token) = bot_token(&core, &cfg.token_secret, &board).await else {
        return;
    };
    if let Err(e) = serve(core, cfg, token, bindings, board.clone()).await {
        board.state("failed", Some(format!("{e:#}")));
    }
}

/// The places the file binds, as the place rule takes them.
fn bound_places(b: &Bindings) -> Vec<theseus_core::places::BoundPlace> {
    let channels = b.channel.iter().map(|c| theseus_core::places::BoundPlace {
        target: format!("discord:channel:{}", c.id),
        name: c.label(),
        private: c.private,
    });
    let dms = b.dm.iter().map(|d| theseus_core::places::BoundPlace {
        target: format!("discord:dm:{}", d.user),
        name: d.label(),
        private: false,
    });
    channels.chain(dms).collect()
}

/// The bot token, once the vault gives it. Fail closed: the binding never
/// connects without it. While it resolves the binding is `waiting`; if it
/// fails, the binding is `failed` with the reason, and connects when a retry
/// resolves the token.
async fn bot_token(core: &Arc<Core>, name: &str, board: &Board) -> Option<String> {
    use theseus_core::secrets::SecretState;
    let t0 = std::time::Instant::now();
    let phase = core.startup_log.begin("discord.token", true, t0);
    let mut rx = core.secrets.subscribe();
    loop {
        let state = rx.borrow_and_update().get(name).cloned();
        match state {
            Some(SecretState::Ready(token)) => {
                core.startup_log.end(
                    phase,
                    json!({"secret": name, "waited_ms": t0.elapsed().as_millis() as u64, "outcome": "ready"}),
                );
                return Some(token.expose().to_string());
            }
            None => {
                board.state(
                    "unconfigured",
                    Some(format!(
                        "no [secrets] entry named {name:?} for the bot token"
                    )),
                );
                core.startup_log
                    .end(phase, json!({"secret": name, "outcome": "unconfigured"}));
                return None;
            }
            Some(SecretState::Resolving) => board.state(
                "waiting",
                Some(format!("waiting for the secret {name} to resolve")),
            ),
            Some(SecretState::Failed(why)) => {
                board.state(
                    "failed",
                    Some(format!(
                        "the secret {name} did not resolve ({why}); the binding connects when a \
                         retry resolves it"
                    )),
                );
                core.startup_log.end(
                    phase,
                    json!({"secret": name, "waited_ms": t0.elapsed().as_millis() as u64, "outcome": "failed", "error": why}),
                );
            }
        }
        if rx.changed().await.is_err() {
            return None;
        }
    }
}

/// The REST client: Discord's, or a local stand-in when `[discord]
/// rest_proxy` names one (tests and scratch daemons), over plain http.
fn http_client(token: &str, cfg: &DiscordConfig) -> Http {
    let b = Http::builder().token(token.to_string());
    match cfg.rest_proxy.as_deref().filter(|p| !p.is_empty()) {
        Some(p) => b.proxy(p.to_string(), true).build(),
        None => b.build(),
    }
}

async fn serve(
    core: Arc<Core>,
    cfg: DiscordConfig,
    token: String,
    bindings: Bindings,
    board: Board,
) -> anyhow::Result<()> {
    let (shared, notes, guild, me) = connect(&core, &cfg, &token, &bindings, &board).await?;
    start_places(&shared, &bindings, &board, notes).await?;
    event_loop(&shared, &cfg, token, guild, &me, &bindings, &board).await;
    Ok(())
}

/// REST, the binding's state, its lanes and courier (what the outbox holds
/// needs only REST), and who the bot is, asked until Discord answers.
async fn connect(
    core: &Arc<Core>,
    cfg: &DiscordConfig,
    token: &str,
    bindings: &Bindings,
    board: &Board,
) -> anyhow::Result<(
    Arc<Shared>,
    mpsc::UnboundedReceiver<Notification>,
    Id<GuildMarker>,
    twilight_model::user::CurrentUser,
)> {
    // Both rustls providers are compiled into this workspace; pick one for the process.
    let _ = rustls::crypto::ring::default_provider().install_default();
    board.state("connecting", None);
    let http = Arc::new(http_client(token, cfg));
    let guild: Id<GuildMarker> = Id::new(snowflake("guild_id", &bindings.guild_id)?);
    let (rpc, notes) = RpcClient::connect(core.clone(), Client::new(CLIENT, Surface::Discord));
    let shared = Arc::new(Shared {
        core: core.clone(),
        rpc: rpc.clone(),
        http: http.clone(),
        board: board.clone(),
        bot_id: AtomicU64::new(0),
        edit_interval: Duration::from_millis(cfg.edit_interval_ms.max(250)),
        notice_embeds: cfg.notice_embeds,
        routes: Mutex::new(Routes::default()),
        files_http: files::client(),
        max_text: core.cfg.tools.max_read_bytes as u64,
        members_intent: OnceLock::new(),
        lanes: Mutex::new(HashMap::new()),
        voice: voice::Voice::new(&core.cfg.voice, bindings),
    });
    // The lanes first: what the outbox holds for these places needs only
    // REST, so it goes out while the rest connects, or while the gateway is
    // down (theseus-q4v).
    shared.clone().start_lanes(bindings)?;
    // A post for a place this file no longer names is refused, not kept
    // pending forever (theseus-l3m).
    shared.refuse_unbound();
    tokio::spawn(courier::courier(shared.clone()));
    shared.wake_lanes();

    // Who the bot is and what it may do, asked until Discord answers.
    let me = shared.connect(guild).await;
    Ok((shared, notes, guild, me))
}

/// Every place, each with its session, and the routing of the core's events
/// to them; then the cards whose question closed meanwhile, and the
/// `[approval]` channels' viewers.
async fn start_places(
    shared: &Arc<Shared>,
    bindings: &Bindings,
    board: &Board,
    notes: mpsc::UnboundedReceiver<Notification>,
) -> anyhow::Result<()> {
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
        let channel = match shared.dm_channel(user).await {
            Ok(c) => Some(Id::new(c)),
            Err(e) => {
                board.error("open DM channel", None, &e.message);
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
    // A card whose question closed while the binding was away (a raise at
    // the vault's confirmation, an answer from the CLI) says how, and loses
    // its buttons: a settle each, delivered like any post (theseus-q4v).
    match shared.core.outbox.reconcile_cards() {
        Ok(0) => {}
        Ok(n) => tracing::info!(
            settles = n,
            "discord: cards whose question closed meanwhile"
        ),
        Err(e) => tracing::warn!(error = %format!("{e:#}"), "discord: reconciling cards failed"),
    }
    shared.wake_lanes();
    // Who can view each guild channel `[approval]` lists, for health and for
    // the first answer; each card and each answer checks again.
    let checks = shared.clone();
    let private: Vec<(u64, String)> = bindings
        .channel
        .iter()
        .filter(|c| c.private)
        .filter_map(|c| Some((c.id.parse().ok()?, c.label())))
        .collect();
    tokio::spawn(async move {
        // Each channel bound `private = true`, read once (the place rule):
        // health warns when anyone besides the owner can view it.
        for (c, name) in &private {
            checks.check_private(*c, name).await;
        }
        for c in &checks.core.approval.discord_channels() {
            checks.check_channel(*c).await;
        }
    });
    Ok(())
}

/// The gateway: until its stream ends, each event Discord sends, with the
/// connection's state on the board.
async fn event_loop(
    shared: &Arc<Shared>,
    cfg: &DiscordConfig,
    token: String,
    guild: Id<GuildMarker>,
    me: &twilight_model::user::CurrentUser,
    bindings: &Bindings,
    board: &Board,
) {
    let intents = Intents::GUILDS
        | Intents::GUILD_MESSAGES
        | Intents::DIRECT_MESSAGES
        | Intents::MESSAGE_CONTENT
        | shared.voice.intents();
    let mut gateway = twilight_gateway::ConfigBuilder::new(token, intents);
    if let Some(p) = cfg.gateway_proxy.as_deref().filter(|p| !p.is_empty()) {
        // A local stand-in for Discord's gateway (tests and scratch daemons).
        gateway = gateway.proxy_url(p.to_string());
    }
    let mut shard = Shard::with_config(ShardId::ONE, gateway.build());
    voice::attach(shared, &shard, me.id);
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
        shared.voice.gateway(&event);
        match event {
            Event::Ready(r) => {
                board.update(|s| {
                    s.state = "ready".into();
                    s.connected_at_ms = theseus_protocol::now_unix_ms();
                    if r.guilds.iter().any(|g| g.id == guild) {
                        s.detail = None;
                    }
                });
                shared.core.binding_ledger(
                    LedgerKind::DiscordReady,
                    None,
                    json!({"bot": me.name, "guilds": r.guilds.len(), "revision": bindings.revision}),
                );
                tracing::info!(bot = %me.name, "discord gateway ready");
                // Back: whatever waited goes now (the lanes back off alone).
                shared.wake_lanes();
            }
            Event::Resumed => {
                board.state("ready", None);
                shared.wake_lanes();
            }
            Event::GuildCreate(g) if g.id() == guild => {
                board.update(|s| s.detail = None);
                shared.refresh_bot_roles(guild).await;
            }
            Event::GatewayClose(frame) => {
                let why = frame
                    .map(|f| format!("close {} {}", f.code, f.reason))
                    .unwrap_or_else(|| "closed".into());
                shared.core.binding_ledger(
                    LedgerKind::DiscordDisconnected,
                    None,
                    json!({"why": why}),
                );
                board.state("resuming", Some(why));
            }
            Event::MessageCreate(m) => shared.clone().on_message(&m.0),
            Event::InteractionCreate(i) => {
                let s = shared.clone();
                let i = i.0;
                tokio::spawn(async move { s.on_interaction(i).await });
            }
            _ => {}
        }
    }
    board.state("disconnected", Some("gateway stream ended".into()));
}

async fn register_commands(http: &Http, app: Id<ApplicationMarker>, board: &Board) {
    let cmds = commands();
    match http.interaction(app).set_global_commands(&cmds).await {
        Ok(_) => tracing::info!(commands = cmds.len(), "discord slash commands registered"),
        Err(e) => board.error("register slash commands", None, e),
    }
}

/// The slash commands every place answers, one effect each (Eddie's rule):
/// `/stop` halts this session's own work and keeps the conversation (W1),
/// `/new` alone starts a fresh session, `/trust` clears its hold on web text
/// (T1b), and `/cancel` names a task (DD7) or a pending wake (DD8), by one
/// option, `id`.
fn commands() -> Vec<twilight_model::application::command::Command> {
    let mut cmds: Vec<_> = [
        (
            "stop",
            "Stop what Theseus is doing here: its turn, jobs, and queued messages. The conversation goes on",
        ),
        (
            "new",
            "Start a fresh session here; the old one stays in the web UI",
        ),
        (
            "status",
            "This place's session: state, turns, dollars, waiting confirms",
        ),
        (
            "tasks",
            "This place's background tasks: state, spend, and what each waits on",
        ),
        (
            "wakes",
            "This place's pending wakes: when each is due, and its note",
        ),
        (
            "trust",
            "Trust this conversation again after it read web text: its calls that change things stop waiting",
        ),
    ]
    .into_iter()
    .map(|(n, d)| CommandBuilder::new(n, d, CommandType::ChatInput).build())
    .collect();
    cmds.push(
        CommandBuilder::new(
            "cancel",
            "Stop a background task and its jobs, or cancel a wake; /tasks and /wakes list them",
            CommandType::ChatInput,
        )
        .option(
            twilight_util::builder::command::StringBuilder::new(
                "id",
                "The task's or wake's id, or its last six characters, as /tasks and /wakes show it",
            )
            .required(true),
        )
        .build(),
    );
    // `/publish` (the place rule): only the owner, from a private place; the
    // core judges it. The channel first, as Discord wants required options.
    let text = |name: &str, what: &str| {
        twilight_util::builder::command::StringBuilder::new(name, what).required(false)
    };
    cmds.push(
        CommandBuilder::new(
            "publish",
            "Publish a node, a file, or a message into a place (only you, from a private place)",
            CommandType::ChatInput,
        )
        .option(
            twilight_util::builder::command::ChannelBuilder::new("to", "The place to publish into")
                .required(true),
        )
        .option(text("node", "A node's id, as the web UI shows it"))
        .option(text("file", "A file's path, absolute or ~/"))
        .option(text("text", "A message"))
        .option(text("note", "Your words above it"))
        .build(),
    );
    cmds.extend(voice::commands());
    cmds
}

/// What every place, lane, and the gateway share.
pub(crate) struct Shared {
    pub(crate) core: Arc<Core>,
    rpc: Arc<RpcClient>,
    pub(crate) http: Arc<Http>,
    pub(crate) board: Board,
    bot_id: AtomicU64,
    edit_interval: Duration,
    /// `[discord] notice_embeds`: each place's renderer posts notice cards.
    notice_embeds: bool,
    routes: Mutex<Routes>,
    /// Downloads a message's attachments (theseus-9g2).
    files_http: reqwest::Client,
    /// `[tools].max_read_bytes`: the largest text attachment downloaded.
    max_text: u64,
    /// The portal has the Server Members intent on: a listed guild channel's
    /// viewers can be checked (theseus-sgh). Asked on first need.
    members_intent: OnceLock<bool>,
    /// Each place's lane, and the operator's, by target (theseus-q4v).
    lanes: Mutex<HashMap<String, mpsc::UnboundedSender<LaneMsg>>>,
    voice: voice::Voice,
}

/// One message for a turn: who wrote it, what it says, its files (still
/// downloading, theseus-9g2), its Discord id, and where it was written.
struct Inbound {
    author: String,
    author_id: u64,
    text: String,
    files: Option<Pending>,
    message: Id<MessageMarker>,
    channel: Id<ChannelMarker>,
    /// None in a DM.
    guild: Option<Id<GuildMarker>>,
}

impl Inbound {
    /// Who wrote it and where, as the core judges an answer (a typed
    /// `/trust`, T1b).
    fn origin(&self) -> DiscordOrigin {
        DiscordOrigin {
            user_id: self.author_id.to_string(),
            channel_id: self.channel.to_string(),
            guild_id: self.guild.map(|g| g.to_string()),
        }
    }
}

/// A message's attachments, downloading in their own task: the gateway loop
/// awaits each message's handler, so it never waits on a download.
struct Pending {
    metas: Vec<files::FileMeta>,
    task: tokio::task::JoinHandle<Vec<Attachment>>,
}

impl Pending {
    /// The entries for `turn.submit`. A task that never reported back lists
    /// its files as failed downloads; the message itself still goes.
    async fn wait(self) -> Vec<Attachment> {
        match self.task.await {
            Ok(v) => v,
            Err(e) => files::lost(&self.metas, &e.to_string()),
        }
    }
}

#[derive(Default)]
struct Routes {
    /// session id → place mailbox
    by_session: HashMap<String, mpsc::UnboundedSender<PlaceMsg>>,
    /// Discord channel id → place mailbox
    by_channel: HashMap<u64, mpsc::UnboundedSender<PlaceMsg>>,
    /// DM user id → place mailbox (the DM channel may open later)
    by_dm_user: HashMap<u64, mpsc::UnboundedSender<PlaceMsg>>,
    /// The DM places in bindings-file order: (user id, label)
    dms: Vec<(u64, String)>,
    /// DM user id → the DM's channel id, once it is open
    dm_channel: HashMap<u64, u64>,
    /// channel id → who may drive it
    users: HashMap<u64, Vec<u64>>,
    /// channel ids where only an @mention or a reply to the bot starts a turn
    mention_only: std::collections::HashSet<u64>,
    /// the bot's roles in the guild (an @Theseus can arrive as its managed role)
    bot_roles: Vec<u64>,
    /// turn id → session id (deltas name only the turn)
    turns: HashMap<String, String>,
}

/// The place a message or an interaction belongs to (`Routes::resolve`).
struct Resolved {
    place: mpsc::UnboundedSender<PlaceMsg>,
    /// Its author may drive the place.
    allowed: bool,
    /// Only an @mention or a reply to the bot starts a turn there.
    mention_only: bool,
}

impl Routes {
    /// The place a message or an interaction belongs to, found one way for
    /// both (theseus-e89, theseus-0g4): in a guild by its channel alone, its
    /// author allowed when the channel lists them; in a DM by its author's DM
    /// binding, which only they reach. `None`: not a place of ours, which
    /// gets no answer, so that a daemon on the same bot that binds it can give
    /// one.
    fn resolve(&self, channel: Option<u64>, guild: bool, author: Option<u64>) -> Option<Resolved> {
        if !guild {
            return Some(Resolved {
                place: self.by_dm_user.get(&author?)?.clone(),
                allowed: true,
                mention_only: false,
            });
        }
        let channel = channel?;
        Some(Resolved {
            place: self.by_channel.get(&channel)?.clone(),
            allowed: author
                .is_some_and(|a| self.users.get(&channel).is_some_and(|u| u.contains(&a))),
            mention_only: self.mention_only.contains(&channel),
        })
    }
}

enum PlaceMsg {
    Inbound(Inbound),
    Event(Box<CoreEvent>),
    SubmitDone(Result<(), CallError>),
    Control {
        cmd: Control,
        by: String,
        reply: oneshot::Sender<String>,
    },
    DmChannel(Id<ChannelMarker>),
    Voice(voice::VoiceTurn),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Control {
    Stop,
    New,
    Status,
    /// This place's tasks (DD7).
    Tasks,
    /// This place's pending wakes (DD8).
    Wakes,
    /// Cancel the wake or stop the task named; None asks which.
    Cancel(Option<String>),
    /// Trust this place's session again after it read web text (T1b), as
    /// the presser, whose ids the handler fills in: None names no one, and
    /// counts only without an `[approval]` section.
    Trust(Option<DiscordOrigin>),
    /// Publish into a place (the place rule), as the presser.
    Publish(Box<PublishAsk>),
    /// Join a voice channel (44b): the one named, as the presser.
    Join(Option<u64>, Option<DiscordOrigin>),
    Leave,
}

/// What a place says when it is bound to a fresh session: how to talk, and
/// each control with its one effect (W1: `/stop` keeps the conversation).
fn bind_notice(session_id: &str, mention_only: bool) -> String {
    let how = if mention_only {
        "@mention me or reply to one of my messages to talk"
    } else {
        "Talk to me in this place"
    };
    format!(
        "🔗 Theseus is bound here (session `{session_id}`). {how}. `/stop` halts what I am doing \
         and keeps the conversation, `/new` starts a fresh one, `/trust` trusts the conversation \
         again after it reads web text, and `/status`, `/tasks` and `/wakes` show this place's; \
         everything shows in the web UI."
    )
}

/// What `/stop` answers (W1): what stopped, and that the conversation goes
/// on; and, when there are any, the tasks and wakes it left running, each of
/// which `/cancel <id>` stops.
fn stop_answer(r: &theseus_protocol::ExecutionStopResult) -> String {
    if !r.stopped {
        return format!(
            "Nothing to stop: this session's execution has ended ({}). `/new` starts a fresh one.",
            r.execution.state
        );
    }
    let mut out = format!(
        "⏹️ Stopped this session's work ({} running action(s) told to stop). The conversation \
         goes on; `/new` starts a fresh one.",
        r.stopped_actions.len()
    );
    let mut going = Vec::new();
    if r.tasks_running > 0 {
        going.push(format!("{} task(s)", r.tasks_running));
    }
    if r.wakes_pending > 0 {
        going.push(format!("{} wake(s)", r.wakes_pending));
    }
    if !going.is_empty() {
        out.push_str(&format!(
            " Its {} go on: `/tasks` and `/wakes` list them, and `/cancel <id>` stops one.",
            going.join(" and ")
        ));
    }
    out
}

/// What `/trust` answers (T1b): who trusted the conversation again, and what
/// it had read; or, when the core refused the presser, why.
fn trust_answer(r: &Result<TrustResult, CallError>) -> String {
    match r {
        Ok(t) => format!(
            "Trusted again by {}: this conversation no longer holds web text (it had read {}). \
             Calls that change things run at their postures again.",
            t.by,
            theseus_core::external::source(&t.held)
        ),
        Err(e) if e.code == theseus_protocol::error_code::REFUSED => format!(
            "🔐 Your /trust did not count: {}. This conversation still holds web text.",
            e.data
                .get("why")
                .and_then(Value::as_str)
                .unwrap_or(&e.message)
        ),
        Err(e) => format!("⚠️ Could not trust the conversation again: {e}."),
    }
}

fn parse_control(text: &str) -> Option<Control> {
    let mut words = text.split_whitespace();
    match words.next()? {
        "/stop" => Some(Control::Stop),
        "/cancel" => Some(Control::Cancel(words.next().map(str::to_string))),
        "/new" => Some(Control::New),
        "/status" => Some(Control::Status),
        "/tasks" => Some(Control::Tasks),
        "/wakes" => Some(Control::Wakes),
        "/trust" => Some(Control::Trust(None)),
        _ => None,
    }
}

impl Shared {
    /// A lane for every place in the bindings file, and one for the
    /// operator's notices: the one writer of each's messages (theseus-q4v).
    fn start_lanes(self: Arc<Self>, bindings: &Bindings) -> anyhow::Result<()> {
        let mut lanes = Vec::new();
        for c in &bindings.channel {
            let id = snowflake("channel id", &c.id)?;
            lanes.push((
                format!("discord:channel:{}", c.id),
                "channel",
                c.label(),
                Some(id),
                None,
            ));
        }
        for d in &bindings.dm {
            let user = snowflake("dm user", &d.user)?;
            self.routes.lock().unwrap().dms.push((user, d.label()));
            lanes.push((
                format!("discord:dm:{}", d.user),
                "dm",
                d.label(),
                None,
                Some(user),
            ));
        }
        lanes.push((
            OPERATOR_TARGET.to_string(),
            "operator",
            "the operator".into(),
            None,
            None,
        ));
        for (target, kind, label, channel, dm_user) in lanes {
            let (tx, rx) = mpsc::unbounded_channel();
            self.lanes.lock().unwrap().insert(target.clone(), tx);
            let lane = Lane::new(self.clone(), target, kind, label, channel, dm_user);
            tokio::spawn(lane.run(rx));
        }
        Ok(())
    }

    /// Tell every lane to look at the outbox.
    pub(crate) fn wake_lanes(&self) {
        for tx in self.lanes.lock().unwrap().values() {
            let _ = tx.send(LaneMsg::Wake);
        }
    }

    /// Refuse the posts for places the bindings file no longer names
    /// (theseus-l3m): a place is bound when it has a lane. The file is read
    /// when the binding starts, so a change to it takes effect, and is
    /// checked, at the next start; a post written later for such a place is
    /// refused at the courier's next wake.
    pub(crate) fn refuse_unbound(&self) {
        let bound: std::collections::HashSet<String> =
            self.lanes.lock().unwrap().keys().cloned().collect();
        let n = self
            .core
            .outbox
            .refuse_unbound("discord", |t| bound.contains(t));
        if n > 0 {
            tracing::info!(
                posts = n,
                "discord: posts for places no longer bound, refused"
            );
        }
    }

    fn lane(&self, target: &str) -> Option<mpsc::UnboundedSender<LaneMsg>> {
        self.lanes.lock().unwrap().get(target).cloned()
    }

    /// Whether this daemon binds `target`: the bindings file names it, so it
    /// has a lane (theseus-l3m, theseus-c3e).
    pub(crate) fn binds(&self, target: &str) -> bool {
        self.lanes.lock().unwrap().contains_key(target)
    }

    /// Who the bot is, what its application allows, the slash commands, and
    /// the bot's roles: asked until Discord answers, while the lanes deliver
    /// on their own.
    async fn connect(&self, guild: Id<GuildMarker>) -> twilight_model::user::CurrentUser {
        let mut attempt = 0u32;
        loop {
            match self.preamble(guild).await {
                Ok(me) => return me,
                Err(e) => {
                    attempt += 1;
                    let why = format!("{e:#}");
                    if attempt == 1 {
                        self.board.error("connect", None, &why);
                    }
                    self.board.state(
                        "connecting",
                        Some(format!("Discord did not answer ({why}); trying again")),
                    );
                    let wait =
                        Duration::from_secs(1 << attempt.min(6)).min(Duration::from_secs(60));
                    tokio::time::sleep(wait).await;
                }
            }
        }
    }

    async fn preamble(
        &self,
        guild: Id<GuildMarker>,
    ) -> anyhow::Result<twilight_model::user::CurrentUser> {
        let me = self.http.current_user().await?.model().await?;
        self.board
            .update(|s| s.bot_user = Some(format!("{} ({})", me.name, me.id)));
        self.bot_id.store(me.id.get(), Ordering::Relaxed);
        let app = self.http.current_user_application().await?.model().await?;
        let app_id = app.id;
        // Who can view a guild channel takes the member list, which Discord
        // gives only when the portal has the Server Members intent on
        // (theseus-sgh). It is read from the application's flags; the gateway
        // intents stay as they are, since asking for a privileged intent the
        // portal has off closes the gateway.
        let members_intent = viewers::members_intent(app.flags);
        let _ = self.members_intent.set(members_intent);
        self.board
            .update(|s| s.members_intent = Some(members_intent));
        let in_guild = self
            .http
            .current_user_guilds()
            .await?
            .models()
            .await?
            .iter()
            .any(|g| g.id == guild);
        if !in_guild {
            self.board.update(|s| {
                s.detail = Some(format!(
                    "the bot is not in guild {guild} yet; invite it: https://discord.com/oauth2/authorize?client_id={app_id}&scope=bot+applications.commands&permissions=117824"
                ))
            });
        }
        register_commands(&self.http, app_id, &self.board).await;
        self.refresh_bot_roles(guild).await;
        Ok(me)
    }

    fn bot_id(&self) -> u64 {
        self.bot_id.load(Ordering::Relaxed)
    }

    /// The DM channel with `user`: known, or opened now (REST only).
    pub(crate) async fn dm_channel(&self, user: u64) -> Result<u64, SendErr> {
        if let Some(c) = self.routes.lock().unwrap().dm_channel.get(&user).copied() {
            return Ok(c);
        }
        let c = self
            .http
            .create_private_channel(Id::new(user))
            .await
            .map_err(|e| SendErr::of(&e))?
            .model()
            .await
            .map_err(|e| SendErr {
                away: true,
                unsure: false,
                gone: false,
                message: e.to_string(),
            })?
            .id
            .get();
        self.routes.lock().unwrap().dm_channel.insert(user, c);
        if let Some(tx) = self.lane(&format!("discord:dm:{user}")) {
            let _ = tx.send(LaneMsg::Channel(c));
        }
        Ok(c)
    }

    /// Every bound DM's channel, so the DM approvals go to can be chosen.
    pub(crate) async fn open_dm_channels(&self) -> Result<(), SendErr> {
        let users: Vec<u64> = self
            .routes
            .lock()
            .unwrap()
            .dms
            .iter()
            .map(|(u, _)| *u)
            .collect();
        for u in users {
            self.dm_channel(u).await?;
        }
        Ok(())
    }

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
        let target = format!("discord:{key}");
        let lane = self
            .lane(&target)
            .ok_or_else(|| anyhow::anyhow!("no lane for {target}"))?;
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
                if let Some(c) = channel {
                    r.dm_channel.insert(users[0], c.get());
                }
            }
        }
        if let (Some(c), "dm") = (channel, kind) {
            let _ = lane.send(LaneMsg::Channel(c.get()));
        }
        let renderer = self.renderer();
        let place = Place {
            shared: self.clone(),
            key,
            target,
            kind,
            label,
            channel,
            users,
            mention_only,
            session_id,
            renderer,
            lane,
            inflight: false,
            queued: Vec::new(),
            saw_failure: false,
            stopped_turn: None,
            stopping: false,
            last_activity_ms: 0,
            tx,
        };
        place.report();
        if fresh {
            place.say(&bind_notice(&place.session_id, mention_only), None);
        }
        tokio::spawn(place.run(rx));
        Ok(())
    }

    /// The session stored for this place, if it still exists and can take
    /// turns; otherwise a new conversation session, stored for next time.
    async fn session_for(&self, key: &str, label: &str) -> anyhow::Result<(String, bool)> {
        if let Some(sid) = self.core.outbox.place_session(key)? {
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
                    opened_from: None,
                },
            )
            .await?;
        // The place's record, and where the session's posts go from now on.
        self.core.outbox.bind_place(key, &info.session_id)?;
        self.core.binding_ledger(
            LedgerKind::DiscordBound,
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
        let roles = match self.http.guild_member(guild, Id::new(self.bot_id())).await {
            Ok(r) => match r.model().await {
                Ok(m) => m.roles.iter().map(|r| r.get()).collect(),
                Err(_) => vec![],
            },
            Err(_) => vec![], // not in the guild yet
        };
        self.routes.lock().unwrap().bot_roles = roles;
    }

    fn on_message(self: Arc<Self>, m: &twilight_model::channel::Message) {
        if m.author.bot || m.author.id.get() == self.bot_id() {
            return;
        }
        let (found, bot_roles) = {
            let r = self.routes.lock().unwrap();
            let found = r.resolve(
                Some(m.channel_id.get()),
                m.guild_id.is_some(),
                Some(m.author.id.get()),
            );
            (found, r.bot_roles.clone())
        };
        let Some(Resolved {
            place: tx,
            allowed,
            mention_only,
        }) = found
        else {
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
                self.bot_id(),
                &bot_roles,
            ) {
                return; // talk in a shared channel that is not for Theseus
            }
        }
        if !allowed {
            self.board.update(|s| s.ignored += 1);
            self.core.binding_ledger(
                LedgerKind::DiscordIgnored,
                None,
                json!({"channel": m.channel_id.to_string(), "author": m.author.name, "author_id": m.author.id.to_string(), "reason": "not in this place's users"}),
            );
            return;
        }
        if m.guild_id.is_none() {
            let _ = tx.send(PlaceMsg::DmChannel(m.channel_id));
        }
        let text = if mention_only {
            strip_mentions(&m.content, self.bot_id(), &bot_roles)
        } else {
            m.content.clone()
        };
        if text.trim().is_empty() && m.attachments.is_empty() {
            return;
        }
        // The downloads run beside the gateway loop; the place's submit
        // waits for them, so the message keeps its place in line.
        let files = (!m.attachments.is_empty()).then(|| {
            let metas: Vec<files::FileMeta> =
                m.attachments.iter().map(files::FileMeta::of).collect();
            let task = tokio::spawn(files::fetch_all(
                self.files_http.clone(),
                metas.clone(),
                self.max_text,
            ));
            Pending { metas, task }
        });
        self.board.update(|s| s.messages_in += 1);
        let _ = tx.send(PlaceMsg::Inbound(Inbound {
            author: m.author.name.clone(),
            author_id: m.author.id.get(),
            text,
            files,
            message: m.id,
            channel: m.channel_id,
            guild: m.guild_id,
        }));
    }

    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
    async fn on_interaction(self: Arc<Self>, i: Interaction) {
        let user = i.author().map(|u| (u.id.get(), u.name.clone()));
        let channel = i.channel.as_ref().map(|c| c.id.get());
        // Its place is found as a message's is (theseus-e89), so a command in
        // a guild channel never acts on a DM.
        let found = self.routes.lock().unwrap().resolve(
            channel,
            i.guild_id.is_some(),
            user.as_ref().map(|(u, _)| *u),
        );
        let Some(Resolved {
            place: tx, allowed, ..
        }) = found
        else {
            // Not a place of ours: no answer at all, so that a daemon on the
            // same bot that binds it can give one.
            return;
        };
        self.board.update(|s| s.interactions += 1);
        let who = user
            .as_ref()
            .map(|(_, n)| format!("discord:{n}"))
            .unwrap_or_else(|| "discord".into());
        if !allowed {
            self.respond(
                &i,
                InteractionResponseType::ChannelMessageWithSource,
                Some("Only the people this place is bound to can do that.".into()),
                true,
            )
            .await;
            return;
        }
        // Who pressed, and where: the ids the core judges an answer by, which
        // only this binding may name (theseus-sgh).
        let discord = match (&user, channel) {
            (Some((u, _)), Some(c)) => Some(DiscordOrigin {
                user_id: u.to_string(),
                channel_id: c.to_string(),
                guild_id: i.guild_id.map(|g| g.to_string()),
            }),
            _ => None,
        };
        match &i.data {
            Some(InteractionData::MessageComponent(c)) => {
                if let Some(asked) = parse_asked_pick(&c.custom_id, &c.values) {
                    self.should_have_asked(&i, asked, &who, discord).await;
                    return;
                }
                let Some(Press {
                    approve,
                    trust,
                    corr,
                }) = parse_confirm_id(&c.custom_id)
                else {
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
                // A guild channel `[approval]` lists is checked again as the
                // answer arrives; the core judges the answer against it.
                if let (Some(c), Some(_)) = (channel, i.guild_id) {
                    if self.core.approval.lists_discord_channel(c) {
                        self.check_channel(c).await;
                    }
                }
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
                            discord,
                            trust,
                        },
                    )
                    .await;
                if let Err(e) = &r {
                    if e.code == theseus_protocol::error_code::REFUSED {
                        // The answer did not count: the card keeps its
                        // buttons for one that does, and only the presser
                        // is told why.
                        self.core.binding_ledger(
                            LedgerKind::DiscordConfirm,
                            None,
                            json!({"correlation_id": corr, "approve": approve, "trust": trust, "by": who, "ok": false, "refused": true, "error": e.message}),
                        );
                        let why = e.data.get("why").and_then(Value::as_str);
                        self.followup(
                            &i,
                            &format!(
                                "🔐 Your answer did not count: {}. It keeps waiting for an \
                                 answer that does.",
                                why.unwrap_or(&e.message)
                            ),
                        )
                        .await;
                        return;
                    }
                }
                let line = i
                    .message
                    .as_ref()
                    .and_then(|m| m.content.lines().next())
                    .map(|l| {
                        l.trim_start_matches(crate::render::FLOOR_ASK)
                            .trim_start_matches(crate::render::BUDGET_ASK)
                            .trim_start_matches(crate::render::ASK)
                            .to_string()
                    })
                    .unwrap_or_default();
                let content = match &r {
                    Ok(_) if trust => {
                        format!("✅ **Approved** by {who}, and trusted the session again · {line}")
                    }
                    Ok(_) if approve => format!("✅ **Approved** by {who} · {line}"),
                    Ok(_) => format!("❎ **Declined** by {who} · {line}"),
                    Err(e) => format!("⚠️ Could not answer: {e} · {line}"),
                };
                self.core.binding_ledger(
                    LedgerKind::DiscordConfirm,
                    None,
                    json!({"correlation_id": corr, "approve": approve, "trust": trust, "by": who, "ok": r.is_ok(), "error": r.as_ref().err().map(|e| e.message.clone())}),
                );
                // An outbox card is settled by its settle post, which the
                // answer just wrote (theseus-q4v); a card from before the
                // outbox, or an answer that failed, is settled here.
                let settled_by_post = r.is_ok() && self.core.outbox.has_card(&corr);
                if let (Some(ch), Some(m), false) =
                    (i.channel.as_ref(), i.message.as_ref(), settled_by_post)
                {
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
                    "stop" => Control::Stop,
                    // `/cancel id:<id>` (DD8), a task's or a wake's; a
                    // client that still has DD7's `task:` option sends that.
                    "cancel" => Control::Cancel(c.options.iter().find_map(|o| {
                        match (&*o.name, &o.value) {
                            (
                                "id" | "task",
                                twilight_model::application::interaction::application_command::CommandOptionValue::String(s),
                            ) => Some(s.clone()),
                            _ => None,
                        }
                    })),
                    "new" => Control::New,
                    "tasks" => Control::Tasks,
                    "wakes" => Control::Wakes,
                    // As the presser (T1b), as a card's press is.
                    "trust" => Control::Trust(discord),
                    "publish" => Control::Publish(Box::new(PublishAsk::of(&c.options, discord))),
                    "join" => Control::Join(voice::join_option(&c.options), discord),
                    "leave" => Control::Leave,
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
                    LedgerKind::DiscordCommand,
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
                    .interaction(i.application_id)
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

    /// A "Should have asked…" pick, or the button on a notice card
    /// (theseus-sgh): the tool asks first from now on. The core tells every
    /// place, whose tool messages then say who tightened it and stop
    /// offering it; the presser alone hears how it went and how to undo it.
    async fn should_have_asked(
        &self,
        i: &Interaction,
        asked: Asked,
        who: &str,
        discord: Option<DiscordOrigin>,
    ) {
        self.respond(
            i,
            InteractionResponseType::DeferredUpdateMessage,
            None,
            false,
        )
        .await;
        let r = send_tighten(&self.rpc, &asked, who, discord).await;
        self.core.binding_ledger(
            LedgerKind::DiscordTighten,
            None,
            json!({"tool": asked.tool, "correlation_id": asked.correlation_id, "by": who,
                   "ok": r.is_ok(), "error": r.as_ref().err().map(|e| e.message.clone())}),
        );
        self.followup(i, &tightened_reply(&asked.tool, &r)).await;
    }

    /// An ephemeral follow-up to an interaction already acknowledged.
    async fn followup(&self, i: &Interaction, text: &str) {
        let none = AllowedMentions::default();
        if let Err(e) = self
            .http
            .interaction(i.application_id)
            .create_followup(&i.token)
            .content(text)
            .flags(MessageFlags::EPHEMERAL)
            .allowed_mentions(Some(&none))
            .await
        {
            self.board.error("interaction follow-up", None, e);
        }
    }

    /// A place's renderer: the `[discord]` notice setting.
    fn renderer(&self) -> Renderer {
        Renderer::new(self.notice_embeds)
    }

    /// Who may drive the bound channel `channel` (its `[[channel]] users`):
    /// the only people whose presses there count, whom its cards mention
    /// (theseus-9j9).
    pub(crate) fn place_users(&self, channel: u64) -> Vec<u64> {
        let r = self.routes.lock().unwrap();
        r.users.get(&channel).cloned().unwrap_or_default()
    }

    /// A place here renders this session's turns: its calls' tool lines are
    /// shown here, before their cards (theseus-50p).
    pub(crate) fn renders(&self, session_id: &str) -> bool {
        self.routes
            .lock()
            .unwrap()
            .by_session
            .contains_key(session_id)
    }

    /// The DM an approval card goes to when its place is not a trusted
    /// channel: the turn's author's, when they are a trusted user with an
    /// open DM here, else the first such DM in the bindings file.
    pub(crate) fn approval_dm(&self, prefer: Option<u64>) -> Option<(u64, String)> {
        let r = self.routes.lock().unwrap();
        let trusted: Vec<&(u64, String)> = r
            .dms
            .iter()
            .filter(|(u, _)| {
                r.dm_channel
                    .get(u)
                    .is_some_and(|c| self.core.approval.trusts_dm(*u, Some(*c)))
            })
            .collect();
        prefer
            .and_then(|p| trusted.iter().find(|(u, _)| *u == p))
            .or(trusted.first())
            .map(|d| (*d).clone())
    }

    /// Whether the portal has the Server Members intent on: the application's
    /// flags, asked on first need when `connect` has not asked yet.
    async fn members_intent(&self) -> bool {
        if let Some(m) = self.members_intent.get() {
            return *m;
        }
        match self.http.current_user_application().await {
            Ok(r) => match r.model().await {
                Ok(app) => {
                    let m = viewers::members_intent(app.flags);
                    let _ = self.members_intent.set(m);
                    self.board.update(|s| s.members_intent = Some(m));
                    m
                }
                Err(_) => false,
            },
            Err(_) => false,
        }
    }

    /// Check who can view a guild channel `[approval]` lists, and tell the
    /// core, which judges answers from there against it (theseus-sgh).
    /// Without the Server Members intent it cannot be verified. True when it
    /// is trusted.
    pub(crate) async fn check_channel(&self, channel: u64) -> bool {
        let intent = self.members_intent().await;
        let view = match intent {
            true => Some(self.view(channel).await),
            false => None,
        };
        let (trusted, detail) = match (viewers::unverifiable(intent), &view) {
            (Some(v), _) => v,
            (None, Some(Ok(v))) => {
                let trusted = self.core.approval.discord_users();
                let outside: Vec<&viewers::Member> = v
                    .viewers
                    .iter()
                    .filter(|m| !trusted.contains(&m.id))
                    .collect();
                viewers::verdict(&outside, v.checked)
            }
            (None, Some(Err(e))) => (false, format!("could not check who can view it: {e}")),
            (None, None) => (false, viewers::NO_INTENT.to_string()),
        };
        self.core.approval_checked(
            channel,
            Checked {
                trusted,
                detail,
                at_ms: theseus_protocol::now_unix_ms(),
            },
        );
        trusted
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
            .interaction(i.application_id)
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

/// What a card's button asks: approve, and whether to trust the session
/// again too (theseus-9bp), for one correlation id.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Press {
    approve: bool,
    trust: bool,
    corr: String,
}

/// `confirm:approve:<corr>`, `confirm:trust:<corr>` (approve, and trust the
/// session again), or `confirm:decline:<corr>`.
fn parse_confirm_id(id: &str) -> Option<Press> {
    let rest = id.strip_prefix("confirm:")?;
    let (verb, corr) = rest.split_once(':')?;
    let (approve, trust) = match verb {
        "approve" => (true, false),
        "trust" => (true, true),
        "decline" => (false, false),
        _ => return None,
    };
    Some(Press {
        approve,
        trust,
        corr: corr.to_string(),
    })
}

/// A notification every place gets, whatever session it names.
fn everywhere(e: &CoreEvent) -> bool {
    matches!(
        e,
        CoreEvent::PolicyTightened(_) | CoreEvent::PolicyUntightened(_)
    )
}

/// The select menu's custom id; a notice card's button is `tighten:<value>`.
const ASKED_MENU: &str = "tighten";

/// Discord's limit on a custom id and on an option's value.
const ID_LIMIT: usize = 100;

/// A "should have asked" choice as Discord carries it (theseus-sgh):
/// `<tool>|<correlation id>`, or the tool alone when there is no call or the
/// card button's id for the pair, the longer of the two, would pass the limit.
fn asked_value(a: &Asked) -> String {
    match &a.correlation_id {
        Some(c) if format!("{ASKED_MENU}:{}|{c}", a.tool).len() <= ID_LIMIT => {
            format!("{}|{c}", a.tool)
        }
        _ => a.tool.clone(),
    }
}

fn parse_asked(v: &str) -> Option<Asked> {
    let (tool, corr) = match v.split_once('|') {
        Some((t, c)) => (t, Some(c.to_string()).filter(|c| !c.is_empty())),
        None => (v, None),
    };
    (!tool.is_empty()).then(|| Asked {
        tool: tool.to_string(),
        correlation_id: corr,
    })
}

/// A component interaction that is a "should have asked" press: the select
/// menu's pick, or a notice card's button.
fn parse_asked_pick(custom_id: &str, values: &[String]) -> Option<Asked> {
    if custom_id == ASKED_MENU {
        return values.first().and_then(|v| parse_asked(v));
    }
    parse_asked(custom_id.strip_prefix(ASKED_MENU)?.strip_prefix(':')?)
}

/// What a "should have asked" press says it does, on the menu's options and a
/// notice card's button (Eddie, 2026-10-03 14:53).
const ASK_IN_FUTURE: &str = "Make actions like this ask in the future";

/// The one "Should I have asked?" menu on a tool message: an option per
/// distinct notified tool (the renderer caps them at 25). Every option says
/// the same words, so its description names what the press does: it tightens
/// the whole tool.
pub(crate) fn asked_menu(options: &[Asked]) -> Vec<Component> {
    let options = options
        .iter()
        .take(crate::render::MAX_ASKED)
        .map(|a| SelectMenuOption {
            default: false,
            description: Some(format!("Every {} call asks you first", a.tool)),
            emoji: None,
            label: ASK_IN_FUTURE.into(),
            value: asked_value(a),
        })
        .collect();
    vec![Component::ActionRow(ActionRow {
        id: None,
        components: vec![Component::SelectMenu(SelectMenu {
            id: None,
            channel_types: None,
            custom_id: ASKED_MENU.into(),
            default_values: None,
            disabled: false,
            kind: SelectMenuType::Text,
            max_values: Some(1),
            min_values: Some(1),
            options: Some(options),
            placeholder: Some("Should I have asked?".into()),
            required: None,
        })],
    })]
}

/// A notice card's "Make actions like this ask in the future" button (with
/// `[discord] notice_embeds`), or none once its tool asks first.
pub(crate) fn asked_button(ask: Option<&Asked>) -> Vec<Component> {
    let Some(a) = ask else {
        return vec![];
    };
    vec![Component::ActionRow(ActionRow {
        id: None,
        components: vec![Component::Button(Button {
            id: None,
            custom_id: Some(format!("{ASKED_MENU}:{}", asked_value(a))),
            disabled: false,
            emoji: None,
            label: Some(ASK_IN_FUTURE.into()),
            style: ButtonStyle::Secondary,
            url: None,
            sku_id: None,
        })],
    })]
}

/// A "should have asked" press, sent to the core as `policy.tighten` with
/// who pressed and where.
async fn send_tighten(
    rpc: &RpcClient,
    asked: &Asked,
    who: &str,
    discord: Option<DiscordOrigin>,
) -> Result<TightenResult, CallError> {
    rpc.call(
        theseus_protocol::method::POLICY_TIGHTEN,
        PolicyTightenParams {
            tool: asked.tool.clone(),
            correlation_id: asked.correlation_id.clone(),
            author: Some(who.to_string()),
            discord,
        },
    )
    .await
}

/// What the presser alone is told after a "should have asked" press.
fn tightened_reply(tool: &str, r: &Result<TightenResult, CallError>) -> String {
    let undo =
        format!("Undo it in the web UI's Tools view, or with `theseus policy untighten {tool}`.");
    match r {
        Ok(t) if t.already => format!(
            "🔒 `{tool}` already asks first: tightened by {}.",
            t.tightening.by
        ),
        Ok(t) if !t.changed => format!(
            "🔒 `{tool}` already asks ({}), and now keeps asking if the config changes. {undo}",
            t.config_setting
        ),
        Ok(_) => format!("🔒 `{tool}` asks first from now on. {undo}"),
        Err(e) if e.code == theseus_protocol::error_code::REFUSED => format!(
            "🔐 Your press did not count: {}.",
            e.data
                .get("why")
                .and_then(Value::as_str)
                .unwrap_or(&e.message)
        ),
        Err(e) => format!("⚠️ Could not tighten `{tool}`: {}", e.message),
    }
}

/// Approve and Decline, and with `trust` (theseus-9bp) "Approve + trust
/// session" between them, for a call that waits because its session read
/// external text.
pub(crate) fn confirm_buttons(corr: &str, trust: bool) -> Vec<Component> {
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
    let mut components = vec![button("approve", "Approve", ButtonStyle::Success)];
    if trust {
        components.push(button(
            "trust",
            "Approve + trust session",
            ButtonStyle::Primary,
        ));
    }
    components.push(button("decline", "Decline", ButtonStyle::Danger));
    vec![Component::ActionRow(ActionRow {
        id: None,
        components,
    })]
}

/// Hand each notification to the place whose session it names. A tightening
/// holds for every session, so every place gets it (theseus-sgh). A refused
/// answer from a job's process is the core's post to the operator
/// (theseus-q4v), so no place renders it.
async fn route(shared: Arc<Shared>, mut notes: mpsc::UnboundedReceiver<Notification>) {
    while let Some(n) = notes.recv().await {
        // The binding runs with its core, so every notification is one this
        // build reads.
        let Ok(Some(e)) = CoreEvent::from_notification(&n.method, &n.params) else {
            continue;
        };
        if matches!(e, CoreEvent::ApprovalRefused(_)) {
            continue;
        }
        if everywhere(&e) {
            let places: Vec<mpsc::UnboundedSender<PlaceMsg>> = shared
                .routes
                .lock()
                .unwrap()
                .by_session
                .values()
                .cloned()
                .collect();
            for tx in places {
                let _ = tx.send(PlaceMsg::Event(Box::new(e.clone())));
            }
            continue;
        }
        let target = {
            let mut r = shared.routes.lock().unwrap();
            let sid = e.session_id().map(str::to_string);
            let turn = e.turn_id();
            if let (Some(sid), Some(turn)) = (&sid, turn) {
                if matches!(e, CoreEvent::TurnStarted(_)) {
                    r.turns.insert(turn.to_string(), sid.clone());
                }
            }
            let sid = sid.or_else(|| turn.and_then(|t| r.turns.get(t).cloned()));
            if matches!(e, CoreEvent::TurnEnded(_)) {
                if let Some(t) = turn {
                    r.turns.remove(t);
                }
            }
            sid.and_then(|s| r.by_session.get(&s).cloned())
        };
        if let Some(tx) = target {
            let _ = tx.send(PlaceMsg::Event(Box::new(e)));
        }
    }
}

struct Place {
    shared: Arc<Shared>,
    key: String,
    /// Where its posts go: `discord:<key>`.
    target: String,
    kind: &'static str,
    label: String,
    channel: Option<Id<ChannelMarker>>,
    users: Vec<u64>,
    mention_only: bool,
    session_id: String,
    renderer: Renderer,
    /// The place's lane: the one writer of its messages (theseus-q4v).
    lane: mpsc::UnboundedSender<LaneMsg>,
    /// A `turn.submit` of ours is outstanding.
    inflight: bool,
    /// Messages that arrived mid-turn, for the next turn.
    queued: Vec<Inbound>,
    saw_failure: bool,
    /// The turn a `/stop` stopped (W1): its stream and new calls show no
    /// more here, until its end.
    stopped_turn: Option<String>,
    /// A `/stop` came while our `turn.submit` was outstanding: its end says
    /// nothing more, since the stop's answer did.
    stopping: bool,
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
                    self.apply(ops);
                }
                _ = typing.tick() => {
                    if (self.inflight && !self.stopping) || self.renderer.busy() {
                        self.apply(vec![crate::render::Op::Typing]);
                    }
                }
            }
        }
    }

    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
    async fn handle(&mut self, m: PlaceMsg) {
        match m {
            PlaceMsg::Inbound(m) => {
                self.last_activity_ms = theseus_protocol::now_unix_ms();
                let _ = self.lane.send(LaneMsg::Author(m.author_id));
                let mut row = json!({"place": self.label, "author": m.author, "chars": m.text.chars().count(), "message_id": m.message.to_string()});
                if let Some(p) = &m.files {
                    row["attachments"] = json!(p.metas.len());
                }
                self.shared.core.binding_ledger(
                    LedgerKind::DiscordMessageIn,
                    Some(&self.session_id),
                    row,
                );
                if let Some(cmd) = parse_control(&m.text) {
                    let cmd = match cmd {
                        Control::Trust(None) => Control::Trust(Some(m.origin())),
                        cmd => cmd,
                    };
                    let reply = self.control(cmd, &format!("discord:{}", m.author)).await;
                    self.say(&reply, Some(m.message));
                    return;
                }
                if self.inflight {
                    self.queued.push(m);
                } else {
                    self.submit(vec![m]);
                }
                self.report();
            }
            PlaceMsg::Event(e) => {
                let e = *e;
                if matches!(e, CoreEvent::TurnFailed(_)) {
                    self.saw_failure = true;
                }
                // The turn a `/stop` stopped (W1) streams no more here, and
                // what it proposes will not run; its tool messages still
                // take their last state, and its end clears it.
                self.voice_heard(&e);
                let turn = e.turn_id();
                if turn.is_some() && turn == self.stopped_turn.as_deref() {
                    match e {
                        CoreEvent::ModelDelta(_)
                        | CoreEvent::ModelThinking(_)
                        | CoreEvent::ToolProposed(_) => return,
                        CoreEvent::TurnEnded(_) | CoreEvent::TurnFailed(_) => {
                            self.stopped_turn = None
                        }
                        _ => {}
                    }
                }
                let ops = self.renderer.on_event(&e);
                self.apply(ops);
                // The renderer has shown the call that asks, and its tool line
                // went to the lane first: the question's card may follow
                // (theseus-50p).
                if let CoreEvent::ConfirmRequested(r) = &e {
                    let _ = self.lane.send(LaneMsg::Asked(r.correlation_id.clone()));
                }
            }
            PlaceMsg::SubmitDone(r) => {
                self.inflight = false;
                // The stop's answer said what stopped (W1).
                let stopping = std::mem::take(&mut self.stopping);
                let r = if stopping { Ok(()) } else { r };
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
                                self.say(&note, None);
                                let batch = std::mem::take(&mut self.queued);
                                if !batch.is_empty() {
                                    self.submit(batch);
                                }
                                return;
                            }
                            Err(err) => self.shared.board.error("rebind", None, err),
                        }
                    } else if !self.saw_failure {
                        self.say(&format!("⚠️ {}", e.message), None);
                    }
                }
                let batch = std::mem::take(&mut self.queued);
                if !batch.is_empty() {
                    self.submit(batch);
                }
                self.voice_next();
                self.report();
            }
            PlaceMsg::Voice(t) => self.voice_turn(t),
            PlaceMsg::Control { cmd, by, reply } => {
                let text = self.control(cmd, &by).await;
                let _ = reply.send(text);
            }
            PlaceMsg::DmChannel(c) => {
                if self.channel != Some(c) {
                    self.channel = Some(c);
                    let _ = self.lane.send(LaneMsg::Channel(c.get()));
                    let mut r = self.shared.routes.lock().unwrap();
                    r.by_channel.insert(c.get(), self.tx.clone());
                    r.users.insert(c.get(), self.users.clone());
                    r.dm_channel.insert(self.users[0], c.get());
                    drop(r);
                    self.report();
                }
            }
        }
    }

    /// Start a turn with these messages (one, or several coalesced, authors
    /// kept). Their attachments are awaited in the turn's own task, in order.
    fn submit(&mut self, batch: Vec<Inbound>) {
        let one_author = batch.iter().all(|m| m.author == batch[0].author);
        let texts = batch.iter().filter(|m| !m.text.trim().is_empty());
        let input = if batch.len() == 1 {
            batch[0].text.clone()
        } else if one_author {
            texts
                .map(|m| m.text.as_str())
                .collect::<Vec<_>>()
                .join("\n\n")
        } else {
            texts
                .map(|m| format!("[{}] {}", m.author, m.text))
                .collect::<Vec<_>>()
                .join("\n\n")
        };
        let author = if one_author {
            format!("discord:{}", batch[0].author)
        } else {
            "discord".into()
        };
        // The turn's first message replies to the last one it answers: the
        // stream's, or the reply's post when the stream posted nothing.
        let anchor = batch.last().map(|m| m.message);
        if let Some(a) = anchor {
            let _ = self.lane.send(LaneMsg::Anchor(a.get()));
        }
        self.inflight = true;
        self.saw_failure = false;
        let pending: Vec<Pending> = batch.into_iter().filter_map(|m| m.files).collect();
        let (rpc, tx, sid) = (
            self.shared.rpc.clone(),
            self.tx.clone(),
            self.session_id.clone(),
        );
        tokio::spawn(async move {
            let mut attachments = Vec::new();
            for p in pending {
                attachments.extend(p.wait().await);
            }
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
                        attachments,
                        reply_to: anchor.map(|a| a.to_string()),
                        opened_from: None,
                    },
                )
                .await
                .map(|_| ());
            let _ = tx.send(PlaceMsg::SubmitDone(r));
        });
    }

    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
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
                // W1: halt this session's work and keep the conversation. The
                // messages queued for the next turn go too; the next message
                // continues this session. Its tasks and wakes are untouched.
                self.queued.clear();
                self.stopping = self.inflight;
                // The stopped turn's stream stops here, where Discord last
                // saw it; it posts no reply.
                if let Some(turn) = self.renderer.running_turn() {
                    let ops = self.renderer.stop(&turn);
                    self.apply(ops);
                    self.stopped_turn = Some(turn);
                }
                let Some(eid) = self
                    .shared
                    .session_info(&self.session_id)
                    .await
                    .and_then(|s| s.execution_id)
                else {
                    return "Nothing to stop: this session has taken no turn yet.".into();
                };
                match self
                    .shared
                    .rpc
                    .call::<_, theseus_protocol::ExecutionStopResult>(
                        theseus_protocol::method::EXECUTION_STOP,
                        theseus_protocol::ExecutionStopParams {
                            execution_id: eid,
                            author: Some(by.to_string()),
                        },
                    )
                    .await
                {
                    Ok(r) => stop_answer(&r),
                    Err(e) => format!("⚠️ Could not stop: {e}."),
                }
            }
            Control::Tasks => {
                // The tasks that report here (DD7), the newest first.
                match self
                    .shared
                    .rpc
                    .call::<_, theseus_protocol::TaskListResult>(
                        theseus_protocol::method::TASK_LIST,
                        theseus_protocol::TaskListParams {
                            session_id: None,
                            target: Some(self.target.clone()),
                        },
                    )
                    .await
                {
                    Ok(l) => crate::render::tasks(&l.tasks, theseus_protocol::now_unix_ms()),
                    Err(e) => format!("⚠️ Could not list the tasks: {e}."),
                }
            }
            Control::Wakes => {
                // The wakes whose turns post here (DD8), the soonest first.
                match self
                    .shared
                    .rpc
                    .call::<_, theseus_protocol::WakeListResult>(
                        theseus_protocol::method::WAKE_LIST,
                        theseus_protocol::WakeListParams {
                            session_id: None,
                            target: Some(self.target.clone()),
                        },
                    )
                    .await
                {
                    Ok(l) => crate::render::wakes(&l.wakes),
                    Err(e) => format!("⚠️ Could not list the wakes: {e}."),
                }
            }
            Control::Cancel(None) => "Which one? `/cancel <id>` stops a task or cancels a wake, \
                                      by the id `/tasks` or `/wakes` shows. `/stop` halts this \
                                      session's own work and keeps the conversation."
                .to_string(),
            Control::Cancel(Some(name)) => {
                // A pending wake first (DD8), then a task; the core refuses a
                // name that means both.
                let no_wake = match self
                    .shared
                    .rpc
                    .call::<_, theseus_protocol::WakeCancelResult>(
                        theseus_protocol::method::WAKE_CANCEL,
                        theseus_protocol::WakeCancelParams {
                            wake: name.clone(),
                            author: Some(by.to_string()),
                        },
                    )
                    .await
                {
                    Ok(r) => {
                        return format!(
                            "⏹️ Wake `{}` cancelled: it will not run. It was due <t:{}:t>: {}",
                            r.wake.short,
                            r.wake.due_at_ms / 1000,
                            crate::render::clip(r.wake.note.lines().next().unwrap_or_default(), 200)
                        )
                    }
                    Err(e) if e.code == theseus_protocol::error_code::NOT_FOUND => e.message,
                    Err(e) => return format!("⚠️ Could not cancel: {e}."),
                };
                self.cancel_task(name, by, &no_wake).await
            }
            Control::Trust(origin) => self.trust(origin, by).await,
            Control::Publish(ask) => self.publish(*ask, by).await,
            Control::Join(named, origin) => self.join(named, origin, by).await,
            Control::Leave => self.leave(by).await,
        }
    }

    /// `/trust` (T1b): this place's session no longer holds web text. It
    /// goes through `policy.trust` as the presser, so the core judges it as a
    /// card's press (`[approval]`, and `approval.refused` when it does not
    /// count). A session that holds nothing is told so, and nothing is
    /// written.
    async fn trust(&self, origin: Option<DiscordOrigin>, by: &str) -> String {
        let held = self
            .shared
            .session_info(&self.session_id)
            .await
            .and_then(|s| s.external_text);
        if held.is_none() {
            return "This conversation holds no web text, so there is nothing to trust.".into();
        }
        // A guild channel `[approval]` lists is checked again, as for a press.
        let listed = origin
            .as_ref()
            .filter(|o| o.guild_id.is_some())
            .and_then(|o| o.channel_id.parse::<u64>().ok())
            .filter(|c| self.shared.core.approval.lists_discord_channel(*c));
        if let Some(c) = listed {
            self.shared.check_channel(c).await;
        }
        let r = self
            .shared
            .rpc
            .call::<_, TrustResult>(
                theseus_protocol::method::POLICY_TRUST,
                theseus_protocol::PolicyTrustParams {
                    session_id: self.session_id.clone(),
                    author: Some(by.to_string()),
                    discord: origin,
                },
            )
            .await;
        trust_answer(&r)
    }

    /// `/cancel <id>` for a task (DD7), once no wake had the name.
    async fn cancel_task(&self, task: String, by: &str, no_wake: &str) -> String {
        match self
            .shared
            .rpc
            .call::<_, theseus_protocol::TaskCancelResult>(
                theseus_protocol::method::TASK_CANCEL,
                theseus_protocol::TaskCancelParams {
                    task,
                    author: Some(by.to_string()),
                },
            )
            .await
        {
            Ok(r) if r.cancelled_actions.is_empty() && r.task.state != "cancelled" => format!(
                "Task `{}` had already ended ({}).",
                r.task.short, r.task.state
            ),
            Ok(r) => format!(
                "⏹️ Task `{}` stopped ({} running action(s) told to stop). Its report says so here.",
                r.task.short,
                r.cancelled_actions.len()
            ),
            Err(e) if e.code == theseus_protocol::error_code::NOT_FOUND => {
                format!("⚠️ Nothing to cancel: {e}, and {no_wake}.")
            }
            Err(e) => format!("⚠️ Could not cancel: {e}."),
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
        self.renderer = self.shared.renderer();
        self.report();
        Ok(())
    }

    /// Live progress to the place's lane: best-effort, never replayed.
    fn apply(&mut self, ops: Vec<crate::render::Op>) {
        for op in ops {
            let _ = self.lane.send(LaneMsg::Live(op));
        }
    }

    /// Something the place must say that is no turn's (a bind notice, a
    /// control's answer, a rebind): an outbox post, delivered when Discord
    /// can take it (theseus-q4v).
    fn say(&self, text: &str, reply_to: Option<Id<MessageMarker>>) {
        let body =
            json!({"kind": "notice", "text": text, "reply_to": reply_to.map(|m| m.to_string())});
        if let Err(e) = self
            .shared
            .core
            .outbox
            .post(&self.session_id, "", &self.target, body)
        {
            self.shared
                .board
                .error("post notice", Some(&self.session_id), format!("{e:#}"));
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

/// What a lane needs, for a test that drives one directly: the core's
/// `[discord]` REST (a fake's), and nothing started.
#[cfg(test)]
pub(crate) fn shared_for_tests(core: &Arc<Core>) -> Arc<Shared> {
    let (rpc, _notes) = RpcClient::connect(core.clone(), Client::new(CLIENT, Surface::Discord));
    Arc::new(Shared {
        core: core.clone(),
        rpc,
        http: Arc::new(http_client("fake-token-not-a-secret", &core.cfg.discord)),
        board: Board::new(core.clone()),
        bot_id: AtomicU64::new(0),
        edit_interval: Duration::from_millis(250),
        notice_embeds: false,
        routes: Mutex::new(Routes::default()),
        files_http: files::client(),
        max_text: 0,
        members_intent: OnceLock::new(),
        lanes: Mutex::new(HashMap::new()),
        voice: voice::Voice::none(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_slash_commands_list_tasks_and_wakes_and_cancel_one_by_name() {
        let cmds = commands();
        let names: Vec<&str> = cmds.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            names[..8],
            ["stop", "new", "status", "tasks", "wakes", "trust", "cancel", "publish"]
        );
        // One required option names a task or a wake (DD8; DD7 called it `task`).
        let cancel = cmds.iter().find(|c| c.name == "cancel").unwrap();
        assert_eq!(cancel.options.len(), 1);
        assert_eq!(cancel.options[0].name, "id");
        assert_eq!(cancel.options[0].required, Some(true));
        assert!(cancel.description.contains("cancel a wake"));
        // One command, one effect (W1): `/stop` halts and keeps the
        // conversation; `/new` alone starts fresh.
        let desc = |n: &str| {
            cmds.iter()
                .find(|c| c.name == n)
                .unwrap()
                .description
                .clone()
        };
        assert_eq!(
            desc("stop"),
            "Stop what Theseus is doing here: its turn, jobs, and queued messages. The \
             conversation goes on"
        );
        assert_eq!(
            desc("new"),
            "Start a fresh session here; the old one stays in the web UI"
        );
        // `/trust` (T1b): its one effect.
        assert_eq!(
            desc("trust"),
            "Trust this conversation again after it read web text: its calls that change things \
             stop waiting"
        );
        assert!(cmds
            .iter()
            .find(|c| c.name == "trust")
            .unwrap()
            .options
            .is_empty());
        // Discord refuses a description over 100 characters.
        assert!(cmds.iter().all(|c| c.description.chars().count() <= 100));
    }

    /// What a bound place says first, and what `/stop` answers (W1).
    #[test]
    fn the_bind_notice_and_the_stops_answer_name_each_control_with_its_one_effect() {
        assert_eq!(
            bind_notice("ses_1", false),
            "🔗 Theseus is bound here (session `ses_1`). Talk to me in this place. `/stop` halts \
             what I am doing and keeps the conversation, `/new` starts a fresh one, `/trust` \
             trusts the conversation again after it reads web text, and `/status`, `/tasks` and \
             `/wakes` show this place's; everything shows in the web UI."
        );
        assert!(bind_notice("ses_1", true).contains(". @mention me or reply"));
        let exec = theseus_protocol::ExecutionInfo {
            execution_id: "exe_1".into(),
            session_id: "ses_1".into(),
            kind: "conversation".into(),
            state: "waiting".into(),
            turns: 2,
            interrupted: 0,
            outstanding: 0,
            queued_results: 0,
            budget: Default::default(),
            wake: Value::Null,
            waiting_on: None,
            reports_to: None,
            ended_reason: None,
            created_at_ms: 0,
            updated_at_ms: 0,
            attention: None,
        };
        let mut r = theseus_protocol::ExecutionStopResult {
            execution: exec,
            stopped: true,
            stopped_actions: vec!["act_1".into()],
            verdicts: vec![],
            declined: vec![],
            turn_running: true,
            tasks_running: 0,
            wakes_pending: 0,
        };
        assert_eq!(
            stop_answer(&r),
            "⏹️ Stopped this session's work (1 running action(s) told to stop). The conversation \
             goes on; `/new` starts a fresh one."
        );
        (r.tasks_running, r.wakes_pending) = (2, 1);
        assert!(stop_answer(&r).ends_with(
            " Its 2 task(s) and 1 wake(s) go on: `/tasks` and `/wakes` list them, and \
             `/cancel <id>` stops one."
        ));
        r.stopped = false;
        r.execution.state = "cancelled".into();
        assert_eq!(
            stop_answer(&r),
            "Nothing to stop: this session's execution has ended (cancelled). `/new` starts a \
             fresh one."
        );
    }

    #[test]
    fn controls_and_confirm_ids_parse() {
        assert_eq!(parse_control("/stop"), Some(Control::Stop));
        // `/cancel` names a task now (DD7); alone, it asks which.
        assert_eq!(
            parse_control("  /cancel a1b2c3"),
            Some(Control::Cancel(Some("a1b2c3".into())))
        );
        assert_eq!(parse_control("/cancel"), Some(Control::Cancel(None)));
        assert_eq!(parse_control("/tasks"), Some(Control::Tasks));
        assert_eq!(parse_control("/wakes"), Some(Control::Wakes));
        assert_eq!(parse_control("/new"), Some(Control::New));
        assert_eq!(parse_control("/status"), Some(Control::Status));
        // A typed `/trust` (T1b): the place fills in who typed it, and where.
        assert_eq!(parse_control("/trust"), Some(Control::Trust(None)));
        assert_eq!(parse_control("/trusted"), None);
        assert_eq!(parse_control("please /stop"), None);
        assert_eq!(parse_control("/stopper"), None);
        let press = |approve, trust, corr: &str| Press {
            approve,
            trust,
            corr: corr.into(),
        };
        assert_eq!(
            parse_confirm_id("confirm:approve:act_1"),
            Some(press(true, false, "act_1"))
        );
        assert_eq!(
            parse_confirm_id("confirm:decline:act_2"),
            Some(press(false, false, "act_2"))
        );
        // "Approve + trust session" (theseus-9bp): an approval that trusts
        // the session again as well.
        assert_eq!(
            parse_confirm_id("confirm:trust:act_3"),
            Some(press(true, true, "act_3"))
        );
        assert_eq!(parse_confirm_id("confirm:maybe:x"), None);
        assert_eq!(parse_confirm_id("other"), None);
        assert!(terminal(Some("cancelled")));
        assert!(!terminal(Some("waiting")));
    }

    #[test]
    fn mention_only_channels_answer_mentions_and_replies_only() {
        let (bot, role) = (1618033988749894848u64, 42u64);
        assert!(addressed("hi", &[bot], &[], None, bot, &[role]));
        assert!(addressed(
            "<@1618033988749894848> hi",
            &[],
            &[],
            None,
            bot,
            &[role]
        ));
        assert!(addressed(
            "<@!1618033988749894848> hi",
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
            strip_mentions("<@1618033988749894848>  run the tests", bot, &[role]),
            "run the tests"
        );
        assert_eq!(strip_mentions("hey <@&42> look", bot, &[role]), "hey look");
    }

    #[test]
    fn confirm_buttons_carry_the_correlation_id() {
        let ids = |trust| {
            let c = confirm_buttons("act_9", trust);
            let Component::ActionRow(row) = &c[0] else {
                panic!()
            };
            row.components
                .iter()
                .map(|b| match b {
                    Component::Button(b) => {
                        (b.custom_id.clone().unwrap(), b.label.clone().unwrap())
                    }
                    _ => panic!(),
                })
                .collect::<Vec<_>>()
        };
        let plain: Vec<String> = ids(false).into_iter().map(|(id, _)| id).collect();
        assert_eq!(
            plain,
            vec!["confirm:approve:act_9", "confirm:decline:act_9"]
        );
        // A call that waits because its session read external text
        // (theseus-9bp): the third button trusts the session again.
        assert_eq!(
            ids(true),
            vec![
                ("confirm:approve:act_9".to_string(), "Approve".to_string()),
                (
                    "confirm:trust:act_9".to_string(),
                    "Approve + trust session".to_string()
                ),
                ("confirm:decline:act_9".to_string(), "Decline".to_string()),
            ]
        );
    }

    /// A "should have asked" choice carries the tool and its call, in the
    /// menu's option value or the card button's custom id, within Discord's
    /// 100 characters (theseus-sgh).
    #[test]
    fn a_should_have_asked_value_carries_the_tool_and_its_call() {
        let a = Asked {
            tool: "proc.run".into(),
            correlation_id: Some("act_019".into()),
        };
        assert_eq!(asked_value(&a), "proc.run|act_019");
        let tool_only = Asked {
            tool: "fs.write".into(),
            correlation_id: None,
        };
        for (id, values, want) in [
            ("tighten", vec!["proc.run|act_019"], Some(a.clone())),
            ("tighten:proc.run|act_019", vec![], Some(a.clone())),
            ("tighten", vec!["fs.write"], Some(tool_only.clone())),
            ("tighten", vec!["fs.write|"], Some(tool_only.clone())),
            ("tighten", vec![], None),
            ("tightening", vec!["x"], None),
            ("confirm:approve:act_1", vec![], None),
        ] {
            let values: Vec<String> = values.into_iter().map(String::from).collect();
            assert_eq!(parse_asked_pick(id, &values), want, "{id} {values:?}");
        }
        let long = Asked {
            tool: format!("mcp:{}/tool", "s".repeat(60)),
            correlation_id: Some(format!("act_{}", "0".repeat(32))),
        };
        assert_eq!(
            asked_value(&long),
            long.tool,
            "past 100 characters, the tool alone"
        );

        let menu = asked_menu(&[a.clone(), tool_only]);
        let Component::ActionRow(row) = &menu[0] else {
            panic!()
        };
        let Component::SelectMenu(m) = &row.components[0] else {
            panic!()
        };
        assert_eq!(m.custom_id, "tighten");
        assert_eq!(m.placeholder.as_deref(), Some("Should I have asked?"));
        assert_eq!((m.min_values, m.max_values), (Some(1), Some(1)));
        let opts: Vec<(&str, Option<&str>, &str)> = m
            .options
            .as_ref()
            .unwrap()
            .iter()
            .map(|o| (o.label.as_str(), o.description.as_deref(), o.value.as_str()))
            .collect();
        assert_eq!(
            opts,
            [
                (
                    "Make actions like this ask in the future",
                    Some("Every proc.run call asks you first"),
                    "proc.run|act_019"
                ),
                (
                    "Make actions like this ask in the future",
                    Some("Every fs.write call asks you first"),
                    "fs.write"
                )
            ]
        );
        assert!(asked_button(None).is_empty());
        let b = asked_button(Some(&a));
        let Component::ActionRow(row) = &b[0] else {
            panic!()
        };
        let Component::Button(b) = &row.components[0] else {
            panic!()
        };
        assert_eq!(b.custom_id.as_deref(), Some("tighten:proc.run|act_019"));
        assert_eq!(
            b.label.as_deref(),
            Some("Make actions like this ask in the future")
        );
        assert!(
            everywhere(&CoreEvent::PolicyTightened(Default::default()))
                && everywhere(&CoreEvent::PolicyUntightened(Default::default()))
        );
        assert!(!everywhere(&CoreEvent::PolicyNotified(Default::default())));
    }

    /// A core over a scratch store, with the template's tools and posture,
    /// and a provider that is never called.
    fn core_for_tests(dir: &std::path::Path) -> Arc<Core> {
        core_with_secrets(dir, theseus_core::secrets::SecretBoard::empty())
    }

    fn core_with_secrets(
        dir: &std::path::Path,
        secrets: Arc<theseus_core::secrets::SecretBoard>,
    ) -> Arc<Core> {
        core_with(dir, secrets, |_| {})
    }

    /// `core_with_secrets`, with `tweak` applied to the config first.
    pub(super) fn core_with(
        dir: &std::path::Path,
        secrets: Arc<theseus_core::secrets::SecretBoard>,
        tweak: impl FnOnce(&mut theseus_core::Config),
    ) -> Arc<Core> {
        let mut cfg = theseus_core::Config::example();
        cfg.server.state_dir = dir.to_string_lossy().into_owned();
        let work = dir.join("work");
        std::fs::create_dir_all(&work).unwrap();
        cfg.tools.projects_dir = Some(work.to_string_lossy().into_owned());
        cfg.tools.roots = vec![];
        tweak(&mut cfg);
        let store = theseus_core::store::Store::open(&dir.join("store")).unwrap();
        let fake: Arc<dyn theseus_core::provider::Provider> =
            Arc::new(theseus_core::provider::FakeProvider::scripted(vec![]));
        let providers = [(cfg.model.provider.clone(), fake)].into_iter().collect();
        Core::build(theseus_core::rpc::Parts {
            cfg,
            providers,
            store,
            secrets,
            startup_log: Arc::default(),
            telemetry: Some(theseus_core::telemetry::Telemetry::disabled()),
            scrubber: Arc::new(theseus_core::scrub::Scrubber::default()),
            launcher: Arc::new(theseus_core::toolrun::InlineLauncher),
            config_gate: theseus_core::config_gate::ConfigGate::file("test"),
            toollets: vec![],
            cpu_cores: None,
        })
        .unwrap()
    }

    /// A core whose model answers from `script`, whose tools ask first, and
    /// whose Discord REST points at a port nothing listens on.
    pub(super) fn core_scripted(
        dir: &std::path::Path,
        script: Vec<theseus_core::provider::Scripted>,
    ) -> Arc<Core> {
        let mut cfg = theseus_core::Config::example();
        cfg.server.state_dir = dir.to_string_lossy().into_owned();
        let work = dir.join("work");
        std::fs::create_dir_all(&work).unwrap();
        cfg.tools.projects_dir = Some(work.to_string_lossy().into_owned());
        cfg.tools.roots = vec![];
        cfg.policy.enforcement = theseus_core::policy::Posture::Approve;
        cfg.discord.rest_proxy = Some("127.0.0.1:9".into());
        cfg.discord.gateway_proxy = Some("ws://127.0.0.1:9".into());
        let store = theseus_core::store::Store::open(&dir.join("store")).unwrap();
        let fake: Arc<dyn theseus_core::provider::Provider> =
            Arc::new(theseus_core::provider::FakeProvider::scripted(script));
        let providers = [(cfg.model.provider.clone(), fake)].into_iter().collect();
        Core::build(theseus_core::rpc::Parts {
            cfg,
            providers,
            store,
            secrets: theseus_core::secrets::SecretBoard::empty(),
            startup_log: Arc::default(),
            telemetry: Some(theseus_core::telemetry::Telemetry::disabled()),
            scrubber: Arc::new(theseus_core::scrub::Scrubber::default()),
            launcher: Arc::new(theseus_core::toolrun::InlineLauncher),
            config_gate: theseus_core::config_gate::ConfigGate::file("test"),
            toollets: vec![],
            cpu_cores: None,
        })
        .unwrap()
    }

    /// A DM place on `core`'s session `sid`, driven directly: the gateway is
    /// not faked, so a test calls its handlers. Its mailbox comes back with it.
    pub(super) fn place_for_tests(
        core: &Arc<Core>,
        sid: &str,
    ) -> (Place, mpsc::UnboundedReceiver<PlaceMsg>) {
        let shared = shared_for_tests(core);
        let (tx, rx) = mpsc::unbounded_channel();
        let (lane, _) = mpsc::unbounded_channel();
        shared
            .routes
            .lock()
            .unwrap()
            .by_session
            .insert(sid.to_string(), tx.clone());
        let place = Place {
            renderer: shared.renderer(),
            shared,
            key: "dm:42".into(),
            target: "discord:dm:42".into(),
            kind: "dm",
            label: "eddie".into(),
            channel: None,
            users: vec![42],
            mention_only: false,
            session_id: sid.to_string(),
            lane,
            inflight: false,
            queued: Vec::new(),
            saw_failure: false,
            stopped_turn: None,
            stopping: false,
            last_activity_ms: 0,
            tx,
        };
        (place, rx)
    }

    /// What the binding logs at info or louder (theseus-j7xi): a subscriber
    /// for this thread alone, which a current-thread test's tasks share.
    #[derive(Clone, Default)]
    struct Loud(Arc<Mutex<Vec<String>>>);

    impl tracing::Subscriber for Loud {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn event(&self, e: &tracing::Event<'_>) {
            let m = e.metadata();
            if matches!(
                *m.level(),
                tracing::Level::ERROR | tracing::Level::WARN | tracing::Level::INFO
            ) {
                self.0
                    .lock()
                    .unwrap()
                    .push(format!("{} {}", m.level(), m.target()));
            }
        }
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
    }

    /// theseus-j7xi: the binding's live progress is best-effort, so it
    /// ignores `events.lost` and `execution.changed` (theseus-in3, 9c). The
    /// route drops `events.lost`, which names no session, and hands
    /// `execution.changed` to its session's place, whose renderer draws
    /// nothing for it. The place sends its lane nothing for either, nothing
    /// fails, and nothing is logged above debug. A turn's start, sent after
    /// them, is still drawn: the place goes on as before.
    #[tokio::test]
    async fn a_place_draws_nothing_for_events_lost_or_execution_changed() {
        let loud = Loud::default();
        let _guard = tracing::subscriber::set_default(loud.clone());
        let d = tempfile::tempdir().unwrap();
        let core = core_for_tests(d.path());
        let rec = theseus_core::session::SessionRecord::new(SessionKind::Conversation, None);
        let sid = rec.session_id.clone();
        core.store.put_session(&sid, &rec).unwrap();
        let (mut place, mut mailbox) = place_for_tests(&core, &sid);
        let (lane_tx, mut lane) = mpsc::unbounded_channel();
        place.lane = lane_tx;
        let (notes, rx) = mpsc::unbounded_channel();
        tokio::spawn(route(place.shared.clone(), rx));
        let view: theseus_protocol::ExecutionView = serde_json::from_value(json!({
            "position": 7, "at_ms": 1, "execution_id": "exec_j7xi", "session_id": sid,
            "kind": "conversation", "state": "waiting", "previous": "running",
            "attention": {"level": "ready", "label": "ready", "since_ms": 1}
        }))
        .unwrap();
        // What the core logged as it was built is not the binding's.
        loud.0.lock().unwrap().clear();
        for e in [
            CoreEvent::EventsLost(theseus_protocol::EventsLost {
                dropped: 865,
                streams: vec!["executions".into(), format!("session:{sid}")],
            }),
            CoreEvent::ExecutionChanged(view),
            CoreEvent::TurnStarted(theseus_protocol::TurnStarted {
                session_id: sid.clone(),
                turn_id: "turn_j7xi".into(),
                ..Default::default()
            }),
        ] {
            notes.send(e.notification()).unwrap();
        }
        // The route hands the place what names its session, in order; the
        // turn's start comes last.
        let mut got = vec![];
        loop {
            let m = tokio::time::timeout(Duration::from_secs(5), mailbox.recv())
                .await
                .expect("the route handed the place its events")
                .unwrap();
            let PlaceMsg::Event(e) = &m else {
                continue;
            };
            got.push(e.method());
            let last = matches!(**e, CoreEvent::TurnStarted(_));
            place.handle(m).await;
            if last {
                break;
            }
        }
        assert_eq!(
            got,
            ["execution.changed", "turn.started"],
            "events.lost names no session, and the route drops it"
        );
        // Only the turn's start reached the lane: typing.
        let mut sent = vec![];
        while let Ok(m) = lane.try_recv() {
            sent.push(match m {
                LaneMsg::Live(crate::render::Op::Typing) => "typing".to_string(),
                LaneMsg::Live(op) => format!("live {:?}", op.key()),
                _ => "other".to_string(),
            });
        }
        assert_eq!(sent, ["typing"]);
        assert!(!place.saw_failure);
        assert!(
            loud.0.lock().unwrap().is_empty(),
            "logged above debug: {:?}",
            loud.0.lock().unwrap()
        );
    }

    /// `/stop` (W1) halts the session's work and keeps the place on the same
    /// session: the approval it waited on is declined, and the next message
    /// continues it. `/new` alone starts a fresh session.
    #[tokio::test]
    async fn stop_keeps_the_places_session_and_new_alone_starts_a_fresh_one() {
        use theseus_core::provider::Scripted;
        let d = tempfile::tempdir().unwrap();
        let core = core_scripted(
            d.path(),
            vec![
                Scripted::tools(
                    "Writing.",
                    &[("w1", "fs_write", json!({"path": "a.txt", "content": "x\n"}))],
                ),
                Scripted::text("Understood: not written."),
            ],
        );
        let rec = theseus_core::session::SessionRecord::new(
            theseus_protocol::SessionKind::Conversation,
            None,
        );
        let sid = rec.session_id.clone();
        core.store.put_session(&sid, &rec).unwrap();
        core.outbox.bind_place("dm:42", &sid).unwrap();
        let (rpc, _notes) = RpcClient::connect(core.clone(), Client::new("test", Surface::Cli));
        let first: TurnSubmitResult = rpc
            .call(
                theseus_protocol::method::TURN_SUBMIT,
                TurnSubmitParams {
                    session_id: Some(sid.clone()),
                    input: "FIRST write a file".into(),
                    profile: None,
                    provider: None,
                    model: None,
                    author: Some("test".into()),
                    attachments: vec![],
                    reply_to: None,
                    opened_from: None,
                },
            )
            .await
            .unwrap();
        assert!(first.awaiting_confirm.is_some(), "it asks first");
        let (mut place, mut rx) = place_for_tests(&core, &sid);
        let answer = place.control(Control::Stop, "discord:eddie").await;
        assert_eq!(
            answer,
            "⏹️ Stopped this session's work (0 running action(s) told to stop). The \
             conversation goes on; `/new` starts a fresh one."
        );
        assert_eq!(place.session_id, sid, "the same session");
        assert!(core.kernel.pending_confirms().unwrap().is_empty());
        // The next message continues it.
        place.submit(vec![Inbound {
            author: "eddie".into(),
            author_id: 42,
            text: "SECOND never mind".into(),
            files: None,
            message: Id::new(7),
            channel: Id::new(43),
            guild: None,
        }]);
        let done = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Some(PlaceMsg::SubmitDone(r)) = rx.recv().await {
                    break r;
                }
            }
        })
        .await
        .unwrap();
        done.unwrap();
        let rec: theseus_core::session::SessionRecord =
            core.store.get_session(&sid).unwrap().unwrap();
        assert_eq!(rec.turns, 2, "both turns in the one session");
        // `/new` alone starts a fresh one.
        let answer = place.control(Control::New, "discord:eddie").await;
        assert_ne!(place.session_id, sid);
        assert!(answer.starts_with("🆕 New session"), "{answer}");
        assert_eq!(
            core.outbox.place_session("dm:42").unwrap().as_deref(),
            Some(place.session_id.as_str())
        );
    }

    /// Fail closed (theseus-qa0): with its token resolving the binding
    /// waits, and with it failed the binding says why and lets the driver go
    /// on. It never starts connecting either way.
    #[tokio::test]
    async fn the_binding_never_connects_without_its_token() {
        use theseus_core::secrets::{Secret, SecretBoard};
        let d = tempfile::tempdir().unwrap();
        let name = "discord_bot_token".to_string();
        let board = SecretBoard::new([name.clone()], std::time::Instant::now());
        let core = core_with_secrets(d.path(), board.clone());
        let path = d.path().join("bindings.toml");
        std::fs::write(&path, crate::EXAMPLE_BINDINGS).unwrap();
        let cfg = core.cfg.discord.clone();
        assert!(cfg.enabled && cfg.token_secret == name);
        let binding = tokio::spawn(run(core.clone(), cfg, path));
        let state = |core: &Arc<Core>| {
            let b = core.bindings.all().pop().unwrap();
            (b.state, b.detail.unwrap_or_default())
        };
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let (s, detail) = state(&core);
        assert_eq!(s, "waiting", "{detail}");
        assert!(detail.contains("discord_bot_token"), "{detail}");
        board.publish(
            [(name.clone(), Err::<Secret, _>("could not find item".into()))].into(),
            "fake",
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let (s, detail) = state(&core);
        assert_eq!(s, "failed");
        assert!(
            detail.contains("discord_bot_token") && detail.contains("could not find item"),
            "{detail}"
        );
        assert!(!binding.is_finished(), "it waits for a retry");
        binding.abort();
        let phases = core.startup_log.snapshot();
        let p = phases.iter().find(|p| p.name == "discord.token").unwrap();
        assert_eq!(p.detail["outcome"], "failed");
    }

    /// A pick sends `policy.tighten` for its tool and call, as the presser
    /// in their place, through the binding's own connection; the core
    /// records it, and the presser alone hears how it went and how to undo
    /// it (theseus-sgh).
    #[tokio::test]
    async fn a_pick_sends_the_tighten_request_and_the_presser_hears_how_to_undo_it() {
        let d = tempfile::tempdir().unwrap();
        let core = core_for_tests(d.path());
        let (rpc, _notes) = RpcClient::connect(core.clone(), Client::new(CLIENT, Surface::Discord));
        let pick = parse_asked_pick(ASKED_MENU, &["proc.run".into()]).unwrap();
        // The binding names who pressed, and where, on every press: without
        // `[approval]` too, a press from it that names none is refused.
        let dm = || {
            Some(DiscordOrigin {
                user_id: "271828182845904523".into(),
                channel_id: "444444444444444444".into(),
                guild_id: None,
            })
        };
        let r = send_tighten(&rpc, &pick, "discord:eddie", dm()).await;
        let t = r.as_ref().unwrap();
        assert_eq!(
            (t.tool.as_str(), t.posture.as_str(), t.changed),
            ("proc.run", "approve", true)
        );
        assert_eq!(
            (t.tightening.by.as_str(), t.tightening.via.as_str()),
            ("discord:eddie", "discord:dm")
        );
        assert_eq!(core.health().tightenings[0].tool, "proc.run");
        assert_eq!(
            tightened_reply("proc.run", &r),
            "🔒 `proc.run` asks first from now on. Undo it in the web UI's Tools view, or with \
             `theseus policy untighten proc.run`."
        );
        let again = send_tighten(&rpc, &pick, "discord:eddie", dm()).await;
        assert_eq!(
            tightened_reply("proc.run", &again),
            "🔒 `proc.run` already asks first: tightened by discord:eddie."
        );
        let bad = Asked {
            tool: "fs.read".into(),
            correlation_id: Some("act_nope".into()),
        };
        let e = send_tighten(&rpc, &bad, "discord:eddie", dm()).await;
        assert_eq!(
            tightened_reply("fs.read", &e),
            "⚠️ Could not tighten `fs.read`: no call act_nope"
        );
        let refused: Result<TightenResult, CallError> = Err(CallError {
            code: theseus_protocol::error_code::REFUSED,
            message: "the press from x does not count".into(),
            data: json!({"why": "it came through a connection no listener named"}),
        });
        assert_eq!(
            tightened_reply("proc.run", &refused),
            "🔐 Your press did not count: it came through a connection no listener named."
        );
    }

    /// Eddie's user id, and a user of a place whom `[approval]` does not list.
    pub(super) const EDDIE: u64 = 271_828_182_845_904_523;
    const MALLORY: u64 = 222_222_222_222_222_222;

    /// Give `sid` a hold on web text, as a fetch leaves one (theseus-9bp);
    /// the core's own tests bring it in through a real fetch.
    fn holding(core: &Arc<Core>, sid: &str) {
        let mut rec: theseus_core::session::SessionRecord =
            core.store.get_session(sid).unwrap().unwrap();
        rec.external = Some(theseus_protocol::ExternalText {
            since_ms: 1_759_266_720_000,
            tool: "http.fetch".into(),
            url: "https://example.test/page".into(),
            node_id: "nod_1".into(),
            from_session: None,
            via: None,
            query: None,
        });
        core.store.put_session(sid, &rec).unwrap();
    }

    /// `/trust` (T1b), driven as W1 drives `/stop`: it clears the place's
    /// session's hold through `policy.trust`, as the presser, and
    /// `session.trusted` names Discord and them; a second press finds nothing
    /// to trust and writes nothing; a user `[approval]` does not list is
    /// refused with the reason, and the hold stays; and a typed `/trust`
    /// answers in the place.
    #[tokio::test]
    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
    async fn trust_clears_the_places_hold_as_the_presser_under_approval() {
        let d = tempfile::tempdir().unwrap();
        let core = core_with(d.path(), theseus_core::secrets::SecretBoard::empty(), |c| {
            c.discord.rest_proxy = Some("127.0.0.1:9".into());
            c.approval = Some(theseus_core::config::ApprovalConfig {
                trusted_users: vec![format!("discord:{EDDIE}")],
                channels: vec!["discord:dm".into(), "cli".into()],
            });
        });
        let rec = theseus_core::session::SessionRecord::new(
            theseus_protocol::SessionKind::Conversation,
            None,
        );
        let sid = rec.session_id.clone();
        core.store.put_session(&sid, &rec).unwrap();
        holding(&core, &sid);
        let (mut place, _rx) = place_for_tests(&core, &sid);
        let dm = |user: u64| {
            Some(DiscordOrigin {
                user_id: user.to_string(),
                channel_id: "444444444444444444".into(),
                guild_id: None,
            })
        };
        let ledger = || {
            core.store
                .ledger_tail::<theseus_core::ledger::LedgerRow>(100_000)
                .unwrap()
        };
        let rows = |kind: &str| -> Vec<Value> {
            ledger()
                .into_iter()
                .filter(|(_, r)| r.kind == kind)
                .map(|(_, r)| r.data)
                .collect()
        };
        let held = || {
            core.store
                .get_session::<theseus_core::session::SessionRecord>(&sid)
                .unwrap()
                .unwrap()
                .external
        };

        let answer = place
            .control(Control::Trust(dm(EDDIE)), "discord:eddie")
            .await;
        assert!(
            answer.starts_with(
                "Trusted again by discord:eddie: this conversation no longer holds web text (it \
                 had read http.fetch https://example.test/page, at "
            ),
            "{answer}"
        );
        assert!(
            answer.ends_with("). Calls that change things run at their postures again."),
            "{answer}"
        );
        assert!(held().is_none());
        let trusted = rows("session.trusted");
        assert_eq!(trusted.len(), 1);
        let who = format!("discord:{EDDIE} (discord:eddie)");
        assert_eq!(
            (
                trusted[0]["via"].as_str(),
                trusted[0]["by"].as_str(),
                trusted[0]["who"].as_str(),
                trusted[0]["how"].as_str()
            ),
            (
                Some("discord:dm"),
                Some("discord:eddie"),
                Some(who.as_str()),
                Some("policy.trust")
            )
        );

        // A second press: nothing to trust, and nothing written.
        let before = ledger().len();
        let answer = place
            .control(Control::Trust(dm(EDDIE)), "discord:eddie")
            .await;
        assert_eq!(
            answer,
            "This conversation holds no web text, so there is nothing to trust."
        );
        assert_eq!(ledger().len(), before);

        // A user `[approval]` does not list: refused with the reason,
        // ledgered, and the hold stays.
        holding(&core, &sid);
        let answer = place
            .control(Control::Trust(dm(MALLORY)), "discord:mallory")
            .await;
        assert_eq!(
            answer,
            format!(
                "🔐 Your /trust did not count: discord:{MALLORY} is not a trusted user \
                 ([approval] trusted_users). This conversation still holds web text."
            )
        );
        assert!(held().is_some());
        let refused = rows("approval.refused");
        assert_eq!(refused.len(), 1);
        assert_eq!(
            (refused[0]["act"].as_str(), refused[0]["via"].as_str()),
            (Some("policy.trust"), Some("discord:dm"))
        );
        assert_eq!(rows("session.trusted").len(), 1);

        // Typed as a message, it answers in the place, as its author.
        place
            .handle(PlaceMsg::Inbound(Inbound {
                author: "eddie".into(),
                author_id: EDDIE,
                text: "/trust".into(),
                files: None,
                message: Id::new(7),
                channel: Id::new(444_444_444_444_444_444),
                guild: None,
            }))
            .await;
        assert!(held().is_none());
        assert_eq!(rows("session.trusted").len(), 2);
        let said: Vec<String> = core
            .outbox
            .open_for("discord:dm:42")
            .iter()
            .filter_map(|a| {
                theseus_core::outbox::body_of(a)["text"]
                    .as_str()
                    .map(String::from)
            })
            .collect();
        assert!(
            said.iter()
                .any(|t| t.starts_with("Trusted again by discord:eddie: ")),
            "{said:?}"
        );
    }

    /// A slash command as the gateway delivers one, from `user`, in
    /// `channel`, in `guild` (None: a DM).
    fn slash(guild: Option<&str>, channel: u64, user: u64, command: &str) -> Interaction {
        let mut v = json!({
            "id": "1000000000000000001",
            "application_id": "1000000000000000002",
            "type": 2,
            "token": "not-a-token",
            "version": 1,
            "authorizing_integration_owners": {},
            "channel": {"id": channel.to_string(), "type": if guild.is_some() { 0 } else { 1 }},
            "user": {"id": user.to_string(), "username": if user == EDDIE { "eddie" } else { "mallory" },
                     "discriminator": "0", "avatar": null},
            "data": {"id": "1000000000000000003", "name": command, "type": 1, "options": []},
        });
        if let Some(g) = guild {
            v["guild_id"] = json!(g);
        }
        serde_json::from_value(v).unwrap()
    }

    /// Interactions find their place as messages do (theseus-e89): a guild
    /// interaction in a channel this daemon does not bind is left alone, with
    /// no answer and nothing acted on, even from a user whose DM it binds; a
    /// DM's reaches the DM's place; a bound channel's reaches its place, with
    /// the presser's ids for `/trust`; and a bound place still refuses a user
    /// it does not list.
    #[tokio::test]
    async fn interactions_find_their_place_as_messages_do_and_leave_the_rest_alone() {
        const GUILD: &str = "314159265358979323";
        const BOUND: u64 = 900_000_000_000_000_001;
        const UNBOUND: u64 = 900_000_000_000_000_002;
        let fake = theseus_sim::fake_discord::FakeDiscord::start();
        let d = tempfile::tempdir().unwrap();
        let addr = fake.addr.clone();
        let core = core_with(d.path(), theseus_core::secrets::SecretBoard::empty(), |c| {
            c.discord.rest_proxy = Some(addr)
        });
        let shared = shared_for_tests(&core);
        let (dm_tx, mut dm_rx) = mpsc::unbounded_channel();
        let (ch_tx, mut ch_rx) = mpsc::unbounded_channel();
        {
            let mut r = shared.routes.lock().unwrap();
            r.by_dm_user.insert(EDDIE, dm_tx);
            r.by_channel.insert(BOUND, ch_tx);
            r.users.insert(BOUND, vec![EDDIE]);
        }
        // Each answer to an interaction is a POST to its callback.
        let answered = || {
            fake.seen()
                .iter()
                .filter(|s| s.path.contains("/interactions/"))
                .count()
        };
        let commands = || {
            core.store
                .ledger_tail::<theseus_core::ledger::LedgerRow>(100_000)
                .unwrap()
                .into_iter()
                .filter(|(_, r)| r.kind == "discord.command")
                .count()
        };
        let control = |m: Option<PlaceMsg>| match m {
            Some(PlaceMsg::Control { cmd, by, reply }) => {
                let _ = reply.send("done".into());
                (cmd, by)
            }
            _ => panic!("not a control"),
        };

        // A guild channel no place of ours binds, from a user whose DM is
        // bound: no answer, and nothing acted on.
        for cmd in ["stop", "trust"] {
            shared
                .clone()
                .on_interaction(slash(Some(GUILD), UNBOUND, EDDIE, cmd))
                .await;
        }
        assert!(dm_rx.try_recv().is_err() && ch_rx.try_recv().is_err());
        assert_eq!((answered(), commands()), (0, 0), "{:?}", fake.seen());
        assert_eq!(shared.board.st.lock().unwrap().interactions, 0);

        // A bound channel still refuses a user it does not list.
        shared
            .clone()
            .on_interaction(slash(Some(GUILD), BOUND, MALLORY, "stop"))
            .await;
        assert!(ch_rx.try_recv().is_err());
        assert_eq!((answered(), commands()), (1, 0));

        // A DM's reaches the DM's place.
        let h = tokio::spawn(shared.clone().on_interaction(slash(
            None,
            444_444_444_444_444_444,
            EDDIE,
            "status",
        )));
        let got = tokio::time::timeout(Duration::from_secs(10), dm_rx.recv())
            .await
            .unwrap();
        assert_eq!(control(got), (Control::Status, "discord:eddie".into()));
        h.await.unwrap();

        // A bound channel's reaches its place, and `/trust` carries who
        // pressed it, and where.
        let h = tokio::spawn(shared.clone().on_interaction(slash(
            Some(GUILD),
            BOUND,
            EDDIE,
            "trust",
        )));
        let got = tokio::time::timeout(Duration::from_secs(10), ch_rx.recv())
            .await
            .unwrap();
        let origin = DiscordOrigin {
            user_id: EDDIE.to_string(),
            channel_id: BOUND.to_string(),
            guild_id: Some(GUILD.into()),
        };
        assert_eq!(
            control(got),
            (Control::Trust(Some(origin)), "discord:eddie".into())
        );
        h.await.unwrap();
        assert_eq!((answered(), commands()), (3, 2));
        assert!(dm_rx.try_recv().is_err() && ch_rx.try_recv().is_err());
    }

    /// One lookup for a message and an interaction (theseus-0g4): a DM by its
    /// author's DM binding, a guild channel by the channel with its author
    /// checked, a mention-only channel says so, and an unbound place is none.
    #[test]
    fn a_message_or_an_interaction_resolves_to_its_place() {
        let (dm, _dm_rx) = mpsc::unbounded_channel::<PlaceMsg>();
        let (lab, _lab_rx) = mpsc::unbounded_channel::<PlaceMsg>();
        let (talk, _talk_rx) = mpsc::unbounded_channel::<PlaceMsg>();
        let mut r = Routes::default();
        r.by_dm_user.insert(7, dm.clone());
        r.by_channel.insert(10, lab.clone());
        r.users.insert(10, vec![7]);
        r.by_channel.insert(20, talk.clone());
        r.users.insert(20, vec![7, 8]);
        r.mention_only.insert(20);
        let found = |f: Option<Resolved>| f.map(|f| (f.place, f.allowed, f.mention_only)).unwrap();

        // A DM: its author's binding, whatever channel it came through.
        let (place, allowed, mention_only) = found(r.resolve(Some(99), false, Some(7)));
        assert!(place.same_channel(&dm) && allowed && !mention_only);
        assert!(
            r.resolve(Some(99), false, Some(8)).is_none(),
            "8 has no DM binding"
        );
        assert!(r.resolve(Some(99), false, None).is_none());

        // A channel: by the channel; its listed user drives it, another not.
        let (place, allowed, mention_only) = found(r.resolve(Some(10), true, Some(7)));
        assert!(place.same_channel(&lab) && allowed && !mention_only);
        let (place, allowed, _) = found(r.resolve(Some(10), true, Some(8)));
        assert!(place.same_channel(&lab) && !allowed);
        let (_, allowed, _) = found(r.resolve(Some(10), true, None));
        assert!(!allowed, "no author drives nothing");

        // A mention-only channel says so.
        let (place, allowed, mention_only) = found(r.resolve(Some(20), true, Some(8)));
        assert!(place.same_channel(&talk) && allowed && mention_only);

        // An unbound channel, and a guild interaction with no channel: none.
        assert!(r.resolve(Some(30), true, Some(7)).is_none());
        assert!(r.resolve(None, true, Some(7)).is_none());
        // A guild channel never reaches a DM binding (theseus-e89).
        assert!(r.resolve(Some(7), true, Some(7)).is_none());
    }
}
