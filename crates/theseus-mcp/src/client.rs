//! The MCP client (step 36a): one connection to one server.
//!
//! **Transports.**
//! - *stdio*: a server process ([`stdio_command`], then
//!   [`Transport::Stdio`]); one JSON message per line on its stdin and
//!   stdout. Its stderr is drained into a capped log and a short tail that
//!   names a crash. Closing it closes its stdin and sends SIGTERM to its
//!   process group, without waiting (a shutdown never waits).
//! - *pipes*: the same framing over any pair of pipes: the fake in this
//!   process, in tests.
//! - *streamable HTTP*: one message per POST, answered as JSON or as a
//!   stream of server-sent events. The session id the server gives at
//!   `initialize` rides on every later request, with the negotiated
//!   revision (`MCP-Protocol-Version`). A 404 for that session makes a new
//!   `initialize`, once, then the request goes again ([`Event::Reinitialized`]).
//!   A stream that ends before its answer is resumed with a GET and
//!   `Last-Event-ID` when it gave an event id (resumable streams, and
//!   2025-11-25's servers that close a stream on purpose). The server's own
//!   stream (a GET) carries what it sends unasked, such as `list_changed`.
//!
//! **Calls.** Each request waits for its answer up to a timeout. A call
//! that times out, or whose future is dropped (a cancel, or a `/stop`
//! aborting its task), sends `notifications/cancelled` and forgets the
//! request; a late answer is dropped. `initialize` is never cancelled. A
//! call whose connection ends gets [`Error::Closed`], whose outcome is
//! unknown. The server's own requests are answered at once: `ping`, and
//! "method not found" for anything else, since v1 declares no sampling,
//! elicitation, or roots.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde_json::{json, Value};
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt,
};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::AbortHandle;
use tokio_util::sync::CancellationToken;

use crate::jsonrpc::{self, Incoming, RpcError};
use crate::sse;
use crate::types::{
    CallToolResult, GetPromptResult, Implementation, InitializeResult, Prompt, ServerCapabilities,
    Tool,
};

/// How a client behaves. The defaults are the design's (§2.1).
#[derive(Debug, Clone)]
pub struct Options {
    /// Who the client says it is in `initialize`.
    pub client_info: Implementation,
    /// The longest wait for `initialize`, a list's page, a prompt, or a ping.
    pub request_timeout: Duration,
    /// The longest wait for a tool call that names none: 110 s, under the
    /// core's 120 s in-process deadline.
    pub call_timeout: Duration,
    /// A list of more pages than this is refused: a server that pages
    /// forever.
    pub max_pages: usize,
    /// A message (a line, a JSON body, an event) larger than this is
    /// refused.
    pub max_message_bytes: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            client_info: Implementation::new("theseus", env!("CARGO_PKG_VERSION")),
            request_timeout: Duration::from_secs(30),
            call_timeout: Duration::from_secs(110),
            max_pages: 100,
            max_message_bytes: 16 << 20,
        }
    }
}

/// How to reach a server.
pub enum Transport {
    Stdio(StdioServer),
    /// Newline-delimited JSON over any pipes: a server in this process.
    Pipes {
        reader: Box<dyn AsyncRead + Send + Unpin>,
        writer: Box<dyn AsyncWrite + Send + Unpin>,
    },
    Http(HttpTarget),
}

impl Transport {
    pub fn pipes(
        reader: impl AsyncRead + Send + Unpin + 'static,
        writer: impl AsyncWrite + Send + Unpin + 'static,
    ) -> Self {
        Transport::Pipes {
            reader: Box::new(reader),
            writer: Box::new(writer),
        }
    }
}

/// A server process, started with its stdin, stdout, and stderr piped
/// ([`stdio_command`]).
pub struct StdioServer {
    pub child: tokio::process::Child,
    /// Where its stderr goes, up to a cap; without one, only a short tail
    /// is kept, to name a crash.
    pub stderr_log: Option<StderrLog>,
}

#[derive(Debug, Clone)]
pub struct StderrLog {
    pub path: PathBuf,
    pub cap_bytes: u64,
}

/// A server's command, ready to spawn: stdin, stdout, and stderr piped, and
/// in a process group of its own, so a stop reaches the children it starts
/// (`npx` starts node). `kill_on_drop` stays off: a stop sends SIGTERM to
/// the group, and does not wait. A board adds the environment and the
/// working directory, and spawns it through its registry.
pub fn stdio_command(argv: &[String]) -> Option<tokio::process::Command> {
    let (program, args) = argv.split_first()?;
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .process_group(0);
    Some(cmd)
}

/// A remote server, over streamable HTTP.
#[derive(Clone)]
pub struct HttpTarget {
    pub url: String,
    /// Sent as `Authorization: Bearer …`; never logged, never shown.
    pub bearer: Option<String>,
    /// A connect that takes longer fails (on some hosts a connect to a
    /// loopback port nothing listens on hangs instead of being refused).
    pub connect_timeout: Duration,
    /// Open the server's own stream after `initialize`, for what it sends
    /// unasked.
    pub server_stream: bool,
}

impl HttpTarget {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            bearer: None,
            connect_timeout: Duration::from_secs(10),
            server_stream: true,
        }
    }
}

impl std::fmt::Debug for HttpTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpTarget")
            .field("url", &self.url)
            .field("bearer", &self.bearer.as_ref().map(|_| "(set)"))
            .field("connect_timeout", &self.connect_timeout)
            .field("server_stream", &self.server_stream)
            .finish()
    }
}

/// What the server said about itself, at the last `initialize`.
#[derive(Debug, Clone, PartialEq)]
pub struct ServerInfo {
    /// The negotiated revision.
    pub protocol_version: String,
    pub server: Implementation,
    pub capabilities: ServerCapabilities,
    pub instructions: Option<String>,
}

