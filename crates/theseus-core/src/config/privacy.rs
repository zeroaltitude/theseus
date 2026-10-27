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

use serde::{Deserialize, Serialize};
use theseus_judge::names::Names;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrivacyConfig {
    #[serde(default)]
    pub sensitive_names: Vec<String>,
}

impl PrivacyConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
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
}
