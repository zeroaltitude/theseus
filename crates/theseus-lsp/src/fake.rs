//! A scripted fake language server, for tests: in this process over pipes,
//! or as the `theseus-lsp-fake` binary (a crash exits with status 3).
//!
//! Its language is words. A line holding `ERROR` has an error diagnostic
//! there ("planted error"). `def NAME`, `fn NAME`, and `function NAME` define
//! NAME, which is a document symbol (a function). A definition, its
//! references, and a rename find NAME as a whole word in the open documents
//! and in the files directly under the root. A hover says the word.
//!
//! What it does is scripted by [`Config`]: diagnostics pushed (with or
//! without versions) or pulled (declared, or registered after
//! `initialized`), or pulled with a check after each save that pushes (as
//! rust-analyzer's `cargo check` does: a line holding `CHECK` is its "checked
//! error"), every answer slow, a crash after some requests, `exit` ignored
//! (as TypeScript 7's server does), and its own requests to the client
//! (`workspace/configuration`, `client/registerCapability`,
//! `window/workDoneProgress/create`, then a progress begun and ended). A
//! custom request, `fake/seen`, answers what it has seen ([`Seen`]).

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};
use tokio::io::{AsyncRead, AsyncWrite, BufReader};
use tokio::sync::{mpsc, oneshot};

use crate::framing;
use crate::jsonrpc::{self, code, Incoming};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diagnostics {
    /// `publishDiagnostics` after each open and change.
    Push,
    /// `diagnosticProvider` declared; nothing pushed.
    Pull,
    /// Nothing declared at `initialize`; pull registered 200 ms after
    /// `initialized`.
    PullRegistered,
    /// `diagnosticProvider` declared, as rust-analyzer does, and a check
    /// after each `didSave`: a progress begun under [`CHECK_TOKEN`], then,
    /// after `push_delay_ms`, a push for each open document (its `ERROR` and
    /// `CHECK` lines, with its version then), then the progress's end. A save
    /// during a check cancels it at once (its end, with no push), and the
    /// next check begins after the debounce.
    PullAndCheck,
}

/// The progress token of [`Diagnostics::PullAndCheck`]'s check.
pub const CHECK_TOKEN: &str = "fake/check/0";

/// How long after its save a check begins (rust-analyzer's took 60 to 80
/// ms in the probe: its 50 ms debounce, then the spawn).
pub const CHECK_DEBOUNCE_MS: u64 = 50;

#[derive(Debug, Clone)]
pub struct Config {
    pub diagnostics: Diagnostics,
    /// Pushed lists carry the document's version.
    pub versions: bool,
    /// Every answer but `initialize`'s, `shutdown`'s, and `fake/seen`'s
    /// waits this long (a `$/cancelRequest` ends the wait).
    pub slow_ms: u64,
    /// Before a push, wait this long (a slow checker).
    pub push_delay_ms: u64,
    /// `PullAndCheck`: the check's end goes out before its push, and the push
    /// just before the fake's next answer, as a server that writes both in
    /// one turn of its loop sends them before it answers what it reads next.
    pub check_end_first: bool,
    /// `PullAndCheck`: reports rust-analyzer's `experimental/serverStatus`,
    /// not quiescent until this long after `initialized`. A save before then
    /// starts no check; one runs as it becomes quiescent, as rust-analyzer's
    /// does once its workspace is loaded. 0: no status, ready at once.
    pub load_ms: u64,
    /// Exit, unanswered, at the request after this many.
    pub crash_after: Option<u32>,
    /// `exit` does nothing; the server runs on with its stdout open.
    pub ignore_exit: bool,
    /// After `initialized`, ask the client things.
    pub ask: bool,
    /// The binary's: a crash exits the process.
    pub exit_on_crash: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            diagnostics: Diagnostics::Push,
            versions: true,
            slow_ms: 0,
            push_delay_ms: 0,
            check_end_first: false,
            load_ms: 0,
            crash_after: None,
            ignore_exit: false,
            ask: false,
            exit_on_crash: false,
        }
    }
}

