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
//!
//! L3 (theseus-n88g.9): with `edit_diagnostics` (on), an edit's result
//! carries its files' errors, waiting up to `edit_wait_ms` for them, where
//! a server for the file's root is up; a server whose `start_on_edit` is on
//! is started for an edit too: on by default for ty, tsgo, and
//! rust-analyzer ([`crate::lsp::START_ON_EDIT`], theseus-ext.12), off for
//! the other presets and the operator's own servers unless set.

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
    /// An edit's result carries the errors its files' servers report (L3).
    #[serde(default = "yes")]
    pub edit_diagnostics: bool,
    /// How long an edit waits for them; a file the bound beats is pending.
    #[serde(default = "default_edit_wait_ms")]
    pub edit_wait_ms: u64,
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
    /// An edit of one of its files starts it, as a call would, when it is
    /// not up: judged at `proc.run`'s posture as any start is. Off: an edit
    /// carries diagnostics only from a server already up. Unset: on for the
    /// presets [`crate::lsp::START_ON_EDIT`] names, off for the rest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_on_edit: Option<bool>,
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
            start_on_edit: None,
        }
    }
}
fn default_idle_stop_mins() -> f64 {
    10.0
}
fn default_request_timeout_secs() -> u64 {
    30
}
fn default_edit_wait_ms() -> u64 {
    1500
}

/// The longest an edit may wait for its diagnostics.
pub const MAX_EDIT_WAIT_MS: u64 = 30_000;

/// The longest a request may wait.
pub const MAX_REQUEST_TIMEOUT_SECS: u64 = 600;

impl Default for LspConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            idle_stop_mins: default_idle_stop_mins(),
            request_timeout_secs: default_request_timeout_secs(),
            edit_diagnostics: true,
            edit_wait_ms: default_edit_wait_ms(),
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
        if !(1..=MAX_EDIT_WAIT_MS).contains(&self.edit_wait_ms) {
            bail!(
                "lsp.edit_wait_ms = {} is outside 1 to {MAX_EDIT_WAIT_MS}",
                self.edit_wait_ms
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
    assert!(l.edit_diagnostics);
    assert_eq!(l.edit_wait_ms, 1500);
    assert!(l.servers["pyright"].settings.is_some());
    assert_eq!(l.servers["pyright"].start_on_edit, Some(true));
    assert_eq!(l.servers["rust-analyzer"].start_on_edit, Some(false));
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
        assert!(d.edit_diagnostics);
        assert_eq!(d.edit_wait_ms, 1500);
        // Unset: on for ty, tsgo, and rust-analyzer, off for the rest.
        let starts: Vec<(String, bool)> = crate::lsp::Spec::all(&d)
            .into_iter()
            .map(|s| (s.name, s.start_on_edit))
            .collect();
        let on: Vec<&str> = starts
            .iter()
            .filter(|(_, on)| *on)
            .map(|(n, _)| n.as_str())
            .collect();
        assert_eq!(on, ["rust-analyzer", "ty", "tsgo"], "{starts:?}");
        assert_eq!(starts.len(), crate::lsp::PRESETS.len());
        let off =
            parse("[lsp]\nenabled = true\n[lsp.servers.rust-analyzer]\nstart_on_edit = false\n")
                .unwrap()
                .lsp;
        assert!(crate::lsp::Spec::all(&off)
            .iter()
            .all(|s| s.start_on_edit == (s.name == "ty" || s.name == "tsgo")));
        let cfg = parse("[lsp]\nenabled = true\n").unwrap();
        cfg.validate().unwrap();
        assert!(parse("[lsp]\nidle_stop = 1\n").is_err());
        assert!(parse("[lsp.servers.ty]\nargs = []\n").is_err());
        for bad in [
            "idle_stop_mins = 0.0",
            "request_timeout_secs = 0",
            "request_timeout_secs = 601",
            "edit_wait_ms = 0",
            "edit_wait_ms = 30001",
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
