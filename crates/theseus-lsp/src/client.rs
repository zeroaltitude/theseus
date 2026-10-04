//! The LSP client: one connection to one language server.
//!
//! **Who starts the server.** The caller does, and hands the client the
//! server's stdin and stdout, and a way to kill its process group
//! ([`Server`]). In the daemon that is L2's board, through
//! `theseus_kernel::children::spawn`, in a process group of its own, with the
//! job environment; the client never spawns on its own. Tests and the probe
//! use [`crate::spawn`].
//!
//! **The connection.** Two tasks: a writer that frames each message onto the
//! server's stdin in the order it was sent, and a reader that routes what
//! comes back. An answer goes to the request waiting for its id. A
//! notification updates the client's state (diagnostics, progress, the
//! server's status) and goes out as an [`Event`]. A request from the server
//! is answered at once ([`Shared::answer`]): `workspace/configuration` from
//! the settings the caller gave, `client/registerCapability` (a registration
//! of pull diagnostics turns the pull path on), `window/workDoneProgress/create`,
//! `workspace/workspaceFolders`, the refreshes, and `workspace/applyEdit`,
//! which is refused: this client never applies an edit. Anything else is
//! "method not found".
//!
//! **Requests.** Each waits for its answer up to [`Options::request_timeout`]
//! (or its own). One that times out, or whose future is dropped, is cancelled
//! at the server with `$/cancelRequest`, and a late answer is dropped.
//! `initialize` and `shutdown` are never cancelled. A request whose
//! connection ends gets [`Error::Closed`]. Before a request about a document,
//! every open document whose file changed on disk is sent again
//! ([`Client::sync_disk`], in `docs.rs`).
//!
//! **The stop** ([`Client::stop`]): `shutdown`, then `exit`, then, if the
//! server's stdout has not closed within [`Options::exit_grace`] (1 s), the
//! kill. TypeScript 7's server answers `shutdown` and ignores `exit`. The
//! last clone dropped without a stop kills the server at once.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tokio::io::{AsyncRead, AsyncWrite, BufReader};
use tokio::sync::{mpsc, oneshot, watch};

use crate::docs::Doc;
use crate::framing;
use crate::jsonrpc::{self, code, Incoming, RpcError};
use crate::types::{InitializeResult, ServerInfo};
use crate::uri;

/// Kills the server's process group. Called at most a few times; a kill of
/// a group that is gone must do nothing.
pub type Kill = Box<dyn Fn() + Send + Sync>;

/// A started server, as the caller hands it over.
pub struct Server {
    /// The server's stdout.
    pub reader: Box<dyn AsyncRead + Send + Unpin>,
    /// The server's stdin.
    pub writer: Box<dyn AsyncWrite + Send + Unpin>,
    /// Kills its process group; `None` for a server in this process.
    pub kill: Option<Kill>,
    /// Its process id, for the record and for `initialize`'s log.
    pub pid: Option<u32>,
}

impl Server {
    /// A server over any pipes, with no process: a fake in this process.
    pub fn pipes(
        reader: impl AsyncRead + Send + Unpin + 'static,
        writer: impl AsyncWrite + Send + Unpin + 'static,
    ) -> Self {
        Self {
            reader: Box::new(reader),
            writer: Box::new(writer),
            kill: None,
            pid: None,
        }
    }
}

/// How a client behaves.
#[derive(Debug, Clone)]
pub struct Options {
    /// The workspace's root: `rootUri` and the one workspace folder.
    pub root: PathBuf,
    /// Who the client says it is in `initialize`.
    pub client_name: String,
    /// `initializationOptions`, as the server wants them (typescript-language-server's
    /// `tsserver.path`, for one).
    pub initialization_options: Value,
    /// What `workspace/configuration` answers from: a section `a.b` is
    /// `settings["a"]["b"]`, and no section is the whole value.
    pub settings: Value,
    /// The server reports readiness with rust-analyzer's
    /// `experimental/serverStatus`, so "ready" waits for `quiescent`.
    pub expects_server_status: bool,
    /// The longest wait for `initialize`'s answer.
    pub initialize_timeout: Duration,
    /// The longest wait for any other request that names none.
    pub request_timeout: Duration,
    /// The longest wait for `shutdown`'s answer.
    pub shutdown_timeout: Duration,
    /// After `exit`, how long the server has to close its stdout before the
    /// kill.
    pub exit_grace: Duration,
    /// A message larger than this ends the connection.
    pub max_message_bytes: usize,
}