/// What the fake has seen, as `fake/seen` answers it.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Seen {
    pub requests: Vec<String>,
    /// `didOpen` and `didChange`: URI, then version.
    pub versions: Vec<(String, i64)>,
    pub saves: Vec<String>,
    pub closes: Vec<String>,
    /// `didChangeWatchedFiles`: URI, then type.
    pub watched: Vec<(String, i64)>,
    pub cancels: Vec<Value>,
    /// The client's answers to the fake's requests, by method.
    pub answers: BTreeMap<String, Value>,
    pub initialized: bool,
    /// `initialize`'s `initializationOptions`.
    pub initialization_options: Value,
    /// `initialize`'s `capabilities`: what the client says it can do.
    pub capabilities: Value,
    pub shutdown: bool,
    pub exit: bool,
}

struct Doc {
    version: i64,
    text: String,
}

struct State {
    seen: Seen,
    docs: HashMap<String, Doc>,
    root: Option<PathBuf>,
    count: u32,
    /// Requests waiting out `slow_ms`, by id, to end on a cancel.
    slow: HashMap<String, oneshot::Sender<()>>,
    /// The fake's own requests, by id: their methods.
    asked: HashMap<String, String>,
    /// `PullAndCheck`: the workspace is loaded (`load_ms` passed).
    loaded: bool,
    /// The checks asked for so far, and the one whose progress runs.
    checks: u64,
    checking: Option<u64>,
    /// Pushes that go out just before the next answer (`check_end_first`).
    held: Vec<Value>,
}

pub struct Fake {
    cfg: Config,
    state: Mutex<State>,
    out: mpsc::UnboundedSender<Value>,
}

/// What reading one message decided.
enum Next {
    Go,
    Exit,
    Crash,
}

impl Fake {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn send(&self, v: Value) {
        let _ = self.out.send(v);
    }

