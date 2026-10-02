//! The Theseus wire protocol.
//!
//! JSON-RPC 2.0, one message per line, UTF-8, on any byte stream (stdio, a Unix
//! domain socket, or an in-process channel). This crate is types only: every
//! client, including the in-binary CLI, links this and nothing else from the
//! core, so no client has a privileged path into the kernel.
//!
//! Requests change state and get exactly one response. Notifications report
//! state and are also ledger rows on the server side.

mod events;
mod gate;
mod push;
#[cfg(test)]
mod ts;

pub use events::*;
pub use gate::*;
pub use push::*;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const VERSION: &str = "0.1";
pub const JSONRPC: &str = "2.0";

/// The largest image an attachment or `fs.read` passes to a model
/// (theseus-9g2): 5 MiB of raw bytes, which every route takes (Anthropic's
/// direct API allows 10 MB of base64; Bedrock and Vertex allow 5 MB).
pub const MAX_IMAGE_BYTES: u64 = 5 * 1024 * 1024;

/// The `tool` of a budget question (theseus-0sg): a session reached its spend
/// limit, and a `confirm.requested` with this tool asks the operator whether
/// its spend may go back to $0. Answered with `action.confirm` like any other.
pub const BUDGET_TOOL: &str = "budget.reset";

/// The `tool` of a provider call's action. It is authorized in the frame after
/// its plan and never asks the operator.
pub const PROVIDER_TOOL: &str = "provider.messages";