/// What the server sent unasked, and what became of the connection.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// List the tools again: they apply from the next turn (36b).
    ToolListChanged,
    PromptListChanged,
    ResourceListChanged,
    Progress {
        token: Value,
        progress: f64,
        total: Option<f64>,
        message: Option<String>,
    },
    /// `notifications/message`.
    Log {
        level: String,
        logger: Option<String>,
        data: Value,
    },
    /// Any other notification, as sent.
    Notification {
        method: String,
        params: Value,
    },
    /// HTTP: the server forgot the session (404), so a new `initialize`
    /// made another. The server may have changed: list again.
    Reinitialized,
    /// The connection ended: a stdio server's stdout closed (it exited or
    /// crashed), or the client was closed. Every waiting call failed with
    /// this reason.
    Closed {
        reason: String,
    },
}

impl Event {
    fn from_notification(method: String, params: Value) -> Self {
        let s = |k: &str| params.get(k).and_then(Value::as_str).map(String::from);
        match method.as_str() {
            "notifications/tools/list_changed" => Event::ToolListChanged,
            "notifications/prompts/list_changed" => Event::PromptListChanged,
            "notifications/resources/list_changed" => Event::ResourceListChanged,
            "notifications/progress" => Event::Progress {
                token: params.get("progressToken").cloned().unwrap_or(Value::Null),
                progress: params
                    .get("progress")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0),
                total: params.get("total").and_then(Value::as_f64),
                message: s("message"),
            },
            "notifications/message" => Event::Log {
                level: s("level").unwrap_or_else(|| "info".into()),
                logger: s("logger"),
                data: params.get("data").cloned().unwrap_or(Value::Null),
            },
            _ => Event::Notification { method, params },
        }
    }
}

/// The server's notifications and the connection's end, in order.
pub type Events = mpsc::UnboundedReceiver<Event>;

#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    /// Nothing was sent: the server could not be reached. Safe to retry.
    #[error("the server is unreachable: {0}")]
    Unreachable(String),
    /// No answer in time. The request was cancelled; whether it ran is
    /// unknown.
    #[error("{method} got no answer within {after:?}, and was cancelled")]
    Timeout { method: String, after: Duration },
    /// The connection ended while the call waited; whether it ran is
    /// unknown.
    #[error("the connection to the server closed: {0}")]
    Closed(String),
    /// The server answered with a JSON-RPC error.
    #[error("the server answered error {code}: {message}")]
    Rpc {
        code: i64,
        message: String,
        data: Value,
    },
    #[error("the server answered protocol revision {answered:?}, which this client does not speak (it offered {offered})")]
    UnsupportedVersion { offered: String, answered: String },
    #[error("HTTP {status}: {body}")]
    Http { status: u16, body: String },
    #[error("the server broke the protocol: {0}")]
    Protocol(String),
    /// HTTP: the server ended the session the request carried (inside the
    /// client, where it makes a new `initialize`).
    #[error("the server ended MCP session {0:?}")]
    SessionExpired(String),
}

impl Error {
    /// Whether the request may have run at the server: after a timeout, or
    /// a connection lost while it waited. A non-repeatable call that ends
    /// this way is `outcome_unknown`, and nothing runs it again.
    pub fn outcome_unknown(&self) -> bool {
        matches!(self, Error::Timeout { .. } | Error::Closed(_))
    }
}

impl From<RpcError> for Error {
    fn from(e: RpcError) -> Self {
        Error::Rpc {
            code: e.code,
            message: e.message,
            data: e.data,
        }
    }
}

/// A tool call's own settings.
#[derive(Debug, Clone, Default)]
pub struct CallOptions {
    /// Instead of [`Options::call_timeout`].
    pub timeout: Option<Duration>,
    /// Asks the server for `notifications/progress` under this token.
    pub progress_token: Option<Value>,
}

/// A connection to one server. Clones share it; the last one dropped
/// closes it.
#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

struct Inner {
    shared: Arc<Shared>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.shared.shut("the client was dropped");
    }
}

