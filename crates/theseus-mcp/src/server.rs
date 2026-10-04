//! Theseus's own MCP server (step 41a, design §2.5): another program
//! (Claude Code, say) opens a Theseus conversation over MCP and gets its
//! replies.
//!
//! **Transport.** Streamable HTTP at `/mcp`, on axum, on a loopback
//! address only. A POST carries one JSON-RPC message, and the answer is
//! `application/json`: v1 streams nothing, so a long turn answers with its
//! status instead. A GET answers 405, since there is no server stream.
//! `Mcp-Session-Id` is set at `initialize`, rides on every later request
//! (none: 400; unknown or idle too long: 404, and the client makes a new
//! one), and a DELETE ends it.
//!
//! **Security**, in order, before anything is read:
//! - the connection's peer, when the wire-in gives an `admit` check (the
//!   daemon's: the client socket's owner must be the daemon's own uid, as
//!   the web UI's must, theseus-3qf), run off the runtime's workers;
//! - the `Host`, and a target's authority when it has one, must name this
//!   listener (its address or `localhost`, at its port), as the web UI's
//!   must (theseus-70f): a DNS-rebinding page's requests carry its own name;
//! - an `Origin`, when there is one, must be a loopback page (MCP's
//!   transport rules ask it of every server);
//! - `Authorization: Bearer <key>`, compared in constant time (both sides
//!   hashed first, so not even the length shows): 401 otherwise;
//! - `requests_per_minute` over every client (one key, one principal, one
//!   budget): 429 with `Retry-After` past it.
//!
//! Each refusal is counted, and reported to the core at most once a minute
//! per kind, with the number left unreported ([`CoreClient::refused`]).
//!
//! **The core** is behind [`CoreClient`]: the wire-in (41b) implements it
//! as an in-process protocol client, as the Discord binding's
//! `rpc_client.rs` is, and [`FakeCore`] stands in for tests. The server
//! passes each call's [`Caller`]: the MCP client's name (the session's
//! label is `mcp <name>`), its MCP session, and the connection's two ends,
//! for the peer trace. Approvals never come from here: that is the core's
//! rule for `Surface::Mcp`.
//!
//! **Tools.** `conversation_open`, `conversation_send`,
//! `conversation_status`, `task_list`, and `wake_list`, named with `_` for
//! clients whose tool names cannot carry dots. Each answer is JSON, as
//! `structuredContent` and as text; a core's refusal is an `isError`
//! result the client's model reads.

use std::collections::HashMap;
use std::future::Future;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::connect_info::Connected;
use axum::extract::{ConnectInfo, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::Router;
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use crate::jsonrpc::{self, clip, code, Incoming};
use crate::types::Implementation;

/// How the server runs. The wire-in fills it from `[mcp_server]` and the
/// vault's key.
#[derive(Clone)]
pub struct Config {
    /// A loopback address and port (0 picks one). Any other is refused.
    pub bind: SocketAddr,
    /// The one static key. At least 16 bytes.
    pub key: String,
    /// Requests a minute, over every client.
    pub requests_per_minute: u32,
    /// `conversation_send`'s wait when the client names none, and the most
    /// it may name.
    pub default_wait: Duration,
    pub max_wait: Duration,
    /// Sessions kept at once; past it, the one idle longest is ended.
    pub max_sessions: usize,
    /// A session idle this long is ended.
    pub session_idle: Duration,
    pub server_info: Implementation,
    pub instructions: Option<String>,
    /// Whether a connection's peer may be served, asked of each request
    /// before anything else, on the blocking pool: `Err` says why not (a
    /// 403, and a `Why::Peer` refusal). None: every peer is.
    pub admit: Option<Admit>,
}

/// A peer check: the connection's two ends, and why it may not be served.
pub type Admit = Arc<dyn Fn(Ends) -> Result<(), String> + Send + Sync>;

impl Config {
    pub fn new(bind: SocketAddr, key: impl Into<String>) -> Self {
        Self {
            bind,
            key: key.into(),
            requests_per_minute: 60,
            default_wait: Duration::from_secs(60),
            max_wait: Duration::from_secs(120),
            max_sessions: 64,
            session_idle: Duration::from_secs(3600),
            server_info: Implementation::new("theseus", env!("CARGO_PKG_VERSION")),
            instructions: Some(INSTRUCTIONS.into()),
            admit: None,
        }
    }
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("bind", &self.bind)
            .field("key", &"(set)")
            .field("requests_per_minute", &self.requests_per_minute)
            .field("default_wait", &self.default_wait)
            .field("max_wait", &self.max_wait)
            .finish_non_exhaustive()
    }
}

const INSTRUCTIONS: &str = "Theseus, an agent harness. Open a conversation with \
conversation_open, then talk in it with conversation_send. A turn that runs past its wait \
answers status \"running\"; follow it with conversation_status. A conversation's acting tool \
calls wait for the operator's approval, which is never given through MCP.";