/// A module of wire names, from one table: each constant with its docs, and
/// `ALL`, built from the same table, so no constant is left out of it. The
/// reader rule's registry test (theseus-wjy, theseus-core's `tests_registry`)
/// enumerates `ALL`: each method needs its dispatch arm, and each
/// notification its `Event` and a sender.
macro_rules! names {
    ($($(#[$doc:meta])* $name:ident = $value:literal,)*) => {
        $($(#[$doc])* pub const $name: &str = $value;)*

        /// Every name in this module, from the same table as the constants.
        pub const ALL: &[&str] = &[$($name,)*];
    };
}

/// Method names. Requests (client → server). `ALL` lets a server say which
/// only read (theseus-2fo).
pub mod method {
    names! {
        HEALTH = "health",
        SESSION_OPEN = "session.open",
        SESSION_LIST = "session.list",
        TURN_SUBMIT = "turn.submit",
        LEDGER_TAIL = "ledger.tail",
        PROFILE_LIST = "profile.list",
        PROFILE_USE = "profile.use",
        EXECUTION_LIST = "execution.list",
        EXECUTION_CANCEL = "execution.cancel",
        /// `/stop` (W1): halt a conversation's work, and keep the conversation.
        EXECUTION_STOP = "execution.stop",
        ACTION_LIST = "action.list",
        ACTION_CONFIRM = "action.confirm",
        /// Every question waiting for the operator, across sessions.
        CONFIRM_LIST = "confirm.list",
        SESSION_HISTORY = "session.history",
        SESSION_WATCH = "session.watch",
        SESSION_UNWATCH = "session.unwatch",
        SESSION_RECOMPILE = "session.recompile",
        CATALOG_LIST = "catalog.list",
        COMPILATION_LIST = "compilation.list",
        NODE_LIST = "node.list",
        TOOL_LIST = "tool.list",
        SHUTDOWN = "shutdown",
        /// The narrative (`narrative = true`): the recent tail, then every new
        /// line as a `narrative.line` notification until `narrative.unwatch`.
        NARRATIVE_WATCH = "narrative.watch",
        NARRATIVE_UNWATCH = "narrative.unwatch",
        /// "Should have asked" (theseus-sgh): the tool asks first from now on.
        /// Stored, never in the config; it can only make a tool stricter.
        POLICY_TIGHTEN = "policy.tighten",
        /// Undo a tightening: the tool goes back to what the config says. It
        /// loosens, so it takes the same trusted answer as an approval.
        POLICY_UNTIGHTEN = "policy.untighten",
        /// Trust a session again (theseus-9bp): it no longer holds external
        /// text, so its calls that act go back to their postures. It loosens, so
        /// it takes the same trusted answer as an approval.
        POLICY_TRUST = "policy.trust",
        /// Tasks (DD7): the child sessions conversations started, with state and
        /// spend.
        TASK_LIST = "task.list",
        /// Stop a task and its jobs; the place hears it once.
        TASK_CANCEL = "task.cancel",
        /// Wakes (DD8): the turns conversations asked for at a time, with
        /// `wake.at`, that have not run yet.
        WAKE_LIST = "wake.list",
        /// Cancel a pending wake: nothing fires.
        WAKE_CANCEL = "wake.cancel",
    }
}

/// Notification names (server → client). Each has its `Event` variant, which
/// senders build and clients match on.
pub mod notify {
    names! {
        TURN_STARTED = "turn.started",
        LOOP_STARTED = "loop.started",
        MODEL_DELTA = "model.delta",
        TOOL_PROPOSED = "tool.proposed",
        LOOP_ENDED = "loop.ended",
        TURN_ENDED = "turn.ended",
        PROFILE_CHANGED = "profile.changed",
        /// Thinking summaries / progress updates as they stream.
        MODEL_THINKING = "model.thinking",
        /// The context for a loop was compiled (append or recompile, sizes, digest).
        CONTEXT_COMPILED = "context.compiled",
        /// A tool call started (after the gate) and ended (with its result).
        TOOL_STARTED = "tool.started",
        TOOL_ENDED = "tool.ended",
        /// A tool call needs the operator's confirmation; the turn has parked.
        CONFIRM_REQUESTED = "confirm.requested",
        CONFIRM_RESOLVED = "confirm.resolved",
        /// A node was written to a watched session (history stays live).
        NODE_WRITTEN = "node.written",
        /// A turn failed after it was admitted (provider error, store error). The
        /// requester also gets the error response; watchers only get this.
        TURN_FAILED = "turn.failed",
        /// A call ran under a `notify` posture (`[policy].enforcement`, or a
        /// `[policy.tools]` / `[policy.mcp]` line), and the operator is told.
        POLICY_NOTIFIED = "policy.notified",
        /// A tool was tightened, or its tightening undone (theseus-sgh). These go
        /// to every connection watching a session, once each, since a tightening
        /// holds for every session. The params are a `TightenResult`.
        POLICY_TIGHTENED = "policy.tightened",
        POLICY_UNTIGHTENED = "policy.untightened",
        /// The operator trusted a session again (theseus-9bp), to the session's
        /// watchers. The params are a `TrustResult`.
        SESSION_TRUSTED = "session.trusted",
        /// A Theseus job's process tried to answer an approval, reset the spend,
        /// or undo a tightening, and was refused (theseus-6qy): a security event,
        /// to every connection. The params are the `approval.refused` ledger
        /// row's, with `act` and `session_id`; `asker` names the process and its
        /// job.
        APPROVAL_REFUSED = "approval.refused",
        /// One line of the narrative, to every `narrative.watch` subscriber.
        /// Unlike the others it is not a ledger row: the narrative is never stored.
        NARRATIVE_LINE = "narrative.line",
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(untagged)]
pub enum Id {
    Num(u64),
    Str(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Request {
    pub jsonrpc: String,
    pub id: Id,
    pub method: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    #[cfg_attr(test, ts(type = "unknown"))]
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Notification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    #[cfg_attr(test, ts(type = "unknown"))]
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    #[cfg_attr(test, ts(type = "unknown"))]
    pub data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Response {
    pub jsonrpc: String,
    pub id: Id,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(type = "unknown"))]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub error: Option<RpcError>,
}

/// Any line on the wire.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(untagged)]
pub enum Message {
    Request(Request),
    Response(Response),
    Notification(Notification),
}

pub mod error_code {
    pub const PARSE: i64 = -32700;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL: i64 = -32603;
    // Theseus-specific codes. -32001 was `BLOCKED`, never sent.
    pub const NOT_FOUND: i64 = -32002;
    pub const PROVIDER: i64 = -32003;
    /// The config turns this feature off (`narrative.watch` without `narrative = true`).
    pub const DISABLED: i64 = -32004;
    /// An answer to a waiting call that does not count (theseus-sgh, spec
    /// §3.9 "Approval"): not from a trusted user, or not through a trusted
    /// channel. The message says why; the call keeps waiting.
    pub const REFUSED: i64 = -32005;
    /// A method that acts, sent while the daemon serves from its copy of the
    /// vault's config note and the vault has not confirmed it (theseus-2fo).
    /// The message says why; `data.class` is `config_unconfirmed`.
    pub const CONFIG_UNCONFIRMED: i64 = -32006;
}

impl Request {
    pub fn new(id: Id, method: &str, params: impl Serialize) -> Self {
        Self {
            jsonrpc: JSONRPC.into(),
            id,
            method: method.into(),
            params: serde_json::to_value(params).unwrap_or(Value::Null),
        }
    }
}

impl Notification {
    pub fn new(method: &str, params: impl Serialize) -> Self {
        Self {
            jsonrpc: JSONRPC.into(),
            method: method.into(),
            params: serde_json::to_value(params).unwrap_or(Value::Null),
        }
    }
}

impl Response {
    pub fn ok(id: Id, result: impl Serialize) -> Self {
        Self {
            jsonrpc: JSONRPC.into(),
            id,
            result: Some(serde_json::to_value(result).unwrap_or(Value::Null)),
            error: None,
        }
    }
    pub fn err(id: Id, code: i64, message: impl Into<String>) -> Self {
        Self::err_with(id, code, message, Value::Null)
    }
    pub fn err_with(id: Id, code: i64, message: impl Into<String>, data: Value) -> Self {
        Self {
            jsonrpc: JSONRPC.into(),
            id,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
                data,
            }),
        }
    }
}

// ---------------------------------------------------------------- payloads

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct HealthResult {
    pub name: String,
    pub version: String,
    pub protocol: String,
    pub uptime_secs: u64,
    pub sessions: u64,
    pub turns: u64,
    pub model: String,
    /// The live profile name.
    #[serde(default)]
    pub profile: String,
    /// Default provider name and every configured provider.
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub providers: Vec<String>,
    /// The secrets whose values are ready (names only).
    pub secrets_resolved: Vec<String>,
    /// Where every secret stands: the daemon serves before they resolve
    /// (theseus-qa0), and each consumer waits for its own.
    #[serde(default)]
    pub secrets: SecretsStatus,
    /// Where the config came from, and whether the vault has confirmed the
    /// copy this start served from (theseus-2fo).
    #[serde(default)]
    pub config: ConfigStatus,
    /// The last start's phases, timed from process start: those on the path
    /// to answering the socket, then those after it.
    #[serde(default)]
    pub startup: Vec<StartupPhase>,
    /// Tokens across every session, summed from session records.
    pub usage_total: Usage,
    pub provider_errors: u64,
    pub ledger_rows: u64,
    #[serde(default)]
    pub telemetry: TelemetryStatus,
    /// The durable kernel (M2): executions, actions, admission.
    #[serde(default)]
    pub kernel: KernelStatus,
    /// The daemon's own children: its job wrappers, the orphans it adopted,
    /// and what waits to be reaped (theseus-z4b).
    #[serde(default)]
    pub children: ChildrenStatus,
    /// The secret broker's grants, each with its uses (theseus-dcy). Names
    /// only, never a value.
    #[serde(default)]
    pub broker: Vec<GrantStatus>,
    /// Dollars across every session, from the model catalog.
    #[serde(default)]
    pub cost_usd_total: f64,
    #[serde(default)]
    pub catalog_version: String,
    /// Channel bindings (M3: Discord) and what each is doing.
    #[serde(default)]
    pub bindings: Vec<BindingStatus>,
    /// `narrative = true` in the config: the narrative runs, and the web UI
    /// shows its tab.
    #[serde(default)]
    pub narrative: bool,
    /// The context files every session's system block carries, and the
    /// persona in play with its own (theseus-c48).
    #[serde(default)]
    pub context: ContextStatus,
    /// Who may answer a waiting call, and through which channels (theseus-sgh).
    #[serde(default)]
    pub approval: ApprovalStatus,
    /// The tools that ask first because someone pressed "should have asked"
    /// (theseus-sgh), oldest first. They are stored, not configured.
    #[serde(default)]
    pub tightenings: Vec<Tightening>,
    /// The wakes conversations set with `wake.at` that have not run yet,
    /// soonest first (DD8).
    #[serde(default)]
    pub wakes: Vec<WakeInfo>,
    /// The sessions that hold external text (theseus-9bp), the longest-held
    /// first: in each, a call that acts waits for approval until the operator
    /// trusts it again.
    #[serde(default)]
    pub external_text: Vec<ExternalTextInfo>,
    /// The web UI's refusals since the daemon's image started (theseus-70f).
    #[serde(default)]
    pub web: WebStatus,
    /// Free space under the state dir, read when health is asked
    /// (theseus-102).
    #[serde(default)]
    pub disk: DiskStatus,
    /// The spool's sweeps of raw job output (theseus-2ij).
    #[serde(default)]
    pub spool: SpoolStatus,
}

/// The spool's sweeps (theseus-2ij): a job's raw output, what it printed
/// before the scrubber saw it, removed once no result will absorb it. A
/// sweep runs after serving, as the daemon starts, then every hour.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SpoolStatus {
    /// The last sweep since the daemon started; none before the first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub last_sweep: Option<SpoolSweep>,
}

/// One sweep of `spool/results`: counts and bytes, never content. Its
/// `spool.swept` row carries the same.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SpoolSweep {
    pub at_unix_ms: u64,
    pub took_ms: u64,
    pub removed: u64,
    pub removed_bytes: u64,
    pub kept: u64,
    pub kept_bytes: u64,
    /// The files removed, by why: `absorbed` (its result was written),
    /// `ended` (its execution ended before a turn read it), `unknown` (no job
    /// in the store, and a day old).
    #[serde(default)]
    pub removed_by: std::collections::BTreeMap<String, u64>,
    /// The files kept, by why: `running`, `pending` (its result may still be
    /// absorbed), `young` (no job in the store, under a day old), `unread`
    /// (the store could not be read for it), `failed` (its delete failed).
    #[serde(default)]
    pub kept_by: std::collections::BTreeMap<String, u64>,
}

/// Free space on the filesystem that holds the state dir (theseus-102), from
/// `statvfs` at the moment health is asked: the space an unprivileged
/// process may still use. On a full disk every append to the store fails, the
/// rows that would say so among them, so health warns first (`low`, under
/// `[server] disk_warn_mb`), and below `[server] disk_floor_mb` a new job is
/// refused with its reason (`below_floor`).
///
/// It sees only the filesystem Linux reports. Under WSL that is the virtual
/// disk, itself a file on the Windows drive (C:), and C: can fill first while
/// this still shows room: check the Windows drive there too.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DiskStatus {
    /// The state dir whose filesystem this is.
    pub path: String,
    /// `ok`, `low` (under the warning), `below_floor` (jobs are refused), or
    /// `unknown` (the filesystem could not be read: `error` says why).
    pub state: String,
    #[serde(default)]
    pub free_mb: u64,
    #[serde(default)]
    pub total_mb: u64,
    /// `[server] disk_warn_mb`; 0 never warns.
    #[serde(default)]
    pub warn_mb: u64,
    /// `[server] disk_floor_mb`; 0 refuses no job.
    #[serde(default)]
    pub floor_mb: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub error: Option<String>,
}

/// What the web UI refused (theseus-70f): a request whose `Host` is not the
/// UI's own loopback address and port (DNS rebinding), a WebSocket upgrade
/// whose `Origin` is not the UI's own page (any other page in the operator's
/// browser), and a connection whose client socket another uid owns (another
/// local user's process, theseus-3qf). Each kind is ledgered as
/// `web.refused`, at most once a minute, with how many refusals the row
/// stands for.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WebStatus {
    pub refused_host: u64,
    pub refused_origin: u64,
    #[serde(default)]
    pub refused_peer: u64,
    /// Set where the port cannot check its clients' owner: why (a platform
    /// with no table of socket owners, not Linux). Then a local process of
    /// any user is served.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub peer_unchecked: Option<String>,
    /// `[web] dev_origin`, when set (theseus-zab): the Vite dev page, whose
    /// `/ws` upgrades are served beside the UI's own page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub dev_origin: Option<String>,
    /// The `/ws` upgrades served for it, each ledgered as `web.dev_origin`
    /// at most once a minute.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub dev_origin_served: u64,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// Where the vault's secrets stand (theseus-qa0, spec §2 FAST): the daemon
/// answers its socket before they resolve, and each consumer waits for its own.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SecretsStatus {
    /// `resolving` until every secret has settled, then `ready`, or `failed`
    /// with `failed` naming each one that did not resolve.
    pub state: String,
    #[serde(default)]
    pub ready: Vec<String>,
    #[serde(default)]
    pub resolving: Vec<String>,
    #[serde(default)]
    pub failed: Vec<SecretFailed>,
    /// How the vault was read: `inject` (one `op inject` for every reference),
    /// or `inject, then read` after a failed injection.
    #[serde(default)]
    pub method: Option<String>,
    /// Rounds run: the first, then one per retry of what failed.
    #[serde(default)]
    pub rounds: u32,
    /// When resolution began, and when its first round settled, in ms after
    /// the process started.
    #[serde(default)]
    pub started_ms: Option<u64>,
    #[serde(default)]
    pub settled_ms: Option<u64>,
    /// Until the next fetch of what failed.
    #[serde(default)]
    pub retry_in_ms: Option<u64>,
}

