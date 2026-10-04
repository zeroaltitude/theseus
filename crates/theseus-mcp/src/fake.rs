//! A fake MCP server, for tests and for the simulator (36b's
//! `theseus-sim fake-mcp` can serve this same `Fake`). It speaks over a pair
//! of pipes (a client in this process), this process's stdin and stdout (the
//! `theseus-mcp-fake` binary), or streamable HTTP on 127.0.0.1.
//!
//! Its modes are `fake_discord`'s kind:
//! - `ok`: every tool answers;
//! - `slow`: every call answers after `slow_ms`;
//! - `crash-after N`: the call after the N-th is never answered: a process
//!   exits (status 3), pipes close, and over HTTP the connection drops;
//! - `change-tools`: each call changes the tool list, and
//!   `notifications/tools/list_changed` comes before its answer;
//! - `error`: every call answers `isError: true`.
//!
//! Its tools are `echo { text }` (`"ping me"` makes it ping the client
//! first), `add { a, b }` (with structured content), `sleep { ms }`,
//! `image`, and `fail`; its prompts are `greet { name }`, `brief { topic? }`,
//! and `review { path }`. Lists page `page_size` at a time. A call that asks
//! for progress gets two progress notifications before its answer.
//!
//! Over HTTP it gives a session id at `initialize` and wants it after (404
//! once [`Fake::expire_sessions`] forgets it); answers a request as JSON or
//! as a stream of events, each event written in two pieces after a comment;
//! offers the session's own stream on a GET; and, with `poll`, closes each
//! request's stream after a priming event and gives the rest to a GET that
//! resumes it with `Last-Event-ID` (2025-11-25's polling). It keeps what it
//! saw ([`Fake::seen`]) and never a header's value, so a key is never kept.

use std::collections::{HashMap, HashSet};
use std::io;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use serde_json::{json, Map, Value};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Notify};
use tokio::task::AbortHandle;
use tokio_util::sync::CancellationToken;

use crate::jsonrpc::{self, code, Incoming};
use crate::sse;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Ok,
    Slow,
    CrashAfter(u64),
    ChangeTools,
    /// Each `prompts/get` changes `greet`'s definition afterward, and says so
    /// (`notifications/prompts/list_changed`).
    ChangePrompts,
    Error,
}

impl Mode {
    /// `ok`, `slow`, `crash-after N` (or `crash-after=N`), `change-tools`,
    /// or `error`.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if let Some(n) = s.strip_prefix("crash-after") {
            return n
                .trim_start_matches(['=', ' '])
                .trim()
                .parse()
                .ok()
                .map(Mode::CrashAfter);
        }
        match s {
            "ok" => Some(Mode::Ok),
            "slow" => Some(Mode::Slow),
            "change-tools" => Some(Mode::ChangeTools),
            "change-prompts" => Some(Mode::ChangePrompts),
            "error" => Some(Mode::Error),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub mode: Mode,
    /// Tools and prompts per page, so that lists page.
    pub page_size: usize,
    /// How long a call takes in `slow`.
    pub slow_ms: u64,
    /// The revisions it speaks, newest first. A client that asks for another
    /// gets the first.
    pub versions: Vec<String>,
    /// HTTP: answer a request as a stream of events, or as JSON.
    pub sse: bool,
    /// HTTP: offer the session's own stream (a GET), or answer 405.
    pub server_stream: bool,
    /// HTTP: close each request's stream after a priming event, and give
    /// the rest to a GET that resumes it.
    pub poll: bool,
    /// HTTP: the key every request must carry as `Authorization: Bearer`.
    pub bearer: Option<String>,
    /// A process's fake exits (status 3) on a crash; otherwise its pipes
    /// close.
    pub exit_on_crash: bool,
    pub name: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mode: Mode::Ok,
            page_size: 2,
            slow_ms: 5_000,
            versions: crate::SUPPORTED_PROTOCOL_VERSIONS
                .iter()
                .map(|v| v.to_string())
                .collect(),
            sse: true,
            server_stream: true,
            poll: false,
            bearer: None,
            exit_on_crash: false,
            name: "fake".into(),
        }
    }
}