/// Who is calling: the MCP client, and the connection it called on.
#[derive(Debug, Clone)]
pub struct Caller {
    /// `clientInfo.name` from its `initialize`: a session it opens is
    /// labelled `mcp <name>`.
    pub client_name: String,
    pub client_version: String,
    /// The MCP session (not a Theseus session).
    pub mcp_session: String,
    /// The connection's two ends, for the peer trace (the loopback owner,
    /// through `/proc/net/tcp`).
    pub ends: Ends,
}

/// A TCP connection's two ends, as it was accepted.
#[derive(Debug, Clone, Copy)]
pub struct Ends {
    pub server: Option<SocketAddr>,
    pub client: SocketAddr,
}

impl Connected<axum::serve::IncomingStream<'_, tokio::net::TcpListener>> for Ends {
    fn connect_info(s: axum::serve::IncomingStream<'_, tokio::net::TcpListener>) -> Self {
        Self {
            server: s.io().local_addr().ok(),
            client: *s.remote_addr(),
        }
    }
}

/// A turn submitted with `conversation_send`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Sent {
    /// The turn ended within the wait.
    Reply {
        turn_id: String,
        reply: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cost_usd: Option<f64>,
    },
    /// It runs on: `conversation_status` follows it.
    Running { status: Running, turn_id: String },
}

/// `"running"`, as `Sent::Running` says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Running {
    Running,
}

/// A conversation's state, for `conversation_status`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Status {
    pub session_id: String,
    /// The execution's state: `idle`, `running`, `waiting`, …
    pub state: String,
    /// What it waits on, while it waits: `confirm`, `budget`, `input`, …
    #[serde(skip_serializing_if = "Option::is_none")]
    pub waiting_on: Option<String>,
    /// The last turn, running or ended: the `turn_id` a `running` answer
    /// named.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_reply: Option<String>,
    pub cost_usd: f64,
    pub turns: u64,
}

/// A core's refusal or failure: the client's model reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreError(pub String);

impl std::fmt::Display for CoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// One answered call: the ledger's `mcp_server.call` row.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CallRecord {
    pub tool: String,
    /// The Theseus session it named or opened, if any.
    pub session_id: Option<String>,
    pub client_name: String,
    pub latency_ms: u64,
    pub ok: bool,
}

/// Why a request was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Why {
    /// The `Host` did not name this listener.
    Host,
    /// A foreign `Origin`.
    Origin,
    /// No key, or a wrong one.
    Key,
    /// Past `requests_per_minute`.
    Rate,
    /// The `admit` check refused the connection's peer.
    Peer,
}

/// A refusal, as reported to the core: at most once a minute per kind.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Refusal {
    pub why: Why,
    pub client: String,
    /// The refused header, clipped (never the key).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Refusals of this kind since the last one reported.
    pub unreported: u64,
}

/// The core, as the server sees it. Its methods may be written as
/// `async fn` in an implementation.
pub trait CoreClient: Send + Sync + 'static {
    /// Open a conversation for `caller` (labelled `mcp <client>`, with the
    /// client's own label after it). Returns its session id.
    fn open(
        &self,
        caller: Caller,
        label: Option<String>,
    ) -> impl Future<Output = Result<String, CoreError>> + Send;

    /// Submit a turn, and wait up to `wait` for its end.
    fn send(
        &self,
        caller: Caller,
        session_id: String,
        text: String,
        wait: Duration,
    ) -> impl Future<Output = Result<Sent, CoreError>> + Send;

    fn status(
        &self,
        caller: Caller,
        session_id: String,
    ) -> impl Future<Output = Result<Status, CoreError>> + Send;

    /// `task.list`'s answer, as JSON.
    fn tasks(
        &self,
        caller: Caller,
        session_id: Option<String>,
    ) -> impl Future<Output = Result<Value, CoreError>> + Send;

    /// `wake.list`'s answer, as JSON.
    fn wakes(
        &self,
        caller: Caller,
        session_id: Option<String>,
    ) -> impl Future<Output = Result<Value, CoreError>> + Send;

    /// A call was answered.
    fn called(&self, _record: &CallRecord) {}

    /// A request was refused.
    fn refused(&self, _refusal: &Refusal) {}
}

/// The server's counters, for health.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Stats {
    pub listening: bool,
    pub port: u16,
    pub sessions: usize,
    /// The client names of the live sessions, sorted, once each.
    pub clients: Vec<String>,
    /// Sessions opened since the start, and the last client to open one.
    pub opened: u64,
    pub last_client: Option<String>,
    pub calls: u64,
    /// Calls whose answer was an error.
    pub errors: u64,
    pub refused: HashMap<Why, u64>,
}