impl Client {
    /// Connect, and run the handshake: `initialize` (the revision is
    /// negotiated; the client declares no capabilities), then
    /// `notifications/initialized`. A server that answers a revision this
    /// client does not speak is refused, and its connection closed.
    pub async fn connect(transport: Transport, opts: Options) -> Result<(Client, Events), Error> {
        let (events, rx) = mpsc::unbounded_channel();
        let shared = match transport {
            Transport::Stdio(s) => start_stdio(s, opts, events)?,
            Transport::Pipes { reader, writer } => start_pipes(reader, writer, None, opts, events),
            Transport::Http(t) => {
                Arc::new(Shared::new(opts, events, None, Some(Http::new(t)?), None))
            }
        };
        let client = Client {
            inner: Arc::new(Inner {
                shared: shared.clone(),
            }),
        };
        let info = initialize(&shared).await?;
        *shared
            .server
            .write()
            .unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(info));
        if shared.http.as_ref().is_some_and(|h| h.server_stream) {
            start_server_stream(&shared);
        }
        Ok((client, rx))
    }

    fn shared(&self) -> &Arc<Shared> {
        &self.inner.shared
    }

    /// What the server said at the last `initialize`.
    pub fn server_info(&self) -> Arc<ServerInfo> {
        self.shared()
            .server
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .expect("connect sets the server's info before it returns")
    }

    /// A stdio server's process id (and process group).
    pub fn pid(&self) -> Option<u32> {
        self.shared().process.as_ref().map(|p| p.pid)
    }

    /// HTTP: the session the server gave.
    pub fn session_id(&self) -> Option<String> {
        self.shared().http.as_ref().and_then(Http::session)
    }

    /// Why the connection closed, once it has.
    pub fn closed(&self) -> Option<String> {
        self.shared().lock().closed.clone()
    }

    /// Requests waiting for their answers.
    pub fn in_flight(&self) -> usize {
        self.shared().lock().pending.len()
    }

    /// Every tool, over every page. A malformed entry is skipped, and
    /// logged.
    pub async fn list_tools(&self) -> Result<Vec<Tool>, Error> {
        let raw = self.list_all("tools/list", "tools").await?;
        Ok(parse_each(raw, "tool"))
    }

    /// Every prompt, over every page.
    pub async fn list_prompts(&self) -> Result<Vec<Prompt>, Error> {
        let raw = self.list_all("prompts/list", "prompts").await?;
        Ok(parse_each(raw, "prompt"))
    }

    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<CallToolResult, Error> {
        self.call_tool_with(name, arguments, &CallOptions::default())
            .await
    }

    pub async fn call_tool_with(
        &self,
        name: &str,
        arguments: Value,
        call: &CallOptions,
    ) -> Result<CallToolResult, Error> {
        let shared = self.shared();
        let mut params = json!({ "name": name });
        if !arguments.is_null() {
            params["arguments"] = arguments;
        }
        if let Some(t) = &call.progress_token {
            params["_meta"] = json!({ "progressToken": t });
        }
        let timeout = call.timeout.unwrap_or(shared.opts.call_timeout);
        let v = request(shared, "tools/call", params, timeout).await?;
        serde_json::from_value(v)
            .map_err(|e| Error::Protocol(format!("an unreadable tools/call answer: {e}")))
    }

    pub async fn get_prompt(
        &self,
        name: &str,
        arguments: &BTreeMap<String, String>,
    ) -> Result<GetPromptResult, Error> {
        let shared = self.shared();
        let mut params = json!({ "name": name });
        if !arguments.is_empty() {
            params["arguments"] = json!(arguments);
        }
        let v = request(shared, "prompts/get", params, shared.opts.request_timeout).await?;
        serde_json::from_value(v)
            .map_err(|e| Error::Protocol(format!("an unreadable prompts/get answer: {e}")))
    }

    /// The round trip of one `ping`.
    pub async fn ping(&self) -> Result<Duration, Error> {
        let shared = self.shared();
        let start = Instant::now();
        request(shared, "ping", Value::Null, shared.opts.request_timeout).await?;
        Ok(start.elapsed())
    }

    /// Any request, answered with its raw result.
    pub async fn request(&self, method: &str, params: Value) -> Result<Value, Error> {
        let shared = self.shared();
        request(shared, method, params, shared.opts.request_timeout).await
    }

    /// Send SIGTERM to a stdio server's process group, and do not wait.
    pub fn terminate(&self) {
        if let Some(p) = &self.shared().process {
            p.signal(libc::SIGTERM);
        }
    }

    /// Send SIGKILL to a stdio server's process group.
    pub fn kill(&self) {
        if let Some(p) = &self.shared().process {
            p.signal(libc::SIGKILL);
        }
    }

    /// End the connection: an HTTP session is ended with a DELETE (5 s at
    /// most); a stdio server's stdin closes and its group gets SIGTERM.
    /// Every waiting call fails.
    pub async fn close(self) {
        let shared = self.shared().clone();
        if shared.http.is_some() {
            delete_session(&shared).await;
        }
        shared.shut("closed by the client");
    }

    async fn list_all(&self, method: &str, key: &str) -> Result<Vec<Value>, Error> {
        let shared = self.shared();
        let mut items = Vec::new();
        let mut cursor: Option<String> = None;
        let mut seen = HashSet::new();
        for _ in 0..shared.opts.max_pages.max(1) {
            let params = match &cursor {
                Some(c) => json!({ "cursor": c }),
                None => Value::Null,
            };
            let mut page = request(shared, method, params, shared.opts.request_timeout).await?;
            match page.get_mut(key) {
                Some(Value::Array(a)) => items.append(a),
                None | Some(Value::Null) => {}
                Some(_) => return Err(Error::Protocol(format!("{method}: `{key}` is not a list"))),
            }
            match page.get("nextCursor").and_then(Value::as_str) {
                Some(c) if !c.is_empty() => {
                    if !seen.insert(c.to_string()) {
                        return Err(Error::Protocol(format!(
                            "{method}: the server gave the same cursor twice"
                        )));
                    }
                    cursor = Some(c.to_string());
                }
                _ => return Ok(items),
            }
        }
        Err(Error::Protocol(format!(
            "{method}: more than {} pages",
            shared.opts.max_pages
        )))
    }
}

fn parse_each<T: serde::de::DeserializeOwned>(raw: Vec<Value>, what: &str) -> Vec<T> {
    raw.into_iter()
        .filter_map(|v| match serde_json::from_value(v) {
            Ok(t) => Some(t),
            Err(e) => {
                tracing::warn!(error = %e, "mcp: a malformed {what} in the server's list, skipped");
                None
            }
        })
        .collect()
}

/// What every task of one connection shares.
struct Shared {
    opts: Options,
    state: Mutex<State>,
    events: mpsc::UnboundedSender<Event>,
    /// stdio and pipes: the lines for the writer task.
    pipe: Option<mpsc::UnboundedSender<String>>,
    http: Option<Http>,
    /// Cancelled once the connection ends: every task stops.
    stop: CancellationToken,
    next_id: AtomicI64,
    server: RwLock<Option<Arc<ServerInfo>>>,
    process: Option<Proc>,
}

#[derive(Default)]
struct State {
    pending: HashMap<i64, Waiter>,
    closed: Option<String>,
}

struct Waiter {
    tx: oneshot::Sender<Result<Value, Error>>,
    /// HTTP: the request carrying the call, stopped when the call is.
    task: Option<AbortHandle>,
}

