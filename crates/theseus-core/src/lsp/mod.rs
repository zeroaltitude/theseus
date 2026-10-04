//! The language-server board (L2, theseus-n88g.8): the servers the `lsp.*`
//! tools drive, over theseus-lsp's client.
//!
//! - **Lazy, never on the start path.** Nothing starts with the daemon. A
//!   server starts at the first call for a file of its language under a
//!   root, one per (server, root). The root is the nearest marker at or above
//!   the file, inside the workspace roots ([`Board::root_for`]): for Rust,
//!   the nearest `Cargo.toml` with `[workspace]`, else the topmost.
//! - **Spawned as a job's program is**: through `children::spawn`
//!   (`Kind::Owned`, so the reaper never takes its status), in a process
//!   group of its own, with the job environment (`[tools] proc_env`), its
//!   stderr in a capped `<state>/lsp/<server>-<root hash>.log`.
//! - **The start is a run** (`gate`): a language server runs build scripts
//!   and proc macros, as `cargo build` would, so a call that would start one
//!   is judged at `proc.run`'s posture for the server's argv as well as its
//!   own, the stricter winning; once a server has started for a root, its
//!   calls take their own postures.
//! - **The stop.** A server stops when idle for `[lsp] idle_stop_mins`, at
//!   the daemon's stop (SIGTERM to its group, never waited for), or after a
//!   request it left unanswered for `request_timeout_secs`; one that ends
//!   unasked is `lsp.failed`. The next call starts it again. A call's cancel
//!   or `/stop` aborts its task, which drops its request: the client sends
//!   `$/cancelRequest`, and the server stays up.
//! - **Seen in** its facts (`fact::lsp`: `lsp.started`, `lsp.ready`,
//!   `lsp.stopped`, `lsp.failed`), health's `lsp` block, and each request's
//!   `lsp.request` span under its call's, which telemetry's
//!   `theseus.lsp.request.duration` reads. The count of servers up is
//!   health's, not a metric: the encoder has no gauge.

pub(crate) mod edits;
mod rename;
mod tools;

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, Weak};
use std::time::{Duration, Instant};

use serde_json::Value;
use sha2::{Digest, Sha256};
use theseus_lsp::servers::Preset;
use theseus_lsp::{Client, Event, Events};
use theseus_protocol::lsp::LspServerStatus;
use theseus_protocol::Span;
use theseus_tools::{Access, Plan, Resource, Tool};

use crate::config::LspConfig;
use crate::fact::lsp::{LspFailed, LspReady, LspStarted, LspStopped};
use crate::fact::Fact;
use crate::ledger::LedgerRow;
use crate::policy::Decision;
use crate::toolrun::ToolRuntime;

pub use edits::{Attached, EDITS};
pub use rename::RenameShown;

/// Every `lsp.*` tool, for the config's `[policy.tools]` check.
pub const NAMES: [&str; 7] = [
    "lsp.definition",
    "lsp.references",
    "lsp.hover",
    "lsp.symbols",
    "lsp.diagnostics",
    "lsp.rename.plan",
    "lsp.rename",
];

/// The built-in servers, in the order a file's server is chosen among those
/// installed.
pub const PRESETS: [&str; 6] = [
    "rust-analyzer",
    "ty",
    "pyright",
    "basedpyright",
    "tsgo",
    "typescript-language-server",
];

const RUST: &[&str] = &["rs"];
const PYTHON: &[&str] = &["py", "pyi"];
const TS_JS: &[&str] = &["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"];
const CARGO: &str = "Cargo.toml";

/// A preset by name, with the extensions it serves and its root markers.
pub fn preset(name: &str) -> Option<(Preset, &'static [&'static str], &'static [&'static str])> {
    use theseus_lsp::servers as s;
    let py: &[&str] = &["pyproject.toml", "setup.py", "requirements.txt"];
    let ts: &[&str] = &["tsconfig.json", "jsconfig.json", "package.json"];
    Some(match name {
        "rust-analyzer" => (s::rust_analyzer(), RUST, &[CARGO]),
        "ty" => (s::ty(), PYTHON, py),
        "pyright" => (s::pyright(), PYTHON, py),
        "basedpyright" => (s::basedpyright(), PYTHON, py),
        "tsgo" => (s::tsgo(), TS_JS, ts),
        // Its tsserver is the root's own, when it has one (`Spec::options`).
        "typescript-language-server" => (s::typescript_language_server(""), TS_JS, ts),
        _ => return None,
    })
}