impl SecretsStatus {
    /// `resolving`, `ready`, or `failed a, b`: what health says in a word.
    pub fn summary(&self) -> String {
        match self.state.as_str() {
            "failed" => format!(
                "failed {}",
                self.failed
                    .iter()
                    .map(|f| f.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            "" => "unknown".into(),
            s => s.into(),
        }
    }
}

/// The context files in the config (theseus-c48), as configured: the
/// system level, which every session gets, and the persona in play, whose
/// files follow the system level's. Until Jev chooses a persona, the persona
/// in play is `[context].default_persona`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ContextStatus {
    #[serde(default)]
    pub system_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub persona: Option<String>,
    #[serde(default)]
    pub persona_files: Vec<String>,
    /// Every persona the config defines.
    #[serde(default)]
    pub personas: Vec<String>,
}

/// Where the config came from, and whether it may act (theseus-2fo, spec
/// §3.19). A start whose config is an `op://` reference serves from the
/// last-known-good copy of the note and reads the vault behind the socket;
/// until the vault confirms the copy, the daemon answers only what reads.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ConfigStatus {
    /// `vault` or `file`: where the config lives.
    pub source: String,
    /// The `op://` reference, or the file's path.
    #[serde(default)]
    pub reference: String,
    /// `confirmed` (it may act), `confirming` (serving from the copy while
    /// the vault is read), `held` (the vault answered, and the copy may not
    /// act: `detail` says why), or `restarting` (onto the vault's changed
    /// note).
    pub state: String,
    /// How this start got it: `vault` (read before serving), `copy`, or `file`.
    #[serde(default)]
    pub started_from: String,
    /// Why it is held, what it is doing, or how it was confirmed, in words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub detail: Option<String>,
    /// When it was confirmed, in ms after the process started: the vault's
    /// answer, or the end of a read before serving.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub confirmed_ms: Option<u64>,
    /// The last-known-good copy's path, when the config is in the vault.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub copy: Option<String>,
    /// Reads of the vault behind the socket: the first, then each retry.
    #[serde(default)]
    pub reads: u32,
    /// Until the next read, when held.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub retry_in_ms: Option<u64>,
    /// This process began as a restart onto the vault's changed note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub restarted: Option<ConfigRestart>,
}

/// A restart onto the vault's changed config note (theseus-2fo): what
/// changed since the copy, by table name and digest, never by value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ConfigRestart {
    pub reference: String,
    /// When the process that found the change asked to restart.
    pub at_unix_ms: u64,
    /// The tables that differ (`kernel`, `policy.tools`, `profiles.glm`).
    pub tables: Vec<String>,
    pub copy_sha256: String,
    pub vault_sha256: String,
}

/// A secret that did not resolve, and why (never a value).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SecretFailed {
    pub name: String,
    pub error: String,
}

/// One phase of the last start (theseus-qa0), timed from process start.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct StartupPhase {
    /// `config`, `store`, `providers`, `kernel`, `core`, `socket` on the path
    /// to serving, one after another; `secrets`, `provider.<name>` (a turn's
    /// first wait for its key), `discord.token`, `github.check`, and
    /// `telemetry.headers` after it.
    pub name: String,
    /// After the socket answers: nothing on the path to serving waits for it.
    #[serde(default)]
    pub background: bool,
    pub start_us: u64,
    /// `None` while it runs.
    #[serde(default)]
    pub end_us: Option<u64>,
    #[serde(default)]
    #[cfg_attr(test, ts(type = "Record<string, unknown> | null"))]
    pub detail: Value,
}

/// A runtime tightening (theseus-sgh, spec §3.9): one press on a notice made
/// a tool ask first from then on. The gate applies it after the config's
/// posture, and the stricter of the two wins, so it never loosens anything.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Tightening {
    /// The tool's canonical name (`proc.run`).
    pub tool: String,
    /// What the tool asks at least: `approve`.
    pub posture: String,
    /// Who pressed, as a label (`discord:eddie`, `sock#3`, `web#1`).
    pub by: String,
    /// Who pressed, as the approval rule knows them: a Discord user by id,
    /// anyone else by label.
    #[serde(default)]
    pub who: String,
    /// The channel it came through, as `[approval].channels` names it
    /// (`cli`, `web`, `discord:dm`, `discord:<channel id>`).
    #[serde(default)]
    pub via: String,
    pub at_ms: u64,
    /// The call whose notice was pressed. With `digest` and `tool`, it is a
    /// labeled example for later judgment work (Jev, M5). A press from the
    /// CLI without `--call` names none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub correlation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_id: Option<String>,
    /// The call's proposal digest (its action's `args_digest`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub digest: Option<String>,
}

/// `policy.tighten`: make a tool ask first from now on.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PolicyTightenParams {
    pub tool: String,
    /// The call whose notice was pressed, when there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub correlation_id: Option<String>,
    /// Who pressed, as a label. Default: the connection. It names and
    /// proves nothing; the connection's surface decides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    /// Set by the Discord binding: the channel and user the press came from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<DiscordOrigin>,
}

/// `policy.untighten`: the tool goes back to what the config says.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PolicyUntightenParams {
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<DiscordOrigin>,
}

