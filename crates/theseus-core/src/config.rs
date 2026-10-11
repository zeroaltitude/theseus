//! Configuration. A TOML document: a local file, or a 1Password item, so a
//! deployment is reconstructible from the vault, named by its `op://`
//! reference in `--config` or `THESEUS_CONFIG`. Secret fields are
//! `op://vault/item/field` references, never values.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::places::PlacesConfig;
use crate::secrets::{OpReader, SecretRef};

mod aws;
pub mod discord;
mod index;
pub use index::IndexConfig;
mod judge;
mod limits;
mod lookup;
pub(crate) mod lsp;
pub(crate) mod memory;
mod outputs;
pub(crate) mod people;
pub(crate) mod routing;
pub mod rsi;
mod sessions;
pub use sessions::SessionsConfig;
mod sparse;
pub mod term;
mod web;
pub use aws::{
    default_hourly_alert_usd, default_runaway_factor, AwsAccountConfig, AwsConfig,
    AwsCredentialNames, HandsNetwork,
};
pub use judge::{JudgeConfig, JudgePackConfig, PackMode, SignalsConfig};
pub use limits::{default_parallel_runs, MaxLoopsMode, SpendLimitMode};
pub use web::{WebToolsConfig, WEB_TIMEOUT_MAX_SECS};
// Where the config comes from when nothing names one (theseus-5aqz).
pub use lookup::{find_config, Lookup, DEFAULT_CONFIG, NO_CONFIG, SYSTEM_CONFIG};
pub use lsp::{LspConfig, LspServerConfig};
pub use memory::{MemoryConfig, MemoryMode};
pub use routing::{RoutingConfig, RoutingMode};
pub use sparse::{sparse_note, SPARSE_HEADER};
pub mod mcp;
pub mod mcp_server;
pub use mcp::{McpConfig, McpServerConfig};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Narrate every step of the session/turn/loop/model-call structure into
    /// the web UI's The Narrative tab. Absent or false: no line is made and
    /// the tab never shows. First, because a TOML key after a table is that
    /// table's.
    #[serde(default)]
    pub narrative: bool,
    #[serde(default)]
    pub model: ModelConfig,
    /// Named model profiles. The implicit `default` profile is built from `[model]`.
    #[serde(default)]
    pub profiles: BTreeMap<String, ProfileConfig>,
    /// `[context]`: the system level of context files, which every session
    /// gets, and the persona in play (theseus-c48).
    #[serde(default, skip_serializing_if = "ContextConfig::is_empty")]
    pub context: ContextConfig,
    /// `[personas.<name>]`: each persona's context files, added after the
    /// system level's while it is in play (theseus-c48).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub personas: BTreeMap<String, PersonaConfig>,
    /// Additional Anthropic-Messages-compatible endpoints by name (e.g. `zai`).
    /// The implicit `anthropic` provider comes from `[model]` unless overridden here.
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderConfig>,
    /// name → where its value comes from: an `op://` reference into the vault
    /// (recommended), or `env:NAME` or `file:PATH` from outside it
    /// (theseus-n88g.1). They resolve in the background while the daemon
    /// serves; a consumer waits for its own secret and fails closed if it did
    /// not resolve, and a failed one is fetched again (theseus-qa0).
    #[serde(default)]
    pub secrets: BTreeMap<String, String>,
    /// `[broker]`: which program gets which secret, in which variable, and
    /// each secret's posture (theseus-dcy). Empty until a grant is added.
    #[serde(default, skip_serializing_if = "BrokerConfig::is_empty")]
    pub broker: BrokerConfig,
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub github: GitHubConfig,
    #[serde(default)]
    pub web: WebConfig,
    #[serde(default)]
    pub telemetry: crate::telemetry::TelemetryConfig,
    #[serde(default)]
    pub kernel: KernelSection,
    /// `[catalog."<model>"]`, changes to the code's model table on purpose: a
    /// table over a built-in model replaces the fields it names, and a
    /// complete one adds a model. The template holds none (theseus-vwar):
    /// every other model keeps the code's figures, which `theseus catalog`
    /// lists, and a table that copies them changes nothing.
    #[serde(default)]
    pub catalog: BTreeMap<String, crate::catalog::CatalogRow>,
    #[serde(default)]
    pub tools: ToolsConfig,
    #[serde(default)]
    pub policy: PolicyConfig,
    #[serde(default)]
    pub discord: DiscordConfig,
    /// `[voice]` (rows 77 and 78): speech in a voice channel, in `crate::voice`.
    #[serde(default, skip_serializing_if = "crate::voice::VoiceConfig::is_default")]
    pub voice: crate::voice::VoiceConfig,
    /// `[aws.accounts.<id>]`: the AWS accounts Theseus owns (AWS design
    /// §3.5). None: no AWS tool, and nothing AWS runs.
    #[serde(default, skip_serializing_if = "AwsConfig::is_empty")]
    pub aws: AwsConfig,
    /// `[index]`: the index tender (roadmap row 51).
    #[serde(default)]
    pub index: IndexConfig,
    /// `[memory]`: recall (M6 step 30a), off by default, in `config/memory.rs`.
    #[serde(default, skip_serializing_if = "MemoryConfig::is_default")]
    pub memory: MemoryConfig,
    /// `[routing]`: Jev's model per interaction mode (M5 25e), in `config/routing.rs`.
    #[serde(default, skip_serializing_if = "RoutingConfig::is_default")]
    pub routing: RoutingConfig,
    /// `[mcp.servers.<name>]`: the MCP servers whose tools turns are offered
    /// (M7 36b), in `config::mcp`.
    #[serde(default, skip_serializing_if = "McpConfig::is_empty")]
    pub mcp: McpConfig,
    /// `[mcp_server]` (step 41b): Theseus's own MCP server, off by default.
    #[serde(
        default,
        skip_serializing_if = "mcp_server::McpServerConfig::is_default"
    )]
    pub mcp_server: mcp_server::McpServerConfig,
    /// `[lsp]`: the language-server board and its tools (L2), off by default, in `config/lsp.rs`.
    #[serde(default, skip_serializing_if = "LspConfig::is_default")]
    pub lsp: LspConfig,
    /// `[sessions]`: the state windows and the re-title (theseus-emqx).
    #[serde(default, skip_serializing_if = "SessionsConfig::is_default")]
    pub sessions: SessionsConfig,
    /// `[people]`: people proposed from text (theseus-wy7y), `config/people.rs`.
    #[serde(default, skip_serializing_if = "people::PeopleConfig::is_default")]
    pub people: people::PeopleConfig,
    /// `[self]`: self-improvement, off by default (theseus-pw1q), in `config/rsi.rs`.
    #[serde(default, rename = "self")]
    #[serde(skip_serializing_if = "rsi::SelfConfig::is_default")]
    pub self_improve: rsi::SelfConfig,
    /// `[sandbox]`: L1 for `proc.run` (M4 17b), in `crate::sandbox`.
    #[serde(default)]
    pub sandbox: crate::sandbox::SandboxConfig,
    /// `[judge]`: Jev's judgments, in shadow (M5 23a), in `config/judge.rs`.
    #[serde(default)]
    pub judge: JudgeConfig,
    /// `[approval]`, retired (theseus-zmgb): an answer counts only from a private place, by the
    /// owner (the place rule). A config with the section loads, with a warning; nothing reads it.
    #[serde(default, skip_serializing)]
    pub approval: Option<toml::Table>,
    /// `[places]` (the place rule; `[labels]`, its old name, still reads).
    #[serde(default, alias = "labels")]
    #[serde(skip_serializing_if = "PlacesConfig::is_empty")]
    pub places: PlacesConfig,
    /// The 1Password token file this daemon was pointed at (`--op-token-file` or
    /// `THESEUS_OP_TOKEN_FILE`): set at startup, never from the TOML; the floor keeps it (theseus-8az).
    #[serde(skip)]
    pub op_token_file: Option<PathBuf>,
    /// The last-known-good copy of the vault's config note (theseus-2fo): set at startup when the
    /// config is `op://`, never read from the TOML. The floor keeps it, as it keeps the token file.
    #[serde(skip)]
    pub config_copy: Option<PathBuf>,
    /// Every profile and provider, the implicit `default` and `anthropic`
    /// that `[model]` names included: resolved once, when the document is read.
    #[serde(skip)]
    resolved: Resolved,
}