impl Shared {
    fn new(
        opts: Options,
        events: mpsc::UnboundedSender<Event>,
        pipe: Option<mpsc::UnboundedSender<String>>,
        http: Option<Http>,
        process: Option<Proc>,
    ) -> Self {
        Self {
            opts,
            state: Mutex::new(State::default()),
            events,
            pipe,
            http,
            stop: CancellationToken::new(),
            next_id: AtomicI64::new(1),
            server: RwLock::new(None),
            process,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn is_pending(&self, id: i64) -> bool {
        self.lock().pending.contains_key(&id)
    }

    fn fail(&self, id: i64, e: Error) {
        let waiter = self.lock().pending.remove(&id);
        if let Some(w) = waiter {
            let _ = w.tx.send(Err(e));
        }
    }

    fn event(&self, e: Event) {
        let _ = self.events.send(e);
    }

    /// Send one message. `carries` names the request it is, whose waiter
    /// fails if it cannot be sent.
    fn send(self: &Arc<Self>, msg: Value, carries: Option<i64>) {
        if let Some(pipe) = &self.pipe {
            if pipe.send(msg.to_string()).is_err() {
                if let Some(id) = carries {
                    let why = self.lock().closed.clone();
                    self.fail(
                        id,
                        Error::Closed(why.unwrap_or_else(|| "the connection closed".into())),
                    );
                }
            }
            return;
        }
        if self.http.is_none() {
            return;
        }
        // A call dropped outside a runtime cannot send its cancel; there is
        // nothing left to send it on.
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            if let Some(id) = carries {
                self.fail(id, Error::Closed("no runtime to send on".into()));
            }
            return;
        };
        let me = self.clone();
        let task = rt.spawn(async move { exchange(me, msg, carries).await });
        if let Some(id) = carries {
            if let Some(w) = self.lock().pending.get_mut(&id) {
                w.task = Some(task.abort_handle());
            }
        }
    }

    /// Route one message from the server.
    fn dispatch(self: &Arc<Self>, v: Value) {
        for m in jsonrpc::classify(v) {
            match m {
                Ok(Incoming::Response { id, outcome }) => {
                    let waiter =
                        jsonrpc::id_number(&id).and_then(|n| self.lock().pending.remove(&n));
                    match waiter {
                        Some(w) => {
                            let _ = w.tx.send(outcome.map_err(Error::from));
                        }
                        None => tracing::debug!(%id, "mcp: an answer to no waiting call, dropped"),
                    }
                }
                Ok(Incoming::Notification { method, params }) => {
                    self.event(Event::from_notification(method, params));
                }
                Ok(Incoming::Request { id, method, .. }) => {
                    let answer = if method == "ping" {
                        jsonrpc::response(id, json!({}))
                    } else {
                        jsonrpc::error_response(
                            id,
                            jsonrpc::code::METHOD_NOT_FOUND,
                            &format!("this client offers no {}", jsonrpc::clip(&method, 80)),
                        )
                    };
                    self.send(answer, None);
                }
                Err(e) => {
                    tracing::warn!(error = %e, "mcp: an unreadable message from the server, skipped")
                }
            }
        }
    }

    /// The connection ended: every waiting call fails with `reason`, once.
    fn close(&self, reason: String) {
        let waiters = {
            let mut st = self.lock();
            if st.closed.is_some() {
                return;
            }
            st.closed = Some(reason.clone());
            std::mem::take(&mut st.pending)
        };
        for (_, w) in waiters {
            if let Some(t) = w.task {
                t.abort();
            }
            let _ = w.tx.send(Err(Error::Closed(reason.clone())));
        }
        self.event(Event::Closed { reason });
        self.stop.cancel();
    }

    /// Close, and send a stdio server's group SIGTERM.
    fn shut(&self, reason: &str) {
        self.close(reason.into());
        if let Some(p) = &self.process {
            p.signal(libc::SIGTERM);
        }
    }
}

/// One request: send it, and wait for its answer.
async fn request(
    shared: &Arc<Shared>,
    method: &str,
    params: Value,
    timeout: Duration,
) -> Result<Value, Error> {
    let id = shared.next_id.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = oneshot::channel();
    {
        let mut st = shared.lock();
        if let Some(why) = &st.closed {
            return Err(Error::Closed(why.clone()));
        }
        st.pending.insert(id, Waiter { tx, task: None });
    }
    let mut call = Outstanding {
        shared,
        id,
        // The spec forbids cancelling `initialize`.
        cancel: method != "initialize",
        live: true,
    };
    shared.send(jsonrpc::request(id, method, params), Some(id));
    match tokio::time::timeout(timeout, rx).await {
        Ok(Ok(answer)) => {
            call.live = false;
            answer
        }
        Ok(Err(_)) => {
            call.live = false;
            Err(Error::Closed("the call's answer was lost".into()))
        }
        Err(_) => {
            call.end(&format!("no answer within {} ms", timeout.as_millis()));
            Err(Error::Timeout {
                method: method.into(),
                after: timeout,
            })
        }
    }
}

/// A request still waiting. Dropped while it waits (the caller stopped
/// waiting: a cancel, a `/stop`), it is cancelled at the server.
struct Outstanding<'a> {
    shared: &'a Arc<Shared>,
    id: i64,
    cancel: bool,
    live: bool,
}

impl Outstanding<'_> {
    fn end(&mut self, reason: &str) {
        if !std::mem::replace(&mut self.live, false) {
            return;
        }
        let waiter = self.shared.lock().pending.remove(&self.id);
        // Answered meanwhile: nothing to cancel.
        let Some(w) = waiter else { return };
        if let Some(t) = w.task {
            t.abort();
        }
        if self.cancel {
            self.shared.send(
                jsonrpc::notification(
                    "notifications/cancelled",
                    json!({ "requestId": self.id, "reason": reason }),
                ),
                None,
            );
        }
    }
}

impl Drop for Outstanding<'_> {
    fn drop(&mut self) {
        self.end("the caller stopped waiting");
    }
}

async fn initialize(shared: &Arc<Shared>) -> Result<ServerInfo, Error> {
    let params = json!({
        "protocolVersion": crate::LATEST_PROTOCOL_VERSION,
        "capabilities": {},
        "clientInfo": shared.opts.client_info,
    });
    let v = request(shared, "initialize", params, shared.opts.request_timeout).await?;
    let r: InitializeResult = serde_json::from_value(v)
        .map_err(|e| Error::Protocol(format!("an unreadable initialize answer: {e}")))?;
    if !crate::supports(&r.protocol_version) {
        return Err(Error::UnsupportedVersion {
            offered: crate::LATEST_PROTOCOL_VERSION.into(),
            answered: r.protocol_version,
        });
    }
    let initialized = jsonrpc::notification("notifications/initialized", Value::Null);
    if let Some(http) = &shared.http {
        http.set_version(Some(r.protocol_version.clone()));
        // Awaited, so it lands before any request that follows.
        post(shared, &initialized, None).await?;
    } else {
        shared.send(initialized, None);
    }
    Ok(ServerInfo {
        protocol_version: r.protocol_version,
        server: r.server_info,
        capabilities: r.capabilities,
        instructions: r.instructions,
    })
}

// ---- stdio and pipes ----