/// What a tighten or an undo did, and the tool's posture now. Also the
/// params of `policy.tightened` and `policy.untightened`, where a field a
/// notification lacks reads as its default (theseus-0g4).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct TightenResult {
    pub tool: String,
    /// Who made this change: the one who pressed, or the one who undid it.
    pub by: String,
    /// The tightening recorded (tighten) or removed (untighten).
    pub tightening: Tightening,
    /// The posture the gate applies to the tool now, and the setting that
    /// chose it.
    pub posture: String,
    pub setting: String,
    /// What the config alone says.
    pub config_posture: String,
    pub config_setting: String,
    /// The tool's posture changed. False for a press under a config that
    /// already asks, and for an undo the config still asks under.
    pub changed: bool,
    /// The tool was tightened already, so nothing was recorded.
    #[serde(default)]
    pub already: bool,
}

/// What made a session hold external text (theseus-9bp, spec §3.9): the first
/// result marked `external` that entered its context since the operator last
/// trusted it, or the hold it took from another session. From then on, every
/// call whose class is not `read` waits for the operator's approval.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ExternalText {
    /// When the session came to hold it.
    pub since_ms: u64,
    /// The tool whose result it was (`http.fetch`, `web.search`). A hold
    /// taken from another session names that session's first source.
    pub tool: String,
    /// Where the text came from: the page's final URL, or the search's
    /// request.
    pub url: String,
    /// The node that brought it into this session: the result, a task's
    /// brief, or a task's report.
    pub node_id: String,
    /// The session it came from, when this one took it from another.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub from_session: Option<String>,
    /// How it came from there: `task.create` (a task that a session holding
    /// it started) or `task.report` (a report from a task that held it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub via: Option<String>,
    /// A search's query (`web.search`), which the hold names in place of
    /// the request's URL (theseus-qiy); the URL stays on the result node.
    /// Absent for a fetch, and in holds written before it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub query: Option<String>,
}

impl ExternalText {
    /// What the session read, as every surface names it (theseus-qiy): a
    /// search by its query, `web.search "tokio JoinSet documentation"`, and
    /// anything else by its URL, `http.fetch <url>`. A hold written before
    /// the query was kept names the search's URL.
    pub fn what(&self) -> String {
        match &self.query {
            Some(q) => format!("{} \"{q}\"", self.tool),
            None => format!("{} {}", self.tool, self.url),
        }
    }
}

/// A session that holds external text, as health lists it (theseus-9bp).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ExternalTextInfo {
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub title: Option<String>,
    /// The task's short id, when the session is a task's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub task: Option<String>,
    pub held: ExternalText,
    /// When the hold began, in the daemon's local time, as the hold's reason
    /// says it (theseus-qiy): `12:55:01`, with the day when it is not today.
    /// Empty from a daemon before it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub since_local: String,
}

/// `policy.trust`: the session no longer holds external text.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PolicyTrustParams {
    pub session_id: String,
    /// Who trusted it, as a label. Default: the connection. It names and
    /// proves nothing; the connection's surface decides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<DiscordOrigin>,
}

/// What a trust cleared, and who cleared it: `policy.trust`'s result, and the
/// params of `session.trusted`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TrustResult {
    pub session_id: String,
    /// Who trusted it, as a label.
    pub by: String,
    /// Who, as the approval rule knows them, and the channel it came through.
    #[serde(default)]
    pub who: String,
    #[serde(default)]
    pub via: String,
    /// `policy.trust`, or `action.confirm` for an approval that trusted the
    /// session too.
    pub how: String,
    /// The approval that trusted it, when one did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub correlation_id: Option<String>,
    pub at_ms: u64,
    /// The hold it cleared.
    pub held: ExternalText,
    /// When the hold began, in the daemon's local time (theseus-qiy), as
    /// health's `since_local`. Empty from a daemon before it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub since_local: String,
}

/// `[approval]` as health reports it (spec §3.9 "Approval"): the trusted
/// users, and each listed channel with its state now.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ApprovalStatus {
    /// The config has an `[approval]` section. Without one, every surface
    /// answers as before theseus-sgh: the CLI, the local web UI, and a
    /// place's listed Discord users.
    pub configured: bool,
    /// Surface-qualified ids, as configured (`discord:<user id>`).
    #[serde(default)]
    pub trusted_users: Vec<String>,
    #[serde(default)]
    pub channels: Vec<ApprovalChannel>,
}

/// One entry of `[approval].channels` and whether it is trusted now.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ApprovalChannel {
    /// As configured: `cli`, `web`, `discord:dm`, or `discord:<channel id>`.
    pub channel: String,
    /// `trusted` | `not_trusted`.
    pub state: String,
    /// Why, in words: what the channel is, or why it does not count.
    pub detail: String,
    /// When Discord last checked who can view it (a guild channel); 0 if never.
    #[serde(default)]
    pub checked_at_ms: u64,
}

/// The Discord ids behind an answer, which the Discord binding reads off the
/// button press (theseus-sgh). The core takes them only from the binding's
/// own connection.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DiscordOrigin {
    pub user_id: String,
    pub channel_id: String,
    /// None in a DM.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub guild_id: Option<String>,
}

/// One channel binding as health reports it (spec P5: bindings as a file).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BindingStatus {
    /// "discord".
    pub kind: String,
    /// unconfigured | disabled | connecting | ready | resuming | disconnected | failed
    pub state: String,
    /// Why it is in that state, when that is not obvious (a missing file, a close code).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub bot_user: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub guild_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub bindings_file: Option<String>,
    /// First 12 hex of the bindings file's SHA-256: the binding revision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub revision: Option<String>,
    #[serde(default)]
    pub places: Vec<PlaceStatus>,
    #[serde(default)]
    pub connected_at_ms: u64,
    /// Gateway heartbeat round trip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub latency_ms: Option<u64>,
    #[serde(default)]
    pub messages_in: u64,
    #[serde(default)]
    pub messages_out: u64,
    #[serde(default)]
    pub edits: u64,
    #[serde(default)]
    pub interactions: u64,
    #[serde(default)]
    pub ignored: u64,
    #[serde(default)]
    pub errors: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub last_error: Option<String>,
    /// The developer portal has the Server Members intent on for this bot
    /// (the application's flags): who can view a guild channel can then be
    /// checked. None until the binding has asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub members_intent: Option<bool>,
    /// Its outbox (theseus-q4v): the posts waiting for it, and how delivery
    /// goes. The core fills it in for health.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub outbox: Option<OutboxStatus>,
}

/// A binding's outbox (theseus-q4v): what waits to reach its channels.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OutboxStatus {
    /// Posts written and not yet delivered.
    pub pending: u64,
    /// Posts delivered, in this store's life.
    pub sent: u64,
    /// Posts the channel refused for good (a deleted channel, lost access).
    pub failed: u64,
    /// When the oldest pending post was written (unix ms); 0 with none.
    #[serde(default)]
    pub oldest_pending_ms: u64,
    /// The last delivery error, and when (unix ms).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub last_error: Option<String>,
    #[serde(default)]
    pub last_error_ms: u64,
}

/// A place Theseus lives in: a text channel or a DM, and the session behind it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PlaceStatus {
    /// "channel" | "dm".
    pub kind: String,
    pub label: String,
    /// The Discord channel id (for a DM, known after the DM channel opens).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub channel_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_id: Option<String>,
    /// Discord user ids that may drive turns here.
    #[serde(default)]
    pub users: Vec<String>,
    /// Only messages that @mention the bot or reply to it start turns.
    #[serde(default)]
    pub mention_only: bool,
    #[serde(default)]
    pub last_activity_ms: u64,
}