#[derive(Debug, Clone, Default)]
struct Resolved {
    profiles: BTreeMap<String, ProfileConfig>,
    providers: BTreeMap<String, ProviderConfig>,
}

/// `[discord]`: the Discord binding (M3). It connects only when the token
/// secret resolves and the bindings file exists; otherwise health reports it
/// unconfigured and nothing else changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscordConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Name of the `[secrets]` entry holding the bot token.
    #[serde(default = "default_discord_token_secret")]
    pub token_secret: String,
    /// Which Discord places Theseus lives in and who may drive it there (spec P5).
    /// Relative paths resolve in the state dir, beside the store that remembers
    /// each place's session, so a scratch instance never binds by accident.
    #[serde(default = "default_bindings_file")]
    pub bindings_file: String,
    /// How often a streaming reply is edited; Discord rate-limits edits per channel.
    #[serde(default = "default_edit_interval_ms")]
    pub edit_interval_ms: u64,
    /// Post each notified call's own embed, edited with its outcome. Off (the
    /// default), the call's line in the loop's tool message carries the notice;
    /// the ledger row and the web UI's notice are the same either way.
    #[serde(default)]
    pub notice_embeds: bool,
    /// The kinds of message that post silent (theseus-l1y1): none by default.
    #[serde(default)]
    pub silent: Vec<discord::Category>,
    /// At most one ping per place in this many seconds; 0, the default, is off.
    #[serde(default)]
    pub ping_window_secs: u64,
    /// A local stand-in for Discord's REST API, `host:port` over plain http
    /// (twilight's proxy base): tests and scratch daemons only, so that no
    /// request and no token leaves the machine (theseus-q4v).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rest_proxy: Option<String>,
    /// The same for the gateway: a `ws://host:port` URL connected instead of
    /// Discord's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateway_proxy: Option<String>,
}

fn default_discord_token_secret() -> String {
    "discord_bot_token".into()
}
fn default_bindings_file() -> String {
    "bindings.toml".into()
}
fn default_edit_interval_ms() -> u64 {
    1200
}

/// The warning a config with `[approval]` loads with (theseus-zmgb).
pub const APPROVAL_RETIRED: &str = "[approval] is retired, and nothing reads it: an answer counts \
     only from a private place (the CLI, the web UI, a DM with the owner, or a channel bound \
     private = true), by the owner ([places] owner, else the person of a DM the bindings file \
     binds). Delete the section";

impl DiscordConfig {
    pub fn bindings_path(&self, state_dir: &std::path::Path) -> PathBuf {
        let p = expand(&self.bindings_file);
        if p.is_absolute() {
            p
        } else {
            state_dir.join(p)
        }
    }
}

/// `[tools]`: where toollets may work and how much they may return (§3.23).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolsConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// The operator's projects directory: the workspace tools work in, and
    /// where relative paths resolve. No built-in default; without it (and
    /// without `roots`) every path is outside the workspace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projects_dir: Option<String>,
    /// More workspace roots beside `projects_dir`. A read or a program run
    /// outside every root waits for approval; a write there takes its tool's
    /// posture (theseus-ewi).
    #[serde(default)]
    pub roots: Vec<String>,
    /// Where relative paths resolve and programs run by default (default: `projects_dir`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Paths that wait for approval, even under a root.
    #[serde(default = "default_approve_paths")]
    pub approve_paths: Vec<String>,
    /// Characters of a tool result the model sees (head and tail kept).
    #[serde(default = "default_result_max_chars")]
    pub result_max_chars: usize,
    #[serde(default = "default_max_read_bytes")]
    pub max_read_bytes: usize,
    /// The largest file a surface accepts whole (theseus-c9l6): a Discord
    /// attachment, `theseus ask --attach`. It is kept in the store's blobs,
    /// and a PDF is read for the model. The default, 32 MiB, is the
    /// provider's limit on a request that carries a PDF.
    #[serde(default = "default_max_attachment_bytes")]
    pub max_attachment_bytes: u64,
    /// The most of one recording `file.read` hears (theseus-c9l6): Deepgram
    /// bills by the minute (about $0.26 an hour on nova-3), and a longer
    /// recording is heard to here, which its transcript says.
    #[serde(default = "default_transcribe_max_minutes")]
    pub transcribe_max_minutes: u64,
    /// Entries a listing, glob, or grep returns.
    #[serde(default = "default_max_entries")]
    pub max_entries: usize,
    /// How long a turn waits for `proc.run` before the job continues in the background.
    #[serde(default = "default_proc_sync_secs")]
    pub proc_sync_secs: u64,
    #[serde(default = "default_parallel_runs")]
    pub parallel_runs: usize,
    #[serde(default = "default_proc_timeout_secs")]
    pub proc_timeout_secs: u64,
    #[serde(default = "default_proc_timeout_max_secs")]
    pub proc_timeout_max_secs: u64,
    /// Environment variables `proc.run` passes through from the daemon (nothing else).
    #[serde(default = "default_proc_env")]
    pub proc_env: Vec<String>,
    /// The most a job's raw output file keeps (theseus-102). Past it the job runs on, what it prints is counted
    /// and dropped, and its result says so. The runtime reads only the file's last 4 MiB.
    #[serde(default = "default_job_output_max_bytes")]
    pub job_output_max_bytes: u64,
    /// A capped result's whole output, kept in `<state>/outputs/` (theseus-v73m),
    /// is deleted after this many days, at its session's retirement, or,
    /// oldest first, while all of them pass `outputs_max_bytes`.
    #[serde(default = "outputs::keep_days")]
    pub outputs_keep_days: u64,
    #[serde(default = "outputs::max_bytes")]
    pub outputs_max_bytes: u64,
    /// Each L0 job's cgroup's `pids.max` (theseus-a5nv), where the daemon's
    /// cgroup is delegated: its processes and threads at once. 0: no cap.
    #[serde(default = "default_job_pids_max")]
    pub job_pids_max: u64,
    /// `[tools.web]`: `http.fetch` and `web.search` (DD5).
    #[serde(default)]
    pub web: WebToolsConfig,
    #[serde(default)]
    pub term: term::TermToolsConfig,
}

