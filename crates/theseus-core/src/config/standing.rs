//! `[standing]` (theseus-slvl.5): what the owner's words become.
//!
//! - `capture`: `directive.v1` reads the owner's own messages in a private
//!   place (the CLI, the web UI, an owner's DM) at the inbound point, in the
//!   request the other inbound packs ride, and keeps a standing preference,
//!   fact or correction it is sure of (with a notice and an undo), asks about
//!   one it is not (and about every order), and counts a message that says a
//!   live record again (true). False: the pack is not asked, nothing is kept
//!   or asked, and the records the owner added by hand stand as before.
//!   `theseus packs rollback directive.v1` puts the pack in shadow instead,
//!   where it still judges and records and acts on nothing.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StandingConfig {
    #[serde(default = "default_capture")]
    pub capture: bool,
}

fn default_capture() -> bool {
    true
}

impl Default for StandingConfig {
    fn default() -> Self {
        Self {
            capture: default_capture(),
        }
    }
}

impl StandingConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The template's `[standing]` is the default, and a note may turn the
    /// capture off.
    #[test]
    fn the_templates_standing_section_is_the_default() {
        let cfg = crate::Config::from_toml(crate::Config::EXAMPLE_TOML).unwrap();
        assert_eq!(cfg.standing, StandingConfig::default());
        assert!(cfg.standing.capture);
        assert!(crate::Config::EXAMPLE_TOML.contains("# [standing]\n# capture = true"));
        let off = crate::Config::from_toml("[standing]\ncapture = false\n").unwrap();
        assert!(!off.standing.capture);
        assert!(crate::Config::from_toml("[standing]\nbogus = 1\n").is_err());
    }
}