/// The OTLP exporter (theseus-hee, spec §3.20): what it sent and dropped.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TelemetryStatus {
    pub enabled: bool,
    pub otlp_endpoint: Option<String>,
    /// `off` (no endpoint), `waiting` (for the vault's confirmation of the
    /// config, or for the headers secret), `exporting`, or `failed` (the
    /// exporter could not be built). Empty from a daemon older than that.
    #[serde(default)]
    pub state: String,
    /// Why it waits, or why it failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub detail: Option<String>,
    /// Batches the receiver took: one per turn's trace, one per metrics export.
    #[serde(default)]
    pub traces_sent: u64,
    #[serde(default)]
    pub metrics_sent: u64,
    #[serde(default)]
    pub spans_sent: u64,
    /// Batches dropped: after a failed retry, or the oldest trace when the
    /// queue was full.
    #[serde(default)]
    pub dropped: u64,
    /// Traces waiting for the sender.
    #[serde(default)]
    pub queued: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub last_error_at_ms: Option<u64>,
}

impl TelemetryStatus {
    pub fn off() -> Self {
        Self {
            state: "off".into(),
            ..Default::default()
        }
    }

    /// What `theseus health` says after `telemetry: `, as of `now_ms`.
    pub fn summary(&self, now_ms: u64) -> String {
        let at = self.otlp_endpoint.as_deref().unwrap_or("?");
        let detail = self.detail.as_deref().unwrap_or("?");
        match (self.state.as_str(), &self.otlp_endpoint) {
            ("exporting", _) => {
                let mut s = format!(
                    "exporting to {at} · sent {} ({} traces, {} metrics) · dropped {}",
                    self.traces_sent + self.metrics_sent,
                    self.traces_sent,
                    self.metrics_sent,
                    self.dropped
                );
                if self.queued > 0 {
                    s.push_str(&format!(" · {} waiting", self.queued));
                }
                if let Some(e) = &self.last_error {
                    s.push_str(&format!(" · last error {e}"));
                    if let Some(t) = self.last_error_at_ms {
                        s.push_str(&format!(
                            " ({} s ago)",
                            now_ms.saturating_sub(t).div_ceil(1000)
                        ));
                    }
                }
                s
            }
            ("waiting", _) => format!("waiting to export to {at}: {detail}"),
            ("failed", _) => format!("not exporting to {at}: {detail}"),
            // A daemon older than theseus-hee.
            ("", Some(e)) => format!("OTLP/HTTP → {e}"),
            _ => "off (no [telemetry].otlp_endpoint)".into(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct KernelStatus {
    /// Startup finished (five steps) and events are accepted.
    pub accepting: bool,
    pub admission_ceiling: u32,
    /// Executions holding a turn right now.
    pub turns_held: u32,
    /// Counts by state: queued, running, waiting, blocked, cancelled, failed, budget_exhausted, complete.
    #[serde(default)]
    pub executions_by_state: std::collections::BTreeMap<String, u64>,
    /// Counts by state: planned, authorized, dispatched, succeeded, failed, outcome_unknown, cancelled.
    #[serde(default)]
    pub actions_by_state: std::collections::BTreeMap<String, u64>,
    /// Completions that matched no action (never inferred into anything).
    pub quarantined_completions: u64,
    /// The last startup: step timings in µs and what it recovered.
    #[serde(default)]
    #[cfg_attr(test, ts(type = "unknown"))]
    pub startup: Value,
    /// The spend limit a new session gets, in US dollars (`[kernel]
    /// spend_limit_usd`).
    #[serde(default)]
    pub spend_limit_usd: f64,
    /// Job wrappers whose command has exited, each still waiting for the
    /// descendants that outlived it (theseus-6qy).
    #[serde(default)]
    pub lingering_wrappers: u64,
}

/// The daemon's children (theseus-z4b): the job wrappers it spawned, the
/// orphans it adopted, and what waits to be reaped. The daemon reaps each
/// wrapper and orphan once it exits, and leaves the `op` processes to tokio,
/// which waits for them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ChildrenStatus {
    /// The daemon is a child subreaper, so a job's descendant whose wrapper
    /// died is reparented to it, not to init. Only the socket daemon is one.
    pub subreaper: bool,
    /// Job wrappers among its children whose command still runs.
    pub wrappers_running: u64,
    /// Job wrappers among its children whose command has exited, each
    /// lingering for what it left running.
    pub wrappers_lingering: u64,
    /// Processes it adopted: a job's descendants whose wrapper died. None of
    /// them may answer an approval.
    pub orphans: u64,
    /// Children that have exited and wait to be reaped: 0 in steady state, so
    /// a count that grows is a leak.
    pub zombies: u64,
    /// Children that tokio waits for itself: the `op` processes.
    pub owned: u64,
    /// Job wrappers reaped since the daemon's image started.
    pub reaped_wrappers: u64,
    /// Orphans reaped since the daemon's image started.
    pub reaped_orphans: u64,
}

/// One grant of the secret broker (theseus-dcy): who gets which secret, how,
/// and how often since the daemon started. Never a value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct GrantStatus {
    /// `program`, for a job's program by its argv, or `tool`, for a toollet.
    pub kind: String,
    /// The program (`gh`) or the toollet (`web.search`).
    pub to: String,
    /// The environment variable a program gets it in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub variable: Option<String>,
    /// The `[secrets]` name.
    pub secret: String,
    /// The secret's posture: a call given it runs at no looser one.
    pub posture: String,
    /// Times handed out since the daemon started.
    pub uses: u64,
}

/// One execution as the protocol shows it (spec §3.15).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ExecutionInfo {
    pub execution_id: String,
    pub session_id: String,
    pub kind: String,
    pub state: String,
    pub turns: u64,
    /// Times a crash interrupted a running turn (requeued at startup).
    pub interrupted: u32,
    /// Dispatched actions not yet settled.
    pub outstanding: u32,
    /// Settled results the next turn will consume.
    pub queued_results: u32,
    pub budget: BudgetInfo,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    #[cfg_attr(test, ts(type = "unknown"))]
    pub wake: Value,
    /// `wake`, typed (theseus-in3): what it waits on, while it waits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub waiting_on: Option<WaitingOn>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub reports_to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub ended_reason: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    /// What it needs from people, by `attention()` (theseus-in3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub attention: Option<Attention>,
}

/// An execution's budget in US dollars (theseus-0sg).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BudgetInfo {
    pub limit_usd: f64,
    /// Settled costs since the execution opened or was last reset. The
    /// session's `cost_usd` is its lifetime total, which a reset never lowers.
    pub spent_usd: f64,
    pub reserved_usd: f64,
    pub held_unknown_usd: f64,
    pub available_usd: f64,
    /// Approved resets of the spend to $0.
    #[serde(default)]
    pub resets: u32,
    /// The budget question waiting for the operator (a correlation id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub question: Option<String>,
    /// The unit budget a record stored before dollar budgets carried, as it
    /// was: `limit`, `spent`, `reserved`, `held_unknown`.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    #[cfg_attr(
        test,
        ts(
            type = "{ limit: number; spent: number; reserved: number; held_unknown: number } | null"
        )
    )]
    pub units_before: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ExecutionListResult {
    pub executions: Vec<ExecutionInfo>,
}

