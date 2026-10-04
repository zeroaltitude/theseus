//! Secrets. The recommended source is 1Password, read through a service
//! account: a `[secrets]` entry is then an `op://` reference. An entry may
//! instead be `env:NAME`, the daemon's own environment variable, or
//! `file:PATH`, a file only its owner can read (theseus-n88g.1, Eddie's D6:
//! a container, CI, or anyone without a vault). Those resolve locally,
//! before any `op` call, and `theseusd check` and health name each one with
//! its source. Values live in zeroizing memory and are never logged, stored,
//! or written anywhere but the header they belong in.
//!
//! Serve first (FAST, §2; theseus-qa0). The daemon answers its socket before
//! any secret resolves: `resolve_into` fills a `SecretBoard` in the
//! background, and each consumer waits for its own secret there. Fail closed
//! still holds, one consumer at a time: a consumer never runs without its
//! secret, and a failure names the secret and why. A secret that failed is
//! fetched again after 5 s, then at doubling intervals up to a minute, since
//! the process no longer exits and waits to be restarted.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use futures_util::future::BoxFuture;
use theseus_protocol::{SecretFailed, SecretSource, SecretsStatus};
use tokio::io::AsyncWriteExt;
use tokio::sync::watch;
use zeroize::Zeroizing;

pub const TOKEN_ENV: &str = "OP_SERVICE_ACCOUNT_TOKEN";

/// The longest one `op` process may take before its references count as failed.
pub const OP_TIMEOUT: Duration = Duration::from_secs(10);
/// The first retry of a failed secret; each later one waits twice as long.
const RETRY_FIRST: Duration = Duration::from_secs(5);
const RETRY_MAX: Duration = Duration::from_secs(60);

/// A secret value. Debug and Display never print it.
#[derive(Clone)]
pub struct Secret(Zeroizing<String>);

impl Secret {
    pub fn new(s: String) -> Self {
        Self(Zeroizing::new(s))
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Secret(<{} bytes>)", self.0.len())
    }
}

/// `op://vault/item/field` or `op://vault/item/section/field`, optionally
/// followed by `#label` to select one `label: value` line out of a multi-line
/// note (many hand-written Secure Notes look like `api key value: …`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretRef {
    pub vault: String,
    pub item: String,
    pub path: String,
    pub line_label: Option<String>,
    raw: String,
    op_ref: String,
}

impl SecretRef {
    pub fn parse(s: &str) -> Result<Self> {
        let (op_ref, line_label) = match s.split_once('#') {
            Some((r, l)) if !l.trim().is_empty() => (r, Some(l.trim().to_string())),
            _ => (s, None),
        };
        let rest = op_ref
            .strip_prefix("op://")
            .with_context(|| format!("not an op:// reference: {s:?}"))?;
        let mut parts = rest.splitn(3, '/');
        let vault = parts.next().unwrap_or_default().to_string();
        let item = parts.next().unwrap_or_default().to_string();
        let path = parts.next().unwrap_or_default().to_string();
        if vault.is_empty() || item.is_empty() || path.is_empty() {
            bail!("op:// reference needs vault/item/field: {s:?}");
        }
        Ok(Self {
            vault,
            item,
            path,
            line_label,
            raw: s.to_string(),
            op_ref: op_ref.to_string(),
        })
    }
    /// The full reference as written, including any `#label`.
    pub fn as_str(&self) -> &str {
        &self.raw
    }
    /// What `op read` receives (no fragment).
    pub fn op_ref(&self) -> &str {
        &self.op_ref
    }

    /// Apply the `#label` selection, if any, to a fetched value.
    pub fn select(&self, value: String) -> Result<String> {
        let Some(label) = &self.line_label else {
            return Ok(value);
        };
        let want = label.to_ascii_lowercase();
        for line in value.lines() {
            if let Some((k, v)) = line.split_once(':') {
                if k.trim().to_ascii_lowercase() == want {
                    let v = v.trim();
                    if v.is_empty() {
                        bail!("{}: line {label:?} is empty", self.raw);
                    }
                    return Ok(v.to_string());
                }
            }
        }
        let labels: Vec<String> = value
            .lines()
            .filter_map(|l| l.split_once(':').map(|(k, _)| k.trim().to_string()))
            .collect();
        bail!(
            "{}: no line labelled {label:?} (labels present: {})",
            self.raw,
            labels.join(", ")
        )
    }
}

/// Where the values of `op://` references come from. The daemon's is
/// `OpReader`; tests use fakes that stall, hang, or fail.
pub trait Fetch: Send + Sync {
    /// One result per reference (no `#label`), in order: the raw value, or
    /// why it could not be read.
    fn fetch<'a>(&'a self, refs: &'a [String]) -> BoxFuture<'a, Fetched>;
}

pub struct Fetched {
    pub values: Vec<Result<Zeroizing<String>, String>>,
    /// How: `inject`, or `inject, then read` after a failed injection.
    pub method: String,
}

/// Reads secrets by shelling out to the `op` CLI under the service-account
/// token. 1Password publishes no first-party Rust SDK; the community FFI
/// wrappers are the later candidate for removing this dependency.
#[derive(Clone)]
pub struct OpReader {
    token: Secret,
    op_bin: PathBuf,
    /// The token file it was pointed at, expanded; read only when
    /// `OP_SERVICE_ACCOUNT_TOKEN` is unset.
    token_file: Option<PathBuf>,
    /// No vault at all (a container with no 1Password, theseus-n88g.1): why.
    /// Every `op://` reference fails with it; `env:` and `file:` entries
    /// never reach the reader.
    absent: Option<String>,
}