/// A stdio server's process: its group is signalled, and its exit and
/// stderr name a crash.
struct Proc {
    pid: u32,
    exited: Arc<AtomicBool>,
    status: watch::Receiver<Option<String>>,
    tail: Arc<Mutex<Tail>>,
    /// Its stderr has been read to its end.
    drained: watch::Receiver<bool>,
}

impl Proc {
    fn signal(&self, sig: i32) {
        if self.exited.load(Ordering::SeqCst) {
            return;
        }
        let Ok(pid) = i32::try_from(self.pid) else {
            return;
        };
        // SAFETY: kill(2) only signals; it reads and writes no memory of
        // ours. A negative pid names the process group `stdio_command`
        // made; a child started without one gets the signal itself.
        unsafe {
            if libc::kill(-pid, sig) != 0 {
                libc::kill(pid, sig);
            }
        }
    }
}

/// The last few KiB of a server's stderr.
#[derive(Default)]
struct Tail {
    bytes: VecDeque<u8>,
}

impl Tail {
    const MAX: usize = 4096;

    fn push(&mut self, b: &[u8]) {
        self.bytes.extend(b);
        let excess = self.bytes.len().saturating_sub(Self::MAX);
        self.bytes.drain(..excess);
    }

    /// The last `max_chars` characters, on one line.
    fn text(&self, max_chars: usize) -> String {
        let bytes: Vec<u8> = self.bytes.iter().copied().collect();
        let s = String::from_utf8_lossy(&bytes);
        let words = s.split_whitespace().collect::<Vec<_>>().join(" ");
        let n = words.chars().count();
        let end: String = words.chars().skip(n.saturating_sub(max_chars)).collect();
        end.trim_start().to_string()
    }
}

fn start_stdio(
    s: StdioServer,
    opts: Options,
    events: mpsc::UnboundedSender<Event>,
) -> Result<Arc<Shared>, Error> {
    let StdioServer {
        mut child,
        stderr_log,
    } = s;
    let pid = child
        .id()
        .ok_or_else(|| Error::Unreachable("the server process has already exited".into()))?;
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        return Err(Error::Unreachable(
            "the server's stdin and stdout must be piped".into(),
        ));
    };
    let tail = Arc::new(Mutex::new(Tail::default()));
    let (drained_tx, drained) = watch::channel(child.stderr.is_none());
    if let Some(stderr) = child.stderr.take() {
        let tail = tail.clone();
        tokio::spawn(async move {
            drain_stderr(stderr, stderr_log, tail).await;
            let _ = drained_tx.send(true);
        });
    }
    let exited = Arc::new(AtomicBool::new(false));
    let (status_tx, status) = watch::channel(None);
    let flag = exited.clone();
    tokio::spawn(async move {
        let status = match child.wait().await {
            Ok(s) => s.to_string(),
            Err(e) => format!("an unknown status ({e})"),
        };
        flag.store(true, Ordering::SeqCst);
        let _ = status_tx.send(Some(status));
    });
    let proc = Proc {
        pid,
        exited,
        status,
        tail,
        drained,
    };
    Ok(start_pipes(
        Box::new(stdout),
        Box::new(stdin),
        Some(proc),
        opts,
        events,
    ))
}

fn start_pipes(
    reader: Box<dyn AsyncRead + Send + Unpin>,
    writer: Box<dyn AsyncWrite + Send + Unpin>,
    process: Option<Proc>,
    opts: Options,
    events: mpsc::UnboundedSender<Event>,
) -> Arc<Shared> {
    let (tx, rx) = mpsc::unbounded_channel();
    let shared = Arc::new(Shared::new(opts, events, Some(tx), None, process));
    tokio::spawn(write_lines(shared.clone(), writer, rx));
    tokio::spawn(read_lines(shared.clone(), reader));
    shared
}

async fn write_lines(
    shared: Arc<Shared>,
    mut w: Box<dyn AsyncWrite + Send + Unpin>,
    mut rx: mpsc::UnboundedReceiver<String>,
) {
    loop {
        let line = tokio::select! {
            _ = shared.stop.cancelled() => break,
            line = rx.recv() => match line {
                Some(l) => l,
                None => break,
            },
        };
        let wrote = async {
            w.write_all(line.as_bytes()).await?;
            w.write_all(b"\n").await?;
            w.flush().await
        }
        .await;
        if let Err(e) = wrote {
            let why = describe_exit(&shared, format!("writing to the server failed: {e}")).await;
            shared.close(why);
            break;
        }
    }
    // The server's stdin closes: a server ends on its own when it does.
    let _ = w.shutdown().await;
}

async fn read_lines(shared: Arc<Shared>, r: Box<dyn AsyncRead + Send + Unpin>) {
    let mut r = tokio::io::BufReader::with_capacity(64 * 1024, r);
    let max = shared.opts.max_message_bytes;
    let why = loop {
        let next = tokio::select! {
            _ = shared.stop.cancelled() => return,
            next = read_line(&mut r, max) => next,
        };
        match next {
            Ok(Line::Text(bytes)) => {
                if bytes.iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                match serde_json::from_slice::<Value>(&bytes) {
                    Ok(v) => shared.dispatch(v),
                    Err(e) => {
                        tracing::warn!(error = %e, "mcp: a line from the server that is not JSON, skipped")
                    }
                }
            }
            Ok(Line::TooLong) => {
                tracing::warn!(
                    max,
                    "mcp: a line from the server over the size limit, skipped"
                );
            }
            Ok(Line::Eof) => break "the server closed its stdout".to_string(),
            Err(e) => break format!("reading the server's stdout failed: {e}"),
        }
    };
    let why = describe_exit(&shared, why).await;
    shared.close(why);
}

/// Name a stdio server's end by its exit status and its last stderr, when
/// it has them (within half a second).
async fn describe_exit(shared: &Shared, why: String) -> String {
    let Some(p) = &shared.process else { return why };
    let deadline = tokio::time::Instant::now() + Duration::from_millis(500);
    let mut status = p.status.clone();
    let exited = match tokio::time::timeout_at(deadline, status.wait_for(Option::is_some)).await {
        Ok(Ok(s)) => (*s).clone(),
        _ => None,
    };
    let mut drained = p.drained.clone();
    let _ = tokio::time::timeout_at(deadline, drained.wait_for(|d| *d)).await;
    let mut why = match exited {
        Some(s) => format!("the server exited ({s})"),
        None => why,
    };
    let tail = p
        .tail
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .text(600);
    if !tail.is_empty() {
        why.push_str("; the end of its stderr: ");
        why.push_str(&tail);
    }
    why
}

