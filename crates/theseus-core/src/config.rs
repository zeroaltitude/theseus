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
    /// Workspace roots: every path a tool touches must be under one.
    #[serde(default = "default_tool_roots")]
    pub roots: Vec<String>,
    /// Where relative paths resolve and programs run by default (default: the first root).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Denied even under a root.
    #[serde(default = "default_deny_paths")]
    pub deny_paths: Vec<String>,
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

fn default_tool_roots() -> Vec<String> {
    vec!["~/projects".into()]
}
fn default_deny_paths() -> Vec<String> {
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
            roots: default_tool_roots(),
            cwd: None,
            deny_paths: default_deny_paths(),
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

/// `[policy]`: the gate's three bands per tool class (§3.9).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyConfig {
    #[serde(default = "mode_allow")]
    pub read: crate::policy::Mode,
    #[serde(default = "mode_confirm")]
    pub write: crate::policy::Mode,
    #[serde(default = "mode_confirm")]
    pub run: crate::policy::Mode,
    /// `proc.run` argv prefixes that run without confirmation.
    #[serde(default = "default_allow_argv")]
    pub allow_argv: Vec<Vec<String>>,
    /// `proc.run` argv prefixes that never run.
    #[serde(default = "default_deny_argv")]
    pub deny_argv: Vec<Vec<String>>,
    /// Per-tool overrides by canonical name, e.g. `"fs.edit" = "allow"`.
    #[serde(default)]
    pub overrides: BTreeMap<String, crate::policy::Mode>,
}

fn mode_allow() -> crate::policy::Mode {
    crate::policy::Mode::Allow
}
fn mode_confirm() -> crate::policy::Mode {
    crate::policy::Mode::Confirm
}
fn argvs(v: &[&[&str]]) -> Vec<Vec<String>> {
    v.iter()
        .map(|a| a.iter().map(|s| s.to_string()).collect())
        .collect()
}
fn default_allow_argv() -> Vec<Vec<String>> {
    argvs(&[
        &["git", "status"],
        &["git", "diff"],
        &["git", "log"],
        &["git", "show"],
        &["ls"],
        &["pwd"],
    ])
}
fn default_deny_argv() -> Vec<Vec<String>> {
    argvs(&[
        &["sudo"],
        &["su"],
        &["doas"],
        &["rm", "-rf", "/"],
        &["mkfs"],
        &["dd"],
        &["shutdown"],
        &["reboot"],
        &["op"],
        &["theseusd"],
    ])
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            read: mode_allow(),
            write: mode_confirm(),
            run: mode_confirm(),
            allow_argv: default_allow_argv(),
            deny_argv: default_deny_argv(),
            overrides: BTreeMap::new(),
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
    "claude-sonnet-5".into()
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
        let cfg: Config = toml::from_str(&text).context("parsing config TOML")?;
        cfg.validate()?;
        Ok(cfg)
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
            .chain(&self.policy.deny_argv)
            .enumerate()
        {
            if argv.is_empty() {
                anyhow::bail!("policy argv entry {k} is empty");
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
                            !k.is_empty()
                                && k.chars()
                                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
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
    }

    /// Every key the code can read appears in the template (set or commented),
    /// so a new field cannot be added without documenting it.
    #[test]
    fn example_template_mentions_every_key() {
        let built = toml::Value::try_from(Config::example()).unwrap();
        let mut missing = Vec::new();
        walk(&built, "", &mut |path, leaf| {
            if !leaf {
                return;
            }
            let key = path.rsplit('.').next().unwrap();
            if !Config::EXAMPLE_TOML.contains(&format!("{key} =")) {
                missing.push(path.to_string());
            }
        });
        // Optional fields that serialize as absent still need a commented line.
        for must in ["system", "otlp_endpoint", "headers_secret"] {
            assert!(
                Config::EXAMPLE_TOML.contains(&format!("# {must} ="))
                    || Config::EXAMPLE_TOML.contains(&format!("{must} =")),
                "template must document `{must}`"
            );
        }
        assert!(missing.is_empty(), "undocumented config keys: {missing:?}");
    }

    fn walk(v: &toml::Value, path: &str, f: &mut dyn FnMut(&str, bool)) {
        match v {
            toml::Value::Table(t) => {
                for (k, v) in t {
                    let p = if path.is_empty() {
                        k.clone()
                    } else {
                        format!("{path}.{k}")
                    };
                    walk(v, &p, f);
                }
            }
            _ => f(path, true),
        }
    }
}