/// Where a `[secrets]` entry's value comes from (theseus-n88g.1): the vault,
/// the recommended source, or, outside it, the daemon's own environment
/// (`env:NAME`) or a file only its owner can read (`file:PATH`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Vault,
    Env,
    File,
}

impl Source {
    /// An entry's source, by its prefix. Anything else is the vault's, and
    /// `SecretRef::parse` judges it.
    pub fn of(reference: &str) -> Self {
        if reference.starts_with("env:") {
            Self::Env
        } else if reference.starts_with("file:") {
            Self::File
        } else {
            Self::Vault
        }
    }

    /// `vault`, `env`, or `file`: what health and `theseusd check` say.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Vault => "vault",
            Self::Env => "env",
            Self::File => "file",
        }
    }
}

/// Whether an entry's value comes from outside the vault (`env:` or `file:`).
pub fn is_local(reference: &str) -> bool {
    Source::of(reference) != Source::Vault
}

/// A local entry's form, as `Config::validate` checks it: `env:` names a
/// variable (letters, digits, and `_`, not starting with a digit), and
/// `file:` an absolute path, or one under `~` or a variable. A relative path
/// would be read from wherever the daemon was started.
pub fn check_local(reference: &str) -> Result<()> {
    if let Some(var) = reference.strip_prefix("env:") {
        let named = var.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            && var.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !named {
            bail!("{reference:?}: env: takes a variable's name (letters, digits, and _)");
        }
    } else if let Some(path) = reference.strip_prefix("file:") {
        if !path.starts_with(['/', '~', '$']) {
            bail!("{reference:?}: file: takes an absolute path, or one under ~ or a $VARIABLE");
        }
    }
    Ok(())
}

/// The value of a local entry (`is_local`), or why it has none; `None` for an
/// `op://` reference. A variable's value and a file's text are trimmed.
fn local_value(reference: &str) -> Option<Result<Secret, String>> {
    if let Some(var) = reference.strip_prefix("env:") {
        return Some(match std::env::var(var) {
            Ok(v) if !v.trim().is_empty() => Ok(Secret::new(v.trim().to_string())),
            _ => Err(format!("{reference}: the variable is unset or empty")),
        });
    }
    let path = reference.strip_prefix("file:")?;
    Some(
        read_private(&crate::config::expand(path))
            .map(Secret::new)
            .map_err(|e| format!("{reference}: {e:#}")),
    )
}

/// A file's trimmed text, refused when its group or others may read it: the
/// service-account token file's rule, and a `file:` entry's.
fn read_private(path: &std::path::Path) -> Result<String> {
    let meta =
        std::fs::metadata(path).with_context(|| format!("{} is not readable", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = meta.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            bail!(
                "{} has mode {:o}; it must not be group- or world-readable (chmod 600)",
                path.display(),
                mode
            );
        }
    }
    let _ = meta;
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("{} is not readable", path.display()))?
        .trim()
        .to_string();
    if text.is_empty() {
        bail!("{} is empty", path.display());
    }
    Ok(text)
}

/// Why an `op inject` gave no values.
enum InjectFailed {
    /// It did not finish within `OP_TIMEOUT`: reading each reference would
    /// wait on the same vault, so every reference fails with this.
    TimedOut,
    /// It exited with an error, which names an item but not a config entry.
    Error(String),
}

impl OpReader {
    /// Token from `OP_SERVICE_ACCOUNT_TOKEN`, or from `token_file` if given.
    /// Without either, or without `op`, there is no vault: a config in the
    /// vault cannot start, and a file config starts on `absent`.
    pub fn from_env(token_file: Option<&str>) -> Result<Self> {
        let token_file = token_file.map(crate::config::expand);
        let token = match std::env::var(TOKEN_ENV) {
            Ok(v) if !v.trim().is_empty() => v.trim().to_string(),
            _ => {
                let path = token_file.as_deref().with_context(|| {
                    format!("{TOKEN_ENV} is not set and no --op-token-file was given")
                })?;
                read_private(path).context("the service-account token file")?
            }
        };
        if token.is_empty() {
            bail!("empty 1Password service-account token");
        }
        let op_bin = which("op").context("the `op` CLI (1Password) is not on PATH")?;
        Ok(Self {
            token: Secret::new(token),
            op_bin,
            token_file,
            absent: None,
        })
    }

    /// A reader with no vault behind it (theseus-n88g.1): a file config
    /// whose secrets are `env:` or `file:` entries needs none, and any
    /// `op://` entry fails with `why`, one consumer at a time.
    pub fn absent(why: String) -> Self {
        Self {
            token: Secret::new(String::new()),
            op_bin: PathBuf::from("op"),
            token_file: None,
            absent: Some(why),
        }
    }

    /// The token file this reader was pointed at (`--op-token-file` or
    /// `THESEUS_OP_TOKEN_FILE`), whether or not the environment's token made
    /// reading it unnecessary.
    pub fn token_file(&self) -> Option<&std::path::Path> {
        self.token_file.as_deref()
    }

