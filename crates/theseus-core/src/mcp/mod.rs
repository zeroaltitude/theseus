//! The MCP board (M7 §2.1, step 36b): the servers `[mcp.servers]` attaches,
//! their processes and connections, their states, and the tools they list,
//! offered to the model in private places as [`McpTool`]s.
//!
//! - **After serving**, never on the start path: [`McpBoard::start`] starts
//!   every enabled server at once, each tended by a task of its own. A stdio
//!   server is spawned through `children::spawn(Kind::Owned)` in its own
//!   process group, with the job's environment and its `[secrets]` grants,
//!   and its stderr in `<state>/mcp/<name>.log`, capped. An HTTP server is
//!   the workspace's reqwest client.
//! - **The stored list.** Each server's last good `tools/list` is a META
//!   record, `mcp.tools.<server>`, written when it changes. A start reads it
//!   (one key per server) and offers those tools at once; a call to a server
//!   that is not up yet waits for that server alone, up to its
//!   `start_timeout_secs`, then fails `mcp_unavailable`.
//! - **States:** `stopped` (not started yet), `starting`, `ready`,
//!   `restarting`, `failed`. A crash restarts after 1 s, then 5 s, then
//!   30 s; a third crash within 10 minutes leaves it `failed` until
//!   [`McpBoard::restart`] (`theseus mcp restart <name>`).
//! - **A stop sends SIGTERM to each server's group and never waits.** A
//!   server whose stdin closes ends on its own: a daemon killed outright
//!   leaves none running that reads its stdin.
//! - **`list_changed`** lists again; the catalog changes at once, and a
//!   turn's request spec, fixed at its start, keeps the tools it began with,
//!   so the new list applies from the next turn. A changed name, schema, or
//!   description is `mcp.tools_changed`, ledgered and narrated.
//!
//! Each start, ready, exit, failure, and change is a fact (`fact::mcp`).

pub mod l1;
pub mod prompts;
#[cfg(test)]
mod tests;
pub mod tool;
mod trial;

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, RwLock, Weak};
use std::time::Duration;

use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use theseus_mcp::types::Tool as Listed;
use theseus_mcp::{Client, Event, Events};
use tokio::sync::{watch, Notify};
use tokio::time::Instant;

use crate::config::{McpConfig, McpServerConfig};
use crate::fact::mcp::{McpExited, McpFailed, McpReady, McpStarted, McpToolsChanged};
use crate::fact::Fact;
pub use tool::McpTool;

/// The META key of a server's stored list.
pub const STORED_PREFIX: &str = "mcp.tools.";
/// The waits before a restart: the first crash's, the second's, and every
/// later one's.
pub const BACKOFF: [Duration; 3] = [
    Duration::from_secs(1),
    Duration::from_secs(5),
    Duration::from_secs(30),
];
/// This many crashes within [`CRASH_WINDOW`] leave a server `failed`.
pub const CRASHES_TO_FAIL: usize = 3;
pub const CRASH_WINDOW: Duration = Duration::from_secs(600);
/// The most a server's stderr log keeps.
pub const LOG_CAP_BYTES: u64 = 1 << 20;

/// A server's state, as health says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Configured, not started yet: before serving, or `enabled = false`.
    Stopped,
    Starting,
    Ready,
    /// Waiting for its backoff after a crash.
    Restarting,
    Failed,
    Disabled,
    /// A proposed extension on trial (43a): started and tested, its tools
    /// never offered.
    Proposed,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Stopped => "stopped",
            State::Starting => "starting",
            State::Ready => "ready",
            State::Restarting => "restarting",
            State::Failed => "failed",
            State::Disabled => "disabled",
            State::Proposed => "proposed",
        }
    }
}

/// What the store keeps of a server's list.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StoredList {
    pub digest: String,
    pub tools: Vec<Listed>,
}

/// The digest of a tool list, as offered: each tool's name, description,
/// and schema, in the server's order.
pub fn digest(tools: &[Listed]) -> String {
    let mut h = Sha256::new();
    for t in tools {
        let one = serde_json::json!([t.name, t.description, t.input_schema]);
        h.update(one.to_string().as_bytes());
        h.update(b"\n");
    }
    hex::encode(&h.finalize()[..8])
}

