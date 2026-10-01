//! Toollets (spec §3.23, §3.24): small typed in-process tools behind one
//! contract. A toollet parses and validates its own input (`plan`), names the
//! paths or argv it will touch so policy can read intent, and either runs in
//! process (`run`, milliseconds, no shell) or describes a job for the detached
//! wrapper (`job`, `proc.run` only). Output is text for the model plus
//! structured `meta` for the ledger and the web UI.
//!
//! Names are canonical and dotted (`fs.read`); the provider's tool-name rule
//! (`^[a-zA-Z0-9_-]{1,64}$`) makes the wire name `fs_read`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub mod fs;
pub mod git;
pub mod image;
pub mod net;
pub mod paths;
pub mod proc;
pub mod text;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolClass {
    /// Reads files or repository state; changes nothing.
    Read,
    /// Changes files.
    Write,
    /// Runs a program.
    Run,
}

impl ToolClass {
    pub fn as_str(self) -> &'static str {
        match self {
            ToolClass::Read => "read",
            ToolClass::Write => "write",
            ToolClass::Run => "run",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    /// In process, synchronous, bounded.
    Inproc,
    /// The detached job wrapper (§3.16): spooled result, own deadline, cancellable.
    Job,
    /// A future on the daemon's runtime (`run_async`): a tool that waits on
    /// the network (DD5), so it holds no core while it waits.
    Async,
    /// The harness itself runs it, inside the turn that calls it: a verb over
    /// Theseus's own state (`task.create`, DD7), which needs the turn's kernel
    /// and store, not the world. The toollet only plans it.
    Harness,
}

impl Backend {
    pub fn as_str(self) -> &'static str {
        match self {
            Backend::Inproc => "inproc",
            Backend::Job => "job",
            Backend::Async => "async",
            Backend::Harness => "harness",
        }
    }
}

/// What a repeat would do (§3.16 retry classes, mapped by the harness).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Retry {
    SafeToRepeat,
    NonRepeatable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    Read,
    Write,
    /// A directory a program runs in.
    Exec,
}

/// A path a call will touch, already resolved against the context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resource {
    pub path: PathBuf,
    pub access: Access,
}

/// What a call will do, before it does it: the gate reads this.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Plan {
    pub resources: Vec<Resource>,
    /// For `proc.run`: the exact argv.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub argv: Option<Vec<String>>,
    /// For a network tool: the URL it asks for (`http.fetch`'s, or the
    /// request `web.search` makes). The gate judges its host (DD5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// One line for humans ("edit src/main.rs (1 occurrence)").
    pub summary: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolOutput {
    pub text: String,
    #[serde(default)]
    pub meta: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolFailure {
    pub message: String,
    #[serde(default)]
    pub meta: Value,
}

impl ToolFailure {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            meta: Value::Null,
        }
    }
}

impl<E: std::fmt::Display> From<E> for ToolFailure {
    fn from(e: E) -> Self {
        ToolFailure::new(e.to_string())
    }
}

/// Where a result's text came from, when it came from outside Theseus (spec
/// §5.2): a page `http.fetch` read, or the results `web.search` got. Such
/// text never becomes durable without the operator's confirmation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct External {
    /// The page's final URL, or the search's request.
    pub url: String,
}

/// What an async tool's run gives: its output, and where the output's text
/// came from when that is outside Theseus.
pub type AsyncResult = Result<(ToolOutput, Option<External>), ToolFailure>;

/// An async tool's run, a future on the daemon's runtime (DD5).
pub type AsyncRun = std::pin::Pin<Box<dyn std::future::Future<Output = AsyncResult> + Send>>;

/// An image a toollet read, for the model to see (`fs.read` of a PNG,
/// theseus-9g2). The runtime stores its bytes once, in the store's blobs,
/// and the result node holds the reference.
#[derive(Debug, Clone)]
pub struct ImageData {
    /// The file's name, for the line that names it.
    pub name: String,
    pub info: image::ImageInfo,
    pub bytes: Vec<u8>,
}

/// A job for the detached wrapper.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobSpec {
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub timeout_secs: u64,
    /// Extra environment the model asked for (values are ledgered; secrets never go here).
    #[serde(default)]
    pub env: Vec<(String, String)>,
}