impl Options {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            client_name: "theseus".into(),
            initialization_options: Value::Null,
            settings: Value::Null,
            expects_server_status: false,
            initialize_timeout: Duration::from_secs(30),
            request_timeout: Duration::from_secs(30),
            shutdown_timeout: Duration::from_secs(2),
            exit_grace: Duration::from_secs(1),
            max_message_bytes: 64 << 20,
        }
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    /// No answer in time. The request was cancelled at the server.
    #[error("{method} got no answer within {after:?}, and was cancelled")]
    Timeout { method: String, after: Duration },
    /// The connection ended: the server exited or crashed, or the client
    /// stopped it.
    #[error("the connection to the language server closed: {0}")]
    Closed(String),
    #[error("the language server answered error {code}: {message}")]
    Rpc {
        code: i64,
        message: String,
        data: Value,
    },
    #[error("the language server broke the protocol: {0}")]
    Protocol(String),
    /// The server did not declare what the request needs.
    #[error("the language server does not offer {0}")]
    NotOffered(String),
    #[error("{path}: {message}")]
    File { path: PathBuf, message: String },
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

/// What the server sent unasked, and what became of the connection, in
/// order. The receiver may be dropped: nothing then queues.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// `textDocument/publishDiagnostics`.
    Diagnostics {
        uri: String,
        version: Option<i32>,
        count: usize,
    },
    /// `$/progress` of a work-done token: `begin`, `report`, or `end`.
    Progress {
        token: String,
        kind: String,
        title: Option<String>,
        message: Option<String>,
        percentage: Option<u32>,
    },
    /// rust-analyzer's `experimental/serverStatus`.
    ServerStatus(ServerStatus),
    /// `window/logMessage` (`level` 1 error … 4 log, 5 debug).
    Log {
        level: u8,
        message: String,
    },
    /// `window/showMessage`, and `window/showMessageRequest` (answered
    /// with no choice).
    Message {
        level: u8,
        message: String,
    },
    Notification {
        method: String,
        params: Value,
    },
    /// The connection ended; every waiting request failed with this reason.
    Closed {
        reason: String,
    },
}

pub type Events = mpsc::UnboundedReceiver<Event>;

/// rust-analyzer's status: `health` is `ok`, `warning`, or `error`, and
/// `quiescent` says it has loaded the workspace and finished its work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerStatus {
    pub health: String,
    pub quiescent: bool,
    pub message: Option<String>,
}

/// Whether the server is ready: no work-done progress running and, for a
/// server that reports a status, a quiescent one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Readiness {
    /// Progress begun and not ended: token, then its title.
    pub running: BTreeMap<String, String>,
    pub status: Option<ServerStatus>,
    /// Whether any progress was ever begun.
    pub saw_progress: bool,
}

impl Readiness {
    pub fn is_ready(&self, expects_status: bool) -> bool {
        self.running.is_empty()
            && match &self.status {
                Some(s) => s.quiescent,
                None => !expects_status,
            }
    }
}

/// What this client reads from the server's capabilities, and the
/// registrations it made since.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Capabilities {
    /// As the server sent them.
    pub raw: Value,
    /// `textDocument/diagnostic`: declared, or registered later.
    pub pull_diagnostics: bool,
    /// The pull's `identifier`, when the server named one.
    pub diagnostic_identifier: Option<String>,
    /// `didSave` should carry the text.
    pub save_include_text: bool,
}

