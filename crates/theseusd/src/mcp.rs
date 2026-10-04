//! Theseus's own MCP server, wired in (step 41b, M7 §2.5): an MCP client on
//! this machine (Claude Code, another agent) opens a Theseus conversation,
//! sends it text, and reads the reply, over streamable HTTP at `/mcp` on
//! 127.0.0.1, at `[mcp_server] port`, behind the vault's key. Off unless
//! `[mcp_server] enabled`.
//!
//! The listener and the mapping from MCP are `theseus-mcp`'s `server`
//! module; this is its core (`Core`, a `CoreClient`): a protocol client on
//! an in-process connection whose surface is `Surface::Mcp`, as the Discord
//! binding's is `Surface::Discord`. So the core judges it as it judges every
//! client: the sessions it opens have the principal `mcp` and the floor, its
//! approvals are refused, and it may call only its tools' methods
//! (`theseus_core::mcp_server`).
//!
//! - **Bound after serving**, once its key's secret resolves; nothing on the
//!   start path waits for it. Health's `mcp_server` says `starting` until it
//!   listens, and why when it fails.
//! - **Only the daemon's user**: each request's connection must be owned by
//!   the daemon's uid, read from `/proc/net/tcp` as the web UI reads it
//!   (theseus-3qf), before its key is.
//! - **A job's hold passes on**: a client running inside a job (its process
//!   is one of the daemon's descendants) opens or writes to a session that
//!   takes the job's session's hold, as the CLI's `opened_from` gives it
//!   (`trace`).
//! - **`conversation_send`** submits the turn and waits for its end, through
//!   the turn's own answer and then `session.wait` while it waits on the
//!   operator, up to the call's wait: then it answers `running` with the
//!   turn's id, and the turn goes on.
//! - **Rows**: `mcp_server.call` per call, `mcp_server.refused` at most once a
//!   minute per kind, both written off the runtime's workers.

mod trace;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use theseus_core::approval::{Client, Surface};
use theseus_core::secrets::Waited;
use theseus_core::Core as TheseusCore;
use theseus_discord::rpc_client::RpcClient;
use theseus_mcp::server::{
    self, CallRecord, Caller, CoreError, Refusal, Running, Sent, Server, Status, Why,
};
use theseus_protocol::mcp_server::{McpServerHealth, McpServerRefusals};
use theseus_protocol::{method, Event, Level, SessionWaitResult, TurnSubmitResult};
use tokio::sync::mpsc;

/// The label of the server's protocol connection, in logs and the ledger.
const LABEL: &str = "mcp";

/// Start the server once its key resolves, and keep health's block. A
/// failure is said in health and the log; the socket is unaffected.
pub fn start(core: Arc<TheseusCore>) {
    tokio::spawn(async move {
        let said = |state: &str, error: Option<String>| McpServerHealth {
            state: state.into(),
            port: core.cfg.mcp_server.port,
            error,
            ..McpServerHealth::default()
        };
        match listen(core.clone()).await {
            Ok(server) => {
                let server = Arc::new(server);
                let read = server.clone();
                core.mcp_server
                    .listening(Arc::new(move || health(&read.stats())));
                // It stops with the daemon.
                core.shutdown.notified().await;
                server.stop();
                core.mcp_server.say(said("stopped", None));
            }
            Err(why) => {
                tracing::error!(error = %why, "mcp server failed; protocol socket unaffected");
                core.mcp_server.say(said("failed", Some(why)));
            }
        }
    });
}

/// The key, the listener, and the core's connection.
async fn listen(core: Arc<TheseusCore>) -> Result<Server, String> {
    let m = core.cfg.mcp_server.clone();
    let key = match core.secrets.settle(&m.key_secret).await {
        Waited::Ready(k) => k.expose().to_string(),
        Waited::Failed(e) => {
            return Err(format!("its key ({}) did not resolve: {e}", m.key_secret))
        }
        Waited::Absent | Waited::Resolving => {
            return Err(format!(
                "its key ({}) is not a [secrets] entry",
                m.key_secret
            ))
        }
    };
    let mut cfg = server::Config::new(SocketAddr::from(([127, 0, 0, 1], m.port)), key);
    cfg.requests_per_minute = m.requests_per_minute;
    cfg.admit = Some(Arc::new(admit));
    let mcp = Arc::new(McpCore::connect(core));
    Server::bind(cfg, mcp)
        .await
        .map_err(|e| format!("binding 127.0.0.1:{}: {e}", m.port))
}

/// The web UI's rule (theseus-3qf): only a connection whose client socket
/// is the daemon's own uid's is served.
fn admit(ends: server::Ends) -> Result<(), String> {
    use theseus_core::peer::{admit, client_uid, own_uid, Admit};
    let Some(server) = ends.server else {
        return Err("its server end had no address".into());
    };
    match admit(&client_uid(server, ends.client), own_uid()) {
        Admit::Serve | Admit::Gone => Ok(()),
        Admit::Refuse { why, .. } => Err(why),
    }
}

