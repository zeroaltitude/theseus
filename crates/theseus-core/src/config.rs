//! Configuration. A TOML document, stored as a 1Password item so a deployment
//! is reconstructible from the vault, or a local file for development. Secret
//! fields are `op://vault/item/field` references, never values.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::secrets::{OpReader, SecretRef};

pub const DEFAULT_CONFIG_REF: &str = "op://Eddie-Tabitha/theseus-config/notesPlain";

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
    /// Additional Anthropic-Messages-compatible endpoints by name (e.g. `zai`).
    /// The implicit `anthropic` provider comes from `[model]` unless overridden here.
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderConfig>,
    /// name → op:// reference. Every entry must resolve or the process refuses to start.
    #[serde(default)]
    pub secrets: BTreeMap<String, String>,
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
    /// Every model's prices (the template lists each built-in model): a table
    /// over a built-in model replaces the fields it names; a complete one adds
    /// a model. A built-in model with no table keeps its built-in prices.
    #[serde(default)]
    pub catalog: BTreeMap<String, crate::catalog::CatalogRow>,
    #[serde(default)]
    pub tools: ToolsConfig,
    #[serde(default)]
    pub policy: PolicyConfig,
    #[serde(default)]
    pub discord: DiscordConfig,
    /// Who may answer a waiting call, and through which channels. Absent (as
    /// in a config from before theseus-sgh): no rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<ApprovalConfig>,
    /// The 1Password token file this daemon was pointed at (`--op-token-file`
    /// or `THESEUS_OP_TOKEN_FILE`): set at startup, never read from the TOML.
    /// The floor keeps it, whichever way it was named (theseus-8az).
    #[serde(skip)]
    pub op_token_file: Option<PathBuf>,
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
        }
    }
}

/// `[approval]` (spec §3.9 "Approval", theseus-sgh): an answer to a waiting
/// call counts only from a trusted user through a trusted channel. Without
/// the section there is no rule: the CLI, the local web UI, and a place's
/// listed Discord users answer, as before.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalConfig {
    /// Who may answer, as surface-qualified ids: `discord:<user id>`. The CLI
    /// and the web UI need no entry: whoever reaches them is this machine's
    /// operator. Default: nobody on Discord.
    #[serde(default)]
    pub trusted_users: Vec<String>,
    /// Where an approval dialogue may happen: `cli`, `web`, `discord:dm` (a
    /// DM between the bot and a trusted user), or `discord:<channel id>` (a
    /// guild channel that only trusted users can view). Default: the CLI and
    /// the web UI.
    #[serde(default = "default_approval_channels")]
    pub channels: Vec<String>,
}

fn default_approval_channels() -> Vec<String> {
    vec!["cli".into(), "web".into()]
}

impl DiscordConfig {
    pub fn bindings_path(&self, state_dir: &std::path::Path) -> PathBuf {
        let p = PathBuf::from(shellexpand::tilde(&self.bindings_file).into_owned());
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
    /// More workspace roots beside `projects_dir`; every path a tool touches must be under one.
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
        }
    }
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

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            enforcement: Default::default(),
            allow_argv: default_allow_argv(),
            approve_argv: default_approve_argv(),
            tools: BTreeMap::new(),
            mcp: BTreeMap::new(),
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
    /// How long a confirmation stays valid.
    #[serde(default = "default_confirm_ttl_secs")]
    pub confirm_ttl_secs: u64,
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
        }
    }
}

/// The localhost web UI. Bound to loopback only; no auth yet (spec §3.14).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_web_bind")]
    pub bind: String,
    #[serde(default = "default_web_port")]
    pub port: u16,
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
        }
    }
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
    /// Files compiled into the system block after `system`, each under a
    /// header naming it (theseus-58a): `~/` or absolute paths. Omitted:
    /// `[model].context_files`; `[]`: none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_files: Option<Vec<String>>,
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
    /// The context files of every profile that names none (theseus-58a).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<Effort>,
    #[serde(default)]
    pub thinking_display: ThinkingDisplay,
    #[serde(default = "default_max_loops")]
    pub max_loops: u32,
    #[serde(default = "default_true")]
    pub refusal_fallbacks: bool,
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
}

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
fn default_state_dir() -> String {
    "~/.theseus".into()
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
            context_files: Vec::new(),
            effort: None,
            thinking_display: ThinkingDisplay::Summarized,
            max_loops: default_max_loops(),
            refusal_fallbacks: true,
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
        }
    }
}

impl Config {
    /// `source` is either an `op://` reference or a filesystem path.
    pub async fn load(source: &str, op: &OpReader) -> Result<Self> {
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
        Ok(cfg)
    }