fn default_approve_paths() -> Vec<String> {
    [
        "~/.ssh",
        "~/.gnupg",
        "~/.aws",
        "~/.config/op",
        "~/.openclaw-1password-service-token",
        "~/.theseus",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}
fn default_result_max_chars() -> usize {
    30_000
}
fn default_max_read_bytes() -> usize {
    262_144
}
fn default_max_attachment_bytes() -> u64 {
    theseus_files::MAX_FILE_BYTES
}
fn default_transcribe_max_minutes() -> u64 {
    60
}
fn default_max_entries() -> usize {
    500
}
fn default_proc_sync_secs() -> u64 {
    60
}
fn default_proc_timeout_secs() -> u64 {
    600
}
fn default_proc_timeout_max_secs() -> u64 {
    3600
}
fn default_job_output_max_bytes() -> u64 {
    theseus_kernel::job::DEFAULT_OUTPUT_MAX_BYTES
}
fn default_job_pids_max() -> u64 {
    theseus_kernel::cgroup::DEFAULT_PIDS_MAX
}
/// The smallest cap a job's output file may have: a page.
const MIN_JOB_OUTPUT_MAX_BYTES: u64 = 4096;
fn default_proc_env() -> Vec<String> {
    [
        "PATH",
        "HOME",
        "USER",
        "LANG",
        "LC_ALL",
        "TERM",
        "TZ",
        "CARGO_HOME",
        "RUSTUP_HOME",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            projects_dir: None,
            roots: vec![],
            cwd: None,
            approve_paths: default_approve_paths(),
            result_max_chars: default_result_max_chars(),
            max_read_bytes: default_max_read_bytes(),
            max_attachment_bytes: default_max_attachment_bytes(),
            transcribe_max_minutes: default_transcribe_max_minutes(),
            max_entries: default_max_entries(),
            proc_sync_secs: default_proc_sync_secs(),
            parallel_runs: default_parallel_runs(),
            proc_timeout_secs: default_proc_timeout_secs(),
            proc_timeout_max_secs: default_proc_timeout_max_secs(),
            proc_env: default_proc_env(),
            job_output_max_bytes: default_job_output_max_bytes(),
            job_pids_max: default_job_pids_max(),
            outputs_keep_days: outputs::keep_days(),
            outputs_max_bytes: outputs::max_bytes(),
            web: WebToolsConfig::default(),
            term: term::TermToolsConfig::default(),
        }
    }
}

/// `[broker]` (theseus-dcy): the secret broker's grants, and each secret's
/// posture.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrokerConfig {
    /// `[broker.programs.<program>]`: what a program gets when a job runs it
    /// by its own argv.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub programs: BTreeMap<String, ProgramGrant>,
    /// `[broker.secrets.<name>]`: a secret's posture.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub secrets: BTreeMap<String, SecretPosture>,
}

impl BrokerConfig {
    pub fn is_empty(&self) -> bool {
        self.programs.is_empty() && self.secrets.is_empty()
    }
}

/// One program's grant: each environment variable it gets, with the name of
/// the `[secrets]` entry whose value it holds; or, for a program that calls
/// AWS (`aws`), an account whose short-lived job session it gets at launch,
/// under the guards, and never the key (AWS design §3.5).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramGrant {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aws_account: Option<String>,
}

/// A secret's posture: a call given the secret runs at no looser a posture.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretPosture {
    #[serde(default = "default_secret_posture")]
    pub posture: crate::policy::Posture,
}

fn default_secret_posture() -> crate::policy::Posture {
    crate::policy::Posture::Notify
}

/// `[policy]`: the posture every tool and MCP inherits, the per-tool and
/// per-MCP exceptions, and the operator's explicit argv lists (§3.9; see
/// `policy` for the order they apply in).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyConfig {
    /// The posture every tool and MCP inherits: open | notify | approve.
    #[serde(default)]
    pub enforcement: crate::policy::Posture,
    /// `proc.run` argv prefixes that run (open) when every path argument is
    /// inside the roots. A prefix covers any arguments after it.
    #[serde(default = "default_allow_argv")]
    pub allow_argv: Vec<Vec<String>>,
    /// `proc.run` argv prefixes that wait for approval.
    #[serde(default = "default_approve_argv")]
    pub approve_argv: Vec<Vec<String>>,
    /// Per-tool postures by canonical name, e.g. `"proc.run" = "approve"`.
    #[serde(default)]
    pub tools: BTreeMap<String, crate::policy::Posture>,
    /// Per-MCP postures: `"server"` (every tool from that server) or
    /// `"server/tool"` (one tool), for tools named `mcp:<server>/<tool>`.
    #[serde(default)]
    pub mcp: BTreeMap<String, crate::policy::Posture>,
    /// After a session reads external text (theseus-9bp): `ask`, the default,
    /// makes a call that acts wait for approval; `notify` runs it with a
    /// notice at least. Either holds until the operator trusts the session.
    #[serde(default)]
    pub external_text: crate::external::Mode,
    /// Programs whose output is outside text (theseus-b5cl): a `proc.run` of
    /// one holds its session as a fetch does (`external::Listed`). `["gh"]`
    /// by default: `gh issue view` prints a stranger's text.
    #[serde(default = "default_external_programs")]
    pub external_programs: Vec<String>,
    /// A URL whose host is a private address (theseus-7gir.20): `ask`, the
    /// default, waits for approval (DD5); `open` judges it as any other, for a
    /// run with no operator to ask (the bench profile).
    #[serde(default)]
    pub private_addresses: crate::web::net::PrivateAddresses,
    /// `[policy.aws]` (AWS design §3.9): an AWS call's posture when its tool
    /// has no `[policy.tools]` line: `"service:Operation"`, then `"service"`,
    /// then its class's (`read`), then `enforcement`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub aws: BTreeMap<String, crate::policy::Posture>,
}

fn argvs(v: &[&[&str]]) -> Vec<Vec<String>> {
    v.iter()
        .map(|a| a.iter().map(|s| s.to_string()).collect())
        .collect()
}
fn default_allow_argv() -> Vec<Vec<String>> {
    // No git: its config and attributes can run programs (fsmonitor, textconv,
    // external diff), and the native git.diff / git.log toollets read history
    // without the git binary.
    argvs(&[&["ls"], &["pwd"]])
}
fn default_approve_argv() -> Vec<Vec<String>> {
    // No `op` or `theseusd`: the floor asks for those whatever the lists say.
    argvs(&[
        &["sudo"],
        &["su"],
        &["doas"],
        &["rm", "-rf", "/"],
        &["mkfs"],
        &["dd"],
        &["shutdown"],
        &["reboot"],
    ])
}
fn default_external_programs() -> Vec<String> {
    vec!["gh".into()]
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            enforcement: Default::default(),
            allow_argv: default_allow_argv(),
            approve_argv: default_approve_argv(),
            tools: BTreeMap::new(),
            mcp: BTreeMap::new(),
            external_text: Default::default(),
            external_programs: default_external_programs(),
            private_addresses: Default::default(),
            aws: BTreeMap::new(),
        }
    }
}

/// `[kernel]`: the durable kernel's knobs (spec §3.2a, §3.15, §3.16; M2).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KernelSection {
    /// How many executions may hold a turn at once.
    #[serde(default = "default_admission_ceiling")]
    pub admission_ceiling: u32,
    /// How long admission may go without admitting any turn before a
    /// waiting turn fails (theseus-4g0l): a queue that moves fails none.
    #[serde(default = "default_admission_stuck_secs")]
    pub admission_stuck_secs: u64,
    /// Each session's spend limit in US dollars (theseus-0sg). What reaching
    /// it does is `spend_limit_mode`: a notice, or the question (theseus-usei).
    #[serde(default = "limits::default_spend_limit_usd")]
    pub spend_limit_usd: f64,
    #[serde(default, skip_serializing_if = "SpendLimitMode::is_default")]
    pub spend_limit_mode: SpendLimitMode,
    /// The daemon's day ceiling over every model call (theseus-kp20).
    #[serde(default = "limits::default_daily_ceiling_usd")]
    pub daily_spend_ceiling_usd: f64,
    /// Retired (theseus-0sg): budget units, and the units kept back as a
    /// control reserve. Both still load, so an older config starts, and are
    /// never honored: budgets are dollars, and nothing reserves for control
    /// (a cancel never spends, and the limit asks instead of ending).
    #[serde(default, skip_serializing)]
    pub default_budget: Option<toml::Value>,
    #[serde(default, skip_serializing)]
    pub control_reserve: Option<toml::Value>,
    /// Heartbeat reconciler cadence.
    #[serde(default = "default_heartbeat_secs")]
    pub heartbeat_secs: u64,
    /// Deadline for an action whose tool declares none.
    #[serde(default = "default_deadline_secs")]
    pub default_deadline_secs: u64,
    /// How long a call's question waits for an answer before it expires,
    /// not run (theseus-830), and how long an approval stays valid.
    #[serde(default = "default_confirm_ttl_secs")]
    pub confirm_ttl_secs: u64,
    /// The shortest span a repeating wake may take, in minutes (37a): a
    /// floor on a runaway series' turns. At least 1.
    #[serde(default = "default_min_repeat_minutes")]
    pub min_repeat_minutes: u64,
    /// A task claim's lease, in minutes (39b): the holder's edits renew it.
    /// At least 1.
    #[serde(default = "default_task_lease_minutes")]
    pub task_lease_minutes: u64,
}

