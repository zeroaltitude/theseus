//! Health's own answer, `HealthResult`, and the status structs it is made of: the AWS accounts, the spool, the disk,
//! the web server, the context files, the config's copy, the start's phases, the bindings and places, telemetry, the
//! kernel, its children and the grants. Their other sections (the store, the secrets, the build) are in `health.rs`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    cred, defer, extend, features, google, import, index, is_zero, judge, lsp, mcp, mcp_server,
    memory, notice, resident, rsi, sandbox, thread_cache, voice, warm, AwsBudgetStatus,
    AwsDurabilityStatus, AwsGuardDutyStatus, AwsHandsStatus, BinaryStatus, Build, CancelCount,
    CrashStatus, DayCeilingBudget, GuildInfo, LedgerGap, OutboxStatus, PlaceCeiling, PlacesHealth,
    PromptCacheHealth, PushStatus, SecretsStatus, StoreStatus, TasksHealth, TenderStatus,
    TerminalInfo, TerminalLeft, Tightening, Usage, WakeInfo,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct HealthResult {
    pub name: String,
    pub version: String,
    pub protocol: String,
    /// The binary's version and commit, as `server.started` names them (theseus-9o5n).
    #[serde(default)]
    pub build: Build,
    /// The commit of a self change that is joined to main and is not what this service runs: its
    /// install was not possible (the service runs a build the loop cannot replace) or did not
    /// take. Absent when there is none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub joined_not_running: Option<String>,
    /// The owner's own instruction file, while what is on disk is not the version accepted (or none is
    /// accepted): one line, which names `theseus own-file accept`. Absent otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub own_file: Option<String>,
    /// The context files the settings name, one line each while what is on disk is not the version
    /// accepted (or none is accepted), naming `theseus own-file accept --file PATH`. Empty otherwise.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub held_files: Vec<String>,
    /// The self loop has started nothing for longer than it should, held on its slots (issue 41):
    /// how long, the trees that hold them, and the one act that clears each. Absent otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub self_held: Option<String>,
    pub uptime_secs: u64,
    /// The owner's own sessions: the imported and erased ones are `imported`'s (theseus-revl).
    pub sessions: u64,
    pub turns: u64,
    /// The sessions an import wrote, held apart from `sessions`.
    #[serde(default)]
    pub imported: import::HealthImported,
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
    /// Where the config came from, and what the vault said of the copy this
    /// start served from (theseus-2fo, theseus-zmgb).
    #[serde(default)]
    pub config: ConfigStatus,
    /// The last start's phases, timed from process start: those on the path
    /// to answering the socket, then those after it.
    #[serde(default)]
    pub startup: Vec<StartupPhase>,
    /// Tokens across every session, summed from session records.
    pub usage_total: Usage,
    pub provider_errors: u64,
    /// `[tui] notify`: how the terminal clients ping the owner when their
    /// `--notify` is not given (theseus-f0bi9). Empty from an older daemon.
    #[serde(default)]
    pub tui_notify: String,
    pub ledger_rows: u64,
    /// The ledger chain's head as the writer last committed it, short
    /// (`3fa1c09e…#1204`); absent before the chain starts (theseus-bmkxk).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ledger_head: Option<String>,
    /// The index lags the log, while it does (theseus-lu5n): absent when whole.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ledger_gap: Option<LedgerGap>,
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
    /// Tools left out for want of what they need, each with why (theseus-4o4c).
    #[serde(default)]
    pub tools_not_offered: Vec<String>,
    /// What a job may be handed, and what stays the harness's own: the AWS
    /// and providers' keys (theseus-gh7). Absent from a daemon before it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub harness_only: Option<cred::HarnessOnly>,
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
    /// The tools that ask first because someone pressed "should have asked"
    /// (theseus-sgh), oldest first. They are stored, not configured.
    #[serde(default)]
    pub tightenings: Vec<Tightening>,
    /// The wakes conversations set with `wake.at` that have not run yet,
    /// soonest first (DD8).
    #[serde(default)]
    pub wakes: Vec<WakeInfo>,
    /// The web UI's refusals since the daemon's image started (theseus-70f).
    #[serde(default)]
    pub web: WebStatus,
    /// Free space under the state dir, read when health is asked
    /// (theseus-102).
    #[serde(default)]
    pub disk: DiskStatus,
    /// Whether this daemon's jobs can write the binary it runs, read when
    /// health is asked (review 2's consideration 3).
    #[serde(default)]
    pub binary: BinaryStatus,
    /// The spool's sweeps of raw job output (theseus-2ij).
    #[serde(default)]
    pub spool: SpoolStatus,
    /// The push (theseus-in3): its board, its watchers, and its seed. Absent from a daemon before it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub push: Option<PushStatus>,
    /// The AWS accounts the config binds (AWS design §3.10), each as its check left it; or none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub aws: Option<AwsStatus>,
    /// The Google account `[google]` binds, as its check left it (the Google design §2.2); or none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub google: Option<google::GoogleStatus>,
    /// The index tender (M6 §2.2, roadmap row 51): the tender as the core
    /// supervises it, and its own status when it answers; or none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub index: Option<index::IndexHealth>,
    /// The MCP servers the config attaches (M7 36b). Empty without one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp: Vec<mcp::McpServerStatus>,
    /// Proposed extensions by state (M7 43a). Absent when there are none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub extensions: Option<extend::ExtendHealth>,
    /// The store's refused reads (R4, theseus-15g); zero from a daemon before it.
    #[serde(default)]
    pub store: StoreStatus,
    /// The newest crash a start found (Review 2's consideration 1); or none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub crash: Option<CrashStatus>,
    /// L1 (M4 17b): the class choice's settings, the limits, the last L1
    /// launch since the start, and the jobs by class. Absent without tools.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub sandbox: Option<sandbox::SandboxHealth>,
    /// The judge (M5 23a): `[judge]`, the breaker, and today's calls and spend.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub judge: Option<judge::JudgeHealth>,
    /// The tasks that cannot progress by themselves (M5 28b).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub tasks: Option<TasksHealth>,
    /// Each backend's cancels since the daemon started, by how they ended
    /// (M4 18a). Empty until the first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cancels: Vec<CancelCount>,
    /// The place rule (theseus-nbsh): each place and its class.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub places: Option<PlacesHealth>,
    /// The open terminals (`term.*`, theseus-n88g.4), oldest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub terminals: Vec<TerminalInfo>,
    /// What terminals' closes left running (theseus-ggqf), while it runs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub terminals_left: Vec<TerminalLeft>,
    /// The MCP server (step 41b): absent while `[mcp_server]` is off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub mcp_server: Option<mcp_server::McpServerHealth>,
    /// Recall's mode and arm and its projections (M6 32a, 32b): absent while memory is off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub memory: Option<memory::MemoryHealth>,
    /// The language servers (L2): absent when `[lsp]` is off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub lsp: Option<Vec<lsp::LspServerStatus>>,
    /// The daemon's own memory and its largest caches (theseus-9lxe).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub resident: Option<resident::ResidentHealth>,
    /// Deferred tools: the selector's state in one line (theseus-7m7w.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub tools_defer: Option<defer::DeferHealth>,
    /// Self-improvement's mode, kill switch and gate (theseus-pw1q.2): health's `self` line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub self_improve: Option<rsi::SelfState>,
    /// What a person's first keystroke warmed, and when (theseus-tnky).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub warm: Option<warm::WarmHealth>,
    /// The prompt cache: each profile's TTL, and the keep-warm (theseus-ezeg).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub cache: Option<PromptCacheHealth>,
    /// Threads' keep-warm window and the day's cold-resume numbers (theseus-ytt8).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub threads: Option<thread_cache::ThreadsHealth>,
    /// Which killer features are on (theseus-is8t): health's first line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub features: Option<features::Features>,
    /// Today's model spend against the daily ceiling (theseus-9e02): set
    /// while the day is at or past its ceiling, so health's first line says
    /// so.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub day_spend: Option<DayCeilingBudget>,
}