/// A running server. Dropping it does not stop it; `stop` does, and does
/// not wait.
pub struct Server {
    addr: SocketAddr,
    stop: CancellationToken,
    stats: Arc<dyn Fn() -> Stats + Send + Sync>,
}

impl Server {
    /// Bind on `cfg.bind` (loopback only) and serve until `stop`.
    pub async fn bind<C: CoreClient>(cfg: Config, core: Arc<C>) -> io::Result<Server> {
        if !cfg.bind.ip().is_loopback() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "the MCP server binds loopback only (reachability rule); got {}",
                    cfg.bind
                ),
            ));
        }
        if cfg.key.len() < 16 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the MCP server's key must be at least 16 bytes",
            ));
        }
        let listener = tokio::net::TcpListener::bind(cfg.bind).await?;
        let addr = listener.local_addr()?;
        let stop = CancellationToken::new();
        let shared = Arc::new(Shared {
            key_digest: Sha256::digest(cfg.key.as_bytes()).into(),
            bucket: Mutex::new(Bucket::new(cfg.requests_per_minute)),
            cfg,
            addr,
            core,
            sessions: Mutex::new(HashMap::new()),
            opened: AtomicU64::new(0),
            last_client: Mutex::new(None),
            calls: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            refusals: Mutex::new(HashMap::new()),
            stopped: stop.clone(),
        });
        let app = Router::new()
            .route("/mcp", any(mcp::<C>))
            .fallback(not_found)
            .with_state(shared.clone());
        let signal = stop.clone().cancelled_owned();
        tokio::spawn(async move {
            let serve = axum::serve(listener, app.into_make_service_with_connect_info::<Ends>())
                .with_graceful_shutdown(signal);
            if let Err(e) = serve.await {
                tracing::warn!(error = %e, "mcp server: the listener failed");
            }
        });
        tracing::info!(%addr, "mcp server listening (loopback only)");
        Ok(Server {
            addr,
            stop,
            stats: Arc::new(move || shared.stats()),
        })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn stats(&self) -> Stats {
        (self.stats)()
    }

    /// Stop listening. Does not wait for calls in flight.
    pub fn stop(&self) {
        self.stop.cancel();
    }
}

struct Shared<C> {
    cfg: Config,
    addr: SocketAddr,
    core: Arc<C>,
    key_digest: [u8; 32],
    bucket: Mutex<Bucket>,
    sessions: Mutex<HashMap<String, McpSession>>,
    opened: AtomicU64,
    last_client: Mutex<Option<String>>,
    calls: AtomicU64,
    errors: AtomicU64,
    refusals: Mutex<HashMap<Why, Tally>>,
    stopped: CancellationToken,
}

struct McpSession {
    client: Implementation,
    last_seen: Instant,
}