/// One server the board may start: a preset, as `[lsp.servers]` changed
/// it, or one of the operator's own.
#[derive(Debug, Clone)]
pub struct Spec {
    pub name: String,
    pub argv: Vec<String>,
    pub extensions: Vec<String>,
    pub markers: Vec<String>,
    settings: Value,
    preset: Option<Preset>,
}

impl Spec {
    /// Every server `cfg` names, then every preset it does not, in order;
    /// none it turns off.
    pub fn all(cfg: &LspConfig) -> Vec<Spec> {
        let named = cfg.servers.keys().map(String::as_str);
        let rest = PRESETS
            .iter()
            .copied()
            .filter(|p| !cfg.servers.contains_key(*p));
        named
            .chain(rest)
            .filter_map(|name| {
                let c = cfg.servers.get(name).cloned().unwrap_or_default();
                if !c.enabled {
                    return None;
                }
                let p = preset(name);
                let strings = |v: &[&str]| v.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
                Some(Spec {
                    name: name.to_string(),
                    argv: c.command.or_else(|| p.as_ref().map(|p| p.0.argv.clone()))?,
                    extensions: c.extensions.or_else(|| p.as_ref().map(|p| strings(p.1)))?,
                    markers: c
                        .roots
                        .or_else(|| p.as_ref().map(|p| strings(p.2)))
                        .unwrap_or_default(),
                    settings: c
                        .settings
                        .and_then(|s| serde_json::to_value(s).ok())
                        .or_else(|| p.as_ref().map(|p| p.0.settings.clone()))
                        .unwrap_or(Value::Null),
                    preset: p.map(|p| p.0),
                })
            })
            .collect()
    }

    /// The client's options on `root`.
    fn options(&self, root: &Path, timeout: Duration) -> theseus_lsp::Options {
        let mut o = match &self.preset {
            Some(p) => p.options(root),
            None => theseus_lsp::Options::new(root),
        };
        o.settings = self.settings.clone();
        o.request_timeout = timeout;
        o.initialize_timeout = timeout.max(Duration::from_secs(30));
        if self.name == "typescript-language-server" {
            let ts = root.join("node_modules/typescript/lib/tsserver.js");
            o.initialization_options = match ts.is_file() {
                true => serde_json::json!({ "tsserver": { "path": ts } }),
                false => Value::Null,
            };
        }
        o
    }

    fn serves(&self, path: &Path) -> bool {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        self.extensions.iter().any(|e| e == ext)
    }
}

/// A started server's process, as the board's spawner hands it over.
pub struct Spawned {
    pub server: theseus_lsp::Server,
    pub pid: Option<u32>,
    /// SIGTERM to its group: the daemon's stop, which never waits.
    pub term: Option<Box<dyn Fn() + Send + Sync>>,
}

/// Starts a server. The daemon's goes through the children registry
/// ([`ChildrenSpawn`]); tests serve the fake in this process.
pub trait Spawn: Send + Sync + 'static {
    fn spawn(
        &self,
        argv: &[String],
        cwd: &Path,
        env: &[(String, String)],
        log: &Path,
    ) -> std::io::Result<Spawned>;

    /// Whether `program` is there to run, on the job's `PATH`.
    fn installed(&self, program: &str, path: Option<&str>) -> bool {
        installed(program, path)
    }
}

/// Whether `program` names a file, or is found on `path`.
pub fn installed(program: &str, path: Option<&str>) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let exec = |p: &Path| {
        std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };
    if program.contains('/') {
        return exec(Path::new(program));
    }
    path.unwrap_or_default()
        .split(':')
        .filter(|d| !d.is_empty())
        .any(|d| exec(&Path::new(d).join(program)))
}

/// The most a server's stderr log keeps.
pub const LOG_CAP_BYTES: u64 = 1 << 20;

/// The daemon's spawner: a child the registry knows (`Kind::Owned`, waited
/// for here), in its own process group, with exactly `env`.
pub struct ChildrenSpawn;