/// One action (a tool or provider call with a correlation id, spec §3.16).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ActionInfo {
    pub correlation_id: String,
    pub execution_id: String,
    pub session_id: String,
    pub tool: String,
    pub state: String,
    pub retry_class: String,
    pub planned_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub authorized_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub dispatched_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub settled_at_ms: Option<u64>,
    pub deadline_at_ms: u64,
    /// What the action's budget reservation holds, in US dollars.
    #[serde(default)]
    pub reserved_usd: f64,
    pub confirmed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub cancel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub external_op_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub result_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub resolution: Option<String>,
    pub completions_seen: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ActionListParams {
    /// Only this execution's actions.
    #[serde(default)]
    pub execution_id: Option<String>,
    /// Newest `n` (default 200).
    #[serde(default)]
    pub n: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ActionListResult {
    pub actions: Vec<ActionInfo>,
    pub total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ExecutionCancelParams {
    pub execution_id: String,
    /// Who asked, as a label in the ledger (e.g. `discord:eddie`). Default: the connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ExecutionCancelResult {
    pub execution: ExecutionInfo,
    /// Dispatched actions whose backends were asked to stop.
    pub cancelled_actions: Vec<String>,
}

/// `execution.stop` (W1, theseus-lji; `/stop`): halt a conversation's work
/// and keep the conversation. Its running jobs and calls are told to stop,
/// what waits on the operator is declined, and a running turn plans nothing
/// more; the execution then waits on its next input. A task is refused:
/// `task.cancel` stops one.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ExecutionStopParams {
    pub execution_id: String,
    /// Who asked, as a label in the ledger (e.g. `discord:eddie`). Default:
    /// the surface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ExecutionStopResult {
    pub execution: ExecutionInfo,
    /// False when the execution had ended already: nothing was stopped.
    pub stopped: bool,
    /// Running jobs and calls whose backends were asked to stop.
    pub stopped_actions: Vec<String>,
    /// Planned calls, approvals, and a budget question that will not run.
    pub declined: Vec<String>,
    /// A turn was running: it ends at its next step.
    pub turn_running: bool,
    /// What goes on, which `/cancel <id>` stops one by one: the session's
    /// running tasks and its pending wakes.
    pub tasks_running: u32,
    pub wakes_pending: u32,
}

/// A task (DD7, theseus-qn2): a child session a conversation opened with
/// `task.create`, which works on its own and reports back to the place.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TaskInfo {
    /// Its session's id (`ses_…`).
    pub task_id: String,
    /// The last six characters of its id, which is how people name it
    /// (`/cancel a1b2c3`).
    pub short: String,
    pub execution_id: String,
    pub parent_session_id: String,
    pub parent_execution_id: String,
    /// The brief's first line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub title: Option<String>,
    /// Its execution's state: `queued`, `running`, `waiting`, `complete`, …
    pub state: String,
    /// What it waits on, while it waits: `actions` (a job), `confirm`,
    /// `budget`, or `input`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub waiting_on: Option<String>,
    /// Its spend under its carved limit (since any reset), and that limit.
    pub spent_usd: f64,
    pub limit_usd: f64,
    /// What its session has cost in all, resets included.
    pub cost_usd: f64,
    pub turns: u64,
    /// Questions it asks the operator now.
    #[serde(default)]
    pub pending_confirms: u32,
    /// Where its cards and report go (`discord:dm:<user>`), if anywhere.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub ended_reason: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    /// Its report starts its parent's next turn (W1, `wake_parent`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub wake_parent: bool,
    /// What it needs from people, by `attention()` (theseus-in3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub attention: Option<Attention>,
}

/// `task.list`: every task, the newest first, or only one session's, or
/// only those that report to one place.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TaskListParams {
    /// Only the tasks this session started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_id: Option<String>,
    /// Only the tasks that report to this place (`discord:dm:<user>`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub target: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TaskListResult {
    pub tasks: Vec<TaskInfo>,
}

/// `task.cancel`: stop a task and its jobs, as `execution.cancel` does; the
/// place hears that it was cancelled, once.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TaskCancelParams {
    /// Its id, its execution's id, or the end of either (`a1b2c3`), as long
    /// as one task matches.
    pub task: String,
    /// Who asked, as the ledger names them. Default: the surface (`the CLI`,
    /// `the web UI`; DD8), or the connection's label on a surface no
    /// listener named.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TaskCancelResult {
    pub task: TaskInfo,
    /// Dispatched actions whose backends were asked to stop.
    pub cancelled_actions: Vec<String>,
}

/// A pending wake (DD8, theseus-cff): a conversation asked, with `wake.at`,
/// for a turn at a time, whose input is its note. It has not run yet.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WakeInfo {
    /// `wak_…`.
    pub wake_id: String,
    /// The last six characters of its id, which is how people name it
    /// (`/cancel a1b2c3`).
    pub short: String,
    pub session_id: String,
    pub execution_id: String,
    /// The session's title, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_title: Option<String>,
    pub due_at_ms: u64,
    /// The due time on the daemon's clock, as people read it
    /// (`2026-09-30 13:15:00 -07:00`).
    pub due_local: String,
    pub note: String,
    pub set_at_ms: u64,
    /// The place it was set from (`discord:dm:<user>`), where its turn's
    /// reply goes, if anywhere.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub target: Option<String>,
    /// Its session's execution state now: a wake waits for a busy session.
    pub state: String,
}

/// `wake.list`: every pending wake, soonest first, or only one session's, or
/// only those whose turns post to one place.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WakeListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_id: Option<String>,
    /// Only the wakes whose turns post to this place (`discord:dm:<user>`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub target: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WakeListResult {
    pub wakes: Vec<WakeInfo>,
}

/// `wake.cancel`: cancel a pending wake, so nothing fires.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WakeCancelParams {
    /// Its id, or the end of it (`a1b2c3`), as long as one wake matches.
    pub wake: String,
    /// Who asked, as the ledger names them. Default: the surface (`the CLI`,
    /// `the web UI`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WakeCancelResult {
    /// The wake as it was before the cancel.
    pub wake: WakeInfo,
}

/// A question's task, when a task asks it (DD7): the card names it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TaskRef {
    pub task_id: String,
    pub short: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub title: Option<String>,
}

/// Also the kernel's: an execution stores it (`theseus_kernel::SessionKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SessionKind {
    Conversation,
    Task,
}