/// What the fake has seen.
#[derive(Debug, Clone, Default)]
pub struct Seen {
    /// Every message from a client, in order.
    pub messages: Vec<Value>,
    pub initializes: u32,
    pub initialized: u32,
    pub calls: u64,
    /// The ids of the requests a client cancelled.
    pub cancelled: Vec<Value>,
    /// A client's answers to the fake's own requests (its pings).
    pub answers: Vec<Value>,
    /// HTTP: each POST's `Mcp-Session-Id` and `MCP-Protocol-Version`.
    pub sessions: Vec<Option<String>>,
    pub versions: Vec<Option<String>>,
    /// HTTP: requests refused for a missing or a wrong key.
    pub unauthorized: u32,
    /// HTTP: the `Last-Event-ID` of each GET that resumed a stream.
    pub resumed: Vec<String>,
    /// HTTP: sessions a client ended with DELETE.
    pub deleted: Vec<String>,
    /// HTTP: GETs that opened a session's own stream.
    pub server_streams: u32,
}

pub struct Fake {
    cfg: Mutex<Config>,
    seen: Mutex<Seen>,
    changed: Notify,
    tools_version: AtomicU64,
    prompts_version: AtomicU64,
    next: AtomicU64,
    /// Requests being answered, by connection or session, then id: a cancel
    /// stops one.
    in_flight: Mutex<HashMap<(String, String), AbortHandle>>,
    http: Mutex<HttpState>,
    parked_changed: Notify,
}

#[derive(Default)]
struct HttpState {
    sessions: HashSet<String>,
    /// Each session's own stream, while a client holds it.
    streams: HashMap<String, mpsc::UnboundedSender<Value>>,
    /// `poll`: each closed stream's messages, by its priming id.
    parked: HashMap<String, Parked>,
}

#[derive(Default)]
struct Parked {
    messages: Vec<Value>,
    done: bool,
}

/// Where a message to the client goes.
#[derive(Clone)]
struct Sink(mpsc::UnboundedSender<Value>);

impl Sink {
    fn send(&self, m: Value) {
        let _ = self.0.send(m);
    }
}