/// What changed between two lists: added, removed, and changed names.
pub fn diff(before: &[Listed], after: &[Listed]) -> (Vec<String>, Vec<String>, Vec<String>) {
    let old: BTreeMap<&str, &Listed> = before.iter().map(|t| (t.name.as_str(), t)).collect();
    let new: BTreeMap<&str, &Listed> = after.iter().map(|t| (t.name.as_str(), t)).collect();
    let added = new
        .keys()
        .filter(|k| !old.contains_key(*k))
        .map(|k| k.to_string())
        .collect();
    let removed = old
        .keys()
        .filter(|k| !new.contains_key(*k))
        .map(|k| k.to_string())
        .collect();
    let changed = new
        .iter()
        .filter(|(k, t)| {
            old.get(*k)
                .is_some_and(|o| o.description != t.description || o.input_schema != t.input_schema)
        })
        .map(|(k, _)| k.to_string())
        .collect();
    (added, removed, changed)
}

/// A connection made: the client and its events.
pub struct Connected {
    pub client: Client,
    pub events: Events,
}

/// How the board reaches a server, so that its tests serve a fake in this
/// process on tokio's paused clock.
pub trait Connect: Send + Sync + 'static {
    /// Start (stdio) or reach (HTTP) `server`, and run the handshake. `env`
    /// is what a stdio server gets besides the job's environment; `bearer`
    /// an HTTP server's key.
    fn connect(
        &self,
        server: &str,
        cfg: &McpServerConfig,
        env: Vec<(String, String)>,
        bearer: Option<String>,
    ) -> BoxFuture<'static, Result<Connected, String>>;
}

/// The real one: a process through the children registry, or HTTP.
pub struct Spawn {
    /// Where each server's stderr goes: `<dir>/<name>.log`.
    pub log_dir: PathBuf,
    /// A stdio server's working directory: the first workspace root.
    pub cwd: PathBuf,
    /// The job's environment, which a stdio server gets too.
    pub base_env: Vec<(String, String)>,
    /// How a server with `sandbox = "l1"` starts (M7 43a).
    pub l1: l1::L1Spawn,
}

fn options(cfg: &McpServerConfig) -> theseus_mcp::Options {
    theseus_mcp::Options {
        request_timeout: Duration::from_secs(cfg.start_timeout_secs),
        call_timeout: Duration::from_secs(cfg.call_timeout_secs),
        ..Default::default()
    }
}

impl Connect for Spawn {
    fn connect(
        &self,
        server: &str,
        cfg: &McpServerConfig,
        env: Vec<(String, String)>,
        bearer: Option<String>,
    ) -> BoxFuture<'static, Result<Connected, String>> {
        let opts = options(cfg);
        let transport = match &cfg.url {
            Some(url) => {
                let mut t = theseus_mcp::client::HttpTarget::new(url.clone());
                t.bearer = bearer;
                Ok(theseus_mcp::Transport::Http(t))
            }
            None => self.spawn(server, cfg, env),
        };
        Box::pin(async move {
            let (client, events) = theseus_mcp::Client::connect(transport?, opts)
                .await
                .map_err(|e| e.to_string())?;
            Ok(Connected { client, events })
        })
    }
}