impl Capabilities {
    fn read(raw: Value) -> Self {
        let pull = raw
            .get("diagnosticProvider")
            .filter(|v| !v.is_null() && **v != json!(false));
        let save = raw.pointer("/textDocumentSync/save");
        Self {
            pull_diagnostics: pull.is_some(),
            diagnostic_identifier: pull
                .and_then(|p| p.get("identifier"))
                .and_then(Value::as_str)
                .map(String::from),
            save_include_text: save
                .and_then(|s| s.get("includeText"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
            raw,
        }
    }

    /// Whether `provider` (`definitionProvider`, …) is declared and not
    /// `false`.
    pub fn offers(&self, provider: &str) -> bool {
        self.raw
            .get(provider)
            .is_some_and(|v| !v.is_null() && *v != json!(false))
    }
}

/// How a stop went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stopped {
    /// `shutdown` was answered (an error answer counts: the server heard).
    pub shutdown_answered: bool,
    /// The server closed its stdout after `exit`, within the grace.
    pub exited: bool,
    /// The server was killed: it ignored `exit`, or never answered.
    pub killed: bool,
    pub took: Duration,
}

/// A connection to one server. Clones share it; the last one dropped kills
/// the server if it was not stopped.
#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

struct Inner {
    shared: Arc<Shared>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        let s = &self.shared;
        if s.lock().closed.is_none() {
            s.send(jsonrpc::notification("exit", Value::Null));
            s.close("the client was dropped".into());
        }
        if !s.lock().eof {
            s.kill();
        }
    }
}

/// What every task of one connection shares.
pub(crate) struct Shared {
    pub(crate) opts: Options,
    state: Mutex<State>,
    /// Bumped on every change a waiter may be waiting for.
    changed: watch::Sender<u64>,
    events: mpsc::UnboundedSender<Event>,
    out: mpsc::UnboundedSender<Value>,
    next_id: AtomicI64,
    kill: Option<Kill>,
    pid: Option<u32>,
    /// Serializes document syncs, so each document's versions go out in
    /// order and a file read twice is not sent twice.
    pub(crate) sync: tokio::sync::Mutex<()>,
}

#[derive(Default)]
pub(crate) struct State {
    pending: HashMap<i64, oneshot::Sender<Result<Value, Error>>>,
    pub(crate) closed: Option<String>,
    /// The server closed its stdout.
    eof: bool,
    pub(crate) server: Option<ServerInfo>,
    pub(crate) caps: Capabilities,
    pub(crate) readiness: Readiness,
    /// Open documents, by normalized URI.
    pub(crate) docs: HashMap<String, Doc>,
    /// Pushed diagnostics, by normalized URI.
    pub(crate) pushed: HashMap<String, crate::diagnostics::Pushed>,
    /// Pulled diagnostics, by normalized URI.
    pub(crate) pulled: HashMap<String, crate::diagnostics::Pulled>,
    /// Messages sent so far: a pushed list with no version is fresh for a
    /// change when it arrived after it.
    pub(crate) sent: u64,
    /// Registrations by id: method, then options.
    registrations: HashMap<String, (String, Value)>,
    /// `$/cancelRequest`s sent.
    pub(crate) cancels: u64,
}