enum Outcome {
    Answer(Value),
    Crash,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Fake {
    pub fn new(cfg: Config) -> Arc<Self> {
        Arc::new(Self {
            cfg: Mutex::new(cfg),
            seen: Mutex::new(Seen::default()),
            changed: Notify::new(),
            tools_version: AtomicU64::new(0),
            prompts_version: AtomicU64::new(0),
            next: AtomicU64::new(1),
            in_flight: Mutex::new(HashMap::new()),
            http: Mutex::new(HttpState::default()),
            parked_changed: Notify::new(),
        })
    }

    pub fn config(&self) -> Config {
        lock(&self.cfg).clone()
    }

    pub fn set_mode(&self, mode: Mode) {
        lock(&self.cfg).mode = mode;
    }

    pub fn seen(&self) -> Seen {
        lock(&self.seen).clone()
    }

    /// Wait until `pred` holds over what the fake has seen, up to `timeout`.
    pub async fn wait_until(&self, timeout: Duration, pred: impl Fn(&Seen) -> bool) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            // Registered before the check, so a change between the two is
            // not missed.
            let changed = self.changed.notified();
            if pred(&lock(&self.seen)) {
                return true;
            }
            if tokio::time::timeout_at(deadline, changed).await.is_err() {
                return pred(&lock(&self.seen));
            }
        }
    }

    /// HTTP: forget every session, as a restarted server does. The next
    /// request that carries one gets 404.
    pub fn expire_sessions(&self) {
        let mut h = lock(&self.http);
        h.sessions.clear();
        h.streams.clear();
    }

    fn record(&self, f: impl FnOnce(&mut Seen)) {
        f(&mut lock(&self.seen));
        self.changed.notify_waiters();
    }

    /// Record a message from a client, and act on what is not a request.
    /// Its requests come back, to be answered.
    fn intake(&self, msg: &Value, conn: &str) -> Vec<(Value, String, Value)> {
        self.record(|s| s.messages.push(msg.clone()));
        let mut requests = Vec::new();
        for m in jsonrpc::classify(msg.clone()).into_iter().flatten() {
            match m {
                Incoming::Request { id, method, params } => requests.push((id, method, params)),
                Incoming::Notification { method, params } => match method.as_str() {
                    "notifications/initialized" => self.record(|s| s.initialized += 1),
                    "notifications/cancelled" => {
                        let id = params.get("requestId").cloned().unwrap_or(Value::Null);
                        let key = (conn.to_string(), id.to_string());
                        if let Some(task) = lock(&self.in_flight).remove(&key) {
                            task.abort();
                        }
                        self.record(|s| s.cancelled.push(id));
                    }
                    _ => {}
                },
                Incoming::Response { .. } => self.record(|s| s.answers.push(msg.clone())),
            }
        }
        requests
    }

    /// Answer one request. `reply` carries what relates to it; `aside`, what
    /// does not (`list_changed`).
    async fn answer(
        self: Arc<Self>,
        id: Value,
        method: String,
        params: Value,
        reply: Sink,
        aside: Sink,
    ) -> Outcome {
        let ok = |result: Value| Outcome::Answer(jsonrpc::response(id.clone(), result));
        let err = |c: i64, m: &str| Outcome::Answer(jsonrpc::error_response(id.clone(), c, m));
        match method.as_str() {
            "initialize" => {
                self.record(|s| s.initializes += 1);
                let cfg = self.config();
                let asked = params
                    .get("protocolVersion")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let version = if cfg.versions.iter().any(|v| v == asked) {
                    asked.to_string()
                } else {
                    cfg.versions.first().cloned().unwrap_or_default()
                };
                ok(json!({
                    "protocolVersion": version,
                    "capabilities": {
                        "tools": { "listChanged": true },
                        "prompts": { "listChanged": true },
                        "logging": {}
                    },
                    "serverInfo": { "name": cfg.name, "version": "1.0.0" },
                    "instructions": "A fake MCP server, for Theseus's tests."
                }))
            }
            "ping" => ok(json!({})),
            "tools/list" => match self.page(&params, "tools", self.tools()) {
                Ok(r) => ok(r),
                Err(e) => err(code::INVALID_PARAMS, &e),
            },
            "prompts/list" => match self.page(&params, "prompts", self.prompts()) {
                Ok(r) => ok(r),
                Err(e) => err(code::INVALID_PARAMS, &e),
            },
            "prompts/get" => match get_prompt(&params) {
                Ok(r) => {
                    if self.config().mode == Mode::ChangePrompts {
                        self.prompts_version.fetch_add(1, Ordering::SeqCst);
                        aside.send(jsonrpc::notification(
                            "notifications/prompts/list_changed",
                            Value::Null,
                        ));
                    }
                    ok(r)
                }
                Err(e) => err(code::INVALID_PARAMS, &e),
            },
            "tools/call" => self.call(id.clone(), &params, &reply, &aside).await,
            _ => err(
                code::METHOD_NOT_FOUND,
                &format!("the fake has no {}", jsonrpc::clip(&method, 80)),
            ),
        }
    }

    async fn call(&self, id: Value, params: &Value, reply: &Sink, aside: &Sink) -> Outcome {
        let cfg = self.config();
        let n = {
            let mut s = lock(&self.seen);
            s.calls += 1;
            s.calls
        };
        self.changed.notify_waiters();
        if let Mode::CrashAfter(k) = cfg.mode {
            if n > k {
                return Outcome::Crash;
            }
        }
        let name = params.get("name").and_then(Value::as_str).unwrap_or("");
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if !self.tools().iter().any(|t| t["name"] == name) {
            return Outcome::Answer(jsonrpc::error_response(
                id,
                code::INVALID_PARAMS,
                &format!("Unknown tool: {}", jsonrpc::clip(name, 80)),
            ));
        }
        if cfg.mode == Mode::Slow {
            tokio::time::sleep(Duration::from_millis(cfg.slow_ms)).await;
        }
        if let Some(token) = params.get("_meta").and_then(|m| m.get("progressToken")) {
            for step in [1, 2] {
                reply.send(jsonrpc::notification(
                    "notifications/progress",
                    json!({ "progressToken": token, "progress": step, "total": 2 }),
                ));
            }
        }
        let result = if cfg.mode == Mode::Error {
            json!({
                "content": [{ "type": "text", "text": format!("the fake failed {name}, as its mode asks") }],
                "isError": true
            })
        } else {
            match name {
                "echo" => {
                    let text = args.get("text").and_then(Value::as_str).unwrap_or("");
                    if text == "ping me" {
                        let ping =
                            format!("fake-ping-{}", self.next.fetch_add(1, Ordering::Relaxed));
                        reply.send(jsonrpc::request(ping, "ping", Value::Null));
                    }
                    json!({ "content": [{ "type": "text", "text": text }] })
                }
                "add" => {
                    let num = |k: &str| args.get(k).and_then(Value::as_f64).unwrap_or(0.0);
                    let sum = num("a") + num("b");
                    json!({
                        "content": [{ "type": "text", "text": json!({ "sum": sum }).to_string() }],
                        "structuredContent": { "sum": sum }
                    })
                }
                "sleep" => {
                    let ms = args
                        .get("ms")
                        .and_then(Value::as_u64)
                        .unwrap_or(1_000)
                        .min(60_000);
                    tokio::time::sleep(Duration::from_millis(ms)).await;
                    json!({ "content": [{ "type": "text", "text": format!("slept {ms} ms") }] })
                }
                "image" => json!({
                    "content": [
                        { "type": "text", "text": "a one-pixel image:" },
                        { "type": "image", "data": TINY_PNG, "mimeType": "image/png" }
                    ]
                }),
                "fail" => json!({
                    "content": [{ "type": "text", "text": "the fail tool failed, as it always does" }],
                    "isError": true
                }),
                other => json!({ "content": [{ "type": "text", "text": format!("{other} ran") }] }),
            }
        };
        if cfg.mode == Mode::ChangeTools {
            self.tools_version.fetch_add(1, Ordering::SeqCst);
            aside.send(jsonrpc::notification(
                "notifications/tools/list_changed",
                Value::Null,
            ));
        }
        Outcome::Answer(jsonrpc::response(id, result))
    }

    fn prompts(&self) -> Vec<Value> {
        prompts_at(self.prompts_version.load(Ordering::SeqCst))
    }

    /// The tool list: five tools, then one more, and a changed `echo`, per
    /// change.
    fn tools(&self) -> Vec<Value> {
        let v = self.tools_version.load(Ordering::SeqCst);
        let echo = if v == 0 {
            "Echoes its text.".to_string()
        } else {
            format!("Echoes its text (list {v}).")
        };
        let object = |props: Value, required: &[&str]| json!({ "type": "object", "properties": props, "required": required });
        let mut tools = vec![
            json!({
                "name": "echo",
                "description": echo,
                "inputSchema": object(json!({ "text": { "type": "string" } }), &["text"]),
                "annotations": { "readOnlyHint": true }
            }),
            json!({
                "name": "add",
                "description": "Adds two numbers.",
                "inputSchema": object(json!({ "a": { "type": "number" }, "b": { "type": "number" } }), &["a", "b"]),
                "annotations": { "readOnlyHint": true, "idempotentHint": true }
            }),
            json!({
                "name": "sleep",
                "description": "Sleeps, then answers.",
                "inputSchema": object(json!({ "ms": { "type": "integer" } }), &[])
            }),
            json!({
                "name": "image",
                "description": "Answers with a one-pixel image.",
                "inputSchema": { "type": "object" }
            }),
            json!({
                "name": "fail",
                "description": "Always fails.",
                "inputSchema": { "type": "object" },
                "annotations": { "destructiveHint": false }
            }),
        ];
        for k in 1..=v.min(20) {
            tools.push(json!({
                "name": format!("tool_v{k}"),
                "description": format!("A tool that change {k} added."),
                "inputSchema": { "type": "object" }
            }));
        }
        tools
    }

    /// One page of `items`, from the request's cursor.
    fn page(&self, params: &Value, key: &str, items: Vec<Value>) -> Result<Value, String> {
        let size = self.config().page_size.max(1);
        let start = match params.get("cursor").and_then(Value::as_str) {
            None => 0,
            Some(c) => c
                .strip_prefix("page-")
                .and_then(|n| n.parse::<usize>().ok())
                .filter(|&n| n <= items.len())
                .ok_or_else(|| format!("Invalid cursor: {}", jsonrpc::clip(c, 40)))?,
        };
        let end = (start + size).min(items.len());
        let mut page = Map::new();
        page.insert(key.into(), Value::Array(items[start..end].to_vec()));
        if end < items.len() {
            page.insert("nextCursor".into(), json!(format!("page-{end}")));
        }
        Ok(Value::Object(page))
    }

    // ---- pipes and stdio ----

    /// Serve one client over a pair of pipes, until it closes its end, or
    /// the fake crashes.
    pub async fn serve_pipes<R, W>(self: Arc<Self>, reader: R, writer: W)
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let conn = format!("pipe-{}", self.next.fetch_add(1, Ordering::Relaxed));
        let (tx, mut rx) = mpsc::unbounded_channel::<Value>();
        let crash = CancellationToken::new();
        let stop = crash.clone();
        let mut writer = writer;
        let writing = tokio::spawn(async move {
            loop {
                let msg = tokio::select! {
                    _ = stop.cancelled() => break,
                    m = rx.recv() => match m {
                        Some(m) => m,
                        None => break,
                    },
                };
                let line = format!("{msg}\n");
                if writer.write_all(line.as_bytes()).await.is_err() || writer.flush().await.is_err()
                {
                    break;
                }
            }
            let _ = writer.shutdown().await;
        });
        let sink = Sink(tx);
        let mut lines = BufReader::new(reader).lines();
        loop {
            let line = tokio::select! {
                _ = crash.cancelled() => break,
                line = lines.next_line() => line,
            };
            let Ok(Some(line)) = line else { break };
            let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            for (id, method, params) in self.intake(&msg, &conn) {
                let key = (conn.clone(), id.to_string());
                let me = self.clone();
                let reply = sink.clone();
                let crash = crash.clone();
                let done_key = key.clone();
                let task = tokio::spawn(async move {
                    let outcome = me
                        .clone()
                        .answer(id, method, params, reply.clone(), reply.clone())
                        .await;
                    lock(&me.in_flight).remove(&done_key);
                    match outcome {
                        Outcome::Answer(a) => reply.send(a),
                        Outcome::Crash if me.config().exit_on_crash => std::process::exit(3),
                        Outcome::Crash => crash.cancel(),
                    }
                });
                lock(&self.in_flight).insert(key, task.abort_handle());
            }
        }
        // The client left, or the fake crashed: its answers stop.
        let conn_prefix = conn;
        lock(&self.in_flight).retain(|(c, _), task| {
            if *c == conn_prefix {
                task.abort();
                false
            } else {
                true
            }
        });
        crash.cancel();
        drop(sink);
        let _ = writing.await;
    }

    // ---- HTTP ----

    /// Listen on 127.0.0.1 at `port` (0 for any), at `/mcp`, until the
    /// process ends. Returns the address it listens on.
    pub async fn serve_http(self: Arc<Self>, port: u16) -> io::Result<SocketAddr> {
        let listener = TcpListener::bind(("127.0.0.1", port)).await?;
        let addr = listener.local_addr()?;
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    continue;
                };
                let me = self.clone();
                tokio::spawn(async move {
                    let _ = me.http_conn(stream).await;
                });
            }
        });
        Ok(addr)
    }

    async fn http_conn(self: Arc<Self>, stream: TcpStream) -> io::Result<()> {
        let (r, mut w) = stream.into_split();
        let mut r = BufReader::new(r);
        let head = tokio::time::timeout(Duration::from_secs(10), read_head(&mut r))
            .await
            .map_err(|_| io::Error::other("a request head that took too long"))??;
        if head.path.split('?').next() != Some("/mcp") {
            return respond(&mut w, 404, &[], "").await;
        }
        if let Some(key) = &self.config().bearer {
            let given = head.headers.get("authorization");
            if given.is_none_or(|g| *g != format!("Bearer {key}")) {
                self.record(|s| s.unauthorized += 1);
                return respond(&mut w, 401, &[("www-authenticate", "Bearer".into())], "").await;
            }
        }
        match head.method.as_str() {
            "POST" => self.http_post(head, r, w).await,
            "GET" => self.http_get(head, r, w).await,
            "DELETE" => {
                let ended = head
                    .headers
                    .get("mcp-session-id")
                    .filter(|s| lock(&self.http).sessions.remove(*s))
                    .cloned();
                match ended {
                    Some(s) => {
                        lock(&self.http).streams.remove(&s);
                        self.record(|seen| seen.deleted.push(s));
                        respond(&mut w, 200, &[], "").await
                    }
                    None => respond(&mut w, 404, &[], "").await,
                }
            }
            _ => respond(&mut w, 405, &[("allow", "GET, POST, DELETE".into())], "").await,
        }
    }

    #[expect(clippy::cognitive_complexity, reason = "shape budget: split it")]
    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
    async fn http_post(
        self: Arc<Self>,
        head: Head,
        mut r: BufReader<OwnedReadHalf>,
        mut w: OwnedWriteHalf,
    ) -> io::Result<()> {
        let len: usize = head
            .headers
            .get("content-length")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        if len > 4 << 20 {
            return respond(&mut w, 413, &[], "").await;
        }
        let mut body = vec![0; len];
        r.read_exact(&mut body).await?;
        let Ok(msg) = serde_json::from_slice::<Value>(&body) else {
            return respond(&mut w, 400, &[], "the body is not JSON").await;
        };
        let session = head.headers.get("mcp-session-id").cloned();
        let version = head.headers.get("mcp-protocol-version").cloned();
        self.record(|s| {
            s.sessions.push(session.clone());
            s.versions.push(version);
        });
        let initialize = msg.get("method").and_then(Value::as_str) == Some("initialize");
        let key = if initialize {
            let s = format!("fake-session-{}", self.next.fetch_add(1, Ordering::Relaxed));
            lock(&self.http).sessions.insert(s.clone());
            s
        } else {
            match session {
                None => return respond(&mut w, 400, &[], "a session id is required").await,
                Some(s) if !lock(&self.http).sessions.contains(&s) => {
                    return respond(&mut w, 404, &[], "no such session").await
                }
                Some(s) => s,
            }
        };
        let Some((id, method, params)) = self.intake(&msg, &key).into_iter().next() else {
            // A notification, or an answer to the fake.
            return respond(&mut w, 202, &[], "").await;
        };
        let cfg = self.config();
        let extra: Vec<(&str, String)> = if initialize {
            vec![("mcp-session-id", key.clone())]
        } else {
            Vec::new()
        };
        let (tx, mut rx) = mpsc::unbounded_channel::<Value>();
        let reply = Sink(tx);
        let aside = self.aside(&key, &reply);
        let flight = (key.clone(), id.to_string());
        let mut work = tokio::spawn(self.clone().answer(id, method, params, reply, aside));
        lock(&self.in_flight).insert(flight.clone(), work.abort_handle());
        if !cfg.sse {
            // JSON: only the answer; what came with it is dropped.
            let outcome = (&mut work).await;
            lock(&self.in_flight).remove(&flight);
            return match outcome {
                Ok(Outcome::Answer(a)) => {
                    let mut headers = extra;
                    headers.push(("content-type", "application/json".into()));
                    respond(&mut w, 200, &headers, &a.to_string()).await
                }
                // A crash, or a cancel: the connection drops unanswered.
                Ok(Outcome::Crash) | Err(_) => Ok(()),
            };
        }
        write_sse_head(&mut w, &extra).await?;
        w.write_all(b": the fake's stream\n\n").await?;
        w.flush().await?;
        if cfg.poll {
            // A priming event with an id and a retry time, then the stream
            // closes; the rest waits for a GET that resumes from that id.
            let prime = format!("p{}", self.next.fetch_add(1, Ordering::Relaxed));
            w.write_all(format!("id: {prime}\nretry: 50\ndata: \n\n").as_bytes())
                .await?;
            w.flush().await?;
            drop(w);
            lock(&self.http)
                .parked
                .insert(prime.clone(), Parked::default());
            let me = self.clone();
            tokio::spawn(async move {
                let outcome = loop {
                    tokio::select! {
                        m = rx.recv() => match m {
                            Some(m) => me.park(&prime, Some(m)),
                            None => break (&mut work).await,
                        },
                        done = &mut work => break done,
                    }
                };
                while let Ok(m) = rx.try_recv() {
                    me.park(&prime, Some(m));
                }
                lock(&me.in_flight).remove(&flight);
                if let Ok(Outcome::Answer(a)) = outcome {
                    me.park(&prime, Some(a));
                }
                me.park(&prime, None);
            });
            return Ok(());
        }
        let outcome = loop {
            tokio::select! {
                m = rx.recv() => match m {
                    Some(m) => write_event(&mut w, None, &m).await?,
                    None => break (&mut work).await,
                },
                done = &mut work => break done,
            }
        };
        lock(&self.in_flight).remove(&flight);
        while let Ok(m) = rx.try_recv() {
            write_event(&mut w, None, &m).await?;
        }
        match outcome {
            Ok(Outcome::Answer(a)) => write_event(&mut w, None, &a).await,
            // A crash, or a cancel: the stream ends with no answer.
            Ok(Outcome::Crash) | Err(_) => Ok(()),
        }
    }

    /// Where what does not belong to a request goes: the session's own
    /// stream, while a client holds it, else the request's.
    fn aside(&self, session: &str, reply: &Sink) -> Sink {
        match lock(&self.http).streams.get(session) {
            Some(tx) if !tx.is_closed() => Sink(tx.clone()),
            _ => reply.clone(),
        }
    }

    /// `poll`: one more message for a closed stream (`None`: its last).
    fn park(&self, prime: &str, m: Option<Value>) {
        if let Some(p) = lock(&self.http).parked.get_mut(prime) {
            match m {
                Some(m) => p.messages.push(m),
                None => p.done = true,
            }
        }
        self.parked_changed.notify_waiters();
    }

    async fn http_get(
        self: Arc<Self>,
        head: Head,
        mut r: BufReader<OwnedReadHalf>,
        mut w: OwnedWriteHalf,
    ) -> io::Result<()> {
        let Some(session) = head.headers.get("mcp-session-id").cloned() else {
            return respond(&mut w, 400, &[], "a session id is required").await;
        };
        if !lock(&self.http).sessions.contains(&session) {
            return respond(&mut w, 404, &[], "no such session").await;
        }
        if let Some(last) = head.headers.get("last-event-id").cloned() {
            self.record(|s| s.resumed.push(last.clone()));
            return self.replay(&last, w).await;
        }
        if !self.config().server_stream {
            return respond(&mut w, 405, &[("allow", "POST, DELETE".into())], "").await;
        }
        let (tx, mut rx) = mpsc::unbounded_channel();
        let taken = {
            let mut h = lock(&self.http);
            let open = h.streams.get(&session).is_some_and(|s| !s.is_closed());
            if !open {
                h.streams.insert(session.clone(), tx);
            }
            open
        };
        if taken {
            return respond(&mut w, 409, &[], "the session's stream is open").await;
        }
        self.record(|s| s.server_streams += 1);
        write_sse_head(&mut w, &[]).await?;
        w.write_all(b": the session's own stream\n\n").await?;
        w.flush().await?;
        let mut probe = [0u8; 1];
        loop {
            tokio::select! {
                m = rx.recv() => match m {
                    Some(m) => {
                        if write_event(&mut w, None, &m).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                },
                // The client hung up.
                _ = r.read(&mut probe) => break,
            }
        }
        Ok(())
    }

    /// `poll`: what followed `last` on a closed stream: its messages after
    /// that one, each with an id, until its answer.
    async fn replay(&self, last: &str, mut w: OwnedWriteHalf) -> io::Result<()> {
        let (prime, after) = match last.split_once('-') {
            Some((p, n)) => (p.to_string(), n.parse::<usize>().unwrap_or(0)),
            None => (last.to_string(), 0),
        };
        if !lock(&self.http).parked.contains_key(&prime) {
            return respond(&mut w, 400, &[], "an unknown event id").await;
        }
        write_sse_head(&mut w, &[]).await?;
        let mut next = after;
        loop {
            let changed = self.parked_changed.notified();
            let (messages, done) = {
                let h = lock(&self.http);
                let p = &h.parked[&prime];
                (p.messages.get(next..).unwrap_or_default().to_vec(), p.done)
            };
            for m in &messages {
                next += 1;
                write_event(&mut w, Some(&format!("{prime}-{next}")), m).await?;
            }
            if done {
                return Ok(());
            }
            if tokio::time::timeout(Duration::from_secs(30), changed)
                .await
                .is_err()
            {
                return Ok(());
            }
        }
    }
}