/// One kind of refusal: how many, and when one was last reported.
#[derive(Default)]
struct Tally {
    count: u64,
    reported_at: Option<Instant>,
    unreported: u64,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl<C: CoreClient> Shared<C> {
    fn stats(&self) -> Stats {
        let mut clients: Vec<String> = {
            let sessions = lock(&self.sessions);
            sessions.values().map(|s| s.client.name.clone()).collect()
        };
        clients.sort();
        clients.dedup();
        Stats {
            listening: !self.stopped.is_cancelled(),
            port: self.addr.port(),
            sessions: lock(&self.sessions).len(),
            clients,
            opened: self.opened.load(Ordering::Relaxed),
            last_client: lock(&self.last_client).clone(),
            calls: self.calls.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
            refused: lock(&self.refusals)
                .iter()
                .map(|(w, t)| (*w, t.count))
                .collect(),
        }
    }

    /// Count a refusal; report it if none of its kind was this minute.
    fn refuse(&self, why: Why, ends: Ends, detail: Option<&[u8]>) {
        let report = {
            let mut all = lock(&self.refusals);
            let t = all.entry(why).or_default();
            t.count += 1;
            let due = t
                .reported_at
                .is_none_or(|at| at.elapsed() >= Duration::from_secs(60));
            if due {
                t.reported_at = Some(Instant::now());
                Some(std::mem::take(&mut t.unreported))
            } else {
                t.unreported += 1;
                None
            }
        };
        if let Some(unreported) = report {
            self.core.refused(&Refusal {
                why,
                client: ends.client.to_string(),
                detail: detail.map(|d| clip(&String::from_utf8_lossy(d), 120)),
                unreported,
            });
        }
    }

    /// A `Host` (or an authority) that names this listener: its address or
    /// `localhost`, at its port.
    fn own_host(&self, host: &str) -> bool {
        split_host(host).is_some_and(|(name, port)| {
            port == self.addr.port()
                && (name.eq_ignore_ascii_case("localhost")
                    || name.parse::<IpAddr>().is_ok_and(|ip| ip == self.addr.ip()))
        })
    }

    fn key_ok(&self, headers: &HeaderMap) -> bool {
        let given = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or("");
        let digest: [u8; 32] = Sha256::digest(given.as_bytes()).into();
        // Every byte is compared, whatever the first difference.
        digest
            .iter()
            .zip(self.key_digest.iter())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
    }

    /// The session a request names: known and fresh, or not. A known one
    /// is touched.
    fn session(&self, id: &str) -> Option<Implementation> {
        let mut sessions = lock(&self.sessions);
        let idle = self.cfg.session_idle;
        let fresh = sessions
            .get(id)
            .is_some_and(|s| s.last_seen.elapsed() < idle);
        if !fresh {
            sessions.remove(id);
            return None;
        }
        let s = sessions.get_mut(id)?;
        s.last_seen = Instant::now();
        Some(s.client.clone())
    }

    fn new_session(&self, client: Implementation) -> String {
        let id = format!("{:032x}", rand::random::<u128>());
        self.opened.fetch_add(1, Ordering::Relaxed);
        *lock(&self.last_client) = Some(client.name.clone());
        let mut sessions = lock(&self.sessions);
        let idle = self.cfg.session_idle;
        sessions.retain(|_, s| s.last_seen.elapsed() < idle);
        while sessions.len() >= self.cfg.max_sessions.max(1) {
            let oldest = sessions
                .iter()
                .min_by_key(|(_, s)| s.last_seen)
                .map(|(k, _)| k.clone());
            match oldest {
                Some(k) => sessions.remove(&k),
                None => break,
            };
        }
        sessions.insert(
            id.clone(),
            McpSession {
                client,
                last_seen: Instant::now(),
            },
        );
        id
    }
}

/// `name:port`, `[v6]:port`, or a bare name, whose port is http's 80.
fn split_host(host: &str) -> Option<(&str, u16)> {
    if let Some(rest) = host.strip_prefix('[') {
        let (name, after) = rest.split_once(']')?;
        let port = match after {
            "" => 80,
            p => p.strip_prefix(':')?.parse().ok()?,
        };
        return Some((name, port));
    }
    match host.rsplit_once(':') {
        Some((name, port)) => Some((name, port.parse().ok()?)),
        None => Some((host, 80)),
    }
}

/// An `Origin` that is a page on this machine: `http` or `https`, and a
/// loopback name at any port.
fn loopback_origin(origin: &str) -> bool {
    let Some(rest) = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
    else {
        return false;
    };
    if rest.contains('/') {
        return false;
    }
    split_host(rest).is_some_and(|(name, _)| {
        name.eq_ignore_ascii_case("localhost")
            || name.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
    })
}

/// A token bucket: a minute's worth at most, refilled evenly.
struct Bucket {
    tokens: f64,
    per_sec: f64,
    max: f64,
    at: Instant,
}

impl Bucket {
    fn new(per_minute: u32) -> Self {
        let max = f64::from(per_minute.max(1));
        Self {
            tokens: max,
            per_sec: max / 60.0,
            max,
            at: Instant::now(),
        }
    }

    /// Take one, or say how long until there is one.
    fn take(&mut self) -> Result<(), Duration> {
        let now = Instant::now();
        let since = now.duration_since(self.at).as_secs_f64();
        self.tokens = (self.tokens + since * self.per_sec).min(self.max);
        self.at = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            Ok(())
        } else {
            Err(Duration::from_secs_f64((1.0 - self.tokens) / self.per_sec))
        }
    }
}

fn plain(status: StatusCode, text: &str) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "text/plain")],
        format!("{text}\n"),
    )
        .into_response()
}

/// A JSON-RPC error that is the transport's (no session, a bad body): an
/// HTTP status, and an error object with a null id.
fn rpc_refusal(status: StatusCode, code: i64, message: &str) -> Response {
    let body = jsonrpc::error_response(Value::Null, code, message);
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        body.to_string(),
    )
        .into_response()
}

async fn not_found() -> Response {
    plain(StatusCode::NOT_FOUND, "the MCP endpoint is /mcp")
}