fn default_admission_ceiling() -> u32 {
    8
}
fn default_admission_stuck_secs() -> u64 {
    600
}
fn default_heartbeat_secs() -> u64 {
    60
}
fn default_deadline_secs() -> u64 {
    600
}
fn default_confirm_ttl_secs() -> u64 {
    900
}
fn default_min_repeat_minutes() -> u64 {
    5
}
fn default_task_lease_minutes() -> u64 {
    30
}

impl Default for KernelSection {
    fn default() -> Self {
        Self {
            admission_ceiling: default_admission_ceiling(),
            admission_stuck_secs: default_admission_stuck_secs(),
            spend_limit_usd: limits::default_spend_limit_usd(),
            spend_limit_mode: SpendLimitMode::default(),
            daily_spend_ceiling_usd: limits::default_daily_ceiling_usd(),
            default_budget: None,
            control_reserve: None,
            heartbeat_secs: default_heartbeat_secs(),
            default_deadline_secs: default_deadline_secs(),
            confirm_ttl_secs: default_confirm_ttl_secs(),
            min_repeat_minutes: default_min_repeat_minutes(),
            task_lease_minutes: default_task_lease_minutes(),
        }
    }
}

impl KernelSection {
    /// The retired unit keys this config still sets.
    pub fn retired(&self) -> Vec<&'static str> {
        [
            ("default_budget", self.default_budget.is_some()),
            ("control_reserve", self.control_reserve.is_some()),
        ]
        .into_iter()
        .filter_map(|(k, set)| set.then_some(k))
        .collect()
    }

    /// The admission queue's stuck window: at least a second.
    pub fn admission_stuck(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.admission_stuck_secs.max(1))
    }

    /// A task claim's lease, in milliseconds: at least a minute.
    pub fn task_lease_ms(&self) -> u64 {
        self.task_lease_minutes.max(1) * 60_000
    }

    pub fn to_kernel_config(&self) -> theseus_kernel::KernelConfig {
        theseus_kernel::KernelConfig {
            admission_ceiling: self.admission_ceiling.max(1),
            default_deadline_ms: self.default_deadline_secs * 1000,
            spend_limit_micros: theseus_kernel::usd_to_micros(self.spend_limit_usd),
            spend_limit_notify: self.spend_limit_mode == SpendLimitMode::Notify,
            confirm_ttl_ms: self.confirm_ttl_secs * 1000,
            heartbeat_ms: self.heartbeat_secs.max(1) * 1000,
            fault_after_startup_step: None,
            min_repeat_ms: self.min_repeat_minutes.max(1) * 60_000,
            daily_ceiling_micros: theseus_kernel::usd_to_micros(self.daily_spend_ceiling_usd),
            // The system's zone.
            ..Default::default()
        }
    }
}

/// The localhost web UI. Bound to loopback only; no auth yet (spec §3.14).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// A loopback address (`127.0.0.1` or `::1`): anything else fails to
    /// load until the web UI has auth (theseus-2fo).
    #[serde(default = "default_web_bind")]
    pub bind: String,
    #[serde(default = "default_web_port")]
    pub port: u16,
    /// For UI development only (theseus-zab): the Vite dev page's origin
    /// (`npm run dev` in `cockpit/`), such as `http://127.0.0.1:5174`. The
    /// cockpit's page opens `/ws` straight, with that `Origin`, which the UI
    /// otherwise refuses; a dev server's proxy would pass the page's `Host`
    /// too. While set, a `/ws` upgrade from exactly this
    /// origin is served too, counted in health, and ledgered
    /// (`web.dev_origin`); every other route still answers only the UI's own
    /// address. Off by default. `http://`, `localhost` or a loopback address,
    /// and a port, else the config fails to load.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dev_origin: Option<String>,
}

fn default_true() -> bool {
    true
}
fn default_web_bind() -> String {
    "127.0.0.1".into()
}
fn default_web_port() -> u16 {
    7433
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            bind: default_web_bind(),
            port: default_web_port(),
            dev_origin: None,
        }
    }
}

/// A dev page's origin as a browser sends it, on this machine
/// (theseus-zab): `http://`, then `localhost` or a loopback address (an
/// IPv6 one in brackets), then a port. Nothing else: no path, no user, no
/// other host.
pub fn dev_origin_ok(origin: &str) -> bool {
    let Some(authority) = origin.strip_prefix("http://") else {
        return false;
    };
    let (host, port) = match authority.strip_prefix('[') {
        Some(rest) => match rest.split_once("]:") {
            Some((host, port)) => (host, port),
            None => return false,
        },
        None => match authority.rsplit_once(':') {
            Some((host, _)) if host.contains(':') => return false,
            Some(split) => split,
            None => return false,
        },
    };
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    loopback && port.bytes().all(|b| b.is_ascii_digit()) && port.parse::<u16>().is_ok_and(|p| p > 0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubConfig {
    /// Name of the entry in `[secrets]` holding the GitHub token; checked at startup.
    #[serde(default = "default_github_secret")]
    pub token_secret: String,
    /// Warn when the token expires within this many days.
    #[serde(default = "default_warn_days")]
    pub warn_days: i64,
}

fn default_github_secret() -> String {
    "github_token".into()
}
fn default_warn_days() -> i64 {
    30
}

impl Default for GitHubConfig {
    fn default() -> Self {
        Self {
            token_secret: default_github_secret(),
            warn_days: default_warn_days(),
        }
    }
}

/// A model endpoint speaking the Anthropic Messages API. The first-party API
/// is one; Z.ai's GLM series exposes the same protocol at another URL, so a
/// second provider is a table entry, not code.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    pub api_base: String,
    /// Name of the entry in `[secrets]` holding this provider's key.
    pub api_key_secret: String,
    #[serde(default)]
    pub kind: ProviderKind,
    #[serde(default)]
    pub timeouts: Option<crate::provider::Timeouts>,
}

/// A provider's wire protocol. Only the Anthropic Messages API exists today.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    #[default]
    AnthropicMessages,
}

/// A named way of running turns: which provider, which model, how long an
/// answer may be, and what system prompt. Exactly one profile is **live** at
/// a time; the live one can be switched over the protocol and the switch
/// persists. Turns may name a profile explicitly; later, sessions and tasks
/// will carry their own override.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProfileConfig {
    #[serde(default = "default_provider_name")]
    pub provider: String,
    pub model: String,
    /// Cap on tokens the model may *generate* per call (the Messages API's
    /// `max_tokens`). Not an input limit; input is whatever the compiler
    /// assembles. Omitted: the model's ceiling from the catalog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(default)]
    pub system: Option<String>,
    /// For models that accept effort. Omitted: the model's default (Opus 5.5:
    /// medium; others: high).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<Effort>,
    /// `effort` stands whatever Jev's route.v3 answers (theseus-qe3v).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub effort_fixed: bool,
    #[serde(default)]
    pub thinking_display: ThinkingDisplay,
    /// Tool loops per turn before a notice, or the turn's end: `max_loops_mode`.
    #[serde(default = "default_max_loops")]
    pub max_loops: u32,
    #[serde(default, skip_serializing_if = "MaxLoopsMode::is_default")]
    pub max_loops_mode: MaxLoopsMode,
    /// Server-side refusal fallbacks where the model supports them.
    #[serde(default = "default_true")]
    pub refusal_fallbacks: bool,
    /// How long the provider keeps the profile's cached prefixes (theseus-ev1).
    #[serde(default, skip_serializing_if = "CacheTtl::is_default")]
    pub cache_ttl: CacheTtl,
}