    /// Start `op`, registered as a child that tokio waits for, so the
    /// daemon's reaper leaves it to tokio (theseus-z4b). This is the only place
    /// the daemon starts a child that it waits for itself.
    fn start(cmd: &mut tokio::process::Command) -> std::io::Result<tokio::process::Child> {
        use theseus_kernel::children::{self, Kind};
        children::spawn(Kind::Owned, || cmd.spawn(), tokio::process::Child::id)
    }

    /// `op`, under the token and nothing else from our environment.
    fn op(&self) -> tokio::process::Command {
        let mut c = tokio::process::Command::new(&self.op_bin);
        c.env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", std::env::var("HOME").unwrap_or_default())
            .env(TOKEN_ENV, self.token.expose())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        c
    }

    /// One reference, `#label` applied (the config note itself is read this way).
    pub async fn read(&self, r: &SecretRef) -> Result<Secret> {
        let raw = self
            .read_raw(r.op_ref())
            .await
            .map_err(|e| anyhow::anyhow!("op read {} failed: {e}", r.as_str()))?;
        Ok(Secret::new(r.select(raw.to_string())?))
    }

    async fn read_raw(&self, op_ref: &str) -> Result<Zeroizing<String>, String> {
        if let Some(why) = &self.absent {
            return Err(format!("no 1Password access: {why}"));
        }
        let mut cmd = self.op();
        cmd.arg("read").arg("--no-newline").arg(op_ref);
        let child = Self::start(&mut cmd).map_err(|e| format!("spawning op: {e}"))?;
        let out = match tokio::time::timeout(OP_TIMEOUT, child.wait_with_output()).await {
            Err(_) => {
                return Err(format!(
                    "op read did not answer within {} s",
                    OP_TIMEOUT.as_secs()
                ))
            }
            Ok(Err(e)) => return Err(format!("running op read: {e}")),
            Ok(Ok(o)) => o,
        };
        if !out.status.success() {
            // op's stderr describes the failure; it does not echo values.
            return Err(op_error(&out.stderr));
        }
        let value = Zeroizing::new(
            String::from_utf8(out.stdout).map_err(|_| "op returned non-UTF-8".to_string())?,
        );
        if value.is_empty() {
            return Err("op returned an empty value".into());
        }
        Ok(value)
    }

    /// Every reference through one `op inject`: one process and one vault
    /// session, where concurrent `op read`s start one each (theseus-qa0
    /// measured 1 s either way, and a sixth of the CPU for the injection).
    /// Each reference gets its own slot between random boundary lines.
    async fn inject(&self, refs: &[String]) -> Result<Vec<Zeroizing<String>>, InjectFailed> {
        if let Some(why) = &self.absent {
            return Err(InjectFailed::Error(format!("no 1Password access: {why}")));
        }
        let boundary = format!("--theseus-{}-", uuid::Uuid::now_v7().simple());
        let mut template = String::new();
        for (i, r) in refs.iter().enumerate() {
            template.push_str(&format!("{boundary}{i}\n{{{{ {r} }}}}\n"));
        }
        template.push_str(&format!("{boundary}end\n"));
        let mut cmd = self.op();
        cmd.arg("inject").stdin(Stdio::piped());
        let mut child =
            Self::start(&mut cmd).map_err(|e| InjectFailed::Error(format!("spawning op: {e}")))?;
        let run = async {
            if let Some(mut stdin) = child.stdin.take() {
                // An `op` that exits before it reads the template (one that
                // refuses at once) closes the pipe first: its status and its
                // stderr say why, so the broken pipe is not the error
                // (theseus-f6f5).
                match stdin.write_all(template.as_bytes()).await {
                    Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {}
                    r => r?,
                }
            }
            child.wait_with_output().await
        };
        let out = match tokio::time::timeout(OP_TIMEOUT, run).await {
            Err(_) => return Err(InjectFailed::TimedOut),
            Ok(Err(e)) => return Err(InjectFailed::Error(format!("running op inject: {e}"))),
            Ok(Ok(o)) => o,
        };
        if !out.status.success() {
            return Err(InjectFailed::Error(op_error(&out.stderr)));
        }
        let text = Zeroizing::new(
            String::from_utf8(out.stdout)
                .map_err(|_| InjectFailed::Error("op returned non-UTF-8".into()))?,
        );
        split_injected(&text, &boundary, refs.len())
            .ok_or_else(|| InjectFailed::Error("op inject returned an unexpected shape".into()))
    }
}

impl Fetch for OpReader {
    /// One `op inject` for every reference. If it fails, one `op read` per
    /// reference, concurrently, so each bad one is named with its own error
    /// and the good ones still resolve; this costs about a second more, and
    /// only when something is already wrong.
    fn fetch<'a>(&'a self, refs: &'a [String]) -> BoxFuture<'a, Fetched> {
        Box::pin(async move {
            match self.inject(refs).await {
                Ok(values) => Fetched {
                    values: values.into_iter().map(Ok).collect(),
                    method: "inject".into(),
                },
                Err(InjectFailed::TimedOut) => Fetched {
                    values: refs
                        .iter()
                        .map(|_| {
                            Err(format!(
                                "op inject did not answer within {} s",
                                OP_TIMEOUT.as_secs()
                            ))
                        })
                        .collect(),
                    method: "inject".into(),
                },
                Err(InjectFailed::Error(why)) => {
                    tracing::info!(error = %why, "op inject failed; reading each reference to name what failed");
                    let reads = refs.iter().map(|r| self.read_raw(r));
                    Fetched {
                        values: futures_util::future::join_all(reads).await,
                        method: "inject, then read".into(),
                    }
                }
            }
        })
    }
}