/// Every request to `/mcp`.
async fn mcp<C: CoreClient>(
    State(s): State<Arc<Shared<C>>>,
    ConnectInfo(ends): ConnectInfo<Ends>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(admit) = s.cfg.admit.clone() {
        let verdict = tokio::task::spawn_blocking(move || admit(ends))
            .await
            .unwrap_or_else(|e| Err(format!("the peer check failed: {e}")));
        if let Err(why) = verdict {
            s.refuse(Why::Peer, ends, Some(why.as_bytes()));
            return plain(
                StatusCode::FORBIDDEN,
                "refused: the MCP server serves only the user that runs it",
            );
        }
    }
    // DNS rebinding: a page's requests carry its own site's name, in `Host`
    // or in the target's authority (HTTP/2, or an absolute-form target).
    // Whichever is there must name this listener, and one must be.
    let host = headers.get(header::HOST);
    let authority = uri.authority();
    let named = host.is_some() || authority.is_some();
    let own = host.is_none_or(|h| h.to_str().is_ok_and(|h| s.own_host(h)))
        && authority.is_none_or(|a| s.own_host(a.as_str()));
    if !(named && own) {
        let shown = host
            .map(HeaderValue::as_bytes)
            .or(authority.map(|a| a.as_str().as_bytes()));
        s.refuse(Why::Host, ends, shown);
        return plain(
            StatusCode::FORBIDDEN,
            "refused: the request's Host is not this server's address",
        );
    }
    if let Some(origin) = headers.get(header::ORIGIN) {
        if !origin.to_str().is_ok_and(loopback_origin) {
            s.refuse(Why::Origin, ends, Some(origin.as_bytes()));
            return plain(StatusCode::FORBIDDEN, "refused: a foreign Origin");
        }
    }
    if !s.key_ok(&headers) {
        s.refuse(Why::Key, ends, None);
        return (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, "Bearer")],
            "a key is required, as Authorization: Bearer\n",
        )
            .into_response();
    }
    let limited = lock(&s.bucket).take();
    if let Err(wait) = limited {
        s.refuse(Why::Rate, ends, None);
        let secs = wait.as_secs().max(1).to_string();
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [(header::RETRY_AFTER, secs)],
            "refused: past requests_per_minute\n",
        )
            .into_response();
    }
    let session_header = headers
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    match method {
        Method::POST => post(&s, ends, &headers, session_header, &body).await,
        Method::DELETE => match session_header {
            Some(id) if lock(&s.sessions).remove(&id).is_some() => {
                plain(StatusCode::OK, "the session has ended")
            }
            _ => plain(StatusCode::NOT_FOUND, "no such session"),
        },
        // v1 has no server stream.
        _ => (
            StatusCode::METHOD_NOT_ALLOWED,
            [(header::ALLOW, "POST, DELETE")],
            "this server sends nothing unasked: POST, or DELETE\n",
        )
            .into_response(),
    }
}

async fn post<C: CoreClient>(
    s: &Arc<Shared<C>>,
    ends: Ends,
    headers: &HeaderMap,
    session: Option<String>,
    body: &[u8],
) -> Response {
    let Ok(msg) = serde_json::from_slice::<Value>(body) else {
        return rpc_refusal(StatusCode::BAD_REQUEST, code::PARSE, "the body is not JSON");
    };
    if msg.is_array() {
        return rpc_refusal(
            StatusCode::BAD_REQUEST,
            code::INVALID_REQUEST,
            "JSON-RPC batches are not supported (2025-06-18)",
        );
    }
    let incoming = match jsonrpc::classify(msg).pop() {
        Some(Ok(m)) => m,
        Some(Err(e)) => return rpc_refusal(StatusCode::BAD_REQUEST, code::INVALID_REQUEST, &e),
        None => return rpc_refusal(StatusCode::BAD_REQUEST, code::INVALID_REQUEST, "no message"),
    };
    if let Incoming::Request { id, method, params } = &incoming {
        if method == "initialize" {
            return initialize(s, id.clone(), params);
        }
    }
    let Some(session_id) = session else {
        return rpc_refusal(
            StatusCode::BAD_REQUEST,
            code::INVALID_REQUEST,
            "Mcp-Session-Id is required after initialize",
        );
    };
    let Some(client) = s.session(&session_id) else {
        return rpc_refusal(
            StatusCode::NOT_FOUND,
            code::INVALID_REQUEST,
            "no such session: initialize again",
        );
    };
    if let Some(v) = headers.get("mcp-protocol-version") {
        if !v.to_str().is_ok_and(crate::supports) {
            return rpc_refusal(
                StatusCode::BAD_REQUEST,
                code::INVALID_REQUEST,
                "an unsupported MCP-Protocol-Version",
            );
        }
    }
    let Incoming::Request { id, method, params } = incoming else {
        // A notification (`initialized`, `cancelled`), or an answer: this
        // server sends no requests, and a cancelled call's wait ends on its
        // own.
        return StatusCode::ACCEPTED.into_response();
    };
    let caller = Caller {
        client_name: client.name,
        client_version: client.version,
        mcp_session: session_id,
        ends,
    };
    let answer = match method.as_str() {
        "ping" => jsonrpc::response(id, json!({})),
        "tools/list" => jsonrpc::response(id, json!({ "tools": tools() })),
        "tools/call" => call(s, caller, id, &params).await,
        _ => jsonrpc::error_response(
            id,
            code::METHOD_NOT_FOUND,
            &format!("this server offers no {}", clip(&method, 80)),
        ),
    };
    json_answer(answer, None)
}