/// Free cores a toollet may borrow for work it can split (theseus-a60): the
/// daemon's CPU pool. A job starts only on a core that is free now, so a
/// toollet already holding one never waits for another, and a pool full of
/// them cannot deadlock.
pub trait Cores: Send + Sync + std::fmt::Debug {
    /// Run `job` on a free core, and say whether one was free. A job that
    /// did not start is dropped; the caller does its work itself.
    fn try_spawn(&self, job: Box<dyn FnOnce() + Send>) -> bool;
}

/// A toollet's secrets (theseus-dcy): the secret broker, bound to one call of
/// one tool. A toollet asks for a `[secrets]` name, and gets its value only
/// when the broker granted that secret to the tool and it has resolved.
pub trait Secrets: Send + Sync + std::fmt::Debug {
    /// The value, or why this call does not get it (never a value).
    fn secret(&self, name: &str) -> Result<zeroize::Zeroizing<String>, String>;
}

/// What every toollet gets: where it may work and how much it may return.
#[derive(Debug, Clone)]
pub struct ToolCtx {
    /// Canonical workspace roots. Relative paths resolve against `cwd`.
    pub roots: Vec<PathBuf>,
    /// The gate's floor paths, canonical: Theseus's own state and the
    /// 1Password CLI's credentials. A tool that reads more than the paths its
    /// plan names (`git.diff`'s working tree) skips whatever is under one
    /// (theseus-bsc). Empty in tests that do not set it.
    pub floor: Vec<PathBuf>,
    pub cwd: PathBuf,
    /// Cap on bytes a read returns.
    pub max_read_bytes: usize,
    /// Cap on entries a listing, glob, or grep returns.
    pub max_entries: usize,
    /// Default and ceiling for `proc.run` timeouts.
    pub proc_timeout_secs: u64,
    pub proc_timeout_max_secs: u64,
    /// The daemon's free cores, when it lends them (`fs.grep` searches a big
    /// tree's files on them). `None`: every toollet runs on its own thread.
    pub cores: Option<Arc<dyn Cores>>,
    /// The secrets this call's tool was granted, set per call by the runtime
    /// (theseus-dcy). `None`: it was granted none.
    pub secrets: Option<Arc<dyn Secrets>>,
    /// The operator approved this call: it ran at `approve`. Set per call by
    /// the runtime. Only then does `http.fetch` reach the private address its
    /// URL names (DD5).
    pub approved: bool,
    /// The operator's umask (theseus-wz2). The daemon runs under 077, so its
    /// own files are private; a file or directory a tool makes in the
    /// workspace gets this one's mode instead, as the operator's shell would
    /// make it. `None`: the process's umask applies as it is.
    pub umask: Option<u32>,
}

impl ToolCtx {
    pub fn for_tests(root: &Path) -> Self {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        Self {
            roots: vec![root.clone()],
            floor: Vec::new(),
            cwd: root,
            max_read_bytes: 256 * 1024,
            max_entries: 500,
            proc_timeout_secs: 60,
            proc_timeout_max_secs: 3600,
            cores: None,
            secrets: None,
            approved: false,
            umask: None,
        }
    }

    /// The value of the `[secrets]` entry `name`, if the broker granted it to
    /// this call's tool (theseus-dcy); else why not.
    pub fn secret(&self, name: &str) -> Result<zeroize::Zeroizing<String>, String> {
        match &self.secrets {
            Some(s) => s.secret(name),
            None => Err(format!("this tool was granted no secret, so not {name}")),
        }
    }

    /// Resolve a path argument: absolute stays, relative joins `cwd`; `..` and
    /// `.` are normalized lexically, then symlinks are resolved for the part
    /// that exists (so a link cannot smuggle a path out of the roots).
    pub fn resolve(&self, p: &str) -> PathBuf {
        paths::resolve(&self.cwd, p)
    }
}