/// A prompt cache entry's lifetime (theseus-ev1). Each read restarts it.
/// `1h` writes cost 2 × input against 1.25 × for `5m`, and pay when the
/// prefix is read again after a pause of 5 to 60 minutes. It applies to
/// every breakpoint of a conversation's requests, the automatic one
/// included; a task's own conversation keeps `5m` (its loops run seconds
/// apart), after the header's `1h` entries, as the order rule asks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CacheTtl {
    #[default]
    #[serde(rename = "5m")]
    FiveMinutes,
    #[serde(rename = "1h")]
    OneHour,
}

impl CacheTtl {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::FiveMinutes => "5m",
            Self::OneHour => "1h",
        }
    }

    /// The `cache_control` marker. Five minutes is the API's default, so its
    /// marker names no `ttl`: the bytes every request carried before 13c.
    pub fn marker(self) -> serde_json::Value {
        match self {
            Self::FiveMinutes => serde_json::json!({"type": "ephemeral"}),
            Self::OneHour => serde_json::json!({"type": "ephemeral", "ttl": "1h"}),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

/// What thinking blocks carry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThinkingDisplay {
    #[default]
    Summarized,
    Omitted,
    Updates,
}

fn default_max_loops() -> u32 {
    40
}

impl ProfileConfig {
    /// The output cap in force: configured, else the catalog ceiling, else 16384.
    pub fn effective_max_tokens(&self, catalog: &crate::catalog::Catalog) -> u32 {
        self.max_output_tokens
            .or_else(|| catalog.get(&self.model).map(|e| e.max_output_tokens))
            .unwrap_or(16_384)
    }
}

/// `[context]` (theseus-c48): context files in two levels. The system
/// level, `files`, is every session's; the files of the persona in play come
/// after it. Each is compiled into the system block under a header naming it
/// and its level (spec §4.4). Paths are `~/` or absolute; nothing is read at
/// load.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextConfig {
    /// The system level: every session's files, before any persona's.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<crate::context_files::ContextEntry>,
    /// The persona in play, a key of `[personas]`. Until Jev chooses one
    /// from its ontology of personas (theseus-8kk, theseus-0j2), this is the
    /// only choice. Absent: no persona, and the system level alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_persona: Option<String>,
}

impl ContextConfig {
    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.default_persona.is_none()
    }
}

/// `[personas.<name>]` (theseus-c48): a persona's context files, compiled
/// after the system level's while the persona is in play.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersonaConfig {
    #[serde(default)]
    pub files: Vec<crate::context_files::ContextEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
    /// Name of the profile that is live at startup (a key of `[profiles]`,
    /// or "default" for the implicit profile built from this section).
    /// A persisted runtime switch (`profile.use`) takes precedence.
    #[serde(default = "default_profile_name")]
    pub live: String,
    /// Default provider name: a key of `[providers]`, or "anthropic" for the implicit one.
    #[serde(default = "default_provider_name")]
    pub provider: String,
    #[serde(default = "default_model")]
    pub model: String,
    /// Output cap per call for the implicit default profile; see `ProfileConfig`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(default)]
    pub system: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<Effort>,
    #[serde(default)]
    pub thinking_display: ThinkingDisplay,
    #[serde(default = "default_max_loops")]
    pub max_loops: u32,
    #[serde(default, skip_serializing_if = "MaxLoopsMode::is_default")]
    pub max_loops_mode: MaxLoopsMode,
    #[serde(default = "default_true")]
    pub refusal_fallbacks: bool,
    #[serde(default, skip_serializing_if = "CacheTtl::is_default")]
    pub cache_ttl: CacheTtl,
    #[serde(default = "default_api_base")]
    pub api_base: String,
    /// Name of the entry in `[secrets]` holding the Anthropic key.
    #[serde(default = "default_key_name")]
    pub api_key_secret: String,
    #[serde(default)]
    pub timeouts: crate::provider::Timeouts,
    #[serde(default)]
    pub retries: Retries,
}

/// `[model.retries]` (theseus-7gir.21): a call that fails with a class that
/// passes with time is made again inside its turn, after a backoff that
/// doubles each time, at most `transient` times. 0, the default: none; the
/// turn fails and the session's driver retries it later (theseus-ljr), as an
/// operator's daemon does. For a headless run, which ends with its turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Retries {
    #[serde(default)]
    pub transient: u32,
    /// The wait before the first retry.
    #[serde(default = "default_backoff_ms")]
    pub backoff_ms: u64,
    /// The longest one wait.
    #[serde(default = "default_backoff_max_ms")]
    pub backoff_max_ms: u64,
    /// A refused request is made once more on its model's fallback, the
    /// catalog's `refusal_fallback_model` (theseus-7gir.18). On by default.
    #[serde(default = "default_true")]
    pub refusal: bool,
}

fn default_backoff_ms() -> u64 {
    2_000
}
fn default_backoff_max_ms() -> u64 {
    30_000
}

impl Default for Retries {
    fn default() -> Self {
        Self {
            transient: 0,
            backoff_ms: default_backoff_ms(),
            backoff_max_ms: default_backoff_max_ms(),
            refusal: true,
        }
    }
}

impl Retries {
    /// The wait before retry `n`, the first being 0.
    pub fn wait(&self, n: u32) -> std::time::Duration {
        let doubled = self.backoff_ms.saturating_mul(1 << n.min(30));
        std::time::Duration::from_millis(doubled.min(self.backoff_max_ms))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    #[serde(default = "default_state_dir")]
    pub state_dir: String,
    #[serde(default = "default_socket")]
    pub socket: String,
    /// Index engine behind the WAL: "redb", the only one (theseus-0g4). Still
    /// accepted so a config that names it loads; any other value is refused.
    #[serde(default)]
    pub store_engine: theseus_store::Engine,
    /// How long a clean stop waits, from its start, for the posts it has
    /// already sent to settle (theseus-pfv). §9 holds a clean stop to 100 ms.
    /// A post still in flight then stays dispatched, and the next start sends
    /// it again under the same nonce. 0 waits for none.
    #[serde(default = "default_stop_grace_ms")]
    pub stop_grace_ms: u64,
    /// Health warns when the filesystem under the state dir has less free
    /// space than this, in MB (theseus-102). 0 never warns.
    #[serde(default = "default_disk_warn_mb")]
    pub disk_warn_mb: u64,
    /// A job is refused, with its reason, when the filesystem under the state
    /// dir has less free space than this, in MB (theseus-102), so the store
    /// keeps room to write. 0 refuses none.
    #[serde(default = "default_disk_floor_mb")]
    pub disk_floor_mb: u64,
    #[serde(default)]
    pub max_connections: u64,
}

fn default_stop_grace_ms() -> u64 {
    50
}

/// Five floors: health says the disk is low while there is still room to
/// act before jobs are refused. One build's target dir can take several GB.
fn default_disk_warn_mb() -> u64 {
    5 * 1024
}

/// Room, once new jobs are refused, for what already runs to finish and be
/// written down: eight turns at once (the admission ceiling), each with a
/// job at its 64 MiB output cap, is 512 MiB, beside a whole 64 MiB WAL
/// segment and the index's checkpoint. It protects the store, whose appends
/// fail on a full disk, the rows that would say so among them.
fn default_disk_floor_mb() -> u64 {
    1024
}

/// The longest grace a clean stop may give the posts in flight.
const MAX_STOP_GRACE_MS: u64 = 10_000;

fn default_model() -> String {
    "claude-sonnet-5-5".into()
}
fn default_provider_name() -> String {
    "anthropic".into()
}
fn default_profile_name() -> String {
    "default".into()
}
fn default_api_base() -> String {
    "https://api.anthropic.com".into()
}
fn default_key_name() -> String {
    "anthropic_api_key".into()
}
/// Where state lives unless `[server].state_dir` or `--state-dir` says
/// otherwise; the config copy is found here before any config is read.
pub const DEFAULT_STATE_DIR: &str = "~/.theseus";

fn default_state_dir() -> String {
    DEFAULT_STATE_DIR.into()
}
fn default_socket() -> String {
    "~/.theseus/theseus.sock".into()
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            live: default_profile_name(),
            provider: default_provider_name(),
            model: default_model(),
            max_output_tokens: None,
            system: None,
            effort: None,
            thinking_display: ThinkingDisplay::Summarized,
            max_loops: default_max_loops(),
            max_loops_mode: MaxLoopsMode::default(),
            refusal_fallbacks: true,
            cache_ttl: CacheTtl::default(),
            api_base: default_api_base(),
            api_key_secret: default_key_name(),
            timeouts: Default::default(),
            retries: Default::default(),
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            state_dir: default_state_dir(),
            socket: default_socket(),
            store_engine: theseus_store::Engine::Redb,
            stop_grace_ms: default_stop_grace_ms(),
            disk_warn_mb: default_disk_warn_mb(),
            disk_floor_mb: default_disk_floor_mb(),
            max_connections: 0,
        }
    }
}