fn json_answer(answer: Value, session: Option<&str>) -> Response {
    let mut r = (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        answer.to_string(),
    )
        .into_response();
    if let Some(id) = session.and_then(|s| HeaderValue::from_str(s).ok()) {
        r.headers_mut().insert("mcp-session-id", id);
    }
    r
}

fn initialize<C: CoreClient>(s: &Arc<Shared<C>>, id: Value, params: &Value) -> Response {
    let asked = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or("");
    let version = if crate::supports(asked) {
        asked
    } else {
        crate::LATEST_PROTOCOL_VERSION
    };
    let client: Implementation = params
        .get("clientInfo")
        .cloned()
        .and_then(|c| serde_json::from_value(c).ok())
        .unwrap_or_default();
    let session = s.new_session(Implementation {
        name: clip(&client.name, 64),
        version: clip(&client.version, 32),
        ..Implementation::default()
    });
    let mut result = json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": s.cfg.server_info,
    });
    if let Some(i) = &s.cfg.instructions {
        result["instructions"] = json!(i);
    }
    json_answer(jsonrpc::response(id, result), Some(&session))
}

/// The five tools.
fn tools() -> Value {
    let session_id =
        json!({ "type": "string", "description": "A session id from conversation_open (ses_…)." });
    json!([
        {
            "name": "conversation_open",
            "title": "Open a Theseus conversation",
            "description": "Open a new Theseus conversation, and return its session_id for conversation_send.",
            "inputSchema": {
                "type": "object",
                "properties": { "label": { "type": "string", "description": "A short label, after `mcp <client>`." } }
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": false, "openWorldHint": false }
        },
        {
            "name": "conversation_send",
            "title": "Send a message to a Theseus conversation",
            "description": "Send text as the next turn of a conversation, and wait for its reply (60 s unless wait_secs says otherwise). A turn still going after the wait answers {status: \"running\", turn_id}; follow it with conversation_status. Acting calls in the turn wait for the operator's approval.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": session_id,
                    "text": { "type": "string" },
                    "wait_secs": { "type": "integer", "minimum": 0, "maximum": 120 }
                },
                "required": ["session_id", "text"]
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": false, "openWorldHint": true }
        },
        {
            "name": "conversation_status",
            "title": "A Theseus conversation's state",
            "description": "A conversation's state, what it waits on, its last reply, and its cost.",
            "inputSchema": {
                "type": "object",
                "properties": { "session_id": session_id },
                "required": ["session_id"]
            },
            "annotations": { "readOnlyHint": true, "openWorldHint": false }
        },
        {
            "name": "task_list",
            "title": "Theseus's tasks",
            "description": "Theseus's tasks, the newest first, or only one session's.",
            "inputSchema": { "type": "object", "properties": { "session_id": session_id } },
            "annotations": { "readOnlyHint": true, "openWorldHint": false }
        },
        {
            "name": "wake_list",
            "title": "Theseus's pending wakes",
            "description": "Theseus's pending wakes, the soonest first, or only one session's.",
            "inputSchema": { "type": "object", "properties": { "session_id": session_id } },
            "annotations": { "readOnlyHint": true, "openWorldHint": false }
        }
    ])
}

/// What a tool answered: JSON for the client, or the core's refusal.
type Answered = Result<Value, CoreError>;

async fn call<C: CoreClient>(
    s: &Arc<Shared<C>>,
    caller: Caller,
    id: Value,
    params: &Value,
) -> Value {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let text = |k: &str| args.get(k).and_then(Value::as_str).map(String::from);
    let need = |k: &str| text(k).ok_or_else(|| CoreError(format!("{name} needs {k}")));
    let client_name = caller.client_name.clone();
    let started = Instant::now();
    let span = tracing::info_span!("mcp_server.call", tool = name, client = %client_name);
    let _entered = span.enter();
    let core = &s.core;
    let (session_id, answered): (Option<String>, Answered) = match name {
        "conversation_open" => match core.open(caller, text("label")).await {
            Ok(sid) => (Some(sid.clone()), Ok(json!({ "session_id": sid }))),
            Err(e) => (None, Err(e)),
        },
        "conversation_send" => match (need("session_id"), need("text")) {
            (Ok(sid), Ok(t)) => {
                let wait = args
                    .get("wait_secs")
                    .and_then(Value::as_u64)
                    .map_or(s.cfg.default_wait, Duration::from_secs)
                    .min(s.cfg.max_wait);
                let sent = core.send(caller, sid.clone(), t, wait).await;
                (Some(sid), sent.map(|r| json!(r)))
            }
            (Err(e), _) | (_, Err(e)) => (None, Err(e)),
        },
        "conversation_status" => match need("session_id") {
            Ok(sid) => {
                let st = core.status(caller, sid.clone()).await;
                (Some(sid), st.map(|r| json!(r)))
            }
            Err(e) => (None, Err(e)),
        },
        "task_list" => {
            let sid = text("session_id");
            (sid.clone(), core.tasks(caller, sid).await)
        }
        "wake_list" => {
            let sid = text("session_id");
            (sid.clone(), core.wakes(caller, sid).await)
        }
        _ => {
            return jsonrpc::error_response(
                id,
                code::INVALID_PARAMS,
                &format!("Unknown tool: {}", clip(name, 80)),
            )
        }
    };
    let ok = answered.is_ok();
    s.calls.fetch_add(1, Ordering::Relaxed);
    if !ok {
        s.errors.fetch_add(1, Ordering::Relaxed);
    }
    s.core.called(&CallRecord {
        tool: name.to_string(),
        session_id,
        client_name,
        latency_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        ok,
    });
    let result = match answered {
        Ok(v) => json!({
            "content": [{ "type": "text", "text": v.to_string() }],
            "structuredContent": v
        }),
        Err(e) => json!({
            "content": [{ "type": "text", "text": e.0 }],
            "isError": true
        }),
    };
    jsonrpc::response(id, result)
}

