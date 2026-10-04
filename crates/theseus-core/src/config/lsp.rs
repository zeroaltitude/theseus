//! `[lsp]` (L2, theseus-n88g.8): the language-server board and its tools,
//! off by default. A server starts lazily, at the first call for a file of
//! its language under a root, never on the start path; it stops when idle
//! for `idle_stop_mins`, at the daemon's stop, or when it gives no answer
//! within `request_timeout_secs`, and the next call starts it again.
//!
//! The servers are the built-in presets (`theseus_lsp::servers`), each with
//! the extensions it serves and the markers its root is found by. A file's
//! server is the first, in `[lsp.servers]`'s order of names then the
//! presets', whose extensions name the file's and whose program is
//! installed. `[lsp.servers.<name>]` changes a preset's `command`,
//! `extensions`, `roots` (its markers), or `settings`, or, with a command
//! and extensions, adds a server.

use std::collections::BTreeMap;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LspConfig {
    /// The `lsp.*` tools, and the servers they start.
    #[serde(default)]
    pub enabled: bool,
    /// A server unused this long stops; the next call starts it again.
    #[serde(default = "default_idle_stop_mins")]
    pub idle_stop_mins: f64,
    /// A request unanswered this long is cancelled, and its server stopped.
    #[serde(default = "default_request_timeout_secs")]
    pub request_timeout_secs: u64,
    /// Changes to the presets, and servers of the operator's own.
    #[serde(default)]
    pub servers: BTreeMap<String, LspServerConfig>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LspServerConfig {
    /// Its argv; the preset's when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<Vec<String>>,
    /// The file extensions it serves, without the dot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Vec<String>>,
    /// The files that mark a workspace root (`Cargo.toml`, `pyproject.toml`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roots: Option<Vec<String>>,
    /// What `workspace/configuration` answers from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<toml::Value>,
    /// Off: this preset is never started.
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

impl Default for LspServerConfig {
    fn default() -> Self {
        Self {
            command: None,
            extensions: None,
            roots: None,
            settings: None,
            enabled: true,
        }
    }
}
fn default_idle_stop_mins() -> f64 {
    10.0
}
fn default_request_timeout_secs() -> u64 {
    30
}

/// The longest a request may wait.
pub const MAX_REQUEST_TIMEOUT_SECS: u64 = 600;

impl Default for LspConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            idle_stop_mins: default_idle_stop_mins(),
            request_timeout_secs: default_request_timeout_secs(),
            servers: BTreeMap::new(),
        }
    }
}

impl LspConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if !(self.idle_stop_mins > 0.0 && self.idle_stop_mins.is_finite()) {
            bail!(
                "lsp.idle_stop_mins = {} must be more than 0",
                self.idle_stop_mins
            );
        }
        if !(1..=MAX_REQUEST_TIMEOUT_SECS).contains(&self.request_timeout_secs) {
            bail!(
                "lsp.request_timeout_secs = {} is outside 1 to {MAX_REQUEST_TIMEOUT_SECS}",
                self.request_timeout_secs
            );
        }
        for (name, s) in &self.servers {
            let preset = crate::lsp::preset(name).is_some();
            if !preset && (s.command.is_none() || s.extensions.is_none()) {
                bail!(
                    "lsp.servers.{name} is not a preset ({}), so it needs a command and extensions",
                    crate::lsp::PRESETS.join(", ")
                );
            }
            if s.command.as_ref().is_some_and(Vec::is_empty) {
                bail!("lsp.servers.{name}.command is empty");
            }
            if let Some(e) = s
                .extensions
                .iter()
                .flatten()
                .find(|e| e.is_empty() || e.starts_with('.'))
            {
                bail!("lsp.servers.{name}.extensions: {e:?} must be an extension without its dot");
            }
        }
        Ok(())
    }
}

/// The template's `[lsp]`, un-commented (`config.rs`'s
/// `example_template_uncommented_still_parses`): on, with the defaults, a
/// change to pyright's settings, and a server of the operator's own; as
/// written, with its lines commented, off.
#[cfg(test)]
pub(crate) fn the_templates_lsp_section(l: &LspConfig) {
    assert!(l.enabled);
    assert_eq!(l.idle_stop_mins, 10.0);
    assert_eq!(l.request_timeout_secs, 30);
    assert!(l.servers["pyright"].settings.is_some());
    assert_eq!(
        l.servers["some-server"].extensions.as_deref(),
        Some(&["some".to_string()][..])
    );
    l.validate().unwrap();
    assert!(!crate::Config::example().lsp.enabled);
    assert!(crate::Config::example().lsp.is_default());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(toml: &str) -> Result<crate::Config> {
        let base = crate::Config::EXAMPLE_TOML;
        Ok(crate::Config::from_toml(&format!("{base}\n{toml}"))?)
    }

    /// Off by default, every key with its default, and the checks.
    #[test]
    fn lsp_is_off_by_default_and_its_keys_are_checked() {
        let d = crate::Config::example().lsp;
        assert!(!d.enabled);
        assert_eq!((d.idle_stop_mins, d.request_timeout_secs), (10.0, 30));
        let cfg = parse("[lsp]\nenabled = true\n").unwrap();
        cfg.validate().unwrap();
        assert!(parse("[lsp]\nidle_stop = 1\n").is_err());
        assert!(parse("[lsp.servers.ty]\nargs = []\n").is_err());
        for bad in [
            "idle_stop_mins = 0.0",
            "request_timeout_secs = 0",
            "request_timeout_secs = 601",
            "[lsp.servers.mine]\ncommand = [\"mine\"]",
            "[lsp.servers.ty]\ncommand = []",
            "[lsp.servers.ty]\nextensions = [\".py\"]",
        ] {
            let cfg = parse(&format!("[lsp]\nenabled = true\n{bad}\n")).unwrap();
            assert!(cfg.validate().is_err(), "{bad}");
        }
        let cfg = parse(
            "[lsp]\nenabled = true\n[lsp.servers.mine]\ncommand = [\"mine\", \"--stdio\"]\nextensions = [\"mn\"]\n",
        )
        .unwrap();
        cfg.validate().unwrap();
    }
}