impl Config {
    /// `source` is either an `op://` reference or a filesystem path.
    pub async fn load(source: &str, op: &OpReader) -> Result<Self> {
        Ok(Self::load_text(source, op).await?.0)
    }

    /// `load`, and the document's text as it was read: the vault's note is
    /// kept as the last-known-good copy (theseus-2fo).
    pub async fn load_text(source: &str, op: &OpReader) -> Result<(Self, String)> {
        let text = if source.starts_with("op://") {
            let r = SecretRef::parse(source)?;
            let secret = op
                .read(&r)
                .await
                .with_context(|| format!("reading config from {source}"))?;
            secret.expose().to_string()
        } else {
            let path = expand(source);
            std::fs::read_to_string(&path)
                .with_context(|| format!("reading config file {}", path.display()))?
        };
        let (cfg, warnings) = Self::parse(&text)?;
        for w in &warnings {
            tracing::warn!("{w}");
        }
        Ok((cfg, text))
    }

    /// A field set in this config that could carry a credential, by name
    /// (theseus-2fo): a URL with a user, a password, or a query. The loader
    /// refuses a value in `[secrets]`; every other field is a reference, a
    /// name, an id, a number, a path, a posture, an argv prefix, or prose for
    /// the model. A note with such a URL is never kept as a copy.
    pub fn credential_in_url(&self) -> Option<String> {
        let mut urls = vec![("model.api_base".to_string(), self.model.api_base.as_str())];
        for (name, p) in &self.providers {
            urls.push((format!("providers.{name}.api_base"), p.api_base.as_str()));
        }
        if let Some(e) = self.telemetry.endpoint() {
            urls.push(("telemetry.otlp_endpoint".to_string(), e));
        }
        urls.into_iter()
            .find(|(_, u)| match reqwest::Url::parse(u) {
                Ok(u) => !u.username().is_empty() || u.password().is_some() || u.query().is_some(),
                Err(_) => u.contains('@') || u.contains('?'),
            })
            .map(|(key, _)| key)
    }