impl Spawn for ChildrenSpawn {
    fn spawn(
        &self,
        argv: &[String],
        cwd: &Path,
        env: &[(String, String)],
        log: &Path,
    ) -> std::io::Result<Spawned> {
        use std::process::Stdio;
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| std::io::Error::other("an empty command"))?;
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(args)
            .current_dir(cwd)
            .env_clear()
            .envs(env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        let mut child = theseus_kernel::children::spawn(
            theseus_kernel::children::Kind::Owned,
            || cmd.spawn(),
            tokio::process::Child::id,
        )?;
        let pid = child
            .id()
            .ok_or_else(|| std::io::Error::other("the server exited at once"))?;
        let (Some(stdin), Some(stdout), Some(stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            return Err(std::io::Error::other("the server's pipes are missing"));
        };
        tokio::spawn(capped_log(stderr, log.to_path_buf()));
        let exited = Arc::new(AtomicBool::new(false));
        let flag = exited.clone();
        // Its owner waits for it, so the registry's sweep never reaps it.
        tokio::spawn(async move {
            let _ = child.wait().await;
            flag.store(true, Ordering::SeqCst);
        });
        let signal = move |exited: &AtomicBool, sig: libc::c_int| {
            if exited.load(Ordering::SeqCst) {
                return;
            }
            let Ok(pgid) = i32::try_from(pid) else { return };
            // SAFETY: kill(2) only sends a signal. The group is the one
            // `process_group(0)` made, whose leader is not yet reaped
            // (checked above), so it is still this daemon's.
            unsafe {
                libc::kill(-pgid, sig);
            }
        };
        let (e1, e2) = (exited.clone(), exited);
        Ok(Spawned {
            server: theseus_lsp::Server {
                reader: Box::new(stdout),
                writer: Box::new(stdin),
                kill: Some(Box::new(move || signal(&e1, libc::SIGKILL))),
                pid: Some(pid),
            },
            pid: Some(pid),
            term: Some(Box::new(move || signal(&e2, libc::SIGTERM))),
        })
    }
}

/// A server's stderr into its log, up to [`LOG_CAP_BYTES`], then read and
/// dropped, so the server never blocks on a full pipe.
async fn capped_log(mut from: tokio::process::ChildStderr, path: PathBuf) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .await
        .ok();
    let mut written = file
        .as_ref()
        .and_then(|_| std::fs::metadata(&path).ok())
        .map_or(0, |m| m.len());
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        let n = match from.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        let Some(f) = file.as_mut() else { continue };
        let room = LOG_CAP_BYTES.saturating_sub(written) as usize;
        let take = n.min(room);
        if take > 0 && f.write_all(&buf[..take]).await.is_ok() {
            written += take as u64;
        }
        if take < n {
            let _ = f
                .write_all(b"\n[theseus: the log reached its cap; the rest is dropped]\n")
                .await;
            file = None;
        }
    }
}

/// Where the board's facts' rows go: the ledger, off the runtime's workers.
pub type Ledger = Arc<dyn Fn(LedgerRow) + Send + Sync>;

type Key = (String, PathBuf);

/// A server that is up.
pub struct Live {
    key: Key,
    pub client: Client,
    pub pid: Option<u32>,
    term: Option<Box<dyn Fn() + Send + Sync>>,
    started: tokio::time::Instant,
    since_ms: u64,
    ready_ms: Mutex<Option<u64>>,
    last_used: Mutex<tokio::time::Instant>,
    requests: AtomicU64,
    /// Tool requests waiting for their answers.
    busy: AtomicU64,
    /// The board stopped it, so its close is no failure.
    ended: AtomicBool,
    /// The files the tools opened in it, for `lsp.diagnostics` with no path.
    opened: Mutex<BTreeSet<PathBuf>>,
}

impl Live {
    pub fn server(&self) -> &str {
        &self.key.0
    }

    pub fn root(&self) -> &Path {
        &self.key.1
    }

    fn touch(&self) {
        *lock(&self.last_used) = tokio::time::Instant::now();
    }

    fn opened(&self, path: &Path) {
        lock(&self.opened).insert(path.to_path_buf());
    }

    pub fn open_files(&self) -> Vec<PathBuf> {
        lock(&self.opened).iter().cloned().collect()
    }
}

/// One request a tool made: for its call's span.
struct Traced {
    task: Option<tokio::task::Id>,
    tool_use_id: Option<String>,
    server: String,
    method: String,
    outcome: &'static str,
    start: Instant,
    end: Instant,
}

/// The requests kept for their calls' spans.
const TRACED: usize = 512;