// ---- the fake core ----

/// A core for tests and the example server: conversations in memory, each
/// turn answered `you said: <text>` after `turn_ms`.
pub struct FakeCore {
    turn_ms: AtomicU64,
    state: Mutex<FakeState>,
    calls: Mutex<Vec<CallRecord>>,
    refusals: Mutex<Vec<Refusal>>,
    /// Every caller the core has seen, in order.
    callers: Mutex<Vec<Caller>>,
}

#[derive(Default)]
struct FakeState {
    next: u64,
    sessions: HashMap<String, FakeSession>,
}

struct FakeSession {
    label: String,
    turns: Vec<FakeTurn>,
}

struct FakeTurn {
    turn_id: String,
    reply: String,
    done_at: tokio::time::Instant,
}

impl Default for FakeCore {
    fn default() -> Self {
        Self::new(Duration::from_millis(20))
    }
}

impl FakeCore {
    pub fn new(turn: Duration) -> Self {
        Self {
            turn_ms: AtomicU64::new(u64::try_from(turn.as_millis()).unwrap_or(u64::MAX)),
            state: Mutex::new(FakeState::default()),
            calls: Mutex::new(Vec::new()),
            refusals: Mutex::new(Vec::new()),
            callers: Mutex::new(Vec::new()),
        }
    }

    pub fn set_turn(&self, turn: Duration) {
        self.turn_ms.store(
            u64::try_from(turn.as_millis()).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
    }

    /// Each session's label (`mcp <client>`, then the client's label).
    pub fn labels(&self) -> Vec<String> {
        let st = lock(&self.state);
        let mut l: Vec<String> = st.sessions.values().map(|s| s.label.clone()).collect();
        l.sort();
        l
    }

    pub fn calls(&self) -> Vec<CallRecord> {
        lock(&self.calls).clone()
    }

    pub fn refusals(&self) -> Vec<Refusal> {
        lock(&self.refusals).clone()
    }

    pub fn callers(&self) -> Vec<Caller> {
        lock(&self.callers).clone()
    }

    /// Wait until a session's last turn has ended.
    pub async fn turn_done(&self, session_id: &str) {
        let done_at = lock(&self.state)
            .sessions
            .get(session_id)
            .and_then(|s| s.turns.last())
            .map(|t| t.done_at);
        if let Some(at) = done_at {
            tokio::time::sleep_until(at).await;
        }
    }

    fn seen(&self, caller: Caller) {
        lock(&self.callers).push(caller);
    }
}

impl CoreClient for FakeCore {
    async fn open(&self, caller: Caller, label: Option<String>) -> Result<String, CoreError> {
        let mut full = format!("mcp {}", caller.client_name);
        if let Some(l) = label.filter(|l| !l.trim().is_empty()) {
            full.push(' ');
            full.push_str(l.trim());
        }
        self.seen(caller);
        let mut st = lock(&self.state);
        st.next += 1;
        let id = format!("ses_fake{:04}", st.next);
        st.sessions.insert(
            id.clone(),
            FakeSession {
                label: full,
                turns: Vec::new(),
            },
        );
        Ok(id)
    }

