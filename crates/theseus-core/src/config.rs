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
mod judge;
pub(crate) mod memory;
mod sparse;
pub use aws::{AwsAccountConfig, AwsConfig, AwsCredentialNames};
pub use judge::{JudgeConfig, JudgePackConfig, PackMode};
pub use memory::{MemoryConfig, MemoryMode};
pub use sparse::{sparse_note, SPARSE_HEADER};
pub mod mcp;
pub use mcp::{McpConfig, McpServerConfig};

/// Where the config is read when neither `--config` nor `THESEUS_CONFIG`
/// names it: a local file, so nothing here names anyone's vault
/// (theseus-8d1b). A deployment kept in 1Password names its note in its
/// environment or its service unit.
pub const DEFAULT_CONFIG: &str = "~/.theseus/theseus.toml";

/// What a start says when the default config file is missing.
pub const NO_CONFIG: &str = "no config: set THESEUS_CONFIG (or --config) to your config's \
     op:// reference or file, or write one at ~/.theseus/theseus.toml, the default \
     (`theseusd example-config` prints a template)";

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
    /// `[mcp.servers.<name>]`: the MCP servers whose tools turns are offered
    /// (M7 36b), in `config::mcp`.
    #[serde(default, skip_serializing_if = "McpConfig::is_empty")]
    pub mcp: McpConfig,
    /// `[sandbox]`: L1 for `proc.run` (M4 17b), in `crate::sandbox`.
    #[serde(default)]
    pub sandbox: crate::sandbox::SandboxConfig,
    /// `[judge]`: Jev's judgments, in shadow (M5 23a), in `config/judge.rs`.
    #[serde(default)]
    pub judge: JudgeConfig,
    /// `[approval]`, retired (theseus-zmgb): an answer counts only from a
    /// private place, by the owner (the place rule). A config that still has
    /// the section loads, with one warning a load; nothing reads it.
    #[serde(default, skip_serializing)]
    pub approval: Option<toml::Table>,
    /// `[places]` (the place rule; `[labels]`, its old name, still reads).
    #[serde(default, alias = "labels")]
    #[serde(skip_serializing_if = "PlacesConfig::is_empty")]
    pub places: PlacesConfig,
    /// The 1Password token file this daemon was pointed at (`--op-token-file`
    /// or `THESEUS_OP_TOKEN_FILE`): set at startup, never read from the TOML.
    /// The floor keeps it, whichever way it was named (theseus-8az).
    #[serde(skip)]
    pub op_token_file: Option<PathBuf>,
    /// The last-known-good copy of the vault's config note (theseus-2fo):
    /// set at startup when the config is `op://`, never read from the TOML.
    /// The floor keeps it, as it keeps the token file.
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

impl Default for DiscordConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            token_secret: default_discord_token_secret(),
            bindings_file: default_bindings_file(),
            edit_interval_ms: default_edit_interval_ms(),
            notice_embeds: false,
            rest_proxy: None,
            gateway_proxy: None,
        }
    }
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
    /// Entries a listing, glob, or grep returns.
    #[serde(default = "default_max_entries")]
    pub max_entries: usize,
    /// How long a turn waits for `proc.run` before the job continues in the background.
    #[serde(default = "default_proc_sync_secs")]
    pub proc_sync_secs: u64,
    #[serde(default = "default_proc_timeout_secs")]
    pub proc_timeout_secs: u64,
    #[serde(default = "default_proc_timeout_max_secs")]
    pub proc_timeout_max_secs: u64,
    /// Environment variables `proc.run` passes through from the daemon (nothing else).
    #[serde(default = "default_proc_env")]
    pub proc_env: Vec<String>,
    /// The most a job's raw output file keeps (theseus-102). Past it the job
    /// runs on, what it prints is counted and dropped, and its result says
    /// so. The runtime reads only the file's last 4 MiB.
    #[serde(default = "default_job_output_max_bytes")]
    pub job_output_max_bytes: u64,
    /// `[tools.web]`: `http.fetch` and `web.search` (DD5).
    #[serde(default)]
    pub web: WebToolsConfig,
}

/// `[tools.web]`: the limits of `http.fetch` and `web.search`, and the secret
/// that holds the search key (DD5).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebToolsConfig {
    /// A fetch's or a search's whole time, redirects included.
    #[serde(default = "default_web_timeout_secs")]
    pub timeout_secs: u64,
    /// The most bytes of a body a fetch reads; a call may ask for fewer.
    #[serde(default = "default_web_max_bytes")]
    pub max_bytes: usize,
    /// The `[secrets]` entry that holds the Brave Search API key.
    #[serde(default = "default_search_key_secret")]
    pub search_key_secret: String,
}

fn default_web_timeout_secs() -> u64 {
    30
}
fn default_web_max_bytes() -> usize {
    2 * 1024 * 1024
}
fn default_search_key_secret() -> String {
    "brave_api_key".into()
}

impl Default for WebToolsConfig {
    fn default() -> Self {
        Self {
            timeout_secs: default_web_timeout_secs(),
            max_bytes: default_web_max_bytes(),
            search_key_secret: default_search_key_secret(),
        }
    }
}

/// The longest a web call may take: every in-process call ends by the
/// runtime's deadline (120 s), so a web call's own timeout must come first.
pub const WEB_TIMEOUT_MAX_SECS: u64 = 110;

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
            max_entries: default_max_entries(),
            proc_sync_secs: default_proc_sync_secs(),
            proc_timeout_secs: default_proc_timeout_secs(),
            proc_timeout_max_secs: default_proc_timeout_max_secs(),
            proc_env: default_proc_env(),
            job_output_max_bytes: default_job_output_max_bytes(),
            web: WebToolsConfig::default(),
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
    /// Each session's spend limit in US dollars (theseus-0sg). At the limit
    /// the session asks the operator whether its spend may go back to $0.
    #[serde(default = "default_spend_limit_usd")]
    pub spend_limit_usd: f64,
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
}