/// The values between `op inject`'s boundary lines, in order, or `None` if
/// the output is not the template's shape.
fn split_injected(text: &str, boundary: &str, n: usize) -> Option<Vec<Zeroizing<String>>> {
    let mut rest = text.strip_prefix(&format!("{boundary}0\n"))?;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let next = if i + 1 < n {
            format!("\n{boundary}{}\n", i + 1)
        } else {
            format!("\n{boundary}end\n")
        };
        let at = rest.find(&next)?;
        out.push(Zeroizing::new(rest[..at].to_string()));
        rest = &rest[at + next.len()..];
    }
    Some(out)
}

/// op's error line without its `[ERROR] <date> <time>` prefix.
fn op_error(stderr: &[u8]) -> String {
    let s = String::from_utf8_lossy(stderr);
    let s = s.trim();
    match s.strip_prefix("[ERROR] ") {
        Some(rest) => rest.splitn(3, ' ').nth(2).unwrap_or(rest).to_string(),
        None => s.to_string(),
    }
}

/// Where one secret stands.
#[derive(Clone, Debug)]
pub enum SecretState {
    Resolving,
    Ready(Secret),
    /// Why it did not resolve; never a value.
    Failed(String),
}

/// What a bounded wait for one secret found.
#[derive(Debug)]
pub enum Waited {
    Ready(Secret),
    Failed(String),
    /// Still resolving when the wait ran out.
    Resolving,
    /// Not a configured secret (tests; config validation keeps it out of a daemon).
    Absent,
}

#[derive(Default)]
struct Progress {
    started: Option<Instant>,
    settled: Option<Instant>,
    method: Option<String>,
    rounds: u32,
    retry_at: Option<Instant>,
}

/// Each secret's state, published as it changes, so a consumer waits for
/// its own secret alone (FAST, §2). The daemon serves while this fills.
pub struct SecretBoard {
    tx: watch::Sender<BTreeMap<String, SecretState>>,
    /// Process start, for the timings health reports.
    origin: Instant,
    progress: Mutex<Progress>,
    /// The entries whose values come from outside the vault, by name, with
    /// their source (theseus-n88g.1). Empty for `new`'s board.
    outside: Vec<(String, Source)>,
}

impl SecretBoard {
    /// Every named secret resolving, each counted as the vault's.
    pub fn new(names: impl IntoIterator<Item = String>, origin: Instant) -> Arc<Self> {
        Self::with_outside(names, Vec::new(), origin)
    }

    /// The config's `[secrets]` (name → entry), every one resolving, with
    /// the ones from outside the vault noted for health (theseus-n88g.1).
    pub fn for_config(refs: &BTreeMap<String, String>, origin: Instant) -> Arc<Self> {
        let outside = refs
            .iter()
            .map(|(n, r)| (n.clone(), Source::of(r)))
            .filter(|(_, s)| *s != Source::Vault)
            .collect();
        Self::with_outside(refs.keys().cloned(), outside, origin)
    }

    fn with_outside(
        names: impl IntoIterator<Item = String>,
        outside: Vec<(String, Source)>,
        origin: Instant,
    ) -> Arc<Self> {
        let map = names
            .into_iter()
            .map(|n| (n, SecretState::Resolving))
            .collect();
        Arc::new(Self {
            tx: watch::Sender::new(map),
            origin,
            progress: Mutex::default(),
            outside,
        })
    }

    /// No secrets at all: nothing to wait for.
    pub fn empty() -> Arc<Self> {
        Self::new([], Instant::now())
    }

    /// The value, if it is ready now.
    pub fn get(&self, name: &str) -> Option<Secret> {
        match self.tx.borrow().get(name) {
            Some(SecretState::Ready(s)) => Some(s.clone()),
            _ => None,
        }
    }