/// The prompt list: `greet` gains a description note and an optional `tone`
/// argument per change (`change-prompts`).
fn prompts_at(version: u64) -> Vec<Value> {
    let mut list = prompts();
    if version > 0 {
        list[0] = json!({
            "name": "greet",
            "description": format!("Greets someone (list {version})."),
            "arguments": [
                { "name": "name", "description": "Who to greet.", "required": true },
                { "name": "tone", "required": false }
            ]
        });
    }
    list
}

fn prompts() -> Vec<Value> {
    vec![
        json!({
            "name": "greet",
            "description": "Greets someone.",
            "arguments": [{ "name": "name", "description": "Who to greet.", "required": true }]
        }),
        json!({
            "name": "brief",
            "description": "Asks for a brief on a topic.",
            "arguments": [{ "name": "topic", "required": false }]
        }),
        json!({
            "name": "review",
            "description": "Asks for a review of a file.",
            "arguments": [{ "name": "path", "required": true }]
        }),
    ]
}

fn get_prompt(params: &Value) -> Result<Value, String> {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let arg = |k: &str| {
        params
            .get("arguments")
            .and_then(|a| a.get(k))
            .and_then(Value::as_str)
            .map(String::from)
    };
    let need = |k: &str| arg(k).ok_or_else(|| format!("prompt {name} needs its argument {k}"));
    let (description, text) = match name {
        "greet" => (
            "Greets someone.",
            format!("Say hello to {}.", need("name")?),
        ),
        "brief" => (
            "Asks for a brief on a topic.",
            format!(
                "Write a brief on {}.",
                arg("topic").unwrap_or_else(|| "anything".into())
            ),
        ),
        "review" => (
            "Asks for a review of a file.",
            format!("Review {}.", need("path")?),
        ),
        other => return Err(format!("Unknown prompt: {}", jsonrpc::clip(other, 80))),
    };
    Ok(json!({
        "description": description,
        "messages": [{ "role": "user", "content": { "type": "text", "text": text } }]
    }))
}