impl SessionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SessionKind::Conversation => "conversation",
            SessionKind::Task => "task",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SessionOpenParams {
    #[serde(default)]
    pub kind: Option<SessionKind>,
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SessionInfo {
    pub session_id: String,
    pub kind: SessionKind,
    pub label: Option<String>,
    pub created_at_unix_ms: u64,
    pub turns: u64,
    /// Cumulative tokens over every turn in this session.
    #[serde(default)]
    pub usage: Usage,
    /// The session's one execution (spec §3.2a) and its current state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub execution_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub execution_state: Option<String>,
    #[serde(default)]
    pub last_active_ms: u64,
    #[serde(default)]
    pub cost_usd: f64,
    #[serde(default)]
    pub tool_calls: u64,
    /// Profile/provider/model of the last turn (continuations reuse it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub compilation_id: Option<String>,
    /// First words of the first prompt, for pickers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub title: Option<String>,
    /// Tool calls in this session waiting for the operator's confirmation.
    #[serde(default)]
    pub pending_confirms: u32,
    /// A task session's parent (DD7): the session that started it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub parent_session_id: Option<String>,
    /// A task's carved limit, in US dollars.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub limit_usd: Option<f64>,
    /// The session holds external text (theseus-9bp): what it read, and
    /// since when. Its calls that act wait until the operator trusts it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub external_text: Option<ExternalText>,
    /// What its execution needs from people, by `attention()` (theseus-in3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub attention: Option<Attention>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SessionListResult {
    pub sessions: Vec<SessionInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TurnSubmitParams {
    /// Omit to open a fresh conversation session for this turn.
    #[serde(default)]
    pub session_id: Option<String>,
    pub input: String,
    /// Profile for this turn (a configured profile name); default is the live profile.
    #[serde(default)]
    pub profile: Option<String>,
    /// Raw override of the profile's provider for this turn.
    #[serde(default)]
    pub provider: Option<String>,
    /// Raw override of the profile's model for this turn.
    #[serde(default)]
    pub model: Option<String>,
    /// Who wrote the input, as a label on the message node (e.g. `discord:eddie`).
    /// Default: the connection's own label. A label, not an authority: every
    /// local protocol client acts as the operator.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    /// Files that came with the input, in order (theseus-9g2). With any, the
    /// input may be empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<Attachment>,
    /// The surface's message this turn answers (a Discord message id): the
    /// reply's first message is posted as a reply to it (theseus-q4v).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub reply_to: Option<String>,
}

/// A file sent with a turn's input (theseus-9g2): a Discord attachment, or
/// `theseus ask --attach`. Text travels as `text` and an image as base64
/// `data`. A file the sender did not read carries `not_read` instead, with
/// the reason, and the model is told it exists.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Attachment {
    pub name: String,
    /// As the sender knows it (`text/plain`, `image/png`); may be empty.
    #[serde(default)]
    pub media_type: String,
    /// Bytes of the whole file.
    #[serde(default)]
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub text: Option<String>,
    /// The file's bytes, base64 (standard alphabet, padded).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub data: Option<String>,
    /// Why it was not read: too large, a type that is not read, a failed download.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub not_read: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProfileInfo {
    pub name: String,
    pub provider: String,
    pub model: String,
    /// Output cap per call (the API's `max_tokens`), not an input limit.
    #[serde(alias = "max_tokens")]
    pub max_output_tokens: u32,
    pub has_system: bool,
    pub live: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProfileListResult {
    pub live: String,
    /// Where the live choice came from: "config" or "runtime" (persisted switch).
    pub live_source: String,
    pub profiles: Vec<ProfileInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProfileUseParams {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProfileChanged {
    pub previous: String,
    pub live: String,
    pub by: String,
}

/// One timed span of a turn trace. Times are microseconds from the turn's
/// start; a mark has `end_us == start_us`. Children are in start order.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Span {
    pub name: String,
    /// turn | loop | provider | tool | mark | advancer | store | lock | compile.
    /// Traces stored before theseus-hco also hold `hook` spans.
    pub kind: String,
    pub start_us: u64,
    #[serde(default)]
    pub end_us: Option<u64>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    #[cfg_attr(test, ts(type = "unknown"))]
    pub attrs: Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Span>,
}

impl Span {
    pub fn duration_us(&self) -> u64 {
        self.end_us
            .unwrap_or(self.start_us)
            .saturating_sub(self.start_us)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
    /// Every cache write, whatever its TTL.
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
    /// Of `cache_creation_input_tokens`, those written with the 1-hour TTL
    /// (Anthropic's `usage.cache_creation.ephemeral_1h_input_tokens`), which
    /// cost 2 × input against 1.25 × for the rest (theseus-ev1). Absent when
    /// 0, so a row or record from before it reads as none.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub cache_creation_1h_input_tokens: u64,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TurnSubmitResult {
    pub session_id: String,
    pub turn_id: String,
    pub loops: u32,
    pub output: String,
    /// Why the Advancer ended the turn.
    pub stop_reason: String,
    /// The provider's own stop reason for the last loop.
    pub provider_stop_reason: Option<String>,
    pub model: String,
    #[serde(default)]
    pub provider: String,
    /// Profile the turn ran under ("" when raw overrides bypassed profiles entirely).
    #[serde(default)]
    pub profile: String,
    pub usage: Usage,
    pub elapsed_ms: u64,
    /// Time to the first streamed token of the last loop.
    #[serde(default)]
    pub first_token_ms: Option<u64>,
    /// Provider request id of the last loop, for support tickets.
    #[serde(default)]
    pub request_id: Option<String>,
    /// Every timed thing in the turn, nested: turn > loops > provider/tools/advancer.
    #[serde(default)]
    pub trace: Option<Span>,
    #[serde(default)]
    pub execution_id: Option<String>,
    /// Dollars for this turn's provider calls, from the model catalog (None: model not in catalog).
    #[serde(default)]
    pub cost_usd: Option<f64>,
    #[serde(default)]
    pub tool_calls: u32,
    /// Set when the turn parked waiting for the operator to confirm this action.
    #[serde(default)]
    pub awaiting_confirm: Option<String>,
    /// The provider's `stop_details` (a refusal's category).
    #[serde(default)]
    #[cfg_attr(test, ts(type = "unknown"))]
    pub stop_details: Option<Value>,
    /// The turn ran without new input (a continuation: late results, a confirm answer, a restart).
    #[serde(default)]
    pub continuation: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LedgerTailParams {
    #[serde(default)]
    pub n: Option<usize>,
    /// Only rows of this kind (e.g. "turn.ended", "provider.error"). A renamed
    /// kind also reads the rows stored under its old name.
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LedgerEntry {
    pub position: u64,
    pub at_unix_ms: u64,
    pub kind: String,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    #[cfg_attr(test, ts(type = "unknown"))]
    pub data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LedgerTailResult {
    pub rows: Vec<LedgerEntry>,
    pub total: u64,
}

// ---------------------------------------------------------------- M3: content

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SessionRef {
    pub session_id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SessionHistoryParams {
    pub session_id: String,
    /// Newest `n` nodes (default all).
    #[serde(default)]
    pub n: Option<usize>,
}

/// One node as clients render it (a message, a tool call, a tool result).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct NodeInfo {
    pub node_id: String,
    /// `user_message`, `assistant_message`, `tool_call`, `tool_result`.
    pub kind: String,
    pub session_id: String,
    pub position: u64,
    pub at_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub loop_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    /// Text for display (user text; assistant text blocks; tool result content).
    #[serde(default)]
    pub text: String,
    /// Thinking summaries (assistant), when the provider returned them.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub thinking: String,
    /// Kind-specific fields: model/usage/cost (assistant), tool/input/gate
    /// (tool call), tool/status/is_error/bytes (tool result).
    #[serde(default)]
    #[cfg_attr(test, ts(type = "Record<string, unknown> | null"))]
    pub detail: Value,
    #[serde(default)]
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SessionHistoryResult {
    pub session: SessionInfo,
    pub nodes: Vec<NodeInfo>,
    /// Actions waiting for the operator's confirmation in this session.
    #[serde(default)]
    pub pending_confirms: Vec<ConfirmRequest>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SessionRecompileParams {
    pub session_id: String,
    /// `fresh` (start over) or `transcript` (keep everything, thinking stripped).
    pub strategy: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct NodeListParams {
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub n: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct NodeListResult {
    pub nodes: Vec<NodeInfo>,
    pub total: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CompilationListParams {
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub n: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CompilationInfo {
    pub compilation_id: String,
    pub session_id: String,
    pub created_at_ms: u64,
    pub trigger: String,
    pub strategy: String,
    pub as_of: u64,
    pub includes: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub derived_from: Option<String>,
    /// The manifest as stored (model, provider, digests, catalog version, strip_thinking).
    #[cfg_attr(test, ts(type = "Record<string, unknown>"))]
    pub manifest: Value,
    /// This is the session's current compilation.
    #[serde(default)]
    pub current: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CompilationListResult {
    pub compilations: Vec<CompilationInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CatalogModel {
    pub model: String,
    /// The catalog row as configured (provider, window, prices, capabilities, source).
    #[cfg_attr(test, ts(type = "Record<string, unknown>"))]
    pub entry: Value,
    /// Profiles that use this model.
    #[serde(default)]
    pub profiles: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CatalogListResult {
    pub version: String,
    pub models: Vec<CatalogModel>,
}

/// A tool call waiting for the operator (spec §3.9: confirmation is bound to
/// the exact tool, arguments, resource, and an expiry).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ConfirmRequest {
    pub correlation_id: String,
    pub session_id: String,
    pub execution_id: String,
    pub tool: String,
    #[cfg_attr(test, ts(type = "unknown"))]
    pub input: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub resource: Option<String>,
    /// Why policy asks (e.g. "write under /home/x/projects", "run cargo test").
    pub reason: String,
    pub by: String,
    pub requested_at_ms: u64,
    /// When the question stops holding; 0 for a budget question, which
    /// holds until it is answered or a newer one replaces it.
    pub expires_at_ms: u64,
    /// The floor asks: the call touches Theseus's own binary or state, or the
    /// 1Password CLI or token. It asks at every posture.
    #[serde(default)]
    pub floor: bool,
    /// Set on a budget question (`tool` is `budget.reset`): the figures it
    /// asks about. `reason` is the question in words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub budget: Option<BudgetAsk>,
    /// The task that asks, when a task does (DD7).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub task: Option<TaskRef>,
    /// The call waits because its session read external text (theseus-9bp):
    /// what it read. An approval with `trust` clears that as well.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub external_text: Option<ExternalText>,
}

/// What a budget question asks about (theseus-0sg): the session reached its
/// spend limit. Approving resets `spent_usd` to $0 and the waiting call
/// proceeds; the session's lifetime cost keeps counting.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BudgetAsk {
    pub spent_usd: f64,
    pub limit_usd: f64,
    /// What the waiting call reserves.
    pub needed_usd: f64,
    /// The session's lifetime cost, resets included.
    pub lifetime_usd: f64,
}

/// `confirm.list`: the questions of every session parked on one, the most
/// recently active session first.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ConfirmListResult {
    pub confirms: Vec<ConfirmRequest>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ActionConfirmParams {
    pub correlation_id: String,
    pub approve: bool,
    #[serde(default)]
    pub note: Option<String>,
    /// Subscribe this connection to the session's events before the answer
    /// wakes the execution, so the continuation turn is seen from its start.
    #[serde(default)]
    pub watch: bool,
    /// Who answered, as a label (e.g. `discord:eddie`). Default: the connection.
    /// A label names; it proves nothing. With `[approval]`, the connection's
    /// surface and `discord` decide whether the answer counts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    /// Set by the Discord binding: the channel and user the answer came from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<DiscordOrigin>,
    /// Approve, and trust the session again (theseus-9bp): it no longer holds
    /// external text, so its calls that act go back to their postures. Only
    /// with `approve`; the answer's judgment covers it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub trust: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ActionConfirmResult {
    pub correlation_id: String,
    pub approved: bool,
    pub session_id: String,
    pub execution_id: String,
    /// The answer woke the execution, so a continuation turn follows. False
    /// for a declined budget question: the session keeps waiting, and its
    /// next message asks again.
    #[serde(default = "yes")]
    pub resumes: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ToolInfo {
    /// Canonical name (`fs.read`).
    pub name: String,
    /// Name on the wire (`fs_read`).
    pub wire_name: String,
    pub family: String,
    pub description: String,
    /// `read`, `write`, or `run`.
    pub class: String,
    /// `inproc` or `job`.
    pub backend: String,
    /// The posture the gate applies now: `open`, `notify`, or `approve`.
    pub policy: String,
    /// What chose it: a config setting (`enforcement = notify`), or a
    /// tightening (`tightened by discord:eddie`).
    #[serde(default)]
    pub setting: String,
    /// What the config alone says (theseus-sgh). It differs from `policy`
    /// only while a tightening is stricter.
    #[serde(default)]
    pub config_posture: String,
    #[serde(default)]
    pub config_setting: String,
    /// Someone pressed "should have asked" for this tool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub tightened: Option<Tightening>,
    #[cfg_attr(test, ts(type = "unknown"))]
    pub input_schema: Value,
    /// Calls since the daemon started.
    #[serde(default)]
    pub calls: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ToolListResult {
    pub tools: Vec<ToolInfo>,
    /// Workspace roots tools may touch.
    pub roots: Vec<String>,
    /// `proc.run` calls over all tool calls (spec §3.23 shell-fallback ratio).
    pub shell_fallback_ratio: f64,
    /// Calls since the daemon started.
    pub calls_total: u64,
}

/// `error.data` on a provider failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProviderErrorData {
    pub class: String,
    pub transient: bool,
    pub usage_unknown: bool,
    pub turn_id: Option<String>,
    pub session_id: String,
    pub elapsed_ms: u64,
    /// The trace up to the failure.
    #[serde(default)]
    pub trace: Option<Span>,
    /// What the turn's finished loops spent before it failed.
    #[serde(default)]
    pub usage: Usage,
    #[serde(default)]
    pub cost_usd: Option<f64>,
    #[serde(default)]
    pub tool_calls: u32,
}

/// A notification's params: a field it lacks reads as its default, as the
/// renderers always read them (theseus-0g4).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct TurnStarted {
    pub session_id: String,
    pub turn_id: String,
    #[serde(default)]
    pub execution_id: Option<String>,
    #[serde(default)]
    pub continuation: bool,
}

/// A notification's params: a field it lacks reads as its default, as the
/// renderers always read them (theseus-0g4).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct TurnFailed {
    pub session_id: String,
    #[serde(default)]
    pub turn_id: Option<String>,
    #[serde(default)]
    pub execution_id: Option<String>,
    #[serde(default)]
    pub continuation: bool,
    /// Error class when known (rate_limited, overloaded, auth, ...).
    #[serde(default)]
    pub class: Option<String>,
    pub error: String,
    /// What the execution does next (theseus-ljr): `backoff` (the driver
    /// retries while the class lasts), `retry` (once), or `park` (it waits
    /// on input, and the next message retries). Absent for a task's turn and
    /// a stopped one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub then: Option<String>,
}

/// A notification's params: a field it lacks reads as its default, as the
/// renderers always read them (theseus-0g4).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct LoopStarted {
    pub turn_id: String,
    pub loop_index: u32,
    pub model: String,
    pub tools_offered: u32,
}

/// A notification's params: a field it lacks reads as its default, as the
/// renderers always read them (theseus-0g4).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ModelDelta {
    pub turn_id: String,
    pub loop_index: u32,
    pub text: String,
}

/// A notification's params: a field it lacks reads as its default, as the
/// renderers always read them (theseus-0g4).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct LoopEnded {
    pub turn_id: String,
    pub loop_index: u32,
    pub provider_stop_reason: Option<String>,
    pub tool_calls: u32,
    pub advancer: String,
    pub decision: String,
}

/// Which architectural part of the session/turn/loop structure a narrative
/// line comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum NarrativePart {
    Session,
    Turn,
    Loop,
    Context,
    Model,
    Tool,
    Approval,
    Job,
    /// The daemon's config: the vault confirming the copy a start served
    /// from, holding it, or restarting onto a changed note (theseus-2fo).
    Config,
}

impl NarrativePart {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Turn => "turn",
            Self::Loop => "loop",
            Self::Context => "context",
            Self::Model => "model",
            Self::Tool => "tool",
            Self::Approval => "approval",
            Self::Job => "job",
            Self::Config => "config",
        }
    }
}

/// One sentence of the narrative, filled into a fixed template from the
/// structure itself (never written by a model). Kept only in memory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct NarrativeLine {
    /// Counts from 1 since the daemon started; a client merges the tail and
    /// the live lines on it.
    pub seq: u64,
    pub at_unix_ms: u64,
    pub part: NarrativePart,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub turn_id: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct NarrativeWatchResult {
    /// The most recent lines, oldest first; at most `capacity`.
    pub lines: Vec<NarrativeLine>,
    pub capacity: u32,
}

pub fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