    /// Every state, for a reader that must not hold it across an await
    /// (the scrubber).
    pub fn states(&self) -> watch::Ref<'_, BTreeMap<String, SecretState>> {
        self.tx.borrow()
    }

    /// A receiver that wakes on every change (a consumer that waits through
    /// failures and retries, such as the Discord binding).
    pub fn subscribe(&self) -> watch::Receiver<BTreeMap<String, SecretState>> {
        self.tx.subscribe()
    }

    /// Wait for one secret to settle (ready or failed).
    pub async fn settle(&self, name: &str) -> Waited {
        let mut rx = self.tx.subscribe();
        let settled = rx
            .wait_for(|m| !matches!(m.get(name), Some(SecretState::Resolving)))
            .await;
        match settled {
            Ok(m) => match m.get(name) {
                Some(SecretState::Ready(s)) => Waited::Ready(s.clone()),
                Some(SecretState::Failed(e)) => Waited::Failed(e.clone()),
                Some(SecretState::Resolving) => Waited::Resolving,
                None => Waited::Absent,
            },
            Err(_) => Waited::Resolving,
        }
    }

    /// `settle`, for at most `max`.
    pub async fn wait(&self, name: &str, max: Duration) -> Waited {
        tokio::time::timeout(max, self.settle(name))
            .await
            .unwrap_or(Waited::Resolving)
    }

    /// Wait until no secret is resolving: the first round has settled.
    pub async fn settle_all(&self) {
        let mut rx = self.tx.subscribe();
        let _ = rx
            .wait_for(|m| !m.values().any(|s| matches!(s, SecretState::Resolving)))
            .await;
    }

    /// `settle_all`, for at most `max`. `Err` names those still resolving.
    pub async fn wait_settled(&self, max: Duration) -> Result<(), Vec<String>> {
        match tokio::time::timeout(max, self.settle_all()).await {
            Ok(()) => Ok(()),
            Err(_) => Err(self.names_in(|s| matches!(s, SecretState::Resolving))),
        }
    }

    fn names_in(&self, pick: impl Fn(&SecretState) -> bool) -> Vec<String> {
        self.tx
            .borrow()
            .iter()
            .filter(|(_, s)| pick(s))
            .map(|(n, _)| n.clone())
            .collect()
    }

    /// Whether the first round has settled: no secret is resolving.
    pub fn is_settled(&self) -> bool {
        !self
            .tx
            .borrow()
            .values()
            .any(|s| matches!(s, SecretState::Resolving))
    }

    /// The names whose values are ready.
    pub fn ready_names(&self) -> Vec<String> {
        self.names_in(|s| matches!(s, SecretState::Ready(_)))
    }

    /// When resolution began, if it has.
    pub fn started_at(&self) -> Option<Instant> {
        self.progress.lock().unwrap().started
    }

    fn begin_round(&self) {
        let mut p = self.progress.lock().unwrap();
        p.started.get_or_insert_with(Instant::now);
        p.retry_at = None;
    }

    /// One round's results; a secret the round did not fetch keeps its state.
    pub fn publish(&self, results: BTreeMap<String, Result<Secret, String>>, method: &str) {
        self.tx.send_modify(|m| {
            for (name, r) in results {
                m.insert(
                    name,
                    match r {
                        Ok(s) => SecretState::Ready(s),
                        Err(e) => SecretState::Failed(e),
                    },
                );
            }
        });
        let mut p = self.progress.lock().unwrap();
        p.rounds += 1;
        p.method = Some(method.to_string());
        let unsettled = self
            .tx
            .borrow()
            .values()
            .any(|s| matches!(s, SecretState::Resolving));
        if !unsettled {
            p.settled.get_or_insert_with(Instant::now);
        }
    }

    fn retry_at(&self, at: Instant) {
        self.progress.lock().unwrap().retry_at = Some(at);
    }

    /// For health: `resolving` until every secret has settled, then `ready`,
    /// or `failed` naming each one that did not resolve and why.
    pub fn status(&self) -> SecretsStatus {
        let (mut ready, mut resolving, mut failed) = (vec![], vec![], vec![]);
        for (name, s) in self.tx.borrow().iter() {
            match s {
                SecretState::Ready(_) => ready.push(name.clone()),
                SecretState::Resolving => resolving.push(name.clone()),
                SecretState::Failed(e) => failed.push(SecretFailed {
                    name: name.clone(),
                    error: e.clone(),
                }),
            }
        }
        let p = self.progress.lock().unwrap();
        let ms = |t: Instant| t.saturating_duration_since(self.origin).as_millis() as u64;
        let state = if !resolving.is_empty() {
            "resolving"
        } else if !failed.is_empty() {
            "failed"
        } else {
            "ready"
        };
        SecretsStatus {
            state: state.into(),
            ready,
            resolving,
            failed,
            method: p.method.clone(),
            rounds: p.rounds,
            started_ms: p.started.map(ms),
            settled_ms: p.settled.map(ms),
            retry_in_ms: p
                .retry_at
                .map(|t| t.saturating_duration_since(Instant::now()).as_millis() as u64),
            outside_vault: self
                .outside
                .iter()
                .map(|(name, s)| SecretSource {
                    name: name.clone(),
                    kind: s.as_str().into(),
                })
                .collect(),
        }
    }
}

impl fmt::Debug for SecretBoard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecretBoard")
            .field("status", &self.status().summary())
            .finish()
    }
}

/// Fetch `names` from `refs` (config name → reference) once: each distinct
/// reference is fetched once, and each name takes its `#label` line. An
/// `env:` or `file:` entry is read here first, with no `op` call. The method
/// is the vault's (`inject`, …), `local` when every entry was read here, or
/// the vault's with ` + local` when some were.
async fn round(
    refs: &BTreeMap<String, String>,
    names: &[String],
    fetch: &dyn Fetch,
) -> (BTreeMap<String, Result<Secret, String>>, String) {
    let mut out = BTreeMap::new();
    let mut parsed = Vec::new();
    let mut distinct: Vec<String> = Vec::new();
    let mut local = false;
    for name in names {
        if let Some(v) = refs.get(name).and_then(|raw| local_value(raw)) {
            out.insert(name.clone(), v);
            local = true;
            continue;
        }
        match refs.get(name).map(|raw| SecretRef::parse(raw)) {
            Some(Ok(r)) => {
                let i = match distinct.iter().position(|d| d == r.op_ref()) {
                    Some(i) => i,
                    None => {
                        distinct.push(r.op_ref().to_string());
                        distinct.len() - 1
                    }
                };
                parsed.push((name.clone(), r, i));
            }
            Some(Err(e)) => {
                out.insert(name.clone(), Err(format!("{e:#}")));
            }
            None => {
                out.insert(name.clone(), Err("no such [secrets] entry".into()));
            }
        }
    }
    if distinct.is_empty() {
        return (out, if local { "local" } else { "" }.to_string());
    }
    let mut fetched = fetch.fetch(&distinct).await;
    if local {
        fetched.method.push_str(" + local");
    }
    for (name, r, i) in parsed {
        let v = match fetched.values.get(i) {
            Some(Ok(raw)) => r
                .select(raw.to_string())
                .map(Secret::new)
                .map_err(|e| format!("{e:#}")),
            Some(Err(e)) => Err(format!("{}: {e}", r.as_str())),
            None => Err(format!("{}: no value returned", r.as_str())),
        };
        out.insert(name, v);
    }
    (out, fetched.method)
}