pub struct Board {
    this: Weak<Board>,
    specs: Vec<Spec>,
    roots: Vec<PathBuf>,
    env: Vec<(String, String)>,
    log_dir: PathBuf,
    idle: Duration,
    timeout: Duration,
    spawner: Mutex<Arc<dyn Spawn>>,
    ledger: OnceLock<Ledger>,
    /// One start at a time per key.
    cells: Mutex<HashMap<Key, Arc<tokio::sync::Mutex<()>>>>,
    up: Mutex<BTreeMap<Key, Arc<Live>>>,
    /// Starts not yet answered: since when.
    starting: Mutex<BTreeMap<Key, u64>>,
    /// The last failure of each key not up: when, and why.
    failed: Mutex<BTreeMap<Key, (u64, String)>>,
    /// Every key a start was made for in this daemon's life: its later
    /// calls take their own postures.
    started: Mutex<BTreeSet<Key>>,
    traced: Mutex<VecDeque<Traced>>,
    /// The renames plans showed, by digest.
    renames: rename::Shows,
    /// The daemon is stopping: nothing starts.
    stopping: AtomicBool,
    /// What the daemon's stop signalled, kept until the process ends, so no
    /// drop kills them before their SIGTERM's grace.
    stopped: Mutex<Vec<Arc<Live>>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn now_ms() -> u64 {
    theseus_protocol::now_unix_ms()
}

/// A root's short name in its log's file name.
fn root_hash(root: &Path) -> String {
    hex::encode(&Sha256::digest(root.as_os_str().as_encoded_bytes())[..6])
}

/// Whether a `Cargo.toml` declares a workspace.
fn cargo_workspace(manifest: &Path) -> bool {
    std::fs::read_to_string(manifest).is_ok_and(|t| t.lines().any(|l| l.trim() == "[workspace]"))
}

impl Board {
    /// The board for `cfg`, over the workspace `roots` (canonical), the job
    /// environment `env`, and the state dir. Nothing starts.
    pub fn new(
        cfg: &LspConfig,
        roots: Vec<PathBuf>,
        env: Vec<(String, String)>,
        state_dir: &Path,
    ) -> Arc<Self> {
        Arc::new_cyclic(|this| Self {
            this: this.clone(),
            specs: Spec::all(cfg),
            roots,
            env,
            log_dir: state_dir.join("lsp"),
            idle: Duration::from_secs_f64(cfg.idle_stop_mins * 60.0),
            timeout: Duration::from_secs(cfg.request_timeout_secs),
            spawner: Mutex::new(Arc::new(ChildrenSpawn)),
            ledger: OnceLock::new(),
            cells: Mutex::default(),
            up: Mutex::default(),
            starting: Mutex::default(),
            failed: Mutex::default(),
            started: Mutex::default(),
            traced: Mutex::default(),
            renames: rename::Shows::default(),
            stopping: AtomicBool::new(false),
            stopped: Mutex::default(),
        })
    }

    /// The tools over this board.
    pub fn tools(self: &Arc<Self>) -> Vec<Arc<dyn Tool>> {
        tools::all(self)
    }

    /// Who starts servers: the children registry, or a test's stand-in.
    pub fn set_spawner(&self, s: Arc<dyn Spawn>) {
        *lock(&self.spawner) = s;
    }

    pub fn set_ledger(&self, l: Ledger) {
        let _ = self.ledger.set(l);
    }

    fn record<F: Fact>(&self, f: &F) {
        if let (Some(kind), Some(l)) = (F::KIND, self.ledger.get()) {
            l(LedgerRow::new(kind, None, None, f.row()));
        }
    }

    pub fn request_timeout(&self) -> Duration {
        self.timeout
    }

    fn job_path(&self) -> Option<String> {
        self.env
            .iter()
            .find(|(k, _)| k == "PATH")
            .map(|(_, v)| v.clone())
    }

    /// The server for `path`, and its root: the first whose extensions name
    /// the file's and whose program is installed. Synchronous, for a plan.
    pub fn server_for(&self, path: &Path) -> Result<(Spec, PathBuf), String> {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let serving: Vec<&Spec> = self.specs.iter().filter(|s| s.serves(path)).collect();
        if serving.is_empty() {
            let mut exts: Vec<&str> = self
                .specs
                .iter()
                .flat_map(|s| s.extensions.iter().map(String::as_str))
                .collect();
            exts.sort_unstable();
            exts.dedup();
            return Err(format!(
                "no language server serves {}: the servers serve .{}",
                match ext {
                    "" => format!("{} (it has no extension)", path.display()),
                    e => format!(".{e} files"),
                },
                exts.join(", .")
            ));
        }
        let spawner = lock(&self.spawner).clone();
        let path_var = self.job_path();
        let Some(spec) = serving
            .iter()
            .find(|s| spawner.installed(&s.argv[0], path_var.as_deref()))
        else {
            let names: Vec<&str> = serving.iter().map(|s| s.name.as_str()).collect();
            return Err(format!(
                "no language server for .{ext} files is installed: none of {} is on the job's PATH \
                 ([lsp.servers.<name>] command names another)",
                names.join(", ")
            ));
        };
        Ok(((*spec).clone(), self.root_for(spec, path)))
    }