    /// Parse and validate a config document. The warnings name the retired
    /// keys it still uses; the caller logs them at startup.
    pub fn parse(text: &str) -> Result<(Self, Vec<String>)> {
        let cfg = Self::from_toml(text).context("parsing config TOML")?;
        cfg.validate()?;
        let mut warnings = Vec::new();
        // One warning for the unit budget however many of its keys remain:
        // they retire together, and the fix is one edit.
        let units = cfg.kernel.retired();
        if !units.is_empty() {
            warnings.push(format!(
                "{} {} budget units, retired and ignored (theseus-0sg): budgets are dollars, \
                 and each session's limit is kernel.spend_limit_usd (now ${}); remove {}",
                units
                    .iter()
                    .map(|k| format!("kernel.{k}"))
                    .collect::<Vec<_>>()
                    .join(" and "),
                if units.len() == 1 { "is" } else { "are" },
                cfg.kernel.spend_limit_usd,
                if units.len() == 1 { "it" } else { "them" },
            ));
        }
        warnings.extend(cfg.sandbox.retired());
        if cfg.context.default_persona.is_none() && !cfg.personas.is_empty() {
            warnings.push(format!(
                "[personas] defines {}, but context.default_persona names none of them, so no \
                 persona's files are compiled: until Jev chooses a persona (theseus-8kk), \
                 context.default_persona is the only choice",
                cfg.persona_names()
            ));
        }
        if cfg.approval.is_some() {
            warnings.push(APPROVAL_RETIRED.into());
        }
        Ok((cfg, warnings))
    }

    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
    pub fn validate(&self) -> Result<()> {
        for (name, r) in &self.secrets {
            if crate::secrets::is_local(r) {
                crate::secrets::check_local(r).with_context(|| format!("secrets.{name}"))?;
                continue;
            }
            SecretRef::parse(r).with_context(|| {
                format!("secrets.{name} is not a valid op:// reference, env:NAME, or file:PATH")
            })?;
        }
        if !self.secrets.contains_key(&self.model.api_key_secret) {
            anyhow::bail!(
                "model.api_key_secret = {:?} has no matching entry under [secrets]",
                self.model.api_key_secret
            );
        }
        for (name, p) in &self.providers {
            if !self.secrets.contains_key(&p.api_key_secret) {
                anyhow::bail!(
                    "providers.{name}.api_key_secret = {:?} has no matching entry under [secrets]",
                    p.api_key_secret
                );
            }
        }
        if !self.all_providers().contains_key(&self.model.provider) {
            anyhow::bail!(
                "model.provider = {:?} is not the implicit \"anthropic\" provider nor a key of [providers]",
                self.model.provider
            );
        }
        if self.server.stop_grace_ms > MAX_STOP_GRACE_MS {
            anyhow::bail!(
                "server.stop_grace_ms = {} is over {MAX_STOP_GRACE_MS}: a clean stop waits at most \
                 10 s for the posts in flight",
                self.server.stop_grace_ms
            );
        }
        if self.server.disk_warn_mb > 0 && self.server.disk_floor_mb > self.server.disk_warn_mb {
            anyhow::bail!(
                "server.disk_floor_mb = {} is above server.disk_warn_mb = {}: health would never \
                 warn before jobs are refused",
                self.server.disk_floor_mb,
                self.server.disk_warn_mb
            );
        }
        if self.tools.job_output_max_bytes < MIN_JOB_OUTPUT_MAX_BYTES {
            anyhow::bail!(
                "tools.job_output_max_bytes = {} is under {MIN_JOB_OUTPUT_MAX_BYTES}: a job's output \
                 file keeps at least a page",
                self.tools.job_output_max_bytes
            );
        }
        self.sandbox.validate()?;
        self.judge.validate(&self.secrets)?;
        let providers = self.all_providers();
        for (name, prof) in &self.profiles {
            if !providers.contains_key(&prof.provider) {
                anyhow::bail!(
                    "profiles.{name}.provider = {:?} is not a configured provider",
                    prof.provider
                );
            }
        }
        if let Some(h) = &self.telemetry.headers_secret {
            if !self.secrets.contains_key(h) {
                anyhow::bail!(
                    "telemetry.headers_secret = {h:?} has no matching entry under [secrets]"
                );
            }
        }
        for (name, prof) in self.all_profiles() {
            if prof.max_loops == 0 {
                anyhow::bail!("profiles.{name}.max_loops must be at least 1");
            }
        }
        // The search key's secret may be absent: web.search is then not
        // offered, and health says why (theseus-4o4c).
        let web = &self.tools.web;
        if !(1..=WEB_TIMEOUT_MAX_SECS).contains(&web.timeout_secs) {
            anyhow::bail!(
                "tools.web.timeout_secs must be 1 to {WEB_TIMEOUT_MAX_SECS}: every in-process call ends by 120 s"
            );
        }
        if web.max_bytes == 0 {
            anyhow::bail!("tools.web.max_bytes must be at least 1");
        }
        if web.search_key_secret.is_empty() {
            anyhow::bail!("tools.web.search_key_secret must name a [secrets] entry");
        }
        // Paths only: the files are read when a turn compiles, never here.
        let files = std::iter::once(("context.files".to_string(), &self.context.files)).chain(
            self.personas
                .iter()
                .map(|(name, p)| (format!("personas.{name}.files"), &p.files)),
        );
        for (key, list) in files {
            if let Some(f) = list
                .iter()
                .map(crate::context_files::ContextEntry::path)
                .find(|f| !(f.starts_with('/') || f.starts_with("~/")))
            {
                anyhow::bail!("{key} entry {f:?} must be an absolute path or start with ~/");
            }
        }
        self.validate_places()?;
        self.memory.validate()?;
        self.routing
            .validate(|p| self.all_profiles().contains_key(p))?;
        self.lsp.validate()?;
        self.sessions.validate()?;
        self.people.validate()?;
        self.validate_voice()?;
        self.validate_mcp_server()?;
        if let Some(name) = &self.context.default_persona {
            if !self.personas.contains_key(name) {
                anyhow::bail!(
                    "context.default_persona = {name:?} is not a persona: {}",
                    match self.persona_names() {
                        known if known.is_empty() => "no [personas.<name>] table is defined".into(),
                        known => format!("the personas are {known}"),
                    }
                );
            }
        }
        for (program, grant) in &self.broker.programs {
            if program.is_empty() || program.contains('/') || program.contains(char::is_whitespace)
            {
                anyhow::bail!(
                    "broker.programs.\"{program}\" must be a program's name as argv[0] gives it, \
                     found on PATH, with no '/'"
                );
            }
            if let Some(what) = crate::broker::launcher(program) {
                anyhow::bail!(
                    "broker.programs.{program}: a secret granted to a program reaches only that \
                     program, never one it can be made to run, and {program} is {what}, whose \
                     job is to run what the call names (theseus-txvt). Grant the secret to the \
                     program {program} would run, and have the job run that one directly"
                );
            }
            if grant.env.is_empty() && grant.aws_account.is_none() {
                anyhow::bail!(
                    "broker.programs.{program} grants nothing: name a variable and its [secrets] \
                     entry, as env = {{ GH_TOKEN = \"github_token\" }}, or an AWS account whose \
                     job session it gets, as aws_account = \"111122223333\""
                );
            }
            if let Some(id) = &grant.aws_account {
                if !self.aws.accounts.contains_key(id) {
                    anyhow::bail!(
                        "broker.programs.{program}.aws_account = {id:?} is not an account under \
                         [aws.accounts]"
                    );
                }
            }
            for (var, secret) in &grant.env {
                let name = var.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
                    && var.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
                if !name {
                    anyhow::bail!(
                        "broker.programs.{program}.env: {var:?} is not an environment variable's name"
                    );
                }
                if !self.secrets.contains_key(secret) {
                    anyhow::bail!(
                        "broker.programs.{program}.env.{var} = {secret:?} has no matching entry \
                         under [secrets]"
                    );
                }
            }
        }
        for name in self.broker.secrets.keys() {
            if !self.secrets.contains_key(name) {
                anyhow::bail!("broker.secrets.{name} has no matching entry under [secrets]");
            }
        }
        for (k, argv) in self
            .policy
            .allow_argv
            .iter()
            .chain(&self.policy.approve_argv)
            .enumerate()
        {
            if argv.is_empty() {
                anyhow::bail!("policy argv entry {k} is empty");
            }
        }
        let registry = theseus_tools::default_registry();
        let mcp_key = |k: &str| {
            let parts: Vec<&str> = k.split('/').collect();
            parts.len() <= 2 && parts.iter().all(|p| !p.is_empty())
        };
        // The core's own tools beside the toollets: the web's, a task's, a
        // wake's, and AWS's.
        let core_tools = || {
            crate::web::NAMES
                .into_iter()
                .chain(crate::task::NAMES)
                .chain(crate::task_graph::tools::NAMES)
                .chain(crate::wake::NAMES)
                .chain(crate::glide::NAMES)
                .chain(crate::extend::NAMES)
                .chain(crate::aws::NAMES)
                .chain(crate::term::NAMES)
                .chain(crate::lsp::NAMES)
                .chain(crate::memory_lookup::NAMES)
        };
        for name in self.policy.tools.keys() {
            let known = match name.strip_prefix(crate::policy::MCP_PREFIX) {
                Some(rest) => rest.contains('/') && mcp_key(rest),
                None => registry.get(name).is_some() || core_tools().any(|t| t == name),
            };
            if !known {
                let names: Vec<&str> = registry
                    .all()
                    .map(|t| t.name())
                    .chain(core_tools())
                    .collect();
                anyhow::bail!(
                    "policy.tools.\"{name}\" is not a tool (the tools: {}; an MCP tool is \"mcp:<server>/<tool>\")",
                    names.join(", ")
                );
            }
        }
        for key in self.policy.mcp.keys() {
            if !mcp_key(key) {
                anyhow::bail!("policy.mcp.\"{key}\" must be \"server\" or \"server/tool\"");
            }
        }
        if !self.all_profiles().contains_key(&self.model.live) {
            anyhow::bail!(
                "model.live = {:?} is not the implicit \"default\" profile nor a key of [profiles]",
                self.model.live
            );
        }
        limits::check_dollars(&self.kernel)?;
        let builtin = crate::catalog::Catalog::builtin();
        for (id, row) in &self.catalog {
            for (key, price) in row.prices() {
                if price.is_some_and(|p| !p.is_finite() || p < 0.0) {
                    anyhow::bail!(
                        "catalog.\"{id}\".{key} must be a price in US dollars per million tokens, \
                         zero or more"
                    );
                }
            }
            if let Some(l) = &row.long_prompt {
                let prices = [
                    l.input_per_mtok,
                    l.output_per_mtok,
                    l.cache_read_per_mtok,
                    l.cache_write_per_mtok,
                    l.cache_write_1h_per_mtok,
                ];
                if l.above_tokens == 0 || prices.iter().any(|p| !p.is_finite() || *p < 0.0) {
                    anyhow::bail!(
                        "catalog.\"{id}\".long_prompt needs above_tokens above zero and its five \
                         prices in US dollars per million tokens, zero or more"
                    );
                }
            }
            if let Err(missing) = row.over(builtin.get(id)) {
                anyhow::bail!(
                    "catalog.\"{id}\" is not a built-in model, so its table needs {}",
                    missing.join(", ")
                );
            }
        }
        // A refusal's fallback is another priced model of the same provider
        // (theseus-7gir.18), so its call reserves and settles as any does.
        let catalog = crate::catalog::Catalog::with_overrides(&self.catalog);
        for (id, e) in &catalog.entries {
            let Some(to) = &e.refusal_fallback_model else {
                continue;
            };
            if to == id || catalog.get(to).is_none_or(|f| f.provider != e.provider) {
                anyhow::bail!(
                    "catalog.\"{id}\".refusal_fallback_model = {to:?} must name another model of \
                     the catalog that {} serves",
                    e.provider
                );
            }
        }
        // The web UI has no auth yet (§3.14): whoever reaches it is the
        // operator, so it listens on loopback only (theseus-2fo).
        let bind = &self.web.bind;
        if !bind
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
        {
            anyhow::bail!(
                "web.bind = {bind:?} is not a loopback address: the web UI has no auth yet \
                 (spec §3.14), so it listens only on 127.0.0.1 or ::1"
            );
        }
        // The dev page's origin is served beside the UI's own: it must be a
        // page on this machine (theseus-zab).
        if let Some(o) = &self.web.dev_origin {
            if !dev_origin_ok(o) {
                anyhow::bail!(
                    "web.dev_origin = {o:?} is not a dev page's origin on this machine: it must be \
                     http://, then localhost or a loopback address, then a port, and nothing more \
                     (for the Vite dev server, http://localhost:5173)"
                );
            }
        }
        self.validate_aws()?;
        self.validate_mcp()
    }