/// Resolve every entry of `refs` (config name → reference) into `board`:
/// one round for all of them, then, for any that failed, another after 5 s,
/// 10 s, 20 s, … at most a minute apart, until each resolves. Runs in the
/// background; nothing on the path to serving waits for it.
pub async fn resolve_into(
    board: Arc<SecretBoard>,
    refs: BTreeMap<String, String>,
    fetch: Arc<dyn Fetch>,
) {
    resolve_with_retry(board, refs, fetch, RETRY_FIRST).await;
}

#[expect(clippy::cognitive_complexity, reason = "shape budget: split it")]
async fn resolve_with_retry(
    board: Arc<SecretBoard>,
    refs: BTreeMap<String, String>,
    fetch: Arc<dyn Fetch>,
    first_retry: Duration,
) {
    let mut pending: Vec<String> = refs.keys().cloned().collect();
    let mut delay = first_retry;
    while !pending.is_empty() {
        board.begin_round();
        let t0 = Instant::now();
        let (results, method) = round(&refs, &pending, fetch.as_ref()).await;
        let failed: Vec<String> = results
            .iter()
            .filter(|(_, r)| r.is_err())
            .map(|(n, _)| n.clone())
            .collect();
        for (name, r) in &results {
            match r {
                Ok(v) => tracing::info!(secret = %name, bytes = v.len(), "resolved"),
                Err(e) => tracing::warn!(secret = %name, error = %e, "did not resolve"),
            }
        }
        tracing::info!(
            ms = t0.elapsed().as_millis() as u64,
            method = %method,
            fetched = results.len(),
            failed = failed.len(),
            "secrets round"
        );
        board.publish(results, &method);
        pending = failed;
        if pending.is_empty() {
            break;
        }
        board.retry_at(Instant::now() + delay);
        tracing::warn!(secrets = ?pending, retry_in_s = delay.as_secs(), "secrets failed; their consumers wait, and they are fetched again");
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(RETRY_MAX);
    }
}

fn which(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(bin))
        .find(|p| p.is_file())
}

/// Fetchers for tests: a scripted vault whose answers can wait on a gate.
#[cfg(test)]
pub mod fake {
    use super::*;

    /// Answers each reference from `values` (a missing one fails), after
    /// `gate` opens if there is one; counts its fetches.
    pub struct FakeVault {
        pub values: BTreeMap<String, String>,
        pub gate: Option<tokio::sync::watch::Receiver<bool>>,
        pub fetches: std::sync::atomic::AtomicU32,
    }

    impl FakeVault {
        pub fn new(values: &[(&str, &str)]) -> Self {
            Self {
                values: values
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
                gate: None,
                fetches: Default::default(),
            }
        }
        /// Answers only once the returned sender sends `true`; never, if it never does.
        pub fn gated(values: &[(&str, &str)]) -> (Self, tokio::sync::watch::Sender<bool>) {
            let (tx, rx) = tokio::sync::watch::channel(false);
            let mut v = Self::new(values);
            v.gate = Some(rx);
            (v, tx)
        }
    }