    /// The workspace root `spec` serves `path` from: the nearest directory
    /// at or above it, inside the workspace root that holds it, with one of
    /// its markers; for `Cargo.toml`, the nearest that declares a workspace,
    /// else the topmost. With none, the workspace root, or, outside every
    /// root, the file's directory.
    pub fn root_for(&self, spec: &Spec, path: &Path) -> PathBuf {
        let top = self
            .roots
            .iter()
            .filter(|r| path.starts_with(r))
            .max_by_key(|r| r.components().count());
        let start = match path.is_dir() {
            true => Some(path),
            false => path.parent(),
        };
        let (mut nearest, mut workspace, mut topmost) = (None, None, None);
        let mut dir = start;
        while let Some(d) = dir {
            for m in &spec.markers {
                if !d.join(m).is_file() {
                    continue;
                }
                if m == CARGO {
                    if workspace.is_none() && cargo_workspace(&d.join(m)) {
                        workspace = Some(d);
                    }
                    topmost = Some(d);
                } else if nearest.is_none() {
                    nearest = Some(d);
                }
            }
            if Some(d) == top.map(PathBuf::as_path) {
                break;
            }
            dir = d.parent();
        }
        workspace
            .or(topmost)
            .or(nearest)
            .or(top.map(PathBuf::as_path))
            .or(start)
            .unwrap_or(path)
            .to_path_buf()
    }

    /// Whether a start was made for this server and root in this daemon's
    /// life.
    pub fn started_before(&self, server: &str, root: &Path) -> bool {
        lock(&self.started).contains(&(server.to_string(), root.to_path_buf()))
    }

    /// The servers up now.
    pub fn up(&self) -> Vec<Arc<Live>> {
        lock(&self.up).values().cloned().collect()
    }

    /// The server for `spec` on `root`, started if it is not up, and ready,
    /// or past the request timeout waiting to be.
    pub async fn live(self: &Arc<Self>, spec: &Spec, root: &Path) -> Result<Arc<Live>, String> {
        let live = self.get_or_start(spec, root).await?;
        live.touch();
        let _ = live.client.wait_ready(self.timeout).await;
        Ok(live)
    }

    async fn get_or_start(self: &Arc<Self>, spec: &Spec, root: &Path) -> Result<Arc<Live>, String> {
        let key: Key = (spec.name.clone(), root.to_path_buf());
        let cell = lock(&self.cells).entry(key.clone()).or_default().clone();
        let guard = cell.lock_owned().await;
        let found = lock(&self.up).get(&key).cloned();
        if let Some(l) = found {
            match l.client.closed() {
                None => return Ok(l),
                Some(why) => self.lost(&l, &why),
            }
        }
        if self.stopping.load(Ordering::SeqCst) {
            return Err("the daemon is stopping: no language server starts".into());
        }
        // The start is a task of its own: a cancelled call leaves it to
        // finish, for the next call.
        let (me, spec) = (self.clone(), spec.clone());
        tokio::spawn(async move {
            let _one = guard;
            me.start(&spec, key).await
        })
        .await
        .map_err(|e| format!("the language server's start failed: {e}"))?
    }