impl Shared {
    pub(crate) fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn bump(&self) {
        self.changed.send_modify(|n| *n += 1);
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<u64> {
        self.changed.subscribe()
    }

    pub(crate) fn event(&self, e: Event) {
        let _ = self.events.send(e);
    }

    /// Queue one message for the writer. Returns the count of messages sent
    /// with it.
    pub(crate) fn send(&self, msg: Value) -> u64 {
        let mut st = self.lock();
        st.sent += 1;
        let _ = self.out.send(msg);
        st.sent
    }

    fn kill(&self) {
        if let Some(k) = &self.kill {
            k();
        }
    }

    /// The connection ended: every waiting request fails with `reason`,
    /// once.
    fn close(&self, reason: String) {
        let pending = {
            let mut st = self.lock();
            if st.closed.is_some() {
                return;
            }
            st.closed = Some(reason.clone());
            std::mem::take(&mut st.pending)
        };
        for (_, tx) in pending {
            let _ = tx.send(Err(Error::Closed(reason.clone())));
        }
        self.event(Event::Closed { reason });
        self.bump();
    }

    fn dispatch(self: &Arc<Self>, v: Value) {
        match jsonrpc::classify(v) {
            Ok(Incoming::Response { id, outcome }) => {
                let waiter = jsonrpc::id_number(&id).and_then(|n| self.lock().pending.remove(&n));
                match waiter {
                    Some(tx) => {
                        let _ = tx.send(outcome.map_err(Error::from));
                    }
                    None => tracing::debug!(%id, "lsp: an answer to no waiting request, dropped"),
                }
            }
            Ok(Incoming::Notification { method, params }) => self.notified(method, params),
            Ok(Incoming::Request { id, method, params }) => {
                let answer = match self.answer(&method, &params) {
                    Ok(result) => jsonrpc::response(id, result),
                    Err((code, message)) => jsonrpc::error_response(id, code, &message),
                };
                self.send(answer);
            }
            Err(e) => {
                tracing::warn!(error = %e, "lsp: an unreadable message from the server, skipped")
            }
        }
    }

    /// The answer to one of the server's requests.
    fn answer(&self, method: &str, params: &Value) -> Result<Value, (i64, String)> {
        match method {
            "workspace/configuration" => Ok(self.configuration(params)),
            "client/registerCapability" => {
                self.register(params);
                Ok(Value::Null)
            }
            "client/unregisterCapability" => {
                self.unregister(params);
                Ok(Value::Null)
            }
            "window/workDoneProgress/create" => Ok(Value::Null),
            "workspace/workspaceFolders" => Ok(json!([self.folder()])),
            "workspace/diagnostic/refresh" => {
                // Every pulled list is stale: the next wait pulls again.
                self.lock().pulled.clear();
                self.bump();
                Ok(Value::Null)
            }
            "workspace/semanticTokens/refresh"
            | "workspace/inlayHint/refresh"
            | "workspace/inlineValue/refresh"
            | "workspace/codeLens/refresh"
            | "workspace/foldingRange/refresh" => Ok(Value::Null),
            "workspace/applyEdit" => Ok(json!({
                "applied": false,
                "failureReason": "this client never applies an edit itself"
            })),
            "window/showMessageRequest" => {
                self.event(Event::Message {
                    level: level(params),
                    message: text(params, "message"),
                });
                Ok(Value::Null)
            }
            "window/showDocument" => Ok(json!({ "success": false })),
            _ => Err((
                code::METHOD_NOT_FOUND,
                format!("this client offers no {}", jsonrpc::clip(method, 80)),
            )),
        }
    }

    fn configuration(&self, params: &Value) -> Value {
        let items = params.get("items").and_then(Value::as_array);
        let answers = items
            .into_iter()
            .flatten()
            .map(|item| match item.get("section").and_then(Value::as_str) {
                None | Some("") => self.opts.settings.clone(),
                Some(section) => section
                    .split('.')
                    .try_fold(&self.opts.settings, |v, k| v.get(k))
                    .cloned()
                    .unwrap_or(Value::Null),
            })
            .collect();
        Value::Array(answers)
    }

    fn register(&self, params: &Value) {
        let mut st = self.lock();
        for r in params
            .get("registrations")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let id = text(r, "id");
            let method = text(r, "method");
            let options = r.get("registerOptions").cloned().unwrap_or(Value::Null);
            if method == "textDocument/diagnostic" {
                st.caps.pull_diagnostics = true;
                if let Some(i) = options.get("identifier").and_then(Value::as_str) {
                    st.caps.diagnostic_identifier = Some(i.to_string());
                }
            }
            st.registrations.insert(id, (method, options));
        }
        drop(st);
        self.bump();
    }

    fn unregister(&self, params: &Value) {
        let mut st = self.lock();
        // The spec's misspelling, `unregisterations`, is the field's name.
        let list = params
            .get("unregisterations")
            .or_else(|| params.get("unregistrations"))
            .and_then(Value::as_array);
        for u in list.into_iter().flatten() {
            st.registrations.remove(&text(u, "id"));
        }
        let declared = st.caps.raw.get("diagnosticProvider").is_some();
        st.caps.pull_diagnostics = declared
            || st
                .registrations
                .values()
                .any(|(m, _)| m == "textDocument/diagnostic");
    }

    pub(crate) fn folder(&self) -> Value {
        let name = self
            .opts
            .root
            .file_name()
            .map_or_else(|| "root".into(), |n| n.to_string_lossy().into_owned());
        json!({ "uri": uri::from_path(&self.opts.root), "name": name })
    }

    fn notified(&self, method: String, params: Value) {
        match method.as_str() {
            "textDocument/publishDiagnostics" => self.published(params),
            "$/progress" => self.progress(&params),
            "experimental/serverStatus" => {
                let status = ServerStatus {
                    health: params
                        .get("health")
                        .and_then(Value::as_str)
                        .unwrap_or("ok")
                        .to_string(),
                    quiescent: params
                        .get("quiescent")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    message: params
                        .get("message")
                        .and_then(Value::as_str)
                        .map(String::from),
                };
                self.lock().readiness.status = Some(status.clone());
                self.event(Event::ServerStatus(status));
                self.bump();
            }
            "window/logMessage" => self.event(Event::Log {
                level: level(&params),
                message: text(&params, "message"),
            }),
            "window/showMessage" => self.event(Event::Message {
                level: level(&params),
                message: text(&params, "message"),
            }),
            _ => self.event(Event::Notification { method, params }),
        }
    }