pub trait Tool: Send + Sync {
    /// Canonical dotted name.
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn input_schema(&self) -> Value;
    fn class(&self) -> ToolClass;
    fn backend(&self) -> Backend {
        Backend::Inproc
    }
    fn retry(&self) -> Retry;
    /// Parse and validate `input`, and name what the call will touch. An
    /// error here is the model's mistake and goes back to it as an error result.
    fn plan(&self, input: &Value, ctx: &ToolCtx) -> Result<Plan, String>;
    /// Run in process (only for `Backend::Inproc`, only after the gate).
    fn run(&self, _input: &Value, _ctx: &ToolCtx) -> Result<ToolOutput, ToolFailure> {
        Err(ToolFailure::new("this tool runs as a job, not in process"))
    }
    /// `run`, plus an image for the model when the tool read one (only
    /// `fs.read` returns one). The runtime calls this.
    fn run_with_image(
        &self,
        input: &Value,
        ctx: &ToolCtx,
    ) -> Result<(ToolOutput, Option<ImageData>), ToolFailure> {
        self.run(input, ctx).map(|o| (o, None))
    }
    /// Run as a future on the daemon's runtime (only for `Backend::Async`,
    /// only after the gate): a tool that waits on the network, not on a core.
    fn run_async(&self, _input: &Value, _ctx: &ToolCtx) -> AsyncRun {
        Box::pin(std::future::ready(Err(ToolFailure::new(
            "this tool does not run async",
        ))))
    }
    /// The job to launch (only for `Backend::Job`).
    fn job(&self, _input: &Value, _ctx: &ToolCtx) -> Result<JobSpec, String> {
        Err("this tool runs in process, not as a job".into())
    }
    fn family(&self) -> &'static str {
        self.name().split('.').next().unwrap_or("")
    }
    /// How the model gets what the runtime cut from the middle of a result
    /// too long to show whole, `left_out` (theseus-46v). Nothing keeps a
    /// result whole, so the answer is always another call: a range where the
    /// tool takes one, else a narrower call.
    fn rest(&self, _left_out: &str) -> String {
        REST_NARROWER.into()
    }
}

/// `Tool::rest`'s default, and the answer for a tool the runtime does not know.
pub const REST_NARROWER: &str = "a narrower call returns them";

pub fn wire_name(name: &str) -> String {
    name.replace('.', "_")
}

/// Parse a tool's typed arguments; the serde error is the validation message.
pub fn parse<T: serde::de::DeserializeOwned>(input: &Value) -> Result<T, String> {
    serde_json::from_value(input.clone()).map_err(|e| format!("invalid input: {e}"))
}

#[derive(Default, Clone)]
pub struct Registry {
    tools: BTreeMap<String, Arc<dyn Tool>>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, t: Arc<dyn Tool>) {
        self.tools.insert(t.name().to_string(), t);
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn Tool>> {
        self.tools.get(name)
    }

    pub fn by_wire(&self, wire: &str) -> Option<&Arc<dyn Tool>> {
        self.tools.values().find(|t| wire_name(t.name()) == wire)
    }

    pub fn all(&self) -> impl Iterator<Item = &Arc<dyn Tool>> {
        self.tools.values()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Wire definitions sorted by wire name, deterministic byte for byte (the
    /// tool list sits at the front of the cached prefix). `eager` sets
    /// `eager_input_streaming` so large inputs stream as they are generated.
    pub fn definitions(&self, eager: bool) -> Vec<Value> {
        let mut v: Vec<(String, Value)> = self
            .tools
            .values()
            .map(|t| {
                let mut d = json!({
                    "name": wire_name(t.name()),
                    "description": t.description(),
                    "input_schema": t.input_schema(),
                });
                if eager {
                    d["eager_input_streaming"] = Value::Bool(true);
                }
                (wire_name(t.name()), d)
            })
            .collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v.into_iter().map(|(_, d)| d).collect()
    }
}

/// Every toollet Theseus ships, registered.
pub fn default_registry() -> Registry {
    let mut r = Registry::new();
    r.register(Arc::new(fs::Read));
    r.register(Arc::new(fs::WriteFile));
    r.register(Arc::new(fs::Edit));
    r.register(Arc::new(fs::Patch));
    r.register(Arc::new(fs::Glob));
    r.register(Arc::new(fs::Grep));
    r.register(Arc::new(fs::List));
    r.register(Arc::new(text::Diff));
    r.register(Arc::new(git::Diff));
    r.register(Arc::new(git::Log));
    r.register(Arc::new(proc::Run));
    r
}