enum Line {
    Text(Vec<u8>),
    TooLong,
    Eof,
}

/// One line, at most `max` bytes. A longer one is read to its end and
/// reported, so the next line is still found.
async fn read_line<R: AsyncBufRead + Unpin>(r: &mut R, max: usize) -> std::io::Result<Line> {
    let mut buf = Vec::new();
    let mut too_long = false;
    loop {
        let available = r.fill_buf().await?;
        if available.is_empty() {
            return Ok(if too_long {
                Line::TooLong
            } else if buf.is_empty() {
                Line::Eof
            } else {
                Line::Text(buf)
            });
        }
        match available.iter().position(|&b| b == b'\n') {
            Some(i) => {
                if !too_long {
                    buf.extend_from_slice(&available[..i]);
                }
                r.consume(i + 1);
                return Ok(if too_long || buf.len() > max {
                    Line::TooLong
                } else {
                    Line::Text(buf)
                });
            }
            None => {
                let n = available.len();
                if !too_long {
                    buf.extend_from_slice(available);
                }
                r.consume(n);
                if buf.len() > max {
                    too_long = true;
                    buf = Vec::new();
                }
            }
        }
    }
}

async fn drain_stderr(
    mut stderr: tokio::process::ChildStderr,
    log: Option<StderrLog>,
    tail: Arc<Mutex<Tail>>,
) {
    let mut file = match &log {
        // The operator's alone, as every file of Theseus's state is.
        Some(l) => tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(&l.path)
            .await
            .ok(),
        None => None,
    };
    let mut room = match (&log, &file) {
        (Some(l), Some(f)) => {
            let used = f.metadata().await.map(|m| m.len()).unwrap_or(0);
            l.cap_bytes.saturating_sub(used)
        }
        _ => 0,
    };
    let mut buf = vec![0u8; 8192];
    loop {
        let n = match stderr.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        tail.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(&buf[..n]);
        if let Some(f) = file.as_mut() {
            let k = usize::try_from(room).unwrap_or(usize::MAX).min(n);
            if k > 0 {
                // Flushed each time: tokio's file finishes a write later
                // otherwise, and a crash's last words must be on disk.
                if f.write_all(&buf[..k]).await.is_err() || f.flush().await.is_err() {
                    file = None;
                }
                room -= k as u64;
            }
        }
    }
}

// ---- streamable HTTP ----

struct Http {
    client: reqwest::Client,
    url: reqwest::Url,
    bearer: Option<String>,
    server_stream: bool,
    session: Mutex<Option<String>>,
    /// The server gave a session once, so a request without one has lost it.
    had_session: AtomicBool,
    version: Mutex<Option<String>>,
    /// Held while a new `initialize` runs, so only one does.
    reinit: tokio::sync::Mutex<()>,
    stream_task: Mutex<Option<AbortHandle>>,
}

impl Http {
    fn new(t: HttpTarget) -> Result<Self, Error> {
        let url = reqwest::Url::parse(&t.url)
            .map_err(|e| Error::Unreachable(format!("not a URL ({e})")))?;
        let mut builder = reqwest::Client::builder().connect_timeout(t.connect_timeout);
        let loopback = url.host_str().is_some_and(|h| {
            let h = h.trim_start_matches('[').trim_end_matches(']');
            h.eq_ignore_ascii_case("localhost")
                || h.parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });
        if loopback {
            // A proxy from the environment never sees a loopback server.
            builder = builder.no_proxy();
        }
        let client = builder
            .build()
            .map_err(|e| Error::Unreachable(format!("the HTTP client: {}", chain(&e))))?;
        Ok(Self {
            client,
            url,
            bearer: t.bearer,
            server_stream: t.server_stream,
            session: Mutex::new(None),
            had_session: AtomicBool::new(false),
            version: Mutex::new(None),
            reinit: tokio::sync::Mutex::new(()),
            stream_task: Mutex::new(None),
        })
    }