/// Health's block, from the server's counters.
fn health(s: &server::Stats) -> McpServerHealth {
    let n = |w: Why| s.refused.get(&w).copied().unwrap_or(0);
    McpServerHealth {
        state: if s.listening { "listening" } else { "stopped" }.into(),
        port: s.port,
        sessions: s.sessions as u64,
        clients: s.clients.clone(),
        opened: s.opened,
        last_client: s.last_client.clone(),
        calls: s.calls,
        errors: s.errors,
        refused: McpServerRefusals {
            host: n(Why::Host),
            origin: n(Why::Origin),
            key: n(Why::Key),
            rate: n(Why::Rate),
            peer: n(Why::Peer),
        },
        error: None,
    }
}

/// The server's core: one in-process protocol connection on
/// `Surface::Mcp`, and the turns it waits on, by session.
pub struct McpCore {
    core: Arc<TheseusCore>,
    rpc: Arc<RpcClient>,
    /// Who waits for the next `turn.started` of each session.
    started: Arc<Mutex<HashMap<String, mpsc::UnboundedSender<String>>>>,
}

impl McpCore {
    pub fn connect(core: Arc<TheseusCore>) -> Self {
        let (rpc, mut notes) = RpcClient::connect(core.clone(), Client::new(LABEL, Surface::Mcp));
        let started: Arc<Mutex<HashMap<String, mpsc::UnboundedSender<String>>>> = Arc::default();
        let waiters = started.clone();
        tokio::spawn(async move {
            while let Some(n) = notes.recv().await {
                if let Ok(Some(Event::TurnStarted(t))) =
                    Event::from_notification(&n.method, &n.params)
                {
                    if let Some(tx) = waiters.lock().unwrap().get(&t.session_id) {
                        let _ = tx.send(t.turn_id);
                    }
                }
            }
        });
        Self { core, rpc, started }
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value, CoreError> {
        self.rpc
            .call(method, params)
            .await
            .map_err(|e| CoreError(e.message))
    }

    /// `session.wait`, read as its answer.
    async fn wait(&self, params: Value) -> Result<SessionWaitResult, CoreError> {
        let v = self.call(method::SESSION_WAIT, params).await?;
        serde_json::from_value(v).map_err(|e| CoreError(format!("session.wait's answer: {e}")))
    }

    /// The session of the job the caller runs in, if it runs in one.
    async fn opened_from(caller: &Caller) -> Option<String> {
        let (server, client) = (caller.ends.server?, caller.ends.client);
        tokio::task::spawn_blocking(move || trace::job_session(server, client))
            .await
            .ok()
            .flatten()
    }

    /// The session's execution as it is now: a wait that is over at once.
    async fn view(&self, session_id: &str) -> Result<SessionWaitResult, CoreError> {
        self.wait(json!({"session_id": session_id, "until": "settled", "timeout_ms": 0}))
            .await
    }

    /// Wait, up to `deadline`, until the session no longer works or needs
    /// anyone (the operator answered and the turn ran on): true when it did.
    async fn settle(&self, session_id: &str, deadline: tokio::time::Instant) -> bool {
        let mut after: Option<u64> = None;
        loop {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            if left.is_zero() {
                return false;
            }
            let mut p = json!({"session_id": session_id, "until": "settled",
                               "timeout_ms": u64::try_from(left.as_millis()).unwrap_or(u64::MAX)});
            if let Some(a) = after {
                p["after_position"] = json!(a);
            }
            let Ok(w) = self.wait(p).await else {
                return false;
            };
            let Some(v) = w.execution.filter(|_| w.reached != "timeout") else {
                return false;
            };
            if matches!(v.attention.level, Level::Ready | Level::Idle) {
                return true;
            }
            after = Some(v.position);
        }
    }

    /// The session's last reply and its turn, from its newest nodes.
    async fn last(
        &self,
        session_id: &str,
    ) -> Result<(Option<String>, Option<String>, Value), CoreError> {
        let h = self
            .call(
                method::SESSION_HISTORY,
                json!({"session_id": session_id, "n": 40}),
            )
            .await?;
        let nodes = h["nodes"].as_array().cloned().unwrap_or_default();
        let reply = nodes
            .iter()
            .rev()
            .find(|n| {
                n["kind"] == "assistant_message"
                    && n["text"].as_str().is_some_and(|t| !t.is_empty())
            })
            .and_then(|n| n["text"].as_str().map(String::from));
        let turn = nodes
            .iter()
            .rev()
            .find_map(|n| n["turn_id"].as_str().map(String::from));
        Ok((reply, turn, h["session"].clone()))
    }
}

impl server::CoreClient for McpCore {
    async fn open(&self, caller: Caller, label: Option<String>) -> Result<String, CoreError> {
        let mut full = format!("mcp {}", caller.client_name);
        if let Some(l) = label.filter(|l| !l.trim().is_empty()) {
            full.push(' ');
            full.push_str(l.trim());
        }
        let mut p = json!({"label": full});
        if let Some(from) = Self::opened_from(&caller).await {
            p["opened_from"] = json!(from);
        }
        let info = self.call(method::SESSION_OPEN, p).await?;
        info["session_id"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| CoreError("session.open answered no session".into()))
    }