    fn progress(&self, params: &Value) {
        let token = match params.get("token") {
            Some(Value::String(s)) => s.clone(),
            Some(v) => v.to_string(),
            None => return,
        };
        let value = params.get("value").cloned().unwrap_or(Value::Null);
        let kind = text(&value, "kind");
        let title = value.get("title").and_then(Value::as_str).map(String::from);
        {
            let mut st = self.lock();
            match kind.as_str() {
                "begin" => {
                    st.readiness.saw_progress = true;
                    st.readiness
                        .running
                        .insert(token.clone(), title.clone().unwrap_or_default());
                }
                "end" => {
                    st.readiness.running.remove(&token);
                }
                _ => {}
            }
        }
        self.event(Event::Progress {
            token,
            kind,
            title,
            message: value
                .get("message")
                .and_then(Value::as_str)
                .map(String::from),
            percentage: value
                .get("percentage")
                .and_then(Value::as_u64)
                .and_then(|p| u32::try_from(p).ok()),
        });
        self.bump();
    }
}

fn text(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn level(v: &Value) -> u8 {
    v.get("type")
        .and_then(Value::as_u64)
        .and_then(|t| u8::try_from(t).ok())
        .unwrap_or(4)
}

impl Client {
    /// Start a connection on a started server, and run the handshake:
    /// `initialize`, then `initialized`.
    pub async fn start(server: Server, opts: Options) -> Result<(Client, Events), Error> {
        let (events_tx, events) = mpsc::unbounded_channel();
        let (out_tx, out_rx) = mpsc::unbounded_channel();
        let shared = Arc::new(Shared {
            opts,
            state: Mutex::new(State::default()),
            changed: watch::channel(0).0,
            events: events_tx,
            out: out_tx,
            next_id: AtomicI64::new(1),
            kill: server.kill,
            pid: server.pid,
            sync: tokio::sync::Mutex::new(()),
        });
        tokio::spawn(write_loop(shared.clone(), server.writer, out_rx));
        tokio::spawn(read_loop(shared.clone(), server.reader));
        let client = Client {
            inner: Arc::new(Inner {
                shared: shared.clone(),
            }),
        };
        client.initialize().await?;
        Ok((client, events))
    }

    pub(crate) fn shared(&self) -> &Arc<Shared> {
        &self.inner.shared
    }

    async fn initialize(&self) -> Result<(), Error> {
        let s = self.shared();
        let params = json!({
            "processId": std::process::id(),
            "clientInfo": { "name": s.opts.client_name, "version": env!("CARGO_PKG_VERSION") },
            "rootUri": uri::from_path(&s.opts.root),
            "rootPath": s.opts.root,
            "workspaceFolders": [s.folder()],
            "capabilities": client_capabilities(),
            "initializationOptions": s.opts.initialization_options,
            "trace": "off",
        });
        let v = self
            .request_within("initialize", params, s.opts.initialize_timeout)
            .await?;
        let r: InitializeResult = serde_json::from_value(v)
            .map_err(|e| Error::Protocol(format!("an unreadable initialize answer: {e}")))?;
        {
            let mut st = s.lock();
            let regs = std::mem::take(&mut st.registrations);
            st.caps = Capabilities::read(r.capabilities);
            st.server = r.server_info;
            // A registration that came before the answer stands.
            for (m, o) in regs.values() {
                if m == "textDocument/diagnostic" {
                    st.caps.pull_diagnostics = true;
                    if let Some(i) = o.get("identifier").and_then(Value::as_str) {
                        st.caps.diagnostic_identifier = Some(i.to_string());
                    }
                }
            }
            st.registrations = regs;
        }
        s.send(jsonrpc::notification("initialized", json!({})));
        Ok(())
    }

    /// What the server said about itself.
    pub fn server_info(&self) -> Option<ServerInfo> {
        self.shared().lock().server.clone()
    }

    pub fn capabilities(&self) -> Capabilities {
        self.shared().lock().caps.clone()
    }

    pub fn readiness(&self) -> Readiness {
        self.shared().lock().readiness.clone()
    }

    /// The server's process id, as the caller gave it.
    pub fn pid(&self) -> Option<u32> {
        self.shared().pid
    }

    /// Why the connection closed, once it has.
    pub fn closed(&self) -> Option<String> {
        self.shared().lock().closed.clone()
    }

    /// Requests waiting for their answers.
    pub fn in_flight(&self) -> usize {
        self.shared().lock().pending.len()
    }

    /// `$/cancelRequest`s sent so far.
    pub fn cancels_sent(&self) -> u64 {
        self.shared().lock().cancels
    }

    /// Wait until the server is ready ([`Readiness::is_ready`]), at most
    /// `bound`. `Err` carries the readiness when the bound ran out.
    pub async fn wait_ready(&self, bound: Duration) -> Result<Readiness, Readiness> {
        let s = self.shared();
        let expects = s.opts.expects_server_status;
        let mut rx = s.subscribe();
        let deadline = tokio::time::Instant::now() + bound;
        loop {
            let r = self.readiness();
            if r.is_ready(expects) || self.closed().is_some() {
                return Ok(r);
            }
            if tokio::time::timeout_at(deadline, rx.changed())
                .await
                .is_err()
            {
                return Err(self.readiness());
            }
        }
    }

    /// Any request, its raw result, within the default timeout. Open
    /// documents changed on disk are sent first.
    pub async fn request(&self, method: &str, params: Value) -> Result<Value, Error> {
        self.sync_disk().await?;
        let t = self.shared().opts.request_timeout;
        self.request_within(method, params, t).await
    }

    /// One request within `timeout`, with no sync.
    pub async fn request_within(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, Error> {
        let s = self.shared();
        let id = s.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        {
            let mut st = s.lock();
            if let Some(why) = &st.closed {
                return Err(Error::Closed(why.clone()));
            }
            st.pending.insert(id, tx);
        }
        let mut call = Outstanding {
            shared: s,
            id,
            cancel: !matches!(method, "initialize" | "shutdown"),
            live: true,
        };
        s.send(jsonrpc::request(id, method, params));
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(answer)) => {
                call.live = false;
                answer
            }
            Ok(Err(_)) => {
                call.live = false;
                Err(Error::Closed("the request's answer was lost".into()))
            }
            Err(_) => {
                call.end();
                Err(Error::Timeout {
                    method: method.into(),
                    after: timeout,
                })
            }
        }
    }

    /// Send a notification.
    pub fn notify(&self, method: &str, params: Value) {
        self.shared().send(jsonrpc::notification(method, params));
    }

    /// New settings: `workspace/configuration` answers from them, and the
    /// server is told (`workspace/didChangeConfiguration`).
    pub fn set_settings(&self, settings: Value) {
        // The options are fixed at the start; the new settings ride along in
        // the notification, which every server that pulls reads as "ask again".
        self.notify(
            "workspace/didChangeConfiguration",
            json!({ "settings": settings }),
        );
    }

    /// Stop the server: `shutdown`, `exit`, and a kill if its stdout is still
    /// open after the grace.
    pub async fn stop(&self) -> Stopped {
        let start = Instant::now();
        let s = self.shared().clone();
        let mut rx = s.subscribe();
        let mut shutdown_answered = false;
        if s.lock().closed.is_none() {
            let r = self
                .request_within("shutdown", Value::Null, s.opts.shutdown_timeout)
                .await;
            shutdown_answered = matches!(r, Ok(_) | Err(Error::Rpc { .. }));
            s.send(jsonrpc::notification("exit", Value::Null));
        }
        let gone = |s: &Shared| {
            let st = s.lock();
            st.eof || (st.closed.is_some() && s.kill.is_none())
        };
        let deadline = tokio::time::Instant::now() + s.opts.exit_grace;
        while !gone(&s) {
            if tokio::time::timeout_at(deadline, rx.changed())
                .await
                .is_err()
            {
                break;
            }
        }
        let exited = gone(&s);
        let killed = !exited && s.kill.is_some();
        if killed {
            s.kill();
            let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
            while !gone(&s) {
                if tokio::time::timeout_at(deadline, rx.changed())
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }
        s.close("stopped by the client".into());
        Stopped {
            shutdown_answered,
            exited,
            killed,
            took: start.elapsed(),
        }
    }
}

/// A request still waiting. Dropped while it waits (the caller stopped
/// waiting), it is cancelled at the server.
struct Outstanding<'a> {
    shared: &'a Arc<Shared>,
    id: i64,
    cancel: bool,
    live: bool,
}