    /// Deserialize a document and resolve its implicit profile and provider.
    fn from_toml(text: &str) -> Result<Self, toml::de::Error> {
        let mut cfg: Config = toml::from_str(text)?;
        let m = &cfg.model;
        let mut profiles = cfg.profiles.clone();
        profiles
            .entry("default".into())
            .or_insert_with(|| ProfileConfig {
                provider: m.provider.clone(),
                model: m.model.clone(),
                max_output_tokens: m.max_output_tokens,
                system: m.system.clone(),
                effort: m.effort,
                effort_fixed: false,
                thinking_display: m.thinking_display,
                max_loops: m.max_loops,
                max_loops_mode: m.max_loops_mode,
                refusal_fallbacks: m.refusal_fallbacks,
                cache_ttl: m.cache_ttl,
            });
        let mut providers = cfg.providers.clone();
        providers
            .entry("anthropic".into())
            .or_insert_with(|| ProviderConfig {
                api_base: m.api_base.clone(),
                api_key_secret: m.api_key_secret.clone(),
                kind: ProviderKind::AnthropicMessages,
                timeouts: None,
            });
        cfg.resolved = Resolved {
            profiles,
            providers,
        };
        Ok(cfg)
    }

    /// Every profile by name, with the implicit `default` synthesized from
    /// `[model]` unless `[profiles.default]` overrides it.
    pub fn all_profiles(&self) -> &BTreeMap<String, ProfileConfig> {
        &self.resolved.profiles
    }

    /// A profile by name, or the error that names every configured one.
    pub fn profile(&self, name: &str) -> anyhow::Result<&ProfileConfig> {
        self.resolved.profiles.get(name).ok_or_else(|| {
            anyhow::anyhow!(
                "unknown profile {name:?}; configured: {}",
                self.resolved
                    .profiles
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
    }

    /// The persona in play (theseus-c48): `[context].default_persona`, the
    /// only choice until Jev chooses one (theseus-8kk, theseus-0j2).
    pub fn persona(&self) -> Option<&str> {
        self.context.default_persona.as_deref()
    }

    /// The context files a session gets while `persona` is in play, in the
    /// order its system block carries them: the system level, then the
    /// persona's.
    pub fn context_paths(&self, persona: Option<&str>) -> Vec<crate::context_files::ContextPath> {
        use crate::context_files::ContextPath;
        let system = self.context.files.iter().map(|p| ContextPath {
            path: p.path().into(),
            persona: None,
            public: p.readers() == crate::context_files::ContextReaders::Public,
        });
        let own = persona
            .and_then(|name| self.personas.get_key_value(name))
            .into_iter()
            .flat_map(|(name, p)| {
                p.files.iter().map(|f| ContextPath {
                    path: f.path().into(),
                    persona: Some(name.clone()),
                    public: f.readers() == crate::context_files::ContextReaders::Public,
                })
            });
        system.chain(own).collect()
    }

    /// Every persona's name, comma-separated.
    fn persona_names(&self) -> String {
        self.personas.keys().cloned().collect::<Vec<_>>().join(", ")
    }

    /// Every provider by name, with the implicit `anthropic` one synthesized
    /// from `[model]` unless `[providers.anthropic]` overrides it.
    pub fn all_providers(&self) -> &BTreeMap<String, ProviderConfig> {
        &self.resolved.providers
    }

    pub fn state_dir(&self) -> PathBuf {
        expand(&self.server.state_dir)
    }
    pub fn socket_path(&self) -> PathBuf {
        expand(&self.server.socket)
    }

    /// The annotated configuration template, with every parameter documented.
    /// Tested to parse, validate, and to parse with every comment un-commented.
    pub const EXAMPLE_TOML: &'static str = include_str!("../config/theseus.example.toml");

    /// The template, parsed. `example-config` prints `EXAMPLE_TOML` itself so
    /// the comments survive.
    pub fn example() -> Self {
        Self::from_toml(Self::EXAMPLE_TOML).expect("the bundled example config parses")
    }
}

/// A configured path with `~` and environment variables read as a shell
/// reads them (review 2's consideration 5, in place of `shellexpand`, whose
/// dependencies are MPL-2.0).
pub fn expand(p: &str) -> PathBuf {
    PathBuf::from(expand_with(p, |name| std::env::var(name).ok()))
}

/// A command's path argument with a leading `~` as `$HOME`, and nothing
/// else, as `shellexpand::tilde` read it for the gate. The arguments reach
/// the program unexpanded (no shell), so a `$VAR` stays as written: the
/// relative path the program will open.
pub fn expand_home(p: &str) -> PathBuf {
    let (home, rest) = tilde(p, || std::env::var("HOME").ok());
    PathBuf::from(home + rest)
}

/// `p`'s leading `~` (alone, or before `/`) as `home`, and the rest. Without
/// a home, or before a user's name (`~user`), it is left as written.
fn tilde(p: &str, home: impl FnOnce() -> Option<String>) -> (String, &str) {
    if let Some(after) = p.strip_prefix('~') {
        if after.is_empty() || after.starts_with('/') {
            if let Some(h) = home().filter(|h| !h.is_empty()) {
                return (h, after);
            }
        }
    }
    (String::new(), p)
}

/// `p` with a leading `~` (alone, or before `/`) as `$HOME`, and each `$NAME`
/// or `${NAME}` as that variable's value, from `var`. What does not resolve
/// is left as written: a variable that is not set, `~user`, a `$` before
/// anything but a name, and `~` when `HOME` is not set.
pub fn expand_with(p: &str, var: impl Fn(&str) -> Option<String>) -> String {
    let (mut out, mut rest) = tilde(p, || var("HOME"));
    let name_char = |c: char| c.is_ascii_alphanumeric() || c == '_';
    while let Some(at) = rest.find('$') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let (name, len) = match after.strip_prefix('{') {
            Some(b) => match b.find('}') {
                Some(end) => (&b[..end], end + 2),
                None => ("", 0),
            },
            None => {
                let end = after.find(|c| !name_char(c)).unwrap_or(after.len());
                (&after[..end], end)
            }
        };
        let named = name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            && name.chars().all(name_char);
        match named.then(|| var(name)).flatten() {
            Some(value) => out.push_str(&value),
            None => out.push_str(&rest[at..at + 1 + len]),
        }
        rest = &after[len..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests;