    /// Serve one client until `exit` (unless ignored), a crash, or the end
    /// of its input.
    pub async fn serve<R, W>(cfg: Config, reader: R, writer: W)
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (out, mut rx) = mpsc::unbounded_channel::<Value>();
        let loaded = cfg.load_ms == 0;
        let fake = Arc::new(Fake {
            cfg,
            state: Mutex::new(State {
                seen: Seen::default(),
                docs: HashMap::new(),
                root: None,
                count: 0,
                slow: HashMap::new(),
                asked: HashMap::new(),
                loaded,
                checks: 0,
                checking: None,
                held: Vec::new(),
            }),
            out,
        });
        let mut writer = writer;
        let writing = tokio::spawn(async move {
            while let Some(m) = rx.recv().await {
                if framing::write_message(&mut writer, m.to_string().as_bytes())
                    .await
                    .is_err()
                {
                    break;
                }
            }
            writer
        });
        let mut r = BufReader::new(reader);
        let crashed = loop {
            let Ok(Some(body)) = framing::read_message(&mut r, 64 << 20).await else {
                break false;
            };
            let Ok(v) = serde_json::from_slice::<Value>(&body) else {
                continue;
            };
            match fake.clone().take(v) {
                Next::Go => {}
                Next::Exit => break false,
                Next::Crash => break true,
            }
        };
        if crashed && fake.cfg.exit_on_crash {
            std::process::exit(3);
        }
        // Dropping the fake's sender ends the writer, which closes stdout.
        let ignore = fake.cfg.ignore_exit && !crashed && fake.lock().seen.exit;
        drop(fake);
        let w = writing.await;
        if ignore {
            // Its stdout stays open: hold the writer until the process is killed.
            let _held = w;
            std::future::pending::<()>().await;
        }
    }

    fn take(self: Arc<Self>, v: Value) -> Next {
        match jsonrpc::classify(v) {
            Ok(Incoming::Request { id, method, params }) => self.request(id, method, params),
            Ok(Incoming::Notification { method, params }) => self.notification(&method, &params),
            Ok(Incoming::Response { id, outcome }) => {
                let mut st = self.lock();
                let key = id
                    .as_str()
                    .map(String::from)
                    .unwrap_or_else(|| id.to_string());
                if let Some(method) = st.asked.remove(&key) {
                    let v = outcome.unwrap_or_else(|e| json!({ "error": e.code }));
                    st.seen.answers.insert(method, v);
                }
                Next::Go
            }
            Err(_) => Next::Go,
        }
    }

    fn request(self: Arc<Self>, id: Value, method: String, params: Value) -> Next {
        {
            let mut st = self.lock();
            st.count += 1;
            if self.cfg.crash_after.is_some_and(|n| st.count > n) {
                return Next::Crash;
            }
            st.seen.requests.push(method.clone());
        }
        let quick = matches!(method.as_str(), "initialize" | "shutdown" | "fake/seen");
        if quick || self.cfg.slow_ms == 0 {
            let answer = self.answer(&method, &params);
            self.reply(id, answer);
            return Next::Go;
        }
        let (tx, rx) = oneshot::channel();
        let key = id.to_string();
        self.lock().slow.insert(key.clone(), tx);
        let slow = Duration::from_millis(self.cfg.slow_ms);
        tokio::spawn(async move {
            let cancelled = tokio::select! {
                _ = tokio::time::sleep(slow) => false,
                _ = rx => true,
            };
            self.lock().slow.remove(&key);
            if cancelled {
                self.send(jsonrpc::error_response(
                    id,
                    code::REQUEST_CANCELLED,
                    "cancelled",
                ));
            } else {
                let answer = self.answer(&method, &params);
                self.reply(id, answer);
            }
        });
        Next::Go
    }

    fn reply(&self, id: Value, answer: Result<Value, (i64, String)>) {
        let held = std::mem::take(&mut self.lock().held);
        for push in held {
            self.send(push);
        }
        self.send(match answer {
            Ok(v) => jsonrpc::response(id, v),
            Err((c, m)) => jsonrpc::error_response(id, c, &m),
        });
    }

    fn answer(&self, method: &str, params: &Value) -> Result<Value, (i64, String)> {
        let at = || {
            let uri = params
                .pointer("/textDocument/uri")
                .and_then(Value::as_str)
                .unwrap_or("");
            let line = params
                .pointer("/position/line")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let col = params
                .pointer("/position/character")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            self.word_at(uri, line as usize, col as usize)
        };
        match method {
            "initialize" => Ok(self.initialize(params)),
            "shutdown" => {
                self.lock().seen.shutdown = true;
                Ok(Value::Null)
            }
            "fake/seen" => Ok(serde_json::to_value(&self.lock().seen).unwrap_or(Value::Null)),
            "textDocument/diagnostic" => Ok(self.pull(params)),
            "textDocument/definition" => Ok(at().map_or(Value::Null, |w| json!(self.definitions(&w)))),
            "textDocument/references" => Ok(at().map_or(json!([]), |w| json!(self.occurrences(&w)))),
            "textDocument/hover" => Ok(at().map_or(Value::Null, |w| {
                json!({ "contents": { "kind": "markdown", "value": format!("`{w}`: a fake symbol") } })
            })),
            "textDocument/documentSymbol" => {
                let uri = params.pointer("/textDocument/uri").and_then(Value::as_str).unwrap_or("");
                Ok(json!(self.symbols(uri)))
            }
            "workspace/symbol" => Ok(self.workspace_symbols(params)),
            "textDocument/rename" => {
                let name = params.get("newName").and_then(Value::as_str).unwrap_or("").to_string();
                Ok(at().map_or(Value::Null, |w| self.rename(&w, &name)))
            }
            _ => Err((code::METHOD_NOT_FOUND, format!("the fake has no {method}"))),
        }
    }

    fn initialize(&self, params: &Value) -> Value {
        self.lock().seen.initialization_options = params
            .get("initializationOptions")
            .cloned()
            .unwrap_or(Value::Null);
        self.lock().seen.capabilities = params.get("capabilities").cloned().unwrap_or(Value::Null);
        self.lock().root = params
            .get("rootUri")
            .and_then(Value::as_str)
            .and_then(crate::uri::to_path);
        let mut caps = json!({
            "textDocumentSync": { "openClose": true, "change": 1, "save": { "includeText": true } },
            "definitionProvider": true,
            "referencesProvider": true,
            "hoverProvider": true,
            "documentSymbolProvider": true,
            "workspaceSymbolProvider": true,
            "renameProvider": true,
        });
        if matches!(
            self.cfg.diagnostics,
            Diagnostics::Pull | Diagnostics::PullAndCheck
        ) {
            caps["diagnosticProvider"] =
                json!({ "interFileDependencies": false, "workspaceDiagnostics": false });
        }
        json!({ "capabilities": caps, "serverInfo": { "name": "theseus-lsp-fake", "version": "1" } })
    }

    fn notification(self: Arc<Self>, method: &str, params: &Value) -> Next {
        let uri = params
            .pointer("/textDocument/uri")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let version = params
            .pointer("/textDocument/version")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        match method {
            "initialized" => {
                self.lock().seen.initialized = true;
                self.after_initialized();
            }
            "textDocument/didOpen" => {
                let text = params
                    .pointer("/textDocument/text")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                self.set_doc(&uri, version, text.to_string());
            }
            "textDocument/didChange" => {
                let text = params
                    .pointer("/contentChanges/0/text")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                self.set_doc(&uri, version, text.to_string());
            }
            "textDocument/didSave" => {
                self.lock().seen.saves.push(uri);
                if self.cfg.diagnostics == Diagnostics::PullAndCheck && self.lock().loaded {
                    self.check();
                }
            }
            "textDocument/didClose" => {
                let mut st = self.lock();
                st.docs.remove(&uri);
                st.seen.closes.push(uri);
            }
            "workspace/didChangeWatchedFiles" => {
                let mut st = self.lock();
                for c in params
                    .get("changes")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let u = c
                        .get("uri")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    st.seen
                        .watched
                        .push((u, c.get("type").and_then(Value::as_i64).unwrap_or(0)));
                }
            }
            "$/cancelRequest" => {
                let id = params.get("id").cloned().unwrap_or(Value::Null);
                let mut st = self.lock();
                if let Some(tx) = st.slow.remove(&id.to_string()) {
                    let _ = tx.send(());
                }
                st.seen.cancels.push(id);
            }
            "exit" => {
                self.lock().seen.exit = true;
                if !self.cfg.ignore_exit {
                    return Next::Exit;
                }
            }
            _ => {}
        }
        Next::Go
    }

    fn ask(&self, id: &str, method: &str, params: Value) {
        self.lock().asked.insert(id.to_string(), method.to_string());
        self.send(jsonrpc::request(id, method, params));
    }

    fn after_initialized(self: &Arc<Self>) {
        if self.cfg.diagnostics == Diagnostics::PullAndCheck && self.cfg.load_ms > 0 {
            let status = |quiescent: bool| {
                jsonrpc::notification(
                    "experimental/serverStatus",
                    json!({ "health": "ok", "quiescent": quiescent }),
                )
            };
            self.send(status(false));
            let me = self.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(me.cfg.load_ms)).await;
                me.lock().loaded = true;
                me.send(status(true));
                me.check();
            });
        }
        if self.cfg.diagnostics == Diagnostics::PullRegistered {
            // Later, as ty registers it: after a client may already wait for a push.
            let me = self.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(200)).await;
                me.ask(
                    "fake-pull",
                    "client/registerCapability",
                    json!({ "registrations": [{ "id": "pull-1", "method": "textDocument/diagnostic",
                        "registerOptions": { "identifier": "fake", "interFileDependencies": false, "workspaceDiagnostics": false } }] }),
                );
            });
        }
        if !self.cfg.ask {
            return;
        }
        self.ask(
            "fake-config",
            "workspace/configuration",
            json!({ "items": [{ "section": "fake.check" }, { "section": "fake.missing" }, {}] }),
        );
        self.ask(
            "fake-register",
            "client/registerCapability",
            json!({ "registrations": [{ "id": "watch-1", "method": "workspace/didChangeWatchedFiles",
                "registerOptions": { "watchers": [{ "globPattern": "**/*.fake" }] } }] }),
        );
        self.ask(
            "fake-progress",
            "window/workDoneProgress/create",
            json!({ "token": "indexing" }),
        );
        self.ask(
            "fake-edit",
            "workspace/applyEdit",
            json!({ "edit": { "changes": {} } }),
        );
        self.ask("fake-unknown", "fake/unknownRequest", Value::Null);
        self.send(jsonrpc::notification(
            "$/progress",
            json!({ "token": "indexing", "value": { "kind": "begin", "title": "Indexing" } }),
        ));
    }

    /// A check, as rust-analyzer's `cargo check` after a save: a check that
    /// runs is cancelled at once (its end goes out, with no push); this one's
    /// progress begins [`CHECK_DEBOUNCE_MS`] later (a save meanwhile folds
    /// into it), then, after `push_delay_ms`, a push for each open document,
    /// then the progress's end.
    fn check(self: &Arc<Self>) {
        let mine = {
            let mut st = self.lock();
            st.checks += 1;
            if st.checking.take().is_some() {
                self.send(progress("end", None));
            }
            st.checks
        };
        let me = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(CHECK_DEBOUNCE_MS)).await;
            {
                let mut st = me.lock();
                if st.checks != mine {
                    return;
                }
                st.checking = Some(mine);
                me.send(progress("begin", Some("cargo check")));
            }
            tokio::time::sleep(Duration::from_millis(me.cfg.push_delay_ms)).await;
            let pushes = {
                let mut st = me.lock();
                if st.checking != Some(mine) {
                    return;
                }
                st.checking = None;
                let mut docs: Vec<(&String, &Doc)> = st.docs.iter().collect();
                docs.sort_by(|a, b| a.0.cmp(b.0));
                docs.into_iter()
                    .map(|(uri, doc)| {
                        let mut items = planted(&doc.text);
                        items.extend(checked(&doc.text));
                        let mut p = json!({ "uri": uri, "diagnostics": items });
                        if me.cfg.versions {
                            p["version"] = json!(doc.version);
                        }
                        jsonrpc::notification("textDocument/publishDiagnostics", p)
                    })
                    .collect::<Vec<_>>()
            };
            if me.cfg.check_end_first {
                me.send(progress("end", None));
                me.lock().held.extend(pushes);
            } else {
                for p in pushes {
                    me.send(p);
                }
                me.send(progress("end", None));
            }
        });
    }

    fn set_doc(self: Arc<Self>, uri: &str, version: i64, text: String) {
        {
            let mut st = self.lock();
            st.seen.versions.push((uri.to_string(), version));
            st.docs.insert(uri.to_string(), Doc { version, text });
        }
        if self.cfg.diagnostics != Diagnostics::Push {
            return;
        }
        let uri = uri.to_string();
        let delay = self.cfg.push_delay_ms;
        let push = move |me: &Fake| {
            let st = me.lock();
            let Some(doc) = st.docs.get(&uri) else { return };
            let mut p = json!({ "uri": uri, "diagnostics": planted(&doc.text) });
            if me.cfg.versions {
                p["version"] = json!(doc.version);
            }
            drop(st);
            me.send(jsonrpc::notification("textDocument/publishDiagnostics", p));
        };
        if delay == 0 {
            push(&self);
        } else {
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(delay)).await;
                push(&self);
            });
        }
    }

    fn pull(&self, params: &Value) -> Value {
        let uri = params
            .pointer("/textDocument/uri")
            .and_then(Value::as_str)
            .unwrap_or("");
        let prev = params.get("previousResultId").and_then(Value::as_str);
        let st = self.lock();
        let Some(doc) = st.docs.get(uri) else {
            return json!({ "kind": "full", "items": [] });
        };
        let id = doc.version.to_string();
        if prev == Some(id.as_str()) {
            return json!({ "kind": "unchanged", "resultId": id });
        }
        json!({ "kind": "full", "resultId": id, "items": planted(&doc.text) })
    }

    /// Every document's text: the open ones, then the files directly under
    /// the root that are not open.
    fn texts(&self) -> Vec<(String, Option<i64>, String)> {
        let st = self.lock();
        let mut out: Vec<_> = st
            .docs
            .iter()
            .map(|(u, d)| (u.clone(), Some(d.version), d.text.clone()))
            .collect();
        if let Some(root) = &st.root {
            for e in std::fs::read_dir(root).into_iter().flatten().flatten() {
                let Some(uri) = crate::uri::from_path(&e.path()) else {
                    continue;
                };
                if st.docs.contains_key(&uri) {
                    continue;
                }
                if let Ok(text) = std::fs::read_to_string(e.path()) {
                    out.push((uri, None, text));
                }
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    fn word_at(&self, uri: &str, line: usize, col: usize) -> Option<String> {
        let st = self.lock();
        let row = st.docs.get(uri)?.text.lines().nth(line)?.to_string();
        drop(st);
        let byte = crate::position::byte_column(&row, u32::try_from(col).ok()?);
        let ident = |c: char| c.is_alphanumeric() || c == '_';
        let start = row[..byte].rfind(|c| !ident(c)).map_or(0, |i| i + 1);
        let end = row[byte..]
            .find(|c| !ident(c))
            .map_or(row.len(), |i| byte + i);
        (start < end).then(|| row[start..end].to_string())
    }

    /// Whole-word occurrences of `word`: (URI, version, range).
    fn ranges(&self, word: &str) -> Vec<(String, Option<i64>, Value)> {
        let mut out = Vec::new();
        for (uri, version, text) in self.texts() {
            for (n, row) in text.lines().enumerate() {
                for (i, _) in row.match_indices(word) {
                    let ident =
                        |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
                    if ident(row[..i].chars().next_back())
                        || ident(row[i + word.len()..].chars().next())
                    {
                        continue;
                    }
                    let c0 = crate::position::utf16_column(row, i);
                    let c1 = crate::position::utf16_column(row, i + word.len());
                    out.push((uri.clone(), version, range(n, c0, c1)));
                }
            }
        }
        out
    }

    fn occurrences(&self, word: &str) -> Vec<Value> {
        self.ranges(word)
            .into_iter()
            .map(|(uri, _, r)| json!({ "uri": uri, "range": r }))
            .collect()
    }

    fn definitions(&self, word: &str) -> Vec<Value> {
        let mut out = Vec::new();
        for (uri, _, text) in self.texts() {
            for (n, row) in text.lines().enumerate() {
                for kw in ["def ", "fn ", "function "] {
                    let pat = format!("{kw}{word}");
                    let Some(i) = row.find(&pat) else { continue };
                    let after = row[i + pat.len()..].chars().next();
                    if after.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                        continue;
                    }
                    let s = i + kw.len();
                    let c0 = crate::position::utf16_column(row, s);
                    let c1 = crate::position::utf16_column(row, s + word.len());
                    out.push(json!({ "uri": uri, "range": range(n, c0, c1) }));
                }
            }
        }
        out
    }

    fn symbols(&self, uri: &str) -> Vec<Value> {
        let st = self.lock();
        let Some(doc) = st.docs.get(uri) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (n, row) in doc.text.lines().enumerate() {
            for kw in ["def ", "fn ", "function "] {
                let Some(i) = row.find(kw) else { continue };
                let s = i + kw.len();
                let name: String = row[s..]
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if name.is_empty() {
                    continue;
                }
                let c0 = crate::position::utf16_column(row, s);
                let c1 = crate::position::utf16_column(row, s + name.len());
                let end = crate::position::utf16_column(row, row.len());
                out.push(json!({ "name": name, "kind": 12, "range": range(n, 0, end), "selectionRange": range(n, c0, c1) }));
            }
        }
        out
    }

    fn workspace_symbols(&self, params: &Value) -> Value {
        let q = params.get("query").and_then(Value::as_str).unwrap_or("");
        let mut out = Vec::new();
        for (uri, _, text) in self.texts() {
            for (n, row) in text.lines().enumerate() {
                let Some(name) = ["def ", "fn ", "function "].iter().find_map(|kw| {
                    let s = row.find(kw)? + kw.len();
                    Some(
                        row[s..]
                            .chars()
                            .take_while(|c| c.is_alphanumeric() || *c == '_')
                            .collect::<String>(),
                    )
                }) else {
                    continue;
                };
                if name.is_empty() || !name.contains(q) {
                    continue;
                }
                let c = crate::position::utf16_column(row, row.find(&name).unwrap_or(0));
                let len = u32::try_from(name.encode_utf16().count()).unwrap_or(0);
                out.push(json!({ "name": name, "kind": 12, "location": { "uri": uri, "range": range(n, c, c + len) } }));
            }
        }
        json!(out)
    }

    fn rename(&self, word: &str, new_name: &str) -> Value {
        let mut by_doc: BTreeMap<String, (Option<i64>, Vec<Value>)> = BTreeMap::new();
        for (uri, version, r) in self.ranges(word) {
            by_doc
                .entry(uri)
                .or_insert((version, Vec::new()))
                .1
                .push(json!({ "range": r, "newText": new_name }));
        }
        let changes: Vec<Value> = by_doc
            .into_iter()
            .map(|(uri, (version, edits))| json!({ "textDocument": { "uri": uri, "version": version }, "edits": edits }))
            .collect();
        json!({ "documentChanges": changes })
    }
}

fn range(line: usize, c0: u32, c1: u32) -> Value {
    json!({ "start": { "line": line, "character": c0 }, "end": { "line": line, "character": c1 } })
}

/// A diagnostic at each `ERROR`.
fn planted(text: &str) -> Vec<Value> {
    marked(text, "ERROR", "fake", "F1", "planted error")
}

/// A diagnostic at each `CHECK`: what the check after a save finds.
fn checked(text: &str) -> Vec<Value> {
    marked(text, "CHECK", "fake-check", "C1", "checked error")
}

fn marked(text: &str, mark: &str, source: &str, code: &str, message: &str) -> Vec<Value> {
    let mut out = Vec::new();
    for (n, row) in text.lines().enumerate() {
        if let Some(i) = row.find(mark) {
            let c0 = crate::position::utf16_column(row, i);
            let c1 = crate::position::utf16_column(row, i + mark.len());
            out.push(json!({
                "range": range(n, c0, c1), "severity": 1, "source": source,
                "code": code, "message": message,
            }));
        }
    }
    out
}

/// A `$/progress` of the check's token.
fn progress(kind: &str, title: Option<&str>) -> Value {
    let mut value = json!({ "kind": kind });
    if let Some(t) = title {
        value["title"] = json!(t);
    }
    jsonrpc::notification(
        "$/progress",
        json!({ "token": CHECK_TOKEN, "value": value }),
    )
}

/// The binary's arguments, parsed into a config.
pub fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Config, String> {
    let mut cfg = Config::default();
    let mut args = args.into_iter();
    let number = |v: Option<String>, flag: &str| -> Result<u64, String> {
        let v = v.ok_or_else(|| format!("{flag} needs a value"))?;
        v.parse()
            .map_err(|_| format!("{flag} wants a number, not {v:?}"))
    };
    while let Some(a) = args.next() {
        match a.as_str() {
            "--push" => cfg.diagnostics = Diagnostics::Push,
            "--pull" => cfg.diagnostics = Diagnostics::Pull,
            "--pull-registered" => cfg.diagnostics = Diagnostics::PullRegistered,
            "--pull-and-check" => cfg.diagnostics = Diagnostics::PullAndCheck,
            "--no-versions" => cfg.versions = false,
            "--slow-ms" => cfg.slow_ms = number(args.next(), "--slow-ms")?,
            "--push-delay-ms" => cfg.push_delay_ms = number(args.next(), "--push-delay-ms")?,
            "--check-end-first" => cfg.check_end_first = true,
            "--load-ms" => cfg.load_ms = number(args.next(), "--load-ms")?,
            "--crash-after" => {
                cfg.crash_after =
                    Some(u32::try_from(number(args.next(), "--crash-after")?).unwrap_or(u32::MAX));
            }
            "--ignore-exit" => cfg.ignore_exit = true,
            "--ask" => cfg.ask = true,
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(cfg)
}