/// The AWS accounts the config binds (`[aws.accounts.<id>]`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AwsStatus {
    pub accounts: Vec<AwsAccountStatus>,
    /// `[policy.aws]` keys that name no service or operation in the catalog,
    /// so they never match; absent when there are none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unknown_policy_keys: Vec<String>,
}

/// One AWS account: whether its key is bound, and its calls. The key is
/// bound once STS has named this account for it, after serving; until then,
/// and when that fails, no AWS call signs (it fails closed).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AwsAccountStatus {
    /// The account's id.
    pub account: String,
    /// The region a call goes to unless it names one.
    pub region: String,
    /// The regions a call may name.
    pub regions: Vec<String>,
    /// `unchecked` (nothing has asked yet), `waiting` (for its key's
    /// secrets), `checking`, `bound`, or `failed`.
    pub state: String,
    /// Who the key is, as STS answered: `arn:aws:iam::…:user/…`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub arn: Option<String>,
    /// When the check last ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub checked_at_unix_ms: Option<u64>,
    /// Why it is not bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub error: Option<String>,
    /// AWS requests since the daemon started, the check's included, and how
    /// many of them failed.
    pub calls: u64,
    pub failed: u64,
    /// What signs its calls (AWS design §3.5): `its key` until the config
    /// names the owner role the bootstrap made, then `role sessions
    /// (theseus-owner)`, and the key signs only STS.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub signer: Option<String>,
    /// The month's budget, as AWS Budgets said at its last read (§3.7).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub budget: Option<AwsBudgetStatus>,
    /// GuardDuty's cost over its last 30 days, as its weekly read found.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub guardduty: Option<AwsGuardDutyStatus>,
    /// What the budget's reconcile did after serving (config to stack).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub reconcile: Option<String>,
    /// The durability tender (step 15), on this account when it ships.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub durability: Option<AwsDurabilityStatus>,
    /// Its hands, the hour's meter, and the reaper (step 40 part 2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub hands: Option<AwsHandsStatus>,
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
    /// What is wrong with the personas in use (theseus-slvl.2): a file that
    /// cannot be read, so its persona compiles on the built-in default, or a
    /// place that names a persona the config lacks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// Where the config came from, and what the vault said of it (theseus-2fo,