    fn session(&self) -> Option<String> {
        self.session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn set_session(&self, s: Option<String>) {
        if s.is_some() {
            self.had_session.store(true, Ordering::SeqCst);
        }
        *self.session.lock().unwrap_or_else(PoisonError::into_inner) = s;
    }

    fn version(&self) -> Option<String> {
        self.version
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn set_version(&self, v: Option<String>) {
        *self.version.lock().unwrap_or_else(PoisonError::into_inner) = v;
    }

    /// A request with the session's headers, and the key.
    fn build(&self, method: reqwest::Method, initialize: bool) -> reqwest::RequestBuilder {
        let mut req = self.client.request(method, self.url.clone());
        if !initialize {
            if let Some(s) = self.session() {
                req = req.header("mcp-session-id", s);
            }
            if let Some(v) = self.version() {
                req = req.header("mcp-protocol-version", v);
            }
        }
        if let Some(b) = &self.bearer {
            req = req.bearer_auth(b);
        }
        req
    }
}

/// One message over HTTP, with the session's recovery: a request that
/// finds its session gone makes a new `initialize`, once, and goes again.
async fn exchange(shared: Arc<Shared>, msg: Value, carries: Option<i64>) {
    let initialize = msg.get("method").and_then(Value::as_str) == Some("initialize");
    let mut again = true;
    loop {
        match post(&shared, &msg, carries).await {
            Ok(()) => return,
            Err(Error::SessionExpired(old)) if !initialize && again => {
                again = false;
                if let Err(e) = reinitialize(&shared, &old).await {
                    if let Some(id) = carries {
                        shared.fail(id, e);
                    }
                    return;
                }
            }
            Err(e) => {
                match carries {
                    Some(id) => shared.fail(id, e),
                    None => {
                        tracing::warn!(error = %e, "mcp: a message to the server was not delivered")
                    }
                }
                return;
            }
        }
    }
}

/// A new `initialize`, after the server forgot session `expired`; nothing,
/// if another request already made one.
async fn reinitialize(shared: &Arc<Shared>, expired: &str) -> Result<(), Error> {
    let http = shared.http.as_ref().expect("an HTTP connection");
    let _only = http.reinit.lock().await;
    if http.session().is_some_and(|now| now != expired) {
        return Ok(());
    }
    http.set_session(None);
    http.set_version(None);
    let info = initialize(shared).await?;
    *shared
        .server
        .write()
        .unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(info));
    shared.event(Event::Reinitialized);
    if http.server_stream {
        start_server_stream(shared);
    }
    Ok(())
}

/// POST one message. Its answer, and whatever comes with it, is
/// dispatched; `carries`' waiter fails if none comes.
async fn post(shared: &Arc<Shared>, msg: &Value, carries: Option<i64>) -> Result<(), Error> {
    let http = shared.http.as_ref().expect("an HTTP connection");
    let initialize = msg.get("method").and_then(Value::as_str) == Some("initialize");
    let session = http.session();
    if !initialize && session.is_none() && http.had_session.load(Ordering::SeqCst) {
        // A new `initialize` is running, or one was interrupted.
        return Err(Error::SessionExpired(String::new()));
    }
    let resp = http
        .build(reqwest::Method::POST, initialize)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json, text/event-stream")
        .body(msg.to_string())
        .send()
        .await
        .map_err(send_error)?;
    let status = resp.status();
    if status == reqwest::StatusCode::NOT_FOUND && !initialize {
        if let Some(s) = session {
            return Err(Error::SessionExpired(s));
        }
    }
    if initialize && status.is_success() {
        let given = resp
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
            .map(String::from);
        http.set_session(given);
    }
    if status == reqwest::StatusCode::ACCEPTED {
        return match carries {
            Some(_) => Err(Error::Protocol(
                "the server accepted a request (202) without answering it".into(),
            )),
            None => Ok(()),
        };
    }
    if !status.is_success() {
        let body = read_body(resp, 2000).await.unwrap_or_default();
        return Err(Error::Http {
            status: status.as_u16(),
            body: jsonrpc::clip(String::from_utf8_lossy(&body).trim(), 500),
        });
    }
    let kind = content_type(&resp);
    if kind.starts_with("text/event-stream") {
        let mut parser = sse::Parser::new(shared.opts.max_message_bytes);
        let ended = read_events(shared, resp, &mut parser, carries).await;
        return match carries {
            Some(id) if shared.is_pending(id) => resume(shared, id, parser, ended).await,
            _ => Ok(()),
        };
    }
    let body = read_body(resp, shared.opts.max_message_bytes).await?;
    if body.iter().all(u8::is_ascii_whitespace) {
        return match carries {
            Some(_) => Err(Error::Protocol("an empty answer to a request".into())),
            None => Ok(()),
        };
    }
    if !kind.starts_with("application/json") && carries.is_some() {
        return Err(Error::Protocol(format!(
            "an answer of type {:?}",
            jsonrpc::clip(&kind, 60)
        )));
    }
    let v: Value = serde_json::from_slice(&body)
        .map_err(|e| Error::Protocol(format!("an answer that is not JSON: {e}")))?;
    shared.dispatch(v);
    match carries {
        Some(id) if shared.is_pending(id) => Err(Error::Protocol(
            "the server's answer was not to this request".into(),
        )),
        _ => Ok(()),
    }
}

/// The server closed a request's stream before its answer. Ask for what
/// followed the last event id on a GET (resumable streams), up to three
/// times without progress; without an event id, the call's outcome is
/// unknown.
async fn resume(
    shared: &Arc<Shared>,
    id: i64,
    mut parser: sse::Parser,
    mut ended: Result<(), String>,
) -> Result<(), Error> {
    let mut fruitless = 0;
    loop {
        let Some(last) = parser.last_event_id().map(String::from) else {
            let why = ended.err().map(|e| format!(" ({e})")).unwrap_or_default();
            return Err(Error::Closed(format!(
                "the server's stream ended before the answer{why}"
            )));
        };
        if fruitless >= 3 {
            return Err(Error::Closed(
                "the server's stream ended before the answer, and resuming it gave nothing".into(),
            ));
        }
        let wait = parser
            .retry()
            .unwrap_or(Duration::from_millis(500))
            .min(Duration::from_secs(10));
        tokio::select! {
            _ = shared.stop.cancelled() => return Ok(()),
            _ = tokio::time::sleep(wait) => {}
        }
        if !shared.is_pending(id) {
            return Ok(());
        }
        match get(shared, Some(&last)).await {
            Ok(r)
                if r.status().is_success() && content_type(&r).starts_with("text/event-stream") =>
            {
                parser.restart();
                ended = read_events(shared, r, &mut parser, Some(id)).await;
                if !shared.is_pending(id) {
                    return Ok(());
                }
                if parser.last_event_id() == Some(last.as_str()) {
                    fruitless += 1;
                } else {
                    fruitless = 0;
                }
            }
            Ok(r) => {
                return Err(Error::Closed(format!(
                    "the server's stream ended before the answer, and resuming it got HTTP {}",
                    r.status().as_u16()
                )))
            }
            Err(e) => {
                fruitless += 1;
                ended = Err(e.to_string());
            }
        }
    }
}

/// A GET on the endpoint: the server's own stream, or a resumed one.
async fn get(shared: &Shared, last_event_id: Option<&str>) -> Result<reqwest::Response, Error> {
    let http = shared.http.as_ref().expect("an HTTP connection");
    let mut req = http
        .build(reqwest::Method::GET, false)
        .header(ACCEPT, "text/event-stream");
    if let Some(id) = last_event_id {
        req = req.header("last-event-id", id);
    }
    req.send().await.map_err(send_error)
}

/// Read a stream's events into `dispatch`, until it ends, fails, the
/// connection stops, or `carries` is answered.
async fn read_events(
    shared: &Arc<Shared>,
    resp: reqwest::Response,
    parser: &mut sse::Parser,
    carries: Option<i64>,
) -> Result<(), String> {
    let mut body = resp.bytes_stream();
    loop {
        let chunk = tokio::select! {
            _ = shared.stop.cancelled() => return Err("the connection was closed".into()),
            chunk = body.next() => chunk,
        };
        let bytes = match chunk {
            None => return Ok(()),
            Some(Err(e)) => return Err(chain(&e.without_url())),
            Some(Ok(b)) => b,
        };
        for ev in parser.feed(&bytes).map_err(|e| e.to_string())? {
            // A priming event (an id, and no data) carries no message.
            if ev.event != "message" || ev.data.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<Value>(&ev.data) {
                Ok(v) => shared.dispatch(v),
                Err(e) => {
                    tracing::warn!(error = %e, "mcp: an event from the server that is not JSON, skipped")
                }
            }
        }
        if let Some(id) = carries {
            if !shared.is_pending(id) {
                return Ok(());
            }
        }
    }
}

fn start_server_stream(shared: &Arc<Shared>) {
    let Some(http) = &shared.http else { return };
    let task = tokio::spawn(server_stream(shared.clone()));
    let old = http
        .stream_task
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .replace(task.abort_handle());
    if let Some(old) = old {
        old.abort();
    }
}

/// The server's own stream: what it sends unasked. Reopened when it ends,
/// from its last event id, after the server's retry time or a backoff. A
/// server that offers none (405), a session that is gone (404, until the
/// next `initialize` opens another), or a stream held elsewhere (409)
/// ends it.
async fn server_stream(shared: Arc<Shared>) {
    let mut parser = sse::Parser::new(shared.opts.max_message_bytes);
    let mut failures = 0u32;
    loop {
        let last = parser.last_event_id().map(String::from);
        let resp = tokio::select! {
            _ = shared.stop.cancelled() => return,
            resp = get(&shared, last.as_deref()) => resp,
        };
        let wait = match resp {
            Ok(r) if matches!(r.status().as_u16(), 404 | 405 | 409) => return,
            Ok(r)
                if r.status().is_success() && content_type(&r).starts_with("text/event-stream") =>
            {
                failures = 0;
                parser.restart();
                let _ = read_events(&shared, r, &mut parser, None).await;
                parser.retry().unwrap_or(Duration::from_secs(1))
            }
            _ => {
                failures += 1;
                Duration::from_secs(1 << failures.min(5))
            }
        };
        tokio::select! {
            _ = shared.stop.cancelled() => return,
            _ = tokio::time::sleep(wait.min(Duration::from_secs(30))) => {}
        }
    }
}

async fn delete_session(shared: &Shared) {
    let Some(http) = &shared.http else { return };
    if http.session().is_none() {
        return;
    }
    let req = http.build(reqwest::Method::DELETE, false).send();
    let _ = tokio::time::timeout(Duration::from_secs(5), req).await;
}

fn content_type(r: &reqwest::Response) -> String {
    r.headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase()
}

async fn read_body(resp: reqwest::Response, max: usize) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    let mut body = resp.bytes_stream();
    while let Some(chunk) = body.next().await {
        let chunk = chunk.map_err(|e| Error::Closed(chain(&e.without_url())))?;
        if out.len() + chunk.len() > max {
            return Err(Error::Protocol(format!(
                "an answer larger than {max} bytes"
            )));
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

/// A failed send: nothing went out if it never connected.
fn send_error(e: reqwest::Error) -> Error {
    let connect = e.is_connect() || e.is_builder();
    let why = chain(&e.without_url());
    if connect {
        Error::Unreachable(why)
    } else {
        Error::Closed(why)
    }
}

/// An error and its causes, on one line.
fn chain(e: &dyn std::error::Error) -> String {
    let mut s = e.to_string();
    let mut next = e.source();
    while let Some(cause) = next {
        let c = cause.to_string();
        if !s.contains(&c) {
            s.push_str(": ");
            s.push_str(&c);
        }
        next = cause.source();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lines_are_bounded_and_resync() {
        let input: &[u8] = b"{\"a\":1}\r\n0123456789abcdef0123\n{\"b\":2}\ntail";
        let mut r = tokio::io::BufReader::with_capacity(4, input);
        let mut got = Vec::new();
        loop {
            match read_line(&mut r, 12).await.unwrap() {
                Line::Text(t) => got.push(String::from_utf8(t).unwrap()),
                Line::TooLong => got.push("<too long>".into()),
                Line::Eof => break,
            }
        }
        assert_eq!(got, ["{\"a\":1}\r", "<too long>", "{\"b\":2}", "tail"]);
    }

    #[test]
    fn a_tail_keeps_the_end_on_one_line() {
        let mut t = Tail::default();
        t.push(b"first line\n");
        t.push(&vec![b'x'; 5000]);
        t.push(b"\nError: boom\n  at main\n");
        assert_eq!(t.bytes.len(), Tail::MAX);
        assert_eq!(t.text(20), "Error: boom at main");
        assert_eq!(t.text(23), "xxx Error: boom at main");
    }

    #[test]
    fn events_from_notifications() {
        assert_eq!(
            Event::from_notification("notifications/tools/list_changed".into(), Value::Null),
            Event::ToolListChanged
        );
        assert_eq!(
            Event::from_notification(
                "notifications/progress".into(),
                json!({"progressToken": "t1", "progress": 2, "total": 4, "message": "half"})
            ),
            Event::Progress {
                token: json!("t1"),
                progress: 2.0,
                total: Some(4.0),
                message: Some("half".into())
            }
        );
        assert!(matches!(
            Event::from_notification("notifications/message".into(), json!({"level": "warning", "data": "x"})),
            Event::Log { level, .. } if level == "warning"
        ));
        assert!(matches!(
            Event::from_notification(
                "notifications/resources/updated".into(),
                json!({"uri": "a"})
            ),
            Event::Notification { .. }
        ));
    }

    #[test]
    fn a_target_never_shows_its_key() {
        let mut t = HttpTarget::new("http://127.0.0.1:1/mcp");
        t.bearer = Some("sekrit-key".into());
        let shown = format!("{t:?}");
        assert!(!shown.contains("sekrit") && shown.contains("(set)"));
    }
}