    async fn start(self: &Arc<Self>, spec: &Spec, key: Key) -> Result<Arc<Live>, String> {
        let root = key.1.clone();
        lock(&self.starting).insert(key.clone(), now_ms());
        let started = tokio::time::Instant::now();
        let fail = |pid: Option<u32>, why: String| {
            lock(&self.starting).remove(&key);
            self.record(&LspFailed {
                server: &spec.name,
                root: &root,
                pid,
                why: &why,
            });
            lock(&self.failed).insert(key.clone(), (now_ms(), why.clone()));
            Err(format!(
                "{} could not start on {}: {why}",
                spec.name,
                root.display()
            ))
        };
        let log = self
            .log_dir
            .join(format!("{}-{}.log", spec.name, root_hash(&root)));
        if let Err(e) = std::fs::create_dir_all(&self.log_dir) {
            return fail(None, format!("its log directory: {e}"));
        }
        let spawner = lock(&self.spawner).clone();
        let s = match spawner.spawn(&spec.argv, &root, &self.env, &log) {
            Ok(s) => s,
            Err(e) => return fail(None, format!("`{}`: {e}", spec.argv.join(" "))),
        };
        let pid = s.pid;
        lock(&self.started).insert(key.clone());
        self.record(&LspStarted {
            server: &spec.name,
            root: &root,
            pid,
            argv: &spec.argv,
            log: &log,
        });
        let (client, events) =
            match theseus_lsp::Client::start(s.server, spec.options(&root, self.timeout)).await {
                Ok(c) => c,
                Err(e) => {
                    if let Some(t) = &s.term {
                        t();
                    }
                    return fail(pid, format!("initialize: {e} (its log: {})", log.display()));
                }
            };
        let live = Arc::new(Live {
            key: key.clone(),
            client,
            pid,
            term: s.term,
            started,
            since_ms: now_ms(),
            ready_ms: Mutex::new(None),
            last_used: Mutex::new(tokio::time::Instant::now()),
            requests: AtomicU64::new(0),
            busy: AtomicU64::new(0),
            ended: AtomicBool::new(false),
            opened: Mutex::default(),
        });
        lock(&self.starting).remove(&key);
        lock(&self.failed).remove(&key);
        lock(&self.up).insert(key, live.clone());
        let board = Arc::downgrade(self);
        tokio::spawn(drain(events, board.clone(), Arc::downgrade(&live)));
        tokio::spawn(readiness(board.clone(), Arc::downgrade(&live)));
        tokio::spawn(idle_stop(board, Arc::downgrade(&live), self.idle));
        Ok(live)
    }

    /// A server that ended unasked: `lsp.failed`, and off the board.
    fn lost(&self, l: &Arc<Live>, why: &str) {
        if l.ended.swap(true, Ordering::SeqCst) {
            return;
        }
        self.take(l);
        self.record(&LspFailed {
            server: l.server(),
            root: l.root(),
            pid: l.pid,
            why,
        });
        lock(&self.failed).insert(l.key.clone(), (now_ms(), why.to_string()));
    }

    /// Off the board, if it is still the one there.
    fn take(&self, l: &Arc<Live>) {
        let mut up = lock(&self.up);
        if up.get(&l.key).is_some_and(|x| Arc::ptr_eq(x, l)) {
            up.remove(&l.key);
        }
    }

    /// Stop a server the board decided to (`idle`, `timeout`): `shutdown`,
    /// `exit`, then the kill after its grace.
    pub async fn stop_live(&self, l: &Arc<Live>, why: &str) {
        if l.ended.swap(true, Ordering::SeqCst) {
            return;
        }
        self.take(l);
        self.record(&LspStopped {
            server: l.server(),
            root: l.root(),
            pid: l.pid,
            why,
            ran_ms: l.started.elapsed().as_millis() as u64,
            requests: l.requests.load(Ordering::SeqCst),
        });
        l.client.stop().await;
    }

    /// The daemon stops: SIGTERM to each server's group, never waited for.
    pub fn stop_all(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        let all: Vec<Arc<Live>> = std::mem::take(&mut *lock(&self.up)).into_values().collect();
        for l in all {
            if l.ended.swap(true, Ordering::SeqCst) {
                continue;
            }
            if let Some(t) = &l.term {
                t();
            }
            self.record(&LspStopped {
                server: l.server(),
                root: l.root(),
                pid: l.pid,
                why: "daemon",
                ran_ms: l.started.elapsed().as_millis() as u64,
                requests: l.requests.load(Ordering::SeqCst),
            });
            lock(&self.stopped).push(l);
        }
    }

