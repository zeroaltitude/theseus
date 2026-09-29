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
    /// Model catalog rows that replace or add to the built-in table.
    #[serde(default)]
    pub catalog: BTreeMap<String, crate::catalog::CatalogEntry>,
    #[serde(default)]
    pub tools: ToolsConfig,
    #[serde(default)]
    pub policy: PolicyConfig,
    #[serde(default)]
    pub discord: DiscordConfig,
    /// The 1Password token file this daemon was pointed at (`--op-token-file`
    /// or `THESEUS_OP_TOKEN_FILE`): set at startup, never read from the TOML.
    /// The floor keeps it, whichever way it was named (theseus-8az).
    #[serde(skip)]
    pub op_token_file: Option<PathBuf>,
}

/// Keys renamed by theseus-8az, as (section, old, new): the old name still
/// loads (a serde alias), with a warning to rename it.
const RENAMED: &[(&str, &str, &str)] = &[
    ("tools", "deny_paths", "approve_paths"),
    ("policy", "deny_argv", "approve_argv"),
];

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
        }
    }
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
    /// Paths that wait for approval, even under a root (old name `deny_paths`).
    #[serde(default = "default_approve_paths", alias = "deny_paths")]
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
    /// inside the roots.
    #[serde(default = "default_allow_argv")]
    pub allow_argv: Vec<Vec<String>>,
    /// `proc.run` argv prefixes that wait for approval (old name `deny_argv`).
    #[serde(default = "default_approve_argv", alias = "deny_argv")]
    pub approve_argv: Vec<Vec<String>>,
    /// Per-tool postures by canonical name, e.g. `"proc.run" = "approve"`.
    #[serde(default)]
    pub tools: BTreeMap<String, crate::policy::Posture>,
    /// Per-MCP postures: `"server"` (every tool from that server) or
    /// `"server/tool"` (one tool), for tools named `mcp:<server>/<tool>`.
    #[serde(default)]
    pub mcp: BTreeMap<String, crate::policy::Posture>,
    /// The retired class keys (`read`, `write`, `run`; theseus-8az), still
    /// accepted so an older config loads, and never honored. Only the old
    /// template's words (`allow`, `confirm`) load: `validate` fails any other
    /// value, since dropping it silently could loosen a stated boundary.
    #[serde(default, skip_serializing)]
    pub read: Option<String>,
    #[serde(default, skip_serializing)]
    pub write: Option<String>,
    #[serde(default, skip_serializing)]
    pub run: Option<String>,
}

impl PolicyConfig {
    /// The retired class keys this config still sets, with their values.
    pub fn retired(&self) -> Vec<(&'static str, &str)> {
        [
            ("read", &self.read),
            ("write", &self.write),
            ("run", &self.run),
        ]
        .into_iter()
        .filter_map(|(k, v)| v.as_deref().map(|v| (k, v)))
        .collect()
    }
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
            read: None,
            write: None,
            run: None,
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
    /// Budget units (tokens today) a new session's execution gets.
    #[serde(default = "default_budget")]
    pub default_budget: u64,
    /// Units kept back for control and cleanup (cancel, final report).
    #[serde(default = "default_control_reserve")]
    pub control_reserve: u64,
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
fn default_budget() -> u64 {
    1_000_000
}
fn default_control_reserve() -> u64 {
    10_000
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
            default_budget: default_budget(),
            control_reserve: default_control_reserve(),
            heartbeat_secs: default_heartbeat_secs(),
            default_deadline_secs: default_deadline_secs(),
            confirm_ttl_secs: default_confirm_ttl_secs(),
        }
    }
}