impl Outstanding<'_> {
    fn end(&mut self) {
        if !std::mem::replace(&mut self.live, false) {
            return;
        }
        let waiting = self.shared.lock().pending.remove(&self.id).is_some();
        if waiting && self.cancel {
            self.shared.lock().cancels += 1;
            self.shared.send(jsonrpc::notification(
                "$/cancelRequest",
                json!({ "id": self.id }),
            ));
        }
    }
}

impl Drop for Outstanding<'_> {
    fn drop(&mut self) {
        self.end();
    }
}

/// What this client can do, as `initialize` declares it.
fn client_capabilities() -> Value {
    json!({
        "general": { "positionEncodings": ["utf-16"] },
        "workspace": {
            "configuration": true,
            "workspaceFolders": true,
            "didChangeConfiguration": { "dynamicRegistration": true },
            "didChangeWatchedFiles": { "dynamicRegistration": true, "relativePatternSupport": true },
            "workspaceEdit": {
                "documentChanges": true,
                "resourceOperations": ["create", "rename", "delete"],
                "failureHandling": "abort",
            },
            "symbol": { "dynamicRegistration": false },
            "diagnostics": { "refreshSupport": true },
        },
        "textDocument": {
            "synchronization": { "dynamicRegistration": false, "didSave": true },
            "publishDiagnostics": {
                "versionSupport": true,
                "relatedInformation": true,
                "tagSupport": { "valueSet": [1, 2] },
            },
            "diagnostic": { "dynamicRegistration": true, "relatedDocumentSupport": false },
            "definition": { "linkSupport": true },
            "references": {},
            "hover": { "contentFormat": ["markdown", "plaintext"] },
            "documentSymbol": { "hierarchicalDocumentSymbolSupport": true },
            "rename": { "prepareSupport": true },
        },
        "window": { "workDoneProgress": true, "showMessage": {} },
        "experimental": { "serverStatusNotification": true },
    })
}