    async fn send(
        &self,
        caller: Caller,
        session_id: String,
        text: String,
        wait: Duration,
    ) -> Result<Sent, CoreError> {
        let deadline = tokio::time::Instant::now() + wait;
        let (tx, mut turn_ids) = mpsc::unbounded_channel();
        self.started.lock().unwrap().insert(session_id.clone(), tx);
        let mut p = json!({"session_id": session_id, "input": text,
                           "author": format!("mcp {}", caller.client_name)});
        if let Some(from) = Self::opened_from(&caller).await {
            p["opened_from"] = json!(from);
        }
        // The turn runs on whatever the call answers; only its answer is
        // given up at the deadline.
        let rpc = self.rpc.clone();
        let mut submit = tokio::spawn(async move {
            rpc.call::<_, TurnSubmitResult>(method::TURN_SUBMIT, p)
                .await
        });
        let ended = tokio::select! {
            r = &mut submit => Some(r),
            () = tokio::time::sleep_until(deadline) => None,
        };
        let first_turn = turn_ids.try_recv().ok();
        self.started.lock().unwrap().remove(&session_id);
        let running = |turn_id: Option<String>| Sent::Running {
            status: Running::Running,
            turn_id: turn_id.unwrap_or_default(),
        };
        let r = match ended {
            None => return Ok(running(first_turn)),
            Some(Err(e)) => return Err(CoreError(format!("the turn's task failed: {e}"))),
            Some(Ok(Err(e))) => return Err(CoreError(e.message)),
            Some(Ok(Ok(r))) => r,
        };
        // A turn that parked on the operator (a call that waits, a budget
        // question) goes on once answered: its reply comes then.
        if !matches!(r.stop_reason.as_str(), "awaiting_confirm" | "budget") {
            return Ok(Sent::Reply {
                turn_id: r.turn_id,
                reply: r.output,
                cost_usd: r.cost_usd,
            });
        }
        if !self.settle(&session_id, deadline).await {
            return Ok(running(Some(r.turn_id)));
        }
        let (reply, _, _) = self.last(&session_id).await?;
        Ok(Sent::Reply {
            turn_id: r.turn_id,
            reply: reply.unwrap_or_default(),
            cost_usd: None,
        })
    }

    async fn status(&self, _caller: Caller, session_id: String) -> Result<Status, CoreError> {
        let (last_reply, turn_id, session) = self.last(&session_id).await?;
        let view = self.view(&session_id).await?.execution;
        let waiting_on = view.as_ref().and_then(|v| {
            let on = serde_json::to_value(v.waiting_on.as_ref()?).ok()?;
            on["on"].as_str().map(String::from)
        });
        Ok(Status {
            session_id,
            state: view
                .as_ref()
                .map_or_else(|| "idle".into(), |v| v.state.clone()),
            waiting_on,
            turn_id,
            last_reply,
            cost_usd: session["cost_usd"].as_f64().unwrap_or(0.0),
            turns: session["turns"].as_u64().unwrap_or(0),
        })
    }

    async fn tasks(&self, _caller: Caller, session_id: Option<String>) -> Result<Value, CoreError> {
        self.call(method::TASK_LIST, json!({"session_id": session_id}))
            .await
    }

    async fn wakes(&self, _caller: Caller, session_id: Option<String>) -> Result<Value, CoreError> {
        self.call(method::WAKE_LIST, json!({"session_id": session_id}))
            .await
    }

    fn called(&self, r: &CallRecord) {
        let core = self.core.clone();
        let data = json!({"tool": r.tool, "client": r.client_name, "latency_ms": r.latency_ms,
                          "ok": r.ok});
        let sid = r.session_id.clone();
        tokio::task::spawn_blocking(move || core.mcp_called(sid.as_deref(), data));
    }

    fn refused(&self, r: &Refusal) {
        let core = self.core.clone();
        let data = json!({"why": r.why, "client": r.client, "detail": r.detail,
                          "unreported": r.unreported});
        tracing::warn!(why = ?r.why, client = %r.client, "mcp server: a request was refused");
        tokio::task::spawn_blocking(move || core.mcp_refused(data));
    }
}