fn default_admission_ceiling() -> u32 {
    8
}
fn default_spend_limit_usd() -> f64 {
    100.0
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

impl Default for KernelSection {
    fn default() -> Self {
        Self {
            admission_ceiling: default_admission_ceiling(),
            spend_limit_usd: default_spend_limit_usd(),
            default_budget: None,
            control_reserve: None,
            heartbeat_secs: default_heartbeat_secs(),
            default_deadline_secs: default_deadline_secs(),
            confirm_ttl_secs: default_confirm_ttl_secs(),
            min_repeat_minutes: default_min_repeat_minutes(),
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

    pub fn to_kernel_config(&self) -> theseus_kernel::KernelConfig {
        theseus_kernel::KernelConfig {
            admission_ceiling: self.admission_ceiling.max(1),
            default_deadline_ms: self.default_deadline_secs * 1000,
            spend_limit_micros: theseus_kernel::usd_to_micros(self.spend_limit_usd),
            confirm_ttl_ms: self.confirm_ttl_secs * 1000,
            heartbeat_ms: self.heartbeat_secs.max(1) * 1000,
            fault_after_startup_step: None,
            min_repeat_ms: self.min_repeat_minutes.max(1) * 60_000,
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

/// `[index]`: the index tender (M6 §2.2, roadmap row 51), `theseus-index`
/// installed beside `theseusd`, run after serving and restarted when it exits.
/// The defaults need no paste: it runs, BM25 and entities always, and vectors
/// once the weights are in `weights_dir`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Where the embedding model's files are (`nomic-embed-text-v1.5/`).
    /// Nothing is fetched: without them, BM25 and entities answer alone.
    #[serde(default = "default_weights_dir")]
    pub weights_dir: String,
    /// The embedding model's threads.
    #[serde(default = "default_index_threads")]
    pub threads: u32,
    /// The model unloads after this many minutes unused, and loads again on
    /// the next use.
    #[serde(default = "default_idle_unload_mins")]
    pub idle_unload_mins: f64,
}

fn default_weights_dir() -> String {
    "~/.cache/theseus/models".into()
}
fn default_index_threads() -> u32 {
    1
}
fn default_idle_unload_mins() -> f64 {
    10.0
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            weights_dir: default_weights_dir(),
            threads: default_index_threads(),
            idle_unload_mins: default_idle_unload_mins(),
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
    #[serde(default)]
    pub thinking_display: ThinkingDisplay,
    /// Tool loops per turn before the Advancer ends it.
    #[serde(default = "default_max_loops")]
    pub max_loops: u32,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
            refusal_fallbacks: true,
            cache_ttl: CacheTtl::default(),
            api_base: default_api_base(),
            api_key_secret: default_key_name(),
            timeouts: Default::default(),
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
        // The search key's secret may be absent: web.search then says it has
        // no key, as a result the model reads (DD5).
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
        self.validate_voice()?;
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
                .chain(crate::wake::NAMES)
                .chain(crate::aws::NAMES)
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
        let limit = self.kernel.spend_limit_usd;
        if !limit.is_finite() || limit <= 0.0 {
            anyhow::bail!("kernel.spend_limit_usd = {limit} must be a dollar amount above zero");
        }
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
            if let Err(missing) = row.over(builtin.get(id)) {
                anyhow::bail!(
                    "catalog.\"{id}\" is not a built-in model, so its table needs {}",
                    missing.join(", ")
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
                thinking_display: m.thinking_display,
                max_loops: m.max_loops,
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
mod tests {
    use super::*;
    use crate::policy::Posture;

    /// Consideration 5 of review 2: `~` and `$VAR` without `shellexpand`. What
    /// does not resolve is left as written.
    #[test]
    fn a_path_expands_home_and_variables_as_a_shell_would() {
        let env = |name: &str| match name {
            "HOME" => Some("/home/invented".to_string()),
            "XDG_STATE" => Some("/var/invented".to_string()),
            "EMPTY" => Some(String::new()),
            _ => None,
        };
        for (p, want) in [
            ("~", "/home/invented"),
            ("~/.theseus", "/home/invented/.theseus"),
            ("~other/x", "~other/x"),
            ("a/~/b", "a/~/b"),
            ("$XDG_STATE/theseus", "/var/invented/theseus"),
            ("${XDG_STATE}x/y", "/var/inventedx/y"),
            ("~/$XDG_STATE", "/home/invented//var/invented"),
            ("$UNSET/x", "$UNSET/x"),
            ("${UNSET}/x", "${UNSET}/x"),
            ("x$EMPTY/y", "x/y"),
            ("cost: $5 and $", "cost: $5 and $"),
            ("${unclosed", "${unclosed"),
            ("${}", "${}"),
            ("$$HOME", "$/home/invented"),
            ("/plain/path", "/plain/path"),
            ("é$HOME/中", "é/home/invented/中"),
        ] {
            assert_eq!(expand_with(p, env), want, "{p}");
        }
        assert_eq!(
            expand_with("~/x", |_| None),
            "~/x",
            "no HOME: left as written"
        );
        assert_eq!(
            expand_with("~/x", |_| Some(String::new())),
            "~/x",
            "an empty HOME: left as written"
        );
    }

    #[test]
    fn example_template_parses_and_validates() {
        let cfg = Config::example();
        cfg.validate().unwrap();
        assert_eq!(cfg.model.live, "sonnet");
        assert!(cfg.profiles.contains_key("glm"));
        assert!(cfg.providers.contains_key("zai"));
        assert_eq!(cfg.telemetry.otlp_endpoint, None);
    }

    /// theseus-n88g.1 (D6): a secret may come from outside the vault, from
    /// the daemon's environment or a private file, and a malformed entry is
    /// refused by its name.
    #[test]
    fn a_secret_may_come_from_the_environment_or_a_file() {
        let mut cfg = Config::example();
        cfg.secrets
            .insert("anthropic_api_key".into(), "env:ANTHROPIC_API_KEY".into());
        cfg.secrets
            .insert("zai_api_key".into(), "file:~/.config/invented/zai".into());
        cfg.validate().unwrap();
        for (bad, why) in [
            ("file:zai.txt", "absolute path"),
            ("env:", "variable's name"),
            ("env:ZAI-KEY", "variable's name"),
            (
                "vault:zai",
                "not a valid op:// reference, env:NAME, or file:PATH",
            ),
        ] {
            cfg.secrets.insert("zai_api_key".into(), bad.into());
            let e = format!("{:#}", cfg.validate().unwrap_err());
            assert!(
                e.contains("secrets.zai_api_key") && e.contains(why),
                "{bad}: {e}"
            );
        }
    }

    /// Every commented-out parameter must be a real parameter with a valid
    /// value: un-comment them all and the result must still parse under
    /// deny_unknown_fields. This is what stops the template from lying.
    #[test]
    #[expect(clippy::cognitive_complexity, reason = "shape budget: split it")]
    fn example_template_uncommented_still_parses() {
        let mut out = String::new();
        for line in Config::EXAMPLE_TOML.lines() {
            let t = line.trim_start();
            if let Some(rest) = t.strip_prefix("# ") {
                let looks_like_toml = rest.starts_with('[')
                    || rest
                        .split_once('=')
                        .map(|(k, _)| {
                            let k = k.trim();
                            // A quoted key: `"proc.run" = …`, `"server/tool" = …`.
                            let quoted = k.len() > 2
                                && k.starts_with('"')
                                && k.ends_with('"')
                                && !k[1..k.len() - 1].contains('"');
                            quoted
                                || (!k.is_empty()
                                    && k.chars()
                                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.'))
                        })
                        .unwrap_or(false);
                if looks_like_toml {
                    out.push_str(rest);
                    out.push('\n');
                    continue;
                }
            }
            out.push_str(line);
            out.push('\n');
        }
        let cfg = Config::from_toml(&out).unwrap_or_else(|e| {
            panic!("un-commented template must parse (a documented key is stale or its value invalid): {e}\n{out}")
        });
        assert_eq!(
            cfg.telemetry.otlp_endpoint.as_deref(),
            Some("http://127.0.0.1:4318")
        );
        assert!(cfg.providers["zai"].timeouts.is_some());
        assert!(cfg.model.system.is_some());
        // The secrets from outside the vault (theseus-n88g.1) are real entries.
        assert_eq!(cfg.secrets["ci_runner_key"], "env:CI_RUNNER_KEY");
        assert_eq!(
            cfg.secrets["ci_deploy_key"],
            "file:~/.config/theseus/deploy-key"
        );
        // The 1-hour cache TTL (theseus-ev1), on the implicit profile and on
        // the Sonnet one; the GLM one keeps the default.
        assert_eq!(cfg.model.cache_ttl, CacheTtl::OneHour);
        assert_eq!(cfg.profiles["sonnet"].cache_ttl, CacheTtl::OneHour);
        assert_eq!(cfg.profiles["glm"].cache_ttl, CacheTtl::FiveMinutes);
        let new = &cfg.catalog["some-new-model"];
        assert_eq!(
            (new.cache_write_1h_per_mtok, new.caches),
            (Some(6.0), Some(true))
        );
        // The commented tool lines and the [policy.mcp] example are real too.
        assert_eq!(cfg.policy.tools["proc.run"], Posture::Approve);
        assert_eq!(cfg.policy.tools["http.fetch"], Posture::Notify);
        assert_eq!(cfg.policy.tools["task.create"], Posture::Notify);
        assert_eq!(cfg.policy.tools["wake.at"], Posture::Notify);
        assert_eq!(cfg.policy.tools["aws.call"], Posture::Approve);
        assert_eq!(cfg.policy.tools["aws.stack.apply"], Posture::Approve);
        assert_eq!(cfg.policy.tools.len(), 30);
        // The AWS account's table, and [policy.aws]'s lines (rows 29 and 30, C1 and C2).
        let a = &cfg.aws.accounts["111122223333"];
        assert_eq!(a.credentials, AwsCredentialNames::default());
        assert_eq!(a.region, "us-west-2");
        assert_eq!(a.allowed_regions(), ["us-west-2", "us-east-1"]);
        assert_eq!(a.endpoint.as_deref(), Some("http://127.0.0.1:4566"));
        assert_eq!(a.owner_role.as_deref(), Some("theseus-owner"));
        assert_eq!(a.deployment(), "theseus-desktop");
        assert_eq!(a.monthly_budget_usd, Some(50));
        assert_eq!(
            cfg.broker.programs["aws"].aws_account.as_deref(),
            Some("111122223333")
        );
        assert_eq!(cfg.policy.aws["write"], Posture::Notify);
        assert_eq!(cfg.policy.aws["run"], Posture::Notify);
        assert_eq!(cfg.policy.aws["read"], Posture::Open);
        assert_eq!(cfg.policy.aws["ec2"], Posture::Notify);
        assert_eq!(cfg.policy.aws["s3:ListBuckets"], Posture::Approve);
        assert_eq!(cfg.policy.mcp["some-server"], Posture::Notify);
        assert_eq!(cfg.policy.mcp["some-server/read-only-tool"], Posture::Open);
        assert_eq!(
            cfg.policy.mcp["some-server/dangerous-tool"],
            Posture::Approve
        );
        // The broker's example grant and posture are real (theseus-dcy).
        assert_eq!(cfg.broker.programs["gh"].env["GH_TOKEN"], "github_token");
        assert_eq!(cfg.broker.secrets["github_token"].posture, Posture::Notify);
        // [voice] and its key's line are real too (rows 77 and 78).
        assert!(cfg.voice.enabled && cfg.secrets.contains_key(&cfg.voice.key_secret));
        crate::sandbox::the_templates_sandbox_section(&cfg.sandbox);
        memory::the_templates_memory_section(&cfg.memory);
        judge::the_templates_judge_section(&cfg);
        crate::broker::the_templates_broker_section(&cfg);
        crate::broker::the_templates_harness_only_keys(&cfg);
        mcp::the_templates_mcp_section(&cfg);
    }

    /// theseus-8d1b: the template and the default config name no one's
    /// vault. Every `op://` reference the template holds, in a value or a
    /// comment, names its vault and item by placeholders (`<your vault>`),
    /// and the default config is a local file.
    #[test]
    fn the_template_and_the_default_name_no_ones_vault() {
        assert!(!DEFAULT_CONFIG.contains("op://"), "{DEFAULT_CONFIG}");
        let placeholder = |s: &str| s.starts_with('<') && s.ends_with('>');
        let mut refs = 0;
        for (at, _) in Config::EXAMPLE_TOML.match_indices("op://") {
            let rest = &Config::EXAMPLE_TOML[at + "op://".len()..];
            // Prose that names the scheme ("an op:// reference").
            if rest.starts_with(char::is_whitespace) {
                continue;
            }
            let mut parts = rest.split('/');
            let (vault, item) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
            assert!(
                placeholder(vault) && placeholder(item),
                "a reference that names a vault or an item: op://{vault}/{item}/…"
            );
            refs += 1;
        }
        assert!(refs >= 8, "{refs} references");
        for (name, r) in &Config::example().secrets {
            let r = SecretRef::parse(r).unwrap();
            assert_eq!(r.vault, "<your vault>", "{name}");
        }
    }

    /// The template's [policy.tools] names every tool in the registry, one
    /// line each (set or commented), and nothing else, so it cannot drift
    /// from the tools as they are added.
    #[test]
    fn example_template_lists_every_tool_under_policy_tools() {
        let listed: Vec<String> = Config::EXAMPLE_TOML
            .lines()
            .skip_while(|l| l.trim() != "[policy.tools]")
            .skip(1)
            .take_while(|l| !l.starts_with('[') && !l.starts_with("# ["))
            .filter_map(|l| {
                let l = l.strip_prefix("# ").unwrap_or(l).trim_start();
                let (name, rest) = l.strip_prefix('"')?.split_once('"')?;
                rest.trim_start().starts_with('=').then(|| name.to_string())
            })
            .collect();
        let tools: Vec<String> = theseus_tools::default_registry()
            .all()
            .map(|t| t.name().to_string())
            .chain(crate::web::NAMES.map(String::from))
            .chain(crate::task::NAMES.map(String::from))
            .chain(crate::wake::NAMES.map(String::from))
            .chain(crate::aws::NAMES.map(String::from))
            .collect();
        for t in &tools {
            assert!(
                listed.contains(t),
                "the template's [policy.tools] must list `{t}`: {listed:?}"
            );
        }
        for l in &listed {
            assert!(tools.contains(l), "[policy.tools] lists `{l}`, not a tool");
        }
        assert_eq!(listed.len(), tools.len(), "one line per tool: {listed:?}");
        // Reads stay quiet out of the box; everything else inherits (an AWS
        // call's posture is [policy.aws]'s).
        let set = Config::example().policy.tools;
        let reads = [
            "fs.read",
            "fs.glob",
            "fs.grep",
            "fs.list",
            "git.diff",
            "git.log",
            "text.diff",
            "aws.describe",
        ];
        for r in reads {
            assert_eq!(set.get(r), Some(&Posture::Open), "{r}");
        }
        assert_eq!(set.len(), reads.len(), "{set:?}");
    }

    /// The template's [policy] is the config to set (theseus-8az): notify
    /// everywhere, reads open, the default approve lists, no retired keys, a
    /// commented [policy.mcp], and no word of a deny.
    #[test]
    fn example_template_is_the_policy_to_set() {
        let (cfg, warnings) = Config::parse(Config::EXAMPLE_TOML).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(cfg.policy.enforcement, Posture::Notify);
        assert_eq!(cfg.policy.allow_argv, default_allow_argv());
        assert_eq!(cfg.policy.approve_argv, default_approve_argv());
        assert_eq!(cfg.tools.approve_paths, default_approve_paths());
        assert!(cfg.policy.mcp.is_empty(), "[policy.mcp] stays commented");
        assert!(cfg.mcp.is_empty(), "[mcp.servers] stays commented");
        let section: Vec<&str> = Config::EXAMPLE_TOML
            .lines()
            .skip_while(|l| !l.contains("------ policy"))
            .take_while(|l| !l.contains("------ catalog"))
            .collect();
        assert!(section.contains(&"[policy]") && section.contains(&"# [policy.mcp]"));
        for l in &section {
            assert!(!l.to_lowercase().contains("refuse"), "{l}");
        }
        assert!(
            !Config::EXAMPLE_TOML.to_lowercase().contains("deny"),
            "the template never speaks of a deny"
        );
    }

    fn policy_only(policy: &str) -> Result<Config> {
        Ok(parse_policy(policy)?.0)
    }

    fn parse_policy(policy: &str) -> Result<(Config, Vec<String>)> {
        Config::parse(&format!(
            "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n[policy]\n{policy}\n"
        ))
    }

    #[test]
    fn a_policy_with_only_enforcement_loads_and_every_tool_inherits_it() {
        let (cfg, warnings) = parse_policy("enforcement = \"notify\"").unwrap();
        assert_eq!(cfg.policy.enforcement, Posture::Notify);
        assert!(cfg.policy.tools.is_empty() && cfg.policy.mcp.is_empty());
        assert!(warnings.is_empty());
        assert_eq!(policy_only("").unwrap().policy.enforcement, Posture::Open);
    }

    /// `[policy] external_text` (theseus-9bp): `ask` by default, so a note
    /// without it loads unchanged; `notify` loads; anything else fails, and
    /// the error names the two.
    #[test]
    fn external_text_is_ask_by_default_and_names_its_two_values() {
        use crate::external::Mode;
        assert_eq!(policy_only("").unwrap().policy.external_text, Mode::Ask);
        let notify = policy_only("external_text = \"notify\"").unwrap();
        assert_eq!(notify.policy.external_text, Mode::Notify);
        let e = format!(
            "{:#}",
            policy_only("external_text = \"approve\"").unwrap_err()
        );
        assert!(e.contains("ask") && e.contains("notify"), "{e}");
        let (t, _) = Config::parse(Config::EXAMPLE_TOML).unwrap();
        assert_eq!(t.policy.external_text, Mode::Ask, "the template says ask");
    }

    /// The spellings the loader once accepted with a warning, which no
    /// deployment still uses (Eddie's note checked by key name, 2026-09-29),
    /// now fail to load like any unknown key (theseus-0g4): the renamed lists,
    /// the policy class keys, the hook spans, and the old output-cap name.
    #[test]
    fn the_retired_spellings_fail_to_load() {
        for (section, key) in [
            ("tools", "deny_paths = [\"~/.ssh\"]"),
            ("policy", "deny_argv = [[\"sudo\"]]"),
            ("policy", "read = \"allow\""),
            ("policy", "run = \"confirm\""),
            ("telemetry", "hook_spans = false"),
            ("model", "max_tokens = 1000"),
        ] {
            let doc =
                format!("[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n[{section}]\n{key}\n");
            let e = format!("{:#}", Config::parse(&doc).unwrap_err());
            assert!(e.contains("unknown field"), "{section}.{key}: {e}");
        }
    }

    /// `effort`, `thinking_display`, and a provider's `kind` are enums: a
    /// value outside them fails to load and the error lists the ones there are.
    #[test]
    fn enum_keys_name_their_values_when_one_is_wrong() {
        for (doc, values) in [
            (
                "[model]\neffort = \"huge\"",
                "`low`, `medium`, `high`, `xhigh`, `max`",
            ),
            (
                "[profiles.p]\nmodel = \"m\"\nthinking_display = \"full\"",
                "`summarized`, `omitted`, `updates`",
            ),
            (
                "[providers.p]\napi_base = \"https://x\"\napi_key_secret = \"anthropic_api_key\"\n\
                 kind = \"openai\"",
                "`anthropic_messages`",
            ),
        ] {
            let text = format!("[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n{doc}\n");
            let e = format!("{:#}", Config::parse(&text).unwrap_err());
            assert!(e.contains("unknown variant") && e.contains(values), "{e}");
        }
        let (cfg, _) = Config::parse(
            "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n[model]\neffort = \"xhigh\"\n\
             thinking_display = \"omitted\"\n",
        )
        .unwrap();
        let d = &cfg.all_profiles()["default"];
        assert_eq!(
            (d.effort, d.thinking_display),
            (Some(Effort::Xhigh), ThinkingDisplay::Omitted)
        );
    }

    /// `[telemetry]` keeps its five keys (theseus-hee), and the exporter is
    /// in every build, so an endpoint loads with no warning (until then a
    /// build without the `otel` feature warned once). A blank endpoint counts
    /// as unset.
    #[test]
    fn an_otlp_endpoint_loads_quietly_now_that_every_build_exports() {
        let base = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\notlp_headers = \"op://v/h/f\"\n\n[telemetry]\n";
        let (cfg, warnings) = Config::parse(&format!(
            "{base}otlp_endpoint = \"http://127.0.0.1:4318\"\nheaders_secret = \"otlp_headers\"\n\
             service_name = \"theseus-x\"\nmetrics_interval_secs = 5\nexport_timeout_secs = 3\n"
        ))
        .unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let t = &cfg.telemetry;
        assert_eq!(t.endpoint(), Some("http://127.0.0.1:4318"));
        assert_eq!(t.headers_secret.as_deref(), Some("otlp_headers"));
        assert_eq!(
            (
                t.service_name.as_str(),
                t.metrics_interval_secs,
                t.export_timeout_secs
            ),
            ("theseus-x", 5, 3)
        );
        for quiet in ["", "otlp_endpoint = \"  \"\n"] {
            let (c, w) =
                Config::parse(&format!("{base}service_name = \"theseus\"\n{quiet}")).unwrap();
            assert!(w.is_empty(), "{w:?}");
            assert_eq!(c.telemetry.endpoint(), None);
        }
    }

    /// Eddie's vault config (its [kernel] keys checked 2026-09-29, by name
    /// only) still sets the unit budget, `default_budget` and
    /// `control_reserve` (theseus-0sg). It loads with exactly one warning,
    /// which names both; the session limit is the $100 default; and
    /// `theseusd config` shows `spend_limit_usd` and neither old key.
    #[test]
    fn the_vault_config_with_its_unit_budget_loads_with_one_warning() {
        let text = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n\
                    [kernel]\nadmission_ceiling = 8\ndefault_budget = 20000000\n\
                    control_reserve = 10000\nheartbeat_secs = 60\n\
                    default_deadline_secs = 600\nconfirm_ttl_secs = 900\n";
        let (cfg, warnings) = Config::parse(text).unwrap();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].starts_with(
                "kernel.default_budget and kernel.control_reserve are budget units, retired \
                 and ignored (theseus-0sg)"
            ) && warnings[0].contains("kernel.spend_limit_usd (now $100)"),
            "{warnings:?}"
        );
        assert_eq!(cfg.kernel.spend_limit_usd, 100.0);
        assert_eq!(
            cfg.kernel.to_kernel_config().spend_limit_micros,
            100_000_000
        );
        let shown = toml::to_string(&cfg.kernel).unwrap();
        assert!(shown.contains("spend_limit_usd = 100.0"), "{shown}");
        assert!(
            !shown.contains("default_budget") && !shown.contains("control_reserve"),
            "{shown}"
        );
        // One key alone is named alone; the template sets neither.
        let one = text.replace("control_reserve = 10000\n", "");
        let (_, w) = Config::parse(&one).unwrap();
        assert!(
            w.len() == 1 && w[0].starts_with("kernel.default_budget is budget units"),
            "{w:?}"
        );
        let (t, w) = Config::parse(Config::EXAMPLE_TOML).unwrap();
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(t.kernel.spend_limit_usd, 100.0);
        for bad in ["0.0", "-5.0", "nan"] {
            let doc = format!(
                "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n[kernel]\nspend_limit_usd = {bad}\n"
            );
            assert!(Config::parse(&doc).is_err(), "{bad}");
        }
    }

    /// Eddie's vault config (its [discord] keys checked 2026-09-29, by name
    /// only) has no `notice_embeds` (theseus-w4f). It loads unchanged, with no
    /// warning and the embeds off; the key parses either way, and the
    /// template leaves it off.
    #[test]
    fn the_vault_discord_section_loads_with_notice_embeds_off() {
        let text = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n\
                    [discord]\nenabled = true\ntoken_secret = \"discord_bot_token\"\n\
                    bindings_file = \"bindings.toml\"\nedit_interval_ms = 1200\n";
        let (cfg, warnings) = Config::parse(text).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(!cfg.discord.notice_embeds);
        assert_eq!(cfg.discord.edit_interval_ms, 1200);
        assert!(!DiscordConfig::default().notice_embeds);
        for on in [true, false] {
            let doc = text.replace("1200\n", &format!("1200\nnotice_embeds = {on}\n"));
            assert_eq!(Config::parse(&doc).unwrap().0.discord.notice_embeds, on);
        }
        assert!(!Config::example().discord.notice_embeds);
    }

    /// Context files in two levels (theseus-c48): `[context] files`, which
    /// every session gets, then the files of the persona in play,
    /// `[personas.<name>] files`, chosen by `[context] default_persona`. Every
    /// path must be absolute or start with `~/`, and the key is named when one
    /// is not. Loading checks spelling only: nothing is read.
    #[test]
    fn context_files_come_in_two_levels_and_must_be_absolute_or_home_paths() {
        let base = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n\
                    [context]\nfiles = [\"~/w/USER.md\", \"/nowhere/RULES.md\"]\n\
                    default_persona = \"theseus\"\n\n\
                    [personas.theseus]\nfiles = [\"~/w/persona.md\"]\n\n\
                    [personas.quiet]\n";
        let (cfg, warnings) = Config::parse(base).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(cfg.persona(), Some("theseus"));
        let paths = |persona: Option<&str>| -> Vec<(String, Option<String>)> {
            cfg.context_paths(persona)
                .into_iter()
                .map(|p| (p.path, p.persona))
                .collect()
        };
        let system = |p: &str| (p.to_string(), None);
        assert_eq!(
            paths(Some("theseus")),
            [
                system("~/w/USER.md"),
                system("/nowhere/RULES.md"),
                ("~/w/persona.md".to_string(), Some("theseus".to_string())),
            ],
            "the system level first, then the persona's"
        );
        assert_eq!(
            paths(Some("quiet")),
            [system("~/w/USER.md"), system("/nowhere/RULES.md")],
            "a persona that names no files adds none"
        );
        assert_eq!(paths(None), paths(Some("quiet")));
        for (bad, key) in [
            ("[context]\nfiles = [\"USER.md\"]", "context.files"),
            ("[personas.p]\nfiles = [\"~other/x\"]", "personas.p.files"),
            ("[context]\nfiles = [\"\"]", "context.files"),
        ] {
            let doc = format!("[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n{bad}\n");
            let e = format!("{:#}", Config::parse(&doc).unwrap_err());
            assert!(e.contains(key) && e.contains("absolute path"), "{e}");
        }
        // A profile no longer names context files, and neither does [model].
        for old in [
            "[model]\ncontext_files = []",
            "[profiles.p]\nmodel = \"m\"\ncontext_files = []",
        ] {
            let doc = format!("[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n{old}\n");
            let e = format!("{:#}", Config::parse(&doc).unwrap_err());
            assert!(e.contains("unknown field `context_files`"), "{e}");
        }
        // A config that never names context files has none, says nothing new,
        // and `theseusd config` shows neither table.
        let (plain, w) = Config::parse("[secrets]\nanthropic_api_key = \"op://v/i/f\"\n").unwrap();
        assert!(w.is_empty(), "{w:?}");
        assert!(plain.context_paths(plain.persona()).is_empty());
        let shown = toml::to_string_pretty(&plain).unwrap();
        assert!(
            !shown.contains("[context]") && !shown.contains("personas"),
            "{shown}"
        );
        let shown = toml::to_string_pretty(&cfg).unwrap();
        assert!(
            shown.contains("[context]") && shown.contains("[personas.theseus]"),
            "{shown}"
        );
        // The template: an empty system level, and the persona commented.
        let t = Config::example();
        assert!(t.context.is_empty() && t.personas.is_empty() && t.tools.roots.is_empty());
        let has = |line: &str| Config::EXAMPLE_TOML.lines().any(|l| l.starts_with(line));
        assert!(has("[context]"));
        assert!(has("files = []"));
        assert!(has("# default_persona = \"theseus\""));
        assert!(has("# [personas.theseus]"));
        assert!(has("# roots = [\"/home/zeroaltitude/reports\"]"));
        assert!(!Config::EXAMPLE_TOML.contains("context_files"));
    }

    /// `[context] default_persona` must name a persona the config defines,
    /// and the loader says which ones it does. Personas with no default are
    /// never in play before Jev, and loading says so.
    #[test]
    fn an_unknown_default_persona_is_refused_naming_the_known_ones() {
        let doc = |rest: &str| format!("[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n{rest}\n");
        let e = format!(
            "{:#}",
            Config::parse(&doc(
                "[context]\ndefault_persona = \"thesues\"\n\n[personas.theseus]\n\n[personas.quiet]"
            ))
            .unwrap_err()
        );
        assert!(
            e.contains("context.default_persona = \"thesues\" is not a persona: the personas are quiet, theseus"),
            "{e}"
        );
        let e = format!(
            "{:#}",
            Config::parse(&doc("[context]\ndefault_persona = \"theseus\"")).unwrap_err()
        );
        assert!(e.contains("no [personas.<name>] table is defined"), "{e}");
        let (cfg, w) = Config::parse(&doc("[personas.theseus]\nfiles = [\"~/p.md\"]")).unwrap();
        assert_eq!(cfg.persona(), None);
        assert!(
            w.len() == 1
                && w[0].starts_with(
                    "[personas] defines theseus, but context.default_persona names none of them"
                ),
            "{w:?}"
        );
    }

    /// `narrative` is a top-level key, off unless the config says true
    /// (theseus-5fy). The vault config (checked 2026-09-28) has no such key and
    /// loads unchanged; the template turns it on; written under a table it is
    /// that table's key and fails to load.
    #[test]
    fn narrative_is_off_unless_the_config_says_true() {
        let base = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n";
        let (cfg, warnings) = Config::parse(base).unwrap();
        assert!(!cfg.narrative && warnings.is_empty(), "{warnings:?}");
        for (value, want) in [("true", true), ("false", false)] {
            let (cfg, _) = Config::parse(&format!("narrative = {value}\n\n{base}")).unwrap();
            assert_eq!(cfg.narrative, want, "narrative = {value}");
        }
        let (cfg, warnings) = Config::parse(Config::EXAMPLE_TOML).unwrap();
        assert!(cfg.narrative && warnings.is_empty(), "{warnings:?}");
        let e = format!(
            "{:#}",
            Config::parse(&format!("{base}\n[web]\nnarrative = true\n")).unwrap_err()
        );
        assert!(e.contains("unknown field `narrative`"), "{e}");
        assert!(Config::parse(&format!("narrative = \"yes\"\n{base}")).is_err());
        // `theseusd config` prints it first, where it still means the top level.
        let shown = toml::to_string_pretty(&cfg).unwrap();
        assert!(shown.starts_with("narrative = true\n"), "{shown}");
    }

    /// There is no deny posture: `deny`, wherever a posture goes, fails to
    /// load, and the error names open, notify, and approve (theseus-8az).
    #[test]
    fn deny_and_the_old_words_fail_to_load_naming_the_three_postures() {
        let three = |e: &str| e.contains("open") && e.contains("notify") && e.contains("approve");
        for doc in [
            "enforcement = \"deny\"",
            "[policy.tools]\n\"proc.run\" = \"deny\"",
            "[policy.mcp]\n\"x\" = \"deny\"",
        ] {
            let e = format!("{:#}", policy_only(doc).unwrap_err());
            assert!(
                e.contains("unknown variant `deny`") && three(&e),
                "{doc}: {e}"
            );
        }
        for old in ["strict", "ask", "allow", "confirm"] {
            let e = format!(
                "{:#}",
                policy_only(&format!("enforcement = \"{old}\"")).unwrap_err()
            );
            assert!(three(&e), "{old}: {e}");
        }
        assert!(
            policy_only("[policy.overrides]\n\"fs.edit\" = \"allow\"").is_err(),
            "the old overrides table is [policy.tools] now"
        );
    }

    #[test]
    fn an_unknown_tool_name_still_fails_to_load() {
        let e = format!(
            "{:#}",
            policy_only("[policy.tools]\n\"proc.rum\" = \"approve\"").unwrap_err()
        );
        assert!(e.contains("\"proc.rum\" is not a tool"), "{e}");
        policy_only("[policy.tools]\n\"mcp:x/y\" = \"approve\"").unwrap();
        assert!(policy_only("[policy.tools]\n\"mcp:x\" = \"approve\"").is_err());
        assert!(policy_only("[policy.mcp]\n\"a/b/c\" = \"approve\"").is_err());
    }

    /// `op` and `theseusd` are not on the default approve list: the floor
    /// asks for them whatever the lists say.
    #[test]
    fn the_default_approve_argv_leaves_op_and_theseusd_to_the_floor() {
        let d = default_approve_argv();
        assert!(
            !d.iter().any(|a| a[0] == "op" || a[0] == "theseusd"),
            "{d:?}"
        );
        assert!(d.contains(&vec!["sudo".to_string()]), "{d:?}");
        assert_eq!(
            crate::policy::floor_argv(),
            argvs(&[&["theseusd"], &["op"]])
        );
    }

    /// The template holds no copy of the prices (theseus-vwar): its one
    /// [catalog] table is the commented new model, so a config from it runs
    /// every model at the code's figures, which `theseus catalog` lists, and
    /// the code's next fix of a price reaches it.
    #[test]
    fn the_template_holds_no_price_table() {
        use crate::catalog::Catalog;
        let cfg = Config::example();
        assert!(cfg.catalog.is_empty(), "{:?}", cfg.catalog.keys());
        let (c, code) = (Catalog::with_overrides(&cfg.catalog), Catalog::builtin());
        assert_eq!((c.version, c.entries), (code.version, code.entries));
    }

    /// A [catalog] table over a built-in model may name one price; a model
    /// the built-in table lacks must name every figure; a price is dollars.
    #[test]
    fn a_catalog_table_names_what_it_changes_and_a_new_model_names_everything() {
        let base = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n";
        let (cfg, _) = Config::parse(&format!(
            "{base}[catalog.\"claude-sonnet-5-5\"]\ncache_read_per_mtok = 0.1\n"
        ))
        .unwrap();
        let c = crate::catalog::Catalog::with_overrides(&cfg.catalog);
        let s = c.get("claude-sonnet-5-5").unwrap();
        assert_eq!((s.cache_read_per_mtok, s.output_per_mtok), (0.1, 10.0));
        let e = format!(
            "{:#}",
            Config::parse(&format!(
                "{base}[catalog.\"mystery\"]\ninput_per_mtok = 1.0\n"
            ))
            .unwrap_err()
        );
        assert!(
            e.contains("catalog.\"mystery\" is not a built-in model")
                && e.contains("provider, context_window, max_output_tokens, output_per_mtok"),
            "{e}"
        );
        let e = format!(
            "{:#}",
            Config::parse(&format!(
                "{base}[catalog.\"glm-5.3\"]\noutput_per_mtok = -1.0\n"
            ))
            .unwrap_err()
        );
        assert!(e.contains("catalog.\"glm-5.3\".output_per_mtok"), "{e}");
        assert!(Config::parse(&format!("{base}[catalog.\"glm-5.3\"]\nprice = 1.0\n")).is_err());
    }

    /// `[approval]` is retired (theseus-zmgb). Eddie's vault config without it
    /// loads with no warning, and `theseusd config` prints no such section;
    /// the template has none. A config that still has one, his three lines
    /// among them, loads with one warning that it is retired, whatever it
    /// holds, and prints none.
    #[test]
    fn an_approval_section_is_retired_and_loads_with_a_warning() {
        let vault = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n\
                     [kernel]\nadmission_ceiling = 8\nheartbeat_secs = 60\n\n\
                     [discord]\nenabled = true\ntoken_secret = \"discord_bot_token\"\n\
                     bindings_file = \"bindings.toml\"\nedit_interval_ms = 1200\n\n\
                     [web]\nenabled = true\nbind = \"127.0.0.1\"\nport = 7433\n";
        let (cfg, warnings) = Config::parse(vault).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(cfg.approval.is_none());
        assert!(Config::example().approval.is_none());
        assert!(
            !Config::EXAMPLE_TOML.contains("[approval]"),
            "the template has no section"
        );
        for section in [
            "trusted_users = [\"discord:271828182845904523\"]\nchannels = [\"cli\", \"web\", \"discord:dm\"]",
            "",
            "channels = [\"discord:general\"]\nusers = []",
        ] {
            let (cfg, w) = Config::parse(&format!("{vault}\n[approval]\n{section}\n")).unwrap();
            assert_eq!(w, [APPROVAL_RETIRED], "{section}");
            assert!(cfg.approval.is_some());
            let shown = toml::to_string_pretty(&cfg).unwrap();
            assert!(!shown.contains("approval"), "{shown}");
        }
    }

    /// `[web] bind` is a loopback address or the config fails to load, since
    /// the web UI has no auth yet (theseus-2fo). Eddie's `127.0.0.1` and the
    /// template's load unchanged.
    #[test]
    fn a_web_bind_off_loopback_fails_to_load() {
        let doc = |bind: &str| {
            format!(
                "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n[web]\nenabled = false\n\
                 bind = \"{bind}\"\nport = 7433\n"
            )
        };
        for ok in ["127.0.0.1", "127.0.0.2", "::1"] {
            Config::parse(&doc(ok)).unwrap_or_else(|e| panic!("{ok}: {e:#}"));
        }
        for bad in [
            "0.0.0.0",
            "192.168.1.20",
            "::",
            "localhost",
            "example.com",
            "",
        ] {
            let e = format!("{:#}", Config::parse(&doc(bad)).unwrap_err());
            assert!(
                e.contains(&format!("web.bind = {bad:?} is not a loopback address"))
                    && e.contains("the web UI has no auth yet"),
                "{bad}: {e}"
            );
        }
        assert_eq!(Config::example().web.bind, "127.0.0.1");
    }

    /// `[web] dev_origin` (theseus-zab) is off unless set, and when set it is
    /// a dev page on this machine or the config fails to load. A config
    /// without it, as Eddie's is, loads unchanged.
    #[test]
    fn a_dev_origin_is_off_by_default_and_a_page_on_this_machine() {
        let doc = |line: &str| {
            format!(
                "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n[web]\nenabled = true\n\
                 bind = \"127.0.0.1\"\nport = 7433\n{line}\n"
            )
        };
        assert_eq!(Config::parse(&doc("")).unwrap().0.web.dev_origin, None);
        assert_eq!(Config::example().web.dev_origin, None);
        assert_eq!(WebConfig::default().dev_origin, None);
        for ok in [
            "http://localhost:5173",
            "http://LocalHost:5173",
            "http://127.0.0.1:5173",
            "http://127.0.0.2:4173",
            "http://[::1]:5173",
        ] {
            let (cfg, _) = Config::parse(&doc(&format!("dev_origin = \"{ok}\"")))
                .unwrap_or_else(|e| panic!("{ok}: {e:#}"));
            assert_eq!(cfg.web.dev_origin.as_deref(), Some(ok));
        }
        for bad in [
            "https://localhost:5173",
            "http://localhost",
            "http://localhost:",
            "http://localhost:0",
            "http://localhost:70000",
            "http://localhost:5173/",
            "http://localhost:5173/ws",
            "http://user@localhost:5173",
            "http://evil.example:5173",
            "http://localhost.evil.example:5173",
            "http://0.0.0.0:5173",
            "http://192.168.1.20:5173",
            "http://::1:5173",
            "http://[::]:5173",
            "localhost:5173",
            "null",
            "",
        ] {
            let e = format!(
                "{:#}",
                Config::parse(&doc(&format!("dev_origin = \"{bad}\""))).unwrap_err()
            );
            assert!(
                e.contains(&format!(
                    "web.dev_origin = {bad:?} is not a dev page's origin"
                )),
                "{bad}: {e}"
            );
        }
    }

    /// The fields that could carry a credential are the URLs: a user, a
    /// password, or a query in one names its field, and such a note is never
    /// kept as a copy (theseus-2fo). The template has none.
    #[test]
    fn a_url_that_could_carry_a_credential_is_named() {
        assert_eq!(Config::example().credential_in_url(), None);
        let with = |f: &dyn Fn(&mut Config)| {
            let mut c = Config::example();
            f(&mut c);
            c.credential_in_url()
        };
        assert_eq!(
            with(&|c| c.model.api_base = "https://user:pw@api.example.com".into()),
            Some("model.api_base".into())
        );
        assert_eq!(
            with(&|c| {
                c.providers.get_mut("zai").unwrap().api_base =
                    "https://api.z.ai/api/anthropic?key=x".into()
            }),
            Some("providers.zai.api_base".into())
        );
        assert_eq!(
            with(&|c| c.telemetry.otlp_endpoint = Some("http://tok@127.0.0.1:4318".into())),
            Some("telemetry.otlp_endpoint".into())
        );
        assert_eq!(
            with(&|c| c.model.api_base = "https://api.anthropic.com/v1".into()),
            None
        );
    }

    /// `cache_ttl` (theseus-ev1) is "5m" or "1h", on a profile and on
    /// `[model]`, whose implicit profile carries it. The default, 5 minutes,
    /// serializes as nothing, and any other value is refused.
    #[test]
    fn cache_ttl_is_five_minutes_or_an_hour_per_profile() {
        let on_sonnet = Config::EXAMPLE_TOML
            .replace("\n[profiles.glm]", "\ncache_ttl = \"1h\"\n\n[profiles.glm]");
        let (cfg, _) = Config::parse(&on_sonnet).unwrap();
        assert_eq!(cfg.profile("sonnet").unwrap().cache_ttl, CacheTtl::OneHour);
        assert_eq!(cfg.profile("glm").unwrap().cache_ttl, CacheTtl::FiveMinutes);
        assert_eq!(
            cfg.profile("default").unwrap().cache_ttl,
            CacheTtl::FiveMinutes
        );
        let text = toml::to_string(cfg.profile("glm").unwrap()).unwrap();
        assert!(!text.contains("cache_ttl"), "{text}");
        let text = toml::to_string(cfg.profile("sonnet").unwrap()).unwrap();
        assert!(text.contains("cache_ttl = \"1h\""), "{text}");

        let on_model = Config::EXAMPLE_TOML
            .replace("live = \"sonnet\"", "live = \"sonnet\"\ncache_ttl = \"1h\"");
        let (cfg, _) = Config::parse(&on_model).unwrap();
        assert_eq!(cfg.profile("default").unwrap().cache_ttl, CacheTtl::OneHour);
        assert_eq!(
            cfg.profile("sonnet").unwrap().cache_ttl,
            CacheTtl::FiveMinutes
        );

        let wrong = Config::EXAMPLE_TOML.replace(
            "\n[profiles.glm]",
            "\ncache_ttl = \"10m\"\n\n[profiles.glm]",
        );
        let e = format!("{:#}", Config::parse(&wrong).unwrap_err());
        assert!(e.contains("cache_ttl") || e.contains("10m"), "{e}");
        assert_eq!(
            (CacheTtl::FiveMinutes.marker(), CacheTtl::OneHour.marker()),
            (
                serde_json::json!({"type": "ephemeral"}),
                serde_json::json!({"type": "ephemeral", "ttl": "1h"})
            )
        );
        assert!(CacheTtl::FiveMinutes < CacheTtl::OneHour);
    }

    /// `[aws.accounts.<id>]` (row 29, C1): an account is its 12 digits, its
    /// key's two `[secrets]` entries, a region, and the regions a call may
    /// name; a stand-in endpoint only on this machine. Anything else fails to
    /// load, and the error says which. The template binds none.
    #[test]
    fn an_aws_account_names_its_id_its_key_and_its_regions() {
        assert!(
            Config::example().aws.is_empty(),
            "the template binds no account"
        );
        let with = |aws: &str| {
            Config::parse(&format!(
                "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\
                 aws_access_key_id = \"op://v/k/notesPlain#AWS_ACCESS_KEY_ID\"\n\
                 aws_secret_access_key = \"op://v/k/notesPlain#AWS_SECRET_ACCESS_KEY\"\n\
                 other_key = \"op://v/o/f\"\n\n{aws}\n"
            ))
            .map(|(c, _)| c)
        };
        let ok = with("[aws.accounts.111122223333]\nregion = \"us-west-2\"").unwrap();
        let a = &ok.aws.accounts["111122223333"];
        assert_eq!(a.credentials, AwsCredentialNames::default());
        assert_eq!(a.allowed_regions(), ["us-west-2"]);
        assert_eq!(a.endpoint, None);
        let named = with(
            "[aws.accounts.444455556666]\nregion = \"eu-central-1\"\n\
             regions = [\"eu-central-1\", \"us-east-1\"]\n\
             credentials = { access_key_id = \"other_key\", secret_access_key = \"other_key\" }\n\
             endpoint = \"http://127.0.0.1:4566\"",
        )
        .unwrap();
        let a = &named.aws.accounts["444455556666"];
        assert_eq!(a.credentials.access_key_id, "other_key");
        assert_eq!(a.allowed_regions(), ["eu-central-1", "us-east-1"]);
        for (bad, says) in [
            ("[aws.accounts.home]\nregion = \"us-west-2\"", "12 digits"),
            (
                "[aws.accounts.11112222333]\nregion = \"us-west-2\"",
                "12 digits",
            ),
            (
                "[aws.accounts.111122223333]\nregion = \"us-west-2\"\n\
                 credentials = { access_key_id = \"nope\" }",
                "credentials.access_key_id = \"nope\" has no matching entry under [secrets]",
            ),
            (
                "[aws.accounts.111122223333]\nregion = \"US-West-2\"",
                "not a region's name",
            ),
            (
                "[aws.accounts.111122223333]\nregion = \"west\"",
                "not a region's name",
            ),
            (
                "[aws.accounts.111122223333]\nregion = \"us-west-2\"\nregions = [\"us-east-1\"]",
                "is not one of its regions: us-east-1",
            ),
            (
                "[aws.accounts.111122223333]\nregion = \"us-west-2\"\n\
                 endpoint = \"https://sts.us-west-2.amazonaws.com\"",
                "is not a stand-in on this machine",
            ),
            (
                "[aws.accounts.111122223333]\nregion = \"us-west-2\"\nprofile = \"default\"",
                "unknown field `profile`",
            ),
            ("[aws.accounts.111122223333]", "missing field `region`"),
            ("[aws]\nregion = \"us-west-2\"", "unknown field `region`"),
        ] {
            let e = format!("{:#}", with(bad).unwrap_err());
            assert!(e.contains(says), "{bad}: {e}");
        }
    }

    /// `[policy.aws]` (AWS design §3.9): a class (`read`, `write`, `run`), a
    /// service, or a service and an operation, as aws.describe names them.
    #[test]
    fn policy_aws_takes_read_a_service_or_an_operation() {
        let cfg = policy_only(
            "[policy.aws]\nread = \"open\"\nwrite = \"notify\"\nrun = \"approve\"\nec2 = \"notify\"\n\
             \"s3:ListBuckets\" = \"approve\"\nresource-groups = \"open\"",
        )
        .unwrap();
        assert_eq!(cfg.policy.aws["read"], Posture::Open);
        assert_eq!(cfg.policy.aws["write"], Posture::Notify);
        assert_eq!(cfg.policy.aws["run"], Posture::Approve);
        assert_eq!(cfg.policy.aws["ec2"], Posture::Notify);
        assert_eq!(cfg.policy.aws["s3:ListBuckets"], Posture::Approve);
        assert!(policy_only("").unwrap().policy.aws.is_empty());
        for (bad, says) in [
            (
                "\"EC2\" = \"open\"",
                "is not a class (`read`, `write`, `run`), a service",
            ),
            ("\"s3:listBuckets\" = \"open\"", "is not a class"),
            ("\"s3:\" = \"open\"", "is not a class"),
            ("read = \"never\"", "open"),
        ] {
            let e = format!(
                "{:#}",
                policy_only(&format!("[policy.aws]\n{bad}")).unwrap_err()
            );
            assert!(e.contains(says), "{bad}: {e}");
        }
    }

    /// A `[policy.tools]` line loads for every tool: the toollets', and the
    /// core's own, the web's, a task's, a wake's, and AWS's. Until row 29 a
    /// config with the template's `task.create` or `wake.at` line uncommented
    /// failed to load.
    #[test]
    fn a_policy_tools_line_loads_for_every_tool() {
        let names: Vec<String> = theseus_tools::default_registry()
            .all()
            .map(|t| t.name().to_string())
            .chain(crate::web::NAMES.map(String::from))
            .chain(crate::task::NAMES.map(String::from))
            .chain(crate::wake::NAMES.map(String::from))
            .chain(crate::aws::NAMES.map(String::from))
            .collect();
        for n in &names {
            policy_only(&format!("[policy.tools]\n\"{n}\" = \"approve\""))
                .unwrap_or_else(|e| panic!("{n}: {e:#}"));
        }
        let e = format!(
            "{:#}",
            policy_only("[policy.tools]\n\"aws.nope\" = \"open\"").unwrap_err()
        );
        assert!(
            e.contains("is not a tool") && e.contains("aws.s3.list"),
            "{e}"
        );
    }

    /// Every key the code can read appears in the template (set or commented),
    /// so a new field cannot be added without documenting it.
    #[test]
    fn example_template_mentions_every_key() {
        let built = toml::Value::try_from(Config::example()).unwrap();
        let mut missing = Vec::new();
        walk(&built, "", "", &mut |path, key| {
            // A map key such as `"fs.read"` appears quoted.
            if !Config::EXAMPLE_TOML.contains(&format!("{key} ="))
                && !Config::EXAMPLE_TOML.contains(&format!("\"{key}\""))
            {
                missing.push(path.to_string());
            }
        });
        // Optional fields that serialize as absent still need a commented line.
        for must in [
            "system =",
            "otlp_endpoint =",
            "headers_secret =",
            "[policy.mcp]",
            "default_persona =",
            "[personas.",
            "[broker.programs.",
            "[broker.secrets.",
            "posture =",
            "cache_ttl =",
            "caches =",
            "[aws.accounts.",
            "credentials =",
            "access_key_id =",
            "secret_access_key =",
            "region =",
            "regions =",
            "endpoint =",
            "[policy.aws]",
        ] {
            assert!(
                Config::EXAMPLE_TOML.contains(must),
                "template must document `{must}`"
            );
        }
        assert!(missing.is_empty(), "undocumented config keys: {missing:?}");
    }

    fn walk(v: &toml::Value, path: &str, key: &str, f: &mut dyn FnMut(&str, &str)) {
        match v {
            toml::Value::Table(t) => {
                for (k, v) in t {
                    let p = if path.is_empty() {
                        k.clone()
                    } else {
                        format!("{path}.{k}")
                    };
                    walk(v, &p, k, f);
                }
            }
            _ => f(path, key),
        }
    }
}
