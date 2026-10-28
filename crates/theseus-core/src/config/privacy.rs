//! `[privacy]` (theseus-oejh3): the one list of names whose matter goes only
//! to a model of US origin (theseus-xs5ah). Route.v6's `data` question reads it
//! as the private names, the pack-text check at start reads it, and a floor
//! keeps a turn that names one off models of any other origin, `cn`, `eu` or
//! `unknown` (`ModelOrigin::cleared_for_sensitive`). The
//! matcher is `theseus_judge::names`. Every key has a default, so an owner's
//! config names none unless it differs.
//!
//! - `sensitive_names`: organisations, clients, projects and people, empty by
//!   default. Matching is case-insensitive on whole words.
//! - `open_source_paths`: directories whose work is public (the pack is told an item from there is public). Written with
//!   `~` as the owner would; read through `expanded_open_source_paths`. **Parsed and exposed only**: the labels rows (R3,
//!   R9) read it (theseus-gyjyo.2).
//!   Not `[places] public_paths`, which opens trees to a shared place's file tools; this one only tells the
//!   classifier.
//! - `redact_outbound`: the vault's values are substituted on copies that leave the machine, on by default. **Parsed
//!   and exposed only**: the record row (R7) reads it.

use serde::{Deserialize, Serialize};
use theseus_judge::names::Names;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrivacyConfig {
    #[serde(default)]
    pub sensitive_names: Vec<String>,
    #[serde(default)]
    pub open_source_paths: Vec<String>,
    #[serde(default = "on")]
    pub redact_outbound: bool,
}

fn on() -> bool {
    true
}

impl Default for PrivacyConfig {
    fn default() -> Self {
        Self {
            sensitive_names: Vec::new(),
            open_source_paths: Vec::new(),
            redact_outbound: on(),
        }
    }
}

impl PrivacyConfig {
    /// An entry must be absolute or start with `~/`, as `[places] public_paths`
    /// requires of its own list: a relative entry expands to a relative path and
    /// silently matches nothing.
    pub fn validate(&self) -> anyhow::Result<()> {
        if let Some(p) = self
            .open_source_paths
            .iter()
            .find(|p| !(p.starts_with('/') || p.starts_with("~/")))
        {
            anyhow::bail!(
                "privacy.open_source_paths entry {p:?} must be an absolute path or start with ~/"
            );
        }
        Ok(())
    }

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// `open_source_paths` with `~` and `$VAR` read as a shell reads them.
    pub fn expanded_open_source_paths(&self) -> Vec<std::path::PathBuf> {
        self.open_source_paths
            .iter()
            .map(|p| crate::config::expand(p))
            .collect()
    }

    /// The names, ready to match.
    pub fn names(&self) -> Names {
        Names::new(&self.sensitive_names)
    }

    /// The names as a pack's state carries them: the sensitive names as the
    /// private ones, and no open topics (`[routing.data]` is gone with the
    /// open view, theseus-9ymz0).
    pub fn data_names(&self) -> theseus_judge::builders::DataNames {
        theseus_judge::builders::DataNames {
            open: Vec::new(),
            private: self.sensitive_names.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_is_empty_by_default_and_parses() {
        assert!(PrivacyConfig::default().names().is_empty());
        let c: PrivacyConfig = toml::from_str("sensitive_names = [\"Acme\", \"Big Co\"]").unwrap();
        assert_eq!(c.names().hits("big co and ACME"), ["Acme", "Big Co"]);
        assert!(toml::from_str::<PrivacyConfig>("names = [\"x\"]").is_err());
    }

    /// `open_source_paths` is empty and `redact_outbound` on by default; each parses, and a path's `~` expands.
    #[test]
    fn open_source_paths_and_redact_outbound() {
        let d = PrivacyConfig::default();
        assert!(d.open_source_paths.is_empty() && d.redact_outbound && d.is_default());
        let c: PrivacyConfig = toml::from_str("").unwrap();
        assert_eq!(c, d);
        let c: PrivacyConfig = toml::from_str(
            "open_source_paths = [\"~/projects/theseus\", \"/srv/x\"]\nredact_outbound = false",
        )
        .unwrap();
        assert!(!c.redact_outbound && !c.is_default());
        assert_eq!(c.open_source_paths, ["~/projects/theseus", "/srv/x"]);
        assert!(
            PrivacyConfig {
                open_source_paths: vec!["projects/x".into()],
                ..Default::default()
            }
            .validate()
            .is_err(),
            "a relative entry is refused, as [places] public_paths refuses one"
        );
        let home = std::env::var("HOME").unwrap();
        assert_eq!(
            c.expanded_open_source_paths(),
            [
                std::path::PathBuf::from(format!("{home}/projects/theseus")),
                "/srv/x".into()
            ]
        );
    }
}