/// A 1×1 transparent PNG.
const TINY_PNG: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGNgYGBgAAAABQABpfZFQAAAAABJRU5ErkJggg==";

struct Head {
    method: String,
    path: String,
    /// Lower-cased names.
    headers: HashMap<String, String>,
}

async fn read_head(r: &mut BufReader<OwnedReadHalf>) -> io::Result<Head> {
    let mut line = String::new();
    r.read_line(&mut line).await?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();
    let mut headers = HashMap::new();
    let mut total = 0;
    loop {
        line.clear();
        let n = r.read_line(&mut line).await?;
        total += n;
        if n == 0 || total > 64 * 1024 {
            break;
        }
        let l = line.trim_end();
        if l.is_empty() {
            break;
        }
        if let Some((k, v)) = l.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    Ok(Head {
        method,
        path,
        headers,
    })
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        _ => "Unknown",
    }
}

async fn respond(
    w: &mut OwnedWriteHalf,
    status: u16,
    headers: &[(&str, String)],
    body: &str,
) -> io::Result<()> {
    let mut s = format!("HTTP/1.1 {status} {}\r\n", reason(status));
    for (k, v) in headers {
        s.push_str(&format!("{k}: {v}\r\n"));
    }
    s.push_str(&format!(
        "content-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    ));
    w.write_all(s.as_bytes()).await?;
    w.flush().await
}

/// A stream's head: no length, so its body runs until the connection
/// closes.
async fn write_sse_head(w: &mut OwnedWriteHalf, headers: &[(&str, String)]) -> io::Result<()> {
    let mut s = String::from(
        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\nconnection: close\r\n",
    );
    for (k, v) in headers {
        s.push_str(&format!("{k}: {v}\r\n"));
    }
    s.push_str("\r\n");
    w.write_all(s.as_bytes()).await?;
    w.flush().await
}

/// One event, written in two pieces, so a client reads split frames.
async fn write_event(w: &mut OwnedWriteHalf, id: Option<&str>, m: &Value) -> io::Result<()> {
    let frame = sse::frame(id, &m.to_string());
    let (a, b) = frame.as_bytes().split_at(frame.len() / 2);
    w.write_all(a).await?;
    w.flush().await?;
    tokio::task::yield_now().await;
    w.write_all(b).await?;
    w.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_parse() {
        assert_eq!(Mode::parse("ok"), Some(Mode::Ok));
        assert_eq!(Mode::parse(" crash-after 2 "), Some(Mode::CrashAfter(2)));
        assert_eq!(Mode::parse("crash-after=0"), Some(Mode::CrashAfter(0)));
        assert_eq!(Mode::parse("change-tools"), Some(Mode::ChangeTools));
        assert_eq!(Mode::parse("crash-after"), None);
        assert_eq!(Mode::parse("sideways"), None);
    }

    #[test]
    fn pages_and_prompts() {
        let fake = Fake::new(Config::default());
        let p = fake.page(&Value::Null, "tools", fake.tools()).unwrap();
        assert_eq!(p["tools"].as_array().unwrap().len(), 2);
        assert_eq!(p["nextCursor"], "page-2");
        let p = fake
            .page(&json!({"cursor": "page-4"}), "tools", fake.tools())
            .unwrap();
        assert_eq!(p["tools"][0]["name"], "fail");
        assert!(p.get("nextCursor").is_none());
        assert!(fake
            .page(&json!({"cursor": "page-9"}), "tools", fake.tools())
            .is_err());
        let g = get_prompt(&json!({"name": "greet", "arguments": {"name": "Eddie"}})).unwrap();
        assert_eq!(g["messages"][0]["content"]["text"], "Say hello to Eddie.");
        assert!(get_prompt(&json!({"name": "greet"})).is_err());
        assert!(get_prompt(&json!({"name": "nope"})).is_err());
    }
}