impl Spawn {
    fn spawn(
        &self,
        server: &str,
        cfg: &McpServerConfig,
        env: Vec<(String, String)>,
    ) -> Result<theseus_mcp::Transport, String> {
        if cfg.command.first().is_none_or(|p| p.trim().is_empty()) {
            return Err("its command names no program".into());
        }
        // An extension's frozen copy is where it runs, never the workspace.
        let cwd = cfg.frozen.clone().unwrap_or_else(|| self.cwd.clone());
        let mut cmd = match cfg.sandbox {
            crate::config::mcp::McpSandbox::L1 => {
                let env = self.base_env.iter().cloned().chain(env).collect();
                l1::command(&self.l1, cfg, env, cwd)?
            }
            crate::config::mcp::McpSandbox::L0 => {
                let mut cmd = theseus_mcp::client::stdio_command(&cfg.command)
                    .ok_or_else(|| "its command names no program".to_string())?;
                cmd.env_clear()
                    .envs(self.base_env.iter().cloned())
                    .envs(env)
                    .current_dir(cwd);
                cmd
            }
        };
        let _ = std::fs::create_dir_all(&self.log_dir);
        let child = theseus_kernel::children::spawn(
            theseus_kernel::children::Kind::Owned,
            || cmd.spawn(),
            tokio::process::Child::id,
        )
        .map_err(|e| format!("{} did not start: {e}", cfg.command[0]))?;
        Ok(theseus_mcp::Transport::Stdio(
            theseus_mcp::client::StdioServer {
                child,
                stderr_log: Some(theseus_mcp::client::StderrLog {
                    path: self.log_dir.join(format!("{server}.log")),
                    cap_bytes: LOG_CAP_BYTES,
                }),
            },
        ))
    }
}

/// `<state>/mcp` beside a store's directory, as the index's is.
pub fn log_dir(store_dir: &std::path::Path) -> PathBuf {
    let name = store_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "store".into());
    store_dir.with_file_name(name.replacen("store", "mcp", 1))
}

/// What one server is now.
#[derive(Default)]
struct Live {
    state: Option<State>,
    client: Option<Client>,
    pid: Option<u32>,
    started_at_ms: Option<u64>,
    last_error: Option<String>,
    protocol: Option<String>,
    /// The tools offered: the live list once it came, else the stored one.
    tools: Vec<Listed>,
    /// Whether `tools` is the stored list.
    stored: bool,
    digest: String,
    prompts: u64,
    /// The prompts listed, or the stored ones (36c, `prompts.rs`).
    prompt_state: prompts::PromptState,
    /// When each crash in the window came.
    crashes: VecDeque<Instant>,
    /// Crashes since the last start that stayed up, or the last restart.
    crash_count: u64,
}

impl Live {
    fn forget_crashes(&mut self) {
        self.crashes.clear();
        self.crash_count = 0;
    }
}

/// One configured server.
pub struct Server {
    pub name: String,
    pub cfg: McpServerConfig,
    live: Mutex<Live>,
    /// Bumped at each change of state: a call waiting for the server wakes.
    changed: watch::Sender<u64>,
    pub calls: AtomicU64,
    pub errors: AtomicU64,
    restart: Notify,
}

impl Server {
    fn new(name: &str, cfg: &McpServerConfig, stored: Option<StoredList>) -> Self {
        let mut live = Live {
            state: Some(if cfg.enabled {
                State::Stopped
            } else {
                State::Disabled
            }),
            ..Default::default()
        };
        if let Some(s) = stored {
            live.digest = s.digest;
            live.tools = s.tools;
            live.stored = true;
        }
        Self {
            name: name.into(),
            cfg: cfg.clone(),
            live: Mutex::new(live),
            changed: watch::Sender::new(0),
            calls: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            restart: Notify::new(),
        }
    }

    fn live(&self) -> std::sync::MutexGuard<'_, Live> {
        self.live.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn set(&self, f: impl FnOnce(&mut Live)) {
        f(&mut self.live());
        self.changed.send_modify(|n| *n += 1);
    }

    pub fn state(&self) -> State {
        self.live().state.unwrap_or(State::Stopped)
    }

    /// The server's client once it is ready: at once when it is, else after
    /// waiting for it alone, up to `wait`. A failed or disabled server, or
    /// one that did not come up in time, is why not.
    pub async fn client(&self, wait: Duration) -> Result<Client, String> {
        let mut rx = self.changed.subscribe();
        let deadline = Instant::now() + wait;
        loop {
            {
                let l = self.live();
                match (l.state.unwrap_or(State::Stopped), &l.client) {
                    (State::Ready, Some(c)) => return Ok(c.clone()),
                    (State::Failed, _) => {
                        return Err(format!(
                        "MCP server {} has failed ({}); `theseus mcp restart {}` starts it again",
                        self.name,
                        l.last_error.as_deref().unwrap_or("it crashed"),
                        self.name
                    ))
                    }
                    (State::Disabled, _) => {
                        return Err(format!("MCP server {} is disabled", self.name))
                    }
                    _ => {}
                }
            }
            if tokio::time::timeout_at(deadline, rx.changed())
                .await
                .map_or(true, |r| r.is_err())
            {
                let why = self.live().last_error.clone();
                return Err(format!(
                    "MCP server {} was not up within {} s{}",
                    self.name,
                    wait.as_secs(),
                    why.map(|w| format!(" (last: {w})")).unwrap_or_default()
                ));
            }
        }
    }