    impl Fetch for FakeVault {
        fn fetch<'a>(&'a self, refs: &'a [String]) -> BoxFuture<'a, Fetched> {
            Box::pin(async move {
                self.fetches
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if let Some(mut g) = self.gate.clone() {
                    let _ = g.wait_for(|open| *open).await;
                }
                Fetched {
                    values: refs
                        .iter()
                        .map(|r| {
                            self.values
                                .get(r)
                                .map(|v| Zeroizing::new(v.clone()))
                                .ok_or_else(|| format!("no item at {r}"))
                        })
                        .collect(),
                    method: "fake".into(),
                }
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeVault;
    use super::*;

    #[test]
    fn parses_refs_with_spaces() {
        let r = SecretRef::parse("op://Harbor Team/model api key/notesPlain").unwrap();
        assert_eq!(r.vault, "Harbor Team");
        assert_eq!(r.item, "model api key");
        assert_eq!(r.path, "notesPlain");
    }

    #[test]
    fn line_label_selects_one_line_of_a_note() {
        let r = SecretRef::parse("op://V/second model key/notesPlain#api key value").unwrap();
        assert_eq!(r.op_ref(), "op://V/second model key/notesPlain");
        let note = "name: zai\napi key id: abc\nApi Key Value:  id.secret \n".to_string();
        assert_eq!(r.select(note).unwrap(), "id.secret");
        let missing = SecretRef::parse("op://V/i/notesPlain#nope").unwrap();
        let e = missing.select("a: 1\nb: 2".into()).unwrap_err().to_string();
        assert!(e.contains("labels present: a, b"));
        let plain = SecretRef::parse("op://V/i/notesPlain").unwrap();
        assert_eq!(plain.select("raw".into()).unwrap(), "raw");
    }

    #[tokio::test]
    async fn env_and_file_entries_resolve_without_the_vault() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("key");
        std::fs::write(&file, "tv-file-7f3a9c\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        let open = dir.path().join("open");
        std::fs::write(&open, "x").unwrap();
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o644)).unwrap();
        // A variable every test process has, so the test changes no one's
        // environment.
        let path_var = std::env::var("PATH").unwrap();
        let refs: BTreeMap<String, String> = [
            ("a", "env:PATH".to_string()),
            ("b", format!("file:{}", file.display())),
            ("c", format!("file:{}", open.display())),
            ("d", "env:THESEUS_N88G_UNSET".to_string()),
            ("e", "op://v/i/f".to_string()),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        let names: Vec<String> = refs.keys().cloned().collect();
        let reader = OpReader::absent("a container with no vault".into());
        let (out, method) = round(&refs, &names, &reader).await;
        assert_eq!(out["a"].as_ref().unwrap().expose(), path_var.trim());
        assert_eq!(out["b"].as_ref().unwrap().expose(), "tv-file-7f3a9c");
        let c = out["c"].as_ref().unwrap_err();
        assert!(c.contains("has mode 644") && !c.contains("token"), "{c}");
        assert!(out["d"].as_ref().unwrap_err().contains("unset or empty"));
        assert!(out["e"]
            .as_ref()
            .unwrap_err()
            .contains("no 1Password access"));
        assert!(method.ends_with(" + local"), "{method}");
        // Only local entries: the method says so, and no fetch is made.
        let local: BTreeMap<String, String> = refs.clone().into_iter().take(2).collect();
        let vault = fake::FakeVault::new(&[]);
        let names: Vec<String> = local.keys().cloned().collect();
        let (out, method) = round(&local, &names, &vault).await;
        assert_eq!((out.len(), method.as_str()), (2, "local"));
        assert_eq!(vault.fetches.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    /// theseus-n88g.1: health names each secret from outside the vault with
    /// its source, never its value, and `new`'s board names none.
    #[tokio::test]
    async fn the_board_names_the_secrets_from_outside_the_vault() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("db");
        std::fs::write(&file, "tv-db-7f3a9c").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        let refs: BTreeMap<String, String> = [
            ("anthropic_api_key", "env:PATH".to_string()),
            ("db", format!("file:{}", file.display())),
            ("zai_api_key", "op://Test/zai/credential".to_string()),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        let board = SecretBoard::for_config(&refs, Instant::now());
        let outside: Vec<(String, String)> = board
            .status()
            .outside_vault
            .into_iter()
            .map(|s| (s.name, s.kind))
            .collect();
        assert_eq!(
            outside,
            [
                ("anthropic_api_key".to_string(), "env".to_string()),
                ("db".to_string(), "file".to_string())
            ]
        );
        let vault = fake::FakeVault::new(&[("op://Test/zai/credential", "tv-zai-7f3a9c")]);
        // Every entry resolves, so this is one round.
        resolve_into(board.clone(), refs, Arc::new(vault)).await;
        let st = board.status();
        assert_eq!((st.state.as_str(), st.ready.len()), ("ready", 3), "{st:?}");
        assert_eq!(st.method.as_deref(), Some("fake + local"));
        let json = serde_json::to_string(&st).unwrap();
        for value in [
            "tv-db-7f3a9c",
            "tv-zai-7f3a9c",
            std::env::var("PATH").unwrap().trim(),
        ] {
            assert!(!json.contains(value), "a value in health: {json}");
        }
        let plain = SecretBoard::new(["k".to_string()], Instant::now());
        assert!(plain.status().outside_vault.is_empty());
    }

    /// theseus-n88g.1: an `env:` entry names a variable, and a `file:` entry
    /// a path that does not depend on where the daemon was started.
    #[test]
    fn a_local_entry_is_checked_for_its_form() {
        for good in [
            "env:ANTHROPIC_API_KEY",
            "env:_k1",
            "file:/run/secrets/key",
            "file:~/.config/theseus/key",
            "file:$CREDENTIALS_DIRECTORY/key",
            "op://v/i/f",
        ] {
            check_local(good).unwrap_or_else(|e| panic!("{good}: {e:#}"));
        }
        for bad in [
            "env:",
            "env:1KEY",
            "env:A-B",
            "env:A B",
            "file:",
            "file:key.txt",
        ] {
            assert!(check_local(bad).is_err(), "{bad}");
        }
        assert_eq!(Source::of("env:X"), Source::Env);
        assert_eq!(Source::of("file:/x"), Source::File);
        assert_eq!(Source::of("op://v/i/f"), Source::Vault);
        assert!(is_local("file:/x") && !is_local("op://v/i/f"));
    }

    #[test]
    fn rejects_short_refs() {
        assert!(SecretRef::parse("op://vault/item").is_err());
        assert!(SecretRef::parse("vault/item/field").is_err());
    }

    #[test]
    fn secret_debug_hides_value() {
        let s = Secret::new("sk-ant-very-secret".into());
        assert!(!format!("{s:?}").contains("secret"));
    }

    /// `op inject`'s output splits back into one value per reference, a
    /// multi-line note included, and anything else is refused.
    #[test]
    fn injected_output_splits_at_its_boundaries() {
        let b = "--theseus-x-";
        let text = format!("{b}0\nfirst\n{b}1\nline one\nline two\n{b}2\n\n{b}end\n");
        let v = split_injected(&text, b, 3).unwrap();
        let v: Vec<&str> = v.iter().map(|s| s.as_str()).collect();
        assert_eq!(v, ["first", "line one\nline two", ""]);
        assert!(split_injected("garbage", b, 1).is_none());
        assert!(split_injected(&format!("{b}0\nno end"), b, 1).is_none());
    }

    #[test]
    fn op_errors_lose_their_timestamp_prefix() {
        let e = op_error(b"[ERROR] 2026/09/29 08:58:19 could not find item x\n");
        assert_eq!(e, "could not find item x");
        assert_eq!(op_error(b"plain"), "plain");
    }

    /// An `op` that refuses at once, before it reads the template, is
    /// named by its own words, not by the pipe it closed (theseus-f6f5). The
    /// template here is past a pipe's 64 KiB, so its write always meets the
    /// closed pipe, however the two processes are scheduled.
    #[tokio::test]
    async fn an_op_that_refuses_before_reading_its_template_is_named_by_its_stderr() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let op_bin = dir.path().join("op");
        std::fs::write(
            &op_bin,
            "#!/bin/sh\necho '[ERROR] 2026/10/03 12:00:00 the vault is sealed' >&2\nexit 1\n",
        )
        .unwrap();
        std::fs::set_permissions(&op_bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let reader = OpReader {
            token: Secret::new("test-not-a-token".into()),
            op_bin,
            token_file: None,
            absent: None,
        };
        let refs: Vec<String> = (0..2000)
            .map(|i| format!("op://Harbor/item-{i:04}/notesPlain"))
            .collect();
        match reader.inject(&refs).await {
            Err(InjectFailed::Error(why)) => assert_eq!(why, "the vault is sealed"),
            Err(InjectFailed::TimedOut) => panic!("timed out"),
            Ok(_) => panic!("op refused, yet values came back"),
        }
    }

    fn refs(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    /// The board starts resolving, settles each secret as its round ends,
    /// names a failure, and fetches a reference two names share only once.
    #[tokio::test]
    async fn the_board_resolves_in_the_background_and_names_a_failure() {
        let refs = refs(&[
            ("key", "op://V/key/notesPlain"),
            ("id", "op://V/aws/notesPlain#id"),
            ("secret", "op://V/aws/notesPlain#secret"),
            ("gone", "op://V/nothing/notesPlain"),
        ]);
        let (vault, open) = FakeVault::gated(&[
            ("op://V/key/notesPlain", "k-123456789"),
            ("op://V/aws/notesPlain", "id: AKIA0000\nsecret: s3cr3tvalue"),
        ]);
        let vault = Arc::new(vault);
        let board = SecretBoard::new(refs.keys().cloned(), Instant::now());
        assert_eq!(board.status().state, "resolving");
        tokio::spawn(resolve_into(board.clone(), refs, vault.clone()));
        assert!(matches!(
            board.wait("key", Duration::from_millis(50)).await,
            Waited::Resolving
        ));
        open.send(true).unwrap();
        match board.wait("key", Duration::from_secs(5)).await {
            Waited::Ready(s) => assert_eq!(s.expose(), "k-123456789"),
            w => panic!("{w:?}"),
        }
        board.wait_settled(Duration::from_secs(5)).await.unwrap();
        assert_eq!(board.get("secret").unwrap().expose(), "s3cr3tvalue");
        let st = board.status();
        assert_eq!(st.state, "failed");
        assert_eq!(st.summary(), "failed gone");
        assert!(
            st.failed[0].error.contains("no item at op://V/nothing"),
            "{st:?}"
        );
        assert_eq!(st.ready, ["id", "key", "secret"]);
        assert!(st.retry_in_ms.is_some(), "a failure is retried");
        assert_eq!(
            vault.fetches.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "one fetch for the round"
        );
        assert!(matches!(
            board.wait("gone", Duration::from_secs(1)).await,
            Waited::Failed(_)
        ));
        assert!(matches!(
            board.wait("unknown", Duration::from_secs(1)).await,
            Waited::Absent
        ));
    }

    /// A secret that failed is fetched again, and resolves when the vault
    /// has it.
    #[tokio::test]
    async fn a_failed_secret_is_fetched_again() {
        struct Flaky(std::sync::atomic::AtomicU32);
        impl Fetch for Flaky {
            fn fetch<'a>(&'a self, refs: &'a [String]) -> BoxFuture<'a, Fetched> {
                let n = self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Box::pin(async move {
                    Fetched {
                        values: refs
                            .iter()
                            .map(|_| {
                                if n == 0 {
                                    Err("network down".to_string())
                                } else {
                                    Ok(Zeroizing::new("v-123456789".to_string()))
                                }
                            })
                            .collect(),
                        method: "fake".into(),
                    }
                })
            }
        }
        let board = SecretBoard::new(["k".to_string()], Instant::now());
        let flaky = Arc::new(Flaky(Default::default()));
        tokio::spawn(resolve_with_retry(
            board.clone(),
            refs(&[("k", "op://V/k/f")]),
            flaky.clone(),
            Duration::from_millis(20),
        ));
        assert!(matches!(
            board.wait("k", Duration::from_secs(1)).await,
            Waited::Failed(_)
        ));
        let mut rx = board.subscribe();
        rx.wait_for(|m| matches!(m.get("k"), Some(SecretState::Ready(_))))
            .await
            .unwrap();
        assert_eq!(board.status().state, "ready");
        assert_eq!(board.status().rounds, 2);
    }
}