async fn write_loop(
    shared: Arc<Shared>,
    mut w: Box<dyn AsyncWrite + Send + Unpin>,
    mut rx: mpsc::UnboundedReceiver<Value>,
) {
    let mut closed = shared.subscribe();
    loop {
        let msg = tokio::select! {
            m = rx.recv() => match m {
                Some(m) => m,
                None => break,
            },
            // The connection closed: send what is queued (the `exit`), then stop.
            _ = closed.changed() => {
                if shared.lock().closed.is_none() {
                    continue;
                }
                while let Ok(m) = rx.try_recv() {
                    let _ = framing::write_message(&mut w, m.to_string().as_bytes()).await;
                }
                break;
            }
        };
        if let Err(e) = framing::write_message(&mut w, msg.to_string().as_bytes()).await {
            shared.close(format!("writing to the server failed: {e}"));
            break;
        }
    }
    let _ = tokio::io::AsyncWriteExt::shutdown(&mut w).await;
}

async fn read_loop(shared: Arc<Shared>, r: Box<dyn AsyncRead + Send + Unpin>) {
    let mut r = BufReader::with_capacity(64 * 1024, r);
    let max = shared.opts.max_message_bytes;
    let why = loop {
        match framing::read_message(&mut r, max).await {
            Ok(Some(body)) => match serde_json::from_slice::<Value>(&body) {
                Ok(v) => shared.dispatch(v),
                Err(e) => tracing::warn!(error = %e, "lsp: a message that is not JSON, skipped"),
            },
            Ok(None) => {
                shared.lock().eof = true;
                break "the server closed its stdout".to_string();
            }
            Err(e) if e.is_cut_short() => {
                shared.lock().eof = true;
                break "the server closed its stdout in the middle of a message".to_string();
            }
            Err(e) => break format!("reading the server's stdout failed: {e}"),
        }
    };
    shared.close(why);
}