/// spec §3.19). A start whose config is an `op://` reference serves from the
/// last-known-good copy of the note and acts on it at once when its digest is
/// the one the daemon recorded as it wrote it (theseus-zmgb); after serving,
/// it reads the vault once.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ConfigStatus {
    /// `vault` or `file`: where the config lives.
    pub source: String,
    /// The `op://` reference, or the file's path.
    #[serde(default)]
    pub reference: String,
    /// `confirmed` (a file, a note read before serving, or a copy the vault
    /// agrees with), `confirming` (acting on the copy while the vault is
    /// read), `held` (the vault's read did not settle it: it did not answer,
    /// its note does not load, or it changed again since a restart; the
    /// daemon keeps serving the copy, and `detail` says why), or
    /// `restarting` (onto the vault's changed note).
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
    /// Reads of the vault behind the socket: one a start from the copy.
    #[serde(default)]
    pub reads: u32,
    /// This process began as a restart onto the vault's changed note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub restarted: Option<ConfigRestart>,
    /// The settings keys this start read and ignored because they are
    /// retired, by their dotted names (issue 44). Absent when there are none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignored_keys: Vec<String>,
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
    /// The one guild, when the bindings file binds places in one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub guild_id: Option<String>,
    /// Every guild the bindings file binds, with its word (step 38a).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guilds: Vec<GuildInfo>,
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
    /// Its voice (rows 77 and 78), when `[voice]` is on: in `voice.rs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub voice: Option<voice::VoiceStatus>,
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
    /// A channel's guild id (step 38a).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub guild: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub ceiling: Option<PlaceCeiling>,
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
    /// `executions_by_state["waiting"]` by what each waits on (theseus-g00vv). Zeroes from an older daemon.
    #[serde(default, skip_serializing_if = "notice::WaitingSplit::is_empty")]
    pub waiting_split: notice::WaitingSplit,
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
    /// What reaching it does (`[kernel] spend_limit_mode`, theseus-usei):
    /// `notify` or `ask`. Unset from a build before it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub spend_limit_mode: Option<String>,
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
    /// The long-lived children it supervises (roadmap row 51): the index
    /// tender, restarted with backoff whenever it exits. Empty from a daemon
    /// before it, and from one that runs none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tenders: Vec<TenderStatus>,
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