    /// One request of a tool's: timed for its call's span, counted, and,
    /// when it timed out, its server stopped (the next call starts it
    /// again). A dropped request (a cancel, `/stop`) is cancelled at the
    /// server by the client, and the server stays up.
    pub async fn call<T>(
        &self,
        live: &Arc<Live>,
        method: &str,
        req: impl std::future::Future<Output = Result<T, theseus_lsp::Error>>,
    ) -> Result<T, String> {
        let mut t = Tracking {
            board: self,
            live,
            method,
            start: Instant::now(),
            outcome: "cancelled",
        };
        live.busy.fetch_add(1, Ordering::SeqCst);
        live.requests.fetch_add(1, Ordering::SeqCst);
        let r = req.await;
        live.touch();
        t.outcome = match &r {
            Ok(_) => "ok",
            Err(theseus_lsp::Error::Timeout { .. }) => "timeout",
            Err(_) => "error",
        };
        drop(t);
        r.map_err(|e| match e {
            theseus_lsp::Error::Timeout { .. } => {
                let why = format!(
                    "{e}. {} on {} was stopped; the next call starts it again.",
                    live.server(),
                    live.root().display()
                );
                if let Some(board) = self.me() {
                    let l = live.clone();
                    tokio::spawn(async move { board.stop_live(&l, "timeout").await });
                }
                why
            }
            theseus_lsp::Error::Closed(why) => {
                self.lost(live, &why);
                format!(
                    "{} on {} ended: {why}. The next call starts it again.",
                    live.server(),
                    live.root().display()
                )
            }
            e => e.to_string(),
        })
    }

    fn me(&self) -> Option<Arc<Board>> {
        self.this.upgrade()
    }
}

/// A request in flight: on its end, or its drop, its span's record.
struct Tracking<'a> {
    board: &'a Board,
    live: &'a Arc<Live>,
    method: &'a str,
    start: Instant,
    outcome: &'static str,
}

impl Drop for Tracking<'_> {
    fn drop(&mut self) {
        self.live.busy.fetch_sub(1, Ordering::SeqCst);
        let mut t = lock(&self.board.traced);
        if t.len() >= TRACED {
            t.pop_front();
        }
        t.push_back(Traced {
            task: tokio::task::try_id(),
            tool_use_id: None,
            server: self.live.server().to_string(),
            method: self.method.to_string(),
            outcome: self.outcome,
            start: self.start,
            end: Instant::now(),
        });
    }
}

impl Board {
    /// The call whose task was `task` is `tool_use_id`: its requests become
    /// its spans (`toolrun`'s async run, once the task has ended).
    pub fn bind(&self, task: tokio::task::Id, tool_use_id: &str) {
        for t in lock(&self.traced).iter_mut() {
            if t.task == Some(task) && t.tool_use_id.is_none() {
                t.tool_use_id = Some(tool_use_id.to_string());
            }
        }
    }

    /// The `lsp.request` spans of one call's requests, placed on the turn's
    /// clock by `at`. A call's are taken once.
    pub fn spans(&self, tool_use_id: &str, at: impl Fn(Instant) -> u64) -> Vec<Span> {
        let mut t = lock(&self.traced);
        let mut out = Vec::new();
        t.retain(|r| {
            if r.tool_use_id.as_deref() != Some(tool_use_id) {
                return true;
            }
            out.push(crate::fact::lsp::request_span(
                &r.server,
                &r.method,
                r.outcome,
                at(r.start),
                at(r.end),
            ));
            false
        });
        out
    }

    /// Health's `lsp` block: each server up, each starting, and each key's
    /// last failure while it is not up.
    pub fn health(&self) -> Vec<LspServerStatus> {
        let now = tokio::time::Instant::now();
        let mut out: Vec<LspServerStatus> = self
            .up()
            .iter()
            .map(|l| {
                let ready = *lock(&l.ready_ms);
                LspServerStatus {
                    server: l.server().into(),
                    root: l.root().display().to_string(),
                    state: match ready {
                        Some(_) => "ready",
                        None => "loading",
                    }
                    .into(),
                    pid: l.pid,
                    since_ms: l.since_ms,
                    ready_ms: ready,
                    memory_kib: l
                        .pid
                        .map(theseus_lsp::spawn::group_rss_kib)
                        .filter(|k| *k > 0),
                    idle_secs: Some(now.saturating_duration_since(*lock(&l.last_used)).as_secs()),
                    requests: l.requests.load(Ordering::SeqCst),
                    why: None,
                }
            })
            .collect();
        for ((server, root), since) in lock(&self.starting).iter() {
            out.push(LspServerStatus {
                server: server.clone(),
                root: root.display().to_string(),
                state: "starting".into(),
                since_ms: *since,
                ..Default::default()
            });
        }
        let up = lock(&self.up);
        for ((server, root), (at, why)) in lock(&self.failed).iter() {
            if up.contains_key(&(server.clone(), root.clone())) {
                continue;
            }
            out.push(LspServerStatus {
                server: server.clone(),
                root: root.display().to_string(),
                state: "failed".into(),
                since_ms: *at,
                why: Some(why.clone()),
                ..Default::default()
            });
        }
        out.sort_by(|a, b| (&a.server, &a.root).cmp(&(&b.server, &b.root)));
        out
    }
}