    /// Parse and validate a config document. The warnings name the retired
    /// keys it still uses; the caller logs them at startup.
    pub fn parse(text: &str) -> Result<(Self, Vec<String>)> {
        let cfg = Self::from_toml(text).context("parsing config TOML")?;
        cfg.validate()?;
        let mut warnings = Vec::new();
        if !cfg!(feature = "otel") && cfg.telemetry.endpoint().is_some() {
            warnings.push(
                "telemetry.otlp_endpoint is set, but this build has no OTLP export (theseus-0g4: \
                 build with the `otel` cargo feature); nothing is exported"
                    .into(),
            );
        }
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
        if let Some(a) = &cfg.approval {
            if a.channels.is_empty() {
                warnings.push(
                    "approval.channels is empty, so no answer counts: a call that waits for \
                     approval waits until it expires, and a budget question until a new message \
                     replaces it"
                        .into(),
                );
            } else if a.trusted_users.is_empty()
                && a.channels.iter().any(|c| c.starts_with("discord:"))
            {
                warnings.push(
                    "approval.channels lists Discord, but approval.trusted_users names nobody, \
                     so no Discord answer counts"
                        .into(),
                );
            }
        }
        Ok((cfg, warnings))
    }

    pub fn validate(&self) -> Result<()> {
        for (name, r) in &self.secrets {
            SecretRef::parse(r)
                .with_context(|| format!("secrets.{name} is not a valid op:// reference"))?;
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
        // Paths only: the files are read when a turn compiles, never here.
        let files = std::iter::once(("model".to_string(), &self.model.context_files)).chain(
            self.profiles.iter().filter_map(|(name, p)| {
                p.context_files
                    .as_ref()
                    .map(|f| (format!("profiles.{name}"), f))
            }),
        );
        for (key, list) in files {
            if let Some(f) = list
                .iter()
                .find(|f| !(f.starts_with('/') || f.starts_with("~/")))
            {
                anyhow::bail!(
                    "{key}.context_files entry {f:?} must be an absolute path or start with ~/"
                );
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
        for name in self.policy.tools.keys() {
            let known = match name.strip_prefix(crate::policy::MCP_PREFIX) {
                Some(rest) => rest.contains('/') && mcp_key(rest),
                None => registry.get(name).is_some(),
            };
            if !known {
                let names: Vec<&str> = registry.all().map(|t| t.name()).collect();
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
        if let Some(a) = &self.approval {
            for u in &a.trusted_users {
                crate::approval::parse_user(u).map_err(anyhow::Error::msg)?;
            }
            for c in &a.channels {
                crate::approval::Channel::parse(c).map_err(anyhow::Error::msg)?;
            }
        }
        Ok(())
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
                context_files: None,
                effort: m.effort,
                thinking_display: m.thinking_display,
                max_loops: m.max_loops,
                refusal_fallbacks: m.refusal_fallbacks,
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

    /// A profile's context files: its own list, else `[model].context_files`.
    pub fn context_files_for<'a>(&'a self, prof: &'a ProfileConfig) -> &'a [String] {
        prof.context_files
            .as_deref()
            .unwrap_or(&self.model.context_files)
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

pub fn expand(p: &str) -> PathBuf {
    PathBuf::from(shellexpand::tilde(p).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::Posture;

    #[test]
    fn example_template_parses_and_validates() {
        let cfg = Config::example();
        cfg.validate().unwrap();
        assert_eq!(cfg.model.live, "sonnet");
        assert!(cfg.profiles.contains_key("glm"));
        assert!(cfg.providers.contains_key("zai"));
        assert_eq!(cfg.telemetry.otlp_endpoint, None);
    }

    /// Every commented-out parameter must be a real parameter with a valid
    /// value: un-comment them all and the result must still parse under
    /// deny_unknown_fields. This is what stops the template from lying.
    #[test]
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
        // The commented tool lines and the [policy.mcp] example are real too.
        assert_eq!(cfg.policy.tools["proc.run"], Posture::Approve);
        assert_eq!(cfg.policy.tools.len(), 11);
        assert_eq!(cfg.policy.mcp["some-server"], Posture::Notify);
        assert_eq!(cfg.policy.mcp["some-server/read-only-tool"], Posture::Open);
        assert_eq!(
            cfg.policy.mcp["some-server/dangerous-tool"],
            Posture::Approve
        );
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
        // Reads stay quiet out of the box; everything else inherits.
        let set = Config::example().policy.tools;
        let reads = [
            "fs.read",
            "fs.glob",
            "fs.grep",
            "fs.list",
            "git.diff",
            "git.log",
            "text.diff",
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

    /// `[telemetry]` parses the same with or without the `otel` feature. A
    /// build without it warns once if an endpoint is set, since nothing is
    /// exported (theseus-0g4); a blank endpoint counts as unset. Eddie's note
    /// sets none, so it gets no new warning.
    #[test]
    fn an_otlp_endpoint_warns_once_in_a_build_without_otel() {
        let base = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n[telemetry]\n";
        let (cfg, warnings) = Config::parse(&format!(
            "{base}otlp_endpoint = \"http://127.0.0.1:4318\"\n"
        ))
        .unwrap();
        assert_eq!(cfg.telemetry.endpoint(), Some("http://127.0.0.1:4318"));
        if cfg!(feature = "otel") {
            assert!(warnings.is_empty(), "{warnings:?}");
        } else {
            assert_eq!(warnings.len(), 1, "{warnings:?}");
            assert!(
                warnings[0].starts_with(
                    "telemetry.otlp_endpoint is set, but this build has no OTLP export"
                ),
                "{warnings:?}"
            );
        }
        for quiet in ["", "otlp_endpoint = \"  \"\n"] {
            let (_, w) =
                Config::parse(&format!("{base}service_name = \"theseus\"\n{quiet}")).unwrap();
            assert!(w.is_empty(), "{w:?}");
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

    /// Context files (theseus-58a): a profile that names none takes
    /// `[model].context_files`, `[]` names none, and a path must be absolute
    /// or start with `~/`. Loading checks spelling only: nothing is read. The
    /// template sets `[]` at `[model]`, keeps the profile's line and the
    /// `roots` example commented, and every one of those lines parses.
    #[test]
    fn context_files_default_from_model_and_must_be_absolute_or_home_paths() {
        let base = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n\
                    [model]\ncontext_files = [\"~/w/SOUL.md\", \"/nowhere/USER.md\"]\n\n\
                    [profiles.inherits]\nmodel = \"m\"\n\n\
                    [profiles.own]\nmodel = \"m\"\ncontext_files = [\"/x/RULES.md\"]\n\n\
                    [profiles.none]\nmodel = \"m\"\ncontext_files = []\n";
        let (cfg, warnings) = Config::parse(base).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let all = cfg.all_profiles();
        let files = |p: &str| cfg.context_files_for(&all[p]).to_vec();
        assert_eq!(files("default"), ["~/w/SOUL.md", "/nowhere/USER.md"]);
        assert_eq!(files("inherits"), files("default"));
        assert_eq!(files("own"), ["/x/RULES.md"]);
        assert!(files("none").is_empty());
        for (bad, key) in [
            (
                "[model]\ncontext_files = [\"SOUL.md\"]",
                "model.context_files",
            ),
            (
                "[profiles.p]\nmodel = \"m\"\ncontext_files = [\"~other/x\"]",
                "profiles.p.context_files",
            ),
            ("[model]\ncontext_files = [\"\"]", "model.context_files"),
        ] {
            let doc = format!("[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n{bad}\n");
            let e = format!("{:#}", Config::parse(&doc).unwrap_err());
            assert!(e.contains(key) && e.contains("absolute path"), "{e}");
        }
        // A config that never names context files has none, and says nothing new.
        let (plain, w) = Config::parse("[secrets]\nanthropic_api_key = \"op://v/i/f\"\n").unwrap();
        assert!(w.is_empty(), "{w:?}");
        assert!(plain
            .all_profiles()
            .values()
            .all(|p| plain.context_files_for(p).is_empty()));
        let shown = toml::to_string(&plain.model).unwrap();
        assert!(!shown.contains("context_files"), "{shown}");
        // The template.
        let t = Config::example();
        assert!(t.model.context_files.is_empty() && t.tools.roots.is_empty());
        assert!(t.profiles.values().all(|p| p.context_files.is_none()));
        let has = |line: &str| Config::EXAMPLE_TOML.lines().any(|l| l.starts_with(line));
        assert!(has("context_files = []"));
        assert!(has("# context_files = []"));
        assert!(has("# roots = [\"/home/zeroaltitude/reports\"]"));
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

    /// The template's [catalog] is the built-in catalog, model for model and
    /// price for price (theseus-0sg). Eddie pastes the template into the
    /// vault, so the config is where he reads prices; were a built-in price to
    /// change without the template, this fails and prints the tables to paste.
    #[test]
    fn the_template_catalog_is_the_builtin_catalog() {
        use crate::catalog::Catalog;
        let text = Config::EXAMPLE_TOML;
        let start = text
            .find("[catalog.\"")
            .expect("the template has catalog tables");
        let end = start
            + text[start..]
                .find("# A model the built-in table lacks")
                .expect("the template ends its tables with the new-model example");
        let want = Catalog::template_tables();
        assert!(
            text[start..end] == want,
            "the template's [catalog] tables are not the built-in catalog; replace them with:\n{want}"
        );
        let cfg = Config::example();
        let builtin = Catalog::builtin();
        assert_eq!(
            cfg.catalog.keys().collect::<Vec<_>>(),
            builtin.entries.keys().collect::<Vec<_>>(),
            "one table per built-in model"
        );
        assert!(Catalog::missing_from(&cfg.catalog).is_empty());
        let loaded = Catalog::with_overrides(&cfg.catalog);
        for (id, e) in &builtin.entries {
            let got = loaded.get(id).unwrap();
            assert_eq!(
                got,
                &crate::catalog::CatalogEntry {
                    source: "config".into(),
                    ..e.clone()
                },
                "{id}: the template changes nothing but where the price is read"
            );
        }
        assert!(loaded
            .version
            .ends_with(&format!("+config:{}", builtin.entries.len())));
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

    /// `[approval]` (theseus-sgh). Eddie's vault config (its sections checked
    /// 2026-09-29, by name only) has none: it loads with no rule and no new
    /// warning, and `theseusd config` prints no such section. The template
    /// keeps it commented, with Eddie's DM as the example, and each of its
    /// lines parses. A section sets only what it names: channels default to
    /// the CLI and the web UI, trusted users to nobody.
    #[test]
    fn approval_is_no_rule_unless_the_config_has_the_section() {
        let vault = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n\
                     [kernel]\nadmission_ceiling = 8\nheartbeat_secs = 60\n\n\
                     [discord]\nenabled = true\ntoken_secret = \"discord_bot_token\"\n\
                     bindings_file = \"bindings.toml\"\nedit_interval_ms = 1200\n\n\
                     [web]\nenabled = true\nbind = \"127.0.0.1\"\nport = 7433\n";
        let (cfg, warnings) = Config::parse(vault).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(cfg.approval.is_none());
        assert!(!crate::approval::Approval::new(cfg.approval.as_ref()).configured());
        let shown = toml::to_string_pretty(&cfg).unwrap();
        assert!(!shown.contains("approval"), "{shown}");
        assert!(
            Config::example().approval.is_none(),
            "the template leaves it commented"
        );
        let has = |line: &str| Config::EXAMPLE_TOML.lines().any(|l| l.starts_with(line));
        assert!(has("# [approval]"));
        assert!(has("# trusted_users = [\"discord:159471966640799744\"]"));
        assert!(has("# channels = [\"cli\", \"web\", \"discord:dm\"]"));

        let with = |section: &str| Config::parse(&format!("{vault}\n[approval]\n{section}\n"));
        let (cfg, w) = with("").unwrap();
        let a = cfg.approval.unwrap();
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(
            (a.trusted_users.len(), a.channels),
            (0, vec!["cli".to_string(), "web".to_string()])
        );
        let (cfg, w) = with(
            "trusted_users = [\"discord:159471966640799744\"]\nchannels = [\"cli\", \"web\", \"discord:dm\", \"discord:333333333333333333\"]",
        )
        .unwrap();
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(cfg.approval.unwrap().channels.len(), 4);
        for (bad, says) in [
            (
                "trusted_users = [\"eddie\"]",
                "approval.trusted_users entry \"eddie\" is not a surface-qualified id",
            ),
            (
                "trusted_users = [\"159471966640799744\"]",
                "write \"discord:<user id>\"",
            ),
            (
                "channels = [\"discord:general\"]",
                "approval.channels entry \"discord:general\" is not a channel",
            ),
            (
                "channels = [\"slack\"]",
                "\"discord:dm\", or \"discord:<channel id>\"",
            ),
            ("users = []", "unknown field `users`"),
        ] {
            let e = format!("{:#}", with(bad).unwrap_err());
            assert!(e.contains(says), "{bad}: {e}");
        }
        let (_, w) = with("channels = []").unwrap();
        assert!(
            w.len() == 1 && w[0].starts_with("approval.channels is empty, so no answer counts"),
            "{w:?}"
        );
        let (_, w) = with("channels = [\"discord:dm\"]").unwrap();
        assert!(
            w.len() == 1
                && w[0]
                    .contains("approval.trusted_users names nobody, so no Discord answer counts"),
            "{w:?}"
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