impl KernelSection {
    pub fn to_kernel_config(&self) -> theseus_kernel::KernelConfig {
        theseus_kernel::KernelConfig {
            admission_ceiling: self.admission_ceiling.max(1),
            default_deadline_ms: self.default_deadline_secs * 1000,
            default_budget: self.default_budget,
            control_reserve: self.control_reserve,
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
    /// Wire protocol. Only `anthropic_messages` exists today.
    #[serde(default = "default_provider_kind")]
    pub kind: String,
    #[serde(default)]
    pub timeouts: Option<crate::provider::Timeouts>,
}

fn default_provider_kind() -> String {
    "anthropic_messages".into()
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
    #[serde(default, alias = "max_tokens", skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(default)]
    pub system: Option<String>,
    /// `low`, `medium`, `high`, `xhigh`, or `max`, for models that accept effort.
    /// Omitted: the model's default (Opus 5.5: medium; others: high).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// What thinking blocks carry: `summarized`, `omitted`, or `updates`.
    #[serde(default = "default_thinking_display")]
    pub thinking_display: String,
    /// Tool loops per turn before the Advancer ends it.
    #[serde(default = "default_max_loops")]
    pub max_loops: u32,
    /// Server-side refusal fallbacks where the model supports them.
    #[serde(default = "default_true")]
    pub refusal_fallbacks: bool,
}

pub const EFFORTS: &[&str] = &["low", "medium", "high", "xhigh", "max"];
pub const THINKING_DISPLAYS: &[&str] = &["summarized", "omitted", "updates"];

fn default_thinking_display() -> String {
    "summarized".into()
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
    #[serde(default, alias = "max_tokens", skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(default)]
    pub system: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default = "default_thinking_display")]
    pub thinking_display: String,
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
    /// Index engine behind the WAL: "redb" (default; the M1 benchmark's pick) or "fjall".
    /// A store directory keeps the engine it was created with.
    #[serde(default = "default_store_engine")]
    pub store_engine: theseus_store::Engine,
}

