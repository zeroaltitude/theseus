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
    /// assembles. You pay only for tokens actually produced.
    #[serde(default = "default_max_tokens", alias = "max_tokens")]
    pub max_output_tokens: u32,
    #[serde(default)]
    pub system: Option<String>,
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
    #[serde(default = "default_max_tokens", alias = "max_tokens")]
    pub max_output_tokens: u32,
    #[serde(default)]
    pub system: Option<String>,
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
            max_output_tokens: default_max_tokens(),
            system: None,
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