    /// `read` entries the server does not list.
    fn unknown_read(&self, tools: &[Listed]) -> Vec<String> {
        self.cfg
            .read
            .iter()
            .filter(|r| !tools.iter().any(|t| &t.name == *r))
            .cloned()
            .collect()
    }

    pub fn status(&self) -> theseus_protocol::mcp::McpServerStatus {
        let l = self.live();
        theseus_protocol::mcp::McpServerStatus {
            name: self.name.clone(),
            transport: self.cfg.transport().into(),
            state: l.state.unwrap_or(State::Stopped).as_str().into(),
            pid: l.pid,
            tools: l.tools.len() as u64,
            prompts: l.prompts,
            stored: l.stored,
            calls: self.calls.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
            crashes: l.crash_count,
            last_error: l.last_error.clone(),
            started_at_ms: l.started_at_ms,
            protocol: l.protocol.clone(),
            digest: l.digest.clone(),
            unknown_read: if l.stored && l.tools.is_empty() {
                Vec::new()
            } else {
                self.unknown_read(&l.tools)
            },
        }
    }
}

/// The MCP tools offered now, shared with the tool runtime: every server's
/// list, each tool with its unique wire name, sorted by canonical name.
#[derive(Default)]
pub struct McpCatalog {
    tools: RwLock<Arc<Vec<Arc<McpTool>>>>,
}