fn default_store_engine() -> theseus_store::Engine {
    theseus_store::Engine::Redb
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
#[allow(dead_code)]
fn default_max_tokens() -> u32 {
    16_384
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
            effort: None,
            thinking_display: default_thinking_display(),
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
            store_engine: default_store_engine(),
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

    /// Parse and validate a config document. The warnings name the renamed
    /// and retired keys it still uses; the caller logs them at startup.
    pub fn parse(text: &str) -> Result<(Self, Vec<String>)> {
        let cfg: Config = toml::from_str(text).context("parsing config TOML")?;
        cfg.validate()?;
        // Serde's alias does not say which name a key came in under, so the
        // old names are looked up in the document itself; a document that
        // never spells one skips the second parse (startup stays FAST).
        let table: toml::Table = if RENAMED.iter().any(|(_, old, _)| text.contains(old)) {
            toml::from_str(text).unwrap_or_default()
        } else {
            toml::Table::new()
        };
        let mut warnings: Vec<String> = RENAMED
            .iter()
            .filter(|(section, old, _)| table.get(*section).and_then(|t| t.get(*old)).is_some())
            .map(|(section, old, new)| {
                format!(
                    "{section}.{old} is renamed {section}.{new} (theseus-8az): a match waits for \
                     your approval; rename the key"
                )
            })
            .collect();
        warnings.extend(cfg.policy.retired().into_iter().map(|(key, _)| {
            format!(
                "policy.{key} is retired and ignored (theseus-8az): every tool inherits \
                 [policy].enforcement unless [policy.tools] names it"
            )
        }));
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
            if p.kind != "anthropic_messages" {
                anyhow::bail!(
                    "providers.{name}.kind = {:?} is not supported (only anthropic_messages)",
                    p.kind
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
            if let Some(e) = &prof.effort {
                if !EFFORTS.contains(&e.as_str()) {
                    anyhow::bail!(
                        "profiles.{name}.effort = {e:?} is not one of {}",
                        EFFORTS.join(", ")
                    );
                }
            }
            if !THINKING_DISPLAYS.contains(&prof.thinking_display.as_str()) {
                anyhow::bail!(
                    "profiles.{name}.thinking_display = {:?} is not one of {}",
                    prof.thinking_display,
                    THINKING_DISPLAYS.join(", ")
                );
            }
            if prof.max_loops == 0 {
                anyhow::bail!("profiles.{name}.max_loops must be at least 1");
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
        for (key, value) in self.policy.retired() {
            if !matches!(value, "allow" | "confirm") {
                let lines: Vec<String> = registry
                    .all()
                    .filter(|t| t.class().as_str() == key)
                    .map(|t| format!("\"{}\" = \"approve\"", t.name()))
                    .collect();
                anyhow::bail!(
                    "policy.{key} = {value:?} is retired (theseus-8az); a tool's posture is \
                     open | notify | approve, set under [policy.tools] (to make these wait: {})",
                    lines.join(", ")
                );
            }
        }
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
        Ok(())
    }

    /// Every profile by name, with the implicit `default` synthesized from
    /// `[model]` unless `[profiles.default]` overrides it.
    pub fn all_profiles(&self) -> BTreeMap<String, ProfileConfig> {
        let mut all = self.profiles.clone();
        all.entry("default".into())
            .or_insert_with(|| ProfileConfig {
                provider: self.model.provider.clone(),
                model: self.model.model.clone(),
                max_output_tokens: self.model.max_output_tokens,
                system: self.model.system.clone(),
                effort: self.model.effort.clone(),
                thinking_display: self.model.thinking_display.clone(),
                max_loops: self.model.max_loops,
                refusal_fallbacks: self.model.refusal_fallbacks,
            });
        all
    }

    /// Every provider by name, with the implicit `anthropic` one synthesized
    /// from `[model]` unless `[providers.anthropic]` overrides it.
    pub fn all_providers(&self) -> BTreeMap<String, ProviderConfig> {
        let mut all = self.providers.clone();
        all.entry("anthropic".into())
            .or_insert_with(|| ProviderConfig {
                api_base: self.model.api_base.clone(),
                api_key_secret: self.model.api_key_secret.clone(),
                kind: default_provider_kind(),
                timeouts: None,
            });
        all
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
        toml::from_str(Self::EXAMPLE_TOML).expect("the bundled example config parses")
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
        let cfg: Config = toml::from_str(&out).unwrap_or_else(|e| {
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
        assert!(cfg.policy.retired().is_empty());
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
            let key = l.trim_start_matches("# ").split('=').next().unwrap().trim();
            assert!(!["read", "write", "run"].contains(&key), "retired key: {l}");
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
        assert!(cfg.policy.retired().is_empty() && warnings.is_empty());
        assert_eq!(policy_only("").unwrap().policy.enforcement, Posture::Open);
    }

    /// The live vault config's shape as of 2026-09-28 (checked by key name
    /// only): the old list names, the retired class keys at the old
    /// template's values, and no [policy.tools]. It loads, the lists keep
    /// their entries under the new names, and each old key gets a warning.
    #[test]
    fn the_live_config_shape_still_loads_with_a_warning_per_old_key() {
        let text = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n\
                    [tools]\nprojects_dir = \"/w\"\ndeny_paths = [\"~/.ssh\", \"~/.theseus\"]\n\n\
                    [policy]\nenforcement = \"notify\"\nread = \"allow\"\nwrite = \"confirm\"\nrun = \"confirm\"\n\
                    allow_argv = [[\"ls\"], [\"pwd\"]]\ndeny_argv = [[\"sudo\"], [\"op\"], [\"theseusd\"]]\n";
        let (cfg, warnings) = Config::parse(text).unwrap();
        assert_eq!(cfg.policy.enforcement, Posture::Notify);
        assert_eq!(cfg.tools.approve_paths, ["~/.ssh", "~/.theseus"]);
        assert_eq!(
            cfg.policy.approve_argv,
            argvs(&[&["sudo"], &["op"], &["theseusd"]])
        );
        let keys: Vec<&str> = cfg.policy.retired().iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, ["read", "write", "run"]);
        assert_eq!(warnings.len(), 5, "{warnings:?}");
        for (w, start) in warnings.iter().zip([
            "tools.deny_paths is renamed tools.approve_paths (theseus-8az)",
            "policy.deny_argv is renamed policy.approve_argv (theseus-8az)",
            "policy.read is retired and ignored",
            "policy.write is retired and ignored",
            "policy.run is retired and ignored",
        ]) {
            assert!(w.starts_with(start), "{w}");
        }
        // `theseusd config` shows the new names and nothing retired.
        let shown = toml::to_string(&cfg.policy).unwrap() + &toml::to_string(&cfg.tools).unwrap();
        assert!(shown.contains("approve_argv = ") && shown.contains("approve_paths = "));
        assert!(
            !shown.contains("deny") && !shown.contains("read =") && !shown.contains("confirm"),
            "{shown}"
        );
        // An old and a new name together set one key twice.
        let both = text.replace("[policy]\n", "[policy]\napprove_argv = [[\"x\"]]\n");
        let e = format!("{:#}", Config::parse(&both).unwrap_err());
        assert!(e.contains("duplicate"), "{e}");
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
        let e = format!("{:#}", policy_only("run = \"deny\"").unwrap_err());
        assert!(
            e.contains("policy.run = \"deny\" is retired")
                && e.contains("open | notify | approve")
                && e.contains("\"proc.run\" = \"approve\""),
            "{e}"
        );
        let e = format!("{:#}", policy_only("write = \"deny\"").unwrap_err());
        for w in ["fs.edit", "fs.write", "fs.patch"] {
            assert!(e.contains(&format!("\"{w}\" = \"approve\"")), "{e}");
        }
        assert!(policy_only("read = \"bogus\"").is_err());
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