    async fn send(
        &self,
        caller: Caller,
        session_id: String,
        text: String,
        wait: Duration,
    ) -> Result<Sent, CoreError> {
        self.seen(caller);
        let turn = Duration::from_millis(self.turn_ms.load(Ordering::Relaxed));
        let (turn_id, done_at, reply) = {
            let mut st = lock(&self.state);
            st.next += 1;
            let turn_id = format!("trn_fake{:04}", st.next);
            let session = st
                .sessions
                .get_mut(&session_id)
                .ok_or_else(|| CoreError(format!("no session {}", clip(&session_id, 40))))?;
            let now = tokio::time::Instant::now();
            if session.turns.last().is_some_and(|t| t.done_at > now) {
                return Err(CoreError(
                    "a turn is running in this session: follow it with conversation_status".into(),
                ));
            }
            let reply = format!("you said: {text}");
            let done_at = now + turn;
            session.turns.push(FakeTurn {
                turn_id: turn_id.clone(),
                reply: reply.clone(),
                done_at,
            });
            (turn_id, done_at, reply)
        };
        let deadline = tokio::time::Instant::now() + wait;
        if done_at <= deadline {
            tokio::time::sleep_until(done_at).await;
            Ok(Sent::Reply {
                turn_id,
                reply,
                cost_usd: Some(0.001),
            })
        } else {
            tokio::time::sleep_until(deadline).await;
            Ok(Sent::Running {
                status: Running::Running,
                turn_id,
            })
        }
    }

    async fn status(&self, caller: Caller, session_id: String) -> Result<Status, CoreError> {
        self.seen(caller);
        let st = lock(&self.state);
        let session = st
            .sessions
            .get(&session_id)
            .ok_or_else(|| CoreError(format!("no session {}", clip(&session_id, 40))))?;
        let now = tokio::time::Instant::now();
        let running = session.turns.last().is_some_and(|t| t.done_at > now);
        let done: Vec<&FakeTurn> = session.turns.iter().filter(|t| t.done_at <= now).collect();
        Ok(Status {
            session_id,
            state: if running { "running" } else { "idle" }.into(),
            waiting_on: None,
            turn_id: session.turns.last().map(|t| t.turn_id.clone()),
            last_reply: done.last().map(|t| t.reply.clone()),
            cost_usd: 0.001 * done.len() as f64,
            turns: done.len() as u64,
        })
    }

    async fn tasks(&self, caller: Caller, session_id: Option<String>) -> Result<Value, CoreError> {
        self.seen(caller);
        Ok(json!({ "tasks": [], "session_id": session_id }))
    }

    async fn wakes(&self, caller: Caller, session_id: Option<String>) -> Result<Value, CoreError> {
        self.seen(caller);
        Ok(json!({ "wakes": [], "session_id": session_id }))
    }

    fn called(&self, record: &CallRecord) {
        lock(&self.calls).push(record.clone());
    }

    fn refused(&self, refusal: &Refusal) {
        lock(&self.refusals).push(refusal.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_and_origins() {
        assert_eq!(split_host("127.0.0.1:7434"), Some(("127.0.0.1", 7434)));
        assert_eq!(split_host("[::1]:7434"), Some(("::1", 7434)));
        assert_eq!(split_host("localhost"), Some(("localhost", 80)));
        assert_eq!(split_host("a:b"), None);
        for o in [
            "http://localhost:6274",
            "http://127.0.0.1",
            "https://127.0.0.2:9",
            "http://[::1]:3000",
            "http://LOCALHOST:1",
        ] {
            assert!(loopback_origin(o), "{o}");
        }
        for o in [
            "null",
            "http://evil.example",
            "http://localhost.evil.example:80",
            "http://127.0.0.1.nip.io",
            "file://",
            "http://127.0.0.1:7434/page",
            "ftp://127.0.0.1",
            "",
        ] {
            assert!(!loopback_origin(o), "{o}");
        }
    }

    #[test]
    fn the_bucket_holds_a_minute_and_refills() {
        let mut b = Bucket::new(3);
        assert!(b.take().is_ok() && b.take().is_ok() && b.take().is_ok());
        let wait = b.take().unwrap_err();
        assert!(
            wait > Duration::from_secs(15) && wait <= Duration::from_secs(20),
            "{wait:?}"
        );
        // Twenty seconds later, one more.
        b.at -= Duration::from_secs(20);
        assert!(b.take().is_ok());
        assert!(b.take().is_err());
    }

    #[test]
    fn sent_and_status_as_json() {
        let r = Sent::Reply {
            turn_id: "trn_1".into(),
            reply: "hi".into(),
            cost_usd: None,
        };
        assert_eq!(json!(r), json!({"turn_id": "trn_1", "reply": "hi"}));
        let r = Sent::Running {
            status: Running::Running,
            turn_id: "trn_2".into(),
        };
        assert_eq!(json!(r), json!({"status": "running", "turn_id": "trn_2"}));
        let st = Status {
            session_id: "ses_1".into(),
            state: "idle".into(),
            ..Status::default()
        };
        assert_eq!(
            json!(st),
            json!({"session_id": "ses_1", "state": "idle", "cost_usd": 0.0, "turns": 0})
        );
        assert!(!format!(
            "{:?}",
            Config::new("127.0.0.1:0".parse().unwrap(), "sixteen-byte-key!")
        )
        .contains("sixteen"));
    }
}