impl McpCatalog {
    pub fn all(&self) -> Arc<Vec<Arc<McpTool>>> {
        self.tools
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn set(&self, tools: Vec<Arc<McpTool>>) {
        *self.tools.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(tools);
    }

    pub fn is_empty(&self) -> bool {
        self.all().is_empty()
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn theseus_tools::Tool>> {
        self.all()
            .iter()
            .find(|t| t.canonical == name)
            .map(|t| t.clone() as Arc<dyn theseus_tools::Tool>)
    }

    pub fn by_wire(&self, wire: &str) -> Option<Arc<dyn theseus_tools::Tool>> {
        self.all()
            .iter()
            .find(|t| t.wire == wire)
            .map(|t| t.clone() as Arc<dyn theseus_tools::Tool>)
    }
}

/// The servers, their tending, and the catalog they fill.
pub struct McpBoard {
    servers: BTreeMap<String, Arc<Server>>,
    catalog: Arc<McpCatalog>,
    broker: Arc<crate::broker::Broker>,
    secrets: Arc<crate::secrets::SecretBoard>,
    connect: RwLock<Arc<dyn Connect>>,
    /// Where its facts go: the core, once built; held weakly, so a tending
    /// task never keeps the store open past a stop.
    core: OnceLock<Weak<crate::rpc::Core>>,
    /// Set once the daemon stops.
    stop: watch::Sender<bool>,
    started: std::sync::atomic::AtomicBool,
    /// Proposed extensions on trial (43a, `trial.rs`): never in `servers`,
    /// so `rebuild` never offers their tools.
    trials: Mutex<BTreeMap<String, Arc<Server>>>,
}

impl McpBoard {
    /// The board of `cfg`, offering each server's stored list at once.
    /// `stored` reads a server's list from the store: one META key each.
    pub fn new(
        cfg: &McpConfig,
        catalog: Arc<McpCatalog>,
        broker: Arc<crate::broker::Broker>,
        secrets: Arc<crate::secrets::SecretBoard>,
        connect: Arc<dyn Connect>,
        stored: impl Fn(&str) -> Option<StoredList>,
    ) -> Arc<Self> {
        let servers = cfg
            .servers
            .iter()
            .map(|(n, c)| (n.clone(), Arc::new(Server::new(n, c, stored(n)))))
            .collect();
        let board = Arc::new(Self {
            servers,
            catalog,
            broker,
            secrets,
            connect: RwLock::new(connect),
            core: OnceLock::new(),
            stop: watch::Sender::new(false),
            started: Default::default(),
            trials: Mutex::default(),
        });
        board.rebuild();
        board
    }

    /// How it reaches its servers, in place of the spawn (tests: a fake in
    /// this process). Before `start`.
    pub fn set_connect(&self, connect: Arc<dyn Connect>) {
        *self.connect.write().unwrap_or_else(PoisonError::into_inner) = connect;
    }

    pub fn attach(&self, core: Weak<crate::rpc::Core>) {
        let _ = self.core.set(core);
    }

    pub fn server(&self, name: &str) -> Option<&Arc<Server>> {
        self.servers.get(name)
    }

    pub fn is_empty(&self) -> bool {
        self.servers.is_empty()
    }

    /// Health's `mcp[]`.
    pub fn status(&self) -> Vec<theseus_protocol::mcp::McpServerStatus> {
        let trials = self.trials.lock().unwrap_or_else(PoisonError::into_inner);
        self.servers
            .values()
            .chain(trials.values())
            .map(|s| s.status())
            .collect()
    }

    /// After serving: tend every enabled server, each in a task of its own.
    /// Once only.
    pub fn start(self: &Arc<Self>) {
        if self.started.swap(true, Ordering::SeqCst) {
            return;
        }
        for s in self.servers.values().filter(|s| s.cfg.enabled) {
            tokio::spawn(self.clone().tend(s.clone()));
        }
    }

    /// The daemon stops: SIGTERM to each server's group, never waited for.
    pub fn stop(&self) {
        self.stop.send_replace(true);
        let trials = self.trials.lock().unwrap_or_else(PoisonError::into_inner);
        for s in self.servers.values().chain(trials.values()) {
            if let Some(c) = s.live().client.take() {
                c.terminate();
            }
        }
    }

    /// `mcp.restart`: start the server again now, a failed one included, its
    /// crashes forgotten. The state it was in, or None for no such server.
    pub fn restart(self: &Arc<Self>, name: &str) -> Option<State> {
        let s = self.servers.get(name)?;
        let was = s.state();
        if was == State::Disabled {
            return Some(was);
        }
        // Before serving, or a board whose tending never began: begin it,
        // which starts every server; a permit left behind would restart this
        // one again once up.
        if self.started.load(Ordering::SeqCst) {
            s.restart.notify_one();
        } else {
            self.start();
        }
        Some(was)
    }

    /// Resolves once the daemon stops.
    async fn stopped(&self) {
        let mut rx = self.stop.subscribe();
        let _ = rx.wait_for(|v| *v).await;
    }

    fn record<F: Fact + Send + 'static>(&self, f: F) {
        let Some(core) = self.core.get().and_then(Weak::upgrade) else {
            return;
        };
        let write = move || core.rec(None).record(&f);
        match tokio::runtime::Handle::try_current() {
            Ok(rt) => {
                rt.spawn_blocking(write);
            }
            Err(_) => write(),
        }
    }

    /// An operator notice in the outbox (a Discord binding posts it to the
    /// owner's DM), written off the runtime's workers.
    fn notice(&self, body: Value) {
        let Some(core) = self.core.get().and_then(Weak::upgrade) else {
            return;
        };
        let write = move || {
            if let Err(e) = core.outbox.to_operator(None, body) {
                tracing::warn!(error = %format!("{e:#}"), "the MCP notice was not written");
            }
        };
        match tokio::runtime::Handle::try_current() {
            Ok(rt) => {
                rt.spawn_blocking(write);
            }
            Err(_) => write(),
        }
    }

    fn store_list(&self, server: &str, list: StoredList) {
        let Some(core) = self.core.get().and_then(Weak::upgrade) else {
            return;
        };
        let key = format!("{STORED_PREFIX}{server}");
        let write = move || {
            if let Err(e) = core.store.put_meta(&key, &list) {
                tracing::warn!(error = %e, key, "the MCP server's tool list was not stored");
            }
        };
        match tokio::runtime::Handle::try_current() {
            Ok(rt) => {
                rt.spawn_blocking(write);
            }
            Err(_) => write(),
        }
    }

    /// The catalog again, from every server's list: unique wire names across
    /// servers, sorted by canonical name. Each tool is granted its server's
    /// secrets, so its calls run at no looser a posture than theirs.
    fn rebuild(&self) {
        let mut pairs: Vec<(Arc<Server>, Listed)> = Vec::new();
        for s in self.servers.values().filter(|s| s.cfg.enabled) {
            for t in s.live().tools.clone() {
                pairs.push((s.clone(), t));
            }
        }
        pairs.sort_by(|a, b| (&a.0.name, &a.1.name).cmp(&(&b.0.name, &b.1.name)));
        let names: Vec<(&str, &str)> = pairs
            .iter()
            .map(|(s, t)| (s.name.as_str(), t.name.as_str()))
            .collect();
        let wires = theseus_mcp::names::wire_names(&names);
        let tools: Vec<Arc<McpTool>> = pairs
            .iter()
            .zip(wires)
            .map(|((s, t), wire)| Arc::new(McpTool::new(s.clone(), t.clone(), wire)))
            .collect();
        for t in &tools {
            for secret in t.server.cfg.secrets() {
                self.broker.grant_tool(&t.canonical, secret);
            }
        }
        self.catalog.set(tools);
    }

    /// A stdio server's environment from `[secrets]`, and an HTTP server's
    /// key: each secret waited for, bounded by the start's timeout. A secret
    /// that does not resolve is left out, and said.
    async fn secret_env(&self, s: &Server) -> (Vec<(String, String)>, Option<String>) {
        let wait = Duration::from_secs(s.cfg.start_timeout_secs);
        let mut env = Vec::new();
        for (var, name) in &s.cfg.env {
            match self.secrets.wait(name, wait).await {
                crate::secrets::Waited::Ready(v) => env.push((var.clone(), v.expose().to_string())),
                _ => tracing::warn!(server = %s.name, secret = %name,
                    "an MCP server's secret did not resolve, so it starts without it"),
            }
        }
        let bearer = match &s.cfg.auth_secret {
            Some(name) => match self.secrets.wait(name, wait).await {
                crate::secrets::Waited::Ready(v) => Some(v.expose().to_string()),
                _ => None,
            },
            None => None,
        };
        (env, bearer)
    }

    /// One server's life: start, serve, and restart after a crash with
    /// backoff, until the daemon stops.
    async fn tend(self: Arc<Self>, s: Arc<Server>) {
        let mut attempt = 0u64;
        loop {
            attempt += 1;
            s.set(|l| l.state = Some(State::Starting));
            let t0 = Instant::now();
            let wait = Duration::from_secs(s.cfg.start_timeout_secs);
            let opened = tokio::select! {
                r = tokio::time::timeout(wait, self.open(&s)) => r,
                () = self.stopped() => return,
            };
            let why = match opened {
                Ok(Ok((c, tools, prompts))) => {
                    self.ready(&s, &c, tools, prompts, attempt, t0);
                    match self.serve(&s, c).await {
                        Ended::Stopped => return,
                        Ended::Restart => {
                            s.set(Live::forget_crashes);
                            continue;
                        }
                        Ended::Closed(why) => why,
                    }
                }
                Ok(Err(e)) => e,
                Err(_) => format!("no handshake and list within {} s", wait.as_secs()),
            };
            if *self.stop.borrow() || !self.crashed(&s, why).await {
                return;
            }
        }
    }

    /// Start or reach the server, and read its tools and how many prompts
    /// it has.
    async fn open(
        &self,
        s: &Server,
    ) -> Result<(Connected, Vec<Listed>, Vec<theseus_mcp::types::Prompt>), String> {
        let (env, bearer) = self.secret_env(s).await;
        let connect = self
            .connect
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let c = connect.connect(&s.name, &s.cfg, env, bearer).await?;
        let tools = c.client.list_tools().await.map_err(|e| e.to_string())?;
        let prompts = match c.client.server_info().capabilities.prompts.is_some() {
            true => c.client.list_prompts().await.unwrap_or_default(),
            false => Vec::new(),
        };
        Ok((c, tools, prompts))
    }

    /// A server up: its rows, its list applied, and its state `ready`.
    fn ready(
        &self,
        s: &Server,
        c: &Connected,
        tools: Vec<Listed>,
        prompts: Vec<theseus_mcp::types::Prompt>,
        attempt: u64,
        t0: Instant,
    ) {
        let pid = c.client.pid();
        self.record(McpStarted {
            server: s.name.clone(),
            transport: s.cfg.transport(),
            pid,
            attempt,
        });
        let protocol = c.client.server_info().protocol_version.clone();
        let count = tools.len();
        let prompts_listed = prompts.len();
        self.apply(s, tools);
        self.apply_prompts(s, prompts);
        s.set(|l| {
            l.state = Some(State::Ready);
            l.client = Some(c.client.clone());
            l.pid = pid;
            l.started_at_ms = Some(theseus_protocol::now_unix_ms());
            l.protocol = Some(protocol.clone());
        });
        let digest = s.live().digest.clone();
        self.record(McpReady {
            server: s.name.clone(),
            ms: t0.elapsed().as_millis() as u64,
            tools: count,
            prompts: prompts_listed,
            protocol,
            digest,
        });
    }

    /// A crash, counted in its window: the backoff, then true to start
    /// again; or, the third in 10 minutes, `failed` until a restart. False
    /// once the daemon stops.
    async fn crashed(&self, s: &Server, why: String) -> bool {
        let now = Instant::now();
        let (failed, crashes) = {
            let mut l = s.live();
            l.client = None;
            l.pid = None;
            l.last_error = Some(why.clone());
            l.crash_count += 1;
            l.crashes.push_back(now);
            while l
                .crashes
                .front()
                .is_some_and(|t| now.duration_since(*t) > CRASH_WINDOW)
            {
                l.crashes.pop_front();
            }
            (l.crashes.len() >= CRASHES_TO_FAIL, l.crash_count)
        };
        if failed {
            s.set(|l| l.state = Some(State::Failed));
            tracing::warn!(server = %s.name, why = %why, "MCP server failed");
            self.record(McpFailed {
                server: s.name.clone(),
                why,
                crashes,
            });
            tokio::select! {
                () = s.restart.notified() => {}
                () = self.stopped() => return false,
            }
            s.set(Live::forget_crashes);
            return true;
        }
        let backoff = BACKOFF[(crashes as usize - 1).min(BACKOFF.len() - 1)];
        s.set(|l| l.state = Some(State::Restarting));
        tracing::warn!(server = %s.name, why = %why, backoff_ms = backoff.as_millis() as u64,
            "MCP server ended; restarting");
        self.record(McpExited {
            server: s.name.clone(),
            why,
            crashes,
            backoff_ms: backoff.as_millis() as u64,
        });
        tokio::select! {
            () = tokio::time::sleep(backoff) => {}
            () = s.restart.notified() => s.set(Live::forget_crashes),
            () = self.stopped() => return false,
        }
        true
    }

    /// A ready server, until its connection ends, it is restarted, or the
    /// daemon stops. `list_changed` (or an HTTP session made again) lists
    /// its tools again.
    async fn serve(&self, s: &Arc<Server>, c: Connected) -> Ended {
        let Connected { client, mut events } = c;
        loop {
            tokio::select! {
                ev = events.recv() => match ev {
                    Some(Event::PromptListChanged) => self.relist_prompts(s, &client).await,
                    Some(ref e @ (Event::ToolListChanged | Event::Reinitialized)) => {
                        if matches!(e, Event::Reinitialized) {
                            self.relist_prompts(s, &client).await;
                        }
                        match client.list_tools().await {
                            Ok(tools) => self.apply(s, tools),
                            Err(e) => tracing::warn!(server = %s.name, error = %e,
                                "an MCP server's changed list could not be read"),
                        }
                    }
                    Some(Event::Closed { reason }) => return Ended::Closed(reason),
                    None => return Ended::Closed("its connection ended".into()),
                    Some(_) => {}
                },
                () = s.restart.notified() => {
                    client.terminate();
                    s.set(|l| { l.client = None; l.pid = None; });
                    return Ended::Restart;
                }
                () = self.stopped() => {
                    client.terminate();
                    return Ended::Stopped;
                }
            }
        }
    }

    /// A list the server gave: descriptions capped, compared with the list
    /// offered before (a change is ledgered and narrated), stored when it
    /// differs from the store's, and the catalog rebuilt.
    fn apply(&self, s: &Server, mut tools: Vec<Listed>) {
        for t in &mut tools {
            if let Some(d) = &mut t.description {
                tool::cap_description(d);
            }
        }
        let new = digest(&tools);
        let (before, old, was_stored) = {
            let l = s.live();
            (l.tools.clone(), l.digest.clone(), l.stored)
        };
        if new != old {
            if !old.is_empty() || !before.is_empty() {
                let (added, removed, changed) = diff(&before, &tools);
                tracing::warn!(server = %s.name, ?added, ?removed, ?changed,
                    "an MCP server's tools changed; they apply from the next turn");
                let fact = McpToolsChanged {
                    server: s.name.clone(),
                    added,
                    removed,
                    changed,
                    digest: new.clone(),
                };
                // The "rug pull" case: said where the operator looks.
                self.notice(serde_json::json!({"kind": "mcp_changed", "server": s.name,
                    "summary": fact.summary()}));
                self.record(fact);
            }
            self.store_list(
                &s.name,
                StoredList {
                    digest: new.clone(),
                    tools: tools.clone(),
                },
            );
        } else if was_stored {
            // The same list as the store's: nothing to write.
        }
        s.set(|l| {
            l.tools = tools;
            l.digest = new;
            l.stored = false;
        });
        self.rebuild();
    }

    /// `mcp.list`: the servers, and every tool offered with its posture.
    pub fn list(&self, rt: &crate::toolrun::ToolRuntime) -> theseus_protocol::mcp::McpListResult {
        let calls = rt.calls.lock().unwrap().clone();
        let tools = self
            .catalog
            .all()
            .iter()
            .map(|t| {
                let now = rt.posture_now(&t.canonical);
                theseus_protocol::mcp::McpToolInfo {
                    server: t.server.name.clone(),
                    tool: t.listed.name.clone(),
                    name: t.canonical.clone(),
                    wire_name: t.wire.clone(),
                    class: theseus_tools::Tool::class(t.as_ref()).as_str().into(),
                    posture: now.posture.as_str().into(),
                    setting: now.setting,
                    hints: t.hints(),
                    description: t.description.clone(),
                    calls: calls.get(&t.canonical).copied().unwrap_or(0),
                }
            })
            .collect();
        theseus_protocol::mcp::McpListResult {
            servers: self.status(),
            tools,
            prompts: self.prompt_infos(None),
        }
    }
}

enum Ended {
    Stopped,
    Restart,
    Closed(String),
}

/// What a stored list of a server reads as, when it reads.
pub fn read_stored(store: &crate::store::Store, server: &str) -> Option<StoredList> {
    match store.get_meta::<StoredList>(&format!("{STORED_PREFIX}{server}")) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, server, "an MCP server's stored tool list did not read");
            None
        }
    }
}

/// A value's JSON, cut to `max` characters, for a call's summary.
pub(crate) fn short(v: &Value, max: usize) -> String {
    let s = v.to_string();
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s,
    }
}

/// An MCP call's span attributes (`mcp.call`): its server and its tool.
pub fn span_attrs(tool: Option<&dyn theseus_tools::Tool>, attrs: &mut Value) {
    let Some(name) = tool
        .map(|t| t.name())
        .filter(|n| n.starts_with(crate::policy::MCP_PREFIX))
    else {
        return;
    };
    if let Some((server, tool)) = name[crate::policy::MCP_PREFIX.len()..].split_once('/') {
        attrs["mcp.call"] = Value::Bool(true);
        attrs["mcp.server"] = Value::String(server.into());
        attrs["mcp.tool"] = Value::String(tool.into());
    }
}