/// The client's events, drained so its unbounded channel never grows: a
/// close the board did not ask for is a failure.
async fn drain(mut events: Events, board: Weak<Board>, live: Weak<Live>) {
    while let Some(e) = events.recv().await {
        if let Event::Closed { reason } = e {
            if let (Some(b), Some(l)) = (board.upgrade(), live.upgrade()) {
                b.lost(&l, &reason);
            }
            return;
        }
    }
}

/// How long a server may take to be ready before the board stops waiting
/// to say so: rust-analyzer took 18 s on a large workspace.
const READY_WITHIN: Duration = Duration::from_secs(600);

/// `lsp.ready`, once the server is.
async fn readiness(board: Weak<Board>, live: Weak<Live>) {
    let Some(client) = live.upgrade().map(|l| l.client.clone()) else {
        return;
    };
    let ready = client.wait_ready(READY_WITHIN).await.is_ok();
    drop(client);
    let (Some(b), Some(l)) = (board.upgrade(), live.upgrade()) else {
        return;
    };
    if !ready || l.client.closed().is_some() {
        return;
    }
    let ms = l.started.elapsed().as_millis() as u64;
    *lock(&l.ready_ms) = Some(ms);
    b.record(&LspReady {
        server: l.server(),
        root: l.root(),
        pid: l.pid,
        ready_ms: ms,
    });
}

/// The idle stop: once the server has gone `idle` unused, with no request
/// in flight, it stops.
async fn idle_stop(board: Weak<Board>, live: Weak<Live>, idle: Duration) {
    loop {
        let due = match live.upgrade() {
            Some(l) if !l.ended.load(Ordering::SeqCst) => *lock(&l.last_used) + idle,
            _ => return,
        };
        tokio::time::sleep_until(due).await;
        let (Some(b), Some(l)) = (board.upgrade(), live.upgrade()) else {
            return;
        };
        if l.ended.load(Ordering::SeqCst) {
            return;
        }
        let quiet = tokio::time::Instant::now() >= *lock(&l.last_used) + idle;
        if quiet && l.busy.load(Ordering::SeqCst) == 0 {
            b.stop_live(&l, "idle").await;
            return;
        }
        if !quiet {
            continue;
        }
        // Busy past its due: look again once the request has had its time.
        drop(l);
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

/// The gate's step for an `lsp.*` call (L2), after the call's own order:
/// a call that would start its server for a root none was started for is
/// judged at `proc.run`'s posture for the server's argv too, the stricter
/// winning, since a language server runs build scripts and proc macros as
/// `cargo build` would; and a rename's write outside the workspace roots
/// waits. It never loosens the call's own decision.
pub(crate) fn gate(rt: &ToolRuntime, tool: &dyn Tool, plan: &Plan, d: Decision) -> Decision {
    let Some(board) = rt.lsp.as_ref().filter(|_| tool.family() == "lsp") else {
        return d;
    };
    let d = rename::outside_roots(&rt.policy.roots, tool.name(), plan, d);
    let Some(path) = plan
        .resources
        .iter()
        .find(|r| r.access == Access::Read)
        .map(|r| r.path.clone())
    else {
        return d;
    };
    let Ok((spec, root)) = board.server_for(&path) else {
        return d;
    };
    if board.started_before(&spec.name, &root) {
        return d;
    }
    let Some(proc) = rt.registry.get("proc.run") else {
        return d;
    };
    let start = Plan {
        resources: vec![Resource {
            path: root.clone(),
            access: Access::Exec,
        }],
        argv: Some(spec.argv.clone()),
        summary: format!("start {} on {}", spec.name, root.display()),
        ..Default::default()
    };
    let t = rt.tightened.get("proc.run");
    let s = rt.policy.decide_with(
        proc.as_ref(),
        &start,
        t.as_ref().map(crate::tighten::as_tightened),
    );
    if s.posture <= d.posture {
        return d;
    }
    let why = format!(
        "this call starts {} on {} (`{}`), a program, judged as proc.run: {}",
        spec.name,
        root.display(),
        spec.argv.join(" "),
        s.reason
    );
    Decision {
        reason: format!("{}: {why}", plan.summary),
        notify: s.notify.map(|mut n| {
            n.rule = why.clone();
            n
        }),
        granted: d.granted,
        external: d.external,
        ..s
    }
}
